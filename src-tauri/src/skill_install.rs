//! Deep-link handling for `loadout://skill/install?identifier=`.
//!
//! Second-instance argv and cold-start args are parsed here; a confirm offer
//! is emitted to the UI. Install itself waits for the user's approval.

use std::path::Path;
use std::sync::Mutex;

use tauri::{AppHandle, Emitter, Manager};

use crate::ShellState;

pub const OFFER_EVENT: &str = "skill-install://offer";

static PENDING: Mutex<Option<grokhub_core::SkillInstallPreview>> = Mutex::new(None);

/// Walk argv for a `loadout://skill/install` URL (browser / OS scheme hand-off).
pub fn find_install_url(args: &[String]) -> Option<String> {
    args.iter()
        .find(|a| grokhub_core::is_skill_install_url(a))
        .cloned()
}

/// Parse the URL, build a preview, store it, and tell the window.
pub fn offer_from_url(app: &AppHandle, url: &str) -> Result<(), String> {
    let state = app.state::<ShellState>();
    let identifier = grokhub_core::parse_skill_install_url(url)?;
    let preview = crate::skills::preview_install(&state.config_dir, &identifier)?;
    set_pending(Some(preview.clone()));
    let _ = app.emit(OFFER_EVENT, &preview);
    crate::show_main(app);
    Ok(())
}

pub fn set_pending(preview: Option<grokhub_core::SkillInstallPreview>) {
    *PENDING.lock().unwrap_or_else(|e| e.into_inner()) = preview;
}

pub fn take_pending() -> Option<grokhub_core::SkillInstallPreview> {
    PENDING.lock().unwrap_or_else(|e| e.into_inner()).take()
}

pub fn peek_pending() -> Option<grokhub_core::SkillInstallPreview> {
    PENDING.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

/// Handle argv from a second launch or cold start. Returns true when a skill
/// install URL was consumed.
pub fn handle_args(app: &AppHandle, args: &[String]) -> bool {
    let Some(url) = find_install_url(args) else {
        return false;
    };
    if let Err(err) = offer_from_url(app, &url) {
        eprintln!("skill install link: {err}");
    }
    true
}

/// Seed a pending offer from process argv once the app is up (cold start).
pub fn setup_from_env(app: &AppHandle) {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let _ = handle_args(app, &args);
}

/// Destination folder shown in tests / UI copy helpers.
#[allow(dead_code)]
pub fn skills_dir(config_dir: &Path) -> std::path::PathBuf {
    grokhub_core::skills_install_dir(config_dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_loadout_url_among_argv() {
        let args = vec![
            "--tray".into(),
            "loadout://skill/install?identifier=library/weekly-meal-plan".into(),
        ];
        assert_eq!(
            find_install_url(&args).as_deref(),
            Some("loadout://skill/install?identifier=library/weekly-meal-plan")
        );
        assert!(find_install_url(&["--autostart".into()]).is_none());
    }
}
