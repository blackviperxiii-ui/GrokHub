//! Home update feed in the slot that held the Coding / Life chip and the
//! under-bar workboard summary. The event slot is hidden when it has nothing
//! to paint. Idea and digest cards use the same store and do not take event rows.
//!
//! On the chat screen the cards sit in a three-deep deck. Hover slides them
//! up to full size, one above the next, on top of the chat box. A card behind
//! the front one can lift out and stays up until the pointer is back on the deck.
//! A digest, suggestion, or image card that is open paints the Pulse card:
//! source and age, a short takeaway, the image, Liked, and Discuss.

use super::*;
use grokhub_core::{
    automation_done_card,
    dismiss_update_at, feed_ideas, feed_visible, file_idea_todo, hold_if_quiet,
    unpin_feed_idea,
    idea_todo_title,
    links_from_research, mark_update_opened, parse_lookup, post_help, post_update,
    quiet_hours_active, remember_dismissed_source, route_schedule,
    schedule_created_card, tick_feed_pulse, visible_digests,
    DigestEdition, DigestMaterial, PausedJob, PulseNow, RepeatedAction,
    TasteNote, UpdateAction, UpdateCard,
    UpdateKind, UpdateStatus, DIGEST_PAINT_MAX,
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
/// Open deck: each card behind the front shows this much, enough for its title row.
pub(super) const PEEK_H: f32 = 34.0;
/// The card under the pointer rises this far above the card in front, so it reads in full.
pub(super) const LIFT_STEP: f32 = FEED_CARD_H + FEED_GAP;
/// Open Pulse card on the deck, before a source image. Grows up from the strip.
/// Fits source · age, a two-line title, three takeaway rows, Read at, and Like/Discuss.
const FULL_FEED_H: f32 = 168.0;
/// Extra height when that card has a source image: one thumb (at most 168) plus spacing.
const FULL_FEED_IMAGE_H: f32 = 174.0;
const SLIDE_SECS: f32 = 0.22;
/// A card joining the deck after one leaves slides in from below and to the right.
const FLY_SECS: f32 = 0.45;
/// Clearer mid-flight offset for stills / video (CD-04).
const FLY_DX: f32 = 56.0;
const FLY_DY: f32 = 42.0;
/// After ×, the card that was already on the deck eases into the front (CD-01).
pub(super) const SETTLE_SECS: f32 = 0.15;
pub(super) const SETTLE_DY: f32 = 2.0;

#[cfg(test)]
pub(super) fn stacked_feed_h(n: usize) -> f32 {
    if n == 0 {
        return 0.0;
    }
    let n = n as f32;
    n * FEED_CARD_H + (n - 1.0) * FEED_GAP
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

/// Settled pose of each card, front first. The front card never moves, so a
/// deck of one stays put. Open, each card behind shows its title row above the
/// one in front, and the lifted card rises only far enough to clear the card in
/// front of it. `room` caps how far above the front card anything goes.
pub(super) fn deck_poses(n: usize, open: bool, lifted: Option<usize>, room: f32) -> Vec<SlidePose> {
    if n <= 1 || !open {
        return (0..n).map(rest_slide).collect();
    }
    let room = room.max(0.0);
    let mut dy = 0.0_f32;
    (0..n)
        .map(|index| {
            if index > 0 {
                dy -= if lifted == Some(index) { LIFT_STEP } else { PEEK_H };
            }
            SlidePose {
                dy: dy.max(-room),
                scale: 1.0,
            }
        })
        .collect()
}

pub(super) fn slide_rect(front: egui::Pos2, width: f32, pose: SlidePose) -> egui::Rect {
    let w = width * pose.scale;
    let h = FEED_CARD_H * pose.scale;
    let x = front.x + (width - w) * 0.5;
    let y = front.y + pose.dy + (FEED_CARD_H - h) * 0.5;
    egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(w, h))
}

/// Where each card rests once the slide is over, back to front, so the last
/// hit under the pointer is the card on top. Hover tests these, never the
/// moving rects, so a card sliding under a still pointer cannot flip the deck.
/// The lifted card's hit reaches down to the card in front, so the pointer
/// crossing the small gap between them does not drop it.
fn settled_hits(
    cards: &[UpdateCard],
    front: egui::Pos2,
    width: f32,
    poses: &[SlidePose],
    lifted: Option<usize>,
    expanded: bool,
    room: f32,
) -> Vec<SlideHit> {
    let lift = chrome_lift(cards, expanded, lifted, room);
    let mut hits: Vec<SlideHit> = (0..cards.len().min(poses.len()))
        .rev()
        .map(|index| {
            let pose = SlidePose {
                dy: card_dy(index, poses[index].dy, &lift, room),
                scale: poses[index].scale,
            };
            let mut rect = slide_rect(front, width, pose);
            if lift.index == Some(index) {
                let h = FEED_CARD_H + lift.extra;
                rect = full_card_rect(rect, h);
            }
            if lifted == Some(index) {
                if let Some(ahead) = index.checked_sub(1) {
                    let ahead = SlidePose {
                        dy: card_dy(ahead, poses[ahead].dy, &lift, room),
                        scale: poses[ahead].scale,
                    };
                    let ahead_top = slide_rect(front, width, ahead).top();
                    if rect.bottom() < ahead_top {
                        rect.set_bottom(ahead_top);
                    }
                }
            }
            SlideHit {
                id: cards[index].id.clone(),
                #[cfg(test)]
                index,
                rect,
            }
        })
        .collect();
    // The open Pulse card overlaps the strip under it. The pointer stays on it.
    if let Some(id) = lift.index.and_then(|index| cards.get(index).map(|card| card.id.clone())) {
        if let Some(pos) = hits.iter().position(|hit| hit.id == id) {
            let hit = hits.remove(pos);
            hits.push(hit);
        }
    }
    hits
}

/// An open deck keeps hover over the pile and every card that slid up, so the
/// pointer can move between them. A closed deck opens only from the pile itself.
fn deck_hover_zone(stack: egui::Rect, hits: &[SlideHit], expanded: bool) -> egui::Rect {
    if !expanded {
        return stack;
    }
    hits.iter().fold(stack, |zone, hit| zone.union(hit.rect))
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct StackView {
    pub expanded: bool,
    pub popped: Option<String>,
}

/// Hover opens a deck of two or more. The card under the pointer lifts; the
/// front card is already readable, so it never does. Off the deck it closes.
pub(super) fn next_feed_stack(
    n: usize,
    on_deck: bool,
    hovered: Option<String>,
    front_id: Option<&str>,
) -> StackView {
    if n <= 1 || !on_deck {
        return StackView::default();
    }
    StackView {
        expanded: true,
        popped: hovered.filter(|id| Some(id.as_str()) != front_id),
    }
}

fn lifted_index(cards: &[UpdateCard], view: &StackView) -> Option<usize> {
    let id = view.popped.as_deref()?;
    cards.iter().position(|card| card.id == id)
}

pub(super) fn home_feed_count(cards: &[UpdateCard], pulse: &grokhub_core::FeedPulse, now: u64) -> usize {
    home_stack_cards(cards, pulse, now).len()
}

pub(super) enum FeedAct {
    Open(String),
    /// × on the main window's deck.
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
}

/// Done-for-you cards carry a white rule down the left edge, the cabin's
/// one accent (same family as the hard card).
fn paint_done_accent(painter: &egui::Painter, card: &UpdateCard, rect: egui::Rect) {
    if card.kind != UpdateKind::DoneForYou {
        return;
    }
    let x = rect.left() + 1.5;
    painter.line_segment(
        [egui::pos2(x, rect.top() + 8.0), egui::pos2(x, rect.bottom() - 8.0)],
        egui::Stroke::new(2.0_f32, crate::theme::fg()),
    );
}

/// Digest, suggestion, or a post that already has a source image.
fn feed_style_card(card: &UpdateCard) -> bool {
    matches!(card.kind, UpdateKind::Digest | UpdateKind::Suggestion)
        || !card.pulse.image_urls.is_empty()
}

fn expanded_feed_h(card: &UpdateCard) -> f32 {
    if card.pulse.image_urls.is_empty() {
        FULL_FEED_H
    } else {
        FULL_FEED_H + FULL_FEED_IMAGE_H
    }
}

struct ChromeLift {
    /// The card painting the full Pulse chrome, if the deck is open.
    index: Option<usize>,
    /// How far that card grows above the compact strip. Cards behind shift up by this.
    extra: f32,
}

fn chrome_lift(cards: &[UpdateCard], expanded: bool, lifted: Option<usize>, room: f32) -> ChromeLift {
    if !expanded {
        return ChromeLift {
            index: None,
            extra: 0.0,
        };
    }
    let index = match lifted {
        Some(i) => cards.get(i).and_then(|card| feed_style_card(card).then_some(i)),
        None => cards.first().and_then(|card| feed_style_card(card).then_some(0)),
    };
    let extra = index
        .map(|i| {
            let want = expanded_feed_h(&cards[i]) - FEED_CARD_H;
            want.min(room.max(0.0)).max(0.0)
        })
        .unwrap_or(0.0);
    ChromeLift { index, extra }
}

fn card_dy(index: usize, pose_dy: f32, lift: &ChromeLift, room: f32) -> f32 {
    let dy = if lift.index.is_some_and(|i| index > i) {
        pose_dy - lift.extra
    } else {
        pose_dy
    };
    dy.max(-room.max(0.0))
}

fn full_card_rect(compact: egui::Rect, h: f32) -> egui::Rect {
    egui::Rect::from_min_max(
        egui::pos2(compact.left(), compact.bottom() - h),
        compact.right_bottom(),
    )
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

    /// Retry on a crash card: open the chat the run was in and send the
    /// card's retry line there (`/retry`, or `/bg …` for a background run).
    pub(super) fn crash_retry(&mut self, id: &str) {
        let Some(card) = self.updates.iter().find(|c| c.id == id).cloned() else {
            return;
        };
        let Some(line) = card.prompt.clone().filter(|_| grokhub_core::is_crash_card(&card)) else {
            return;
        };
        if mark_update_opened(&mut self.updates, id) {
            self.persist_updates();
        }
        self.follow_update_action(card.action.clone());
        self.send_chat(line);
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
        self.note_finished_card_uses();
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
                    && !card.pulse.quiet_batched
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
        let prompt = grokhub_core::pulse::feed_prompt(&self.digest_steer, &self.cfg.feed_instructions);
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
        let now = now_ms();
        let deck = home_deck(&self.updates, &self.cfg.feed_pulse, &self.card_prefs, now);
        let cards = &deck.cards;
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
            let zone = deck_hover_zone(stack, &prev.hits, prev.expanded);
            let on_deck = pointer.is_some_and(|p| zone.contains(p));
            let hovered = hovered_slide_card(pointer, &prev.hits);
            let front_id = cards.first().map(|card| card.id.as_str());
            let view = drop_missing_pop(cards, next_feed_stack(cards.len(), on_deck, hovered, front_id));
            // Paint later, after the home composer, so cards that slide up stay on top.
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
        self.paint_home_fold(ui, pane_w, &deck.rank, now);
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
        let deck = home_deck(&self.updates, &self.cfg.feed_pulse, &self.card_prefs, now);
        let cards = &deck.cards;
        if cards.is_empty() {
            return;
        }
        if grokhub_core::record_home_floors(&mut self.cfg.feed_pulse, &deck.rank.deck, now) {
            self.persist_cfg();
        }
        fly_in_newcomers(ui.ctx(), cards);
        arm_front_settle(ui.ctx(), cards);
        let hints = deck_hints(cards, &self.card_prefs, now, deck.rank.novelty_id.as_deref());
        let front = deferred.stack.left_top();
        let lifted = lifted_index(cards, &deferred.view);
        let room = front.y - ui.ctx().content_rect().top() - 8.0;
        let poses = deck_poses(cards.len(), deferred.view.expanded, lifted, room);
        self.kick_pulse_images(cards);
        let painted = self.paint_slide_deck(
            ui,
            cards,
            &hints,
            front,
            deferred.width,
            &poses,
            cards.len() + deck.waiting,
            deferred.view.expanded,
            lifted,
            room,
            now,
        );
        let hits = settled_hits(
            cards,
            front,
            deferred.width,
            &poses,
            lifted,
            deferred.view.expanded,
            room,
        );
        ui.ctx().data_mut(|d| {
            d.insert_temp(
                egui::Id::new("home-feed-stack"),
                FeedStackMem {
                    expanded: deferred.view.expanded,
                    hits,
                },
            );
        });
        self.apply_feed_act(painted);
    }

    pub(super) fn apply_feed_act(&mut self, act: Option<FeedAct>) {
        match act {
            Some(FeedAct::Dismiss(id)) => self.close_home_card(&id),
            Some(FeedAct::Open(id)) => self.open_feed_card(&id),
            Some(FeedAct::Discuss(id)) => self.discuss_card(&id),
            Some(FeedAct::Like(id)) => self.pulse_like(&id, &Self::local_day()),
            Some(FeedAct::Undo(id)) => self.done_for_you_undo(&id),
            Some(FeedAct::NeverAgain(id)) => self.done_for_you_never(&id),
            Some(FeedAct::Retry(id)) => self.crash_retry(&id),
            Some(FeedAct::DeleteRecording(id)) => self.arm_delete_recording(&id),
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
            .map(|c| c.title.clone())
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
            .map(|c| c.title.clone())
            .unwrap_or_default();
        self.persist_updates();
        let route = route_schedule(&seed);
        let (depth, did) = if route.is_some() {
            (grokhub_core::UseDepth::Ran, grokhub_core::UseAction::Automated)
        } else {
            (grokhub_core::UseDepth::Opened, grokhub_core::UseAction::OpenedAutomations)
        };
        if let Some(card) = self.updates.iter().find(|c| c.id == id).cloned() {
            self.log_card_use(&card, depth, did);
        }
        if let Some(route) = route {
            let _ = self.commit_schedule(route, grokhub_agent::harness::Origin::SelfManage, "from an Automate offer");
        }
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
            if !self.heartbeat_may(grokhub_core::ProactiveAct::Ideas, now) {
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
            let _origin = grokhub_agent::harness::OriginScope::enter(grokhub_agent::harness::Origin::Proactive);
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
                Some(UpdateAction::OpenSession { .. }) | None => grokhub_core::UseAction::OpenedChat,
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

    /// Opened, built, discussed, or used before: an X then is not a "no".
    fn card_touched(&self, card: &UpdateCard) -> bool {
        card.status == UpdateStatus::Opened
            || card.built
            || card.discuss_thread.is_some()
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

    /// × on the main window's deck. A feed card leaves that deck only and stays
    /// in Pulse. An idea leaves the deck and stays on the Ideas board.
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
}

#[derive(Clone, Debug)]
struct SlideHit {
    id: String,
    /// Deck slot, read by tests to find the front card.
    #[cfg(test)]
    index: usize,
    rect: egui::Rect,
}

#[derive(Clone, Debug, Default)]
struct FeedStackMem {
    expanded: bool,
    hits: Vec<SlideHit>,
}

/// Cards still on the main window's deck. × there hides a card from it only.
fn on_home(cards: &[UpdateCard]) -> Vec<UpdateCard> {
    cards.iter().filter(|card| !card.pulse.off_home).cloned().collect()
}

fn home_stack_cards(cards: &[UpdateCard], pulse: &grokhub_core::FeedPulse, now: u64) -> Vec<UpdateCard> {
    let cards = on_home(cards);
    let mut out = grokhub_core::home_event_cards(&cards, pulse, now);
    out.extend(feed_ideas(&cards, now));
    out.extend(visible_digests(&cards).into_iter().take(DIGEST_PAINT_MAX));
    out.truncate(HOME_STACK_SHOW);
    out
}

pub(super) struct HomeDeck {
    /// At most `HOME_STACK_SHOW`, front first.
    pub cards: Vec<UpdateCard>,
    /// Ranked cards waiting for a slot. One joins each time a card leaves.
    pub waiting: usize,
    pub rank: grokhub_core::HomeRank,
}

pub(super) fn home_deck(
    cards: &[UpdateCard],
    pulse: &grokhub_core::FeedPulse,
    prefs: &grokhub_core::CardPrefs,
    now: u64,
) -> HomeDeck {
    let cards = on_home(cards);
    let rank = grokhub_core::rank_home_events(&cards, pulse, prefs, now);
    let mut shown = rank.deck.clone();
    shown.extend(feed_ideas(&cards, now));
    shown.extend(visible_digests(&cards).into_iter().take(DIGEST_PAINT_MAX));
    let waiting = rank.waiting + shown.len().saturating_sub(HOME_STACK_SHOW);
    shown.truncate(HOME_STACK_SHOW);
    HomeDeck {
        cards: shown,
        waiting,
        rank,
    }
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

fn fly_id(card_id: &str) -> egui::Id {
    egui::Id::new(("home-feed-fly", card_id))
}

fn settle_id(card_id: &str) -> egui::Id {
    egui::Id::new(("home-feed-settle", card_id))
}

/// When the front card id changes (× promote), arm a short settle so the new
/// front eases in instead of jump-cutting. Newcomers joining the back do not
/// change the front id, so they keep the fly-in only.
fn arm_front_settle(ctx: &egui::Context, cards: &[UpdateCard]) {
    let key = egui::Id::new("home-deck-front");
    let prev: Option<String> = ctx.data(|d| d.get_temp(key));
    let Some(front) = cards.first() else {
        return;
    };
    // Only when the front *changes* (× promote). First paint must not settle-offset.
    if let Some(prev) = prev.as_deref() {
        if prev != front.id.as_str() && !crate::motion::reduced_motion_ctx(ctx) {
            ctx.animate_value_with_time(settle_id(&front.id), 0.0, 0.0);
        }
    }
    ctx.data_mut(|d| d.insert_temp(key, front.id.clone()));
}

/// A card that was not on the deck last frame flies in. The first deck painted
/// after launch just appears.
fn fly_in_newcomers(ctx: &egui::Context, cards: &[UpdateCard]) {
    let seen_id = egui::Id::new("home-deck-seen");
    let seen: Option<Vec<String>> = ctx.data(|d| d.get_temp(seen_id));
    if let Some(seen) = seen {
        for card in cards.iter().filter(|card| !seen.contains(&card.id)) {
            ctx.animate_value_with_time(fly_id(&card.id), 0.0, 0.0);
        }
    }
    let ids: Vec<String> = cards.iter().map(|card| card.id.clone()).collect();
    ctx.data_mut(|d| d.insert_temp(seen_id, ids));
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
    opacity: f32,
    clip: Option<egui::Rect>,
) -> Option<FeedAct> {
    let mut act = None;
    ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
        if let Some(clip) = clip {
            ui.set_clip_rect(clip);
        }
        ui.multiply_opacity(opacity);
        ui.set_min_size(rect.size());
        ui.set_width(rect.width());
        ui.spacing_mut().item_spacing = egui::vec2(8.0, 4.0);
        act = paint_feed_card(ui, card, rect.width(), hint);
    });
    act
}

fn paint_tucked_edge(ui: &mut egui::Ui, rect: egui::Rect, cover: egui::Rect, opacity: f32) {
    let visible = rect.intersect(egui::Rect::from_min_max(
        egui::pos2(rect.left(), cover.bottom() - 1.0),
        rect.right_bottom(),
    ));
    if visible.height() < 1.0 {
        return;
    }
    let mut painter = ui.painter().with_clip_rect(visible);
    painter.multiply_opacity(opacity);
    painter.rect(
        rect,
        crate::theme::CARD_RADIUS,
        crate::theme::elevated(),
        egui::Stroke::new(1.0_f32, crate::theme::border()),
        egui::StrokeKind::Middle,
    );
}

impl Cabin {
/// Paints the deck back to front. Each card eases toward its pose; one that
/// just joined slides in and fades up. `total` counts the cards waiting too.
/// Reduced motion snaps the slide. A digest or suggestion that is open paints
/// the Pulse card and grows up so the strip's bottom stays put.
#[allow(clippy::too_many_arguments)]
fn paint_slide_deck(
    &mut self,
    ui: &mut egui::Ui,
    cards: &[UpdateCard],
    hints: &[Option<String>],
    front: egui::Pos2,
    width: f32,
    poses: &[SlidePose],
    total: usize,
    expanded: bool,
    lifted: Option<usize>,
    room: f32,
    now_at: u64,
) -> Option<FeedAct> {
    let ctx = ui.ctx().clone();
    let motion = !crate::motion::reduced_motion_ctx(&ctx);
    let slide_secs = if motion { SLIDE_SECS } else { 0.0 };
    let fly_secs = if motion { FLY_SECS } else { 0.0 };
    let settle_secs = if motion { SETTLE_SECS } else { 0.0 };
    let lift = chrome_lift(cards, expanded, lifted, room);
    let placed: Vec<(egui::Rect, SlidePose, f32, f32, bool)> = cards
        .iter()
        .zip(poses)
        .enumerate()
        .map(|(index, (card, pose))| {
            let mut dy = ctx.animate_value_with_time(
                egui::Id::new(("home-feed-dy", card.id.as_str())),
                card_dy(index, pose.dy, &lift, room),
                slide_secs,
            );
            let scale = ctx.animate_value_with_time(
                egui::Id::new(("home-feed-scale", card.id.as_str())),
                pose.scale,
                slide_secs,
            );
            let fly = ctx.animate_value_with_time(fly_id(&card.id), 1.0, fly_secs);
            let settle = ctx.animate_value_with_time(settle_id(&card.id), 1.0, settle_secs);
            let settle_t = if motion {
                egui::emath::easing::quadratic_out(settle.clamp(0.0, 1.0))
            } else {
                1.0
            };
            if index == 0 && motion {
                // Promoted front: ease up the last couple of pixels (CD-01).
                dy += SETTLE_DY * (1.0 - settle_t);
            }
            let away = 1.0 - egui::emath::easing::quadratic_out(fly);
            let now = SlidePose { dy, scale };
            let mut rect =
                slide_rect(front, width, now).translate(egui::vec2(FLY_DX, FLY_DY) * away);
            let full = lift.index == Some(index);
            if full {
                rect = full_card_rect(rect, FEED_CARD_H + lift.extra);
            }
            let opacity = if motion {
                fly * (0.72 + 0.28 * settle_t)
            } else {
                fly
            };
            (rect, now, opacity, settle_t, full)
        })
        .collect();
    let &(cover, _, _, _, _) = placed.first()?;
    let mut act = None;
    let mut popped_full = None;
    for index in (0..placed.len()).rev() {
        let (rect, pose, opacity, _, full) = placed[index];
        if full && lifted == Some(index) {
            popped_full = Some(index);
            continue;
        }
        if pose.dy < -1.0 {
            paint_stack_shadow(ui.painter(), rect);
        }
        let card_act = if index > 0 && pose.dy > -PEEK_H * 0.5 {
            paint_tucked_edge(ui, rect, cover, opacity);
            None
        } else if full {
            self.paint_full_feed_card(ui, &cards[index], rect, now_at, opacity)
        } else {
            // CD-02: clip each open peek to the strip above the card in front so
            // that card's own title row is what shows in PEEK_H.
            let clip = if index > 0 {
                let front_of = placed[index - 1].0.top();
                Some(egui::Rect::from_min_max(
                    egui::pos2(rect.left(), rect.top()),
                    egui::pos2(rect.right(), front_of.max(rect.top())),
                ))
            } else {
                None
            };
            paint_card_at(
                ui,
                &cards[index],
                rect,
                hints.get(index).and_then(|hint| hint.as_deref()),
                opacity,
                clip,
            )
        };
        if card_act.is_some() {
            act = card_act;
        }
    }
    if let Some(index) = popped_full {
        let (rect, pose, opacity, _, _) = placed[index];
        if pose.dy < -1.0 {
            paint_stack_shadow(ui.painter(), rect);
        }
        let card_act = self.paint_full_feed_card(ui, &cards[index], rect, now_at, opacity);
        if card_act.is_some() {
            act = card_act;
        }
    }
    if total > 1 {
        let badge = egui::Rect::from_min_size(
            egui::pos2(cover.right() - 36.0, cover.bottom() - 8.0),
            egui::vec2(28.0, 16.0),
        );
        paint_count_badge(ui.painter(), badge, total);
    }
    act
}

/// Pulse card on the deck: source · age, title, short takeaway, image, Liked, Discuss.
/// × is deck-only. The bottom of `rect` is the compact strip; the card grows up.
fn paint_full_feed_card(
    &mut self,
    ui: &mut egui::Ui,
    card: &UpdateCard,
    rect: egui::Rect,
    now_at: u64,
    opacity: f32,
) -> Option<FeedAct> {
    let ctx = ui.ctx().clone();
    let thumbs: Vec<super::pulse_ui::Thumb> = card
        .pulse
        .image_urls
        .iter()
        .take(3)
        .filter_map(|url| self.pulse_thumb(&ctx, url))
        .collect();
    let mut act = None;
    ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
        ui.multiply_opacity(opacity);
        ui.set_min_size(rect.size());
        ui.set_width(rect.width());
        // Registered first so Like, Discuss, and × keep the click. Tests read this rect.
        let _hover = ui.interact(
            rect,
            egui::Id::new(("feed-card", &card.id)),
            egui::Sense::hover(),
        );
        ui.painter().rect(
            rect,
            crate::theme::CARD_RADIUS,
            crate::theme::elevated(),
            egui::Stroke::new(1.0_f32, crate::theme::border()),
            egui::StrokeKind::Middle,
        );
        paint_done_accent(ui.painter(), card, rect);
        let x_rect = egui::Rect::from_min_size(
            egui::pos2(rect.right() - 32.0, rect.top() + 8.0),
            egui::vec2(24.0, 24.0),
        );
        let inner = egui::Rect::from_min_max(
            egui::pos2(rect.left() + 12.0, rect.top() + 8.0),
            egui::pos2(rect.right() - 12.0, rect.bottom() - 8.0),
        );
        ui.scope_builder(egui::UiBuilder::new().max_rect(inner), |ui| {
            ui.set_width(inner.width());
            ui.spacing_mut().item_spacing.y = 4.0;
            ui.horizontal_top(|ui| {
                super::pulse_ui::paint_feed_icon(ui, card);
                ui.add_space(10.0);
                ui.vertical(|ui| {
                    ui.set_width(ui.available_width());
                    ui.spacing_mut().item_spacing.y = 4.0;
                    ui.label(
                        RichText::new(super::pulse_ui::post_age_line(card, now_at))
                            .size(crate::theme::FONT_TIP)
                            .color(crate::theme::subtle()),
                    );
                    if let Some(did) = super::pulse_ui::paint_post_body(ui, card, &thumbs) {
                        act = Some(match did {
                            super::pulse_ui::FeedPostAct::Like => FeedAct::Like(card.id.clone()),
                            super::pulse_ui::FeedPostAct::Discuss => {
                                FeedAct::Discuss(card.id.clone())
                            }
                            super::pulse_ui::FeedPostAct::Link(url) => FeedAct::Link(url),
                            super::pulse_ui::FeedPostAct::Undo => FeedAct::Undo(card.id.clone()),
                            super::pulse_ui::FeedPostAct::NeverAgain => FeedAct::NeverAgain(card.id.clone()),
                            super::pulse_ui::FeedPostAct::Open => FeedAct::Open(card.id.clone()),
                            super::pulse_ui::FeedPostAct::Retry => FeedAct::Retry(card.id.clone()),
                            super::pulse_ui::FeedPostAct::DeleteRecording => {
                                FeedAct::DeleteRecording(card.id.clone())
                            }
                        });
                    }
                });
            });
        });
        let x = ui.put(
            x_rect,
            egui::Button::new(RichText::new("×").size(16.0).color(crate::theme::muted())).frame(false),
        );
        if x.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        if x.clicked() {
            act = Some(FeedAct::Dismiss(card.id.clone()));
        }
    });
    act
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
    paint_done_accent(ui.painter(), card, rect);
    let x_rect = egui::Rect::from_min_size(
        egui::pos2(rect.right() - 32.0, rect.top() + 6.0),
        egui::vec2(24.0, 24.0),
    );
    let text_right = x_rect.left() - 4.0;
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
        // Digest and suggestion strips use the same short takeaway as Pulse,
        // so an old long body does not dump onto the tucked card.
        if matches!(card.kind, UpdateKind::Digest | UpdateKind::Suggestion) {
            let takeaway = grokhub_core::short_takeaway(card);
            if !takeaway.is_empty() {
                ui.label(
                    RichText::new(takeaway)
                        .size(crate::theme::FONT_TIP)
                        .color(crate::theme::muted()),
                );
            }
            if let Some(hint) = hint.map(str::trim).filter(|text| !text.is_empty()) {
                ui.label(
                    RichText::new(hint)
                        .size(crate::theme::FONT_TIP)
                        .color(crate::theme::muted()),
                );
            }
        } else {
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
        }
    });
    // Registered last so the title labels do not take the click.
    let hit = ui.interact(
        rect,
        egui::Id::new(("feed-card", &card.id)),
        egui::Sense::click(),
    );
    let over_x = hit.hover_pos().is_some_and(|pos| x_rect.contains(pos));
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
    if !hit.clicked() {
        return None;
    }
    let on_x = hit
        .interact_pointer_pos()
        .is_some_and(|pos| x_rect.contains(pos));
    Some(if on_x {
        FeedAct::Dismiss(card.id.clone())
    } else {
        match card.kind {
            UpdateKind::Digest | UpdateKind::Suggestion | UpdateKind::Idea => {
                FeedAct::Discuss(card.id.clone())
            }
            UpdateKind::AutomateOffer
            | UpdateKind::AutomationDone
            | UpdateKind::ScheduleCreated
            | UpdateKind::SelfChange
            | UpdateKind::DoneForYou => FeedAct::Open(card.id.clone()),
        }
    })
}

#[cfg(test)]
mod stack_tests {
    use super::{
        collapsed_stack_h, deck_poses, next_feed_stack, rest_slide, stacked_feed_h, SlidePose,
        StackView, FEED_CARD_H, HOME_STACK_SHOW, LIFT_STEP, PEEK_H, STACK_REST_DY_1,
        STACK_REST_DY_2, STACK_REST_SCALE_1, STACK_REST_SCALE_2,
    };

    fn dys(poses: &[SlidePose]) -> Vec<f32> {
        poses.iter().map(|pose| pose.dy).collect()
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
    fn a_single_card_never_moves() {
        let still = vec![SlidePose { dy: 0.0, scale: 1.0 }];
        assert_eq!(deck_poses(1, true, None, 500.0), still);
        assert_eq!(deck_poses(1, true, Some(0), 500.0), still);
        assert_eq!(deck_poses(1, false, None, 500.0), still);
        assert_eq!(next_feed_stack(1, true, Some("only".into()), Some("only")), StackView::default());
    }

    #[test]
    fn open_deck_peeks_titles_and_lifts_only_the_hovered_card() {
        assert_eq!(PEEK_H, 34.0);
        assert_eq!(LIFT_STEP, 102.0);
        let rest = deck_poses(3, false, None, 500.0);
        assert_eq!(rest[1], SlidePose { dy: STACK_REST_DY_1, scale: STACK_REST_SCALE_1 });
        assert_eq!(rest[2], SlidePose { dy: STACK_REST_DY_2, scale: STACK_REST_SCALE_2 });
        assert_eq!(rest_slide(7), rest[2]);

        assert_eq!(dys(&deck_poses(3, true, None, 500.0)), vec![0.0, -PEEK_H, -2.0 * PEEK_H]);
        assert!(deck_poses(3, true, None, 500.0).iter().all(|pose| pose.scale == 1.0));
        assert_eq!(dys(&deck_poses(3, true, Some(1), 500.0)), vec![0.0, -LIFT_STEP, -LIFT_STEP - PEEK_H]);
        assert_eq!(dys(&deck_poses(3, true, Some(2), 500.0)), vec![0.0, -PEEK_H, -LIFT_STEP - PEEK_H]);
        // The front card never lifts.
        assert_eq!(dys(&deck_poses(3, true, Some(0), 500.0)), vec![0.0, -PEEK_H, -2.0 * PEEK_H]);
        // Near the top of the window nothing goes above the room left.
        assert_eq!(dys(&deck_poses(3, true, Some(2), 50.0)), vec![0.0, -PEEK_H, -50.0]);
        assert_eq!(dys(&deck_poses(2, true, None, -10.0)), vec![0.0, 0.0]);
    }

    #[test]
    fn hover_state_lifts_a_card_behind_and_closes_off_the_deck() {
        assert_eq!(
            next_feed_stack(3, true, Some("back".into()), Some("front")),
            StackView {
                expanded: true,
                popped: Some("back".into()),
            }
        );
        assert_eq!(
            next_feed_stack(3, true, Some("front".into()), Some("front")),
            StackView {
                expanded: true,
                popped: None,
            }
        );
        assert_eq!(
            next_feed_stack(3, false, Some("back".into()), Some("front")),
            StackView::default()
        );
    }
}

#[cfg(test)]
mod deck_hover_tests {
    use super::*;

    const COMPOSER_BOTTOM: f32 = 548.0;
    const STACK_TOP: f32 = 571.0;
    const AWAY: egui::Pos2 = egui::pos2(950.0, 60.0);

    #[derive(Default)]
    struct Frames {
        /// The deck's open flag after each frame.
        expanded: Vec<bool>,
        /// Painted top of the front card after each frame.
        front_top: Vec<f32>,
        /// Card ids front to back after each frame.
        order: Vec<Vec<String>>,
        /// Settled rect of each card, front to back, after each frame.
        rects: Vec<Vec<egui::Rect>>,
    }

    /// Empty-home layout without sign-in: composer, a gap, then the deck,
    /// as `ui_empty_home` places them. One frame per pointer position at 60 fps,
    /// counting on from `start` so one context can play several paths.
    fn play(ctx: &egui::Context, start: usize, cabin: &mut Cabin, path: &[egui::Pos2]) -> Frames {
        crate::theme::install_fonts(ctx);
        let mut frames = Frames::default();
        for (step, pos) in path.iter().enumerate() {
            let frame = start + step;
            let raw = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1000.0, 900.0),
                )),
                time: Some(frame as f64 / 60.0),
                predicted_dt: 1.0 / 60.0,
                events: vec![egui::Event::PointerMoved(*pos)],
                ..Default::default()
            };
            let _ = crate::theme::test_pass(ctx, raw, |ui| {
                egui::CentralPanel::default().show(ui, |ui| {
                    let pane_w = 600.0;
                    ui.add_space(380.0);
                    ui.allocate_exact_size(egui::vec2(pane_w, 160.0), egui::Sense::hover());
                    ui.add_space(20.0);
                    cabin.paint_update_feed(ui, pane_w);
                    cabin.paint_home_deck_over_chat(ui);
                });
            });
            let mem: FeedStackMem = ctx
                .data(|d| d.get_temp(egui::Id::new("home-feed-stack")))
                .unwrap_or_default();
            frames.expanded.push(mem.expanded);
            let mut by_index = mem.hits.clone();
            by_index.sort_by_key(|hit| hit.index);
            let front = by_index.first().map(|hit| hit.id.clone()).unwrap_or_default();
            frames.front_top.push(painted_top(ctx, &front));
            frames.rects.push(by_index.iter().map(|hit| hit.rect).collect());
            frames
                .order
                .push(by_index.into_iter().map(|hit| hit.id).collect());
        }
        frames
    }

    fn painted_top(ctx: &egui::Context, id: &str) -> f32 {
        painted(ctx, id).map(|rect| rect.top()).unwrap_or(f32::NAN)
    }

    fn painted(ctx: &egui::Context, id: &str) -> Option<egui::Rect> {
        ctx.read_response(egui::Id::new(("feed-card", id)))
            .map(|r| r.rect)
    }

    fn flips(v: &[bool]) -> usize {
        v.windows(2).filter(|w| w[0] != w[1]).count()
    }

    fn max_step(v: &[f32]) -> f32 {
        v.windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .filter(|d| d.is_finite())
            .fold(0.0, f32::max)
    }

    fn run_cards(n: u64) -> (std::path::PathBuf, Cabin) {
        let root = crate::config::test_config_root("deck-hover");
        let _ = std::fs::remove_dir_all(&root);
        std::env::set_var("GROKHUB_CONFIG", &root);
        let mut cabin = Cabin::quiet_for_test();
        let now = now_ms();
        for i in 0..n {
            cabin.updates.push(automation_done_card(
                &format!("job-{i}"),
                &format!("Backup run {i}"),
                "ok",
                now - i * 1_000,
            ));
        }
        (root, cabin)
    }

    /// v2.10.85: a still pointer in the gap between the composer and the deck
    /// flipped the deck open and shut 98 times in 120 frames, and the front
    /// card jumped about 270 px a frame between the pile and the fan.
    #[test]
    fn still_pointer_between_composer_and_deck_does_not_flicker() {
        let _g = crate::config::hold_test_config();
        let (root, mut cabin) = run_cards(3);
        let gap_y = 560.0;
        assert!(gap_y > COMPOSER_BOTTOM && gap_y < STACK_TOP);
        let mut path = vec![egui::pos2(500.0, 620.0); 60];
        path.extend(vec![egui::pos2(500.0, gap_y); 120]);
        let frames = play(&egui::Context::default(), 0, &mut cabin, &path);
        assert_eq!(flips(&frames.expanded[60..]), 0, "deck flips with a still pointer");
        assert!(frames.expanded[179], "the deck stays open in the gap");
        let step = max_step(&frames.front_top);
        assert!(step < 20.0, "front card jumped {step} px in one frame");
        let first = frames.order[0].clone();
        assert_eq!(first.len(), 3);
        assert!(frames.order.iter().all(|order| *order == first), "cards reordered");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The deck no longer jumps above the composer: one card stays put, and
    /// with several only the card under the pointer rises, just enough to read.
    #[test]
    fn one_card_stays_put_under_the_pointer() {
        let _g = crate::config::hold_test_config();
        let (root, mut cabin) = run_cards(1);
        let mut path = vec![AWAY; 10];
        path.extend(vec![egui::pos2(500.0, 620.0); 60]);
        let frames = play(&egui::Context::default(), 0, &mut cabin, &path);
        assert!(frames.expanded.iter().all(|open| !open), "a deck of one never opens");
        let rest = frames.front_top[0];
        assert!((rest - STACK_TOP).abs() < 1.0, "front card rests at {rest}");
        assert!(
            frames.front_top.iter().all(|top| (top - rest).abs() < 0.01),
            "the single card moved: {:?}",
            frames.front_top
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn hover_lifts_only_the_card_under_the_pointer() {
        let _g = crate::config::hold_test_config();
        let (root, mut cabin) = run_cards(3);
        let ctx = egui::Context::default();
        // On the front card: titles behind it peek, the front stays where it was.
        let pile = play(&ctx, 0, &mut cabin, &vec![egui::pos2(500.0, 620.0); 40]);
        assert!(pile.expanded[39]);
        let front = pile.front_top[0];
        assert!((front - STACK_TOP).abs() < 1.0, "front card rests at {front}");
        assert!(pile.front_top.iter().all(|top| (top - front).abs() < 0.01));
        let tops: Vec<f32> = pile.rects[39].iter().map(|rect| rect.top() - front).collect();
        assert_eq!(tops, vec![0.0, -PEEK_H, -2.0 * PEEK_H]);

        // On the second card's title strip: that card alone rises clear of the front.
        let second = play(&ctx, 40, &mut cabin, &vec![egui::pos2(500.0, front - 15.0); 40]);
        let tops: Vec<f32> = second.rects[39].iter().map(|rect| rect.top() - front).collect();
        assert_eq!(tops, vec![0.0, -LIFT_STEP, -LIFT_STEP - PEEK_H]);
        assert_eq!(flips(&second.expanded), 0);
        assert!(second.front_top.iter().all(|top| (top - front).abs() < 0.01));
        let back = &second.order[39][1];
        let lifted = painted(&ctx, back).expect("lifted card painted");
        assert!((lifted.top() - (front - LIFT_STEP)).abs() < 0.5, "{lifted:?}");

        // Up to the third card's strip: it lifts and the second drops back to a peek.
        let third_y = front - LIFT_STEP - 15.0;
        let third = play(&ctx, 80, &mut cabin, &vec![egui::pos2(500.0, third_y); 40]);
        let tops: Vec<f32> = third.rects[39].iter().map(|rect| rect.top() - front).collect();
        assert_eq!(tops, vec![0.0, -PEEK_H, -PEEK_H - LIFT_STEP]);
        assert_eq!(flips(&third.expanded), 0);

        // Off the deck it closes once and stays closed, back in the pile.
        let away = play(&ctx, 120, &mut cabin, &[AWAY; 60]);
        assert_eq!(flips(&away.expanded), 0);
        assert!(!away.expanded[59]);
        let tops: Vec<f32> = away.rects[59].iter().map(|rect| rect.top() - front).collect();
        assert_eq!(tops[0], 0.0);
        assert!(tops[1] > 0.0 && tops[2] > tops[1], "{tops:?}");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// × on the main deck keeps the card in Pulse, and the next waiting card
    /// slides into the deck, which never holds more than three.
    #[test]
    fn closing_a_card_flies_the_next_one_in() {
        let _g = crate::config::hold_test_config();
        let (root, mut cabin) = run_cards(5);
        let ctx = egui::Context::default();
        let on_pile = egui::pos2(500.0, 620.0);
        let before = play(&ctx, 0, &mut cabin, &[on_pile; 40]);
        let shown = before.order[39].clone();
        assert_eq!(shown.len(), 3, "the deck holds three");
        let deck = home_deck(&cabin.updates, &cabin.cfg.feed_pulse, &cabin.card_prefs, now_ms());
        assert_eq!(deck.waiting, 2);

        let gone = shown[0].clone();
        cabin.close_home_card(&gone);
        let card = cabin.updates.iter().find(|c| c.id == gone).expect("card kept");
        assert!(card.pulse.off_home);
        assert_ne!(card.status, UpdateStatus::Dismissed);
        assert!(grokhub_core::pulse::pulse_visible(card, now_ms()), "Pulse still shows it");

        // It starts off to the side and settles into its slot.
        let first = play(&ctx, 40, &mut cabin, &[on_pile]);
        let now_shown = first.order[0].clone();
        assert_eq!(now_shown.len(), 3);
        assert!(!now_shown.contains(&gone));
        let joined: Vec<String> = now_shown.iter().filter(|id| !shown.contains(id)).cloned().collect();
        assert_eq!(joined.len(), 1, "one waiting card joins: {now_shown:?}");
        let deck = home_deck(&cabin.updates, &cabin.cfg.feed_pulse, &cabin.card_prefs, now_ms());
        assert_eq!(deck.waiting, 1);
        let at = now_shown.iter().position(|id| *id == joined[0]).unwrap();
        let slot = first.rects[0][at];
        let flying = painted(&ctx, &joined[0]).expect("newcomer painted");
        assert!(flying.left() > slot.left() + 20.0, "{flying:?} vs {slot:?}");
        assert!(flying.top() > slot.top() + 10.0, "{flying:?} vs {slot:?}");
        let _ = play(&ctx, 41, &mut cabin, &vec![on_pile; 40]);
        let landed = painted(&ctx, &joined[0]).expect("newcomer painted");
        assert!((landed.left() - slot.left()).abs() < 0.5, "{landed:?} vs {slot:?}");
        assert!((landed.top() - slot.top()).abs() < 0.5, "{landed:?} vs {slot:?}");
        let _ = std::fs::remove_dir_all(&root);
    }

    fn shape_text(shape: &egui::Shape, out: &mut Vec<String>) {
        match shape {
            egui::Shape::Text(text) => {
                let line = text.galley.job.text.trim().to_string();
                if !line.is_empty() {
                    out.push(line);
                }
            }
            egui::Shape::Vec(shapes) => {
                for shape in shapes {
                    shape_text(shape, out);
                }
            }
            _ => {}
        }
    }

    /// Same layout as `play`, plus the text painted on the last frame.
    fn last_texts(ctx: &egui::Context, start: usize, cabin: &mut Cabin, path: &[egui::Pos2]) -> Vec<String> {
        crate::theme::install_fonts(ctx);
        let mut texts = Vec::new();
        for (step, pos) in path.iter().enumerate() {
            let frame = start + step;
            let raw = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1000.0, 900.0),
                )),
                time: Some(frame as f64 / 60.0),
                predicted_dt: 1.0 / 60.0,
                events: vec![egui::Event::PointerMoved(*pos)],
                ..Default::default()
            };
            texts.clear();
            let out = crate::theme::test_pass(ctx, raw, |ui| {
                egui::CentralPanel::default().show(ui, |ui| {
                    let pane_w = 600.0;
                    ui.add_space(380.0);
                    ui.allocate_exact_size(egui::vec2(pane_w, 160.0), egui::Sense::hover());
                    ui.add_space(20.0);
                    cabin.paint_update_feed(ui, pane_w);
                    cabin.paint_home_deck_over_chat(ui);
                });
            });
            for clipped in &out.shapes {
                shape_text(&clipped.shape, &mut texts);
            }
        }
        texts
    }

    /// Hover opens the front digest into the Pulse card: source, short takeaway,
    /// Liked, Discuss, and ×. The long dump stays out of both the open card and
    /// the compact card behind it. Discuss uses the main-chat handoff.
    #[test]
    fn expanded_deck_paints_the_pulse_card() {
        let _g = crate::config::hold_test_config();
        let root = crate::config::test_config_root("deck-pulse-card");
        let _ = std::fs::remove_dir_all(&root);
        std::env::set_var("GROKHUB_CONFIG", &root);
        let mut cabin = Cabin::quiet_for_test();
        let now = now_ms();
        let long = "I'll look up two real pieces that fit your Linux work. \
Grok Build alpha 1.0.50 changes how a cancel hits a live turn. \
Streams are retried instead of partial output. ZEPHYRTAIL";
        let mut front = grokhub_core::digest_card("xstack", "For you", long, now - 38 * 60 * 1000);
        front.citations = vec!["https://xstack.grok.me/post".into()];
        front.pulse.source_name = Some("xstack.grok.me".into());
        front.reaction = Some(grokhub_core::CardReaction::Up);
        front.why = Some("It changes the cancel button you use.".into());
        let front_id = front.id.clone();
        let back = grokhub_core::digest_card(
            "other",
            "Also noted",
            "A second note about the weather. The sky stays clear. ZEPHYRTAIL should stay off this strip.",
            now - 40 * 60 * 1000,
        );
        let back_id = back.id.clone();
        cabin.updates.push(back);
        cabin.updates.push(front);

        let ctx = egui::Context::default();
        let on_pile = egui::pos2(500.0, 620.0);
        let texts = last_texts(&ctx, 0, &mut cabin, &vec![on_pile; 40]);
        let blob = texts.join("\n");
        for want in [
            "For you",
            "xstack.grok.me · 38m ago",
            "cancel hits a live turn",
            "It changes the cancel button you use.",
            "Read at xstack.grok.me",
            "Liked",
            "Discuss",
            "×",
            "A second note about the weather",
        ] {
            assert!(blob.contains(want), "missing {want:?} in {blob}");
        }
        assert!(!blob.contains("ZEPHYRTAIL"), "dump leaked into the deck: {blob}");
        assert!(!blob.contains("two real pieces"), "lead-in leaked into the deck: {blob}");
        assert!(
            !blob.contains("Streams are retried"),
            "third sentence leaked: {blob}"
        );
        let rect = painted(&ctx, &front_id).expect("front card painted");
        let extra = FULL_FEED_H - FEED_CARD_H;
        assert!(
            (rect.height() - FULL_FEED_H).abs() < 1.0,
            "open card is the Pulse height, got {rect:?}"
        );
        assert!(
            (rect.bottom() - (STACK_TOP + FEED_CARD_H)).abs() < 1.5,
            "open card grows up; the strip bottom stays, got {rect:?}"
        );
        assert!(
            (rect.top() - (STACK_TOP - extra)).abs() < 1.5,
            "open card top, got {rect:?}"
        );

        // The card behind, under the pointer, opens into the same chrome.
        let peek = egui::pos2(500.0, rect.top() - 12.0);
        let popped = last_texts(&ctx, 40, &mut cabin, &vec![peek; 40]);
        let popped_blob = popped.join("\n");
        assert!(popped_blob.contains("Also noted"), "{popped_blob}");
        assert!(popped_blob.contains("Discuss"), "{popped_blob}");
        assert!(!popped_blob.contains("ZEPHYRTAIL"), "{popped_blob}");
        let back_rect = painted(&ctx, &back_id).expect("back card painted");
        assert!(
            (back_rect.height() - FULL_FEED_H).abs() < 1.0,
            "popped card is the Pulse height, got {back_rect:?}"
        );

        cabin.close_home_card(&front_id);
        let kept = cabin.updates.iter().find(|c| c.id == front_id).expect("kept");
        assert!(kept.pulse.off_home, "× leaves Pulse");
        assert_ne!(kept.status, UpdateStatus::Dismissed);

        cabin.apply_feed_act(Some(FeedAct::Discuss(back_id.clone())));
        assert_eq!(cabin.nav, Nav::Chat);
        assert!(cabin.composer_want_focus);
        let thread = cabin
            .threads
            .iter()
            .find(|t| t.title == "Discuss · Also noted")
            .expect("deck discuss opens chat");
        assert!(!thread.background);
        assert!(thread.title_locked);
        let seed = thread
            .messages
            .iter()
            .find(|m| m.0 == "assistant")
            .map(|m| m.1.as_str())
            .unwrap_or("");
        assert!(seed.contains("Also noted"), "{seed}");
        assert!(seed.contains("second note about the weather"), "{seed}");

        let src = include_str!("feed_ui.rs");
        let full = src
            .split("fn paint_full_feed_card(")
            .nth(1)
            .and_then(|rest| rest.split("fn paint_feed_card(").next())
            .expect("paint_full_feed_card");
        assert!(full.contains("post_age_line"), "{full}");
        assert!(full.contains("paint_post_body"), "{full}");
        assert!(full.contains("FeedAct::Discuss") && full.contains("FeedAct::Like"));
        assert!(full.contains("FeedAct::Dismiss") && full.contains("\"×\""));
        let pulse = include_str!("pulse_ui.rs");
        let body = pulse
            .split("fn paint_post_body(")
            .nth(1)
            .and_then(|rest| rest.split("fn post_source(").next())
            .expect("paint_post_body");
        assert!(body.contains("short_takeaway"), "{body}");
        assert!(body.contains("Read at"), "{body}");
        assert!(body.contains("\"Discuss\""), "{body}");

        let _ = std::fs::remove_dir_all(&root);
    }

    /// Reduced motion snaps the open deck in one frame. Automation cards stay
    /// the compact strip, so this does not depend on the Pulse card height.
    #[test]
    fn reduced_motion_snaps_the_open_deck() {
        let _g = crate::config::hold_test_config();
        let (root, mut cabin) = run_cards(3);
        let ctx = egui::Context::default();
        let _ = play(&ctx, 0, &mut cabin, &[AWAY; 5]);
        ctx.all_styles_mut(|style| style.animation_time = 0.0);
        let snap = play(&ctx, 5, &mut cabin, &[egui::pos2(500.0, 620.0)]);
        assert!(snap.expanded[0], "one frame opens the deck");
        let front = snap.front_top[0];
        assert!((front - STACK_TOP).abs() < 1.0, "front stays put at {front}");
        let tops: Vec<f32> = snap.rects[0].iter().map(|rect| rect.top() - front).collect();
        let want = [0.0, -PEEK_H, -2.0 * PEEK_H];
        assert_eq!(tops.len(), want.len());
        for (got, exp) in tops.iter().zip(want) {
            assert!(
                (got - exp).abs() < 1.0,
                "reduced motion peek tops {tops:?} want {want:?}"
            );
        }
        // Settled hits are the target. The painted card is what has to snap.
        let second = &snap.order[0][1];
        let painted_second = painted(&ctx, second).expect("second card painted");
        assert!(
            (painted_second.top() - (front - PEEK_H)).abs() < 1.0,
            "reduced motion snaps the slide, got {painted_second:?} front {front}"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// CD-02: each open peek strip shows that card's own title, front → back.
    #[test]
    fn open_peek_strips_show_each_cards_title_front_to_back() {
        let _g = crate::config::hold_test_config();
        let (root, mut cabin) = run_cards(3);
        let ctx = egui::Context::default();
        let on_pile = egui::pos2(500.0, 620.0);
        let _ = play(&ctx, 0, &mut cabin, &[on_pile; 40]);
        let texts = last_texts(&ctx, 40, &mut cabin, &[on_pile; 20]);
        let titles: Vec<&str> = texts
            .iter()
            .filter(|line| line.contains("Backup run"))
            .map(|line| line.as_str())
            .collect();
        assert!(
            titles.iter().any(|t| t.contains("Backup run 0")),
            "front title missing in {titles:?} from {texts:?}"
        );
        assert!(
            titles.iter().any(|t| t.contains("Backup run 1")),
            "middle peek title missing in {titles:?} from {texts:?}"
        );
        assert!(
            titles.iter().any(|t| t.contains("Backup run 2")),
            "back peek title missing in {titles:?} from {texts:?}"
        );
        // Painted back→front, so titles appear back→front in the shape list.
        let i0 = titles.iter().position(|t| t.contains("Backup run 0")).unwrap();
        let i1 = titles.iter().position(|t| t.contains("Backup run 1")).unwrap();
        let i2 = titles.iter().position(|t| t.contains("Backup run 2")).unwrap();
        assert!(
            i2 < i1 && i1 < i0,
            "expected back→front paint order of titles, got idxs {i0},{i1},{i2} in {titles:?}"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// CD-01: promoted front is wired to settle, and lands after dismiss.
    #[test]
    fn promoted_front_settles_after_dismiss() {
        assert!((SETTLE_SECS - 0.15).abs() < 0.001);
        assert!((SETTLE_DY - 2.0).abs() < 0.001);
        // Wiring: settle helpers exist in this module (compile-time names).
        let _ = (arm_front_settle as fn(&egui::Context, &[UpdateCard]), settle_id as fn(&str) -> egui::Id);
        let _g = crate::config::hold_test_config();
        let (root, mut cabin) = run_cards(4);
        let ctx = egui::Context::default();
        let on_pile = egui::pos2(500.0, 620.0);
        let before = play(&ctx, 0, &mut cabin, &[on_pile; 40]);
        let shown = before.order[39].clone();
        assert_eq!(shown.len(), 3);
        let gone = shown[0].clone();
        let promoted = shown[1].clone();
        cabin.close_home_card(&gone);
        // Enough frames for slide + settle to finish.
        let after = play(&ctx, 40, &mut cabin, &[on_pile; 40]);
        assert_eq!(after.order[39][0], promoted, "promoted card becomes front");
        let slot = after.rects[39][0];
        let landed = painted(&ctx, &promoted).expect("promoted painted");
        assert!(
            (landed.top() - slot.top()).abs() < 1.0,
            "settle should land, got {landed:?} vs {slot:?}"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// CD-04: reduced motion snaps the newcomer fly to the settled slot.
    #[test]
    fn reduced_motion_snaps_newcomer_fly_in() {
        let _g = crate::config::hold_test_config();
        let (root, mut cabin) = run_cards(5);
        let ctx = egui::Context::default();
        let on_pile = egui::pos2(500.0, 620.0);
        let before = play(&ctx, 0, &mut cabin, &[on_pile; 40]);
        let shown = before.order[39].clone();
        let gone = shown[0].clone();
        cabin.close_home_card(&gone);
        ctx.all_styles_mut(|style| style.animation_time = 0.0);
        let first = play(&ctx, 40, &mut cabin, &[on_pile]);
        let now_shown = first.order[0].clone();
        let joined: Vec<String> = now_shown
            .iter()
            .filter(|id| !shown.contains(id))
            .cloned()
            .collect();
        assert_eq!(joined.len(), 1, "one waiting card joins: {now_shown:?}");
        let at = now_shown.iter().position(|id| *id == joined[0]).unwrap();
        let slot = first.rects[0][at];
        let flying = painted(&ctx, &joined[0]).expect("newcomer painted");
        assert!(
            (flying.left() - slot.left()).abs() < 0.5,
            "reduced motion must snap fly x, {flying:?} vs {slot:?}"
        );
        assert!(
            (flying.top() - slot.top()).abs() < 0.5,
            "reduced motion must snap fly y, {flying:?} vs {slot:?}"
        );
        let _ = std::fs::remove_dir_all(&root);
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
        let mut job = grokhub_core::BoardCard::new("Fix the tray icon", "Paused. This is where to resume.", "");
        job.thread_id = Some(job_thread.clone());
        let job_id = job.id.clone();
        cabin.board.push(job);
        let threads_before = cabin.threads.len();
        let card = grokhub_core::suggestion_card(&format!("pause:{job_id}"), "Paused: Fix the tray icon", "Paused 2 hours ago.", 5);
        let id = card.id.clone();
        cabin.updates.push(card);
        cabin.nav = Nav::Pulse;
        cabin.discuss_card(&id);
        assert_eq!(cabin.nav, Nav::Chat);
        assert_eq!(cabin.threads[cabin.thread_idx].id, job_thread, "the job's own chat");
        assert_eq!(cabin.threads.len(), threads_before, "no Discuss chat");
        let saved = cabin.updates.iter().find(|c| c.id == id).unwrap();
        assert_eq!((saved.status, saved.discuss_thread.as_deref()), (UpdateStatus::Opened, None));
        // A paused job with no chat opens the Workboard instead.
        let lone = grokhub_core::BoardCard::new("Sort receipts", "Paused. This is where to resume.", "");
        let lone_id = lone.id.clone();
        cabin.board.push(lone);
        let card = grokhub_core::suggestion_card(&format!("pause:{lone_id}"), "Paused: Sort receipts", "", 6);
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
        assert!(body.contains("It changes the cancel button you use."), "{body}");
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

        let mut suggestion = grokhub_core::suggestion_card("tip", "Morning build", "placeholder", 11);
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
        assert!(body.contains("Your morning build is the one that waits."), "{body}");
        assert!(!body.contains("ZEPHYRTAIL"), "{body}");
        let _ = std::fs::remove_dir_all(&root);
    }
}
