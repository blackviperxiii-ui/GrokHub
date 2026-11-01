//! Journal page: local-first decision backlog / experiment journal.
//! Labeled "Journal" (not "Decisions") so it never collides with the
//! permission inbox (`decisions_waiting`).

use super::*;
use grokhub_core::{
    abandon_entry, add_evidence, add_option, create_entry, decide_entry, defer_entry,
    dismiss_nudge, due_nudges, edit_entry, promote_to_board, promote_to_mission, remove_evidence,
    remove_option, search_entries, set_status, EvidenceKind, JournalEntry, JournalStatus,
};

#[derive(Default)]
pub(super) struct JournalView {
    pub search: String,
    pub draft_question: String,
    pub open_id: Option<String>,
    pub edit_question: String,
    pub edit_context: String,
    pub edit_experiment: String,
    pub edit_unknowns: String,
    pub draft_option: String,
    pub draft_evidence: String,
    pub draft_learned: String,
}

impl Cabin {
    pub(super) fn flush_journal(&mut self) {
        let entries = self.journal.clone();
        let io = self.persist_io.clone();
        self.persist_idle_key = self.persist_idle_now();
        self.last_persist = Instant::now();
        std::thread::spawn(move || {
            if let Ok(_g) = io.lock() {
                let _ = config::save_journal(&entries);
            }
        });
    }

    fn journal_open_editor(&mut self, id: &str) {
        if let Some(e) = self.journal.iter().find(|e| e.id == id) {
            self.journal_view.edit_question = e.question.clone();
            self.journal_view.edit_context = e.context.clone();
            self.journal_view.edit_experiment = e.next_experiment.clone();
            self.journal_view.edit_unknowns = e.unknowns.join("\n");
            self.journal_view.draft_option.clear();
            self.journal_view.draft_evidence.clear();
            self.journal_view.draft_learned.clear();
            self.journal_view.open_id = Some(id.to_string());
        }
    }

    /// Start a decision's mission: back to its chat when that still exists,
    /// otherwise a new chat named for the question. The chat opens with a
    /// mission note (so it is never reused as an empty chat) and the first
    /// message drafted in the composer. Nothing is sent until the user presses Send.
    pub(super) fn promote_journal_to_mission(&mut self, id: &str) {
        let linked = self
            .journal
            .iter()
            .find(|e| e.id == id)
            .and_then(|e| e.mission_thread.clone());
        if let Some(idx) = linked.and_then(|tid| self.threads.iter().position(|t| t.id == tid)) {
            if idx != self.thread_idx {
                self.apply_switch_thread(idx);
            }
            self.nav = Nav::Chat;
            self.status = "Back on this decision's mission".into();
            return;
        }
        if !self
            .journal
            .iter()
            .any(|e| e.id == id && e.status.is_active())
        {
            return;
        }
        self.new_thread(false);
        let trace = self.trace_id();
        let Some(start) = promote_to_mission(&mut self.journal, id, &trace) else {
            return;
        };
        if let Some(t) = self.threads.get_mut(self.thread_idx) {
            t.title = start.title.clone();
            t.title_locked = true;
            t.goal.label = start.title.clone();
        }
        let note = format!(
            "Mission from the Journal: {}\nThe first message is drafted below. Press Send to run the next experiment.",
            start.title
        );
        self.live_mut()
            .push(("assistant".into(), grokhub_core::mark_slash_result(&note)));
        self.stamp_current_access();
        self.composer = start.prompt;
        self.nav = Nav::Chat;
        self.flush_journal();
        self.persist();
        self.status = "Mission ready: press Send to run the next experiment".into();
    }

    pub(super) fn ui_journal(&mut self, ui: &mut egui::Ui) {
        egui::CentralPanel::default()
            .frame(
                egui::Frame::NONE
                    .fill(crate::theme::bg())
                    .inner_margin(egui::Margin::same(24)),
            )
            .show(ui, |ui| {
                crate::cards::page_header(ui, "Journal", "");
                ui.label(
                    RichText::new(
                        "Open questions and experiments — not tasks. Start a mission to run the next experiment, or file it on Workboards.",
                    )
                    .size(12.0)
                    .color(crate::theme::subtle()),
                );
                ui.add_space(10.0);
                super::pages::page_body(ui, "journal", |ui| {

                    let now = now_ms();
                    let due: Vec<(String, String)> = due_nudges(&self.journal, now)
                        .into_iter()
                        .map(|e| (e.id.clone(), e.question.clone()))
                        .collect();
                    let mut dismiss: Option<String> = None;
                    for (id, question) in &due {
                        egui::Frame::NONE
                            .fill(crate::theme::elevated())
                            .corner_radius(8.0)
                            .stroke(egui::Stroke::new(1.0, crate::theme::border()))
                            .inner_margin(egui::Margin::symmetric(10, 8))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(
                                        RichText::new(format!("Due to revisit: {question}"))
                                            .size(13.0)
                                            .color(crate::theme::fg()),
                                    );
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            if crate::cards::ghost_pill(ui, "Dismiss") {
                                                dismiss = Some(id.clone());
                                            }
                                            if crate::cards::ghost_pill(ui, "Open") {
                                                self.journal_open_editor(id);
                                            }
                                        },
                                    );
                                });
                            });
                        ui.add_space(6.0);
                    }
                    if let Some(id) = dismiss {
                        if dismiss_nudge(&mut self.journal, &id) {
                            self.flush_journal();
                        }
                    }

                    ui.horizontal(|ui| {
                        crate::cards::search_bar(
                            ui,
                            &mut self.journal_view.search,
                            "Search journal",
                            280.0,
                        );
                    });
                    ui.add_space(8.0);

                    ui.horizontal(|ui| {
                        let r = ui.add(
                            egui::TextEdit::singleline(&mut self.journal_view.draft_question)
                                .hint_text("What should we decide?")
                                .desired_width((ui.available_width() - 100.0).max(120.0)),
                        );
                        let create = crate::cards::white_pill(ui, "Add")
                            || (r.lost_focus()
                                && ui.input(|i| i.key_pressed(egui::Key::Enter))
                                && !self.journal_view.draft_question.trim().is_empty());
                        if create {
                            if let Some(id) =
                                create_entry(&mut self.journal, &self.journal_view.draft_question)
                            {
                                self.journal_view.draft_question.clear();
                                self.journal_open_editor(&id);
                                self.flush_journal();
                                self.status = "Journal entry added".into();
                            }
                        }
                    });
                    ui.add_space(12.0);

                    if self.journal.is_empty() {
                        let _ = crate::cards::empty_prompt_tile(
                            ui,
                            crate::icons::TileIcon::List,
                            "No open questions yet",
                            "Capture a decision-shaped question so interesting ideas do not become silent commitments.",
                        );
                        return;
                    }

                    let q = self.journal_view.search.clone();
                    let ids: Vec<String> = search_entries(&self.journal, &q)
                        .into_iter()
                        .map(|e| e.id.clone())
                        .collect();
                    if ids.is_empty() {
                        ui.label(
                            RichText::new("No matches.")
                                .size(13.0)
                                .color(crate::theme::muted()),
                        );
                        return;
                    }

                    let open = self.journal_view.open_id.clone();
                    for id in ids {
                        let Some(entry) = self.journal.iter().find(|e| e.id == id).cloned() else {
                            continue;
                        };
                        let expanded = open.as_deref() == Some(id.as_str());
                        self.paint_journal_row(ui, &entry, expanded);
                        ui.add_space(6.0);
                    }
                });
            });
    }

    fn paint_journal_row(&mut self, ui: &mut egui::Ui, entry: &JournalEntry, expanded: bool) {
        let id = entry.id.clone();
        egui::Frame::NONE
            .fill(crate::theme::elevated())
            .corner_radius(10.0)
            .stroke(egui::Stroke::new(1.0, crate::theme::border()))
            .inner_margin(egui::Margin::symmetric(12, 10))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    let title = RichText::new(&entry.question)
                        .size(14.0)
                        .strong()
                        .color(crate::theme::fg());
                    if ui
                        .add(egui::Label::new(title).sense(egui::Sense::click()))
                        .clicked()
                    {
                        if expanded {
                            self.journal_view.open_id = None;
                        } else {
                            self.journal_open_editor(&id);
                        }
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            RichText::new(entry.status.label())
                                .size(12.0)
                                .color(crate::theme::muted()),
                        );
                    });
                });
                if !entry.next_experiment.trim().is_empty() {
                    ui.label(
                        RichText::new(format!("Next: {}", entry.next_experiment.trim()))
                            .size(12.0)
                            .color(crate::theme::subtle()),
                    );
                }
                if !expanded {
                    return;
                }
                ui.add_space(8.0);
                ui.label(
                    RichText::new("Question")
                        .size(11.0)
                        .color(crate::theme::subtle()),
                );
                ui.add(
                    egui::TextEdit::singleline(&mut self.journal_view.edit_question)
                        .desired_width(ui.available_width()),
                );
                ui.label(
                    RichText::new("Context")
                        .size(11.0)
                        .color(crate::theme::subtle()),
                );
                ui.add(
                    egui::TextEdit::multiline(&mut self.journal_view.edit_context)
                        .desired_width(ui.available_width())
                        .desired_rows(2),
                );
                ui.label(
                    RichText::new("Next experiment")
                        .size(11.0)
                        .color(crate::theme::subtle()),
                );
                ui.add(
                    egui::TextEdit::singleline(&mut self.journal_view.edit_experiment)
                        .desired_width(ui.available_width()),
                );
                ui.label(
                    RichText::new("Unknowns (one per line)")
                        .size(11.0)
                        .color(crate::theme::subtle()),
                );
                ui.add(
                    egui::TextEdit::multiline(&mut self.journal_view.edit_unknowns)
                        .desired_width(ui.available_width())
                        .desired_rows(2),
                );
                ui.add_space(4.0);
                if crate::cards::ghost_pill(ui, "Save") {
                    let unknowns: Vec<String> = self
                        .journal_view
                        .edit_unknowns
                        .lines()
                        .map(|s| s.to_string())
                        .collect();
                    if edit_entry(
                        &mut self.journal,
                        &id,
                        Some(&self.journal_view.edit_question.clone()),
                        Some(&self.journal_view.edit_context.clone()),
                        Some(&self.journal_view.edit_experiment.clone()),
                        Some(unknowns),
                        None,
                    ) {
                        self.flush_journal();
                        self.status = "Journal saved".into();
                    }
                }

                ui.add_space(8.0);
                crate::cards::section_label(ui, "Options");
                let mut drop_opt: Option<String> = None;
                for opt in &entry.options {
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(&opt.label)
                                .size(13.0)
                                .color(crate::theme::fg()),
                        );
                        if entry.options.len() > 1 && crate::cards::ghost_pill(ui, "Remove") {
                            drop_opt = Some(opt.id.clone());
                        }
                    });
                }
                if let Some(oid) = drop_opt {
                    if remove_option(&mut self.journal, &id, &oid) {
                        self.flush_journal();
                    }
                }
                ui.horizontal(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.journal_view.draft_option)
                            .hint_text("Another option")
                            .desired_width(220.0),
                    );
                    if crate::cards::ghost_pill(ui, "Add option") {
                        let label = self.journal_view.draft_option.clone();
                        if add_option(&mut self.journal, &id, &label).is_some() {
                            self.journal_view.draft_option.clear();
                            self.flush_journal();
                        }
                    }
                });

                ui.add_space(8.0);
                crate::cards::section_label(ui, "Evidence");
                let mut drop_ev: Option<String> = None;
                for ev in &entry.evidence {
                    let line = if ev.label.is_empty() {
                        format!("{} · {}", ev.kind.as_str(), ev.reference)
                    } else {
                        format!("{} · {} — {}", ev.kind.as_str(), ev.label, ev.reference)
                    };
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(line).size(12.0).color(crate::theme::fg()));
                        if crate::cards::ghost_pill(ui, "Remove") {
                            drop_ev = Some(ev.id.clone());
                        }
                    });
                }
                if let Some(eid) = drop_ev {
                    if remove_evidence(&mut self.journal, &id, &eid) {
                        self.flush_journal();
                    }
                }
                ui.horizontal(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.journal_view.draft_evidence)
                            .hint_text("URL or note")
                            .desired_width(220.0),
                    );
                    if crate::cards::ghost_pill(ui, "Add evidence") {
                        let raw = self.journal_view.draft_evidence.clone();
                        let kind = if raw.trim().starts_with("http://")
                            || raw.trim().starts_with("https://")
                        {
                            EvidenceKind::Url
                        } else {
                            EvidenceKind::Note
                        };
                        if add_evidence(&mut self.journal, &id, kind, &raw, "").is_some() {
                            self.journal_view.draft_evidence.clear();
                            self.flush_journal();
                        }
                    }
                });

                ui.add_space(8.0);
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    if entry.status.is_active() {
                        if crate::cards::ghost_pill(ui, "Investigating") {
                            set_status(&mut self.journal, &id, JournalStatus::Investigating);
                            self.flush_journal();
                        }
                        if crate::cards::ghost_pill(ui, "Ready to decide") {
                            set_status(&mut self.journal, &id, JournalStatus::ReadyToDecide);
                            self.flush_journal();
                        }
                        if crate::cards::ghost_pill(ui, "Defer") {
                            defer_entry(&mut self.journal, &id);
                            self.flush_journal();
                        }
                        if crate::cards::ghost_pill(ui, "Abandon") {
                            abandon_entry(&mut self.journal, &id);
                            self.flush_journal();
                        }
                    }
                    if entry.status.is_active() && crate::cards::ghost_pill(ui, "To mission") {
                        self.promote_journal_to_mission(&id);
                    }
                    if crate::cards::ghost_pill(ui, "To workboard")
                        && promote_to_board(&mut self.journal, &mut self.board, &id).is_some()
                    {
                        self.flush_journal();
                        self.flush_board();
                        self.status = "Filed on Workboards".into();
                    }
                });

                if entry.status != JournalStatus::Decided
                    && entry.status != JournalStatus::Abandoned
                {
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut self.journal_view.draft_learned)
                                .hint_text("What did you learn?")
                                .desired_width(220.0),
                        );
                        if crate::cards::white_pill(ui, "Decide") {
                            let learned = self.journal_view.draft_learned.clone();
                            let chosen = entry
                                .leaning
                                .as_ref()
                                .map(|l| l.option_id.as_str())
                                .filter(|s| !s.is_empty())
                                .or_else(|| entry.options.first().map(|o| o.id.as_str()));
                            if decide_entry(&mut self.journal, &id, chosen, &learned) {
                                self.journal_view.draft_learned.clear();
                                self.flush_journal();
                                self.status = "Decided".into();
                            }
                        }
                    });
                }

                if let Some(outcome) = &entry.outcome {
                    if !outcome.learned.is_empty() {
                        ui.label(
                            RichText::new(format!("Learned: {}", outcome.learned))
                                .size(12.0)
                                .color(crate::theme::subtle()),
                        );
                    }
                }
                if let Some(board_id) = &entry.board_id {
                    ui.label(
                        RichText::new(format!("Workboard: {board_id}"))
                            .size(11.0)
                            .color(crate::theme::muted()),
                    );
                }
                if !entry.history.is_empty() {
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new(format!(
                            "History · {} events (kept on remove)",
                            entry.history.len()
                        ))
                        .size(11.0)
                        .color(crate::theme::muted()),
                    );
                }
            });
    }
}
