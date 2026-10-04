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
    dismiss_update_at, feed_ideas, feed_visible, file_idea_todo, hold_if_quiet,
    idea_open_line, unpin_feed_idea,
    idea_todo_title,
    links_from_research, mark_update_opened, parse_lookup, post_help, post_update,
    quiet_hours_active, remember_dismissed_source, route_schedule,
    schedule_created_card, tick_feed_pulse, visible_digests,
    CardReaction, DigestEdition, DigestMaterial, PausedJob, PulseNow, RepeatedAction,
    TasteNote, UpdateAction, UpdateCard,
    UpdateKind, UpdateStatus, DIGEST_PAINT_MAX, FEED_PAINT_MAX,
};

/// What an ideas reply is checked against.
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

/// Title, runs meta, body, and the why line each need a row.
const FEED_CARD_H: f32 = 96.0;
/// `Rect::intersects` treats a shared edge as a hit, so the deck keeps a 1px gap.
const DECK_CLEAR: f32 = 1.0;
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

/// Fully open deck. Cards fan upward from `front` and stay in the band above
/// `composer`. A composer with no area leaves only the on-screen clamp.
pub(super) fn expanded_deck_rects(
    front: egui::Pos2,
    width: f32,
    n: usize,
    card_h: f32,
    stride: f32,
    composer: egui::Rect,
    screen_top: f32,
) -> Vec<egui::Rect> {
    if n == 0 {
        return Vec::new();
    }
    let card_h = card_h.max(0.0);
    let stride = stride.max(0.0);
    let constrained = composer.height() > 1.0 && composer.width() > 1.0;
    if !constrained {
        let shift = if n <= 1 {
            0.0
        } else {
            let top = front.y - (n - 1) as f32 * stride;
            (screen_top + 8.0 - top).max(0.0)
        };
        return (0..n)
            .map(|index| {
                let top = front.y - index as f32 * stride + shift;
                egui::Rect::from_min_size(egui::pos2(front.x, top), egui::vec2(width, card_h))
            })
            .collect();
    }
    let limit = composer.top() - DECK_CLEAR;
    let mut height = card_h;
    let mut tops: Vec<f32> = (0..n)
        .map(|index| front.y - index as f32 * stride)
        .collect();
    let lowest_bottom = tops[0] + height;
    if lowest_bottom > limit {
        let up = lowest_bottom - limit;
        for top in &mut tops {
            *top -= up;
        }
    }
    let highest = tops.iter().copied().fold(f32::INFINITY, f32::min);
    if highest < screen_top {
        let room = (limit - screen_top).max(0.0);
        if room <= 0.0 {
            return (0..n)
                .map(|_| {
                    egui::Rect::from_min_size(egui::pos2(front.x, limit), egui::vec2(width, 0.0))
                })
                .collect();
        }
        height = card_h.min(room);
        let extra = (room - height).max(0.0);
        let step = if n <= 1 {
            0.0
        } else {
            stride.min(extra / (n as f32 - 1.0))
        };
        tops = (0..n)
            .map(|index| limit - height - index as f32 * step)
            .collect();
    }
    tops.into_iter()
        .map(|top| egui::Rect::from_min_size(egui::pos2(front.x, top), egui::vec2(width, height)))
        .collect()
}

fn lerp_rect(from: egui::Rect, to: egui::Rect, t: f32) -> egui::Rect {
    let t = t.clamp(0.0, 1.0);
    egui::Rect::from_min_max(from.min + (to.min - from.min) * t, from.max + (to.max - from.max) * t)
}

/// Move a card that crosses the composer into the band above it.
fn separate_from_composer(rect: egui::Rect, composer: egui::Rect) -> egui::Rect {
    if composer.height() <= 1.0 || composer.width() <= 1.0 || !rect.intersects(composer) {
        return rect;
    }
    let top = composer.top() - DECK_CLEAR - rect.height();
    egui::Rect::from_min_size(egui::pos2(rect.left(), top), rect.size())
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

pub(super) fn home_feed_count(cards: &[UpdateCard], pulse: &grokhub_core::FeedPulse, now: u64) -> usize {
    home_stack_cards(cards, pulse, now).len()
}

pub(super) enum FeedAct {
    Open(String),
    Dismiss(String),
    Build(String),
    Discuss(String),
    Archive(String),
    /// Remove an idea from the Ideas board.
    Drop(String),
    More(String),
    Less(String),
    Hide(String),
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

    pub(super) fn more_like_this(&mut self, id: &str) {
        let Some(card) = self.updates.iter().find(|c| c.id == id).cloned() else {
            return;
        };
        grokhub_core::clear_less_mute(&mut self.cfg.feed_pulse, &card);
        self.log_card_signal(&card, grokhub_core::CardEvent::More, None);
        self.persist_cfg();
    }

    pub(super) fn less_like_this(&mut self, id: &str) {
        let Some(card) = self.updates.iter().find(|c| c.id == id).cloned() else {
            return;
        };
        grokhub_core::mute_less_like(&mut self.cfg.feed_pulse, &card, now_ms());
        self.log_card_signal(&card, grokhub_core::CardEvent::Less, None);
        self.persist_cfg();
    }

    pub(super) fn hide_automation_from_home(&mut self, id: &str) {
        let Some(card) = self.updates.iter().find(|c| c.id == id).cloned() else {
            return;
        };
        if card.kind != UpdateKind::AutomationDone || card.source_id.trim().is_empty() {
            return;
        }
        let before = self.cfg.feed_pulse.muted_sources.len();
        grokhub_core::hide_home_source(&mut self.cfg.feed_pulse, &card.source_id);
        if self.cfg.feed_pulse.muted_sources.len() == before {
            return;
        }
        self.log_card_signal(&card, grokhub_core::CardEvent::Hidden, None);
        self.persist_cfg();
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
            PulseNow { now_ms: now, quiet },
            material,
        );
        if fed_edition && tick.digest_consumed {
            self.digest_pending = None;
        }
        self.digest_wants_lookup = tick.digest_needs_lookup;
        if tick.digest_needs_lookup {
            self.digest_steer = steer;
        }
        let help_changed = self.note_help_offers(now, quiet);
        if tick.cards_changed || help_changed {
            self.persist_updates();
        }
        if tick.pulse_changed || help_changed {
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
            }) {
                self.situation_ping = Some((
                    card.title.clone(),
                    card.body.clone().unwrap_or_default(),
                ));
            }
        }
        help.ideas > 0
            || help.posted
            || self.updates.len() != before
            || self.cfg.feed_pulse.paused_seen != seen_before
    }

    /// One lookup a day, off the UI thread. A missing key waits and does not stamp a search.
    /// Unit tests never start it: this machine's Grok login would spend a real call.
    pub(super) fn follow_feed_lookup(&mut self) {
        if !self.digest_wants_lookup || self.digest_busy || self.digest_pending.is_some() {
            return;
        }
        if self.cfg.native_engine {
            self.spawn_native_digest();
            return;
        }
        if cfg!(test) {
            return;
        }
        let key = self.bearer();
        if key.trim().is_empty() {
            return;
        }
        let prompt = grokhub_core::digest_lookup_prompt(&self.digest_steer);
        let model = CABIN_FAST_MODEL.to_string();
        let (tx, rx) = mpsc::channel();
        self.digest_rx = Some(rx);
        self.digest_busy = true;
        std::thread::spawn(move || {
            let messages = [("user".into(), prompt)];
            let effort = Some(grokhub_core::BACKGROUND_EFFORT);
            // The completion has no live search, so a URL it gives is only a claim:
            // keep the ones that answer.
            let reply = grok_chat(&key, &model, &messages, None, effort).map(|text| {
                let dead: Vec<String> = links_from_research(&text)
                    .into_iter()
                    .map(|link| link.url)
                    .filter(|url| !crate::xai::url_answers(url))
                    .collect();
                grokhub_core::drop_dead_links(&text, &dead)
            });
            let _ = tx.send(reply);
        });
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
                if self.cfg.native_engine && !err.is_empty() {
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
        crate::notify::ping("GrokHub", &line);
    }

    pub(super) fn paint_update_feed(&mut self, ui: &mut egui::Ui, pane_w: f32, composer: egui::Rect) {
        self.ensure_useful_ideas();
        let mem_id = egui::Id::new("home-feed-stack");
        if !feed_visible(&self.updates) {
            ui.ctx().data_mut(|d| {
                d.insert_temp(mem_id, FeedStackMem::default());
                d.insert_temp(egui::Id::new("home-deck-defer"), None::<DeckDefer>);
            });
            return;
        }
        let now = now_ms();
        let (cards, rank) = home_deck(&self.updates, &self.cfg.feed_pulse, &self.card_prefs, now);
        if cards.is_empty() && rank.folded.is_empty() {
            ui.ctx().data_mut(|d| {
                d.insert_temp(mem_id, FeedStackMem::default());
                d.insert_temp(egui::Id::new("home-deck-defer"), None::<DeckDefer>);
            });
            return;
        }
        if cards.is_empty() {
            ui.ctx().data_mut(|d| {
                d.insert_temp(mem_id, FeedStackMem::default());
                d.insert_temp(egui::Id::new("home-deck-defer"), None::<DeckDefer>);
            });
        } else {
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
            // Paint later, after the home composer, inside the chat pane. The open
            // fan stays in the space above the composer and does not cover that box.
            ui.ctx().data_mut(|d| {
                d.insert_temp(
                    egui::Id::new("home-deck-defer"),
                    Some(DeckDefer {
                        stack,
                        width: pane_w,
                        view,
                        composer,
                    }),
                );
            });
        }
        self.paint_home_fold(ui, pane_w, &rank, now);
    }

    fn paint_home_fold(
        &mut self,
        ui: &mut egui::Ui,
        pane_w: f32,
        rank: &grokhub_core::HomeRank,
        now: u64,
    ) {
        let n = rank.folded.len();
        if n == 0 {
            return;
        }
        ui.add_space(4.0);
        let label = format!("More ({n})");
        let clicked = ui
            .add(
                egui::Label::new(
                    RichText::new(label)
                        .size(crate::theme::FONT_TIP)
                        .color(crate::theme::muted()),
                )
                .sense(egui::Sense::click()),
            )
            .clicked();
        if clicked {
            self.home_fold_open = true;
        }
        ui.label(
            RichText::new(grokhub_core::FOLD_NOTE)
                .size(crate::theme::FONT_TIP)
                .color(crate::theme::muted()),
        );
        if !self.home_fold_open {
            return;
        }
        let folded = rank.folded.clone();
        let novelty = rank.novelty_id.clone();
        for card in folded {
            let hint = event_hint(&card, &self.card_prefs, now, novelty.as_deref());
            if let Some(act) = paint_feed_card(ui, &card, pane_w, hint.as_deref()) {
                self.apply_feed_act(Some(act));
            }
            ui.add_space(6.0);
        }
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
        let now = now_ms();
        let (cards, rank) = home_deck(&self.updates, &self.cfg.feed_pulse, &self.card_prefs, now);
        if cards.is_empty() {
            return;
        }
        if grokhub_core::record_home_floors(&mut self.cfg.feed_pulse, &rank.deck, now) {
            self.persist_cfg();
        }
        let hints = deck_hints(&cards, &self.card_prefs, now, rank.novelty_id.as_deref());
        let painted = paint_slide_deck(
            ui,
            &cards,
            &hints,
            deferred.stack,
            deferred.width,
            &deferred.view,
            deferred.composer,
        );
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
            Some(FeedAct::Open(id)) => self.open_feed_card(&id),
            Some(FeedAct::Discuss(id)) => self.discuss_card(&id),
            Some(FeedAct::Archive(id)) => self.archive_feed_digest(&id),
            Some(FeedAct::Drop(id)) => self.delete_idea(&id),
            Some(FeedAct::More(id)) => self.more_like_this(&id),
            Some(FeedAct::Less(id)) => self.less_like_this(&id),
            Some(FeedAct::Hide(id)) => self.hide_automation_from_home(&id),
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
            self.log_card_signal(&card, grokhub_core::CardEvent::Opened, None);
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
            .map(|c| c.title.clone())
            .unwrap_or_default();
        self.persist_updates();
        if let Some(card) = self.updates.iter().find(|c| c.id == id).cloned() {
            self.log_card_signal(&card, grokhub_core::CardEvent::Opened, None);
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
            .map(|c| c.title.clone())
            .unwrap_or_default();
        self.persist_updates();
        if let Some(card) = self.updates.iter().find(|c| c.id == id).cloned() {
            self.log_card_signal(&card, grokhub_core::CardEvent::Opened, None);
        }
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
        let (_, inputs) = self.idea_request();
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
        if self.ideas_rx.is_some() || (!self.llm_ready() && !self.cfg.native_engine) {
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
        let (prompt, inputs) = self.idea_request();
        self.cfg.feed_pulse.last_ideas_ms = now;
        self.persist_cfg();
        let (tx, rx) = mpsc::channel();
        self.ideas_rx = Some((rx, inputs));
        if self.cfg.native_engine {
            self.spawn_native_ideas(prompt, tx);
            return;
        }
        let key = self.bearer();
        std::thread::spawn(move || {
            let _ = tx.send(cabin_fast_llm(key, prompt));
        });
    }

    /// The prompt plus what the reply must not copy (their own lines) or repeat,
    /// and what an idea has to stand on.
    pub(super) fn idea_request(&self) -> (String, IdeaInputs) {
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
        if !matches!(card.kind, UpdateKind::Digest | UpdateKind::Suggestion) {
            return;
        }
        self.log_card_signal(&card, grokhub_core::CardEvent::Opened, None);
        if let Some(thread_id) = card.discuss_thread.as_deref() {
            if let Some(idx) = self.threads.iter().position(|t| t.id == thread_id) {
                self.switch_thread(idx);
                self.nav = Nav::Chat;
                self.composer_want_focus = true;
                return;
            }
        }
        let context = if card.kind == UpdateKind::Suggestion {
            card.body.clone().unwrap_or_else(|| card.title.clone())
        } else {
            idea_open_line(&card)
        };
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
        if matches!(kind, Some(UpdateKind::AutomateOffer)) {
            self.open_offer_on_automations(id);
            return;
        }
        if !mark_update_opened(&mut self.updates, id) {
            return;
        }
        let (action, board_card) = self
            .updates
            .iter()
            .find(|c| c.id == id)
            .map(|c| (c.action.clone(), c.board_id.clone()))
            .unwrap_or_default();
        self.persist_updates();
        if let Some(card) = self.updates.iter().find(|c| c.id == id).cloned() {
            self.log_card_signal(&card, grokhub_core::CardEvent::Opened, None);
        }
        self.follow_update_action(action);
        // A run that filed a Follow up card opens that card on the board.
        if let Some(card) = board_card.filter(|b| self.board.iter().any(|c| &c.id == b)) {
            self.nav = Nav::Workboard;
            self.board_view.open = Some(card);
            self.board_view.pinned = true;
        }
    }

    pub(super) fn dismiss_feed_card(&mut self, id: &str) {
        let prior = self.updates.iter().find(|c| c.id == id).cloned();
        let kind = prior.as_ref().map(|c| c.kind);
        let was_open = prior
            .as_ref()
            .is_some_and(|c| c.status == UpdateStatus::Opened);
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
                self.log_card_signal(&card, grokhub_core::CardEvent::Dismissed, Some(was_open));
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

#[derive(Clone, Debug)]
struct DeckDefer {
    stack: egui::Rect,
    width: f32,
    view: StackView,
    composer: egui::Rect,
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

fn home_stack_cards(cards: &[UpdateCard], pulse: &grokhub_core::FeedPulse, now: u64) -> Vec<UpdateCard> {
    let mut out = grokhub_core::home_event_cards(cards, pulse, now);
    out.extend(feed_ideas(cards, now));
    out.extend(visible_digests(cards).into_iter().take(DIGEST_PAINT_MAX));
    out
}

fn home_deck(
    cards: &[UpdateCard],
    pulse: &grokhub_core::FeedPulse,
    prefs: &grokhub_core::CardPrefs,
    now: u64,
) -> (Vec<UpdateCard>, grokhub_core::HomeRank) {
    let rank = grokhub_core::rank_home_events(cards, pulse, prefs, now);
    let mut shown = rank.deck.clone();
    shown.extend(feed_ideas(cards, now));
    shown.extend(visible_digests(cards).into_iter().take(DIGEST_PAINT_MAX));
    (shown, rank)
}

fn event_hint(
    card: &UpdateCard,
    prefs: &grokhub_core::CardPrefs,
    now: u64,
    novelty_id: Option<&str>,
) -> Option<String> {
    if !card.kind.event() {
        return None;
    }
    grokhub_core::explain_hint(card, prefs, now, novelty_id == Some(card.id.as_str()))
}

fn deck_hints(
    cards: &[UpdateCard],
    prefs: &grokhub_core::CardPrefs,
    now: u64,
    novelty_id: Option<&str>,
) -> Vec<Option<String>> {
    cards
        .iter()
        .map(|card| event_hint(card, prefs, now, novelty_id))
        .collect()
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

fn paint_card_at(
    ui: &mut egui::Ui,
    card: &UpdateCard,
    rect: egui::Rect,
    hint: Option<&str>,
) -> Option<FeedAct> {
    let mut act = None;
    ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
        ui.set_min_size(rect.size());
        ui.set_width(rect.width());
        ui.spacing_mut().item_spacing = egui::vec2(8.0, 4.0);
        act = paint_feed_card(ui, card, rect.width(), hint);
    });
    act
}

fn tucked_slide(index: usize, spread: f32) -> bool {
    index > 0 && spread < 0.42
}

struct SlidePlacement {
    spread: f32,
    popped: bool,
    lift: f32,
}

fn slide_placements(ctx: &egui::Context, cards: &[UpdateCard], view: &StackView) -> Vec<SlidePlacement> {
    cards
        .iter()
        .enumerate()
        .map(|(index, card)| {
            let popped = view.popped.as_deref() == Some(card.id.as_str());
            SlidePlacement {
                spread: slide_spread(ctx, index, view.expanded || popped),
                popped,
                lift: slide_lift(ctx, index, popped),
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
        egui::StrokeKind::Middle,
    );
}

fn paint_slide_deck(
    ui: &mut egui::Ui,
    cards: &[UpdateCard],
    hints: &[Option<String>],
    stack: egui::Rect,
    width: f32,
    view: &StackView,
    composer: egui::Rect,
) -> SlidePaint {
    let front = stack.left_top();
    let screen_top = ui.ctx().content_rect().top();
    let placements = slide_placements(ui.ctx(), cards, view);
    let open = expanded_deck_rects(
        front,
        width,
        cards.len(),
        FEED_CARD_H,
        slide_stride(),
        composer,
        screen_top,
    );
    let rects: Vec<egui::Rect> = placements
        .iter()
        .enumerate()
        .map(|(index, place)| {
            let rest = slide_rect(front, width, rest_slide(index));
            let target = open.get(index).copied().unwrap_or(rest);
            let mut rect = lerp_rect(rest, target, place.spread);
            if place.lift > 0.0 {
                rect = rect.translate(egui::vec2(0.0, -STACK_POP * place.lift));
            }
            separate_from_composer(rect, composer)
        })
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
                paint_card_at(ui, &cards[index], rect, hints.get(index).and_then(|hint| hint.as_deref()))
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
    hint: Option<&str>,
) -> Option<FeedAct> {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(pane_w, FEED_CARD_H), egui::Sense::hover());
    ui.painter().rect(
        rect,
        crate::theme::CARD_RADIUS,
        crate::theme::elevated(),
        egui::Stroke::new(1.0_f32, crate::theme::border()),
        egui::StrokeKind::Middle,
    );
    let x_rect = egui::Rect::from_min_size(
        egui::pos2(rect.right() - 32.0, rect.top() + 6.0),
        egui::vec2(24.0, 24.0),
    );
    let menu_rect = egui::Rect::from_min_size(
        egui::pos2(x_rect.left() - 28.0, rect.top() + 6.0),
        egui::vec2(24.0, 24.0),
    );
    let text_right = if card.kind.event() {
        menu_rect.left() - 4.0
    } else {
        x_rect.left() - 4.0
    };
    let text_rect = egui::Rect::from_min_max(
        egui::pos2(rect.left() + 8.0, rect.top() + 6.0),
        egui::pos2(text_right, rect.bottom() - 6.0),
    );
    ui.scope_builder(egui::UiBuilder::new().max_rect(text_rect), |ui| {
        ui.set_width(text_rect.width());
        ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
        let title_color = if card.status == UpdateStatus::Opened || card.built {
            crate::theme::muted()
        } else {
            crate::theme::fg()
        };
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
        if card.runs > 1 {
            let clock = Cabin::local_clock();
            if let Some(line) = grokhub_core::runs_latest_line(
                card.runs,
                card.created_at,
                now_ms(),
                clock.hour,
                clock.minute,
            ) {
                ui.label(
                    RichText::new(line)
                        .size(crate::theme::FONT_TIP)
                        .color(crate::theme::muted()),
                );
            }
        }
        if let Some(body) = card.body.as_deref() {
            ui.label(
                RichText::new(body)
                    .size(crate::theme::FONT_TIP)
                    .color(crate::theme::muted()),
            );
        }
        if let Some(why) = card.why.as_deref().map(str::trim).filter(|line| !line.is_empty()) {
            let line = match hint.map(str::trim).filter(|text| !text.is_empty()) {
                Some(hint) => format!("{why} · {hint}"),
                None => why.to_string(),
            };
            ui.label(
                RichText::new(line)
                    .size(crate::theme::FONT_TIP)
                    .color(crate::theme::muted()),
            );
        }
    });
    // Registered last so the title labels do not take the click.
    let hit = ui.interact(
        rect,
        egui::Id::new(("feed-card", &card.id)),
        egui::Sense::click(),
    );
    let over_x = hit.hover_pos().is_some_and(|pos| x_rect.contains(pos));
    let over_menu = card.kind.event() && hit.hover_pos().is_some_and(|pos| menu_rect.contains(pos));
    if card.kind.event() {
        ui.painter().text(
            menu_rect.center(),
            egui::Align2::CENTER_CENTER,
            "⋯",
            egui::FontId::proportional(16.0),
            if over_menu {
                crate::theme::fg()
            } else {
                crate::theme::muted()
            },
        );
    }
    ui.painter().text(
        x_rect.center(),
        egui::Align2::CENTER_CENTER,
        "×",
        egui::FontId::proportional(16.0),
        if over_x {
            crate::theme::fg()
        } else {
            crate::theme::muted()
        },
    );
    if hit.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let mut menu_act = None;
    if card.kind.event() {
        let popup_id = egui::Id::new(("feed-card-menu", &card.id));
        if hit.secondary_clicked() {
            egui::Popup::open_id(ui.ctx(), popup_id);
        }
        let on_menu_click = hit.clicked()
            && hit
                .interact_pointer_pos()
                .is_some_and(|pos| menu_rect.contains(pos));
        if on_menu_click {
            egui::Popup::toggle_id(ui.ctx(), popup_id);
        }
        let hide = card.kind == UpdateKind::AutomationDone && !card.source_id.trim().is_empty();
        egui::Popup::new(popup_id, ui.ctx().clone(), menu_rect, ui.layer_id())
            .kind(egui::PopupKind::Menu)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClick)
            .layout(egui::Layout::top_down_justified(egui::Align::Min))
            .show(|ui| {
                if ui.button("More like this").clicked() {
                    menu_act = Some(FeedAct::More(card.id.clone()));
                    ui.close();
                }
                if ui.button("Less like this").clicked() {
                    menu_act = Some(FeedAct::Less(card.id.clone()));
                    ui.close();
                }
                if hide && ui.button("Hide this automation's runs from Home").clicked() {
                    menu_act = Some(FeedAct::Hide(card.id.clone()));
                    ui.close();
                }
            });
    }
    if menu_act.is_some() {
        return menu_act;
    }
    if !hit.clicked() {
        return None;
    }
    let on_menu = card.kind.event()
        && hit
            .interact_pointer_pos()
            .is_some_and(|pos| menu_rect.contains(pos));
    if on_menu {
        return None;
    }
    let on_x = hit
        .interact_pointer_pos()
        .is_some_and(|pos| x_rect.contains(pos));
    Some(if on_x {
        if card.kind == UpdateKind::Digest {
            FeedAct::Archive(card.id.clone())
        } else {
            FeedAct::Dismiss(card.id.clone())
        }
    } else {
        match card.kind {
            UpdateKind::Digest | UpdateKind::Suggestion | UpdateKind::Idea => {
                FeedAct::Discuss(card.id.clone())
            }
            UpdateKind::AutomateOffer
            | UpdateKind::AutomationDone
            | UpdateKind::ScheduleCreated => FeedAct::Open(card.id.clone()),
        }
    })
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

    #[test]
    fn expanded_deck_never_covers_the_composer() {
        use super::{expanded_deck_rects, open_slide, slide_stride};
        use eframe::egui;
        let width = 280.0;
        let stride = slide_stride();
        let composer = egui::Rect::from_min_max(egui::pos2(0.0, 420.0), egui::pos2(320.0, 520.0));
        let front = egui::pos2(20.0, 540.0);
        let rects = expanded_deck_rects(front, width, 3, FEED_CARD_H, stride, composer, 0.0);
        assert_eq!(rects.len(), 3);
        for rect in &rects {
            assert!(
                !rect.intersects(composer),
                "open card {rect:?} covers the composer {composer:?}"
            );
            assert!(rect.bottom() <= composer.top() - 1.0 + 0.05);
        }
        assert!(
            rects[0].top() < front.y,
            "the fan shifts up off the resting slot"
        );

        let above = egui::Rect::from_min_max(egui::pos2(0.0, 500.0), egui::pos2(320.0, 640.0));
        let high = egui::pos2(20.0, 200.0);
        let parked = expanded_deck_rects(high, width, 2, FEED_CARD_H, stride, above, 0.0);
        assert!((parked[0].top() - high.y).abs() < 0.05);
        assert!((parked[1].top() - (high.y + open_slide(1).dy)).abs() < 0.05);
        for rect in &parked {
            assert!(!rect.intersects(above));
            assert!(rect.bottom() <= above.top() - 1.0 + 0.05);
        }

        let tight = egui::Rect::from_min_max(egui::pos2(0.0, 50.0), egui::pos2(320.0, 200.0));
        let squeezed = expanded_deck_rects(front, width, 3, FEED_CARD_H, stride, tight, 0.0);
        for rect in &squeezed {
            assert!(!rect.intersects(tight), "{rect:?}");
            assert!(rect.top() >= -0.05);
            assert!(rect.bottom() <= tight.top() - 1.0 + 0.05);
        }
        assert!(squeezed[2].top() + 0.05 >= 0.0);

        let none = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(0.0, 0.0));
        let free = expanded_deck_rects(egui::pos2(10.0, 40.0), width, 8, FEED_CARD_H, stride, none, 0.0);
        assert!(free[7].top() >= 8.0 - 0.05);
        assert!(!free.iter().any(|rect| rect.intersects(none) && none.height() > 1.0));
    }
}
