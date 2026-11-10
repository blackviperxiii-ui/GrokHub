//! Cabin side of the one-app-per-profile lock (`grokhub_core::app_lock`).
//! Only the window and tray starts (`run_cabin`) take it; the hub, MCP
//! servers, doctor, update, oauth and bench never do.

use grokhub_core::app_lock::{self, AppKind, AppLock, Held};
use std::sync::Mutex;

static LOCK: Mutex<Option<AppLock>> = Mutex::new(None);

/// How a start that finds the lock held tells the user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HeldNotice {
    /// Tray start (systemd, `--agent`): one stderr line, exit 0, no window,
    /// so `Restart=on-failure` never loops.
    Stderr(String),
    /// A window start: a message box, then exit 0.
    Dialog(String),
}

pub fn held_notice(hidden: bool, held: &Held) -> HeldNotice {
    if hidden {
        let who = match held.other {
            Some(o) => format!(
                "{} pid {}",
                match o.app {
                    AppKind::Shell => "shell",
                    AppKind::Cabin => "cabin",
                },
                o.pid
            ),
            None => "another app".to_string(),
        };
        return HeldNotice::Stderr(format!(
            "grokhub: this profile is open in {who}; tray start skipped"
        ));
    }
    HeldNotice::Dialog(app_lock::held_message(
        AppKind::Cabin,
        held.other.map(|o| o.app),
    ))
}

/// Take the app lock for this process's config dir. A second call while
/// held is a no-op.
pub fn claim() -> Result<(), Held> {
    let mut slot = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    if slot.is_some() {
        return Ok(());
    }
    *slot = Some(app_lock::claim(
        &crate::config::config_dir(),
        AppKind::Cabin,
    )?);
    Ok(())
}

/// Drop the lock (quit, or a restart about to spawn the next cabin).
pub fn release() {
    let mut slot = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    slot.take();
}

pub fn tell(notice: &HeldNotice) {
    match notice {
        HeldNotice::Stderr(line) => eprintln!("{line}"),
        HeldNotice::Dialog(text) => {
            let _ = rfd::MessageDialog::new()
                .set_level(rfd::MessageLevel::Warning)
                .set_title(grokhub_core::brand::product_name())
                .set_description(text.as_str())
                .set_buttons(rfd::MessageButtons::Ok)
                .show();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use grokhub_core::app_lock::Owner;

    #[test]
    fn tray_start_exits_quietly_with_one_line() {
        let held = Held {
            other: Some(Owner {
                app: AppKind::Shell,
                pid: 4242,
            }),
        };
        assert_eq!(
            held_notice(true, &held),
            HeldNotice::Stderr(
                "grokhub: this profile is open in shell pid 4242; tray start skipped".into()
            )
        );
        assert_eq!(
            held_notice(true, &Held { other: None }),
            HeldNotice::Stderr(
                "grokhub: this profile is open in another app; tray start skipped".into()
            )
        );
    }

    #[test]
    fn window_start_shows_a_message_box() {
        let held = Held {
            other: Some(Owner {
                app: AppKind::Shell,
                pid: 4242,
            }),
        };
        assert_eq!(
            held_notice(false, &held),
            HeldNotice::Dialog(
                "The other Loadout app is already open with this profile. Quit it from its tray, then try again."
                    .into()
            )
        );
    }

    fn between<'a>(src: &'a str, start: &str, end: &str) -> &'a str {
        src.split(start)
            .nth(1)
            .and_then(|s| s.split(end).next())
            .unwrap_or_else(|| panic!("{start}..{end}"))
    }

    #[test]
    fn only_window_and_tray_starts_take_the_lock() {
        let main = include_str!("main.rs");
        assert_eq!(
            main.matches("app_guard::claim()").count(),
            1,
            "one claim site"
        );
        let run = between(main, "fn run_cabin(", "fn run_cabin_with_fx");
        assert!(run.contains("app_guard::claim()"), "{run}");
        assert!(
            run.find("tray::try_claim_cabin()").expect("pid claim")
                < run.find("app_guard::claim()").expect("app claim"),
            "a second cabin raises the first before the app lock is checked: {run}"
        );
        assert!(
            run.find("app_guard::claim()").expect("app claim")
                < run.find("eframe::run_native").expect("run_native"),
            "{run}"
        );
        let dispatch = between(
            main,
            "    match launch {\n        Launch::Version =>",
            "\nfn run_oauth_cli(",
        );
        assert_eq!(dispatch.matches("run_cabin(").count(), 2, "{dispatch}");
        for arm in [
            "Launch::Hub =>",
            "Launch::Doctor =>",
            "Launch::Update =>",
            "Launch::Oauth =>",
            "Launch::McpDesktop =>",
            "Launch::McpCua =>",
            "Launch::McpSelf =>",
            "Launch::BenchLocal =>",
        ] {
            let body = between(dispatch, arm, "Launch::");
            assert!(
                !body.contains("run_cabin("),
                "{arm} must not open the cabin: {body}"
            );
        }
        assert!(
            between(dispatch, "Launch::Agent =>", "Launch::Cabin =>").contains("run_cabin(true)")
        );
        assert!(dispatch
            .split("Launch::Cabin =>")
            .nth(1)
            .expect("cabin arm")
            .contains("run_cabin(false)"));
    }

    #[test]
    fn restarts_release_the_lock_before_the_next_cabin_starts() {
        let tray = include_str!("tray.rs");
        let release = between(tray, "pub fn release_cabin_claim() {", "\n}\n");
        assert!(release.contains("app_guard::release()"), "{release}");
        let update = include_str!("update.rs");
        let replace = between(update, "fn replace_process(", "\n}\n");
        assert!(
            replace.find("release_cabin_claim()").expect("release")
                < replace.find("spawn_detached").expect("spawn"),
            "{replace}"
        );
        assert!(
            replace.contains("app_guard::claim()"),
            "a failed restart keeps the lock: {replace}"
        );
    }
}
