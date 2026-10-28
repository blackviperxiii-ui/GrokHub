//! Quick chips and the empty-home greeting.

use super::*;

/// True when a chat chip should run through `send_chat` (slash parse or empty-composer send).
fn chip_chat_runs_immediately(chip: &QuickChip, composer_draft: &str) -> bool {
    let value = chip.value.trim();
    if value.starts_with('/') {
        return true;
    }
    if composer_draft.trim().is_empty() {
        return true;
    }
    false
}

fn chip_action_tip(chip: &QuickChip, composer_draft: &str) -> String {
    match chip.kind {
        ChipKind::Nav => "Opens this page — nothing is sent".into(),
        ChipKind::Mode => "Changes the model mode — nothing is sent".into(),
        ChipKind::Shell => "Runs on the host when you send — review the command first".into(),
        ChipKind::Chat => {
            if chip_chat_runs_immediately(chip, composer_draft) {
                if chip.value.trim().starts_with('/') {
                    "Runs this slash command".into()
                } else {
                    "Sends when you are signed in — edit in the composer first if you need to"
                        .into()
                }
            } else {
                "Inserts draft text — edit and send when ready (never auto-sends)".into()
            }
        }
    }
}

fn polish_chip_hints(chips: &mut [QuickChip], composer_draft: &str) {
    for c in chips {
        if c.hint.trim().is_empty() {
            c.hint = chip_action_tip(c, composer_draft);
        } else if !c.hint.contains("send")
            && !c.hint.contains("Inserts")
            && !c.hint.contains("Opens")
        {
            let action = chip_action_tip(c, composer_draft);
            if action != c.hint {
                c.hint = format!("{}\n{}", c.hint.trim(), action);
            }
        }
    }
}

impl Cabin {
    /// Chat pairs for chip rebuild: scan a 4KB prefix so an 8MB complete
    /// does not get cloned into `chip_suggest_prompt` / `chip_thread_from_messages`.
    pub(super) fn chip_chat_pairs(&self) -> Vec<(String, String)> {
        self.messages
            .iter()
            .map(|m| (m.0.clone(), chip_scan(&m.1).to_string()))
            .collect()
    }

    pub(super) fn chip_hour() -> u8 {
        Self::local_clock().hour as u8
    }

    pub(super) fn other_chip_threads(&self) -> Vec<ChipThread> {
        let current = self
            .threads
            .get(self.thread_idx)
            .map(|t| t.id.as_str())
            .unwrap_or("");
        collect_other_chip_threads(&self.threads, current)
    }

    pub(super) fn poll_chips(&mut self) {
        let Some(rx) = self.chip_rx.take() else {
            return;
        };
        match rx.try_recv() {
            Ok(chips) => {
                self.chip_busy = false;
                self.llm_chips = chips
                    .into_iter()
                    .filter(|c| is_plain_text(&c.label) && is_plain_text(&c.value))
                    .collect();
            }
            Err(mpsc::TryRecvError::Empty) => {
                self.chip_rx = Some(rx);
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                self.chip_busy = false;
            }
        }
    }

    pub(super) fn poll_greeting(&mut self) {
        if let Some(rx) = self.greeting_files_rx.take() {
            match rx.try_recv() {
                Ok((user_at, user, memory_at, memory)) => {
                    self.greeting_user_md = user;
                    self.greeting_user_at = user_at;
                    self.greeting_memory_md = memory;
                    self.greeting_memory_at = memory_at;
                }
                Err(mpsc::TryRecvError::Empty) => {
                    self.greeting_files_rx = Some(rx);
                }
                Err(mpsc::TryRecvError::Disconnected) => {}
            }
        }
        let Some(rx) = self.greeting_rx.take() else {
            return;
        };
        match rx.try_recv() {
            Ok(raw) => {
                self.greeting_busy = false;
                let mut sources = vec![
                    self.greeting_user_md.clone(),
                    self.greeting_memory_md.clone(),
                    self.last_night_hint(),
                ];
                if self.mem_name == "USER.md" || self.mem_name == "MEMORY.md" {
                    sources.push(self.mem_body.clone());
                }
                for insight in &self.learning.insights {
                    sources.push(insight.text.clone());
                }
                self.greeting = pick_greeting_against(&self.greeting, Some(&raw), &sources);
            }
            Err(mpsc::TryRecvError::Empty) => {
                self.greeting_rx = Some(rx);
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                self.greeting_busy = false;
            }
        }
    }

    pub(super) fn refresh_greeting(&mut self) {
        let empty = self.messages.is_empty();
        let scratch = self.scratch();
        if !should_paint_greeting(empty, scratch) {
            if !self.greeting.is_empty() {
                self.greeting.clear();
            }
            return;
        }
        if !self.scratch()
            && (self.greeting_flush_name != self.mem_name
                || self.greeting_flush_len != self.mem_body.len())
        {
            let name = self.mem_name.clone();
            let body = self.mem_body.clone();
            std::thread::spawn(move || {
                if config::read_memory(&name) != body {
                    let _ = config::write_memory(&name, &body);
                }
            });
            self.greeting_flush_name = self.mem_name.clone();
            self.greeting_flush_len = self.mem_body.len();
        }
        let user_at = config::memory_updated_at("USER.md");
        let memory_at = config::memory_updated_at("MEMORY.md");
        if self.greeting_user_at != user_at || self.greeting_memory_at != memory_at {
            if self.greeting_user_at == 0 && self.greeting_memory_at == 0 {
                self.greeting_user_md = config::read_memory("USER.md");
                self.greeting_memory_md = config::read_memory("MEMORY.md");
                self.greeting_user_at = user_at;
                self.greeting_memory_at = memory_at;
            } else if self.greeting_files_rx.is_none() {
                let (tx, rx) = mpsc::channel();
                self.greeting_files_rx = Some(rx);
                std::thread::spawn(move || {
                    let user = config::read_memory("USER.md");
                    let memory = config::read_memory("MEMORY.md");
                    let user_at = config::memory_updated_at("USER.md");
                    let memory_at = config::memory_updated_at("MEMORY.md");
                    let _ = tx.send((user_at, user, memory_at, memory));
                });
            }
        }
        let insights: Vec<String> = self
            .learning
            .insights
            .iter()
            .take(6)
            .map(|i| i.text.clone())
            .collect();
        let display_name = self
            .secrets
            .oauth
            .as_ref()
            .and_then(|t| t.name.clone())
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_default();
        let hour = Self::chip_hour();
        let last_night = self.last_night_hint();
        let (local, fp, warm) = {
            let user_md = if self.mem_name == "USER.md" {
                self.mem_body.as_str()
            } else {
                self.greeting_user_md.as_str()
            };
            let memory_md = if self.mem_name == "MEMORY.md" {
                self.mem_body.as_str()
            } else {
                self.greeting_memory_md.as_str()
            };
            let last_project =
                Self::last_project_title_from(&last_night, &self.continue_hint, &self.cfg.goal_pin);
            let first_run = is_cabin_first_run(self.cfg.get_started_done, self.has_real_history());
            let signed_in = self.cabin_signed_in();
            let paused = self
                .board
                .iter()
                .rev()
                .find(|c| c.run && c.status == BoardStatus::Blocked)
                .map(|c| c.title.clone())
                .unwrap_or_default();
            let input = GreetingInput {
                user_md,
                memory_md,
                insights: &insights,
                display_name: &display_name,
                hour,
                last_night: &last_night,
                signed_in,
                first_run,
                last_project: &last_project,
                paused_task: &paused,
            };
            let fp = greeting_fingerprint(&input);
            let local = local_greeting(&input);
            // Greeting stays local. Debounce fields stay warm so a future
            // Settings opt-in can re-enable paraphrase without a schema bump.
            let warm = grokhub_core::should_refresh_greeting(
                &self.greeting_llm_fp,
                &fp,
                self.greeting_llm_at,
                now_ms(),
                self.llm_ready(),
                self.greeting_busy,
            );
            (local, fp, warm)
        };
        if self.greeting_fp != fp {
            self.greeting = local;
            self.greeting_fp = fp.clone();
        }
        if warm {
            self.greeting_llm_fp = fp;
            self.greeting_llm_at = now_ms();
        }
    }

    /// Test-only Fast-model greeting path. Empty-home paint uses `local_greeting` only.
    #[cfg(test)]
    pub(super) fn spawn_greeting_llm(&mut self, prompt: String) {
        if self.greeting_busy {
            return;
        }
        self.spawn_native_greeting(prompt);
    }

    pub(super) fn poll_goals(&mut self) {
        let Some(rx) = self.goal_rx.take() else {
            if self.goal_stale {
                self.spawn_thread_goal();
            }
            return;
        };
        match rx.try_recv() {
            Ok((tid, reply)) => {
                self.goal_busy = false;
                self.apply_thread_goal(&tid, &reply);
                if self.goal_stale {
                    self.spawn_thread_goal();
                }
            }
            Err(mpsc::TryRecvError::Empty) => {
                self.goal_rx = Some(rx);
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                self.goal_busy = false;
                if self.goal_stale {
                    self.spawn_thread_goal();
                }
            }
        }
    }

    pub(super) fn apply_thread_goal(&mut self, tid: &str, reply: &str) {
        let topics = parse_fast_topics(reply);
        if topics.is_empty() {
            return;
        }
        let current = self
            .threads
            .get(self.thread_idx)
            .map(|t| t.id.clone())
            .unwrap_or_default();
        let renaming = self
            .rename_idx
            .and_then(|i| self.threads.get(i))
            .is_some_and(|r| r.id == tid);
        {
            let Some(t) = self.threads.iter_mut().find(|t| t.id == tid) else {
                return;
            };
            if t.scratch {
                return;
            }
            t.goal = blend_thread_goal(&t.goal, &topics, GOAL_DROP_AFTER);
            if !t.goal.label.is_empty() {
                let mut tab = ThreadTab {
                    title: t.title.clone(),
                    pinned: t.pinned,
                    title_locked: t.title_locked,
                };
                if apply_auto_title_in(&mut tab, &t.goal.label, renaming) {
                    t.title = tab.title;
                    t.accessed_ms = now_ms();
                }
                if tid == current {
                    self.cfg.goal_pin = t.goal.label.clone();
                }
            }
        }
        self.persist();
    }

    pub(super) fn spawn_thread_goal(&mut self) {
        self.spawn_thread_goal_on(None);
    }

    pub(super) fn spawn_thread_goal_on(&mut self, thread_id: Option<&str>) {
        if self.goal_busy {
            self.goal_stale = true;
            return;
        }
        let vis = self.visible_thread_id();
        let tid = thread_id
            .map(|s| s.to_string())
            .or_else(|| self.threads.get(self.thread_idx).map(|t| t.id.clone()))
            .unwrap_or_default();
        let on_visible = tid == vis || tid.is_empty();
        let scratch = if on_visible {
            self.scratch()
        } else {
            self.threads
                .iter()
                .find(|t| t.id == tid)
                .map(|t| t.scratch)
                .unwrap_or(false)
        };
        let pairs = if on_visible {
            self.chip_chat_pairs()
        } else {
            self.threads
                .iter()
                .find(|t| t.id == tid)
                .map(|t| {
                    t.messages
                        .iter()
                        .map(|(r, c)| (r.clone(), chip_scan(c).to_string()))
                        .collect()
                })
                .unwrap_or_default()
        };
        let user_turns = visible_turn_count(&pairs);
        let locked = self
            .threads
            .iter()
            .find(|t| t.id == tid)
            .map(|t| t.title_locked)
            .unwrap_or(false);
        if locked || !should_name_thread(scratch, user_turns) {
            self.goal_stale = false;
            return;
        }
        if tid.is_empty() {
            self.goal_stale = false;
            return;
        }
        // Built-in topics from the chat — no key, network, or GOAL: parse.
        let topics = topics_from_turns(&pairs);
        self.goal_stale = false;
        if topics.is_empty() {
            return;
        }
        let reply = format!("GOAL: {}", topics.join(", "));
        self.apply_thread_goal(&tid, &reply);
    }

    pub(super) fn refresh_chips(&mut self) {
        if self.running {
            return;
        }
        let hour = Self::chip_hour();
        let n = self.messages.len();
        let last = self.messages.last().map(|m| m.1.len()).unwrap_or(0);
        let title = self
            .threads
            .get(self.thread_idx)
            .map(|t| t.title.clone())
            .unwrap_or_default();
        let last_failed = self.last_receipt_ok == Some(false);
        let draft_head: String = self.composer.chars().take(16).collect();
        let draft_tail: String = self.composer.chars().rev().take(16).collect();
        let last_slash = self.chip_memory.last_slash.clone().unwrap_or_default();
        let last_surface = self.chip_memory.last_surface.clone().unwrap_or_default();
        let first_run = is_cabin_first_run(self.cfg.get_started_done, self.has_real_history());
        let last_project = Self::last_project_title_from(
            &self.last_night_hint(),
            &self.continue_hint,
            &self.cfg.goal_pin,
        );
        let skill_count = self.skill_list.len();
        let session_mode = self.session_mode.as_str().to_string();
        let key = format!(
            "{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}",
            self.thread_idx,
            n,
            last,
            self.composer.len(),
            draft_head,
            draft_tail,
            hour,
            last_failed,
            title,
            self.chip_dismissed.len(),
            self.llm_chips.len(),
            self.has_key(),
            self.llm_ready(),
            self.cabin_signed_in(),
            self.usage.messages,
            last_slash,
            last_surface,
            first_run,
            last_project,
            session_mode,
            persistable_cabin_lane(&self.cfg.cabin_lane)
        );
        if self.chip_paint_key == key && !self.visible_chips.is_empty() {
            return;
        }
        self.chip_paint_key = key;
        let chat = self.chip_chat_pairs();
        let others = self.other_chip_threads();
        let input = ChipInput {
            chat: &chat,
            draft: &self.composer,
            grok_connected: self.cabin_signed_in(),
            host_on: false,
            mode: if self.cfg.mode.trim().is_empty() {
                "auto"
            } else {
                self.cfg.mode.as_str()
            },
            thread_title: &title,
            usage_messages: self.usage.messages,
            usage_cap: self.cfg.daily_auto_cap,
            memory: &self.chip_memory,
            dismissed: &self.chip_dismissed,
            llm_chips: &self.llm_chips,
            last_failed,
            hour,
            now_ms: now_ms(),
            max: CHIP_VISIBLE_MAX,
            other_threads: &others,
            first_run,
            last_slash: &last_slash,
            last_surface: &last_surface,
            last_project: &last_project,
            skill_count: skill_count as u32,
            session_mode: &session_mode,
        };
        let mode = input.mode;
        let user_turns = chat.iter().filter(|(role, _)| role == "user").count();
        let pace = grokhub_core::cabin_pace(&input);
        self.offer_repeated = matches!(
            pace,
            grokhub_core::CabinPace::With | grokhub_core::CabinPace::Quiet
        );
        // Habit / stage / draft / skill-offer chips are enough; a fast-model
        // garnish is not required for composer chrome (offline / weak model).
        self.visible_chips = build_quick_chips(input);
        let mut fp = context_fingerprint(&chat, &self.composer, last_failed, hour, mode);
        if !others.is_empty() {
            let extra: String = others
                .iter()
                .take(4)
                .map(|t| t.title.chars().take(16).collect::<String>())
                .collect::<Vec<_>>()
                .join(",");
            fp = format!("{fp}+o:{extra}");
        }
        // Keep debounce state warm; do not spawn a model ask from paint.
        let spend_ready = grokhub_core::chip_spend_allowed(pace, user_turns)
            && now_ms().saturating_sub(self.chip_llm_at) >= grokhub_core::CHIP_LLM_SPEND_MS;
        if spend_ready
            && grokhub_core::should_refresh_llm(
                &self.chip_fp,
                &fp,
                self.chip_llm_at,
                now_ms(),
                self.llm_ready(),
                self.chip_busy,
            )
        {
            self.chip_fp = fp;
            self.chip_llm_at = now_ms();
        }
    }

    /// Test-only Fast-model chip path. Composer paint uses `build_quick_chips` only.
    #[cfg(test)]
    pub(super) fn spawn_chip_llm(&mut self) {
        if self.chip_busy {
            return;
        }
        self.spawn_native_chips();
    }

    /// Ranked composer chips. Empty home and mid-thread share `visible_chips`
    /// (habit / static / stage pool when the LLM row is not ready). A thread
    /// with messages must not clear that pool.
    pub(super) fn composer_chips(&self) -> Vec<QuickChip> {
        if super::loadout_home::hangout_composer_layout(self.messages.is_empty()) {
            return super::loadout_home::hangout_quick_chips();
        }
        let mut chips = self.visible_chips.clone();
        if let Some(c) = skill_offer_chip(&self.composer, &self.skill_list) {
            let gone = grokhub_core::chip_dismissed_for_good(&self.chip_memory, &c)
                || self
                    .chip_dismissed
                    .iter()
                    .any(|d| d == &c.id || d == &c.value);
            if !gone && chips.iter().all(|x| x.id != c.id) {
                chips.insert(0, c);
            }
        }
        bias_chips_for_lane(&mut chips, cabin_lane(&self.cfg.cabin_lane));
        polish_chip_hints(&mut chips, &self.composer);
        let mut resume = self.model_chips();
        resume.extend(self.resume_chips());
        resume.extend(self.radar_chips());
        // A clicked resume chip is remembered and can come back as a plain chip.
        chips.retain(|x| {
            resume
                .iter()
                .all(|r| r.id != x.id && r.value != x.value && r.label != x.label)
        });
        chips.splice(0..0, resume);
        if let Some(c) = self.focus_chip() {
            chips.retain(|x| x.id != c.id);
            chips.insert(0, c);
        }
        chips
    }

    pub(super) fn take_chip_act(&mut self, act: crate::cards::ChipRowAct, chips: &[QuickChip]) {
        match act {
            crate::cards::ChipRowAct::Apply(i) => {
                if let Some(c) = chips.get(i).cloned() {
                    self.apply_chip(c);
                }
            }
            crate::cards::ChipRowAct::Dismiss(i) => {
                if let Some(c) = chips.get(i).cloned() {
                    self.dismiss_chip(c);
                    self.refresh_chips();
                }
            }
        }
    }

    pub(super) fn apply_chip(&mut self, chip: QuickChip) {
        // One-shot chips act here and are not learned: replayed later they would mean nothing.
        if self.take_radar_chip(&chip) {
            return;
        }
        let hour = Self::chip_hour();
        let mode = if self.cfg.mode.trim().is_empty() {
            "auto"
        } else {
            self.cfg.mode.as_str()
        };
        let tag = context_fingerprint(
            &self.chip_chat_pairs(),
            &self.composer,
            self.last_receipt_ok == Some(false),
            hour,
            mode,
        );
        remember_chip_click(&mut self.chip_memory, &chip, Some(&tag), now_ms(), hour);
        self.flush_chips();
        match chip.kind {
            ChipKind::Nav => {
                if let Some(id) = nav_from_chip_value(&chip.value) {
                    self.nav = Self::nav_from_id(id);
                    if id == "imagine" {
                        remember_home_surface(&mut self.chip_memory, "imagine", now_ms());
                    } else if id == "skills" {
                        remember_home_surface(&mut self.chip_memory, "skills", now_ms());
                    }
                }
            }
            ChipKind::Mode => {
                if let Some(mode) = mode_from_chip_value(&chip.value) {
                    self.run_slash(Slash::Mode(mode.to_string()));
                }
            }
            ChipKind::Shell => {
                let cmd = chip.value.trim().trim_start_matches('$').trim();
                let cmd = cmd.strip_prefix("/sh ").unwrap_or(cmd);
                self.run_slash(Slash::Sh(cmd.to_string()));
            }
            ChipKind::Chat => {
                // Slash chips use the typed send path so `/learn` click and Enter
                // both parse locally instead of inserting a no-op or hitting Grok.
                if chip_chat_runs_immediately(&chip, &self.composer) {
                    self.composer.clear();
                    self.send_chat(chip.value);
                } else {
                    self.composer = chip.value;
                    self.composer_want_focus = true;
                }
            }
        }
    }

    pub(super) fn dismiss_chip(&mut self, chip: QuickChip) {
        remember_chip_dismiss(&mut self.chip_memory, &chip, now_ms(), Self::chip_hour());
        self.chip_dismissed.push(chip.id);
        self.chip_dismissed.push(chip.value);
        self.flush_chips();
    }

    pub(super) fn flush_chips(&self) {
        let chips = self.chip_memory.clone();
        let io = self.persist_io.clone();
        std::thread::spawn(move || {
            if let Ok(_g) = io.lock() {
                let _ = crate::store::save_chips(&chips);
            }
        });
    }

    /// Run start. `kick_native_turn` calls this after the prompt succeeds.
    /// A Doing card is filed only when the ask is a task. Ordinary chat does not.
    /// Writes `workboard.json` through `flush_board` when the card changes.
    pub(super) fn note_inflight_card(&mut self, ask: &str, thread_label: &str) {
        if self.job_is_idea_talk() {
            return;
        }
        let Some(id) = self.chat_job_thread.clone() else {
            return;
        };
        // A reply in a Follow up card's chat is that card's work, not a new Doing card.
        if self
            .board
            .iter()
            .any(|c| c.automation.is_some() && c.thread_id.as_deref() == Some(id.as_str()))
        {
            return;
        }
        let title = inflight_card_title(ask, thread_label);
        if title.trim().is_empty() {
            return;
        }
        if !self.inflight_open {
            release_inflight_card(&mut self.board, &id);
        }
        self.inflight_open = true;
        if upsert_inflight_card(&mut self.board, &id, &title) {
            self.flush_board();
        }
    }

    /// Run complete. `finish_acp_turn` calls this before it drops `chat_job_thread`.
    /// Doing → done, then `WORK_PIN:` / `WORK_UPDATE:` lines in the assistant text.
    pub(super) fn settle_turn_card(&mut self, assistant: &str) {
        self.inflight_open = false;
        let Some(id) = self.chat_job_thread.clone() else {
            return;
        };
        let mut changed = settle_inflight_card(&mut self.board, &id);
        if apply_assistant_work_marks(&mut self.board, assistant, &id) {
            changed = true;
        }
        if changed {
            self.flush_board();
            if let Some(title) = self
                .board
                .iter()
                .find(|c| {
                    c.thread_id.as_deref() == Some(id.as_str())
                        && c.status == grokhub_core::BoardStatus::Done
                })
                .map(|c| c.title.clone())
            {
                let key = format!("done:{}", grokhub_core::engine_slug(&title));
                self.engine_note("workboard", &key, &title);
            }
        }
    }

    /// The turn never finished. Undoes the Doing card this attempt filed.
    pub(super) fn abandon_turn_card(&mut self) {
        if !self.inflight_open {
            return;
        }
        self.inflight_open = false;
        let Some(id) = self.chat_job_thread.clone() else {
            return;
        };
        if abandon_inflight_card(&mut self.board, &id) {
            self.flush_board();
            if let Some(title) = self
                .board
                .iter()
                .find(|c| {
                    c.thread_id.as_deref() == Some(id.as_str())
                        && c.status == grokhub_core::BoardStatus::Blocked
                })
                .map(|c| c.title.clone())
            {
                let key = format!("paused:{}", grokhub_core::engine_slug(&title));
                self.engine_note("workboard", &key, &title);
            }
        }
    }

    pub(super) fn flush_board(&mut self) {
        let board = self.board.clone();
        let io = self.persist_io.clone();
        self.persist_idle_key = self.persist_idle_now();
        self.last_persist = Instant::now();
        std::thread::spawn(move || {
            if let Ok(_g) = io.lock() {
                let _ = config::save_board(&board);
            }
        });
    }
}

/// Composer quick-chip row (paint + layout). Restyled with ui_kit tokens.
pub mod row {
    use eframe::egui::text::{LayoutJob, TextFormat, TextWrapping};
    use eframe::egui::{self, Align2, Color32, FontId, Sense, Stroke, StrokeKind};
    use std::sync::Arc;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum ChipRowAct {
        Apply(usize),
        Dismiss(usize),
    }

    pub fn chip_paint_label(label: &str) -> String {
        label
            .split_whitespace()
            .flat_map(|w| w.split('…'))
            .filter(|w| !w.is_empty())
            .collect::<Vec<_>>()
            .join(" ")
    }

    pub const CHIP_LABEL_MAX_W: f32 = 180.0;
    pub const CHIP_PAD_X: f32 = crate::theme::SPACING_12;
    #[cfg(test)]
    pub const CHIP_PAD_Y: f32 = 8.0;
    pub const CHIP_GAP: f32 = 6.0;
    pub const CHIP_DISMISS_SLOT: f32 = crate::theme::MIN_HIT * 0.55;
    pub const CHIP_DISMISS_GAP: f32 = crate::theme::SPACING_4;
    /// One fixed chip height. Does not grow when a label is long.
    pub const CHIP_ROW_H: f32 = 40.0;

    pub fn chip_row_width_lock(avail: f32) -> f32 {
        avail.clamp(120.0, crate::theme::CHAT_COL_W)
    }

    pub fn chip_row_visible_w(ui: &egui::Ui) -> f32 {
        let left = ui.cursor().left();
        let clip_w = (ui.clip_rect().right() - left).max(0.0);
        let avail = ui.available_width().min(clip_w);
        let col = crate::cards::composer_pill_w(ui.ctx().content_rect().width());
        chip_row_width_lock(avail.min(col))
    }

    pub fn chip_pill_width(label_w: f32) -> f32 {
        CHIP_PAD_X + label_w.max(0.0) + CHIP_DISMISS_GAP + CHIP_DISMISS_SLOT + CHIP_PAD_X
    }

    pub fn chip_dismiss_rect(pill: egui::Rect) -> egui::Rect {
        egui::Rect::from_center_size(
            egui::pos2(
                pill.right() - CHIP_PAD_X - CHIP_DISMISS_SLOT * 0.5,
                pill.center().y,
            ),
            egui::vec2(CHIP_DISMISS_SLOT, CHIP_DISMISS_SLOT),
        )
    }

    pub fn chip_hit_act(index: usize, dismiss: egui::Rect, pointer: egui::Pos2) -> ChipRowAct {
        if dismiss.contains(pointer) {
            ChipRowAct::Dismiss(index)
        } else {
            ChipRowAct::Apply(index)
        }
    }

    pub fn chip_label_budget(row_w: f32) -> f32 {
        let chrome = chip_pill_width(0.0);
        (row_w - chrome).clamp(32.0, CHIP_LABEL_MAX_W)
    }

    pub fn fluid_chip_count(widths: &[f32], row_w: f32, gap: f32) -> usize {
        if widths.is_empty() || row_w <= 0.0 {
            return 0;
        }
        let mut used = 0.0;
        let mut n = 0usize;
        for (i, w) in widths.iter().enumerate() {
            let add = if i == 0 { *w } else { gap + *w };
            if used + add > row_w + 0.5 {
                break;
            }
            used += add;
            n += 1;
        }
        n
    }

    pub fn chip_cluster_w(widths: &[f32], fit: usize, gap: f32) -> f32 {
        let n = fit.min(widths.len());
        let pills: f32 = widths[..n].iter().sum();
        pills + gap * n.saturating_sub(1) as f32
    }

    pub fn chip_cluster_lead(row_w: f32, cluster_w: f32, ppp: f32) -> f32 {
        let lead = ((row_w - cluster_w) * 0.5).max(0.0);
        let ppp = if ppp > 0.0 { ppp } else { 1.0 };
        (lead * ppp).round() / ppp
    }

    #[cfg(test)]
    pub(crate) fn begin_chip_dismiss_hits(ui: &egui::Ui) {
        ui.ctx().data_mut(|d| {
            d.insert_temp(
                egui::Id::new("qchip-dismiss-hits"),
                Vec::<egui::Rect>::new(),
            );
            d.insert_temp(egui::Id::new("qchip-pills"), Vec::<egui::Rect>::new());
        });
    }

    #[cfg(test)]
    pub(crate) fn remember_chip_dismiss_hit(ui: &egui::Ui, rect: egui::Rect) {
        ui.ctx().data_mut(|d| {
            let id = egui::Id::new("qchip-dismiss-hits");
            let mut hits = d.get_temp::<Vec<egui::Rect>>(id).unwrap_or_default();
            hits.push(rect);
            d.insert_temp(id, hits);
        });
    }

    #[cfg(test)]
    pub(crate) fn remember_chip_pill(ui: &egui::Ui, rect: egui::Rect) {
        ui.ctx().data_mut(|d| {
            let id = egui::Id::new("qchip-pills");
            let mut pills = d.get_temp::<Vec<egui::Rect>>(id).unwrap_or_default();
            pills.push(rect);
            d.insert_temp(id, pills);
        });
    }

    pub fn layout_chip_label(
        ui: &egui::Ui,
        text: &str,
        font: FontId,
        color: Color32,
        max_w: f32,
    ) -> Arc<egui::Galley> {
        let mut job = LayoutJob::single_section(
            text.to_owned(),
            TextFormat {
                font_id: font,
                color,
                ..Default::default()
            },
        );
        job.wrap = TextWrapping::truncate_at_width(max_w.max(1.0));
        job.break_on_newline = false;
        ui.fonts_mut(|f| f.layout_job(job))
    }

    pub const CHIP_EMPTY_LABEL: &str = "Nothing queued";

    fn chip_fill(primary: bool, hovered: bool) -> Color32 {
        if primary {
            crate::theme::blend_color(
                crate::theme::selection(),
                crate::theme::surface_hover(),
                if hovered { 0.35 } else { 0.15 },
            )
        } else if hovered {
            crate::theme::surface_hover()
        } else {
            crate::theme::elevated()
        }
    }

    pub fn quick_chip_fill(primary: bool) -> Color32 {
        chip_fill(primary, false)
    }

    pub fn quick_chip_fg(primary: bool) -> Color32 {
        if primary {
            crate::theme::fg()
        } else {
            crate::theme::muted()
        }
    }

    pub fn quick_chip_stroke_w(primary: bool) -> f32 {
        if primary {
            1.5
        } else {
            1.0
        }
    }

    pub fn quick_chip_stroke(primary: bool) -> Color32 {
        if primary {
            crate::theme::border_strong()
        } else {
            crate::theme::border()
        }
    }

    fn quick_chip_stroke_paint(primary: bool) -> Stroke {
        Stroke::new(quick_chip_stroke_w(primary), quick_chip_stroke(primary))
    }

    pub fn quick_chip_strong(primary: bool) -> bool {
        primary
    }

    pub fn chip_why_tip(hint: &str, label: &str) -> String {
        let why = hint.trim();
        if why == "New here" {
            return "Suggested because you're new here".into();
        }
        if !why.is_empty() {
            return format!("Why this?\n{why}");
        }
        let label = label.trim();
        if label.is_empty() {
            "Why this?".into()
        } else {
            format!("Why this?\n{label}")
        }
    }

    fn paint_chip_pill(
        ui: &mut egui::Ui,
        rect: egui::Rect,
        fill: Color32,
        primary: bool,
        resp: &egui::Response,
    ) {
        let stroke = quick_chip_stroke_paint(primary);
        let body = rect.shrink(stroke.width * 0.5);
        let radius = crate::theme::RADIUS_MD;
        ui.painter()
            .rect(body, radius, fill, stroke, StrokeKind::Inside);
        crate::theme::paint_focus_ring(ui, body, radius, resp);
    }

    pub fn paint_empty_chip_state(ui: &mut egui::Ui) {
        let max_w = chip_row_visible_w(ui);
        ui.allocate_ui_with_layout(
            egui::vec2(max_w, CHIP_ROW_H),
            egui::Layout::left_to_right(egui::Align::Center)
                .with_main_wrap(false)
                .with_main_align(egui::Align::Min),
            |ui| {
                ui.set_max_width(max_w);
                #[cfg(test)]
                begin_chip_dismiss_hits(ui);
                let color = quick_chip_fg(false);
                let budget = chip_label_budget(max_w);
                let galley = layout_chip_label(
                    ui,
                    CHIP_EMPTY_LABEL,
                    FontId::proportional(crate::theme::TYPE_CAPTION),
                    color,
                    budget,
                );
                let pill_w = (CHIP_PAD_X + galley.size().x.max(8.0) + CHIP_PAD_X).min(max_w);
                ui.add_space(chip_cluster_lead(
                    max_w,
                    pill_w,
                    ui.ctx().pixels_per_point(),
                ));
                let (rect, resp) =
                    ui.allocate_exact_size(egui::vec2(pill_w, CHIP_ROW_H), Sense::hover());
                #[cfg(test)]
                remember_chip_pill(ui, rect);
                let fill = if resp.hovered() {
                    chip_fill(false, true)
                } else {
                    quick_chip_fill(false)
                };
                paint_chip_pill(ui, rect, fill, false, &resp);
                let text_pos = egui::pos2(
                    rect.left() + CHIP_PAD_X,
                    rect.center().y - galley.size().y * 0.5,
                );
                ui.painter().galley(text_pos, galley, color);
            },
        );
    }

    pub fn quick_chip_row(
        ui: &mut egui::Ui,
        chips: &[grokhub_core::QuickChip],
    ) -> Option<ChipRowAct> {
        if chips.is_empty() {
            paint_empty_chip_state(ui);
            return None;
        }
        let mut act = None;
        let max_w = chip_row_visible_w(ui);
        let budget = chip_label_budget(max_w);
        let measured: Vec<Arc<egui::Galley>> = chips
            .iter()
            .map(|c| {
                let font = if quick_chip_strong(c.primary) {
                    crate::theme::title_font(crate::theme::TYPE_CAPTION)
                } else {
                    FontId::proportional(crate::theme::TYPE_CAPTION)
                };
                let color = quick_chip_fg(c.primary);
                layout_chip_label(ui, &chip_paint_label(&c.label), font, color, budget)
            })
            .collect();
        let widths: Vec<f32> = measured
            .iter()
            .map(|g| chip_pill_width(g.size().x.min(budget)).min(max_w))
            .collect();
        let fit = fluid_chip_count(&widths, max_w, CHIP_GAP);
        ui.allocate_ui_with_layout(
            egui::vec2(max_w, CHIP_ROW_H),
            egui::Layout::left_to_right(egui::Align::Center)
                .with_main_wrap(false)
                .with_main_align(egui::Align::Min),
            |ui| {
                ui.set_max_width(max_w);
                ui.spacing_mut().item_spacing = egui::vec2(CHIP_GAP, 0.0);
                crate::theme::glide_paint(ui, "quick-chips");
                let mut chip_hover: Option<egui::Rect> = None;
                #[cfg(test)]
                begin_chip_dismiss_hits(ui);
                let cluster_w = chip_cluster_w(&widths, fit, CHIP_GAP);
                ui.add_space(chip_cluster_lead(
                    max_w,
                    cluster_w,
                    ui.ctx().pixels_per_point(),
                ));
                let mut used = 0.0f32;
                for (i, c) in chips.iter().enumerate().take(fit) {
                    let galley = measured[i].clone();
                    let pill_w = widths[i];
                    let add = if used == 0.0 {
                        pill_w
                    } else {
                        CHIP_GAP + pill_w
                    };
                    if used + add > max_w + 0.5 {
                        break;
                    }
                    used += add;
                    let chip_id = ui.id().with(("qchip", i));
                    let color = quick_chip_fg(c.primary);
                    let paint = chip_paint_label(&c.label);
                    let why = if c.hint.is_empty() && paint != c.label {
                        c.label.as_str()
                    } else {
                        c.hint.as_str()
                    };
                    let tip = chip_why_tip(why, &c.label);
                    let (rect, hit_resp) =
                        ui.allocate_exact_size(egui::vec2(pill_w, CHIP_ROW_H), Sense::click());
                    let hit_resp = crate::theme::pointing(hit_resp);
                    let hovered = hit_resp.hovered();
                    if hovered {
                        chip_hover = Some(rect);
                    }
                    let fill = chip_fill(c.primary, hovered);
                    let (_, felt, painted) =
                        crate::theme::feel_response(ui, hit_resp.clone(), fill);
                    paint_chip_pill(ui, felt, painted, c.primary, &hit_resp);
                    #[cfg(test)]
                    remember_chip_pill(ui, rect);
                    let text_pos = egui::pos2(
                        rect.left() + CHIP_PAD_X,
                        rect.center().y - galley.size().y * 0.5,
                    );
                    ui.painter().galley(text_pos, galley, color);
                    let x_rect = chip_dismiss_rect(rect);
                    #[cfg(test)]
                    remember_chip_dismiss_hit(ui, x_rect);
                    let dismiss_t = ui.ctx().animate_bool_with_time(
                        chip_id.with("dismiss"),
                        hovered,
                        grokhub_core::SELECT_SECS,
                    );
                    let over_x = hit_resp.hover_pos().is_some_and(|p| x_rect.contains(p));
                    let hit = if over_x {
                        hit_resp.on_hover_text("Hide this suggestion")
                    } else {
                        hit_resp.on_hover_text(tip)
                    };
                    let hit_resp = hit;
                    if dismiss_t > 0.01 {
                        if over_x {
                            ui.painter().rect_filled(
                                x_rect,
                                crate::theme::RADIUS_SM,
                                crate::theme::lift_fill(
                                    Color32::TRANSPARENT,
                                    grokhub_core::HOVER_WASH * dismiss_t,
                                ),
                            );
                        }
                        let x_color = crate::theme::blend_color(
                            Color32::TRANSPARENT,
                            crate::theme::subtle(),
                            dismiss_t,
                        );
                        ui.painter().text(
                            x_rect.center(),
                            Align2::CENTER_CENTER,
                            "×",
                            FontId::proportional(crate::theme::TYPE_CAPTION),
                            x_color,
                        );
                    }
                    if hit_resp.clicked() {
                        act = Some(match hit_resp.interact_pointer_pos() {
                            Some(pos) => chip_hit_act(i, x_rect, pos),
                            None => ChipRowAct::Apply(i),
                        });
                    }
                    if hit_resp.has_focus() {
                        crate::theme::paint_focus_ring(
                            ui,
                            felt,
                            crate::theme::RADIUS_MD,
                            &hit_resp,
                        );
                    }
                }
                if let Some(rect) = chip_hover {
                    crate::theme::glide_candidate(ui, "quick-chips", rect, true, false);
                }
                crate::theme::glide_aim(ui, "quick-chips");
            },
        );
        act
    }
    // quick_chip_row_end

    #[cfg(test)]
    mod tests {
        use super::*;
        use crate::theme::{hold_paint_test, set_paint_dark};

        #[test]
        fn chip_row_source_layout_contract() {
            let src = include_str!("chips.rs");
            let chip_row = src
                .split("pub fn quick_chip_row(")
                .nth(1)
                .and_then(|s| s.split("#[cfg(test)]").next())
                .expect("quick_chip_row");
            assert!(
                chip_row.contains("with_main_wrap(false)")
                    && chip_row.contains("fluid_chip_count")
                    && chip_row.contains("layout_chip_label")
                    && chip_row.contains("CHIP_ROW_H")
                    && chip_row.contains("chip_row_visible_w")
                    && !chip_row.contains("with_main_wrap(true)"),
                "chips are one fixed line, ellipsized, and drop overflow inside Ask anything"
            );
        }

        #[test]
        fn chip_row_height_meets_caption_pad() {
            let _hold = hold_paint_test();
            const {
                assert!(CHIP_ROW_H >= crate::theme::TYPE_CAPTION + CHIP_PAD_Y * 2.0);
            }
        }

        #[test]
        fn chip_pill_contrast_dark_and_light() {
            let _hold = hold_paint_test();
            for dark in [true, false] {
                set_paint_dark(dark);
                let fill = quick_chip_fill(true);
                let fg = quick_chip_fg(true);
                assert!(
                    crate::theme::contrast_ratio(fg, fill) >= 4.5,
                    "primary chip contrast dark={dark}"
                );
            }
        }
    }
}

#[cfg(test)]
mod chip_apply_tests {
    use super::*;
    use grokhub_core::{AttachKind, ChipKind, QuickChip};

    fn chat_chip(id: &str, label: &str, value: &str) -> QuickChip {
        QuickChip {
            id: id.into(),
            label: label.into(),
            value: value.into(),
            kind: ChipKind::Chat,
            score: 1.0,
            hint: String::new(),
            primary: false,
        }
    }

    #[test]
    fn chip_insert_keeps_draft_and_attachments() {
        let _hold = crate::config::hold_test_config();
        let root = crate::config::test_config_root("chip-insert");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("config root");
        std::env::set_var("GROKHUB_CONFIG", &root);

        let mut cabin = Cabin::quiet_for_test();
        cabin.composer = "fix the leak".into();
        cabin.attach_path = Some("/tmp/leak.png".into());
        cabin.attach_name = Some("leak.png".into());
        cabin.attach_kind = Some(AttachKind::Image);

        let expanded = "fix the leak. Be concrete and complete the goal.";
        cabin.apply_chip(chat_chip("pred-expand-draft", "Expand & send", expanded));

        assert_eq!(cabin.composer, expanded);
        assert_eq!(cabin.attach_path.as_deref(), Some("/tmp/leak.png"));
        assert_eq!(cabin.attach_name.as_deref(), Some("leak.png"));
        assert_eq!(cabin.attach_kind, Some(AttachKind::Image));
        assert!(!cabin.running);
        assert!(cabin.messages.is_empty());

        let _ = std::fs::remove_dir_all(&root);
        std::env::remove_var("GROKHUB_CONFIG");
    }
}
