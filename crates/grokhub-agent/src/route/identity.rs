//! Who is answering. The prompt names the assistant Grok, so a reply from the
//! device or a provider model used to insist it was Grok when asked which
//! model it is. A call the router sends anywhere but an xAI Grok model gets
//! one paragraph at the end of its first system message naming the model the
//! router actually picked, and who serves it. Only the sent copy changes, never
//! the session's history.

use grokhub_core::model_registry::discover::split_provider_model;

use crate::client::{ContentPart, InputItem};

/// Starts the identity paragraph.
pub const TAG: &str = "[Model identity]";
const OPENS: &str = "[Model identity] This reply comes from ";

/// The model as a reply should name it: `local_name` is the device's model
/// file (its GGUF name) when there is one; `fallback` is OpenRouter's
/// fallback switch, which may hand the call to its router instead.
pub fn describe(model: &str, local_name: Option<&str>, fallback: bool) -> String {
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
            } else if fallback && provider == "openrouter" {
                format!(
                    "`{wire}` through {host} (or, if it had no endpoint, a model {host}'s router picked instead: then name your own model if you know it)"
                )
            } else {
                format!("`{wire}`, served through {host}")
            }
        }
        None => format!("`{m}` by xAI"),
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

/// The paragraph the model reads.
pub fn line(model: &str, local_name: Option<&str>, fallback: bool) -> String {
    let product = grokhub_core::brand::product_name();
    let who = describe(model, local_name, fallback);
    if grok_family(model) {
        return format!(
            "{OPENS}{who}, which {product} sent this turn to. When the user asks which model or AI is answering, name this model plainly."
        );
    }
    format!(
        "{OPENS}{who}, which {product} sent this turn to. \"Grok\" above is this app's assistant name, not your model. When the user asks which model or AI is answering, name this model plainly, and never insist you are Grok or made by xAI."
    )
}

/// Put [`line`] at the end of the first system message (one is added when
/// there is none), replacing a stamp this function left there before (only
/// its own trailing paragraph, never text that merely names the tag). One
/// system message stays one: some on-device chat templates take a single
/// system turn only. An xAI Grok model is left as it is.
pub fn stamp(input: &mut Vec<InputItem>, model: &str, local_name: Option<&str>, fallback: bool) {
    if is_grok(model) {
        return;
    }
    let para = line(model, local_name, fallback);
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
            describe("local:small", Some("Qwen3-8B-Q4_K_M"), false),
            "Qwen3-8B-Q4_K_M, an open model running on this computer through llama.cpp"
        );
        assert_eq!(
            describe("local:tiny", None, false),
            "an open model running on this computer through llama.cpp"
        );
        assert_eq!(
            describe(
                "openrouter/nvidia/nemotron-3-ultra-550b-a55b:free",
                None,
                false
            ),
            "`nvidia/nemotron-3-ultra-550b-a55b:free`, served through OpenRouter"
        );
        assert_eq!(
            describe("anthropic/claude-x", None, false),
            "`claude-x`, served through Anthropic"
        );
        assert!(describe("openrouter/openrouter/free", None, false)
            .starts_with("a model that OpenRouter's router `openrouter/free` chose"));
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
        stamp(&mut input, "local:small", Some("Qwen3-8B"), false);
        assert_eq!(input.len(), 2, "no second system message");
        let first = text_of(&input[0]).to_string();
        assert!(first.starts_with("You are Grok inside Loadout.\n\n[Model identity] This reply comes from Qwen3-8B, an open model"), "{first}");
        assert!(first.contains("never insist you are Grok"));
        // A second stamp (xAI fell back to another route) replaces the first.
        stamp(&mut input, "openrouter/vendor/model", None, false);
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
        stamp(&mut grok, "grok-4.7", None, false);
        assert_eq!(text_of(&grok[0]), "You are Grok inside Loadout.");
        // No system message: one is added up front.
        let mut bare = vec![user("hi")];
        stamp(&mut bare, "anthropic/claude-x", None, false);
        assert_eq!(bare.len(), 2);
        assert!(text_of(&bare[0]).starts_with(TAG));
        assert_eq!(text_of(&bare[1]), "hi");
    }

    #[test]
    fn grok_through_a_provider_the_fallback_router_and_the_tag_in_a_prompt() {
        // Grok through OpenRouter is still Grok: no "never insist you are Grok".
        let grok = line("openrouter/x-ai/grok-4", None, false);
        assert!(
            grok.contains("`x-ai/grok-4`, served through OpenRouter"),
            "{grok}"
        );
        assert!(!grok.contains("never insist"), "{grok}");
        // With OpenRouter's fallback on, the router may have answered.
        let fb = describe("openrouter/vendor/model", None, true);
        assert!(
            fb.starts_with("`vendor/model` through OpenRouter (or, if it had no endpoint"),
            "{fb}"
        );
        assert_eq!(
            describe("anthropic/claude-x", None, true),
            "`claude-x`, served through Anthropic"
        );
        // A prompt that merely names the tag keeps its text; only a stamp goes.
        let mut input = vec![
            system("Rules mention [Model identity] lines. Keep this."),
            user("hi"),
        ];
        stamp(&mut input, "local:small", None, false);
        stamp(&mut input, "local:small", None, false);
        let text = text_of(&input[0]);
        assert!(text.starts_with("Rules mention [Model identity] lines. Keep this.\n\n[Model identity] This reply comes from"), "{text}");
        assert_eq!(text.matches("This reply comes from").count(), 1, "{text}");
        assert!(text.contains(", which "), "{text}");
    }
}
