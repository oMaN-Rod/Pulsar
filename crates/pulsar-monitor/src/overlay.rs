use pulsar_core::config::{Config, DisplayMode, LabelStyle, RemoveFrom, Units};
use pulsar_core::gpu_slots::GpuSlots;
use pulsar_core::layout::fit::{self, Fit, GROW_MARGIN_DIP, Room, Rung, TierStyle};
use pulsar_core::layout::{ItemSpec, Layout, LayoutInput, TextSpacing, compute_layout, item_specs};
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
use crate::taskbar::placement::{
    HideReason, Placement, PlacementInput, child_position, free_gaps, gaps_in_order, is_clear,
    place, room_in, scaled, settle, start_along,
};
use crate::taskbar::space::{Rects, Tracker};
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
    floating: bool,
    dpi: u32,
    mode: DisplayMode,
    items: Vec<ItemSpec>,
    font_size_pt: u32,
    labels: LabelStyle,
    icons: bool,
    graph_inline: bool,
    upload_on_top: bool,
    text_spacing: TextSpacing,
    /// Temperatures change width with their unit.
    units: Units,
    /// `None` keeps one layout: floating, or fitting turned off.
    remove_from: Option<RemoveFrom>,
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
    /// Kept here because refreshing replaces `taskbar`.
    pub tracker: Tracker,
    /// How often to read the taskbar; `None` while there is no reason to.
    pub pace: Option<Duration>,
    /// Empty when the overlay keeps one layout.
    ladder: Vec<Rung>,
    fit: Fit,
    selected: usize,
    pub slack: Option<i32>,
    pub no_room: bool,
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
            tracker: Tracker::default(),
            pace: None,
            ladder: Vec::new(),
            fit: Fit::default(),
            selected: 0,
            slack: None,
            no_room: false,
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

    /// Floating above the taskbar means leaving it. Coming back is held off,
    /// so that the taskbar's room coming and going does not keep reparenting.
    pub fn hold_off_attaching(&mut self) {
        self.attach_retry = Some(Instant::now() + ATTACH_RETRY);
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

    pub fn update_layout(&mut self, config: &Config, gpus: &GpuSlots, text: &Text) {
        let bar = self.taskbar.rect;
        let key = LayoutKey {
            thickness: (i64::from(bar.height().min(bar.width())) * i64::from(self.dpi())
                / i64::from(self.taskbar.dpi.max(1))) as i32,
            vertical: self.float_dpi.is_none() && bar.is_vertical(),
            floating: self.float_dpi.is_some(),
            dpi: self.dpi(),
            mode: config.display.mode,
            items: item_specs(config, gpus),
            font_size_pt: config.display.font_size_pt.to_bits(),
            labels: config.display.labels,
            icons: config.display.icons,
            graph_inline: config.display.graph_inline,
            upload_on_top: config.display.upload_on_top,
            text_spacing: TextSpacing::from(&config.display),
            units: config.units,
            remove_from: (config.display.fit_to_taskbar && self.float_dpi.is_none())
                .then_some(config.display.fit_remove_from),
        };
        if self.key.as_ref() == Some(&key) {
            return;
        }
        let input = LayoutInput {
            mode: key.mode,
            taskbar_height_px: key.thickness as f32,
            fit_font: key.floating,
            dpi: key.dpi,
            font_size_pt: config.display.font_size_pt,
            labels: key.labels,
            icons: key.icons,
            graph_inline: key.graph_inline,
            upload_on_top: key.upload_on_top,
            vertical_bar_px: key.vertical.then_some(key.thickness as f32),
            text_spacing: key.text_spacing,
        };
        self.ladder = key
            .remove_from
            .map_or_else(Vec::new, |from| fit::ladder(&key.items, input, from, text));
        self.layout = match self.ladder.first() {
            Some(full) => full.layout.clone(),
            None => compute_layout(&key.items, input, text),
        };
        self.fit.reset();
        self.selected = 0;
        self.slack = None;
        self.no_room = false;
        self.key = Some(key);
    }

    /// Forces the next `update_layout` to measure again, e.g. after a font change.
    pub fn invalidate_layout(&mut self) {
        self.key = None;
    }

    pub fn size(&self) -> (i32, i32) {
        pixels(&self.layout)
    }

    /// The size with every item showing, which decides whether the overlay can
    /// be a child of the taskbar; dropping items never changes that answer.
    pub fn full_size(&self) -> (i32, i32) {
        self.ladder
            .first()
            .map_or_else(|| self.size(), |full| pixels(&full.layout))
    }

    /// The labelling of the layout on show, when it differs by tier.
    pub fn tier_style(&self) -> Option<TierStyle> {
        self.ladder.get(self.selected).map(|rung| rung.style)
    }

    pub fn tier_is_full(&self) -> bool {
        self.selected == 0
    }

    pub fn full_layout(&self) -> &Layout {
        self.ladder.first().map_or(&self.layout, |r| &r.layout)
    }

    pub fn grow_due(&self, now: Instant) -> bool {
        self.fit.grow_due(now)
    }

    pub fn reset_fit(&mut self) {
        self.fit.reset();
        self.select(0);
        self.slack = None;
        self.no_room = false;
    }

    fn select(&mut self, rung: usize) {
        if rung != self.selected
            && let Some(r) = self.ladder.get(rung)
        {
            log::info!(
                "the taskbar has room for {:?}, {} px",
                r.tier,
                r.length().ceil()
            );
            self.layout = r.layout.clone();
            self.selected = rung;
        }
    }

    fn sized(&self, input: &PlacementInput, rung: usize) -> PlacementInput {
        let (overlay_w, overlay_h) = self
            .ladder
            .get(rung)
            .map_or_else(|| self.size(), |r| pixels(&r.layout));
        PlacementInput {
            overlay_w,
            overlay_h,
            ..*input
        }
    }

    /// Where the overlay goes on the taskbar, choosing the layout that fits.
    /// Unless the taskbar is known to leave no room for today's spot, the full
    /// layout stays exactly there.
    pub fn fit_to(
        &mut self,
        input: &PlacementInput,
        measured: Option<&Rects>,
        other_side: bool,
        now: Instant,
    ) -> Placement {
        let measured = measured.filter(|_| !self.ladder.is_empty());
        let Some(measured) = measured else {
            self.reset_fit();
            return place(&self.sized(input, 0));
        };
        let full = self.sized(input, 0);
        let today = place(&full);
        if matches!(today, Placement::Hidden(_)) {
            self.reset_fit();
            return today;
        }
        let tray = input
            .tray
            .filter(|t| t.width() > 0 && t.height() > 0)
            .or(measured.tray);
        let gaps = free_gaps(&input.taskbar, tray.as_ref(), &measured.occupied);
        let with_tray = PlacementInput { tray, ..*input };
        let order = gaps_in_order(&gaps, &with_tray, measured.centred, other_side);
        let room = room_in(&order, &with_tray);
        let lengths: Vec<f32> = self.ladder.iter().map(|r| r.length().ceil()).collect();
        let clear_today = self.fit.rung() == 0 && is_clear(today, &full, &gaps);
        let usable = if clear_today {
            (room as f32).max(lengths[0])
        } else {
            room as f32
        };
        let margin = scaled(GROW_MARGIN_DIP, input.dpi);
        let rung = self
            .fit
            .update(&lengths, Room::Known(usable), margin as f32, now);
        let no_room = rung >= lengths.len();
        if no_room != self.no_room {
            log::info!("the taskbar has room for nothing: {no_room}");
        }
        self.no_room = no_room;
        if self.no_room {
            self.select(0);
            self.slack = None;
            return Placement::Hidden(HideReason::NoRoom);
        }
        self.select(rung);
        self.slack = Some(room - lengths[rung] as i32);
        let sized = self.sized(input, rung);
        let spot = place(&sized);
        let sitting_at = self
            .shown_at
            .map(|(x, y)| start_along(&input.taskbar, x, y));
        let sized = PlacementInput { tray, ..sized };
        settle(spot, &sized, &gaps, &order, sitting_at, margin)
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

fn pixels(layout: &Layout) -> (i32, i32) {
    (layout.width.ceil() as i32, layout.height.round() as i32)
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
            work: Rect32::default(),
            dpi: 96,
        };
        Overlay::create(instance, taskbar).unwrap()
    }

    const BAR: Rect32 = Rect32 {
        left: 0,
        top: 1032,
        right: 1920,
        bottom: 1080,
    };
    const TRAY: Rect32 = Rect32 {
        left: 1609,
        top: 1032,
        right: 1920,
        bottom: 1080,
    };

    fn fitted(config: &Config) -> Overlay {
        let mut o = overlay();
        o.taskbar.rect = BAR;
        let text = Text::new(None, false).unwrap();
        o.update_layout(config, &GpuSlots::new(), &text);
        o
    }

    fn input(o: &Overlay) -> PlacementInput {
        let (overlay_w, overlay_h) = o.size();
        PlacementInput {
            taskbar: BAR,
            tray: Some(TRAY),
            monitor: Rect32 {
                left: 0,
                top: 0,
                right: 1920,
                bottom: 1080,
            },
            overlay_w,
            overlay_h,
            position: pulsar_core::config::Position::Right,
            offset_px: 0,
            fallback_margin_px: 160,
            dpi: 96,
        }
    }

    fn buttons_to(right: i32) -> Rects {
        Rects {
            occupied: vec![Rect32 {
                left: 0,
                top: 1032,
                right,
                bottom: 1080,
            }],
            tray: None,
            centred: false,
        }
    }

    #[test]
    fn plenty_of_room_stays_at_todays_spot_with_every_item() {
        let mut o = fitted(&Config::default());
        let full = o.size();
        let i = input(&o);
        let got = o.fit_to(&i, Some(&buttons_to(300)), true, Instant::now());
        assert_eq!(got, place(&i));
        assert_eq!(o.size(), full);
    }

    #[test]
    fn unknown_keeps_todays_spot_with_every_item() {
        let mut o = fitted(&Config::default());
        let i = input(&o);
        let got = o.fit_to(&i, None, true, Instant::now());
        assert_eq!(got, place(&i));
        assert_eq!(o.fit.rung(), 0);
    }

    #[test]
    fn today_is_kept_even_when_it_only_just_clears_the_buttons() {
        let mut o = fitted(&Config::default());
        let i = input(&o);
        let (x, _) = match place(&i) {
            Placement::At { x, y } => (x, y),
            other => panic!("{other:?}"),
        };
        let got = o.fit_to(&i, Some(&buttons_to(x)), true, Instant::now());
        assert_eq!(got, place(&i));
        assert_eq!(o.fit.rung(), 0);
    }

    #[test]
    fn a_covered_button_makes_it_shrink_at_once_and_clear_the_buttons() {
        let mut o = fitted(&Config::default());
        let full = o.size().0;
        let i = input(&o);
        let edge = TRAY.left - full / 2;
        let got = o.fit_to(&i, Some(&buttons_to(edge)), true, Instant::now());
        let (w, _) = o.size();
        assert!(w < full);
        let Placement::At { x, .. } = got else {
            panic!("{got:?}");
        };
        assert!(x >= edge, "{x} {edge}");
        assert!(x + w <= TRAY.left);
    }

    #[test]
    fn a_full_taskbar_has_no_room_and_shows_every_item_again() {
        let mut o = fitted(&Config::default());
        let full = o.size();
        let i = input(&o);
        let got = o.fit_to(&i, Some(&buttons_to(TRAY.left)), true, Instant::now());
        assert_eq!(got, Placement::Hidden(HideReason::NoRoom));
        assert!(o.no_room);
        assert_eq!(o.size(), full);
    }

    #[test]
    fn it_grows_back_only_after_the_hold_and_never_while_short() {
        let mut o = fitted(&Config::default());
        let full = o.size().0;
        let i = input(&o);
        let t0 = Instant::now();
        let edge = TRAY.left - full / 2;
        o.fit_to(&i, Some(&buttons_to(edge)), true, t0);
        assert!(o.size().0 < full);
        let free = buttons_to(300);
        o.fit_to(&i, Some(&free), true, t0);
        assert!(o.size().0 < full);
        assert!(!o.grow_due(t0));
        let later = t0 + fit::GROW_HOLD;
        assert!(o.grow_due(later));
        let got = o.fit_to(&i, Some(&free), true, later);
        assert_eq!(o.size().0, full);
        assert_eq!(got, place(&i));
    }

    #[test]
    fn a_centred_taskbar_uses_the_other_side_before_shrinking() {
        let mut o = fitted(&Config::default());
        let full = o.size().0;
        let i = input(&o);
        let centred = Rects {
            occupied: vec![Rect32 {
                left: TRAY.left - full / 2 - 200,
                top: 1032,
                right: TRAY.left - full / 2,
                bottom: 1080,
            }],
            tray: None,
            centred: true,
        };
        let got = o.fit_to(&i, Some(&centred), true, Instant::now());
        assert_eq!(o.size().0, full);
        assert_eq!(got, Placement::At { x: 8, y: 1032 });
    }

    #[test]
    fn with_the_other_side_off_a_centred_taskbar_shrinks() {
        let mut o = fitted(&Config::default());
        let full = o.size().0;
        let i = input(&o);
        let centred = Rects {
            occupied: vec![Rect32 {
                left: 400,
                top: 1032,
                right: TRAY.left - full / 2,
                bottom: 1080,
            }],
            tray: None,
            centred: true,
        };
        o.fit_to(&i, Some(&centred), false, Instant::now());
        assert!(o.size().0 < full);
    }

    #[test]
    fn a_secondary_taskbar_uses_the_measured_tray() {
        let mut o = fitted(&Config::default());
        let full = o.size().0;
        let i = PlacementInput {
            tray: None,
            ..input(&o)
        };
        let tray = Rect32 {
            left: 1845,
            top: 1041,
            right: 1896,
            bottom: 1070,
        };
        let rects = Rects {
            occupied: vec![tray],
            tray: Some(tray),
            centred: false,
        };
        let got = o.fit_to(&i, Some(&rects), true, Instant::now());
        assert_eq!(got, place(&i), "the fallback margin is clear of it");
        let close = Rect32 { left: 1600, ..tray };
        let rects = Rects {
            occupied: vec![close],
            tray: Some(close),
            centred: false,
        };
        let got = o.fit_to(&i, Some(&rects), true, Instant::now());
        assert_eq!(
            got,
            Placement::At {
                x: 1600 - 8 - full,
                y: 1032
            }
        );
    }

    #[test]
    fn fitting_turned_off_keeps_one_layout() {
        let mut config = Config::default();
        config.display.fit_to_taskbar = false;
        let mut o = fitted(&config);
        assert!(o.ladder.is_empty());
        let i = input(&o);
        let got = o.fit_to(&i, Some(&buttons_to(TRAY.left)), true, Instant::now());
        assert_eq!(got, place(&i));
    }

    #[test]
    fn a_floating_overlay_keeps_one_layout() {
        let mut o = overlay();
        o.taskbar.rect = BAR;
        o.float_dpi = Some(96);
        let text = Text::new(None, false).unwrap();
        o.update_layout(&Config::default(), &GpuSlots::new(), &text);
        assert!(o.ladder.is_empty());
    }

    #[test]
    fn a_tier_change_is_only_a_new_size_and_never_a_new_parent() {
        let mut o = fitted(&Config::default());
        let i = input(&o);
        let full = o.full_size();
        let edge = TRAY.left - full.0 / 2;
        let parent = unsafe { GetParent(o.hwnd) }.ok();
        o.fit_to(&i, Some(&buttons_to(edge)), true, Instant::now());
        assert!(o.size().0 < full.0);
        assert_eq!(o.full_size(), full);
        assert_eq!(unsafe { GetParent(o.hwnd) }.ok(), parent);
    }

    #[test]
    fn tier_style_follows_the_rung() {
        let mut config = Config::default();
        config.display.mode = DisplayMode::Text;
        let mut o = fitted(&config);
        assert_eq!(o.tier_style().unwrap().labels, LabelStyle::Full);
        let i = input(&o);
        let (w, _) = o.size();
        o.fit_to(
            &i,
            Some(&buttons_to(TRAY.left - w + 20)),
            true,
            Instant::now(),
        );
        assert_ne!(o.tier_style().unwrap().labels, LabelStyle::Full);
    }

    #[test]
    fn leaving_floating_returns_to_the_taskbar_height() {
        let mut overlay = overlay();
        overlay.taskbar.rect = Rect32 {
            left: 0,
            top: 1032,
            right: 1920,
            bottom: 1080,
        };
        let mut config = Config::default();
        config.display.font_size_pt = 20.0;
        let text = Text::new(None, false).unwrap();
        overlay.update_layout(&config, &GpuSlots::new(), &text);
        assert_eq!(overlay.size().1, 48);
        overlay.float_dpi = Some(96);
        overlay.update_layout(&config, &GpuSlots::new(), &text);
        assert!(overlay.size().1 > 48);
        overlay.float_dpi = None;
        overlay.update_layout(&config, &GpuSlots::new(), &text);
        assert_eq!(overlay.size().1, 48);
    }

    #[test]
    fn a_tile_size_redoes_the_layout_on_the_taskbar_and_on_the_game_strip() {
        let mut overlay = overlay();
        overlay.taskbar.rect = Rect32 {
            left: 0,
            top: 1032,
            right: 1920,
            bottom: 1080,
        };
        let mut config = Config::default();
        let text = Text::new(None, false).unwrap();
        overlay.update_layout(&config, &GpuSlots::new(), &text);
        let (auto_w, auto_h) = overlay.size();
        let first = config.enabled_items()[0];
        let item = config.item_mut(first).unwrap();
        item.tile_width = Some(200);
        item.tile_height = Some(100);
        overlay.update_layout(&config, &GpuSlots::new(), &text);
        assert!(overlay.size().0 > auto_w);
        assert_eq!(overlay.size().1, auto_h, "the bar limits a docked tile");

        // The game strip floats with its own copy of the settings.
        let strip = crate::game::strip_config(&config);
        overlay.float_dpi = Some(96);
        overlay.update_layout(&strip, &GpuSlots::new(), &text);
        assert_eq!(overlay.size().1, 108);
        assert!(overlay.layout.cells.iter().any(|c| c.rect.h == 100.0));

        overlay.float_dpi = None;
        overlay.update_layout(&config, &GpuSlots::new(), &text);
        assert_eq!(overlay.size().1, auto_h);
    }

    #[test]
    fn changing_the_network_cells_redoes_the_layout() {
        use pulsar_core::config::NetworkShow;
        use pulsar_core::layout::CellPart;
        let mut overlay = overlay();
        overlay.taskbar.rect = Rect32 {
            left: 0,
            top: 1032,
            right: 1920,
            bottom: 1080,
        };
        let mut config = Config::default();
        let text = Text::new(None, false).unwrap();
        let gpus = GpuSlots::new();
        let mut parts = |config: &Config| {
            overlay.update_layout(config, &gpus, &text);
            overlay
                .layout
                .cells
                .iter()
                .filter(|c| c.kind == pulsar_core::metric::ItemKind::Network)
                .map(|c| c.part)
                .collect::<Vec<_>>()
        };
        assert_eq!(parts(&config), [CellPart::Main]);
        config.network.show = NetworkShow::Upload;
        assert_eq!(parts(&config), [CellPart::Up]);
        config.network.show = NetworkShow::Download;
        assert_eq!(parts(&config), [CellPart::Down]);
        config.network.show = NetworkShow::Both;
        assert_eq!(parts(&config), [CellPart::Main]);
        config.network.split_tiles = true;
        assert_eq!(parts(&config), [CellPart::Down, CellPart::Up]);
        config.display.upload_on_top = true;
        assert_eq!(parts(&config), [CellPart::Up, CellPart::Down]);
        config.network.split_tiles = false;
        config.display.mode = DisplayMode::Text;
        assert_eq!(parts(&config), [CellPart::Up, CellPart::Down]);
        config.display.upload_on_top = false;
        assert_eq!(parts(&config), [CellPart::Down, CellPart::Up]);
        config.network.adapters = vec!["eth".into(), "wifi".into()];
        config.network.show = NetworkShow::Upload;
        assert_eq!(
            parts(&config),
            [CellPart::AdapterUp(0), CellPart::AdapterUp(1)]
        );
    }

    #[test]
    fn changing_a_text_gap_redoes_the_layout() {
        let mut overlay = overlay();
        overlay.taskbar.rect = Rect32 {
            left: 0,
            top: 1032,
            right: 1920,
            bottom: 1080,
        };
        let mut config = Config::default();
        config.display.mode = DisplayMode::Text;
        let text = Text::new(None, false).unwrap();
        overlay.update_layout(&config, &GpuSlots::new(), &text);
        let width = overlay.layout.width;
        config.display.text_item_gap = 40;
        overlay.update_layout(&config, &GpuSlots::new(), &text);
        assert!(overlay.layout.width > width);
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
