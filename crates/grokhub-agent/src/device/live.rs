//! One device read per launch, shared by the whole process, then a light
//! re-read of power and the OS look every few minutes.
//!
//! `start` returns at once: the last `device.json` is live straight away (so
//! a launch never waits on `nvidia-smi`), and a thread reads the machine,
//! hands the full profile to the local-model launcher, sets the router's
//! tier from the battery and the hardware, and saves the new record.
//! `generation` goes up whenever the record changes, so the auto setup knows
//! to look again.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use grokhub_core::autotune::device::{self as dev, DeviceProfile, Display};
use grokhub_core::local_profile::{LocalProfile, PowerInfo};
use grokhub_core::local_setup::Hardware;

use crate::route::{local, local_perf};

static CURRENT: Mutex<Option<DeviceProfile>> = Mutex::new(None);
static GENERATION: AtomicU64 = AtomicU64::new(0);
static STARTED: AtomicBool = AtomicBool::new(false);
static CONFIG: Mutex<Option<PathBuf>> = Mutex::new(None);
/// Where models go, once [`start`] ran.
static HOME: Mutex<Option<PathBuf>> = Mutex::new(None);
/// Held while the hardware is read, so [`read_now`] waits for the thread's
/// read instead of running a second one.
static READING: Mutex<()> = Mutex::new(());

/// How often power and the OS look are read again.
const RELOOK: Duration = Duration::from_secs(5 * 60);

/// The device as last read, or the cached `device.json` before the first read.
pub fn current() -> Option<DeviceProfile> {
    CURRENT.lock().ok()?.clone()
}

/// Goes up each time the record changes (first read, power, OS look, screen).
pub fn generation() -> u64 {
    GENERATION.load(Ordering::SeqCst)
}

fn store(mut d: DeviceProfile) {
    let changed = {
        let Ok(mut cur) = CURRENT.lock() else {
            return;
        };
        // The screen comes from the UI; a reader's record never clears it.
        if d.display.is_none() {
            d.display = cur.as_ref().and_then(|c| c.display);
        }
        let same = cur.as_ref().is_some_and(|c| same_record(c, &d));
        *cur = Some(d.clone());
        !same
    };
    if changed {
        GENERATION.fetch_add(1, Ordering::SeqCst);
        if let Some(dir) = CONFIG.lock().ok().and_then(|c| c.clone()) {
            let _ = dev::save(&dir, &d);
        }
    }
}

/// Equal apart from when it was read and the free disk drifting.
fn same_record(a: &DeviceProfile, b: &DeviceProfile) -> bool {
    let strip = |d: &DeviceProfile| DeviceProfile {
        read_at_ms: 0,
        free_disk_mb: d.free_disk_mb / 1_024,
        battery_pct: d.battery_pct.map(|p| p / 10),
        ..d.clone()
    };
    strip(a) == strip(b)
}

/// The router's downshift rung and the local runtime defaults from the record.
/// `first`: also size the local runtime (context, GPU layers, threads).
pub fn apply_to_router(d: &DeviceProfile, first: bool) {
    local::set_device(router_device(d));
    if first {
        let main = d
            .discrete()
            .max_by_key(|g| g.vram_mb)
            .or_else(|| d.gpus.iter().find(|g| g.unified));
        local_perf::apply_hardware(&Hardware {
            gpu: main.map(|g| g.name.clone()).unwrap_or_default(),
            vram_mb: main.map(|g| g.vram_mb).unwrap_or(0),
            ram_mb: d.ram_mb,
            free_disk_mb: d.free_disk_mb,
        });
    }
}

fn router_device(d: &DeviceProfile) -> local::Device {
    local::Device {
        battery_low: d.battery_low(),
        low_end: d.low_end(),
    }
}

/// The UI tells us about the screen it opened on (the readers can't see it).
pub fn set_display(display: Display) {
    let Some(mut d) = current() else {
        return;
    };
    if d.display == Some(display) {
        return;
    }
    d.display = Some(display);
    store(d);
}

/// Read the machine once per process, off the calling thread. `home` is where
/// models go (for free disk), `config_dir` holds `device.json` and `local/`.
pub fn start(home: PathBuf, config_dir: PathBuf) {
    if STARTED.swap(true, Ordering::SeqCst) {
        return;
    }
    if let Ok(mut h) = HOME.lock() {
        *h = Some(home.clone());
    }
    if let Some(cached) = load_cached(&config_dir) {
        apply_to_router(&cached, true);
    }
    let spawned = std::thread::Builder::new()
        .name("device-read".into())
        .spawn(move || {
            let profile = read_local(&home, &config_dir);
            let d = super::read_device(&home, &profile);
            apply_to_router(&d, true);
            store(d);
            loop {
                std::thread::sleep(RELOOK);
                relook(&profile);
            }
        });
    if spawned.is_err() {
        STARTED.store(false, Ordering::SeqCst);
    }
}

/// The hardware for the local-model launcher, read once: the thread's read,
/// or [`read_now`]'s when a start came first.
fn read_local(home: &Path, config_dir: &Path) -> LocalProfile {
    read_local_with(home, config_dir, &super::read_profile)
}

/// [`read_local`] with `read` reading the hardware.
fn read_local_with(
    home: &Path,
    config_dir: &Path,
    read: &dyn Fn(&Path) -> LocalProfile,
) -> LocalProfile {
    let _one = READING.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(p) = crate::route::local_autoconfig::profile() {
        return p;
    }
    let profile = read(home);
    crate::route::local_autoconfig::set_profile(profile.clone(), config_dir.join("local"));
    profile
}

/// Read the hardware on this thread when a model starts before the
/// background read is done, so its launch is planned (with its drafter)
/// rather than the one-shot defaults. False when [`start`] never ran (the
/// command line and tests), so there is nothing to read with.
pub fn read_now() -> bool {
    read_now_with(&super::read_profile)
}

/// [`read_now`] with `read` reading the hardware.
fn read_now_with(read: &dyn Fn(&Path) -> LocalProfile) -> bool {
    let home = HOME.lock().ok().and_then(|h| h.clone());
    let config = CONFIG.lock().ok().and_then(|c| c.clone());
    let (Some(home), Some(config)) = (home, config) else {
        return false;
    };
    read_local_with(&home, &config, read);
    true
}

/// Make the last saved record live and remember where records go.
fn load_cached(config_dir: &Path) -> Option<DeviceProfile> {
    if let Ok(mut c) = CONFIG.lock() {
        *c = Some(config_dir.to_path_buf());
    }
    let cached = dev::load(config_dir)?;
    if let Ok(mut cur) = CURRENT.lock() {
        *cur = Some(cached.clone());
    }
    GENERATION.fetch_add(1, Ordering::SeqCst);
    Some(cached)
}

/// Power and the OS look again; the hardware stays as read at launch.
fn relook(profile: &LocalProfile) {
    let Some(mut d) = current() else {
        return;
    };
    let look = super::look::read_look();
    let power = super::power_quick().unwrap_or(profile.power);
    set_power(&mut d, power);
    d.look = look;
    d.read_at_ms = grokhub_core::now_ms();
    apply_to_router(&d, false);
    store(d);
}

fn set_power(d: &mut DeviceProfile, p: PowerInfo) {
    d.battery = p.battery;
    d.on_ac = p.on_ac;
    d.battery_pct = p.percent;
    d.power_saver = p.power_saver;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn laptop() -> DeviceProfile {
        DeviceProfile {
            os: "linux".into(),
            ram_mb: 16_000,
            battery: true,
            on_ac: true,
            battery_pct: Some(81),
            free_disk_mb: 120_000,
            ..DeviceProfile::default()
        }
    }

    #[test]
    fn an_early_start_reads_the_hardware_once_and_the_device_read_reuses_it() {
        use std::sync::atomic::AtomicUsize;
        let root = std::env::temp_dir().join(format!("gh-read-now-{}", std::process::id()));
        let (home, config) = (root.join("home"), root.join("config"));
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(&config).unwrap();
        let set = |h: Option<PathBuf>, c: Option<PathBuf>| {
            *HOME.lock().unwrap() = h;
            *CONFIG.lock().unwrap() = c;
        };
        let reads = AtomicUsize::new(0);
        let machine = LocalProfile {
            os: "linux",
            arch: "x86_64",
            gpus: Vec::new(),
            cpu: grokhub_core::local_profile::CpuInfo::new(
                8,
                4,
                0,
                grokhub_core::local_picker::CpuSimd::Avx2,
            ),
            ram_mb: 12_345,
            free_ram_mb: Some(8_000),
            power: PowerInfo::default(),
            thermal: grokhub_core::local_profile::Thermal::Nominal,
            free_disk_mb: 100_000,
        };
        let read = |_: &Path| {
            reads.fetch_add(1, Ordering::SeqCst);
            machine.clone()
        };
        crate::route::local_autoconfig::forget_profile();
        // No device read began (the command line): nothing to read with, so
        // the start keeps its one-shot defaults.
        set(None, None);
        assert!(!read_now_with(&read));
        assert_eq!(reads.load(Ordering::SeqCst), 0);
        assert_eq!(crate::route::local_autoconfig::profile(), None);
        // Once it began, a start that comes first reads the hardware itself.
        set(Some(home.clone()), Some(config.clone()));
        assert!(read_now_with(&read));
        assert_eq!(reads.load(Ordering::SeqCst), 1);
        assert_eq!(
            crate::route::local_autoconfig::profile().map(|p| p.ram_mb),
            Some(12_345)
        );
        assert_eq!(
            crate::route::local_autoconfig::tune_dir(),
            Some(config.join("local"))
        );
        // A second start and the device thread reuse that read.
        assert!(read_now_with(&read));
        assert_eq!(read_local_with(&home, &config, &read).ram_mb, 12_345);
        assert_eq!(reads.load(Ordering::SeqCst), 1);
        crate::route::local_autoconfig::forget_profile();
        set(None, None);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn small_drifts_are_the_same_record_and_power_changes_are_not() {
        let a = laptop();
        let drift = DeviceProfile {
            read_at_ms: 99,
            free_disk_mb: 120_300,
            battery_pct: Some(84),
            ..laptop()
        };
        assert!(same_record(&a, &drift));
        let unplugged = DeviceProfile {
            on_ac: false,
            ..laptop()
        };
        assert!(!same_record(&a, &unplugged));
        let mut bigger = laptop();
        bigger.look.text_scale_pct = Some(150);
        assert!(!same_record(&a, &bigger));
    }

    #[test]
    fn the_router_downshifts_on_a_low_battery_and_back_when_plugged_in() {
        let low = DeviceProfile {
            on_ac: false,
            battery_pct: Some(18),
            ..laptop()
        };
        assert_eq!(local::tier_for(router_device(&low)), local::TIERS[1]);
        assert_eq!(local::tier_for(router_device(&laptop())), local::TIERS[0]);
        let small = DeviceProfile {
            ram_mb: 7_900,
            battery: false,
            ..laptop()
        };
        assert_eq!(local::tier_for(router_device(&small)), local::TIERS[1]);
    }

    #[test]
    fn a_cached_record_is_live_at_once_and_the_screen_is_kept() {
        let dir = std::env::temp_dir().join(format!("grokhub-device-live-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("dir");
        dev::save(&dir, &laptop()).expect("save");
        let before = generation();
        // The cached record only: the reader thread would set this process's
        // local-model profile under other tests.
        load_cached(&dir);
        assert_eq!(current().map(|d| d.ram_mb), Some(16_000));
        assert!(generation() > before);
        let g = generation();
        set_display(Display {
            scale_pct: 150,
            width_px: 2_560,
            monitors: 1,
        });
        assert!(generation() > g);
        assert_eq!(
            current().and_then(|d| d.display).map(|s| s.scale_pct),
            Some(150)
        );
        // The same screen again changes nothing.
        let g = generation();
        set_display(Display {
            scale_pct: 150,
            width_px: 2_560,
            monitors: 1,
        });
        assert_eq!(generation(), g);
        let _ = std::fs::remove_dir_all(dir);
    }
}
