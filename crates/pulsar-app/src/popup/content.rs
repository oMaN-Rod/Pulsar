//! What a hover popup shows for an item, independent of how it is drawn.

use pulsar_core::format::format_value;
use pulsar_core::history::HistoryStore;
use pulsar_core::layout::CellPart;
use pulsar_core::metric::{GpuEngineKind, ItemKind, MetricKey, Snapshot, SourceId, Unit};
use pulsar_core::sources::{ProcessSort, top_by};

use crate::display::cell_value;

const TOP_PROCESSES: usize = 5;

#[derive(Clone, Debug, PartialEq)]
pub enum Block {
    Rows {
        heading: Option<&'static str>,
        rows: Vec<(String, String)>,
    },
    /// One bar per value, each 0–100 (per-core CPU).
    Bars {
        heading: &'static str,
        values: Vec<f64>,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct PopupContent {
    pub kind: ItemKind,
    pub title: &'static str,
    pub value: String,
    /// Graph series, drawn in order; the first is filled.
    pub series: Vec<CellPart>,
    pub error: Option<String>,
    pub blocks: Vec<Block>,
}

/// Whether the popup lists processes, so the sampler must collect them.
pub fn needs_processes(kind: ItemKind) -> bool {
    matches!(kind, ItemKind::Cpu | ItemKind::Ram | ItemKind::Gpu)
}

fn fmt(snapshot: &Snapshot, key: MetricKey) -> String {
    format_value(snapshot.get(key).unwrap_or(f64::NAN), key.unit())
}

fn of(snapshot: &Snapshot, used: MetricKey, total: MetricKey) -> String {
    format!("{} of {}", fmt(snapshot, used), fmt(snapshot, total))
}

fn processes(snapshot: &Snapshot, sort: ProcessSort) -> Block {
    let top = top_by(&snapshot.processes, sort, TOP_PROCESSES);
    let note = |text: &str| vec![(text.to_string(), String::new())];
    let rows = if let Some(reason) = snapshot.errors.get(&SourceId::Processes) {
        note(reason)
    } else if snapshot.processes.is_empty() {
        note("Collecting…")
    } else if top.is_empty() {
        note("No activity")
    } else {
        top.into_iter()
            .map(|p| {
                let value = match sort {
                    ProcessSort::Cpu => format_value(p.cpu_percent, Unit::Percent),
                    ProcessSort::Memory => format_value(p.private_bytes, Unit::Bytes),
                    ProcessSort::Gpu => format_value(p.gpu_percent, Unit::Percent),
                };
                (p.name, value)
            })
            .collect()
    };
    Block::Rows {
        heading: Some("Top processes"),
        rows,
    }
}

fn cores(snapshot: &Snapshot) -> Vec<f64> {
    (0u16..)
        .map_while(|i| snapshot.get(MetricKey::CpuCore(i)))
        .collect()
}

fn volumes(snapshot: &Snapshot) -> Vec<(String, String)> {
    (b'A'..=b'Z')
        .filter_map(|letter| {
            let free = snapshot.get(MetricKey::VolumeFreeBytes(letter))?;
            let total = snapshot.get(MetricKey::VolumeTotalBytes(letter))?;
            Some((
                format!("{}:", letter as char),
                format!(
                    "{} free of {}",
                    format_value(free, Unit::Bytes),
                    format_value(total, Unit::Bytes)
                ),
            ))
        })
        .collect()
}

fn engines(snapshot: &Snapshot) -> Vec<(String, String)> {
    [
        (GpuEngineKind::ThreeD, "3D"),
        (GpuEngineKind::Compute, "Compute"),
        (GpuEngineKind::VideoDecode, "Video decode"),
        (GpuEngineKind::VideoEncode, "Video encode"),
        (GpuEngineKind::Copy, "Copy"),
    ]
    .into_iter()
    .filter_map(|(kind, label)| {
        let v = snapshot.get(MetricKey::GpuEngine(kind))?;
        Some((label.to_string(), format_value(v, Unit::Percent)))
    })
    .collect()
}

fn ping_stats(history: &HistoryStore) -> String {
    let h = history.get(MetricKey::PingMs);
    let stat = |f: fn(&pulsar_core::history::History) -> Option<f64>| {
        h.and_then(f).map_or("—".to_string(), |v| format!("{v:.0}"))
    };
    format!(
        "{} / {} / {} ms",
        stat(|h| h.min()),
        stat(|h| h.avg()),
        stat(|h| h.max())
    )
}

pub fn build(
    kind: ItemKind,
    snapshot: &Snapshot,
    history: &HistoryStore,
    ping_host: &str,
) -> PopupContent {
    let error = snapshot.errors.get(&kind.source()).cloned();
    let (title, value, series, blocks) = match kind {
        ItemKind::Cpu => (
            "CPU",
            cell_value(kind, CellPart::Main, snapshot),
            vec![CellPart::Main],
            vec![
                Block::Rows {
                    heading: None,
                    rows: vec![("Speed".into(), fmt(snapshot, MetricKey::CpuClockMhz))],
                },
                Block::Bars {
                    heading: "Cores",
                    values: cores(snapshot),
                },
                processes(snapshot, ProcessSort::Cpu),
            ],
        ),
        ItemKind::Ram => (
            "Memory",
            cell_value(kind, CellPart::Main, snapshot),
            vec![CellPart::Main],
            vec![
                Block::Rows {
                    heading: None,
                    rows: vec![
                        (
                            "In use".into(),
                            of(snapshot, MetricKey::MemUsedBytes, MetricKey::MemTotalBytes),
                        ),
                        (
                            "Available".into(),
                            fmt(snapshot, MetricKey::MemAvailableBytes),
                        ),
                        (
                            "Committed".into(),
                            of(
                                snapshot,
                                MetricKey::CommitBytes,
                                MetricKey::CommitLimitBytes,
                            ),
                        ),
                    ],
                },
                processes(snapshot, ProcessSort::Memory),
            ],
        ),
        ItemKind::Disk => (
            "Disk",
            cell_value(kind, CellPart::Main, snapshot),
            vec![CellPart::Main],
            vec![
                Block::Rows {
                    heading: None,
                    rows: vec![
                        ("Read".into(), fmt(snapshot, MetricKey::DiskReadBps)),
                        ("Write".into(), fmt(snapshot, MetricKey::DiskWriteBps)),
                    ],
                },
                Block::Rows {
                    heading: Some("Free space"),
                    rows: match snapshot.errors.get(&SourceId::DiskSpace) {
                        Some(reason) => vec![(reason.clone(), String::new())],
                        None => volumes(snapshot),
                    },
                },
            ],
        ),
        ItemKind::Network => {
            let adapters: Vec<(String, String)> = snapshot
                .adapters
                .iter()
                .map(|a| {
                    (
                        a.name.clone(),
                        format!(
                            "↓ {}  ↑ {}",
                            format_value(a.down_bps, Unit::BytesPerSec),
                            format_value(a.up_bps, Unit::BytesPerSec)
                        ),
                    )
                })
                .collect();
            (
                "Network",
                format!(
                    "↓ {}  ↑ {}",
                    cell_value(kind, CellPart::Down, snapshot),
                    cell_value(kind, CellPart::Up, snapshot)
                ),
                vec![CellPart::Down, CellPart::Up],
                vec![Block::Rows {
                    heading: Some("Adapters"),
                    rows: if adapters.is_empty() {
                        vec![("No traffic".into(), String::new())]
                    } else {
                        adapters
                    },
                }],
            )
        }
        ItemKind::Gpu => (
            "GPU",
            cell_value(kind, CellPart::Main, snapshot),
            vec![CellPart::Main],
            vec![
                Block::Rows {
                    heading: Some("Engines"),
                    rows: engines(snapshot),
                },
                Block::Rows {
                    heading: Some("Memory"),
                    rows: vec![
                        (
                            "Dedicated".into(),
                            fmt(snapshot, MetricKey::GpuDedicatedBytes),
                        ),
                        ("Shared".into(), fmt(snapshot, MetricKey::GpuSharedBytes)),
                    ],
                },
                processes(snapshot, ProcessSort::Gpu),
            ],
        ),
        ItemKind::Ping => (
            "Ping",
            cell_value(kind, CellPart::Main, snapshot),
            vec![CellPart::Main],
            vec![Block::Rows {
                heading: None,
                rows: vec![
                    ("Host".into(), ping_host.to_string()),
                    ("Min / avg / max".into(), ping_stats(history)),
                    (
                        "Packet loss".into(),
                        fmt(snapshot, MetricKey::PingLossPercent),
                    ),
                ],
            }],
        ),
    };
    PopupContent {
        kind,
        title,
        value,
        series,
        error,
        blocks,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pulsar_core::metric::{AdapterRate, ProcessUsage};

    const GB: f64 = 1024.0 * 1024.0 * 1024.0;

    fn rows(block: &Block) -> &[(String, String)] {
        match block {
            Block::Rows { rows, .. } => rows,
            Block::Bars { .. } => panic!("expected rows"),
        }
    }

    fn proc(name: &str, cpu: f64, mem: f64, gpu: f64) -> ProcessUsage {
        ProcessUsage {
            name: name.into(),
            cpu_percent: cpu,
            private_bytes: mem,
            gpu_percent: gpu,
        }
    }

    #[test]
    fn cpu_shows_speed_cores_and_top_processes() {
        let mut s = Snapshot::default();
        s.set(MetricKey::CpuTotal, 23.0);
        s.set(MetricKey::CpuClockMhz, 3920.0);
        for (i, v) in [10.0, 90.0, 40.0].into_iter().enumerate() {
            s.set(MetricKey::CpuCore(i as u16), v);
        }
        s.processes = vec![
            proc("idle", 0.0, 1.0, 0.0),
            proc("code", 12.0, 1.0, 0.0),
            proc("chrome", 30.0, 1.0, 0.0),
        ];
        let c = build(ItemKind::Cpu, &s, &HistoryStore::new(60), "");
        assert_eq!((c.title, c.value.as_str()), ("CPU", "23%"));
        assert_eq!(rows(&c.blocks[0])[0], ("Speed".into(), "3.92 GHz".into()));
        assert_eq!(
            c.blocks[1],
            Block::Bars {
                heading: "Cores",
                values: vec![10.0, 90.0, 40.0]
            }
        );
        let top = rows(&c.blocks[2]);
        assert_eq!(top[0], ("chrome".into(), "30%".into()));
        assert_eq!(top.len(), 2, "processes at zero are left out");
    }

    #[test]
    fn process_list_says_collecting_before_first_sample() {
        let c = build(
            ItemKind::Ram,
            &Snapshot::default(),
            &HistoryStore::new(60),
            "",
        );
        assert_eq!(
            rows(&c.blocks[1]),
            [("Collecting…".to_string(), String::new())]
        );
    }

    #[test]
    fn idle_process_list_says_no_activity_not_collecting() {
        let s = Snapshot {
            processes: vec![proc("idle", 0.0, 1.0, 0.0)],
            ..Snapshot::default()
        };
        let c = build(ItemKind::Gpu, &s, &HistoryStore::new(60), "");
        assert_eq!(
            rows(&c.blocks[2]),
            [("No activity".to_string(), String::new())]
        );
    }

    #[test]
    fn failed_process_source_shows_its_reason() {
        let mut s = Snapshot::default();
        s.errors
            .insert(SourceId::Processes, "Process counters unavailable".into());
        let c = build(ItemKind::Cpu, &s, &HistoryStore::new(60), "");
        assert_eq!(rows(&c.blocks[2])[0].0, "Process counters unavailable");
    }

    #[test]
    fn failed_disk_space_shows_its_reason() {
        let mut s = Snapshot::default();
        s.errors.insert(SourceId::DiskSpace, "Access denied".into());
        let c = build(ItemKind::Disk, &s, &HistoryStore::new(60), "");
        assert_eq!(
            rows(&c.blocks[1]),
            [("Access denied".to_string(), String::new())]
        );
    }

    #[test]
    fn memory_shows_used_of_total_and_commit() {
        let mut s = Snapshot::default();
        s.set(MetricKey::MemUsedBytes, 26.2 * GB);
        s.set(MetricKey::MemTotalBytes, 31.8 * GB);
        s.set(MetricKey::CommitBytes, 50.0 * GB);
        s.set(MetricKey::CommitLimitBytes, 62.0 * GB);
        let c = build(ItemKind::Ram, &s, &HistoryStore::new(60), "");
        let r = rows(&c.blocks[0]);
        assert_eq!(r[0].1, "26.2 GB of 31.8 GB");
        assert_eq!(r[2].1, "50.0 GB of 62.0 GB");
        assert_eq!(r[1].1, "—", "missing metric shows a dash");
    }

    #[test]
    fn disk_lists_volumes_in_letter_order() {
        let mut s = Snapshot::default();
        for (letter, free) in [(b'D', 100.0), (b'C', 269.0)] {
            s.set(MetricKey::VolumeFreeBytes(letter), free * GB);
            s.set(MetricKey::VolumeTotalBytes(letter), 931.0 * GB);
        }
        let c = build(ItemKind::Disk, &s, &HistoryStore::new(60), "");
        let v = rows(&c.blocks[1]);
        assert_eq!(v[0], ("C:".into(), "269 GB free of 931 GB".into()));
        assert_eq!(v[1].0, "D:");
    }

    #[test]
    fn network_shows_both_directions_and_adapters() {
        let mut s = Snapshot::default();
        s.set(MetricKey::NetDownBps, 2048.0);
        s.set(MetricKey::NetUpBps, 512.0);
        s.adapters = vec![AdapterRate {
            name: "Realtek".into(),
            down_bps: 2048.0,
            up_bps: 512.0,
        }];
        let c = build(ItemKind::Network, &s, &HistoryStore::new(60), "");
        assert_eq!(c.value, "↓ 2.0 KB/s  ↑ 512 B/s");
        assert_eq!(c.series, vec![CellPart::Down, CellPart::Up]);
        assert_eq!(
            rows(&c.blocks[0])[0],
            ("Realtek".into(), "↓ 2.0 KB/s  ↑ 512 B/s".into())
        );

        let idle = build(
            ItemKind::Network,
            &Snapshot::default(),
            &HistoryStore::new(60),
            "",
        );
        assert_eq!(rows(&idle.blocks[0])[0].0, "No traffic");
    }

    #[test]
    fn gpu_lists_only_reported_engines() {
        let mut s = Snapshot::default();
        s.set(MetricKey::GpuEngine(GpuEngineKind::ThreeD), 7.0);
        s.set(MetricKey::GpuEngine(GpuEngineKind::VideoDecode), 2.0);
        let c = build(ItemKind::Gpu, &s, &HistoryStore::new(60), "");
        let labels: Vec<&str> = rows(&c.blocks[0]).iter().map(|(l, _)| l.as_str()).collect();
        assert_eq!(labels, ["3D", "Video decode"]);
    }

    #[test]
    fn ping_shows_host_and_history_stats() {
        let mut history = HistoryStore::new(60);
        for ms in [12.0, 20.0, 16.0] {
            let mut s = Snapshot::default();
            s.set(MetricKey::PingMs, ms);
            history.record(&s);
        }
        let c = build(ItemKind::Ping, &Snapshot::default(), &history, "1.1.1.1");
        let r = rows(&c.blocks[0]);
        assert_eq!(r[0].1, "1.1.1.1");
        assert_eq!(r[1].1, "12 / 16 / 20 ms");
    }

    #[test]
    fn failed_source_reason_is_shown() {
        let mut s = Snapshot::default();
        s.errors
            .insert(SourceId::Gpu, "GPU counters unavailable".into());
        let c = build(ItemKind::Gpu, &s, &HistoryStore::new(60), "");
        assert_eq!(c.error.as_deref(), Some("GPU counters unavailable"));
        assert_eq!(c.value, "—");
    }

    #[test]
    fn process_lists_only_for_cpu_ram_gpu() {
        let with: Vec<ItemKind> = ItemKind::ALL
            .into_iter()
            .filter(|&k| needs_processes(k))
            .collect();
        assert_eq!(with, [ItemKind::Cpu, ItemKind::Ram, ItemKind::Gpu]);
    }
}
