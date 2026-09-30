use std::io;
use std::process::Command;

use pulsar_core::ipc::{ABOUT_ARG, SETTINGS_EXE, sibling};
use windows::Win32::UI::WindowsAndMessaging::{ASFW_ANY, AllowSetForegroundWindow};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    General,
    About,
}

pub fn args(page: Page) -> &'static [&'static str] {
    match page {
        Page::General => &[],
        Page::About => &[ABOUT_ARG],
    }
}

/// Starts `pulsar-settings.exe` from the same folder as this executable.
/// The settings process focuses an already open window itself.
pub fn open(page: Page) -> io::Result<()> {
    let exe = sibling(&std::env::current_exe()?, SETTINGS_EXE);
    // Tray clicks and menu picks give this process the right to take the
    // foreground; pass it on so the settings window is not left behind.
    unsafe {
        let _ = AllowSetForegroundWindow(ASFW_ANY);
    }
    Command::new(exe).args(args(page)).spawn().map(drop)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn about_page_is_requested_by_argument() {
        assert!(args(Page::General).is_empty());
        assert_eq!(args(Page::About), [ABOUT_ARG]);
    }
}
