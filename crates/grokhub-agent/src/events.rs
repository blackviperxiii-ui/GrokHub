//! Adapt loop events into the cabin's existing ACP events.

use std::path::PathBuf;

use grokhub_acp::{AcpEvent, GrokUsage, PermissionAsk, ToolCard};

use crate::gate::{Gate, PermitWait};
use crate::tools::DesktopOps;
use crate::{
    run_loop, AuthKind, CancelToken, HaltCheck, InputItem, LoopEvent, LoopIn, ModelClient, SteerQueue,
    StopReason, Usage, DEFAULT_MAX_TURNS,
};

pub trait Engine {
    fn prompt(&mut self, text: &str, image: Option<&str>, emit: &mut dyn FnMut(AcpEvent)) -> Result<(), String>;
    fn cancel(&self);
}

pub struct NativeEngine {
    client: Box<dyn ModelClient + Send>,
    workspace: PathBuf,
    model: String,
    effort: Option<String>,
    system: String,
    conversation_id: String,
    auth_kind: AuthKind,
    max_turns: u32,
    cancel: CancelToken,
    steer: SteerQueue,
    halt: Box<dyn HaltCheck + Send>,
    gate: Gate,
    desktop: Option<Box<dyn DesktopOps>>,
    permits: Box<dyn PermitWait + Send>,
    history: Vec<InputItem>,
    usage: Usage,
    /// Context window for the meter and the 85% auto-compact gate.
    context_length: u64,
    /// The main engine clears a previous Halt. A `/bg` engine does not.
    reopen_tasks: bool,
}

pub struct EngineParts {
    pub client: Box<dyn ModelClient + Send>,
    pub workspace: PathBuf,
    pub model: String,
    pub effort: Option<String>,
    pub system: String,
    pub conversation_id: String,
    pub auth_kind: AuthKind,
    pub max_turns: u32,
    pub cancel: CancelToken,
    pub steer: SteerQueue,
    pub halt: Box<dyn HaltCheck + Send>,
    pub gate: Gate,
    pub desktop: Option<Box<dyn DesktopOps>>,
    pub permits: Box<dyn PermitWait + Send>,
}

impl NativeEngine {
    pub fn new(parts: EngineParts) -> Self {
        let context_length = crate::models::context_length_for(&parts.model, &[]);
        Self {
            client: parts.client,
            workspace: parts.workspace,
            model: parts.model,
            effort: parts.effort,
            system: parts.system,
            conversation_id: parts.conversation_id,
            auth_kind: parts.auth_kind,
            max_turns: if parts.max_turns == 0 { DEFAULT_MAX_TURNS } else { parts.max_turns },
            cancel: parts.cancel,
            steer: parts.steer,
            halt: parts.halt,
            gate: parts.gate,
            desktop: parts.desktop,
            permits: parts.permits,
            history: Vec::new(),
            usage: Usage::default(),
            context_length,
            reopen_tasks: true,
        }
    }

    /// `/bg` leaves this false so a Halt that landed first is not cleared.
    pub fn set_reopen_tasks(&mut self, reopen: bool) {
        self.reopen_tasks = reopen;
    }

    pub fn set_workspace(&mut self, workspace: PathBuf) {
        self.workspace = workspace;
    }

    pub fn set_gate(&mut self, gate: Gate) {
        self.gate = gate;
    }

    pub fn steer(&self) -> SteerQueue {
        self.steer.clone()
    }

    pub fn set_route(
        &mut self,
        client: Box<dyn ModelClient + Send>,
        model: String,
        effort: Option<String>,
        system: String,
        auth_kind: AuthKind,
    ) {
        self.context_length = crate::models::context_length_for(&model, &[]);
        self.client = client;
        self.model = model;
        self.effort = effort;
        self.system = system;
        self.auth_kind = auth_kind;
    }

    pub fn set_halt(&mut self, halt: Box<dyn HaltCheck + Send>) {
        self.halt = halt;
    }

    /// Replace the in-memory transcript with a session file. The next prompt continues it.
    pub fn resume(&mut self, items: Vec<InputItem>, usage: Usage) {
        self.history = items;
        self.usage = usage;
    }
}

impl Engine for NativeEngine {
    fn cancel(&self) {
        self.cancel.cancel();
    }

    fn prompt(
        &mut self,
        text: &str,
        image: Option<&str>,
        emit: &mut dyn FnMut(AcpEvent),
    ) -> Result<(), String> {
        let hub = crate::tasks::hub_for(&self.conversation_id);
        if !self.reopen_tasks && (hub.is_halted() || self.cancel.is_cancelled()) {
            emit(AcpEvent::Done {
                stop_reason: "halted".into(),
            });
            return Ok(());
        }
        self.cancel.reset();
        if self.reopen_tasks {
            hub.reopen();
        }
        if image.is_none() && crate::compact::is_manual_compact_command(text) {
            return self.compact_now(emit);
        }
        let policy = crate::perm::Policy::load(&self.workspace);
        let input = LoopIn {
            client: self.client.as_ref(),
            workspace: &self.workspace,
            model: &self.model,
            effort: self.effort.as_deref(),
            system: &self.system,
            conversation_id: &self.conversation_id,
            max_turns: self.max_turns,
            usage_base: self.usage.clone(),
            cancel: &self.cancel,
            steer: &self.steer,
            halt: self.halt.as_ref(),
            gate: self.gate,
            desktop: self.desktop.as_deref(),
            permits: self.permits.as_ref(),
            perms: Some(&policy),
            context_length: self.context_length,
            tasks: Some(hub),
        };
        let session = self.conversation_id.clone();
        let cwd = self.workspace.display().to_string();
        let model = self.model.clone();
        let kind = self.auth_kind;
        let meter = kind.meter();
        let before_len = self.history.len();
        let before_usage = self.usage.clone();
        let mut meter_used = crate::compact::estimate_input_tokens(&self.history);
        let mut meter_limit = self.context_length;
        let out = run_loop(&input, &mut self.history, text, image, &mut |ev| match ev {
            LoopEvent::Meter { used, limit } => {
                meter_used = used;
                meter_limit = limit;
            }
            LoopEvent::Compact {
                started,
                usage,
                error,
            } => {
                emit(AcpEvent::Compact {
                    started,
                    usage: grok_usage(&usage, kind, meter_used, meter_limit),
                    error,
                });
            }
            other => {
                if let Some(acp) = to_acp(other, kind, &session, meter_used, meter_limit) {
                    emit(acp);
                }
            }
        });
        self.usage = out.usage.clone();
        let delta = self.usage.saturating_delta(&before_usage);
        if out.compacted {
            let _ = crate::session::record_compaction(
                &session,
                &cwd,
                &model,
                &self.history,
                &delta,
                meter,
            );
        } else {
            let fresh = self.history.get(before_len..).unwrap_or(&[]).to_vec();
            let _ =
                crate::session::record_turn(&session, &cwd, &model, &fresh, &delta, meter, text);
        }
        let stop = match out.stop {
            StopReason::EndTurn => "end_turn".to_string(),
            StopReason::Cancelled => "cancelled".to_string(),
            StopReason::Halted => "halted".to_string(),
            StopReason::MaxTurns => "max_turns".to_string(),
            StopReason::RepeatedCall => "repeated_call".to_string(),
            StopReason::Error(message) => {
                emit(AcpEvent::Err(message));
                "error".into()
            }
        };
        let used = crate::compact::estimate_input_tokens(&self.history);
        emit(grok_usage_event(
            &self.usage,
            self.auth_kind,
            used,
            self.context_length,
        ));
        emit(AcpEvent::Done { stop_reason: stop });
        Ok(())
    }
}

impl NativeEngine {
    /// Manual `/compact`. The command is not stored as a user turn.
    /// A failure or cancel leaves `history` as it was.
    fn compact_now(&mut self, emit: &mut dyn FnMut(AcpEvent)) -> Result<(), String> {
        let session = self.conversation_id.clone();
        let cwd = self.workspace.display().to_string();
        let model = self.model.clone();
        let kind = self.auth_kind;
        let meter = kind.meter();
        let limit = self.context_length;
        let before = self.usage.clone();
        let used = crate::compact::estimate_input_tokens(&self.history);
        emit(AcpEvent::Compact {
            started: true,
            usage: grok_usage(&self.usage, kind, used, limit),
            error: None,
        });
        match crate::compact::compact_transcript(
            self.client.as_ref(),
            &self.cancel,
            &self.model,
            self.effort.as_deref(),
            &self.conversation_id,
            &mut self.history,
        ) {
            Ok(extra) => {
                self.usage.add(&extra);
                let delta = self.usage.saturating_delta(&before);
                let _ = crate::session::record_compaction(
                    &session,
                    &cwd,
                    &model,
                    &self.history,
                    &delta,
                    meter,
                );
                let used = crate::compact::estimate_input_tokens(&self.history);
                let usage = grok_usage(&self.usage, kind, used, limit);
                emit(AcpEvent::Compact {
                    started: false,
                    usage: usage.clone(),
                    error: None,
                });
                emit(AcpEvent::Usage(usage));
                emit(AcpEvent::Done {
                    stop_reason: "end_turn".into(),
                });
            }
            Err(crate::compact::CompactError::Cancelled) => {
                emit(AcpEvent::Compact {
                    started: false,
                    usage: grok_usage(&self.usage, kind, used, limit),
                    error: Some("cancelled".into()),
                });
                emit(AcpEvent::Done {
                    stop_reason: "cancelled".into(),
                });
            }
            Err(crate::compact::CompactError::Failed(message)) => {
                emit(AcpEvent::Compact {
                    started: false,
                    usage: grok_usage(&self.usage, kind, used, limit),
                    error: Some(message),
                });
                emit(AcpEvent::Done {
                    stop_reason: "end_turn".into(),
                });
            }
        }
        Ok(())
    }
}

fn to_acp(ev: LoopEvent, kind: AuthKind, session: &str, used: u64, limit: u64) -> Option<AcpEvent> {
    Some(match ev {
        LoopEvent::Text(text) => AcpEvent::Text(text),
        LoopEvent::Thought(text) => AcpEvent::Thought(text),
        LoopEvent::Tool { id, name, status, detail, image } => AcpEvent::Tool(ToolCard {
            id,
            title: name.clone(),
            kind: name,
            status,
            detail,
            diff: String::new(),
            image_data_url: image,
        }),
        LoopEvent::Usage(usage) => grok_usage_event(&usage, kind, used, limit),
        LoopEvent::Meter { .. } | LoopEvent::Compact { .. } => return None,
        LoopEvent::Permission {
            id,
            name,
            action,
            reason,
        } => AcpEvent::Permission(PermissionAsk {
            rpc_id: serde_json::Value::String(id.clone()),
            session_id: session.to_string(),
            title: name.clone(),
            tool_call_id: id,
            action,
            reason,
            reject_option: Some("denied".into()),
        }),
    })
}

fn grok_usage(usage: &Usage, kind: AuthKind, used: u64, limit: u64) -> GrokUsage {
    GrokUsage {
        input_tokens: usage.input_tokens,
        output_tokens: usage.output_tokens,
        reasoning_tokens: usage.reasoning_tokens,
        total_tokens: usage
            .input_tokens
            .saturating_add(usage.output_tokens)
            .saturating_add(usage.reasoning_tokens),
        cost_in_usd_ticks: usage.cost_in_usd_ticks,
        context_tokens_used: used,
        context_window_tokens: limit,
        meter: kind.meter().to_string(),
        ..GrokUsage::default()
    }
}

fn grok_usage_event(usage: &Usage, kind: AuthKind, used: u64, limit: u64) -> AcpEvent {
    AcpEvent::Usage(grok_usage(usage, kind, used, limit))
}

pub fn meter_for(kind: AuthKind) -> &'static str {
    kind.meter()
}

/// Halt when a stamp written after `started_ms` is visible to `read`.
pub struct StampHalt<F> {
    pub started_ms: u64,
    pub read: F,
}

impl<F> HaltCheck for StampHalt<F>
where
    F: Fn() -> Option<u64> + Send,
{
    fn halted(&self) -> bool {
        (self.read)().is_some_and(|ms| grokhub_core::stamp_halts(ms, self.started_ms))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;
    use std::sync::Mutex;

    struct Speak;
    impl ModelClient for Speak {
        fn stream(
            &self,
            _req: &crate::ResponsesRequest,
            _cancel: &CancelToken,
            sink: &mut dyn FnMut(crate::StreamEvent),
        ) -> Result<crate::TurnOutput, crate::ClientError> {
            sink(crate::StreamEvent::ReasoningDelta("think".into()));
            sink(crate::StreamEvent::TextDelta("hello".into()));
            Ok(crate::TurnOutput {
                text: "hello".into(),
                reasoning: "think".into(),
                calls: Vec::new(),
                usage: Usage {
                    input_tokens: 3,
                    output_tokens: 1,
                    reasoning_tokens: 1,
                    cost_in_usd_ticks: 9,
                },
            })
        }
    }

    struct NoHalt;
    impl HaltCheck for NoHalt {
        fn halted(&self) -> bool {
            false
        }
    }

    #[test]
    fn native_engine_emits_acp_events_without_permission() {
        let dir = std::env::temp_dir().join(format!("gh-eng-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let _guard = crate::perm::ConfigGuard::set(&dir);
        let mut engine = NativeEngine::new(EngineParts {
            client: Box::new(Speak),
            workspace: dir.clone(),
            model: "grok-4.7".into(),
            effort: None,
            system: "sys".into(),
            conversation_id: "c".into(),
            auth_kind: AuthKind::OAuth,
            max_turns: 2,
            cancel: CancelToken::new(),
            steer: SteerQueue::new(),
            halt: Box::new(NoHalt),
            gate: crate::Gate::phase_readonly(),
            desktop: None,
            permits: Box::new(crate::gate::ClosedPermits),
        });
        let events = Mutex::new(Vec::new());
        engine
            .prompt("hi", None, &mut |ev| events.lock().unwrap().push(ev))
            .unwrap();
        let events = events.into_inner().unwrap();
        assert!(events.iter().any(|ev| matches!(ev, AcpEvent::Thought(t) if t == "think")));
        assert!(events.iter().any(|ev| matches!(ev, AcpEvent::Text(t) if t == "hello")));
        assert!(events.iter().any(|ev| matches!(
            ev,
            AcpEvent::Usage(u) if u.meter == "SuperGrok pool" && u.input_tokens == 3 && u.cost_in_usd_ticks == 9
                && u.context_window_tokens == crate::models::GROK_47_CONTEXT_LENGTH
                && u.context_tokens_used > 0
        )));
        assert!(events
            .iter()
            .any(|ev| matches!(ev, AcpEvent::Done { stop_reason } if stop_reason == "end_turn")));
        assert!(!events
            .iter()
            .any(|ev| matches!(ev, AcpEvent::Permission(_))));
        let loaded = crate::session::load_session("c").expect("turn is on disk");
        let resumed = loaded.input();
        assert!(
            resumed.iter().any(|item| matches!(
                item,
                InputItem::Message { role, content }
                    if role == "user"
                        && content.iter().any(|part| {
                            matches!(part, crate::ContentPart::InputText(text) if text == "hi")
                        })
            )),
            "{resumed:?}"
        );
        assert_eq!(loaded.usage.cost_in_usd_ticks, 9);
        assert_eq!(crate::session::resume_input("c").unwrap(), resumed);
        drop(_guard);
        let _ = std::fs::remove_dir_all(&dir);
    }

    struct Summarize {
        calls: AtomicUsize,
        fail: bool,
    }

    impl ModelClient for Summarize {
        fn stream(
            &self,
            req: &crate::ResponsesRequest,
            _cancel: &CancelToken,
            sink: &mut dyn FnMut(crate::StreamEvent),
        ) -> Result<crate::TurnOutput, crate::ClientError> {
            self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let compact = req.input.iter().any(|item| {
                crate::compact::message_text(item).contains("faithful, concise summary")
            });
            if compact {
                if self.fail {
                    return Err(crate::ClientError::Protocol("disk full".into()));
                }
                sink(crate::StreamEvent::TextDelta("hidden summary delta".into()));
                return Ok(crate::TurnOutput {
                    text: "<summary>kept the plan</summary>".into(),
                    reasoning: String::new(),
                    calls: Vec::new(),
                    usage: Usage {
                        input_tokens: 4,
                        output_tokens: 2,
                        reasoning_tokens: 0,
                        cost_in_usd_ticks: 3,
                    },
                });
            }
            Ok(crate::TurnOutput {
                text: "hello".into(),
                reasoning: String::new(),
                calls: Vec::new(),
                usage: Usage {
                    input_tokens: 3,
                    output_tokens: 1,
                    reasoning_tokens: 0,
                    cost_in_usd_ticks: 1,
                },
            })
        }
    }

    fn engine(
        dir: &std::path::Path,
        id: &str,
        client: Box<dyn ModelClient + Send>,
    ) -> NativeEngine {
        NativeEngine::new(EngineParts {
            client,
            workspace: dir.to_path_buf(),
            model: "grok-4.7".into(),
            effort: None,
            system: String::new(),
            conversation_id: id.into(),
            auth_kind: AuthKind::ApiKey,
            max_turns: 2,
            cancel: CancelToken::new(),
            steer: SteerQueue::new(),
            halt: Box::new(NoHalt),
            gate: crate::Gate::phase_readonly(),
            desktop: None,
            permits: Box::new(crate::gate::ClosedPermits),
        })
    }

    #[test]
    fn manual_compact_records_the_marker_and_hides_summary_deltas() {
        let dir = std::env::temp_dir().join(format!("gh-eng-compact-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let _guard = crate::perm::ConfigGuard::set(&dir);
        let mut engine = engine(
            &dir,
            "native-manual",
            Box::new(Summarize {
                calls: AtomicUsize::new(0),
                fail: false,
            }),
        );
        engine
            .prompt("remember the harbor", None, &mut |_| {})
            .unwrap();
        let events = Mutex::new(Vec::new());
        engine
            .prompt("/compact", None, &mut |ev| events.lock().unwrap().push(ev))
            .unwrap();
        let events = events.into_inner().unwrap();
        assert!(events.iter().any(|ev| matches!(
            ev,
            AcpEvent::Compact {
                started: true,
                error: None,
                ..
            }
        )));
        assert!(events.iter().any(|ev| matches!(
            ev,
            AcpEvent::Compact { started: false, error: None, usage }
                if usage.cost_in_usd_ticks == 4
                    && usage.context_window_tokens == crate::models::GROK_47_CONTEXT_LENGTH
                    && usage.context_tokens_used > 0
        )));
        assert!(!events
            .iter()
            .any(|ev| matches!(ev, AcpEvent::Text(text) if text.contains("hidden summary"))));
        assert!(events
            .iter()
            .any(|ev| matches!(ev, AcpEvent::Done { stop_reason } if stop_reason == "end_turn")));
        let loaded = crate::session::load_session("native-manual").unwrap();
        let input = loaded.input();
        assert!(input
            .iter()
            .any(|item| crate::compact::message_text(item)
                .contains("This session is being continued")));
        assert!(input
            .iter()
            .any(|item| crate::compact::message_text(item) == "remember the harbor"));
        assert!(!input
            .iter()
            .any(|item| crate::compact::message_text(item).contains("/compact")));
        assert_eq!(loaded.usage.cost_in_usd_ticks, 4);
        assert_eq!(loaded.usage.input_tokens, 7);
        drop(_guard);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn failed_manual_compact_keeps_the_transcript() {
        let dir = std::env::temp_dir().join(format!("gh-eng-compact-fail-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let _guard = crate::perm::ConfigGuard::set(&dir);
        let mut engine = engine(
            &dir,
            "native-fail",
            Box::new(Summarize {
                calls: AtomicUsize::new(0),
                fail: true,
            }),
        );
        engine.prompt("keep this", None, &mut |_| {}).unwrap();
        let events = Mutex::new(Vec::new());
        engine
            .prompt("/compact", None, &mut |ev| events.lock().unwrap().push(ev))
            .unwrap();
        let events = events.into_inner().unwrap();
        assert!(events.iter().any(|ev| matches!(
            ev,
            AcpEvent::Compact { error: Some(message), .. } if message.contains("disk full")
        )));
        let loaded = crate::session::load_session("native-fail").unwrap();
        let input = loaded.input();
        assert!(input
            .iter()
            .any(|item| crate::compact::message_text(item) == "keep this"));
        assert!(!input
            .iter()
            .any(|item| crate::compact::message_text(item)
                .contains("This session is being continued")));
        assert_eq!(loaded.usage.cost_in_usd_ticks, 1);
        drop(_guard);
        let _ = std::fs::remove_dir_all(&dir);
    }

    struct RefuseCall;

    impl ModelClient for RefuseCall {
        fn stream(
            &self,
            _req: &crate::ResponsesRequest,
            _cancel: &CancelToken,
            _sink: &mut dyn FnMut(crate::StreamEvent),
        ) -> Result<crate::TurnOutput, crate::ClientError> {
            panic!("empty /compact must not call the model");
        }
    }

    #[test]
    fn empty_manual_compact_does_not_call_the_model() {
        let dir = std::env::temp_dir().join(format!("gh-eng-compact-empty-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let _guard = crate::perm::ConfigGuard::set(&dir);
        let mut engine = engine(&dir, "native-empty", Box::new(RefuseCall));
        let events = Mutex::new(Vec::new());
        engine
            .prompt("/compact", None, &mut |ev| events.lock().unwrap().push(ev))
            .unwrap();
        let events = events.into_inner().unwrap();
        assert!(events.iter().any(|ev| matches!(
            ev,
            AcpEvent::Compact { error: Some(message), .. } if message.contains("nothing to compact")
        )));
        assert!(crate::session::load_session("native-empty").is_err());
        drop(_guard);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn stamp_halt_uses_the_cabin_rule() {
        let halt = StampHalt { started_ms: 10, read: || Some(11u64) };
        assert!(halt.halted());
        let same = StampHalt { started_ms: 10, read: || Some(10u64) };
        assert!(!same.halted());
    }
}
