use pulsar_core::config::{Config, LabelStyle};
use pulsar_core::l10n;
use pulsar_core::layout::{CellPart, ItemNames, cell_label};
use pulsar_core::metric::ItemKind;

fn default_label(kind: ItemKind, part: CellPart, labels: LabelStyle, icons: bool) -> String {
    let number = |i: u8| u32::from(i) + 1;
    let name = match labels {
        LabelStyle::Full => kind.label(),
        LabelStyle::Short => kind.short_label(),
        LabelStyle::None => "",
    };
    match part {
        CellPart::Down | CellPart::Up if icons => String::new(),
        CellPart::Down => "↓".into(),
        CellPart::Up => "↑".into(),
        CellPart::AdapterDown(i) | CellPart::AdapterUp(i) if icons => number(i).to_string(),
        CellPart::AdapterDown(i) => format!("{}↓", number(i)),
        CellPart::AdapterUp(i) => format!("{}↑", number(i)),
        CellPart::AdapterPair(i) => number(i).to_string(),
        CellPart::DriveActive(d) | CellPart::DriveUsed(d) => format!("{}:", d as char),
        CellPart::Gpu(i) => format!("{name}{}", number(i)),
        CellPart::Main => name.to_string(),
    }
}

/// Alone in this binary, because the language belongs to the whole process.
#[test]
fn defaults_match_in_every_language_and_custom_names_never_translate() {
    let parts = [
        CellPart::Main,
        CellPart::Down,
        CellPart::Up,
        CellPart::DriveActive(b'C'),
        CellPart::DriveUsed(b'D'),
        CellPart::Gpu(1),
        CellPart::AdapterDown(0),
        CellPart::AdapterUp(1),
        CellPart::AdapterPair(1),
    ];
    let mut custom = Config::default();
    // "NET" has a translation; a custom name that happens to match must stay as typed.
    custom.item_mut(ItemKind::Cpu).unwrap().label = Some("NET".into());
    custom.network.down_label = Some("PWR".into());
    let custom = custom.sanitize();
    let mut resolved = Vec::new();
    for language in &l10n::LANGUAGES {
        l10n::set_language(Some(language.code));
        let names = ItemNames::resolve(&Config::default());
        for kind in ItemKind::ALL {
            for part in parts {
                for style in [LabelStyle::Full, LabelStyle::Short, LabelStyle::None] {
                    for icons in [false, true] {
                        assert_eq!(
                            cell_label(kind, part, style, icons, &names),
                            default_label(kind, part, style, icons),
                            "{} {kind:?} {part:?} {style:?} {icons}",
                            language.code
                        );
                    }
                }
            }
        }
        let renamed = ItemNames::resolve(&custom);
        assert_eq!(renamed.full(ItemKind::Cpu), "NET", "{}", language.code);
        assert_eq!(renamed.down(), Some("PWR"), "{}", language.code);
        resolved.push((language.code, names));
    }
    l10n::set_language(Some("en"));
    let (en, es) = (&resolved[0].1, &resolved[1].1);
    assert_eq!(en.full(ItemKind::Network), "NET");
    assert_eq!(es.full(ItemKind::Network), "RED");
    assert_ne!(
        en, es,
        "a layout measured in one language is stale in another"
    );
}
