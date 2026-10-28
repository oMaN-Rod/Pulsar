//! The details window: everything Pulsar knows about the machine, with
//! the live graphs from the hover popups. `pulsar.exe` starts it and streams
//! its history and snapshots over standard input; started on its own, it
//! samples for itself.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod graph;
#[path = "../language.rs"]
mod language;
#[path = "../settings/notify.rs"]
#[allow(dead_code)]
mod notify;
mod pages;
mod prefs;
#[path = "../settings/titlebar.rs"]
mod titlebar;

use std::cell::{Cell, RefCell};
use std::error::Error;
use std::io::BufRead;
use std::rc::Rc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use pulsar_core::config::{self, Config};
use pulsar_core::feed::{self, Message};
use pulsar_core::hardware::{self, HardwareInfo};
use pulsar_core::history::HistoryStore;
use pulsar_core::metric::{ItemKind, Snapshot};
use pulsar_core::sampler::Sampler;
use pulsar_core::single_instance::SingleInstance;
use pulsar_core::{crash, ipc, l10n, logging, paths, project, sensors};
use slint::{ComponentHandle, Model, ModelRc, Timer, TimerMode, VecModel};
use windows::Win32::System::SystemInformation::GetTickCount64;
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
use windows::core::{HSTRING, PCWSTR, w};

use pages::{Data, Extremes, Page};
use prefs::{CoreView, MemoryView, Prefs};

include!(concat!(env!("OUT_DIR"), "/details.rs"));

const CAPTION_RETRY: Duration = Duration::from_millis(15);
/// Up time and theme changes, between snapshots.
const TICK: Duration = Duration::from_secs(5);

/// The page for a clicked overlay item.
fn page_for(item: Option<ItemKind>) -> Page {
    match item {
        None => Page::Overview,
        Some(ItemKind::Cpu | ItemKind::CpuTemp | ItemKind::CpuPower | ItemKind::CpuClock) => {
            Page::Cpu
        }
        Some(ItemKind::Ram) => Page::Memory,
        Some(ItemKind::Gpu | ItemKind::GpuTemp | ItemKind::Fps) => Page::Gpu,
        Some(ItemKind::Disk | ItemKind::DiskTemp) => Page::Storage,
        Some(ItemKind::ChipsetTemp | ItemKind::Battery) => Page::System,
        Some(ItemKind::Network | ItemKind::Ping) => Page::Network,
    }
}

/// Models the window keeps for its whole life, updated row by row so cards
/// are not rebuilt on every snapshot.
struct Models {
    nav: Rc<VecModel<NavEntry>>,
    graphs: Rc<VecModel<Graph>>,
    stats: Rc<VecModel<Stat>>,
    core_cells: Rc<VecModel<Core>>,
    threads: Rc<VecModel<f32>>,
    thread_graphs: Rc<VecModel<Graph>>,
    facts: Rc<VecModel<Fact>>,
    groups: Rc<VecModel<SpecGroup>>,
    sensors: Rc<VecModel<SensorRow>>,
}

impl Models {
    fn attach(ui: &DetailsWindow) -> Self {
        let m = Self {
            nav: Rc::new(VecModel::default()),
            graphs: Rc::new(VecModel::default()),
            stats: Rc::new(VecModel::default()),
            core_cells: Rc::new(VecModel::default()),
            threads: Rc::new(VecModel::default()),
            thread_graphs: Rc::new(VecModel::default()),
            facts: Rc::new(VecModel::default()),
            groups: Rc::new(VecModel::default()),
            sensors: Rc::new(VecModel::default()),
        };
        ui.set_nav(ModelRc::from(m.nav.clone()));
        ui.set_graphs(ModelRc::from(m.graphs.clone()));
        ui.set_stats(ModelRc::from(m.stats.clone()));
        ui.set_core_cells(ModelRc::from(m.core_cells.clone()));
        ui.set_threads(ModelRc::from(m.threads.clone()));
        ui.set_thread_graphs(ModelRc::from(m.thread_graphs.clone()));
        ui.set_facts(ModelRc::from(m.facts.clone()));
        ui.set_groups(ModelRc::from(m.groups.clone()));
        ui.set_sensors(ModelRc::from(m.sensors.clone()));
        m
    }
}

/// Replaces only the rows that changed.
fn sync<T: Clone + PartialEq + 'static>(model: &VecModel<T>, rows: Vec<T>) {
    let len = rows.len();
    for (i, row) in rows.into_iter().enumerate() {
        if i < model.row_count() {
            if model.row_data(i).as_ref() != Some(&row) {
                model.set_row_data(i, row);
            }
        } else {
            model.push(row);
        }
    }
    while model.row_count() > len {
        model.remove(model.row_count() - 1);
    }
}

struct App {
    ui: slint::Weak<DetailsWindow>,
    models: Models,
    prefs: Prefs,
    info: HardwareInfo,
    history: HistoryStore,
    latest: Snapshot,
    extremes: Extremes,
    config: Config,
    page: Page,
}

thread_local! {
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
}

fn with_app(f: impl FnOnce(&mut App)) {
    APP.with(|cell| {
        if let Ok(mut app) = cell.try_borrow_mut()
            && let Some(app) = app.as_mut()
        {
            f(app);
        }
    });
}

impl App {
    fn refresh(&mut self) {
        let Some(ui) = self.ui.upgrade() else { return };
        let data = Data {
            info: &self.info,
            latest: &self.latest,
            history: &self.history,
            extremes: &self.extremes,
            config: &self.config,
            accent: graph::accent(),
            uptime: Duration::from_millis(unsafe { GetTickCount64() }),
            service: sensors::read().map(drop),
            prefs: &self.prefs,
        };
        sync(&self.models.nav, pages::nav(&data));
        let view = pages::build(self.page, &data);
        ui.set_page(Page::ALL.iter().position(|&p| p == self.page).unwrap_or(0) as i32);
        ui.set_sensors_page(self.page == Page::Sensors);
        ui.set_heading(view.heading.into());
        ui.set_subheading(view.subheading.into());
        ui.set_note(view.note.into());
        ui.set_note_action(view.note_action.into());
        ui.set_feature_graph(view.feature_graph);
        ui.set_mem_usage(view.mem_usage);
        ui.set_cores(view.cores);
        sync(&self.models.graphs, view.graphs);
        sync(&self.models.stats, view.stats);
        sync(&self.models.core_cells, view.core_cells);
        sync(&self.models.threads, view.threads);
        sync(&self.models.thread_graphs, view.thread_graphs);
        sync(&self.models.facts, view.facts);
        sync(&self.models.groups, view.groups);
        sync(&self.models.sensors, view.sensors);
    }

    fn change_prefs(&mut self, change: impl FnOnce(&mut Prefs)) {
        change(&mut self.prefs);
        if let Some(path) = prefs::path()
            && let Err(e) = self.prefs.save(&path)
        {
            log::warn!("Couldn't save the details window's preferences: {e}");
        }
        self.refresh();
    }

    fn hardware_ids(&self) -> Vec<String> {
        self.models
            .sensors
            .iter()
            .filter(|r| r.depth == 0)
            .map(|r| r.id.to_string())
            .collect()
    }

    fn receive(&mut self, message: Message) {
        match message {
            Message::History { capacity, series } => {
                self.history = feed::history_store(capacity, series);
                self.extremes.seed(&self.history);
            }
            Message::Snapshot(wire) => {
                let snapshot = Snapshot::from(wire);
                self.history.record(&snapshot);
                self.extremes.record(&snapshot);
                self.latest = snapshot;
            }
            Message::Show { item } => {
                self.page = page_for(item);
                if let Some(ui) = self.ui.upgrade() {
                    ui.invoke_scroll_to_top();
                    let _ = ui.show();
                }
                notify::focus_window(ipc::details_title());
            }
        }
        self.refresh();
    }
}

/// Hands a message to the UI thread.
fn deliver(message: Message) -> bool {
    slint::invoke_from_event_loop(move || with_app(|app| app.receive(message))).is_ok()
}

/// Reads the feed from `pulsar.exe` until it closes, then closes the window.
fn read_feed() {
    std::thread::Builder::new()
        .name("pulsar-feed".into())
        .spawn(|| {
            for line in std::io::stdin().lock().lines() {
                let Ok(line) = line else { break };
                if let Some(message) = Message::decode(&line)
                    && !deliver(message)
                {
                    return;
                }
            }
            log::info!("Pulsar closed the feed");
            let _ = slint::invoke_from_event_loop(|| {
                let _ = slint::quit_event_loop();
            });
        })
        .expect("spawn feed thread");
}

/// Started on its own: sample everything, as `pulsar.exe` does for us.
fn sample_alone(config: &Config) {
    let mut sampler = Sampler::from_config(config);
    sampler.extras_flag().store(true, Ordering::Relaxed);
    sampler.processes_flag().store(true, Ordering::Relaxed);
    let interval = Duration::from_millis(config.general.sample_interval_ms.into());
    std::thread::Builder::new()
        .name("pulsar-sampler".into())
        .spawn(move || {
            loop {
                if !deliver(Message::snapshot(&sampler.sample())) {
                    return;
                }
                std::thread::sleep(interval);
            }
        })
        .expect("spawn sampler thread");
}

/// The static facts take most of a second; the window opens without them.
fn collect_hardware() {
    std::thread::spawn(|| {
        let info = hardware::collect();
        let _ = slint::invoke_from_event_loop(move || {
            with_app(|app| {
                app.info = info;
                app.refresh();
            })
        });
    });
}

fn open_link(url: &str) {
    unsafe {
        ShellExecuteW(
            None,
            w!("open"),
            &HSTRING::from(url),
            PCWSTR::null(),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        );
    }
}

type Caption = ((u8, u8, u8), bool);

fn refresh_caption(ui: &DetailsWindow, applied: &Cell<Option<Caption>>) {
    let bg = ui.get_backdrop().color();
    let wanted = ((bg.red(), bg.green(), bg.blue()), ui.get_dark());
    if applied.get() != Some(wanted) && titlebar::blend(ipc::details_title(), wanted.0, wanted.1) {
        applied.set(Some(wanted));
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let fed = std::env::args().any(|a| a == feed::FEED_ARG);
    let config = config::default_path()
        .and_then(|p| config::read(&p).ok())
        .unwrap_or_default();
    language::init(&config);
    let Some(_instance) = SingleInstance::acquire(ipc::DETAILS_MUTEX) else {
        notify::focus_window(ipc::details_title());
        return Ok(());
    };
    if let Some(dir) = paths::logs_dir() {
        logging::init(&dir, "pulsar-details");
        crash::install_panic_hook(dir, "pulsar-details", env!("CARGO_PKG_VERSION"));
    }
    log::info!("Details {} opened", env!("CARGO_PKG_VERSION"));

    let ui = DetailsWindow::new()?;
    language::apply_to_slint(l10n::current());
    ui.set_window_title(ipc::details_title().into());
    ui.set_ui_font(language::ui_font().into());
    let app = App {
        ui: ui.as_weak(),
        models: Models::attach(&ui),
        prefs: prefs::path().map(|p| Prefs::load(&p)).unwrap_or_default(),
        info: HardwareInfo::default(),
        history: HistoryStore::new(config.general.history_len),
        latest: Snapshot::default(),
        extremes: Extremes::default(),
        page: Page::Overview,
        config: config.clone(),
    };
    APP.with(|cell| *cell.borrow_mut() = Some(app));
    with_app(App::refresh);
    ui.on_select(|index| {
        with_app(|app| {
            app.page = Page::ALL[index.clamp(0, Page::ALL.len() as i32 - 1) as usize];
            app.refresh();
        })
    });
    ui.on_note_clicked(|| open_link(&format!("{}#sensor-service", project::REPO_URL)));
    ui.on_core_mode(|index| {
        with_app(|app| {
            app.change_prefs(|p| {
                p.cores = CoreView::ALL[index.clamp(0, CoreView::ALL.len() as i32 - 1) as usize]
            })
        })
    });
    ui.on_memory_mode(|index| {
        with_app(|app| app.change_prefs(|p| p.memory = MemoryView::ALL[index.clamp(0, 1) as usize]))
    });
    ui.on_toggle_sensor(|id| {
        with_app(|app| {
            app.change_prefs(|p| {
                if !p.closed.remove(id.as_str()) {
                    p.closed.insert(id.to_string());
                }
            })
        })
    });
    ui.on_expand_sensors(|| with_app(|app| app.change_prefs(|p| p.closed.clear())));
    ui.on_collapse_sensors(|| {
        with_app(|app| {
            let ids = app.hardware_ids();
            app.change_prefs(|p| p.closed.extend(ids))
        })
    });

    collect_hardware();
    if fed {
        read_feed();
    } else {
        sample_alone(&config);
    }

    let caption = Rc::new(Cell::new(None));
    let first_caption = Rc::new(Timer::default());
    first_caption.start(TimerMode::Repeated, CAPTION_RETRY, {
        let (weak, caption, timer) = (ui.as_weak(), caption.clone(), Rc::downgrade(&first_caption));
        move || {
            if let Some(ui) = weak.upgrade() {
                refresh_caption(&ui, &caption);
            }
            if caption.get().is_some()
                && let Some(timer) = timer.upgrade()
            {
                timer.stop();
            }
        }
    });
    let tick = Timer::default();
    tick.start(TimerMode::Repeated, TICK, {
        let weak = ui.as_weak();
        move || {
            if let Some(ui) = weak.upgrade() {
                refresh_caption(&ui, &caption);
            }
            with_app(App::refresh);
        }
    });

    ui.run()?;
    APP.with(|cell| cell.borrow_mut().take());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clicked_items_open_their_page() {
        assert_eq!(page_for(None), Page::Overview);
        assert_eq!(page_for(Some(ItemKind::CpuPower)), Page::Cpu);
        assert_eq!(page_for(Some(ItemKind::GpuTemp)), Page::Gpu);
        assert_eq!(page_for(Some(ItemKind::Ping)), Page::Network);
    }

    #[test]
    fn sync_touches_only_what_changed() {
        let model = VecModel::from(vec![1, 2, 3]);
        sync(&model, vec![1, 5]);
        assert_eq!(model.iter().collect::<Vec<_>>(), [1, 5]);
        sync(&model, vec![1, 5, 7, 9]);
        assert_eq!(model.iter().collect::<Vec<_>>(), [1, 5, 7, 9]);
    }
}
