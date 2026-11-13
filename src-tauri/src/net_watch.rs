//! The internet log for the shell. When the user has it on, about once a
//! minute this times a TCP connect to two public DNS resolvers (the same
//! probe the cabin runs) and logs drops and slow minutes in the shared log
//! file. It defers to the cabin while the cabin's scheduler beat is fresh, so
//! the two never probe at once. Only reachability and timings are kept.

use std::path::{Path, PathBuf};
use std::time::Duration;

use grokhub_core::net_log;
use grokhub_core::scheduler_lease::{read_lease, shell_may_fire};

/// Start the background prober.
pub fn start(config_dir: PathBuf) {
    let _ = std::thread::Builder::new()
        .name("net-watch".into())
        .spawn(move || {
            let mut fails = 0;
            loop {
                std::thread::sleep(Duration::from_millis(net_log::PROBE_EVERY_MS));
                let offset = crate::scheduler::utc_offset().unwrap_or(0);
                fails = tick(
                    &config_dir,
                    std::process::id(),
                    fails,
                    &net_log::probe,
                    &grokhub_core::now_ms,
                    offset,
                );
            }
        });
}

/// One look: probe when the log is on and the cabin isn't running, then log
/// the result. Returns the failed probes in a row so far (the log file
/// doesn't keep them).
pub fn tick(
    config_dir: &Path,
    pid: u32,
    fails: u32,
    probe: &dyn Fn() -> Option<u32>,
    now: &dyn Fn() -> u64,
    offset_min: i32,
) -> u32 {
    if !shell_may_fire(read_lease(config_dir).as_ref(), pid, now()) {
        return 0;
    }
    if !net_log::load(config_dir).on {
        return 0;
    }
    let latency = probe();
    // Read again after the probe, so a switch-off meanwhile wins.
    let mut log = net_log::load(config_dir);
    if !log.on {
        return 0;
    }
    log.fails = fails;
    log.note(latency, now(), offset_min);
    let _ = net_log::save(config_dir, &log);
    log.fails
}

/// Home's Turn on for the internet log: switch it on in the shared file.
pub fn turn_on(config_dir: &Path) -> Result<(), String> {
    let mut log = net_log::load(config_dir);
    if log.on {
        return Ok(());
    }
    log.on = true;
    net_log::save(config_dir, &log)
}

#[cfg(test)]
mod tests {
    use super::*;
    use grokhub_core::scheduler_lease::{write_lease, LeaseOwner, SchedulerLease};

    const NOW: u64 = 1_800_000_000_000;

    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "grokhub-net-watch-{name}-{}-{}",
            std::process::id(),
            grokhub_core::now_ms()
        ));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn probes_only_when_on_and_logs_a_drop_after_two_misses() {
        let d = dir("drop");
        let never = || -> Option<u32> { panic!("probed while off") };
        assert_eq!(tick(&d, 7, 0, &never, &|| NOW, 0), 0);
        turn_on(&d).unwrap();
        let down = || None;
        let fails = tick(&d, 7, 0, &down, &|| NOW, 0);
        assert_eq!(fails, 1);
        assert!(!net_log::load(&d).down());
        let fails = tick(&d, 7, fails, &down, &|| NOW + 60_000, 0);
        assert_eq!(fails, 2);
        assert!(net_log::load(&d).down());
        assert_eq!(tick(&d, 7, fails, &|| Some(20), &|| NOW + 120_000, 0), 0);
        assert!(!net_log::load(&d).down());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn defers_while_the_cabin_is_running() {
        let d = dir("cabin");
        turn_on(&d).unwrap();
        write_lease(
            &d,
            &SchedulerLease {
                owner: LeaseOwner::Cabin,
                pid: 99,
                beat_ms: NOW,
            },
        )
        .unwrap();
        let never = || -> Option<u32> { panic!("probed while the cabin runs") };
        assert_eq!(tick(&d, 7, 0, &never, &|| NOW + 1_000, 0), 0);
        let _ = std::fs::remove_dir_all(&d);
    }
}
