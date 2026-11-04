//! Physical drives, asked through storage queries that need no access
//! rights to the disk itself.

use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex, MutexGuard, OnceLock, PoisonError};
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAGS_AND_ATTRIBUTES, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows::Win32::System::IO::DeviceIoControl;
use windows::Win32::System::Power::GetDevicePowerState;
use windows::Win32::System::Threading::{OpenMutexW, SYNCHRONIZATION_SYNCHRONIZE};
use windows::core::{BOOL, HSTRING};

use crate::metric::{MetricKey, Snapshot, Source, SourceError, SourceId};
use crate::sources::fixed_drives;

const IOCTL_STORAGE_QUERY_PROPERTY: u32 = 0x002D_1400;
const IOCTL_DISK_GET_DRIVE_GEOMETRY_EX: u32 = 0x0007_00A0;
const IOCTL_VOLUME_GET_VOLUME_DISK_EXTENTS: u32 = 0x0056_0000;
const DEVICE_PROPERTY: u32 = 0;
const SEEK_PENALTY_PROPERTY: u32 = 7;
const TEMPERATURE_PROPERTY: u32 = 52;
/// More physical drives than any desktop has.
const MAX_DRIVES: u32 = 32;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DiskInfo {
    /// `N` in `\\.\PhysicalDriveN`.
    pub number: u32,
    pub model: String,
    pub firmware: Option<String>,
    pub bus: Option<&'static str>,
    /// `None` when the drive does not say.
    pub solid_state: Option<bool>,
    pub size_bytes: u64,
    pub removable: bool,
    pub letters: Vec<char>,
    /// Thresholds the drive reports, in °C.
    pub warning_c: Option<i16>,
    pub critical_c: Option<i16>,
}

impl DiskInfo {
    pub fn kind(&self) -> &'static str {
        match (self.bus, self.solid_state) {
            (Some("NVMe"), _) => "NVMe SSD",
            (_, Some(true)) => "SSD",
            (_, Some(false)) => "HDD",
            _ => crate::tr!("Drive"),
        }
    }
}

struct Handle(HANDLE);

impl Drop for Handle {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

/// Opens a device for queries only: no read or write access is asked for.
fn open(path: &str) -> Option<Handle> {
    unsafe {
        CreateFileW(
            &HSTRING::from(path),
            0,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            None,
            OPEN_EXISTING,
            FILE_FLAGS_AND_ATTRIBUTES(0),
            None,
        )
    }
    .ok()
    .map(Handle)
}

fn control(device: &Handle, code: u32, input: &[u8], output_len: usize) -> Option<Vec<u8>> {
    let mut output = vec![0u8; output_len];
    let mut returned = 0u32;
    unsafe {
        DeviceIoControl(
            device.0,
            code,
            (!input.is_empty()).then_some(input.as_ptr().cast()),
            input.len() as u32,
            Some(output.as_mut_ptr().cast()),
            output.len() as u32,
            Some(&mut returned),
            None,
        )
    }
    .ok()?;
    output.truncate(returned as usize);
    Some(output)
}

/// A standard `STORAGE_PROPERTY_QUERY` for one property.
fn property(device: &Handle, id: u32, output_len: usize) -> Option<Vec<u8>> {
    let mut query = [0u8; 12];
    query[..4].copy_from_slice(&id.to_le_bytes());
    control(device, IOCTL_STORAGE_QUERY_PROPERTY, &query, output_len)
}

fn u32_at(bytes: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        bytes.get(offset..offset + 4)?.try_into().ok()?,
    ))
}

fn i16_at(bytes: &[u8], offset: usize) -> Option<i16> {
    Some(i16::from_le_bytes(
        bytes.get(offset..offset + 2)?.try_into().ok()?,
    ))
}

/// A NUL-terminated ASCII string at an offset the descriptor gives; zero means none.
fn ascii_at(bytes: &[u8], offset: u32) -> Option<String> {
    let start = offset as usize;
    if start == 0 {
        return None;
    }
    let tail = bytes.get(start..)?;
    let end = tail.iter().position(|&b| b == 0).unwrap_or(tail.len());
    let s = String::from_utf8_lossy(&tail[..end]).trim().to_string();
    (!s.is_empty()).then_some(s)
}

pub fn bus_name(code: u32) -> Option<&'static str> {
    Some(match code {
        1 => "SCSI",
        3 => "ATA",
        7 => "USB",
        8 => "RAID",
        10 => "SAS",
        11 => "SATA",
        12 => "SD",
        13 => "MMC",
        14 | 15 => crate::tr!("Virtual"),
        16 => crate::tr!("Storage Spaces"),
        17 => "NVMe",
        19 => "UFS",
        _ => return None,
    })
}

/// Model, firmware, bus and removability from a `STORAGE_DEVICE_DESCRIPTOR`.
fn describe(bytes: &[u8]) -> Option<(String, Option<String>, Option<&'static str>, bool)> {
    let vendor = ascii_at(bytes, u32_at(bytes, 12)?);
    let product = ascii_at(bytes, u32_at(bytes, 16)?);
    let firmware = ascii_at(bytes, u32_at(bytes, 20)?);
    let bus = bus_name(u32_at(bytes, 28)?);
    let removable = bytes.get(10).is_some_and(|&b| b != 0);
    let model = match (vendor, product) {
        (Some(v), Some(p)) if !p.starts_with(&v) => format!("{v} {p}"),
        (_, Some(p)) => p,
        (Some(v), None) => v,
        (None, None) => return None,
    };
    Some((model, firmware, bus, removable))
}

fn serial_of(bytes: &[u8]) -> Option<String> {
    ascii_at(bytes, u32_at(bytes, 24)?)
}

/// Current temperature and the warning and critical thresholds, in °C.
type Temperatures = (i16, Option<i16>, Option<i16>);

fn temperatures(bytes: &[u8]) -> Option<Temperatures> {
    let count = u16::from_le_bytes(bytes.get(12..14)?.try_into().ok()?);
    if count == 0 {
        return None;
    }
    let threshold = |offset| i16_at(bytes, offset).filter(|&t| t > 0);
    Some((i16_at(bytes, 26)?, threshold(10), threshold(8)))
}

fn disk_of_volume(letter: u8) -> Option<u32> {
    let volume = open(&format!(r"\\.\{}:", letter as char))?;
    let extents = control(&volume, IOCTL_VOLUME_GET_VOLUME_DISK_EXTENTS, &[], 256)?;
    // A volume spanning disks is listed under the first.
    (u32_at(&extents, 0)? > 0)
        .then(|| u32_at(&extents, 8))
        .flatten()
}

fn physical(number: u32) -> Option<Handle> {
    open(&format!(r"\\.\PhysicalDrive{number}"))
}

/// Temperature queries can be audible on a hard drive, so it is read far
/// less often than a solid state drive.
const ROTATIONAL_INTERVAL: Duration = Duration::from_secs(5 * 60);
/// Each query is a command to the drive, which costs milliseconds and can
/// keep it from its deeper idle states.
const SOLID_STATE_INTERVAL: Duration = Duration::from_secs(10);
const MAX_BACKOFF: Duration = Duration::from_secs(60 * 60);
/// How long a rotational drive's last reading still counts as its temperature.
const STALE_AFTER: Duration = Duration::from_secs(30 * 60);
const REDISCOVER: Duration = Duration::from_secs(60);

trait DriveCalls: Send {
    /// Opens the drive and keeps it open until `forget`.
    fn describe(&mut self, number: u32) -> Option<Described>;
    fn size_bytes(&mut self, number: u32) -> u64;
    /// `None` when Windows cannot say.
    fn powered_on(&mut self, number: u32) -> Option<bool>;
    fn temperature(&mut self, number: u32) -> Option<Temperatures>;
    fn forget(&mut self, number: u32);
    /// Whether another process is reading rotational drives for this one.
    fn yields_rotational_reads(&self) -> bool;
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Described {
    model: Option<String>,
    serial: Option<String>,
    firmware: Option<String>,
    bus: Option<&'static str>,
    removable: bool,
    solid_state: Option<bool>,
}

impl Described {
    /// A drive that does not say it has no seek penalty may be a hard drive.
    fn is_rotational(&self) -> bool {
        self.solid_state != Some(true)
    }
}

#[derive(Default)]
struct SystemDrives {
    handles: HashMap<u32, Handle>,
}

// The handles are only used from the thread that owns the value.
unsafe impl Send for SystemDrives {}

impl DriveCalls for SystemDrives {
    fn describe(&mut self, number: u32) -> Option<Described> {
        let drive = physical(number)?;
        let descriptor = property(&drive, DEVICE_PROPERTY, 1024);
        let (model, firmware, bus, removable) = descriptor
            .as_deref()
            .and_then(describe)
            .map_or((None, None, None, false), |(m, f, b, r)| (Some(m), f, b, r));
        let described = Described {
            model,
            serial: descriptor.as_deref().and_then(serial_of),
            firmware,
            bus,
            removable,
            solid_state: property(&drive, SEEK_PENALTY_PROPERTY, 12)
                .and_then(|b| b.get(8).map(|&seek| seek == 0)),
        };
        self.handles.insert(number, drive);
        Some(described)
    }

    fn size_bytes(&mut self, number: u32) -> u64 {
        self.handles
            .get(&number)
            .and_then(|drive| control(drive, IOCTL_DISK_GET_DRIVE_GEOMETRY_EX, &[], 256))
            .and_then(|g| Some(u64::from_le_bytes(g.get(24..32)?.try_into().ok()?)))
            .unwrap_or(0)
    }

    fn powered_on(&mut self, number: u32) -> Option<bool> {
        let drive = self.handles.get(&number)?;
        let mut on = BOOL(0);
        unsafe { GetDevicePowerState(drive.0, &mut on) }.ok().ok()?;
        Some(on.as_bool())
    }

    fn temperature(&mut self, number: u32) -> Option<Temperatures> {
        property(self.handles.get(&number)?, TEMPERATURE_PROPERTY, 256)
            .as_deref()
            .and_then(temperatures)
    }

    fn forget(&mut self, number: u32) {
        self.handles.remove(&number);
    }

    fn yields_rotational_reads(&self) -> bool {
        YIELD_TO.get().is_some_and(|name| mutex_exists(name))
    }
}

static YIELD_TO: OnceLock<&'static str> = OnceLock::new();

/// Processes share no schedule: this one leaves rotational drives alone
/// while the process holding `mutex` is running, so they are read only once.
pub fn yield_rotational_reads_to(mutex: &'static str) {
    let _ = YIELD_TO.set(mutex);
}

fn mutex_exists(name: &str) -> bool {
    match unsafe { OpenMutexW(SYNCHRONIZATION_SYNCHRONIZE, false, &HSTRING::from(name)) } {
        Ok(handle) => {
            drop(Handle(handle));
            true
        }
        Err(_) => false,
    }
}

/// What stays the same when a drive changes number. Without a serial number
/// to tell it from other drives, its drive number has to stand in.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct DriveId {
    model: Option<String>,
    serial: Option<String>,
    number: Option<u32>,
}

impl DriveId {
    fn of(facts: &Described, number: u32) -> Self {
        DriveId {
            model: facts.model.clone(),
            serial: facts.serial.clone(),
            number: facts.serial.is_none().then_some(number),
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct Reading {
    temp_c: i16,
    warning_c: Option<i16>,
    critical_c: Option<i16>,
    at: Instant,
}

#[derive(Clone, Copy, Debug, Default)]
struct DriveState {
    next_due: Option<Instant>,
    failures: u32,
    last: Option<Reading>,
    asleep: bool,
}

enum Outcome {
    Asleep,
    PowerUnknown,
    Read(Option<Temperatures>),
}

/// The only path to a rotational drive's temperature in this process.
#[derive(Default)]
struct Schedule {
    drives: HashMap<DriveId, DriveState>,
}

impl Schedule {
    /// Takes the drive's turn if it is due, holding off everyone else while
    /// the caller queries the drive without the lock.
    fn claim(&mut self, id: &DriveId, now: Instant) -> bool {
        let state = self.drives.entry(id.clone()).or_default();
        if state.next_due.is_some_and(|due| now < due) {
            return false;
        }
        state.next_due = Some(now + ROTATIONAL_INTERVAL);
        true
    }

    fn record(&mut self, id: &DriveId, outcome: Outcome, now: Instant) {
        let state = self.drives.entry(id.clone()).or_default();
        state.asleep = matches!(outcome, Outcome::Asleep);
        state.next_due = Some(now + ROTATIONAL_INTERVAL);
        match outcome {
            Outcome::Asleep | Outcome::PowerUnknown => {}
            Outcome::Read(Some((temp_c, warning_c, critical_c))) => {
                state.failures = 0;
                state.last = Some(Reading {
                    temp_c,
                    warning_c,
                    critical_c,
                    at: now,
                });
            }
            Outcome::Read(None) => {
                state.failures += 1;
                state.next_due = Some(now + backoff(state.failures));
            }
        }
    }

    fn state(&self, id: &DriveId) -> DriveState {
        self.drives.get(id).copied().unwrap_or_default()
    }
}

fn backoff(failures: u32) -> Duration {
    ROTATIONAL_INTERVAL
        .saturating_mul(1 << failures.saturating_sub(1).min(4))
        .min(MAX_BACKOFF)
}

fn shared_schedule() -> Arc<Mutex<Schedule>> {
    static SHARED: LazyLock<Arc<Mutex<Schedule>>> = LazyLock::new(Arc::default);
    SHARED.clone()
}

fn lock(schedule: &Mutex<Schedule>) -> MutexGuard<'_, Schedule> {
    schedule.lock().unwrap_or_else(PoisonError::into_inner)
}

pub fn read() -> Vec<DiskInfo> {
    let volumes: Vec<(u32, char)> = fixed_drives()
        .into_iter()
        .filter_map(|letter| Some((disk_of_volume(letter)?, letter as char)))
        .collect();
    inventory(&mut SystemDrives::default(), &shared_schedule(), &volumes)
}

fn inventory(
    calls: &mut dyn DriveCalls,
    schedule: &Mutex<Schedule>,
    volumes: &[(u32, char)],
) -> Vec<DiskInfo> {
    let mut seen: Vec<DriveId> = Vec::new();
    (0..MAX_DRIVES)
        .filter_map(|number| {
            let facts = calls.describe(number)?;
            let model = facts.model.clone()?;
            let mut id = DriveId::of(&facts, number);
            if seen.contains(&id) {
                id.number = Some(number);
            }
            let limits = if facts.is_rotational() {
                lock(schedule)
                    .state(&id)
                    .last
                    .map(|r| (r.warning_c, r.critical_c))
            } else {
                calls.temperature(number).map(|t| (t.1, t.2))
            };
            seen.push(id);
            Some(DiskInfo {
                number,
                model,
                firmware: facts.firmware,
                bus: facts.bus,
                solid_state: facts.solid_state,
                size_bytes: calls.size_bytes(number),
                removable: facts.removable,
                letters: volumes
                    .iter()
                    .filter(|(disk, _)| *disk == number)
                    .map(|(_, letter)| *letter)
                    .collect(),
                warning_c: limits.and_then(|l| l.0),
                critical_c: limits.and_then(|l| l.1),
            })
        })
        .collect()
}

struct TrackedDrive {
    number: u32,
    id: DriveId,
    rotational: bool,
    /// The last call failed, so its handle may be to a drive that has gone.
    suspect: bool,
    /// A solid state drive's last read and its temperature; a failed read
    /// leaves none.
    last_read: Option<(Instant, Option<i16>)>,
}

pub struct DriveTempSource {
    calls: Box<dyn DriveCalls>,
    schedule: Arc<Mutex<Schedule>>,
    clock: Box<dyn Fn() -> Instant + Send>,
    drives: Vec<TrackedDrive>,
    found_at: Instant,
}

impl DriveTempSource {
    pub fn new() -> Self {
        Self::with(
            Box::new(SystemDrives::default()),
            shared_schedule(),
            Box::new(Instant::now),
        )
    }

    fn with(
        calls: Box<dyn DriveCalls>,
        schedule: Arc<Mutex<Schedule>>,
        clock: Box<dyn Fn() -> Instant + Send>,
    ) -> Self {
        let found_at = clock();
        let mut source = Self {
            calls,
            schedule,
            clock,
            drives: Vec::new(),
            found_at,
        };
        source.discover();
        source
    }

    /// Keeps the drives that still answer and opens the other drive numbers.
    fn discover(&mut self) {
        let Self {
            calls,
            drives,
            clock,
            ..
        } = self;
        drives.retain(|drive| {
            if drive.suspect {
                calls.forget(drive.number);
            }
            !drive.suspect
        });
        for number in 0..MAX_DRIVES {
            if drives.iter().any(|d| d.number == number) {
                continue;
            }
            let Some(facts) = calls.describe(number) else {
                continue;
            };
            let mut last_read = None;
            if !facts.is_rotational() {
                let Some(read) = calls.temperature(number) else {
                    calls.forget(number);
                    continue;
                };
                last_read = Some((clock(), Some(read.0).filter(|&t| t > 0)));
            }
            let mut id = DriveId::of(&facts, number);
            if let Some(holder) = drives.iter().position(|d| d.id == id) {
                // The same drive under a new number, or two that look alike.
                if calls.powered_on(drives[holder].number).is_some() {
                    id.number = Some(number);
                } else {
                    calls.forget(drives.remove(holder).number);
                }
            }
            drives.push(TrackedDrive {
                number,
                id,
                rotational: facts.is_rotational(),
                suspect: false,
                last_read,
            });
        }
        drives.sort_by_key(|d| d.number);
    }
}

impl Default for DriveTempSource {
    fn default() -> Self {
        Self::new()
    }
}

fn poll_rotational(
    calls: &mut dyn DriveCalls,
    schedule: &Mutex<Schedule>,
    drive: &mut TrackedDrive,
    clock: &dyn Fn() -> Instant,
) {
    if !lock(schedule).claim(&drive.id, clock()) || calls.yields_rotational_reads() {
        return;
    }
    let outcome = match calls.powered_on(drive.number) {
        Some(false) => Outcome::Asleep,
        None => Outcome::PowerUnknown,
        Some(true) => Outcome::Read(calls.temperature(drive.number).filter(|t| t.0 > 0)),
    };
    drive.suspect = matches!(outcome, Outcome::PowerUnknown | Outcome::Read(None));
    lock(schedule).record(&drive.id, outcome, clock());
}

impl Source for DriveTempSource {
    fn id(&self) -> SourceId {
        SourceId::DriveTemps
    }

    fn sample(&mut self, out: &mut Snapshot) -> Result<(), SourceError> {
        let now = (self.clock)();
        if now.saturating_duration_since(self.found_at) >= REDISCOVER {
            self.discover();
            self.found_at = (self.clock)();
        }
        let mut hottest = None;
        let mut reporting = false;
        for drive in &mut self.drives {
            let number = drive.number as u8;
            let temp = if drive.rotational {
                poll_rotational(
                    self.calls.as_mut(),
                    &self.schedule,
                    drive,
                    self.clock.as_ref(),
                );
                let now = (self.clock)();
                let state = lock(&self.schedule).state(&drive.id);
                let fresh = state
                    .last
                    .filter(|r| now.saturating_duration_since(r.at) <= STALE_AFTER);
                if let Some(last) = state.last {
                    if let Some(warning) = last.warning_c {
                        out.set(MetricKey::DiskWarningC(number), f64::from(warning));
                    }
                    if let Some(critical) = last.critical_c {
                        out.set(MetricKey::DiskCriticalC(number), f64::from(critical));
                    }
                }
                if let Some(fresh) = fresh {
                    let age = now.saturating_duration_since(fresh.at).as_secs_f64();
                    out.set(MetricKey::DiskTempAgeSeconds(number), age);
                }
                if state.asleep {
                    out.set(MetricKey::DiskAsleep(number), 1.0);
                }
                reporting |= fresh.is_some() || state.asleep || state.failures == 0;
                fresh.map(|r| r.temp_c)
            } else {
                reporting = true;
                let due = drive.last_read.is_none_or(|(at, _)| {
                    now.saturating_duration_since(at) >= SOLID_STATE_INTERVAL
                });
                if due {
                    let read = self.calls.temperature(drive.number);
                    drive.suspect = read.is_none();
                    let temp = read.map(|t| t.0).filter(|&t| t > 0);
                    drive.last_read = Some((now, temp));
                }
                drive.last_read.and_then(|(_, temp)| temp)
            };
            if let Some(temp) = temp {
                let temp = f64::from(temp);
                out.set(MetricKey::DiskTempC(number), temp);
                hottest = Some(hottest.map_or(temp, |h: f64| h.max(temp)));
            }
        }
        if !reporting {
            return Err(SourceError::Unavailable(
                crate::tr!("No drive reports a temperature").into(),
            ));
        }
        if let Some(temp) = hottest {
            out.set(MetricKey::DiskTempMaxC, temp);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIN: Duration = Duration::from_secs(60);
    const SEC: Duration = Duration::from_secs(1);

    fn descriptor(vendor: &str, product: &str, bus: u32) -> Vec<u8> {
        let mut b = vec![0u8; 40];
        b[28..32].copy_from_slice(&bus.to_le_bytes());
        let mut put = |field: usize, s: &str| {
            if s.is_empty() {
                return;
            }
            let at = b.len() as u32;
            b[field..field + 4].copy_from_slice(&at.to_le_bytes());
            b.extend_from_slice(s.as_bytes());
            b.push(0);
        };
        put(12, vendor);
        put(16, product);
        put(20, "3B2QGXA7");
        b
    }

    #[test]
    fn the_model_joins_vendor_and_product_once() {
        let (model, firmware, bus, _) =
            describe(&descriptor("", "Samsung SSD 990 PRO 2TB", 17)).unwrap();
        assert_eq!(model, "Samsung SSD 990 PRO 2TB");
        assert_eq!(firmware.as_deref(), Some("3B2QGXA7"));
        assert_eq!(bus, Some("NVMe"));
        let (model, ..) = describe(&descriptor("WDC", "WD40EFRX", 11)).unwrap();
        assert_eq!(model, "WDC WD40EFRX");
        assert!(describe(&descriptor("", "", 11)).is_none());
    }

    #[test]
    fn the_serial_number_comes_from_the_descriptor() {
        let mut b = descriptor("WDC", "WD40EFRX", 11);
        assert_eq!(serial_of(&b), None);
        let at = b.len() as u32;
        b[24..28].copy_from_slice(&at.to_le_bytes());
        b.extend_from_slice(b"  WD-123  \0");
        assert_eq!(serial_of(&b).as_deref(), Some("WD-123"));
    }

    #[test]
    fn temperatures_and_thresholds() {
        let mut b = vec![0u8; 40];
        b[8..10].copy_from_slice(&84i16.to_le_bytes());
        b[10..12].copy_from_slice(&80i16.to_le_bytes());
        b[12..14].copy_from_slice(&1u16.to_le_bytes());
        b[26..28].copy_from_slice(&41i16.to_le_bytes());
        assert_eq!(temperatures(&b), Some((41, Some(80), Some(84))));
        b[12] = 0;
        assert_eq!(temperatures(&b), None);
    }

    #[test]
    fn kinds_follow_bus_and_seek_penalty() {
        let disk = |bus, solid_state| DiskInfo {
            bus,
            solid_state,
            ..DiskInfo::default()
        };
        assert_eq!(disk(Some("NVMe"), None).kind(), "NVMe SSD");
        assert_eq!(disk(Some("SATA"), Some(true)).kind(), "SSD");
        assert_eq!(disk(Some("SATA"), Some(false)).kind(), "HDD");
    }

    #[test]
    fn this_machine_has_a_disk() {
        let disks = read();
        assert!(!disks.is_empty());
        assert!(disks.iter().any(|d| d.size_bytes > 0));
    }

    #[test]
    fn a_mutex_is_found_while_it_is_held() {
        let name = format!(r"Local\Pulsar.Test.Yield.{}", std::process::id());
        assert!(!mutex_exists(&name));
        let held = crate::single_instance::SingleInstance::acquire(&name).unwrap();
        assert!(mutex_exists(&name));
        drop(held);
        assert!(!mutex_exists(&name));
    }

    /// Asks each real drive once; run with `--ignored --nocapture`.
    #[test]
    #[ignore = "queries real drives"]
    fn drive_calls_on_this_machine() {
        let mut calls = SystemDrives::default();
        for number in 0..MAX_DRIVES {
            if let Some(facts) = calls.describe(number) {
                println!(
                    "{number}: {:?} serial known {} solid {:?} power {:?} temperature {:?}",
                    facts.model,
                    facts.serial.is_some(),
                    facts.solid_state,
                    calls.powered_on(number),
                    calls.temperature(number),
                );
            }
        }
    }

    #[derive(Clone)]
    struct FakeDrive {
        facts: Described,
        power: Option<bool>,
        temp: Option<Temperatures>,
    }

    fn hdd(serial: &str) -> FakeDrive {
        FakeDrive {
            facts: Described {
                model: Some("WD40EFRX".into()),
                serial: Some(serial.into()),
                solid_state: Some(false),
                ..Described::default()
            },
            power: Some(true),
            temp: Some((38, Some(55), Some(60))),
        }
    }

    fn ssd(serial: &str) -> FakeDrive {
        FakeDrive {
            facts: Described {
                model: Some("Samsung 990".into()),
                serial: Some(serial.into()),
                solid_state: Some(true),
                ..Described::default()
            },
            power: Some(true),
            temp: Some((45, None, None)),
        }
    }

    #[derive(Default)]
    struct FakeState {
        drives: HashMap<u32, FakeDrive>,
        yields: bool,
        describes: HashMap<u32, u32>,
        powers: HashMap<u32, u32>,
        temps: HashMap<u32, u32>,
        slow_read: Option<(u32, Arc<Mutex<Instant>>, Duration)>,
    }

    #[derive(Clone, Default)]
    struct FakeCalls(Arc<Mutex<FakeState>>);

    impl DriveCalls for FakeCalls {
        fn describe(&mut self, number: u32) -> Option<Described> {
            let mut s = self.0.lock().unwrap();
            let facts = s.drives.get(&number)?.facts.clone();
            *s.describes.entry(number).or_default() += 1;
            Some(facts)
        }

        fn size_bytes(&mut self, _: u32) -> u64 {
            1
        }

        fn powered_on(&mut self, number: u32) -> Option<bool> {
            let mut s = self.0.lock().unwrap();
            *s.powers.entry(number).or_default() += 1;
            s.drives.get(&number)?.power
        }

        fn temperature(&mut self, number: u32) -> Option<Temperatures> {
            let mut s = self.0.lock().unwrap();
            *s.temps.entry(number).or_default() += 1;
            if let Some((_, clock, by)) = s.slow_read.as_ref().filter(|slow| slow.0 == number) {
                *clock.lock().unwrap() += *by;
            }
            s.drives.get(&number)?.temp
        }

        fn forget(&mut self, _: u32) {}

        fn yields_rotational_reads(&self) -> bool {
            self.0.lock().unwrap().yields
        }
    }

    struct Rig {
        calls: FakeCalls,
        schedule: Arc<Mutex<Schedule>>,
        clock: Arc<Mutex<Instant>>,
    }

    impl Rig {
        fn new(drives: impl IntoIterator<Item = (u32, FakeDrive)>) -> Self {
            let rig = Self {
                calls: FakeCalls::default(),
                schedule: Arc::default(),
                clock: Arc::new(Mutex::new(Instant::now())),
            };
            rig.calls.0.lock().unwrap().drives = drives.into_iter().collect();
            rig
        }

        fn source(&self) -> DriveTempSource {
            let clock = self.clock.clone();
            DriveTempSource::with(
                Box::new(self.calls.clone()),
                self.schedule.clone(),
                Box::new(move || *clock.lock().unwrap()),
            )
        }

        fn advance(&self, by: Duration) {
            *self.clock.lock().unwrap() += by;
        }

        fn run(&self, source: &mut DriveTempSource, span: Duration) {
            for _ in 0..span.as_secs() {
                let _ = source.sample(&mut Snapshot::default());
                self.advance(SEC);
            }
        }

        fn drive(&self, number: u32, change: impl FnOnce(&mut FakeDrive)) {
            change(
                self.calls
                    .0
                    .lock()
                    .unwrap()
                    .drives
                    .get_mut(&number)
                    .unwrap(),
            );
        }

        fn temps(&self, number: u32) -> u32 {
            let s = self.calls.0.lock().unwrap();
            s.temps.get(&number).copied().unwrap_or(0)
        }

        fn powers(&self, number: u32) -> u32 {
            let s = self.calls.0.lock().unwrap();
            s.powers.get(&number).copied().unwrap_or(0)
        }
    }

    fn sample(source: &mut DriveTempSource) -> Snapshot {
        let mut out = Snapshot::default();
        source.sample(&mut out).unwrap();
        out
    }

    #[test]
    fn a_slow_drive_does_not_shorten_the_next_drives_interval_or_age_its_reading() {
        let rig = Rig::new([(0, hdd("A")), (1, hdd("B"))]);
        let mut source = rig.source();
        rig.calls.0.lock().unwrap().slow_read = Some((0, rig.clock.clone(), 60 * SEC));
        let s = sample(&mut source);
        rig.calls.0.lock().unwrap().slow_read = None;
        assert_eq!(s.get(MetricKey::DiskTempAgeSeconds(1)), Some(0.0));
        rig.advance(ROTATIONAL_INTERVAL - SEC);
        sample(&mut source);
        assert_eq!(rig.temps(1), 1);
        rig.advance(SEC);
        sample(&mut source);
        assert_eq!(rig.temps(1), 2);
    }

    #[test]
    fn a_rotational_drive_is_read_every_five_minutes_and_repeats_its_value() {
        let rig = Rig::new([(0, hdd("A"))]);
        let mut source = rig.source();
        assert_eq!(rig.temps(0), 0, "building the source reads nothing");
        for _ in 0..3600 {
            let s = sample(&mut source);
            assert_eq!(s.get(MetricKey::DiskTempC(0)), Some(38.0));
            assert_eq!(s.get(MetricKey::DiskTempMaxC), Some(38.0));
            rig.advance(SEC);
        }
        assert_eq!(rig.temps(0), 12);
        assert_eq!(rig.powers(0), 12);
    }

    #[test]
    fn the_reading_says_how_old_it_is_and_carries_the_limits() {
        let rig = Rig::new([(0, hdd("A"))]);
        let mut source = rig.source();
        assert_eq!(
            sample(&mut source).get(MetricKey::DiskTempAgeSeconds(0)),
            Some(0.0)
        );
        rig.advance(90 * SEC);
        let s = sample(&mut source);
        assert_eq!(s.get(MetricKey::DiskTempAgeSeconds(0)), Some(90.0));
        assert_eq!(s.get(MetricKey::DiskWarningC(0)), Some(55.0));
        assert_eq!(s.get(MetricKey::DiskCriticalC(0)), Some(60.0));
    }

    #[test]
    fn a_sleeping_drive_is_not_read_and_is_asked_once_a_turn() {
        let rig = Rig::new([(0, hdd("A"))]);
        rig.drive(0, |d| d.power = Some(false));
        let mut source = rig.source();
        let first = sample(&mut source);
        assert_eq!(first.get(MetricKey::DiskAsleep(0)), Some(1.0));
        assert_eq!(first.get(MetricKey::DiskTempC(0)), None);
        rig.run(&mut source, 20 * MIN);
        assert_eq!(rig.temps(0), 0);
        assert_eq!(rig.powers(0), 4);
    }

    #[test]
    fn a_drive_that_falls_asleep_keeps_its_reading_and_is_read_again_on_waking() {
        let rig = Rig::new([(0, hdd("A"))]);
        let mut source = rig.source();
        sample(&mut source);
        rig.drive(0, |d| d.power = Some(false));
        rig.advance(5 * MIN);
        let asleep = sample(&mut source);
        assert_eq!(asleep.get(MetricKey::DiskAsleep(0)), Some(1.0));
        assert_eq!(asleep.get(MetricKey::DiskTempC(0)), Some(38.0));
        assert_eq!(rig.temps(0), 1);
        rig.drive(0, |d| d.power = Some(true));
        rig.advance(5 * MIN);
        let awake = sample(&mut source);
        assert_eq!(awake.get(MetricKey::DiskAsleep(0)), None);
        assert_eq!(rig.temps(0), 2);
    }

    #[test]
    fn an_unknown_power_state_is_not_asleep_and_waits_for_the_next_turn() {
        let rig = Rig::new([(0, hdd("A"))]);
        rig.drive(0, |d| d.power = None);
        let mut source = rig.source();
        let s = sample(&mut source);
        assert_eq!(s.get(MetricKey::DiskAsleep(0)), None);
        rig.run(&mut source, 5 * MIN - SEC);
        assert_eq!(rig.temps(0), 0, "not read while the state is unknown");
        assert_eq!(rig.powers(0), 1);
        rig.drive(0, |d| d.power = Some(true));
        rig.run(&mut source, 2 * SEC);
        assert_eq!(rig.temps(0), 1);
    }

    #[test]
    fn rediscovery_keeps_the_schedule_and_opens_only_new_drive_numbers() {
        let rig = Rig::new([(0, hdd("A"))]);
        let mut source = rig.source();
        rig.run(&mut source, 4 * MIN);
        assert_eq!(rig.calls.0.lock().unwrap().describes[&0], 1);
        assert_eq!(rig.temps(0), 1);

        rig.calls.0.lock().unwrap().drives.insert(3, hdd("B"));
        rig.run(&mut source, 61 * SEC);
        assert_eq!(rig.calls.0.lock().unwrap().describes[&3], 1);
        assert_eq!(rig.temps(3), 1, "the new drive has its own first turn");
        assert_eq!(rig.temps(0), 2, "the old one stays on its own schedule");
    }

    #[test]
    fn a_new_source_keeps_the_schedule_of_the_one_it_replaces() {
        let rig = Rig::new([(0, hdd("A"))]);
        let mut first = rig.source();
        sample(&mut first);
        drop(first);
        rig.advance(MIN);
        let mut second = rig.source();
        let s = sample(&mut second);
        assert_eq!(s.get(MetricKey::DiskTempC(0)), Some(38.0));
        assert_eq!(s.get(MetricKey::DiskTempAgeSeconds(0)), Some(60.0));
        assert_eq!(rig.temps(0), 1, "not read again before its turn");
    }

    #[test]
    fn the_schedule_follows_the_drive_and_not_its_number() {
        let rig = Rig::new([(1, hdd("A"))]);
        let mut source = rig.source();
        sample(&mut source);
        assert_eq!(rig.temps(1), 1);

        // A moves to number 2, leaving a dead handle on number 1.
        {
            let mut s = rig.calls.0.lock().unwrap();
            s.drives.insert(2, hdd("A"));
            s.drives.get_mut(&1).unwrap().power = None;
        }
        rig.advance(MIN);
        let moved = sample(&mut source);
        assert_eq!(rig.temps(2), 0, "A is not due yet under its new number");
        assert_eq!(moved.get(MetricKey::DiskTempAgeSeconds(2)), Some(60.0));
        assert_eq!(moved.get(MetricKey::DiskTempC(1)), None);

        // B takes the old number and is a new drive, due at once.
        rig.calls.0.lock().unwrap().drives.insert(1, hdd("B"));
        rig.advance(MIN);
        sample(&mut source);
        assert_eq!(rig.temps(1), 2);
        assert_eq!(rig.temps(2), 0);
    }

    #[test]
    fn drives_with_the_same_identity_keep_separate_schedules() {
        let rig = Rig::new([(0, hdd("SAME")), (1, hdd("SAME"))]);
        let mut source = rig.source();
        sample(&mut source);
        assert_eq!((rig.temps(0), rig.temps(1)), (1, 1));
    }

    #[test]
    fn a_drive_without_a_serial_number_is_known_by_its_number() {
        let mut drive = hdd("");
        drive.facts.serial = None;
        let rig = Rig::new([(0, drive.clone()), (1, drive)]);
        let mut source = rig.source();
        sample(&mut source);
        assert_eq!((rig.temps(0), rig.temps(1)), (1, 1));
    }

    #[test]
    fn a_drive_that_does_not_say_counts_as_rotational() {
        let mut unknown = hdd("A");
        unknown.facts.solid_state = None;
        let rig = Rig::new([(0, unknown)]);
        let mut source = rig.source();
        rig.run(&mut source, 10 * MIN);
        assert_eq!(rig.temps(0), 2);
    }

    #[test]
    fn a_solid_state_drive_is_read_every_ten_seconds() {
        let rig = Rig::new([(0, ssd("S"))]);
        let mut source = rig.source();
        assert_eq!(rig.temps(0), 1, "building the source probes it");
        rig.run(&mut source, 5 * MIN);
        assert_eq!(rig.temps(0), 30);
        assert_eq!(rig.powers(0), 0);
        let s = sample(&mut source);
        assert_eq!(s.get(MetricKey::DiskTempC(0)), Some(45.0));
        assert_eq!(s.get(MetricKey::DiskTempAgeSeconds(0)), None);
        assert_eq!(s.get(MetricKey::DiskAsleep(0)), None);
    }

    #[test]
    fn a_solid_state_drive_keeps_its_value_between_reads_but_not_after_a_failure() {
        let rig = Rig::new([(0, ssd("S"))]);
        let mut source = rig.source();
        assert_eq!(sample(&mut source).get(MetricKey::DiskTempC(0)), Some(45.0));
        rig.drive(0, |d| d.temp = Some((50, None, None)));
        rig.advance(9 * SEC);
        assert_eq!(sample(&mut source).get(MetricKey::DiskTempC(0)), Some(45.0));
        rig.advance(SEC);
        assert_eq!(sample(&mut source).get(MetricKey::DiskTempC(0)), Some(50.0));
        rig.drive(0, |d| d.temp = None);
        rig.advance(10 * SEC);
        assert_eq!(sample(&mut source).get(MetricKey::DiskTempC(0)), None);
        rig.drive(0, |d| d.temp = Some((48, None, None)));
        rig.advance(10 * SEC);
        assert_eq!(sample(&mut source).get(MetricKey::DiskTempC(0)), Some(48.0));
    }

    #[test]
    fn a_solid_state_drive_without_a_temperature_is_probed_each_rediscovery() {
        let mut silent = ssd("S");
        silent.temp = None;
        let rig = Rig::new([(0, silent)]);
        let mut source = rig.source();
        let mut out = Snapshot::default();
        assert!(source.sample(&mut out).is_err());
        rig.advance(MIN);
        assert!(source.sample(&mut out).is_err());
        assert_eq!(rig.temps(0), 2);
    }

    #[test]
    fn failed_reads_back_off_up_to_an_hour() {
        let rig = Rig::new([(0, hdd("A"))]);
        rig.drive(0, |d| d.temp = None);
        let mut source = rig.source();
        let mut read_at = Vec::new();
        let mut seen = 0;
        for minute in 0..200 {
            rig.run(&mut source, MIN);
            if rig.temps(0) > seen {
                seen = rig.temps(0);
                read_at.push(minute);
            }
        }
        assert_eq!(read_at, [0, 5, 15, 35, 75, 135, 195]);
    }

    #[test]
    fn an_empty_or_zero_read_backs_off_like_a_failure() {
        let rig = Rig::new([(0, hdd("A"))]);
        rig.drive(0, |d| d.temp = Some((0, None, None)));
        let mut source = rig.source();
        rig.run(&mut source, 14 * MIN);
        assert_eq!(rig.temps(0), 2, "at 0 and 5 minutes");
    }

    #[test]
    fn a_good_read_ends_the_back_off() {
        let rig = Rig::new([(0, hdd("A"))]);
        rig.drive(0, |d| d.temp = None);
        let mut source = rig.source();
        rig.run(&mut source, 6 * MIN);
        rig.drive(0, |d| d.temp = Some((38, None, None)));
        rig.run(&mut source, 10 * MIN);
        assert_eq!(rig.temps(0), 3, "at 0, 5 and 15 minutes");
        rig.run(&mut source, 5 * MIN);
        assert_eq!(rig.temps(0), 4, "back to every five minutes");
    }

    #[test]
    fn a_reading_counts_for_thirty_minutes() {
        let rig = Rig::new([(0, hdd("A")), (1, ssd("S"))]);
        let mut source = rig.source();
        sample(&mut source);
        rig.drive(0, |d| {
            d.temp = Some((70, None, None));
            d.power = None;
        });
        rig.drive(1, |d| d.temp = Some((30, None, None)));
        rig.advance(30 * MIN);
        let last = sample(&mut source);
        assert_eq!(last.get(MetricKey::DiskTempC(0)), Some(38.0));
        assert_eq!(last.get(MetricKey::DiskTempMaxC), Some(38.0));
        rig.advance(SEC);
        let gone = sample(&mut source);
        assert_eq!(gone.get(MetricKey::DiskTempC(0)), None);
        assert_eq!(gone.get(MetricKey::DiskTempAgeSeconds(0)), None);
        assert_eq!(gone.get(MetricKey::DiskTempMaxC), Some(30.0));
    }

    #[test]
    fn the_source_fails_only_once_no_drive_can_report() {
        let rig = Rig::new([(0, hdd("A"))]);
        let mut source = rig.source();
        rig.drive(0, |d| d.power = Some(false));
        assert!(source.sample(&mut Snapshot::default()).is_ok(), "asleep");
        rig.drive(0, |d| {
            d.power = Some(true);
            d.temp = None;
        });
        rig.advance(5 * MIN);
        assert!(
            source.sample(&mut Snapshot::default()).is_err(),
            "read and found nothing"
        );
    }

    #[test]
    fn a_process_that_yields_leaves_rotational_drives_alone() {
        let rig = Rig::new([(0, hdd("A")), (1, ssd("S"))]);
        rig.calls.0.lock().unwrap().yields = true;
        let mut source = rig.source();
        rig.run(&mut source, 10 * MIN);
        assert_eq!(rig.temps(0), 0);
        assert_eq!(rig.powers(0), 0);
        assert_eq!(rig.temps(1), 60, "solid state drives are not affected");

        rig.calls.0.lock().unwrap().yields = false;
        rig.run(&mut source, 5 * MIN);
        assert_eq!(rig.temps(0), 1);
    }

    #[test]
    fn a_turn_taken_by_one_caller_is_not_taken_by_another() {
        let id = DriveId {
            model: None,
            serial: None,
            number: Some(0),
        };
        let now = Instant::now();
        let mut schedule = Schedule::default();
        assert!(schedule.claim(&id, now));
        assert!(!schedule.claim(&id, now));
        assert!(schedule.claim(&id, now + ROTATIONAL_INTERVAL));
    }

    #[test]
    fn the_inventory_reads_no_rotational_temperature() {
        let rig = Rig::new([(0, hdd("A")), (1, ssd("S"))]);
        rig.drive(1, |d| d.temp = Some((45, Some(70), Some(80))));
        let mut calls = rig.calls.clone();

        let disks = inventory(&mut calls, &rig.schedule, &[(0, 'D')]);
        assert_eq!(rig.temps(0), 0);
        assert_eq!(rig.powers(0), 0);
        assert_eq!(rig.temps(1), 1, "solid state drives are asked as before");
        assert_eq!(disks[0].warning_c, None, "omitted before any reading");
        assert_eq!(disks[0].letters, ['D']);
        assert_eq!(disks[1].warning_c, Some(70));

        let mut source = rig.source();
        sample(&mut source);
        let before = rig.temps(0);
        let disks = inventory(&mut calls, &rig.schedule, &[]);
        assert_eq!(rig.temps(0), before);
        assert_eq!(disks[0].warning_c, Some(55), "from the last reading");
        assert_eq!(disks[0].critical_c, Some(60));
    }
}
