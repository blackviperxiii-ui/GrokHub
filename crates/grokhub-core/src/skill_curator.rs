//! Skill curator: track views, uses and patches for self-made skills, mark
//! stale ones, archive (never delete), pin/lock, and a weekly Pulse report.
//! Pure rules plus an on-disk store for views/pin/lock; uses come from
//! runtime metrics and patches from ChangeLedger counts the host injects.
//! Config lives in `app.json` under `skillCurator` and is re-read each tick.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::model_registry::store::{models_dir, read_json, write_json};
use crate::organs::LocalClock;
use crate::self_review::self_review_due;

/// One day in milliseconds.
pub const DAY_MS: u64 = 86_400_000;
/// Default unused window before a skill is stale.
pub const DEFAULT_STALE_DAYS: u32 = 30;
/// On-disk curator store under `models/`.
pub const CURATOR_FILE: &str = "skill_curator.json";
pub const CURATOR_SCHEMA: u32 = 1;
/// `app.json` key for the clear off switch and stale window.
pub const SETTINGS_KEY: &str = "skillCurator";
/// Pulse digest source prefix.
pub const CARD_SOURCE_PREFIX: &str = "skill-curator:";
pub const CARD_TITLE: &str = "Skill curator";
/// Origin string for library-pack installs (not self-made).
pub const LIBRARY_ORIGIN: &str = "library-pack";

/// Lifecycle for a tracked skill.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SkillLifecycle {
    Active,
    Stale,
    Archived,
}

impl SkillLifecycle {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Stale => "stale",
            Self::Archived => "archived",
        }
    }
}

/// Curator switches from `app.json`. Re-read every pass — no restart.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillCuratorSettings {
    /// Master off switch. Default on so self-made skills get tracked.
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    /// Unused days before Active → Stale. Weekly pass archives Stale skills.
    #[serde(default = "default_stale_days")]
    pub stale_days: u32,
}

fn default_enabled() -> bool {
    true
}

fn default_stale_days() -> u32 {
    DEFAULT_STALE_DAYS
}

impl Default for SkillCuratorSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            stale_days: DEFAULT_STALE_DAYS,
        }
    }
}

impl SkillCuratorSettings {
    /// Parse from an `app.json` map value; missing or bad JSON → defaults.
    pub fn from_app_value(v: Option<&serde_json::Value>) -> Self {
        v.and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default()
    }

    pub fn stale_ms(&self) -> u64 {
        u64::from(self.stale_days.max(1)) * DAY_MS
    }
}

/// Per-skill curator flags and view count (uses live in skill_metrics).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CuratorSkillEntry {
    #[serde(default)]
    pub views: u32,
    #[serde(default)]
    pub pinned: bool,
    #[serde(default)]
    pub locked: bool,
    /// When the curator archived it (0 = not via curator / unknown).
    #[serde(default)]
    pub archived_at: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CuratorStore {
    #[serde(default)]
    pub schema: u32,
    #[serde(default)]
    pub skills: BTreeMap<String, CuratorSkillEntry>,
    /// Local day of the last weekly Pulse report (`YYYY-MM-DD`).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub last_report_day: String,
}

/// One row the Skills page / weekly report shows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CuratorRow {
    pub skill_id: String,
    pub views: u32,
    pub uses: u32,
    pub patches: u32,
    pub last_used_ms: u64,
    pub pinned: bool,
    pub locked: bool,
    pub archived: bool,
    pub lifecycle: SkillLifecycle,
    /// Self-made cabin skill (curator target).
    pub self_made: bool,
}

/// Inputs for one skill when building curator rows / deciding archive.
#[derive(Debug, Clone, Default)]
pub struct CuratorSkillInput {
    pub skill_id: String,
    pub origin: String,
    pub pack: String,
    pub views: u32,
    pub uses: u32,
    pub patches: u32,
    pub last_used_ms: u64,
    pub pinned: bool,
    pub locked: bool,
    pub archived: bool,
}

/// Weekly Pulse report body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CuratorReport {
    pub day: String,
    pub generated_at: u64,
    pub active: usize,
    pub stale: usize,
    pub archived: usize,
    pub archived_this_pass: Vec<String>,
    pub top_used: Vec<(String, u32)>,
    pub writer: String,
}

impl CuratorReport {
    pub fn source_id(&self) -> String {
        format!("{CARD_SOURCE_PREFIX}{}", self.day)
    }

    pub fn card_body(&self) -> String {
        let mut parts = Vec::new();
        parts.push(format!(
            "{} active, {} stale, {} archived.",
            self.active, self.stale, self.archived
        ));
        if !self.archived_this_pass.is_empty() {
            parts.push(format!(
                "Archived this week: {}.",
                self.archived_this_pass.join(", ")
            ));
        }
        if let Some((name, uses)) = self.top_used.first() {
            parts.push(format!("Most used: {name} ({uses} uses)."));
        }
        parts.push(format!("Ledger writer: {}.", self.writer));
        parts.join(" ")
    }

    pub fn render(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!("# {CARD_TITLE}\n\n"));
        out.push_str(&format!("Week of {}.\n\n", self.day));
        out.push_str(&format!(
            "- Active: {}\n- Stale: {}\n- Archived: {}\n",
            self.active, self.stale, self.archived
        ));
        if !self.archived_this_pass.is_empty() {
            out.push_str("\n## Archived this pass\n\n");
            for name in &self.archived_this_pass {
                out.push_str(&format!("- {name}\n"));
            }
        }
        if !self.top_used.is_empty() {
            out.push_str("\n## Top used\n\n");
            for (name, uses) in &self.top_used {
                out.push_str(&format!("- {name}: {uses} uses\n"));
            }
        }
        out.push_str(&format!(
            "\nArchive/restore ledger lines name the writer as `{}`.\n",
            self.writer
        ));
        out
    }
}

/// Cabin / user skills with empty or non-library origin are self-made.
pub fn is_self_made(origin: &str, pack: &str) -> bool {
    let origin = origin.trim();
    let pack = pack.trim();
    if origin.eq_ignore_ascii_case(LIBRARY_ORIGIN) {
        return false;
    }
    if !pack.is_empty() && origin.is_empty() {
        // Pack tag alone still counts as curated when origin was set.
        // Empty origin + empty pack = self-made; pack without library origin
        // can still be self-authored with a pack label — treat as self-made
        // unless origin is library-pack.
    }
    true
}

/// Decide Active / Stale / Archived from counters and flags.
pub fn lifecycle(
    last_used_ms: u64,
    now_ms: u64,
    stale_ms: u64,
    pinned: bool,
    locked: bool,
    archived: bool,
) -> SkillLifecycle {
    if archived {
        return SkillLifecycle::Archived;
    }
    if pinned || locked {
        return SkillLifecycle::Active;
    }
    let age = now_ms.saturating_sub(last_used_ms);
    if last_used_ms == 0 || age >= stale_ms {
        SkillLifecycle::Stale
    } else {
        SkillLifecycle::Active
    }
}

/// Whether the weekly pass should archive this skill.
pub fn should_archive(row: &CuratorRow, settings: &SkillCuratorSettings) -> bool {
    settings.enabled
        && row.self_made
        && !row.archived
        && !row.pinned
        && !row.locked
        && row.lifecycle == SkillLifecycle::Stale
}

pub fn build_row(input: &CuratorSkillInput, now_ms: u64, stale_ms: u64) -> CuratorRow {
    let self_made = is_self_made(&input.origin, &input.pack);
    let life = lifecycle(
        input.last_used_ms,
        now_ms,
        stale_ms,
        input.pinned,
        input.locked,
        input.archived,
    );
    CuratorRow {
        skill_id: input.skill_id.clone(),
        views: input.views,
        uses: input.uses,
        patches: input.patches,
        last_used_ms: input.last_used_ms,
        pinned: input.pinned,
        locked: input.locked,
        archived: input.archived,
        lifecycle: life,
        self_made,
    }
}

/// Build the weekly report from rows and names archived this pass.
pub fn build_report(
    day: &str,
    generated_at: u64,
    rows: &[CuratorRow],
    archived_this_pass: &[String],
    writer: &str,
) -> CuratorReport {
    let mut active = 0usize;
    let mut stale = 0usize;
    let mut archived = 0usize;
    let mut top: Vec<(String, u32)> = rows
        .iter()
        .filter(|r| r.self_made)
        .map(|r| {
            match r.lifecycle {
                SkillLifecycle::Active => active += 1,
                SkillLifecycle::Stale => stale += 1,
                SkillLifecycle::Archived => archived += 1,
            }
            (r.skill_id.clone(), r.uses)
        })
        .collect();
    top.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    top.truncate(5);
    CuratorReport {
        day: day.to_string(),
        generated_at,
        active,
        stale,
        archived,
        archived_this_pass: archived_this_pass.to_vec(),
        top_used: top,
        writer: writer.to_string(),
    }
}

/// Same Sunday/`dreamHour` slot as systems brief / self-review.
pub fn curator_report_due(
    last_day: Option<&str>,
    today: &str,
    clock: &LocalClock,
    night_hour: u32,
) -> bool {
    self_review_due(last_day, today, clock, night_hour)
}

pub fn store_path(config_dir: &Path) -> PathBuf {
    models_dir(config_dir).join(CURATOR_FILE)
}

pub fn load_store(config_dir: &Path) -> CuratorStore {
    let mut s: CuratorStore = read_json(&store_path(config_dir)).unwrap_or_default();
    if s.schema == 0 {
        s.schema = CURATOR_SCHEMA;
    }
    s
}

pub fn save_store(config_dir: &Path, store: &CuratorStore) -> Result<(), String> {
    let mut s = store.clone();
    s.schema = CURATOR_SCHEMA;
    write_json(&store_path(config_dir), &s).map_err(|e| e.to_string())
}

pub fn entry_mut<'a>(store: &'a mut CuratorStore, skill_id: &str) -> &'a mut CuratorSkillEntry {
    store.skills.entry(skill_id.trim().to_string()).or_default()
}

/// Record one Skills-page view. Returns the new view count.
pub fn note_view(store: &mut CuratorStore, skill_id: &str) -> u32 {
    let id = skill_id.trim();
    if id.is_empty() {
        return 0;
    }
    let e = entry_mut(store, id);
    e.views = e.views.saturating_add(1);
    e.views
}

pub fn set_pinned(store: &mut CuratorStore, skill_id: &str, pinned: bool) {
    let id = skill_id.trim();
    if id.is_empty() {
        return;
    }
    entry_mut(store, id).pinned = pinned;
}

pub fn set_locked(store: &mut CuratorStore, skill_id: &str, locked: bool) {
    let id = skill_id.trim();
    if id.is_empty() {
        return;
    }
    entry_mut(store, id).locked = locked;
}

/// Folder name under `skills/` for archived skills (never deleted).
pub fn archived_dir(skills_dir: &Path) -> PathBuf {
    skills_dir.join(".archived")
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: u64 = 1_700_000_000_000;
    const STALE_DAYS: u32 = 30;
    const STALE_MS: u64 = 30 * DAY_MS;

    fn settings() -> SkillCuratorSettings {
        SkillCuratorSettings {
            enabled: true,
            stale_days: STALE_DAYS,
        }
    }

    #[test]
    fn settings_default_and_stale_ms_are_literal() {
        let s = SkillCuratorSettings::default();
        assert!(s.enabled);
        assert_eq!(s.stale_days, 30);
        assert_eq!(s.stale_ms(), 2_592_000_000);
        assert_eq!(DAY_MS, 86_400_000);
    }

    #[test]
    fn settings_from_app_value_apply_without_restart_shape() {
        let v = serde_json::json!({"enabled": false, "staleDays": 7});
        let s = SkillCuratorSettings::from_app_value(Some(&v));
        assert!(!s.enabled);
        assert_eq!(s.stale_days, 7);
        assert_eq!(s.stale_ms(), 7 * DAY_MS);
        let missing = SkillCuratorSettings::from_app_value(None);
        assert!(missing.enabled);
        assert_eq!(missing.stale_days, 30);
    }

    #[test]
    fn library_pack_origin_is_not_self_made() {
        assert!(!is_self_made(LIBRARY_ORIGIN, "everyday-life"));
        assert!(is_self_made("", ""));
        assert!(is_self_made("user", ""));
    }

    #[test]
    fn lifecycle_active_stale_archived_with_literal_dates() {
        let used = NOW - (10 * DAY_MS);
        assert_eq!(
            lifecycle(used, NOW, STALE_MS, false, false, false),
            SkillLifecycle::Active
        );
        let old = NOW - (30 * DAY_MS);
        assert_eq!(
            lifecycle(old, NOW, STALE_MS, false, false, false),
            SkillLifecycle::Stale
        );
        let older = NOW - (45 * DAY_MS);
        assert_eq!(
            lifecycle(older, NOW, STALE_MS, false, false, false),
            SkillLifecycle::Stale
        );
        assert_eq!(
            lifecycle(0, NOW, STALE_MS, false, false, false),
            SkillLifecycle::Stale
        );
        assert_eq!(
            lifecycle(old, NOW, STALE_MS, true, false, false),
            SkillLifecycle::Active
        );
        assert_eq!(
            lifecycle(old, NOW, STALE_MS, false, true, false),
            SkillLifecycle::Active
        );
        assert_eq!(
            lifecycle(used, NOW, STALE_MS, false, false, true),
            SkillLifecycle::Archived
        );
    }

    #[test]
    fn build_row_holds_literal_view_use_patch_counts() {
        let input = CuratorSkillInput {
            skill_id: "clip-trimmer".into(),
            origin: String::new(),
            pack: String::new(),
            views: 4,
            uses: 7,
            patches: 2,
            last_used_ms: NOW - (5 * DAY_MS),
            pinned: false,
            locked: false,
            archived: false,
        };
        let row = build_row(&input, NOW, STALE_MS);
        assert_eq!(row.views, 4);
        assert_eq!(row.uses, 7);
        assert_eq!(row.patches, 2);
        assert_eq!(row.last_used_ms, NOW - (5 * DAY_MS));
        assert_eq!(row.lifecycle, SkillLifecycle::Active);
        assert!(row.self_made);
    }

    #[test]
    fn should_archive_only_stale_unlocked_self_made_when_enabled() {
        let s = settings();
        let mut row = build_row(
            &CuratorSkillInput {
                skill_id: "old-one".into(),
                views: 1,
                uses: 0,
                patches: 0,
                last_used_ms: NOW - (40 * DAY_MS),
                ..CuratorSkillInput::default()
            },
            NOW,
            STALE_MS,
        );
        assert_eq!(row.lifecycle, SkillLifecycle::Stale);
        assert!(should_archive(&row, &s));
        row.pinned = true;
        row.lifecycle = lifecycle(row.last_used_ms, NOW, STALE_MS, true, false, false);
        assert!(!should_archive(&row, &s));
        let off = SkillCuratorSettings {
            enabled: false,
            ..s
        };
        row.pinned = false;
        row.lifecycle = SkillLifecycle::Stale;
        assert!(!should_archive(&row, &off));
    }

    #[test]
    fn note_view_increments_literal_counts() {
        let mut store = CuratorStore::default();
        assert_eq!(note_view(&mut store, "flash"), 1);
        assert_eq!(note_view(&mut store, "flash"), 2);
        assert_eq!(note_view(&mut store, "flash"), 3);
        assert_eq!(store.skills["flash"].views, 3);
        set_pinned(&mut store, "flash", true);
        set_locked(&mut store, "flash", true);
        assert!(store.skills["flash"].pinned);
        assert!(store.skills["flash"].locked);
    }

    #[test]
    fn weekly_report_names_writer_and_literal_totals() {
        let rows = vec![
            build_row(
                &CuratorSkillInput {
                    skill_id: "a".into(),
                    uses: 9,
                    last_used_ms: NOW - DAY_MS,
                    ..CuratorSkillInput::default()
                },
                NOW,
                STALE_MS,
            ),
            build_row(
                &CuratorSkillInput {
                    skill_id: "b".into(),
                    uses: 3,
                    last_used_ms: NOW - (40 * DAY_MS),
                    ..CuratorSkillInput::default()
                },
                NOW,
                STALE_MS,
            ),
            build_row(
                &CuratorSkillInput {
                    skill_id: "c".into(),
                    uses: 1,
                    archived: true,
                    last_used_ms: NOW - (100 * DAY_MS),
                    ..CuratorSkillInput::default()
                },
                NOW,
                STALE_MS,
            ),
        ];
        let report = build_report("2026-11-22", NOW, &rows, &["b".into()], "self_manage");
        assert_eq!(report.active, 1);
        assert_eq!(report.stale, 1);
        assert_eq!(report.archived, 1);
        assert_eq!(report.archived_this_pass, vec!["b".to_string()]);
        assert_eq!(report.top_used[0], ("a".into(), 9));
        assert_eq!(report.writer, "self_manage");
        assert!(report.card_body().contains("Ledger writer: self_manage."));
        assert!(report.render().contains("`self_manage`"));
        assert_eq!(report.source_id(), "skill-curator:2026-11-22");
    }

    #[test]
    fn store_round_trips_under_models() {
        let dir = std::env::temp_dir().join(format!(
            "grokhub-skill-curator-{}-{}",
            std::process::id(),
            NOW
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(models_dir(&dir)).unwrap();
        let mut store = CuratorStore::default();
        note_view(&mut store, "notes");
        set_pinned(&mut store, "notes", true);
        store.last_report_day = "2026-11-22".into();
        save_store(&dir, &store).unwrap();
        let loaded = load_store(&dir);
        assert_eq!(loaded.skills["notes"].views, 1);
        assert!(loaded.skills["notes"].pinned);
        assert_eq!(loaded.last_report_day, "2026-11-22");
        assert_eq!(loaded.schema, CURATOR_SCHEMA);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
