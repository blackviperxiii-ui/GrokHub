//! Findings card (card 34 PR C): the agent's `report_findings` lands as a
//! result bubble after the reply, and the newest one gets a pill per fix.
//! A tapped fix starts a follow-up run in the same chat, through the normal
//! gates. Dismiss hides the pills quietly; the report stays.

use eframe::egui;
use grokhub_core::findings::{Findings, FINDINGS_HEAD};

use super::Cabin;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum FindingsAct {
    Run(String),
    Dismiss,
}

pub(super) const DISMISS: &str = "Dismiss";

/// The newest findings bubble among the painted rows, with its card, unless
/// it was dismissed or has no fixes.
pub(super) fn newest_findings_row(
    views: &[grokhub_core::ChatView],
    dismissed: &[String],
) -> Option<(usize, Findings)> {
    let i = views
        .iter()
        .rposition(|v| v.kind == grokhub_core::ChatKind::Result && v.body.starts_with(FINDINGS_HEAD))?;
    let body = &views[i].body;
    if dismissed.iter().any(|d| d == body) {
        return None;
    }
    Findings::parse(body).filter(|f| !f.fixes.is_empty()).map(|f| (i, f))
}

/// One pill per fix, then a ghost Dismiss. Pointer clicks only: a stray
/// Enter never starts a run.
pub(super) fn paint_findings_fixes(ui: &mut egui::Ui, card: &Findings) -> Option<FindingsAct> {
    let mut hit = None;
    ui.horizontal_wrapped(|ui| {
        ui.add_space(super::chat_ui::RESULT_TEXT_INSET);
        for (n, fix) in card.fixes.iter().enumerate() {
            let id = ui.id().with(("findings-fix", n));
            if crate::cards::felt_pill_at(ui, Some(id), &fix.label, crate::cards::PillStyle::Solid)
                .clicked_by(egui::PointerButton::Primary)
            {
                hit = Some(FindingsAct::Run(fix.prompt()));
            }
        }
        let id = ui.id().with("findings-dismiss");
        if crate::cards::felt_pill_at(ui, Some(id), DISMISS, crate::cards::PillStyle::Ghost)
            .clicked_by(egui::PointerButton::Primary)
        {
            hit = Some(FindingsAct::Dismiss);
        }
    });
    ui.add_space(4.0);
    hit
}

impl Cabin {
    /// After the turn's reply: the findings bubble goes on the chat the run
    /// belonged to (`origin`), even when another tab is open now.
    pub(super) fn post_findings(&mut self, origin: Option<String>, body: &str) {
        let saved = std::mem::replace(&mut self.chat_job_thread, origin);
        self.push_bound_msg("assistant", grokhub_core::mark_slash_result(body));
        self.chat_job_thread = saved;
        self.persist();
    }

    pub(super) fn findings_clicked(&mut self, act: FindingsAct, body: &str) {
        match act {
            FindingsAct::Run(prompt) => {
                self.harness.findings_dismissed.push(body.to_string());
                self.send_from_composer(prompt);
            }
            FindingsAct::Dismiss => self.harness.findings_dismissed.push(body.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use grokhub_core::{ChatKind, ChatView};

    fn view(kind: ChatKind, body: &str) -> ChatView {
        ChatView { kind, title: String::new(), body: body.to_string() }
    }

    fn card() -> Findings {
        Findings::from_json(&serde_json::json!({
            "findings": [{"severity": "high", "text": "nextdns can't bind port 53: systemd-resolved owns it"}],
            "fixes": [
                {"label": "Fix port 53 conflict", "goal": "Free port 53 for nextdns."},
                {"label": "Explain all", "goal": "Explain each finding."}
            ]
        }))
        .unwrap()
    }

    #[test]
    fn the_newest_findings_bubble_with_fixes_gets_the_pills() {
        let old = Findings { fixes: Vec::new(), ..card() };
        let views = vec![
            view(ChatKind::User, "scan my computer"),
            view(ChatKind::Result, &card().to_body()),
            view(ChatKind::Assistant, &card().to_body()),
            view(ChatKind::Result, &old.to_body()),
        ];
        // The newest card has no fixes, so nothing to tap.
        assert_eq!(newest_findings_row(&views, &[]), None);
        assert_eq!(newest_findings_row(&views[..3], &[]), Some((1, card())));
        // An assistant reply that quotes the head is not a card.
        assert_eq!(newest_findings_row(&views[2..3], &[]), None);
        assert_eq!(newest_findings_row(&views[..3], &[card().to_body()]), None);
    }

    #[test]
    fn a_fix_runs_its_goal_in_the_same_chat_and_dismiss_is_quiet() {
        let mut cabin = Cabin::quiet_for_test();
        cabin.threads.push(crate::threads::ChatThread::new("Chat", false));
        cabin.thread_idx = cabin.threads.len() - 1;
        let id = cabin.visible_thread_id();
        let body = card().to_body();
        cabin.post_findings(Some(id), &body);
        assert_eq!(
            cabin.messages.last().map(|m| (m.0.as_str(), grokhub_core::strip_slash_result(&m.1))),
            Some(("assistant", body.as_str()))
        );
        cabin.findings_clicked(FindingsAct::Dismiss, &body);
        assert_eq!(cabin.harness.findings_dismissed, std::slice::from_ref(&body));
        // A fix goes down the typed-send path; with no engine here, that path
        // stops at the sign-in line instead of writing a turn.
        cabin.harness.pending_findings = Some(body.clone());
        cabin.findings_clicked(FindingsAct::Run(card().fixes[0].prompt()), &body);
        // Starting the next run drops a card left by one that never finished.
        assert_eq!(cabin.harness.pending_findings, None);
        assert_eq!(cabin.status, grokhub_core::XAI_NEED_SIGNIN);
        assert_eq!(cabin.harness.findings_dismissed, [body.clone(), body.clone()]);
    }
}
