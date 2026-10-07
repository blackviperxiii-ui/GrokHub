//! Background runs beside the chat, and steering a live reply.
//!
//! A background run is its own headless `grok -p`. It forks the chat's Grok
//! session, so it knows the conversation, and never writes into that session.
//! When it ends, its reply is posted on the chat that started it, and the next
//! turn on that chat is told what came back.
//!
//! Steering stops the live turn, keeps what it already said and did, and starts
//! the next turn with the new message plus a short note of that progress.

use crate::chat_view::assistant_prose;

/// Live background runs at once. More would fight over the same tree and CPU.
pub const BG_TASK_MAX: usize = 3;

/// Reply line that asks the cabin to start a background run.
pub const BG_TASK_MARK: &str = "BACKGROUND_TASK:";

const BG_TASK_PROMPT_CAP: usize = 1200;
const BG_TITLE_CAP: usize = 60;
const BG_FOLLOW_CAP: usize = 900;
const STEER_ASK_CAP: usize = 600;
const STEER_PARTIAL_CAP: usize = 900;
const STEER_TOOLS_MAX: usize = 12;

/// Where a background run came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BgOrigin {
    /// `/bg <task>` from the composer.
    User,
    /// A `BACKGROUND_TASK:` line in Grok's reply.
    Agent,
    /// A live reply moved off the composer.
    Detached,
    /// A scheduled automation, run on the hidden Background chat.
    Scheduled,
}

/// How a background run ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BgEnd {
    Done,
    Failed(String),
    Stopped,
}

/// A message typed while this chat's reply is still running.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiveSend {
    /// Stop the turn where it is and carry on with the new message folded in.
    Steer,
    /// Hold the message until the turn ends.
    Queue,
}

/// Steer by default. Queue when they asked to (Alt+Enter, `/queue`), or when
/// Grok has its own background tasks on the turn: stopping it would kill those.
pub fn live_send(queue_asked: bool, grok_tasks_open: bool) -> LiveSend {
    if queue_asked || grok_tasks_open {
        LiveSend::Queue
    } else {
        LiveSend::Steer
    }
}

/// A live reply can move to the background only when it is a plain headless
/// `grok -p` chat turn. ACP (Ask) needs you at the permission card, scheduled
/// and `/send` runs settle their own bookkeeping when they end, and a capture,
/// verify, or host step still has work queued behind the turn.
pub fn can_detach_turn(headless: bool, acp: bool, scheduled: bool, side_work: bool) -> bool {
    headless && !acp && !scheduled && !side_work
}

/// `BACKGROUND_TASK:` lines in a finished reply, in order, at most `room`.
/// Each is one self-contained task; repeats and blanks are dropped.
pub fn extract_background_tasks(text: &str, room: usize) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in text.lines() {
        if out.len() >= room {
            break;
        }
        let Some(rest) = line.trim().strip_prefix(BG_TASK_MARK) else {
            continue;
        };
        let task: String = rest.trim().chars().take(BG_TASK_PROMPT_CAP).collect();
        if task.is_empty() || out.iter().any(|t| t.eq_ignore_ascii_case(&task)) {
            continue;
        }
        out.push(task);
    }
    out
}

/// Prompt for a background run. Nobody answers questions until it ends, and a
/// `BACKGROUND_TASK:` line from inside one is ignored, so it does the work itself.
pub fn bg_task_prompt(task: &str) -> String {
    format!(
        "You are running as a background task beside their chat. Work on your own: nobody can answer questions until you finish, and BACKGROUND_TASK lines are ignored here, so do the work yourself. End with a short summary of what you did and found.\n\nTask: {}",
        task.trim()
    )
}

/// Short label for the strip and the result post: the first line, clipped.
pub fn bg_task_title(prompt: &str) -> String {
    let line = prompt
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("");
    clip_chars(line, BG_TITLE_CAP)
}

/// The assistant message posted on the origin chat when a run ends.
pub fn bg_result_post(title: &str, end: &BgEnd, reply: &str) -> String {
    let prose = assistant_prose(reply);
    let head = match end {
        BgEnd::Done => "Background task done",
        BgEnd::Failed(_) => "Background task failed",
        BgEnd::Stopped => "Background task stopped",
    };
    let mut out = format!("**{head}** · {}", title.trim());
    let body = match end {
        BgEnd::Failed(err) if prose.trim().is_empty() => err.trim().to_string(),
        BgEnd::Done if prose.trim().is_empty() => "It finished without a reply.".to_string(),
        _ => prose.trim().to_string(),
    };
    if !body.is_empty() {
        out.push_str("\n\n");
        out.push_str(&body);
    }
    out
}

/// One line for the next turn on that chat: what the run was and what it said.
pub fn bg_result_note(title: &str, end: &BgEnd, reply: &str) -> String {
    let how = match end {
        BgEnd::Done => "done",
        BgEnd::Failed(_) => "failed",
        BgEnd::Stopped => "stopped",
    };
    let said = match end {
        BgEnd::Failed(err) if assistant_prose(reply).trim().is_empty() => err.trim().to_string(),
        _ => assistant_prose(reply),
    };
    let said = clip_chars(&said.split_whitespace().collect::<Vec<_>>().join(" "), BG_FOLLOW_CAP);
    if said.is_empty() {
        format!("- {} ({how})", title.trim())
    } else {
        format!("- {} ({how}): {said}", title.trim())
    }
}

/// Block put ahead of the next prompt on a chat whose background runs ended.
pub fn bg_results_follow(notes: &[String]) -> Option<String> {
    let notes: Vec<&str> = notes
        .iter()
        .map(|n| n.trim())
        .filter(|n| !n.is_empty())
        .collect();
    if notes.is_empty() {
        return None;
    }
    Some(format!(
        "Background tasks that finished since your last turn. Their replies are already in this chat. Use them; do not redo that work.\n{}",
        notes.join("\n")
    ))
}

/// `12s`, `4m`, `1h 5m`.
pub fn bg_elapsed_label(ms: u64) -> String {
    let s = ms / 1000;
    if s < 60 {
        format!("{s}s")
    } else if s < 3600 {
        format!("{}m", s / 60)
    } else {
        format!("{}h {}m", s / 3600, (s % 3600) / 60)
    }
}

/// Context put ahead of a steering message. The turn it replaces was stopped
/// mid-way; Grok resumes the same session, and this note covers what a cut-off
/// turn may not have saved.
pub fn steer_follow_block(prev_ask: &str, partial: &str, tools: &[String]) -> String {
    let mut out = String::from(
        "STEER: They sent the message below while you were still working, so your last turn was stopped where it was. Fold it in and carry on from there. Do not redo steps that already finished.",
    );
    let ask = clip_chars(prev_ask.trim(), STEER_ASK_CAP);
    if !ask.is_empty() {
        out.push_str("\nTheir previous ask: ");
        out.push_str(&ask);
    }
    let mut done: Vec<&str> = Vec::new();
    for t in tools.iter().map(|t| t.trim()).filter(|t| !t.is_empty()) {
        if done.last() != Some(&t) {
            done.push(t);
        }
    }
    if done.len() > STEER_TOOLS_MAX {
        done.drain(..done.len() - STEER_TOOLS_MAX);
    }
    if !done.is_empty() {
        out.push_str("\nAlready ran this turn: ");
        out.push_str(&done.join("; "));
    }
    let said = assistant_prose(partial);
    let said = tail_chars(said.trim(), STEER_PARTIAL_CAP);
    if !said.is_empty() {
        out.push_str("\nYou had said so far: ");
        out.push_str(&said);
    }
    out
}

fn clip_chars(s: &str, cap: usize) -> String {
    if s.chars().count() <= cap {
        return s.to_string();
    }
    let mut out: String = s.chars().take(cap.saturating_sub(1)).collect();
    out.push('…');
    out
}

fn tail_chars(s: &str, cap: usize) -> String {
    let n = s.chars().count();
    if n <= cap {
        return s.to_string();
    }
    let mut out = String::from("…");
    out.extend(s.chars().skip(n - cap.saturating_sub(1)));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typing_during_a_run_steers_unless_asked_to_queue() {
        assert_eq!(live_send(false, false), LiveSend::Steer);
        assert_eq!(live_send(true, false), LiveSend::Queue);
        assert_eq!(
            live_send(false, true),
            LiveSend::Queue,
            "stopping the turn would kill Grok's own background tasks"
        );
    }

    #[test]
    fn only_a_plain_headless_turn_moves_to_the_background() {
        assert!(can_detach_turn(true, false, false, false));
        assert!(!can_detach_turn(false, false, false, false), "no grok -p child to keep");
        assert!(!can_detach_turn(true, true, false, false), "Ask needs you at the card");
        assert!(!can_detach_turn(true, false, true, false), "scheduled runs settle themselves");
        assert!(!can_detach_turn(true, false, false, true), "a capture or host step is queued");
    }

    #[test]
    fn background_markers_are_read_in_order_and_capped() {
        let text = "On it.\nBACKGROUND_TASK: run the test suite and report failures\n  BACKGROUND_TASK:   \nBACKGROUND_TASK: Run the test suite and report failures\nBACKGROUND_TASK: rebuild the docs\nBACKGROUND_TASK: third\nBACKGROUND_TASK: fourth";
        let tasks = extract_background_tasks(text, BG_TASK_MAX);
        assert_eq!(
            tasks,
            vec![
                "run the test suite and report failures".to_string(),
                "rebuild the docs".to_string(),
                "third".to_string(),
            ]
        );
        assert!(extract_background_tasks(text, 0).is_empty());
        assert!(extract_background_tasks("no markers here", 3).is_empty());
        let long = format!("BACKGROUND_TASK: {}", "x".repeat(5000));
        assert_eq!(extract_background_tasks(&long, 1)[0].chars().count(), BG_TASK_PROMPT_CAP);
    }

    #[test]
    fn marker_lines_stay_off_the_bubble() {
        let prose = assistant_prose("Started it.\nBACKGROUND_TASK: rebuild the docs");
        assert_eq!(prose, "Started it.");
    }

    #[test]
    fn a_background_prompt_works_alone() {
        let p = bg_task_prompt("  run the tests ");
        assert!(p.ends_with("Task: run the tests"), "{p}");
        assert!(p.contains("nobody can answer questions"), "{p}");
        assert!(p.contains("BACKGROUND_TASK lines are ignored"), "{p}");
    }

    #[test]
    fn titles_are_the_first_line_clipped() {
        assert_eq!(bg_task_title("\n  fix the build  \nthen test"), "fix the build");
        let t = bg_task_title(&"y".repeat(200));
        assert_eq!(t.chars().count(), BG_TITLE_CAP);
        assert!(t.ends_with('…'));
        assert_eq!(bg_task_title(""), "");
    }

    #[test]
    fn result_posts_say_how_the_run_ended() {
        let done = bg_result_post("docs", &BgEnd::Done, "Rebuilt.\nWORK_PIN: docs");
        assert_eq!(done, "**Background task done** · docs\n\nRebuilt.");
        let empty = bg_result_post("docs", &BgEnd::Done, "");
        assert!(empty.contains("finished without a reply"), "{empty}");
        let failed = bg_result_post("docs", &BgEnd::Failed("exit 1".into()), "");
        assert_eq!(failed, "**Background task failed** · docs\n\nexit 1");
        let stopped = bg_result_post("docs", &BgEnd::Stopped, "");
        assert_eq!(stopped, "**Background task stopped** · docs");
    }

    #[test]
    fn the_next_turn_hears_what_came_back() {
        assert!(bg_results_follow(&[]).is_none());
        assert!(bg_results_follow(&["  ".into()]).is_none());
        let note = bg_result_note("tests", &BgEnd::Done, "All   42\npassed.");
        assert_eq!(note, "- tests (done): All 42 passed.");
        let failed = bg_result_note("tests", &BgEnd::Failed("exit 2".into()), "");
        assert_eq!(failed, "- tests (failed): exit 2");
        assert_eq!(bg_result_note("tests", &BgEnd::Stopped, ""), "- tests (stopped)");
        let block = bg_results_follow(&[note]).unwrap();
        assert!(block.starts_with("Background tasks that finished"), "{block}");
        assert!(block.ends_with("- tests (done): All 42 passed."), "{block}");
        let long = bg_result_note("t", &BgEnd::Done, &"z ".repeat(2000));
        assert!(long.chars().count() < BG_FOLLOW_CAP + 40);
    }

    #[test]
    fn elapsed_reads_short() {
        assert_eq!(bg_elapsed_label(0), "0s");
        assert_eq!(bg_elapsed_label(59_999), "59s");
        assert_eq!(bg_elapsed_label(61_000), "1m");
        assert_eq!(bg_elapsed_label(3_900_000), "1h 5m");
    }

    #[test]
    fn steer_note_carries_the_progress_of_the_stopped_turn() {
        let tools = vec![
            "Read Cargo.toml".to_string(),
            "Read Cargo.toml".to_string(),
            " ".to_string(),
            "cargo test".to_string(),
        ];
        let block = steer_follow_block("fix the failing test", "Looking at it.\nUSER_FACT: x", &tools);
        assert!(block.starts_with("STEER:"), "{block}");
        assert!(block.contains("Their previous ask: fix the failing test"), "{block}");
        assert!(block.contains("Already ran this turn: Read Cargo.toml; cargo test"), "{block}");
        assert!(block.contains("You had said so far: Looking at it."), "{block}");
        assert!(!block.contains("USER_FACT"), "{block}");
        let bare = steer_follow_block("", "", &[]);
        assert!(!bare.contains("previous ask") && !bare.contains("Already ran") && !bare.contains("said so far"));
        let many: Vec<String> = (0..30).map(|i| format!("tool {i}")).collect();
        let capped = steer_follow_block("a", "", &many);
        assert!(capped.contains("tool 29") && !capped.contains("tool 17;"), "{capped}");
        let long = steer_follow_block("a", &format!("start {}", "w".repeat(3000)), &[]);
        assert!(!long.contains("start"), "the newest words win: {long}");
    }
}
