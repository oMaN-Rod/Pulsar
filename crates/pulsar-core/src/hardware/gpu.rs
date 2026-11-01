//! Graphics adapters as DirectX lists them.

use std::sync::OnceLock;
use windows::Win32::Graphics::Direct3D::D3D_FEATURE_LEVEL_11_0;
use windows::Win32::Graphics::Direct3D12::{
    D3D12_FEATURE_ARCHITECTURE, D3D12_FEATURE_DATA_ARCHITECTURE, D3D12CreateDevice, ID3D12Device,
};

use windows::Win32::Foundation::LUID;
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory1, DXGI_ADAPTER_DESC1, DXGI_ADAPTER_FLAG_SOFTWARE, IDXGIAdapter1, IDXGIDevice,
    IDXGIFactory1,
};
use windows::core::Interface;

const MICROSOFT: u32 = 0x1414;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GpuInfo {
    pub name: String,
    pub vendor: Option<&'static str>,
    pub vendor_id: u32,
    pub device_id: u32,
    pub dedicated_bytes: u64,
    pub shared_bytes: u64,
    pub driver: Option<String>,
    pub unified_memory: Option<bool>,
}

impl GpuInfo {
    /// Integrated parts carve their memory from system RAM.
    pub fn integrated(&self) -> bool {
        self.unified_memory
            .unwrap_or(self.dedicated_bytes < 512 * 1024 * 1024)
    }
}

fn unified_memory(adapter: &IDXGIAdapter1) -> Option<bool> {
    let mut device: Option<ID3D12Device> = None;
    unsafe { D3D12CreateDevice(adapter, D3D_FEATURE_LEVEL_11_0, &mut device) }.ok()?;
    let mut architecture = D3D12_FEATURE_DATA_ARCHITECTURE::default();
    unsafe {
        device?.CheckFeatureSupport(
            D3D12_FEATURE_ARCHITECTURE,
            (&mut architecture as *mut D3D12_FEATURE_DATA_ARCHITECTURE).cast(),
            size_of::<D3D12_FEATURE_DATA_ARCHITECTURE>() as u32,
        )
    }
    .ok()?;
    Some(architecture.UMA.as_bool())
}

pub fn vendor_name(id: u32) -> Option<&'static str> {
    Some(match id {
        0x10DE => "NVIDIA",
        0x1002 | 0x1022 => "AMD",
        0x8086 => "Intel",
        0x5143 => "Qualcomm",
        _ => return None,
    })
}

/// The driver's file version, `32.0.15.9144`; NVIDIA's own numbering is
/// the last five digits, `591.44`.
pub fn driver_version(raw: i64, vendor_id: u32) -> String {
    let raw = raw as u64;
    let parts = [
        raw >> 48,
        (raw >> 32) & 0xFFFF,
        (raw >> 16) & 0xFFFF,
        raw & 0xFFFF,
    ];
    if vendor_id == 0x10DE {
        let digits = (parts[2] % 10) * 10_000 + parts[3];
        return format!("{}.{:02}", digits / 100, digits % 100);
    }
    parts.map(|p| p.to_string()).join(".")
}

/// Packs a LUID the way PDH counter instance names spell it, high part first.
pub fn luid_key(luid: LUID) -> u64 {
    (u64::from(luid.HighPart as u32) << 32) | u64::from(luid.LowPart)
}

/// Hardware adapters, discrete first: the order per-adapter metrics use.
fn enumerate() -> Vec<(IDXGIAdapter1, DXGI_ADAPTER_DESC1, String)> {
    let Ok(factory) = (unsafe { CreateDXGIFactory1::<IDXGIFactory1>() }) else {
        return Vec::new();
    };
    let mut out: Vec<(IDXGIAdapter1, DXGI_ADAPTER_DESC1, String)> = Vec::new();
    for i in 0.. {
        let Ok(adapter) = (unsafe { factory.EnumAdapters1(i) }) else {
            break;
        };
        let Ok(desc) = (unsafe { adapter.GetDesc1() }) else {
            continue;
        };
        if desc.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32 != 0 || desc.VendorId == MICROSOFT {
            continue;
        }
        // An adapter shows once per output-capable node on some systems;
        // identical cards still differ by LUID.
        if out
            .iter()
            .any(|(_, d, _)| luid_key(d.AdapterLuid) == luid_key(desc.AdapterLuid))
        {
            continue;
        }
        let name_len = desc.Description.iter().position(|&c| c == 0).unwrap_or(128);
        let name =
            super::cpu::clean_brand(&String::from_utf16_lossy(&desc.Description[..name_len]));
        out.push((adapter, desc, name));
    }
    out.sort_by_key(|(_, d, _)| std::cmp::Reverse(d.DedicatedVideoMemory));
    let mut names: Vec<String> = out.iter().map(|(_, _, n)| n.clone()).collect();
    number_duplicates(&mut names);
    for ((_, _, name), unique) in out.iter_mut().zip(names) {
        *name = unique;
    }
    out
}

/// Tells identical cards apart: the second "RTX 4090" becomes "RTX 4090 #2".
pub fn number_duplicates(names: &mut [String]) {
    let original = names.to_vec();
    for (i, name) in names.iter_mut().enumerate() {
        let seen = original[..i].iter().filter(|n| **n == original[i]).count();
        if seen > 0 {
            *name = format!("{} #{}", original[i], seen + 1);
        }
    }
}

/// Each adapter's name and LUID, in the order `read` lists them.
pub fn adapters() -> Vec<(String, u64)> {
    enumerate()
        .into_iter()
        .map(|(_, desc, name)| (name, luid_key(desc.AdapterLuid)))
        .collect()
}

/// Adapter names, read once per process; per-adapter metrics number them
/// by their place in this list.
pub fn names() -> &'static [String] {
    static NAMES: OnceLock<Vec<String>> = OnceLock::new();
    NAMES.get_or_init(|| adapters().into_iter().map(|(name, _)| name).collect())
}

/// The discrete card comes first: it is the one the sensors describe.
pub fn read() -> Vec<GpuInfo> {
    enumerate()
        .into_iter()
        .map(|(adapter, desc, name)| GpuInfo {
            name,
            vendor: vendor_name(desc.VendorId),
            vendor_id: desc.VendorId,
            device_id: desc.DeviceId,
            dedicated_bytes: desc.DedicatedVideoMemory as u64,
            shared_bytes: desc.SharedSystemMemory as u64,
            driver: unsafe { adapter.CheckInterfaceSupport(&IDXGIDevice::IID) }
                .ok()
                .map(|v| driver_version(v, desc.VendorId)),
            unified_memory: unified_memory(&adapter),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "reads this machine's adapters"]
    fn probe() {
        for g in read() {
            println!(
                "{} uma={:?} integrated={}",
                g.name,
                g.unified_memory,
                g.integrated()
            );
        }
    }

    #[test]
    fn nvidia_drivers_read_as_nvidia_numbers_them() {
        let raw = (32i64 << 48) | (15 << 16) | 9144;
        assert_eq!(driver_version(raw, 0x10DE), "591.44");
        assert_eq!(driver_version(raw, 0x1002), "32.0.15.9144");
    }

    #[test]
    fn identical_cards_are_numbered() {
        let mut names = ["RTX 4090", "Radeon", "RTX 4090", "RTX 4090"].map(String::from);
        number_duplicates(&mut names);
        assert_eq!(names, ["RTX 4090", "Radeon", "RTX 4090 #2", "RTX 4090 #3"]);
    }

    #[test]
    fn luids_pack_high_part_first() {
        let luid = LUID {
            LowPart: 0xC5E6,
            HighPart: 1,
        };
        assert_eq!(luid_key(luid), 0x1_0000_C5E6);
    }

    #[test]
    fn vendors_are_named_by_pci_id() {
        assert_eq!(vendor_name(0x10DE), Some("NVIDIA"));
        assert_eq!(vendor_name(0x1234), None);
    }
}
