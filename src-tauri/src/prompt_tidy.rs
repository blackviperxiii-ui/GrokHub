//! Settings → Local AI → Tidy typos on this computer: the on-device model
//! fixes typos in a chat message before the chat model reads it. The chat
//! keeps what was typed; numbers, names, paths and links never change.
//!
//! The switch is the cabin's `promptCleanup` in app.json, off until the
//! person turns it on. The router reads it (`route::host::read`) only while
//! Local AI is on (`localModel` is not `false`, the switch in `local_use`),
//! and `route::host::sync` runs before every turn. Nothing goes to the keyring.

use std::path::Path;

use serde::Serialize;
use serde_json::json;

/// The app.json key the router reads.
pub const KEY: &str = "promptCleanup";

/// No local engine and model on disk yet.
pub const NOT_SET_UP: &str = "Set up a local engine and model first.";

const OFF: &str = "Off. Your messages go to the model exactly as you type them.";
/// The tidy only uses a model that is already loaded; it never starts one.
const ON: &str = "On. While the on-device model is loaded, it fixes typos first (up to 0.3 s); Keep loaded tidies every message. Your chat keeps what you typed; numbers, names, paths and links are never changed.";
/// Turning it on while Local AI is switched off (`local_use`).
pub const NEEDS_LOCAL: &str = "Turn on Use Local AI on this computer first.";
/// Local AI is switched off, so the router skips the tidy.
const LOCAL_OFF: &str = "Off while Local AI is off. Turn on Use Local AI on this computer first.";
const PAUSED: &str = "On, but paused. Set up a local engine and model first.";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TidyView {
    /// What the router does: the saved choice while Local AI is on.
    pub on: bool,
    /// Whether it can be turned on now. Turning it off always works.
    pub available: bool,
    pub hint: String,
}

/// The line under the switch: On or Off first.
fn hint(on: bool, set_up: bool, local_model: bool) -> &'static str {
    match (on, set_up) {
        _ if !local_model => LOCAL_OFF,
        (true, true) => ON,
        (true, false) => PAUSED,
        (false, false) => "Off. Set up a local engine and model first.",
        (false, true) => OFF,
    }
}

/// Whether a local engine and model are on disk.
fn set_up(config_dir: &Path) -> bool {
    grokhub_agent::route::local_sidecar::resolve_paths(config_dir).is_some()
}

pub fn view(config_dir: &Path) -> TidyView {
    view_with(config_dir, set_up(config_dir))
}

fn view_with(config_dir: &Path, set_up: bool) -> TidyView {
    let routing = grokhub_agent::route::host::read(config_dir);
    TidyView {
        on: routing.prompt_cleanup,
        available: set_up && routing.local_model,
        hint: hint(routing.prompt_cleanup, set_up, routing.local_model).into(),
    }
}

/// Turn it on or off; the router takes it at once. It turns on only while
/// Local AI is on and set up.
pub fn set(config_dir: &Path, on: bool) -> Result<TidyView, String> {
    set_with(config_dir, on, set_up(config_dir))
}

fn set_with(config_dir: &Path, on: bool, set_up: bool) -> Result<TidyView, String> {
    if on && !grokhub_agent::route::host::read(config_dir).local_model {
        return Err(NEEDS_LOCAL.into());
    }
    if on && !set_up {
        return Err(NOT_SET_UP.into());
    }
    grokhub_core::app_json::patch(config_dir, &[(KEY, json!(on))])?;
    grokhub_agent::route::host::sync(config_dir);
    Ok(view_with(config_dir, set_up))
}

#[cfg(test)]
#[path = "prompt_tidy_tests.rs"]
mod tests;
