//! `{config}/models/`: `registry.json`, `health.jsonl` (passive calls),
//! `model_profiles/<id>.json`, `onboarding.json`, `probe_log.jsonl` and
//! `quota.json` (detected account state, see [`super::quota`]).
//! Paths are built with `std::path`, so Windows (`%APPDATA%\GrokHub`) and
//! Linux (`~/.config/GrokHub`) read the same layout.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;
use serde::Serialize;

use super::health::Observation;
use super::Registry;

pub const MODELS_DIR: &str = "models";
pub const REGISTRY_FILE: &str = "registry.json";
pub const HEALTH_FILE: &str = "health.jsonl";
pub const PROFILES_DIR: &str = "model_profiles";
pub const ONBOARDING_FILE: &str = "onboarding.json";
pub const PROBE_LOG_FILE: &str = "probe_log.jsonl";
pub const QUOTA_FILE: &str = "quota.json";

pub fn models_dir(config_dir: &Path) -> PathBuf {
    config_dir.join(MODELS_DIR)
}

pub fn registry_path(config_dir: &Path) -> PathBuf {
    models_dir(config_dir).join(REGISTRY_FILE)
}

pub fn health_path(config_dir: &Path) -> PathBuf {
    models_dir(config_dir).join(HEALTH_FILE)
}

pub fn profiles_dir(config_dir: &Path) -> PathBuf {
    models_dir(config_dir).join(PROFILES_DIR)
}

pub fn onboarding_path(config_dir: &Path) -> PathBuf {
    models_dir(config_dir).join(ONBOARDING_FILE)
}

pub fn probe_log_path(config_dir: &Path) -> PathBuf {
    models_dir(config_dir).join(PROBE_LOG_FILE)
}

pub fn quota_path(config_dir: &Path) -> PathBuf {
    models_dir(config_dir).join(QUOTA_FILE)
}

/// A model id as a file name on every OS: `/ : * ? " < > |` (and `\`) become `_`.
pub fn safe_file_stem(id: &str) -> String {
    id.trim()
        .chars()
        .map(|c| {
            if matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') || c.is_control() {
                '_'
            } else {
                c
            }
        })
        .collect()
}

/// Write `value` as pretty JSON through a temp file and a rename.
pub fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let text = serde_json::to_string_pretty(value).map_err(|e| e.to_string())?;
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, text.as_bytes()).map_err(|e| e.to_string())?;
    fs::rename(&tmp, path).map_err(|e| e.to_string())
}

pub fn read_json<T: DeserializeOwned>(path: &Path) -> Option<T> {
    let text = fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

/// Append one JSON line.
pub fn append_line<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let mut line = serde_json::to_string(value).map_err(|e| e.to_string())?;
    line.push('\n');
    let mut f = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|e| e.to_string())?;
    f.write_all(line.as_bytes()).map_err(|e| e.to_string())
}

/// The saved registry, or an empty one (a bad file is never fatal).
pub fn load_registry(config_dir: &Path) -> Registry {
    // The user's profile overrides ride with the registry: every refresh
    // that follows a load fills from them.
    super::builtin::install_overrides(config_dir);
    read_json(&registry_path(config_dir)).unwrap_or_default()
}

/// The saved quota ledger, or an empty one (a bad file is never fatal).
pub fn load_quota(config_dir: &Path) -> super::quota::QuotaLedger {
    read_json(&quota_path(config_dir)).unwrap_or_default()
}

pub fn save_quota(config_dir: &Path, quota: &super::quota::QuotaLedger) -> Result<(), String> {
    write_json(&quota_path(config_dir), quota)
}

pub fn save_registry(config_dir: &Path, reg: &Registry) -> Result<(), String> {
    write_json(&registry_path(config_dir), reg)
}

/// Log one real call for the next health fold.
pub fn append_observation(config_dir: &Path, obs: &Observation) -> Result<(), String> {
    append_line(&health_path(config_dir), obs)
}

/// Take every logged call and empty the file. A bad line is skipped.
pub fn take_observations(config_dir: &Path) -> Vec<Observation> {
    let path = health_path(config_dir);
    let Ok(text) = fs::read_to_string(&path) else {
        return Vec::new();
    };
    let _ = fs::write(&path, b"");
    text.lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_become_windows_safe_file_names() {
        assert_eq!(safe_file_stem("grok-4.7"), "grok-4.7");
        assert_eq!(
            safe_file_stem(r#"x/a:b*c?d"e<f>g|h\i"#),
            "x_a_b_c_d_e_f_g_h_i"
        );
    }

    #[test]
    fn paths_are_under_config_models() {
        let base = Path::new("cfg");
        assert_eq!(
            registry_path(base),
            base.join("models").join("registry.json")
        );
        assert_eq!(
            profiles_dir(base),
            base.join("models").join("model_profiles")
        );
        assert_eq!(
            probe_log_path(base),
            base.join("models").join("probe_log.jsonl")
        );
        assert_eq!(quota_path(base), base.join("models").join("quota.json"));
    }

    #[test]
    fn quota_round_trips_through_the_store() {
        let dir = std::env::temp_dir().join(format!("grokhub-quota-store-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let ledger = super::super::quota::QuotaLedger {
            openrouter: super::super::quota::OpenRouterQuota {
                state: super::super::quota::OpenRouterState::FreeOnly,
                free_daily_limit: Some(50),
                ..Default::default()
            },
            ..Default::default()
        };
        save_quota(&dir, &ledger).unwrap();
        assert_eq!(load_quota(&dir), ledger);
        // A missing file reads as an empty ledger, never an error.
        let _ = fs::remove_dir_all(&dir);
        assert_eq!(
            load_quota(&dir),
            super::super::quota::QuotaLedger::default()
        );
    }
}
