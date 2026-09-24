//! Home update feed in the slot that held the Coding / Life chip and the
//! under-bar workboard summary. Hidden entirely when nothing is undismissed.

use super::*;
use grokhub_core::{
    automation_done_card, dismiss_update, feed_visible, mark_update_opened, post_update,
    schedule_created_card, visible_updates, UpdateAction, UpdateCard, UpdateKind, UpdateStatus,
    FEED_PAINT_MAX,
};

const FEED_CARD_H: f32 = 64.0;
const FEED_GAP: f32 = 6.0;

pub(super) fn update_feed_h(n: usize) -> f32 {
    let n = n.min(FEED_PAINT_MAX);
    if n == 0 {
        return 0.0;
    }
    let n = n as f32;
    n * FEED_CARD_H + (n - 1.0).max(0.0) * FEED_GAP
}

impl Cabin {
    /// `poll_grok_loop` and a finished night recipe replay call this.
    pub(super) fn note_automation_done(&mut self, source_id: &str, title: &str, summary: &str) {
        if title.trim().is_empty() && summary.trim().is_empty() {
            return;
        }
        let card = automation_done_card(source_id, title, summary, now_ms());
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

    pub(super) fn paint_update_feed(&mut self, ui: &mut egui::Ui, pane_w: f32) {
        if !feed_visible(&self.updates) {
            return;
        }
        let shown: Vec<UpdateCard> = visible_updates(&self.updates)
            .into_iter()
            .take(FEED_PAINT_MAX)
            .collect();
        if shown.is_empty() {
            return;
        }
        let mut opened: Option<String> = None;
        let mut dismissed: Option<String> = None;
        ui.vertical(|ui| {
            ui.set_width(pane_w);
            ui.spacing_mut().item_spacing.y = FEED_GAP;
            for card in &shown {
                let (rect, _) =
                    ui.allocate_exact_size(egui::vec2(pane_w, FEED_CARD_H), egui::Sense::hover());
                ui.painter().rect(
                    rect,
                    crate::theme::CARD_RADIUS,
                    crate::theme::elevated(),
                    egui::Stroke::new(1.0_f32, crate::theme::border()),
                );
                let inner = rect.shrink(8.0);
                ui.allocate_new_ui(egui::UiBuilder::new().max_rect(inner), |ui| {
                    ui.horizontal(|ui| {
                        let text_w = (inner.width() - 150.0).max(80.0);
                        let title_color = if card.status == UpdateStatus::Opened {
                            crate::theme::muted()
                        } else {
                            crate::theme::fg()
                        };
                        let (text_rect, text_resp) = ui.allocate_exact_size(
                            egui::vec2(text_w, inner.height()),
                            egui::Sense::click(),
                        );
                        ui.allocate_new_ui(egui::UiBuilder::new().max_rect(text_rect), |ui| {
                            ui.set_width(text_w);
                            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
                            ui.label(
                                RichText::new(format!("{} · {}", card.kind.label(), card.title))
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
                            opened = Some(card.id.clone());
                        }
                        if text_resp.hovered() {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if crate::cards::ghost_pill(ui, "Dismiss") {
                                dismissed = Some(card.id.clone());
                            }
                            if matches!(
                                card.kind,
                                UpdateKind::Suggestion | UpdateKind::AutomateOffer
                            ) && crate::cards::ghost_pill(ui, "Accept")
                            {
                                opened = Some(card.id.clone());
                            }
                        });
                    });
                });
            }
        });
        if let Some(id) = dismissed {
            self.dismiss_feed_card(&id);
        } else if let Some(id) = opened {
            self.open_feed_card(&id);
        }
    }

    fn open_feed_card(&mut self, id: &str) {
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
        if dismiss_update(&mut self.updates, id) {
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
