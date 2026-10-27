//! The chipset's temperature, through a module that only reads it.

use crate::pawnio::PawnIo;

pub const MODULE: &[u8] = include_bytes!("../../resources/pawnio/IntelPCHThermal.bin");
/// The thermal controller's own PCI function, on older chipsets.
const ROUTE_PCI: u64 = 1;
/// A fixed window of the power management registers, on recent chipsets.
const ROUTE_PWRM: u64 = 2;
const VALID: u64 = 1 << 9;

/// °C from the raw register; all zeros or all ones mean no reading.
fn celsius(raw: u64, route: u64) -> Option<f64> {
    let value = raw & 0x1FF;
    if value == 0 || value == 0x1FF {
        return None;
    }
    match route {
        ROUTE_PCI => Some(value as f64 * 0.5 - 50.0),
        ROUTE_PWRM if raw & VALID != 0 => Some(value as f64),
        _ => None,
    }
}

pub struct Chipset(PawnIo);

impl Chipset {
    /// `None` where the module finds no Intel chipset it knows.
    pub fn connect() -> Option<Self> {
        let io = PawnIo::open().ok()?;
        io.load(MODULE).ok()?;
        Some(Self(io))
    }

    pub fn temp(&self) -> Option<f64> {
        let mut out = [0u64; 2];
        self.0.execute("ioctl_read_raw", &[], &mut out).ok()?;
        celsius(out[0], out[1])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn older_chipsets_count_half_degrees_from_minus_50() {
        assert_eq!(celsius(190, ROUTE_PCI), Some(45.0));
    }

    #[test]
    fn recent_chipsets_need_the_valid_bit() {
        assert_eq!(celsius(0x8000_0231, ROUTE_PWRM), Some(49.0));
        assert_eq!(celsius(0x31, ROUTE_PWRM), None);
    }

    #[test]
    fn empty_and_floating_registers_are_not_readings() {
        assert_eq!(celsius(0, ROUTE_PCI), None);
        assert_eq!(celsius(0x3FF, ROUTE_PWRM), None);
    }
}
