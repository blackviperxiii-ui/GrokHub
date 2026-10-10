//! Automations, loops, and night review.

use super::*;
use grokhub_agent::harness::Origin;
use grokhub_core::{
    automation_failed_card, automation_health_line, hold_if_quiet, mark_automation_failed,
    mark_automation_ok, mark_automation_stopped, BgEnd, BgOrigin,
};

/// Primary pill on a scheduled row. A failing job retries; a healthy one runs.
pub(super) fn scheduled_primary_label(failing: bool) -> &'static str {
    if failing {
        "Retry"
    } else {
        "Run"
    }
}

/// The row's name. A blank name falls back to the instructions (or the loop prompt).
pub(super) fn automation_row_title(a: &Automation) -> String {
    match a.name.trim() {
        "" => a.instructions.clone(),
        name => name.to_string(),
    }
}

/// Words that mark this job's post on the shared Background history thread.
fn run_needles(a: &Automation) -> Vec<String> {
    let mut out = Vec::new();
    let task = grokhub_core::bg_task_title(&a.instructions);
    if !task.is_empty() {
        out.push(task);
    }
    let name = a.name.trim();
    if !name.is_empty() {
        out.push(name.to_string());
    }
    let err = a.health.error.trim();
    if err.len() >= 12 {
        out.push(err.to_string());
    }
    out
}

/// Trailing actions on a job row. Run is a ghost pill. Retry stays filled.
/// Remove lives in ··· and only asks; the caller confirms before anything is deleted.
fn job_row_menu(ui: &mut egui::Ui, primary: &str) -> (bool, bool) {
    let mut ran = false;
    let mut remove = false;
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        crate::cards::dots_menu(ui, false, |ui| {
            if ui.button("Remove").clicked() {
                remove = true;
                ui.close();
            }
        });
        if job_primary_pill(ui, primary) {
            ran = true;
        }
    });
    (ran, remove)
}

/// Healthy Run is a ghost. A failed scheduled Retry stays filled white.
/// Loops always pass "Run", so both lists share this choice.
fn job_primary_pill(ui: &mut egui::Ui, label: &str) -> bool {
    if label == "Retry" {
        crate::cards::white_pill(ui, label)
    } else {
        crate::cards::ghost_pill(ui, label)
    }
}

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
            let _ = crate::skills::save_skill_logged(
                &written,
                grokhub_agent::harness::Origin::User,
                "added from Suggested",
            );
        });
        self.harness.skill_rows = None;
        self.remember_skill(parsed.clone());
        let name = parsed.name.clone();
        self.status = format!("Wrote skill {name}");
    }

    /// One door for both schedulers. A clock time ("every weekday at 9") is a cabin
    /// automation in `automations.json`; an interval stays a Grok Build `/loop` row.
    pub(super) fn save_schedule(&mut self, seed: &str) -> Option<String> {
        Some(self.commit_schedule(route_schedule(seed)?, Origin::User, ""))
    }

    /// [`Self::save_schedule`] for job text the model wrote (an Ideas Add).
    pub(super) fn save_schedule_as(&mut self, seed: &str, origin: Origin, reason: &str) -> Option<String> {
        Some(self.commit_schedule(route_schedule(seed)?, origin, reason))
    }

    /// Save a schedule. With `Origin::SelfManage` (the job text came from the
    /// model: an Ideas Add or an Automate offer) a clock job is saved through
    /// the ChangeLedger, so it keeps a version, gets a Work-tree row with
    /// Undo, and shows in the next Home update. Interval loops are Grok Build
    /// rows and stay outside the ledger.
    pub(super) fn commit_schedule(&mut self, route: ScheduleRoute, origin: Origin, reason: &str) -> String {
        match route {
            ScheduleRoute::Clock(a) => {
                if self.automations.len() >= LOOP_MAX {
                    return "Maximum 50 scheduled automations".into();
                }
                let mut a = *a;
                a.id = uid("auto");
                a = ensure_automation_schedule(a, Self::local_clock());
                let label = automation_schedule_label(&a);
                let id = a.id.clone();
                let name = a.name.clone();
                self.automations.push(a);
                if origin == Origin::SelfManage {
                    let (path, list) = (crate::night::path(), self.automations.clone());
                    let target = grokhub_agent::harness::AutomationsFile { path: &path, id: &id };
                    let saved = grokhub_agent::harness::record_change(&config::config_dir(), &target, origin, reason, || {
                        crate::night::save(&list)
                    });
                    if let Err(why) = saved {
                        self.automations.retain(|a| a.id != id);
                        return why;
                    }
                } else {
                    self.persist_automations();
                }
                self.note_schedule_created(&id, &name, &label);
                format!("Automation added · {label}")
            }
            ScheduleRoute::Interval { interval, prompt } => {
                if self.grok_loops.len() >= LOOP_MAX {
                    return "Maximum 50 scheduled loops".into();
                }
                let title = prompt.clone();
                let mut row = new_loop(interval.clone(), prompt, now_ms());
                row.id = uid("loop");
                let id = row.id.clone();
                self.grok_loops.push(row);
                self.persist_loops();
                self.note_schedule_created(&id, &title, &format!("every {interval}"));
                format!("Loop added · every {interval}")
            }
        }
    }

    pub(super) fn ui_night(&mut self, ui: &mut egui::Ui) {
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(crate::theme::bg()).inner_margin(egui::Margin::same(24)))
            .show(ui, |ui| {
            if crate::cards::page_header(ui, "Automations", "New job") {
                self.auto_compose = true;
            }
            egui::ScrollArea::vertical().show(ui, |ui| {
            crate::cards::help_text(ui, "Loops repeat on an interval. Scheduled jobs run at a clock time. Stop a job when its work is done.");
            ui.add_space(12.0);
            if self.auto_compose {
                ui.add_space(12.0);
                egui::Frame::NONE
                    .fill(crate::theme::elevated())
                    .corner_radius(crate::theme::CARD_RADIUS)
                    .stroke(egui::Stroke::new(1.0_f32, crate::theme::border()))
                    .inner_margin(egui::Margin::same(14))
                    .show(ui, |ui| {
                        ui.label(RichText::new("New job").strong());
                        let edit = ui.add(
                            egui::TextEdit::singleline(&mut self.night_nl)
                                .hint_text(crate::theme::hint("/loop 30m check deploy · every weekday at 9, summarize the board"))
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
            let mut remove_loop: Option<usize> = None;
            let mut run_loop: Option<usize> = None;
            let mut unhide_loop: Option<String> = None;
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
                    egui::Frame::NONE
                        .fill(crate::theme::elevated())
                        .corner_radius(crate::theme::CARD_RADIUS)
                        .stroke(egui::Stroke::new(1.0_f32, crate::theme::border()))
                        .inner_margin(egui::Margin::same(12))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                if crate::cards::enabled_switch(ui, self.grok_loops[i].enabled) {
                                    self.grok_loops[i].enabled = !self.grok_loops[i].enabled;
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
                                    if grokhub_core::source_hidden(
                                        &self.cfg.feed_pulse,
                                        &self.grok_loops[i].id,
                                    ) && crate::cards::ghost_pill(ui, grokhub_core::HOME_HIDDEN_NOTE)
                                    {
                                        unhide_loop = Some(self.grok_loops[i].id.clone());
                                    }
                                });
                                let (ran, remove_hit) = job_row_menu(ui, "Run");
                                if remove_hit {
                                    remove_loop = Some(i);
                                }
                                if ran {
                                    run_loop = Some(i);
                                }
                            });
                        });
                    ui.add_space(8.0);
                }
                ui.add_space(12.0);
            }
            crate::cards::section_label(ui, "Suggested");
            ui.label(
                RichText::new("Suggested from your recent work")
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
            if let Some(i) = remove_loop {
                if let Some(row) = self.grok_loops.get(i) {
                    let id = row.id.clone();
                    let title = row.prompt.clone();
                    self.arm_remove_job(RemoveJobKind::Loop, id, title);
                }
            } else if let Some(i) = run_loop {
                if let Some(row) = self.grok_loops.get(i).cloned() {
                    self.fire_loop(row);
                }
            }
            if let Some(id) = unhide_loop {
                self.undo_hide_automation_from_home(&id);
            }
            });
        });
    }

    /// Clock-time jobs from `automations.json` — the ones the 15s pulse fires at 09:00.
    pub(super) fn ui_scheduled_automations(&mut self, ui: &mut egui::Ui) {
        crate::cards::section_label(ui, "Scheduled");
        if self.automations.is_empty() {
            crate::cards::help_text(ui, "No clock jobs yet. Add `every weekday at 9, summarize the board`.");
            ui.add_space(16.0);
            return;
        }
        let now = now_ms();
        let clock = Self::local_clock();
        let mut remove: Option<usize> = None;
        let mut run: Option<usize> = None;
        let mut view: Option<usize> = None;
        let mut unhide: Option<String> = None;
        let mut toggled = false;
        for i in 0..self.automations.len() {
            let title = automation_row_title(&self.automations[i]);
            let body = automation_summary_line(&self.automations[i], clock);
            let health = automation_health_line(&self.automations[i]);
            let ring = if health.is_some() {
                crate::theme::offline().gamma_multiply(0.6)
            } else {
                crate::theme::border()
            };
            egui::Frame::NONE
                .fill(crate::theme::elevated())
                .corner_radius(crate::theme::CARD_RADIUS)
                .stroke(egui::Stroke::new(1.0_f32, ring))
                .inner_margin(egui::Margin::same(12))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        if crate::cards::enabled_switch(ui, self.automations[i].enabled) {
                            self.automations[i].enabled = !self.automations[i].enabled;
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
                            if let Some(line) = &health {
                                ui.add(
                                    egui::Label::new(
                                        RichText::new(line)
                                            .size(12.0)
                                            .color(crate::theme::offline()),
                                    )
                                    .wrap(),
                                );
                                // After the error: open the last run's output.
                                let link = ui.add(
                                    egui::Label::new(
                                        RichText::new("View last run")
                                            .size(12.0)
                                            .underline()
                                            .color(crate::theme::link()),
                                    )
                                    .sense(egui::Sense::click())
                                    .selectable(false),
                                );
                                if link.hovered() {
                                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                                }
                                if link.clicked() {
                                    view = Some(i);
                                }
                            }
                            if grokhub_core::source_hidden(&self.cfg.feed_pulse, &self.automations[i].id)
                                && crate::cards::ghost_pill(ui, grokhub_core::HOME_HIDDEN_NOTE)
                            {
                                unhide = Some(self.automations[i].id.clone());
                            }
                        });
                        let primary = scheduled_primary_label(health.is_some());
                        let (ran, remove_hit) = job_row_menu(ui, primary);
                        if remove_hit {
                            remove = Some(i);
                        }
                        if ran {
                            run = Some(i);
                        }
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
        if let Some(i) = view {
            self.open_last_scheduled_run(i);
        } else if let Some(i) = remove {
            if let Some(a) = self.automations.get(i) {
                let id = a.id.clone();
                let title = automation_row_title(a);
                self.arm_remove_job(RemoveJobKind::Scheduled, id, title);
            }
        } else if let Some(i) = run {
            if let Some(a) = self.automations.get(i).cloned() {
                self.fire_night(a, now);
            }
        }
        if let Some(id) = unhide {
            self.undo_hide_automation_from_home(&id);
        }
        ui.add_space(12.0);
    }

    /// "View last run" on a failing scheduled job. A Follow up card for this
    /// automation opens on the workboard. Otherwise the Background history chat,
    /// when it holds this run. Otherwise a short sheet with the stored error.
    pub(super) fn open_last_scheduled_run(&mut self, idx: usize) {
        let Some(a) = self.automations.get(idx).cloned() else {
            return;
        };
        let title = automation_row_title(&a);
        if let Some(card_id) = self.follow_up_for(&a.id) {
            self.nav = Nav::Workboard;
            self.board_view.open = Some(card_id);
            self.status = format!("Last run · {title}");
            return;
        }
        if self.open_background_run(&a, &title) {
            return;
        }
        let body = last_run_sheet_body(&a.health.error, "");
        self.confirm = Some(ConfirmKind::LastRun { title: title.clone(), body });
        self.status = format!("Last run · {title}");
    }

    /// Newest workboard card filed for this automation. A dismissed card is gone.
    fn follow_up_for(&self, automation_id: &str) -> Option<String> {
        self.board
            .iter()
            .filter(|c| {
                c.automation.as_deref() == Some(automation_id) && c.status != BoardStatus::Dismissed
            })
            .max_by_key(|c| c.updated_ms)
            .map(|c| c.id.clone())
    }

    /// Open Chat on the hidden Background thread when a post there is this run.
    fn open_background_run(&mut self, a: &Automation, title: &str) -> bool {
        let Some(idx) = self.threads.iter().position(|t| {
            t.background && t.title == threads::BACKGROUND_THREAD_TITLE
        }) else {
            return false;
        };
        let needles = run_needles(a);
        if needles.is_empty() {
            return false;
        }
        let bodies = self.bodies_of(idx);
        let found = bodies.iter().rev().any(|body| {
            needles.iter().any(|n| body.contains(n.as_str()))
        });
        if !found {
            return false;
        }
        if idx != self.thread_idx {
            self.switch_thread(idx);
        } else if self.messages.is_empty() {
            if let Some(t) = self.threads.get(idx) {
                self.messages = t.messages.clone();
            }
        }
        self.nav = Nav::Chat;
        self.pin_chat_tail();
        self.status = format!("Last run · {title}");
        true
    }

    fn bodies_of(&self, idx: usize) -> Vec<String> {
        if idx == self.thread_idx && !self.messages.is_empty() {
            return self.messages.iter().map(|(_, b)| b.clone()).collect();
        }
        self.threads
            .get(idx)
            .map(|t| t.messages.iter().map(|(_, b)| b.clone()).collect())
            .unwrap_or_default()
    }

    pub(super) fn tick_night(&mut self) -> bool {
        // Your chat turn does not hold a job back: it runs in its own process.
        if self.last_auto_tick.elapsed() < Duration::from_secs(5) {
            return self.night_check_rx.is_some();
        }
        // One scheduled run at a time. The next due job waits for it.
        if self.bg.scheduled_live() {
            return false;
        }
        self.last_auto_tick = Instant::now();
        let clock = Self::local_clock();
        self.roll_today();
        self.daily_auto_day = self.usage.day.clone();
        self.daily_auto_used = self.usage.automation;
        if daily_units_blocked(self.usage.automation, self.cfg.daily_auto_cap) {
            return false;
        }
        if self.budget_holds_scheduled() {
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
        if self.scheduled_waits(&a) {
            return false;
        }
        if let Some(cmd) = night_check_command(&a.check_command) {
            self.spawn_night_check(a.id.clone(), cmd.to_string());
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
                    if night_check_may_fire(self.scheduled_waits(&a)) {
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

    pub(super) fn spawn_night_check(&mut self, id: String, cmd: String) {
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
    }

    /// A due job waits only for the scheduled run before it, or, for a saved
    /// recipe replay that drives the desktop, for your live turn.
    pub(super) fn scheduled_waits(&self, a: &Automation) -> bool {
        self.bg.scheduled_live()
            || (self.running && replay_automation_target(&a.instructions).is_some())
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
        let replay =
            replay_automation_target(&a.instructions).map(|id| self.replay_saved_recipe(id));
        if !night_counts_run(replay) {
            self.mark_auto_skipped(&a.id, now_ms);
            return;
        }
        // Nothing on screen while it runs: no status line, no glow, no strip.
        if replay.is_some() {
            self.mark_auto_ran(&a.id, now_ms);
            self.update_auto_health(&a.id, mark_automation_ok);
            self.note_automation_done(&a.id, &a.name, &a.instructions);
            bump_usage(&mut self.usage, "automation");
            self.daily_auto_used = self.usage.automation;
            self.daily_auto_day = self.usage.day.clone();
            self.persist_usage();
            return;
        }
        // Its own process beside your chat: it never takes the composer, never
        // opens the chat page, and a message you send does not stop it.
        match self.start_scheduled_run(&a) {
            Ok(_) => {
                self.mark_auto_ran(&a.id, now_ms);
                bump_usage(&mut self.usage, "automation");
                self.daily_auto_used = self.usage.automation;
                self.daily_auto_day = self.usage.day.clone();
                self.persist_usage();
            }
            Err(why) => {
                self.mark_auto_skipped(&a.id, now_ms);
                self.status = format!("Night skipped {} ({why})", a.name);
                self.note_auto_failed(&a.id, &why);
            }
        }
    }

    /// Start the job as a background run on the hidden Background chat. It forks
    /// that chat's session, so runs never write into each other or into yours.
    fn start_scheduled_run(&mut self, a: &Automation) -> Result<String, String> {
        let idx = self.ensure_background_history_thread();
        let Some(thread) = self.threads.get_mut(idx) else {
            return Err("The run did not start".into());
        };
        thread.native = true;
        let thread_id = thread.id.clone();
        let task = self.scheduled_task_text(&a.instructions);
        self.start_bg_task(&task, &thread_id, BgOrigin::Scheduled)?;
        self.automation_span(&thread_id, &a.id);
        // Named for the job, not the skill steps riding in front of it.
        let title = grokhub_core::bg_task_title(&a.instructions);
        if let Some(run) = self.bg.runs.last_mut() {
            run.automation = Some(a.id.clone());
            run.title = title.clone();
        }
        Ok(title)
    }

    /// The job's instructions, with a matching skill's steps in front, as the
    /// chat-slot run gave them before scheduled runs moved to the background.
    pub(super) fn scheduled_task_text(&self, instructions: &str) -> String {
        let follow = match_skill(instructions, &self.skill_list)
            .filter(|_| self.policy().injects_skill())
            .map(skill_follow_block);
        apply_skill_follow(instructions, follow.as_deref())
    }

    /// A scheduled run ended. A good run may leave a Follow up card with its report.
    pub(super) fn settle_scheduled_run(&mut self, id: Option<&str>, end: &BgEnd, reply: &str) {
        let Some(id) = id else {
            return;
        };
        match end {
            BgEnd::Done => {
                self.update_auto_health(id, mark_automation_ok);
                let Some(a) = self.automations.iter().find(|x| x.id == id).cloned() else {
                    return;
                };
                if !reply.trim().is_empty() {
                    self.file_automation_follow_up(&a.id, &a.name, &a.instructions, reply);
                }
            }
            BgEnd::Stopped => self.update_auto_health(id, mark_automation_stopped),
            BgEnd::Failed(why) => self.note_auto_failed(id, why),
        }
    }

    pub(super) fn update_auto_health(&mut self, id: &str, f: fn(Automation) -> Automation) {
        let Some(a) = self.automations.iter_mut().find(|x| x.id == id) else {
            return;
        };
        let before = a.health.clone();
        *a = f(a.clone());
        if a.health != before {
            self.persist_automations();
        }
    }

    /// Record the failure on the job. The first failure in a streak posts one feed card.
    pub(super) fn note_auto_failed(&mut self, id: &str, why: &str) {
        let Some(a) = self.automations.iter_mut().find(|x| x.id == id) else {
            return;
        };
        *a = mark_automation_failed(a.clone(), why);
        let first = a.health.fail_streak == 1;
        let name = match a.name.trim() {
            "" => a.instructions.clone(),
            n => n.to_string(),
        };
        let error = a.health.error.clone();
        self.persist_automations();
        if first {
            let mut card = automation_failed_card(id, &name, &error, now_ms());
            hold_if_quiet(&mut card, self.quiet_now());
            self.post_feed_card(card);
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
        if self.budget_holds_scheduled() {
            return false;
        }
        let due = due_loops(&self.grok_loops, now_ms());
        let Some(row) = due.into_iter().next() else {
            return false;
        };
        self.fire_loop(row);
        true
    }

    /// Live update-feed hook. A finished `/loop` posts `automation_done`.
    pub(super) fn poll_grok_loop(&mut self) -> bool {
        let Some((id, rx)) = self.grok_loop_rx.take() else {
            return false;
        };
        match rx.try_recv() {
            Ok(text) => {
                let prompt = self
                    .grok_loops
                    .iter()
                    .find(|x| x.id == id)
                    .map(|r| r.prompt.clone())
                    .unwrap_or_default();
                let summary = if let Ok(turn) = grokhub_core::wire::parse_single_turn(&text) {
                    super::background::hide_background_session(&turn.session_id);
                    if let Some(row) = self.grok_loops.iter_mut().find(|x| x.id == id) {
                        row.session_id = Some(turn.session_id);
                    }
                    self.persist_loops();
                    turn.text
                } else {
                    text
                };
                self.note_automation_done(&id, &prompt, &summary);
                self.file_automation_follow_up(&id, &loop_card_name(&prompt), &prompt, &summary);
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
        self.automation_span(super::background::LOOP_TRACE, &row.id);
        self.spawn_native_loop(row);
    }

    pub(super) fn tick_session_suggestions(&mut self) {
        let today = Self::local_day();
        if !review_due(
            self.suggestions.last_session_suggest_day.as_deref(),
            &today,
            &Self::local_clock(),
            self.dream_hour(),
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
        self.skill_suggestions_to_ideas();
        self.persist_suggestions();
    }

    /// Suggested skills live on the Ideas board now, as Skill ideas, not on the
    /// Skills page. Each one moves over once; Apply on the card saves the skill.
    pub(super) fn skill_suggestions_to_ideas(&mut self) {
        if self.suggestions.skills.is_empty() {
            return;
        }
        let have: Vec<String> = self.skill_list.iter().map(|s| s.name.clone()).collect();
        let have: Vec<&str> = have.iter().map(String::as_str).collect();
        let items = std::mem::take(&mut self.suggestions.skills);
        let mut posted = 0;
        for item in &items {
            if grokhub_core::post_skill_idea(
                &mut self.updates,
                &mut self.cfg.feed_pulse,
                now_ms(),
                item,
                &have,
            ) {
                posted += 1;
            }
        }
        if posted > 0 {
            self.persist_updates();
            self.persist_cfg();
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
            self.dream_hour(),
        ) {
            return;
        }
        if !grokhub_core::review_worth_tokens(
            self.learning.total_turns,
            self.learning.reviewed_turns,
        ) {
            return;
        }
        if !self.heartbeat_may(grokhub_core::ProactiveAct::Review, now_ms()) {
            return;
        }
        self.learning.reviewed_turns = self.learning.total_turns;
        self.spawn_review();
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
        self.spawn_native_review();
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

    /// A skill or automation from the review needs the same reason an idea does:
    /// work they repeat, or lasting context. One per topic.
    pub(super) fn keep_reasoned_suggestions(&self, items: &mut Vec<grokhub_core::LearnedSuggestion>) {
        let (_, inputs) = self.idea_request();
        let ground = grokhub_core::IdeaGround {
            asks: &inputs.asks,
            lasting: &inputs.lasting,
        };
        let mut kept: Vec<String> = Vec::new();
        items.retain(|item| {
            let kind = match item.kind {
                grokhub_core::SuggestionKind::Skill => grokhub_core::IdeaKind::Skill,
                grokhub_core::SuggestionKind::Auto => grokhub_core::IdeaKind::Automation,
                grokhub_core::SuggestionKind::Connector => return true,
            };
            let topic = format!(
                "{} {} {} {}",
                item.title,
                item.body,
                item.seed.as_deref().unwrap_or(""),
                item.trigger.as_deref().unwrap_or("")
            );
            if !grokhub_core::idea_has_reason(kind, &topic, &ground)
                || kept.iter().any(|k| grokhub_core::same_topic(k, &topic))
                || grokhub_core::turned_down_topic(&self.cfg.feed_pulse, &item.title)
                || item
                    .name
                    .as_deref()
                    .is_some_and(|n| grokhub_core::turned_down_topic(&self.cfg.feed_pulse, n))
            {
                return false;
            }
            kept.push(topic);
            true
        });
    }

    fn recent_user_lines(&self) -> Vec<String> {
        let mut out = Vec::new();
        let mut push = |text: &str| {
            if out.len() >= 40 {
                return;
            }
            let line: String = text.chars().take(280).collect();
            if !line.trim().is_empty() {
                out.push(line);
            }
        };
        for (role, text) in self.messages.iter() {
            if role == "user" {
                push(text);
            }
        }
        for thread in &self.threads {
            for (role, text) in thread.messages.iter() {
                if role == "user" {
                    push(text);
                }
            }
        }
        out
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
                let said = self.recent_user_lines();
                let mut items = dedupe_suggestions(
                    grokhub_core::drop_echoed_suggestions(parse_suggest_lines(&text), &said),
                    &skill_names,
                    &auto_names,
                    &live_tools,
                );
                self.keep_reasoned_suggestions(&mut items);
                self.heartbeat_outcome(
                    grokhub_core::ProactiveAct::Review,
                    if items.is_empty() {
                        grokhub_core::ActOutcome::Empty
                    } else {
                        grokhub_core::ActOutcome::Useful
                    },
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
                self.skill_suggestions_to_ideas();
                let suggestions = self.suggestions.clone();
                std::thread::spawn(move || {
                    let _ = crate::store::save_suggestions(&suggestions);
                });
            }
            Err(e) => {
                self.heartbeat_outcome(
                    grokhub_core::ProactiveAct::Review,
                    grokhub_core::ActOutcome::Empty,
                );
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
        let name = self
            .automations
            .iter()
            .find(|x| x.id == id)
            .map(|a| a.name.clone())
            .unwrap_or_default();
        if let Some(a) = self.automations.iter_mut().find(|x| x.id == id) {
            *a = mark_automation_ran(a.clone(), now);
        }
        if !name.is_empty() {
            self.engine_note("automations", &format!("ran:{name}"), &name);
        }
        let list = self.automations.clone();
        std::thread::spawn(move || {
            let _ = crate::night::save(&list);
        });
    }

    pub(super) fn mark_auto_skipped(&mut self, id: &str, now: u64) {
        let clock = Self::local_clock();
        let name = self
            .automations
            .iter()
            .find(|x| x.id == id)
            .map(|a| a.name.clone())
            .unwrap_or_default();
        if let Some(a) = self.automations.iter_mut().find(|x| x.id == id) {
            *a = mark_automation_skipped(a.clone(), now, clock);
        }
        if !name.is_empty() {
            self.engine_note("automations", &format!("skipped:{name}"), &name);
        }
        let list = self.automations.clone();
        std::thread::spawn(move || {
            let _ = crate::night::save(&list);
        });
    }
}

/// A `/loop` has no name; its card is named by the start of its prompt.
pub(super) fn loop_card_name(prompt: &str) -> String {
    let p = prompt.trim().trim_start_matches("/loop").trim();
    // Drop the interval word ("12h", "1d") the loop starts with.
    let p = match p.split_once(' ') {
        Some((head, rest)) if head.chars().next().is_some_and(|c| c.is_ascii_digit()) => rest.trim(),
        _ => p,
    };
    let mut out: String = p.chars().take(60).collect();
    if p.chars().count() > 60 {
        out.push('…');
    }
    let mut chars = out.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}
