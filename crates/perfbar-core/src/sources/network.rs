use crate::metric::{AdapterRate, MetricKey, Snapshot, Source, SourceError, SourceId};
use crate::pdh::{Counter, Query};

pub struct NetworkSource {
    query: Query,
    received: Counter,
    sent: Counter,
}

impl NetworkSource {
    pub fn new() -> Result<Self, SourceError> {
        let query = Query::new()?;
        let received = query.add(r"\Network Interface(*)\Bytes Received/sec")?;
        let sent = query.add(r"\Network Interface(*)\Bytes Sent/sec")?;
        query.collect()?;
        Ok(Self {
            query,
            received,
            sent,
        })
    }
}

impl Source for NetworkSource {
    fn id(&self) -> SourceId {
        SourceId::Network
    }

    fn sample(&mut self, out: &mut Snapshot) -> Result<(), SourceError> {
        self.query.collect()?;
        let adapters = merge_adapters(self.received.array()?, self.sent.array()?);
        out.set(
            MetricKey::NetDownBps,
            adapters.iter().map(|a| a.down_bps).sum(),
        );
        out.set(MetricKey::NetUpBps, adapters.iter().map(|a| a.up_bps).sum());
        out.adapters = adapters
            .into_iter()
            .filter(|a| a.down_bps + a.up_bps > 0.0)
            .collect();
        Ok(())
    }
}

/// Joins per-adapter receive and send rates by instance name, busiest first.
pub fn merge_adapters(received: Vec<(String, f64)>, sent: Vec<(String, f64)>) -> Vec<AdapterRate> {
    let mut adapters: Vec<AdapterRate> = received
        .into_iter()
        .map(|(name, down)| AdapterRate {
            name,
            down_bps: down.max(0.0),
            up_bps: 0.0,
        })
        .collect();
    for (name, up) in sent {
        match adapters.iter_mut().find(|a| a.name == name) {
            Some(a) => a.up_bps = up.max(0.0),
            None => adapters.push(AdapterRate {
                name,
                down_bps: 0.0,
                up_bps: up.max(0.0),
            }),
        }
    }
    adapters.sort_by(|a, b| (b.down_bps + b.up_bps).total_cmp(&(a.down_bps + a.up_bps)));
    adapters
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merges_by_name_and_sorts_by_traffic() {
        let merged = merge_adapters(
            vec![("wifi".into(), 100.0), ("eth".into(), 5.0)],
            vec![("eth".into(), 1000.0), ("vpn".into(), 1.0)],
        );
        let names: Vec<&str> = merged.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(names, ["eth", "wifi", "vpn"]);
        assert_eq!(merged[0].down_bps, 5.0);
        assert_eq!(merged[0].up_bps, 1000.0);
        assert_eq!(merged[2].down_bps, 0.0);
    }

    #[test]
    fn negative_rates_are_clamped() {
        let merged = merge_adapters(vec![("a".into(), -3.0)], vec![("a".into(), -1.0)]);
        assert_eq!((merged[0].down_bps, merged[0].up_bps), (0.0, 0.0));
    }
}
