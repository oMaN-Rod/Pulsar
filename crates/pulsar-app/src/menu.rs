use pulsar_core::config::DisplayMode;
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, DestroyMenu, MF_CHECKED, MF_SEPARATOR, MF_STRING, PostMessageW,
    SW_SHOWNORMAL, SetForegroundWindow, TPM_BOTTOMALIGN, TPM_NONOTIFY, TPM_RETURNCMD,
    TPM_RIGHTBUTTON, TrackPopupMenu, WM_NULL,
};
use windows::core::{HSTRING, PCWSTR, w};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    Mode(DisplayMode),
    Settings,
    About,
    TaskManager,
    Exit,
}

const ID_GRAPHS: usize = 1;
const ID_TEXT: usize = 2;
const ID_TASK_MANAGER: usize = 3;
const ID_EXIT: usize = 4;
const ID_SETTINGS: usize = 5;
const ID_ABOUT: usize = 6;

pub fn command_for(id: usize) -> Option<Command> {
    match id {
        ID_GRAPHS => Some(Command::Mode(DisplayMode::Graph)),
        ID_TEXT => Some(Command::Mode(DisplayMode::Text)),
        ID_SETTINGS => Some(Command::Settings),
        ID_ABOUT => Some(Command::About),
        ID_TASK_MANAGER => Some(Command::TaskManager),
        ID_EXIT => Some(Command::Exit),
        _ => None,
    }
}

/// Shows the context menu at a screen position and returns the chosen
/// command. Runs a modal loop: the caller must not hold app state borrowed.
pub fn show(owner: HWND, x: i32, y: i32, mode: DisplayMode) -> Option<Command> {
    unsafe {
        let menu = CreatePopupMenu().ok()?;
        let check = |m: DisplayMode| {
            if mode == m {
                MF_STRING | MF_CHECKED
            } else {
                MF_STRING
            }
        };
        let _ = AppendMenuW(menu, check(DisplayMode::Graph), ID_GRAPHS, w!("Graphs"));
        let _ = AppendMenuW(menu, check(DisplayMode::Text), ID_TEXT, w!("Text only"));
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
        let _ = AppendMenuW(menu, MF_STRING, ID_SETTINGS, w!("Settings…"));
        let _ = AppendMenuW(menu, MF_STRING, ID_TASK_MANAGER, w!("Open Task Manager"));
        let _ = AppendMenuW(menu, MF_STRING, ID_ABOUT, w!("About Pulsar"));
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
        let _ = AppendMenuW(menu, MF_STRING, ID_EXIT, w!("Exit Pulsar"));

        // Without being foreground, the menu would not close on an outside click.
        let _ = SetForegroundWindow(owner);
        let flags = TPM_RETURNCMD | TPM_NONOTIFY | TPM_RIGHTBUTTON | TPM_BOTTOMALIGN;
        let id = TrackPopupMenu(menu, flags, x, y, None, owner, None).0 as usize;
        let _ = PostMessageW(Some(owner), WM_NULL, WPARAM(0), LPARAM(0));
        let _ = DestroyMenu(menu);
        command_for(id)
    }
}

pub fn open_task_manager() {
    unsafe {
        ShellExecuteW(
            None,
            w!("open"),
            &HSTRING::from("taskmgr.exe"),
            PCWSTR::null(),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_map_to_commands() {
        assert_eq!(command_for(ID_TEXT), Some(Command::Mode(DisplayMode::Text)));
        assert_eq!(command_for(ID_EXIT), Some(Command::Exit));
        assert_eq!(command_for(ID_SETTINGS), Some(Command::Settings));
        assert_eq!(command_for(ID_ABOUT), Some(Command::About));
        assert_eq!(command_for(0), None, "menu dismissed");
    }
}
