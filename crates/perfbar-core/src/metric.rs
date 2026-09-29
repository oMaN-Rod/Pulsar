use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::pdh::PdhError;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SourceId {
    Cpu,
    Memory,
    DiskIo,
    DiskSpace,
    Network,
    Gpu,
    Ping,
    Processes,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemKind {
    Cpu,
    Ram,
    Disk,
    Network,
    Gpu,
    Ping,
}

impl ItemKind {
    pub const ALL: [ItemKind; 6] = [
        ItemKind::Cpu,
        ItemKind::Ram,
        ItemKind::Disk,
        ItemKind::Network,
        ItemKind::Gpu,
        ItemKind::Ping,
    ];

    pub fn source(self) -> SourceId {
        match self {
            ItemKind::Cpu => SourceId::Cpu,
            ItemKind::Ram => SourceId::Memory,
            ItemKind::Disk => SourceId::DiskIo,
            ItemKind::Network => SourceId::Network,
            ItemKind::Gpu => SourceId::Gpu,
            ItemKind::Ping => SourceId::Ping,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            ItemKind::Cpu => "CPU",
            ItemKind::Ram => "RAM",
            ItemKind::Disk => "DSK",
            ItemKind::Network => "NET",
            ItemKind::Gpu => "GPU",
            ItemKind::Ping => "PING",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum GpuEngineKind {
    ThreeD,
    Compute,
    VideoDecode,
    VideoEncode,
    Copy,
    Other,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MetricKey {
    CpuTotal,
    CpuCore(u16),
    CpuClockMhz,
    MemUsedPercent,
    MemUsedBytes,
    MemAvailableBytes,
    MemTotalBytes,
    CommitBytes,
    CommitLimitBytes,
    DiskActivePercent,
    DiskReadBps,
    DiskWriteBps,
    /// Drive letter as an ASCII byte, e.g. `b'C'`.
    VolumeFreeBytes(u8),
    VolumeTotalBytes(u8),
    NetDownBps,
    NetUpBps,
    GpuUtil,
    GpuEngine(GpuEngineKind),
    GpuDedicatedBytes,
    GpuSharedBytes,
    PingMs,
    PingLossPercent,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unit {
    Percent,
    Bytes,
    BytesPerSec,
    Millis,
    MHz,
}

impl MetricKey {
    pub fn unit(self) -> Unit {
        use MetricKey::*;
        match self {
            CpuTotal | CpuCore(_) | MemUsedPercent | DiskActivePercent | GpuUtil | GpuEngine(_)
            | PingLossPercent => Unit::Percent,
            MemUsedBytes | MemAvailableBytes | MemTotalBytes | CommitBytes | CommitLimitBytes
            | VolumeFreeBytes(_) | VolumeTotalBytes(_) | GpuDedicatedBytes | GpuSharedBytes => {
                Unit::Bytes
            }
            DiskReadBps | DiskWriteBps | NetDownBps | NetUpBps => Unit::BytesPerSec,
            PingMs => Unit::Millis,
            CpuClockMhz => Unit::MHz,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct AdapterRate {
    pub name: String,
    pub down_bps: f64,
    pub up_bps: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ProcessUsage {
    pub name: String,
    pub cpu_percent: f64,
    pub private_bytes: f64,
    pub gpu_percent: f64,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Snapshot {
    pub metrics: HashMap<MetricKey, f64>,
    pub errors: HashMap<SourceId, String>,
    pub adapters: Vec<AdapterRate>,
    pub processes: Vec<ProcessUsage>,
}

impl Snapshot {
    pub fn get(&self, key: MetricKey) -> Option<f64> {
        self.metrics.get(&key).copied()
    }

    pub fn set(&mut self, key: MetricKey, value: f64) {
        self.metrics.insert(key, value);
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SourceError {
    #[error(transparent)]
    Pdh(#[from] PdhError),
    #[error(transparent)]
    Win32(#[from] windows::core::Error),
    #[error("{0}")]
    Unavailable(String),
}

pub trait Source: Send {
    fn id(&self) -> SourceId;
    fn sample(&mut self, out: &mut Snapshot) -> Result<(), SourceError>;
}
