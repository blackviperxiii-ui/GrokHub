//! Pulse's Saved page: posts kept with Save, read from Saved Finds
//! (`saved-finds.json`), a file the feed never cleans, expires, archives or
//! deletes. Each saved card is always open and has no row of buttons: only
//! a ··· menu with one item, Remove from saved.

use super::pulse_sources_ui::saved_finds_path;
use super::*;
use grokhub_core::pulse_sources::{self as ps, SavedFind};

/// The one item in a saved card's ··· menu.
pub(super) const REMOVE_FROM_SAVED: &str = "Remove from saved";
/// The Saved page with nothing on it.
pub(super) const SAVED_EMPTY: &str =
    "Nothing saved yet. Save a post in the feed and it stays here.";

/// What a tap on the Saved page asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum SavedTap {
    Open(String),
    Remove(String),
}

impl Cabin {
    /// Every saved card, read fresh so the desktop app's saves show too.
    pub(super) fn saved_page_list(&mut self) -> Vec<SavedFind> {
        let list = ps::load_saved(&saved_finds_path());
        self.sources.saved = Some(list.clone());
        list
    }

    /// Remove from saved: read the file, take the link out, write it back.
    pub(super) fn remove_saved(&mut self, url: &str) {
        let mut list = ps::load_saved(&saved_finds_path());
        if ps::remove_find(&mut list, url) {
            if let Err(err) = ps::save_saved(&saved_finds_path(), &list) {
                self.status = err;
                return;
            }
            self.status = "Removed from saved.".into();
        }
        self.sources.saved = Some(list);
    }

    pub(super) fn ui_pulse_saved(&mut self, ui: &mut egui::Ui) {
        let list = self.saved_page_list();
        if list.is_empty() {
            ui.label(
                RichText::new(SAVED_EMPTY)
                    .size(crate::theme::FONT_BODY)
                    .color(crate::theme::muted()),
            );
            return;
        }
        let mut tap = None;
        for find in &list {
            if let Some(t) = paint_saved_card(ui, find) {
                tap = Some(t);
            }
            ui.add_space(crate::theme::SPACING_8);
        }
        match tap {
            Some(SavedTap::Open(url)) => {
                self.follow_update_action(Some(grokhub_core::UpdateAction::DeepLink { href: url }));
            }
            Some(SavedTap::Remove(url)) => self.remove_saved(&url),
            None => {}
        }
    }
}

/// One saved card: title (opens the link), where it is from and why, the
/// summary in full, and the ··· menu. No other buttons.
fn paint_saved_card(ui: &mut egui::Ui, find: &SavedFind) -> Option<SavedTap> {
    let mut tap = None;
    super::feed_ui::kit_pulse_feed_post(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.spacing_mut().item_spacing.y = 4.0;
        ui.horizontal(|ui| {
            let title = ui.add(
                egui::Label::new(
                    RichText::new(&find.title)
                        .size(crate::theme::FONT_BODY)
                        .strong()
                        .color(crate::theme::fg()),
                )
                .sense(egui::Sense::click())
                .wrap(),
            );
            if title.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            if title.clicked() {
                tap = Some(SavedTap::Open(find.url.clone()));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                crate::cards::dots_menu(ui, false, |ui| {
                    if ui.button(REMOVE_FROM_SAVED).clicked() {
                        tap = Some(SavedTap::Remove(find.url.clone()));
                        ui.close();
                    }
                });
            });
        });
        ui.label(
            RichText::new(saved_source_line(find))
                .size(crate::theme::FONT_TIP)
                .color(crate::theme::muted()),
        );
        if !find.summary.trim().is_empty() {
            ui.add(
                egui::Label::new(
                    RichText::new(find.summary.trim())
                        .size(crate::theme::FONT_BODY)
                        .color(crate::theme::muted()),
                )
                .wrap()
                .selectable(false),
            );
        }
    });
    tap
}

/// "Rust Blog · You follow Rust".
pub(super) fn saved_source_line(find: &SavedFind) -> String {
    let source = if find.source_name.trim().is_empty() {
        "Saved"
    } else {
        find.source_name.trim()
    };
    match find
        .because
        .as_deref()
        .map(str::trim)
        .filter(|b| !b.is_empty())
    {
        Some(why) => format!("{source} · {why}"),
        None => source.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_saved_page_has_one_menu_item_and_a_pill_back_to_the_feed() {
        assert_eq!(REMOVE_FROM_SAVED, "Remove from saved");
        assert_eq!(super::super::pulse_ui::saved_pill_label(false), "Saved");
        assert_eq!(
            super::super::pulse_ui::saved_pill_label(true),
            "Back to feed"
        );
        let mut find = SavedFind {
            title: "Rust 1.99".into(),
            url: "https://a.test/1".into(),
            source_name: "Rust Blog".into(),
            summary: String::new(),
            because: Some("You follow Rust".into()),
            saved_ms: 1,
        };
        assert_eq!(saved_source_line(&find), "Rust Blog · You follow Rust");
        find.source_name.clear();
        find.because = None;
        assert_eq!(saved_source_line(&find), "Saved");
    }
}
