//! Home update feed store. `updates.json` next to `automations.json`. Not AppConfig.

use grokhub_core::UpdateCard;

use crate::config;

pub fn path() -> std::path::PathBuf {
    config::config_dir().join("updates.json")
}

pub fn load() -> Vec<UpdateCard> {
    let mut cards: Vec<UpdateCard> = config::load_json(&path(), config::JSON_STORE_CAP);
    if grokhub_core::collapse_feed(&mut cards) {
        let _ = save(&cards);
    }
    cards
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

    #[test]
    fn load_collapses_repeated_runs() {
        let _g = crate::config::hold_test_config();
        let root = crate::config::test_config_root("updates-collapse");
        let _ = fs::remove_dir_all(&root);
        std::env::set_var("GROKHUB_CONFIG", &root);
        let cards: Vec<_> = [1_u64, 2, 3, 4]
            .into_iter()
            .map(|ms| automation_done_card("loop-a", "Host snapshot", "report", ms))
            .collect();
        save(&cards).expect("save");
        let loaded = load();
        let visible = grokhub_core::visible_updates(&loaded);
        assert_eq!(visible.len(), 1);
        assert_eq!(visible[0].runs, 4);
        assert_eq!(loaded.iter().filter(|card| card.collapsed).count(), 3);
        let again = load();
        assert_eq!(grokhub_core::visible_updates(&again)[0].runs, 4);
        let _ = fs::remove_dir_all(&root);
        std::env::remove_var("GROKHUB_CONFIG");
    }
}
