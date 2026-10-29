use std::cell::{Cell, RefCell};

use windows::Win32::Foundation::{HINSTANCE, HWND};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::{GetDpiForSystem, GetSystemMetricsForDpi};
use windows::Win32::UI::Shell::{
    NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_TIP, NIIF_INFO, NIIF_NOSOUND, NIIF_WARNING, NIM_ADD,
    NIM_DELETE, NIM_MODIFY, NIM_SETVERSION, NOTIFY_ICON_INFOTIP_FLAGS, NOTIFYICON_VERSION_4,
    NOTIFYICONDATAW, Shell_NotifyIconW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    HICON, IDI_APPLICATION, IMAGE_ICON, LR_DEFAULTCOLOR, LR_SHARED, LoadIconW, LoadImageW,
    SM_CXSMICON,
};
use windows::core::PCWSTR;

use crate::messages::WM_APP_TRAY;

const ICON_ID: u32 = 1;

pub struct Tray {
    host: HWND,
    /// What clicking the current balloon opens.
    target: RefCell<Option<String>>,
    /// Whether the user wants the icon shown.
    visible: Cell<bool>,
    /// A hidden icon is in the tray only until its balloon closes.
    for_balloon: Cell<bool>,
}

/// Icon resource 1 at the small-icon size, shared so it needs no cleanup.
fn app_icon() -> HICON {
    unsafe {
        let size = GetSystemMetricsForDpi(SM_CXSMICON, GetDpiForSystem());
        let module = GetModuleHandleW(None).ok().map(HINSTANCE::from);
        LoadImageW(
            module,
            PCWSTR(1 as _),
            IMAGE_ICON,
            size,
            size,
            LR_DEFAULTCOLOR | LR_SHARED,
        )
        .map(|h| HICON(h.0))
        .or_else(|_| LoadIconW(None, IDI_APPLICATION))
        .unwrap_or_default()
    }
}

fn copy_into<const N: usize>(dst: &mut [u16; N], s: &str) {
    for (d, c) in dst.iter_mut().zip(s.encode_utf16().take(N - 1)) {
        *d = c;
    }
}

impl Tray {
    pub fn new(host: HWND, visible: bool) -> Self {
        let tray = Self {
            host,
            target: RefCell::new(None),
            visible: Cell::new(visible),
            for_balloon: Cell::new(false),
        };
        if visible {
            tray.add();
        }
        tray
    }

    fn present(&self) -> bool {
        self.visible.get() || self.for_balloon.get()
    }

    /// An unchanged preference leaves a notification's own icon alone.
    pub fn set_visible(&self, visible: bool) {
        if visible == self.visible.get() {
            return;
        }
        let was = self.present();
        self.visible.set(visible);
        self.for_balloon.set(false);
        match (was, visible) {
            (false, true) => self.add(),
            (true, false) => self.remove(),
            _ => {}
        }
    }

    /// Explorer restarted and dropped every tray icon, balloons included.
    pub fn restore(&self) {
        self.for_balloon.set(false);
        if self.visible.get() {
            self.add();
        }
    }

    /// The balloon timed out or was clicked.
    pub fn balloon_closed(&self) {
        if self.for_balloon.replace(false) {
            self.remove();
        }
    }

    fn data(&self) -> NOTIFYICONDATAW {
        NOTIFYICONDATAW {
            cbSize: size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: self.host,
            uID: ICON_ID,
            ..Default::default()
        }
    }

    fn add(&self) {
        let mut data = self.data();
        data.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
        data.uCallbackMessage = WM_APP_TRAY;
        data.hIcon = app_icon();
        copy_into(&mut data.szTip, "Pulsar");
        data.Anonymous.uVersion = NOTIFYICON_VERSION_4;
        unsafe {
            let _ = Shell_NotifyIconW(NIM_ADD, &data);
            let _ = Shell_NotifyIconW(NIM_SETVERSION, &data);
        }
    }

    fn remove(&self) {
        unsafe {
            let _ = Shell_NotifyIconW(NIM_DELETE, &self.data());
        }
    }

    fn balloon(&self, flags: NOTIFY_ICON_INFOTIP_FLAGS, title: &str, message: &str) {
        if !self.present() {
            self.for_balloon.set(true);
            self.add();
        }
        let mut data = self.data();
        data.uFlags = NIF_INFO;
        data.dwInfoFlags = flags;
        copy_into(&mut data.szInfoTitle, title);
        copy_into(&mut data.szInfo, message);
        unsafe {
            let _ = Shell_NotifyIconW(NIM_MODIFY, &data);
        }
    }

    pub fn warn(&self, title: &str, message: &str) {
        log::warn!("{title}: {message}");
        self.target.replace(None);
        self.balloon(NIIF_WARNING, title, message);
    }

    /// A silent balloon that does nothing when clicked.
    pub fn inform(&self, title: &str, message: &str) {
        self.target.replace(None);
        self.balloon(NIIF_INFO | NIIF_NOSOUND, title, message);
    }

    /// A balloon that opens `target` (a file or URL) when clicked.
    pub fn offer(&self, title: &str, message: &str, target: String) {
        self.target.replace(Some(target));
        self.balloon(NIIF_INFO, title, message);
    }

    pub fn target(&self) -> Option<String> {
        self.target.borrow().clone()
    }
}

impl Drop for Tray {
    fn drop(&mut self) {
        self.remove();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copy_truncates_and_keeps_terminator() {
        let mut buf = [0u16; 4];
        copy_into(&mut buf, "Pulsar");
        assert_eq!(String::from_utf16_lossy(&buf[..3]), "Pul");
        assert_eq!(buf[3], 0);
    }

    #[test]
    fn an_offer_is_kept_until_a_warning_replaces_it() {
        let tray = Tray::new(HWND::default(), true);
        tray.offer("t", "m", "https://example.com".into());
        assert_eq!(tray.target().as_deref(), Some("https://example.com"));
        assert_eq!(tray.target().as_deref(), Some("https://example.com"));
        tray.warn("t", "m");
        assert_eq!(tray.target(), None);
    }

    #[test]
    fn a_hidden_icon_appears_only_while_its_balloon_is_open() {
        let tray = Tray::new(HWND::default(), false);
        assert!(!tray.present());
        tray.warn("t", "m");
        assert!(tray.present());
        tray.balloon_closed();
        assert!(!tray.present());
    }

    #[test]
    fn showing_the_icon_keeps_it_after_a_balloon_closes() {
        let tray = Tray::new(HWND::default(), false);
        tray.warn("t", "m");
        tray.set_visible(true);
        tray.balloon_closed();
        assert!(tray.present());
        tray.set_visible(false);
        assert!(!tray.present());
    }

    #[test]
    fn an_explorer_restart_drops_a_balloon_only_icon() {
        let tray = Tray::new(HWND::default(), false);
        tray.warn("t", "m");
        tray.restore();
        assert!(!tray.present());
    }
}
