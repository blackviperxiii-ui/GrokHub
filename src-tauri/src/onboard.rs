//! Get to know you in the desktop app: the stage after Auto Setup's first
//! run that drafts USER.md, SOUL.md and MEMORY.md (and Personal rows when AMR
//! is on) for review. The files and rows are written by
//! `grokhub_agent::onboard`, the same writer the cabin uses.

use std::path::Path;
use std::sync::Mutex;

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
    /// Review: an OpenClaw, Hermes, Claude Code, Cursor or Codex workspace's
    /// files added to the drafts. `source` is the id from `importSources`
    /// (`openclaw`, `hermes`, `claude`, `cursor`, `codex`); empty picks the
    /// first OpenClaw/Hermes workspace.
    Openclaw {
        drafts: Drafts,
        #[serde(default)]
        source: Option<String>,
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
pub struct ImportSource {
    pub id: String,
    pub label: String,
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
    /// An OpenClaw, Hermes, Claude Code, Cursor or Codex workspace is here.
    pub openclaw: bool,
    /// Every importable source found on this computer.
    pub import_sources: Vec<ImportSource>,
    pub saved: Option<Saved>,
    /// What the last act could not do, or left out, and why.
    pub note: Option<String>,
}

/// The command runs off the main thread, so the page can have two acts out
/// at once (Save pressed twice, Next pressed quickly). Two writers of one
/// file would share its temp file and its `.prev` copy, so acts take turns.
static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

/// One act. `personality` is the voice picked now, the soul step's start.
pub fn act(
    config_dir: &Path,
    home: Option<&Path>,
    personality: &str,
    act: Act,
    now_ms: u64,
) -> Result<Reply, String> {
    let _turn = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
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
            note = onboard::review_note(config_dir);
        }
        Act::Openclaw {
            drafts: base,
            source,
        } => {
            let held = onboard::review_note(config_dir);
            let id = source.as_deref().unwrap_or("");
            match home.and_then(|h| onboard::import_by_id(h, id, &base)) {
                Some(import) => {
                    drafts = Some(import.drafts);
                    note = match (held, import.note) {
                        (Some(a), Some(b)) => Some(format!("{a} {b}")),
                        (a, b) => a.or(b),
                    };
                }
                None => {
                    note = Some(
                        "No OpenClaw, Hermes, Claude Code, Cursor or Codex workspace on this computer."
                            .into(),
                    );
                    drafts = Some(base);
                }
            }
        }
        Act::Save { drafts: d } => {
            // Save clears the answers, so the name is read first.
            let typed =
                onboard::pending(config_dir).then(|| onboard::load(config_dir).answers.name);
            let done = onboard::commit(config_dir, &d, now_ms)?;
            if let Some(typed) = typed {
                name_the_profile(config_dir, &typed);
            }
            note = onboard::saved_note(config_dir, &done);
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
    let sources: Vec<ImportSource> = home
        .map(onboard::import_sources)
        .unwrap_or_default()
        .into_iter()
        .map(|s| ImportSource {
            id: s.id,
            label: s.label,
        })
        .collect();
    Ok(Reply {
        pending: onboard::pending(config_dir) || saved.is_some(),
        stage: p.stage,
        step: p.step,
        answers,
        drafts,
        amr: onboard::amr_on(config_dir),
        openclaw: !sources.is_empty(),
        import_sources: sources,
        saved,
        note,
    })
}

/// The name typed in Get to know you becomes the profile name when none is
/// saved yet, as in the cabin. A name set in Settings → Account is never
/// replaced by older answers, and a failed write never fails the save: the
/// files are already in.
fn name_the_profile(config_dir: &Path, typed: &str) {
    let name = grokhub_core::onboard_identity::clean(typed);
    if name.is_empty() || !crate::profile::load(config_dir, None).name.is_empty() {
        return;
    }
    if let Err(e) = crate::profile::set_name(config_dir, &name) {
        eprintln!("get to know you: the profile name wasn't saved: {e}");
    }
}

#[cfg(test)]
#[path = "onboard_tests.rs"]
mod tests;
