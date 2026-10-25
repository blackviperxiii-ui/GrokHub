//! Hardware profile v2 for on-device models: every GPU (vendor, VRAM,
//! integrated or unified), CPU cores and SIMD, RAM, power and thermal state,
//! and free disk. Probes sit behind traits so each OS plugs in its own readers
//! and tests plug in mocked machines; every field has a conservative default
//! when a reader finds nothing. The parsers here are pure.

use crate::local_picker::{self, CpuSimd};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GpuVendor {
    Nvidia,
    Amd,
    Intel,
    Apple,
    Other,
}

impl GpuVendor {
    /// PCI vendor id as sysfs writes it (`0x10de`).
    pub fn from_pci_id(id: &str) -> Self {
        match id.trim().to_ascii_lowercase().as_str() {
            "0x10de" => GpuVendor::Nvidia,
            "0x1002" | "0x1022" => GpuVendor::Amd,
            "0x8086" => GpuVendor::Intel,
            "0x106b" => GpuVendor::Apple,
            _ => GpuVendor::Other,
        }
    }

    pub fn from_name(name: &str) -> Self {
        let n = name.to_ascii_lowercase();
        if n.contains("nvidia") || n.contains("geforce") || n.contains("quadro") {
            GpuVendor::Nvidia
        } else if n.contains("amd") || n.contains("radeon") {
            GpuVendor::Amd
        } else if n.contains("intel") {
            GpuVendor::Intel
        } else if n.contains("apple") {
            GpuVendor::Apple
        } else {
            GpuVendor::Other
        }
    }
}

/// One GPU. VRAM in MiB; zero when the reader couldn't tell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Gpu {
    pub vendor: GpuVendor,
    pub name: String,
    pub vram_mb: u64,
    pub free_vram_mb: Option<u64>,
    /// An iGPU: it shares system RAM, so the RAM budget covers it.
    pub integrated: bool,
    /// Apple Silicon: one pool of unified memory.
    pub unified: bool,
    pub driver: String,
}

impl Gpu {
    fn discrete(&self) -> bool {
        !self.integrated && !self.unified
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CpuInfo {
    pub logical: u32,
    pub physical: u32,
    /// Performance cores (all physical cores on a non-hybrid CPU).
    pub p_cores: u32,
    pub e_cores: u32,
    pub simd: CpuSimd,
}

impl Default for CpuInfo {
    fn default() -> Self {
        CpuInfo {
            logical: 2,
            physical: 2,
            p_cores: 2,
            e_cores: 0,
            simd: CpuSimd::detect(),
        }
    }
}

impl CpuInfo {
    /// Hybrid CPUs list efficiency threads apart (Intel `cpu_atom`, Apple perflevel1).
    pub fn new(logical: u32, physical: u32, e_cores: u32, simd: CpuSimd) -> Self {
        let physical = physical.max(1);
        CpuInfo {
            logical: logical.max(physical),
            physical,
            p_cores: physical.saturating_sub(e_cores).max(1),
            e_cores: e_cores.min(physical.saturating_sub(1)),
            simd,
        }
    }
}

/// Battery and power source. Unknown reads as a desktop on AC.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PowerInfo {
    pub battery: bool,
    pub on_ac: bool,
    pub percent: Option<u8>,
    pub power_saver: bool,
}

impl Default for PowerInfo {
    fn default() -> Self {
        PowerInfo {
            battery: false,
            on_ac: true,
            percent: None,
            power_saver: false,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
pub enum Thermal {
    #[default]
    Unknown,
    Nominal,
    Fair,
    Serious,
    Critical,
}

/// Everything the auto-config, the picker and the governor read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalProfile {
    pub os: &'static str,
    pub arch: &'static str,
    pub gpus: Vec<Gpu>,
    pub cpu: CpuInfo,
    pub ram_mb: u64,
    pub free_ram_mb: Option<u64>,
    pub power: PowerInfo,
    pub thermal: Thermal,
    pub free_disk_mb: u64,
}

pub trait GpuProbe {
    fn gpus(&self) -> Vec<Gpu>;
}

pub trait CpuProbe {
    fn cpu(&self) -> CpuInfo;
}

pub trait MemoryProbe {
    fn ram_mb(&self) -> Option<u64>;
    fn free_ram_mb(&self) -> Option<u64>;
    fn free_disk_mb(&self) -> Option<u64>;
}

pub trait PowerProbe {
    fn power(&self) -> Option<PowerInfo>;
}

pub trait ThermalProbe {
    fn thermal(&self) -> Thermal;
}

/// The readers for one machine. GPU readers run in order; the first that
/// finds a GPU wins (NVML or `nvidia-smi`, then DRM / DXGI, then Vulkan).
pub struct Probes<'a> {
    pub gpu: &'a [&'a dyn GpuProbe],
    pub cpu: &'a dyn CpuProbe,
    pub memory: &'a dyn MemoryProbe,
    pub power: &'a dyn PowerProbe,
    pub thermal: &'a dyn ThermalProbe,
}

pub fn profile(p: &Probes) -> LocalProfile {
    let gpus = p
        .gpu
        .iter()
        .map(|g| g.gpus())
        .find(|v| !v.is_empty())
        .unwrap_or_default();
    LocalProfile {
        os: std::env::consts::OS,
        arch: std::env::consts::ARCH,
        gpus,
        cpu: p.cpu.cpu(),
        ram_mb: p.memory.ram_mb().unwrap_or(0),
        free_ram_mb: p.memory.free_ram_mb(),
        power: p.power.power().unwrap_or_default(),
        thermal: p.thermal.thermal(),
        free_disk_mb: p.memory.free_disk_mb().unwrap_or(0),
    }
}

/// The GPUs a model runs on, and how layers split across them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DevicePlan {
    pub gpus: Vec<Gpu>,
    /// `--tensor-split` weights (free VRAM per GPU) when two or more share the work.
    pub tensor_split: Option<Vec<u64>>,
}

impl DevicePlan {
    pub fn vram_mb(&self) -> u64 {
        self.gpus.iter().map(|g| g.vram_mb).sum()
    }

    pub fn unified(&self) -> bool {
        self.gpus.iter().any(|g| g.unified)
    }
}

impl LocalProfile {
    /// Discrete GPUs of the vendor with the most VRAM (one backend), else the
    /// Apple unified GPU. iGPUs are left out: they share RAM.
    pub fn device_plan(&self) -> DevicePlan {
        let discrete: Vec<&Gpu> = self.gpus.iter().filter(|g| g.discrete()).collect();
        let vendor = [
            GpuVendor::Nvidia,
            GpuVendor::Amd,
            GpuVendor::Intel,
            GpuVendor::Other,
        ]
        .into_iter()
        .max_by_key(|v| {
            discrete
                .iter()
                .filter(|g| g.vendor == *v)
                .map(|g| g.vram_mb)
                .sum::<u64>()
        });
        let gpus: Vec<Gpu> = match vendor {
            Some(v) if discrete.iter().any(|g| g.vendor == v) => discrete
                .into_iter()
                .filter(|g| g.vendor == v)
                .cloned()
                .collect(),
            _ => self.gpus.iter().filter(|g| g.unified).cloned().collect(),
        };
        let tensor_split = (gpus.len() > 1).then(|| {
            gpus.iter()
                .map(|g| g.free_vram_mb.unwrap_or(g.vram_mb))
                .collect()
        });
        DevicePlan { gpus, tensor_split }
    }

    /// What the model picker reads.
    pub fn picker_profile(&self, llama_build: Option<u32>) -> local_picker::Profile {
        let plan = self.device_plan();
        local_picker::Profile {
            gpus_vram_mb: if plan.unified() {
                Vec::new()
            } else {
                plan.gpus.iter().map(|g| g.vram_mb).collect()
            },
            unified: plan.unified(),
            ram_mb: self.ram_mb,
            free_disk_mb: self.free_disk_mb,
            simd: self.cpu.simd,
            llama_build,
        }
    }
}

/// llama.cpp backends, most preferred first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Backend {
    Cuda,
    Rocm,
    Metal,
    Vulkan,
    Sycl,
    Cpu,
}

pub const BACKEND_ORDER: [Backend; 6] = [
    Backend::Cuda,
    Backend::Rocm,
    Backend::Metal,
    Backend::Vulkan,
    Backend::Sycl,
    Backend::Cpu,
];

impl Backend {
    /// The ggml library name part (`libggml-cuda.so`, `ggml-hip.dll`).
    fn lib_tag(self) -> &'static str {
        match self {
            Backend::Cuda => "cuda",
            Backend::Rocm => "hip",
            Backend::Metal => "metal",
            Backend::Vulkan => "vulkan",
            Backend::Sycl => "sycl",
            Backend::Cpu => "cpu",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Backend::Cuda => "CUDA",
            Backend::Rocm => "ROCm",
            Backend::Metal => "Metal",
            Backend::Vulkan => "Vulkan",
            Backend::Sycl => "SYCL",
            Backend::Cpu => "CPU",
        }
    }

    fn runs_on(self, g: &Gpu) -> bool {
        match self {
            Backend::Cuda => g.vendor == GpuVendor::Nvidia,
            Backend::Rocm => g.vendor == GpuVendor::Amd && !g.integrated,
            Backend::Metal => g.vendor == GpuVendor::Apple,
            Backend::Vulkan => g.vendor != GpuVendor::Apple,
            Backend::Sycl => g.vendor == GpuVendor::Intel,
            Backend::Cpu => true,
        }
    }
}

/// The backends an installed engine carries, from the files next to
/// `llama-server`. The CPU backend is always there.
pub fn engine_backends(files: &[&str]) -> Vec<Backend> {
    BACKEND_ORDER
        .into_iter()
        .filter(|b| {
            *b == Backend::Cpu
                || files.iter().any(|f| {
                    let f = f.to_ascii_lowercase();
                    f.contains(&format!("ggml-{}", b.lib_tag()))
                })
        })
        .collect()
}

/// The most preferred backend the engine carries, that runs on this
/// machine's GPUs and whose speed test passed; else the CPU.
pub fn choose_backend(
    p: &LocalProfile,
    available: &[Backend],
    bench_ok: &dyn Fn(Backend) -> bool,
) -> Backend {
    let plan = p.device_plan();
    // An iGPU-only machine may still offload through Vulkan or SYCL.
    let targets: Vec<&Gpu> = if plan.gpus.is_empty() {
        p.gpus.iter().collect()
    } else {
        plan.gpus.iter().collect()
    };
    BACKEND_ORDER
        .into_iter()
        .filter(|b| *b != Backend::Cpu)
        .find(|b| {
            available.contains(b)
                && !targets.is_empty()
                && targets.iter().all(|g| b.runs_on(g))
                && bench_ok(*b)
        })
        .unwrap_or(Backend::Cpu)
}

/// `nvidia-smi --query-gpu=name,memory.total,memory.free,driver_version
/// --format=csv,noheader,nounits`: one GPU per line.
pub fn parse_nvidia_smi_all(text: &str) -> Vec<Gpu> {
    text.lines()
        .filter_map(|l| {
            let cols: Vec<&str> = l.split(',').map(str::trim).collect();
            let [name, total, free, driver] = cols[..] else {
                return None;
            };
            let vram_mb: u64 = total.parse().ok()?;
            (!name.is_empty()).then(|| Gpu {
                vendor: GpuVendor::Nvidia,
                name: name.into(),
                vram_mb,
                free_vram_mb: free.parse().ok(),
                integrated: false,
                unified: false,
                driver: driver.into(),
            })
        })
        .collect()
}

/// One `/sys/class/drm/cardN/device` as read from sysfs.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DrmCard {
    /// `vendor` (`0x1002`).
    pub vendor: String,
    /// `DRIVER=` from `uevent` (`amdgpu`, `i915`, `xe`).
    pub driver: String,
    /// `mem_info_vram_total` (amdgpu) bytes.
    pub vram_total: Option<u64>,
    /// `mem_info_vram_used` bytes.
    pub vram_used: Option<u64>,
    /// Intel discrete local memory bytes (`lmem_total_bytes` on i915, `tile0/vram0/size` on xe).
    pub lmem_total: Option<u64>,
}

/// AMD below this much VRAM is an APU's carve-out, not a card.
pub const APU_CARVEOUT_MAX_MB: u64 = 2_048;

/// GPUs from DRM sysfs. NVIDIA cards are skipped: `nvidia-smi` reads them.
pub fn gpus_from_drm(cards: &[DrmCard]) -> Vec<Gpu> {
    const MIB: u64 = 1_024 * 1_024;
    cards
        .iter()
        .filter_map(|c| {
            let vendor = GpuVendor::from_pci_id(&c.vendor);
            let (name, vram_mb, integrated) = match vendor {
                GpuVendor::Amd => {
                    let mb = c.vram_total? / MIB;
                    let apu = mb < APU_CARVEOUT_MAX_MB;
                    (
                        if apu {
                            "AMD Radeon iGPU"
                        } else {
                            "AMD Radeon GPU"
                        },
                        mb,
                        apu,
                    )
                }
                GpuVendor::Intel => match c.lmem_total.filter(|b| *b > 0) {
                    Some(b) => ("Intel Arc GPU", b / MIB, false),
                    None => ("Intel iGPU", 0, true),
                },
                _ => return None,
            };
            Some(Gpu {
                vendor,
                name: name.into(),
                vram_mb,
                free_vram_mb: c
                    .vram_total
                    .zip(c.vram_used)
                    .map(|(t, u)| t.saturating_sub(u) / MIB)
                    .filter(|_| !integrated),
                integrated,
                unified: false,
                driver: c.driver.clone(),
            })
        })
        .collect()
}

fn looks_integrated(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    (n.contains("intel") && !n.contains("arc"))
        || n.contains("radeon(tm) graphics")
        || n.contains("radeon graphics")
        || n.contains("microsoft basic")
}

/// Windows: `Name,Bytes` CSV of each display adapter's 64-bit
/// `HardwareInformation.qwMemorySize` (the registry value DXGI reports as
/// dedicated memory; WMI's `AdapterRAM` stops at 4 GB).
pub fn parse_windows_gpus(csv: &str) -> Vec<Gpu> {
    const MIB: u64 = 1_024 * 1_024;
    csv.lines()
        .filter_map(|l| {
            let l = l.trim();
            if l.is_empty() || l.to_ascii_lowercase().starts_with("\"name\"") {
                return None;
            }
            let (name, bytes) = l.rsplit_once(',')?;
            let name = name.trim().trim_matches('"').trim();
            let bytes: u64 = bytes.trim().trim_matches('"').parse().ok()?;
            if name.is_empty() || name.to_ascii_lowercase().contains("microsoft basic") {
                return None;
            }
            let vram_mb = bytes / MIB;
            let integrated = looks_integrated(name) || vram_mb < 1_024;
            Some(Gpu {
                vendor: GpuVendor::from_name(name),
                name: name.into(),
                vram_mb: if integrated { 0 } else { vram_mb },
                free_vram_mb: None,
                integrated,
                unified: false,
                driver: String::new(),
            })
        })
        .collect()
}

/// `vulkaninfo --summary`: each device's name, vendor and type. Vulkan
/// doesn't report VRAM here, so discrete cards read 0 and the RAM budget
/// decides until a better reader fills it in.
pub fn parse_vulkan_summary(text: &str) -> Vec<Gpu> {
    let mut out = Vec::new();
    let mut cur: Option<Gpu> = None;
    for line in text.lines() {
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        let (k, v) = (k.trim(), v.trim());
        match k {
            "vendorID" => {
                out.extend(cur.take());
                cur = Some(Gpu {
                    vendor: GpuVendor::from_pci_id(v),
                    name: String::new(),
                    vram_mb: 0,
                    free_vram_mb: None,
                    integrated: false,
                    unified: false,
                    driver: String::new(),
                });
            }
            "deviceName" => {
                if let Some(g) = cur.as_mut() {
                    g.name = v.into();
                }
            }
            "deviceType" => {
                if let Some(g) = cur.as_mut() {
                    g.integrated = v.contains("INTEGRATED");
                    if v.contains("CPU") {
                        // llvmpipe and other software renderers are not GPUs.
                        cur = None;
                    }
                }
            }
            "driverInfo" => {
                if let Some(g) = cur.as_mut() {
                    g.driver = v.into();
                }
            }
            _ => {}
        }
    }
    out.extend(cur);
    out.into_iter().filter(|g| !g.name.is_empty()).collect()
}

/// A `/proc/meminfo` field (`MemAvailable:`) in MiB.
pub fn parse_meminfo_mb(text: &str, key: &str) -> Option<u64> {
    let line = text.lines().find(|l| l.starts_with(key))?;
    let kb: u64 = line.split_whitespace().nth(1)?.parse().ok()?;
    Some(kb / 1_024)
}

/// The CPU's SIMD from `/proc/cpuinfo` (`flags` on x86, `Features` on ARM).
pub fn parse_cpuinfo_simd(text: &str) -> CpuSimd {
    let words = |key: &str| -> Option<Vec<String>> {
        let line = text
            .lines()
            .find(|l| l.split(':').next().map(str::trim) == Some(key))?;
        Some(
            line.split_once(':')?
                .1
                .split_whitespace()
                .map(str::to_string)
                .collect(),
        )
    };
    if let Some(f) = words("flags") {
        return if f.iter().any(|w| w == "avx2") {
            CpuSimd::Avx2
        } else if f.iter().any(|w| w == "avx") {
            CpuSimd::AvxOnly
        } else {
            CpuSimd::NoAvx
        };
    }
    match words("Features") {
        Some(f) if f.iter().any(|w| w == "asimd" || w == "neon") => CpuSimd::Neon,
        _ => CpuSimd::detect(),
    }
}

/// A sysfs CPU list (`0-7,16-23`) → how many CPUs it names.
pub fn parse_cpu_list(text: &str) -> u32 {
    text.trim()
        .split(',')
        .filter(|p| !p.is_empty())
        .map(|p| match p.split_once('-') {
            Some((a, b)) => match (a.trim().parse::<u32>(), b.trim().parse::<u32>()) {
                (Ok(a), Ok(b)) if b >= a => b - a + 1,
                _ => 0,
            },
            None => u32::from(p.trim().parse::<u32>().is_ok()),
        })
        .sum()
}

/// One `/sys/class/power_supply/*`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PowerSupply {
    /// `type`: `Mains`, `Battery`, `USB`.
    pub kind: String,
    pub online: Option<bool>,
    pub capacity: Option<u8>,
    /// `status`: `Charging`, `Discharging`, `Full`.
    pub status: Option<String>,
}

/// Linux power from `power_supply` plus `/sys/firmware/acpi/platform_profile`.
pub fn power_from_supplies(supplies: &[PowerSupply], platform_profile: Option<&str>) -> PowerInfo {
    let batteries: Vec<&PowerSupply> = supplies.iter().filter(|s| s.kind == "Battery").collect();
    let mains_online = supplies
        .iter()
        .filter(|s| s.kind != "Battery")
        .any(|s| s.online == Some(true));
    let discharging = batteries
        .iter()
        .any(|b| b.status.as_deref() == Some("Discharging"));
    PowerInfo {
        battery: !batteries.is_empty(),
        on_ac: batteries.is_empty() || mains_online || !discharging,
        percent: batteries.iter().filter_map(|b| b.capacity).min(),
        power_saver: platform_profile.map(str::trim) == Some("low-power"),
    }
}

/// Windows `Win32_Battery` CSV (`BatteryStatus,EstimatedChargeRemaining`).
/// No rows is a desktop. Status 1 is discharging; 2 and up are on AC.
pub fn parse_windows_battery(csv: &str) -> PowerInfo {
    let row = csv
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty() && !l.to_ascii_lowercase().starts_with("\"batterystatus\""));
    let Some(row) = row else {
        return PowerInfo::default();
    };
    let mut cols = row.split(',').map(|c| c.trim().trim_matches('"'));
    let status: Option<u32> = cols.next().and_then(|s| s.parse().ok());
    let percent: Option<u8> = cols.next().and_then(|s| s.parse().ok());
    PowerInfo {
        battery: true,
        on_ac: status != Some(1),
        percent,
        power_saver: false,
    }
}

/// Linux `thermal_zone*` readings (`type`, millidegrees C) → the hottest zone's state.
pub fn thermal_from_zones(zones: &[(String, i64)]) -> Thermal {
    let hottest = zones
        .iter()
        .map(|(_, milli)| *milli)
        .filter(|m| (1..150_000).contains(m))
        .max();
    match hottest {
        None => Thermal::Unknown,
        Some(m) if m >= 95_000 => Thermal::Critical,
        Some(m) if m >= 85_000 => Thermal::Serious,
        Some(m) if m >= 75_000 => Thermal::Fair,
        Some(_) => Thermal::Nominal,
    }
}

#[cfg(test)]
#[path = "local_profile_tests.rs"]
mod tests;
