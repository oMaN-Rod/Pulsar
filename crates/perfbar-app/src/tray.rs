use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Shell::{
    NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_TIP, NIIF_WARNING, NIM_ADD, NIM_DELETE, NIM_MODIFY,
    NIM_SETVERSION, NOTIFYICON_VERSION_4, NOTIFYICONDATAW, Shell_NotifyIconW,
};
use windows::Win32::UI::WindowsAndMessaging::{IDI_APPLICATION, LoadIconW};

use crate::messages::WM_APP_TRAY;

const ICON_ID: u32 = 1;

pub struct Tray {
    host: HWND,
}

fn copy_into<const N: usize>(dst: &mut [u16; N], s: &str) {
    for (d, c) in dst.iter_mut().zip(s.encode_utf16().take(N - 1)) {
        *d = c;
    }
}

impl Tray {
    pub fn new(host: HWND) -> Self {
        let tray = Self { host };
        tray.add();
        tray
    }

    fn data(&self) -> NOTIFYICONDATAW {
        NOTIFYICONDATAW {
            cbSize: size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: self.host,
            uID: ICON_ID,
            ..Default::default()
        }
    }

    /// Also called after Explorer restarts, which drops every tray icon.
    pub fn add(&self) {
        let mut data = self.data();
        data.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
        data.uCallbackMessage = WM_APP_TRAY;
        data.hIcon = unsafe { LoadIconW(None, IDI_APPLICATION) }.unwrap_or_default();
        copy_into(&mut data.szTip, "PerfBar");
        data.Anonymous.uVersion = NOTIFYICON_VERSION_4;
        unsafe {
            let _ = Shell_NotifyIconW(NIM_ADD, &data);
            let _ = Shell_NotifyIconW(NIM_SETVERSION, &data);
        }
    }

    pub fn warn(&self, title: &str, message: &str) {
        let mut data = self.data();
        data.uFlags = NIF_INFO;
        data.dwInfoFlags = NIIF_WARNING;
        copy_into(&mut data.szInfoTitle, title);
        copy_into(&mut data.szInfo, message);
        unsafe {
            let _ = Shell_NotifyIconW(NIM_MODIFY, &data);
        }
    }
}

impl Drop for Tray {
    fn drop(&mut self) {
        unsafe {
            let _ = Shell_NotifyIconW(NIM_DELETE, &self.data());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copy_truncates_and_keeps_terminator() {
        let mut buf = [0u16; 4];
        copy_into(&mut buf, "PerfBar");
        assert_eq!(String::from_utf16_lossy(&buf[..3]), "Per");
        assert_eq!(buf[3], 0);
    }
}
