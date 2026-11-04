use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::gpu_slots::{GpuReadings, GpuSlots};
use crate::pdh::PdhError;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SourceId {
    Cpu,
    Memory,
    DiskIo,
    DiskSpace,
    Network,
    Gpu,
    Ping,
    Processes,
    GpuSensors,
    SensorService,
    DriveTemps,
    Battery,
    Frames,
    ThermalZones,
    EnergyMeter,
    GpuPower,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemKind {
    Cpu,
    Ram,
    Disk,
    Network,
    Gpu,
    GpuTemp,
    GpuMemory,
    Ping,
    CpuTemp,
    CpuPower,
    CpuClock,
    DiskTemp,
    ChipsetTemp,
    Battery,
    Fps,
    GpuPower,
}

impl ItemKind {
    /// Default display order; adjacent pairs share a text-mode column.
    pub const ALL: [ItemKind; 16] = [
        ItemKind::Cpu,
        ItemKind::CpuTemp,
        ItemKind::CpuPower,
        ItemKind::CpuClock,
        ItemKind::Ram,
        ItemKind::ChipsetTemp,
        ItemKind::Gpu,
        ItemKind::GpuTemp,
        ItemKind::GpuMemory,
        ItemKind::GpuPower,
        ItemKind::Disk,
        ItemKind::DiskTemp,
        ItemKind::Network,
        ItemKind::Ping,
        ItemKind::Battery,
        ItemKind::Fps,
    ];

    pub fn source(self) -> SourceId {
        match self {
            ItemKind::Cpu | ItemKind::CpuClock => SourceId::Cpu,
            ItemKind::Ram => SourceId::Memory,
            ItemKind::Disk => SourceId::DiskIo,
            ItemKind::Network => SourceId::Network,
            ItemKind::Gpu | ItemKind::GpuMemory => SourceId::Gpu,
            ItemKind::GpuTemp => SourceId::GpuSensors,
            ItemKind::Ping => SourceId::Ping,
            ItemKind::CpuTemp | ItemKind::CpuPower | ItemKind::ChipsetTemp => {
                SourceId::SensorService
            }
            ItemKind::DiskTemp => SourceId::DriveTemps,
            ItemKind::Battery => SourceId::Battery,
            ItemKind::Fps => SourceId::Frames,
            ItemKind::GpuPower => SourceId::GpuPower,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            ItemKind::Cpu => crate::tr!("CPU"),
            ItemKind::Ram => crate::tr!("RAM"),
            ItemKind::Disk => crate::tr!("DSK"),
            ItemKind::Network => crate::tr!("NET"),
            ItemKind::Gpu => crate::tr!("GPU"),
            ItemKind::Ping => crate::tr!("PING"),
            ItemKind::GpuTemp => crate::tr!("TEMP"),
            ItemKind::GpuMemory => crate::tr!("VRAM"),
            ItemKind::CpuTemp => crate::tr!("CPU°"),
            ItemKind::CpuPower => crate::tr!("PWR"),
            ItemKind::CpuClock => crate::tr!("CLK"),
            ItemKind::DiskTemp => crate::tr!("DSK°"),
            ItemKind::ChipsetTemp => crate::tr!("PCH°"),
            ItemKind::Battery => crate::tr!("BAT"),
            ItemKind::Fps => crate::tr!("FPS"),
            ItemKind::GpuPower => crate::tr!("GPWR"),
        }
    }

    /// One-letter labels for compact text; network parts keep their arrows.
    pub fn short_label(self) -> &'static str {
        match self {
            ItemKind::Cpu => "C",
            ItemKind::Ram => "M",
            ItemKind::Gpu => "G",
            ItemKind::GpuTemp => "T",
            ItemKind::GpuMemory => "V",
            ItemKind::Disk => "D",
            ItemKind::Network => "↓",
            ItemKind::Ping => "P",
            ItemKind::CpuTemp => "H",
            ItemKind::CpuPower => "W",
            ItemKind::CpuClock => "F",
            ItemKind::DiskTemp => "S",
            ItemKind::ChipsetTemp => "B",
            ItemKind::Battery => "E",
            ItemKind::Fps => "R",
            ItemKind::GpuPower => "Q",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum GpuEngineKind {
    ThreeD,
    Compute,
    VideoDecode,
    VideoEncode,
    Copy,
    Other,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
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
    /// Cached pages that are free to reuse.
    MemStandbyBytes,
    /// Changed pages waiting to be written to disk.
    MemModifiedBytes,
    MemFreeBytes,
    MemPagedPoolBytes,
    MemNonPagedPoolBytes,
    /// Installed memory that Windows cannot use.
    MemHardwareReservedBytes,
    ProcessCount,
    ThreadCount,
    HandleCount,
    UptimeSeconds,
    DiskActivePercent,
    DiskReadBps,
    DiskWriteBps,
    /// Drive letter as an ASCII byte, e.g. `b'C'`.
    VolumeFreeBytes(u8),
    VolumeTotalBytes(u8),
    DriveActivePercent(u8),
    DriveUsedPercent(u8),
    NetDownBps,
    NetUpBps,
    /// Received and sent since sampling started.
    NetDownTotalBytes,
    NetUpTotalBytes,
    /// 2 with internet access, 1 with the local network only, 0 with neither.
    NetConnectivity,
    /// By place in `NetworkConfig::adapters`.
    NetAdapterDownBps(u8),
    NetAdapterUpBps(u8),
    GpuUtil,
    GpuEngine(GpuEngineKind),
    GpuDedicatedBytes,
    GpuDedicatedTotalBytes,
    GpuSharedBytes,
    /// The fullest card's.
    GpuMemoryPercent,
    /// The bytes in use that `GpuMemoryPercent` counts.
    GpuMemoryUsedBytes,
    /// By slot in `GpuSlots`.
    GpuAdapterUtil(u8),
    GpuAdapterMemoryPercent(u8),
    GpuAdapterMemoryUsedBytes(u8),
    GpuAdapterDedicatedBytes(u8),
    GpuAdapterSharedBytes(u8),
    GpuAdapterTempC(u8),
    GpuTempC,
    GpuFanRpm,
    /// Of the card's power limit, from the display driver.
    GpuPowerPercent,
    /// Every NVIDIA card's, together.
    GpuPowerW,
    GpuMemClockMhz,
    GpuHotSpotC,
    GpuMemTempC,
    CpuTempC,
    CpuPowerW,
    /// By physical core, in processor order.
    CpuCoreTempC(u16),
    CpuCoreClockMhz(u16),
    CpuCoreVolts(u16),
    CpuCorePowerW(u16),
    CpuCoresPowerW,
    CpuGraphicsPowerW,
    CpuDramPowerW,
    /// 1 while the CPU is thermally throttling, else 0.
    CpuThrottling,
    /// By physical drive number.
    DiskTempC(u8),
    /// The hottest drive.
    DiskTempMaxC,
    /// How old a `DiskTempC` is, for a drive that is not read every tick.
    DiskTempAgeSeconds(u8),
    /// 1 while the drive was last found asleep.
    DiskAsleep(u8),
    /// Limits a drive that is not read every tick reported with its reading.
    DiskWarningC(u8),
    DiskCriticalC(u8),
    /// The platform controller hub, the board's chipset.
    ChipsetTempC,
    CpuTdpW,
    CpuPowerLimit1W,
    CpuPowerLimit2W,
    /// The lowest and highest multipliers of the bus clock.
    CpuMinRatio,
    CpuMaxRatio,
    RingClockMhz,
    /// Half the memory transfer rate.
    MemClockMhz,
    MemControllerClockMhz,
    MemTiming(Timing),
    PingMs,
    PingLossPercent,
    BatteryPercent,
    /// 1 while on mains power, else 0.
    BatteryPluggedIn,
    /// 1 while charging, else 0.
    BatteryCharging,
    BatterySecondsLeft,
    /// Positive while charging, negative while discharging.
    BatteryRateW,
    BatteryRemainingWh,
    BatteryFullWh,
    BatteryDesignWh,
    BatteryHealthPercent,
    BatteryCycles,
    BatteryVolts,
    /// What the frame rate item shows: the foreground application's, or
    /// a pinned display's.
    FrameRate,
    /// The foreground application's, zero while it is not drawing.
    AppFrameRate,
    /// How often the picture changed on the display the item follows.
    DisplayFrameRate,
    FrameTimeMs,
    /// The rate the slowest 1% and 0.1% of recent frames would run at.
    FrameRateLow1,
    FrameRateLow01,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Timing {
    Cl,
    Rcd,
    Rp,
    Ras,
    Rc,
    CommandRate,
}

/// A metric with one value per core, such as `MetricKey::CpuCoreTempC`.
pub type PerCoreKey = fn(u16) -> MetricKey;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unit {
    Percent,
    Bytes,
    BytesPerSec,
    /// Bytes per second, shown in bits when the user asks for them.
    NetworkRate,
    Millis,
    MHz,
    Celsius,
    Rpm,
    Watts,
    Volts,
    Ratio,
    Cycles,
    WattHours,
    Seconds,
    /// A plain count.
    Count,
    FramesPerSec,
}

impl MetricKey {
    pub fn gpu_slot(self) -> Option<u8> {
        use MetricKey::*;
        match self {
            GpuAdapterUtil(slot)
            | GpuAdapterMemoryPercent(slot)
            | GpuAdapterMemoryUsedBytes(slot)
            | GpuAdapterDedicatedBytes(slot)
            | GpuAdapterSharedBytes(slot)
            | GpuAdapterTempC(slot) => Some(slot),
            _ => None,
        }
    }

    /// Whether this is numbered by place in `NetworkConfig::adapters`.
    pub fn is_net_adapter(self) -> bool {
        matches!(
            self,
            MetricKey::NetAdapterDownBps(_) | MetricKey::NetAdapterUpBps(_)
        )
    }

    pub fn unit(self) -> Unit {
        use MetricKey::*;
        match self {
            CpuTotal
            | CpuCore(_)
            | MemUsedPercent
            | DiskActivePercent
            | GpuUtil
            | GpuEngine(_)
            | GpuAdapterUtil(_)
            | GpuMemoryPercent
            | GpuAdapterMemoryPercent(_)
            | PingLossPercent
            | GpuPowerPercent
            | CpuThrottling
            | DriveActivePercent(_)
            | DriveUsedPercent(_)
            | BatteryPercent
            | BatteryHealthPercent => Unit::Percent,
            MemUsedBytes
            | MemAvailableBytes
            | MemTotalBytes
            | CommitBytes
            | CommitLimitBytes
            | MemStandbyBytes
            | MemModifiedBytes
            | MemFreeBytes
            | MemPagedPoolBytes
            | MemNonPagedPoolBytes
            | MemHardwareReservedBytes
            | VolumeFreeBytes(_)
            | VolumeTotalBytes(_)
            | GpuDedicatedBytes
            | GpuDedicatedTotalBytes
            | GpuSharedBytes
            | GpuMemoryUsedBytes
            | GpuAdapterMemoryUsedBytes(_)
            | GpuAdapterDedicatedBytes(_)
            | GpuAdapterSharedBytes(_)
            | NetDownTotalBytes
            | NetUpTotalBytes => Unit::Bytes,
            ProcessCount | ThreadCount | HandleCount | BatteryPluggedIn | BatteryCharging
            | BatteryCycles | DiskAsleep(_) | NetConnectivity => Unit::Count,
            UptimeSeconds | BatterySecondsLeft | DiskTempAgeSeconds(_) => Unit::Seconds,
            BatteryRemainingWh | BatteryFullWh | BatteryDesignWh => Unit::WattHours,
            DiskReadBps | DiskWriteBps => Unit::BytesPerSec,
            NetDownBps | NetUpBps | NetAdapterDownBps(_) | NetAdapterUpBps(_) => Unit::NetworkRate,
            PingMs | FrameTimeMs => Unit::Millis,
            FrameRate | AppFrameRate | DisplayFrameRate | FrameRateLow1 | FrameRateLow01 => {
                Unit::FramesPerSec
            }
            CpuClockMhz
            | GpuMemClockMhz
            | CpuCoreClockMhz(_)
            | RingClockMhz
            | MemClockMhz
            | MemControllerClockMhz => Unit::MHz,
            CpuMinRatio | CpuMaxRatio => Unit::Ratio,
            MemTiming(_) => Unit::Cycles,
            CpuCoreVolts(_) | BatteryVolts => Unit::Volts,
            GpuTempC | GpuAdapterTempC(_) | GpuHotSpotC | GpuMemTempC | CpuTempC
            | CpuCoreTempC(_) | DiskTempC(_) | DiskTempMaxC | DiskWarningC(_)
            | DiskCriticalC(_) | ChipsetTempC => Unit::Celsius,
            GpuFanRpm => Unit::Rpm,
            CpuPowerW | CpuCorePowerW(_) | CpuCoresPowerW | CpuGraphicsPowerW | CpuDramPowerW
            | CpuTdpW | CpuPowerLimit1W | CpuPowerLimit2W | BatteryRateW | GpuPowerW => Unit::Watts,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AdapterRate {
    pub name: String,
    pub down_bps: f64,
    pub up_bps: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProcessUsage {
    pub name: String,
    pub cpu_percent: f64,
    pub private_bytes: f64,
    pub gpu_percent: f64,
    /// File, network and device reads and writes together.
    pub io_bps: f64,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Snapshot {
    pub metrics: HashMap<MetricKey, f64>,
    pub errors: HashMap<SourceId, String>,
    pub adapters: Vec<AdapterRate>,
    /// The latest pings, oldest first; `None` was lost.
    pub ping_attempts: Vec<Option<f64>>,
    pub processes: Vec<ProcessUsage>,
    pub frames: FrameLabels,
    pub gpus: GpuSlots,
    /// For the sampler, which files them under `gpus` and empties this.
    pub gpu_readings: GpuReadings,
}

/// Names for the frame rate readings.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct FrameLabels {
    /// The foreground application.
    pub app: Option<String>,
    /// The display `DisplayFrameRate` is of.
    pub display: Option<String>,
    /// Whether the item shows that display rather than the application.
    pub pinned: bool,
}

impl Snapshot {
    pub fn get(&self, key: MetricKey) -> Option<f64> {
        self.metrics.get(&key).copied()
    }

    pub fn set(&mut self, key: MetricKey, value: f64) {
        self.metrics.insert(key, value);
    }

    /// Why `kind` has no reading. A sensor item that Windows' own thermal
    /// zones or energy meters filled in has none.
    pub fn item_error(&self, kind: ItemKind) -> Option<&String> {
        let error = self.errors.get(&kind.source())?;
        let filled = matches!(kind, ItemKind::CpuTemp | ItemKind::CpuPower)
            && self.get(crate::layout::primary_metric(kind)).is_some();
        (!filled).then_some(error)
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
    /// For a source with work of its own between samples.
    fn set_paused(&mut self, _paused: bool) {}
}
