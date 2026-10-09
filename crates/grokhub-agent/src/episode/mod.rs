//! Spike-3b: a long supervised desktop session (Hermes #10) you can watch,
//! steer and stop, that only says done after an independent check.
//!
//! One episode is one trace: every span of a desktop task started while
//! desktop control is on carries the same `episode` id, across turns, Steers
//! and pauses. The kernel loop is observe → propose tool → gate → execute (or
//! park) → observe → claim. It stops only when the worker returns no tools
//! and VerifyGate passes; `GOAL_COMPLETE` counts only after `VERIFY_OK`.
//! Halt, Stop, or [`EPISODE_IDLE`] without a step also end it.
//!
//! There is no step or wall-time cap. A loop (an identical call failing
//! twice, the same call changing nothing three times, the same failure three
//! times; see [`loops`]) re-plans at once. After [`STALL_REPLAN`] steps that
//! move nothing the worker is told to re-plan ([`REPLAN_NOTE`]) and goes on.
//! Any re-plan steps the worker up one model for [`STEP_UP_STEPS`] steps,
//! quietly, then it drops back. A
//! checker reject, or a checker that can't run, re-plans with its reason
//! instead of waiting; the same reject on an unchanged screen
//! [`SAME_REJECT_END`] times ends the episode with a named note. Every
//! step goes through `harness::decide`; the episode adds no executor and no
//! bypass. Hard class always parks.

mod kernel;
pub mod loops;
mod view;
mod verify;

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use crate::harness::{Ladder, Origin, Span};

pub use kernel::{
    fan_out, run_episode, step_span, EpisodeOut, EpisodeStop, FileParks, KernelIn, Parks, CHECKER_ERROR, DEAD_WORKER,
    EPISODE_RULES, FANOUT_CAPPED, VERIFY_REJECT,
};
pub use verify::{
    parse_verdict, verify_call, verify_call_at, verify_gate, Observation, Verdict, CHECKER_UNAVAILABLE, ESCALATED_CLASS,
    JUDGE_SYSTEM, OBSERVATION_CAP,
};
pub use view::{
    clip_bytes, zoom_schema, EpisodeView, FoldDone, Folder, ViewNode, FOLD_CAP_BYTES, FOLD_SYSTEM, VIEW_BUDGET_BYTES,
    VIEW_HEAD, ZOOM_TOOL,
};

/// Steps in a row that move nothing before the worker is told to re-plan.
/// The slow backstop: [`loops`] catches a repeated call sooner.
pub const STALL_REPLAN: u32 = 5;
/// Worker calls that go one model up after a re-plan, before dropping back.
pub const STEP_UP_STEPS: u32 = 3;
/// The worker's next note after a re-plan.
pub const REPLAN_NOTE: &str = "The last steps changed nothing. Stop repeating them. Re-read the goal, \
say in one line what you tried, then pick a different approach (another tool, another route or a smaller sub-step), \
take a fresh screenshot and go on.";
/// The same checker reject, on an unchanged screen, this many times in a row
/// ends the episode with a named note instead of re-planning again.
pub const SAME_REJECT_END: u32 = 3;
/// An open episode with no step for this long ends.
pub const EPISODE_IDLE: Duration = Duration::from_secs(10 * 60);
/// Most independent reads one fan-out runs side by side.
pub const FANOUT_CAP: usize = 20;
/// Span tool for episode markers (`begin`, `end`, `fold`).
pub const EPISODE_TOOL: &str = "episode";
/// Park ids the episode kernel posts. The cabin card answers them; the
/// kernel writes their spans.
pub const PARK_PREFIX: &str = "epk-";
/// How much of a goal or goal step a span keeps.
pub const GOAL_CAP: usize = 200;
/// How much of a goal or Steer the worker and the checker see. A 400-char
/// instruction must not reach them cut mid-sentence.
pub const GOAL_PROMPT_CAP: usize = 4_000;

/// A fresh episode id: `ep-<ms hex>-<n>`.
pub fn new_episode_id(now_ms: u64) -> String {
    static N: AtomicU32 = AtomicU32::new(0);
    format!("ep-{now_ms:x}-{}", N.fetch_add(1, Ordering::Relaxed))
}

/// "Desktop session · 12 steps · 4 min", the Work-tree group header.
pub fn episode_header(steps: u32, elapsed: Duration) -> String {
    let unit = if steps == 1 { "step" } else { "steps" };
    format!("Desktop session · {steps} {unit} · {} min", elapsed.as_secs() / 60)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EpisodeEnd {
    /// `GOAL_COMPLETE` and VerifyGate's `VERIFY_OK`.
    Verified,
    Halt,
    Stop,
    Idle,
    /// The checker kept rejecting for the same reason on an unchanged
    /// screen ([`SAME_REJECT_END`] times): ended with a named note.
    Unconfirmed,
}

impl EpisodeEnd {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Verified => "verified",
            Self::Halt => "halt",
            Self::Stop => "stop",
            Self::Idle => "idle",
            Self::Unconfirmed => "unconfirmed",
        }
    }
}

/// One hard step waiting on the user's card. The raw args stay in memory to
/// run the step once on Approve; spans and the park file get redacted args.
#[derive(Debug, Clone)]
pub struct OpenPark {
    pub id: String,
    pub step: u32,
    pub tool: String,
    pub args: String,
    pub args_redacted: String,
    pub class: crate::harness::HardClass,
    pub parked_ms: u64,
    pub goal_step: String,
}

/// The fixed shape every step records: goal step, tool, decision,
/// `ui_changed`, result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepShape {
    pub goal_step: String,
    pub tool: String,
    pub decision: String,
    pub ui_changed: Option<bool>,
    pub result: String,
}

/// How much of a result a step line keeps.
pub const STEP_RESULT_CAP: usize = 120;

impl StepShape {
    /// The step's one line in the episode view.
    pub fn line(&self, n: u32) -> String {
        let ui = match self.ui_changed {
            Some(true) => "true",
            Some(false) => "false",
            None => "unknown",
        };
        let goal: String = self.goal_step.chars().take(60).collect();
        let result: String = self.result.replace('\n', " ").chars().take(STEP_RESULT_CAP).collect();
        format!(
            "#{n} goal={goal:?} tool={} decision={} ui_changed={ui} result={result:?}",
            self.tool, self.decision
        )
    }
}

/// One supervised desktop episode.
#[derive(Debug, Clone)]
pub struct Episode {
    pub id: String,
    pub chat_id: String,
    /// The task, redacted. VerifyGate checks against this.
    pub goal: String,
    /// What the worker is on now (the goal, a Steer, or a ladder note).
    pub goal_step: String,
    pub started_ms: u64,
    pub last_ms: u64,
    pub steps: u32,
    /// Steps in a row that moved nothing (see [`Episode::note_progress`]).
    pub stall: u32,
    pub ended: Option<EpisodeEnd>,
    pub parks: Vec<OpenPark>,
    pub ladder: Ladder,
    /// A ladder repair prompt for the next step.
    pub note: Option<String>,
    /// The user turn spans are tagged with.
    pub turn: u32,
    /// Spans this episode wrote, for the detectors and the ladder.
    pub trail: Vec<Span>,
    pub(crate) seen_findings: Vec<String>,
    /// The latest checker reject: its reason, the observation hash it saw,
    /// and how many times in a row both stayed the same.
    pub(crate) last_reject: Option<(String, String, u32)>,
    /// The last few steps, for [`loops::note_call`].
    pub(crate) ring: VecDeque<loops::LoopEntry>,
    /// Worker calls still stepped up after the latest re-plan.
    pub step_up: u32,
}

fn cap_text(text: &str, held: &[String]) -> String {
    let clean = grokhub_core::redact_held_secrets(&grokhub_core::redact_secrets(text), held);
    clean.trim().chars().take(GOAL_PROMPT_CAP).collect()
}

impl Episode {
    pub fn begin(id: &str, chat_id: &str, goal: &str, now_ms: u64, held: &[String]) -> Self {
        let goal = cap_text(goal, held);
        Self {
            id: id.into(),
            chat_id: chat_id.into(),
            goal_step: goal.clone(),
            goal,
            started_ms: now_ms,
            last_ms: now_ms,
            steps: 0,
            stall: 0,
            ended: None,
            parks: Vec::new(),
            ladder: Ladder::new(),
            note: None,
            turn: 0,
            trail: Vec::new(),
            seen_findings: Vec::new(),
            last_reject: None,
            ring: VecDeque::new(),
            step_up: 0,
        }
    }

    /// Count one recorded step toward a stall. A step moved when the screen
    /// changed, or when there was no screen reading and it didn't fail. A
    /// park is neutral. True when [`STALL_REPLAN`] steps in a row moved
    /// nothing; the count then starts over.
    pub fn note_progress(&mut self, decision: &str, ui_changed: Option<bool>, failed: bool) -> bool {
        if decision == "park" {
            return false;
        }
        let moved = match ui_changed {
            Some(changed) => changed,
            None => !failed,
        };
        if moved {
            self.stall = 0;
            return false;
        }
        self.stall = self.stall.saturating_add(1);
        if self.stall >= STALL_REPLAN {
            self.stall = 0;
            return true;
        }
        false
    }

    /// Count a checker reject. True when the same reason on the same
    /// observation came [`SAME_REJECT_END`] times in a row.
    pub fn note_reject(&mut self, why: &str, obs_hash: &str) -> bool {
        let n = match &self.last_reject {
            Some((w, h, n)) if w == why && h == obs_hash => n.saturating_add(1),
            _ => 1,
        };
        self.last_reject = Some((why.to_string(), obs_hash.to_string(), n));
        n >= SAME_REJECT_END
    }

    /// No step for [`EPISODE_IDLE`].
    pub fn idle(&self, now_ms: u64) -> bool {
        self.ended.is_none() && now_ms.saturating_sub(self.last_ms) >= EPISODE_IDLE.as_millis() as u64
    }

    /// A Steer or a new message: same episode, same parks, new goal step.
    pub fn steer(&mut self, note: &str, held: &[String]) {
        let note = cap_text(note, held);
        if !note.is_empty() {
            self.goal_step = note;
        }
    }

    pub fn header(&self, now_ms: u64) -> String {
        episode_header(self.steps, Duration::from_millis(now_ms.saturating_sub(self.started_ms)))
    }

    /// An episode marker span (`begin`, `pause`, `resume`, `end`, `fold`).
    pub fn marker(&self, decision: &str, result: &str, claim: &str) -> Span {
        let mut s = Span::deny(&self.chat_id, EPISODE_TOOL, "{}", result, "soft");
        s.decision = decision.into();
        s.claim = claim.into();
        s.goal_step = self.goal_step.clone();
        s.from_origin(Origin::User).in_episode(&self.id)
    }
}

#[cfg(test)]
mod tests;
