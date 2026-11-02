//! Where a dragged item lands among the items drawn on screen.

use crate::layout::Rect;
use crate::metric::ItemKind;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Slot {
    pub target: ItemKind,
    pub after: bool,
    /// The line showing where the item would go.
    pub marker: Rect,
}

fn distance(r: Rect, x: f32, y: f32) -> f32 {
    let dx = (r.x - x).max(x - r.right()).max(0.0);
    let dy = (r.y - y).max(y - (r.y + r.h)).max(0.0);
    dx.hypot(dy)
}

fn union(a: Rect, b: Rect) -> Rect {
    let (x, y) = (a.x.min(b.x), a.y.min(b.y));
    Rect {
        x,
        y,
        w: a.right().max(b.right()) - x,
        h: (a.y + a.h).max(b.y + b.h) - y,
    }
}

pub fn nearest(cells: &[(Rect, ItemKind)], x: f32, y: f32) -> Option<ItemKind> {
    cells
        .iter()
        .min_by(|a, b| distance(a.0, x, y).total_cmp(&distance(b.0, x, y)))
        .map(|&(_, kind)| kind)
}

/// The slot nearest `(x, y)` among `cells`; `None` over the dragged item itself.
///
/// Items sharing a column run top to bottom and the columns run left to right,
/// which is the order every surface draws them in.
pub fn slot(
    cells: &[(Rect, ItemKind)],
    dragged: ItemKind,
    x: f32,
    y: f32,
    thickness: f32,
) -> Option<Slot> {
    let target = nearest(cells, x, y)?;
    if target == dragged {
        return None;
    }
    let area = cells
        .iter()
        .filter(|(_, kind)| *kind == target)
        .map(|(r, _)| *r)
        .reduce(union)?;
    let stacked = cells
        .iter()
        .any(|(r, kind)| *kind != target && (r.x - area.x).abs() < 1.0);
    let (after, marker) = if stacked {
        let after = y > area.y + area.h / 2.0;
        let edge = if after { area.y + area.h } else { area.y };
        let marker = Rect {
            y: (edge - thickness / 2.0).max(0.0),
            h: thickness,
            ..area
        };
        (after, marker)
    } else {
        let after = x > area.x + area.w / 2.0;
        let edge = if after { area.right() } else { area.x };
        let marker = Rect {
            x: (edge - thickness / 2.0).max(0.0),
            w: thickness,
            ..area
        };
        (after, marker)
    };
    Some(Slot {
        target,
        after,
        marker,
    })
}

/// Moves the entry at `from` to just before or after the one at `target`.
pub fn reposition<T>(list: &mut Vec<T>, from: usize, target: usize, after: bool) {
    if from == target || from >= list.len() || target >= list.len() {
        return;
    }
    let item = list.remove(from);
    let target = if from < target { target - 1 } else { target };
    list.insert(target + usize::from(after), item);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect { x, y, w, h }
    }

    #[test]
    fn a_row_orders_by_the_side_of_the_target() {
        let cells = [
            (rect(0.0, 0.0, 20.0, 30.0), ItemKind::Cpu),
            (rect(22.0, 0.0, 20.0, 30.0), ItemKind::Ram),
        ];
        let s = slot(&cells, ItemKind::Cpu, 25.0, 10.0, 2.0).unwrap();
        assert_eq!((s.target, s.after), (ItemKind::Ram, false));
        assert_eq!(s.marker, rect(21.0, 0.0, 2.0, 30.0));
        let s = slot(&cells, ItemKind::Cpu, 40.0, 10.0, 2.0).unwrap();
        assert!(s.after);
        assert_eq!(s.marker, rect(41.0, 0.0, 2.0, 30.0));
    }

    #[test]
    fn a_stack_orders_by_height() {
        let cells = [
            (rect(0.0, 0.0, 20.0, 10.0), ItemKind::Cpu),
            (rect(0.0, 12.0, 20.0, 10.0), ItemKind::Ram),
        ];
        let s = slot(&cells, ItemKind::Cpu, 5.0, 20.0, 2.0).unwrap();
        assert_eq!((s.target, s.after), (ItemKind::Ram, true));
        assert_eq!(s.marker, rect(0.0, 21.0, 20.0, 2.0));
    }

    #[test]
    fn an_item_of_several_cells_is_one_target() {
        let cells = [
            (rect(0.0, 0.0, 20.0, 30.0), ItemKind::Cpu),
            (rect(22.0, 0.0, 20.0, 30.0), ItemKind::Network),
            (rect(44.0, 0.0, 20.0, 30.0), ItemKind::Network),
        ];
        let s = slot(&cells, ItemKind::Cpu, 30.0, 10.0, 2.0).unwrap();
        assert_eq!((s.target, s.after), (ItemKind::Network, false));
        let s = slot(&cells, ItemKind::Cpu, 50.0, 10.0, 2.0).unwrap();
        assert_eq!((s.target, s.after), (ItemKind::Network, true));
        assert_eq!(s.marker.x, 63.0);
    }

    #[test]
    fn the_cursor_off_the_items_picks_the_nearest_and_over_itself_none() {
        let cells = [
            (rect(0.0, 0.0, 20.0, 30.0), ItemKind::Cpu),
            (rect(22.0, 0.0, 20.0, 30.0), ItemKind::Ram),
        ];
        let s = slot(&cells, ItemKind::Cpu, 500.0, -40.0, 2.0).unwrap();
        assert_eq!((s.target, s.after), (ItemKind::Ram, true));
        assert_eq!(slot(&cells, ItemKind::Cpu, 5.0, 5.0, 2.0), None);
        assert_eq!(slot(&[], ItemKind::Cpu, 5.0, 5.0, 2.0), None);
    }

    #[test]
    fn repositioning_lands_beside_the_target_from_either_side() {
        let moved = |from, target, after| {
            let mut list = vec!['a', 'b', 'c', 'd'];
            reposition(&mut list, from, target, after);
            list.into_iter().collect::<String>()
        };
        assert_eq!(moved(0, 2, false), "bacd");
        assert_eq!(moved(0, 2, true), "bcad");
        assert_eq!(moved(3, 1, false), "adbc");
        assert_eq!(moved(3, 1, true), "abdc");
        assert_eq!(moved(1, 1, true), "abcd");
        assert_eq!(moved(9, 1, true), "abcd");
    }
}
