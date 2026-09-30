use pulsar_core::config::{Config, DisplayMode};
use pulsar_core::layout::{ItemSpec, Layout, LayoutInput, compute_layout, item_specs};
use windows::Win32::Foundation::{HINSTANCE, HWND};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DestroyWindow, HWND_TOPMOST, IDC_ARROW, LoadCursorW, RegisterClassW, SW_HIDE,
    SW_SHOWNOACTIVATE, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SetWindowPos, ShowWindow, WNDCLASSW,
    WNDPROC, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};
use windows::core::{Result, w};

use crate::render::Surface;
use crate::taskbar::Taskbar;
use crate::text::Text;

pub const CLASS: windows::core::PCWSTR = w!("PulsarOverlay");

pub fn register_class(instance: HINSTANCE, wndproc: WNDPROC) {
    let class = WNDCLASSW {
        lpfnWndProc: wndproc,
        hInstance: instance,
        lpszClassName: CLASS,
        hCursor: unsafe { LoadCursorW(None, IDC_ARROW) }.unwrap_or_default(),
        ..Default::default()
    };
    unsafe { RegisterClassW(&class) };
}

#[derive(Clone, Debug, PartialEq)]
struct LayoutKey {
    height: i32,
    dpi: u32,
    mode: DisplayMode,
    items: Vec<ItemSpec>,
    font_size_pt: u32,
    short_labels: bool,
}

/// One always-on-top, non-activating layered window per taskbar. It is a
/// top-level window, never a child of Explorer's windows.
pub struct Overlay {
    pub hwnd: HWND,
    pub taskbar: Taskbar,
    pub layout: Layout,
    key: Option<LayoutKey>,
    surface: Option<Surface>,
    pub shown_at: Option<(i32, i32)>,
}

impl Overlay {
    pub fn create(instance: HINSTANCE, taskbar: Taskbar) -> Result<Self> {
        let hwnd = unsafe {
            CreateWindowExW(
                WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_TOPMOST | WS_EX_LAYERED,
                CLASS,
                w!("Pulsar"),
                WS_POPUP,
                0,
                0,
                0,
                0,
                None,
                None,
                Some(instance),
                None,
            )?
        };
        Ok(Self {
            hwnd,
            taskbar,
            layout: Layout::default(),
            key: None,
            surface: None,
            shown_at: None,
        })
    }

    /// Recomputes the layout only when something it depends on changed, so
    /// the overlay never resizes between samples.
    pub fn update_layout(&mut self, config: &Config, text: &Text) {
        let key = LayoutKey {
            height: self.taskbar.rect.height(),
            dpi: self.taskbar.dpi,
            mode: config.display.mode,
            items: item_specs(config),
            font_size_pt: config.display.font_size_pt.to_bits(),
            short_labels: config.display.short_labels,
        };
        if self.key.as_ref() == Some(&key) {
            return;
        }
        self.layout = compute_layout(
            &key.items,
            LayoutInput {
                mode: key.mode,
                taskbar_height_px: key.height as f32,
                dpi: key.dpi,
                font_size_pt: config.display.font_size_pt,
                short_labels: key.short_labels,
            },
            text,
        );
        self.key = Some(key);
    }

    /// Forces the next `update_layout` to measure again, e.g. after a font change.
    pub fn invalidate_layout(&mut self) {
        self.key = None;
    }

    pub fn size(&self) -> (i32, i32) {
        (
            self.layout.width.ceil() as i32,
            self.layout.height.round() as i32,
        )
    }

    pub fn surface(&mut self) -> Result<&Surface> {
        let (w, h) = self.size();
        if self
            .surface
            .as_ref()
            .is_none_or(|s| s.width != w || s.height != h)
        {
            self.surface = Some(Surface::new(w.max(1), h.max(1))?);
        }
        Ok(self.surface.as_ref().unwrap())
    }

    /// Called after the first present at a position: shows the window and
    /// puts it above the taskbar.
    pub fn mark_shown(&mut self, x: i32, y: i32) {
        if self.shown_at.is_none() {
            unsafe {
                let _ = ShowWindow(self.hwnd, SW_SHOWNOACTIVATE);
            }
            self.raise();
        }
        self.shown_at = Some((x, y));
    }

    pub fn raise(&self) {
        unsafe {
            let _ = SetWindowPos(
                self.hwnd,
                Some(HWND_TOPMOST),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            );
        }
    }

    pub fn hide(&mut self) {
        if self.shown_at.take().is_some() {
            unsafe {
                let _ = ShowWindow(self.hwnd, SW_HIDE);
            }
        }
    }
}

impl Drop for Overlay {
    fn drop(&mut self) {
        unsafe {
            let _ = DestroyWindow(self.hwnd);
        }
    }
}
