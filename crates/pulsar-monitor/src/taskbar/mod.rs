pub mod hooks;
pub mod placement;

use windows::Win32::Foundation::{CloseHandle, HWND, LPARAM, POINT, RECT};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint, MonitorFromWindow,
};
use windows::Win32::System::Threading::{
    OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, FindWindowExW, FindWindowW, GW_HWNDPREV, GWL_STYLE, GetClassNameW,
    GetForegroundWindow, GetWindow, GetWindowLongW, GetWindowRect, GetWindowThreadProcessId,
    IsWindowVisible, IsZoomed, WS_CAPTION,
};
use windows::core::{BOOL, PWSTR, w};

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

fn rect32(r: RECT) -> Rect32 {
    Rect32 {
        left: r.left,
        top: r.top,
        right: r.right,
        bottom: r.bottom,
    }
}

/// The monitor, and its work area, nearest a screen point.
pub fn monitor_near(x: i32, y: i32) -> (Rect32, Rect32) {
    let mut info = MONITORINFO {
        cbSize: size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    unsafe {
        let _ = GetMonitorInfoW(
            MonitorFromPoint(POINT { x, y }, MONITOR_DEFAULTTONEAREST),
            &mut info,
        );
    }
    (rect32(info.rcMonitor), rect32(info.rcWork))
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

/// The desktop, taskbars, and shell surfaces that cover a monitor briefly
/// (Task View, Start, Search, Alt+Tab).
const SHELL_CLASSES: [&str; 8] = [
    "Progman",
    "WorkerW",
    "Shell_TrayWnd",
    "Shell_SecondaryTrayWnd",
    "MultitaskingViewFrame",
    "XamlExplorerHostIslandWindow",
    "ForegroundStaging",
    "TopLevelWindowForOverflowXamlIsland",
];

/// Start, Search and the notification flyouts are CoreWindows like any UWP
/// app; only their host process tells them apart.
const SHELL_HOSTS: [&str; 3] = [
    "startmenuexperiencehost.exe",
    "searchhost.exe",
    "shellexperiencehost.exe",
];

pub fn is_shell_host(image_path: &str) -> bool {
    let name = image_path
        .rsplit('\\')
        .next()
        .unwrap_or(image_path)
        .to_ascii_lowercase();
    SHELL_HOSTS.contains(&name.as_str())
}

fn image_path(pid: u32) -> Option<String> {
    unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = [0u16; 512];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_WIN32,
            PWSTR(buf.as_mut_ptr()),
            &mut len,
        );
        let _ = CloseHandle(process);
        ok.ok()?;
        Some(String::from_utf16_lossy(&buf[..len as usize]))
    }
}

/// A non-maximised, captionless window covering its entire monitor
/// (borderless or exclusive fullscreen). Maximised windows are excluded: with
/// an auto-hide taskbar their rect also covers the monitor.
pub fn is_fullscreen(
    window: &Rect32,
    monitor: &Rect32,
    class: &str,
    maximized: bool,
    has_caption: bool,
) -> bool {
    !maximized && !has_caption && !SHELL_CLASSES.contains(&class) && window.covers(monitor)
}

/// The monitor whose taskbar should hide because a fullscreen app is in front.
pub fn fullscreen_monitor() -> Option<Rect32> {
    let fg = unsafe { GetForegroundWindow() };
    if fg.is_invalid() || !unsafe { IsWindowVisible(fg) }.as_bool() {
        return None;
    }
    let monitor = monitor_of(fg);
    let maximized = unsafe { IsZoomed(fg) }.as_bool();
    let style = unsafe { GetWindowLongW(fg, GWL_STYLE) } as u32;
    let has_caption = style & WS_CAPTION.0 == WS_CAPTION.0;
    if image_path(process_id(fg)).is_some_and(|p| is_shell_host(&p)) {
        return None;
    }
    is_fullscreen(
        &rect_of(fg)?,
        &monitor,
        &class_name(fg),
        maximized,
        has_caption,
    )
    .then_some(monitor)
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
        assert!(is_fullscreen(
            &MONITOR,
            &MONITOR,
            "Chrome_WidgetWin_1",
            false,
            false
        ));
        let bigger = Rect32 {
            left: -8,
            top: -8,
            right: 1928,
            bottom: 1088,
        };
        assert!(is_fullscreen(
            &bigger,
            &MONITOR,
            "UnityWndClass",
            false,
            false
        ));
        let maximised = Rect32 {
            left: -8,
            top: -8,
            right: 1928,
            bottom: 1040,
        };
        assert!(!is_fullscreen(&maximised, &MONITOR, "Notepad", true, false));
    }

    #[test]
    fn maximised_window_is_not_fullscreen_with_auto_hide_taskbar() {
        // With auto-hide the work area is the whole monitor, so a maximised
        // window's rect covers it too.
        let maximised = Rect32 {
            left: -8,
            top: -8,
            right: 1928,
            bottom: 1088,
        };
        assert!(!is_fullscreen(&maximised, &MONITOR, "Notepad", true, false));
    }

    #[test]
    fn desktop_and_shell_are_never_fullscreen() {
        for class in SHELL_CLASSES {
            assert!(
                !is_fullscreen(&MONITOR, &MONITOR, class, false, false),
                "{class}"
            );
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

    #[test]
    fn shell_surfaces_are_never_fullscreen() {
        for class in [
            "MultitaskingViewFrame",
            "XamlExplorerHostIslandWindow",
            "ForegroundStaging",
        ] {
            assert!(
                !is_fullscreen(&MONITOR, &MONITOR, class, false, false),
                "{class}"
            );
        }
    }

    #[test]
    fn a_captioned_window_is_not_fullscreen() {
        assert!(!is_fullscreen(&MONITOR, &MONITOR, "Notepad", false, true));
    }

    #[test]
    fn a_fullscreen_corewindow_app_hides_the_overlay() {
        assert!(is_fullscreen(
            &MONITOR,
            &MONITOR,
            "Windows.UI.Core.CoreWindow",
            false,
            false
        ));
    }

    #[test]
    fn start_and_search_hosts_are_shell() {
        assert!(is_shell_host(
            r"C:\Windows\SystemApps\X\StartMenuExperienceHost.exe"
        ));
        assert!(is_shell_host(r"C:\Windows\SystemApps\X\searchhost.exe"));
        assert!(is_shell_host("ShellExperienceHost.exe"));
        assert!(!is_shell_host(r"C:\Games\game.exe"));
    }
}
