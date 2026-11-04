use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use windows::Win32::Foundation::NO_ERROR;
use windows::Win32::NetworkManagement::IpHelper::{
    FreeMibTable, GetIfEntry2, GetIfTable2, GetNetworkConnectivityHint, MIB_IF_ROW2, MIB_IF_TABLE2,
};
use windows::Win32::Networking::WinSock::{
    NL_NETWORK_CONNECTIVITY_HINT, NL_NETWORK_CONNECTIVITY_LEVEL_HINT,
    NetworkConnectivityLevelHintConstrainedInternetAccess,
    NetworkConnectivityLevelHintInternetAccess, NetworkConnectivityLevelHintLocalAccess,
    NetworkConnectivityLevelHintNone,
};

use crate::metric::{AdapterRate, MetricKey, Snapshot, Source, SourceError, SourceId};

const INTERFACES_REFRESH: Duration = Duration::from_secs(30);
const HARDWARE_INTERFACE: u8 = 0x01;
const FILTER_INTERFACE: u8 = 0x02;
const IF_TYPE_SOFTWARE_LOOPBACK: u32 = 24;

/// Rates come from each chosen interface's byte counters, read one by one:
/// listing every interface, or PDH's `Network Interface` object, costs
/// milliseconds a tick.
pub struct NetworkSource {
    interfaces: Vec<Interface>,
    listed: Option<Instant>,
    last: HashMap<u64, Counts>,
    adapters: Vec<String>,
    totals: Totals,
}

/// An interface by its LUID, named as PDH and the settings name it.
#[derive(Clone, Debug, PartialEq)]
pub struct Interface {
    luid: u64,
    name: String,
    hardware: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Counts {
    at: Instant,
    received: u64,
    sent: u64,
}

/// Bytes per second since `last`; `None` when the counters went back, as
/// after a driver reset, and must be read again before they mean a rate.
fn rates(last: Counts, now: Counts) -> Option<(f64, f64)> {
    let seconds = now.at.saturating_duration_since(last.at).as_secs_f64();
    if seconds <= 0.0 || now.received < last.received || now.sent < last.sent {
        return None;
    }
    Some((
        (now.received - last.received) as f64 / seconds,
        (now.sent - last.sent) as f64 / seconds,
    ))
}

/// The interfaces whose traffic counts: the chosen ones by name, or else the
/// physical ones, so VPN tunnels and virtual switches that relay the same
/// traffic are not counted twice; every interface when none is physical.
fn choose(interfaces: &[Interface], names: &[String]) -> Vec<Interface> {
    let kept: Vec<Interface> = if names.is_empty() {
        interfaces.iter().filter(|i| i.hardware).cloned().collect()
    } else {
        interfaces
            .iter()
            .filter(|i| names.contains(&i.name))
            .cloned()
            .collect()
    };
    if kept.is_empty() && names.is_empty() {
        return interfaces.to_vec();
    }
    kept
}

/// Bytes moved since the first sample, summed from the rates.
#[derive(Default)]
struct Totals {
    last: Option<Instant>,
    down: f64,
    up: f64,
}

const MAX_GAP_SECONDS: f64 = 10.0;

impl Totals {
    fn add(&mut self, now: Instant, down_bps: f64, up_bps: f64) {
        if let Some(last) = self.last {
            let seconds = now.duration_since(last).as_secs_f64().min(MAX_GAP_SECONDS);
            self.down += down_bps * seconds;
            self.up += up_bps * seconds;
        }
        self.last = Some(now);
    }
}

impl NetworkSource {
    /// `adapters` are PDH instance names, each read on its own and summed
    /// for the totals; none sums the hardware adapters.
    pub fn new(adapters: Vec<String>) -> Result<Self, SourceError> {
        let mut source = Self {
            interfaces: Vec::new(),
            listed: None,
            last: HashMap::new(),
            adapters,
            totals: Totals::default(),
        };
        source.rates();
        Ok(source)
    }

    /// Each chosen interface's name and rates since the last reading; zero
    /// for one read for the first time or whose counters went back.
    fn rates(&mut self) -> Vec<AdapterRate> {
        if self
            .listed
            .is_none_or(|t| t.elapsed() >= INTERFACES_REFRESH)
        {
            self.interfaces = list_interfaces();
            self.listed = Some(Instant::now());
            let live: HashSet<u64> = self.interfaces.iter().map(|i| i.luid).collect();
            self.last.retain(|luid, _| live.contains(luid));
        }
        let mut adapters: Vec<AdapterRate> = Vec::new();
        for interface in choose(&self.interfaces, &self.adapters) {
            let Some(counts) = read_interface(interface.luid) else {
                self.last.remove(&interface.luid);
                self.listed = None;
                continue;
            };
            let (down_bps, up_bps) = self
                .last
                .insert(interface.luid, counts)
                .and_then(|last| rates(last, counts))
                .unwrap_or_default();
            match adapters.iter_mut().find(|a| a.name == interface.name) {
                // Interfaces PDH would name alike are one adapter to the user.
                Some(a) => {
                    a.down_bps += down_bps;
                    a.up_bps += up_bps;
                }
                None => adapters.push(AdapterRate {
                    name: interface.name,
                    down_bps,
                    up_bps,
                }),
            }
        }
        adapters.sort_by(|a, b| (b.down_bps + b.up_bps).total_cmp(&(a.down_bps + a.up_bps)));
        adapters
    }
}

fn read_interface(luid: u64) -> Option<Counts> {
    let mut row = MIB_IF_ROW2::default();
    row.InterfaceLuid.Value = luid;
    unsafe { GetIfEntry2(&mut row) }.ok().ok()?;
    Some(Counts {
        at: Instant::now(),
        received: row.InOctets,
        sent: row.OutOctets,
    })
}

/// Every interface PDH lists under `Network Interface`: not loopback, and
/// not the filter layers stacked on an adapter.
fn list_interfaces() -> Vec<Interface> {
    let mut interfaces = Vec::new();
    let mut table: *mut MIB_IF_TABLE2 = std::ptr::null_mut();
    if unsafe { GetIfTable2(&mut table) }.is_err() || table.is_null() {
        return interfaces;
    }
    unsafe {
        let count = (*table).NumEntries as usize;
        let rows = std::slice::from_raw_parts((*table).Table.as_ptr(), count);
        for row in rows {
            let flags = row.InterfaceAndOperStatusFlags._bitfield;
            if flags & FILTER_INTERFACE != 0 || row.Type == IF_TYPE_SOFTWARE_LOOPBACK {
                continue;
            }
            interfaces.push(Interface {
                luid: row.InterfaceLuid.Value,
                name: pdh_instance_name(&description(row)),
                hardware: flags & HARDWARE_INTERFACE != 0,
            });
        }
        FreeMibTable(table.cast());
    }
    interfaces
}

fn description(row: &MIB_IF_ROW2) -> String {
    let len = row
        .Description
        .iter()
        .position(|&c| c == 0)
        .unwrap_or(row.Description.len());
    String::from_utf16_lossy(&row.Description[..len])
}

impl Source for NetworkSource {
    fn id(&self) -> SourceId {
        SourceId::Network
    }

    fn sample(&mut self, out: &mut Snapshot) -> Result<(), SourceError> {
        if let Some(level) = connectivity() {
            out.set(MetricKey::NetConnectivity, level);
        }
        let result = self.read(out);
        if result.is_err() {
            self.totals.last = None;
        }
        result
    }
}

/// As Windows judges it for the network icon in the tray.
fn connectivity() -> Option<f64> {
    let mut hint = NL_NETWORK_CONNECTIVITY_HINT::default();
    if unsafe { GetNetworkConnectivityHint(&mut hint) } != NO_ERROR {
        return None;
    }
    connectivity_level(hint.ConnectivityLevel)
}

/// `MetricKey::NetConnectivity` for a level; a captive portal counts as
/// the local network only.
pub fn connectivity_level(level: NL_NETWORK_CONNECTIVITY_LEVEL_HINT) -> Option<f64> {
    let local = [
        NetworkConnectivityLevelHintLocalAccess,
        NetworkConnectivityLevelHintConstrainedInternetAccess,
    ];
    if level == NetworkConnectivityLevelHintInternetAccess {
        Some(2.0)
    } else if local.contains(&level) {
        Some(1.0)
    } else if level == NetworkConnectivityLevelHintNone {
        Some(0.0)
    } else {
        None
    }
}

impl NetworkSource {
    fn read(&mut self, out: &mut Snapshot) -> Result<(), SourceError> {
        let adapters = self.rates();
        if adapters.is_empty() && !self.adapters.is_empty() {
            return Err(SourceError::Unavailable(crate::tr!(
                "Adapter \"{}\" not found",
                self.adapters.join(", ")
            )));
        }
        for (i, name) in (0..=u8::MAX).zip(&self.adapters) {
            if let Some(a) = adapters.iter().find(|a| &a.name == name) {
                out.set(MetricKey::NetAdapterDownBps(i), a.down_bps);
                out.set(MetricKey::NetAdapterUpBps(i), a.up_bps);
            }
        }
        let down: f64 = adapters.iter().map(|a| a.down_bps).sum();
        let up: f64 = adapters.iter().map(|a| a.up_bps).sum();
        out.set(MetricKey::NetDownBps, down);
        out.set(MetricKey::NetUpBps, up);
        self.totals.add(Instant::now(), down, up);
        out.set(MetricKey::NetDownTotalBytes, self.totals.down);
        out.set(MetricKey::NetUpTotalBytes, self.totals.up);
        out.adapters = adapters
            .into_iter()
            .filter(|a| a.down_bps + a.up_bps > 0.0)
            .collect();
        Ok(())
    }
}

/// PDH instance names of the physical adapters, sorted, for choosing one.
pub fn hardware_adapters() -> Vec<String> {
    let mut names: Vec<String> = list_interfaces()
        .into_iter()
        .filter(|i| i.hardware)
        .map(|i| i.name)
        .collect();
    names.sort();
    names.dedup();
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn totals_add_up_the_rates_over_time() {
        let mut totals = Totals::default();
        let start = Instant::now();
        totals.add(start, 1000.0, 10.0);
        assert_eq!(
            (totals.down, totals.up),
            (0.0, 0.0),
            "nothing before a second sample"
        );
        totals.add(start + Duration::from_secs(2), 500.0, 50.0);
        assert_eq!((totals.down, totals.up), (1000.0, 100.0));
    }

    fn counts(at: Instant, received: u64, sent: u64) -> Counts {
        Counts { at, received, sent }
    }

    #[test]
    fn rates_are_bytes_per_second_between_two_readings() {
        let start = Instant::now();
        let later = start + Duration::from_millis(500);
        assert_eq!(
            rates(counts(start, 1000, 10), counts(later, 3000, 60)),
            Some((4000.0, 100.0))
        );
    }

    #[test]
    fn counters_that_go_back_are_read_again_rather_than_wrapped() {
        let start = Instant::now();
        let later = start + Duration::from_secs(1);
        assert_eq!(rates(counts(start, 5000, 10), counts(later, 100, 20)), None);
        assert_eq!(rates(counts(start, 5000, 10), counts(later, 6000, 5)), None);
        assert_eq!(rates(counts(start, 1, 1), counts(start, 2, 2)), None);
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

    fn interface(luid: u64, name: &str, hardware: bool) -> Interface {
        Interface {
            luid,
            name: name.into(),
            hardware,
        }
    }

    fn names(chosen: &[Interface]) -> Vec<&str> {
        chosen.iter().map(|i| i.name.as_str()).collect()
    }

    #[test]
    fn only_physical_adapters_are_counted() {
        let all = [
            interface(1, "Realtek 2.5GbE", true),
            interface(2, "WireGuard Tunnel", false),
        ];
        assert_eq!(names(&choose(&all, &[])), ["Realtek 2.5GbE"]);
    }

    #[test]
    fn with_no_physical_adapter_every_one_counts() {
        let all = [interface(1, "a", false), interface(2, "b", false)];
        assert_eq!(names(&choose(&all, &[])), ["a", "b"], "never silently zero");
    }

    #[test]
    fn chosen_adapters_are_the_only_ones_counted_even_virtual_ones() {
        let all = [
            interface(1, "wifi", true),
            interface(2, "eth", true),
            interface(3, "vpn", false),
        ];
        let chosen = choose(&all, &["eth".into(), "vpn".into(), "unplugged".into()]);
        assert_eq!(names(&chosen), ["eth", "vpn"]);
    }

    #[test]
    fn a_missing_chosen_adapter_counts_nothing() {
        let all = [interface(1, "wifi", true)];
        assert!(choose(&all, &["eth".into()]).is_empty());
    }

    #[test]
    fn interface_names_match_live_pdh_instances() {
        let query = crate::pdh::Query::new().unwrap();
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
        let hardware = hardware_adapters();
        assert!(
            hardware.iter().any(|h| instances.contains(h)),
            "hardware {hardware:?} vs PDH {instances:?}"
        );
    }

    #[test]
    fn each_chosen_adapter_is_reported_by_its_place() {
        let mut source = NetworkSource::new(hardware_adapters()).unwrap();
        let mut s = Snapshot::default();
        std::thread::sleep(Duration::from_millis(500));
        if source.sample(&mut s).is_ok() {
            let down = s.get(MetricKey::NetDownBps).unwrap();
            let each: f64 = (0..source.adapters.len() as u8)
                .filter_map(|i| s.get(MetricKey::NetAdapterDownBps(i)))
                .sum();
            assert!((down - each).abs() < 1e-6, "{down} vs {each}");
        }
    }

    #[test]
    fn hardware_adapters_are_listed_sorted() {
        let list = hardware_adapters();
        let mut sorted = list.clone();
        sorted.sort();
        assert_eq!(list, sorted);
    }

    #[test]
    fn connectivity_reads_as_internet_local_or_none() {
        assert_eq!(
            connectivity_level(NetworkConnectivityLevelHintInternetAccess),
            Some(2.0)
        );
        for local in [
            NetworkConnectivityLevelHintLocalAccess,
            NetworkConnectivityLevelHintConstrainedInternetAccess,
        ] {
            assert_eq!(connectivity_level(local), Some(1.0));
        }
        assert_eq!(
            connectivity_level(NetworkConnectivityLevelHintNone),
            Some(0.0)
        );
        assert_eq!(
            connectivity_level(NL_NETWORK_CONNECTIVITY_LEVEL_HINT(0)),
            None
        );
        // Unknown until Windows has judged the network, so only check it answers.
        let _ = connectivity();
    }
}
