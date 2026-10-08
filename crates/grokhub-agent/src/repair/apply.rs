//! Spike-9 apply, verify and undo (pillar P6 steps 3–4).
//!
//! [`ApplyRun::begin`] refuses unless the user is in the seat, then takes the
//! restore point (file backups always, a system snapshot when there is one)
//! and writes its span before anything else. [`ApplyRun::next_action`] walks the
//! steps one at a time: each goes through `harness::decide` again; soft ones
//! go to Grok Build (path B, under the pill), hard ones park a hard card, and
//! hard-floor ones are never run. The cabin never runs a step itself and never
//! types into a password, polkit or UAC prompt: the user does.
//!
//! Verify re-runs the probe that found the problem. Only a passing re-check
//! writes `pass` on the verify span, and only then does the result say fixed.
//! Undo puts every backed-up file back byte for byte (it needs an `UndoAsk`)
//! and shows the plain way back to a system snapshot.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::json;

use crate::harness::{self, AccessMode, GateOutcome, HardClass, Origin, Span, UndoAsk, VERIFY_TOOL};

use super::fix::{gate_text, FixPlan, StepClass};
use super::interpret::Severity;
use super::probes::{Os, ProbeId};
use super::restore::{backup_files, detect_backends, restore_files, FileBackup, RestorePoint, Snapshot};
use super::DiagnoseReport;

/// Span tool for the restore point, written before the first step.
pub const RESTORE_TOOL: &str = "restore_point";
/// Span tool for each step handed to Grok Build.
pub const REPAIR_TOOL: &str = "repair_step";
/// Span tool for a refused or stopped apply.
pub const APPLY_TOOL: &str = "repair_apply";
/// Span tool for Undo.
pub const UNDO_TOOL: &str = "repair_undo";
/// What Grok Build replies when a step worked.
pub const STEP_OK: &str = "STEP_OK";
/// What Grok Build replies when a step didn't.
pub const STEP_FAILED: &str = "STEP_FAILED";
/// Apply with nobody at the computer.
pub const UNATTENDED: &str = "Fixing needs you at the computer, so I didn't change anything. Open GrokHub and press Fix it yourself.";

/// Who is asking, and where spans go.
pub struct ApplyCtx<'a> {
    pub config_dir: &'a Path,
    pub session_id: &'a str,
    pub os: Os,
    /// The user is in the seat. Heartbeat, automations and unattended runs are not.
    pub attended: bool,
    pub access: AccessMode,
    pub has_bin: &'a dyn Fn(&str) -> bool,
}

/// What the cabin does next.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Send `prompt` to Grok Build. `approved` when the user clicked Approve
    /// on this step's hard card, so GB's own Allow card asks once.
    Gb { prompt: String, command: String, approved: bool },
    /// Park a hard card for this step and wait for the click.
    Park { class: HardClass, command: String, plain: String },
    /// Re-run this probe off the UI thread, then call [`ApplyRun::record_verify`].
    Verify(ProbeId),
    /// Nothing more to do. [`ApplyRun::result_text`] says how it went.
    Done,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Ready,
    AwaitGb,
    AwaitCard,
    AwaitVerify,
    Done,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Queued {
    command: String,
    gate: String,
    plain: String,
    elevated: bool,
    /// The snapshot step: if it fails the file backup still holds.
    restore: bool,
}

/// One fix being applied.
#[derive(Debug, Clone)]
pub struct ApplyRun {
    pub id: String,
    pub plan: FixPlan,
    pub files: FileBackup,
    pub snapshot: Option<Snapshot>,
    config_dir: PathBuf,
    session_id: String,
    access: AccessMode,
    queue: Vec<Queued>,
    cursor: usize,
    approved: Option<usize>,
    phase: Phase,
    /// Why the run stopped before the end (a denied card, a failed step, Halt).
    pub stopped: Option<String>,
    /// The re-check's answer, once it ran.
    pub verified: Option<bool>,
    /// What the re-check found, in plain words.
    pub verify_note: String,
    pub undone: bool,
}

impl ApplyRun {
    /// Take the restore point and get ready. Refuses with a span when nobody
    /// is at the computer or the plan has nothing the app may run.
    pub fn begin(ctx: &ApplyCtx<'_>, plan: FixPlan) -> Result<Self, String> {
        let args = json!({ "finding": plan.finding.probe.key() }).to_string();
        let refuse = |why: &str| {
            let span = Span::deny(ctx.session_id, APPLY_TOOL, &args, why, "soft").from_origin(Origin::Repair);
            let _ = harness::append_span(ctx.config_dir, &span);
            why.to_string()
        };
        if !ctx.attended {
            return Err(refuse(UNATTENDED));
        }
        if plan.runnable() == 0 {
            return Err(refuse("This fix has no step I'm allowed to run, so I only explained what to do."));
        }
        let id = run_id();
        let files = backup_files(ctx.config_dir, &id, &plan.touches()).map_err(|e| refuse(&format!("I couldn't take a backup first, so I changed nothing: {e}")))?;
        let snapshot = detect_backends(ctx.os, ctx.has_bin)
            .first()
            .map(|b| Snapshot { backend: *b, label: format!("grokhub-repair-{id}") });
        let mut queue = Vec::new();
        if let Some(cmd) = snapshot.as_ref().and_then(|s| s.create_step()) {
            queue.push(Queued { gate: cmd.clone(), command: cmd, plain: "Save a system snapshot first.".into(), elevated: true, restore: true });
        }
        for s in plan.steps.iter().filter(|s| s.class != StepClass::HardFloor) {
            queue.push(Queued {
                command: s.command.clone(),
                gate: gate_text(&s.command, &s.touches),
                plain: s.plain.clone(),
                elevated: s.elevated,
                restore: false,
            });
        }
        let run = Self {
            id,
            plan,
            files,
            snapshot,
            config_dir: ctx.config_dir.to_path_buf(),
            session_id: ctx.session_id.to_string(),
            access: ctx.access,
            queue,
            cursor: 0,
            approved: None,
            phase: Phase::Ready,
            stopped: None,
            verified: None,
            verify_note: String::new(),
            undone: false,
        };
        let args = json!({
            "restore_ref": run.restore_ref(),
            "files": run.files.entries.len(),
            "finding": run.plan.finding.probe.key(),
        })
        .to_string();
        let mut span = Span::soft_allow(&run.session_id, RESTORE_TOOL, &args, "taken", "restore point before fixing", run.access, "repair");
        span.decision = "restore".into();
        run.write(span);
        Ok(run)
    }

    /// `files:<id>`, then `;<backend>:<label>` when a snapshot is planned.
    pub fn restore_ref(&self) -> String {
        let mut r = self.files.restore_ref();
        if let Some(s) = &self.snapshot {
            r.push(';');
            r.push_str(&s.restore_ref());
        }
        r
    }

    pub fn is_done(&self) -> bool {
        self.phase == Phase::Done
    }

    /// Waiting on Grok Build to finish the step it was sent.
    pub fn awaiting_gb(&self) -> bool {
        self.phase == Phase::AwaitGb
    }

    /// The next thing to do. Call it once the last action is answered.
    pub fn next_action(&mut self) -> Action {
        if self.stopped.is_some() || self.phase == Phase::Done {
            self.phase = Phase::Done;
            return Action::Done;
        }
        let total = self.queue.len();
        while let Some(q) = self.queue.get(self.cursor).cloned() {
            let n = self.cursor + 1;
            let args = json!({ "step": n, "command": q.command }).to_string();
            match harness::decide(harness::Step::Repair { command: &q.gate }) {
                GateOutcome::Refuse { reason } => {
                    self.write(Span::deny(&self.session_id, REPAIR_TOOL, &args, &reason, "floor"));
                    self.cursor += 1;
                }
                GateOutcome::Park { hard, .. } if self.approved != Some(self.cursor) => {
                    let class = hard.unwrap_or(HardClass::IrreversibleOs);
                    self.write(Span::hard_park(&self.session_id, REPAIR_TOOL, &args, class));
                    self.phase = Phase::AwaitCard;
                    return Action::Park { class, command: q.command, plain: q.plain };
                }
                hit => {
                    let approved = self.approved == Some(self.cursor);
                    let mut span = Span::soft_allow(&self.session_id, REPAIR_TOOL, &args, "sent to Grok Build", &q.plain, self.access, "grok-build");
                    if let GateOutcome::Park { hard: Some(class), .. } = hit {
                        span.approval_class = class.as_str().into();
                        span.hard_approved = true;
                    }
                    self.write(span.on_path("B"));
                    self.phase = Phase::AwaitGb;
                    return Action::Gb { prompt: gb_prompt(&q.command, n, total, q.elevated), command: q.command, approved };
                }
            }
        }
        if self.verified.is_none() {
            self.phase = Phase::AwaitVerify;
            return Action::Verify(self.plan.finding.probe);
        }
        self.phase = Phase::Done;
        Action::Done
    }

    /// Grok Build's reply to the step it was sent. False when it didn't work.
    pub fn step_finished(&mut self, reply: &str) -> bool {
        let ok = step_ok(reply);
        if let Some(q) = self.queue.get(self.cursor).cloned() {
            let args = json!({ "step": self.cursor + 1, "command": q.command }).to_string();
            if !ok && q.restore {
                // The file backup still holds; there is just no snapshot to go back to.
                self.snapshot = None;
                self.write(Span::deny(&self.session_id, RESTORE_TOOL, &args, "failed: no system snapshot, file backup kept", "soft"));
            } else if !ok {
                self.write(Span::deny(&self.session_id, REPAIR_TOOL, &args, "failed", "soft"));
                self.stopped = Some(format!("A step didn't work ({}), so I stopped there.", q.plain.trim_end_matches('.')));
            }
        }
        self.cursor += 1;
        self.approved = None;
        self.phase = Phase::Ready;
        ok
    }

    /// The user's click on this step's hard card. Deny stops the fix.
    pub fn answer_hard(&mut self, approve: bool) {
        if self.phase == Phase::Done {
            return;
        }
        if approve {
            self.approved = Some(self.cursor);
        } else {
            self.stopped = Some("You said no to a step, so I stopped there. Nothing after it ran.".into());
        }
        self.phase = Phase::Ready;
    }

    /// Halt or a closed cabin: stop with a span.
    pub fn halt(&mut self, why: &str) {
        if self.phase == Phase::Done {
            return;
        }
        let args = json!({ "restore_ref": self.restore_ref() }).to_string();
        self.write(Span::deny(&self.session_id, APPLY_TOOL, &args, why, "soft"));
        self.stopped = Some("I stopped the fix because you halted. Nothing more will run.".into());
        self.phase = Phase::Done;
    }

    /// The re-check's report. `pass` only when the probe ran and the problem
    /// it found is gone.
    pub fn record_verify(&mut self, report: &DiagnoseReport) -> bool {
        let probe = self.plan.finding.probe;
        let still = report.findings.iter().find(|f| f.probe == probe && f.severity >= Severity::Warning);
        let pass = report.ask.is_none() && report.ran.contains(&probe) && still.is_none();
        self.verify_note = match (still, report.findings.iter().find(|f| f.probe == probe)) {
            (Some(f), _) => f.plain.clone(),
            (None, Some(f)) if pass => f.plain.clone(),
            _ => report.ask.clone().unwrap_or_else(|| "I couldn't check it again.".into()),
        };
        let args = json!({ "probe": probe.key(), "restore_ref": self.restore_ref() }).to_string();
        let result = if pass { "pass" } else { "fail" };
        self.write(Span::soft_allow(&self.session_id, VERIFY_TOOL, &args, result, &self.verify_note, self.access, "repair"));
        self.verified = Some(pass);
        self.phase = Phase::Ready;
        let text = self.result_text();
        let claim = if pass { format!("{text}\nVERIFY_OK\nGOAL_COMPLETE") } else { text };
        self.write(Span::reply(&self.session_id, &claim, &[]));
        pass
    }

    /// The plain result. Says fixed only after a passing re-check.
    pub fn result_text(&self) -> String {
        if let Some(why) = &self.stopped {
            return format!("{why} Undo fix puts back what I changed.");
        }
        match self.verified {
            Some(true) => format!("Fixed. I checked again: {} If anything seems off, press Undo fix.", self.verify_note),
            Some(false) => format!("That didn't fix it. I checked again: {} Undo fix puts back what I changed.", self.verify_note),
            None => "Working on the fix…".into(),
        }
    }

    /// Put the backed-up files back and say how to reach the snapshot.
    pub fn undo(&mut self, ask: UndoAsk) -> String {
        let report = restore_files(&self.files, ask);
        self.undone = true;
        let args = json!({ "restore_ref": self.restore_ref() }).to_string();
        let result = format!("restored {}, removed {}, failed {}", report.restored.len(), report.removed.len(), report.failed.len());
        let mut span = Span::soft_allow(&self.session_id, UNDO_TOOL, &args, &result, "undo fix", self.access, "repair");
        span.decision = "undo".into();
        self.write(span);
        let mut out = match report.restored.len() + report.removed.len() {
            0 => "Undone. This fix didn't change any files, so there was nothing to put back.".to_string(),
            1 => "Undone. I put back the one file this fix changed, exactly as it was.".to_string(),
            n => format!("Undone. I put back the {n} files this fix changed, exactly as they were."),
        };
        for (path, copy) in &report.failed {
            out.push_str(&format!(
                "\nI couldn't put back {}: it needs admin rights. The saved copy is at {}.",
                path.display(),
                copy.display()
            ));
        }
        if let Some(g) = self.snapshot.as_ref().and_then(|s| s.undo_guidance()) {
            out.push('\n');
            out.push_str(&g);
        }
        out
    }

    fn write(&self, span: Span) {
        let _ = harness::append_span(&self.config_dir, &span.from_origin(Origin::Repair));
    }
}

/// What Grok Build gets for one step: run exactly this, never type a password.
pub fn gb_prompt(command: &str, n: usize, total: usize, elevated: bool) -> String {
    let admin = if elevated {
        " Your computer will show its own password prompt. Never type into it; the user types the password. Wait for them."
    } else {
        ""
    };
    format!(
        "GrokHub fix, step {n} of {total}. Run exactly this one command in the terminal, once, and nothing else: {command}\n\
         Don't change it or add other commands.{admin} When it finishes, reply with one line: {STEP_OK} if it worked, or {STEP_FAILED} and the reason."
    )
}

/// Grok Build's reply says the step worked.
pub fn step_ok(reply: &str) -> bool {
    reply.contains(STEP_OK) && !reply.contains(STEP_FAILED)
}

/// Milliseconds plus a counter, so two fixes started in the same instant
/// never share a backup folder.
fn run_id() -> String {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let ms = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
    format!("{ms}-{}", NEXT.fetch_add(1, Ordering::Relaxed))
}
