//! Hover bookkeeping for overlay windows, keyed by window handle.

/// Overlays with a pending `TME_HOVER` request. Windows delivers one
/// `WM_MOUSEHOVER` per request, so a hover that opens nothing (or a click
/// that closes the popup) must disarm the overlay; the next mouse move then
/// arms it again.
#[derive(Debug, Default)]
pub struct HoverArming {
    armed: Vec<isize>,
}

impl HoverArming {
    /// Returns true when the caller should issue `TrackMouseEvent`.
    pub fn arm(&mut self, overlay: isize) -> bool {
        if self.armed.contains(&overlay) {
            return false;
        }
        self.armed.push(overlay);
        true
    }

    pub fn disarm(&mut self, overlay: isize) {
        self.armed.retain(|&h| h != overlay);
    }

    /// Forgets overlays that were destroyed, so a reused handle arms again.
    pub fn retain_live(&mut self, live: &[isize]) {
        self.armed.retain(|h| live.contains(h));
    }
}

/// True when the popup's overlay no longer exists or is hidden, so the popup
/// would never receive the mouse-leave that normally closes it.
pub fn popup_orphaned(owner: Option<isize>, shown: &[isize]) -> bool {
    owner.is_some_and(|o| !shown.contains(&o))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arms_once_until_disarmed() {
        let mut h = HoverArming::default();
        assert!(h.arm(1), "first move arms");
        assert!(!h.arm(1), "already armed");
        h.disarm(1);
        assert!(h.arm(1), "re-armed after a hover that opened nothing");
    }

    #[test]
    fn overlays_are_armed_independently() {
        let mut h = HoverArming::default();
        assert!(h.arm(1));
        assert!(h.arm(2));
        h.disarm(1);
        assert!(!h.arm(2));
    }

    #[test]
    fn destroyed_overlays_are_forgotten() {
        let mut h = HoverArming::default();
        h.arm(1);
        h.arm(2);
        h.retain_live(&[2]);
        assert!(h.arm(1), "a reused handle is armed again");
        assert!(!h.arm(2));
    }

    #[test]
    fn popup_is_orphaned_when_its_overlay_is_gone_or_hidden() {
        assert!(!popup_orphaned(None, &[]));
        assert!(!popup_orphaned(Some(1), &[1, 2]));
        assert!(popup_orphaned(Some(1), &[2]), "owner destroyed or hidden");
        assert!(popup_orphaned(Some(1), &[]));
    }
}
