//! Code tab → Secrets, in the cabin: this project's deploy secrets, names
//! only. A value goes into the OS keychain once and is never shown again;
//! the field is masked and empties itself whether the save worked or not.
//! Deleting asks first. The names file and the keychain entries are the
//! same ones the desktop app's Code settings use, so a secret set in one
//! app shows by name in the other.

use std::path::{Path, PathBuf};
use std::sync::mpsc;

use eframe::egui::{self, RichText};
use grokhub_code::secrets::{self, Vault};

use super::confirm::{paint_confirm_sheet, ConfirmAct, ConfirmSpec};

pub(super) const SECRETS_TITLE: &str = "Secrets";
pub(super) const SECRETS_NOTE: &str = "Deploy runs get these as environment variables. Values stay in your system keychain and are hidden in output; only the names are saved here.";
pub(super) const SECRETS_NONE: &str = "No secrets for this project yet.";
pub(super) const SECRETS_DELETE_NOTE: &str =
    "Its value is removed from the keychain. A deploy that needs it stops until you add it again.";
pub(super) const NAME_HINT: &str = "DEPLOY_TOKEN";

/// The delete confirm; the secret's name shows under it.
pub(super) fn delete_spec() -> ConfirmSpec {
    ConfirmSpec {
        title: "Delete this secret?",
        consequence: SECRETS_DELETE_NOTE,
        primary: "Delete",
        danger: true,
    }
}

/// The project's key, the same one the shell's Code tab uses.
pub(super) fn repo_key_of(ws: &Path) -> String {
    grokhub_code::runs::repo_key(ws, grokhub_code::repo::origin_url(ws).as_deref())
}

/// Work for the worker thread. The value rides along only to the keychain.
pub(super) enum SecretJob {
    List,
    Set {
        name: String,
        value: String,
    },
    /// Only made by the confirm card's Delete.
    Delete {
        name: String,
    },
}

impl std::fmt::Debug for SecretJob {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::List => f.write_str("List"),
            Self::Set { name, .. } => f.debug_struct("Set").field("name", name).finish(),
            Self::Delete { name } => f.debug_struct("Delete").field("name", name).finish(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum SecretOut {
    Listed(Vec<String>),
    Saved {
        name: String,
        replaced: bool,
        res: Result<Vec<String>, String>,
    },
    Deleted(Result<Vec<String>, String>),
}

impl SecretJob {
    /// Runs on the worker thread against `vault` (the keychain in the app).
    pub(super) fn run(self, config_dir: &Path, ws: &Path, vault: &dyn Vault) -> SecretOut {
        let key = repo_key_of(ws);
        let keep = super::code_ship_ui::settings(config_dir).activity_keep;
        match self {
            SecretJob::List => SecretOut::Listed(secrets::list(config_dir, &key).names),
            SecretJob::Set { name, value } => {
                let name = name.trim().to_string();
                let replaced = secrets::list(config_dir, &key).names.contains(&name);
                let res = secrets::set_logged(config_dir, vault, &key, &name, &value, keep)
                    .map(|l| l.names);
                SecretOut::Saved {
                    name,
                    replaced,
                    res,
                }
            }
            SecretJob::Delete { name } => SecretOut::Deleted(
                secrets::delete_logged(config_dir, vault, &key, &name, true, keep).map(|l| l.names),
            ),
        }
    }
}

/// The Secrets section between frames.
#[derive(Default)]
pub(super) struct SecretsPanel {
    pub(super) names: Option<Vec<String>>,
    pub(super) name: String,
    /// Masked; emptied as soon as Add or Replace is pressed.
    pub(super) value: String,
    /// The name the delete confirm is open for.
    pub(super) deleting: Option<String>,
    pub(super) busy: bool,
    pub(super) note: Option<String>,
    focus_value: bool,
    ws: Option<PathBuf>,
    rx: Option<mpsc::Receiver<SecretOut>>,
}

impl SecretsPanel {
    pub(super) fn replacing(&self) -> bool {
        let typed = self.name.trim();
        self.names
            .as_ref()
            .is_some_and(|n| n.iter().any(|x| x == typed))
    }

    pub(super) fn can_save(&self) -> bool {
        !self.busy && !self.name.trim().is_empty() && !self.value.trim().is_empty()
    }

    /// Add or Replace: the value leaves the panel now, saved or not.
    pub(super) fn save(&mut self) -> Option<SecretJob> {
        if !self.can_save() {
            return None;
        }
        self.busy = true;
        self.note = None;
        let value = std::mem::take(&mut self.value);
        Some(SecretJob::Set {
            name: self.name.trim().to_string(),
            value,
        })
    }

    /// A row's Delete only opens the confirm; nothing is removed yet.
    pub(super) fn ask_delete(&mut self, name: &str) {
        self.deleting = Some(name.to_string());
    }

    pub(super) fn confirm_delete(&mut self) -> Option<SecretJob> {
        if self.busy {
            return None;
        }
        let name = self.deleting.take()?;
        self.busy = true;
        self.note = None;
        Some(SecretJob::Delete { name })
    }

    pub(super) fn cancel_delete(&mut self) {
        self.deleting = None;
    }

    /// A row's Replace: its name in the field, the value field empty and focused.
    pub(super) fn replace(&mut self, name: &str) {
        self.name = name.to_string();
        self.value.clear();
        self.deleting = None;
        self.focus_value = true;
    }

    pub(super) fn apply(&mut self, out: SecretOut) {
        self.busy = false;
        match out {
            SecretOut::Listed(names) => self.names = Some(names),
            SecretOut::Saved {
                name,
                replaced,
                res,
            } => match res {
                Ok(names) => {
                    self.names = Some(names);
                    // A new name shows in the list; a replaced value changes nothing visible.
                    if replaced {
                        self.note = Some(format!("Replaced {name}'s value."));
                    }
                    self.name.clear();
                }
                Err(e) => self.note = Some(e),
            },
            SecretOut::Deleted(res) => match res {
                Ok(names) => self.names = Some(names),
                Err(e) => self.note = Some(e),
            },
        }
    }

    fn start(&mut self, job: Option<SecretJob>, ctx: &egui::Context, config_dir: &Path, ws: &Path) {
        let Some(job) = job else {
            return;
        };
        if self.rx.is_some() {
            return;
        }
        let (tx, rx) = mpsc::channel();
        self.rx = Some(rx);
        let ctx = ctx.clone();
        let config = config_dir.to_path_buf();
        let ws = ws.to_path_buf();
        std::thread::spawn(move || {
            let _ = tx.send(job.run(&config, &ws, &secrets::Keychain));
            ctx.request_repaint();
        });
    }

    fn poll(&mut self) {
        let Some(rx) = self.rx.take() else {
            return;
        };
        match rx.try_recv() {
            Ok(out) => self.apply(out),
            Err(mpsc::TryRecvError::Empty) => self.rx = Some(rx),
            Err(mpsc::TryRecvError::Disconnected) => {
                self.busy = false;
                self.note = Some("That stopped before it finished. Try again.".into());
            }
        }
    }

    pub(super) fn ui(&mut self, ui: &mut egui::Ui, config_dir: &Path, ws: &Path) {
        let ctx = ui.ctx().clone();
        if self.ws.as_deref() != Some(ws) {
            *self = Self {
                ws: Some(ws.to_path_buf()),
                busy: true,
                ..Self::default()
            };
            self.start(Some(SecretJob::List), &ctx, config_dir, ws);
        }
        self.poll();

        crate::cards::section_label(ui, SECRETS_TITLE);
        muted(ui, SECRETS_NOTE);
        ui.add_space(8.0);
        let mut job = None;
        match self.names.clone() {
            None if self.note.is_none() => muted(ui, "Loading…"),
            None => {}
            Some(names) if names.is_empty() => muted(ui, SECRETS_NONE),
            Some(names) => {
                for n in names {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(&n).monospace().color(crate::theme::fg()));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if !self.busy && crate::cards::ghost_pill(ui, "Delete") {
                                self.ask_delete(&n);
                            }
                            if !self.busy && crate::cards::ghost_pill(ui, "Replace") {
                                self.replace(&n);
                            }
                        });
                    });
                    ui.add_space(4.0);
                }
            }
        }
        if let Some(target) = self.deleting.clone() {
            ui.add_space(8.0);
            match paint_confirm_sheet(ui, delete_spec(), &target) {
                Some(ConfirmAct::Confirm) => job = self.confirm_delete(),
                Some(ConfirmAct::Cancel) => self.cancel_delete(),
                None => {}
            }
        }
        ui.add_space(8.0);
        let mut enter = false;
        ui.horizontal(|ui| {
            let r = ui.add_enabled(
                !self.busy,
                egui::TextEdit::singleline(&mut self.name)
                    .hint_text(NAME_HINT)
                    .font(egui::TextStyle::Monospace)
                    .desired_width(200.0),
            );
            enter |= r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            let r = ui.add_enabled(
                !self.busy,
                egui::TextEdit::singleline(&mut self.value)
                    .password(true)
                    .hint_text("Value")
                    .desired_width(240.0),
            );
            if std::mem::take(&mut self.focus_value) {
                r.request_focus();
            }
            enter |= r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            let label = if self.replacing() { "Replace" } else { "Add" };
            if self.can_save() && crate::cards::white_pill(ui, label) {
                enter = true;
            }
        });
        if enter {
            job = job.or_else(|| self.save());
        }
        if let Some(note) = &self.note {
            ui.add_space(8.0);
            muted(ui, note);
        }
        self.start(job, &ctx, config_dir, ws);
    }
}

fn muted(ui: &mut egui::Ui, text: &str) {
    ui.add(
        egui::Label::new(
            RichText::new(text)
                .size(crate::theme::TYPE_CAPTION)
                .color(crate::theme::muted()),
        )
        .wrap(),
    );
}

#[cfg(test)]
#[path = "code_secrets_ui_tests.rs"]
mod tests;
