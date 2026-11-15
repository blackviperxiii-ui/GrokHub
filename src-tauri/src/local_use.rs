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

use serde::Serialize;
use serde_json::json;

/// The app.json key the router reads.
pub const KEY: &str = "localModel";

const ON: &str = "On. Background jobs such as sorting, summaries and hiding private details can run on this computer once a local engine and model are set up.";
const OFF: &str = "Off. Nothing runs on the on-device model, and typo tidy is paused.";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalUseView {
    pub on: bool,
    pub hint: String,
}

fn view_of(on: bool) -> LocalUseView {
    LocalUseView {
        on,
        hint: (if on { ON } else { OFF }).into(),
    }
}

pub fn view(config_dir: &Path) -> LocalUseView {
    view_of(grokhub_agent::route::host::read(config_dir).local_model)
}

/// Turn Local AI on or off; the router takes it at once. Other app.json keys
/// stay as they are.
pub fn set(config_dir: &Path, on: bool) -> Result<LocalUseView, String> {
    grokhub_core::app_json::patch(config_dir, &[(KEY, json!(on))])?;
    grokhub_agent::route::host::sync(config_dir);
    Ok(view(config_dir))
}

#[cfg(test)]
#[path = "local_use_tests.rs"]
mod tests;
