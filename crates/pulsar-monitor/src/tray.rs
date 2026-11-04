use std::cell::{Cell, RefCell};

use windows::Win32::Foundation::{COLORREF, HINSTANCE, HWND, RECT};
use windows::Win32::Graphics::Gdi::{
    ANTIALIASED_QUALITY, BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CLIP_DEFAULT_PRECIS, CreateBitmap,
    CreateCompatibleDC, CreateDIBSection, CreateFontW, DEFAULT_CHARSET, DEFAULT_PITCH,
    DIB_RGB_COLORS, DT_CENTER, DT_NOPREFIX, DT_SINGLELINE, DT_VCENTER, DeleteDC, DeleteObject,
    DrawTextW, FF_DONTCARE, FW_SEMIBOLD, OUT_DEFAULT_PRECIS, SelectObject, SetBkMode, SetTextColor,
    TRANSPARENT,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::{GetDpiForSystem, GetSystemMetricsForDpi};
use windows::Win32::UI::Shell::{
    NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_TIP, NIIF_INFO, NIIF_NOSOUND, NIIF_WARNING, NIM_ADD,
    NIM_DELETE, NIM_MODIFY, NIM_SETVERSION, NOTIFY_ICON_INFOTIP_FLAGS, NOTIFYICON_VERSION_4,
    NOTIFYICONDATAW, Shell_NotifyIconW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateIconIndirect, DestroyIcon, HICON, ICONINFO, IDI_APPLICATION, IMAGE_ICON, LR_DEFAULTCOLOR,
    LR_SHARED, LoadIconW, LoadImageW, SM_CXSMICON,
};
use windows::core::{PCWSTR, w};

use crate::messages::WM_APP_TRAY;

const ICON_ID: u32 = 1;
const TIP_CAPACITY: usize = 127;

/// A reading shown in place of the app icon, when the taskbar has no room for it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Metric {
    pub text: String,
    pub tip: String,
    pub light_taskbar: bool,
}

pub struct Tray {
    host: HWND,
    /// What clicking the current balloon opens.
    target: RefCell<Option<String>>,
    /// Whether the user wants the icon shown.
    visible: Cell<bool>,
    /// A hidden icon is in the tray only until its balloon closes.
    for_balloon: Cell<bool>,
    metric: RefCell<Option<Metric>>,
    /// Drawn for `metric`, and owned here.
    metric_icon: Cell<Option<HICON>>,
}

/// The number at the front of a formatted value, which is all that fits: `12%`
/// is 12 and `1.2 MB/s` is 1.2. The tip has the rest.
pub fn icon_text(value: &str) -> String {
    let number: String = value
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    let number = if number.is_empty() { value } else { &number };
    number
        .chars()
        .take(3)
        .collect::<String>()
        .trim_end_matches('.')
        .to_string()
}

/// White text over black gives each pixel's coverage, which becomes the alpha
/// of a single color.
fn metric_icon(text: &str, size: i32, light_taskbar: bool) -> Option<HICON> {
    let (r, g, b): (u32, u32, u32) = if light_taskbar {
        (0, 0, 0)
    } else {
        (255, 255, 255)
    };
    unsafe {
        let dc = CreateCompatibleDC(None);
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: size,
                biHeight: -size,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits = std::ptr::null_mut();
        let Ok(color) = CreateDIBSection(Some(dc), &info, DIB_RGB_COLORS, &mut bits, None, 0)
        else {
            let _ = DeleteDC(dc);
            return None;
        };
        let old_bitmap = SelectObject(dc, color.into());
        let font = CreateFontW(
            -(size * 3 / 4),
            0,
            0,
            0,
            FW_SEMIBOLD.0 as i32,
            0,
            0,
            0,
            DEFAULT_CHARSET,
            OUT_DEFAULT_PRECIS,
            CLIP_DEFAULT_PRECIS,
            ANTIALIASED_QUALITY,
            (DEFAULT_PITCH.0 | FF_DONTCARE.0) as u32,
            w!("Segoe UI"),
        );
        let old_font = SelectObject(dc, font.into());
        SetBkMode(dc, TRANSPARENT);
        SetTextColor(dc, COLORREF(0x00FF_FFFF));
        let mut wide: Vec<u16> = text.encode_utf16().collect();
        let mut area = RECT {
            left: 0,
            top: 0,
            right: size,
            bottom: size,
        };
        DrawTextW(
            dc,
            &mut wide,
            &mut area,
            DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
        );
        let pixels = std::slice::from_raw_parts_mut(bits.cast::<u32>(), (size * size) as usize);
        for pixel in pixels.iter_mut() {
            let a = (*pixel >> 16 & 0xFF)
                .max(*pixel >> 8 & 0xFF)
                .max(*pixel & 0xFF);
            *pixel = a << 24 | (r * a / 255) << 16 | (g * a / 255) << 8 | (b * a / 255);
        }
        SelectObject(dc, old_font);
        SelectObject(dc, old_bitmap);
        let _ = DeleteObject(font.into());
        let _ = DeleteDC(dc);
        let mask = CreateBitmap(size, size, 1, 1, None);
        let icon = CreateIconIndirect(&ICONINFO {
            fIcon: true.into(),
            hbmMask: mask,
            hbmColor: color,
            ..Default::default()
        })
        .ok();
        let _ = DeleteObject(mask.into());
        let _ = DeleteObject(color.into());
        icon
    }
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
            metric: RefCell::new(None),
            metric_icon: Cell::new(None),
        };
        if visible {
            tray.add();
        }
        tray
    }

    fn present(&self) -> bool {
        self.visible.get() || self.for_balloon.get() || self.metric.borrow().is_some()
    }

    pub fn set_visible(&self, visible: bool) {
        if visible == self.visible.get() {
            return;
        }
        let was = self.present();
        self.visible.set(visible);
        self.for_balloon.set(false);
        match (was, self.present()) {
            (false, true) => self.add(),
            (true, false) => self.remove(),
            _ => {}
        }
    }

    /// Explorer restarted and dropped every tray icon, balloons included.
    pub fn restore(&self) {
        self.for_balloon.set(false);
        if self.present() {
            self.add();
        }
    }

    /// The balloon timed out or was clicked.
    pub fn balloon_closed(&self) {
        if self.for_balloon.replace(false) && !self.present() {
            self.remove();
        }
    }

    /// Shows `metric` as the icon, even if the user hides the icon, because it
    /// stands in for the overlay; `None` goes back to the app icon.
    pub fn set_metric(&self, metric: Option<Metric>) {
        if *self.metric.borrow() == metric {
            return;
        }
        let was = self.present();
        let old_icon = self.metric_icon.take();
        self.metric_icon.set(metric.as_ref().and_then(|m| {
            let size = unsafe { GetSystemMetricsForDpi(SM_CXSMICON, GetDpiForSystem()) };
            metric_icon(&m.text, size, m.light_taskbar)
        }));
        self.metric.replace(metric);
        match (was, self.present()) {
            (false, true) => self.add(),
            (true, false) => self.remove(),
            (true, true) => {
                let mut data = self.data();
                self.fill_icon(&mut data);
                unsafe {
                    let _ = Shell_NotifyIconW(NIM_MODIFY, &data);
                }
            }
            _ => {}
        }
        if let Some(icon) = old_icon {
            unsafe {
                let _ = DestroyIcon(icon);
            }
        }
    }

    fn fill_icon(&self, data: &mut NOTIFYICONDATAW) {
        data.uFlags |= NIF_ICON | NIF_TIP;
        data.hIcon = self.metric_icon.get().unwrap_or_else(app_icon);
        match &*self.metric.borrow() {
            Some(m) => copy_into::<128>(&mut data.szTip, truncated(&m.tip)),
            None => copy_into(&mut data.szTip, "Pulsar"),
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
        data.uFlags = NIF_MESSAGE;
        self.fill_icon(&mut data);
        data.uCallbackMessage = WM_APP_TRAY;
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

/// Whole lines that fit in a tip, so that a cut never lands mid-value.
fn truncated(tip: &str) -> &str {
    let mut end = 0;
    let mut units = 0;
    for line in tip.split_inclusive('\n') {
        units += line.encode_utf16().count();
        if units > TIP_CAPACITY {
            break;
        }
        end += line.len();
    }
    tip[..end].trim_end()
}

impl Drop for Tray {
    fn drop(&mut self) {
        self.remove();
        if let Some(icon) = self.metric_icon.take() {
            unsafe {
                let _ = DestroyIcon(icon);
            }
        }
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

    fn metric(text: &str) -> Metric {
        Metric {
            text: text.into(),
            tip: "CPU 12%".into(),
            light_taskbar: false,
        }
    }

    #[test]
    fn a_metric_shows_a_hidden_icon_until_it_is_cleared() {
        let tray = Tray::new(HWND::default(), false);
        tray.set_metric(Some(metric("12")));
        assert!(tray.present());
        assert!(tray.metric_icon.get().is_some());
        tray.set_metric(Some(metric("13")));
        tray.restore();
        assert!(tray.present());
        tray.set_metric(None);
        assert!(!tray.present());
        assert!(tray.metric_icon.get().is_none());
    }

    #[test]
    fn a_closing_balloon_leaves_a_metric_in_place() {
        let tray = Tray::new(HWND::default(), false);
        tray.set_metric(Some(metric("12")));
        tray.warn("t", "m");
        tray.balloon_closed();
        assert!(tray.present());
    }

    #[test]
    fn the_icon_holds_only_the_number() {
        assert_eq!(icon_text("12%"), "12");
        assert_eq!(icon_text("100%"), "100");
        assert_eq!(icon_text("1.2 MB/s"), "1.2");
        assert_eq!(icon_text("103 KB/s"), "103");
        assert_eq!(icon_text("100. KB/s"), "100");
        assert_eq!(icon_text("—"), "—");
    }

    #[test]
    fn a_long_tip_loses_whole_lines() {
        let line = "Network 999.9 MB/s\n";
        let long = line.repeat(10);
        let cut = truncated(&long);
        assert!(cut.encode_utf16().count() <= TIP_CAPACITY);
        assert!(cut.ends_with("MB/s"));
        assert_eq!(truncated("CPU 1%\nRAM 2%"), "CPU 1%\nRAM 2%");
    }

    #[test]
    fn an_explorer_restart_drops_a_balloon_only_icon() {
        let tray = Tray::new(HWND::default(), false);
        tray.warn("t", "m");
        tray.restore();
        assert!(!tray.present());
    }
}
