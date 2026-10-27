//! The memory controller's clock and primary timings, read from its
//! registers: on Intel through a module that only reads them, on AMD over
//! the system management network.

use pulsar_core::sensors::Sensor;

use crate::pawnio::{PawnIo, PciLock};

pub const INTEL_MODULE: &[u8] = include_bytes!("../../resources/pawnio/IntelMCHBAR.bin");
/// Alder Lake and Raptor Lake, whose register layout this decodes.
const INTEL_MODELS: [u32; 5] = [0x97, 0x9A, 0xB7, 0xBA, 0xBF];
/// The second controller's registers follow the first's.
const INTEL_MC_STRIDE: u64 = 0x10000;
const INTEL_CHANNEL_STRIDE: u64 = 0x800;
const INTEL_MAD_DIMM: u64 = 0xD80C;
const INTEL_TC_PRE: u64 = 0xE000;
const INTEL_TC_ODT: u64 = 0xE070;
const INTEL_SC_GS_CFG: u64 = 0xE088;
const INTEL_SA_PERF_STATUS: u64 = 0x5918;
const INTEL_MC_BIOS_DATA: u64 = 0x5E04;
const INTEL_BCLK_FREQ: u64 = 0x5F60;
/// Desktop sockets have two; larger ones more.
const AMD_UMCS: u32 = 4;
const AMD_UMC_SDP_CTRL: u32 = 0x104;
const AMD_UMC_CONFIG: u32 = 0x100;
const AMD_UMC_TIMING: u32 = 0x200;
const AMD_UMC_DTR1: u32 = 0x204;
const AMD_UMC_DTR2: u32 = 0x208;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Timings {
    /// Half the transfer rate.
    pub clock_mhz: Option<f64>,
    pub controller_mhz: Option<f64>,
    pub cl: u64,
    pub rcd: u64,
    pub rp: u64,
    pub ras: u64,
    pub rc: u64,
    pub command_rate: Option<u64>,
}

impl Timings {
    pub fn values(&self) -> Vec<(Sensor, f64)> {
        let mut values = vec![
            (Sensor::MemoryCl, self.cl as f64),
            (Sensor::MemoryRcd, self.rcd as f64),
            (Sensor::MemoryRp, self.rp as f64),
            (Sensor::MemoryRas, self.ras as f64),
            (Sensor::MemoryRc, self.rc as f64),
        ];
        values.extend(self.clock_mhz.map(|c| (Sensor::MemoryClock, c)));
        values.extend(
            self.controller_mhz
                .map(|c| (Sensor::MemoryControllerClock, c)),
        );
        values.extend(
            self.command_rate
                .map(|cr| (Sensor::MemoryCommandRate, cr as f64)),
        );
        values
    }

    /// All zeros is a controller that is not set up.
    fn valid(self) -> Option<Self> {
        (self.cl > 0 && self.rcd > 0 && self.rp > 0 && self.ras > 0).then_some(self)
    }
}

pub fn intel_supported(family: u32, model: u32) -> bool {
    family == 6 && INTEL_MODELS.contains(&model)
}

/// A channel holds a DIMM when either of its size fields is set.
fn intel_populated(mad_dimm: u64) -> bool {
    mad_dimm != 0xFFFF_FFFF && mad_dimm & 0x007F_007F != 0
}

/// The timings in one channel's command timing registers.
fn intel_timings(tc_pre: u64, tc_odt: u64, sc_gs_cfg: u64) -> Timings {
    let rp = tc_pre & 0xFF;
    let ras = (tc_pre >> 42) & 0x1FF;
    Timings {
        cl: (tc_odt >> 16) & 0x7F,
        rcd: (tc_pre >> 51) & 0xFF,
        rp,
        ras,
        // The controller keeps no tRC of its own.
        rc: ras + rp,
        command_rate: match (sc_gs_cfg >> 3) & 0b11 {
            0b11 => None,
            stretch => Some(stretch + 1),
        },
        ..Timings::default()
    }
}

/// Memory and controller clocks in MHz: the controller runs at a ratio of
/// its reference clock, and the memory at one, two or four times that.
fn intel_clocks(sa_perf_status: u64, mc_bios_data: u64, bclk_khz: u64) -> Option<(f64, f64)> {
    let ratio = (sa_perf_status >> 2) & 0xFF;
    let reference = match (mc_bios_data >> 8) & 0xF {
        0 => 400.0 / 3.0,
        1 => 100.0,
        _ => return None,
    };
    let gear = f64::from(1u32 << ((mc_bios_data >> 12) & 0b11));
    let scale = if bclk_khz > 0 {
        bclk_khz as f64 / 100_000.0
    } else {
        1.0
    };
    let controller = ratio as f64 * reference * scale / 2.0;
    (ratio > 0).then_some((controller * gear, controller))
}

/// DDR5 bursts 16 beats; DDR4 eight.
fn amd_is_ddr5(config: u32) -> bool {
    (config >> 8) & 0b11 == 0b11
}

/// The memory clock and command rate from the first memory state's register.
fn amd_clock(timing: u32, ddr5: bool) -> (Option<f64>, u64) {
    if ddr5 {
        // LPDDR5's clock runs at a further multiple this does not decode.
        let wck = (timing >> 26) & 0b11;
        let clock = f64::from(timing & 0xFFFF);
        let clock = (wck == 0 && clock > 0.0).then_some(clock);
        (clock, u64::from((timing >> 17) & 1) + 1)
    } else {
        let ratio = f64::from(timing & 0x7F);
        let clock = (ratio > 0.0).then_some(ratio * 100.0 / 3.0);
        (clock, u64::from((timing >> 10) & 1) + 1)
    }
}

fn amd_timings(timing: u32, dtr1: u32, dtr2: u32, ddr5: bool) -> Timings {
    let (clock_mhz, command_rate) = amd_clock(timing, ddr5);
    Timings {
        clock_mhz,
        controller_mhz: None,
        cl: u64::from(dtr1 & 0x3F),
        ras: u64::from((dtr1 >> 8) & 0x7F),
        rcd: u64::from((dtr1 >> 16) & 0x3F),
        rc: u64::from(dtr2 & 0xFF),
        rp: u64::from((dtr2 >> 16) & 0x3F),
        command_rate: Some(command_rate),
    }
}

/// Intel's controller registers, on a handle of their own.
pub struct IntelMemory(PawnIo);

impl IntelMemory {
    pub fn connect() -> Option<Self> {
        let io = PawnIo::open().ok()?;
        io.load(INTEL_MODULE).ok()?;
        Some(Self(io))
    }

    fn read(&self, offset: u64, qword: bool) -> Option<u64> {
        let mut value = [0u64];
        let function = if qword {
            "ioctl_read_qword"
        } else {
            "ioctl_read_dword"
        };
        self.0.execute(function, &[offset], &mut value).ok()?;
        Some(value[0])
    }

    /// The first channel with a DIMM in it.
    fn channel(&self) -> Option<u64> {
        (0..2).find_map(|mc| {
            (0..2).find_map(|ch| {
                let mad = self.read(mc * INTEL_MC_STRIDE + INTEL_MAD_DIMM + ch * 4, false)?;
                intel_populated(mad).then_some(mc * INTEL_MC_STRIDE + ch * INTEL_CHANNEL_STRIDE)
            })
        })
    }

    pub fn timings(&self) -> Option<Timings> {
        let base = self.channel()?;
        let mut timings = intel_timings(
            self.read(base + INTEL_TC_PRE, true)?,
            self.read(base + INTEL_TC_ODT, true)?,
            self.read(base + INTEL_SC_GS_CFG, true)?,
        );
        let clocks = intel_clocks(
            self.read(INTEL_SA_PERF_STATUS, false)?,
            self.read(INTEL_MC_BIOS_DATA, false)?,
            self.read(INTEL_BCLK_FREQ, false).unwrap_or(0),
        );
        if let Some((clock, controller)) = clocks {
            timings.clock_mhz = Some(clock);
            timings.controller_mhz = Some(controller);
        }
        timings.valid()
    }
}

/// AMD's unified memory controllers, through the already loaded CPU module.
pub fn amd_timings_of(io: &PawnIo, pci: &PciLock) -> Option<Timings> {
    let read = |address: u32| io.read_smn(address, pci).filter(|&v| v != 0xFFFF_FFFF);
    (0..AMD_UMCS).find_map(|n| {
        let base = 0x50000 + (n << 20);
        let sdp = read(base + AMD_UMC_SDP_CTRL)?;
        if sdp & (1 << 31) == 0 {
            return None;
        }
        let ddr5 = amd_is_ddr5(read(base + AMD_UMC_CONFIG)?);
        amd_timings(
            read(base + AMD_UMC_TIMING)?,
            read(base + AMD_UMC_DTR1)?,
            read(base + AMD_UMC_DTR2)?,
            ddr5,
        )
        .valid()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intel_timings_decode_a_ddr5_4800_channel() {
        // 40-40-40-76 at command rate 2T.
        let tc_pre = (40 << 51) | (76 << 42) | 40;
        let tc_odt = 40 << 16;
        let sc_gs_cfg = (1 << 31) | (0b01 << 3);
        let t = intel_timings(tc_pre, tc_odt, sc_gs_cfg);
        assert_eq!((t.cl, t.rcd, t.rp, t.ras, t.rc), (40, 40, 40, 76, 116));
        assert_eq!(t.command_rate, Some(2));
        assert_eq!(intel_timings(0, 0, 0b11 << 3).command_rate, None);
    }

    #[test]
    fn intel_clocks_follow_ratio_reference_and_gear() {
        // Ratio 24 of 100 MHz in gear 2: DDR5-4800.
        let status = (1 << 10) | (24 << 2);
        let bios = (1 << 12) | (1 << 8);
        assert_eq!(intel_clocks(status, bios, 100_000), Some((2400.0, 1200.0)));
        // Ratio 27 of 133 MHz in gear 1: DDR4-3600.
        let (clock, controller) = intel_clocks(27 << 2, 0, 0).unwrap();
        assert!((clock - 1800.0).abs() < 1e-9 && clock == controller);
        assert_eq!(intel_clocks(0, bios, 100_000), None);
    }

    #[test]
    fn empty_intel_channels_are_skipped() {
        assert!(intel_populated(0x0000_0020));
        assert!(!intel_populated(0));
        assert!(!intel_populated(0xFFFF_FFFF));
    }

    #[test]
    fn only_known_intel_generations_are_decoded() {
        assert!(intel_supported(6, 0xB7));
        assert!(!intel_supported(6, 0xAA));
        assert!(!intel_supported(0x19, 0x97));
    }

    #[test]
    fn amd_ddr4_counts_its_clock_in_thirds_of_100_mhz() {
        // DDR4-3600 18-22-22-42, tRC 64, 1T.
        let t = amd_timings(54, (22 << 16) | (42 << 8) | 18, (22 << 16) | 64, false);
        assert_eq!((t.cl, t.rcd, t.rp, t.ras, t.rc), (18, 22, 22, 42, 64));
        assert_eq!(t.clock_mhz, Some(1800.0));
        assert_eq!(t.command_rate, Some(1));
        assert_eq!(amd_clock(54 | 1 << 10, false).1, 2);
    }

    #[test]
    fn amd_ddr5_keeps_its_clock_in_mhz() {
        assert!(amd_is_ddr5(0x300));
        assert!(!amd_is_ddr5(0x200));
        assert_eq!(amd_clock(3000 | 1 << 17, true), (Some(3000.0), 2));
        assert_eq!(amd_clock(3000 | 1 << 26, true).0, None);
    }

    #[test]
    fn unset_timings_are_not_published() {
        assert_eq!(Timings::default().valid(), None);
    }
}
