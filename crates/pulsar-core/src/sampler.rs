use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use crate::config::Config;
use crate::hardware::DriveTempSource;
use crate::metric::{ItemKind, Snapshot, Source, SourceError, SourceId};
use crate::sources::{
    BatterySource, CpuSource, DiskIoSource, DiskSpaceSource, GpuSensorsSource, GpuSource,
    MemorySource, NetworkSource, PingSource, ProcessesSource, SensorServiceSource,
};

pub type ProcessesFactory = Box<dyn Fn() -> Box<dyn Source> + Send>;
pub type ExtrasFactory = Box<dyn Fn() -> Vec<Box<dyn Source>> + Send>;

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
        }
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
        let sensors_shown = [ItemKind::Cpu, ItemKind::Gpu, ItemKind::GpuTemp]
            .into_iter()
            .any(|kind| config.is_sampled(kind));
        let sensors = sensors_required || sensors_shown;
        if sensors {
            sources.push(Box::new(SensorServiceSource {
                required: sensors_required,
            }));
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
                // Pinging is traffic the user turned off.
                .filter(|&kind| {
                    sources_of(kind) == kind && !enabled.contains(&kind) && kind != ItemKind::Ping
                })
                .flat_map(|kind| item_sources(kind, &config))
                .collect();
            if !sensors {
                extras.push(Box::new(SensorServiceSource { required: false }));
            }
            if !gpu_sensors {
                extras.push(gpu_sensors_source());
            }
            extras
        }))
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
            NetworkSource::new(config.network.adapter.clone()),
        )],
        ItemKind::Gpu => vec![or_failed(SourceId::Gpu, GpuSource::new())],
        ItemKind::Ping => vec![Box::new(PingSource::spawn(
            config.ping.host.clone(),
            Duration::from_millis(config.ping.interval_ms.into()),
        ))],
        ItemKind::DiskTemp => vec![Box::new(DriveTempSource::new())],
        ItemKind::Battery => vec![Box::new(BatterySource::default())],
        ItemKind::GpuTemp
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
        kind => kind,
    }
}

fn gpu_sensors_source() -> Box<dyn Source> {
    or_failed(SourceId::GpuSensors, GpuSensorsSource::new())
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
    use crate::metric::{MetricKey, ProcessUsage};

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
