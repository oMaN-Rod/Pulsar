use std::collections::HashSet;
use std::time::{Duration, Instant};

use windows::Win32::NetworkManagement::IpHelper::{FreeMibTable, GetIfTable2, MIB_IF_TABLE2};

use crate::metric::{AdapterRate, MetricKey, Snapshot, Source, SourceError, SourceId};
use crate::pdh::{Counter, Query};

const HARDWARE_REFRESH: Duration = Duration::from_secs(30);

pub struct NetworkSource {
    query: Query,
    received: Counter,
    sent: Counter,
    hardware: HashSet<String>,
    hardware_refreshed: Option<Instant>,
    adapter: Option<String>,
}

impl NetworkSource {
    /// `adapter` limits the totals to one PDH instance; `None` sums hardware adapters.
    pub fn new(adapter: Option<String>) -> Result<Self, SourceError> {
        let query = Query::new()?;
        let received = query.add(r"\Network Interface(*)\Bytes Received/sec")?;
        let sent = query.add(r"\Network Interface(*)\Bytes Sent/sec")?;
        query.collect()?;
        Ok(Self {
            query,
            received,
            sent,
            hardware: HashSet::new(),
            hardware_refreshed: None,
            adapter,
        })
    }
}

impl Source for NetworkSource {
    fn id(&self) -> SourceId {
        SourceId::Network
    }

    fn sample(&mut self, out: &mut Snapshot) -> Result<(), SourceError> {
        if self
            .hardware_refreshed
            .is_none_or(|t| t.elapsed() >= HARDWARE_REFRESH)
        {
            self.hardware = hardware_instance_names();
            self.hardware_refreshed = Some(Instant::now());
        }
        self.query.collect()?;
        let all = merge_adapters(self.received.array()?, self.sent.array()?);
        let adapters = match &self.adapter {
            Some(name) => select_adapter(all, name)
                .ok_or_else(|| SourceError::Unavailable(format!("Adapter \"{name}\" not found")))?,
            None => select_hardware(all, &self.hardware),
        };
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

/// Keeps physical adapters only, so VPN tunnels and virtual switches that
/// relay the same traffic are not counted twice. When no adapter matches
/// the hardware list (the query failed, or PDH names an adapter
/// differently), every adapter is kept rather than reporting zero traffic.
pub fn select_hardware(adapters: Vec<AdapterRate>, hardware: &HashSet<String>) -> Vec<AdapterRate> {
    if !adapters.iter().any(|a| hardware.contains(&a.name)) {
        return adapters;
    }
    adapters
        .into_iter()
        .filter(|a| hardware.contains(&a.name))
        .collect()
}

/// Only `name`'s traffic; `None` when that adapter is not present.
pub fn select_adapter(adapters: Vec<AdapterRate>, name: &str) -> Option<Vec<AdapterRate>> {
    let kept: Vec<AdapterRate> = adapters.into_iter().filter(|a| a.name == name).collect();
    (!kept.is_empty()).then_some(kept)
}

/// PDH instance names of the physical adapters, sorted, for choosing one.
pub fn hardware_adapters() -> Vec<String> {
    let mut names: Vec<String> = hardware_instance_names().into_iter().collect();
    names.sort();
    names
}

/// PDH derives `Network Interface` instance names from the adapter
/// description, replacing characters that are reserved in counter paths.
pub fn pdh_instance_name(description: &str) -> String {
    description
        .chars()
        .map(|c| match c {
            '(' => '[',
            ')' => ']',
            '#' | '/' | '\\' => '_',
            c => c,
        })
        .collect()
}

fn hardware_instance_names() -> HashSet<String> {
    const HARDWARE_INTERFACE: u8 = 0x01;
    const FILTER_INTERFACE: u8 = 0x02;

    let mut names = HashSet::new();
    let mut table: *mut MIB_IF_TABLE2 = std::ptr::null_mut();
    if unsafe { GetIfTable2(&mut table) }.is_err() || table.is_null() {
        return names;
    }
    unsafe {
        let count = (*table).NumEntries as usize;
        let rows = std::slice::from_raw_parts((*table).Table.as_ptr(), count);
        for row in rows {
            let flags = row.InterfaceAndOperStatusFlags._bitfield;
            if flags & HARDWARE_INTERFACE == 0 || flags & FILTER_INTERFACE != 0 {
                continue;
            }
            let len = row
                .Description
                .iter()
                .position(|&c| c == 0)
                .unwrap_or(row.Description.len());
            names.insert(pdh_instance_name(&String::from_utf16_lossy(
                &row.Description[..len],
            )));
        }
        FreeMibTable(table.cast());
    }
    names
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

    #[test]
    fn instance_names_replace_reserved_characters() {
        assert_eq!(
            pdh_instance_name("Intel(R) Wi-Fi 6 AX201 160MHz"),
            "Intel[R] Wi-Fi 6 AX201 160MHz"
        );
        assert_eq!(
            pdh_instance_name("Hyper-V Virtual Ethernet Adapter #3"),
            "Hyper-V Virtual Ethernet Adapter _3"
        );
        assert_eq!(pdh_instance_name("a/b\\c"), "a_b_c");
    }

    #[test]
    fn only_hardware_adapters_are_counted() {
        let rate = |name: &str| AdapterRate {
            name: name.into(),
            down_bps: 100.0,
            up_bps: 10.0,
        };
        let hardware = HashSet::from(["Realtek 2.5GbE".to_string()]);
        let kept = select_hardware(
            vec![rate("Realtek 2.5GbE"), rate("WireGuard Tunnel")],
            &hardware,
        );
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].name, "Realtek 2.5GbE");
    }

    #[test]
    fn no_matching_hardware_keeps_everything() {
        let rate = AdapterRate {
            name: "Adapter PDH names differently".into(),
            down_bps: 5.0,
            up_bps: 1.0,
        };
        let hardware = HashSet::from(["Some Other Adapter".to_string()]);
        let kept = select_hardware(vec![rate], &hardware);
        assert_eq!(kept.len(), 1, "never silently report zero traffic");
    }

    #[test]
    fn empty_hardware_list_keeps_everything() {
        let rate = AdapterRate {
            name: "x".into(),
            down_bps: 1.0,
            up_bps: 1.0,
        };
        assert_eq!(select_hardware(vec![rate], &HashSet::new()).len(), 1);
    }

    #[test]
    fn hardware_names_match_live_pdh_instances() {
        let query = Query::new().unwrap();
        let counter = query
            .add(r"\Network Interface(*)\Bytes Received/sec")
            .unwrap();
        query.collect().unwrap();
        std::thread::sleep(Duration::from_millis(500));
        query.collect().unwrap();
        let instances: HashSet<String> = counter
            .array()
            .unwrap()
            .into_iter()
            .map(|(name, _)| name)
            .collect();
        let hardware = hardware_instance_names();
        assert!(
            hardware.iter().any(|h| instances.contains(h)),
            "hardware {hardware:?} vs PDH {instances:?}"
        );
    }

    #[test]
    fn a_chosen_adapter_is_the_only_one_counted() {
        let rate = |name: &str, down: f64| AdapterRate {
            name: name.into(),
            down_bps: down,
            up_bps: 1.0,
        };
        let kept = select_adapter(vec![rate("wifi", 10.0), rate("eth", 5.0)], "eth").unwrap();
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].down_bps, 5.0);
    }

    #[test]
    fn a_missing_adapter_is_an_error() {
        let rate = AdapterRate {
            name: "wifi".into(),
            down_bps: 1.0,
            up_bps: 1.0,
        };
        assert_eq!(select_adapter(vec![rate], "eth"), None);
    }

    #[test]
    fn hardware_adapters_are_listed_sorted() {
        let list = hardware_adapters();
        let mut sorted = list.clone();
        sorted.sort();
        assert_eq!(list, sorted);
    }
}
