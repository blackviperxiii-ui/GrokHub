use grokhub_core::{uid, GrokLoop};

use crate::config;

pub fn path() -> std::path::PathBuf {
    config::config_dir().join("loops.json")
}

pub fn load() -> Vec<GrokLoop> {
    let mut list: Vec<GrokLoop> = config::load_json(&path(), config::JSON_STORE_CAP);
    for a in &mut list {
        if a.id.is_empty() {
            a.id = uid("loop");
        }
    }
    list
}

pub fn save(list: &[GrokLoop]) -> Result<(), String> {
    let s = serde_json::to_string_pretty(list).map_err(|e| e.to_string())?;
    config::atomic_write(&path(), s.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn loop_roundtrip() {
        let _g = crate::config::hold_test_config();
        let root = crate::config::test_config_root("loops");
        let _ = fs::remove_dir_all(&root);
        std::env::set_var("GROKHUB_CONFIG", &root);
        let rows = vec![GrokLoop {
            id: "loop-1".into(),
            interval: "30m".into(),
            prompt: "check deploy".into(),
            enabled: true,
            created_ms: 1,
            last_run: None,
            next_run: Some(2),
            run_count: 0,
            session_id: None,
        }];
        save(&rows).unwrap();
        let loaded = load();
        let _ = fs::remove_dir_all(&root);
        std::env::remove_var("GROKHUB_CONFIG");
        assert_eq!(loaded, rows);
    }
}
