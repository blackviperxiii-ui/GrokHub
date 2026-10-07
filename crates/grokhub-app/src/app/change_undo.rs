//! Spike-5b in the cabin (ChangeLedger, harness design §12 P3): what GrokHub
//! changed on its own in skills, connections, and automations.
//!
//! - Work-tree rows: "Grok added connection notes" with ghost Undo and Keep,
//!   under the approval cards. A self-made change posts one row as it lands,
//!   and changes from the last day come back as rows after a restart (the
//!   ledger is on disk).
//! - Home update: each self-made change posts a `SelfChange` card.
//! - `/connections changes` and `/automations changes`: the report bubble,
//!   with Undo (and Keep) rows under the newest one, like `/skills changes`.
//!
//! Undo and Keep answer a pointer click only (`change_row_clicked` builds the
//! one `UndoAsk`); Enter or Space on a focused pill does nothing, and no
//! model reply, automation, or Pulse run reaches them.

use super::*;
use grokhub_agent::harness::{self as hx, ChangeKind, ChangeOp, Origin};

pub(super) const CONNECTION_CHANGES_HEAD: &str = "/connections changes — what GrokHub changed in your connections";
pub(super) const AUTOMATION_CHANGES_HEAD: &str = "/automations changes — what GrokHub changed in your automations";
/// Ledger lines a report lists.
const CHANGES_SHOWN: usize = 12;
/// Undo rows under a report bubble.
const REPORT_ROWS_MAX: usize = 6;
/// Work-tree rows at once, newest first.
const WORK_ROWS_MAX: usize = 3;
/// After a restart, self-made changes this recent come back as Work-tree rows.
const WORK_ROW_MS: u64 = 24 * 60 * 60 * 1000;
/// Longest reason a report prints, in chars.
const REASON_SHOWN: usize = 72;
/// Height of one row: the pill's own height.
const ROW_H: f32 = 28.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ChangeAct {
    Undo,
    Keep,
}

/// One row with Undo (and Keep when GrokHub's change is still open).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ChangeRow {
    pub kind: ChangeKind,
    pub id: String,
    pub label: String,
    pub keep: bool,
}

pub(super) fn changes_head(kind: ChangeKind) -> &'static str {
    match kind {
        ChangeKind::Connection => CONNECTION_CHANGES_HEAD,
        _ => AUTOMATION_CHANGES_HEAD,
    }
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

/// `/connections changes` or `/automations changes` text from one ledger.
/// The ledger holds ids, labels, reasons (redacted), and hashes only.
pub(super) fn changes_report(ledger: &hx::ChangeLedger, now_ms: u64) -> String {
    let kind = ledger.kind();
    let mut out = vec![changes_head(kind).to_string(), String::new()];
    let recent = ledger.recent(CHANGES_SHOWN);
    if recent.is_empty() {
        out.push(format!(
            "- Nothing yet. When GrokHub adds, changes, or removes {} on its own, it shows here with Undo.",
            match kind {
                ChangeKind::Connection => "a connection",
                _ => "an automation",
            }
        ));
    }
    for c in recent {
        let why = match c.op {
            ChangeOp::Undo | ChangeOp::Restore | ChangeOp::Accept => String::new(),
            _ if c.reason.is_empty() => String::new(),
            _ => format!(" · {}", short(&c.reason)),
        };
        out.push(format!(
            "- {} · {} by {}{why} · {}",
            c.shown_name(),
            what(c.op),
            who(c.origin),
            grokhub_core::pulse::ago_label(c.at, now_ms)
        ));
    }
    out.push(String::new());
    out.push(format!(
        "Each {} keeps its last {} versions. Undo puts back the one before. It runs only when you click it.",
        kind.as_str(),
        hx::HISTORY_CAP
    ));
    if kind == ChangeKind::Automation {
        out.push(format!(
            "GrokHub adds at most {} automations a week on its own until you keep one.",
            hx::SELF_AUTOMATION_WEEK_CAP
        ));
    }
    out.join("\n")
}

/// One row per id with something to undo, newest first.
pub(super) fn report_rows(ledger: &hx::ChangeLedger, now_ms: u64) -> Vec<ChangeRow> {
    let mut seen: Vec<&str> = Vec::new();
    let mut rows = Vec::new();
    for c in ledger.all().iter().rev() {
        if rows.len() >= REPORT_ROWS_MAX {
            break;
        }
        if seen.contains(&c.id.as_str()) {
            continue;
        }
        seen.push(&c.id);
        if let Some(t) = ledger.undo_target(&c.id) {
            rows.push(ChangeRow {
                kind: ledger.kind(),
                id: c.id.clone(),
                label: format!(
                    "{} · {} {}",
                    c.shown_name(),
                    what(t.op),
                    grokhub_core::pulse::ago_label(t.at, now_ms)
                ),
                keep: ledger.open_self_change(&c.id).is_some(),
            });
        }
    }
    rows
}

/// The Work-tree row for one self-made change.
pub(super) fn work_row(kind: ChangeKind, c: &hx::Change) -> ChangeRow {
    ChangeRow {
        kind,
        id: c.id.clone(),
        label: format!("Grok {} {} {}", what(c.op), kind.as_str(), c.shown_name()),
        keep: true,
    }
}

/// Work-tree rows after a restart: self-made changes from the last day that
/// are still in effect and not kept, newest first.
/// `skip` drops lines that already have their own Undo (a Done-for-you card).
pub(super) fn work_rows_from_disk(
    config_dir: &std::path::Path,
    now_ms: u64,
    skip: &dyn Fn(ChangeKind, u64) -> bool,
) -> Vec<ChangeRow> {
    let mut found: Vec<(u64, ChangeRow)> = Vec::new();
    for kind in ChangeKind::ALL {
        let ledger = hx::ChangeLedger::load_kind(config_dir, kind);
        let mut seen: Vec<&str> = Vec::new();
        for c in ledger.all().iter().rev() {
            if seen.contains(&c.id.as_str()) {
                continue;
            }
            seen.push(&c.id);
            if let Some(open) = ledger.open_self_change(&c.id) {
                if now_ms.saturating_sub(open.at) < WORK_ROW_MS && !skip(kind, open.seq) {
                    found.push((open.at, work_row(kind, open)));
                }
            }
        }
    }
    found.sort_by_key(|(at, _)| std::cmp::Reverse(*at));
    found.into_iter().take(WORK_ROWS_MAX).map(|(_, r)| r).collect()
}

/// The newest `/connections changes` or `/automations changes` report among
/// the painted rows, and which kind it is.
pub(super) fn newest_change_report(views: &[grokhub_core::ChatView]) -> Option<(usize, ChangeKind)> {
    views.iter().enumerate().rev().find_map(|(i, v)| {
        if v.kind != grokhub_core::ChatKind::Result {
            return None;
        }
        if v.body.starts_with(CONNECTION_CHANGES_HEAD) {
            Some((i, ChangeKind::Connection))
        } else if v.body.starts_with(AUTOMATION_CHANGES_HEAD) {
            Some((i, ChangeKind::Automation))
        } else {
            None
        }
    })
}

/// The result line after a click.
fn done_line(row: &ChangeRow, act: ChangeAct, done: &Result<Option<Vec<u8>>, String>) -> String {
    let (kind, name) = (row.kind.as_str(), row.label_name());
    match (act, done) {
        (_, Err(e)) => e.clone(),
        (ChangeAct::Keep, Ok(_)) => format!("Kept the {kind} {name}."),
        (ChangeAct::Undo, Ok(None)) => format!("Removed the {kind} {name}: GrokHub had added it."),
        (ChangeAct::Undo, Ok(Some(_))) => format!("Undid GrokHub's change to the {kind} {name}."),
    }
}

impl ChangeRow {
    /// The name part of the label (after "Grok added connection ", or
    /// before " · " on a report row).
    fn label_name(&self) -> &str {
        let head = format!(" {} ", self.kind.as_str());
        match self.label.split_once(&head) {
            Some((_, name)) => name,
            None => self.label.split(" · ").next().unwrap_or(&self.label),
        }
    }
}

impl Cabin {
    /// `/connections changes` and `/automations changes`: one result bubble
    /// from the ledger, with Undo rows under it.
    pub(super) fn run_change_report(&mut self, kind: ChangeKind) {
        let ledger = hx::ChangeLedger::load_kind(&config::config_dir(), kind);
        let body = changes_report(&ledger, now_ms());
        self.harness.change_rows = None;
        self.live_mut().push(("assistant".into(), mark_slash_result(&body)));
        self.stamp_current_access();
        self.persist();
    }

    /// Rows for the newest report bubble, read once per change.
    pub(super) fn change_rows_now(&mut self, kind: ChangeKind) -> Vec<ChangeRow> {
        if self.harness.change_rows.as_ref().is_none_or(|(k, _)| *k != kind) {
            let ledger = hx::ChangeLedger::load_kind(&config::config_dir(), kind);
            self.harness.change_rows = Some((kind, report_rows(&ledger, now_ms())));
        }
        self.harness.change_rows.as_ref().map(|(_, r)| r.clone()).unwrap_or_default()
    }

    /// Each frame: changes GrokHub just made on its own become a Work-tree
    /// row and a Home update. The first call also brings back the rows of
    /// the last day from disk.
    pub(super) fn poll_self_changes(&mut self) {
        let dir = config::config_dir();
        if !self.harness.work_rows_loaded {
            self.harness.work_rows_loaded = true;
            let auto = self.auto_lines();
            self.harness.work_rows = work_rows_from_disk(&dir, now_ms(), &|kind, seq| auto.contains(&(kind, seq)));
        }
        let fresh = hx::take_self_changes(&dir);
        if fresh.is_empty() {
            return;
        }
        self.harness.change_rows = None;
        for (kind, c) in fresh {
            // An auto-act's Done-for-you card already carries its Undo.
            if self.auto_lines().contains(&(kind, c.seq)) {
                continue;
            }
            if kind == ChangeKind::Skill {
                self.harness.skill_rows = None;
            }
            let row = work_row(kind, &c);
            self.harness.work_rows.retain(|r| !(r.kind == kind && r.id == row.id));
            self.harness.work_rows.insert(0, row);
            self.harness.work_rows.truncate(WORK_ROWS_MAX);
            let card = grokhub_core::self_change_card(kind.as_str(), c.shown_name(), what(c.op), &c.reason, c.at);
            self.post_feed_card(card);
        }
    }

    /// Undo or Keep clicked on a Work-tree row, under a report bubble, or on
    /// a Done-for-you card. Only those painters return a row, and only for a
    /// pointer click. True when it worked.
    pub(super) fn change_row_clicked(&mut self, row: &ChangeRow, act: ChangeAct) -> bool {
        let dir = config::config_dir();
        let ask = hx::UndoAsk::from_click();
        let done: Result<Option<Vec<u8>>, String> = match (act, row.kind) {
            (ChangeAct::Keep, kind) => hx::accept_change(&dir, kind, &row.id, ask).map(|_| Some(Vec::new())),
            (ChangeAct::Undo, ChangeKind::Skill) => {
                let res = hx::undo_skill_change(&dir, &skills::skills_dir(), &row.id, ask);
                let now = res.as_ref().map(|d| d.now.clone()).map_err(|e| e.clone());
                self.finish_skill_revert(&row.id, res);
                now
            }
            (ChangeAct::Undo, ChangeKind::Connection) => {
                let res = hx::undo_connection(&dir, &grokhub_agent::mcp::config_file(), &row.id, ask);
                grokhub_agent::mcp::invalidate();
                res.map(|d| d.now)
            }
            (ChangeAct::Undo, ChangeKind::Automation) => {
                let path = crate::night::path();
                let res = hx::undo_change(&dir, &hx::AutomationsFile { path: &path, id: &row.id }, ask);
                if res.is_ok() {
                    // The cabin's list must match the file, or its next save undoes the undo.
                    self.automations = crate::night::load();
                }
                res.map(|d| d.now)
            }
        };
        if done.is_ok() {
            self.harness.work_rows.retain(|r| !(r.kind == row.kind && r.id == row.id));
        }
        self.harness.change_rows = None;
        self.harness.skill_rows = None;
        self.status = done_line(row, act, &done);
        done.is_ok()
    }

    /// Work-tree rows under the approval cards. Click only.
    pub(super) fn paint_work_rows(&mut self, ui: &mut egui::Ui) {
        if self.harness.work_rows.is_empty() {
            return;
        }
        ui.add_space(8.0);
        let rows = self.harness.work_rows.clone();
        if let Some((row, act)) = paint_change_rows(ui, "work-row", &rows, false) {
            self.change_row_clicked(&row, act);
        }
    }
}

/// A ghost pill that answers a pointer click only.
pub(super) fn click_pill(ui: &mut egui::Ui, label: &str) -> bool {
    let resp = crate::theme::felt_label_button(
        ui,
        label,
        Color32::TRANSPARENT,
        crate::theme::muted(),
        8.0,
        egui::vec2(0.0, ROW_H),
        Some(egui::Stroke::new(1.0_f32, crate::theme::border())),
        false,
    );
    let enabled = resp.enabled();
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, label));
    resp.clicked_by(egui::PointerButton::Primary)
}

/// One row per change: muted label, then ghost Undo and (when open) Keep.
/// `inset` lines rows up with a report bubble's text. Returns the clicked
/// row and pill. Typed text and keys never answer it.
pub(super) fn paint_change_rows(
    ui: &mut egui::Ui,
    salt: &str,
    rows: &[ChangeRow],
    inset: bool,
) -> Option<(ChangeRow, ChangeAct)> {
    let mut hit = None;
    let labels: Vec<&str> = rows.iter().map(|r| r.label.as_str()).collect();
    let pads = super::chat_ui::result_row_label_pads(ui, &labels);
    for (row, pad) in rows.iter().zip(pads) {
        ui.push_id((salt, row.kind.as_str(), row.id.as_str()), |ui| {
            let size = egui::vec2(ui.available_width(), ROW_H);
            ui.allocate_ui_with_layout(size, egui::Layout::left_to_right(egui::Align::Center), |ui| {
                if inset {
                    ui.add_space(super::chat_ui::RESULT_TEXT_INSET);
                }
                ui.label(
                    RichText::new(&row.label).size(super::chat_ui::RESULT_ROW_LABEL_SIZE).color(crate::theme::muted()),
                );
                ui.add_space(pad);
                if click_pill(ui, "Undo") {
                    hit = Some((row.clone(), ChangeAct::Undo));
                }
                if row.keep && click_pill(ui, "Keep") {
                    hit = Some((row.clone(), ChangeAct::Keep));
                }
            });
        });
    }
    if !rows.is_empty() {
        ui.add_space(4.0);
    }
    hit
}

#[cfg(test)]
mod tests {
    use super::*;

    fn job(id: &str, name: &str, every: u32) -> grokhub_core::Automation {
        grokhub_core::Automation {
            id: id.into(),
            name: name.into(),
            schedule: "heartbeat".into(),
            time: "09:00".into(),
            times: vec![],
            instructions: format!("{name} now"),
            heartbeat_every_min: every,
            check_command: String::new(),
            enabled: true,
            last_run: None,
            next_run: None,
            run_count: 0,
            health: Default::default(),
        }
    }

    fn fixture(label: &str) -> std::path::PathBuf {
        let root = crate::config::test_config_root(label);
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("automations.json");
        let save = |list: &[grokhub_core::Automation]| {
            std::fs::write(&path, serde_json::to_string_pretty(list).unwrap()).map_err(|e| e.to_string())
        };
        save(&[job("auto-a", "board digest", 30)]).unwrap();
        let t = hx::AutomationsFile { path: &path, id: "auto-a" };
        hx::record_change(&root, &t, Origin::SelfManage, "every 15 min instead", || save(&[job("auto-a", "board digest", 15)]))
            .unwrap();
        let t = hx::AutomationsFile { path: &path, id: "auto-b" };
        hx::record_change(&root, &t, Origin::SelfManage, "scheduled every 60 min", || {
            save(&[job("auto-a", "board digest", 15), job("auto-b", "inbox sweep", 60)])
        })
        .unwrap();
        let _ = hx::take_self_changes(&root);
        root
    }

    #[test]
    fn automation_report_and_rows_from_the_ledger() {
        let root = fixture("change-report");
        let ledger = hx::ChangeLedger::load_kind(&root, ChangeKind::Automation);
        let now = ledger.all().last().unwrap().at + 2 * 3_600_000;
        assert_eq!(
            changes_report(&ledger, now),
            [
                "/automations changes — what GrokHub changed in your automations",
                "",
                "- inbox sweep · added by GrokHub · scheduled every 60 min · 2h ago",
                "- board digest · changed by GrokHub · every 15 min instead · 2h ago",
                "",
                "Each automation keeps its last 20 versions. Undo puts back the one before. It runs only when you click it.",
                "GrokHub adds at most 2 automations a week on its own until you keep one.",
            ]
            .join("\n")
        );
        assert_eq!(
            report_rows(&ledger, now),
            vec![
                ChangeRow { kind: ChangeKind::Automation, id: "auto-b".into(), label: "inbox sweep · added 2h ago".into(), keep: true },
                ChangeRow { kind: ChangeKind::Automation, id: "auto-a".into(), label: "board digest · changed 2h ago".into(), keep: true },
            ]
        );
        let empty = changes_report(&hx::ChangeLedger::load_kind(&root, ChangeKind::Connection), now);
        assert!(
            empty.starts_with(CONNECTION_CHANGES_HEAD)
                && empty.contains("- Nothing yet. When GrokHub adds, changes, or removes a connection on its own, it shows here with Undo."),
            "{empty}"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Undo survives a restart: the Work-tree rows come back from the ledger
    /// on disk, newest first, and a kept change has no row.
    #[test]
    fn work_rows_come_back_from_disk() {
        let root = fixture("change-work-rows");
        let ledger = hx::ChangeLedger::load_kind(&root, ChangeKind::Automation);
        let now = ledger.all().last().unwrap().at + 60_000;
        let rows = work_rows_from_disk(&root, now, &|_, _| false);
        assert_eq!(
            rows.iter().map(|r| r.label.as_str()).collect::<Vec<_>>(),
            ["Grok added automation inbox sweep", "Grok changed automation board digest"]
        );
        hx::accept_change(&root, ChangeKind::Automation, "auto-b", hx::UndoAsk::from_click()).unwrap();
        let rows = work_rows_from_disk(&root, now, &|_, _| false);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, "auto-a");
        assert!(work_rows_from_disk(&root, now + WORK_ROW_MS, &|_, _| false).is_empty(), "a day later the row is gone");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn rows_paint_with_undo_and_keep_and_painting_never_acts() {
        let rows = vec![
            ChangeRow { kind: ChangeKind::Connection, id: "notes".into(), label: "Grok added connection notes".into(), keep: true },
            ChangeRow { kind: ChangeKind::Automation, id: "auto-a".into(), label: "board digest · changed 1m ago".into(), keep: false },
        ];
        let ctx = egui::Context::default();
        crate::theme::install_fonts_on(&ctx);
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1000.0, 600.0))),
            ..Default::default()
        };
        let mut hit = None;
        let out = crate::theme::test_pass(&ctx, input, |ui| {
            egui::CentralPanel::default().show(ui, |ui| {
                hit = paint_change_rows(ui, "t", &rows, false);
            });
        });
        let mut texts = Vec::new();
        fn walk(shape: &egui::Shape, out: &mut Vec<String>) {
            match shape {
                egui::Shape::Text(t) => out.push(t.galley.text().to_string()),
                egui::Shape::Vec(v) => v.iter().for_each(|c| walk(c, out)),
                _ => {}
            }
        }
        for clipped in &out.shapes {
            walk(&clipped.shape, &mut texts);
        }
        assert_eq!(texts.iter().filter(|t| *t == "Undo").count(), 2, "{texts:?}");
        assert_eq!(texts.iter().filter(|t| *t == "Keep").count(), 1, "{texts:?}");
        assert!(texts.iter().any(|t| t == "Grok added connection notes"), "{texts:?}");
        assert_eq!(hit, None);
    }

    #[test]
    fn newest_report_and_label_names() {
        let view = |body: &str| grokhub_core::ChatView {
            kind: grokhub_core::ChatKind::Result,
            title: String::new(),
            body: body.into(),
        };
        let views = [view(AUTOMATION_CHANGES_HEAD), view(CONNECTION_CHANGES_HEAD), view("other")];
        assert_eq!(newest_change_report(&views), Some((1, ChangeKind::Connection)));
        assert_eq!(newest_change_report(&views[..1]), Some((0, ChangeKind::Automation)));
        let row = ChangeRow { kind: ChangeKind::Connection, id: "notes".into(), label: "Grok added connection notes".into(), keep: true };
        assert_eq!(row.label_name(), "notes");
        let row = ChangeRow { kind: ChangeKind::Automation, id: "auto-a".into(), label: "board digest · changed 1m ago".into(), keep: true };
        assert_eq!(row.label_name(), "board digest");
        assert_eq!(done_line(&row, ChangeAct::Undo, &Ok(Some(vec![]))), "Undid GrokHub's change to the automation board digest.");
        assert_eq!(done_line(&row, ChangeAct::Keep, &Ok(Some(vec![]))), "Kept the automation board digest.");
    }
}
