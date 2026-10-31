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
        if !self.ticks.due(FILE_NAME, now, TICK_EVERY_MS) {
            return;
        }
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
        let quiet = self.quiet_now();
        for mut card in out.cards {
            grokhub_core::hold_if_quiet(&mut card, quiet);
            self.post_feed_card(card);
        }
    }

    /// Process fragment for the last focused app, for path tip preference.
    fn focus_process_hint(&self) -> Option<String> {
        let app = self.focus.app.as_ref()?;
        // A recognized game is named by its title ("Hades"), which no
        // catalog fragment matches; any game points to game modding.
        if app.category == grokhub_core::focus_app::FocusCategory::Gaming {
            return Some("steam".into());
        }
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
        let root = config::test_config_root("hobby-tick-off");
        let _pin = config::TestConfigDir::set(root.clone());
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
        assert!(
            root.join(FILE_NAME).is_file(),
            "the tick saved its ledger in the test config folder"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_tip_posted_in_quiet_hours_is_held() {
        let root = config::test_config_root("hobby-quiet");
        let _pin = config::TestConfigDir::set(root.clone());
        let know_me = grokhub_core::know_me::KnowMe {
            on: true,
            tags: vec![grokhub_core::know_me::Tag {
                tag: "dev: Rust".into(),
                signal: grokhub_core::know_me::Signal::Dev,
                topic: Some("Rust".into()),
            }],
            ..Default::default()
        };
        grokhub_core::know_me::save(&root.join(grokhub_core::know_me::FILE_NAME), &know_me)
            .unwrap();
        let mut cabin = Cabin::quiet_for_test();
        cabin.cfg.hobby_paths.set_on(HobbyId::Rust, true);
        // Quiet from this minute for an hour, so the tick lands inside it.
        let now = Cabin::local_clock().hm();
        let start = grokhub_core::organs::hm_min(&now).unwrap();
        let end = (start + 60) % (24 * 60);
        cabin.cfg.quiet_start = now;
        cabin.cfg.quiet_end = format!("{:02}:{:02}", end / 60, end % 60);
        assert!(cabin.quiet_now());
        cabin.tick_hobby_paths();
        let tips: Vec<_> = cabin
            .updates
            .iter()
            .filter(|c| c.source_id.starts_with(hp::SOURCE_PREFIX))
            .collect();
        assert_eq!(tips.len(), 1);
        assert_eq!(
            tips[0].source_id,
            hp::source_id(HobbyId::Rust, "rust-cargo-new")
        );
        assert!(tips[0].held, "quiet hours hold the tip until they end");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_recognized_game_points_to_game_modding() {
        let mut cabin = Cabin::quiet_for_test();
        cabin.cfg.focus_help = true;
        cabin.note_focus_process(Some("gamescope"));
        let hint = cabin.focus_process_hint();
        assert_eq!(hint.as_deref(), Some("steam"));
        assert_eq!(
            hp::hobby_for_process(hint.as_deref().unwrap()),
            Some(HobbyId::GameModding)
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
