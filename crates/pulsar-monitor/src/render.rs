use std::borrow::Cow;

use pulsar_core::config::{DisplayMode, GraphStyle};
use pulsar_core::hardware::cpu::cached_topology;
use pulsar_core::history::{History, HistoryStore};
use pulsar_core::layout::{
    Cell, CellPart, Layout, Rect, StackedLabel, TextMeasure, TileGraphic, TileLook, TileText,
    cell_icon, cell_label, heat_rows, icon_gap, inline_slots, label_width, network_pair,
    stacked_label, tile_regions,
};
use pulsar_core::metric::{ItemKind, Snapshot};
use windows::Win32::Foundation::{D2DERR_RECREATE_TARGET, HWND, POINT, RECT, SIZE};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D_RECT_F, D2D_SIZE_F, D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F, D2D1_FIGURE_BEGIN_FILLED,
    D2D1_FIGURE_BEGIN_HOLLOW, D2D1_FIGURE_END_CLOSED, D2D1_FIGURE_END_OPEN, D2D1_PIXEL_FORMAT,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1_ARC_SEGMENT, D2D1_ARC_SIZE_LARGE, D2D1_ARC_SIZE_SMALL, D2D1_CAP_STYLE_FLAT,
    D2D1_CAP_STYLE_ROUND, D2D1_DASH_STYLE_SOLID, D2D1_DRAW_TEXT_OPTIONS_NONE, D2D1_ELLIPSE,
    D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1_FEATURE_LEVEL_DEFAULT, D2D1_LINE_JOIN_ROUND,
    D2D1_RENDER_TARGET_PROPERTIES, D2D1_RENDER_TARGET_TYPE_SOFTWARE, D2D1_RENDER_TARGET_USAGE_NONE,
    D2D1_ROUNDED_RECT, D2D1_STROKE_STYLE_PROPERTIES, D2D1_SWEEP_DIRECTION_CLOCKWISE,
    D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE, D2D1CreateFactory, ID2D1DCRenderTarget, ID2D1Factory,
    ID2D1PathGeometry, ID2D1SolidColorBrush, ID2D1StrokeStyle,
};
use windows::Win32::Graphics::DirectWrite::IDWriteTextFormat;
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Gdi::{
    AC_SRC_ALPHA, AC_SRC_OVER, BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BLENDFUNCTION,
    CreateCompatibleDC, CreateDIBSection, DIB_RGB_COLORS, DeleteDC, DeleteObject, HBITMAP, HDC,
    HGDIOBJ, SelectObject,
};
use windows::Win32::UI::WindowsAndMessaging::{ULW_ALPHA, UpdateLayeredWindow};
use windows::core::Result;
use windows_numerics::Vector2;

use crate::text::Text;
use crate::theme::{Color, Palette};
use pulsar_core::display::{
    CoreLoad, NetworkFloor, area_points, bar_rects, core_bars, core_loads, floored_cell_value,
    graph_max, heat_cells, lit_segments, meter_fraction, metric_for, ring_arc, ring_companion,
    segment_rects,
};

/// Below this a tile's ring has no room for a second ring inside it, in DIPs.
const MIN_COMPANION_RING_DIP: f32 = 24.0;
const SEGMENTS: usize = 10;
const SEGMENT_DIP: f32 = 6.0;
/// Efficient cores are drawn a little darker than performance cores.
const EFFICIENT_ALPHA: f32 = 0.72;

/// A 32-bit premultiplied DIB that Direct2D draws into and
/// `UpdateLayeredWindow` presents with per-pixel alpha.
pub struct Surface {
    dc: HDC,
    bitmap: HBITMAP,
    previous: HGDIOBJ,
    pub width: i32,
    pub height: i32,
}

impl Surface {
    pub fn new(width: i32, height: i32) -> Result<Self> {
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width,
                biHeight: -height,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        unsafe {
            let dc = CreateCompatibleDC(None);
            let mut bits = std::ptr::null_mut();
            let bitmap = CreateDIBSection(Some(dc), &info, DIB_RGB_COLORS, &mut bits, None, 0)
                .inspect_err(|_| {
                    let _ = DeleteDC(dc);
                })?;
            let previous = SelectObject(dc, bitmap.into());
            Ok(Self {
                dc,
                bitmap,
                previous,
                width,
                height,
            })
        }
    }
}

impl Surface {
    /// The memory DC the bitmap is selected into, for copying it elsewhere.
    pub fn dc(&self) -> HDC {
        self.dc
    }
}

impl Drop for Surface {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.dc, self.previous);
            let _ = DeleteObject(self.bitmap.into());
            let _ = DeleteDC(self.dc);
        }
    }
}

pub struct Frame<'a> {
    pub layout: &'a Layout,
    pub mode: DisplayMode,
    pub palette: &'a Palette,
    pub snapshot: &'a Snapshot,
    pub history: &'a HistoryStore,
    pub dpi: u32,
    pub text: TileText<'a>,
    pub graph_inline: bool,
    /// Graph mode: how each item draws its graph; `None` draws text only.
    pub styles: &'a [(ItemKind, Option<GraphStyle>)],
    pub network_floor: NetworkFloor,
}

impl Frame<'_> {
    fn value_text(&self, kind: ItemKind, part: CellPart) -> String {
        floored_cell_value(kind, part, self.snapshot, self.network_floor)
    }

    fn style(&self, kind: ItemKind) -> Option<GraphStyle> {
        self.styles
            .iter()
            .find(|(k, _)| *k == kind)
            .map_or(Some(GraphStyle::Area), |(_, s)| *s)
    }

    /// A graph already shows the load, so only text without one takes it.
    fn value_color(&self, kind: ItemKind, part: CellPart, graphed: bool) -> Color {
        if graphed {
            return self.palette.text;
        }
        let failed = self.snapshot.item_error(kind).is_some();
        let key = metric_for(kind, part);
        let value = self
            .snapshot
            .get(key)
            .filter(|_| !failed)
            .map(|v| self.network_floor.value(key, v));
        self.palette.value_at(kind, part, value)
    }
}

/// Every configured item's graph style, for `Frame::styles`.
pub fn graph_styles(config: &pulsar_core::config::Config) -> Vec<(ItemKind, Option<GraphStyle>)> {
    config
        .items
        .iter()
        .map(|i| (i.kind, i.graph.then_some(i.style)))
        .collect()
}

/// How core bars mark what they show.
#[derive(Clone, Copy)]
pub struct CoreMarks {
    pub split: Option<usize>,
    pub track: Color,
    pub peak: Color,
    pub scale: f32,
}

pub struct Renderer {
    factory: ID2D1Factory,
    target: ID2D1DCRenderTarget,
    round_stroke: ID2D1StrokeStyle,
    brush: ID2D1SolidColorBrush,
}

impl Renderer {
    pub fn new() -> Result<Self> {
        let factory: ID2D1Factory =
            unsafe { D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)? };
        let target = create_target(&factory)?;
        let round = D2D1_STROKE_STYLE_PROPERTIES {
            startCap: D2D1_CAP_STYLE_ROUND,
            endCap: D2D1_CAP_STYLE_ROUND,
            dashCap: D2D1_CAP_STYLE_FLAT,
            lineJoin: D2D1_LINE_JOIN_ROUND,
            miterLimit: 10.0,
            dashStyle: D2D1_DASH_STYLE_SOLID,
            dashOffset: 0.0,
        };
        let round_stroke = unsafe { factory.CreateStrokeStyle(&round, None)? };
        let brush = create_brush(&target)?;
        Ok(Self {
            factory,
            target,
            round_stroke,
            brush,
        })
    }

    /// The renderer's one brush, set to `color`, so a fill allocates nothing.
    fn brush(&self, color: Color) -> &ID2D1SolidColorBrush {
        unsafe { self.brush.SetColor(&d2d(color)) };
        &self.brush
    }

    pub fn draw(&mut self, surface: &Surface, frame: &Frame, text: &Text) -> Result<()> {
        self.paint(surface, frame.palette.hit, |r| {
            r.draw_panel(frame, surface)?;
            match frame.mode {
                DisplayMode::Graph => r.draw_graph_cells(frame, text),
                DisplayMode::Text => r.draw_text_cells(frame, text),
            }
        })
    }

    /// Binds `surface`, clears it to `clear`, runs `draw`, and ends the frame,
    /// recreating the target if Direct2D asks for it.
    pub fn paint(
        &mut self,
        surface: &Surface,
        clear: Color,
        draw: impl FnOnce(&Self) -> Result<()>,
    ) -> Result<()> {
        let bounds = RECT {
            left: 0,
            top: 0,
            right: surface.width,
            bottom: surface.height,
        };
        let rt = &self.target;
        unsafe {
            rt.BindDC(surface.dc, &bounds)?;
            rt.BeginDraw();
            rt.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);
            rt.Clear(Some(&d2d(clear)));
        }
        let drawn = draw(self);
        let ended = unsafe { self.target.EndDraw(None, None) };
        if let Err(e) = &ended
            && e.code() == D2DERR_RECREATE_TARGET
        {
            self.target = create_target(&self.factory)?;
            self.brush = create_brush(&self.target)?;
        }
        drawn.and(ended)
    }

    pub fn present(&self, hwnd: HWND, surface: &Surface, x: i32, y: i32) -> Result<()> {
        self.present_alpha(hwnd, surface, Some((x, y)), 255)
    }

    /// Presents with the whole window's opacity scaled by `alpha`, for fades.
    pub fn present_alpha(
        &self,
        hwnd: HWND,
        surface: &Surface,
        at: Option<(i32, i32)>,
        alpha: u8,
    ) -> Result<()> {
        let at = at.map(|(x, y)| POINT { x, y });
        let blend = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: alpha,
            AlphaFormat: AC_SRC_ALPHA as u8,
        };
        unsafe {
            UpdateLayeredWindow(
                hwnd,
                None,
                at.as_ref().map(std::ptr::from_ref),
                Some(&SIZE {
                    cx: surface.width,
                    cy: surface.height,
                }),
                Some(surface.dc),
                Some(&POINT::default()),
                Default::default(),
                Some(&blend),
                ULW_ALPHA,
            )
        }
    }

    fn draw_panel(&self, frame: &Frame, surface: &Surface) -> Result<()> {
        let Some(panel) = frame.palette.panel else {
            return Ok(());
        };
        let bounds = Rect {
            x: 0.0,
            y: 0.0,
            w: surface.width as f32,
            h: surface.height as f32,
        };
        self.fill_rounded(bounds, 6.0 * frame.dpi as f32 / 96.0, panel)
    }

    /// A cell's icon (when enabled) followed by its label text.
    #[allow(clippy::too_many_arguments)]
    fn draw_label(
        &self,
        text: &Text,
        frame: &Frame,
        font_px: f32,
        kind: ItemKind,
        part: CellPart,
        x: f32,
        y: f32,
    ) -> Result<()> {
        let color = frame.palette.label_for(kind, part);
        let mut x = x;
        if frame.text.icons {
            let glyph = cell_icon(kind, part);
            let format = text.icon_format(font_px)?;
            let dy = text.icon_offset(glyph, font_px);
            self.text_at(text, &format, glyph, x, y + dy, color)?;
            x += text.icon_width(glyph, font_px) + icon_gap(font_px);
        }
        let TileText {
            labels,
            icons,
            names,
            ..
        } = frame.text;
        let label = cell_label(kind, part, labels, icons, names);
        if !label.is_empty() {
            self.text_at(text, &text.format(font_px)?, &label, x, y, color)?;
        }
        Ok(())
    }

    /// One letter per line, centred in `area`, with the icon on top.
    fn draw_stacked_label(
        &self,
        text: &Text,
        frame: &Frame,
        cell: &Cell,
        stack: &StackedLabel,
        area: Rect,
    ) -> Result<()> {
        let color = frame.palette.label_for(cell.kind, cell.part);
        let px = stack.px;
        let line_h = text.line_height(px);
        let total = stack.lines() as f32 * stack.step;
        let top = area.y + (area.h - total) / 2.0;
        let at = |line: usize| top + line as f32 * stack.step + (stack.step - line_h) / 2.0;
        let mut line = 0;
        if let Some(icon) = stack.icon {
            let format = text.icon_format(px)?;
            let x = area.x + (area.w - text.icon_width(icon, px)) / 2.0;
            let y = at(0) + text.icon_offset(icon, px);
            self.text_at(text, &format, icon, x, y, color)?;
            line = 1;
        }
        let format = text.format(px)?;
        for glyph in &stack.glyphs {
            let x = area.x + (area.w - text.width(glyph, px)) / 2.0;
            self.text_at(text, &format, glyph, x, at(line), color)?;
            line += 1;
        }
        Ok(())
    }

    fn draw_text_cells(&self, frame: &Frame, text: &Text) -> Result<()> {
        let font_px = frame.layout.font_px;
        let format = text.format(font_px)?;
        let scale = frame.dpi as f32 / 96.0;
        for cell in &frame.layout.cells {
            if frame.text.vertical_labels {
                let stack = stacked_label(
                    cell.kind,
                    cell.part,
                    frame.text,
                    font_px,
                    cell.rect.h,
                    scale,
                    text,
                );
                let area = Rect {
                    w: stack.width,
                    ..cell.rect
                };
                self.draw_stacked_label(text, frame, cell, &stack, area)?;
            } else {
                self.draw_label(
                    text,
                    frame,
                    font_px,
                    cell.kind,
                    cell.part,
                    cell.rect.x,
                    cell.rect.y,
                )?;
            }
            let value = frame.value_text(cell.kind, cell.part);
            let color = frame.value_color(cell.kind, cell.part, false);
            self.text_at(text, &format, &value, cell.value_x, cell.value_y, color)?;
        }
        Ok(())
    }

    fn draw_graph_cells(&self, frame: &Frame, text: &Text) -> Result<()> {
        let scale = frame.dpi as f32 / 96.0;
        for cell in &frame.layout.cells {
            let small_px = cell.text_px;
            let small = text.format(small_px)?;
            let line_h = small_px * 1.3;
            self.fill_rounded(cell.rect, 4.0 * scale, frame.palette.tile)?;
            let style = frame.style(cell.kind);
            let graphed = style.is_some();
            let look = TileLook {
                text: frame.text,
                inline: frame.graph_inline,
                vertical_bar: frame.layout.vertical,
                scale,
            };
            let graphic = style.map_or(TileGraphic::Behind, |s| TileGraphic::of(s, 0, false));
            let regions = tile_regions(
                cell.rect, cell.kind, cell.part, graphic, look, small_px, text,
            );
            match (style, regions.ring, regions.cores) {
                (Some(GraphStyle::Ring), Some(ring), _) => {
                    self.draw_rings(frame, cell, ring, scale)?;
                }
                (Some(GraphStyle::Cores), _, Some(area)) => {
                    self.draw_cores(frame, cell, area, scale)?;
                }
                (Some(GraphStyle::Heat), _, Some(area)) => {
                    self.draw_heat(frame, cell, area, scale)?;
                }
                (Some(GraphStyle::Segments), ..) => {
                    if let Some(area) = regions.strip {
                        self.draw_segments(frame, cell, area, scale)?;
                    }
                }
                (Some(GraphStyle::Spark), ..) => {
                    if let Some(area) = regions.strip {
                        self.draw_spark(frame, cell, area, scale)?;
                    }
                }
                (Some(style), ..) => self.draw_series(frame, cell, style, regions.graph, scale)?,
                (None, ..) => {}
            }
            if let Some((stack, area)) = &regions.stack {
                self.draw_stacked_label(text, frame, cell, stack, *area)?;
            }
            let color = |part| frame.value_color(cell.kind, part, graphed);
            let pad = 4.0 * scale;
            let (top, bottom) = (
                cell.rect.y + pad * 0.5,
                cell.rect.y + cell.rect.h - line_h - pad * 0.5,
            );
            let x = regions.text_x;
            if frame.graph_inline {
                let y = cell.rect.y + (cell.rect.h - text.line_height(small_px)) / 2.0;
                let (slots, _) =
                    inline_slots(cell.kind, cell.part, frame.text, small_px, scale, text);
                for slot in slots {
                    let label_x = x + slot.label_x;
                    self.draw_label(text, frame, small_px, cell.kind, slot.part, label_x, y)?;
                    let value = frame.value_text(cell.kind, slot.part);
                    let value_x = x + slot.value_x;
                    self.text_at(text, &small, &value, value_x, y, color(slot.part))?;
                }
            } else if let Some([first, second]) =
                network_pair(cell.kind, cell.part, frame.text.upload_on_top)
            {
                for (part, y) in [(first, top), (second, bottom)] {
                    self.draw_label(text, frame, small_px, cell.kind, part, x, y)?;
                    let TileText {
                        labels,
                        icons,
                        names,
                        ..
                    } = frame.text;
                    let label_w =
                        label_width(cell.kind, part, labels, icons, names, small_px, text);
                    let value = frame.value_text(cell.kind, part);
                    let value_x = x + label_w + icon_gap(small_px);
                    self.text_at(text, &small, &value, value_x, y, color(part))?;
                }
            } else if let Some(strip) = regions.strip.filter(|_| regions.strip_below) {
                let y = cell.rect.y + 2.0 * scale;
                if !frame.text.vertical_labels {
                    self.draw_label(text, frame, small_px, cell.kind, cell.part, x, y)?;
                }
                let value = frame.value_text(cell.kind, cell.part);
                let value_x = if frame.text.vertical_labels {
                    x
                } else {
                    strip.right() - text.width(&value, small_px)
                };
                self.text_at(text, &small, &value, value_x, y, color(cell.part))?;
            } else if frame.text.vertical_labels {
                let y = cell.rect.y + (cell.rect.h - text.line_height(small_px)) / 2.0;
                let value = frame.value_text(cell.kind, cell.part);
                self.text_at(text, &small, &value, x, y, color(cell.part))?;
            } else {
                self.draw_label(text, frame, small_px, cell.kind, cell.part, x, top)?;
                let value = frame.value_text(cell.kind, cell.part);
                self.text_at(text, &small, &value, x, bottom, color(cell.part))?;
            }
        }
        Ok(())
    }

    /// A cell's readings with their histories, and the ceiling they share.
    fn cell_series<'a>(frame: &'a Frame, cell: &Cell) -> (Vec<(CellPart, Cow<'a, History>)>, f64) {
        let pair = network_pair(cell.kind, cell.part, frame.text.upload_on_top);
        let parts: &[CellPart] = match &pair {
            Some(pair) => pair,
            None => std::slice::from_ref(&cell.part),
        };
        let histories: Vec<(CellPart, Cow<History>)> = parts
            .iter()
            .filter_map(|&p| {
                let key = metric_for(cell.kind, p);
                let history = frame.history.get(key)?;
                Some((p, frame.network_floor.series(key, history)))
            })
            .collect();
        let unit = metric_for(cell.kind, parts[0]).unit();
        let max = graph_max(
            unit,
            &histories.iter().map(|(_, h)| &**h).collect::<Vec<_>>(),
        );
        (histories, max)
    }

    /// The reading's ring, with its temperature inside when there is room,
    /// as on the details page. A pair has a ring per direction.
    fn draw_rings(&self, frame: &Frame, cell: &Cell, area: Rect, scale: f32) -> Result<()> {
        let palette = frame.palette;
        let (histories, max) = Self::cell_series(frame, cell);
        let mut rings: Vec<(f32, Color)> = histories
            .iter()
            .map(|(part, history)| {
                let latest = history.latest();
                (
                    meter_fraction(latest.unwrap_or(f64::NAN), max),
                    palette.graph_at(cell.kind, *part, latest),
                )
            })
            .collect();
        if rings.is_empty() {
            rings.push((0.0, palette.graph(cell.kind, cell.part)));
        }
        if rings.len() == 1 && area.w >= MIN_COMPANION_RING_DIP * scale {
            let companion = ring_companion(cell.kind).and_then(|kind| {
                let value = frame.snapshot.get(metric_for(kind, cell.part))?;
                frame
                    .snapshot
                    .item_error(kind)
                    .is_none()
                    .then_some((kind, value))
            });
            if let Some((kind, value)) = companion {
                rings.push((
                    meter_fraction(value, 100.0),
                    palette.graph_at(kind, cell.part, Some(value)),
                ));
            }
        }
        let center = (area.x + area.w / 2.0, area.y + area.h / 2.0);
        let stroke = area.w * if rings.len() == 1 { 0.11 } else { 0.095 };
        let track = palette.text.with_alpha(0.14);
        let mut radius = area.w / 2.0 - stroke / 2.0;
        for (fraction, color) in rings {
            self.draw_ring(center, radius, stroke, fraction, color, track)?;
            radius -= stroke + scale;
        }
        Ok(())
    }

    pub(crate) fn draw_ring(
        &self,
        center: (f32, f32),
        radius: f32,
        stroke: f32,
        fraction: f32,
        color: Color,
        track: Color,
    ) -> Result<()> {
        let v = |(x, y): (f32, f32)| Vector2 { X: x, Y: y };
        let ellipse = D2D1_ELLIPSE {
            point: v(center),
            radiusX: radius,
            radiusY: radius,
        };
        unsafe {
            self.target
                .DrawEllipse(&ellipse, self.brush(track), stroke, None);
        }
        let Some(arc) = ring_arc(center, radius, fraction) else {
            return Ok(());
        };
        unsafe {
            let geometry = self.factory.CreatePathGeometry()?;
            let sink = geometry.Open()?;
            sink.BeginFigure(v(arc.start), D2D1_FIGURE_BEGIN_HOLLOW);
            sink.AddArc(&D2D1_ARC_SEGMENT {
                point: v(arc.end),
                size: D2D_SIZE_F {
                    width: radius,
                    height: radius,
                },
                rotationAngle: 0.0,
                sweepDirection: D2D1_SWEEP_DIRECTION_CLOCKWISE,
                arcSize: if arc.large {
                    D2D1_ARC_SIZE_LARGE
                } else {
                    D2D1_ARC_SIZE_SMALL
                },
            });
            sink.EndFigure(D2D1_FIGURE_END_OPEN);
            sink.Close()?;
            self.target
                .DrawGeometry(&geometry, self.brush(color), stroke, &self.round_stroke);
        }
        Ok(())
    }

    fn cell_core_loads(frame: &Frame, cell: &Cell) -> Vec<CoreLoad> {
        if frame.snapshot.item_error(cell.kind).is_some() {
            return Vec::new();
        }
        core_loads(frame.snapshot, frame.history)
    }

    fn draw_cores(&self, frame: &Frame, cell: &Cell, area: Rect, scale: f32) -> Result<()> {
        let palette = frame.palette;
        let loads = Self::cell_core_loads(frame, cell);
        let marks = CoreMarks {
            split: cached_topology().performance_threads(loads.len()),
            track: palette.text.with_alpha(0.1),
            peak: palette.text.with_alpha(0.85),
            scale,
        };
        self.draw_core_bars(&loads, area, marks, |load| {
            palette.graph_at(cell.kind, cell.part, Some(load))
        })
    }

    /// Bars for `loads` in `area`, with peak marks and core types set apart.
    pub(crate) fn draw_core_bars(
        &self,
        loads: &[CoreLoad],
        area: Rect,
        marks: CoreMarks,
        color: impl Fn(f64) -> Color,
    ) -> Result<()> {
        let scale = marks.scale;
        let gap = scale.round().max(1.0);
        let radius = scale;
        for bar in core_bars(loads, area, marks.split, gap, scale.round().max(1.0)) {
            let mut fill = color(bar.load);
            if bar.efficient {
                fill = fill.with_alpha(fill.a * EFFICIENT_ALPHA);
            }
            self.fill_rounded(bar.track, radius, marks.track)?;
            self.fill_rounded(bar.fill, radius, fill)?;
            if let Some(y) = bar.peak_y {
                let mark = Rect {
                    y,
                    h: scale.round().max(1.0),
                    ..bar.track
                };
                self.fill_rect(mark, marks.peak)?;
            }
        }
        Ok(())
    }

    fn draw_heat(&self, frame: &Frame, cell: &Cell, area: Rect, scale: f32) -> Result<()> {
        let loads: Vec<f64> = Self::cell_core_loads(frame, cell)
            .iter()
            .map(|c| c.load)
            .collect();
        // On a vertical bar the cells sit behind the text, which must stay readable.
        let strength = if frame.layout.vertical { 0.45 } else { 1.0 };
        let rows = heat_rows(area.h, scale);
        for (rect, load) in heat_cells(&loads, area, rows, scale.round().max(1.0)) {
            let color = frame.palette.heat_at(load);
            self.fill_rounded(rect, 2.0 * scale, color.with_alpha(color.a * strength))?;
        }
        Ok(())
    }

    /// One row of segments per reading, centred in its share of `area`.
    fn draw_segments(&self, frame: &Frame, cell: &Cell, area: Rect, scale: f32) -> Result<()> {
        let palette = frame.palette;
        let (histories, max) = Self::cell_series(frame, cell);
        let rows = histories.len().max(1) as f32;
        let height = (SEGMENT_DIP * scale)
            .min(area.h / rows - scale)
            .round()
            .max(1.0);
        let pitch = area.h / rows;
        let unlit = palette.text.with_alpha(0.14);
        let gap = scale.round().max(1.0);
        for (row, (part, history)) in histories.iter().enumerate() {
            let band = Rect {
                y: (area.y + row as f32 * pitch + (pitch - height) / 2.0).round(),
                h: height,
                ..area
            };
            let latest = history.latest();
            let segments = segment_rects(band, SEGMENTS, gap);
            let fraction = meter_fraction(latest.unwrap_or(f64::NAN), max);
            let lit = lit_segments(fraction, segments.len());
            let count = segments.len() as f64;
            for (i, segment) in segments.into_iter().enumerate() {
                let color = if i < lit {
                    let step = max * (i as f64 + 0.5) / count;
                    palette.graph_at(cell.kind, *part, Some(step))
                } else {
                    unlit
                };
                self.fill_rounded(segment, scale, color)?;
            }
        }
        Ok(())
    }

    fn draw_spark(&self, frame: &Frame, cell: &Cell, area: Rect, scale: f32) -> Result<()> {
        let (histories, max) = Self::cell_series(frame, cell);
        for (part, history) in &histories {
            let color = frame.palette.graph_at(cell.kind, *part, history.latest());
            self.series(&area_points(history, area, max), color, false, 1.25 * scale)?;
        }
        Ok(())
    }

    fn draw_series(
        &self,
        frame: &Frame,
        cell: &Cell,
        style: GraphStyle,
        area: Rect,
        scale: f32,
    ) -> Result<()> {
        let (histories, max) = Self::cell_series(frame, cell);
        if histories.is_empty() {
            return Ok(());
        }
        let palette = frame.palette;
        if style == GraphStyle::Meter {
            let band_h = area.h / histories.len() as f32;
            for (i, (part, history)) in histories.iter().enumerate() {
                let latest = history.latest();
                let color = palette.graph_at(cell.kind, *part, latest);
                let fill = meter_fraction(latest.unwrap_or(f64::NAN), max);
                let band = Rect {
                    x: area.x,
                    y: area.y + i as f32 * band_h,
                    w: area.w * fill,
                    h: band_h,
                };
                self.fill_rounded(band, 2.0 * scale, color.with_alpha(0.35))?;
                let edge = 2.0 * scale;
                let bar = Rect {
                    y: band.y + band.h - edge,
                    h: edge,
                    ..band
                };
                self.fill_rounded(bar, edge / 2.0, color)?;
            }
            return Ok(());
        }
        for (i, (part, history)) in histories.iter().enumerate() {
            let color = palette.graph_at(cell.kind, *part, history.latest());
            if style == GraphStyle::Bars && i == 0 {
                for (bar, value) in bar_rects(history, area, max, 2.0 * scale, 1.0 * scale) {
                    let color = palette.graph_at(cell.kind, *part, Some(value));
                    self.fill_rect(bar, color.with_alpha(0.6))?;
                }
                continue;
            }
            let fill = style == GraphStyle::Area && i == 0;
            self.series(&area_points(history, area, max), color, fill, 1.25 * scale)?;
        }
        Ok(())
    }

    fn fill_rect(&self, r: Rect, color: Color) -> Result<()> {
        unsafe {
            self.target.FillRectangle(&rect_f(r), self.brush(color));
        }
        Ok(())
    }

    /// Draws an `area_points` outline: optionally filled, always stroked on top.
    pub(crate) fn series(
        &self,
        points: &[(f32, f32)],
        color: Color,
        fill: bool,
        width: f32,
    ) -> Result<()> {
        if points.len() < 3 {
            return Ok(());
        }
        if fill {
            let area = self.path(points, true)?;
            let brush = self.brush(color.with_alpha(0.35));
            unsafe { self.target.FillGeometry(&area, brush, None) };
        }
        let line = self.path(&points[1..points.len() - 1], false)?;
        unsafe {
            self.target
                .DrawGeometry(&line, self.brush(color), width, None);
        }
        Ok(())
    }

    fn path(&self, points: &[(f32, f32)], closed: bool) -> Result<ID2D1PathGeometry> {
        let v = |&(x, y): &(f32, f32)| Vector2 { X: x, Y: y };
        unsafe {
            let geometry = self.factory.CreatePathGeometry()?;
            let sink = geometry.Open()?;
            let begin = if closed {
                D2D1_FIGURE_BEGIN_FILLED
            } else {
                D2D1_FIGURE_BEGIN_HOLLOW
            };
            sink.BeginFigure(v(&points[0]), begin);
            if points.len() > 1 {
                sink.AddLines(&points[1..].iter().map(v).collect::<Vec<_>>());
            }
            sink.EndFigure(if closed {
                D2D1_FIGURE_END_CLOSED
            } else {
                D2D1_FIGURE_END_OPEN
            });
            sink.Close()?;
            Ok(geometry)
        }
    }

    pub(crate) fn fill_rounded(&self, r: Rect, radius: f32, color: Color) -> Result<()> {
        let rounded = D2D1_ROUNDED_RECT {
            rect: rect_f(r),
            radiusX: radius,
            radiusY: radius,
        };
        unsafe {
            self.target
                .FillRoundedRectangle(&rounded, self.brush(color));
        }
        Ok(())
    }

    pub(crate) fn text_at(
        &self,
        text: &Text,
        format: &IDWriteTextFormat,
        s: &str,
        x: f32,
        y: f32,
        color: Color,
    ) -> Result<()> {
        let layout = text.layout(s, format)?;
        unsafe {
            self.target.DrawTextLayout(
                Vector2 {
                    X: x.round(),
                    Y: y.round(),
                },
                &layout,
                self.brush(color),
                D2D1_DRAW_TEXT_OPTIONS_NONE,
            );
        }
        Ok(())
    }
}

/// Software rendering: a hardware target loads the GPU driver stack, which
/// costs ~50 MB of private memory for a small overlay redrawn once a second.
fn create_target(factory: &ID2D1Factory) -> Result<ID2D1DCRenderTarget> {
    let props = D2D1_RENDER_TARGET_PROPERTIES {
        r#type: D2D1_RENDER_TARGET_TYPE_SOFTWARE,
        pixelFormat: D2D1_PIXEL_FORMAT {
            format: DXGI_FORMAT_B8G8R8A8_UNORM,
            alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
        },
        dpiX: 96.0,
        dpiY: 96.0,
        usage: D2D1_RENDER_TARGET_USAGE_NONE,
        minLevel: D2D1_FEATURE_LEVEL_DEFAULT,
    };
    unsafe { factory.CreateDCRenderTarget(&props) }
}

fn create_brush(target: &ID2D1DCRenderTarget) -> Result<ID2D1SolidColorBrush> {
    unsafe { target.CreateSolidColorBrush(&d2d(Color::rgb(0, 0, 0)), None) }
}

fn d2d(c: Color) -> D2D1_COLOR_F {
    D2D1_COLOR_F {
        r: c.r,
        g: c.g,
        b: c.b,
        a: c.a,
    }
}

fn rect_f(r: Rect) -> D2D_RECT_F {
    D2D_RECT_F {
        left: r.x,
        top: r.y,
        right: r.x + r.w,
        bottom: r.y + r.h,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pulsar_core::config::{Config, DisplayMode, NetworkShow};
    use pulsar_core::layout::{ItemNames, LayoutInput, TextSpacing, compute_layout};
    use pulsar_core::metric::MetricKey;

    fn pixels(surface: &Surface) -> Vec<u32> {
        let mut out = vec![0u32; (surface.width * surface.height) as usize];
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: surface.width,
                biHeight: -surface.height,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        unsafe {
            windows::Win32::Graphics::Gdi::GetDIBits(
                surface.dc,
                surface.bitmap,
                0,
                surface.height as u32,
                Some(out.as_mut_ptr().cast()),
                &info as *const _ as *mut _,
                DIB_RGB_COLORS,
            );
        }
        out
    }

    fn render(mode: DisplayMode) -> (Surface, Layout) {
        render_with_samples(mode, &[10.0, 50.0, 90.0])
    }

    fn render_with_samples(mode: DisplayMode, samples: &[f64]) -> (Surface, Layout) {
        render_with(mode, samples, &Config::default())
    }

    fn render_with(mode: DisplayMode, samples: &[f64], config: &Config) -> (Surface, Layout) {
        render_on(mode, samples, config, false, None)
    }

    fn render_on(
        mode: DisplayMode,
        samples: &[f64],
        config: &Config,
        fit_font: bool,
        vertical_bar_px: Option<f32>,
    ) -> (Surface, Layout) {
        let text = Text::new(None, false).unwrap();
        let items = pulsar_core::layout::item_specs(config, &Default::default());
        let names = ItemNames::resolve(config);
        let layout = compute_layout(
            &items,
            LayoutInput {
                mode,
                taskbar_height_px: 48.0,
                fit_font,
                dpi: 96,
                font_size_pt: 9.0,
                labels: config.display.labels,
                icons: config.display.icons,
                graph_inline: config.display.graph_inline,
                vertical_labels: config.display.vertical_labels,
                upload_on_top: config.display.upload_on_top,
                vertical_bar_px,
                text_spacing: TextSpacing::default(),
                names: &names,
            },
            &text,
        );
        let mut snapshot = Snapshot::default();
        snapshot.set(MetricKey::CpuTotal, 50.0);
        snapshot.set(MetricKey::CpuTempC, 60.0);
        for core in 0..24 {
            snapshot.set(MetricKey::CpuCore(core), f64::from(core) * 4.0);
        }
        let mut history = HistoryStore::new(60);
        for &v in samples {
            let mut s = Snapshot::default();
            s.set(MetricKey::CpuTotal, v);
            history.record(&s);
        }
        let palette = Palette::new(false, config, None, &crate::theme::Theme::windows());
        let surface = Surface::new(layout.width.ceil() as i32, layout.height as i32).unwrap();
        let mut renderer = Renderer::new().unwrap();
        let frame = Frame {
            layout: &layout,
            mode,
            palette: &palette,
            snapshot: &snapshot,
            history: &history,
            dpi: 96,
            text: TileText {
                labels: config.display.labels,
                icons: config.display.icons,
                vertical_labels: config.display.vertical_labels,
                upload_on_top: config.display.upload_on_top,
                names: &names,
            },
            graph_inline: config.display.graph_inline && !config.display.vertical_labels,
            styles: &graph_styles(config),
            network_floor: NetworkFloor::new(&config.network),
        };
        renderer.draw(&surface, &frame, &text).unwrap();
        (surface, layout)
    }

    /// What `kind`'s first tile looks like.
    fn tile_pixels(surface: &Surface, layout: &Layout, kind: ItemKind) -> Vec<u32> {
        let all = pixels(surface);
        let cell = layout.cells.iter().find(|c| c.kind == kind).unwrap();
        let (x, y) = (cell.rect.x as i32, cell.rect.y as i32);
        let (w, h) = (cell.rect.w as i32, cell.rect.h as i32);
        (y..y + h)
            .flat_map(|row| {
                let start = (row * surface.width + x) as usize;
                all[start..start + w as usize].to_vec()
            })
            .collect()
    }

    /// Only `first` and `second` shown, in that order, with `second` sized.
    fn two_items(first: ItemKind, second: ItemKind, inline: bool, height: Option<u16>) -> Config {
        let mut config = Config::default();
        config.display.graph_inline = inline;
        for item in &mut config.items {
            item.enabled = false;
        }
        for kind in [second, first] {
            let at = config.items.iter().position(|i| i.kind == kind).unwrap();
            let mut item = config.items.remove(at);
            item.enabled = true;
            config.items.insert(0, item);
        }
        config.item_mut(second).unwrap().tile_height = height;
        config
    }

    #[test]
    fn the_overlay_draws_the_custom_name() {
        for mode in [DisplayMode::Text, DisplayMode::Graph] {
            let samples = [10.0, 50.0, 90.0];
            let mut config = Config::default();
            let (surface, layout) = render_with(mode, &samples, &config);
            let plain = tile_pixels(&surface, &layout, ItemKind::Cpu);
            config.item_mut(ItemKind::Cpu).unwrap().label = Some("XYZ".into());
            let (surface, layout) = render_with(mode, &samples, &config);
            assert_ne!(
                tile_pixels(&surface, &layout, ItemKind::Cpu),
                plain,
                "{mode:?}"
            );
        }
    }

    #[test]
    fn a_network_tile_grows_to_draw_custom_directions() {
        let rates = [
            (MetricKey::NetDownBps, [1e6; 3]),
            (MetricKey::NetUpBps, [2e6; 3]),
        ];
        let plain = network_pixels(&Config::default(), DisplayMode::Graph, None, &rates);
        let mut config = Config::default();
        config.network.down_label = Some("Inbound".into());
        config.network.up_label = Some("Outgoing1234".into());
        let renamed = network_pixels(&config, DisplayMode::Graph, None, &rates);
        assert!(
            renamed.len() > plain.len(),
            "a wider tile for the longer name"
        );
    }

    #[test]
    fn each_tile_draws_its_own_text_size_in_either_order() {
        let graph = DisplayMode::Graph;
        let samples = [10.0, 50.0, 90.0];
        for inline in [false, true] {
            for other in [ItemKind::Ram, ItemKind::Network] {
                let height = if inline { 12 } else { 20 };
                let draw = |config: &Config, kind| {
                    let (surface, layout) = render_on(graph, &samples, config, false, None);
                    tile_pixels(&surface, &layout, kind)
                };
                let auto = two_items(ItemKind::Cpu, other, inline, None);
                let before = two_items(ItemKind::Cpu, other, inline, Some(height));
                let after = two_items(other, ItemKind::Cpu, inline, None);
                let mut after_sized = after.clone();
                after_sized.item_mut(other).unwrap().tile_height = Some(height);
                assert_eq!(
                    draw(&before, ItemKind::Cpu),
                    draw(&auto, ItemKind::Cpu),
                    "a short tile after it leaves the first tile alone"
                );
                assert_eq!(
                    draw(&after_sized, ItemKind::Cpu),
                    draw(&auto, ItemKind::Cpu),
                    "a short tile before it leaves the second tile alone"
                );
                assert_eq!(
                    draw(&before, other),
                    draw(&after_sized, other),
                    "the short tile looks the same in either order"
                );
            }
        }
    }

    #[test]
    fn a_tall_floating_tile_grows_the_surface_and_leaves_the_others_alone() {
        let samples = [10.0, 50.0, 90.0];
        let auto = two_items(ItemKind::Cpu, ItemKind::Ram, false, None);
        let tall = two_items(ItemKind::Ram, ItemKind::Cpu, false, None);
        let mut tall = tall;
        tall.item_mut(ItemKind::Ram).unwrap().tile_height = Some(100);
        let (surface, layout) = render_on(DisplayMode::Graph, &samples, &auto, true, None);
        let expected = tile_pixels(&surface, &layout, ItemKind::Cpu);
        let (surface, layout) = render_on(DisplayMode::Graph, &samples, &tall, true, None);
        assert_eq!(surface.height, 108);
        assert_eq!(tile_pixels(&surface, &layout, ItemKind::Cpu), expected);
        let tile = layout
            .cells
            .iter()
            .find(|c| c.kind == ItemKind::Ram)
            .unwrap();
        assert_eq!(tile.rect.h, 100.0);
    }

    #[test]
    fn vertical_tiles_draw_their_own_text_size_in_either_order() {
        let samples = [10.0, 50.0, 90.0];
        let draw = |config: &Config| {
            let (surface, layout) =
                render_on(DisplayMode::Graph, &samples, config, false, Some(120.0));
            tile_pixels(&surface, &layout, ItemKind::Cpu)
        };
        let auto = draw(&two_items(ItemKind::Cpu, ItemKind::Ram, false, None));
        let mut first = two_items(ItemKind::Ram, ItemKind::Cpu, false, None);
        first.item_mut(ItemKind::Ram).unwrap().tile_height = Some(20);
        let mut second = two_items(ItemKind::Cpu, ItemKind::Ram, false, None);
        second.item_mut(ItemKind::Ram).unwrap().tile_height = Some(20);
        assert_eq!(draw(&first), auto);
        assert_eq!(draw(&second), auto);
    }

    /// An overlay showing only the network, with three samples per metric; the
    /// last is the current value.
    fn network_pixels(
        config: &Config,
        mode: DisplayMode,
        vertical_bar_px: Option<f32>,
        rates: &[(MetricKey, [f64; 3])],
    ) -> Vec<u32> {
        let mut config = config.clone();
        for item in &mut config.items {
            item.enabled = item.kind == ItemKind::Network;
        }
        let text = Text::new(None, false).unwrap();
        let items = pulsar_core::layout::item_specs(&config, &Default::default());
        let names = ItemNames::resolve(&config);
        let layout = compute_layout(
            &items,
            LayoutInput {
                mode,
                taskbar_height_px: 48.0,
                fit_font: false,
                dpi: 96,
                font_size_pt: 9.0,
                labels: config.display.labels,
                icons: config.display.icons,
                graph_inline: config.display.graph_inline,
                vertical_labels: false,
                upload_on_top: config.display.upload_on_top,
                vertical_bar_px,
                text_spacing: TextSpacing::default(),
                names: &names,
            },
            &text,
        );
        let mut history = HistoryStore::new(60);
        let mut snapshot = Snapshot::default();
        for sample in 0..3 {
            snapshot = Snapshot::default();
            for (key, values) in rates {
                snapshot.set(*key, values[sample]);
            }
            history.record(&snapshot);
        }
        let palette = Palette::new(false, &config, None, &crate::theme::Theme::windows());
        let surface = Surface::new(layout.width.ceil() as i32, layout.height as i32).unwrap();
        let frame = Frame {
            layout: &layout,
            mode,
            palette: &palette,
            snapshot: &snapshot,
            history: &history,
            dpi: 96,
            text: TileText {
                labels: config.display.labels,
                icons: config.display.icons,
                vertical_labels: false,
                upload_on_top: config.display.upload_on_top,
                names: &names,
            },
            graph_inline: config.display.graph_inline && !layout.vertical,
            styles: &graph_styles(&config),
            network_floor: NetworkFloor::new(&config.network),
        };
        Renderer::new()
            .unwrap()
            .draw(&surface, &frame, &text)
            .unwrap();
        pixels(&surface)
    }

    const KB: f64 = 1024.0;

    fn every_network_look(mut check: impl FnMut(&Config, DisplayMode, Option<f32>)) {
        for upload_on_top in [false, true] {
            for vertical in [None, Some(80.0)] {
                let mut config = Config::default();
                config.display.upload_on_top = upload_on_top;
                check(&config, DisplayMode::Text, vertical);
                for inline in [false, true] {
                    for style in [
                        GraphStyle::Area,
                        GraphStyle::Line,
                        GraphStyle::Bars,
                        GraphStyle::Meter,
                    ] {
                        config.display.graph_inline = inline;
                        config.item_mut(ItemKind::Network).unwrap().style = style;
                        check(&config, DisplayMode::Graph, vertical);
                    }
                }
            }
        }
    }

    fn floored(config: &Config) -> Config {
        let mut config = config.clone();
        config.network.min_down_bps = (10.0 * KB) as u64;
        config.network.min_up_bps = KB as u64;
        config
    }

    #[test]
    fn rates_below_the_floor_draw_as_zero() {
        use MetricKey::{NetDownBps, NetUpBps};
        every_network_look(|config, mode, vertical| {
            let measured = [
                (NetDownBps, [4.0 * KB, 25.0 * KB, 4.0 * KB]),
                (NetUpBps, [512.0, 2.0 * KB, 512.0]),
            ];
            let projected = [
                (NetDownBps, [0.0, 25.0 * KB, 0.0]),
                (NetUpBps, [0.0, 2.0 * KB, 0.0]),
            ];
            assert_eq!(
                network_pixels(&floored(config), mode, vertical, &measured),
                network_pixels(config, mode, vertical, &projected),
            );
            assert_ne!(
                network_pixels(&floored(config), mode, vertical, &measured),
                network_pixels(config, mode, vertical, &measured),
                "the floor changes what is drawn",
            );
        });
    }

    #[test]
    fn rates_at_or_above_the_floor_draw_as_measured() {
        use MetricKey::{NetDownBps, NetUpBps};
        every_network_look(|config, mode, vertical| {
            let measured = [
                (NetDownBps, [10.0 * KB, 25.0 * KB, 10.0 * KB]),
                (NetUpBps, [KB, 2.0 * KB, KB]),
            ];
            assert_eq!(
                network_pixels(&floored(config), mode, vertical, &measured),
                network_pixels(config, mode, vertical, &measured),
            );
        });
    }

    #[test]
    fn each_direction_draws_against_its_own_floor() {
        use MetricKey::{NetDownBps, NetUpBps};
        every_network_look(|config, mode, vertical| {
            let measured = [
                (NetDownBps, [5.0 * KB, 5.0 * KB, 5.0 * KB]),
                (NetUpBps, [5.0 * KB, 5.0 * KB, 5.0 * KB]),
            ];
            let projected = [
                (NetDownBps, [0.0, 0.0, 0.0]),
                (NetUpBps, [5.0 * KB, 5.0 * KB, 5.0 * KB]),
            ];
            assert_eq!(
                network_pixels(&floored(config), mode, vertical, &measured),
                network_pixels(config, mode, vertical, &projected),
            );
        });
    }

    #[test]
    fn each_adapter_draws_against_the_floor_by_itself() {
        use MetricKey::{NetAdapterDownBps as Down, NetAdapterUpBps as Up};
        every_network_look(|config, mode, vertical| {
            let mut config = config.clone();
            config.network.adapters = vec!["eth".into(), "wifi".into()];
            let measured = [
                (Down(0), [4.0 * KB, 4.0 * KB, 4.0 * KB]),
                (Up(0), [512.0, 512.0, 512.0]),
                (Down(1), [25.0 * KB, 25.0 * KB, 25.0 * KB]),
                (Up(1), [2.0 * KB, 2.0 * KB, 2.0 * KB]),
            ];
            let projected = [
                (Down(0), [0.0, 0.0, 0.0]),
                (Up(0), [0.0, 0.0, 0.0]),
                (Down(1), [25.0 * KB, 25.0 * KB, 25.0 * KB]),
                (Up(1), [2.0 * KB, 2.0 * KB, 2.0 * KB]),
            ];
            assert_eq!(
                network_pixels(&floored(&config), mode, vertical, &measured),
                network_pixels(&config, mode, vertical, &projected),
            );
        });
    }

    /// Each network tile's pixels with two adapters, optionally with the first
    /// adapter's upload in its history or snapshot.
    fn two_adapter_tiles(
        vertical_bar_px: Option<f32>,
        up_history: bool,
        up_value: bool,
    ) -> (Vec<Cell>, Vec<Vec<u32>>) {
        adapter_tiles(&|_| {}, vertical_bar_px, up_history, up_value)
    }

    fn adapter_tiles(
        edit: &dyn Fn(&mut Config),
        vertical_bar_px: Option<f32>,
        up_history: bool,
        up_value: bool,
    ) -> (Vec<Cell>, Vec<Vec<u32>>) {
        let mut config = Config::default();
        for item in &mut config.items {
            item.enabled = item.kind == ItemKind::Network;
        }
        config.network.adapters = vec!["eth".into(), "wifi".into()];
        edit(&mut config);
        let text = Text::new(None, false).unwrap();
        let items = pulsar_core::layout::item_specs(&config, &Default::default());
        let names = ItemNames::resolve(&config);
        let layout = compute_layout(
            &items,
            LayoutInput {
                mode: DisplayMode::Graph,
                taskbar_height_px: 48.0,
                fit_font: false,
                dpi: 96,
                font_size_pt: 9.0,
                labels: config.display.labels,
                icons: config.display.icons,
                graph_inline: config.display.graph_inline,
                vertical_labels: false,
                upload_on_top: config.display.upload_on_top,
                vertical_bar_px,
                text_spacing: TextSpacing::default(),
                names: &names,
            },
            &text,
        );
        let up = MetricKey::NetAdapterUpBps(0);
        let mut snapshot = Snapshot::default();
        if up_value {
            snapshot.set(up, 5e6);
        }
        let mut history = HistoryStore::new(60);
        // Level, as a peak's mitred stroke can reach into the tile above.
        for _ in 0..3 {
            let mut s = Snapshot::default();
            s.set(MetricKey::NetAdapterDownBps(1), 1e3);
            if up_history {
                s.set(up, 2e6);
            }
            history.record(&s);
        }
        let palette = Palette::new(false, &config, None, &crate::theme::Theme::windows());
        let surface = Surface::new(layout.width.ceil() as i32, layout.height as i32).unwrap();
        let frame = Frame {
            layout: &layout,
            mode: DisplayMode::Graph,
            palette: &palette,
            snapshot: &snapshot,
            history: &history,
            dpi: 96,
            text: TileText {
                labels: config.display.labels,
                icons: config.display.icons,
                vertical_labels: false,
                upload_on_top: config.display.upload_on_top,
                names: &names,
            },
            graph_inline: config.display.graph_inline,
            styles: &graph_styles(&config),
            network_floor: NetworkFloor::default(),
        };
        Renderer::new()
            .unwrap()
            .draw(&surface, &frame, &text)
            .unwrap();
        let all = pixels(&surface);
        let tile = |c: &Cell| -> Vec<u32> {
            let row = |y: i32| {
                let at = (y * surface.width + c.rect.x as i32) as usize;
                all[at..at + c.rect.w as usize].to_vec()
            };
            (c.rect.y as i32..(c.rect.y + c.rect.h) as i32)
                .flat_map(row)
                .collect()
        };
        let tiles = layout.cells.iter().map(tile).collect();
        (layout.cells.clone(), tiles)
    }

    #[test]
    fn a_horizontal_adapter_tile_draws_both_directions() {
        let (cells, plain) = two_adapter_tiles(None, false, false);
        assert_eq!(
            cells.iter().map(|c| c.part).collect::<Vec<_>>(),
            [CellPart::AdapterPair(0), CellPart::AdapterPair(1)],
            "one tile per adapter"
        );
        let (_, graphed) = two_adapter_tiles(None, true, false);
        assert_ne!(graphed[0], plain[0], "the upload graph is in its tile");
        assert_eq!(graphed[1], plain[1], "and not in the other adapter's");
        let (_, valued) = two_adapter_tiles(None, false, true);
        assert_ne!(valued[0], plain[0], "the upload value is in its tile");
        assert_eq!(valued[1], plain[1], "and not in the other adapter's");
    }

    #[test]
    fn a_vertical_adapter_direction_has_a_tile_of_its_own() {
        let bar = Some(80.0);
        let (cells, plain) = two_adapter_tiles(bar, false, false);
        assert_eq!(
            cells.iter().map(|c| c.part).collect::<Vec<_>>(),
            [
                CellPart::AdapterDown(0),
                CellPart::AdapterUp(0),
                CellPart::AdapterDown(1),
                CellPart::AdapterUp(1)
            ],
        );
        let (_, graphed) = two_adapter_tiles(bar, true, false);
        assert_eq!(graphed[0], plain[0], "no upload graph on the download tile");
        assert_ne!(graphed[1], plain[1], "the upload tile has it");
        let (_, valued) = two_adapter_tiles(bar, false, true);
        assert_eq!(valued[0], plain[0], "no upload value on the download tile");
        assert_ne!(valued[1], plain[1], "the upload tile has it");
    }

    #[test]
    fn a_horizontal_download_tile_draws_no_upload() {
        let edit = |c: &mut Config| c.network.show = NetworkShow::Download;
        let (cells, plain) = adapter_tiles(&edit, None, false, false);
        assert_eq!(
            cells.iter().map(|c| c.part).collect::<Vec<_>>(),
            [CellPart::AdapterDown(0), CellPart::AdapterDown(1)],
        );
        let (_, graphed) = adapter_tiles(&edit, None, true, false);
        assert_eq!(graphed, plain, "no upload graph");
        let (_, valued) = adapter_tiles(&edit, None, false, true);
        assert_eq!(valued, plain, "no upload value");
    }

    #[test]
    fn a_horizontal_upload_tile_draws_its_own_direction() {
        let edit = |c: &mut Config| c.network.show = NetworkShow::Upload;
        let (cells, plain) = adapter_tiles(&edit, None, false, false);
        assert_eq!(
            cells.iter().map(|c| c.part).collect::<Vec<_>>(),
            [CellPart::AdapterUp(0), CellPart::AdapterUp(1)],
            "upload alone keeps its tiles"
        );
        let (_, graphed) = adapter_tiles(&edit, None, true, false);
        assert_ne!(graphed[0], plain[0], "the upload graph is in its tile");
        assert_eq!(graphed[1], plain[1], "and not in the other adapter's");
        let (_, valued) = adapter_tiles(&edit, None, false, true);
        assert_ne!(valued[0], plain[0], "the upload value is in its tile");
        assert_eq!(valued[1], plain[1], "and not in the other adapter's");
    }

    #[test]
    fn split_tiles_draw_one_direction_each() {
        let edit = |c: &mut Config| c.network.split_tiles = true;
        let (cells, plain) = adapter_tiles(&edit, None, false, false);
        assert_eq!(
            cells.iter().map(|c| c.part).collect::<Vec<_>>(),
            [
                CellPart::AdapterDown(0),
                CellPart::AdapterUp(0),
                CellPart::AdapterDown(1),
                CellPart::AdapterUp(1)
            ],
        );
        let (_, graphed) = adapter_tiles(&edit, None, true, false);
        assert_eq!(graphed[0], plain[0], "no upload graph on the download tile");
        assert_ne!(graphed[1], plain[1], "the upload tile has it");
    }

    #[test]
    fn inline_tiles_draw_only_their_own_direction() {
        let edit = |c: &mut Config| {
            c.network.show = NetworkShow::Download;
            c.display.graph_inline = true;
        };
        let (_, plain) = adapter_tiles(&edit, None, false, false);
        let (_, valued) = adapter_tiles(&edit, None, false, true);
        assert_eq!(valued, plain, "no upload value on one line");
        let both = |c: &mut Config| c.display.graph_inline = true;
        let (_, plain) = adapter_tiles(&both, None, false, false);
        let (_, valued) = adapter_tiles(&both, None, false, true);
        assert_ne!(valued[0], plain[0], "a pair shows upload on one line");
    }

    #[test]
    fn inline_vertical_tiles_draw_only_their_own_direction() {
        let edit = |c: &mut Config| c.display.graph_inline = true;
        let bar = Some(160.0);
        let (_, plain) = adapter_tiles(&edit, bar, false, false);
        let (_, valued) = adapter_tiles(&edit, bar, false, true);
        assert_eq!(valued[0], plain[0], "no upload value on the download tile");
        assert_ne!(valued[1], plain[1], "the upload tile has it");
    }

    fn alpha_at(surface: &Surface, x: f32, y: f32) -> u32 {
        pixels(surface)[(y as i32 * surface.width + x as i32) as usize] >> 24
    }

    #[test]
    fn zero_tile_opacity_draws_no_tile_background() {
        // RAM has no history in these tests, so its top-right corner shows only
        // the tile background.
        let corner = |config: &Config| {
            let (surface, layout) = render_with(DisplayMode::Graph, &[], config);
            let ram = layout
                .cells
                .iter()
                .find(|c| c.kind == ItemKind::Ram)
                .unwrap();
            alpha_at(&surface, ram.rect.right() - 3.0, ram.rect.y + 3.0)
        };
        assert!(corner(&Config::default()) > 10, "default tile is visible");
        let mut config = Config::default();
        config.display.tile_opacity = Some(0);
        assert_eq!(corner(&config), 1, "only the hit-test fill remains");
    }

    #[test]
    fn panel_fills_gaps_between_cells() {
        let gap = |config: &Config| {
            let (surface, layout) = render_with(DisplayMode::Text, &[], config);
            let first = &layout.cells[0];
            alpha_at(&surface, first.rect.right() + 2.0, first.rect.y + 2.0)
        };
        assert_eq!(gap(&Config::default()), 1, "no panel by default");
        let mut config = Config::default();
        config.display.panel_opacity = Some(60);
        assert!(gap(&config) >= 150, "panel at 60 % opacity");
    }

    #[test]
    fn a_single_sample_draws_without_a_line() {
        let (surface, _) = render_with_samples(DisplayMode::Graph, &[42.0]);
        assert!(pixels(&surface).iter().any(|p| p >> 24 > 0x40));
    }

    #[test]
    fn every_pixel_is_hit_testable() {
        for mode in [DisplayMode::Graph, DisplayMode::Text] {
            let (surface, _) = render(mode);
            assert!(
                pixels(&surface).iter().all(|p| p >> 24 > 0),
                "{mode:?} has fully transparent pixels"
            );
        }
    }

    #[test]
    fn draws_visible_content_in_each_cell() {
        for mode in [DisplayMode::Graph, DisplayMode::Text] {
            let (surface, layout) = render(mode);
            let px = pixels(&surface);
            for cell in &layout.cells {
                let r = cell.rect;
                let opaque = (r.y as i32..(r.y + r.h) as i32)
                    .flat_map(|y| (r.x as i32..(r.x + r.w) as i32).map(move |x| (x, y)))
                    .filter(|&(x, y)| x < surface.width && y < surface.height)
                    .filter(|&(x, y)| px[(y * surface.width + x) as usize] >> 24 > 0x40)
                    .count();
                assert!(opaque > 10, "{mode:?} {:?} drew nothing", cell.kind);
            }
        }
    }

    #[test]
    fn inline_graph_text_shares_one_centred_line() {
        let mut config = Config::default();
        config.display.graph_inline = true;
        let (surface, layout) = render_with(DisplayMode::Graph, &[], &config);
        let px = pixels(&surface);
        let ram = layout
            .cells
            .iter()
            .find(|c| c.kind == ItemKind::Ram)
            .unwrap();
        let r = ram.rect;
        // RAM has no history here, so any ink is its label and value.
        let ink = |x: i32, y: i32| px[(y * surface.width + x) as usize] >> 24 > 0x80;
        let rows: Vec<i32> = (r.y as i32..(r.y + r.h) as i32)
            .filter(|&y| (r.x as i32..r.right() as i32).any(|x| ink(x, y)))
            .collect();
        let (first, last) = (rows[0] as f32, *rows.last().unwrap() as f32);
        assert!(last - first < r.h / 2.0, "one line, not two");
        let centre = r.y + r.h / 2.0;
        assert!(((first + last) / 2.0 - centre).abs() < 4.0, "centred");
        let half = (r.x + r.w / 2.0) as i32;
        assert!(
            rows.iter()
                .any(|&y| (half..r.right() as i32).any(|x| ink(x, y))),
            "the value sits beside the label"
        );
    }

    #[test]
    fn text_only_items_draw_no_graph() {
        // The CPU graph peaks at 100%, so the tile's top-right corner is ink
        // only while the graph is drawn.
        let corner = |config: &Config| {
            let (surface, layout) = render_with(DisplayMode::Graph, &[100.0; 60], config);
            let cpu = layout
                .cells
                .iter()
                .find(|c| c.kind == ItemKind::Cpu)
                .unwrap();
            alpha_at(&surface, cpu.rect.right() - 3.0, cpu.rect.y + 3.0)
        };
        let mut config = Config::default();
        config.display.tile_opacity = Some(0);
        assert!(corner(&config) > 0x40, "graph drawn by default");
        config.items[0].graph = false;
        assert_eq!(config.items[0].kind, ItemKind::Cpu);
        assert_eq!(corner(&config), 1, "only the hit-test fill remains");
    }

    #[test]
    fn a_core_bar_style_draws_a_bar_per_core_right_of_the_text() {
        let mut config = Config::default();
        config.display.tile_opacity = Some(0);
        config.item_mut(ItemKind::Cpu).unwrap().style = GraphStyle::Cores;
        let (surface, layout) = render_with(DisplayMode::Graph, &[50.0; 8], &config);
        let cpu = layout
            .cells
            .iter()
            .find(|c| c.kind == ItemKind::Cpu)
            .unwrap();
        let bottom = cpu.rect.y + cpu.rect.h - 5.0;
        let last = alpha_at(&surface, cpu.rect.right() - 5.0, bottom);
        assert!(last > 0x40, "no bar at the right edge ({last})");
    }

    #[test]
    fn peak_marks_show_above_a_quiet_core() {
        let surface = Surface::new(40, 40).unwrap();
        let mut renderer = Renderer::new().unwrap();
        let transparent = Color::rgb(0, 0, 0).with_alpha(0.0);
        let area = Rect {
            x: 0.0,
            y: 0.0,
            w: 40.0,
            h: 40.0,
        };
        let loads = [CoreLoad {
            load: 0.0,
            peak: 50.0,
        }];
        let marks = CoreMarks {
            split: None,
            track: transparent,
            peak: Color::rgb(255, 255, 255),
            scale: 1.0,
        };
        renderer
            .paint(&surface, transparent, |r| {
                r.draw_core_bars(&loads, area, marks, |_| transparent)
            })
            .unwrap();
        assert!(alpha_at(&surface, 20.0, 20.5) > 0x80, "no mark at the peak");
        assert_eq!(alpha_at(&surface, 20.0, 10.5), 0, "nothing above it");
    }

    #[test]
    fn heat_segment_and_spark_styles_draw_beside_or_under_the_text() {
        for (style, below) in [
            (GraphStyle::Heat, false),
            (GraphStyle::Segments, true),
            (GraphStyle::Spark, true),
        ] {
            let mut config = Config::default();
            config.display.tile_opacity = Some(0);
            config.item_mut(ItemKind::Cpu).unwrap().style = style;
            let (surface, layout) = render_with(DisplayMode::Graph, &[50.0; 60], &config);
            let cpu = layout
                .cells
                .iter()
                .find(|c| c.kind == ItemKind::Cpu)
                .unwrap();
            let tile = tile_pixels(&surface, &layout, ItemKind::Cpu);
            let (w, h) = (cpu.rect.w as usize, cpu.rect.h as usize);
            let rows = if below { h / 2..h - 1 } else { 0..h };
            let inked = rows
                .into_iter()
                .any(|row| (w / 2..w - 2).any(|x| tile[row * w + x] >> 24 > 0x20));
            assert!(inked, "{style:?} drew nothing where it should");
        }
    }

    #[test]
    fn a_ring_inks_its_track_beside_the_text() {
        let mut config = Config::default();
        config.display.tile_opacity = Some(0);
        config.item_mut(ItemKind::Cpu).unwrap().style = GraphStyle::Ring;
        let (surface, layout) = render_with(DisplayMode::Graph, &[100.0; 8], &config);
        let cpu = layout
            .cells
            .iter()
            .find(|c| c.kind == ItemKind::Cpu)
            .unwrap();
        let center_x = cpu.rect.x + 4.0 + (cpu.rect.h - 6.0) / 2.0;
        let top = alpha_at(&surface, center_x, cpu.rect.y + 5.0);
        assert!(top > 0x40, "no arc at twelve o'clock ({top})");
    }

    #[test]
    fn vertical_labels_keep_the_value_on_one_centred_line() {
        let mut config = Config::default();
        config.display.tile_opacity = Some(0);
        config.display.vertical_labels = true;
        config.item_mut(ItemKind::Cpu).unwrap().graph = false;
        let (surface, layout) = render_with(DisplayMode::Graph, &[50.0; 8], &config);
        let cpu = layout
            .cells
            .iter()
            .find(|c| c.kind == ItemKind::Cpu)
            .unwrap();
        let tile = tile_pixels(&surface, &layout, ItemKind::Cpu);
        let (w, h) = (cpu.rect.w as usize, cpu.rect.h as usize);
        let ink_right_half = |row: usize| (w / 2..w).any(|x| tile[row * w + x] >> 24 > 0x40);
        assert!((0..h).any(ink_right_half));
        assert!(!ink_right_half(1), "the top line stays clear of the value");
        assert!(
            !ink_right_half(h - 2),
            "the bottom line stays clear of the value"
        );
    }

    #[test]
    fn every_graph_style_draws_ink_where_the_graph_peaks() {
        use pulsar_core::config::GraphStyle;
        // A full-height history: every style inks the top right of the tile.
        for style in [
            GraphStyle::Area,
            GraphStyle::Line,
            GraphStyle::Bars,
            GraphStyle::Meter,
        ] {
            let mut config = Config::default();
            config.display.tile_opacity = Some(0);
            config.item_mut(ItemKind::Cpu).unwrap().style = style;
            let (surface, layout) = render_with(DisplayMode::Graph, &[100.0; 60], &config);
            let cpu = layout
                .cells
                .iter()
                .find(|c| c.kind == ItemKind::Cpu)
                .unwrap();
            let alpha = alpha_at(&surface, cpu.rect.right() - 3.0, cpu.rect.y + 1.5);
            assert!(alpha > 0x40, "{style:?} drew nothing at the peak ({alpha})");
        }
    }

    #[test]
    fn icons_draw_in_every_cell() {
        let mut config = Config::default();
        config.display.labels = pulsar_core::config::LabelStyle::None;
        config.display.icons = true;
        let (surface, layout) = render_with(DisplayMode::Text, &[], &config);
        let px = pixels(&surface);
        for cell in &layout.cells {
            let r = cell.rect;
            let ink = (r.y as i32..(r.y + r.h) as i32)
                .flat_map(|y| (r.x as i32..cell.value_x as i32).map(move |x| (x, y)))
                .filter(|&(x, y)| px[(y * surface.width + x) as usize] >> 24 > 0x40)
                .count();
            assert!(ink > 5, "{:?} {:?} has no icon", cell.kind, cell.part);
        }
    }
}
