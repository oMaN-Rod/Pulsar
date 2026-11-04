use std::borrow::Cow;

use super::CellPart;
use crate::config::{Config, NetworkShow};
use crate::metric::ItemKind;

#[derive(Clone, Debug, PartialEq, Eq)]
struct KindNames {
    full: Cow<'static, str>,
    short: Cow<'static, str>,
}

/// What the overlay calls each item: the user's names, over the defaults of
/// the language at the time. Settings and popups keep the item's own name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ItemNames {
    kinds: [KindNames; ItemKind::ALL.len()],
    down: Option<String>,
    up: Option<String>,
    /// The direction whose arrow the connection dot leads, when it is on.
    connection_dot: Option<CellPart>,
}

impl ItemNames {
    pub fn resolve(config: &Config) -> Self {
        let kinds = ItemKind::ALL.map(|kind| {
            let item = config.item(kind);
            let name = |custom: Option<&String>, default: &'static str| {
                custom.map_or(Cow::Borrowed(default), |c| Cow::Owned(c.clone()))
            };
            KindNames {
                full: name(item.and_then(|i| i.label.as_ref()), kind.label()),
                short: name(
                    item.and_then(|i| i.short_label.as_ref()),
                    kind.short_label(),
                ),
            }
        });
        Self {
            kinds,
            down: config.network.down_label.clone(),
            up: config.network.up_label.clone(),
            connection_dot: config
                .network
                .connection_dot
                .then_some(match config.network.show {
                    NetworkShow::Upload => CellPart::Up,
                    NetworkShow::Both if config.display.upload_on_top => CellPart::Up,
                    NetworkShow::Both | NetworkShow::Download => CellPart::Down,
                }),
        }
    }

    fn names(&self, kind: ItemKind) -> &KindNames {
        let at = ItemKind::ALL.iter().position(|&k| k == kind);
        &self.kinds[at.expect("every kind is in ALL")]
    }

    pub fn full(&self, kind: ItemKind) -> &str {
        &self.names(kind).full
    }

    pub fn short(&self, kind: ItemKind) -> &str {
        &self.names(kind).short
    }

    pub fn down(&self) -> Option<&str> {
        self.down.as_deref()
    }

    pub fn up(&self) -> Option<&str> {
        self.up.as_deref()
    }

    pub fn renames_directions(&self) -> bool {
        self.down.is_some() || self.up.is_some()
    }

    /// Whether `part` leaves room before its label for the connection dot,
    /// so the arrows of a pair stay in line.
    pub fn dot_room(&self, part: CellPart) -> bool {
        self.connection_dot.is_some() && matches!(part.base(), CellPart::Down | CellPart::Up)
    }

    /// Whether `part` draws the connection dot: once per adapter, on the
    /// first direction shown.
    pub fn draws_dot(&self, part: CellPart) -> bool {
        self.connection_dot == Some(part.base())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{DisplayMode, LabelStyle, NetworkShow, RemoveFrom};
    use crate::layout::fit::{self, Tier};
    use crate::layout::{
        CellPart, ItemSpec, LayoutInput, TextMeasure, TextSpacing, TileText, cell_label,
        compute_layout, inline_slots, item_specs,
    };

    struct Fixed;
    impl TextMeasure for Fixed {
        fn width(&self, text: &str, font_px: f32) -> f32 {
            text.chars().count() as f32 * font_px * 0.5
        }
        fn line_height(&self, font_px: f32) -> f32 {
            font_px * 1.25
        }
        fn icon_width(&self, _glyph: &str, font_px: f32) -> f32 {
            font_px
        }
    }

    fn input(mode: DisplayMode, names: &ItemNames) -> LayoutInput<'_> {
        LayoutInput {
            mode,
            taskbar_height_px: 48.0,
            fit_font: false,
            dpi: 96,
            font_size_pt: 9.0,
            labels: LabelStyle::Full,
            icons: false,
            graph_inline: false,
            vertical_labels: false,
            upload_on_top: false,
            vertical_bar_px: None,
            text_spacing: TextSpacing::default(),
            names,
        }
    }

    fn renamed() -> Config {
        let mut c = Config::default();
        let cpu = c.item_mut(ItemKind::Cpu).unwrap();
        cpu.label = Some("Proc".into());
        cpu.short_label = Some("Pr".into());
        let gpu = c.item_mut(ItemKind::Gpu).unwrap();
        gpu.label = Some("Video".into());
        gpu.short_label = Some("V".into());
        c.network.down_label = Some("In".into());
        c.network.up_label = Some("Out".into());
        c.sanitize()
    }

    fn spec(kind: ItemKind, parts: &[CellPart]) -> ItemSpec {
        ItemSpec {
            kind,
            parts: parts.to_vec(),
            tile: Default::default(),
            graphic: Default::default(),
        }
    }

    #[test]
    fn defaults_are_todays_labels() {
        let names = ItemNames::resolve(&Config::default());
        for kind in ItemKind::ALL {
            assert_eq!(names.full(kind), kind.label());
            assert_eq!(names.short(kind), kind.short_label());
        }
        assert!(!names.renames_directions());
    }

    #[test]
    fn a_custom_name_follows_the_label_style() {
        let names = ItemNames::resolve(&renamed());
        let label = |kind, style| cell_label(kind, CellPart::Main, style, false, &names);
        assert_eq!(label(ItemKind::Cpu, LabelStyle::Full), "Proc");
        assert_eq!(label(ItemKind::Cpu, LabelStyle::Short), "Pr");
        assert_eq!(label(ItemKind::Cpu, LabelStyle::None), "");
        assert_eq!(label(ItemKind::Ram, LabelStyle::Full), "RAM");
        assert_eq!(label(ItemKind::Ram, LabelStyle::Short), "M");
    }

    #[test]
    fn one_custom_name_leaves_the_other_style_at_its_default() {
        let mut c = Config::default();
        c.item_mut(ItemKind::Ram).unwrap().label = Some("Mem".into());
        c.item_mut(ItemKind::Cpu).unwrap().short_label = Some("Pr".into());
        let names = ItemNames::resolve(&c.sanitize());
        assert_eq!(names.short(ItemKind::Ram), "M");
        assert_eq!(names.full(ItemKind::Cpu), "CPU");
    }

    #[test]
    fn each_gpu_numbers_from_the_custom_name() {
        let names = ItemNames::resolve(&renamed());
        let label = |style| cell_label(ItemKind::Gpu, CellPart::Gpu(1), style, false, &names);
        assert_eq!(label(LabelStyle::Full), "Video2");
        assert_eq!(label(LabelStyle::Short), "V2");
        assert_eq!(label(LabelStyle::None), "2");
    }

    #[test]
    fn each_adapter_numbers_from_the_custom_direction() {
        let names = ItemNames::resolve(&renamed());
        let label = |part, style, icons| cell_label(ItemKind::Network, part, style, icons, &names);
        for style in [LabelStyle::Full, LabelStyle::Short] {
            for icons in [false, true] {
                assert_eq!(label(CellPart::AdapterDown(0), style, icons), "In1");
                assert_eq!(label(CellPart::AdapterUp(1), style, icons), "Out2");
            }
        }
        assert_eq!(label(CellPart::AdapterDown(0), LabelStyle::None, true), "1");
        assert_eq!(label(CellPart::AdapterUp(1), LabelStyle::None, false), "2↑");
    }

    #[test]
    fn direction_names_replace_arrows_and_icons_unless_labels_are_off() {
        let names = ItemNames::resolve(&renamed());
        let label = |part, style, icons| cell_label(ItemKind::Network, part, style, icons, &names);
        for style in [LabelStyle::Full, LabelStyle::Short] {
            for icons in [false, true] {
                assert_eq!(label(CellPart::Down, style, icons), "In");
                assert_eq!(label(CellPart::Up, style, icons), "Out");
            }
        }
        assert_eq!(label(CellPart::Down, LabelStyle::None, true), "");
        assert_eq!(label(CellPart::Up, LabelStyle::None, false), "↑");
    }

    #[test]
    fn drive_letters_stay_whatever_the_disk_is_called() {
        let mut c = Config::default();
        c.item_mut(ItemKind::Disk).unwrap().label = Some("Store".into());
        let names = ItemNames::resolve(&c.sanitize());
        let part = CellPart::DriveActive(b'C');
        let label = |part, style| cell_label(ItemKind::Disk, part, style, false, &names);
        assert_eq!(label(part, LabelStyle::Full), "C:");
        assert_eq!(label(CellPart::Main, LabelStyle::Full), "Store");
    }

    #[test]
    fn width_follows_the_custom_text_in_every_layout() {
        let defaults = ItemNames::resolve(&Config::default());
        let mut c = Config::default();
        c.item_mut(ItemKind::Cpu).unwrap().label = Some("Processor123".into());
        let long = ItemNames::resolve(&c.sanitize());
        let items = [spec(ItemKind::Cpu, &[CellPart::Main])];
        for mode in [DisplayMode::Text, DisplayMode::Graph] {
            let width = |names| compute_layout(&items, input(mode, names), &Fixed).width;
            assert!(width(&long) > width(&defaults), "{mode:?}");
        }
        let inline = |names| {
            let i = LayoutInput {
                graph_inline: true,
                ..input(DisplayMode::Graph, names)
            };
            compute_layout(&items, i, &Fixed).width
        };
        assert!(inline(&long) > inline(&defaults));
        let bar = |names| {
            let i = LayoutInput {
                vertical_bar_px: Some(200.0),
                ..input(DisplayMode::Text, names)
            };
            compute_layout(&items, i, &Fixed)
        };
        assert_eq!(bar(&long).font_px, 12.0);
        assert!(bar(&long).cells[0].value_x > bar(&defaults).cells[0].value_x);
        let narrow = LayoutInput {
            vertical_bar_px: Some(48.0),
            ..input(DisplayMode::Text, &long)
        };
        let shrunk = compute_layout(&items, narrow, &Fixed);
        assert!(shrunk.font_px < 12.0);
        assert!(Fixed.width("Processor123", shrunk.font_px) <= 44.0 + 0.01);
    }

    #[test]
    fn a_network_tile_fits_the_longer_of_its_two_names() {
        let mut c = Config::default();
        c.network.up_label = Some("Upstream".into());
        let up = ItemNames::resolve(&c.sanitize());
        let mut c = Config::default();
        c.network.down_label = Some("Upstream".into());
        let down = ItemNames::resolve(&c.sanitize());
        let items = [spec(ItemKind::Network, &[CellPart::Main])];
        let width = |names| compute_layout(&items, input(DisplayMode::Graph, names), &Fixed).width;
        assert_eq!(width(&up), width(&down));
        let defaults = ItemNames::resolve(&Config::default());
        assert!(width(&up) > width(&defaults));
    }

    #[test]
    fn inline_slots_measure_custom_names() {
        let names = ItemNames::resolve(&renamed());
        let text = TileText {
            labels: LabelStyle::Full,
            icons: false,
            vertical_labels: false,
            upload_on_top: false,
            names: &names,
        };
        let (slots, _) = inline_slots(ItemKind::Network, CellPart::Main, text, 10.0, 1.0, &Fixed);
        // "In" is 10px wide, then a 4px gap.
        assert_eq!(slots[0].value_x, 14.0);
        assert_eq!(slots[1].value_x - slots[1].label_x, 19.0);
    }

    #[test]
    fn compact_uses_the_custom_short_name() {
        let mut c = Config::default();
        c.display.mode = DisplayMode::Text;
        let cpu = c.item_mut(ItemKind::Cpu).unwrap();
        cpu.label = Some("Processor".into());
        cpu.short_label = Some("Pr".into());
        let names = ItemNames::resolve(&c.clone().sanitize());
        let items = [
            spec(ItemKind::Cpu, &[CellPart::Main]),
            spec(ItemKind::Ram, &[CellPart::Main]),
        ];
        let i = input(DisplayMode::Text, &names);
        let rungs = fit::ladder(&items, i, RemoveFrom::Right, &Fixed);
        assert_eq!(rungs[1].tier, Tier::Compact);
        assert_eq!(rungs[1].style.labels, LabelStyle::Short);
        let cell = &rungs[1].layout.cells[0];
        let label_w = Fixed.width("Pr", rungs[1].layout.font_px);
        assert!((cell.value_x - cell.rect.x - label_w - 2.0).abs() < 0.01);
        for pair in rungs.windows(2) {
            assert!(pair[1].length() < pair[0].length());
        }
    }

    #[test]
    fn a_short_name_longer_than_the_full_one_never_widens_a_rung() {
        let mut c = Config::default();
        let cpu = c.item_mut(ItemKind::Cpu).unwrap();
        cpu.label = Some("C".into());
        cpu.short_label = Some("Processor123".into());
        let names = ItemNames::resolve(&c.sanitize());
        let items = [
            spec(ItemKind::Cpu, &[CellPart::Main]),
            spec(ItemKind::Ram, &[CellPart::Main]),
        ];
        let i = input(DisplayMode::Text, &names);
        let rungs = fit::ladder(&items, i, RemoveFrom::Right, &Fixed);
        for pair in rungs.windows(2) {
            assert!(pair[1].length() < pair[0].length());
        }
    }

    #[test]
    fn every_mode_keeps_custom_names_inside_their_cells() {
        let c = {
            let mut c = renamed();
            c.network.adapters = vec!["eth".into(), "wifi".into()];
            c.sanitize()
        };
        let names = ItemNames::resolve(&c);
        for show in [
            NetworkShow::Both,
            NetworkShow::Download,
            NetworkShow::Upload,
        ] {
            let mut c = c.clone();
            c.network.show = show;
            for (mode, inline, vertical) in [
                (DisplayMode::Text, false, None),
                (DisplayMode::Text, false, Some(200.0)),
                (DisplayMode::Graph, false, None),
                (DisplayMode::Graph, true, None),
                (DisplayMode::Graph, false, Some(200.0)),
            ] {
                c.display.mode = mode;
                let i = LayoutInput {
                    graph_inline: inline,
                    vertical_bar_px: vertical,
                    ..input(mode, &names)
                };
                let l = compute_layout(&item_specs(&c, &Default::default()), i, &Fixed);
                assert!(!l.cells.is_empty());
                for pair in l.cells.windows(2) {
                    let (a, b) = (&pair[0].rect, &pair[1].rect);
                    let apart = a.right() <= b.x
                        || b.right() <= a.x
                        || a.y + a.h <= b.y
                        || b.y + b.h <= a.y;
                    assert!(apart, "{mode:?} {inline} {vertical:?}");
                }
            }
        }
    }

    #[test]
    fn the_connection_dot_leads_the_first_direction_shown_and_aligns_the_other() {
        let mut c = Config::default();
        assert!(!ItemNames::resolve(&c).dot_room(CellPart::Down));
        c.network.connection_dot = true;
        for (show, upload_on_top, lead) in [
            (NetworkShow::Both, false, CellPart::Down),
            (NetworkShow::Both, true, CellPart::Up),
            (NetworkShow::Download, true, CellPart::Down),
            (NetworkShow::Upload, false, CellPart::Up),
        ] {
            c.network.show = show;
            c.display.upload_on_top = upload_on_top;
            let names = ItemNames::resolve(&c);
            for part in [CellPart::Down, CellPart::Up, CellPart::AdapterDown(1)] {
                assert!(names.dot_room(part));
                assert_eq!(
                    names.draws_dot(part),
                    part.base() == lead,
                    "{show:?} {part:?}"
                );
            }
            assert!(!names.dot_room(CellPart::Main), "a pair measures each line");
        }

        let names = ItemNames::resolve(&c);
        let width = |names: &ItemNames, part| {
            crate::layout::label_width(
                ItemKind::Network,
                part,
                LabelStyle::Full,
                false,
                names,
                12.0,
                &Fixed,
            )
        };
        let plain = ItemNames::resolve(&Config::default());
        assert!(width(&names, CellPart::Down) > width(&plain, CellPart::Down));
        assert_eq!(width(&names, CellPart::Down), width(&names, CellPart::Up));
        assert_eq!(
            width(&names, CellPart::Main),
            width(&plain, CellPart::Main),
            "other cells keep their width"
        );
    }
}
