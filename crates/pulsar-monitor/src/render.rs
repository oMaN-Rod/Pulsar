use pulsar_core::config::{DisplayMode, LabelStyle};
use pulsar_core::history::{History, HistoryStore};
use pulsar_core::layout::{
    Cell, CellPart, Layout, Rect, TextMeasure, cell_icon, cell_label, icon_gap, label_width,
};
use pulsar_core::metric::{ItemKind, Snapshot};
use windows::Win32::Foundation::{D2DERR_RECREATE_TARGET, HWND, POINT, RECT, SIZE};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D_RECT_F, D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F, D2D1_FIGURE_BEGIN_FILLED,
    D2D1_FIGURE_BEGIN_HOLLOW, D2D1_FIGURE_END_CLOSED, D2D1_FIGURE_END_OPEN, D2D1_PIXEL_FORMAT,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1_DRAW_TEXT_OPTIONS_NONE, D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1_FEATURE_LEVEL_DEFAULT,
    D2D1_RENDER_TARGET_PROPERTIES, D2D1_RENDER_TARGET_TYPE_SOFTWARE, D2D1_RENDER_TARGET_USAGE_NONE,
    D2D1_ROUNDED_RECT, D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE, D2D1CreateFactory, ID2D1DCRenderTarget,
    ID2D1Factory, ID2D1PathGeometry,
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

use crate::display::{area_points, cell_value, graph_max, metric_for};
use crate::text::Text;
use crate::theme::{Color, Palette};

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
            let bitmap = CreateDIBSection(Some(dc), &info, DIB_RGB_COLORS, &mut bits, None, 0)?;
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
    pub labels: LabelStyle,
    pub icons: bool,
}

pub struct Renderer {
    factory: ID2D1Factory,
    target: ID2D1DCRenderTarget,
}

impl Renderer {
    pub fn new() -> Result<Self> {
        let factory: ID2D1Factory =
            unsafe { D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)? };
        let target = create_target(&factory)?;
        Ok(Self { factory, target })
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
        }
        drawn.and(ended)
    }

    pub fn present(&self, hwnd: HWND, surface: &Surface, x: i32, y: i32) -> Result<()> {
        self.present_alpha(hwnd, surface, x, y, 255)
    }

    /// Presents with the whole window's opacity scaled by `alpha`, for fades.
    pub fn present_alpha(
        &self,
        hwnd: HWND,
        surface: &Surface,
        x: i32,
        y: i32,
        alpha: u8,
    ) -> Result<()> {
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
                Some(&POINT { x, y }),
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
        if frame.icons {
            let glyph = cell_icon(kind, part);
            let format = text.icon_format(font_px)?;
            let dy = text.icon_offset(glyph, font_px);
            self.text_at(text, &format, glyph, x, y + dy, color)?;
            x += text.icon_width(glyph, font_px) + icon_gap(font_px);
        }
        let label = cell_label(kind, part, frame.labels, frame.icons);
        if !label.is_empty() {
            self.text_at(text, &text.format(font_px)?, &label, x, y, color)?;
        }
        Ok(())
    }

    fn draw_text_cells(&self, frame: &Frame, text: &Text) -> Result<()> {
        let font_px = frame.layout.font_px;
        let format = text.format(font_px)?;
        for cell in &frame.layout.cells {
            self.draw_label(
                text,
                frame,
                font_px,
                cell.kind,
                cell.part,
                cell.rect.x,
                cell.rect.y,
            )?;
            let value = cell_value(cell.kind, cell.part, frame.snapshot);
            self.text_at(
                text,
                &format,
                &value,
                cell.value_x,
                cell.rect.y,
                frame.palette.text,
            )?;
        }
        Ok(())
    }

    fn draw_graph_cells(&self, frame: &Frame, text: &Text) -> Result<()> {
        let scale = frame.dpi as f32 / 96.0;
        let small_px = frame.layout.font_px * 0.85;
        let small = text.format(small_px)?;
        let line_h = frame.layout.font_px * 0.85 * 1.3;
        for cell in &frame.layout.cells {
            self.fill_rounded(cell.rect, 4.0 * scale, frame.palette.tile)?;
            self.draw_series(frame, cell, scale)?;
            let pad = 4.0 * scale;
            let (top, bottom) = (
                cell.rect.y + pad * 0.5,
                cell.rect.y + cell.rect.h - line_h - pad * 0.5,
            );
            let x = cell.rect.x + pad;
            if cell.kind == ItemKind::Network {
                for (part, y) in [(CellPart::Down, top), (CellPart::Up, bottom)] {
                    self.draw_label(text, frame, small_px, cell.kind, part, x, y)?;
                    let label_w =
                        label_width(cell.kind, part, frame.labels, frame.icons, small_px, text);
                    let value = cell_value(cell.kind, part, frame.snapshot);
                    let value_x = x + label_w + icon_gap(small_px);
                    self.text_at(text, &small, &value, value_x, y, frame.palette.text)?;
                }
            } else {
                self.draw_label(text, frame, small_px, cell.kind, cell.part, x, top)?;
                let value = cell_value(cell.kind, cell.part, frame.snapshot);
                self.text_at(text, &small, &value, x, bottom, frame.palette.text)?;
            }
        }
        Ok(())
    }

    fn draw_series(&self, frame: &Frame, cell: &Cell, scale: f32) -> Result<()> {
        let parts: &[CellPart] = if cell.kind == ItemKind::Network {
            &[CellPart::Down, CellPart::Up]
        } else {
            std::slice::from_ref(&cell.part)
        };
        let histories: Vec<(CellPart, &History)> = parts
            .iter()
            .filter_map(|&p| frame.history.get(metric_for(cell.kind, p)).map(|h| (p, h)))
            .collect();
        if histories.is_empty() {
            return Ok(());
        }
        let unit = metric_for(cell.kind, parts[0]).unit();
        let max = graph_max(unit, &histories.iter().map(|(_, h)| *h).collect::<Vec<_>>());
        let inset = 1.0 * scale;
        let area = Rect {
            x: cell.rect.x + inset,
            y: cell.rect.y + inset,
            w: cell.rect.w - 2.0 * inset,
            h: cell.rect.h - 2.0 * inset,
        };
        for (i, (part, history)) in histories.iter().enumerate() {
            let color = frame.palette.graph(cell.kind, *part);
            self.series(
                &area_points(history, area, max),
                color,
                i == 0,
                1.25 * scale,
            )?;
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
            let brush = unsafe {
                self.target
                    .CreateSolidColorBrush(&d2d(color.with_alpha(0.35)), None)?
            };
            unsafe { self.target.FillGeometry(&area, &brush, None) };
        }
        let line = self.path(&points[1..points.len() - 1], false)?;
        let brush = unsafe { self.target.CreateSolidColorBrush(&d2d(color), None)? };
        unsafe { self.target.DrawGeometry(&line, &brush, width, None) };
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
            let brush = self.target.CreateSolidColorBrush(&d2d(color), None)?;
            self.target.FillRoundedRectangle(&rounded, &brush);
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
            let brush = self.target.CreateSolidColorBrush(&d2d(color), None)?;
            self.target.DrawTextLayout(
                Vector2 {
                    X: x.round(),
                    Y: y.round(),
                },
                &layout,
                &brush,
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
    use pulsar_core::config::{Config, DisplayMode};
    use pulsar_core::layout::{LayoutInput, compute_layout};
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
        let text = Text::new(None, false).unwrap();
        let items = pulsar_core::layout::item_specs(config);
        let layout = compute_layout(
            &items,
            LayoutInput {
                mode,
                taskbar_height_px: 48.0,
                dpi: 96,
                font_size_pt: 9.0,
                labels: config.display.labels,
                icons: config.display.icons,
            },
            &text,
        );
        let mut snapshot = Snapshot::default();
        snapshot.set(MetricKey::CpuTotal, 50.0);
        let mut history = HistoryStore::new(60);
        for &v in samples {
            let mut s = Snapshot::default();
            s.set(MetricKey::CpuTotal, v);
            history.record(&s);
        }
        let palette = Palette::new(false, config, None);
        let surface = Surface::new(layout.width.ceil() as i32, layout.height as i32).unwrap();
        let mut renderer = Renderer::new().unwrap();
        let frame = Frame {
            layout: &layout,
            mode,
            palette: &palette,
            snapshot: &snapshot,
            history: &history,
            dpi: 96,
            labels: config.display.labels,
            icons: config.display.icons,
        };
        renderer.draw(&surface, &frame, &text).unwrap();
        (surface, layout)
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
        config.display.panel_opacity = 60;
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
