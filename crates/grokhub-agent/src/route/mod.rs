//! Router-ready (Spike-4c): one helper every cabin model call goes through.
//! A call names its (provider, model, effort) per step, so effort or model
//! routing can later change in one place. Each call writes one span to
//! `spans/model-calls.jsonl` with who started it and what it used (tokens in,
//! cached, out, reasoning, and cost). No prompt, no reply.

use std::path::Path;

use serde_json::Value;

use crate::harness::{append_span, current_origin, AccessMode, ModelUsage, Origin, Span};

/// Span session for model calls.
pub const MODEL_TRACE: &str = "model-calls";
/// Span tool name for model calls.
pub const MODEL_TOOL: &str = "model.call";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Provider {
    Xai,
}

impl Provider {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Xai => "xai",
        }
    }
}

/// One model call: where it goes and how hard it thinks.
#[derive(Debug, Clone, Copy)]
pub struct ModelCall<'a> {
    pub provider: Provider,
    pub model: &'a str,
    pub effort: Option<&'a str>,
    pub origin: Origin,
}

impl<'a> ModelCall<'a> {
    /// The origin is this thread's [`OriginScope`](crate::harness::OriginScope).
    pub fn new(provider: Provider, model: &'a str, effort: Option<&'a str>) -> Self {
        Self { provider, model, effort, origin: current_origin() }
    }
}

/// Run `send` for `call` and write its span. `send` returns the result plus
/// the provider's usage report; a failed call is logged with no usage.
pub fn call_model<T>(
    config_dir: &Path,
    call: &ModelCall<'_>,
    send: impl FnOnce(&ModelCall<'_>) -> Result<(T, ModelUsage), String>,
) -> Result<T, String> {
    let out = send(call);
    let args = serde_json::json!({
        "provider": call.provider.as_str(),
        "model": call.model,
        "effort": call.effort.unwrap_or(""),
    })
    .to_string();
    let (result, usage) = match &out {
        Ok((_, usage)) => ("ok", Some(*usage)),
        Err(_) => ("error", None),
    };
    let mut span = Span::soft_allow(MODEL_TRACE, MODEL_TOOL, &args, result, "", AccessMode::Supervised, call.provider.as_str())
        .from_origin(call.origin)
        .on_path("model");
    span.access = String::new();
    span.usage = usage;
    let _ = append_span(config_dir, &span);
    out.map(|(value, _)| value)
}

/// The usage block of an xAI reply, Responses or Chat Completions shape.
pub fn xai_usage(reply: &Value) -> ModelUsage {
    let u = &reply["usage"];
    let n = |v: &Value| v.as_u64().unwrap_or(0);
    ModelUsage {
        input_tokens: n(&u["input_tokens"]).max(n(&u["prompt_tokens"])),
        cached_tokens: n(&u["input_tokens_details"]["cached_tokens"]).max(n(&u["prompt_tokens_details"]["cached_tokens"])),
        output_tokens: n(&u["output_tokens"]).max(n(&u["completion_tokens"])),
        reasoning_tokens: n(&u["output_tokens_details"]["reasoning_tokens"])
            .max(n(&u["completion_tokens_details"]["reasoning_tokens"])),
        cost_in_usd_ticks: u["cost_in_usd_ticks"].as_i64().unwrap_or(0),
    }
//! Router-ready model calls (Spike-3b). Every model call an episode makes
//! names its provider, model and effort per step and goes through
//! [`call_model`], so effort or model routing can change later in one place.
//! The only provider today is xAI over [`crate::XaiClient`] (or a test
//! [`ModelClient`]); this adds no effort UI and reads no `reasoning_effort`.

pub mod cabin;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::client::{ClientError, InputItem, ModelClient, ResponsesRequest, TurnOutput, Usage};
use crate::CancelToken;

/// The provider [`call_model`] knows.
pub const PROVIDER_XAI: &str = "xai";

/// The episode worker's own step (the model that proposes tools).
pub const CLASS_EPISODE: &str = "episode:step";
/// VerifyGate's checker (fresh context).
pub const CLASS_JUDGE: &str = "background:judge";
/// Folding two step summaries into one parent.
pub const CLASS_COMPACT: &str = "background:compact";

/// Effort for background calls (judge and fold). A constant, not a setting.
pub const BACKGROUND_EFFORT: &str = "low";

/// One model call: who answers it and what it is for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelCall {
    pub provider: String,
    pub model: String,
    /// `None` sends no reasoning field.
    pub effort: Option<String>,
    /// What the call is for (`background:judge`), logged with its tokens.
    pub class: String,
    pub conversation_id: String,
    pub input: Vec<InputItem>,
    pub tools: Vec<Value>,
}

impl ModelCall {
    pub fn xai(model: &str, effort: Option<&str>, class: &str, conversation_id: &str, input: Vec<InputItem>) -> Self {
        Self {
            provider: PROVIDER_XAI.into(),
            model: model.into(),
            effort: effort.map(str::to_string),
            class: class.into(),
            conversation_id: conversation_id.into(),
            input,
            tools: Vec::new(),
        }
    }

    pub fn with_tools(mut self, tools: Vec<Value>) -> Self {
        self.tools = tools;
        self
    }

    /// The wire request this call sends.
    pub fn request(&self) -> ResponsesRequest {
        ResponsesRequest {
            model: self.model.clone(),
            effort: self.effort.clone(),
            input: self.input.clone(),
            conversation_id: self.conversation_id.clone(),
            tools: self.tools.clone(),
            hosted_search: false,
            call_timeout: None,
        }
    }
}

/// Tokens and cost of one routed call, for the step's span.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CallTokens {
    pub class: String,
    pub model: String,
    #[serde(default)]
    pub effort: String,
    #[serde(rename = "in")]
    pub input: u64,
    pub cached: u64,
    pub out: u64,
    pub reasoning: u64,
    /// xAI cost ticks (1e-10 USD).
    pub cost_ticks: i64,
}

impl CallTokens {
    pub fn of(call: &ModelCall, usage: &Usage) -> Self {
        Self {
            class: call.class.clone(),
            model: call.model.clone(),
            effort: call.effort.clone().unwrap_or_default(),
            input: usage.input_tokens,
            cached: usage.cached_tokens,
            out: usage.output_tokens,
            reasoning: usage.reasoning_tokens,
            cost_ticks: usage.cost_in_usd_ticks,
        }
    }
}

/// A routed call's answer plus its token line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Routed {
    pub out: TurnOutput,
    pub tokens: CallTokens,
}

/// Send one call to its provider. An unknown provider is an error, never a
/// silent fallback to another one.
pub fn call_model(client: &dyn ModelClient, call: &ModelCall, cancel: &CancelToken) -> Result<Routed, ClientError> {
    if call.provider != PROVIDER_XAI {
        return Err(ClientError::Protocol(format!("no route for provider `{}`", call.provider)));
    }
    let out = client.stream(&call.request(), cancel, &mut |_| {})?;
    let tokens = CallTokens::of(call, &out.usage);
    Ok(Routed { out, tokens })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::{read_spans, OriginScope};

    #[test]
    fn a_model_call_writes_one_span_with_origin_tokens_and_cost_but_no_text() {
        let dir = crate::harness::test_dir("route-span");
        let reply = serde_json::json!({
            "output": [{"content": [{"text": "secret reply sk-abcdefghijklmnopqrstuv"}]}],
            "usage": {
                "input_tokens": 1200,
                "input_tokens_details": {"cached_tokens": 800},
                "output_tokens": 90,
                "output_tokens_details": {"reasoning_tokens": 40},
                "cost_in_usd_ticks": 31_000_000
            }
        });
        let got = {
            let _o = OriginScope::enter(Origin::Proactive);
            let call = ModelCall::new(Provider::Xai, "grok-4-fast", Some("low"));
            call_model(&dir, &call, |c| {
                assert_eq!((c.model, c.effort), ("grok-4-fast", Some("low")));
                Ok(("the reply", xai_usage(&reply)))
            })
        };
        assert_eq!(got, Ok("the reply"));
        let failed = call_model(&dir, &ModelCall::new(Provider::Xai, "grok-4", None), |_| Err::<((), ModelUsage), _>("HTTP 500".into()));
        assert_eq!(failed, Err("HTTP 500".to_string()));
        let spans = read_spans(&dir, MODEL_TRACE).unwrap();
        assert_eq!(spans.len(), 2);
        assert_eq!(spans[0].origin, Origin::Proactive);
        assert_eq!(spans[0].args_redacted, r#"{"effort":"low","model":"grok-4-fast","provider":"xai"}"#);
        assert_eq!(
            spans[0].usage,
            Some(ModelUsage {
                input_tokens: 1200,
                cached_tokens: 800,
                output_tokens: 90,
                reasoning_tokens: 40,
                cost_in_usd_ticks: 31_000_000
            })
        );
        assert_eq!((spans[1].origin, spans[1].result.as_str(), spans[1].usage), (Origin::User, "error", None));
        let raw = std::fs::read_to_string(crate::harness::span_path(&dir, MODEL_TRACE)).unwrap();
        assert!(!raw.contains("secret reply") && !raw.contains("sk-abc"), "{raw}");
        let chat = serde_json::json!({"usage": {"prompt_tokens": 10, "prompt_tokens_details": {"cached_tokens": 4}, "completion_tokens": 3, "completion_tokens_details": {"reasoning_tokens": 1}}});
        assert_eq!(
            xai_usage(&chat),
            ModelUsage { input_tokens: 10, cached_tokens: 4, output_tokens: 3, reasoning_tokens: 1, cost_in_usd_ticks: 0 }
        );
        let _ = std::fs::remove_dir_all(dir);
    use crate::client::{ContentPart, StreamEvent};
    use std::sync::Mutex;

    struct Echo(Mutex<Vec<ResponsesRequest>>);

    impl ModelClient for Echo {
        fn stream(
            &self,
            req: &ResponsesRequest,
            _cancel: &CancelToken,
            _sink: &mut dyn FnMut(StreamEvent),
        ) -> Result<TurnOutput, ClientError> {
            self.0.lock().unwrap().push(req.clone());
            Ok(TurnOutput {
                text: "VERIFY_OK".into(),
                reasoning: String::new(),
                calls: Vec::new(),
                usage: Usage {
                    input_tokens: 900,
                    output_tokens: 7,
                    reasoning_tokens: 3,
                    cost_in_usd_ticks: 1_200,
                    cached_tokens: 640,
                },
            })
        }
    }

    #[test]
    fn call_model_sends_model_and_effort_and_logs_every_token_kind() {
        let echo = Echo(Mutex::new(Vec::new()));
        let input = vec![InputItem::Message { role: "user".into(), content: vec![ContentPart::InputText("hi".into())] }];
        let call = ModelCall::xai("grok-4.7", Some(BACKGROUND_EFFORT), CLASS_JUDGE, "ep-1", input);
        let routed = call_model(&echo, &call, &CancelToken::new()).unwrap();
        assert_eq!(routed.out.text, "VERIFY_OK");
        assert_eq!(
            routed.tokens,
            CallTokens {
                class: "background:judge".into(),
                model: "grok-4.7".into(),
                effort: "low".into(),
                input: 900,
                cached: 640,
                out: 7,
                reasoning: 3,
                cost_ticks: 1_200,
            }
        );
        let sent = echo.0.lock().unwrap();
        assert_eq!(sent.len(), 1);
        assert_eq!((sent[0].model.as_str(), sent[0].effort.as_deref()), ("grok-4.7", Some("low")));
        assert!(!sent[0].hosted_search);
        let line = serde_json::to_string(&routed.tokens).unwrap();
        assert_eq!(
            line,
            r#"{"class":"background:judge","model":"grok-4.7","effort":"low","in":900,"cached":640,"out":7,"reasoning":3,"cost_ticks":1200}"#
        );
    }

    #[test]
    fn unknown_provider_is_refused_and_sends_nothing() {
        let echo = Echo(Mutex::new(Vec::new()));
        let mut call = ModelCall::xai("m", None, CLASS_COMPACT, "ep-1", Vec::new());
        call.provider = "other".into();
        let err = call_model(&echo, &call, &CancelToken::new()).unwrap_err();
        assert_eq!(err, ClientError::Protocol("no route for provider `other`".into()));
        assert!(echo.0.lock().unwrap().is_empty());
    }
}
