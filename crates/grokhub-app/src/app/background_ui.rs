//! Settings → Background: one quiet line per governor job (Brain 4). Each shows
//! when it next runs, today's tokens against its daily budget, and whether it
//! is waiting because you're active, for quiet hours, or for a new day.
//! Read-only: budgets live in `app.json` under `background.dailyTokens`.

use super::*;
use grokhub_core::{JobHold, JobStatus};

/// One job's line, as Settings → Background shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct BackgroundRow {
    pub name: &'static str,
    /// "Next check in 12s", "Due now", or why it waits.
    pub next: String,
    /// "1200 of 100000 tokens today", or "Local work, no tokens".
    pub budget: String,
    pub waiting: Option<JobHold>,
}

/// When a job runs next, in plain words.
pub(super) fn next_run_text(next_due_ms: u64, now_ms: u64) -> String {
    if next_due_ms <= now_ms {
        return "Due now".into();
    }
    let secs = (next_due_ms - now_ms).div_ceil(1000);
    if secs < 60 {
        format!("Next check in {secs}s")
    } else {
        format!("Next check in {}m", secs.div_ceil(60))
    }
}

pub(super) fn background_row(s: &JobStatus, now_ms: u64) -> BackgroundRow {
    BackgroundRow {
        name: s.kind.label(),
        next: match s.waiting {
            Some(hold) => hold.plain().to_string(),
            None => next_run_text(s.next_due_ms, now_ms),
        },
        budget: match s.daily_budget {
            Some(budget) => format!("{} of {budget} tokens today", s.spent_today),
            None => "Local work, no tokens".into(),
        },
        waiting: s.waiting,
    }
}

/// The line above the list: are you holding heavy jobs right now.
pub(super) fn background_headline(active: bool) -> &'static str {
    if active {
        "You're active, so heavy jobs wait and light ones run on the cheaper model."
    } else {
        "You're idle, so every job runs on its own schedule."
    }
}

fn hold_tone(hold: JobHold) -> crate::cards::ChipTone {
    match hold {
        JobHold::Heavy | JobHold::Quiet => crate::cards::ChipTone::Mute,
        JobHold::BudgetSpent | JobHold::Halted => crate::cards::ChipTone::Offline,
    }
}

fn hold_chip(hold: JobHold) -> &'static str {
    match hold {
        JobHold::Heavy => "You're active",
        JobHold::Quiet => "Quiet hours",
        JobHold::BudgetSpent => "Budget used",
        JobHold::Halted => "Halted",
    }
}

impl Cabin {
    pub(super) fn ui_settings_background(&mut self, ui: &mut egui::Ui) {
        crate::cards::help_text(
            ui,
            "Everything the cabin does on its own. While you type or a chat is running, heavy jobs wait for you to go idle. Daily token budgets reset each local day; set them in `app.json` under `background.dailyTokens`.",
        );
        ui.add_space(8.0);
        crate::cards::settings_note(ui, background_headline(self.user_active_now()));
        crate::cards::settings_note(
            ui,
            &format!(
                "Quiet hours {}–{}",
                self.cfg.quiet_start, self.cfg.quiet_end
            ),
        );
        let now = now_ms();
        let rows: Vec<BackgroundRow> = self
            .governor
            .status(&self.cfg.background)
            .iter()
            .map(|s| background_row(s, now))
            .collect();
        for row in &rows {
            paint_background_row(ui, row);
            ui.add_space(6.0);
        }
        // Next-run countdowns move on their own.
        ui.ctx().request_repaint_after(Duration::from_secs(1));
    }
}

fn paint_background_row(ui: &mut egui::Ui, row: &BackgroundRow) {
    egui::Frame::NONE
        .fill(crate::theme::surface())
        .corner_radius(12.0)
        .stroke(egui::Stroke::new(1.0_f32, crate::theme::border()))
        .inner_margin(egui::Margin::same(10))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(row.name)
                        .size(crate::theme::FONT_BODY)
                        .strong()
                        .color(crate::theme::fg()),
                );
                if let Some(hold) = row.waiting {
                    crate::cards::status_chip(ui, hold_chip(hold), hold_tone(hold));
                }
            });
            ui.label(
                RichText::new(format!("{} · {}", row.next, row.budget))
                    .size(crate::theme::FONT_META)
                    .color(crate::theme::muted()),
            );
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    use grokhub_core::JobKind;

    #[test]
    fn next_run_reads_in_seconds_then_minutes() {
        assert_eq!(next_run_text(0, 1_000), "Due now");
        assert_eq!(next_run_text(1_000, 1_000), "Due now");
        assert_eq!(next_run_text(1_001, 1_000), "Next check in 1s");
        assert_eq!(next_run_text(16_000, 1_000), "Next check in 15s");
        assert_eq!(next_run_text(61_000, 1_000), "Next check in 1m");
        assert_eq!(next_run_text(62_000, 1_000), "Next check in 2m");
    }

    #[test]
    fn a_row_names_the_job_its_next_run_its_budget_and_why_it_waits() {
        let row = background_row(
            &JobStatus {
                kind: JobKind::Review,
                next_due_ms: 0,
                spent_today: 1_000,
                daily_budget: Some(1_000),
                waiting: Some(JobHold::BudgetSpent),
            },
            5_000,
        );
        assert_eq!(
            row,
            BackgroundRow {
                name: "Nightly review",
                next: "Stopped for today: budget used".into(),
                budget: "1000 of 1000 tokens today".into(),
                waiting: Some(JobHold::BudgetSpent),
            }
        );
        let row = background_row(
            &JobStatus {
                kind: JobKind::Indexers,
                next_due_ms: 0,
                spent_today: 0,
                daily_budget: None,
                waiting: Some(JobHold::Heavy),
            },
            5_000,
        );
        assert_eq!(
            (row.name, row.next.as_str(), row.budget.as_str()),
            (
                "Indexers",
                "Waiting while you're active",
                "Local work, no tokens"
            )
        );
        assert_eq!(hold_chip(JobHold::Quiet), "Quiet hours");
        assert_eq!(
            background_headline(true),
            "You're active, so heavy jobs wait and light ones run on the cheaper model."
        );
        assert_eq!(
            background_headline(false),
            "You're idle, so every job runs on its own schedule."
        );
        let row = background_row(
            &JobStatus {
                kind: JobKind::Digest,
                next_due_ms: 20_000,
                spent_today: 1_234,
                daily_budget: Some(150_000),
                waiting: None,
            },
            5_000,
        );
        assert_eq!(
            (row.next.as_str(), row.budget.as_str()),
            ("Next check in 15s", "1234 of 150000 tokens today")
        );
    }
}
