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
    chat_request_body_vision(model, messages, None)
}

pub fn chat_request_body_vision(
    model: &str,
    messages: &[(String, String)],
    image_data_url: Option<&str>,
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
    json!({
        "model": if model.is_empty() { DEFAULT_MODEL } else { model },
        "stream": false,
        "messages": msgs,
    })
}

pub fn should_failover_status(status: u16) -> bool {
    matches!(status, 401 | 403 | 429) || (500..600).contains(&status)
}

/// Permanent until Jeremy says otherwise: Think → Grok 4.3, Max → Grok 4.6.
pub fn model_for_mode(mode: &str) -> &'static str {
    match mode {
        "max" | "deep" | "heavy" => "grok-4.6",
        "thinking" | "think" => "grok-4.3",
        "balanced" | "build" | "expert" => "grok-3",
        "auto" => "grok-3-mini-fast",
        _ => DEFAULT_MODEL,
    }
}

pub fn failover_model(current: &str) -> Option<&'static str> {
    let tier = tier_of_model(current);
    match crate::next_failover_tier(tier) {
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
    }

    #[test]
    fn thinking_is_grok_4_3_and_max_is_grok_4_6() {
        assert_eq!(model_for_mode("thinking"), "grok-4.3");
        assert_eq!(model_for_mode("think"), "grok-4.3");
        assert_eq!(model_for_mode("max"), "grok-4.6");
        assert_eq!(model_for_mode("deep"), "grok-4.6");
        assert_eq!(model_for_mode("heavy"), "grok-4.6");
        assert_ne!(model_for_mode("thinking"), model_for_mode("max"));
        assert_ne!(model_for_mode("max"), "grok-4-latest");
    }
}
