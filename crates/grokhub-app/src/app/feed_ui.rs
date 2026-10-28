//! Feed posting, card actions, and Pulse/home helpers.
//! Cards paint on Pulse. The empty main chat no longer stacks a Home deck.

use super::*;
use crate::ui_kit::{badge, card_frame, empty_state, BadgeTone, CardVariant};
use grokhub_core::{
    automation_done_card, dismiss_update_at, file_idea_todo, hold_if_quiet, idea_todo_title,
    mark_update_opened, parse_lookup, post_help, post_update, quiet_hours_active,
    remember_dismissed_source, route_schedule, schedule_created_card, tick_feed_pulse,
    unpin_feed_idea, DigestEdition, DigestMaterial, PausedJob, PulseNow, RepeatedAction, TasteNote,
    UpdateAction, UpdateCard, UpdateKind, UpdateStatus,
};

#[derive(Debug, Clone, Default)]
pub(super) struct IdeaInputs {
    /// Their own lines (asks, memory). An idea must not copy them.
    pub sources: Vec<String>,
    /// Titles on the board, turned down, or already running.
    pub taken: Vec<String>,
    /// Recent asks, newest first: an automation or skill needs a repeated one.
    pub asks: Vec<String>,
    /// Lasting context: memory and USER.md lines, open workboard cards.
    pub lasting: Vec<String>,
}

/// Longest wait for the model's idea list before the board gives up on that ask.
pub(super) const IDEAS_WAIT_MS: u64 = 180_000;

/// Card actions formerly driven by the empty-chat Home deck. Kept for tests and
/// `close_home_card` / dismiss wiring; Pulse paints its own acts.
#[cfg(test)]
pub(super) enum FeedAct {
    Open(String),
    /// × dismiss / hide-from-home on a feed card.
    Dismiss(String),
    Discuss(String),
    Like(String),
    Link(String),
    /// Undo on a Done-for-you card. Pointer click only.
    Undo(String),
    /// "Don't do this again" on a Done-for-you card. Pointer click only.
    NeverAgain(String),
    /// Retry on a crash card.
    Retry(String),
    /// Delete recording on a screen recording card.
    DeleteRecording(String),
    /// Retry now on a no-route wait card.
    RetryNow,
    /// Save / Not interested / More like this on a source post.
    Source(String, super::pulse_ui::SourceAct),
}

impl Cabin {
    pub(super) fn note_automation_done(&mut self, source_id: &str, title: &str, summary: &str) {
        if title.trim().is_empty() && summary.trim().is_empty() {
            return;
        }
        let mut card = automation_done_card(source_id, title, summary, now_ms());
        hold_if_quiet(&mut card, self.quiet_now());
        self.post_feed_card(card);
    }

    /// One Home card per store that would not load and was moved aside: a settings or
    /// feed file (`config::quarantine`), the hub state, or a damaged memory note.
    /// Notes for files outside this cabin's config folder are dropped.
    pub(super) fn poll_recovered_stores(&mut self) {
        let dir = crate::config::config_dir();
        for r in grokhub_core::recovered::drain() {
            if r.path.starts_with(&dir) {
                self.post_feed_card(grokhub_core::recovered_card(&r, now_ms()));
            }
        }
    }

    /// `commit_schedule` calls this after a clock job or interval loop is saved.
    pub(super) fn note_schedule_created(&mut self, source_id: &str, title: &str, when_label: &str) {
        if title.trim().is_empty() && when_label.trim().is_empty() {
            return;
        }
        let card = schedule_created_card(source_id, title, when_label, now_ms());
        self.post_feed_card(card);
    }

    /// Retry on a crash card: open the chat the run was in and send the
    /// card's retry line there (`/retry`, or `/bg …` for a background run).
    pub(super) fn crash_retry(&mut self, id: &str) {
        let Some(card) = self.updates.iter().find(|c| c.id == id).cloned() else {
            return;
        };
        let Some(line) = card
            .prompt
            .clone()
            .filter(|_| grokhub_core::is_crash_card(&card))
        else {
            return;
        };
        if mark_update_opened(&mut self.updates, id) {
            self.persist_updates();
        }
        self.follow_update_action(card.action.clone());
        self.send_chat(line);
    }

    pub(super) fn post_feed_card(&mut self, card: UpdateCard) {
        if !post_update(&mut self.updates, card) {
            return;
        }
        let now = now_ms();
        let rank = grokhub_core::rank_home_events(
            &self.updates,
            &self.cfg.feed_pulse,
            &self.card_prefs,
            now,
        );
        if grokhub_core::record_home_floors(&mut self.cfg.feed_pulse, &rank.deck, now) {
            self.persist_cfg();
        }
        self.persist_updates();
    }

    pub(super) fn log_card_signal(
        &mut self,
        card: &UpdateCard,
        event: grokhub_core::CardEvent,
        after_open: Option<bool>,
    ) {
        let now = now_ms();
        let row = grokhub_core::signal_for(card, event, now, after_open);
        grokhub_core::append_signal(&crate::config::config_dir(), &row);
        grokhub_core::apply_card_event(&mut self.card_prefs, card, event, after_open, now);
        let _ = crate::card_prefs::save(&self.card_prefs);
    }

    /// Learn how far the user took a card. The same step twice logs once.
    pub(super) fn log_card_use(
        &mut self,
        card: &UpdateCard,
        depth: grokhub_core::UseDepth,
        did: grokhub_core::UseAction,
    ) {
        let now = now_ms();
        if !grokhub_core::record_card_use(&mut self.card_prefs, card, depth, did, now) {
            return;
        }
        let row = grokhub_core::signal_for(card, grokhub_core::use_event(depth), now, None);
        grokhub_core::append_signal(&crate::config::config_dir(), &row);
        let _ = crate::card_prefs::save(&self.card_prefs);
    }

    /// A workboard card filed from a Home card reached Done: that use is complete.
    pub(super) fn note_finished_card_uses(&mut self) {
        let done: Vec<UpdateCard> = self
            .updates
            .iter()
            .filter(|card| {
                card.board_id.as_deref().is_some_and(|board_id| {
                    self.board
                        .iter()
                        .any(|b| b.id == board_id && b.status == BoardStatus::Done)
                })
            })
            .filter(|card| {
                grokhub_core::used_depth(&self.card_prefs, card)
                    < grokhub_core::UseDepth::Completed as u8
            })
            .cloned()
            .collect();
        for card in done {
            self.log_card_use(
                &card,
                grokhub_core::UseDepth::Completed,
                grokhub_core::UseAction::FinishedTodo,
            );
        }
    }

    pub(super) fn undo_hide_automation_from_home(&mut self, source_id: &str) {
        if !grokhub_core::source_hidden(&self.cfg.feed_pulse, source_id) {
            return;
        }
        grokhub_core::unhide_home_source(&mut self.cfg.feed_pulse, source_id);
        let card = self
            .updates
            .iter()
            .find(|c| c.kind == UpdateKind::AutomationDone && c.source_id == source_id)
            .cloned()
            .unwrap_or_else(|| grokhub_core::automation_done_card(source_id, "", "", now_ms()));
        self.log_card_signal(&card, grokhub_core::CardEvent::Unhidden, None);
        self.persist_cfg();
    }

    pub(super) fn quiet_now(&self) -> bool {
        let clock = Self::local_clock();
        quiet_hours_active(&clock.hm(), &self.cfg.quiet_start, &self.cfg.quiet_end)
    }

    /// The governor's Pulse refresh job: idea expiry, quiet release, digest
    /// clock. Not the nightly review, and not the ideas ask (its own job).
    pub(super) fn tick_feed_pulse(&mut self) {
        let now = now_ms();
        let quiet = self.quiet_now();
        let brief = self.brief_gate(Self::local_clock().hour, now);
        let pulse = self.cfg.feed_pulse.clone();
        let on_clock = brief.unwrap_or(
            pulse.last_digest_ms == 0
                || now.saturating_sub(pulse.last_digest_ms) >= pulse.digest_ms,
        );
        let want_digest = pulse.digest_on && (pulse.digest_held || on_clock);
        let (user_md, memory_md, soul_md, taste) = if want_digest && !quiet {
            (
                crate::config::read_memory("USER.md"),
                crate::config::read_memory("MEMORY.md"),
                crate::config::read_memory("SOUL.md"),
                self.digest_taste(),
            )
        } else {
            (String::new(), String::new(), String::new(), Vec::new())
        };
        if want_digest && !quiet {
            self.seed_interests(&user_md, &memory_md);
        }
        let project = self.cfg.project_dir.clone();
        let steer = if want_digest && !quiet {
            grokhub_core::digest_steer(&self.cfg.digest_brief, &user_md, &memory_md, &project)
        } else {
            self.cfg.digest_brief.clone()
        };
        let parsed = self.digest_pending.as_deref().map(parse_lookup);
        let edition = parsed.as_ref().map(|item| DigestEdition {
            found: item.found,
            refused: item.refused,
            title: item.title.as_str(),
            body: item.body.as_str(),
        });
        let links = parsed
            .as_ref()
            .map(|item| item.links.clone())
            .unwrap_or_default();
        let material = DigestMaterial {
            brief: &steer,
            user_md: &user_md,
            memory_md: &memory_md,
            soul_md: &soul_md,
            links: &links,
            taste: &taste,
            edition,
        };
        let fed_edition = edition.is_some();
        let tick = tick_feed_pulse(
            &mut self.updates,
            &mut self.cfg.feed_pulse,
            PulseNow {
                now_ms: now,
                quiet,
                brief,
            },
            material,
        );
        if fed_edition && tick.digest_consumed {
            self.digest_pending = None;
        }
        self.digest_wants_lookup = tick.digest_needs_lookup;
        if tick.digest_needs_lookup {
            self.digest_steer = steer;
            self.digest_interests = self.interests_prompt();
        }
        self.note_finished_card_uses();
        let help_changed = self.note_help_offers(now, quiet);
        if tick.cards_changed || help_changed {
            self.persist_updates();
        }
        if tick.pulse_changed || help_changed {
            self.persist_cfg();
        }
        self.tick_nudge_watchers();
        self.tick_ask_offers();
        self.tick_hobby_paths();
    }

    fn digest_taste(&self) -> Vec<TasteNote> {
        let mut notes = Vec::new();
        for card in &self.updates {
            if !matches!(card.kind, UpdateKind::Idea | UpdateKind::Digest) {
                continue;
            }
            let said = card
                .discuss_thread
                .as_deref()
                .and_then(|id| self.threads.iter().find(|t| t.id == id))
                .map(|thread| {
                    thread
                        .messages
                        .iter()
                        .filter(|(role, _)| role == "user")
                        .map(|(_, text)| text.trim())
                        .filter(|text| !text.is_empty())
                        .collect::<Vec<_>>()
                        .join(" ")
                })
                .unwrap_or_default();
            if card.reaction.is_none() && said.is_empty() {
                continue;
            }
            notes.push(TasteNote {
                title: card.title.clone(),
                reaction: card.reaction,
                said,
            });
        }
        notes
    }

    /// Local situation cards. No model call and no desktop ping in this function.
    fn note_help_offers(&mut self, now: u64, quiet: bool) -> bool {
        let paused_owned: Vec<(String, String, String)> = self
            .board
            .iter()
            .filter(|card| card.detail == "Paused. This is where to resume.")
            .map(|card| (card.id.clone(), card.title.clone(), card.detail.clone()))
            .collect();
        let paused: Vec<PausedJob> = paused_owned
            .iter()
            .map(|(id, title, detail)| PausedJob {
                id: id.as_str(),
                title: title.as_str(),
                detail: detail.as_str(),
            })
            .collect();
        let names: Vec<String> = self
            .automations
            .iter()
            .map(|job| job.name.to_ascii_lowercase())
            .chain(
                self.grok_loops
                    .iter()
                    .map(|job| job.prompt.to_ascii_lowercase()),
            )
            .collect();
        let mut repeats_owned: Vec<(String, String, u32, bool, bool)> = Vec::new();
        for dests in self.chip_memory.transitions.values() {
            for (key, count) in dests {
                if *count < 3 {
                    continue;
                }
                let Some(hit) = self.chip_memory.hits.iter().find(|hit| hit.key == *key) else {
                    continue;
                };
                let label = hit.label.clone();
                let key_l = key.to_ascii_lowercase();
                let label_l = label.to_ascii_lowercase();
                let automated = names.iter().any(|name| name == &key_l || name == &label_l);
                if let Some(row) = repeats_owned.iter_mut().find(|row| row.0 == *key) {
                    row.2 = row.2.max(*count);
                } else {
                    repeats_owned.push((key.clone(), label, *count, hit.dismisses > 0, automated));
                }
            }
        }
        let repeats: Vec<RepeatedAction> = repeats_owned
            .iter()
            .map(|(key, label, count, dismissed, automated)| RepeatedAction {
                key: key.as_str(),
                label: label.as_str(),
                count: *count,
                dismissed: *dismissed,
                automated: *automated,
            })
            .collect();
        let before = self.updates.len();
        let seen_before = self.cfg.feed_pulse.paused_seen.clone();
        let help = post_help(
            &mut self.updates,
            &mut self.cfg.feed_pulse,
            now,
            quiet,
            self.offer_repeated,
            &paused,
            &repeats,
            &grokhub_core::brief_for(&self.learning, "ideas"),
        );
        if help.ping {
            if let Some(card) = self.updates.iter().rev().find(|card| {
                card.kind == UpdateKind::Suggestion
                    && card.status == UpdateStatus::Unread
                    && !card.held
                    && !card.pulse.quiet_batched
            }) {
                self.situation_ping =
                    Some((card.title.clone(), card.body.clone().unwrap_or_default()));
            }
        }
        help.ideas > 0
            || help.posted
            || self.updates.len() != before
            || self.cfg.feed_pulse.paused_seen != seen_before
    }

    /// One lookup a day, off the UI thread, on the native engine.
    pub(super) fn follow_feed_lookup(&mut self) {
        if !self.digest_wants_lookup || self.digest_busy || self.digest_pending.is_some() {
            return;
        }
        self.spawn_native_digest();
    }

    pub(super) fn poll_digest_lookup(&mut self) {
        let Some(rx) = self.digest_rx.take() else {
            return;
        };
        match rx.try_recv() {
            Ok(Ok(text)) => {
                self.digest_busy = false;
                self.digest_pending = Some(text);
            }
            Ok(Err(err)) => {
                self.digest_busy = false;
                self.digest_pending = Some("NONE".into());
                if !err.is_empty() {
                    self.status = err;
                }
            }
            Err(mpsc::TryRecvError::Empty) => {
                self.digest_rx = Some(rx);
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                self.digest_busy = false;
                self.digest_pending = Some("NONE".into());
            }
        }
    }

    /// The situation card only. News stays on the home feed. Quiet hours and a focused window stay silent.
    pub(super) fn release_situation_ping(&mut self) {
        let Some((title, body)) = self.situation_ping.take() else {
            return;
        };
        if self.quiet_now() || self.window_focused {
            return;
        }
        let line = if body.is_empty() {
            title
        } else {
            format!("{title} {body}")
        };
        crate::notify::ping(grokhub_core::brand::product_name(), &line);
    }

    #[cfg(test)]
    pub(super) fn apply_feed_act(&mut self, act: Option<FeedAct>) {
        match act {
            Some(FeedAct::Dismiss(id)) => self.close_home_card(&id),
            Some(FeedAct::Open(id)) => self.open_feed_card(&id),
            Some(FeedAct::Discuss(id)) => self.discuss_card(&id),
            Some(FeedAct::Like(id)) => self.pulse_like(&id, &Self::local_day()),
            Some(FeedAct::Source(id, did)) => self.source_post_act(&id, did),
            Some(FeedAct::Undo(id)) => self.done_for_you_undo(&id),
            Some(FeedAct::NeverAgain(id)) => self.done_for_you_never(&id),
            Some(FeedAct::Retry(id)) => self.crash_retry(&id),
            Some(FeedAct::DeleteRecording(id)) => self.arm_delete_recording(&id),
            Some(FeedAct::RetryNow) => self.retry_model_wait_now(),
            Some(FeedAct::Link(url)) => {
                self.follow_update_action(Some(UpdateAction::DeepLink { href: url }));
            }
            None => {}
        }
    }

    /// Same build on the feed and the Ideas surface. Files one Todo titled with the task.
    pub(super) fn build_idea(&mut self, id: &str) {
        let Some(idea) = self
            .updates
            .iter()
            .find(|c| c.id == id && c.kind == UpdateKind::Idea)
            .cloned()
        else {
            return;
        };
        if idea.built {
            self.nav = Nav::Workboard;
            return;
        }
        let task = idea_todo_title(&idea.title, idea.body.as_deref().unwrap_or(""));
        let board_id = file_idea_todo(&mut self.board, &task, "");
        self.flush_board();
        if let Some(card) = self.updates.iter_mut().find(|c| c.id == id) {
            card.built = true;
            card.board_id = Some(board_id);
            card.status = UpdateStatus::Opened;
        }
        if let Some(card) = self.updates.iter().find(|c| c.id == id).cloned() {
            self.log_card_use(
                &card,
                grokhub_core::UseDepth::Ran,
                grokhub_core::UseAction::FiledTodo,
            );
        }
        self.persist_updates();
    }

    /// The offer opens the schedule box. Add on that page is what saves it.
    fn open_offer_on_automations(&mut self, id: &str) {
        if !mark_update_opened(&mut self.updates, id) {
            return;
        }
        let seed = self
            .updates
            .iter()
            .find(|c| c.id == id)
            .map(|c| {
                let from_preview = grokhub_core::automate_accept_seed(c);
                if from_preview.trim().is_empty() {
                    c.title.clone()
                } else {
                    from_preview
                }
            })
            .unwrap_or_default();
        self.persist_updates();
        if let Some(card) = self.updates.iter().find(|c| c.id == id).cloned() {
            self.log_card_use(
                &card,
                grokhub_core::UseDepth::Opened,
                grokhub_core::UseAction::OpenedAutomations,
            );
        }
        self.night_nl = seed;
        self.auto_compose = true;
        self.nav = Nav::Night;
    }

    pub(super) fn accept_automate_offer(&mut self, id: &str) {
        if !mark_update_opened(&mut self.updates, id) {
            return;
        }
        let seed = self
            .updates
            .iter()
            .find(|c| c.id == id)
            .map(|c| {
                let from_preview = grokhub_core::automate_accept_seed(c);
                if from_preview.trim().is_empty() {
                    c.title.clone()
                } else {
                    from_preview
                }
            })
            .unwrap_or_default();
        self.persist_updates();
        let route = route_schedule(&seed);
        let (depth, did) = if route.is_some() {
            (
                grokhub_core::UseDepth::Ran,
                grokhub_core::UseAction::Automated,
            )
        } else {
            (
                grokhub_core::UseDepth::Opened,
                grokhub_core::UseAction::OpenedAutomations,
            )
        };
        if let Some(card) = self.updates.iter().find(|c| c.id == id).cloned() {
            self.log_card_use(&card, depth, did);
        }
        if let Some(route) = route {
            let _ = self.commit_schedule(
                route,
                grokhub_agent::harness::Origin::SelfManage,
                "from an Automate offer",
            );
        }
    }

    /// Once per launch: purge template idea cards. The automatic model ask comes
    /// from the governor's Ideas job (`maybe_suggest_ideas`).
    pub(super) fn ensure_useful_ideas(&mut self) {
        if self.ideas_filled {
            return;
        }
        self.ideas_filled = true;
        let inputs = self.idea_inputs();
        let ground = grokhub_core::IdeaGround {
            asks: &inputs.asks,
            lasting: &inputs.lasting,
        };
        let purged = grokhub_core::purge_template_ideas(&mut self.updates)
            + grokhub_core::purge_one_off_ideas(&mut self.updates, &ground);
        if purged > 0 {
            self.persist_updates();
        }
        // Skill suggestions saved by an older cabin move to the Ideas board once.
        self.skill_suggestions_to_ideas();
    }

    /// Ask the model for ideas grounded in this person's work. The automatic ask
    /// comes from the heartbeat (`tick_feed_pulse`). Runs when the board has fewer
    /// than a handful of generated ideas and the last ask is hours old, never in
    /// quiet hours or over the token budget. `force` is the Suggest ideas button.
    pub(super) fn maybe_suggest_ideas(&mut self, force: bool) {
        if self.ideas_rx.is_some() {
            return;
        }
        let now = now_ms();
        if !force {
            let live = grokhub_core::live_generated_ideas(&self.updates);
            // The governor's Ideas job: it never runs in quiet hours.
            if !grokhub_core::ideas_refresh_due(self.cfg.feed_pulse.last_ideas_ms, now, live)
                || self.budget_holds_scheduled()
            {
                return;
            }
            if !self.heartbeat_may(grokhub_core::ProactiveAct::Ideas, now) {
                return;
            }
        }
        let (prompt, inputs) = self.idea_request();
        self.cfg.feed_pulse.last_ideas_ms = now;
        self.persist_cfg();
        let (tx, rx) = mpsc::channel();
        self.ideas_rx = Some((rx, inputs));
        self.spawn_native_ideas(prompt, tx, (!force).then_some(JobKind::Ideas));
    }

    /// The prompt plus what the reply must not copy (their own lines) or repeat,
    /// and what an idea has to stand on.
    pub(super) fn idea_request(&self) -> (String, IdeaInputs) {
        self.idea_parts(true)
    }

    /// What an idea is checked against, without the prompt (which reads the
    /// local clock through `date` on Linux).
    pub(super) fn idea_inputs(&self) -> IdeaInputs {
        self.idea_parts(false).1
    }

    fn idea_parts(&self, with_prompt: bool) -> (String, IdeaInputs) {
        let user_md = crate::config::read_memory("USER.md");
        let memory_md = crate::config::read_memory("MEMORY.md");
        // Enough history to tell a routine from a one-time job.
        let asks = self.recent_asks(30);
        let open_cards: Vec<String> = self
            .board
            .iter()
            .filter(|c| c.status.column().is_some() && c.status != BoardStatus::Done)
            .map(|c| c.title.clone())
            .collect();
        let automations: Vec<String> = self
            .automations
            .iter()
            .map(|a| match a.name.trim() {
                "" => a.instructions.clone(),
                n => n.to_string(),
            })
            .chain(self.grok_loops.iter().map(|l| l.prompt.clone()))
            .collect();
        let skills: Vec<String> = self.skill_list.iter().map(|s| s.name.clone()).collect();
        let existing: Vec<String> = self
            .updates
            .iter()
            .filter(|c| c.kind == UpdateKind::Idea)
            .map(|c| c.title.clone())
            .collect();
        // Newest first, what they said no to ahead of the rest: the prompt shows only 12.
        let rejected = grokhub_core::turned_down_titles(&self.cfg.feed_pulse, &existing);
        let prompt = if !with_prompt {
            String::new()
        } else {
            let clock = Self::local_clock();
            let weekday = match clock.weekday {
                0 => "Sunday",
                1 => "Monday",
                2 => "Tuesday",
                3 => "Wednesday",
                4 => "Thursday",
                5 => "Friday",
                _ => "Saturday",
            };
            grokhub_core::idea_prompt(&grokhub_core::IdeaContext {
                user_md: &user_md,
                memory_md: &memory_md,
                recent_asks: &asks,
                open_cards: &open_cards,
                automations: &automations,
                skills: &skills,
                existing: &existing,
                rejected: &rejected,
                hour: clock.hour as u8,
                weekday,
            })
        };
        let memory_lines: Vec<String> = user_md
            .lines()
            .chain(memory_md.lines())
            .map(|l| l.trim().trim_start_matches(['-', '*', '#', ' ']).trim())
            .filter(|l| l.len() >= 12)
            .map(str::to_string)
            .collect();
        let mut sources = asks.clone();
        sources.extend(memory_lines.iter().cloned());
        let mut lasting = memory_lines;
        lasting.extend(open_cards);
        let mut taken = existing;
        taken.extend(rejected);
        taken.extend(automations);
        (
            prompt,
            IdeaInputs {
                sources,
                taken,
                asks,
                lasting,
            },
        )
    }

    /// What they asked lately, newest first: real asks only (no slash commands,
    /// tool results, or "no, still…" feedback), each once.
    pub(super) fn recent_asks(&self, n: usize) -> Vec<String> {
        let mut threads: Vec<&crate::threads::ChatThread> =
            self.threads.iter().filter(|t| !t.background).collect();
        threads.sort_by_key(|t| std::cmp::Reverse(t.accessed_ms));
        let open = self.messages.iter().rev();
        let rest = threads.into_iter().flat_map(|t| t.messages.iter().rev());
        let mut out: Vec<String> = Vec::new();
        for (role, text) in open.chain(rest) {
            if out.len() >= n {
                break;
            }
            if role != "user" {
                continue;
            }
            let t = text.trim();
            if t.len() < 8
                || t.starts_with('/')
                || grokhub_core::is_workload_user(t)
                || grokhub_core::is_feedback_ask(t)
                || !grokhub_core::is_plain_text(t)
            {
                continue;
            }
            let line: String = t.lines().next().unwrap_or("").chars().take(200).collect();
            if !out.iter().any(|o| o.eq_ignore_ascii_case(&line)) {
                out.push(line);
            }
        }
        out
    }

    /// Post what came back. A reply that yields nothing leaves the board as it was.
    pub(super) fn poll_ideas(&mut self) {
        let Some((rx, inputs)) = self.ideas_rx.take() else {
            return;
        };
        match rx.try_recv() {
            Ok(raw) => {
                let parsed = grokhub_core::parse_ideas(&raw, &inputs.sources, &inputs.taken);
                // Only ideas with a reason: repeated work or lasting context, one per topic.
                let seeds = grokhub_core::keep_reasoned_ideas(
                    parsed,
                    &grokhub_core::IdeaGround {
                        asks: &inputs.asks,
                        lasting: &inputs.lasting,
                    },
                    &inputs.taken,
                );
                let names: Vec<&str> = inputs.taken.iter().map(String::as_str).collect();
                let n = grokhub_core::post_generated_ideas(
                    &mut self.updates,
                    &mut self.cfg.feed_pulse,
                    now_ms(),
                    &seeds,
                    &names,
                    &grokhub_core::brief_for(&self.learning, "ideas"),
                );
                if n > 0 {
                    self.persist_updates();
                    self.persist_cfg();
                }
                self.heartbeat_outcome(
                    grokhub_core::ProactiveAct::Ideas,
                    if n > 0 {
                        grokhub_core::ActOutcome::Useful
                    } else {
                        grokhub_core::ActOutcome::Empty
                    },
                );
                if self.nav == Nav::Pulse {
                    self.status = match n {
                        0 => "No new ideas this time".into(),
                        1 => "1 new idea".into(),
                        n => format!("{n} new ideas"),
                    };
                }
            }
            Err(mpsc::TryRecvError::Empty) => {
                // A hung ask must not leave the Ideas button on "Thinking…" for good.
                if now_ms().saturating_sub(self.cfg.feed_pulse.last_ideas_ms) > IDEAS_WAIT_MS {
                    if self.nav == Nav::Pulse {
                        self.status = "For You took too long. Try Suggest for you again.".into();
                    }
                } else {
                    self.ideas_rx = Some((rx, inputs));
                }
            }
            Err(mpsc::TryRecvError::Disconnected) => {}
        }
    }

    pub(super) fn job_is_idea_talk(&self) -> bool {
        let Some(id) = self.chat_job_thread.as_deref() else {
            return false;
        };
        self.threads
            .iter()
            .any(|t| t.id == id && t.background && t.title.starts_with("Discuss · "))
    }

    /// A paused-job suggestion opens that job, like Open chat on its Workboard
    /// card; a job with no chat opens the Workboard. Resuming stays the normal
    /// path from there.
    fn open_paused_job(&mut self, card: &UpdateCard, job: &str) {
        let thread = self
            .board
            .iter()
            .find(|c| c.id == job)
            .and_then(|c| c.thread_id.clone())
            .filter(|tid| self.threads.iter().any(|t| t.id == *tid));
        let did = if thread.is_some() {
            grokhub_core::UseAction::OpenedChat
        } else {
            grokhub_core::UseAction::OpenedBoard
        };
        self.log_card_use(card, grokhub_core::UseDepth::Opened, did);
        match thread {
            Some(tid) => self.open_board_thread(&tid),
            None => self.nav = Nav::Workboard,
        }
        if let Some(saved) = self.updates.iter_mut().find(|c| c.id == card.id) {
            saved.status = UpdateStatus::Opened;
        }
        self.persist_updates();
    }

    pub(super) fn discuss_card(&mut self, id: &str) {
        let Some(card) = self.updates.iter().find(|c| c.id == id).cloned() else {
            return;
        };
        if card.kind == UpdateKind::Idea {
            self.open_idea_on_board(&card.id);
            return;
        }
        if !matches!(card.kind, UpdateKind::Digest | UpdateKind::Suggestion) {
            return;
        }
        if let Some(job) = grokhub_core::paused_job_of(&card) {
            self.open_paused_job(&card, job);
            return;
        }
        self.log_card_use(
            &card,
            grokhub_core::UseDepth::Opened,
            grokhub_core::UseAction::Discussed,
        );
        if let Some(thread_id) = card.discuss_thread.as_deref() {
            if let Some(idx) = self.threads.iter().position(|t| t.id == thread_id) {
                self.switch_thread(idx);
                self.nav = Nav::Chat;
                self.composer_want_focus = true;
                return;
            }
        }
        let context = grokhub_core::discuss_context(&card);
        let title = format!("Discuss · {}", card.title);
        self.new_thread(false);
        let thread_id = self
            .threads
            .get(self.thread_idx)
            .map(|t| t.id.clone())
            .unwrap_or_default();
        if let Some(thread) = self.threads.get_mut(self.thread_idx) {
            thread.title = title;
            thread.title_locked = true;
            thread.messages_mut().push(("assistant".into(), context));
            self.messages = thread.messages.clone();
        }
        if let Some(card) = self.updates.iter_mut().find(|c| c.id == id) {
            card.discuss_thread = Some(thread_id);
            card.status = UpdateStatus::Opened;
        }
        self.nav = Nav::Chat;
        self.composer_want_focus = true;
        self.persist_updates();
        self.persist();
    }

    pub(super) fn open_feed_card(&mut self, id: &str) {
        let kind = self.updates.iter().find(|c| c.id == id).map(|c| c.kind);
        if matches!(kind, Some(UpdateKind::Idea)) {
            self.discuss_card(id);
            return;
        }
        if matches!(kind, Some(UpdateKind::AutomateOffer)) {
            self.open_offer_on_automations(id);
            return;
        }
        if !mark_update_opened(&mut self.updates, id) {
            return;
        }
        if let Some(title) = self
            .updates
            .iter()
            .find(|c| c.id == id)
            .map(|c| c.title.clone())
        {
            self.interests_feedback(&title, grokhub_core::interests::InterestSignal::Opened);
        }
        let (action, board_card) = self
            .updates
            .iter()
            .find(|c| c.id == id)
            .map(|c| (c.action.clone(), c.board_id.clone()))
            .unwrap_or_default();
        self.persist_updates();
        if let Some(card) = self.updates.iter().find(|c| c.id == id).cloned() {
            let did = match &action {
                Some(UpdateAction::OpenWorkboard) => grokhub_core::UseAction::OpenedBoard,
                Some(UpdateAction::OpenAutomations) => grokhub_core::UseAction::OpenedAutomations,
                Some(UpdateAction::DeepLink { .. }) => grokhub_core::UseAction::OpenedLink,
                Some(UpdateAction::OpenSession { .. }) | None => {
                    grokhub_core::UseAction::OpenedChat
                }
            };
            self.log_card_use(&card, grokhub_core::UseDepth::Opened, did);
        }
        self.follow_update_action(action);
        // A run that filed a Follow up card opens that card on the board.
        if let Some(card) = board_card.filter(|b| self.board.iter().any(|c| &c.id == b)) {
            self.nav = Nav::Workboard;
            self.board_view.open = Some(card);
        }
    }

    /// Opened, built, discussed, liked, or used before: an X then is not a "no".
    fn card_touched(&self, card: &UpdateCard) -> bool {
        card.status == UpdateStatus::Opened
            || card.built
            || card.discuss_thread.is_some()
            || card.reaction == Some(grokhub_core::CardReaction::Up)
            || grokhub_core::used_depth(&self.card_prefs, card) > 0
    }

    fn log_card_closed(&mut self, card: &UpdateCard, touched: bool) {
        if touched {
            self.log_card_signal(card, grokhub_core::CardEvent::Dismissed, Some(true));
        } else {
            // Never opened or used: a strong "less like this" for its kind and topic.
            self.log_card_signal(card, grokhub_core::CardEvent::Rejected, None);
        }
    }

    /// Hide a feed card from Home (`off_home`). It stays in Pulse. An idea is
    /// dismissed from the feed and stays on the Ideas board.
    #[cfg(test)]
    pub(super) fn close_home_card(&mut self, id: &str) {
        let Some(card) = self.updates.iter().find(|c| c.id == id).cloned() else {
            return;
        };
        if card.kind == UpdateKind::Idea {
            self.dismiss_feed_card(id);
            return;
        }
        let touched = self.card_touched(&card);
        if grokhub_core::pulse::hide_from_home(&mut self.updates, id) {
            self.persist_updates();
            self.log_card_closed(&card, touched);
        }
    }

    pub(super) fn dismiss_feed_card(&mut self, id: &str) {
        let prior = self.updates.iter().find(|c| c.id == id).cloned();
        let kind = prior.as_ref().map(|c| c.kind);
        let touched = prior.as_ref().is_some_and(|c| self.card_touched(c));
        let (source, title) = prior
            .as_ref()
            .map(|c| (c.source_id.clone(), c.title.clone()))
            .unwrap_or_default();
        let removed = if kind == Some(UpdateKind::Idea) {
            unpin_feed_idea(&mut self.updates, id)
        } else {
            dismiss_update_at(&mut self.updates, id, now_ms())
        };
        if removed {
            if kind == Some(UpdateKind::Suggestion) {
                remember_dismissed_source(&mut self.cfg.feed_pulse, &source);
                self.persist_cfg();
            }
            // The idea stays on the Ideas board. New ideas on its topic stay off.
            if kind == Some(UpdateKind::Idea) {
                grokhub_core::remember_turned_down(&mut self.cfg.feed_pulse, &title);
                self.persist_cfg();
            }
            self.persist_updates();
            if let Some(card) = prior {
                self.log_card_closed(&card, touched);
            }
        }
    }

    pub(super) fn follow_update_action(&mut self, action: Option<UpdateAction>) {
        match action {
            Some(UpdateAction::OpenSession { thread_id }) => {
                if let Some(idx) = self.threads.iter().position(|t| t.id == thread_id) {
                    self.apply_switch_thread(idx);
                    self.nav = Nav::Chat;
                }
            }
            Some(UpdateAction::OpenWorkboard) => self.nav = Nav::Workboard,
            Some(UpdateAction::OpenAutomations) => self.nav = Nav::Night,
            Some(UpdateAction::DeepLink { href }) => {
                if self.open_local_model_link(&href) {
                    return;
                }
                let href = href.trim();
                if !href.is_empty() {
                    self.status = href.to_string();
                }
                if crate::desktop::url_safe_to_open(href) {
                    let _ = crate::desktop::open_url(href);
                }
            }
            None => {}
        }
    }
}

// --- Pulse / Feed paint (ui kit) ---

/// Ideas row chrome on Pulse → Ideas, in the Loadout look: a raised card
/// with a dim red rim, lifted with a signal-red rim and an amber tab while
/// hovered or focused (deck slide 15).
pub(super) fn kit_pulse_ideas_row(
    ui: &mut eframe::egui::Ui,
    active: bool,
    contents: impl FnOnce(&mut eframe::egui::Ui),
) -> eframe::egui::Response {
    let variant = if active {
        CardVariant::SignalHot
    } else {
        CardVariant::Signal
    };
    let resp = card_frame(ui, variant, true, contents);
    if active {
        let r = resp.rect;
        let tab = egui::Rect::from_min_size(
            egui::pos2(r.left() + 1.0, r.top() + 14.0),
            egui::vec2(3.0, 24.0_f32.min(r.height() - 16.0).max(0.0)),
        );
        ui.painter().rect_filled(tab, 1.5, crate::theme::amber());
    }
    resp
}

/// One Feed post on Pulse (includes D3 daily-drop rows).
pub(super) fn kit_pulse_feed_post(
    ui: &mut eframe::egui::Ui,
    contents: impl FnOnce(&mut eframe::egui::Ui),
) -> eframe::egui::Response {
    card_frame(ui, CardVariant::Signal, true, contents)
}

pub(super) fn kit_daily_drop_badge(ui: &mut eframe::egui::Ui) {
    badge(ui, "Daily drop", BadgeTone::Accent);
}

/// Feed tab empty line with a Feed instructions link. Returns true when the link is clicked.
pub(super) fn paint_feed_empty_prompt(ui: &mut eframe::egui::Ui) -> bool {
    use grokhub_core::pulse::{FEED_EMPTY_LEAD, FEED_EMPTY_LINK};
    let mut open = false;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        ui.label(
            RichText::new(FEED_EMPTY_LEAD)
                .size(crate::theme::FONT_BODY)
                .color(crate::theme::muted()),
        );
        let link = ui.add(
            egui::Label::new(
                RichText::new(FEED_EMPTY_LINK)
                    .size(crate::theme::FONT_BODY)
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
            open = true;
        }
        ui.label(
            RichText::new(".")
                .size(crate::theme::FONT_BODY)
                .color(crate::theme::muted()),
        );
    });
    open
}

/// For You tab empty body. Returns true when Suggest for you is pressed.
pub(super) fn paint_ideas_empty_prompt(ui: &mut eframe::egui::Ui) -> bool {
    use grokhub_core::pulse::{FOR_YOU_LABEL, IDEAS_EMPTY};
    empty_state(ui, FOR_YOU_LABEL, IDEAS_EMPTY, Some("Suggest for you"))
}

#[cfg(test)]
mod feed_pulse_kit_tests {
    #[test]
    fn feed_ui_wires_ui_kit_for_pulse() {
        let src = include_str!("feed_ui.rs");
        assert!(
            src.contains("kit_pulse_ideas_row")
                && src.contains("kit_pulse_feed_post")
                && src.contains("card_frame")
                && src.contains("kit_daily_drop_badge"),
            "Pulse/Feed paint must use the ui kit"
        );
    }
}

#[cfg(test)]
mod feed_action_tests {
    use super::*;

    /// Keeps every `FeedAct` variant constructed so clippy dead_code stays quiet.
    #[test]
    fn feed_act_variants_still_construct() {
        let acts = [
            FeedAct::Open("missing".into()),
            FeedAct::Dismiss("missing".into()),
            FeedAct::Discuss("missing".into()),
            FeedAct::Like("missing".into()),
            FeedAct::Link("https://example.invalid".into()),
            FeedAct::Undo("missing".into()),
            FeedAct::NeverAgain("missing".into()),
            FeedAct::Retry("missing".into()),
            FeedAct::DeleteRecording("missing".into()),
            FeedAct::RetryNow,
            FeedAct::Source("missing".into(), super::super::pulse_ui::SourceAct::Save),
        ];
        assert_eq!(acts.len(), 11);
    }

    #[test]
    fn a_paused_job_suggestion_opens_that_job() {
        let _g = crate::config::hold_test_config();
        let root = crate::config::test_config_root("paused-job-card");
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::create_dir_all(&root);
        std::env::set_var("GROKHUB_CONFIG", &root);
        let mut cabin = Cabin::quiet_for_test();
        cabin.new_thread(false);
        let job_thread = cabin.threads[cabin.thread_idx].id.clone();
        cabin.new_thread(false);
        let mut job = grokhub_core::BoardCard::new(
            "Fix the tray icon",
            "Paused. This is where to resume.",
            "",
        );
        job.thread_id = Some(job_thread.clone());
        let job_id = job.id.clone();
        cabin.board.push(job);
        let threads_before = cabin.threads.len();
        let card = grokhub_core::suggestion_card(
            &format!("pause:{job_id}"),
            "Paused: Fix the tray icon",
            "Paused 2 hours ago.",
            5,
        );
        let id = card.id.clone();
        cabin.updates.push(card);
        cabin.nav = Nav::Pulse;
        cabin.discuss_card(&id);
        assert_eq!(cabin.nav, Nav::Chat);
        assert_eq!(
            cabin.threads[cabin.thread_idx].id, job_thread,
            "the job's own chat"
        );
        assert_eq!(cabin.threads.len(), threads_before, "no Discuss chat");
        let saved = cabin.updates.iter().find(|c| c.id == id).unwrap();
        assert_eq!(
            (saved.status, saved.discuss_thread.as_deref()),
            (UpdateStatus::Opened, None)
        );
        // A paused job with no chat opens the Workboard instead.
        let lone =
            grokhub_core::BoardCard::new("Sort receipts", "Paused. This is where to resume.", "");
        let lone_id = lone.id.clone();
        cabin.board.push(lone);
        let card = grokhub_core::suggestion_card(
            &format!("pause:{lone_id}"),
            "Paused: Sort receipts",
            "",
            6,
        );
        let id = card.id.clone();
        cabin.updates.push(card);
        cabin.discuss_card(&id);
        assert_eq!(cabin.nav, Nav::Workboard);
        assert_eq!(cabin.threads.len(), threads_before);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn discuss_card_opens_chat_with_the_feed_post() {
        let _g = crate::config::hold_test_config();
        let root = crate::config::test_config_root("discuss-feed-card");
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::create_dir_all(&root);
        std::env::set_var("GROKHUB_CONFIG", &root);
        let mut cabin = Cabin::quiet_for_test();
        let long = "I'll look up two real pieces that fit your Linux work. \
Grok Build changes how a cancel hits a live turn. \
ZEPHYRTAIL is the rest of the dump https://evil.example/nope";
        let mut digest = grokhub_core::digest_card("xstack", "For you", long, 10);
        digest.citations = vec!["https://xstack.grok.me/post".into()];
        digest.why = Some("It changes the cancel button you use.".into());
        let id = digest.id.clone();
        cabin.updates.push(digest);
        cabin.discuss_card(&id);
        assert_eq!(cabin.nav, Nav::Chat);
        assert!(cabin.composer_want_focus);
        let thread = cabin
            .threads
            .iter()
            .find(|t| t.title == "Discuss · For you")
            .expect("digest thread");
        assert!(!thread.background, "feed discuss is the main chat");
        assert!(thread.title_locked);
        assert_eq!(cabin.threads[cabin.thread_idx].id, thread.id);
        let body = thread
            .messages
            .iter()
            .find(|m| m.0 == "assistant")
            .map(|m| m.1.as_str())
            .unwrap_or("");
        assert!(body.contains("Feed post: For you"), "{body}");
        assert!(!body.contains("two real pieces"), "{body}");
        assert!(body.contains("cancel hits a live turn"), "{body}");
        assert!(body.contains("https://xstack.grok.me/post"), "{body}");
        assert!(
            body.contains("It changes the cancel button you use."),
            "{body}"
        );
        assert!(
            body.contains("You opened this from your feed and want to act on it."),
            "{body}"
        );
        assert!(!body.contains("The person opened"), "{body}");
        assert!(!body.contains("ZEPHYRTAIL"), "{body}");
        assert!(!body.contains("evil.example"), "{body}");
        let card = cabin.updates.iter().find(|c| c.id == id).unwrap();
        assert_eq!(card.status, UpdateStatus::Opened);
        assert!(card.discuss_thread.is_some());

        let mut suggestion =
            grokhub_core::suggestion_card("tip", "Morning build", "placeholder", 11);
        suggestion.body = Some(
            "Your crate graph changed overnight. The lint now catches a stale lock. ZEPHYRTAIL"
                .into(),
        );
        suggestion.citations = vec!["https://example.com/morning-build".into()];
        suggestion.why = Some("Your morning build is the one that waits.".into());
        let sid = suggestion.id.clone();
        cabin.updates.push(suggestion);
        cabin.discuss_card(&sid);
        assert_eq!(cabin.nav, Nav::Chat);
        assert!(cabin.composer_want_focus);
        let thread = cabin
            .threads
            .iter()
            .find(|t| t.title == "Discuss · Morning build")
            .expect("suggestion thread");
        assert!(!thread.background);
        assert!(thread.title_locked);
        let body = thread
            .messages
            .iter()
            .find(|m| m.0 == "assistant")
            .map(|m| m.1.as_str())
            .unwrap_or("");
        assert!(body.contains("Morning build"), "{body}");
        assert!(body.contains("crate graph changed overnight"), "{body}");
        assert!(body.contains("https://example.com/morning-build"), "{body}");
        assert!(
            body.contains("Your morning build is the one that waits."),
            "{body}"
        );
        assert!(!body.contains("ZEPHYRTAIL"), "{body}");
        let _ = std::fs::remove_dir_all(&root);
    }
}
