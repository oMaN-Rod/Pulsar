use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use crate::config::Config;
use crate::gpu_slots::{GpuAdapter, GpuSlots};
use crate::hardware::DriveTempSource;
use crate::metric::{ItemKind, Snapshot, Source, SourceError, SourceId};
use crate::sources::{
    BatterySource, CpuSource, DiskIoSource, DiskSpaceSource, EnergyMeterSource, FramesSource,
    GpuPowerSource, GpuSensorsSource, GpuSource, MemorySource, NetworkSource, PingSource,
    ProcessesSource, SensorServiceSource, ThermalZoneSource,
};

pub type ProcessesFactory = Box<dyn Fn() -> Box<dyn Source> + Send>;
pub type ExtrasFactory = Box<dyn Fn() -> Vec<Box<dyn Source>> + Send>;
type AdapterList = Box<dyn Fn() -> Vec<GpuAdapter> + Send>;

/// How often the adapters are listed when nothing suggests they changed.
const ADAPTER_REFRESH: Duration = Duration::from_secs(30);
/// How soon to list them again after finding none, as DirectX may not be
/// ready, or when the sources no longer see one that was listed.
const NO_ADAPTER_RETRY: Duration = Duration::from_secs(5);

/// Keeps the card table current and files each tick's readings under it, so
/// load, memory and temperature always agree on which slot a card has.
struct GpuTracker {
    slots: GpuSlots,
    list: AdapterList,
    seen: BTreeSet<u64>,
    listed: Option<Instant>,
}

impl GpuTracker {
    fn new(list: AdapterList) -> Self {
        Self {
            slots: GpuSlots::new(),
            list,
            seen: BTreeSet::new(),
            listed: None,
        }
    }

    /// Lists adapters only once a GPU source has reported any.
    fn publish(&mut self, snapshot: &mut Snapshot, now: Instant) {
        let readings = std::mem::take(&mut snapshot.gpu_readings);
        let settled = self.slots.present().next().is_some()
            && self
                .slots
                .present()
                .all(|(_, s)| readings.seen.contains(&s.luid));
        let wait = if settled {
            ADAPTER_REFRESH
        } else {
            NO_ADAPTER_RETRY
        };
        let due =
            !readings.seen.is_empty() && self.listed.is_none_or(|t| now.duration_since(t) >= wait);
        if due || readings.seen != self.seen {
            self.slots.update(&(self.list)());
            self.listed = Some(now);
            self.seen.clone_from(&readings.seen);
        }
        self.slots.publish(&readings, snapshot);
        snapshot.gpus.clone_from(&self.slots);
    }
}

/// Runs every source once per tick. A failing source is reported in
/// `Snapshot::errors` and never prevents the others from sampling.
pub struct Sampler {
    sources: Vec<Box<dyn Source>>,
    processes: Option<Box<dyn Source>>,
    processes_factory: Option<ProcessesFactory>,
    want_processes: Arc<AtomicBool>,
    processes_primed: bool,
    extras: Option<Vec<Box<dyn Source>>>,
    extras_factory: Option<ExtrasFactory>,
    want_extras: Arc<AtomicBool>,
    gpus: GpuTracker,
}

impl Sampler {
    pub fn new(sources: Vec<Box<dyn Source>>, processes: Option<Box<dyn Source>>) -> Self {
        Self {
            sources,
            processes,
            processes_factory: None,
            want_processes: Arc::new(AtomicBool::new(false)),
            processes_primed: false,
            extras: None,
            extras_factory: None,
            want_extras: Arc::new(AtomicBool::new(false)),
            gpus: GpuTracker::new(Box::new(crate::hardware::gpu::adapters)),
        }
    }

    /// Continues the table of the sampler this one replaces: cards must keep
    /// their slots for the life of the process.
    pub fn with_gpu_slots(mut self, slots: GpuSlots) -> Self {
        self.gpus.slots = slots;
        self
    }

    /// Sources for what the overlay leaves out, built while the details
    /// window is open and released when it closes.
    pub fn with_extras(self, factory: ExtrasFactory) -> Self {
        Self {
            extras_factory: Some(factory),
            ..self
        }
    }

    /// The processes source is built when first wanted and released when no
    /// longer wanted: its PDH query costs several MB and is only needed while
    /// a popup lists processes.
    pub fn with_lazy_processes(sources: Vec<Box<dyn Source>>, factory: ProcessesFactory) -> Self {
        Self {
            processes_factory: Some(factory),
            ..Self::new(sources, None)
        }
    }

    pub fn from_config(config: &Config) -> Self {
        let mut enabled: Vec<ItemKind> = Vec::new();
        for kind in config.sampled_items().into_iter().map(sources_of) {
            if !enabled.contains(&kind) {
                enabled.push(kind);
            }
        }
        let mut sources: Vec<Box<dyn Source>> = enabled
            .iter()
            .flat_map(|&kind| item_sources(kind, config))
            .collect();
        // The CPU and GPU popups show the service's readings when it is
        // there, but only the sensor items need it.
        let sensors_required = [ItemKind::CpuTemp, ItemKind::CpuPower, ItemKind::ChipsetTemp]
            .into_iter()
            .any(|kind| config.is_sampled(kind));
        let sensors_shown = [
            ItemKind::Cpu,
            ItemKind::CpuClock,
            ItemKind::Gpu,
            ItemKind::GpuTemp,
        ]
        .into_iter()
        .any(|kind| config.is_sampled(kind));
        let sensors = sensors_required || sensors_shown;
        if sensors {
            sources.push(Box::new(SensorServiceSource::new(sensors_required)));
            sources.extend(sensor_fallbacks());
        }
        // The GPU popup shows the sensors too, so either item needs them.
        let gpu_sensors = config.is_sampled(ItemKind::Gpu) || config.is_sampled(ItemKind::GpuTemp);
        if gpu_sensors {
            sources.push(gpu_sensors_source());
        }
        let config = config.clone();
        Self::with_lazy_processes(
            sources,
            Box::new(|| or_failed(SourceId::Processes, ProcessesSource::new())),
        )
        .with_extras(Box::new(move || {
            let mut extras: Vec<Box<dyn Source>> = ItemKind::ALL
                .into_iter()
                .filter(|&kind| {
                    sources_of(kind) == kind && !enabled.contains(&kind) && sampled_as_extra(kind)
                })
                .flat_map(|kind| item_sources(kind, &config))
                .collect();
            if !sensors {
                extras.push(Box::new(SensorServiceSource::new(false)));
                extras.extend(sensor_fallbacks());
            }
            if !gpu_sensors {
                extras.push(gpu_sensors_source());
            }
            extras
        }))
    }

    pub fn with_flags(self, processes: Arc<AtomicBool>, extras: Arc<AtomicBool>) -> Self {
        Self {
            want_processes: processes,
            want_extras: extras,
            ..self
        }
    }

    /// Shared flag the UI sets while the details window is open; the sampler
    /// then reads everything, not just what the overlay shows.
    pub fn extras_flag(&self) -> Arc<AtomicBool> {
        self.want_extras.clone()
    }

    /// Shared flag the UI sets while a popup that lists processes is open.
    pub fn processes_flag(&self) -> Arc<AtomicBool> {
        self.want_processes.clone()
    }

    /// Tells sources that work between samples to stop or go on.
    pub fn set_paused(&mut self, paused: bool) {
        for source in self
            .sources
            .iter_mut()
            .chain(self.extras.iter_mut().flatten())
            .chain(self.processes.iter_mut())
        {
            source.set_paused(paused);
        }
        if paused {
            self.processes_primed = false;
        }
    }

    /// How long each source takes per reading, averaged over `rounds`.
    pub fn source_costs(&mut self, rounds: u32, settle: Duration) -> Vec<(SourceId, Duration)> {
        self.source_timings(rounds, settle)
            .into_iter()
            .map(|(id, times)| (id, times.iter().sum::<Duration>() / rounds.max(1)))
            .collect()
    }

    /// Each source's time for every one of `rounds` readings, in sampling order.
    pub fn source_timings(
        &mut self,
        rounds: u32,
        settle: Duration,
    ) -> Vec<(SourceId, Vec<Duration>)> {
        let mut timings: Vec<(SourceId, Vec<Duration>)> = Vec::new();
        for round in 0..rounds {
            if round > 0 {
                std::thread::sleep(settle);
            }
            let sources = self
                .sources
                .iter_mut()
                .chain(self.extras.iter_mut().flatten())
                .chain(self.processes.iter_mut());
            // Shared, as fallback sources skip what earlier sources provided.
            let mut snapshot = Snapshot::default();
            for (i, source) in sources.enumerate() {
                let start = Instant::now();
                run(source.as_mut(), &mut snapshot);
                let spent = start.elapsed();
                match timings.get_mut(i) {
                    Some((_, times)) => times.push(spent),
                    None => timings.push((source.id(), vec![spent])),
                }
            }
        }
        timings
    }

    pub fn sample(&mut self) -> Snapshot {
        let mut snapshot = Snapshot::default();
        for source in &mut self.sources {
            run(source.as_mut(), &mut snapshot);
        }
        if self.want_extras.load(Ordering::Relaxed) {
            if self.extras.is_none()
                && let Some(factory) = &self.extras_factory
            {
                self.extras = Some(factory());
            }
            for source in self.extras.iter_mut().flatten() {
                run(source.as_mut(), &mut snapshot);
            }
        } else {
            self.extras = None;
        }

        let wanted = self.want_processes.load(Ordering::Relaxed);
        if wanted
            && self.processes.is_none()
            && let Some(factory) = &self.processes_factory
        {
            self.processes = Some(factory());
        }
        if !wanted && self.processes_factory.is_some() {
            self.processes = None;
        }
        if let (true, Some(processes)) = (wanted, self.processes.as_mut()) {
            if self.processes_primed {
                run(processes.as_mut(), &mut snapshot);
            } else {
                // Rate counters average over the time since the last collect,
                // which may be minutes; discard that first reading.
                run(processes.as_mut(), &mut Snapshot::default());
                self.processes_primed = true;
            }
        } else if !wanted {
            self.processes_primed = false;
        }
        self.gpus.publish(&mut snapshot, Instant::now());
        snapshot
    }
}

/// The sources an overlay item reads.
fn item_sources(kind: ItemKind, config: &Config) -> Vec<Box<dyn Source>> {
    match kind {
        ItemKind::Cpu => vec![or_failed(SourceId::Cpu, CpuSource::new())],
        ItemKind::Ram => vec![Box::new(MemorySource::new())],
        ItemKind::Disk => vec![
            or_failed(SourceId::DiskIo, DiskIoSource::new()),
            Box::new(DiskSpaceSource::default()),
        ],
        ItemKind::Network => vec![or_failed(
            SourceId::Network,
            NetworkSource::new(config.network.adapters.clone()),
        )],
        ItemKind::Gpu => vec![or_failed(SourceId::Gpu, GpuSource::new())],
        ItemKind::Ping => vec![Box::new(PingSource::spawn(
            config.ping.host.clone(),
            Duration::from_millis(config.ping.interval_ms.into()),
        ))],
        ItemKind::DiskTemp => vec![Box::new(DriveTempSource::new())],
        ItemKind::Battery => vec![Box::new(BatterySource::default())],
        ItemKind::Fps => vec![Box::new(FramesSource::new(config.frames.monitor.clone()))],
        ItemKind::GpuPower => vec![Box::new(GpuPowerSource::default())],
        ItemKind::GpuTemp
        | ItemKind::GpuMemory
        | ItemKind::CpuTemp
        | ItemKind::CpuPower
        | ItemKind::CpuClock
        | ItemKind::ChipsetTemp => Vec::new(),
    }
}

/// The item whose sources `kind` reads.
fn sources_of(kind: ItemKind) -> ItemKind {
    match kind {
        ItemKind::CpuClock => ItemKind::Cpu,
        ItemKind::GpuMemory => ItemKind::Gpu,
        kind => kind,
    }
}

/// Whether the details window samples `kind` while its item is off. Pinging
/// is traffic the user turned off, asking for frame rates starts a trace in
/// the service, and NVIDIA's library keeps about 20 MB once loaded, even
/// after it is freed.
fn sampled_as_extra(kind: ItemKind) -> bool {
    !matches!(kind, ItemKind::Ping | ItemKind::Fps | ItemKind::GpuPower)
}

/// Windows' own readings, for what the sensor service did not provide;
/// they must run after it.
fn sensor_fallbacks() -> [Box<dyn Source>; 2] {
    [
        Box::new(ThermalZoneSource::default()),
        Box::new(EnergyMeterSource::default()),
    ]
}

fn gpu_sensors_source() -> Box<dyn Source> {
    Box::new(GpuSensorsSource::default())
}

/// Sources whose error state differs between two snapshots, in `SourceId`
/// order: `Some(reason)` when one started failing or its reason changed,
/// `None` when it recovered.
pub fn error_changes(
    old: &HashMap<SourceId, String>,
    new: &HashMap<SourceId, String>,
) -> Vec<(SourceId, Option<String>)> {
    let mut changes: Vec<(SourceId, Option<String>)> = new
        .iter()
        .filter(|(id, reason)| old.get(id) != Some(reason))
        .map(|(id, reason)| (*id, Some(reason.clone())))
        .chain(
            old.keys()
                .filter(|id| !new.contains_key(id))
                .map(|id| (*id, None)),
        )
        .collect();
    changes.sort_by_key(|(id, _)| *id as u8);
    changes
}

fn run(source: &mut dyn Source, snapshot: &mut Snapshot) {
    if let Err(e) = source.sample(snapshot) {
        snapshot.errors.insert(source.id(), e.to_string());
    }
}

fn or_failed<S: Source + 'static>(id: SourceId, result: Result<S, SourceError>) -> Box<dyn Source> {
    match result {
        Ok(source) => Box::new(source),
        Err(e) => Box::new(FailedSource {
            id,
            message: e.to_string(),
        }),
    }
}

/// Stands in for a source that could not be created, reporting why on every tick.
struct FailedSource {
    id: SourceId,
    message: String,
}

impl Source for FailedSource {
    fn id(&self) -> SourceId {
        self.id
    }

    fn sample(&mut self, _: &mut Snapshot) -> Result<(), SourceError> {
        Err(SourceError::Unavailable(self.message.clone()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gpu_slots::{GpuReadings, adapter};
    use crate::history::HistoryStore;
    use crate::metric::{MetricKey, ProcessUsage};
    use std::sync::Mutex;

    struct Fake(SourceId, Result<f64, &'static str>);

    impl Source for Fake {
        fn id(&self) -> SourceId {
            self.0
        }
        fn sample(&mut self, out: &mut Snapshot) -> Result<(), SourceError> {
            match self.1 {
                Ok(v) => {
                    out.set(MetricKey::CpuTotal, v);
                    Ok(())
                }
                Err(e) => Err(SourceError::Unavailable(e.into())),
            }
        }
    }

    struct FakeProcesses(u32);

    impl Source for FakeProcesses {
        fn id(&self) -> SourceId {
            SourceId::Processes
        }
        fn sample(&mut self, out: &mut Snapshot) -> Result<(), SourceError> {
            self.0 += 1;
            out.processes = vec![ProcessUsage {
                name: format!("call{}", self.0),
                cpu_percent: 1.0,
                private_bytes: 0.0,
                gpu_percent: 0.0,
                io_bps: 0.0,
            }];
            Ok(())
        }
    }

    #[test]
    fn the_clock_item_shares_the_cpu_source() {
        let mut config = Config::default();
        for item in &mut config.items {
            item.enabled = matches!(item.kind, ItemKind::Cpu | ItemKind::CpuClock);
        }
        let ids = |config: &Config| -> Vec<SourceId> {
            Sampler::from_config(config)
                .sources
                .iter()
                .map(|s| s.id())
                .collect()
        };
        assert_eq!(
            ids(&config)
                .iter()
                .filter(|&&id| id == SourceId::Cpu)
                .count(),
            1
        );
        for item in &mut config.items {
            item.enabled = item.kind == ItemKind::CpuClock;
        }
        assert!(ids(&config).contains(&SourceId::Cpu));
    }

    #[test]
    fn the_memory_item_shares_the_gpu_source() {
        let sources = |kinds: &[ItemKind]| {
            let mut config = Config::default();
            for item in &mut config.items {
                item.enabled = kinds.contains(&item.kind);
            }
            let sources = Sampler::from_config(&config).sources;
            sources.iter().filter(|s| s.id() == SourceId::Gpu).count()
        };
        assert_eq!(sources(&[ItemKind::GpuMemory]), 1);
        assert_eq!(sources(&[ItemKind::Gpu, ItemKind::GpuMemory]), 1);
    }

    struct PauseAware(Arc<AtomicBool>);

    impl Source for PauseAware {
        fn id(&self) -> SourceId {
            SourceId::Ping
        }
        fn sample(&mut self, _: &mut Snapshot) -> Result<(), SourceError> {
            Ok(())
        }
        fn set_paused(&mut self, paused: bool) {
            self.0.store(paused, Ordering::Relaxed);
        }
    }

    #[test]
    fn pausing_reaches_the_sources() {
        let paused = Arc::new(AtomicBool::new(false));
        let mut sampler = Sampler::new(vec![Box::new(PauseAware(paused.clone()))], None);
        sampler.set_paused(true);
        assert!(paused.load(Ordering::Relaxed));
        sampler.set_paused(false);
        assert!(!paused.load(Ordering::Relaxed));
    }

    #[test]
    fn source_costs_list_each_source_once_whatever_the_rounds() {
        let flag = Arc::<AtomicBool>::default();
        let mut sampler = Sampler::new(vec![Box::new(PauseAware(flag))], None);
        let costs = sampler.source_costs(3, Duration::ZERO);
        assert_eq!(costs.len(), 1);
        assert_eq!(costs[0].0, SourceId::Ping);
    }

    #[test]
    fn failing_source_is_isolated() {
        let mut sampler = Sampler::new(
            vec![
                Box::new(Fake(SourceId::Gpu, Err("GPU counters unavailable"))),
                Box::new(Fake(SourceId::Cpu, Ok(42.0))),
            ],
            None,
        );
        let snap = sampler.sample();
        assert_eq!(snap.get(MetricKey::CpuTotal), Some(42.0));
        assert_eq!(snap.errors[&SourceId::Gpu], "GPU counters unavailable");
        assert!(!snap.errors.contains_key(&SourceId::Cpu));
    }

    #[test]
    fn processes_only_sampled_when_wanted_and_first_reading_discarded() {
        let mut sampler = Sampler::new(vec![], Some(Box::new(FakeProcesses(0))));
        let flag = sampler.processes_flag();

        assert!(sampler.sample().processes.is_empty(), "not wanted");

        flag.store(true, Ordering::Relaxed);
        assert!(
            sampler.sample().processes.is_empty(),
            "priming read discarded"
        );
        assert_eq!(sampler.sample().processes[0].name, "call2");

        flag.store(false, Ordering::Relaxed);
        assert!(sampler.sample().processes.is_empty());

        flag.store(true, Ordering::Relaxed);
        assert!(
            sampler.sample().processes.is_empty(),
            "re-primed after being turned off"
        );
        assert_eq!(sampler.sample().processes[0].name, "call4");
    }

    #[test]
    fn processes_source_is_created_only_when_first_wanted() {
        use std::sync::atomic::AtomicUsize;
        let created = Arc::new(AtomicUsize::new(0));
        let counter = created.clone();
        let mut sampler = Sampler::with_lazy_processes(
            vec![],
            Box::new(move || {
                counter.fetch_add(1, Ordering::SeqCst);
                Box::new(FakeProcesses(0)) as Box<dyn Source>
            }),
        );
        sampler.sample();
        sampler.sample();
        assert_eq!(created.load(Ordering::SeqCst), 0, "not built while unused");

        sampler.processes_flag().store(true, Ordering::Relaxed);
        sampler.sample();
        sampler.sample();
        assert_eq!(created.load(Ordering::SeqCst), 1, "built once on first use");
        assert!(!sampler.sample().processes.is_empty());
    }

    #[test]
    fn processes_source_is_released_when_no_longer_wanted() {
        use std::sync::atomic::AtomicUsize;

        struct Counted(Arc<AtomicUsize>);
        impl Source for Counted {
            fn id(&self) -> SourceId {
                SourceId::Processes
            }
            fn sample(&mut self, _: &mut Snapshot) -> Result<(), SourceError> {
                Ok(())
            }
        }
        impl Drop for Counted {
            fn drop(&mut self) {
                self.0.fetch_add(1, Ordering::SeqCst);
            }
        }

        let created = Arc::new(AtomicUsize::new(0));
        let dropped = Arc::new(AtomicUsize::new(0));
        let (c, d) = (created.clone(), dropped.clone());
        let mut sampler = Sampler::with_lazy_processes(
            vec![],
            Box::new(move || {
                c.fetch_add(1, Ordering::SeqCst);
                Box::new(Counted(d.clone())) as Box<dyn Source>
            }),
        );
        let flag = sampler.processes_flag();

        flag.store(true, Ordering::Relaxed);
        sampler.sample();
        flag.store(false, Ordering::Relaxed);
        sampler.sample();
        assert_eq!(
            dropped.load(Ordering::SeqCst),
            1,
            "released after the popup closes"
        );

        flag.store(true, Ordering::Relaxed);
        sampler.sample();
        assert_eq!(created.load(Ordering::SeqCst), 2, "rebuilt on the next use");
    }

    #[test]
    fn extras_are_sampled_only_while_wanted() {
        let mut sampler = Sampler::new(vec![], None).with_extras(Box::new(|| {
            vec![Box::new(Fake(SourceId::Cpu, Ok(7.0))) as Box<dyn Source>]
        }));
        let flag = sampler.extras_flag();
        assert_eq!(sampler.sample().get(MetricKey::CpuTotal), None);
        flag.store(true, Ordering::Relaxed);
        assert_eq!(sampler.sample().get(MetricKey::CpuTotal), Some(7.0));
        flag.store(false, Ordering::Relaxed);
        assert_eq!(sampler.sample().get(MetricKey::CpuTotal), None);
        assert!(sampler.extras.is_none(), "released when no longer wanted");
    }

    #[test]
    fn details_leaves_out_traffic_traces_and_nvml() {
        for kind in [ItemKind::Ping, ItemKind::Fps, ItemKind::GpuPower] {
            assert!(!sampled_as_extra(kind), "{kind:?}");
        }
        assert!(sampled_as_extra(ItemKind::Gpu));
    }

    #[test]
    fn failed_source_reports_its_message() {
        let mut sampler = Sampler::new(
            vec![or_failed::<Fake>(
                SourceId::Gpu,
                Err(SourceError::Unavailable("nope".into())),
            )],
            None,
        );
        assert_eq!(sampler.sample().errors[&SourceId::Gpu], "nope");
    }

    /// What the fake system lists and reads now: each card's load, with its
    /// memory the same and its temperature one higher.
    #[derive(Clone, Default)]
    struct Machine(Arc<Mutex<Vec<(GpuAdapter, f64)>>>);

    impl Machine {
        fn set(&self, cards: &[(&str, u64, f64)]) {
            *self.0.lock().unwrap() = cards
                .iter()
                .map(|&(name, luid, value)| (adapter(name, luid), value))
                .collect();
        }

        fn lister(&self) -> AdapterList {
            let machine = self.clone();
            Box::new(move || {
                let cards = machine.0.lock().unwrap();
                cards.iter().map(|(a, _)| a.clone()).collect()
            })
        }

        fn sampler(&self) -> Sampler {
            let mut sampler = Sampler::new(
                vec![
                    Box::new(FakeEngines(self.clone())),
                    Box::new(FakeSensors(self.clone())),
                ],
                None,
            );
            sampler.gpus.list = self.lister();
            sampler
        }
    }

    struct FakeEngines(Machine);

    impl Source for FakeEngines {
        fn id(&self) -> SourceId {
            SourceId::Gpu
        }
        fn sample(&mut self, out: &mut Snapshot) -> Result<(), SourceError> {
            let cards = self.0.0.lock().unwrap();
            let by_luid: HashMap<u64, f64> = cards.iter().map(|(a, v)| (a.luid, *v)).collect();
            out.gpu_readings.seen.extend(by_luid.keys());
            out.gpu_readings.load = Some(by_luid.clone());
            out.gpu_readings.dedicated = by_luid;
            Ok(())
        }
    }

    struct FakeSensors(Machine);

    impl Source for FakeSensors {
        fn id(&self) -> SourceId {
            SourceId::GpuSensors
        }
        fn sample(&mut self, out: &mut Snapshot) -> Result<(), SourceError> {
            let cards = self.0.0.lock().unwrap();
            for (a, v) in cards.iter() {
                out.gpu_readings.seen.insert(a.luid);
                out.gpu_readings.temp_c.insert(a.luid, v + 1.0);
            }
            Ok(())
        }
    }

    /// A slot's load, having checked its memory and temperature agree on the card.
    fn card(s: &Snapshot, slot: u8) -> Option<f64> {
        let load = s.get(MetricKey::GpuAdapterUtil(slot));
        assert_eq!(s.get(MetricKey::GpuAdapterDedicatedBytes(slot)), load);
        assert_eq!(
            s.get(MetricKey::GpuAdapterTempC(slot)),
            load.map(|v| v + 1.0)
        );
        load
    }

    #[test]
    fn a_slot_goes_on_reading_its_own_card_as_adapters_change() {
        let machine = Machine::default();
        machine.set(&[("RTX 4090", 1, 10.0), ("Radeon", 2, 20.0)]);
        let mut sampler = machine.sampler();
        let s = sampler.sample();
        assert_eq!((card(&s, 0), card(&s, 1)), (Some(10.0), Some(20.0)));

        machine.set(&[("Radeon", 2, 21.0)]);
        let s = sampler.sample();
        assert_eq!(
            (card(&s, 0), card(&s, 1)),
            (None, Some(21.0)),
            "a card that's gone shows nothing, not the other's reading"
        );

        machine.set(&[("Radeon", 2, 22.0), ("RTX 5090", 3, 30.0)]);
        let s = sampler.sample();
        assert_eq!(
            (card(&s, 0), card(&s, 1), card(&s, 2)),
            (None, Some(22.0), Some(30.0))
        );

        machine.set(&[
            ("RTX 5090", 3, 31.0),
            ("Radeon", 2, 23.0),
            ("RTX 4090", 1, 11.0),
        ]);
        let s = sampler.sample();
        assert_eq!(
            (card(&s, 0), card(&s, 1), card(&s, 2)),
            (Some(11.0), Some(23.0), Some(31.0)),
            "a replug under the same LUID returns to its slot, whatever the order"
        );
    }

    #[test]
    fn identical_cards_keep_their_readings_when_one_goes() {
        let machine = Machine::default();
        machine.set(&[("RTX 4090", 1, 10.0), ("RTX 4090", 2, 20.0)]);
        let mut sampler = machine.sampler();
        sampler.sample();

        machine.set(&[("RTX 4090", 2, 21.0)]);
        let s = sampler.sample();
        assert_eq!((card(&s, 0), card(&s, 1)), (None, Some(21.0)));
        assert_eq!(s.gpus.slot_named("RTX 4090 #2"), Some(1));
        assert_eq!(s.gpus.slot_named("RTX 4090"), None);
    }

    #[test]
    fn history_of_a_slot_taken_by_a_new_luid_starts_over() {
        let machine = Machine::default();
        machine.set(&[("RTX 4090", 1, 10.0), ("Radeon", 2, 20.0)]);
        let mut sampler = machine.sampler();
        let mut history = HistoryStore::new(10);
        history.record(&sampler.sample());

        machine.set(&[("Radeon", 2, 21.0)]);
        history.record(&sampler.sample());
        let series = |h: &HistoryStore, key| -> Vec<f64> { h.get(key).unwrap().iter().collect() };
        let slot0 = series(&history, MetricKey::GpuAdapterUtil(0));
        assert_eq!(slot0[0], 10.0);
        assert!(slot0[1].is_nan(), "a gap while it's gone");

        machine.set(&[("Radeon", 2, 22.0), ("RTX 4090", 9, 40.0)]);
        let s = sampler.sample();
        assert_eq!(card(&s, 0), Some(40.0));
        history.record(&s);
        for (key, latest) in [
            (MetricKey::GpuAdapterUtil(0), 40.0),
            (MetricKey::GpuAdapterDedicatedBytes(0), 40.0),
            (MetricKey::GpuAdapterTempC(0), 41.0),
        ] {
            let samples = series(&history, key);
            assert!(samples[..2].iter().all(|v| v.is_nan()), "{key:?}");
            assert_eq!(samples[2], latest, "{key:?}");
        }
        assert_eq!(
            series(&history, MetricKey::GpuAdapterUtil(1)),
            [20.0, 21.0, 22.0],
            "the other card keeps its history"
        );
    }

    #[test]
    fn a_replacement_sampler_continues_the_table() {
        let machine = Machine::default();
        machine.set(&[("RTX 4090", 1, 10.0), ("Radeon", 2, 20.0)]);
        let mut first = machine.sampler();
        first.sample();
        machine.set(&[("Radeon", 2, 21.0)]);
        let table = first.sample().gpus;

        let mut second = machine.sampler().with_gpu_slots(table);
        let s = second.sample();
        assert_eq!(
            (card(&s, 0), card(&s, 1)),
            (None, Some(21.0)),
            "a fresh listing would have put the Radeon in slot 0"
        );
    }

    #[test]
    fn an_empty_adapter_list_is_listed_again() {
        let machine = Machine::default();
        let mut tracker = GpuTracker::new(machine.lister());
        let reading = || Snapshot {
            gpu_readings: GpuReadings {
                seen: BTreeSet::from([1]),
                load: Some(HashMap::from([(1, 50.0)])),
                ..GpuReadings::default()
            },
            ..Snapshot::default()
        };
        let start = Instant::now();
        let mut s = reading();
        tracker.publish(&mut s, start);
        assert!(s.gpus.is_empty());

        machine.set(&[("RTX 4090", 1, 0.0)]);
        let mut s = reading();
        tracker.publish(&mut s, start + Duration::from_secs(1));
        assert!(s.gpus.is_empty(), "not asked again on every tick");

        let mut s = reading();
        tracker.publish(&mut s, start + NO_ADAPTER_RETRY);
        assert_eq!(s.gpus.slot_of(1), Some(0));
        assert_eq!(s.get(MetricKey::GpuAdapterUtil(0)), Some(50.0));
    }

    #[test]
    fn a_card_still_listed_after_it_went_shows_nothing_and_is_listed_again_soon() {
        let machine = Machine::default();
        machine.set(&[("RTX 4090", 1, 10.0), ("Radeon", 2, 20.0)]);
        let mut tracker = GpuTracker::new(machine.lister());
        let seeing = |luids: &[u64]| Snapshot {
            gpu_readings: GpuReadings {
                seen: luids.iter().copied().collect(),
                load: Some(HashMap::new()),
                ..GpuReadings::default()
            },
            ..Snapshot::default()
        };
        let start = Instant::now();
        tracker.publish(&mut seeing(&[1, 2]), start);

        let mut s = seeing(&[2]);
        tracker.publish(&mut s, start + Duration::from_secs(1));
        assert_eq!(s.gpus.slot_of(1), Some(0), "DirectX still lists it");
        assert_eq!(s.get(MetricKey::GpuAdapterUtil(0)), None, "but no reading");
        assert_eq!(s.get(MetricKey::GpuAdapterUtil(1)), Some(0.0));

        machine.set(&[("Radeon", 2, 20.0)]);
        let mut s = seeing(&[2]);
        tracker.publish(&mut s, start + Duration::from_secs(1) + NO_ADAPTER_RETRY);
        assert_eq!(s.gpus.slot_of(1), None);
    }

    #[test]
    fn adapters_are_listed_on_a_change_and_otherwise_rarely() {
        use std::sync::atomic::AtomicUsize;
        let calls = Arc::new(AtomicUsize::new(0));
        let counter = calls.clone();
        let mut tracker = GpuTracker::new(Box::new(move || {
            counter.fetch_add(1, Ordering::SeqCst);
            vec![adapter("RTX 4090", 1)]
        }));
        let seeing = |luids: &[u64]| {
            let mut s = Snapshot::default();
            s.gpu_readings.seen = luids.iter().copied().collect();
            s
        };
        let start = Instant::now();
        let at = |secs| start + Duration::from_secs(secs);
        tracker.publish(&mut seeing(&[]), at(0));
        assert_eq!(calls.load(Ordering::SeqCst), 0, "no GPU source, no DirectX");
        tracker.publish(&mut seeing(&[1]), at(0));
        tracker.publish(&mut seeing(&[1]), at(1));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        tracker.publish(&mut seeing(&[1, 2]), at(2));
        assert_eq!(calls.load(Ordering::SeqCst), 2, "the sources saw a change");
        tracker.publish(&mut seeing(&[1, 2]), at(31));
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        tracker.publish(&mut seeing(&[1, 2]), at(32));
        assert_eq!(calls.load(Ordering::SeqCst), 3, "the periodic check");
    }

    #[test]
    fn error_changes_report_new_changed_and_recovered_sources() {
        let old = HashMap::from([
            (SourceId::Gpu, "GPU counters unavailable".to_string()),
            (SourceId::Ping, "timeout".to_string()),
        ]);
        let new = HashMap::from([
            (SourceId::Ping, "unreachable".to_string()),
            (SourceId::Cpu, "no counters".to_string()),
        ]);
        assert_eq!(
            error_changes(&old, &new),
            vec![
                (SourceId::Cpu, Some("no counters".to_string())),
                (SourceId::Gpu, None),
                (SourceId::Ping, Some("unreachable".to_string())),
            ]
        );
        assert!(error_changes(&new, &new).is_empty());
    }
}
