//! The values shown in the settings window, and their mapping to `Config`.

use pulsar_core::colors::Rgb;
#[cfg(test)]
use pulsar_core::colors::default_rgb;
use pulsar_core::config::{
    Config, DisplayMode, DriveValue, ItemConfig, LabelStyle, NetworkShow, NoRoom, PopupBackground,
    Position, RateUnit, RemoveFrom, TemperatureUnit, WidgetLayer, WidgetStyle, sanitize_label,
};
use pulsar_core::layout::CellPart;
use pulsar_core::metric::ItemKind;
use pulsar_core::theme::{Colors, Theme};

use crate::names::{COLOR_MODES, DETAILS, STYLES, WIDGET_STYLES};

#[derive(Clone, Debug, PartialEq)]
pub struct ItemRow {
    pub kind: ItemKind,
    pub enabled: bool,
    /// `#RRGGBB`, or empty for the default color.
    pub color: String,
    /// An index into `names::STYLES`.
    pub style: i32,
    /// An index into `names::COLOR_MODES`.
    pub color_mode: i32,
    /// 0 follows the popup setting; otherwise one more than an index into
    /// `names::DETAILS`.
    pub popup: i32,
    /// 0 follows the popup setting.
    pub popup_width: i32,
    /// 0 follows the popup setting.
    pub popup_graph_height: i32,
    /// 0 is automatic.
    pub tile_width: i32,
    /// 0 is automatic.
    pub tile_height: i32,
    /// Empty uses the default.
    pub label: String,
    /// Empty uses the default.
    pub short_label: String,
    /// 0 follows the widget; otherwise one more than an index into
    /// `names::WIDGET_STYLES`.
    pub widget_style: i32,
    /// 0 follows the widget; otherwise one more than an index into
    /// `names::DETAILS`.
    pub widget_detail: i32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Form {
    pub autostart: bool,
    pub update_check: bool,
    pub tray_icon: bool,
    pub sample_interval_ms: i32,
    pub history_len: i32,
    pub text_mode: bool,
    pub graph_inline: bool,
    pub hover_popup: bool,
    /// 0 next to the tray, 1 left edge, 2 floating.
    pub position: i32,
    pub lock_position: bool,
    pub hide_in_fullscreen: bool,
    pub attach_to_taskbar: bool,
    /// 0 full, 1 short, 2 none (icons only).
    pub labels: i32,
    pub icons: bool,
    /// Empty for the theme color.
    pub label_color: String,
    pub value_color: String,
    /// Empty uses the colors above on a light taskbar too.
    pub label_color_light: String,
    pub value_color_light: String,
    pub color_labels: bool,
    /// Empty for the default font.
    pub font_family: String,
    pub font_bold: bool,
    pub offset_px: i32,
    pub fallback_margin_px: i32,
    pub font_size_pt: i32,
    pub text_item_gap: i32,
    pub text_label_gap: i32,
    pub text_row_gap: i32,
    pub show_on_all_taskbars: bool,
    /// A monitor's device path, or empty for the primary taskbar.
    pub taskbar_monitor: String,
    pub fit_to_taskbar: bool,
    /// 0 from the right, 1 from the left.
    pub fit_remove_from: i32,
    /// 0 float, 1 tray icon, 2 stay hidden.
    pub fit_no_room: i32,
    pub fit_other_side: bool,
    pub accent_graphs: bool,
    /// A theme id; empty for Windows default.
    pub theme: String,
    /// Empty for the theme tint.
    pub tile_color: String,
    /// Off means the theme's opacity; `tile_opacity` is then ignored.
    pub tile_opacity_custom: bool,
    pub tile_opacity: i32,
    /// Empty for the theme color.
    pub panel_color: String,
    /// Off means the theme's panel; `panel_opacity` is then ignored.
    pub panel_opacity_custom: bool,
    pub panel_opacity: i32,
    pub items: Vec<ItemRow>,
    pub ping_host: String,
    pub ping_interval_ms: i32,
    /// Letters with their own disk cell; empty shows all disks combined.
    pub drives: Vec<String>,
    pub drive_used: bool,
    /// GPUs with their own cells; empty shows all of them combined.
    pub gpus: Vec<String>,
    /// Adapters to count; empty sums every hardware adapter.
    pub adapters: Vec<String>,
    /// Network minimums as shown: an amount and an index into the units of
    /// `network_floor_bits`, the rate unit the form was filled in.
    pub min_down: i32,
    pub min_down_unit: i32,
    pub min_up: i32,
    pub min_up_unit: i32,
    pub network_floor_bits: bool,
    /// A monitor's device path, or empty for the foreground app's rate.
    pub frame_monitor: String,
    /// A language code, or empty to follow Windows.
    pub language: String,
    pub fahrenheit: bool,
    /// Alerts and their limits; temperatures in the unit the form was
    /// filled in, which `alert_fahrenheit` records.
    pub alert_cpu: bool,
    pub alert_cpu_limit: i32,
    pub alert_gpu: bool,
    pub alert_gpu_limit: i32,
    pub alert_battery: bool,
    pub alert_battery_limit: i32,
    pub alert_drives: bool,
    pub alert_drives_limit: i32,
    pub alert_fahrenheit: bool,
    /// Shortcuts as text; empty when not set.
    pub hotkey_show_hide: String,
    pub hotkey_widget: String,
    pub hotkey_details: String,
    pub hotkey_record: String,
    pub hotkey_next_profile: String,
    pub network_bits: bool,
    pub upload_on_top: bool,
    pub network_show: i32,
    pub network_split_tiles: bool,
    pub network_popup_show: i32,
    /// Empty uses the arrow.
    pub network_down_label: String,
    /// Empty uses the arrow.
    pub network_up_label: String,
    pub game_strip: bool,
    pub game_summary: bool,
    /// An index into `names::DETAILS`.
    pub popup_detail: i32,
    pub popup_graph: bool,
    pub popup_translucent: bool,
    pub popup_processes: i32,
    pub popup_width: i32,
    pub popup_graph_height: i32,
    pub widget_enabled: bool,
    pub widget_on_top: bool,
    /// 0 sparklines, 1 tiles, 2 meters.
    pub widget_style: i32,
    pub widget_lock: bool,
    pub widget_click_through: bool,
    /// An index into `names::DETAILS`.
    pub widget_detail: i32,
    pub widget_width: i32,
    pub widget_opacity: i32,
    pub widget_hide_in_fullscreen: bool,
    /// Off shows the taskbar's items.
    pub widget_own_items: bool,
    pub widget_items: Vec<ItemKind>,
}

/// `#RRGGBB` → `(r, g, b)`, for the color swatches; anything else is `None`.
pub fn parse_hex(s: &str) -> Option<Rgb> {
    let hex = s.trim().strip_prefix('#').filter(|h| h.len() == 6)?;
    let byte = |i: usize| u8::from_str_radix(hex.get(i..i + 2)?, 16).ok();
    Some((byte(0)?, byte(2)?, byte(4)?))
}

pub fn to_hex((r, g, b): Rgb) -> String {
    format!("#{r:02X}{g:02X}{b:02X}")
}

/// The color an item is drawn in: its valid custom color, else the default.
pub fn item_rgb(kind: ItemKind, hex: &str, theme: &Colors) -> Rgb {
    parse_hex(hex).unwrap_or_else(|| theme.item(kind, CellPart::Main))
}

/// Selects `theme` and clears the colors that would hide it.
pub fn apply_theme(f: &mut Form, theme: &Theme) {
    f.theme = theme.id.clone();
    for row in &mut f.items {
        row.color.clear();
    }
    for color in [
        &mut f.label_color,
        &mut f.value_color,
        &mut f.label_color_light,
        &mut f.value_color_light,
        &mut f.tile_color,
        &mut f.panel_color,
    ] {
        color.clear();
    }
    f.tile_opacity_custom = false;
    f.panel_opacity_custom = false;
    f.color_labels = theme.color_labels;
}

/// `base` with the form's custom colors for a dark taskbar folded in, to
/// save as a new theme.
pub fn theme_from(f: &Form, base: &Theme, name: &str) -> Theme {
    let mut dark = base.dark.clone();
    let pick = |hex: &str, fallback: Rgb| parse_hex(hex).unwrap_or(fallback);
    dark.text = pick(&f.value_color, dark.text);
    dark.label = pick(&f.label_color, dark.label);
    dark.tile = pick(&f.tile_color, dark.tile);
    dark.panel = pick(&f.panel_color, dark.panel);
    if f.tile_opacity_custom {
        dark.tile_opacity = clamp_u8(f.tile_opacity);
    }
    if f.panel_opacity_custom {
        dark.panel_opacity = clamp_u8(f.panel_opacity);
    }
    let mut light = base.light.clone();
    light.text = pick(&f.value_color_light, pick(&f.value_color, light.text));
    light.label = pick(&f.label_color_light, pick(&f.label_color, light.label));
    light.tile = pick(&f.tile_color, light.tile);
    light.panel = pick(&f.panel_color, light.panel);
    if f.tile_opacity_custom {
        light.tile_opacity = clamp_u8(f.tile_opacity);
    }
    if f.panel_opacity_custom {
        light.panel_opacity = clamp_u8(f.panel_opacity);
    }
    for row in &f.items {
        if let Some(c) = parse_hex(&row.color) {
            dark.set_item(row.kind, c);
            light.set_item(row.kind, c);
        }
    }
    Theme {
        id: String::new(),
        name: name.trim().to_string(),
        dark,
        light,
        color_labels: f.color_labels,
    }
}

const NETWORK_SHOW: [NetworkShow; 3] = [
    NetworkShow::Both,
    NetworkShow::Download,
    NetworkShow::Upload,
];

fn index_of<T: PartialEq>(all: &[T], value: &T) -> i32 {
    all.iter().position(|v| v == value).unwrap_or(0) as i32
}

fn at<T: Copy>(all: &[T], index: i32) -> Option<T> {
    all.get(usize::try_from(index).ok()?).copied()
}

pub fn to_form(c: &Config) -> Form {
    let floor_bits = c.units.network == RateUnit::Bits;
    let (min_down, min_down_unit) = shown_rate(c.network.min_down_bps, floor_bits);
    let (min_up, min_up_unit) = shown_rate(c.network.min_up_bps, floor_bits);
    let d = &c.display;
    let theme = pulsar_core::theme::load(
        pulsar_core::theme::themes_dir().as_deref(),
        d.theme.as_deref(),
    );
    Form {
        autostart: c.general.autostart,
        update_check: c.general.update_check,
        tray_icon: c.general.tray_icon,
        sample_interval_ms: c.general.sample_interval_ms as i32,
        history_len: c.general.history_len as i32,
        text_mode: d.mode == DisplayMode::Text,
        graph_inline: d.graph_inline,
        hover_popup: d.hover_popup,
        position: match d.position {
            Position::Right => 0,
            Position::Left => 1,
            Position::Floating => 2,
        },
        lock_position: d.lock_position,
        hide_in_fullscreen: d.hide_in_fullscreen,
        attach_to_taskbar: d.attach_to_taskbar,
        labels: match d.labels {
            LabelStyle::Full => 0,
            LabelStyle::Short => 1,
            LabelStyle::None => 2,
        },
        icons: d.icons,
        label_color: d.label_color.clone().unwrap_or_default(),
        value_color: d.value_color.clone().unwrap_or_default(),
        label_color_light: d.label_color_light.clone().unwrap_or_default(),
        value_color_light: d.value_color_light.clone().unwrap_or_default(),
        color_labels: d.color_labels,
        font_family: d.font_family.clone().unwrap_or_default(),
        font_bold: d.font_bold,
        offset_px: d.offset_px,
        fallback_margin_px: d.fallback_margin_px,
        font_size_pt: d.font_size_pt.round() as i32,
        text_item_gap: d.text_item_gap,
        text_label_gap: d.text_label_gap,
        text_row_gap: d.text_row_gap,
        show_on_all_taskbars: d.show_on_all_taskbars,
        taskbar_monitor: d.taskbar_monitor.clone().unwrap_or_default(),
        fit_to_taskbar: d.fit_to_taskbar,
        fit_remove_from: match d.fit_remove_from {
            RemoveFrom::Right => 0,
            RemoveFrom::Left => 1,
        },
        fit_no_room: match d.fit_no_room {
            NoRoom::Float => 0,
            NoRoom::Tray => 1,
            NoRoom::Hide => 2,
        },
        fit_other_side: d.fit_other_side,
        accent_graphs: d.accent_graphs,
        tile_color: d.tile_color.clone().unwrap_or_default(),
        tile_opacity_custom: d.tile_opacity.is_some(),
        tile_opacity: i32::from(d.tile_opacity.unwrap_or(theme.dark.tile_opacity)),
        theme: d.theme.clone().unwrap_or_default(),
        panel_color: d.panel_color.clone().unwrap_or_default(),
        panel_opacity_custom: d.panel_opacity.is_some(),
        panel_opacity: i32::from(d.panel_opacity.unwrap_or(theme.dark.panel_opacity)),
        items: c
            .items
            .iter()
            .map(|i| ItemRow {
                kind: i.kind,
                enabled: i.enabled,
                color: i.color.clone().unwrap_or_default(),
                style: index_of(&STYLES, &i.graph.then_some(i.style)),
                color_mode: index_of(&COLOR_MODES, &i.color_mode),
                popup: i.popup.map_or(0, |p| index_of(&DETAILS, &p) + 1),
                popup_width: i.popup_width.map_or(0, i32::from),
                popup_graph_height: i.popup_graph_height.map_or(0, i32::from),
                tile_width: i.tile_width.map_or(0, i32::from),
                tile_height: i.tile_height.map_or(0, i32::from),
                label: i.label.clone().unwrap_or_default(),
                short_label: i.short_label.clone().unwrap_or_default(),
                widget_style: i
                    .widget_style
                    .map_or(0, |s| index_of(&WIDGET_STYLES, &s) + 1),
                widget_detail: i.widget_detail.map_or(0, |d| index_of(&DETAILS, &d) + 1),
            })
            .collect(),
        ping_host: c.ping.host.clone(),
        ping_interval_ms: c.ping.interval_ms as i32,
        drives: c.disk.drives.clone(),
        drive_used: c.disk.value == DriveValue::Used,
        gpus: c.gpu.adapters.clone(),
        adapters: c.network.adapters.clone(),
        min_down,
        min_down_unit,
        min_up,
        min_up_unit,
        network_floor_bits: floor_bits,
        frame_monitor: c.frames.monitor.clone().unwrap_or_default(),
        language: c.general.language.clone().unwrap_or_default(),
        fahrenheit: c.units.temperature == TemperatureUnit::Fahrenheit,
        alert_cpu: c.alerts.cpu.on,
        alert_cpu_limit: shown_temp(c.alerts.cpu.limit, c),
        alert_gpu: c.alerts.gpu.on,
        alert_gpu_limit: shown_temp(c.alerts.gpu.limit, c),
        alert_battery: c.alerts.battery.on,
        alert_battery_limit: c.alerts.battery.limit as i32,
        alert_drives: c.alerts.drives.on,
        alert_drives_limit: c.alerts.drives.limit as i32,
        alert_fahrenheit: c.units.temperature == TemperatureUnit::Fahrenheit,
        hotkey_show_hide: c.hotkeys.show_hide.clone().unwrap_or_default(),
        hotkey_widget: c.hotkeys.widget.clone().unwrap_or_default(),
        hotkey_details: c.hotkeys.details.clone().unwrap_or_default(),
        hotkey_record: c.hotkeys.record.clone().unwrap_or_default(),
        hotkey_next_profile: c.hotkeys.next_profile.clone().unwrap_or_default(),
        network_bits: c.units.network == RateUnit::Bits,
        upload_on_top: d.upload_on_top,
        network_show: index_of(&NETWORK_SHOW, &c.network.show),
        network_split_tiles: c.network.split_tiles,
        network_popup_show: index_of(&NETWORK_SHOW, &c.network.popup_show),
        network_down_label: c.network.down_label.clone().unwrap_or_default(),
        network_up_label: c.network.up_label.clone().unwrap_or_default(),
        game_strip: c.game.strip,
        game_summary: c.game.summary,
        popup_detail: index_of(&DETAILS, &c.popup.detail),
        popup_graph: c.popup.graph,
        popup_translucent: c.popup.background == PopupBackground::Translucent,
        popup_processes: i32::from(c.popup.processes),
        popup_width: i32::from(c.popup.width),
        popup_graph_height: i32::from(c.popup.graph_height),
        widget_enabled: c.widget.enabled,
        widget_on_top: c.widget.layer == WidgetLayer::OnTop,
        widget_style: match c.widget.style {
            WidgetStyle::Sparklines => 0,
            WidgetStyle::Tiles => 1,
            WidgetStyle::Meters => 2,
        },
        widget_lock: c.widget.lock_position,
        widget_click_through: c.widget.click_through,
        widget_detail: index_of(&DETAILS, &c.widget.detail),
        widget_width: i32::from(c.widget.width),
        widget_opacity: i32::from(c.widget.opacity),
        widget_hide_in_fullscreen: c.widget.hide_in_fullscreen,
        widget_own_items: !c.widget.items.is_empty(),
        // Choosing items starts from the taskbar's.
        widget_items: if c.widget.items.is_empty() {
            c.enabled_items()
        } else {
            c.widget.items.clone()
        },
    }
}

/// Every setting back to its default, except autostart, which mirrors the
/// registry and is changed only by its own switch, and the language.
fn non_empty(s: &str) -> Option<String> {
    let s = s.trim();
    (!s.is_empty()).then(|| s.to_string())
}

fn clamp_u8(v: i32) -> u8 {
    v.clamp(0, 100) as u8
}

fn clamp_u16(v: i32) -> u16 {
    v.clamp(0, i32::from(u16::MAX)) as u16
}

/// Builds a sanitised config from the form, keeping anything the form does
/// not show (such as the file version) from `base`.
/// A temperature limit in the unit the user reads temperatures in.
fn shown_temp(celsius: u32, c: &Config) -> i32 {
    if c.units.temperature == TemperatureUnit::Fahrenheit {
        (f64::from(celsius) * 9.0 / 5.0 + 32.0).round() as i32
    } else {
        celsius as i32
    }
}

fn celsius(shown: i32, fahrenheit: bool) -> u32 {
    let c = if fahrenheit {
        ((f64::from(shown) - 32.0) * 5.0 / 9.0).round() as i32
    } else {
        shown
    };
    c.max(0) as u32
}

const MAX_SHOWN_RATE: u64 = 1_000_000_000;
const BYTE_RATE_UNITS: [u64; 3] = [1, 1024, 1024 * 1024];
const BIT_RATE_UNITS: [u64; 3] = [1, 1000, 1_000_000];

/// A rate in the largest unit that shows it exactly, as an amount and a unit
/// index. A rate too big or fine for that shows rounded in the largest unit.
fn shown_rate(bytes_per_sec: u64, bits: bool) -> (i32, i32) {
    let (amount, units) = if bits {
        (bytes_per_sec.saturating_mul(8), BIT_RATE_UNITS)
    } else {
        (bytes_per_sec, BYTE_RATE_UNITS)
    };
    if amount == 0 {
        return (0, 1);
    }
    let exact = (0..units.len())
        .rev()
        .find(|&i| amount % units[i] == 0 && amount / units[i] <= MAX_SHOWN_RATE);
    match exact {
        Some(i) => ((amount / units[i]) as i32, i as i32),
        None => {
            let last = units.len() - 1;
            let rounded = amount.saturating_add(units[last] / 2) / units[last];
            (rounded.min(MAX_SHOWN_RATE) as i32, last as i32)
        }
    }
}

/// Rounds up, so a nonzero amount of bits stays a nonzero minimum.
fn rate_from_form(amount: i32, unit: i32, bits: bool) -> u64 {
    let units = if bits {
        BIT_RATE_UNITS
    } else {
        BYTE_RATE_UNITS
    };
    let unit = units[unit.clamp(0, units.len() as i32 - 1) as usize];
    let total = amount.max(0) as u64 * unit;
    if bits { total.div_ceil(8) } else { total }
}

/// Keeps `base` while the form still shows it, as not every rate shows exactly.
fn rate_or_base(base: u64, amount: i32, unit: i32, bits: bool) -> u64 {
    if shown_rate(base, bits) == (amount, unit) {
        base
    } else {
        rate_from_form(amount, unit, bits)
    }
}

pub fn to_config(f: &Form, base: &Config) -> Config {
    let mut c = base.clone();
    c.alerts.cpu.on = f.alert_cpu;
    c.alerts.cpu.limit = celsius(f.alert_cpu_limit, f.alert_fahrenheit);
    c.alerts.gpu.on = f.alert_gpu;
    c.alerts.gpu.limit = celsius(f.alert_gpu_limit, f.alert_fahrenheit);
    c.alerts.battery.on = f.alert_battery;
    c.alerts.battery.limit = f.alert_battery_limit.max(0) as u32;
    c.alerts.drives.on = f.alert_drives;
    c.alerts.drives.limit = f.alert_drives_limit.max(0) as u32;
    c.hotkeys.show_hide = non_empty(&f.hotkey_show_hide);
    c.hotkeys.widget = non_empty(&f.hotkey_widget);
    c.hotkeys.details = non_empty(&f.hotkey_details);
    c.hotkeys.record = non_empty(&f.hotkey_record);
    c.hotkeys.next_profile = non_empty(&f.hotkey_next_profile);
    c.general.autostart = f.autostart;
    c.general.update_check = f.update_check;
    c.general.tray_icon = f.tray_icon;
    c.general.sample_interval_ms = f.sample_interval_ms.max(0) as u32;
    c.general.history_len = f.history_len.max(0) as usize;
    let d = &mut c.display;
    d.mode = if f.text_mode {
        DisplayMode::Text
    } else {
        DisplayMode::Graph
    };
    d.graph_inline = f.graph_inline;
    d.hover_popup = f.hover_popup;
    d.position = match f.position {
        1 => Position::Left,
        2 => Position::Floating,
        _ => Position::Right,
    };
    d.lock_position = f.lock_position;
    d.hide_in_fullscreen = f.hide_in_fullscreen;
    d.attach_to_taskbar = f.attach_to_taskbar;
    d.labels = match f.labels {
        1 => LabelStyle::Short,
        2 => LabelStyle::None,
        _ => LabelStyle::Full,
    };
    d.icons = f.icons;
    d.label_color = non_empty(&f.label_color);
    d.value_color = non_empty(&f.value_color);
    d.label_color_light = non_empty(&f.label_color_light);
    d.value_color_light = non_empty(&f.value_color_light);
    d.upload_on_top = f.upload_on_top;
    c.game.strip = f.game_strip;
    c.game.summary = f.game_summary;
    d.color_labels = f.color_labels;
    d.font_family = non_empty(&f.font_family);
    d.font_bold = f.font_bold;
    d.offset_px = f.offset_px;
    d.fallback_margin_px = f.fallback_margin_px;
    if f.font_size_pt != base.display.font_size_pt.round() as i32 {
        d.font_size_pt = f.font_size_pt as f32;
    }
    d.text_item_gap = f.text_item_gap;
    d.text_label_gap = f.text_label_gap;
    d.text_row_gap = f.text_row_gap;
    d.show_on_all_taskbars = f.show_on_all_taskbars;
    d.taskbar_monitor = non_empty(&f.taskbar_monitor);
    d.fit_to_taskbar = f.fit_to_taskbar;
    d.fit_remove_from = match f.fit_remove_from {
        1 => RemoveFrom::Left,
        _ => RemoveFrom::Right,
    };
    d.fit_no_room = match f.fit_no_room {
        1 => NoRoom::Tray,
        2 => NoRoom::Hide,
        _ => NoRoom::Float,
    };
    d.fit_other_side = f.fit_other_side;
    d.accent_graphs = f.accent_graphs;
    d.tile_color = non_empty(&f.tile_color);
    d.tile_opacity = f.tile_opacity_custom.then(|| clamp_u8(f.tile_opacity));
    d.panel_color = non_empty(&f.panel_color);
    d.panel_opacity = f.panel_opacity_custom.then(|| clamp_u8(f.panel_opacity));
    d.theme = non_empty(&f.theme);
    c.items = f
        .items
        .iter()
        .map(|r| {
            let style = at(&STYLES, r.style).unwrap_or(STYLES[0]);
            ItemConfig {
                color: non_empty(&r.color),
                graph: style.is_some(),
                // Text only keeps the graph style it had, for when the graph returns.
                style: style
                    .unwrap_or_else(|| base.item(r.kind).map(|i| i.style).unwrap_or_default()),
                color_mode: at(&COLOR_MODES, r.color_mode).unwrap_or_default(),
                popup: at(&DETAILS, r.popup - 1),
                popup_width: (r.popup_width > 0).then(|| clamp_u16(r.popup_width)),
                popup_graph_height: (r.popup_graph_height > 0)
                    .then(|| clamp_u16(r.popup_graph_height)),
                tile_width: (r.tile_width > 0).then(|| clamp_u16(r.tile_width)),
                tile_height: (r.tile_height > 0).then(|| clamp_u16(r.tile_height)),
                label: sanitize_label(&r.label),
                short_label: sanitize_label(&r.short_label),
                widget_style: at(&WIDGET_STYLES, r.widget_style - 1),
                widget_detail: at(&DETAILS, r.widget_detail - 1),
                ..ItemConfig::new(r.kind, r.enabled)
            }
        })
        .collect();
    c.ping.host = f.ping_host.trim().to_string();
    c.ping.interval_ms = f.ping_interval_ms.max(0) as u32;
    c.disk.drives = f.drives.clone();
    c.disk.value = if f.drive_used {
        DriveValue::Used
    } else {
        DriveValue::Activity
    };
    c.gpu.adapters = f.gpus.clone();
    c.network.adapters = f.adapters.clone();
    c.network.min_down_bps = rate_or_base(
        base.network.min_down_bps,
        f.min_down,
        f.min_down_unit,
        f.network_floor_bits,
    );
    c.network.min_up_bps = rate_or_base(
        base.network.min_up_bps,
        f.min_up,
        f.min_up_unit,
        f.network_floor_bits,
    );
    c.network.show = at(&NETWORK_SHOW, f.network_show).unwrap_or_default();
    c.network.split_tiles = f.network_split_tiles;
    c.network.popup_show = at(&NETWORK_SHOW, f.network_popup_show).unwrap_or_default();
    c.network.down_label = sanitize_label(&f.network_down_label);
    c.network.up_label = sanitize_label(&f.network_up_label);
    c.frames.monitor = non_empty(&f.frame_monitor);
    c.general.language = non_empty(&f.language);
    c.units.temperature = if f.fahrenheit {
        TemperatureUnit::Fahrenheit
    } else {
        TemperatureUnit::Celsius
    };
    c.units.network = if f.network_bits {
        RateUnit::Bits
    } else {
        RateUnit::Bytes
    };
    c.popup.detail = at(&DETAILS, f.popup_detail).unwrap_or_default();
    c.popup.graph = f.popup_graph;
    c.popup.background = if f.popup_translucent {
        PopupBackground::Translucent
    } else {
        PopupBackground::Solid
    };
    c.popup.processes = f.popup_processes.clamp(0, 255) as u8;
    c.popup.width = clamp_u16(f.popup_width);
    c.popup.graph_height = clamp_u16(f.popup_graph_height);
    let w = &mut c.widget;
    w.enabled = f.widget_enabled;
    w.layer = if f.widget_on_top {
        WidgetLayer::OnTop
    } else {
        WidgetLayer::Desktop
    };
    w.style = match f.widget_style {
        1 => WidgetStyle::Tiles,
        2 => WidgetStyle::Meters,
        _ => WidgetStyle::Sparklines,
    };
    w.lock_position = f.widget_lock;
    w.click_through = f.widget_click_through;
    w.detail = at(&DETAILS, f.widget_detail).unwrap_or_default();
    w.width = f.widget_width.clamp(0, i32::from(u16::MAX)) as u16;
    w.opacity = clamp_u8(f.widget_opacity);
    w.hide_in_fullscreen = f.widget_hide_in_fullscreen;
    w.items = if f.widget_own_items {
        f.widget_items.clone()
    } else {
        Vec::new()
    };
    c.sanitize()
}

/// Moves the row at `index` one place up or down; out-of-range moves do nothing.
pub fn move_item(items: &mut [ItemRow], index: usize, up: bool) {
    let target = if up {
        index.checked_sub(1)
    } else {
        index.checked_add(1)
    };
    if let Some(target) = target
        && index < items.len()
        && target < items.len()
    {
        items.swap(index, target);
    }
}

/// Saved devices in their saved order, then newly ticked ones: cells are
/// numbered by place in this list.
pub fn in_saved_order(saved: &[String], ticked: Vec<String>) -> Vec<String> {
    let mut ordered: Vec<String> = saved
        .iter()
        .filter(|name| ticked.contains(name))
        .cloned()
        .collect();
    ordered.extend(ticked.into_iter().filter(|name| !saved.contains(name)));
    ordered
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saved_devices_keep_their_order_whatever_order_the_rows_are_in() {
        let list = |names: &[&str]| names.iter().map(|n| n.to_string()).collect::<Vec<_>>();
        let saved = list(&["wifi", "gone", "eth"]);
        assert_eq!(
            in_saved_order(&saved, list(&["eth", "wifi", "gone"])),
            saved,
            "unchanged, so nothing renumbers and the sampler isn't restarted"
        );
        assert_eq!(
            in_saved_order(&saved, list(&["eth", "vpn", "wifi", "gone"])),
            list(&["wifi", "gone", "eth", "vpn"]),
            "a new tick goes last"
        );
        assert_eq!(
            in_saved_order(&saved, list(&["eth", "gone"])),
            list(&["gone", "eth"]),
            "an untick only closes the gap"
        );
        assert_eq!(in_saved_order(&[], list(&["b", "a"])), list(&["b", "a"]));
    }

    #[test]
    fn default_config_round_trips() {
        let c = Config::default();
        assert_eq!(to_config(&to_form(&c), &c), c);
    }

    #[test]
    fn theme_tile_opacity_is_not_written_until_customised() {
        let c = Config::default();
        let mut f = to_form(&c);
        assert!(!f.tile_opacity_custom);
        assert_eq!(f.tile_opacity, 8);
        f.tile_opacity = 40;
        assert_eq!(
            to_config(&f, &c).display.tile_opacity,
            None,
            "slider ignored while using the theme"
        );
        f.tile_opacity_custom = true;
        assert_eq!(to_config(&f, &c).display.tile_opacity, Some(40));
    }

    #[test]
    fn empty_colors_mean_default_and_invalid_ones_are_dropped() {
        let c = Config::default();
        let mut f = to_form(&c);
        f.tile_color = "  ".into();
        f.panel_color = "#20202".into();
        f.items[0].color = "#FF0000".into();
        let out = to_config(&f, &c);
        assert_eq!(out.display.tile_color, None);
        assert_eq!(
            out.display.panel_color, None,
            "incomplete hex is dropped by sanitize"
        );
        assert_eq!(out.items[0].color.as_deref(), Some("#FF0000"));
    }

    #[test]
    fn out_of_range_numbers_are_clamped() {
        let c = Config::default();
        let mut f = to_form(&c);
        f.sample_interval_ms = 10;
        f.history_len = -5;
        f.panel_opacity_custom = true;
        f.panel_opacity = 250;
        f.font_size_pt = 99;
        f.text_item_gap = 99;
        f.text_label_gap = -1;
        f.text_row_gap = 99;
        let out = to_config(&f, &c);
        assert_eq!(out.general.sample_interval_ms, 500);
        assert_eq!(out.general.history_len, 10);
        assert_eq!(out.display.panel_opacity, Some(100));
        assert_eq!(out.display.font_size_pt, 20.0);
        assert_eq!(out.display.text_item_gap, 40);
        assert_eq!(out.display.text_label_gap, 0);
        assert_eq!(out.display.text_row_gap, 16);
    }

    #[test]
    fn mode_and_position_map_both_ways() {
        let c = Config::default();
        let mut f = to_form(&c);
        f.text_mode = true;
        f.position = 1;
        let out = to_config(&f, &c);
        assert_eq!(out.display.mode, DisplayMode::Text);
        assert_eq!(out.display.position, Position::Left);
        assert!(to_form(&out).text_mode && to_form(&out).position == 1);
        f.position = 2;
        assert_eq!(to_config(&f, &c).display.position, Position::Floating);
    }

    #[test]
    fn enabling_ping_and_reordering_items() {
        let c = Config::default();
        let mut f = to_form(&c);
        let ping = f
            .items
            .iter()
            .position(|r| r.kind == ItemKind::Ping)
            .unwrap();
        f.items[ping].enabled = true;
        move_item(&mut f.items, ping, true);
        f.ping_host = " 8.8.8.8 ".into();
        let out = to_config(&f, &c);
        assert!(out.is_enabled(ItemKind::Ping));
        assert_eq!(out.items[ping - 1].kind, ItemKind::Ping);
        assert_eq!(out.ping.host, "8.8.8.8");
    }

    #[test]
    fn moves_at_the_ends_do_nothing() {
        let mut items = to_form(&Config::default()).items;
        let before = items.clone();
        move_item(&mut items, 0, true);
        let last = items.len() - 1;
        move_item(&mut items, last, false);
        move_item(&mut items, 99, true);
        assert_eq!(items, before);
    }

    #[test]
    fn parses_complete_hex_colors_only() {
        assert_eq!(parse_hex("#FF8000"), Some((255, 128, 0)));
        assert_eq!(parse_hex(" #ff8000 "), Some((255, 128, 0)));
        assert_eq!(parse_hex("#FF80"), None, "half-typed");
        assert_eq!(parse_hex("FF8000"), None);
        assert_eq!(parse_hex("#GG8000"), None);
        assert_eq!(parse_hex("#ÄÄÄÄÄÄ"), None, "non-ASCII never panics");
    }

    #[test]
    fn formats_hex_that_parses_back() {
        assert_eq!(to_hex((255, 128, 0)), "#FF8000");
        assert_eq!(to_hex((0, 1, 2)), "#000102");
        assert_eq!(parse_hex(&to_hex((18, 52, 86))), Some((18, 52, 86)));
    }

    #[test]
    fn item_swatch_falls_back_to_the_theme_color() {
        let windows = Theme::windows();
        assert_eq!(item_rgb(ItemKind::Cpu, "#010203", &windows.dark), (1, 2, 3));
        assert_eq!(
            item_rgb(ItemKind::Cpu, "", &windows.dark),
            default_rgb(ItemKind::Cpu, CellPart::Main)
        );
        let ocean = pulsar_core::theme::load(None, Some("ocean"));
        assert_eq!(
            item_rgb(ItemKind::Cpu, "#01", &ocean.dark),
            ocean.dark.item(ItemKind::Cpu, CellPart::Main)
        );
    }

    #[test]
    fn per_item_widget_choices_round_trip() {
        let c = Config::default();
        let mut f = to_form(&c);
        assert!(
            f.items
                .iter()
                .all(|r| r.widget_style == 0 && r.widget_detail == 0)
        );
        f.items[0].widget_style = 2;
        f.items[0].widget_detail = 3;
        let out = to_config(&f, &c);
        let kind = f.items[0].kind;
        assert_eq!(out.widget_style(kind), WIDGET_STYLES[1]);
        assert_eq!(out.widget_detail(kind), DETAILS[2]);
        assert_eq!(to_form(&out).items[0].widget_style, 2);
        f.items[0].widget_style = 0;
        assert_eq!(to_config(&f, &c).item(kind).unwrap().widget_style, None);
    }

    #[test]
    fn widget_settings_round_trip() {
        let c = Config::default();
        let mut f = to_form(&c);
        assert!(!f.widget_own_items);
        assert_eq!(
            f.widget_items,
            c.enabled_items(),
            "prefilled from the taskbar"
        );
        assert_eq!(
            to_config(&f, &c).widget.items,
            [],
            "still following the taskbar"
        );
        f.widget_enabled = true;
        f.widget_on_top = true;
        f.widget_own_items = true;
        f.widget_items = vec![ItemKind::Ping];
        f.widget_width = 10_000;
        let out = to_config(&f, &c);
        assert!(out.widget.enabled);
        assert_eq!(out.widget.layer, WidgetLayer::OnTop);
        assert_eq!(out.widget.items, [ItemKind::Ping]);
        assert_eq!(out.widget.width, 600);
        assert_eq!(to_form(&out).widget_items, [ItemKind::Ping]);
    }

    #[test]
    fn applying_a_theme_selects_it_and_clears_custom_colors() {
        let c = Config::default();
        let mut f = to_form(&c);
        f.items[0].color = "#FF0000".into();
        f.label_color = "#00FF00".into();
        f.panel_opacity_custom = true;
        let neon = pulsar_core::theme::load(None, Some("neon"));
        apply_theme(&mut f, &neon);
        let out = to_config(&f, &c);
        assert_eq!(out.display.theme.as_deref(), Some("neon"));
        assert!(out.items.iter().all(|i| i.color.is_none()));
        assert_eq!(out.display.label_color, None);
        assert_eq!(out.display.panel_opacity, None);
        assert!(out.display.color_labels);
    }

    #[test]
    fn a_saved_theme_keeps_custom_colors() {
        let mut f = to_form(&Config::default());
        f.items[0].color = "#010203".into();
        f.value_color = "#0A0B0C".into();
        f.panel_opacity_custom = true;
        f.panel_opacity = 40;
        let t = theme_from(&f, &Theme::windows(), " Mine ");
        assert_eq!(t.name, "Mine");
        assert_eq!(t.dark.item(f.items[0].kind, CellPart::Main), (1, 2, 3));
        assert_eq!(t.light.item(f.items[0].kind, CellPart::Main), (1, 2, 3));
        assert_eq!(t.dark.text, (10, 11, 12));
        assert_eq!(t.dark.panel_opacity, 40);
        assert_eq!(
            t.light.text,
            (10, 11, 12),
            "as the overlay shows it on a light taskbar"
        );
        assert_eq!(t.light.panel_opacity, 40);
    }

    #[test]
    fn item_styles_colors_and_popups_map_both_ways() {
        use pulsar_core::config::{ColorMode, GraphStyle, PopupDetail};
        let c = Config::default();
        let mut f = to_form(&c);
        assert_eq!(
            (f.items[0].style, f.items[0].color_mode, f.items[0].popup),
            (0, 0, 0)
        );
        f.items[0].style = 2;
        f.items[0].color_mode = 2;
        f.items[0].popup = 3;
        f.items[1].style = 4;
        let out = to_config(&f, &c);
        let first = &out.items[0];
        assert_eq!(
            (first.graph, first.style, first.color_mode, first.popup),
            (
                true,
                GraphStyle::Bars,
                ColorMode::Load,
                Some(PopupDetail::Detailed)
            )
        );
        assert!(!out.items[1].graph, "the last style is text only");
        assert_eq!(to_form(&out).items[1].style, 4);
    }

    #[test]
    fn popup_sizes_map_both_ways_and_zero_inherits() {
        let mut c = Config::default();
        c.popup.width = 420;
        c.popup.graph_height = 120;
        let c = c.sanitize();
        let mut f = to_form(&c);
        assert_eq!((f.popup_width, f.popup_graph_height), (420, 120));
        assert_eq!(
            (f.items[0].popup_width, f.items[0].popup_graph_height),
            (0, 0)
        );
        assert_eq!(to_config(&f, &c), c);
        f.items[0].popup_width = 500;
        let out = to_config(&f, &c);
        assert_eq!(out.items[0].popup_width, Some(500));
        assert_eq!(out.items[0].popup_graph_height, None);
        assert_eq!(out.popup_graph_height(out.items[0].kind), 120);
        assert_eq!(to_form(&out).items[0].popup_width, 500);
        f.items[0].popup_width = 0;
        assert_eq!(to_config(&f, &c).items[0].popup_width, None);
    }

    #[test]
    fn tile_sizes_map_both_ways_and_zero_is_automatic() {
        let c = Config::default().sanitize();
        let mut f = to_form(&c);
        assert_eq!((f.items[0].tile_width, f.items[0].tile_height), (0, 0));
        assert_eq!(to_config(&f, &c), c);
        f.items[0].tile_width = 120;
        f.items[0].tile_height = 32;
        let out = to_config(&f, &c);
        assert_eq!(out.items[0].tile_width, Some(120));
        assert_eq!(out.items[0].tile_height, Some(32));
        assert_eq!(out.items[1].tile_width, None);
        assert_eq!(to_form(&out).items[0].tile_width, 120);
        assert_eq!(to_form(&out).items[0].tile_height, 32);
        f.items[0].tile_width = 0;
        f.items[0].tile_height = 0;
        let out = to_config(&f, &c);
        assert_eq!(
            (out.items[0].tile_width, out.items[0].tile_height),
            (None, None)
        );
    }

    #[test]
    fn overlay_labels_map_both_ways_and_empty_is_the_default() {
        let c = Config::default().sanitize();
        let mut f = to_form(&c);
        assert_eq!(f.items[0].label, "");
        assert_eq!(f.items[0].short_label, "");
        assert_eq!(f.network_down_label, "");
        assert_eq!(to_config(&f, &c), c);

        f.items[0].label = "  Proc\t ".into();
        f.items[0].short_label = "p".into();
        f.network_down_label = "abcdefghijklmnop".into();
        f.network_up_label = " 上传 ".into();
        let out = to_config(&f, &c);
        assert_eq!(out.items[0].label.as_deref(), Some("Proc"));
        assert_eq!(out.items[0].short_label.as_deref(), Some("p"));
        assert_eq!(out.items[1].label, None);
        assert_eq!(out.network.down_label.as_deref(), Some("abcdefghijkl"));
        assert_eq!(out.network.up_label.as_deref(), Some("上传"));
        let back = to_form(&out);
        assert_eq!(back.items[0].label, "Proc");
        assert_eq!(back.network_up_label, "上传");
        assert_eq!(to_config(&back, &out), out);

        f.items[0].label = "   ".into();
        f.network_up_label = String::new();
        let out = to_config(&f, &c);
        assert_eq!(out.items[0].label, None);
        assert_eq!(out.network.up_label, None);
    }

    #[test]
    fn text_only_keeps_the_graph_style_for_later() {
        use pulsar_core::config::GraphStyle;
        let mut c = Config::default();
        c.items[0].style = GraphStyle::Meter;
        let mut f = to_form(&c);
        f.items[0].style = 4;
        let out = to_config(&f, &c);
        assert!(!out.items[0].graph);
        assert_eq!(out.items[0].style, GraphStyle::Meter);
    }

    #[test]
    fn alert_limits_show_in_the_temperature_unit_and_round_trip() {
        let mut c = Config::default();
        c.units.temperature = TemperatureUnit::Fahrenheit;
        c.alerts.cpu.on = true;
        c.alerts.battery.on = true;
        let c = c.sanitize();
        let f = to_form(&c);
        assert_eq!((f.alert_cpu_limit, f.alert_gpu_limit), (194, 185));
        assert_eq!(to_config(&f, &c), c);
        for limit in 50..=110 {
            let mut c = c.clone();
            c.alerts.gpu.limit = limit;
            assert_eq!(to_config(&to_form(&c), &c).alerts.gpu.limit, limit);
        }
    }

    #[test]
    fn a_network_minimum_is_not_rewritten_by_the_form() {
        for bits in [false, true] {
            for rate in [
                0,
                1,
                3,
                300,
                1000,
                1024,
                1500,
                10_240,
                1_000_001,
                5_000_000_000,
                i64::MAX as u64,
            ] {
                let mut c = Config::default();
                c.units.network = if bits {
                    RateUnit::Bits
                } else {
                    RateUnit::Bytes
                };
                c.network.min_down_bps = rate;
                c.network.min_up_bps = rate;
                let c = c.sanitize();
                let f = to_form(&c);
                assert!(f.min_down >= 0 && f.min_down as u64 <= MAX_SHOWN_RATE);
                assert_eq!(to_config(&f, &c), c, "{rate} bytes, bits {bits}");
            }
        }
    }

    #[test]
    fn network_minimums_show_in_the_units_of_the_rates() {
        assert_eq!(shown_rate(0, false), (0, 1));
        assert_eq!(shown_rate(300, false), (300, 0));
        assert_eq!(shown_rate(10 * 1024, false), (10, 1));
        assert_eq!(shown_rate(3 * 1024 * 1024, false), (3, 2));
        assert_eq!(shown_rate(3, true), (24, 0));
        assert_eq!(shown_rate(125, true), (1, 1));
        assert_eq!(shown_rate(10_000, true), (80, 1));
        assert_eq!(shown_rate(125_000, true), (1, 2));
        assert_eq!(shown_rate(5_000_000_000, true), (40_000, 2));
    }

    #[test]
    fn network_minimums_convert_to_bytes() {
        assert_eq!(rate_from_form(300, 0, false), 300);
        assert_eq!(rate_from_form(10, 1, false), 10_240);
        assert_eq!(rate_from_form(2, 2, false), 2_097_152);
        assert_eq!(rate_from_form(8, 0, true), 1);
        assert_eq!(rate_from_form(3, 0, true), 1, "rounds up, not to off");
        assert_eq!(rate_from_form(80, 1, true), 10_000);
        assert_eq!(rate_from_form(1, 2, true), 125_000);
        assert_eq!(rate_from_form(-5, 1, false), 0);
    }

    #[test]
    fn editing_one_network_minimum_leaves_the_other() {
        let mut c = Config::default();
        c.network.min_down_bps = 1_000_001;
        c.network.min_up_bps = 1024;
        let c = c.sanitize();
        let mut f = to_form(&c);
        f.min_up = 5;
        f.min_up_unit = 1;
        let out = to_config(&f, &c);
        assert_eq!(out.network.min_down_bps, 1_000_001);
        assert_eq!(out.network.min_up_bps, 5 * 1024);
    }

    #[test]
    fn changing_the_rate_unit_keeps_a_network_minimum() {
        let mut c = Config::default();
        c.network.min_down_bps = 1500;
        let c = c.sanitize();
        let mut f = to_form(&c);
        f.network_bits = true;
        let c = to_config(&f, &c);
        assert_eq!(c.units.network, RateUnit::Bits);
        assert_eq!(c.network.min_down_bps, 1500);
        let f = to_form(&c);
        assert!(f.network_floor_bits);
        assert_eq!((f.min_down, f.min_down_unit), (12, 1));
        assert_eq!(to_config(&f, &c).network.min_down_bps, 1500);
    }

    #[test]
    fn units_and_popup_settings_round_trip() {
        let mut c = Config::default();
        c.units.temperature = TemperatureUnit::Fahrenheit;
        c.units.network = RateUnit::Bits;
        c.display.upload_on_top = true;
        c.display.value_color_light = Some("#101010".into());
        c.popup.detail = pulsar_core::config::PopupDetail::Simple;
        c.popup.graph = false;
        c.popup.background = PopupBackground::Solid;
        c.popup.processes = 10;
        c.popup.width = 640;
        c.popup.graph_height = 200;
        c.items[2].popup_width = Some(260);
        c.items[2].popup_graph_height = Some(48);
        c.items[2].tile_width = Some(120);
        c.items[2].tile_height = Some(32);
        c.items[2].label = Some("Máquina".into());
        c.items[2].short_label = Some("Má".into());
        c.network.down_label = Some("DL".into());
        c.network.up_label = Some("UL".into());
        c.game.strip = false;
        c.game.games = vec!["game.exe".into()];
        c.hotkeys.widget = Some("Ctrl+Alt+W".into());
        let c = c.sanitize();
        assert_eq!(to_config(&to_form(&c), &c), c);
    }

    #[test]
    fn network_directions_round_trip_through_the_form() {
        for show in NETWORK_SHOW {
            for popup_show in NETWORK_SHOW {
                for split_tiles in [false, true] {
                    let mut c = Config::default();
                    c.network.show = show;
                    c.network.popup_show = popup_show;
                    c.network.split_tiles = split_tiles;
                    let c = c.sanitize();
                    assert_eq!(to_config(&to_form(&c), &c), c);
                }
            }
        }
        let f = to_form(&Config::default());
        assert_eq!((f.network_show, f.network_popup_show), (0, 0));
        assert!(!f.network_split_tiles);
    }

    #[test]
    fn a_split_choice_survives_while_the_switch_is_hidden() {
        let mut c = Config::default();
        c.network.split_tiles = true;
        c.network.show = NetworkShow::Upload;
        c.display.mode = DisplayMode::Text;
        let out = to_config(&to_form(&c), &c);
        assert!(out.network.split_tiles);
        assert_eq!(out.network.show, NetworkShow::Upload);
    }

    #[test]
    fn every_no_room_choice_round_trips() {
        for choice in [NoRoom::Float, NoRoom::Tray, NoRoom::Hide] {
            let mut c = Config::default();
            c.display.fit_no_room = choice;
            assert_eq!(to_config(&to_form(&c), &c).display.fit_no_room, choice);
        }
    }

    #[test]
    fn new_fields_round_trip_through_the_form() {
        let mut c = Config::default();
        c.display.labels = LabelStyle::None;
        c.display.icons = true;
        c.display.label_color = Some("#101010".into());
        c.display.value_color = Some("#EEEEEE".into());
        c.display.color_labels = true;
        c.display.font_family = Some("Bahnschrift".into());
        c.display.font_bold = true;
        c.display.hide_in_fullscreen = false;
        c.display.attach_to_taskbar = false;
        c.display.position = Position::Floating;
        c.display.float_position = Some([10, 20]);
        c.display.lock_position = true;
        c.display.graph_inline = true;
        c.display.taskbar_monitor = Some(r"\\?\DISPLAY#DEL5678#5&1&UID2".into());
        c.display.fit_to_taskbar = false;
        c.display.fit_remove_from = RemoveFrom::Left;
        c.display.fit_no_room = NoRoom::Tray;
        c.display.fit_other_side = false;
        c.general.tray_icon = false;
        c.disk.drives = vec!["C".into(), "D".into()];
        c.disk.value = DriveValue::Used;
        c.network.adapters = vec!["eth".into(), "wifi".into()];
        c.gpu.adapters = vec!["NVIDIA GeForce RTX 4090".into()];
        c.frames.monitor = Some(r"\\?\DISPLAY#DEL1234#5&1&UID1".into());
        c.general.language = Some("es".into());
        c.items[0].graph = false;
        c.items[1].popup = Some(pulsar_core::config::PopupDetail::Simple);
        let c = c.sanitize();
        assert_eq!(to_config(&to_form(&c), &c), c);
    }
}
