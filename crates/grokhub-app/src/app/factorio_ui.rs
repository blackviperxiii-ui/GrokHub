//! Factorio (`grokhub_core::factorio_mod`, `grokhub_core::factorio_state`).
//!
//! Under the game watcher in Settings: the Loadout mod's Install and Remove,
//! with what a mod costs a save said first, and what the mod last wrote.
//! While the watcher is on and Factorio runs from Steam, this notes the
//! mod's file at launch; when the game closes it reads the file again and
//! posts what changed to Pulse. The only reads are that small file; nothing
//! opens a handle to the game, and the mod is installed only on a click.

use super::*;
use grokhub_core::factorio_mod::{self as fmod, ModState};
use grokhub_core::factorio_state::{self as fstate, FactorioSession, FactorioState};

/// What a mod costs, said before Install.
const WARNING: &str = "Any mod moves a save to Factorio's modded achievement track, so Steam achievements stop for saves played with it, and in multiplayer every player needs it. Best for single-player. Factorio asks you to enable it after install.";

#[derive(Default)]
pub(super) struct FactorioUi {
    /// Factorio's user folder; read once.
    user_dir: Option<Option<PathBuf>>,
    mod_state: Option<ModState>,
    /// The mod's file at launch, while the game runs.
    before: Option<Option<FactorioState>>,
    /// What the mod last wrote, read at startup and when the game closes.
    latest: Option<FactorioState>,
    note: String,
}

impl Cabin {
    /// Factorio's user folder, once the game has run on this computer.
    fn factorio_dir(&mut self) -> Option<PathBuf> {
        self.focus
            .factorio
            .user_dir
            .get_or_insert_with(|| fmod::this_user_dir().filter(|d| d.is_dir()))
            .clone()
    }

    /// Each frame: on the first one, read the mod's state and bring an
    /// older copy of Loadout's own mod up to date; then note the file at
    /// launch and post the session at close.
    pub(super) fn tick_factorio(&mut self) {
        if self.focus.factorio.mod_state.is_none() {
            let Some(dir) = self.factorio_dir() else {
                self.focus.factorio.mod_state = Some(ModState::NotInstalled);
                return;
            };
            let mods = fmod::mods_dir(&dir);
            if let Err(e) = fmod::sync(&mods) {
                self.focus.factorio.note = e;
            }
            self.focus.factorio.mod_state = Some(fmod::state(&mods));
            self.focus.factorio.latest = fstate::read(&dir);
        }
        let running = self.cfg.game_watch && self.game_running() == Some(fmod::APP_ID);
        if running == self.focus.factorio.before.is_some() {
            return;
        }
        let Some(dir) = self.factorio_dir() else {
            return;
        };
        let now_file = fstate::read(&dir);
        if running {
            self.focus.factorio.before = Some(now_file);
            return;
        }
        let before = self.focus.factorio.before.take().flatten();
        self.end_factorio_session(before, now_file, now_ms());
    }

    fn end_factorio_session(
        &mut self,
        before: Option<FactorioState>,
        after: Option<FactorioState>,
        now: u64,
    ) {
        let session = FactorioSession::between(before, after.clone());
        if after.is_some() {
            self.focus.factorio.latest = after;
        }
        let Some(session) = session else {
            return;
        };
        let card = fstate::session_card(&session, now);
        if grokhub_core::post_update(&mut self.updates, card) {
            self.persist_updates();
        }
    }

    /// The mod's row under the game watcher, shown once Factorio has run.
    pub(super) fn ui_factorio(&mut self, ui: &mut egui::Ui) {
        let Some(dir) = self.factorio_dir() else {
            return;
        };
        let f = &self.focus.factorio;
        let state = f.mod_state.clone().unwrap_or(ModState::NotInstalled);
        let line = factorio_status(&state, f.latest.as_ref());
        let note = f.note.clone();
        let mut click = None;
        ui.indent("factorio-mod", |ui| {
            crate::cards::help_text(ui, &line);
            if !state.installed() {
                crate::cards::help_text(ui, WARNING);
            }
            click = match state {
                ModState::NotInstalled => {
                    crate::cards::white_pill(ui, "Install Factorio mod").then_some(true)
                }
                ModState::Outdated { .. } => {
                    crate::cards::white_pill(ui, "Update Factorio mod").then_some(true)
                }
                ModState::Current { .. } => {
                    crate::cards::ghost_pill(ui, "Remove Factorio mod").then_some(false)
                }
                ModState::Foreign => None,
            };
            if !note.is_empty() {
                crate::cards::help_text(ui, &note);
            }
        });
        if let Some(install) = click {
            self.run_factorio_action(&dir, install);
        }
    }

    fn run_factorio_action(&mut self, dir: &std::path::Path, install: bool) {
        let mods = fmod::mods_dir(dir);
        let result = if install {
            fmod::install(&mods)
        } else {
            fmod::remove(&mods)
        };
        let f = &mut self.focus.factorio;
        f.note = match result {
            Ok(()) if install => {
                "Installed. Enable \"Loadout stats\" in Factorio's Mods menu.".into()
            }
            Ok(()) => "Removed.".into(),
            Err(e) => e,
        };
        f.mod_state = Some(fmod::state(&mods));
    }
}

/// `Factorio mod: installed (1.0.0). 42 technologies researched...`
fn factorio_status(state: &ModState, latest: Option<&FactorioState>) -> String {
    let mut line = match state {
        ModState::NotInstalled => "Factorio mod: not installed.".to_string(),
        ModState::Current { version } => format!("Factorio mod: installed ({version})."),
        ModState::Outdated { version } => format!("Factorio mod: {version}, an update is ready."),
        ModState::Foreign => {
            "Factorio: a mod named loadout that Loadout didn't install is there, so it's left alone."
                .to_string()
        }
    };
    if let Some(s) = latest.filter(|_| state.installed()) {
        line.push(' ');
        line.push_str(&s.state_line());
    }
    line
}

#[cfg(test)]
mod tests {
    use super::*;

    const STATE: &str = r#"{"v":1,"researched":5,"rockets":0,"deaths":0}"#;

    #[test]
    fn the_status_line_says_where_the_mod_stands() {
        let s = fstate::parse(STATE).unwrap();
        assert_eq!(
            factorio_status(&ModState::NotInstalled, Some(&s)),
            "Factorio mod: not installed."
        );
        assert_eq!(
            factorio_status(
                &ModState::Current {
                    version: "1.0.0".into()
                },
                Some(&s)
            ),
            "Factorio mod: installed (1.0.0). 5 technologies researched. No deaths."
        );
        assert!(factorio_status(&ModState::Foreign, None).contains("left alone"));
    }

    #[test]
    fn a_session_posts_what_the_mod_wrote_while_the_game_ran() {
        let mut cabin = Cabin::quiet_for_test();
        let before = cabin.updates.len();
        let a = fstate::parse(STATE).unwrap();
        cabin.end_factorio_session(Some(a.clone()), Some(a.clone()), 1);
        assert_eq!(cabin.updates.len(), before);
        let mut b = a.clone();
        b.rockets = 1;
        cabin.end_factorio_session(Some(a), Some(b.clone()), 2);
        assert_eq!(cabin.updates.len(), before + 1);
        let card = cabin.updates.last().unwrap();
        assert_eq!(card.title, "Factorio session: a rocket launched");
        assert_eq!(cabin.focus.factorio.latest, Some(b));
    }

    #[test]
    fn the_mod_installs_and_removes_from_the_button_actions() {
        let dir = std::env::temp_dir().join(format!("loadout-factorio-ui-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut cabin = Cabin::quiet_for_test();
        cabin.run_factorio_action(&dir, true);
        assert!(cabin.focus.factorio.mod_state.as_ref().unwrap().installed());
        assert!(cabin.focus.factorio.note.starts_with("Installed."));
        cabin.run_factorio_action(&dir, false);
        assert_eq!(cabin.focus.factorio.mod_state, Some(ModState::NotInstalled));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
