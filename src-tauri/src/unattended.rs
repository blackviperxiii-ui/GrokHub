//! Unattended runs as the cabin ran them (`native_unattended.rs`): a scheduled
//! automation, a resumed run card, and `/bg`, `/send` and `/consult`. Nobody is
//! there to answer a card, so the gate is unattended and closed permits deny
//! at once.
//!
//! Each run binds its job's tool set: `TOOLS:`, `NEED_SHELL` and `NEED_MCP`
//! lean it, and a job that declares none keeps the full set, as chat does. A
//! scheduled or resumed run takes your Approvals. Ask stays a read-only
//! session, so an allow rule cannot turn a refused write into one that runs.
//! Desktop tools come only while desktop control is on and no game holds it,
//! and each desktop step still asks the game guard itself.

use std::path::PathBuf;
use std::sync::Arc;

use grokhub_agent::{
    AuthKind, CancelToken, Gate, ModelClient, PermMode, UnattendedDone, UnattendedRun,
};
use grokhub_core::wire::PermissionMode;

/// What an unattended run may do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RunGate {
    pub mode: PermMode,
    pub desktop: bool,
}

impl RunGate {
    /// `/bg`, `/send` and `/consult`: Ask, read-only, no desktop.
    pub const BACKGROUND: Self = Self {
        mode: PermMode::Ask,
        desktop: false,
    };

    /// A scheduled or resumed run: your Approvals, and the desktop while
    /// control is on and `game` (the guard's refusal) is `None`.
    pub fn scheduled(mode: PermMode, desktop_control: bool, game: Option<&str>) -> Self {
        Self {
            mode,
            desktop: desktop_control && game.is_none(),
        }
    }

    pub fn gate(self) -> Gate {
        Gate {
            mode: self.mode,
            readonly_session: self.mode == PermMode::Ask,
            attended: false,
            desktop: self.desktop,
        }
    }

    /// The mode the run's card records.
    pub fn wire(self) -> PermissionMode {
        match self.mode {
            PermMode::Ask => PermissionMode::Ask,
            PermMode::Auto => PermissionMode::Auto,
            PermMode::Always => PermissionMode::AlwaysApprove,
        }
    }

    /// The same gate in `mode`, as a resumed run keeps the mode it started in.
    pub fn in_mode(self, mode: PermissionMode) -> Self {
        Self {
            mode: match mode {
                PermissionMode::Ask => PermMode::Ask,
                PermissionMode::Auto => PermMode::Auto,
                PermissionMode::AlwaysApprove => PermMode::Always,
            },
            ..self
        }
    }
}

/// Why the desktop is off for a run now: a game or an anti-cheat holds it.
#[cfg(desktop)]
pub fn game_hold() -> Option<String> {
    grokhub_desktop::game_guard::peek().blocks(grokhub_core::game_guard::Guarded::DesktopControl)
}

/// Phones have no desktop tools to hold.
#[cfg(not(desktop))]
pub fn game_hold() -> Option<String> {
    None
}

/// One unattended run.
pub struct Job {
    pub client: Arc<dyn ModelClient + Send + Sync>,
    pub model: String,
    pub workspace: PathBuf,
    pub session_id: String,
    pub prompt: String,
    /// The job text whose `TOOLS:` line binds the tool set, when the prompt
    /// is not it (a resumed automation's instructions); else the prompt.
    pub tools_from: Option<String>,
    pub gate: RunGate,
    pub cancel: CancelToken,
    /// Continue the native session `session_id` when its file is there.
    pub resume: bool,
}

pub fn run(job: Job) -> UnattendedDone {
    let set = grokhub_agent::tool_set_from_job(job.tools_from.as_deref().unwrap_or(&job.prompt));
    let _set = grokhub_agent::bind_job_tool_set(job.session_id.clone(), set);
    let gate = job.gate.gate();
    let desktop_ops = gate.desktop.then(crate::agent::native_desktop);
    let system = crate::agent::system_prompt(&job.workspace, gate.desktop, "");
    grokhub_agent::run_unattended(UnattendedRun {
        client: job.client,
        workspace: job.workspace,
        model: job.model,
        effort: None,
        system,
        session_id: job.session_id,
        auth_kind: AuthKind::ApiKey,
        prompt: job.prompt,
        image: None,
        mode: gate.mode,
        readonly_session: gate.readonly_session,
        desktop: gate.desktop,
        cancel: job.cancel,
        halt: Box::new(crate::agent::NeverHalted),
        desktop_ops,
        imagine_bearer: String::new(),
        resume: job.resume,
    })
}

#[cfg(test)]
#[path = "unattended_tests.rs"]
mod tests;
