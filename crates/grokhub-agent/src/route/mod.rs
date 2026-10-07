//! Router-ready model calls (Spike-6a). Every model call a new step adds goes
//! through [`call_model`] with its own (provider, model, effort), so effort and
//! model routing can later be chosen per step in one place. This is the thin
//! wrapper only: no effort UI and no `reasoning_effort` reads here.
//!
//! Each call writes one line to `{config_dir}/spans/model.jsonl` with tokens in,
//! cached, out, reasoning and cost (provider ticks, 1e-6 USD). Never the prompt
//! or the reply.

use std::io::Write;
use std::path::Path;

use serde::{Deserialize, Serialize};

/// One step's routing: who answers and how hard it thinks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelCall<'a> {
    /// What the call is for (`proactive.draft`), for the span line.
    pub step: &'a str,
    pub provider: &'a str,
    pub model: &'a str,
    pub effort: Option<&'a str>,
}

/// What the provider reported for one call. Unknown fields stay 0.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelUsage {
    pub tokens_in: u64,
    pub cached: u64,
    pub tokens_out: u64,
    pub reasoning: u64,
    /// Provider cost ticks (1e-6 USD).
    pub cost_ticks: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelReply {
    pub text: String,
    pub usage: ModelUsage,
}

/// The `spans/model.jsonl` line. Additive and serde-default.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelSpan {
    #[serde(default)]
    pub ts_ms: u64,
    #[serde(default)]
    pub step: String,
    #[serde(default)]
    pub provider: String,
    #[serde(default)]
    pub model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
    #[serde(default)]
    pub ok: bool,
    #[serde(default, flatten)]
    pub usage: ModelUsage,
}

pub const MODEL_SPAN_FILE: &str = "model.jsonl";

/// Run one model call through `send` and log its usage. `config_dir` `None`
/// skips the log (tests, scratch).
pub fn call_model(
    config_dir: Option<&Path>,
    call: &ModelCall<'_>,
    prompt: &str,
    send: impl FnOnce(&ModelCall<'_>, &str) -> Result<ModelReply, String>,
) -> Result<String, String> {
    let out = send(call, prompt);
    if let Some(dir) = config_dir {
        let span = ModelSpan {
            ts_ms: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0),
            step: call.step.to_string(),
            provider: call.provider.to_string(),
            model: call.model.to_string(),
            effort: call.effort.map(str::to_string),
            ok: out.is_ok(),
            usage: out.as_ref().map(|r| r.usage).unwrap_or_default(),
        };
        let _ = append_model_span(dir, &span);
    }
    out.map(|r| r.text)
}

pub fn append_model_span(config_dir: &Path, span: &ModelSpan) -> Result<(), String> {
    let dir = config_dir.join("spans");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let line = serde_json::to_string(span).map_err(|e| e.to_string())?;
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join(MODEL_SPAN_FILE))
        .map_err(|e| e.to_string())?;
    writeln!(f, "{line}").map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_call_routes_by_step_and_logs_usage_without_content() {
        let root = std::env::temp_dir().join(format!("grokhub-route-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let call = ModelCall { step: "proactive.draft", provider: "xai", model: "grok-4.7", effort: Some("low") };
        let mut seen = None;
        let text = call_model(Some(&root), &call, "secret prompt words", |c, p| {
            seen = Some((c.model.to_string(), c.effort.map(str::to_string), p.to_string()));
            Ok(ModelReply {
                text: "Hi Sam".into(),
                usage: ModelUsage { tokens_in: 120, cached: 80, tokens_out: 12, reasoning: 30, cost_ticks: 450 },
            })
        })
        .unwrap();
        assert_eq!(text, "Hi Sam");
        assert_eq!(seen, Some(("grok-4.7".into(), Some("low".into()), "secret prompt words".into())));
        let log = std::fs::read_to_string(root.join("spans").join(MODEL_SPAN_FILE)).unwrap();
        assert!(!log.contains("secret prompt") && !log.contains("Hi Sam"), "{log}");
        let span: ModelSpan = serde_json::from_str(log.lines().next().unwrap()).unwrap();
        assert_eq!(
            (span.step.as_str(), span.provider.as_str(), span.model.as_str(), span.effort.as_deref(), span.ok),
            ("proactive.draft", "xai", "grok-4.7", Some("low"), true)
        );
        assert_eq!(
            span.usage,
            ModelUsage { tokens_in: 120, cached: 80, tokens_out: 12, reasoning: 30, cost_ticks: 450 }
        );
        let err = call_model(Some(&root), &call, "p", |_, _| Err("offline".into()));
        assert_eq!(err, Err("offline".to_string()));
        let log = std::fs::read_to_string(root.join("spans").join(MODEL_SPAN_FILE)).unwrap();
        let last: ModelSpan = serde_json::from_str(log.lines().last().unwrap()).unwrap();
        assert!(!last.ok);
        assert_eq!(last.usage, ModelUsage::default());
        let _ = std::fs::remove_dir_all(&root);
    }
}
