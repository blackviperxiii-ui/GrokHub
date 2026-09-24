//! Quick chips and the empty-home greeting.

use super::*;


pub(super) fn cabin_fast_llm(key: String, prompt: String) -> String {
    let key = if key.trim().is_empty() {
        grokhub_acp::grok_cli_key().unwrap_or_default()
    } else {
        key
    };
    if !key.trim().is_empty() {
        let primary = grok_chat(
            &key,
            CABIN_FAST_MODEL,
            &[("user".into(), prompt.clone())],
            None,
            None,
        )
        .unwrap_or_default();
        if !primary.trim().is_empty() {
            return primary;
        }
        return grok_chat(
            &key,
            CABIN_FAST_FALLBACK,
            &[("user".into(), prompt)],
            None,
            None,
        )
        .unwrap_or_default();
    }
    let Some(bin) = grokhub_acp::find_grok() else {
        return String::new();
    };
    let home = std::env::var("HOME").ok();
    let profile = std::env::var("USERPROFILE").ok();
    let session = grokhub_core::session_home(cfg!(windows), home.as_deref(), profile.as_deref());
    let work = grokhub_core::cabin_work_root(cfg!(windows), home.as_deref(), profile.as_deref());
    let picked = resolve_acp_cwd("", session.as_deref(), &work);
    let path = std::path::PathBuf::from(picked);
    let cwd = grokhub_acp::ensure_session_cwd(&path).unwrap_or(path);
    let text = grokhub_acp::grok_stdout(
        &bin,
        &cwd,
        &[
            "--no-auto-update",
            "--model",
            CABIN_FAST_MODEL,
            "-p",
            &prompt,
        ],
    )
    .unwrap_or_default();
    if !text.trim().is_empty() {
        return text;
    }
    grokhub_acp::grok_stdout(
        &bin,
        &cwd,
        &[
            "--no-auto-update",
            "--model",
            CABIN_FAST_FALLBACK,
            "-p",
            &prompt,
        ],
    )
    .unwrap_or_default()
}

impl Cabin {

    pub(super) fn chat_pairs(&self) -> Vec<(String, String)> {
        self.messages
            .iter()
            .map(|m| (m.0.clone(), m.1.clone()))
            .collect()
    }

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
                self.greeting = pick_greeting(&self.greeting, Some(&raw));
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
        let (local, fp, llm_prompt) = {
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
            };
            let fp = greeting_fingerprint(&input);
            let local = local_greeting(&input);
            let llm_prompt = if should_refresh_greeting(
                &self.greeting_llm_fp,
                &fp,
                self.greeting_llm_at,
                now_ms(),
                self.llm_ready(),
                self.greeting_busy,
            ) {
                Some(greeting_prompt(&input))
            } else {
                None
            };
            (local, fp, llm_prompt)
        };
        if self.greeting_fp != fp {
            self.greeting = local;
            self.greeting_fp = fp.clone();
        }
        if let Some(prompt) = llm_prompt {
            self.greeting_llm_fp = fp;
            self.greeting_llm_at = now_ms();
            self.spawn_greeting_llm(prompt);
        }
    }

    pub(super) fn spawn_greeting_llm(&mut self, prompt: String) {
        if self.greeting_busy {
            return;
        }
        let key = self.bearer();
        if key.trim().is_empty() && grokhub_acp::find_grok().is_none() {
            return;
        }
        let (tx, rx) = mpsc::channel();
        self.greeting_rx = Some(rx);
        self.greeting_busy = true;
        std::thread::spawn(move || {
            let raw = cabin_fast_llm(key, prompt);
            let _ = tx.send(raw);
        });
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
        if !self.llm_ready() {
            self.goal_stale = false;
            return;
        }
        if tid.is_empty() {
            self.goal_stale = false;
            return;
        }
        let prompt = thread_goal_prompt(&pairs);
        let key = self.bearer();
        if key.trim().is_empty() {
            self.goal_stale = false;
            return;
        }
        let model = model_for_mode("fast").to_string();
        let (tx, rx) = mpsc::channel();
        self.goal_rx = Some(rx);
        self.goal_busy = true;
        self.goal_stale = false;
        std::thread::spawn(move || {
            let reply =
                grok_chat(&key, &model, &[("user".into(), prompt)], None, None).unwrap_or_default();
            let _ = tx.send((tid, reply));
        });
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
        if should_refresh_llm(
            &self.chip_fp,
            &fp,
            self.chip_llm_at,
            now_ms(),
            self.llm_ready(),
            self.chip_busy,
        ) {
            self.chip_fp = fp;
            self.chip_llm_at = now_ms();
            self.spawn_chip_llm();
        }
    }

    pub(super) fn spawn_chip_llm(&mut self) {
        if self.chip_busy {
            return;
        }
        let key = self.bearer();
        if key.trim().is_empty() && grokhub_acp::find_grok().is_none() {
            return;
        }
        let chat = self.chip_chat_pairs();
        let title = self
            .threads
            .get(self.thread_idx)
            .map(|t| t.title.clone())
            .unwrap_or_default();
        let habits = top_habit_labels(&self.chip_memory, 6);
        let others = self.other_chip_threads();
        let prompt = chip_suggest_prompt(
            &chat,
            &title,
            &self.composer,
            &habits,
            &self.chip_dismissed,
            &others,
        );
        let (tx, rx) = mpsc::channel();
        self.chip_rx = Some(rx);
        self.chip_busy = true;
        std::thread::spawn(move || {
            let chips = parse_llm_chips(&cabin_fast_llm(key, prompt));
            let _ = tx.send(chips);
        });
    }

    /// Ranked composer chips. Empty home and mid-thread share `visible_chips`
    /// (habit / static / stage pool when the LLM row is not ready). A thread
    /// with messages must not clear that pool.
    pub(super) fn composer_chips(&self) -> Vec<QuickChip> {
        let mut chips = self.visible_chips.clone();
        if let Some(c) = skill_offer_chip(&self.composer, &self.skill_list) {
            let gone = self
                .chip_dismissed
                .iter()
                .any(|d| d == &c.id || d == &c.value);
            if !gone && chips.iter().all(|x| x.id != c.id) {
                chips.insert(0, c);
            }
        }
        bias_chips_for_lane(&mut chips, cabin_lane(&self.cfg.cabin_lane));
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
                self.composer.clear();
                self.send_chat(chip.value);
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

    /// Run start. `kick_model` calls this after the prompt or `grok -p` spawn succeeds.
    /// A Doing card is filed only when the ask is a task. Ordinary chat does not.
    /// Writes `workboard.json` through `flush_board` when the card changes.
    pub(super) fn note_inflight_card(&mut self, ask: &str, thread_label: &str) {
        let Some(id) = self.chat_job_thread.clone() else {
            return;
        };
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
