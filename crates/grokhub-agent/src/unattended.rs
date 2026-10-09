//! Sync unattended runs. One prompt, no permission card, no human wait.
//! Ask denies every non-read-only tool. Auto uses the Phase 5 judge and fails closed.
//! Halt and quit cancel the same token the attended engine watches.

use std::path::PathBuf;
use std::sync::Arc;

use grokhub_acp::{AcpEvent, GrokUsage};

use crate::gate::{ClosedPermits, Gate, PermMode};
use crate::tools::DesktopOps;
use crate::{AuthKind, CancelToken, Engine, HaltCheck, ModelClient, Usage};

pub struct UnattendedRun {
    pub client: Arc<dyn ModelClient + Send + Sync>,
    pub workspace: PathBuf,
    pub model: String,
    pub effort: Option<String>,
    pub system: String,
    pub session_id: String,
    pub auth_kind: AuthKind,
    pub prompt: String,
    pub image: Option<String>,
    pub mode: PermMode,
    pub readonly_session: bool,
    pub desktop: bool,
    pub cancel: CancelToken,
    pub halt: Box<dyn HaltCheck + Send>,
    pub desktop_ops: Option<Box<dyn DesktopOps>>,
    pub imagine_bearer: String,
    /// Resume a native session file when one already exists for `session_id`.
    pub resume: bool,
}

pub struct UnattendedDone {
    pub text: String,
    pub thought: String,
    pub stop_reason: String,
    pub error: String,
    pub usage: Usage,
    pub meter: String,
    pub context_tokens_used: u64,
    pub context_window_tokens: u64,
    pub session_id: String,
    pub permission_cards: u32,
    pub elicit_cards: u32,
}

/// Run `prompt` on a fresh [`crate::NativeEngine`] with an unattended gate.
/// The gate's attended flag is forced off. Closed permits deny immediately,
/// so a bug that still asks cannot wait. Permission and elicit events are counted
/// and dropped. A halt or cancel also shuts MCP servers this process started.
/// The route class unattended runs log (not in the §14.3 table yet, so the
/// shadow route keeps today's effort for it).
pub const UNATTENDED_CLASS: &str = "background:unattended";

pub fn run_unattended(spec: UnattendedRun) -> UnattendedDone {
    // Router R0: scheduled runs log their shadow routes under their own class.
    let _class = crate::route::live::ClassScope::enter(UNATTENDED_CLASS);
    let session = spec.session_id.clone();
    let cancel = spec.cancel.clone();
    let hub = crate::tasks::hub_for(&session);
    crate::tasks::watch_cancel(&session, cancel.clone());
    let _guard = crate::session::attach_run(&session, cancel.clone());
    if hub.is_halted() || cancel.is_cancelled() || spec.halt.halted() {
        crate::mcp::shutdown_all();
        drop(_guard);
        crate::tasks::forget_session(&session);
        return empty_done(session, "halted");
    }
    let gate = Gate {
        mode: spec.mode,
        readonly_session: spec.readonly_session,
        attended: false,
        desktop: spec.desktop,
    };
    let mut engine = crate::NativeEngine::new(crate::EngineParts {
        client: spec.client,
        workspace: spec.workspace,
        model: spec.model,
        effort: spec.effort,
        system: spec.system,
        conversation_id: session.clone(),
        auth_kind: spec.auth_kind,
        // No turn cap: a night job runs until it answers, Halt or Stop.
        max_turns: 0,
        cancel,
        steer: crate::SteerQueue::new(),
        halt: spec.halt,
        gate,
        desktop: spec.desktop_ops,
        permits: Arc::new(ClosedPermits),
    });
    engine.set_imagine_bearer(&spec.imagine_bearer);
    engine.set_reopen_tasks(false);
    if spec.resume {
        if let Ok(info) = crate::session::load_session(&session) {
            engine.resume(info.input(), info.usage);
        }
    }
    let mut done = empty_done(session.clone(), "end_turn");
    let image = spec.image;
    let _ = engine.prompt(spec.prompt.as_str(), image.as_deref(), &mut |ev| match ev {
        AcpEvent::Text(text) => done.text.push_str(&text),
        AcpEvent::Thought(text) => done.thought.push_str(&text),
        AcpEvent::Permission(_) => done.permission_cards = done.permission_cards.saturating_add(1),
        AcpEvent::Elicit(_) => done.elicit_cards = done.elicit_cards.saturating_add(1),
        AcpEvent::Err(err) => done.error = err,
        AcpEvent::Usage(usage) => take_usage(&mut done, &usage),
        AcpEvent::Done { stop_reason } => done.stop_reason = stop_reason,
        _ => {}
    });
    if done.stop_reason == "halted" || done.stop_reason == "cancelled" {
        crate::mcp::shutdown_all();
    }
    drop(_guard);
    crate::tasks::forget_session(&session);
    done
}

fn empty_done(session_id: String, stop_reason: &str) -> UnattendedDone {
    UnattendedDone {
        text: String::new(),
        thought: String::new(),
        stop_reason: stop_reason.to_string(),
        error: String::new(),
        usage: Usage::default(),
        meter: String::new(),
        context_tokens_used: 0,
        context_window_tokens: 0,
        session_id,
        permission_cards: 0,
        elicit_cards: 0,
    }
}

fn take_usage(done: &mut UnattendedDone, usage: &GrokUsage) {
    done.usage.input_tokens = usage.input_tokens;
    done.usage.output_tokens = usage.output_tokens;
    done.usage.reasoning_tokens = usage.reasoning_tokens;
    done.usage.cost_in_usd_ticks = usage.cost_in_usd_ticks;
    if !usage.meter.is_empty() {
        done.meter = usage.meter.clone();
    }
    done.context_tokens_used = usage.context_tokens_used;
    done.context_window_tokens = usage.context_window_tokens;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::Mutex;
    use std::time::Duration;

    use serde_json::Value;

    use crate::client::{
        ContentPart, FunctionCall, InputItem, ResponsesRequest, StreamEvent, TurnOutput,
    };
    use crate::gate::unattended_deny;
    use crate::tools::{DesktopOps, ToolOutput};
    use crate::{ClientError, ModelClient};

    struct NoHalt;
    impl HaltCheck for NoHalt {
        fn halted(&self) -> bool {
            false
        }
    }

    struct YesHalt;
    impl HaltCheck for YesHalt {
        fn halted(&self) -> bool {
            true
        }
    }

    struct SpyDesk {
        calls: Arc<AtomicUsize>,
    }

    impl DesktopOps for SpyDesk {
        fn halted(&self) -> bool {
            false
        }
        fn locked(&self) -> bool {
            false
        }
        fn call(&self, _name: &str, _args: &Value) -> ToolOutput {
            self.calls.fetch_add(1, Ordering::SeqCst);
            ToolOutput::ok("clicked")
        }
    }

    fn blob(req: &ResponsesRequest) -> String {
        let mut out = String::new();
        for item in &req.input {
            match item {
                InputItem::Message { content, .. } => {
                    for part in content {
                        if let ContentPart::InputText(text) = part {
                            out.push_str(text);
                            out.push('\n');
                        }
                    }
                }
                InputItem::FunctionCallOutput { output, .. } => {
                    out.push_str(output);
                    out.push('\n');
                }
                InputItem::FunctionCall {
                    name, arguments, ..
                } => {
                    out.push_str(name);
                    out.push(' ');
                    out.push_str(arguments);
                    out.push('\n');
                }
            }
        }
        out
    }

    fn is_judge(req: &ResponsesRequest) -> bool {
        !req.hosted_search && req.tools.is_empty() && blob(req).contains("## Proposed action")
    }

    struct Matrix {
        n: AtomicUsize,
        saw_judge: AtomicBool,
        final_blob: Mutex<String>,
    }

    impl ModelClient for Matrix {
        fn stream(
            &self,
            req: &ResponsesRequest,
            _cancel: &CancelToken,
            sink: &mut dyn FnMut(StreamEvent),
        ) -> Result<TurnOutput, ClientError> {
            if is_judge(req) {
                self.saw_judge.store(true, Ordering::SeqCst);
                return Ok(TurnOutput {
                    text: "banana".into(),
                    reasoning: String::new(),
                    calls: Vec::new(),
                    usage: Usage::default(),
                });
            }
            let n = self.n.fetch_add(1, Ordering::SeqCst);
            if n == 0 {
                return Ok(TurnOutput {
                    text: String::new(),
                    reasoning: String::new(),
                    calls: vec![
                        FunctionCall {
                            call_id: "r".into(),
                            name: "read_file".into(),
                            arguments: r#"{"target_file":"alpha.txt"}"#.into(),
                        },
                        FunctionCall {
                            call_id: "w".into(),
                            name: "write".into(),
                            arguments: r#"{"path":"nope.txt","content":"secret"}"#.into(),
                        },
                        FunctionCall {
                            call_id: "s".into(),
                            name: "run_terminal_command".into(),
                            arguments: r#"{"command":"touch shell-ran.txt"}"#.into(),
                        },
                        FunctionCall {
                            call_id: "d".into(),
                            name: "screenshot".into(),
                            arguments: "{}".into(),
                        },
                        FunctionCall {
                            call_id: "f".into(),
                            name: "web_fetch".into(),
                            arguments: r#"{"url":"https://example.com"}"#.into(),
                        },
                        FunctionCall {
                            call_id: "m".into(),
                            name: "image_generate".into(),
                            arguments: r#"{"prompt":"a cat"}"#.into(),
                        },
                        FunctionCall {
                            call_id: "p".into(),
                            name: "docs__lookup".into(),
                            arguments: "{}".into(),
                        },
                    ],
                    usage: Usage::default(),
                });
            }
            *self
                .final_blob
                .lock()
                .unwrap_or_else(|err| err.into_inner()) = blob(req);
            sink(StreamEvent::TextDelta("ask matrix done".into()));
            Ok(TurnOutput {
                text: "ask matrix done".into(),
                reasoning: String::new(),
                calls: Vec::new(),
                usage: Usage {
                    input_tokens: 2,
                    output_tokens: 2,
                    reasoning_tokens: 0,
                    cost_in_usd_ticks: 1,
                    cached_tokens: 0,
                },
            })
        }
    }

    fn workspace(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "gh-unattended-{label}-{}-{}",
            std::process::id(),
            crate::perm::config_dir().display().to_string().len()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn run_in(
        dir: &std::path::Path,
        spec_mode: PermMode,
        client: Arc<dyn ModelClient + Send + Sync>,
        desk_calls: Arc<AtomicUsize>,
    ) -> UnattendedDone {
        let _guard = crate::perm::ConfigGuard::set(dir);
        run_unattended(UnattendedRun {
            client,
            workspace: dir.to_path_buf(),
            model: "grok-4.7".into(),
            effort: Some("low".into()),
            system: String::new(),
            session_id: format!("native-u-{spec_mode:?}"),
            auth_kind: AuthKind::ApiKey,
            prompt: "do the chore".into(),
            image: None,
            mode: spec_mode,
            readonly_session: false,
            desktop: true,
            cancel: CancelToken::new(),
            halt: Box::new(NoHalt),
            desktop_ops: Some(Box::new(SpyDesk { calls: desk_calls })),
            imagine_bearer: String::new(),
            resume: false,
        })
    }

    #[test]
    fn ask_denies_non_readonly_without_a_card_and_allows_read() {
        let dir = workspace("ask");
        std::fs::write(dir.join("alpha.txt"), "alpha-line\n").unwrap();
        let desk = Arc::new(AtomicUsize::new(0));
        let client = Arc::new(Matrix {
            n: AtomicUsize::new(0),
            saw_judge: AtomicBool::new(false),
            final_blob: Mutex::new(String::new()),
        });
        let shared: Arc<dyn ModelClient + Send + Sync> = client.clone();
        let done = run_in(&dir, PermMode::Ask, shared, Arc::clone(&desk));
        let seen = client
            .final_blob
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .clone();
        assert_eq!(done.permission_cards, 0);
        assert_eq!(done.elicit_cards, 0);
        assert_eq!(done.stop_reason, "end_turn");
        assert!(
            seen.contains("alpha-line"),
            "read must reach the model: {seen}"
        );
        assert!(seen.contains(&unattended_deny("write")), "{seen}");
        assert!(
            seen.contains(&unattended_deny("run_terminal_command")),
            "{seen}"
        );
        assert!(seen.contains(&unattended_deny("screenshot")), "{seen}");
        assert!(seen.contains(&unattended_deny("web_fetch")), "{seen}");
        assert!(seen.contains(&unattended_deny("image_generate")), "{seen}");
        assert!(
            seen.contains("docs__lookup")
                && (seen.contains("unknown tool") || seen.contains("deny rule")),
            "{seen}"
        );
        assert!(!dir.join("nope.txt").exists());
        assert!(!dir.join("shell-ran.txt").exists());
        assert_eq!(desk.load(Ordering::SeqCst), 0);
        assert!(!client.saw_judge.load(Ordering::SeqCst));
        let _ = std::fs::remove_dir_all(&dir);
    }

    struct JudgeWrite {
        n: AtomicUsize,
        verdict: String,
        saw_judge: AtomicBool,
        judge_hosted_off: AtomicBool,
        judge_tools_empty: AtomicBool,
        saw_proposal: AtomicBool,
        final_blob: Mutex<String>,
    }

    impl ModelClient for JudgeWrite {
        fn stream(
            &self,
            req: &ResponsesRequest,
            _cancel: &CancelToken,
            sink: &mut dyn FnMut(StreamEvent),
        ) -> Result<TurnOutput, ClientError> {
            if is_judge(req) {
                self.saw_judge.store(true, Ordering::SeqCst);
                self.judge_hosted_off
                    .store(!req.hosted_search, Ordering::SeqCst);
                self.judge_tools_empty
                    .store(req.tools.is_empty(), Ordering::SeqCst);
                self.saw_proposal
                    .store(blob(req).contains("## Proposed action"), Ordering::SeqCst);
                return Ok(TurnOutput {
                    text: self.verdict.clone(),
                    reasoning: String::new(),
                    calls: Vec::new(),
                    usage: Usage::default(),
                });
            }
            let n = self.n.fetch_add(1, Ordering::SeqCst);
            if n == 0 {
                return Ok(TurnOutput {
                    text: String::new(),
                    reasoning: String::new(),
                    calls: vec![FunctionCall {
                        call_id: "w".into(),
                        name: "write".into(),
                        arguments: r#"{"path":"note.txt","content":"yes"}"#.into(),
                    }],
                    usage: Usage::default(),
                });
            }
            *self
                .final_blob
                .lock()
                .unwrap_or_else(|err| err.into_inner()) = blob(req);
            sink(StreamEvent::TextDelta("auto done".into()));
            Ok(TurnOutput {
                text: "auto done".into(),
                reasoning: String::new(),
                calls: Vec::new(),
                usage: Usage::default(),
            })
        }
    }

    #[test]
    fn auto_garbage_fails_closed_and_allow_runs_the_write() {
        let dir = workspace("auto-deny");
        let client = Arc::new(JudgeWrite {
            n: AtomicUsize::new(0),
            verdict: "banana".into(),
            saw_judge: AtomicBool::new(false),
            judge_hosted_off: AtomicBool::new(false),
            judge_tools_empty: AtomicBool::new(false),
            saw_proposal: AtomicBool::new(false),
            final_blob: Mutex::new(String::new()),
        });
        let shared: Arc<dyn ModelClient + Send + Sync> = client.clone();
        let done = run_in(&dir, PermMode::Auto, shared, Arc::new(AtomicUsize::new(0)));
        let seen = client
            .final_blob
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .clone();
        assert!(client.saw_judge.load(Ordering::SeqCst));
        assert!(client.judge_hosted_off.load(Ordering::SeqCst));
        assert!(client.judge_tools_empty.load(Ordering::SeqCst));
        assert!(client.saw_proposal.load(Ordering::SeqCst));
        assert!(seen.contains(&unattended_deny("write")), "{seen}");
        assert!(!dir.join("note.txt").exists());
        assert_eq!(done.permission_cards, 0);
        assert_eq!(done.elicit_cards, 0);
        let _ = std::fs::remove_dir_all(&dir);

        let dir = workspace("auto-allow");
        let client = Arc::new(JudgeWrite {
            n: AtomicUsize::new(0),
            verdict: r#"{"verdict":"allow","reason":"ok"}"#.into(),
            saw_judge: AtomicBool::new(false),
            judge_hosted_off: AtomicBool::new(false),
            judge_tools_empty: AtomicBool::new(false),
            saw_proposal: AtomicBool::new(false),
            final_blob: Mutex::new(String::new()),
        });
        let shared: Arc<dyn ModelClient + Send + Sync> = client.clone();
        let _done = run_in(&dir, PermMode::Auto, shared, Arc::new(AtomicUsize::new(0)));
        assert!(client.saw_judge.load(Ordering::SeqCst));
        assert_eq!(
            std::fs::read_to_string(dir.join("note.txt")).unwrap(),
            "yes"
        );
        assert_eq!(_done.permission_cards, 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    struct Blocker {
        started: Arc<AtomicBool>,
        calls: AtomicUsize,
    }

    impl ModelClient for Blocker {
        fn stream(
            &self,
            _req: &ResponsesRequest,
            cancel: &CancelToken,
            _sink: &mut dyn FnMut(StreamEvent),
        ) -> Result<TurnOutput, ClientError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.started.store(true, Ordering::SeqCst);
            let start = std::time::Instant::now();
            loop {
                if cancel.is_cancelled() {
                    return Err(ClientError::Cancelled);
                }
                if start.elapsed() > Duration::from_secs(5) {
                    return Err(ClientError::Protocol("halt did not arrive".into()));
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        }
    }

    struct SpeakUsage;
    impl ModelClient for SpeakUsage {
        fn stream(
            &self,
            _req: &ResponsesRequest,
            _cancel: &CancelToken,
            sink: &mut dyn FnMut(StreamEvent),
        ) -> Result<TurnOutput, ClientError> {
            sink(StreamEvent::TextDelta("usage report".into()));
            Ok(TurnOutput {
                text: "usage report".into(),
                reasoning: String::new(),
                calls: Vec::new(),
                usage: Usage {
                    input_tokens: 11,
                    output_tokens: 7,
                    reasoning_tokens: 3,
                    cost_in_usd_ticks: 42,
                    cached_tokens: 0,
                },
            })
        }
    }

    struct CountCalls {
        n: AtomicUsize,
    }
    impl ModelClient for CountCalls {
        fn stream(
            &self,
            _req: &ResponsesRequest,
            _cancel: &CancelToken,
            _sink: &mut dyn FnMut(StreamEvent),
        ) -> Result<TurnOutput, ClientError> {
            self.n.fetch_add(1, Ordering::SeqCst);
            Ok(TurnOutput {
                text: "no".into(),
                reasoning: String::new(),
                calls: Vec::new(),
                usage: Usage::default(),
            })
        }
    }

    #[test]
    fn halt_before_prompt_skips_the_model_and_halt_during_stops_the_run() {
        let dir = workspace("halt-now");
        let _guard = crate::perm::ConfigGuard::set(&dir);
        let calls = Arc::new(CountCalls {
            n: AtomicUsize::new(0),
        });
        let client: Arc<dyn ModelClient + Send + Sync> = calls.clone();
        let done = run_unattended(UnattendedRun {
            client,
            workspace: dir.clone(),
            model: "grok-4.7".into(),
            effort: Some("low".into()),
            system: String::new(),
            session_id: "native-u-halt-now".into(),
            auth_kind: AuthKind::ApiKey,
            prompt: "go".into(),
            image: None,
            mode: PermMode::Ask,
            readonly_session: false,
            desktop: false,
            cancel: CancelToken::new(),
            halt: Box::new(YesHalt),
            desktop_ops: None,
            imagine_bearer: String::new(),
            resume: false,
        });
        assert_eq!(calls.n.load(Ordering::SeqCst), 0);
        assert_eq!(done.stop_reason, "halted");
        assert_eq!(done.permission_cards, 0);
        assert_eq!(done.elicit_cards, 0);

        let started = Arc::new(AtomicBool::new(false));
        let blocker = Arc::new(Blocker {
            started: Arc::clone(&started),
            calls: AtomicUsize::new(0),
        });
        let client: Arc<dyn ModelClient + Send + Sync> = blocker.clone();
        let session = "native-u-halt-live".to_string();
        let session_halt = session.clone();
        let started_halt = Arc::clone(&started);
        std::thread::spawn(move || {
            let start = std::time::Instant::now();
            while !started_halt.load(Ordering::SeqCst) && start.elapsed() < Duration::from_secs(2) {
                std::thread::sleep(Duration::from_millis(5));
            }
            crate::tasks::halt_session(&session_halt);
        });
        let done = run_unattended(UnattendedRun {
            client,
            workspace: dir.clone(),
            model: "grok-4.7".into(),
            effort: Some("low".into()),
            system: String::new(),
            session_id: session,
            auth_kind: AuthKind::ApiKey,
            prompt: "go".into(),
            image: None,
            mode: PermMode::Ask,
            readonly_session: false,
            desktop: false,
            cancel: CancelToken::new(),
            halt: Box::new(NoHalt),
            desktop_ops: None,
            imagine_bearer: String::new(),
            resume: false,
        });
        assert!(started.load(Ordering::SeqCst));
        assert!(
            done.stop_reason == "cancelled" || done.stop_reason == "halted",
            "{}",
            done.stop_reason
        );
        assert!(
            done.error.is_empty() || done.stop_reason != "error",
            "{}",
            done.error
        );
        assert_eq!(done.permission_cards, 0);
        assert_eq!(done.elicit_cards, 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn usage_is_recorded_on_the_session_file() {
        let dir = workspace("usage");
        let _guard = crate::perm::ConfigGuard::set(&dir);
        let work = dir.join("work");
        std::fs::create_dir_all(&work).unwrap();
        let client: Arc<dyn ModelClient + Send + Sync> = Arc::new(SpeakUsage);
        let done = run_unattended(UnattendedRun {
            client,
            workspace: work,
            model: "grok-4.7".into(),
            effort: Some("low".into()),
            system: String::new(),
            session_id: "native-u-usage".into(),
            auth_kind: AuthKind::ApiKey,
            prompt: "report the spend".into(),
            image: None,
            mode: PermMode::Ask,
            readonly_session: false,
            desktop: false,
            cancel: CancelToken::new(),
            halt: Box::new(NoHalt),
            desktop_ops: None,
            imagine_bearer: String::new(),
            resume: false,
        });
        assert_eq!(done.usage.input_tokens, 11);
        assert_eq!(done.usage.output_tokens, 7);
        assert_eq!(done.usage.reasoning_tokens, 3);
        assert_eq!(done.usage.cost_in_usd_ticks, 42);
        assert_eq!(done.meter, "API credits");
        let info = crate::session::load_session("native-u-usage").expect("session");
        assert_eq!(info.usage.input_tokens, 11);
        assert_eq!(info.usage.output_tokens, 7);
        assert_eq!(info.usage.reasoning_tokens, 3);
        assert_eq!(info.usage.cost_in_usd_ticks, 42);
        assert_eq!(info.meter, "API credits");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `tools` turns of `list_dir`, then a reply. `same` repeats one call.
    struct ToolTurns {
        tools: usize,
        same: bool,
        n: AtomicUsize,
        last_blob: Mutex<String>,
    }

    impl ModelClient for ToolTurns {
        fn stream(
            &self,
            req: &ResponsesRequest,
            _cancel: &CancelToken,
            sink: &mut dyn FnMut(StreamEvent),
        ) -> Result<TurnOutput, ClientError> {
            let n = self.n.fetch_add(1, Ordering::SeqCst);
            *self.last_blob.lock().unwrap() = blob(req);
            if n < self.tools {
                let path = if self.same { "p".to_string() } else { format!("p{n}") };
                return Ok(TurnOutput {
                    text: String::new(),
                    reasoning: String::new(),
                    calls: vec![FunctionCall {
                        call_id: format!("t{n}"),
                        name: "list_dir".into(),
                        arguments: format!(r#"{{"path":"{path}"}}"#),
                    }],
                    usage: Usage::default(),
                });
            }
            sink(StreamEvent::TextDelta("chore done".into()));
            Ok(TurnOutput {
                text: "chore done".into(),
                reasoning: String::new(),
                calls: Vec::new(),
                usage: Usage::default(),
            })
        }
    }

    fn run_tool_turns(label: &str, tools: usize, same: bool) -> (UnattendedDone, Arc<ToolTurns>) {
        let dir = workspace(label);
        let client = Arc::new(ToolTurns {
            tools,
            same,
            n: AtomicUsize::new(0),
            last_blob: Mutex::new(String::new()),
        });
        let done = run_in(&dir, PermMode::Ask, client.clone(), Arc::new(AtomicUsize::new(0)));
        let _ = std::fs::remove_dir_all(&dir);
        (done, client)
    }

    #[test]
    fn a_night_job_runs_past_fifty_turns_to_its_answer() {
        let (done, client) = run_tool_turns("long", 70, false);
        assert_eq!(done.stop_reason, "end_turn");
        assert_eq!(done.text, "chore done");
        assert_eq!(client.n.load(Ordering::SeqCst), 71);
        assert_eq!(done.permission_cards, 0);
    }

    #[test]
    fn six_identical_calls_get_the_replan_note_and_still_end_with_no_card() {
        let (done, client) = run_tool_turns("repeat", 6, true);
        assert_eq!(done.stop_reason, "end_turn");
        assert_eq!(done.text, "chore done");
        assert_eq!(client.n.load(Ordering::SeqCst), 7);
        let last = client.last_blob.lock().unwrap().clone();
        assert_eq!(last.matches(crate::REPEAT_REPLAN).count(), 4, "calls 3 to 6 re-plan: {last}");
        assert_eq!((done.permission_cards, done.elicit_cards), (0, 0));
    }
}
