//! Devices on the PCI bus as Windows lists them: their identity, and how
//! fast their link is running.

use windows::Win32::Devices::DeviceAndDriverInstallation::{
    DIGCF_ALLCLASSES, DIGCF_PRESENT, HDEVINFO, SETUP_DI_REGISTRY_PROPERTY, SP_DEVINFO_DATA,
    SPDRP_ADDRESS, SPDRP_BUSNUMBER, SPDRP_DEVICEDESC, SPDRP_FRIENDLYNAME, SPDRP_HARDWAREID,
    SetupDiDestroyDeviceInfoList, SetupDiEnumDeviceInfo, SetupDiGetClassDevsW,
    SetupDiGetDevicePropertyW, SetupDiGetDeviceRegistryPropertyW,
};
use windows::Win32::Devices::Properties::DEVPROPTYPE;
use windows::Win32::Foundation::DEVPROPKEY;
use windows::core::{GUID, w};

const PCI_DEVICE: GUID = GUID::from_u128(0x3ab22e31_8264_4b4e_9af5_a8d2d8e33e62);
const CURRENT_LINK_SPEED: u32 = 9;
const CURRENT_LINK_WIDTH: u32 = 10;
const MAX_LINK_SPEED: u32 = 11;
const MAX_LINK_WIDTH: u32 = 12;
/// Where Intel chipsets put their LPC or eSPI bridge.
const INTEL_LPC: (u32, u32, u32) = (0, 31, 0);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Ids {
    pub vendor: u16,
    pub device: u16,
    /// The board maker's vendor ID, for cards built by a partner.
    pub subsystem_vendor: u16,
    pub revision: u8,
}

/// PCI Express generation and lane count, now and at most.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Link {
    pub generation: u32,
    pub width: u32,
    pub max_generation: u32,
    pub max_width: u32,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PciDevice {
    pub name: String,
    pub ids: Ids,
    /// Bus, device and function.
    pub location: Option<(u32, u32, u32)>,
    pub link: Option<Link>,
}

/// `PCI\VEN_10DE&DEV_2484&SUBSYS_404C1458&REV_A1`, as Windows writes the
/// first hardware ID.
pub fn parse_ids(hardware_id: &str) -> Option<Ids> {
    let field = |name: &str| {
        hardware_id
            .to_ascii_uppercase()
            .split(['\\', '&'])
            .find_map(|part| part.strip_prefix(name).map(str::to_string))
    };
    let hex = |name: &str| u32::from_str_radix(&field(name)?, 16).ok();
    Some(Ids {
        vendor: hex("VEN_")? as u16,
        device: hex("DEV_")? as u16,
        subsystem_vendor: hex("SUBSYS_").map_or(0, |s| (s & 0xFFFF) as u16),
        revision: hex("REV_").map_or(0, |r| r as u8),
    })
}

/// The generation for the speed code Windows reports: 1 is 2.5 GT/s.
pub fn transfer_rate(generation: u32) -> Option<&'static str> {
    Some(match generation {
        1 => "2.5 GT/s",
        2 => "5 GT/s",
        3 => "8 GT/s",
        4 => "16 GT/s",
        5 => "32 GT/s",
        6 => "64 GT/s",
        _ => return None,
    })
}

/// Who built a board, from its subsystem vendor ID.
pub fn board_maker(id: u16) -> Option<&'static str> {
    Some(match id {
        0x1002 => "AMD",
        0x1025 => "Acer",
        0x1028 => "Dell",
        0x103C => "HP",
        0x1043 => "ASUS",
        0x10B0 => "Gainward",
        0x10DE => "NVIDIA",
        0x1458 => "Gigabyte",
        0x1462 => "MSI",
        0x148C => "PowerColor",
        0x1569 => "Palit",
        0x1682 => "XFX",
        0x17AA => "Lenovo",
        0x1849 => "ASRock",
        0x196E => "PNY",
        0x19DA => "Zotac",
        0x1B4C => "Galax",
        0x1DA2 => "Sapphire",
        0x3842 => "EVGA",
        0x7377 => "Colorful",
        0x8086 => "Intel",
        _ => return None,
    })
}

/// `Intel(R) LPC Controller/eSPI Controller (Z690) - 7A84` names the
/// chipset in brackets.
pub fn chipset_model(bridge_name: &str) -> Option<String> {
    bridge_name
        .split('(')
        .skip(1)
        .filter_map(|part| part.split_once(')'))
        .map(|(inside, _)| inside.trim())
        .find(|inside| !inside.is_empty() && !["R", "TM"].contains(inside))
        .map(str::to_string)
}

/// The chipset, where its LPC bridge says which one it is.
pub fn chipset(devices: &[PciDevice]) -> Option<String> {
    let bridge = devices
        .iter()
        .find(|d| d.ids.vendor == 0x8086 && d.location == Some(INTEL_LPC))?;
    chipset_model(&bridge.name).map(|model| format!("Intel {model}"))
}

fn registry_property(
    set: HDEVINFO,
    device: &SP_DEVINFO_DATA,
    which: SETUP_DI_REGISTRY_PROPERTY,
) -> Option<Vec<u8>> {
    let mut buffer = vec![0u8; 2048];
    let mut used = 0u32;
    unsafe {
        SetupDiGetDeviceRegistryPropertyW(
            set,
            device,
            which,
            None,
            Some(&mut buffer),
            Some(&mut used),
        )
    }
    .ok()?;
    buffer.truncate(used as usize);
    Some(buffer)
}

fn number(bytes: &[u8]) -> Option<u32> {
    Some(u32::from_le_bytes(bytes.get(..4)?.try_into().ok()?))
}

/// The first string of a property, which may hold several.
fn text(bytes: &[u8]) -> String {
    let units: Vec<u16> = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|&pair| u16::from_le_bytes(pair))
        .take_while(|&u| u != 0)
        .collect();
    String::from_utf16_lossy(&units)
}

fn pci_property(set: HDEVINFO, device: &SP_DEVINFO_DATA, pid: u32) -> Option<u32> {
    let key = DEVPROPKEY {
        fmtid: PCI_DEVICE,
        pid,
    };
    let mut kind = DEVPROPTYPE::default();
    let mut buffer = [0u8; 4];
    unsafe { SetupDiGetDevicePropertyW(set, device, &key, &mut kind, Some(&mut buffer), None, 0) }
        .ok()?;
    Some(u32::from_le_bytes(buffer)).filter(|&v| v > 0)
}

pub fn read() -> Vec<PciDevice> {
    let Ok(set) =
        (unsafe { SetupDiGetClassDevsW(None, w!("PCI"), None, DIGCF_ALLCLASSES | DIGCF_PRESENT) })
    else {
        return Vec::new();
    };
    let mut devices = Vec::new();
    for index in 0.. {
        let mut device = SP_DEVINFO_DATA {
            cbSize: size_of::<SP_DEVINFO_DATA>() as u32,
            ..Default::default()
        };
        if unsafe { SetupDiEnumDeviceInfo(set, index, &mut device) }.is_err() {
            break;
        }
        let Some(ids) =
            registry_property(set, &device, SPDRP_HARDWAREID).and_then(|id| parse_ids(&text(&id)))
        else {
            continue;
        };
        let name = registry_property(set, &device, SPDRP_FRIENDLYNAME)
            .or_else(|| registry_property(set, &device, SPDRP_DEVICEDESC))
            .map(|n| text(&n))
            .unwrap_or_default();
        let bus = registry_property(set, &device, SPDRP_BUSNUMBER).and_then(|b| number(&b));
        let address = registry_property(set, &device, SPDRP_ADDRESS).and_then(|a| number(&a));
        let link = |pid| pci_property(set, &device, pid);
        devices.push(PciDevice {
            name,
            ids,
            location: bus
                .zip(address)
                .map(|(bus, address)| (bus, address >> 16, address & 0xFFFF)),
            link: link(CURRENT_LINK_SPEED).map(|generation| Link {
                generation,
                width: link(CURRENT_LINK_WIDTH).unwrap_or(0),
                max_generation: link(MAX_LINK_SPEED).unwrap_or(0),
                max_width: link(MAX_LINK_WIDTH).unwrap_or(0),
            }),
        });
    }
    unsafe {
        let _ = SetupDiDestroyDeviceInfoList(set);
    }
    devices
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hardware_ids_split_into_their_fields() {
        let ids = parse_ids(r"PCI\VEN_10DE&DEV_2484&SUBSYS_404C1458&REV_A1").unwrap();
        assert_eq!(
            ids,
            Ids {
                vendor: 0x10DE,
                device: 0x2484,
                subsystem_vendor: 0x1458,
                revision: 0xA1,
            }
        );
        assert_eq!(board_maker(ids.subsystem_vendor), Some("Gigabyte"));
        assert!(parse_ids(r"ACPI\PNP0A08").is_none());
    }

    #[test]
    fn the_chipset_is_named_in_brackets() {
        assert_eq!(
            chipset_model("Intel(R) LPC Controller/eSPI Controller (Z690) - 7A84").as_deref(),
            Some("Z690")
        );
        assert_eq!(chipset_model("PCI standard ISA bridge"), None);
        let bridge = PciDevice {
            name: "Intel(R) LPC Controller/eSPI Controller (Z690) - 7A84".into(),
            ids: Ids {
                vendor: 0x8086,
                ..Ids::default()
            },
            location: Some(INTEL_LPC),
            link: None,
        };
        assert_eq!(chipset(&[bridge]).as_deref(), Some("Intel Z690"));
    }

    #[test]
    fn speed_codes_are_generations() {
        assert_eq!(transfer_rate(4), Some("16 GT/s"));
        assert_eq!(transfer_rate(0), None);
    }

    #[test]
    fn this_machine_has_pci_devices() {
        assert!(read().iter().any(|d| d.ids.vendor != 0));
    }
}
