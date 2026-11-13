//! Who is answering. The prompt names the assistant Grok, so a reply from the
//! device or a provider model used to insist it was Grok when asked which
//! model it is. A call the router sends anywhere but an xAI Grok model gets
//! one paragraph at the end of its first system message naming the model the
//! router actually picked, who serves it, and the reasoning effort the call is
//! sent with. The app shows neither anywhere, so asking in chat is how a user
//! finds out. Only the sent copy changes, never the session's history.

use grokhub_core::model_registry::discover::split_provider_model;

use crate::client::{ContentPart, InputItem};

/// Starts the identity paragraph.
pub const TAG: &str = "[Model identity]";
const OPENS: &str = "[Model identity] This reply comes from ";

/// The model as a reply should name it: `local_name` is the device's model
/// file (its GGUF name) when there is one.
pub fn describe(model: &str, local_name: Option<&str>) -> String {
    let m = model.trim();
    if super::local::is_local(m) {
        return match local_name.map(str::trim).filter(|n| !n.is_empty()) {
            Some(n) => format!("{n}, an open model running on this computer through llama.cpp"),
            None => "an open model running on this computer through llama.cpp".to_string(),
        };
    }
    match split_provider_model(m) {
        Some((provider, wire)) => {
            let host = provider_name(provider);
            if super::providers::is_openrouter_router(wire) {
                format!(
                    "a model that {host}'s router `{wire}` chose for this reply (name your own underlying model if you know it, and say it came through {host})"
                )
            } else {
                format!("`{wire}`, served through {host}")
            }
        }
        None => format!("`{m}` by xAI"),
    }
}

/// Who serves `model`, as a notice names it: the router's model never shows,
/// only where it runs ("xAI", "OpenRouter", "Local AI").
pub fn served_by(model: &str) -> String {
    let m = model.trim();
    if super::local::is_local(m) {
        return "Local AI".to_string();
    }
    match split_provider_model(m) {
        Some((provider, _)) => provider_name(provider),
        None => "xAI".to_string(),
    }
}

fn provider_name(id: &str) -> String {
    match id {
        "openrouter" => "OpenRouter".to_string(),
        "anthropic" => "Anthropic".to_string(),
        "openai" => "OpenAI".to_string(),
        other => other.to_string(),
    }
}

/// An xAI Grok model: the prompt's persona is already true.
pub fn is_grok(model: &str) -> bool {
    let m = model.trim();
    !super::local::is_local(m) && split_provider_model(m).is_none()
}

/// A Grok model reached through a provider (`openrouter/x-ai/grok-4`).
fn grok_family(model: &str) -> bool {
    split_provider_model(model.trim()).is_some_and(|(_, wire)| {
        let wire = wire.to_ascii_lowercase();
        wire.starts_with("x-ai/")
            || wire
                .rsplit('/')
                .next()
                .is_some_and(|s| s.starts_with("grok"))
    })
}

/// How the paragraph states the call's reasoning effort: empty when the call
/// sends none.
fn effort_words(effort: Option<&str>) -> String {
    match effort.map(str::trim).filter(|e| !e.is_empty()) {
        Some("none") => " with reasoning off".to_string(),
        Some(e) => format!(" at {} reasoning effort", grokhub_core::effort_label(e)),
        None => String::new(),
    }
}

/// The paragraph the model reads. `effort` is the reasoning effort the call
/// is sent with, if any.
pub fn line(model: &str, local_name: Option<&str>, effort: Option<&str>) -> String {
    let product = grokhub_core::brand::product_name();
    let who = describe(model, local_name);
    let how = effort_words(effort);
    let ask = if how.is_empty() {
        "When the user asks which model or AI is answering, name this model plainly"
    } else {
        "When the user asks which model or AI is answering, or at what effort, name this model and effort plainly"
    };
    if grok_family(model) {
        return format!("{OPENS}{who}, which {product} sent this turn to{how}. {ask}.");
    }
    format!(
        "{OPENS}{who}, which {product} sent this turn to{how}. \"Grok\" above is this app's assistant name, not your model. {ask}, and never insist you are Grok or made by xAI."
    )
}

/// Put [`line`] at the end of the first system message (one is added when
/// there is none), replacing a stamp this function left there before (only
/// its own trailing paragraph, never text that merely names the tag). One
/// system message stays one: some on-device chat templates take a single
/// system turn only. An xAI Grok model is left as it is.
pub fn stamp(
    input: &mut Vec<InputItem>,
    model: &str,
    local_name: Option<&str>,
    effort: Option<&str>,
) {
    if is_grok(model) {
        return;
    }
    let para = line(model, local_name, effort);
    for item in input.iter_mut() {
        let InputItem::Message { role, content } = item else {
            continue;
        };
        if role != "system" && role != "developer" {
            continue;
        }
        if let Some(ContentPart::InputText(text)) = content
            .iter_mut()
            .find(|c| matches!(c, ContentPart::InputText(_)))
        {
            if text.starts_with(OPENS) {
                text.clear();
            } else if let Some(at) = text.rfind(&format!("\n\n{OPENS}")) {
                text.truncate(at);
            }
            let kept = text.trim_end().len();
            text.truncate(kept);
            if !text.is_empty() {
                text.push_str("\n\n");
            }
            text.push_str(&para);
            return;
        }
    }
    input.insert(
        0,
        InputItem::Message {
            role: "system".into(),
            content: vec![ContentPart::InputText(para)],
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn system(text: &str) -> InputItem {
        InputItem::Message {
            role: "system".into(),
            content: vec![ContentPart::InputText(text.into())],
        }
    }

    fn user(text: &str) -> InputItem {
        InputItem::Message {
            role: "user".into(),
            content: vec![ContentPart::InputText(text.into())],
        }
    }

    fn text_of(item: &InputItem) -> &str {
        match item {
            InputItem::Message { content, .. } => match &content[0] {
                ContentPart::InputText(t) => t,
                _ => "",
            },
            _ => "",
        }
    }

    #[test]
    fn each_route_names_the_model_that_really_answers() {
        assert_eq!(
            describe("local:small", Some("Qwen3-8B-Q4_K_M")),
            "Qwen3-8B-Q4_K_M, an open model running on this computer through llama.cpp"
        );
        assert_eq!(
            describe("local:tiny", None),
            "an open model running on this computer through llama.cpp"
        );
        assert_eq!(
            describe("openrouter/nvidia/nemotron-3-ultra-550b-a55b:free", None),
            "`nvidia/nemotron-3-ultra-550b-a55b:free`, served through OpenRouter"
        );
        assert_eq!(
            describe("anthropic/claude-x", None),
            "`claude-x`, served through Anthropic"
        );
        assert!(describe("openrouter/openrouter/free", None)
            .starts_with("a model that OpenRouter's router `openrouter/free` chose"));
        assert_eq!(
            (
                served_by("grok-4.7"),
                served_by("local:small"),
                served_by("openrouter/vendor/model"),
                served_by("anthropic/claude-x"),
            ),
            (
                "xAI".to_string(),
                "Local AI".to_string(),
                "OpenRouter".to_string(),
                "Anthropic".to_string()
            )
        );
        assert!(is_grok("grok-4.7"));
        assert!(!is_grok("local:small"));
        assert!(!is_grok("openrouter/openrouter/auto"));
    }

    #[test]
    fn the_stamp_rides_the_first_system_message_once_and_skips_grok() {
        let mut input = vec![
            system("You are Grok inside Loadout."),
            user("which model are you?"),
        ];
        stamp(&mut input, "local:small", Some("Qwen3-8B"), None);
        assert_eq!(input.len(), 2, "no second system message");
        let first = text_of(&input[0]).to_string();
        assert!(first.starts_with("You are Grok inside Loadout.\n\n[Model identity] This reply comes from Qwen3-8B, an open model"), "{first}");
        assert!(first.contains("never insist you are Grok"));
        // A second stamp (xAI fell back to another route) replaces the first.
        stamp(&mut input, "openrouter/vendor/model", None, None);
        let again = text_of(&input[0]);
        assert_eq!(again.matches(TAG).count(), 1, "{again}");
        assert!(
            again.contains("`vendor/model`, served through OpenRouter"),
            "{again}"
        );
        assert!(!again.contains("Qwen3-8B"));
        assert_eq!(text_of(&input[1]), "which model are you?");
        // A Grok model keeps the prompt untouched.
        let mut grok = vec![system("You are Grok inside Loadout."), user("hi")];
        stamp(&mut grok, "grok-4.7", None, Some("high"));
        assert_eq!(text_of(&grok[0]), "You are Grok inside Loadout.");
        // No system message: one is added up front.
        let mut bare = vec![user("hi")];
        stamp(&mut bare, "anthropic/claude-x", None, None);
        assert_eq!(bare.len(), 2);
        assert!(text_of(&bare[0]).starts_with(TAG));
        assert_eq!(text_of(&bare[1]), "hi");
    }

    #[test]
    fn the_stamp_states_the_effort_the_call_is_sent_with() {
        let product = grokhub_core::brand::product_name();
        let mut with = vec![user("which model and effort are you on?")];
        stamp(&mut with, "openrouter/vendor/model", None, Some("high"));
        assert_eq!(
            text_of(&with[0]),
            format!(
                "[Model identity] This reply comes from `vendor/model`, served through OpenRouter, which {product} sent this turn to at High reasoning effort. \"Grok\" above is this app's assistant name, not your model. When the user asks which model or AI is answering, or at what effort, name this model and effort plainly, and never insist you are Grok or made by xAI."
            )
        );
        let mut without = vec![user("which model are you?")];
        stamp(&mut without, "openrouter/vendor/model", None, None);
        assert_eq!(
            text_of(&without[0]),
            format!(
                "[Model identity] This reply comes from `vendor/model`, served through OpenRouter, which {product} sent this turn to. \"Grok\" above is this app's assistant name, not your model. When the user asks which model or AI is answering, name this model plainly, and never insist you are Grok or made by xAI."
            )
        );
        assert_eq!(
            line("local:small", Some("Qwen3-8B"), Some("none")),
            format!(
                "[Model identity] This reply comes from Qwen3-8B, an open model running on this computer through llama.cpp, which {product} sent this turn to with reasoning off. \"Grok\" above is this app's assistant name, not your model. When the user asks which model or AI is answering, or at what effort, name this model and effort plainly, and never insist you are Grok or made by xAI."
            )
        );
        assert_eq!(
            line("openrouter/x-ai/grok-4", None, Some("low")),
            format!(
                "[Model identity] This reply comes from `x-ai/grok-4`, served through OpenRouter, which {product} sent this turn to at Low reasoning effort. When the user asks which model or AI is answering, or at what effort, name this model and effort plainly."
            )
        );
    }

    #[test]
    fn grok_through_a_provider_and_the_tag_in_a_prompt() {
        // Grok through OpenRouter is still Grok: no "never insist you are Grok".
        let grok = line("openrouter/x-ai/grok-4", None, None);
        assert!(
            grok.contains("`x-ai/grok-4`, served through OpenRouter"),
            "{grok}"
        );
        assert!(!grok.contains("never insist"), "{grok}");
        // A prompt that merely names the tag keeps its text; only a stamp goes.
        let mut input = vec![
            system("Rules mention [Model identity] lines. Keep this."),
            user("hi"),
        ];
        stamp(&mut input, "local:small", None, None);
        stamp(&mut input, "local:small", None, None);
        let text = text_of(&input[0]);
        assert!(text.starts_with("Rules mention [Model identity] lines. Keep this.\n\n[Model identity] This reply comes from"), "{text}");
        assert_eq!(text.matches("This reply comes from").count(), 1, "{text}");
        assert!(text.contains(", which "), "{text}");
    }
}
