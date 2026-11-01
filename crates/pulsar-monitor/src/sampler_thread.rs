use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use pulsar_core::config::Config;
use pulsar_core::gpu_slots::GpuSlots;
use pulsar_core::metric::Snapshot;
use pulsar_core::sampler::Sampler;
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::PostMessageW;

use crate::messages::WM_APP_SNAPSHOT;

/// Samples on its own thread at a fixed rate, sending each snapshot over a
/// channel and posting `WM_APP_SNAPSHOT` so the UI thread wakes to read it.
pub struct SamplerThread {
    pub snapshots: Receiver<Snapshot>,
    /// Set while a popup lists processes; the sampler collects them only then.
    pub processes: Arc<AtomicBool>,
    /// Set while the details window is open: everything is sampled, not
    /// just what the overlay shows.
    pub extras: Arc<AtomicBool>,
    stop: Option<Sender<()>>,
    handle: Option<JoinHandle<()>>,
}

impl SamplerThread {
    pub fn spawn(config: &Config, gpus: GpuSlots, notify: Option<HWND>) -> Self {
        let interval = Duration::from_millis(config.general.sample_interval_ms.into());
        let config = config.clone();
        Self::start(
            move || Sampler::from_config(&config).with_gpu_slots(gpus),
            interval,
            notify,
        )
    }

    /// Building a sampler can wait on a sleeping disk, so the new thread does it.
    fn start(
        build: impl FnOnce() -> Sampler + Send + 'static,
        interval: Duration,
        notify: Option<HWND>,
    ) -> Self {
        let processes = Arc::<AtomicBool>::default();
        let extras = Arc::<AtomicBool>::default();
        let flags = (processes.clone(), extras.clone());
        let notify = notify.map(|h| h.0 as isize);
        let (tx, snapshots) = mpsc::channel();
        let (stop, stopped) = mpsc::channel::<()>();
        let handle = thread::Builder::new()
            .name("pulsar-sampler".into())
            .spawn(move || {
                let mut sampler = build().with_flags(flags.0, flags.1);
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
            processes,
            extras,
            stop: Some(stop),
            handle: Some(handle),
        }
    }

    /// Lets the thread finish by itself: its current sample can take seconds.
    pub fn retire(&mut self) {
        self.handle = None;
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
    use pulsar_core::metric::{MetricKey, Source, SourceError, SourceId};

    fn fast_config() -> Config {
        let mut config = Config::default();
        config.general.sample_interval_ms = 500;
        config
    }

    #[test]
    fn delivers_snapshots_on_the_interval() {
        let thread = SamplerThread::spawn(&fast_config(), GpuSlots::new(), None);
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
    fn processes_are_collected_only_while_flagged() {
        use std::sync::atomic::Ordering;
        let thread = SamplerThread::spawn(&fast_config(), GpuSlots::new(), None);
        let first = thread
            .snapshots
            .recv_timeout(Duration::from_secs(3))
            .unwrap();
        assert!(first.processes.is_empty());
        thread.processes.store(true, Ordering::Relaxed);
        let listed = (0..6).any(|_| {
            thread
                .snapshots
                .recv_timeout(Duration::from_secs(3))
                .is_ok_and(|s| !s.processes.is_empty())
        });
        assert!(
            listed,
            "processes appear once flagged (after one priming sample)"
        );
    }

    const INTERVAL: Duration = Duration::from_millis(500);

    struct Blocked {
        release: Receiver<()>,
        limit: Duration,
    }

    impl Source for Blocked {
        fn id(&self) -> SourceId {
            SourceId::Processes
        }
        fn sample(&mut self, _: &mut Snapshot) -> Result<(), SourceError> {
            let _ = self.release.recv_timeout(self.limit);
            Ok(())
        }
    }

    #[test]
    fn a_slow_build_does_not_hold_up_the_caller() {
        let (release, released) = mpsc::channel::<()>();
        let thread = SamplerThread::start(
            move || {
                let _ = released.recv();
                Sampler::new(Vec::new(), None)
            },
            INTERVAL,
            None,
        );
        assert!(thread.snapshots.try_recv().is_err());
        drop(release);
        assert!(
            thread
                .snapshots
                .recv_timeout(Duration::from_secs(3))
                .is_ok()
        );
    }

    #[test]
    fn retiring_does_not_wait_for_a_sample() {
        let (_keep_blocked, release) = mpsc::channel::<()>();
        let limit = Duration::from_secs(5);
        let mut thread = SamplerThread::start(
            move || Sampler::new(vec![Box::new(Blocked { release, limit })], None),
            INTERVAL,
            None,
        );
        let start = Instant::now();
        thread.retire();
        assert!(start.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn drop_stops_promptly() {
        let thread = SamplerThread::spawn(&fast_config(), GpuSlots::new(), None);
        thread
            .snapshots
            .recv_timeout(Duration::from_secs(3))
            .unwrap();
        let start = Instant::now();
        drop(thread);
        assert!(start.elapsed() < Duration::from_millis(600));
    }
}
