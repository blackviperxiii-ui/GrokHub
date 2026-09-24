//! Home update feed store. `updates.json` next to `automations.json`. Not AppConfig.

use grokhub_core::UpdateCard;

use crate::config;

pub fn path() -> std::path::PathBuf {
    config::config_dir().join("updates.json")
}

pub fn load() -> Vec<UpdateCard> {
    config::load_json(&path(), config::JSON_STORE_CAP)
}

pub fn save(list: &[UpdateCard]) -> Result<(), String> {
    let s = serde_json::to_string_pretty(list).map_err(|e| e.to_string())?;
    config::atomic_write(&path(), s.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use grokhub_core::{automation_done_card, feed_visible};
    use std::fs;

    #[test]
    fn updates_roundtrip_and_boot_does_not_slurp() {
        let _g = crate::config::hold_test_config();
        let root = crate::config::test_config_root("updates");
        let _ = fs::remove_dir_all(&root);
        std::env::set_var("GROKHUB_CONFIG", &root);
        let card = automation_done_card("loop-1", "Board", "done", 42);
        save(std::slice::from_ref(&card)).expect("save");
        let loaded = load();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].id, card.id);
        assert!(feed_visible(&loaded));
        let src = include_str!("feed.rs");
        let code = src.split("#[cfg(test)]").next().expect("feed");
        assert!(
            code.contains("load_json") && !code.contains("read_to_string"),
            "boot must not slurp unbounded updates JSON: {code}"
        );
        let _ = fs::remove_dir_all(&root);
        std::env::remove_var("GROKHUB_CONFIG");
    }
}
