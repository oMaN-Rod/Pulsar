//! Timing for showing and hiding overlays: a debounce so shell transitions
//! do not flicker them, and alpha steps for the fade.

use std::time::{Duration, Instant};

pub struct Debounce<T> {
    current: T,
    candidate: Option<(T, Instant)>,
}

impl<T: PartialEq> Debounce<T> {
    pub fn new(value: T) -> Self {
        Self {
            current: value,
            candidate: None,
        }
    }

    pub fn current(&self) -> &T {
        &self.current
    }

    /// Adopts `value` once it has been seen continuously for `delay`, or at
    /// once when `immediate`. Returns whether `current` changed.
    pub fn update(&mut self, value: T, now: Instant, delay: Duration, immediate: bool) -> bool {
        if value == self.current {
            self.candidate = None;
            return false;
        }
        if immediate {
            self.current = value;
            self.candidate = None;
            return true;
        }
        match &self.candidate {
            Some((v, since)) if *v == value => {
                if now.duration_since(*since) >= delay {
                    self.current = value;
                    self.candidate = None;
                    return true;
                }
                false
            }
            _ => {
                self.candidate = Some((value, now));
                false
            }
        }
    }
}

pub fn step(alpha: u8, target: u8, by: u8) -> u8 {
    if alpha < target {
        alpha.saturating_add(by).min(target)
    } else {
        alpha.saturating_sub(by).max(target)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entering_fullscreen_waits_leaving_is_immediate() {
        let t0 = Instant::now();
        let delay = Duration::from_millis(300);
        let mut d = Debounce::new(None::<i32>);
        assert!(!d.update(Some(1), t0, delay, false), "not yet");
        assert_eq!(*d.current(), None);
        assert!(!d.update(Some(1), t0 + Duration::from_millis(200), delay, false));
        assert!(d.update(Some(1), t0 + Duration::from_millis(310), delay, false));
        assert_eq!(*d.current(), Some(1));
        assert!(
            d.update(None, t0 + Duration::from_millis(320), delay, true),
            "leaving applies at once"
        );
        assert_eq!(*d.current(), None);
    }

    #[test]
    fn a_brief_flip_is_ignored() {
        let t0 = Instant::now();
        let delay = Duration::from_millis(300);
        let mut d = Debounce::new(None::<i32>);
        d.update(Some(1), t0, delay, false);
        assert!(
            !d.update(None, t0 + Duration::from_millis(100), delay, true),
            "back to current: no change"
        );
        assert!(
            !d.update(Some(1), t0 + Duration::from_millis(350), delay, false),
            "the timer restarted"
        );
    }

    #[test]
    fn fade_steps_towards_the_target() {
        assert_eq!(step(0, 255, 60), 60);
        assert_eq!(step(240, 255, 60), 255);
        assert_eq!(step(100, 0, 60), 40);
        assert_eq!(step(30, 0, 60), 0);
    }
}
