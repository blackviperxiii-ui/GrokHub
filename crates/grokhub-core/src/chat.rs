use crate::host_plan::strip_host_cmd_line;
use serde_json::{json, Value};

pub const XAI_BASE: &str = "https://api.x.ai/v1";
pub const DEFAULT_MODEL: &str = "grok-4.3";
/// Default `grok -p --model` id, plus greeting and chips.
/// Grok CLI alpha accepts `-m` / `--model grok-4.7` (xAI model name).
pub const CABIN_FAST_MODEL: &str = "grok-4.7";
/// Used only if the default model returns empty.
pub const CABIN_FAST_FALLBACK: &str = "grok-4.5";
/// Reasoning effort for work the cabin starts on its own: scheduled runs (automations,
/// loops, /send tasks), the nightly review, ideas, the daily read, chips, the greeting,
/// and the thread goal. The composer's effort is for what you type.
pub const BACKGROUND_EFFORT: &str = "low";

pub fn needs_auth_banner(has_key: bool) -> bool {
    !has_key
}

/// First-run empty chat still needs the Connect Grok banner.
pub fn paint_connect_banner(has_key: bool, _message_count: usize) -> bool {
    needs_auth_banner(has_key)
}

pub fn extract_host_cmds(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in text.lines() {
        let t = line.trim();
        if let Some(cmd) = strip_host_cmd_line(t) {
            out.push(cmd.to_string());
        }
    }
    out
}

pub fn chat_request_body(model: &str, messages: &[(String, String)]) -> Value {
    chat_request_body_vision(model, messages, None, None)
}

pub fn chat_request_body_for_mode(mode: &str, messages: &[(String, String)]) -> Value {
    let model = model_for_mode(mode);
    chat_request_body_vision(model, messages, None, reasoning_effort_for_mode(mode))
}

pub fn chat_request_body_vision(
    model: &str,
    messages: &[(String, String)],
    image_data_url: Option<&str>,
    effort: Option<&str>,
) -> Value {
    let images: Vec<&str> = image_data_url.into_iter().collect();
    chat_request_body_images(model, messages, &images, effort)
}

/// Chat Completions body with every `data:image` URL on the last user message, in order.
pub fn chat_request_body_images(
    model: &str,
    messages: &[(String, String)],
    images: &[&str],
    effort: Option<&str>,
) -> Value {
    let mut msgs: Vec<Value> = messages
        .iter()
        .map(|(role, content)| json!({ "role": role, "content": content }))
        .collect();
    let urls: Vec<&str> = images
        .iter()
        .copied()
        .filter(|s| s.starts_with("data:image"))
        .collect();
    if !urls.is_empty() {
        if let Some(last) = msgs.last_mut() {
            if last["role"] == "user" {
                let text = last["content"].as_str().unwrap_or("").to_string();
                let mut parts = vec![json!({ "type": "text", "text": text })];
                parts.extend(
                    urls.iter()
                        .map(|url| json!({ "type": "image_url", "image_url": { "url": url } })),
                );
                last["content"] = Value::Array(parts);
            }
        }
    }
    let resolved = if model.is_empty() {
        DEFAULT_MODEL
    } else {
        model
    };
    let mut body = json!({
        "model": resolved,
        "stream": false,
        "messages": msgs,
    });
    if sends_reasoning_effort(resolved) {
        if let Some(effort) = effort {
            body["reasoning_effort"] = json!(effort);
        }
    }
    body
}

fn sends_reasoning_effort(model: &str) -> bool {
    matches!(model, "grok-4.3" | "grok-4.6" | "grok-4.7")
}

/// Fast's effort. Grok 4.3 takes `none` (xAI's model page lists none to xhigh, default low),
/// which is what keeps Fast quicker than Balance on the same model.
pub const FAST_EFFORT: &str = "none";

/// Think is Grok 4.7 at high. Legacy Max mode is the same model at xhigh.
/// Fast is Grok 4.3 with no reasoning. Balance is Grok 4.3 at its default (low).
/// Saved effort `max` migrates to xhigh.
pub fn reasoning_effort_for_mode(mode: &str) -> Option<&'static str> {
    match mode.trim() {
        "max" | "deep" | "heavy" => Some("xhigh"),
        "think" | "build" | "expert" => Some("high"),
        "fast" => Some(FAST_EFFORT),
        _ => None,
    }
}

/// The effort for a quick ask (`chat:quick`) on `model`: Fast's when it is Fast's model, else unset.
pub fn quick_effort(model: &str) -> Option<&'static str> {
    (model.trim() == model_for_mode("fast")).then_some(FAST_EFFORT)
}

/// Grok Build `grok agent --reasoning-effort` from the composer ladder / `/effort`.
pub fn agent_reasoning_effort_for_mode(mode: &str) -> Option<&'static str> {
    match mode.trim() {
        "max" | "deep" | "heavy" => Some("xhigh"),
        "think" | "build" | "expert" => Some("high"),
        "fast" | "auto" => Some("low"),
        "balanced" | "balance" => Some("medium"),
        _ => None,
    }
}

/// Composer effort dropdown levels (Grok Build 1.0.11 canonical ladder).
pub const REASONING_EFFORTS: &[(&str, &str)] = &[
    ("none", "None"),
    ("low", "Low"),
    ("medium", "Medium"),
    ("high", "High"),
    ("xhigh", "Extra High"),
];

/// Models that take `reasoning.effort` low to xhigh (docs.x.ai Reasoning guide
/// and model pages): grok-4.7, grok-4.6, grok-4.5, grok-4.3 and
/// grok-4.20-multi-agent, with their dated or `-latest` ids. Every other xAI
/// model answers 400 to it, and a `-non-reasoning` id never takes one.
pub fn accepts_reasoning_effort(model: &str) -> bool {
    let m = model.trim().to_ascii_lowercase();
    if m.contains("non-reasoning") {
        return false;
    }
    [
        "grok-4.7",
        "grok-4.6",
        "grok-4.5",
        "grok-4.3",
        "grok-4.20-multi-agent",
    ]
    .iter()
    .any(|f| {
        m.strip_prefix(f)
            .is_some_and(|rest| rest.is_empty() || rest.starts_with('-'))
    })
}

/// Models whose reasoning can be switched off with `reasoning.effort: "none"`:
/// Grok 4.3 and its dated or `-latest` ids (docs.x.ai model page: none to xhigh,
/// default low). This is what makes Fast ([`FAST_EFFORT`]) quicker than Balance.
pub fn accepts_effort_none(model: &str) -> bool {
    let m = model.trim().to_ascii_lowercase();
    m.strip_prefix("grok-4.3")
        .is_some_and(|rest| rest.is_empty() || rest.starts_with('-'))
}

/// Normalize effort ids and legacy aliases (`/effort`, `/mode`).
pub fn parse_reasoning_effort(s: &str) -> Option<&'static str> {
    match s.trim().to_ascii_lowercase().as_str() {
        "none" | "off" => Some("none"),
        // Minimal was never a real level (2.10.87). A saved one loads as Low, the
        // smallest level that still reasons; None would switch reasoning off.
        "low" | "fast" | "minimal" | "mini" => Some("low"),
        "medium" | "med" | "balanced" | "balance" => Some("medium"),
        "high" | "think" | "build" | "expert" => Some("high"),
        // Saved Max is the next lower offered level (Extra High). Do not send `max`.
        "xhigh" | "deep" | "heavy" | "max" => Some("xhigh"),
        _ => None,
    }
}

pub fn effort_label(id: &str) -> &'static str {
    parse_reasoning_effort(id)
        .and_then(|e| {
            REASONING_EFFORTS
                .iter()
                .find(|(k, _)| *k == e)
                .map(|(_, l)| *l)
        })
        .unwrap_or("High")
}

pub fn chat_timeout_secs(effort: Option<&str>) -> u64 {
    match effort {
        Some("high") | Some("xhigh") | Some("max") => 600,
        _ => 120,
    }
}

pub fn should_failover_status(status: u16) -> bool {
    matches!(status, 401 | 403 | 429) || (500..600).contains(&status)
}

pub fn model_for_mode(mode: &str) -> &'static str {
    match mode {
        "max" | "deep" | "heavy" => CABIN_FAST_MODEL,
        "think" | "build" | "expert" => CABIN_FAST_MODEL,
        "balanced" | "balance" => "grok-4.3",
        "auto" | "fast" => "grok-4.3",
        _ => DEFAULT_MODEL,
    }
}

pub fn failover_model(current: &str) -> Option<&'static str> {
    let tier = tier_of_model(current);
    let next = crate::next_failover_tier(tier);
    let model = model_for_mode(next);
    // Fast and Balance share Grok 4.3, so 4.3 has nothing lower to fail over to.
    if next == tier || model == current.trim() {
        None
    } else {
        Some(model)
    }
}

fn tier_of_model(model: &str) -> &'static str {
    let m = model.to_ascii_lowercase();
    if m.contains("4.3") || m.contains("balance") {
        "balanced"
    } else if m.contains("4.6")
        || m.contains("4-latest")
        || m.contains("max")
        || m.contains("heavy")
        || m.contains("grok-4")
        || m.contains("grok4")
    {
        "max"
    } else {
        "fast"
    }
}

pub fn parse_chat_content(body: &Value) -> Option<String> {
    body.get("choices")?
        .as_array()?
        .first()?
        .get("message")?
        .get("content")?
        .as_str()
        .map(|s| s.to_string())
}

pub fn parse_chat_reasoning(body: &Value) -> Option<String> {
    let msg = body.get("choices")?.as_array()?.first()?.get("message")?;
    msg.get("reasoning_content")
        .or_else(|| msg.get("reasoning"))
        .and_then(|c| c.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
}

pub fn responses_url() -> String {
    format!("{XAI_BASE}/responses")
}

pub fn responses_request_body(
    model: &str,
    messages: &[(String, String)],
    image_data_url: Option<&str>,
    effort: Option<&str>,
) -> Value {
    let images: Vec<&str> = image_data_url.into_iter().collect();
    responses_request_body_images(model, messages, &images, effort)
}

/// Responses body with every `data:image` URL on the last user message, in order.
pub fn responses_request_body_images(
    model: &str,
    messages: &[(String, String)],
    images: &[&str],
    effort: Option<&str>,
) -> Value {
    let resolved = if model.is_empty() {
        DEFAULT_MODEL
    } else {
        model
    };
    let mut input: Vec<Value> = messages
        .iter()
        .map(|(role, content)| json!({ "role": role, "content": content }))
        .collect();
    let urls: Vec<&str> = images
        .iter()
        .copied()
        .filter(|s| s.starts_with("data:image"))
        .collect();
    if !urls.is_empty() {
        if let Some(last) = input.last_mut() {
            if last["role"] == "user" {
                let text = last["content"].as_str().unwrap_or("").to_string();
                let mut parts = vec![json!({ "type": "input_text", "text": text })];
                parts.extend(
                    urls.iter()
                        .map(|url| json!({ "type": "input_image", "image_url": url })),
                );
                last["content"] = Value::Array(parts);
            }
        }
    }
    let mut body = json!({
        "model": resolved,
        "stream": false,
        "store": false,
        "input": input,
    });
    if let Some(effort) = effort {
        body["reasoning"] = json!({ "effort": effort });
    }
    body
}

fn output_text_parts(item: &Value) -> String {
    let mut out = String::new();
    let t = item.get("type").and_then(|x| x.as_str()).unwrap_or("");
    if t == "output_text" || t == "text" {
        if let Some(text) = item.get("text").and_then(|x| x.as_str()) {
            out.push_str(text);
        }
        return out;
    }
    if let Some(content) = item.get("content").and_then(|c| c.as_array()) {
        for part in content {
            out.push_str(&output_text_parts(part));
        }
    } else if let Some(text) = item.get("text").and_then(|x| x.as_str()) {
        out.push_str(text);
    }
    out
}

pub fn parse_responses_text(body: &Value) -> Option<String> {
    let output = body.get("output").and_then(|o| o.as_array())?;
    let mut out = String::new();
    for item in output {
        let t = item.get("type").and_then(|x| x.as_str()).unwrap_or("");
        match t {
            "message" | "output_text" | "text" => out.push_str(&output_text_parts(item)),
            _ => {}
        }
    }
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

pub fn parse_responses_reasoning(body: &Value) -> Option<String> {
    let output = body.get("output").and_then(|o| o.as_array())?;
    let mut out = String::new();
    for item in output {
        if item.get("type").and_then(|x| x.as_str()) != Some("reasoning") {
            continue;
        }
        if let Some(summary) = item.get("summary").and_then(|s| s.as_array()) {
            for part in summary {
                if let Some(text) = part.get("text").and_then(|x| x.as_str()) {
                    out.push_str(text);
                }
            }
        }
        if out.is_empty() {
            out.push_str(&output_text_parts(item));
        }
    }
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

pub fn parse_model_text(body: &Value) -> Option<String> {
    parse_responses_text(body).or_else(|| parse_chat_content(body))
}

pub fn parse_model_reasoning(body: &Value) -> Option<String> {
    parse_responses_reasoning(body).or_else(|| parse_chat_reasoning(body))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn several_stills_ride_the_last_user_message_in_order() {
        let msgs = vec![
            ("system".to_string(), "diagnose".to_string()),
            ("user".to_string(), "video stutters".to_string()),
        ];
        let stills = [
            "data:image/png;base64,AAA",
            "https://x/y.png",
            "data:image/jpeg;base64,BBB",
        ];
        let r = responses_request_body_images("grok-4.7", &msgs, &stills, None);
        assert_eq!(
            r["input"][0],
            json!({ "role": "system", "content": "diagnose" })
        );
        assert_eq!(
            r["input"][1]["content"],
            json!([
                { "type": "input_text", "text": "video stutters" },
                { "type": "input_image", "image_url": "data:image/png;base64,AAA" },
                { "type": "input_image", "image_url": "data:image/jpeg;base64,BBB" }
            ])
        );
        let c = chat_request_body_images("grok-4.7", &msgs, &stills, None);
        assert_eq!(
            c["messages"][1]["content"],
            json!([
                { "type": "text", "text": "video stutters" },
                { "type": "image_url", "image_url": { "url": "data:image/png;base64,AAA" } },
                { "type": "image_url", "image_url": { "url": "data:image/jpeg;base64,BBB" } }
            ])
        );
        assert_eq!(
            responses_request_body("grok-4.7", &msgs, Some("data:image/png;base64,AAA"), None),
            responses_request_body_images("grok-4.7", &msgs, &["data:image/png;base64,AAA"], None),
            "one image is the same body as before"
        );
        assert_eq!(
            chat_request_body_images("grok-4.7", &msgs, &[], None)["messages"][1]["content"],
            "video stutters"
        );
    }

    #[test]
    fn only_reasoning_models_accept_an_effort() {
        for m in [
            "grok-4.7",
            "grok-4.7-latest",
            "GROK-4.6",
            "grok-4.5-0901",
            "grok-4.3",
            "grok-4.3-latest",
            "grok-4.20-multi-agent",
            " grok-4.20-multi-agent-0309 ",
        ] {
            assert!(accepts_reasoning_effort(m), "{m}");
        }
        for m in [
            "grok-4.20-0309-non-reasoning",
            "grok-4-1-fast-non-reasoning",
            "grok-4.20-0309-reasoning",
            "grok-4-fast",
            "grok-3-mini",
            "grok-4.70",
            "grok-4.30",
            "grok-code-fast-1",
            "",
        ] {
            assert!(!accepts_reasoning_effort(m), "{m}");
        }
    }

    #[test]
    fn banner() {
        assert!(needs_auth_banner(false));
        assert!(!needs_auth_banner(true));
        assert!(
            paint_connect_banner(false, 0),
            "first-run empty chat must still show Connect Grok"
        );
        assert!(paint_connect_banner(false, 3));
        assert!(!paint_connect_banner(true, 0));
    }

    #[test]
    fn host_cmds() {
        let t = "Checking.\nHOST_CMD: ls /tmp\nHOST_CMD: cat README.md\n";
        assert_eq!(extract_host_cmds(t), vec!["ls /tmp", "cat README.md"]);
        assert!(
            extract_host_cmds("HOST_CMDLINE: backup the repo\n").is_empty(),
            "HOST_CMD must not match HOST_CMDLINE"
        );
    }

    #[test]
    fn body_and_parse() {
        let body = chat_request_body("grok-3-mini-fast", &[("user".into(), "hi".into())]);
        assert_eq!(body["model"], "grok-3-mini-fast");
        assert_eq!(body["messages"][0]["content"], "hi");
        let vis = chat_request_body_vision(
            "grok-3-mini-fast",
            &[("user".into(), "see".into())],
            Some("data:image/jpeg;base64,AAAA"),
            None,
        );
        assert_eq!(vis["messages"][0]["content"][1]["type"], "image_url");
        let reply = json!({
            "choices": [{ "message": { "content": "hello" } }]
        });
        assert_eq!(parse_chat_content(&reply).as_deref(), Some("hello"));
        let reasoned = json!({
            "choices": [{ "message": { "content": "hello", "reasoning_content": "Need a snapshot." } }]
        });
        assert_eq!(
            parse_chat_reasoning(&reasoned).as_deref(),
            Some("Need a snapshot.")
        );
        assert!(should_failover_status(429));
        assert!(!should_failover_status(200));
        assert_eq!(failover_model("grok-4-latest"), Some("grok-4.3"));
        assert_eq!(failover_model("grok-4.6"), Some("grok-4.3"));
        assert_eq!(failover_model("grok-4.7"), Some("grok-4.3"));
        assert!(failover_model(DEFAULT_MODEL).is_none());
        let max = chat_request_body_for_mode("max", &[("user".into(), "hi".into())]);
        assert_eq!(max["model"], "grok-4.7");
        assert_eq!(max["reasoning_effort"], "xhigh");
        let fast = chat_request_body(DEFAULT_MODEL, &[("user".into(), "hi".into())]);
        assert!(fast.get("reasoning_effort").is_none());
        let streamed = responses_request_body(
            "grok-4.7",
            &[("user".into(), "hi".into())],
            None,
            Some("xhigh"),
        );
        assert_eq!(streamed["model"], "grok-4.7");
        assert_eq!(streamed["input"][0]["content"], "hi");
        assert_eq!(streamed["store"], false);
        assert_eq!(streamed["reasoning"]["effort"], "xhigh");
        assert_eq!(responses_url(), "https://api.x.ai/v1/responses");
        let vis = responses_request_body(
            "grok-4.7",
            &[("user".into(), "see".into())],
            Some("data:image/jpeg;base64,AAAA"),
            None,
        );
        assert_eq!(vis["input"][0]["content"][1]["type"], "input_image");
        let reply = json!({
            "output": [{
                "type": "message",
                "content": [{ "type": "output_text", "text": "hello" }]
            }]
        });
        assert_eq!(parse_responses_text(&reply).as_deref(), Some("hello"));
        let reasoned = json!({
            "output": [
                { "type": "reasoning", "summary": [{ "type": "summary_text", "text": "Need a snapshot." }] },
                { "type": "message", "content": [{ "type": "output_text", "text": "hello" }] }
            ]
        });
        assert_eq!(
            parse_responses_reasoning(&reasoned).as_deref(),
            Some("Need a snapshot.")
        );
        assert_eq!(parse_model_text(&reply).as_deref(), Some("hello"));
        assert_eq!(
            parse_model_text(&json!({
                "choices": [{ "message": { "content": "legacy" } }]
            }))
            .as_deref(),
            Some("legacy")
        );
        assert_eq!(chat_timeout_secs(Some("xhigh")), 600);
        assert_eq!(chat_timeout_secs(None), 120);
    }

    #[test]
    fn max_is_grok_4_7_xhigh() {
        assert_eq!(model_for_mode("max"), "grok-4.7");
        assert_eq!(model_for_mode("deep"), "grok-4.7");
        assert_eq!(model_for_mode("heavy"), "grok-4.7");
        assert_eq!(reasoning_effort_for_mode("max"), Some("xhigh"));
        assert_ne!(model_for_mode("max"), "grok-4-latest");
        assert_eq!(reasoning_effort_for_mode("max"), Some("xhigh"));
    }

    #[test]
    fn parse_reasoning_effort_aliases() {
        assert_eq!(parse_reasoning_effort("low"), Some("low"));
        assert_eq!(parse_reasoning_effort("fast"), Some("low"));
        assert_eq!(parse_reasoning_effort("medium"), Some("medium"));
        assert_eq!(parse_reasoning_effort("balance"), Some("medium"));
        assert_eq!(parse_reasoning_effort("high"), Some("high"));
        assert_eq!(parse_reasoning_effort("think"), Some("high"));
        assert_eq!(parse_reasoning_effort("xhigh"), Some("xhigh"));
        assert_eq!(parse_reasoning_effort("max"), Some("xhigh"));
        assert_eq!(parse_reasoning_effort("mini"), Some("low"));
        assert_eq!(parse_reasoning_effort("none"), Some("none"));
        assert_eq!(effort_label("xhigh"), "Extra High");
        assert_eq!(effort_label("max"), "Extra High");
        assert_eq!(REASONING_EFFORTS.len(), 5);
        assert!(REASONING_EFFORTS
            .iter()
            .all(|(id, label)| *id != "max" && *label != "Max"));
    }

    /// 2.10.87: Minimal is off the ladder. Saved `minimal` / `mini` load as Low.
    #[test]
    fn minimal_effort_is_gone_and_loads_as_low() {
        let ids: Vec<&str> = REASONING_EFFORTS.iter().map(|(id, _)| *id).collect();
        assert_eq!(ids, vec!["none", "low", "medium", "high", "xhigh"]);
        assert!(REASONING_EFFORTS
            .iter()
            .all(|(_, label)| *label != "Minimal"));
        assert_eq!(parse_reasoning_effort("minimal"), Some("low"));
        assert_eq!(parse_reasoning_effort(" Minimal "), Some("low"));
        assert_eq!(parse_reasoning_effort("MINI"), Some("low"));
        assert_eq!(effort_label("minimal"), "Low");
        assert_eq!(parse_reasoning_effort("none"), Some("none"));
    }

    #[test]
    fn think_is_grok_4_7_high() {
        assert_eq!(model_for_mode("think"), "grok-4.7");
        assert_eq!(model_for_mode("build"), "grok-4.7");
        assert_eq!(model_for_mode("expert"), "grok-4.7");
        assert_eq!(reasoning_effort_for_mode("think"), Some("high"));
        assert_eq!(reasoning_effort_for_mode("max"), Some("xhigh"));
        assert_eq!(agent_reasoning_effort_for_mode("fast"), Some("low"));
        assert_eq!(agent_reasoning_effort_for_mode("balanced"), Some("medium"));
        assert_eq!(agent_reasoning_effort_for_mode("max"), Some("xhigh"));
        assert_eq!(reasoning_effort_for_mode("auto"), None);
        let think = chat_request_body_for_mode("think", &[("user".into(), "hi".into())]);
        assert_eq!(think["model"], "grok-4.7");
        assert_eq!(think["reasoning_effort"], "high");
        let max = chat_request_body_for_mode("max", &[("user".into(), "hi".into())]);
        assert_eq!(max["model"], "grok-4.7");
        assert_eq!(max["reasoning_effort"], "xhigh");
        assert_ne!(think["reasoning_effort"], max["reasoning_effort"]);
        let pinned = chat_request_body_vision(
            "grok-4.6",
            &[("user".into(), "hi".into())],
            None,
            Some("high"),
        );
        assert_eq!(pinned["reasoning_effort"], "high");
        assert_eq!(chat_timeout_secs(Some("high")), 600);
        assert_eq!(failover_model("grok-4.7"), Some("grok-4.3"));
        assert_eq!(failover_model("grok-4.6"), Some("grok-4.3"));
    }

    #[test]
    fn balance_is_grok_4_3() {
        assert_eq!(model_for_mode("balanced"), "grok-4.3");
        assert_eq!(model_for_mode("balance"), "grok-4.3");
        assert_eq!(reasoning_effort_for_mode("balanced"), None);
        assert_eq!(reasoning_effort_for_mode("think"), Some("high"));
        let body = chat_request_body_for_mode("balanced", &[("user".into(), "hi".into())]);
        assert_eq!(body["model"], "grok-4.3");
        assert!(body.get("reasoning_effort").is_none());
        assert_ne!(model_for_mode("think"), "grok-4.3");
        assert_eq!(failover_model("grok-4.3"), None);
    }

    #[test]
    fn fast_is_grok_4_3_with_no_reasoning() {
        assert_eq!(model_for_mode("fast"), "grok-4.3");
        assert_eq!(reasoning_effort_for_mode("fast"), Some("none"));
        assert_eq!(reasoning_effort_for_mode("auto"), None);
        let body = chat_request_body_for_mode("fast", &[("user".into(), "hi".into())]);
        assert_eq!(body["model"], "grok-4.3");
        assert_eq!(body["reasoning_effort"], "none");
        let resp = responses_request_body_images(
            "grok-4.3",
            &[("user".into(), "hi".into())],
            &[],
            Some("none"),
        );
        assert_eq!(resp["reasoning"]["effort"], "none");
        assert_eq!(quick_effort("grok-4.3"), Some("none"));
        assert_eq!(quick_effort(" grok-4.3 "), Some("none"));
        assert_eq!(quick_effort("grok-4.7"), None);
        assert_eq!(quick_effort(""), None);
    }

    #[test]
    fn failover_follows_new_mode_ladder() {
        assert_eq!(failover_model("grok-4.6"), Some("grok-4.3"));
        assert_eq!(failover_model("grok-4.3"), None);
        assert_eq!(failover_model("grok-3-mini-fast"), None);
        assert_eq!(failover_model("grok-3"), None);
        assert_eq!(failover_model("grok-4-latest"), Some("grok-4.3"));
        assert_eq!(crate::next_failover_tier("max"), "balanced");
        assert_eq!(crate::next_failover_tier("think"), "balanced");
        assert_eq!(crate::next_failover_tier("balanced"), "fast");
        assert_eq!(crate::next_failover_tier("fast"), "fast");
        assert_eq!(model_for_mode(crate::next_failover_tier("max")), "grok-4.3");
        assert_eq!(
            model_for_mode(crate::next_failover_tier("balanced")),
            "grok-4.3"
        );
    }

    #[test]
    fn failover_only_drops_grok_4_family() {
        assert_eq!(
            failover_model("grok-4-1-fast-non-reasoning"),
            Some("grok-4.3")
        );
        assert!(failover_model("gpt-4").is_none());
        assert!(failover_model("4").is_none());
        assert!(failover_model("grok-3").is_none());
    }
}
