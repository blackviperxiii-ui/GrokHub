//! Findings card (card 34 PR C): the agent's `report_findings` lands as a
//! result bubble after the reply, and the newest one gets a pill per fix.
//! A tapped fix starts a follow-up run in the same chat, through the normal
//! gates. Dismiss hides the pills quietly; the report stays. With two or more
//! independent fixes a Do all pill runs them one after another, then asks for
//! one summary of what got done.

use eframe::egui;
use grokhub_core::findings::{Findings, DO_ALL, FINDINGS_HEAD};

use super::Cabin;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum FindingsAct {
    Run(String),
    DoAll,
    Dismiss,
}

pub(super) const DISMISS: &str = "Dismiss";

/// The newest findings bubble among the painted rows, with its card, unless
/// it was dismissed or has no fixes. After Do all ran on it, only the fixes
/// Do all left for their own tap stay.
pub(super) fn newest_findings_row(
    views: &[grokhub_core::ChatView],
    dismissed: &[String],
    batched: &[String],
) -> Option<(usize, Findings)> {
    let i = views.iter().rposition(|v| {
        v.kind == grokhub_core::ChatKind::Result && v.body.starts_with(FINDINGS_HEAD)
    })?;
    let body = &views[i].body;
    if dismissed.iter().any(|d| d == body) {
        return None;
    }
    let mut card = Findings::parse(body)?;
    if batched.iter().any(|b| b == body) {
        card.fixes = card.do_all().map(|b| b.skipped).unwrap_or_default();
    }
    (!card.fixes.is_empty()).then_some((i, card))
}

/// Hover text for Do all: the order it runs in, and what it leaves out.
pub(super) fn do_all_hint(card: &Findings) -> Option<String> {
    let batch = card.do_all()?;
    let names = |fixes: &[grokhub_core::findings::Fix]| {
        fixes
            .iter()
            .map(|f| f.label.as_str())
            .collect::<Vec<_>>()
            .join(", then ")
    };
    let mut hint = format!("Runs {} one at a time, then sums up.", names(&batch.run));
    if !batch.skipped.is_empty() {
        let left: Vec<&str> = batch.skipped.iter().map(|f| f.label.as_str()).collect();
        hint.push_str(&format!(
            " Skips {}: it deletes, overwrites or sends something, so it needs its own tap.",
            left.join(" and ")
        ));
    }
    Some(hint)
}

/// Do all (when the fixes can run as a batch), one pill per fix, then a
/// ghost Dismiss. Pointer clicks only: a stray Enter never starts a run.
pub(super) fn paint_findings_fixes(ui: &mut egui::Ui, card: &Findings) -> Option<FindingsAct> {
    let mut hit = None;
    ui.horizontal_wrapped(|ui| {
        ui.add_space(super::chat_ui::result_text_inset());
        if let Some(hint) = do_all_hint(card) {
            let id = ui.id().with("findings-do-all");
            if crate::cards::felt_pill_at(ui, Some(id), DO_ALL, crate::cards::PillStyle::Solid)
                .on_hover_text(hint)
                .clicked_by(egui::PointerButton::Primary)
            {
                hit = Some(FindingsAct::DoAll);
            }
        }
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
            FindingsAct::DoAll => {
                let Some(batch) = Findings::parse(body).and_then(|c| c.do_all()) else {
                    return;
                };
                // Only from idle: then a run that is live after the send can
                // only be the first fix's, so the rest are safe to queue.
                if self.running || self.pending_kick.is_some() {
                    self.status = format!("{DO_ALL} waits for this run to finish");
                    return;
                }
                let mut steps = batch.prompts().into_iter();
                let Some(first) = steps.next() else {
                    return;
                };
                self.send_from_composer(first);
                if self.queue_do_all_rest(steps.collect(), batch.run.len()) {
                    self.harness.findings_batched.push(body.to_string());
                }
            }
            FindingsAct::Dismiss => self.harness.findings_dismissed.push(body.to_string()),
        }
    }

    /// After Do all's first fix: the rest wait in the queue, so each shows
    /// as its own row until its turn. A send that never started a run (no
    /// sign-in, a refused gate) drops them instead of firing them later.
    /// True when the batch is under way.
    fn queue_do_all_rest(&mut self, rest: Vec<String>, fixes: usize) -> bool {
        if !self.running && self.pending_kick.is_none() {
            return false;
        }
        self.followup_queue.extend(rest);
        self.status = format!("{DO_ALL}: 1 of {fixes} running");
        true
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
        let old = Findings {
            fixes: Vec::new(),
            ..card()
        };
        let views = vec![
            view(ChatKind::User, "scan my computer"),
            view(ChatKind::Result, &card().to_body()),
            view(ChatKind::Assistant, &card().to_body()),
            view(ChatKind::Result, &old.to_body()),
        ];
        // The newest card has no fixes, so nothing to tap.
        assert_eq!(newest_findings_row(&views, &[], &[]), None);
        assert_eq!(
            newest_findings_row(&views[..3], &[], &[]),
            Some((1, card()))
        );
        // An assistant reply that quotes the head is not a card.
        assert_eq!(newest_findings_row(&views[2..3], &[], &[]), None);
        assert_eq!(
            newest_findings_row(&views[..3], &[card().to_body()], &[]),
            None
        );
    }

    #[test]
    fn a_fix_runs_its_goal_in_the_same_chat_and_dismiss_is_quiet() {
        let mut cabin = Cabin::quiet_for_test();
        cabin
            .threads
            .push(crate::threads::ChatThread::new("Chat", false));
        cabin.thread_idx = cabin.threads.len() - 1;
        let id = cabin.visible_thread_id();
        let body = card().to_body();
        cabin.post_findings(Some(id), &body);
        assert_eq!(
            cabin
                .messages
                .last()
                .map(|m| (m.0.as_str(), grokhub_core::strip_slash_result(&m.1))),
            Some(("assistant", body.as_str()))
        );
        cabin.findings_clicked(FindingsAct::Dismiss, &body);
        assert_eq!(
            cabin.harness.findings_dismissed,
            std::slice::from_ref(&body)
        );
        // A fix goes down the typed-send path; with no engine here, that path
        // stops at the sign-in line instead of writing a turn.
        cabin.harness.pending_findings = Some(body.clone());
        cabin.findings_clicked(FindingsAct::Run(card().fixes[0].prompt()), &body);
        // Starting the next run drops a card left by one that never finished.
        assert_eq!(cabin.harness.pending_findings, None);
        assert_eq!(cabin.status, grokhub_core::XAI_NEED_SIGNIN);
        assert_eq!(
            cabin.harness.findings_dismissed,
            [body.clone(), body.clone()]
        );
    }

    fn batch_card() -> Findings {
        Findings::from_json(&serde_json::json!({
            "findings": [{"severity": "high", "text": "nextdns can't bind port 53"}],
            "fixes": [
                {"label": "Explain all", "goal": "Explain each finding in plain words."},
                {"label": "Fix port 53 conflict", "goal": "Free port 53 for nextdns."},
                {"label": "Delete old journal logs", "goal": "Delete journal logs older than a week."},
                {"label": "Reinstall 6 packages", "goal": "Reinstall the 6 packages with missing files."}
            ]
        }))
        .unwrap()
    }

    /// The text one 1100x720 frame of `add` paints, in paint order.
    fn painted(mut add: impl FnMut(&mut egui::Ui)) -> Vec<String> {
        let ctx = egui::Context::default();
        crate::theme::install_fonts_on(&ctx);
        let raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1100.0, 720.0),
            )),
            ..Default::default()
        };
        let out = crate::theme::test_pass(&ctx, raw, |ui| add(ui));
        out.shapes
            .iter()
            .filter_map(|c| match &c.shape {
                egui::Shape::Text(t) => Some(t.galley.job.text.clone()),
                _ => None,
            })
            .collect()
    }

    fn pill_texts(card: &Findings) -> Vec<String> {
        painted(|ui| {
            let _ = paint_findings_fixes(ui, card);
        })
    }

    #[test]
    fn do_all_leads_the_pills_only_when_the_fixes_can_batch() {
        assert_eq!(
            pill_texts(&batch_card()),
            [
                "Do all",
                "Explain all",
                "Fix port 53 conflict",
                "Delete old journal logs",
                "Reinstall 6 packages",
                "Dismiss"
            ]
        );
        assert_eq!(
            do_all_hint(&batch_card()).as_deref(),
            Some(
                "Runs Fix port 53 conflict, then Reinstall 6 packages, then Explain all one at a time, \
then sums up. Skips Delete old journal logs: it deletes, overwrites or sends something, so it \
needs its own tap."
            )
        );
        // Alternatives: pick one, so no Do all.
        let pick = Findings::from_json(&serde_json::json!({
            "findings": [{"severity": "medium", "text": "two resolvers fight over port 53"}],
            "fixes": [
                {"label": "Use NextDNS", "goal": "Point DNS at NextDNS."},
                {"label": "Use systemd-resolved", "goal": "Keep systemd-resolved."}
            ]
        }))
        .unwrap();
        assert_eq!(
            pill_texts(&pick),
            ["Use NextDNS", "Use systemd-resolved", "Dismiss"]
        );
        assert_eq!(do_all_hint(&pick), None);
        assert_eq!(
            do_all_hint(&card()),
            Some("Runs Fix port 53 conflict, then Explain all one at a time, then sums up.".into())
        );
    }

    #[test]
    fn do_all_queues_each_fix_then_the_summary_and_drops_them_if_nothing_ran() {
        let mut cabin = Cabin::quiet_for_test();
        cabin
            .threads
            .push(crate::threads::ChatThread::new("Chat", false));
        cabin.thread_idx = cabin.threads.len() - 1;
        let body = batch_card().to_body();
        // No engine: the first fix stops at the sign-in line, so nothing waits
        // in the queue to fire later, and the card keeps its pills to retry.
        cabin.findings_clicked(FindingsAct::DoAll, &body);
        assert_eq!(cabin.status, grokhub_core::XAI_NEED_SIGNIN);
        assert!(
            cabin.followup_queue.is_empty(),
            "{:?}",
            cabin.followup_queue
        );
        assert!(cabin.harness.findings_dismissed.is_empty());
        assert!(cabin.harness.findings_batched.is_empty());
        // A run already live (an idea talk, say): the send could be refused
        // while `running` stays true, so Do all doesn't start at all rather
        // than queue 2/3 onward without 1/3.
        cabin.running = true;
        cabin.findings_clicked(FindingsAct::DoAll, &body);
        assert_eq!(cabin.status, "Do all waits for this run to finish");
        assert!(cabin.followup_queue.is_empty());
        assert!(cabin.harness.findings_batched.is_empty());
        // A run that did start: the rest queue in order, summary last.
        let steps = batch_card().do_all().unwrap().prompts();
        assert!(cabin.queue_do_all_rest(steps[1..].to_vec(), 3));
        assert_eq!(
            cabin.followup_queue,
            [
                "Do all 2/3 · Reinstall 6 packages: Reinstall the 6 packages with missing files.",
                "Do all 3/3 · Explain all: Explain each finding in plain words.",
                "Do all summary · In a few lines, say which of these got done and which didn't, and why: \
Fix port 53 conflict, Reinstall 6 packages, Explain all. Left for their own tap because they \
delete, overwrite or send something: Delete old journal logs.",
            ]
        );
        assert_eq!(cabin.status, "Do all: 1 of 3 running");
        // Each waiting fix paints as its own Queued row while the run works.
        let texts = painted(|ui| cabin.paint_live_work(ui));
        assert!(
            texts.contains(
                &"Do all 2/3 · Reinstall 6 packages: Reinstall the 6 packages with".to_string()
            ) && texts.contains(
                &"Do all 3/3 · Explain all: Explain each finding in plain words.".to_string()
            ),
            "{texts:?}"
        );
        // When the turn ends, the next fix goes out and leaves the queue.
        cabin.running = false;
        cabin.drain_followup_queue();
        assert_eq!(cabin.followup_queue.len(), 2);
        assert!(cabin.followup_queue[0].starts_with("Do all 3/3"));
    }

    #[test]
    fn after_do_all_only_the_fixes_it_left_out_keep_a_pill() {
        let body = batch_card().to_body();
        let views = vec![view(ChatKind::Result, &body)];
        let batched = [body.clone()];
        let (i, left) = newest_findings_row(&views, &[], &batched).expect("the risky fix stays");
        assert_eq!(i, 0);
        assert_eq!(pill_texts(&left), ["Delete old journal logs", "Dismiss"]);
        // A batch with nothing left out leaves no pills at all.
        let all_safe = card().to_body();
        assert_eq!(
            newest_findings_row(
                &[view(ChatKind::Result, &all_safe)],
                &[],
                std::slice::from_ref(&all_safe)
            ),
            None
        );
    }
}
