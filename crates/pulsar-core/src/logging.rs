//! A size-capped log: `<name>.log` rolls to `<name>.1.log`, `<name>.2.log`, …

use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use log::{Level, LevelFilter, Log, Metadata, Record};
use windows::Win32::System::SystemInformation::GetLocalTime;

pub const MAX_BYTES: u64 = 1024 * 1024;
pub const KEEP: usize = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stamp {
    pub year: u16,
    pub month: u16,
    pub day: u16,
    pub hour: u16,
    pub minute: u16,
    pub second: u16,
    pub millis: u16,
}

pub struct RollingFile {
    dir: PathBuf,
    name: String,
    max_bytes: u64,
    keep: usize,
    file: Option<File>,
    len: u64,
}

impl Stamp {
    pub fn now() -> Self {
        let t = unsafe { GetLocalTime() };
        Self {
            year: t.wYear,
            month: t.wMonth,
            day: t.wDay,
            hour: t.wHour,
            minute: t.wMinute,
            second: t.wSecond,
            millis: t.wMilliseconds,
        }
    }

    pub fn log_format(&self) -> String {
        format!(
            "{:04}-{:02}-{:02} {:02}:{:02}:{:02}.{:03}",
            self.year, self.month, self.day, self.hour, self.minute, self.second, self.millis
        )
    }

    /// Safe in file names and sorts chronologically.
    pub fn file_format(&self) -> String {
        format!(
            "{:04}{:02}{:02}-{:02}{:02}{:02}",
            self.year, self.month, self.day, self.hour, self.minute, self.second
        )
    }
}

pub fn format_line(stamp: &Stamp, level: Level, target: &str, message: &fmt::Arguments) -> String {
    format!("{} {level:<5} {target}: {message}", stamp.log_format())
}

impl RollingFile {
    pub fn new(dir: &Path, name: &str, max_bytes: u64, keep: usize) -> Self {
        debug_assert!(keep >= 2);
        Self {
            dir: dir.to_path_buf(),
            name: name.to_string(),
            max_bytes,
            keep,
            file: None,
            len: 0,
        }
    }

    pub fn path(&self, index: usize) -> PathBuf {
        match index {
            0 => self.dir.join(format!("{}.log", self.name)),
            i => self.dir.join(format!("{}.{i}.log", self.name)),
        }
    }

    pub fn write_line(&mut self, line: &str) -> io::Result<()> {
        let bytes = line.len() as u64 + 2;
        if self.file.is_none() {
            self.open()?;
        }
        if self.len > 0 && self.len + bytes > self.max_bytes {
            self.roll()?;
        }
        let Some(file) = self.file.as_mut() else {
            return Err(io::ErrorKind::NotFound.into());
        };
        write!(file, "{line}\r\n")?;
        self.len += bytes;
        Ok(())
    }

    fn open(&mut self) -> io::Result<()> {
        fs::create_dir_all(&self.dir)?;
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.path(0))?;
        self.len = file.metadata()?.len();
        self.file = Some(file);
        Ok(())
    }

    fn roll(&mut self) -> io::Result<()> {
        self.file = None;
        for i in (1..self.keep).rev() {
            let from = self.path(i - 1);
            if from.exists() {
                fs::rename(&from, self.path(i))?;
            }
        }
        self.open()
    }
}

struct Logger(Mutex<RollingFile>);

impl Log for Logger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= Level::Info
    }

    fn log(&self, record: &Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let line = format_line(
            &Stamp::now(),
            record.level(),
            record.target(),
            record.args(),
        );
        if let Ok(mut file) = self.0.lock() {
            let _ = file.write_line(&line);
        }
    }

    fn flush(&self) {}
}

/// Sends `log` records at info level and above to `<dir>\<name>.log`.
pub fn init(dir: &Path, name: &str) {
    let file = RollingFile::new(dir, name, MAX_BYTES, KEEP);
    if log::set_boxed_logger(Box::new(Logger(Mutex::new(file)))).is_ok() {
        log::set_max_level(LevelFilter::Info);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LINE: &str = "line 00 ........";

    fn read(path: &Path) -> String {
        fs::read_to_string(path).unwrap()
    }

    fn names(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        names
    }

    fn stamp() -> Stamp {
        Stamp {
            year: 2026,
            month: 9,
            day: 29,
            hour: 7,
            minute: 6,
            second: 5,
            millis: 42,
        }
    }

    #[test]
    fn stamps_format_for_lines_and_file_names() {
        assert_eq!(stamp().log_format(), "2026-09-29 07:06:05.042");
        assert_eq!(stamp().file_format(), "20260929-070605");
    }

    #[test]
    fn a_line_has_time_level_target_and_message() {
        let line = format_line(
            &stamp(),
            Level::Warn,
            "pulsar::app",
            &format_args!("x = {}", 1),
        );
        assert_eq!(line, "2026-09-29 07:06:05.042 WARN  pulsar::app: x = 1");
    }

    #[test]
    fn lines_append_to_the_current_file() {
        let dir = tempfile::tempdir().unwrap();
        let mut log = RollingFile::new(dir.path(), "t", 1000, 3);
        log.write_line("a").unwrap();
        log.write_line("b").unwrap();
        assert_eq!(read(&log.path(0)), "a\r\nb\r\n");
        assert_eq!(log.path(0), dir.path().join("t.log"));
        assert_eq!(log.path(2), dir.path().join("t.2.log"));
    }

    #[test]
    fn a_full_file_rolls_and_at_most_keep_files_remain() {
        let dir = tempfile::tempdir().unwrap();
        // Each line is 18 bytes with its CRLF, so two fit in 50.
        let mut log = RollingFile::new(dir.path(), "t", 50, 3);
        for i in 0..10 {
            log.write_line(&format!("line {i:02} ........")).unwrap();
        }
        assert_eq!(names(dir.path()), ["t.1.log", "t.2.log", "t.log"]);
        assert_eq!(
            read(&log.path(0)),
            "line 08 ........\r\nline 09 ........\r\n"
        );
        assert!(read(&log.path(1)).starts_with("line 06"));
        assert!(read(&log.path(2)).starts_with("line 04"));
    }

    #[test]
    fn an_existing_file_counts_toward_the_limit() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("t.log"), "x".repeat(40)).unwrap();
        let mut log = RollingFile::new(dir.path(), "t", 50, 3);
        log.write_line(LINE).unwrap();
        assert_eq!(read(&log.path(1)), "x".repeat(40));
        assert_eq!(read(&log.path(0)), format!("{LINE}\r\n"));
    }

    #[test]
    fn the_folder_is_created_on_first_write() {
        let dir = tempfile::tempdir().unwrap();
        let nested = dir.path().join("a").join("logs");
        let mut log = RollingFile::new(&nested, "t", 1000, 3);
        log.write_line(LINE).unwrap();
        assert!(nested.join("t.log").is_file());
    }

    #[test]
    fn an_unwritable_folder_is_an_error_not_a_panic() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("file");
        fs::write(&file, "").unwrap();
        let mut log = RollingFile::new(&file.join("logs"), "t", 1000, 3);
        assert!(log.write_line(LINE).is_err());
        assert!(log.write_line(LINE).is_err(), "and stays harmless");
    }
}
