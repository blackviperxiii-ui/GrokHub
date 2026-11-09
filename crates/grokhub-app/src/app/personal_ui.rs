//! Personal: everything the app has learned about the user, in one place.
//! Grouped, searchable rows with where each came from and when; Edit, pause
//! ("Don't use") and Forget per row; add a line in your own words; export
//! to one readable file; and Wipe, after a confirm. Everything stays on this
//! computer. Account and app settings are a link away.

use eframe::egui::{self, RichText};
use grokhub_core::amr::{self, MemoryRow, PersonalGroup, UserForget};

use super::{now_ms, Cabin, Nav};
use crate::config;

/// What Personal holds between frames.
#[derive(Default)]
pub(super) struct PersonalPage {
    q: String,
    /// The rows, read once until something changes.
    rows: Option<Vec<MemoryRow>>,
    add_text: String,
    add_group: Option<PersonalGroup>,
    /// The row being edited and its draft text.
    edit: Option<(String, String)>,
    /// Wipe was pressed once; the confirm panel is up.
    wipe_ask: bool,
}

/// One press on a row.
enum RowAct {
    Edit,
    Pause(bool),
    Forget,
    Approve,
    Reject,
}

/// A row's second line: paused or waiting, then where it came from and when.
pub(super) fn personal_row_meta(row: &MemoryRow) -> String {
    let day: String = row.updated.chars().take(10).collect();
    let mut out = String::new();
    if row.paused {
        out.push_str("Paused, not used · ");
    } else if row.proposed {
        out.push_str("Waiting for your OK · ");
    }
    out.push_str(&row.source.label());
    if !day.is_empty() {
        out.push_str(" · ");
        out.push_str(&day);
    }
    out
}

/// The export file's name in Downloads, dated so a second export never
/// replaces the first.
pub(super) fn export_file_name(now_ms: u64) -> String {
    let day: String = grokhub_core::oauth::unix_ms_to_rfc3339(now_ms)
        .chars()
        .take(10)
        .collect();
    format!(
        "{} - what it knows about you {day}.md",
        grokhub_core::brand::product_name()
    )
}

/// The notes files Wipe empties. SOUL.md is how the app talks, not about you.
const WIPED_NOTES: [&str; 2] = ["USER.md", "MEMORY.md"];

/// Empty USER.md and MEMORY.md back to their heading and drop the `.prev`
/// copies a save keeps, so nothing learned is left in them.
fn empty_notes_files() -> Result<(), String> {
    let dir = config::memory_dir();
    for name in WIPED_NOTES {
        let _ = std::fs::remove_file(dir.join(format!("{name}.prev")));
        if dir.join(name).exists() {
            config::atomic_write(&dir.join(name), format!("# {name}\n").as_bytes())?;
        }
    }
    Ok(())
}

/// One switch row in the settings look; true when it flipped.
fn switch_row(ui: &mut egui::Ui, title: &str, hint: &str, on: &mut bool) -> bool {
    let before = *on;
    crate::ui_kit::setting_row(
        ui,
        title,
        hint,
        crate::ui_kit::SettingRowControl::Switch(on),
    );
    *on != before
}

fn row_card(ui: &mut egui::Ui, row: &MemoryRow) -> Option<RowAct> {
    let mut act = None;
    egui::Frame::NONE
        .fill(crate::theme::elevated())
        .corner_radius(crate::ui_kit::recipe::CARD_RADIUS)
        .stroke(egui::Stroke::new(1.0_f32, crate::theme::border()))
        .inner_margin(egui::Margin::same(12))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.set_max_width((ui.available_width() - 240.0).max(140.0));
                    let color = if row.in_use() {
                        crate::theme::fg()
                    } else {
                        crate::theme::muted()
                    };
                    ui.add(
                        egui::Label::new(
                            RichText::new(&row.text)
                                .size(crate::theme::TYPE_UI)
                                .color(color),
                        )
                        .wrap(),
                    );
                    ui.label(
                        RichText::new(personal_row_meta(row))
                            .size(crate::theme::TYPE_CAPTION)
                            .color(crate::theme::muted()),
                    );
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if row.proposed {
                        if crate::cards::ghost_pill(ui, "No") {
                            act = Some(RowAct::Reject);
                        }
                        if crate::cards::white_pill(ui, "OK") {
                            act = Some(RowAct::Approve);
                        }
                        return;
                    }
                    if crate::cards::ghost_pill(ui, "Forget") {
                        act = Some(RowAct::Forget);
                    }
                    if row.paused {
                        if crate::cards::white_pill(ui, "Use again") {
                            act = Some(RowAct::Pause(false));
                        }
                    } else {
                        if crate::cards::ghost_pill(ui, "Don't use") {
                            act = Some(RowAct::Pause(true));
                        }
                        if crate::cards::ghost_pill(ui, "Edit") {
                            act = Some(RowAct::Edit);
                        }
                    }
                });
            });
        });
    ui.add_space(8.0);
    act
}

impl Cabin {
    fn personal_rows(&mut self) -> Vec<MemoryRow> {
        if self.personal.rows.is_none() {
            let store = self.amr_store(false);
            self.personal.rows = Some(amr::memory_rows(&store).0);
        }
        self.personal.rows.clone().unwrap_or_default()
    }

    fn personal_changed(&mut self) {
        self.personal.rows = None;
    }

    /// The Personal page.
    pub(super) fn ui_personal(&mut self, ui: &mut egui::Ui) {
        let frame = crate::theme::page_frame(ui.ctx());
        let _ = super::dock_ui::page_panel(ui, self.cfg.dock_panels, frame, |ui| {
            if crate::cards::page_header(ui, "Personal", "Refresh") {
                self.personal_changed();
            }
            crate::cards::help_text(
                ui,
                grokhub_core::brand::filled(
                    "What {product} knows about you, and where it learned it. It all stays on this computer.",
                ),
            );
            ui.add_space(12.0);
            self.personal_switches(ui);
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if crate::cards::ghost_pill(ui, "Account and app settings") {
                    self.open_profile();
                }
                if crate::cards::ghost_pill(ui, "Notes files") {
                    self.nav = Nav::Memory;
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    crate::cards::search_field(ui, &mut self.personal.q);
                });
            });
            ui.add_space(12.0);
            egui::ScrollArea::vertical()
                .id_salt("personal")
                .show(ui, |ui| {
                    self.personal_add(ui);
                    self.personal_groups(ui);
                    ui.add_space(16.0);
                    self.personal_export_and_wipe(ui);
                });
        });
    }

    fn personal_switches(&mut self, ui: &mut egui::Ui) {
        let mut learn = self.cfg.memory_writes;
        if switch_row(
            ui,
            "Learn from chats",
            "Off: nothing new is saved. What's here stays, and you can still forget it.",
            &mut learn,
        ) {
            self.cfg.memory_writes = learn;
            self.persist_cfg();
            self.status = if learn {
                "Learning from chats again.".into()
            } else {
                "Not learning from chats. Nothing new is saved.".into()
            };
        }
        let mut know = self.know_me_on();
        if switch_row(
            ui,
            "Know me from this computer",
            "Reads which games and apps you have so answers fit you. Nothing leaves the computer.",
            &mut know,
        ) {
            self.set_know_me_on(know);
        }
    }

    fn personal_add(&mut self, ui: &mut egui::Ui) {
        crate::cards::section_label(ui, "Add something about you");
        let group = self.personal.add_group.unwrap_or(PersonalGroup::About);
        let mut add = false;
        ui.horizontal(|ui| {
            egui::ComboBox::from_id_salt("personal-add-group")
                .selected_text(group.label())
                .show_ui(ui, |ui| {
                    for g in PersonalGroup::ALL {
                        if g != PersonalGroup::Waiting
                            && ui.selectable_label(g == group, g.label()).clicked()
                        {
                            self.personal.add_group = Some(g);
                        }
                    }
                });
            let edit = ui.add(
                egui::TextEdit::singleline(&mut self.personal.add_text)
                    .hint_text("In your words, like: I play after the kids are in bed")
                    .desired_width((ui.available_width() - 80.0).max(160.0)),
            );
            let enter = edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            add = crate::cards::white_pill(ui, "Add") || enter;
        });
        ui.add_space(12.0);
        if !add || self.personal.add_text.trim().is_empty() {
            return;
        }
        let store = self.amr_store(false);
        match amr::add_fact(&store, &self.personal.add_text, group, now_ms()) {
            Ok(_) => {
                self.status = "Added.".into();
                self.personal.add_text.clear();
                self.personal_changed();
            }
            Err(e) => self.status = format!("Couldn't add that: {e}"),
        }
    }

    fn personal_groups(&mut self, ui: &mut egui::Ui) {
        let rows = self.personal_rows();
        if rows.is_empty() {
            ui.label(
                RichText::new(grokhub_core::brand::filled(
                    "Nothing learned yet. {product} adds to this as you chat, or add something above.",
                ))
                .size(crate::theme::FONT_BODY)
                .color(crate::theme::muted()),
            );
            return;
        }
        let shown = amr::search_rows(&rows, &self.personal.q);
        if shown.is_empty() {
            ui.label(
                RichText::new("None matched.")
                    .size(crate::theme::FONT_BODY)
                    .color(crate::theme::muted()),
            );
            return;
        }
        let mut acted: Option<(String, RowAct)> = None;
        for group in PersonalGroup::ALL {
            let mine: Vec<&MemoryRow> = shown
                .iter()
                .copied()
                .filter(|r| r.group() == group)
                .collect();
            if mine.is_empty() {
                continue;
            }
            crate::cards::section_label(ui, group.label());
            ui.add_space(4.0);
            for row in mine {
                if let Some(act) = row_card(ui, row) {
                    acted = Some((row.id.clone(), act));
                }
                self.personal_edit_form(ui, &row.id);
            }
            ui.add_space(8.0);
        }
        if let Some((id, act)) = acted {
            self.personal_act(&id, act, &rows);
        }
    }

    fn personal_edit_form(&mut self, ui: &mut egui::Ui, id: &str) {
        let Some((_, text)) = self.personal.edit.as_mut().filter(|(e, _)| e == id) else {
            return;
        };
        ui.add(egui::TextEdit::singleline(text).desired_width(f32::INFINITY));
        let mut save = false;
        let mut cancel = false;
        ui.horizontal(|ui| {
            save = crate::cards::white_pill(ui, "Save");
            cancel = crate::cards::ghost_pill(ui, "Cancel");
        });
        ui.add_space(8.0);
        if cancel {
            self.personal.edit = None;
        }
        if !save {
            return;
        }
        let Some((id, text)) = self.personal.edit.take() else {
            return;
        };
        let store = self.amr_store(false);
        match amr::edit_node(&store, &id, &text, now_ms()) {
            Ok(_) => {
                self.status = "Saved in your words.".into();
                self.personal_changed();
            }
            Err(e) => self.status = format!("Couldn't save that: {e}"),
        }
    }

    fn personal_act(&mut self, id: &str, act: RowAct, rows: &[MemoryRow]) {
        let store = self.amr_store(false);
        let now = now_ms();
        let res = match act {
            RowAct::Edit => {
                let text = rows
                    .iter()
                    .find(|r| r.id == id)
                    .map(|r| r.text.clone())
                    .unwrap_or_default();
                self.personal.edit = Some((id.to_string(), text));
                return;
            }
            RowAct::Pause(paused) => amr::pause_node(&store, id, paused).map(|()| {
                if paused {
                    "Kept, but not used until you say so."
                } else {
                    "In use again."
                }
            }),
            RowAct::Forget => {
                amr::forget_node(&store, id, UserForget::from_click(), now).map(|()| "Forgotten.")
            }
            RowAct::Approve => amr::approve_node(&store, id, now).map(|_| "Saved."),
            RowAct::Reject => {
                amr::reject_node(&store, id, UserForget::from_click(), now).map(|()| "Dropped.")
            }
        };
        self.status = match res {
            Ok(line) => line.to_string(),
            Err(e) => format!("Couldn't do that: {e}"),
        };
        self.personal_changed();
    }

    fn personal_export_and_wipe(&mut self, ui: &mut egui::Ui) {
        crate::cards::section_label(ui, "Keep a copy, or start over");
        let mut export = false;
        let mut wipe = false;
        ui.horizontal(|ui| {
            export = crate::cards::ghost_pill(ui, "Export to Downloads");
            if !self.personal.wipe_ask {
                wipe = crate::cards::danger_pill(ui, "Wipe everything");
            }
        });
        if export {
            self.personal_export();
        }
        if wipe {
            self.personal.wipe_ask = true;
        }
        if !self.personal.wipe_ask {
            return;
        }
        ui.add_space(8.0);
        crate::cards::settings_note(
            ui,
            grokhub_core::brand::filled(
                "This deletes every line {product} learned about you, empties USER.md and MEMORY.md, and turns Know me off and clears what it found. It can't be undone. Export first if you want a copy.",
            ),
        );
        let mut confirm = false;
        let mut cancel = false;
        ui.horizontal(|ui| {
            confirm = crate::cards::danger_pill(ui, "Wipe for good");
            cancel = crate::cards::ghost_pill(ui, "Cancel");
        });
        if cancel {
            self.personal.wipe_ask = false;
        }
        if confirm {
            self.personal.wipe_ask = false;
            self.personal_wipe();
        }
    }

    fn personal_export(&mut self) {
        let Some(dir) = grokhub_core::diagnostics::downloads_dir() else {
            self.status = "Couldn't find your Downloads folder.".into();
            return;
        };
        let store = self.amr_store(false);
        let text = amr::personal_export(&store, now_ms());
        let path = dir.join(export_file_name(now_ms()));
        self.status = match std::fs::create_dir_all(&dir)
            .map_err(|e| e.to_string())
            .and_then(|()| config::atomic_write(&path, text.as_bytes()))
        {
            Ok(()) => format!("Saved to {}.", path.display()),
            Err(e) => format!("Couldn't save the copy: {e}"),
        };
    }

    fn personal_wipe(&mut self) {
        let store = self.amr_store(false);
        let report = amr::wipe_user_model(&store, UserForget::from_click());
        let notes = empty_notes_files();
        self.wipe_know_me();
        self.personal_changed();
        self.status = match (report, notes) {
            (Ok(r), Ok(())) if r.locked > 0 => format!(
                "Wiped {} lines. {} private notes were locked and stay until you open them.",
                r.removed, r.locked
            ),
            (Ok(r), Ok(())) => format!("Wiped {} lines. Personal is empty.", r.removed),
            (Err(e), _) => format!("Couldn't wipe: {e}"),
            (_, Err(e)) => {
                format!("Wiped the learned lines, but couldn't empty the notes files: {e}")
            }
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn export_names_say_what_they_are_and_carry_the_day() {
        let name = export_file_name(1_791_374_400_000);
        assert!(
            name.ends_with("what it knows about you 2026-10-07.md"),
            "{name}"
        );
        assert!(
            name.starts_with(grokhub_core::brand::product_name()),
            "{name}"
        );
    }

    #[test]
    fn wipe_empties_user_and_memory_notes_and_their_backups_but_not_soul() {
        let _g = config::hold_test_config();
        let root = config::test_config_root("personal-wipe");
        let _ = std::fs::remove_dir_all(&root);
        let _pin = config::TestConfigDir::set(root.clone());
        config::write_memory("USER.md", "- works nights\n").unwrap();
        config::write_memory("USER.md", "- works nights\n- has a dog\n").unwrap();
        config::write_memory("MEMORY.md", "- 2026-11-01 asked about the garage\n").unwrap();
        config::write_memory("SOUL.md", "Speaks plainly.\n").unwrap();
        assert!(config::memory_dir().join("USER.md.prev").exists());
        empty_notes_files().unwrap();
        assert_eq!(config::read_memory("USER.md"), "# USER.md\n");
        assert_eq!(config::read_memory("MEMORY.md"), "# MEMORY.md\n");
        assert!(!config::memory_dir().join("USER.md.prev").exists());
        assert_eq!(config::read_memory("SOUL.md"), "Speaks plainly.\n");
        assert!(grokhub_core::learn_notes::is_placeholder(
            &config::read_memory("USER.md")
        ));
    }
}
