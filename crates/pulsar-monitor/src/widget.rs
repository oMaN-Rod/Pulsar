//! The desktop widget: a panel that stays open with a compact section per
//! item, in as many columns as it needs to fit on the screen.

use std::borrow::Cow;

use pulsar_core::config::{WidgetLayer, WidgetStyle};
use pulsar_core::display::{
    CoreLoad, NetworkFloor, area_points, core_loads, graph_max, meter_fraction, metric_for,
};
use pulsar_core::hardware::cpu::cached_topology;
use pulsar_core::history::{History, HistoryStore};
use pulsar_core::layout::{CellPart, Rect, TextMeasure};
use pulsar_core::metric::{ItemKind, Snapshot, Unit};
use pulsar_core::tr;
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM};
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DestroyWindow, EnumWindows, FindWindowExW, FindWindowW, GW_HWNDPREV,
    GWL_EXSTYLE, GetWindow, GetWindowLongPtrW, HWND_BOTTOM, HWND_NOTOPMOST, HWND_TOP, HWND_TOPMOST,
    IDC_ARROW, IDC_SIZEALL, LoadCursorW, RegisterClassW, SW_HIDE, SW_SHOWNOACTIVATE,
    SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SetCursor, SetWindowLongPtrW, SetWindowPos, ShowWindow,
    WINDOW_EX_STYLE, WNDCLASSW, WNDPROC, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
    WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};
use windows::core::{BOOL, Result, w};

use crate::popup::content::{Block, PopupContent};
use crate::popup::layout::ellipsize;
use crate::render::{CoreMarks, Renderer, Surface};
use crate::taskbar::placement::Rect32;
use crate::text::Text;
use crate::theme::{Color, Palette};

pub const CLASS: windows::core::PCWSTR = w!("PulsarWidget");

/// Distance kept from the work area's edges, and how close a dragged widget
/// must come to one to snap to it, in logical pixels.
pub const EDGE_GAP: f32 = 16.0;
const SNAP: f32 = 24.0;

pub fn register_class(instance: HINSTANCE, wndproc: WNDPROC) {
    let class = WNDCLASSW {
        lpfnWndProc: wndproc,
        hInstance: instance,
        lpszClassName: CLASS,
        hCursor: unsafe { LoadCursorW(None, IDC_ARROW) }.unwrap_or_default(),
        ..Default::default()
    };
    unsafe { RegisterClassW(&class) };
}

/// A non-activating layered window. On the desktop layer it sits just above
/// the desktop icons, and comes forward while the desktop is shown.
pub struct Widget {
    pub hwnd: HWND,
    surface: Option<Surface>,
    pub shown_at: Option<(i32, i32)>,
    layer: Option<WidgetLayer>,
    click_through: bool,
    /// Whether it was last raised over the desktop shown with Win+D.
    over_desktop: Option<bool>,
    /// Where each item was drawn, for right-clicks.
    hits: Vec<(Rect, ItemKind)>,
}

impl Widget {
    pub fn create(instance: HINSTANCE) -> Result<Self> {
        let hwnd = unsafe {
            CreateWindowExW(
                WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                CLASS,
                w!("Pulsar widget"),
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
            surface: None,
            shown_at: None,
            layer: None,
            click_through: false,
            over_desktop: None,
            hits: Vec::new(),
        })
    }

    pub fn dpi(&self) -> u32 {
        match unsafe { GetDpiForWindow(self.hwnd) } {
            0 => 96,
            dpi => dpi,
        }
    }

    pub fn size(&self) -> (i32, i32) {
        self.surface
            .as_ref()
            .map_or((0, 0), |s| (s.width, s.height))
    }

    /// Draws `contents` in one panel. `at` picks the top-left corner from
    /// the panel's size.
    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        &mut self,
        renderer: &mut Renderer,
        text: &Text,
        contents: &[(PopupContent, WidgetStyle)],
        snapshot: &Snapshot,
        palette: &Palette,
        history: &HistoryStore,
        look: &Look,
        at: impl FnOnce(i32, i32) -> (i32, i32),
    ) -> Result<()> {
        let d = Dims::new(text, look);
        let mut sections: Vec<Section> = contents
            .iter()
            .map(|(c, style)| Section::new(c, *style, snapshot, history))
            .collect();
        let plan = Plan::new(&mut sections, &d, look.max_h, text);
        self.hits = plan
            .placed
            .iter()
            .map(|p| (p.rect, sections[p.index].content.kind))
            .collect();
        let (w, h) = (plan.width.ceil() as i32, plan.height.ceil() as i32);
        if self
            .surface
            .as_ref()
            .is_none_or(|s| s.width != w || s.height != h)
        {
            self.surface = Some(Surface::new(w, h)?);
        }
        let surface = self.surface.as_ref().unwrap();
        let ctx = Ctx {
            text,
            palette,
            history,
            network_floor: look.network_floor,
            d: &d,
        };
        renderer.paint(surface, Color::rgb(0, 0, 0).with_alpha(0.0), |r| {
            let (wf, hf) = (w as f32, h as f32);
            let full = Rect {
                x: 0.0,
                y: 0.0,
                w: wf,
                h: hf,
            };
            let p = palette;
            let border = p
                .popup_border
                .with_alpha(p.popup_border.a * look.opacity.max(0.3));
            r.fill_rounded(full, 8.0 * d.scale, border)?;
            let inner = Rect {
                x: 1.0,
                y: 1.0,
                w: wf - 2.0,
                h: hf - 2.0,
            };
            // Never fully clear, so the whole panel takes the mouse.
            r.fill_rounded(
                inner,
                7.0 * d.scale,
                p.popup.with_alpha(look.opacity.max(1.0 / 255.0)),
            )?;
            for placed in &plan.placed {
                ctx.section(r, &sections[placed.index], placed.rect, placed.value_w)?;
                if placed.divider {
                    let line = Rect {
                        x: placed.rect.x,
                        y: placed.rect.y + placed.rect.h + d.between / 2.0,
                        w: placed.rect.w,
                        h: d.scale.max(1.0),
                    };
                    r.fill_rounded(line, 0.0, p.popup_border)?;
                }
            }
            Ok(())
        })?;
        let (x, y) = at(w, h);
        renderer.present(self.hwnd, surface, x, y)?;
        if self.shown_at.is_none() {
            unsafe {
                let _ = ShowWindow(self.hwnd, SW_SHOWNOACTIVATE);
            }
        }
        self.shown_at = Some((x, y));
        Ok(())
    }

    /// The item drawn at a point in the window.
    pub fn hit(&self, x: i32, y: i32) -> Option<ItemKind> {
        let (x, y) = (x as f32, y as f32);
        self.hits
            .iter()
            .find(|(r, _)| x >= r.x && x < r.x + r.w && y >= r.y && y < r.y + r.h)
            .map(|&(_, kind)| kind)
    }

    /// Moves the drawn panel without redrawing it.
    pub fn move_to(&mut self, renderer: &Renderer, x: i32, y: i32) -> Result<()> {
        if let Some(surface) = &self.surface {
            renderer.present(self.hwnd, surface, x, y)?;
            self.shown_at = Some((x, y));
        }
        Ok(())
    }

    pub fn hide(&mut self) {
        if self.shown_at.take().is_some() {
            unsafe {
                let _ = ShowWindow(self.hwnd, SW_HIDE);
            }
        }
        self.surface = None;
        self.over_desktop = None;
    }

    pub fn set_click_through(&mut self, on: bool) {
        if on == self.click_through {
            return;
        }
        self.click_through = on;
        unsafe {
            let style = WINDOW_EX_STYLE(GetWindowLongPtrW(self.hwnd, GWL_EXSTYLE) as u32);
            let style = if on {
                style | WS_EX_TRANSPARENT
            } else {
                style & !WS_EX_TRANSPARENT
            };
            SetWindowLongPtrW(self.hwnd, GWL_EXSTYLE, style.0 as isize);
        }
    }

    /// Keeps the widget in its layer; `foreground` is the active window.
    pub fn arrange(&mut self, layer: WidgetLayer, foreground: HWND) {
        if self.shown_at.is_none() {
            return;
        }
        let flags = SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE;
        let changed = self.layer != Some(layer);
        self.layer = Some(layer);
        if layer == WidgetLayer::OnTop {
            if changed {
                unsafe {
                    let _ = SetWindowPos(self.hwnd, Some(HWND_TOPMOST), 0, 0, 0, 0, flags);
                }
            }
            return;
        }
        if changed {
            unsafe {
                let _ = SetWindowPos(self.hwnd, Some(HWND_NOTOPMOST), 0, 0, 0, 0, flags);
            }
            self.over_desktop = None;
        }
        let desktop = desktop_window();
        let over = desktop.is_some_and(|d| d == foreground);
        if self.over_desktop == Some(over) {
            return;
        }
        self.over_desktop = Some(over);
        let after = if over {
            HWND_TOP
        } else {
            match desktop.and_then(|d| unsafe { GetWindow(d, GW_HWNDPREV) }.ok()) {
                Some(prev) if prev == self.hwnd => return,
                Some(prev) if !is_topmost(prev) => prev,
                Some(_) => HWND_TOP,
                None => HWND_BOTTOM,
            }
        };
        unsafe {
            let _ = SetWindowPos(self.hwnd, Some(after), 0, 0, 0, 0, flags);
        }
    }

    /// Forgets the z-order it set, after Explorer restarts.
    pub fn rearrange(&mut self) {
        self.layer = None;
        self.over_desktop = None;
    }
}

impl Drop for Widget {
    fn drop(&mut self) {
        unsafe {
            let _ = DestroyWindow(self.hwnd);
        }
    }
}

/// What the widget looks like, in physical pixels.
pub struct Look {
    /// One column's width.
    pub column_w: f32,
    /// The tallest the widget may be before it adds a column.
    pub max_h: f32,
    pub font_px: f32,
    pub opacity: f32,
    pub scale: f32,
    pub network_floor: NetworkFloor,
}

const DIALS_PER_ROW: usize = 3;
const MAX_DIAL_DIP: f32 = 76.0;

struct Dims {
    scale: f32,
    pad: f32,
    gap: f32,
    /// Space between sections, with a divider in its middle.
    between: f32,
    font_px: f32,
    line_h: f32,
    small_px: f32,
    small_h: f32,
    /// A strip of per-core cells or shares.
    strip_h: f32,
    tile_pad: f32,
    tile_graph_h: f32,
    column_w: f32,
    wall_h: f32,
    dial: f32,
}

impl Dims {
    fn new(text: &Text, look: &Look) -> Self {
        let s = look.scale;
        let small_px = look.font_px * 0.88;
        let inner = look.column_w - 2.0 * 10.0 * s;
        let dial_cell = (inner - 2.0 * 8.0 * s) / DIALS_PER_ROW as f32;
        Self {
            scale: s,
            pad: 10.0 * s,
            gap: 8.0 * s,
            between: 9.0 * s,
            font_px: look.font_px,
            line_h: text.line_height(look.font_px),
            small_px,
            small_h: text.line_height(small_px),
            strip_h: 6.0 * s,
            tile_pad: 6.0 * s,
            tile_graph_h: 18.0 * s,
            column_w: look.column_w,
            wall_h: 64.0 * s,
            dial: dial_cell.min(MAX_DIAL_DIP * s).max(1.0),
        }
    }

    fn tile_h(&self) -> f32 {
        2.0 * self.tile_pad + self.small_h + self.line_h + self.tile_graph_h
    }

    fn dial_h(&self) -> f32 {
        self.dial + self.gap / 2.0 + self.small_h
    }
}

/// An item's popup content, flattened for the widget's compact layout.
struct Section<'a> {
    content: &'a PopupContent,
    style: WidgetStyle,
    /// Detail rows, shown two to a line.
    pairs: Vec<(&'a str, &'a str)>,
    /// Per-core load, 0–100.
    cores: &'a [f64],
    /// Shares of a whole, such as memory in use, cached and free.
    shares: Vec<f64>,
    /// The current value as a fraction of its scale, for a meter.
    meter: Option<f32>,
    /// Per-core load and peak, for the core wall.
    wall: Vec<CoreLoad>,
    /// Indexes into `pairs` by line: two side by side, or one that needs
    /// the whole width. Set by `lay_out_pairs`.
    rows: Vec<(usize, Option<usize>)>,
}

impl<'a> Section<'a> {
    fn new(
        content: &'a PopupContent,
        style: WidgetStyle,
        snapshot: &Snapshot,
        history: &HistoryStore,
    ) -> Self {
        let mut pairs = Vec::new();
        let mut cores: &[f64] = &[];
        let mut shares = Vec::new();
        for block in &content.blocks {
            match block {
                Block::Rows { rows, .. } => {
                    pairs.extend(rows.iter().map(|(l, v)| (l.as_str(), v.as_str())));
                }
                Block::Bars { values, .. } => cores = values,
                Block::Composition { parts, .. } => {
                    shares = parts.iter().map(|(_, v, _)| v.max(0.0)).collect();
                    pairs.extend(parts.iter().map(|(l, _, v)| (l.as_str(), v.as_str())));
                }
            }
        }
        let is_wall = style == WidgetStyle::CoreWall;
        if is_wall {
            cores = &[];
        }
        let wall = if is_wall && snapshot.item_error(content.kind).is_none() {
            core_loads(snapshot, history)
        } else {
            Vec::new()
        };
        let key = metric_for(content.kind, content.series[0]);
        let meter = match key.unit() {
            Unit::Percent | Unit::Celsius => snapshot
                .get(key)
                .filter(|v| v.is_finite())
                .map(|v| (v / 100.0).clamp(0.0, 1.0) as f32),
            _ => None,
        };
        Self {
            content,
            style,
            pairs,
            cores,
            shares,
            meter,
            wall,
            rows: Vec::new(),
        }
    }

    /// Pairs share a line when both fit in half the width.
    fn lay_out_pairs(&mut self, d: &Dims, inner: f32, text: &dyn TextMeasure) {
        let half = (inner - d.gap) / 2.0;
        let fits = |&(label, value): &(&str, &str)| {
            text.width(label, d.small_px) + text.width(value, d.small_px) + 4.0 * d.scale <= half
        };
        let wide: Vec<bool> = self.pairs.iter().map(|p| !fits(p)).collect();
        self.rows = pair_rows(&wide);
    }

    /// Where efficient cores begin, on a hybrid CPU.
    fn split(&self) -> Option<usize> {
        cached_topology().performance_threads(self.wall.len())
    }

    /// Height in the line styles.
    fn height(&self, d: &Dims) -> f32 {
        let mut h = d.line_h;
        if self.style == WidgetStyle::CoreWall {
            h += d.gap / 2.0 + d.wall_h;
            if self.split().is_some() {
                h += d.small_h;
            }
        }
        if !self.cores.is_empty() {
            h += d.gap / 2.0 + d.strip_h;
        }
        if !self.shares.is_empty() {
            h += d.gap / 2.0 + d.strip_h;
        }
        if !self.rows.is_empty() {
            h += d.gap / 2.0 + self.rows.len() as f32 * d.small_h;
        }
        if self.content.error.is_some() {
            h += d.small_h;
        }
        h
    }
}

/// Lines of detail pairs in order: narrow pairs two to a line, a wide one
/// alone. A narrow pair left waiting goes alone before a wide one.
fn pair_rows(wide: &[bool]) -> Vec<(usize, Option<usize>)> {
    let mut rows = Vec::new();
    let mut waiting = None;
    for (i, &w) in wide.iter().enumerate() {
        if w {
            if let Some(first) = waiting.take() {
                rows.push((first, None));
            }
            rows.push((i, None));
        } else if let Some(first) = waiting.take() {
            rows.push((first, Some(i)));
        } else {
            waiting = Some(i);
        }
    }
    if let Some(first) = waiting {
        rows.push((first, None));
    }
    rows
}

struct Placed {
    index: usize,
    rect: Rect,
    /// Room kept for values on the right of the section's column, so
    /// graphs line up.
    value_w: f32,
    divider: bool,
}

struct Plan {
    width: f32,
    height: f32,
    placed: Vec<Placed>,
}

impl Plan {
    fn new(sections: &mut [Section], d: &Dims, max_h: f32, text: &dyn TextMeasure) -> Self {
        let inner = d.column_w - 2.0 * d.pad;
        for s in sections.iter_mut() {
            s.lay_out_pairs(d, inner, text);
        }
        let styles: Vec<WidgetStyle> = sections.iter().map(|s| s.style).collect();
        let wide: Vec<bool> = sections.iter().map(|s| wide_tile(s.content.kind)).collect();
        let units = units(&styles, &wide);
        let heights: Vec<f32> = units
            .iter()
            .map(|u| match styles[u[0]] {
                WidgetStyle::Tiles => d.tile_h(),
                WidgetStyle::Dials => d.dial_h(),
                _ => sections[u[0]].height(d),
            })
            .collect();
        let columns = columns(&heights, d.between, max_h - 2.0 * d.pad);
        let mut placed = Vec::new();
        let mut tallest: f32 = 0.0;
        for (c, column) in columns.iter().enumerate() {
            let x = d.pad + c as f32 * (inner + 2.0 * d.pad);
            let value_w = column
                .iter()
                .flat_map(|&u| &units[u])
                .filter(|&&i| per_row(styles[i]) == 1)
                .map(|&i| text.width(&sections[i].content.value, d.font_px))
                .fold(0.0, f32::max)
                .min(inner * 0.3);
            let mut y = d.pad;
            for (n, &u) in column.iter().enumerate() {
                let h = heights[u];
                let is_line = per_row(styles[units[u][0]]) == 1;
                let next_is_line = column
                    .get(n + 1)
                    .is_some_and(|&next| per_row(styles[units[next][0]]) == 1);
                let across = per_row(styles[units[u][0]]) as f32;
                let cell_w = (inner - (across - 1.0) * d.gap) / across;
                for (k, &index) in units[u].iter().enumerate() {
                    let rect = if is_line || wide[index] {
                        Rect { x, y, w: inner, h }
                    } else {
                        Rect {
                            x: x + k as f32 * (cell_w + d.gap),
                            y,
                            w: cell_w,
                            h,
                        }
                    };
                    placed.push(Placed {
                        index,
                        rect,
                        value_w,
                        divider: is_line && next_is_line,
                    });
                }
                y += h;
                if n + 1 < column.len() {
                    y += d.between;
                }
            }
            tallest = tallest.max(y - d.pad);
        }
        let n = columns.len().max(1) as f32;
        Self {
            width: n * d.column_w,
            height: tallest + 2.0 * d.pad,
            placed,
        }
    }
}

/// Network shows two rates, which need a tile the whole width.
fn wide_tile(kind: ItemKind) -> bool {
    kind == ItemKind::Network
}

/// How many sections in a style share a row.
fn per_row(style: WidgetStyle) -> usize {
    match style {
        WidgetStyle::Tiles => 2,
        WidgetStyle::Dials => DIALS_PER_ROW,
        WidgetStyle::Sparklines | WidgetStyle::Meters | WidgetStyle::CoreWall => 1,
    }
}

/// Groups sections into what stacks in a column: one section in a line
/// style, a wide tile, or a row of tiles or dials side by side.
fn units(styles: &[WidgetStyle], wide: &[bool]) -> Vec<Vec<usize>> {
    let mut out: Vec<Vec<usize>> = Vec::new();
    let shares = |i: usize| per_row(styles[i]) > 1 && !wide[i];
    for i in 0..styles.len() {
        let joins = shares(i)
            && out.last().is_some_and(|last| {
                last.len() < per_row(styles[i]) && shares(last[0]) && styles[last[0]] == styles[i]
            });
        match out.last_mut() {
            Some(last) if joins => last.push(i),
            _ => out.push(vec![i]),
        }
    }
    out
}

/// Splits sections into columns, in order, starting a new one when the next
/// section would make the current one taller than `max_h`.
fn columns(heights: &[f32], between: f32, max_h: f32) -> Vec<Vec<usize>> {
    let mut out: Vec<Vec<usize>> = Vec::new();
    let mut used = 0.0;
    for (i, &h) in heights.iter().enumerate() {
        match out.last_mut() {
            Some(column) if used + between + h <= max_h => {
                column.push(i);
                used += between + h;
            }
            _ => {
                out.push(vec![i]);
                used = h;
            }
        }
    }
    out
}

/// What drawing a section needs besides the section.
struct Ctx<'a> {
    text: &'a Text,
    palette: &'a Palette,
    history: &'a HistoryStore,
    network_floor: NetworkFloor,
    d: &'a Dims,
}

impl Ctx<'_> {
    fn section(&self, r: &Renderer, s: &Section, at: Rect, value_w: f32) -> Result<()> {
        match s.style {
            WidgetStyle::Tiles => self.tile(r, s, at),
            WidgetStyle::Dials => self.dial(r, s, at),
            _ => self.lines(r, s, at, value_w),
        }
    }

    fn accent(&self, s: &Section) -> Color {
        self.palette.graph(s.content.kind, s.content.series[0])
    }

    /// `column_value_w` is the room the column keeps for values, so graphs
    /// end in line.
    fn lines(&self, r: &Renderer, s: &Section, at: Rect, column_value_w: f32) -> Result<()> {
        let (d, p, t) = (self.d, self.palette, self.text);
        let c = s.content;
        let body = t.format(d.font_px)?;
        let small = t.format(d.small_px)?;
        let accent = self.accent(s);
        let value = ellipsize(&c.value, at.w * 0.7, d.font_px, t);
        let value_w = t.width(&value, d.font_px);
        let value_x = at.x + at.w - value_w;
        r.text_at(t, &body, &value, value_x, at.y, accent)?;
        // The name comes first; the graph takes what is left, ending in line
        // with the column's other graphs.
        let graph_end = value_x.min(at.x + at.w - column_value_w) - d.gap;
        let title_w = t.width(c.title, d.font_px);
        let graph_w = (at.w * 0.32).min(graph_end - (at.x + title_w + d.gap));
        let middle = Rect {
            x: graph_end - graph_w,
            y: at.y + d.line_h * 0.15,
            w: graph_w,
            h: d.line_h * 0.7,
        };
        let has_graph = s.style != WidgetStyle::CoreWall
            && middle.w >= 24.0 * d.scale
            && (s.style != WidgetStyle::Meters || s.meter.is_some());
        let title_end = if has_graph { middle.x } else { value_x } - d.gap;
        let title = ellipsize(c.title, title_end - at.x, d.font_px, t);
        r.text_at(t, &body, &title, at.x, at.y, p.popup_text)?;
        if has_graph {
            match s.style {
                WidgetStyle::Meters => {
                    if let Some(fraction) = s.meter {
                        let track = Rect {
                            y: middle.y + middle.h * 0.25,
                            h: middle.h * 0.5,
                            ..middle
                        };
                        r.fill_rounded(track, track.h / 2.0, p.popup_border)?;
                        let fill = Rect {
                            w: track.w * fraction,
                            ..track
                        };
                        r.fill_rounded(fill, track.h / 2.0, accent)?;
                    }
                }
                _ => self.graph(r, s, middle, 1.2)?,
            }
        }
        let mut y = at.y + d.line_h;
        if s.style == WidgetStyle::CoreWall {
            y += d.gap / 2.0;
            let wall = Rect {
                x: at.x,
                y,
                w: at.w,
                h: d.wall_h,
            };
            let marks = CoreMarks {
                split: s.split(),
                track: p.popup_border,
                peak: p.popup_text,
                scale: d.scale,
            };
            r.draw_core_bars(&s.wall, wall, marks, |load| {
                p.graph_at(c.kind, c.series[0], Some(load))
            })?;
            y += d.wall_h;
            if let Some(split) = s.split() {
                let efficient = s.wall.len() - split;
                let half = (at.w - d.gap) / 2.0;
                let first = ellipsize(&tr!("P-cores, {} threads", split), half, d.small_px, t);
                let second = ellipsize(&tr!("E-cores, {} threads", efficient), half, d.small_px, t);
                r.text_at(t, &small, &first, at.x, y, p.popup_label)?;
                let x = at.x + at.w - t.width(&second, d.small_px);
                r.text_at(t, &small, &second, x, y, p.popup_label)?;
                y += d.small_h;
            }
        }
        if !s.cores.is_empty() {
            y += d.gap / 2.0;
            let n = s.cores.len() as f32;
            let cell_gap = if at.w / n >= 4.0 * d.scale {
                d.scale.max(1.0)
            } else {
                0.0
            };
            let cell_w = (at.w - (n - 1.0) * cell_gap) / n;
            for (i, v) in s.cores.iter().enumerate() {
                let cell = Rect {
                    x: at.x + i as f32 * (cell_w + cell_gap),
                    y,
                    w: cell_w,
                    h: d.strip_h,
                };
                let load = (v / 100.0).clamp(0.0, 1.0) as f32;
                r.fill_rounded(cell, d.scale, p.popup_border)?;
                r.fill_rounded(cell, d.scale, accent.with_alpha(0.15 + 0.85 * load))?;
            }
            y += d.strip_h;
        }
        if !s.shares.is_empty() {
            y += d.gap / 2.0;
            let track = Rect {
                x: at.x,
                y,
                w: at.w,
                h: d.strip_h,
            };
            r.fill_rounded(track, d.strip_h / 2.0, p.popup_border)?;
            let whole: f64 = s.shares.iter().sum();
            let mut x = at.x;
            let last = s.shares.len().saturating_sub(1);
            for (i, share) in s.shares.iter().enumerate() {
                if whole <= 0.0 || (i == last && last > 0) {
                    break;
                }
                let w = at.w * (share / whole) as f32;
                let alpha = 1.0 - 0.6 * i as f32 / last.max(1) as f32;
                r.fill_rounded(Rect { x, w, ..track }, d.scale, accent.with_alpha(alpha))?;
                x += w;
            }
            y += d.strip_h;
        }
        if !s.rows.is_empty() {
            y += d.gap / 2.0;
            let half = (at.w - d.gap) / 2.0;
            for &(first, second) in &s.rows {
                let cells = match second {
                    Some(second) => vec![(first, at.x, half), (second, at.x + half + d.gap, half)],
                    None => vec![(first, at.x, at.w)],
                };
                for (i, x, w) in cells {
                    let (label, value) = s.pairs[i];
                    let label_w = t.width(label, d.small_px).min(w * 0.4);
                    let value = ellipsize(value, w - label_w - 4.0 * d.scale, d.small_px, t);
                    let value_w = t.width(&value, d.small_px);
                    r.text_at(t, &small, &value, x + w - value_w, y, p.popup_text)?;
                    let room = w - value_w - 4.0 * d.scale;
                    let label = ellipsize(label, room, d.small_px, t);
                    r.text_at(t, &small, &label, x, y, p.popup_label)?;
                }
                y += d.small_h;
            }
        }
        if let Some(error) = &c.error {
            let error = ellipsize(error, at.w, d.small_px, t);
            r.text_at(t, &small, &error, at.x, y, p.warning)?;
        }
        Ok(())
    }

    fn tile(&self, r: &Renderer, s: &Section, at: Rect) -> Result<()> {
        let (d, p, t) = (self.d, self.palette, self.text);
        let c = s.content;
        r.fill_rounded(at, 6.0 * d.scale, p.popup_border)?;
        let body = t.format(d.font_px)?;
        let small = t.format(d.small_px)?;
        let inner_x = at.x + d.tile_pad;
        let inner_w = at.w - 2.0 * d.tile_pad;
        let title = ellipsize(c.title, inner_w, d.small_px, t);
        r.text_at(t, &small, &title, inner_x, at.y + d.tile_pad, p.popup_label)?;
        let value = ellipsize(&c.value, inner_w, d.font_px, t);
        let value_y = at.y + d.tile_pad + d.small_h;
        r.text_at(t, &body, &value, inner_x, value_y, self.accent(s))?;
        let graph = Rect {
            x: inner_x,
            y: value_y + d.line_h,
            w: inner_w,
            h: d.tile_graph_h,
        };
        self.graph(r, s, graph, 1.0)
    }

    /// A ring per reading with its value in the middle and its name below.
    /// A wide dial has the ring on the left and the text beside it, as a
    /// value such as two network rates doesn't fit inside a ring.
    fn dial(&self, r: &Renderer, s: &Section, at: Rect) -> Result<()> {
        let (d, p, t) = (self.d, self.palette, self.text);
        let c = s.content;
        let wide = wide_tile(c.kind);
        let center_x = if wide {
            at.x + d.dial / 2.0
        } else {
            at.x + at.w / 2.0
        };
        let center = (center_x, at.y + d.dial / 2.0);
        let (histories, max) = self.series(s);
        let stroke = d.dial * if histories.len() > 1 { 0.09 } else { 0.11 };
        let mut radius = d.dial / 2.0 - stroke / 2.0;
        let readings: Vec<(f32, Color)> = if histories.is_empty() {
            vec![(0.0, self.accent(s))]
        } else {
            histories
                .iter()
                .map(|(part, history)| {
                    let latest = history.latest();
                    (
                        meter_fraction(latest.unwrap_or(f64::NAN), max),
                        p.graph_at(c.kind, *part, latest),
                    )
                })
                .collect()
        };
        for (fraction, color) in readings {
            r.draw_ring(center, radius, stroke, fraction, color, p.popup_border)?;
            radius -= stroke + d.scale;
        }
        let small = t.format(d.small_px)?;
        if wide {
            let x = at.x + d.dial + d.gap;
            let room = at.right() - x;
            let middle = at.y + d.dial / 2.0;
            let title = ellipsize(c.title, room, d.small_px, t);
            r.text_at(t, &small, &title, x, middle - d.small_h, p.popup_label)?;
            let value = ellipsize(&c.value, room, d.font_px, t);
            let body = t.format(d.font_px)?;
            r.text_at(t, &body, &value, x, middle, p.popup_text)?;
        } else {
            let inside = (d.dial - 2.0 * stroke - 4.0 * d.scale).max(1.0);
            let px = [d.font_px, d.small_px, d.small_px * 0.8]
                .into_iter()
                .find(|&px| t.width(&c.value, px) <= inside)
                .unwrap_or(d.small_px * 0.8);
            let value = ellipsize(&c.value, inside, px, t);
            let x = center.0 - t.width(&value, px) / 2.0;
            let y = center.1 - t.line_height(px) / 2.0;
            r.text_at(t, &t.format(px)?, &value, x, y, p.popup_text)?;
            let title = ellipsize(c.title, at.w, d.small_px, t);
            let x = center.0 - t.width(&title, d.small_px) / 2.0;
            let y = at.y + d.dial + d.gap / 2.0;
            r.text_at(t, &small, &title, x, y, p.popup_label)?;
        }
        Ok(())
    }

    fn series<'s>(&'s self, s: &Section) -> (Vec<(CellPart, Cow<'s, History>)>, f64) {
        let c = s.content;
        let histories: Vec<(CellPart, Cow<History>)> = c
            .series
            .iter()
            .filter_map(|&part| {
                let key = metric_for(c.kind, part);
                let history = self.history.get(key)?;
                Some((part, self.network_floor.series(key, history)))
            })
            .collect();
        let unit = metric_for(c.kind, c.series[0]).unit();
        let max = graph_max(
            unit,
            &histories.iter().map(|(_, h)| &**h).collect::<Vec<_>>(),
        );
        (histories, max)
    }

    fn graph(&self, r: &Renderer, s: &Section, rect: Rect, stroke: f32) -> Result<()> {
        let c = s.content;
        let (histories, max) = self.series(s);
        for (i, (part, history)) in histories.iter().enumerate() {
            r.series(
                &area_points(history, rect, max),
                self.palette.graph(c.kind, *part),
                i == 0,
                stroke * self.d.scale,
            )?;
        }
        Ok(())
    }
}

pub fn drag_cursor() {
    unsafe {
        if let Ok(cursor) = LoadCursorW(None, IDC_SIZEALL) {
            SetCursor(Some(cursor));
        }
    }
}

fn is_topmost(hwnd: HWND) -> bool {
    let style = unsafe { GetWindowLongPtrW(hwnd, GWL_EXSTYLE) } as u32;
    style & WS_EX_TOPMOST.0 != 0
}

fn has_icons(hwnd: HWND) -> bool {
    unsafe { FindWindowExW(Some(hwnd), None, w!("SHELLDLL_DefView"), None) }.is_ok()
}

/// The top-level window holding the desktop icons: Program Manager, or the
/// worker window Explorer moves them into while the wallpaper changes.
fn desktop_window() -> Option<HWND> {
    let progman = unsafe { FindWindowW(w!("Progman"), None) }.ok();
    if let Some(p) = progman
        && has_icons(p)
    {
        return Some(p);
    }
    unsafe extern "system" fn visit(hwnd: HWND, found: LPARAM) -> BOOL {
        if crate::taskbar::class_name(hwnd) == "WorkerW" && has_icons(hwnd) {
            unsafe { *(found.0 as *mut Option<HWND>) = Some(hwnd) };
            return false.into();
        }
        true.into()
    }
    let mut found: Option<HWND> = None;
    unsafe {
        let _ = EnumWindows(Some(visit), LPARAM(&raw mut found as isize));
    }
    found.or(progman)
}

/// The top-left corner for a `w` × `h` widget: the saved one kept inside
/// `work`, else the top right of it.
pub fn place(
    saved: Option<(i32, i32)>,
    (w, h): (i32, i32),
    work: Rect32,
    scale: f32,
) -> (i32, i32) {
    let gap = (EDGE_GAP * scale) as i32;
    let (x, y) = saved.unwrap_or((work.right - w - gap, work.top + gap));
    (
        x.clamp(work.left, (work.right - w).max(work.left)),
        y.clamp(work.top, (work.bottom - h).max(work.top)),
    )
}

/// Pulls a dragged widget onto `work`'s edges when it comes close.
pub fn snap((x, y): (i32, i32), (w, h): (i32, i32), work: Rect32, scale: f32) -> (i32, i32) {
    let gap = (EDGE_GAP * scale) as i32;
    let reach = (SNAP * scale) as i32;
    let pull = |pos: i32, size: i32, low: i32, high: i32| {
        if (pos - (low + gap)).abs() < reach {
            low + gap
        } else if (pos + size - (high - gap)).abs() < reach {
            high - gap - size
        } else {
            pos
        }
    };
    (
        pull(x, w, work.left, work.right),
        pull(y, h, work.top, work.bottom),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const WORK: Rect32 = Rect32 {
        left: 0,
        top: 0,
        right: 1920,
        bottom: 1040,
    };

    #[test]
    fn a_new_widget_goes_top_right() {
        assert_eq!(place(None, (300, 400), WORK, 1.0), (1920 - 300 - 16, 16));
        assert_eq!(place(None, (300, 400), WORK, 2.0), (1920 - 300 - 32, 32));
    }

    #[test]
    fn a_saved_position_is_kept_on_screen() {
        assert_eq!(place(Some((100, 200)), (300, 400), WORK, 1.0), (100, 200));
        assert_eq!(place(Some((5000, -50)), (300, 400), WORK, 1.0), (1620, 0));
    }

    #[test]
    fn sections_fill_a_column_before_starting_the_next() {
        assert_eq!(columns(&[10.0, 10.0, 10.0], 2.0, 34.0), [vec![0, 1, 2]]);
        assert_eq!(
            columns(&[10.0, 10.0, 10.0], 2.0, 33.0),
            [vec![0, 1], vec![2]]
        );
        assert_eq!(
            columns(&[50.0, 10.0], 2.0, 20.0),
            [vec![0], vec![1]],
            "a section taller than the screen still gets a column"
        );
        assert!(columns(&[], 2.0, 20.0).is_empty());
    }

    #[test]
    fn tiles_pair_up_and_lines_stand_alone() {
        use WidgetStyle::{Meters, Sparklines, Tiles};
        let narrow = [false; 6];
        assert_eq!(
            units(&[Tiles, Tiles, Tiles, Sparklines, Tiles, Meters], &narrow),
            [vec![0, 1], vec![2], vec![3], vec![4], vec![5]]
        );
        assert_eq!(
            units(&[Tiles, Tiles, Tiles, Tiles], &[false, true, false, false]),
            [vec![0], vec![1], vec![2, 3]],
            "a wide tile stands alone"
        );
        assert!(units(&[], &[]).is_empty());
    }

    #[test]
    fn dials_share_rows_of_three_with_their_own_style_only() {
        use WidgetStyle::{CoreWall, Dials, Tiles};
        let narrow = [false; 7];
        assert_eq!(
            units(
                &[Dials, Dials, Dials, Dials, Tiles, Dials, CoreWall],
                &narrow
            ),
            [vec![0, 1, 2], vec![3], vec![4], vec![5], vec![6]]
        );
        assert_eq!(
            units(&[Dials, Dials, Dials], &[false, true, false]),
            [vec![0], vec![1], vec![2]],
            "a wide dial stands alone"
        );
    }

    fn sample() -> (Snapshot, HistoryStore) {
        use pulsar_core::metric::MetricKey;
        let mut history = HistoryStore::new(60);
        let mut last = Snapshot::default();
        for tick in 0..12 {
            let mut s = Snapshot::default();
            s.set(MetricKey::CpuTotal, 38.0);
            s.set(MetricKey::CpuTempC, 61.0);
            s.set(MetricKey::MemUsedPercent, 41.0);
            s.set(MetricKey::GpuUtil, 12.0);
            s.set(MetricKey::NetDownBps, 300_000.0);
            s.set(MetricKey::NetUpBps, 40_000.0);
            for core in 0..24u16 {
                let load = f64::from((core * 7 + tick * 11) % 100);
                s.set(MetricKey::CpuCore(core), load);
            }
            history.record(&s);
            last = s;
        }
        (last, history)
    }

    #[test]
    fn dials_and_the_core_wall_draw_inside_the_panel() {
        use pulsar_core::config::Config;
        let text = Text::new(None, false).unwrap();
        let config = Config::default();
        let (snapshot, history) = sample();
        let kinds = [
            (ItemKind::Cpu, WidgetStyle::CoreWall),
            (ItemKind::Cpu, WidgetStyle::Dials),
            (ItemKind::CpuTemp, WidgetStyle::Dials),
            (ItemKind::Ram, WidgetStyle::Dials),
            (ItemKind::Gpu, WidgetStyle::Dials),
            (ItemKind::Network, WidgetStyle::Dials),
        ];
        let contents: Vec<(PopupContent, WidgetStyle)> = kinds
            .iter()
            .map(|&(kind, style)| {
                let options = crate::popup::content::Options::new(&config, kind, &snapshot.gpus);
                let content =
                    crate::popup::content::build_with(kind, &snapshot, &history, &options);
                (content, style)
            })
            .collect();
        let look = Look {
            column_w: 300.0,
            max_h: 1000.0,
            font_px: 12.0,
            opacity: 1.0,
            scale: 1.0,
            network_floor: NetworkFloor::default(),
        };
        let d = Dims::new(&text, &look);
        let mut sections: Vec<Section> = contents
            .iter()
            .map(|(c, style)| Section::new(c, *style, &snapshot, &history))
            .collect();
        let plan = Plan::new(&mut sections, &d, look.max_h, &text);
        for placed in &plan.placed {
            let r = placed.rect;
            assert!(r.x >= 0.0 && r.right() <= plan.width, "{:?}", r);
            assert!(r.y >= 0.0 && r.y + r.h <= plan.height, "{:?}", r);
        }
        assert_eq!(
            plan.placed[1].rect.y, plan.placed[3].rect.y,
            "dials share a row"
        );
        assert!(
            plan.placed[5].rect.w > plan.placed[1].rect.w,
            "network is wide"
        );
        let palette = Palette::new(false, &config, None, &crate::theme::Theme::windows());
        let ctx = Ctx {
            text: &text,
            palette: &palette,
            history: &history,
            network_floor: look.network_floor,
            d: &d,
        };
        let surface = Surface::new(plan.width as i32, plan.height.ceil() as i32).unwrap();
        let mut renderer = Renderer::new().unwrap();
        renderer
            .paint(&surface, Color::rgb(28, 28, 28), |r| {
                for placed in &plan.placed {
                    ctx.section(r, &sections[placed.index], placed.rect, placed.value_w)?;
                }
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn narrow_pairs_share_lines_and_wide_ones_take_a_whole_line() {
        assert_eq!(pair_rows(&[false, false, false]), [(0, Some(1)), (2, None)]);
        assert_eq!(
            pair_rows(&[false, true, false, false]),
            [(0, None), (1, None), (2, Some(3))]
        );
        assert!(pair_rows(&[]).is_empty());
    }

    #[test]
    fn dragging_near_an_edge_snaps_to_it() {
        assert_eq!(snap((30, 500), (300, 400), WORK, 1.0), (16, 500));
        assert_eq!(snap((1600, 625), (300, 400), WORK, 1.0), (1604, 624));
        assert_eq!(snap((800, 500), (300, 400), WORK, 1.0), (800, 500));
    }
}
