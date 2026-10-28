//! The shared memory through which the elevated sensor service hands
//! readings to the unelevated app. It carries values one way only. The app
//! sends the service a single bit: an event it sets while it wants frame
//! rates, which the service clears whenever it looks.

use std::sync::atomic::{AtomicI64, AtomicU32, AtomicU64, Ordering};

use windows::Win32::Foundation::{
    CloseHandle, HANDLE, HLOCAL, INVALID_HANDLE_VALUE, LocalFree, WAIT_OBJECT_0,
};
use windows::Win32::Security::Authorization::{
    ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows::Win32::Security::{PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES};
use windows::Win32::System::Memory::{
    CreateFileMappingW, FILE_MAP, FILE_MAP_READ, FILE_MAP_WRITE, MEMORY_MAPPED_VIEW_ADDRESS,
    MapViewOfFile, OpenFileMappingW, PAGE_READWRITE, UnmapViewOfFile,
};
use windows::Win32::System::SystemInformation::GetTickCount64;
use windows::Win32::System::Threading::{
    CreateEventW, EVENT_MODIFY_STATE, OpenEventW, ResetEvent, SetEvent, WaitForSingleObject,
};
use windows::core::{HSTRING, PCWSTR, w};

pub const SERVICE_NAME: &str = "PulsarSensors";
const MAPPING_NAME: &str = r"Global\Pulsar.Sensors";
const FRAMES_EVENT_NAME: &str = r"Global\Pulsar.FramesWanted";
const MAGIC: u32 = 0x534E_5350;
const VERSION: u32 = 7;
/// A reading older than this means the service has stopped.
const STALE_MS: u64 = 5000;
/// The service and administrators own the section; everyone else reads it.
const SECURITY: PCWSTR = w!("D:(A;;GA;;;SY)(A;;GA;;;BA)(A;;GR;;;AU)");
/// Signed-in users may only set the event; waiting on it is the service's.
const FRAMES_EVENT_SECURITY: PCWSTR = w!("D:(A;;GA;;;SY)(A;;GA;;;BA)(A;;0x2;;;AU)");
/// Processes whose frame rates the section holds at once.
pub const FRAME_SLOTS: usize = 16;
/// Displays whose update rates the section holds at once.
pub const DISPLAY_SLOTS: usize = 16;
/// More slots than sensors, so adding one keeps the layout.
const SENSOR_SLOTS: usize = 64;
/// Each per-core reading follows the sensors, one slot a core.
const CORE_SLOTS: usize = 256;
const SLOTS: usize = SENSOR_SLOTS + PerCore::ALL.len() * CORE_SLOTS;
const MISSING: i64 = i64::MIN;

/// A slot in the section. Values are thousandths of the sensor's unit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sensor {
    /// °C.
    CpuTemp = 0,
    /// Watts.
    CpuPower = 1,
    /// 1 while the CPU is slowing itself down to cool off, else 0.
    CpuThrottling = 2,
    /// °C.
    GpuHotSpot = 3,
    /// °C, the hottest memory chip.
    GpuMemoryTemp = 4,
    /// Watts drawn by the cores alone.
    CpuCoresPower = 5,
    /// Watts drawn by the integrated graphics.
    CpuGraphicsPower = 6,
    /// Watts drawn by the memory, where the CPU measures it.
    CpuDramPower = 7,
    /// °C, the platform controller hub.
    ChipsetTemp = 8,
    /// Watts the CPU is designed to dissipate.
    CpuTdp = 9,
    /// Watts the CPU may draw indefinitely.
    CpuPowerLimit1 = 10,
    /// Watts the CPU may draw in bursts.
    CpuPowerLimit2 = 11,
    /// The lowest and highest multipliers of the bus clock.
    CpuMinRatio = 12,
    CpuMaxRatio = 13,
    /// MHz of the ring that links the cores and the last-level cache.
    RingClock = 14,
    /// MHz of the memory bus, half the transfer rate.
    MemoryClock = 15,
    /// MHz the memory controller runs at.
    MemoryControllerClock = 16,
    /// Memory timings in memory clock cycles.
    MemoryCl = 17,
    MemoryRcd = 18,
    MemoryRp = 19,
    MemoryRas = 20,
    MemoryRc = 21,
    /// 1 or 2: memory clock cycles per command.
    MemoryCommandRate = 22,
}

/// A reading taken on each physical core, in processor order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PerCore {
    /// °C.
    Temp = 0,
    /// MHz while running, averaged over the last second.
    Clock = 1,
    /// Volts requested.
    Voltage = 2,
    /// Watts.
    Power = 3,
}

impl PerCore {
    pub const ALL: [PerCore; 4] = [
        PerCore::Temp,
        PerCore::Clock,
        PerCore::Voltage,
        PerCore::Power,
    ];

    fn range(self) -> std::ops::Range<usize> {
        let start = SENSOR_SLOTS + self as usize * CORE_SLOTS;
        start..start + CORE_SLOTS
    }
}

/// Per-core readings to publish; an empty list means the CPU has no such sensor.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CoreReadings {
    pub temps: Vec<f64>,
    pub clocks: Vec<f64>,
    pub voltages: Vec<f64>,
    pub power: Vec<f64>,
}

impl CoreReadings {
    fn of(&self, which: PerCore) -> &[f64] {
        match which {
            PerCore::Temp => &self.temps,
            PerCore::Clock => &self.clocks,
            PerCore::Voltage => &self.voltages,
            PerCore::Power => &self.power,
        }
    }
}

/// Why the service has nothing to report, when it is running.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Ok = 0,
    NoDriver = 1,
    UnsupportedCpu = 2,
    ReadFailed = 3,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum Unavailable {
    #[error("{}", crate::tr!("The Pulsar sensor service is not installed"))]
    NotInstalled,
    #[error("{}", crate::tr!("The Pulsar sensor service is not running"))]
    NotRunning,
    #[error("{}", crate::tr!("The Pulsar sensor service needs updating"))]
    WrongVersion,
    #[error("{}", crate::tr!("PawnIO is not installed"))]
    NoDriver,
    #[error("{}", crate::tr!("This CPU is not supported yet"))]
    UnsupportedCpu,
    #[error("{}", crate::tr!("The CPU sensors could not be read"))]
    ReadFailed,
}

/// A process's recent frame rate.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FrameRate {
    pub pid: u32,
    /// Frames presented in the last second.
    pub fps: f64,
    /// Milliseconds, the average over the last second.
    pub frame_time: f64,
    /// The rate the slowest 1% and 0.1% of recent frames would run at.
    pub low_1: f64,
    pub low_01: f64,
}

/// How often a display's picture changed lately, whichever app drew it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct DisplayRate {
    /// The graphics adapter driving the display; zero when unknown.
    pub adapter_luid: u64,
    /// The adapter's video output, as display configuration numbers it.
    pub source: u32,
    pub fps: f64,
}

/// What the service measured in its last second.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FrameRates {
    pub apps: Vec<FrameRate>,
    pub displays: Vec<DisplayRate>,
}

#[repr(C)]
struct FrameSlot {
    /// Zero while the slot is empty or being written.
    pid: AtomicU32,
    values: [AtomicI64; 4],
}

#[repr(C)]
struct DisplaySlot {
    /// One more than the source, so zero while empty or being written.
    source: AtomicU32,
    adapter_luid: AtomicU64,
    fps: AtomicI64,
}

#[repr(C)]
struct Shared {
    // `magic` and `version` must stay first, matching `Header`.
    magic: AtomicU32,
    version: AtomicU32,
    status: AtomicU32,
    /// `GetTickCount64` at the last update.
    tick_ms: AtomicU64,
    values: [AtomicI64; SLOTS],
    frames: [FrameSlot; FRAME_SLOTS],
    displays: [DisplaySlot; DISPLAY_SLOTS],
}

/// One sample of every sensor the service could read.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Readings([i64; SLOTS]);

impl Readings {
    pub fn get(&self, sensor: Sensor) -> Option<f64> {
        let raw = self.0[sensor as usize];
        (raw != MISSING).then(|| raw as f64 / 1000.0)
    }

    /// One value for each physical core, in processor order; empty where
    /// the CPU has no such sensor.
    pub fn per_core(&self, which: PerCore) -> Vec<f64> {
        self.0[which.range()]
            .iter()
            .take_while(|&&raw| raw != MISSING)
            .map(|&raw| raw as f64 / 1000.0)
            .collect()
    }

    pub fn core_temps(&self) -> Vec<f64> {
        self.per_core(PerCore::Temp)
    }
}

fn milli(value: f64) -> i64 {
    if value.is_finite() {
        (value * 1000.0).round() as i64
    } else {
        MISSING
    }
}

fn slots(values: &[(Sensor, f64)], cores: &CoreReadings) -> [i64; SLOTS] {
    let mut slots = [MISSING; SLOTS];
    for &(sensor, value) in values {
        slots[sensor as usize] = milli(value);
    }
    for which in PerCore::ALL {
        for (slot, &value) in slots[which.range()].iter_mut().zip(cores.of(which)) {
            *slot = milli(value);
        }
    }
    slots
}

/// What a reader makes of the section's header at `now_ms`.
fn check(
    magic: u32,
    version: u32,
    status: u32,
    tick_ms: u64,
    now_ms: u64,
) -> Result<(), Unavailable> {
    if magic != MAGIC || version != VERSION {
        return Err(Unavailable::WrongVersion);
    }
    if now_ms.saturating_sub(tick_ms) > STALE_MS {
        return Err(Unavailable::NotRunning);
    }
    match status {
        0 => Ok(()),
        1 => Err(Unavailable::NoDriver),
        2 => Err(Unavailable::UnsupportedCpu),
        _ => Err(Unavailable::ReadFailed),
    }
}

struct View {
    handle: HANDLE,
    address: MEMORY_MAPPED_VIEW_ADDRESS,
}

/// The fields every layout version starts with.
#[repr(C)]
struct Header {
    magic: AtomicU32,
    version: AtomicU32,
}

impl View {
    /// Readable whatever version wrote the section: every one is at least a page.
    fn header(&self) -> &Header {
        unsafe { &*self.address.Value.cast::<Header>() }
    }

    /// Only for a section this version created, or one whose header says so:
    /// an older service's section is smaller.
    fn shared(&self) -> &Shared {
        unsafe { &*self.address.Value.cast::<Shared>() }
    }
}

impl Drop for View {
    fn drop(&mut self) {
        unsafe {
            let _ = UnmapViewOfFile(self.address);
            let _ = CloseHandle(self.handle);
        }
    }
}

/// Maps the whole section, however large its writer made it.
fn map(handle: HANDLE, access: FILE_MAP) -> Option<View> {
    let address = unsafe { MapViewOfFile(handle, access, 0, 0, 0) };
    if address.Value.is_null() {
        unsafe {
            let _ = CloseHandle(handle);
        }
        return None;
    }
    Some(View { handle, address })
}

/// Opens the section on every call, so a restarted service is picked up.
fn open() -> Result<View, Unavailable> {
    let name = HSTRING::from(MAPPING_NAME);
    let handle = unsafe { OpenFileMappingW(FILE_MAP_READ.0, false, &name) }
        .map_err(|_| Unavailable::NotInstalled)?;
    let view = map(handle, FILE_MAP_READ).ok_or(Unavailable::NotInstalled)?;
    let header = view.header();
    if header.magic.load(Ordering::Relaxed) != MAGIC
        || header.version.load(Ordering::Relaxed) != VERSION
    {
        return Err(Unavailable::WrongVersion);
    }
    Ok(view)
}

/// The service's latest readings.
pub fn read() -> Result<Readings, Unavailable> {
    let view = open()?;
    let s = view.shared();
    check(
        s.magic.load(Ordering::Relaxed),
        s.version.load(Ordering::Relaxed),
        s.status.load(Ordering::Relaxed),
        s.tick_ms.load(Ordering::Relaxed),
        unsafe { GetTickCount64() },
    )?;
    Ok(Readings(std::array::from_fn(|i| {
        s.values[i].load(Ordering::Relaxed)
    })))
}

/// The frame rates of the processes presenting lately and of the
/// displays. They need no driver, so the service's sensor status does not
/// matter.
pub fn read_frames() -> Result<FrameRates, Unavailable> {
    let view = open()?;
    let s = view.shared();
    check(
        s.magic.load(Ordering::Relaxed),
        s.version.load(Ordering::Relaxed),
        Status::Ok as u32,
        s.tick_ms.load(Ordering::Relaxed),
        unsafe { GetTickCount64() },
    )?;
    Ok(FrameRates {
        apps: s.frames.iter().filter_map(read_slot).collect(),
        displays: s.displays.iter().filter_map(read_display).collect(),
    })
}

fn read_display(slot: &DisplaySlot) -> Option<DisplayRate> {
    let source = slot.source.load(Ordering::Acquire);
    let adapter_luid = slot.adapter_luid.load(Ordering::Relaxed);
    let fps = slot.fps.load(Ordering::Relaxed) as f64 / 1000.0;
    (source != 0 && slot.source.load(Ordering::Acquire) == source).then(|| DisplayRate {
        adapter_luid,
        source: source - 1,
        fps,
    })
}

fn write_display(slot: &DisplaySlot, rate: Option<&DisplayRate>) {
    slot.source.store(0, Ordering::Release);
    let Some(r) = rate else { return };
    slot.adapter_luid.store(r.adapter_luid, Ordering::Relaxed);
    slot.fps.store(milli(r.fps).max(0), Ordering::Relaxed);
    slot.source.store(r.source + 1, Ordering::Release);
}

fn read_slot(slot: &FrameSlot) -> Option<FrameRate> {
    let pid = slot.pid.load(Ordering::Acquire);
    let [fps, frame_time, low_1, low_01] =
        std::array::from_fn(|i| slot.values[i].load(Ordering::Relaxed) as f64 / 1000.0);
    // A slot rewritten while it was read belongs to another process now.
    (pid != 0 && slot.pid.load(Ordering::Acquire) == pid).then_some(FrameRate {
        pid,
        fps,
        frame_time,
        low_1,
        low_01,
    })
}

fn write_slot(slot: &FrameSlot, rate: Option<&FrameRate>) {
    slot.pid.store(0, Ordering::Release);
    let Some(r) = rate else { return };
    for (value, v) in slot
        .values
        .iter()
        .zip([r.fps, r.frame_time, r.low_1, r.low_01])
    {
        value.store(milli(v).max(0), Ordering::Relaxed);
    }
    slot.pid.store(r.pid, Ordering::Release);
}

/// Asks the service for frame rates for the next few seconds.
pub fn want_frames() {
    let name = HSTRING::from(FRAMES_EVENT_NAME);
    if let Ok(event) = unsafe { OpenEventW(EVENT_MODIFY_STATE, false, &name) } {
        unsafe {
            let _ = SetEvent(event);
            let _ = CloseHandle(event);
        }
    }
}

/// The service's side of the app's request for frame rates.
pub struct FramesWanted(HANDLE);

// Only signalled, waited on and reset.
unsafe impl Send for FramesWanted {}

impl FramesWanted {
    pub fn create() -> windows::core::Result<Self> {
        let handle = with_security(FRAMES_EVENT_SECURITY, |attributes| unsafe {
            CreateEventW(
                Some(attributes),
                true,
                false,
                &HSTRING::from(FRAMES_EVENT_NAME),
            )
        })?;
        Ok(Self(handle))
    }

    /// Whether the app asked since the last call.
    pub fn take(&self) -> bool {
        let asked = unsafe { WaitForSingleObject(self.0, 0) } == WAIT_OBJECT_0;
        if asked {
            unsafe {
                let _ = ResetEvent(self.0);
            }
        }
        asked
    }
}

impl Drop for FramesWanted {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

fn with_security<T>(
    sddl: PCWSTR,
    f: impl FnOnce(&SECURITY_ATTRIBUTES) -> windows::core::Result<T>,
) -> windows::core::Result<T> {
    let mut descriptor = PSECURITY_DESCRIPTOR::default();
    unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl,
            SDDL_REVISION_1,
            &mut descriptor,
            None,
        )?;
    }
    let attributes = SECURITY_ATTRIBUTES {
        nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: descriptor.0,
        bInheritHandle: false.into(),
    };
    let result = f(&attributes);
    unsafe {
        let _ = LocalFree(Some(HLOCAL(descriptor.0)));
    }
    result
}

/// The service's side of the section.
pub struct Publisher(View);

// The view is only touched through atomics.
unsafe impl Send for Publisher {}

impl Publisher {
    pub fn create() -> windows::core::Result<Self> {
        let handle = with_security(SECURITY, |attributes| unsafe {
            CreateFileMappingW(
                INVALID_HANDLE_VALUE,
                Some(attributes),
                PAGE_READWRITE,
                0,
                size_of::<Shared>() as u32,
                &HSTRING::from(MAPPING_NAME),
            )
        })?;
        let view = map(handle, FILE_MAP_WRITE).ok_or_else(windows::core::Error::from_thread)?;
        let s = view.shared();
        s.version.store(VERSION, Ordering::Relaxed);
        s.magic.store(MAGIC, Ordering::Relaxed);
        Ok(Self(view))
    }

    /// Sensors left out of `values` read as missing.
    pub fn publish(&self, status: Status, values: &[(Sensor, f64)], cores: &CoreReadings) {
        let s = self.0.shared();
        for (slot, value) in s.values.iter().zip(slots(values, cores)) {
            slot.store(value, Ordering::Relaxed);
        }
        s.status.store(status as u32, Ordering::Relaxed);
        s.tick_ms
            .store(unsafe { GetTickCount64() }, Ordering::Relaxed);
    }

    /// Rates past the section's slots are left out.
    pub fn publish_frames(&self, rates: &FrameRates) {
        let s = self.0.shared();
        for (i, slot) in s.frames.iter().enumerate() {
            write_slot(slot, rates.apps.get(i));
        }
        for (i, slot) in s.displays.iter().enumerate() {
            write_display(slot, rates.displays.get(i));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temps(t: &[f64]) -> CoreReadings {
        CoreReadings {
            temps: t.to_vec(),
            ..CoreReadings::default()
        }
    }

    #[test]
    fn published_values_read_back_and_the_rest_are_missing() {
        let r = Readings(slots(&[(Sensor::CpuTemp, 41.5)], &temps(&[])));
        assert_eq!(r.get(Sensor::CpuTemp), Some(41.5));
        assert_eq!(r.get(Sensor::CpuPower), None);
        let r = Readings(slots(
            &[(Sensor::CpuPower, 87.25), (Sensor::CpuTemp, -5.0)],
            &temps(&[]),
        ));
        assert_eq!(r.get(Sensor::CpuPower), Some(87.25));
        assert_eq!(r.get(Sensor::CpuTemp), Some(-5.0));
    }

    #[test]
    fn a_value_that_is_not_a_number_is_missing() {
        let r = Readings(slots(&[(Sensor::CpuPower, f64::NAN)], &temps(&[])));
        assert_eq!(r.get(Sensor::CpuPower), None);
    }

    #[test]
    fn core_temperatures_keep_their_order_and_count() {
        let r = Readings(slots(
            &[(Sensor::CpuThrottling, 1.0)],
            &temps(&[40.0, 55.5, 38.0]),
        ));
        assert_eq!(r.core_temps(), [40.0, 55.5, 38.0]);
        assert_eq!(r.get(Sensor::CpuThrottling), Some(1.0));
        assert!(Readings(slots(&[], &temps(&[]))).core_temps().is_empty());
        let many = vec![50.0; CORE_SLOTS + 10];
        assert_eq!(
            Readings(slots(&[], &temps(&many))).core_temps().len(),
            CORE_SLOTS
        );
    }

    #[test]
    fn each_per_core_reading_has_its_own_slots() {
        let cores = CoreReadings {
            temps: vec![40.0],
            clocks: vec![5300.0, 4200.0],
            voltages: vec![1.285],
            power: Vec::new(),
        };
        let r = Readings(slots(&[(Sensor::CpuDramPower, 3.5)], &cores));
        assert_eq!(r.per_core(PerCore::Clock), [5300.0, 4200.0]);
        assert_eq!(r.per_core(PerCore::Voltage), [1.285]);
        assert_eq!(r.per_core(PerCore::Temp), [40.0]);
        assert!(r.per_core(PerCore::Power).is_empty());
        assert_eq!(r.get(Sensor::CpuDramPower), Some(3.5));
    }

    #[test]
    fn frame_rates_read_back_from_their_slots() {
        let slot = FrameSlot {
            pid: AtomicU32::new(0),
            values: Default::default(),
        };
        assert_eq!(read_slot(&slot), None);
        let rate = FrameRate {
            pid: 4242,
            fps: 143.5,
            frame_time: 6.968,
            low_1: 98.25,
            low_01: 61.0,
        };
        write_slot(&slot, Some(&rate));
        assert_eq!(read_slot(&slot), Some(rate));
        write_slot(&slot, None);
        assert_eq!(read_slot(&slot), None);
    }

    #[test]
    fn display_rates_read_back_with_output_zero_kept() {
        let slot = DisplaySlot {
            source: AtomicU32::new(0),
            adapter_luid: AtomicU64::new(0),
            fps: AtomicI64::new(0),
        };
        assert_eq!(read_display(&slot), None);
        let rate = DisplayRate {
            adapter_luid: 0x1_0000_ABCD,
            source: 0,
            fps: 119.5,
        };
        write_display(&slot, Some(&rate));
        assert_eq!(read_display(&slot), Some(rate));
        write_display(&slot, None);
        assert_eq!(read_display(&slot), None);
    }

    #[test]
    fn a_fresh_section_is_usable() {
        assert_eq!(check(MAGIC, VERSION, 0, 1000, 1500), Ok(()));
    }

    #[test]
    fn a_stale_section_means_the_service_stopped() {
        assert_eq!(
            check(MAGIC, VERSION, 0, 1000, 1000 + STALE_MS + 1),
            Err(Unavailable::NotRunning)
        );
    }

    #[test]
    fn another_layout_is_never_read() {
        assert_eq!(
            check(MAGIC, VERSION + 1, 0, 0, 0),
            Err(Unavailable::WrongVersion)
        );
        assert_eq!(check(0, VERSION, 0, 0, 0), Err(Unavailable::WrongVersion));
    }

    #[test]
    fn the_service_says_why_it_has_no_readings() {
        let failed = |status: Status| check(MAGIC, VERSION, status as u32, 0, 0);
        assert_eq!(failed(Status::NoDriver), Err(Unavailable::NoDriver));
        assert_eq!(
            failed(Status::UnsupportedCpu),
            Err(Unavailable::UnsupportedCpu)
        );
        assert_eq!(failed(Status::ReadFailed), Err(Unavailable::ReadFailed));
        assert_eq!(Unavailable::NoDriver.to_string(), "PawnIO is not installed");
    }
}
