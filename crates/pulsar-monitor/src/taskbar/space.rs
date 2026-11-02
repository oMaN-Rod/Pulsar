//! Reads where the taskbar's own buttons are, on a thread of its own: each read
//! is a call into Explorer that can take as long as Explorer does. Only plain
//! rectangles leave that thread, and the UI thread never waits for them. The
//! watchdog cannot see a wait in another process, so a read that never returns
//! is noticed here, and that thread is left behind.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use windows::Win32::Foundation::HWND;
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize,
};
use windows::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW};
use windows::Win32::UI::Accessibility::{
    CUIAutomation8, IUIAutomation2, IUIAutomationCacheRequest, IUIAutomationCondition,
    IUIAutomationElement, TreeScope_Children, TreeScope_Descendants, UIA_AutomationIdPropertyId,
    UIA_BoundingRectanglePropertyId, UIA_IsOffscreenPropertyId,
};
use windows::Win32::UI::WindowsAndMessaging::FindWindowExW;
use windows::core::{PCWSTR, Result, w};

use super::placement::Rect32;
use super::rect32;

const UIA_TIMEOUT_MS: u32 = 1000;
const IDLE_WAIT: Duration = Duration::from_secs(3600);
pub const PACE_ACTIVE: Duration = Duration::from_millis(500);
pub const PACE_SLACK: Duration = Duration::from_millis(2000);
pub const PACE_HIDDEN: Duration = Duration::from_millis(2000);
pub const PACE_UNKNOWN: Duration = Duration::from_millis(5000);
/// Two timed-out calls in one read, and some to spare.
const STUCK_AFTER: Duration = Duration::from_secs(5);
/// Longer than the slowest pace at which rectangles are expected.
const MAX_AGE: Duration = Duration::from_secs(6);
const FAILURES_BEFORE_UNKNOWN: u32 = 3;
const RETRY_AFTER: Duration = Duration::from_secs(30);
/// Threads left behind on a stuck read, until the taskbar changes.
const MAX_ABANDONED: u32 = 4;

const FRAME_ID: &str = "TaskbarFrame";
const TRAY_ID: &str = "SystemTrayIcon";
const ISLAND_BRIDGES: [PCWSTR; 2] = [
    w!("Windows.UI.Composition.DesktopWindowContentBridge"),
    w!("Microsoft.UI.Content.DesktopChildSiteBridge"),
];

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Rects {
    pub occupied: Vec<Rect32>,
    pub tray: Option<Rect32>,
    pub centred: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    Rects(Rects),
    NoFrame,
    Failed,
}

#[derive(Clone, Debug)]
pub struct Measurement {
    pub hwnd: isize,
    pub generation: u64,
    pub taken: Instant,
    pub outcome: Outcome,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Target {
    pub hwnd: isize,
    pub generation: u64,
    pub pace: Duration,
}

/// What one overlay last learned about its taskbar. Results from before the
/// taskbar's last change, or too old, are ignored; a few failures in a row, or
/// silence, mean the taskbar is no longer known.
#[derive(Clone, Debug, Default)]
pub struct Tracker {
    generation: u64,
    rects: Option<Rects>,
    fresh_until: Option<Instant>,
    last_taken: Option<Instant>,
    failures: u32,
}

impl Tracker {
    pub fn new(generation: u64) -> Self {
        Self {
            generation,
            ..Self::default()
        }
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn current(&self) -> Option<&Rects> {
        self.rects.as_ref()
    }

    /// Returns whether what is known changed.
    pub fn accept(&mut self, m: Measurement, now: Instant) -> bool {
        if m.generation != self.generation
            || now.saturating_duration_since(m.taken) > MAX_AGE
            || self.last_taken.is_some_and(|t| m.taken < t)
        {
            return false;
        }
        self.last_taken = Some(m.taken);
        match m.outcome {
            Outcome::Rects(rects) => {
                self.failures = 0;
                self.fresh_until = Some(m.taken + MAX_AGE);
                let changed = self.rects.as_ref() != Some(&rects);
                self.rects = Some(rects);
                changed
            }
            Outcome::NoFrame | Outcome::Failed => {
                self.failures += 1;
                self.failures >= FAILURES_BEFORE_UNKNOWN && self.rects.take().is_some()
            }
        }
    }

    /// Returns whether what was known has just run out.
    pub fn expire(&mut self, now: Instant) -> bool {
        let old = self.fresh_until.is_none_or(|until| now > until);
        old && self.rects.take().is_some()
    }

    /// Reading restarts after a pause; what is known may stand until it reports.
    pub fn resume(&mut self, now: Instant) {
        self.fresh_until = Some(now + MAX_AGE);
    }

    pub fn forget(&mut self) {
        *self = Self::new(self.generation);
    }
}

trait Probe {
    fn measure(&mut self, target: &Target) -> Outcome;
    fn retain(&mut self, targets: &[Target]);
}

type MakeProbe = Arc<dyn Fn() -> Option<Box<dyn Probe>> + Send + Sync>;

struct Worker {
    commands: Sender<Vec<Target>>,
    results: Receiver<Measurement>,
    /// Milliseconds since `epoch`, plus one, when the running read began; zero
    /// between reads.
    busy_since: Arc<AtomicU64>,
    epoch: Instant,
    handle: JoinHandle<()>,
}

impl Worker {
    fn spawn(make: &MakeProbe) -> Option<Self> {
        let (commands, inbox) = mpsc::channel();
        let (outbox, results) = mpsc::channel();
        let busy_since = Arc::new(AtomicU64::new(0));
        let epoch = Instant::now();
        let handle = thread::Builder::new()
            .name("pulsar-space".into())
            .spawn({
                let (make, busy) = (make.clone(), busy_since.clone());
                move || {
                    if let Some(probe) = make() {
                        run(probe, &inbox, &outbox, &busy, epoch);
                    }
                }
            })
            .ok()?;
        Some(Self {
            commands,
            results,
            busy_since,
            epoch,
            handle,
        })
    }

    fn stuck(&self, now: Instant) -> bool {
        let began = self.busy_since.load(Ordering::Relaxed);
        began != 0
            && now.saturating_duration_since(self.epoch + Duration::from_millis(began - 1))
                > STUCK_AFTER
    }
}

fn run(
    mut probe: Box<dyn Probe>,
    inbox: &Receiver<Vec<Target>>,
    outbox: &Sender<Measurement>,
    busy: &AtomicU64,
    epoch: Instant,
) {
    let mut targets: Vec<(Target, Instant)> = Vec::new();
    loop {
        let now = Instant::now();
        let wait = targets
            .iter()
            .map(|(_, due)| due.saturating_duration_since(now))
            .min()
            .unwrap_or(IDLE_WAIT);
        match inbox.recv_timeout(wait) {
            Ok(wanted) => {
                probe.retain(&wanted);
                targets = merged(&targets, wanted, Instant::now());
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
        for (target, due) in &mut targets {
            let now = Instant::now();
            if *due > now {
                continue;
            }
            busy.store(
                now.duration_since(epoch).as_millis() as u64 + 1,
                Ordering::Relaxed,
            );
            let outcome = probe.measure(target);
            busy.store(0, Ordering::Relaxed);
            let taken = Instant::now();
            let sent = outbox.send(Measurement {
                hwnd: target.hwnd,
                generation: target.generation,
                taken,
                outcome,
            });
            if sent.is_err() {
                return;
            }
            *due = taken + target.pace;
        }
    }
}

/// A taskbar already being read keeps its schedule, up to its new pace; a new
/// one is read at once.
fn merged(
    current: &[(Target, Instant)],
    wanted: Vec<Target>,
    now: Instant,
) -> Vec<(Target, Instant)> {
    wanted
        .into_iter()
        .map(|target| {
            let due = current
                .iter()
                .find(|(t, _)| t.hwnd == target.hwnd && t.generation == target.generation)
                .map_or(now, |(_, due)| (*due).min(now + target.pace));
            (target, due)
        })
        .collect()
}

#[derive(Debug, Default)]
pub struct Polled {
    pub measurements: Vec<Measurement>,
    /// The worker was given up on: nothing it read can be trusted to be current.
    pub abandoned: bool,
}

/// Starts a worker the first time there is a taskbar to read, and replaces one
/// that stops answering. Dropping this never waits for it.
pub struct Space {
    make: MakeProbe,
    worker: Option<Worker>,
    targets: Vec<Target>,
    abandoned: u32,
    retry_at: Option<Instant>,
}

impl Space {
    pub fn new() -> Self {
        Self::with_probe(Arc::new(|| {
            Uia::new().map(|uia| Box::new(uia) as Box<dyn Probe>)
        }))
    }

    fn with_probe(make: MakeProbe) -> Self {
        Self {
            make,
            worker: None,
            targets: Vec::new(),
            abandoned: 0,
            retry_at: None,
        }
    }

    pub fn set_targets(&mut self, targets: Vec<Target>) {
        if targets == self.targets {
            return;
        }
        self.targets = targets;
        if let Some(worker) = &self.worker {
            let _ = worker.commands.send(self.targets.clone());
        }
    }

    /// The taskbar was replaced, so a stuck read may not recur.
    pub fn taskbar_changed(&mut self) {
        self.abandoned = 0;
        self.retry_at = None;
    }

    pub fn poll(&mut self, now: Instant) -> Polled {
        let dead = self
            .worker
            .as_ref()
            .is_some_and(|w| w.stuck(now) || w.handle.is_finished());
        let mut polled = Polled::default();
        if dead {
            log::warn!("gave up on reading the taskbar's buttons; it did not answer");
            self.worker = None;
            self.abandoned += 1;
            self.retry_at = Some(now + RETRY_AFTER * self.abandoned);
            polled.abandoned = true;
        }
        if self.worker.is_none()
            && !self.targets.is_empty()
            && self.abandoned < MAX_ABANDONED
            && self.retry_at.is_none_or(|at| now >= at)
        {
            self.worker = Worker::spawn(&self.make);
            if let Some(worker) = &self.worker {
                let _ = worker.commands.send(self.targets.clone());
            }
        }
        if let Some(worker) = &self.worker {
            polled.measurements = worker.results.try_iter().collect();
        }
        polled
    }
}

struct Com;

impl Drop for Com {
    fn drop(&mut self) {
        unsafe { CoUninitialize() };
    }
}

struct Held {
    generation: u64,
    root: IUIAutomationElement,
    frame: IUIAutomationElement,
}

/// Field order matters: the COM objects go before `_com` uninitializes.
struct Uia {
    held: HashMap<isize, Held>,
    automation: IUIAutomation2,
    cache: IUIAutomationCacheRequest,
    everything: IUIAutomationCondition,
    _com: Com,
}

impl Uia {
    fn new() -> Option<Self> {
        unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }
            .ok()
            .ok()?;
        let com = Com;
        let build = || -> Result<_> {
            unsafe {
                let automation: IUIAutomation2 =
                    CoCreateInstance(&CUIAutomation8, None, CLSCTX_INPROC_SERVER)?;
                automation.SetConnectionTimeout(UIA_TIMEOUT_MS)?;
                automation.SetTransactionTimeout(UIA_TIMEOUT_MS)?;
                let cache = automation.CreateCacheRequest()?;
                cache.AddProperty(UIA_BoundingRectanglePropertyId)?;
                cache.AddProperty(UIA_AutomationIdPropertyId)?;
                cache.AddProperty(UIA_IsOffscreenPropertyId)?;
                let everything = automation.CreateTrueCondition()?;
                Ok((automation, cache, everything))
            }
        };
        match build() {
            Ok((automation, cache, everything)) => Some(Self {
                held: HashMap::new(),
                automation,
                cache,
                everything,
                _com: com,
            }),
            Err(e) => {
                log::warn!("UI Automation is unavailable: {e}");
                None
            }
        }
    }

    /// Starts below the taskbar window, at the island hosting its buttons, so
    /// that Pulsar's own child of the taskbar is never walked.
    fn discover(&self, bar: HWND) -> Result<Option<Held>> {
        let Some(site) = island(bar) else {
            return Ok(None);
        };
        unsafe {
            let root = self.automation.ElementFromHandle(site)?;
            let all =
                root.FindAllBuildCache(TreeScope_Descendants, &self.everything, &self.cache)?;
            for i in 0..all.Length()? {
                let element = all.GetElement(i)?;
                if element.CachedAutomationId()? == FRAME_ID {
                    return Ok(Some(Held {
                        generation: 0,
                        root,
                        frame: element,
                    }));
                }
            }
        }
        Ok(None)
    }

    fn read(&self, held: &Held) -> Result<Rects> {
        let mut seen = Vec::new();
        for parent in [&held.frame, &held.root] {
            unsafe {
                let found =
                    parent.FindAllBuildCache(TreeScope_Children, &self.everything, &self.cache)?;
                for i in 0..found.Length()? {
                    let element = found.GetElement(i)?;
                    seen.push(Seen {
                        id: element.CachedAutomationId()?.to_string(),
                        rect: rect32(element.CachedBoundingRectangle()?),
                        offscreen: element.CachedIsOffscreen()?.as_bool(),
                    });
                }
            }
        }
        Ok(rects_from(&seen, taskbar_is_centred()))
    }
}

impl Probe for Uia {
    fn measure(&mut self, target: &Target) -> Outcome {
        let current = self
            .held
            .get(&target.hwnd)
            .is_some_and(|h| h.generation == target.generation);
        if !current {
            self.held.remove(&target.hwnd);
            match self.discover(HWND(target.hwnd as *mut _)) {
                Ok(Some(held)) => {
                    self.held.insert(
                        target.hwnd,
                        Held {
                            generation: target.generation,
                            ..held
                        },
                    );
                }
                Ok(None) => return Outcome::NoFrame,
                Err(_) => return Outcome::Failed,
            }
        }
        let read = self.held.get(&target.hwnd).map(|held| self.read(held));
        match read {
            Some(Ok(rects)) if !rects.occupied.is_empty() => Outcome::Rects(rects),
            Some(Ok(_)) | None => Outcome::NoFrame,
            Some(Err(_)) => {
                self.held.remove(&target.hwnd);
                Outcome::Failed
            }
        }
    }

    fn retain(&mut self, targets: &[Target]) {
        self.held
            .retain(|hwnd, _| targets.iter().any(|t| t.hwnd == *hwnd));
    }
}

fn island(bar: HWND) -> Option<HWND> {
    let bridge = ISLAND_BRIDGES
        .iter()
        .find_map(|class| unsafe { FindWindowExW(Some(bar), None, *class, None) }.ok())?;
    let site = unsafe {
        FindWindowExW(
            Some(bridge),
            None,
            w!("Windows.UI.Input.InputSite.WindowClass"),
            None,
        )
    };
    Some(site.unwrap_or(bridge))
}

/// Windows 11 centres the taskbar unless told otherwise.
fn taskbar_is_centred() -> bool {
    let mut value = 1u32;
    let mut size = size_of::<u32>() as u32;
    let _ = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!(r"Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced"),
            w!("TaskbarAl"),
            RRF_RT_REG_DWORD,
            None,
            Some((&raw mut value).cast()),
            Some(&mut size),
        )
    };
    value != 0
}

struct Seen {
    id: String,
    rect: Rect32,
    offscreen: bool,
}

fn rects_from(seen: &[Seen], centred: bool) -> Rects {
    let mut rects = Rects {
        centred,
        ..Rects::default()
    };
    for s in seen {
        if s.offscreen || s.id == FRAME_ID {
            continue;
        }
        rects.occupied.push(s.rect);
        if s.id == TRAY_ID {
            rects.tray = Some(rects.tray.map_or(s.rect, |t| Rect32 {
                left: t.left.min(s.rect.left),
                top: t.top.min(s.rect.top),
                right: t.right.max(s.rect.right),
                bottom: t.bottom.max(s.rect.bottom),
            }));
        }
    }
    rects
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::taskbar::{discover, placement};
    use std::sync::Mutex;

    fn rect(left: i32, right: i32) -> Rect32 {
        Rect32 {
            left,
            top: 1032,
            right,
            bottom: 1080,
        }
    }

    fn measurement(generation: u64, taken: Instant, outcome: Outcome) -> Measurement {
        Measurement {
            hwnd: 1,
            generation,
            taken,
            outcome,
        }
    }

    fn known(x: i32) -> Outcome {
        Outcome::Rects(Rects {
            occupied: vec![rect(0, x)],
            tray: None,
            centred: false,
        })
    }

    #[test]
    fn a_result_is_kept_and_a_new_one_reports_a_change() {
        let now = Instant::now();
        let mut t = Tracker::new(1);
        assert!(t.accept(measurement(1, now, known(100)), now));
        assert!(t.current().is_some());
        assert!(!t.accept(measurement(1, now, known(100)), now));
        assert!(t.accept(measurement(1, now, known(200)), now));
    }

    #[test]
    fn a_result_from_an_earlier_generation_is_ignored() {
        let now = Instant::now();
        let mut t = Tracker::new(2);
        assert!(!t.accept(measurement(1, now, known(100)), now));
        assert!(t.current().is_none());
    }

    #[test]
    fn a_result_too_old_is_ignored() {
        let taken = Instant::now();
        let mut t = Tracker::new(1);
        let late = taken + MAX_AGE + Duration::from_millis(1);
        assert!(!t.accept(measurement(1, taken, known(100)), late));
        assert!(t.current().is_none());
    }

    #[test]
    fn results_out_of_order_keep_the_newer() {
        let taken = Instant::now();
        let mut t = Tracker::new(1);
        let later = taken + Duration::from_millis(500);
        t.accept(measurement(1, later, known(200)), later);
        assert!(!t.accept(measurement(1, taken, known(100)), later));
        assert_eq!(t.current().unwrap().occupied, [rect(0, 200)]);
    }

    #[test]
    fn one_failure_keeps_what_is_known_and_repeated_ones_forget_it() {
        let now = Instant::now();
        let mut t = Tracker::new(1);
        t.accept(measurement(1, now, known(100)), now);
        assert!(!t.accept(measurement(1, now, Outcome::Failed), now));
        assert!(!t.accept(measurement(1, now, Outcome::NoFrame), now));
        assert!(t.current().is_some());
        assert!(t.accept(measurement(1, now, Outcome::Failed), now));
        assert!(t.current().is_none());
        assert!(!t.accept(measurement(1, now, Outcome::Failed), now));
    }

    #[test]
    fn a_success_clears_the_failures() {
        let now = Instant::now();
        let mut t = Tracker::new(1);
        for _ in 0..FAILURES_BEFORE_UNKNOWN - 1 {
            t.accept(measurement(1, now, Outcome::Failed), now);
        }
        t.accept(measurement(1, now, known(100)), now);
        for _ in 0..FAILURES_BEFORE_UNKNOWN - 1 {
            t.accept(measurement(1, now, Outcome::Failed), now);
        }
        assert!(t.current().is_some());
    }

    #[test]
    fn silence_forgets_once_and_a_resume_buys_time() {
        let now = Instant::now();
        let mut t = Tracker::new(1);
        t.accept(measurement(1, now, known(100)), now);
        assert!(!t.expire(now + MAX_AGE));
        let later = now + MAX_AGE * 5;
        t.resume(later);
        assert!(!t.expire(later + MAX_AGE));
        assert!(t.current().is_some());
        assert!(t.expire(later + MAX_AGE + Duration::from_millis(1)));
        assert!(t.current().is_none());
        assert!(!t.expire(later + MAX_AGE * 2));
    }

    #[test]
    fn tray_buttons_join_up_and_containers_and_hidden_ones_are_dropped() {
        let seen = [
            Seen {
                id: FRAME_ID.into(),
                rect: rect(0, 1920),
                offscreen: false,
            },
            Seen {
                id: "StartButton".into(),
                rect: rect(0, 55),
                offscreen: false,
            },
            Seen {
                id: "Appid:x".into(),
                rect: rect(2000, 2044),
                offscreen: true,
            },
            Seen {
                id: TRAY_ID.into(),
                rect: rect(1663, 1701),
                offscreen: false,
            },
            Seen {
                id: TRAY_ID.into(),
                rect: rect(1845, 1896),
                offscreen: false,
            },
        ];
        let rects = rects_from(&seen, true);
        assert_eq!(
            rects.occupied,
            [rect(0, 55), rect(1663, 1701), rect(1845, 1896)]
        );
        assert_eq!(rects.tray, Some(rect(1663, 1896)));
        assert!(rects.centred);
    }

    /// Answers at once, except that its first read waits on `hang` if given one.
    struct FakeProbe {
        hang: Arc<Mutex<Option<Receiver<()>>>>,
        reads: Arc<AtomicU64>,
    }

    impl Probe for FakeProbe {
        fn measure(&mut self, target: &Target) -> Outcome {
            self.reads.fetch_add(1, Ordering::Relaxed);
            let waiting = self.hang.lock().unwrap().take();
            if let Some(release) = waiting {
                let _ = release.recv();
            }
            known(target.hwnd as i32)
        }
        fn retain(&mut self, _: &[Target]) {}
    }

    fn fake(hang: Option<Receiver<()>>) -> (Space, Arc<AtomicU64>) {
        let hang = Arc::new(Mutex::new(hang));
        let reads = Arc::new(AtomicU64::new(0));
        let probe_reads = reads.clone();
        let make: MakeProbe = Arc::new(move || {
            Some(Box::new(FakeProbe {
                hang: hang.clone(),
                reads: probe_reads.clone(),
            }) as Box<dyn Probe>)
        });
        (Space::with_probe(make), reads)
    }

    fn target(pace: Duration) -> Target {
        Target {
            hwnd: 7,
            generation: 3,
            pace,
        }
    }

    fn wait_for(mut done: impl FnMut() -> bool) -> bool {
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(5) {
            if done() {
                return true;
            }
            thread::sleep(Duration::from_millis(5));
        }
        false
    }

    #[test]
    fn no_worker_starts_without_a_taskbar_to_read() {
        let (mut space, _) = fake(None);
        let polled = space.poll(Instant::now());
        assert!(space.worker.is_none());
        assert!(polled.measurements.is_empty());
    }

    #[test]
    fn measurements_come_back_tagged() {
        let (mut space, _) = fake(None);
        space.set_targets(vec![target(Duration::from_millis(10))]);
        let mut got = Vec::new();
        assert!(wait_for(|| {
            got.extend(space.poll(Instant::now()).measurements);
            got.len() >= 2
        }));
        assert_eq!(got[0].hwnd, 7);
        assert_eq!(got[0].generation, 3);
        assert!(got[1].taken >= got[0].taken);
    }

    #[test]
    fn stopping_all_targets_stops_the_reads() {
        let (mut space, reads) = fake(None);
        space.set_targets(vec![target(Duration::from_millis(5))]);
        space.poll(Instant::now());
        assert!(wait_for(|| reads.load(Ordering::Relaxed) >= 2));
        space.set_targets(Vec::new());
        thread::sleep(Duration::from_millis(50));
        let settled = reads.load(Ordering::Relaxed);
        thread::sleep(Duration::from_millis(50));
        assert_eq!(reads.load(Ordering::Relaxed), settled);
    }

    #[test]
    fn a_new_generation_is_read_at_once() {
        let (mut space, _) = fake(None);
        space.set_targets(vec![target(Duration::from_secs(60))]);
        let mut got = Vec::new();
        assert!(wait_for(|| {
            got.extend(space.poll(Instant::now()).measurements);
            !got.is_empty()
        }));
        space.set_targets(vec![Target {
            generation: 4,
            ..target(Duration::from_secs(60))
        }]);
        assert!(wait_for(|| {
            got.extend(space.poll(Instant::now()).measurements);
            got.iter().any(|m| m.generation == 4)
        }));
    }

    #[test]
    fn a_stuck_read_is_noticed_and_the_worker_replaced() {
        let (release, hang) = mpsc::channel::<()>();
        let (mut space, reads) = fake(Some(hang));
        space.set_targets(vec![target(Duration::from_millis(5))]);
        space.poll(Instant::now());
        assert!(wait_for(|| reads.load(Ordering::Relaxed) == 1));
        assert!(!space.poll(Instant::now()).abandoned);
        let late = Instant::now() + STUCK_AFTER + Duration::from_secs(1);
        assert!(space.poll(late).abandoned);
        assert!(space.worker.is_none());
        assert!(!space.poll(late).abandoned, "reported once");
        assert!(space.poll(late + RETRY_AFTER).measurements.is_empty());
        assert!(space.worker.is_some(), "replaced after the wait");
        let mut got = Vec::new();
        assert!(wait_for(|| {
            got.extend(space.poll(Instant::now()).measurements);
            !got.is_empty()
        }));
        drop(release);
    }

    #[test]
    fn abandoned_workers_are_capped_until_the_taskbar_changes() {
        let make: MakeProbe = Arc::new(|| None);
        let mut space = Space::with_probe(make);
        space.set_targets(vec![target(Duration::from_millis(5))]);
        let mut now = Instant::now();
        for _ in 0..MAX_ABANDONED * 3 {
            space.poll(now);
            let finished = space
                .worker
                .as_ref()
                .is_none_or(|w| wait_for(|| w.handle.is_finished()));
            assert!(finished);
            space.poll(now);
            now += RETRY_AFTER * (MAX_ABANDONED + 1);
        }
        assert_eq!(space.abandoned, MAX_ABANDONED);
        assert!(space.worker.is_none());
        space.taskbar_changed();
        space.poll(now);
        assert!(space.worker.is_some());
    }

    #[test]
    fn dropping_never_waits_for_a_stuck_read() {
        let (release, hang) = mpsc::channel::<()>();
        let (mut space, reads) = fake(Some(hang));
        space.set_targets(vec![target(Duration::from_millis(5))]);
        space.poll(Instant::now());
        assert!(wait_for(|| reads.load(Ordering::Relaxed) == 1));
        let start = Instant::now();
        drop(space);
        assert!(start.elapsed() < Duration::from_millis(100));
        drop(release);
    }

    #[test]
    fn a_worker_stops_when_its_owner_goes() {
        let (mut space, reads) = fake(None);
        space.set_targets(vec![target(Duration::from_millis(5))]);
        space.poll(Instant::now());
        assert!(wait_for(|| reads.load(Ordering::Relaxed) >= 1));
        let handle = space.worker.take().unwrap().handle;
        drop(space);
        assert!(wait_for(|| handle.is_finished()));
    }

    #[test]
    fn reads_a_real_taskbar_when_it_has_the_tree() {
        let Some(bar) = discover(false).into_iter().next() else {
            return;
        };
        let mut uia = Uia::new().expect("UI Automation");
        let target = Target {
            hwnd: bar.hwnd.0 as isize,
            generation: 1,
            pace: PACE_ACTIVE,
        };
        let start = Instant::now();
        let first = uia.measure(&target);
        let cold = start.elapsed();
        let start = Instant::now();
        let second = uia.measure(&target);
        let held = start.elapsed();
        println!("cold {cold:?}, held {held:?}: {first:?}");
        assert!(cold < Duration::from_secs(5));
        assert!(held < Duration::from_secs(2));
        match first {
            Outcome::Rects(rects) => {
                assert_eq!(Outcome::Rects(rects.clone()), second);
                let gaps = placement::free_gaps(&bar.rect, bar.tray_rect.as_ref(), &rects.occupied);
                println!("gaps {gaps:?}");
                assert!(
                    rects
                        .occupied
                        .iter()
                        .all(|r| r.width() >= 0 && r.height() >= 0)
                );
            }
            Outcome::NoFrame => println!("no frame on this shell"),
            Outcome::Failed => panic!("the probe failed"),
        }
    }
}
