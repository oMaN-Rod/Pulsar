//! What each page of the details window shows, built from the static
//! hardware facts, the latest snapshot and the history.

use std::collections::{BTreeSet, HashMap};
use std::ops::Range;
use std::time::Duration;

use pulsar_core::config::Config;
use pulsar_core::format::{format_value, temperature};
use pulsar_core::hardware::cpu::{Cache, CacheKind, Topology};
use pulsar_core::hardware::pci::{self, Link};
use pulsar_core::hardware::{DiskInfo, HardwareInfo};
use pulsar_core::history::HistoryStore;
use pulsar_core::layout::CellPart;
use pulsar_core::metric::{
    GpuEngineKind, ItemKind, MetricKey, ProcessUsage, Snapshot, SourceId, Timing, Unit,
};
use pulsar_core::sensors::Unavailable;
use pulsar_core::sources::{ProcessSort, pdh_instance_name, top_by};
use pulsar_core::{l10n, tr, tr_noop};
use slint::{Color, Model, ModelRc, SharedString, VecModel};

use crate::graph::{self, Series, series_color};
use crate::prefs::{CoreView, MemoryView, Prefs};
use crate::{
    Core, Cores, Fact, Graph, MemUsage, Meter, NavEntry, SensorRow, Spec, SpecGroup, Stat,
};

const TOP_PROCESSES: usize = 6;
const SPARKLINE: usize = 30;
const GB: f64 = 1024.0 * 1024.0 * 1024.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    Overview,
    Cpu,
    Memory,
    Gpu,
    Storage,
    Network,
    System,
    Sensors,
}

impl Page {
    pub const ALL: [Page; 8] = [
        Page::Overview,
        Page::Cpu,
        Page::Memory,
        Page::Gpu,
        Page::Storage,
        Page::Network,
        Page::System,
        Page::Sensors,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Page::Overview => tr!("Overview"),
            Page::Cpu => tr!("Processor"),
            Page::Memory => tr!("Memory"),
            Page::Gpu => tr!("Graphics"),
            Page::Storage => tr!("Storage"),
            Page::Network => tr!("Network"),
            Page::System => tr!("System"),
            Page::Sensors => tr!("Sensors"),
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            Page::Overview => "\u{E80F}",
            Page::Cpu => "\u{E950}",
            Page::Memory => "\u{EEA0}",
            Page::Gpu => "\u{F211}",
            Page::Storage => "\u{EDA2}",
            Page::Network => "\u{E968}",
            Page::System => "\u{E7F4}",
            Page::Sensors => "\u{E9CA}",
        }
    }
}

/// Lowest and highest value seen of each metric since the window opened.
#[derive(Debug, Default)]
pub struct Extremes(HashMap<MetricKey, (f64, f64)>);

impl Extremes {
    pub fn record(&mut self, snapshot: &Snapshot) {
        for (&key, &value) in &snapshot.metrics {
            if !value.is_finite() {
                continue;
            }
            let e = self.0.entry(key).or_insert((value, value));
            e.0 = e.0.min(value);
            e.1 = e.1.max(value);
        }
    }

    pub fn seed(&mut self, history: &HistoryStore) {
        for (key, h) in history.iter() {
            if let (Some(lo), Some(hi)) = (h.min(), h.max()) {
                self.0.insert(key, (lo, hi));
            }
        }
    }

    pub fn get(&self, key: MetricKey) -> Option<(f64, f64)> {
        self.0.get(&key).copied()
    }
}

pub struct Data<'a> {
    pub info: &'a HardwareInfo,
    pub latest: &'a Snapshot,
    pub history: &'a HistoryStore,
    pub extremes: &'a Extremes,
    pub config: &'a Config,
    pub accent: Option<Color>,
    pub uptime: Duration,
    pub service: Result<(), Unavailable>,
    pub prefs: &'a Prefs,
}

#[derive(Debug, Default)]
pub struct View {
    pub heading: String,
    pub subheading: String,
    pub note: String,
    pub note_action: String,
    pub graphs: Vec<Graph>,
    /// Beside the memory usage card, when that shows.
    pub feature_graph: Graph,
    pub mem_usage: MemUsage,
    pub stats: Vec<Stat>,
    pub cores: Cores,
    pub core_cells: Vec<Core>,
    pub threads: Vec<f32>,
    /// One load graph per logical processor, when the cores card shows them.
    pub thread_graphs: Vec<Graph>,
    /// The overview's summary of the machine.
    pub facts: Vec<Fact>,
    pub groups: Vec<SpecGroup>,
    pub sensors: Vec<SensorRow>,
}

impl Data<'_> {
    fn get(&self, key: MetricKey) -> Option<f64> {
        self.latest.get(key)
    }

    fn fmt(&self, key: MetricKey) -> String {
        format_value(self.get(key).unwrap_or(f64::NAN), key.unit())
    }

    fn color(&self, kind: ItemKind) -> Color {
        series_color(self.config, self.accent, kind, CellPart::Main)
    }

    fn series(&self, key: MetricKey, kind: ItemKind, legend: &'static str) -> Series<'static> {
        Series {
            key,
            color: self.color(kind),
            legend,
        }
    }

    fn has_history(&self, key: MetricKey) -> bool {
        self.history.get(key).is_some_and(|h| h.max().is_some())
    }

    fn graph(&self, title: &str, key: MetricKey, kind: ItemKind, caption: String) -> Graph {
        graph::graph(
            self.history,
            title,
            self.fmt(key),
            caption,
            self.series(key, kind, ""),
            None,
        )
    }

    fn stat(&self, label: &str, key: MetricKey) -> Option<Stat> {
        self.get(key)
            .map(|v| stat(label, format_value(v, key.unit())))
    }

    /// Why temperatures and power are missing, and what to do about it.
    fn service_note(&self, view: &mut View) {
        let Err(reason) = self.service else {
            return;
        };
        view.note = match reason {
            Unavailable::NotInstalled => {
                tr!("Temperatures, power draw and clocks come from the optional Pulsar sensor service, which reads them through PawnIO.").into()
            }
            other => tr!("{}. Temperatures, power draw and clocks are unavailable until it is fixed.", other),
        };
        view.note_action = tr!("Learn how").into();
    }

    fn cpu_cores(&self) -> u32 {
        self.info.cpu.topology.cores
    }
}

fn stat(label: &str, value: impl Into<SharedString>) -> Stat {
    Stat {
        label: label.into(),
        value: value.into(),
        warn: false,
    }
}

fn model<T: Clone + 'static>(rows: Vec<T>) -> ModelRc<T> {
    ModelRc::new(VecModel::from(rows))
}

fn specs(rows: Vec<(&str, Option<String>)>) -> ModelRc<Spec> {
    model(
        rows.into_iter()
            .filter_map(|(label, value)| {
                Some(Spec {
                    label: label.into(),
                    value: value.filter(|v| !v.is_empty())?.into(),
                })
            })
            .collect(),
    )
}

fn group(icon: &str, title: &str, subtitle: &str, rows: Vec<(&str, Option<String>)>) -> SpecGroup {
    SpecGroup {
        tint: Color::default(),
        icon: icon.into(),
        title: title.into(),
        subtitle: subtitle.into(),
        badge: SharedString::default(),
        specs: specs(rows),
        meters: model(Vec::new()),
    }
}

fn meter(label: impl Into<SharedString>, level: f64, text: impl Into<SharedString>) -> Meter {
    Meter {
        label: label.into(),
        level: if level.is_finite() { level as f32 } else { 0.0 },
        text: text.into(),
    }
}

fn bytes(v: u64) -> String {
    format_value(v as f64, Unit::Bytes)
}

/// Marketing capacity: decimal units, as printed on the box.
fn capacity(bytes: u64) -> String {
    let gb = bytes as f64 / 1e9;
    if gb >= 1000.0 {
        format!("{:.1} TB", gb / 1000.0)
    } else {
        format!("{gb:.0} GB")
    }
}

fn mhz(v: u32) -> String {
    if v >= 1000 {
        format!("{:.2} GHz", f64::from(v) / 1000.0)
    } else {
        format!("{v} MHz")
    }
}

fn link_speed(bps: u64) -> String {
    match bps {
        b if b >= 1_000_000_000 => format!("{} Gbps", trim(b as f64 / 1e9)),
        b if b >= 1_000_000 => format!("{} Mbps", trim(b as f64 / 1e6)),
        b => format!("{} Kbps", trim(b as f64 / 1e3)),
    }
}

fn trim(v: f64) -> String {
    let s = format!("{v:.1}");
    s.strip_suffix(".0").map(str::to_string).unwrap_or(s)
}

pub fn uptime(d: Duration) -> String {
    let minutes = d.as_secs() / 60;
    let (days, hours, minutes) = (minutes / 1440, minutes / 60 % 24, minutes % 60);
    match days {
        0 => tr!("{} h {} min", hours, minutes),
        _ => tr!("{} d {} h", days, hours),
    }
}

fn cache_line(caches: &[Cache], level: u8, kind: CacheKind) -> Option<String> {
    let parts: Vec<String> = caches
        .iter()
        .filter(|c| c.level == level && c.kind == kind)
        .map(|c| {
            let size = if c.size_bytes >= 1024 * 1024 {
                format!("{} MB", trim(f64::from(c.size_bytes) / (1024.0 * 1024.0)))
            } else {
                format!("{} KB", c.size_bytes / 1024)
            };
            let ways = if c.ways == 0xFF {
                tr!("fully associative").to_string()
            } else {
                tr!("{}-way", c.ways)
            };
            if c.count > 1 {
                format!("{} × {size}, {ways}", c.count)
            } else {
                format!("{size}, {ways}")
            }
        })
        .collect();
    (!parts.is_empty()).then(|| parts.join("\n"))
}

fn processes(data: &Data, sort: ProcessSort) -> SpecGroup {
    let top = top_by(&data.latest.processes, sort, TOP_PROCESSES);
    let rows: Vec<(String, String)> =
        if let Some(reason) = data.latest.errors.get(&SourceId::Processes) {
            vec![(reason.clone(), String::new())]
        } else if data.latest.processes.is_empty() {
            vec![(tr!("Collecting…").into(), String::new())]
        } else {
            top.into_iter()
                .map(|p: ProcessUsage| {
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
    SpecGroup {
        tint: Color::default(),
        icon: "\u{E9F9}".into(),
        title: tr!("Top processes").into(),
        subtitle: SharedString::default(),
        badge: SharedString::default(),
        specs: model(
            rows.into_iter()
                .map(|(label, value)| Spec {
                    label: label.into(),
                    value: value.into(),
                })
                .collect(),
        ),
        meters: model(Vec::new()),
    }
}

fn core_values(data: &Data, key: fn(u16) -> MetricKey) -> Vec<f64> {
    (0u16..).map_while(|i| data.get(key(i))).collect()
}

fn installed_memory(data: &Data) -> (u64, Option<&'static str>, Option<u32>) {
    let modules = data
        .info
        .firmware
        .memory
        .modules
        .iter()
        .filter(|m| m.installed());
    let total = modules.clone().map(|m| m.size_bytes).sum();
    let kind = modules.clone().find_map(|m| m.kind);
    let speed = modules
        .filter_map(|m| m.configured_mts.or(m.rated_mts))
        .max();
    (total, kind, speed)
}

pub fn nav(data: &Data) -> Vec<NavEntry> {
    let line = |key: MetricKey, kind: ItemKind| {
        data.history
            .get(key)
            .filter(|h| h.max().is_some())
            .map(|h| (graph::recent(h, SPARKLINE, key), data.color(kind)))
    };
    Page::ALL
        .into_iter()
        .map(|page| {
            let (value, spark) = match page {
                Page::Cpu => (
                    join([
                        data.get(MetricKey::CpuTotal)
                            .map(|_| data.fmt(MetricKey::CpuTotal)),
                        data.get(MetricKey::CpuTempC)
                            .map(|_| data.fmt(MetricKey::CpuTempC)),
                    ]),
                    line(MetricKey::CpuTotal, ItemKind::Cpu),
                ),
                Page::Memory => (
                    join([
                        data.get(MetricKey::MemUsedPercent)
                            .map(|_| data.fmt(MetricKey::MemUsedPercent)),
                        data.get(MetricKey::MemUsedBytes)
                            .map(|_| data.fmt(MetricKey::MemUsedBytes)),
                    ]),
                    line(MetricKey::MemUsedPercent, ItemKind::Ram),
                ),
                Page::Gpu => (
                    join([
                        data.get(MetricKey::GpuUtil)
                            .map(|_| data.fmt(MetricKey::GpuUtil)),
                        data.get(MetricKey::GpuTempC)
                            .map(|_| data.fmt(MetricKey::GpuTempC)),
                    ]),
                    line(MetricKey::GpuUtil, ItemKind::Gpu),
                ),
                Page::Storage => (
                    data.get(MetricKey::DiskActivePercent)
                        .map(|_| tr!("{} active", data.fmt(MetricKey::DiskActivePercent)))
                        .unwrap_or_default(),
                    line(MetricKey::DiskActivePercent, ItemKind::Disk),
                ),
                Page::Network => (
                    data.get(MetricKey::NetDownBps)
                        .map(|_| format!("↓ {}", data.fmt(MetricKey::NetDownBps)))
                        .unwrap_or_default(),
                    line(MetricKey::NetDownBps, ItemKind::Network),
                ),
                _ => (String::new(), None),
            };
            let (paths, color) = spark.unwrap_or_default();
            NavEntry {
                icon: page.icon().into(),
                label: page.label().into(),
                value: value.into(),
                color,
                line: paths.line.into(),
                viewbox: paths.viewbox,
            }
        })
        .collect()
}

fn join<const N: usize>(parts: [Option<String>; N]) -> String {
    parts.into_iter().flatten().collect::<Vec<_>>().join(" · ")
}

pub fn build(page: Page, data: &Data) -> View {
    let mut view = match page {
        Page::Overview => overview(data),
        Page::Cpu => cpu(data),
        Page::Memory => memory(data),
        Page::Gpu => gpu(data),
        Page::Storage => storage(data),
        Page::Network => network(data),
        Page::System => system(data),
        Page::Sensors => sensors(data),
    };
    view.stats.retain(|s| !s.value.is_empty());
    view.groups
        .retain(|g| g.specs.row_count() > 0 || g.meters.row_count() > 0);
    let tint = match page {
        Page::Cpu => data.color(ItemKind::Cpu),
        Page::Memory => data.color(ItemKind::Ram),
        Page::Gpu => data.color(ItemKind::Gpu),
        Page::Storage => data.color(ItemKind::Disk),
        Page::Network => series_color(data.config, data.accent, ItemKind::Network, CellPart::Down),
        _ => data.accent.unwrap_or_else(|| data.color(ItemKind::Cpu)),
    };
    for g in &mut view.groups {
        g.tint = tint;
    }
    view
}

fn overview(data: &Data) -> View {
    let info = data.info;
    let os = &info.os;
    let mut graphs = Vec::new();
    for (title, key, kind) in [
        (tr!("Processor"), MetricKey::CpuTotal, ItemKind::Cpu),
        (tr!("Memory"), MetricKey::MemUsedPercent, ItemKind::Ram),
        (tr!("Graphics"), MetricKey::GpuUtil, ItemKind::Gpu),
        (tr!("Storage"), MetricKey::DiskActivePercent, ItemKind::Disk),
    ] {
        if data.has_history(key) {
            graphs.push(data.graph(title, key, kind, String::new()));
        }
    }
    if data.has_history(MetricKey::NetDownBps) {
        graphs.push(network_graph(data));
    }
    let (ram, ram_kind, ram_speed) = installed_memory(data);
    let ram_total = data
        .get(MetricKey::MemTotalBytes)
        .map(|v| format_value(v, Unit::Bytes));
    let memory = match (ram, ram_kind, ram_speed) {
        (0, ..) => ram_total,
        (bytes_total, kind, speed) => Some(join([
            Some(format!("{:.0} GB", bytes_total as f64 / GB)),
            kind.map(|k| match speed {
                Some(s) => format!("{k}-{s}"),
                None => k.to_string(),
            }),
        ])),
    };
    let board = info.firmware.board.as_ref();
    let storage_total: u64 = info.disks.iter().map(|d| d.size_bytes).sum();
    let board_tint = Color::from_rgb_u8(0x2E, 0xC4, 0xB6);
    let windows_tint = Color::from_rgb_u8(0x3B, 0x9C, 0xFF);
    let facts = [
        (
            "\u{E950}",
            data.color(ItemKind::Cpu),
            tr!("Processor"),
            Some(info.cpu.brand.clone()),
        ),
        (
            "\u{F211}",
            data.color(ItemKind::Gpu),
            tr!("Graphics"),
            Some(
                info.gpus
                    .iter()
                    .map(|g| g.name.clone())
                    .collect::<Vec<_>>()
                    .join("\n"),
            ),
        ),
        ("\u{EEA0}", data.color(ItemKind::Ram), tr!("Memory"), memory),
        (
            "\u{E964}",
            board_tint,
            tr!("Motherboard"),
            board.map(|b| join([b.manufacturer.clone(), b.product.clone()])),
        ),
        (
            "\u{EDA2}",
            data.color(ItemKind::Disk),
            tr!("Storage"),
            (storage_total > 0).then(|| {
                tr!(
                    "{} across {} drives",
                    capacity(storage_total),
                    info.disks.len()
                )
            }),
        ),
        (
            "\u{E7F4}",
            windows_tint,
            "Windows",
            Some(join([Some(os.name.clone()), os.version.clone()])),
        ),
    ]
    .into_iter()
    .filter_map(|(icon, tint, label, value)| {
        Some(Fact {
            icon: icon.into(),
            tint,
            label: label.into(),
            value: value.filter(|v| !v.is_empty())?.into(),
        })
    })
    .collect();
    let mut stats = Vec::new();
    stats.extend(data.stat(tr!("CPU temperature"), MetricKey::CpuTempC));
    stats.extend(data.stat(tr!("CPU power"), MetricKey::CpuPowerW));
    stats.extend(data.stat(tr!("GPU temperature"), MetricKey::GpuTempC));
    stats.extend(data.stat(tr!("Ping"), MetricKey::PingMs));
    stats.push(stat(tr!("Up time"), uptime(data.uptime)));
    View {
        heading: tr!("Overview").into(),
        subheading: join([
            Some(os.name.clone()),
            os.version.clone(),
            os.build.as_ref().map(|b| tr!("build {}", b)),
        ]),
        graphs,
        stats,
        facts,
        ..View::default()
    }
}

fn cpu(data: &Data) -> View {
    let c = &data.info.cpu;
    let t = c.topology;
    let mut view = View {
        heading: c.brand.clone(),
        subheading: join([
            c.microarchitecture.map(str::to_string),
            Some(match (t.performance_cores, t.efficient_cores) {
                (Some(p), Some(e)) => tr!("{} cores ({}P + {}E)", t.cores, p, e),
                _ => tr!("{} cores", t.cores),
            }),
            Some(tr!("{} threads", t.threads)),
        ]),
        ..View::default()
    };
    data.service_note(&mut view);
    view.graphs.push(
        data.graph(
            tr!("Utilization"),
            MetricKey::CpuTotal,
            ItemKind::Cpu,
            data.get(MetricKey::CpuClockMhz)
                .map(|_| tr!("Running at {}", data.fmt(MetricKey::CpuClockMhz)))
                .unwrap_or_default(),
        ),
    );
    if data.has_history(MetricKey::CpuTempC) {
        let hottest = core_values(data, MetricKey::CpuCoreTempC)
            .into_iter()
            .reduce(f64::max);
        view.graphs.push(
            data.graph(
                tr!("Temperature"),
                MetricKey::CpuTempC,
                ItemKind::CpuTemp,
                hottest
                    .map(|h| tr!("Hottest core {}", format_value(h, Unit::Celsius)))
                    .unwrap_or_default(),
            ),
        );
    }
    if data.has_history(MetricKey::CpuPowerW) {
        view.graphs.push(data.graph(
            tr!("Package power"),
            MetricKey::CpuPowerW,
            ItemKind::CpuPower,
            String::new(),
        ));
    }
    view.stats
        .extend(data.stat(tr!("Speed"), MetricKey::CpuClockMhz));
    let bus_mhz = data
        .info
        .firmware
        .processor
        .as_ref()
        .and_then(|p| p.bus_mhz)
        .map_or(100.0, f64::from);
    if let Some(clock) = data.get(MetricKey::CpuClockMhz) {
        let ratio = (clock / bus_mhz * 10.0).round() / 10.0;
        view.stats
            .push(stat(tr!("Multiplier"), format_value(ratio, Unit::Ratio)));
    }
    view.stats
        .extend(data.stat(tr!("Ring clock"), MetricKey::RingClockMhz));
    view.stats
        .extend(data.stat(tr!("Temperature"), MetricKey::CpuTempC));
    view.stats
        .extend(data.stat(tr!("Package power"), MetricKey::CpuPowerW));
    view.stats
        .extend(data.stat(tr!("Cores power"), MetricKey::CpuCoresPowerW));
    view.stats
        .extend(data.stat(tr!("Graphics power"), MetricKey::CpuGraphicsPowerW));
    view.stats
        .extend(data.stat(tr!("Memory power"), MetricKey::CpuDramPowerW));
    if let Some(peak) = core_values(data, MetricKey::CpuCoreVolts)
        .into_iter()
        .reduce(f64::max)
    {
        view.stats
            .push(stat(tr!("Core voltage"), format_value(peak, Unit::Volts)));
    }
    if let Some(throttling) = data.get(MetricKey::CpuThrottling) {
        view.stats.push(Stat {
            warn: throttling > 0.0,
            ..stat(
                tr!("Thermal throttling"),
                if throttling > 0.0 {
                    tr!("Yes")
                } else {
                    tr!("No")
                },
            )
        });
    }
    view.stats.push(stat(
        tr!("Cores / threads"),
        format!("{} / {}", t.cores, t.threads),
    ));

    if let Some((cores, cells, threads)) = cores(data) {
        view.cores = cores;
        view.core_cells = cells;
        view.threads = threads;
    }
    if data.prefs.cores == CoreView::Graphs {
        view.thread_graphs = (0..view.threads.len() as u16)
            .map(|i| {
                let title = format!("CPU {i}");
                data.graph(&title, MetricKey::CpuCore(i), ItemKind::Cpu, String::new())
            })
            .collect();
    }
    let firmware_cpu = data.info.firmware.processor.as_ref();
    view.groups.push(group(
        "\u{E950}",
        tr!("Processor"),
        "",
        vec![
            (tr!("Vendor"), Some(c.vendor.clone())),
            (
                tr!("Microarchitecture"),
                c.microarchitecture.map(str::to_string),
            ),
            (
                tr!("Family · model · stepping"),
                Some(format!("{:X}h · {:X}h · {}", c.family, c.model, c.stepping)),
            ),
            (tr!("Socket"), firmware_cpu.and_then(|p| p.socket.clone())),
            (tr!("Base clock"), c.base_mhz.map(mhz)),
            (
                tr!("Bus clock"),
                firmware_cpu.and_then(|p| p.bus_mhz).map(mhz),
            ),
            (
                tr!("Multiplier range"),
                data.get(MetricKey::CpuMinRatio)
                    .zip(data.get(MetricKey::CpuMaxRatio))
                    .map(|(lo, hi)| {
                        format!(
                            "{} – {}",
                            format_value(lo, Unit::Ratio),
                            format_value(hi, Unit::Ratio)
                        )
                    }),
            ),
            ("TDP", data.get(MetricKey::CpuTdpW).map(watts)),
            (tr!("Power limits"), power_limits(data)),
            (tr!("Microcode"), c.microcode.map(|m| format!("0x{m:X}"))),
            (
                tr!("Packages"),
                (t.packages > 1).then(|| t.packages.to_string()),
            ),
            (tr!("Instructions"), Some(c.features.join(", "))),
        ],
    ));
    view.groups.push(group(
        "\u{E8F1}",
        tr!("Caches"),
        "",
        vec![
            (tr!("L1 data"), cache_line(&c.caches, 1, CacheKind::Data)),
            (
                tr!("L1 instruction"),
                cache_line(&c.caches, 1, CacheKind::Instruction),
            ),
            ("L2", cache_line(&c.caches, 2, CacheKind::Unified)),
            ("L3", cache_line(&c.caches, 3, CacheKind::Unified)),
        ],
    ));
    view.groups.push(processes(data, ProcessSort::Cpu));
    view
}

/// The logical processors of each physical core. Windows numbers a core's
/// threads together, performance cores first.
fn thread_ranges(t: Topology, threads: usize) -> Vec<Range<usize>> {
    let cores = t.cores as usize;
    let counts = match (t.performance_cores, t.efficient_cores) {
        _ if cores == 0 => Vec::new(),
        (Some(p), Some(e)) => {
            let (p, e) = (p as usize, e as usize);
            let per = threads.saturating_sub(e) / p.max(1);
            [vec![per; p], vec![1; e]].concat()
        }
        _ => vec![threads / cores; cores],
    };
    if counts.contains(&0) || counts.iter().sum::<usize>() != threads {
        return (0..threads).map(|i| i..i + 1).collect();
    }
    let mut start = 0;
    counts
        .into_iter()
        .map(|n| {
            start += n;
            start - n..start
        })
        .collect()
}

fn cores(data: &Data) -> Option<(Cores, Vec<Core>, Vec<f32>)> {
    let loads = core_values(data, MetricKey::CpuCore);
    if loads.is_empty() {
        return None;
    }
    let clocks = core_values(data, MetricKey::CpuCoreClockMhz);
    let temps = core_values(data, MetricKey::CpuCoreTempC);
    let t = data.info.cpu.topology;
    let ranges = thread_ranges(t, loads.len());
    let top_clock = (0..ranges.len() as u16)
        .filter_map(|i| data.extremes.get(MetricKey::CpuCoreClockMhz(i)))
        .map(|(_, hi)| hi)
        .chain(clocks.iter().copied())
        .fold(1000.0, f64::max);
    let level = |j: usize| loads.get(j).copied().unwrap_or(0.0) / 100.0;
    let cells = ranges
        .iter()
        .enumerate()
        .map(|(i, r)| {
            let load_a = level(r.start);
            let load_b = r.clone().skip(1).map(level).fold(0.0, f64::max);
            let load = load_a.max(load_b);
            let clock = clocks.get(i).copied();
            let temp = temps.get(i).copied();
            Core {
                label: core_label(data, i).into(),
                load_a: load_a as f32,
                load_b: load_b as f32,
                two: r.len() > 1,
                load: load as f32,
                load_text: format_value(load * 100.0, Unit::Percent).into(),
                clock: clock.map_or(0.0, |c| (c / top_clock) as f32),
                clock_text: clock
                    .map(|c| format!("{:.1}", c / 1000.0))
                    .unwrap_or_default()
                    .into(),
                temp: temp.map_or(0.0, |t| (t / 100.0) as f32),
                temp_text: temp
                    .map(|t| format!("{:.0}", temperature(t).0))
                    .unwrap_or_default()
                    .into(),
                heat: temp.map_or_else(Color::default, graph::heat),
                load_arc: graph::arc(load.max(0.02), 32.0, 27.0).into(),
                temp_arc: temp
                    .map(|t| graph::arc(t / 100.0, 32.0, 19.0))
                    .unwrap_or_default()
                    .into(),
            }
        })
        .collect::<Vec<_>>();
    let hybrid = t.performance_cores.is_some() && t.efficient_cores.is_some_and(|e| e > 0);
    let split = t
        .performance_cores
        .filter(|&p| hybrid && (p as usize) < cells.len())
        .map_or(-1, |p| p as i32);
    let note = match (ranges.iter().any(|r| r.len() > 1), hybrid) {
        (false, _) => "",
        (true, true) => tr!("P-cores show both threads' load"),
        (true, false) => tr!("Each core shows both threads' load"),
    };
    let fastest = clocks.iter().copied().reduce(f64::max);
    let hottest = temps.iter().copied().reduce(f64::max);
    let info = Cores {
        show: true,
        mode: CoreView::ALL
            .iter()
            .position(|&v| v == data.prefs.cores)
            .unwrap_or(0) as i32,
        has_clock: !clocks.is_empty(),
        has_temp: !temps.is_empty(),
        temp_unit: temperature(0.0).1.into(),
        split,
        load_color: data.color(ItemKind::Cpu),
        clock_color: data.color(ItemKind::CpuPower),
        note: note.into(),
        load_caption: tr!("{} threads", loads.len()).into(),
        clock_caption: fastest
            .map(|c| tr!("fastest {} GHz", format!("{:.1}", c / 1000.0)))
            .unwrap_or_default()
            .into(),
        temp_caption: hottest
            .map(|t| tr!("hottest {}", format_value(t, Unit::Celsius)))
            .unwrap_or_default()
            .into(),
    };
    let threads = loads.iter().map(|&v| (v / 100.0) as f32).collect();
    Some((info, cells, threads))
}

/// Hybrid parts list their performance cores first.
fn core_label(data: &Data, index: usize) -> String {
    match data.info.cpu.topology.performance_cores {
        Some(p) if (index as u32) < p => format!("P{index}"),
        Some(p) => format!("E{}", index as u32 - p),
        None => index.to_string(),
    }
}

fn memory(data: &Data) -> View {
    let array = &data.info.firmware.memory;
    let (total, kind, speed) = installed_memory(data);
    let used_slots = array.modules.iter().filter(|m| m.installed()).count();
    let total_text = if total > 0 {
        format!("{:.0} GB", total as f64 / GB)
    } else {
        data.fmt(MetricKey::MemTotalBytes)
    };
    let mut view = View {
        heading: join([Some(total_text), kind.map(str::to_string)]).replace(" · ", " "),
        subheading: join([
            (array.slots > 0).then(|| tr!("{} of {} slots used", used_slots, array.slots)),
            speed.map(|s| format!("{s} MT/s")),
            array
                .max_bytes
                .map(|m| tr!("Up to {} GB", format!("{:.0}", m as f64 / GB))),
        ]),
        ..View::default()
    };
    view.feature_graph = data.graph(
        tr!("Memory in use"),
        MetricKey::MemUsedPercent,
        ItemKind::Ram,
        tr!(
            "{} of {}",
            data.fmt(MetricKey::MemUsedBytes),
            data.fmt(MetricKey::MemTotalBytes)
        ),
    );
    view.mem_usage = mem_usage(data);
    view.stats
        .extend(data.stat(tr!("In use"), MetricKey::MemUsedBytes));
    view.stats
        .extend(data.stat(tr!("Available"), MetricKey::MemAvailableBytes));
    view.stats
        .extend(data.stat(tr!("Committed"), MetricKey::CommitBytes));
    view.stats
        .extend(data.stat(tr!("Commit limit"), MetricKey::CommitLimitBytes));
    if let Some(s) = speed {
        view.stats.push(stat(tr!("Speed"), format!("{s} MT/s")));
    }
    if let Some(timings) = timings(data) {
        view.groups.push(timings);
    }
    for m in array.modules.iter().filter(|m| m.installed()) {
        let mut g = group(
            "\u{EEA0}",
            m.slot.as_deref().unwrap_or(tr!("Module")),
            m.part_number.as_deref().unwrap_or(""),
            vec![
                (tr!("Manufacturer"), m.manufacturer.clone()),
                (
                    tr!("Type"),
                    Some(
                        join([m.kind.map(str::to_string), m.form.map(str::to_string)])
                            .replace(" · ", " "),
                    ),
                ),
                (
                    tr!("Speed"),
                    match (m.configured_mts, m.rated_mts) {
                        (Some(c), Some(r)) if c != r => Some(tr!("{} MT/s (rated {})", c, r)),
                        (c, r) => c.or(r).map(|s| format!("{s} MT/s")),
                    },
                ),
                (tr!("Ranks"), m.ranks.map(|r| r.to_string())),
                (
                    tr!("Voltage"),
                    m.configured_mv
                        .map(|mv| format!("{:.2} V", f64::from(mv) / 1000.0)),
                ),
                (tr!("Bank"), m.bank.clone()),
            ],
        );
        g.badge = format!("{:.0} GB", m.size_bytes as f64 / GB).into();
        view.groups.push(g);
    }
    view.groups.push(processes(data, ProcessSort::Memory));
    view
}

fn watts(w: f64) -> String {
    format!("{w:.0} W")
}

/// The sustained and burst package power limits.
fn power_limits(data: &Data) -> Option<String> {
    let pl1 = data.get(MetricKey::CpuPowerLimit1W);
    let pl2 = data.get(MetricKey::CpuPowerLimit2W);
    let parts: Vec<String> = [("PL1", pl1), ("PL2", pl2)]
        .into_iter()
        .filter_map(|(name, w)| {
            // The largest value the register holds means no limit.
            let w = w?;
            Some(if w >= 4095.0 {
                tr!("{} unlimited", name)
            } else {
                format!("{name} {}", watts(w))
            })
        })
        .collect();
    (!parts.is_empty()).then(|| parts.join(" · "))
}

/// The memory controller's timings, as the sensor service reads them.
fn timings(data: &Data) -> Option<SpecGroup> {
    let cycles = |t: Timing| data.get(MetricKey::MemTiming(t)).map(|v| format!("{v:.0}"));
    let primary = [Timing::Cl, Timing::Rcd, Timing::Rp, Timing::Ras]
        .into_iter()
        .map(cycles)
        .collect::<Option<Vec<_>>>()
        .map(|t| t.join("-"));
    let clock = data.get(MetricKey::MemClockMhz);
    let controller = data.get(MetricKey::MemControllerClockMhz);
    let rows = vec![
        (tr!("Memory clock"), clock.map(|c| format!("{c:.0} MHz"))),
        (
            tr!("Controller clock"),
            controller.map(|mc| match clock {
                Some(c) if mc > 0.0 && (c / mc).round() >= 1.0 => {
                    tr!(
                        "{} MHz (gear {})",
                        format!("{mc:.0}"),
                        format!("{:.0}", (c / mc).round())
                    )
                }
                _ => format!("{mc:.0} MHz"),
            }),
        ),
        ("CL-tRCD-tRP-tRAS", primary),
        ("tRC", cycles(Timing::Rc)),
        (
            tr!("Command rate"),
            data.get(MetricKey::MemTiming(Timing::CommandRate))
                .map(|cr| format!("{cr:.0}T")),
        ),
    ];
    rows.iter()
        .any(|(_, v)| v.is_some())
        .then(|| group("\u{E916}", tr!("Timings"), "", rows))
}

/// `PCIe 4.0 ×16`.
fn pcie(generation: u32, width: u32) -> String {
    format!("PCIe {generation}.0 ×{width}")
}

fn link_rows(link: Link) -> [(&'static str, Option<String>); 2] {
    [
        (
            tr!("Link"),
            Some(match pci::transfer_rate(link.generation) {
                Some(rate) => format!("{} ({rate})", pcie(link.generation, link.width)),
                None => pcie(link.generation, link.width),
            }),
        ),
        (
            tr!("Max link"),
            (link.max_generation > 0).then(|| pcie(link.max_generation, link.max_width)),
        ),
    ]
}

fn mem_usage(data: &Data) -> MemUsage {
    let ratio = |used: MetricKey, total: MetricKey| {
        data.get(used)
            .zip(data.get(total))
            .map_or(0.0, |(u, t)| u / t.max(1.0))
    };
    let physical = ratio(MetricKey::MemUsedBytes, MetricKey::MemTotalBytes);
    let committed = ratio(MetricKey::CommitBytes, MetricKey::CommitLimitBytes);
    let color = data.color(ItemKind::Ram);
    let of = |a: MetricKey, b: MetricKey| format!("{} / {}", data.fmt(a), data.fmt(b));
    MemUsage {
        show: true,
        mode: MemoryView::ALL
            .iter()
            .position(|&v| v == data.prefs.memory)
            .unwrap_or(0) as i32,
        physical: physical as f32,
        committed: committed as f32,
        physical_arc: graph::arc(physical, 60.0, 52.0).into(),
        commit_arc: graph::arc(committed, 60.0, 38.0).into(),
        percent: data.fmt(MetricKey::MemUsedPercent).into(),
        used: data.fmt(MetricKey::MemUsedBytes).into(),
        total: data.fmt(MetricKey::MemTotalBytes).into(),
        physical_text: of(MetricKey::MemUsedBytes, MetricKey::MemTotalBytes).into(),
        commit_text: of(MetricKey::CommitBytes, MetricKey::CommitLimitBytes).into(),
        available: data.fmt(MetricKey::MemAvailableBytes).into(),
        color,
        commit_color: mix(color, Color::from_rgb_u8(255, 255, 255), 0.45),
    }
}

fn mix(a: Color, b: Color, k: f32) -> Color {
    let m = |x: u8, y: u8| (f32::from(x) + (f32::from(y) - f32::from(x)) * k).round() as u8;
    Color::from_rgb_u8(
        m(a.red(), b.red()),
        m(a.green(), b.green()),
        m(a.blue(), b.blue()),
    )
}

/// Translated where shown.
const ENGINES: [(GpuEngineKind, &str); 5] = [
    (GpuEngineKind::ThreeD, tr_noop!("3D")),
    (GpuEngineKind::Compute, tr_noop!("Compute")),
    (GpuEngineKind::VideoDecode, tr_noop!("Video decode")),
    (GpuEngineKind::VideoEncode, tr_noop!("Video encode")),
    (GpuEngineKind::Copy, tr_noop!("Copy")),
];

fn gpu(data: &Data) -> View {
    let gpus = &data.info.gpus;
    let main = gpus.first();
    let mut view = View {
        heading: main.map_or(tr!("Graphics").into(), |g| g.name.clone()),
        subheading: main
            .map(|g| {
                join([
                    g.vendor.map(str::to_string),
                    (!g.integrated()).then(|| tr!("{} dedicated", bytes(g.dedicated_bytes))),
                    g.driver.as_ref().map(|d| tr!("Driver {}", d)),
                ])
            })
            .unwrap_or_default(),
        ..View::default()
    };
    view.graphs.push(
        data.graph(
            tr!("Utilization"),
            MetricKey::GpuUtil,
            ItemKind::Gpu,
            data.get(MetricKey::GpuDedicatedBytes)
                .map(|v| tr!("{} dedicated memory in use", format_value(v, Unit::Bytes)))
                .unwrap_or_default(),
        ),
    );
    if data.has_history(MetricKey::GpuTempC) {
        let caption = data
            .get(MetricKey::GpuHotSpotC)
            .map(|v| tr!("Hot spot {}", format_value(v, Unit::Celsius)))
            .unwrap_or_default();
        view.graphs.push(data.graph(
            tr!("Temperature"),
            MetricKey::GpuTempC,
            ItemKind::GpuTemp,
            caption,
        ));
    }
    view.stats
        .extend(data.stat(tr!("Temperature"), MetricKey::GpuTempC));
    view.stats
        .extend(data.stat(tr!("Hot spot"), MetricKey::GpuHotSpotC));
    view.stats
        .extend(data.stat(tr!("Memory temperature"), MetricKey::GpuMemTempC));
    view.stats.extend(
        data.stat(tr!("Fan"), MetricKey::GpuFanRpm)
            .filter(|_| data.get(MetricKey::GpuFanRpm).is_some_and(|v| v > 0.0)),
    );
    view.stats
        .extend(data.stat(tr!("Power"), MetricKey::GpuPowerPercent));
    view.stats.extend(
        data.stat(tr!("Memory clock"), MetricKey::GpuMemClockMhz)
            .map(|mut s| {
                s.value = format!(
                    "{:.0} MHz",
                    data.get(MetricKey::GpuMemClockMhz).unwrap_or(0.0)
                )
                .into();
                s
            }),
    );
    let mut engines = group("\u{E9D9}", tr!("Engines"), "", Vec::new());
    engines.meters = model(
        ENGINES
            .iter()
            .filter_map(|&(kind, label)| {
                let v = data.get(MetricKey::GpuEngine(kind))?;
                Some(meter(
                    l10n::translate(label),
                    v / 100.0,
                    format_value(v, Unit::Percent),
                ))
            })
            .collect(),
    );
    let mut memory = group("\u{EEA0}", tr!("Graphics memory"), "", Vec::new());
    let dedicated_total =
        Some(gpus.iter().map(|g| g.dedicated_bytes).sum::<u64>()).filter(|&b| b > 0);
    let shared_total = main.map(|g| g.shared_bytes).filter(|&b| b > 0);
    let mut meters = Vec::new();
    if let Some(used) = data.get(MetricKey::GpuDedicatedBytes) {
        let total = dedicated_total.map_or(f64::NAN, |t| t as f64);
        meters.push(meter(
            tr!("Dedicated"),
            used / total,
            tr!(
                "{} of {}",
                format_value(used, Unit::Bytes),
                format_value(total, Unit::Bytes)
            ),
        ));
    }
    if let Some(used) = data.get(MetricKey::GpuSharedBytes) {
        let total = shared_total.map_or(f64::NAN, |t| t as f64);
        meters.push(meter(
            tr!("Shared"),
            used / total,
            tr!(
                "{} of {}",
                format_value(used, Unit::Bytes),
                format_value(total, Unit::Bytes)
            ),
        ));
    }
    memory.meters = model(meters);
    view.groups.push(engines);
    view.groups.push(memory);
    for g in gpus {
        let device = data.info.pci.iter().find(|d| {
            u32::from(d.ids.vendor) == g.vendor_id && u32::from(d.ids.device) == g.device_id
        });
        let mut rows = vec![
            (
                tr!("Board maker"),
                device
                    .and_then(|d| pci::board_maker(d.ids.subsystem_vendor))
                    .filter(|maker| Some(*maker) != g.vendor)
                    .map(str::to_string),
            ),
            (
                tr!("Revision"),
                device.map(|d| format!("{:02X}", d.ids.revision)),
            ),
        ];
        rows.extend(device.and_then(|d| d.link).into_iter().flat_map(link_rows));
        let mut card = group(
            "\u{F211}",
            &g.name,
            g.vendor.unwrap_or(""),
            vec![
                (
                    tr!("Dedicated memory"),
                    (g.dedicated_bytes > 0).then(|| bytes(g.dedicated_bytes)),
                ),
                (
                    tr!("Shared memory"),
                    (g.shared_bytes > 0).then(|| bytes(g.shared_bytes)),
                ),
                (tr!("Driver"), g.driver.clone()),
                (
                    tr!("Device ID"),
                    Some(format!("{:04X}:{:04X}", g.vendor_id, g.device_id)),
                ),
            ]
            .into_iter()
            .chain(rows)
            .collect(),
        );
        card.badge = if g.integrated() {
            tr!("Integrated")
        } else {
            tr!("Discrete")
        }
        .into();
        view.groups.push(card);
    }
    view.groups.push(processes(data, ProcessSort::Gpu));
    view
}

fn network_graph(data: &Data) -> Graph {
    graph::graph(
        data.history,
        tr!("Network"),
        format!("↓ {}", data.fmt(MetricKey::NetDownBps)),
        String::new(),
        Series {
            key: MetricKey::NetDownBps,
            color: series_color(data.config, data.accent, ItemKind::Network, CellPart::Down),
            legend: &tr!("Download {}", data.fmt(MetricKey::NetDownBps)),
        },
        Some(Series {
            key: MetricKey::NetUpBps,
            color: series_color(data.config, data.accent, ItemKind::Network, CellPart::Up),
            legend: &tr!("Upload {}", data.fmt(MetricKey::NetUpBps)),
        }),
    )
}

fn disk_card(data: &Data, d: &DiskInfo) -> SpecGroup {
    let temp = data.get(MetricKey::DiskTempC(d.number as u8));
    let mut card = group(
        "\u{EDA2}",
        &d.model,
        &join([
            Some(d.kind().to_string()),
            (d.size_bytes > 0).then(|| capacity(d.size_bytes)),
        ]),
        vec![
            (tr!("Interface"), d.bus.map(str::to_string)),
            (tr!("Firmware"), d.firmware.clone()),
            (
                tr!("Temperature"),
                temp.map(|t| format_value(t, Unit::Celsius)),
            ),
            (
                tr!("Temperature limits"),
                match (d.warning_c, d.critical_c) {
                    (Some(w), Some(c)) => Some(tr!("Warning {}°C · critical {}°C", w, c)),
                    (Some(w), None) => Some(tr!("Warning {}°C", w)),
                    (None, Some(c)) => Some(tr!("Critical {}°C", c)),
                    (None, None) => None,
                },
            ),
            (
                tr!("Removable"),
                d.removable.then(|| tr!("Yes").to_string()),
            ),
        ],
    );
    card.badge = d
        .letters
        .iter()
        .map(|l| format!("{l}:"))
        .collect::<Vec<_>>()
        .join(" ")
        .into();
    card.meters = model(
        d.letters
            .iter()
            .filter_map(|&letter| {
                let letter = letter as u8;
                let free = data.get(MetricKey::VolumeFreeBytes(letter))?;
                let total = data.get(MetricKey::VolumeTotalBytes(letter))?;
                let active = data
                    .get(MetricKey::DriveActivePercent(letter))
                    .map(|a| format!(" · {}", tr!("{} active", format_value(a, Unit::Percent))))
                    .unwrap_or_default();
                Some(meter(
                    format!("{}:", letter as char),
                    1.0 - free / total.max(1.0),
                    format!(
                        "{}{active}",
                        tr!(
                            "{} free of {}",
                            format_value(free, Unit::Bytes),
                            format_value(total, Unit::Bytes)
                        )
                    ),
                ))
            })
            .collect(),
    );
    card
}

fn storage(data: &Data) -> View {
    let disks = &data.info.disks;
    let total: u64 = disks.iter().map(|d| d.size_bytes).sum();
    let mut view = View {
        heading: tr!("Storage").into(),
        subheading: join([
            Some(if disks.len() == 1 {
                tr!("1 drive").to_string()
            } else {
                tr!("{} drives", disks.len())
            }),
            (total > 0).then(|| tr!("{} in total", capacity(total))),
        ]),
        ..View::default()
    };
    view.graphs.push(data.graph(
        tr!("Activity"),
        MetricKey::DiskActivePercent,
        ItemKind::Disk,
        String::new(),
    ));
    let color = data.color(ItemKind::Disk);
    view.graphs.push(graph::graph(
        data.history,
        tr!("Transfer rate"),
        format_value(
            data.get(MetricKey::DiskReadBps).unwrap_or(0.0)
                + data.get(MetricKey::DiskWriteBps).unwrap_or(0.0),
            Unit::BytesPerSec,
        ),
        String::new(),
        Series {
            key: MetricKey::DiskReadBps,
            color,
            legend: &tr!("Read {}", data.fmt(MetricKey::DiskReadBps)),
        },
        Some(Series {
            key: MetricKey::DiskWriteBps,
            color: color.brighter(0.6),
            legend: &tr!("Write {}", data.fmt(MetricKey::DiskWriteBps)),
        }),
    ));
    view.stats
        .extend(data.stat(tr!("Active"), MetricKey::DiskActivePercent));
    view.stats
        .extend(data.stat(tr!("Read"), MetricKey::DiskReadBps));
    view.stats
        .extend(data.stat(tr!("Write"), MetricKey::DiskWriteBps));
    if let Some(hottest) = disks
        .iter()
        .filter_map(|d| data.get(MetricKey::DiskTempC(d.number as u8)))
        .reduce(f64::max)
    {
        view.stats.push(stat(
            tr!("Hottest drive"),
            format_value(hottest, Unit::Celsius),
        ));
    }
    view.groups = disks.iter().map(|d| disk_card(data, d)).collect();
    view
}

fn network(data: &Data) -> View {
    let adapters = &data.info.network;
    let main = adapters.first();
    let mut view = View {
        heading: main.map_or(tr!("Network").into(), |a| a.name.clone()),
        subheading: main
            .map(|a| join([Some(a.description.clone()), a.link_bps.map(link_speed)]))
            .unwrap_or_default(),
        ..View::default()
    };
    view.graphs.push(network_graph(data));
    if data.has_history(MetricKey::PingMs) {
        view.graphs.push(data.graph(
            tr!("Latency"),
            MetricKey::PingMs,
            ItemKind::Ping,
            tr!(
                "{} · {} loss",
                data.config.ping.host,
                data.fmt(MetricKey::PingLossPercent)
            ),
        ));
    }
    view.stats
        .extend(data.stat(tr!("Download"), MetricKey::NetDownBps));
    view.stats
        .extend(data.stat(tr!("Upload"), MetricKey::NetUpBps));
    view.stats.extend(data.stat(tr!("Ping"), MetricKey::PingMs));
    view.stats
        .extend(data.stat(tr!("Packet loss"), MetricKey::PingLossPercent));
    for a in adapters {
        let rate = data
            .latest
            .adapters
            .iter()
            .find(|r| r.name == pdh_instance_name(&a.description));
        let ipv6: Vec<String> = a
            .ipv6
            .iter()
            .filter(|ip| !ip.is_unicast_link_local())
            .map(|ip| ip.to_string())
            .collect();
        let mut card = group(
            if a.wireless { "\u{E701}" } else { "\u{E839}" },
            &a.name,
            &a.description,
            vec![
                (
                    tr!("Traffic"),
                    rate.map(|r| {
                        format!(
                            "↓ {}  ↑ {}",
                            format_value(r.down_bps, Unit::NetworkRate),
                            format_value(r.up_bps, Unit::NetworkRate)
                        )
                    }),
                ),
                (tr!("Link speed"), a.link_bps.map(link_speed)),
                (
                    "IPv4",
                    Some(
                        a.ipv4
                            .iter()
                            .map(|ip| ip.to_string())
                            .collect::<Vec<_>>()
                            .join("\n"),
                    ),
                ),
                ("IPv6", Some(ipv6.join("\n"))),
                (tr!("Gateway"), a.gateway.map(|g| g.to_string())),
                (tr!("MAC address"), a.mac.clone()),
            ],
        );
        card.badge = if a.wireless {
            tr!("Wi-Fi")
        } else {
            tr!("Ethernet")
        }
        .into();
        view.groups.push(card);
    }
    view
}

fn system(data: &Data) -> View {
    let fw = &data.info.firmware;
    let os = &data.info.os;
    let board = fw.board.clone().unwrap_or_default();
    let product = fw.system.clone().unwrap_or_default();
    let bios = fw.bios.clone().unwrap_or_default();
    let mut groups = vec![group(
        "\u{E964}",
        tr!("Motherboard"),
        "",
        vec![
            (tr!("Manufacturer"), board.manufacturer.clone()),
            (tr!("Model"), board.product.clone()),
            (tr!("Revision"), board.version),
            (tr!("Chipset"), pci::chipset(&data.info.pci)),
            (
                tr!("Chipset temperature"),
                data.get(MetricKey::ChipsetTempC)
                    .map(|t| format_value(t, Unit::Celsius)),
            ),
            (
                tr!("CPU socket"),
                fw.processor.as_ref().and_then(|p| p.socket.clone()),
            ),
            (
                tr!("Memory slots"),
                (fw.memory.slots > 0).then(|| fw.memory.slots.to_string()),
            ),
        ],
    )];
    groups.push(group(
        "\u{E7B8}",
        "BIOS",
        "",
        vec![
            (tr!("Vendor"), bios.vendor),
            (tr!("Version"), bios.version),
            (tr!("Release date"), bios.date),
        ],
    ));
    // Prebuilt PCs name the model here; self-built ones repeat the board.
    if product.product.is_some() && product.product != board.product {
        groups.push(group(
            "\u{E7F8}",
            tr!("System"),
            "",
            vec![
                (tr!("Manufacturer"), product.manufacturer),
                (tr!("Model"), product.product),
                (tr!("Version"), product.version),
            ],
        ));
    }
    groups.push(group(
        "\u{E7F4}",
        "Windows",
        "",
        vec![
            (tr!("Edition"), Some(os.name.clone())),
            (tr!("Version"), os.version.clone()),
            (tr!("Build"), os.build.clone()),
            (tr!("Up time"), Some(uptime(data.uptime))),
        ],
    ));
    View {
        heading: board
            .product
            .clone()
            .unwrap_or_else(|| tr!("System").into()),
        subheading: board.manufacturer.unwrap_or_default(),
        groups,
        ..View::default()
    }
}

struct Hardware {
    id: String,
    icon: &'static str,
    tint: Color,
    name: String,
    summary: String,
    kinds: Vec<(&'static str, Vec<SensorRow>)>,
}

/// Readings grouped by hardware, then by kind, in the order first added.
struct Tree<'d> {
    data: &'d Data<'d>,
    hardware: Vec<Hardware>,
}

impl<'d> Tree<'d> {
    fn hardware(
        &mut self,
        id: String,
        icon: &'static str,
        tint: Color,
        name: &str,
        summary: String,
    ) {
        self.hardware.push(Hardware {
            id,
            icon,
            tint,
            name: name.into(),
            summary,
            kinds: Vec::new(),
        });
    }

    fn add(&mut self, kind: &'static str, label: impl Into<SharedString>, key: MetricKey) {
        let Some(v) = self.data.get(key) else { return };
        let (lo, hi) = self.data.extremes.get(key).unwrap_or((v, v));
        let f = |x| format_value(x, key.unit());
        let row = SensorRow {
            depth: 2,
            label: label.into(),
            value: f(v).into(),
            low: f(lo).into(),
            high: f(hi).into(),
            ..SensorRow::default()
        };
        let Some(hw) = self.hardware.last_mut() else {
            return;
        };
        match hw.kinds.iter_mut().find(|(k, _)| *k == kind) {
            Some((_, rows)) => rows.push(row),
            None => hw.kinds.push((kind, vec![row])),
        }
    }

    /// The visible rows: everything under a closed branch is left out, and
    /// hardware with no readings is skipped.
    fn rows(self, closed: &BTreeSet<String>) -> Vec<SensorRow> {
        let mut out = Vec::new();
        for hw in self.hardware.into_iter().filter(|h| !h.kinds.is_empty()) {
            let open = !closed.contains(&hw.id);
            out.push(SensorRow {
                depth: 0,
                id: hw.id.as_str().into(),
                open,
                icon: hw.icon.into(),
                tint: hw.tint,
                label: hw.name.into(),
                value: hw.summary.into(),
                ..SensorRow::default()
            });
            if !open {
                continue;
            }
            for (kind, rows) in hw.kinds {
                let id = format!("{}/{kind}", hw.id);
                let open = !closed.contains(&id);
                out.push(SensorRow {
                    depth: 1,
                    id: id.into(),
                    open,
                    label: l10n::translate(kind).into(),
                    ..SensorRow::default()
                });
                if open {
                    out.extend(rows);
                }
            }
        }
        out
    }
}

fn sensors(data: &Data) -> View {
    let mut view = View {
        heading: tr!("Sensors").into(),
        subheading: tr!(
            "Every reading, with the lowest and highest seen while this window has been open"
        )
        .into(),
        ..View::default()
    };
    data.service_note(&mut view);
    let mut tree = Tree {
        data,
        hardware: Vec::new(),
    };
    let summary = |keys: &[MetricKey]| {
        keys.iter()
            .filter(|&&k| data.get(k).is_some())
            .map(|&k| data.fmt(k))
            .collect::<Vec<_>>()
            .join(" · ")
    };

    let board = data.info.firmware.board.as_ref();
    tree.hardware(
        "board".into(),
        Page::System.icon(),
        data.color(ItemKind::ChipsetTemp),
        board
            .and_then(|b| b.product.as_deref())
            .unwrap_or(tr!("Motherboard")),
        summary(&[MetricKey::ChipsetTempC]),
    );
    tree.add(
        tr_noop!("Temperatures"),
        tr!("Chipset"),
        MetricKey::ChipsetTempC,
    );
    tree.hardware(
        "cpu".into(),
        Page::Cpu.icon(),
        data.color(ItemKind::Cpu),
        &data.info.cpu.brand,
        summary(&[MetricKey::CpuTotal, MetricKey::CpuTempC]),
    );
    let cores = data.cpu_cores().max(1) as u16;
    let core = |i: u16| tr!("Core {}", core_label(data, i as usize));
    tree.add(tr_noop!("Load"), tr!("Total"), MetricKey::CpuTotal);
    for i in 0u16.. {
        if data.get(MetricKey::CpuCore(i)).is_none() {
            break;
        }
        tree.add(tr_noop!("Load"), format!("CPU {i}"), MetricKey::CpuCore(i));
    }
    tree.add(tr_noop!("Clocks"), tr!("Average"), MetricKey::CpuClockMhz);
    tree.add(tr_noop!("Clocks"), tr!("Ring"), MetricKey::RingClockMhz);
    for i in 0..cores {
        tree.add(tr_noop!("Clocks"), core(i), MetricKey::CpuCoreClockMhz(i));
    }
    tree.add(
        tr_noop!("Temperatures"),
        tr!("Package"),
        MetricKey::CpuTempC,
    );
    for i in 0..cores {
        tree.add(
            tr_noop!("Temperatures"),
            core(i),
            MetricKey::CpuCoreTempC(i),
        );
    }
    for i in 0..cores {
        tree.add(tr_noop!("Voltages"), core(i), MetricKey::CpuCoreVolts(i));
    }
    tree.add(tr_noop!("Powers"), tr!("Package"), MetricKey::CpuPowerW);
    tree.add(tr_noop!("Powers"), tr!("Cores"), MetricKey::CpuCoresPowerW);
    tree.add(
        tr_noop!("Powers"),
        tr!("Graphics"),
        MetricKey::CpuGraphicsPowerW,
    );
    tree.add(tr_noop!("Powers"), tr!("Memory"), MetricKey::CpuDramPowerW);
    for i in 0..cores {
        tree.add(tr_noop!("Powers"), core(i), MetricKey::CpuCorePowerW(i));
    }

    let gpu_name = data
        .info
        .gpus
        .first()
        .map_or(tr!("Graphics"), |g| g.name.as_str());
    tree.hardware(
        "gpu".into(),
        Page::Gpu.icon(),
        data.color(ItemKind::Gpu),
        gpu_name,
        summary(&[MetricKey::GpuUtil, MetricKey::GpuTempC]),
    );
    tree.add(tr_noop!("Load"), tr!("Core"), MetricKey::GpuUtil);
    for (kind, label) in ENGINES {
        tree.add(
            tr_noop!("Load"),
            l10n::translate(label),
            MetricKey::GpuEngine(kind),
        );
    }
    tree.add(tr_noop!("Temperatures"), tr!("Core"), MetricKey::GpuTempC);
    tree.add(
        tr_noop!("Temperatures"),
        tr!("Hot spot"),
        MetricKey::GpuHotSpotC,
    );
    tree.add(
        tr_noop!("Temperatures"),
        tr!("Memory"),
        MetricKey::GpuMemTempC,
    );
    tree.add(tr_noop!("Clocks"), tr!("Memory"), MetricKey::GpuMemClockMhz);
    tree.add(tr_noop!("Fans"), tr!("Fan"), MetricKey::GpuFanRpm);
    tree.add(tr_noop!("Powers"), tr!("Power"), MetricKey::GpuPowerPercent);
    tree.add(
        tr_noop!("Data"),
        tr!("Dedicated memory"),
        MetricKey::GpuDedicatedBytes,
    );
    tree.add(
        tr_noop!("Data"),
        tr!("Shared memory"),
        MetricKey::GpuSharedBytes,
    );

    tree.hardware(
        "memory".into(),
        Page::Memory.icon(),
        data.color(ItemKind::Ram),
        tr!("Memory"),
        summary(&[MetricKey::MemUsedPercent]),
    );
    tree.add(tr_noop!("Load"), tr!("Physical"), MetricKey::MemUsedPercent);
    tree.add(tr_noop!("Data"), tr!("In use"), MetricKey::MemUsedBytes);
    tree.add(
        tr_noop!("Data"),
        tr!("Available"),
        MetricKey::MemAvailableBytes,
    );
    tree.add(tr_noop!("Data"), tr!("Committed"), MetricKey::CommitBytes);

    for d in &data.info.disks {
        let temp = MetricKey::DiskTempC(d.number as u8);
        let first = d
            .letters
            .first()
            .map(|&l| MetricKey::DriveActivePercent(l as u8));
        tree.hardware(
            format!("disk{}", d.number),
            Page::Storage.icon(),
            data.color(ItemKind::Disk),
            &d.model,
            summary(
                &[first, Some(temp)]
                    .into_iter()
                    .flatten()
                    .collect::<Vec<_>>(),
            ),
        );
        tree.add(tr_noop!("Temperatures"), tr!("Drive"), temp);
        for &letter in &d.letters {
            let letter = letter as u8;
            tree.add(
                tr_noop!("Load"),
                tr!("{}: activity", letter as char),
                MetricKey::DriveActivePercent(letter),
            );
            tree.add(
                tr_noop!("Load"),
                tr!("{}: used space", letter as char),
                MetricKey::DriveUsedPercent(letter),
            );
        }
    }
    tree.hardware(
        "drives".into(),
        Page::Storage.icon(),
        data.color(ItemKind::Disk),
        tr!("All drives"),
        summary(&[MetricKey::DiskActivePercent]),
    );
    tree.add(
        tr_noop!("Load"),
        tr!("Activity"),
        MetricKey::DiskActivePercent,
    );
    tree.add(tr_noop!("Throughput"), tr!("Read"), MetricKey::DiskReadBps);
    tree.add(
        tr_noop!("Throughput"),
        tr!("Write"),
        MetricKey::DiskWriteBps,
    );

    tree.hardware(
        "network".into(),
        Page::Network.icon(),
        series_color(data.config, data.accent, ItemKind::Network, CellPart::Down),
        tr!("Network"),
        data.get(MetricKey::NetDownBps)
            .map(|_| format!("↓ {}", data.fmt(MetricKey::NetDownBps)))
            .unwrap_or_default(),
    );
    tree.add(
        tr_noop!("Throughput"),
        tr!("Download"),
        MetricKey::NetDownBps,
    );
    tree.add(tr_noop!("Throughput"), tr!("Upload"), MetricKey::NetUpBps);
    tree.add(tr_noop!("Latency"), tr!("Ping"), MetricKey::PingMs);
    tree.add(
        tr_noop!("Latency"),
        tr!("Packet loss"),
        MetricKey::PingLossPercent,
    );

    view.sensors = tree.rows(&data.prefs.closed);
    view
}

#[cfg(test)]
mod tests {
    use super::*;

    static PREFS: std::sync::LazyLock<Prefs> = std::sync::LazyLock::new(Prefs::default);

    fn data<'a>(
        info: &'a HardwareInfo,
        latest: &'a Snapshot,
        history: &'a HistoryStore,
        extremes: &'a Extremes,
        config: &'a Config,
    ) -> Data<'a> {
        Data {
            info,
            latest,
            history,
            extremes,
            config,
            accent: None,
            uptime: Duration::from_secs(90_061),
            service: Err(Unavailable::NotInstalled),
            prefs: &PREFS,
        }
    }

    #[test]
    fn up_time_reads_naturally() {
        assert_eq!(uptime(Duration::from_secs(90_061)), "1 d 1 h");
        assert_eq!(uptime(Duration::from_secs(3_720)), "1 h 2 min");
    }

    #[test]
    fn capacities_use_decimal_units() {
        assert_eq!(capacity(1_000_204_886_016), "1.0 TB");
        assert_eq!(capacity(512_110_190_592), "512 GB");
        assert_eq!(link_speed(1_000_000_000), "1 Gbps");
        assert_eq!(link_speed(866_700_000), "866.7 Mbps");
    }

    #[test]
    fn caches_group_by_level_and_count() {
        let caches = [
            Cache {
                level: 1,
                kind: CacheKind::Data,
                size_bytes: 48 * 1024,
                ways: 12,
                count: 8,
            },
            Cache {
                level: 3,
                kind: CacheKind::Unified,
                size_bytes: 30 * 1024 * 1024,
                ways: 12,
                count: 1,
            },
        ];
        assert_eq!(
            cache_line(&caches, 1, CacheKind::Data).as_deref(),
            Some("8 × 48 KB, 12-way")
        );
        assert_eq!(
            cache_line(&caches, 3, CacheKind::Unified).as_deref(),
            Some("30 MB, 12-way")
        );
        assert_eq!(cache_line(&caches, 2, CacheKind::Unified), None);
    }

    #[test]
    fn hybrid_cores_are_labelled_by_kind() {
        let mut info = HardwareInfo::default();
        info.cpu.topology = Topology {
            cores: 4,
            threads: 6,
            packages: 1,
            performance_cores: Some(2),
            efficient_cores: Some(2),
        };
        let (s, h, e, c) = (
            Snapshot::default(),
            HistoryStore::new(5),
            Extremes::default(),
            Config::default(),
        );
        let d = data(&info, &s, &h, &e, &c);
        assert_eq!(core_label(&d, 1), "P1");
        assert_eq!(core_label(&d, 2), "E0");
    }

    #[test]
    fn the_cpu_page_explains_missing_sensors_and_lists_cores() {
        let info = HardwareInfo::default();
        let mut s = Snapshot::default();
        s.set(MetricKey::CpuTotal, 30.0);
        s.set(MetricKey::CpuCore(0), 10.0);
        s.set(MetricKey::CpuCore(1), 50.0);
        let mut h = HistoryStore::new(5);
        h.record(&s);
        let (e, c) = (Extremes::default(), Config::default());
        let view = build(Page::Cpu, &data(&info, &s, &h, &e, &c));
        assert!(view.note.contains("sensor service"));
        assert_eq!(view.graphs[0].value, "30%");
        assert!(view.cores.show);
        assert_eq!(view.threads, [0.1, 0.5]);
        assert_eq!(view.core_cells[1].load_text, "50%");
        assert!(
            view.thread_graphs.is_empty(),
            "only the graphs view draws them"
        );

        let prefs = Prefs {
            cores: CoreView::Graphs,
            ..Prefs::default()
        };
        let view = build(
            Page::Cpu,
            &Data {
                prefs: &prefs,
                ..data(&info, &s, &h, &e, &c)
            },
        );
        let titles: Vec<_> = view
            .thread_graphs
            .iter()
            .map(|g| g.title.as_str())
            .collect();
        assert_eq!(titles, ["CPU 0", "CPU 1"]);
        assert_eq!(view.thread_graphs[1].value, "50%");
        assert!(!view.thread_graphs[1].line.is_empty());
    }

    #[test]
    fn threads_pair_up_with_their_cores() {
        let hybrid = Topology {
            cores: 3,
            threads: 5,
            packages: 1,
            performance_cores: Some(2),
            efficient_cores: Some(1),
        };
        assert_eq!(thread_ranges(hybrid, 5), [0..2, 2..4, 4..5]);
        let smt = Topology {
            cores: 2,
            threads: 4,
            packages: 1,
            performance_cores: None,
            efficient_cores: None,
        };
        assert_eq!(thread_ranges(smt, 4), [0..2, 2..4]);
        assert_eq!(thread_ranges(smt, 3), [0..1, 1..2, 2..3], "counts disagree");
        assert_eq!(thread_ranges(Topology::default(), 2), [0..1, 1..2]);
    }

    #[test]
    fn sensor_rows_carry_extremes_and_skip_empty_sections() {
        let info = HardwareInfo::default();
        let mut s = Snapshot::default();
        s.set(MetricKey::CpuTotal, 40.0);
        let mut e = Extremes::default();
        for v in [10.0, 90.0, 40.0] {
            let mut x = Snapshot::default();
            x.set(MetricKey::CpuTotal, v);
            e.record(&x);
        }
        let (h, c) = (HistoryStore::new(5), Config::default());
        let view = build(Page::Sensors, &data(&info, &s, &h, &e, &c));
        let total = view.sensors.iter().find(|r| r.label == "Total").unwrap();
        assert_eq!(
            (
                total.value.as_str(),
                total.low.as_str(),
                total.high.as_str()
            ),
            ("40%", "10%", "90%")
        );
        assert!(
            !view.sensors.iter().any(|r| r.id == "memory"),
            "no memory readings, no memory branch"
        );
        assert_eq!(
            view.sensors
                .iter()
                .map(|r| (r.depth, r.id.as_str()))
                .collect::<Vec<_>>(),
            [(0, "cpu"), (1, "cpu/Load"), (2, "")]
        );
    }

    #[test]
    fn closed_branches_hide_what_is_under_them() {
        let info = HardwareInfo::default();
        let mut s = Snapshot::default();
        s.set(MetricKey::CpuTotal, 40.0);
        s.set(MetricKey::CpuTempC, 50.0);
        s.set(MetricKey::MemUsedPercent, 70.0);
        let (h, e, c) = (HistoryStore::new(5), Extremes::default(), Config::default());
        let prefs = Prefs {
            closed: ["cpu/Load".to_string(), "memory".to_string()].into(),
            ..Prefs::default()
        };
        let d = Data {
            prefs: &prefs,
            ..data(&info, &s, &h, &e, &c)
        };
        let rows = build(Page::Sensors, &d).sensors;
        let shown: Vec<(&str, bool)> = rows
            .iter()
            .filter(|r| r.depth < 2)
            .map(|r| (r.id.as_str(), r.open))
            .collect();
        assert_eq!(
            shown,
            [
                ("cpu", true),
                ("cpu/Load", false),
                ("cpu/Temperatures", true),
                ("memory", false)
            ]
        );
        assert_eq!(rows[0].value, "40% · 50°C");
        assert_eq!(rows.iter().filter(|r| r.depth == 2).count(), 1);
    }
}
