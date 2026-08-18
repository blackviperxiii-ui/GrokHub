//! Bind an in-flight chat stream to the thread that started it.
//! New chat must stay empty and idle while the origin thread keeps the reply.

use crate::organs::last_user_text;
use crate::slash::is_cabin_slash_turn;

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

/// Halt / redirect must not leave a truncated assistant in the transcript.
pub fn drop_trailing_assistant(messages: &mut Vec<(String, String)>) {
    if messages.last().map(|(r, _)| r == "assistant").unwrap_or(false) {
        messages.pop();
    }
}

/// Update / host / voice jobs have no chat thread — errors stay on the status bar.
pub fn job_error_goes_to_chat(chat_job_thread: Option<&str>) -> bool {
    chat_job_thread.is_some()
}

/// Host / connector / consult receipts stay on the thread that started the job.
pub fn push_bound_message(
    job_thread_id: Option<&str>,
    visible_thread_id: &str,
    visible_messages: &mut Vec<(String, String)>,
    stored: &mut [(String, Vec<(String, String)>)],
    role: &str,
    content: String,
) {
    let target = job_thread_id.unwrap_or(visible_thread_id);
    if target == visible_thread_id {
        visible_messages.push((role.to_string(), content));
        return;
    }
    if let Some((_, msgs)) = stored.iter_mut().find(|(id, _)| id == target) {
        msgs.push((role.to_string(), content));
    }
}

/// Composer send without auth must not write a user turn.
pub fn persist_user_turn(has_key: bool) -> bool {
    has_key
}

/// Scratch policy follows the job thread, not whichever tab is visible.
pub fn job_is_scratch(
    job_thread_id: Option<&str>,
    visible_thread_id: &str,
    visible_scratch: bool,
    stored_scratch: &[(String, bool)],
) -> bool {
    let target = job_thread_id.unwrap_or(visible_thread_id);
    if target == visible_thread_id {
        return visible_scratch;
    }
    stored_scratch
        .iter()
        .find(|(id, _)| id == target)
        .map(|(_, scratch)| *scratch)
        .unwrap_or(visible_scratch)
}

/// Halt must drop the partial assistant on the job thread, not only the visible tab.
pub fn drop_trailing_assistant_on(
    job_thread_id: Option<&str>,
    visible_thread_id: &str,
    visible_messages: &mut Vec<(String, String)>,
    stored: &mut [(String, Vec<(String, String)>)],
) {
    let target = job_thread_id.unwrap_or(visible_thread_id);
    if target == visible_thread_id {
        drop_trailing_assistant(visible_messages);
        return;
    }
    if let Some((_, msgs)) = stored.iter_mut().find(|(id, _)| id == target) {
        drop_trailing_assistant(msgs);
    }
}

/// Follow-up after host/connector must read the origin thread, not the visible tab.
pub fn kick_messages_for_job(
    job_thread_id: Option<&str>,
    visible_thread_id: &str,
    visible_messages: &[(String, String)],
    stored: &[(String, Vec<(String, String)>)],
) -> Vec<(String, String)> {
    let raw = match job_thread_id {
        Some(job) if job != visible_thread_id => stored
            .iter()
            .find(|(id, _)| id == job)
            .map(|(_, msgs)| msgs.clone())
            .unwrap_or_else(|| visible_messages.to_vec()),
        _ => visible_messages.to_vec(),
    };
    raw.into_iter()
        .filter(|(role, content)| !is_cabin_slash_turn(role, content))
        .collect()
}

/// Skill draft after host must use the origin thread's last real user turn.
pub fn last_user_for_job(
    job_thread_id: Option<&str>,
    visible_thread_id: &str,
    visible_messages: &[(String, String)],
    stored: &[(String, Vec<(String, String)>)],
) -> Option<String> {
    last_user_text(&kick_messages_for_job(
        job_thread_id,
        visible_thread_id,
        visible_messages,
        stored,
    ))
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

    #[test]
    fn halt_drops_partial_assistant() {
        let mut msgs = origin_partial();
        drop_trailing_assistant(&mut msgs);
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].0, "user");
        assert!(!job_error_goes_to_chat(None));
        assert!(job_error_goes_to_chat(Some("thr-a")));
        assert!(!persist_user_turn(false));
        assert!(persist_user_turn(true));
    }

    #[test]
    fn scratch_follows_the_job_thread() {
        let stored = [("thr-a".into(), true), ("thr-b".into(), false)];
        assert!(
            job_is_scratch(Some("thr-a"), "thr-b", false, &stored),
            "a scratch host job must not write a skill after switching tabs"
        );
        assert!(
            !job_is_scratch(Some("thr-b"), "thr-a", true, &stored),
            "a real-thread host job must still save a skill from Scratch"
        );
        assert!(job_is_scratch(None, "thr-a", true, &stored));
        assert!(!job_is_scratch(Some("thr-b"), "thr-b", false, &stored));
    }

    #[test]
    fn host_receipt_stays_on_the_origin_thread() {
        let mut visible = vec![("user".into(), "other".into())];
        let mut stored = vec![
            ("thr-a".into(), vec![("user".into(), "run ls".into())]),
            ("thr-b".into(), visible.clone()),
        ];
        push_bound_message(
            Some("thr-a"),
            "thr-b",
            &mut visible,
            &mut stored,
            "user",
            "HOST_RESULT (facts only):\nok".into(),
        );
        assert_eq!(visible, vec![("user".into(), "other".into())]);
        assert_eq!(
            stored[0].1.last().map(|m| m.1.as_str()),
            Some("HOST_RESULT (facts only):\nok")
        );
        push_bound_message(
            Some("thr-b"),
            "thr-b",
            &mut visible,
            &mut stored,
            "user",
            "CONNECTOR_RESULT (facts only):\nok".into(),
        );
        assert_eq!(
            visible.last().map(|m| m.1.as_str()),
            Some("CONNECTOR_RESULT (facts only):\nok")
        );
    }

    #[test]
    fn halt_drops_partial_on_the_origin_thread() {
        let mut visible = vec![("user".into(), "other".into())];
        let mut stored = vec![
            (
                "thr-a".into(),
                vec![
                    ("user".into(), "what is rust".into()),
                    ("assistant".into(), "Rust is".into()),
                ],
            ),
            ("thr-b".into(), visible.clone()),
        ];
        drop_trailing_assistant_on(Some("thr-a"), "thr-b", &mut visible, &mut stored);
        assert_eq!(visible, vec![("user".into(), "other".into())]);
        assert_eq!(stored[0].1.len(), 1);
        assert_eq!(stored[0].1[0].0, "user");
        let origin = vec![
            ("user".into(), "run ls".into()),
            ("user".into(), "HOST_RESULT (facts only):\nok".into()),
        ];
        let stored = vec![("thr-a".into(), origin.clone()), ("thr-b".into(), visible.clone())];
        let msgs = kick_messages_for_job(Some("thr-a"), "thr-b", &visible, &stored);
        assert_eq!(msgs, origin);
        let user = last_user_for_job(Some("thr-a"), "thr-b", &visible, &stored);
        assert_eq!(user.as_deref(), Some("run ls"));
    }

    #[test]
    fn kick_drops_slash_help_dumps() {
        let visible = vec![
            ("user".into(), "hi".into()),
            ("assistant".into(), "SLASH_RESULT:\n/help — this list".into()),
            ("assistant".into(), "/help — this list\n/new — new chat".into()),
        ];
        let msgs = kick_messages_for_job(None, "thr-a", &visible, &[]);
        assert_eq!(msgs, vec![("user".into(), "hi".into())]);
    }
}
