//! Crash reports: the panic hook writes one to the logs folder and marks it
//! pending, so the next start of the app can offer to open it.

use std::any::Any;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::logging::Stamp;

const PENDING: &str = "crash.pending";
const KEEP_REPORTS: usize = 5;

pub fn report_text(
    exe: &str,
    version: &str,
    stamp: &Stamp,
    message: &str,
    location: &str,
    backtrace: &str,
) -> String {
    format!(
        "{exe} {version} crashed at {}\r\n\r\n{message}\r\nat {location}\r\n\r\nBacktrace:\r\n{backtrace}\r\n",
        stamp.log_format()
    )
}

/// Writes the report, marks it as the pending one and keeps the newest few.
pub fn record(dir: &Path, exe: &str, stamp: &Stamp, text: &str) -> io::Result<PathBuf> {
    fs::create_dir_all(dir)?;
    let name = format!("crash-{}-{exe}.txt", stamp.file_format());
    let path = dir.join(&name);
    fs::write(&path, text)?;
    fs::write(dir.join(PENDING), &name)?;
    prune(dir);
    Ok(path)
}

/// The report written since the last call, if it still exists.
pub fn take_pending(dir: &Path) -> Option<PathBuf> {
    let marker = dir.join(PENDING);
    let name = fs::read_to_string(&marker).ok()?;
    let _ = fs::remove_file(&marker);
    let path = dir.join(name.trim());
    path.is_file().then_some(path)
}

fn panic_message(payload: &(dyn Any + Send)) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        s.to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "unknown panic".to_string()
    }
}

/// Logs a panic and writes a crash report to `dir`; the previous hook still runs.
pub fn install_panic_hook(dir: PathBuf, exe: &'static str, version: &'static str) {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let message = panic_message(info.payload());
        let location = info
            .location()
            .map_or_else(|| "an unknown location".to_string(), |l| l.to_string());
        log::error!("panic: {message} at {location}");
        let backtrace = std::backtrace::Backtrace::force_capture().to_string();
        let stamp = Stamp::now();
        let text = report_text(exe, version, &stamp, &message, &location, &backtrace);
        let _ = record(&dir, exe, &stamp, &text);
        previous(info);
    }));
}

fn prune(dir: &Path) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut reports: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("crash-") && n.ends_with(".txt"))
        })
        .collect();
    // Names start with the time, so they sort oldest first.
    reports.sort();
    let excess = reports.len().saturating_sub(KEEP_REPORTS);
    for old in &reports[..excess] {
        let _ = fs::remove_file(old);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stamp(second: u16) -> Stamp {
        Stamp {
            year: 2026,
            month: 9,
            day: 29,
            hour: 22,
            minute: 6,
            second,
            millis: 0,
        }
    }

    fn reports(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .filter(|n| n.starts_with("crash-"))
            .collect();
        names.sort();
        names
    }

    #[test]
    fn a_report_names_the_panic_and_where_it_happened() {
        let text = report_text(
            "pulsar",
            "0.1.0",
            &stamp(5),
            "boom",
            "src/app.rs:10:5",
            "0: main",
        );
        assert!(text.starts_with("pulsar 0.1.0 crashed at 2026-09-29 22:06:05.000\r\n"));
        assert!(text.contains("boom\r\nat src/app.rs:10:5\r\n"));
        assert!(text.ends_with("Backtrace:\r\n0: main\r\n"));
    }

    #[test]
    fn a_recorded_report_is_pending_once() {
        let dir = tempfile::tempdir().unwrap();
        let path = record(dir.path(), "pulsar", &stamp(5), "boom").unwrap();
        assert_eq!(path, dir.path().join("crash-20260929-220605-pulsar.txt"));
        assert_eq!(fs::read_to_string(&path).unwrap(), "boom");
        assert_eq!(take_pending(dir.path()), Some(path));
        assert_eq!(take_pending(dir.path()), None);
    }

    #[test]
    fn the_newest_report_is_the_pending_one() {
        let dir = tempfile::tempdir().unwrap();
        record(dir.path(), "pulsar-settings", &stamp(1), "a").unwrap();
        let newest = record(dir.path(), "pulsar", &stamp(2), "b").unwrap();
        assert_eq!(take_pending(dir.path()), Some(newest));
    }

    #[test]
    fn a_deleted_report_is_not_offered() {
        let dir = tempfile::tempdir().unwrap();
        let path = record(dir.path(), "pulsar", &stamp(5), "boom").unwrap();
        fs::remove_file(path).unwrap();
        assert_eq!(take_pending(dir.path()), None);
        assert!(!dir.path().join(PENDING).exists());
    }

    #[test]
    fn only_the_newest_reports_are_kept() {
        let dir = tempfile::tempdir().unwrap();
        for s in 0..8 {
            record(dir.path(), "pulsar", &stamp(s), "x").unwrap();
        }
        let kept = reports(dir.path());
        assert_eq!(kept.len(), KEEP_REPORTS);
        assert_eq!(kept[0], "crash-20260929-220603-pulsar.txt");
    }

    #[test]
    fn panic_payloads_become_messages() {
        let text: Box<dyn Any + Send> = Box::new("static");
        let formatted: Box<dyn Any + Send> = Box::new(String::from("formatted 1"));
        let other: Box<dyn Any + Send> = Box::new(7);
        assert_eq!(panic_message(text.as_ref()), "static");
        assert_eq!(panic_message(formatted.as_ref()), "formatted 1");
        assert_eq!(panic_message(other.as_ref()), "unknown panic");
    }

    #[test]
    fn the_panic_hook_writes_a_pending_report() {
        let dir = tempfile::tempdir().unwrap();
        install_panic_hook(dir.path().to_path_buf(), "test", "0.0.0");
        let result = std::panic::catch_unwind(|| panic!("hook test"));
        drop(std::panic::take_hook());
        assert!(result.is_err());
        // Any other test panicking meanwhile would also land here, so look
        // for this one's report rather than trusting the marker.
        let found = reports(dir.path()).iter().any(|name| {
            fs::read_to_string(dir.path().join(name))
                .unwrap()
                .contains("hook test")
        });
        assert!(found);
    }
}
