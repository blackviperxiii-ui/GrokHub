//! Optional model-backed sleep pass. The deterministic tidy stays in `dream.rs`.
//! This module owns the phase machine and the rule that, while the pass is on,
//! a turn may only propose a core line. The dream pass is the one that may
//! write a lesson, and even then the lesson goes through the replay gate.

use super::dream::DreamReport;
use super::signal::{propose_line, Noted};
use super::store::AmrStore;
use super::write::{remember_line, LineWrite, Remembered};
use super::AmrError;

/// Output budget for the one nightly model call. The request has no token
/// field, so the caller truncates the reply to this many estimated tokens
/// before it parses JSON.
pub const MODEL_DREAM_OUTPUT_TOKEN_CAP: u32 = 4096;

/// Who is asking to write a core line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoreWriter {
    /// A turn, reflect, or `/remember`.
    InTurn,
    /// The sleep pass after the deterministic dream.
    DreamPass,
}

/// Sleep-time phases. Each variant is one step. The next step is [`DreamStep`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DreamPhase {
    DeterministicDone { looked_at: usize },
    ModelPass,
    ProposalsQueued { model: bool },
    MorningSummary { text: String },
}

/// A step that is illegal from the current phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DreamPhaseError;

impl std::fmt::Display for DreamPhaseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("illegal dream transition")
    }
}

/// One legal move on [`DreamPhase`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DreamStep {
    RunModel,
    SkipModel,
    ModelParsed { had_items: bool },
    Summarize { text: String },
}

impl DreamPhase {
    pub fn after_deterministic(report: &DreamReport) -> Self {
        Self::DeterministicDone {
            looked_at: report.looked_at,
        }
    }

    pub fn advance(self, step: DreamStep) -> Result<Self, DreamPhaseError> {
        match (self, step) {
            (Self::DeterministicDone { .. }, DreamStep::RunModel) => Ok(Self::ModelPass),
            (Self::DeterministicDone { .. }, DreamStep::SkipModel) => {
                Ok(Self::ProposalsQueued { model: false })
            }
            (Self::ModelPass, DreamStep::ModelParsed { .. }) => {
                Ok(Self::ProposalsQueued { model: true })
            }
            (Self::ProposalsQueued { .. }, DreamStep::Summarize { text }) => {
                Ok(Self::MorningSummary { text })
            }
            _ => Err(DreamPhaseError),
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Self::DeterministicDone { .. } => "deterministic_done",
            Self::ModelPass => "model_pass",
            Self::ProposalsQueued { .. } => "proposals_queued",
            Self::MorningSummary { .. } => "morning_summary",
        }
    }

    pub fn summary(&self) -> Option<&str> {
        match self {
            Self::MorningSummary { text } => Some(text),
            _ => None,
        }
    }
}

/// Knobs for the optional model call. Off unless the caller sets `enabled`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelDreamOpts {
    pub enabled: bool,
    pub output_token_cap: u32,
}

impl Default for ModelDreamOpts {
    fn default() -> Self {
        Self {
            enabled: false,
            output_token_cap: MODEL_DREAM_OUTPUT_TOKEN_CAP,
        }
    }
}

/// What the model pass queued. Empty when the pass is off.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ModelDreamReport {
    pub calls: u32,
    pub output_token_cap: u32,
    pub lessons_applied: u32,
    pub lessons_refused: Vec<String>,
    pub proposed: Vec<String>,
    pub refused: Vec<String>,
}

/// The morning line, also the calibration log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MorningNote {
    pub text: String,
}

/// While model dream is on, neither a turn nor the dream pass writes a core
/// line. Insights stay proposed until someone approves them. The lesson file
/// is the dream pass's own write, and only when the flag is on.
pub fn allows_direct_core_write(dream_model: bool, writer: CoreWriter) -> bool {
    if !dream_model {
        return true;
    }
    match writer {
        CoreWriter::InTurn | CoreWriter::DreamPass => false,
    }
}

pub fn allows_lesson_write(dream_model: bool, writer: CoreWriter) -> bool {
    match (dream_model, writer) {
        (true, CoreWriter::DreamPass) => true,
        (true, CoreWriter::InTurn) | (false, CoreWriter::InTurn | CoreWriter::DreamPass) => false,
    }
}

/// A core line after the sleep-time rule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StagedCore {
    Proposed(Noted),
    Remembered(Remembered),
}

pub fn stage_core_line(
    store: &AmrStore,
    write: &LineWrite<'_>,
    now_ms: u64,
    dream_model: bool,
    writer: CoreWriter,
) -> Result<StagedCore, AmrError> {
    if allows_direct_core_write(dream_model, writer) {
        remember_line(store, write, now_ms).map(StagedCore::Remembered)
    } else {
        propose_line(store, write.text, write.source, now_ms).map(StagedCore::Proposed)
    }
}

pub fn morning_text(proposed: &[String], lesson_applied: bool, threshold_log: &str) -> String {
    let head = match proposed {
        [] => "Proposed nothing".to_string(),
        [one] => format!("Proposed {one}"),
        many => format!("Proposed {}", many.len()),
    };
    let lesson = if lesson_applied {
        "applied dream-lessons"
    } else {
        "no lesson applied"
    };
    format!("{head}. {lesson}. {threshold_log}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    struct Tmp(PathBuf);

    impl Tmp {
        fn new(label: &str) -> Self {
            static N: AtomicU64 = AtomicU64::new(0);
            let n = N.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "grokhub-dream-model-{label}-{}-{n}",
                std::process::id()
            ));
            let _ = fs::remove_dir_all(&path);
            let store = AmrStore::at(path.join("amr"));
            store.init().unwrap();
            Self(path)
        }

        fn store(&self) -> AmrStore {
            AmrStore::at(self.0.join("amr"))
        }
    }

    impl Drop for Tmp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn write(text: &str) -> LineWrite<'_> {
        LineWrite {
            text,
            source: "chat:t",
            tags: Vec::new(),
            confidence: 0.9,
            revive: false,
        }
    }

    #[test]
    fn phases_run_deterministic_then_model_then_summary() {
        let done = DreamPhase::after_deterministic(&DreamReport {
            looked_at: 3,
            ..DreamReport::default()
        });
        assert_eq!(done.name(), "deterministic_done");
        let err = done
            .clone()
            .advance(DreamStep::Summarize {
                text: "too soon".into(),
            })
            .unwrap_err();
        assert_eq!(err.to_string(), "illegal dream transition");
        let queued = done.advance(DreamStep::RunModel).unwrap();
        assert_eq!(queued.name(), "model_pass");
        let queued = queued
            .advance(DreamStep::ModelParsed { had_items: true })
            .unwrap();
        assert_eq!(queued.name(), "proposals_queued");
        let morning = queued
            .advance(DreamStep::Summarize {
                text: morning_text(&[], false, "reflect_min_confidence 0.70"),
            })
            .unwrap();
        assert_eq!(morning.name(), "morning_summary");
        assert_eq!(
            morning.summary(),
            Some("Proposed nothing. no lesson applied. reflect_min_confidence 0.70")
        );
    }

    #[test]
    fn skip_model_still_reaches_the_morning_summary() {
        let done = DreamPhase::after_deterministic(&DreamReport::default());
        let queued = done.advance(DreamStep::SkipModel).unwrap();
        assert_eq!(queued.name(), "proposals_queued");
        let morning = queued
            .advance(DreamStep::Summarize {
                text: "Proposed nothing. no lesson applied. reflect_min_confidence 0.70".into(),
            })
            .unwrap();
        assert_eq!(morning.name(), "morning_summary");
    }

    #[test]
    fn model_dream_defaults_off_and_caps_one_call_at_4096() {
        let opts = ModelDreamOpts::default();
        assert!(!opts.enabled);
        assert_eq!(opts.output_token_cap, 4096);
        assert_eq!(MODEL_DREAM_OUTPUT_TOKEN_CAP, 4096);
    }

    #[test]
    fn flag_on_proposes_and_flag_off_remembers() {
        let tmp = Tmp::new("stage");
        let store = tmp.store();
        let proposed = stage_core_line(
            &store,
            &write("prefers quiet mornings"),
            1,
            true,
            CoreWriter::InTurn,
        )
        .unwrap();
        assert!(matches!(proposed, StagedCore::Proposed(Noted::New(_))));
        assert!(store.recall("quiet").is_empty());
        let dream = stage_core_line(
            &store,
            &write("speaks in short lines"),
            2,
            true,
            CoreWriter::DreamPass,
        )
        .unwrap();
        assert!(matches!(dream, StagedCore::Proposed(Noted::New(_))));
        assert!(store.recall("short").is_empty());
        let kept = stage_core_line(
            &store,
            &write("keeps a slate board"),
            3,
            false,
            CoreWriter::InTurn,
        )
        .unwrap();
        assert!(matches!(kept, StagedCore::Remembered(Remembered::New(_))));
        let lines: Vec<String> = store
            .recall("slate")
            .into_iter()
            .map(|hit| hit.line)
            .collect();
        assert_eq!(lines, vec!["keeps a slate board".to_string()]);
        assert!(!allows_direct_core_write(true, CoreWriter::InTurn));
        assert!(!allows_direct_core_write(true, CoreWriter::DreamPass));
        assert!(allows_direct_core_write(false, CoreWriter::InTurn));
        assert!(allows_lesson_write(true, CoreWriter::DreamPass));
        assert!(!allows_lesson_write(true, CoreWriter::InTurn));
        assert!(!allows_lesson_write(false, CoreWriter::DreamPass));
    }

    #[test]
    fn morning_line_names_one_proposal_or_the_count() {
        assert_eq!(
            morning_text(
                &["prefers quiet mornings".into()],
                true,
                "reflect_min_confidence 0.70"
            ),
            "Proposed prefers quiet mornings. applied dream-lessons. reflect_min_confidence 0.70"
        );
        assert_eq!(
            morning_text(
                &[
                    "prefers quiet mornings".into(),
                    "speaks in short lines".into()
                ],
                false,
                "reflect_min_confidence 0.75"
            ),
            "Proposed 2. no lesson applied. reflect_min_confidence 0.75"
        );
    }
}
