//! Card 32 PR D: Settings → Behavior → Self-improvement history. Every
//! change the weekly pass made (on trial, kept, rolled back, reverted), with
//! a click Revert on a kept one, and the code-change ideas the user saved.
//! A code idea is never applied: it can only be copied as a GitHub issue the
//! user pastes by hand, with home paths, emails and keys taken out. Nothing
//! here makes a network request.

use eframe::egui;
use grokhub_agent::harness as hx;
use grokhub_core::self_review as sr;
use serde::{Deserialize, Serialize};

use super::self_review_ui::{load_state, save_state};
use super::Cabin;
use crate::{config, skills};

/// History rows Settings shows, newest first.
const HISTORY_ROWS: usize = 8;
/// Saved code ideas kept, newest last.
pub(super) const SAVED_IDEAS_CAP: usize = 50;
const HISTORY_EMPTY: &str = "No self-changes yet. The Sunday self-review proposes them as Pulse cards.";

/// A code-change idea the user saved from its Pulse card.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct SavedIdea {
    pub title: String,
    pub text: String,
    pub at: u64,
}

/// One history line in Settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct HistoryRow {
    pub title: String,
    pub hint: String,
    /// Set on a kept version that is still the newest change on its target.
    pub revert: Option<String>,
}

fn status_word(s: hx::VersionStatus) -> &'static str {
    match s {
        hx::VersionStatus::Canary => "Trying",
        hx::VersionStatus::Kept => "Kept",
        hx::VersionStatus::RolledBack => "Rolled back",
        hx::VersionStatus::Ended => "Ended",
        hx::VersionStatus::Reverted => "Reverted",
    }
}

/// Newest first. Revert goes only on the newest kept version of a target.
pub(super) fn history_rows(versions: &[hx::Version], now: u64) -> Vec<HistoryRow> {
    let mut rows = Vec::new();
    for (i, v) in versions.iter().enumerate().rev().take(HISTORY_ROWS) {
        let newest = !versions[i + 1..].iter().any(|o| o.kind == v.kind && o.id == v.id);
        let mut hint = format!("{} · {}", v.diff, grokhub_core::pulse::ago_label(v.at, now));
        if !v.note.is_empty() {
            hint = format!("{} · {hint}", v.note);
        }
        rows.push(HistoryRow {
            title: format!("{}: {}", status_word(v.status), v.label),
            hint,
            revert: (newest && v.status == hx::VersionStatus::Kept).then(|| v.version.clone()),
        });
    }
    rows
}

impl Cabin {
    /// A code-idea card's Save: keep it in the local list, change nothing else.
    pub(super) fn save_code_idea(&mut self, title: &str, text: &str) -> bool {
        let mut state = load_state();
        state.saved_ideas.retain(|i| i.title != title);
        state.saved_ideas.push(SavedIdea { title: title.to_string(), text: text.to_string(), at: grokhub_core::now_ms() });
        let over = state.saved_ideas.len().saturating_sub(SAVED_IDEAS_CAP);
        state.saved_ideas.drain(..over);
        save_state(&state);
        self.status = "Idea saved in Settings → Behavior. GrokHub never changes its own code.".into();
        true
    }

    /// The issue text for a code idea, redacted, for the clipboard.
    pub(super) fn issue_for(title: &str, text: &str) -> String {
        let home = std::env::var(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).ok();
        sr::issue_text(title, text, &crate::update::build_version_line(), home.as_deref())
    }

    /// A code-idea card's Copy as GitHub issue: the text goes to the
    /// clipboard on the next frame; the card stays.
    pub(super) fn copy_code_idea(&mut self, id: &str) {
        let Some(card) = self.updates.iter().find(|c| c.id == id) else {
            return;
        };
        let Some(pending) = load_state().pending.into_iter().find(|p| p.source == card.source_id) else {
            return;
        };
        self.harness.self_review.copy_out = Some(Self::issue_for(&card.title, &pending.md));
        self.status = ISSUE_COPIED.into();
    }

    /// Settings: the self-change history and the saved code ideas.
    pub(super) fn ui_self_change_history(&mut self, ui: &mut egui::Ui) {
        let dir = config::config_dir();
        let rows = history_rows(&hx::read_versions(&dir), grokhub_core::now_ms());
        crate::cards::settings_note(ui, "Self-improvement history");
        if rows.is_empty() {
            crate::cards::settings_note(ui, HISTORY_EMPTY);
        }
        for row in rows {
            let Some(version) = row.revert else {
                crate::cards::settings_note(ui, &format!("{}\n{}", row.title, row.hint));
                continue;
            };
            if crate::cards::settings_action(ui, &row.title, &row.hint, "Revert") {
                self.revert_kept_version(&version);
            }
        }
        let ideas = load_state().saved_ideas;
        for idea in ideas.iter().rev() {
            if crate::cards::settings_action(ui, &format!("Idea: {}", idea.title), &idea.text, "Copy as GitHub issue") {
                ui.ctx().copy_text(Self::issue_for(&idea.title, &idea.text));
                self.status = ISSUE_COPIED.into();
            }
        }
    }

    /// Revert on a kept history row: the user's click undoes it through the ledger.
    pub(super) fn revert_kept_version(&mut self, version: &str) {
        let dir = config::config_dir();
        match hx::revert_version(&dir, &skills::skills_dir(), version, hx::UndoAsk::from_click(), grokhub_core::now_ms()) {
            Ok(v) => {
                if v.kind == hx::ChangeKind::Skill.as_str() {
                    self.skill_list = skills::list_skills();
                    self.harness.skill_rows = None;
                }
                self.harness.change_rows = None;
                self.status = v.note;
            }
            Err(e) => self.status = e,
        }
    }
}

const ISSUE_COPIED: &str = "Issue copied. Paste it on GitHub yourself; GrokHub sent nothing.";
