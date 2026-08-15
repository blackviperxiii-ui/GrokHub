use serde_json::{json, Value};

pub const XAI_BASE: &str = "https://api.x.ai/v1";
pub const DEFAULT_MODEL: &str = "grok-3-mini-fast";

pub fn needs_auth_banner(has_key: bool) -> bool {
    !has_key
}

pub fn extract_host_cmds(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in text.lines() {
        let t = line.trim();
        let rest = t
            .strip_prefix("HOST_CMD:")
            .or_else(|| t.strip_prefix("HOST_CMD"));
        if let Some(rest) = rest {
            let cmd = rest.trim().trim_start_matches(':').trim();
            if !cmd.is_empty() {
                out.push(cmd.to_string());
            }
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
        "auto" => "grok-3-mini-fast",
        _ => DEFAULT_MODEL,
    }
}

/// Max and Think send Grok 4.6. Balance sends Grok 4.3. Other modes keep a pin, else the mode map.
pub fn resolve_chat_model(mode: &str, model: &str) -> String {
    match mode.trim() {
        "max" | "deep" | "heavy" | "think" | "build" | "expert" | "balanced" | "balance" => {
            model_for_mode(mode.trim()).to_string()
        }
        _ if !model.trim().is_empty() => model.trim().to_string(),
        mode if !mode.is_empty() => model_for_mode(mode).to_string(),
        _ => DEFAULT_MODEL.to_string(),
    }
}

pub fn failover_model(current: &str) -> Option<&'static str> {
    let tier = tier_of_model(current);
    match crate::next_failover_tier(tier) {
        "balanced" => Some("grok-3"),
        next if next != tier => Some(model_for_mode(next)),
        _ => None,
    }
}

fn tier_of_model(model: &str) -> &'static str {
    let m = model.to_ascii_lowercase();
    if m.contains("4") || m.contains("max") || m.contains("heavy") {
        "max"
    } else if m.contains("mini") || m.contains("fast") {
        "fast"
    } else {
        "balanced"
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn banner() {
        assert!(needs_auth_banner(false));
        assert!(!needs_auth_banner(true));
    }

    #[test]
    fn host_cmds() {
        let t = "Checking.\nHOST_CMD: ls /tmp\nHOST_CMD: cat README.md\n";
        assert_eq!(extract_host_cmds(t), vec!["ls /tmp", "cat README.md"]);
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
        assert!(should_failover_status(429));
        assert!(!should_failover_status(200));
        assert_eq!(failover_model("grok-4-latest"), Some("grok-3"));
        assert_eq!(failover_model("grok-4.6"), Some("grok-3"));
        assert!(failover_model(DEFAULT_MODEL).is_none());
        let max = chat_request_body_for_mode("max", &[("user".into(), "hi".into())]);
        assert_eq!(max["model"], "grok-4.6");
        assert_eq!(max["reasoning_effort"], "xhigh");
        let fast = chat_request_body(DEFAULT_MODEL, &[("user".into(), "hi".into())]);
        assert!(fast.get("reasoning_effort").is_none());
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
        assert_eq!(failover_model("grok-4.6"), Some("grok-3"));
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
        assert_eq!(failover_model("grok-4.3"), Some("grok-3"));
    }
}
