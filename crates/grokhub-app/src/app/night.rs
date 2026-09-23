//! Automations, loops, and night review.

use super::*;

impl Cabin {

    pub(super) fn add_automation_seed(&mut self, seed: &str) {
        match self.save_schedule(seed) {
            Some(status) => {
                let saved = status.contains("added");
                self.status = status;
                if saved {
                    dismiss_accepted_auto(&mut self.suggestions, seed, "");
                    self.persist_suggestions();
                }
            }
            None => self.status = "Need `/loop 30m …`, `every 2h …`, or `every day at 9 …`".into(),
        }
    }

    /// Skills Suggested Add — same door as Automations Add → `save_schedule`.
    pub(super) fn add_suggested_skill(&mut self, item: &grokhub_core::LearnedSuggestion) {
        let Some(parsed) = skill_from_suggestion(item) else {
            self.status = "Need a cabin-real skill name and steps".into();
            return;
        };
        let written = parsed.clone();
        std::thread::spawn(move || {
            let _ = crate::skills::save_skill(&written);
        });
        self.remember_skill(parsed.clone());
        let name = parsed.name.clone();
        self.suggestions
            .skills
            .retain(|s| s.name.as_deref() != Some(name.as_str()));
        self.persist_suggestions();
        self.status = format!("Wrote skill {name}");
    }

    /// One door for both schedulers. A clock time ("every weekday at 9") is a cabin
    /// automation in `automations.json`; an interval stays a Grok Build `/loop` row.
    pub(super) fn save_schedule(&mut self, seed: &str) -> Option<String> {
        Some(self.commit_schedule(route_schedule(seed)?))
    }

    pub(super) fn commit_schedule(&mut self, route: ScheduleRoute) -> String {
        match route {
            ScheduleRoute::Clock(a) => {
                if self.automations.len() >= LOOP_MAX {
                    return "Maximum 50 scheduled automations".into();
                }
                let mut a = *a;
                a.id = uid("auto");
                a = ensure_automation_schedule(a, Self::local_clock());
                let label = automation_schedule_label(&a);
                self.automations.push(a);
                self.persist_automations();
                format!("Automation added · {label}")
            }
            ScheduleRoute::Interval { interval, prompt } => {
                if self.grok_loops.len() >= LOOP_MAX {
                    return "Maximum 50 scheduled loops".into();
                }
                let mut row = new_loop(interval.clone(), prompt, now_ms());
                row.id = uid("loop");
                self.grok_loops.push(row);
                self.persist_loops();
                format!("Loop added · every {interval}")
            }
        }
    }

    pub(super) fn teach_watched_routine(&mut self) {
        let ask = self.teach_nl.trim().to_string();
        match teach_routine(&ask, &self.watched_steps) {
            Some(route) => {
                let status = self.commit_schedule(route);
                let saved = status.contains("added");
                self.status = status;
                if saved {
                    self.teach_nl.clear();
                    self.watched_steps.clear();
                    self.watch_once = false;
                }
            }
            None => {
                let tried = user_asked_to_schedule(&ask)
                    || ask.contains("/loop")
                    || ask.to_ascii_lowercase().contains("every ");
                self.status = if tried {
                    "Need `/loop 30m …`, `every 2h …`, or `every day at 9 …`".into()
                } else {
                    "A job is saved only when you ask to schedule it.".into()
                };
            }
        }
    }

    pub(super) fn ui_night(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(crate::theme::bg()).inner_margin(egui::Margin::same(24.0)))
            .show(ctx, |ui| {
            if crate::cards::page_header(ui, "Automations", "New job") {
                self.auto_compose = true;
            }
            egui::ScrollArea::vertical().show(ui, |ui| {
            ui.label(
                RichText::new("Interval prompts run as Grok Build `/loop`. A clock time — `every weekday at 9` — runs as a cabin automation on the 15s pulse. Stop a job when the work is done.")
                    .size(12.0)
                    .color(crate::theme::muted()),
            );
            ui.add_space(12.0);
            if self.auto_compose {
                ui.add_space(12.0);
                egui::Frame::none()
                    .fill(crate::theme::elevated())
                    .rounding(12.0)
                    .stroke(egui::Stroke::new(1.0_f32, crate::theme::border()))
                    .inner_margin(egui::Margin::same(14.0))
                    .show(ui, |ui| {
                        ui.label(RichText::new("New job").strong());
                        let edit = ui.add(
                            egui::TextEdit::singleline(&mut self.night_nl)
                                .hint_text("/loop 30m check deploy · every weekday at 9, summarize the board")
                                .desired_width(f32::INFINITY),
                        );
                        let enter = edit.lost_focus()
                            && ui.input(|i| i.key_pressed(egui::Key::Enter));
                        ui.horizontal(|ui| {
                            if crate::cards::white_pill(ui, "Add") || enter {
                                let seed = self.night_nl.clone();
                                self.add_automation_seed(&seed);
                                if self.status.contains("added") {
                                    self.night_nl.clear();
                                    self.auto_compose = false;
                                }
                            }
                            if crate::cards::ghost_pill(ui, "Cancel") {
                                self.auto_compose = false;
                            }
                        });
                    });
            }
            ui.add_space(8.0);
            self.ui_scheduled_automations(ui);
            crate::cards::section_label(ui, "Loops");
            if self.status.starts_with("Loop:") {
                crate::cards::status_chip(ui, &self.status, crate::cards::ChipTone::Live);
                ui.add_space(8.0);
            }
            let mut drop: Option<usize> = None;
            if self.grok_loops.is_empty() {
                if crate::cards::empty_prompt_tile(
                    ui,
                    crate::icons::TileIcon::Moon,
                    "None yet",
                    "Pick a suggestion or add `/loop 30m …`.",
                ) {
                    self.auto_compose = true;
                }
                ui.add_space(16.0);
            } else {
                for i in 0..self.grok_loops.len() {
                    let title = self.grok_loops[i].prompt.clone();
                    let body = format!(
                        "every {} · {} runs",
                        self.grok_loops[i].interval,
                        self.grok_loops[i].run_count
                    );
                    egui::Frame::none()
                        .fill(crate::theme::elevated())
                        .rounding(14.0)
                        .stroke(egui::Stroke::new(1.0_f32, crate::theme::border()))
                        .inner_margin(egui::Margin::same(12.0))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                if ui.checkbox(&mut self.grok_loops[i].enabled, "").changed() {
                                    self.persist_loops();
                                }
                                ui.vertical(|ui| {
                                    ui.add(
                                        egui::Label::new(
                                            RichText::new(&title)
                                                .size(15.0)
                                                .color(crate::theme::fg()),
                                        )
                                        .wrap(),
                                    );
                                    ui.label(
                                        RichText::new(&body).size(12.0).color(crate::theme::muted()),
                                    );
                                });
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        if crate::cards::ghost_pill(ui, "Remove") {
                                            drop = Some(i);
                                        }
                                        if crate::cards::white_pill(ui, "Run") {
                                            drop = Some(usize::MAX - i);
                                        }
                                    },
                                );
                            });
                        });
                    ui.add_space(8.0);
                }
                ui.add_space(12.0);
            }
            crate::cards::section_label(ui, "Suggested");
            ui.label(
                RichText::new(review_status_line(
                    self.suggestions.last_review_day.as_deref(),
                    &Self::local_day(),
                ))
                .size(12.0)
                .color(crate::theme::muted()),
            );
            ui.add_space(8.0);
            let mut active_names: Vec<String> = self
                .grok_loops
                .iter()
                .map(|a| a.prompt.clone())
                .collect();
            active_names.extend(self.automations.iter().map(|a| a.name.clone()));
            active_names.extend(self.automations.iter().map(|a| a.instructions.clone()));
            let auto_tiles = crate::cards::merge_suggested_autos(&self.suggestions.autos, &active_names);
            crate::cards::tile_row(ui, auto_tiles.len(), |ui, i| {
                let (icon, title, body, seed) = &auto_tiles[i];
                if matches!(
                    crate::cards::grok_tile(ui, *icon, title, body, Some("Add"), false),
                    crate::cards::TileHit::Add | crate::cards::TileHit::Body
                ) {
                    self.add_automation_seed(seed);
                }
            });
            if let Some(i) = drop {
                if i < self.grok_loops.len() {
                    self.grok_loops.remove(i);
                    self.persist_loops();
                } else {
                    let idx = usize::MAX - i;
                    if let Some(row) = self.grok_loops.get(idx).cloned() {
                        self.fire_loop(row);
                    }
                }
            }
            });
        });
    }

    /// Clock-time jobs from `automations.json` — the ones the 15s pulse fires at 09:00.
    pub(super) fn ui_scheduled_automations(&mut self, ui: &mut egui::Ui) {
        crate::cards::section_label(ui, "Scheduled");
        if self.automations.is_empty() {
            ui.label(
                RichText::new("No clock jobs yet. Add `every weekday at 9, summarize the board`.")
                    .size(12.0)
                    .color(crate::theme::muted()),
            );
            ui.add_space(16.0);
            return;
        }
        let now = now_ms();
        let clock = Self::local_clock();
        let mut remove: Option<usize> = None;
        let mut run: Option<usize> = None;
        let mut toggled = false;
        for i in 0..self.automations.len() {
            let title = match self.automations[i].name.trim() {
                "" => self.automations[i].instructions.clone(),
                name => name.to_string(),
            };
            let body = automation_summary_line(&self.automations[i], now);
            egui::Frame::none()
                .fill(crate::theme::elevated())
                .rounding(14.0)
                .stroke(egui::Stroke::new(1.0_f32, crate::theme::border()))
                .inner_margin(egui::Margin::same(12.0))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        if ui.checkbox(&mut self.automations[i].enabled, "").changed() {
                            toggled = true;
                        }
                        ui.vertical(|ui| {
                            ui.add(
                                egui::Label::new(
                                    RichText::new(&title).size(15.0).color(crate::theme::fg()),
                                )
                                .wrap(),
                            );
                            ui.label(RichText::new(&body).size(12.0).color(crate::theme::muted()));
                        });
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if crate::cards::ghost_pill(ui, "Remove") {
                                remove = Some(i);
                            }
                            if crate::cards::white_pill(ui, "Run") {
                                run = Some(i);
                            }
                        });
                    });
                });
            ui.add_space(8.0);
        }
        if toggled {
            // A paused job drops its next run; re-enabling has to find the next slot.
            self.automations = std::mem::take(&mut self.automations)
                .into_iter()
                .map(|a| ensure_automation_schedule(a, clock))
                .collect();
            self.persist_automations();
        }
        if let Some(i) = remove {
            if i < self.automations.len() {
                self.automations.remove(i);
                self.persist_automations();
                self.status = "Automation removed".into();
            }
        } else if let Some(i) = run {
            if let Some(a) = self.automations.get(i).cloned() {
                self.fire_night(a, now);
            }
        }
        ui.add_space(12.0);
    }

    pub(super) fn tick_night(&mut self) -> bool {
        if self.running || self.last_auto_tick.elapsed() < Duration::from_secs(5) {
            return self.running || self.night_check_rx.is_some();
        }
        self.last_auto_tick = Instant::now();
        let clock = Self::local_clock();
        self.roll_today();
        self.daily_auto_day = self.usage.day.clone();
        self.daily_auto_used = self.usage.automation;
        if daily_units_blocked(self.usage.automation, self.cfg.daily_auto_cap) {
            return false;
        }
        let clock_copy = clock;
        self.automations = std::mem::take(&mut self.automations)
            .into_iter()
            .map(|a| ensure_automation_schedule(a, clock_copy))
            .collect();
        if self.poll_night_check(clock.now_ms) {
            return true;
        }
        let due = due_automations(&self.automations, clock.now_ms);
        let Some(a) = due.into_iter().next() else {
            return false;
        };
        if let Some(cmd) = night_check_command(&a.check_command) {
            self.spawn_night_check(a.id.clone(), a.name.clone(), cmd.to_string());
            return true;
        }
        self.fire_night(a, clock.now_ms);
        true
    }

    pub(super) fn poll_night_check(&mut self, now_ms: u64) -> bool {
        let Some((id, rx)) = self.night_check_rx.take() else {
            return false;
        };
        match rx.try_recv() {
            Ok((out, _code)) => {
                if skip_night_check_receipt(&out) {
                    let name = self
                        .automations
                        .iter()
                        .find(|a| a.id == id)
                        .map(|a| a.name.clone())
                        .unwrap_or_else(|| id.clone());
                    self.mark_auto_skipped(&id, now_ms);
                    self.status = format!("Night skipped {name} (check)");
                } else if let Some(a) = self.automations.iter().find(|x| x.id == id).cloned() {
                    if night_check_may_fire(self.running) {
                        self.fire_night(a, now_ms);
                    }
                }
                true
            }
            Err(mpsc::TryRecvError::Empty) => {
                self.night_check_rx = Some((id, rx));
                true
            }
            Err(mpsc::TryRecvError::Disconnected) => false,
        }
    }

    pub(super) fn spawn_night_check(&mut self, id: String, name: String, cmd: String) {
        if let Some(why) = forbidden_reason(&cmd) {
            self.mark_auto_skipped(&id, now_ms());
            self.status = format!("Night check blocked: {why}");
            return;
        }
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let out = run_host(&cmd, Duration::from_secs(20));
            let code = night_check_exit_code(&out);
            let _ = tx.send((out, code));
        });
        self.night_check_rx = Some((id, rx));
        self.status = format!("Night check: {name}");
    }

    pub(super) fn fire_night(&mut self, a: Automation, now_ms: u64) {
        let clock = Self::local_clock();
        let quiet = quiet_hours_active(&clock.hm(), &self.cfg.quiet_start, &self.cfg.quiet_end);
        let destructive = a.instructions.to_ascii_lowercase().contains("rm ")
            || host_risk(&a.instructions) == HostRisk::Destructive;
        if automation_blocked_by_policy(quiet, destructive, 3) {
            self.mark_auto_skipped(&a.id, now_ms);
            self.status = format!("Night skipped {} (quiet/policy)", a.name);
            return;
        }
        if night_unauth_should_skip(self.llm_ready()) {
            self.mark_auto_skipped(&a.id, now_ms);
            self.status = "Connect Grok OAuth in Settings".into();
            return;
        }
        let replay =
            replay_automation_target(&a.instructions).map(|id| self.replay_saved_recipe(id));
        if !night_counts_run(replay) {
            self.mark_auto_skipped(&a.id, now_ms);
            return;
        }
        if replay.is_none() && !self.can_agent() {
            self.mark_auto_skipped(&a.id, now_ms);
            self.status = "Install Grok Build (x.ai/cli) or Connect Grok in Settings".into();
            return;
        }
        self.status = format!("Night: {}", a.name);
        if replay.is_some() {
            self.mark_auto_ran(&a.id, now_ms);
            bump_usage(&mut self.usage, "automation");
            self.daily_auto_used = self.usage.automation;
            self.daily_auto_day = self.usage.day.clone();
            self.persist_usage();
            return;
        }
        self.land_on_real_chat();
        self.send_scheduled_chat(a.instructions);
        if self.running || self.pending_kick.is_some() || self.grok_p_rx.is_some() {
            self.mark_auto_ran(&a.id, now_ms);
            bump_usage(&mut self.usage, "automation");
            self.daily_auto_used = self.usage.automation;
            self.daily_auto_day = self.usage.day.clone();
            self.persist_usage();
        } else {
            self.mark_auto_skipped(&a.id, now_ms);
            self.status = format!("Night skipped {} (kick did not start)", a.name);
        }
    }

    pub(super) fn tick_loops(&mut self) -> bool {
        if self.poll_grok_loop() {
            return true;
        }
        if self.grok_loop_rx.is_some() || self.last_night_tick.elapsed() < Duration::from_secs(5) {
            return self.grok_loop_rx.is_some();
        }
        self.last_night_tick = Instant::now();
        self.roll_today();
        self.daily_auto_day = self.usage.day.clone();
        self.daily_auto_used = self.usage.automation;
        if daily_units_blocked(self.usage.automation, self.cfg.daily_auto_cap) {
            return false;
        }
        if grokhub_acp::find_grok().is_none() {
            return false;
        }
        let due = due_loops(&self.grok_loops, now_ms());
        let Some(row) = due.into_iter().next() else {
            return false;
        };
        self.fire_loop(row);
        true
    }

    pub(super) fn poll_grok_loop(&mut self) -> bool {
        let Some((id, rx)) = self.grok_loop_rx.take() else {
            return false;
        };
        match rx.try_recv() {
            Ok(text) => {
                if let Ok(turn) = grokhub_acp::parse_single_turn(&text) {
                    if let Some(row) = self.grok_loops.iter_mut().find(|x| x.id == id) {
                        row.session_id = Some(turn.session_id);
                    }
                    self.persist_loops();
                    let clip: String = turn.text.chars().take(160).collect();
                    if !clip.is_empty() {
                        self.status = format!("Loop: {clip}");
                    }
                } else {
                    let clip: String = text.chars().take(160).collect();
                    if !clip.is_empty() {
                        self.status = clip;
                    }
                }
                true
            }
            Err(mpsc::TryRecvError::Empty) => {
                self.grok_loop_rx = Some((id, rx));
                true
            }
            Err(mpsc::TryRecvError::Disconnected) => false,
        }
    }

    pub(super) fn fire_loop(&mut self, row: GrokLoop) {
        let now = now_ms();
        if let Some(slot) = self.grok_loops.iter_mut().find(|x| x.id == row.id) {
            *slot = mark_loop_ran(slot.clone(), now);
        }
        self.persist_loops();
        let Some(bin) = grokhub_acp::find_grok() else {
            self.status = build_agent::grok_banner();
            return;
        };
        bump_usage(&mut self.usage, "automation");
        self.daily_auto_used = self.usage.automation;
        self.daily_auto_day = self.usage.day.clone();
        self.persist_usage();
        let cwd = self.grok_cwd();
        let prompt = row.prompt.clone();
        let resume = row.session_id.clone().filter(|s| !s.is_empty());
        let perm_args = self.permission_mode.scheduled_args();
        let (tx, rx) = mpsc::channel();
        self.grok_loop_rx = Some((row.id.clone(), rx));
        let title: String = row.prompt.chars().take(48).collect();
        self.status = format!("Loop: {title}");
        std::thread::spawn(move || {
            let mut args = vec![
                "--no-auto-update".into(),
                "-p".into(),
                prompt,
                "--verbatim".into(),
                "--cwd".into(),
                cwd.display().to_string(),
                "--output-format".into(),
                "json".into(),
            ];
            args.extend(perm_args);
            if let Some(id) = resume {
                args.push("--resume".into());
                args.push(id);
            }
            let refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
            let text =
                grokhub_acp::grok_user_stdout_timeout(&bin, &cwd, &refs, 300).unwrap_or_else(|e| e);
            let _ = tx.send(text);
        });
    }

    pub(super) fn tick_session_suggestions(&mut self) {
        let today = Self::local_day();
        if !review_due(
            self.suggestions.last_session_suggest_day.as_deref(),
            &today,
            &Self::local_clock(),
            REVIEW_NIGHT_HOUR,
        ) {
            return;
        }
        let (thread_lines, _) = self.review_chat_digest();
        let skill_names: Vec<String> = self.skill_list.iter().map(|s| s.name.clone()).collect();
        let mut auto_names: Vec<String> = self.automations.iter().map(|a| a.name.clone()).collect();
        auto_names.extend(self.grok_loops.iter().map(|a| a.prompt.clone()));
        let items = suggestions_from_sessions(&thread_lines, &skill_names, &auto_names);
        self.suggestions.last_session_suggest_day = Some(today);
        if !items.is_empty() {
            let incoming = partition_suggestions(items);
            self.suggestions = merge_suggestion_store(&self.suggestions, incoming);
        }
        self.persist_suggestions();
    }

    pub(super) fn tick_review(&mut self) {
        self.tick_session_suggestions();
        if self.review_busy {
            return;
        }
        let today = Self::local_day();
        if !review_due(
            self.suggestions.last_review_day.as_deref(),
            &today,
            &Self::local_clock(),
            REVIEW_NIGHT_HOUR,
        ) {
            return;
        }
        if !self.llm_ready() {
            return;
        }
        self.spawn_review();
    }

    pub(super) fn review_digest(&self) -> String {
        let (thread_lines, host_receipts) = self.review_chat_digest();
        let input = ReviewDigest {
            insight_pin: insight_pin(&self.learning),
            user_md: config::read_memory("USER.md"),
            memory_md: config::read_memory("MEMORY.md"),
            skill_names: self.skill_list.iter().map(|s| s.name.clone()).collect(),
            automation_names: self.automations.iter().map(|a| a.name.clone()).collect(),
            github_pat: !self.secrets.github_token.trim().is_empty(),
            host_receipts,
            chip_habits: top_habit_labels(&self.chip_memory, 6),
            thread_lines,
            trajectory: summarize_trajectory(
                &parse_trajectory_jsonl(&crate::store::read_trajectory()),
                yesterday_ms(now_ms()),
                12,
            ),
        };
        build_review_digest(&input)
    }

    pub(super) fn review_chat_digest(&self) -> (Vec<DigestLine>, Vec<String>) {
        let current = self
            .threads
            .get(self.thread_idx)
            .map(|t| t.id.as_str())
            .unwrap_or("");
        let mut thread_lines = Vec::new();
        for m in self.messages.iter().rev() {
            if let Some(line) = digest_line_from(&m.0, &m.1) {
                thread_lines.push(line);
                if thread_lines.len() >= 24 {
                    break;
                }
            }
        }
        for t in self.threads.iter().rev() {
            if t.id == current {
                continue;
            }
            for (role, text) in t.messages.iter().rev() {
                if let Some(line) = digest_line_from(role, text) {
                    thread_lines.push(line);
                    if thread_lines.len() >= 40 {
                        break;
                    }
                }
            }
            if thread_lines.len() >= 40 {
                break;
            }
        }
        thread_lines.reverse();
        let mut host_receipts =
            thread_host_receipts_from(self.messages.iter().map(|m| (m.0.as_str(), m.1.as_str())));
        for t in self.threads.iter().rev() {
            if t.id == current {
                continue;
            }
            host_receipts.extend(thread_host_receipts(&t.messages));
        }
        if host_receipts.len() > 6 {
            host_receipts = host_receipts.split_off(host_receipts.len() - 6);
        }
        (thread_lines, host_receipts)
    }

    pub(super) fn spawn_review(&mut self) {
        if self.review_busy {
            return;
        }
        let key = self.bearer();
        if key.trim().is_empty() {
            return;
        }
        let mem_name = self.mem_name.clone();
        let mem_body = self.mem_body.clone();
        let (thread_lines, host_receipts) = self.review_chat_digest();
        let insight = insight_pin(&self.learning);
        let skill_names: Vec<String> = self.skill_list.iter().map(|s| s.name.clone()).collect();
        let automation_names: Vec<String> =
            self.automations.iter().map(|a| a.name.clone()).collect();
        let github_pat = !self.secrets.github_token.trim().is_empty();
        let chip_habits = top_habit_labels(&self.chip_memory, 6);
        let now = now_ms();
        let model = model_for_mode("balanced").to_string();
        let prompt = review_system_prompt().to_string();
        let (tx, rx) = mpsc::channel();
        self.review_rx = Some(rx);
        self.review_busy = true;
        std::thread::spawn(move || {
            if config::read_memory(&mem_name) != mem_body {
                let _ = config::write_memory(&mem_name, &mem_body);
            }
            let digest = build_review_digest(&ReviewDigest {
                insight_pin: insight,
                user_md: config::read_memory("USER.md"),
                memory_md: config::read_memory("MEMORY.md"),
                skill_names,
                automation_names,
                github_pat,
                host_receipts,
                chip_habits,
                thread_lines,
                trajectory: summarize_trajectory(
                    &parse_trajectory_jsonl(&crate::store::read_trajectory()),
                    yesterday_ms(now),
                    12,
                ),
            });
            let messages = [("system".into(), prompt), ("user".into(), digest)];
            let out = grok_chat(&key, &model, &messages, None, None);
            let _ = tx.send(out);
        });
    }

    pub(super) fn poll_review(&mut self) {
        let Some(rx) = self.review_rx.take() else {
            return;
        };
        match rx.try_recv() {
            Ok(raw) => {
                self.review_busy = false;
                self.apply_review_reply(raw);
            }
            Err(mpsc::TryRecvError::Empty) => {
                self.review_rx = Some(rx);
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                self.review_busy = false;
            }
        }
    }

    pub(super) fn apply_review_reply(&mut self, raw: Result<String, String>) {
        match raw {
            Ok(text) => {
                self.apply_review_skill_patches(&text);
                let skill_names: Vec<String> =
                    self.skill_list.iter().map(|s| s.name.clone()).collect();
                let auto_names: Vec<String> =
                    self.automations.iter().map(|a| a.name.clone()).collect();
                let live_tools: Vec<String> = CABIN_GITHUB_TOOLS
                    .iter()
                    .map(|t| (*t).to_string())
                    .collect();
                let items = dedupe_suggestions(
                    parse_suggest_lines(&text),
                    &skill_names,
                    &auto_names,
                    &live_tools,
                );
                let day = Some(Self::local_day());
                let ms = now_ms();
                if items.is_empty() {
                    self.suggestions.last_review_day = day;
                    self.suggestions.last_review_ms = ms;
                } else {
                    let mut incoming = partition_suggestions(items);
                    incoming.last_review_day = day;
                    incoming.last_review_ms = ms;
                    self.suggestions = merge_suggestion_store(&self.suggestions, incoming);
                }
                prune_live_suggestions(&mut self.suggestions, &live_tools);
                let suggestions = self.suggestions.clone();
                std::thread::spawn(move || {
                    let _ = crate::store::save_suggestions(&suggestions);
                });
            }
            Err(e) => {
                self.status = format!("Nightly review held — {e}");
                self.suggestions.last_review_day = Some(Self::local_day());
                self.suggestions.last_review_ms = now_ms();
                let suggestions = self.suggestions.clone();
                std::thread::spawn(move || {
                    let _ = crate::store::save_suggestions(&suggestions);
                });
            }
        }
    }

    pub(super) fn mark_auto_ran(&mut self, id: &str, now: u64) {
        if let Some(a) = self.automations.iter_mut().find(|x| x.id == id) {
            *a = mark_automation_ran(a.clone(), now);
        }
        let list = self.automations.clone();
        std::thread::spawn(move || {
            let _ = crate::night::save(&list);
        });
    }

    pub(super) fn mark_auto_skipped(&mut self, id: &str, now: u64) {
        let clock = Self::local_clock();
        if let Some(a) = self.automations.iter_mut().find(|x| x.id == id) {
            *a = mark_automation_skipped(a.clone(), now, clock);
        }
        let list = self.automations.clone();
        std::thread::spawn(move || {
            let _ = crate::night::save(&list);
        });
    }
}
