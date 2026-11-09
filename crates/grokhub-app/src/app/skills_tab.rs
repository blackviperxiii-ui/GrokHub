//! The Skills tab's own state and rows: an on/off switch per skill, Edit and
//! Remove for the ones this computer keeps, and New skill. Built-in skills
//! are not listed here; they still run by slash and belong to the Code tab.

use eframe::egui::{self, RichText};
use grokhub_agent::harness as hx;
use grokhub_core::SkillMd;

use super::Cabin;
use crate::{config, skills};

/// What the Skills tab holds between frames.
#[derive(Default)]
pub(super) struct SkillsTab {
    /// The search box, shared with Settings → Connectors' skill list.
    pub(super) q: String,
    /// Every cabin skill, off ones too. `None` reads them again.
    all: Option<Vec<SkillMd>>,
    new_open: bool,
    new_name: String,
    new_what: String,
    edit: Option<SkillEdit>,
    /// The skill just removed, so its Undo stays in reach.
    removed: Option<String>,
}

/// The fields Edit changes, in the words the tab shows.
#[derive(Clone)]
struct SkillEdit {
    name: String,
    what: String,
    when: String,
    steps: String,
}

/// One press on a skill row.
pub(super) enum RowAct {
    Toggle,
    Use,
    Edit,
    Remove,
}

/// The project, user and plugin skills the Skills tab lists. Built-ins live
/// in the Code tab, and cabin skills have their own section.
pub(super) fn listed_native(native: &[grokhub_agent::Skill]) -> Vec<grokhub_agent::Skill> {
    native
        .iter()
        .filter(|s| {
            s.source != grokhub_agent::SkillSource::Builtin
                && s.source != grokhub_agent::SkillSource::Cabin
        })
        .cloned()
        .collect()
}

/// A row's second line: on or off, how often it ran, what it does.
pub(super) fn skill_row_body(on: bool, runs: Option<u32>, what: &str) -> String {
    let mut parts: Vec<String> = Vec::new();
    if !on {
        parts.push("Off, chat won't use it".into());
    }
    match runs {
        Some(0) => parts.push("never run".into()),
        Some(1) => parts.push("ran once".into()),
        Some(n) => parts.push(format!("ran {n} times")),
        None => {}
    }
    let what = what.trim();
    if !what.is_empty() {
        parts.push(what.to_string());
    }
    parts.join(" · ")
}

/// One skill as a card: switch, name and line, then its buttons.
pub(super) fn skill_row(
    ui: &mut egui::Ui,
    name: &str,
    body: &str,
    on: bool,
    editable: bool,
) -> Option<RowAct> {
    let mut act = None;
    egui::Frame::NONE
        .fill(crate::theme::elevated())
        .corner_radius(crate::theme::CARD_RADIUS)
        .stroke(egui::Stroke::new(1.0_f32, crate::theme::border()))
        .inner_margin(egui::Margin::same(12))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                if crate::cards::enabled_switch(ui, on) {
                    act = Some(RowAct::Toggle);
                }
                ui.vertical(|ui| {
                    ui.set_max_width((ui.available_width() - 220.0).max(120.0));
                    ui.add(
                        egui::Label::new(
                            RichText::new(name)
                                .size(crate::theme::TYPE_UI)
                                .color(crate::theme::fg()),
                        )
                        .wrap(),
                    );
                    ui.add(
                        egui::Label::new(
                            RichText::new(body)
                                .size(crate::theme::TYPE_CAPTION)
                                .color(crate::theme::muted()),
                        )
                        .wrap(),
                    );
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if editable {
                        if crate::cards::ghost_pill(ui, "Remove") {
                            act = Some(RowAct::Remove);
                        }
                        if crate::cards::ghost_pill(ui, "Edit") {
                            act = Some(RowAct::Edit);
                        }
                    }
                    if on && crate::cards::white_pill(ui, "Use") {
                        act = Some(RowAct::Use);
                    }
                });
            });
        });
    ui.add_space(8.0);
    act
}

fn field(ui: &mut egui::Ui, title: &str, hint: &str, value: &mut String, lines: usize) {
    ui.label(
        RichText::new(title)
            .size(crate::theme::TYPE_UI)
            .color(crate::theme::fg()),
    );
    if !hint.is_empty() {
        ui.label(
            RichText::new(hint)
                .size(crate::theme::TYPE_CAPTION)
                .color(crate::theme::muted()),
        );
    }
    ui.add_space(4.0);
    ui.add(
        egui::TextEdit::multiline(value)
            .desired_rows(lines)
            .desired_width(f32::INFINITY),
    );
    ui.add_space(8.0);
}

impl Cabin {
    /// Every cabin skill for the tab, read once until something changes.
    pub(super) fn skills_tab_all(&mut self) -> Vec<SkillMd> {
        if self.skills_tab.all.is_none() {
            self.skills_tab.all = Some(skills::list_all_skills());
        }
        self.skills_tab.all.clone().unwrap_or_default()
    }

    /// After any change: the tab, chat's own list and the engine's listing
    /// all read the skills again.
    pub(super) fn skills_tab_changed(&mut self) {
        self.skills_tab.all = None;
        self.skill_list = skills::list_skills();
        self.native_listing_cwd.clear();
    }

    /// The New skill button and, once pressed, its two fields.
    pub(super) fn skills_tab_new(&mut self, ui: &mut egui::Ui) {
        if !self.skills_tab.new_open {
            return;
        }
        crate::cards::section_label(ui, "New skill");
        let tab = &mut self.skills_tab;
        field(
            ui,
            "Name",
            "A few words, like Clip trimmer.",
            &mut tab.new_name,
            1,
        );
        field(
            ui,
            "What it should do",
            "In your own words. You can add steps after with Edit.",
            &mut tab.new_what,
            3,
        );
        let mut save = false;
        let mut cancel = false;
        ui.horizontal(|ui| {
            save = crate::cards::white_pill(ui, "Save skill");
            cancel = crate::cards::ghost_pill(ui, "Cancel");
        });
        ui.add_space(12.0);
        if cancel {
            self.skills_tab.new_open = false;
            return;
        }
        if save {
            match skills::new_skill(&self.skills_tab.new_name, &self.skills_tab.new_what) {
                Ok(made) => {
                    self.status = format!("Made {}. Chat uses it when you ask for it.", made.name);
                    self.skills_tab.new_open = false;
                    self.skills_tab.new_name.clear();
                    self.skills_tab.new_what.clear();
                    self.skills_tab_changed();
                }
                Err(e) => self.status = e,
            }
        }
    }

    /// The edit form under the skill being edited, if any.
    fn skills_tab_edit_form(&mut self, ui: &mut egui::Ui, name: &str) {
        let Some(edit) = self.skills_tab.edit.as_mut().filter(|e| e.name == name) else {
            return;
        };
        field(ui, "What it does", "", &mut edit.what, 2);
        field(
            ui,
            "When to use it",
            "The kind of ask that should pick it.",
            &mut edit.when,
            2,
        );
        field(
            ui,
            "Steps",
            "What to do, one step a line.",
            &mut edit.steps,
            6,
        );
        let mut save = false;
        let mut cancel = false;
        ui.horizontal(|ui| {
            save = crate::cards::white_pill(ui, "Save changes");
            cancel = crate::cards::ghost_pill(ui, "Cancel");
        });
        ui.add_space(12.0);
        if cancel {
            self.skills_tab.edit = None;
            return;
        }
        if !save {
            return;
        }
        let Some(edit) = self.skills_tab.edit.take() else {
            return;
        };
        let Some(mut skill) = self
            .skills_tab_all()
            .into_iter()
            .find(|s| s.name == edit.name)
        else {
            self.status = format!("{} is gone.", edit.name);
            return;
        };
        skill.description = edit.what.trim().to_string();
        skill.trigger = edit.when.trim().to_string();
        skill.instructions = edit.steps.trim().to_string();
        match skills::save_skill_logged(&skill, hx::Origin::User, "edited on the Skills tab") {
            Ok(()) => {
                self.status = format!("Saved {}.", skill.name);
                self.skills_tab_changed();
            }
            Err(e) => self.status = e,
        }
    }

    /// The cabin skills, one row each, with the edit form under the one open.
    /// Returns the slash and name of a skill to use in chat.
    pub(super) fn skills_tab_cabin_rows(
        &mut self,
        ui: &mut egui::Ui,
        rows: &[SkillMd],
    ) -> Option<(String, String)> {
        let mut use_it = None;
        for s in rows {
            let on = !skills::skill_is_off(&s.name);
            let body = skill_row_body(on, Some(s.runs), &s.description);
            match skill_row(ui, &s.name, &body, on, true) {
                Some(RowAct::Toggle) => self.skills_tab_toggle(&s.name, !on),
                Some(RowAct::Use) => use_it = Some((s.slash.clone(), s.name.clone())),
                Some(RowAct::Edit) => {
                    self.skills_tab.edit = Some(SkillEdit {
                        name: s.name.clone(),
                        what: s.description.clone(),
                        when: s.trigger.clone(),
                        steps: s.instructions.clone(),
                    });
                }
                Some(RowAct::Remove) => self.skills_tab_remove(&s.name),
                None => {}
            }
            self.skills_tab_edit_form(ui, &s.name);
        }
        use_it
    }

    pub(super) fn skills_tab_toggle(&mut self, name: &str, on: bool) {
        match skills::set_skill_on(name, on) {
            Ok(()) => {
                self.status = if on {
                    format!("{name} is on.")
                } else {
                    format!("{name} is off. Chat won't use it until you turn it back on.")
                };
                self.skills_tab_changed();
            }
            Err(e) => self.status = e,
        }
    }

    fn skills_tab_remove(&mut self, name: &str) {
        match skills::remove_skill(name) {
            Ok(()) => {
                self.status = format!("Removed {name}.");
                self.skills_tab.removed = Some(name.to_string());
                if self
                    .skills_tab
                    .edit
                    .as_ref()
                    .is_some_and(|e| e.name == name)
                {
                    self.skills_tab.edit = None;
                }
                self.skills_tab_changed();
            }
            Err(e) => self.status = e,
        }
    }

    /// "Removed X. Undo" under the header, until it's used or a new removal.
    pub(super) fn skills_tab_undo_line(&mut self, ui: &mut egui::Ui) {
        let Some(name) = self.skills_tab.removed.clone() else {
            return;
        };
        let mut undo = false;
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(format!("Removed {name}."))
                    .size(crate::theme::FONT_BODY)
                    .color(crate::theme::muted()),
            );
            undo = crate::cards::ghost_pill(ui, "Undo");
        });
        ui.add_space(8.0);
        if !undo {
            return;
        }
        self.skills_tab.removed = None;
        let res = hx::undo_skill_change(
            &config::config_dir(),
            &skills::skills_dir(),
            &grokhub_core::skill_dir_name(&name),
            hx::UndoAsk::from_click(),
        );
        self.status = match res {
            Ok(_) => format!("{name} is back."),
            Err(e) => e,
        };
        self.skills_tab_changed();
    }

    /// The New skill button in the header row.
    pub(super) fn skills_tab_new_button(&mut self, ui: &mut egui::Ui) {
        if !self.skills_tab.new_open && crate::cards::white_pill(ui, "New skill") {
            self.skills_tab.new_open = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn native(name: &str, source: grokhub_agent::SkillSource) -> grokhub_agent::Skill {
        grokhub_agent::Skill {
            name: name.into(),
            description: String::new(),
            allowed_tools: Vec::new(),
            source,
            path: std::path::PathBuf::new(),
            dir: std::path::PathBuf::new(),
        }
    }

    #[test]
    fn the_skills_tab_leaves_out_built_ins_and_lists_cabin_skills_once() {
        use grokhub_agent::SkillSource as S;
        let all = vec![
            native("qa-walkthrough", S::Builtin),
            native("clip-trimmer", S::Cabin),
            native("notes", S::Project),
            native("mine", S::User),
            native("from-plugin", S::Plugin),
        ];
        let names: Vec<String> = listed_native(&all).into_iter().map(|s| s.name).collect();
        assert_eq!(names, vec!["notes", "mine", "from-plugin"]);
    }

    #[test]
    fn a_row_says_plainly_when_a_skill_is_off() {
        assert_eq!(
            skill_row_body(true, Some(0), "Trim clips"),
            "never run · Trim clips"
        );
        assert_eq!(skill_row_body(true, Some(1), ""), "ran once");
        assert_eq!(
            skill_row_body(false, Some(3), "Trim clips"),
            "Off, chat won't use it · ran 3 times · Trim clips"
        );
        assert_eq!(skill_row_body(true, None, " Notes "), "Notes");
    }
}
