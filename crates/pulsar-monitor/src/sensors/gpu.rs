//! NVIDIA temperatures the display driver does not report: the hot spot and
//! the memory chips.

use std::time::{Duration, Instant};

use pulsar_core::hardware::pci::powered_down;
use pulsar_core::sensors::Sensor;
use windows::Win32::Devices::DeviceAndDriverInstallation::{
    DIGCF_PRESENT, GUID_DEVCLASS_DISPLAY, SETUP_DI_REGISTRY_PROPERTY, SP_DEVINFO_DATA,
    SPDRP_ADDRESS, SPDRP_BUSNUMBER, SPDRP_HARDWAREID, SetupDiDestroyDeviceInfoList,
    SetupDiEnumDeviceInfo, SetupDiGetClassDevsW, SetupDiGetDeviceRegistryPropertyW,
};

use crate::pawnio::PawnIo;

const MODULE: &[u8] = include_bytes!("../../resources/pawnio/Nvidia.bin");
const THERMAL_CHANNELS: usize = 6;
/// A topology word, a count, then one temperature per possible sensor.
const MEMORY_WORDS: usize = 50;

fn is_nvidia(hardware_id: &str) -> bool {
    hardware_id.to_ascii_uppercase().contains("VEN_10DE")
}

/// Bus, device and function from the two numbers Windows keeps for a PCI device.
fn location(bus: u32, address: u32) -> [u64; 3] {
    [
        u64::from(bus),
        u64::from(address >> 16),
        u64::from(address & 0xFFFF),
    ]
}

/// The first thermal channel is the hot spot; it is usable only when flagged.
fn hot_spot(channels: &[u64]) -> Option<f64> {
    let raw = *channels.first()?;
    (raw & (1 << 30) != 0).then(|| (raw & 0xFFFF) as f64 / 256.0)
}

/// The hottest memory chip; positions without a sensor hold a sentinel.
fn memory_temp(words: &[u64]) -> Option<f64> {
    words
        .get(2..)?
        .iter()
        .map(|&w| w as i64)
        .filter(|t| (1..=150).contains(t))
        .max()
        .map(|t| t as f64)
}

fn property(
    set: windows::Win32::Devices::DeviceAndDriverInstallation::HDEVINFO,
    device: &SP_DEVINFO_DATA,
    which: SETUP_DI_REGISTRY_PROPERTY,
) -> Option<Vec<u8>> {
    let mut buffer = vec![0u8; 2048];
    let mut used = 0u32;
    unsafe {
        SetupDiGetDeviceRegistryPropertyW(
            set,
            device,
            which,
            None,
            Some(&mut buffer),
            Some(&mut used),
        )
    }
    .ok()?;
    buffer.truncate(used as usize);
    Some(buffer)
}

fn number(bytes: Vec<u8>) -> Option<u32> {
    Some(u32::from_le_bytes(bytes.get(..4)?.try_into().ok()?))
}

fn text(bytes: Vec<u8>) -> String {
    let units: Vec<u16> = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|&pair| u16::from_le_bytes(pair))
        .collect();
    String::from_utf16_lossy(&units)
}

/// Where the first NVIDIA display adapter sits on the PCI bus, and its
/// device node.
fn find_nvidia() -> Option<([u64; 3], u32)> {
    let set =
        unsafe { SetupDiGetClassDevsW(Some(&GUID_DEVCLASS_DISPLAY), None, None, DIGCF_PRESENT) }
            .ok()?;
    let mut found = None;
    for index in 0.. {
        let mut device = SP_DEVINFO_DATA {
            cbSize: size_of::<SP_DEVINFO_DATA>() as u32,
            ..Default::default()
        };
        if unsafe { SetupDiEnumDeviceInfo(set, index, &mut device) }.is_err() {
            break;
        }
        let nvidia =
            property(set, &device, SPDRP_HARDWAREID).is_some_and(|id| is_nvidia(&text(id)));
        let bus = property(set, &device, SPDRP_BUSNUMBER).and_then(number);
        let address = property(set, &device, SPDRP_ADDRESS).and_then(number);
        if let (true, Some(bus), Some(address)) = (nvidia, bus, address) {
            found = Some((location(bus, address), device.DevInst));
            break;
        }
    }
    unsafe {
        let _ = SetupDiDestroyDeviceInfoList(set);
    }
    found
}

/// The NVIDIA module on its own handle, and which of its readings this
/// card has turned out to support.
pub struct Gpu {
    io: PawnIo,
    location: [u64; 3],
    node: u32,
    has_hot_spot: bool,
    has_memory_temp: bool,
    searched: Option<Instant>,
}

const SEARCH_AGAIN: Duration = Duration::from_secs(30);

impl Gpu {
    /// `None` without an NVIDIA card.
    pub fn connect() -> Option<Self> {
        let (location, node) = find_nvidia()?;
        let io = PawnIo::open().ok()?;
        io.load(MODULE).ok()?;
        Some(Self {
            io,
            location,
            node,
            has_hot_spot: true,
            has_memory_temp: true,
            searched: None,
        })
    }

    /// Windows hands out a new device node when it lists the card again.
    fn find_again(&mut self) {
        let now = Instant::now();
        if self
            .searched
            .is_some_and(|t| now.duration_since(t) < SEARCH_AGAIN)
        {
            return;
        }
        self.searched = Some(now);
        if let Some((location, node)) = find_nvidia() {
            if location != self.location {
                self.has_hot_spot = true;
                self.has_memory_temp = true;
            }
            self.location = location;
            self.node = node;
        }
    }

    /// A reading the card refuses once is not asked for again. A laptop's
    /// card that is powered down, or whose state is unknown, is left alone:
    /// reading its registers would wake it.
    pub fn sample(&mut self) -> Vec<(Sensor, f64)> {
        let mut values = Vec::new();
        let mut state = powered_down(self.node);
        if state.is_none() {
            self.find_again();
            state = powered_down(self.node);
        }
        if state != Some(false) {
            return values;
        }
        if self.has_hot_spot {
            let mut channels = [0u64; THERMAL_CHANNELS];
            let read = self.io.execute(
                "ioctl_read_thermal_registers",
                &self.location,
                &mut channels,
            );
            self.has_hot_spot = read.is_ok();
            values.extend(hot_spot(&channels).map(|v| (Sensor::GpuHotSpot, v)));
        }
        if self.has_memory_temp {
            let mut words = [0u64; MEMORY_WORDS];
            let read =
                self.io
                    .execute("ioctl_read_memory_temperatures", &self.location, &mut words);
            self.has_memory_temp = read.is_ok();
            values.extend(memory_temp(&words).map(|v| (Sensor::GpuMemoryTemp, v)));
        }
        values
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nvidia_is_recognised_by_its_pci_vendor() {
        assert!(is_nvidia(r"PCI\VEN_10DE&DEV_2484&SUBSYS_146B10DE&REV_A1"));
        assert!(is_nvidia(r"pci\ven_10de&dev_2484"));
        assert!(!is_nvidia(r"PCI\VEN_8086&DEV_A780"));
    }

    #[test]
    fn the_address_splits_into_device_and_function() {
        assert_eq!(location(1, 0), [1, 0, 0]);
        assert_eq!(location(3, 0x0002_0001), [3, 2, 1]);
    }

    #[test]
    fn the_hot_spot_needs_its_valid_flag() {
        // 71.5 °C in 1/256 steps.
        let raw = (1 << 30) | (71 * 256 + 128);
        assert_eq!(hot_spot(&[raw, 0, 0, 0, 0, 0]), Some(71.5));
        assert_eq!(hot_spot(&[71 * 256, 0, 0, 0, 0, 0]), None);
        assert_eq!(hot_spot(&[]), None);
    }

    #[test]
    fn memory_temperature_is_the_hottest_chip_with_a_sensor() {
        let missing = i64::from(i32::MIN) as u64;
        let mut words = [missing; MEMORY_WORDS];
        words[0] = 0x0040_0000;
        words[1] = 48;
        assert_eq!(memory_temp(&words), None, "the header is not a reading");
        words[2] = 62;
        words[9] = 78;
        words[10] = 0;
        assert_eq!(memory_temp(&words), Some(78.0));
    }

    #[test]
    fn looking_for_a_card_never_fails_hard() {
        if let Some(([bus, device, function], _)) = find_nvidia() {
            assert!(bus <= 0xFF && device <= 0x1F && function <= 7);
        }
    }
}
