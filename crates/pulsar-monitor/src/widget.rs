//! The desktop widget: a panel that stays open with a compact section per
//! item, in as many columns as it needs to fit on the screen.

use std::borrow::Cow;

use pulsar_core::config::{
    FocusTransition, WidgetLayer, WidgetLayout, WidgetMaterial, WidgetStyle,
};
use pulsar_core::display::{
    CoreLoad, NetworkFloor, area_points, core_loads, graph_max, meter_fraction, metric_for,
};
use pulsar_core::format::format_value;
use pulsar_core::hardware::cpu::{cached_brand, cached_topology};
use pulsar_core::history::{History, HistoryStore};
use pulsar_core::layout::{CellPart, Rect, TextMeasure};
use pulsar_core::metric::{ItemKind, MetricKey, Snapshot, Unit};
use pulsar_core::tr;
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM};
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DestroyWindow, EnumWindows, FindWindowExW, FindWindowW, GW_HWNDPREV,
    GWL_EXSTYLE, GetWindow, GetWindowLongPtrW, HWND_BOTTOM, HWND_NOTOPMOST, HWND_TOP, HWND_TOPMOST,
    IDC_ARROW, IDC_SIZEALL, IDC_SIZENESW, IDC_SIZENS, IDC_SIZENWSE, IDC_SIZEWE, LoadCursorW,
    RegisterClassW, SW_HIDE, SW_SHOWNOACTIVATE, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SetCursor,
    SetWindowLongPtrW, SetWindowPos, ShowWindow, WINDOW_EX_STYLE, WNDCLASSW, WNDPROC,
    WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};
use windows::core::{BOOL, Result, w};

use crate::glass::{Backdrop, Shape};
use crate::popup::content::{Block, PopupContent};
use crate::popup::layout::ellipsize;
use crate::render::Reorder;
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
    instance: HINSTANCE,
    surface: Option<Surface>,
    backdrop: Option<Backdrop>,
    no_backdrop: bool,
    shapes: Vec<Shape>,
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
            instance,
            surface: None,
            backdrop: None,
            no_backdrop: false,
            shapes: Vec::new(),
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
        let size = look
            .size
            .or_else(|| natural_size(look, contents, snapshot, history, text));
        let arranged = size.map(|(w, h)| {
            (
                w,
                h,
                chosen(look.layout, w / look.scale, h / look.scale, contents.len()),
            )
        });
        let unsized_style = match look.layout {
            WidgetLayout::Dashboard => Some(WidgetStyle::Cards),
            WidgetLayout::Lanes => Some(WidgetStyle::Sparklines),
            WidgetLayout::Tiles => Some(WidgetStyle::Tiles),
            WidgetLayout::Meters => Some(WidgetStyle::Meters),
            WidgetLayout::Dials => Some(WidgetStyle::Dials),
            _ => None,
        };
        let panel_gap = PANEL_GAP_DIP * look.scale;
        let (column_w, max_h, forced) = match arranged {
            Some((w, h, Arrangement::Dashboard { columns })) => {
                let n = columns as f32;
                let gaps = if look.separate {
                    (n - 1.0) * panel_gap
                } else {
                    0.0
                };
                ((w - gaps) / n, h, Some(WidgetStyle::Cards))
            }
            Some((w, _, Arrangement::Lanes)) => (w, f32::MAX, Some(WidgetStyle::Sparklines)),
            Some((w, _, Arrangement::Tiles)) => (w, f32::MAX, Some(WidgetStyle::Tiles)),
            Some((w, _, Arrangement::Meters)) => (w, f32::MAX, Some(WidgetStyle::Meters)),
            Some((w, _, Arrangement::Dials)) => (w, f32::MAX, Some(WidgetStyle::Dials)),
            Some((w, h, Arrangement::Focus)) => (w, h, Some(WidgetStyle::Cards)),
            Some((w, _, Arrangement::Strip | Arrangement::Rings)) => (w, f32::MAX, None),
            None => (look.column_w, look.max_h, unsized_style),
        };
        let d = Dims::new(text, look, column_w);
        let mut sections: Vec<Section> = contents
            .iter()
            .map(|(c, style)| Section::new(c, forced.unwrap_or(*style), snapshot, history))
            .collect();
        let row = arranged.and_then(|(w, h, a)| match a {
            Arrangement::Strip => Some((w, h, false)),
            Arrangement::Rings => Some((w, h, true)),
            _ => None,
        });
        let focus = matches!(arranged, Some((_, _, Arrangement::Focus)));
        let shown = look.focus.index % sections.len().max(1);
        let separate = look.separate && row.is_none();
        let mut plan = match (row, arranged) {
            (Some((w, h, rings)), _) => {
                let radius = row_radius(look.row_radius, h);
                Plan::row(&sections, &d, w, h, rings, radius, text)
            }
            (None, Some((w, h, Arrangement::Focus))) => {
                Plan::focus(&mut sections, shown, &d, w, h, text)
            }
            (None, _) => {
                let balance = match (arranged, look.layout) {
                    (Some((_, _, Arrangement::Dashboard { columns })), _) => Some(columns),
                    (None, WidgetLayout::Dashboard) => Some(sections.len().div_ceil(2).clamp(1, 3)),
                    _ => None,
                };
                Plan::new(&mut sections, &d, max_h, separate, balance, text)
            }
        };
        if let Some((w, h, _)) = arranged {
            plan.fit(w, h, separate);
        }
        let blur = look.material.blurs() && look.transparency && self.ensure_backdrop();
        let skin = Skin::new(look, palette, blur);
        let radius = match row {
            Some((_, h, _)) => row_radius(look.row_radius, h),
            None => skin.radius * d.scale,
        };
        self.shapes = plan.panels.iter().map(|&r| (r, radius)).collect();
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
            bare_tiles: look.separate,
        };
        renderer.paint(surface, Color::rgb(0, 0, 0).with_alpha(0.0), |r| {
            let p = palette;
            if arranged.is_some() {
                // Fully clear pixels let clicks through, which would leave
                // nothing to grab where the cards stop short of the edges.
                let band = RESIZE_BAND_DIP * d.scale;
                let hit = Color::rgb(0, 0, 0).with_alpha(1.0 / 255.0);
                let (wf, hf) = (w as f32, h as f32);
                for edge in [
                    Rect {
                        x: 0.0,
                        y: 0.0,
                        w: band,
                        h: hf,
                    },
                    Rect {
                        x: wf - band,
                        y: 0.0,
                        w: band,
                        h: hf,
                    },
                    Rect {
                        x: 0.0,
                        y: 0.0,
                        w: wf,
                        h: band,
                    },
                    Rect {
                        x: 0.0,
                        y: hf - band,
                        w: wf,
                        h: band,
                    },
                ] {
                    r.fill_rounded(edge, 0.0, hit)?;
                }
            }
            for &panel in &plan.panels {
                skin.panel(r, panel, radius, d.scale, look.border)?;
            }
            for placed in &plan.placed {
                let s = &sections[placed.index];
                match row {
                    Some((_, _, true)) => ctx.ring_cell(r, s, placed.rect)?,
                    Some((_, _, false)) => ctx.strip_cell(r, s, placed.rect)?,
                    None if focus => {
                        let leaving = look.focus.leaving.and_then(|(from, p)| {
                            let from = from % sections.len();
                            (from != placed.index && p < 1.0).then_some((from, p))
                        });
                        match leaving {
                            Some((from, p)) => {
                                let at = placed.rect;
                                let [out, into] =
                                    transition_frames(look.focus.transition, p, (at.w, at.h));
                                let gone = &sections[from];
                                r.layered(at, out.0, out.1, || ctx.focus(r, gone, at))?;
                                r.layered(at, into.0, into.1, || ctx.focus(r, s, at))?;
                            }
                            None => ctx.focus(r, s, placed.rect)?,
                        }
                    }
                    None => ctx.section(r, s, placed.rect, placed.value_w)?,
                }
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
            if let Some(reorder) = look.reorder {
                let cells = plan
                    .placed
                    .iter()
                    .map(|c| (c.rect, sections[c.index].content.kind));
                r.draw_reorder(p, cells, reorder)?;
            }
            Ok(())
        })?;
        let (x, y) = at(w, h);
        renderer.present_alpha(self.hwnd, surface, Some((x, y)), look.alpha)?;
        if self.shown_at.is_none() {
            unsafe {
                let _ = ShowWindow(self.hwnd, SW_SHOWNOACTIVATE);
            }
        }
        self.shown_at = Some((x, y));
        if blur {
            self.place_backdrop();
            if let Some(backdrop) = &self.backdrop {
                backdrop.set_opacity(f32::from(look.alpha) / 255.0);
            }
        } else if let Some(backdrop) = &mut self.backdrop {
            backdrop.hide();
        }
        Ok(())
    }

    pub fn present_frame(&mut self, renderer: &Renderer, x: i32, y: i32, alpha: u8) -> Result<()> {
        let Some(surface) = &self.surface else {
            return Ok(());
        };
        renderer.present_alpha(self.hwnd, surface, Some((x, y)), alpha)?;
        self.shown_at = Some((x, y));
        if self.backdrop.as_ref().is_some_and(Backdrop::shown) {
            self.place_backdrop();
            if let Some(backdrop) = &self.backdrop {
                backdrop.set_opacity(f32::from(alpha) / 255.0);
            }
        }
        Ok(())
    }

    pub fn bounds(&self) -> Option<Rect32> {
        let (x, y) = self.shown_at?;
        let (w, h) = self.size();
        Some(Rect32 {
            left: x,
            top: y,
            right: x + w,
            bottom: y + h,
        })
    }

    fn ensure_backdrop(&mut self) -> bool {
        if self.backdrop.is_none() && !self.no_backdrop {
            match Backdrop::create(self.instance) {
                Ok(backdrop) => self.backdrop = Some(backdrop),
                Err(e) => {
                    log::info!("no blur behind the widget: {e}");
                    self.no_backdrop = true;
                }
            }
        }
        self.backdrop.is_some()
    }

    fn place_backdrop(&mut self) {
        let (Some(backdrop), Some((x, y))) = (&mut self.backdrop, self.shown_at) else {
            return;
        };
        let (w, h) = self
            .surface
            .as_ref()
            .map_or((0, 0), |s| (s.width, s.height));
        if let Err(e) = backdrop.set_shapes(&self.shapes) {
            log::warn!("could not shape the widget's blur: {e}");
            self.backdrop = None;
            self.no_backdrop = true;
            return;
        }
        backdrop.place(self.hwnd, x, y, w, h);
    }

    pub fn cells(&self) -> &[(Rect, ItemKind)] {
        &self.hits
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
            if self.backdrop.as_ref().is_some_and(Backdrop::shown) {
                self.place_backdrop();
            }
        }
        Ok(())
    }

    pub fn hide(&mut self) {
        if self.shown_at.take().is_some() {
            unsafe {
                let _ = ShowWindow(self.hwnd, SW_HIDE);
            }
        }
        if let Some(backdrop) = &mut self.backdrop {
            backdrop.hide();
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
                if self.backdrop.as_ref().is_some_and(Backdrop::shown) {
                    self.place_backdrop();
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
        if self.backdrop.as_ref().is_some_and(Backdrop::shown) {
            self.place_backdrop();
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
    /// `None` uses the theme's panel color.
    pub background: Option<Color>,
    pub reorder: Option<Reorder>,
    pub scale: f32,
    pub network_floor: NetworkFloor,
    pub material: WidgetMaterial,
    pub separate: bool,
    /// Windows' "Transparency effects" setting.
    pub transparency: bool,
    pub alpha: u8,
    /// Set by dragging its edges, in physical pixels; picks the layout.
    pub size: Option<(f32, f32)>,
    pub layout: WidgetLayout,
    /// Physical pixels; `None` rounds a row's ends fully.
    pub row_radius: Option<f32>,
    /// 0–1; Glass is clear at 0.
    pub glass_tint: f32,
    pub focus: FocusView,
    pub border: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FocusView {
    pub index: usize,
    /// The item shown before, and how far the change has come, 0–1.
    pub leaving: Option<(usize, f32)>,
    pub transition: FocusTransition,
}

/// Opacity and offset of the item leaving and the item arriving, `p` of the
/// way through a change across a `w` × `h` area.
pub fn transition_frames(
    transition: FocusTransition,
    p: f32,
    (w, h): (f32, f32),
) -> [(f32, (f32, f32)); 2] {
    let e = 1.0 - (1.0 - p.clamp(0.0, 1.0)).powi(3);
    match transition {
        FocusTransition::None => [(0.0, (0.0, 0.0)), (1.0, (0.0, 0.0))],
        FocusTransition::Fade => [(1.0 - e, (0.0, 0.0)), (e, (0.0, 0.0))],
        FocusTransition::Slide => [(1.0, (-e * w, 0.0)), (1.0, ((1.0 - e) * w, 0.0))],
        FocusTransition::SlideUp => [(1.0, (0.0, -e * h)), (1.0, (0.0, (1.0 - e) * h))],
    }
}

struct Skin {
    fill: Color,
    border: Color,
    /// In DIPs.
    radius: f32,
}

impl Skin {
    fn new(look: &Look, p: &Palette, blur: bool) -> Self {
        let base = look.background.unwrap_or(p.popup);
        let light = p.light();
        let edge = |dark: f32, bright: f32| {
            if light {
                Color::rgb(0, 0, 0).with_alpha(bright)
            } else {
                Color::rgb(0xFF, 0xFF, 0xFF).with_alpha(dark)
            }
        };
        let solid = |radius| Self {
            // Never fully clear, so the whole panel takes the mouse.
            fill: base.with_alpha(look.opacity.max(1.0 / 255.0)),
            border: p
                .popup_border
                .with_alpha(p.popup_border.a * look.opacity.max(0.3)),
            radius,
        };
        match look.material {
            WidgetMaterial::Solid => solid(8.0),
            WidgetMaterial::Glass if blur => Self {
                fill: base.with_alpha((GLASS_MAX_TINT * look.glass_tint).max(1.0 / 255.0)),
                border: edge(0.22, 0.10),
                radius: 16.0,
            },
            WidgetMaterial::Acrylic if blur => Self {
                fill: base.with_alpha((0.8 * look.opacity).max(1.0 / 255.0)),
                border: edge(0.09, 0.06),
                radius: 8.0,
            },
            WidgetMaterial::Glass => solid(16.0),
            WidgetMaterial::Acrylic => solid(8.0),
            WidgetMaterial::Clear => Self {
                fill: base.with_alpha((0.28 * look.opacity).max(1.0 / 255.0)),
                border: edge(0.10, 0.08),
                radius: 12.0,
            },
        }
    }

    fn panel(&self, r: &Renderer, at: Rect, radius: f32, scale: f32, border: bool) -> Result<()> {
        r.fill_rounded(at, radius, self.fill)?;
        if !border {
            return Ok(());
        }
        r.stroke_rounded(at, radius, scale.max(1.0), self.border)
    }
}

const DIALS_PER_ROW: usize = 3;
/// A full tint still lets some of the blur through.
const GLASS_MAX_TINT: f32 = 0.85;
const PANEL_GAP_DIP: f32 = 8.0;
pub const RESIZE_BAND_DIP: f32 = 8.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Arrangement {
    Dashboard { columns: usize },
    Lanes,
    Tiles,
    Meters,
    Dials,
    Strip,
    Rings,
    Focus,
}

const DASHBOARD_COLUMN_DIP: f32 = 300.0;
const STRIP_HEIGHTS_DIP: (f32, f32) = (32.0, 52.0);
const RINGS_HEIGHT_DIP: f32 = 60.0;

/// The heights a chosen row layout may be dragged between, in DIPs.
pub fn height_range(layout: WidgetLayout) -> Option<(f32, f32)> {
    match layout {
        WidgetLayout::Strip => Some(STRIP_HEIGHTS_DIP),
        WidgetLayout::Rings => Some((RINGS_HEIGHT_DIP, RINGS_HEIGHT_DIP)),
        WidgetLayout::Focus => Some((FOCUS_MIN_DIP.1, f32::MAX)),
        _ => None,
    }
}

/// The narrowest a chosen layout may be dragged, in DIPs.
pub fn min_width(layout: WidgetLayout) -> f32 {
    match layout {
        WidgetLayout::Focus => FOCUS_MIN_DIP.0,
        _ => 0.0,
    }
}

/// A chosen layout's size when the widget fits its content; `None` where the
/// content itself sets the size.
fn natural_size(
    look: &Look,
    contents: &[(PopupContent, WidgetStyle)],
    snapshot: &Snapshot,
    history: &HistoryStore,
    text: &Text,
) -> Option<(f32, f32)> {
    let s = look.scale;
    let n = contents.len().max(1) as f32;
    match look.layout {
        WidgetLayout::Strip => {
            let h = STRIP_HEIGHTS_DIP.1 * s;
            let d = Dims::new(text, look, look.column_w);
            let cells: f32 = contents
                .iter()
                .map(|(c, style)| {
                    strip_width(&Section::new(c, *style, snapshot, history), &d, text)
                })
                .sum();
            let end = row_radius(look.row_radius, h).max(d.pad);
            Some(((cells + 2.0 * end).ceil(), h))
        }
        WidgetLayout::Rings => {
            let h = RINGS_HEIGHT_DIP * s;
            Some(((n * h * 0.85 + 2.0 * h * 0.15).ceil(), h))
        }
        WidgetLayout::Focus => Some((look.column_w, (look.column_w * FOCUS_ASPECT).round())),
        _ => None,
    }
}

fn row_radius(radius: Option<f32>, h: f32) -> f32 {
    radius.map_or(h / 2.0, |r| r.min(h / 2.0))
}
const FOCUS_VALUE_SCALE: f32 = 1.6;
const FOCUS_ASPECT: f32 = 0.8;
/// Room for the title, the large value, a graph and a line of details.
const FOCUS_MIN_DIP: (f32, f32) = (180.0, 140.0);
const STRIP_CELL_DIP: f32 = 92.0;

pub fn chosen(layout: WidgetLayout, w: f32, h: f32, items: usize) -> Arrangement {
    match layout {
        WidgetLayout::Auto => arrangement(w, h, items),
        WidgetLayout::Dashboard => Arrangement::Dashboard {
            columns: dashboard_columns(w, items),
        },
        WidgetLayout::Lanes => Arrangement::Lanes,
        WidgetLayout::Tiles => Arrangement::Tiles,
        WidgetLayout::Meters => Arrangement::Meters,
        WidgetLayout::Dials => Arrangement::Dials,
        WidgetLayout::Strip => Arrangement::Strip,
        WidgetLayout::Rings => Arrangement::Rings,
        WidgetLayout::Focus => Arrangement::Focus,
    }
}

fn dashboard_columns(w: f32, items: usize) -> usize {
    let fit = ((w + PANEL_GAP_DIP) / (DASHBOARD_COLUMN_DIP + PANEL_GAP_DIP)) as usize;
    fit.clamp(1, items.max(1))
}

/// The layout for a `w` × `h` DIP widget with `items` items.
pub fn arrangement(w: f32, h: f32, items: usize) -> Arrangement {
    let items = items.max(1);
    if h < 72.0 {
        return if w >= items as f32 * STRIP_CELL_DIP + 24.0 {
            Arrangement::Strip
        } else {
            Arrangement::Rings
        };
    }
    if items == 1 && w >= FOCUS_MIN_DIP.0 && h >= FOCUS_MIN_DIP.1 {
        return Arrangement::Focus;
    }
    if w >= 600.0 && h >= 200.0 {
        return Arrangement::Dashboard {
            columns: dashboard_columns(w, items),
        };
    }
    if w >= 0.8 * h {
        Arrangement::Tiles
    } else {
        Arrangement::Lanes
    }
}
const MAX_DIAL_DIP: f32 = 76.0;

struct Dims {
    scale: f32,
    pad: f32,
    gap: f32,
    panel_gap: f32,
    big_px: f32,
    big_h: f32,
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
    fn new(text: &Text, look: &Look, column_w: f32) -> Self {
        let s = look.scale;
        let small_px = look.font_px * 0.88;
        let pad = if look.separate { 12.0 } else { 10.0 } * s;
        let inner = column_w - 2.0 * pad;
        let dial_cell = (inner - 2.0 * 8.0 * s) / DIALS_PER_ROW as f32;
        let big_px = look.font_px * 2.2;
        Self {
            scale: s,
            pad,
            gap: 8.0 * s,
            panel_gap: PANEL_GAP_DIP * s,
            big_px,
            big_h: text.line_height(big_px),
            between: 9.0 * s,
            font_px: look.font_px,
            line_h: text.line_height(look.font_px),
            small_px,
            small_h: text.line_height(small_px),
            strip_h: 6.0 * s,
            tile_pad: 6.0 * s,
            tile_graph_h: 18.0 * s,
            column_w,
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
    model: Option<String>,
    side: Option<String>,
    latest: Option<f64>,
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
        let (model, side) = if style == WidgetStyle::Cards {
            card_head(content.kind, snapshot)
        } else {
            (None, None)
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
            model,
            side,
            latest: snapshot.get(key).filter(|v| v.is_finite()),
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
        let head = if self.style == WidgetStyle::Cards {
            d.line_h + d.gap / 2.0 + d.big_h
        } else {
            d.line_h
        };
        head + self.details_height(d)
    }

    /// Height of what `Ctx::details` draws, and the core wall above it.
    fn details_height(&self, d: &Dims) -> f32 {
        let mut h = 0.0;
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
    panels: Vec<Rect>,
}

impl Plan {
    /// `balance` spreads the sections evenly over that many columns.
    fn new(
        sections: &mut [Section],
        d: &Dims,
        max_h: f32,
        separate: bool,
        balance: Option<usize>,
        text: &dyn TextMeasure,
    ) -> Self {
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
        let (between, cell_gap, column_gap) = if separate {
            let apart = d.panel_gap + 2.0 * d.pad;
            (apart, apart, d.panel_gap)
        } else {
            (d.between, d.gap, 0.0)
        };
        let room = max_h - 2.0 * d.pad;
        let limit = balance.map_or(room, |count| balanced(&heights, between, count).min(room));
        let columns = columns(&heights, between, limit);
        let mut placed = Vec::new();
        let mut tallest: f32 = 0.0;
        for (c, column) in columns.iter().enumerate() {
            let x = d.pad + c as f32 * (d.column_w + column_gap);
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
                let cell_w = (inner - (across - 1.0) * cell_gap) / across;
                for (k, &index) in units[u].iter().enumerate() {
                    let rect = if is_line || wide[index] {
                        Rect { x, y, w: inner, h }
                    } else {
                        Rect {
                            x: x + k as f32 * (cell_w + cell_gap),
                            y,
                            w: cell_w,
                            h,
                        }
                    };
                    placed.push(Placed {
                        index,
                        rect,
                        value_w,
                        divider: !separate && is_line && next_is_line,
                    });
                }
                y += h;
                if n + 1 < column.len() {
                    y += between;
                }
            }
            tallest = tallest.max(y - d.pad);
        }
        let n = columns.len().max(1) as f32;
        let width = n * d.column_w + (n - 1.0) * column_gap;
        let height = tallest + 2.0 * d.pad;
        let panels = if separate {
            placed
                .iter()
                .map(|p| Rect {
                    x: p.rect.x - d.pad,
                    y: p.rect.y - d.pad,
                    w: p.rect.w + 2.0 * d.pad,
                    h: p.rect.h + 2.0 * d.pad,
                })
                .collect()
        } else {
            vec![Rect {
                x: 0.0,
                y: 0.0,
                w: width,
                h: height,
            }]
        };
        Self {
            width,
            height,
            placed,
            panels,
        }
    }
}

fn card_head(kind: ItemKind, snapshot: &Snapshot) -> (Option<String>, Option<String>) {
    let temperature = |key| {
        snapshot
            .get(key)
            .filter(|v| v.is_finite())
            .map(|v| format_value(v, Unit::Celsius))
    };
    match kind {
        ItemKind::Cpu => (
            Some(cached_brand().to_string()).filter(|b| !b.is_empty()),
            temperature(MetricKey::CpuTempC),
        ),
        ItemKind::Gpu => (
            snapshot.gpus.present().next().map(|(_, g)| g.name.clone()),
            temperature(MetricKey::GpuTempC),
        ),
        _ => (None, None),
    }
}

fn split_unit(value: &str) -> (&str, &str) {
    let starts_with_number = value.chars().next().is_some_and(|c| c.is_ascii_digit());
    match value.rfind(|c: char| c.is_ascii_digit()) {
        Some(end) if starts_with_number => {
            let (number, unit) = value.split_at(end + 1);
            (number, unit.trim_start())
        }
        _ => (value, ""),
    }
}

impl Plan {
    /// One row across `w` × `h`, as many items as fit: rings at a fixed
    /// pitch, strip cells as wide as their text with the spare room shared.
    fn row(
        sections: &[Section],
        d: &Dims,
        w: f32,
        h: f32,
        rings: bool,
        radius: f32,
        text: &Text,
    ) -> Self {
        let end = if rings { h * 0.15 } else { radius.max(d.pad) };
        let room = (w - 2.0 * end).max(0.0);
        let mut widths = Vec::new();
        let mut used = 0.0;
        for s in sections {
            let cell = if rings {
                h * 0.85
            } else {
                strip_width(s, d, text)
            };
            if used + cell > room {
                break;
            }
            used += cell;
            widths.push(cell);
        }
        let extra = (room - used) / widths.len().max(1) as f32;
        let mut x = end;
        let placed = widths
            .iter()
            .enumerate()
            .map(|(index, &cell)| {
                let rect = Rect {
                    x,
                    y: 0.0,
                    w: cell + extra,
                    h,
                };
                x += cell + extra;
                Placed {
                    index,
                    rect,
                    value_w: 0.0,
                    divider: false,
                }
            })
            .collect();
        Self {
            width: w,
            height: h,
            placed,
            panels: vec![Rect {
                x: 0.0,
                y: 0.0,
                w,
                h,
            }],
        }
    }

    fn focus(
        sections: &mut [Section],
        index: usize,
        d: &Dims,
        w: f32,
        h: f32,
        text: &Text,
    ) -> Self {
        let rect = Rect {
            x: d.pad,
            y: d.pad,
            w: (w - 2.0 * d.pad).max(0.0),
            h: (h - 2.0 * d.pad).max(0.0),
        };
        for s in sections.iter_mut() {
            s.lay_out_pairs(d, rect.w, text);
        }
        let panel = Rect {
            x: 0.0,
            y: 0.0,
            w,
            h,
        };
        Self {
            width: w,
            height: h,
            placed: sections
                .get(index)
                .map(|_| Placed {
                    index,
                    rect,
                    value_w: 0.0,
                    divider: false,
                })
                .into_iter()
                .collect(),
            panels: vec![panel],
        }
    }

    fn fit(&mut self, w: f32, h: f32, separate: bool) {
        let inside = |r: &Rect| r.right() <= w + 0.5 && r.y + r.h <= h + 0.5;
        if separate {
            let kept: Vec<bool> = self.panels.iter().map(inside).collect();
            let mut keep = kept.iter();
            self.placed.retain(|_| *keep.next().unwrap_or(&false));
            let mut keep = kept.iter();
            self.panels.retain(|_| *keep.next().unwrap_or(&false));
        } else {
            self.placed.retain(|p| inside(&p.rect));
            self.panels = vec![Rect {
                x: 0.0,
                y: 0.0,
                w,
                h,
            }];
        }
        self.width = w;
        self.height = h;
    }
}

fn strip_width(s: &Section, d: &Dims, text: &Text) -> f32 {
    let dot = 6.0 * d.scale;
    dot + d.gap / 1.5
        + text.width(s.content.title, d.small_px)
        + d.gap / 1.5
        + text.width(&s.content.value, d.font_px)
        + d.gap
}

/// The first number in `value`, such as the download rate of a network
/// pair; the whole text when it has none.
fn first_number(value: &str) -> &str {
    let Some(digits) = value.find(|c: char| c.is_ascii_digit()) else {
        return value;
    };
    let start = match value[..digits].chars().next_back() {
        Some(sign @ ('-' | '\u{2212}')) => digits - sign.len_utf8(),
        _ => digits,
    };
    let rest = &value[digits..];
    let end = rest
        .find(|c: char| !(c.is_ascii_digit() || c == '.' || c == ','))
        .unwrap_or(rest.len());
    &value[start..digits + end]
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
        WidgetStyle::Sparklines
        | WidgetStyle::Meters
        | WidgetStyle::CoreWall
        | WidgetStyle::Cards => 1,
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

fn balanced(heights: &[f32], between: f32, count: usize) -> f32 {
    let total = heights.iter().sum::<f32>() + between * heights.len().saturating_sub(1) as f32;
    let mut limit = heights
        .iter()
        .copied()
        .fold(0.0, f32::max)
        .max(total / count.max(1) as f32);
    while columns(heights, between, limit).len() > count.max(1) {
        limit += 1.0;
    }
    limit
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
    bare_tiles: bool,
}

impl Ctx<'_> {
    fn section(&self, r: &Renderer, s: &Section, at: Rect, value_w: f32) -> Result<()> {
        match s.style {
            WidgetStyle::Tiles => self.tile(r, s, at),
            WidgetStyle::Dials => self.dial(r, s, at),
            WidgetStyle::Cards => self.card(r, s, at),
            _ => self.lines(r, s, at, value_w),
        }
    }

    fn card(&self, r: &Renderer, s: &Section, at: Rect) -> Result<()> {
        let d = self.d;
        let (baseline, x) = self.card_top(r, s, at, at.w * 0.55, d.big_px)?;
        let y = at.y + d.line_h + d.gap / 2.0;
        let graph = Rect {
            x: x + 1.5 * d.gap,
            y: y + d.big_h * 0.1,
            w: at.right() - (x + 1.5 * d.gap),
            h: d.big_h * 0.85,
        };
        if s.content.graph && graph.w >= 24.0 * d.scale {
            self.graph(r, s, graph, 1.4)?;
        }
        self.details(r, s, at, baseline)
    }

    fn focus(&self, r: &Renderer, s: &Section, at: Rect) -> Result<()> {
        let (d, p) = (self.d, self.palette);
        let (baseline, _) = self.card_top(r, s, at, at.w, d.big_px * FOCUS_VALUE_SCALE)?;
        let details = at.y + at.h - s.details_height(d);
        let range = self.range(s);
        let range_y = details - d.small_h;
        let bottom = if range.is_some() { range_y } else { details };
        let graph = Rect {
            x: at.x,
            y: baseline + d.gap,
            w: at.w,
            h: bottom - d.gap / 2.0 - (baseline + d.gap),
        };
        if let Some((min, max)) = &range {
            let (t, small) = (self.text, self.text.format(d.small_px)?);
            let mut x = at.x;
            for (label, value) in [(tr!("Min"), min), (tr!("Max"), max)] {
                let label_w = t.width(label, d.small_px);
                let value_w = t.width(value, d.small_px);
                if x + label_w + d.gap / 2.0 + value_w > at.right() {
                    break;
                }
                r.text_at(t, &small, label, x, range_y, p.popup_label)?;
                x += label_w + d.gap / 2.0;
                r.text_at(t, &small, value, x, range_y, p.popup_text)?;
                x += value_w + 2.0 * d.gap;
            }
        }
        if s.content.graph && graph.h >= 16.0 * d.scale {
            let line = d.scale.max(1.0);
            for quarter in 1..4 {
                let y = (graph.y + graph.h * quarter as f32 / 4.0).round();
                r.fill_rounded(
                    Rect {
                        y,
                        h: line,
                        ..graph
                    },
                    0.0,
                    p.popup_border,
                )?;
            }
            self.graph(r, s, graph, 1.8)?;
        }
        self.details(r, s, at, details)
    }

    fn range(&self, s: &Section) -> Option<(String, String)> {
        let (histories, _) = self.series(s);
        let (_, history) = histories.first()?;
        let unit = metric_for(s.content.kind, s.content.series[0]).unit();
        Some((
            format_value(history.min()?, unit),
            format_value(history.max()?, unit),
        ))
    }

    /// The title line and the value under it, at most `big_px` and within
    /// `room`; returns the value's baseline and right end.
    fn card_top(
        &self,
        r: &Renderer,
        s: &Section,
        at: Rect,
        room: f32,
        big_px: f32,
    ) -> Result<(f32, f32)> {
        let (d, p, t) = (self.d, self.palette, self.text);
        let c = s.content;
        let body = t.format(d.font_px)?;
        let small = t.format(d.small_px)?;
        let mut right = at.right();
        if let Some(side) = &s.side {
            let side = ellipsize(side, at.w * 0.4, d.small_px, t);
            right -= t.width(&side, d.small_px);
            let y = at.y + (d.line_h - d.small_h);
            r.text_at(t, &small, &side, right, y, p.popup_text)?;
            right -= d.gap;
        }
        let title = ellipsize(c.title, right - at.x, d.font_px, t);
        r.text_at(t, &body, &title, at.x, at.y, p.popup_text)?;
        if let Some(model) = &s.model {
            let x = at.x + t.width(&title, d.font_px) + d.gap;
            let model = ellipsize(model, right - x, d.small_px, t);
            let y = at.y + (d.line_h - d.small_h);
            r.text_at(t, &small, &model, x, y, p.popup_label)?;
        }

        let y = at.y + d.line_h + d.gap / 2.0;
        let (number, unit) = split_unit(&c.value);
        let unit_w = t.width(unit, d.font_px);
        let big_h = t.line_height(big_px);
        let smallest = d.font_px * 1.25;
        let room = room - 2.0 * d.scale;
        let big_px = [1.0, 0.85, 0.7, 0.58, 0.48]
            .into_iter()
            .map(|f| (big_px * f).max(smallest))
            .chain([smallest])
            .find(|&px| t.width(number, px) + unit_w <= room)
            .unwrap_or(smallest);
        let number = ellipsize(number, room - unit_w, big_px, t);
        let base = p.graph(c.kind, c.series[0]);
        let loaded = p.graph_at(c.kind, c.series[0], s.latest);
        let color = if loaded == base { p.popup_text } else { loaded };
        let baseline = y + big_h;
        let number_y = baseline - t.line_height(big_px);
        r.text_at(t, &t.format(big_px)?, &number, at.x, number_y, color)?;
        let mut x = at.x + t.width(&number, big_px);
        if !unit.is_empty() {
            x += d.scale * 2.0;
            let unit_y = baseline - d.line_h - big_h * 0.06;
            r.text_at(t, &body, unit, x, unit_y, p.popup_label)?;
            x += unit_w;
        }
        Ok((baseline, x))
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
        self.details(r, s, at, y)
    }

    fn details(&self, r: &Renderer, s: &Section, at: Rect, mut y: f32) -> Result<()> {
        let (d, p, t) = (self.d, self.palette, self.text);
        let c = s.content;
        let small = t.format(d.small_px)?;
        let accent = self.accent(s);
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
        if !self.bare_tiles {
            r.fill_rounded(at, 6.0 * d.scale, p.popup_border)?;
        }
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

    fn strip_cell(&self, r: &Renderer, s: &Section, at: Rect) -> Result<()> {
        let (d, p, t) = (self.d, self.palette, self.text);
        let c = s.content;
        let middle = at.y + at.h / 2.0;
        let dot = 6.0 * d.scale;
        let mut x = at.x + (at.w - strip_width(s, d, t) + d.gap) / 2.0;
        let mark = Rect {
            x,
            y: middle - dot / 2.0,
            w: dot,
            h: dot,
        };
        r.fill_rounded(mark, dot / 2.0, self.accent(s))?;
        x += dot + d.gap / 1.5;
        let small = t.format(d.small_px)?;
        r.text_at(
            t,
            &small,
            c.title,
            x,
            middle - d.small_h / 2.0,
            p.popup_label,
        )?;
        x += t.width(c.title, d.small_px) + d.gap / 1.5;
        let body = t.format(d.font_px)?;
        let base = p.graph(c.kind, c.series[0]);
        let loaded = p.graph_at(c.kind, c.series[0], s.latest);
        let color = if loaded == base { p.popup_text } else { loaded };
        let value = ellipsize(&c.value, at.right() - x, d.font_px, t);
        r.text_at(t, &body, &value, x, middle - d.line_h / 2.0, color)
    }

    fn ring_cell(&self, r: &Renderer, s: &Section, at: Rect) -> Result<()> {
        let (d, p, t) = (self.d, self.palette, self.text);
        let c = s.content;
        let size = at.w.min(at.h) * 0.8;
        let stroke = (size * 0.1).max(2.0 * d.scale);
        let center = (at.x + at.w / 2.0, at.y + at.h / 2.0);
        let (histories, max) = self.series(s);
        let (fraction, color) = histories
            .first()
            .map_or((0.0, self.accent(s)), |(part, h)| {
                let latest = h.latest();
                (
                    meter_fraction(latest.unwrap_or(f64::NAN), max),
                    p.graph_at(c.kind, *part, latest),
                )
            });
        r.draw_ring(
            center,
            size / 2.0 - stroke / 2.0,
            stroke,
            fraction,
            color,
            p.popup_border,
        )?;
        let inside = size - 2.0 * stroke - 2.0 * d.scale;
        let number = first_number(&c.value);
        let px = [d.small_px, d.small_px * 0.8, d.small_px * 0.65]
            .into_iter()
            .find(|&px| t.width(number, px) <= inside)
            .unwrap_or(d.small_px * 0.65);
        let number = ellipsize(number, inside, px, t);
        let x = center.0 - t.width(&number, px) / 2.0;
        let y = center.1 - t.line_height(px) / 2.0;
        r.text_at(t, &t.format(px)?, &number, x, y, p.popup_text)
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
    set_cursor(IDC_SIZEALL);
}

pub fn resize_cursor(edges: Edges) {
    let across = edges.left || edges.right;
    let up = edges.top || edges.bottom;
    set_cursor(match (across, up) {
        (true, true) if edges.left == edges.top => IDC_SIZENWSE,
        (true, true) => IDC_SIZENESW,
        (true, false) => IDC_SIZEWE,
        _ => IDC_SIZENS,
    });
}

fn set_cursor(id: windows::core::PCWSTR) {
    unsafe {
        if let Ok(cursor) = LoadCursorW(None, id) {
            SetCursor(Some(cursor));
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Edges {
    pub left: bool,
    pub right: bool,
    pub top: bool,
    pub bottom: bool,
}

impl Edges {
    pub fn any(self) -> bool {
        self.left || self.right || self.top || self.bottom
    }
}

pub fn grip((x, y): (i32, i32), (w, h): (i32, i32), reach: i32) -> Option<Edges> {
    let edges = Edges {
        left: x >= 0 && x < reach,
        right: x >= w - reach && x < w,
        top: y >= 0 && y < reach,
        bottom: y >= h - reach && y < h,
    };
    edges.any().then_some(edges)
}

/// The size and top-left corner after dragging `edges` by `delta`: the edges
/// not dragged stay where they were.
pub fn resized(
    edges: Edges,
    (x, y): (i32, i32),
    (w, h): (i32, i32),
    (dx, dy): (i32, i32),
    clamp_w: impl Fn(i32) -> i32,
    clamp_h: impl Fn(i32) -> i32,
) -> ((i32, i32), (i32, i32)) {
    let new_w = clamp_w(match (edges.left, edges.right) {
        (true, _) => w - dx,
        (_, true) => w + dx,
        _ => w,
    });
    let new_h = clamp_h(match (edges.top, edges.bottom) {
        (true, _) => h - dy,
        (_, true) => h + dy,
        _ => h,
    });
    let x = if edges.left { x + w - new_w } else { x };
    let y = if edges.top { y + h - new_h } else { y };
    ((x, y), (new_w, new_h))
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

    fn draw_checked(kinds: &[(ItemKind, WidgetStyle)], separate: bool) -> Plan {
        use pulsar_core::config::Config;
        let text = Text::new(None, false).unwrap();
        let config = Config::default();
        let (snapshot, history) = sample();
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
            background: None,
            reorder: None,
            scale: 1.0,
            network_floor: NetworkFloor::default(),
            material: WidgetMaterial::Glass,
            separate,
            transparency: true,
            alpha: 255,
            size: None,
            layout: WidgetLayout::Auto,
            row_radius: None,
            glass_tint: 0.0,
            focus: FocusView::default(),
            border: true,
        };
        let d = Dims::new(&text, &look, look.column_w);
        let mut sections: Vec<Section> = contents
            .iter()
            .map(|(c, style)| Section::new(c, *style, &snapshot, &history))
            .collect();
        let plan = Plan::new(&mut sections, &d, look.max_h, separate, None, &text);
        for placed in &plan.placed {
            let r = placed.rect;
            assert!(r.x >= 0.0 && r.right() <= plan.width, "{:?}", r);
            assert!(r.y >= 0.0 && r.y + r.h <= plan.height, "{:?}", r);
            assert!(
                plan.panels.iter().any(|p| p.x <= r.x
                    && p.y <= r.y
                    && r.right() <= p.right()
                    && r.y + r.h <= p.y + p.h),
                "{r:?} is on a panel"
            );
        }
        let palette = Palette::new(false, &config, None, &crate::theme::Theme::windows());
        let ctx = Ctx {
            text: &text,
            palette: &palette,
            history: &history,
            network_floor: look.network_floor,
            d: &d,
            bare_tiles: separate,
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
        plan
    }

    #[test]
    fn dials_and_the_core_wall_draw_inside_the_panel() {
        let plan = draw_checked(
            &[
                (ItemKind::Cpu, WidgetStyle::CoreWall),
                (ItemKind::Cpu, WidgetStyle::Dials),
                (ItemKind::CpuTemp, WidgetStyle::Dials),
                (ItemKind::Ram, WidgetStyle::Dials),
                (ItemKind::Gpu, WidgetStyle::Dials),
                (ItemKind::Network, WidgetStyle::Dials),
            ],
            false,
        );
        assert_eq!(
            plan.placed[1].rect.y, plan.placed[3].rect.y,
            "dials share a row"
        );
        assert!(
            plan.placed[5].rect.w > plan.placed[1].rect.w,
            "network is wide"
        );
        assert_eq!(plan.panels.len(), 1);
    }

    #[test]
    fn separate_cards_each_get_a_panel_apart_from_the_others() {
        let plan = draw_checked(
            &[
                (ItemKind::Cpu, WidgetStyle::Cards),
                (ItemKind::Gpu, WidgetStyle::Cards),
                (ItemKind::Ram, WidgetStyle::Tiles),
                (ItemKind::Disk, WidgetStyle::Tiles),
                (ItemKind::Network, WidgetStyle::Cards),
            ],
            true,
        );
        assert_eq!(plan.panels.len(), plan.placed.len());
        for (i, a) in plan.panels.iter().enumerate() {
            for b in &plan.panels[i + 1..] {
                let apart =
                    a.right() < b.x || b.right() < a.x || a.y + a.h < b.y || b.y + b.h < a.y;
                assert!(apart, "{a:?} and {b:?} overlap");
            }
        }
        assert_eq!(plan.panels[0].x, 0.0);
        assert_eq!(plan.panels[0].y, 0.0);
        assert_eq!(
            plan.placed[2].rect.y, plan.placed[3].rect.y,
            "tiles still share a row"
        );
    }

    #[test]
    fn every_blurred_material_still_takes_the_mouse_when_fully_clear() {
        let config = pulsar_core::config::Config::default();
        let palette = Palette::new(false, &config, None, &crate::theme::Theme::windows());
        for material in [WidgetMaterial::Glass, WidgetMaterial::Acrylic] {
            let look = Look {
                column_w: 300.0,
                max_h: 600.0,
                font_px: 12.0,
                opacity: 0.0,
                background: None,
                reorder: None,
                scale: 1.0,
                network_floor: NetworkFloor::default(),
                material,
                separate: true,
                transparency: true,
                alpha: 255,
                size: None,
                layout: WidgetLayout::Auto,
                row_radius: None,
                glass_tint: 0.0,
                focus: FocusView::default(),
                border: false,
            };
            assert!(
                Skin::new(&look, &palette, true).fill.a > 0.0,
                "{material:?}"
            );
        }
    }

    #[test]
    fn the_size_picks_the_layout() {
        assert_eq!(arrangement(700.0, 60.0, 5), Arrangement::Strip);
        assert_eq!(arrangement(240.0, 60.0, 5), Arrangement::Rings);
        assert_eq!(
            arrangement(940.0, 320.0, 5),
            Arrangement::Dashboard { columns: 3 }
        );
        assert_eq!(
            arrangement(2400.0, 320.0, 2),
            Arrangement::Dashboard { columns: 2 },
            "no more columns than items"
        );
        assert_eq!(arrangement(300.0, 300.0, 5), Arrangement::Tiles);
        assert_eq!(arrangement(280.0, 600.0, 5), Arrangement::Lanes);
        assert_eq!(arrangement(300.0, 250.0, 1), Arrangement::Focus);
        assert_eq!(
            arrangement(300.0, 100.0, 1),
            Arrangement::Tiles,
            "too short for Focus"
        );
        assert_eq!(first_number("-5°C"), "-5");
        assert_eq!(first_number("\u{2212}12 °C"), "\u{2212}12");
        assert_eq!(first_number("↓ 3.2 MB/s"), "3.2");
        assert_eq!(arrangement(900.0, 600.0, 1), Arrangement::Focus, "any size");
        assert_eq!(
            arrangement(300.0, 60.0, 1),
            Arrangement::Strip,
            "too short for a graph"
        );
    }

    #[test]
    fn dragging_the_left_or_top_edge_keeps_the_far_edge_in_place() {
        let edges = |left, right, top, bottom| Edges {
            left,
            right,
            top,
            bottom,
        };
        let keep = |v: i32| v.clamp(100, 1000);
        let left = resized(
            edges(true, false, false, false),
            (500, 300),
            (400, 200),
            (-50, 0),
            keep,
            keep,
        );
        assert_eq!(left, ((450, 300), (450, 200)));
        let top_left = resized(
            edges(true, false, true, false),
            (500, 300),
            (400, 200),
            (30, 20),
            keep,
            keep,
        );
        assert_eq!(top_left, ((530, 320), (370, 180)));
        let floor = resized(
            edges(true, false, false, false),
            (500, 300),
            (400, 200),
            (390, 0),
            keep,
            keep,
        );
        assert_eq!(
            floor,
            ((800, 300), (100, 200)),
            "the right edge holds at the smallest width"
        );
        let right = resized(
            edges(false, true, false, true),
            (500, 300),
            (400, 200),
            (10, 10),
            keep,
            keep,
        );
        assert_eq!(right, ((500, 300), (410, 210)));
        assert_eq!(
            grip((2, 50), (400, 200), 8),
            Some(edges(true, false, false, false))
        );
        assert_eq!(
            grip((398, 2), (400, 200), 8),
            Some(edges(false, true, true, false))
        );
        assert_eq!(grip((200, 100), (400, 200), 8), None);
    }

    #[test]
    fn a_focus_change_hands_over_from_the_leaving_item_to_the_arriving_one() {
        let area = (300.0, 200.0);
        for t in [
            FocusTransition::Fade,
            FocusTransition::Slide,
            FocusTransition::SlideUp,
        ] {
            let [out, into] = transition_frames(t, 0.0, area);
            assert_eq!(out, (1.0, (0.0, 0.0)), "{t:?} starts on the old item");
            assert!(
                into.0 == 0.0 || into.1 != (0.0, 0.0),
                "{t:?} hides the new one"
            );
            let [out, into] = transition_frames(t, 1.0, area);
            assert_eq!(into, (1.0, (0.0, 0.0)), "{t:?} ends on the new item");
            assert!(
                out.0 == 0.0 || out.1 != (0.0, 0.0),
                "{t:?} has the old one gone"
            );
        }
        let [_, into] = transition_frames(FocusTransition::None, 0.0, area);
        assert_eq!(into, (1.0, (0.0, 0.0)));
    }

    #[test]
    fn a_chosen_layout_overrides_the_size() {
        assert_eq!(
            chosen(WidgetLayout::Auto, 300.0, 300.0, 5),
            Arrangement::Tiles
        );
        assert_eq!(
            chosen(WidgetLayout::Focus, 300.0, 300.0, 5),
            Arrangement::Focus
        );
        assert_eq!(
            chosen(WidgetLayout::Strip, 300.0, 300.0, 5),
            Arrangement::Strip
        );
        assert_eq!(
            chosen(WidgetLayout::Meters, 300.0, 300.0, 5),
            Arrangement::Meters
        );
        assert_eq!(
            chosen(WidgetLayout::Dials, 900.0, 60.0, 5),
            Arrangement::Dials
        );
        assert_eq!(
            chosen(WidgetLayout::Dashboard, 300.0, 100.0, 5),
            Arrangement::Dashboard { columns: 1 },
            "as many columns as fit"
        );
    }

    #[test]
    fn focus_fills_the_widget_with_one_items_graph_above_its_details() {
        use pulsar_core::config::Config;
        let text = Text::new(None, false).unwrap();
        let config = Config::default();
        let (snapshot, history) = sample();
        let options = crate::popup::content::Options::new(&config, ItemKind::Cpu, &snapshot.gpus);
        let content =
            crate::popup::content::build_with(ItemKind::Cpu, &snapshot, &history, &options);
        let look = Look {
            column_w: 300.0,
            max_h: 260.0,
            font_px: 12.0,
            opacity: 1.0,
            background: None,
            reorder: None,
            scale: 1.0,
            network_floor: NetworkFloor::default(),
            material: WidgetMaterial::Glass,
            separate: true,
            transparency: true,
            alpha: 255,
            size: Some((300.0, 260.0)),
            layout: WidgetLayout::Auto,
            row_radius: None,
            glass_tint: 0.0,
            focus: FocusView::default(),
            border: true,
        };
        let d = Dims::new(&text, &look, 300.0);
        let mut sections = vec![Section::new(
            &content,
            WidgetStyle::Cards,
            &snapshot,
            &history,
        )];
        let plan = Plan::focus(&mut sections, 0, &d, 300.0, 260.0, &text);
        assert_eq!((plan.width, plan.height), (300.0, 260.0));
        assert_eq!(plan.panels.len(), 1);
        let at = plan.placed[0].rect;
        assert!(at.x > 0.0 && at.right() < 300.0 && at.y + at.h < 260.0);
        let palette = Palette::new(false, &config, None, &crate::theme::Theme::windows());
        let ctx = Ctx {
            text: &text,
            palette: &palette,
            history: &history,
            network_floor: look.network_floor,
            d: &d,
            bare_tiles: true,
        };
        let surface = Surface::new(300, 260).unwrap();
        let mut renderer = Renderer::new().unwrap();
        let clear = Color::rgb(0, 0, 0).with_alpha(0.0);
        renderer
            .paint(&surface, clear, |r| ctx.focus(r, &sections[0], at))
            .unwrap();
        let bits = crate::render::tests::pixels(&surface);
        let range = d.small_h;
        let bottom = at.y + at.h - sections[0].details_height(&d) - range - d.gap / 2.0;
        let (x, y) = ((at.right() - 3.0) as usize, (bottom - 3.0) as usize);
        assert!(
            bits[y * 300 + x] >> 24 > 0,
            "the latest reading fills down to the details"
        );
        assert!(
            bottom - at.y > 260.0 / 2.0,
            "the graph takes most of the height"
        );
    }

    #[test]
    fn a_row_keeps_what_fits_and_a_sized_widget_drops_what_does_not() {
        use pulsar_core::config::Config;
        let text = Text::new(None, false).unwrap();
        let config = Config::default();
        let (snapshot, history) = sample();
        let kinds = [
            ItemKind::Cpu,
            ItemKind::Ram,
            ItemKind::Gpu,
            ItemKind::Disk,
            ItemKind::Network,
        ];
        let contents: Vec<PopupContent> = kinds
            .iter()
            .map(|&kind| {
                let options = crate::popup::content::Options::new(&config, kind, &snapshot.gpus);
                crate::popup::content::build_with(kind, &snapshot, &history, &options)
            })
            .collect();
        let look = Look {
            column_w: 300.0,
            max_h: 1000.0,
            font_px: 12.0,
            opacity: 1.0,
            background: None,
            reorder: None,
            scale: 1.0,
            network_floor: NetworkFloor::default(),
            material: WidgetMaterial::Glass,
            separate: true,
            transparency: true,
            alpha: 255,
            size: None,
            layout: WidgetLayout::Auto,
            row_radius: None,
            glass_tint: 0.0,
            focus: FocusView::default(),
            border: true,
        };
        let d = Dims::new(&text, &look, look.column_w);
        let mut sections: Vec<Section> = contents
            .iter()
            .map(|c| Section::new(c, WidgetStyle::Cards, &snapshot, &history))
            .collect();
        let rings = Plan::row(&sections, &d, 240.0, 60.0, true, 30.0, &text);
        assert!(!rings.placed.is_empty() && rings.placed.len() < kinds.len());
        let strip = Plan::row(&sections, &d, 900.0, 60.0, false, 30.0, &text);
        let square = Plan::row(&sections, &d, 900.0, 60.0, false, 0.0, &text);
        assert!(
            square.placed[0].rect.x < strip.placed[0].rect.x,
            "square ends need less room"
        );
        assert_eq!(strip.placed.len(), kinds.len());
        for plan in [&rings, &strip] {
            for p in &plan.placed {
                assert!(
                    p.rect.x >= 0.0 && p.rect.right() <= plan.width,
                    "{:?}",
                    p.rect
                );
            }
        }
        let spread = Plan::new(&mut sections, &d, 1000.0, true, Some(3), &text);
        let mut lefts: Vec<i32> = spread.placed.iter().map(|p| p.rect.x as i32).collect();
        lefts.dedup();
        assert_eq!(lefts.len(), 3, "balanced over three columns");
        let mut cards = Plan::new(&mut sections, &d, f32::MAX, true, None, &text);
        let tall = cards.height;
        cards.fit(300.0, tall / 2.0, true);
        assert!(!cards.placed.is_empty() && cards.placed.len() < kinds.len());
        assert_eq!(cards.placed.len(), cards.panels.len());
        assert!(cards.panels.iter().all(|p| p.y + p.h <= tall / 2.0 + 0.5));
    }

    #[test]
    fn a_card_splits_its_value_from_the_unit() {
        assert_eq!(split_unit("41%"), ("41", "%"));
        assert_eq!(split_unit("2.7 MB/s"), ("2.7", "MB/s"));
        assert_eq!(split_unit("1,234 MHz"), ("1,234", "MHz"));
        assert_eq!(split_unit("58 °C"), ("58", "°C"));
        assert_eq!(split_unit("↓ 2 KB/s ↑ 1 KB/s"), ("↓ 2 KB/s ↑ 1 KB/s", ""));
        assert_eq!(split_unit("Offline"), ("Offline", ""));
        assert_eq!(first_number("↓ 9.4 KB/s  ↑ 1 KB/s"), "9.4");
        assert_eq!(first_number("1,234 MHz"), "1,234");
        assert_eq!(first_number("—"), "—");
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
