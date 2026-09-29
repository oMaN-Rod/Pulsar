use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use crate::config::Config;
use crate::metric::{ItemKind, Snapshot, Source, SourceError, SourceId};
use crate::sources::{
    CpuSource, DiskIoSource, DiskSpaceSource, GpuSource, MemorySource, NetworkSource, PingSource,
    ProcessesSource,
};

/// Runs every source once per tick. A failing source is reported in
/// `Snapshot::errors` and never prevents the others from sampling.
pub struct Sampler {
    sources: Vec<Box<dyn Source>>,
    processes: Option<Box<dyn Source>>,
    want_processes: Arc<AtomicBool>,
    processes_primed: bool,
}

impl Sampler {
    pub fn new(sources: Vec<Box<dyn Source>>, processes: Option<Box<dyn Source>>) -> Self {
        Self {
            sources,
            processes,
            want_processes: Arc::new(AtomicBool::new(false)),
            processes_primed: false,
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
                ItemKind::Network => {
                    sources.push(or_failed(SourceId::Network, NetworkSource::new()))
                }
                ItemKind::Gpu => sources.push(or_failed(SourceId::Gpu, GpuSource::new())),
                ItemKind::Ping => sources.push(Box::new(PingSource::spawn(
                    config.ping.host.clone(),
                    Duration::from_millis(config.ping.interval_ms.into()),
                ))),
            }
        }
        let processes = Some(or_failed(SourceId::Processes, ProcessesSource::new()));
        Self::new(sources, processes)
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
}
