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
        }
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
        self.client = client;
        self.model = model;
        self.effort = effort;
        self.system = system;
        self.auth_kind = auth_kind;
    }

    pub fn set_halt(&mut self, halt: Box<dyn HaltCheck + Send>) {
        self.halt = halt;
    }
}

impl Engine for NativeEngine {
    fn cancel(&self) {
        self.cancel.cancel();
    }

    fn prompt(&mut self, text: &str, image: Option<&str>, emit: &mut dyn FnMut(AcpEvent)) -> Result<(), String> {
        self.cancel.reset();
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
        };
        let session = self.conversation_id.clone();
        let out = run_loop(&input, &mut self.history, text, image, &mut |ev| {
            if let Some(acp) = to_acp(ev, self.auth_kind, &session) {
                emit(acp);
            }
        });
        self.usage = out.usage.clone();
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
        emit(grok_usage_event(&self.usage, self.auth_kind));
        emit(AcpEvent::Done { stop_reason: stop });
        Ok(())
    }
}

fn to_acp(ev: LoopEvent, kind: AuthKind, session: &str) -> Option<AcpEvent> {
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
        LoopEvent::Usage(usage) => grok_usage_event(&usage, kind),
        LoopEvent::Permission { id, name, action } => AcpEvent::Permission(PermissionAsk {
            rpc_id: serde_json::Value::String(id.clone()),
            session_id: session.to_string(),
            title: name.clone(),
            tool_call_id: id,
            action,
            reason: String::new(),
            reject_option: Some("denied".into()),
        }),
    })
}

fn grok_usage_event(usage: &Usage, kind: AuthKind) -> AcpEvent {
    AcpEvent::Usage(GrokUsage {
        input_tokens: usage.input_tokens,
        output_tokens: usage.output_tokens,
        reasoning_tokens: usage.reasoning_tokens,
        total_tokens: usage
            .input_tokens
            .saturating_add(usage.output_tokens)
            .saturating_add(usage.reasoning_tokens),
        cost_in_usd_ticks: usage.cost_in_usd_ticks,
        meter: kind.meter().to_string(),
        ..GrokUsage::default()
    })
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
        )));
        assert!(events.iter().any(|ev| matches!(ev, AcpEvent::Done { stop_reason } if stop_reason == "end_turn")));
        assert!(!events.iter().any(|ev| matches!(ev, AcpEvent::Permission(_))));
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
