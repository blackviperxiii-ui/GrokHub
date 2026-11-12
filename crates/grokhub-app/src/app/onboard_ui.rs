//! Get to know you in the cabin: the stage after the setup wizard. A few
//! short, skippable steps draft USER.md, SOUL.md and MEMORY.md (and Personal
//! rows when AMR is on); nothing is written until Review's Save. The files
//! and rows go through `grokhub_agent::onboard`, the desktop app's writer
//! too. Settings → Personal → Get to know you opens it again.

use grokhub_agent::onboard::{self, Saved};
use grokhub_core::onboard_identity::{self as oi, Answers, Drafts, Step};

use super::*;

/// The stage's state for this launch (kept in the setup wizard's state).
#[derive(Default)]
pub(in crate::app) struct Onboard {
    pub(in crate::app) open: bool,
    pub(in crate::app) step: Step,
    pub(in crate::app) answers: Answers,
    /// Review's drafts, as the person edits them.
    pub(in crate::app) drafts: Option<Drafts>,
    pub(in crate::app) saved: Option<Saved>,
    pub(in crate::app) note: Option<String>,
}

pub(in crate::app) const WELCOME_TITLE: &str = "Help {product} get to know you";
pub(in crate::app) const WELCOME_BODY: &str = "About two minutes. You see and can edit everything before it's saved, and you can skip any part. It all stays on this computer.";
const REVIEW_NOTE: &str = "This is exactly what Save writes. Edit or delete anything first.";
const DONE_NOTE: &str = "Edit any of it later in Settings → Personal.";
const TONES: [(&str, &str); 3] = [
    ("friendly", "Friendly"),
    ("direct", "Straight to the point"),
    ("playful", "Playful"),
];

/// What a press asks for, run once the window is drawn.
enum OnboardAct {
    None,
    Go(Step),
    Skip,
    Save,
    OpenClaw,
    Close,
}

fn next_of(step: Step) -> Option<Step> {
    Some(match step {
        Step::Welcome => Step::About,
        Step::About => Step::Soul,
        Step::Soul => Step::Life,
        Step::Life => Step::Review,
        Step::Review | Step::Done => return None,
    })
}

fn back_of(step: Step) -> Option<Step> {
    Some(match step {
        Step::Soul => Step::About,
        Step::Life => Step::Soul,
        Step::Review => Step::Life,
        Step::Welcome | Step::About | Step::Done => return None,
    })
}

/// "Saved USER.md, SOUL.md and 2 Personal rows."
pub(in crate::app) fn saved_line(s: &Saved) -> String {
    if s.files.is_empty() && s.rows == 0 {
        return "Nothing changed.".into();
    }
    let rows = match s.rows {
        0 => String::new(),
        1 => " and 1 Personal row".into(),
        n => format!(" and {n} Personal rows"),
    };
    format!("Saved {}{rows}.", s.files.join(", "))
}

/// The Saved screen's line: what Save wrote, then each file it left alone
/// and why (the writer's own sentence, kept in `note` by Save).
pub(in crate::app) fn done_line(o: &Onboard) -> String {
    let line = o.saved.as_ref().map(saved_line).unwrap_or_default();
    match &o.note {
        Some(left) => format!("{line} {left}"),
        None => line,
    }
}

/// Two notes as one; either can be missing.
fn both(a: Option<String>, b: Option<String>) -> Option<String> {
    match (a, b) {
        (Some(a), Some(b)) => Some(format!("{a} {b}")),
        (a, b) => a.or(b),
    }
}

fn line_field(ui: &mut egui::Ui, label: &str, value: &mut String) {
    ui.label(
        RichText::new(label)
            .size(crate::theme::FONT_BODY)
            .color(crate::theme::fg()),
    );
    ui.add(egui::TextEdit::singleline(value).desired_width(f32::INFINITY));
    ui.add_space(8.0);
}

fn long_field(ui: &mut egui::Ui, label: &str, value: &mut String, rows: usize) {
    ui.label(
        RichText::new(label)
            .size(crate::theme::FONT_BODY)
            .color(crate::theme::fg()),
    );
    ui.add(
        egui::TextEdit::multiline(value)
            .desired_rows(rows)
            .desired_width(f32::INFINITY),
    );
    ui.add_space(8.0);
}

impl Onboard {
    /// Review opens: the drafts for the answers, and after any note already
    /// there, which files Review shows empty because their text can't be read.
    fn open_review(&mut self, dir: &std::path::Path) {
        self.drafts = Some(onboard::draft(dir, &self.answers, grokhub_core::now_ms()));
        self.note = both(self.note.take(), onboard::review_note(dir));
    }
}

impl Cabin {
    /// Open it when it waits: right after the setup wizard, once at launch
    /// for a profile whose USER.md and SOUL.md are still seeds, or from
    /// Settings (`reopen`), which starts again over whatever is written.
    pub(in crate::app) fn open_onboard(&mut self, reopen: bool) {
        let dir = crate::config::config_dir();
        if reopen && onboard::reopen(&dir).is_err() {
            return;
        }
        if !onboard::pending(&dir) {
            return;
        }
        let p = onboard::load(&dir);
        let tone = self.auto_talk_tone();
        let o = &mut self.setup.onboard;
        o.open = true;
        o.step = p.step;
        o.answers = p.answers;
        if o.answers.tone.is_empty() {
            o.answers.tone = tone.to_string();
        }
        o.saved = None;
        o.note = None;
        o.drafts = None;
        if o.step == Step::Review {
            o.open_review(&dir);
        }
    }

    pub(in crate::app) fn onboard_go(&mut self, to: Step) {
        let dir = crate::config::config_dir();
        let o = &mut self.setup.onboard;
        o.step = to;
        // A note belongs to the step it was said on.
        o.note = onboard::keep(&dir, to, &o.answers).err();
        if to == Step::Review {
            o.open_review(&dir);
        }
    }

    pub(in crate::app) fn onboard_skip(&mut self) {
        let _ = onboard::skip(&crate::config::config_dir(), grokhub_core::now_ms());
        self.setup.onboard = Onboard::default();
        self.status = "Get to know you skipped. Settings → Personal opens it again.".into();
    }

    /// Save the reviewed drafts; the name also becomes the rail's name.
    pub(in crate::app) fn onboard_save(&mut self) {
        let Some(drafts) = self.setup.onboard.drafts.clone() else {
            return;
        };
        let name = oi::clean(&self.setup.onboard.answers.name);
        let dir = crate::config::config_dir();
        match onboard::commit(&dir, &drafts, grokhub_core::now_ms()) {
            Ok(saved) => {
                if !name.is_empty() && self.cfg.display_name != name {
                    self.cfg.display_name = name;
                    self.persist_cfg();
                }
                let o = &mut self.setup.onboard;
                o.note = onboard::saved_note(&dir, &saved);
                o.saved = Some(saved);
                o.step = Step::Done;
            }
            Err(e) => self.setup.onboard.note = Some(e),
        }
    }

    /// Add the workspace's notes under `home` to Review's drafts, and say
    /// what the import left out or found already there.
    pub(in crate::app) fn onboard_openclaw(&mut self, home: &std::path::Path) {
        let o = &mut self.setup.onboard;
        let Some(base) = o.drafts.clone() else {
            return;
        };
        let held = onboard::review_note(&crate::config::config_dir());
        let said = match onboard::openclaw_import(home, &base) {
            Some(import) => {
                o.drafts = Some(import.drafts);
                import.note
            }
            None => Some("No OpenClaw or Hermes workspace on this computer.".into()),
        };
        o.note = both(held, said);
    }

    pub(in crate::app) fn paint_onboard(&mut self, ctx: &egui::Context) {
        if !self.setup.onboard.open {
            return;
        }
        let step = self.setup.onboard.step;
        let openclaw = step == Step::Review
            && grokhub_core::paths::user_home()
                .as_deref()
                .and_then(onboard::openclaw_workspace)
                .is_some();
        let amr = onboard::amr_on(&crate::config::config_dir());
        let mut know_me = self.know_me_on();
        let mut act = OnboardAct::None;
        let modal = egui::Modal::new(egui::Id::unique("onboard-identity"))
            .backdrop_color(egui::Color32::from_black_alpha(153))
            .frame(
                egui::Frame::NONE
                    .fill(crate::theme::elevated())
                    .stroke(egui::Stroke::new(1.0_f32, crate::theme::border_strong()))
                    .corner_radius(16.0)
                    .inner_margin(egui::Margin::same(24)),
            )
            .show(ctx, |ui| {
                ui.set_width(600.0_f32.min(ctx.content_rect().width() - 80.0));
                let welcome = grokhub_core::brand::filled(WELCOME_TITLE);
                let title = match step {
                    Step::Welcome => welcome,
                    Step::About => "About you",
                    Step::Soul => "How should I sound?",
                    Step::Life => "Your life and work",
                    Step::Review => "Check what I'll save",
                    Step::Done => "Saved",
                };
                ui.label(RichText::new(title).font(crate::theme::title_font(22.0)).color(crate::theme::fg()));
                ui.add_space(8.0);
                let max_h = (ctx.content_rect().height() - 220.0).max(160.0);
                egui::ScrollArea::vertical().max_height(max_h).show(ui, |ui| {
                    let o = &mut self.setup.onboard;
                    let a = &mut o.answers;
                    match step {
                        Step::Welcome => crate::cards::settings_note(ui, WELCOME_BODY),
                        Step::About => {
                            line_field(ui, "Your name", &mut a.name);
                            line_field(ui, "What should I call you?", &mut a.call_me);
                            long_field(ui, "Anything I should know about you?", &mut a.about, 2);
                        }
                        Step::Soul => {
                            ui.horizontal_wrapped(|ui| {
                                for (value, label) in TONES {
                                    if crate::cards::felt_segment(ui, label, a.tone == value).clicked() {
                                        a.tone = if a.tone == value { String::new() } else { value.into() };
                                    }
                                }
                            });
                            ui.add_space(8.0);
                            long_field(ui, "Anything I should always or never do?", &mut a.soul_notes, 2);
                        }
                        Step::Life => {
                            line_field(ui, "Goals you're working on", &mut a.goals);
                            line_field(ui, "Interests", &mut a.interests);
                            line_field(ui, "Routines", &mut a.routines);
                            line_field(ui, "Apps you use", &mut a.apps);
                            line_field(ui, "How you like things done", &mut a.prefs);
                            long_field(ui, "Anything else to remember", &mut a.remember, 2);
                            crate::cards::settings_toggle(
                                ui,
                                "Know me",
                                "Look at harmless signals on this PC (apps, games, language). Off unless you turn it on.",
                                &mut know_me,
                            );
                        }
                        Step::Review => {
                            crate::cards::settings_note(ui, REVIEW_NOTE);
                            if let Some(d) = o.drafts.as_mut() {
                                long_field(ui, "About you (USER.md)", &mut d.user, 4);
                                long_field(ui, "How I sound (SOUL.md)", &mut d.soul, 3);
                                long_field(ui, "Notes to keep (MEMORY.md)", &mut d.memory, 3);
                                if amr && !d.rows.is_empty() {
                                    crate::cards::settings_note(ui, "These also go to Settings → Personal:");
                                    let mut drop = None;
                                    for (i, r) in d.rows.iter().enumerate() {
                                        ui.horizontal(|ui| {
                                            ui.label(RichText::new(&r.text).color(crate::theme::fg()));
                                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                                if crate::cards::ghost_pill(ui, "Remove") {
                                                    drop = Some(i);
                                                }
                                            });
                                        });
                                    }
                                    if let Some(i) = drop {
                                        d.rows.remove(i);
                                    }
                                }
                            }
                            if openclaw && crate::cards::ghost_pill(ui, "Add from OpenClaw") {
                                act = OnboardAct::OpenClaw;
                            }
                        }
                        Step::Done => {
                            crate::cards::settings_note(ui, &format!("{} {DONE_NOTE}", done_line(o)));
                        }
                    }
                    // The Saved screen's line already ends with the note.
                    if let Some(note) = o.note.as_ref().filter(|_| step != Step::Done) {
                        crate::cards::settings_note(ui, note);
                    }
                });
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    if let Some(to) = back_of(step) {
                        if crate::cards::ghost_pill(ui, "Back") {
                            act = OnboardAct::Go(to);
                        }
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        match step {
                            Step::Welcome => {
                                if crate::cards::white_pill(ui, "Start") {
                                    act = OnboardAct::Go(Step::About);
                                }
                            }
                            Step::Review => {
                                if crate::cards::white_pill(ui, "Save") {
                                    act = OnboardAct::Save;
                                }
                            }
                            Step::Done => {
                                if crate::cards::white_pill(ui, "Done") {
                                    act = OnboardAct::Close;
                                }
                            }
                            _ => {
                                let label = if step == Step::Life { "Review" } else { "Next" };
                                if let Some(to) = next_of(step) {
                                    if crate::cards::white_pill(ui, label) {
                                        act = OnboardAct::Go(to);
                                    }
                                }
                            }
                        }
                        if step != Step::Done && crate::cards::ghost_pill(ui, "Skip for now") {
                            act = OnboardAct::Skip;
                        }
                    });
                });
            });
        if know_me != self.know_me_on() {
            self.set_know_me_on(know_me);
        }
        match act {
            OnboardAct::None if modal.should_close() => {
                if step == Step::Done {
                    self.setup.onboard = Onboard::default();
                } else {
                    self.onboard_skip();
                }
            }
            OnboardAct::None => {}
            OnboardAct::Go(to) => self.onboard_go(to),
            OnboardAct::Skip => self.onboard_skip(),
            OnboardAct::Save => self.onboard_save(),
            OnboardAct::OpenClaw => {
                self.onboard_openclaw(&grokhub_core::paths::user_home().unwrap_or_default());
            }
            OnboardAct::Close => self.setup.onboard = Onboard::default(),
        }
    }
}

#[cfg(test)]
#[path = "onboard_ui_tests.rs"]
mod tests;
