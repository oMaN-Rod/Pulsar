use pulsar_core::config::Material;
use pulsar_core::layout::Rect;
use windows::core::Result;

use crate::render::Renderer;
use crate::theme::{Color, Palette};

/// A full tint still lets some of the blur through.
const GLASS_MAX_TINT: f32 = 0.85;
const LIQUID_MAX_TINT: f32 = 0.7;
/// How much of the blur covers what is behind Liquid glass; the rest shows
/// through sharp, so the blur reads lighter than Glass's.
const LIQUID_BLUR: f32 = 0.6;

pub struct Skin {
    pub fill: Color,
    pub border: Color,
    /// In DIPs.
    pub radius: f32,
    /// 0–1: how strongly the blur behind the panel shows.
    pub blur: f32,
    liquid: bool,
    light: bool,
}

impl Skin {
    /// `blur`: Windows blurs behind the panel. `opacity` and `tint` are 0–1.
    pub fn new(
        material: Material,
        base: Color,
        opacity: f32,
        tint: f32,
        p: &Palette,
        blur: bool,
    ) -> Self {
        let light = p.light();
        let edge = |dark: f32, bright: f32| {
            if light {
                Color::rgb(0, 0, 0).with_alpha(bright)
            } else {
                Color::rgb(0xFF, 0xFF, 0xFF).with_alpha(dark)
            }
        };
        let plain = |fill: Color, border: Color, radius: f32| Self {
            // Never fully clear, so the whole panel takes the mouse.
            fill: fill.with_alpha(fill.a.max(1.0 / 255.0)),
            border,
            radius,
            blur: 1.0,
            liquid: false,
            light,
        };
        let solid = |radius| {
            plain(
                base.with_alpha(opacity),
                p.popup_border
                    .with_alpha(p.popup_border.a * opacity.max(0.3)),
                radius,
            )
        };
        match material {
            Material::Solid => solid(8.0),
            Material::Glass if blur => plain(
                base.with_alpha(GLASS_MAX_TINT * tint),
                edge(0.22, 0.10),
                16.0,
            ),
            Material::Liquid if blur => Self {
                liquid: true,
                blur: LIQUID_BLUR,
                ..plain(
                    base.with_alpha(LIQUID_MAX_TINT * tint),
                    edge(0.3, 0.12),
                    20.0,
                )
            },
            Material::Acrylic if blur => {
                plain(base.with_alpha(0.8 * opacity), edge(0.09, 0.06), 8.0)
            }
            Material::Glass => solid(16.0),
            Material::Liquid => solid(20.0),
            Material::Acrylic => solid(8.0),
            Material::Clear => plain(base.with_alpha(0.28 * opacity), edge(0.10, 0.08), 12.0),
        }
    }

    /// `radius` and `scale` are in physical pixels.
    pub fn panel(
        &self,
        r: &Renderer,
        at: Rect,
        radius: f32,
        scale: f32,
        border: bool,
    ) -> Result<()> {
        r.fill_rounded(at, radius, self.fill)?;
        if self.liquid {
            return self.liquid_light(r, at, radius, scale.max(1.0));
        }
        if !border {
            return Ok(());
        }
        r.stroke_rounded(at, radius, scale.max(1.0), self.border)
    }

    fn liquid_light(&self, r: &Renderer, at: Rect, radius: f32, line: f32) -> Result<()> {
        let white = |a: f32| Color::rgb(0xFF, 0xFF, 0xFF).with_alpha(a);
        let black = |a: f32| Color::rgb(0, 0, 0).with_alpha(a);
        let k = if self.light { 1.6 } else { 1.0 };
        r.fill_rounded_vertical(
            at,
            radius,
            &[
                (0.0, white(0.14 * k)),
                (0.35, white(0.035 * k)),
                (0.7, white(0.0)),
                (1.0, white(0.06 * k)),
            ],
        )?;
        let rim = if self.light {
            [(0.0, white(0.95)), (0.5, black(0.06)), (1.0, black(0.14))]
        } else {
            [(0.0, white(0.55)), (0.5, white(0.12)), (1.0, white(0.3))]
        };
        r.stroke_rounded_vertical(at, radius, line, &rim)?;
        let inner = Rect {
            x: at.x + line,
            y: at.y + line,
            w: at.w - 2.0 * line,
            h: at.h - 2.0 * line,
        };
        if inner.w <= 0.0 || inner.h <= 0.0 {
            return Ok(());
        }
        r.stroke_rounded_vertical(
            inner,
            (radius - line).max(0.0),
            line,
            &[(0.0, white(0.2 * k)), (0.25, white(0.0)), (1.0, white(0.0))],
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_material_still_takes_the_mouse_when_fully_clear() {
        let config = pulsar_core::config::Config::default();
        let palette = Palette::new(false, &config, None, &crate::theme::Theme::windows());
        for material in [
            Material::Solid,
            Material::Glass,
            Material::Liquid,
            Material::Acrylic,
            Material::Clear,
        ] {
            for blur in [false, true] {
                let skin = Skin::new(material, palette.popup, 0.0, 0.0, &palette, blur);
                assert!(skin.fill.a > 0.0, "{material:?} blur {blur}");
            }
        }
    }

    #[test]
    fn liquid_glass_blurs_less_than_frosted_glass() {
        let config = pulsar_core::config::Config::default();
        let palette = Palette::new(false, &config, None, &crate::theme::Theme::windows());
        let skin = |m| Skin::new(m, palette.popup, 1.0, 0.0, &palette, true);
        assert!(skin(Material::Liquid).blur < skin(Material::Glass).blur);
        assert_eq!(skin(Material::Acrylic).blur, 1.0);
    }
}
