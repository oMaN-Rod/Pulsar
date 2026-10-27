//! Static facts about the machine for the details window. Nothing here
//! needs administrator rights.

pub mod cpu;
pub mod gpu;
pub mod network;
pub mod os;
pub mod pci;
mod registry;
pub mod smbios;
pub mod storage;

pub use cpu::CpuInfo;
pub use gpu::GpuInfo;
pub use network::NetInfo;
pub use os::OsInfo;
pub use pci::PciDevice;
pub use smbios::Smbios;
pub use storage::{DiskInfo, DriveTempSource};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct HardwareInfo {
    pub cpu: CpuInfo,
    pub firmware: Smbios,
    pub gpus: Vec<GpuInfo>,
    pub disks: Vec<DiskInfo>,
    pub network: Vec<NetInfo>,
    pub os: OsInfo,
    pub pci: Vec<PciDevice>,
}

/// Takes a fraction of a second, mostly in DirectX and the disk queries.
pub fn collect() -> HardwareInfo {
    HardwareInfo {
        cpu: cpu::read(),
        firmware: smbios::read().unwrap_or_default(),
        gpus: gpu::read(),
        disks: storage::read(),
        network: network::read(),
        os: os::read(),
        pci: pci::read(),
    }
}
