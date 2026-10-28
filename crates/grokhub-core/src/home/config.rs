//! Home layout schema (`home_layout.json` v2). Load/save lives in the cabin.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use super::registry::lookup;

pub const HOME_LAYOUT_FILE: &str = "home_layout.json";
pub const HOME_LAYOUT_VERSION: u32 = 2;

/// Named presets offered in Edit Home.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HomePreset {
    MissionControl,
    Streamer,
    Performance,
}

impl HomePreset {
    pub fn label(self) -> &'static str {
        match self {
            HomePreset::MissionControl => "Mission Control",
            HomePreset::Streamer => "Streamer/Social",
            HomePreset::Performance => "Performance/Tuning",
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            HomePreset::MissionControl => "mission_control",
            HomePreset::Streamer => "streamer",
            HomePreset::Performance => "performance",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.trim() {
            "mission_control" | "default" => Some(HomePreset::MissionControl),
            "streamer" | "streamer_social" => Some(HomePreset::Streamer),
            "performance" | "performance_tuning" => Some(HomePreset::Performance),
            _ => None,
        }
    }
}

/// On-disk Home layout (v2). Unknown module ids fall back to the default slot.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HomeLayout {
    pub version: u32,
    #[serde(default = "default_preset_str")]
    pub preset: String,
    pub large: String,
    /// Exactly two entries preferred; shorter is fine (remaining slots empty).
    pub medium: Vec<String>,
    /// Exactly four; `null` = hidden.
    pub small: [Option<String>; 4],
    #[serde(default = "default_true")]
    pub xp_bar: bool,
    #[serde(default)]
    pub module_cfg: BTreeMap<String, serde_json::Value>,
}

fn default_preset_str() -> String {
    HomePreset::MissionControl.id().into()
}

fn default_true() -> bool {
    true
}

impl Default for HomeLayout {
    fn default() -> Self {
        Self::preset(HomePreset::MissionControl)
    }
}

impl HomeLayout {
    pub fn preset(kind: HomePreset) -> Self {
        match kind {
            // Deck slide 09: Objectives (+ streak) | PC Loadout | Up Next covers.
            HomePreset::MissionControl => Self {
                version: HOME_LAYOUT_VERSION,
                preset: kind.id().into(),
                large: "objectives".into(),
                medium: vec!["pc_loadout".into()],
                small: [Some("up_next".into()), None, None, None],
                xp_bar: true,
                module_cfg: BTreeMap::new(),
            },
            HomePreset::Streamer => Self {
                version: HOME_LAYOUT_VERSION,
                preset: kind.id().into(),
                large: "clips".into(),
                medium: vec!["squad".into(), "pulse".into()],
                small: [
                    Some("stream_check".into()),
                    Some("speeds".into()),
                    Some("connection".into()),
                    Some("daily_drop".into()),
                ],
                xp_bar: true,
                module_cfg: BTreeMap::new(),
            },
            HomePreset::Performance => Self {
                version: HOME_LAYOUT_VERSION,
                preset: kind.id().into(),
                large: "pc_loadout".into(),
                medium: vec!["speeds".into(), "game_mode".into()],
                small: [
                    Some("frame_rate".into()),
                    Some("storage".into()),
                    Some("usage".into()),
                    Some("updates".into()),
                ],
                xp_bar: true,
                module_cfg: BTreeMap::new(),
            },
        }
    }

    /// Replace unknown / removed ids with the matching default-slot module.
    pub fn sanitize(mut self) -> Self {
        let fallback = HomeLayout::default();
        if lookup(&self.large).is_none() {
            self.large = fallback.large.clone();
        }
        while self.medium.len() < 2 {
            self.medium.push(String::new());
        }
        self.medium.truncate(2);
        for (i, slot) in self.medium.iter_mut().enumerate() {
            if slot.is_empty() {
                continue;
            }
            if lookup(slot).is_none() {
                *slot = fallback.medium.get(i).cloned().unwrap_or_default();
            }
        }
        for (i, slot) in self.small.iter_mut().enumerate() {
            if let Some(id) = slot.as_ref() {
                if lookup(id).is_none() {
                    *slot = fallback.small[i].clone();
                }
            }
        }
        if self.version == 0 {
            self.version = HOME_LAYOUT_VERSION;
        }
        self
    }

    /// Migrate a v1-shaped file: keep large/medium, first 4 smalls, pad with defaults.
    pub fn from_v1_value(v: serde_json::Value) -> Self {
        let mut out = HomeLayout::default();
        if let Some(s) = v.get("large").and_then(|x| x.as_str()) {
            out.large = s.to_string();
        }
        if let Some(arr) = v.get("medium").and_then(|x| x.as_array()) {
            out.medium = arr
                .iter()
                .filter_map(|x| x.as_str().map(str::to_string))
                .take(2)
                .collect();
        }
        if let Some(arr) = v.get("small").and_then(|x| x.as_array()) {
            for (i, item) in arr.iter().take(4).enumerate() {
                out.small[i] = match item {
                    serde_json::Value::Null => None,
                    serde_json::Value::String(s) if s.is_empty() => None,
                    serde_json::Value::String(s) => Some(s.clone()),
                    _ => out.small[i].clone(),
                };
            }
        }
        if let Some(p) = v.get("preset").and_then(|x| x.as_str()) {
            out.preset = p.to_string();
        }
        if let Some(b) = v.get("xp_bar").and_then(|x| x.as_bool()) {
            out.xp_bar = b;
        }
        out.version = HOME_LAYOUT_VERSION;
        out.sanitize()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_matches_mission_control_spec() {
        let d = HomeLayout::default();
        assert_eq!(d.version, 2);
        assert_eq!(d.large, "objectives");
        assert_eq!(d.medium, vec!["pc_loadout".to_string()]);
        assert_eq!(d.small, [Some("up_next".into()), None, None, None,]);
        assert!(d.xp_bar);
        assert_eq!(d.preset, "mission_control");
        assert_eq!(HomePreset::MissionControl.label(), "Mission Control");
        assert_eq!(HomePreset::parse("streamer"), Some(HomePreset::Streamer));
    }

    #[test]
    fn presets_match_spec_slots() {
        let s = HomeLayout::preset(HomePreset::Streamer);
        assert_eq!(s.large, "clips");
        assert_eq!(s.medium[0], "squad");
        assert_eq!(s.small[0].as_deref(), Some("stream_check"));
        let p = HomeLayout::preset(HomePreset::Performance);
        assert_eq!(p.large, "pc_loadout");
        assert_eq!(p.medium[1], "game_mode");
        assert_eq!(p.small[0].as_deref(), Some("frame_rate"));
    }

    #[test]
    fn sanitize_replaces_unknown_ids() {
        let bad = HomeLayout {
            large: "not_a_module".into(),
            small: [
                Some("pc_loadout".into()),
                Some("speeds".into()),
                Some("gone".into()),
                Some("streak".into()),
            ],
            ..HomeLayout::default()
        };
        let fixed = bad.sanitize();
        assert_eq!(fixed.large, "objectives");
        // Mission Control hides S3; unknown ids fall back to the preset slot (None).
        assert_eq!(fixed.small[2], None);
    }

    #[test]
    fn v1_migrates_to_v2_keeping_first_four_smalls() {
        let v = serde_json::json!({
            "version": 1,
            "preset": "mission_control",
            "large": "objectives",
            "medium": ["up_next", "pulse"],
            "small": ["pc_loadout", "speeds", "usage", "streak", "extra_ignored"],
            "xp_bar": false
        });
        let layout = HomeLayout::from_v1_value(v);
        assert_eq!(layout.version, 2);
        assert!(!layout.xp_bar);
        assert_eq!(layout.small[3].as_deref(), Some("streak"));
    }
}
