#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod autostart;
mod form;
mod notify;

use std::cell::RefCell;
use std::error::Error;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, SystemTime};

use pulsar_core::config::{self, Config};
use pulsar_core::ipc;
use pulsar_core::metric::ItemKind;
use pulsar_core::single_instance::SingleInstance;
use slint::{Color, Model, ModelRc, SharedString, Timer, TimerMode, VecModel};

use form::Form;

slint::include_modules!();

/// Edits are saved once typing or dragging pauses for this long.
const SAVE_DELAY: Duration = Duration::from_millis(300);
/// How often to look for changes made by the app (e.g. the tray menu).
const WATCH_INTERVAL: Duration = Duration::from_secs(1);
const ABOUT_TAB: i32 = 3;

struct State {
    path: PathBuf,
    config: Config,
    /// Item kinds in the order of the `items` model rows.
    kinds: Vec<ItemKind>,
    /// Modification time of our last write, to tell our saves from others.
    written: Option<SystemTime>,
}

fn modified(path: &PathBuf) -> Option<SystemTime> {
    std::fs::metadata(path).ok()?.modified().ok()
}

fn rgb((r, g, b): pulsar_core::colors::Rgb) -> Color {
    Color::from_rgb_u8(r, g, b)
}

/// Swatch for a background colour field; empty or half-typed shows as
/// transparent, meaning the theme's colour.
fn swatch(hex: &str) -> Color {
    form::parse_hex(hex).map_or(Color::from_argb_u8(0, 0, 0, 0), rgb)
}

fn refresh_swatches(ui: &SettingsWindow) {
    ui.set_tile_swatch(swatch(&ui.get_tile_color()));
    ui.set_panel_swatch(swatch(&ui.get_panel_color()));
}

fn push(ui: &SettingsWindow, f: &Form) {
    ui.set_autostart(f.autostart);
    ui.set_update_check(f.update_check);
    ui.set_sample_interval_ms(f.sample_interval_ms);
    ui.set_history_len(f.history_len);
    ui.set_mode(i32::from(f.text_mode));
    ui.set_hover_popup(f.hover_popup);
    ui.set_position(i32::from(f.position_left));
    ui.set_offset_px(f.offset_px);
    ui.set_fallback_margin_px(f.fallback_margin_px);
    ui.set_font_size_pt(f.font_size_pt);
    ui.set_show_on_all_taskbars(f.show_on_all_taskbars);
    ui.set_accent_graphs(f.accent_graphs);
    ui.set_tile_color(f.tile_color.as_str().into());
    ui.set_tile_opacity_custom(f.tile_opacity_custom);
    ui.set_tile_opacity(f.tile_opacity as f32);
    ui.set_panel_color(f.panel_color.as_str().into());
    ui.set_panel_opacity(f.panel_opacity as f32);
    let rows: Vec<ItemRow> = f
        .items
        .iter()
        .map(|r| ItemRow {
            name: form::item_name(r.kind).into(),
            enabled: r.enabled,
            color: r.color.as_str().into(),
            swatch: rgb(form::item_rgb(r.kind, &r.color)),
        })
        .collect();
    ui.set_items(ModelRc::new(VecModel::from(rows)));
    refresh_swatches(ui);
    ui.set_ping_host(f.ping_host.as_str().into());
    ui.set_ping_interval_ms(f.ping_interval_ms);
}

fn pull(ui: &SettingsWindow, kinds: &[ItemKind]) -> Form {
    Form {
        autostart: ui.get_autostart(),
        update_check: ui.get_update_check(),
        sample_interval_ms: ui.get_sample_interval_ms(),
        history_len: ui.get_history_len(),
        text_mode: ui.get_mode() == 1,
        hover_popup: ui.get_hover_popup(),
        position_left: ui.get_position() == 1,
        offset_px: ui.get_offset_px(),
        fallback_margin_px: ui.get_fallback_margin_px(),
        font_size_pt: ui.get_font_size_pt(),
        show_on_all_taskbars: ui.get_show_on_all_taskbars(),
        accent_graphs: ui.get_accent_graphs(),
        tile_color: ui.get_tile_color().to_string(),
        tile_opacity_custom: ui.get_tile_opacity_custom(),
        tile_opacity: ui.get_tile_opacity().round() as i32,
        panel_color: ui.get_panel_color().to_string(),
        panel_opacity: ui.get_panel_opacity().round() as i32,
        items: ui
            .get_items()
            .iter()
            .zip(kinds)
            .map(|(r, &kind)| form::ItemRow {
                kind,
                enabled: r.enabled,
                color: r.color.to_string(),
            })
            .collect(),
        ping_host: ui.get_ping_host().to_string(),
        ping_interval_ms: ui.get_ping_interval_ms(),
    }
}

fn show(ui: &SettingsWindow, state: &mut State, config: Config) {
    state.kinds = config.items.iter().map(|i| i.kind).collect();
    push(ui, &form::to_form(&config));
    state.config = config;
}

/// Picks up a change made by another process. A file that does not parse
/// (e.g. caught mid-write, or a hand edit in progress) is left alone and
/// retried on the next tick; unchanged contents leave the form untouched.
fn pick_up_external_change(ui: &SettingsWindow, state: &mut State) {
    let stamp = modified(&state.path);
    let Ok(config) = config::read(&state.path) else {
        return;
    };
    state.written = stamp;
    if config != state.config {
        show(ui, state, config);
    }
}

fn sync_autostart(enabled: bool) {
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let command = autostart::command_for(&ipc::sibling(&exe, ipc::APP_EXE));
    let _ = autostart::set(autostart::VALUE_NAME, enabled.then_some(command.as_str()));
}

fn save(ui: &SettingsWindow, state: &mut State) {
    let new = form::to_config(&pull(ui, &state.kinds), &state.config);
    if new == state.config {
        return;
    }
    if new.general.autostart != state.config.general.autostart {
        sync_autostart(new.general.autostart);
    }
    if config::save(&state.path, &new).is_ok() {
        state.written = modified(&state.path);
        state.config = new;
        notify::post_to_class(ipc::HOST_CLASS, ipc::CONFIG_CHANGED);
    }
}

fn update_row(ui: &SettingsWindow, index: i32, edit: impl FnOnce(&mut ItemRow)) {
    let items = ui.get_items();
    let index = index as usize;
    if let Some(mut row) = items.row_data(index) {
        edit(&mut row);
        items.set_row_data(index, row);
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let about = std::env::args().any(|a| a == ipc::ABOUT_ARG);
    let Some(_instance) = SingleInstance::acquire(ipc::SETTINGS_MUTEX) else {
        if about {
            notify::signal(ipc::SHOW_ABOUT_EVENT);
        }
        notify::focus_window(ipc::SETTINGS_TITLE);
        return Ok(());
    };
    let about_requests = notify::Request::create(ipc::SHOW_ABOUT_EVENT);
    let path = config::default_path().ok_or("APPDATA is not set")?;

    let ui = SettingsWindow::new()?;
    ui.set_window_title(ipc::SETTINGS_TITLE.into());
    ui.set_version(env!("CARGO_PKG_VERSION").into());
    ui.set_config_path(SharedString::from(path.display().to_string()));
    if about {
        ui.set_tab(ABOUT_TAB);
    }

    let state = Rc::new(RefCell::new(State {
        path,
        config: Config::default(),
        kinds: Vec::new(),
        written: None,
    }));
    {
        let mut s = state.borrow_mut();
        let loaded = config::load(&s.path).config;
        s.written = modified(&s.path);
        show(&ui, &mut s, loaded);
    }

    let save_timer = Rc::new(Timer::default());
    ui.on_changed({
        let (weak, state, timer) = (ui.as_weak(), state.clone(), save_timer.clone());
        move || {
            if let Some(ui) = weak.upgrade() {
                refresh_swatches(&ui);
            }
            let (weak, state) = (weak.clone(), state.clone());
            timer.start(TimerMode::SingleShot, SAVE_DELAY, move || {
                if let Some(ui) = weak.upgrade() {
                    save(&ui, &mut state.borrow_mut());
                }
            });
        }
    });
    ui.on_item_enabled({
        let weak = ui.as_weak();
        move |index, enabled| {
            if let Some(ui) = weak.upgrade() {
                update_row(&ui, index, |r| r.enabled = enabled);
                ui.invoke_changed();
            }
        }
    });
    ui.on_item_color({
        let (weak, state) = (ui.as_weak(), state.clone());
        move |index, color| {
            let Some(ui) = weak.upgrade() else { return };
            let Some(&kind) = state.borrow().kinds.get(index.max(0) as usize) else {
                return;
            };
            update_row(&ui, index, |r| {
                r.swatch = rgb(form::item_rgb(kind, &color));
                r.color = color;
            });
            ui.invoke_changed();
        }
    });
    ui.on_move_item({
        let (weak, state) = (ui.as_weak(), state.clone());
        move |index, up| {
            let Some(ui) = weak.upgrade() else { return };
            let mut f = pull(&ui, &state.borrow().kinds);
            form::move_item(&mut f.items, index.max(0) as usize, up);
            state.borrow_mut().kinds = f.items.iter().map(|r| r.kind).collect();
            push(&ui, &f);
            ui.invoke_changed();
        }
    });
    ui.on_open_config_folder({
        let state = state.clone();
        move || {
            if let Some(dir) = state.borrow().path.parent() {
                let _ = std::process::Command::new("explorer").arg(dir).spawn();
            }
        }
    });

    let watch = Timer::default();
    watch.start(TimerMode::Repeated, WATCH_INTERVAL, {
        let (weak, state, pending) = (ui.as_weak(), state.clone(), save_timer.clone());
        move || {
            let Some(ui) = weak.upgrade() else { return };
            if about_requests.as_ref().is_some_and(notify::Request::take) {
                ui.set_tab(ABOUT_TAB);
            }
            let changed_elsewhere = {
                let s = state.borrow();
                modified(&s.path) != s.written
            };
            if changed_elsewhere && !pending.running() {
                pick_up_external_change(&ui, &mut state.borrow_mut());
            }
        }
    });

    ui.run()?;
    if save_timer.running() {
        save_timer.stop();
        save(&ui, &mut state.borrow_mut());
    }
    Ok(())
}
