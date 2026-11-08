//! Round 3 item 28: one safety event per tool call that a rule, the Auto
//! judge, a hook or the sandbox stopped or asked about, and per call the
//! person turned down. Each event names the request that produced the call
//! ([`super::replay`]), so the turn can be replayed exactly.
//!
//! An event holds no arguments and no refusal text, only what happened and
//! who decided. It is still sealed like the rest of the learned tier, rolls to
//! `safety-events.1.jsonl` past 1 MiB, and a roll older than
//! [`super::replay::KEEP_DAYS`] is removed.

use std::fs;
use std::path::Path;
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};

use super::at_rest;

pub const EVENTS_FILE: &str = "safety-events.jsonl";
const ROLLED_FILE: &str = "safety-events.1.jsonl";
const ROLL_AT: u64 = 1024 * 1024;
pub const AAD_SAFETY: &str = "grokhub:safety:v1";

/// What happened to the call.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    /// Refused before it ran.
    Stopped,
    /// The person was asked; `answer` says what they said.
    Asked,
}

/// Who made the call stop or ask.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DecidedBy {
    /// Permission mode, a deny or ask rule, a hard class or a floor.
    Rule,
    /// The Auto judge.
    Judge,
    /// A PreToolUse hook.
    Hook,
    /// The call asked to run outside the shell sandbox.
    Sandbox,
}

/// The person's answer to an ask.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Answer {
    Allowed,
    AlwaysAllowed,
    Denied,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SafetyEvent {
    pub ts_ms: u64,
    /// The chat (conversation id).
    pub chat: String,
    pub tool: String,
    pub outcome: Outcome,
    pub by: DecidedBy,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub answer: Option<Answer>,
    /// The stored request whose reply made this call; empty when none was stored.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub request: String,
}

/// The coarse kind of a gate decision, for working out who changed it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Run,
    Ask,
    Refuse,
}

impl Kind {
    pub fn of(d: &crate::gate::Decision) -> Self {
        match d {
            crate::gate::Decision::Run => Self::Run,
            crate::gate::Decision::Ask => Self::Ask,
            crate::gate::Decision::Refuse(_) => Self::Refuse,
        }
    }
}

/// Who is behind the final decision: the last stage that changed it (hook,
/// then judge), else the gate's rules. A gate ask for a call that wants out
/// of the sandbox is the sandbox's.
pub fn decided_by(base: Kind, reviewed: Kind, last: Kind, outside_sandbox: bool) -> DecidedBy {
    if last != reviewed {
        DecidedBy::Hook
    } else if reviewed != base {
        DecidedBy::Judge
    } else if outside_sandbox && last == Kind::Ask {
        DecidedBy::Sandbox
    } else {
        DecidedBy::Rule
    }
}

/// Whether the call's arguments ask to run outside the shell sandbox.
pub fn wants_outside_sandbox(arguments: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(arguments)
        .ok()
        .and_then(|v| v.get(crate::sandbox::OUTSIDE_ARG).and_then(|b| b.as_bool()))
        == Some(true)
}

/// Append one event, sealed. No key means nothing is written.
pub fn record(config_dir: &Path, event: &SafetyEvent) -> Result<(), String> {
    let path = config_dir.join(EVENTS_FILE);
    let rolled = config_dir.join(ROLLED_FILE);
    if fs::metadata(&path)
        .map(|m| m.len() > ROLL_AT)
        .unwrap_or(false)
    {
        let _ = fs::rename(&path, &rolled);
    }
    let keep = Duration::from_secs(super::replay::settings(config_dir).keep_days * 24 * 60 * 60);
    let stale = fs::metadata(&rolled)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| SystemTime::now().duration_since(t).ok())
        .is_some_and(|age| age > keep);
    if stale {
        let _ = fs::remove_file(&rolled);
    }
    let line = serde_json::to_string(event).map_err(|e| e.to_string())?;
    at_rest::append_sealed_line(config_dir, &path, AAD_SAFETY, &line)
        .map_err(|why| format!("{why:?}"))
}

/// Events oldest first (the roll, then the current file). Locked ⇒ empty.
pub fn read(config_dir: &Path) -> Vec<SafetyEvent> {
    let mut out = Vec::new();
    for name in [ROLLED_FILE, EVENTS_FILE] {
        let Ok(text) = fs::read_to_string(config_dir.join(name)) else {
            continue;
        };
        let read = at_rest::read_sealed_jsonl(config_dir, &text, AAD_SAFETY, true);
        out.extend(
            read.lines
                .iter()
                .filter_map(|l| serde_json::from_str::<SafetyEvent>(l).ok()),
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::harness::{use_key_store_for, MemoryKeyStore};

    #[test]
    fn the_last_stage_that_changed_the_decision_gets_the_credit() {
        use Kind::*;
        // A deny rule, untouched by judge and hook.
        assert_eq!(decided_by(Refuse, Refuse, Refuse, false), DecidedBy::Rule);
        // The judge turned an ask into a refusal.
        assert_eq!(decided_by(Ask, Refuse, Refuse, false), DecidedBy::Judge);
        // A hook denied what the judge let run.
        assert_eq!(decided_by(Ask, Run, Refuse, false), DecidedBy::Hook);
        // A hook turned a run into an ask.
        assert_eq!(decided_by(Run, Run, Ask, true), DecidedBy::Hook);
        // The gate asked because the call wants out of the sandbox.
        assert_eq!(decided_by(Ask, Ask, Ask, true), DecidedBy::Sandbox);
        assert_eq!(decided_by(Ask, Ask, Ask, false), DecidedBy::Rule);
        assert!(wants_outside_sandbox(
            r#"{"command":"ls","outside_sandbox":true}"#
        ));
        assert!(!wants_outside_sandbox(r#"{"command":"ls"}"#));
        assert!(!wants_outside_sandbox("not json"));
    }

    #[test]
    fn events_are_sealed_and_read_back_in_order() {
        let dir = crate::harness::test_dir("safety-events");
        use_key_store_for(&dir, Arc::new(MemoryKeyStore::new()));
        let stopped = SafetyEvent {
            ts_ms: 1,
            chat: "chat-1".into(),
            tool: "shell".into(),
            outcome: Outcome::Stopped,
            by: DecidedBy::Judge,
            answer: None,
            request: "ab".repeat(32),
        };
        let asked = SafetyEvent {
            ts_ms: 2,
            outcome: Outcome::Asked,
            by: DecidedBy::Sandbox,
            answer: Some(Answer::Denied),
            request: String::new(),
            ..stopped.clone()
        };
        record(&dir, &stopped).unwrap();
        record(&dir, &asked).unwrap();
        assert_eq!(read(&dir), vec![stopped, asked]);
        let raw = fs::read_to_string(dir.join(EVENTS_FILE)).unwrap();
        assert!(
            raw.lines().all(|l| l.starts_with(at_rest::SEALED_PREFIX)),
            "{raw}"
        );
        assert!(!raw.contains("shell") && !raw.contains("chat-1"), "{raw}");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn no_key_means_no_event_is_written() {
        let dir = crate::harness::test_dir("safety-events-locked");
        use_key_store_for(&dir, Arc::new(MemoryKeyStore::unavailable()));
        let ev = SafetyEvent {
            ts_ms: 1,
            chat: "c".into(),
            tool: "write".into(),
            outcome: Outcome::Stopped,
            by: DecidedBy::Rule,
            answer: None,
            request: String::new(),
        };
        assert!(record(&dir, &ev).is_err());
        assert!(!dir.join(EVENTS_FILE).exists());
        let _ = fs::remove_dir_all(dir);
    }
}
