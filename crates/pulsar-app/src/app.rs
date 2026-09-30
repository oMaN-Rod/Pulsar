use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::sync::atomic::Ordering;

use pulsar_core::config::{self, Config};
use pulsar_core::history::HistoryStore;
use pulsar_core::ipc;
use pulsar_core::metric::{ItemKind, Snapshot};
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    TME_HOVER, TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent,
};
use windows::Win32::UI::Shell::NIN_SELECT;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetCursorPos, GetMessageW,
    KillTimer, MA_NOACTIVATE, MSG, PostQuitMessage, RegisterClassW, RegisterWindowMessageW,
    SetTimer, TranslateMessage, WM_CONTEXTMENU, WM_DESTROY, WM_DISPLAYCHANGE, WM_LBUTTONUP,
    WM_MOUSEACTIVATE, WM_MOUSEMOVE, WM_RBUTTONUP, WM_SETTINGCHANGE, WM_TIMER, WNDCLASSW,
    WS_EX_TOOLWINDOW, WS_POPUP,
};
use windows::core::{HSTRING, PCWSTR, Result, w};

use crate::hover::{HoverArming, popup_orphaned};
use crate::launcher::{self, Page};
use crate::menu::{self, Command};
use crate::messages::{WM_APP_FOREGROUND, WM_APP_SNAPSHOT, WM_APP_TASKBAR, WM_APP_TRAY};
use crate::overlay::{self, Overlay};
use crate::popup::content::needs_processes;
use crate::popup::layout::item_screen_rect;
use crate::popup::{self, Anchor, Popup, Style as PopupStyle};
use crate::render::{Frame, Renderer};
use crate::sampler_thread::SamplerThread;
use crate::taskbar::hooks::{self, Hooks};
use crate::taskbar::placement::{Placement, PlacementInput, Rect32, place, tray_is_laid_out};
use crate::taskbar::{self, Taskbar};
use crate::text::Text;
use crate::theme::{Palette, accent_color, taskbar_is_light};
use crate::tray::Tray;

const TIMER_WATCH: usize = 1;
const TIMER_VALIDATE: usize = 2;
const TIMER_RETRY: usize = 3;
const WATCH_MS: u32 = 250;
const VALIDATE_MS: u32 = 2000;
/// Explorer lays out the tray some time after announcing the taskbar.
const RETRY_MS: [u32; 4] = [100, 250, 500, 1000];

const HOVER_MS: u32 = 300;
/// `NIN_SELECT | NINF_KEY`: the tray icon was activated from the keyboard.
const NIN_KEYSELECT: u32 = NIN_SELECT | 1;
const WM_MOUSEHOVER: u32 = 0x02A1;
const WM_MOUSELEAVE: u32 = 0x02A3;

/// Broadcasts that arrived while the app was busy; replayed on the next tick.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Deferred {
    taskbar_created: bool,
    settings_changed: bool,
    config_changed: bool,
}

thread_local! {
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
    static TASKBAR_CREATED: Cell<u32> = const { Cell::new(0) };
    static CONFIG_CHANGED: Cell<u32> = const { Cell::new(0) };
    static DEFERRED: Cell<Deferred> = const {
        Cell::new(Deferred {
            taskbar_created: false,
            settings_changed: false,
            config_changed: false,
        })
    };
}

fn defer(mark: impl FnOnce(&mut Deferred)) {
    let mut deferred = DEFERRED.get();
    mark(&mut deferred);
    DEFERRED.set(deferred);
}

fn take_deferred() -> Deferred {
    DEFERRED.take()
}

/// Runs `f` against the app. Returns `None` when the app is already borrowed
/// further up the stack (a modal menu loop or a synchronous window message);
/// snapshots are drained by the next one, repositioning is redone by the
/// timers, and missed broadcasts are replayed through `Deferred`.
fn with_app<R>(f: impl FnOnce(&mut App) -> R) -> Option<R> {
    APP.with(|cell| {
        let mut guard = cell.try_borrow_mut().ok()?;
        guard.as_mut().map(f)
    })
}

pub struct App {
    instance: HINSTANCE,
    host: HWND,
    config: Config,
    config_path: Option<PathBuf>,
    text: Text,
    renderer: Renderer,
    palette: Palette,
    history: HistoryStore,
    latest: Snapshot,
    sampler: SamplerThread,
    overlays: Vec<Overlay>,
    tray: Tray,
    hooks: Option<(u32, Hooks)>,
    fullscreen: Option<Rect32>,
    retry_step: usize,
    popup: Popup,
    /// The overlay the open popup belongs to.
    popup_owner: Option<HWND>,
    hover: HoverArming,
}

impl App {
    fn new(instance: HINSTANCE, host: HWND) -> Result<Self> {
        let config_path = config::default_path();
        let loaded = config_path.as_deref().map(config::load);
        let (config, warning) = match loaded {
            Some(l) => (l.config, l.warning),
            None => (Config::default(), None),
        };
        let tray = Tray::new(host);
        if let Some(warning) = warning {
            tray.warn("Pulsar settings reset", &warning);
        }
        Ok(Self {
            instance,
            host,
            palette: Palette::new(taskbar_is_light(), &config, accent_color()),
            history: HistoryStore::new(config.general.history_len),
            sampler: SamplerThread::spawn(&config, Some(host)),
            text: Text::new(
                config.display.font_family.as_deref(),
                config.display.font_bold,
            )?,
            config,
            config_path,
            renderer: Renderer::new()?,
            latest: Snapshot::default(),
            overlays: Vec::new(),
            tray,
            hooks: None,
            fullscreen: None,
            retry_step: 0,
            popup: Popup::create(instance)?,
            popup_owner: None,
            hover: HoverArming::default(),
        })
    }

    fn start(&mut self) {
        unsafe {
            SetTimer(Some(self.host), TIMER_WATCH, WATCH_MS, None);
            SetTimer(Some(self.host), TIMER_VALIDATE, VALIDATE_MS, None);
        }
        self.fullscreen = taskbar::fullscreen_monitor();
        self.refresh();
    }

    /// Rediscovers taskbars, reconciles overlays with them and repositions.
    fn refresh(&mut self) {
        let found = taskbar::discover(self.config.display.show_on_all_taskbars);
        self.overlays
            .retain(|o| found.iter().any(|t| t.hwnd == o.taskbar.hwnd));
        for tb in found {
            match self.overlays.iter_mut().find(|o| o.taskbar.hwnd == tb.hwnd) {
                Some(o) => o.taskbar = tb,
                None => {
                    if let Ok(o) = Overlay::create(self.instance, tb) {
                        self.overlays.push(o);
                    }
                }
            }
        }
        self.install_hooks();
        let mut tray_pending = false;
        for i in 0..self.overlays.len() {
            let tb = &self.overlays[i].taskbar;
            if tb.tray.is_some() && !tray_is_laid_out(&tb.rect, tb.tray_rect.as_ref()) {
                tray_pending = true;
            }
            self.show(i);
        }
        if tray_pending {
            self.schedule_retry();
        } else {
            self.retry_step = 0;
        }
        let live: Vec<isize> = self.overlays.iter().map(|o| key(o.hwnd)).collect();
        self.hover.retain_live(&live);
        let shown: Vec<isize> = self
            .overlays
            .iter()
            .filter(|o| o.shown_at.is_some())
            .map(|o| key(o.hwnd))
            .collect();
        if popup_orphaned(self.popup_owner.map(key), &shown) {
            self.close_popup();
        }
    }

    fn install_hooks(&mut self) {
        let Some(primary) = self.overlays.first().map(|o| o.taskbar.hwnd) else {
            return;
        };
        let pid = taskbar::process_id(primary);
        if self.hooks.as_ref().is_none_or(|(p, _)| *p != pid) {
            self.hooks = Some((pid, Hooks::install(self.host, pid)));
        }
        let watched = self
            .overlays
            .iter()
            .flat_map(|o| [Some(o.taskbar.hwnd), o.taskbar.tray])
            .flatten()
            .collect();
        hooks::watch(watched);
    }

    fn schedule_retry(&mut self) {
        if let Some(&ms) = RETRY_MS.get(self.retry_step) {
            self.retry_step += 1;
            unsafe { SetTimer(Some(self.host), TIMER_RETRY, ms, None) };
        }
    }

    fn show(&mut self, i: usize) {
        let o = &mut self.overlays[i];
        o.update_layout(&self.config, &self.text);
        let (w, h) = o.size();
        let tb: &Taskbar = &o.taskbar;
        let placement = place(&PlacementInput {
            taskbar: tb.rect,
            tray: tb.tray_rect,
            monitor: tb.monitor,
            overlay_w: w,
            overlay_h: h,
            position: self.config.display.position,
            offset_px: self.config.display.offset_px,
            fallback_margin_px: self.config.display.fallback_margin_px,
            dpi: tb.dpi,
        });
        let covered = self.fullscreen == Some(tb.monitor);
        match placement {
            Placement::At { x, y } if !covered => {
                if self.draw(i, x, y).is_err() {
                    self.overlays[i].hide();
                }
            }
            _ => self.overlays[i].hide(),
        }
    }

    fn draw(&mut self, i: usize, x: i32, y: i32) -> Result<()> {
        let o = &mut self.overlays[i];
        let dpi = o.taskbar.dpi;
        let hwnd = o.hwnd;
        let layout = o.layout.clone();
        let surface = o.surface()?;
        let frame = Frame {
            layout: &layout,
            mode: self.config.display.mode,
            palette: &self.palette,
            snapshot: &self.latest,
            history: &self.history,
            dpi,
            short_labels: self.config.display.short_labels,
        };
        self.renderer.draw(surface, &frame, &self.text)?;
        self.renderer.present(hwnd, surface, x, y)?;
        o.mark_shown(x, y);
        Ok(())
    }

    fn redraw(&mut self) {
        for i in 0..self.overlays.len() {
            if let Some((x, y)) = self.overlays[i].shown_at
                && self.draw(i, x, y).is_err()
            {
                self.overlays[i].hide();
            }
        }
    }

    fn on_snapshot(&mut self) {
        let mut received = false;
        while let Ok(snapshot) = self.sampler.snapshots.try_recv() {
            self.history.record(&snapshot);
            self.latest = snapshot;
            received = true;
        }
        if received {
            self.redraw();
            if let (Some((kind, _)), Some(owner)) = (self.popup.open, self.popup_owner) {
                self.open_popup(owner, kind);
            }
        }
    }

    fn hit(&self, overlay: HWND, x: i32, y: i32) -> Option<ItemKind> {
        let o = self.overlays.iter().find(|o| o.hwnd == overlay)?;
        o.layout.hit_test(x as f32, y as f32)
    }

    fn on_mouse_move(&mut self, overlay: HWND, x: i32, y: i32) {
        if self.hover.arm(key(overlay)) {
            let mut request = TRACKMOUSEEVENT {
                cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
                dwFlags: TME_HOVER | TME_LEAVE,
                hwndTrack: overlay,
                dwHoverTime: HOVER_MS,
            };
            if unsafe { TrackMouseEvent(&mut request) }.is_err() {
                self.hover.disarm(key(overlay));
            }
        }
        if let Some((open, _)) = self.popup.open
            && let Some(kind) = self.hit(overlay, x, y)
            && kind != open
        {
            self.open_popup(overlay, kind);
        }
    }

    fn on_mouse_hover(&mut self, overlay: HWND, x: i32, y: i32) {
        if self.config.display.hover_popup
            && let Some(kind) = self.hit(overlay, x, y)
        {
            self.open_popup(overlay, kind);
        }
        if self.popup.open.is_none() {
            self.hover.disarm(key(overlay));
        }
    }

    fn on_mouse_leave(&mut self, overlay: HWND) {
        self.hover.disarm(key(overlay));
        self.close_popup();
    }

    fn on_click(&mut self, overlay: HWND) {
        self.close_popup();
        self.hover.disarm(key(overlay));
    }

    fn open_popup(&mut self, overlay: HWND, kind: ItemKind) {
        let Some(o) = self.overlays.iter().find(|o| o.hwnd == overlay) else {
            return;
        };
        let Some(item) = o
            .shown_at
            .and_then(|origin| item_screen_rect(&o.layout, kind, origin))
        else {
            return;
        };
        let anchor = Anchor {
            item,
            taskbar: o.taskbar.rect,
            monitor: o.taskbar.monitor,
            dpi: o.taskbar.dpi,
        };
        self.sampler
            .processes
            .store(needs_processes(kind), Ordering::Relaxed);
        self.popup_owner = Some(overlay);
        self.show_popup(kind, anchor);
    }

    fn show_popup(&mut self, kind: ItemKind, anchor: Anchor) {
        let content = popup::content::build(
            kind,
            &self.latest,
            &self.history,
            &self.config.ping.host,
            &self.config.drive_letters(),
        );
        let style = PopupStyle {
            palette: &self.palette,
            history: &self.history,
            font_px: self.config.display.font_size_pt * anchor.dpi as f32 / 72.0,
        };
        if self
            .popup
            .show(anchor, &content, &style, &self.text)
            .is_err()
        {
            self.close_popup();
        }
    }

    fn close_popup(&mut self) {
        self.popup.hide();
        self.popup_owner = None;
        self.sampler.processes.store(false, Ordering::Relaxed);
    }

    /// Runs every 250 ms and on foreground changes: hides for fullscreen apps
    /// and lifts overlays that the taskbar has been raised above.
    fn watch(&mut self) {
        let fullscreen = taskbar::fullscreen_monitor();
        if fullscreen != self.fullscreen {
            self.fullscreen = fullscreen;
            self.refresh();
            return;
        }
        for o in &self.overlays {
            if o.shown_at.is_some() && taskbar::is_behind(o.hwnd, o.taskbar.hwnd) {
                o.raise();
            }
        }
    }

    fn on_timer(&mut self, id: usize) {
        let deferred = take_deferred();
        if deferred.taskbar_created {
            self.on_taskbar_created();
        }
        if deferred.settings_changed {
            self.on_settings_changed();
        }
        if deferred.config_changed {
            self.reload_config();
        }
        match id {
            TIMER_WATCH => self.watch(),
            TIMER_VALIDATE => self.refresh(),
            TIMER_RETRY => {
                unsafe {
                    let _ = KillTimer(Some(self.host), TIMER_RETRY);
                }
                self.refresh();
            }
            _ => {}
        }
    }

    fn on_taskbar_created(&mut self) {
        self.tray.add();
        self.retry_step = 0;
        self.refresh();
        self.schedule_retry();
    }

    fn on_settings_changed(&mut self) {
        self.palette = Palette::new(taskbar_is_light(), &self.config, accent_color());
        self.redraw();
    }

    fn open_settings(&self, page: Page) {
        if let Err(e) = launcher::open(page) {
            self.tray
                .warn("Pulsar could not open Settings", &e.to_string());
        }
    }

    /// Applies a config saved by the settings process. Only a change to the
    /// sampled sources, interval or ping target restarts the sampler; layout,
    /// palette and placement are recomputed by `refresh`.
    fn reload_config(&mut self) {
        let Some(path) = &self.config_path else {
            return;
        };
        let new = config::load(path).config;
        if new == self.config {
            return;
        }
        if config::sampling_changed(&self.config, &new) {
            self.close_popup();
            self.sampler = SamplerThread::spawn(&new, Some(self.host));
        }
        if new.general.history_len != self.config.general.history_len {
            self.history = HistoryStore::new(new.general.history_len);
        }
        let font = |c: &Config| (c.display.font_family.clone(), c.display.font_bold);
        if font(&new) != font(&self.config)
            && let Ok(text) = Text::new(new.display.font_family.as_deref(), new.display.font_bold)
        {
            self.text = text;
            self.overlays
                .iter_mut()
                .for_each(Overlay::invalidate_layout);
        }
        self.config = new;
        self.palette = Palette::new(taskbar_is_light(), &self.config, accent_color());
        if !self.config.display.hover_popup {
            self.close_popup();
        }
        self.refresh();
        self.redraw();
    }

    fn apply(&mut self, command: Command) {
        match command {
            Command::Mode(mode) => {
                self.config.display.mode = mode;
                if let Some(path) = &self.config_path
                    && let Err(e) = config::save(path, &self.config)
                {
                    self.tray
                        .warn("Pulsar could not save settings", &e.to_string());
                }
                self.refresh();
            }
            Command::Settings => self.open_settings(Page::General),
            Command::About => self.open_settings(Page::About),
            Command::TaskManager => menu::open_task_manager(),
            Command::Exit => unsafe {
                let _ = DestroyWindow(self.host);
            },
        }
    }
}

fn context_menu(x: i32, y: i32) {
    let Some((host, mode)) = with_app(|a| {
        a.close_popup();
        (a.host, a.config.display.mode)
    }) else {
        return;
    };
    if let Some(command) = menu::show(host, x, y, mode) {
        with_app(|a| a.apply(command));
    }
}

fn tray_opens_menu(event: u32) -> bool {
    event == WM_CONTEXTMENU
}

/// Left click or keyboard selection on a version-4 tray icon.
fn tray_opens_settings(event: u32) -> bool {
    event == NIN_SELECT || event == NIN_KEYSELECT
}

fn key(hwnd: HWND) -> isize {
    hwnd.0 as isize
}

fn low_word(v: usize) -> u32 {
    (v & 0xFFFF) as u32
}

fn signed_words(v: usize) -> (i32, i32) {
    (
        (v & 0xFFFF) as i16 as i32,
        ((v >> 16) & 0xFFFF) as i16 as i32,
    )
}

unsafe extern "system" fn host_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if msg == TASKBAR_CREATED.get() && msg != 0 {
        if with_app(App::on_taskbar_created).is_none() {
            defer(|d| d.taskbar_created = true);
        }
        return LRESULT(0);
    }
    if msg == CONFIG_CHANGED.get() && msg != 0 {
        if with_app(App::reload_config).is_none() {
            defer(|d| d.config_changed = true);
        }
        return LRESULT(0);
    }
    match msg {
        WM_APP_SNAPSHOT => {
            with_app(App::on_snapshot);
        }
        WM_APP_TASKBAR => {
            hooks::handled(msg);
            with_app(App::refresh);
        }
        WM_APP_FOREGROUND => {
            hooks::handled(msg);
            with_app(App::watch);
        }
        WM_APP_TRAY => {
            let event = low_word(lp.0 as usize);
            if tray_opens_menu(event) {
                let (x, y) = signed_words(wp.0);
                context_menu(x, y);
            } else if tray_opens_settings(event) {
                with_app(|a| a.apply(Command::Settings));
            }
        }
        WM_TIMER => {
            with_app(|a| a.on_timer(wp.0));
        }
        WM_SETTINGCHANGE => {
            if with_app(App::on_settings_changed).is_none() {
                defer(|d| d.settings_changed = true);
            }
        }
        WM_DISPLAYCHANGE => {
            with_app(App::refresh);
        }
        WM_DESTROY => unsafe { PostQuitMessage(0) },
        _ => return unsafe { DefWindowProcW(hwnd, msg, wp, lp) },
    }
    LRESULT(0)
}

unsafe extern "system" fn overlay_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        WM_MOUSEMOVE => {
            let (x, y) = signed_words(lp.0 as usize);
            with_app(|a| a.on_mouse_move(hwnd, x, y));
            LRESULT(0)
        }
        WM_MOUSEHOVER => {
            let (x, y) = signed_words(lp.0 as usize);
            with_app(|a| a.on_mouse_hover(hwnd, x, y));
            LRESULT(0)
        }
        WM_MOUSELEAVE => {
            with_app(|a| a.on_mouse_leave(hwnd));
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            with_app(|a| a.on_click(hwnd));
            menu::open_task_manager();
            LRESULT(0)
        }
        WM_RBUTTONUP => {
            let mut pt = POINT::default();
            unsafe {
                let _ = GetCursorPos(&mut pt);
            }
            context_menu(pt.x, pt.y);
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wp, lp) },
    }
}

pub fn run() -> Result<()> {
    let instance: HINSTANCE = unsafe { GetModuleHandleW(None)? }.into();
    let host_class = HSTRING::from(ipc::HOST_CLASS);
    unsafe {
        RegisterClassW(&WNDCLASSW {
            lpfnWndProc: Some(host_proc),
            hInstance: instance,
            lpszClassName: PCWSTR(host_class.as_ptr()),
            ..Default::default()
        });
        TASKBAR_CREATED.set(RegisterWindowMessageW(w!("TaskbarCreated")));
        CONFIG_CHANGED.set(RegisterWindowMessageW(&HSTRING::from(ipc::CONFIG_CHANGED)));
    }
    overlay::register_class(instance, Some(overlay_proc));

    // A hidden top-level window: message-only windows miss the
    // TaskbarCreated and WM_SETTINGCHANGE broadcasts.
    let host = unsafe {
        CreateWindowExW(
            WS_EX_TOOLWINDOW,
            &host_class,
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
    let app = App::new(instance, host)?;
    APP.with(|cell| *cell.borrow_mut() = Some(app));
    with_app(App::start);

    let mut msg = MSG::default();
    while unsafe { GetMessageW(&mut msg, None, 0, 0) }.as_bool() {
        unsafe {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    let app = APP.with(|cell| cell.borrow_mut().take());
    drop(app);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn broadcasts_missed_while_busy_are_deferred() {
        // No App is installed on the test thread, as when it is borrowed.
        TASKBAR_CREATED.set(0xC0DE);
        CONFIG_CHANGED.set(0xC0DF);
        unsafe {
            host_proc(HWND::default(), 0xC0DE, WPARAM(0), LPARAM(0));
            host_proc(HWND::default(), WM_SETTINGCHANGE, WPARAM(0), LPARAM(0));
            host_proc(HWND::default(), 0xC0DF, WPARAM(0), LPARAM(0));
        }
        assert_eq!(
            take_deferred(),
            Deferred {
                taskbar_created: true,
                settings_changed: true,
                config_changed: true
            }
        );
        assert_eq!(take_deferred(), Deferred::default());
    }

    #[test]
    fn tray_left_click_opens_settings() {
        assert!(tray_opens_settings(NIN_SELECT));
        assert!(tray_opens_settings(NIN_KEYSELECT));
        assert!(!tray_opens_settings(WM_CONTEXTMENU));
    }

    #[test]
    fn tray_right_click_opens_the_menu_once() {
        // Version-4 icons send WM_RBUTTONUP followed by WM_CONTEXTMENU.
        assert!(tray_opens_menu(WM_CONTEXTMENU));
        assert!(!tray_opens_menu(WM_RBUTTONUP));
    }

    #[test]
    fn tray_callback_words_are_sign_extended() {
        assert_eq!(signed_words(0x0010_0020), (0x20, 0x10));
        assert_eq!(
            signed_words(0xFFFF_FFF6),
            (-10, -1),
            "coordinates on a monitor left of primary"
        );
        assert_eq!(low_word(0x0001_007B), WM_CONTEXTMENU);
    }
}
