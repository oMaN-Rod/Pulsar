//! A child of the taskbar can hold up Explorer if Pulsar's UI thread stops
//! pumping messages, so while attached a second thread checks on it, and
//! restarts Pulsar as a separate window when it stays silent or a change of
//! parent never finishes.

use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread::{self, Thread};
use std::time::Duration;

use pulsar_core::single_instance::SingleInstance;
use windows::Win32::Foundation::{ERROR_TIMEOUT, GetLastError, HWND, LPARAM, WPARAM};
use windows::Win32::System::Diagnostics::Debug::IsDebuggerPresent;
use windows::Win32::System::Threading::{GetCurrentProcess, TerminateProcess};
use windows::Win32::UI::WindowsAndMessaging::{SMTO_NORMAL, SendMessageTimeoutW, WM_NULL};

pub const RECOVERED_ARG: &str = "--recovered";
const POLL: Duration = Duration::from_secs(2);
const REPLY_MS: u32 = 1000;
/// Consecutive bad checks before restarting. One alone proves little: the
/// first check after the PC wakes from sleep can time out.
const MISSES: u32 = 5;
const RESTART_WAIT: Duration = Duration::from_millis(100);
const RESTART_TRIES: u32 = 50;

#[derive(Default)]
struct Shared {
    armed: AtomicBool,
    /// While nobody can see the overlay; a change of parent is still watched.
    paused: AtomicBool,
    /// Odd while a change of parent is under way.
    attaches: AtomicU64,
}

impl Shared {
    fn checks_replies(&self) -> bool {
        self.armed.load(Ordering::Relaxed) && !self.paused.load(Ordering::Relaxed)
    }

    fn watching(&self) -> bool {
        self.checks_replies() || self.attaches.load(Ordering::Relaxed) % 2 == 1
    }
}

pub struct Watchdog {
    host: isize,
    shared: Arc<Shared>,
    thread: Option<Thread>,
}

impl Watchdog {
    pub fn new(host: HWND) -> Self {
        Self {
            host: host.0 as isize,
            shared: Arc::default(),
            thread: None,
        }
    }

    pub fn arm(&mut self, armed: bool) {
        self.shared.armed.store(armed, Ordering::Relaxed);
        if armed && self.thread.is_none() {
            let (host, shared) = (self.host, self.shared.clone());
            self.thread = thread::Builder::new()
                .name("pulsar-watchdog".into())
                .spawn(move || watch(host, &shared))
                .ok()
                .map(|handle| handle.thread().clone());
        }
        self.wake();
    }

    /// While the display is off or the session locked, the thread sleeps
    /// instead of checking on the UI every two seconds.
    pub fn pause(&self, paused: bool) {
        self.shared.paused.store(paused, Ordering::Relaxed);
        self.wake();
    }

    fn wake(&self) {
        if let Some(thread) = &self.thread {
            thread.unpark();
        }
    }

    /// Runs a change of parent. It waits on Explorer, and `answers` cannot see
    /// that wait, so it is timed separately.
    pub fn attaching<T>(&self, change: impl FnOnce() -> T) -> T {
        self.shared.attaches.fetch_add(1, Ordering::Relaxed);
        self.wake();
        let result = change();
        self.shared.attaches.fetch_add(1, Ordering::Relaxed);
        result
    }
}

#[derive(Default)]
struct Health {
    silent: u32,
    attach_seen: u64,
    attach_stuck: u32,
}

impl Health {
    fn must_restart(&mut self, answered: bool, attaches: u64) -> bool {
        self.silent = if answered { 0 } else { self.silent + 1 };
        self.attach_stuck = if attaches % 2 == 1 && attaches == self.attach_seen {
            self.attach_stuck + 1
        } else {
            0
        };
        self.attach_seen = attaches;
        self.silent >= MISSES || self.attach_stuck >= MISSES
    }
}

fn watch(host: isize, shared: &Shared) {
    let mut health = Health::default();
    loop {
        if !shared.watching() {
            health = Health::default();
            thread::park();
            continue;
        }
        thread::sleep(POLL);
        if unsafe { IsDebuggerPresent() }.as_bool() {
            health = Health::default();
            continue;
        }
        let answered = !shared.checks_replies() || answers(host);
        if health.must_restart(answered, shared.attaches.load(Ordering::Relaxed)) {
            restart();
        }
    }
}

/// A thread waiting on a call into another process still answers this, so
/// a hung Explorer does not count against Pulsar.
fn answers(host: isize) -> bool {
    let sent = unsafe {
        SendMessageTimeoutW(
            HWND(host as *mut _),
            WM_NULL,
            WPARAM(0),
            LPARAM(0),
            SMTO_NORMAL,
            REPLY_MS,
            None,
        )
    };
    sent.0 != 0 || unsafe { GetLastError() } != ERROR_TIMEOUT
}

/// Takes no locks: the hung thread may hold them. The new process does the logging.
fn restart() {
    if let Ok(exe) = std::env::current_exe() {
        let _ = Command::new(exe).arg(RECOVERED_ARG).spawn();
    }
    unsafe {
        let _ = TerminateProcess(GetCurrentProcess(), 1);
    }
}

pub fn acquire_after_restart(name: &str) -> Option<SingleInstance> {
    (0..RESTART_TRIES).find_map(|_| {
        SingleInstance::acquire(name).or_else(|| {
            thread::sleep(RESTART_WAIT);
            None
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restarts_only_after_consecutive_silence() {
        let mut health = Health::default();
        for _ in 1..MISSES {
            assert!(!health.must_restart(false, 0));
        }
        assert!(!health.must_restart(true, 0));
        for _ in 1..MISSES {
            assert!(!health.must_restart(false, 0));
        }
        assert!(health.must_restart(false, 0));
    }

    #[test]
    fn a_stuck_attach_restarts_even_while_answering() {
        let mut health = Health::default();
        assert!(!health.must_restart(true, 1));
        for _ in 1..MISSES {
            assert!(!health.must_restart(true, 1));
        }
        assert!(health.must_restart(true, 1));
    }

    #[test]
    fn it_sleeps_while_disarmed_or_unseen_but_watches_a_change_of_parent() {
        let shared = Shared::default();
        assert!(!shared.watching());
        shared.armed.store(true, Ordering::Relaxed);
        assert!(shared.watching());
        shared.paused.store(true, Ordering::Relaxed);
        assert!(!shared.watching());
        shared.attaches.store(1, Ordering::Relaxed);
        assert!(shared.watching() && !shared.checks_replies());
        shared.attaches.store(2, Ordering::Relaxed);
        assert!(!shared.watching());
    }

    #[test]
    fn attaches_that_finish_never_restart() {
        let mut health = Health::default();
        for attaches in (1..100).step_by(2) {
            assert!(!health.must_restart(true, attaches));
        }
        for _ in 0..100 {
            assert!(!health.must_restart(true, 100));
        }
    }
}
