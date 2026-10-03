//! Agent loop. Turns stop when the model stops calling tools, or at the cap.

use std::collections::{HashMap, VecDeque};
use std::path::Path;
use std::sync::{Arc, Mutex};

use crate::gate::{self, Decision, Gate, PermAnswer, PermitWait, Waited};
use crate::tasks::TaskHub;
use crate::tools::{self, DesktopOps, ToolCtx, ToolOutput};
use crate::{
    CancelToken, ClientError, ContentPart, FunctionCall, InputItem, ModelClient, ResponsesRequest,
    StreamEvent, Usage,
};

pub const DEFAULT_MAX_TURNS: u32 = 50;
const REPEAT_LIMIT: u32 = 3;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StopReason {
    EndTurn,
    Cancelled,
    Halted,
    MaxTurns,
    RepeatedCall,
    Error(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoopEvent {
    Text(String),
    Thought(String),
    Tool {
        id: String,
        name: String,
        status: String,
        detail: String,
        image: Option<String>,
    },
    Usage(Usage),
    /// Context meter for the usage event that follows.
    Meter {
        used: u64,
        limit: u64,
    },
    Compact {
        started: bool,
        usage: Usage,
        error: Option<String>,
    },
    Permission {
        id: String,
        name: String,
        action: String,
        reason: String,
    },
    Elicit(crate::mcp::ElicitView),
}

#[derive(Clone)]
pub struct SteerQueue {
    inner: Arc<Mutex<VecDeque<String>>>,
}

impl SteerQueue {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(VecDeque::new())),
        }
    }

    pub fn push(&self, text: impl Into<String>) {
        let text = text.into();
        if text.trim().is_empty() {
            return;
        }
        self.inner.lock().unwrap_or_else(|e| e.into_inner()).push_back(text);
    }

    pub fn drain(&self) -> Vec<String> {
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .drain(..)
            .collect()
    }
}

impl Default for SteerQueue {
    fn default() -> Self {
        Self::new()
    }
}

pub trait HaltCheck {
    fn halted(&self) -> bool;
}

pub struct LoopIn<'a> {
    pub client: &'a dyn ModelClient,
    pub workspace: &'a Path,
    pub model: &'a str,
    pub effort: Option<&'a str>,
    pub system: &'a str,
    pub conversation_id: &'a str,
    pub max_turns: u32,
    /// Already counted this session. Events and the returned total include it.
    pub usage_base: Usage,
    pub cancel: &'a CancelToken,
    pub steer: &'a SteerQueue,
    pub halt: &'a dyn HaltCheck,
    pub gate: Gate,
    pub desktop: Option<&'a dyn DesktopOps>,
    pub permits: &'a dyn PermitWait,
    /// `None` keeps gate v0. The native engine passes the loaded policy.
    pub perms: Option<&'a crate::perm::Policy>,
    /// Model context window. `0` disables auto-compact.
    pub context_length: u64,
    /// Background commands and monitors for this session. `None` in tests that do not spawn.
    pub tasks: Option<Arc<TaskHub>>,
}

pub struct LoopOut {
    pub stop: StopReason,
    pub usage: Usage,
    /// A summary replaced older history. The caller records a compaction marker.
    pub compacted: bool,
}

pub fn run_loop(
    input: &LoopIn<'_>,
    history: &mut Vec<InputItem>,
    user_text: &str,
    image: Option<&str>,
    on_event: &mut dyn FnMut(LoopEvent),
) -> LoopOut {
    if history.is_empty() && !input.system.trim().is_empty() {
        history.push(InputItem::Message {
            role: "system".into(),
            content: vec![ContentPart::InputText(input.system.to_string())],
        });
    }
    history.push(user_message(user_text, image));
    input.permits.drain();
    let mut usage = input.usage_base.clone();
    let mut did_compact = false;
    let mut repeats: HashMap<String, u32> = HashMap::new();
    let max_turns = if input.max_turns == 0 {
        DEFAULT_MAX_TURNS
    } else {
        input.max_turns
    };
    for _turn in 0..max_turns {
        if input.cancel.is_cancelled() {
            return LoopOut {
                stop: StopReason::Cancelled,
                usage,
                compacted: did_compact,
            };
        }
        if stop_for_halt(input) {
            return LoopOut {
                stop: StopReason::Halted,
                usage,
                compacted: did_compact,
            };
        }
        inject_notices(input, history);
        if !did_compact && crate::compact::needs_auto_compact(history, input.context_length) {
            on_event(LoopEvent::Compact {
                started: true,
                usage: usage.clone(),
                error: None,
            });
            match crate::compact::compact_transcript(
                input.client,
                input.cancel,
                input.model,
                input.effort,
                input.conversation_id,
                history,
            ) {
                Ok(extra) => {
                    usage.add(&extra);
                    did_compact = true;
                    on_event(LoopEvent::Compact {
                        started: false,
                        usage: usage.clone(),
                        error: None,
                    });
                    emit_usage(on_event, &usage, history, input.context_length);
                }
                Err(crate::compact::CompactError::Cancelled) => {
                    on_event(LoopEvent::Compact {
                        started: false,
                        usage: usage.clone(),
                        error: Some("cancelled".into()),
                    });
                    return LoopOut {
                        stop: StopReason::Cancelled,
                        usage,
                        compacted: false,
                    };
                }
                Err(crate::compact::CompactError::Failed(message)) => {
                    on_event(LoopEvent::Compact {
                        started: false,
                        usage: usage.clone(),
                        error: Some(message),
                    });
                }
            }
        }
        let mut wire = history.clone();
        crate::image_budget::apply_image_budget(&mut wire);
        let req = ResponsesRequest {
            model: input.model.to_string(),
            effort: input.effort.map(str::to_string),
            input: wire,
            conversation_id: input.conversation_id.to_string(),
            tools: tools::schemas_for(&input.gate),
            hosted_search: true,
            call_timeout: None,
        };
        let turn = match input.client.stream(&req, input.cancel, &mut |ev| match ev {
            StreamEvent::TextDelta(text) => on_event(LoopEvent::Text(text)),
            StreamEvent::ReasoningDelta(text) => on_event(LoopEvent::Thought(text)),
        }) {
            Ok(turn) => turn,
            Err(ClientError::Cancelled) => {
                return LoopOut {
                    stop: StopReason::Cancelled,
                    usage,
                    compacted: did_compact,
                }
            }
            Err(err) => {
                return LoopOut {
                    stop: StopReason::Error(err.to_string()),
                    usage,
                    compacted: did_compact,
                }
            }
        };
        usage.add(&turn.usage);
        emit_usage(on_event, &usage, history, input.context_length);
        if !turn.text.is_empty() {
            history.push(InputItem::Message {
                role: "assistant".into(),
                content: vec![ContentPart::InputText(turn.text)],
            });
        }
        if turn.calls.is_empty() {
            let notes = input.steer.drain();
            if notes.is_empty() {
                return LoopOut {
                    stop: StopReason::EndTurn,
                    usage,
                    compacted: did_compact,
                };
            }
            for note in notes {
                history.push(user_message(&note, None));
            }
            continue;
        }
        let mut repeated = false;
        let mut always = input.gate.mode == gate::PermMode::Always;
        for call in &turn.calls {
            history.push(InputItem::FunctionCall {
                call_id: call.call_id.clone(),
                name: call.name.clone(),
                arguments: call.arguments.clone(),
            });
        }
        for call in &turn.calls {
            if input.cancel.is_cancelled() {
                return LoopOut {
                    stop: StopReason::Cancelled,
                    usage,
                    compacted: did_compact,
                };
            }
            if stop_for_halt(input) {
                return LoopOut {
                    stop: StopReason::Halted,
                    usage,
                    compacted: did_compact,
                };
            }
            let key = format!("{}\\n{}", call.name, call.arguments);
            let seen = repeats.entry(key).or_insert(0);
            *seen = seen.saturating_add(1);
            if *seen >= REPEAT_LIMIT {
                repeated = true;
                let output = ToolOutput::err(format!(
                    "{}; the same call already ran twice, so it was not run again",
                    tools::READ_ONLY_PHASE
                ));
                push_output(history, call, output);
                continue;
            }
            let id = tool_id(call);
            let desk = tools::desk_flags(&call.name, &input.gate, input.desktop);
            let base = gate::decide_with(
                &input.gate,
                &call.name,
                &call.arguments,
                always,
                desk,
                input.workspace,
                input.perms,
            );
            let reviewed = crate::auto_review::review(&crate::auto_review::ReviewIn {
                client: input.client,
                cancel: input.cancel,
                halt: &|| stop_for_halt(input),
                gate: &input.gate,
                name: &call.name,
                arguments: &call.arguments,
                workspace: input.workspace,
                policy: input.perms,
                latched_always: always,
                desk,
                history,
                conversation_id: input.conversation_id,
                base,
                timeout: crate::auto_review::JUDGE_TIMEOUT,
            });
            if let Some(extra) = &reviewed.judge_usage {
                usage.add(extra);
                emit_usage(on_event, &usage, history, input.context_length);
            }
            if reviewed.cancelled {
                let output = ToolOutput::err(gate::user_cancelled(&call.name));
                emit_tool(on_event, &id, call, "failed", &output.text, None);
                push_output(history, call, output);
                return LoopOut {
                    stop: StopReason::Cancelled,
                    usage,
                    compacted: did_compact,
                };
            }
            if reviewed.halted {
                let _ = stop_for_halt(input);
                return LoopOut {
                    stop: StopReason::Halted,
                    usage,
                    compacted: did_compact,
                };
            }
            let ask_reason = reviewed.ask_reason;
            let output = match reviewed.decision {
                Decision::Refuse(text) => {
                    let output = ToolOutput::err(text);
                    emit_tool(on_event, &id, call, "in_progress", &call.arguments, None);
                    emit_tool(on_event, &id, call, if output.failed { "failed" } else { "completed" }, &output.text, output.image_data_url.clone());
                    output
                }
                Decision::Ask => {
                    on_event(LoopEvent::Permission {
                        id: id.clone(),
                        name: call.name.clone(),
                        action: action_line(&call.name, &call.arguments),
                        reason: ask_reason,
                    });
                    match input
                        .permits
                        .wait(&id, input.cancel, &|| stop_for_halt(input))
                    {
                        Waited::Answer(PermAnswer::Allow) => {
                            run_allowed(input, call, &id, on_event)
                        }
                        Waited::Answer(PermAnswer::Always) => {
                            if input.perms.is_some() {
                                let _ = crate::perm::remember_allow_always(
                                    input.workspace,
                                    &call.name,
                                    &call.arguments,
                                );
                            }
                            always = true;
                            run_allowed(input, call, &id, on_event)
                        }
                        Waited::Answer(PermAnswer::Deny) => {
                            let output = ToolOutput::err(gate::user_rejected(&call.name));
                            emit_tool(on_event, &id, call, "failed", &output.text, None);
                            output
                        }
                        Waited::Answer(PermAnswer::Cancel) | Waited::Cancelled => {
                            let output = ToolOutput::err(gate::user_cancelled(&call.name));
                            emit_tool(on_event, &id, call, "failed", &output.text, None);
                            push_output(history, call, output);
                            return LoopOut {
                                stop: StopReason::Cancelled,
                                usage,
                                compacted: did_compact,
                            };
                        }
                        Waited::Halted => {
                            let _ = stop_for_halt(input);
                            return LoopOut {
                                stop: StopReason::Halted,
                                usage,
                                compacted: did_compact,
                            };
                        }
                    }
                }
                Decision::Run => run_allowed(input, call, &id, on_event),
            };
            let cancelled = input.cancel.is_cancelled();
            let halted = stop_for_halt(input);
            push_output(history, call, output);
            if cancelled {
                return LoopOut {
                    stop: StopReason::Cancelled,
                    usage,
                    compacted: did_compact,
                };
            }
            if halted {
                return LoopOut {
                    stop: StopReason::Halted,
                    usage,
                    compacted: did_compact,
                };
            }
        }
        if repeated {
            return LoopOut {
                stop: StopReason::RepeatedCall,
                usage,
                compacted: did_compact,
            };
        }
        for note in input.steer.drain() {
            history.push(user_message(&note, None));
        }
    }
    LoopOut {
        stop: StopReason::MaxTurns,
        usage,
        compacted: did_compact,
    }
}

fn emit_usage(
    on_event: &mut dyn FnMut(LoopEvent),
    usage: &Usage,
    history: &[InputItem],
    limit: u64,
) {
    on_event(LoopEvent::Meter {
        used: crate::compact::estimate_input_tokens(history),
        limit,
    });
    on_event(LoopEvent::Usage(usage.clone()));
}

fn user_message(text: &str, image: Option<&str>) -> InputItem {
    let mut content = vec![ContentPart::InputText(text.to_string())];
    if let Some(url) = image.map(str::trim).filter(|s| !s.is_empty()) {
        content.push(ContentPart::InputImage(url.to_string()));
    }
    InputItem::Message { role: "user".into(), content }
}

fn push_output(history: &mut Vec<InputItem>, call: &FunctionCall, output: ToolOutput) {
    history.push(InputItem::FunctionCallOutput {
        call_id: call.call_id.clone(),
        output: output.text,
    });
    if let Some(url) = output.image_data_url {
        history.push(InputItem::Message {
            role: "user".into(),
            content: vec![
                ContentPart::InputText(format!("image from {}", call.name)),
                ContentPart::InputImage(url),
            ],
        });
    }
}

fn emit_tool(
    on_event: &mut dyn FnMut(LoopEvent),
    id: &str,
    call: &FunctionCall,
    status: &str,
    detail: &str,
    image: Option<String>,
) {
    on_event(LoopEvent::Tool {
        id: id.to_string(),
        name: call.name.clone(),
        status: status.into(),
        detail: clip(detail, 180),
        image,
    });
}

fn stop_for_halt(input: &LoopIn<'_>) -> bool {
    let stop = input.halt.halted() || input.tasks.as_ref().is_some_and(|hub| hub.is_halted());
    if stop {
        if let Some(tasks) = &input.tasks {
            tasks.halt_all();
        }
    }
    stop
}

fn inject_notices(input: &LoopIn<'_>, history: &mut Vec<InputItem>) {
    let Some(tasks) = &input.tasks else {
        return;
    };
    for note in tasks.drain_notices() {
        let text = format!("<background-notice>\n{note}\n</background-notice>");
        history.push(user_message(&text, None));
    }
}

fn run_allowed(
    input: &LoopIn<'_>,
    call: &FunctionCall,
    id: &str,
    on_event: &mut dyn FnMut(LoopEvent),
) -> ToolOutput {
    emit_tool(on_event, id, call, "in_progress", &call.arguments, None);
    let attended = input.gate.attended;
    let session = input.conversation_id.to_string();
    let cancel = input.cancel.clone();
    let tasks = input.tasks.clone();
    let halt = input.halt;
    let ctx = ToolCtx {
        workspace: input.workspace,
        desktop: input.desktop,
        stop: &|| input.cancel.is_cancelled() || stop_for_halt(input),
        tasks: input.tasks.clone(),
    };
    let mut emit_card = |view: crate::mcp::ElicitView| {
        on_event(LoopEvent::Elicit(view));
    };
    let mut wait_card = |eid: &str| {
        crate::mcp::wait_elicit(&session, eid, &cancel, &|| {
            halt.halted() || tasks.as_ref().is_some_and(|hub| hub.is_halted())
        })
    };
    let output = crate::mcp::with_elicit(attended, &mut emit_card, &mut wait_card, || {
        tools::dispatch(&ctx, &call.name, &call.arguments)
    });
    let status = if output.failed { "failed" } else { "completed" };
    emit_tool(on_event, id, call, status, &output.text, output.image_data_url.clone());
    output
}

fn action_line(name: &str, arguments: &str) -> String {
    let _ = name;
    let value: serde_json::Value = serde_json::from_str(arguments).unwrap_or(serde_json::Value::Null);
    let picked = ["path", "file_path", "target_file", "command", "text", "keys"].iter().find_map(|key| {
        value.get(*key).and_then(|item| item.as_str()).map(str::trim).filter(|text| !text.is_empty())
    });
    clip(picked.unwrap_or(arguments), 180)
}

fn tool_id(call: &FunctionCall) -> String {
    if call.call_id.is_empty() {
        format!("tool-{}", call.name)
    } else {
        call.call_id.clone()
    }
}

fn clip(text: &str, max: usize) -> String {
    let mut out: String = text.chars().take(max).collect();
    if text.chars().count() > max {
        out.push('…');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gate::{self, Gate, PermAnswer, PermitWait, Waited};
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct Script {
        turns: Mutex<Vec<ScriptTurn>>,
        seen: Mutex<Vec<Vec<InputItem>>>,
        cancel_on_text: bool,
        steer: Option<SteerQueue>,
    }

    #[derive(Clone)]
    struct ScriptTurn {
        text: String,
        calls: Vec<FunctionCall>,
        usage: Usage,
    }

    impl ModelClient for Script {
        fn stream(
            &self,
            req: &ResponsesRequest,
            cancel: &CancelToken,
            sink: &mut dyn FnMut(StreamEvent),
        ) -> Result<crate::TurnOutput, ClientError> {
            let n = self.seen.lock().unwrap().len();
            self.seen.lock().unwrap().push(req.input.clone());
            if n == 0 {
                if let Some(steer) = &self.steer {
                    steer.push("please look again");
                }
            }
            if self.cancel_on_text {
                sink(StreamEvent::TextDelta("partial".into()));
                cancel.cancel();
                return Err(ClientError::Cancelled);
            }
            let turn = {
                let mut turns = self.turns.lock().unwrap();
                if turns.is_empty() {
                    ScriptTurn {
                        text: String::new(),
                        calls: Vec::new(),
                        usage: Usage::default(),
                    }
                } else {
                    turns.remove(0)
                }
            };
            if !turn.text.is_empty() {
                sink(StreamEvent::TextDelta(turn.text.clone()));
            }
            Ok(crate::TurnOutput {
                text: turn.text,
                reasoning: String::new(),
                calls: turn.calls,
                usage: turn.usage,
            })
        }
    }

    struct NeverHalt;
    impl HaltCheck for NeverHalt {
        fn halted(&self) -> bool {
            false
        }
    }

    struct HaltAfter(AtomicUsize);
    impl HaltCheck for HaltAfter {
        fn halted(&self) -> bool {
            self.0.fetch_add(1, Ordering::SeqCst) >= 1
        }
    }

    struct AlwaysTool {
        calls: AtomicUsize,
    }

    impl ModelClient for AlwaysTool {
        fn stream(
            &self,
            _req: &ResponsesRequest,
            _cancel: &CancelToken,
            _sink: &mut dyn FnMut(StreamEvent),
        ) -> Result<crate::TurnOutput, ClientError> {
            let n = self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(crate::TurnOutput {
                text: String::new(),
                reasoning: String::new(),
                calls: vec![FunctionCall {
                    call_id: format!("c{n}"),
                    name: "list_dir".into(),
                    arguments: format!(r#"{{"path":"p{n}"}}"#),
                }],
                usage: Usage::default(),
            })
        }
    }

    fn call(id: &str, name: &str, arguments: &str) -> FunctionCall {
        FunctionCall {
            call_id: id.into(),
            name: name.into(),
            arguments: arguments.into(),
        }
    }

    fn usage(input: u64, output: u64, reasoning: u64, cost: i64) -> Usage {
        Usage {
            input_tokens: input,
            output_tokens: output,
            reasoning_tokens: reasoning,
            cost_in_usd_ticks: cost,
        }
    }

    fn workspace(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("gh-loop-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("note.txt"), "hello native\n").unwrap();
        dir
    }

    fn run(
        script: &Script,
        dir: &std::path::Path,
        max_turns: u32,
        cancel: &CancelToken,
        steer: &SteerQueue,
        halt: &dyn HaltCheck,
    ) -> (LoopOut, Vec<InputItem>, Vec<LoopEvent>) {
        let input = LoopIn {
            client: script,
            workspace: dir,
            model: "grok-4.7",
            effort: Some("low"),
            system: "sys",
            conversation_id: "conv",
            max_turns,
            usage_base: Usage::default(),
            cancel,
            steer,
            halt,
            gate: Gate::phase_readonly(),
            desktop: None,
            permits: &gate::ClosedPermits,
            perms: None,
            context_length: 0,
            tasks: None,
        };
        let mut history = Vec::new();
        let mut events = Vec::new();
        let out = run_loop(&input, &mut history, "read the note", None, &mut |ev| events.push(ev));
        (out, history, events)
    }

    #[test]
    fn default_turn_cap_is_fifty() {
        assert_eq!(DEFAULT_MAX_TURNS, 50);
    }

    #[test]
    fn loop_runs_parallel_read_only_tools_and_sums_usage() {
        let dir = workspace("sum");
        let script = Script {
            turns: Mutex::new(vec![
                ScriptTurn {
                    text: String::new(),
                    calls: vec![
                        call("a", "read_file", r#"{"target_file":"note.txt"}"#),
                        call("b", "list_dir", "{}"),
                    ],
                    usage: usage(10, 5, 2, 100),
                },
                ScriptTurn {
                    text: "done".into(),
                    calls: Vec::new(),
                    usage: usage(7, 3, 1, 50),
                },
            ]),
            seen: Mutex::new(Vec::new()),
            cancel_on_text: false,
            steer: None,
        };
        let (out, history, events) = run(
            &script,
            &dir,
            DEFAULT_MAX_TURNS,
            &CancelToken::new(),
            &SteerQueue::new(),
            &NeverHalt,
        );
        assert_eq!(out.stop, StopReason::EndTurn);
        assert_eq!(out.usage.input_tokens, 17);
        assert_eq!(out.usage.output_tokens, 8);
        assert_eq!(out.usage.reasoning_tokens, 3);
        assert_eq!(out.usage.cost_in_usd_ticks, 150);
        let outputs: Vec<&str> = history
            .iter()
            .filter_map(|item| match item {
                InputItem::FunctionCallOutput { output, .. } => Some(output.as_str()),
                _ => None,
            })
            .collect();
        assert!(outputs.iter().any(|t| t.contains("hello native")), "{outputs:?}");
        assert!(outputs.len() >= 2, "{outputs:?}");
        assert!(events.iter().any(|ev| matches!(ev, LoopEvent::Tool { status, .. } if status == "in_progress")));
        assert!(events.iter().any(|ev| matches!(ev, LoopEvent::Tool { status, .. } if status == "completed")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn loop_refuses_write_injects_steer_and_guards_repeat_cancel_halt_cap() {
        let dir = workspace("guards");
        let write = Script {
            turns: Mutex::new(vec![
                ScriptTurn {
                    text: String::new(),
                    calls: vec![call("w", "write", r#"{"path":"x"}"#)],
                    usage: Usage::default(),
                },
                ScriptTurn {
                    text: "stopped".into(),
                    calls: Vec::new(),
                    usage: Usage::default(),
                },
            ]),
            seen: Mutex::new(Vec::new()),
            cancel_on_text: false,
            steer: None,
        };
        let (out, history, _) = run(&write, &dir, 4, &CancelToken::new(), &SteerQueue::new(), &NeverHalt);
        assert_eq!(out.stop, StopReason::EndTurn);
        assert!(history.iter().any(|item| matches!(
            item,
            InputItem::FunctionCallOutput { output, .. } if output.contains(tools::READ_ONLY_PHASE)
        )));
        assert!(!dir.join("x").exists());

        let steer = SteerQueue::new();
        let steer_script = Script {
            turns: Mutex::new(vec![
                ScriptTurn {
                    text: String::new(),
                    calls: vec![call("r", "read_file", r#"{"target_file":"missing.txt"}"#)],
                    usage: Usage::default(),
                },
                ScriptTurn {
                    text: "after steer".into(),
                    calls: Vec::new(),
                    usage: Usage::default(),
                },
            ]),
            seen: Mutex::new(Vec::new()),
            cancel_on_text: false,
            steer: Some(steer.clone()),
        };
        let _ = run(&steer_script, &dir, 4, &CancelToken::new(), &steer, &NeverHalt);
        let seen = steer_script.seen.lock().unwrap();
        let blob = format!("{:?}", seen.get(1));
        assert!(blob.contains("please look again"), "{blob}");
        drop(seen);

        let repeat = Script {
            turns: Mutex::new(vec![
                ScriptTurn {
                    text: String::new(),
                    calls: vec![call("1", "read_file", r#"{"target_file":"note.txt"}"#)],
                    usage: Usage::default(),
                },
                ScriptTurn {
                    text: String::new(),
                    calls: vec![call("2", "read_file", r#"{"target_file":"note.txt"}"#)],
                    usage: Usage::default(),
                },
                ScriptTurn {
                    text: String::new(),
                    calls: vec![call("3", "read_file", r#"{"target_file":"note.txt"}"#)],
                    usage: Usage::default(),
                },
            ]),
            seen: Mutex::new(Vec::new()),
            cancel_on_text: false,
            steer: None,
        };
        let (out, history, _) = run(&repeat, &dir, 10, &CancelToken::new(), &SteerQueue::new(), &NeverHalt);
        assert_eq!(out.stop, StopReason::RepeatedCall);
        let runs = history
            .iter()
            .filter(|item| {
                matches!(item, InputItem::FunctionCallOutput { output, .. } if !output.contains("not run again"))
            })
            .count();
        assert_eq!(runs, 2);

        let cancel_script = Script {
            turns: Mutex::new(vec![ScriptTurn {
                text: "nope".into(),
                calls: Vec::new(),
                usage: Usage::default(),
            }]),
            seen: Mutex::new(Vec::new()),
            cancel_on_text: true,
            steer: None,
        };
        let (out, _, _) = run(
            &cancel_script,
            &dir,
            4,
            &CancelToken::new(),
            &SteerQueue::new(),
            &NeverHalt,
        );
        assert_eq!(out.stop, StopReason::Cancelled);

        let halt_script = Script {
            turns: Mutex::new(vec![ScriptTurn {
                text: String::new(),
                calls: vec![call("h", "read_file", r#"{"target_file":"note.txt"}"#)],
                usage: Usage::default(),
            }]),
            seen: Mutex::new(Vec::new()),
            cancel_on_text: false,
            steer: None,
        };
        let (out, history, _) = run(
            &halt_script,
            &dir,
            4,
            &CancelToken::new(),
            &SteerQueue::new(),
            &HaltAfter(AtomicUsize::new(0)),
        );
        assert_eq!(out.stop, StopReason::Halted);
        assert!(!history.iter().any(|item| matches!(item, InputItem::FunctionCallOutput { .. })));

        let cap_client = AlwaysTool { calls: AtomicUsize::new(0) };
        let input = LoopIn {
            client: &cap_client,
            workspace: &dir,
            model: "grok-4.7",
            effort: None,
            system: "",
            conversation_id: "c",
            max_turns: 2,
            usage_base: Usage::default(),
            cancel: &CancelToken::new(),
            steer: &SteerQueue::new(),
            halt: &NeverHalt,
            gate: Gate::phase_readonly(),
            desktop: None,
            permits: &gate::ClosedPermits,
            perms: None,
            context_length: 0,
            tasks: None,
        };
        let mut history = Vec::new();
        let out = run_loop(&input, &mut history, "go", None, &mut |_| {});
        assert_eq!(out.stop, StopReason::MaxTurns);
        assert_eq!(cap_client.calls.load(Ordering::SeqCst), 2);
        let _ = std::fs::remove_dir_all(&dir);
    }

    struct Answer {
        answer: PermAnswer,
        asks: AtomicUsize,
    }

    impl PermitWait for Answer {
        fn wait(&self, _call_id: &str, _cancel: &crate::CancelToken, _halted: &dyn Fn() -> bool) -> Waited {
            self.asks.fetch_add(1, Ordering::SeqCst);
            Waited::Answer(self.answer)
        }
    }

    struct Spy {
        calls: AtomicUsize,
        halted: bool,
        locked: bool,
    }

    impl DesktopOps for Spy {
        fn halted(&self) -> bool {
            self.halted
        }
        fn locked(&self) -> bool {
            self.locked
        }
        fn call(&self, name: &str, _args: &serde_json::Value) -> ToolOutput {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if name == "screenshot" {
                ToolOutput {
                    text: "geom".into(),
                    image_data_url: Some("data:image/png;base64,YQ==".into()),
                    failed: false,
                }
            } else {
                ToolOutput::ok("ok")
            }
        }
    }

    fn chat(mode: gate::PermMode, attended: bool, desktop: bool) -> Gate {
        Gate {
            mode,
            readonly_session: false,
            attended,
            desktop,
        }
    }

    fn once(
        script: &Script,
        dir: &std::path::Path,
        gate: Gate,
        desktop: Option<&dyn DesktopOps>,
        permits: &dyn PermitWait,
    ) -> (LoopOut, Vec<InputItem>, Vec<LoopEvent>) {
        let input = LoopIn {
            client: script,
            workspace: dir,
            model: "grok-4.7",
            effort: None,
            system: "",
            conversation_id: "conv",
            max_turns: 4,
            usage_base: Usage::default(),
            cancel: &CancelToken::new(),
            steer: &SteerQueue::new(),
            halt: &NeverHalt,
            gate,
            desktop,
            permits,
            perms: None,
            context_length: 0,
            tasks: None,
        };
        let mut history = Vec::new();
        let mut events = Vec::new();
        let out = run_loop(&input, &mut history, "go", None, &mut |ev| events.push(ev));
        (out, history, events)
    }

    #[test]
    fn loop_gate_ask_allow_deny_always_and_unattended() {
        let dir = workspace("gate");
        let write_turn = |id: &str, path: &str| ScriptTurn {
            text: String::new(),
            calls: vec![call(id, "write", &format!(r#"{{"path":"{path}","content":"yes"}}"#))],
            usage: Usage::default(),
        };
        let done = ScriptTurn {
            text: "done".into(),
            calls: Vec::new(),
            usage: Usage::default(),
        };

        let allow = Answer { answer: PermAnswer::Allow, asks: AtomicUsize::new(0) };
        let asking = Script {
            turns: Mutex::new(vec![write_turn("w", "asked.txt"), done.clone()]),
            seen: Mutex::new(Vec::new()),
            cancel_on_text: false,
            steer: None,
        };
        let (out, _, events) = once(&asking, &dir, chat(gate::PermMode::Ask, true, false), None, &allow);
        assert_eq!(out.stop, StopReason::EndTurn);
        assert_eq!(allow.asks.load(Ordering::SeqCst), 1);
        assert!(events.iter().any(|ev| matches!(ev, LoopEvent::Permission { name, .. } if name == "write")));
        assert_eq!(std::fs::read_to_string(dir.join("asked.txt")).unwrap(), "yes");

        let auto = Answer { answer: PermAnswer::Deny, asks: AtomicUsize::new(0) };
        let auto_script = Script {
            turns: Mutex::new(vec![write_turn("a", "auto.txt"), done.clone()]),
            seen: Mutex::new(Vec::new()),
            cancel_on_text: false,
            steer: None,
        };
        let (out, history, events) = once(&auto_script, &dir, chat(gate::PermMode::Auto, true, false), None, &auto);
        assert_eq!(out.stop, StopReason::EndTurn);
        assert_eq!(auto.asks.load(Ordering::SeqCst), 1);
        assert!(events.iter().any(|ev| matches!(ev, LoopEvent::Permission { .. })));
        assert!(history.iter().any(|item| matches!(
            item,
            InputItem::FunctionCallOutput { output, .. } if output.contains("User rejected the execution for tool `write`")
        )));
        assert!(!dir.join("auto.txt").exists());

        let quiet = Answer { answer: PermAnswer::Deny, asks: AtomicUsize::new(0) };
        let always = Script {
            turns: Mutex::new(vec![write_turn("y", "always.txt"), done.clone()]),
            seen: Mutex::new(Vec::new()),
            cancel_on_text: false,
            steer: None,
        };
        let (out, _, events) = once(&always, &dir, chat(gate::PermMode::Always, true, false), None, &quiet);
        assert_eq!(out.stop, StopReason::EndTurn);
        assert_eq!(quiet.asks.load(Ordering::SeqCst), 0);
        assert!(!events.iter().any(|ev| matches!(ev, LoopEvent::Permission { .. })));
        assert_eq!(std::fs::read_to_string(dir.join("always.txt")).unwrap(), "yes");

        let away = Answer { answer: PermAnswer::Allow, asks: AtomicUsize::new(0) };
        let unattended = Script {
            turns: Mutex::new(vec![write_turn("u", "away.txt"), done.clone()]),
            seen: Mutex::new(Vec::new()),
            cancel_on_text: false,
            steer: None,
        };
        let (out, history, events) = once(&unattended, &dir, chat(gate::PermMode::Ask, false, false), None, &away);
        assert_eq!(out.stop, StopReason::EndTurn);
        assert_eq!(away.asks.load(Ordering::SeqCst), 0);
        assert!(!events.iter().any(|ev| matches!(ev, LoopEvent::Permission { .. })));
        assert!(history.iter().any(|item| matches!(
            item,
            InputItem::FunctionCallOutput { output, .. }
                if output == "Tool `write` was not executed: Denied by permission policy: deny rule on edit"
        )));
        assert!(!dir.join("away.txt").exists());

        let auto_away = Answer { answer: PermAnswer::Deny, asks: AtomicUsize::new(0) };
        let auto_go = Script {
            turns: Mutex::new(vec![write_turn("g", "autogo.txt"), done]),
            seen: Mutex::new(Vec::new()),
            cancel_on_text: false,
            steer: None,
        };
        let (out, _, _) = once(&auto_go, &dir, chat(gate::PermMode::Auto, false, false), None, &auto_away);
        assert_eq!(out.stop, StopReason::EndTurn);
        assert_eq!(auto_away.asks.load(Ordering::SeqCst), 0);
        assert!(!dir.join("autogo.txt").exists(), "unattended Auto denies until Phase 5");

        let escape = Script {
            turns: Mutex::new(vec![
                ScriptTurn {
                    text: String::new(),
                    calls: vec![call("e", "write", r#"{"path":"../nope.txt","content":"x"}"#)],
                    usage: Usage::default(),
                },
                ScriptTurn {
                    text: "done".into(),
                    calls: Vec::new(),
                    usage: Usage::default(),
                },
            ]),
            seen: Mutex::new(Vec::new()),
            cancel_on_text: false,
            steer: None,
        };
        let (out, history, _) = once(&escape, &dir, chat(gate::PermMode::Always, true, false), None, &quiet);
        assert_eq!(out.stop, StopReason::EndTurn);
        assert!(history.iter().any(|item| matches!(
            item,
            InputItem::FunctionCallOutput { output, .. } if output.contains("escapes")
        )));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn allow_always_covers_the_rest_of_the_turn() {
        let dir = workspace("latch");
        let permits = Answer { answer: PermAnswer::Always, asks: AtomicUsize::new(0) };
        let script = Script {
            turns: Mutex::new(vec![ScriptTurn {
                text: String::new(),
                calls: vec![
                    call("1", "write", r#"{"path":"a.txt","content":"a"}"#),
                    call("2", "write", r#"{"path":"b.txt","content":"b"}"#),
                ],
                usage: Usage::default(),
            }, ScriptTurn {
                text: "done".into(),
                calls: Vec::new(),
                usage: Usage::default(),
            }]),
            seen: Mutex::new(Vec::new()),
            cancel_on_text: false,
            steer: None,
        };
        let (out, _, _) = once(&script, &dir, chat(gate::PermMode::Ask, true, false), None, &permits);
        assert_eq!(out.stop, StopReason::EndTurn);
        assert_eq!(permits.asks.load(Ordering::SeqCst), 1);
        assert_eq!(std::fs::read_to_string(dir.join("a.txt")).unwrap(), "a");
        assert_eq!(std::fs::read_to_string(dir.join("b.txt")).unwrap(), "b");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn desktop_tools_follow_the_switch_halt_and_lock() {
        let dir = workspace("desk");
        let shot = |id: &str| ScriptTurn {
            text: String::new(),
            calls: vec![call(id, "screenshot", "{}")],
            usage: Usage::default(),
        };
        let done = ScriptTurn {
            text: "done".into(),
            calls: Vec::new(),
            usage: Usage::default(),
        };
        let permits = Answer { answer: PermAnswer::Allow, asks: AtomicUsize::new(0) };

        let off_script = Script {
            turns: Mutex::new(vec![shot("s"), done.clone()]),
            seen: Mutex::new(Vec::new()),
            cancel_on_text: false,
            steer: None,
        };
        let spy = Spy { calls: AtomicUsize::new(0), halted: false, locked: false };
        let (out, history, _) = once(
            &off_script,
            &dir,
            chat(gate::PermMode::Always, true, false),
            Some(&spy),
            &permits,
        );
        assert_eq!(out.stop, StopReason::EndTurn);
        assert_eq!(spy.calls.load(Ordering::SeqCst), 0);
        assert!(history.iter().any(|item| matches!(
            item,
            InputItem::FunctionCallOutput { output, .. }
                if output.contains(grokhub_core::desktop_mcp::OFF_MSG)
        )));
        let body = crate::responses_body(&crate::ResponsesRequest {
            model: "grok-4.7".into(),
            effort: None,
            input: Vec::new(),
            conversation_id: "c".into(),
            tools: tools::schemas_for(&chat(gate::PermMode::Always, true, false)),
            hosted_search: true,
            call_timeout: None,
        });
        assert!(body["tools"].as_array().unwrap().iter().all(|tool| tool["name"] != "screenshot"));

        let halt_script = Script {
            turns: Mutex::new(vec![shot("h"), done.clone()]),
            seen: Mutex::new(Vec::new()),
            cancel_on_text: false,
            steer: None,
        };
        let halted = Spy { calls: AtomicUsize::new(0), halted: true, locked: false };
        let (_, history, _) = once(
            &halt_script,
            &dir,
            chat(gate::PermMode::Always, true, true),
            Some(&halted),
            &permits,
        );
        assert_eq!(halted.calls.load(Ordering::SeqCst), 0);
        assert!(history.iter().any(|item| matches!(
            item,
            InputItem::FunctionCallOutput { output, .. }
                if output.contains(grokhub_core::desktop_mcp::HALT_MSG)
        )));

        let lock_script = Script {
            turns: Mutex::new(vec![shot("l"), done.clone()]),
            seen: Mutex::new(Vec::new()),
            cancel_on_text: false,
            steer: None,
        };
        let locked = Spy { calls: AtomicUsize::new(0), halted: false, locked: true };
        let (_, history, _) = once(
            &lock_script,
            &dir,
            chat(gate::PermMode::Always, true, true),
            Some(&locked),
            &permits,
        );
        assert_eq!(locked.calls.load(Ordering::SeqCst), 0);
        assert!(history.iter().any(|item| matches!(
            item,
            InputItem::FunctionCallOutput { output, .. }
                if output.contains(grokhub_core::desktop_mcp::LOCK_MSG)
        )));

        let run_script = Script {
            turns: Mutex::new(vec![shot("r"), done]),
            seen: Mutex::new(Vec::new()),
            cancel_on_text: false,
            steer: None,
        };
        let live = Spy { calls: AtomicUsize::new(0), halted: false, locked: false };
        let (_, history, _) = once(
            &run_script,
            &dir,
            chat(gate::PermMode::Always, true, true),
            Some(&live),
            &permits,
        );
        assert_eq!(live.calls.load(Ordering::SeqCst), 1);
        assert!(history.iter().any(|item| matches!(
            item,
            InputItem::Message { content, .. }
                if content.iter().any(|part| matches!(part, ContentPart::InputText(text) if text == "image from screenshot"))
        )));
        let tools = tools::schemas_for(&chat(gate::PermMode::Always, true, true));
        assert!(tools.iter().any(|tool| tool["name"] == "screenshot"));
        assert!(tools.iter().all(|tool| tool["name"] != "list_monitors"));
        let plan = tools::schemas_for(&Gate::phase_readonly());
        assert!(plan.iter().all(|tool| tool["name"] != "write" && tool["name"] != "screenshot"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    fn notice_count(items: &[InputItem]) -> usize {
        items
            .iter()
            .filter(|item| {
                matches!(
                    item,
                    InputItem::Message { content, .. }
                        if content.iter().any(|part| matches!(part, ContentPart::InputText(text) if text.contains("<background-notice>")))
                )
            })
            .count()
    }

    #[cfg(unix)]
    fn task_id_in(items: &[InputItem]) -> Option<String> {
        for item in items.iter().rev() {
            let InputItem::FunctionCallOutput { output, .. } = item else {
                continue;
            };
            for line in output.lines() {
                if let Some(rest) = line.trim().strip_prefix("task id:") {
                    let id = rest.trim();
                    if !id.is_empty() {
                        return Some(id.to_string());
                    }
                }
            }
        }
        None
    }

    #[cfg(unix)]
    struct NoticeClient {
        n: AtomicUsize,
        seen: Mutex<Vec<Vec<InputItem>>>,
    }

    #[cfg(unix)]
    impl ModelClient for NoticeClient {
        fn stream(
            &self,
            req: &crate::ResponsesRequest,
            _cancel: &CancelToken,
            _sink: &mut dyn FnMut(crate::StreamEvent),
        ) -> Result<crate::TurnOutput, crate::ClientError> {
            self.seen.lock().unwrap().push(req.input.clone());
            let n = self.n.fetch_add(1, Ordering::SeqCst);
            let calls = if n == 0 {
                vec![call(
                    "bg",
                    "run_terminal_command",
                    r#"{"command":"echo hello-notice","is_background":true}"#,
                )]
            } else if n == 1 {
                let id = task_id_in(&req.input).unwrap_or_else(|| "missing".into());
                vec![call(
                    "out",
                    "get_command_or_subagent_output",
                    &format!(r#"{{"task_id":"{id}","timeout_ms":8000}}"#),
                )]
            } else if n == 2 {
                vec![call("r", "read_file", r#"{"target_file":"note.txt"}"#)]
            } else {
                Vec::new()
            };
            Ok(crate::TurnOutput {
                text: if calls.is_empty() {
                    "done".into()
                } else {
                    String::new()
                },
                reasoning: String::new(),
                calls,
                usage: Usage::default(),
            })
        }
    }

    #[cfg(unix)]
    #[test]
    fn notice_is_injected_once_on_the_next_turn() {
        let dir = workspace("notice");
        let sid = format!("notice-{}", std::process::id());
        let hub = crate::tasks::hub_for(&sid);
        hub.reopen();
        let client = NoticeClient {
            n: AtomicUsize::new(0),
            seen: Mutex::new(Vec::new()),
        };
        let input = LoopIn {
            client: &client,
            workspace: &dir,
            model: "grok-4.7",
            effort: None,
            system: "",
            conversation_id: &sid,
            max_turns: 6,
            usage_base: Usage::default(),
            cancel: &CancelToken::new(),
            steer: &SteerQueue::new(),
            halt: &NeverHalt,
            gate: chat(gate::PermMode::Always, true, false),
            desktop: None,
            permits: &gate::ClosedPermits,
            perms: None,
            context_length: 0,
            tasks: Some(hub),
        };
        let mut history = Vec::new();
        let out = run_loop(&input, &mut history, "go", None, &mut |_| {});
        assert_eq!(out.stop, StopReason::EndTurn);
        let seen = client.seen.lock().unwrap();
        assert!(seen.len() >= 4, "turns {}", seen.len());
        assert_eq!(notice_count(&seen[0]), 0, "the first request has no notice");
        assert!(seen.iter().all(|items| notice_count(items) <= 1));
        assert_eq!(notice_count(&seen[2]), 1, "{:?}", seen[2]);
        assert_eq!(
            notice_count(&seen[3]),
            1,
            "a later turn must not copy the notice again"
        );
        let blob = format!("{:?}", seen[3]);
        assert!(blob.contains("hello-notice"), "{blob}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn steer_lands_at_the_next_tool_boundary() {
        let dir = workspace("steer-boundary");
        let steer = SteerQueue::new();
        let script = Script {
            turns: Mutex::new(vec![
                ScriptTurn {
                    text: String::new(),
                    calls: vec![call("r", "read_file", r#"{"target_file":"note.txt"}"#)],
                    usage: Usage::default(),
                },
                ScriptTurn {
                    text: "after steer".into(),
                    calls: Vec::new(),
                    usage: Usage::default(),
                },
            ]),
            seen: Mutex::new(Vec::new()),
            cancel_on_text: false,
            steer: Some(steer.clone()),
        };
        let _ = run(&script, &dir, 4, &CancelToken::new(), &steer, &NeverHalt);
        let seen = script.seen.lock().unwrap();
        let first = format!("{:?}", seen.first());
        assert!(!first.contains("please look again"), "{first}");
        let second = format!("{:?}", seen.get(1));
        let output_at = second.find("FunctionCallOutput").expect(&second);
        let steer_at = second.find("please look again").expect(&second);
        assert!(output_at < steer_at, "{second}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn halt_stops_the_run_and_its_tasks_and_monitors() {
        let dir = workspace("halt-tasks");
        let sid = format!(
            "halt-run-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        );
        let hub = crate::tasks::hub_for(&sid);
        hub.reopen();
        let task = hub
            .spawn(&dir, "sleep 30 & echo $! > child.pid; wait")
            .expect("spawn");
        let _mon = hub
            .start_monitor(&dir, None, Some(task), None, 60_000, "watch".into())
            .expect("monitor");
        let pidfile = dir.join("child.pid");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(8);
        let mut pid = 0i32;
        while std::time::Instant::now() < deadline {
            if let Ok(text) = std::fs::read_to_string(&pidfile) {
                if let Ok(n) = text.trim().parse::<i32>() {
                    if n > 1 {
                        pid = n;
                        break;
                    }
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(pid > 1, "child pid was not written");
        let script = Script {
            turns: Mutex::new(vec![ScriptTurn {
                text: String::new(),
                calls: vec![call("r", "read_file", r#"{"target_file":"note.txt"}"#)],
                usage: Usage::default(),
            }]),
            seen: Mutex::new(Vec::new()),
            cancel_on_text: false,
            steer: None,
        };
        let input = LoopIn {
            client: &script,
            workspace: &dir,
            model: "grok-4.7",
            effort: None,
            system: "",
            conversation_id: &sid,
            max_turns: 4,
            usage_base: Usage::default(),
            cancel: &CancelToken::new(),
            steer: &SteerQueue::new(),
            halt: &HaltAfter(AtomicUsize::new(0)),
            gate: Gate::phase_readonly(),
            desktop: None,
            permits: &gate::ClosedPermits,
            perms: None,
            context_length: 0,
            tasks: Some(hub.clone()),
        };
        let mut history = Vec::new();
        let out = run_loop(&input, &mut history, "go", None, &mut |_| {});
        assert_eq!(out.stop, StopReason::Halted);
        assert!(hub.is_halted());
        assert!(hub.spawn(&dir, "echo still").is_err());
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        while std::time::Instant::now() < deadline {
            let alive = unsafe { libc::kill(pid, 0) } == 0;
            if !alive {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let alive = unsafe { libc::kill(pid, 0) } == 0;
        assert!(!alive, "child {pid} still alive");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        let mut notes = Vec::new();
        while std::time::Instant::now() < deadline {
            notes.extend(hub.drain_notices());
            if notes
                .iter()
                .any(|note| note.contains("cancelled") || note.contains("killed"))
            {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(
            notes
                .iter()
                .any(|note| note.contains("cancelled") || note.contains("killed")),
            "{notes:?}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn bg_ask_refuses_non_readonly_tools() {
        let dir = workspace("bg-ask");
        let cfg = std::env::temp_dir().join(format!("gh-bg-ask-cfg-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&cfg);
        std::fs::create_dir_all(&cfg).unwrap();
        let _guard = crate::perm::ConfigGuard::set(&cfg);
        let asks = Answer {
            answer: PermAnswer::Deny,
            asks: AtomicUsize::new(0),
        };
        let script = Script {
            turns: Mutex::new(vec![
                ScriptTurn {
                    text: String::new(),
                    calls: vec![
                        call("w", "write", r#"{"path":"no.txt","content":"x"}"#),
                        call(
                            "s",
                            "run_terminal_command",
                            r#"{"command":"echo ran > ran.txt"}"#,
                        ),
                        call("k", "kill_command_or_subagent", r#"{"task_id":"t1"}"#),
                        call(
                            "c",
                            "scheduler_create",
                            r#"{"interval":"10m","prompt":"nope"}"#,
                        ),
                        call("m", "monitor", r#"{"command":"echo hi"}"#),
                    ],
                    usage: Usage::default(),
                },
                ScriptTurn {
                    text: "done".into(),
                    calls: Vec::new(),
                    usage: Usage::default(),
                },
            ]),
            seen: Mutex::new(Vec::new()),
            cancel_on_text: false,
            steer: None,
        };
        let sid = format!("bg-ask-{}", std::process::id());
        let hub = crate::tasks::hub_for(&sid);
        hub.reopen();
        let input = LoopIn {
            client: &script,
            workspace: &dir,
            model: "grok-4.7",
            effort: None,
            system: "",
            conversation_id: &sid,
            max_turns: 4,
            usage_base: Usage::default(),
            cancel: &CancelToken::new(),
            steer: &SteerQueue::new(),
            halt: &NeverHalt,
            gate: chat(gate::PermMode::Ask, false, false),
            desktop: None,
            permits: &asks,
            perms: None,
            context_length: 0,
            tasks: Some(hub),
        };
        let mut history = Vec::new();
        let mut events = Vec::new();
        let out = run_loop(&input, &mut history, "go", None, &mut |ev| events.push(ev));
        assert_eq!(out.stop, StopReason::EndTurn);
        assert_eq!(asks.asks.load(Ordering::SeqCst), 0);
        assert!(events
            .iter()
            .all(|ev| !matches!(ev, LoopEvent::Permission { .. })));
        for name in [
            "write",
            "run_terminal_command",
            "kill_command_or_subagent",
            "scheduler_create",
            "monitor",
        ] {
            assert!(
                history.iter().any(|item| matches!(
                    item,
                    InputItem::FunctionCallOutput { output, .. }
                        if output.contains(name) && output.contains("was not executed")
                )),
                "{name} missing from {history:?}"
            );
        }
        assert!(!dir.join("no.txt").exists());
        assert!(!dir.join("ran.txt").exists());
        assert!(!cfg.join("automations.json").exists());
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&cfg);
    }
}

