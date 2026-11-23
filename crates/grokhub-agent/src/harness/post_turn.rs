//! Post-turn warm-prefix lesson capture & mid-run drift advisor (Card 957).
//!
//! 1. **Post-turn review**: after a complex multi-tool turn (or a turn that
//!    recovered from errors) while the prompt prefix cache is still warm,
//!    [`stage_post_turn_lesson`] stages a concise reversible lesson under
//!    `{config_dir}/tunable/<id>.md` through [`ChangeLedger`] (`Origin::SelfManage`)
//!    so the user can immediately inspect or Undo it without waiting for the
//!    nightly/weekly batch.
//! 2. **Mid-run drift advisor**: [`DriftAdvisor`] watches long autonomous
//!    loops (`run.rs`) for repeated identical tool calls or consecutive error
//!    streaks and emits a single bounded steering nudge with cooldown.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};

use crate::harness::changes::{
    content_hash, entry_id, record_change, ChangeKind, ChangeLedger, ChangeTarget, SnippetTarget,
};
use crate::harness::span::Origin;

/// Minimum tool steps in a turn to trigger immediate post-turn review.
pub const POST_TURN_MIN_STEPS: usize = 6;
/// Maximum lesson size written to `tunable/<id>.md`.
pub const MAX_LESSON_BYTES: usize = 4 * 1024;

/// Words naming hard-class actions that a post-turn lesson may not introduce.
const HARD_WORDS: &[&str] = &[
    "delete",
    "remove",
    "erase",
    "wipe",
    "password",
    "credential",
    "token",
    "transfer",
    "format",
    "uninstall",
    "shutdown",
    "reboot",
    "purchase",
];

/// Signals from a completed turn used to decide whether to capture a lesson
/// while the prompt prefix cache is warm.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnReviewInput {
    pub conversation_id: String,
    pub lesson_name: String,
    pub tool_steps: usize,
    pub recovered_errors: usize,
    pub warm_prefix_tokens: u64,
    pub lesson_markdown: String,
    pub reason: String,
}

/// Result of staging a reversible post-turn lesson through [`ChangeLedger`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PostTurnOutcome {
    pub id: String,
    pub path: PathBuf,
    pub ledger_seq: u64,
    pub warm_prefix_tokens: u64,
}

/// True when a completed turn had enough complexity or error recovery to merit
/// an immediate post-turn lesson capture while the prompt prefix is still warm.
pub fn should_trigger_review(input: &TurnReviewInput) -> bool {
    let complex = input.tool_steps >= POST_TURN_MIN_STEPS || input.recovered_errors > 0;
    complex && !input.lesson_markdown.trim().is_empty()
}

/// Dry-run safety check mirroring `self_improve`'s replay gate: rejects
/// oversized lessons or lessons introducing hard-class action words.
pub fn lesson_passes_guard(markdown: &str) -> Result<(), String> {
    let trimmed = markdown.trim();
    if trimmed.is_empty() {
        return Err("lesson is empty".into());
    }
    if trimmed.len() > MAX_LESSON_BYTES {
        return Err(format!("lesson exceeds {MAX_LESSON_BYTES} bytes"));
    }
    let lower = trimmed.to_ascii_lowercase();
    for word in HARD_WORDS {
        if has_word(&lower, word) {
            return Err(format!(
                "post-turn lesson rejected by safety guard: mentions hard-class word `{word}`"
            ));
        }
    }
    Ok(())
}

fn has_word(haystack: &str, needle: &str) -> bool {
    haystack
        .split(|c: char| !c.is_ascii_alphanumeric() && c != '_')
        .any(|tok| tok == needle)
}

/// Stage a reversible post-turn lesson into `{config_dir}/tunable/<id>.md` via
/// [`ChangeLedger`] (`Origin::SelfManage`).
///
/// Returns `Ok(None)` when the turn does not meet the review threshold, when
/// the exact bytes were previously undone by the user (`undone_by_user`), or
/// when the content is unchanged.
pub fn stage_post_turn_lesson(
    config_dir: &Path,
    input: &TurnReviewInput,
) -> Result<Option<PostTurnOutcome>, String> {
    if !should_trigger_review(input) {
        return Ok(None);
    }
    lesson_passes_guard(&input.lesson_markdown)?;

    let id = entry_id(&input.lesson_name)?;
    let bytes = input.lesson_markdown.trim().as_bytes().to_vec();
    let after_hash = content_hash(&bytes);

    let ledger = ChangeLedger::load_kind(config_dir, ChangeKind::Snippet);
    if ledger.undone_by_user(&id, &after_hash) {
        return Ok(None);
    }

    let target = SnippetTarget {
        config_dir,
        name: &id,
    };
    let path = target.file()?;
    let reason = if input.reason.trim().is_empty() {
        format!(
            "post-turn review ({} steps, {} recovered, {} warm prefix tokens)",
            input.tool_steps, input.recovered_errors, input.warm_prefix_tokens
        )
    } else {
        input.reason.trim().to_string()
    };

    let recorded = record_change(config_dir, &target, Origin::SelfManage, &reason, || {
        target.put(Some(&bytes))
    })?;

    Ok(recorded.map(|change| PostTurnOutcome {
        id,
        path,
        ledger_seq: change.seq,
        warm_prefix_tokens: input.warm_prefix_tokens,
    }))
}

/// Minimum turn number before the mid-run drift advisor activates.
pub const DRIFT_MIN_TURN: u32 = 4;
/// Sliding window of recent tool steps inspected for repetition.
pub const DRIFT_WINDOW: usize = 6;
/// Identical `(tool, args_sig)` calls inside [`DRIFT_WINDOW`] that signal drift.
pub const DRIFT_REPEAT_LIMIT: usize = 3;
/// Consecutive tool errors that signal a stalled loop.
pub const DRIFT_ERROR_STREAK: usize = 3;
/// Minimum turns between emitted drift nudges.
pub const DRIFT_COOLDOWN_TURNS: u32 = 3;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DriftKind {
    RepeatingTool { tool: String, count: usize },
    ErrorStreak { count: usize },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DriftAdvice {
    pub turn: u32,
    pub kind: DriftKind,
}

impl DriftAdvice {
    /// Concise steering reminder injected into a drifting autonomous loop.
    pub fn steering_message(&self) -> String {
        match &self.kind {
            DriftKind::RepeatingTool { tool, count } => format!(
                "[Drift check at turn {}] You called `{}` with the same arguments {} times recently. Pause, verify your assumptions, and try a different approach or conclude if blocked.",
                self.turn, tool, count
            ),
            DriftKind::ErrorStreak { count } => format!(
                "[Drift check at turn {}] The last {} tool calls failed consecutively. Step back, re-read the error output carefully, and adjust your plan before retrying.",
                self.turn, count
            ),
        }
    }
}

/// Stateful mid-run drift detector for long autonomous loops.
#[derive(Debug, Default)]
pub struct DriftAdvisor {
    recent: VecDeque<(String, String)>,
    error_streak: usize,
    last_nudge_turn: Option<u32>,
}

impl DriftAdvisor {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record one tool step and return a [`DriftAdvice`] nudge if the loop
    /// appears stuck in repetition or consecutive failures.
    pub fn record_step(
        &mut self,
        turn: u32,
        tool: &str,
        args_sig: &str,
        is_error: bool,
    ) -> Option<DriftAdvice> {
        if is_error {
            self.error_streak += 1;
        } else {
            self.error_streak = 0;
        }

        let key = (tool.to_string(), args_sig.to_string());
        if self.recent.len() == DRIFT_WINDOW {
            self.recent.pop_front();
        }
        self.recent.push_back(key.clone());

        if turn < DRIFT_MIN_TURN {
            return None;
        }
        if let Some(prev) = self.last_nudge_turn
            && turn.saturating_sub(prev) < DRIFT_COOLDOWN_TURNS
        {
            return None;
        }

        if self.error_streak >= DRIFT_ERROR_STREAK {
            self.last_nudge_turn = Some(turn);
            return Some(DriftAdvice {
                turn,
                kind: DriftKind::ErrorStreak {
                    count: self.error_streak,
                },
            });
        }

        let repeats = self.recent.iter().filter(|entry| **entry == key).count();
        if repeats >= DRIFT_REPEAT_LIMIT {
            self.last_nudge_turn = Some(turn);
            return Some(DriftAdvice {
                turn,
                kind: DriftKind::RepeatingTool {
                    tool: tool.to_string(),
                    count: repeats,
                },
            });
        }

        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::changes::{undo_change, UndoAsk};

    fn tmp_dir(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("loadout-post-turn-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn stages_reversible_lesson_and_respects_user_undo() {
        let cfg = tmp_dir("undo");
        let input = TurnReviewInput {
            conversation_id: "conv-1".into(),
            lesson_name: "cargo-edition-note".into(),
            tool_steps: 7,
            recovered_errors: 1,
            warm_prefix_tokens: 12_400,
            lesson_markdown: "Always pass --edition 2024 to rustfmt on leaf files.".into(),
            reason: String::new(),
        };

        let out = stage_post_turn_lesson(&cfg, &input)
            .unwrap()
            .expect("should stage lesson");
        assert!(out.path.is_file());
        assert_eq!(out.warm_prefix_tokens, 12_400);

        // User undoes the staged lesson.
        let target = SnippetTarget {
            config_dir: &cfg,
            name: &out.id,
        };
        undo_change(&cfg, &target, UndoAsk::from_click()).unwrap();
        assert!(!out.path.exists());

        // Re-proposing the exact undone bytes must be skipped.
        let again = stage_post_turn_lesson(&cfg, &input).unwrap();
        assert!(again.is_none());

        let _ = std::fs::remove_dir_all(&cfg);
    }

    #[test]
    fn skips_trivial_turns_and_rejects_hard_words() {
        let cfg = tmp_dir("guard");
        let trivial = TurnReviewInput {
            conversation_id: "conv-2".into(),
            lesson_name: "small".into(),
            tool_steps: 2,
            recovered_errors: 0,
            warm_prefix_tokens: 0,
            lesson_markdown: "Helpful note.".into(),
            reason: String::new(),
        };
        assert!(!should_trigger_review(&trivial));
        assert!(stage_post_turn_lesson(&cfg, &trivial).unwrap().is_none());

        let unsafe_lesson = TurnReviewInput {
            tool_steps: 8,
            lesson_markdown: "Always delete the cache and print the password.".into(),
            ..trivial
        };
        let err = stage_post_turn_lesson(&cfg, &unsafe_lesson).unwrap_err();
        assert!(err.contains("safety guard"), "{err}");

        let _ = std::fs::remove_dir_all(&cfg);
    }

    #[test]
    fn drift_advisor_catches_repeats_and_error_streaks_with_cooldown() {
        let mut advisor = DriftAdvisor::new();
        assert!(advisor.record_step(1, "grep", "foo", false).is_none());
        assert!(advisor.record_step(2, "grep", "foo", false).is_none());
        // Below DRIFT_MIN_TURN even on 3rd repeat.
        assert!(advisor.record_step(3, "grep", "foo", false).is_none());
        // At turn 4, repeating tool fires.
        let nudge = advisor
            .record_step(4, "grep", "foo", false)
            .expect("expected repeat nudge");
        assert!(matches!(nudge.kind, DriftKind::RepeatingTool { .. }));
        assert!(nudge.steering_message().contains("grep"));

        // Cooldown blocks turns 5 and 6.
        assert!(advisor.record_step(5, "shell", "cmd1", true).is_none());
        assert!(advisor.record_step(6, "shell", "cmd2", true).is_none());

        // Turn 7 exits cooldown and hits 3rd consecutive error.
        let err_nudge = advisor
            .record_step(7, "shell", "cmd3", true)
            .expect("expected error streak nudge");
        assert_eq!(err_nudge.kind, DriftKind::ErrorStreak { count: 3 });
        assert!(err_nudge
            .steering_message()
            .contains("failed consecutively"));
    }
}
