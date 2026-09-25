//! Home update feed in the slot that held the Coding / Life chip and the
//! under-bar workboard summary. The event slot is hidden when it has nothing
//! to paint. Idea and digest cards use the same store and do not take event rows.

use super::*;
use grokhub_core::{
    archive_digest, automation_done_card, card_matches, discuss_context, dismiss_idea,
    dismiss_update, feed_visible, file_idea_todo, hold_if_quiet, home_feed_n, idea_todo_title,
    links_from_research, mark_update_opened, post_update, quiet_hours_active, route_schedule,
    schedule_created_card, tick_feed_pulse, visible_digests, visible_ideas, visible_updates,
    CardReaction, CitedLink, DigestMaterial, PulseNow, TasteNote, UpdateAction, UpdateCard,
    UpdateKind, UpdateStatus, DIGEST_PAINT_MAX, FEED_PAINT_MAX, IDEA_DISCOVERY_MAX,
};

const FEED_CARD_H: f32 = 64.0;
const FEED_GAP: f32 = 6.0;

pub(super) fn stacked_feed_h(n: usize) -> f32 {
    if n == 0 {
        return 0.0;
    }
    let n = n as f32;
    n * FEED_CARD_H + (n - 1.0) * FEED_GAP
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

    fn quiet_now(&self) -> bool {
        let clock = Self::local_clock();
        quiet_hours_active(&clock.hm(), &self.cfg.quiet_start, &self.cfg.quiet_end)
    }

    /// Housekeep: idea expiry, quiet release, digest clock. Not the nightly review.
    pub(super) fn tick_feed_pulse(&mut self) {
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
        if !feed_visible(&self.updates) {
            return;
        }
        let events: Vec<UpdateCard> = visible_updates(&self.updates)
            .into_iter()
            .take(FEED_PAINT_MAX)
            .collect();
        let ideas: Vec<UpdateCard> = visible_ideas(&self.updates)
            .into_iter()
            .take(IDEA_DISCOVERY_MAX)
            .collect();
        let digests: Vec<UpdateCard> = visible_digests(&self.updates)
            .into_iter()
            .take(DIGEST_PAINT_MAX)
            .collect();
        let mut act = None;
        ui.vertical(|ui| {
            ui.set_width(pane_w);
            ui.spacing_mut().item_spacing.y = FEED_GAP;
            for card in events.iter().chain(ideas.iter()).chain(digests.iter()) {
                if let Some(next) = paint_feed_card(ui, card, pane_w, false) {
                    act = Some(next);
                }
            }
        });
        self.apply_feed_act(act);
    }

    pub(super) fn ui_ideas(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default()
            .frame(
                egui::Frame::none()
                    .fill(crate::theme::bg())
                    .inner_margin(egui::Margin::same(24.0)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("Ideas")
                            .size(crate::theme::FONT_HEADING)
                            .color(crate::theme::fg()),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if crate::cards::ghost_pill(ui, "Refresh") {
                            self.persist_updates();
                            let _ = crate::feed::save(&self.updates);
                            self.updates = crate::feed::load();
                        }
                    });
                });
                ui.add_space(8.0);
                let brief = ui.add(
                    egui::TextEdit::multiline(&mut self.brief_buf)
                        .hint_text("less crypto, more F1")
                        .desired_width(f32::INFINITY)
                        .desired_rows(2),
                );
                if brief.changed() {
                    self.cfg.digest_brief = self.brief_buf.trim().to_string();
                    self.persist_cfg();
                }
                ui.add_space(8.0);
                ui.add(
                    egui::TextEdit::singleline(&mut self.ideas_q)
                        .hint_text("Search posts")
                        .desired_width(f32::INFINITY),
                );
                ui.add_space(12.0);
                let query = self.ideas_q.clone();
                let ideas: Vec<UpdateCard> = visible_ideas(&self.updates)
                    .into_iter()
                    .filter(|c| card_matches(c, &query))
                    .collect();
                let digests: Vec<UpdateCard> = visible_digests(&self.updates)
                    .into_iter()
                    .filter(|c| card_matches(c, &query))
                    .collect();
                let archived: Vec<UpdateCard> = grokhub_core::archived_digests(&self.updates)
                    .into_iter()
                    .filter(|c| card_matches(c, &query))
                    .collect();
                let mut act = None;
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for card in ideas.iter().chain(digests.iter()).chain(archived.iter()) {
                        if let Some(next) = paint_feed_card(ui, card, ui.available_width(), true) {
                            act = Some(next);
                        }
                        ui.add_space(FEED_GAP);
                    }
                });
                self.apply_feed_act(act);
            });
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

    fn accept_automate_offer(&mut self, id: &str) {
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

    fn react_card(&mut self, id: &str, reaction: CardReaction) {
        let Some(card) = self.updates.iter_mut().find(|c| c.id == id) else {
            return;
        };
        if !matches!(card.kind, UpdateKind::Idea | UpdateKind::Digest) {
            return;
        }
        card.reaction = Some(reaction);
        self.persist_updates();
    }

    fn discuss_card(&mut self, id: &str) {
        let Some(card) = self.updates.iter().find(|c| c.id == id).cloned() else {
            return;
        };
        if !matches!(card.kind, UpdateKind::Idea | UpdateKind::Digest) {
            return;
        }
        let context = discuss_context(&card);
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

    fn archive_feed_digest(&mut self, id: &str) {
        if archive_digest(&mut self.updates, id) {
            self.persist_updates();
        }
    }

    fn open_feed_card(&mut self, id: &str) {
        let kind = self.updates.iter().find(|c| c.id == id).map(|c| c.kind);
        if matches!(kind, Some(UpdateKind::Idea)) {
            let built = self.updates.iter().any(|c| c.id == id && c.built);
            self.nav = if built { Nav::Workboard } else { Nav::Ideas };
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

    fn dismiss_feed_card(&mut self, id: &str) {
        let idea = self
            .updates
            .iter()
            .any(|c| c.id == id && c.kind == UpdateKind::Idea);
        let removed = if idea {
            dismiss_idea(&mut self.updates, id)
        } else {
            dismiss_update(&mut self.updates, id)
        };
        if removed {
            self.persist_updates();
        }
    }

    fn follow_update_action(&mut self, action: Option<UpdateAction>) {
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
                let title = if card.built {
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
                act = Some(FeedAct::Open(card.id.clone()));
            }
            if text_resp.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            ui.with_layout(
                egui::Layout::right_to_left(egui::Align::Center),
                |ui| match card.kind {
                    UpdateKind::Idea => {
                        if crate::cards::ghost_pill(ui, "Dismiss") {
                            act = Some(FeedAct::Dismiss(card.id.clone()));
                        }
                        if full && crate::cards::ghost_pill(ui, "Up") {
                            act = Some(FeedAct::React(card.id.clone(), CardReaction::Up));
                        }
                        if full && crate::cards::ghost_pill(ui, "Discuss") {
                            act = Some(FeedAct::Discuss(card.id.clone()));
                        }
                        if card.built {
                            ui.label(
                                RichText::new("Built")
                                    .size(crate::theme::FONT_TIP)
                                    .color(crate::theme::muted()),
                            );
                        } else if crate::cards::ghost_pill(ui, "Accept") {
                            act = Some(FeedAct::Build(card.id.clone()));
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
