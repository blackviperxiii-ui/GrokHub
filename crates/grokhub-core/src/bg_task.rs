//! Background runs beside the chat: `/bg <task>` and `/send <task>` in the
//! desktop app (`src-tauri/src/bg_runs.rs`). A run is its own read-only
//! session; a bare `/bg` lists what it found.

use crate::chat_view::assistant_prose;
use crate::turn_timeline::clip_chars;

/// Live background runs at once. More would fight over the same tree and CPU.
pub const BG_TASK_MAX: usize = 3;

const BG_TITLE_CAP: usize = 60;

/// How a background run ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BgEnd {
    Done,
    Failed(String),
    Stopped,
}

/// Prompt for a background run. Nobody answers questions until it ends, so it
/// does the work itself.
pub fn bg_task_prompt(task: &str) -> String {
    format!(
        "You are running as a background task beside their chat. Work on your own: nobody can answer questions until you finish, so do the work yourself. End with a short summary of what you did and found.\n\nTask: {}",
        task.trim()
    )
}

/// Short label for the run list: the first line, clipped.
pub fn bg_task_title(prompt: &str) -> String {
    let line = prompt
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("");
    clip_chars(line, BG_TITLE_CAP)
}

/// What a run keeps as its result: the reply's prose, or why it failed.
pub fn bg_result_body(end: &BgEnd, reply: &str) -> String {
    let prose = assistant_prose(reply);
    match end {
        BgEnd::Failed(err) if prose.trim().is_empty() => err.trim().to_string(),
        BgEnd::Done if prose.trim().is_empty() => "It finished without a reply.".to_string(),
        _ => prose.trim().to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_background_prompt_works_alone() {
        assert_eq!(
            bg_task_prompt("  run the tests "),
            "You are running as a background task beside their chat. Work on your own: nobody can answer questions until you finish, so do the work yourself. End with a short summary of what you did and found.\n\nTask: run the tests"
        );
    }

    #[test]
    fn titles_are_the_first_line_clipped() {
        assert_eq!(
            bg_task_title("\n  fix the build  \nthen test"),
            "fix the build"
        );
        let t = bg_task_title(&"y".repeat(200));
        assert_eq!(t.chars().count(), BG_TITLE_CAP);
        assert!(t.ends_with('…'));
        assert_eq!(bg_task_title(""), "");
    }

    #[test]
    fn a_result_is_the_reply_or_why_it_ended() {
        assert_eq!(
            bg_result_body(&BgEnd::Done, "Rebuilt.\nWORK_PIN: docs"),
            "Rebuilt."
        );
        assert_eq!(
            bg_result_body(&BgEnd::Done, ""),
            "It finished without a reply."
        );
        assert_eq!(
            bg_result_body(&BgEnd::Failed(" exit 1 ".into()), ""),
            "exit 1"
        );
        assert_eq!(
            bg_result_body(&BgEnd::Failed("exit 1".into()), "Half done."),
            "Half done."
        );
        assert_eq!(bg_result_body(&BgEnd::Stopped, ""), "");
    }
}
