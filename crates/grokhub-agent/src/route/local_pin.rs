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
use serde_json::Value;

use super::table::SpeedQuality;

const FILE: &str = "local-chat-pins.json";

/// The chats set to a tier, by conversation id. Each entry is read on its
/// own: one this build can't read (a tier from a newer build, a hand edit)
/// is carried through every write untouched and reads as Auto.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Pins {
    #[serde(default)]
    pub chats: BTreeMap<String, Value>,
}

impl Pins {
    /// The tier `id` is set to, when this build knows it.
    pub fn tier(&self, id: &str) -> Option<SpeedQuality> {
        serde_json::from_value(self.chats.get(id)?.clone()).ok()
    }
}

/// One writer at a time, so two quick changes don't drop one.
static LOCK: Mutex<()> = Mutex::new(());

/// The saved pins: none without a file, an error when the file is there but
/// isn't a pins object, so a write never replaces what it couldn't read.
fn load(dir: &Path) -> Result<Pins, String> {
    match std::fs::read(dir.join(FILE)) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map_err(|e| format!("Couldn't read {FILE}, so it was left as it is: {e}")),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Pins::default()),
        Err(e) => Err(format!("Couldn't read {FILE}: {e}")),
    }
}

/// The saved pins, or none when the file can't be read.
pub fn read(dir: &Path) -> Pins {
    load(dir).unwrap_or_default()
}

/// The tier `id` is set to, or `None` on Auto.
pub fn get(dir: &Path, id: &str) -> Option<SpeedQuality> {
    read(dir).tier(id)
}

/// Set `id` to `tier`, or back to Auto with `None`. Returns what it is now.
pub fn set(
    dir: &Path,
    id: &str,
    tier: Option<SpeedQuality>,
) -> Result<Option<SpeedQuality>, String> {
    let _held = LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let mut pins = load(dir)?;
    match tier {
        Some(t) => {
            let v = serde_json::to_value(t).map_err(|e| e.to_string())?;
            pins.chats.insert(id.to_string(), v)
        }
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

    #[test]
    fn a_tier_this_build_cant_read_is_kept_and_a_broken_file_is_never_overwritten() {
        let dir =
            std::env::temp_dir().join(format!("grokhub-local-pin-new-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join(FILE),
            r#"{"chats":{"shell-a":"speed","shell-b":"quality","shell-c":"turbo"}}"#,
        )
        .unwrap();
        assert_eq!(get(&dir, "shell-a"), Some(SpeedQuality::Speed));
        assert_eq!(get(&dir, "shell-b"), Some(SpeedQuality::Quality));
        assert_eq!(get(&dir, "shell-c"), None, "an unknown tier reads as Auto");
        assert_eq!(
            set(&dir, "shell-d", Some(SpeedQuality::Speed)),
            Ok(Some(SpeedQuality::Speed))
        );
        let text = std::fs::read_to_string(dir.join(FILE)).unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&text).unwrap(),
            serde_json::json!({"chats": {
                "shell-a": "speed",
                "shell-b": "quality",
                "shell-c": "turbo",
                "shell-d": "speed"
            }})
        );
        // A file that isn't a pins object: the read is Auto, a write refuses.
        std::fs::write(dir.join(FILE), "{not json").unwrap();
        assert_eq!(get(&dir, "shell-a"), None);
        assert!(set(&dir, "shell-a", Some(SpeedQuality::Quality))
            .is_err_and(|e| e.starts_with("Couldn't read local-chat-pins.json")));
        assert_eq!(
            std::fs::read_to_string(dir.join(FILE)).unwrap(),
            "{not json",
            "left as it was"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
