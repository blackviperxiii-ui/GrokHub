//! One-time Pulse offers for helpers that are off by default, so a fresh
//! profile finds them without digging through Settings → Behavior.

use super::*;

/// An off-by-default helper Pulse offers to turn on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum HelperOffer {
    WindDown,
    Tidy,
}

impl HelperOffer {
    pub(super) const ALL: [HelperOffer; 2] = [HelperOffer::WindDown, HelperOffer::Tidy];

    /// Stored in `helper_offers_dismissed` after "Not now".
    pub(super) fn id(self) -> &'static str {
        match self {
            HelperOffer::WindDown => "wind_down",
            HelperOffer::Tidy => "downloads_tidy",
        }
    }

    pub(super) fn ask(self) -> &'static str {
        match self {
            HelperOffer::WindDown => "Offer to sleep the PC late at night?",
            HelperOffer::Tidy => "Suggest Downloads cleanups?",
        }
    }

    fn hint(self) -> &'static str {
        match self {
            HelperOffer::WindDown => {
                "Once a night, when you're well past your usual stop or idle, I offer to sleep the PC. Never during a game or a task, and only after you confirm."
            }
            HelperOffer::Tidy => {
                "Once a week I look at Downloads and suggest old installers, big old archives and duplicates. Nothing moves until you approve it."
            }
        }
    }

    fn turned_on(self) -> &'static str {
        match self {
            HelperOffer::WindDown => {
                "Late-night wind-down is on. Turn it off in Settings › Behavior."
            }
            HelperOffer::Tidy => "Downloads cleanups are on. Turn them off in Settings › Behavior.",
        }
    }
}

impl Cabin {
    /// Helpers still off and not dismissed, in [`HelperOffer::ALL`] order.
    pub(super) fn helper_offers(&mut self) -> Vec<HelperOffer> {
        let mut out = Vec::new();
        for offer in HelperOffer::ALL {
            let on = match offer {
                HelperOffer::WindDown => self.wind_down_on(),
                HelperOffer::Tidy => self.cfg.downloads_tidy,
            };
            let dismissed = self
                .cfg
                .helper_offers_dismissed
                .iter()
                .any(|d| d == offer.id());
            if !on && !dismissed {
                out.push(offer);
            }
        }
        out
    }

    /// "Turn on" switches the helper on; "Not now" stops offering it.
    pub(super) fn answer_helper_offer(&mut self, offer: HelperOffer, turn_on: bool) {
        if turn_on {
            // A failed wind-down write keeps its error in the status line.
            let saved = match offer {
                HelperOffer::WindDown => self.set_wind_on(true),
                HelperOffer::Tidy => {
                    self.set_downloads_tidy(true);
                    true
                }
            };
            if saved {
                self.status = offer.turned_on().into();
            }
        } else {
            self.cfg.helper_offers_dismissed.push(offer.id().into());
            self.persist_cfg();
        }
    }

    /// One line per open offer under the Pulse header: the question, Turn on, Not now.
    pub(super) fn ui_helper_offers(&mut self, ui: &mut egui::Ui) {
        for offer in self.helper_offers() {
            let mut answer = None;
            ui.add_space(8.0);
            // A row of its own: the question on the left, its two buttons
            // pinned right with room between, not jammed against the text.
            egui::Frame::NONE
                .fill(crate::theme::elevated())
                .corner_radius(10.0)
                .stroke(egui::Stroke::new(1.0, crate::theme::border()))
                .inner_margin(egui::Margin::symmetric(12, 8))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(offer.ask())
                                .size(crate::theme::FONT_BODY)
                                .color(crate::theme::fg()),
                        )
                        .on_hover_text(offer.hint());
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.spacing_mut().item_spacing.x = 8.0;
                            // Right to left: Not now paints first, so Turn on reads first.
                            if crate::cards::ghost_pill(ui, "Not now") {
                                answer = Some(false);
                            }
                            if crate::cards::ghost_pill(ui, "Turn on") {
                                answer = Some(true);
                            }
                        });
                    });
                });
            if let Some(turn_on) = answer {
                self.answer_helper_offer(offer, turn_on);
            }
        }
    }
}
