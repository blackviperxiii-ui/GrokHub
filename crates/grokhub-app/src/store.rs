use grokhub_core::{empty_chip_memory, ChipMemory, LearningState, UsageDay};
use std::fs;

use crate::config;

pub fn learning_path() -> std::path::PathBuf {
    config::config_dir().join("learning.json")
}

pub fn load_learning() -> LearningState {
    let raw = fs::read_to_string(learning_path()).unwrap_or_default();
    serde_json::from_str(&raw).unwrap_or_default()
}

pub fn save_learning(s: &LearningState) -> Result<(), String> {
    fs::create_dir_all(config::config_dir()).map_err(|e| e.to_string())?;
    fs::write(learning_path(), serde_json::to_string_pretty(s).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())
}

pub fn usage_path() -> std::path::PathBuf {
    config::config_dir().join("usage.json")
}

pub fn load_usage() -> UsageDay {
    let raw = fs::read_to_string(usage_path()).unwrap_or_default();
    serde_json::from_str(&raw).unwrap_or_default()
}

pub fn save_usage(d: &UsageDay) -> Result<(), String> {
    fs::create_dir_all(config::config_dir()).map_err(|e| e.to_string())?;
    fs::write(usage_path(), serde_json::to_string_pretty(d).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())
}

pub fn chips_path() -> std::path::PathBuf {
    config::config_dir().join("chips.json")
}

pub fn load_chips() -> ChipMemory {
    let raw = fs::read_to_string(chips_path()).unwrap_or_default();
    serde_json::from_str(&raw).unwrap_or_else(|_| empty_chip_memory())
}

pub fn save_chips(s: &ChipMemory) -> Result<(), String> {
    fs::create_dir_all(config::config_dir()).map_err(|e| e.to_string())?;
    fs::write(chips_path(), serde_json::to_string_pretty(s).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::TEST_CONFIG_LOCK;

    #[test]
    fn learning_roundtrip() {
        let _g = TEST_CONFIG_LOCK.lock().unwrap();
        let root = std::env::temp_dir().join(format!("grokhub-learn-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        std::env::set_var("GROKHUB_CONFIG", &root);
        let mut s = LearningState::default();
        grokhub_core::upsert_insight(&mut s, "pref", "prefer nvim always");
        save_learning(&s).expect("save");
        assert_eq!(load_learning().insights[0].text, "prefer nvim always");
        let _ = fs::remove_dir_all(&root);
        std::env::remove_var("GROKHUB_CONFIG");
    }

    #[test]
    fn chips_roundtrip() {
        let _g = TEST_CONFIG_LOCK.lock().unwrap();
        let root = std::env::temp_dir().join(format!("grokhub-chips-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        std::env::set_var("GROKHUB_CONFIG", &root);
        let mut s = empty_chip_memory();
        s.total_events = 3;
        save_chips(&s).expect("save");
        assert_eq!(load_chips().total_events, 3);
        let _ = fs::remove_dir_all(&root);
        std::env::remove_var("GROKHUB_CONFIG");
    }
}
