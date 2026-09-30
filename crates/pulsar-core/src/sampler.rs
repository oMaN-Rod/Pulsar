use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use crate::config::Config;
use crate::metric::{ItemKind, Snapshot, Source, SourceError, SourceId};
use crate::sources::{
    CpuSource, DiskIoSource, DiskSpaceSource, GpuSensorsSource, GpuSource, MemorySource,
    NetworkSource, PingSource, ProcessesSource,
};

pub type ProcessesFactory = Box<dyn Fn() -> Box<dyn Source> + Send>;

/// Runs every source once per tick. A failing source is reported in
/// `Snapshot::errors` and never prevents the others from sampling.
pub struct Sampler {
    sources: Vec<Box<dyn Source>>,
    processes: Option<Box<dyn Source>>,
    processes_factory: Option<ProcessesFactory>,
    want_processes: Arc<AtomicBool>,
    processes_primed: bool,
}

impl Sampler {
    pub fn new(sources: Vec<Box<dyn Source>>, processes: Option<Box<dyn Source>>) -> Self {
        Self {
            sources,
            processes,
            processes_factory: None,
            want_processes: Arc::new(AtomicBool::new(false)),
            processes_primed: false,
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
        let mut sources: Vec<Box<dyn Source>> = Vec::new();
        for kind in config.enabled_items() {
            match kind {
                ItemKind::Cpu => sources.push(or_failed(SourceId::Cpu, CpuSource::new())),
                ItemKind::Ram => sources.push(Box::new(MemorySource)),
                ItemKind::Disk => {
                    sources.push(or_failed(SourceId::DiskIo, DiskIoSource::new()));
                    sources.push(Box::new(DiskSpaceSource::default()));
                }
                ItemKind::Network => sources.push(or_failed(
                    SourceId::Network,
                    NetworkSource::new(config.network.adapter.clone()),
                )),
                ItemKind::Gpu => sources.push(or_failed(SourceId::Gpu, GpuSource::new())),
                ItemKind::GpuTemp => {}
                ItemKind::Ping => sources.push(Box::new(PingSource::spawn(
                    config.ping.host.clone(),
                    Duration::from_millis(config.ping.interval_ms.into()),
                ))),
            }
        }
        // The GPU popup shows the sensors too, so either item needs them.
        if config.is_enabled(ItemKind::Gpu) || config.is_enabled(ItemKind::GpuTemp) {
            sources.push(or_failed(SourceId::GpuSensors, GpuSensorsSource::new()));
        }
        Self::with_lazy_processes(
            sources,
            Box::new(|| or_failed(SourceId::Processes, ProcessesSource::new())),
        )
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
            }];
            Ok(())
        }
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
