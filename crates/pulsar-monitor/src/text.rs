use pulsar_core::layout::TextMeasure;
use windows::Win32::Graphics::DirectWrite::{
    DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_FEATURE, DWRITE_FONT_FEATURE_TAG_TABULAR_FIGURES,
    DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_WEIGHT,
    DWRITE_FONT_WEIGHT_BOLD, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_TEXT_METRICS, DWRITE_TEXT_RANGE,
    DWRITE_WORD_WRAPPING_NO_WRAP, DWriteCreateFactory, IDWriteFactory, IDWriteFontCollection,
    IDWriteTextFormat, IDWriteTextLayout, IDWriteTypography,
};
use windows::core::{BOOL, HSTRING, Result, w};

const FAMILIES: [&str; 2] = ["Segoe UI Variable Text", "Segoe UI"];
const ICON_FAMILIES: [&str; 2] = ["Segoe Fluent Icons", "Segoe MDL2 Assets"];

pub struct Text {
    factory: IDWriteFactory,
    family: HSTRING,
    icon_family: HSTRING,
    weight: DWRITE_FONT_WEIGHT,
    tabular: IDWriteTypography,
}

impl Text {
    /// `family` is used when installed, else Segoe UI Variable, else Segoe UI.
    pub fn new(family: Option<&str>, bold: bool) -> Result<Self> {
        let factory: IDWriteFactory = unsafe { DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)? };
        let candidates = family.into_iter().chain(FAMILIES);
        let family =
            pick_family(&factory, candidates).unwrap_or_else(|| HSTRING::from(FAMILIES[1]));
        let icon_family =
            pick_family(&factory, ICON_FAMILIES).unwrap_or_else(|| HSTRING::from(ICON_FAMILIES[1]));
        let tabular = unsafe {
            let typography = factory.CreateTypography()?;
            typography.AddFontFeature(DWRITE_FONT_FEATURE {
                nameTag: DWRITE_FONT_FEATURE_TAG_TABULAR_FIGURES,
                parameter: 1,
            })?;
            typography
        };
        Ok(Self {
            factory,
            family,
            icon_family,
            weight: if bold {
                DWRITE_FONT_WEIGHT_BOLD
            } else {
                DWRITE_FONT_WEIGHT_NORMAL
            },
            tabular,
        })
    }

    #[cfg(test)]
    pub fn family(&self) -> String {
        self.family.to_string()
    }

    pub fn format(&self, font_px: f32) -> Result<IDWriteTextFormat> {
        self.create_format(&self.family, self.weight, font_px)
    }

    pub fn icon_format(&self, font_px: f32) -> Result<IDWriteTextFormat> {
        self.create_format(&self.icon_family, DWRITE_FONT_WEIGHT_NORMAL, font_px)
    }

    /// How far to move an icon down so it sits centred on a text line.
    pub fn icon_offset(&self, glyph: &str, font_px: f32) -> f32 {
        let icon_h = self
            .format_metrics(glyph, self.icon_format(font_px))
            .map_or(font_px, |m| m.height);
        ((self.line_height(font_px) - icon_h) / 2.0).round()
    }

    fn create_format(
        &self,
        family: &HSTRING,
        weight: DWRITE_FONT_WEIGHT,
        font_px: f32,
    ) -> Result<IDWriteTextFormat> {
        unsafe {
            let format = self.factory.CreateTextFormat(
                family,
                None,
                weight,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                font_px.max(1.0),
                w!("en-us"),
            )?;
            format.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP)?;
            Ok(format)
        }
    }

    /// A single-line layout with tabular figures, so changing digits never
    /// change the text width.
    pub fn layout(&self, text: &str, format: &IDWriteTextFormat) -> Result<IDWriteTextLayout> {
        let wide: Vec<u16> = text.encode_utf16().collect();
        unsafe {
            let layout = self
                .factory
                .CreateTextLayout(&wide, format, 10_000.0, 10_000.0)?;
            layout.SetTypography(
                &self.tabular,
                DWRITE_TEXT_RANGE {
                    startPosition: 0,
                    length: wide.len() as u32,
                },
            )?;
            Ok(layout)
        }
    }

    fn metrics(&self, text: &str, font_px: f32) -> Result<DWRITE_TEXT_METRICS> {
        self.format_metrics(text, self.format(font_px))
    }

    fn format_metrics(
        &self,
        text: &str,
        format: Result<IDWriteTextFormat>,
    ) -> Result<DWRITE_TEXT_METRICS> {
        let layout = self.layout(text, &format?)?;
        let mut metrics = DWRITE_TEXT_METRICS::default();
        unsafe { layout.GetMetrics(&mut metrics)? };
        Ok(metrics)
    }
}

impl TextMeasure for Text {
    fn width(&self, text: &str, font_px: f32) -> f32 {
        self.metrics(text, font_px)
            .map(|m| m.widthIncludingTrailingWhitespace.ceil())
            .unwrap_or(text.chars().count() as f32 * font_px * 0.6)
    }

    fn line_height(&self, font_px: f32) -> f32 {
        self.metrics("Ag", font_px)
            .map(|m| m.height.ceil())
            .unwrap_or(font_px * 1.33)
    }

    fn icon_width(&self, glyph: &str, font_px: f32) -> f32 {
        self.format_metrics(glyph, self.icon_format(font_px))
            .map(|m| m.widthIncludingTrailingWhitespace.ceil())
            .unwrap_or(font_px)
    }
}

fn pick_family<'a>(
    factory: &IDWriteFactory,
    candidates: impl IntoIterator<Item = &'a str>,
) -> Option<HSTRING> {
    let mut collection: Option<IDWriteFontCollection> = None;
    unsafe { factory.GetSystemFontCollection(&mut collection, false) }.ok()?;
    let collection = collection?;
    candidates.into_iter().map(HSTRING::from).find(|family| {
        let (mut index, mut exists) = (0u32, BOOL(0));
        unsafe { collection.FindFamilyName(family, &mut index, &mut exists) }.is_ok()
            && exists.as_bool()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uses_a_segoe_family() {
        let text = Text::new(None, false).unwrap();
        assert!(
            FAMILIES.contains(&text.family().as_str()),
            "{}",
            text.family()
        );
    }

    #[test]
    fn longer_strings_measure_wider() {
        let text = Text::new(None, false).unwrap();
        let short = text.width("9%", 12.0);
        let long = text.width("99.9 MB/s", 12.0);
        assert!(short > 0.0 && long > short * 2.0, "{short} vs {long}");
    }

    #[test]
    fn digits_are_tabular() {
        let text = Text::new(None, false).unwrap();
        assert_eq!(text.width("111", 12.0), text.width("888", 12.0));
    }

    #[test]
    fn widths_and_lines_scale_with_font_size() {
        let text = Text::new(None, false).unwrap();
        assert!(text.width("100%", 24.0) > text.width("100%", 12.0) * 1.8);
        let line = text.line_height(12.0);
        assert!((12.0..24.0).contains(&line), "{line}");
    }

    #[test]
    fn a_missing_family_falls_back_to_segoe() {
        let text = Text::new(Some("No Such Font Family 123"), false).unwrap();
        assert!(FAMILIES.contains(&text.family().as_str()));
    }

    #[test]
    fn an_installed_family_is_used() {
        let text = Text::new(Some("Arial"), true).unwrap();
        assert_eq!(text.family(), "Arial");
    }
}
