use grokhub_core::{uid, Automation};
use std::fs;

use crate::config;

pub fn path() -> std::path::PathBuf {
    config::config_dir().join("automations.json")
}

pub fn load() -> Vec<Automation> {
    let mut list: Vec<Automation> = config::load_json(&path(), config::JSON_STORE_CAP);
    for a in &mut list {
        if a.id.is_empty() {
            a.id = uid("auto");
        }
    }
    list
}

pub fn save(list: &[Automation]) -> Result<(), String> {
    let s = serde_json::to_string_pretty(list).map_err(|e| e.to_string())?;
    config::atomic_write(&path(), s.as_bytes())
}

pub fn rewind_index_path() -> std::path::PathBuf {
    config::config_dir().join("rewind.json")
}

pub fn load_rewinds() -> Vec<grokhub_core::RewindRecord> {
    config::load_json(&rewind_index_path(), config::JSON_STORE_CAP)
}

pub fn save_rewinds(rows: &[grokhub_core::RewindRecord]) -> Result<(), String> {
    let s = serde_json::to_string_pretty(rows).map_err(|e| e.to_string())?;
    config::atomic_write(&rewind_index_path(), s.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::TEST_CONFIG_LOCK;

    #[test]
    fn automation_roundtrip() {
        let _g = TEST_CONFIG_LOCK.lock().unwrap();
        let root = std::env::temp_dir().join(format!("grokhub-night-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        std::env::set_var("GROKHUB_CONFIG", &root);
        let a = Automation {
            id: "auto-1".into(),
            name: "board".into(),
            schedule: "weekdays".into(),
            time: "09:00".into(),
            times: vec![],
            instructions: "summarize the workboard".into(),
            heartbeat_every_min: 0,
            check_command: String::new(),
            enabled: true,
            last_run: None,
            next_run: None,
            run_count: 0,
        };
        save(&[a]).expect("save");
        let loaded = load();
        assert_eq!(loaded[0].schedule, "weekdays");
        let _ = fs::remove_dir_all(&root);
        std::env::remove_var("GROKHUB_CONFIG");
    }

    #[test]
    fn night_loads_do_not_slurp_huge_files() {
        let src = include_str!("night.rs");
        let code = src.split("#[cfg(test)]").next().expect("night");
        assert!(
            code.contains("load_json") && !code.contains("read_to_string"),
            "boot must not slurp unbounded automations/rewind JSON: {code}"
        );
        assert!(
            !code.contains("MEMORY_FILE_CAP"),
            "a capped read severs JSON mid-token, so the loader returns the default and \
             the next persist writes it back over the store: {code}"
        );
    }
}
