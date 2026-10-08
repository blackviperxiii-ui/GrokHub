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
    let started = std::time::Instant::now();
    let out = send(call);
    let elapsed = started.elapsed();
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
    // Router R0 shadow: what the router would pick for this call. The call is unchanged.
    let class = if call.origin == Origin::User { "chat:default" } else { "background:summarize" };
    let shadow = super::ShadowCall { provider: call.provider.as_str(), class, model: call.model, effort: call.effort, ..Default::default() };
    let error = out.as_ref().err().cloned().unwrap_or_default();
    let done = super::ShadowDone::cabin(out.is_ok(), usage.as_ref(), &error, elapsed, None);
    let mut route = super::shadow::route_record(config_dir, &shadow, &done, span.ts_ms);
    route.span_id = span.span_ref();
    span.route = Some(Box::new(route));
    let _ = append_span(config_dir, &span);
    if let Some(obs) = super::shadow::observation(&shadow, &done, span.ts_ms) {
        let _ = grokhub_core::model_registry::store::append_observation(config_dir, &obs);
    }
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
    }
}
