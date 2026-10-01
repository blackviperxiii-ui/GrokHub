//! Home update feed in the slot that held the Coding / Life chip and the
//! under-bar workboard summary. The event slot is hidden when it has nothing
//! to paint. Idea and digest cards use the same store and do not take event rows.
//!
//! On the chat screen the cards sit in a three-deep deck. Hover slides them
//! up to full size, one above the next, on top of the chat box. A card behind
//! the front one can lift out and stays up until the pointer is back on the deck.

use super::*;
use grokhub_core::{
    archive_digest, automation_done_card,
    dismiss_update, feed_ideas, feed_visible, file_idea_todo, hold_if_quiet,
    home_feed_n, idea_open_line, unpin_feed_idea,
    idea_todo_title,
    links_from_research, mark_update_opened, post_update, quiet_hours_active, route_schedule,
    schedule_created_card, tick_feed_pulse, visible_digests, visible_updates,
    CardReaction, CitedLink, DigestMaterial, PulseNow, TasteNote, UpdateAction, UpdateCard,
    UpdateKind, UpdateStatus, DIGEST_PAINT_MAX, FEED_PAINT_MAX,
};

const FEED_CARD_H: f32 = 64.0;
/// Longest wait for the model's idea list before the board gives up on that ask.
pub(super) const IDEAS_WAIT_MS: u64 = 180_000;
const FEED_GAP: f32 = 6.0;
/// Collapsed chat deck shows this many edges. The rest stay in the count.
pub(super) const HOME_STACK_SHOW: usize = 3;
/// Second card, tucked under the front. Same offsets as a slide-up deck at rest.
pub(super) const STACK_REST_DY_1: f32 = 8.0;
pub(super) const STACK_REST_SCALE_1: f32 = 0.95;
/// Third card, and every card still hidden in that slot.
pub(super) const STACK_REST_DY_2: f32 = 16.0;
pub(super) const STACK_REST_SCALE_2: f32 = 0.90;
/// Open step. 110% of the card height leaves a small gap, then the card is full size.
const SLIDE_UP: f32 = 1.10;
/// Extra lift once a card has left the deck.
const STACK_POP: f32 = 14.0;
const SLIDE_SECS: f32 = 0.50;

pub(super) fn stacked_feed_h(n: usize) -> f32 {
    if n == 0 {
        return 0.0;
    }
    let n = n as f32;
    n * FEED_CARD_H + (n - 1.0) * FEED_GAP
}

pub(super) fn slide_stride() -> f32 {
    FEED_CARD_H * SLIDE_UP
}

/// Height of the resting deck. The front card is full size. Two edges stick out below it.
pub(super) fn collapsed_stack_h(n: usize) -> f32 {
    if n == 0 {
        return 0.0;
    }
    let peek = match n {
        1 => 0.0,
        2 => STACK_REST_DY_1,
        _ => STACK_REST_DY_2,
    };
    FEED_CARD_H + peek
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct SlidePose {
    /// From the front card's top. Negative is up.
    pub dy: f32,
    pub scale: f32,
}

pub(super) fn rest_slide(index: usize) -> SlidePose {
    match index {
        0 => SlidePose { dy: 0.0, scale: 1.0 },
        1 => SlidePose {
            dy: STACK_REST_DY_1,
            scale: STACK_REST_SCALE_1,
        },
        _ => SlidePose {
            dy: STACK_REST_DY_2,
            scale: STACK_REST_SCALE_2,
        },
    }
}

/// Open pose. Card 0 stays. Each card behind it sits one stride higher, at full size.
pub(super) fn open_slide(index: usize) -> SlidePose {
    SlidePose {
        dy: -(index as f32) * slide_stride(),
        scale: 1.0,
    }
}

pub(super) fn mix_slide(rest: SlidePose, open: SlidePose, t: f32) -> SlidePose {
    let t = t.clamp(0.0, 1.0);
    SlidePose {
        dy: rest.dy + (open.dy - rest.dy) * t,
        scale: rest.scale + (open.scale - rest.scale) * t,
    }
}

/// How far to push the open deck down so the top card stays on screen.
pub(super) fn slide_up_shift(front_y: f32, n: usize, screen_top: f32) -> f32 {
    if n <= 1 {
        return 0.0;
    }
    let top = front_y + open_slide(n - 1).dy;
    let min_top = screen_top + 8.0;
    (min_top - top).max(0.0)
}

pub(super) fn slide_rect(front: egui::Pos2, width: f32, pose: SlidePose) -> egui::Rect {
    let w = width * pose.scale;
    let h = FEED_CARD_H * pose.scale;
    let x = front.x + (width - w) * 0.5;
    let y = front.y + pose.dy + (FEED_CARD_H - h) * 0.5;
    egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(w, h))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum StackHit {
    /// Pointer is on the lifted card.
    Card,
    /// Pointer is on the pile, not on the lifted card.
    Pile,
    /// Pointer is elsewhere.
    Away,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct StackHover {
    pub on_card: bool,
    pub on_pile: bool,
}

pub(super) fn stack_hit(hover: StackHover) -> StackHit {
    if hover.on_card {
        StackHit::Card
    } else if hover.on_pile {
        StackHit::Pile
    } else {
        StackHit::Away
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct StackView {
    pub expanded: bool,
    pub popped: Option<String>,
}

/// Cards behind the front one lift. The front card is already full size, so it stays put.
pub(super) fn pile_pop_target(front_id: Option<&str>, hovered: Option<String>) -> Option<String> {
    match hovered.as_deref() {
        Some(id) if Some(id) != front_id => hovered,
        _ => None,
    }
}

/// Hover opens the pile. A lifted card stays up until the pointer is back on the pile.
pub(super) fn next_feed_stack(prev: &StackView, hit: StackHit, hovered: Option<String>) -> StackView {
    match hit {
        StackHit::Card => StackView {
            expanded: prev.expanded,
            popped: prev.popped.clone().or(hovered),
        },
        StackHit::Pile => StackView {
            expanded: true,
            popped: hovered,
        },
        StackHit::Away => StackView {
            expanded: false,
            popped: prev.popped.clone(),
        },
    }
}

pub(super) fn update_feed_h(n: usize) -> f32 {
    stacked_feed_h(n.min(FEED_PAINT_MAX))
}

pub(super) fn home_feed_count(cards: &[UpdateCard]) -> usize {
    home_feed_n(cards)
}

pub(super) enum FeedAct {
    Open(String),
    Dismiss(String),
    Build(String),
    Offer(String),
    React(String, CardReaction),
    Discuss(String),
    Archive(String),
    /// Remove an idea from the Ideas board.
    Drop(String),
}

impl Cabin {
    /// `poll_grok_loop` and a finished night recipe replay call this.
    /// Quiet hours still record the card. Paint waits for the release pass.
    pub(super) fn note_automation_done(&mut self, source_id: &str, title: &str, summary: &str) {
        if title.trim().is_empty() && summary.trim().is_empty() {
            return;
        }
        let mut card = automation_done_card(source_id, title, summary, now_ms());
        hold_if_quiet(&mut card, self.quiet_now());
        self.post_feed_card(card);
    }

    /// `commit_schedule` calls this after a clock job or interval loop is saved.
    pub(super) fn note_schedule_created(&mut self, source_id: &str, title: &str, when_label: &str) {
        if title.trim().is_empty() && when_label.trim().is_empty() {
            return;
        }
        let card = schedule_created_card(source_id, title, when_label, now_ms());
        self.post_feed_card(card);
    }

    pub(super) fn post_feed_card(&mut self, card: UpdateCard) {
        post_update(&mut self.updates, card);
        self.persist_updates();
    }

    pub(super) fn quiet_now(&self) -> bool {
        let clock = Self::local_clock();
        quiet_hours_active(&clock.hm(), &self.cfg.quiet_start, &self.cfg.quiet_end)
    }

    /// Housekeep: idea expiry, quiet release, digest clock. Not the nightly review.
    pub(super) fn tick_feed_pulse(&mut self) {
        self.maybe_suggest_ideas(false);
        let now = now_ms();
        let quiet = self.quiet_now();
        let pulse = self.cfg.feed_pulse.clone();
        let want_digest = pulse.digest_on
            && (pulse.digest_held
                || pulse.last_digest_ms == 0
                || now.saturating_sub(pulse.last_digest_ms) >= pulse.digest_ms);
        let (user_md, memory_md, soul_md, taste, links) = if want_digest && !quiet {
            (
                crate::config::read_memory("USER.md"),
                crate::config::read_memory("MEMORY.md"),
                crate::config::read_memory("SOUL.md"),
                self.digest_taste(),
                Vec::<CitedLink>::new(),
            )
        } else {
            (
                String::new(),
                String::new(),
                String::new(),
                Vec::new(),
                links_from_research(""),
            )
        };
        let material = DigestMaterial {
            brief: &self.cfg.digest_brief,
            user_md: &user_md,
            memory_md: &memory_md,
            soul_md: &soul_md,
            links: &links,
            taste: &taste,
        };
        let tick = tick_feed_pulse(
            &mut self.updates,
            &mut self.cfg.feed_pulse,
            PulseNow { now_ms: now, quiet },
            material,
        );
        if tick.cards_changed {
            self.persist_updates();
        }
        if tick.pulse_changed {
            self.persist_cfg();
        }
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

    pub(super) fn paint_update_feed(&mut self, ui: &mut egui::Ui, pane_w: f32) {
        self.ensure_useful_ideas();
        let mem_id = egui::Id::new("home-feed-stack");
        if !feed_visible(&self.updates) {
            ui.ctx().data_mut(|d| {
                d.insert_temp(mem_id, FeedStackMem::default());
                d.insert_temp(egui::Id::new("home-deck-defer"), None::<DeckDefer>);
            });
            return;
        }
        let cards = home_stack_cards(&self.updates, now_ms());
        if cards.is_empty() {
            ui.ctx().data_mut(|d| {
                d.insert_temp(mem_id, FeedStackMem::default());
                d.insert_temp(egui::Id::new("home-deck-defer"), None::<DeckDefer>);
            });
            return;
        }
        let (stack, _) = ui.allocate_exact_size(
            egui::vec2(pane_w, collapsed_stack_h(cards.len())),
            egui::Sense::hover(),
        );
        let prev: FeedStackMem = ui.ctx().data(|d| d.get_temp(mem_id)).unwrap_or_default();
        let pointer = ui.ctx().input(|i| i.pointer.hover_pos());
        let on_card = prev
            .popped_rect
            .is_some_and(|rect| pointer.is_some_and(|p| rect.contains(p)));
        let on_pile = pointer.is_some_and(|p| {
            stack.contains(p) || prev.hits.iter().any(|hit| hit.rect.contains(p))
        });
        let hit = stack_hit(StackHover { on_card, on_pile });
        let hovered = hovered_slide_card(pointer, &prev.hits);
        let front_id = cards.first().map(|card| card.id.as_str());
        let hovered_for = if hit == StackHit::Pile {
            pile_pop_target(front_id, hovered)
        } else {
            hovered
        };
        let view = drop_missing_pop(&cards, next_feed_stack(&prev.view(), hit, hovered_for));
        // Paint later, after the home composer, so the open deck covers that
        // box and stays inside the chat pane.
        ui.ctx().data_mut(|d| {
            d.insert_temp(
                egui::Id::new("home-deck-defer"),
                Some(DeckDefer {
                    stack,
                    width: pane_w,
                    view,
                }),
            );
        });
    }

    /// Open deck for the empty home chat. Call after the composer in that pane.
    pub(super) fn paint_home_deck_over_chat(&mut self, ui: &mut egui::Ui) {
        let deferred = ui
            .ctx()
            .data_mut(|d| d.remove_temp::<Option<DeckDefer>>(egui::Id::new("home-deck-defer")))
            .flatten();
        let Some(deferred) = deferred else {
            return;
        };
        let cards = home_stack_cards(&self.updates, now_ms());
        if cards.is_empty() {
            return;
        }
        let painted = paint_slide_deck(ui, &cards, deferred.stack, deferred.width, &deferred.view);
        ui.ctx().data_mut(|d| {
            d.insert_temp(
                egui::Id::new("home-feed-stack"),
                FeedStackMem {
                    expanded: deferred.view.expanded,
                    popped: deferred.view.popped.clone(),
                    hits: painted.hits,
                    popped_rect: deferred.view.popped.as_ref().and(painted.popped_rect),
                },
            );
        });
        self.apply_feed_act(painted.act);
    }

    pub(super) fn apply_feed_act(&mut self, act: Option<FeedAct>) {
        match act {
            Some(FeedAct::Dismiss(id)) => self.dismiss_feed_card(&id),
            Some(FeedAct::Build(id)) => self.build_idea(&id),
            Some(FeedAct::Offer(id)) => self.accept_automate_offer(&id),
            Some(FeedAct::Open(id)) => self.open_feed_card(&id),
            Some(FeedAct::React(id, reaction)) => self.react_card(&id, reaction),
            Some(FeedAct::Discuss(id)) => self.discuss_card(&id),
            Some(FeedAct::Archive(id)) => self.archive_feed_digest(&id),
            Some(FeedAct::Drop(id)) => self.delete_idea(&id),
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
        self.persist_updates();
    }

    pub(super) fn accept_automate_offer(&mut self, id: &str) {
        if !mark_update_opened(&mut self.updates, id) {
            return;
        }
        let seed = self
            .updates
            .iter()
            .find(|c| c.id == id)
            .map(|c| c.title.clone())
            .unwrap_or_default();
        self.persist_updates();
        if let Some(route) = route_schedule(&seed) {
            let _ = self.commit_schedule(route);
        }
    }

    pub(super) fn react_card(&mut self, id: &str, reaction: CardReaction) {
        let Some(card) = self.updates.iter_mut().find(|c| c.id == id) else {
            return;
        };
        if !matches!(card.kind, UpdateKind::Idea | UpdateKind::Digest) {
            return;
        }
        card.reaction = Some(reaction);
        self.persist_updates();
    }

    /// Once per launch: purge template idea cards. The automatic model ask comes
    /// from the heartbeat (`tick_feed_pulse` → `maybe_suggest_ideas`).
    pub(super) fn ensure_useful_ideas(&mut self) {
        if self.ideas_filled {
            return;
        }
        self.ideas_filled = true;
        if grokhub_core::purge_template_ideas(&mut self.updates) > 0 {
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
        if self.ideas_rx.is_some() || !self.llm_ready() {
            return;
        }
        let now = now_ms();
        if !force {
            let live = grokhub_core::live_generated_ideas(&self.updates);
            if !grokhub_core::ideas_refresh_due(self.cfg.feed_pulse.last_ideas_ms, now, live)
                || self.quiet_now()
                || self.budget_holds_scheduled()
            {
                return;
            }
        }
        let (prompt, sources, taken) = self.idea_request();
        self.cfg.feed_pulse.last_ideas_ms = now;
        self.persist_cfg();
        let key = self.bearer();
        let (tx, rx) = mpsc::channel();
        self.ideas_rx = Some((rx, sources, taken));
        std::thread::spawn(move || {
            let _ = tx.send(cabin_fast_llm(key, prompt));
        });
    }

    /// The prompt plus what the reply must not copy (their own lines) or repeat.
    pub(super) fn idea_request(&self) -> (String, Vec<String>, Vec<String>) {
        let user_md = crate::config::read_memory("USER.md");
        let memory_md = crate::config::read_memory("MEMORY.md");
        let asks = self.recent_asks(15);
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
        let rejected: Vec<String> = self
            .cfg
            .feed_pulse
            .idea_titles
            .iter()
            .filter(|t| !existing.iter().any(|e| e.eq_ignore_ascii_case(t)))
            .cloned()
            .collect();
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
        let prompt = grokhub_core::idea_prompt(&grokhub_core::IdeaContext {
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
        });
        let mut sources = asks;
        sources.extend(
            user_md
                .lines()
                .chain(memory_md.lines())
                .map(str::trim)
                .filter(|l| l.len() >= 12)
                .map(str::to_string),
        );
        let mut taken = existing;
        taken.extend(rejected);
        taken.extend(automations);
        (prompt, sources, taken)
    }

    /// What they asked lately, newest first: real asks only (no slash commands,
    /// tool results, or "no, still…" feedback), each once.
    pub(super) fn recent_asks(&self, n: usize) -> Vec<String> {
        let mut threads: Vec<&crate::threads::ChatThread> =
            self.threads.iter().filter(|t| !t.background).collect();
        threads.sort_by_key(|t| std::cmp::Reverse(t.accessed_ms));
        let open = self.messages.iter().rev();
        let rest = threads
            .into_iter()
            .flat_map(|t| t.messages.iter().rev());
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
        let Some((rx, sources, taken)) = self.ideas_rx.take() else {
            return;
        };
        match rx.try_recv() {
            Ok(raw) => {
                let seeds = grokhub_core::parse_ideas(&raw, &sources, &taken);
                let names: Vec<&str> = taken.iter().map(String::as_str).collect();
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
                if self.nav == Nav::Ideas {
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
                    if self.nav == Nav::Ideas {
                        self.status = "Ideas took too long. Try Suggest ideas again.".into();
                    }
                } else {
                    self.ideas_rx = Some((rx, sources, taken));
                }
            }
            Err(mpsc::TryRecvError::Disconnected) => {}
        }
    }

    pub(super) fn job_is_idea_talk(&self) -> bool {
        let Some(id) = self.chat_job_thread.as_deref() else {
            return false;
        };
        self.threads.iter().any(|t| {
            t.id == id && t.background && t.title.starts_with("Discuss · ")
        })
    }

    pub(super) fn discuss_card(&mut self, id: &str) {
        let Some(card) = self.updates.iter().find(|c| c.id == id).cloned() else {
            return;
        };
        if card.kind == UpdateKind::Idea {
            self.open_idea_on_board(&card.id);
            return;
        }
        if !matches!(card.kind, UpdateKind::Digest) {
            return;
        }
        if let Some(thread_id) = card.discuss_thread.as_deref() {
            if let Some(idx) = self.threads.iter().position(|t| t.id == thread_id) {
                self.switch_thread(idx);
                self.nav = Nav::Chat;
                self.composer_want_focus = true;
                return;
            }
        }
        let context = idea_open_line(&card);
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
        self.persist_updates();
        self.persist();
    }

    pub(super) fn archive_feed_digest(&mut self, id: &str) {
        if archive_digest(&mut self.updates, id) {
            self.persist_updates();
        }
    }

    pub(super) fn open_feed_card(&mut self, id: &str) {
        let kind = self.updates.iter().find(|c| c.id == id).map(|c| c.kind);
        if matches!(kind, Some(UpdateKind::Idea)) {
            self.discuss_card(id);
            return;
        }
        if !mark_update_opened(&mut self.updates, id) {
            return;
        }
        let action = self
            .updates
            .iter()
            .find(|c| c.id == id)
            .and_then(|c| c.action.clone());
        self.persist_updates();
        self.follow_update_action(action);
    }

    pub(super) fn dismiss_feed_card(&mut self, id: &str) {
        let idea = self
            .updates
            .iter()
            .any(|c| c.id == id && c.kind == UpdateKind::Idea);
        let removed = if idea {
            unpin_feed_idea(&mut self.updates, id)
        } else {
            dismiss_update(&mut self.updates, id)
        };
        if removed {
            self.persist_updates();
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
                let href = href.trim();
                if !href.is_empty() {
                    self.status = href.to_string();
                }
            }
            None => {}
        }
    }
}

#[derive(Clone, Debug)]
struct DeckDefer {
    stack: egui::Rect,
    width: f32,
    view: StackView,
}

#[derive(Clone, Debug)]
struct SlideHit {
    id: String,
    index: usize,
    rect: egui::Rect,
}

#[derive(Clone, Debug, Default)]
struct FeedStackMem {
    expanded: bool,
    popped: Option<String>,
    hits: Vec<SlideHit>,
    popped_rect: Option<egui::Rect>,
}

impl FeedStackMem {
    fn view(&self) -> StackView {
        StackView {
            expanded: self.expanded,
            popped: self.popped.clone(),
        }
    }
}

struct SlidePaint {
    act: Option<FeedAct>,
    hits: Vec<SlideHit>,
    popped_rect: Option<egui::Rect>,
}

fn home_stack_cards(cards: &[UpdateCard], now: u64) -> Vec<UpdateCard> {
    let mut out: Vec<UpdateCard> = visible_updates(cards)
        .into_iter()
        .take(FEED_PAINT_MAX)
        .collect();
    out.extend(feed_ideas(cards, now));
    out.extend(visible_digests(cards).into_iter().take(DIGEST_PAINT_MAX));
    out
}

fn drop_missing_pop(cards: &[UpdateCard], mut view: StackView) -> StackView {
    if view
        .popped
        .as_ref()
        .is_some_and(|id| cards.iter().all(|card| &card.id != id))
    {
        view.popped = None;
    }
    view
}

fn hovered_slide_card(pointer: Option<egui::Pos2>, hits: &[SlideHit]) -> Option<String> {
    let pointer = pointer?;
    hits.iter()
        .rev()
        .find(|hit| hit.rect.contains(pointer))
        .map(|hit| hit.id.clone())
}

fn slide_spread(ctx: &egui::Context, index: usize, open: bool) -> f32 {
    ctx.animate_bool_with_time_and_easing(
        egui::Id::new(("home-feed-slide", index)),
        open,
        SLIDE_SECS,
        egui::emath::easing::quadratic_out,
    )
}

fn slide_lift(ctx: &egui::Context, index: usize, popped: bool) -> f32 {
    ctx.animate_bool_with_time_and_easing(
        egui::Id::new(("home-feed-lift", index)),
        popped,
        0.28,
        egui::emath::easing::quadratic_out,
    )
}

fn paint_count_badge(painter: &egui::Painter, rect: egui::Rect, n: usize) {
    painter.rect_filled(rect, 9.0, crate::theme::fg());
    painter.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        n.to_string(),
        egui::FontId::proportional(11.0),
        crate::theme::bg(),
    );
}

fn paint_stack_shadow(painter: &egui::Painter, rect: egui::Rect) {
    let shadow = rect.translate(egui::vec2(0.0, 6.0)).expand(2.0);
    painter.rect_filled(
        shadow,
        crate::theme::CARD_RADIUS,
        egui::Color32::from_black_alpha(64),
    );
}

fn paint_card_at(ui: &mut egui::Ui, card: &UpdateCard, rect: egui::Rect) -> Option<FeedAct> {
    let mut act = None;
    ui.allocate_new_ui(egui::UiBuilder::new().max_rect(rect), |ui| {
        ui.set_min_size(rect.size());
        ui.set_width(rect.width());
        ui.spacing_mut().item_spacing = egui::vec2(8.0, 4.0);
        act = paint_feed_card(ui, card, rect.width(), false);
    });
    act
}

fn tucked_slide(index: usize, spread: f32) -> bool {
    index > 0 && spread < 0.42
}

struct SlidePlacement {
    pose: SlidePose,
    spread: f32,
    popped: bool,
}

fn slide_placements(
    ctx: &egui::Context,
    cards: &[UpdateCard],
    view: &StackView,
    front_y: f32,
    screen_top: f32,
) -> Vec<SlidePlacement> {
    let shift = slide_up_shift(front_y, cards.len(), screen_top);
    cards
        .iter()
        .enumerate()
        .map(|(index, card)| {
            let popped = view.popped.as_deref() == Some(card.id.as_str());
            let spread = slide_spread(ctx, index, view.expanded || popped);
            let lift = slide_lift(ctx, index, popped);
            let mut pose = mix_slide(rest_slide(index), open_slide(index), spread);
            pose.dy += shift * spread;
            pose.dy -= STACK_POP * lift;
            SlidePlacement {
                pose,
                spread,
                popped,
            }
        })
        .collect()
}

fn paint_tucked_edge(ui: &mut egui::Ui, rect: egui::Rect, cover: egui::Rect) {
    let visible = rect.intersect(egui::Rect::from_min_max(
        egui::pos2(rect.left(), cover.bottom() - 1.0),
        rect.right_bottom(),
    ));
    if visible.height() < 1.0 {
        return;
    }
    let painter = ui.painter().with_clip_rect(visible);
    painter.rect(
        rect,
        crate::theme::CARD_RADIUS,
        crate::theme::elevated(),
        egui::Stroke::new(1.0_f32, crate::theme::border()),
    );
}

fn paint_slide_deck(
    ui: &mut egui::Ui,
    cards: &[UpdateCard],
    stack: egui::Rect,
    width: f32,
    view: &StackView,
) -> SlidePaint {
    let front = stack.left_top();
    let placements = slide_placements(ui.ctx(), cards, view, front.y, ui.ctx().screen_rect().top());
    let rects: Vec<egui::Rect> = placements
        .iter()
        .map(|place| slide_rect(front, width, place.pose))
        .collect();
    let mut act = None;
    let mut hits = Vec::new();
    let mut popped_rect = None;
    let mut paint_one = |ui: &mut egui::Ui| {
        let mut order: Vec<usize> = (0..cards.len()).collect();
        order.sort_by_key(|&index| std::cmp::Reverse(index));
        if let Some(popped) = view.popped.as_deref() {
            if let Some(pos) = order.iter().position(|&index| cards[index].id == popped) {
                let index = order.remove(pos);
                order.push(index);
            }
        }
        for index in order {
            let place = &placements[index];
            let rect = rects[index];
            if place.popped || place.spread > 0.2 {
                paint_stack_shadow(ui.painter(), rect);
            }
            let card_act = if tucked_slide(index, place.spread) {
                paint_tucked_edge(ui, rect, rects[0]);
                None
            } else {
                paint_card_at(ui, &cards[index], rect)
            };
            if card_act.is_some() {
                act = card_act;
            }
            let mut bridge = rect;
            if place.popped {
                bridge.set_bottom(bridge.bottom() + STACK_POP);
                popped_rect = Some(bridge);
            }
            hits.push(SlideHit {
                id: cards[index].id.clone(),
                index,
                rect: bridge,
            });
        }
        if cards.len() > 1 {
            let badge = egui::Rect::from_min_size(
                egui::pos2(rects[0].right() - 36.0, rects[0].bottom() - 8.0),
                egui::vec2(28.0, 16.0),
            );
            paint_count_badge(ui.painter(), badge, cards.len());
        }
    };
    paint_one(ui);
    SlidePaint {
        act,
        hits,
        popped_rect,
    }
}

fn paint_feed_card(
    ui: &mut egui::Ui,
    card: &UpdateCard,
    pane_w: f32,
    full: bool,
) -> Option<FeedAct> {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(pane_w, FEED_CARD_H), egui::Sense::hover());
    ui.painter().rect(
        rect,
        crate::theme::CARD_RADIUS,
        crate::theme::elevated(),
        egui::Stroke::new(1.0_f32, crate::theme::border()),
    );
    let inner = rect.shrink(8.0);
    let mut act = None;
    ui.allocate_new_ui(egui::UiBuilder::new().max_rect(inner), |ui| {
        ui.horizontal(|ui| {
            let text_w = (inner.width() - 220.0).max(80.0);
            let title_color = if card.status == UpdateStatus::Opened || card.built {
                crate::theme::muted()
            } else {
                crate::theme::fg()
            };
            let (text_rect, text_resp) =
                ui.allocate_exact_size(egui::vec2(text_w, inner.height()), egui::Sense::click());
            ui.allocate_new_ui(egui::UiBuilder::new().max_rect(text_rect), |ui| {
                ui.set_width(text_w);
                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
                let title = if card.kind == UpdateKind::Idea {
                    format!("{} · {}", card.idea_type_label(), card.title)
                } else if card.built {
                    format!("{} · {} · On the board", card.kind.label(), card.title)
                } else {
                    format!("{} · {}", card.kind.label(), card.title)
                };
                ui.label(
                    RichText::new(title)
                        .size(crate::theme::FONT_BODY)
                        .color(title_color),
                );
                if let Some(body) = card.body.as_deref() {
                    ui.label(
                        RichText::new(body)
                            .size(crate::theme::FONT_TIP)
                            .color(crate::theme::muted()),
                    );
                }
            });
            if text_resp.clicked() {
                act = Some(if card.kind == UpdateKind::Idea {
                    FeedAct::Discuss(card.id.clone())
                } else {
                    FeedAct::Open(card.id.clone())
                });
            }
            if text_resp.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            ui.with_layout(
                egui::Layout::right_to_left(egui::Align::Center),
                |ui| match card.kind {
                    UpdateKind::Idea => {
                        if !full && crate::cards::ghost_pill(ui, "Dismiss") {
                            act = Some(FeedAct::Dismiss(card.id.clone()));
                        }
                        if crate::cards::ghost_pill(ui, "Open") {
                            act = Some(FeedAct::Discuss(card.id.clone()));
                        }
                    }
                    UpdateKind::AutomateOffer => {
                        if crate::cards::ghost_pill(ui, "Dismiss") {
                            act = Some(FeedAct::Dismiss(card.id.clone()));
                        }
                        if crate::cards::ghost_pill(ui, "Accept") {
                            act = Some(FeedAct::Offer(card.id.clone()));
                        }
                    }
                    UpdateKind::Suggestion => {
                        if crate::cards::ghost_pill(ui, "Dismiss") {
                            act = Some(FeedAct::Dismiss(card.id.clone()));
                        }
                        if crate::cards::ghost_pill(ui, "Accept") {
                            act = Some(FeedAct::Open(card.id.clone()));
                        }
                    }
                    UpdateKind::Digest => {
                        if card.status == UpdateStatus::Dismissed {
                            ui.label(
                                RichText::new("Archived")
                                    .size(crate::theme::FONT_TIP)
                                    .color(crate::theme::muted()),
                            );
                        } else if crate::cards::ghost_pill(ui, "Delete") {
                            act = Some(FeedAct::Archive(card.id.clone()));
                        }
                        if crate::cards::ghost_pill(ui, "Discuss") {
                            act = Some(FeedAct::Discuss(card.id.clone()));
                        }
                        if crate::cards::ghost_pill(ui, "Up") {
                            act = Some(FeedAct::React(card.id.clone(), CardReaction::Up));
                        }
                    }
                    UpdateKind::AutomationDone | UpdateKind::ScheduleCreated => {
                        if crate::cards::ghost_pill(ui, "Dismiss") {
                            act = Some(FeedAct::Dismiss(card.id.clone()));
                        }
                    }
                },
            );
        });
    });
    act
}

#[cfg(test)]
mod stack_tests {
    use super::{
        collapsed_stack_h, mix_slide, next_feed_stack, open_slide, rest_slide, slide_up_shift,
        stack_hit, stacked_feed_h, StackHit, StackHover, StackView, FEED_CARD_H, HOME_STACK_SHOW,
        STACK_REST_DY_1, STACK_REST_DY_2, STACK_REST_SCALE_1, STACK_REST_SCALE_2,
    };

    fn view(expanded: bool, popped: Option<&str>) -> StackView {
        StackView {
            expanded,
            popped: popped.map(str::to_string),
        }
    }

    #[test]
    fn collapsed_pile_stays_three_cards_tall() {
        assert_eq!(HOME_STACK_SHOW, 3);
        assert_eq!(collapsed_stack_h(0), 0.0);
        assert_eq!(collapsed_stack_h(1), FEED_CARD_H);
        assert_eq!(collapsed_stack_h(2), FEED_CARD_H + STACK_REST_DY_1);
        assert_eq!(collapsed_stack_h(3), FEED_CARD_H + STACK_REST_DY_2);
        assert_eq!(collapsed_stack_h(9), collapsed_stack_h(3));
        assert!(collapsed_stack_h(9) < stacked_feed_h(4));
    }

    #[test]
    fn slide_up_opens_full_cards_above_the_front() {
        let front = open_slide(0);
        let second = open_slide(1);
        let third = open_slide(2);
        assert_eq!(front.dy, 0.0);
        assert_eq!(front.scale, 1.0);
        assert!(second.dy < front.dy);
        assert!(third.dy < second.dy);
        assert_eq!(second.scale, 1.0);
        assert_eq!(third.scale, 1.0);
        assert!((second.dy - third.dy) > FEED_CARD_H);
        let rest = rest_slide(1);
        assert_eq!(rest.dy, STACK_REST_DY_1);
        assert_eq!(rest.scale, STACK_REST_SCALE_1);
        assert_eq!(rest_slide(4).dy, STACK_REST_DY_2);
        assert_eq!(rest_slide(4).scale, STACK_REST_SCALE_2);
        let mid = mix_slide(rest_slide(1), open_slide(1), 0.0);
        assert_eq!(mid.dy, STACK_REST_DY_1);
        let opened = mix_slide(rest_slide(1), open_slide(1), 1.0);
        assert_eq!(opened.dy, open_slide(1).dy);
        assert!(open_slide(8).dy < open_slide(1).dy);
    }

    #[test]
    fn slide_up_stays_on_screen() {
        assert_eq!(slide_up_shift(300.0, 1, 0.0), 0.0);
        assert_eq!(slide_up_shift(300.0, 2, 0.0), 0.0);
        let tall = slide_up_shift(40.0, 8, 0.0);
        assert!(tall > 0.0);
        let top = 40.0 + open_slide(7).dy + tall;
        assert!((top - 8.0).abs() < 0.5);
    }

    #[test]
    fn lifted_card_stays_until_the_pointer_returns_to_the_pile() {
        let prev = view(true, Some("idea"));
        let on_card = next_feed_stack(
            &prev,
            stack_hit(StackHover {
                on_card: true,
                on_pile: false,
            }),
            None,
        );
        assert!(on_card.expanded);
        assert_eq!(on_card.popped.as_deref(), Some("idea"));

        let away = next_feed_stack(
            &on_card,
            stack_hit(StackHover {
                on_card: false,
                on_pile: false,
            }),
            None,
        );
        assert!(!away.expanded);
        assert_eq!(away.popped.as_deref(), Some("idea"));

        let back = next_feed_stack(
            &away,
            stack_hit(StackHover {
                on_card: false,
                on_pile: true,
            }),
            Some("other".into()),
        );
        assert!(back.expanded);
        assert_eq!(back.popped.as_deref(), Some("other"));
        assert_eq!(
            stack_hit(StackHover {
                on_card: false,
                on_pile: false,
            }),
            StackHit::Away
        );
    }

    #[test]
    fn front_card_does_not_lift_out_of_the_open_pile() {
        use super::pile_pop_target;
        assert_eq!(pile_pop_target(Some("front"), Some("front".into())), None);
        assert_eq!(
            pile_pop_target(Some("front"), Some("peek".into())).as_deref(),
            Some("peek")
        );
    }
}
