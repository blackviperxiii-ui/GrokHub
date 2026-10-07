//! Skill history in the cabin (ChangeLedger, harness design §12 P3):
//! `/skills changes`, `/skills undo <name>`, `/skills restore <name>`, and the
//! Undo / Restore rows under the newest `/skills changes` bubble.
//!
//! An undo or restore runs only from the user's own typed line
//! (`HarnessState::typed_send`, set by `send_from_composer`) or a pointer
//! click on a row (`paint_skill_undo_rows`). The same slash from a night job,
//! an automation, a phone task, a Pulse run, or an idea only posts the rows,
//! so the model never undoes or re-applies a change on its own.

use super::*;
use grokhub_agent::harness::{self as hx, ChangeOp, Origin};

/// First line of the `/skills changes` report; the chat pane finds the bubble by it.
pub(super) const SKILL_CHANGES_HEAD: &str = "/skills changes — what GrokHub changed in your skills";
/// Ledger lines the report lists.
const SKILL_CHANGES_SHOWN: usize = 12;
/// Undo / Restore rows under the bubble.
const SKILL_ROWS_MAX: usize = 6;
/// Longest reason the report prints, in chars.
const REASON_SHOWN: usize = 72;
/// Status when the slash did not come from the user's own typing.
pub(super) const UNDO_NEEDS_YOU: &str =
    "Skill undo runs from your own typing or an Undo click. The rows are below.";
/// Height of one row: the pill's own height.
const SKILL_ROW_H: f32 = 28.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SkillAct {
    Undo,
    Restore,
}

impl SkillAct {
    fn label(self) -> &'static str {
        match self {
            Self::Undo => "Undo",
            Self::Restore => "Restore",
        }
    }
}

/// One row under the `/skills changes` bubble.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SkillRow {
    pub id: String,
    pub label: String,
    pub act: SkillAct,
}

fn who(origin: Origin) -> &'static str {
    match origin {
        Origin::User => "you",
        _ => "GrokHub",
    }
}

fn what(op: ChangeOp) -> &'static str {
    match op {
        ChangeOp::Create => "added",
        ChangeOp::Modify => "changed",
        ChangeOp::Delete => "removed",
        ChangeOp::Undo => "undone",
        ChangeOp::Restore => "restored",
    }
}

fn short(text: &str) -> String {
    if text.chars().count() <= REASON_SHOWN {
        return text.to_string();
    }
    let cut: String = text.chars().take(REASON_SHOWN - 1).collect();
    format!("{}…", cut.trim_end())
}

/// `/skills changes` text: the newest ledger lines. No skill text: the
/// ledger holds ids, reasons (redacted), and hashes only.
pub(super) fn skill_changes_report(ledger: &hx::ChangeLedger, now_ms: u64) -> String {
    let mut out = vec![SKILL_CHANGES_HEAD.to_string(), String::new()];
    let recent = ledger.recent(SKILL_CHANGES_SHOWN);
    if recent.is_empty() {
        out.push("- Nothing yet. When GrokHub adds, changes, or removes a skill on its own, it shows here with Undo.".into());
    }
    for c in recent {
        let why = match c.op {
            ChangeOp::Undo | ChangeOp::Restore => String::new(),
            _ if c.reason.is_empty() => String::new(),
            _ => format!(" · {}", short(&c.reason)),
        };
        out.push(format!(
            "- {} · {} by {}{why} · {}",
            c.id,
            what(c.op),
            who(c.origin),
            grokhub_core::pulse::ago_label(c.at, now_ms)
        ));
    }
    out.push(String::new());
    out.push(format!(
        "Each skill keeps its last {} versions. Undo puts back the one before. It runs only when you type it or click it.",
        hx::HISTORY_CAP
    ));
    out.join("\n")
}

/// One row per skill with something to undo or restore, newest first.
pub(super) fn skill_undo_rows(ledger: &hx::ChangeLedger, now_ms: u64) -> Vec<SkillRow> {
    let mut seen: Vec<&str> = Vec::new();
    let mut rows = Vec::new();
    for c in ledger.all().iter().rev() {
        if rows.len() >= SKILL_ROWS_MAX {
            break;
        }
        if seen.contains(&c.id.as_str()) {
            continue;
        }
        seen.push(&c.id);
        let ago = |at: u64| grokhub_core::pulse::ago_label(at, now_ms);
        if let Some(t) = ledger.undo_target(&c.id) {
            rows.push(SkillRow {
                id: c.id.clone(),
                label: format!("{} · {} {}", c.id, what(t.op), ago(t.at)),
                act: SkillAct::Undo,
            });
        } else if let Some(gone) = ledger.restore_source(&c.id) {
            rows.push(SkillRow {
                id: c.id.clone(),
                label: format!("{} · removed {}", c.id, ago(gone.at)),
                act: SkillAct::Restore,
            });
        }
    }
    rows
}

/// The result line after an undo or restore.
pub(super) fn revert_line(done: &hx::Reverted) -> String {
    let id = &done.change.id;
    if done.change.op == ChangeOp::Restore {
        return format!("Restored {id} from its kept copy.");
    }
    if done.now.is_none() {
        return format!("Removed {id}: it was new. Its text is kept, and /skills restore {id} brings it back.");
    }
    if done.reverted.as_ref().is_some_and(|c| c.op == ChangeOp::Delete) {
        return format!("{id} is back.");
    }
    format!("Undid the newest change to {id}. The version before it is back.")
}

impl Cabin {
    /// `/skills changes`: one result bubble from the ledger, with rows under it.
    pub(super) fn run_skill_changes(&mut self) {
        let ledger = hx::ChangeLedger::load(&config::config_dir());
        let body = skill_changes_report(&ledger, now_ms());
        self.harness.skill_rows = None;
        self.post_skill_result(&body);
    }

    /// `/skills undo <name>`. Only the user's own typed line undoes; anything
    /// else (night, automation, phone, Pulse, idea text) gets the rows.
    pub(super) fn run_skill_undo(&mut self, name: &str) {
        if !self.harness.typed_send {
            self.run_skill_changes();
            self.status = UNDO_NEEDS_YOU.into();
            return;
        }
        let res = hx::undo_skill_change(
            &config::config_dir(),
            &skills::skills_dir(),
            name,
            hx::UndoAsk::from_typing(),
        );
        self.finish_skill_revert(name, res);
    }

    /// `/skills restore <name>`, on the same terms as undo.
    pub(super) fn run_skill_restore(&mut self, name: &str) {
        if !self.harness.typed_send {
            self.run_skill_changes();
            self.status = UNDO_NEEDS_YOU.into();
            return;
        }
        let res = hx::restore_skill(
            &config::config_dir(),
            &skills::skills_dir(),
            name,
            hx::UndoAsk::from_typing(),
        );
        self.finish_skill_revert(name, res);
    }

    /// Undo or Restore clicked under the `/skills changes` bubble. Only
    /// `paint_skill_undo_rows` returns a row, and only for a click.
    pub(super) fn skill_row_clicked(&mut self, row: &SkillRow) {
        let (dir, skills) = (config::config_dir(), skills::skills_dir());
        let res = match row.act {
            SkillAct::Undo => hx::undo_skill_change(&dir, &skills, &row.id, hx::UndoAsk::from_click()),
            SkillAct::Restore => hx::restore_skill(&dir, &skills, &row.id, hx::UndoAsk::from_click()),
        };
        self.finish_skill_revert(&row.id, res);
    }

    /// The rows for the newest `/skills changes` bubble, read once per change.
    pub(super) fn skill_rows_now(&mut self) -> Vec<SkillRow> {
        if self.harness.skill_rows.is_none() {
            let ledger = hx::ChangeLedger::load(&config::config_dir());
            self.harness.skill_rows = Some(skill_undo_rows(&ledger, now_ms()));
        }
        self.harness.skill_rows.clone().unwrap_or_default()
    }

    fn finish_skill_revert(&mut self, name: &str, res: Result<hx::Reverted, String>) {
        self.harness.skill_rows = None;
        let line = match res {
            Ok(done) => {
                match &done.now {
                    Some(bytes) => {
                        let raw = String::from_utf8_lossy(bytes).into_owned();
                        let parsed = grokhub_core::parse_skill_md(&raw);
                        let _ = skills::rewrite_verify_scripts(&parsed);
                        if self.skill_name == parsed.name {
                            self.skill_body = raw;
                        }
                        self.remember_skill(parsed);
                    }
                    None => {
                        let dir = grokhub_core::skill_dir_name(name);
                        self.skill_list.retain(|s| grokhub_core::skill_dir_name(&s.name) != dir);
                    }
                }
                revert_line(&done)
            }
            Err(e) => e,
        };
        self.status = line.clone();
        self.post_skill_result(&line);
    }

    fn post_skill_result(&mut self, body: &str) {
        self.live_mut().push(("assistant".into(), mark_slash_result(body)));
        self.stamp_current_access();
        self.persist();
    }
}

/// Under the newest `/skills changes` bubble: one row per skill with a ghost
/// Undo or Restore. Returns the clicked row. Typed text never answers it.
pub(super) fn paint_skill_undo_rows(ui: &mut egui::Ui, rows: &[SkillRow]) -> Option<SkillRow> {
    let mut hit = None;
    for row in rows {
        ui.push_id(("skill-undo", row.id.as_str()), |ui| {
            let size = egui::vec2(ui.available_width(), SKILL_ROW_H);
            ui.allocate_ui_with_layout(size, egui::Layout::left_to_right(egui::Align::Center), |ui| {
                ui.label(RichText::new(&row.label).size(13.0).color(crate::theme::muted()));
                if crate::cards::ghost_pill(ui, row.act.label()) {
                    hit = Some(row.clone());
                }
            });
        });
    }
    if !rows.is_empty() {
        ui.add_space(4.0);
    }
    hit
}

/// The newest `/skills changes` report among the painted rows, if any.
pub(super) fn newest_skill_changes_row(views: &[grokhub_core::ChatView]) -> Option<usize> {
    views
        .iter()
        .rposition(|v| v.kind == grokhub_core::ChatKind::Result && v.body.starts_with(SKILL_CHANGES_HEAD))
}
