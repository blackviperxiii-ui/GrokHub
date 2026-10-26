//! This machine's readers for the hardware profile (`local_profile`): GPUs
//! from `nvidia-smi`, then Linux DRM sysfs or the Windows display-adapter
//! registry, then `vulkaninfo`; CPU cores and SIMD; RAM; battery; thermal
//! zones. Anything a reader can't see falls back to the profile's defaults.
//! macOS reads Apple Silicon as unified memory at 70% of RAM until a Metal
//! reader lands.

use std::path::{Path, PathBuf};

use grokhub_core::local_picker::CpuSimd;
use grokhub_core::local_profile::{
    self as lp, CpuInfo, CpuProbe, Gpu, GpuProbe, MemoryProbe, PowerInfo, PowerProbe, Thermal,
    ThermalProbe,
};

use super::setup_wizard::{free_disk_mb, ram_mb, run};

struct NvidiaSmi;

impl GpuProbe for NvidiaSmi {
    fn gpus(&self) -> Vec<Gpu> {
        run(
            "nvidia-smi",
            &[
                "--query-gpu=name,memory.total,memory.free,driver_version",
                "--format=csv,noheader,nounits",
            ],
        )
        .map(|s| lp::parse_nvidia_smi_all(&s))
        .unwrap_or_default()
    }
}

/// Linux DRM sysfs (AMD, Intel); Windows display-adapter registry.
struct Platform;

#[cfg(target_os = "linux")]
fn read(path: PathBuf) -> Option<String> {
    std::fs::read_to_string(path).ok()
}

#[cfg(target_os = "linux")]
fn read_u64(path: PathBuf) -> Option<u64> {
    read(path)?.trim().parse().ok()
}

impl GpuProbe for Platform {
    #[cfg(target_os = "linux")]
    fn gpus(&self) -> Vec<Gpu> {
        let Ok(entries) = std::fs::read_dir("/sys/class/drm") else {
            return Vec::new();
        };
        let mut cards: Vec<(String, lp::DrmCard)> = entries
            .flatten()
            .filter_map(|e| {
                let name = e.file_name().to_string_lossy().into_owned();
                // card0, card1, … — not card0-HDMI-A-1 connectors.
                if !name.starts_with("card") || name.contains('-') {
                    return None;
                }
                let dev = e.path().join("device");
                let driver = read(dev.join("uevent"))
                    .and_then(|u| {
                        u.lines()
                            .find_map(|l| l.strip_prefix("DRIVER=").map(|d| d.trim().to_string()))
                    })
                    .unwrap_or_default();
                Some((
                    name,
                    lp::DrmCard {
                        vendor: read(dev.join("vendor")).unwrap_or_default(),
                        driver,
                        vram_total: read_u64(dev.join("mem_info_vram_total")),
                        vram_used: read_u64(dev.join("mem_info_vram_used")),
                        lmem_total: read_u64(dev.join("lmem_total_bytes"))
                            .or_else(|| read_u64(dev.join("tile0/vram0/size"))),
                    },
                ))
            })
            .collect();
        cards.sort_by(|a, b| a.0.cmp(&b.0));
        let cards: Vec<lp::DrmCard> = cards.into_iter().map(|(_, c)| c).collect();
        lp::gpus_from_drm(&cards)
    }

    #[cfg(windows)]
    fn gpus(&self) -> Vec<Gpu> {
        // HardwareInformation.qwMemorySize is 64-bit; WMI's AdapterRAM stops at 4 GB.
        const SCRIPT: &str = "Get-ItemProperty 'HKLM:\\SYSTEM\\CurrentControlSet\\Control\\Class\\{4d36e968-e325-11ce-bfc1-08002be10318}\\0*' -ErrorAction SilentlyContinue | Where-Object { $_.DriverDesc } | ForEach-Object { [pscustomobject]@{ Name = $_.DriverDesc; Bytes = [uint64]$_.'HardwareInformation.qwMemorySize' } } | ConvertTo-Csv -NoTypeInformation";
        run("powershell", &["-NoProfile", "-Command", SCRIPT])
            .map(|s| lp::parse_windows_gpus(&s))
            .unwrap_or_default()
    }

    #[cfg(target_os = "macos")]
    fn gpus(&self) -> Vec<Gpu> {
        if cfg!(target_arch = "aarch64") {
            let ram = ram_mb().unwrap_or(0);
            return vec![Gpu {
                vendor: lp::GpuVendor::Apple,
                name: "Apple GPU".into(),
                vram_mb: ram * grokhub_core::local_picker::UNIFIED_PERCENT / 100,
                free_vram_mb: None,
                integrated: false,
                unified: true,
                driver: String::new(),
            }];
        }
        Vec::new()
    }

    #[cfg(not(any(target_os = "linux", windows, target_os = "macos")))]
    fn gpus(&self) -> Vec<Gpu> {
        Vec::new()
    }
}

struct Vulkan;

impl GpuProbe for Vulkan {
    fn gpus(&self) -> Vec<Gpu> {
        run("vulkaninfo", &["--summary"])
            .map(|s| lp::parse_vulkan_summary(&s))
            .unwrap_or_default()
    }
}

/// CPU, memory, power and thermal for this OS; `home` is where models go.
struct System {
    home: PathBuf,
}

impl CpuProbe for System {
    fn cpu(&self) -> CpuInfo {
        let logical = std::thread::available_parallelism()
            .map(|n| n.get() as u32)
            .unwrap_or(2);
        let physical = grokhub_agent::route::local_perf::physical_cores().unwrap_or(logical);
        CpuInfo::new(logical, physical, e_cores(), simd())
    }
}

#[cfg(target_os = "linux")]
fn simd() -> CpuSimd {
    read(PathBuf::from("/proc/cpuinfo"))
        .map(|s| lp::parse_cpuinfo_simd(&s))
        .unwrap_or_else(CpuSimd::detect)
}

#[cfg(not(target_os = "linux"))]
fn simd() -> CpuSimd {
    CpuSimd::detect()
}

/// Intel hybrid lists its E-cores under `cpu_atom`; Apple under perflevel1.
#[cfg(target_os = "linux")]
fn e_cores() -> u32 {
    read(PathBuf::from("/sys/devices/cpu_atom/cpus"))
        .map(|s| lp::parse_cpu_list(&s))
        .unwrap_or(0)
}

#[cfg(target_os = "macos")]
fn e_cores() -> u32 {
    run("sysctl", &["-n", "hw.perflevel1.physicalcpu"])
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0)
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn e_cores() -> u32 {
    0
}

impl MemoryProbe for System {
    fn ram_mb(&self) -> Option<u64> {
        ram_mb()
    }

    fn free_ram_mb(&self) -> Option<u64> {
        free_ram_mb()
    }

    fn free_disk_mb(&self) -> Option<u64> {
        free_disk_mb(&self.home)
    }
}

#[cfg(target_os = "linux")]
fn free_ram_mb() -> Option<u64> {
    lp::parse_meminfo_mb(&read(PathBuf::from("/proc/meminfo"))?, "MemAvailable:")
}

#[cfg(windows)]
fn free_ram_mb() -> Option<u64> {
    use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
    // SAFETY: a zeroed MEMORYSTATUSEX with dwLength set is what the call expects.
    let mut m: MEMORYSTATUSEX = unsafe { std::mem::zeroed() };
    m.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
    (unsafe { GlobalMemoryStatusEx(&mut m) } != 0).then_some(m.ullAvailPhys / (1_024 * 1_024))
}

#[cfg(not(any(target_os = "linux", windows)))]
fn free_ram_mb() -> Option<u64> {
    None
}

impl PowerProbe for System {
    #[cfg(target_os = "linux")]
    fn power(&self) -> Option<PowerInfo> {
        let entries = std::fs::read_dir("/sys/class/power_supply").ok()?;
        let supplies: Vec<lp::PowerSupply> = entries
            .flatten()
            .map(|e| {
                let d = e.path();
                lp::PowerSupply {
                    kind: read(d.join("type")).unwrap_or_default().trim().to_string(),
                    online: read_u64(d.join("online")).map(|v| v == 1),
                    capacity: read_u64(d.join("capacity")).and_then(|v| u8::try_from(v).ok()),
                    status: read(d.join("status")).map(|s| s.trim().to_string()),
                }
            })
            .collect();
        let profile = read(PathBuf::from("/sys/firmware/acpi/platform_profile"));
        Some(lp::power_from_supplies(&supplies, profile.as_deref()))
    }

    #[cfg(windows)]
    fn power(&self) -> Option<PowerInfo> {
        run(
            "powershell",
            &[
                "-NoProfile",
                "-Command",
                "Get-CimInstance Win32_Battery | Select-Object BatteryStatus,EstimatedChargeRemaining | ConvertTo-Csv -NoTypeInformation",
            ],
        )
        .map(|s| lp::parse_windows_battery(&s))
    }

    #[cfg(not(any(target_os = "linux", windows)))]
    fn power(&self) -> Option<PowerInfo> {
        None
    }
}

impl ThermalProbe for System {
    #[cfg(target_os = "linux")]
    fn thermal(&self) -> Thermal {
        let Ok(entries) = std::fs::read_dir("/sys/class/thermal") else {
            return Thermal::Unknown;
        };
        let zones: Vec<(String, i64)> = entries
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().starts_with("thermal_zone"))
            .filter_map(|e| {
                let kind = read(e.path().join("type")).unwrap_or_default();
                let temp = read(e.path().join("temp"))?.trim().parse().ok()?;
                Some((kind.trim().to_string(), temp))
            })
            .collect();
        lp::thermal_from_zones(&zones)
    }

    #[cfg(not(target_os = "linux"))]
    fn thermal(&self) -> Thermal {
        Thermal::Unknown
    }
}

/// Power, thermal and free RAM for the governor's sampler.
#[cfg(target_os = "linux")]
pub(super) fn power_now() -> Option<PowerInfo> {
    System {
        home: PathBuf::new(),
    }
    .power()
}

#[cfg(target_os = "linux")]
pub(super) fn thermal_now() -> Thermal {
    System {
        home: PathBuf::new(),
    }
    .thermal()
}

#[cfg(any(target_os = "linux", windows))]
pub(super) fn free_ram_now() -> Option<u64> {
    free_ram_mb()
}

/// Read this machine. Slow (it runs `nvidia-smi` and friends): call it off the UI thread.
pub(super) fn read_profile(home: &Path) -> lp::LocalProfile {
    let system = System {
        home: home.to_path_buf(),
    };
    lp::profile(&lp::Probes {
        gpu: &[&NvidiaSmi, &Platform, &Vulkan],
        cpu: &system,
        memory: &system,
        power: &system,
        thermal: &system,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn this_machine_reads_without_panicking() {
        let p = read_profile(&std::env::temp_dir());
        assert!(p.cpu.logical >= 1 && p.cpu.p_cores >= 1, "{p:?}");
        if cfg!(any(target_os = "linux", windows)) {
            assert!(p.ram_mb > 0 && p.free_disk_mb > 0, "{p:?}");
        }
        assert!(p.cpu.logical >= p.cpu.physical);
    }
}
