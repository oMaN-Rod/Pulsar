//! Physical drives, asked through storage queries that need no access
//! rights to the disk itself.

use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAGS_AND_ATTRIBUTES, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows::Win32::System::IO::DeviceIoControl;
use windows::core::HSTRING;

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

/// Current temperature and the warning and critical thresholds, in °C.
fn temperatures(bytes: &[u8]) -> Option<(i16, Option<i16>, Option<i16>)> {
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

pub fn read() -> Vec<DiskInfo> {
    let volumes: Vec<(u32, char)> = fixed_drives()
        .into_iter()
        .filter_map(|letter| Some((disk_of_volume(letter)?, letter as char)))
        .collect();
    (0..MAX_DRIVES)
        .filter_map(|number| {
            let drive = physical(number)?;
            let (model, firmware, bus, removable) =
                describe(&property(&drive, DEVICE_PROPERTY, 1024)?)?;
            let solid_state = property(&drive, SEEK_PENALTY_PROPERTY, 12)
                .and_then(|b| b.get(8).map(|&seek| seek == 0));
            let size_bytes = control(&drive, IOCTL_DISK_GET_DRIVE_GEOMETRY_EX, &[], 256)
                .and_then(|g| Some(u64::from_le_bytes(g.get(24..32)?.try_into().ok()?)))
                .unwrap_or(0);
            let thresholds = property(&drive, TEMPERATURE_PROPERTY, 256)
                .as_deref()
                .and_then(temperatures);
            Some(DiskInfo {
                number,
                model,
                firmware,
                bus,
                solid_state,
                size_bytes,
                removable,
                letters: volumes
                    .iter()
                    .filter(|(disk, _)| *disk == number)
                    .map(|(_, letter)| *letter)
                    .collect(),
                warning_c: thresholds.and_then(|t| t.1),
                critical_c: thresholds.and_then(|t| t.2),
            })
        })
        .collect()
}

/// Drive temperatures, for drives that report one.
pub struct DriveTempSource {
    drives: Vec<(u8, Handle)>,
}

// The handles are only used from the sampler thread that owns the source.
unsafe impl Send for DriveTempSource {}

impl DriveTempSource {
    pub fn new() -> Self {
        let drives = (0..MAX_DRIVES)
            .filter_map(|n| {
                let drive = physical(n)?;
                property(&drive, TEMPERATURE_PROPERTY, 256)
                    .as_deref()
                    .and_then(temperatures)?;
                Some((n as u8, drive))
            })
            .collect();
        Self { drives }
    }
}

impl Default for DriveTempSource {
    fn default() -> Self {
        Self::new()
    }
}

impl Source for DriveTempSource {
    fn id(&self) -> SourceId {
        SourceId::DriveTemps
    }

    fn sample(&mut self, out: &mut Snapshot) -> Result<(), SourceError> {
        if self.drives.is_empty() {
            return Err(SourceError::Unavailable(
                crate::tr!("No drive reports a temperature").into(),
            ));
        }
        let mut hottest = None;
        for (number, drive) in &self.drives {
            if let Some((temp, _, _)) = property(drive, TEMPERATURE_PROPERTY, 256)
                .as_deref()
                .and_then(temperatures)
                && temp > 0
            {
                let temp = f64::from(temp);
                out.set(MetricKey::DiskTempC(*number), temp);
                hottest = Some(hottest.map_or(temp, |h: f64| h.max(temp)));
            }
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
}
