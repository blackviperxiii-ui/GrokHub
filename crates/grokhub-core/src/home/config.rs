//! Home layout schema (`home_layout.json` v2). Load/save lives in the cabin.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use super::layout::ModuleSize;
use super::registry::{lookup, REGISTRY};

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

/// Every preset, in the order Edit Home offers them.
pub const HOME_PRESETS: [HomePreset; 3] = [
    HomePreset::MissionControl,
    HomePreset::Streamer,
    HomePreset::Performance,
];

/// What Edit Home picked: the module in each slot. An empty or missing
/// Medium or Small slot is hidden; Large always shows something.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HomePick {
    #[serde(default)]
    pub preset: String,
    #[serde(default)]
    pub large: String,
    #[serde(default)]
    pub medium: Vec<String>,
    #[serde(default)]
    pub small: Vec<Option<String>>,
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

    /// The slots as Edit Home shows them.
    pub fn pick(&self) -> HomePick {
        HomePick {
            preset: self.preset.clone(),
            large: self.large.clone(),
            medium: (0..2)
                .map(|i| self.medium.get(i).cloned().unwrap_or_default())
                .collect(),
            small: self.small.to_vec(),
        }
    }

    /// This layout with `pick`'s slots. A module shows once (its first slot
    /// wins), one in a size it can't draw falls back like any bad id, and
    /// the XP bar and module settings are kept. The preset name stays only
    /// while the slots still match that preset.
    pub fn with_pick(mut self, pick: &HomePick) -> Self {
        let mut seen: Vec<String> = Vec::new();
        let mut take = |id: Option<&str>| -> Option<String> {
            let id = id.map(str::trim).filter(|s| !s.is_empty())?;
            if seen.iter().any(|s| s == id) {
                return None;
            }
            seen.push(id.to_string());
            Some(id.to_string())
        };
        self.large = take(Some(&pick.large)).unwrap_or_default();
        self.medium = (0..2)
            .map(|i| take(pick.medium.get(i).map(String::as_str)).unwrap_or_default())
            .collect();
        for (i, slot) in self.small.iter_mut().enumerate() {
            *slot = take(pick.small.get(i).and_then(|s| s.as_deref()));
        }
        self.version = HOME_LAYOUT_VERSION;
        let mut out = self.sanitize();
        let named = HomePreset::parse(&pick.preset)
            .filter(|kind| HomeLayout::preset(*kind).pick().slots_eq(&out.pick()));
        out.preset = named.map_or("custom".into(), |kind| kind.id().into());
        out
    }

    /// Replace unknown or removed ids, and a module placed in a size it
    /// doesn't draw at, with the matching default-slot module.
    pub fn sanitize(mut self) -> Self {
        let fits = |id: &str, size: ModuleSize| lookup(id).is_some_and(|m| m.supports(size));
        let fallback = HomeLayout::default();
        if !fits(&self.large, ModuleSize::Large) {
            // Prefer the default Large, but never one already shown in
            // another slot, so a tile doesn't appear twice.
            let used = |id: &str| {
                self.medium.iter().any(|m| m == id) || self.small.iter().flatten().any(|s| s == id)
            };
            self.large = std::iter::once(fallback.large.as_str())
                .chain(REGISTRY.iter().map(|m| m.id))
                .find(|id| fits(id, ModuleSize::Large) && !used(id))
                .unwrap_or(fallback.large.as_str())
                .to_string();
        }
        while self.medium.len() < 2 {
            self.medium.push(String::new());
        }
        self.medium.truncate(2);
        for (i, slot) in self.medium.iter_mut().enumerate() {
            if slot.is_empty() {
                continue;
            }
            if !fits(slot, ModuleSize::Medium) {
                *slot = fallback.medium.get(i).cloned().unwrap_or_default();
            }
        }
        for (i, slot) in self.small.iter_mut().enumerate() {
            if let Some(id) = slot.as_ref() {
                if !fits(id, ModuleSize::Small) {
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

impl HomePick {
    fn slots_eq(&self, other: &HomePick) -> bool {
        let med = |p: &HomePick| -> Vec<String> {
            (0..2)
                .map(|i| p.medium.get(i).cloned().unwrap_or_default())
                .collect()
        };
        let small = |p: &HomePick| -> Vec<Option<String>> {
            (0..4).map(|i| p.small.get(i).cloned().flatten()).collect()
        };
        self.large == other.large && med(self) == med(other) && small(self) == small(other)
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
    fn a_missing_large_never_doubles_a_tile_shown_elsewhere() {
        let default_large = HomeLayout::default().large;
        let bad = HomeLayout {
            large: String::new(),
            medium: vec![default_large.clone(), String::new()],
            ..HomeLayout::default()
        };
        let fixed = bad.sanitize();
        assert_ne!(fixed.large, default_large);
        assert!(lookup(&fixed.large).is_some_and(|m| m.supports(ModuleSize::Large)));
    }

    #[test]
    fn sanitize_moves_a_module_out_of_a_size_it_cannot_draw() {
        let small_only = super::super::REGISTRY
            .iter()
            .find(|m| !m.supports(ModuleSize::Large))
            .expect("some module has no Large look");
        let bad = HomeLayout {
            large: small_only.id.into(),
            ..HomeLayout::default()
        };
        assert_eq!(bad.sanitize().large, HomeLayout::default().large);
    }

    #[test]
    fn every_preset_survives_sanitize_unchanged() {
        for kind in [
            HomePreset::MissionControl,
            HomePreset::Streamer,
            HomePreset::Performance,
        ] {
            let p = HomeLayout::preset(kind);
            let s = p.clone().sanitize();
            let filled = |m: &[String]| {
                m.iter()
                    .filter(|x| !x.is_empty())
                    .cloned()
                    .collect::<Vec<_>>()
            };
            assert_eq!(s.large, p.large, "{kind:?}");
            assert_eq!(filled(&s.medium), filled(&p.medium), "{kind:?}");
            assert_eq!(s.small, p.small, "{kind:?}");
        }
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

    #[test]
    fn a_pick_keeps_each_module_once_and_names_the_preset_only_while_it_matches() {
        let base = HomeLayout {
            xp_bar: false,
            ..HomeLayout::default()
        };
        let streamer = HomeLayout::preset(HomePreset::Streamer).pick();
        let same = base.clone().with_pick(&streamer);
        assert_eq!(same.preset, "streamer");
        assert_eq!(same.large, "clips");
        assert!(!same.xp_bar, "the XP bar setting is kept");

        let mut changed = streamer.clone();
        changed.small[3] = None;
        let custom = base.clone().with_pick(&changed);
        assert_eq!(custom.preset, "custom");
        assert_eq!(custom.small[3], None);

        let twice = HomePick {
            preset: "custom".into(),
            large: "pc_loadout".into(),
            medium: vec!["speeds".into(), "speeds".into()],
            small: vec![Some("speeds".into()), Some("  ".into())],
        };
        let once = base.with_pick(&twice);
        assert_eq!(once.medium, vec!["speeds".to_string(), String::new()]);
        assert_eq!(once.small, [None, None, None, None]);
        assert_eq!(once.version, HOME_LAYOUT_VERSION);
    }
}
