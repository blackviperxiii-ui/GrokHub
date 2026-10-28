//! Settings → Playbooks: a card per playbook (shipped, edited or the user's own) with
//! View / Edit, Duplicate and Reset or Remove. The editor is the playbook's TOML; Save
//! refuses a playbook that breaks the format or the hard-class lock and says why.
//! Edited and custom playbooks live in `{config}/playbooks/`.

use super::*;
use grokhub_agent::brain::playbook::{self as pb, Loaded, Source};

/// Settings → Playbooks state.
#[derive(Default)]
pub(super) struct PlaybookState {
    /// Shipped playbooks with the user's on top. Read when the tab first opens and
    /// again after every save, duplicate, reset or remove.
    pub all: Vec<Loaded>,
    /// User files that could not be used, each naming the file and why.
    pub problems: Vec<String>,
    pub loaded: bool,
    pub draft: Option<PlaybookDraft>,
}

/// A playbook open in the editor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct PlaybookDraft {
    pub id: String,
    pub name: String,
    pub text: String,
    /// Why the last Save was refused.
    pub error: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PlaybookAct {
    Edit,
    Duplicate,
    Reset,
}

fn source_chip(source: Source) -> Option<(&'static str, crate::cards::ChipTone)> {
    match source {
        Source::Shipped => None,
        Source::Edited => Some(("Edited", crate::cards::ChipTone::Setup)),
        Source::Yours => Some(("Yours", crate::cards::ChipTone::Mute)),
    }
}

/// "Fits repair · linux" for a card, or "Fits any task" when it names nothing.
fn fits_line(l: &Loaded) -> String {
    let a = &l.playbook.applies_when;
    let parts: Vec<String> = [&a.intent, &a.domain, &a.os]
        .into_iter()
        .filter(|v| !v.is_empty())
        .map(|v| v.join(", "))
        .collect();
    if parts.is_empty() {
        "Fits any task".into()
    } else {
        format!("Fits {}", parts.join(" · "))
    }
}

/// "3 steps, up to Safe, reversible"
fn steps_line(l: &Loaded) -> String {
    let tiers: Vec<_> = l
        .playbook
        .steps()
        .map(|(phase, s)| s.tier_in(phase))
        .collect();
    let top = tiers.iter().max();
    let n = tiers.len();
    let noun = if n == 1 { "step" } else { "steps" };
    match top {
        Some(t) => format!("{n} {noun}, up to {}", t.label()),
        None => format!("{n} {noun}"),
    }
}

impl Cabin {
    fn reload_playbooks(&mut self) {
        let (all, problems) = pb::load(&crate::config::config_dir());
        self.playbooks.all = all;
        self.playbooks.problems = problems;
        self.playbooks.loaded = true;
    }

    fn open_playbook(&mut self, id: &str) {
        if let Some(l) = self.playbooks.all.iter().find(|l| l.playbook.id == id) {
            self.playbooks.draft = Some(PlaybookDraft {
                id: l.playbook.id.clone(),
                name: l.playbook.name.clone(),
                text: l.text.clone(),
                error: String::new(),
            });
        }
    }

    /// Save the editor's text. A refused save keeps the editor open with the reason.
    fn save_playbook_draft(&mut self) {
        let Some(draft) = self.playbooks.draft.as_mut() else {
            return;
        };
        match pb::save(&crate::config::config_dir(), &draft.text) {
            Ok(p) => {
                self.status = format!("Saved playbook {}", p.name);
                self.playbooks.draft = None;
                self.reload_playbooks();
            }
            Err(e) => draft.error = format!("Not saved: {e}"),
        }
    }

    fn act_on_playbook(&mut self, act: PlaybookAct, id: &str) {
        let dir = crate::config::config_dir();
        match act {
            PlaybookAct::Edit => self.open_playbook(id),
            PlaybookAct::Duplicate => match pb::duplicate(&dir, id) {
                Ok(p) => {
                    self.status = format!("Made {}", p.name);
                    self.reload_playbooks();
                    self.open_playbook(&p.id);
                }
                Err(e) => self.status = format!("Couldn't duplicate: {e}"),
            },
            PlaybookAct::Reset => match pb::reset(&dir, id) {
                Ok(()) => {
                    self.status = format!("Playbook {id} reset");
                    if self.playbooks.draft.as_ref().is_some_and(|d| d.id == id) {
                        self.playbooks.draft = None;
                    }
                    self.reload_playbooks();
                }
                Err(e) => self.status = format!("Couldn't reset: {e}"),
            },
        }
    }

    pub(super) fn ui_settings_playbooks(&mut self, ui: &mut egui::Ui) {
        if !self.playbooks.loaded {
            self.reload_playbooks();
        }
        crate::cards::help_text(
            ui,
            "How the agent works through a task: look first, then safe steps, then risky ones, with a backup, a check and a way back. Risky steps still ask you first; a playbook never changes an approval.",
        );
        ui.add_space(10.0);
        if self.playbooks.draft.is_some() {
            self.ui_playbook_editor(ui);
            ui.add_space(16.0);
        }
        for problem in self.playbooks.problems.clone() {
            crate::cards::settings_note(ui, &format!("Skipped {problem}"));
        }
        let mut act_on: Option<(PlaybookAct, String)> = None;
        for l in &self.playbooks.all {
            if let Some(act) = paint_playbook_card(ui, l) {
                act_on = Some((act, l.playbook.id.clone()));
            }
            ui.add_space(10.0);
        }
        if let Some((act, id)) = act_on {
            self.act_on_playbook(act, &id);
        }
    }

    fn ui_playbook_editor(&mut self, ui: &mut egui::Ui) {
        let Some(mut draft) = self.playbooks.draft.clone() else {
            return;
        };
        let mut save = false;
        let mut close = false;
        egui::Frame::NONE
            .fill(crate::theme::surface())
            .corner_radius(12.0)
            .stroke(egui::Stroke::new(1.0_f32, crate::theme::fg()))
            .inner_margin(egui::Margin::same(14))
            .show(ui, |ui| {
                crate::cards::section_label(ui, &format!("Edit {}", draft.name));
                crate::cards::help_text(
                    ui,
                    "Saved as playbooks/<id>.toml in the cabin's settings folder. Steps go look-only, then safe, then risky. A step that deletes, sends, pays, touches sign-ins or can't be undone must be risky, and commands GrokHub never runs are refused.",
                );
                ui.add_space(6.0);
                ui.add(
                    egui::TextEdit::multiline(&mut draft.text)
                        .code_editor()
                        .desired_rows(18)
                        .desired_width(f32::INFINITY),
                );
                ui.add_space(8.0);
                if !draft.error.is_empty() {
                    ui.label(RichText::new(&draft.error).size(crate::theme::FONT_META).color(crate::theme::offline()));
                    ui.add_space(6.0);
                }
                ui.horizontal(|ui| {
                    if crate::cards::white_pill(ui, "Save") {
                        save = true;
                    }
                    if crate::cards::ghost_pill(ui, "Cancel") {
                        close = true;
                    }
                });
            });
        if close {
            self.playbooks.draft = None;
            return;
        }
        self.playbooks.draft = Some(draft);
        if save {
            self.save_playbook_draft();
        }
    }
}

fn paint_playbook_card(ui: &mut egui::Ui, l: &Loaded) -> Option<PlaybookAct> {
    let mut act = None;
    egui::Frame::NONE
        .fill(crate::theme::surface())
        .corner_radius(12.0)
        .stroke(egui::Stroke::new(1.0_f32, crate::theme::border()))
        .inner_margin(egui::Margin::same(12))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(&l.playbook.name)
                        .size(crate::theme::FONT_BODY)
                        .strong()
                        .color(crate::theme::fg()),
                );
                if let Some((label, tone)) = source_chip(l.source) {
                    crate::cards::status_chip(ui, label, tone);
                }
            });
            ui.label(
                RichText::new(&l.playbook.summary)
                    .size(crate::theme::FONT_META)
                    .color(crate::theme::muted()),
            );
            ui.label(
                RichText::new(format!("{} · {}", fits_line(l), steps_line(l)))
                    .size(crate::theme::FONT_META)
                    .color(crate::theme::muted()),
            );
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                let edit = if l.source == Source::Shipped {
                    "View / Edit"
                } else {
                    "Edit"
                };
                if crate::cards::ghost_pill(ui, edit) {
                    act = Some(PlaybookAct::Edit);
                }
                if crate::cards::ghost_pill(ui, "Duplicate") {
                    act = Some(PlaybookAct::Duplicate);
                }
                let reset = match l.source {
                    Source::Shipped => None,
                    Source::Edited => Some("Reset"),
                    Source::Yours => Some("Remove"),
                };
                if let Some(label) = reset {
                    if crate::cards::ghost_pill(ui, label) {
                        act = Some(PlaybookAct::Reset);
                    }
                }
            });
        });
    act
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cabin() -> (
        std::sync::MutexGuard<'static, ()>,
        crate::config::TestConfigDir,
        Cabin,
    ) {
        let lock = crate::config::hold_test_config();
        let root = crate::config::test_config_root("playbook-ui");
        std::fs::create_dir_all(&root).unwrap();
        let pin = crate::config::TestConfigDir::set(root);
        (lock, pin, Cabin::quiet_for_test())
    }

    #[test]
    fn a_refused_save_keeps_the_editor_open_and_a_good_one_shows_as_edited() {
        let (_lock, _root, mut cabin) = cabin();
        cabin.reload_playbooks();
        assert_eq!(cabin.playbooks.all.len(), 7);
        assert_eq!(
            steps_line(&cabin.playbooks.all[0]),
            "3 steps, up to Safe, reversible"
        );
        assert_eq!(fits_line(&cabin.playbooks.all[0]), "Fits task, repair");
        assert_eq!(
            fits_line(&cabin.playbooks.all[1]),
            "Fits repair · system · linux"
        );
        assert_eq!(
            steps_line(&cabin.playbooks.all[1]),
            "19 steps, up to Risky, asks first"
        );
        assert_eq!(fits_line(&cabin.playbooks.all[6]), "Fits task · desktop");
        assert_eq!(fits_line(&cabin.playbooks.all[5]), "Fits research");
        cabin.act_on_playbook(PlaybookAct::Edit, "task-general");
        let draft = cabin.playbooks.draft.as_mut().unwrap();
        let good = draft
            .text
            .replace("name = \"Any task\"", "name = \"Any job\"");
        draft.text = good.replacen(
            "name = \"Check the change before the next one\"",
            "name = \"Check the change before the next one\"\n\n[[phase.step]]\nname = \"Clear it\"\nrun = \"rm -rf ~/old\"",
            1,
        );
        cabin.save_playbook_draft();
        assert_eq!(
            cabin.playbooks.draft.as_ref().map(|d| d.error.as_str()),
            Some("Not saved: step \"Clear it\" is a hard-class action (Delete) and must be marked risky; it still asks you first")
        );
        assert_eq!(cabin.playbooks.all[0].source, Source::Shipped);
        cabin.playbooks.draft.as_mut().unwrap().text = good;
        cabin.save_playbook_draft();
        assert_eq!(cabin.playbooks.draft, None);
        assert_eq!(cabin.status, "Saved playbook Any job");
        assert_eq!(
            (
                cabin.playbooks.all[0].playbook.name.as_str(),
                cabin.playbooks.all[0].source
            ),
            ("Any job", Source::Edited)
        );
        // Duplicate opens the copy; Reset puts the shipped one back.
        cabin.act_on_playbook(PlaybookAct::Duplicate, "task-general");
        assert_eq!(
            cabin.playbooks.draft.as_ref().map(|d| d.id.as_str()),
            Some("task-general-copy")
        );
        cabin.act_on_playbook(PlaybookAct::Reset, "task-general");
        let rows: Vec<(&str, &str, Source)> = cabin
            .playbooks
            .all
            .iter()
            .map(|l| (l.playbook.id.as_str(), l.playbook.name.as_str(), l.source))
            .collect();
        assert_eq!(
            rows,
            vec![
                ("task-general", "Any task", Source::Shipped),
                ("repair-linux", "Repair Linux", Source::Shipped),
                ("repair-windows", "Repair Windows", Source::Shipped),
                ("code-dev", "Code changes", Source::Shipped),
                ("files", "Files", Source::Shipped),
                ("research", "Research", Source::Shipped),
                ("desktop", "Desktop apps", Source::Shipped),
                ("task-general-copy", "Any job (copy)", Source::Yours)
            ]
        );
    }
}
