use crate::metric::{MetricKey, Snapshot, Source, SourceError, SourceId};
use crate::pdh::{Counter, Query};

pub struct CpuSource {
    query: Query,
    total: Counter,
    cores: Counter,
    performance: Counter,
    base_mhz: Counter,
}

impl CpuSource {
    pub fn new() -> Result<Self, SourceError> {
        let query = Query::new()?;
        let total = query.add(r"\Processor Information(_Total)\% Processor Utility")?;
        let cores = query.add(r"\Processor Information(*)\% Processor Utility")?;
        let performance = query.add(r"\Processor Information(_Total)\% Processor Performance")?;
        let base_mhz = query.add(r"\Processor Information(_Total)\Processor Frequency")?;
        query.collect()?;
        Ok(Self {
            query,
            total,
            cores,
            performance,
            base_mhz,
        })
    }
}

impl Source for CpuSource {
    fn id(&self) -> SourceId {
        SourceId::Cpu
    }

    fn sample(&mut self, out: &mut Snapshot) -> Result<(), SourceError> {
        self.query.collect()?;
        out.set(MetricKey::CpuTotal, self.total.value()?.clamp(0.0, 100.0));

        let mut cores: Vec<((u32, u32), f64)> = self
            .cores
            .array()?
            .into_iter()
            .filter_map(|(name, value)| core_index(&name).map(|index| (index, value)))
            .collect();
        cores.sort_by_key(|&(index, _)| index);
        for (i, (_, value)) in cores.into_iter().enumerate() {
            out.set(MetricKey::CpuCore(i as u16), value.clamp(0.0, 100.0));
        }

        if let (Ok(perf), Ok(base)) = (self.performance.value(), self.base_mhz.value()) {
            out.set(MetricKey::CpuClockMhz, perf * base / 100.0);
        }
        Ok(())
    }
}

/// `"0,3"` → `(0, 3)`; `"_Total"` and `"0,_Total"` → `None`.
fn core_index(instance: &str) -> Option<(u32, u32)> {
    let (group, core) = instance.split_once(',')?;
    Some((group.parse().ok()?, core.parse().ok()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn core_index_parses_group_and_core() {
        assert_eq!(core_index("0,3"), Some((0, 3)));
        assert_eq!(core_index("1,12"), Some((1, 12)));
        assert_eq!(core_index("0,_Total"), None);
        assert_eq!(core_index("_Total"), None);
    }
}
