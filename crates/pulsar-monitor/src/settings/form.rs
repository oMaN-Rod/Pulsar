//! The values shown in the settings window, and their mapping to `Config`.

use pulsar_core::colors::{Rgb, default_rgb};
use pulsar_core::config::{Config, DisplayMode, DriveValue, ItemConfig, LabelStyle, Position};
use pulsar_core::layout::CellPart;
use pulsar_core::metric::ItemKind;

/// Shown in the opacity slider when the theme default is in use.
pub const THEME_TILE_OPACITY: i32 = 8;

#[derive(Clone, Debug, PartialEq)]
pub struct ItemRow {
    pub kind: ItemKind,
    pub enabled: bool,
    /// `#RRGGBB`, or empty for the default colour.
    pub color: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Form {
    pub autostart: bool,
    pub update_check: bool,
    pub sample_interval_ms: i32,
    pub history_len: i32,
    pub text_mode: bool,
    pub hover_popup: bool,
    /// 0 next to the tray, 1 left edge, 2 floating.
    pub position: i32,
    pub lock_position: bool,
    pub hide_in_fullscreen: bool,
    /// 0 full, 1 short, 2 none (icons only).
    pub labels: i32,
    pub icons: bool,
    /// Empty for the theme colour.
    pub label_color: String,
    pub value_color: String,
    pub color_labels: bool,
    /// Empty for the default font.
    pub font_family: String,
    pub font_bold: bool,
    pub offset_px: i32,
    pub fallback_margin_px: i32,
    pub font_size_pt: i32,
    pub show_on_all_taskbars: bool,
    pub accent_graphs: bool,
    /// Empty for the theme tint.
    pub tile_color: String,
    /// Off means the theme's opacity; `tile_opacity` is then ignored.
    pub tile_opacity_custom: bool,
    pub tile_opacity: i32,
    /// Empty for the theme colour.
    pub panel_color: String,
    pub panel_opacity: i32,
    pub items: Vec<ItemRow>,
    pub ping_host: String,
    pub ping_interval_ms: i32,
    /// Letters with their own disk cell; empty shows all disks combined.
    pub drives: Vec<String>,
    pub drive_used: bool,
    /// Empty sums every hardware adapter.
    pub adapter: String,
}

/// `#RRGGBB` → `(r, g, b)`, for the colour swatches; anything else is `None`.
pub fn parse_hex(s: &str) -> Option<Rgb> {
    let hex = s.trim().strip_prefix('#').filter(|h| h.len() == 6)?;
    let byte = |i: usize| u8::from_str_radix(hex.get(i..i + 2)?, 16).ok();
    Some((byte(0)?, byte(2)?, byte(4)?))
}

pub fn to_hex((r, g, b): Rgb) -> String {
    format!("#{r:02X}{g:02X}{b:02X}")
}

/// The colour an item is drawn in: its valid custom colour, else the default.
pub fn item_rgb(kind: ItemKind, hex: &str) -> Rgb {
    parse_hex(hex).unwrap_or_else(|| default_rgb(kind, CellPart::Main))
}

pub fn item_name(kind: ItemKind) -> &'static str {
    match kind {
        ItemKind::Cpu => "CPU",
        ItemKind::Ram => "Memory",
        ItemKind::Gpu => "GPU",
        ItemKind::Disk => "Disk",
        ItemKind::Network => "Network",
        ItemKind::Ping => "Ping",
        ItemKind::GpuTemp => "GPU temperature",
    }
}

pub fn to_form(c: &Config) -> Form {
    let d = &c.display;
    Form {
        autostart: c.general.autostart,
        update_check: c.general.update_check,
        sample_interval_ms: c.general.sample_interval_ms as i32,
        history_len: c.general.history_len as i32,
        text_mode: d.mode == DisplayMode::Text,
        hover_popup: d.hover_popup,
        position: match d.position {
            Position::Right => 0,
            Position::Left => 1,
            Position::Floating => 2,
        },
        lock_position: d.lock_position,
        hide_in_fullscreen: d.hide_in_fullscreen,
        labels: match d.labels {
            LabelStyle::Full => 0,
            LabelStyle::Short => 1,
            LabelStyle::None => 2,
        },
        icons: d.icons,
        label_color: d.label_color.clone().unwrap_or_default(),
        value_color: d.value_color.clone().unwrap_or_default(),
        color_labels: d.color_labels,
        font_family: d.font_family.clone().unwrap_or_default(),
        font_bold: d.font_bold,
        offset_px: d.offset_px,
        fallback_margin_px: d.fallback_margin_px,
        font_size_pt: d.font_size_pt.round() as i32,
        show_on_all_taskbars: d.show_on_all_taskbars,
        accent_graphs: d.accent_graphs,
        tile_color: d.tile_color.clone().unwrap_or_default(),
        tile_opacity_custom: d.tile_opacity.is_some(),
        tile_opacity: d.tile_opacity.map_or(THEME_TILE_OPACITY, i32::from),
        panel_color: d.panel_color.clone().unwrap_or_default(),
        panel_opacity: i32::from(d.panel_opacity),
        items: c
            .items
            .iter()
            .map(|i| ItemRow {
                kind: i.kind,
                enabled: i.enabled,
                color: i.color.clone().unwrap_or_default(),
            })
            .collect(),
        ping_host: c.ping.host.clone(),
        ping_interval_ms: c.ping.interval_ms as i32,
        drives: c.disk.drives.clone(),
        drive_used: c.disk.value == DriveValue::Used,
        adapter: c.network.adapter.clone().unwrap_or_default(),
    }
}

/// Every setting back to its default, except autostart, which mirrors the
/// registry and is changed only by its own switch.
pub fn reset(current: &Form) -> Form {
    Form {
        autostart: current.autostart,
        ..to_form(&Config::default())
    }
}

fn non_empty(s: &str) -> Option<String> {
    let s = s.trim();
    (!s.is_empty()).then(|| s.to_string())
}

fn clamp_u8(v: i32) -> u8 {
    v.clamp(0, 100) as u8
}

/// Builds a sanitised config from the form, keeping anything the form does
/// not show (such as the file version) from `base`.
pub fn to_config(f: &Form, base: &Config) -> Config {
    let mut c = base.clone();
    c.general.autostart = f.autostart;
    c.general.update_check = f.update_check;
    c.general.sample_interval_ms = f.sample_interval_ms.max(0) as u32;
    c.general.history_len = f.history_len.max(0) as usize;
    let d = &mut c.display;
    d.mode = if f.text_mode {
        DisplayMode::Text
    } else {
        DisplayMode::Graph
    };
    d.hover_popup = f.hover_popup;
    d.position = match f.position {
        1 => Position::Left,
        2 => Position::Floating,
        _ => Position::Right,
    };
    d.lock_position = f.lock_position;
    d.hide_in_fullscreen = f.hide_in_fullscreen;
    d.labels = match f.labels {
        1 => LabelStyle::Short,
        2 => LabelStyle::None,
        _ => LabelStyle::Full,
    };
    d.icons = f.icons;
    d.label_color = non_empty(&f.label_color);
    d.value_color = non_empty(&f.value_color);
    d.color_labels = f.color_labels;
    d.font_family = non_empty(&f.font_family);
    d.font_bold = f.font_bold;
    d.offset_px = f.offset_px;
    d.fallback_margin_px = f.fallback_margin_px;
    d.font_size_pt = f.font_size_pt as f32;
    d.show_on_all_taskbars = f.show_on_all_taskbars;
    d.accent_graphs = f.accent_graphs;
    d.tile_color = non_empty(&f.tile_color);
    d.tile_opacity = f.tile_opacity_custom.then(|| clamp_u8(f.tile_opacity));
    d.panel_color = non_empty(&f.panel_color);
    d.panel_opacity = clamp_u8(f.panel_opacity);
    c.items = f
        .items
        .iter()
        .map(|r| ItemConfig {
            kind: r.kind,
            enabled: r.enabled,
            color: non_empty(&r.color),
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
    c.network.adapter = non_empty(&f.adapter);
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

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(f.tile_opacity, THEME_TILE_OPACITY);
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
    fn empty_colours_mean_default_and_invalid_ones_are_dropped() {
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
        f.panel_opacity = 250;
        f.font_size_pt = 99;
        let out = to_config(&f, &c);
        assert_eq!(out.general.sample_interval_ms, 500);
        assert_eq!(out.general.history_len, 10);
        assert_eq!(out.display.panel_opacity, 100);
        assert_eq!(out.display.font_size_pt, 20.0);
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
    fn parses_complete_hex_colours_only() {
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
    fn item_swatch_falls_back_to_the_default_colour() {
        assert_eq!(item_rgb(ItemKind::Cpu, "#010203"), (1, 2, 3));
        assert_eq!(
            item_rgb(ItemKind::Cpu, ""),
            default_rgb(ItemKind::Cpu, CellPart::Main)
        );
        assert_eq!(
            item_rgb(ItemKind::Cpu, "#01"),
            default_rgb(ItemKind::Cpu, CellPart::Main)
        );
    }

    #[test]
    fn every_item_has_a_display_name() {
        for kind in ItemKind::ALL {
            assert!(!item_name(kind).is_empty());
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
        c.display.position = Position::Floating;
        c.display.float_position = Some([10, 20]);
        c.display.lock_position = true;
        c.disk.drives = vec!["C".into(), "D".into()];
        c.disk.value = DriveValue::Used;
        c.network.adapter = Some("eth".into());
        let c = c.sanitize();
        assert_eq!(to_config(&to_form(&c), &c), c);
    }

    #[test]
    fn reset_keeps_autostart_and_nothing_else() {
        let mut f = to_form(&Config::default());
        f.autostart = true;
        f.labels = 1;
        f.icons = true;
        f.font_family = "Arial".into();
        let r = reset(&f);
        assert!(r.autostart);
        assert_eq!(r.labels, 0);
        assert!(!r.icons);
        assert_eq!(r.font_family, "");
    }
}
