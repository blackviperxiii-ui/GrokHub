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
//! and each desktop step still asks the game guard itself. Each desktop step
//! also reads the saved switch again ([`SavedSwitch`]), so turning desktop
//! control off stops a running job's next step, a look at the screen too.
//! A run that started from a paired browser's words ([`RunGate::origin`])
//! has no desktop at all.

use std::path::PathBuf;
use std::sync::Arc;

use grokhub_agent::web_origin::RunOrigin;
use grokhub_agent::{
    AuthKind, CancelToken, DesktopOps, Gate, ModelClient, PermMode, ToolOutput, UnattendedDone,
    UnattendedRun,
};
use grokhub_core::wire::PermissionMode;

/// What an unattended run may do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RunGate {
    pub mode: PermMode,
    pub desktop: bool,
    /// Whose words the run goes on: a browser's run gets no desktop, and
    /// what it starts and writes is a browser's too.
    pub origin: RunOrigin,
}

impl RunGate {
    /// `/bg`, `/send` and `/consult`: Ask, read-only, no desktop.
    pub const BACKGROUND: Self = Self {
        mode: PermMode::Ask,
        desktop: false,
        origin: RunOrigin::Window,
    };

    /// A scheduled or resumed run: your Approvals, and the desktop while
    /// control is on and `game` (the guard's refusal) is `None`.
    pub fn scheduled(mode: PermMode, desktop_control: bool, game: Option<&str>) -> Self {
        Self {
            mode,
            desktop: desktop_control && game.is_none(),
            origin: RunOrigin::Window,
        }
    }

    /// The same gate for a run that also goes on `origin`'s words: a
    /// browser's anywhere makes it a browser's run.
    pub fn with_origin(self, origin: RunOrigin) -> Self {
        Self {
            origin: self.origin.and(origin),
            ..self
        }
    }

    pub fn gate(self) -> Gate {
        Gate {
            mode: self.mode,
            readonly_session: self.mode == PermMode::Ask,
            attended: false,
            desktop: self.desktop && grokhub_agent::web_origin::desktop_allowed(self.origin),
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
    // The folder whose settings hold the switch: the caller pins it.
    let config_dir = grokhub_agent::perm::config_dir();
    // The run starts here with its origin, which its helpers, its routine
    // steps and the jobs it writes take on. A session that started as a
    // browser's (a browser's chat, or a resumed browser run) stays one. A
    // browser run that can't be kept on file as one doesn't start.
    if let Err(error) =
        grokhub_agent::web_origin::start_run(&config_dir, &job.session_id, job.gate.origin)
    {
        return UnattendedDone {
            text: String::new(),
            thought: String::new(),
            stop_reason: "error".into(),
            error,
            usage: Default::default(),
            meter: String::new(),
            context_tokens_used: 0,
            context_window_tokens: 0,
            session_id: job.session_id,
            permission_cards: 0,
            elicit_cards: 0,
        };
    }
    let origin = grokhub_agent::web_origin::of_in(&config_dir, Some(&job.session_id));
    let gate = job.gate.with_origin(origin).gate();
    let desktop_ops = gate.desktop.then(|| {
        Box::new(SavedSwitch {
            inner: crate::agent::native_desktop(),
            config_dir,
        }) as Box<dyn DesktopOps>
    });
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
        resume: job.resume,
    })
}

/// The desktop behind the Settings switch as it is saved now. The gate reads
/// the switch once, when the run starts; this reads it again for every step,
/// in this app or in the background service, so a step after the switch went
/// off is refused and reads nothing off the screen.
struct SavedSwitch {
    inner: Box<dyn DesktopOps>,
    config_dir: PathBuf,
}

impl SavedSwitch {
    fn on(&self) -> bool {
        crate::settings::load(&self.config_dir).desktop_control
    }
}

impl DesktopOps for SavedSwitch {
    fn halted(&self) -> bool {
        self.inner.halted()
    }

    fn locked(&self) -> bool {
        self.inner.locked()
    }

    fn call(&self, name: &str, args: &serde_json::Value) -> ToolOutput {
        if !self.on() {
            return ToolOutput::err(*grokhub_core::desktop_mcp::OFF_MSG);
        }
        self.inner.call(name, args)
    }

    fn hint(&self, name: &str, args: &serde_json::Value) -> serde_json::Value {
        if !self.on() {
            return args.clone();
        }
        self.inner.hint(name, args)
    }

    /// In a chat a look asks each time and needs no desktop control. An
    /// unattended run has nobody to ask, so with the switch off it reads
    /// nothing off the screen either.
    fn look(&self, args: &serde_json::Value) -> ToolOutput {
        if !self.on() {
            return ToolOutput::err(*grokhub_core::desktop_mcp::OFF_MSG);
        }
        self.inner.look(args)
    }
}

#[cfg(test)]
#[path = "unattended_tests.rs"]
mod tests;
