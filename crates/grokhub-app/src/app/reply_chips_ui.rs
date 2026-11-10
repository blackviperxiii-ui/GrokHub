//! Predicted reply chips on cards that wait on the person: the plan card
//! and the agent's questions. The chips come from
//! [`grokhub_core::reply_chips`]; this file paints them and acts on a tap.
//! Safety approvals keep their own Allow / Deny buttons and are not here.

use super::*;
use grokhub_core::reply_chips::{self, ReplyAct, ReplyChip};

/// The plan's approval card name, from the agent's `exit_plan_mode`.
pub(super) const PLAN_ELICIT_SERVER: &str = "Plan";
/// Status after Not now on a plan with no turn running.
pub(super) const PLAN_SET_ASIDE: &str = "Plan set aside. Your next message gets a straight answer";
/// Status after a reply on the agent's plan card.
const PLAN_REPLY_SENT: &str = "Reply sent to the plan";
const REPLY_HINT: &str = "Or type your own reply";

/// One row of reply chips; the first is the primary one. Returns the tapped chip.
fn paint_reply_row(ui: &mut egui::Ui, chips: &[ReplyChip]) -> Option<ReplyChip> {
    let mut picked = None;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(8.0, 8.0);
        for (i, chip) in chips.iter().enumerate() {
            let hit = if i == 0 {
                crate::cards::white_pill(ui, &chip.label)
            } else {
                crate::cards::ghost_pill(ui, &chip.label)
            };
            if hit {
                picked = Some(chip.clone());
            }
        }
    });
    picked
}

/// The plan waits on the person with no turn running: chips under it answer it.
pub(super) fn plan_waits_off_turn(
    running: bool,
    mode: SessionMode,
    plan: &str,
    agent_card: bool,
) -> bool {
    !running && mode == SessionMode::Plan && !plan.trim().is_empty() && !agent_card
}

/// A question's text without the option list the agent wrote out for a
/// typed answer: the options are chips here.
pub(super) fn question_text(p: &grokhub_core::wire::ElicitAsk) -> String {
    let text = p.message.trim();
    if p.options.is_empty() {
        return text.to_string();
    }
    text.lines()
        .filter(|line| !line.starts_with("- ") && line.trim() != "Reply with the option label.")
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string()
}

impl Cabin {
    fn note_reply_pick(&mut self, chip: &ReplyChip) {
        reply_chips::remember_reply_pick(&mut self.chip_memory, &chip.key, now_ms());
        self.flush_chips();
    }

    fn agent_plan_card_open(&self) -> bool {
        self.elicit_ask
            .as_ref()
            .is_some_and(|p| p.server_name == PLAN_ELICIT_SERVER)
    }

    /// Chips under the plan card once the turn has ended and the plan waits.
    pub(super) fn paint_plan_reply_chips(&mut self, ui: &mut egui::Ui, plan: &str) {
        if !plan_waits_off_turn(
            self.running,
            self.session_mode,
            plan,
            self.agent_plan_card_open(),
        ) {
            return;
        }
        let chips = reply_chips::plan_reply_chips(plan, &self.chip_memory);
        ui.add_space(8.0);
        if let Some(chip) = paint_reply_row(ui, &chips) {
            self.take_plan_reply(chip);
        }
    }

    /// One tap on a plan reply with no turn running. A go leaves plan mode
    /// through the turn reader; Not now sets the plan aside without a call.
    pub(super) fn take_plan_reply(&mut self, chip: ReplyChip) {
        self.note_reply_pick(&chip);
        match chip.act {
            ReplyAct::Send(text) => self.send_chat(text),
            ReplyAct::Prefill(text) => {
                self.composer = text;
                self.composer_chip_text = self.composer.clone();
                self.composer_want_focus = true;
            }
            ReplyAct::Dismiss => {
                self.set_session_mode(SessionMode::Chat);
                self.plan_open = false;
                self.status = PLAN_SET_ASIDE.into();
            }
        }
    }

    /// The agent's plan approval, mid-turn: the plan's reply chips and a
    /// reply box. Every reply goes back into the same turn; only a go runs it.
    pub(super) fn paint_plan_elicit(
        &mut self,
        ui: &mut egui::Ui,
        p: &grokhub_core::wire::ElicitAsk,
    ) {
        let shown_above = self
            .threads
            .get(self.thread_idx)
            .is_some_and(|t| t.plan_body.trim() == p.message.trim());
        ui.label(
            RichText::new("The plan")
                .size(crate::theme::TYPE_BODY)
                .color(crate::theme::fg()),
        );
        if !shown_above && !p.message.trim().is_empty() {
            ui.add_space(4.0);
            ui.label(
                RichText::new(&p.message)
                    .size(crate::theme::TYPE_BODY)
                    .color(crate::theme::muted()),
            );
        }
        ui.add_space(8.0);
        let chips = reply_chips::plan_reply_chips(&p.message, &self.chip_memory);
        let mut focus = false;
        if let Some(chip) = paint_reply_row(ui, &chips) {
            self.note_reply_pick(&chip);
            match chip.act {
                ReplyAct::Send(text) => return self.send_plan_reply(p, &text),
                ReplyAct::Prefill(text) => {
                    self.elicit_draft = text;
                    focus = true;
                }
                ReplyAct::Dismiss => {
                    if let Some(h) = &self.acp {
                        let _ = h.answer_elicit(p.rpc_id.clone(), "decline", None);
                    }
                    self.elicit_ask = None;
                    self.elicit_draft.clear();
                    return;
                }
            }
        }
        ui.add_space(8.0);
        let mut send = false;
        ui.horizontal(|ui| {
            let edit = ui.add(
                egui::TextEdit::singleline(&mut self.elicit_draft)
                    .hint_text(crate::theme::hint(REPLY_HINT))
                    .desired_width((ui.available_width() - 72.0).max(120.0)),
            );
            if focus {
                edit.request_focus();
            }
            let enter = edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            let typed = !self.elicit_draft.trim().is_empty();
            if typed && (crate::cards::ghost_pill(ui, "Send") || enter) {
                send = true;
            }
        });
        if send {
            let text = self.elicit_draft.trim().to_string();
            self.send_plan_reply(p, &text);
        }
    }

    fn send_plan_reply(&mut self, p: &grokhub_core::wire::ElicitAsk, text: &str) {
        if let Some(h) = &self.acp {
            let _ = h.answer_elicit(
                p.rpc_id.clone(),
                "accept",
                Some(serde_json::json!({ "reply": text })),
            );
        }
        self.elicit_ask = None;
        self.elicit_draft.clear();
        self.status = PLAN_REPLY_SENT.into();
    }

    /// The agent's question options as one-tap answers. True when one was picked.
    pub(super) fn paint_question_chips(
        &mut self,
        ui: &mut egui::Ui,
        p: &grokhub_core::wire::ElicitAsk,
    ) -> bool {
        if p.options.is_empty() {
            return false;
        }
        let chips = reply_chips::question_reply_chips(&p.options, &self.chip_memory);
        ui.add_space(8.0);
        let Some(chip) = paint_reply_row(ui, &chips) else {
            return false;
        };
        self.note_reply_pick(&chip);
        let ReplyAct::Send(answer) = chip.act else {
            return false;
        };
        let field = p.field_name.clone().unwrap_or_else(|| "answer".into());
        if let Some(h) = &self.acp {
            let _ = h.answer_elicit(
                p.rpc_id.clone(),
                "accept",
                Some(serde_json::json!({ field: answer })),
            );
        }
        self.elicit_ask = None;
        self.elicit_draft.clear();
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plan_chips_show_only_while_the_plan_waits_with_no_turn_running() {
        assert!(plan_waits_off_turn(false, SessionMode::Plan, "1. a", false));
        assert!(!plan_waits_off_turn(true, SessionMode::Plan, "1. a", false));
        assert!(!plan_waits_off_turn(
            false,
            SessionMode::Chat,
            "1. a",
            false
        ));
        assert!(!plan_waits_off_turn(false, SessionMode::Plan, "  ", false));
        // The agent's own approval card carries the chips instead.
        assert!(!plan_waits_off_turn(false, SessionMode::Plan, "1. a", true));
    }

    #[test]
    fn a_question_with_options_drops_the_written_out_list() {
        let mut p = grokhub_core::wire::ElicitAsk {
            rpc_id: serde_json::Value::Null,
            session_id: "s".into(),
            tool_call_id: "t".into(),
            server_name: "Question".into(),
            message:
                "Which driver?\n- Studio\n- Game Ready (Recommended)\nReply with the option label."
                    .into(),
            mode: "form".into(),
            url: String::new(),
            elicitation_id: "e".into(),
            field_name: Some("answer".into()),
            field_title: String::new(),
            secret: false,
            options: vec![grokhub_core::wire::ElicitChoice {
                label: "Studio".into(),
                description: String::new(),
                recommended: false,
            }],
        };
        assert_eq!(question_text(&p), "Which driver?");
        p.options.clear();
        assert!(question_text(&p).contains("- Studio"));
    }

    #[test]
    fn not_now_sets_the_plan_aside_without_sending() {
        let mut cabin = Cabin::quiet_for_test();
        cabin.new_thread(false);
        cabin.set_session_mode(SessionMode::Plan);
        cabin.plan_open = true;
        let before = cabin.messages.len();
        let chips = reply_chips::plan_reply_chips("Do it.", &cabin.chip_memory);
        let not_now = chips
            .into_iter()
            .find(|c| c.label == "Not now")
            .expect("not now chip");
        cabin.take_plan_reply(not_now);
        assert_eq!(cabin.session_mode, SessionMode::Chat);
        assert!(!cabin.plan_open);
        assert_eq!(cabin.status, PLAN_SET_ASIDE);
        assert_eq!(cabin.messages.len(), before);
        assert_eq!(cabin.chip_memory.reply_picks["plan:not-now"], 1);
    }

    #[test]
    fn change_this_fills_the_box_and_focuses_it() {
        let mut cabin = Cabin::quiet_for_test();
        cabin.new_thread(false);
        cabin.set_session_mode(SessionMode::Plan);
        let chips = reply_chips::plan_reply_chips("Do it.", &cabin.chip_memory);
        let change = chips
            .into_iter()
            .find(|c| c.label == "Change this")
            .expect("change chip");
        cabin.take_plan_reply(change);
        assert_eq!(cabin.composer, "Change the plan: ");
        assert!(cabin.composer_want_focus);
        assert_eq!(cabin.session_mode, SessionMode::Plan);
    }
}
