//! Startup and periodic background sync for built-in vstack skills.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use grokhub_core::vstack_sync::VstackStatus;

/// Wait briefly after launch so window startup stays immediate before the first
/// upstream check runs.
const STARTUP_DELAY: Duration = Duration::from_secs(5);
/// Wake hourly; `grokhub_agent::vstack_sync::sync_if_due` enforces the 6h cadence.
const TICK: Duration = Duration::from_secs(60 * 60);

static SYNC_LOCK: Mutex<()> = Mutex::new(());

/// Check upstream on launch and periodically while the app runs.
pub fn start(config_dir: PathBuf) {
    std::thread::spawn(move || {
        std::thread::sleep(STARTUP_DELAY);
        loop {
            tick(&config_dir);
            std::thread::sleep(TICK);
        }
    });
}

fn tick(config_dir: &Path) {
    let _guard = SYNC_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    match grokhub_agent::vstack_sync::sync_if_due(config_dir, grokhub_core::now_ms()) {
        Ok(Some(st)) => eprintln!(
            "vstack skills: synced {} ({} skills)",
            st.version, st.skill_count
        ),
        Ok(None) => {}
        Err(why) => eprintln!("vstack skills: sync skipped: {why}"),
    }
}

/// Current active version/commit, source (`"synced"` / `"bundled"`), and last
/// sync metadata for Settings → Code.
pub fn status(config_dir: &Path) -> VstackStatus {
    grokhub_agent::vstack_sync::status(config_dir)
}

/// Immediately check `blackviperxiii-ui/vstack` `main` and update on-disk
/// skills if needed.
pub fn sync_now(config_dir: &Path) -> Result<VstackStatus, String> {
    let _guard = SYNC_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    grokhub_agent::vstack_sync::sync_now(config_dir)
}
