//! What a hover popup shows for an item, independent of how it is drawn.

use pulsar_core::config::{Config, PopupDetail};
use pulsar_core::format::format_value;
use pulsar_core::history::HistoryStore;
use pulsar_core::layout::{CellPart, network_parts};
use pulsar_core::metric::{GpuEngineKind, ItemKind, MetricKey, Snapshot, SourceId, Unit};
use pulsar_core::sources::{ProcessSort, top_by};
use pulsar_core::tr;

use pulsar_core::display::{cell_value, metric_for};

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
    /// Shares of a whole as one stacked bar, with a legend row per part.
    Composition {
        heading: &'static str,
        /// Label, amount, and the amount as shown.
        parts: Vec<(String, f64, String)>,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct PopupContent {
    pub kind: ItemKind,
    pub title: &'static str,
    pub value: String,
    /// Graph series, drawn in order; the first is filled.
    pub series: Vec<CellPart>,
    pub graph: bool,
    pub error: Option<String>,
    pub blocks: Vec<Block>,
}

/// What the settings ask the popups to show.
#[derive(Clone, Debug, PartialEq)]
pub struct Options<'a> {
    pub detail: PopupDetail,
    pub graph: bool,
    /// Rows in top-process lists; 0 leaves them out.
    pub processes: usize,
    pub ping_host: &'a str,
    /// The letters shown as separate disk cells.
    pub drives: Vec<u8>,
    pub upload_on_top: bool,
    /// Names for physical drives by number, for drive temperatures.
    pub disk_names: &'a [(u8, String)],
}

impl<'a> Options<'a> {
    pub fn new(config: &'a Config, kind: ItemKind) -> Self {
        Self {
            detail: config.popup_detail(kind),
            graph: config.popup.graph,
            processes: usize::from(config.popup.processes),
            ping_host: &config.ping.host,
            drives: config.drive_letters(),
            upload_on_top: config.display.upload_on_top,
            disk_names: &[],
        }
    }
}

/// Whether the popup lists processes, so the sampler must collect them.
pub fn needs_processes(kind: ItemKind, options: &Options) -> bool {
    options.processes > 0
        && options.detail != PopupDetail::Simple
        && matches!(
            kind,
            ItemKind::Cpu | ItemKind::Ram | ItemKind::Gpu | ItemKind::Disk
        )
}

fn fmt(snapshot: &Snapshot, key: MetricKey) -> String {
    format_value(snapshot.get(key).unwrap_or(f64::NAN), key.unit())
}

fn of(snapshot: &Snapshot, used: MetricKey, total: MetricKey) -> String {
    tr!("{} of {}", fmt(snapshot, used), fmt(snapshot, total))
}

fn processes(snapshot: &Snapshot, sort: ProcessSort, count: usize) -> Block {
    let top = top_by(&snapshot.processes, sort, count);
    let note = |text: &str| vec![(text.to_string(), String::new())];
    let rows = if let Some(reason) = snapshot.errors.get(&SourceId::Processes) {
        note(reason)
    } else if snapshot.processes.is_empty() {
        note(tr!("Collecting…"))
    } else if top.is_empty() {
        note(tr!("No activity"))
    } else {
        top.into_iter()
            .map(|p| {
                let value = match sort {
                    ProcessSort::Cpu => format_value(p.cpu_percent, Unit::Percent),
                    ProcessSort::Memory => format_value(p.private_bytes, Unit::Bytes),
                    ProcessSort::Gpu => format_value(p.gpu_percent, Unit::Percent),
                    ProcessSort::Io => format_value(p.io_bps, Unit::BytesPerSec),
                };
                (p.name, value)
            })
            .collect()
    };
    Block::Rows {
        heading: Some(if sort == ProcessSort::Io {
            tr!("Top processes (I/O)")
        } else {
            tr!("Top processes")
        }),
        rows,
    }
}

fn is_process_list(block: &Block) -> bool {
    matches!(block, Block::Rows { heading: Some(h), .. }
        if [tr!("Top processes"), tr!("Top processes (I/O)")].contains(h))
}

/// Rows for the metrics that have a reading, leaving out the rest.
fn known(snapshot: &Snapshot, rows: &[(&str, MetricKey)]) -> Vec<(String, String)> {
    rows.iter()
        .filter_map(|&(label, key)| {
            let value = snapshot.get(key)?;
            Some((label.to_string(), format_value(value, key.unit())))
        })
        .collect()
}

/// Memory in use as Task Manager counts it, leaving out modified pages
/// when the memory lists can be read.
fn memory_in_use(snapshot: &Snapshot) -> Option<f64> {
    let lists = || {
        let total = snapshot.get(MetricKey::MemTotalBytes)?;
        let standby = snapshot.get(MetricKey::MemStandbyBytes)?;
        let modified = snapshot.get(MetricKey::MemModifiedBytes)?;
        let free = snapshot.get(MetricKey::MemFreeBytes)?;
        Some((total - standby - modified - free).max(0.0))
    };
    lists().or_else(|| snapshot.get(MetricKey::MemUsedBytes))
}

/// Where physical memory goes, as Task Manager splits it.
fn memory_composition(snapshot: &Snapshot) -> Option<Block> {
    snapshot.get(MetricKey::MemTotalBytes)?;
    let standby = snapshot.get(MetricKey::MemStandbyBytes)?;
    let modified = snapshot.get(MetricKey::MemModifiedBytes)?;
    let free = snapshot.get(MetricKey::MemFreeBytes)?;
    let in_use = memory_in_use(snapshot)?;
    let part =
        |label: &str, bytes: f64| (label.to_string(), bytes, format_value(bytes, Unit::Bytes));
    Some(Block::Composition {
        heading: tr!("Breakdown"),
        parts: vec![
            part(tr!("In use"), in_use),
            part(tr!("Modified"), modified),
            part(tr!("Standby"), standby),
            part(tr!("Free"), free),
        ],
    })
}

/// Charging, on battery, or plugged in and full.
fn battery_state(snapshot: &Snapshot) -> Option<&'static str> {
    let plugged = snapshot.get(MetricKey::BatteryPluggedIn)? > 0.0;
    let charging = snapshot.get(MetricKey::BatteryCharging).unwrap_or(0.0) > 0.0;
    Some(match (plugged, charging) {
        (_, true) => tr!("Charging"),
        (true, false) => tr!("On AC power"),
        (false, false) => tr!("On battery"),
    })
}

fn battery_rows(snapshot: &Snapshot) -> Vec<(String, String)> {
    let mut rows = Vec::new();
    if let Some(state) = battery_state(snapshot) {
        rows.push((tr!("State").to_string(), state.to_string()));
    }
    let time_label = if snapshot.get(MetricKey::BatteryCharging).unwrap_or(0.0) > 0.0 {
        tr!("Time to full")
    } else {
        tr!("Remaining")
    };
    rows.extend(known(
        snapshot,
        &[(time_label, MetricKey::BatterySecondsLeft)],
    ));
    rows
}

fn battery_details(snapshot: &Snapshot) -> Vec<(String, String)> {
    let mut rows = Vec::new();
    if let Some(rate) = snapshot.get(MetricKey::BatteryRateW) {
        let label = if rate >= 0.0 {
            tr!("Charge rate")
        } else {
            tr!("Discharge rate")
        };
        rows.push((label.to_string(), format_value(rate.abs(), Unit::Watts)));
    }
    if let (Some(left), Some(full)) = (
        snapshot.get(MetricKey::BatteryRemainingWh),
        snapshot.get(MetricKey::BatteryFullWh),
    ) {
        rows.push((
            tr!("Energy").to_string(),
            tr!(
                "{} of {}",
                format_value(left, Unit::WattHours),
                format_value(full, Unit::WattHours)
            ),
        ));
    }
    rows.extend(known(
        snapshot,
        &[
            (tr!("Capacity when new"), MetricKey::BatteryDesignWh),
            (tr!("Health"), MetricKey::BatteryHealthPercent),
            (tr!("Charge cycles"), MetricKey::BatteryCycles),
            (tr!("Voltage"), MetricKey::BatteryVolts),
        ],
    ));
    rows
}

/// The application in front and its frames, and the display's rate.
fn frame_rows(snapshot: &Snapshot) -> Vec<(String, String)> {
    let labels = &snapshot.frames;
    let mut rows: Vec<(String, String)> = labels
        .app
        .iter()
        .map(|app| (tr!("Application").to_string(), app.clone()))
        .collect();
    if labels.pinned {
        rows.extend(known(
            snapshot,
            &[(tr!("Application rate"), MetricKey::AppFrameRate)],
        ));
    }
    rows.extend(known(
        snapshot,
        &[
            (tr!("Frame time"), MetricKey::FrameTimeMs),
            (tr!("1% low"), MetricKey::FrameRateLow1),
            (tr!("0.1% low"), MetricKey::FrameRateLow01),
        ],
    ));
    if let (Some(name), Some(fps)) = (&labels.display, snapshot.get(MetricKey::DisplayFrameRate)) {
        rows.push((
            tr!("Display ({})", name),
            format_value(fps, Unit::FramesPerSec),
        ));
    }
    rows
}

/// The lowest, mean and highest of the graph's history.
fn history_range(history: &HistoryStore, key: MetricKey) -> Option<String> {
    let h = history.get(key)?;
    let unit = key.unit();
    Some(format!(
        "{} / {} / {}",
        format_value(h.min()?, unit),
        format_value(h.avg()?, unit),
        format_value(h.max()?, unit)
    ))
}

/// What the detailed popup adds to the standard one's first rows.
fn detailed_rows(kind: ItemKind, history: &HistoryStore) -> Vec<(String, String)> {
    match kind {
        ItemKind::Network => Vec::new(),
        ItemKind::Ping => history
            .get(MetricKey::PingMs)
            .and_then(jitter)
            .map(|ms| (tr!("Jitter").to_string(), format_value(ms, Unit::Millis)))
            .into_iter()
            .collect(),
        _ => history_range(history, metric_for(kind, CellPart::Main))
            .map(|range| (tr!("Min / avg / max").to_string(), range))
            .into_iter()
            .collect(),
    }
}

/// What the detailed popup adds to the standard one.
fn detailed_blocks(kind: ItemKind, snapshot: &Snapshot) -> Vec<Block> {
    match kind {
        ItemKind::Cpu => vec![Block::Rows {
            heading: Some(tr!("System")),
            rows: known(
                snapshot,
                &[
                    (tr!("Processes"), MetricKey::ProcessCount),
                    (tr!("Threads"), MetricKey::ThreadCount),
                    (tr!("Handles"), MetricKey::HandleCount),
                    (tr!("Up time"), MetricKey::UptimeSeconds),
                ],
            ),
        }],
        ItemKind::Ram => memory_composition(snapshot)
            .into_iter()
            .chain([Block::Rows {
                heading: Some(tr!("Kernel")),
                rows: known(
                    snapshot,
                    &[
                        (tr!("Paged pool"), MetricKey::MemPagedPoolBytes),
                        (tr!("Non-paged pool"), MetricKey::MemNonPagedPoolBytes),
                        (
                            tr!("Reserved by hardware"),
                            MetricKey::MemHardwareReservedBytes,
                        ),
                    ],
                ),
            }])
            .collect(),
        ItemKind::Network => vec![Block::Rows {
            heading: Some(tr!("Since Pulsar started")),
            rows: known(
                snapshot,
                &[
                    (tr!("Received"), MetricKey::NetDownTotalBytes),
                    (tr!("Sent"), MetricKey::NetUpTotalBytes),
                ],
            ),
        }],
        ItemKind::Battery => vec![Block::Rows {
            heading: Some(tr!("Battery")),
            rows: battery_details(snapshot),
        }],
        ItemKind::Gpu => vec![
            Block::Rows {
                heading: Some(tr!("Engines")),
                rows: engines(snapshot),
            },
            Block::Rows {
                heading: Some(tr!("Sensors")),
                rows: match snapshot.get(MetricKey::GpuTempC) {
                    Some(_) => sensors(snapshot),
                    None => Vec::new(),
                },
            },
        ],
        ItemKind::Disk => vec![Block::Rows {
            heading: Some(tr!("Free space")),
            rows: match snapshot.errors.get(&SourceId::DiskSpace) {
                Some(reason) => vec![(reason.clone(), String::new())],
                None => volumes(snapshot),
            },
        }],
        ItemKind::CpuTemp => vec![Block::Bars {
            heading: tr!("Core temperatures"),
            values: core_temps(snapshot),
        }],
        ItemKind::CpuPower => vec![
            Block::Rows {
                heading: Some(tr!("Power")),
                rows: cpu_power_rows(snapshot),
            },
            Block::Bars {
                heading: tr!("Core power"),
                values: relative_bars(&core_values(snapshot, MetricKey::CpuCorePowerW)),
            },
        ],
        ItemKind::CpuClock => vec![Block::Bars {
            heading: tr!("Core clocks"),
            values: relative_bars(&core_values(snapshot, MetricKey::CpuCoreClockMhz)),
        }],
        _ => Vec::new(),
    }
}

/// Where the package power goes, and the limits it is held to.
fn cpu_power_rows(snapshot: &Snapshot) -> Vec<(String, String)> {
    let mut rows = known(
        snapshot,
        &[
            (tr!("Cores"), MetricKey::CpuCoresPowerW),
            (tr!("Graphics"), MetricKey::CpuGraphicsPowerW),
            (tr!("Memory"), MetricKey::CpuDramPowerW),
            ("TDP", MetricKey::CpuTdpW),
        ],
    );
    // Desktop processors report no memory power, as a steady zero.
    if snapshot.get(MetricKey::CpuDramPowerW) == Some(0.0) {
        rows.retain(|(label, _)| label != tr!("Memory"));
    }
    for (label, key) in [
        (tr!("Sustained limit"), MetricKey::CpuPowerLimit1W),
        (tr!("Burst limit"), MetricKey::CpuPowerLimit2W),
    ] {
        // The largest value the register holds means no limit.
        if let Some(w) = snapshot.get(key) {
            let value = if w >= 4095.0 {
                tr!("Unlimited").to_string()
            } else {
                format_value(w, Unit::Watts)
            };
            rows.push((label.to_string(), value));
        }
    }
    rows
}

/// The mean change between consecutive replies.
fn jitter(history: &pulsar_core::history::History) -> Option<f64> {
    let values: Vec<f64> = history.iter().filter(|v| !v.is_nan()).collect();
    let steps: Vec<f64> = values.windows(2).map(|w| (w[1] - w[0]).abs()).collect();
    (!steps.is_empty()).then(|| steps.iter().sum::<f64>() / steps.len() as f64)
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
                tr!(
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
        (GpuEngineKind::ThreeD, tr!("3D")),
        (GpuEngineKind::Compute, tr!("Compute")),
        (GpuEngineKind::VideoDecode, tr!("Video decode")),
        (GpuEngineKind::VideoEncode, tr!("Video encode")),
        (GpuEngineKind::Copy, tr!("Copy")),
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

fn sensors(snapshot: &Snapshot) -> Vec<(String, String)> {
    let mut rows = vec![
        (tr!("Fan").into(), fmt(snapshot, MetricKey::GpuFanRpm)),
        (
            tr!("Power").into(),
            fmt(snapshot, MetricKey::GpuPowerPercent),
        ),
        (
            tr!("Memory clock").into(),
            fmt(snapshot, MetricKey::GpuMemClockMhz),
        ),
    ];
    // Only the sensor service reports these, and only for some cards.
    for (label, key) in [
        (tr!("Hot spot"), MetricKey::GpuHotSpotC),
        (tr!("Memory temperature"), MetricKey::GpuMemTempC),
    ] {
        if let Some(value) = snapshot.get(key) {
            rows.push((label.into(), format_value(value, key.unit())));
        }
    }
    rows
}

fn core_temps(snapshot: &Snapshot) -> Vec<f64> {
    core_values(snapshot, MetricKey::CpuCoreTempC)
}

fn core_values(snapshot: &Snapshot, key: fn(u16) -> MetricKey) -> Vec<f64> {
    (0..).map_while(|i| snapshot.get(key(i))).collect()
}

/// Whatever the sensor service reported; nothing without it.
fn cpu_sensors(snapshot: &Snapshot) -> Vec<(String, String)> {
    let mut rows = Vec::new();
    let mut row = |label: &str, key: MetricKey| {
        if let Some(value) = snapshot.get(key) {
            rows.push((label.to_string(), format_value(value, key.unit())));
        }
    };
    row(tr!("Temperature"), MetricKey::CpuTempC);
    row(tr!("Power"), MetricKey::CpuPowerW);
    if let Some(hottest) = core_temps(snapshot).into_iter().reduce(f64::max) {
        rows.push((
            tr!("Hottest core").into(),
            format_value(hottest, Unit::Celsius),
        ));
    }
    if let Some(throttling) = snapshot.get(MetricKey::CpuThrottling) {
        let text = if throttling > 0.0 {
            tr!("Yes")
        } else {
            tr!("No")
        };
        rows.push((tr!("Thermal throttling").into(), text.into()));
    }
    rows
}

/// Each value as a share of the highest one.
fn relative_bars(values: &[f64]) -> Vec<f64> {
    let highest = values.iter().copied().fold(0.0, f64::max);
    if highest <= 0.0 {
        return Vec::new();
    }
    values.iter().map(|v| v / highest * 100.0).collect()
}

/// Drives that reported a temperature, by name where known.
fn drive_temps(snapshot: &Snapshot, names: &[(u8, String)]) -> Vec<(String, String)> {
    (0..=u8::MAX)
        .filter_map(|n| {
            let temp = snapshot.get(MetricKey::DiskTempC(n))?;
            let name = names
                .iter()
                .find(|(number, _)| *number == n)
                .map_or_else(|| tr!("Disk {}", n), |(_, name)| name.clone());
            Some((name, format_value(temp, Unit::Celsius)))
        })
        .collect()
}

fn drive_rows(snapshot: &Snapshot, drives: &[u8]) -> Vec<(String, String)> {
    drives
        .iter()
        .map(|&d| {
            let active = snapshot.get(MetricKey::DriveActivePercent(d));
            let used = snapshot.get(MetricKey::DriveUsedPercent(d));
            let value = match (active, used) {
                (None, None) => "—".to_string(),
                (a, u) => tr!(
                    "{} active · {} used",
                    format_value(a.unwrap_or(f64::NAN), Unit::Percent),
                    format_value(u.unwrap_or(f64::NAN), Unit::Percent)
                ),
            };
            (format!("{}:", d as char), value)
        })
        .collect()
}

/// The standard popup with five processes, for tests that do not care
/// about the options. `drives` are the letters shown as separate disk cells.
#[cfg(test)]
pub fn build(
    kind: ItemKind,
    snapshot: &Snapshot,
    history: &HistoryStore,
    ping_host: &str,
    drives: &[u8],
) -> PopupContent {
    let options = Options {
        detail: PopupDetail::Standard,
        graph: true,
        processes: 5,
        ping_host,
        drives: drives.to_vec(),
        upload_on_top: false,
        disk_names: &[],
    };
    build_with(kind, snapshot, history, &options)
}

pub fn build_with(
    kind: ItemKind,
    snapshot: &Snapshot,
    history: &HistoryStore,
    options: &Options,
) -> PopupContent {
    let (ping_host, drives, count) = (options.ping_host, &options.drives[..], options.processes);
    let error = snapshot.item_error(kind).cloned();
    let (title, value, series, mut blocks) = match kind {
        ItemKind::Cpu => (
            tr!("CPU"),
            cell_value(kind, CellPart::Main, snapshot),
            vec![CellPart::Main],
            vec![
                Block::Rows {
                    heading: None,
                    rows: [(tr!("Speed").into(), fmt(snapshot, MetricKey::CpuClockMhz))]
                        .into_iter()
                        .chain(cpu_sensors(snapshot))
                        .collect(),
                },
                Block::Bars {
                    heading: tr!("Cores"),
                    values: cores(snapshot),
                },
                processes(snapshot, ProcessSort::Cpu, count),
            ],
        ),
        ItemKind::Ram => (
            tr!("Memory"),
            cell_value(kind, CellPart::Main, snapshot),
            vec![CellPart::Main],
            vec![
                Block::Rows {
                    heading: None,
                    rows: vec![
                        (
                            tr!("In use").into(),
                            tr!(
                                "{} of {}",
                                format_value(
                                    memory_in_use(snapshot).unwrap_or(f64::NAN),
                                    Unit::Bytes
                                ),
                                fmt(snapshot, MetricKey::MemTotalBytes)
                            ),
                        ),
                        (
                            tr!("Available").into(),
                            fmt(snapshot, MetricKey::MemAvailableBytes),
                        ),
                        (
                            tr!("Committed").into(),
                            of(
                                snapshot,
                                MetricKey::CommitBytes,
                                MetricKey::CommitLimitBytes,
                            ),
                        ),
                    ],
                },
                processes(snapshot, ProcessSort::Memory, count),
            ],
        ),
        ItemKind::Disk => (
            tr!("Disk"),
            cell_value(kind, CellPart::Main, snapshot),
            vec![CellPart::Main],
            vec![
                Block::Rows {
                    heading: None,
                    rows: vec![
                        (tr!("Read").into(), fmt(snapshot, MetricKey::DiskReadBps)),
                        (tr!("Write").into(), fmt(snapshot, MetricKey::DiskWriteBps)),
                    ],
                },
                Block::Rows {
                    heading: Some(tr!("Drives")),
                    rows: drive_rows(snapshot, drives),
                },
                processes(snapshot, ProcessSort::Io, count),
            ],
        ),
        ItemKind::Network => {
            let order = network_parts(options.upload_on_top);
            let both = |down: String, up: String| {
                let (down, up) = (format!("↓ {down}"), format!("↑ {up}"));
                if options.upload_on_top {
                    format!("{up}  {down}")
                } else {
                    format!("{down}  {up}")
                }
            };
            let adapters: Vec<(String, String)> = snapshot
                .adapters
                .iter()
                .map(|a| {
                    (
                        a.name.clone(),
                        both(
                            format_value(a.down_bps, Unit::NetworkRate),
                            format_value(a.up_bps, Unit::NetworkRate),
                        ),
                    )
                })
                .collect();
            (
                tr!("Network"),
                both(
                    cell_value(kind, CellPart::Down, snapshot),
                    cell_value(kind, CellPart::Up, snapshot),
                ),
                order.to_vec(),
                vec![Block::Rows {
                    heading: Some(tr!("Adapters")),
                    rows: if adapters.is_empty() {
                        vec![(tr!("No traffic").into(), String::new())]
                    } else {
                        adapters
                    },
                }],
            )
        }
        ItemKind::Gpu => (
            tr!("GPU"),
            cell_value(kind, CellPart::Main, snapshot),
            vec![CellPart::Main],
            vec![
                Block::Rows {
                    heading: None,
                    rows: known(snapshot, &[(tr!("Temperature"), MetricKey::GpuTempC)]),
                },
                Block::Rows {
                    heading: Some(tr!("Memory")),
                    rows: vec![
                        (
                            tr!("Dedicated").into(),
                            fmt(snapshot, MetricKey::GpuDedicatedBytes),
                        ),
                        (
                            tr!("Shared").into(),
                            fmt(snapshot, MetricKey::GpuSharedBytes),
                        ),
                    ],
                },
                processes(snapshot, ProcessSort::Gpu, count),
            ],
        ),
        ItemKind::GpuTemp => (
            tr!("GPU temperature"),
            cell_value(kind, CellPart::Main, snapshot),
            vec![CellPart::Main],
            vec![Block::Rows {
                heading: None,
                rows: sensors(snapshot),
            }],
        ),
        ItemKind::CpuTemp | ItemKind::CpuPower => (
            if kind == ItemKind::CpuTemp {
                tr!("CPU temperature")
            } else {
                tr!("CPU power")
            },
            cell_value(kind, CellPart::Main, snapshot),
            vec![CellPart::Main],
            vec![Block::Rows {
                heading: None,
                rows: cpu_sensors(snapshot),
            }],
        ),
        ItemKind::CpuClock => {
            let clocks = core_values(snapshot, MetricKey::CpuCoreClockMhz);
            let mut rows = vec![(
                tr!("Average").to_string(),
                fmt(snapshot, MetricKey::CpuClockMhz),
            )];
            if let Some(fastest) = clocks.iter().copied().reduce(f64::max) {
                rows.push((tr!("Fastest core").into(), format_value(fastest, Unit::MHz)));
            }
            (
                tr!("CPU clock"),
                cell_value(kind, CellPart::Main, snapshot),
                vec![CellPart::Main],
                vec![Block::Rows {
                    heading: None,
                    rows,
                }],
            )
        }
        ItemKind::DiskTemp => (
            tr!("Drive temperature"),
            cell_value(kind, CellPart::Main, snapshot),
            vec![CellPart::Main],
            vec![Block::Rows {
                heading: None,
                rows: drive_temps(snapshot, options.disk_names),
            }],
        ),
        ItemKind::ChipsetTemp => (
            tr!("Chipset temperature"),
            cell_value(kind, CellPart::Main, snapshot),
            vec![CellPart::Main],
            vec![Block::Rows {
                heading: None,
                rows: [
                    (tr!("Chipset"), MetricKey::ChipsetTempC),
                    (tr!("CPU"), MetricKey::CpuTempC),
                    (tr!("GPU"), MetricKey::GpuTempC),
                    (tr!("Hottest drive"), MetricKey::DiskTempMaxC),
                ]
                .into_iter()
                .filter_map(|(label, key)| {
                    let value = snapshot.get(key)?;
                    Some((label.to_string(), format_value(value, key.unit())))
                })
                .collect(),
            }],
        ),
        ItemKind::Fps => (
            tr!("Frame rate"),
            cell_value(kind, CellPart::Main, snapshot),
            vec![CellPart::Main],
            vec![Block::Rows {
                heading: None,
                rows: frame_rows(snapshot),
            }],
        ),
        ItemKind::Ping => (
            tr!("Ping"),
            cell_value(kind, CellPart::Main, snapshot),
            vec![CellPart::Main],
            vec![Block::Rows {
                heading: None,
                rows: vec![
                    (tr!("Host").into(), ping_host.to_string()),
                    (tr!("Min / avg / max").into(), ping_stats(history)),
                    (
                        tr!("Packet loss").into(),
                        fmt(snapshot, MetricKey::PingLossPercent),
                    ),
                ],
            }],
        ),
        ItemKind::Battery => (
            tr!("Battery"),
            cell_value(kind, CellPart::Main, snapshot),
            vec![CellPart::Main],
            vec![Block::Rows {
                heading: None,
                rows: battery_rows(snapshot),
            }],
        ),
    };
    match options.detail {
        PopupDetail::Simple => blocks.clear(),
        PopupDetail::Standard => {}
        PopupDetail::Detailed => {
            let rows = detailed_rows(kind, history);
            match blocks.iter_mut().find_map(|b| match b {
                Block::Rows {
                    heading: None,
                    rows,
                } => Some(rows),
                _ => None,
            }) {
                Some(first) => first.extend(rows),
                None if !rows.is_empty() => blocks.insert(
                    0,
                    Block::Rows {
                        heading: None,
                        rows,
                    },
                ),
                None => {}
            }
            // Process lists stay last.
            let at = blocks
                .iter()
                .position(is_process_list)
                .unwrap_or(blocks.len());
            blocks.splice(at..at, detailed_blocks(kind, snapshot));
        }
    }
    if count == 0 {
        blocks.retain(|b| !is_process_list(b));
    }
    // Optional sections vanish rather than showing a bare heading.
    blocks.retain(|b| match b {
        Block::Rows { rows, .. } => !rows.is_empty(),
        Block::Bars { .. } => true,
        Block::Composition { parts, .. } => !parts.is_empty(),
    });
    PopupContent {
        kind,
        title,
        value,
        series,
        graph: options.graph,
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
            _ => panic!("expected rows"),
        }
    }

    fn proc(name: &str, cpu: f64, mem: f64, gpu: f64) -> ProcessUsage {
        ProcessUsage {
            name: name.into(),
            cpu_percent: cpu,
            private_bytes: mem,
            gpu_percent: gpu,
            io_bps: mem,
        }
    }

    fn options(detail: PopupDetail, processes: usize) -> Options<'static> {
        Options {
            detail,
            graph: true,
            processes,
            ping_host: "",
            drives: Vec::new(),
            upload_on_top: false,
            disk_names: &[],
        }
    }

    fn detailed(kind: ItemKind, snapshot: &Snapshot) -> PopupContent {
        build_with(
            kind,
            snapshot,
            &HistoryStore::new(60),
            &options(PopupDetail::Detailed, 5),
        )
    }

    fn rows_under<'c>(c: &'c PopupContent, title: &str) -> &'c [(String, String)] {
        c.blocks
            .iter()
            .find(|b| heading(b) == Some(title))
            .map(rows)
            .unwrap_or_else(|| panic!("no {title} in {:?}", c.blocks))
    }

    fn heading(block: &Block) -> Option<&'static str> {
        match block {
            Block::Rows { heading, .. } => *heading,
            Block::Bars { heading, .. } | Block::Composition { heading, .. } => Some(heading),
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
        let c = build(ItemKind::Cpu, &s, &HistoryStore::new(60), "", &[]);
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
            &[],
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
        let c = build(ItemKind::Gpu, &s, &HistoryStore::new(60), "", &[]);
        let list = c.blocks.iter().find(|b| is_process_list(b)).unwrap();
        assert_eq!(rows(list), [("No activity".to_string(), String::new())]);
    }

    #[test]
    fn failed_process_source_shows_its_reason() {
        let mut s = Snapshot::default();
        s.errors
            .insert(SourceId::Processes, "Process counters unavailable".into());
        let c = build(ItemKind::Cpu, &s, &HistoryStore::new(60), "", &[]);
        assert_eq!(rows(&c.blocks[2])[0].0, "Process counters unavailable");
    }

    #[test]
    fn failed_disk_space_shows_its_reason() {
        let mut s = Snapshot::default();
        s.errors.insert(SourceId::DiskSpace, "Access denied".into());
        let c = detailed(ItemKind::Disk, &s);
        assert_eq!(
            rows_under(&c, "Free space"),
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
        let c = build(ItemKind::Ram, &s, &HistoryStore::new(60), "", &[]);
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
        let c = detailed(ItemKind::Disk, &s);
        let v = rows_under(&c, "Free space");
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
        let c = build(ItemKind::Network, &s, &HistoryStore::new(60), "", &[]);
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
            &[],
        );
        assert_eq!(rows(&idle.blocks[0])[0].0, "No traffic");
    }

    #[test]
    fn gpu_lists_only_reported_engines() {
        let mut s = Snapshot::default();
        s.set(MetricKey::GpuEngine(GpuEngineKind::ThreeD), 7.0);
        s.set(MetricKey::GpuEngine(GpuEngineKind::VideoDecode), 2.0);
        let c = detailed(ItemKind::Gpu, &s);
        let labels: Vec<&str> = rows_under(&c, "Engines")
            .iter()
            .map(|(l, _)| l.as_str())
            .collect();
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
        let c = build(
            ItemKind::Ping,
            &Snapshot::default(),
            &history,
            "1.1.1.1",
            &[],
        );
        let r = rows(&c.blocks[0]);
        assert_eq!(r[0].1, "1.1.1.1");
        assert_eq!(r[1].1, "12 / 16 / 20 ms");
    }

    #[test]
    fn failed_source_reason_is_shown() {
        let mut s = Snapshot::default();
        s.errors
            .insert(SourceId::Gpu, "GPU counters unavailable".into());
        let c = build(ItemKind::Gpu, &s, &HistoryStore::new(60), "", &[]);
        assert_eq!(c.error.as_deref(), Some("GPU counters unavailable"));
        assert_eq!(c.value, "—");
    }

    #[test]
    fn process_lists_for_cpu_ram_gpu_and_disk_unless_turned_off() {
        let standard = options(PopupDetail::Standard, 5);
        let with: Vec<ItemKind> = ItemKind::ALL
            .into_iter()
            .filter(|&k| needs_processes(k, &standard))
            .collect();
        assert_eq!(
            with,
            [ItemKind::Cpu, ItemKind::Ram, ItemKind::Gpu, ItemKind::Disk]
        );
        assert!(!needs_processes(
            ItemKind::Cpu,
            &options(PopupDetail::Standard, 0)
        ));
        assert!(!needs_processes(
            ItemKind::Cpu,
            &options(PopupDetail::Simple, 5)
        ));
    }

    #[test]
    fn simple_popups_show_only_the_value_and_graph() {
        let mut s = Snapshot::default();
        s.set(MetricKey::CpuTotal, 23.0);
        s.set(MetricKey::CpuClockMhz, 3920.0);
        let history = HistoryStore::new(60);
        let c = build_with(
            ItemKind::Cpu,
            &s,
            &history,
            &options(PopupDetail::Simple, 5),
        );
        assert_eq!(c.value, "23%");
        assert!(c.blocks.is_empty());
        assert!(c.graph);
    }

    #[test]
    fn process_count_and_graph_follow_the_options() {
        let s = Snapshot {
            processes: (0..10)
                .map(|i| proc(&format!("p{i}"), 1.0 + f64::from(i), 1.0, 0.0))
                .collect(),
            ..Snapshot::default()
        };
        let history = HistoryStore::new(60);
        let three = build_with(
            ItemKind::Cpu,
            &s,
            &history,
            &options(PopupDetail::Standard, 3),
        );
        let list = three.blocks.iter().find(|b| is_process_list(b)).unwrap();
        assert_eq!(rows(list).len(), 3);
        let mut none = options(PopupDetail::Standard, 0);
        none.graph = false;
        let c = build_with(ItemKind::Cpu, &s, &history, &none);
        assert!(!c.blocks.iter().any(is_process_list));
        assert!(!c.graph);
    }

    #[test]
    fn detailed_memory_splits_where_it_goes() {
        let mut s = Snapshot::default();
        s.set(MetricKey::MemTotalBytes, 32.0 * GB);
        s.set(MetricKey::MemStandbyBytes, 10.0 * GB);
        s.set(MetricKey::MemModifiedBytes, 1.0 * GB);
        s.set(MetricKey::MemFreeBytes, 5.0 * GB);
        s.set(MetricKey::MemPagedPoolBytes, 0.5 * GB);
        let c = build_with(
            ItemKind::Ram,
            &s,
            &HistoryStore::new(60),
            &options(PopupDetail::Detailed, 5),
        );
        let parts = c
            .blocks
            .iter()
            .find_map(|b| match b {
                Block::Composition { parts, .. } => Some(parts.clone()),
                _ => None,
            })
            .unwrap();
        let amounts: Vec<f64> = parts.iter().map(|(_, v, _)| v / GB).collect();
        assert_eq!(amounts, [16.0, 1.0, 10.0, 5.0]);
        assert_eq!(parts[0].2, "16.0 GB");
        assert!(c.blocks.iter().any(|b| heading(b) == Some("Kernel")));
        let standard = build_with(
            ItemKind::Ram,
            &s,
            &HistoryStore::new(60),
            &options(PopupDetail::Standard, 5),
        );
        assert!(
            !standard
                .blocks
                .iter()
                .any(|b| matches!(b, Block::Composition { .. }))
        );
    }

    #[test]
    fn detailed_cpu_adds_system_counts() {
        let mut s = Snapshot::default();
        s.set(MetricKey::ProcessCount, 312.0);
        s.set(MetricKey::UptimeSeconds, 7500.0);
        let c = build_with(
            ItemKind::Cpu,
            &s,
            &HistoryStore::new(60),
            &options(PopupDetail::Detailed, 0),
        );
        let system = c
            .blocks
            .iter()
            .find(|b| heading(b) == Some("System"))
            .unwrap();
        assert_eq!(
            rows(system),
            [
                ("Processes".to_string(), "312".to_string()),
                ("Up time".to_string(), "2 h 05 min".to_string())
            ]
        );
    }

    #[test]
    fn disk_lists_top_processes_by_io() {
        let s = Snapshot {
            processes: vec![
                proc("backup", 0.0, 4096.0, 0.0),
                proc("idle", 0.0, 0.0, 0.0),
            ],
            ..Snapshot::default()
        };
        let c = build(ItemKind::Disk, &s, &HistoryStore::new(60), "", &[]);
        let io = c
            .blocks
            .iter()
            .find(|b| heading(b) == Some("Top processes (I/O)"))
            .unwrap();
        assert_eq!(rows(io), [("backup".to_string(), "4.0 KB/s".to_string())]);
    }

    #[test]
    fn battery_shows_state_and_details() {
        let mut s = Snapshot::default();
        s.set(MetricKey::BatteryPercent, 76.0);
        s.set(MetricKey::BatteryPluggedIn, 0.0);
        s.set(MetricKey::BatteryCharging, 0.0);
        s.set(MetricKey::BatterySecondsLeft, 5400.0);
        s.set(MetricKey::BatteryRateW, -9.5);
        s.set(MetricKey::BatteryRemainingWh, 30.0);
        s.set(MetricKey::BatteryFullWh, 51.3);
        s.set(MetricKey::BatteryCycles, 212.0);
        let c = build_with(
            ItemKind::Battery,
            &s,
            &HistoryStore::new(60),
            &options(PopupDetail::Detailed, 5),
        );
        assert_eq!(c.value, "76%");
        assert_eq!(
            rows(&c.blocks[0]),
            [
                ("State".to_string(), "On battery".to_string()),
                ("Remaining".to_string(), "1 h 30 min".to_string())
            ]
        );
        let details = rows(&c.blocks[1]);
        assert_eq!(
            details[0],
            ("Discharge rate".to_string(), "9.5 W".to_string())
        );
        assert_eq!(details[1].1, "30.0 Wh of 51.3 Wh");
        assert!(details.contains(&("Charge cycles".to_string(), "212".to_string())));
    }

    #[test]
    fn a_missing_battery_says_why() {
        let mut s = Snapshot::default();
        s.errors
            .insert(SourceId::Battery, "This PC has no battery".into());
        let c = build(ItemKind::Battery, &s, &HistoryStore::new(60), "", &[]);
        assert_eq!(c.error.as_deref(), Some("This PC has no battery"));
        assert!(c.blocks.is_empty(), "no empty state rows");
    }

    #[test]
    fn upload_on_top_swaps_the_network_value_and_series() {
        let mut s = Snapshot::default();
        s.set(MetricKey::NetDownBps, 2048.0);
        s.set(MetricKey::NetUpBps, 512.0);
        let mut o = options(PopupDetail::Standard, 5);
        o.upload_on_top = true;
        let c = build_with(ItemKind::Network, &s, &HistoryStore::new(60), &o);
        assert_eq!(c.value, "↑ 512 B/s  ↓ 2.0 KB/s");
        assert_eq!(c.series, [CellPart::Up, CellPart::Down]);
    }

    #[test]
    fn ping_jitter_is_the_mean_step_between_replies() {
        let mut history = HistoryStore::new(60);
        for ms in [10.0, 20.0, 14.0] {
            let mut s = Snapshot::default();
            s.set(MetricKey::PingMs, ms);
            history.record(&s);
        }
        let c = build_with(
            ItemKind::Ping,
            &Snapshot::default(),
            &history,
            &options(PopupDetail::Detailed, 5),
        );
        assert_eq!(c.blocks.len(), 1, "jitter joins the other rows");
        assert_eq!(
            rows(&c.blocks[0]).last().unwrap(),
            &("Jitter".to_string(), "8.0 ms".to_string())
        );
    }

    #[test]
    fn detailed_adds_the_history_range_to_the_first_rows() {
        let mut history = HistoryStore::new(60);
        for c in [40.0, 50.0, 45.0] {
            let mut s = Snapshot::default();
            s.set(MetricKey::DiskTempMaxC, c);
            history.record(&s);
        }
        let c = build_with(
            ItemKind::DiskTemp,
            &Snapshot::default(),
            &history,
            &options(PopupDetail::Detailed, 5),
        );
        assert_eq!(
            rows(&c.blocks[0]),
            [(
                "Min / avg / max".to_string(),
                "40°C / 45°C / 50°C".to_string()
            )]
        );
    }

    #[test]
    fn detailed_sections_come_before_the_process_list() {
        let mut s = Snapshot::default();
        s.set(MetricKey::GpuTempC, 55.0);
        s.processes = vec![proc("game", 0.0, 0.0, 40.0)];
        let c = detailed(ItemKind::Gpu, &s);
        assert!(is_process_list(c.blocks.last().unwrap()));
        let standard = build(ItemKind::Gpu, &s, &HistoryStore::new(60), "", &[]);
        assert!(c.blocks.len() > standard.blocks.len());
    }

    #[test]
    fn cpu_power_details_show_the_split_and_limits() {
        let mut s = Snapshot::default();
        s.set(MetricKey::CpuCoresPowerW, 20.0);
        s.set(MetricKey::CpuPowerLimit1W, 125.0);
        s.set(MetricKey::CpuPowerLimit2W, 4095.0);
        s.set(MetricKey::CpuCorePowerW(0), 2.0);
        s.set(MetricKey::CpuCorePowerW(1), 4.0);
        let c = detailed(ItemKind::CpuPower, &s);
        assert_eq!(
            rows_under(&c, "Power"),
            [
                ("Cores".to_string(), "20 W".to_string()),
                ("Sustained limit".to_string(), "125 W".to_string()),
                ("Burst limit".to_string(), "Unlimited".to_string()),
            ]
        );
        assert!(c.blocks.contains(&Block::Bars {
            heading: "Core power",
            values: vec![50.0, 100.0],
        }));
    }

    #[test]
    fn drive_temperatures_use_drive_names() {
        let mut s = Snapshot::default();
        s.set(MetricKey::DiskTempC(0), 34.0);
        s.set(MetricKey::DiskTempC(4), 43.0);
        let names = [(4, "C: Samsung 990".to_string())];
        let mut o = options(PopupDetail::Standard, 5);
        o.disk_names = &names;
        let c = build_with(ItemKind::DiskTemp, &s, &HistoryStore::new(60), &o);
        let labels: Vec<&str> = rows(&c.blocks[0]).iter().map(|(l, _)| l.as_str()).collect();
        assert_eq!(labels, ["Disk 0", "C: Samsung 990"]);
    }

    #[test]
    fn memory_in_use_leaves_out_modified_pages() {
        let mut s = Snapshot::default();
        s.set(MetricKey::MemTotalBytes, 32.0 * GB);
        s.set(MetricKey::MemUsedBytes, 16.2 * GB);
        s.set(MetricKey::MemStandbyBytes, 15.0 * GB);
        s.set(MetricKey::MemModifiedBytes, 0.5 * GB);
        s.set(MetricKey::MemFreeBytes, 0.5 * GB);
        let c = detailed(ItemKind::Ram, &s);
        assert_eq!(rows(&c.blocks[0])[0].1, "16.0 GB of 32.0 GB");
        let Some(Block::Composition { parts, .. }) = c
            .blocks
            .iter()
            .find(|b| matches!(b, Block::Composition { .. }))
        else {
            panic!("no breakdown");
        };
        assert_eq!(parts[0].2, "16.0 GB");
    }

    #[test]
    fn gpu_temperature_popup_lists_the_sensors() {
        let mut s = Snapshot::default();
        s.set(MetricKey::GpuTempC, 49.1);
        s.set(MetricKey::GpuFanRpm, 0.0);
        s.set(MetricKey::GpuPowerPercent, 10.6);
        s.set(MetricKey::GpuMemClockMhz, 7001.0);
        let c = build(ItemKind::GpuTemp, &s, &HistoryStore::new(60), "", &[]);
        assert_eq!(c.title, "GPU temperature");
        assert_eq!(c.value, "49°C");
        let r = rows(&c.blocks[0]);
        assert_eq!(r[0], ("Fan".to_string(), "0 RPM".to_string()));
        assert_eq!(r[1], ("Power".to_string(), "11%".to_string()));
        assert_eq!(r[2], ("Memory clock".to_string(), "7.00 GHz".to_string()));
    }

    #[test]
    fn gpu_popup_shows_temperature_when_available() {
        let sensors = |c: &PopupContent| {
            c.blocks.iter().any(|b| {
                matches!(
                    b,
                    Block::Rows {
                        heading: Some("Sensors"),
                        ..
                    }
                )
            })
        };
        let mut s = Snapshot::default();
        s.set(MetricKey::GpuTempC, 55.0);
        assert!(sensors(&detailed(ItemKind::Gpu, &s)));
        assert!(!sensors(&build(
            ItemKind::Gpu,
            &s,
            &HistoryStore::new(60),
            "",
            &[]
        )));
        assert!(!sensors(&detailed(ItemKind::Gpu, &Snapshot::default())));
    }

    #[test]
    fn disk_popup_lists_chosen_drives() {
        let mut s = Snapshot::default();
        s.set(MetricKey::DriveActivePercent(b'C'), 12.0);
        s.set(MetricKey::DriveUsedPercent(b'C'), 48.0);
        let c = build(ItemKind::Disk, &s, &HistoryStore::new(60), "", b"CX");
        let drives = c
            .blocks
            .iter()
            .find_map(|b| match b {
                Block::Rows {
                    heading: Some("Drives"),
                    rows,
                } => Some(rows.clone()),
                _ => None,
            })
            .unwrap();
        assert_eq!(
            drives[0],
            ("C:".to_string(), "12% active · 48% used".to_string())
        );
        assert_eq!(
            drives[1],
            ("X:".to_string(), "—".to_string()),
            "a missing drive has no value"
        );
    }
}
