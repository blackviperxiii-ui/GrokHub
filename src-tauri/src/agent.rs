//! The agent loop behind chat (cross-platform recode, Phase 3). Each
//! conversation gets a worker thread that owns a `grokhub_agent::NativeEngine`,
//! the same loop the egui cabin runs. Tool calls go through the cabin's
//! permission gate in Ask mode: a risky step parks until the UI answers.

use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;

use grokhub_agent::{
    AuthKind, CancelToken, Engine, EngineParts, Gate, HaltCheck, ModelClient, NativeEngine,
    PermAnswer, PermMode, PermitInbox, PermitNote, SteerQueue,
};
use grokhub_core::wire::AcpEvent;
use serde::Serialize;

/// The cabin's attended chat gate: tools that change things ask first, and
/// computer-use stays off until the shell has a desktop backend.
pub fn shell_gate() -> Gate {
    Gate {
        mode: PermMode::Ask,
        readonly_session: false,
        attended: true,
        desktop: false,
    }
}

/// The system prompt the cabin's native chat uses, without the desktop line.
pub fn system_prompt(workspace: &std::path::Path) -> String {
    grokhub_agent::system_prompt_parts(
        &grokhub_core::cabin_rules::cabin_static_rules(false),
        "",
        workspace,
    )
}

/// The folder the agent works in: the one picked in Settings, or the
/// cabin's default session folder.
pub fn workspace_for(picked: &str) -> PathBuf {
    let home = std::env::var("HOME").ok();
    let profile = std::env::var("USERPROFILE").ok();
    PathBuf::from(grokhub_core::cabin_session_cwd(
        picked,
        cfg!(windows),
        home.as_deref(),
        profile.as_deref(),
    ))
}

/// What the UI renders. Engine events the shell does not show yet map to `None`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum UiEvent {
    Text {
        text: String,
    },
    Tool {
        id: String,
        title: String,
        status: String,
        detail: String,
    },
    Permission {
        id: String,
        title: String,
        action: String,
        reason: String,
    },
    Error {
        message: String,
    },
    Done {
        stop: String,
    },
}

pub fn ui_event(event: AcpEvent) -> Option<UiEvent> {
    Some(match event {
        AcpEvent::Text(text) => UiEvent::Text { text },
        AcpEvent::Tool(card) => UiEvent::Tool {
            id: card.id,
            title: card.title,
            status: card.status,
            detail: card.detail,
        },
        AcpEvent::Permission(ask) => UiEvent::Permission {
            id: ask.tool_call_id,
            title: ask.title,
            action: ask.action,
            reason: ask.reason,
        },
        AcpEvent::Err(message) => UiEvent::Error { message },
        AcpEvent::Done { stop_reason } => UiEvent::Done { stop: stop_reason },
        _ => return None,
    })
}

pub fn parse_answer(answer: &str) -> Result<PermAnswer, String> {
    match answer {
        "allow" => Ok(PermAnswer::Allow),
        "always" => Ok(PermAnswer::Always),
        "deny" => Ok(PermAnswer::Deny),
        other => Err(format!("unknown answer: {other}")),
    }
}

/// The model, credential and folder for one prompt. Read fresh for every
/// prompt, so a Settings change applies to the next message.
pub struct Route {
    pub client: Arc<dyn ModelClient + Send + Sync>,
    pub auth_kind: AuthKind,
    pub model: String,
    pub workspace: PathBuf,
}

struct Job {
    text: String,
    route: Route,
    emit: Box<dyn FnMut(UiEvent) + Send>,
    done: Sender<Result<String, String>>,
}

struct NeverHalted;

impl HaltCheck for NeverHalted {
    fn halted(&self) -> bool {
        false
    }
}

/// One conversation. Dropping it ends the worker once its prompt returns.
pub struct Session {
    jobs: Sender<Job>,
    permits: Sender<PermitNote>,
    cancel: CancelToken,
}

impl Session {
    /// `config_dir` pins the GrokHub config folder for the worker, where the
    /// engine keeps session files and the permission rules it learns.
    pub fn spawn(conversation_id: &str, config_dir: PathBuf) -> Self {
        let (jobs, rx) = channel::<Job>();
        let (permits, inbox) = PermitInbox::pair();
        let cancel = CancelToken::new();
        let worker_cancel = cancel.clone();
        let id = conversation_id.to_string();
        std::thread::spawn(move || serve(id, config_dir, worker_cancel, inbox, rx));
        Self {
            jobs,
            permits,
            cancel,
        }
    }

    /// Runs one prompt and waits for it. Blocks while a tool waits for an
    /// answer, so call it off the UI thread.
    pub fn prompt(
        &self,
        text: &str,
        route: Route,
        emit: Box<dyn FnMut(UiEvent) + Send>,
    ) -> Result<String, String> {
        if text.trim().is_empty() {
            return Err("Type a message first.".into());
        }
        let (done, wait) = channel();
        self.jobs
            .send(Job {
                text: text.to_string(),
                route,
                emit,
                done,
            })
            .map_err(|_| "The chat worker stopped.".to_string())?;
        wait.recv()
            .map_err(|_| "The chat worker stopped.".to_string())?
    }

    pub fn answer(&self, call_id: &str, answer: PermAnswer) {
        let _ = self.permits.send(PermitNote {
            id: call_id.to_string(),
            answer,
        });
    }

    /// Stops the running prompt. A tool that waits for an answer is cancelled
    /// and does not run.
    pub fn cancel(&self) {
        self.cancel.cancel();
    }
}

fn serve(
    conversation_id: String,
    config_dir: PathBuf,
    cancel: CancelToken,
    inbox: PermitInbox,
    jobs: Receiver<Job>,
) {
    let _config = grokhub_agent::perm::ConfigGuard::set(&config_dir);
    let mut engine: Option<NativeEngine> = None;
    let mut inbox = Some(inbox);
    while let Ok(job) = jobs.recv() {
        let Job {
            text,
            route,
            mut emit,
            done,
        } = job;
        let system = system_prompt(&route.workspace);
        let engine = match (&mut engine, inbox.take()) {
            (Some(engine), _) => {
                engine.set_workspace(route.workspace);
                engine.set_route(route.client, route.model, None, system, route.auth_kind);
                engine
            }
            (none, inbox) => {
                let Some(inbox) = inbox else {
                    let _ = done.send(Err("The chat worker stopped.".into()));
                    return;
                };
                none.insert(NativeEngine::new(EngineParts {
                    client: route.client,
                    workspace: route.workspace,
                    model: route.model,
                    effort: None,
                    system,
                    conversation_id: conversation_id.clone(),
                    auth_kind: route.auth_kind,
                    max_turns: 0,
                    cancel: cancel.clone(),
                    steer: SteerQueue::new(),
                    halt: Box::new(NeverHalted),
                    gate: shell_gate(),
                    desktop: None,
                    permits: Arc::new(inbox),
                }))
            }
        };
        let mut stop = String::new();
        let mut error = None;
        let ran = engine.prompt(&text, None, &mut |event| {
            let Some(ui) = ui_event(event) else {
                return;
            };
            match &ui {
                UiEvent::Done { stop: s } => stop = s.clone(),
                UiEvent::Error { message } => error = Some(message.clone()),
                _ => {}
            }
            emit(ui);
        });
        let result = match (ran, error) {
            (Err(err), _) => Err(err),
            (Ok(()), Some(message)) if stop == "error" => Err(message),
            (Ok(()), _) => Ok(stop),
        };
        let _ = done.send(result);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use grokhub_agent::{
        ClientError, FunctionCall, InputItem, ResponsesRequest, StreamEvent, TurnOutput,
    };
    use std::sync::Mutex;
    use std::time::Duration;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "grokhub-shell-agent-{name}-{}-{}",
            std::process::id(),
            grokhub_core::now_ms()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    /// Asks for one tool call, then answers with the tool's output.
    struct OneTool {
        name: &'static str,
        arguments: String,
        requests: Mutex<Vec<ResponsesRequest>>,
    }

    impl OneTool {
        fn new(name: &'static str, arguments: serde_json::Value) -> Arc<Self> {
            Arc::new(Self {
                name,
                arguments: arguments.to_string(),
                requests: Mutex::new(Vec::new()),
            })
        }

        fn calls(&self) -> usize {
            self.requests.lock().expect("requests").len()
        }

        fn tool_output(&self) -> Option<String> {
            let requests = self.requests.lock().expect("requests");
            requests.last()?.input.iter().find_map(|item| match item {
                InputItem::FunctionCallOutput { output, .. } => Some(output.clone()),
                _ => None,
            })
        }
    }

    impl ModelClient for OneTool {
        fn stream(
            &self,
            req: &ResponsesRequest,
            _cancel: &CancelToken,
            sink: &mut dyn FnMut(StreamEvent),
        ) -> Result<TurnOutput, ClientError> {
            let mut requests = self.requests.lock().expect("requests");
            requests.push(req.clone());
            if requests.len() == 1 {
                return Ok(TurnOutput {
                    calls: vec![FunctionCall {
                        call_id: "call-1".into(),
                        name: self.name.into(),
                        arguments: self.arguments.clone(),
                    }],
                    ..TurnOutput::default()
                });
            }
            sink(StreamEvent::TextDelta("All done.".into()));
            Ok(TurnOutput {
                text: "All done.".into(),
                ..TurnOutput::default()
            })
        }
    }

    struct Run {
        session: Arc<Session>,
        events: Arc<Mutex<Vec<UiEvent>>>,
        result: std::thread::JoinHandle<Result<String, String>>,
    }

    fn start(client: Arc<OneTool>, workspace: &std::path::Path, config: &std::path::Path) -> Run {
        let session = Arc::new(Session::spawn("gate-test", config.to_path_buf()));
        let events = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&events);
        let route = Route {
            client,
            auth_kind: AuthKind::ApiKey,
            model: "grok-4.7".into(),
            workspace: workspace.to_path_buf(),
        };
        let runner = Arc::clone(&session);
        let result = std::thread::spawn(move || {
            runner.prompt(
                "do the thing",
                route,
                Box::new(move |event| sink.lock().expect("events").push(event)),
            )
        });
        Run {
            session,
            events,
            result,
        }
    }

    fn wait_for_permission(run: &Run) -> UiEvent {
        for _ in 0..400 {
            let found = run
                .events
                .lock()
                .expect("events")
                .iter()
                .find(|event| matches!(event, UiEvent::Permission { .. }))
                .cloned();
            if let Some(ask) = found {
                return ask;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        panic!(
            "no permission card: {:?}",
            run.events.lock().expect("events")
        );
    }

    /// The prompt is still parked, nothing ran, and the model was not called again.
    fn assert_parked(run: &Run, client: &OneTool) {
        std::thread::sleep(Duration::from_millis(400));
        assert!(
            !run.result.is_finished(),
            "the turn went on without an answer"
        );
        assert_eq!(client.calls(), 1);
    }

    #[test]
    fn write_waits_for_an_ok_and_deny_leaves_no_file() {
        let (workspace, config) = (temp_dir("deny-ws"), temp_dir("deny-cfg"));
        let client = OneTool::new(
            "write",
            serde_json::json!({"path": "note.txt", "content": "hi"}),
        );
        let run = start(Arc::clone(&client), &workspace, &config);

        let ask = wait_for_permission(&run);
        let UiEvent::Permission { id, title, .. } = &ask else {
            unreachable!()
        };
        assert_eq!((id.as_str(), title.as_str()), ("call-1", "write"));
        assert_parked(&run, &client);
        assert!(!workspace.join("note.txt").exists());

        run.session.answer("call-1", PermAnswer::Deny);
        assert_eq!(run.result.join().expect("join"), Ok("end_turn".to_string()));
        assert!(!workspace.join("note.txt").exists());
        assert_eq!(
            client.tool_output().as_deref(),
            Some("User rejected the execution for tool `write`")
        );
        let events = run.events.lock().expect("events");
        assert!(events.contains(&UiEvent::Text {
            text: "All done.".into()
        }));
        assert_eq!(
            events.last(),
            Some(&UiEvent::Done {
                stop: "end_turn".into()
            })
        );
    }

    #[test]
    fn allow_runs_the_write() {
        let (workspace, config) = (temp_dir("allow-ws"), temp_dir("allow-cfg"));
        let client = OneTool::new(
            "write",
            serde_json::json!({"path": "note.txt", "content": "hi"}),
        );
        let run = start(Arc::clone(&client), &workspace, &config);

        wait_for_permission(&run);
        assert_parked(&run, &client);
        assert!(!workspace.join("note.txt").exists());

        run.session.answer("call-1", PermAnswer::Allow);
        assert_eq!(run.result.join().expect("join"), Ok("end_turn".to_string()));
        assert_eq!(
            std::fs::read_to_string(workspace.join("note.txt")).expect("written"),
            "hi"
        );
        // The loop then nudges the model to check its change; only the
        // tool's output matters here.
        let requests = client.requests.lock().expect("requests");
        assert_eq!(
            requests.get(1).and_then(|r| r.input.last()),
            Some(&InputItem::FunctionCallOutput {
                call_id: "call-1".into(),
                output: "wrote 2 bytes to note.txt".into(),
            })
        );
    }

    #[test]
    fn a_shell_command_waits_and_stop_runs_nothing() {
        let (workspace, config) = (temp_dir("stop-ws"), temp_dir("stop-cfg"));
        let client = OneTool::new(
            "run_terminal_command",
            serde_json::json!({"command": "echo ran > ran.txt"}),
        );
        let run = start(Arc::clone(&client), &workspace, &config);

        let ask = wait_for_permission(&run);
        assert!(
            matches!(&ask, UiEvent::Permission { title, action, .. }
                if title == "run_terminal_command" && action.contains("echo ran")),
            "{ask:?}"
        );
        assert_parked(&run, &client);

        run.session.cancel();
        assert_eq!(
            run.result.join().expect("join"),
            Ok("cancelled".to_string())
        );
        std::thread::sleep(Duration::from_millis(200));
        assert!(!workspace.join("ran.txt").exists());
        assert_eq!(client.calls(), 1);
    }

    #[test]
    fn a_read_runs_without_asking() {
        let (workspace, config) = (temp_dir("read-ws"), temp_dir("read-cfg"));
        std::fs::write(workspace.join("a.txt"), "alpha").expect("seed");
        let client = OneTool::new("read_file", serde_json::json!({"target_file": "a.txt"}));
        let run = start(Arc::clone(&client), &workspace, &config);

        assert_eq!(run.result.join().expect("join"), Ok("end_turn".to_string()));
        let events = run.events.lock().expect("events");
        assert!(!events
            .iter()
            .any(|event| matches!(event, UiEvent::Permission { .. })));
        assert!(client.tool_output().expect("output").contains("alpha"));
    }

    #[test]
    fn engine_events_map_to_ui_events() {
        assert_eq!(
            ui_event(AcpEvent::Text("hi".into())),
            Some(UiEvent::Text { text: "hi".into() })
        );
        assert_eq!(
            ui_event(AcpEvent::Err("boom".into())),
            Some(UiEvent::Error {
                message: "boom".into()
            })
        );
        assert_eq!(ui_event(AcpEvent::Thought("hmm".into())), None);
        let json = serde_json::to_value(UiEvent::Permission {
            id: "c".into(),
            title: "write".into(),
            action: "note.txt".into(),
            reason: String::new(),
        })
        .expect("json");
        assert_eq!(
            json,
            serde_json::json!({"kind": "permission", "id": "c", "title": "write", "action": "note.txt", "reason": ""})
        );
    }

    #[test]
    fn answers_parse_and_junk_is_refused() {
        assert_eq!(parse_answer("allow"), Ok(PermAnswer::Allow));
        assert_eq!(parse_answer("always"), Ok(PermAnswer::Always));
        assert_eq!(parse_answer("deny"), Ok(PermAnswer::Deny));
        assert_eq!(parse_answer("yes"), Err("unknown answer: yes".to_string()));
    }

    #[test]
    fn the_gate_asks_and_keeps_computer_use_off() {
        assert_eq!(
            shell_gate(),
            Gate {
                mode: PermMode::Ask,
                readonly_session: false,
                attended: true,
                desktop: false,
            }
        );
    }
}
