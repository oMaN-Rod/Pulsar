use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use perfbar_core::config::Config;
use perfbar_core::metric::Snapshot;
use perfbar_core::sampler::Sampler;
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::PostMessageW;

use crate::messages::WM_APP_SNAPSHOT;

/// Samples on its own thread at a fixed rate, sending each snapshot over a
/// channel and posting `WM_APP_SNAPSHOT` so the UI thread wakes to read it.
pub struct SamplerThread {
    pub snapshots: Receiver<Snapshot>,
    stop: Option<Sender<()>>,
    handle: Option<JoinHandle<()>>,
}

impl SamplerThread {
    pub fn spawn(config: &Config, notify: Option<HWND>) -> Self {
        let mut sampler = Sampler::from_config(config);
        let interval = Duration::from_millis(config.general.sample_interval_ms.into());
        let notify = notify.map(|h| h.0 as isize);
        let (tx, snapshots) = mpsc::channel();
        let (stop, stopped) = mpsc::channel::<()>();
        let handle = thread::Builder::new()
            .name("perfbar-sampler".into())
            .spawn(move || {
                let mut next = Instant::now();
                loop {
                    if tx.send(sampler.sample()).is_err() {
                        return;
                    }
                    if let Some(hwnd) = notify {
                        unsafe {
                            let _ = PostMessageW(
                                Some(HWND(hwnd as *mut _)),
                                WM_APP_SNAPSHOT,
                                WPARAM(0),
                                LPARAM(0),
                            );
                        }
                    }
                    next += interval;
                    let wait = next.saturating_duration_since(Instant::now());
                    match stopped.recv_timeout(wait) {
                        Err(RecvTimeoutError::Timeout) => {}
                        _ => return,
                    }
                    if Instant::now() > next + interval {
                        next = Instant::now();
                    }
                }
            })
            .expect("spawn sampler thread");
        Self {
            snapshots,
            stop: Some(stop),
            handle: Some(handle),
        }
    }
}

impl Drop for SamplerThread {
    fn drop(&mut self) {
        drop(self.stop.take());
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use perfbar_core::metric::MetricKey;

    fn fast_config() -> Config {
        let mut config = Config::default();
        config.general.sample_interval_ms = 500;
        config
    }

    #[test]
    fn delivers_snapshots_on_the_interval() {
        let thread = SamplerThread::spawn(&fast_config(), None);
        let first = thread
            .snapshots
            .recv_timeout(Duration::from_secs(3))
            .unwrap();
        assert!(first.get(MetricKey::MemUsedPercent).is_some());
        let start = Instant::now();
        thread
            .snapshots
            .recv_timeout(Duration::from_secs(3))
            .unwrap();
        let gap = start.elapsed();
        assert!(gap < Duration::from_millis(900), "{gap:?}");
    }

    #[test]
    fn drop_stops_promptly() {
        let thread = SamplerThread::spawn(&fast_config(), None);
        thread
            .snapshots
            .recv_timeout(Duration::from_secs(3))
            .unwrap();
        let start = Instant::now();
        drop(thread);
        assert!(start.elapsed() < Duration::from_millis(600));
    }
}
