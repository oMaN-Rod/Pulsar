use std::collections::VecDeque;
use std::net::{Ipv4Addr, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use windows::Win32::Foundation::HANDLE;
use windows::Win32::NetworkManagement::IpHelper::{
    ICMP_ECHO_REPLY, IcmpCloseHandle, IcmpCreateFile, IcmpSendEcho,
};

use crate::metric::{MetricKey, Snapshot, Source, SourceError, SourceId};

const TIMEOUT_MS: u32 = 1000;
const WINDOW: usize = 20;
const IP_SUCCESS: u32 = 0;

#[derive(Default)]
struct PingState {
    /// Most recent attempts, oldest first; `None` = no reply.
    recent: VecDeque<Option<f64>>,
    error: Option<String>,
}

/// Pings an IPv4 host from a background thread so slow replies never delay
/// other sources. Dropping the source stops the thread after its current wait.
pub struct PingSource {
    state: Arc<Mutex<PingState>>,
    stop: Arc<AtomicBool>,
}

impl PingSource {
    pub fn spawn(host: String, interval: Duration) -> Self {
        let state = Arc::new(Mutex::new(PingState::default()));
        let stop = Arc::new(AtomicBool::new(false));
        let (worker_state, worker_stop) = (state.clone(), stop.clone());
        thread::Builder::new()
            .name("perfbar-ping".into())
            .spawn(move || run(&host, interval, &worker_state, &worker_stop))
            .expect("spawn ping thread");
        Self { state, stop }
    }
}

impl Drop for PingSource {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

impl Source for PingSource {
    fn id(&self) -> SourceId {
        SourceId::Ping
    }

    fn sample(&mut self, out: &mut Snapshot) -> Result<(), SourceError> {
        let state = self.state.lock().unwrap();
        if let Some(error) = &state.error {
            return Err(SourceError::Unavailable(error.clone()));
        }
        let Some(latest) = state.recent.back() else {
            return Err(SourceError::Unavailable("Waiting for first reply".into()));
        };
        if let Some(ms) = latest {
            out.set(MetricKey::PingMs, *ms);
        }
        out.set(MetricKey::PingLossPercent, loss_percent(&state.recent));
        Ok(())
    }
}

pub fn loss_percent(recent: &VecDeque<Option<f64>>) -> f64 {
    if recent.is_empty() {
        return 0.0;
    }
    recent.iter().filter(|r| r.is_none()).count() as f64 / recent.len() as f64 * 100.0
}

fn resolve(host: &str) -> Option<Ipv4Addr> {
    if let Ok(ip) = host.parse() {
        return Some(ip);
    }
    (host, 0)
        .to_socket_addrs()
        .ok()?
        .find_map(|a| match a.ip() {
            std::net::IpAddr::V4(ip) => Some(ip),
            std::net::IpAddr::V6(_) => None,
        })
}

fn run(host: &str, interval: Duration, state: &Mutex<PingState>, stop: &AtomicBool) {
    let handle = match unsafe { IcmpCreateFile() } {
        Ok(h) => h,
        Err(e) => {
            state.lock().unwrap().error = Some(format!("ICMP unavailable: {e}"));
            return;
        }
    };
    let mut target = None;
    while !stop.load(Ordering::Relaxed) {
        if target.is_none() {
            target = resolve(host);
        }
        match target {
            None => state.lock().unwrap().error = Some(format!("Cannot resolve {host} (IPv4)")),
            Some(ip) => {
                let reply = echo(handle, ip);
                let mut s = state.lock().unwrap();
                s.error = None;
                if s.recent.len() == WINDOW {
                    s.recent.pop_front();
                }
                s.recent.push_back(reply);
                if reply.is_none() {
                    target = None;
                }
            }
        }
        thread::sleep(interval);
    }
    let _ = unsafe { IcmpCloseHandle(handle) };
}

fn echo(handle: HANDLE, ip: Ipv4Addr) -> Option<f64> {
    let payload = [0u8; 32];
    let mut reply = vec![0u8; size_of::<ICMP_ECHO_REPLY>() + payload.len() + 8];
    let count = unsafe {
        IcmpSendEcho(
            handle,
            u32::from_ne_bytes(ip.octets()),
            payload.as_ptr().cast(),
            payload.len() as u16,
            None,
            reply.as_mut_ptr().cast(),
            reply.len() as u32,
            TIMEOUT_MS,
        )
    };
    if count == 0 {
        return None;
    }
    let reply = unsafe { reply.as_ptr().cast::<ICMP_ECHO_REPLY>().read_unaligned() };
    (reply.Status == IP_SUCCESS).then_some(reply.RoundTripTime as f64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loss_counts_missing_replies() {
        let recent: VecDeque<Option<f64>> = [Some(10.0), None, Some(12.0), None].into();
        assert_eq!(loss_percent(&recent), 50.0);
        assert_eq!(loss_percent(&VecDeque::new()), 0.0);
    }

    #[test]
    fn resolves_literal_ipv4() {
        assert_eq!(resolve("127.0.0.1"), Some(Ipv4Addr::LOCALHOST));
        assert_eq!(resolve("definitely-not-a-host.invalid"), None);
    }
}
