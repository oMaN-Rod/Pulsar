//! Frame rates from the graphics runtimes' own events. Windows timestamps
//! every present a process makes, and every refresh of every display, so
//! nothing is loaded into the game.

use std::collections::{HashMap, VecDeque};
use std::ffi::c_void;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use pulsar_core::sensors::{DISPLAY_SLOTS, DisplayRate, FRAME_SLOTS, FrameRate, FrameRates};
use windows::Win32::Foundation::ERROR_SUCCESS;
use windows::Win32::System::Diagnostics::Etw::{
    CONTROLTRACE_HANDLE, CloseTrace, ControlTraceW, ENABLE_TRACE_PARAMETERS,
    ENABLE_TRACE_PARAMETERS_VERSION_2, EVENT_CONTROL_CODE_CAPTURE_STATE,
    EVENT_CONTROL_CODE_ENABLE_PROVIDER, EVENT_FILTER_DESCRIPTOR, EVENT_FILTER_TYPE_EVENT_ID,
    EVENT_HEADER_FLAG_32_BIT_HEADER, EVENT_RECORD, EVENT_TRACE_CONTROL_STOP, EVENT_TRACE_LOGFILEW,
    EVENT_TRACE_PROPERTIES, EVENT_TRACE_REAL_TIME_MODE, EnableTraceEx2, OpenTraceW,
    PROCESS_TRACE_MODE_EVENT_RECORD, PROCESS_TRACE_MODE_REAL_TIME, PROCESSTRACE_HANDLE,
    ProcessTrace, StartTraceW, TRACE_LEVEL_INFORMATION, WNODE_FLAG_TRACED_GUID,
};
use windows::Win32::System::SystemInformation::GetSystemTimePreciseAsFileTime;
use windows::core::{GUID, HSTRING, PWSTR};

const SESSION: &str = "Pulsar Frames";
const DIAGNOSTIC_SESSION: &str = "Pulsar Frames Diagnostic";
const DXGI: GUID = GUID::from_u128(0xca11c036_0102_4a2d_a6ad_f03cfed5d3c9);
const D3D9: GUID = GUID::from_u128(0x783aca0a_790e_4d7f_8451_aa850511c6b9);
const DXGKRNL: GUID = GUID::from_u128(0x802ec45a_1e99_4b83_9920_87c98277ba9d);
/// Present and PresentMultiplaneOverlay, at their start.
const DXGI_PRESENTS: [u16; 2] = [42, 55];
const D3D9_PRESENTS: [u16; 1] = [1];
/// PresentHistory and PresentHistoryDetailed, at their start: what Vulkan
/// and OpenGL presents leave behind.
const KERNEL_PRESENTS: [u16; 2] = [171, 215];
/// VSyncDPC, VSyncDPCMultiPlane and HSyncDPCMultiPlane: a display's refresh
/// and what it scanned out.
const VSYNC: u16 = 17;
const VSYNC_PLANES: u16 = 273;
const HSYNC_PLANES: u16 = 382;
/// DpiReportAdapter, which pairs an adapter's kernel address with its LUID.
const ADAPTER_REPORT: u16 = 110;
const KERNEL_EVENTS: [u16; 6] = [171, 215, VSYNC, VSYNC_PLANES, HSYNC_PLANES, ADAPTER_REPORT];
const RUNTIME_KEYWORD: u64 = 0x2;
const KERNEL_PRESENT_KEYWORD: u64 = 0x800_0000;
/// A present that only asks whether the window is visible.
const DXGI_PRESENT_TEST: u32 = 0x1;
/// How long the trace outlives the app's last request.
const LINGER: Duration = Duration::from_secs(5);
/// In the 100 ns ticks of event timestamps.
const SECOND: i64 = 10_000_000;
/// How far back the lows look.
const WINDOW: i64 = 10 * SECOND;
/// A process silent for longer has stopped drawing.
const IDLE: i64 = 3 * SECOND;
const INVALID_PROCESSTRACE: u64 = u64::MAX;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Source {
    /// DXGI or Direct3D 9, which the game called.
    Runtime,
    /// The kernel's record of the same frames, for everything else.
    Kernel,
}

/// Which kind of frame an event marks, if any. `flags` is a DXGI present's
/// flags, where the event carries them.
fn classify(provider: GUID, id: u16, flags: Option<u32>) -> Option<Source> {
    if provider == DXGI && DXGI_PRESENTS.contains(&id) {
        (flags.unwrap_or(0) & DXGI_PRESENT_TEST == 0).then_some(Source::Runtime)
    } else if provider == D3D9 && D3D9_PRESENTS.contains(&id) {
        Some(Source::Runtime)
    } else if provider == DXGKRNL && KERNEL_PRESENTS.contains(&id) {
        Some(Source::Kernel)
    } else {
        None
    }
}

/// How a display's refresh was reported. Later kinds describe overlay
/// planes too, so they win where a display reports more than one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Scan {
    Single = 0,
    Planes = 1,
    Queue = 2,
}

/// A refresh: the adapter's kernel address, the output, and a fingerprint
/// of what every plane showed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Refresh {
    scan: Scan,
    adapter: u64,
    source: u32,
    shown: u64,
}

fn u32_at(data: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(data.get(at..at + 4)?.try_into().ok()?))
}

fn u64_at(data: &[u8], at: usize) -> Option<u64> {
    Some(u64::from_le_bytes(data.get(at..at + 8)?.try_into().ok()?))
}

/// Kernel events always carry 64-bit pointers. The single-plane event names
/// the scanned address; the multi-plane ones list one per plane, ahead of
/// the output.
fn refresh(id: u16, data: &[u8]) -> Option<Refresh> {
    let adapter = u64_at(data, 0)?;
    let (scan, source, shown) = match id {
        VSYNC => (Scan::Single, u32_at(data, 20)?, u64_at(data, 12)?),
        VSYNC_PLANES | HSYNC_PLANES => {
            let planes = u32_at(data, 8)? as usize;
            let end = 12 + planes.checked_mul(8)?;
            let shown = data
                .get(12..end)?
                .as_chunks::<8>()
                .0
                .iter()
                .fold(0xcbf2_9ce4_8422_2325u64, |h, &plane| {
                    (h ^ u64::from_le_bytes(plane)).wrapping_mul(0x100_0000_01b3)
                });
            let scan = if id == VSYNC_PLANES {
                Scan::Planes
            } else {
                Scan::Queue
            };
            (scan, u32_at(data, end)?, shown)
        }
        _ => return None,
    };
    Some(Refresh {
        scan,
        adapter,
        source,
        shown,
    })
}

/// The adapter's kernel address and LUID, which ends the event.
fn adapter_report(data: &[u8]) -> Option<(u64, u64)> {
    Some((u64_at(data, 0)?, u64_at(data, data.len().checked_sub(8)?)?))
}

/// A DXGI present's flags follow the swap chain pointer, whose size is the
/// presenting process's.
fn dxgi_flags(data: &[u8], wide_pointers: bool) -> Option<u32> {
    let at = if wide_pointers { 8 } else { 4 };
    Some(u32::from_le_bytes(data.get(at..at + 4)?.try_into().ok()?))
}

#[derive(Default)]
struct Presents {
    runtime: VecDeque<i64>,
    kernel: VecDeque<i64>,
}

impl Presents {
    /// A game's presents show in both, so the runtime's count when it has any.
    fn times(&mut self) -> &mut VecDeque<i64> {
        if self.runtime.is_empty() {
            &mut self.kernel
        } else {
            &mut self.runtime
        }
    }
}

#[derive(Default)]
struct Scans {
    /// The newest refresh reported this way.
    seen: i64,
    shown: Option<u64>,
    /// Refreshes that showed something new.
    changes: VecDeque<i64>,
}

#[derive(Default)]
struct Display {
    scans: [Scans; 3],
}

impl Display {
    fn record(&mut self, scan: Scan, shown: u64, at: i64) {
        let s = &mut self.scans[scan as usize];
        if s.shown.is_some_and(|before| before != shown) {
            s.changes.push_back(at);
        }
        s.shown = Some(shown);
        s.seen = s.seen.max(at);
    }
}

#[derive(Default)]
struct Log {
    apps: HashMap<u32, Presents>,
    /// By adapter address and output.
    displays: HashMap<(u64, u32), Display>,
    luids: HashMap<u64, u64>,
}

/// The rate over the second before the newest present, and the lows over
/// the whole of `times`, which must be in order.
fn rate(pid: u32, times: &[i64]) -> Option<FrameRate> {
    let &newest = times.last()?;
    let mut gaps: Vec<i64> = times.windows(2).map(|w| w[1] - w[0]).collect();
    let recent = times
        .iter()
        .rev()
        .take_while(|&&t| t > newest - SECOND)
        .count();
    let last_second = &gaps[gaps.len().saturating_sub(recent)..];
    let total: i64 = last_second.iter().sum();
    if total <= 0 {
        return None;
    }
    let ms = |ticks: i64| ticks as f64 / 10_000.0;
    let frame_time = ms(total) / last_second.len() as f64;
    gaps.sort_unstable_by(|a, b| b.cmp(a));
    let low = |share: f64| 1000.0 / ms(gaps[(gaps.len() as f64 * share) as usize].max(1));
    Some(FrameRate {
        pid,
        fps: 1000.0 / frame_time,
        frame_time,
        low_1: low(0.01),
        low_01: low(0.001),
    })
}

/// Drops what has aged out, and the rates of the processes drawing lately,
/// most recent first.
fn app_rates(log: &mut HashMap<u32, Presents>, now: i64) -> Vec<FrameRate> {
    let mut latest = Vec::new();
    log.retain(|&pid, presents| {
        for times in [&mut presents.runtime, &mut presents.kernel] {
            // Events from different processors arrive slightly out of order.
            times.make_contiguous().sort_unstable();
            while times.front().is_some_and(|&t| t < now - WINDOW) {
                times.pop_front();
            }
        }
        let times = presents.times();
        let Some(&newest) = times.back() else {
            return false;
        };
        if newest >= now - IDLE {
            latest.extend(rate(pid, times.make_contiguous()).map(|r| (newest, r)));
        }
        true
    });
    latest.sort_unstable_by_key(|&(newest, _)| std::cmp::Reverse(newest));
    latest
        .into_iter()
        .take(FRAME_SLOTS)
        .map(|(_, r)| r)
        .collect()
}

/// How many refreshes in the second before the newest showed something
/// new, for the displays still refreshing.
fn display_rates(log: &mut Log, now: i64) -> Vec<DisplayRate> {
    let mut rates = Vec::new();
    log.displays.retain(|&(adapter, source), display| {
        for s in &mut display.scans {
            s.changes.make_contiguous().sort_unstable();
            while s.changes.front().is_some_and(|&t| t < now - WINDOW) {
                s.changes.pop_front();
            }
        }
        let Some(s) = display.scans.iter().rev().find(|s| s.seen >= now - IDLE) else {
            return false;
        };
        let fps = s.changes.iter().filter(|&&t| t > s.seen - SECOND).count();
        rates.push(DisplayRate {
            adapter_luid: log.luids.get(&adapter).copied().unwrap_or(0),
            source,
            fps: fps as f64,
        });
        true
    });
    rates.truncate(DISPLAY_SLOTS);
    rates
}

fn now() -> i64 {
    let t = unsafe { GetSystemTimePreciseAsFileTime() };
    (i64::from(t.dwHighDateTime) << 32) | i64::from(t.dwLowDateTime)
}

unsafe extern "system" fn on_event(record: *mut EVENT_RECORD) {
    let Some(record) = (unsafe { record.as_ref() }) else {
        return;
    };
    let header = &record.EventHeader;
    let data = if record.UserData.is_null() {
        &[][..]
    } else {
        unsafe {
            std::slice::from_raw_parts(record.UserData.cast::<u8>(), record.UserDataLength.into())
        }
    };
    let id = header.EventDescriptor.Id;
    let log = unsafe { &*record.UserContext.cast::<Mutex<Log>>() };
    if header.ProviderId == DXGKRNL {
        if let Some(r) = refresh(id, data) {
            if let Ok(mut log) = log.lock() {
                let display = log.displays.entry((r.adapter, r.source)).or_default();
                display.record(r.scan, r.shown, header.TimeStamp);
            }
            return;
        }
        if id == ADAPTER_REPORT {
            if let (Some((adapter, luid)), Ok(mut log)) = (adapter_report(data), log.lock()) {
                log.luids.insert(adapter, luid);
            }
            return;
        }
    }
    let wide = u32::from(header.Flags) & EVENT_HEADER_FLAG_32_BIT_HEADER == 0;
    let flags = (header.ProviderId == DXGI)
        .then(|| dxgi_flags(data, wide))
        .flatten();
    let Some(source) = classify(header.ProviderId, id, flags) else {
        return;
    };
    let Ok(mut log) = log.lock() else {
        return;
    };
    let presents = log.apps.entry(header.ProcessId).or_default();
    match source {
        Source::Runtime => presents.runtime.push_back(header.TimeStamp),
        Source::Kernel => presents.kernel.push_back(header.TimeStamp),
    }
}

/// `EVENT_TRACE_PROPERTIES` with room for the session's name after it.
fn properties() -> Vec<u64> {
    let size = size_of::<EVENT_TRACE_PROPERTIES>() + (DIAGNOSTIC_SESSION.len() + 1) * 2;
    let mut buffer = vec![0u64; size.div_ceil(8)];
    let p = unsafe { &mut *buffer.as_mut_ptr().cast::<EVENT_TRACE_PROPERTIES>() };
    p.Wnode.BufferSize = (buffer.len() * 8) as u32;
    p.Wnode.Flags = WNODE_FLAG_TRACED_GUID;
    // Timestamps from the performance counter, the finest clock there is.
    p.Wnode.ClientContext = 1;
    p.LogFileMode = EVENT_TRACE_REAL_TIME_MODE;
    p.FlushTimer = 1;
    p.LoggerNameOffset = size_of::<EVENT_TRACE_PROPERTIES>() as u32;
    buffer
}

/// Asks the provider to report what already exists, such as its adapters.
fn capture_state(session: CONTROLTRACE_HANDLE, provider: GUID, keyword: u64) {
    unsafe {
        let _ = EnableTraceEx2(
            session,
            &provider,
            EVENT_CONTROL_CODE_CAPTURE_STATE.0,
            TRACE_LEVEL_INFORMATION as u8,
            keyword,
            0,
            0,
            None,
        );
    }
}

/// Turns on only the listed events of a provider.
fn enable(session: CONTROLTRACE_HANDLE, provider: GUID, keyword: u64, ids: &[u16]) -> bool {
    // EVENT_FILTER_EVENT_ID: FilterIn, Reserved, Count, then the IDs.
    let mut filter = vec![1u16, ids.len() as u16];
    filter.extend_from_slice(ids);
    let mut descriptor = EVENT_FILTER_DESCRIPTOR {
        Ptr: filter.as_ptr() as u64,
        Size: (filter.len() * 2) as u32,
        Type: EVENT_FILTER_TYPE_EVENT_ID,
    };
    let parameters = ENABLE_TRACE_PARAMETERS {
        Version: ENABLE_TRACE_PARAMETERS_VERSION_2,
        EnableFilterDesc: &mut descriptor,
        FilterDescCount: 1,
        ..Default::default()
    };
    let result = unsafe {
        EnableTraceEx2(
            session,
            &provider,
            EVENT_CONTROL_CODE_ENABLE_PROVIDER.0,
            TRACE_LEVEL_INFORMATION as u8,
            keyword,
            0,
            0,
            Some(&parameters),
        )
    };
    result == ERROR_SUCCESS
}

fn stop_session(name: &str) {
    let mut properties = properties();
    unsafe {
        let _ = ControlTraceW(
            CONTROLTRACE_HANDLE::default(),
            &HSTRING::from(name),
            properties.as_mut_ptr().cast(),
            EVENT_TRACE_CONTROL_STOP,
        );
    }
}

pub fn clear_leftover() {
    stop_session(SESSION);
}

const RESTART_AFTER: Duration = Duration::from_secs(10);

/// A running trace session and the thread reading it.
struct Trace {
    name: &'static str,
    consumer: PROCESSTRACE_HANDLE,
    reader: Option<JoinHandle<()>>,
    log: Arc<Mutex<Log>>,
}

impl Trace {
    fn start(session_name: &'static str) -> Option<Self> {
        stop_session(session_name);
        let name = HSTRING::from(session_name);
        let mut properties = properties();
        let mut session = CONTROLTRACE_HANDLE::default();
        let started = unsafe { StartTraceW(&mut session, &name, properties.as_mut_ptr().cast()) };
        if started != ERROR_SUCCESS {
            return None;
        }
        let runtimes = enable(session, DXGI, RUNTIME_KEYWORD, &DXGI_PRESENTS)
            | enable(session, D3D9, RUNTIME_KEYWORD, &D3D9_PRESENTS);
        let kernel = enable(session, DXGKRNL, KERNEL_PRESENT_KEYWORD, &KERNEL_EVENTS);
        if !(runtimes || kernel) {
            stop_session(session_name);
            return None;
        }
        let log = Arc::new(Mutex::new(Log::default()));
        let mut logfile = EVENT_TRACE_LOGFILEW {
            LoggerName: PWSTR(name.as_ptr().cast_mut()),
            Context: Arc::as_ptr(&log).cast_mut().cast::<c_void>(),
            ..Default::default()
        };
        logfile.Anonymous1.ProcessTraceMode =
            PROCESS_TRACE_MODE_REAL_TIME | PROCESS_TRACE_MODE_EVENT_RECORD;
        logfile.Anonymous2.EventRecordCallback = Some(on_event);
        let consumer = unsafe { OpenTraceW(&mut logfile) };
        if consumer.Value == INVALID_PROCESSTRACE {
            stop_session(session_name);
            return None;
        }
        let reader = std::thread::spawn(move || unsafe {
            let _ = ProcessTrace(&[consumer], None, None);
        });
        if kernel {
            capture_state(session, DXGKRNL, KERNEL_PRESENT_KEYWORD);
        }
        Some(Self {
            name: session_name,
            consumer,
            reader: Some(reader),
            log,
        })
    }

    fn ended(&self) -> bool {
        self.reader.as_ref().is_none_or(JoinHandle::is_finished)
    }

    fn rates(&self) -> FrameRates {
        let Ok(mut log) = self.log.lock() else {
            return FrameRates::default();
        };
        let now = now();
        FrameRates {
            apps: app_rates(&mut log.apps, now),
            displays: display_rates(&mut log, now),
        }
    }
}

impl Drop for Trace {
    fn drop(&mut self) {
        stop_session(self.name);
        unsafe {
            let _ = CloseTrace(self.consumer);
        }
        // The callback borrows the log until the reader returns.
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}

/// Runs the trace while the app wants frame rates, and for a moment after.
pub struct Frames {
    session: &'static str,
    trace: Option<Trace>,
    asked_at: Option<Instant>,
    restart_at: Option<Instant>,
}

impl Default for Frames {
    fn default() -> Self {
        Self {
            session: SESSION,
            trace: None,
            asked_at: None,
            restart_at: None,
        }
    }
}

impl Frames {
    pub fn update(&mut self, asked: bool) -> FrameRates {
        if asked {
            self.asked_at = Some(Instant::now());
        }
        if self.trace.as_ref().is_some_and(Trace::ended) {
            self.trace = None;
            self.restart_at = Some(Instant::now() + RESTART_AFTER);
        }
        if self.asked_at.is_some_and(|at| at.elapsed() < LINGER) {
            if self.trace.is_none() && self.restart_at.is_none_or(|t| Instant::now() >= t) {
                self.trace = Trace::start(self.session);
                if self.trace.is_none() {
                    self.restart_at = Some(Instant::now() + RESTART_AFTER);
                }
            }
        } else {
            self.trace = None;
        }
        self.trace
            .as_ref()
            .map_or_else(FrameRates::default, Trace::rates)
    }
}

/// Prints the rates every second, for trying the trace from an elevated
/// terminal.
pub fn print() {
    let mut frames = Frames {
        session: DIAGNOSTIC_SESSION,
        ..Frames::default()
    };
    loop {
        let rates = frames.update(true);
        for d in rates.displays {
            println!(
                "adapter {:#x} output {}  {:6.1} fps",
                d.adapter_luid, d.source, d.fps
            );
        }
        for r in rates.apps {
            println!(
                "process {:>6}  {:6.1} fps  {:6.2} ms  1% {:6.1}  0.1% {:6.1}",
                r.pid, r.fps, r.frame_time, r.low_1, r.low_01
            );
        }
        if frames.trace.is_none() {
            println!("The trace could not start; it needs an elevated terminal.");
        }
        println!();
        std::thread::sleep(Duration::from_secs(1));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::System::Diagnostics::Etw::{
        EVENT_DESCRIPTOR, TRACE_EVENT_INFO, TdhGetManifestEventInformation,
    };

    /// Presents `fps` times a second for `seconds`, ending at `end`.
    fn steady(fps: i64, seconds: i64, end: i64) -> Vec<i64> {
        let n = fps * seconds;
        (0..=n).map(|i| end - (n - i) * SECOND / fps).collect()
    }

    #[test]
    fn a_steady_rate_has_lows_at_the_same_rate() {
        let r = rate(7, &steady(100, 5, 1_000 * SECOND)).unwrap();
        assert_eq!(r.pid, 7);
        assert!((r.fps - 100.0).abs() < 0.01, "{r:?}");
        assert!((r.frame_time - 10.0).abs() < 0.001);
        assert!((r.low_1 - 100.0).abs() < 0.01 && (r.low_01 - 100.0).abs() < 0.01);
    }

    #[test]
    fn a_stutter_pulls_down_the_lows_but_not_the_rate() {
        let end = 1_000 * SECOND;
        let mut times = steady(100, 3, end - 2 * SECOND);
        // One 100 ms hitch, then more than a second of smooth frames.
        let resume = times.last().unwrap() + SECOND / 10;
        times.extend((0..150).map(|i| resume + i * SECOND / 100));
        let r = rate(1, &times).unwrap();
        assert!(r.fps > 99.0, "{r:?}");
        assert!(r.low_01 < 11.0, "{r:?}");
        assert!(r.low_1 > 99.0, "{r:?}");
    }

    #[test]
    fn one_present_is_not_a_rate() {
        assert_eq!(rate(1, &[5 * SECOND]), None);
        assert_eq!(rate(1, &[]), None);
    }

    #[test]
    fn processes_that_stopped_drawing_drop_out_newest_first() {
        let now = 1_000 * SECOND;
        let mut log = HashMap::new();
        for (pid, end) in [(1, now - SECOND), (2, now), (3, now - 2 * IDLE)] {
            log.insert(
                pid,
                Presents {
                    runtime: steady(60, 2, end).into(),
                    kernel: VecDeque::new(),
                },
            );
        }
        let pids: Vec<u32> = app_rates(&mut log, now).iter().map(|r| r.pid).collect();
        assert_eq!(pids, [2, 1]);
        log.get_mut(&3).unwrap().runtime.clear();
        app_rates(&mut log, now + WINDOW + SECOND);
        assert!(log.is_empty());
    }

    #[test]
    fn the_kernel_counts_only_when_the_runtime_saw_nothing() {
        let now = 1_000 * SECOND;
        let mut log = HashMap::from([(
            9,
            Presents {
                runtime: VecDeque::new(),
                kernel: steady(30, 2, now).into(),
            },
        )]);
        assert!((app_rates(&mut log, now)[0].fps - 30.0).abs() < 0.01);
        log.get_mut(&9).unwrap().runtime = steady(144, 2, now).into();
        assert!((app_rates(&mut log, now)[0].fps - 144.0).abs() < 0.01);
    }

    #[test]
    fn only_present_events_count_and_visibility_tests_do_not() {
        assert_eq!(classify(DXGI, 42, Some(0)), Some(Source::Runtime));
        assert_eq!(classify(DXGI, 42, Some(DXGI_PRESENT_TEST)), None);
        assert_eq!(classify(DXGI, 43, Some(0)), None);
        assert_eq!(classify(D3D9, 1, None), Some(Source::Runtime));
        assert_eq!(classify(DXGKRNL, 215, None), Some(Source::Kernel));
        assert_eq!(classify(DXGKRNL, 184, None), None);
    }

    #[test]
    fn dxgi_flags_sit_after_a_pointer_of_the_callers_width() {
        let mut data = vec![0xAA; 8];
        data.extend(1u32.to_le_bytes());
        assert_eq!(dxgi_flags(&data, true), Some(1));
        assert_eq!(dxgi_flags(&data[4..], false), Some(1));
        assert_eq!(dxgi_flags(&data[..10], true), None);
    }

    #[test]
    fn a_display_counts_the_refreshes_that_showed_something_new() {
        let mut log = Log::default();
        log.luids.insert(0xA, 0x55);
        let now = 1_000 * SECOND;
        let refresh = SECOND / 120;
        let display = log.displays.entry((0xA, 1)).or_default();
        // 120 Hz for two seconds, a new picture every other refresh.
        for i in 0..240 {
            display.record(Scan::Planes, i / 2, now - 2 * SECOND + i as i64 * refresh);
        }
        let rates = display_rates(&mut log, now);
        assert_eq!(rates.len(), 1);
        assert_eq!((rates[0].adapter_luid, rates[0].source), (0x55, 1));
        assert!((rates[0].fps - 60.0).abs() <= 1.0, "{rates:?}");
    }

    #[test]
    fn the_richest_report_of_a_display_wins_and_quiet_displays_drop_out() {
        let mut log = Log::default();
        let now = 1_000 * SECOND;
        let display = log.displays.entry((0xB, 0)).or_default();
        for i in 0..60 {
            let at = now - SECOND + i * SECOND / 60;
            display.record(Scan::Single, i as u64, at);
            display.record(Scan::Planes, 7, at);
        }
        assert_eq!(display_rates(&mut log, now)[0].fps, 0.0);
        assert_eq!(display_rates(&mut log, now)[0].adapter_luid, 0);
        assert!(display_rates(&mut log, now + 2 * IDLE).is_empty());
        assert!(log.displays.is_empty());
    }

    #[test]
    fn refreshes_are_read_from_their_event_layouts() {
        let mut single = vec![0u8; 56];
        single[..8].copy_from_slice(&0xA0u64.to_le_bytes());
        single[12..20].copy_from_slice(&0x1234u64.to_le_bytes());
        single[20..24].copy_from_slice(&2u32.to_le_bytes());
        let r = refresh(VSYNC, &single).unwrap();
        assert_eq!(
            (r.scan, r.adapter, r.source, r.shown),
            (Scan::Single, 0xA0, 2, 0x1234)
        );

        let planes = |addresses: &[u64], source: u32| {
            let mut d = 0xB0u64.to_le_bytes().to_vec();
            d.extend((addresses.len() as u32).to_le_bytes());
            addresses.iter().for_each(|a| d.extend(a.to_le_bytes()));
            d.extend(source.to_le_bytes());
            d.extend([0u8; 16]);
            d
        };
        let a = refresh(VSYNC_PLANES, &planes(&[1, 2], 3)).unwrap();
        assert_eq!((a.scan, a.adapter, a.source), (Scan::Planes, 0xB0, 3));
        let b = refresh(HSYNC_PLANES, &planes(&[1, 9], 3)).unwrap();
        assert_eq!(b.scan, Scan::Queue);
        assert_ne!(a.shown, b.shown);
        assert_eq!(refresh(VSYNC_PLANES, &planes(&[1, 2], 3)[..20]), None);
        assert_eq!(refresh(171, &single), None);

        let mut report = 0xC0u64.to_le_bytes().to_vec();
        report.extend([0u8; 40]);
        report.extend(0x99u64.to_le_bytes());
        assert_eq!(adapter_report(&report), Some((0xC0, 0x99)));
    }

    fn property_names(provider: GUID, id: u16, version: u8) -> Vec<String> {
        let descriptor = EVENT_DESCRIPTOR {
            Id: id,
            Version: version,
            ..Default::default()
        };
        let mut size = 0u32;
        unsafe { TdhGetManifestEventInformation(&provider, &descriptor, None, &mut size) };
        let mut buffer = vec![0u64; (size as usize).div_ceil(8)];
        let info = buffer.as_mut_ptr().cast::<TRACE_EVENT_INFO>();
        let result = unsafe {
            TdhGetManifestEventInformation(&provider, &descriptor, Some(info), &mut size)
        };
        assert_eq!(result, 0, "no manifest for event {id}");
        let info = unsafe { &*info };
        let properties = unsafe {
            std::slice::from_raw_parts(
                info.EventPropertyInfoArray.as_ptr(),
                info.TopLevelPropertyCount as usize,
            )
        };
        properties
            .iter()
            .map(|p| unsafe {
                let name = buffer.as_ptr().cast::<u8>().add(p.NameOffset as usize);
                windows::core::PCWSTR(name.cast()).to_string().unwrap()
            })
            .collect()
    }

    #[test]
    fn the_installed_manifest_lays_display_events_out_as_read() {
        let names = |id, version| property_names(DXGKRNL, id, version);
        assert_eq!(
            names(VSYNC, 0)[..4],
            [
                "pDxgAdapter",
                "VidPnTargetId",
                "ScannedPhysicalAddress",
                "VidPnSourceId"
            ]
        );
        assert_eq!(
            names(VSYNC_PLANES, 4)[..4],
            [
                "pDxgAdapter",
                "PlaneCount",
                "PresentIdOrPhysicalAddress",
                "VidPnSourceId"
            ]
        );
        assert_eq!(
            names(HSYNC_PLANES, 2)[..4],
            [
                "pDxgAdapter",
                "PlaneCount",
                "ScannedPhysicalAddress",
                "VidPnSourceId"
            ]
        );
        assert_eq!(names(ADAPTER_REPORT, 1).last().unwrap(), "AdapterLuid");
    }

    #[test]
    fn the_installed_manifest_puts_dxgi_flags_after_the_swap_chain() {
        for id in DXGI_PRESENTS {
            let names = property_names(DXGI, id, 0);
            assert_eq!(names[..2], ["pIDXGISwapChain", "Flags"], "event {id}");
        }
    }
}
