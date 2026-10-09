//! The real catalog sources and the probe transport: the only router code that
//! touches the network or runs `grok`. Every request goes through
//! `guard_egress` to api.x.ai with the bearer the native engine already loads
//! (a Sign in with Grok token or the console key). Never Grok Build's
//! credentials, never `~/.grok`: `grok models` runs in the cabin Grok home.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde_json::Value;

use grokhub_core::model_registry::cost_class::CostClass;
use grokhub_core::model_registry::discover::{gb_listing, parse_xai_catalog, CatalogSource, Listing};
use grokhub_core::model_registry::probe::{run_probe, ProbeCall, ProbeEnv, ProbeReply, ProbeTransport};
use grokhub_core::model_registry::profile::ProbeResult;
use grokhub_core::model_registry::{ModelMeta, SourceKind};

use crate::client::{RESPONSES_URL, USER_AGENT};
use crate::harness::{append_span, guard_egress, AccessMode, EgressReq, GateOutcome, ModelUsage, Origin, OriginScope, Span};

pub const XAI_MODELS_URL: &str = crate::models::XAI_MODELS_URL;
pub const XAI_LANGUAGE_MODELS_URL: &str = "https://api.x.ai/v1/language-models";
/// Probe call spans go here, beside the model-call spans.
pub const PROBE_TOOL: &str = "model.probe";
pub const PROBE_CLASS: &str = "background:probe";
/// A listing body larger than this is refused.
const BODY_CAP: u64 = 4 * 1024 * 1024;

fn agent(timeout: Duration) -> ureq::Agent {
    ureq::AgentBuilder::new().timeout_connect(Duration::from_secs(15)).timeout(timeout).build()
}

fn read_capped(resp: ureq::Response) -> Result<String, String> {
    let mut buf = String::new();
    resp.into_reader().take(BODY_CAP).read_to_string(&mut buf).map_err(|e| e.to_string())?;
    Ok(buf)
}

/// The guard in front of every router request. Listings and probes carry no user data.
fn guard(config_dir: &Path, url: &str) -> Result<(), String> {
    match guard_egress(config_dir, &EgressReq::new(url, &[])) {
        GateOutcome::Allow => Ok(()),
        GateOutcome::Park { reason, .. } | GateOutcome::Refuse { reason } => Err(reason),
    }
}

/// `GET /v1/models` + `/v1/language-models`, scoped to the user's own sign-in or key.
pub struct XaiApiSource {
    pub config_dir: PathBuf,
    pub bearer: String,
}

impl XaiApiSource {
    fn get(&self, url: &str) -> Result<String, String> {
        guard(&self.config_dir, url)?;
        let resp = agent(Duration::from_secs(30))
            .get(url)
            .set("Authorization", &format!("Bearer {}", self.bearer))
            .set("User-Agent", USER_AGENT)
            .call()
            .map_err(|e| match e {
                ureq::Error::Status(code, _) => format!("HTTP {code}"),
                ureq::Error::Transport(t) => t.to_string(),
            })?;
        read_capped(resp)
    }
}

impl CatalogSource for XaiApiSource {
    fn kind(&self) -> SourceKind {
        SourceKind::XaiApi
    }

    fn fetch(&self) -> Result<Listing, String> {
        if self.bearer.trim().is_empty() {
            return Err("not signed in".into());
        }
        let models = self.get(XAI_MODELS_URL);
        let language = self.get(XAI_LANGUAGE_MODELS_URL);
        if let (Err(a), Err(_)) = (&models, &language) {
            return Err(a.clone());
        }
        let rows = parse_xai_catalog(models.as_deref().ok(), language.as_deref().ok())?;
        Ok(Listing::new(SourceKind::XaiApi, rows))
    }
}

/// `grok models` in the cabin Grok home (plan scoped). Ids only.
pub struct GrokBuildSource {
    pub bin: PathBuf,
    pub cwd: PathBuf,
}

impl CatalogSource for GrokBuildSource {
    fn kind(&self) -> SourceKind {
        SourceKind::GrokBuild
    }

    fn fetch(&self) -> Result<Listing, String> {
        let text = grokhub_acp::grok_stdout_timeout(&self.bin, &self.cwd, &["models"], 20)?;
        let ids = grokhub_acp::parse_models_list(&text);
        if ids.is_empty() {
            return Err("grok models listed nothing".into());
        }
        let version = grokhub_acp::grok_stdout_timeout(&self.bin, &self.cwd, &["--version"], 10).ok();
        let version = version.as_deref().map(str::trim).map(|v| v.strip_prefix("grok ").unwrap_or(v));
        Ok(gb_listing(&ids, version))
    }
}

/// Every source that answered. A failed source is skipped (it changes nothing).
pub fn fetch_all(sources: &[&dyn CatalogSource]) -> (Vec<Listing>, Vec<String>) {
    let mut ok = Vec::new();
    let mut errors = Vec::new();
    for s in sources {
        match s.fetch() {
            Ok(l) => ok.push(l),
            Err(e) => errors.push(format!("{}: {e}", s.kind().as_str())),
        }
    }
    (ok, errors)
}

/// The probe's wire: `POST /v1/responses`, not streamed, one call at a time.
pub struct XaiProbeTransport {
    pub bearer: String,
}

fn reply_of(v: &Value) -> (String, Vec<(String, String)>) {
    let mut text = String::new();
    let mut calls = Vec::new();
    for item in v["output"].as_array().into_iter().flatten() {
        match item["type"].as_str() {
            Some("function_call") => calls.push((
                item["name"].as_str().unwrap_or("").to_string(),
                item["arguments"].as_str().unwrap_or("").to_string(),
            )),
            Some("message") => {
                for part in item["content"].as_array().into_iter().flatten() {
                    text.push_str(part["text"].as_str().unwrap_or(""));
                }
            }
            _ => {}
        }
    }
    (text, calls)
}

impl ProbeTransport for XaiProbeTransport {
    fn send(&mut self, call: &ProbeCall) -> Result<ProbeReply, String> {
        let started = Instant::now();
        let resp = agent(Duration::from_secs(call.timeout_secs))
            .post(RESPONSES_URL)
            .set("Authorization", &format!("Bearer {}", self.bearer))
            .set("User-Agent", USER_AGENT)
            .send_json(call.body.clone());
        let latency_ms = started.elapsed().as_millis() as u64;
        let resp = match resp {
            Ok(r) => r,
            Err(ureq::Error::Status(code, _)) => return Ok(ProbeReply { status: code, latency_ms, ..ProbeReply::default() }),
            Err(ureq::Error::Transport(t)) => {
                let timed_out = t.to_string().to_ascii_lowercase().contains("timed out");
                return Ok(ProbeReply { timed_out, latency_ms, ..ProbeReply::default() });
            }
        };
        let v: Value = serde_json::from_str(&read_capped(resp)?).map_err(|e| e.to_string())?;
        let u = super::cabin::xai_usage(&v);
        let (text, tool_calls) = reply_of(&v);
        Ok(ProbeReply {
            status: 200,
            timed_out: false,
            latency_ms,
            served_model: v["model"].as_str().map(str::to_string),
            input_tokens: u.input_tokens,
            cached_tokens: u.cached_tokens,
            output_tokens: u.output_tokens,
            reasoning_tokens: u.reasoning_tokens,
            cost_ticks: u.cost_in_usd_ticks,
            text,
            tool_calls,
        })
    }
}

/// The span one probe call writes: origin `self_manage`, class `background:probe`, counts only.
pub fn probe_span(call: &ProbeCall, reply: &ProbeReply) -> Span {
    let args = serde_json::json!({
        "class": PROBE_CLASS,
        "model": call.model,
        "effort": call.effort.clone().unwrap_or_default(),
        "check": call.kind.as_str(),
    })
    .to_string();
    let ok = (200..300).contains(&reply.status);
    let mut span = Span::soft_allow(super::cabin::MODEL_TRACE, PROBE_TOOL, &args, if ok { "ok" } else { "error" }, "", AccessMode::Supervised, "xai")
        .from_origin(Origin::SelfManage)
        .on_path("model");
    span.access = String::new();
    span.usage = Some(ModelUsage {
        input_tokens: reply.input_tokens,
        cached_tokens: reply.cached_tokens,
        output_tokens: reply.output_tokens,
        reasoning_tokens: reply.reasoning_tokens,
        cost_in_usd_ticks: reply.cost_ticks,
    });
    span
}

/// Probe one model for real: guarded, spanned, with the gap between calls slept.
pub fn probe_model(config_dir: &Path, bearer: &str, meta: &ModelMeta, cost: CostClass) -> ProbeResult {
    let _lap = crate::timing::lap("route:onboarding_probe");
    let _origin = OriginScope::enter(Origin::SelfManage);
    let mut transport = XaiProbeTransport { bearer: bearer.to_string() };
    let mut guard_fn = || guard(config_dir, RESPONSES_URL).is_ok();
    let mut wait = |ms: u64| std::thread::sleep(Duration::from_millis(ms));
    let mut on_call = |call: &ProbeCall, reply: &ProbeReply| {
        let _ = append_span(config_dir, &probe_span(call, reply));
    };
    run_probe(meta, cost, ProbeEnv { transport: &mut transport, guard: &mut guard_fn, wait: &mut wait, on_call: &mut on_call })
}

/// Rebuild the routing table when the model list or a profile changed
/// (`force`: the Refresh models button). With a plan sign-in, today's eval
/// build runs on the real wire: guarded, spanned as `background:eval`
/// (origin `self_manage`), only on `included` routes.
pub fn rebuild_table(config_dir: &Path, plan_bearer: Option<&str>, force: bool, now_ms: u64) -> super::table::Rebuilt {
    let _lap = crate::timing::lap("route:table_rebuild");
    let _origin = OriginScope::enter(Origin::SelfManage);
    let Some(bearer) = plan_bearer else {
        return super::table::rebuild(config_dir, None, force, now_ms);
    };
    let mut transport = XaiProbeTransport { bearer: bearer.to_string() };
    let mut guard_fn = || guard(config_dir, RESPONSES_URL).is_ok();
    let mut on_call = |call: &ProbeCall, reply: &ProbeReply| {
        let _ = append_span(config_dir, &eval_span(call, reply));
    };
    let env = super::table::EvalEnv { transport: &mut transport, guard: &mut guard_fn, on_call: &mut on_call };
    super::table::rebuild(config_dir, Some(env), force, now_ms)
}

/// One eval call's span: like a probe's, with class `background:eval`.
pub fn eval_span(call: &ProbeCall, reply: &ProbeReply) -> Span {
    let mut span = probe_span(call, reply);
    span.args_redacted = serde_json::json!({
        "class": super::table::EVAL_CLASS,
        "model": call.model,
        "effort": call.effort.clone().unwrap_or_default(),
    })
    .to_string();
    span
}

#[cfg(test)]
mod tests {
    use super::*;
    use grokhub_core::model_registry::probe::{tool_call, ProbeKind};

    #[test]
    fn reply_of_reads_text_and_function_calls() {
        let v = serde_json::json!({"output": [
            {"type": "function_call", "name": "probe_echo", "arguments": "{\"word\":\"ping\"}"},
            {"type": "message", "content": [{"type": "output_text", "text": "{\"a\":\"ok\"}"}]}
        ]});
        let (text, calls) = reply_of(&v);
        assert_eq!(text, "{\"a\":\"ok\"}");
        assert_eq!(calls, vec![("probe_echo".to_string(), "{\"word\":\"ping\"}".to_string())]);
    }

    #[test]
    fn a_probe_span_is_self_manage_background_probe_with_counts_and_no_text() {
        let call = tool_call("grok-4.7", Some("low"));
        assert_eq!(call.kind, ProbeKind::Tool);
        let reply = ProbeReply { status: 200, input_tokens: 70, output_tokens: 9, cost_ticks: 1_500, text: "secret".into(), ..ProbeReply::default() };
        let span = probe_span(&call, &reply);
        assert_eq!(span.origin, Origin::SelfManage);
        assert_eq!(span.args_redacted, r#"{"check":"tool","class":"background:probe","effort":"low","model":"grok-4.7"}"#);
        assert_eq!(span.usage.unwrap().input_tokens, 70);
        assert!(!serde_json::to_string(&span).unwrap().contains("secret"));
    }

    struct Down(SourceKind);
    impl CatalogSource for Down {
        fn kind(&self) -> SourceKind {
            self.0
        }
        fn fetch(&self) -> Result<Listing, String> {
            Err("offline".into())
        }
    }

    #[test]
    fn a_failed_source_is_skipped_and_named() {
        let (ok, errors) = fetch_all(&[&Down(SourceKind::XaiApi), &Down(SourceKind::GrokBuild)]);
        assert!(ok.is_empty());
        assert_eq!(errors, vec!["xai_api: offline".to_string(), "grok_build: offline".to_string()]);
    }
}
