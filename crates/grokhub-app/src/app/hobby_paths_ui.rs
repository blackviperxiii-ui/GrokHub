//! Bite-sized hobby learning paths in Pulse (Know me opt-in).

use grokhub_core::hobby_paths::{
    self as hp, HobbyId, HobbyPathsLedger, PathFeedback, FILE_NAME, TICK_EVERY_MS,
};

use super::*;
use crate::config;

fn ledger_path() -> std::path::PathBuf {
    config::config_dir().join(FILE_NAME)
}

pub(super) fn load_ledger() -> HobbyPathsLedger {
    hp::load_ledger(&ledger_path())
}

pub(super) fn save_ledger(ledger: &HobbyPathsLedger) {
    let _ = hp::save_ledger(&ledger_path(), ledger);
}

impl Cabin {
    pub(super) fn tick_hobby_paths(&mut self) {
        let now = now_ms();
        let mut ledger = load_ledger();
        if now.saturating_sub(ledger.last_tick_ms) < TICK_EVERY_MS {
            return;
        }
        if !self.cfg.hobby_paths.any_on() {
            ledger.last_tick_ms = now;
            save_ledger(&ledger);
            return;
        }
        let know_me = grokhub_core::know_me::load_tags(&config::config_dir());
        let focus_process = self.focus_process_hint();
        let today = Self::local_day();
        let out = hp::tick(hp::TickInput {
            settings: &self.cfg.hobby_paths,
            ledger: &mut ledger,
            know_me: &know_me,
            focus_process: focus_process.as_deref(),
            today: &today,
            now_ms: now,
        });
        save_ledger(&ledger);
        for card in out.cards {
            self.post_feed_card(card);
        }
    }

    /// Process fragment for the last focused app, for path tip preference.
    fn focus_process_hint(&self) -> Option<String> {
        let app = self.focus.app.as_ref()?;
        // Display names → process fragments the catalog understands.
        let key = match app.name.as_str() {
            "Blender" => "blender",
            "Creality Print" => "creality",
            "Cura" => "cura",
            "PrusaSlicer" => "prusaslicer",
            "OrcaSlicer" => "orcaslicer",
            "Bambu Studio" => "bambu",
            "FreeCAD" => "freecad",
            "Fusion 360" => "fusion360",
            "OpenSCAD" => "openscad",
            "Krita" => "krita",
            "GIMP" => "gimp",
            "Inkscape" => "inkscape",
            "Photoshop" => "photoshop",
            "Cursor" => "cursor",
            "VS Code" | "VSCodium" => "code",
            "Zed" => "zed",
            "RustRover" => "rustrover",
            other => other,
        };
        Some(key.to_ascii_lowercase())
    }

    pub(super) fn hobby_path_feedback(&mut self, id: &str, feedback: PathFeedback) {
        let Some(card) = self.pulse_card(id) else {
            return;
        };
        let Some(meta) = card.pulse.hobby_path.clone() else {
            return;
        };
        let mut ledger = load_ledger();
        let _ = hp::feedback_on_card(&mut ledger, &meta, feedback);
        save_ledger(&ledger);
        if let Some(card) = self.updates.iter_mut().find(|c| c.id == id) {
            if let Some(m) = card.pulse.hobby_path.as_mut() {
                m.answered = true;
            }
            card.status = grokhub_core::UpdateStatus::Opened;
        }
        self.persist_updates();
        let label = match feedback {
            PathFeedback::Done => "Marked done",
            PathFeedback::Skip => "Skipped",
            PathFeedback::TooEasy => "Noted — next tip a bit harder",
            PathFeedback::TooHard => "Noted — next tip a bit easier",
        };
        self.status = label.into();
    }

    pub(super) fn ui_hobby_paths(&mut self, ui: &mut egui::Ui) {
        crate::cards::section_label(ui, "Learning paths");
        crate::cards::help_text(
            ui,
            "Optional bite-sized tips from free docs for hobbies Know me already tagged. Opt in per hobby. Progress stays on this computer; tips can prefer the app you were just in.",
        );
        ui.add_space(8.0);
        let mut daily = self.cfg.hobby_paths.daily_max;
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new("Max tips per day")
                    .size(crate::theme::FONT_BODY)
                    .color(crate::theme::fg()),
            );
            if ui
                .add(egui::DragValue::new(&mut daily).range(1..=6).speed(0.2))
                .changed()
            {
                self.cfg.hobby_paths.daily_max = daily;
                self.persist_cfg();
            }
        });
        ui.add_space(8.0);
        let tags = grokhub_core::know_me::load_tags(&config::config_dir());
        let tagged = hp::hobbies_from_tags(&tags);
        for id in HobbyId::ALL {
            let mut on = self.cfg.hobby_paths.on(id);
            let tagged_here = tagged.contains(&id);
            let label = if tagged_here {
                id.label().to_string()
            } else {
                not_noticed_label(id.label())
            };
            if ui
                .checkbox(
                    &mut on,
                    egui::RichText::new(label).size(crate::theme::FONT_BODY),
                )
                .changed()
            {
                self.cfg.hobby_paths.set_on(id, on);
                self.persist_cfg();
            }
        }
    }
}

/// A hobby nothing on this PC (apps, game library) points to yet, in plain words.
fn not_noticed_label(hobby: &str) -> String {
    format!("{hobby} (not seen on this PC yet)")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unseen_hobby_says_so_in_plain_words() {
        assert_eq!(
            not_noticed_label("Woodworking"),
            "Woodworking (not seen on this PC yet)"
        );
    }

    #[test]
    fn settings_default_off_and_tick_posts_nothing() {
        let mut cabin = Cabin::quiet_for_test();
        assert!(!cabin.cfg.hobby_paths.any_on());
        cabin.tick_hobby_paths();
        assert!(
            cabin
                .updates
                .iter()
                .all(|c| !c.source_id.starts_with(hp::SOURCE_PREFIX)),
            "no tips while all hobbies off"
        );
    }

    #[test]
    fn focus_process_hint_maps_blender() {
        let mut cabin = Cabin::quiet_for_test();
        cabin.cfg.focus_help = true;
        cabin.note_focus_process(Some("blender"));
        assert_eq!(cabin.focus_process_hint().as_deref(), Some("blender"));
    }
}
