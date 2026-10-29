pub mod content;
pub mod layout;

use std::cell::Cell;
use std::ffi::c_void;

use pulsar_core::history::{History, HistoryStore};
use pulsar_core::layout::{Rect, TextMeasure};
use pulsar_core::metric::ItemKind;
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Dwm::{
    DWM_WINDOW_CORNER_PREFERENCE, DWMWA_USE_IMMERSIVE_DARK_MODE, DWMWA_WINDOW_CORNER_PREFERENCE,
    DWMWCP_ROUND, DwmExtendFrameIntoClientArea, DwmSetWindowAttribute,
};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, BitBlt, EndPaint, GetDC, HDC, PAINTSTRUCT, ReleaseDC, SRCCOPY,
};
use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
use windows::Win32::UI::Controls::MARGINS;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, HTTRANSPARENT, HWND_TOPMOST, MA_NOACTIVATE,
    RegisterClassW, SW_HIDE, SW_SHOWNOACTIVATE, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
    SetWindowPos, ShowWindow, WINDOW_EX_STYLE, WM_MOUSEACTIVATE, WM_NCHITTEST, WM_PAINT, WNDCLASSW,
    WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};
use windows::core::{BOOL, Result, s, w};

use crate::render::{Renderer, Surface};
use crate::taskbar::placement::Rect32;
use crate::text::Text;
use crate::theme::{Color, Palette};
use content::{Block, PopupContent};
use layout::{Metrics, ellipsize, height, position};
use pulsar_core::display::{area_points, graph_max, metric_for};

const CLASS: windows::core::PCWSTR = w!("PulsarPopup");

thread_local! {
    /// The translucent popup's window and the bitmap it shows, for repaints
    /// Windows asks for between our own.
    static SHOWN: Cell<Option<(HWND, HDC, i32, i32)>> = const { Cell::new(None) };
}

unsafe extern "system" fn popup_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        // Mouse input passes through to whatever is underneath.
        WM_NCHITTEST => LRESULT(HTTRANSPARENT as isize),
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            let dc = unsafe { BeginPaint(hwnd, &mut ps) };
            if let Some((shown, source, w, h)) = SHOWN.get()
                && shown == hwnd
            {
                unsafe {
                    let _ = BitBlt(dc, 0, 0, w, h, Some(source), 0, 0, SRCCOPY);
                }
            }
            unsafe {
                let _ = EndPaint(hwnd, &ps);
            }
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wp, lp) },
    }
}

#[repr(C)]
struct AccentPolicy {
    state: u32,
    flags: u32,
    color: u32,
    animation: u32,
}

#[repr(C)]
struct CompositionData {
    attribute: u32,
    data: *mut c_void,
    size: usize,
}

type SetCompositionAttribute = unsafe extern "system" fn(HWND, *mut CompositionData) -> BOOL;

const WCA_ACCENT_POLICY: u32 = 19;
const ACCENT_ENABLE_ACRYLICBLURBEHIND: u32 = 4;
/// How much the popup's background colour covers the blur.
const TINT_ALPHA: f32 = 0.6;

/// Blurs what is behind the window under `tint`. The system backdrop
/// materials turn to a flat fill on windows that are not active, which a
/// popup never is, so this uses the older, undocumented accent policy.
fn set_acrylic(hwnd: HWND, tint: Color) -> bool {
    let Ok(user32) = (unsafe { GetModuleHandleW(w!("user32.dll")) }) else {
        return false;
    };
    let Some(proc) = (unsafe { GetProcAddress(user32, s!("SetWindowCompositionAttribute")) })
    else {
        return false;
    };
    let set: SetCompositionAttribute = unsafe { std::mem::transmute(proc) };
    let byte = |c: f32| (c.clamp(0.0, 1.0) * 255.0).round() as u32;
    let mut accent = AccentPolicy {
        state: ACCENT_ENABLE_ACRYLICBLURBEHIND,
        flags: 0,
        color: byte(TINT_ALPHA) << 24 | byte(tint.b) << 16 | byte(tint.g) << 8 | byte(tint.r),
        animation: 0,
    };
    let mut data = CompositionData {
        attribute: WCA_ACCENT_POLICY,
        data: (&mut accent as *mut AccentPolicy).cast(),
        size: size_of::<AccentPolicy>(),
    };
    unsafe { set(hwnd, &mut data) }.as_bool()
}

/// Blurs what is behind the whole window, with rounded corners; false
/// where Windows does not offer it.
fn apply_backdrop(hwnd: HWND) -> bool {
    let corners = DWMWCP_ROUND;
    let whole = MARGINS {
        cxLeftWidth: -1,
        cxRightWidth: -1,
        cyTopHeight: -1,
        cyBottomHeight: -1,
    };
    unsafe {
        DwmExtendFrameIntoClientArea(hwnd, &whole).is_ok()
            && DwmSetWindowAttribute(
                hwnd,
                DWMWA_WINDOW_CORNER_PREFERENCE,
                (&corners as *const DWM_WINDOW_CORNER_PREFERENCE).cast(),
                size_of::<DWM_WINDOW_CORNER_PREFERENCE>() as u32,
            )
            .is_ok()
            && set_acrylic(hwnd, Color::rgb(0x20, 0x20, 0x20))
    }
}

/// The border is drawn dark or light to match the taskbar.
fn set_dark(hwnd: HWND, dark: bool) {
    let dark = BOOL::from(dark);
    unsafe {
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_USE_IMMERSIVE_DARK_MODE,
            (&dark as *const BOOL).cast(),
            size_of::<BOOL>() as u32,
        );
    }
}

fn create_window(instance: HINSTANCE, extra: WINDOW_EX_STYLE) -> Result<HWND> {
    unsafe {
        CreateWindowExW(
            WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_TOPMOST | extra,
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
        )
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

/// A click-through, non-activating window reused for every hover. Its
/// bitmap and its own renderer are dropped while hidden, so the popup's
/// larger surfaces and glyph caches do not stay in memory once it closes.
///
/// A solid popup is a layered window drawn with per-pixel alpha. A
/// translucent one is an ordinary window whose client area is all frame,
/// so Windows shows its blur wherever the bitmap is clear.
pub struct Popup {
    hwnd: HWND,
    translucent: bool,
    surface: Option<Surface>,
    renderer: Option<Renderer>,
    pub open: Option<(ItemKind, Anchor)>,
}

impl Popup {
    pub fn create(instance: HINSTANCE, translucent: bool) -> Result<Self> {
        unsafe {
            RegisterClassW(&WNDCLASSW {
                lpfnWndProc: Some(popup_proc),
                hInstance: instance,
                lpszClassName: CLASS,
                ..Default::default()
            });
        }
        let mut hwnd = None;
        if translucent {
            let candidate = create_window(instance, WINDOW_EX_STYLE(0))?;
            if apply_backdrop(candidate) {
                hwnd = Some(candidate);
            } else {
                log::info!("no acrylic blur on this Windows; popups stay solid");
                unsafe {
                    let _ = DestroyWindow(candidate);
                }
            }
        }
        let translucent = hwnd.is_some();
        let hwnd = match hwnd {
            Some(hwnd) => hwnd,
            None => create_window(instance, WS_EX_LAYERED | WS_EX_TRANSPARENT)?,
        };
        crate::overlay::disable_transitions(hwnd);
        Ok(Self {
            hwnd,
            translucent,
            surface: None,
            renderer: None,
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
        if self.renderer.is_none() {
            self.renderer = Some(Renderer::new()?);
        }
        let renderer = self.renderer.as_mut().unwrap();
        let m = Metrics::new(anchor.dpi, style.font_px, text.line_height(style.font_px));
        let (w, h) = (m.width.ceil() as i32, height(content, &m) as i32);
        if self
            .surface
            .as_ref()
            .is_none_or(|s| s.width != w || s.height != h)
        {
            self.surface = Some(Surface::new(w, h)?);
        }
        let surface = self.surface.as_ref().unwrap();
        let backdrop = self.translucent;
        renderer.paint(surface, Color::rgb(0, 0, 0).with_alpha(0.0), |r| {
            draw(r, text, content, style, &m, w as f32, h as f32, backdrop)
        })?;
        let (x, y) = position(anchor.item, anchor.taskbar, anchor.monitor, w, h, m.scale);
        if self.translucent {
            set_dark(self.hwnd, !style.palette.light());
            set_acrylic(self.hwnd, style.palette.popup);
            unsafe {
                SetWindowPos(self.hwnd, Some(HWND_TOPMOST), x, y, w, h, SWP_NOACTIVATE)?;
            }
            SHOWN.set(Some((self.hwnd, surface.dc(), w, h)));
            if self.open.is_none() {
                unsafe {
                    let _ = ShowWindow(self.hwnd, SW_SHOWNOACTIVATE);
                }
            }
            unsafe {
                let dc = GetDC(Some(self.hwnd));
                let _ = BitBlt(dc, 0, 0, w, h, Some(surface.dc()), 0, 0, SRCCOPY);
                ReleaseDC(Some(self.hwnd), dc);
            }
        } else {
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
        }
        self.open = Some((content.kind, anchor));
        Ok(())
    }

    pub fn hide(&mut self) {
        if self.open.take().is_some() {
            unsafe {
                let _ = ShowWindow(self.hwnd, SW_HIDE);
            }
        }
        if SHOWN.get().is_some_and(|(h, ..)| h == self.hwnd) {
            SHOWN.set(None);
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
}

/// `backdrop`: Windows draws the background, border and corners.
#[allow(clippy::too_many_arguments)]
fn draw(
    r: &Renderer,
    text: &Text,
    content: &PopupContent,
    style: &Style,
    m: &Metrics,
    w: f32,
    h: f32,
    backdrop: bool,
) -> Result<()> {
    let p = style.palette;
    if !backdrop {
        let full = Rect {
            x: 0.0,
            y: 0.0,
            w,
            h,
        };
        r.fill_rounded(full, 8.0 * m.scale, p.popup_border)?;
        let inner = Rect {
            x: 1.0,
            y: 1.0,
            w: w - 2.0,
            h: h - 2.0,
        };
        r.fill_rounded(inner, 7.0 * m.scale, p.popup)?;
    }

    let body = text.format(m.font_px)?;
    let title = text.format(m.title_px)?;
    let right = w - m.pad;
    let accent = p.graph(content.kind, content.series[0]);
    let mut y = m.pad;

    let value = ellipsize(&content.value, (right - m.pad) * 0.5, m.title_px, text);
    let value_w = text.width(&value, m.title_px);
    let heading = ellipsize(
        content.title,
        right - value_w - m.pad - 8.0 * m.scale,
        m.title_px,
        text,
    );
    r.text_at(text, &title, &heading, m.pad, y, p.popup_text)?;
    r.text_at(text, &title, &value, right - value_w, y, accent)?;
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
        r.text_at(text, &body, error, m.pad, y, p.warning)?;
        y += m.line_h + m.block_gap;
    }

    let gap = 12.0 * m.scale;
    for block in &content.blocks {
        match block {
            Block::Rows { heading, rows } => {
                if let Some(heading) = heading {
                    r.text_at(text, &body, heading, m.pad, y, p.popup_label)?;
                    y += m.line_h;
                }
                for (label, value) in rows {
                    let value = ellipsize(value, (right - m.pad) * 0.7, m.font_px, text);
                    let value_w = text.width(&value, m.font_px);
                    r.text_at(text, &body, &value, right - value_w, y, p.popup_text)?;
                    let room = right - value_w - gap - m.pad;
                    let label = ellipsize(label, room, m.font_px, text);
                    r.text_at(text, &body, &label, m.pad, y, p.popup_text)?;
                    y += m.line_h;
                }
            }
            Block::Bars { values, .. } if values.is_empty() => continue,
            Block::Bars { heading, values } => {
                r.text_at(text, &body, heading, m.pad, y, p.popup_label)?;
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
                r.text_at(text, &body, heading, m.pad, y, p.popup_label)?;
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
                    let value_w = text.width(shown, m.font_px);
                    r.text_at(text, &body, shown, right - value_w, y, p.popup_text)?;
                    let label_x = m.pad + dot + 6.0 * m.scale;
                    r.text_at(text, &body, label, label_x, y, p.popup_text)?;
                    y += m.line_h;
                }
            }
        }
        y += m.block_gap;
    }
    Ok(())
}

/// The accent fading out part by part; the last part, what is left over,
/// takes the track colour.
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
        for translucent in [false, true] {
            check_resources_are_released(Popup::create(instance, translucent).unwrap());
        }
    }

    fn check_resources_are_released(mut popup: Popup) {
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
        if popup.translucent {
            assert!(SHOWN.get().is_some(), "kept for repaints while shown");
        }
        popup.hide();
        assert!(popup.open.is_none());
        assert!(SHOWN.get().is_none(), "no repaints from a freed bitmap");
        assert!(popup.surface.is_none(), "bitmap freed while hidden");
        assert!(
            popup.renderer.is_none(),
            "renderer and its caches freed while hidden"
        );
    }
}
