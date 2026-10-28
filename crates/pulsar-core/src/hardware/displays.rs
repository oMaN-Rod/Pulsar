//! The displays Windows is driving, and which one shows a window.

use windows::Win32::Devices::Display::{
    DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME, DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME,
    DISPLAYCONFIG_DEVICE_INFO_HEADER, DISPLAYCONFIG_MODE_INFO, DISPLAYCONFIG_PATH_INFO,
    DISPLAYCONFIG_SOURCE_DEVICE_NAME, DISPLAYCONFIG_TARGET_DEVICE_NAME, DisplayConfigGetDeviceInfo,
    GetDisplayConfigBufferSizes, QDC_ONLY_ACTIVE_PATHS, QueryDisplayConfig,
};
use windows::Win32::Foundation::{ERROR_SUCCESS, HWND, LUID};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MONITORINFOEXW, MonitorFromWindow,
};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Display {
    pub adapter_luid: u64,
    /// The adapter's video output.
    pub source: u32,
    /// Such as `\\.\DISPLAY1`.
    pub gdi_name: String,
    /// The monitor's own name, or the GDI name where it has none.
    pub name: String,
    /// Stable across restarts while the monitor stays on the same port.
    pub path: String,
}

pub fn luid_value(luid: LUID) -> u64 {
    (u64::from(luid.HighPart as u32) << 32) | u64::from(luid.LowPart)
}

fn text(wide: &[u16]) -> String {
    let len = wide.iter().position(|&c| c == 0).unwrap_or(wide.len());
    String::from_utf16_lossy(&wide[..len])
}

fn header<T>(kind: i32, adapter: LUID, id: u32) -> DISPLAYCONFIG_DEVICE_INFO_HEADER {
    DISPLAYCONFIG_DEVICE_INFO_HEADER {
        r#type: windows::Win32::Devices::Display::DISPLAYCONFIG_DEVICE_INFO_TYPE(kind),
        size: size_of::<T>() as u32,
        adapterId: adapter,
        id,
    }
}

/// One entry per output; a cloned output lists once.
pub fn active() -> Vec<Display> {
    let (mut path_count, mut mode_count) = (0, 0);
    let sized = unsafe {
        GetDisplayConfigBufferSizes(QDC_ONLY_ACTIVE_PATHS, &mut path_count, &mut mode_count)
    };
    if sized != ERROR_SUCCESS {
        return Vec::new();
    }
    let mut paths = vec![DISPLAYCONFIG_PATH_INFO::default(); path_count as usize];
    let mut modes = vec![DISPLAYCONFIG_MODE_INFO::default(); mode_count as usize];
    let queried = unsafe {
        QueryDisplayConfig(
            QDC_ONLY_ACTIVE_PATHS,
            &mut path_count,
            paths.as_mut_ptr(),
            &mut mode_count,
            modes.as_mut_ptr(),
            None,
        )
    };
    if queried != ERROR_SUCCESS {
        return Vec::new();
    }
    paths.truncate(path_count as usize);
    let mut displays: Vec<Display> = Vec::new();
    for p in &paths {
        let (adapter, source) = (p.sourceInfo.adapterId, p.sourceInfo.id);
        let adapter_luid = luid_value(adapter);
        if displays
            .iter()
            .any(|d| d.adapter_luid == adapter_luid && d.source == source)
        {
            continue;
        }
        let mut source_name = DISPLAYCONFIG_SOURCE_DEVICE_NAME {
            header: header::<DISPLAYCONFIG_SOURCE_DEVICE_NAME>(
                DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME.0,
                adapter,
                source,
            ),
            ..Default::default()
        };
        let mut target = DISPLAYCONFIG_TARGET_DEVICE_NAME {
            header: header::<DISPLAYCONFIG_TARGET_DEVICE_NAME>(
                DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME.0,
                p.targetInfo.adapterId,
                p.targetInfo.id,
            ),
            ..Default::default()
        };
        unsafe {
            DisplayConfigGetDeviceInfo(&mut source_name.header);
            DisplayConfigGetDeviceInfo(&mut target.header);
        }
        let gdi_name = text(&source_name.viewGdiDeviceName);
        let name = text(&target.monitorFriendlyDeviceName);
        displays.push(Display {
            adapter_luid,
            source,
            name: if name.is_empty() {
                gdi_name.clone()
            } else {
                name
            },
            gdi_name,
            path: text(&target.monitorDevicePath),
        });
    }
    displays
}

/// The GDI name of the monitor showing most of the window.
pub fn gdi_name_of(window: HWND) -> Option<String> {
    let mut info = MONITORINFOEXW {
        monitorInfo: MONITORINFO {
            cbSize: size_of::<MONITORINFOEXW>() as u32,
            ..Default::default()
        },
        ..Default::default()
    };
    let ok = unsafe {
        GetMonitorInfoW(
            MonitorFromWindow(window, MONITOR_DEFAULTTONEAREST),
            &mut info.monitorInfo,
        )
    };
    ok.as_bool().then(|| text(&info.szDevice))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_luid_reads_as_one_number_high_part_first() {
        let luid = LUID {
            LowPart: 0x0001_E2F3,
            HighPart: 0,
        };
        assert_eq!(luid_value(luid), 0x0001_E2F3);
        let luid = LUID {
            LowPart: 1,
            HighPart: -1,
        };
        assert_eq!(luid_value(luid), 0xFFFF_FFFF_0000_0001);
    }

    #[test]
    fn every_active_display_has_a_gdi_name() {
        for d in active() {
            assert!(d.gdi_name.starts_with(r"\\.\DISPLAY"), "{d:?}");
            assert!(!d.name.is_empty());
        }
    }
}
