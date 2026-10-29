//! Readings that cross the user's limits. Each alert fires once, and fires
//! again only after its reading has clearly recovered.

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use crate::config::AlertsConfig;
use crate::metric::{MetricKey, Snapshot};

/// How long a temperature must stay over its limit, so a spike is ignored.
pub const HOT_FOR: Duration = Duration::from_secs(30);
/// How far a reading must come back before its alert can fire again.
const COOL_BY_C: f64 = 5.0;
const CHARGE_BY_PERCENT: f64 = 5.0;
const FREE_BY_PERCENT: f64 = 2.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Alert {
    CpuHot,
    GpuHot,
    BatteryLow,
    /// By drive letter.
    DriveLow(u8),
}

/// Where a reading stands against its limit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Level {
    Over,
    /// Back under the limit but not yet by the margin.
    Near,
    Clear,
}

fn high(value: f64, limit: f64, margin: f64) -> Level {
    if value >= limit {
        Level::Over
    } else if value > limit - margin {
        Level::Near
    } else {
        Level::Clear
    }
}

fn low(value: f64, limit: f64, margin: f64) -> Level {
    if value <= limit {
        Level::Over
    } else if value < limit + margin {
        Level::Near
    } else {
        Level::Clear
    }
}

/// Every alert that is on, where it stands now, and how long it must hold.
fn levels(config: &AlertsConfig, s: &Snapshot) -> Vec<(Alert, Level, Duration)> {
    let mut out = Vec::new();
    if config.cpu.on
        && let Some(t) = s.get(MetricKey::CpuTempC)
    {
        let level = high(t, f64::from(config.cpu.limit), COOL_BY_C);
        out.push((Alert::CpuHot, level, HOT_FOR));
    }
    if config.gpu.on
        && let Some(t) = s.get(MetricKey::GpuTempC)
    {
        let level = high(t, f64::from(config.gpu.limit), COOL_BY_C);
        out.push((Alert::GpuHot, level, HOT_FOR));
    }
    if config.battery.on
        && let Some(charge) = s.get(MetricKey::BatteryPercent)
    {
        let plugged = s.get(MetricKey::BatteryPluggedIn).is_some_and(|p| p > 0.0);
        let level = if plugged {
            Level::Clear
        } else {
            low(charge, f64::from(config.battery.limit), CHARGE_BY_PERCENT)
        };
        out.push((Alert::BatteryLow, level, Duration::ZERO));
    }
    if config.drives.on {
        for (&key, &used) in &s.metrics {
            if let MetricKey::DriveUsedPercent(letter) = key {
                let level = low(
                    100.0 - used,
                    f64::from(config.drives.limit),
                    FREE_BY_PERCENT,
                );
                out.push((Alert::DriveLow(letter), level, Duration::ZERO));
            }
        }
    }
    out
}

#[derive(Default)]
pub struct Alerts {
    /// When each alert's reading went over its limit.
    over_since: HashMap<Alert, Instant>,
    fired: HashSet<Alert>,
}

impl Alerts {
    /// The alerts to show now.
    pub fn check(&mut self, config: &AlertsConfig, s: &Snapshot, now: Instant) -> Vec<Alert> {
        let mut due = Vec::new();
        let levels = levels(config, s);
        self.over_since
            .retain(|alert, _| levels.iter().any(|(a, ..)| a == alert));
        for (alert, level, hold) in levels {
            if level != Level::Over {
                self.over_since.remove(&alert);
                if level == Level::Clear {
                    self.fired.remove(&alert);
                }
                continue;
            }
            let since = *self.over_since.entry(alert).or_insert(now);
            if now.duration_since(since) >= hold && self.fired.insert(alert) {
                due.push(alert);
            }
        }
        due
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Alarm;

    fn config() -> AlertsConfig {
        AlertsConfig {
            cpu: Alarm {
                on: true,
                limit: 90,
            },
            gpu: Alarm {
                on: false,
                limit: 85,
            },
            battery: Alarm {
                on: true,
                limit: 15,
            },
            drives: Alarm {
                on: true,
                limit: 10,
            },
        }
    }

    fn snapshot(pairs: &[(MetricKey, f64)]) -> Snapshot {
        let mut s = Snapshot::default();
        for &(k, v) in pairs {
            s.set(k, v);
        }
        s
    }

    #[test]
    fn a_hot_cpu_fires_once_after_holding_and_again_only_once_cooled() {
        let (mut a, c, t0) = (Alerts::default(), config(), Instant::now());
        let at = |s: u64| t0 + Duration::from_secs(s);
        let hot = snapshot(&[(MetricKey::CpuTempC, 93.0)]);
        assert!(a.check(&c, &hot, at(0)).is_empty());
        assert!(a.check(&c, &hot, at(29)).is_empty());
        assert_eq!(a.check(&c, &hot, at(30)), [Alert::CpuHot]);
        assert!(a.check(&c, &hot, at(90)).is_empty());
        // Just under the limit is not cool enough to fire again.
        let warm = snapshot(&[(MetricKey::CpuTempC, 88.0)]);
        assert!(a.check(&c, &warm, at(100)).is_empty());
        assert!(a.check(&c, &hot, at(101)).is_empty());
        assert!(a.check(&c, &hot, at(140)).is_empty());
        let cool = snapshot(&[(MetricKey::CpuTempC, 70.0)]);
        a.check(&c, &cool, at(150));
        a.check(&c, &hot, at(160));
        assert_eq!(a.check(&c, &hot, at(190)), [Alert::CpuHot]);
    }

    #[test]
    fn a_spike_shorter_than_the_hold_is_ignored() {
        let (mut a, c, t0) = (Alerts::default(), config(), Instant::now());
        let hot = snapshot(&[(MetricKey::CpuTempC, 95.0)]);
        let warm = snapshot(&[(MetricKey::CpuTempC, 89.0)]);
        a.check(&c, &hot, t0);
        a.check(&c, &warm, t0 + Duration::from_secs(20));
        assert!(a.check(&c, &hot, t0 + Duration::from_secs(35)).is_empty());
    }

    #[test]
    fn the_battery_alerts_only_on_battery_power() {
        let (mut a, c, now) = (Alerts::default(), config(), Instant::now());
        let plugged = snapshot(&[
            (MetricKey::BatteryPercent, 9.0),
            (MetricKey::BatteryPluggedIn, 1.0),
        ]);
        assert!(a.check(&c, &plugged, now).is_empty());
        let unplugged = snapshot(&[
            (MetricKey::BatteryPercent, 9.0),
            (MetricKey::BatteryPluggedIn, 0.0),
        ]);
        assert_eq!(a.check(&c, &unplugged, now), [Alert::BatteryLow]);
        assert!(a.check(&c, &unplugged, now).is_empty());
        a.check(&c, &plugged, now);
        assert_eq!(a.check(&c, &unplugged, now), [Alert::BatteryLow]);
    }

    #[test]
    fn each_drive_alerts_on_its_own() {
        let (mut a, c, now) = (Alerts::default(), config(), Instant::now());
        let s = snapshot(&[
            (MetricKey::DriveUsedPercent(b'C'), 95.0),
            (MetricKey::DriveUsedPercent(b'D'), 40.0),
        ]);
        assert_eq!(a.check(&c, &s, now), [Alert::DriveLow(b'C')]);
    }

    #[test]
    fn alerts_that_are_off_or_unread_stay_quiet() {
        let (mut a, now) = (Alerts::default(), Instant::now());
        let mut c = config();
        c.cpu.on = false;
        let s = snapshot(&[(MetricKey::CpuTempC, 105.0), (MetricKey::GpuTempC, 99.0)]);
        assert!(a.check(&c, &s, now).is_empty());
        assert!(a.check(&c, &s, now + HOT_FOR).is_empty());
    }
}
