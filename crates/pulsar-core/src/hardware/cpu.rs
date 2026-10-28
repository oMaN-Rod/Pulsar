//! What the processor says about itself through CPUID and what Windows
//! knows of its cores and caches.

use windows::Win32::Foundation::ERROR_INSUFFICIENT_BUFFER;
use windows::Win32::System::SystemInformation::{
    CacheData, CacheInstruction, CacheUnified, GetLogicalProcessorInformationEx,
    LOGICAL_PROCESSOR_RELATIONSHIP, RelationAll, RelationCache, RelationProcessorCore,
    RelationProcessorPackage, SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX,
};

use super::registry;

const PROCESSOR_KEY: &str = r"HARDWARE\DESCRIPTION\System\CentralProcessor\0";

#[derive(Clone, Debug, Default, PartialEq)]
pub struct CpuInfo {
    pub vendor: String,
    pub brand: String,
    pub family: u32,
    pub model: u32,
    pub stepping: u32,
    pub microarchitecture: Option<&'static str>,
    pub topology: Topology,
    pub caches: Vec<Cache>,
    pub base_mhz: Option<u32>,
    pub microcode: Option<u32>,
    pub features: Vec<&'static str>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Topology {
    pub packages: u32,
    pub cores: u32,
    pub threads: u32,
    /// Hybrid designs only: cores of the fastest class, and the rest.
    pub performance_cores: Option<u32>,
    pub efficient_cores: Option<u32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CacheKind {
    Data,
    Instruction,
    Unified,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cache {
    pub level: u8,
    pub kind: CacheKind,
    pub size_bytes: u32,
    pub ways: u8,
    /// How many of this cache there are.
    pub count: u32,
}

impl Cache {
    pub fn label(&self) -> String {
        match self.kind {
            CacheKind::Data => crate::tr!("L{} data", self.level),
            CacheKind::Instruction => crate::tr!("L{} instruction", self.level),
            CacheKind::Unified => format!("L{}", self.level),
        }
    }
}

pub fn read() -> CpuInfo {
    let id = cpuid::identify();
    let (topology, caches) = topology();
    CpuInfo {
        microarchitecture: microarchitecture(&id.vendor, id.family, id.model),
        vendor: id.vendor,
        brand: id.brand,
        family: id.family,
        model: id.model,
        stepping: id.stepping,
        topology,
        caches,
        base_mhz: base_mhz(),
        microcode: registry::binary(PROCESSOR_KEY, "Update Revision")
            .and_then(|b| microcode(&b))
            .filter(|&r| r != 0),
        features: id.features,
    }
}

/// The clock Windows measured at boot, close to the base clock that the
/// CPU's fixed-rate counters tick at.
pub fn base_mhz() -> Option<u32> {
    registry::dword(PROCESSOR_KEY, "~MHz").filter(|&mhz| mhz > 0)
}

/// Older builds store eight bytes with the revision in the high dword;
/// newer ones store just the revision.
fn microcode(bytes: &[u8]) -> Option<u32> {
    let revision = bytes.get(4..8).or_else(|| bytes.get(..4))?;
    Some(u32::from_le_bytes(revision.try_into().ok()?))
}

/// The brand string without trademark marks or padding:
/// `13th Gen Intel(R) Core(TM) i7-13700K` becomes `13th Gen Intel Core i7-13700K`.
pub fn clean_brand(brand: &str) -> String {
    let mut s = brand.to_string();
    for mark in ["(R)", "(r)", "(TM)", "(tm)", "\u{AE}", "\u{2122}"] {
        s = s.replace(mark, "");
    }
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Family and model as CPU-Z shows them, folding in the extended fields.
pub fn display_family_model(eax: u32) -> (u32, u32, u32) {
    let stepping = eax & 0xF;
    let base_model = (eax >> 4) & 0xF;
    let base_family = (eax >> 8) & 0xF;
    let family = if base_family == 0xF {
        base_family + ((eax >> 20) & 0xFF)
    } else {
        base_family
    };
    let model = if base_family == 0x6 || base_family == 0xF {
        base_model | (((eax >> 16) & 0xF) << 4)
    } else {
        base_model
    };
    (family, model, stepping)
}

pub fn microarchitecture(vendor: &str, family: u32, model: u32) -> Option<&'static str> {
    match (vendor, family) {
        ("GenuineIntel", 6) => Some(match model {
            0x4E | 0x5E => "Skylake",
            0x8E => "Kaby Lake",
            0x9E => "Coffee Lake",
            0xA5 | 0xA6 => "Comet Lake",
            0xA7 => "Rocket Lake",
            0x7D | 0x7E => "Ice Lake",
            0x8C | 0x8D => "Tiger Lake",
            0x97 | 0x9A => "Alder Lake",
            0xBE => "Alder Lake-N",
            0xB7 | 0xBA | 0xBF => "Raptor Lake",
            0xAA | 0xAC => "Meteor Lake",
            0xBD => "Lunar Lake",
            0xC5 | 0xC6 => "Arrow Lake",
            0xCC => "Panther Lake",
            _ => return None,
        }),
        ("AuthenticAMD", 0x17) => Some(match model {
            0x08 | 0x18 => "Zen+",
            0x00..=0x2F => "Zen",
            _ => "Zen 2",
        }),
        ("AuthenticAMD", 0x19) => Some(match model {
            0x40..=0x4F => "Zen 3+",
            0x00..=0x0F | 0x20..=0x5F => "Zen 3",
            _ => "Zen 4",
        }),
        ("AuthenticAMD", 0x1A) => Some("Zen 5"),
        _ => None,
    }
}

/// The instruction-set extensions worth listing, from the CPUID feature leaves.
pub fn features(
    leaf1: (u32, u32),
    leaf7: (u32, u32, u32),
    ext1: (u32, u32),
    amd: bool,
) -> Vec<&'static str> {
    let (ecx1, edx1) = leaf1;
    let (ebx7, ecx7, eax7_1) = leaf7;
    let (ecx_e, edx_e) = ext1;
    let bit = |reg: u32, n: u32| reg & (1 << n) != 0;
    [
        (bit(edx1, 23), "MMX"),
        (bit(edx1, 25), "SSE"),
        (bit(edx1, 26), "SSE2"),
        (bit(ecx1, 0), "SSE3"),
        (bit(ecx1, 9), "SSSE3"),
        (bit(ecx1, 19), "SSE4.1"),
        (bit(ecx1, 20), "SSE4.2"),
        (amd && bit(ecx_e, 6), "SSE4A"),
        (bit(edx_e, 29), "x86-64"),
        (bit(ecx1, 25), "AES"),
        (bit(ecx1, 28), "AVX"),
        (bit(ebx7, 5), "AVX2"),
        (bit(eax7_1, 4), "AVX-VNNI"),
        (bit(ebx7, 16), "AVX-512F"),
        (bit(ecx1, 12), "FMA3"),
        (bit(ecx1, 29), "F16C"),
        (bit(ebx7, 3) && bit(ebx7, 8), "BMI"),
        (bit(ebx7, 29), "SHA"),
        (bit(ecx7, 9), "VAES"),
        (bit(ecx1, 5), "VT-x"),
        (amd && bit(ecx_e, 2), "AMD-V"),
    ]
    .into_iter()
    .filter_map(|(present, name)| present.then_some(name))
    .collect()
}

#[cfg(target_arch = "x86_64")]
mod cpuid {
    use std::arch::x86_64::{__cpuid, __cpuid_count, CpuidResult};

    pub struct Identity {
        pub vendor: String,
        pub brand: String,
        pub family: u32,
        pub model: u32,
        pub stepping: u32,
        pub features: Vec<&'static str>,
    }

    /// `__cpuid` is an unsafe function on older compilers and a safe one on newer.
    #[allow(unused_unsafe)]
    fn leaf(leaf: u32, sub: u32) -> CpuidResult {
        unsafe {
            if sub == 0 {
                __cpuid(leaf)
            } else {
                __cpuid_count(leaf, sub)
            }
        }
    }

    fn text(registers: &[u32]) -> String {
        let bytes: Vec<u8> = registers
            .iter()
            .flat_map(|r| r.to_le_bytes())
            .take_while(|&b| b != 0)
            .collect();
        String::from_utf8_lossy(&bytes).trim().to_string()
    }

    pub fn identify() -> Identity {
        let l0 = leaf(0, 0);
        let vendor = text(&[l0.ebx, l0.edx, l0.ecx]);
        let l1 = leaf(1, 0);
        let (family, model, stepping) = super::display_family_model(l1.eax);
        let (l7, l7_1) = if l0.eax >= 7 {
            (leaf(7, 0), leaf(7, 1))
        } else {
            (
                CpuidResult {
                    eax: 0,
                    ebx: 0,
                    ecx: 0,
                    edx: 0,
                },
                CpuidResult {
                    eax: 0,
                    ebx: 0,
                    ecx: 0,
                    edx: 0,
                },
            )
        };
        let max_ext = leaf(0x8000_0000, 0).eax;
        let ext1 = if max_ext >= 0x8000_0001 {
            leaf(0x8000_0001, 0)
        } else {
            CpuidResult {
                eax: 0,
                ebx: 0,
                ecx: 0,
                edx: 0,
            }
        };
        let brand = if max_ext >= 0x8000_0004 {
            let regs: Vec<u32> = (0x8000_0002..=0x8000_0004)
                .flat_map(|l| {
                    let r = leaf(l, 0);
                    [r.eax, r.ebx, r.ecx, r.edx]
                })
                .collect();
            text(&regs)
        } else {
            String::new()
        };
        let amd = vendor == "AuthenticAMD";
        Identity {
            features: super::features(
                (l1.ecx, l1.edx),
                (l7.ebx, l7.ecx, l7_1.eax),
                (ext1.ecx, ext1.edx),
                amd,
            ),
            vendor,
            brand: super::clean_brand(&brand),
            family,
            model,
            stepping,
        }
    }
}

#[cfg(not(target_arch = "x86_64"))]
mod cpuid {
    pub struct Identity {
        pub vendor: String,
        pub brand: String,
        pub family: u32,
        pub model: u32,
        pub stepping: u32,
        pub features: Vec<&'static str>,
    }

    pub fn identify() -> Identity {
        let brand = super::registry::string(super::PROCESSOR_KEY, "ProcessorNameString")
            .unwrap_or_default();
        Identity {
            vendor: super::registry::string(super::PROCESSOR_KEY, "VendorIdentifier")
                .unwrap_or_default(),
            brand,
            family: 0,
            model: 0,
            stepping: 0,
            features: Vec::new(),
        }
    }
}

fn records(relation: LOGICAL_PROCESSOR_RELATIONSHIP) -> Vec<u64> {
    let mut bytes = 0u32;
    let sized = unsafe { GetLogicalProcessorInformationEx(relation, None, &mut bytes) };
    if sized.is_ok() || sized.is_err_and(|e| e.code() != ERROR_INSUFFICIENT_BUFFER.to_hresult()) {
        return Vec::new();
    }
    // u64 storage keeps the records aligned.
    let mut buffer = vec![0u64; (bytes as usize).div_ceil(8)];
    let start = buffer
        .as_mut_ptr()
        .cast::<SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX>();
    if unsafe { GetLogicalProcessorInformationEx(relation, Some(start), &mut bytes) }.is_err() {
        return Vec::new();
    }
    buffer.truncate((bytes as usize).div_ceil(8));
    buffer
}

fn topology() -> (Topology, Vec<Cache>) {
    let buffer = records(RelationAll);
    let bytes = buffer.len() * 8;
    let start = buffer.as_ptr().cast::<u8>();
    let mut t = Topology::default();
    let mut classes: Vec<u8> = Vec::new();
    let mut caches: Vec<Cache> = Vec::new();
    let mut offset = 0usize;
    while offset + size_of::<u32>() * 2 <= bytes {
        // Records are variable-length and each states its own size.
        let record = unsafe {
            &*start
                .add(offset)
                .cast::<SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX>()
        };
        if record.Size == 0 {
            break;
        }
        match record.Relationship {
            r if r == RelationProcessorCore => {
                let p = unsafe { &record.Anonymous.Processor };
                t.cores += 1;
                t.threads += p.GroupMask[0].Mask.count_ones();
                classes.push(p.EfficiencyClass);
            }
            r if r == RelationProcessorPackage => t.packages += 1,
            r if r == RelationCache => {
                let c = unsafe { &record.Anonymous.Cache };
                let kind = [
                    (CacheData, CacheKind::Data),
                    (CacheInstruction, CacheKind::Instruction),
                    (CacheUnified, CacheKind::Unified),
                ]
                .into_iter()
                .find_map(|(t, kind)| (t == c.Type).then_some(kind));
                if let Some(kind) = kind {
                    let cache = Cache {
                        level: c.Level,
                        kind,
                        size_bytes: c.CacheSize,
                        ways: c.Associativity,
                        count: 1,
                    };
                    match caches.iter_mut().find(|k| {
                        Cache {
                            count: k.count,
                            ..cache
                        } == **k
                    }) {
                        Some(existing) => existing.count += 1,
                        None => caches.push(cache),
                    }
                }
            }
            _ => {}
        }
        offset += record.Size as usize;
    }
    if let Some(&fastest) = classes.iter().max()
        && classes.iter().any(|&c| c != fastest)
    {
        let p = classes.iter().filter(|&&c| c == fastest).count() as u32;
        t.performance_cores = Some(p);
        t.efficient_cores = Some(t.cores - p);
    }
    // Hybrid parts have caches of several sizes at one level; the big ones go first.
    caches.sort_by_key(|c| (c.level, c.kind as u8, std::cmp::Reverse(c.size_bytes)));
    (t, caches)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn family_and_model_fold_in_the_extended_fields() {
        // i7-13700K: family 6, model 0xB7, stepping 1.
        assert_eq!(display_family_model(0x000B_0671), (6, 0xB7, 1));
        // Ryzen 7 5800X3D: family 0x19, model 0x21, stepping 2.
        assert_eq!(display_family_model(0x00A2_0F12), (0x19, 0x21, 2));
    }

    #[test]
    fn known_models_are_named() {
        assert_eq!(
            microarchitecture("GenuineIntel", 6, 0xB7),
            Some("Raptor Lake")
        );
        assert_eq!(microarchitecture("AuthenticAMD", 0x19, 0x21), Some("Zen 3"));
        assert_eq!(microarchitecture("AuthenticAMD", 0x19, 0x61), Some("Zen 4"));
        assert_eq!(microarchitecture("AuthenticAMD", 0x17, 0x08), Some("Zen+"));
        assert_eq!(microarchitecture("GenuineIntel", 6, 0x01), None);
    }

    #[test]
    fn features_come_from_their_bits() {
        let f = features(
            (1 << 0 | 1 << 28, 1 << 26),
            (1 << 5, 0, 0),
            (1 << 6, 1 << 29),
            false,
        );
        assert_eq!(f, ["SSE2", "SSE3", "x86-64", "AVX", "AVX2"]);
        let amd = features((0, 0), (0, 0, 0), (1 << 6 | 1 << 2, 0), true);
        assert_eq!(amd, ["SSE4A", "AMD-V"]);
    }

    #[test]
    fn brands_lose_their_trademark_marks() {
        assert_eq!(
            clean_brand("13th Gen Intel(R) Core(TM) i7-13700K  "),
            "13th Gen Intel Core i7-13700K"
        );
        assert_eq!(
            clean_brand("AMD Ryzen 7 5800X3D 8-Core Processor"),
            "AMD Ryzen 7 5800X3D 8-Core Processor"
        );
    }

    #[test]
    fn microcode_is_the_high_dword() {
        assert_eq!(microcode(&[0, 0, 0, 0, 0x2B, 0x01, 0, 0]), Some(0x12B));
        assert_eq!(microcode(&[0x0E, 0x01, 0, 0]), Some(0x10E));
        assert_eq!(microcode(&[0; 2]), None);
    }

    #[test]
    fn this_machine_has_cores_and_caches() {
        let info = read();
        assert!(!info.brand.is_empty());
        assert!(info.topology.cores >= 1);
        assert!(info.topology.threads >= info.topology.cores);
        assert!(info.caches.iter().any(|c| c.level == 1));
    }
}
