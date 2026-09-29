use perfbar_core::config::Position;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Rect32 {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl Rect32 {
    pub fn width(&self) -> i32 {
        self.right - self.left
    }

    pub fn height(&self) -> i32 {
        self.bottom - self.top
    }

    /// True when `self` covers all of `other`.
    pub fn covers(&self, other: &Rect32) -> bool {
        self.left <= other.left
            && self.top <= other.top
            && self.right >= other.right
            && self.bottom >= other.bottom
    }
}

#[derive(Clone, Copy, Debug)]
pub struct PlacementInput {
    pub taskbar: Rect32,
    pub tray: Option<Rect32>,
    pub monitor: Rect32,
    pub overlay_w: i32,
    pub overlay_h: i32,
    pub position: Position,
    pub offset_px: i32,
    pub fallback_margin_px: i32,
    pub dpi: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HideReason {
    Empty,
    AutoHidden,
    Vertical,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Placement {
    Hidden(HideReason),
    At { x: i32, y: i32 },
}

const GAP_DIP: i32 = 8;

fn scaled(dip: i32, dpi: u32) -> i32 {
    dip * dpi as i32 / 96
}

/// Right after Explorer starts, and while icons are being added, the tray
/// reports a zero or out-of-bounds rectangle; it must not be trusted then.
pub fn tray_is_laid_out(taskbar: &Rect32, tray: Option<&Rect32>) -> bool {
    tray.is_some_and(|t| t.width() > 0 && t.left > taskbar.left && t.right <= taskbar.right)
}

pub fn place(input: &PlacementInput) -> Placement {
    let tb = input.taskbar;
    if input.overlay_w <= 0 {
        return Placement::Hidden(HideReason::Empty);
    }
    if tb.height() > tb.width() {
        return Placement::Hidden(HideReason::Vertical);
    }
    let visible_h = tb.bottom.min(input.monitor.bottom) - tb.top.max(input.monitor.top);
    if visible_h < tb.height() / 2 {
        return Placement::Hidden(HideReason::AutoHidden);
    }

    let gap = scaled(GAP_DIP, input.dpi);
    let offset = scaled(input.offset_px, input.dpi);
    let x = match input.position {
        Position::Left => tb.left + gap + offset,
        Position::Right => {
            let anchor = if tray_is_laid_out(&tb, input.tray.as_ref()) {
                input.tray.unwrap().left
            } else {
                tb.right - scaled(input.fallback_margin_px, input.dpi)
            };
            anchor - gap - input.overlay_w - offset
        }
    };
    let x = x.clamp(tb.left, (tb.right - input.overlay_w).max(tb.left));
    let y = tb.top + (tb.height() - input.overlay_h) / 2;
    Placement::At { x, y }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MONITOR: Rect32 = Rect32 {
        left: 0,
        top: 0,
        right: 1920,
        bottom: 1080,
    };
    const TASKBAR: Rect32 = Rect32 {
        left: 0,
        top: 1032,
        right: 1920,
        bottom: 1080,
    };
    const TRAY: Rect32 = Rect32 {
        left: 1609,
        top: 1032,
        right: 1920,
        bottom: 1080,
    };

    fn input() -> PlacementInput {
        PlacementInput {
            taskbar: TASKBAR,
            tray: Some(TRAY),
            monitor: MONITOR,
            overlay_w: 200,
            overlay_h: 48,
            position: Position::Right,
            offset_px: 0,
            fallback_margin_px: 160,
            dpi: 96,
        }
    }

    #[test]
    fn right_sits_just_left_of_the_tray() {
        assert_eq!(
            place(&input()),
            Placement::At {
                x: 1609 - 8 - 200,
                y: 1032
            }
        );
    }

    #[test]
    fn unlaid_tray_uses_fallback_margin() {
        let zero = Rect32 {
            left: 0,
            top: 1032,
            right: 0,
            bottom: 1080,
        };
        for tray in [None, Some(zero), Some(Rect32 { left: 0, ..TRAY })] {
            let p = place(&PlacementInput { tray, ..input() });
            assert_eq!(
                p,
                Placement::At {
                    x: 1920 - 160 - 8 - 200,
                    y: 1032
                },
                "{tray:?}"
            );
        }
    }

    #[test]
    fn tray_validity() {
        assert!(tray_is_laid_out(&TASKBAR, Some(&TRAY)));
        assert!(!tray_is_laid_out(&TASKBAR, None));
        assert!(!tray_is_laid_out(
            &TASKBAR,
            Some(&Rect32 { left: 0, ..TRAY })
        ));
        assert!(!tray_is_laid_out(
            &TASKBAR,
            Some(&Rect32 {
                right: 1609,
                ..TRAY
            })
        ));
    }

    #[test]
    fn left_starts_after_gap_plus_offset() {
        let p = place(&PlacementInput {
            position: Position::Left,
            offset_px: 10,
            ..input()
        });
        assert_eq!(p, Placement::At { x: 18, y: 1032 });
    }

    #[test]
    fn right_offset_moves_further_left() {
        let p = place(&PlacementInput {
            offset_px: 50,
            ..input()
        });
        assert_eq!(
            p,
            Placement::At {
                x: 1609 - 8 - 200 - 50,
                y: 1032
            }
        );
    }

    #[test]
    fn scales_gap_offset_and_margin_with_dpi() {
        let p = place(&PlacementInput {
            tray: None,
            offset_px: 10,
            dpi: 144,
            ..input()
        });
        assert_eq!(
            p,
            Placement::At {
                x: 1920 - 240 - 12 - 200 - 15,
                y: 1032
            }
        );
    }

    #[test]
    fn vertically_centred() {
        let p = place(&PlacementInput {
            overlay_h: 40,
            ..input()
        });
        assert_eq!(p, Placement::At { x: 1401, y: 1036 });
    }

    #[test]
    fn hidden_when_auto_hidden_or_sliding_away() {
        let slid = Rect32 {
            top: 1060,
            bottom: 1108,
            ..TASKBAR
        };
        assert_eq!(
            place(&PlacementInput {
                taskbar: slid,
                ..input()
            }),
            Placement::Hidden(HideReason::AutoHidden)
        );
        let half = Rect32 {
            top: 1050,
            bottom: 1098,
            ..TASKBAR
        };
        assert!(matches!(
            place(&PlacementInput {
                taskbar: half,
                ..input()
            }),
            Placement::At { .. }
        ));
    }

    #[test]
    fn hidden_when_nothing_to_show_or_vertical() {
        assert_eq!(
            place(&PlacementInput {
                overlay_w: 0,
                ..input()
            }),
            Placement::Hidden(HideReason::Empty)
        );
        let vertical = Rect32 {
            left: 0,
            top: 0,
            right: 48,
            bottom: 1080,
        };
        assert_eq!(
            place(&PlacementInput {
                taskbar: vertical,
                ..input()
            }),
            Placement::Hidden(HideReason::Vertical)
        );
    }

    #[test]
    fn never_leaves_the_taskbar() {
        let p = place(&PlacementInput {
            offset_px: 5000,
            ..input()
        });
        assert_eq!(p, Placement::At { x: 0, y: 1032 });
        let p = place(&PlacementInput {
            position: Position::Left,
            offset_px: 5000,
            ..input()
        });
        assert_eq!(p, Placement::At { x: 1720, y: 1032 });
    }

    #[test]
    fn secondary_monitor_coordinates() {
        let monitor = Rect32 {
            left: 1920,
            top: 0,
            right: 3840,
            bottom: 1080,
        };
        let taskbar = Rect32 {
            left: 1920,
            top: 1032,
            right: 3840,
            bottom: 1080,
        };
        let p = place(&PlacementInput {
            monitor,
            taskbar,
            tray: None,
            ..input()
        });
        assert_eq!(
            p,
            Placement::At {
                x: 3840 - 160 - 8 - 200,
                y: 1032
            }
        );
    }

    #[test]
    fn covers() {
        assert!(MONITOR.covers(&MONITOR));
        assert!(!TASKBAR.covers(&MONITOR));
    }
}
