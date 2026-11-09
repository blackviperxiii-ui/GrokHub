//! This machine's readers, shared by the native cabin and the desktop app.
//!
//! - The hardware profile for local models (`read_profile`): GPUs from
//!   `nvidia-smi`, then Linux DRM sysfs or the Windows display-adapter
//!   registry, then `vulkaninfo`; CPU cores and SIMD; RAM; battery; thermal
//!   zones. Anything a reader can't see falls back to the profile's defaults.
//!   macOS reads Apple Silicon as unified memory at 70% of RAM until a Metal
//!   reader lands.
//! - The auto setup's device record (`read_device`, `look`): what the OS says
//!   about text size, motion, contrast and dark mode, the clock, the model
//!   name, Steam, git folders and a microphone.
//! - `start`: read once per launch off the UI thread, cache to
//!   `{config}/device.json`, hand the profile to the local-model launcher and
//!   the router's tier, then re-read power and the OS look every few minutes.
//!
//! Every reader is slow (it spawns processes): never call one on a UI thread.

mod live;
mod look;

pub use live::{apply_to_router, current, generation, set_display, start};
pub use look::read_device;

use std::path::{Path, PathBuf};

use grokhub_core::local_picker::CpuSimd;
use grokhub_core::local_profile::{
    self as lp, CpuInfo, CpuProbe, Gpu, GpuProbe, MemoryProbe, PowerInfo, PowerProbe, Thermal,
    ThermalProbe,
};

/// Run a program and return what it printed, or `None` when it's missing or
/// failed. No console window on Windows.
pub fn run(cmd: &str, args: &[&str]) -> Option<String> {
    let mut c = std::process::Command::new(cmd);
    c.args(args)
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        c.creation_flags(CREATE_NO_WINDOW);
    }
    let out = c.output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

#[cfg(target_os = "linux")]
pub fn ram_mb() -> Option<u64> {
    grokhub_core::local_setup::parse_meminfo(&std::fs::read_to_string("/proc/meminfo").ok()?)
}

#[cfg(target_os = "macos")]
pub fn ram_mb() -> Option<u64> {
    grokhub_core::local_setup::parse_memsize_bytes(&run("sysctl", &["-n", "hw.memsize"])?)
}

#[cfg(windows)]
pub fn ram_mb() -> Option<u64> {
    use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
    // SAFETY: a zeroed MEMORYSTATUSEX with dwLength set is what the call expects.
    let mut m: MEMORYSTATUSEX = unsafe { std::mem::zeroed() };
    m.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
    (unsafe { GlobalMemoryStatusEx(&mut m) } != 0).then_some(m.ullTotalPhys / (1_024 * 1_024))
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
pub fn ram_mb() -> Option<u64> {
    None
}

#[cfg(unix)]
pub fn free_disk_mb(dir: &Path) -> Option<u64> {
    grokhub_core::local_setup::parse_df_avail_mb(&run("df", &["-Pk", &dir.to_string_lossy()])?)
}

#[cfg(windows)]
pub fn free_disk_mb(dir: &Path) -> Option<u64> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
    let wide: Vec<u16> = dir.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut free: u64 = 0;
    // SAFETY: a NUL-terminated path and one out pointer; the other two may be null.
    let ok = unsafe {
        GetDiskFreeSpaceExW(
            wide.as_ptr(),
            &mut free,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    (ok != 0).then_some(free / (1_024 * 1_024))
}

#[cfg(not(any(unix, windows)))]
pub fn free_disk_mb(_dir: &Path) -> Option<u64> {
    None
}

struct NvidiaSmi;

/// Free VRAM across NVIDIA GPUs right now (MB), from `nvidia-smi`. `None`
/// without it. Cheap enough for the governor's slow tick and a launch.
pub fn nvidia_free_vram_now() -> Option<u64> {
    let text = run(
        "nvidia-smi",
        &["--query-gpu=memory.free", "--format=csv,noheader,nounits"],
    )?;
    let frees: Vec<u64> = text
        .lines()
        .filter_map(|l| l.trim().parse::<u64>().ok())
        .collect();
    (!frees.is_empty()).then(|| frees.iter().sum())
}

/// Free VRAM right now (MB): `nvidia-smi`, else (Linux) the DRM sysfs of AMD
/// and other cards with their own memory. `None` without either.
pub fn free_vram_now() -> Option<u64> {
    nvidia_free_vram_now().or_else(drm_free_vram_now)
}

/// Discrete cards carry at least this much VRAM; an APU's carve-out is less.
const DISCRETE_VRAM_BYTES: u64 = 2 * 1_024 * 1_024 * 1_024;

#[cfg(target_os = "linux")]
fn drm_free_vram_now() -> Option<u64> {
    let entries = std::fs::read_dir("/sys/class/drm").ok()?;
    let cards: Vec<(u64, u64)> = entries
        .flatten()
        .filter(|e| {
            let n = e.file_name().to_string_lossy().into_owned();
            n.starts_with("card") && !n.contains('-')
        })
        .filter_map(|e| {
            let dev = e.path().join("device");
            Some((
                read_u64(dev.join("mem_info_vram_total"))?,
                read_u64(dev.join("mem_info_vram_used"))?,
            ))
        })
        .collect();
    drm_free_mb(&cards)
}

#[cfg(not(target_os = "linux"))]
fn drm_free_vram_now() -> Option<u64> {
    None
}

/// Free MB across `(total, used)` byte pairs of discrete cards.
pub fn drm_free_mb(cards: &[(u64, u64)]) -> Option<u64> {
    let free: Vec<u64> = cards
        .iter()
        .filter(|(total, _)| *total >= DISCRETE_VRAM_BYTES)
        .map(|(total, used)| total.saturating_sub(*used) / (1_024 * 1_024))
        .collect();
    (!free.is_empty()).then(|| free.iter().sum())
}

/// Idle and total CPU time from `/proc/stat`'s first line.
pub fn parse_proc_stat(text: &str) -> Option<(u64, u64)> {
    let line = text.lines().next()?.strip_prefix("cpu ")?;
    let v: Vec<u64> = line
        .split_whitespace()
        .filter_map(|x| x.parse().ok())
        .collect();
    if v.len() < 4 {
        return None;
    }
    // user nice system idle iowait irq softirq steal: idle + iowait is idle.
    let idle = v[3] + v.get(4).copied().unwrap_or(0);
    let total = v.iter().take(8).sum();
    Some((idle, total))
}

/// Share of the CPU in use between two `(idle, total)` samples.
pub fn busy_pct(prev: (u64, u64), now: (u64, u64)) -> Option<u8> {
    let total = now.1.checked_sub(prev.1).filter(|t| *t > 0)?;
    let idle = now.0.saturating_sub(prev.0).min(total);
    u8::try_from((total - idle) * 100 / total).ok()
}

static CPU_LAST: std::sync::Mutex<Option<(u64, u64)>> = std::sync::Mutex::new(None);

/// Share of the CPU in use since the last call; `None` on the first call and
/// where the OS can't say.
pub fn cpu_busy_pct() -> Option<u8> {
    let now = cpu_times()?;
    let mut last = CPU_LAST.lock().ok()?;
    let prev = last.replace(now)?;
    busy_pct(prev, now)
}

#[cfg(target_os = "linux")]
fn cpu_times() -> Option<(u64, u64)> {
    parse_proc_stat(&std::fs::read_to_string("/proc/stat").ok()?)
}

#[cfg(windows)]
fn cpu_times() -> Option<(u64, u64)> {
    use windows_sys::Win32::Foundation::FILETIME;
    use windows_sys::Win32::System::Threading::GetSystemTimes;
    let z = || FILETIME {
        dwLowDateTime: 0,
        dwHighDateTime: 0,
    };
    let (mut idle, mut kernel, mut user) = (z(), z(), z());
    // SAFETY: three out-params the call fills in.
    if unsafe { GetSystemTimes(&mut idle, &mut kernel, &mut user) } == 0 {
        return None;
    }
    let v = |f: FILETIME| (u64::from(f.dwHighDateTime) << 32) | u64::from(f.dwLowDateTime);
    // Kernel time includes idle time.
    Some((v(idle), v(kernel) + v(user)))
}

#[cfg(not(any(target_os = "linux", windows)))]
fn cpu_times() -> Option<(u64, u64)> {
    None
}

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

/// `card10` -> 10, so cards sort the way the kernel numbered them.
#[cfg(any(target_os = "linux", test))]
fn card_number(name: &str) -> u32 {
    name.trim_start_matches("card").parse().unwrap_or(u32::MAX)
}

/// The PCI id database pciutils / hwdata install, for model names.
#[cfg(target_os = "linux")]
fn pci_ids() -> Option<String> {
    [
        "/usr/share/hwdata/pci.ids",
        "/usr/share/misc/pci.ids",
        "/usr/share/pci.ids",
    ]
    .iter()
    .find_map(|p| std::fs::read_to_string(p).ok())
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
                        device: read(dev.join("device")).unwrap_or_default().trim().into(),
                        pci_name: None,
                    },
                ))
            })
            .collect();
        // card2 before card10.
        cards.sort_by_key(|(name, _)| card_number(name));
        let ids = pci_ids();
        let cards: Vec<lp::DrmCard> = cards
            .into_iter()
            .map(|(_, mut c)| {
                c.vendor = c.vendor.trim().into();
                c.pci_name = ids
                    .as_deref()
                    .and_then(|t| lp::pci_ids_name(t, &c.vendor, &c.device));
                c
            })
            .collect();
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
        let physical = crate::route::local_perf::physical_cores().unwrap_or(logical);
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
pub fn power_now() -> Option<PowerInfo> {
    System {
        home: PathBuf::new(),
    }
    .power()
}

#[cfg(target_os = "linux")]
pub fn thermal_now() -> Thermal {
    System {
        home: PathBuf::new(),
    }
    .thermal()
}

#[cfg(not(any(target_os = "linux", windows)))]
pub fn free_ram_now() -> Option<u64> {
    None
}

#[cfg(any(target_os = "linux", windows))]
pub fn free_ram_now() -> Option<u64> {
    free_ram_mb()
}

/// Power right now, cheaply: `GetSystemPowerStatus` on Windows, sysfs on
/// Linux. `None` elsewhere (the launch read stands).
#[cfg(target_os = "linux")]
pub fn power_quick() -> Option<PowerInfo> {
    power_now()
}

#[cfg(windows)]
pub fn power_quick() -> Option<PowerInfo> {
    use windows_sys::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
    // SAFETY: a zeroed out struct the call fills in.
    let mut s: SYSTEM_POWER_STATUS = unsafe { std::mem::zeroed() };
    if unsafe { GetSystemPowerStatus(&mut s) } == 0 {
        return None;
    }
    Some(windows_power(
        s.ACLineStatus,
        s.BatteryFlag,
        s.BatteryLifePercent,
        s.SystemStatusFlag,
    ))
}

#[cfg(not(any(target_os = "linux", windows)))]
pub fn power_quick() -> Option<PowerInfo> {
    None
}

/// `SYSTEM_POWER_STATUS` fields: AC 1 = plugged in (255 unknown), battery
/// flag 128 = no battery (255 unknown), percent 255 = unknown, status flag
/// 1 = battery saver on.
#[cfg(any(windows, test))]
fn windows_power(ac: u8, battery_flag: u8, percent: u8, saver: u8) -> PowerInfo {
    let battery = battery_flag != 128 && battery_flag != 255;
    PowerInfo {
        battery,
        on_ac: !battery || ac != 0,
        percent: (battery && percent <= 100).then_some(percent),
        power_saver: saver == 1,
    }
}

/// Read this machine. Slow (it runs `nvidia-smi` and friends): call it off the UI thread.
pub fn read_profile(home: &Path) -> lp::LocalProfile {
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

/// The main GPU's temperature in °C, read from the OS: `nvidia-smi`, else
/// the hwmon `temp1_input` of the DRM card with the most VRAM (Linux). Never
/// touches a game process. `None` when nothing reads (Windows without
/// `nvidia-smi`).
pub fn gpu_temp_c() -> Option<i32> {
    run(
        "nvidia-smi",
        &[
            "--query-gpu=temperature.gpu",
            "--format=csv,noheader,nounits",
        ],
    )
    .and_then(|s| first_int(&s))
    .or_else(drm_gpu_temp_c)
}

/// The first whole number in a reader's output (`61`, `61\n58`).
fn first_int(text: &str) -> Option<i32> {
    text.lines().next()?.trim().parse().ok()
}

#[cfg(target_os = "linux")]
fn drm_gpu_temp_c() -> Option<i32> {
    let entries = std::fs::read_dir("/sys/class/drm").ok()?;
    entries
        .flatten()
        .filter(|e| {
            let n = e.file_name();
            let n = n.to_string_lossy();
            n.starts_with("card") && !n.contains('-')
        })
        .filter_map(|e| {
            let dev = e.path().join("device");
            let vram = read_u64(dev.join("mem_info_vram_total"))
                .or_else(|| read_u64(dev.join("lmem_total_bytes")))
                .unwrap_or(0);
            let milli = std::fs::read_dir(dev.join("hwmon"))
                .ok()?
                .flatten()
                .find_map(|h| read_u64(h.path().join("temp1_input")))?;
            Some((vram, (milli / 1_000) as i32))
        })
        .max_by_key(|(vram, _)| *vram)
        .map(|(_, c)| c)
}

#[cfg(not(target_os = "linux"))]
fn drm_gpu_temp_c() -> Option<i32> {
    None
}

/// The CPU's temperature in °C from the Linux hwmon sensors (k10temp,
/// coretemp, zenpower and the like). `None` elsewhere: Windows has no
/// reading a normal user can take without a vendor driver.
pub fn cpu_temp_c() -> Option<i32> {
    #[cfg(target_os = "linux")]
    {
        let rows = hwmon_temps(Path::new("/sys/class/hwmon"));
        grokhub_core::pc_health::temps_from_hwmon(&rows)
            .0
            .map(|c| c.round() as i32)
    }
    #[cfg(not(target_os = "linux"))]
    {
        None
    }
}

/// Every hwmon chip's name and its first few `tempN_input` readings, in
/// millidegrees.
#[cfg(target_os = "linux")]
pub fn hwmon_temps(root: &Path) -> Vec<(String, i64)> {
    let Ok(entries) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for ent in entries.flatten().take(24) {
        let name = read(ent.path().join("name")).unwrap_or_default();
        let name = name.trim();
        if name.is_empty() {
            continue;
        }
        for i in 1..=5 {
            if let Some(milli) = read(ent.path().join(format!("temp{i}_input")))
                .and_then(|t| t.trim().parse::<i64>().ok())
            {
                out.push((name.to_string(), milli));
            }
        }
    }
    out
}

/// The apps that start with this PC, by name (`grokhub_core::startup_apps`).
/// Linux: the autostart folder. Windows: the `Run` keys and Startup folders,
/// minus what Task Manager turned off. Slow on Windows (`reg query`).
pub fn startup_apps(home: &Path) -> Option<Vec<String>> {
    #[cfg(target_os = "linux")]
    {
        use grokhub_core::startup_apps as sa;
        let dir = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .unwrap_or_else(|| home.join(".config"))
            .join("autostart");
        let Ok(entries) = std::fs::read_dir(&dir) else {
            // No folder: nothing starts from it.
            return Some(Vec::new());
        };
        let mut names: Vec<String> = entries
            .flatten()
            .take(sa::MAX_FILES)
            .filter(|e| e.path().extension().is_some_and(|x| x == "desktop"))
            .filter_map(|e| {
                let stem = e.path().file_stem()?.to_string_lossy().into_owned();
                sa::desktop_autostart(&stem, &read(e.path())?)
            })
            .collect();
        names.sort();
        names.dedup();
        Some(names)
    }
    #[cfg(windows)]
    {
        use grokhub_core::startup_apps as sa;
        let _ = home;
        const APPROVED: &str =
            r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved";
        const RUN: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
        let query = |key: String| run("reg", &["query", &key]).unwrap_or_default();
        let mut names = Vec::new();
        for hive in ["HKCU", "HKLM"] {
            names.extend(sa::run_key_apps(
                &query(format!(r"{hive}\{RUN}")),
                &query(format!(r"{hive}\{APPROVED}\Run")),
            ));
        }
        // 32-bit apps on 64-bit Windows; Task Manager keeps their switch as Run32.
        names.extend(sa::run_key_apps(
            &query(r"HKLM\Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Run".into()),
            &query(format!(r"HKLM\{APPROVED}\Run32")),
        ));
        let folders = [
            (std::env::var_os("APPDATA"), "HKCU"),
            (std::env::var_os("ProgramData"), "HKLM"),
        ];
        for (base, hive) in folders {
            let Some(base) = base else { continue };
            let dir = PathBuf::from(base).join(r"Microsoft\Windows\Start Menu\Programs\Startup");
            let files: Vec<String> = std::fs::read_dir(dir)
                .map(|it| {
                    it.flatten()
                        .take(sa::MAX_FILES)
                        .map(|e| e.file_name().to_string_lossy().into_owned())
                        .collect()
                })
                .unwrap_or_default();
            names.extend(sa::startup_folder_apps(
                &files,
                &query(format!(r"{hive}\{APPROVED}\StartupFolder")),
            ));
        }
        Some(names)
    }
    #[cfg(not(any(target_os = "linux", windows)))]
    {
        let _ = home;
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drm_free_vram_skips_small_carve_outs() {
        let gib = 1_024 * 1_024 * 1_024;
        // A 16 GiB card with 4 GiB used, and an APU's 512 MiB carve-out.
        assert_eq!(
            drm_free_mb(&[(16 * gib, 4 * gib), (gib / 2, 0)]),
            Some(12_288)
        );
        assert_eq!(drm_free_mb(&[(gib / 2, 0)]), None);
        assert_eq!(drm_free_mb(&[]), None);
    }

    #[test]
    fn cpu_busy_from_two_proc_stat_samples() {
        let a = parse_proc_stat("cpu  100 0 100 700 100 0 0 0 0 0\ncpu0 1 2 3 4\n").unwrap();
        assert_eq!(a, (800, 1_000));
        let b = parse_proc_stat("cpu  300 0 200 1300 200 0 0 0 0 0\n").unwrap();
        // 1000 more ticks, 700 of them idle: 30% busy.
        assert_eq!(busy_pct(a, b), Some(30));
        assert_eq!(busy_pct(a, a), None);
        assert_eq!(parse_proc_stat("intr 1 2 3"), None);
    }

    #[test]
    fn this_machine_reads_without_panicking() {
        let p = read_profile(&std::env::temp_dir());
        assert!(p.cpu.logical >= 1 && p.cpu.p_cores >= 1, "{p:?}");
        if cfg!(any(target_os = "linux", windows)) {
            assert!(p.ram_mb > 0 && p.free_disk_mb > 0, "{p:?}");
        }
        assert!(p.cpu.logical >= p.cpu.physical);
    }

    #[test]
    fn windows_power_status_fields() {
        let desktop = windows_power(1, 128, 255, 0);
        assert_eq!(
            (
                desktop.battery,
                desktop.on_ac,
                desktop.percent,
                desktop.power_saver
            ),
            (false, true, None, false)
        );
        let unplugged = windows_power(0, 1, 62, 1);
        assert_eq!(
            (
                unplugged.battery,
                unplugged.on_ac,
                unplugged.percent,
                unplugged.power_saver
            ),
            (true, false, Some(62), true)
        );
        let charging = windows_power(1, 8, 40, 0);
        assert_eq!((charging.on_ac, charging.percent), (true, Some(40)));
    }

    #[test]
    fn temperature_reads_the_first_number() {
        assert_eq!(first_int("61\n58\n"), Some(61));
        assert_eq!(first_int("[N/A]"), None);
        assert_eq!(first_int(""), None);
    }

    #[test]
    fn cards_sort_by_number_not_by_text() {
        let mut names = vec!["card10", "card2", "card0"];
        names.sort_by_key(|n| card_number(n));
        assert_eq!(names, ["card0", "card2", "card10"]);
    }
}
