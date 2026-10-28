//! What `pulsar.exe` streams to the details window over its standard input:
//! one JSON message a line, the history first and then every new snapshot.

use serde::{Deserialize, Serialize};

use crate::history::HistoryStore;
use crate::metric::{
    AdapterRate, FrameLabels, ItemKind, MetricKey, ProcessUsage, Snapshot, SourceId,
};

/// Passed to `pulsar-details.exe` when its standard input carries the feed.
pub const FEED_ARG: &str = "--feed";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Message {
    /// Missing samples travel as `None`: JSON has no NaN.
    History {
        capacity: usize,
        series: Vec<(MetricKey, Vec<Option<f64>>)>,
    },
    Snapshot(WireSnapshot),
    /// Bring the window to the front, on the page for the item clicked.
    Show {
        item: Option<ItemKind>,
    },
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct WireSnapshot {
    metrics: Vec<(MetricKey, f64)>,
    errors: Vec<(SourceId, String)>,
    adapters: Vec<AdapterRate>,
    processes: Vec<ProcessUsage>,
    #[serde(default)]
    frames: FrameLabels,
}

impl Message {
    pub fn history(store: &HistoryStore) -> Self {
        Message::History {
            capacity: store.capacity(),
            series: store
                .iter()
                .map(|(key, h)| (key, h.iter().map(|v| (!v.is_nan()).then_some(v)).collect()))
                .collect(),
        }
    }

    pub fn snapshot(snapshot: &Snapshot) -> Self {
        Message::Snapshot(WireSnapshot {
            metrics: snapshot
                .metrics
                .iter()
                .filter(|(_, v)| v.is_finite())
                .map(|(k, v)| (*k, *v))
                .collect(),
            errors: snapshot
                .errors
                .iter()
                .map(|(id, reason)| (*id, reason.clone()))
                .collect(),
            adapters: snapshot.adapters.clone(),
            processes: snapshot.processes.clone(),
            frames: snapshot.frames.clone(),
        })
    }

    /// The message as one line, newline included.
    pub fn encode(&self) -> String {
        let mut line = serde_json::to_string(self).expect("messages always serialize");
        line.push('\n');
        line
    }

    pub fn decode(line: &str) -> Option<Self> {
        serde_json::from_str(line.trim_end()).ok()
    }
}

impl From<WireSnapshot> for Snapshot {
    fn from(w: WireSnapshot) -> Self {
        Snapshot {
            metrics: w.metrics.into_iter().collect(),
            errors: w.errors.into_iter().collect(),
            adapters: w.adapters,
            processes: w.processes,
            frames: w.frames,
        }
    }
}

/// The store a `History` message describes.
pub fn history_store(capacity: usize, series: Vec<(MetricKey, Vec<Option<f64>>)>) -> HistoryStore {
    HistoryStore::from_series(
        capacity,
        series.into_iter().map(|(key, samples)| {
            (
                key,
                samples.into_iter().map(|v| v.unwrap_or(f64::NAN)).collect(),
            )
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metric::GpuEngineKind;

    fn round_trip(snapshot: &Snapshot) -> Snapshot {
        let line = Message::snapshot(snapshot).encode();
        match Message::decode(&line) {
            Some(Message::Snapshot(back)) => back.into(),
            other => panic!("not a snapshot: {other:?}"),
        }
    }

    #[test]
    fn a_snapshot_survives_the_trip_on_one_line() {
        let mut s = Snapshot::default();
        s.set(MetricKey::CpuTotal, 12.5);
        s.set(MetricKey::GpuEngine(GpuEngineKind::ThreeD), 40.0);
        s.set(MetricKey::VolumeFreeBytes(b'C'), 1e12);
        s.errors.insert(SourceId::Ping, "timed out".into());
        s.processes.push(ProcessUsage {
            name: "pulsar.exe".into(),
            cpu_percent: 0.1,
            private_bytes: 4e6,
            gpu_percent: 0.0,
            io_bps: 0.0,
        });
        let line = Message::snapshot(&s).encode();
        assert_eq!(line.matches('\n').count(), 1);
        assert!(line.ends_with('\n'));
        assert_eq!(round_trip(&s), s);
    }

    #[test]
    fn values_that_are_not_numbers_are_left_out() {
        let mut s = Snapshot::default();
        s.set(MetricKey::CpuTotal, f64::NAN);
        s.set(MetricKey::PingMs, 3.0);
        let back = round_trip(&s);
        assert_eq!(back.get(MetricKey::CpuTotal), None);
        assert_eq!(back.get(MetricKey::PingMs), Some(3.0));
    }

    #[test]
    fn history_gaps_survive_the_trip() {
        let mut store = HistoryStore::new(5);
        let mut a = Snapshot::default();
        a.set(MetricKey::CpuTotal, 5.0);
        store.record(&a);
        store.record(&Snapshot::default());
        let Some(Message::History { capacity, series }) =
            Message::decode(&Message::history(&store).encode())
        else {
            panic!("not a history");
        };
        let back = history_store(capacity, series);
        assert_eq!(back.capacity(), 5);
        let cpu: Vec<f64> = back.get(MetricKey::CpuTotal).unwrap().iter().collect();
        assert_eq!(cpu[0], 5.0);
        assert!(cpu[1].is_nan());
    }

    #[test]
    fn show_reads_back_and_garbage_is_ignored() {
        let show = Message::Show {
            item: Some(ItemKind::Gpu),
        };
        assert_eq!(Message::decode(&show.encode()), Some(show));
        assert_eq!(Message::decode("not json"), None);
    }
}
