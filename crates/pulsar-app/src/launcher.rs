use std::io;
use std::process::Command;

use pulsar_core::ipc::{ABOUT_ARG, SETTINGS_EXE, sibling};

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
