//! A game in progress: its process, its readings, and the strip shown on
//! its monitor while it is in front.

use std::time::{Duration, Instant};

use pulsar_core::config::Config;
use pulsar_core::format::format_value;
use pulsar_core::games::{Session, Summary};
use pulsar_core::metric::{ItemKind, Unit};
use pulsar_core::tr;
use windows::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0};
use windows::Win32::System::Threading::{OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject};
use windows::Win32::UI::WindowsAndMessaging::{
    GWL_EXSTYLE, GetWindowLongPtrW, SetWindowLongPtrW, WINDOW_EX_STYLE, WS_EX_TRANSPARENT,
};

use crate::overlay::Overlay;
use crate::taskbar::placement::Rect32;

/// Sampled while a game runs: the strip's frame rate and the summary's
/// temperatures.
pub const SESSION_ITEMS: [ItemKind; 3] = [ItemKind::Fps, ItemKind::CpuTemp, ItemKind::GpuTemp];
/// Between the strip and the monitor's corner, in DIPs.
const MARGIN_DIP: i32 = 8;

pub struct Game {
    pub pid: u32,
    process: HANDLE,
    started: Instant,
    pub session: Session,
    pub strip: Option<Overlay>,
}

impl Game {
    pub fn start(pid: u32, name: String) -> Option<Self> {
        let process = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, false, pid) }.ok()?;
        Some(Self {
            pid,
            process,
            started: Instant::now(),
            session: Session::new(name),
            strip: None,
        })
    }

    pub fn exited(&self) -> bool {
        unsafe { WaitForSingleObject(self.process, 0) == WAIT_OBJECT_0 }
    }

    pub fn summary(&self) -> Summary {
        self.session.summary(self.started.elapsed())
    }

    pub fn played(&self) -> Duration {
        self.started.elapsed()
    }
}

impl Drop for Game {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.process);
        }
    }
}

/// Clicks pass through the strip to the game.
pub fn click_through(strip: &Overlay) {
    unsafe {
        let style = WINDOW_EX_STYLE(GetWindowLongPtrW(strip.hwnd, GWL_EXSTYLE) as u32);
        SetWindowLongPtrW(
            strip.hwnd,
            GWL_EXSTYLE,
            (style | WS_EX_TRANSPARENT).0 as isize,
        );
    }
}

/// The taskbar's items with the frame rate first.
pub fn strip_config(config: &Config) -> Config {
    let mut c = config.clone();
    if let Some(at) = c.items.iter().position(|i| i.kind == ItemKind::Fps) {
        let mut fps = c.items.remove(at);
        fps.enabled = true;
        c.items.insert(0, fps);
    }
    c
}

/// The monitor's top-left corner, inset.
pub fn strip_position(monitor: Rect32, dpi: u32) -> (i32, i32) {
    let margin = MARGIN_DIP * dpi as i32 / 96;
    (monitor.left + margin, monitor.top + margin)
}

pub fn summary_text(s: &Summary) -> String {
    let mut parts = vec![format_value(s.duration.as_secs_f64(), Unit::Seconds)];
    if let Some(fps) = s.average_fps {
        parts.push(tr!("{} average", format_value(fps, Unit::FramesPerSec)));
    }
    if let Some(low) = s.low_1 {
        parts.push(tr!("1% low {}", format_value(low, Unit::FramesPerSec)));
    }
    if let Some(t) = s.cpu_max {
        parts.push(tr!("CPU up to {}", format_value(t, Unit::Celsius)));
    }
    if let Some(t) = s.gpu_max {
        parts.push(tr!("GPU up to {}", format_value(t, Unit::Celsius)));
    }
    parts.join(" · ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_strip_leads_with_the_frame_rate() {
        let c = strip_config(&Config::default());
        assert_eq!(c.items[0].kind, ItemKind::Fps);
        assert!(c.items[0].enabled);
        assert_eq!(c.items.len(), ItemKind::ALL.len());
        assert!(!Config::default().is_enabled(ItemKind::Fps));
    }

    #[test]
    fn the_strip_sits_inside_the_corner_at_the_monitor_scale() {
        let monitor = Rect32 {
            left: -1920,
            top: 0,
            right: 0,
            bottom: 1080,
        };
        assert_eq!(strip_position(monitor, 96), (-1912, 8));
        assert_eq!(strip_position(monitor, 144), (-1908, 12));
    }

    #[test]
    fn the_summary_lists_what_was_measured() {
        let s = Summary {
            name: "g".into(),
            started: String::new(),
            duration: Duration::from_secs(3900),
            average_fps: Some(87.4),
            low_1: None,
            cpu_max: Some(78.0),
            gpu_max: None,
        };
        assert_eq!(
            summary_text(&s),
            "1 h 05 min · 87 fps average · CPU up to 78°C"
        );
    }
}
