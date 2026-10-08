//! `{config}/models/`: `registry.json`, `health.jsonl` (passive calls),
//! `model_profiles/<id>.json`, `onboarding.json` and `probe_log.jsonl`.
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
/// `health.jsonl` is folded and cut at this size so it can't grow forever.
pub const HEALTH_CAP_BYTES: u64 = 1024 * 1024;

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

/// A model id as a file name on every OS: `/ : * ? " < > |` (and `\`) become `_`.
pub fn safe_file_stem(id: &str) -> String {
    id.trim()
        .chars()
        .map(|c| if matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') || c.is_control() { '_' } else { c })
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
    let mut f = OpenOptions::new().create(true).append(true).open(path).map_err(|e| e.to_string())?;
    f.write_all(line.as_bytes()).map_err(|e| e.to_string())
}

/// The saved registry, or an empty one (a bad file is never fatal).
pub fn load_registry(config_dir: &Path) -> Registry {
    read_json(&registry_path(config_dir)).unwrap_or_default()
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
    text.lines().filter_map(|l| serde_json::from_str(l).ok()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_become_windows_safe_file_names() {
        assert_eq!(safe_file_stem("grok-4.7"), "grok-4.7");
        assert_eq!(safe_file_stem(r#"x/a:b*c?d"e<f>g|h\i"#), "x_a_b_c_d_e_f_g_h_i");
    }

    #[test]
    fn paths_are_under_config_models() {
        let base = Path::new("cfg");
        assert_eq!(registry_path(base), base.join("models").join("registry.json"));
        assert_eq!(profiles_dir(base), base.join("models").join("model_profiles"));
        assert_eq!(probe_log_path(base), base.join("models").join("probe_log.jsonl"));
    }
}
