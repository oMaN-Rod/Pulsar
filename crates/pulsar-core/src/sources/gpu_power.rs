//! GPU power in watts from NVIDIA's management library, which its driver
//! installs. The display driver itself reports power only as a share of the
//! card's limit, and other vendors offer watts only through their SDKs.
//!
//! A laptop's NVIDIA GPU sleeps while nothing uses it, and asking the library
//! anything can wake it. So a card that Windows reports powered down counts
//! as drawing nothing, and the library stays unloaded while every card sleeps.

use std::ffi::c_void;
use std::time::{Duration, Instant};

use windows::Win32::Devices::DeviceAndDriverInstallation::{
    CM_DRP_DEVICE_POWER_DATA, CM_Get_DevNode_Registry_PropertyW, CR_SUCCESS,
};
use windows::Win32::Foundation::{FreeLibrary, HMODULE};
use windows::Win32::System::LibraryLoader::{
    GetProcAddress, LOAD_LIBRARY_SEARCH_SYSTEM32, LoadLibraryExW,
};
use windows::core::{PCSTR, s, w};

use crate::hardware::pci;
use crate::metric::{MetricKey, Snapshot, Source, SourceError, SourceId};

const NVIDIA: u16 = 0x10DE;
/// How often the cards are listed again, to notice one that came or went.
const RELIST: Duration = Duration::from_secs(30);
/// How soon to load the library again after it failed.
const RELOAD: Duration = Duration::from_secs(30);
const NVML_SUCCESS: i32 = 0;
const NVML_ERROR_NOT_SUPPORTED: i32 = 3;
/// `DEVICE_POWER_STATE`: D3 and below are powered down.
const POWER_DEVICE_D3: u32 = 4;

/// A card's reading this tick.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CardPower {
    /// Powered down, so drawing next to nothing; not asked, as that could wake it.
    Asleep,
    Watts(f64),
    /// The card does not report its power.
    Unknown,
}

/// The total of the cards that report their power, with sleeping ones at
/// zero; `None` when none is asleep or reports it.
pub fn total_watts(cards: &[CardPower]) -> Option<f64> {
    cards.iter().fold(None, |total, card| match card {
        CardPower::Asleep => Some(total.unwrap_or(0.0)),
        CardPower::Watts(w) => Some(total.unwrap_or(0.0) + w),
        CardPower::Unknown => total,
    })
}

/// An NVIDIA GPU as Windows lists it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Card {
    bus: u32,
    device: u32,
    /// Its device node, for asking how powered it is.
    node: u32,
}

fn list_cards() -> Vec<Card> {
    pci::read()
        .into_iter()
        .filter(|d| d.ids.vendor == NVIDIA)
        .filter_map(|d| match d.location? {
            (bus, device, 0) => Some(Card {
                bus,
                device,
                node: d.node,
            }),
            _ => None,
        })
        .collect()
}

fn asleep(node: u32) -> bool {
    // `CM_POWER_DATA`: its size, then the most recent device power state.
    let mut data = [0u32; 14];
    let mut len = size_of_val(&data) as u32;
    let read = unsafe {
        CM_Get_DevNode_Registry_PropertyW(
            node,
            CM_DRP_DEVICE_POWER_DATA,
            None,
            Some(data.as_mut_ptr().cast()),
            &mut len,
            0,
        )
    };
    read == CR_SUCCESS && data[1] >= POWER_DEVICE_D3
}

type Device = *mut c_void;

/// `nvmlPciInfo_t`.
#[repr(C)]
struct PciInfo {
    bus_id_legacy: [u8; 16],
    domain: u32,
    bus: u32,
    device: u32,
    pci_device_id: u32,
    pci_subsystem_id: u32,
    bus_id: [u8; 32],
}

/// The library, loaded and started, with a handle per card.
struct Nvml {
    module: HMODULE,
    shutdown: unsafe extern "C" fn() -> i32,
    power_usage: unsafe extern "C" fn(Device, *mut u32) -> i32,
    /// Each card's handle, with its bus and device numbers.
    devices: Vec<(Device, u32, u32)>,
}

// The library's handles may be used from any thread.
unsafe impl Send for Nvml {}

impl Nvml {
    fn load() -> Option<Self> {
        // Only from System32, so a DLL beside a portable copy is never loaded.
        let module =
            unsafe { LoadLibraryExW(w!("nvml.dll"), None, LOAD_LIBRARY_SEARCH_SYSTEM32) }.ok()?;
        let nvml = unsafe { Self::start(module) };
        if nvml.is_none() {
            unsafe {
                let _ = FreeLibrary(module);
            }
        }
        nvml
    }

    unsafe fn start(module: HMODULE) -> Option<Self> {
        let find = |name: PCSTR| unsafe { GetProcAddress(module, name) };
        type Init = unsafe extern "C" fn() -> i32;
        type Count = unsafe extern "C" fn(*mut u32) -> i32;
        type ByIndex = unsafe extern "C" fn(u32, *mut Device) -> i32;
        type Pci = unsafe extern "C" fn(Device, *mut PciInfo) -> i32;
        type Power = unsafe extern "C" fn(Device, *mut u32) -> i32;
        unsafe {
            let init: Init = std::mem::transmute(find(s!("nvmlInit_v2"))?);
            let shutdown: Init = std::mem::transmute(find(s!("nvmlShutdown"))?);
            let count: Count = std::mem::transmute(find(s!("nvmlDeviceGetCount_v2"))?);
            let by_index: ByIndex = std::mem::transmute(find(s!("nvmlDeviceGetHandleByIndex_v2"))?);
            let pci: Pci = std::mem::transmute(find(s!("nvmlDeviceGetPciInfo_v3"))?);
            let power_usage: Power = std::mem::transmute(find(s!("nvmlDeviceGetPowerUsage"))?);
            if init() != NVML_SUCCESS {
                return None;
            }
            let mut n = 0;
            if count(&mut n) != NVML_SUCCESS {
                shutdown();
                return None;
            }
            let devices = (0..n)
                .filter_map(|i| {
                    let mut device: Device = std::ptr::null_mut();
                    let mut info: PciInfo = std::mem::zeroed();
                    (by_index(i, &mut device) == NVML_SUCCESS
                        && pci(device, &mut info) == NVML_SUCCESS)
                        .then_some((device, info.bus, info.device))
                })
                .collect();
            Some(Self {
                module,
                shutdown,
                power_usage,
                devices,
            })
        }
    }

    /// `None` when the library failed, rather than the card lacking a sensor.
    fn watts(&self, bus: u32, device: u32) -> Option<CardPower> {
        let Some(&(handle, ..)) = self
            .devices
            .iter()
            .find(|&&(_, b, d)| (b, d) == (bus, device))
        else {
            return Some(CardPower::Unknown);
        };
        let mut milliwatts = 0;
        match unsafe { (self.power_usage)(handle, &mut milliwatts) } {
            NVML_SUCCESS => Some(CardPower::Watts(f64::from(milliwatts) / 1000.0)),
            NVML_ERROR_NOT_SUPPORTED => Some(CardPower::Unknown),
            _ => None,
        }
    }
}

impl Drop for Nvml {
    fn drop(&mut self) {
        unsafe {
            (self.shutdown)();
            let _ = FreeLibrary(self.module);
        }
    }
}

#[derive(Default)]
pub struct GpuPowerSource {
    cards: Vec<Card>,
    listed: Option<Instant>,
    nvml: Option<Nvml>,
    failed: Option<Instant>,
}

impl GpuPowerSource {
    fn nvml(&mut self, now: Instant) -> Option<&Nvml> {
        let wait_over = self.failed.is_none_or(|t| now.duration_since(t) >= RELOAD);
        if self.nvml.is_none() && wait_over {
            self.nvml = Nvml::load();
            self.failed = self.nvml.is_none().then_some(now);
        }
        self.nvml.as_ref()
    }
}

impl Source for GpuPowerSource {
    fn id(&self) -> SourceId {
        SourceId::GpuPower
    }

    fn sample(&mut self, out: &mut Snapshot) -> Result<(), SourceError> {
        let now = Instant::now();
        if self.listed.is_none_or(|t| now.duration_since(t) >= RELIST) {
            let cards = list_cards();
            if cards != self.cards {
                // The library finds the cards once, when it starts.
                self.nvml = None;
                self.cards = cards;
            }
            self.listed = Some(now);
        }
        if self.cards.is_empty() {
            self.nvml = None;
            return Err(SourceError::Unavailable(
                crate::tr!("GPU power needs an NVIDIA GPU").into(),
            ));
        }
        let awake: Vec<Card> = self
            .cards
            .iter()
            .copied()
            .filter(|c| !asleep(c.node))
            .collect();
        if awake.is_empty() {
            // Unloading lets the driver forget that anything is watching.
            self.nvml = None;
        }
        let mut readings = vec![CardPower::Asleep; self.cards.len() - awake.len()];
        if !awake.is_empty() {
            let nvml = self.nvml(now).ok_or_else(unavailable)?;
            let read: Option<Vec<CardPower>> =
                awake.iter().map(|c| nvml.watts(c.bus, c.device)).collect();
            let Some(read) = read else {
                // A driver update or a reset leaves the handles stale.
                self.nvml = None;
                self.failed = Some(now);
                return Err(unavailable());
            };
            readings.extend(read);
        }
        let watts = total_watts(&readings).ok_or_else(|| {
            SourceError::Unavailable(crate::tr!("This GPU does not report its power").into())
        })?;
        out.set(MetricKey::GpuPowerW, watts);
        Ok(())
    }
}

fn unavailable() -> SourceError {
    SourceError::Unavailable(crate::tr!("GPU power unavailable").into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sleeping_cards_draw_nothing_and_silent_ones_are_left_out() {
        use CardPower::*;
        assert_eq!(total_watts(&[Watts(30.5), Watts(4.5)]), Some(35.0));
        assert_eq!(total_watts(&[Asleep]), Some(0.0));
        assert_eq!(total_watts(&[Asleep, Watts(12.0)]), Some(12.0));
        assert_eq!(total_watts(&[Unknown, Watts(12.0)]), Some(12.0));
        assert_eq!(total_watts(&[Unknown]), None);
        assert_eq!(total_watts(&[]), None);
    }

    #[test]
    fn live_power_reads_or_is_cleanly_unavailable() {
        let mut s = Snapshot::default();
        match GpuPowerSource::default().sample(&mut s) {
            Ok(()) => assert!((0.0..1000.0).contains(&s.get(MetricKey::GpuPowerW).unwrap())),
            Err(e) => assert!(
                [
                    "GPU power needs an NVIDIA GPU",
                    "This GPU does not report its power",
                    "GPU power unavailable",
                ]
                .contains(&e.to_string().as_str()),
                "{e}"
            ),
        }
    }
}
