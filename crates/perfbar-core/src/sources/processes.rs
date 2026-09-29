use std::collections::HashMap;

use crate::metric::{ProcessUsage, Snapshot, Source, SourceError, SourceId};
use crate::pdh::{Counter, Query};
use crate::sources::gpu::{ENGINE_COUNTER, aggregate_engines};

pub struct ProcessesSource {
    query: Query,
    cpu: Counter,
    private: Counter,
    ids: Counter,
    /// GPU columns are zero when this is `None` or has no instances.
    gpu: Option<Counter>,
    cpu_count: usize,
}

impl ProcessesSource {
    pub fn new() -> Result<Self, SourceError> {
        Self::with_gpu_counter(ENGINE_COUNTER)
    }

    fn with_gpu_counter(gpu_path: &str) -> Result<Self, SourceError> {
        let query = Query::new()?;
        let cpu = query.add(r"\Process(*)\% Processor Time")?;
        let private = query.add(r"\Process(*)\Working Set - Private")?;
        let ids = query.add(r"\Process(*)\ID Process")?;
        let gpu = query.add(gpu_path).ok();
        query.collect()?;
        let cpu_count = std::thread::available_parallelism().map_or(1, |n| n.get());
        Ok(Self {
            query,
            cpu,
            private,
            ids,
            gpu,
            cpu_count,
        })
    }
}

impl Source for ProcessesSource {
    fn id(&self) -> SourceId {
        SourceId::Processes
    }

    fn sample(&mut self, out: &mut Snapshot) -> Result<(), SourceError> {
        self.query.collect()?;
        let gpu_per_pid = self
            .gpu
            .as_ref()
            .and_then(|counter| counter.array().ok())
            .map(|samples| aggregate_engines(&samples).per_pid)
            .unwrap_or_default();
        out.processes = group_processes(
            &self.cpu.array()?,
            &self.private.array()?,
            &self.ids.array()?,
            &gpu_per_pid,
            self.cpu_count,
        );
        Ok(())
    }
}

/// `"chrome#12"` → `"chrome"`.
fn base_name(instance: &str) -> &str {
    instance.split_once('#').map_or(instance, |(name, _)| name)
}

fn group<'m, 's>(
    groups: &'m mut HashMap<&'s str, ProcessUsage>,
    instance: &'s str,
) -> Option<&'m mut ProcessUsage> {
    let name = base_name(instance);
    if name == "_Total" || name == "Idle" {
        return None;
    }
    Some(groups.entry(name).or_insert_with(|| ProcessUsage {
        name: name.to_string(),
        cpu_percent: 0.0,
        private_bytes: 0.0,
        gpu_percent: 0.0,
    }))
}

/// Groups PDH `Process` instances by executable name (as Task Manager does),
/// normalising CPU to a share of all logical processors.
pub fn group_processes(
    cpu: &[(String, f64)],
    private: &[(String, f64)],
    ids: &[(String, f64)],
    gpu_per_pid: &HashMap<u32, f64>,
    cpu_count: usize,
) -> Vec<ProcessUsage> {
    let mut groups: HashMap<&str, ProcessUsage> = HashMap::new();
    let cpu_count = cpu_count.max(1) as f64;
    for (instance, value) in cpu {
        if let Some(p) = group(&mut groups, instance) {
            p.cpu_percent += value.max(0.0) / cpu_count;
        }
    }
    for (instance, value) in private {
        if let Some(p) = group(&mut groups, instance) {
            p.private_bytes += value.max(0.0);
        }
    }
    for (instance, pid) in ids {
        let gpu = gpu_per_pid.get(&(*pid as u32)).copied().unwrap_or(0.0);
        if let Some(p) = group(&mut groups, instance) {
            p.gpu_percent = (p.gpu_percent + gpu).min(100.0);
        }
    }
    let mut result: Vec<ProcessUsage> = groups.into_values().collect();
    for p in &mut result {
        p.cpu_percent = p.cpu_percent.min(100.0);
    }
    result
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProcessSort {
    Cpu,
    Memory,
    Gpu,
}

/// The `n` heaviest processes by `sort`, excluding those at zero.
pub fn top_by(processes: &[ProcessUsage], sort: ProcessSort, n: usize) -> Vec<ProcessUsage> {
    let key = |p: &ProcessUsage| match sort {
        ProcessSort::Cpu => p.cpu_percent,
        ProcessSort::Memory => p.private_bytes,
        ProcessSort::Gpu => p.gpu_percent,
    };
    let mut sorted: Vec<ProcessUsage> =
        processes.iter().filter(|p| key(p) > 0.0).cloned().collect();
    sorted.sort_by(|a, b| key(b).total_cmp(&key(a)).then_with(|| a.name.cmp(&b.name)));
    sorted.truncate(n);
    sorted
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(pairs: &[(&str, f64)]) -> Vec<(String, f64)> {
        pairs.iter().map(|(n, x)| (n.to_string(), *x)).collect()
    }

    fn find<'a>(ps: &'a [ProcessUsage], name: &str) -> &'a ProcessUsage {
        ps.iter().find(|p| p.name == name).unwrap()
    }

    #[test]
    fn groups_instances_by_name_and_skips_totals() {
        let ps = group_processes(
            &v(&[
                ("chrome", 40.0),
                ("chrome#1", 40.0),
                ("_Total", 800.0),
                ("Idle", 700.0),
            ]),
            &v(&[("chrome", 100.0), ("chrome#1", 50.0)]),
            &v(&[("chrome", 10.0), ("chrome#1", 11.0)]),
            &HashMap::from([(10, 5.0), (11, 7.0)]),
            8,
        );
        assert_eq!(ps.len(), 1);
        let chrome = find(&ps, "chrome");
        assert_eq!(chrome.cpu_percent, 10.0, "80% of one core over 8 cores");
        assert_eq!(chrome.private_bytes, 150.0);
        assert_eq!(chrome.gpu_percent, 12.0);
    }

    #[test]
    fn missing_gpu_instances_do_not_break_process_lists() {
        let mut source =
            ProcessesSource::with_gpu_counter(crate::sources::gpu::tests::EMPTY_OBJECT).unwrap();
        source.sample(&mut Snapshot::default()).ok();
        std::thread::sleep(std::time::Duration::from_millis(500));
        let mut snap = Snapshot::default();
        source
            .sample(&mut snap)
            .expect("process sampling survives missing GPU data");
        assert!(!snap.processes.is_empty());
        assert!(snap.processes.iter().all(|p| p.gpu_percent == 0.0));
    }

    #[test]
    fn zero_cpu_count_does_not_divide_by_zero() {
        let ps = group_processes(&v(&[("a", 50.0)]), &[], &[], &HashMap::new(), 0);
        assert_eq!(find(&ps, "a").cpu_percent, 50.0);
    }

    #[test]
    fn top_by_sorts_descending_and_truncates() {
        let p = |name: &str, cpu: f64, mem: f64| ProcessUsage {
            name: name.into(),
            cpu_percent: cpu,
            private_bytes: mem,
            gpu_percent: 0.0,
        };
        let all = vec![
            p("a", 1.0, 300.0),
            p("b", 5.0, 100.0),
            p("c", 3.0, 200.0),
            p("d", 0.0, 50.0),
        ];
        let names = |v: Vec<ProcessUsage>| v.into_iter().map(|p| p.name).collect::<Vec<_>>();
        assert_eq!(names(top_by(&all, ProcessSort::Cpu, 2)), ["b", "c"]);
        assert_eq!(
            names(top_by(&all, ProcessSort::Memory, 5)),
            ["a", "c", "b", "d"]
        );
        assert!(
            top_by(&all, ProcessSort::Gpu, 5).is_empty(),
            "all zero → nothing to show"
        );
    }
}
