//! Skill curator host glue: archive/restore under `skills/.archived/` (never
//! delete), pin/lock/view counts in the curator store, patch counts from the
//! skill ChangeLedger, uses from `skill_metrics`, and a weekly pass that
//! archives stale self-made skills when enabled. Ledger lines name the real
//! writer (`self_manage` for the weekly pass, `user` for one-click restore).

use std::fs;
use std::path::{Path, PathBuf};

use grokhub_core::skill_curator::{
    archived_dir, build_report, build_row, entry_mut, is_self_made, load_store, note_view,
    save_store, set_locked, set_pinned, should_archive, CuratorReport, CuratorRow,
    CuratorSkillInput, CuratorStore, SkillCuratorSettings, SkillLifecycle,
};
use grokhub_core::{
    parse_skill_md, parse_skill_meta, render_skill_md_with_meta, skill_dir_name, SkillMeta,
};

use crate::harness::{self as hx, ChangeKind, ChangeOp, Origin};
use crate::route::skill_metrics;
use crate::skill_write::skills_dir;

/// Read curator settings from `app.json` (re-read each call — no restart).
pub fn settings(config_dir: &Path) -> SkillCuratorSettings {
    let map = grokhub_core::app_json::read(config_dir).unwrap_or_default();
    SkillCuratorSettings::from_app_value(map.get(grokhub_core::SKILL_CURATOR_SETTINGS_KEY))
}

/// Patch count for one skill id from the skills ChangeLedger (create+modify).
pub fn patch_count(config_dir: &Path, skill_id: &str) -> u32 {
    let id = ChangeKind::Skill.id_of(skill_id);
    if id.is_empty() {
        return 0;
    }
    hx::ChangeLedger::load(config_dir)
        .all()
        .iter()
        .filter(|c| c.id == id && matches!(c.op, ChangeOp::Create | ChangeOp::Modify))
        .count() as u32
}

fn uses_and_last(config_dir: &Path, skill_id: &str) -> (u32, u64) {
    let m = skill_metrics::load(config_dir);
    m.skills
        .get(skill_id)
        .or_else(|| m.skills.get(&skill_dir_name(skill_id)))
        .map(|s| (s.invocations, s.last_used_ms))
        .unwrap_or((0, 0))
}

fn read_cabin_skill(dir: &Path) -> Option<(String, SkillMeta, String)> {
    let raw = fs::read_to_string(dir.join("SKILL.md")).ok()?;
    let skill = parse_skill_md(&raw);
    let name = skill.name.trim().to_string();
    if name.is_empty() {
        return None;
    }
    let meta = parse_skill_meta(&raw);
    Some((name, meta, raw))
}

fn write_meta(dir: &Path, meta: &SkillMeta) -> Result<(), String> {
    let path = dir.join("SKILL.md");
    let raw = fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let skill = parse_skill_md(&raw);
    let text = render_skill_md_with_meta(&skill, meta);
    grokhub_core::work_file::write_private_atomic(&path, text.as_bytes())
}

fn cabin_dirs(skills: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(skills) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for e in entries.flatten() {
        let p = e.path();
        let name = e.file_name();
        let name = name.to_string_lossy();
        if name.starts_with('.') {
            continue;
        }
        if p.join("SKILL.md").is_file() {
            out.push(p);
        }
    }
    out.sort();
    out
}

fn archived_skill_dirs(skills: &Path) -> Vec<PathBuf> {
    let root = archived_dir(skills);
    let Ok(entries) = fs::read_dir(&root) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for e in entries.flatten() {
        let p = e.path();
        if p.join("SKILL.md").is_file() {
            out.push(p);
        }
    }
    out.sort();
    out
}

fn input_for(
    config_dir: &Path,
    store: &CuratorStore,
    name: &str,
    meta: &SkillMeta,
    archived: bool,
) -> CuratorSkillInput {
    let key = skill_dir_name(name);
    let entry = store
        .skills
        .get(&key)
        .or_else(|| store.skills.get(name))
        .cloned()
        .unwrap_or_default();
    let (uses, metric_last) = uses_and_last(config_dir, &key);
    let last_used_ms = meta.last_used_at.max(metric_last);
    CuratorSkillInput {
        skill_id: key,
        origin: meta.origin.clone(),
        pack: meta.pack.clone(),
        views: entry.views,
        uses,
        patches: patch_count(config_dir, name),
        last_used_ms,
        pinned: entry.pinned,
        locked: entry.locked,
        archived: archived || meta.archived,
    }
}

/// Curator rows for every cabin skill (active + archived).
pub fn rows(config_dir: &Path, now_ms: u64) -> Vec<CuratorRow> {
    let settings = settings(config_dir);
    let stale_ms = settings.stale_ms();
    let store = load_store(config_dir);
    let skills = skills_dir(config_dir);
    let mut out = Vec::new();
    for dir in cabin_dirs(&skills) {
        let Some((name, meta, _)) = read_cabin_skill(&dir) else {
            continue;
        };
        out.push(build_row(
            &input_for(config_dir, &store, &name, &meta, false),
            now_ms,
            stale_ms,
        ));
    }
    for dir in archived_skill_dirs(&skills) {
        let Some((name, meta, _)) = read_cabin_skill(&dir) else {
            continue;
        };
        out.push(build_row(
            &input_for(config_dir, &store, &name, &meta, true),
            now_ms,
            stale_ms,
        ));
    }
    out.sort_by(|a, b| a.skill_id.cmp(&b.skill_id));
    out
}

/// Record a Skills detail/page view for a cabin skill.
pub fn record_view(config_dir: &Path, skill_id: &str) -> Result<u32, String> {
    let mut store = load_store(config_dir);
    let n = note_view(&mut store, &skill_dir_name(skill_id));
    save_store(config_dir, &store)?;
    Ok(n)
}

pub fn pin(config_dir: &Path, skill_id: &str, pinned: bool) -> Result<(), String> {
    let mut store = load_store(config_dir);
    set_pinned(&mut store, &skill_dir_name(skill_id), pinned);
    save_store(config_dir, &store)
}

pub fn lock(config_dir: &Path, skill_id: &str, locked: bool) -> Result<(), String> {
    let mut store = load_store(config_dir);
    set_locked(&mut store, &skill_dir_name(skill_id), locked);
    save_store(config_dir, &store)
}

fn move_skill_folder(from: &Path, to: &Path) -> Result<(), String> {
    if to.exists() {
        return Err(format!("destination already exists: {}", to.display()));
    }
    if let Some(parent) = to.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    fs::rename(from, to).map_err(|e| e.to_string())
}

/// Archive a cabin skill into `skills/.archived/` and set `archived: true`.
/// Never deletes files. Ledger writer is `origin`.
pub fn archive_skill(
    config_dir: &Path,
    name: &str,
    origin: Origin,
    reason: &str,
    now_ms: u64,
) -> Result<(), String> {
    let skills = skills_dir(config_dir);
    let key = skill_dir_name(name);
    let from = skills.join(&key);
    if !from.join("SKILL.md").is_file() {
        return Err(format!("no active skill named {name}"));
    }
    let to = archived_dir(&skills).join(&key);
    let done = hx::record_skill_change(config_dir, &skills, name, origin, reason, || {
        let mut meta = read_cabin_skill(&from)
            .map(|(_, m, _)| m)
            .unwrap_or_default();
        meta.archived = true;
        write_meta(&from, &meta)?;
        move_skill_folder(&from, &to)
    })?;
    let mut store = load_store(config_dir);
    entry_mut(&mut store, &key).archived_at = now_ms;
    save_store(config_dir, &store)?;
    let _ = done;
    Ok(())
}

/// Restore an archived skill back under `skills/` in one step.
pub fn restore_skill(
    config_dir: &Path,
    name: &str,
    origin: Origin,
    reason: &str,
) -> Result<(), String> {
    let skills = skills_dir(config_dir);
    let key = skill_dir_name(name);
    let from = archived_dir(&skills).join(&key);
    if !from.join("SKILL.md").is_file() {
        return Err(format!("no archived skill named {name}"));
    }
    let to = skills.join(&key);
    let done = hx::record_skill_change(config_dir, &skills, name, origin, reason, || {
        let mut meta = read_cabin_skill(&from)
            .map(|(_, m, _)| m)
            .unwrap_or_default();
        meta.archived = false;
        write_meta(&from, &meta)?;
        move_skill_folder(&from, &to)
    })?;
    let mut store = load_store(config_dir);
    if let Some(e) = store.skills.get_mut(&key) {
        e.archived_at = 0;
    }
    save_store(config_dir, &store)?;
    let _ = done;
    Ok(())
}

/// Weekly pass: archive stale self-made skills when enabled; build a report.
pub fn weekly_pass(config_dir: &Path, day: &str, now_ms: u64) -> Option<CuratorReport> {
    let settings = settings(config_dir);
    if !settings.enabled {
        return None;
    }
    let mut archived_names = Vec::new();
    let current = rows(config_dir, now_ms);
    for row in &current {
        if !should_archive(row, &settings) {
            continue;
        }
        if archive_skill(
            config_dir,
            &row.skill_id,
            Origin::SelfManage,
            "skill curator: unused past stale window",
            now_ms,
        )
        .is_ok()
        {
            archived_names.push(row.skill_id.clone());
        }
    }
    let after = rows(config_dir, now_ms);
    let report = build_report(
        day,
        now_ms,
        &after,
        &archived_names,
        Origin::SelfManage.as_str(),
    );
    let mut store = load_store(config_dir);
    store.last_report_day = day.to_string();
    let _ = save_store(config_dir, &store);
    Some(report)
}

/// Sync `last_used_at` on a cabin skill's SKILL.md from metrics (best effort).
pub fn touch_last_used(config_dir: &Path, skill_id: &str, at_ms: u64) {
    let skills = skills_dir(config_dir);
    let dir = skills.join(skill_dir_name(skill_id));
    let Ok(Some((_, mut meta, _))) = fs::read_to_string(dir.join("SKILL.md"))
        .map(|raw| {
            let skill = parse_skill_md(&raw);
            let name = skill.name.trim().to_string();
            if name.is_empty() {
                None
            } else {
                Some((name, parse_skill_meta(&raw), raw))
            }
        })
        .map_err(|_| ())
    else {
        return;
    };
    if !is_self_made(&meta.origin, &meta.pack) || meta.archived {
        return;
    }
    if meta.last_used_at >= at_ms {
        return;
    }
    meta.last_used_at = at_ms;
    let _ = write_meta(&dir, &meta);
}

/// Expose lifecycle label for UI rows.
pub fn lifecycle_label(life: SkillLifecycle) -> &'static str {
    life.as_str()
}

#[cfg(test)]
mod tests {
    use super::*;
    use grokhub_core::skill_curator::{DAY_MS, DEFAULT_STALE_DAYS};
    use grokhub_core::SkillMd;

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "grokhub-agent-curator-{name}-{}-{}",
            std::process::id(),
            grokhub_core::now_ms()
        ));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn write_cabin(cfg: &Path, name: &str, last_used: u64) {
        let dir = skills_dir(cfg).join(skill_dir_name(name));
        fs::create_dir_all(&dir).unwrap();
        let skill = SkillMd {
            name: name.into(),
            description: "test".into(),
            slash: format!("/{name}"),
            trigger: "test".into(),
            instructions: "do it".into(),
            ..SkillMd::default()
        };
        let meta = SkillMeta {
            last_used_at: last_used,
            ..SkillMeta::default()
        };
        fs::write(
            dir.join("SKILL.md"),
            render_skill_md_with_meta(&skill, &meta),
        )
        .unwrap();
    }

    #[test]
    fn archive_and_restore_keep_files_and_literal_counts() {
        let cfg = temp("archive");
        let _g = crate::perm::ConfigGuard::set(&cfg);
        let now = 1_700_000_000_000u64;
        write_cabin(&cfg, "clip-trimmer", now - (40 * DAY_MS));
        let mut store = load_store(&cfg);
        assert_eq!(note_view(&mut store, "clip-trimmer"), 1);
        assert_eq!(note_view(&mut store, "clip-trimmer"), 2);
        save_store(&cfg, &store).unwrap();

        // Two patches via ledger.
        crate::skill_write::save_skill_logged(
            &cfg,
            &parse_skill_md(
                &fs::read_to_string(skills_dir(&cfg).join("clip-trimmer").join("SKILL.md"))
                    .unwrap(),
            ),
            Origin::User,
            "edit one",
        )
        .unwrap();
        let mut edited = parse_skill_md(
            &fs::read_to_string(skills_dir(&cfg).join("clip-trimmer").join("SKILL.md")).unwrap(),
        );
        edited.instructions = "do it better".into();
        // Preserve meta on edit through curator write_meta path after save.
        crate::skill_write::save_skill_logged(&cfg, &edited, Origin::User, "edit two").unwrap();

        let before = rows(&cfg, now);
        let row = before
            .iter()
            .find(|r| r.skill_id == "clip-trimmer")
            .unwrap();
        assert_eq!(row.views, 2);
        assert!(row.patches >= 1);
        assert_eq!(row.lifecycle, SkillLifecycle::Stale);

        archive_skill(&cfg, "clip-trimmer", Origin::SelfManage, "stale", now).unwrap();
        assert!(!skills_dir(&cfg)
            .join("clip-trimmer")
            .join("SKILL.md")
            .exists());
        assert!(archived_dir(&skills_dir(&cfg))
            .join("clip-trimmer")
            .join("SKILL.md")
            .exists());

        restore_skill(&cfg, "clip-trimmer", Origin::User, "one-click restore").unwrap();
        assert!(skills_dir(&cfg)
            .join("clip-trimmer")
            .join("SKILL.md")
            .exists());
        let after = rows(&cfg, now);
        let restored = after.iter().find(|r| r.skill_id == "clip-trimmer").unwrap();
        assert!(!restored.archived);
        assert_eq!(restored.views, 2);

        let _ = fs::remove_dir_all(&cfg);
    }

    #[test]
    fn weekly_pass_archives_stale_when_enabled_and_skips_when_off() {
        let cfg = temp("weekly");
        let _g = crate::perm::ConfigGuard::set(&cfg);
        let now = 1_700_000_000_000u64;
        write_cabin(
            &cfg,
            "old-skill",
            now - (u64::from(DEFAULT_STALE_DAYS) * DAY_MS),
        );
        write_cabin(&cfg, "fresh-skill", now - DAY_MS);
        grokhub_core::app_json::patch(
            &cfg,
            &[(
                grokhub_core::SKILL_CURATOR_SETTINGS_KEY,
                serde_json::json!({"enabled": true, "staleDays": 30}),
            )],
        )
        .unwrap();
        let report = weekly_pass(&cfg, "2026-11-22", now).expect("report");
        assert_eq!(report.archived_this_pass, vec!["old-skill".to_string()]);
        assert!(archived_dir(&skills_dir(&cfg))
            .join("old-skill")
            .join("SKILL.md")
            .exists());
        assert!(skills_dir(&cfg)
            .join("fresh-skill")
            .join("SKILL.md")
            .exists());
        assert_eq!(report.writer, "self_manage");

        grokhub_core::app_json::patch(
            &cfg,
            &[(
                grokhub_core::SKILL_CURATOR_SETTINGS_KEY,
                serde_json::json!({"enabled": false, "staleDays": 30}),
            )],
        )
        .unwrap();
        write_cabin(&cfg, "another-old", now - (60 * DAY_MS));
        assert!(weekly_pass(&cfg, "2026-11-29", now).is_none());
        assert!(skills_dir(&cfg)
            .join("another-old")
            .join("SKILL.md")
            .exists());

        let _ = fs::remove_dir_all(&cfg);
    }

    #[test]
    fn pin_blocks_archive() {
        let cfg = temp("pin");
        let _g = crate::perm::ConfigGuard::set(&cfg);
        let now = 1_700_000_000_000u64;
        write_cabin(&cfg, "kept", now - (60 * DAY_MS));
        pin(&cfg, "kept", true).unwrap();
        grokhub_core::app_json::patch(
            &cfg,
            &[(
                grokhub_core::SKILL_CURATOR_SETTINGS_KEY,
                serde_json::json!({"enabled": true, "staleDays": 30}),
            )],
        )
        .unwrap();
        let report = weekly_pass(&cfg, "2026-11-22", now).unwrap();
        assert!(report.archived_this_pass.is_empty());
        assert!(skills_dir(&cfg).join("kept").join("SKILL.md").exists());
        let _ = fs::remove_dir_all(&cfg);
    }
}
