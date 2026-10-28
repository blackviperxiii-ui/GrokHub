//! Repeated-ask automate offers: local request log + Pulse suggestion cards.

use grokhub_core::ask_offers::{
    self as ao, append_ask, dismiss_cluster, is_ask_offer_card, AskOfferLedger, TickInput,
    LEDGER_FILE, TICK_EVERY_MS,
};
use grokhub_core::UpdateStatus;

use super::*;
use crate::config;

fn ledger_path() -> std::path::PathBuf {
    config::config_dir().join(LEDGER_FILE)
}

pub(crate) fn load_ask_offer_ledger() -> AskOfferLedger {
    std::fs::read_to_string(ledger_path())
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

pub(super) fn save_ask_offer_ledger(ledger: &AskOfferLedger) {
    if let Ok(text) = serde_json::to_string_pretty(ledger) {
        let path = ledger_path();
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(path, text);
    }
}

impl Cabin {
    /// Log a typed user ask locally (intent + hour). Never leaves the device.
    pub(super) fn note_ask_offer(&mut self, text: &str) {
        if !self.cfg.ask_offers.enabled {
            return;
        }
        let clock = Self::local_clock();
        let mut ledger = load_ask_offer_ledger();
        append_ask(&mut ledger, text, clock.hour, clock.weekday, now_ms());
        save_ask_offer_ledger(&ledger);
    }

    pub(super) fn tick_ask_offers(&mut self) {
        let now = now_ms();
        let mut ledger = load_ask_offer_ledger();
        if now.saturating_sub(ledger.last_tick_ms) < TICK_EVERY_MS {
            return;
        }
        if !self.cfg.ask_offers.enabled {
            ledger.last_tick_ms = now;
            save_ask_offer_ledger(&ledger);
            return;
        }
        let existing: Vec<String> = self
            .automations
            .iter()
            .flat_map(|a| [a.name.clone(), a.instructions.clone()])
            .collect();
        let live: Vec<String> = self
            .updates
            .iter()
            .filter(|c| c.status != UpdateStatus::Dismissed)
            .map(|c| c.source_id.clone())
            .filter(|s| !s.is_empty())
            .collect();
        let today = Self::local_day();
        let out = ao::tick(TickInput {
            settings: &self.cfg.ask_offers,
            ledger: &mut ledger,
            existing_autos: &existing,
            live_sources: &live,
            today: &today,
            now_ms: now,
        });
        save_ask_offer_ledger(&ledger);
        for card in out.cards {
            self.post_feed_card(card);
        }
    }

    /// Dismiss = never suggest that cluster again.
    pub(super) fn ask_offer_dismissed(&mut self, card: &grokhub_core::UpdateCard) {
        if !is_ask_offer_card(card) {
            return;
        }
        let Some(key) = ao::cluster_key_from_source(&card.source_id) else {
            return;
        };
        let mut ledger = load_ask_offer_ledger();
        dismiss_cluster(&mut ledger, key);
        save_ask_offer_ledger(&ledger);
        grokhub_core::remember_dismissed_source(&mut self.cfg.feed_pulse, &card.source_id);
        self.persist_cfg();
    }

    pub(super) fn ui_ask_offers(&mut self, ui: &mut egui::Ui) {
        crate::cards::section_label(ui, "Repeated asks");
        crate::cards::help_text(
            ui,
            "When you ask the same kind of thing around the same hour, GrokHub offers once to schedule it or save a skill. Asks stay on this computer.",
        );
        ui.add_space(8.0);
        let mut on = self.cfg.ask_offers.enabled;
        if ui
            .checkbox(
                &mut on,
                egui::RichText::new("Offer to automate repeated asks")
                    .size(crate::theme::FONT_BODY),
            )
            .changed()
        {
            self.cfg.ask_offers.enabled = on;
            self.persist_cfg();
        }
        if self.cfg.ask_offers.enabled {
            let mut daily = self.cfg.ask_offers.daily_max;
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("Max offers per day")
                        .size(crate::theme::FONT_BODY)
                        .color(crate::theme::fg()),
                );
                if ui
                    .add(egui::DragValue::new(&mut daily).range(1..=5).speed(0.2))
                    .changed()
                {
                    self.cfg.ask_offers.daily_max = daily;
                    self.persist_cfg();
                }
            });
        }
    }
}
