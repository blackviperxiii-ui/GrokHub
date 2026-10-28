//! Command Centre card 49: opt-in screenshot folder watch and Explain chip.

use std::path::PathBuf;

use grokhub_core::{
    dismiss_screenshot_offer, tick_screenshot_offer, user_home, watch_dirs, ScreenshotOfferLedger,
    CHIP_LABEL, DISMISS_LABEL, EXPLAIN_PROMPT,
};

use super::*;
use crate::config;

const LEDGER_FILE: &str = "screenshot-offer.json";
const TICK_EVERY_MS: u64 = 30_000;

fn ledger_path() -> std::path::PathBuf {
    config::config_dir().join(LEDGER_FILE)
}

pub(super) fn load_screenshot_offer_ledger() -> ScreenshotOfferLedger {
    std::fs::read_to_string(ledger_path())
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

pub(super) fn save_screenshot_offer_ledger(ledger: &ScreenshotOfferLedger) {
    if let Ok(text) = serde_json::to_string_pretty(ledger) {
        let path = ledger_path();
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(path, text);
    }
}

impl Cabin {
    pub(super) fn tick_screenshot_offer(&mut self) {
        let now = now_ms();
        let mut ledger = load_screenshot_offer_ledger();
        if now.saturating_sub(ledger.last_tick_ms()) < TICK_EVERY_MS {
            return;
        }
        ledger.set_last_tick_ms(now);
        if !self.cfg.screenshot_explain.enabled {
            if ledger.pending.take().is_some() {
                save_screenshot_offer_ledger(&ledger);
            }
            return;
        }
        let Some(home) = user_home() else {
            save_screenshot_offer_ledger(&ledger);
            return;
        };
        let os = if cfg!(windows) {
            "windows"
        } else if cfg!(target_os = "macos") {
            "macos"
        } else {
            "linux"
        };
        let dirs = watch_dirs(&home, os);
        let _ = tick_screenshot_offer(
            &mut ledger,
            &self.cfg.screenshot_explain,
            &dirs,
            now,
        );
        save_screenshot_offer_ledger(&ledger);
    }

    pub(super) fn ui_screenshot_offer_chip(&mut self, ui: &mut egui::Ui) {
        if !self.cfg.screenshot_explain.enabled {
            return;
        }
        let now = now_ms();
        let ledger = load_screenshot_offer_ledger();
        let Some(offer) = ledger.offer_live(now) else {
            return;
        };
        let path = offer.path.clone();
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            if ui
                .button(format!("{CHIP_LABEL}"))
                .on_hover_text("Read this screenshot and explain it in chat")
                .clicked()
            {
                self.accept_screenshot_explain(path);
            }
            if ui.button(DISMISS_LABEL).clicked() {
                let mut ledger = load_screenshot_offer_ledger();
                dismiss_screenshot_offer(&mut ledger);
                save_screenshot_offer_ledger(&ledger);
            }
        });
    }

    fn accept_screenshot_explain(&mut self, path: String) {
        let mut ledger = load_screenshot_offer_ledger();
        dismiss_screenshot_offer(&mut ledger);
        save_screenshot_offer_ledger(&ledger);
        let path = PathBuf::from(path);
        match plus_from_path(PlusTarget::Chat, path) {
            PlusPick::Ready(ready) => {
                self.apply_plus_ready(PlusTarget::Chat, ready);
                self.composer = EXPLAIN_PROMPT.into();
            }
            PlusPick::Err(e) => self.status = e,
            PlusPick::NativeMiss | PlusPick::ClipEmpty | PlusPick::ClipText(_) => {
                self.status = "That screenshot is gone.".into();
            }
        }
    }

    pub(super) fn ui_screenshot_explain_settings(&mut self, ui: &mut egui::Ui) {
        ui.label("When you save a new screenshot, offer a small Explain chip. The image is not read until you tap.");
        let mut on = self.cfg.screenshot_explain.enabled;
        if ui.checkbox(&mut on, "Screenshot explain").changed() {
            self.cfg.screenshot_explain.enabled = on;
            if !on {
                let mut ledger = load_screenshot_offer_ledger();
                dismiss_screenshot_offer(&mut ledger);
                save_screenshot_offer_ledger(&ledger);
            }
            self.persist_cfg();
        }
    }
}
