//! Focus help (`grokhub_core::focus_app`), opt-in under Settings → Behavior.
//! When the cabin loses focus it reads, once and off the UI thread, the
//! process name of the app that took it. When the cabin is back in front, a
//! known app puts one chip first in the composer row ("Because you were in
//! Blender") and one context line on the next turn. Off reads nothing.

use std::sync::{Arc, Mutex};

use super::*;
use grokhub_core::focus_app::{self, FocusApp};
use grokhub_core::QuickChip;

/// Let the other app finish taking focus before the one read.
const READ_DELAY: Duration = Duration::from_millis(300);

#[derive(Default)]
pub(super) struct FocusState {
    was_focused: bool,
    /// The process name the last read found; taken when the cabin is back.
    read: Arc<Mutex<Option<String>>>,
    /// The app the user was just in, while the cabin is in front.
    pub app: Option<FocusApp>,
    /// The next turn carries the app's context line once.
    context_pending: bool,
    /// "Pick up where you left off" for code, the other what-you-were-doing help.
    pub resume: super::code_resume_ui::ResumeState,
    /// The late-night wind-down reminder.
    pub wind: super::wind_down_ui::WindDownState,
    /// Downloads tidy suggestions and their review.
    pub tidy: super::downloads_tidy_ui::TidyState,
    /// Backup and unpushed-work reminders.
    pub backup: super::backup_ui::BackupState,
    /// The internet drop log.
    pub net: super::net_log_ui::NetState,
    /// The game launch companion.
    pub game: super::game_companion_ui::GameUi,
    /// The game watcher's GSI file and listener.
    pub watch: super::game_watch_ui::WatchState,
    /// Battle.net games and the WoW combat log follow.
    pub wow: super::wow_ui::WowState,
    /// Path of Exile's own log, followed while it runs.
    pub poe: super::poe_ui::PoeState,
    /// Minecraft's own logs, followed while the game watcher is on.
    pub minecraft: super::minecraft_ui::McState,
    /// New 3D models in Downloads.
    pub models: super::model_drop_ui::ModelDropState,
    /// The screenshot Explain offer's folder scan and image load.
    pub shot: super::screenshot_offer_ui::ShotState,
    /// Elite Dangerous's Player Journal, followed while it runs.
    pub elite: super::elite_ui::EliteState,
    /// Stardew Valley's farm saves, compared across a play session.
    pub stardew: super::stardew_ui::StardewState,
    /// The Loadout ESO addon and what it saved.
    pub eso: super::eso_ui::EsoUi,
    /// Rocket League's Stats API, read while the game runs.
    pub rl: super::rl_ui::RlUi,
}

impl Cabin {
    /// Each frame: whether the window has focus, then the focus-help tick.
    pub(super) fn read_window_focus(&mut self, ctx: &egui::Context) {
        self.window_focused = ctx.input(|i| i.viewport().focused.unwrap_or(false));
        self.tick_focus(self.window_focused);
        self.tick_code_resume();
        self.tick_wind_down();
        self.tick_downloads_tidy();
        self.tick_backup();
        self.tick_board_store();
        self.tick_board_notices();
        self.tick_work_resume();
        self.tick_net_log();
        self.tick_game_companion();
        self.tick_game_watch();
        self.tick_wow();
        self.tick_poe();
        self.tick_minecraft();
        self.tick_model_drop();
        self.poll_screenshot_offer();
        self.tick_elite();
        self.tick_stardew();
        self.tick_eso();
        self.tick_rl();
        if self.focus.shot.busy() {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
    }

    /// From the UI pass: what the focus helpers paint over the cabin.
    pub(super) fn paint_focus_overlays(&mut self, ctx: &egui::Context) {
        self.paint_game_note(ctx);
    }

    /// Each frame with the window's focus. Only transitions do anything.
    pub(super) fn tick_focus(&mut self, focused: bool) {
        let was = std::mem::replace(&mut self.focus.was_focused, focused);
        if !self.cfg.focus_help || was == focused {
            return;
        }
        if focused {
            let name = self.focus.read.lock().ok().and_then(|mut g| g.take());
            self.note_focus_process(name.as_deref());
        } else {
            self.focus.app = None;
            let slot = Arc::clone(&self.focus.read);
            std::thread::spawn(move || {
                std::thread::sleep(READ_DELAY);
                let name = super::local_signals::foreground_process();
                if let Ok(mut g) = slot.lock() {
                    *g = name;
                }
            });
        }
    }

    /// The cabin is back in front after the user was in `process`.
    pub(super) fn note_focus_process(&mut self, process: Option<&str>) {
        let games = self.focus_games();
        self.focus.app = process.and_then(|p| focus_app::categorize(p, &games));
        self.focus.context_pending = self.focus.app.is_some();
        // The chip row is cached on a key; a new app must repaint it.
        self.chip_paint_key.clear();
    }

    /// Game names from the Steam source, when the user turned it on.
    fn focus_games(&mut self) -> Vec<String> {
        self.steam_game_names()
    }

    /// The focus chip, unless the user dismissed it.
    pub(super) fn focus_chip(&self) -> Option<QuickChip> {
        if !self.cfg.focus_help {
            return None;
        }
        let chip = self.focus.app.as_ref()?.chip();
        let gone = self
            .chip_dismissed
            .iter()
            .any(|d| d == &chip.id || d == &chip.value);
        (!gone).then_some(chip)
    }

    /// The context line for the next turn, once per return to the cabin.
    pub(super) fn take_focus_context(&mut self) -> Option<String> {
        if !self.cfg.focus_help || !std::mem::take(&mut self.focus.context_pending) {
            return None;
        }
        self.focus.app.as_ref().map(FocusApp::context_line)
    }

    pub(super) fn set_focus_help(&mut self, on: bool) {
        if self.cfg.focus_help == on {
            return;
        }
        self.cfg.focus_help = on;
        if !on {
            // Downloads tidy is its own setting: keep its findings and review.
            let tidy = std::mem::take(&mut self.focus.tidy);
            self.focus = FocusState::default();
            self.focus.tidy = tidy;
            self.chip_paint_key.clear();
        }
        self.persist_cfg();
        self.status = "Saved".into();
    }

    pub(super) fn ui_focus_help_row(&mut self, ui: &mut egui::Ui) {
        let mut on = self.cfg.focus_help;
        if crate::cards::settings_toggle(
            ui,
            "Help with the app I was just in",
            "When you come back, the first suggestion fits the app you left (Blender, a slicer, a game, an editor). Reads only that app's process name, once, never window titles or contents. Off by default.",
            &mut on,
        ) {
            self.set_focus_help(on);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn off_reads_nothing_and_shows_nothing() {
        let mut cabin = Cabin::quiet_for_test();
        cabin.cfg.focus_help = false;
        cabin.tick_focus(true);
        cabin.tick_focus(false);
        std::thread::sleep(READ_DELAY * 3);
        assert_eq!(*cabin.focus.read.lock().unwrap(), None, "no read while off");
        cabin.note_focus_process(Some("blender"));
        assert_eq!(cabin.focus_chip(), None);
        assert_eq!(cabin.take_focus_context(), None);
    }

    #[test]
    fn a_known_app_puts_one_chip_first_and_one_context_line() {
        let mut cabin = Cabin::quiet_for_test();
        cabin.cfg.focus_help = true;
        cabin.note_focus_process(Some("Cursor.exe"));
        let chip = cabin.focus_chip().expect("chip");
        assert_eq!(chip.label, "Next coding step");
        assert_eq!(chip.hint, "Because you were in Cursor");
        assert_eq!(cabin.composer_chips()[0].id, "focus:coding:cursor");
        assert_eq!(
            cabin.take_focus_context().as_deref(),
            Some(
                "[Context: the user was just in Cursor (coding). Use it only if it fits the ask.]"
            )
        );
        assert_eq!(cabin.take_focus_context(), None, "once per return");
        cabin.dismiss_chip(chip);
        assert_eq!(cabin.focus_chip(), None, "dismissed stays gone");
        assert!(cabin
            .composer_chips()
            .iter()
            .all(|c| !c.id.starts_with("focus:")));
    }

    #[test]
    fn an_unknown_app_or_turning_it_off_clears_the_hint() {
        let mut cabin = Cabin::quiet_for_test();
        cabin.cfg.focus_help = true;
        cabin.note_focus_process(Some("firefox"));
        assert_eq!(cabin.focus_chip(), None);
        assert_eq!(cabin.take_focus_context(), None);
        cabin.note_focus_process(Some("blender"));
        assert!(cabin.focus_chip().is_some());
        cabin.set_focus_help(false);
        assert_eq!(cabin.focus.app, None);
        assert_eq!(cabin.focus_chip(), None);
    }
}
