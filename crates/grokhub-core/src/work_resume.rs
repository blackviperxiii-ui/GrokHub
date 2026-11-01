//! KB-03: resume from the checkpoint after a disconnect, a model swap or a
//! restart. A run that ends without finishing is marked Interrupted on its
//! card (Blocked, with the cause and where it left off). Resuming hands the
//! model one short block built only from the card's checkpoint and steps,
//! never the progress log or a transcript, at most [`RESUME_BLOCK_MAX`] bytes,
//! once, in the resume turn's input.
//!
//! The policy is plain data work too: agent-owned automation and background
//! cards resume on their own once per interruption and at most
//! [`AUTO_RESUME_PER_DAY`] times a day; work you started waits for Resume.

use serde::{Deserialize, Serialize};

use crate::wire::PermissionMode;
use crate::work_item::{
    clip_line, StepStatus, WorkKind, WorkOrigin, CHECKPOINT_BLOCKER_MAX, CHECKPOINT_GOAL_MAX,
    CHECKPOINT_NEXT_MAX, CHECKPOINT_TAG_MAX,
};
use crate::work_run::{end_run, next_step, RunEnd};
use crate::workboard::{BoardCard, BoardStatus};

/// The resume block never grows past this, in bytes.
pub const RESUME_BLOCK_MAX: usize = 1200;
/// Auto-resumes one card may take in a day.
pub const AUTO_RESUME_PER_DAY: usize = 2;
const DAY_MS: u64 = 24 * 60 * 60 * 1000;
/// The longest title the block names, in bytes.
const BLOCK_TITLE_MAX: usize = 100;

/// Run hosts. Each one marks only its own orphaned cards at startup, so the
/// cabin never cuts a run the shell still has going.
pub const HOST_CABIN: &str = "cabin";
pub const HOST_SHELL: &str = "shell";

/// The model tag a run on the device carries. The swap check compares only
/// "on the device or not": a Settings change mid-run does not reach a live turn.
pub const LOCAL_MODEL_TAG: &str = "local";

/// What cut a run short.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InterruptCause {
    /// The app quit or crashed with the card in Doing.
    Restart,
    /// The engine worker went away mid-run.
    WorkerGone,
    /// The provider answered with an error and the run ended.
    ProviderError,
    /// The route moved the run to another model mid-run.
    ModelSwap,
}

impl InterruptCause {
    pub fn text(self) -> &'static str {
        match self {
            Self::Restart => "the app restarted",
            Self::WorkerGone => "the engine stopped",
            Self::ProviderError => "the model returned an error",
            Self::ModelSwap => "the model changed mid-run",
        }
    }

    /// A failed run's error as a cause. The engine's own "worker gone" line and
    /// the swap note keep their cause; anything else came from the provider.
    pub fn from_failure(why: &str) -> Self {
        if why.trim() == MODEL_SWAP_WHY {
            Self::ModelSwap
        } else if why.trim() == crate::worker_gone_status()
            || crate::proc_util::is_sigterm_status(why)
        {
            Self::WorkerGone
        } else {
            Self::ProviderError
        }
    }
}

/// The failure a background run ends with when the swap check cuts it.
pub const MODEL_SWAP_WHY: &str = "The model changed mid-run.";

/// One interruption on a card.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Interruption {
    pub cause: InterruptCause,
    pub at_ms: u64,
    /// This interruption's one auto-resume was used, or the cap refused it.
    #[serde(default, skip_serializing_if = "is_false")]
    pub auto_used: bool,
}

fn is_false(v: &bool) -> bool {
    !*v
}

/// Who runs a card, in which permission mode, and how its last run was cut.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunState {
    /// [`HOST_CABIN`] or [`HOST_SHELL`]. Empty on cards no host claimed.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub host: String,
    /// The permission mode the run started in (`PermissionMode::as_str`).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub mode: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interrupted: Option<Interruption>,
    /// When the last auto-resumes ran, newest last, within a day.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub auto_resumes: Vec<u64>,
}

impl RunState {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

/// A run started on the card under `host` in `mode`.
pub fn claim_run(card: &mut BoardCard, host: &str, mode: PermissionMode) {
    card.run_state.host = clip_line(host, CHECKPOINT_TAG_MAX);
    card.run_state.mode = mode.as_str().to_string();
}

/// The card's run in Doing ended without finishing: Blocked, Interrupted, with
/// its checkpoint. A blocker the run already had stays; else the cause is the
/// blocker. `next` points at the first open step. False when it was not in Doing.
pub fn interrupt_card(card: &mut BoardCard, cause: InterruptCause, now: u64) -> bool {
    if card.status != BoardStatus::InProgress {
        return false;
    }
    card.clear_inflight_undo();
    card.status = BoardStatus::Blocked;
    card.updated_ms = now;
    let next = next_step(&card.steps);
    let has_steps = !card.steps.is_empty();
    let title = card.title.clone();
    let cp = card.checkpoint.get_or_insert_with(Default::default);
    if cp.goal.trim().is_empty() {
        cp.goal = clip_line(&title, CHECKPOINT_GOAL_MAX);
    }
    if has_steps {
        cp.next = next;
    }
    if cp.blocker.trim().is_empty() {
        cp.blocker = clip_line(
            &format!("Interrupted: {}.", cause.text()),
            CHECKPOINT_BLOCKER_MAX,
        );
    }
    cp.at_ms = now;
    card.run_state.interrupted = Some(Interruption {
        cause,
        at_ms: now,
        auto_used: false,
    });
    true
}

/// A run in Doing that failed is Interrupted rather than just Blocked: the
/// final checkpoint keeps the failure as its blocker and the cause follows the
/// error. Done and Stopped are left to the caller. True when it interrupted.
pub fn interrupt_failed(card: &mut BoardCard, end: &RunEnd, now: u64) -> bool {
    let RunEnd::Failed(why) = end else {
        return false;
    };
    if card.status != BoardStatus::InProgress {
        return false;
    }
    end_run(card, end, now);
    interrupt_card(card, InterruptCause::from_failure(why), now)
}

/// Startup: every Doing card `host` ran that has no live run is Interrupted
/// (the app restarted under it). Cards another host runs, cards no run ever
/// started on, and cards in `live_ids` are left alone. Returns their ids.
pub fn interrupt_orphans(
    cards: &mut [BoardCard],
    host: &str,
    live_ids: &[String],
    now: u64,
) -> Vec<String> {
    let mut cut = Vec::new();
    for c in cards.iter_mut() {
        let started = c.checkpoint.as_ref().is_some_and(|cp| cp.attempt > 0);
        let live = live_ids
            .iter()
            .any(|l| *l == c.id || c.thread_id.as_deref() == Some(l.as_str()));
        if c.status != BoardStatus::InProgress || c.run_state.host != host || !started || live {
            continue;
        }
        if interrupt_card(c, InterruptCause::Restart, now) {
            cut.push(c.id.clone());
        }
    }
    cut
}

/// Did the route move this run on or off the device since it started?
/// `started_model` is the checkpoint's model; `on_device` is the route now.
pub fn model_swapped(started_model: &str, on_device: bool) -> bool {
    let started = started_model.trim();
    !started.is_empty() && (started == LOCAL_MODEL_TAG) != on_device
}

/// The model tag a run starts under.
pub fn run_model_tag(model: &str, on_device: bool) -> String {
    if on_device {
        LOCAL_MODEL_TAG.to_string()
    } else {
        model.trim().to_string()
    }
}

/// How a resume talks to the model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResumeSession {
    /// Same model and its session file is intact: continue that native session
    /// (the provider's prefix cache still holds it) and add the block.
    Continue(String),
    /// Another model, or the session is gone: a fresh session with only the block.
    Fresh,
}

/// Continue the card's session only when the model is the one it ran on, the
/// session file is intact, and no model swap cut it.
pub fn resume_session(card: &BoardCard, model_now: &str, session_intact: bool) -> ResumeSession {
    let Some(cp) = card.checkpoint.as_ref() else {
        return ResumeSession::Fresh;
    };
    let swapped = card
        .run_state
        .interrupted
        .is_some_and(|i| i.cause == InterruptCause::ModelSwap);
    let same_model =
        !cp.model.trim().is_empty() && cp.model.trim().eq_ignore_ascii_case(model_now.trim());
    if session_intact && same_model && !swapped && !cp.session.trim().is_empty() {
        ResumeSession::Continue(cp.session.trim().to_string())
    } else {
        ResumeSession::Fresh
    }
}

fn step_word(s: StepStatus) -> &'static str {
    match s {
        StepStatus::Planned => "planned",
        StepStatus::Doing => "doing",
        StepStatus::Done => "done",
        StepStatus::Skipped => "skipped",
        StepStatus::Failed => "failed",
    }
}

/// The parts of a block, so trimming drops whole lines.
struct BlockParts {
    head: String,
    goal: String,
    next: String,
    blocker: String,
    /// (step number from 1, status, text)
    steps: Vec<(usize, StepStatus, String)>,
    refs: Vec<String>,
}

impl BlockParts {
    fn render(&self) -> String {
        let mut out = self.head.clone();
        for (name, value) in [
            ("Goal", &self.goal),
            ("Next", &self.next),
            ("Blocked by", &self.blocker),
        ] {
            if !value.is_empty() {
                out.push_str(&format!("\n{name}: {value}"));
            }
        }
        if !self.steps.is_empty() {
            out.push_str("\nSteps:");
            for (n, status, text) in &self.steps {
                out.push_str(&format!("\n{n}. [{}] {text}", step_word(*status)));
            }
        }
        if !self.refs.is_empty() {
            out.push_str(&format!("\nRefs: {}", self.refs.join(", ")));
        }
        out.push_str(RESUME_TAIL);
        out
    }
}

/// The last line of every block. The `work_*` tools load only by name.
const RESUME_TAIL: &str = "\nPick up at Next and do not redo done steps. Check the card with \
     work_get and checkpoint it with work_update (load them by name).";

/// The resume note for `card`, built only from its checkpoint and steps, at most
/// [`RESUME_BLOCK_MAX`] bytes. Over the cap, done steps go first (oldest first),
/// then refs, then open steps from the end. Goal, next and blocker always stay.
pub fn resume_block(card: &BoardCard) -> String {
    let cp = card.checkpoint.clone().unwrap_or_default();
    let cause = card
        .run_state
        .interrupted
        .map(|i| format!(" after {}", i.cause.text()))
        .unwrap_or_default();
    let title = clip_line(&card.title, BLOCK_TITLE_MAX);
    let id = clip_line(&card.id, CHECKPOINT_TAG_MAX);
    let next = if cp.next.trim().is_empty() {
        next_step(&card.steps)
    } else {
        cp.next.clone()
    };
    let next = clip_line(&next, CHECKPOINT_NEXT_MAX);
    let mut parts = BlockParts {
        head: format!("Resuming workboard card {id} \"{title}\"{cause}, from its checkpoint."),
        goal: clip_line(&cp.goal, CHECKPOINT_GOAL_MAX),
        next: next.clone(),
        blocker: clip_line(&cp.blocker, CHECKPOINT_BLOCKER_MAX),
        steps: card
            .steps
            .iter()
            .enumerate()
            .map(|(i, s)| (i + 1, s.status, s.text.clone()))
            .collect(),
        refs: cp.refs.clone(),
    };
    loop {
        let out = parts.render();
        if out.len() <= RESUME_BLOCK_MAX {
            return out;
        }
        let done = parts
            .steps
            .iter()
            .position(|(_, s, _)| matches!(s, StepStatus::Done | StepStatus::Skipped));
        if let Some(i) = done {
            parts.steps.remove(i);
            continue;
        }
        if parts.refs.pop().is_some() {
            continue;
        }
        let open = parts.steps.iter().rposition(|(_, _, t)| *t != next);
        if let Some(i) = open {
            parts.steps.remove(i);
            continue;
        }
        if !parts.steps.is_empty() {
            parts.steps.clear();
            continue;
        }
        // Goal, next and blocker alone fit by their caps; this only guards a
        // future cap change.
        let mut end = RESUME_BLOCK_MAX;
        while !out.is_char_boundary(end) {
            end -= 1;
        }
        return out[..end].to_string();
    }
}

/// Agent-owned automation and background work may resume on its own. Work you
/// started (a chat task, a desktop session), or a card you touched, never does.
pub fn auto_resumable(card: &BoardCard) -> bool {
    card.origin == Some(WorkOrigin::Agent)
        && !card.touched
        && matches!(card.kind, WorkKind::Automation | WorkKind::Background)
}

/// What the auto-resume tick does with one card.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutoResume {
    /// Resume it now.
    Go,
    /// Already resumed twice today: it stays Blocked and the bell says so.
    GiveUp,
}

/// `None` unless the card is Interrupted, agent-owned, and this interruption's
/// one auto-resume is still unused.
pub fn auto_resume_decision(card: &BoardCard, now: u64) -> Option<AutoResume> {
    let i = card.run_state.interrupted?;
    if card.status != BoardStatus::Blocked || i.auto_used || !auto_resumable(card) {
        return None;
    }
    let today = card
        .run_state
        .auto_resumes
        .iter()
        .filter(|at| now.saturating_sub(**at) < DAY_MS)
        .count();
    Some(if today >= AUTO_RESUME_PER_DAY {
        AutoResume::GiveUp
    } else {
        AutoResume::Go
    })
}

/// Spend this interruption's auto-resume. `ran` counts it toward the daily cap;
/// a give-up only marks the interruption handled. Old entries are dropped.
pub fn note_auto_resume(card: &mut BoardCard, ran: bool, now: u64) {
    if let Some(i) = card.run_state.interrupted.as_mut() {
        i.auto_used = true;
    }
    let rs = &mut card.run_state;
    rs.auto_resumes
        .retain(|at| now.saturating_sub(*at) < DAY_MS);
    if ran {
        rs.auto_resumes.push(now);
    }
}

/// The blocker an auto-resume give-up leaves.
pub fn give_up_blocker() -> String {
    format!("Auto-resume stopped after {AUTO_RESUME_PER_DAY} tries today. Resume it yourself.")
}

/// The card leaves Blocked for Doing on resume. The interruption is cleared;
/// the run hooks start a new attempt on it.
pub fn begin_resume(card: &mut BoardCard, now: u64) {
    card.status = BoardStatus::InProgress;
    card.archived_ms = 0;
    card.updated_ms = now;
    card.run_state.interrupted = None;
}

/// The permission mode a resumed run gets: the one it started in, never looser
/// than the mode now. An unattended run started in Ask or Auto stays there.
pub fn resume_mode(started: &str, now: PermissionMode) -> PermissionMode {
    fn rank(m: PermissionMode) -> u8 {
        match m {
            PermissionMode::Ask => 0,
            PermissionMode::Auto => 1,
            PermissionMode::AlwaysApprove => 2,
        }
    }
    match PermissionMode::parse(started) {
        Some(s) if rank(s) < rank(now) => s,
        _ => now,
    }
}

/// A resumed step may drive the desktop: a desktop session's card, or any card
/// while desktop control is on. The game guard is asked before it runs.
pub fn resume_touches_desktop(card: &BoardCard, desktop_control: bool) -> bool {
    desktop_control || card.kind == WorkKind::Episode
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::work_item::{
        Checkpoint, WorkStep, CHECKPOINT_REFS_MAX, CHECKPOINT_REF_MAX, STEPS_MAX, STEP_TEXT_MAX,
    };

    const NOW: u64 = 10 * DAY_MS;

    fn doing(host: &str) -> BoardCard {
        let mut c = BoardCard::new("Fix the tray icon", "", "");
        c.status = BoardStatus::InProgress;
        c.run = true;
        c.thread_id = Some("t1".into());
        c.checkpoint = Some(Checkpoint {
            goal: "Fix the tray icon on Windows".into(),
            next: "Re-run the app tests".into(),
            session: "native-abc".into(),
            model: "grok-4".into(),
            attempt: 1,
            ..Default::default()
        });
        claim_run(&mut c, host, PermissionMode::Auto);
        c
    }

    fn maximal() -> BoardCard {
        let mut c = BoardCard::new(&"T".repeat(120), "", "");
        c.id = "w-max".into();
        c.steps = (0..STEPS_MAX)
            .map(|i| WorkStep {
                text: format!("{i}{}", "s".repeat(STEP_TEXT_MAX - 2)),
                status: if i < 6 {
                    StepStatus::Done
                } else {
                    StepStatus::Planned
                },
            })
            .collect();
        c.checkpoint = Some(Checkpoint {
            goal: "G".repeat(CHECKPOINT_GOAL_MAX),
            next: "N".repeat(CHECKPOINT_NEXT_MAX),
            blocker: "B".repeat(CHECKPOINT_BLOCKER_MAX),
            refs: vec!["r".repeat(CHECKPOINT_REF_MAX); CHECKPOINT_REFS_MAX],
            session: "x".repeat(CHECKPOINT_TAG_MAX),
            model: "m".repeat(CHECKPOINT_TAG_MAX),
            attempt: 9,
            at_ms: 1,
        });
        c.run_state.interrupted = Some(Interruption {
            cause: InterruptCause::ProviderError,
            at_ms: 1,
            auto_used: false,
        });
        c
    }

    #[test]
    fn restart_interrupts_only_this_hosts_orphans() {
        let mut ours = doing(HOST_CABIN);
        ours.steps = vec![
            WorkStep {
                text: "Read the tray code".into(),
                status: StepStatus::Done,
            },
            WorkStep {
                text: "Patch the DPI scaling".into(),
                status: StepStatus::Doing,
            },
        ];
        let shell = doing(HOST_SHELL);
        let mut live = doing(HOST_CABIN);
        live.thread_id = Some("t-live".into());
        let mut never_ran = BoardCard::new("Dragged to Doing", "", "");
        never_ran.status = BoardStatus::InProgress;
        never_ran.run_state.host = HOST_CABIN.into();
        let mut cards = vec![ours, shell, live, never_ran];
        let cut = interrupt_orphans(&mut cards, HOST_CABIN, &["t-live".into()], NOW);
        assert_eq!(cut, vec![cards[0].id.clone()]);
        let c = &cards[0];
        assert_eq!(c.status, BoardStatus::Blocked);
        let cp = c.checkpoint.as_ref().unwrap();
        assert_eq!(cp.next, "Patch the DPI scaling");
        assert_eq!(cp.blocker, "Interrupted: the app restarted.");
        assert_eq!(cp.at_ms, NOW);
        assert_eq!(
            c.run_state.interrupted,
            Some(Interruption {
                cause: InterruptCause::Restart,
                at_ms: NOW,
                auto_used: false
            })
        );
        assert_eq!(cards[1].status, BoardStatus::InProgress, "the shell's run");
        assert_eq!(cards[2].status, BoardStatus::InProgress, "a live run");
        assert_eq!(cards[3].status, BoardStatus::InProgress, "no run started");
        assert!(!interrupt_card(
            &mut cards[0],
            InterruptCause::WorkerGone,
            NOW
        ));
    }

    #[test]
    fn interruption_keeps_a_real_blocker_and_survives_json() {
        let mut c = doing(HOST_CABIN);
        c.checkpoint.as_mut().unwrap().blocker = "Needs the signing key".into();
        assert!(interrupt_card(&mut c, InterruptCause::ProviderError, 5));
        assert_eq!(
            c.checkpoint.as_ref().unwrap().blocker,
            "Needs the signing key"
        );
        let json = serde_json::to_string(&c).unwrap();
        assert!(json.contains("\"runState\":{"), "{json}");
        assert!(json.contains("\"cause\":\"provider_error\""), "{json}");
        let back: BoardCard = serde_json::from_str(&json).unwrap();
        assert_eq!(back.run_state, c.run_state);
        let bare = serde_json::to_string(&BoardCard::new("x", "", "")).unwrap();
        assert!(!bare.contains("runState"), "{bare}");
    }

    #[test]
    fn a_failed_run_is_interrupted_with_its_error() {
        let mut c = doing(HOST_CABIN);
        assert!(!interrupt_failed(&mut c, &RunEnd::Done, 3));
        assert!(!interrupt_failed(&mut c, &RunEnd::Stopped, 3));
        assert_eq!(c.status, BoardStatus::InProgress);
        assert!(interrupt_failed(
            &mut c,
            &RunEnd::Failed("502 from the provider".into()),
            3
        ));
        assert_eq!(c.status, BoardStatus::Blocked);
        assert_eq!(
            c.checkpoint.as_ref().unwrap().blocker,
            "Failed: 502 from the provider"
        );
        assert_eq!(
            c.run_state.interrupted.map(|i| i.cause),
            Some(InterruptCause::ProviderError)
        );
        assert!(!interrupt_failed(&mut c, &RunEnd::Failed("x".into()), 4));
    }

    #[test]
    fn block_carries_checkpoint_and_steps_only() {
        let mut c = doing(HOST_CABIN);
        c.id = "w-1".into();
        c.notes = "private notes".into();
        c.detail = "status line".into();
        c.steps = vec![
            WorkStep {
                text: "Read the tray code".into(),
                status: StepStatus::Done,
            },
            WorkStep {
                text: "Re-run the app tests".into(),
                status: StepStatus::Planned,
            },
        ];
        c.checkpoint.as_mut().unwrap().refs = vec!["src/tray.rs".into()];
        assert!(interrupt_card(&mut c, InterruptCause::WorkerGone, 7));
        assert_eq!(
            resume_block(&c),
            "Resuming workboard card w-1 \"Fix the tray icon\" after the engine stopped, from its checkpoint.\n\
             Goal: Fix the tray icon on Windows\n\
             Next: Re-run the app tests\n\
             Blocked by: Interrupted: the engine stopped.\n\
             Steps:\n\
             1. [done] Read the tray code\n\
             2. [planned] Re-run the app tests\n\
             Refs: src/tray.rs\n\
             Pick up at Next and do not redo done steps. Check the card with work_get and \
             checkpoint it with work_update (load them by name)."
        );
    }

    #[test]
    fn maximal_block_fits_the_cap_and_keeps_goal_next_and_blocker() {
        let c = maximal();
        let full = BlockParts {
            head: String::new(),
            goal: String::new(),
            next: String::new(),
            blocker: String::new(),
            steps: c
                .steps
                .iter()
                .enumerate()
                .map(|(i, s)| (i + 1, s.status, s.text.clone()))
                .collect(),
            refs: c.checkpoint.as_ref().unwrap().refs.clone(),
        }
        .render();
        assert!(full.len() > 1000, "the uncut card is over budget");
        let block = resume_block(&c);
        assert!(block.len() <= RESUME_BLOCK_MAX, "{} bytes", block.len());
        assert!(block.contains(&format!("Goal: {}", "G".repeat(CHECKPOINT_GOAL_MAX))));
        assert!(block.contains(&format!("Next: {}", "N".repeat(CHECKPOINT_NEXT_MAX))));
        assert!(block.contains(&format!(
            "Blocked by: {}",
            "B".repeat(CHECKPOINT_BLOCKER_MAX)
        )));
        assert!(block.contains("work_get") && block.contains("work_update"));
        assert!(!block.contains("[done]"), "done steps go first: {block}");
        assert!(!block.contains("Refs:"), "then refs: {block}");
        assert!(
            !block.contains(&"x".repeat(CHECKPOINT_TAG_MAX)),
            "no session id"
        );
        // Open steps keep their own numbers for work_update.
        assert!(block.contains("\n7. [planned] 6"), "{block}");
    }

    #[test]
    fn swap_or_lost_session_means_a_fresh_session() {
        let mut c = doing(HOST_CABIN);
        assert_eq!(
            resume_session(&c, "grok-4", true),
            ResumeSession::Continue("native-abc".into())
        );
        assert_eq!(resume_session(&c, "grok-code", true), ResumeSession::Fresh);
        assert_eq!(resume_session(&c, "grok-4", false), ResumeSession::Fresh);
        interrupt_card(&mut c, InterruptCause::ModelSwap, 1);
        assert_eq!(resume_session(&c, "grok-4", true), ResumeSession::Fresh);
        assert!(model_swapped("grok-4", true));
        assert!(model_swapped(LOCAL_MODEL_TAG, false));
        assert!(!model_swapped("grok-4", false));
        assert!(!model_swapped("", true), "no reading at start, no swap");
        assert_eq!(run_model_tag("grok-4", true), LOCAL_MODEL_TAG);
        assert_eq!(run_model_tag(" grok-4 ", false), "grok-4");
        assert_eq!(
            InterruptCause::from_failure(MODEL_SWAP_WHY),
            InterruptCause::ModelSwap
        );
        assert_eq!(
            InterruptCause::from_failure(crate::worker_gone_status()),
            InterruptCause::WorkerGone
        );
        assert_eq!(
            InterruptCause::from_failure("429 rate limited"),
            InterruptCause::ProviderError
        );
    }

    #[test]
    fn agent_work_auto_resumes_twice_a_day_then_gives_up() {
        let mut c = doing(HOST_CABIN).by_agent();
        c.kind = WorkKind::Background;
        let mut t = NOW;
        for _ in 0..AUTO_RESUME_PER_DAY {
            assert!(interrupt_card(&mut c, InterruptCause::ProviderError, t));
            assert_eq!(auto_resume_decision(&c, t), Some(AutoResume::Go));
            note_auto_resume(&mut c, true, t);
            assert_eq!(auto_resume_decision(&c, t), None, "once per interruption");
            begin_resume(&mut c, t);
            assert_eq!(c.status, BoardStatus::InProgress);
            t += 60_000;
        }
        assert!(interrupt_card(&mut c, InterruptCause::ProviderError, t));
        assert_eq!(auto_resume_decision(&c, t), Some(AutoResume::GiveUp));
        note_auto_resume(&mut c, false, t);
        assert_eq!(auto_resume_decision(&c, t), None);
        assert_eq!(c.run_state.auto_resumes.len(), AUTO_RESUME_PER_DAY);
        // A day later the cap has room again.
        let later = t + DAY_MS;
        c.run_state.interrupted.as_mut().unwrap().auto_used = false;
        assert_eq!(auto_resume_decision(&c, later), Some(AutoResume::Go));
        note_auto_resume(&mut c, true, later);
        assert_eq!(c.run_state.auto_resumes, vec![later]);
    }

    #[test]
    fn user_work_never_auto_resumes() {
        let mut task = doing(HOST_CABIN).by_agent();
        task.kind = WorkKind::Task;
        let mut desktop = doing(HOST_CABIN).by_agent();
        desktop.kind = WorkKind::Episode;
        let mut yours = doing(HOST_CABIN);
        yours.kind = WorkKind::Background;
        yours.origin = Some(WorkOrigin::User);
        let mut touched = doing(HOST_CABIN).by_agent();
        touched.kind = WorkKind::Automation;
        touched.touched = true;
        for mut c in [task, desktop, yours, touched] {
            assert!(interrupt_card(&mut c, InterruptCause::Restart, NOW));
            assert!(!auto_resumable(&c));
            assert_eq!(auto_resume_decision(&c, NOW), None, "{:?}", c.kind);
        }
    }

    #[test]
    fn resume_keeps_the_mode_it_started_in() {
        use PermissionMode::*;
        assert_eq!(resume_mode("ask", Auto), Ask);
        assert_eq!(resume_mode("auto", Auto), Auto);
        assert_eq!(resume_mode("auto", AlwaysApprove), Auto);
        assert_eq!(
            resume_mode("always-approve", Ask),
            Ask,
            "never looser than now"
        );
        assert_eq!(resume_mode("", Auto), Auto);
        let mut c = doing(HOST_CABIN);
        assert_eq!(c.run_state.mode, "auto");
        interrupt_card(&mut c, InterruptCause::Restart, 1);
        begin_resume(&mut c, 2);
        assert_eq!(c.run_state.mode, "auto", "resume leaves the mode alone");
        assert!(c.run_state.interrupted.is_none());
        assert!(resume_touches_desktop(&c, true));
        assert!(!resume_touches_desktop(&c, false));
        c.kind = WorkKind::Episode;
        assert!(resume_touches_desktop(&c, false));
    }
}
