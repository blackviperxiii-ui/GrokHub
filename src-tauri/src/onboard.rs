//! Get to know you in the desktop app: the stage after Auto Setup's first
//! run that drafts USER.md, SOUL.md and MEMORY.md (and Personal rows when AMR
//! is on) for review. The files and rows are written by
//! `grokhub_agent::onboard`, the same writer the cabin uses.

use std::path::Path;

use grokhub_agent::onboard::{self, Saved};
use grokhub_core::onboard_identity::{tone_for_personality, Answers, Drafts, Stage, Step};
use serde::{Deserialize, Serialize};

/// What the page asks for.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "act", rename_all = "camelCase")]
pub enum Act {
    View,
    /// The person moved to `step`: keep it and what they typed.
    Keep {
        step: Step,
        answers: Answers,
    },
    /// Review: the drafts for these answers.
    Draft {
        answers: Answers,
    },
    /// Review: an OpenClaw or Hermes workspace's files added to the drafts.
    Openclaw {
        drafts: Drafts,
    },
    Save {
        drafts: Drafts,
    },
    /// Skip for now.
    Skip,
    /// Settings → Personal → Get to know you.
    Reopen,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Reply {
    /// The stage should show.
    pub pending: bool,
    pub stage: Stage,
    pub step: Step,
    pub answers: Answers,
    pub drafts: Option<Drafts>,
    /// AMR is on, so Save adds Personal rows too.
    pub amr: bool,
    /// An OpenClaw or Hermes workspace is on this computer.
    pub openclaw: bool,
    pub saved: Option<Saved>,
    /// What the last act could not do, or left out, and why.
    pub note: Option<String>,
}

/// One act. `personality` is the voice picked now, the soul step's start.
pub fn act(
    config_dir: &Path,
    home: Option<&Path>,
    personality: &str,
    act: Act,
    now_ms: u64,
) -> Result<Reply, String> {
    let mut drafts = None;
    let mut saved = None;
    let mut note = None;
    match act {
        Act::View => {}
        Act::Keep { step, answers } => {
            onboard::keep(config_dir, step, &answers)?;
        }
        Act::Draft { answers } => {
            // The step is kept so a quit resumes at Review. /import asks for
            // the drafts over a stage that is not waiting: nothing to resume,
            // so the progress file is left as it is.
            if onboard::pending(config_dir) {
                onboard::keep(config_dir, Step::Review, &answers)?;
            }
            drafts = Some(onboard::draft(config_dir, &answers, now_ms));
            note = too_big(&onboard::too_big(config_dir), "Save leaves it alone.");
        }
        Act::Openclaw { drafts: base } => {
            let held = too_big(&onboard::too_big(config_dir), "Save leaves it alone.");
            match home.and_then(|h| onboard::openclaw_import(h, &base)) {
                Some(import) => {
                    drafts = Some(import.drafts);
                    note = match (held, import.note) {
                        (Some(a), Some(b)) => Some(format!("{a} {b}")),
                        (a, b) => a.or(b),
                    };
                }
                None => {
                    note = Some("No OpenClaw or Hermes workspace on this computer.".into());
                    drafts = Some(base);
                }
            }
        }
        Act::Save { drafts: d } => {
            let done = onboard::commit(config_dir, &d, now_ms)?;
            note = too_big(&done.skipped, "it was left alone.");
            saved = Some(done);
        }
        Act::Skip => {
            onboard::skip(config_dir, now_ms)?;
        }
        Act::Reopen => {
            onboard::reopen(config_dir)?;
        }
    }
    let p = onboard::load(config_dir);
    let mut answers = p.answers;
    // Up to the soul step an empty tone means "not asked yet"; past it, the
    // person cleared it, and it stays cleared.
    let before_soul = matches!(p.step, Step::Welcome | Step::About | Step::Soul);
    if answers.tone.is_empty() && p.stage == Stage::Pending && before_soul {
        answers.tone = tone_for_personality(personality).to_string();
    }
    Ok(Reply {
        pending: onboard::pending(config_dir) || saved.is_some(),
        stage: p.stage,
        step: p.step,
        answers,
        drafts,
        amr: onboard::amr_on(config_dir),
        openclaw: home.and_then(onboard::openclaw_workspace).is_some(),
        saved,
        note,
    })
}

/// One sentence per file that is over the size cap, or `None` without one.
fn too_big(names: &[String], then: &str) -> Option<String> {
    let lines: Vec<String> = names
        .iter()
        .map(|name| format!("{name} is too big to change here; {then}"))
        .collect();
    (!lines.is_empty()).then(|| lines.join(" "))
}

#[cfg(test)]
#[path = "onboard_tests.rs"]
mod tests;
