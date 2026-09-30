use std::collections::{HashMap, VecDeque};

use crate::metric::{MetricKey, Snapshot};

/// Fixed-capacity sample buffer. Missing samples are stored as NaN so graphs
/// can show gaps; statistics ignore them.
#[derive(Clone, Debug)]
pub struct History {
    samples: VecDeque<f64>,
    capacity: usize,
}

impl History {
    pub fn new(capacity: usize) -> Self {
        let capacity = capacity.max(1);
        Self {
            samples: VecDeque::with_capacity(capacity),
            capacity,
        }
    }

    pub fn push(&mut self, value: f64) {
        if self.samples.len() == self.capacity {
            self.samples.pop_front();
        }
        self.samples.push_back(value);
    }

    pub fn len(&self) -> usize {
        self.samples.len()
    }

    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Oldest to newest.
    pub fn iter(&self) -> impl Iterator<Item = f64> + '_ {
        self.samples.iter().copied()
    }

    pub fn latest(&self) -> Option<f64> {
        self.samples.back().copied().filter(|v| !v.is_nan())
    }

    pub fn min(&self) -> Option<f64> {
        self.valid().reduce(f64::min)
    }

    pub fn max(&self) -> Option<f64> {
        self.valid().reduce(f64::max)
    }

    pub fn avg(&self) -> Option<f64> {
        let (sum, n) = self.valid().fold((0.0, 0usize), |(s, n), v| (s + v, n + 1));
        (n > 0).then(|| sum / n as f64)
    }

    fn valid(&self) -> impl Iterator<Item = f64> + '_ {
        self.iter().filter(|v| !v.is_nan())
    }
}

/// One `History` per tracked metric, all advanced together on each snapshot.
#[derive(Debug)]
pub struct HistoryStore {
    capacity: usize,
    series: HashMap<MetricKey, History>,
}

impl HistoryStore {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            series: HashMap::new(),
        }
    }

    pub fn record(&mut self, snapshot: &Snapshot) {
        let prior_len = self.series.values().map(History::len).max().unwrap_or(0);
        for (key, value) in &snapshot.metrics {
            let capacity = self.capacity;
            let history = self.series.entry(*key).or_insert_with(|| {
                let mut history = History::new(capacity);
                for _ in 0..prior_len {
                    history.push(f64::NAN);
                }
                history
            });
            history.push(*value);
        }
        for (key, history) in &mut self.series {
            if !snapshot.metrics.contains_key(key) {
                history.push(f64::NAN);
            }
        }
    }

    pub fn get(&self, key: MetricKey) -> Option<&History> {
        self.series.get(&key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_drops_oldest_when_full() {
        let mut h = History::new(3);
        for v in [1.0, 2.0, 3.0, 4.0] {
            h.push(v);
        }
        assert_eq!(h.iter().collect::<Vec<_>>(), vec![2.0, 3.0, 4.0]);
        assert_eq!(h.len(), 3);
    }

    #[test]
    fn zero_capacity_is_clamped_to_one() {
        let mut h = History::new(0);
        h.push(1.0);
        h.push(2.0);
        assert_eq!(h.iter().collect::<Vec<_>>(), vec![2.0]);
    }

    #[test]
    fn stats_ignore_nan() {
        let mut h = History::new(5);
        for v in [10.0, f64::NAN, 30.0, 20.0] {
            h.push(v);
        }
        assert_eq!(h.min(), Some(10.0));
        assert_eq!(h.max(), Some(30.0));
        assert_eq!(h.avg(), Some(20.0));
    }

    #[test]
    fn stats_are_none_when_all_missing() {
        let mut h = History::new(3);
        h.push(f64::NAN);
        assert_eq!(h.min(), None);
        assert_eq!(h.avg(), None);
        assert_eq!(h.latest(), None);
    }

    #[test]
    fn store_pads_missing_metrics_with_nan() {
        let mut store = HistoryStore::new(10);
        let mut a = Snapshot::default();
        a.set(MetricKey::CpuTotal, 5.0);
        store.record(&a);

        let mut b = Snapshot::default();
        b.set(MetricKey::PingMs, 12.0);
        store.record(&b);

        let cpu: Vec<f64> = store.get(MetricKey::CpuTotal).unwrap().iter().collect();
        assert_eq!(cpu[0], 5.0);
        assert!(cpu[1].is_nan());

        let ping: Vec<f64> = store.get(MetricKey::PingMs).unwrap().iter().collect();
        assert_eq!(
            ping.len(),
            2,
            "late metric is back-filled so series stay aligned"
        );
        assert!(ping[0].is_nan());
        assert_eq!(ping[1], 12.0);
    }

    #[test]
    fn metrics_new_in_same_snapshot_are_not_over_padded() {
        let mut store = HistoryStore::new(10);
        let mut a = Snapshot::default();
        a.set(MetricKey::CpuTotal, 1.0);
        a.set(MetricKey::MemUsedPercent, 2.0);
        a.set(MetricKey::NetDownBps, 3.0);
        store.record(&a);
        for key in [
            MetricKey::CpuTotal,
            MetricKey::MemUsedPercent,
            MetricKey::NetDownBps,
        ] {
            assert_eq!(store.get(key).unwrap().len(), 1);
        }
    }
}
