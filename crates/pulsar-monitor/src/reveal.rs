use std::time::{Duration, Instant};

use pulsar_core::config::{PushStrength, ScreenEdge};
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::Input::{
    GetRawInputData, HRAWINPUT, RAWINPUT, RAWINPUTDEVICE, RAWINPUTHEADER, RID_INPUT,
    RIDEV_INPUTSINK, RIDEV_REMOVE, RIM_TYPEMOUSE, RegisterRawInputDevices,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, HWND_TOPMOST, RegisterClassW,
    SPI_GETCLIENTAREAANIMATION, SW_HIDE, SW_SHOWNOACTIVATE, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
    SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, SetWindowPos, ShowWindow, SystemParametersInfoW,
    WNDCLASSW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT,
    WS_POPUP,
};
use windows::core::{BOOL, Result, w};

use crate::render::{Renderer, Surface};
use crate::taskbar::placement::Rect32;
use crate::theme::Color;

const PUSH_PAUSE: Duration = Duration::from_millis(250);
/// Pen, touch and remote sessions move the pointer to absolute positions,
/// which cannot push past the edge, so resting there opens it instead.
pub const REST: Duration = Duration::from_millis(300);
const OPEN: Duration = Duration::from_millis(220);
const CLOSE: Duration = Duration::from_millis(160);
const FADE: Duration = Duration::from_millis(120);
/// In DIPs.
pub const CORNER: f32 = 48.0;
pub const SLIDE: f32 = 36.0;
pub const REACH: f32 = 10.0;

/// Raw mouse counts, which do not scale with the pointer speed setting.
pub fn threshold(push: PushStrength) -> i32 {
    match push {
        PushStrength::Light => 40,
        PushStrength::Medium => 100,
        PushStrength::Firm => 220,
    }
}

pub fn outward(edge: ScreenEdge, dx: i32, dy: i32) -> (i32, i32) {
    match edge {
        ScreenEdge::Left => (-dx, dy),
        ScreenEdge::Right => (dx, dy),
        ScreenEdge::Top => (-dy, dx),
        ScreenEdge::Bottom => (dy, dx),
    }
}

/// Whether `at` is on `edge`'s outermost pixels of `monitor`, away from its
/// corners, where the title bar buttons and the Start button are.
pub fn on_edge(edge: ScreenEdge, at: (i32, i32), monitor: Rect32, corner: i32) -> bool {
    let (x, y) = at;
    let within = |v: i32, low: i32, high: i32| v >= low + corner && v < high - corner;
    match edge {
        ScreenEdge::Left => x <= monitor.left && within(y, monitor.top, monitor.bottom),
        ScreenEdge::Right => x >= monitor.right - 1 && within(y, monitor.top, monitor.bottom),
        ScreenEdge::Top => y <= monitor.top && within(x, monitor.left, monitor.right),
        ScreenEdge::Bottom => y >= monitor.bottom - 1 && within(x, monitor.left, monitor.right),
    }
}

/// The point just past `at` on `edge`, which must be off every screen for
/// a push to count; elsewhere the pointer just crosses to the next monitor.
pub fn beyond(edge: ScreenEdge, (x, y): (i32, i32)) -> (i32, i32) {
    match edge {
        ScreenEdge::Left => (x - 1, y),
        ScreenEdge::Right => (x + 1, y),
        ScreenEdge::Top => (x, y - 1),
        ScreenEdge::Bottom => (x, y + 1),
    }
}

/// The panel grown by `margin` and stretched to the screen edge it came
/// from, so moving back towards the edge does not hide it.
pub fn reach(panel: Rect32, edge: ScreenEdge, monitor: Rect32, margin: i32) -> Rect32 {
    let mut r = Rect32 {
        left: panel.left - margin,
        top: panel.top - margin,
        right: panel.right + margin,
        bottom: panel.bottom + margin,
    };
    match edge {
        ScreenEdge::Left => r.left = monitor.left,
        ScreenEdge::Right => r.right = monitor.right,
        ScreenEdge::Top => r.top = monitor.top,
        ScreenEdge::Bottom => r.bottom = monitor.bottom,
    }
    r
}

pub fn contains(r: Rect32, (x, y): (i32, i32)) -> bool {
    x >= r.left && x < r.right && y >= r.top && y < r.bottom
}

pub fn docked_at(edge: ScreenEdge, work: Rect32, (w, h): (i32, i32), gap: i32) -> (i32, i32) {
    let centered_x = work.left + (work.width() - w) / 2;
    match edge {
        ScreenEdge::Left => (work.left + gap, work.top + gap),
        ScreenEdge::Right => (work.right - gap - w, work.top + gap),
        ScreenEdge::Top => (centered_x, work.top + gap),
        ScreenEdge::Bottom => (centered_x, work.bottom - gap - h),
    }
}

pub fn slide_offset(edge: ScreenEdge, distance: i32) -> (i32, i32) {
    match edge {
        ScreenEdge::Left => (-distance, 0),
        ScreenEdge::Right => (distance, 0),
        ScreenEdge::Top => (0, -distance),
        ScreenEdge::Bottom => (0, distance),
    }
}

#[derive(Debug, Default)]
struct Push {
    total: i32,
    last: Option<Instant>,
}

impl Push {
    fn feed(&mut self, out: i32, along: i32, now: Instant, threshold: i32) -> bool {
        if self
            .last
            .is_some_and(|t| now.duration_since(t) > PUSH_PAUSE)
        {
            self.total = 0;
        }
        self.last = Some(now);
        if out < 0 {
            self.total = 0;
            return false;
        }
        if out > 0 && out >= along.abs() {
            self.total += out;
        }
        self.total >= threshold
    }

    fn reset(&mut self) {
        *self = Self::default();
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Phase {
    Hidden,
    Opening(Instant),
    Open,
    Closing(Instant),
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Pointer {
    pub at: (i32, i32),
    pub dx: i32,
    pub dy: i32,
    /// From a pen, touch screen or remote session.
    pub absolute: bool,
    pub pressed: bool,
    /// A button is held, as while dragging a window or selecting text.
    pub held: bool,
    /// Another monitor lies past the edge here, so the pointer can cross.
    pub crossing: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct Dock {
    pub edge: ScreenEdge,
    pub monitor: Rect32,
    pub corner: i32,
    pub margin: i32,
    pub threshold: i32,
    pub hide_after: Duration,
    /// Whether the edge may open the widget now, as it may not over a
    /// fullscreen game unless asked.
    pub allowed: bool,
    pub panel: Option<Rect32>,
}

#[derive(Debug)]
pub struct Reveal {
    pub phase: Phase,
    push: Push,
    /// After hiding, the pointer must leave the edge before it opens again.
    armed: bool,
    /// Opened by the shortcut: stays until dismissed.
    pub pinned: bool,
    entered: bool,
    resting_since: Option<Instant>,
    left_at: Option<Instant>,
    pub slide: bool,
}

impl Default for Reveal {
    fn default() -> Self {
        Self {
            phase: Phase::Hidden,
            push: Push::default(),
            armed: true,
            pinned: false,
            entered: false,
            resting_since: None,
            left_at: None,
            slide: true,
        }
    }
}

impl Reveal {
    pub fn showing(&self) -> bool {
        self.phase != Phase::Hidden
    }

    pub fn open(&mut self, now: Instant, pinned: bool) {
        self.pinned = pinned;
        self.entered = false;
        self.left_at = None;
        self.resting_since = None;
        self.push.reset();
        if !matches!(self.phase, Phase::Open | Phase::Opening(_)) {
            self.phase = Phase::Opening(now);
        }
    }

    pub fn close(&mut self, now: Instant) {
        self.pinned = false;
        self.left_at = None;
        if matches!(self.phase, Phase::Open | Phase::Opening(_)) {
            self.phase = Phase::Closing(now);
        }
    }

    /// Reacts to a pointer movement; true when a timer is needed to finish
    /// what it started.
    pub fn on_pointer(&mut self, p: &Pointer, dock: &Dock, now: Instant) -> bool {
        match self.phase {
            Phase::Hidden | Phase::Closing(_) => self.while_hidden(p, dock, now),
            Phase::Open | Phase::Opening(_) => self.while_shown(p, dock, now),
        }
        self.needs_ticks()
    }

    fn while_hidden(&mut self, p: &Pointer, dock: &Dock, now: Instant) {
        let touching = on_edge(dock.edge, p.at, dock.monitor, dock.corner) && !p.crossing;
        if !touching {
            self.armed = true;
        }
        if !touching || !dock.allowed || p.held || !self.armed {
            self.push.reset();
            self.resting_since = None;
            return;
        }
        if p.absolute {
            self.resting_since.get_or_insert(now);
            return;
        }
        let (out, along) = outward(dock.edge, p.dx, p.dy);
        if self.push.feed(out, along, now, dock.threshold) {
            self.open(now, false);
        }
    }

    fn while_shown(&mut self, p: &Pointer, dock: &Dock, now: Instant) {
        let Some(panel) = dock.panel else {
            return;
        };
        if p.pressed && !contains(panel, p.at) {
            self.close(now);
            return;
        }
        if contains(reach(panel, dock.edge, dock.monitor, dock.margin), p.at) {
            self.entered = true;
            self.left_at = None;
        } else if self.entered && !self.pinned {
            self.left_at.get_or_insert(now);
        }
    }

    pub fn tick(&mut self, cursor: (i32, i32), dock: &Dock, now: Instant) -> bool {
        match self.phase {
            Phase::Hidden => {
                if let Some(since) = self.resting_since {
                    let touching = on_edge(dock.edge, cursor, dock.monitor, dock.corner);
                    if !touching || !dock.allowed {
                        self.resting_since = None;
                    } else if now.duration_since(since) >= REST {
                        self.open(now, false);
                    }
                }
            }
            Phase::Opening(start) => {
                if now.duration_since(start) >= self.open_time() {
                    self.phase = Phase::Open;
                }
            }
            Phase::Open => {
                if let (Some(left), Some(panel)) = (self.left_at, dock.panel) {
                    if contains(reach(panel, dock.edge, dock.monitor, dock.margin), cursor) {
                        self.left_at = None;
                    } else if now.duration_since(left) >= dock.hide_after {
                        self.close(now);
                    }
                }
            }
            Phase::Closing(start) => {
                if now.duration_since(start) >= self.close_time() {
                    self.phase = Phase::Hidden;
                    self.armed = false;
                }
            }
        }
        self.needs_ticks()
    }

    pub fn needs_ticks(&self) -> bool {
        match self.phase {
            Phase::Hidden => self.resting_since.is_some(),
            Phase::Open => self.left_at.is_some(),
            Phase::Opening(_) | Phase::Closing(_) => true,
        }
    }

    pub fn animating(&self) -> bool {
        matches!(self.phase, Phase::Opening(_) | Phase::Closing(_))
    }

    fn open_time(&self) -> Duration {
        if self.slide { OPEN } else { FADE }
    }

    fn close_time(&self) -> Duration {
        if self.slide { CLOSE } else { FADE }
    }

    pub fn shown(&self, now: Instant) -> f32 {
        let part = |start: Instant, length: Duration| {
            (now.duration_since(start).as_secs_f32() / length.as_secs_f32()).clamp(0.0, 1.0)
        };
        match self.phase {
            Phase::Hidden => 0.0,
            Phase::Open => 1.0,
            Phase::Opening(start) => {
                let t = part(start, self.open_time());
                1.0 - (1.0 - t).powi(3)
            }
            Phase::Closing(start) => {
                let t = part(start, self.close_time());
                1.0 - t * t * t
            }
        }
    }
}

pub fn lip_rect(edge: ScreenEdge, monitor: Rect32, scale: f32) -> Rect32 {
    let thick = (3.0 * scale).round().max(2.0) as i32;
    let span = |low: i32, high: i32| {
        let length = ((high - low) as f32 * 0.3).max(80.0 * scale) as i32;
        let start = low + (high - low - length) / 2;
        (start, start + length)
    };
    match edge {
        ScreenEdge::Left | ScreenEdge::Right => {
            let (top, bottom) = span(monitor.top, monitor.bottom);
            let left = if edge == ScreenEdge::Left {
                monitor.left
            } else {
                monitor.right - thick
            };
            Rect32 {
                left,
                top,
                right: left + thick,
                bottom,
            }
        }
        ScreenEdge::Top | ScreenEdge::Bottom => {
            let (left, right) = span(monitor.left, monitor.right);
            let top = if edge == ScreenEdge::Top {
                monitor.top
            } else {
                monitor.bottom - thick
            };
            Rect32 {
                left,
                top,
                right,
                bottom: top + thick,
            }
        }
    }
}

pub struct Lip {
    hwnd: HWND,
    drawn: Option<(Rect32, Color)>,
}

impl Lip {
    pub fn create(instance: HINSTANCE) -> Result<Self> {
        static CLASS_ONCE: std::sync::Once = std::sync::Once::new();
        CLASS_ONCE.call_once(|| unsafe {
            RegisterClassW(&WNDCLASSW {
                lpfnWndProc: Some(lip_proc),
                hInstance: instance,
                lpszClassName: w!("PulsarLip"),
                ..Default::default()
            });
        });
        let hwnd = unsafe {
            CreateWindowExW(
                WS_EX_LAYERED
                    | WS_EX_TRANSPARENT
                    | WS_EX_TOOLWINDOW
                    | WS_EX_NOACTIVATE
                    | WS_EX_TOPMOST,
                w!("PulsarLip"),
                w!("Pulsar edge"),
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
        Ok(Self { hwnd, drawn: None })
    }

    pub fn show(&mut self, renderer: &mut Renderer, at: Rect32, color: Color) -> Result<()> {
        if self.drawn == Some((at, color)) {
            return Ok(());
        }
        let surface = Surface::new(at.width(), at.height())?;
        renderer.paint(&surface, color, |_| Ok(()))?;
        renderer.present(self.hwnd, &surface, at.left, at.top)?;
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_SHOWNOACTIVATE);
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
        self.drawn = Some((at, color));
        Ok(())
    }

    pub fn hide(&mut self) {
        if self.drawn.take().is_some() {
            unsafe {
                let _ = ShowWindow(self.hwnd, SW_HIDE);
            }
        }
    }
}

impl Drop for Lip {
    fn drop(&mut self) {
        unsafe {
            let _ = DestroyWindow(self.hwnd);
        }
    }
}

unsafe extern "system" fn lip_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe { DefWindowProcW(hwnd, msg, wp, lp) }
}

pub fn listen(host: HWND, on: bool) -> bool {
    let device = RAWINPUTDEVICE {
        usUsagePage: 0x01,
        usUsage: 0x02,
        dwFlags: if on { RIDEV_INPUTSINK } else { RIDEV_REMOVE },
        hwndTarget: if on { host } else { HWND::default() },
    };
    unsafe { RegisterRawInputDevices(&[device], size_of::<RAWINPUTDEVICE>() as u32) }.is_ok()
}

const MOUSE_MOVE_ABSOLUTE: u16 = 0x01;
const BUTTONS_DOWN: u16 = 0x0001 | 0x0004 | 0x0010 | 0x0040 | 0x0100;

pub fn read_mouse(lp: LPARAM) -> Option<(i32, i32, bool, bool)> {
    let mut input = RAWINPUT::default();
    let mut size = size_of::<RAWINPUT>() as u32;
    let read = unsafe {
        GetRawInputData(
            HRAWINPUT(lp.0 as _),
            RID_INPUT,
            Some((&raw mut input).cast()),
            &mut size,
            size_of::<RAWINPUTHEADER>() as u32,
        )
    };
    if read == u32::MAX || input.header.dwType != RIM_TYPEMOUSE.0 {
        return None;
    }
    let mouse = unsafe { input.data.mouse };
    let buttons = unsafe { mouse.Anonymous.Anonymous.usButtonFlags };
    Some((
        mouse.lLastX,
        mouse.lLastY,
        mouse.usFlags.0 & MOUSE_MOVE_ABSOLUTE != 0,
        buttons & BUTTONS_DOWN != 0,
    ))
}

pub fn animations_enabled() -> bool {
    let mut on = BOOL::from(true);
    unsafe {
        let _ = SystemParametersInfoW(
            SPI_GETCLIENTAREAANIMATION,
            0,
            Some((&raw mut on).cast()),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        );
    }
    on.as_bool()
}

#[cfg(test)]
mod tests {
    use super::*;

    const MONITOR: Rect32 = Rect32 {
        left: 0,
        top: 0,
        right: 1920,
        bottom: 1080,
    };
    const PANEL: Rect32 = Rect32 {
        left: 1500,
        top: 16,
        right: 1904,
        bottom: 600,
    };

    fn dock() -> Dock {
        Dock {
            edge: ScreenEdge::Right,
            monitor: MONITOR,
            corner: 48,
            margin: 10,
            threshold: 100,
            hide_after: Duration::from_millis(300),
            allowed: true,
            panel: Some(PANEL),
        }
    }

    fn push(at: (i32, i32), dx: i32) -> Pointer {
        Pointer {
            at,
            dx,
            ..Pointer::default()
        }
    }

    fn ms(start: Instant, n: u64) -> Instant {
        start + Duration::from_millis(n)
    }

    #[test]
    fn only_the_outer_pixels_away_from_the_corners_count() {
        let corner = 48;
        assert!(on_edge(ScreenEdge::Right, (1919, 500), MONITOR, corner));
        assert!(!on_edge(ScreenEdge::Right, (1918, 500), MONITOR, corner));
        assert!(!on_edge(ScreenEdge::Right, (1919, 20), MONITOR, corner));
        assert!(!on_edge(ScreenEdge::Right, (1919, 1060), MONITOR, corner));
        assert!(on_edge(ScreenEdge::Bottom, (900, 1079), MONITOR, corner));
        assert!(on_edge(ScreenEdge::Top, (900, 0), MONITOR, corner));
        assert!(on_edge(ScreenEdge::Left, (0, 500), MONITOR, corner));
        assert!(!on_edge(ScreenEdge::Left, (0, 10), MONITOR, corner));
    }

    #[test]
    fn a_steady_push_opens_and_a_brush_past_does_not() {
        let t = Instant::now();
        let mut r = Reveal::default();
        let d = dock();
        for i in 0..4 {
            r.on_pointer(&push((1919, 500), 20), &d, ms(t, i * 10));
        }
        assert_eq!(r.phase, Phase::Hidden, "80 counts is not enough");
        r.on_pointer(&push((1919, 500), 20), &d, ms(t, 50));
        assert!(matches!(r.phase, Phase::Opening(_)));

        let mut r = Reveal::default();
        r.on_pointer(&push((1919, 500), 60), &d, t);
        r.on_pointer(&push((1919, 500), 60), &d, ms(t, 400));
        assert_eq!(r.phase, Phase::Hidden, "a pause starts over");
        r.on_pointer(&push((1919, 500), -5), &d, ms(t, 410));
        r.on_pointer(&push((1919, 500), 60), &d, ms(t, 420));
        assert_eq!(r.phase, Phase::Hidden, "moving back starts over");
    }

    #[test]
    fn sideways_moves_along_the_edge_do_not_push() {
        let t = Instant::now();
        let mut r = Reveal::default();
        let d = dock();
        for i in 0..20 {
            let p = Pointer {
                at: (1919, 300 + i * 10),
                dx: 5,
                dy: 30,
                ..Pointer::default()
            };
            r.on_pointer(&p, &d, ms(t, i as u64 * 5));
        }
        assert_eq!(r.phase, Phase::Hidden);
    }

    #[test]
    fn a_held_button_a_corner_or_a_fullscreen_game_keeps_it_hidden() {
        let t = Instant::now();
        let d = dock();
        let mut held = Reveal::default();
        let mut corner = Reveal::default();
        let mut game = Reveal::default();
        let blocked = Dock {
            allowed: false,
            ..dock()
        };
        for i in 0..10 {
            let now = ms(t, i * 10);
            let mut p = push((1919, 500), 50);
            p.held = true;
            held.on_pointer(&p, &d, now);
            corner.on_pointer(&push((1919, 10), 50), &d, now);
            game.on_pointer(&push((1919, 500), 50), &blocked, now);
        }
        assert_eq!(held.phase, Phase::Hidden);
        assert_eq!(corner.phase, Phase::Hidden);
        assert_eq!(game.phase, Phase::Hidden);
    }

    #[test]
    fn an_edge_shared_with_another_monitor_does_not_push() {
        let t = Instant::now();
        let d = dock();
        let mut r = Reveal::default();
        for i in 0..10 {
            let mut p = push((1919, 500), 50);
            p.crossing = true;
            r.on_pointer(&p, &d, ms(t, i * 10));
        }
        assert_eq!(r.phase, Phase::Hidden);
    }

    #[test]
    fn resting_an_absolute_pointer_on_the_edge_opens_it() {
        let t = Instant::now();
        let d = dock();
        let mut r = Reveal::default();
        let p = Pointer {
            at: (1919, 500),
            absolute: true,
            ..Pointer::default()
        };
        assert!(r.on_pointer(&p, &d, t), "needs a timer");
        r.tick((1919, 500), &d, ms(t, 200));
        assert_eq!(r.phase, Phase::Hidden);
        r.tick((1919, 500), &d, ms(t, 310));
        assert!(matches!(r.phase, Phase::Opening(_)));
    }

    #[test]
    fn it_hides_after_the_pointer_leaves_and_stays_hidden_until_it_leaves_the_edge() {
        let t = Instant::now();
        let d = dock();
        let mut r = Reveal::default();
        r.open(t, false);
        r.tick((1919, 500), &d, ms(t, 300));
        assert_eq!(r.phase, Phase::Open);
        r.on_pointer(&push((1700, 300), -5), &d, ms(t, 310));
        r.on_pointer(&push((1919, 300), 5), &d, ms(t, 320));
        r.tick((1919, 300), &d, ms(t, 1000));
        assert_eq!(
            r.phase,
            Phase::Open,
            "the strip out to the edge is in reach"
        );
        r.on_pointer(&push((800, 300), -50), &d, ms(t, 1010));
        r.tick((800, 300), &d, ms(t, 1200));
        assert_eq!(r.phase, Phase::Open, "not yet");
        r.tick((800, 300), &d, ms(t, 1320));
        assert!(matches!(r.phase, Phase::Closing(_)));
        r.tick((800, 300), &d, ms(t, 1500));
        assert_eq!(r.phase, Phase::Hidden);
        let hidden = Dock { panel: None, ..d };
        for i in 0..10 {
            r.on_pointer(&push((1919, 500), 50), &hidden, ms(t, 1600 + i * 10));
        }
        assert_eq!(
            r.phase,
            Phase::Hidden,
            "not until the pointer left the edge"
        );
        r.on_pointer(&push((1800, 500), -5), &hidden, ms(t, 1800));
        for i in 0..3 {
            r.on_pointer(&push((1919, 500), 50), &hidden, ms(t, 1810 + i * 10));
        }
        assert!(matches!(r.phase, Phase::Opening(_)));
    }

    #[test]
    fn a_click_elsewhere_hides_at_once_but_not_a_click_on_it() {
        let t = Instant::now();
        let d = dock();
        let mut r = Reveal::default();
        r.open(t, true);
        let mut on = push((1600, 100), 0);
        on.pressed = true;
        r.on_pointer(&on, &d, ms(t, 10));
        assert!(matches!(r.phase, Phase::Opening(_)));
        let mut away = push((300, 300), 0);
        away.pressed = true;
        r.on_pointer(&away, &d, ms(t, 20));
        assert!(matches!(r.phase, Phase::Closing(_)));
    }

    #[test]
    fn a_pinned_widget_ignores_the_pointer_leaving() {
        let t = Instant::now();
        let d = dock();
        let mut r = Reveal::default();
        r.open(t, true);
        r.on_pointer(&push((1600, 100), 0), &d, ms(t, 10));
        r.on_pointer(&push((200, 100), 0), &d, ms(t, 20));
        r.tick((200, 100), &d, ms(t, 2000));
        assert_eq!(r.phase, Phase::Open);
    }

    #[test]
    fn docked_widgets_sit_inside_the_work_area() {
        let work = Rect32 {
            bottom: 1032,
            ..MONITOR
        };
        let size = (400, 200);
        assert_eq!(docked_at(ScreenEdge::Right, work, size, 8), (1512, 8));
        assert_eq!(docked_at(ScreenEdge::Left, work, size, 8), (8, 8));
        assert_eq!(docked_at(ScreenEdge::Top, work, size, 8), (760, 8));
        assert_eq!(
            docked_at(ScreenEdge::Bottom, work, size, 8),
            (760, 824),
            "above the taskbar"
        );
    }

    #[test]
    fn the_lip_lies_along_the_middle_of_its_edge() {
        let right = lip_rect(ScreenEdge::Right, MONITOR, 1.0);
        assert_eq!((right.left, right.right), (1917, 1920));
        assert_eq!((right.top, right.bottom), (378, 702));
        let bottom = lip_rect(ScreenEdge::Bottom, MONITOR, 2.0);
        assert_eq!((bottom.top, bottom.bottom), (1074, 1080));
        assert_eq!(bottom.width(), 576);
    }

    #[test]
    fn showing_eases_from_nothing_to_whole() {
        let t = Instant::now();
        let mut r = Reveal::default();
        r.open(t, false);
        assert_eq!(r.shown(t), 0.0);
        let half = r.shown(ms(t, 110));
        assert!(half > 0.5 && half < 1.0, "eases out: {half}");
        assert_eq!(r.shown(ms(t, 400)), 1.0);
    }
}
