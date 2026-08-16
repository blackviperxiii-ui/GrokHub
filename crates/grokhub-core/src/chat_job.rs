//! Bind an in-flight chat stream to the thread that started it.
//! New chat must stay empty and idle while the origin thread keeps the reply.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatSendKind {
    Fresh,
    Redirect,
}

pub fn upsert_assistant_turn(messages: &mut Vec<(String, String)>, content: &str) {
    if content.is_empty() {
        return;
    }
    if let Some((role, body)) = messages.last_mut() {
        if role == "assistant" {
            *body = content.to_string();
            return;
        }
    }
    messages.push(("assistant".into(), content.to_string()));
}

/// Whether the visible thread owns the in-flight chat stream.
pub fn chat_stream_is_visible(job_thread_id: Option<&str>, visible_thread_id: &str) -> bool {
    match job_thread_id {
        Some(id) => id == visible_thread_id,
        None => false,
    }
}

/// Thinking / live-thought chrome for this thread only.
/// A host/imagine job (`job_thread_id` none) still busy the whole cabin.
pub fn chat_shows_thinking(
    job_thread_id: Option<&str>,
    visible_thread_id: &str,
    running: bool,
) -> bool {
    if !running {
        return false;
    }
    match job_thread_id {
        Some(id) => id == visible_thread_id,
        None => true,
    }
}

/// Same-thread interrupt uses redirect. A different thread starts a fresh send.
pub fn chat_send_kind(
    job_thread_id: Option<&str>,
    visible_thread_id: &str,
    running: bool,
) -> ChatSendKind {
    if !running {
        return ChatSendKind::Fresh;
    }
    match job_thread_id {
        Some(id) if id == visible_thread_id => ChatSendKind::Redirect,
        Some(_) => ChatSendKind::Fresh,
        None => ChatSendKind::Redirect,
    }
}

/// Write the live assistant snapshot onto the job's thread, not whichever is visible.
pub fn apply_stream_snapshot(
    job_thread_id: Option<&str>,
    visible_thread_id: &str,
    visible_messages: &mut Vec<(String, String)>,
    stored: &mut [(String, Vec<(String, String)>)],
    content: &str,
) {
    let Some(job_id) = job_thread_id else {
        upsert_assistant_turn(visible_messages, content);
        return;
    };
    if job_id == visible_thread_id {
        upsert_assistant_turn(visible_messages, content);
        return;
    }
    if let Some((_, msgs)) = stored.iter_mut().find(|(id, _)| id == job_id) {
        upsert_assistant_turn(msgs, content);
    }
}

/// Replace a partial live assistant with the error, or append one.
pub fn apply_job_error(messages: &mut Vec<(String, String)>, err: &str) -> String {
    let text = format!("Error: {err}");
    if let Some((role, body)) = messages.last_mut() {
        if role == "assistant" {
            *body = text;
            return err.to_string();
        }
    }
    messages.push(("assistant".into(), text));
    err.to_string()
}

pub fn worker_gone_status() -> &'static str {
    "Job dropped — worker gone"
}

#[cfg(test)]
mod tests {
    use super::*;

    fn origin_partial() -> Vec<(String, String)> {
        vec![
            ("user".into(), "what is rust".into()),
            ("assistant".into(), "Rust is".into()),
        ]
    }

    #[test]
    fn new_chat_does_not_take_the_live_stream() {
        let mut visible = Vec::new();
        let mut stored = vec![
            ("thr-a".into(), origin_partial()),
            ("thr-b".into(), Vec::new()),
        ];
        apply_stream_snapshot(
            Some("thr-a"),
            "thr-b",
            &mut visible,
            &mut stored,
            "Rust is a language",
        );
        assert!(
            visible.is_empty(),
            "the new chat must stay empty while the origin is still answering"
        );
        assert_eq!(
            stored[0].1.last().map(|m| m.1.as_str()),
            Some("Rust is a language")
        );
        assert!(stored[1].1.is_empty());
        assert!(!chat_shows_thinking(Some("thr-a"), "thr-b", true));
        assert!(chat_shows_thinking(Some("thr-a"), "thr-a", true));
        assert!(!chat_stream_is_visible(Some("thr-a"), "thr-b"));
        assert!(chat_stream_is_visible(Some("thr-a"), "thr-a"));
    }

    #[test]
    fn visible_origin_keeps_taking_deltas() {
        let mut visible = origin_partial();
        let mut stored = vec![("thr-a".into(), origin_partial())];
        apply_stream_snapshot(
            Some("thr-a"),
            "thr-a",
            &mut visible,
            &mut stored,
            "Rust is a language",
        );
        assert_eq!(visible.last().map(|m| m.1.as_str()), Some("Rust is a language"));
    }

    #[test]
    fn same_thread_send_redirects_the_live_reply() {
        assert_eq!(
            chat_send_kind(Some("thr-a"), "thr-a", true),
            ChatSendKind::Redirect
        );
    }

    #[test]
    fn new_chat_send_is_fresh_not_a_redirect() {
        assert_eq!(
            chat_send_kind(Some("thr-a"), "thr-b", true),
            ChatSendKind::Fresh
        );
    }

    #[test]
    fn host_job_with_no_thread_still_busy_everywhere() {
        assert!(chat_shows_thinking(None, "thr-b", true));
        assert_eq!(chat_send_kind(None, "thr-b", true), ChatSendKind::Redirect);
        assert!(!chat_shows_thinking(None, "thr-b", false));
        assert_eq!(chat_send_kind(None, "thr-b", false), ChatSendKind::Fresh);
    }

    #[test]
    fn job_error_replaces_partial_assistant() {
        let mut msgs = origin_partial();
        let status = apply_job_error(&mut msgs, "429 rate limited");
        assert_eq!(status, "429 rate limited");
        assert_eq!(
            msgs.last().map(|m| m.1.as_str()),
            Some("Error: 429 rate limited")
        );
        assert_eq!(msgs.iter().filter(|(r, _)| r == "assistant").count(), 1);
    }

    #[test]
    fn job_error_appends_when_no_assistant_yet() {
        let mut msgs = vec![("user".into(), "hi".into())];
        apply_job_error(&mut msgs, "401 unauthorized");
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[1], ("assistant".into(), "Error: 401 unauthorized".into()));
    }

    #[test]
    fn worker_gone_has_a_status() {
        assert!(!worker_gone_status().is_empty());
    }
}
