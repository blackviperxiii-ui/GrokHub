//! Composer send and the native turn kick.

use super::*;
use grokhub_agent::route::intent::{self, TurnCx, TurnRead, TurnRoute};
use grokhub_core::{live_send, LiveSend};

/// `/btw <question>` or `/plan <task>`: a one-shot route typed with its words.
/// A bare `/btw` or `/plan` is still a slash command.
pub(super) fn typed_route_with_words(text: &str) -> bool {
    let t = text.trim_start();
    ["/btw", "/plan", "/side"].iter().any(|cmd| {
        t.strip_prefix(cmd)
            .is_some_and(|rest| rest.starts_with(char::is_whitespace) && !rest.trim().is_empty())
    })
}

impl Cabin {
    pub(super) fn send_chat(&mut self, text: String) {
        // Alt+Enter or `/queue`: wait for the live reply instead of steering it.
        let mut queue_asked = std::mem::take(&mut self.bg.queue_next);
        let forced = self.forced_route.take();
        self.turn_retried = false;
        // A typed turn is never a resume turn: the resume block went once (KB-03).
        self.work_resume.follow = None;
        let mut text = text.trim().to_string();
        if text.is_empty() {
            return;
        }
        let typed = text.clone();
        grokhub_agent::timing::note_enter();
        let _lap = grokhub_agent::timing::lap("ui:send_chat");
        if self.running && self.job_is_idea_talk() {
            self.status = "The idea talk is still going".into();
            return;
        }
        let typed_route = typed_route_with_words(&text);
        if !typed_route {
            if let Some(slash) = parse_slash(&text) {
                self.run_slash(slash);
                return;
            }
        }
        if !typed_route && unknown_cabin_slash(&text) {
            self.status = "Unknown command — /help".into();
            return;
        }
        if self.try_diagnose_intent(&text) || self.try_setting_ask(&text) {
            return;
        }
        // A bare "continue" after a run stopped early (a provider error left
        // Do all's next steps waiting) sends the next waiting step, not a
        // new turn that re-reads the whole chat to guess what was meant.
        let mut resumed_step = false;
        if !self.running && is_bare_continue(&text) {
            if let Some(step) = self.followup_queue.take_next_for(&self.visible_thread_id()) {
                text = step;
                resumed_step = true;
            }
        }
        // The app reads what the message is for: a side question, a
        // correction, stop, or a new job while a reply runs; plan first or
        // just answer while nothing does. No pill or mode to pick.
        let running_here = matches!(
            chat_send_kind(
                self.chat_job_thread.as_deref(),
                &self.visible_thread_id(),
                self.running,
            ),
            ChatSendKind::Redirect
        );
        if !(resumed_step || queue_asked && running_here) {
            let read = self.read_turn_for(&text, running_here, forced);
            text = read.text;
            if running_here {
                match read.route {
                    TurnRoute::Side => {
                        self.status = match self.start_side_run(text) {
                            Ok(()) => TurnRoute::Side.ack().into(),
                            Err(err) => err,
                        };
                        return;
                    }
                    TurnRoute::Stop => {
                        self.halt_work(TurnRoute::Stop.ack());
                        return;
                    }
                    TurnRoute::Queue => queue_asked = true,
                    _ => {}
                }
            } else {
                self.settle_turn_mode(&typed, read.route);
            }
        }
        self.begin_turn_origin();
        if self.apply_unparsed_native_slash(&text) {
            return;
        }
        if let Some(recipe) = grokhub_agent::native_deep_research_prompt(&text) {
            text = recipe;
        }
        match chat_send_kind(
            self.chat_job_thread.as_deref(),
            &self.visible_thread_id(),
            self.running,
        ) {
            ChatSendKind::Redirect => {
                if queue_asked || self.acp.is_some() || self.grok_p_rx.is_some() {
                    // Steer this chat's own reply. A cabin-wide Grok command
                    // (`/compact`), a `/loop` on the ACP session, or Grok's own
                    // background tasks on the turn still queue.
                    match live_send(
                        queue_asked || !self.can_steer_live_turn(),
                        self.background_tasks_open(),
                    ) {
                        LiveSend::Queue => {
                            let here = self.visible_thread_id();
                            self.followup_queue.push(&here, text);
                            self.status = match self.followup_queue.count_for(&here) {
                                1 => TurnRoute::Queue.ack().into(),
                                n => format!("{} ({n})", TurnRoute::Queue.ack()),
                            };
                            return;
                        }
                        LiveSend::Steer => {
                            self.steer_native_live(text);
                            return;
                        }
                    }
                } else {
                    let prev =
                        last_user_scan(self.messages.iter().map(|m| (m.0.as_str(), m.1.as_str())))
                            .unwrap_or_default();
                    self.halt_work("Redirected");
                    text = redirect_prompt(&prev, &text);
                }
            }
            ChatSendKind::Fresh => {
                if self.running && self.chat_job_thread.is_some() {
                    if queue_asked || self.acp.is_some() || self.background_tasks_open() {
                        // It waits for its own chat, the one on screen, not
                        // for whichever chat is open when the other reply ends.
                        let here = self.visible_thread_id();
                        self.followup_queue.push(&here, text);
                        self.status = format!("Queued ({})", self.followup_queue.count_for(&here));
                        return;
                    }
                    self.halt_in_flight_for("you sent a new message");
                    self.finish_hub_dispatch("Interrupted", false);
                }
            }
        }
        self.touch();
        // How long the row stood before this step (chip_health).
        grokhub_core::log_chip_next_action(&mut self.chip_memory, "composer", now_ms());
        // A chip's send was learned on its click; a typed ask teaches its
        // kind (never its words) and leaves no chip to credit.
        self.chip_turn_key = self.chip_send_key.take();
        if self.chip_turn_key.is_none() {
            let clock = Self::local_clock();
            remember_typed_prompt(&mut self.chip_memory, &text, now_ms(), clock.hour as u8);
            grokhub_core::remember_intent(
                &mut self.chip_memory,
                "",
                &text,
                now_ms(),
                clock.hour as u8,
                clock.weekday,
            );
        }
        remember_home_surface(&mut self.chip_memory, "chat", now_ms());
        if !persist_user_turn(self.chat_ready()) {
            self.hands_attach = false;
            self.eyes_attach = false;
            self.speak_next = false;
            self.status = self.no_agent_note().into();
            // Signed out: hand the typed text back and offer sign-in inline.
            // A waiting step it would have sent keeps its place.
            if resumed_step {
                let here = self.visible_thread_id();
                self.followup_queue.push_front(&here, text);
            }
            if self.composer.trim().is_empty() {
                self.composer = typed;
            }
            self.signin_card = true;
            return;
        }
        self.signin_card = false;
        if !self.attach_tray.is_empty() {
            self.append_pending_attach_lines(&mut text);
        } else if let Some(name) = self.attach_name.clone() {
            if !name.trim().is_empty() {
                let kind = self.attach_kind.unwrap_or(AttachKind::Image);
                let path = self.attach_path.clone().unwrap_or_default();
                let line = if kind == AttachKind::Image {
                    attach_prompt_line(AttachKind::Image, &name)
                } else {
                    attach_send_line(kind, &name, &path)
                };
                text = append_composer(&text, &line);
            }
        }
        self.verify_ok_turn = verify_ok_after_user_turn(self.verify_ok_turn, true);
        self.active_skill_follow = None;
        // Notes typed on a workboard card linked to this chat go with this turn,
        // once per change. The chat pane shows your message, not the block.
        let notes_thread = self.visible_thread_id();
        self.card_notes_follow =
            grokhub_core::take_card_notes_block(&mut self.board, &notes_thread);
        if self.card_notes_follow.is_some() {
            self.flush_board();
        }
        // Background results that landed since this chat's last turn.
        self.bg.results_follow = if self.scheduled_perm {
            None
        } else {
            self.take_bg_results_follow(&notes_thread)
        };
        // Unproven dream lessons are never auto-followed (round 3 item 39).
        let matched = match_skill(&text, &self.skill_list)
            .filter(|sk| {
                grokhub_agent::dream_gate::may_follow(&sk.name, &crate::config::config_dir())
            })
            .cloned()
            .map(|sk| {
                let follow = self.trial_follow(&sk);
                (
                    sk.name.clone(),
                    self.policy()
                        .injects_skill()
                        .then(|| skill_follow_block(&follow)),
                )
            });
        if let Some((skill_ran, follow)) = matched {
            self.skill_name = skill_ran.clone();
            self.engine_note("skills", &format!("ran:{skill_ran}"), &skill_ran);
            self.status = format!("Skill {skill_ran}");
            if let Some(follow) = follow {
                self.active_skill_follow = Some(follow);
            }
        }
        self.eyes_attach = false;
        self.hands_attach = false;
        self.followup_step = 0;
        if self.scheduled_perm {
            let idx = self.ensure_background_history_thread();
            if let Some(t) = self.threads.get(idx) {
                self.chat_job_thread = Some(t.id.clone());
            }
        }
        let hidden = self.scheduled_perm
            && self
                .chat_job_thread
                .as_deref()
                .is_some_and(|id| id != self.visible_thread_id());
        if hidden {
            self.push_bound_msg("user", text.clone());
        } else {
            self.live_mut().push(("user".into(), text.clone()));
        }
        self.unarchive_on_reply();
        self.stamp_current_access();
        self.persist();
        self.kick_model(true);
    }

    /// Night, anticipate, and `/send` tasks enqueue through `send_chat` so they
    /// share the composer PermissionMode pill — not a separate always-yolo path.
    /// `scheduled_perm` runs the native turn unattended (`native_gate`), so Ask
    /// is fail-closed and nothing is always-approved behind the user's back.
    pub(super) fn send_scheduled_chat(&mut self, text: String) {
        self.scheduled_perm = true;
        self.send_chat(text);
        if !self.running && self.pending_kick.is_none() {
            self.scheduled_perm = false;
        }
    }

    pub(super) fn kick_model(&mut self, consume_attach: bool) {
        // A card from an attempt that never reached Done (error, retry, halt)
        // must not land under this reply.
        self.harness.pending_findings = None;
        if !self.chat_ready() {
            self.running = false;
            self.chat_job_thread = None;
            self.status = self.no_agent_note().into();
            return;
        }
        if !self.kick_skip
            && self.kick_frame.is_none()
            && (self.kick_cap_rx.is_some()
                || should_capture_before_chat(self.eyes_attach || self.hands_attach))
        {
            match self.poll_cabin_frame() {
                CabinFrame::Pending => {
                    self.pending_kick = Some(consume_attach);
                    if self.chat_job_thread.is_none() {
                        self.chat_job_thread = Some(self.visible_thread_id());
                    }
                    self.running = true;
                    if self.chrome_here() {
                        self.status = "Capturing…".into();
                    }
                    return;
                }
                CabinFrame::Ready(url) => {
                    self.kick_frame = Some(url);
                }
                CabinFrame::Skip => {}
            }
        }
        if self.verify_rx.is_some() {
            self.pending_kick = Some(consume_attach);
            if self.chat_job_thread.is_none() {
                self.chat_job_thread = Some(self.visible_thread_id());
            }
            self.running = true;
            if self.chrome_here() {
                self.status = "Verifying…".into();
            }
            return;
        }
        self.running = true;
        if self
            .chat_job_thread
            .as_deref()
            .is_none_or(|id| id == self.visible_thread_id())
        {
            self.status = "Thinking…".into();
        }
        if self.chat_job_thread.is_none() {
            self.chat_job_thread = Some(self.visible_thread_id());
        }
        let vis = self.visible_thread_id();
        let thread_label = self
            .threads
            .iter()
            .find(|t| self.chat_job_thread.as_deref() == Some(t.id.as_str()))
            .map(|t| t.title.clone())
            .unwrap_or_default();
        let raw_ask = {
            let job = self.chat_job_thread.as_deref();
            if job.is_none() || job == Some(vis.as_str()) {
                self.messages
                    .iter()
                    .rev()
                    .find(|m| m.0 == "user" && !is_workload_user(&m.1))
                    .map(|m| m.1.clone())
                    .unwrap_or_default()
            } else {
                self.threads
                    .iter()
                    .find(|t| Some(t.id.as_str()) == job)
                    .and_then(|t| {
                        t.messages
                            .iter()
                            .rev()
                            .find(|(role, content)| role == "user" && !is_workload_user(content))
                            .map(|(_, content)| content.clone())
                    })
                    .unwrap_or_else(|| {
                        self.messages
                            .iter()
                            .rev()
                            .find(|m| m.0 == "user" && !is_workload_user(&m.1))
                            .map(|m| m.1.clone())
                            .unwrap_or_default()
                    })
            }
        };
        // KB-03: the resume block rides in this turn's input only, never the system prompt.
        let with_resume = apply_skill_follow(&raw_ask, self.work_resume.follow.as_deref());
        let with_notes = apply_skill_follow(&with_resume, self.card_notes_follow.as_deref());
        let with_bg = apply_skill_follow(&with_notes, self.bg.results_follow.as_deref());
        let with_skill = apply_skill_follow(&with_bg, self.active_skill_follow.as_deref());
        let with_idea = apply_skill_follow(&with_skill, self.idea_talk_brief().as_deref());
        // A reply that will be spoken is shaped for the ear.
        let voice_style = self.speak_next.then_some(grokhub_core::VOICE_TALK_STYLE);
        let last_user = apply_skill_follow(&with_idea, voice_style);
        if self.grok_p_rx.is_some() {
            return;
        }
        let cabin = self.kick_frame.take();
        self.kick_skip = false;
        self.eyes_attach = false;
        self.hands_attach = false;
        // A new turn's reply replaces one still being spoken.
        self.voice_spoken = None;
        self.stream_buf.clear();
        self.begin_reply_chips();
        self.thought_buf.clear();
        self.turn_log.clear();
        self.thought_seam = false;
        self.say_seam = false;
        if self.stream_here() {
            self.tool_cards.clear();
            self.live_blocks.clear();
        }
        self.perm_ask = None;
        self.perm_queue.clear();
        self.perm_always_confirm = None;
        self.confirm = None;
        self.elicit_ask = None;
        self.elicit_draft.clear();
        let image = if consume_attach {
            let tray_url = self.first_pending_vision_url();
            let url = next_chat_image(
                tray_url.as_deref().or(self.attach_url.as_deref()),
                cabin.as_deref(),
            )
            .map(str::to_string);
            self.consume_pending_attach_on_send();
            url
        } else {
            None
        };
        self.kick_native_turn(&last_user, image.as_deref(), &raw_ask, &thread_label);
    }

    pub(super) fn kick_model_retry(&mut self, t: String) {
        self.try_again = false;
        // A retry re-runs the same ask: keep its background notes, which
        // halting clears and `send_chat` will not set again.
        let results = self.bg.results_follow.take();
        self.halt_in_flight_for("you asked to try again");
        self.bg.results_follow = results;
        self.active_skill_follow = None;
        if let Some(sk) = match_skill(&t, &self.skill_list).filter(|sk| {
            grokhub_agent::dream_gate::may_follow(&sk.name, &crate::config::config_dir())
        }) {
            if self.policy().injects_skill() {
                self.active_skill_follow = Some(skill_follow_block(sk));
            }
        }
        self.kick_model(true);
    }

    pub(super) fn poll_pending_kick(&mut self) {
        let Some(consume) = self.pending_kick else {
            return;
        };
        if self.grok_p_rx.is_some() {
            return;
        }
        if self.kick_frame.is_some()
            || (self.kick_cap_rx.is_none()
                && !should_capture_before_chat(self.eyes_attach || self.hands_attach))
        {
            self.pending_kick = None;
            self.kick_model(consume);
            return;
        }
        match self.poll_cabin_frame() {
            CabinFrame::Pending => {}
            CabinFrame::Ready(url) => {
                self.kick_frame = Some(url);
                self.pending_kick = None;
                self.kick_model(consume);
            }
            CabinFrame::Skip => {
                self.pending_kick = None;
                self.kick_model(consume);
            }
        }
    }
}

/// "continue", "go on", "keep going", "try again" and the like, alone.
pub(super) fn is_bare_continue(text: &str) -> bool {
    let t: String = text
        .trim()
        .trim_end_matches(['.', '!', '?'])
        .to_ascii_lowercase();
    matches!(
        t.as_str(),
        "continue"
            | "cont"
            | "go on"
            | "go ahead"
            | "keep going"
            | "carry on"
            | "resume"
            | "try again"
            | "retry"
            | "next"
            | "proceed"
    )
}

impl Cabin {
    /// Read one typed message for its route. A forced route (Tell the main
    /// task) skips the read; a learned nudge from the user's own corrections
    /// can flip the rules' answer.
    fn read_turn_for(&self, text: &str, running: bool, forced: Option<TurnRoute>) -> TurnRead {
        match forced {
            Some(route) => TurnRead {
                route,
                confidence: 1.0,
                text: text.to_string(),
                rules: vec![format!("turn:{}:forced", route.id())],
            },
            None => intent::read_turn_learned(
                &crate::config::config_dir(),
                text,
                &TurnCx {
                    running,
                    prior: None,
                },
            ),
        }
    }

    #[cfg(test)]
    pub(super) fn settle_turn_mode_for_test(&mut self, typed: &str) {
        let read = self.read_turn_for(typed, false, None);
        self.settle_turn_mode(typed, read.route);
    }

    /// Nothing runs: this turn plans first or is answered straight away. A
    /// plan still waiting on approval keeps planning; "no, just answer" or
    /// "make a plan first" right after a turn fixes it and teaches the reader.
    fn settle_turn_mode(&mut self, typed: &str, route: TurnRoute) {
        let fix = intent::correction_of_previous(typed);
        if let (Some(fix), Some((prev, was))) = (fix, self.last_turn_read.take()) {
            if fix != was {
                intent::note_correction(&crate::config::config_dir(), &prev, false, fix);
            }
        }
        let route = fix.unwrap_or(route);
        // A go ("yes", "go ahead, but skip step 3") runs the waiting plan.
        let pending_plan = self.session_mode == SessionMode::Plan
            && fix != Some(TurnRoute::Chat)
            && !intent::approves_plan(typed);
        let plan = std::mem::take(&mut self.plan_next) || route == TurnRoute::Plan || pending_plan;
        let mode = if plan {
            SessionMode::Plan
        } else {
            SessionMode::Chat
        };
        if mode != self.session_mode {
            self.set_session_mode(mode);
        }
        if plan && !pending_plan {
            self.status = TurnRoute::Plan.ack().into();
        }
        self.last_turn_read = Some((
            typed.to_string(),
            if plan {
                TurnRoute::Plan
            } else {
                TurnRoute::Chat
            },
        ));
    }
}

#[cfg(test)]
mod continue_tests {
    use super::*;

    #[test]
    fn a_bare_continue_is_told_apart_from_a_real_ask() {
        for t in [
            "Continue",
            "continue.",
            " go on ",
            "Keep going!",
            "try again",
        ] {
            assert!(is_bare_continue(t), "{t}");
        }
        for t in [
            "continue with the disk cleanup",
            "next week",
            "go on then, delete it",
        ] {
            assert!(!is_bare_continue(t), "{t}");
        }
    }

    #[test]
    fn continue_never_loses_a_waiting_do_all_step() {
        let mut cabin = Cabin::quiet_for_test();
        cabin.followup_queue =
            Vec::<String>::from(["Fix 2: Clear temp files".into(), "Do all summary".into()]).into();
        cabin.send_chat("Continue".into());
        if cabin.signin_card {
            // Signed out: nothing ran, so the step keeps its place.
            assert_eq!(cabin.followup_queue.len(), 2);
            assert_eq!(cabin.followup_queue[0], "Fix 2: Clear temp files");
        } else {
            // Signed in: the step went instead of a bare "Continue".
            assert_eq!(cabin.followup_queue, ["Do all summary"]);
        }
    }
}
