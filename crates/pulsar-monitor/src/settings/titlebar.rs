//! A caption that blends into the page, like the Windows 11 Settings app: the
//! native title bar keeps its buttons, snapping and resizing, but takes the
//! window's background colour.

use windows::Win32::Graphics::Dwm::{
    DWMWA_CAPTION_COLOR, DWMWA_USE_IMMERSIVE_DARK_MODE, DwmSetWindowAttribute,
};
use windows::Win32::UI::WindowsAndMessaging::{FindWindowW, GetWindowThreadProcessId};
use windows::core::{BOOL, HSTRING, PCWSTR};

/// DWM colours are `0x00BBGGRR`.
pub fn colorref(r: u8, g: u8, b: u8) -> u32 {
    u32::from(r) | u32::from(g) << 8 | u32::from(b) << 16
}

/// Colours the caption of this process's window titled `title`. False while
/// that window does not exist yet.
pub fn blend(title: &str, (r, g, b): (u8, u8, u8), dark: bool) -> bool {
    let Ok(hwnd) = (unsafe { FindWindowW(PCWSTR::null(), &HSTRING::from(title)) }) else {
        return false;
    };
    let mut pid = 0;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    if pid != std::process::id() {
        return false;
    }
    let dark = BOOL::from(dark);
    let color = colorref(r, g, b);
    unsafe {
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_USE_IMMERSIVE_DARK_MODE,
            (&dark as *const BOOL).cast(),
            size_of::<BOOL>() as u32,
        );
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_CAPTION_COLOR,
            (&color as *const u32).cast(),
            size_of::<u32>() as u32,
        );
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colours_become_colorref_in_bgr_order() {
        assert_eq!(colorref(0x20, 0x40, 0x60), 0x0060_4020);
        assert_eq!(colorref(0xFF, 0, 0), 0x0000_00FF);
    }
}
