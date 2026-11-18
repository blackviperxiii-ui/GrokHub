//! Each chat's on-device tier from its tab menu ("Model for this chat"), in
//! `{config}/local-chat-pins.json` as `{"chats": {"<conversation id>": tier}}`.
//! A chat with no entry is on Auto. Words in a turn ("be quick", "think
//! hard") still beat the setting for that turn (see
//! [`super::local_scale::chat_tier`]). A helper's own conversation id
//! (`parent/child`) never has an entry, so a helper runs on whatever loads.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use super::table::SpeedQuality;

const FILE: &str = "local-chat-pins.json";

/// The chats set to a tier, by conversation id.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pins {
    #[serde(default)]
    pub chats: BTreeMap<String, SpeedQuality>,
}

/// One writer at a time, so two quick changes don't drop one.
static LOCK: Mutex<()> = Mutex::new(());

pub fn read(dir: &Path) -> Pins {
    std::fs::read(dir.join(FILE))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

/// The tier `id` is set to, or `None` on Auto.
pub fn get(dir: &Path, id: &str) -> Option<SpeedQuality> {
    read(dir).chats.get(id).copied()
}

/// Set `id` to `tier`, or back to Auto with `None`. Returns what it is now.
pub fn set(
    dir: &Path,
    id: &str,
    tier: Option<SpeedQuality>,
) -> Result<Option<SpeedQuality>, String> {
    let _held = LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let mut pins = read(dir);
    match tier {
        Some(t) => pins.chats.insert(id.to_string(), t),
        None => pins.chats.remove(id),
    };
    let body = serde_json::to_vec_pretty(&pins).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(dir).map_err(|e| format!("Couldn't save: {e}"))?;
    let tmp = dir.join(format!("{FILE}.tmp"));
    std::fs::write(&tmp, body).map_err(|e| format!("Couldn't save: {e}"))?;
    std::fs::rename(&tmp, dir.join(FILE)).map_err(|e| format!("Couldn't save: {e}"))?;
    Ok(tier)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_chat_tier_round_trips_and_an_unknown_chat_is_on_auto() {
        let dir = std::env::temp_dir().join(format!("grokhub-local-pin-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let _cfg = crate::perm::ConfigGuard::set(&dir);
        let config = crate::perm::config_dir();
        assert_eq!(get(&config, "shell-a"), None, "no file yet");
        assert_eq!(
            set(&config, "shell-a", Some(SpeedQuality::Speed)),
            Ok(Some(SpeedQuality::Speed))
        );
        set(&config, "shell-b", Some(SpeedQuality::Quality)).unwrap();
        assert_eq!(get(&config, "shell-a"), Some(SpeedQuality::Speed));
        assert_eq!(get(&config, "shell-b"), Some(SpeedQuality::Quality));
        assert_eq!(get(&config, "shell-c"), None);
        assert_eq!(
            get(&config, "shell-a/helper-1"),
            None,
            "a helper has its own id"
        );
        let text = std::fs::read_to_string(dir.join(FILE)).unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&text).unwrap(),
            serde_json::json!({"chats": {"shell-a": "speed", "shell-b": "quality"}})
        );
        assert_eq!(set(&config, "shell-a", None), Ok(None));
        assert_eq!(get(&config, "shell-a"), None, "back on Auto");
        assert_eq!(get(&config, "shell-b"), Some(SpeedQuality::Quality));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
