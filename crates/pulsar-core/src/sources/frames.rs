//! Frame rates, which the sensor service measures while this source asks
//! for them: the foreground application's, and how often each display's
//! picture changes.

use std::collections::HashSet;
use std::time::{Duration, Instant};

use windows::Win32::Foundation::FILETIME;
use windows::Win32::Foundation::{CloseHandle, HWND, LPARAM};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::Threading::{
    GetProcessTimes, OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
    QueryFullProcessImageNameW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetForegroundWindow, GetWindowThreadProcessId, IsWindowVisible,
};
use windows::core::{BOOL, PWSTR};

use crate::hardware::displays::{self, Display};
use crate::metric::{FrameLabels, MetricKey, Snapshot, Source, SourceError, SourceId};
use crate::sensors::{self, DisplayRate, FrameRate};

/// How long the helpers found for the foreground app are trusted while
/// nothing else changes, for one that gains or loses a window.
const HELPERS_FOR: Duration = Duration::from_secs(10);

pub struct FramesSource {
    /// The monitor whose rate the item shows, by device path; `None` for
    /// the foreground application.
    monitor: Option<String>,
    app: Option<App>,
}

impl FramesSource {
    pub fn new(monitor: Option<String>) -> Self {
        Self { monitor, app: None }
    }
}

struct App {
    pid: u32,
    /// Its start time, so a reused process ID is not taken for it.
    started: u64,
    name: Option<String>,
    /// The other processes the service had rates for when they were found,
    /// sorted. One that stops drawing and starts again is already judged.
    drawing: Vec<u32>,
    helpers: Vec<u32>,
    found: Instant,
}

/// Whether the helpers must be found again. Taking a snapshot of every
/// process costs several milliseconds, so it waits for a change.
fn helpers_stale(app: &App, pid: u32, started: u64, drawing: &[u32], now: Instant) -> bool {
    app.pid != pid
        || app.started != started
        || drawing
            .iter()
            .any(|p| app.drawing.binary_search(p).is_err())
        || now.duration_since(app.found) >= HELPERS_FOR
}

impl Source for FramesSource {
    fn id(&self) -> SourceId {
        SourceId::Frames
    }

    fn sample(&mut self, out: &mut Snapshot) -> Result<(), SourceError> {
        sensors::want_frames();
        let rates = sensors::read_frames().map_err(|e| SourceError::Unavailable(e.to_string()))?;
        let window = unsafe { GetForegroundWindow() };
        let pid = window_pid(window);
        let now = Instant::now();
        let app = pid.and_then(|pid| {
            let (started, name) = process_info(pid);
            let mut drawing: Vec<u32> = rates
                .apps
                .iter()
                .map(|r| r.pid)
                .filter(|&p| p != pid)
                .collect();
            drawing.sort_unstable();
            drawing.dedup();
            let cached = self
                .app
                .take()
                .filter(|a| !helpers_stale(a, pid, started, &drawing, now));
            let app = cached.unwrap_or_else(|| {
                // With nothing else drawing, no helper can be.
                let helpers = if drawing.is_empty() {
                    Vec::new()
                } else {
                    helpers(pid, &processes(), &visible_window_pids())
                };
                App {
                    pid,
                    started,
                    name,
                    drawing,
                    helpers,
                    found: now,
                }
            });
            let rate = app_rate(&rates.apps, pid, &app.helpers);
            self.app = Some(app);
            rate
        });
        if pid.is_none() {
            self.app = None;
        }
        if let Some(r) = app {
            out.set(MetricKey::FrameTimeMs, r.frame_time);
            out.set(MetricKey::FrameRateLow1, r.low_1);
            out.set(MetricKey::FrameRateLow01, r.low_01);
        }
        // An app that has stopped drawing is at zero, not unknown.
        let app_fps = app.map_or(0.0, |r| r.fps);
        out.set(MetricKey::AppFrameRate, app_fps);

        let all = displays::active();
        let pinned = self
            .monitor
            .as_ref()
            .and_then(|path| all.iter().find(|d| &d.path == path));
        let shown = pinned.or_else(|| {
            let gdi = displays::gdi_name_of(window)?;
            all.iter().find(|d| d.gdi_name == gdi)
        });
        let display_fps = shown.and_then(|d| display_rate(&rates.displays, d));
        if let Some(fps) = display_fps {
            out.set(MetricKey::DisplayFrameRate, fps);
        }
        let main = if pinned.is_some() {
            display_fps
        } else {
            Some(app_fps)
        };
        if let Some(fps) = main {
            out.set(MetricKey::FrameRate, fps);
        }
        out.frames = FrameLabels {
            app: self.app.as_ref().and_then(|a| a.name.clone()),
            display: shown.map(|d| d.name.clone()),
            pinned: pinned.is_some(),
        };
        Ok(())
    }
}

/// The app's own rate, or that of a helper drawing for it, whichever is
/// higher: browsers and apps built on them draw from a GPU process.
fn app_rate(rates: &[FrameRate], pid: u32, helpers: &[u32]) -> Option<FrameRate> {
    rates
        .iter()
        .filter(|r| r.pid == pid || helpers.contains(&r.pid))
        .copied()
        .max_by(|a, b| a.fps.total_cmp(&b.fps))
}

/// The service may not know the adapter, and then matches by output alone.
fn display_rate(rates: &[DisplayRate], display: &Display) -> Option<f64> {
    rates
        .iter()
        .filter(|r| r.source == display.source)
        .find(|r| r.adapter_luid == display.adapter_luid || r.adapter_luid == 0)
        .map(|r| r.fps)
}

/// Descendants of `pid` with no window of their own. Apps it launched
/// have windows, so a game started from Explorer is not Explorer's helper.
fn helpers(pid: u32, processes: &[(u32, u32)], windowed: &HashSet<u32>) -> Vec<u32> {
    let mut found = vec![pid];
    let mut i = 0;
    while i < found.len() {
        let parent = found[i];
        for &(child, of) in processes {
            if of == parent && child != pid && !windowed.contains(&child) && !found.contains(&child)
            {
                found.push(child);
            }
        }
        i += 1;
    }
    found.remove(0);
    found
}

/// Every process and its parent.
fn processes() -> Vec<(u32, u32)> {
    let mut list = Vec::new();
    let Ok(snapshot) = (unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) }) else {
        return list;
    };
    let mut entry = PROCESSENTRY32W {
        dwSize: size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };
    let mut more = unsafe { Process32FirstW(snapshot, &mut entry) }.is_ok();
    while more {
        list.push((entry.th32ProcessID, entry.th32ParentProcessID));
        more = unsafe { Process32NextW(snapshot, &mut entry) }.is_ok();
    }
    unsafe {
        let _ = CloseHandle(snapshot);
    }
    list
}

fn visible_window_pids() -> HashSet<u32> {
    unsafe extern "system" fn add(window: HWND, pids: LPARAM) -> BOOL {
        let pids = unsafe { &mut *(pids.0 as *mut HashSet<u32>) };
        if unsafe { IsWindowVisible(window) }.as_bool() {
            pids.extend(window_pid(window));
        }
        true.into()
    }
    let mut pids = HashSet::new();
    unsafe {
        let _ = EnumWindows(Some(add), LPARAM(&mut pids as *mut _ as isize));
    }
    pids
}

fn window_pid(window: HWND) -> Option<u32> {
    let mut pid = 0;
    unsafe { GetWindowThreadProcessId(window, Some(&mut pid)) };
    (pid != 0).then_some(pid)
}

/// When the process started, and its executable's name without its folder
/// or extension.
fn process_info(pid: u32) -> (u64, Option<String>) {
    let Ok(process) = (unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) })
    else {
        return (0, None);
    };
    let (mut created, mut exited, mut kernel, mut user) = (
        FILETIME::default(),
        FILETIME::default(),
        FILETIME::default(),
        FILETIME::default(),
    );
    let started =
        unsafe { GetProcessTimes(process, &mut created, &mut exited, &mut kernel, &mut user) }
            .map_or(0, |()| {
                (u64::from(created.dwHighDateTime) << 32) | u64::from(created.dwLowDateTime)
            });
    let mut buf = [0u16; 512];
    let mut len = buf.len() as u32;
    let named = unsafe {
        QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_WIN32,
            PWSTR(buf.as_mut_ptr()),
            &mut len,
        )
    };
    unsafe {
        let _ = CloseHandle(process);
    }
    let name = named
        .ok()
        .map(|()| stem(&String::from_utf16_lossy(&buf[..len as usize])).to_string());
    (started, name)
}

fn stem(path: &str) -> &str {
    let file = path.rsplit('\\').next().unwrap_or(path);
    file.rsplit_once('.').map_or(file, |(stem, _)| stem)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rate(pid: u32, fps: f64) -> FrameRate {
        FrameRate {
            pid,
            fps,
            ..FrameRate::default()
        }
    }

    #[test]
    fn the_app_is_named_by_its_file() {
        assert_eq!(stem(r"C:\Games\Some Game\Game-Win64.exe"), "Game-Win64");
        assert_eq!(stem("noext"), "noext");
    }

    #[test]
    fn a_helper_drawing_for_the_app_counts_as_the_app() {
        let rates = [rate(1, 60.0), rate(20, 144.0), rate(30, 90.0)];
        assert_eq!(app_rate(&rates, 1, &[20]).map(|r| r.fps), Some(144.0));
        assert_eq!(app_rate(&rates, 1, &[]).map(|r| r.fps), Some(60.0));
        assert_eq!(app_rate(&rates, 5, &[30]).map(|r| r.pid), Some(30));
        assert_eq!(app_rate(&rates, 5, &[]), None);
    }

    #[test]
    fn helpers_are_windowless_descendants() {
        // Teams starts a web view, which starts its GPU process; Explorer
        // started a game, which has a window.
        let processes = [(10, 1), (11, 10), (12, 11), (40, 2), (41, 40), (12, 12)];
        let windowed = HashSet::from([10, 40, 41]);
        let mut found = helpers(10, &processes, &windowed);
        found.sort_unstable();
        assert_eq!(found, [11, 12]);
        assert!(helpers(40, &processes, &windowed).is_empty());
    }

    #[test]
    fn helpers_are_found_again_only_on_a_change() {
        let now = Instant::now();
        let app = App {
            pid: 10,
            started: 7,
            name: None,
            drawing: vec![20, 30],
            helpers: vec![20],
            found: now,
        };
        let later = |s| now + Duration::from_secs(s);
        assert!(!helpers_stale(&app, 10, 7, &[20, 30], later(1)));
        assert!(
            !helpers_stale(&app, 10, 7, &[30], later(1)),
            "one stopped drawing"
        );
        assert!(
            helpers_stale(&app, 10, 7, &[20, 40], later(1)),
            "a new one draws"
        );
        assert!(
            helpers_stale(&app, 11, 7, &[20, 30], later(1)),
            "another app"
        );
        assert!(
            helpers_stale(&app, 10, 8, &[20, 30], later(1)),
            "its ID was reused"
        );
        assert!(helpers_stale(&app, 10, 7, &[20, 30], later(10)), "too old");
    }

    #[test]
    fn a_display_matches_by_adapter_and_output() {
        let display = Display {
            adapter_luid: 0x55,
            source: 1,
            ..Display::default()
        };
        let rates = [
            DisplayRate {
                adapter_luid: 0x66,
                source: 1,
                fps: 30.0,
            },
            DisplayRate {
                adapter_luid: 0x55,
                source: 1,
                fps: 120.0,
            },
        ];
        assert_eq!(display_rate(&rates, &display), Some(120.0));
        let unknown = [DisplayRate {
            adapter_luid: 0,
            source: 1,
            fps: 75.0,
        }];
        assert_eq!(display_rate(&unknown, &display), Some(75.0));
        assert_eq!(display_rate(&rates[..1], &display), None);
    }
}
