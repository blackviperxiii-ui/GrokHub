//! Cabin glue for modular Home / Mission Control.
//!
//! Catalog, slot grid, and `HomeLayout` schema live in `grokhub_core::home`.
//! This module loads/saves `{config}/home_layout.json` and paints Home.

mod edit;
mod live;
mod page;
mod paint;
mod pc_sheet;
#[cfg(test)]
mod tile_tests;
mod updates;

use std::path::{Path, PathBuf};

use grokhub_core::home::{HomeLayout, HOME_LAYOUT_FILE, HOME_LAYOUT_VERSION};

use crate::config::{atomic_write, load_json_or, peek_json, JSON_STORE_CAP};

pub(in crate::app) use edit::HomeEditDraft;

pub fn home_layout_path(dir: &Path) -> PathBuf {
    dir.join(HOME_LAYOUT_FILE)
}

/// Load layout from `{dir}/home_layout.json`. Missing → default. Bad → default
/// after quarantine. v1 files migrate; unknown ids sanitize to defaults.
pub fn load_home_layout(dir: &Path) -> HomeLayout {
    let path = home_layout_path(dir);
    match peek_json::<serde_json::Value>(&path, JSON_STORE_CAP) {
        None => load_json_or(&path, JSON_STORE_CAP, HomeLayout::default).sanitize(),
        Some(v) => {
            let ver = v.get("version").and_then(|x| x.as_u64()).unwrap_or(1) as u32;
            if ver < HOME_LAYOUT_VERSION {
                HomeLayout::from_v1_value(v)
            } else {
                match serde_json::from_value::<HomeLayout>(v) {
                    Ok(layout) => layout.sanitize(),
                    Err(_) => load_json_or(&path, JSON_STORE_CAP, HomeLayout::default).sanitize(),
                }
            }
        }
    }
}

pub fn save_home_layout(dir: &Path, layout: &HomeLayout) -> Result<(), String> {
    let mut to_save = layout.clone().sanitize();
    to_save.version = HOME_LAYOUT_VERSION;
    let s = serde_json::to_string_pretty(&to_save).map_err(|e| e.to_string())?;
    atomic_write(&home_layout_path(dir), s.as_bytes())
}

/// Load the layout, writing the default file once when none exists yet.
pub fn ensure_home_layout(dir: &Path) -> HomeLayout {
    let path = home_layout_path(dir);
    if !path.exists() {
        let layout = HomeLayout::default();
        let _ = save_home_layout(dir, &layout);
        return layout;
    }
    load_home_layout(dir)
}

#[cfg(test)]
mod tests {
    use super::*;
    use grokhub_core::home::{place_slots, HomePreset, REGISTRY};
    use std::fs;

    #[test]
    fn round_trip_on_disk() {
        let dir = std::env::temp_dir().join(format!(
            "grokhub-home-layout-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        let layout = HomeLayout::preset(HomePreset::Performance);
        save_home_layout(&dir, &layout).expect("save");
        let loaded = load_home_layout(&dir);
        assert_eq!(loaded.large, "pc_loadout");
        assert_eq!(loaded.preset, "performance");
        assert_eq!(loaded.version, 2);
        assert_eq!(REGISTRY.len(), 36);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn ensure_writes_default_when_missing() {
        let dir = std::env::temp_dir().join(format!(
            "grokhub-home-layout-ensure-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        let loaded = ensure_home_layout(&dir);
        assert_eq!(loaded, HomeLayout::default());
        assert!(home_layout_path(&dir).is_file());
        let placed = place_slots((0.0, 0.0, 1600.0, 470.0), &loaded);
        // Mission Control / slide 09: Objectives | PC Loadout | Up Next.
        assert_eq!(placed.len(), 3);
        let _ = fs::remove_dir_all(&dir);
    }
}
