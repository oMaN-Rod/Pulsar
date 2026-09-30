use windows::Win32::UI::WindowsAndMessaging::WM_APP;

pub const WM_APP_SNAPSHOT: u32 = WM_APP + 1;
pub const WM_APP_TRAY: u32 = WM_APP + 2;
pub const WM_APP_TASKBAR: u32 = WM_APP + 3;
pub const WM_APP_FOREGROUND: u32 = WM_APP + 4;
