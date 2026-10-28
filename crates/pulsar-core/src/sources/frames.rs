//! Frame rates, which the sensor service measures while this source asks
//! for them: the foreground application's, and how often each display's
//! picture changes.

use std::collections::HashSet;

use windows::Win32::Foundation::{CloseHandle, HWND, LPARAM};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::Threading::{
    OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetForegroundWindow, GetWindowThreadProcessId, IsWindowVisible,
};
use windows::core::{BOOL, PWSTR};

use crate::hardware::displays::{self, Display};
use crate::metric::{FrameLabels, MetricKey, Snapshot, Source, SourceError, SourceId};
use crate::sensors::{self, DisplayRate, FrameRate};

pub struct FramesSource {
    /// The monitor whose rate the item shows, by device path; `None` for
    /// the foreground application.
    pub monitor: Option<String>,
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
        let app = pid.and_then(|pid| {
            let processes = processes();
            let helpers = helpers(pid, &processes, &visible_window_pids());
            app_rate(&rates.apps, pid, &helpers)
        });
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
            app: pid.and_then(app_name),
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
            if of == parent && child != parent && !found.contains(&child) {
                found.push(child);
            }
        }
        i += 1;
    }
    found.retain(|p| *p != pid && !windowed.contains(p));
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

/// The executable's name without its folder or extension.
fn app_name(pid: u32) -> Option<String> {
    let path = unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = [0u16; 512];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_WIN32,
            PWSTR(buf.as_mut_ptr()),
            &mut len,
        );
        let _ = CloseHandle(process);
        ok.ok()?;
        String::from_utf16_lossy(&buf[..len as usize])
    };
    Some(stem(&path).to_string())
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
