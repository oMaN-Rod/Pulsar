//! The system battery: charge and time left from the power status, and
//! rate, capacity and wear from the first battery device.

use std::time::{Duration, Instant};

use windows::Win32::Devices::DeviceAndDriverInstallation::{
    DIGCF_DEVICEINTERFACE, DIGCF_PRESENT, SP_DEVICE_INTERFACE_DATA,
    SP_DEVICE_INTERFACE_DETAIL_DATA_W, SetupDiDestroyDeviceInfoList, SetupDiEnumDeviceInterfaces,
    SetupDiGetClassDevsW, SetupDiGetDeviceInterfaceDetailW,
};
use windows::Win32::Foundation::{CloseHandle, GENERIC_READ, GENERIC_WRITE, HANDLE};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows::Win32::System::IO::DeviceIoControl;
use windows::Win32::System::Power::{
    BATTERY_INFORMATION, BATTERY_QUERY_INFORMATION, BATTERY_STATUS, BATTERY_UNKNOWN_CAPACITY,
    BATTERY_UNKNOWN_RATE, BATTERY_WAIT_STATUS, BatteryInformation, GUID_DEVICE_BATTERY,
    GetSystemPowerStatus, IOCTL_BATTERY_QUERY_INFORMATION, IOCTL_BATTERY_QUERY_STATUS,
    IOCTL_BATTERY_QUERY_TAG, SYSTEM_POWER_STATUS,
};
use windows::core::PCWSTR;

use crate::metric::{MetricKey, Snapshot, Source, SourceError, SourceId};

/// `BatteryFlag` bits.
const CHARGING: u8 = 8;
const NO_SYSTEM_BATTERY: u8 = 128;
const UNKNOWN_STATUS: u8 = 255;
/// `BatteryLifeTime` when Windows cannot estimate it.
const UNKNOWN_TIME: u32 = u32::MAX;
/// `BATTERY_INFORMATION::Capabilities`: capacities are percentages, not mWh.
const RELATIVE_CAPACITY: u32 = 0x4000_0000;

const SEARCH_AGAIN: Duration = Duration::from_secs(60);

#[derive(Default)]
pub struct BatterySource {
    device: Option<Device>,
    search_at: Option<Instant>,
}

impl Source for BatterySource {
    fn id(&self) -> SourceId {
        SourceId::Battery
    }

    fn sample(&mut self, out: &mut Snapshot) -> Result<(), SourceError> {
        let mut status = SYSTEM_POWER_STATUS::default();
        unsafe { GetSystemPowerStatus(&mut status)? };
        let reading = PowerStatus::from(&status)
            .ok_or_else(|| SourceError::Unavailable(crate::tr!("This PC has no battery").into()))?;
        reading.write(out);
        let now = Instant::now();
        if self.device.is_none() && self.search_at.is_none_or(|t| now >= t) {
            self.device = Device::first();
            self.search_at = Some(now + SEARCH_AGAIN);
        }
        if let Some(device) = &self.device {
            match device.read() {
                Some(details) => details.write(out),
                None => self.device = None,
            }
        }
        Ok(())
    }
}

/// What `GetSystemPowerStatus` tells about the battery.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PowerStatus {
    pub percent: Option<f64>,
    pub plugged_in: bool,
    pub charging: bool,
    pub seconds_left: Option<f64>,
}

impl PowerStatus {
    /// `None` when the system has no battery.
    pub fn from(s: &SYSTEM_POWER_STATUS) -> Option<Self> {
        if s.BatteryFlag != UNKNOWN_STATUS && s.BatteryFlag & NO_SYSTEM_BATTERY != 0 {
            return None;
        }
        Some(Self {
            percent: (s.BatteryLifePercent <= 100).then(|| f64::from(s.BatteryLifePercent)),
            plugged_in: s.ACLineStatus == 1,
            charging: s.BatteryFlag != UNKNOWN_STATUS && s.BatteryFlag & CHARGING != 0,
            seconds_left: (s.BatteryLifeTime != UNKNOWN_TIME).then(|| f64::from(s.BatteryLifeTime)),
        })
    }

    fn write(&self, out: &mut Snapshot) {
        if let Some(p) = self.percent {
            out.set(MetricKey::BatteryPercent, p);
        }
        out.set(
            MetricKey::BatteryPluggedIn,
            f64::from(u8::from(self.plugged_in)),
        );
        out.set(
            MetricKey::BatteryCharging,
            f64::from(u8::from(self.charging)),
        );
        if let Some(s) = self.seconds_left {
            out.set(MetricKey::BatterySecondsLeft, s);
        }
    }
}

/// What the battery device reports, in watts and watt-hours.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BatteryDetails {
    /// Positive while charging, negative while discharging.
    pub rate_w: Option<f64>,
    pub remaining_wh: Option<f64>,
    pub full_wh: Option<f64>,
    pub design_wh: Option<f64>,
    pub cycles: Option<f64>,
    pub volts: Option<f64>,
}

impl BatteryDetails {
    pub fn from(info: &BATTERY_INFORMATION, status: &BATTERY_STATUS) -> Self {
        let absolute = info.Capabilities & RELATIVE_CAPACITY == 0;
        let wh = |mwh: u32| {
            (absolute && mwh != 0 && mwh != BATTERY_UNKNOWN_CAPACITY)
                .then(|| f64::from(mwh) / 1000.0)
        };
        Self {
            rate_w: (absolute && status.Rate as u32 != BATTERY_UNKNOWN_RATE)
                .then(|| f64::from(status.Rate) / 1000.0),
            remaining_wh: wh(status.Capacity),
            full_wh: wh(info.FullChargedCapacity),
            design_wh: wh(info.DesignedCapacity),
            cycles: (info.CycleCount > 0).then(|| f64::from(info.CycleCount)),
            volts: (status.Voltage != 0 && status.Voltage != BATTERY_UNKNOWN_CAPACITY)
                .then(|| f64::from(status.Voltage) / 1000.0),
        }
    }

    /// Full charge capacity as a share of the design capacity.
    pub fn health_percent(&self) -> Option<f64> {
        match (self.full_wh, self.design_wh) {
            (Some(full), Some(design)) if design > 0.0 => Some((full / design * 100.0).min(100.0)),
            _ => None,
        }
    }

    fn write(&self, out: &mut Snapshot) {
        for (key, value) in [
            (MetricKey::BatteryRateW, self.rate_w),
            (MetricKey::BatteryRemainingWh, self.remaining_wh),
            (MetricKey::BatteryFullWh, self.full_wh),
            (MetricKey::BatteryDesignWh, self.design_wh),
            (MetricKey::BatteryCycles, self.cycles),
            (MetricKey::BatteryVolts, self.volts),
            (MetricKey::BatteryHealthPercent, self.health_percent()),
        ] {
            if let Some(v) = value {
                out.set(key, v);
            }
        }
    }
}

struct Device {
    handle: HANDLE,
    tag: u32,
}

// The handle is only used from the sampler thread that owns the source.
unsafe impl Send for Device {}

impl Drop for Device {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.handle);
        }
    }
}

impl Device {
    fn first() -> Option<Self> {
        interface_paths()
            .iter()
            .filter_map(|path| Self::open(path))
            .find(|device| device.read().is_some())
    }

    fn open(path: &[u16]) -> Option<Self> {
        let handle = unsafe {
            CreateFileW(
                PCWSTR(path.as_ptr()),
                (GENERIC_READ | GENERIC_WRITE).0,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                None,
                OPEN_EXISTING,
                FILE_ATTRIBUTE_NORMAL,
                None,
            )
        }
        .ok()?;
        let mut device = Self { handle, tag: 0 };
        let wait = 0u32;
        let tag: u32 = device.control(IOCTL_BATTERY_QUERY_TAG, &wait)?;
        (tag != 0).then_some(())?;
        device.tag = tag;
        Some(device)
    }

    fn control<I, O: Default>(&self, code: u32, input: &I) -> Option<O> {
        let mut output = O::default();
        let mut returned = 0u32;
        unsafe {
            DeviceIoControl(
                self.handle,
                code,
                Some((input as *const I).cast()),
                size_of::<I>() as u32,
                Some((&mut output as *mut O).cast()),
                size_of::<O>() as u32,
                Some(&mut returned),
                None,
            )
        }
        .ok()?;
        (returned as usize == size_of::<O>()).then_some(output)
    }

    fn read(&self) -> Option<BatteryDetails> {
        let query = BATTERY_QUERY_INFORMATION {
            BatteryTag: self.tag,
            InformationLevel: BatteryInformation,
            AtRate: 0,
        };
        let info: BATTERY_INFORMATION = self.control(IOCTL_BATTERY_QUERY_INFORMATION, &query)?;
        let wait = BATTERY_WAIT_STATUS {
            BatteryTag: self.tag,
            ..Default::default()
        };
        let status: BATTERY_STATUS = self.control(IOCTL_BATTERY_QUERY_STATUS, &wait)?;
        Some(BatteryDetails::from(&info, &status))
    }
}

fn interface_paths() -> Vec<Vec<u16>> {
    let mut paths = Vec::new();
    unsafe {
        let Ok(set) = SetupDiGetClassDevsW(
            Some(&GUID_DEVICE_BATTERY),
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
            if SetupDiEnumDeviceInterfaces(set, None, &GUID_DEVICE_BATTERY, index, &mut data)
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

    fn status(ac: u8, flag: u8, percent: u8, life: u32) -> SYSTEM_POWER_STATUS {
        SYSTEM_POWER_STATUS {
            ACLineStatus: ac,
            BatteryFlag: flag,
            BatteryLifePercent: percent,
            BatteryLifeTime: life,
            ..Default::default()
        }
    }

    #[test]
    fn a_desktop_has_no_battery() {
        assert_eq!(
            PowerStatus::from(&status(1, NO_SYSTEM_BATTERY, 255, UNKNOWN_TIME)),
            None
        );
    }

    #[test]
    fn reads_charge_plug_and_time_left() {
        let s = PowerStatus::from(&status(0, 1, 76, 5400)).unwrap();
        assert_eq!(
            s,
            PowerStatus {
                percent: Some(76.0),
                plugged_in: false,
                charging: false,
                seconds_left: Some(5400.0)
            }
        );
        let charging = PowerStatus::from(&status(1, 8, 40, UNKNOWN_TIME)).unwrap();
        assert!(charging.plugged_in && charging.charging);
        assert_eq!(charging.seconds_left, None);
    }

    #[test]
    fn unknown_status_still_counts_as_a_battery() {
        let s = PowerStatus::from(&status(255, UNKNOWN_STATUS, 255, UNKNOWN_TIME)).unwrap();
        assert_eq!(s.percent, None);
        assert!(!s.charging);
    }

    #[test]
    fn device_values_are_watts_and_watt_hours() {
        let info = BATTERY_INFORMATION {
            DesignedCapacity: 57_000,
            FullChargedCapacity: 51_300,
            CycleCount: 212,
            ..Default::default()
        };
        let status = BATTERY_STATUS {
            PowerState: 0,
            Capacity: 30_000,
            Voltage: 11_850,
            Rate: -9_500,
        };
        let d = BatteryDetails::from(&info, &status);
        assert_eq!(d.rate_w, Some(-9.5));
        assert_eq!(d.remaining_wh, Some(30.0));
        assert_eq!(d.cycles, Some(212.0));
        assert_eq!(d.volts, Some(11.85));
        assert!((d.health_percent().unwrap() - 90.0).abs() < 1e-9);
    }

    #[test]
    fn relative_and_unknown_capacities_are_left_out() {
        let info = BATTERY_INFORMATION {
            Capabilities: RELATIVE_CAPACITY,
            DesignedCapacity: 100,
            FullChargedCapacity: 100,
            ..Default::default()
        };
        let status = BATTERY_STATUS {
            Capacity: BATTERY_UNKNOWN_CAPACITY,
            Rate: BATTERY_UNKNOWN_RATE as i32,
            ..Default::default()
        };
        let d = BatteryDetails::from(&info, &status);
        assert_eq!((d.full_wh, d.rate_w, d.remaining_wh), (None, None, None));
        assert_eq!(d.health_percent(), None);
    }

    #[test]
    fn sampling_never_panics_with_or_without_a_battery() {
        let mut source = BatterySource::default();
        let mut snapshot = Snapshot::default();
        match source.sample(&mut snapshot) {
            Ok(()) => assert!(snapshot.get(MetricKey::BatteryPluggedIn).is_some()),
            Err(e) => assert!(!e.to_string().is_empty()),
        }
    }
}
