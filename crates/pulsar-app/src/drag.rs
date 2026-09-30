//! Dragging a floating overlay; movement below the system drag threshold is a click.

pub struct Drag {
    start_cursor: (i32, i32),
    start_pos: (i32, i32),
    moved: bool,
}

impl Drag {
    pub fn new(start_cursor: (i32, i32), start_pos: (i32, i32)) -> Self {
        Self {
            start_cursor,
            start_pos,
            moved: false,
        }
    }

    pub fn moved(&self) -> bool {
        self.moved
    }

    /// The window position for `cursor`, or `None` while still within `threshold`.
    pub fn to(&mut self, cursor: (i32, i32), threshold: i32) -> Option<(i32, i32)> {
        let (dx, dy) = (
            cursor.0 - self.start_cursor.0,
            cursor.1 - self.start_cursor.1,
        );
        if !self.moved && dx.abs() < threshold && dy.abs() < threshold {
            return None;
        }
        self.moved = true;
        Some((self.start_pos.0 + dx, self.start_pos.1 + dy))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_movements_are_clicks() {
        let mut d = Drag::new((100, 100), (500, 900));
        assert_eq!(d.to((102, 101), 4), None);
        assert!(!d.moved());
    }

    #[test]
    fn a_drag_moves_the_window_by_the_cursor_delta() {
        let mut d = Drag::new((100, 100), (500, 900));
        assert_eq!(d.to((130, 80), 4), Some((530, 880)));
        assert!(d.moved());
        assert_eq!(
            d.to((101, 100), 4),
            Some((501, 900)),
            "once moving, every move counts"
        );
    }
}
