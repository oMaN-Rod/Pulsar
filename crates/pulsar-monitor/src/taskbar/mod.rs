pub mod hooks;
pub mod placement;
pub mod space;

use windows::Win32::Foundation::{CloseHandle, HANDLE, HWND, LPARAM, POINT, RECT};
use windows::Win32::Graphics::Gdi::{
    EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITOR_DEFAULTTONEAREST, MONITORINFO,
    MonitorFromPoint, MonitorFromWindow,
};
use windows::Win32::Security::{GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation};
use windows::Win32::System::Threading::{
    GetCurrentProcess, OpenProcess, OpenProcessToken, PROCESS_NAME_WIN32,
    PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows::Win32::UI::HiDpi::{
    AreDpiAwarenessContextsEqual, GetDpiForMonitor, GetDpiForWindow, GetWindowDpiAwarenessContext,
    MDT_EFFECTIVE_DPI,
};
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
    pub work: Rect32,
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

pub fn dpi_near(x: i32, y: i32) -> u32 {
    let monitor = unsafe { MonitorFromPoint(POINT { x, y }, MONITOR_DEFAULTTONEAREST) };
    let (mut dpi_x, mut dpi_y) = (0, 0);
    match unsafe { GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y) } {
        Ok(()) => dpi_x.max(96),
        Err(_) => 96,
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

/// The monitor showing most of a window, and its work area.
fn monitor_and_work(hwnd: HWND) -> (Rect32, Rect32) {
    let mut info = MONITORINFO {
        cbSize: size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    unsafe {
        let _ = GetMonitorInfoW(MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST), &mut info);
    }
    (rect32(info.rcMonitor), rect32(info.rcWork))
}

pub fn monitor_of(hwnd: HWND) -> Rect32 {
    monitor_and_work(hwnd).0
}

fn describe(hwnd: HWND, tray: Option<HWND>) -> Option<Taskbar> {
    let (monitor, work) = monitor_and_work(hwnd);
    Some(Taskbar {
        hwnd,
        tray,
        rect: rect_of(hwnd)?,
        tray_rect: tray.and_then(rect_of),
        monitor,
        work,
        dpi: unsafe { GetDpiForWindow(hwnd) }.max(96),
    })
}

/// The taskbar on the monitor called `chosen`, whatever order the taskbars were
/// found in; the primary comes first in `found` and stands in for a monitor
/// that has gone.
pub fn select_taskbar(
    found: Vec<Taskbar>,
    chosen: Option<&str>,
    monitor_name: impl Fn(&Taskbar) -> Option<String>,
) -> Vec<Taskbar> {
    let at = chosen
        .and_then(|name| {
            found
                .iter()
                .position(|t| monitor_name(t).as_deref() == Some(name))
        })
        .unwrap_or(0);
    found.into_iter().skip(at).take(1).collect()
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

pub fn is_elevated() -> bool {
    unsafe {
        let mut token = HANDLE::default();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).is_err() {
            return false;
        }
        let mut elevation = TOKEN_ELEVATION::default();
        let mut written = 0;
        let read = GetTokenInformation(
            token,
            TokenElevation,
            Some((&raw mut elevation).cast()),
            size_of::<TOKEN_ELEVATION>() as u32,
            &mut written,
        );
        let _ = CloseHandle(token);
        read.is_ok() && elevation.TokenIsElevated != 0
    }
}

/// Parenting across processes with different awareness forcibly resets the
/// awareness of the child's process.
pub fn same_dpi_awareness(a: HWND, b: HWND) -> bool {
    unsafe {
        AreDpiAwarenessContextsEqual(
            GetWindowDpiAwarenessContext(a),
            GetWindowDpiAwarenessContext(b),
        )
    }
    .as_bool()
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

pub fn image_path(pid: u32) -> Option<String> {
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

/// A captionless window covering its entire monitor (borderless or exclusive
/// fullscreen). With an auto-hide taskbar a maximised window covers the
/// monitor too, but its frame overhangs the edges; browsers keep the maximised
/// state in fullscreen yet fit the monitor exactly.
pub fn is_fullscreen(
    window: &Rect32,
    monitor: &Rect32,
    class: &str,
    maximized: bool,
    has_caption: bool,
) -> bool {
    let fits = if maximized {
        window == monitor
    } else {
        window.covers(monitor)
    };
    fits && !has_caption && !SHELL_CLASSES.contains(&class)
}

fn monitors() -> Vec<Rect32> {
    unsafe extern "system" fn visit(
        monitor: HMONITOR,
        _: HDC,
        _: *mut RECT,
        found: LPARAM,
    ) -> BOOL {
        let mut info = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        unsafe {
            if GetMonitorInfoW(monitor, &mut info).as_bool() {
                (*(found.0 as *mut Vec<Rect32>)).push(rect32(info.rcMonitor));
            }
        }
        true.into()
    }
    let mut found: Vec<Rect32> = Vec::new();
    unsafe {
        let _ = EnumDisplayMonitors(
            None,
            None,
            Some(visit),
            LPARAM(&mut found as *mut Vec<Rect32> as isize),
        );
    }
    found
}

/// The monitors whose taskbars should hide because a fullscreen app is in front.
pub fn fullscreen_monitors() -> Vec<Rect32> {
    let fg = unsafe { GetForegroundWindow() };
    if fg.is_invalid() || !unsafe { IsWindowVisible(fg) }.as_bool() {
        return Vec::new();
    }
    if image_path(process_id(fg)).is_some_and(|p| is_shell_host(&p)) {
        return Vec::new();
    }
    let Some(window) = rect_of(fg) else {
        return Vec::new();
    };
    let own = monitor_of(fg);
    let maximized = unsafe { IsZoomed(fg) }.as_bool();
    let style = unsafe { GetWindowLongW(fg, GWL_STYLE) } as u32;
    let has_caption = style & WS_CAPTION.0 == WS_CAPTION.0;
    let class = class_name(fg);
    let mut all = monitors();
    if !all.contains(&own) {
        all.push(own);
    }
    all.into_iter()
        .filter(|m| is_fullscreen(&window, m, &class, maximized && *m == own, has_caption))
        .collect()
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

    fn bar(id: usize) -> Taskbar {
        Taskbar {
            hwnd: HWND(id as *mut _),
            tray: None,
            rect: MONITOR,
            tray_rect: None,
            monitor: MONITOR,
            work: MONITOR,
            dpi: 96,
        }
    }

    fn name(t: &Taskbar) -> Option<String> {
        match t.hwnd.0 as usize {
            1 => Some("primary".into()),
            2 => Some("left".into()),
            3 => Some("right".into()),
            _ => None,
        }
    }

    fn selected(found: &[usize], chosen: Option<&str>) -> Vec<usize> {
        let found = found.iter().map(|&id| bar(id)).collect();
        select_taskbar(found, chosen, name)
            .iter()
            .map(|t| t.hwnd.0 as usize)
            .collect()
    }

    #[test]
    fn the_chosen_monitors_taskbar_is_found_in_any_z_order() {
        assert_eq!(selected(&[1, 2, 3], Some("right")), [3]);
        assert_eq!(selected(&[1, 3, 2], Some("right")), [3]);
        assert_eq!(selected(&[1, 3, 2], Some("left")), [2]);
        assert_eq!(selected(&[1, 2, 3], Some("primary")), [1]);
    }

    #[test]
    fn no_choice_or_a_gone_monitor_means_the_primary() {
        assert_eq!(selected(&[1, 2, 3], None), [1]);
        assert_eq!(selected(&[1, 2], Some("right")), [1]);
        assert_eq!(selected(&[], Some("right")), Vec::<usize>::new());
    }

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
    fn a_maximised_browser_in_fullscreen_is_fullscreen() {
        assert!(is_fullscreen(
            &MONITOR,
            &MONITOR,
            "Chrome_WidgetWin_1",
            true,
            false
        ));
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
