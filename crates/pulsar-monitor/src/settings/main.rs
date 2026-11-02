#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod autostart;
mod fonts;
mod form;
#[path = "../language.rs"]
mod language;
#[path = "../names.rs"]
mod names;
mod notify;
mod search;
mod titlebar;

use std::cell::{Cell, RefCell};
use std::error::Error;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, SystemTime};

use pulsar_core::colors::{self, default_rgb};
use pulsar_core::config::GraphStyle;
use pulsar_core::config::{self, Config};
use pulsar_core::hardware::displays::{self, Display};
use pulsar_core::hotkey::Hotkey;
use pulsar_core::layout::CellPart;
use pulsar_core::metric::ItemKind;
use pulsar_core::profile::{self, Profile};
use pulsar_core::single_instance::SingleInstance;
use pulsar_core::sources::{fixed_drives, hardware_adapters};
use pulsar_core::theme::{self, Theme};
use pulsar_core::update::{self, Availability, Outcome, UpdateState};
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
const PROFILES_TAB: i32 = 9;
const RESULTS_TAB: i32 = -1;
const CAPTION_RETRY: Duration = Duration::from_millis(15);

struct State {
    path: PathBuf,
    config: Config,
    /// Item kinds in the order of the `items` model rows.
    kinds: Vec<ItemKind>,
    /// Modification time of our last write, to tell our saves from others.
    written: Option<SystemTime>,
    choices: Choices,
    /// Profile ids in the order of the `profiles` model rows.
    profile_ids: Vec<String>,
}

/// What the font, GPU and adapter pickers offer, and the value behind each entry.
#[derive(Default)]
struct Choices {
    fonts: Vec<String>,
    gpus: Vec<String>,
    adapters: Vec<String>,
    /// What is saved, in its order: cells are numbered by place in it.
    saved_gpus: Vec<String>,
    saved_adapters: Vec<String>,
    font_values: Vec<String>,
    monitors: Vec<Display>,
    /// Device paths in the order of the frame rate picker.
    monitor_values: Vec<String>,
    /// Device paths in the order of the taskbar picker.
    taskbar_values: Vec<String>,
    /// Theme ids in the order of the theme picker.
    theme_values: Vec<String>,
    /// The selected theme, for the color fields' fallbacks.
    theme: Option<Theme>,
    theme_id: String,
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

/// `first`, then each monitor, then the chosen one if it is not connected.
fn monitor_picker(
    first: &str,
    monitors: &[Display],
    current: &str,
) -> (Vec<SharedString>, Vec<String>, i32) {
    let mut names: Vec<SharedString> = vec![first.into()];
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

/// A row per device, numbered as their cells are when `numbered`, then
/// each chosen one that is no longer there.
fn device_rows(available: &[String], chosen: &[String], numbered: bool) -> ModelRc<DeviceRow> {
    let present = available.iter().enumerate().map(|(i, name)| DeviceRow {
        label: if numbered {
            format!("{} · {name}", i + 1)
        } else {
            name.clone()
        }
        .into(),
        value: name.as_str().into(),
        checked: chosen.contains(name),
    });
    let gone = chosen
        .iter()
        .filter(|name| !available.contains(name))
        .map(|name| DeviceRow {
            label: format!("{name} ({})", tr!("not connected")).into(),
            value: name.as_str().into(),
            checked: true,
        });
    ModelRc::new(VecModel::from(present.chain(gone).collect::<Vec<_>>()))
}

/// The ticked devices, or none while the combined reading is chosen.
fn ticked(each: bool, rows: &ModelRc<DeviceRow>, saved: &[String]) -> Vec<String> {
    if !each {
        return Vec::new();
    }
    form::in_saved_order(
        saved,
        rows.iter()
            .filter(|r| r.checked)
            .map(|r| r.value.to_string())
            .collect(),
    )
}

/// Applies a tick unless it would leave none or more than `max` ticked;
/// a refused tick returns the rows as they were, to redraw them.
fn device_toggled(
    rows: ModelRc<DeviceRow>,
    index: i32,
    checked: bool,
    max: usize,
) -> Option<ModelRc<DeviceRow>> {
    let ticked = rows.iter().filter(|r| r.checked).count();
    if (!checked && ticked <= 1) || (checked && ticked >= max) {
        let same: Vec<_> = rows.iter().collect();
        return Some(ModelRc::new(VecModel::from(same)));
    }
    if let Some(mut row) = rows.row_data(index.max(0) as usize) {
        row.checked = checked;
        rows.set_row_data(index.max(0) as usize, row);
    }
    None
}

/// "Each" with nothing ticked would mean "combined"; start with the first.
fn tick_first(rows: &ModelRc<DeviceRow>) {
    if !rows.iter().any(|r| r.checked)
        && let Some(mut first) = rows.row_data(0)
    {
        first.checked = true;
        rows.set_row_data(0, first);
    }
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

/// Swatch for a background color field; empty or half-typed shows as
/// transparent, meaning the theme's color.
fn swatch(hex: &str) -> Color {
    form::parse_hex(hex).map_or(Color::from_argb_u8(0, 0, 0, 0), rgb)
}

/// The caption color to match: the page background and whether it is dark.
type Caption = ((u8, u8, u8), bool);

/// Matches the title bar to the page, again whenever the theme changes it.
fn refresh_caption(ui: &SettingsWindow, applied: &Cell<Option<Caption>>) {
    let bg = ui.get_backdrop().color();
    let wanted = ((bg.red(), bg.green(), bg.blue()), ui.get_dark());
    if applied.get() != Some(wanted) && titlebar::blend(ipc::settings_title(), wanted.0, wanted.1) {
        applied.set(Some(wanted));
    }
}

/// Sets up the color pickers: hex formatting and quick swatches.
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

/// Fills the theme picker and the colors empty fields fall back to.
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
    choices.theme_id = f.theme.clone();
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
    ui.set_widget_background_default(rgb(colors.panel));
}

fn refresh_swatches(ui: &SettingsWindow) {
    ui.set_tile_swatch(swatch(&ui.get_tile_color()));
    ui.set_panel_swatch(swatch(&ui.get_panel_color()));
    ui.set_widget_background_swatch(swatch(&ui.get_widget_background()));
    ui.set_label_swatch(swatch(&ui.get_label_color()));
    ui.set_value_swatch(swatch(&ui.get_value_color()));
    ui.set_label_light_swatch(swatch(&ui.get_label_color_light()));
    ui.set_value_light_swatch(swatch(&ui.get_value_color_light()));
    ui.set_load_low_swatch(swatch(&ui.get_load_low_color()));
    ui.set_load_moderate_swatch(swatch(&ui.get_load_moderate_color()));
    ui.set_load_high_swatch(swatch(&ui.get_load_high_color()));
    ui.set_load_critical_swatch(swatch(&ui.get_load_critical_color()));
}

fn push(ui: &SettingsWindow, f: &Form, choices: &mut Choices) {
    push_theme(ui, f, choices);
    ui.set_autostart(f.autostart);
    ui.set_update_check(f.update_check);
    ui.set_tray_icon(f.tray_icon);
    ui.set_sample_interval_ms(f.sample_interval_ms);
    ui.set_battery_interval_on(f.battery_interval_on);
    ui.set_battery_interval_ms(f.battery_interval_ms);
    ui.set_history_len(f.history_len);
    ui.set_mode(i32::from(f.text_mode));
    ui.set_graph_inline(f.graph_inline);
    ui.set_vertical_labels(f.vertical_labels);
    ui.set_hover_popup(f.hover_popup);
    ui.set_position(f.position);
    ui.set_lock_position(f.lock_position);
    ui.set_hide_in_fullscreen(f.hide_in_fullscreen);
    ui.set_attach_to_taskbar(f.attach_to_taskbar);
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
    ui.set_network_show(f.network_show);
    ui.set_network_split_tiles(f.network_split_tiles);
    ui.set_network_popup_show(f.network_popup_show);
    ui.set_network_down_label(f.network_down_label.as_str().into());
    ui.set_network_up_label(f.network_up_label.as_str().into());
    ui.set_game_strip(f.game_strip);
    ui.set_game_summary(f.game_summary);
    ui.set_popup_detail(f.popup_detail);
    ui.set_popup_graph(f.popup_graph);
    ui.set_popup_translucent(f.popup_translucent);
    ui.set_popup_processes(f.popup_processes);
    ui.set_popup_width(f.popup_width);
    ui.set_popup_graph_height(f.popup_graph_height);
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
    ui.set_widget_background(f.widget_background.as_str().into());
    ui.set_widget_order_custom(!f.widget_order.is_empty());
    let mut in_widget_order: Vec<&form::ItemRow> = f.items.iter().collect();
    in_widget_order.sort_by_key(|r| {
        f.widget_order
            .iter()
            .position(|k| *k == r.kind)
            .unwrap_or(usize::MAX)
    });
    let widget_rows: Vec<WidgetItemRow> = in_widget_order
        .into_iter()
        .map(|r| WidgetItemRow {
            id: item_id(r.kind).into(),
            name: names::item_name(r.kind).into(),
            checked: f.widget_items.contains(&r.kind),
            style: r.widget_style,
            is_cpu: r.kind == ItemKind::Cpu,
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
    choices.saved_gpus.clone_from(&f.gpus);
    choices.saved_adapters.clone_from(&f.adapters);
    ui.set_gpu_rows(device_rows(&choices.gpus, &f.gpus, true));
    ui.set_per_gpu(!f.gpus.is_empty());
    ui.set_adapter_rows(device_rows(&choices.adapters, &f.adapters, false));
    ui.set_per_adapter(!f.adapters.is_empty());
    ui.set_network_floor_bits(f.network_floor_bits);
    ui.set_min_down(f.min_down);
    ui.set_min_down_unit(f.min_down_unit);
    ui.set_min_up(f.min_up);
    ui.set_min_up_unit(f.min_up_unit);
    let (names, values, index) = monitor_picker(
        tr!("Primary monitor"),
        &choices.monitors,
        &f.taskbar_monitor,
    );
    ui.set_taskbar_monitors(strings(names));
    ui.set_taskbar_monitor_index(index);
    choices.taskbar_values = values;
    ui.set_fit_to_taskbar(f.fit_to_taskbar);
    ui.set_fit_remove_from(f.fit_remove_from);
    ui.set_fit_no_room(f.fit_no_room);
    ui.set_fit_other_side(f.fit_other_side);
    let (names, values, index) =
        monitor_picker(tr!("The app in front"), &choices.monitors, &f.frame_monitor);
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
    ui.set_text_item_gap(f.text_item_gap);
    ui.set_text_label_gap(f.text_label_gap);
    ui.set_text_row_gap(f.text_row_gap);
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
                id: item_id(r.kind).into(),
                expanded: open.contains(&name),
                name,
                enabled: r.enabled,
                color: r.color.as_str().into(),
                swatch: rgb(form::item_rgb(r.kind, &r.color, choices.theme())),
                style: r.style,
                style_set: names::style_set(r.kind) as i32,
                trend: r.trend,
                color_mode: r.color_mode,
                popup: r.popup,
                popup_width: r.popup_width,
                popup_graph_height: r.popup_graph_height,
                tile_width: r.tile_width,
                tile_height: r.tile_height,
                label: r.label.as_str().into(),
                short_label: r.short_label.as_str().into(),
                default_label: r.kind.label().into(),
                default_short_label: r.kind.short_label().into(),
            }
        })
        .collect();
    ui.set_items(ModelRc::new(VecModel::from(rows)));
    ui.set_load_low_color(f.load_colors[0].as_str().into());
    ui.set_load_moderate_color(f.load_colors[1].as_str().into());
    ui.set_load_high_color(f.load_colors[2].as_str().into());
    ui.set_load_critical_color(f.load_colors[3].as_str().into());
    refresh_swatches(ui);
    ui.set_ping_host(f.ping_host.as_str().into());
    ui.set_ping_interval_ms(f.ping_interval_ms);
    ui.set_alert_fahrenheit(f.alert_fahrenheit);
    ui.set_load_percent_moderate(f.load_percent[0]);
    ui.set_load_percent_high(f.load_percent[1]);
    ui.set_load_percent_critical(f.load_percent[2]);
    ui.set_load_temperature_moderate(f.load_temperature[0]);
    ui.set_load_temperature_high(f.load_temperature[1]);
    ui.set_load_temperature_critical(f.load_temperature[2]);
    ui.set_load_ping_moderate(f.load_ping[0]);
    ui.set_load_ping_high(f.load_ping[1]);
    ui.set_load_ping_critical(f.load_ping[2]);
    ui.set_alert_cpu(f.alert_cpu);
    ui.set_hotkey_show_hide(f.hotkey_show_hide.as_str().into());
    ui.set_hotkey_widget(f.hotkey_widget.as_str().into());
    ui.set_hotkey_details(f.hotkey_details.as_str().into());
    ui.set_hotkey_record(f.hotkey_record.as_str().into());
    ui.set_hotkey_next_profile(f.hotkey_next_profile.as_str().into());
    ui.set_alert_cpu_limit(f.alert_cpu_limit);
    ui.set_alert_gpu(f.alert_gpu);
    ui.set_alert_gpu_limit(f.alert_gpu_limit);
    ui.set_alert_battery(f.alert_battery);
    ui.set_alert_battery_limit(f.alert_battery_limit);
    ui.set_alert_drives(f.alert_drives);
    ui.set_alert_drives_limit(f.alert_drives_limit);
}

fn pull(ui: &SettingsWindow, kinds: &[ItemKind], choices: &Choices) -> Form {
    let widget_rows: Vec<WidgetItemRow> = ui.get_widget_items().iter().collect();
    let widget_row = |kind: ItemKind| {
        widget_rows
            .iter()
            .find(|r| r.id.as_str() == item_id(kind).as_str())
            .cloned()
    };
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
        battery_interval_on: ui.get_battery_interval_on(),
        battery_interval_ms: ui.get_battery_interval_ms(),
        history_len: ui.get_history_len(),
        text_mode: ui.get_mode() == 1,
        graph_inline: ui.get_graph_inline(),
        vertical_labels: ui.get_vertical_labels(),
        hover_popup: ui.get_hover_popup(),
        position: ui.get_position(),
        lock_position: ui.get_lock_position(),
        hide_in_fullscreen: ui.get_hide_in_fullscreen(),
        attach_to_taskbar: ui.get_attach_to_taskbar(),
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
        text_item_gap: ui.get_text_item_gap(),
        text_label_gap: ui.get_text_label_gap(),
        text_row_gap: ui.get_text_row_gap(),
        show_on_all_taskbars: ui.get_show_on_all_taskbars(),
        taskbar_monitor: chosen(&choices.taskbar_values, ui.get_taskbar_monitor_index()),
        fit_to_taskbar: ui.get_fit_to_taskbar(),
        fit_remove_from: ui.get_fit_remove_from(),
        fit_no_room: ui.get_fit_no_room(),
        fit_other_side: ui.get_fit_other_side(),
        accent_graphs: ui.get_accent_graphs(),
        tile_color: ui.get_tile_color().to_string(),
        tile_opacity_custom: ui.get_tile_opacity_custom(),
        tile_opacity: ui.get_tile_opacity().round() as i32,
        panel_color: ui.get_panel_color().to_string(),
        panel_opacity_custom: ui.get_panel_opacity_custom(),
        panel_opacity: ui.get_panel_opacity().round() as i32,
        theme: choices.theme_id.clone(),
        items: ui
            .get_items()
            .iter()
            .zip(kinds)
            .map(|(r, &kind)| (r, kind, widget_row(kind)))
            .map(|(r, kind, w)| form::ItemRow {
                kind,
                enabled: r.enabled,
                color: r.color.to_string(),
                style: r.style,
                color_mode: r.color_mode,
                popup: r.popup,
                popup_width: r.popup_width,
                popup_graph_height: r.popup_graph_height,
                trend: r.trend,
                tile_width: r.tile_width,
                tile_height: r.tile_height,
                label: r.label.to_string(),
                short_label: r.short_label.to_string(),
                widget_style: w.as_ref().map_or(0, |w| w.style),
                widget_detail: w.as_ref().map_or(0, |w| w.detail),
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
        load_colors: [
            ui.get_load_low_color().to_string(),
            ui.get_load_moderate_color().to_string(),
            ui.get_load_high_color().to_string(),
            ui.get_load_critical_color().to_string(),
        ],
        load_percent: [
            ui.get_load_percent_moderate(),
            ui.get_load_percent_high(),
            ui.get_load_percent_critical(),
        ],
        load_temperature: [
            ui.get_load_temperature_moderate(),
            ui.get_load_temperature_high(),
            ui.get_load_temperature_critical(),
        ],
        load_ping: [
            ui.get_load_ping_moderate(),
            ui.get_load_ping_high(),
            ui.get_load_ping_critical(),
        ],
        hotkey_show_hide: ui.get_hotkey_show_hide().to_string(),
        hotkey_widget: ui.get_hotkey_widget().to_string(),
        hotkey_details: ui.get_hotkey_details().to_string(),
        hotkey_record: ui.get_hotkey_record().to_string(),
        hotkey_next_profile: ui.get_hotkey_next_profile().to_string(),
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
        gpus: ticked(ui.get_per_gpu(), &ui.get_gpu_rows(), &choices.saved_gpus),
        adapters: ticked(
            ui.get_per_adapter(),
            &ui.get_adapter_rows(),
            &choices.saved_adapters,
        ),
        min_down: ui.get_min_down(),
        min_down_unit: ui.get_min_down_unit(),
        min_up: ui.get_min_up(),
        min_up_unit: ui.get_min_up_unit(),
        network_floor_bits: ui.get_network_floor_bits(),
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
        network_show: ui.get_network_show(),
        network_split_tiles: ui.get_network_split_tiles(),
        network_popup_show: ui.get_network_popup_show(),
        network_down_label: ui.get_network_down_label().to_string(),
        network_up_label: ui.get_network_up_label().to_string(),
        game_strip: ui.get_game_strip(),
        game_summary: ui.get_game_summary(),
        popup_detail: ui.get_popup_detail(),
        popup_graph: ui.get_popup_graph(),
        popup_translucent: ui.get_popup_translucent(),
        popup_processes: ui.get_popup_processes(),
        popup_width: ui.get_popup_width(),
        popup_graph_height: ui.get_popup_graph_height(),
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
        widget_items: kinds
            .iter()
            .copied()
            .filter(|&kind| widget_row(kind).is_some_and(|r| r.checked))
            .collect(),
        widget_order: if ui.get_widget_order_custom() {
            widget_rows
                .iter()
                .filter_map(|r| {
                    kinds
                        .iter()
                        .copied()
                        .find(|&k| item_id(k).as_str() == r.id.as_str())
                })
                .collect()
        } else {
            Vec::new()
        },
        widget_background: ui.get_widget_background().to_string(),
    }
}

/// A preview of a style for a 64 × 34 box; empty for text only.
fn style_thumb(style: Option<GraphStyle>) -> &'static str {
    match style {
        Some(GraphStyle::Area) => "M0 30 L10 20 L20 24 L32 8 L44 18 L54 10 L64 16 L64 34 L0 34 Z",
        Some(GraphStyle::Line) => "M0 28 L10 20 L20 24 L32 8 L44 18 L54 10 L64 16",
        Some(GraphStyle::Bars) => {
            "M3 34 L3 22 M11 34 L11 14 M19 34 L19 26 M27 34 L27 8 M35 34 L35 18 M43 34 L43 12 M51 34 L51 24 M59 34 L59 16"
        }
        Some(GraphStyle::Meter) => "M2 17 L42 17",
        Some(GraphStyle::Segments) => {
            "M2 17 L8 17 M12 17 L18 17 M22 17 L28 17 M32 17 L38 17 M42 17 L48 17 M52 17 L58 17"
        }
        Some(GraphStyle::Spark) => "M2 26 L12 22 L22 25 L32 20 L42 23 L52 19 L62 22",
        Some(GraphStyle::Ring) => "M32 4 A13 13 0 1 1 19 17",
        Some(GraphStyle::Cores) => {
            "M4 32 L4 18 M12 32 L12 8 M20 32 L20 24 M28 32 L28 14 M36 32 L36 6 M44 32 L44 22 M52 32 L52 12 M60 32 L60 26"
        }
        Some(GraphStyle::Heat) => {
            "M4 6 L12 6 M18 6 L26 6 M32 6 L40 6 M46 6 L54 6 M4 17 L12 17 M18 17 L26 17 M32 17 L40 17 M46 17 L54 17 M4 28 L12 28 M18 28 L26 28 M32 28 L40 28 M46 28 L54 28"
        }
        Some(GraphStyle::HeatStrip) => {
            "M4 11 L4 23 M11 11 L11 23 M18 11 L18 23 M25 11 L25 23 M32 11 L32 23 M39 11 L39 23 M46 11 L46 23 M53 11 L53 23 M60 11 L60 23"
        }
        Some(GraphStyle::DotMatrix) => {
            "M6 28 L6 31 M6 22 L6 25 M16 28 L16 31 M16 22 L16 25 M16 16 L16 19 M16 10 L16 13 M26 28 L26 31 M36 28 L36 31 M36 22 L36 25 M36 16 L36 19 M46 28 L46 31 M46 22 L46 25 M46 16 L46 19 M46 10 L46 13 M46 4 L46 7 M56 28 L56 31 M56 22 L56 25"
        }
        Some(GraphStyle::Bullet) => {
            "M2 12 L62 12 L62 22 L2 22 Z M2 12 L38 12 L38 22 L2 22 Z M50 7 L53 7 L53 27 L50 27 Z"
        }
        Some(GraphStyle::RangeRail) => {
            "M2 16 L62 16 L62 18 L2 18 Z M16 12 L48 12 L48 22 L16 22 Z M38 8 L40 8 L40 26 L38 26 Z"
        }
        Some(GraphStyle::Tint) => "M2 2 L62 2 L62 32 L2 32 Z",
        Some(GraphStyle::Bubble) => {
            "M8 11 L34 11 M8 23 L28 23 M44 17 A8 8 0 1 1 60 17 A8 8 0 1 1 44 17 Z"
        }
        Some(GraphStyle::Uptime) => {
            "M4 12 L4 22 M11 12 L11 22 M18 12 L18 22 M25 5 L25 29 M32 12 L32 22 M39 12 L39 22 M46 5 L46 29 M53 12 L53 22 M60 12 L60 22"
        }
        Some(GraphStyle::BatteryCell) => {
            "M6 9 L52 9 L52 25 L6 25 Z M6 9 L34 9 L34 25 L6 25 Z M54 14 L58 14 L58 20 L54 20 Z"
        }
        Some(GraphStyle::Combined) => {
            "M4 32 L4 18 L12 18 L12 26 L20 26 L20 12 L28 12 L28 22 L36 22 L36 16 L44 16 L44 32 Z M50 4 A6 6 0 1 1 62 4 A6 6 0 1 1 50 4 Z"
        }
        Some(GraphStyle::CoreTypes) => {
            "M2 7 L62 7 L62 15 L2 15 Z M2 7 L44 7 L44 15 L2 15 Z M2 19 L62 19 L62 27 L2 27 Z M2 19 L20 19 L20 27 L2 27 Z"
        }
        Some(GraphStyle::MirrorBars) => {
            "M6 16 L6 8 M6 18 L6 24 M16 16 L16 4 M16 18 L16 26 M26 16 L26 10 M26 18 L26 22 M36 16 L36 2 M36 18 L36 30 M46 16 L46 7 M46 18 L46 25 M56 16 L56 12 M56 18 L56 23"
        }
        None => "",
    }
}

/// How a style's preview is stroked, and whether it is filled.
fn style_stroke(style: Option<GraphStyle>) -> (f32, bool) {
    match style {
        Some(GraphStyle::Area) => (1.5, true),
        Some(GraphStyle::Line | GraphStyle::Spark) => (1.5, false),
        Some(GraphStyle::Bars) => (5.0, false),
        Some(GraphStyle::Meter) => (8.0, false),
        Some(GraphStyle::Segments) => (6.0, false),
        Some(GraphStyle::Ring) => (4.0, false),
        Some(GraphStyle::Cores) => (5.0, false),
        Some(GraphStyle::Heat) => (8.0, false),
        Some(GraphStyle::HeatStrip) => (4.0, false),
        Some(GraphStyle::DotMatrix) => (4.0, false),
        Some(GraphStyle::Bullet | GraphStyle::RangeRail | GraphStyle::Tint) => (1.0, true),
        Some(GraphStyle::Bubble) => (2.0, true),
        Some(GraphStyle::MirrorBars | GraphStyle::Uptime) => (4.0, false),
        Some(GraphStyle::BatteryCell | GraphStyle::Combined | GraphStyle::CoreTypes) => (1.0, true),
        None => (0.0, false),
    }
}

fn style_sets() -> slint::ModelRc<slint::ModelRc<StyleChoice>> {
    let sets: Vec<slint::ModelRc<StyleChoice>> = names::STYLE_SETS
        .iter()
        .map(|set| {
            let choices: Vec<StyleChoice> = set
                .iter()
                .map(|&style| {
                    let (stroke, filled) = style_stroke(style);
                    StyleChoice {
                        name: names::style_name(style).into(),
                        thumb: style_thumb(style).into(),
                        stroke,
                        filled,
                    }
                })
                .collect();
            slint::ModelRc::new(slint::VecModel::from(choices))
        })
        .collect();
    slint::ModelRc::new(slint::VecModel::from(sets))
}

fn widget_style_names(kind: ItemKind) -> slint::ModelRc<SharedString> {
    strings(
        std::iter::once(tr!("Widget style").into())
            .chain(
                names::widget_styles_for(kind)
                    .iter()
                    .map(|&s| names::widget_style_name(s).into()),
            )
            .collect(),
    )
}

/// The text the window gets from here rather than from its `.slint` file.
fn localize(ui: &SettingsWindow) {
    ui.set_window_title(ipc::settings_title().into());
    ui.set_ui_font(language::ui_font().into());
    ui.set_style_sets(style_sets());
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
    ui.set_cpu_widget_item_styles(widget_style_names(ItemKind::Cpu));
    ui.set_widget_item_styles(widget_style_names(ItemKind::Ram));
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
    refresh_search(ui);
}

fn item_id(kind: ItemKind) -> String {
    format!("item/{kind:?}")
}

fn search_entries(ui: &SettingsWindow) -> Vec<search::Entry> {
    let items: Vec<search::Item> = ItemKind::ALL
        .into_iter()
        .map(|kind| search::Item {
            id: item_id(kind),
            name: names::item_msgid(kind),
        })
        .collect();
    search::entries(&items, ui.get_alert_fahrenheit(), l10n::translate)
}

fn refresh_search(ui: &SettingsWindow) {
    let entries = search_entries(ui);
    let results: Vec<SearchResult> = search::matches(&entries, &ui.get_search_text())
        .into_iter()
        .map(|entry| SearchResult {
            id: entry.id.as_str().into(),
            title: entry.title.into(),
            path: entry.path.as_str().into(),
            description: entry.description.into(),
        })
        .collect();
    ui.set_search_results(ModelRc::new(VecModel::from(results)));
}

fn open_search_result(ui: &SettingsWindow, kinds: &[ItemKind], id: &str) {
    let entries = search_entries(ui);
    let Some(entry) = entries.iter().find(|e| e.id == id) else {
        return;
    };
    let row = entry
        .owner
        .as_ref()
        .and_then(|owner| kinds.iter().position(|&kind| item_id(kind) == *owner));
    if let Some(row) = row {
        update_row(ui, row as i32, |r| r.expanded = true);
    }
    let (title, key) = match &entry.owner {
        Some(_) => ("", id),
        None if entry.is_row => (entry.title, ""),
        None => ("", ""),
    };
    let jump = ui.global::<SearchJump>();
    jump.set_tab(entry.tab);
    jump.set_title(title.into());
    jump.set_key(key.into());
    jump.set_missing(false);
    jump.set_pending(entry.is_row);
    ui.set_tab(entry.tab);
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
    push_profiles(ui, state);
}

/// Lists the saved profiles, marking those the settings match.
fn push_profiles(ui: &SettingsWindow, state: &mut State) {
    let dir = profile::profiles_dir();
    let entries = dir.as_deref().map(profile::list).unwrap_or_default();
    let rows: Vec<ProfileRow> = entries
        .iter()
        .map(|e| ProfileRow {
            name: e.name.as_str().into(),
            active: dir
                .as_deref()
                .and_then(|d| profile::load(d, &e.id))
                .is_some_and(|p| p.matches(&state.config)),
        })
        .collect();
    state.profile_ids = entries.into_iter().map(|e| e.id).collect();
    ui.set_profiles(ModelRc::new(VecModel::from(rows)));
}

/// Picks up a change made by another process. A file that does not parse
/// (e.g. caught mid-write, or a hand edit in progress) is left alone and
/// retried on the next tick; unchanged contents leave the form untouched.
fn pick_up_external_change(ui: &SettingsWindow, state: &mut State) {
    let stamp = modified(&state.path);
    let Ok(mut config) = config::read(&state.path) else {
        return;
    };
    config.general.autostart = autostart::get(autostart::VALUE_NAME).is_some();
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
    let units = state.config.units;
    let new = form::to_config(&pull(ui, &state.kinds, &state.choices), &state.config);
    let merged = write(state, new, true);
    let choices = &mut state.choices;
    choices.saved_gpus.clone_from(&state.config.gpu.adapters);
    choices
        .saved_adapters
        .clone_from(&state.config.network.adapters);
    refresh_after_write(ui, state, merged || state.config.units != units);
}

fn refresh_after_write(ui: &SettingsWindow, state: &mut State, stale: bool) {
    if stale || follow_language(ui, &state.config) {
        let config = state.config.clone();
        show(ui, state, config);
    } else {
        push_profiles(ui, state);
    }
}

fn write(state: &mut State, mut new: Config, merge: bool) -> bool {
    let mut merged = false;
    if merge
        && modified(&state.path) != state.written
        && let Ok(theirs) = config::read(&state.path)
        && theirs != state.config
    {
        let autostart = new.general.autostart;
        new = config::merge(&state.config, &new, &theirs);
        new.general.autostart = autostart;
        merged = true;
    }
    if merge && !merged && new == state.config {
        return false;
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
    merged
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

/// Blocks for as long as the helper takes, so it belongs on a worker thread.
fn check_for_update() -> Availability {
    let outcome = match std::env::current_exe() {
        Ok(exe) => update::run_helper(&ipc::sibling(&exe, ipc::APP_EXE)),
        Err(e) => Outcome::Failed(e.to_string()),
    };
    if let Some(path) = update::state_path() {
        let (_, saved) = UpdateState::change_saved(&path, |state| {
            update::note_manual_check(state, &outcome, update::current(), update::now());
        });
        if let Err(e) = saved {
            log::warn!("could not save the update check state: {e}");
        }
    }
    let availability = update::assess(&outcome, update::current());
    if let Availability::Failed(reason) = &availability {
        log::warn!("update check failed: {reason}");
    }
    availability
}

fn show_update_result(ui: &SettingsWindow, availability: Availability) {
    let (status, url, notes) = match availability {
        Availability::Available(release) => (
            tr!("Pulsar {} is available", release.version),
            release.url,
            release.notes,
        ),
        Availability::UpToDate => (
            tr!("You have the latest version.").to_string(),
            String::new(),
            String::new(),
        ),
        Availability::NoRelease => (
            tr!("No release has been published yet.").to_string(),
            String::new(),
            String::new(),
        ),
        Availability::Failed(reason) => (
            tr!("Could not check for updates: {}", reason),
            String::new(),
            String::new(),
        ),
    };
    ui.set_update_status(status.into());
    ui.set_update_url(url.into());
    ui.set_update_notes(notes.into());
    ui.set_update_checking(false);
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
    let profiles = std::env::args().any(|a| a == ipc::PROFILES_ARG);
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
        if profiles {
            notify::signal(ipc::SHOW_PROFILES_EVENT);
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
    let profile_requests = notify::Request::create(ipc::SHOW_PROFILES_EVENT);
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
    ui.on_check_for_update({
        let weak = ui.as_weak();
        move || {
            let Some(ui) = weak.upgrade() else { return };
            if ui.get_update_checking() {
                return;
            }
            ui.set_update_checking(true);
            ui.set_update_status(tr!("Checking for updates…").into());
            ui.set_update_url(SharedString::new());
            ui.set_update_notes(SharedString::new());
            let weak = weak.clone();
            std::thread::spawn(move || {
                let availability = check_for_update();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(ui) = weak.upgrade() {
                        show_update_result(&ui, availability);
                    }
                });
            });
        }
    });
    init_color_pickers(&ui);
    if about {
        ui.set_tab(ABOUT_TAB);
    } else if profiles {
        ui.set_tab(PROFILES_TAB);
    }

    let state = Rc::new(RefCell::new(State {
        path,
        config: Config::default(),
        kinds: Vec::new(),
        written: None,
        profile_ids: Vec::new(),
        choices: Choices {
            fonts: fonts::system_families(),
            gpus: pulsar_core::hardware::gpu::names(),
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
                    3 => r.popup_width = value,
                    4 => r.popup_graph_height = value,
                    5 => r.tile_width = value,
                    6 => r.tile_height = value,
                    7 => r.trend = value != 0,
                    _ => r.popup = value,
                });
                ui.invoke_changed();
            }
        }
    });
    ui.on_limit_label(|text| config::typed_label(&text).into());
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
    ui.on_drop_item({
        let (weak, state) = (ui.as_weak(), state.clone());
        move |from, target, after| {
            let Some(ui) = weak.upgrade() else { return };
            let mut s = state.borrow_mut();
            let mut f = pull(&ui, &s.kinds, &s.choices);
            pulsar_core::reorder::reposition(
                &mut f.items,
                from.max(0) as usize,
                target.max(0) as usize,
                after,
            );
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
            notify::post_to_class(ipc::HOST_CLASS, ipc::CONFIG_CHANGED);
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
    ui.on_save_profile({
        let (weak, state, pending) = (ui.as_weak(), state.clone(), save_timer.clone());
        move |name| {
            let Some(ui) = weak.upgrade() else { return };
            let Some(dir) = profile::profiles_dir() else {
                return;
            };
            pending.stop();
            let mut s = state.borrow_mut();
            save(&ui, &mut s);
            let current = form::to_config(&pull(&ui, &s.kinds, &s.choices), &s.config);
            let new = Profile::from_config(&name, &current);
            if new.name.is_empty() {
                return;
            }
            let status = match profile::save(&dir, &new) {
                Ok(_) => tr!("Saved.").to_string(),
                Err(e) => {
                    log::warn!("could not save profile: {e}");
                    tr!("Could not save the profile: {}", e)
                }
            };
            ui.set_profile_status(status.into());
            push_profiles(&ui, &mut s);
        }
    });
    ui.on_apply_profile({
        let (weak, state, pending) = (ui.as_weak(), state.clone(), save_timer.clone());
        move |index| {
            let Some(ui) = weak.upgrade() else { return };
            let Some(dir) = profile::profiles_dir() else {
                return;
            };
            pending.stop();
            let mut s = state.borrow_mut();
            let id = s.profile_ids.get(index.max(0) as usize).cloned();
            save(&ui, &mut s);
            let Some(chosen) = id.and_then(|id| profile::load(&dir, &id)) else {
                ui.set_profile_status(tr!("Could not read the profile.").into());
                return;
            };
            let new = chosen.applied_to(&s.config);
            write(&mut s, new, true);
            let config = s.config.clone();
            show(&ui, &mut s, config);
            ui.set_profile_status(SharedString::new());
        }
    });
    ui.on_delete_profile({
        let (weak, state) = (ui.as_weak(), state.clone());
        move |index| {
            let Some(ui) = weak.upgrade() else { return };
            let Some(dir) = profile::profiles_dir() else {
                return;
            };
            let mut s = state.borrow_mut();
            let Some(id) = s.profile_ids.get(index.max(0) as usize) else {
                return;
            };
            if let Err(e) = profile::delete(&dir, id) {
                log::warn!("could not delete profile {id}: {e}");
            }
            ui.set_profile_status(SharedString::new());
            push_profiles(&ui, &mut s);
        }
    });
    ui.on_open_profiles_folder(|| {
        let Some(dir) = profile::profiles_dir() else {
            return;
        };
        if let Err(e) = std::fs::create_dir_all(&dir) {
            log::warn!("could not create {}: {e}", dir.display());
            return;
        }
        let _ = std::process::Command::new("explorer").arg(dir).spawn();
    });
    ui.on_reset_all({
        let (weak, state, pending) = (ui.as_weak(), state.clone(), save_timer.clone());
        move || {
            let Some(ui) = weak.upgrade() else { return };
            pending.stop();
            let mut s = state.borrow_mut();
            let mut new = Config::default();
            new.general.autostart = s.config.general.autostart;
            new.general.language = s.config.general.language.clone();
            write(&mut s, new, false);
            let config = s.config.clone();
            show(&ui, &mut s, config);
        }
    });
    ui.on_reset_position({
        let (weak, state, pending) = (ui.as_weak(), state.clone(), save_timer.clone());
        move || {
            let Some(ui) = weak.upgrade() else { return };
            pending.stop();
            let mut s = state.borrow_mut();
            save(&ui, &mut s);
            let mut new = config::read(&s.path).unwrap_or_else(|_| s.config.clone());
            new.general.autostart = s.config.general.autostart;
            new.display.float_position = None;
            write(&mut s, new, false);
            refresh_after_write(&ui, &mut s, true);
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
                2 => ui.set_hotkey_details(value),
                3 => ui.set_hotkey_record(value),
                _ => ui.set_hotkey_next_profile(value),
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
    ui.on_drop_widget_item({
        let (weak, state) = (ui.as_weak(), state.clone());
        move |from, target, after| {
            let Some(ui) = weak.upgrade() else { return };
            let mut rows: Vec<WidgetItemRow> = ui.get_widget_items().iter().collect();
            pulsar_core::reorder::reposition(
                &mut rows,
                from.max(0) as usize,
                target.max(0) as usize,
                after,
            );
            // Dropped back into the taskbar's order, it follows the taskbar again.
            let custom = !rows.iter().map(|r| r.id.to_string()).eq(state
                .borrow()
                .kinds
                .iter()
                .map(|&k| item_id(k)));
            ui.set_widget_items(ModelRc::new(VecModel::from(rows)));
            ui.set_widget_order_custom(custom);
            ui.invoke_changed();
        }
    });
    ui.on_widget_item_toggled({
        let weak = ui.as_weak();
        move |index, checked| {
            let Some(ui) = weak.upgrade() else { return };
            let rows = ui.get_widget_items();
            if !checked && rows.iter().filter(|r| r.checked).count() <= 1 {
                let same: Vec<_> = rows.iter().collect();
                ui.set_widget_items(ModelRc::new(VecModel::from(same)));
                return;
            }
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
            let ticked = rows.iter().filter(|r| r.checked).count();
            if (!checked && ticked <= 1) || (checked && ticked >= config::MAX_DRIVES) {
                let same: Vec<_> = rows.iter().collect();
                ui.set_drive_rows(ModelRc::new(VecModel::from(same)));
                return;
            }
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
    ui.on_gpu_toggled({
        let weak = ui.as_weak();
        move |index, checked| {
            let Some(ui) = weak.upgrade() else { return };
            match device_toggled(ui.get_gpu_rows(), index, checked, config::MAX_GPUS) {
                Some(same) => ui.set_gpu_rows(same),
                None => ui.invoke_changed(),
            }
        }
    });
    ui.on_adapter_toggled({
        let weak = ui.as_weak();
        move |index, checked| {
            let Some(ui) = weak.upgrade() else { return };
            match device_toggled(ui.get_adapter_rows(), index, checked, config::MAX_ADAPTERS) {
                Some(same) => ui.set_adapter_rows(same),
                None => ui.invoke_changed(),
            }
        }
    });
    ui.on_per_gpu_selected({
        let weak = ui.as_weak();
        move || {
            let Some(ui) = weak.upgrade() else { return };
            if ui.get_per_gpu() {
                tick_first(&ui.get_gpu_rows());
            }
            ui.invoke_changed();
        }
    });
    ui.on_per_adapter_selected({
        let weak = ui.as_weak();
        move || {
            let Some(ui) = weak.upgrade() else { return };
            if ui.get_per_adapter() {
                tick_first(&ui.get_adapter_rows());
            }
            ui.invoke_changed();
        }
    });
    ui.on_search_edited({
        let (weak, tab_before) = (ui.as_weak(), Cell::new(ui.get_tab()));
        move |text| {
            let Some(ui) = weak.upgrade() else { return };
            let tab = ui.get_tab();
            if text.trim().is_empty() {
                if tab == RESULTS_TAB {
                    ui.set_tab(tab_before.get());
                }
            } else if tab != RESULTS_TAB {
                tab_before.set(tab);
                ui.set_tab(RESULTS_TAB);
            }
            refresh_search(&ui);
        }
    });
    ui.on_search_open({
        let (weak, state) = (ui.as_weak(), state.clone());
        move |id| {
            if let Some(ui) = weak.upgrade() {
                open_search_result(&ui, &state.borrow().kinds, &id);
            }
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
    // The window appears a few frames after the event loop starts; color its
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
            if profile_requests.as_ref().is_some_and(notify::Request::take) {
                ui.set_tab(PROFILES_TAB);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ticked_keeps_the_saved_order_of_adapters() {
        let row = |name: &str, checked| DeviceRow {
            label: name.into(),
            value: name.into(),
            checked,
        };
        let rows = ModelRc::new(VecModel::from(vec![
            row("eth", true),
            row("vpn", false),
            row("wifi", true),
        ]));
        let saved = ["wifi".to_string(), "eth".to_string()];
        assert_eq!(ticked(true, &rows, &saved), saved);
        assert!(ticked(false, &rows, &saved).is_empty());
    }
}
