//! Permission gate v0. `decide` still treats Auto like Ask.
//! Phase 5 auto-review runs in the loop, and only on calls this gate would ask about.
//! Plan and btw stay read-only.

use std::path::Path;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::sync::Mutex;
use std::time::Duration;

use grokhub_core::desktop_mcp::{HALT_MSG, LOCK_MSG, OFF_MSG};

use crate::CancelToken;

pub const READ_ONLY_PHASE: &str = "read-only in this phase";

const READONLY: &[&str] = &[
    "read_file",
    "list_dir",
    "grep",
    "glob",
    "get_command_or_subagent_output",
    "scheduler_list",
    "search_tool",
    "skill",
    "todo_write",
    "ask_user_question",
    "enter_plan_mode",
    "exit_plan_mode",
];
const EDIT: &[&str] = &["write", "search_replace"];
const SHELL: &[&str] = &["run_terminal_command"];
const CONTROL: &[&str] = &[
    "kill_command_or_subagent",
    "monitor",
    "scheduler_create",
    "scheduler_delete",
    "spawn_subagent",
    "send_subagent_message",
];
const DESKTOP: &[&str] = &[
    "screenshot",
    "click",
    "move",
    "drag",
    "scroll",
    "type",
    "key",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermMode {
    Ask,
    Auto,
    Always,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Gate {
    pub mode: PermMode,
    pub readonly_session: bool,
    pub attended: bool,
    pub desktop: bool,
}

impl Gate {
    /// Phase 3a behavior: every mutating tool is refused with [`READ_ONLY_PHASE`].
    pub fn phase_readonly() -> Self {
        Self {
            mode: PermMode::Ask,
            readonly_session: true,
            attended: true,
            desktop: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeskFlags {
    pub halted: bool,
    pub locked: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    Run,
    Ask,
    Refuse(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermAnswer {
    Allow,
    Always,
    Deny,
    Cancel,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Waited {
    Answer(PermAnswer),
    Cancelled,
    Halted,
}

pub trait PermitWait: Send {
    fn wait(&self, call_id: &str, cancel: &CancelToken, halted: &dyn Fn() -> bool) -> Waited;
    fn drain(&self) {}
}

#[derive(Debug)]
pub struct PermitNote {
    pub id: String,
    pub answer: PermAnswer,
}

pub struct PermitInbox {
    rx: Mutex<Receiver<PermitNote>>,
}

impl PermitInbox {
    pub fn pair() -> (Sender<PermitNote>, Self) {
        let (tx, rx) = std::sync::mpsc::channel();
        (tx, Self { rx: Mutex::new(rx) })
    }
}

impl PermitWait for PermitInbox {
    fn drain(&self) {
        let rx = self.rx.lock().unwrap_or_else(|err| err.into_inner());
        while rx.try_recv().is_ok() {}
    }

    fn wait(&self, call_id: &str, cancel: &CancelToken, halted: &dyn Fn() -> bool) -> Waited {
        loop {
            if cancel.is_cancelled() {
                return Waited::Cancelled;
            }
            if halted() {
                return Waited::Halted;
            }
            let rx = self.rx.lock().unwrap_or_else(|err| err.into_inner());
            match rx.recv_timeout(Duration::from_millis(30)) {
                Ok(note) if note.id == call_id => return Waited::Answer(note.answer),
                Ok(_) => continue,
                Err(RecvTimeoutError::Timeout) => continue,
                Err(RecvTimeoutError::Disconnected) => return Waited::Cancelled,
            }
        }
    }
}

pub struct ClosedPermits;

impl PermitWait for ClosedPermits {
    fn wait(&self, _call_id: &str, cancel: &CancelToken, halted: &dyn Fn() -> bool) -> Waited {
        if cancel.is_cancelled() {
            return Waited::Cancelled;
        }
        if halted() {
            return Waited::Halted;
        }
        Waited::Answer(PermAnswer::Deny)
    }
}

pub fn is_readonly(name: &str) -> bool {
    READONLY.contains(&name)
}

pub fn is_desktop(name: &str) -> bool {
    DESKTOP.contains(&name)
}

pub fn is_known(name: &str) -> bool {
    is_readonly(name)
        || EDIT.contains(&name)
        || SHELL.contains(&name)
        || CONTROL.contains(&name)
        || is_desktop(name)
}

pub fn readonly_refusal(name: &str) -> String {
    format!("{READ_ONLY_PHASE}: `{name}` is not available. Use read_file, list_dir, grep, or glob.")
}

pub fn mcp_policy_deny(name: &str) -> String {
    format!("Tool `{name}` was not executed: Denied by permission policy: deny rule on mcp")
}

pub fn unattended_deny(name: &str) -> String {
    let label = if is_desktop(name) {
        "mcp matching \"grokhub-desktop__*\""
    } else if SHELL.contains(&name) || name == "kill_command_or_subagent" || name == "monitor" {
        "bash"
    } else {
        "edit"
    };
    format!("Tool `{name}` was not executed: Denied by permission policy: deny rule on {label}")
}

pub fn user_rejected(name: &str) -> String {
    format!("User rejected the execution for tool `{name}`")
}

pub fn user_cancelled(name: &str) -> String {
    format!("User cancelled the execution for tool `{name}`")
}

pub fn decide(gate: &Gate, name: &str, latched_always: bool, desk: Option<DeskFlags>) -> Decision {
    if is_readonly(name) {
        return Decision::Run;
    }
    if gate.readonly_session {
        return Decision::Refuse(readonly_refusal(name));
    }
    if !is_known(name) {
        return Decision::Refuse(format!("unknown tool `{name}`"));
    }
    if is_desktop(name) {
        if !gate.desktop {
            return Decision::Refuse(OFF_MSG.to_string());
        }
        match desk {
            None => return Decision::Refuse(OFF_MSG.to_string()),
            Some(DeskFlags { halted: true, .. }) => return Decision::Refuse(HALT_MSG.to_string()),
            Some(DeskFlags { locked: true, .. }) => return Decision::Refuse(LOCK_MSG.to_string()),
            Some(_) => {}
        }
    }
    if gate.mode == PermMode::Always || latched_always {
        return Decision::Run;
    }
    if gate.attended {
        return Decision::Ask;
    }
    // Unattended Ask refuses. Unattended Auto refuses here too; the loop may
    // replace that soft refusal with the Phase 5 judge. This function does not.
    Decision::Refuse(unattended_deny(name))
}

/// Gate v0, then the permission engine when a policy is loaded.
/// `decide` itself is unchanged. The native loop calls this.
pub fn decide_with(
    gate: &Gate,
    name: &str,
    arguments: &str,
    latched_always: bool,
    desk: Option<DeskFlags>,
    workspace: &Path,
    policy: Option<&crate::perm::Policy>,
) -> Decision {
    if let Some(decision) = crate::mcp::permission(gate, name, arguments, latched_always, policy) {
        return decision;
    }
    if name == "monitor" && crate::tasks::monitor_watch_only(arguments) {
        return Decision::Run;
    }
    // Explore is read-only work. It runs in plan mode and while unattended.
    // A general spawn stays on the normal gate, so it is never looser than the parent.
    if crate::subagent::spawn_is_explore(name, arguments) {
        return Decision::Run;
    }
    let base = decide(gate, name, latched_always, desk);
    match policy {
        Some(policy) => crate::perm::govern(base, gate, name, arguments, workspace, policy),
        None => base,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use grokhub_core::desktop_mcp::{HALT_MSG, LOCK_MSG, OFF_MSG};

    fn gate(mode: PermMode, readonly: bool, attended: bool, desktop: bool) -> Gate {
        Gate {
            mode,
            readonly_session: readonly,
            attended,
            desktop,
        }
    }

    #[test]
    fn unattended_and_user_messages_match_the_cli() {
        assert_eq!(
            unattended_deny("write"),
            "Tool `write` was not executed: Denied by permission policy: deny rule on edit"
        );
        assert_eq!(
            unattended_deny("search_replace"),
            "Tool `search_replace` was not executed: Denied by permission policy: deny rule on edit"
        );
        assert_eq!(
            unattended_deny("run_terminal_command"),
            "Tool `run_terminal_command` was not executed: Denied by permission policy: deny rule on bash"
        );
        assert_eq!(
            unattended_deny("click"),
            "Tool `click` was not executed: Denied by permission policy: deny rule on mcp matching \"grokhub-desktop__*\""
        );
        assert_eq!(
            unattended_deny("kill_command_or_subagent"),
            "Tool `kill_command_or_subagent` was not executed: Denied by permission policy: deny rule on bash"
        );
        assert_eq!(
            unattended_deny("monitor"),
            "Tool `monitor` was not executed: Denied by permission policy: deny rule on bash"
        );
        assert_eq!(
            unattended_deny("scheduler_create"),
            "Tool `scheduler_create` was not executed: Denied by permission policy: deny rule on edit"
        );
        assert_eq!(
            unattended_deny("scheduler_delete"),
            "Tool `scheduler_delete` was not executed: Denied by permission policy: deny rule on edit"
        );
        assert_eq!(
            user_rejected("write"),
            "User rejected the execution for tool `write`"
        );
        assert_eq!(
            user_cancelled("write"),
            "User cancelled the execution for tool `write`"
        );
    }

    #[test]
    fn gate_matrix_for_each_mode() {
        for mode in [PermMode::Ask, PermMode::Auto, PermMode::Always] {
            for attended in [true, false] {
                let g = gate(mode, false, attended, false);
                assert_eq!(decide(&g, "read_file", false, None), Decision::Run);
                assert_eq!(decide(&g, "grep", false, None), Decision::Run);
            }
        }

        let ask = gate(PermMode::Ask, false, true, false);
        let auto = gate(PermMode::Auto, false, true, false);
        let always = gate(PermMode::Always, false, true, true);
        assert_eq!(decide(&ask, "write", false, None), Decision::Ask);
        assert_eq!(decide(&auto, "search_replace", false, None), Decision::Ask);
        assert_eq!(decide(&auto, "run_terminal_command", false, None), Decision::Ask);
        assert_eq!(decide(&always, "write", false, None), Decision::Run);
        assert_eq!(decide(&ask, "write", true, None), Decision::Run);

        let ask_away = gate(PermMode::Ask, false, false, false);
        let auto_away = gate(PermMode::Auto, false, false, false);
        let always_away = gate(PermMode::Always, false, false, false);
        assert_eq!(
            decide(&ask_away, "write", false, None),
            Decision::Refuse(unattended_deny("write"))
        );
        assert_eq!(
            decide(&ask_away, "run_terminal_command", false, None),
            Decision::Refuse(unattended_deny("run_terminal_command"))
        );
        assert_eq!(
            decide(&auto_away, "write", false, None),
            Decision::Refuse(unattended_deny("write"))
        );
        assert_eq!(
            decide(&auto_away, "run_terminal_command", false, None),
            Decision::Refuse(unattended_deny("run_terminal_command"))
        );
        assert_eq!(decide(&always_away, "search_replace", false, None), Decision::Run);

        for mode in [PermMode::Ask, PermMode::Auto, PermMode::Always] {
            for attended in [true, false] {
                let plan = gate(mode, true, attended, true);
                let refused = decide(&plan, "write", false, None);
                match refused {
                    Decision::Refuse(text) => assert!(text.contains(READ_ONLY_PHASE), "{text}"),
                    other => panic!("plan must refuse write, got {other:?}"),
                }
                assert!(matches!(
                    decide(&plan, "screenshot", false, Some(DeskFlags { halted: false, locked: false })),
                    Decision::Refuse(text) if text.contains(READ_ONLY_PHASE)
                ));
            }
        }

        let off = gate(PermMode::Always, false, true, false);
        assert_eq!(decide(&off, "screenshot", false, None), Decision::Refuse(OFF_MSG.into()));
        assert_eq!(decide(&off, "type", false, None), Decision::Refuse(OFF_MSG.into()));

        let on = gate(PermMode::Always, false, true, true);
        assert_eq!(
            decide(&on, "screenshot", false, Some(DeskFlags { halted: true, locked: false })),
            Decision::Refuse(HALT_MSG.into())
        );
        assert_eq!(
            decide(&on, "screenshot", false, Some(DeskFlags { halted: false, locked: true })),
            Decision::Refuse(LOCK_MSG.into())
        );
        assert_eq!(
            decide(&on, "key", false, Some(DeskFlags { halted: false, locked: false })),
            Decision::Run
        );
        assert_eq!(decide(&on, "screenshot", false, None), Decision::Refuse(OFF_MSG.into()));

        let ask_desk = gate(PermMode::Ask, false, true, true);
        assert_eq!(
            decide(&ask_desk, "click", false, Some(DeskFlags { halted: false, locked: false })),
            Decision::Ask
        );
        assert_eq!(
            decide(
                &gate(PermMode::Ask, false, false, true),
                "screenshot",
                false,
                Some(DeskFlags { halted: false, locked: false })
            ),
            Decision::Refuse(unattended_deny("screenshot"))
        );
    }
}
