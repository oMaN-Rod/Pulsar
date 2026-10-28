#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod autostart;
mod fonts;
mod form;
#[path = "../language.rs"]
mod language;
#[path = "../names.rs"]
mod names;
mod notify;
mod titlebar;

use std::cell::{Cell, RefCell};
use std::error::Error;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, SystemTime};

use pulsar_core::colors::{self, default_rgb};
use pulsar_core::config::{self, Config};
use pulsar_core::hardware::displays::{self, Display};
use pulsar_core::hotkey::Hotkey;
use pulsar_core::layout::CellPart;
use pulsar_core::metric::ItemKind;
use pulsar_core::single_instance::SingleInstance;
use pulsar_core::sources::{fixed_drives, hardware_adapters};
use pulsar_core::theme::{self, Theme};
use pulsar_core::{crash, ipc, l10n, logging, paths, project, system_theme, tr};
use slint::{Color, ComponentHandle, Model, ModelRc, SharedString, Timer, TimerMode, VecModel};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
use windows::core::{HSTRING, PCWSTR, w};

use form::Form;

slint::include_modules!();

/// Edits are saved once typing or dragging pauses for this long.
const SAVE_DELAY: Duration = Duration::from_millis(300);
/// How often to look for changes made by the app (e.g. the tray menu).
const WATCH_INTERVAL: Duration = Duration::from_secs(1);
const ABOUT_TAB: i32 = 8;
const CAPTION_RETRY: Duration = Duration::from_millis(15);

struct State {
    path: PathBuf,
    config: Config,
    /// Item kinds in the order of the `items` model rows.
    kinds: Vec<ItemKind>,
    /// Modification time of our last write, to tell our saves from others.
    written: Option<SystemTime>,
    choices: Choices,
}

/// What the font and adapter pickers offer, and the value behind each entry.
#[derive(Default)]
struct Choices {
    fonts: Vec<String>,
    adapters: Vec<String>,
    font_values: Vec<String>,
    adapter_values: Vec<String>,
    monitors: Vec<Display>,
    /// Device paths in the order of the frame rate picker.
    monitor_values: Vec<String>,
    /// Theme ids in the order of the theme picker.
    theme_values: Vec<String>,
    /// The selected theme, for the colour fields' fallbacks.
    theme: Option<Theme>,
}

impl Choices {
    fn theme(&self) -> &theme::Colors {
        static WINDOWS: std::sync::LazyLock<Theme> = std::sync::LazyLock::new(Theme::windows);
        self.theme
            .as_ref()
            .unwrap_or(&WINDOWS)
            .colors(system_theme::taskbar_is_light())
    }
}

/// A picker with a "none" first entry. A configured value that is no longer
/// available stays listed, marked, so it is not silently replaced.
fn picker(
    first: &str,
    available: &[String],
    current: &str,
    missing: &str,
) -> (Vec<SharedString>, Vec<String>, i32) {
    let mut names: Vec<SharedString> = vec![first.into()];
    let mut values = vec![String::new()];
    for a in available {
        names.push(a.as_str().into());
        values.push(a.clone());
    }
    if !current.is_empty() && !available.iter().any(|a| a == current) {
        names.push(format!("{current} ({missing})").into());
        values.push(current.to_string());
    }
    let index = values.iter().position(|v| v == current).unwrap_or(0);
    (names, values, index as i32)
}

/// The foreground app, then each monitor, then the chosen one if it is
/// not connected.
fn monitor_picker(monitors: &[Display], current: &str) -> (Vec<SharedString>, Vec<String>, i32) {
    let mut names: Vec<SharedString> = vec![tr!("The app in front").into()];
    let mut values = vec![String::new()];
    for m in monitors {
        names.push(tr!("Monitor: {}", m.name).into());
        values.push(m.path.clone());
    }
    if !current.is_empty() && !values.iter().any(|v| v == current) {
        names.push(tr!("A monitor that is not connected").into());
        values.push(current.to_string());
    }
    let index = values.iter().position(|v| v == current).unwrap_or(0);
    (names, values, index as i32)
}

fn strings(v: Vec<SharedString>) -> ModelRc<SharedString> {
    ModelRc::new(VecModel::from(v))
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

/// The caption colour to match: the page background and whether it is dark.
type Caption = ((u8, u8, u8), bool);

/// Matches the title bar to the page, again whenever the theme changes it.
fn refresh_caption(ui: &SettingsWindow, applied: &Cell<Option<Caption>>) {
    let bg = ui.get_backdrop().color();
    let wanted = ((bg.red(), bg.green(), bg.blue()), ui.get_dark());
    if applied.get() != Some(wanted) && titlebar::blend(ipc::settings_title(), wanted.0, wanted.1) {
        applied.set(Some(wanted));
    }
}

/// Sets up the colour pickers: hex formatting and quick swatches.
fn init_color_pickers(ui: &SettingsWindow) {
    let util = ui.global::<ColorUtil>();
    util.on_hex(|c| form::to_hex((c.red(), c.green(), c.blue())).into());
    let mut swatches: Vec<Color> = ItemKind::ALL
        .into_iter()
        .map(|k| rgb(default_rgb(k, CellPart::Main)))
        .collect();
    swatches.extend([rgb((0xFF, 0xFF, 0xFF)), rgb((0, 0, 0))]);
    util.set_swatches(ModelRc::new(VecModel::from(swatches)));
}

/// A theme picker card showing `theme` on the current taskbar.
fn theme_card(name: &str, theme: &Theme, light: bool) -> ThemeCard {
    let c = theme.colors(light);
    let alpha = |(r, g, b): colors::Rgb, percent: u8| {
        Color::from_argb_u8((u16::from(percent.min(100)) * 255 / 100) as u8, r, g, b)
    };
    let items: Vec<Color> = [
        ItemKind::Cpu,
        ItemKind::Ram,
        ItemKind::Gpu,
        ItemKind::Network,
    ]
    .into_iter()
    .map(|k| rgb(c.item(k, CellPart::Main)))
    .collect();
    ThemeCard {
        name: name.into(),
        taskbar: rgb(theme::Colors::windows(light).panel),
        panel: alpha(c.panel, c.panel_opacity),
        // The overlay's faint tiles barely show at this size.
        tile: alpha(c.tile, c.tile_opacity.saturating_mul(2)),
        label: rgb(c.label),
        text: rgb(c.text),
        items: ModelRc::new(VecModel::from(items)),
    }
}

/// Fills the theme picker and the colours empty fields fall back to.
fn push_theme(ui: &SettingsWindow, f: &Form, choices: &mut Choices) {
    let dir = theme::themes_dir();
    let entries = theme::list(dir.as_deref());
    let light = system_theme::taskbar_is_light();
    let cards: Vec<ThemeCard> = entries
        .iter()
        .map(|e| {
            theme_card(
                &e.display_name(),
                &theme::load(dir.as_deref(), Some(&e.id)),
                light,
            )
        })
        .collect();
    let index = entries.iter().position(|e| e.id == f.theme).unwrap_or(0);
    choices.theme_values = entries.into_iter().map(|e| e.id).collect();
    ui.set_themes(ModelRc::new(VecModel::from(cards)));
    ui.set_theme_index(index as i32);
    choices.theme = Some(theme::load(
        dir.as_deref(),
        Some(&f.theme).filter(|t| !t.is_empty()).map(String::as_str),
    ));
    let colors = choices.theme();
    ui.set_label_default(rgb(colors.label));
    ui.set_value_default(rgb(colors.text));
    ui.set_tile_default(rgb(colors.tile));
    ui.set_panel_default(rgb(colors.panel));
}

fn refresh_swatches(ui: &SettingsWindow) {
    ui.set_tile_swatch(swatch(&ui.get_tile_color()));
    ui.set_panel_swatch(swatch(&ui.get_panel_color()));
    ui.set_label_swatch(swatch(&ui.get_label_color()));
    ui.set_value_swatch(swatch(&ui.get_value_color()));
    ui.set_label_light_swatch(swatch(&ui.get_label_color_light()));
    ui.set_value_light_swatch(swatch(&ui.get_value_color_light()));
}

fn push(ui: &SettingsWindow, f: &Form, choices: &mut Choices) {
    push_theme(ui, f, choices);
    ui.set_autostart(f.autostart);
    ui.set_update_check(f.update_check);
    ui.set_tray_icon(f.tray_icon);
    ui.set_sample_interval_ms(f.sample_interval_ms);
    ui.set_history_len(f.history_len);
    ui.set_mode(i32::from(f.text_mode));
    ui.set_graph_inline(f.graph_inline);
    ui.set_hover_popup(f.hover_popup);
    ui.set_position(f.position);
    ui.set_lock_position(f.lock_position);
    ui.set_hide_in_fullscreen(f.hide_in_fullscreen);
    ui.set_labels(f.labels);
    ui.set_icons(f.icons);
    ui.set_label_color(f.label_color.as_str().into());
    ui.set_value_color(f.value_color.as_str().into());
    ui.set_label_color_light(f.label_color_light.as_str().into());
    ui.set_value_color_light(f.value_color_light.as_str().into());
    ui.set_color_labels(f.color_labels);
    ui.set_temperature_unit(i32::from(f.fahrenheit));
    ui.set_network_unit(i32::from(f.network_bits));
    ui.set_upload_on_top(f.upload_on_top);
    ui.set_game_strip(f.game_strip);
    ui.set_game_summary(f.game_summary);
    ui.set_popup_detail(f.popup_detail);
    ui.set_popup_graph(f.popup_graph);
    ui.set_popup_translucent(f.popup_translucent);
    ui.set_popup_processes(f.popup_processes);
    ui.set_widget_enabled(f.widget_enabled);
    ui.set_widget_layer(i32::from(f.widget_on_top));
    ui.set_widget_style(f.widget_style);
    ui.set_widget_lock(f.widget_lock);
    ui.set_widget_click_through(f.widget_click_through);
    ui.set_widget_detail(f.widget_detail);
    ui.set_widget_width(f.widget_width);
    ui.set_widget_opacity(f.widget_opacity as f32);
    ui.set_widget_hide_in_fullscreen(f.widget_hide_in_fullscreen);
    ui.set_widget_own_items(f.widget_own_items);
    let widget_rows: Vec<WidgetItemRow> = f
        .items
        .iter()
        .map(|r| WidgetItemRow {
            name: names::item_name(r.kind).into(),
            checked: f.widget_items.contains(&r.kind),
            style: r.widget_style,
            detail: r.widget_detail,
        })
        .collect();
    ui.set_widget_items(ModelRc::new(VecModel::from(widget_rows)));
    ui.set_font_bold(f.font_bold);
    ui.set_language_index(
        l10n::LANGUAGES
            .iter()
            .position(|l| l.code == f.language)
            .map_or(0, |i| i as i32 + 1),
    );
    let (names, values, index) = picker(
        tr!("Default"),
        &choices.fonts,
        &f.font_family,
        tr!("not installed"),
    );
    ui.set_fonts(strings(names));
    ui.set_font_index(index);
    choices.font_values = values;
    let (names, values, index) = picker(
        tr!("All hardware adapters"),
        &choices.adapters,
        &f.adapter,
        tr!("not connected"),
    );
    ui.set_adapters(strings(names));
    ui.set_adapter_index(index);
    choices.adapter_values = values;
    let (names, values, index) = monitor_picker(&choices.monitors, &f.frame_monitor);
    ui.set_frame_monitors(strings(names));
    ui.set_frame_monitor_index(index);
    choices.monitor_values = values;
    let mut letters: Vec<String> = fixed_drives()
        .into_iter()
        .map(|d| (d as char).to_string())
        .collect();
    for d in &f.drives {
        if !letters.contains(d) {
            letters.push(d.clone());
        }
    }
    letters.sort();
    let drives: Vec<DriveRow> = letters
        .into_iter()
        .map(|letter| DriveRow {
            checked: f.drives.contains(&letter),
            letter: letter.into(),
        })
        .collect();
    ui.set_drive_rows(ModelRc::new(VecModel::from(drives)));
    ui.set_per_drive(!f.drives.is_empty());
    ui.set_drive_used(f.drive_used);
    ui.set_offset_px(f.offset_px);
    ui.set_fallback_margin_px(f.fallback_margin_px);
    ui.set_font_size_pt(f.font_size_pt);
    ui.set_show_on_all_taskbars(f.show_on_all_taskbars);
    ui.set_accent_graphs(f.accent_graphs);
    ui.set_tile_color(f.tile_color.as_str().into());
    ui.set_tile_opacity_custom(f.tile_opacity_custom);
    ui.set_tile_opacity(f.tile_opacity as f32);
    ui.set_panel_color(f.panel_color.as_str().into());
    ui.set_panel_opacity_custom(f.panel_opacity_custom);
    ui.set_panel_opacity(f.panel_opacity as f32);
    // Keep a row open across a reload, such as after moving it.
    let open: Vec<SharedString> = ui
        .get_items()
        .iter()
        .filter(|r| r.expanded)
        .map(|r| r.name)
        .collect();
    let rows: Vec<ItemRow> = f
        .items
        .iter()
        .map(|r| {
            let name: SharedString = names::item_name(r.kind).into();
            ItemRow {
                expanded: open.contains(&name),
                name,
                enabled: r.enabled,
                color: r.color.as_str().into(),
                swatch: rgb(form::item_rgb(r.kind, &r.color, choices.theme())),
                style: r.style,
                color_mode: r.color_mode,
                popup: r.popup,
            }
        })
        .collect();
    ui.set_items(ModelRc::new(VecModel::from(rows)));
    refresh_swatches(ui);
    ui.set_ping_host(f.ping_host.as_str().into());
    ui.set_ping_interval_ms(f.ping_interval_ms);
    ui.set_alert_fahrenheit(f.alert_fahrenheit);
    ui.set_alert_cpu(f.alert_cpu);
    ui.set_hotkey_show_hide(f.hotkey_show_hide.as_str().into());
    ui.set_hotkey_widget(f.hotkey_widget.as_str().into());
    ui.set_hotkey_details(f.hotkey_details.as_str().into());
    ui.set_alert_cpu_limit(f.alert_cpu_limit);
    ui.set_alert_gpu(f.alert_gpu);
    ui.set_alert_gpu_limit(f.alert_gpu_limit);
    ui.set_alert_battery(f.alert_battery);
    ui.set_alert_battery_limit(f.alert_battery_limit);
    ui.set_alert_drives(f.alert_drives);
    ui.set_alert_drives_limit(f.alert_drives_limit);
}

fn pull(ui: &SettingsWindow, kinds: &[ItemKind], choices: &Choices) -> Form {
    let chosen = |values: &[String], index: i32| {
        values
            .get(index.max(0) as usize)
            .cloned()
            .unwrap_or_default()
    };
    Form {
        autostart: ui.get_autostart(),
        update_check: ui.get_update_check(),
        tray_icon: ui.get_tray_icon(),
        sample_interval_ms: ui.get_sample_interval_ms(),
        history_len: ui.get_history_len(),
        text_mode: ui.get_mode() == 1,
        graph_inline: ui.get_graph_inline(),
        hover_popup: ui.get_hover_popup(),
        position: ui.get_position(),
        lock_position: ui.get_lock_position(),
        hide_in_fullscreen: ui.get_hide_in_fullscreen(),
        labels: ui.get_labels(),
        icons: ui.get_icons(),
        label_color: ui.get_label_color().to_string(),
        value_color: ui.get_value_color().to_string(),
        label_color_light: ui.get_label_color_light().to_string(),
        value_color_light: ui.get_value_color_light().to_string(),
        color_labels: ui.get_color_labels(),
        font_family: chosen(&choices.font_values, ui.get_font_index()),
        font_bold: ui.get_font_bold(),
        offset_px: ui.get_offset_px(),
        fallback_margin_px: ui.get_fallback_margin_px(),
        font_size_pt: ui.get_font_size_pt(),
        show_on_all_taskbars: ui.get_show_on_all_taskbars(),
        accent_graphs: ui.get_accent_graphs(),
        tile_color: ui.get_tile_color().to_string(),
        tile_opacity_custom: ui.get_tile_opacity_custom(),
        tile_opacity: ui.get_tile_opacity().round() as i32,
        panel_color: ui.get_panel_color().to_string(),
        panel_opacity_custom: ui.get_panel_opacity_custom(),
        panel_opacity: ui.get_panel_opacity().round() as i32,
        theme: choices
            .theme
            .as_ref()
            .map(|t| t.id.clone())
            .filter(|id| id != theme::DEFAULT_ID)
            .unwrap_or_default(),
        items: ui
            .get_items()
            .iter()
            .zip(ui.get_widget_items().iter())
            .zip(kinds)
            .map(|((r, w), &kind)| form::ItemRow {
                kind,
                enabled: r.enabled,
                color: r.color.to_string(),
                style: r.style,
                color_mode: r.color_mode,
                popup: r.popup,
                widget_style: w.style,
                widget_detail: w.detail,
            })
            .collect(),
        ping_host: ui.get_ping_host().to_string(),
        ping_interval_ms: ui.get_ping_interval_ms(),
        alert_cpu: ui.get_alert_cpu(),
        alert_cpu_limit: ui.get_alert_cpu_limit(),
        alert_gpu: ui.get_alert_gpu(),
        alert_gpu_limit: ui.get_alert_gpu_limit(),
        alert_battery: ui.get_alert_battery(),
        alert_battery_limit: ui.get_alert_battery_limit(),
        alert_drives: ui.get_alert_drives(),
        alert_drives_limit: ui.get_alert_drives_limit(),
        alert_fahrenheit: ui.get_alert_fahrenheit(),
        hotkey_show_hide: ui.get_hotkey_show_hide().to_string(),
        hotkey_widget: ui.get_hotkey_widget().to_string(),
        hotkey_details: ui.get_hotkey_details().to_string(),
        drives: if ui.get_per_drive() {
            ui.get_drive_rows()
                .iter()
                .filter(|r| r.checked)
                .map(|r| r.letter.to_string())
                .collect()
        } else {
            Vec::new()
        },
        drive_used: ui.get_drive_used(),
        adapter: chosen(&choices.adapter_values, ui.get_adapter_index()),
        frame_monitor: chosen(&choices.monitor_values, ui.get_frame_monitor_index()),
        language: (ui.get_language_index() - 1)
            .try_into()
            .ok()
            .and_then(|i: usize| l10n::LANGUAGES.get(i))
            .map(|l| l.code.to_string())
            .unwrap_or_default(),
        fahrenheit: ui.get_temperature_unit() == 1,
        network_bits: ui.get_network_unit() == 1,
        upload_on_top: ui.get_upload_on_top(),
        game_strip: ui.get_game_strip(),
        game_summary: ui.get_game_summary(),
        popup_detail: ui.get_popup_detail(),
        popup_graph: ui.get_popup_graph(),
        popup_translucent: ui.get_popup_translucent(),
        popup_processes: ui.get_popup_processes(),
        widget_enabled: ui.get_widget_enabled(),
        widget_on_top: ui.get_widget_layer() == 1,
        widget_style: ui.get_widget_style(),
        widget_lock: ui.get_widget_lock(),
        widget_click_through: ui.get_widget_click_through(),
        widget_detail: ui.get_widget_detail(),
        widget_width: ui.get_widget_width(),
        widget_opacity: ui.get_widget_opacity().round() as i32,
        widget_hide_in_fullscreen: ui.get_widget_hide_in_fullscreen(),
        widget_own_items: ui.get_widget_own_items(),
        widget_items: ui
            .get_widget_items()
            .iter()
            .zip(kinds)
            .filter(|(r, _)| r.checked)
            .map(|(_, &kind)| kind)
            .collect(),
    }
}

/// The text the window gets from here rather than from its `.slint` file.
fn localize(ui: &SettingsWindow) {
    ui.set_window_title(ipc::settings_title().into());
    ui.set_ui_font(language::ui_font().into());
    ui.set_styles(strings(
        names::STYLES
            .iter()
            .map(|&s| names::style_name(s).into())
            .collect(),
    ));
    ui.set_color_modes(strings(
        names::COLOR_MODES
            .iter()
            .map(|&m| names::color_mode_name(m).into())
            .collect(),
    ));
    let details: Vec<SharedString> = names::DETAILS
        .iter()
        .map(|&d| names::detail_name(d).into())
        .collect();
    ui.set_popup_details(strings(details.clone()));
    ui.set_item_popup_details(strings(
        std::iter::once(tr!("Same as other items").into())
            .chain(details.clone())
            .collect(),
    ));
    ui.set_widget_item_styles(strings(
        std::iter::once(tr!("Widget style").into())
            .chain(
                names::WIDGET_STYLES
                    .iter()
                    .map(|&s| names::widget_style_name(s).into()),
            )
            .collect(),
    ));
    ui.set_widget_item_details(strings(
        std::iter::once(tr!("Widget detail").into())
            .chain(details)
            .collect(),
    ));
    let default = tr!("Windows default ({})", l10n::system_default().name);
    ui.set_languages(strings(
        std::iter::once(default.into())
            .chain(l10n::LANGUAGES.iter().map(|l| l.name.into()))
            .collect(),
    ));
}

/// Switches the window to the config's language if that changed.
fn follow_language(ui: &SettingsWindow, config: &Config) -> bool {
    let before = l10n::current();
    let now = language::apply(config);
    if std::ptr::eq(before, now) {
        return false;
    }
    language::apply_to_slint(now);
    localize(ui);
    true
}

fn show(ui: &SettingsWindow, state: &mut State, config: Config) {
    follow_language(ui, &config);
    state.kinds = config.items.iter().map(|i| i.kind).collect();
    push(ui, &form::to_form(&config), &mut state.choices);
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
    if let Err(e) = autostart::set(autostart::VALUE_NAME, enabled.then_some(command.as_str())) {
        log::warn!("could not change autostart: {e}");
    }
}

fn save(ui: &SettingsWindow, state: &mut State) {
    let new = form::to_config(&pull(ui, &state.kinds, &state.choices), &state.config);
    write(state, new);
    if follow_language(ui, &state.config) {
        push(ui, &form::to_form(&state.config), &mut state.choices);
    }
}

fn write(state: &mut State, new: Config) {
    if new == state.config {
        return;
    }
    if new.general.autostart != state.config.general.autostart {
        sync_autostart(new.general.autostart);
    }
    match config::save(&state.path, &new) {
        Ok(()) => {
            state.written = modified(&state.path);
            state.config = new;
            notify::post_to_class(ipc::HOST_CLASS, ipc::CONFIG_CHANGED);
        }
        Err(e) => log::warn!("could not save settings: {e}"),
    }
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
    // Read without moving a bad file aside; that is the running instance's job.
    language::init(
        &config::default_path()
            .and_then(|p| config::read(&p).ok())
            .unwrap_or_default(),
    );
    let Some(_instance) = SingleInstance::acquire(ipc::SETTINGS_MUTEX) else {
        if about {
            notify::signal(ipc::SHOW_ABOUT_EVENT);
        }
        notify::focus_window(ipc::settings_title());
        return Ok(());
    };
    if let Some(dir) = paths::logs_dir() {
        logging::init(&dir, "pulsar-settings");
        crash::install_panic_hook(dir, "pulsar-settings", env!("CARGO_PKG_VERSION"));
    }
    log::info!("Settings {} opened", env!("CARGO_PKG_VERSION"));
    let about_requests = notify::Request::create(ipc::SHOW_ABOUT_EVENT);
    let path = config::default_path().ok_or("APPDATA is not set")?;

    let ui = SettingsWindow::new()?;
    language::apply_to_slint(l10n::current());
    localize(&ui);
    ui.set_version(env!("CARGO_PKG_VERSION").into());
    ui.set_config_path(SharedString::from(path.display().to_string()));
    let logs = paths::logs_dir();
    ui.set_logs_path(
        logs.as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_default()
            .into(),
    );
    ui.set_repo_url(project::REPO_URL.into());
    ui.set_issues_url(project::ISSUES_URL.into());
    ui.on_open_logs_folder(move || {
        if let Some(dir) = &logs {
            let _ = std::fs::create_dir_all(dir);
            let _ = std::process::Command::new("explorer").arg(dir).spawn();
        }
    });
    ui.on_open_link(|url| open_link(&url));
    ui.on_save_diagnostics({
        let weak = ui.as_weak();
        move || {
            let Some(ui) = weak.upgrade() else { return };
            ui.set_diagnostics_status(tr!("Reading every source…").into());
            let config = config::default_path()
                .and_then(|p| config::read(&p).ok())
                .unwrap_or_default();
            let weak = weak.clone();
            std::thread::spawn(move || {
                pulsar_core::format::set_units(config.units);
                let header = format!("Pulsar {} diagnostics", env!("CARGO_PKG_VERSION"));
                let saved = pulsar_core::diag::save(&config, &header, None);
                let _ = slint::invoke_from_event_loop(move || {
                    let Some(ui) = weak.upgrade() else { return };
                    let status = match &saved {
                        Ok(path) => {
                            open_link(&path.display().to_string());
                            tr!("Saved to {}", path.display())
                        }
                        Err(e) => tr!("Could not save the report: {}", e),
                    };
                    ui.set_diagnostics_status(status.into());
                });
            });
        }
    });
    init_color_pickers(&ui);
    if about {
        ui.set_tab(ABOUT_TAB);
    }

    let state = Rc::new(RefCell::new(State {
        path,
        config: Config::default(),
        kinds: Vec::new(),
        written: None,
        choices: Choices {
            fonts: fonts::system_families(),
            adapters: hardware_adapters(),
            monitors: displays::active(),
            ..Choices::default()
        },
    }));
    {
        let mut s = state.borrow_mut();
        let mut loaded = config::load(&s.path).config;
        // The installer or an older build may have written the Run value
        // without the config knowing.
        loaded.general.autostart = autostart::get(autostart::VALUE_NAME).is_some();
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
    ui.on_item_choice({
        let weak = ui.as_weak();
        move |index, field, value| {
            if let Some(ui) = weak.upgrade() {
                update_row(&ui, index, |r| match field {
                    0 => r.style = value,
                    1 => r.color_mode = value,
                    _ => r.popup = value,
                });
                ui.invoke_changed();
            }
        }
    });
    ui.on_item_expanded({
        let weak = ui.as_weak();
        move |index, expanded| {
            if let Some(ui) = weak.upgrade() {
                update_row(&ui, index, |r| r.expanded = expanded);
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
                r.swatch = rgb(form::item_rgb(kind, &color, state.borrow().choices.theme()));
                r.color = color;
            });
            ui.invoke_changed();
        }
    });
    ui.on_move_item({
        let (weak, state) = (ui.as_weak(), state.clone());
        move |index, up| {
            let Some(ui) = weak.upgrade() else { return };
            let mut s = state.borrow_mut();
            let mut f = pull(&ui, &s.kinds, &s.choices);
            form::move_item(&mut f.items, index.max(0) as usize, up);
            s.kinds = f.items.iter().map(|r| r.kind).collect();
            push(&ui, &f, &mut s.choices);
            drop(s);
            ui.invoke_changed();
        }
    });
    ui.on_apply_theme({
        let (weak, state) = (ui.as_weak(), state.clone());
        move |index| {
            let Some(ui) = weak.upgrade() else { return };
            let mut s = state.borrow_mut();
            let Some(id) = s.choices.theme_values.get(index.max(0) as usize).cloned() else {
                return;
            };
            let chosen = theme::load(theme::themes_dir().as_deref(), Some(&id));
            let mut f = pull(&ui, &s.kinds, &s.choices);
            form::apply_theme(&mut f, &chosen);
            f.theme = if id == theme::DEFAULT_ID {
                String::new()
            } else {
                id
            };
            s.choices.theme = Some(chosen);
            push(&ui, &f, &mut s.choices);
            drop(s);
            ui.invoke_changed();
        }
    });
    ui.on_save_theme({
        let (weak, state) = (ui.as_weak(), state.clone());
        move |name| {
            let Some(ui) = weak.upgrade() else { return };
            let Some(dir) = theme::themes_dir() else {
                return;
            };
            let mut s = state.borrow_mut();
            let mut f = pull(&ui, &s.kinds, &s.choices);
            let base = s.choices.theme.clone().unwrap_or_else(Theme::windows);
            let new = form::theme_from(&f, &base, &name);
            if new.name.is_empty() {
                return;
            }
            match theme::save(&dir, &new) {
                Ok(id) => {
                    let saved = Theme {
                        id: id.clone(),
                        ..new
                    };
                    form::apply_theme(&mut f, &saved);
                    s.choices.theme = Some(saved);
                    push(&ui, &f, &mut s.choices);
                    ui.set_theme_status(tr!("Saved.").into());
                }
                Err(e) => {
                    log::warn!("could not save theme: {e}");
                    ui.set_theme_status(tr!("Could not save the theme: {}", e).into());
                    return;
                }
            }
            drop(s);
            ui.invoke_changed();
        }
    });
    ui.on_open_themes_folder(|| {
        let Some(dir) = theme::themes_dir() else {
            return;
        };
        if let Err(e) = std::fs::create_dir_all(&dir) {
            log::warn!("could not create {}: {e}", dir.display());
            return;
        }
        let _ = std::process::Command::new("explorer").arg(dir).spawn();
    });
    ui.on_reset_all({
        let (weak, state) = (ui.as_weak(), state.clone());
        move || {
            let Some(ui) = weak.upgrade() else { return };
            let mut s = state.borrow_mut();
            let f = form::reset(&pull(&ui, &s.kinds, &s.choices));
            s.kinds = f.items.iter().map(|r| r.kind).collect();
            push(&ui, &f, &mut s.choices);
            drop(s);
            ui.invoke_changed();
        }
    });
    ui.on_reset_position({
        let state = state.clone();
        move || {
            let mut s = state.borrow_mut();
            let mut new = s.config.clone();
            new.display.float_position = None;
            write(&mut s, new);
        }
    });
    ui.on_hotkey_pressed({
        let weak = ui.as_weak();
        move |which, text, ctrl, alt, shift, win| {
            let Some(ui) = weak.upgrade() else { return };
            let value = match text.as_str() {
                // Escape, Backspace and Delete clear the shortcut.
                "\u{1b}" | "\u{8}" | "\u{7f}" => String::new(),
                text => match Hotkey::from_press(text, ctrl, alt, shift, win) {
                    Some(key) => key.to_string(),
                    None => return,
                },
            };
            let value = SharedString::from(value);
            match which {
                0 => ui.set_hotkey_show_hide(value),
                1 => ui.set_hotkey_widget(value),
                _ => ui.set_hotkey_details(value),
            }
            ui.invoke_changed();
        }
    });
    ui.on_widget_item_choice({
        let weak = ui.as_weak();
        move |index, which, value| {
            let Some(ui) = weak.upgrade() else { return };
            let rows = ui.get_widget_items();
            if let Some(mut row) = rows.row_data(index.max(0) as usize) {
                if which == 0 {
                    row.style = value;
                } else {
                    row.detail = value;
                }
                rows.set_row_data(index.max(0) as usize, row);
            }
            ui.invoke_changed();
        }
    });
    ui.on_widget_item_toggled({
        let weak = ui.as_weak();
        move |index, checked| {
            let Some(ui) = weak.upgrade() else { return };
            let rows = ui.get_widget_items();
            if let Some(mut row) = rows.row_data(index.max(0) as usize) {
                row.checked = checked;
                rows.set_row_data(index.max(0) as usize, row);
            }
            ui.invoke_changed();
        }
    });
    ui.on_drive_toggled({
        let weak = ui.as_weak();
        move |index, checked| {
            let Some(ui) = weak.upgrade() else { return };
            let rows = ui.get_drive_rows();
            if let Some(mut row) = rows.row_data(index.max(0) as usize) {
                row.checked = checked;
                rows.set_row_data(index.max(0) as usize, row);
            }
            ui.invoke_changed();
        }
    });
    ui.on_per_drive_selected({
        let weak = ui.as_weak();
        move || {
            let Some(ui) = weak.upgrade() else { return };
            let rows = ui.get_drive_rows();
            // "Each drive" with nothing ticked would mean "combined"; start with the first.
            if ui.get_per_drive()
                && !rows.iter().any(|r| r.checked)
                && let Some(mut first) = rows.row_data(0)
            {
                first.checked = true;
                rows.set_row_data(0, first);
            }
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

    let caption = Rc::new(Cell::new(None));
    // The window appears a few frames after the event loop starts; colour its
    // caption as soon as it exists rather than on the next watch tick.
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
    let watch = Timer::default();
    watch.start(TimerMode::Repeated, WATCH_INTERVAL, {
        let (weak, state, pending) = (ui.as_weak(), state.clone(), save_timer.clone());
        move || {
            let Some(ui) = weak.upgrade() else { return };
            refresh_caption(&ui, &caption);
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
