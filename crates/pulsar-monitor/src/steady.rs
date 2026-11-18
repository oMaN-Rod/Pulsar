//! Values laid out so a changing reading never moves the text around it.

use pulsar_core::format::{spans, widest_like};
use pulsar_core::layout::TextMeasure;
use windows::Win32::Graphics::DirectWrite::IDWriteTextFormat;
use windows::core::Result;

use crate::render::Renderer;
use crate::text::Text;
use crate::theme::Color;

/// `text` placed piece by piece: each reading keeps the room of its widest
/// form, so the pieces after it and the whole width hold still.
pub struct Steady<'a> {
    pieces: Vec<(&'a str, f32)>,
    pub width: f32,
}

impl<'a> Steady<'a> {
    /// `widest` is `text` at its widest when the caller knows it, such as
    /// a reading that is still a placeholder; otherwise it is worked out.
    pub fn new(t: &dyn TextMeasure, text: &'a str, widest: Option<&str>, px: f32) -> Self {
        let guessed;
        let widest = match widest {
            Some(w) => w,
            None => {
                guessed = widest_like(text);
                &guessed
            }
        };
        let live = spans(text);
        let wide = spans(widest);
        let paired =
            live.len() == wide.len() && live.iter().zip(&wide).all(|(a, b)| a.reading == b.reading);
        if !paired {
            return Self {
                pieces: vec![(text, 0.0)],
                width: t.width(text, px).max(t.width(widest, px)),
            };
        }
        let mut x = 0.0;
        let pieces = live
            .iter()
            .zip(&wide)
            .map(|(a, b)| {
                let w = t.width(a.text, px);
                let room = w.max(t.width(b.text, px));
                let at = if a.right || b.right { x + room - w } else { x };
                x += room;
                (a.text, at)
            })
            .collect();
        Self { pieces, width: x }
    }

    pub fn draw(
        &self,
        r: &Renderer,
        t: &Text,
        format: &IDWriteTextFormat,
        x: f32,
        y: f32,
        color: Color,
    ) -> Result<()> {
        for &(piece, at) in &self.pieces {
            r.text_at(t, format, piece, x + at, y, color)?;
        }
        Ok(())
    }
}

/// The room `text` keeps at its widest.
pub fn steady_width(t: &dyn TextMeasure, text: &str, widest: Option<&str>, px: f32) -> f32 {
    Steady::new(t, text, widest, px).width
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixed;

    impl TextMeasure for Fixed {
        fn width(&self, text: &str, font_px: f32) -> f32 {
            text.chars().count() as f32 * font_px / 2.0
        }
        fn line_height(&self, font_px: f32) -> f32 {
            font_px
        }
        fn icon_width(&self, _glyph: &str, font_px: f32) -> f32 {
            font_px
        }
    }

    #[test]
    fn a_changing_reading_keeps_the_width_and_the_text_after_it_still() {
        let at = |text: &str| {
            let s = Steady::new(&Fixed, text, None, 2.0);
            (s.width, s.pieces.last().unwrap().1)
        };
        let idle = at("↓ 0.0 KB/s  ↑ 0.5 KB/s");
        let busy = at("↓ 120 MB/s  ↑ 20 KB/s");
        assert_eq!(idle.0, busy.0);
        let up = |text: &str| {
            let s = Steady::new(&Fixed, text, None, 2.0);
            s.pieces.iter().find(|p| p.0.contains('↑')).unwrap().1
        };
        assert_eq!(up("↓ 0.0 KB/s  ↑ 1 KB/s"), up("↓ 120 MB/s  ↑ 1 KB/s"));
        assert_eq!(at("0 fps").0, at("60 fps").0);
    }

    #[test]
    fn a_placeholder_takes_the_room_of_the_known_widest() {
        let widest = "↓ 999 MB/s  ↑ 999 MB/s";
        let waiting = Steady::new(&Fixed, "↓ —  ↑ —", Some(widest), 2.0);
        let busy = Steady::new(&Fixed, "↓ 9.0 KB/s  ↑ 12 KB/s", Some(widest), 2.0);
        assert_eq!(waiting.width, busy.width);
        assert_eq!(waiting.pieces[2].1, busy.pieces[2].1);
    }
}
