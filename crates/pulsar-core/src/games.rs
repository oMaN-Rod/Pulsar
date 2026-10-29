//! Telling games from other apps, and summing up a play session.

use std::collections::HashSet;
use std::fs::OpenOptions;
use std::io::{self, Write};
use std::path::Path;
use std::time::Duration;

use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_READ, RRF_RT_REG_SZ, RegCloseKey, RegEnumKeyExW, RegGetValueW,
    RegOpenKeyExW,
};
use windows::Win32::System::SystemInformation::GetLocalTime;
use windows::core::{HSTRING, PWSTR};

use crate::config::GameConfig;
use crate::metric::{MetricKey, Snapshot};

/// Where Windows keeps the games Game Bar and Game Mode have recognised.
const GAME_STORE: &str = r"System\GameConfigStore\Children";
const PATH_VALUE: &str = "MatchedExeFullPath";
/// Shorter sessions, such as a launcher flashing up, get no summary.
pub const MIN_SESSION: Duration = Duration::from_secs(60);

/// The full paths, in lower case, of the games Windows knows.
pub fn windows_games() -> HashSet<String> {
    let mut games = HashSet::new();
    let mut store = HKEY::default();
    let opened = unsafe {
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            &HSTRING::from(GAME_STORE),
            None,
            KEY_READ,
            &mut store,
        )
    };
    if opened.is_err() {
        return games;
    }
    let mut name = [0u16; 256];
    for index in 0.. {
        let mut len = name.len() as u32;
        let found = unsafe {
            RegEnumKeyExW(
                store,
                index,
                Some(PWSTR(name.as_mut_ptr())),
                &mut len,
                None,
                None,
                None,
                None,
            )
        };
        if found.is_err() {
            break;
        }
        let child = String::from_utf16_lossy(&name[..len as usize]);
        if let Some(path) = string_value(store, &child, PATH_VALUE) {
            games.insert(path.to_lowercase());
        }
    }
    unsafe {
        let _ = RegCloseKey(store);
    }
    games
}

fn string_value(key: HKEY, subkey: &str, value: &str) -> Option<String> {
    let (subkey, value) = (HSTRING::from(subkey), HSTRING::from(value));
    let mut buffer = [0u16; 1024];
    let mut size = (buffer.len() * 2) as u32;
    unsafe {
        RegGetValueW(
            key,
            &subkey,
            &value,
            RRF_RT_REG_SZ,
            None,
            Some(buffer.as_mut_ptr().cast()),
            Some(&mut size),
        )
    }
    .ok()
    .ok()?;
    let len = buffer.iter().position(|&c| c == 0).unwrap_or(buffer.len());
    let s = String::from_utf16_lossy(&buffer[..len]);
    (!s.is_empty()).then_some(s)
}

/// The executable's file name in lower case, which marks are kept by.
pub fn exe_name(path: &str) -> String {
    path.rsplit(['\\', '/'])
        .next()
        .unwrap_or(path)
        .to_lowercase()
}

/// The name shown for a game: its file name without the extension.
pub fn display_name(path: &str) -> String {
    let file = path.rsplit(['\\', '/']).next().unwrap_or(path);
    file.rsplit_once('.')
        .map_or(file, |(stem, _)| stem)
        .to_string()
}

/// The user's marks win over Windows' list either way.
pub fn is_game(path: &str, windows: &HashSet<String>, config: &GameConfig) -> bool {
    let name = exe_name(path);
    if config.not_games.contains(&name) {
        return false;
    }
    config.games.contains(&name) || windows.contains(&path.to_lowercase())
}

/// Flips whether the app counts as a game.
pub fn toggle(path: &str, windows: &HashSet<String>, config: &mut GameConfig) {
    let name = exe_name(path);
    let was = is_game(path, windows, config);
    config.games.retain(|g| *g != name);
    config.not_games.retain(|g| *g != name);
    let listed = windows.contains(&path.to_lowercase());
    match (was, listed) {
        (true, true) => config.not_games.push(name),
        (false, false) => config.games.push(name),
        _ => {}
    }
}

/// Readings gathered while a game runs.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Session {
    pub name: String,
    /// `YYYY-MM-DD HH:MM`, local time.
    pub started: String,
    fps_sum: f64,
    frames: u32,
    seconds: Vec<f64>,
    cpu_max: Option<f64>,
    gpu_max: Option<f64>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Summary {
    pub name: String,
    pub started: String,
    pub duration: Duration,
    pub average_fps: Option<f64>,
    pub low_1: Option<f64>,
    pub cpu_max: Option<f64>,
    pub gpu_max: Option<f64>,
}

impl Session {
    pub fn new(name: String) -> Self {
        let t = unsafe { GetLocalTime() };
        Self {
            name,
            started: format!(
                "{:04}-{:02}-{:02} {:02}:{:02}",
                t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute
            ),
            ..Self::default()
        }
    }

    /// Frame rates count only while the game is in front and drawing.
    pub fn record(&mut self, snapshot: &Snapshot, in_front: bool) {
        let max = |a: Option<f64>, b: Option<f64>| match (a, b) {
            (Some(a), Some(b)) => Some(a.max(b)),
            (a, b) => a.or(b),
        };
        self.cpu_max = max(self.cpu_max, snapshot.get(MetricKey::CpuTempC));
        self.gpu_max = max(self.gpu_max, snapshot.get(MetricKey::GpuTempC));
        if !in_front {
            return;
        }
        if let Some(fps) = snapshot.get(MetricKey::AppFrameRate).filter(|f| *f > 0.0) {
            self.fps_sum += fps;
            self.frames += 1;
            self.seconds.push(fps);
        }
    }

    pub fn summary(&self, duration: Duration) -> Summary {
        let mean = |sum: f64, n: u32| (n > 0).then(|| sum / f64::from(n));
        Summary {
            name: self.name.clone(),
            started: self.started.clone(),
            duration,
            average_fps: mean(self.fps_sum, self.frames),
            low_1: low_1(&self.seconds),
            cpu_max: self.cpu_max,
            gpu_max: self.gpu_max,
        }
    }
}

fn low_1(seconds: &[f64]) -> Option<f64> {
    let mut sorted = seconds.to_vec();
    sorted.sort_by(f64::total_cmp);
    let wanted = sorted.iter().sum::<f64>() * 0.01;
    let mut counted = 0.0;
    sorted.into_iter().find(|&fps| {
        counted += fps;
        counted >= wanted
    })
}

const CSV_HEADER: &str = "started,game,minutes,average_fps,low_1_fps,cpu_max_c,gpu_max_c";

fn csv_line(s: &Summary) -> String {
    let number = |v: Option<f64>| v.map_or(String::new(), |v| format!("{v:.0}"));
    format!(
        "{},\"{}\",{:.1},{},{},{},{}",
        s.started,
        s.name.replace('"', "\"\""),
        s.duration.as_secs_f64() / 60.0,
        number(s.average_fps),
        number(s.low_1),
        number(s.cpu_max),
        number(s.gpu_max),
    )
}

/// Appends the session to the CSV file, writing its header first if new.
pub fn log_session(path: &Path, summary: &Summary) -> io::Result<()> {
    let new = !path.exists();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    if new {
        writeln!(file, "{CSV_HEADER}")?;
    }
    writeln!(file, "{}", csv_line(summary))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(games: &[&str], not_games: &[&str]) -> GameConfig {
        GameConfig {
            games: games.iter().map(|g| g.to_string()).collect(),
            not_games: not_games.iter().map(|g| g.to_string()).collect(),
            ..GameConfig::default()
        }
    }

    #[test]
    fn windows_list_matches_the_full_path_and_marks_win() {
        let windows = HashSet::from([r"c:\games\a\game.exe".to_string()]);
        let none = config(&[], &[]);
        assert!(is_game(r"C:\Games\A\Game.exe", &windows, &none));
        assert!(!is_game(r"C:\Other\Game.exe", &windows, &none));
        assert!(is_game(
            r"C:\Other\Game.exe",
            &windows,
            &config(&["game.exe"], &[])
        ));
        assert!(!is_game(
            r"C:\Games\A\Game.exe",
            &windows,
            &config(&[], &["game.exe"])
        ));
    }

    #[test]
    fn toggling_flips_and_keeps_one_mark_at_most() {
        let windows = HashSet::from([r"c:\games\listed.exe".to_string()]);
        let mut c = GameConfig::default();
        toggle(r"C:\Apps\Tool.exe", &windows, &mut c);
        assert_eq!(c.games, ["tool.exe"]);
        toggle(r"C:\Apps\Tool.exe", &windows, &mut c);
        assert!(c.games.is_empty() && c.not_games.is_empty());
        toggle(r"C:\Games\Listed.exe", &windows, &mut c);
        assert_eq!(c.not_games, ["listed.exe"]);
        toggle(r"C:\Games\Listed.exe", &windows, &mut c);
        assert!(c.games.is_empty() && c.not_games.is_empty());
    }

    #[test]
    fn names_come_from_the_file() {
        assert_eq!(exe_name(r"C:\G\Elden Ring\eldenring.EXE"), "eldenring.exe");
        assert_eq!(display_name(r"C:\G\Elden Ring\eldenring.exe"), "eldenring");
    }

    #[test]
    fn a_session_averages_only_frames_drawn_in_front() {
        let mut s = Session::new("game".into());
        let snap = |fps: f64, low: f64, cpu: f64| {
            let mut s = Snapshot::default();
            s.set(MetricKey::AppFrameRate, fps);
            s.set(MetricKey::FrameRateLow1, low);
            s.set(MetricKey::CpuTempC, cpu);
            s
        };
        s.record(&snap(100.0, 80.0, 60.0), true);
        s.record(&snap(140.0, 100.0, 75.0), true);
        s.record(&snap(30.0, 20.0, 90.0), false);
        s.record(&snap(0.0, 0.0, 50.0), true);
        let sum = s.summary(Duration::from_secs(600));
        assert_eq!(sum.average_fps, Some(120.0));
        assert_eq!(sum.low_1, Some(100.0));
        assert_eq!(sum.cpu_max, Some(90.0));
        assert_eq!(sum.gpu_max, None);
    }

    #[test]
    fn the_low_is_the_slowest_frames_not_an_average() {
        let seconds: Vec<f64> = [120.0; 50].into_iter().chain([30.0; 50]).collect();
        assert_eq!(low_1(&seconds), Some(30.0));
        assert_eq!(low_1(&[]), None);
    }

    #[test]
    fn the_csv_quotes_names_and_leaves_missing_readings_empty() {
        let s = Summary {
            name: "Say \"hi\", game".into(),
            started: "2026-10-28 16:30".into(),
            duration: Duration::from_secs(90),
            average_fps: Some(143.6),
            low_1: None,
            cpu_max: Some(71.0),
            gpu_max: None,
        };
        assert_eq!(
            csv_line(&s),
            "2026-10-28 16:30,\"Say \"\"hi\"\", game\",1.5,144,,71,"
        );
    }

    #[test]
    fn sessions_append_under_one_header() {
        let dir = std::env::temp_dir().join(format!("pulsar-sessions-{}", std::process::id()));
        let path = dir.join("sessions.csv");
        let _ = std::fs::remove_file(&path);
        let s = Session::new("g".into()).summary(Duration::from_secs(60));
        log_session(&path, &s).unwrap();
        log_session(&path, &s).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert_eq!(text.lines().count(), 3);
        assert_eq!(text.lines().next(), Some(CSV_HEADER));
        let _ = std::fs::remove_dir_all(dir);
    }
}
