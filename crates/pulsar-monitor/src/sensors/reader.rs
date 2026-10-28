use std::arch::x86_64::{__cpuid, CpuidResult};
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::sleep;
use std::time::{Duration, Instant};

use pulsar_core::sensors::{CoreReadings, FramesWanted, Publisher, Sensor, Status};
use windows::Win32::System::SystemInformation::GROUP_AFFINITY;

use crate::chipset::Chipset;
use crate::cores;
use crate::frames::Frames;
use crate::gpu::Gpu;
use crate::memory::{self, IntelMemory, Timings};
use crate::pawnio::{PawnIo, PciLock};

const INTEL_MODULE: &[u8] = include_bytes!("../../resources/pawnio/IntelMSR.bin");
const AMD_MODULE: &[u8] = include_bytes!("../../resources/pawnio/AMDFamily17.bin");
const INTEL_TEMPERATURE_TARGET: u32 = 0x1A2;
const INTEL_PACKAGE_THERM_STATUS: u32 = 0x1B1;
const INTEL_CORE_THERM_STATUS: u32 = 0x19C;
const INTEL_PERF_STATUS: u32 = 0x198;
const INTEL_POWER_UNIT: u32 = 0x606;
const INTEL_PKG_ENERGY: u32 = 0x611;
const INTEL_DRAM_ENERGY: u32 = 0x619;
const INTEL_PP0_ENERGY: u32 = 0x639;
const INTEL_PP1_ENERGY: u32 = 0x641;
const INTEL_PLATFORM_INFO: u32 = 0xCE;
const INTEL_TURBO_RATIO_LIMIT: u32 = 0x1AD;
const INTEL_PKG_POWER_LIMIT: u32 = 0x610;
const INTEL_PKG_POWER_INFO: u32 = 0x614;
const INTEL_UNCORE_PERF_STATUS: u32 = 0x621;
const INTEL_MPERF: u32 = 0xE7;
/// Intel's reference clock, which the ring's ratio multiplies.
const BUS_MHZ: f64 = 100.0;
const INTEL_APERF: u32 = 0xE8;
const AMD_MPERF: u32 = 0xC000_00E7;
const AMD_APERF: u32 = 0xC000_00E8;
const AMD_POWER_UNIT: u32 = 0xC001_0299;
const AMD_CORE_ENERGY: u32 = 0xC001_029A;
const AMD_PKG_ENERGY: u32 = 0xC001_029B;
/// The thermal controller's current temperature, on the system management network.
const AMD_SMN_CUR_TEMP: u32 = 0x0005_9800;
const SAMPLE: Duration = Duration::from_secs(1);
/// How often to look for a driver that was missing or went away.
const RETRY: Duration = Duration::from_secs(10);
const STOP_POLL: Duration = Duration::from_millis(100);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Vendor {
    Intel,
    Amd,
}

fn vendor_of(id: &[u8; 12]) -> Option<Vendor> {
    match id {
        b"GenuineIntel" => Some(Vendor::Intel),
        b"AuthenticAMD" => Some(Vendor::Amd),
        _ => None,
    }
}

/// `__cpuid` is an unsafe function on older compilers and a safe one on newer.
#[allow(unused_unsafe)]
fn cpuid(leaf: u32) -> CpuidResult {
    unsafe { __cpuid(leaf) }
}

fn vendor() -> Option<Vendor> {
    let leaf = cpuid(0);
    let mut id = [0u8; 12];
    for (chunk, register) in id
        .as_chunks_mut::<4>()
        .0
        .iter_mut()
        .zip([leaf.ebx, leaf.edx, leaf.ecx])
    {
        chunk.copy_from_slice(&register.to_le_bytes());
    }
    vendor_of(&id)
}

/// The processor's marketing name, e.g. `AMD Ryzen 7 2700X Eight-Core Processor`.
fn brand() -> String {
    let bytes: Vec<u8> = (0x8000_0002..=0x8000_0004u32)
        .flat_map(|leaf| {
            let r = cpuid(leaf);
            [r.eax, r.ebx, r.ecx, r.edx]
        })
        .flat_map(u32::to_le_bytes)
        .take_while(|&b| b != 0)
        .collect();
    String::from_utf8_lossy(&bytes).trim().to_string()
}

/// Intel temperature in °C, for the package or a core: the CPU reports how
/// far it is below its throttling point, which the target register names.
fn intel_temp(target: u64, status: u64) -> f64 {
    let tj_max = match (target >> 16) & 0xFF {
        0 => 100,
        t => t as i32,
    };
    let below = ((status >> 16) & 0x7F) as i32;
    f64::from(tj_max - below)
}

/// Whether an Intel thermal status says the CPU is slowing itself down now.
fn intel_throttling(status: u64) -> bool {
    status & 1 != 0
}

/// A core's status carries a flag for whether its reading can be used.
fn intel_core_valid(status: u64) -> bool {
    status & (1 << 31) != 0
}

/// How far some AMD models' control temperature sits above the real one, to
/// make their fans spin up sooner.
fn amd_tctl_offset(brand: &str) -> f64 {
    const OFFSETS: [(&str, f64); 8] = [
        ("Ryzen 5 1600X", 20.0),
        ("Ryzen 7 1700X", 20.0),
        ("Ryzen 7 1800X", 20.0),
        ("Ryzen 7 2700X", 10.0),
        ("Threadripper 19", 27.0),
        ("Threadripper 29", 27.0),
        ("Threadripper 39", 0.0),
        ("Threadripper 59", 0.0),
    ];
    OFFSETS
        .iter()
        .find(|(model, _)| brand.contains(model))
        .map_or(0.0, |&(_, offset)| offset)
}

/// AMD die temperature in °C from the thermal controller's register: eighths
/// of a degree in the top bits, on a scale that starts at -49 °C when either
/// range flag is set.
fn amd_temp(raw: u32, tctl_offset: f64) -> f64 {
    let low_range = raw & (1 << 19) != 0 || (raw >> 16) & 0b11 == 0b11;
    let base = if low_range { -49.0 } else { 0.0 };
    f64::from(raw >> 21) / 8.0 + base - tctl_offset
}

/// Joules per count of the energy registers.
fn energy_unit(power_unit: u64) -> f64 {
    1.0 / f64::from(1u32 << ((power_unit >> 8) & 0x1F))
}

/// Watts per count of the power limit registers.
fn power_unit(power_unit: u64) -> f64 {
    1.0 / f64::from(1u32 << (power_unit & 0xF))
}

/// The package's design power, in watts.
fn intel_tdp(info: u64, watts_per_count: f64) -> Option<f64> {
    let raw = info & 0x7FFF;
    (raw > 0).then_some(raw as f64 * watts_per_count)
}

/// The sustained and burst package power limits, where enabled.
fn intel_power_limits(limit: u64, watts_per_count: f64) -> [Option<f64>; 2] {
    [(0, 15), (32, 47)].map(|(at, enabled)| {
        (limit & (1 << enabled) != 0).then(|| ((limit >> at) & 0x7FFF) as f64 * watts_per_count)
    })
}

/// The lowest ratio the CPU runs at.
fn intel_min_ratio(platform_info: u64) -> u64 {
    (platform_info >> 40) & 0xFF
}

/// The highest turbo ratio, which the fewest active cores get.
fn intel_max_turbo(limits: u64) -> Option<u64> {
    (0..8)
        .map(|group| (limits >> (group * 8)) & 0xFF)
        .max()
        .filter(|&ratio| ratio > 0)
}

/// The voltage a core asks its regulator for, from `IA32_PERF_STATUS`:
/// bits 47:32 in units of 2^-13 V. Zero means the field is not reported.
fn intel_vid(status: u64) -> Option<f64> {
    let raw = (status >> 32) & 0xFFFF;
    (raw > 0).then(|| raw as f64 / 8192.0)
}

/// The clock a core ran at while busy: the actual-performance counter
/// against the fixed-rate one, scaled to the rate the latter ticks at.
fn effective_mhz(before: (u64, u64), after: (u64, u64), base_mhz: f64) -> Option<f64> {
    let actual = after.0.wrapping_sub(before.0);
    let reference = after.1.wrapping_sub(before.1);
    (reference > 0).then(|| actual as f64 / reference as f64 * base_mhz)
}

/// Average watts between two readings of a 32-bit energy counter that wraps.
fn watts(before: u64, after: u64, joules_per_count: f64, elapsed: Duration) -> Option<f64> {
    let counts = (after as u32).wrapping_sub(before as u32);
    let seconds = elapsed.as_secs_f64();
    (seconds > 0.0).then(|| f64::from(counts) * joules_per_count / seconds)
}

enum Thermal {
    Intel,
    Amd { tctl_offset: f64, pci: PciLock },
}

/// An energy counter and its last reading.
struct Energy {
    msr: u32,
    last: Option<(u64, Instant)>,
}

impl Energy {
    fn new(msr: u32) -> Self {
        Self { msr, last: None }
    }

    /// `None` on the first reading and where the counter does not exist.
    fn watts(&mut self, io: &PawnIo, joules_per_count: f64) -> Option<f64> {
        let now = (io.read_msr(self.msr).ok()?, Instant::now());
        let (before, at) = self.last.replace(now)?;
        watts(before, now.0, joules_per_count, now.1 - at).filter(|w| *w >= 0.0)
    }
}

/// What one visit to a core reads.
#[derive(Clone, Copy, Default)]
struct CoreRead {
    temp: Option<f64>,
    volts: Option<f64>,
    /// Actual and reference performance counters.
    counters: Option<(u64, u64)>,
    energy: Option<u64>,
}

/// The values worth publishing up to the first core without one: readers
/// take the list as running from core 0.
fn leading(values: impl IntoIterator<Item = Option<f64>>) -> Vec<f64> {
    values.into_iter().map_while(|v| v).collect()
}

/// A loaded module and what the last sample left behind.
struct Session {
    io: PawnIo,
    vendor: Vendor,
    thermal: Thermal,
    cores: Vec<GROUP_AFFINITY>,
    base_mhz: Option<f64>,
    /// Zero where the platform has no energy counters.
    joules_per_count: f64,
    package: Energy,
    /// Intel's cores, graphics and memory domains.
    domains: Vec<(Sensor, Energy)>,
    previous: Vec<CoreRead>,
    previous_at: Option<Instant>,
    gpu: Option<Gpu>,
    chipset: Option<Chipset>,
    intel_memory: Option<IntelMemory>,
}

impl Session {
    fn connect() -> Result<Self, Status> {
        let vendor = vendor().ok_or(Status::UnsupportedCpu)?;
        let (module, unit_msr, energy_msr) = match vendor {
            Vendor::Intel => (INTEL_MODULE, INTEL_POWER_UNIT, INTEL_PKG_ENERGY),
            Vendor::Amd => (AMD_MODULE, AMD_POWER_UNIT, AMD_PKG_ENERGY),
        };
        let io = PawnIo::open().map_err(|_| Status::NoDriver)?;
        // The modules turn away families they do not know.
        io.load(module).map_err(|_| Status::UnsupportedCpu)?;
        let thermal = match vendor {
            Vendor::Intel => Thermal::Intel,
            Vendor::Amd => Thermal::Amd {
                tctl_offset: amd_tctl_offset(&brand()),
                pci: PciLock::open().map_err(|_| Status::ReadFailed)?,
            },
        };
        let joules_per_count = io.read_msr(unit_msr).map_or(0.0, energy_unit);
        let (family, model, _) = pulsar_core::hardware::cpu::display_family_model(cpuid(1).eax);
        let domains = match vendor {
            Vendor::Intel => vec![
                (Sensor::CpuCoresPower, Energy::new(INTEL_PP0_ENERGY)),
                (Sensor::CpuGraphicsPower, Energy::new(INTEL_PP1_ENERGY)),
                (Sensor::CpuDramPower, Energy::new(INTEL_DRAM_ENERGY)),
            ],
            Vendor::Amd => Vec::new(),
        };
        Ok(Self {
            io,
            vendor,
            thermal,
            cores: cores::physical_cores(),
            base_mhz: pulsar_core::hardware::cpu::base_mhz().map(f64::from),
            joules_per_count,
            package: Energy::new(energy_msr),
            domains,
            previous: Vec::new(),
            previous_at: None,
            gpu: Gpu::connect(),
            chipset: (vendor == Vendor::Intel).then(Chipset::connect).flatten(),
            intel_memory: (vendor == Vendor::Intel && memory::intel_supported(family, model))
                .then(IntelMemory::connect)
                .flatten(),
        })
    }

    /// Intel's power limits, multipliers and ring clock.
    fn intel_limits(&self) -> Vec<(Sensor, f64)> {
        let read = |msr| self.io.read_msr(msr).ok();
        let mut values = Vec::new();
        if let Some(unit) = read(INTEL_POWER_UNIT).map(power_unit) {
            values.extend(
                read(INTEL_PKG_POWER_INFO)
                    .and_then(|info| intel_tdp(info, unit))
                    .map(|w| (Sensor::CpuTdp, w)),
            );
            if let Some([pl1, pl2]) =
                read(INTEL_PKG_POWER_LIMIT).map(|l| intel_power_limits(l, unit))
            {
                values.extend(pl1.map(|w| (Sensor::CpuPowerLimit1, w)));
                values.extend(pl2.map(|w| (Sensor::CpuPowerLimit2, w)));
            }
        }
        values.extend(
            read(INTEL_PLATFORM_INFO)
                .map(intel_min_ratio)
                .filter(|&ratio| ratio > 0)
                .map(|ratio| (Sensor::CpuMinRatio, ratio as f64)),
        );
        values.extend(
            read(INTEL_TURBO_RATIO_LIMIT)
                .and_then(intel_max_turbo)
                .map(|ratio| (Sensor::CpuMaxRatio, ratio as f64)),
        );
        values.extend(
            read(INTEL_UNCORE_PERF_STATUS)
                .map(|status| status & 0x7F)
                .filter(|&ratio| ratio > 0)
                .map(|ratio| (Sensor::RingClock, ratio as f64 * BUS_MHZ)),
        );
        values
    }

    /// Reads every core's own registers, one core at a time.
    fn visit_cores(&self, target: Option<u64>) -> Vec<CoreRead> {
        let io = &self.io;
        cores::on_each(&self.cores, || {
            let read = |msr| io.read_msr(msr).ok();
            Some(match self.vendor {
                Vendor::Intel => CoreRead {
                    temp: target.and_then(|target| {
                        let status = read(INTEL_CORE_THERM_STATUS)?;
                        intel_core_valid(status).then(|| intel_temp(target, status))
                    }),
                    volts: read(INTEL_PERF_STATUS).and_then(intel_vid),
                    counters: read(INTEL_APERF).zip(read(INTEL_MPERF)),
                    energy: None,
                },
                Vendor::Amd => CoreRead {
                    temp: None,
                    volts: None,
                    counters: read(AMD_APERF).zip(read(AMD_MPERF)),
                    energy: read(AMD_CORE_ENERGY),
                },
            })
        })
    }

    /// Package temperature and throttling, then every per-core reading.
    fn cpu(&mut self) -> (Vec<(Sensor, f64)>, CoreReadings) {
        let mut values = Vec::new();
        let mut target = None;
        match &self.thermal {
            Thermal::Intel => {
                let registers = self
                    .io
                    .read_msr(INTEL_TEMPERATURE_TARGET)
                    .and_then(|t| Ok((t, self.io.read_msr(INTEL_PACKAGE_THERM_STATUS)?)));
                if let Ok((t, status)) = registers {
                    target = Some(t);
                    values.push((Sensor::CpuTemp, intel_temp(t, status)));
                    values.push((
                        Sensor::CpuThrottling,
                        f64::from(u8::from(intel_throttling(status))),
                    ));
                }
            }
            Thermal::Amd { tctl_offset, pci } => {
                values.extend(
                    self.io
                        .read_smn(AMD_SMN_CUR_TEMP, pci)
                        .map(|raw| (Sensor::CpuTemp, amd_temp(raw, *tctl_offset))),
                );
            }
        }
        let now = Instant::now();
        let reads = self.visit_cores(target);
        let elapsed = self.previous_at.map(|at| now - at);
        let pairs = || {
            reads.iter().zip(
                self.previous
                    .iter()
                    .map(Some)
                    .chain(std::iter::repeat(None)),
            )
        };
        let clocks =
            leading(pairs().map(|(now, before)| {
                effective_mhz(before?.counters?, now.counters?, self.base_mhz?)
            }));
        let power = leading(pairs().map(|(now, before)| {
            watts(
                before?.energy?,
                now.energy?,
                self.joules_per_count,
                elapsed?,
            )
        }));
        if self.vendor == Vendor::Amd && !power.is_empty() {
            values.push((Sensor::CpuCoresPower, power.iter().sum()));
        }
        let readings = CoreReadings {
            temps: leading(reads.iter().map(|r| r.temp)),
            clocks,
            voltages: leading(reads.iter().map(|r| r.volts)),
            power,
        };
        self.previous = reads;
        self.previous_at = Some(now);
        (values, readings)
    }

    /// Every sensor that answered; none at all means the driver went away.
    fn sample(&mut self) -> (Vec<(Sensor, f64)>, CoreReadings) {
        let (mut values, cores) = self.cpu();
        let unit = self.joules_per_count;
        values.extend(
            self.package
                .watts(&self.io, unit)
                .filter(|w| *w > 0.0)
                .map(|v| (Sensor::CpuPower, v)),
        );
        for (sensor, energy) in &mut self.domains {
            values.extend(energy.watts(&self.io, unit).map(|v| (*sensor, v)));
        }
        if self.vendor == Vendor::Intel {
            values.extend(self.intel_limits());
        }
        if let Some(gpu) = &mut self.gpu {
            values.extend(gpu.sample());
        }
        if let Some(temp) = self.chipset.as_ref().and_then(Chipset::temp) {
            values.push((Sensor::ChipsetTemp, temp));
        }
        let timings = match &self.thermal {
            Thermal::Intel => self.intel_memory.as_ref().and_then(IntelMemory::timings),
            Thermal::Amd { pci, .. } => memory::amd_timings_of(&self.io, pci),
        };
        values.extend(timings.iter().flat_map(Timings::values));
        (values, cores)
    }
}

/// Publishes the readings every second until `stop` is set.
pub fn run(stop: &AtomicBool) {
    let Ok(publisher) = Publisher::create() else {
        return;
    };
    let wanted = FramesWanted::create().ok();
    let mut frames = Frames::default();
    let mut session: Result<Session, Status> = Err(Status::NoDriver);
    let mut retry_at = Instant::now();
    while !stop.load(Ordering::Relaxed) {
        let asked = wanted.as_ref().is_some_and(FramesWanted::take);
        publisher.publish_frames(&frames.update(asked));
        if session.is_err() && Instant::now() >= retry_at {
            session = Session::connect();
            retry_at = Instant::now() + RETRY;
        }
        match &mut session {
            Ok(s) => {
                let (values, cores) = s.sample();
                if values.is_empty() {
                    session = Err(Status::ReadFailed);
                    publisher.publish(Status::ReadFailed, &[], &CoreReadings::default());
                } else {
                    publisher.publish(Status::Ok, &values, &cores);
                }
            }
            Err(reason) => publisher.publish(*reason, &[], &CoreReadings::default()),
        }
        let wake = Instant::now() + SAMPLE;
        while Instant::now() < wake && !stop.load(Ordering::Relaxed) {
            sleep(STOP_POLL);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn this_machine_has_a_known_vendor_and_a_brand() {
        assert!(vendor().is_some());
        assert!(!brand().is_empty());
        assert_eq!(vendor_of(b"AuthenticAMD"), Some(Vendor::Amd));
        assert_eq!(vendor_of(b"CentaurHauls"), None);
    }

    #[test]
    fn intel_temperature_is_the_distance_below_tj_max() {
        // TjMax 100 °C, 58 °C below it.
        assert_eq!(intel_temp(100 << 16, 0x8000_0000 | 58 << 16), 42.0);
        assert_eq!(intel_temp(90 << 16, 0), 90.0);
    }

    #[test]
    fn intel_status_flags_are_read_from_their_bits() {
        assert!(intel_throttling(0x8839_0001));
        assert!(!intel_throttling(0x8839_0000));
        assert!(intel_core_valid(0x8839_0000));
        assert!(!intel_core_valid(0x0039_0000));
    }

    #[test]
    fn a_missing_tj_max_is_taken_as_100() {
        assert_eq!(intel_temp(0, 30 << 16), 70.0);
    }

    #[test]
    fn amd_temperature_is_in_eighths_of_a_degree() {
        // 52.5 °C is 420 eighths.
        assert_eq!(amd_temp(420 << 21, 0.0), 52.5);
        assert_eq!(amd_temp(420 << 21, 10.0), 42.5);
    }

    #[test]
    fn amd_range_flags_shift_the_scale_down() {
        let raw = 808 << 21;
        assert_eq!(amd_temp(raw, 0.0), 101.0);
        assert_eq!(amd_temp(raw | 1 << 19, 0.0), 52.0);
        assert_eq!(amd_temp(raw | 0b11 << 16, 0.0), 52.0);
        assert_eq!(amd_temp(raw | 0b01 << 16, 0.0), 101.0);
    }

    #[test]
    fn only_some_amd_models_report_a_raised_temperature() {
        assert_eq!(
            amd_tctl_offset("AMD Ryzen 7 2700X Eight-Core Processor"),
            10.0
        );
        assert_eq!(
            amd_tctl_offset("AMD Ryzen Threadripper 2950X 16-Core Processor"),
            27.0
        );
        assert_eq!(amd_tctl_offset("AMD Ryzen 7 5800X3D 8-Core Processor"), 0.0);
        assert_eq!(
            amd_tctl_offset("AMD Ryzen 7 2700 Eight-Core Processor"),
            0.0
        );
    }

    #[test]
    fn the_requested_voltage_is_in_8192ths_of_a_volt() {
        assert_eq!(intel_vid(0x2A00_0000_2E00), Some(0x2A00 as f64 / 8192.0));
        assert_eq!(intel_vid(0x2E00), None);
    }

    #[test]
    fn effective_clock_scales_the_base_by_the_counter_ratio() {
        assert_eq!(effective_mhz((0, 0), (150, 100), 3400.0), Some(5100.0));
        assert_eq!(
            effective_mhz((10, 10), (10, 10), 3400.0),
            None,
            "an idle core"
        );
        assert_eq!(
            effective_mhz((u64::MAX - 49, u64::MAX), (100, 99), 1000.0),
            Some(1500.0),
            "counters that wrap"
        );
    }

    #[test]
    fn published_lists_stop_at_the_first_gap() {
        assert_eq!(leading([Some(1.0), Some(2.0), None, Some(4.0)]), [1.0, 2.0]);
        assert!(leading([None, Some(1.0)]).is_empty());
    }

    #[test]
    fn the_energy_unit_is_a_power_of_two_fraction() {
        // The usual value: 2^-14 J, about 61 µJ.
        assert_eq!(energy_unit(0x000A_0E03), 1.0 / 16384.0);
    }

    #[test]
    fn power_is_energy_over_time() {
        let unit = 1.0 / 16384.0;
        let second = Duration::from_secs(1);
        assert_eq!(watts(0, 16384 * 65, unit, second), Some(65.0));
        assert_eq!(watts(0, 16384 * 65, unit, second * 2), Some(32.5));
        assert_eq!(watts(0, 1, unit, Duration::ZERO), None);
    }

    #[test]
    fn the_energy_counter_wraps_at_32_bits() {
        let unit = 1.0 / 16384.0;
        let before = 0xFFFF_FFFF - 16383;
        assert_eq!(
            watts(before, 16384, unit, Duration::from_secs(1)),
            Some(2.0)
        );
    }

    /// Needs an elevated terminal and the PawnIO driver.
    #[test]
    #[ignore]
    fn print_this_machines_readings() {
        let mut session = Session::connect().unwrap_or_else(|s| panic!("{s:?}"));
        session.sample();
        sleep(SAMPLE);
        let (values, cores) = session.sample();
        for (sensor, value) in values {
            println!("{sensor:?}: {value}");
        }
        println!("{cores:?}");
    }

    #[test]
    fn power_limits_count_in_eighths_of_a_watt() {
        let unit = power_unit(0x000A_0E03);
        assert_eq!(unit, 0.125);
        assert_eq!(intel_tdp(0x0000_03E8, unit), Some(125.0));
        // PL1 125 W enabled, PL2 253 W enabled.
        let limit = (1 << 47) | (2024 << 32) | (1 << 15) | 1000;
        assert_eq!(intel_power_limits(limit, unit), [Some(125.0), Some(253.0)]);
        assert_eq!(intel_power_limits(1000, unit), [None, None]);
    }

    #[test]
    fn ratios_come_from_their_own_bytes() {
        assert_eq!(intel_min_ratio(0x0000_0800_0000_2200), 8);
        assert_eq!(intel_max_turbo(0x2F2F_3030_3131_3335), Some(0x35));
        assert_eq!(intel_max_turbo(0), None);
    }

    #[test]
    fn the_bundled_modules_are_signed() {
        // A signature length, then the signature, then the module.
        for module in [
            INTEL_MODULE,
            AMD_MODULE,
            crate::chipset::MODULE,
            memory::INTEL_MODULE,
        ] {
            let signature = u32::from_le_bytes(module[..4].try_into().unwrap()) as usize;
            assert!(signature > 0 && signature + 4 < module.len());
        }
    }
}
