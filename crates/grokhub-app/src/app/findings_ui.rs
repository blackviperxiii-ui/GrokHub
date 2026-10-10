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
        cabin.findings_clicked(FindingsAct::Run(card().fixes[0].prompt()), &body);
        assert_eq!(cabin.status, "Install Grok Build (x.ai/cli) or Connect Grok in Settings");
        assert_eq!(cabin.harness.findings_dismissed, [body.clone(), body.clone()]);
    }

    fn pinned(label: &str) -> (crate::config::TestConfigDir, std::path::PathBuf) {
        let root = crate::config::test_config_root(label);
        let _ = std::fs::create_dir_all(&root);
        (crate::config::TestConfigDir::set(root.clone()), root)
    }

    fn open_chat(label: &str) -> (crate::config::TestConfigDir, Cabin, String) {
        let (pin, _root) = pinned(label);
        let mut cabin = Cabin::quiet_for_test();
        cabin.threads.push(crate::threads::ChatThread::new("Chat", false));
        cabin.thread_idx = cabin.threads.len() - 1;
        let id = cabin.visible_thread_id();
        cabin.chat_job_thread = Some(id.clone());
        (pin, cabin, id)
    }

    fn attach(cabin: &mut Cabin) -> std::sync::mpsc::Sender<grokhub_acp::AcpEvent> {
        let (handle, _cmds, tx) =
            grokhub_acp::AcpHandle::external(std::env::temp_dir(), "sess-findings".into());
        cabin.acp = Some(handle);
        tx
    }

    fn finding_bodies(cabin: &Cabin) -> Vec<String> {
        cabin
            .messages
            .iter()
            .filter(|m| m.1.contains("nextdns can't bind port 53"))
            .map(|m| m.1.clone())
            .collect()
    }

    /// The next reply after an abort. Posts whatever card is still pending.
    fn finish_next_reply(
        cabin: &mut Cabin,
        id: &str,
        tx: &std::sync::mpsc::Sender<grokhub_acp::AcpEvent>,
    ) {
        cabin.running = true;
        cabin.chat_job_thread = Some(id.to_string());
        tx.send(grokhub_acp::AcpEvent::Text("newer reply".into())).unwrap();
        tx.send(grokhub_acp::AcpEvent::Done {
            stop_reason: "end_turn".into(),
        })
        .unwrap();
        cabin.poll_acp();
    }

    #[test]
    fn a_stopped_turn_does_not_post_its_findings_under_the_next_reply() {
        let (_pin, mut cabin, id) = open_chat("findings-halt");
        let body = card().to_body();
        cabin.running = true;
        cabin.harness.pending_findings = Some(body.clone());
        cabin.halt_in_flight();
        assert_eq!(cabin.harness.pending_findings, None);

        let tx = attach(&mut cabin);
        cabin.running = false;
        tx.send(grokhub_acp::AcpEvent::Findings(body)).unwrap();
        cabin.poll_acp();
        assert_eq!(cabin.harness.pending_findings, None);

        finish_next_reply(&mut cabin, &id, &tx);
        assert_eq!(finding_bodies(&cabin), Vec::<String>::new());
        assert!(
            cabin.messages.iter().any(|m| m.1.contains("newer reply")),
            "the next reply still lands: {:?}",
            cabin.messages.iter().map(|m| m.1.as_str()).collect::<Vec<_>>()
        );
    }

    #[test]
    fn a_fatal_or_sigterm_turn_does_not_post_its_findings_under_the_next_reply() {
        let (_pin, mut cabin, id) = open_chat("findings-err");
        let body = card().to_body();

        cabin.running = true;
        let tx = attach(&mut cabin);
        tx.send(grokhub_acp::AcpEvent::Findings(body.clone())).unwrap();
        tx.send(grokhub_acp::AcpEvent::Err("model exploded".into())).unwrap();
        cabin.poll_acp();
        assert_eq!(cabin.harness.pending_findings, None);
        assert_eq!(finding_bodies(&cabin), Vec::<String>::new());

        let tx = attach(&mut cabin);
        finish_next_reply(&mut cabin, &id, &tx);
        assert_eq!(finding_bodies(&cabin), Vec::<String>::new());

        // The one automatic SIGTERM retry starts a new turn. The old card
        // must not ride along, and kick must not spawn a real agent.
        cabin.running = true;
        cabin.turn_retried = false;
        cabin.chat_job_thread = Some(id.clone());
        let (_kick_tx, kick_rx) = std::sync::mpsc::channel();
        cabin.acp_spawn_rx = Some(kick_rx);
        let tx = attach(&mut cabin);
        tx.send(grokhub_acp::AcpEvent::Findings(body)).unwrap();
        tx.send(grokhub_acp::AcpEvent::Err("exit 143".into())).unwrap();
        cabin.poll_acp();
        assert_eq!(cabin.harness.pending_findings, None);
        assert_eq!(finding_bodies(&cabin), Vec::<String>::new());
        cabin.acp_spawn_rx = None;
        cabin.pending_kick = None;

        let tx = attach(&mut cabin);
        finish_next_reply(&mut cabin, &id, &tx);
        assert_eq!(finding_bodies(&cabin), Vec::<String>::new());
    }

    #[test]
    fn a_transient_error_keeps_this_turns_findings() {
        let (_pin, mut cabin, _id) = open_chat("findings-transient");
        let body = card().to_body();
        let marked = grokhub_core::mark_slash_result(&body);
        cabin.running = true;
        let tx = attach(&mut cabin);
        tx.send(grokhub_acp::AcpEvent::Findings(body.clone())).unwrap();
        tx.send(grokhub_acp::AcpEvent::Err("connection reset".into())).unwrap();
        cabin.poll_acp();
        assert_eq!(cabin.harness.pending_findings.as_deref(), Some(body.as_str()));
        tx.send(grokhub_acp::AcpEvent::Done { stop_reason: "end_turn".into() }).unwrap();
        cabin.poll_acp();
        assert_eq!(finding_bodies(&cabin), vec![marked]);
    }
}
