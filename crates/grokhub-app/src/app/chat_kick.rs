//! Composer send and the grok -p / ACP kick.

use super::*;

impl Cabin {

    pub(super) fn send_chat(&mut self, text: String) {
        let mut text = text.trim().to_string();
        if text.is_empty() {
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
        match chat_send_kind(
            self.chat_job_thread.as_deref(),
            &self.visible_thread_id(),
            self.running,
        ) {
            ChatSendKind::Redirect => {
                if self.acp.is_some() || self.grok_p_rx.is_some() {
                    self.followup_queue.push(text);
                    self.status = format!("Queued ({})", self.followup_queue.len());
                    return;
                }
                let prev =
                    last_user_scan(self.messages.iter().map(|m| (m.0.as_str(), m.1.as_str())))
                        .unwrap_or_default();
                self.halt_work("Redirected");
                text = redirect_prompt(&prev, &text);
            }
            ChatSendKind::Fresh => {
                if self.running && self.chat_job_thread.is_some() {
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
        if !persist_user_turn(self.can_agent()) {
            self.hands_attach = false;
            self.eyes_attach = false;
            self.speak_next = false;
            self.status = "Install Grok Build (x.ai/cli) or Connect Grok in Settings".into();
            return;
        }
        if let Some(name) = self.attach_name.as_deref() {
            if !name.trim().is_empty() {
                text = append_composer(&text, &attach_prompt_line(AttachKind::Image, name));
            }
        }
        self.verify_ok_turn = verify_ok_after_user_turn(self.verify_ok_turn, true);
        self.active_skill_follow = None;
        if let Some(sk) = match_skill(&text, &self.skill_list) {
            self.skill_name = sk.name.clone();
            self.status = format!("Skill {}", sk.name);
            if self.policy().injects_skill() {
                self.active_skill_follow = Some(skill_follow_block(sk));
            }
        }
        self.live_mut().push(("user".into(), text.clone()));
        self.eyes_attach = false;
        self.hands_attach = false;
        self.followup_step = 0;
        self.stamp_current_access();
        self.persist();
        self.kick_model(true);
    }

    /// Night, anticipate, and phone `/v1/task` enqueue through `send_chat` so they
    /// share the composer PermissionMode pill — not a separate always-yolo path.
    /// `scheduled_perm` makes `kick_model` skip ACP and honor `scheduled_flags`
    /// / `scheduled_args` (Ask is fail-closed, no `--always-approve`).
    pub(super) fn send_scheduled_chat(&mut self, text: String) {
        // kick_model honors scheduled_flags / scheduled_args while scheduled_perm.
        self.scheduled_perm = true;
        self.send_chat(text);
        if !self.running && self.pending_kick.is_none() {
            self.scheduled_perm = false;
        }
    }

    pub(super) fn send_followup_turn(&mut self) {
        if self.followup_step >= FOLLOWUP_MAX_STEPS {
            return;
        }
        self.followup_step += 1;
        self.push_bound_msg("user", FOLLOWUP_PROMPT.into());
        self.persist();
        self.kick_model(false);
    }

    pub(super) fn kick_model(&mut self, consume_attach: bool) {
        if !self.can_agent() {
            self.status = "Install Grok Build (x.ai/cli) or Connect Grok in Settings".into();
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
                    self.status = "Capturing…".into();
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
            self.status = "Verifying…".into();
            return;
        }
        self.running = true;
        self.status = "Thinking…".into();
        if self.chat_job_thread.is_none() {
            self.chat_job_thread = Some(self.visible_thread_id());
        }
        let vis = self.visible_thread_id();
        let last_user = {
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
        let last_user = match self.active_skill_follow.as_deref() {
            Some(block) if !block.trim().is_empty() => format!("{block}\n\n{last_user}"),
            _ => last_user,
        };
        if self.grok_p_rx.is_some() {
            return;
        }
        if self.acp_spawn_rx.is_some() {
            self.pending_kick = Some(consume_attach);
            return;
        }
        if !self.scheduled_perm && self.permission_mode.uses_acp() && self.acp.is_none() {
            if let Err(e) = self.ensure_acp() {
                self.fail_ask_without_acp(&e);
                return;
            }
            if self.acp.is_none() {
                self.pending_kick = Some(consume_attach);
                return;
            }
        }
        let cabin = self.kick_frame.take();
        self.kick_skip = false;
        self.eyes_attach = false;
        self.hands_attach = false;
        self.stream_buf.clear();
        self.thought_buf.clear();
        self.tool_cards.clear();
        self.live_blocks.clear();
        self.perm_ask = None;
        self.perm_always_confirm = None;
        self.confirm = None;
        self.elicit_ask = None;
        self.elicit_draft.clear();
        let image = if consume_attach {
            let url =
                next_chat_image(self.attach_url.as_deref(), cabin.as_deref()).map(str::to_string);
            self.attach_url = None;
            self.attach_name = None;
            url
        } else {
            None
        };
        if !self.scheduled_perm && self.permission_mode.uses_acp() {
            let prompt_err = self
                .acp
                .as_ref()
                .map(|h| h.prompt_with_image(&last_user, image.as_deref()));
            match prompt_err {
                Some(Ok(())) => {}
                Some(Err(e)) => {
                    self.acp = None;
                    self.fail_ask_without_acp(&e);
                }
                None => self.fail_ask_without_acp(""),
            }
            return;
        }
        let idx = self
            .chat_job_thread
            .as_deref()
            .and_then(|id| self.threads.iter().position(|t| t.id == id))
            .unwrap_or(self.thread_idx);
        let cwd = self
            .threads
            .get(idx)
            .and_then(|t| t.grok_cwd.clone())
            .filter(|s| !s.trim().is_empty())
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| self.grok_cwd());
        let resume = self.threads.get(idx).and_then(|t| {
            let id = t.grok_session.clone().filter(|s| !s.trim().is_empty())?;
            if t.grok_user_home || grokhub_acp::cabin_has_session(&id) {
                Some(id)
            } else {
                None
            }
        });
        let (yolo, auto) = if self.scheduled_perm {
            self.permission_mode.scheduled_flags()
        } else {
            self.permission_mode.composer_headless_flags()
        };
        let model = grokhub_core::cabin_spawn_model(&self.cfg.model).to_string();
        let effort = grokhub_core::parse_reasoning_effort(&self.cfg.reasoning_effort);
        let resume_in_cabin = resume
            .as_deref()
            .is_some_and(grokhub_acp::cabin_has_session);
        let user_home = self
            .threads
            .get(idx)
            .map(|t| t.grok_user_home)
            .unwrap_or(true)
            || !resume_in_cabin;
        let fork = self.threads.get(idx).map(|t| t.grok_fork).unwrap_or(false);
        let worktree = self
            .threads
            .get(idx)
            .map(|t| t.grok_worktree)
            .unwrap_or(false);
        match grokhub_acp::spawn_grok_p_stream(
            &last_user,
            &cwd,
            resume.as_deref(),
            yolo,
            auto,
            Some(model.as_str()),
            effort,
            self.session_mode,
            image.as_deref(),
            fork,
            user_home,
            worktree,
        ) {
            Ok((pid, rx)) => {
                self.grok_p_pid = Some(pid);
                self.grok_p_rx = Some(rx);
                if let Some(t) = self.threads.get_mut(idx) {
                    t.grok_fork = false;
                    t.grok_user_home = user_home;
                }
            }
            Err(e) => {
                self.running = false;
                self.scheduled_perm = false;
                self.status = self.apply_job_fail(&e);
                self.chat_job_thread = None;
            }
        }
    }

    pub(super) fn kick_model_retry(&mut self, t: String) {
        self.try_again = false;
        self.halt_in_flight();
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
        if self.acp_spawn_rx.is_some() || self.grok_p_rx.is_some() {
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
