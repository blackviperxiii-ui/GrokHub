use grokhub_core::{
    empty_chip_memory, ChipMemory, ImagineWall, LearningState, ProjectNode, SuggestionStore, UsageDay,
};
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
    let body = serde_json::to_string_pretty(s).map_err(|e| e.to_string())?;
    config::atomic_write(&learning_path(), body.as_bytes())
}

pub fn usage_path() -> std::path::PathBuf {
    config::config_dir().join("usage.json")
}

pub fn load_usage() -> UsageDay {
    let raw = fs::read_to_string(usage_path()).unwrap_or_default();
    serde_json::from_str(&raw).unwrap_or_default()
}

pub fn save_usage(d: &UsageDay) -> Result<(), String> {
    let body = serde_json::to_string_pretty(d).map_err(|e| e.to_string())?;
    config::atomic_write(&usage_path(), body.as_bytes())
}

pub fn chips_path() -> std::path::PathBuf {
    config::config_dir().join("chips.json")
}

pub fn load_chips() -> ChipMemory {
    let raw = fs::read_to_string(chips_path()).unwrap_or_default();
    serde_json::from_str(&raw).unwrap_or_else(|_| empty_chip_memory())
}

pub fn wall_path() -> std::path::PathBuf {
    config::config_dir().join("imagine-wall.json")
}

pub fn load_wall() -> ImagineWall {
    let raw = fs::read_to_string(wall_path()).unwrap_or_default();
    serde_json::from_str(&raw).unwrap_or_default()
}

pub fn save_wall(w: &ImagineWall) -> Result<(), String> {
    let body = serde_json::to_string_pretty(w).map_err(|e| e.to_string())?;
    config::atomic_write(&wall_path(), body.as_bytes())
}

pub fn projects_path() -> std::path::PathBuf {
    config::config_dir().join("projects.json")
}

pub fn load_projects() -> Vec<ProjectNode> {
    let raw = fs::read_to_string(projects_path()).unwrap_or_default();
    serde_json::from_str(&raw).unwrap_or_default()
}

pub fn save_projects(nodes: &[ProjectNode]) -> Result<(), String> {
    let body = serde_json::to_string(nodes).map_err(|e| e.to_string())?;
    config::atomic_write(&projects_path(), body.as_bytes())
}

pub fn save_chips(s: &ChipMemory) -> Result<(), String> {
    let body = serde_json::to_string_pretty(s).map_err(|e| e.to_string())?;
    config::atomic_write(&chips_path(), body.as_bytes())
}

pub fn suggestions_path() -> std::path::PathBuf {
    config::config_dir().join("suggestions.json")
}

pub fn load_suggestions() -> SuggestionStore {
    let raw = fs::read_to_string(suggestions_path()).unwrap_or_default();
    serde_json::from_str(&raw).unwrap_or_default()
}

pub fn save_suggestions(s: &SuggestionStore) -> Result<(), String> {
    let body = serde_json::to_string_pretty(s).map_err(|e| e.to_string())?;
    config::atomic_write(&suggestions_path(), body.as_bytes())
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

    #[test]
    fn wall_roundtrip() {
        let _g = TEST_CONFIG_LOCK.lock().unwrap();
        let root = std::env::temp_dir().join(format!("grokhub-wall-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        std::env::set_var("GROKHUB_CONFIG", &root);
        let mut w = ImagineWall::default();
        w.last_ms = 9;
        w.gifs.push(grokhub_core::WallGif {
            id: "a1".into(),
            title: "Ember night".into(),
            prompt: "still of embers, no people, no text".into(),
            created_ms: 3,
            path_a: "a.jpg".into(),
            path_b: "b.jpg".into(),
            tall: true,
        });
        save_wall(&w).expect("save");
        let loaded = load_wall();
        assert_eq!(loaded.last_ms, 9);
        assert_eq!(loaded.gifs[0].title, "Ember night");
        let _ = fs::remove_dir_all(&root);
        std::env::remove_var("GROKHUB_CONFIG");
    }

    #[test]
    fn projects_roundtrip() {
        let _g = TEST_CONFIG_LOCK.lock().unwrap();
        let root = std::env::temp_dir().join(format!("grokhub-proj-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        std::env::set_var("GROKHUB_CONFIG", &root);
        let nodes = grokhub_core::seed_from_bound("/tmp/GrokHub-Work");
        save_projects(&nodes).expect("save");
        let loaded = load_projects();
        assert_eq!(loaded[0].name, "GrokHub-Work");
        let raw = fs::read_to_string(projects_path()).expect("raw");
        assert!(
            !raw.contains("\n  "),
            "projects.json should stay compact so the 2s persist is cheap"
        );
        save_projects(&[]).expect("empty");
        assert!(projects_path().exists());
        assert!(load_projects().is_empty());
        let _ = fs::remove_dir_all(&root);
        std::env::remove_var("GROKHUB_CONFIG");
    }

    #[test]
    fn suggestions_roundtrip() {
        let _g = TEST_CONFIG_LOCK.lock().unwrap();
        let root = std::env::temp_dir().join(format!("grokhub-suggest-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        std::env::set_var("GROKHUB_CONFIG", &root);
        let mut s = SuggestionStore::default();
        s.last_review_day = Some("2026-08-16".into());
        s.last_review_ms = 9;
        s.autos.push(grokhub_core::LearnedSuggestion {
            kind: grokhub_core::SuggestionKind::Auto,
            title: "Night wrap".into(),
            body: "Close the day".into(),
            seed: Some("every day at 21, say good night".into()),
            name: None,
            trigger: None,
            instructions: None,
            provider: None,
            tool: None,
        });
        save_suggestions(&s).expect("save");
        let loaded = load_suggestions();
        assert_eq!(loaded.last_review_day.as_deref(), Some("2026-08-16"));
        assert_eq!(loaded.autos[0].title, "Night wrap");
        let _ = fs::remove_dir_all(&root);
        std::env::remove_var("GROKHUB_CONFIG");
    }
}
