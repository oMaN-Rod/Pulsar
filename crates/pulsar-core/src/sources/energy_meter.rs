//! CPU power from the Windows Energy Meter Interface (EMI), which exposes
//! the RAPL energy counters on many machines without a driver of our own.

use windows::Win32::Devices::DeviceAndDriverInstallation::{
    DIGCF_DEVICEINTERFACE, DIGCF_PRESENT, SP_DEVICE_INTERFACE_DATA,
    SP_DEVICE_INTERFACE_DETAIL_DATA_W, SetupDiDestroyDeviceInfoList, SetupDiEnumDeviceInterfaces,
    SetupDiGetClassDevsW, SetupDiGetDeviceInterfaceDetailW,
};
use windows::Win32::Foundation::{CloseHandle, GENERIC_READ, HANDLE};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows::Win32::System::IO::DeviceIoControl;
use windows::core::{GUID, PCWSTR};

use crate::metric::{MetricKey, Snapshot, Source, SourceError, SourceId};

const GUID_DEVICE_ENERGY_METER: GUID = GUID::from_u128(0x45bd8344_7ed6_49cf_a440_c276c933b053);
const IOCTL_EMI_GET_VERSION: u32 = 0x0022_4000;
const IOCTL_EMI_GET_METADATA_SIZE: u32 = 0x0022_4004;
const IOCTL_EMI_GET_METADATA: u32 = 0x0022_4008;
const IOCTL_EMI_GET_MEASUREMENT: u32 = 0x0022_400C;
const EMI_VERSION_V1: u16 = 1;
const EMI_VERSION_V2: u16 = 2;
/// `EMI_METADATA_V1`: unit, OEM and model names, revision, then the name size.
const V1_NAME_SIZE_AT: usize = 4 + 32 + 32 + 2;
/// `EMI_METADATA_V2`: OEM and model names, revision and channel count, then
/// the channels; unlike V1 it has no unit of its own.
const V2_CHANNELS_AT: usize = 32 + 32 + 2 + 2;
const PICOWATT_HOURS_TO_JOULES: f64 = 3.6e-9;
/// `AbsoluteTime` is in 100 ns units.
const TICKS_PER_SECOND: f64 = 1e7;

/// What a RAPL channel measures, from the end of its name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Channel {
    Package,
    Cores,
    Graphics,
    Dram,
}

impl Channel {
    pub fn from_name(name: &str) -> Option<Self> {
        let upper = name.to_ascii_uppercase();
        if !upper.starts_with("RAPL_") {
            return None;
        }
        match upper.rsplit('_').next()? {
            "PKG" => Some(Channel::Package),
            "PP0" => Some(Channel::Cores),
            "PP1" => Some(Channel::Graphics),
            "DRAM" => Some(Channel::Dram),
            _ => None,
        }
    }

    fn key(self) -> MetricKey {
        match self {
            Channel::Package => MetricKey::CpuPowerW,
            Channel::Cores => MetricKey::CpuCoresPowerW,
            Channel::Graphics => MetricKey::CpuGraphicsPowerW,
            Channel::Dram => MetricKey::CpuDramPowerW,
        }
    }
}

/// Energy in picowatt-hours and time in 100 ns ticks.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Measurement {
    pub energy: u64,
    pub time: u64,
}

/// Average watts between two measurements of one channel.
pub fn watts(before: Measurement, after: Measurement) -> Option<f64> {
    let energy = after.energy.checked_sub(before.energy)?;
    let ticks = after.time.checked_sub(before.time).filter(|&t| t > 0)?;
    Some(energy as f64 * PICOWATT_HOURS_TO_JOULES / (ticks as f64 / TICKS_PER_SECOND))
}

/// Channel names from `EMI_METADATA_V2`, in measurement order.
pub fn v2_channel_names(metadata: &[u8]) -> Vec<String> {
    let count = metadata
        .get(V2_CHANNELS_AT - 2..V2_CHANNELS_AT)
        .map_or(0, |b| u16::from_le_bytes([b[0], b[1]]));
    let mut names = Vec::new();
    let mut at = V2_CHANNELS_AT;
    for _ in 0..count {
        // `EMI_CHANNEL_V2`: a unit, the name's size in bytes, then the name.
        let Some(size) = metadata
            .get(at + 4..at + 6)
            .map(|b| u16::from_le_bytes([b[0], b[1]]) as usize)
        else {
            break;
        };
        let Some(bytes) = metadata.get(at + 6..at + 6 + size) else {
            break;
        };
        names.push(utf16_name(bytes));
        at += 6 + size;
    }
    names
}

fn v1_name(metadata: &[u8]) -> Option<String> {
    let size = metadata
        .get(V1_NAME_SIZE_AT..V1_NAME_SIZE_AT + 2)
        .map(|b| u16::from_le_bytes([b[0], b[1]]) as usize)?;
    let start = V1_NAME_SIZE_AT + 2;
    metadata.get(start..start + size).map(utf16_name)
}

fn utf16_name(bytes: &[u8]) -> String {
    let words: Vec<u16> = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|&c| u16::from_le_bytes(c))
        .take_while(|&w| w != 0)
        .collect();
    String::from_utf16_lossy(&words)
}

/// One energy meter device and the RAPL channels it reports.
pub struct Meter {
    handle: HANDLE,
    version: u16,
    /// Each measurement slot's channel, if it is one we show.
    channels: Vec<Option<Channel>>,
}

// The handle is only used from the sampler thread that owns the source.
unsafe impl Send for Meter {}

impl Drop for Meter {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.handle);
        }
    }
}

impl Meter {
    /// Every present meter that reports at least one RAPL channel.
    pub fn open_all() -> Vec<Meter> {
        interface_paths()
            .into_iter()
            .filter_map(|path| Meter::open(&path))
            .collect()
    }

    fn open(path: &[u16]) -> Option<Meter> {
        let handle = unsafe {
            CreateFileW(
                PCWSTR(path.as_ptr()),
                GENERIC_READ.0,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                None,
                OPEN_EXISTING,
                FILE_ATTRIBUTE_NORMAL,
                None,
            )
        }
        .ok()?;
        let mut meter = Meter {
            handle,
            version: 0,
            channels: Vec::new(),
        };
        let mut version = [0u8; 2];
        meter.control(IOCTL_EMI_GET_VERSION, &mut version)?;
        meter.version = u16::from_le_bytes(version);
        let mut size = [0u8; 4];
        meter.control(IOCTL_EMI_GET_METADATA_SIZE, &mut size)?;
        let mut metadata = vec![0u8; u32::from_le_bytes(size) as usize];
        meter.control(IOCTL_EMI_GET_METADATA, &mut metadata)?;
        let names = match meter.version {
            EMI_VERSION_V1 => v1_name(&metadata).into_iter().collect(),
            EMI_VERSION_V2 => v2_channel_names(&metadata),
            _ => Vec::new(),
        };
        log::debug!("energy meter v{} channels: {names:?}", meter.version);
        meter.channels = names.iter().map(|n| Channel::from_name(n)).collect();
        meter.channels.iter().any(Option::is_some).then_some(meter)
    }

    /// The counters of every channel, in measurement order.
    pub fn measure(&self) -> Option<Vec<Measurement>> {
        let mut buffer = vec![0u8; 16 * self.channels.len()];
        let returned = self.control(IOCTL_EMI_GET_MEASUREMENT, &mut buffer)?;
        let read = |at: usize| {
            buffer
                .get(at..at + 8)
                .map(|b| u64::from_le_bytes(b.try_into().unwrap_or_default()))
        };
        (0..self.channels.len())
            .filter(|i| 16 * i + 16 <= returned)
            .map(|i| {
                Some(Measurement {
                    energy: read(16 * i)?,
                    time: read(16 * i + 8)?,
                })
            })
            .collect()
    }

    fn control(&self, code: u32, output: &mut [u8]) -> Option<usize> {
        let mut returned = 0u32;
        unsafe {
            DeviceIoControl(
                self.handle,
                code,
                None,
                0,
                Some(output.as_mut_ptr().cast()),
                output.len() as u32,
                Some(&mut returned),
                None,
            )
        }
        .ok()?;
        Some(returned as usize)
    }
}

/// Fills CPU power from the energy meters when nothing earlier did.
#[derive(Default)]
pub struct EnergyMeterSource {
    meters: Option<Vec<Meter>>,
    last: Vec<Option<Vec<Measurement>>>,
}

impl Source for EnergyMeterSource {
    fn id(&self) -> SourceId {
        SourceId::EnergyMeter
    }

    fn sample(&mut self, out: &mut Snapshot) -> Result<(), SourceError> {
        if out.get(MetricKey::CpuPowerW).is_some() {
            self.last.clear();
            return Ok(());
        }
        let meters = self.meters.get_or_insert_with(Meter::open_all);
        self.last.resize(meters.len(), None);
        for (meter, last) in meters.iter().zip(self.last.iter_mut()) {
            let now = meter.measure();
            if let (Some(before), Some(after)) = (last.as_ref(), now.as_ref()) {
                for ((channel, b), a) in meter.channels.iter().zip(before).zip(after) {
                    if let (Some(channel), Some(w)) = (channel, watts(*b, *a)) {
                        out.set(channel.key(), w);
                    }
                }
            }
            *last = now;
        }
        Ok(())
    }
}

fn interface_paths() -> Vec<Vec<u16>> {
    let mut paths = Vec::new();
    unsafe {
        let Ok(set) = SetupDiGetClassDevsW(
            Some(&GUID_DEVICE_ENERGY_METER),
            PCWSTR::null(),
            None,
            DIGCF_PRESENT | DIGCF_DEVICEINTERFACE,
        ) else {
            return paths;
        };
        for index in 0.. {
            let mut data = SP_DEVICE_INTERFACE_DATA {
                cbSize: size_of::<SP_DEVICE_INTERFACE_DATA>() as u32,
                ..Default::default()
            };
            if SetupDiEnumDeviceInterfaces(set, None, &GUID_DEVICE_ENERGY_METER, index, &mut data)
                .is_err()
            {
                break;
            }
            let mut needed = 0u32;
            let _ = SetupDiGetDeviceInterfaceDetailW(set, &data, None, 0, Some(&mut needed), None);
            // u32 words keep the buffer aligned for the detail struct.
            let mut buffer = vec![0u32; (needed as usize).div_ceil(4).max(2)];
            let detail = buffer
                .as_mut_ptr()
                .cast::<SP_DEVICE_INTERFACE_DETAIL_DATA_W>();
            (*detail).cbSize = size_of::<SP_DEVICE_INTERFACE_DETAIL_DATA_W>() as u32;
            if SetupDiGetDeviceInterfaceDetailW(set, &data, Some(detail), needed, None, None)
                .is_err()
            {
                continue;
            }
            let start = std::ptr::addr_of!((*detail).DevicePath).cast::<u16>();
            let len = (0..).take_while(|&i| *start.add(i) != 0).count();
            let mut path = std::slice::from_raw_parts(start, len).to_vec();
            path.push(0);
            paths.push(path);
        }
        let _ = SetupDiDestroyDeviceInfoList(set);
    }
    paths
}

#[cfg(test)]
mod tests {
    use super::*;

    fn utf16(s: &str) -> Vec<u8> {
        s.encode_utf16()
            .chain([0])
            .flat_map(u16::to_le_bytes)
            .collect()
    }

    #[test]
    fn rapl_names_map_to_channels() {
        assert_eq!(
            Channel::from_name("RAPL_Package0_PKG"),
            Some(Channel::Package)
        );
        assert_eq!(
            Channel::from_name("RAPL_Package0_PP0"),
            Some(Channel::Cores)
        );
        assert_eq!(
            Channel::from_name("RAPL_Package0_PP1"),
            Some(Channel::Graphics)
        );
        assert_eq!(
            Channel::from_name("RAPL_Package0_DRAM"),
            Some(Channel::Dram)
        );
        assert_eq!(Channel::from_name("RAPL_Package0_PSYS"), None);
        assert_eq!(Channel::from_name("Battery_PKG"), None);
    }

    #[test]
    fn v2_metadata_lists_channels_in_order() {
        let mut metadata = vec![0u8; V2_CHANNELS_AT - 2];
        metadata.extend(2u16.to_le_bytes());
        for name in ["RAPL_Package0_PKG", "RAPL_Package0_PP0"] {
            let bytes = utf16(name);
            metadata.extend(0u32.to_le_bytes());
            metadata.extend((bytes.len() as u16).to_le_bytes());
            metadata.extend(bytes);
        }
        assert_eq!(
            v2_channel_names(&metadata),
            ["RAPL_Package0_PKG", "RAPL_Package0_PP0"]
        );
        assert!(v2_channel_names(&metadata[..V2_CHANNELS_AT + 3]).is_empty());
    }

    #[test]
    fn watts_from_picowatt_hours_over_time() {
        let before = Measurement { energy: 0, time: 0 };
        // 10 J over one second: 10 / 3.6e-9 pWh.
        let after = Measurement {
            energy: (10.0 / PICOWATT_HOURS_TO_JOULES) as u64,
            time: 10_000_000,
        };
        assert!((watts(before, after).unwrap() - 10.0).abs() < 1e-6);
        assert_eq!(watts(after, before), None, "a counter reset");
        assert_eq!(watts(before, before), None, "no time passed");
    }

    #[test]
    #[ignore = "reads this machine's energy meters"]
    fn source_fills_power_from_the_second_sample() {
        let mut source = EnergyMeterSource::default();
        let mut first = Snapshot::default();
        source.sample(&mut first).unwrap();
        assert_eq!(first.get(MetricKey::CpuPowerW), None);
        std::thread::sleep(std::time::Duration::from_millis(500));
        let mut second = Snapshot::default();
        source.sample(&mut second).unwrap();
        println!("{:?} W", second.get(MetricKey::CpuPowerW));
        assert!(second.get(MetricKey::CpuPowerW).is_some_and(|w| w > 0.0));
    }

    #[test]
    #[ignore = "reads this machine's energy meters"]
    fn probe() {
        let meters = Meter::open_all();
        println!("{} meter(s)", meters.len());
        for meter in &meters {
            let before = meter.measure().unwrap_or_default();
            std::thread::sleep(std::time::Duration::from_millis(500));
            let after = meter.measure().unwrap_or_default();
            for ((channel, b), a) in meter.channels.iter().zip(&before).zip(&after) {
                println!("{channel:?}: {:?} W", watts(*b, *a));
            }
        }
    }
}
