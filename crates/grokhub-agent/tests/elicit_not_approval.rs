//! Accepted MCP elicitation never counts as tool-call approval.
//!
//! Spec version: v1 (Order 990). Tests only — no production or fake_mcp edits.
//! FAKE_MCP_ELICIT hardcodes message "Need a value"; the threat model is an
//! accept that must not approve a later gated tool (literal "approve this tool
//! call" wording needs a fake_mcp mode and is skipped here).

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};

use grokhub_agent::harness::{ConsentLedger, CONSENT_FILE};
use grokhub_agent::mcp::{self, ElicitNote};
use grokhub_agent::perm::ConfigGuard;
use grokhub_agent::{
    run_loop, CancelToken, ClientError, FunctionCall, Gate, HaltCheck, InputItem, LoopEvent,
    LoopIn, ModelClient, PermAnswer, PermMode, PermitInbox, PermitNote, SteerQueue, StreamEvent,
    TurnOutput, Usage,
};
use serde_json::json;

struct Clean(PathBuf);

impl Drop for Clean {
    fn drop(&mut self) {
        mcp::shutdown_all();
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn scratch(label: &str) -> (PathBuf, Clean, ConfigGuard) {
    let dir = std::env::temp_dir().join(format!(
        "gh-elicit-not-approval-{label}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let clean = Clean(dir.clone());
    let guard = ConfigGuard::set(&dir);
    (dir, clean, guard)
}

fn import_bin(bin: &str) {
    let doc = json!({
        "mcpServers": {
            "box": {
                "command": bin,
                "env": {"FAKE_MCP_ELICIT": "1"},
                "startup_timeout_sec": 8,
                "tool_timeout_sec": 8
            }
        }
    });
    mcp::import_documents(&[doc.to_string()]).unwrap();
}

struct NoHalt;

impl HaltCheck for NoHalt {
    fn halted(&self) -> bool {
        false
    }
}

/// Turn 0: MCP echo (triggers elicitation). Turn 1: hard-class shell.
struct Script {
    turn: AtomicUsize,
}

impl ModelClient for Script {
    fn stream(
        &self,
        _req: &grokhub_agent::ResponsesRequest,
        _cancel: &CancelToken,
        sink: &mut dyn FnMut(StreamEvent),
    ) -> Result<TurnOutput, ClientError> {
        let n = self.turn.fetch_add(1, Ordering::SeqCst);
        if n == 0 {
            return Ok(TurnOutput {
                text: String::new(),
                reasoning: String::new(),
                calls: vec![FunctionCall {
                    call_id: "elicit-1".into(),
                    name: "box__echo".into(),
                    arguments: "{}".into(),
                }],
                usage: Usage::default(),
                ..Default::default()
            });
        }
        if n == 1 {
            return Ok(TurnOutput {
                text: String::new(),
                reasoning: String::new(),
                calls: vec![FunctionCall {
                    call_id: "hard-1".into(),
                    name: "run_terminal_command".into(),
                    arguments: r#"{"command":"rm -rf old"}"#.into(),
                }],
                usage: Usage::default(),
                ..Default::default()
            });
        }
        sink(StreamEvent::TextDelta("done".into()));
        Ok(TurnOutput {
            text: "done".into(),
            reasoning: String::new(),
            calls: Vec::new(),
            usage: Usage::default(),
            ..Default::default()
        })
    }
}

fn ledger_active(dir: &Path) -> usize {
    ConsentLedger::load(dir).active().count()
}

fn assert_ledger_empty(dir: &Path) {
    assert_eq!(ledger_active(dir), 0, "consent ledger must stay empty");
    let path = dir.join(CONSENT_FILE);
    if path.exists() {
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        assert!(
            text.trim().is_empty(),
            "consent.jsonl must be empty, got {text:?}"
        );
    }
}

fn run_flow(dir: &Path, answer: PermAnswer) -> (Vec<LoopEvent>, Vec<InputItem>) {
    let bin = env!("CARGO_BIN_EXE_fake_mcp");
    import_bin(bin);

    let (elicit_tx, elicit_inbox) = mcp::ElicitInbox::pair();
    mcp::attach_elicit("s", elicit_inbox);
    let (permit_tx, permits) = PermitInbox::pair();

    let client = Script {
        turn: AtomicUsize::new(0),
    };
    let cancel = CancelToken::new();
    let steer = SteerQueue::new();
    let halt = NoHalt;
    let events: Arc<Mutex<Vec<LoopEvent>>> = Arc::new(Mutex::new(Vec::new()));
    let events_cb = Arc::clone(&events);
    let permission_answers = AtomicUsize::new(0);

    let input = LoopIn {
        client: &client,
        workspace: dir,
        model: "grok-4.7",
        effort: None,
        system: "",
        conversation_id: "s",
        max_turns: 4,
        usage_base: Usage::default(),
        cancel: &cancel,
        steer: &steer,
        halt: &halt,
        gate: Gate {
            mode: PermMode::Always,
            readonly_session: false,
            attended: true,
            desktop: false,
        },
        desktop: None,
        permits: &permits,
        perms: None,
        context_length: 0,
        tasks: None,
        depth: 0,
        agent_id: None,
        shared_client: None,
        shared_permits: None,
        shared_desktop: None,
    };

    let mut history = Vec::new();
    let _ = run_loop(&input, &mut history, "go", None, &mut |ev| {
        match &ev {
            LoopEvent::Elicit(view) => {
                assert_eq!(view.message, "Need a value");
                assert_eq!(view.server_name, "box");
                let _ = elicit_tx.send(ElicitNote {
                    id: view.id.clone(),
                    action: "accept".into(),
                    content: Some(json!({"note": "hi"})),
                });
            }
            LoopEvent::Permission { id, name, hard, .. } => {
                let prior = events_cb.lock().unwrap();
                let tool_started = prior.iter().any(|e| {
                    matches!(
                        e,
                        LoopEvent::Tool {
                            id: tid,
                            status,
                            ..
                        } if tid == id && (*status == "in_progress" || *status == "completed")
                    )
                });
                drop(prior);
                assert!(
                    !tool_started,
                    "gated tool must not run before the approval answer"
                );
                assert_eq!(*name, "run_terminal_command");
                assert!(*hard, "hard-class card expected");
                let n = permission_answers.fetch_add(1, Ordering::SeqCst) + 1;
                assert_eq!(n, 1, "exactly one approval request for the gated tool");
                answer_permit(&permit_tx, id, answer);
            }
            _ => {}
        }
        events_cb.lock().unwrap().push(ev);
    });

    mcp::detach_elicit("s");
    mcp::shutdown_all();

    assert_eq!(
        permission_answers.load(Ordering::SeqCst),
        1,
        "expected exactly one approval request"
    );
    let events = events.lock().unwrap().clone();
    (events, history)
}

fn answer_permit(tx: &Sender<PermitNote>, id: &str, answer: PermAnswer) {
    let _ = tx.send(PermitNote {
        id: id.to_string(),
        answer,
    });
}

#[test]
fn accepted_elicit_does_not_preapprove_next_gated_tool() {
    let (dir, _clean, _guard) = scratch("allow");
    let (events, history) = run_flow(&dir, PermAnswer::Allow);

    let elicits: Vec<_> = events
        .iter()
        .filter(|e| matches!(e, LoopEvent::Elicit(_)))
        .collect();
    assert_eq!(elicits.len(), 1, "{events:?}");

    let permissions: Vec<_> = events
        .iter()
        .filter(|e| matches!(e, LoopEvent::Permission { .. }))
        .collect();
    assert_eq!(permissions.len(), 1, "{events:?}");

    let echo_ok = history.iter().any(|item| matches!(
        item,
        InputItem::FunctionCallOutput { call_id, output }
            if call_id == "elicit-1" && output.contains("action=accept") && output.contains("hi")
    ));
    assert!(
        echo_ok,
        "accepted elicit must complete the MCP call: {history:?}"
    );

    let hard_rejected = history.iter().any(|item| matches!(
        item,
        InputItem::FunctionCallOutput { call_id, output }
            if call_id == "hard-1" && output.contains("User rejected the execution for tool `run_terminal_command`")
    ));
    assert!(
        !hard_rejected,
        "Allow must not reject the gated tool: {history:?}"
    );
    let hard_ran = history.iter().any(|item| {
        matches!(
            item,
            InputItem::FunctionCallOutput { call_id, .. } if call_id == "hard-1"
        )
    });
    assert!(hard_ran, "gated tool must run after Allow: {history:?}");

    assert_ledger_empty(&dir);
}

#[test]
fn accepted_elicit_then_deny_keeps_tool_from_running() {
    let (dir, _clean, _guard) = scratch("deny");
    let (events, history) = run_flow(&dir, PermAnswer::Deny);

    let elicits: Vec<_> = events
        .iter()
        .filter(|e| matches!(e, LoopEvent::Elicit(_)))
        .collect();
    assert_eq!(elicits.len(), 1, "{events:?}");

    let permissions: Vec<_> = events
        .iter()
        .filter(|e| matches!(e, LoopEvent::Permission { .. }))
        .collect();
    assert_eq!(permissions.len(), 1, "{events:?}");

    let echo_ok = history.iter().any(|item| {
        matches!(
            item,
            InputItem::FunctionCallOutput { call_id, output }
                if call_id == "elicit-1" && output.contains("action=accept")
        )
    });
    assert!(echo_ok, "elicit accept must still succeed: {history:?}");

    let hard_rejected = history.iter().any(|item| {
        matches!(
            item,
            InputItem::FunctionCallOutput { call_id, output }
                if call_id == "hard-1"
                    && output == "User rejected the execution for tool `run_terminal_command`"
        )
    });
    assert!(
        hard_rejected,
        "Deny must reject without running the tool: {history:?}"
    );

    let hard_progress = events.iter().any(|e| {
        matches!(
            e,
            LoopEvent::Tool {
                id,
                status,
                ..
            } if id == "hard-1" && *status == "in_progress"
        )
    });
    assert!(
        !hard_progress,
        "denied hard tool must never start: {events:?}"
    );

    assert_ledger_empty(&dir);
}
