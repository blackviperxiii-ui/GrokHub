//! Composer send and the grok -p / ACP kick.

use super::*;
use grokhub_core::{live_send, LiveSend};

impl Cabin {

    pub(super) fn send_chat(&mut self, text: String) {
        // Alt+Enter or `/queue`: wait for the live reply instead of steering it.
        let queue_asked = std::mem::take(&mut self.bg.queue_next);
        self.turn_retried = false;
        let mut text = text.trim().to_string();
        if text.is_empty() {
            return;
        }
        grokhub_agent::timing::note_enter();
        let _lap = grokhub_agent::timing::lap("ui:send_chat");
        if self.running && self.job_is_idea_talk() {
            self.status = "The idea talk is still going".into();
            return;
        }
        if let Some(slash) = parse_slash(&text) {
            self.run_slash(slash);
            return;
        }
        if unknown_cabin_slash(&text) {
            self.status = "Unknown command — /help".into();
            return;
        }
        if self.try_diagnose_intent(&text) {
            return;
        }
        self.begin_turn_origin();
        if self.apply_unparsed_native_slash(&text) {
            return;
        }
        if let Some(recipe) =
            grokhub_agent::native_deep_research_prompt(&text)
        {
            text = recipe;
        }
        if btw_queues_without_interrupt(self.session_mode == SessionMode::Ask, self.running) {
            self.side_ask_queue.push(text);
            self.status = format!(
                "btw queued ({}) — main run continues",
                self.side_ask_queue.len()
            );
            return;
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
                            self.followup_queue.push(text);
                            self.status = format!("Queued ({})", self.followup_queue.len());
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
                        self.followup_queue.push(text);
                        self.status = format!("Queued ({})", self.followup_queue.len());
                        return;
                    }
                    self.halt_in_flight();
                    self.finish_hub_dispatch("Interrupted", false);
                }
            }
        }
        self.touch();
        remember_typed_prompt(
            &mut self.chip_memory,
            &text,
            now_ms(),
            Self::local_clock().hour as u8,
        );
        remember_home_surface(&mut self.chip_memory, "chat", now_ms());
        if !persist_user_turn(self.agent_ready()) {
            self.hands_attach = false;
            self.eyes_attach = false;
            self.speak_next = false;
            self.status = self.no_agent_note().into();
            return;
        }
        if let Some(name) = self.attach_name.clone() {
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
        let matched = match_skill(&text, &self.skill_list).map(|sk| {
            (
                sk.name.clone(),
                self.policy().injects_skill().then(|| skill_follow_block(sk)),
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
        if !self.agent_ready() {
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
        let with_notes = apply_skill_follow(&raw_ask, self.card_notes_follow.as_deref());
        let with_bg = apply_skill_follow(&with_notes, self.bg.results_follow.as_deref());
        let with_skill = apply_skill_follow(&with_bg, self.active_skill_follow.as_deref());
        let last_user = apply_skill_follow(&with_skill, self.idea_talk_brief().as_deref());
        if self.grok_p_rx.is_some() {
            return;
        }
        let cabin = self.kick_frame.take();
        self.kick_skip = false;
        self.eyes_attach = false;
        self.hands_attach = false;
        self.stream_buf.clear();
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
            let url =
                next_chat_image(self.attach_url.as_deref(), cabin.as_deref()).map(str::to_string);
            self.attach_url = None;
            self.attach_name = None;
            self.attach_path = None;
            self.attach_kind = None;
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
        self.halt_in_flight();
        self.bg.results_follow = results;
        self.active_skill_follow = None;
        if let Some(sk) = match_skill(&t, &self.skill_list) {
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
