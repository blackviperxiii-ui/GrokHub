//! The shell's own settings file, `shell.json` in the GrokHub config folder.
//! The API key is never written here; it lives in the OS keychain.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const FILE: &str = "shell.json";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ShellSettings {
    pub model: String,
    pub close_to_tray: bool,
    /// The folder the agent works in. Empty means the cabin's default.
    pub workspace: String,
}

impl Default for ShellSettings {
    fn default() -> Self {
        Self {
            model: grokhub_agent::DEFAULT_MODEL.to_string(),
            close_to_tray: true,
            workspace: String::new(),
        }
    }
}

pub fn path(config_dir: &Path) -> PathBuf {
    config_dir.join(FILE)
}

/// A missing or unreadable file gives the defaults, so a bad edit never blocks startup.
pub fn load(config_dir: &Path) -> ShellSettings {
    std::fs::read_to_string(path(config_dir))
        .ok()
        .and_then(|raw| serde_json::from_str::<ShellSettings>(&raw).ok())
        .map(|mut s| {
            if s.model.trim().is_empty() {
                s.model = ShellSettings::default().model;
            }
            s
        })
        .unwrap_or_default()
}

/// Writes through a temp file and a rename, so a crash never leaves half a file.
pub fn save(config_dir: &Path, settings: &ShellSettings) -> Result<(), String> {
    if settings.model.trim().is_empty() {
        return Err("Pick a model.".into());
    }
    std::fs::create_dir_all(config_dir).map_err(|e| e.to_string())?;
    let raw = serde_json::to_string_pretty(settings).map_err(|e| e.to_string())?;
    let tmp = config_dir.join(format!("{FILE}.tmp"));
    std::fs::write(&tmp, raw).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, path(config_dir)).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "grokhub-shell-{name}-{}-{}",
            std::process::id(),
            grokhub_core::now_ms()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn missing_file_gives_defaults() {
        let dir = temp_dir("missing");
        assert_eq!(
            load(&dir),
            ShellSettings {
                model: "grok-4.7".into(),
                close_to_tray: true,
                workspace: String::new(),
            }
        );
    }

    #[test]
    fn save_then_load_round_trips() {
        let dir = temp_dir("roundtrip");
        let wanted = ShellSettings {
            model: "grok-5".into(),
            close_to_tray: false,
            workspace: "/work/notes".into(),
        };
        save(&dir, &wanted).expect("save");
        assert_eq!(load(&dir), wanted);
        let raw = std::fs::read_to_string(path(&dir)).expect("file");
        assert!(raw.contains("\"closeToTray\": false"), "{raw}");
        assert!(raw.contains("\"workspace\": \"/work/notes\""), "{raw}");
        assert!(!dir.join("shell.json.tmp").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn junk_or_blank_model_falls_back() {
        let dir = temp_dir("junk");
        std::fs::create_dir_all(&dir).expect("dir");
        std::fs::write(path(&dir), "{not json").expect("write");
        assert_eq!(load(&dir).model, "grok-4.7");
        std::fs::write(path(&dir), r#"{"model":"  ","closeToTray":false}"#).expect("write");
        assert_eq!(
            load(&dir),
            ShellSettings {
                model: "grok-4.7".into(),
                close_to_tray: false,
                workspace: String::new(),
            }
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn blank_model_is_refused() {
        let dir = temp_dir("blank");
        let blank = ShellSettings {
            model: " ".into(),
            close_to_tray: true,
            workspace: String::new(),
        };
        assert_eq!(save(&dir, &blank), Err("Pick a model.".to_string()));
        assert!(!path(&dir).exists());
    }
}
