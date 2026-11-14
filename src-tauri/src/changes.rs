//! The change ledger for skills, connections and automations: what the app
//! changed on its own, with undo and restore. The reports and result lines
//! read the same as the cabin's, and an undo or restore goes through
//! `grokhub_agent::harness` only, so a ledger line is written and the other
//! app sees the change. Nothing here edits a skill, connection or automation
//! file directly.

use std::path::Path;

use grokhub_agent::harness::{self as hx, ChangeKind, ChangeOp, Origin};
use grokhub_core::Automation;
use serde::Serialize;

/// Ledger lines a report lists.
const SHOWN: usize = 12;
/// Undo rows beside a report.
const ROWS_MAX: usize = 6;
/// Longest reason a report prints, in chars.
const REASON_SHOWN: usize = 72;

fn who(origin: Origin) -> &'static str {
    match origin {
        Origin::User => "you",
        _ => grokhub_core::brand::product_name(),
    }
}

fn what(op: ChangeOp) -> &'static str {
    match op {
        ChangeOp::Create => "added",
        ChangeOp::Modify => "changed",
        ChangeOp::Delete => "removed",
        ChangeOp::Undo => "undone",
        ChangeOp::Restore => "restored",
        ChangeOp::Accept => "kept",
    }
}

fn short(text: &str) -> String {
    if text.chars().count() <= REASON_SHOWN {
        return text.to_string();
    }
    let cut: String = text.chars().take(REASON_SHOWN - 1).collect();
    format!("{}…", cut.trim_end())
}

fn head(kind: ChangeKind) -> String {
    let product = grokhub_core::brand::product_name();
    match kind {
        ChangeKind::Skill => format!("/skills changes — what {product} changed in your skills"),
        ChangeKind::Connection => {
            format!("/connections changes — what {product} changed in your connections")
        }
        _ => format!("/automations changes — what {product} changed in your automations"),
    }
}

/// The report for one kind: the newest ledger lines. The ledger holds ids,
/// labels, reasons (redacted) and hashes only, never the text itself.
pub fn report(config_dir: &Path, kind: ChangeKind, now_ms: u64) -> String {
    let ledger = hx::ChangeLedger::load_kind(config_dir, kind);
    report_of(&ledger, now_ms)
}

fn report_of(ledger: &hx::ChangeLedger, now_ms: u64) -> String {
    let kind = ledger.kind();
    let product = grokhub_core::brand::product_name();
    let mut out = vec![head(kind), String::new()];
    let recent = ledger.recent(SHOWN);
    if recent.is_empty() {
        out.push(format!(
            "- Nothing yet. When {product} adds, changes, or removes {} on its own, it shows here with Undo.",
            match kind {
                ChangeKind::Skill => "a skill",
                ChangeKind::Connection => "a connection",
                _ => "an automation",
            }
        ));
    }
    for c in recent {
        let quiet = match kind {
            ChangeKind::Skill => matches!(c.op, ChangeOp::Undo | ChangeOp::Restore),
            _ => matches!(c.op, ChangeOp::Undo | ChangeOp::Restore | ChangeOp::Accept),
        };
        let why = if quiet || c.reason.is_empty() {
            String::new()
        } else {
            format!(" · {}", short(&c.reason))
        };
        let name = if kind == ChangeKind::Skill {
            c.id.as_str()
        } else {
            c.shown_name()
        };
        out.push(format!(
            "- {name} · {} by {}{why} · {}",
            what(c.op),
            who(c.origin),
            grokhub_core::pulse::ago_label(c.at, now_ms)
        ));
    }
    out.push(String::new());
    if kind == ChangeKind::Skill {
        out.push(format!(
            "Each skill keeps its last {} versions. Undo puts back the one before. It runs only when you type it or click it.",
            hx::HISTORY_CAP
        ));
    } else {
        out.push(format!(
            "Each {} keeps its last {} versions. Undo puts back the one before. It runs only when you click it.",
            kind.as_str(),
            hx::HISTORY_CAP
        ));
    }
    if kind == ChangeKind::Automation {
        out.push(format!(
            "{product} adds at most {} automations a week on its own until you keep one.",
            hx::SELF_AUTOMATION_WEEK_CAP
        ));
    }
    out.join("\n")
}

/// What a row can do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Act {
    Undo,
    Restore,
}

/// One row beside a report: something to undo, or a removed skill to restore.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Row {
    pub id: String,
    pub label: String,
    pub act: Act,
    /// GrokHub's own change is still open, so Keep is offered too.
    pub keep: bool,
}

/// One row per id with something to undo (or, for skills, restore), newest first.
pub fn rows(config_dir: &Path, kind: ChangeKind, now_ms: u64) -> Vec<Row> {
    let ledger = hx::ChangeLedger::load_kind(config_dir, kind);
    let ago = |at: u64| grokhub_core::pulse::ago_label(at, now_ms);
    let mut seen: Vec<&str> = Vec::new();
    let mut out = Vec::new();
    for c in ledger.all().iter().rev() {
        if out.len() >= ROWS_MAX {
            break;
        }
        if seen.contains(&c.id.as_str()) {
            continue;
        }
        seen.push(&c.id);
        let name = if kind == ChangeKind::Skill {
            c.id.as_str()
        } else {
            c.shown_name()
        };
        if let Some(t) = ledger.undo_target(&c.id) {
            out.push(Row {
                id: c.id.clone(),
                label: format!("{name} · {} {}", what(t.op), ago(t.at)),
                act: Act::Undo,
                keep: ledger.open_self_change(&c.id).is_some(),
            });
        } else if kind == ChangeKind::Skill {
            if let Some(gone) = ledger.restore_source(&c.id) {
                out.push(Row {
                    id: c.id.clone(),
                    label: format!("{name} · removed {}", ago(gone.at)),
                    act: Act::Restore,
                    keep: false,
                });
            }
        }
    }
    out
}

/// The result line after an undo or restore.
fn revert_line(kind: ChangeKind, id: &str, done: &hx::Reverted) -> String {
    if kind != ChangeKind::Skill {
        let product = grokhub_core::brand::product_name();
        return match &done.now {
            None => format!(
                "Removed the {} {id}: {product} had added it.",
                kind.as_str()
            ),
            Some(_) => format!("Undid {product}'s change to the {} {id}.", kind.as_str()),
        };
    }
    let id = &done.change.id;
    if done.change.op == ChangeOp::Restore {
        return format!("Restored {id} from its kept copy.");
    }
    if done.now.is_none() {
        return format!(
            "Removed {id}: it was new. Its text is kept, and /skills restore {id} brings it back."
        );
    }
    if done
        .reverted
        .as_ref()
        .is_some_and(|c| c.op == ChangeOp::Delete)
    {
        return format!("{id} is back.");
    }
    format!("Undid the newest change to {id}. The version before it is back.")
}

/// The proof the person asked: their own typed line, or a click on a row.
/// The callers of the functions below are the composer's slash line and a
/// click on a row. Nothing here can check that: this app has no automation
/// or background run that feeds the slash runner, and a paired browser's
/// `/skills undo` or `/skills restore` is refused in `slash::parse`.
fn ask(click: bool) -> hx::UndoAsk {
    if click {
        hx::UndoAsk::from_click()
    } else {
        hx::UndoAsk::from_typing()
    }
}

/// Undo the newest change on one skill, connection or automation that is
/// still in effect. Goes through the ledger, which keeps the bytes it
/// replaced and writes the `undo` line.
pub fn undo(config_dir: &Path, kind: ChangeKind, id: &str, click: bool) -> Result<String, String> {
    let ask = ask(click);
    let done = match kind {
        ChangeKind::Skill => {
            let done = hx::undo_skill_change(config_dir, &config_dir.join("skills"), id, ask)?;
            // The skill's runs from the day before count as failures now.
            crate::chat_trials::skill_undone(config_dir, id, grokhub_core::now_ms());
            done
        }
        ChangeKind::Connection => {
            let done = hx::undo_connection(config_dir, &grokhub_agent::mcp::config_file(), id, ask);
            grokhub_agent::mcp::invalidate();
            done?
        }
        ChangeKind::Automation => {
            // A pause Done for you or Pause it made comes back on as the job
            // is now, so edits made while it was paused stay.
            if let Some(seq) = crate::done_for_you::open_pause(config_dir, id) {
                match undo_pause_as(config_dir, id, seq, ask) {
                    Ok(back) => return Ok(format!("Turned {} back on.", shown_job(&back))),
                    Err(e) if e == CHANGED_SINCE => {}
                    Err(e) => return Err(e),
                }
            }
            let path = crate::automations::path(config_dir);
            let done = crate::automations::with_store(config_dir, |_| {
                hx::undo_change(
                    config_dir,
                    &hx::AutomationsFile { path: &path, id },
                    self::ask(click),
                )
            })?;
            // After the lock: this reads spans and edits Pulse cards.
            if done.reverted.is_some() {
                crate::done_for_you::after_undo(config_dir, kind, id);
            }
            done
        }
        _ => return Err("That kind of change is undone in the cabin.".into()),
    };
    Ok(revert_line(kind, id, &done))
}

/// A Done for you card's Undo found a newer change on the same automation.
pub const CHANGED_SINCE: &str = "It changed again since. Turn it back on from Automations.";

/// A Done for you card's Undo: turn the job back on as it is now, so edits,
/// runs and health since the pause stay, and only while the pause (ledger line
/// `seq`) is still the change in effect on it. It writes the ledger's `undo`
/// line for `seq` and returns the row as it is now.
pub fn undo_pause(config_dir: &Path, id: &str, seq: u64) -> Result<Automation, String> {
    undo_pause_as(config_dir, id, seq, ask(true))
}

fn undo_pause_as(
    config_dir: &Path,
    id: &str,
    seq: u64,
    ask: hx::UndoAsk,
) -> Result<Automation, String> {
    let path = crate::automations::path(config_dir);
    let now = grokhub_core::now_ms();
    let clock = crate::scheduler::local_clock(now, crate::scheduler::utc_offset().unwrap_or(0));
    let mut back = None;
    crate::automations::with_store(config_dir, |_| {
        let target = hx::AutomationsFile { path: &path, id };
        hx::undo_change_at(config_dir, &target, seq, ask, |now| {
            let now = now.ok_or_else(|| "That automation is gone.".to_string())?;
            let mut job: Automation = serde_json::from_slice(&now).map_err(|e| e.to_string())?;
            job.enabled = true;
            let job = grokhub_core::ensure_automation_schedule(job, clock);
            let bytes = serde_json::to_vec_pretty(&job).map_err(|e| e.to_string())?;
            back = Some(job);
            Ok(Some(bytes))
        })
    })?
    .ok_or_else(|| CHANGED_SINCE.to_string())?;
    // After the lock: this reads spans and edits Pulse cards.
    crate::done_for_you::after_undo(config_dir, ChangeKind::Automation, id);
    back.ok_or_else(|| "That automation is gone.".to_string())
}

fn shown_job(a: &Automation) -> String {
    let name = a.name.trim();
    if name.is_empty() {
        a.id.clone()
    } else {
        format!("\"{name}\"")
    }
}

/// Bring back a skill that is gone, from the version it had just before it left.
pub fn restore_skill(config_dir: &Path, name: &str, click: bool) -> Result<String, String> {
    let done = hx::restore_skill(config_dir, &config_dir.join("skills"), name, ask(click))?;
    Ok(revert_line(ChangeKind::Skill, name, &done))
}

/// Keep a change the app made on its own: one `accept` line, nothing on disk changes.
pub fn keep(config_dir: &Path, kind: ChangeKind, id: &str) -> Result<String, String> {
    hx::accept_change(config_dir, kind, id, hx::UndoAsk::from_click())?;
    Ok(format!("Kept the {} {id}.", kind.as_str()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const V1: &str =
        "---\nname: weekly-report\n---\n\n# weekly-report\n\n## Steps\nopen the sheet\n";
    const V2: &str = "---\nname: weekly-report\n---\n\n# weekly-report\n\n## Steps\nopen the sheet, then mail it\n";

    fn dir(label: &str) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!(
            "grokhub-shell-changes-{label}-{}-{}",
            std::process::id(),
            grokhub_core::now_ms()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("skills")).expect("skills dir");
        root
    }

    fn write_skill(root: &Path, body: &str) {
        let folder = root.join("skills").join("weekly-report");
        std::fs::create_dir_all(&folder).expect("folder");
        std::fs::write(folder.join("SKILL.md"), body).expect("write");
    }

    fn read_skill(root: &Path) -> Option<String> {
        std::fs::read_to_string(root.join("skills/weekly-report/SKILL.md")).ok()
    }

    fn record(root: &Path, body: &str) {
        hx::record_skill_change(
            root,
            &root.join("skills"),
            "weekly-report",
            Origin::SelfManage,
            "tightened the steps",
            || {
                write_skill(root, body);
                Ok(())
            },
        )
        .expect("record");
    }

    #[test]
    fn an_empty_ledger_says_so_and_names_the_product() {
        let root = dir("empty");
        let text = report(&root, ChangeKind::Connection, 1_000);
        assert!(text.starts_with("/connections changes — what "), "{text}");
        assert!(text.contains("- Nothing yet. When "), "{text}");
        assert!(text.contains("a connection"), "{text}");
        let auto = report(&root, ChangeKind::Automation, 1_000);
        assert!(auto.contains("an automation"), "{auto}");
        assert!(auto.contains("automations a week on its own"), "{auto}");
        assert_eq!(rows(&root, ChangeKind::Skill, 1_000), Vec::new());
    }

    #[test]
    fn a_skill_change_can_be_undone_and_then_restored() {
        let root = dir("skill");
        write_skill(&root, V1);
        record(&root, V2);

        let text = report(&root, ChangeKind::Skill, grokhub_core::now_ms());
        assert!(text.contains("weekly-report · changed by "), "{text}");
        assert!(text.contains("· tightened the steps"), "{text}");
        let listed = rows(&root, ChangeKind::Skill, grokhub_core::now_ms());
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, "weekly-report");
        assert_eq!(listed[0].act, Act::Undo);
        assert!(listed[0].keep);

        let line = undo(&root, ChangeKind::Skill, "weekly-report", false).expect("undo");
        assert_eq!(
            line,
            "Undid the newest change to weekly-report. The version before it is back."
        );
        assert_eq!(read_skill(&root).as_deref(), Some(V1));
        let after = report(&root, ChangeKind::Skill, grokhub_core::now_ms());
        assert!(after.contains("weekly-report · undone by you"), "{after}");
        assert_eq!(
            undo(&root, ChangeKind::Skill, "weekly-report", false)
                .err()
                .as_deref(),
            Some("No change to undo on weekly-report")
        );
    }

    #[test]
    fn a_skill_undo_fails_that_skills_runs_from_the_day_before() {
        use grokhub_core::outcome::{self as oc, OutcomeResult, OutcomeSignals, TaskOutcome};
        let root = dir("undo-runs");
        write_skill(&root, V1);
        record(&root, V2);
        let now = grokhub_core::now_ms();
        let ran = TaskOutcome::finished(
            "shell-r:1",
            "skill:weekly-report",
            &OutcomeSignals {
                verify_ok: true,
                ..OutcomeSignals::default()
            },
            now - 60_000,
        );
        oc::append_outcome(&root, &ran).unwrap();
        undo(&root, ChangeKind::Skill, "weekly-report", false).expect("undo");
        let all = oc::read_outcomes(&root);
        assert_eq!(
            (all.len(), all[0].result, all[0].reasons.clone()),
            (1, OutcomeResult::Failure, vec!["undo".to_string()])
        );
    }

    #[test]
    fn a_new_skill_is_removed_by_undo_and_comes_back_with_restore() {
        let root = dir("restore");
        record(&root, V1);

        let line = undo(&root, ChangeKind::Skill, "weekly-report", false).expect("undo");
        assert_eq!(
            line,
            "Removed weekly-report: it was new. Its text is kept, and /skills restore weekly-report brings it back."
        );
        assert_eq!(read_skill(&root), None);
        let listed = rows(&root, ChangeKind::Skill, grokhub_core::now_ms());
        assert_eq!(listed[0].act, Act::Restore);

        let line = restore_skill(&root, "weekly-report", false).expect("restore");
        assert_eq!(line, "Restored weekly-report from its kept copy.");
        assert_eq!(read_skill(&root).as_deref(), Some(V1));
        assert_eq!(
            restore_skill(&root, "weekly-report", false)
                .err()
                .as_deref(),
            Some("weekly-report is already there. /skills undo steps it back.")
        );
    }

    #[test]
    fn a_connections_fixture_change_is_listed() {
        let root = dir("conn");
        let line = hx::Change {
            seq: 1,
            at: 1_000,
            kind: "connection".into(),
            id: "notes".into(),
            op: ChangeOp::Create,
            origin: Origin::SelfManage,
            reason: "wanted a notes server".into(),
            before_hash: String::new(),
            after_hash: "abc".into(),
            undoes: None,
            label: String::new(),
        };
        let path = hx::ledger_path(&root, ChangeKind::Connection);
        std::fs::create_dir_all(path.parent().expect("parent")).expect("dir");
        std::fs::write(
            &path,
            format!("{}\n", serde_json::to_string(&line).expect("json")),
        )
        .expect("ledger");
        let text = report(&root, ChangeKind::Connection, 1_000 + 3_600_000);
        assert!(
            text.contains("- notes · added by ")
                && text.contains("· wanted a notes server · 1h ago"),
            "{text}"
        );
    }
}
