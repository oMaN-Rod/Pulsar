pub mod content;
pub mod layout;

use pulsar_core::config::Material;
use pulsar_core::history::{History, HistoryStore};
use pulsar_core::layout::{Rect, TextMeasure};
use pulsar_core::metric::ItemKind;
use pulsar_core::tr;
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::DirectWrite::IDWriteTextFormat;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, HTTRANSPARENT, HWND_TOPMOST, MA_NOACTIVATE,
    RegisterClassW, SW_HIDE, SW_SHOWNOACTIVATE, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
    SetWindowPos, ShowWindow, WM_MOUSEACTIVATE, WM_NCHITTEST, WNDCLASSW, WS_EX_LAYERED,
    WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};
use windows::core::{Result, w};

use crate::glass::Backdrop;
use crate::render::{Renderer, Surface};
use crate::skin::Skin;
use crate::steady::Steady;
use crate::taskbar::placement::Rect32;
use crate::text::Text;
use crate::theme::{Color, Palette};
use content::{Block, PopupContent};
use layout::{Metrics, ellipsize, fit, position};
use pulsar_core::display::{area_points, graph_max, metric_for};

const CLASS: windows::core::PCWSTR = w!("PulsarPopup");

unsafe extern "system" fn popup_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        // Mouse input passes through to whatever is underneath.
        WM_NCHITTEST => LRESULT(HTTRANSPARENT as isize),
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        _ => unsafe { DefWindowProcW(hwnd, msg, wp, lp) },
    }
}

/// Where the popup points: the hovered item's screen rectangle and the
/// taskbar and monitor it belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Anchor {
    pub item: Rect32,
    pub taskbar: Rect32,
    pub monitor: Rect32,
    pub dpi: u32,
}

/// A click-through, non-activating layered window reused for every hover.
/// Its bitmap and its own renderer are dropped while hidden, so the popup's
/// larger surfaces and glyph caches do not stay in memory once it closes.
/// The blurring materials put a blur window beneath it.
pub struct Popup {
    hwnd: HWND,
    instance: HINSTANCE,
    surface: Option<Surface>,
    renderer: Option<Renderer>,
    backdrop: Option<Backdrop>,
    no_backdrop: bool,
    pub open: Option<(ItemKind, Anchor)>,
}

impl Popup {
    pub fn create(instance: HINSTANCE) -> Result<Self> {
        let hwnd = unsafe {
            RegisterClassW(&WNDCLASSW {
                lpfnWndProc: Some(popup_proc),
                hInstance: instance,
                lpszClassName: CLASS,
                ..Default::default()
            });
            CreateWindowExW(
                WS_EX_TOOLWINDOW
                    | WS_EX_NOACTIVATE
                    | WS_EX_TOPMOST
                    | WS_EX_LAYERED
                    | WS_EX_TRANSPARENT,
                CLASS,
                w!("Pulsar popup"),
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
        crate::overlay::disable_transitions(hwnd);
        Ok(Self {
            hwnd,
            instance,
            surface: None,
            renderer: None,
            backdrop: None,
            no_backdrop: false,
            open: None,
        })
    }

    pub fn show(
        &mut self,
        anchor: Anchor,
        content: &PopupContent,
        style: &Style,
        text: &Text,
    ) -> Result<()> {
        let blur = style.material.blurs() && style.transparency && self.ensure_backdrop();
        if self.renderer.is_none() {
            self.renderer = Some(Renderer::new()?);
        }
        let renderer = self.renderer.as_mut().unwrap();
        let m = Metrics::with_size(
            anchor.dpi,
            style.font_px,
            text.line_height(style.font_px),
            style.width,
            style.graph_height,
        );
        let fitted = fit(
            content,
            m,
            anchor.monitor,
            tr!("More in the details window"),
        );
        let (content, m) = (fitted.content.as_ref(), fitted.metrics);
        let (w, h) = (m.width.ceil() as i32, fitted.height);
        if self
            .surface
            .as_ref()
            .is_none_or(|s| s.width != w || s.height != h)
        {
            self.surface = Some(Surface::new(w, h)?);
        }
        let surface = self.surface.as_ref().unwrap();
        let p = style.palette;
        let skin = Skin::new(style.material, p.popup, 1.0, style.glass_tint, p, blur);
        let radius = skin.radius * m.scale;
        let full = Rect {
            x: 0.0,
            y: 0.0,
            w: w as f32,
            h: h as f32,
        };
        renderer.paint(surface, Color::rgb(0, 0, 0).with_alpha(0.0), |r| {
            skin.panel(r, full, radius, m.scale, true)?;
            draw(r, text, content, style, &m, full.w)
        })?;
        let (x, y) = position(anchor.item, anchor.taskbar, anchor.monitor, w, h, m.scale);
        renderer.present(self.hwnd, surface, x, y)?;
        if self.open.is_none() {
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
        }
        match &mut self.backdrop {
            Some(backdrop) if blur => {
                if let Err(e) = backdrop.set_shapes(&[(full, radius)], skin.blur) {
                    log::warn!("could not shape the popup's blur: {e}");
                    self.backdrop = None;
                    self.no_backdrop = true;
                } else {
                    backdrop.place(self.hwnd, x, y, w, h);
                }
            }
            Some(backdrop) => backdrop.hide(),
            None => {}
        }
        self.open = Some((content.kind, anchor));
        Ok(())
    }

    fn ensure_backdrop(&mut self) -> bool {
        if self.backdrop.is_none() && !self.no_backdrop {
            match Backdrop::create(self.instance) {
                Ok(backdrop) => self.backdrop = Some(backdrop),
                Err(e) => {
                    log::info!("no blur behind popups; they stay solid: {e}");
                    self.no_backdrop = true;
                }
            }
        }
        self.backdrop.is_some()
    }

    pub fn hide(&mut self) {
        if self.open.take().is_some() {
            unsafe {
                let _ = ShowWindow(self.hwnd, SW_HIDE);
            }
        }
        if let Some(backdrop) = &mut self.backdrop {
            backdrop.hide();
        }
        self.surface = None;
        self.renderer = None;
    }
}

impl Drop for Popup {
    fn drop(&mut self) {
        self.hide();
        unsafe {
            let _ = DestroyWindow(self.hwnd);
        }
    }
}

/// Everything the popup needs besides its content.
pub struct Style<'a> {
    pub palette: &'a Palette,
    pub history: &'a HistoryStore,
    pub font_px: f32,
    /// Logical pixels.
    pub width: u16,
    /// Logical pixels.
    pub graph_height: u16,
    pub material: Material,
    /// 0–1.
    pub glass_tint: f32,
    /// Windows' "Transparency effects" setting.
    pub transparency: bool,
}

fn draw(
    r: &Renderer,
    text: &Text,
    content: &PopupContent,
    style: &Style,
    m: &Metrics,
    w: f32,
) -> Result<()> {
    let p = style.palette;

    let body = text.format(m.font_px)?;
    let title = text.format(m.title_px)?;
    let right = w - m.pad;
    let accent = p.graph(content.kind, content.series[0]);
    let mut y = m.pad;

    let room = (right - m.pad) * 0.5;
    let value = Steady::new(text, &content.value, Some(&content.widest), m.title_px);
    let value_w = value.width.min(room);
    let heading = ellipsize(
        content.title,
        right - value_w - m.pad - 8.0 * m.scale,
        m.title_px,
        text,
    );
    r.text_at(text, &title, &heading, m.pad, y, p.popup_text)?;
    if value.width <= room {
        value.draw(r, text, &title, right - value_w, y, accent)?;
    } else {
        let value = ellipsize(&content.value, room, m.title_px, text);
        let x = right - text.width(&value, m.title_px);
        r.text_at(text, &title, &value, x, y, accent)?;
    }
    y += m.title_h + m.block_gap;

    if content.graph {
        let graph = Rect {
            x: m.pad,
            y,
            w: w - 2.0 * m.pad,
            h: m.graph_h,
        };
        r.fill_rounded(graph, 4.0 * m.scale, p.popup_border)?;
        let histories: Vec<&History> = content
            .series
            .iter()
            .filter_map(|&part| style.history.get(metric_for(content.kind, part)))
            .collect();
        let unit = metric_for(content.kind, content.series[0]).unit();
        let max = graph_max(unit, &histories);
        for (i, &part) in content.series.iter().enumerate() {
            if let Some(history) = style.history.get(metric_for(content.kind, part)) {
                let color = p.graph(content.kind, part);
                r.series(
                    &area_points(history, graph, max),
                    color,
                    i == 0,
                    1.5 * m.scale,
                )?;
            }
        }
        y += m.graph_h + m.block_gap;
    }

    if let Some(error) = &content.error {
        let error = ellipsize(error, right - m.pad, m.font_px, text);
        r.text_at(text, &body, &error, m.pad, y, p.warning)?;
        y += m.line_h + m.block_gap;
    }

    let gap = 12.0 * m.scale;
    for block in &content.blocks {
        match block {
            Block::Rows { heading, rows } => {
                if let Some(heading) = heading {
                    let heading = ellipsize(heading, right - m.pad, m.font_px, text);
                    r.text_at(text, &body, &heading, m.pad, y, p.popup_label)?;
                    y += m.line_h;
                }
                for (label, value) in rows {
                    let value_w =
                        steady_value(r, text, &body, m.font_px, value, (right, y), p.popup_text)?;
                    let room = right - value_w - gap - m.pad;
                    let label = ellipsize(label, room, m.font_px, text);
                    r.text_at(text, &body, &label, m.pad, y, p.popup_text)?;
                    y += m.line_h;
                }
            }
            Block::Bars { values, .. } if values.is_empty() => continue,
            Block::Bars { heading, values } => {
                let heading = ellipsize(heading, right - m.pad, m.font_px, text);
                r.text_at(text, &body, &heading, m.pad, y, p.popup_label)?;
                y += m.line_h;
                let n = values.len() as f32;
                let span = w - 2.0 * m.pad;
                let bar_gap = if span / n >= 4.0 * m.scale {
                    2.0 * m.scale
                } else {
                    0.0
                };
                let bar_w = (span - (n - 1.0) * bar_gap) / n;
                for (i, v) in values.iter().enumerate() {
                    let x = m.pad + i as f32 * (bar_w + bar_gap);
                    let slot = Rect {
                        x,
                        y,
                        w: bar_w,
                        h: m.bars_h,
                    };
                    r.fill_rounded(slot, 1.5 * m.scale, p.popup_border)?;
                    let filled = m.bars_h * (v / 100.0).clamp(0.0, 1.0) as f32;
                    let bar = Rect {
                        x,
                        y: y + m.bars_h - filled,
                        w: bar_w,
                        h: filled,
                    };
                    r.fill_rounded(bar, 1.5 * m.scale, accent)?;
                }
                y += m.bars_h;
            }
            Block::Composition { heading, parts } => {
                let heading = ellipsize(heading, right - m.pad, m.font_px, text);
                r.text_at(text, &body, &heading, m.pad, y, p.popup_label)?;
                y += m.line_h;
                let whole: f64 = parts.iter().map(|(_, v, _)| v.max(0.0)).sum();
                let colors: Vec<Color> = (0..parts.len())
                    .map(|i| share_color(accent, p.popup_border, i, parts.len()))
                    .collect();
                let track = Rect {
                    x: m.pad,
                    y,
                    w: w - 2.0 * m.pad,
                    h: m.stack_h,
                };
                r.fill_rounded(track, m.stack_h / 2.0, p.popup_border)?;
                let mut x = track.x;
                for ((_, value, _), &color) in parts.iter().zip(&colors) {
                    if whole <= 0.0 {
                        break;
                    }
                    let seg_w = track.w * (value.max(0.0) / whole) as f32;
                    let seg = Rect {
                        x,
                        w: seg_w,
                        ..track
                    };
                    r.fill_rounded(seg, 1.5 * m.scale, color)?;
                    x += seg_w;
                }
                y += m.stack_h + m.block_gap / 2.0;
                let dot = (m.font_px * 0.6).round();
                for ((label, _, shown), &color) in parts.iter().zip(&colors) {
                    let swatch = Rect {
                        x: m.pad,
                        y: y + (m.line_h - dot) / 2.0,
                        w: dot,
                        h: dot,
                    };
                    r.fill_rounded(swatch, dot / 4.0, color)?;
                    let value_w =
                        steady_value(r, text, &body, m.font_px, shown, (right, y), p.popup_text)?;
                    let label_x = m.pad + dot + 6.0 * m.scale;
                    let room = right - value_w - gap - label_x;
                    let label = ellipsize(label, room, m.font_px, text);
                    r.text_at(text, &body, &label, label_x, y, p.popup_text)?;
                    y += m.line_h;
                }
            }
        }
        y += m.block_gap;
    }
    Ok(())
}

/// Draws `value` ending at `right`, in the room of its widest form so the
/// label beside it keeps its room, within most of the width from the left
/// edge; returns that room.
fn steady_value(
    r: &Renderer,
    text: &Text,
    format: &IDWriteTextFormat,
    px: f32,
    value: &str,
    (right, y): (f32, f32),
    color: Color,
) -> Result<f32> {
    let room = right * 0.7;
    let steady = Steady::new(text, value, None, px);
    if steady.width <= room {
        steady.draw(r, text, format, right - steady.width, y, color)?;
        return Ok(steady.width);
    }
    let value = ellipsize(value, room, px, text);
    let w = text.width(&value, px);
    r.text_at(text, format, &value, right - w, y, color)?;
    Ok(w)
}

/// The accent fading out part by part; the last part, what is left over,
/// takes the track color.
fn share_color(accent: Color, track: Color, index: usize, count: usize) -> Color {
    if index + 1 == count && count > 1 {
        return track.with_alpha((track.a * 2.5).min(1.0));
    }
    let steps = count.saturating_sub(2).max(1) as f32;
    accent.with_alpha(1.0 - 0.6 * index as f32 / steps)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pulsar_core::config::Config;
    use pulsar_core::metric::{MetricKey, Snapshot};

    #[test]
    fn popup_resources_are_released_when_hidden() {
        let instance = unsafe { windows::Win32::System::LibraryLoader::GetModuleHandleW(None) }
            .unwrap()
            .into();
        let mut popup = Popup::create(instance).unwrap();
        for material in [
            Material::Solid,
            Material::Glass,
            Material::Liquid,
            Material::Acrylic,
            Material::Clear,
        ] {
            check_resources_are_released(&mut popup, material);
        }
    }

    fn check_resources_are_released(popup: &mut Popup, material: Material) {
        let mut snapshot = Snapshot::default();
        snapshot.set(MetricKey::CpuTotal, 12.0);
        for i in 0..8u16 {
            snapshot.set(MetricKey::CpuCore(i), f64::from(i) * 10.0);
        }
        let mut history = HistoryStore::new(60);
        history.record(&snapshot);
        history.record(&snapshot);
        let config = Config::default();
        let palette = Palette::new(false, &config, None, &crate::theme::Theme::windows());
        let content = content::build(ItemKind::Cpu, &snapshot, &history, "", &[]);
        let style = Style {
            palette: &palette,
            history: &history,
            font_px: 12.0,
            width: config.popup.width,
            graph_height: config.popup.graph_height,
            material,
            glass_tint: 0.3,
            transparency: true,
        };
        // Far off-screen so the test never flashes a window on the desktop.
        let anchor = Anchor {
            item: Rect32 {
                left: -4000,
                top: -4000,
                right: -3956,
                bottom: -3960,
            },
            taskbar: Rect32 {
                left: -5000,
                top: -4000,
                right: -3000,
                bottom: -3952,
            },
            monitor: Rect32 {
                left: -5000,
                top: -5000,
                right: -3000,
                bottom: -3952,
            },
            dpi: 96,
        };
        let text = Text::new(None, false).unwrap();
        popup.show(anchor, &content, &style, &text).unwrap();
        assert_eq!(popup.open.map(|(k, _)| k), Some(ItemKind::Cpu));
        assert!(popup.surface.is_some() && popup.renderer.is_some());
        if material.blurs() {
            assert!(
                popup.backdrop.as_ref().is_none_or(Backdrop::shown),
                "{material:?} blurs behind the popup"
            );
        }
        popup.hide();
        assert!(popup.open.is_none());
        assert!(!popup.backdrop.as_ref().is_some_and(Backdrop::shown));
        assert!(popup.surface.is_none(), "bitmap freed while hidden");
        assert!(
            popup.renderer.is_none(),
            "renderer and its caches freed while hidden"
        );
    }
}
