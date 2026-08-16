use grokhub_core::{recipe_from_json, recipe_to_json, Recipe};
use std::fs;

use crate::config;

pub fn dir() -> std::path::PathBuf {
    config::config_dir().join("recipes")
}

pub fn last_path() -> std::path::PathBuf {
    dir().join("last.json")
}

pub fn path_for(id: &str) -> std::path::PathBuf {
    let safe: String = id
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect();
    dir().join(format!("{safe}.json"))
}

pub fn save_recipe(id: &str, recipe: &Recipe) -> Result<std::path::PathBuf, String> {
    fs::create_dir_all(dir()).map_err(|e| e.to_string())?;
    let json = recipe_to_json(id, recipe)?;
    let dest = path_for(id);
    config::atomic_write(&dest, json.as_bytes())?;
    if id != "last" {
        config::atomic_write(&last_path(), json.as_bytes())?;
    }
    Ok(dest)
}

pub fn load_recipe(id: &str) -> Option<Recipe> {
    let raw = fs::read_to_string(path_for(id)).ok()?;
    recipe_from_json(&raw).ok().map(|(_, r)| r)
}

pub fn load_last() -> Option<Recipe> {
    load_recipe("last").or_else(|| {
        let raw = fs::read_to_string(last_path()).ok()?;
        recipe_from_json(&raw).ok().map(|(_, r)| r)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::TEST_CONFIG_LOCK;
    use grokhub_core::{ComputerOp, ScreenSize};

    #[test]
    fn recipe_disk_roundtrip() {
        let _g = TEST_CONFIG_LOCK.lock().unwrap();
        let root = std::env::temp_dir().join(format!("grokhub-recipes-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        std::env::set_var("GROKHUB_CONFIG", &root);
        let recipe = Recipe {
            screen: Some(ScreenSize { w: 1920, h: 1080 }),
            ops: vec![ComputerOp::Act { name: "Save".into() }],
        };
        save_recipe("desk-1", &recipe).expect("save");
        assert_eq!(load_recipe("desk-1").unwrap(), recipe);
        assert_eq!(load_last().unwrap(), recipe);
        let _ = fs::remove_dir_all(&root);
        std::env::remove_var("GROKHUB_CONFIG");
    }
}
