//! GPU power in watts from NVIDIA's management library, which its driver
//! installs. The display driver itself reports power only as a share of the
//! card's limit, and other vendors offer watts only through their SDKs.
//!
//! A laptop's NVIDIA GPU sleeps while nothing uses it, and asking the library
//! about a card wakes it. So a card that Windows reports powered down counts
//! as drawing nothing and is never asked, and a card's handle is only fetched
//! while it is awake. Once loaded, the library stays: freeing it gives back
//! none of its memory, and shutting it down wakes the card.

use std::collections::{HashMap, HashSet};
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
const NVML_ERROR_NOT_FOUND: i32 = 6;
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

/// The cards' total; `None` when none is asleep or reports its power.
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

/// `None` when Windows cannot say, as for a device node gone stale.
fn asleep(node: u32) -> Option<bool> {
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
    (read == CR_SUCCESS).then_some(data[1] >= POWER_DEVICE_D3)
}

type Device = *mut c_void;

struct Nvml {
    module: HMODULE,
    shutdown: unsafe extern "C" fn() -> i32,
    by_bus_id: unsafe extern "C" fn(PCSTR, *mut Device) -> i32,
    power_usage: unsafe extern "C" fn(Device, *mut u32) -> i32,
    handles: HashMap<(u32, u32), Device>,
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
        type ByBusId = unsafe extern "C" fn(PCSTR, *mut Device) -> i32;
        type Power = unsafe extern "C" fn(Device, *mut u32) -> i32;
        unsafe {
            let init: Init = std::mem::transmute(find(s!("nvmlInit_v2"))?);
            let shutdown: Init = std::mem::transmute(find(s!("nvmlShutdown"))?);
            let by_bus_id: ByBusId =
                std::mem::transmute(find(s!("nvmlDeviceGetHandleByPciBusId_v2"))?);
            let power_usage: Power = std::mem::transmute(find(s!("nvmlDeviceGetPowerUsage"))?);
            if init() != NVML_SUCCESS {
                return None;
            }
            Some(Self {
                module,
                shutdown,
                by_bus_id,
                power_usage,
                handles: HashMap::new(),
            })
        }
    }

    /// Fetching a handle starts that card, so only an awake card's is asked
    /// for; enumerating by index would start every card.
    fn handle(&mut self, card: Card) -> Result<Device, i32> {
        if let Some(&handle) = self.handles.get(&(card.bus, card.device)) {
            return Ok(handle);
        }
        let id = format!("00000000:{:02X}:{:02X}.0\0", card.bus, card.device);
        let mut handle: Device = std::ptr::null_mut();
        match unsafe { (self.by_bus_id)(PCSTR(id.as_ptr()), &mut handle) } {
            NVML_SUCCESS => {
                self.handles.insert((card.bus, card.device), handle);
                Ok(handle)
            }
            e => Err(e),
        }
    }

    /// `None` when the library failed, rather than the card.
    fn watts(&mut self, card: Card) -> Option<Read> {
        let handle = match self.handle(card) {
            Ok(handle) => handle,
            Err(NVML_ERROR_NOT_FOUND) => return Some(Read::NotFound),
            Err(_) => return None,
        };
        let mut milliwatts = 0;
        match unsafe { (self.power_usage)(handle, &mut milliwatts) } {
            NVML_SUCCESS => Some(Read::Watts(f64::from(milliwatts) / 1000.0)),
            NVML_ERROR_NOT_SUPPORTED => Some(Read::NoSensor),
            _ => None,
        }
    }
}

enum Read {
    Watts(f64),
    /// The card has no power sensor, which stays true while it sleeps.
    NoSensor,
    NotFound,
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
    no_sensor: HashSet<(u32, u32)>,
}

impl GpuPowerSource {
    fn nvml(&mut self, now: Instant) -> Option<&mut Nvml> {
        let wait_over = self.failed.is_none_or(|t| now.duration_since(t) >= RELOAD);
        if self.nvml.is_none() && wait_over {
            self.nvml = Nvml::load();
            self.failed = self.nvml.is_none().then_some(now);
        }
        self.nvml.as_mut()
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
                // Another card may now sit at a sensorless card's address.
                self.no_sensor.clear();
            }
            self.cards = cards;
            self.listed = Some(now);
        }
        if self.cards.is_empty() {
            self.nvml = None;
            return Err(SourceError::Unavailable(
                crate::tr!("GPU power needs an NVIDIA GPU").into(),
            ));
        }
        let mut readings = Vec::with_capacity(self.cards.len());
        let mut awake = Vec::new();
        for card in self.cards.clone() {
            let state = asleep(card.node);
            match state {
                _ if self.no_sensor.contains(&(card.bus, card.device)) => {
                    readings.push(CardPower::Unknown);
                }
                Some(true) => readings.push(CardPower::Asleep),
                Some(false) => awake.push(card),
                None => readings.push(CardPower::Unknown),
            }
            if state.is_none() {
                // Not asked, as it may be asleep; its node may be stale.
                self.listed = None;
            }
        }
        if !awake.is_empty() {
            let nvml = self.nvml(now).ok_or_else(unavailable)?;
            let read: Option<Vec<Read>> = awake.iter().map(|&c| nvml.watts(c)).collect();
            let Some(read) = read else {
                // A driver update or a reset leaves the handles stale.
                self.nvml = None;
                self.failed = Some(now);
                return Err(unavailable());
            };
            for (card, read) in awake.iter().zip(read) {
                readings.push(match read {
                    Read::Watts(w) => CardPower::Watts(w),
                    Read::NoSensor => {
                        self.no_sensor.insert((card.bus, card.device));
                        CardPower::Unknown
                    }
                    Read::NotFound => CardPower::Unknown,
                });
            }
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
