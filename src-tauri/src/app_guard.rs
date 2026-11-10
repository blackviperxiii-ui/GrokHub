//! The shell's side of the one-app-per-profile lock (`grokhub_core::app_lock`):
//! the egui cabin and this shell never run on the same config dir at once.
//! Only the app itself takes it, after the single-instance hand-off (so a
//! second shell launch still just shows the first) and before any window.

use std::path::Path;
use std::sync::Mutex;

use grokhub_core::app_lock::{self, AppKind, AppLock, Held};
use tauri::AppHandle;

/// Held for as long as the shell runs; the OS drops it at exit or crash.
static LOCK: Mutex<Option<AppLock>> = Mutex::new(None);

/// What a start does about the lock.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LockStart {
    /// This shell holds the profile now.
    Go,
    /// A login start finding the profile taken: leave quietly, exit 0.
    ExitQuietly,
    /// A start someone asked for: say why, then exit 0.
    Tell(String),
}

pub fn lock_start(held: Option<&Held>, login: bool) -> LockStart {
    match held {
        None => LockStart::Go,
        Some(_) if login => LockStart::ExitQuietly,
        Some(h) => LockStart::Tell(app_lock::held_message(
            AppKind::Shell,
            h.other.map(|o| o.app),
        )),
    }
}

fn claim(dir: &Path) -> Result<(), Held> {
    let mut slot = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    if slot.is_none() {
        *slot = Some(app_lock::claim(dir, AppKind::Shell)?);
    }
    Ok(())
}

/// Take the profile for this shell. False when setup must stop: the start
/// then either exits quietly or shows a message box and exits on its OK.
pub fn take_profile(app: &AppHandle, dir: &Path) -> bool {
    let held = claim(dir).err();
    match lock_start(held.as_ref(), crate::os_extras::at_login(std::env::args())) {
        LockStart::Go => true,
        LockStart::ExitQuietly => {
            eprintln!("grokhub-shell: this profile is open in the other app; login start skipped");
            std::process::exit(0);
        }
        LockStart::Tell(text) => {
            use tauri_plugin_dialog::{DialogExt, MessageDialogKind};
            let handle = app.clone();
            app.dialog()
                .message(text)
                .title(grokhub_core::brand::product_name())
                .kind(MessageDialogKind::Warning)
                .show(move |_| handle.exit(0));
            false
        }
    }
}

#[cfg(test)]
#[path = "app_guard_tests.rs"]
mod tests;
