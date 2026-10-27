//! Dry-run close-check fixtures.
//!
//! A single reply with nothing open makes one model call. An open intention
//! or a plan-mode todo spends one extra low-effort call, then at most one
//! continuation. Depth above 0 never checks.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Mutex;

use grokhub_core::{load_intentions, save_intentions, Intention, IntentionStatus, Intentions};

use crate::client::{
    ClientError, ContentPart, InputItem, ModelClient, ResponsesRequest, StreamEvent, TurnOutput,
};
use crate::gate::{ClosedPermits, Gate};
use crate::perm::ConfigGuard;
use crate::run::{HaltCheck, LoopEvent, LoopIn, StopReason};
use crate::{run_loop, CancelToken, SteerQueue, Usage};

const EXPECTED_CALLS: u32 = 10;

static SEQ: AtomicU64 = AtomicU64::new(0);

struct Pin {
    dir: PathBuf,
    _guard: ConfigGuard,
    id: String,
}

impl Pin {
    fn new(label: &str) -> Result<Self, &'static str> {
        let n = SEQ.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("gh-close-{label}-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&dir).map_err(|_| "failed: scratch")?;
        let guard = ConfigGuard::set(&dir);
        Ok(Self {
            dir,
            _guard: guard,
            id: format!("close-{label}-{n}"),
        })
    }
}

impl Drop for Pin {
    fn drop(&mut self) {
        crate::session_tools::set_plan_session(&self.id, false);
        #[cfg(test)]
        crate::session_tools::invalidate_todos(&self.id);
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

struct Script {
    n: AtomicUsize,
    seen: Mutex<Vec<ResponsesRequest>>,
    replies: Mutex<VecDeque<String>>,
    check: String,
}

impl Script {
    fn new(replies: &[&str], check: &str) -> Self {
        Self {
            n: AtomicUsize::new(0),
            seen: Mutex::new(Vec::new()),
            replies: Mutex::new(replies.iter().map(|line| (*line).to_string()).collect()),
            check: check.to_string(),
        }
    }

    fn calls(&self) -> usize {
        self.n.load(Ordering::SeqCst)
    }

    fn checks(&self) -> Vec<ResponsesRequest> {
        self.seen
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .iter()
            .filter(|req| request_text(req).contains(crate::close_check::CLOSE_SYSTEM))
            .cloned()
            .collect()
    }
}

impl ModelClient for Script {
    fn stream(
        &self,
        req: &ResponsesRequest,
        _cancel: &CancelToken,
        sink: &mut dyn FnMut(StreamEvent),
    ) -> Result<TurnOutput, ClientError> {
        self.n.fetch_add(1, Ordering::SeqCst);
        self.seen
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .push(req.clone());
        let text = if request_text(req).contains(crate::close_check::CLOSE_SYSTEM) {
            self.check.clone()
        } else {
            self.replies
                .lock()
                .unwrap_or_else(|err| err.into_inner())
                .pop_front()
                .unwrap_or_default()
        };
        if !text.is_empty() {
            sink(StreamEvent::TextDelta(text.clone()));
        }
        Ok(TurnOutput {
            text,
            ..TurnOutput::default()
        })
    }
}

struct NoHalt;

impl HaltCheck for NoHalt {
    fn halted(&self) -> bool {
        false
    }
}

struct Passed {
    calls: usize,
    checks: Vec<ResponsesRequest>,
    history: Vec<InputItem>,
    findings: Vec<String>,
}

fn request_text(req: &ResponsesRequest) -> String {
    let mut out = String::new();
    for item in &req.input {
        let InputItem::Message { content, .. } = item else {
            continue;
        };
        for part in content {
            if let ContentPart::InputText(text) = part {
                out.push_str(text);
                out.push('\n');
            }
        }
    }
    out
}

fn history_text(history: &[InputItem]) -> String {
    let mut out = String::new();
    for item in history {
        let InputItem::Message { content, .. } = item else {
            continue;
        };
        for part in content {
            if let ContentPart::InputText(text) = part {
                out.push_str(text);
                out.push('\n');
            }
        }
    }
    out
}

fn save_open(pin: &Pin, text: &str) -> Result<(), &'static str> {
    let list = Intentions {
        items: vec![Intention {
            id: "ship".into(),
            text: text.into(),
            status: IntentionStatus::Open,
        }],
    };
    save_intentions(&pin.dir, &pin.id, &list).map_err(|_| "failed: intentions did not save")
}

fn seed_pending_todo(pin: &Pin) -> Result<(), &'static str> {
    let arguments = serde_json::json!({
        "merge": false,
        "todos": [{
            "id": "notes",
            "content": "write the notes",
            "status": "pending"
        }]
    })
    .to_string();
    crate::session::record_turn(
        &pin.id,
        ".",
        "grok-4.7",
        &[InputItem::FunctionCall {
            call_id: "seed".into(),
            name: "todo_write".into(),
            arguments,
        }],
        &Usage::default(),
        "",
        "",
    )
    .map_err(|_| "failed: todos did not load")?;
    let todos = crate::session_tools::todos_for(&pin.id);
    if todos.len() == 1 && todos[0].status == "pending" {
        Ok(())
    } else {
        Err("failed: todos did not load")
    }
}

fn drive(pin: &Pin, script: &Script, depth: u32, user: &str) -> Result<Passed, &'static str> {
    let (cancel, steer) = (CancelToken::new(), SteerQueue::new());
    let permits = ClosedPermits;
    let halt = NoHalt;
    let input = LoopIn {
        client: script,
        workspace: &pin.dir,
        model: "grok-4.7",
        effort: Some("low"),
        system: "",
        conversation_id: &pin.id,
        max_turns: 8,
        usage_base: Usage::default(),
        cancel: &cancel,
        steer: &steer,
        halt: &halt,
        gate: Gate::phase_readonly(),
        desktop: None,
        permits: &permits,
        perms: None,
        context_length: 0,
        tasks: None,
        depth,
        agent_id: None,
        shared_client: None,
        shared_permits: None,
        shared_desktop: None,
    };
    let mut history = Vec::new();
    let mut findings = Vec::new();
    let out = run_loop(&input, &mut history, user, None, &mut |ev| {
        if let LoopEvent::Findings(body) = ev {
            findings.push(body);
        }
    });
    if out.stop != StopReason::EndTurn {
        return Err("failed: turn did not end");
    }
    Ok(Passed {
        calls: script.calls(),
        checks: script.checks(),
        history,
        findings,
    })
}

fn assert_low_check(checks: &[ResponsesRequest]) -> Result<(), &'static str> {
    let Some(req) = checks.first() else {
        return Err("failed: close-check did not run");
    };
    if checks.len() != 1 || !req.tools.is_empty() || req.hosted_search {
        return Err("failed: close-check did not run");
    }
    if req.effort.as_deref() != Some("low") {
        return Err("failed: close-check effort");
    }
    Ok(())
}

fn plain_reply() -> Result<u32, &'static str> {
    let pin = Pin::new("plain")?;
    let script = Script::new(
        &["Hello."],
        r#"{"need_met":false,"open":["x"],"next_steps":["nope"],"status_updates":[]}"#,
    );
    let passed = drive(&pin, &script, 0, "hi")?;
    if passed.calls != 1 || !passed.checks.is_empty() {
        return Err("failed: plain chat called the checker");
    }
    if history_text(&passed.history).contains("<intentions>") || !passed.findings.is_empty() {
        return Err("failed: plain chat called the checker");
    }
    Ok(passed.calls as u32)
}

fn finished_intention() -> Result<u32, &'static str> {
    let pin = Pin::new("done")?;
    save_open(&pin, "ship the notes")?;
    let script = Script::new(
        &["All done."],
        r#"{"need_met":true,"open":[],"next_steps":[],"status_updates":[{"id":"ship","status":"done","text":"stolen"}]}"#,
    );
    let passed = drive(&pin, &script, 0, "ship it")?;
    if passed.calls != 2 {
        return Err("failed: finished intention call count");
    }
    assert_low_check(&passed.checks)?;
    let list = load_intentions(&pin.dir, &pin.id);
    let item = list
        .items
        .first()
        .ok_or("failed: finished intention lost")?;
    if item.id != "ship" || item.text != "ship the notes" || item.status != IntentionStatus::Done {
        return Err("failed: finished intention status");
    }
    let card = passed.findings.join("\n");
    if !card.contains("Finished") || !card.contains("ship the notes") {
        return Err("failed: finished intention notice");
    }
    Ok(passed.calls as u32)
}

fn open_intention_continues_once() -> Result<u32, &'static str> {
    let pin = Pin::new("open")?;
    save_open(&pin, "ship the notes")?;
    let script = Script::new(
        &["All done.", "Working on the tests."],
        r#"{"need_met":false,"open":["ship"],"next_steps":["run the tests"],"status_updates":[{"id":"ship","status":"waiting","text":"stolen"}]}"#,
    );
    let passed = drive(&pin, &script, 0, "ship it")?;
    if passed.calls != 3 {
        return Err("failed: open intention call count");
    }
    assert_low_check(&passed.checks)?;
    let list = load_intentions(&pin.dir, &pin.id);
    let item = list.items.first().ok_or("failed: open intention lost")?;
    if item.id != "ship" || item.text != "ship the notes" || item.status != IntentionStatus::Waiting
    {
        return Err("failed: open intention status");
    }
    let history = history_text(&passed.history);
    if !history.contains("<intentions>")
        || !history.contains("ship the notes")
        || !history.contains("Not done yet.")
        || !history.contains("run the tests")
    {
        return Err("failed: open intention did not continue");
    }
    let card = passed.findings.join("\n");
    if !card.contains("run the tests") {
        return Err("failed: open intention steps");
    }
    Ok(passed.calls as u32)
}

fn depth_skips() -> Result<u32, &'static str> {
    let pin = Pin::new("depth")?;
    save_open(&pin, "ship the notes")?;
    let script = Script::new(
        &["Hello."],
        r#"{"need_met":false,"open":["ship"],"next_steps":["run the tests"],"status_updates":[]}"#,
    );
    let passed = drive(&pin, &script, 1, "ship it")?;
    if passed.calls != 1 || !passed.checks.is_empty() {
        return Err("failed: subagent ran the checker");
    }
    if history_text(&passed.history).contains("<intentions>") {
        return Err("failed: subagent saw intentions");
    }
    let list = load_intentions(&pin.dir, &pin.id);
    if list.items.first().map(|item| item.status) != Some(IntentionStatus::Open) {
        return Err("failed: subagent changed an intention");
    }
    Ok(passed.calls as u32)
}

fn plan_todo_checks_once() -> Result<u32, &'static str> {
    let pin = Pin::new("plan")?;
    seed_pending_todo(&pin)?;
    crate::session_tools::set_plan_session(&pin.id, true);
    let script = Script::new(
        &["All done.", "Still going."],
        r#"{"need_met":true,"open":[],"next_steps":["finish the checklist"],"status_updates":[{"id":"notes","text":"stolen","status":"done"}]}"#,
    );
    let passed = drive(&pin, &script, 0, "finish the list")?;
    if passed.calls != 3 {
        return Err("failed: plan todo call count");
    }
    assert_low_check(&passed.checks)?;
    let todos = crate::session_tools::todos_for(&pin.id);
    if todos.len() != 1 || todos[0].id != "notes" || todos[0].status != "pending" {
        return Err("failed: plan todo status");
    }
    let history = history_text(&passed.history);
    let card = passed.findings.join("\n");
    if !history.contains("finish the checklist") || !card.contains("finish the checklist") {
        return Err("failed: plan todo steps");
    }
    Ok(passed.calls as u32)
}

/// Suite row. Status stays a fixed string so two dry-runs match.
pub fn run_fixtures() -> (&'static str, u32) {
    let _class = crate::route::live::ClassScope::enter(super::EVAL_CLASS);
    let mut turns = 0u32;
    for case in [
        plain_reply,
        finished_intention,
        open_intention_continues_once,
        depth_skips,
        plan_todo_checks_once,
    ] {
        match case() {
            Ok(calls) => turns = turns.saturating_add(calls),
            Err(err) => return (err, turns),
        }
    }
    if turns != EXPECTED_CALLS {
        return ("failed: close-check call count", turns);
    }
    ("checked", turns)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_single_reply_skips_and_open_work_checks_once() {
        let (status, turns) = run_fixtures();
        assert_eq!(status, "checked");
        assert_eq!(turns, EXPECTED_CALLS);
    }
}
