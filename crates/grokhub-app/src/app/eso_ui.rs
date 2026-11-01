//! The Elder Scrolls Online (`grokhub_core::eso_addon`).
//!
//! Under the game watcher in Settings: the Loadout addon's Install and
//! Remove for each server folder, and the character the addon saved last.
//! While the watcher is on and ESO runs from Steam, this notes the saved
//! characters at launch; when the game closes it reads them again and posts
//! what changed to Pulse. The only reads are the addon's saved-variables
//! file; nothing opens a handle to the game, and the addon is installed
//! only on a click.

use super::*;
use grokhub_core::eso_addon::{self as eso, AddonState, EsoCharacter, EsoSession};

/// One server folder and where the addon stands in it.
#[derive(Debug, Clone, PartialEq)]
struct EsoServer {
    dir: PathBuf,
    state: AddonState,
}

#[derive(Default)]
pub(super) struct EsoUi {
    /// The server folders found; read on the first tick and after a click.
    servers: Option<Vec<EsoServer>>,
    /// The characters at launch, while the game runs.
    before: Option<Vec<EsoCharacter>>,
    /// The characters the addon saved last.
    latest: Vec<EsoCharacter>,
    note: String,
}

impl Cabin {
    fn eso_scan(&mut self) {
        let root = self.focus.watch.steam_root();
        let dirs = eso::find_server_dirs(root.as_deref());
        let mut servers = Vec::new();
        for dir in dirs {
            if let Err(e) = eso::sync(&dir) {
                self.focus.eso.note = e;
            }
            let state = eso::state(&dir);
            servers.push(EsoServer { dir, state });
        }
        self.focus.eso.latest = eso_characters(&servers);
        self.focus.eso.servers = Some(servers);
    }

    /// Each frame: on the first one, find the server folders and bring an
    /// older copy of Loadout's own addon up to date; then note the saved
    /// characters at launch and post the session at close.
    pub(super) fn tick_eso(&mut self) {
        if self.focus.eso.servers.is_none() {
            self.eso_scan();
        }
        let running = self.cfg.game_watch && self.game_running() == Some(eso::APP_ID);
        if running == self.focus.eso.before.is_some() {
            return;
        }
        let servers = self.focus.eso.servers.clone().unwrap_or_default();
        let now = eso_characters(&servers);
        if running {
            self.focus.eso.before = Some(now);
            return;
        }
        let before = self.focus.eso.before.take().unwrap_or_default();
        self.end_eso_session(&before, now, now_ms());
    }

    fn end_eso_session(&mut self, before: &[EsoCharacter], after: Vec<EsoCharacter>, now: u64) {
        let session = EsoSession::between(before, &after);
        if !after.is_empty() {
            self.focus.eso.latest = after;
        }
        let Some(session) = session else {
            return;
        };
        let card = eso::session_card(&session, now);
        if grokhub_core::post_update(&mut self.updates, card) {
            self.persist_updates();
        }
    }

    /// The addon's rows under the game watcher, shown once ESO has a
    /// server folder on this computer.
    pub(super) fn ui_eso(&mut self, ui: &mut egui::Ui) {
        let Some(servers) = self.focus.eso.servers.clone() else {
            return;
        };
        if servers.is_empty() {
            return;
        }
        let latest = self.focus.eso.latest.first().map(eso::character_line);
        let note = self.focus.eso.note.clone();
        let many = servers.len() > 1;
        let mut click = None;
        ui.indent("eso-addon", |ui| {
            for (i, s) in servers.iter().enumerate() {
                let label = if many {
                    format!(" ({})", eso::server_label(&s.dir))
                } else {
                    String::new()
                };
                crate::cards::help_text(ui, &eso_status(&s.state, &label));
                let pressed = match s.state {
                    AddonState::NotInstalled => {
                        crate::cards::white_pill(ui, &format!("Install ESO addon{label}"))
                            .then_some(true)
                    }
                    AddonState::Outdated { .. } => {
                        crate::cards::white_pill(ui, &format!("Update ESO addon{label}"))
                            .then_some(true)
                    }
                    AddonState::Current { .. } => {
                        crate::cards::ghost_pill(ui, &format!("Remove ESO addon{label}"))
                            .then_some(false)
                    }
                    AddonState::Foreign => None,
                };
                if let Some(install) = pressed {
                    click = Some((i, install));
                }
            }
            if let Some(line) = &latest {
                crate::cards::help_text(ui, line);
            }
            if !note.is_empty() {
                crate::cards::help_text(ui, &note);
            }
        });
        if let Some((i, install)) = click {
            self.run_eso_action(&servers[i].dir, install);
        }
    }

    fn run_eso_action(&mut self, dir: &std::path::Path, install: bool) {
        let result = if install {
            eso::install(dir)
        } else {
            eso::remove(dir)
        };
        self.focus.eso.note = match result {
            Ok(()) if install => {
                "Installed. Turn on Loadout in ESO's Add-Ons menu, then it saves when you log out or reload the UI.".into()
            }
            Ok(()) => "Removed.".into(),
            Err(e) => e,
        };
        if let Some(servers) = self.focus.eso.servers.as_mut() {
            for s in servers.iter_mut().filter(|s| s.dir == dir) {
                s.state = eso::state(dir);
            }
        }
    }
}

/// Every server's saved characters, newest first.
fn eso_characters(servers: &[EsoServer]) -> Vec<EsoCharacter> {
    let mut all: Vec<EsoCharacter> = servers
        .iter()
        .flat_map(|s| eso::read_characters(&s.dir))
        .collect();
    all.sort_by_key(|c| std::cmp::Reverse(c.seen));
    all
}

/// `ESO addon: installed (1.0.0).`
fn eso_status(state: &AddonState, label: &str) -> String {
    match state {
        AddonState::NotInstalled => format!("ESO addon{label}: not installed."),
        AddonState::Current { version } => format!("ESO addon{label}: installed ({version})."),
        AddonState::Outdated { version } => {
            format!("ESO addon{label}: {version}, an update is ready.")
        }
        AddonState::Foreign => format!(
            "ESO{label}: an addon named Loadout that Loadout didn't install is there, so it's left alone."
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn knight(level: u64) -> EsoCharacter {
        EsoCharacter {
            name: "Testknight".into(),
            level: Some(level),
            seen: 10,
            ..Default::default()
        }
    }

    #[test]
    fn the_status_line_says_where_the_addon_stands() {
        assert_eq!(
            eso_status(&AddonState::NotInstalled, ""),
            "ESO addon: not installed."
        );
        assert_eq!(
            eso_status(
                &AddonState::Current {
                    version: "1.0.0".into()
                },
                " (EU)"
            ),
            "ESO addon (EU): installed (1.0.0)."
        );
        assert!(eso_status(&AddonState::Foreign, "").contains("left alone"));
    }

    #[test]
    fn a_session_posts_what_the_addon_saved_while_the_game_ran() {
        let mut cabin = Cabin::quiet_for_test();
        let before = cabin.updates.len();
        cabin.end_eso_session(&[knight(10)], vec![knight(10)], 1);
        assert_eq!(cabin.updates.len(), before);
        cabin.end_eso_session(&[knight(10)], vec![knight(12)], 2);
        assert_eq!(cabin.updates.len(), before + 1);
        let card = cabin.updates.last().unwrap();
        assert_eq!(
            card.title,
            "The Elder Scrolls Online session: reached level 12"
        );
        assert_eq!(cabin.focus.eso.latest, vec![knight(12)]);
    }

    #[test]
    fn the_addon_installs_and_removes_from_the_button_actions() {
        let dir = std::env::temp_dir().join(format!("loadout-eso-ui-{}/live", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut cabin = Cabin::quiet_for_test();
        cabin.focus.eso.servers = Some(vec![EsoServer {
            dir: dir.clone(),
            state: AddonState::NotInstalled,
        }]);
        cabin.run_eso_action(&dir, true);
        let state = |c: &Cabin| c.focus.eso.servers.as_ref().unwrap()[0].state.clone();
        assert!(state(&cabin).installed());
        assert!(cabin.focus.eso.note.starts_with("Installed."));
        cabin.run_eso_action(&dir, false);
        assert_eq!(state(&cabin), AddonState::NotInstalled);
        let _ = std::fs::remove_dir_all(dir.parent().unwrap());
    }
}
