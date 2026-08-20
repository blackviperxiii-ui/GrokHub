use crate::host_plan::strip_host_cmd_line;
use serde_json::{json, Value};

pub const XAI_BASE: &str = "https://api.x.ai/v1";
pub const DEFAULT_MODEL: &str = "grok-3-mini-fast";
/// Greeting, chips, and other cabin Fast-path calls. Not the composer ladder.
/// Product name: Grok 4.1 Fast. Live API id still accepted after retirement.
pub const CABIN_FAST_MODEL: &str = "grok-4-1-fast-non-reasoning";
/// Used only if 4.1 Fast returns empty (retired alias). Fast non-reasoning successor.
pub const CABIN_FAST_FALLBACK: &str = "grok-4.20-0309-non-reasoning";

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
    let mut msgs: Vec<Value> = messages
        .iter()
        .map(|(role, content)| json!({ "role": role, "content": content }))
        .collect();
    if let Some(url) = image_data_url.filter(|s| s.starts_with("data:image")) {
        if let Some(last) = msgs.last_mut() {
            if last["role"] == "user" {
                let text = last["content"].as_str().unwrap_or("").to_string();
                last["content"] = json!([
                    { "type": "text", "text": text },
                    { "type": "image_url", "image_url": { "url": url } }
                ]);
            }
        }
    }
    let resolved = if model.is_empty() { DEFAULT_MODEL } else { model };
    let mut body = json!({
        "model": resolved,
        "stream": false,
        "messages": msgs,
    });
    if resolved == "grok-4.6" {
        if let Some(effort) = effort {
            body["reasoning_effort"] = json!(effort);
        }
    }
    body
}

/// Think is Grok 4.6 at high. Max is the same model at xhigh. Balance leaves effort unset.
pub fn reasoning_effort_for_mode(mode: &str) -> Option<&'static str> {
    match mode.trim() {
        "max" | "deep" | "heavy" => Some("xhigh"),
        "think" | "build" | "expert" => Some("high"),
        _ => None,
    }
}

pub fn chat_timeout_secs(effort: Option<&str>) -> u64 {
    match effort {
        Some("high") | Some("xhigh") => 600,
        _ => 120,
    }
}

pub fn should_failover_status(status: u16) -> bool {
    matches!(status, 401 | 403 | 429) || (500..600).contains(&status)
}

pub fn model_for_mode(mode: &str) -> &'static str {
    match mode {
        "max" | "deep" | "heavy" => "grok-4.6",
        "think" | "build" | "expert" => "grok-4.6",
        "balanced" | "balance" => "grok-4.3",
        "auto" | "fast" => "grok-3-mini-fast",
        _ => DEFAULT_MODEL,
    }
}

/// Max and Think send Grok 4.6. Balance sends Grok 4.3. Fast sends mini.
/// Auto keeps a real Settings pin; leftover ladder defaults do not count.
pub fn resolve_chat_model(mode: &str, model: &str) -> String {
    match mode.trim() {
        "max" | "deep" | "heavy" | "think" | "build" | "expert" | "balanced" | "balance"
        | "fast" => model_for_mode(mode.trim()).to_string(),
        _ if settings_pin_blocks_auto(model) => {
            crate::models::sanitize_chat_model(model).to_string()
        }
        mode if !mode.is_empty() => model_for_mode(mode).to_string(),
        _ => DEFAULT_MODEL.to_string(),
    }
}

/// Models the composer ladder already owns. `/mode` used to write these into
/// the Settings pin, which made Auto look pinned and never route.
pub fn is_composer_ladder_model(model: &str) -> bool {
    matches!(model.trim(), "grok-3-mini-fast" | "grok-4.3" | "grok-4.6")
}

/// A Settings chat-model pin skips Auto. Ladder defaults do not count as a pin.
pub fn settings_pin_blocks_auto(pinned_model: &str) -> bool {
    let pin = pinned_model.trim();
    !pin.is_empty() && !is_composer_ladder_model(pin)
}

/// Auto picks Fast / Balance / Think / Max from the ask. A Settings chat-model pin skips this.
pub fn route_auto_mode(prompt: &str) -> &'static str {
    let t = prompt.to_ascii_lowercase();
    let len = prompt.chars().count();
    if len > 4000 || contains_any(&t, MAX_ROUTE_SIGNALS) {
        return "max";
    }
    if len > 800 || contains_any(&t, THINK_ROUTE_SIGNALS) {
        return "think";
    }
    if len > 160 || contains_any(&t, BALANCE_ROUTE_SIGNALS) {
        return "balanced";
    }
    "fast"
}

const MAX_ROUTE_SIGNALS: &[&str] = &[
    "think as hard as possible",
    "maximum effort",
    "deepest model",
    "xhigh",
];

const THINK_ROUTE_SIGNALS: &[&str] = &[
    "architect",
    "root cause",
    "step by step",
    "debug",
    "refactor",
    "implement",
    "design a",
    "prove ",
];

const BALANCE_ROUTE_SIGNALS: &[&str] = &[
    "explain",
    "compare",
    "why ",
    "how does",
    "review",
    "write a",
];

fn contains_any(hay: &str, needles: &[&str]) -> bool {
    needles.iter().any(|n| hay.contains(n))
}

pub fn effective_chat_mode(mode: &str, prompt: &str, pinned_model: &str) -> String {
    let mode = mode.trim();
    let mode = if mode.is_empty() { "auto" } else { mode };
    if matches!(mode, "auto" | "adaptive" | "smart") && !settings_pin_blocks_auto(pinned_model) {
        route_auto_mode(prompt).to_string()
    } else {
        mode.to_string()
    }
}

pub fn failover_model(current: &str) -> Option<&'static str> {
    let tier = tier_of_model(current);
    let next = crate::next_failover_tier(tier);
    if next == tier {
        None
    } else {
        Some(model_for_mode(next))
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
    let resolved = if model.is_empty() { DEFAULT_MODEL } else { model };
    let mut input: Vec<Value> = messages
        .iter()
        .map(|(role, content)| json!({ "role": role, "content": content }))
        .collect();
    if let Some(url) = image_data_url.filter(|s| s.starts_with("data:image")) {
        if let Some(last) = input.last_mut() {
            if last["role"] == "user" {
                let text = last["content"].as_str().unwrap_or("").to_string();
                last["content"] = json!([
                    { "type": "input_text", "text": text },
                    { "type": "input_image", "image_url": url }
                ]);
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
        assert_eq!(parse_chat_reasoning(&reasoned).as_deref(), Some("Need a snapshot."));
        assert!(should_failover_status(429));
        assert!(!should_failover_status(200));
        assert_eq!(failover_model("grok-4-latest"), Some("grok-4.3"));
        assert_eq!(failover_model("grok-4.6"), Some("grok-4.3"));
        assert!(failover_model(DEFAULT_MODEL).is_none());
        let max = chat_request_body_for_mode("max", &[("user".into(), "hi".into())]);
        assert_eq!(max["model"], "grok-4.6");
        assert_eq!(max["reasoning_effort"], "xhigh");
        let fast = chat_request_body(DEFAULT_MODEL, &[("user".into(), "hi".into())]);
        assert!(fast.get("reasoning_effort").is_none());
        let streamed = responses_request_body(
            "grok-4.6",
            &[("user".into(), "hi".into())],
            None,
            Some("xhigh"),
        );
        assert_eq!(streamed["model"], "grok-4.6");
        assert_eq!(streamed["input"][0]["content"], "hi");
        assert_eq!(streamed["store"], false);
        assert_eq!(streamed["reasoning"]["effort"], "xhigh");
        assert_eq!(responses_url(), "https://api.x.ai/v1/responses");
        let vis = responses_request_body(
            "grok-4.6",
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
        assert_eq!(parse_responses_reasoning(&reasoned).as_deref(), Some("Need a snapshot."));
        assert_eq!(parse_model_text(&reply).as_deref(), Some("hello"));
        assert_eq!(parse_model_text(&json!({
            "choices": [{ "message": { "content": "legacy" } }]
        })).as_deref(), Some("legacy"));
        assert_eq!(chat_timeout_secs(Some("xhigh")), 600);
        assert_eq!(chat_timeout_secs(None), 120);
    }

    #[test]
    fn max_is_grok_4_6_xhigh() {
        assert_eq!(model_for_mode("max"), "grok-4.6");
        assert_eq!(model_for_mode("deep"), "grok-4.6");
        assert_eq!(model_for_mode("heavy"), "grok-4.6");
        assert_eq!(reasoning_effort_for_mode("max"), Some("xhigh"));
        assert_ne!(model_for_mode("max"), "grok-4-latest");
        assert_eq!(resolve_chat_model("max", "grok-3"), "grok-4.6");
        assert_eq!(resolve_chat_model("max", ""), "grok-4.6");
        assert_eq!(resolve_chat_model("auto", "grok-3"), "grok-3");
        assert_eq!(resolve_chat_model("auto", ""), "grok-3-mini-fast");
        assert_eq!(
            resolve_chat_model("auto", "gpt-4"),
            DEFAULT_MODEL,
            "invalid Settings pin must not be sent to the API"
        );
        assert_eq!(
            reasoning_effort_for_mode("max"),
            Some("xhigh")
        );
    }

    #[test]
    fn think_is_grok_4_6_high() {
        assert_eq!(model_for_mode("think"), "grok-4.6");
        assert_eq!(model_for_mode("build"), "grok-4.6");
        assert_eq!(model_for_mode("expert"), "grok-4.6");
        assert_eq!(resolve_chat_model("think", "grok-3"), "grok-4.6");
        assert_eq!(resolve_chat_model("think", ""), "grok-4.6");
        assert_eq!(reasoning_effort_for_mode("think"), Some("high"));
        assert_eq!(reasoning_effort_for_mode("max"), Some("xhigh"));
        assert_eq!(reasoning_effort_for_mode("auto"), None);
        let think = chat_request_body_for_mode("think", &[("user".into(), "hi".into())]);
        assert_eq!(think["model"], "grok-4.6");
        assert_eq!(think["reasoning_effort"], "high");
        let max = chat_request_body_for_mode("max", &[("user".into(), "hi".into())]);
        assert_eq!(max["model"], "grok-4.6");
        assert_eq!(max["reasoning_effort"], "xhigh");
        assert_ne!(think["reasoning_effort"], max["reasoning_effort"]);
        assert_eq!(chat_timeout_secs(Some("high")), 600);
        assert_eq!(failover_model("grok-4.6"), Some("grok-4.3"));
    }

    #[test]
    fn balance_is_grok_4_3() {
        assert_eq!(model_for_mode("balanced"), "grok-4.3");
        assert_eq!(model_for_mode("balance"), "grok-4.3");
        assert_eq!(resolve_chat_model("balanced", "grok-4.6"), "grok-4.3");
        assert_eq!(resolve_chat_model("balance", ""), "grok-4.3");
        assert_eq!(reasoning_effort_for_mode("balanced"), None);
        assert_eq!(reasoning_effort_for_mode("think"), Some("high"));
        let body = chat_request_body_for_mode("balanced", &[("user".into(), "hi".into())]);
        assert_eq!(body["model"], "grok-4.3");
        assert!(body.get("reasoning_effort").is_none());
        assert_ne!(model_for_mode("think"), "grok-4.3");
        assert_eq!(failover_model("grok-4.3"), Some("grok-3-mini-fast"));
    }

    #[test]
    fn failover_follows_new_mode_ladder() {
        assert_eq!(failover_model("grok-4.6"), Some("grok-4.3"));
        assert_eq!(failover_model("grok-4.3"), Some("grok-3-mini-fast"));
        assert_eq!(failover_model("grok-3-mini-fast"), None);
        assert_eq!(failover_model("grok-3"), None);
        assert_eq!(failover_model("grok-4-latest"), Some("grok-4.3"));
        assert_eq!(crate::next_failover_tier("max"), "balanced");
        assert_eq!(crate::next_failover_tier("think"), "balanced");
        assert_eq!(crate::next_failover_tier("balanced"), "fast");
        assert_eq!(crate::next_failover_tier("fast"), "fast");
        assert_eq!(model_for_mode(crate::next_failover_tier("max")), "grok-4.3");
        assert_eq!(model_for_mode(crate::next_failover_tier("balanced")), "grok-3-mini-fast");
    }

    #[test]
    fn auto_routes_among_new_modes() {
        assert_eq!(route_auto_mode("hi"), "fast");
        assert_eq!(route_auto_mode("thanks"), "fast");
        assert_eq!(
            route_auto_mode("explain how rust ownership works in detail"),
            "balanced"
        );
        assert_eq!(
            route_auto_mode("architect a host-tool plan and implement the first slice"),
            "think"
        );
        assert_eq!(
            route_auto_mode("debug the root cause step by step and refactor the auth path"),
            "think"
        );
        assert_eq!(
            route_auto_mode("think as hard as possible about this proof"),
            "max"
        );
        assert_eq!(model_for_mode(route_auto_mode("hi")), "grok-3-mini-fast");
        assert_eq!(reasoning_effort_for_mode(route_auto_mode("hi")), None);
        assert_eq!(
            model_for_mode(route_auto_mode(
                "architect a host-tool plan and implement the first slice"
            )),
            "grok-4.6"
        );
        assert_eq!(
            reasoning_effort_for_mode(route_auto_mode(
                "architect a host-tool plan and implement the first slice"
            )),
            Some("high")
        );
        assert_eq!(
            reasoning_effort_for_mode(route_auto_mode(
                "think as hard as possible about this proof"
            )),
            Some("xhigh")
        );
        assert_eq!(effective_chat_mode("auto", "hi", ""), "fast");
        assert_eq!(
            effective_chat_mode("", "architect a host-tool plan and implement the first slice", ""),
            "think",
            "empty composer mode is Auto"
        );
        assert_eq!(
            resolve_chat_model(&effective_chat_mode("", "hi", ""), ""),
            "grok-3-mini-fast"
        );
        assert_eq!(effective_chat_mode("auto", "hi", "grok-3"), "auto");
        assert_eq!(
            effective_chat_mode(
                "auto",
                "architect a host-tool plan and implement the first slice",
                ""
            ),
            "think"
        );
        assert_eq!(effective_chat_mode("think", "hi", ""), "think");
        assert_eq!(effective_chat_mode("max", "hi", ""), "max");
        assert_eq!(effective_chat_mode("balanced", "hi", ""), "balanced");
    }

    #[test]
    fn auto_routes_when_pin_is_a_ladder_default() {
        assert_eq!(
            effective_chat_mode(
                "auto",
                "architect a host-tool plan and implement the first slice",
                "grok-3-mini-fast"
            ),
            "think"
        );
        assert_eq!(effective_chat_mode("auto", "hi", "grok-3-mini-fast"), "fast");
        assert_eq!(
            effective_chat_mode("auto", "explain how rust ownership works in detail", "grok-4.3"),
            "balanced"
        );
        assert_eq!(
            effective_chat_mode("auto", "think as hard as possible about this proof", "grok-4.6"),
            "max"
        );
        assert_eq!(
            effective_chat_mode(
                "auto",
                "architect a host-tool plan and implement the first slice",
                "grok-3"
            ),
            "auto"
        );
        assert_eq!(effective_chat_mode("auto", "hi", "grok-4-latest"), "auto");
        assert_eq!(
            resolve_chat_model(&effective_chat_mode("auto", "hi", "grok-4.6"), "grok-4.6"),
            "grok-3-mini-fast"
        );
        assert_eq!(resolve_chat_model("fast", "grok-4.6"), "grok-3-mini-fast");
        assert_eq!(resolve_chat_model("fast", "grok-3"), "grok-3-mini-fast");
        assert!(settings_pin_blocks_auto("grok-3"));
        assert!(settings_pin_blocks_auto("grok-4-latest"));
        assert!(!settings_pin_blocks_auto("grok-3-mini-fast"));
        assert!(!settings_pin_blocks_auto("grok-4.3"));
        assert!(!settings_pin_blocks_auto("grok-4.6"));
        assert!(!settings_pin_blocks_auto(""));
        assert!(is_composer_ladder_model("grok-4.6"));
        assert!(!is_composer_ladder_model("grok-3"));
    }

    #[test]
    fn failover_only_drops_grok_4_family() {
        assert_eq!(failover_model("grok-4-1-fast-non-reasoning"), Some("grok-4.3"));
        assert!(failover_model("gpt-4").is_none());
        assert!(failover_model("4").is_none());
        assert!(failover_model("grok-3").is_none());
    }
}
