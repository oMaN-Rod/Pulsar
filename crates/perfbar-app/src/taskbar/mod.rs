pub mod hooks;
pub mod placement;

use windows::Win32::Foundation::{HWND, LPARAM, RECT};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow,
};
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, FindWindowExW, FindWindowW, GW_HWNDPREV, GetClassNameW, GetForegroundWindow,
    GetWindow, GetWindowRect, GetWindowThreadProcessId, IsWindowVisible,
};
use windows::core::{BOOL, w};

use placement::Rect32;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Taskbar {
    pub hwnd: HWND,
    pub tray: Option<HWND>,
    pub rect: Rect32,
    pub tray_rect: Option<Rect32>,
    pub monitor: Rect32,
    pub dpi: u32,
}

pub fn rect_of(hwnd: HWND) -> Option<Rect32> {
    let mut r = RECT::default();
    unsafe { GetWindowRect(hwnd, &mut r) }.ok()?;
    Some(Rect32 {
        left: r.left,
        top: r.top,
        right: r.right,
        bottom: r.bottom,
    })
}

fn monitor_of(hwnd: HWND) -> Rect32 {
    let mut info = MONITORINFO {
        cbSize: size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    unsafe {
        let _ = GetMonitorInfoW(MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST), &mut info);
    }
    let m = info.rcMonitor;
    Rect32 {
        left: m.left,
        top: m.top,
        right: m.right,
        bottom: m.bottom,
    }
}

fn describe(hwnd: HWND, tray: Option<HWND>) -> Option<Taskbar> {
    Some(Taskbar {
        hwnd,
        tray,
        rect: rect_of(hwnd)?,
        tray_rect: tray.and_then(rect_of),
        monitor: monitor_of(hwnd),
        dpi: unsafe { GetDpiForWindow(hwnd) }.max(96),
    })
}

pub fn class_name(hwnd: HWND) -> String {
    let mut buf = [0u16; 64];
    let len = unsafe { GetClassNameW(hwnd, &mut buf) }.max(0) as usize;
    String::from_utf16_lossy(&buf[..len])
}

/// The primary taskbar first, then secondary taskbars when requested.
pub fn discover(include_secondary: bool) -> Vec<Taskbar> {
    let mut found = Vec::new();
    if let Ok(primary) = unsafe { FindWindowW(w!("Shell_TrayWnd"), None) } {
        let tray = unsafe { FindWindowExW(Some(primary), None, w!("TrayNotifyWnd"), None) }.ok();
        found.extend(describe(primary, tray));
    }
    if include_secondary {
        let mut secondary: Vec<HWND> = Vec::new();
        unsafe extern "system" fn collect(hwnd: HWND, out: LPARAM) -> BOOL {
            if class_name(hwnd) == "Shell_SecondaryTrayWnd" {
                unsafe { (*(out.0 as *mut Vec<HWND>)).push(hwnd) };
            }
            BOOL(1)
        }
        unsafe {
            let _ = EnumWindows(
                Some(collect),
                LPARAM(&mut secondary as *mut Vec<HWND> as isize),
            );
        }
        found.extend(secondary.into_iter().filter_map(|h| describe(h, None)));
    }
    found
}

pub fn process_id(hwnd: HWND) -> u32 {
    let mut pid = 0;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    pid
}

/// True when `taskbar` is above `overlay` in z-order, i.e. covering it.
pub fn is_behind(overlay: HWND, taskbar: HWND) -> bool {
    let mut current = unsafe { GetWindow(overlay, GW_HWNDPREV) }.ok();
    while let Some(h) = current {
        if h == taskbar {
            return true;
        }
        current = unsafe { GetWindow(h, GW_HWNDPREV) }.ok();
    }
    false
}

const SHELL_CLASSES: [&str; 4] = [
    "Progman",
    "WorkerW",
    "Shell_TrayWnd",
    "Shell_SecondaryTrayWnd",
];

/// A window covering its entire monitor (borderless or exclusive
/// fullscreen). Maximised windows stop at the work area and do not qualify.
pub fn is_fullscreen(window: &Rect32, monitor: &Rect32, class: &str) -> bool {
    !SHELL_CLASSES.contains(&class) && window.covers(monitor)
}

/// The monitor whose taskbar should hide because a fullscreen app is in front.
pub fn fullscreen_monitor() -> Option<Rect32> {
    let fg = unsafe { GetForegroundWindow() };
    if fg.is_invalid() || !unsafe { IsWindowVisible(fg) }.as_bool() {
        return None;
    }
    let monitor = monitor_of(fg);
    is_fullscreen(&rect_of(fg)?, &monitor, &class_name(fg)).then_some(monitor)
}

#[cfg(test)]
mod tests {
    use super::*;

    const MONITOR: Rect32 = Rect32 {
        left: 0,
        top: 0,
        right: 1920,
        bottom: 1080,
    };

    #[test]
    fn fullscreen_requires_covering_the_monitor() {
        assert!(is_fullscreen(&MONITOR, &MONITOR, "Chrome_WidgetWin_1"));
        let bigger = Rect32 {
            left: -8,
            top: -8,
            right: 1928,
            bottom: 1088,
        };
        assert!(is_fullscreen(&bigger, &MONITOR, "UnityWndClass"));
        let maximised = Rect32 {
            left: -8,
            top: -8,
            right: 1928,
            bottom: 1040,
        };
        assert!(!is_fullscreen(&maximised, &MONITOR, "Notepad"));
    }

    #[test]
    fn desktop_and_shell_are_never_fullscreen() {
        for class in SHELL_CLASSES {
            assert!(!is_fullscreen(&MONITOR, &MONITOR, class), "{class}");
        }
    }

    #[test]
    fn discovers_the_primary_taskbar_with_its_tray() {
        let found = discover(false);
        let primary = found.first().expect("Shell_TrayWnd exists");
        assert_eq!(class_name(primary.hwnd), "Shell_TrayWnd");
        assert!(primary.rect.width() > 0 && primary.rect.height() > 0);
        assert!(primary.tray.is_some());
        assert!(primary.dpi >= 96);
    }
}
