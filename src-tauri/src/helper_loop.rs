//! The PC helpers' once-a-minute tick for the shell: the daily rhythm counts a
//! minute while the user is at this window, and wind-down puts an idle PC to
//! sleep. It runs off the main thread and only while the cabin's scheduler
//! beat is stale (the cabin runs the same helpers when it is open), the same
//! rule as `net_watch`. Nothing is read while both helpers are off.
//!
//! GPU busy comes from `grokhub_desktop::signals` (nvidia-smi / AMD sysfs /
//! Windows PDH), with this process left out of the total. A PC that still
//! can't see a game, fullscreen or GPU signal stays blind and the Settings
//! page says so. The one-time set-up card isn't posted here: the page offers
//! the idle times itself.

use std::path::{Path, PathBuf};
use std::time::Duration;

use grokhub_core::rhythm;
use grokhub_core::scheduler_lease::{read_lease, shell_may_fire};
use grokhub_core::wind_down::{self, Busy, Machine, Step};
use tauri::{AppHandle, Manager};

const EVERY: Duration = Duration::from_secs(60);
/// No input for this long and the user isn't at the window.
const ACTIVE_WITHIN_SECS: u64 = 120;

/// What one tick did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Did {
    pub counted_minute: bool,
    pub slept: bool,
}

/// Start the tick thread.
pub fn start(config_dir: PathBuf, app: AppHandle) {
    let _ = std::thread::Builder::new()
        .name("pc-helpers".into())
        .spawn(move || loop {
            std::thread::sleep(EVERY);
            let now = grokhub_core::now_ms();
            let offset = crate::scheduler::utc_offset().unwrap_or(0);
            let focused = app
                .webview_windows()
                .values()
                .any(|w| w.is_focused().unwrap_or(false));
            let _ = tick(
                &config_dir,
                std::process::id(),
                now,
                offset,
                focused,
                &read_machine,
                &grokhub_desktop::signals::sleep_pc,
            );
        });
}

/// One fresh reading of this PC, blocking on the OS readers.
pub fn read_machine() -> Machine {
    use grokhub_desktop::signals;
    let external_gpu_pct = signals::external_gpu_pct(Some(std::process::id()));
    // Fullscreen is Windows-only in `signals`. When the GPU reader answered,
    // treat a missing fullscreen as clear so Linux can sleep once GPU and
    // game are known; games still block through `game()`.
    let fullscreen = signals::fullscreen().or_else(|| external_gpu_pct.map(|_| false));
    Machine {
        idle_secs: signals::idle_secs(),
        game: signals::game(),
        fullscreen,
        external_gpu_pct,
    }
}

/// One look. `focused` is whether this app's window has focus.
pub fn tick(
    config_dir: &Path,
    pid: u32,
    now_ms: u64,
    offset_min: i32,
    focused: bool,
    read: &dyn Fn() -> Machine,
    sleep: &dyn Fn() -> Result<(), String>,
) -> Result<Did, String> {
    let mut did = Did::default();
    if !shell_may_fire(read_lease(config_dir).as_ref(), pid, now_ms) {
        return Ok(did);
    }
    let rhythm_path = config_dir.join(rhythm::FILE_NAME);
    let rhythm_on = rhythm::load(&rhythm_path).on;
    let wind_on = wind_down::load(config_dir).on;
    if !rhythm_on && !wind_on {
        return Ok(did);
    }
    let machine = read();
    let at_window = focused && machine.idle_secs.is_some_and(|s| s < ACTIVE_WITHIN_SECS);

    if rhythm_on && at_window {
        let clock = crate::scheduler::local_clock(now_ms, offset_min);
        crate::pc_helpers::locked(|| {
            let mut r = rhythm::load(&rhythm_path);
            if !r.note_active(&clock) {
                return Ok(());
            }
            did.counted_minute = true;
            rhythm::save(&rhythm_path, &r)
        })?;
    }

    if wind_on {
        crate::pc_helpers::note_blind(machine.busy() == Some(Busy::Unknown));
        let tracker = wind_down::load(config_dir);
        if let Some(Step::Sleep { .. }) = tracker.decide(now_ms, machine.idle_secs, machine.busy())
        {
            // Read again after the reading, so a switch-off meanwhile wins.
            let mut tracker = wind_down::load(config_dir);
            if tracker.on {
                tracker.slept(now_ms);
                crate::pc_helpers::locked(|| wind_down::save(config_dir, &tracker))?;
                if shell_may_fire(read_lease(config_dir).as_ref(), pid, now_ms) {
                    sleep()?;
                    did.slept = true;
                }
            }
        }
    }
    Ok(did)
}

#[cfg(test)]
#[path = "helper_loop_tests.rs"]
mod tests;
