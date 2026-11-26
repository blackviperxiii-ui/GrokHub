//! Grants, failures, and successful tool results kept for one conversation.
//! Project files still hold only safe shell grants. This map is the rest:
//! the same tool and arguments, for the rest of that chat, including the
//! next user turn. It never allows a call the gate refused. An identical
//! successful call (same tool, same arguments) reuses the earlier result.

use std::sync::Mutex;

const CHATS: usize = 256;

/// Shown once when the project file could not store an Allow always grant.
pub const GRANT_NOT_SAVED: &str = "Allow always was not saved for this project: ";

struct Grant {
    tool: String,
    args: String,
    any_args: bool,
}

struct Fail {
    tool: String,
    args: String,
    text: String,
}

struct Cached {
    tool: String,
    args: String,
    text: String,
    image: Option<String>,
}

#[derive(Default)]
struct Chat {
    grants: Vec<Grant>,
    failures: Vec<Fail>,
    results: Vec<Cached>,
    store_error_shown: bool,
}

/// An earlier successful tool result for these arguments.
pub struct HeldResult {
    pub text: String,
    pub image: Option<String>,
}

/// An earlier `image_generate` result for these arguments.
pub type HeldImage = HeldResult;

fn book() -> std::sync::MutexGuard<'static, Vec<(String, Chat)>> {
    static BOOK: Mutex<Vec<(String, Chat)>> = Mutex::new(Vec::new());
    BOOK.lock().unwrap_or_else(|err| err.into_inner())
}

fn chat<'a>(book: &'a mut Vec<(String, Chat)>, id: &str) -> &'a mut Chat {
    if let Some(at) = book.iter().position(|(key, _)| key == id) {
        return &mut book[at].1;
    }
    if book.len() >= CHATS {
        book.remove(0);
    }
    book.push((id.to_string(), Chat::default()));
    &mut book.last_mut().unwrap().1
}

/// Drop one conversation. Tests isolate chats that share a process.
pub fn clear_conversation(id: &str) {
    book().retain(|(key, _)| key != id);
}

/// Remember Allow always for this conversation. `any_args` is web_search:
/// one yes covers every query. The egress guard still runs on each hop.
pub fn remember_session(id: &str, tool: &str, args: &str, any_args: bool) {
    if id.is_empty() || tool.is_empty() {
        return;
    }
    let mut book = book();
    let chat = chat(&mut book, id);
    if any_args {
        if chat
            .grants
            .iter()
            .any(|grant| grant.tool == tool && grant.any_args)
        {
            return;
        }
        chat.grants.push(Grant {
            tool: tool.to_string(),
            args: String::new(),
            any_args: true,
        });
        return;
    }
    if chat
        .grants
        .iter()
        .any(|grant| grant.tool == tool && (grant.any_args || grant.args == args))
    {
        return;
    }
    chat.grants.push(Grant {
        tool: tool.to_string(),
        args: args.to_string(),
        any_args: false,
    });
}

pub fn session_covers(id: &str, tool: &str, args: &str) -> bool {
    if id.is_empty() {
        return false;
    }
    book()
        .iter()
        .find(|(key, _)| key == id)
        .is_some_and(|(_, chat)| {
            chat.grants
                .iter()
                .any(|grant| grant.tool == tool && (grant.any_args || grant.args == args))
        })
}

pub fn note_failure(id: &str, tool: &str, args: &str, text: &str) {
    if id.is_empty() {
        return;
    }
    let mut book = book();
    let chat = chat(&mut book, id);
    if let Some(fail) = chat
        .failures
        .iter_mut()
        .find(|fail| fail.tool == tool && fail.args == args)
    {
        fail.text = text.to_string();
        return;
    }
    chat.failures.push(Fail {
        tool: tool.to_string(),
        args: args.to_string(),
        text: text.to_string(),
    });
}

pub fn earlier_failure(id: &str, tool: &str, args: &str) -> Option<String> {
    book()
        .iter()
        .find(|(key, _)| key == id)
        .and_then(|(_, chat)| {
            chat.failures
                .iter()
                .find(|fail| fail.tool == tool && fail.args == args)
                .map(|fail| fail.text.clone())
        })
}

/// Remember a successful tool result for this conversation. The next identical
/// call (same tool, same arguments) returns it without running again.
pub fn remember_result(id: &str, tool: &str, args: &str, text: &str, image: Option<&str>) {
    if id.is_empty() || tool.is_empty() {
        return;
    }
    let mut book = book();
    let chat = chat(&mut book, id);
    if let Some(held) = chat
        .results
        .iter_mut()
        .find(|held| held.tool == tool && held.args == args)
    {
        held.text = text.to_string();
        held.image = image.map(str::to_string);
        return;
    }
    chat.results.push(Cached {
        tool: tool.to_string(),
        args: args.to_string(),
        text: text.to_string(),
        image: image.map(str::to_string),
    });
}

pub fn earlier_result(id: &str, tool: &str, args: &str) -> Option<HeldResult> {
    book()
        .iter()
        .find(|(key, _)| key == id)
        .and_then(|(_, chat)| {
            chat.results
                .iter()
                .find(|held| held.tool == tool && held.args == args)
                .map(|held| HeldResult {
                    text: held.text.clone(),
                    image: held.image.clone(),
                })
        })
}

pub fn remember_image(id: &str, args: &str, text: &str, image: Option<&str>) {
    remember_result(id, "image_generate", args, text, image);
}

pub fn earlier_image(id: &str, args: &str) -> Option<HeldImage> {
    earlier_result(id, "image_generate", args)
}

/// Log a project-grant write failure and return the line to show once.
pub fn note_store_error(id: &str, err: &str) -> Option<String> {
    eprintln!("grokhub grant: allow-always was not stored: {err}");
    if id.is_empty() {
        return Some(format!("{GRANT_NOT_SAVED}{err}"));
    }
    let mut book = book();
    let chat = chat(&mut book, id);
    if chat.store_error_shown {
        return None;
    }
    chat.store_error_shown = true;
    Some(format!("{GRANT_NOT_SAVED}{err}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allow_always_matches_the_same_arguments_and_web_search_any_query() {
        let id = "sess-ac2-literal";
        clear_conversation(id);
        let write = r#"{"path":"a.txt","content":"a"}"#;
        remember_session(id, "write", write, false);
        assert!(session_covers(id, "write", write));
        assert!(!session_covers(
            id,
            "write",
            r#"{"path":"b.txt","content":"b"}"#
        ));
        remember_session(id, "web_search", r#"{"query":"rust release"}"#, true);
        assert!(session_covers(
            id,
            "web_search",
            r#"{"query":"python wheels"}"#
        ));
        clear_conversation(id);
        assert!(!session_covers(id, "write", write));
    }

    #[test]
    fn an_earlier_failure_and_result_come_back_for_the_same_arguments() {
        let id = "sess-ac4-img-9";
        clear_conversation(id);
        let args = r#"{"prompt":"a red fox in snow"}"#;
        note_failure(
            id,
            "write",
            r#"{"path":"a.txt","content":"a"}"#,
            "disk full",
        );
        assert_eq!(
            earlier_failure(id, "write", r#"{"path":"a.txt","content":"a"}"#).as_deref(),
            Some("disk full")
        );
        assert!(earlier_failure(id, "write", r#"{"path":"b.txt","content":"b"}"#).is_none());
        remember_image(
            id,
            args,
            "IMAGINE: https://cdn.example/fox.png",
            Some("file:///earlier/fox.png"),
        );
        let again = earlier_image(id, args).unwrap();
        assert_eq!(again.text, "IMAGINE: https://cdn.example/fox.png");
        assert_eq!(again.image.as_deref(), Some("file:///earlier/fox.png"));
        assert!(earlier_image(id, r#"{"prompt":"a blue whale"}"#).is_none());
        remember_result(
            id,
            "web_search",
            r#"{"query":"rust"}"#,
            "<<<UNTRUSTED source=web_search>>>\nrust hits",
            None,
        );
        assert_eq!(
            earlier_result(id, "web_search", r#"{"query":"rust"}"#)
                .unwrap()
                .text,
            "<<<UNTRUSTED source=web_search>>>\nrust hits"
        );
        assert!(earlier_result(id, "web_search", r#"{"query":"go"}"#).is_none());
        clear_conversation(id);
    }

    #[test]
    fn a_grant_store_error_is_shown_once() {
        let id = "sess-ac3-store";
        clear_conversation(id);
        let first = note_store_error(id, "permission denied");
        assert_eq!(
            first.as_deref(),
            Some("Allow always was not saved for this project: permission denied")
        );
        assert!(note_store_error(id, "permission denied").is_none());
        clear_conversation(id);
    }
}
