//! One app per profile: the new shell and the classic cabin must not run on
//! the same config dir at once. Both take an exclusive OS file lock on
//! `{config}/app.lock` (std `File::try_lock`: flock on Unix, LockFileEx on
//! Windows). The OS drops the lock when the process exits or crashes, so a
//! lock can never go stale. A different config dir is a different lock.
//!
//! `app-owner.json` names the holder so the message can say which app is open.
//! It only words the message; the file lock alone decides.

use serde::{Deserialize, Serialize};
use std::fs::{self, File, OpenOptions, TryLockError};
use std::path::{Path, PathBuf};

pub const LOCK_FILE: &str = "app.lock";
pub const OWNER_FILE: &str = "app-owner.json";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AppKind {
    /// The Tauri desktop shell.
    Shell,
    /// The classic egui cabin.
    Cabin,
}

impl AppKind {
    fn described(self) -> &'static str {
        match self {
            AppKind::Shell => "GrokHub Shell",
            AppKind::Cabin => "The GrokHub cabin",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Owner {
    pub app: AppKind,
    pub pid: u32,
}

/// Another process holds the lock for this config dir.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Held {
    /// Who holds it, when the owner file was readable.
    pub other: Option<Owner>,
}

/// Held for as long as this app runs. Dropping it releases the lock.
#[derive(Debug)]
pub struct AppLock {
    file: Option<File>,
    owner: PathBuf,
}

impl AppLock {
    /// False when the lock file could not be opened or locked for a reason
    /// other than another holder (read-only dir, a filesystem without locks).
    /// The app still starts: the guard fails open rather than locking the
    /// user out.
    pub fn is_guarded(&self) -> bool {
        self.file.is_some()
    }
}

impl Drop for AppLock {
    fn drop(&mut self) {
        if self.file.is_none() {
            return;
        }
        if read_owner(&self.owner).is_some_and(|o| o.pid == std::process::id()) {
            let _ = fs::remove_file(&self.owner);
        }
        // Closing the file releases the OS lock.
        self.file = None;
    }
}

pub fn lock_path(dir: &Path) -> PathBuf {
    dir.join(LOCK_FILE)
}

pub fn owner_path(dir: &Path) -> PathBuf {
    dir.join(OWNER_FILE)
}

pub fn parse_owner(text: &str) -> Option<Owner> {
    serde_json::from_str(text.trim()).ok()
}

fn read_owner(path: &Path) -> Option<Owner> {
    fs::read_to_string(path).ok().and_then(|t| parse_owner(&t))
}

/// Take the app lock for `dir` as `me`. Only GUI and tray starts call this;
/// headless tools (hub, MCP servers, doctor, update) never do.
pub fn claim(dir: &Path, me: AppKind) -> Result<AppLock, Held> {
    let owner = owner_path(dir);
    let unguarded = || AppLock {
        file: None,
        owner: owner.clone(),
    };
    if fs::create_dir_all(dir).is_err() {
        return Ok(unguarded());
    }
    let Ok(file) = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(lock_path(dir))
    else {
        return Ok(unguarded());
    };
    match file.try_lock() {
        Ok(()) => {}
        Err(TryLockError::WouldBlock) => {
            return Err(Held {
                other: read_owner(&owner),
            })
        }
        Err(TryLockError::Error(_)) => return Ok(unguarded()),
    }
    let mine = Owner {
        app: me,
        pid: std::process::id(),
    };
    if let Ok(text) = serde_json::to_string(&mine) {
        let _ = fs::write(&owner, text);
    }
    Ok(AppLock {
        file: Some(file),
        owner,
    })
}

/// The sentence shown when `me` finds the lock held by `other`.
pub fn held_message(me: AppKind, other: Option<AppKind>) -> String {
    let who = match other {
        Some(app) if app != me => app.described(),
        _ => "Another GrokHub app",
    };
    format!("{who} is already open with this profile. Quit it from its tray, then try again.")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Write};
    use std::process::{Command, Stdio};
    use std::sync::atomic::{AtomicU32, Ordering};

    const CHILD_DIR_ENV: &str = "GROKHUB_APP_LOCK_TEST_CHILD_DIR";

    fn tmp(label: &str) -> PathBuf {
        static N: AtomicU32 = AtomicU32::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "grokhub-app-lock-{label}-{}-{n}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn second_claim_on_the_same_dir_is_held_and_names_the_owner() {
        let dir = tmp("same");
        let first = claim(&dir, AppKind::Shell).expect("first claim");
        assert!(first.is_guarded());
        let second = claim(&dir, AppKind::Cabin).expect_err("second claim must be held");
        assert_eq!(
            second,
            Held {
                other: Some(Owner {
                    app: AppKind::Shell,
                    pid: std::process::id(),
                }),
            }
        );
        assert_eq!(
            fs::read_to_string(owner_path(&dir)).expect("owner file"),
            format!("{{\"app\":\"shell\",\"pid\":{}}}", std::process::id())
        );
        drop(first);
        assert!(!owner_path(&dir).exists(), "owner file goes with the lock");
        let again = claim(&dir, AppKind::Cabin).expect("claim after release");
        assert!(again.is_guarded());
        drop(again);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn different_config_dirs_are_different_locks() {
        let a = tmp("a");
        let b = tmp("b");
        let shell = claim(&a, AppKind::Shell).expect("shell on a");
        let cabin = claim(&b, AppKind::Cabin).expect("cabin on b");
        assert!(shell.is_guarded() && cabin.is_guarded());
        assert_eq!(lock_path(&a), a.join("app.lock"));
        drop((shell, cabin));
        let _ = fs::remove_dir_all(&a);
        let _ = fs::remove_dir_all(&b);
    }

    #[test]
    fn owner_file_parses() {
        assert_eq!(
            parse_owner("{\"app\":\"shell\",\"pid\":4242}\n"),
            Some(Owner {
                app: AppKind::Shell,
                pid: 4242,
            })
        );
        assert_eq!(
            parse_owner("{\"app\":\"cabin\",\"pid\":7}"),
            Some(Owner {
                app: AppKind::Cabin,
                pid: 7,
            })
        );
        assert_eq!(parse_owner("{\"app\":\"phone\",\"pid\":1}"), None);
        assert_eq!(parse_owner(""), None);
    }

    #[test]
    fn held_messages_name_the_other_app() {
        assert_eq!(
            held_message(AppKind::Cabin, Some(AppKind::Shell)),
            "GrokHub Shell is already open with this profile. Quit it from its tray, then try again."
        );
        assert_eq!(
            held_message(AppKind::Shell, Some(AppKind::Cabin)),
            "The GrokHub cabin is already open with this profile. Quit it from its tray, then try again."
        );
        assert_eq!(
            held_message(AppKind::Shell, None),
            "Another GrokHub app is already open with this profile. Quit it from its tray, then try again."
        );
    }

    /// Only does anything when the cross-process test starts this binary
    /// with the env var set: take the lock, say so, and wait to be killed.
    #[test]
    #[ignore = "helper process for killed_holder_frees_the_lock"]
    fn child_holds_the_lock_until_killed() {
        let Some(dir) = std::env::var_os(CHILD_DIR_ENV) else {
            return;
        };
        let _lock = claim(Path::new(&dir), AppKind::Shell).expect("child claim");
        let mut out = std::io::stdout();
        writeln!(out, "locked").expect("stdout");
        out.flush().expect("flush");
        std::thread::sleep(std::time::Duration::from_secs(120));
    }

    #[test]
    fn killed_holder_frees_the_lock() {
        let dir = tmp("cross");
        fs::create_dir_all(&dir).expect("dir");
        let exe = std::env::current_exe().expect("test exe");
        let mut child = Command::new(exe)
            .args([
                "app_lock::tests::child_holds_the_lock_until_killed",
                "--exact",
                "--ignored",
                "--nocapture",
                "--test-threads=1",
            ])
            .env(CHILD_DIR_ENV, &dir)
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn holder");
        let child_pid = child.id();
        // libtest prints `test <name> ... ` before the test's own output, on
        // the same line.
        let mut lines = BufReader::new(child.stdout.take().expect("stdout")).lines();
        let saw_locked =
            lines.any(|l| l.map(|l| l.trim_end().ends_with("locked")).unwrap_or(false));
        assert!(saw_locked, "holder never reported the lock");

        let held = claim(&dir, AppKind::Cabin).expect_err("lock held by the child");
        assert_eq!(
            held.other,
            Some(Owner {
                app: AppKind::Shell,
                pid: child_pid,
            })
        );

        child.kill().expect("kill holder");
        child.wait().expect("reap holder");

        // The killed holder never removed its owner file; the lock is free anyway.
        let next = claim(&dir, AppKind::Cabin).expect("claim after the holder died");
        assert!(next.is_guarded());
        assert_eq!(
            read_owner(&owner_path(&dir)),
            Some(Owner {
                app: AppKind::Cabin,
                pid: std::process::id(),
            })
        );
        drop(next);
        let _ = fs::remove_dir_all(&dir);
    }
}
