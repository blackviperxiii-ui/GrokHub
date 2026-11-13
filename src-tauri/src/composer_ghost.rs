//! Sentence autocomplete for the Tauri composer: a short Grok continuation
//! when Behavior → Sentence autocomplete is on.

use std::path::Path;
use std::time::Duration;

use grokhub_agent::{CancelToken, ContentPart, InputItem, ModelClient, ResponsesRequest};

const SYSTEM: &str =
    "Complete the user's partial chat message. Reply with only the continuation (no quotes, no preamble). One short line only. Do not repeat text they already typed.";

const MIN_DRAFT_CHARS: usize = 3;

/// One short continuation for `draft`, or empty when the setting is off, the
/// draft is too short / a slash, or there is no sign-in.
pub fn suggest(
    config_dir: &Path,
    draft: &str,
    client: &dyn ModelClient,
    model: &str,
) -> Result<String, String> {
    let behavior = crate::settings::load(config_dir).behavior;
    if !behavior.autocomplete {
        return Ok(String::new());
    }
    let t = draft.trim();
    if t.is_empty() || t.starts_with('/') || t.len() < MIN_DRAFT_CHARS {
        return Ok(String::new());
    }
    let messages = [
        ("system".to_string(), SYSTEM.to_string()),
        (
            "user".to_string(),
            format!("Continue this draft:\n{draft}"),
        ),
    ];
    let raw = ask(client, model, &messages)?;
    Ok(normalize_suggestion(&raw, draft))
}

fn ask(
    client: &dyn ModelClient,
    model: &str,
    messages: &[(String, String)],
) -> Result<String, String> {
    let req = ResponsesRequest {
        text_format: None,
        model: model.to_string(),
        effort: Some(grokhub_core::BACKGROUND_EFFORT.to_string()),
        input: messages
            .iter()
            .map(|(role, text)| InputItem::Message {
                role: role.clone(),
                content: vec![ContentPart::InputText(text.clone())],
            })
            .collect(),
        conversation_id: "composer-ghost".into(),
        tools: Vec::new(),
        hosted_search: false,
        call_timeout: Some(Duration::from_secs(30)),
    };
    let out = client
        .stream(&req, &CancelToken::new(), &mut |_| {})
        .map_err(|e| e.to_string())?;
    Ok(out.text.trim().to_string())
}

pub fn normalize_suggestion(raw: &str, draft: &str) -> String {
    let line = raw
        .lines()
        .next()
        .unwrap_or("")
        .trim()
        .trim_matches('"')
        .trim_matches('\'')
        .trim();
    if line.is_empty() {
        return String::new();
    }
    let draft_trim = draft.trim_end();
    line.strip_prefix(draft_trim)
        .map(|tail| tail.trim_start().to_string())
        .unwrap_or_else(|| line.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_strips_echo_and_quotes() {
        assert_eq!(normalize_suggestion("\" world\"", "hello"), "world");
        assert_eq!(normalize_suggestion("hello world", "hello"), "world");
        assert_eq!(normalize_suggestion("  world\nmore", "hello"), "world");
        assert_eq!(normalize_suggestion("", "hello"), "");
    }
}
