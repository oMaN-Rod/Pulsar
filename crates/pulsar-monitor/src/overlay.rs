use pulsar_core::config::{Config, DisplayMode, LabelStyle, Units};
use pulsar_core::layout::{ItemSpec, Layout, LayoutInput, compute_layout, item_specs};
use std::cell::Cell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{HINSTANCE, HWND};
use windows::Win32::Graphics::Dwm::{DWMWA_TRANSITIONS_FORCEDISABLED, DwmSetWindowAttribute};
use windows::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DestroyWindow, GW_HWNDPREV, GWL_EXSTYLE, GWL_STYLE, GWLP_USERDATA, GetParent,
    GetWindow, GetWindowLongW, GetWindowThreadProcessId, HWND_TOP, HWND_TOPMOST, IDC_ARROW,
    LoadCursorW, RegisterClassW, SW_HIDE, SW_SHOWNOACTIVATE, SWP_FRAMECHANGED, SWP_NOACTIVATE,
    SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SetParent, SetWindowLongPtrW, SetWindowPos, ShowWindow,
    WINDOW_STYLE, WNDCLASSW, WNDPROC, WS_CHILD, WS_EX_LAYERED, WS_EX_LAYOUTRTL, WS_EX_NOACTIVATE,
    WS_EX_NOPARENTNOTIFY, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};
use windows::core::{BOOL, Result, w};

use crate::render::Surface;
use crate::taskbar::placement::child_position;
use crate::taskbar::{self, Taskbar};
use crate::text::Text;

pub const CLASS: windows::core::PCWSTR = w!("PulsarOverlay");
const ATTACH_RETRY: Duration = Duration::from_secs(30);

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
    thickness: i32,
    vertical: bool,
    dpi: u32,
    mode: DisplayMode,
    items: Vec<ItemSpec>,
    font_size_pt: u32,
    labels: LabelStyle,
    icons: bool,
    graph_inline: bool,
    upload_on_top: bool,
    /// Temperatures change width with their unit.
    units: Units,
}

/// One non-activating layered window per taskbar: a child of the taskbar when
/// placed on it, otherwise always-on-top.
pub struct Overlay {
    pub hwnd: HWND,
    pub taskbar: Taskbar,
    pub layout: Layout,
    key: Option<LayoutKey>,
    surface: Option<Surface>,
    pub shown_at: Option<(i32, i32)>,
    /// Current and target opacity; a target of 0 means hidden for fullscreen.
    pub alpha: u8,
    pub fade_target: u8,
    pub float_dpi: Option<u32>,
    attached: bool,
    attach_retry: Option<Instant>,
    /// The position and size last given to the child window.
    child_at: Option<(i32, i32, i32, i32)>,
    /// Destroying the taskbar destroys its children. `hwnd` may then name
    /// another window, so it is not used once this is set.
    destroyed: Rc<Cell<bool>>,
}

/// Call on `WM_NCDESTROY`.
pub fn window_destroyed(hwnd: HWND) {
    let flag = unsafe { SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0) } as *const Cell<bool>;
    if !flag.is_null() {
        unsafe { Rc::from_raw(flag) }.set(true);
    }
}

/// Stops DWM animating the window when it is shown or hidden, which flickers
/// during Task View and other shell transitions.
pub fn disable_transitions(hwnd: HWND) {
    let on = BOOL(1);
    unsafe {
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_TRANSITIONS_FORCEDISABLED,
            (&on as *const BOOL).cast(),
            size_of::<BOOL>() as u32,
        );
    }
}

impl Overlay {
    pub fn create(instance: HINSTANCE, taskbar: Taskbar) -> Result<Self> {
        let hwnd = unsafe {
            CreateWindowExW(
                WS_EX_TOOLWINDOW
                    | WS_EX_NOACTIVATE
                    | WS_EX_TOPMOST
                    | WS_EX_LAYERED
                    | WS_EX_NOPARENTNOTIFY,
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
        disable_transitions(hwnd);
        let destroyed = Rc::new(Cell::new(false));
        unsafe {
            SetWindowLongPtrW(
                hwnd,
                GWLP_USERDATA,
                Rc::into_raw(destroyed.clone()) as isize,
            );
        }
        Ok(Self {
            hwnd,
            taskbar,
            layout: Layout::default(),
            key: None,
            surface: None,
            shown_at: None,
            alpha: 255,
            fade_target: 255,
            float_dpi: None,
            attached: false,
            attach_retry: None,
            child_at: None,
            destroyed,
        })
    }

    pub fn alive(&self) -> bool {
        !self.destroyed.get()
    }

    pub fn attached(&self) -> bool {
        self.attached
    }

    /// While Start or Search is open Windows moves the taskbar to a higher
    /// window band, where no top-level window can follow; a child shares it.
    pub fn attach(&mut self, attached: bool) {
        if !self.alive() {
            return;
        }
        if self.attached && unsafe { GetParent(self.hwnd) }.ok() != Some(self.taskbar.hwnd) {
            log::warn!("the overlay lost its place on the taskbar");
            self.hide();
            self.attached = false;
            self.set_style(WS_POPUP);
        }
        if self.attached == attached {
            if attached && self.separate_input() {
                log::warn!("Windows linked the overlay's input to the taskbar again");
            }
            return;
        }
        if attached && self.attach_retry.is_some_and(|at| Instant::now() < at) {
            return;
        }
        if attached && !taskbar::same_dpi_awareness(self.hwnd, self.taskbar.hwnd) {
            self.attach_failed("the taskbar has a different DPI awareness");
            return;
        }
        self.hide();
        if !self.alive() {
            return;
        }
        if attached {
            self.set_style(WS_CHILD);
            if let Err(e) = unsafe { SetParent(self.hwnd, Some(self.taskbar.hwnd)) } {
                self.set_style(WS_POPUP);
                self.attach_failed(&e.to_string());
                return;
            }
            if self.separate_input() {
                log::info!("attached to the taskbar with separate input");
            } else {
                log::warn!("attached to the taskbar, but could not separate its input");
            }
        } else {
            if let Err(e) = unsafe { SetParent(self.hwnd, None) } {
                log::warn!("could not detach the overlay from the taskbar: {e}");
                return;
            }
            self.set_style(WS_POPUP);
        }
        self.attached = attached;
        self.child_at = None;
        if !self.alive() {
            return;
        }
        unsafe {
            let _ = SetWindowPos(
                self.hwnd,
                None,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED,
            );
        }
    }

    fn set_style(&self, style: WINDOW_STYLE) {
        if !self.alive() {
            return;
        }
        unsafe {
            SetWindowLongPtrW(self.hwnd, GWL_STYLE, style.0 as isize);
        }
    }

    fn attach_failed(&mut self, reason: &str) {
        if self.attach_retry.is_none() {
            log::warn!("kept the overlay separate from the taskbar: {reason}");
        }
        self.attach_retry = Some(Instant::now() + ATTACH_RETRY);
    }

    /// Parenting shares one input queue between this thread and Explorer's, so
    /// that a hang in either stalls input to both. Returns whether they were
    /// linked; Windows reports an error when they are already separate.
    fn separate_input(&self) -> bool {
        unsafe {
            let theirs = GetWindowThreadProcessId(self.taskbar.hwnd, None);
            AttachThreadInput(GetCurrentThreadId(), theirs, false).as_bool()
        }
    }

    /// The screen position to present at. A child is moved here instead,
    /// in its parent's coordinates, and presented in place. It stays put
    /// relative to the taskbar when the taskbar moves.
    pub fn present_target(&mut self, x: i32, y: i32) -> Option<(i32, i32)> {
        if !self.attached {
            return Some((x, y));
        }
        let (w, h) = self.size();
        let parent = self.taskbar.hwnd;
        let style = unsafe { GetWindowLongW(parent, GWL_EXSTYLE) } as u32;
        let mirrored = style & WS_EX_LAYOUTRTL.0 != 0;
        let (cx, cy) = child_position((x, y), w, self.taskbar.rect, mirrored);
        if self.child_at != Some((cx, cy, w, h)) {
            let moved = unsafe {
                SetWindowPos(self.hwnd, None, cx, cy, w, h, SWP_NOZORDER | SWP_NOACTIVATE)
            };
            self.child_at = moved.is_ok().then_some((cx, cy, w, h));
        }
        None
    }

    pub fn is_covered(&self) -> bool {
        if !self.alive() {
            false
        } else if self.attached {
            unsafe { GetWindow(self.hwnd, GW_HWNDPREV) }.is_ok()
        } else {
            taskbar::is_behind(self.hwnd, self.taskbar.hwnd)
        }
    }

    /// Recomputes the layout only when something it depends on changed, so
    /// the overlay never resizes between samples.
    pub fn dpi(&self) -> u32 {
        self.float_dpi.unwrap_or(self.taskbar.dpi)
    }

    pub fn update_layout(&mut self, config: &Config, text: &Text) {
        let bar = self.taskbar.rect;
        let key = LayoutKey {
            thickness: (i64::from(bar.height().min(bar.width())) * i64::from(self.dpi())
                / i64::from(self.taskbar.dpi.max(1))) as i32,
            vertical: self.float_dpi.is_none() && bar.is_vertical(),
            dpi: self.dpi(),
            mode: config.display.mode,
            items: item_specs(config),
            font_size_pt: config.display.font_size_pt.to_bits(),
            labels: config.display.labels,
            icons: config.display.icons,
            graph_inline: config.display.graph_inline,
            upload_on_top: config.display.upload_on_top,
            units: config.units,
        };
        if self.key.as_ref() == Some(&key) {
            return;
        }
        self.layout = compute_layout(
            &key.items,
            LayoutInput {
                mode: key.mode,
                taskbar_height_px: key.thickness as f32,
                dpi: key.dpi,
                font_size_pt: config.display.font_size_pt,
                labels: key.labels,
                icons: key.icons,
                graph_inline: key.graph_inline,
                upload_on_top: key.upload_on_top,
                vertical_bar_px: key.vertical.then_some(key.thickness as f32),
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
        if !self.alive() {
            return;
        }
        if self.shown_at.is_none() {
            unsafe {
                let _ = ShowWindow(self.hwnd, SW_SHOWNOACTIVATE);
            }
            self.raise();
        }
        self.shown_at = Some((x, y));
    }

    pub fn raise(&self) {
        if !self.alive() {
            return;
        }
        unsafe {
            let _ = SetWindowPos(
                self.hwnd,
                Some(if self.attached {
                    HWND_TOP
                } else {
                    HWND_TOPMOST
                }),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            );
        }
    }

    pub fn hide(&mut self) {
        if self.shown_at.take().is_some() && self.alive() {
            unsafe {
                let _ = ShowWindow(self.hwnd, SW_HIDE);
            }
        }
    }
}

impl Drop for Overlay {
    fn drop(&mut self) {
        if self.alive() {
            unsafe {
                let _ = DestroyWindow(self.hwnd);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::taskbar::placement::Rect32;
    use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::WindowsAndMessaging::{
        DefWindowProcW, WM_NCDESTROY, WM_WINDOWPOSCHANGING,
    };

    thread_local! {
        static DESTROY_WHEN_MOVED: Cell<bool> = const { Cell::new(false) };
    }

    unsafe extern "system" fn proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
        if msg == WM_NCDESTROY {
            window_destroyed(hwnd);
        }
        if msg == WM_WINDOWPOSCHANGING && DESTROY_WHEN_MOVED.replace(false) {
            let _ = unsafe { DestroyWindow(hwnd) };
            return LRESULT(0);
        }
        unsafe { DefWindowProcW(hwnd, msg, wp, lp) }
    }

    fn overlay() -> Overlay {
        let instance: HINSTANCE = unsafe { GetModuleHandleW(None) }.unwrap().into();
        register_class(instance, Some(proc));
        let taskbar = Taskbar {
            hwnd: HWND::default(),
            tray: None,
            rect: Rect32::default(),
            tray_rect: None,
            monitor: Rect32::default(),
            dpi: 96,
        };
        Overlay::create(instance, taskbar).unwrap()
    }

    #[test]
    fn a_window_destroyed_while_moving_is_not_shown() {
        let mut overlay = overlay();
        overlay.attached = true;
        DESTROY_WHEN_MOVED.set(true);
        overlay.present_target(0, 0);
        assert!(!overlay.alive());
        overlay.mark_shown(0, 0);
        assert_eq!(overlay.shown_at, None);
    }

    #[test]
    fn a_window_destroyed_elsewhere_is_no_longer_used() {
        let mut overlay = overlay();
        assert!(overlay.alive());
        overlay.shown_at = Some((0, 0));
        unsafe { DestroyWindow(overlay.hwnd) }.unwrap();
        assert!(!overlay.alive());
        assert!(!overlay.is_covered());
        overlay.hide();
        assert_eq!(overlay.shown_at, None);
    }
}
