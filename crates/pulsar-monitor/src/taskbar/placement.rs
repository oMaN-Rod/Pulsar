use pulsar_core::config::Position;

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

    pub fn is_vertical(&self) -> bool {
        self.height() > self.width()
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
    NoRoom,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Placement {
    Hidden(HideReason),
    At { x: i32, y: i32 },
}

const GAP_DIP: i32 = 8;

pub fn scaled(dip: i32, dpi: u32) -> i32 {
    dip * dpi as i32 / 96
}

/// Right after Explorer starts, and while icons are being added, the tray
/// reports a zero or out-of-bounds rectangle; it must not be trusted then.
pub fn tray_is_laid_out(taskbar: &Rect32, tray: Option<&Rect32>) -> bool {
    tray.is_some_and(|t| {
        if taskbar.is_vertical() {
            t.height() > 0 && t.top > taskbar.top && t.bottom <= taskbar.bottom
        } else {
            t.width() > 0 && t.left > taskbar.left && t.right <= taskbar.right
        }
    })
}

pub fn place(input: &PlacementInput) -> Placement {
    let tb = input.taskbar;
    let off_monitor = tb.right <= input.monitor.left || tb.left >= input.monitor.right;
    if input.overlay_w <= 0 || tb.width() <= 0 || tb.height() <= 0 || off_monitor {
        return Placement::Hidden(HideReason::Empty);
    }
    let vertical = tb.is_vertical();
    let (visible, thickness) = if vertical {
        (
            tb.right.min(input.monitor.right) - tb.left.max(input.monitor.left),
            tb.width(),
        )
    } else {
        (
            tb.bottom.min(input.monitor.bottom) - tb.top.max(input.monitor.top),
            tb.height(),
        )
    };
    if visible < thickness / 2 {
        return Placement::Hidden(HideReason::AutoHidden);
    }

    let gap = scaled(GAP_DIP, input.dpi);
    let offset = scaled(input.offset_px, input.dpi);
    if vertical {
        let y = match input.position {
            Position::Left => tb.top + gap + offset,
            Position::Right | Position::Floating => {
                let anchor = if tray_is_laid_out(&tb, input.tray.as_ref()) {
                    input.tray.unwrap().top
                } else {
                    tb.bottom - scaled(input.fallback_margin_px, input.dpi)
                };
                anchor - gap - input.overlay_h - offset
            }
        };
        return Placement::At {
            x: tb.left + (tb.width() - input.overlay_w) / 2,
            y: y.clamp(tb.top, (tb.bottom - input.overlay_h).max(tb.top)),
        };
    }
    let x = match input.position {
        Position::Left => tb.left + gap + offset,
        Position::Right | Position::Floating => {
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

/// A free stretch of the taskbar, in screen coordinates along its axis.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Gap {
    pub start: i32,
    pub end: i32,
}

impl Gap {
    pub fn length(&self) -> i32 {
        self.end - self.start
    }

    fn holds(&self, start: i32, end: i32) -> bool {
        start >= self.start && end <= self.end
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Edge {
    Start,
    End,
}

/// Along the taskbar, then across it.
fn spans(rect: &Rect32, vertical: bool) -> ((i32, i32), (i32, i32)) {
    let (x, y) = ((rect.left, rect.right), (rect.top, rect.bottom));
    if vertical { (y, x) } else { (x, y) }
}

fn tray_span(taskbar: &Rect32, tray: Option<&Rect32>) -> Option<(i32, i32)> {
    let vertical = taskbar.is_vertical();
    let (bar, _) = spans(taskbar, vertical);
    let (span, _) = spans(tray?, vertical);
    (span.1 > span.0 && span.0 < bar.1 && span.1 > bar.0).then_some(span)
}

/// A mirrored taskbar has its tray at the low end.
fn tray_is_high(taskbar: &Rect32, tray: Option<&Rect32>) -> bool {
    let (bar, _) = spans(taskbar, taskbar.is_vertical());
    tray_span(taskbar, tray).is_none_or(|t| t.0 + t.1 >= bar.0 + bar.1)
}

/// The free stretches between the taskbar's buttons, up to the tray. Rectangles
/// that are empty, off the bar, or as long as the whole bar (a container, not
/// a button) say nothing about where the buttons are.
pub fn free_gaps(taskbar: &Rect32, tray: Option<&Rect32>, occupied: &[Rect32]) -> Vec<Gap> {
    let vertical = taskbar.is_vertical();
    let (bar, across) = spans(taskbar, vertical);
    if bar.1 <= bar.0 {
        return Vec::new();
    }
    let (mut low, mut high) = bar;
    if let Some(t) = tray_span(taskbar, tray) {
        if tray_is_high(taskbar, tray) {
            high = t.0.max(low);
        } else {
            low = t.1.min(high);
        }
    }
    let mut taken: Vec<(i32, i32)> = occupied
        .iter()
        .filter_map(|r| {
            let (along, over) = spans(r, vertical);
            let empty = along.1 <= along.0 || over.1 <= over.0;
            let off = over.0 >= across.1 || over.1 <= across.0;
            let container = along.0 <= bar.0 && along.1 >= bar.1;
            (!empty && !off && !container).then_some((along.0.max(low), along.1.min(high)))
        })
        .filter(|(a, b)| b > a)
        .collect();
    taken.sort_unstable();
    let mut gaps = Vec::new();
    let mut at = low;
    for (start, end) in taken {
        if start > at {
            gaps.push(Gap {
                start: at,
                end: start,
            });
        }
        at = at.max(end);
    }
    if high > at {
        gaps.push(Gap {
            start: at,
            end: high,
        });
    }
    gaps
}

/// Where to look for room, best first, each with the end to line up with.
/// Position sets the end of the bar to start from. On a centred taskbar a
/// gap on the far side of the buttons lines up with the outer end of the bar,
/// which stays put as buttons come and go.
pub fn gaps_in_order(
    gaps: &[Gap],
    input: &PlacementInput,
    centred: bool,
    other_side: bool,
) -> Vec<(Gap, Edge)> {
    let high = tray_is_high(&input.taskbar, input.tray.as_ref());
    let anchor = match (input.position, high) {
        (Position::Left, true) | (Position::Right | Position::Floating, false) => Edge::Start,
        _ => Edge::End,
    };
    let (bar, _) = spans(&input.taskbar, input.taskbar.is_vertical());
    let middle = bar.0 + bar.1;
    let on_anchor_side = |g: &Gap| match anchor {
        Edge::Start => g.start + g.end <= middle,
        Edge::End => g.start + g.end >= middle,
    };
    let mut ordered: Vec<Gap> = gaps.to_vec();
    if anchor == Edge::End {
        ordered.reverse();
    }
    if !centred {
        return ordered.into_iter().map(|g| (g, anchor)).collect();
    }
    let opposite = match anchor {
        Edge::Start => Edge::End,
        Edge::End => Edge::Start,
    };
    let (near, far): (Vec<Gap>, Vec<Gap>) = ordered.into_iter().partition(on_anchor_side);
    let far = far.into_iter().rev().filter(|_| other_side);
    near.into_iter()
        .map(|g| (g, anchor))
        .chain(far.map(|g| (g, opposite)))
        .collect()
}

/// The longest overlay these gaps can hold, keeping `GAP_DIP` at both ends and
/// honouring a positive offset.
pub fn room_in(candidates: &[(Gap, Edge)], input: &PlacementInput) -> i32 {
    let longest = candidates
        .iter()
        .map(|(g, _)| g.length())
        .max()
        .unwrap_or(0);
    (longest - spare(input)).max(0)
}

fn spare(input: &PlacementInput) -> i32 {
    2 * scaled(GAP_DIP, input.dpi) + scaled(input.offset_px, input.dpi).max(0)
}

fn overlay_length(input: &PlacementInput) -> i32 {
    if input.taskbar.is_vertical() {
        input.overlay_h
    } else {
        input.overlay_w
    }
}

/// The first gap that holds the overlay. One it already sits in is kept
/// until a better one has `margin` to spare, so it does not hop between them.
pub fn choose_gap(
    candidates: &[(Gap, Edge)],
    input: &PlacementInput,
    sitting_at: Option<i32>,
    margin: i32,
) -> Option<(Gap, Edge)> {
    let needed = overlay_length(input) + spare(input);
    let holds = |extra: i32| move |c: &&(Gap, Edge)| c.0.length() >= needed + extra;
    let current = sitting_at.and_then(|at| {
        candidates
            .iter()
            .filter(holds(0))
            .find(|(g, _)| g.start <= at && at < g.end)
    });
    let Some(current) = current else {
        return candidates.iter().find(holds(0)).copied();
    };
    let better = candidates
        .iter()
        .take_while(|c| *c != current)
        .find(holds(margin));
    better.or(Some(current)).copied()
}

/// The overlay at one end of `gap`, moved along by the offset but never out of it.
pub fn place_in_gap(input: &PlacementInput, gap: Gap, edge: Edge) -> Placement {
    let tb = input.taskbar;
    let length = overlay_length(input);
    let pad = scaled(GAP_DIP, input.dpi);
    let offset = scaled(input.offset_px, input.dpi);
    let along = match edge {
        Edge::Start => gap.start + pad + offset,
        Edge::End => gap.end - pad - length - offset,
    }
    .clamp(gap.start, (gap.end - length).max(gap.start));
    if tb.is_vertical() {
        Placement::At {
            x: tb.left + (tb.width() - input.overlay_w) / 2,
            y: along,
        }
    } else {
        Placement::At {
            x: along,
            y: tb.top + (tb.height() - input.overlay_h) / 2,
        }
    }
}

pub fn is_clear(placement: Placement, input: &PlacementInput, gaps: &[Gap]) -> bool {
    let Placement::At { x, y } = placement else {
        return false;
    };
    let start = if input.taskbar.is_vertical() { y } else { x };
    let end = start + overlay_length(input);
    gaps.iter().any(|g| g.holds(start, end))
}

pub fn start_along(taskbar: &Rect32, x: i32, y: i32) -> i32 {
    if taskbar.is_vertical() { y } else { x }
}

/// Today's `spot` while the overlay is there or has not been placed. Coming
/// back to it from another gap has to clear `margin`, so that a button coming
/// and going at the edge of the spot cannot move the overlay between sides.
pub fn settle(
    spot: Placement,
    input: &PlacementInput,
    gaps: &[Gap],
    order: &[(Gap, Edge)],
    sitting_at: Option<i32>,
    margin: i32,
) -> Placement {
    let Placement::At { x, y } = spot else {
        return spot;
    };
    let clear = is_clear(spot, input, gaps);
    if clear && sitting_at.is_none_or(|at| at == start_along(&input.taskbar, x, y)) {
        return spot;
    }
    match choose_gap(order, input, sitting_at, margin) {
        Some((gap, _)) if clear && is_clear(spot, input, &[gap]) => spot,
        Some((gap, edge)) => place_in_gap(input, gap, edge),
        None => spot,
    }
}

/// The work area of an auto-hide taskbar's monitor runs underneath it.
pub fn is_auto_hide(taskbar: &Rect32, work: &Rect32) -> bool {
    taskbar.left < work.right
        && work.left < taskbar.right
        && taskbar.top < work.bottom
        && work.top < taskbar.bottom
}

/// Where a child at screen position `at` goes in its parent's coordinates. In
/// a mirrored parent x runs leftwards from the right edge to the child's right edge.
pub fn child_position(
    (x, y): (i32, i32),
    width: i32,
    parent: Rect32,
    mirrored: bool,
) -> (i32, i32) {
    let client_x = if mirrored {
        parent.right - x - width
    } else {
        x - parent.left
    };
    (client_x, y - parent.top)
}

/// Floating placement: the saved top-left, pulled fully inside the monitor's
/// work area; without one, centred just above the taskbar.
pub fn place_floating(
    saved: Option<(i32, i32)>,
    (w, h): (i32, i32),
    work: Rect32,
    taskbar: Option<Rect32>,
    dpi: u32,
) -> (i32, i32) {
    let (x, y) = saved.unwrap_or_else(|| {
        let bottom = taskbar
            .filter(|t| !t.is_vertical())
            .map_or(work.bottom, |t| t.top.min(work.bottom));
        (
            (work.left + work.right - w) / 2,
            bottom - h - scaled(GAP_DIP, dpi),
        )
    });
    (
        x.clamp(work.left, (work.right - w).max(work.left)),
        y.clamp(work.top, (work.bottom - h).max(work.top)),
    )
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
    fn hidden_when_nothing_to_show() {
        assert_eq!(
            place(&PlacementInput {
                overlay_w: 0,
                ..input()
            }),
            Placement::Hidden(HideReason::Empty)
        );
    }

    const LEFT_BAR: Rect32 = Rect32 {
        left: 0,
        top: 0,
        right: 48,
        bottom: 1080,
    };

    fn vertical_input() -> PlacementInput {
        PlacementInput {
            taskbar: LEFT_BAR,
            tray: Some(Rect32 {
                top: 908,
                ..LEFT_BAR
            }),
            overlay_w: 48,
            overlay_h: 300,
            ..input()
        }
    }

    #[test]
    fn vertical_right_sits_just_above_the_tray() {
        assert_eq!(
            place(&vertical_input()),
            Placement::At {
                x: 0,
                y: 908 - 8 - 300
            }
        );
    }

    #[test]
    fn vertical_left_starts_at_the_top_after_gap_plus_offset() {
        let p = place(&PlacementInput {
            position: Position::Left,
            offset_px: 10,
            ..vertical_input()
        });
        assert_eq!(p, Placement::At { x: 0, y: 18 });
    }

    #[test]
    fn vertical_unlaid_tray_uses_fallback_margin() {
        for tray in [
            None,
            Some(Rect32 {
                top: 0,
                bottom: 0,
                ..LEFT_BAR
            }),
        ] {
            let p = place(&PlacementInput {
                tray,
                ..vertical_input()
            });
            assert_eq!(
                p,
                Placement::At {
                    x: 0,
                    y: 1080 - 160 - 8 - 300
                },
                "{tray:?}"
            );
        }
    }

    #[test]
    fn vertical_never_leaves_the_taskbar() {
        let p = place(&PlacementInput {
            offset_px: 5000,
            ..vertical_input()
        });
        assert_eq!(p, Placement::At { x: 0, y: 0 });
        let p = place(&PlacementInput {
            position: Position::Left,
            offset_px: 5000,
            ..vertical_input()
        });
        assert_eq!(p, Placement::At { x: 0, y: 780 });
    }

    #[test]
    fn vertical_hides_when_slid_off_the_side() {
        let slid = Rect32 {
            left: -46,
            right: 2,
            ..LEFT_BAR
        };
        assert_eq!(
            place(&PlacementInput {
                taskbar: slid,
                tray: None,
                ..vertical_input()
            }),
            Placement::Hidden(HideReason::AutoHidden)
        );
    }

    #[test]
    fn vertical_tray_validity() {
        let tray = Rect32 {
            top: 908,
            ..LEFT_BAR
        };
        assert!(tray_is_laid_out(&LEFT_BAR, Some(&tray)));
        assert!(!tray_is_laid_out(&LEFT_BAR, Some(&LEFT_BAR)));
    }

    #[test]
    fn floating_default_ignores_a_vertical_taskbar() {
        let work = Rect32 {
            left: 48,
            ..MONITOR
        };
        let (_, y) = place_floating(None, (200, 40), work, Some(LEFT_BAR), 96);
        assert_eq!(y, 1080 - 40 - 8);
    }

    #[test]
    fn hidden_for_a_degenerate_or_off_monitor_taskbar() {
        let zero = Rect32::default();
        assert_eq!(
            place(&PlacementInput {
                taskbar: zero,
                ..input()
            }),
            Placement::Hidden(HideReason::Empty)
        );
        let elsewhere = Rect32 {
            left: 5000,
            top: 1032,
            right: 6920,
            bottom: 1080,
        };
        assert_eq!(
            place(&PlacementInput {
                taskbar: elsewhere,
                ..input()
            }),
            Placement::Hidden(HideReason::Empty)
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
    fn child_position_is_relative_to_the_parent() {
        assert_eq!(child_position((1500, 1036), 200, TASKBAR, false), (1500, 4));
        let second = Rect32 {
            left: -1920,
            top: 1032,
            right: 0,
            bottom: 1080,
        };
        assert_eq!(child_position((-400, 1036), 200, second, false), (1520, 4));
    }

    #[test]
    fn child_position_mirrors_x_in_a_mirrored_parent() {
        assert_eq!(child_position((1500, 1036), 200, TASKBAR, true), (220, 4));
    }

    #[test]
    fn covers() {
        assert!(MONITOR.covers(&MONITOR));
        assert!(!TASKBAR.covers(&MONITOR));
    }

    const WORK: Rect32 = Rect32 {
        left: 0,
        top: 0,
        right: 1920,
        bottom: 1032,
    };

    #[test]
    fn floating_defaults_above_the_taskbar_centre() {
        let (x, y) = place_floating(None, (200, 40), WORK, Some(TASKBAR), 96);
        assert_eq!(x, (1920 - 200) / 2);
        assert_eq!(y, 1032 - 40 - 8);
    }

    #[test]
    fn a_saved_position_stays_on_screen_as_the_overlay_grows() {
        let saved = Some((300, 992));
        assert_eq!(place_floating(saved, (200, 40), WORK, None, 96), (300, 992));
        assert_eq!(place_floating(saved, (200, 80), WORK, None, 96), (300, 952));
    }

    #[test]
    fn floating_position_is_clamped_onto_the_monitor() {
        assert_eq!(
            place_floating(Some((1900, 1020)), (200, 40), WORK, None, 96),
            (1720, 992)
        );
        assert_eq!(
            place_floating(Some((-500, -20)), (200, 40), WORK, None, 96),
            (0, 0)
        );
        assert_eq!(
            place_floating(Some((300, 400)), (200, 40), WORK, None, 96),
            (300, 400)
        );
    }

    fn at(left: i32, right: i32) -> Rect32 {
        Rect32 {
            left,
            top: 1032,
            right,
            bottom: 1080,
        }
    }

    fn gap(start: i32, end: i32) -> Gap {
        Gap { start, end }
    }

    #[test]
    fn left_aligned_buttons_leave_one_gap_up_to_the_tray() {
        // Rectangles read from a real 25H2 taskbar at 100%.
        let buttons = [
            at(0, 55),
            at(55, 99),
            at(99, 143),
            at(143, 187),
            at(187, 231),
            at(231, 275),
            at(275, 319),
        ];
        assert_eq!(
            free_gaps(&TASKBAR, Some(&at(1663, 1920)), &buttons),
            [gap(319, 1663)]
        );
    }

    #[test]
    fn secondary_taskbars_read_the_same_at_any_coordinates() {
        // Rectangles read from real 25H2 secondary taskbars, whose tray is only a clock.
        let right_of_primary = Rect32 {
            left: 1920,
            top: 1032,
            right: 3840,
            bottom: 1080,
        };
        let clock = Rect32 {
            left: 3773,
            top: 1041,
            right: 3824,
            bottom: 1070,
        };
        let buttons = Rect32 {
            right: 2151,
            ..right_of_primary
        };
        assert_eq!(
            free_gaps(&right_of_primary, Some(&clock), &[buttons, clock]),
            [gap(2151, 3773)]
        );
        let left_of_primary = Rect32 {
            left: -1080,
            top: 1330,
            right: 0,
            bottom: 1378,
        };
        let clock = Rect32 {
            left: -67,
            top: 1339,
            right: -16,
            bottom: 1368,
        };
        let buttons = Rect32 {
            right: -981,
            ..left_of_primary
        };
        assert_eq!(
            free_gaps(&left_of_primary, Some(&clock), &[buttons, clock]),
            [gap(-981, -67)]
        );
    }

    #[test]
    fn tray_buttons_read_as_occupied_change_nothing() {
        let occupied = [at(0, 319), at(1663, 1701), at(1845, 1896)];
        assert_eq!(
            free_gaps(&TASKBAR, Some(&at(1663, 1920)), &occupied),
            [gap(319, 1663)]
        );
    }

    #[test]
    fn centred_buttons_leave_a_gap_each_side() {
        assert_eq!(
            free_gaps(&TASKBAR, Some(&TRAY), &[at(700, 1200)]),
            [gap(0, 700), gap(1200, 1609)]
        );
    }

    #[test]
    fn widgets_on_the_right_split_the_free_space() {
        assert_eq!(
            free_gaps(&TASKBAR, Some(&TRAY), &[at(0, 400), at(1500, 1600)]),
            [gap(400, 1500), gap(1600, 1609)]
        );
    }

    #[test]
    fn widgets_on_the_left_of_a_centred_bar() {
        assert_eq!(
            free_gaps(&TASKBAR, Some(&TRAY), &[at(0, 150), at(800, 1100)]),
            [gap(150, 800), gap(1100, 1609)]
        );
    }

    #[test]
    fn a_vertical_bar_uses_its_length() {
        let bar = Rect32 {
            left: 0,
            top: 0,
            right: 48,
            bottom: 1080,
        };
        let tray = Rect32 { top: 900, ..bar };
        let start = Rect32 {
            top: 0,
            bottom: 200,
            ..bar
        };
        assert_eq!(free_gaps(&bar, Some(&tray), &[start]), [gap(200, 900)]);
    }

    #[test]
    fn a_mirrored_bar_has_its_tray_at_the_low_end() {
        assert_eq!(
            free_gaps(&TASKBAR, Some(&at(0, 304)), &[at(1500, 1920)]),
            [gap(304, 1500)]
        );
    }

    #[test]
    fn zero_and_stale_rectangles_are_ignored() {
        let stale = [
            Rect32::default(),
            at(500, 500),
            Rect32 {
                left: 600,
                top: 0,
                right: 700,
                bottom: 48,
            },
            Rect32 {
                left: 800,
                top: 1032,
                right: 900,
                bottom: 1032,
            },
            at(900, 800),
            at(3000, 3100),
        ];
        assert_eq!(free_gaps(&TASKBAR, Some(&TRAY), &stale), [gap(0, 1609)]);
    }

    #[test]
    fn a_container_as_long_as_the_bar_is_not_a_button() {
        assert_eq!(
            free_gaps(&TASKBAR, Some(&TRAY), &[TASKBAR, at(0, 100)]),
            [gap(100, 1609)]
        );
    }

    #[test]
    fn buttons_past_the_tray_leave_no_gap_beyond_it() {
        let occupied = [at(0, 1500), at(1550, 1700), at(1750, 1800), at(1950, 2000)];
        assert_eq!(
            free_gaps(&TASKBAR, Some(&TRAY), &occupied),
            [gap(1500, 1550)]
        );
    }

    #[test]
    fn buttons_overlapping_each_other_merge() {
        assert_eq!(
            free_gaps(&TASKBAR, Some(&TRAY), &[at(10, 50), at(30, 90), at(20, 40)]),
            [gap(0, 10), gap(90, 1609)]
        );
    }

    #[test]
    fn a_full_bar_has_no_gaps() {
        assert_eq!(free_gaps(&TASKBAR, Some(&TRAY), &[at(0, 1700)]), []);
    }

    #[test]
    fn an_unlaid_tray_leaves_the_whole_bar() {
        assert_eq!(
            free_gaps(&TASKBAR, Some(&at(0, 0)), &[at(0, 100)]),
            [gap(100, 1920)]
        );
        assert_eq!(free_gaps(&TASKBAR, None, &[at(0, 100)]), [gap(100, 1920)]);
    }

    #[test]
    fn a_degenerate_bar_has_no_gaps() {
        assert_eq!(free_gaps(&Rect32::default(), None, &[]), []);
    }

    fn sides() -> [Gap; 2] {
        [gap(0, 700), gap(1200, 1609)]
    }

    #[test]
    fn coming_back_to_the_usual_spot_needs_the_margin() {
        let i = input();
        let needed = overlay_length(&i) + spare(&i);
        let left = gap(0, 700);
        let end = TRAY.left;
        let settled = |extra: i32| {
            let own = gap(end - needed - extra, end);
            let spot = place(&i);
            assert!(is_clear(spot, &i, &[own]), "the spot fits the near gap");
            let gaps = [left, own];
            let order = [(own, Edge::End), (left, Edge::Start)];
            let sitting = place_in_gap(&i, left, Edge::Start);
            let Placement::At { x, y } = sitting else {
                unreachable!()
            };
            let at = start_along(&i.taskbar, x, y);
            (
                settle(spot, &i, &gaps, &order, Some(at), 48),
                sitting,
                settle(spot, &i, &gaps, &order, None, 48),
                spot,
            )
        };
        let (near, sitting, first, spot) = settled(10);
        assert_eq!(near, sitting, "too close to come back");
        assert_eq!(first, spot, "first placement takes the spot");
        let (far, _, _, spot) = settled(60);
        assert_eq!(far, spot, "room to spare brings it back");
    }

    #[test]
    fn centred_right_tries_its_own_side_then_the_other() {
        let order = gaps_in_order(&sides(), &input(), true, true);
        assert_eq!(
            order,
            [(gap(1200, 1609), Edge::End), (gap(0, 700), Edge::Start)]
        );
    }

    #[test]
    fn centred_left_tries_its_own_side_then_the_other() {
        let order = gaps_in_order(
            &sides(),
            &PlacementInput {
                position: Position::Left,
                ..input()
            },
            true,
            true,
        );
        assert_eq!(
            order,
            [(gap(0, 700), Edge::Start), (gap(1200, 1609), Edge::End)]
        );
    }

    #[test]
    fn the_other_side_can_be_turned_off() {
        assert_eq!(
            gaps_in_order(&sides(), &input(), true, false),
            [(gap(1200, 1609), Edge::End)]
        );
    }

    #[test]
    fn left_aligned_looks_inward_from_the_tray() {
        let gaps = [gap(400, 1500), gap(1600, 1609)];
        assert_eq!(
            gaps_in_order(&gaps, &input(), false, true),
            [(gap(1600, 1609), Edge::End), (gap(400, 1500), Edge::End)]
        );
    }

    #[test]
    fn mirrored_right_starts_from_the_tray_at_the_low_end() {
        let gaps = [gap(304, 700), gap(1200, 1500)];
        let mirrored = PlacementInput {
            tray: Some(at(0, 304)),
            ..input()
        };
        assert_eq!(
            gaps_in_order(&gaps, &mirrored, false, true),
            [(gap(304, 700), Edge::Start), (gap(1200, 1500), Edge::Start)]
        );
    }

    #[test]
    fn room_keeps_a_gap_at_both_ends_and_honours_the_offset() {
        let order = [(gap(100, 1100), Edge::End)];
        assert_eq!(room_in(&order, &input()), 1000 - 16);
        let offset = PlacementInput {
            offset_px: 50,
            ..input()
        };
        assert_eq!(room_in(&order, &offset), 1000 - 16 - 50);
        let negative = PlacementInput {
            offset_px: -50,
            ..input()
        };
        assert_eq!(room_in(&order, &negative), 1000 - 16);
        let dpi = PlacementInput {
            dpi: 144,
            ..input()
        };
        assert_eq!(room_in(&order, &dpi), 1000 - 24);
        assert_eq!(room_in(&[], &input()), 0);
    }

    #[test]
    fn choose_skips_a_gap_too_small() {
        let order = [(gap(1600, 1609), Edge::End), (gap(400, 1500), Edge::End)];
        assert_eq!(
            choose_gap(&order, &input(), None, 48),
            Some((gap(400, 1500), Edge::End))
        );
        let huge = PlacementInput {
            overlay_w: 1100,
            ..input()
        };
        assert_eq!(choose_gap(&order, &huge, None, 48), None);
    }

    #[test]
    fn choose_stays_in_the_other_gap_until_home_has_the_margin() {
        let order = [(gap(1200, 1420), Edge::End), (gap(0, 700), Edge::Start)];
        let there = Some(100);
        assert_eq!(
            choose_gap(&order, &input(), there, 48),
            Some((gap(0, 700), Edge::Start))
        );
        let roomy = [(gap(1200, 1470), Edge::End), (gap(0, 700), Edge::Start)];
        assert_eq!(
            choose_gap(&roomy, &input(), there, 48),
            Some((gap(1200, 1470), Edge::End))
        );
    }

    #[test]
    fn choose_leaves_a_gap_that_no_longer_holds_it() {
        let order = [(gap(1200, 1420), Edge::End), (gap(0, 100), Edge::Start)];
        assert_eq!(
            choose_gap(&order, &input(), Some(50), 48),
            Some((gap(1200, 1420), Edge::End))
        );
    }

    #[test]
    fn places_at_either_end_of_a_gap() {
        assert_eq!(
            place_in_gap(&input(), gap(400, 1500), Edge::End),
            Placement::At {
                x: 1500 - 8 - 200,
                y: 1032
            }
        );
        assert_eq!(
            place_in_gap(&input(), gap(400, 1500), Edge::Start),
            Placement::At { x: 408, y: 1032 }
        );
    }

    #[test]
    fn an_offset_moves_within_the_gap_but_never_out() {
        let far = PlacementInput {
            offset_px: 5000,
            ..input()
        };
        assert_eq!(
            place_in_gap(&far, gap(400, 1500), Edge::End),
            Placement::At { x: 400, y: 1032 }
        );
        let back = PlacementInput {
            offset_px: -5000,
            ..input()
        };
        assert_eq!(
            place_in_gap(&back, gap(400, 1500), Edge::End),
            Placement::At { x: 1300, y: 1032 }
        );
    }

    #[test]
    fn places_along_a_vertical_bar() {
        assert_eq!(
            place_in_gap(&vertical_input(), gap(200, 900), Edge::End),
            Placement::At {
                x: 0,
                y: 900 - 8 - 300
            }
        );
    }

    #[test]
    fn todays_spot_is_clear_only_inside_one_gap() {
        let today = place(&input());
        assert!(is_clear(today, &input(), &[gap(0, 1609)]));
        assert!(is_clear(today, &input(), &[gap(1401, 1601)]));
        assert!(!is_clear(today, &input(), &[gap(0, 1500)]));
        assert!(!is_clear(today, &input(), &[gap(0, 1450), gap(1460, 1609)]));
        assert!(!is_clear(today, &input(), &[]));
        assert!(!is_clear(
            Placement::Hidden(HideReason::Empty),
            &input(),
            &[gap(0, 1609)]
        ));
    }

    #[test]
    fn auto_hide_shows_as_a_work_area_under_the_taskbar() {
        assert!(!is_auto_hide(&TASKBAR, &WORK));
        assert!(is_auto_hide(&TASKBAR, &MONITOR));
        let left = Rect32 {
            left: 48,
            ..MONITOR
        };
        assert!(!is_auto_hide(&LEFT_BAR, &left));
    }
}
