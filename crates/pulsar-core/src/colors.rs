//! Default item colours, shared by the overlay and the settings window.

use crate::layout::CellPart;
use crate::metric::ItemKind;

pub type Rgb = (u8, u8, u8);

pub fn default_rgb(kind: ItemKind, part: CellPart) -> Rgb {
    match (kind, part) {
        (ItemKind::Cpu, _) => (0x4C, 0xC2, 0xFF),
        (ItemKind::Ram, _) => (0xB1, 0x86, 0xF6),
        (ItemKind::Disk, _) => (0x6C, 0xCB, 0x5F),
        (ItemKind::Network, CellPart::Up) => (0xF7, 0x63, 0x0C),
        (ItemKind::Network, _) => (0xFF, 0xB9, 0x00),
        (ItemKind::Gpu, _) => (0xFF, 0x6F, 0xB5),
        (ItemKind::GpuTemp, _) => (0xFF, 0x8C, 0x42),
        (ItemKind::Ping, _) => (0x2E, 0xD5, 0xC4),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn items_have_distinct_default_colours() {
        let all: Vec<Rgb> = ItemKind::ALL
            .into_iter()
            .map(|k| default_rgb(k, CellPart::Main))
            .collect();
        for (i, a) in all.iter().enumerate() {
            assert!(!all[i + 1..].contains(a), "{a:?} repeats");
        }
    }

    #[test]
    fn network_up_differs_from_down() {
        assert_ne!(
            default_rgb(ItemKind::Network, CellPart::Down),
            default_rgb(ItemKind::Network, CellPart::Up)
        );
        assert_eq!(
            default_rgb(ItemKind::Network, CellPart::Main),
            default_rgb(ItemKind::Network, CellPart::Down)
        );
    }
}
