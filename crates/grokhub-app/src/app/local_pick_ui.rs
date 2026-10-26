//! Choosing the on-device model: the first-run wizard and Settings both show
//! the catalog as a dropdown (the hardware's suggestion marked recommended),
//! then Download, Use (already downloaded) or what's running now. Other
//! downloaded models can be removed. A switch records the pick in
//! `local_ai.json` (the shell's Settings → Local AI reads the same file) and
//! stops the running engine so the next request loads the new file.

use grokhub_agent::route::local_sidecar;
use grokhub_core::local_setup::{self as ls, Hardware, LocalModel};
use grokhub_core::model_download::{self as md, Installed};

use super::setup_wizard::models_dir;
use super::*;

/// The models a person can pick: the catalog without draft models.
pub(super) fn choices() -> Vec<&'static LocalModel> {
    ls::local_models()
        .iter()
        .filter(|m| !m.is_drafter)
        .collect()
}

/// One dropdown line: name, download size, memory it needs, and whether it is
/// the one this machine should start with.
pub(super) fn choice_label(m: &LocalModel, suggested: Option<&LocalModel>) -> String {
    let mut line = format!(
        "{} · {} download · needs about {} memory",
        m.name,
        ls::size_label(m.download_mb),
        ls::size_label(m.run_mb)
    );
    if suggested.is_some_and(|s| s.id == m.id) {
        line.push_str(" · recommended");
    }
    line
}

/// The model the dropdown starts on: the one picked this session, the one
/// running, the hardware's suggestion, then the first catalog entry.
pub(super) fn default_choice(
    picked: Option<&str>,
    installed: Option<&Installed>,
    suggested: Option<&'static LocalModel>,
) -> Option<&'static LocalModel> {
    picked
        .or(installed.map(|r| r.id.as_str()))
        .and_then(ls::model_by_id)
        .filter(|m| !m.is_drafter)
        .or(suggested)
        .or_else(|| choices().into_iter().next())
}

impl Cabin {
    fn suggested_model(&self) -> Option<&'static LocalModel> {
        self.setup
            .hw
            .as_ref()
            .and_then(|hw: &Hardware| ls::suggest(hw))
    }

    /// The model dropdown; returns the model it shows.
    pub(super) fn ui_model_choice(&mut self, ui: &mut egui::Ui) -> Option<&'static LocalModel> {
        let suggested = self.suggested_model();
        let current = default_choice(
            self.setup.choice.as_deref(),
            self.setup.installed.as_ref(),
            suggested,
        )?;
        let all = choices();
        let labels: Vec<String> = all.iter().map(|m| choice_label(m, suggested)).collect();
        let shown = choice_label(current, suggested);
        let hint = "Pick before you download. You can switch later in Settings without reinstalling GrokHub.";
        if let Some(i) = crate::cards::settings_dropdown(ui, "Local model", hint, &shown, &labels) {
            self.setup.choice = Some(all[i].id.to_string());
            return Some(all[i]);
        }
        Some(current)
    }

    /// Download, Use or the running note for `m`, then the other downloaded models.
    pub(super) fn ui_model_pick_rows(&mut self, ui: &mut egui::Ui, m: &'static LocalModel) {
        let running = self.setup.installed.as_ref().is_some_and(|r| r.id == m.id);
        if running {
            self.ui_model_download(ui, m);
        } else if self.setup.dl.is_some() {
            self.ui_download_progress(ui);
        } else if md::final_path(&models_dir(), m).is_file() {
            if crate::cards::white_pill(ui, &format!("Use {}", m.name)) {
                self.switch_local_model(m);
            }
        } else {
            self.ui_download_button(ui, m);
        }
        let installed = self.setup.installed.as_ref().map(|r| r.id.clone());
        for other in md::downloaded(&models_dir()) {
            if Some(other.id) == installed.as_deref() || other.id == m.id {
                continue;
            }
            ui.horizontal(|ui| {
                crate::cards::settings_note(ui, &format!("{} is downloaded too.", other.name));
                if crate::cards::ghost_pill(ui, &format!("Remove {}", other.name)) {
                    self.remove_local_model(other);
                }
            });
        }
    }

    /// Run `m`, already downloaded, instead of the current model.
    pub(super) fn switch_local_model(&mut self, m: &'static LocalModel) {
        match md::activate(&models_dir(), m) {
            Ok(rec) => {
                self.adopt_local_model(rec);
                self.status = format!("Switched the local model to {}.", m.name);
            }
            Err(e) => self.status = e,
        }
    }

    /// A model just became the installed one (switched to or downloaded):
    /// record the pick, stop the old engine, and turn the route on.
    pub(super) fn adopt_local_model(&mut self, rec: Installed) {
        let id = rec.id.clone();
        self.setup.installed = Some(rec);
        self.setup.choice = Some(id.clone());
        if let Err(e) = local_sidecar::set_pick(&crate::config::config_dir(), &id) {
            self.status = e;
        }
        local_sidecar::stop_global();
        self.set_local_model(true);
    }

    pub(super) fn remove_local_model(&mut self, m: &'static LocalModel) {
        self.status = match md::remove(&models_dir(), m) {
            Ok(()) => format!("Removed {} from {}.", m.name, models_dir().display()),
            Err(e) => e,
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_dropdown_marks_the_suggestion_and_starts_on_the_running_model() {
        let all = choices();
        assert!(all.len() >= 2, "the catalog offers a choice");
        assert!(all.iter().all(|m| !m.is_drafter));
        let (a, b) = (all[0], all[1]);
        assert_eq!(
            choice_label(a, Some(a)),
            format!(
                "{} · {} download · needs about {} memory · recommended",
                a.name,
                ls::size_label(a.download_mb),
                ls::size_label(a.run_mb)
            )
        );
        assert!(!choice_label(b, Some(a)).ends_with("recommended"));
        let running = Installed {
            id: b.id.into(),
            file: b.file.into(),
            sha256: String::new(),
            bytes: 1,
        };
        assert_eq!(
            default_choice(None, Some(&running), Some(a)).map(|m| m.id),
            Some(b.id)
        );
        assert_eq!(
            default_choice(Some(a.id), Some(&running), None).map(|m| m.id),
            Some(a.id)
        );
        assert_eq!(
            default_choice(None, None, Some(b)).map(|m| m.id),
            Some(b.id)
        );
        assert_eq!(default_choice(None, None, None).map(|m| m.id), Some(a.id));
        // A file the catalog no longer lists falls through to the suggestion.
        let old = Installed {
            id: "retired-model".into(),
            ..running
        };
        assert_eq!(
            default_choice(None, Some(&old), Some(b)).map(|m| m.id),
            Some(b.id)
        );
    }
}
