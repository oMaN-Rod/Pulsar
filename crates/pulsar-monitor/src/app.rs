use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use pulsar_core::alerts::Alerts;
use pulsar_core::config::{self, Config, Position};
use pulsar_core::history::HistoryStore;
use pulsar_core::hotkey::Hotkey;
use pulsar_core::ipc;
use pulsar_core::metric::{ItemKind, Snapshot};
use pulsar_core::recording::Recorder;
use pulsar_core::update::{self as release, Outcome, UpdateState};
use pulsar_core::{crash, games, paths, profile, sampler, tr};
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::System::Com::CoTaskMemFree;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, MOD_WIN, RegisterHotKey, ReleaseCapture,
    SetCapture, TME_HOVER, TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent, UnregisterHotKey,
};
use windows::Win32::UI::Shell::{
    ABM_NEW, ABM_REMOVE, ABN_FULLSCREENAPP, APPBARDATA, FOLDERID_Documents, KF_FLAG_DEFAULT,
    NIN_BALLOONTIMEOUT, NIN_BALLOONUSERCLICK, NIN_SELECT, SHAppBarMessage, SHGetKnownFolderPath,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetCursorPos,
    GetForegroundWindow, GetMessageW, GetSystemMetrics, KillTimer, MA_NOACTIVATE, MSG,
    PostQuitMessage, RegisterClassW, RegisterWindowMessageW, SM_CXDRAG, SetTimer, TranslateMessage,
    WM_CAPTURECHANGED, WM_CONTEXTMENU, WM_DESTROY, WM_DISPLAYCHANGE, WM_HOTKEY, WM_LBUTTONDOWN,
    WM_LBUTTONUP, WM_MOUSEACTIVATE, WM_MOUSEMOVE, WM_RBUTTONUP, WM_SETCURSOR, WM_SETTINGCHANGE,
    WM_TIMER, WNDCLASSW, WS_EX_TOOLWINDOW, WS_POPUP,
};
use windows::core::{HSTRING, PCWSTR, Result, w};

use crate::alerts;
use crate::details::Details;
use crate::drag::Drag;
use crate::fade::{self, Debounce};
use crate::game::{self, Game};
use crate::hover::{HoverArming, popup_orphaned};
use crate::launcher::{self, Page};
use crate::menu::{self, Command};
use crate::messages::{
    WM_APP_APPBAR, WM_APP_FOREGROUND, WM_APP_SNAPSHOT, WM_APP_TASKBAR, WM_APP_TRAY, WM_APP_UPDATE,
};
use crate::overlay::{self, Overlay};
use crate::popup::content::{Options as PopupOptions, needs_processes};
use crate::popup::layout::item_screen_rect;
use crate::popup::{self, Anchor, Popup, Style as PopupStyle};
use crate::render::{Frame, Renderer, graph_styles};
use crate::sampler_thread::SamplerThread;
use crate::taskbar::hooks::{self, Hooks};
use crate::taskbar::placement::{
    HideReason, Placement, PlacementInput, Rect32, place, place_floating, tray_is_laid_out,
};
use crate::taskbar::{self, Taskbar};
use crate::text::Text;
use crate::theme::Palette;
use crate::tray::Tray;
use crate::update;
use crate::widget::{self, Widget};

const TIMER_WATCH: usize = 1;
const TIMER_VALIDATE: usize = 2;
const TIMER_RETRY: usize = 3;
const TIMER_FADE: usize = 5;
const TIMER_UPDATE: usize = 4;
/// The first check waits for startup to settle; after that an hourly tick asks
/// whether a day has passed, since a PC may stay on for days.
const UPDATE_FIRST_MS: u32 = 60_000;
const UPDATE_EVERY_MS: u32 = 60 * 60 * 1000;
const WATCH_MS: u32 = 250;
const FADE_MS: u32 = 15;
const FADE_STEP: u8 = 40;
/// A fullscreen window must stay in front this long before overlays hide,
/// so shell transitions do not flicker them.
const FULLSCREEN_DELAY: Duration = Duration::from_millis(300);
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
    fullscreen: Debounce<Vec<Rect32>>,
    retry_step: usize,
    popup: Popup,
    /// The overlay the open popup belongs to.
    popup_owner: Option<HWND>,
    hover: HoverArming,
    /// A floating overlay being moved with the mouse.
    drag: Option<(HWND, Drag)>,
    /// The release check running in the helper process, if any.
    update_check: Option<Receiver<Outcome>>,
    details: Option<Details>,
    /// Physical drive names, read when a drive temperature popup first opens.
    disk_names: Option<Vec<(u8, String)>>,
    widget: Widget,
    /// The widget being moved with the mouse.
    widget_drag: Option<Drag>,
    /// Sampled for the game in progress, on top of what the settings ask for.
    session_items: Vec<ItemKind>,
    /// The games Windows knows, and when they were read.
    known_games: HashSet<String>,
    games_read: Option<Instant>,
    last_foreground: HWND,
    /// The app last in front other than Pulsar and the shell, for marking
    /// as a game.
    last_app: Option<String>,
    game: Option<Game>,
    menu_profiles: Vec<String>,
    on_disk: Option<Config>,
    reload_failed: bool,
    alerts: Alerts,
    /// Hidden with the shortcut until it is pressed again.
    hidden: bool,
    /// Shortcuts another app had taken, last time they were registered.
    hotkeys_taken: Vec<String>,
    recorder: Option<Recorder>,
}

/// What drawing an overlay needs besides the overlay.
struct Paint<'a> {
    config: &'a Config,
    renderer: &'a mut Renderer,
    text: &'a Text,
    palette: &'a Palette,
    snapshot: &'a Snapshot,
    history: &'a HistoryStore,
}

impl Paint<'_> {
    fn overlay(&mut self, o: &mut Overlay, x: i32, y: i32) -> Result<()> {
        let dpi = o.dpi();
        let hwnd = o.hwnd;
        let alpha = o.alpha;
        let layout = o.layout.clone();
        let surface = o.surface()?;
        let styles = graph_styles(self.config);
        let d = &self.config.display;
        let frame = Frame {
            layout: &layout,
            mode: d.mode,
            palette: self.palette,
            snapshot: self.snapshot,
            history: self.history,
            dpi,
            text: pulsar_core::layout::TileText {
                labels: d.labels,
                icons: d.icons,
                upload_on_top: d.upload_on_top,
            },
            graph_inline: d.graph_inline,
            styles: &styles,
        };
        self.renderer.draw(surface, &frame, self.text)?;
        self.renderer.present_alpha(hwnd, surface, x, y, alpha)?;
        o.mark_shown(x, y);
        Ok(())
    }
}

/// The Windows game list is read again when older than this.
const GAMES_REREAD: Duration = Duration::from_secs(60);
const SESSIONS_FILE: &str = "sessions.csv";
/// The shortcuts' IDs, in the order of `HotkeysConfig::all`.
const HOTKEY_BASE: i32 = 1;

impl App {
    fn new(instance: HINSTANCE, host: HWND) -> Result<Self> {
        let config_path = config::default_path();
        let loaded = config_path.as_deref().map(config::load);
        let (config, warning) = match loaded {
            Some(l) => (l.config, l.warning),
            None => (Config::default(), None),
        };
        crate::language::init(&config);
        pulsar_core::format::set_units(config.units);
        let tray = Tray::new(host, config.general.tray_icon);
        if let Some(warning) = warning {
            tray.warn(tr!("Pulsar settings reset"), &warning.to_string());
        }
        if let Some(report) = paths::logs_dir().and_then(|dir| crash::take_pending(&dir)) {
            tray.offer(
                tr!("Pulsar closed unexpectedly"),
                tr!("Select to open the crash report. Attaching it to a GitHub issue helps get it fixed."),
                report.display().to_string(),
            );
        }
        let translucent = translucent_popups(&config);
        let on_disk = Some(config.clone());
        Ok(Self {
            instance,
            host,
            palette: Palette::current(&config),
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
            fullscreen: Debounce::new(Vec::new()),
            retry_step: 0,
            popup: Popup::create(instance, translucent)?,
            popup_owner: None,
            hover: HoverArming::default(),
            drag: None,
            update_check: None,
            details: None,
            disk_names: None,
            widget: Widget::create(instance)?,
            widget_drag: None,
            session_items: Vec::new(),
            known_games: HashSet::new(),
            games_read: None,
            last_foreground: HWND::default(),
            last_app: None,
            game: None,
            menu_profiles: Vec::new(),
            on_disk,
            reload_failed: false,
            alerts: Alerts::default(),
            hidden: false,
            hotkeys_taken: Vec::new(),
            recorder: None,
        })
    }

    fn start(&mut self) {
        unsafe {
            SetTimer(Some(self.host), TIMER_WATCH, WATCH_MS, None);
            SetTimer(Some(self.host), TIMER_VALIDATE, VALIDATE_MS, None);
            SetTimer(Some(self.host), TIMER_UPDATE, UPDATE_FIRST_MS, None);
        }
        self.fullscreen = Debounce::new(taskbar::fullscreen_monitors());
        self.register_appbar(ABM_NEW);
        self.register_hotkeys();
        self.refresh();
    }

    fn register_hotkeys(&mut self) {
        let mut taken = Vec::new();
        let keys = self.config.hotkeys.all();
        for i in 0..keys.len() {
            unsafe {
                let _ = UnregisterHotKey(Some(self.host), HOTKEY_BASE + i as i32);
            }
        }
        for (i, key) in keys.into_iter().enumerate() {
            let id = HOTKEY_BASE + i as i32;
            let Some(key) = key.as_deref().and_then(Hotkey::parse) else {
                continue;
            };
            let mut modifiers = MOD_NOREPEAT;
            for (on, flag) in [
                (key.ctrl, MOD_CONTROL),
                (key.alt, MOD_ALT),
                (key.shift, MOD_SHIFT),
                (key.win, MOD_WIN),
            ] {
                if on {
                    modifiers |= flag;
                }
            }
            let registered =
                unsafe { RegisterHotKey(Some(self.host), id, modifiers, key.virtual_key()) };
            if registered.is_err() {
                taken.push(key.to_string());
            }
        }
        if !taken.is_empty() && taken != self.hotkeys_taken {
            self.tray.warn(
                tr!("Shortcut already in use"),
                &tr!(
                    "Another app uses {}. Choose another in Settings.",
                    taken.join(", ")
                ),
            );
        }
        self.hotkeys_taken = taken;
    }

    fn on_hotkey(&mut self, id: usize) {
        match id as i32 - HOTKEY_BASE {
            0 => {
                self.hidden = !self.hidden;
                self.close_popup();
                self.refresh();
            }
            1 => self.change_config(|c| menu::apply_toggle(c, menu::Toggle::Widget)),
            2 => self.open_details(None),
            3 => self.toggle_recording(),
            4 => self.next_profile(),
            _ => {}
        }
    }

    /// Rediscovers taskbars, reconciles overlays with them and repositions.
    fn floating(&self) -> bool {
        self.config.display.position == Position::Floating
    }

    fn refresh(&mut self) {
        let floating = self.floating();
        let mut found = taskbar::discover(self.config.display.show_on_all_taskbars && !floating);
        if floating {
            found.truncate(1);
        }
        let before = self.overlays.len();
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
        if self.overlays.len() != before {
            log::info!("attached to {} taskbar(s)", self.overlays.len());
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
        self.show_widget();
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
        // The mouse owns a dragged overlay's position until it is released.
        let hwnd = self.overlays[i].hwnd;
        if self
            .drag
            .as_ref()
            .is_some_and(|(h, d)| *h == hwnd && d.moved())
        {
            return;
        }
        let o = &mut self.overlays[i];
        let floating = self.config.display.position == Position::Floating;
        let saved = self.config.display.float_position.map(|[x, y]| (x, y));
        let tb_rect = o.taskbar.rect;
        let probe = saved.unwrap_or(((tb_rect.left + tb_rect.right) / 2, tb_rect.top));
        o.float_dpi = floating.then(|| taskbar::dpi_near(probe.0, probe.1));
        o.update_layout(&self.config, &self.text);
        let (w, h) = o.size();
        let dpi = o.dpi();
        let tb: &Taskbar = &o.taskbar;
        let (placement, monitor) = if floating {
            let (monitor, work) = taskbar::monitor_near(probe.0, probe.1);
            let placement = if w <= 0 {
                Placement::Hidden(HideReason::Empty)
            } else {
                let (x, y) = place_floating(saved, (w, h), work, Some(tb.rect), dpi);
                Placement::At { x, y }
            };
            (placement, monitor)
        } else {
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
            (placement, tb.monitor)
        };
        let covered = self.hidden
            || (self.config.display.hide_in_fullscreen
                && self.fullscreen.current().contains(&monitor));
        match placement {
            Placement::At { x, y } if !covered => {
                let o = &mut self.overlays[i];
                if o.fade_target == 0 {
                    o.fade_target = 255;
                    if o.shown_at.is_none() {
                        o.alpha = 0;
                    }
                    self.start_fade();
                }
                if self.draw(i, x, y).is_err() {
                    self.overlays[i].hide();
                }
            }
            Placement::At { .. } => {
                let o = &mut self.overlays[i];
                if o.fade_target != 0 {
                    o.fade_target = 0;
                    if o.shown_at.is_some() {
                        self.start_fade();
                    }
                }
            }
            Placement::Hidden(_) => {
                let o = &mut self.overlays[i];
                o.hide();
                o.alpha = 255;
                o.fade_target = 255;
            }
        }
    }

    fn start_fade(&self) {
        unsafe { SetTimer(Some(self.host), TIMER_FADE, FADE_MS, None) };
    }

    /// One step of every running fade; an overlay that fades out is hidden.
    fn fade_tick(&mut self) {
        let mut running = false;
        for o in &mut self.overlays {
            if o.alpha == o.fade_target {
                continue;
            }
            o.alpha = fade::step(o.alpha, o.fade_target, FADE_STEP);
            if let Some((x, y)) = o.shown_at {
                if o.alpha == 0 {
                    o.hide();
                } else {
                    let hwnd = o.hwnd;
                    let alpha = o.alpha;
                    if let Ok(surface) = o.surface() {
                        let _ = self.renderer.present_alpha(hwnd, surface, x, y, alpha);
                    }
                }
            }
            running |= o.alpha != o.fade_target;
        }
        if !running {
            unsafe {
                let _ = KillTimer(Some(self.host), TIMER_FADE);
            }
        }
    }

    /// A callback-only appbar: Windows then tells us when a fullscreen app
    /// opens or closes. It never claims screen space, so the work area is untouched.
    fn register_appbar(&self, message: u32) {
        let mut data = APPBARDATA {
            cbSize: size_of::<APPBARDATA>() as u32,
            hWnd: self.host,
            uCallbackMessage: WM_APP_APPBAR,
            ..Default::default()
        };
        unsafe { SHAppBarMessage(message, &mut data) };
    }

    fn draw(&mut self, i: usize, x: i32, y: i32) -> Result<()> {
        let mut paint = Paint {
            config: &self.config,
            renderer: &mut self.renderer,
            text: &self.text,
            palette: &self.palette,
            snapshot: &self.latest,
            history: &self.history,
        };
        paint.overlay(&mut self.overlays[i], x, y)
    }

    fn redraw(&mut self) {
        for i in 0..self.overlays.len() {
            if let Some((x, y)) = self.overlays[i].shown_at
                && self.draw(i, x, y).is_err()
            {
                self.overlays[i].hide();
            }
        }
        self.show_widget();
        let config = game::strip_config(&self.config);
        let mut paint = Paint {
            config: &config,
            renderer: &mut self.renderer,
            text: &self.text,
            palette: &self.palette,
            snapshot: &self.latest,
            history: &self.history,
        };
        if let Some(strip) = self.game.as_mut().and_then(|g| g.strip.as_mut())
            && let Some((x, y)) = strip.shown_at
            && paint.overlay(strip, x, y).is_err()
        {
            strip.hide();
        }
    }

    /// Notices games coming to the front and closing, and keeps the strip
    /// on the game's monitor while it is in front.
    fn watch_games(&mut self) {
        if !self.config.game.watching() {
            self.end_game(false);
            return;
        }
        let foreground = unsafe { GetForegroundWindow() };
        if foreground != self.last_foreground {
            self.last_foreground = foreground;
            self.foreground_changed(foreground);
        }
        if self.game.as_ref().is_some_and(Game::exited) {
            self.end_game(true);
            self.foreground_changed(foreground);
        }
        self.place_strip(foreground);
    }

    fn foreground_changed(&mut self, foreground: HWND) {
        let pid = taskbar::process_id(foreground);
        if pid == 0 || pid == std::process::id() {
            return;
        }
        let Some(path) = taskbar::image_path(pid) else {
            return;
        };
        if taskbar::is_shell_host(&path) || games::exe_name(&path) == "explorer.exe" {
            return;
        }
        self.last_app = Some(path.clone());
        if self.game.is_none() && self.is_game(&path) {
            self.start_game(pid, &path);
        }
    }

    fn is_game(&mut self, path: &str) -> bool {
        if self.games_read.is_none_or(|t| t.elapsed() > GAMES_REREAD) {
            self.known_games = games::windows_games();
            self.games_read = Some(Instant::now());
        }
        games::is_game(path, &self.known_games, &self.config.game)
    }

    fn start_game(&mut self, pid: u32, path: &str) {
        let Some(g) = Game::start(pid, games::display_name(path)) else {
            return;
        };
        log::info!("game started: {}", g.session.name);
        self.game = Some(g);
        self.set_session_items(game::SESSION_ITEMS.to_vec());
    }

    /// `closed`: the game exited, rather than being unmarked or the
    /// feature turned off, so it gets a summary.
    fn end_game(&mut self, closed: bool) {
        let Some(g) = self.game.take() else {
            return;
        };
        self.set_session_items(Vec::new());
        log::info!("game ended: {}", g.session.name);
        if !closed || !self.config.game.summary || g.played() < games::MIN_SESSION {
            return;
        }
        let summary = g.summary();
        let Some(file) = paths::logs_dir().map(|dir| dir.join(SESSIONS_FILE)) else {
            return;
        };
        if let Err(e) = games::log_session(&file, &summary) {
            log::warn!("could not log the session: {e}");
        }
        self.tray.offer(
            &tr!("{} closed", summary.name),
            &game::summary_text(&summary),
            file.display().to_string(),
        );
    }

    fn place_strip(&mut self, foreground: HWND) {
        let Some(pid) = self.game.as_ref().map(|g| g.pid) else {
            return;
        };
        if self.hidden || !self.config.game.strip || taskbar::process_id(foreground) != pid {
            if let Some(strip) = self.game.as_mut().and_then(|g| g.strip.as_mut()) {
                strip.hide();
            }
            return;
        }
        let monitor = taskbar::monitor_of(foreground);
        let stale = self
            .game
            .as_ref()
            .and_then(|g| g.strip.as_ref())
            .is_none_or(|s| s.taskbar.monitor != monitor);
        if stale {
            // The taskbar on the game's monitor sets the strip's size and scale.
            let tb = taskbar::discover(true)
                .into_iter()
                .find(|t| t.monitor == monitor)
                .or_else(|| {
                    self.overlays.first().map(|o| Taskbar {
                        monitor,
                        ..o.taskbar.clone()
                    })
                });
            let strip = tb.and_then(|tb| Overlay::create(self.instance, tb).ok());
            if let Some(s) = &strip {
                game::click_through(s);
            }
            if let Some(g) = &mut self.game {
                g.strip = strip;
            }
        }
        let config = game::strip_config(&self.config);
        let mut paint = Paint {
            config: &config,
            renderer: &mut self.renderer,
            text: &self.text,
            palette: &self.palette,
            snapshot: &self.latest,
            history: &self.history,
        };
        let Some(strip) = self.game.as_mut().and_then(|g| g.strip.as_mut()) else {
            return;
        };
        strip.float_dpi = Some(taskbar::dpi_near(monitor.left, monitor.top));
        strip.update_layout(&config, &self.text);
        let at = game::strip_position(monitor, strip.dpi());
        if strip.shown_at == Some(at) {
            strip.raise();
        } else if paint.overlay(strip, at.0, at.1).is_err() {
            strip.hide();
        }
    }

    /// The settings plus what the game in progress needs sampled.
    fn sampling_config(&self, config: &Config) -> Config {
        let mut c = config.clone();
        c.session_items = self.session_items.clone();
        if let Some(r) = &self.recorder {
            c.session_items.extend(r.items.iter().copied());
        }
        c
    }

    fn stop_recording(&mut self) -> Option<Recorder> {
        let before = self.sampling_config(&self.config);
        let recorder = self.recorder.take();
        let after = self.sampling_config(&self.config);
        if config::sampling_changed(&before, &after) {
            self.respawn_sampler(&after);
        }
        recorder
    }

    fn set_session_items(&mut self, items: Vec<ItemKind>) {
        let before = self.sampling_config(&self.config);
        self.session_items = items;
        let after = self.sampling_config(&self.config);
        if config::sampling_changed(&before, &after) {
            self.respawn_sampler(&after);
        }
    }

    fn respawn_sampler(&mut self, config: &Config) {
        self.close_popup();
        self.sampler = SamplerThread::spawn(config, Some(self.host));
        let open = self.details.is_some();
        self.sampler.extras.store(open, Ordering::Relaxed);
        self.sampler.processes.store(open, Ordering::Relaxed);
    }

    fn apply_profile(&mut self, id: &str) {
        let Some(dir) = profile::profiles_dir() else {
            return;
        };
        match profile::load(&dir, id) {
            Some(p) => self.change_config(|c| *c = p.applied_to(c)),
            None => self.tray.warn(
                tr!("Pulsar could not load the profile"),
                &dir.display().to_string(),
            ),
        }
    }

    fn next_profile(&mut self) {
        let Some(dir) = profile::profiles_dir() else {
            return;
        };
        let entries = profile::list(&dir);
        let current = entries
            .iter()
            .position(|e| profile::load(&dir, &e.id).is_some_and(|p| p.matches(&self.config)));
        let Some(index) = profile::next(current, entries.len()) else {
            self.tray.inform(
                tr!("No profiles saved"),
                tr!("Save one in Settings → Profiles."),
            );
            return;
        };
        self.apply_profile(&entries[index].id);
        self.tray.inform(tr!("Profile"), &entries[index].name);
    }

    fn toggle_recording(&mut self) {
        if let Some(r) = self.stop_recording() {
            self.tray.offer(
                tr!("Recording saved"),
                &r.path.display().to_string(),
                r.path.display().to_string(),
            );
            return;
        }
        let Some(dir) = documents_dir().map(|d| d.join("Pulsar")) else {
            return;
        };
        let items = self.sampling_config(&self.config).sampled_items();
        match Recorder::start(&dir, &items) {
            Ok(r) => {
                self.tray.offer(
                    tr!("Recording readings"),
                    &tr!("Every sample goes to {} until you stop.", r.path.display()),
                    dir.display().to_string(),
                );
                self.recorder = Some(r);
            }
            Err(e) => self
                .tray
                .warn(tr!("Pulsar could not start recording"), &e.to_string()),
        }
    }

    fn toggle_game(&mut self) {
        let Some(path) = self.last_app.clone() else {
            return;
        };
        let known = self.known_games.clone();
        self.change_config(|c| games::toggle(&path, &known, &mut c.game));
        let name = games::display_name(&path);
        if !self.is_game(&path) && self.game.as_ref().is_some_and(|g| g.session.name == name) {
            self.end_game(false);
        }
        // Looks at the app in front again, so a newly marked game starts.
        self.last_foreground = HWND::default();
    }

    /// Draws the widget, or hides it when it is off, empty, or covered by a
    /// fullscreen app on its monitor.
    fn show_widget(&mut self) {
        if self.hidden {
            self.widget.hide();
            return;
        }
        let w = &self.config.widget;
        let items = self.config.widget_items();
        if items.is_empty() {
            self.widget.hide();
            return;
        }
        let saved = w.position.map(|[x, y]| (x, y));
        let probe = self.widget.shown_at.or(saved).unwrap_or((0, 0));
        let (monitor, work) = taskbar::monitor_near(probe.0, probe.1);
        if w.hide_in_fullscreen && self.fullscreen.current().contains(&monitor) {
            self.widget.hide();
            return;
        }
        let dragging = self.widget_drag.as_ref().is_some_and(Drag::moved);
        let current = self.widget.shown_at;
        let dpi = self.widget.dpi();
        let scale = dpi as f32 / 96.0;
        let look = widget::Look {
            column_w: f32::from(w.width) * scale,
            max_h: work.height() as f32 - 2.0 * widget::EDGE_GAP * scale,
            font_px: self.config.display.font_size_pt * dpi as f32 / 72.0,
            opacity: f32::from(w.opacity) / 100.0,
            scale,
        };
        let contents: Vec<_> = items
            .into_iter()
            .map(|kind| {
                let mut options = PopupOptions::new(&self.config, kind);
                options.detail = self.config.widget_detail(kind);
                options.processes = 0;
                options.graph = true;
                options.disk_names = self.disk_names.as_deref().unwrap_or_default();
                let content =
                    popup::content::build_with(kind, &self.latest, &self.history, &options);
                (content, self.config.widget_style(kind))
            })
            .collect();
        let drawn = self.widget.draw(
            &mut self.renderer,
            &self.text,
            &contents,
            &self.latest,
            &self.palette,
            &self.history,
            &look,
            |width, height| match current {
                Some(at) if dragging => at,
                _ => widget::place(saved, (width, height), work, scale),
            },
        );
        if let Err(e) = drawn {
            log::warn!("could not draw the widget: {e}");
            self.widget.hide();
            return;
        }
        let w = &self.config.widget;
        self.widget
            .set_click_through(w.lock_position && w.click_through);
        self.widget
            .arrange(w.layer, unsafe { GetForegroundWindow() });
    }

    fn on_widget_button_down(&mut self) {
        if self.config.widget.lock_position {
            return;
        }
        if let Some(pos) = self.widget.shown_at {
            self.widget_drag = Some(Drag::new(cursor(), pos));
            unsafe { SetCapture(self.widget.hwnd) };
        }
    }

    fn on_widget_drag_move(&mut self) {
        let Some(drag) = self.widget_drag.as_mut() else {
            return;
        };
        let threshold = unsafe { GetSystemMetrics(SM_CXDRAG) };
        let at = cursor();
        if let Some(to) = drag.to(at, threshold) {
            let (_, work) = taskbar::monitor_near(at.0, at.1);
            let scale = self.widget.dpi() as f32 / 96.0;
            let (x, y) = widget::snap(to, self.widget.size(), work, scale);
            let _ = self.widget.move_to(&self.renderer, x, y);
        }
    }

    fn on_widget_button_up(&mut self) {
        let Some(drag) = self.widget_drag.take() else {
            return;
        };
        unsafe {
            let _ = ReleaseCapture();
        }
        if drag.moved()
            && let Some((x, y)) = self.widget.shown_at
        {
            // Redraws it too, at the new monitor's DPI.
            self.change_config(|c| c.widget.position = Some([x, y]));
        }
    }

    fn on_snapshot(&mut self) {
        let mut received = false;
        while let Ok(snapshot) = self.sampler.snapshots.try_recv() {
            self.history.record(&snapshot);
            if let Some(g) = &mut self.game {
                let in_front = taskbar::process_id(unsafe { GetForegroundWindow() }) == g.pid;
                let seconds = f64::from(self.config.general.sample_interval_ms) / 1000.0;
                g.session.record(&snapshot, in_front, seconds);
            }
            if let Some(r) = &mut self.recorder
                && let Err(e) = r.record(&snapshot)
            {
                self.stop_recording();
                self.tray
                    .warn(tr!("Pulsar stopped recording"), &e.to_string());
            }
            let due = self
                .alerts
                .check(&self.config.alerts, &snapshot, Instant::now());
            for alert in due {
                let (title, message) = alerts::text(alert, &snapshot, &self.config.alerts);
                self.tray.warn(&title, &message);
            }
            for (id, change) in sampler::error_changes(&self.latest.errors, &snapshot.errors) {
                match change {
                    Some(reason) => log::warn!("{id:?} unavailable: {reason}"),
                    None => log::info!("{id:?} recovered"),
                }
            }
            if let Some(details) = &self.details
                && !details.send(&snapshot)
            {
                self.details_closed();
            }
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
        if overlay == self.widget.hwnd {
            return self.widget.hit(x, y);
        }
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

    fn on_button_down(&mut self, overlay: HWND) {
        if !self.floating() || self.config.display.lock_position {
            return;
        }
        let Some(pos) = self
            .overlays
            .iter()
            .find(|o| o.hwnd == overlay)
            .and_then(|o| o.shown_at)
        else {
            return;
        };
        self.drag = Some((overlay, Drag::new(cursor(), pos)));
        unsafe { SetCapture(overlay) };
    }

    /// Moves a dragged overlay; returns whether a drag is in progress.
    fn on_drag_move(&mut self, overlay: HWND) -> bool {
        let Some((hwnd, drag)) = self.drag.as_mut().filter(|(h, _)| *h == overlay) else {
            return false;
        };
        let hwnd = *hwnd;
        let threshold = unsafe { GetSystemMetrics(SM_CXDRAG) };
        if let Some((x, y)) = drag.to(cursor(), threshold) {
            self.close_popup();
            if let Some(i) = self.overlays.iter().position(|o| o.hwnd == hwnd)
                && self.draw(i, x, y).is_err()
            {
                self.overlays[i].hide();
            }
        }
        true
    }

    /// Ends a drag and saves where it left the overlay; returns whether the
    /// press was a drag rather than a click.
    fn on_button_up(&mut self, overlay: HWND) -> bool {
        let Some((hwnd, drag)) = self.drag.take() else {
            return false;
        };
        unsafe {
            let _ = ReleaseCapture();
        }
        if hwnd != overlay || !drag.moved() {
            return false;
        }
        if let Some((x, y)) = self
            .overlays
            .iter()
            .find(|o| o.hwnd == hwnd)
            .and_then(|o| o.shown_at)
        {
            self.change_config(|c| c.display.float_position = Some([x, y]));
        }
        true
    }

    /// Returns whether Task Manager should open: a plain click opens it,
    /// a Ctrl+click opens the details window instead.
    fn on_click(&mut self, overlay: HWND, x: i32, y: i32, ctrl: bool) -> bool {
        let item = self.hit(overlay, x, y);
        self.close_popup();
        self.hover.disarm(key(overlay));
        if ctrl {
            self.open_details(item);
        }
        !ctrl
    }

    /// Opens the details window on the page for `item`, or brings it forward.
    fn open_details(&mut self, item: Option<ItemKind>) {
        if let Some(details) = &mut self.details {
            if details.alive() {
                details.show(item);
                return;
            }
            self.details_closed();
        }
        match Details::open(&self.history, item) {
            Ok(details) => {
                self.details = Some(details);
                self.sampler.extras.store(true, Ordering::Relaxed);
                self.sampler.processes.store(true, Ordering::Relaxed);
            }
            Err(e) => self.tray.warn(
                tr!("Pulsar could not open the details window"),
                &e.to_string(),
            ),
        }
    }

    fn details_closed(&mut self) {
        self.details = None;
        self.sampler.extras.store(false, Ordering::Relaxed);
        let popup_lists = self
            .popup
            .open
            .is_some_and(|(kind, _)| self.popup_lists_processes(kind));
        self.sampler.processes.store(popup_lists, Ordering::Relaxed);
    }

    fn popup_lists_processes(&self, kind: ItemKind) -> bool {
        needs_processes(kind, &PopupOptions::new(&self.config, kind))
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
        let anchor = match (self.floating(), o.shown_at) {
            // Floating: the popup goes above or below the overlay itself.
            (true, Some((x, y))) => {
                let (w, h) = o.size();
                Anchor {
                    item,
                    taskbar: Rect32 {
                        left: x,
                        top: y,
                        right: x + w,
                        bottom: y + h,
                    },
                    monitor: taskbar::monitor_near(x, y).0,
                    dpi: o.dpi(),
                }
            }
            _ => Anchor {
                item,
                taskbar: o.taskbar.rect,
                monitor: o.taskbar.monitor,
                dpi: o.dpi(),
            },
        };
        self.sampler.processes.store(
            self.popup_lists_processes(kind) || self.details.is_some(),
            Ordering::Relaxed,
        );
        self.popup_owner = Some(overlay);
        self.show_popup(kind, anchor);
    }

    fn show_popup(&mut self, kind: ItemKind, anchor: Anchor) {
        if kind == ItemKind::DiskTemp && self.disk_names.is_none() {
            self.disk_names = Some(disk_names());
        }
        let mut options = PopupOptions::new(&self.config, kind);
        options.disk_names = self.disk_names.as_deref().unwrap_or_default();
        let content = popup::content::build_with(kind, &self.latest, &self.history, &options);
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
        self.sampler
            .processes
            .store(self.details.is_some(), Ordering::Relaxed);
    }

    /// Runs every 250 ms and on foreground changes: hides for fullscreen apps
    /// and lifts overlays that the taskbar has been raised above.
    fn watch(&mut self) {
        if self.reload_failed {
            self.reload_config();
        }
        let fullscreen = taskbar::fullscreen_monitors();
        let leaving = fullscreen.is_empty();
        if self
            .fullscreen
            .update(fullscreen, Instant::now(), FULLSCREEN_DELAY, leaving)
        {
            self.refresh();
            return;
        }
        for o in &self.overlays {
            if o.shown_at.is_some() && taskbar::is_behind(o.hwnd, o.taskbar.hwnd) {
                o.raise();
            }
        }
        self.widget
            .arrange(self.config.widget.layer, unsafe { GetForegroundWindow() });
        self.watch_games();
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
            TIMER_FADE => self.fade_tick(),
            TIMER_UPDATE => {
                unsafe { SetTimer(Some(self.host), TIMER_UPDATE, UPDATE_EVERY_MS, None) };
                self.check_for_update();
            }
            TIMER_RETRY => {
                unsafe {
                    let _ = KillTimer(Some(self.host), TIMER_RETRY);
                }
                self.refresh();
            }
            _ => {}
        }
    }

    fn check_for_update(&mut self) {
        // An outcome whose message arrived while the app was busy.
        if self.update_check.is_some() {
            self.on_update_checked();
        }
        if !self.config.general.update_check || self.update_check.is_some() {
            return;
        }
        if release::state_path().is_some_and(|path| UpdateState::load(&path).due(update::now())) {
            self.update_check = update::spawn(self.host);
        }
    }

    fn on_update_checked(&mut self) {
        let Some(outcome) = self.update_check.as_ref().and_then(|rx| rx.try_recv().ok()) else {
            return;
        };
        self.update_check = None;
        let Some(path) = release::state_path() else {
            return;
        };
        let mut state = UpdateState::load(&path);
        let announcement = release::apply(&mut state, &outcome, update::current(), update::now());
        match (&outcome, &announcement) {
            (_, Some(a)) => log::info!("Pulsar {} is available: {}", a.version, a.url),
            (Outcome::Failed(reason), _) => log::warn!("update check failed: {reason}"),
            (Outcome::NoRelease, _) => log::info!("update check: no release published yet"),
            (Outcome::Found(r), None) => {
                log::info!("update check: latest release is {}", r.tag_name)
            }
        }
        if let Err(e) = state.save(&path) {
            log::warn!("could not save the update check state: {e}");
        }
        if let Some(a) = announcement {
            self.tray.offer(
                &tr!("Pulsar {} is available", a.version),
                tr!("Select to open the release page."),
                a.url,
            );
        }
    }

    fn on_balloon_click(&mut self) {
        if let Some(target) = self.tray.target() {
            menu::shell_open(&target);
        }
        self.tray.balloon_closed();
    }

    fn on_taskbar_created(&mut self) {
        log::info!("taskbar re-created; attaching again");
        self.tray.restore();
        self.widget.rearrange();
        self.retry_step = 0;
        self.refresh();
        self.schedule_retry();
    }

    fn on_settings_changed(&mut self) {
        self.palette = Palette::current(&self.config);
        self.redraw();
    }

    fn open_settings(&self, page: Page) {
        if let Err(e) = launcher::open(page) {
            self.tray
                .warn(tr!("Pulsar could not open Settings"), &e.to_string());
        }
    }

    /// Applies a config saved by the settings process. Only a change to the
    /// sampled sources, interval or ping target restarts the sampler; layout,
    /// palette and placement are recomputed by `refresh`.
    fn reload_config(&mut self) {
        let Some(path) = &self.config_path else {
            return;
        };
        match config::read(path) {
            Ok(new) => {
                self.reload_failed = false;
                self.on_disk = Some(new.clone());
                self.apply_config(new);
                // A theme file may have changed though the settings did not.
                self.on_settings_changed();
            }
            Err(e) => {
                if !self.reload_failed {
                    log::warn!("kept the current settings; could not read them: {e}");
                }
                self.reload_failed = true;
            }
        }
    }

    /// Changes the config from the app itself (the menu), saving it so the
    /// settings window picks it up.
    fn change_config(&mut self, edit: impl FnOnce(&mut Config)) {
        let theirs = self
            .config_path
            .as_deref()
            .and_then(|p| config::read(p).ok());
        let mut new = match (&self.on_disk, &theirs) {
            (Some(known), Some(t)) if t != known => config::merge(known, &self.config, t),
            _ => self.config.clone(),
        };
        if theirs.is_some() {
            self.on_disk = theirs;
        }
        edit(&mut new);
        if let Some(path) = &self.config_path {
            match config::save(path, &new) {
                Ok(()) => self.on_disk = Some(new.clone()),
                Err(e) => self
                    .tray
                    .warn(tr!("Pulsar could not save settings"), &e.to_string()),
            }
        }
        self.apply_config(new);
    }

    fn apply_config(&mut self, new: Config) {
        if new == self.config {
            return;
        }
        log::info!("settings changed");
        let (before, after) = (
            self.sampling_config(&self.config),
            self.sampling_config(&new),
        );
        if config::sampling_changed(&before, &after) {
            self.respawn_sampler(&after);
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
            if let Some(strip) = self.game.as_mut().and_then(|g| g.strip.as_mut()) {
                strip.invalidate_layout();
            }
        }
        if new.units != self.config.units {
            pulsar_core::format::set_units(new.units);
        }
        if new.popup.background != self.config.popup.background {
            self.close_popup();
            match Popup::create(self.instance, translucent_popups(&new)) {
                Ok(popup) => self.popup = popup,
                Err(e) => log::warn!("could not recreate the popup: {e}"),
            }
        }
        if new.general.language != self.config.general.language {
            crate::language::apply(&new);
            self.close_popup();
            self.overlays
                .iter_mut()
                .for_each(Overlay::invalidate_layout);
            if let Some(strip) = self.game.as_mut().and_then(|g| g.strip.as_mut()) {
                strip.invalidate_layout();
            }
        }
        if new.alerts != self.config.alerts {
            self.alerts = Alerts::default();
        }
        let hotkeys_changed = new.hotkeys != self.config.hotkeys;
        self.config = new;
        if hotkeys_changed {
            self.register_hotkeys();
        }
        self.tray.set_visible(self.config.general.tray_icon);
        self.palette = Palette::current(&self.config);
        if !self.config.display.hover_popup {
            self.close_popup();
        }
        self.refresh();
        self.redraw();
    }

    fn apply(&mut self, command: Command) {
        match command {
            Command::Mode(mode) => self.change_config(|c| c.display.mode = mode),
            Command::Toggle(toggle) => self.change_config(|c| menu::apply_toggle(c, toggle)),
            Command::Edit(edit) => self.change_config(|c| menu::apply_edit(c, edit)),
            Command::Settings => self.open_settings(Page::General),
            Command::Details => self.open_details(None),
            Command::About => self.open_settings(Page::About),
            Command::TaskManager => menu::open_task_manager(),
            Command::ToggleGame => self.toggle_game(),
            Command::Record => self.toggle_recording(),
            Command::Profile(index) => {
                if let Some(id) = self.menu_profiles.get(index).cloned() {
                    self.apply_profile(&id);
                }
            }
            Command::ManageProfiles => self.open_settings(Page::Profiles),
            Command::Exit => unsafe {
                let _ = DestroyWindow(self.host);
            },
        }
    }
}

impl Drop for App {
    fn drop(&mut self) {
        self.register_appbar(ABM_REMOVE);
    }
}

fn documents_dir() -> Option<PathBuf> {
    unsafe {
        let path = SHGetKnownFolderPath(&FOLDERID_Documents, KF_FLAG_DEFAULT, None).ok()?;
        let text = path.to_string();
        CoTaskMemFree(Some(path.0.cast()));
        text.ok().map(PathBuf::from)
    }
}

/// Each drive's letters and model, by physical drive number.
fn disk_names() -> Vec<(u8, String)> {
    pulsar_core::hardware::storage::read()
        .into_iter()
        .filter_map(|d| {
            let mut name: Vec<String> = d.letters.iter().map(|l| format!("{l}:")).collect();
            name.push(d.model);
            Some((u8::try_from(d.number).ok()?, name.join(" ")))
        })
        .collect()
}

fn saved_profiles(config: &Config) -> Vec<(String, String, bool)> {
    let Some(dir) = profile::profiles_dir() else {
        return Vec::new();
    };
    profile::list(&dir)
        .into_iter()
        .map(|e| {
            let active = profile::load(&dir, &e.id).is_some_and(|p| p.matches(config));
            (e.id, e.name, active)
        })
        .collect()
}

fn translucent_popups(config: &Config) -> bool {
    config.popup.background == pulsar_core::config::PopupBackground::Translucent
}

/// `item` is the overlay item that was right-clicked, if any.
fn context_menu(x: i32, y: i32, item: Option<(HWND, i32, i32)>) {
    let Some((host, config, kind, on_widget, dark, state)) = with_app(|a| {
        a.close_popup();
        let kind = item.and_then(|(overlay, x, y)| a.hit(overlay, x, y));
        let on_widget = item.is_some_and(|(hwnd, ..)| hwnd == a.widget.hwnd);
        let game = a
            .last_app
            .clone()
            .filter(|_| a.config.game.watching())
            .map(|path| (games::display_name(&path), a.is_game(&path)));
        (
            a.host,
            a.config.clone(),
            kind,
            on_widget,
            !a.palette.light(),
            menu::AppState {
                game,
                recording: a.recorder.is_some(),
                profiles: {
                    let saved = saved_profiles(&a.config);
                    a.menu_profiles = saved.iter().map(|(id, ..)| id.clone()).collect();
                    saved
                        .into_iter()
                        .map(|(_, name, active)| (name, active))
                        .collect()
                },
            },
        )
    }) else {
        return;
    };
    if let Some(command) = menu::show(host, x, y, &config, kind, on_widget, dark, &state) {
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

fn tray_balloon_clicked(event: u32) -> bool {
    event == NIN_BALLOONUSERCLICK
}

fn tray_balloon_timed_out(event: u32) -> bool {
    event == NIN_BALLOONTIMEOUT
}

/// `MK_CONTROL` in a mouse message's key state.
fn ctrl_held(wp: WPARAM) -> bool {
    wp.0 & 0x0008 != 0
}

fn cursor() -> (i32, i32) {
    let mut pt = POINT::default();
    unsafe {
        let _ = GetCursorPos(&mut pt);
    }
    (pt.x, pt.y)
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
        WM_APP_UPDATE => {
            with_app(App::on_update_checked);
        }
        WM_APP_APPBAR if wp.0 as u32 == ABN_FULLSCREENAPP => {
            with_app(App::watch);
        }
        WM_APP_TRAY => {
            let event = low_word(lp.0 as usize);
            if tray_opens_menu(event) {
                let (x, y) = signed_words(wp.0);
                context_menu(x, y, None);
            } else if tray_opens_settings(event) {
                with_app(|a| a.apply(Command::Settings));
            } else if tray_balloon_clicked(event) {
                with_app(App::on_balloon_click);
            } else if tray_balloon_timed_out(event) {
                with_app(|a| a.tray.balloon_closed());
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
        WM_HOTKEY => {
            with_app(|a| a.on_hotkey(wp.0));
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
            with_app(|a| {
                if !a.on_drag_move(hwnd) {
                    a.on_mouse_move(hwnd, x, y);
                }
            });
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            with_app(|a| a.on_button_down(hwnd));
            LRESULT(0)
        }
        WM_CAPTURECHANGED => {
            with_app(|a| a.drag = None);
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
            let dragged = with_app(|a| a.on_button_up(hwnd)).unwrap_or(false);
            if !dragged {
                let (x, y) = signed_words(lp.0 as usize);
                let ctrl = ctrl_held(wp);
                if with_app(|a| a.on_click(hwnd, x, y, ctrl)).unwrap_or(!ctrl) {
                    menu::open_task_manager();
                }
            }
            LRESULT(0)
        }
        WM_RBUTTONUP => {
            let mut pt = POINT::default();
            unsafe {
                let _ = GetCursorPos(&mut pt);
            }
            let (x, y) = signed_words(lp.0 as usize);
            context_menu(pt.x, pt.y, Some((hwnd, x, y)));
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wp, lp) },
    }
}

unsafe extern "system" fn widget_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        WM_SETCURSOR if with_app(|a| a.config.widget.lock_position) == Some(false) => {
            widget::drag_cursor();
            LRESULT(1)
        }
        WM_LBUTTONDOWN => {
            with_app(App::on_widget_button_down);
            LRESULT(0)
        }
        WM_MOUSEMOVE => {
            with_app(App::on_widget_drag_move);
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            with_app(App::on_widget_button_up);
            LRESULT(0)
        }
        WM_CAPTURECHANGED => {
            with_app(|a| a.widget_drag = None);
            LRESULT(0)
        }
        WM_RBUTTONUP => {
            let mut pt = POINT::default();
            unsafe {
                let _ = GetCursorPos(&mut pt);
            }
            let (x, y) = signed_words(lp.0 as usize);
            context_menu(pt.x, pt.y, Some((hwnd, x, y)));
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
    widget::register_class(instance, Some(widget_proc));

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
    fn ctrl_is_read_from_the_key_state() {
        assert!(ctrl_held(WPARAM(0x0008)));
        assert!(ctrl_held(WPARAM(0x0008 | 0x0004)), "with Shift too");
        assert!(!ctrl_held(WPARAM(0x0004)));
        assert!(!ctrl_held(WPARAM(0)));
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

    #[test]
    fn clicking_a_balloon_opens_its_target() {
        assert!(tray_balloon_clicked(NIN_BALLOONUSERCLICK));
        assert!(!tray_balloon_clicked(NIN_SELECT));
        assert!(tray_balloon_timed_out(NIN_BALLOONTIMEOUT));
        assert!(!tray_balloon_timed_out(NIN_BALLOONUSERCLICK));
    }
}
