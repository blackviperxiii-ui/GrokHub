//! Agent loop. Turns stop when the model stops calling tools, or at the cap.

use std::collections::{HashMap, VecDeque};
use std::path::Path;
use std::sync::{Arc, Mutex};

use crate::tools::{self, ToolOutput};
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
}

pub struct LoopOut {
    pub stop: StopReason,
    pub usage: Usage,
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
    let mut usage = input.usage_base.clone();
    let mut repeats: HashMap<String, u32> = HashMap::new();
    let max_turns = if input.max_turns == 0 {
        DEFAULT_MAX_TURNS
    } else {
        input.max_turns
    };
    for _turn in 0..max_turns {
        if input.cancel.is_cancelled() {
            return LoopOut { stop: StopReason::Cancelled, usage };
        }
        if input.halt.halted() {
            return LoopOut { stop: StopReason::Halted, usage };
        }
        let req = ResponsesRequest {
            model: input.model.to_string(),
            effort: input.effort.map(str::to_string),
            input: history.clone(),
            conversation_id: input.conversation_id.to_string(),
        };
        let turn = match input.client.stream(&req, input.cancel, &mut |ev| match ev {
            StreamEvent::TextDelta(text) => on_event(LoopEvent::Text(text)),
            StreamEvent::ReasoningDelta(text) => on_event(LoopEvent::Thought(text)),
        }) {
            Ok(turn) => turn,
            Err(ClientError::Cancelled) => return LoopOut { stop: StopReason::Cancelled, usage },
            Err(err) => {
                return LoopOut {
                    stop: StopReason::Error(err.to_string()),
                    usage,
                }
            }
        };
        usage.add(&turn.usage);
        on_event(LoopEvent::Usage(usage.clone()));
        if !turn.text.is_empty() {
            history.push(InputItem::Message {
                role: "assistant".into(),
                content: vec![ContentPart::InputText(turn.text)],
            });
        }
        if turn.calls.is_empty() {
            let notes = input.steer.drain();
            if notes.is_empty() {
                return LoopOut { stop: StopReason::EndTurn, usage };
            }
            for note in notes {
                history.push(user_message(&note, None));
            }
            continue;
        }
        let mut repeated = false;
        for call in &turn.calls {
            history.push(InputItem::FunctionCall {
                call_id: call.call_id.clone(),
                name: call.name.clone(),
                arguments: call.arguments.clone(),
            });
        }
        for call in &turn.calls {
            if input.cancel.is_cancelled() {
                return LoopOut { stop: StopReason::Cancelled, usage };
            }
            if input.halt.halted() {
                return LoopOut { stop: StopReason::Halted, usage };
            }
            let key = format!("{}\\n{}", call.name, call.arguments);
            let seen = repeats.entry(key).or_insert(0);
            *seen = seen.saturating_add(1);
            let output = if *seen >= REPEAT_LIMIT {
                repeated = true;
                ToolOutput::err(format!(
                    "{}; the same call already ran twice, so it was not run again",
                    tools::READ_ONLY_PHASE
                ))
            } else {
                let id = tool_id(call);
                on_event(LoopEvent::Tool {
                    id: id.clone(),
                    name: call.name.clone(),
                    status: "in_progress".into(),
                    detail: clip(&call.arguments, 180),
                    image: None,
                });
                let output = tools::execute(input.workspace, &call.name, &call.arguments);
                on_event(LoopEvent::Tool {
                    id,
                    name: call.name.clone(),
                    status: if output.failed { "failed" } else { "completed" }.into(),
                    detail: clip(&output.text, 180),
                    image: output.image_data_url.clone(),
                });
                output
            };
            history.push(InputItem::FunctionCallOutput {
                call_id: call.call_id.clone(),
                output: output.text.clone(),
            });
            if let Some(url) = output.image_data_url {
                history.push(InputItem::Message {
                    role: "user".into(),
                    content: vec![
                        ContentPart::InputText("image from read_file".into()),
                        ContentPart::InputImage(url),
                    ],
                });
            }
        }
        if repeated {
            return LoopOut { stop: StopReason::RepeatedCall, usage };
        }
        for note in input.steer.drain() {
            history.push(user_message(&note, None));
        }
    }
    LoopOut { stop: StopReason::MaxTurns, usage }
}

fn user_message(text: &str, image: Option<&str>) -> InputItem {
    let mut content = vec![ContentPart::InputText(text.to_string())];
    if let Some(url) = image.map(str::trim).filter(|s| !s.is_empty()) {
        content.push(ContentPart::InputImage(url.to_string()));
    }
    InputItem::Message { role: "user".into(), content }
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
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct Script {
        turns: Mutex<Vec<ScriptTurn>>,
        seen: Mutex<Vec<Vec<InputItem>>>,
        cancel_on_text: bool,
        steer: Option<SteerQueue>,
    }

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
        };
        let mut history = Vec::new();
        let out = run_loop(&input, &mut history, "go", None, &mut |_| {});
        assert_eq!(out.stop, StopReason::MaxTurns);
        assert_eq!(cap_client.calls.load(Ordering::SeqCst), 2);
        let _ = std::fs::remove_dir_all(&dir);
    }

}

