//! Spike-3a session search over spans. Tool steps join the rows History and
//! the palette already show (no new page, panel or chip), and opening one
//! goes back to that chat and scrolls to that turn's Work card. Reads only.

use std::collections::HashMap;
use std::path::Path;

use grokhub_core::{ChatKind, ChatView};

/// History / palette rows for the tool steps that match `q`, as
/// `(step:<turn>:<chat>, chat · turn N · tool · decision)`. Reads span
/// files, so call it off the UI thread.
pub(super) fn step_hits(
    config_dir: &Path,
    q: &str,
    titles: &HashMap<String, String>,
    held: &[String],
) -> Vec<(String, String)> {
    grokhub_agent::harness::search_spans(config_dir, q, titles, held)
        .hits
        .into_iter()
        .map(|h| (h.target(), h.line))
        .collect()
}

/// `step:<turn>:<chat>` → `(turn, chat)`.
pub(super) fn parse_step_target(target: &str) -> Option<(u32, &str)> {
    let (turn, chat) = target.strip_prefix("step:")?.split_once(':')?;
    let turn = turn.parse().ok()?;
    (!chat.is_empty()).then_some((turn, chat))
}

/// The row to scroll to for user turn `turn` (1-based, as spans count it):
/// that turn's first Work card, else the user's own message.
pub(super) fn turn_jump_row(views: &[ChatView], turn: u32) -> Option<usize> {
    let ask = views
        .iter()
        .enumerate()
        .filter(|(_, v)| v.kind == ChatKind::User)
        .nth(usize::try_from(turn).ok()?.checked_sub(1)?)
        .map(|(i, _)| i)?;
    let work = views[ask + 1..]
        .iter()
        .take_while(|v| v.kind != ChatKind::User)
        .position(|v| v.kind == ChatKind::Tool)
        .map(|j| ask + 1 + j);
    Some(work.unwrap_or(ask))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view(kind: ChatKind) -> ChatView {
        ChatView { kind, title: String::new(), body: String::new() }
    }

    #[test]
    fn step_targets_parse_and_turns_land_on_their_work_card() {
        assert_eq!(parse_step_target("step:3:chat-a"), Some((3, "chat-a")));
        assert_eq!(parse_step_target("step:3:chat:with:colons"), Some((3, "chat:with:colons")));
        assert_eq!(parse_step_target("step:x:chat-a"), None);
        assert_eq!(parse_step_target("step:3:"), None);
        assert_eq!(parse_step_target("thread:chat-a"), None);

        use ChatKind::*;
        let views: Vec<ChatView> = [User, Assistant, User, Thought, Tool, Assistant, User, Assistant]
            .into_iter()
            .map(view)
            .collect();
        assert_eq!(turn_jump_row(&views, 1), Some(0), "no Work card: the ask itself");
        assert_eq!(turn_jump_row(&views, 2), Some(4));
        assert_eq!(turn_jump_row(&views, 3), Some(6));
        assert_eq!(turn_jump_row(&views, 4), None);
        assert_eq!(turn_jump_row(&views, 0), None);
    }
}
