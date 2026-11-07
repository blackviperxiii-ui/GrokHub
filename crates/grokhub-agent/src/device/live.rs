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
    if let Some(cached) = load_cached(&config_dir) {
        apply_to_router(&cached, true);
    }
    let spawned = std::thread::Builder::new()
        .name("device-read".into())
        .spawn(move || {
            let profile = super::read_profile(&home);
            crate::route::local_autoconfig::set_profile(profile.clone(), config_dir.join("local"));
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
