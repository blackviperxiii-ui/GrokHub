//! Evidence panel: a compact proof receipt under a finished turn.
//! Derived from harness spans / outcome (see `grokhub_agent::harness::run_evidence`).
//! Not Pulse `TopicEvidence`, not detector `Evidence` quotes.

use eframe::egui::{self, RichText};
use grokhub_agent::harness::{self as hx, RunEvidence};

use super::Cabin;

/// Newest Assistant or Result row that can host the Evidence panel.
pub(super) fn evidence_host_row(views: &[grokhub_core::ChatView]) -> Option<usize> {
    views.iter().rposition(|v| {
        matches!(
            v.kind,
            grokhub_core::ChatKind::Assistant | grokhub_core::ChatKind::Result
        )
    })
}

/// Collapsing Evidence receipt. Default closed; empty sections say unavailable.
pub(super) fn paint_evidence_panel(ui: &mut egui::Ui, ev: &RunEvidence) {
    let title = hx::evidence_summary(ev);
    ui.add_space(2.0);
    egui::CollapsingHeader::new(
        RichText::new(title)
            .size(crate::theme::FONT_META)
            .color(crate::theme::muted()),
    )
    .id_salt(("evidence", ev.chat_id.as_str(), ev.turn))
    .default_open(false)
    .show(ui, |ui| {
        ui.add_space(2.0);
        ui.set_max_width(
            ui.available_width()
                .min(crate::markdown::bubble_width(ui.available_width())),
        );
        let goal = goal_line(ev);
        section(ui, "Goal", std::slice::from_ref(&goal));
        section(ui, "Plan status", &ev.plan);
        section(ui, "Actions", &ev.actions);
        section(ui, "Checks", &ev.checks);
        section(ui, "Changes", &ev.changes);
        section(ui, "Confidence", std::slice::from_ref(&ev.confidence));
        if ev.tool_ran_not_verified {
            ui.label(
                RichText::new("A successful tool is not proof the goal succeeded.")
                    .size(crate::theme::FONT_META)
                    .color(crate::theme::muted()),
            );
        }
        section(ui, "Recovery", std::slice::from_ref(&ev.recovery));
        if !ev.goal_detail.is_empty() && ev.goal != "unavailable" {
            ui.label(
                RichText::new(format!("Detail: {}", ev.goal_detail))
                    .size(crate::theme::FONT_META)
                    .color(crate::theme::muted()),
            );
        }
    });
    ui.add_space(4.0);
}

fn goal_line(ev: &RunEvidence) -> String {
    if ev.goal == "unavailable" {
        "unavailable".into()
    } else if ev.goal_detail.is_empty() {
        ev.goal.clone()
    } else {
        format!("{} ({})", ev.goal, ev.goal_detail)
    }
}

fn section(ui: &mut egui::Ui, title: &str, lines: &[String]) {
    ui.label(
        RichText::new(title)
            .size(crate::theme::FONT_META)
            .color(crate::theme::subtle()),
    );
    if lines.is_empty() {
        ui.label(
            RichText::new("unavailable")
                .size(crate::theme::FONT_META)
                .color(crate::theme::muted()),
        );
    } else {
        for line in lines {
            ui.label(
                RichText::new(format!("· {line}"))
                    .size(crate::theme::FONT_META)
                    .color(crate::theme::muted()),
            );
        }
    }
    ui.add_space(2.0);
}

impl Cabin {
    /// Rebuild the open chat's Evidence cache from spans after turn end, or
    /// when the visible chat changed and the cache is stale.
    pub(super) fn refresh_run_evidence(&mut self, chat_id: &str, turn: u32) {
        let dir = crate::config::config_dir();
        let (spans, _) = hx::read_spans_tail(&dir, chat_id, hx::SPAN_SEARCH_LINES);
        let outcome = self
            .harness
            .self_review
            .last_outcome
            .as_ref()
            .filter(|o| o.task == format!("{chat_id}:{turn}"));
        let recovery = self
            .harness
            .soft_parks
            .iter()
            .find(|p| p.chat_id == chat_id)
            .map(|p| p.reason.as_str());
        self.harness.evidence = hx::run_evidence(&spans, chat_id, turn, outcome, recovery);
        self.harness.evidence_checked = Some(chat_id.to_string());
    }

    /// If the visible chat has harness steps but no matching cache, load it
    /// so Evidence survives app restart (derived from spans on disk).
    pub(super) fn ensure_run_evidence(&mut self) {
        let chat = self.trace_id();
        if chat.is_empty() || self.scratch() {
            return;
        }
        if self
            .harness
            .evidence
            .as_ref()
            .is_some_and(|e| e.chat_id == chat)
        {
            return;
        }
        if self.harness.evidence_checked.as_ref() == Some(&chat) {
            return;
        }
        let dir = crate::config::config_dir();
        let (spans, _) = hx::read_spans_tail(&dir, &chat, hx::SPAN_SEARCH_LINES);
        self.harness.evidence_checked = Some(chat.clone());
        let Some(turn) = hx::latest_evidence_turn(&spans, &chat) else {
            return;
        };
        let outcome = self
            .harness
            .self_review
            .last_outcome
            .as_ref()
            .filter(|o| o.task == format!("{chat}:{turn}"));
        let recovery = self
            .harness
            .soft_parks
            .iter()
            .find(|p| p.chat_id == chat)
            .map(|p| p.reason.as_str());
        self.harness.evidence = hx::run_evidence(&spans, &chat, turn, outcome, recovery);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use grokhub_core::{ChatKind, ChatView};

    fn view(kind: ChatKind, body: &str) -> ChatView {
        ChatView {
            kind,
            title: String::new(),
            body: body.to_string(),
        }
    }

    #[test]
    fn evidence_hosts_on_the_newest_assistant_or_result() {
        let views = vec![
            view(ChatKind::User, "do it"),
            view(ChatKind::Assistant, "working"),
            view(ChatKind::Result, "Findings"),
            view(ChatKind::Thought, "secret"),
        ];
        assert_eq!(evidence_host_row(&views), Some(2));
        assert_eq!(evidence_host_row(&views[..2]), Some(1));
        assert_eq!(evidence_host_row(&[]), None);
    }

    #[test]
    fn refresh_caches_evidence_for_the_turn() {
        let root = crate::config::test_config_root("evidence-refresh");
        let _ = std::fs::remove_dir_all(&root);
        unsafe { std::env::set_var("GROKHUB_CONFIG", &root) };
        let mut cabin = Cabin::quiet_for_test();
        let chat = "ev-chat";
        let mut click = hx::Span::soft_allow(
            chat,
            "click",
            r#"{"x":1}"#,
            "ok",
            "click",
            hx::AccessMode::Supervised,
            "grok_build",
        );
        click.chat_id = chat.into();
        click.turn = 1;
        click.ts_ms = 10;
        hx::append_span(&root, &click).unwrap();
        cabin.refresh_run_evidence(chat, 1);
        let ev = cabin.harness.evidence.clone().expect("evidence");
        drop(cabin);
        let _ = std::fs::remove_dir_all(&root);
        unsafe { std::env::remove_var("GROKHUB_CONFIG") };
        assert_eq!(ev.chat_id, chat);
        assert_eq!(ev.turn, 1);
        assert_eq!(ev.confidence, "inferred");
        assert!(ev.tool_ran_not_verified);
    }
}
