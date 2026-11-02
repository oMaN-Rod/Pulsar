//! Area graphs as path commands for Slint, scaled like the hover popup's.

use std::fmt::Write;

use pulsar_core::colors::default_rgb;
use pulsar_core::config::Config;
use pulsar_core::display::graph_max;
use pulsar_core::format::format_value;
use pulsar_core::history::{History, HistoryStore};
use pulsar_core::layout::CellPart;
use pulsar_core::metric::{ItemKind, MetricKey};
use slint::Color;

use crate::Graph;

/// Commands for a series in a box `capacity - 1` wide and 100 high, the
/// newest sample at the right edge; missing samples draw as zero.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Paths {
    pub fill: String,
    pub line: String,
    pub viewbox: f32,
}

pub fn paths<I>(samples: I, capacity: usize, max: f64) -> Paths
where
    I: ExactSizeIterator<Item = f64>,
{
    let width = (capacity.max(2) - 1) as f32;
    let n = samples.len();
    let mut out = Paths {
        viewbox: width,
        ..Paths::default()
    };
    if n == 0 || max <= 0.0 {
        return out;
    }
    let x_of = |i: usize| width - (n - 1 - i) as f32;
    let mut line = String::with_capacity(n * 12);
    for (i, v) in samples.enumerate() {
        let level = if v.is_nan() {
            0.0
        } else {
            (v / max).clamp(0.0, 1.0)
        };
        let y = 100.0 - level as f32 * 100.0;
        let _ = write!(
            line,
            "{}{:.1} {:.1} ",
            if i == 0 { "M" } else { "L" },
            x_of(i),
            y
        );
    }
    out.fill = format!(
        "M{:.1} 100 L{} L{:.1} 100 Z",
        x_of(0),
        line.trim_end().trim_start_matches('M'),
        width
    );
    out.line = line.trim_end().to_string();
    out
}

/// The newest `count` samples of a history, for a sparkline.
pub fn recent(history: &History, count: usize, key: MetricKey) -> Paths {
    let skip = history.len().saturating_sub(count);
    let values: Vec<f64> = history.iter().skip(skip).collect();
    paths(values.into_iter(), count, graph_max(key.unit(), &[history]))
}

/// What the overlay draws a series in: the item's own color, the Windows
/// accent when asked for, or the default.
pub fn series_color(
    config: &Config,
    accent: Option<Color>,
    kind: ItemKind,
    part: CellPart,
) -> Color {
    let custom = config
        .items
        .iter()
        .find(|i| i.kind == kind)
        .and_then(|i| i.color.as_deref())
        .and_then(parse_hex);
    custom
        .or(if config.display.accent_graphs {
            accent
        } else {
            None
        })
        .unwrap_or_else(|| {
            let (r, g, b) = default_rgb(kind, part);
            Color::from_rgb_u8(r, g, b)
        })
}

fn parse_hex(hex: &str) -> Option<Color> {
    let digits = hex.strip_prefix('#')?;
    if digits.len() != 6 {
        return None;
    }
    let v = u32::from_str_radix(digits, 16).ok()?;
    Some(Color::from_rgb_u8((v >> 16) as u8, (v >> 8) as u8, v as u8))
}

pub fn accent() -> Option<Color> {
    pulsar_core::system_theme::accent_abgr()
        .map(|abgr| Color::from_rgb_u8(abgr as u8, (abgr >> 8) as u8, (abgr >> 16) as u8))
}

/// A clockwise arc from twelve o'clock covering `fraction` of a circle
/// centred at (`centre`, `centre`); empty when there is nothing to draw.
pub fn arc(fraction: f64, centre: f32, radius: f32) -> String {
    if !fraction.is_finite() || fraction <= 0.0 {
        return String::new();
    }
    let f = fraction.min(0.9999) as f32;
    let angle = f * std::f32::consts::TAU;
    let (x, y) = (centre + radius * angle.sin(), centre - radius * angle.cos());
    format!(
        "M{centre} {:.2} A{radius} {radius} 0 {} 1 {x:.2} {y:.2}",
        centre - radius,
        u8::from(f > 0.5)
    )
}

/// Cool blue through green and yellow to red, from 30 to 90°C.
pub fn heat(celsius: f64) -> Color {
    const STOPS: [(f64, [f64; 3]); 4] = [
        (30.0, [76.0, 194.0, 255.0]),
        (50.0, [108.0, 203.0, 95.0]),
        (70.0, [245.0, 208.0, 74.0]),
        (90.0, [255.0, 90.0, 95.0]),
    ];
    let t = celsius.clamp(STOPS[0].0, STOPS[3].0);
    let i = STOPS.iter().position(|s| t <= s.0).unwrap_or(3).max(1);
    let ((a, ca), (b, cb)) = (STOPS[i - 1], STOPS[i]);
    let k = (t - a) / (b - a);
    let mix = |j: usize| (ca[j] + (cb[j] - ca[j]) * k).round() as u8;
    Color::from_rgb_u8(mix(0), mix(1), mix(2))
}

/// One or two series from the store on a shared scale.
pub struct Series<'a> {
    pub key: MetricKey,
    pub color: Color,
    pub legend: &'a str,
}

pub fn graph(
    history: &HistoryStore,
    title: &str,
    value: String,
    caption: String,
    first: Series,
    second: Option<Series>,
) -> Graph {
    let capacity = history.capacity();
    let histories: Vec<&History> = [Some(&first), second.as_ref()]
        .into_iter()
        .flatten()
        .filter_map(|s| history.get(s.key))
        .collect();
    let max = graph_max(first.key.unit(), &histories);
    let draw = |key: MetricKey| {
        history
            .get(key)
            .map(|h| paths(h.iter().collect::<Vec<_>>().into_iter(), capacity, max))
            .unwrap_or_else(|| Paths {
                viewbox: (capacity.max(2) - 1) as f32,
                ..Paths::default()
            })
    };
    let main = draw(first.key);
    let other = second.as_ref().map(|s| draw(s.key));
    Graph {
        title: title.into(),
        value: value.into(),
        caption: caption.into(),
        color: first.color,
        fill: main.fill.into(),
        line: main.line.into(),
        second_color: second.as_ref().map_or(first.color, |s| s.color),
        second_line: other.map(|p| p.line).unwrap_or_default().into(),
        legend: if second.is_some() {
            first.legend.into()
        } else {
            Default::default()
        },
        second_legend: second.as_ref().map(|s| s.legend.into()).unwrap_or_default(),
        ceiling: format_value(max, first.key.unit()).into(),
        viewbox: main.viewbox,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_newest_sample_sits_at_the_right_edge() {
        let p = paths(vec![0.0, 50.0, 100.0].into_iter(), 5, 100.0);
        assert_eq!(p.viewbox, 4.0);
        assert_eq!(p.line, "M2.0 100.0 L3.0 50.0 L4.0 0.0");
        assert_eq!(p.fill, "M2.0 100 L2.0 100.0 L3.0 50.0 L4.0 0.0 L4.0 100 Z");
    }

    #[test]
    fn missing_samples_draw_as_zero_and_peaks_clamp() {
        let p = paths(vec![f64::NAN, 250.0].into_iter(), 2, 100.0);
        assert_eq!(p.line, "M0.0 100.0 L1.0 0.0");
    }

    #[test]
    fn nothing_to_draw_is_empty() {
        let p = paths(Vec::new().into_iter(), 60, 100.0);
        assert!(p.line.is_empty() && p.fill.is_empty());
        assert_eq!(p.viewbox, 59.0);
    }

    #[test]
    fn arcs_start_at_the_top_and_go_clockwise() {
        assert_eq!(arc(0.0, 32.0, 27.0), "");
        assert_eq!(arc(0.25, 32.0, 27.0), "M32 5.00 A27 27 0 0 1 59.00 32.00");
        assert!(arc(0.75, 32.0, 27.0).contains(" 0 1 1 5.00 32.00"));
        assert!(!arc(1.0, 32.0, 27.0).is_empty());
    }

    #[test]
    fn heat_runs_from_blue_to_red() {
        assert_eq!(heat(10.0), Color::from_rgb_u8(76, 194, 255));
        assert_eq!(heat(50.0), Color::from_rgb_u8(108, 203, 95));
        assert_eq!(heat(60.0), Color::from_rgb_u8(177, 206, 85));
        assert_eq!(heat(120.0), Color::from_rgb_u8(255, 90, 95));
    }

    #[test]
    fn colors_follow_the_overlay() {
        let mut config = Config::default();
        let cpu = series_color(&config, None, ItemKind::Cpu, CellPart::Main);
        assert_eq!(cpu, Color::from_rgb_u8(0x4C, 0xC2, 0xFF));
        config.display.accent_graphs = true;
        let accent = Color::from_rgb_u8(1, 2, 3);
        assert_eq!(
            series_color(&config, Some(accent), ItemKind::Cpu, CellPart::Main),
            accent
        );
        config
            .items
            .iter_mut()
            .find(|i| i.kind == ItemKind::Cpu)
            .unwrap()
            .color = Some("#102030".into());
        assert_eq!(
            series_color(&config, Some(accent), ItemKind::Cpu, CellPart::Main),
            Color::from_rgb_u8(0x10, 0x20, 0x30)
        );
    }
}
