//! Writing every sample to a CSV file while the user records.

use std::fs::{self, File};
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};

use windows::Win32::System::SystemInformation::GetLocalTime;

use crate::metric::{ItemKind, MetricKey, Snapshot};

/// The readings recorded for an item, in column order.
fn item_columns(kind: ItemKind) -> &'static [MetricKey] {
    use MetricKey::*;
    match kind {
        ItemKind::Cpu => &[CpuTotal],
        ItemKind::CpuClock => &[CpuClockMhz],
        ItemKind::CpuTemp => &[CpuTempC],
        ItemKind::CpuPower => &[CpuPowerW],
        ItemKind::Ram => &[MemUsedPercent, MemUsedBytes],
        ItemKind::ChipsetTemp => &[ChipsetTempC],
        ItemKind::Gpu => &[GpuUtil, GpuDedicatedBytes],
        ItemKind::GpuTemp => &[GpuTempC],
        ItemKind::Disk => &[DiskActivePercent, DiskReadBps, DiskWriteBps],
        ItemKind::DiskTemp => &[DiskTempMaxC],
        ItemKind::Network => &[NetDownBps, NetUpBps],
        ItemKind::Ping => &[PingMs],
        ItemKind::Battery => &[BatteryPercent],
        ItemKind::Fps => &[AppFrameRate, FrameTimeMs],
    }
}

pub fn columns(items: &[ItemKind]) -> Vec<MetricKey> {
    items
        .iter()
        .flat_map(|&k| item_columns(k).iter().copied())
        .collect()
}

/// `CpuTempC` becomes `cpu_temp_c`.
fn column_name(key: MetricKey) -> String {
    let mut name = String::new();
    for (i, c) in format!("{key:?}").chars().enumerate() {
        if c.is_ascii_uppercase() {
            if i > 0 {
                name.push('_');
            }
            name.push(c.to_ascii_lowercase());
        } else {
            name.push(c);
        }
    }
    name
}

fn header(columns: &[MetricKey]) -> String {
    std::iter::once("time".to_string())
        .chain(columns.iter().map(|&k| column_name(k)))
        .collect::<Vec<_>>()
        .join(",")
}

/// Missing readings are left empty.
fn row(time: &str, columns: &[MetricKey], s: &Snapshot) -> String {
    std::iter::once(time.to_string())
        .chain(columns.iter().map(|&k| {
            s.get(k)
                .filter(|v| v.is_finite())
                .map_or_else(String::new, |v| {
                    let text = format!("{v:.2}");
                    text.trim_end_matches('0').trim_end_matches('.').to_string()
                })
        }))
        .collect::<Vec<_>>()
        .join(",")
}

fn local_time(date_sep: char, time_sep: char, between: char) -> String {
    let t = unsafe { GetLocalTime() };
    format!(
        "{:04}{date_sep}{:02}{date_sep}{:02}{between}{:02}{time_sep}{:02}{time_sep}{:02}",
        t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond
    )
}

pub struct Recorder {
    out: BufWriter<File>,
    columns: Vec<MetricKey>,
    pub path: PathBuf,
}

impl Recorder {
    /// Starts a new file in `dir`, named for the time, with a column for
    /// each reading of `items`.
    pub fn start(dir: &Path, items: &[ItemKind]) -> io::Result<Self> {
        fs::create_dir_all(dir)?;
        let stem = format!("readings-{}", local_time('-', '-', '-'));
        let (path, file) = (1..)
            .map(|n| match n {
                1 => dir.join(format!("{stem}.csv")),
                n => dir.join(format!("{stem}-{n}.csv")),
            })
            .find_map(|path| match File::create_new(&path) {
                Ok(file) => Some(Ok((path, file))),
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => None,
                Err(e) => Some(Err(e)),
            })
            .expect("an unused name exists")?;
        let mut out = BufWriter::new(file);
        let columns = columns(items);
        writeln!(out, "{}", header(&columns))?;
        out.flush()?;
        Ok(Self { out, columns, path })
    }

    /// Flushed at once, so the file is whole whenever recording stops.
    pub fn record(&mut self, s: &Snapshot) -> io::Result<()> {
        let time = local_time('-', ':', ' ');
        writeln!(self.out, "{}", row(&time, &self.columns, s))?;
        self.out.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn columns_follow_the_items_and_name_their_unit() {
        let cols = columns(&[ItemKind::Network, ItemKind::CpuTemp]);
        assert_eq!(
            cols,
            [
                MetricKey::NetDownBps,
                MetricKey::NetUpBps,
                MetricKey::CpuTempC
            ]
        );
        assert_eq!(header(&cols), "time,net_down_bps,net_up_bps,cpu_temp_c");
        for kind in ItemKind::ALL {
            assert!(!item_columns(kind).is_empty(), "{kind:?}");
        }
    }

    #[test]
    fn rows_trim_their_numbers_and_leave_gaps_empty() {
        let mut s = Snapshot::default();
        s.set(MetricKey::CpuTotal, 12.5);
        s.set(MetricKey::MemUsedBytes, 8_589_934_592.0);
        s.set(MetricKey::CpuTempC, f64::NAN);
        let cols = [
            MetricKey::CpuTotal,
            MetricKey::MemUsedBytes,
            MetricKey::CpuTempC,
            MetricKey::PingMs,
        ];
        assert_eq!(row("t", &cols, &s), "t,12.5,8589934592,,");
    }

    #[test]
    fn a_recording_starts_with_its_header_and_grows_a_row_a_sample() {
        let dir = std::env::temp_dir().join(format!("pulsar-recording-{}", std::process::id()));
        let mut r = Recorder::start(&dir, &[ItemKind::Cpu]).unwrap();
        let mut s = Snapshot::default();
        s.set(MetricKey::CpuTotal, 40.0);
        r.record(&s).unwrap();
        r.record(&s).unwrap();
        let text = fs::read_to_string(&r.path).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0], "time,cpu_total");
        assert!(lines[1].ends_with(",40"), "{}", lines[1]);
        drop(r);
        let _ = fs::remove_dir_all(dir);
    }
}
