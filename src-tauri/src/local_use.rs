//! Settings → Local AI → Use Local AI on this computer: the app.json
//! `localModel` switch the router reads (`route::host::read`). A missing key
//! means on, so setting up an engine and model is enough for new installs.
//!
//! The retired egui app saved `localModel: false` for anyone who never turned
//! its on-device model on, which kept Local AI off here with no way back.
//! This switch is that way back, and the off switch for anyone who wants it.
//! `route::host::sync` runs right after a change, so the router takes it at
//! once. Nothing goes to the keyring.

use std::path::Path;

use grokhub_agent::route::local_sidecar;
use serde::Serialize;
use serde_json::json;

/// The app.json key the router reads.
pub const KEY: &str = "localModel";

/// On, with an engine and model on disk.
const ON: &str = "On. While the model is loaded, background jobs and short chats run on this computer, and any chat does while you're offline or the cloud can't answer.";
/// On, with nothing installed yet.
const ON_NOT_SET_UP: &str = "On. Set up a local engine and model below and it starts helping.";
const OFF: &str = "Off. Nothing runs on the on-device model.";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalUseView {
    pub on: bool,
    pub hint: String,
}

fn view_of(on: bool, set_up: bool) -> LocalUseView {
    let hint = match (on, set_up) {
        (false, _) => OFF,
        (true, true) => ON,
        (true, false) => ON_NOT_SET_UP,
    };
    LocalUseView {
        on,
        hint: hint.into(),
    }
}

/// Whether a local engine and model are on disk.
fn set_up(config_dir: &Path) -> bool {
    local_sidecar::resolve_paths(config_dir).is_some()
}

/// Whether the switch is on: a missing or unreadable app.json counts as on.
pub fn is_on(config_dir: &Path) -> bool {
    grokhub_agent::route::host::read(config_dir).local_model
}

pub fn view(config_dir: &Path) -> LocalUseView {
    view_of(is_on(config_dir), set_up(config_dir))
}

/// Turn Local AI on or off; the router takes it at once. Off lets an answer
/// in flight finish, then stops the loaded model so it frees its memory; on
/// cancels that stop. Other app.json keys stay as they are.
pub fn set(config_dir: &Path, on: bool) -> Result<LocalUseView, String> {
    grokhub_core::app_json::patch(config_dir, &[(KEY, json!(on))])?;
    grokhub_agent::route::host::sync(config_dir);
    if on {
        local_sidecar::cancel_pending_stop();
    } else {
        local_sidecar::stop_when_idle();
    }
    Ok(view(config_dir))
}

#[cfg(test)]
#[path = "local_use_tests.rs"]
mod tests;
