//! Claude Code usage for the Fleet page, from its local transcripts
//! (`<claude dir>/projects/**/*.jsonl`). Each assistant line carries the
//! reply's token counts and time, so the page can show the current 5-hour
//! window and the last 7 days. Read-only: only `.jsonl` files under
//! `projects/` are opened, never Claude Code's settings or sign-in.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub const WINDOW_MS: u64 = 5 * 60 * 60 * 1000;
pub const WEEK_MS: u64 = 7 * 24 * 60 * 60 * 1000;
const HOUR_MS: u64 = 60 * 60 * 1000;
/// How deep under `projects/` transcripts are looked for: a workflow's
/// agent transcripts sit at `<project>/<session>/subagents/workflows/<run>/`.
const MAX_DEPTH: usize = 6;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Tokens {
    pub input: u64,
    pub output: u64,
    pub cache_write: u64,
    pub cache_read: u64,
}

impl Tokens {
    fn add(&mut self, other: Tokens) {
        self.input += other.input;
        self.output += other.output;
        self.cache_write += other.cache_write;
        self.cache_read += other.cache_read;
    }
}

/// One model reply in a transcript.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Turn {
    pub at_ms: u64,
    /// The reply's message and request ids: a reply is written once per
    /// content block, each line with the same usage.
    pub key: String,
    pub tokens: Tokens,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeUsage {
    /// Tokens in the 5-hour window that is open now; zero when none is.
    pub window: Tokens,
    /// When the open window ends; `None` when no window is open, or when its
    /// start can't be known (see `window_rolling`).
    pub window_resets_at_ms: Option<u64>,
    /// No 5-hour pause is in the history read, so the window's start can't be
    /// placed: `window` is then the last 5 hours instead.
    pub window_rolling: bool,
    pub week: Tokens,
    pub last_at_ms: Option<u64>,
}

/// The fields of a transcript line that are read; serde skips the rest
/// (the reply's text and tool output) without building it.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Line {
    #[serde(rename = "type")]
    kind: String,
    timestamp: Option<String>,
    request_id: Option<String>,
    message: Option<Message>,
}

#[derive(Deserialize)]
struct Message {
    id: Option<String>,
    usage: Option<Usage>,
}

#[derive(Deserialize)]
struct Usage {
    input_tokens: Option<u64>,
    output_tokens: Option<u64>,
    cache_creation_input_tokens: Option<u64>,
    cache_read_input_tokens: Option<u64>,
}

/// The reply on one transcript line, or `None` for any other line.
pub fn parse_line(line: &str) -> Option<Turn> {
    if !line.contains("\"usage\"") {
        return None;
    }
    let line: Line = serde_json::from_str(line).ok()?;
    if line.kind != "assistant" {
        return None;
    }
    let at_ms = crate::pulse_sources::parse_date_ms(line.timestamp.as_deref()?)?;
    let message = line.message?;
    let usage = message.usage?;
    Some(Turn {
        at_ms,
        key: format!(
            "{}:{}",
            message.id.unwrap_or_default(),
            line.request_id.unwrap_or_default()
        ),
        tokens: Tokens {
            input: usage.input_tokens.unwrap_or(0),
            output: usage.output_tokens.unwrap_or(0),
            cache_write: usage.cache_creation_input_tokens.unwrap_or(0),
            cache_read: usage.cache_read_input_tokens.unwrap_or(0),
        },
    })
}

/// Replies before this may be missing from what [`read_turns`] returns at
/// `now`: older transcripts aren't opened.
pub fn read_from(now: u64) -> u64 {
    now.saturating_sub(WEEK_MS + WINDOW_MS)
}

/// The 5-hour window and the 7 days up to `now`. A window opens on the hour
/// of the first reply after the last one ended, as Claude's plan limits
/// count them. Replies before `missing_before` may be absent, so a window
/// is only placed from a reply known to open one: the first after a pause
/// of 5 hours or more. With no such pause, `window` is the last 5 hours and
/// `window_rolling` says so.
pub fn usage(turns: impl IntoIterator<Item = Turn>, now: u64, missing_before: u64) -> ClaudeUsage {
    let mut seen = HashSet::new();
    let mut turns: Vec<Turn> = turns
        .into_iter()
        .filter(|t| t.at_ms <= now)
        .filter(|t| t.key == ":" || seen.insert(t.key.clone()))
        .collect();
    turns.sort_by_key(|t| t.at_ms);
    let mut out = ClaudeUsage {
        last_at_ms: turns.last().map(|t| t.at_ms),
        ..Default::default()
    };
    let week_from = now.saturating_sub(WEEK_MS);
    for turn in turns.iter().filter(|t| t.at_ms >= week_from) {
        out.week.add(turn.tokens);
    }
    let opens = |i: usize| {
        let before = if i == 0 {
            missing_before
        } else {
            turns[i - 1].at_ms
        };
        turns[i].at_ms.saturating_sub(before) >= WINDOW_MS
    };
    let Some(anchor) = (0..turns.len()).rev().find(|&i| opens(i)) else {
        out.window_rolling = !turns.is_empty();
        let rolling_from = now.saturating_sub(WINDOW_MS);
        for turn in turns.iter().filter(|t| t.at_ms > rolling_from) {
            out.window.add(turn.tokens);
        }
        return out;
    };
    let mut start = 0;
    for (i, turn) in turns.iter().enumerate().skip(anchor) {
        if i == anchor || turn.at_ms >= start + WINDOW_MS {
            start = turn.at_ms - turn.at_ms % HOUR_MS;
            out.window = Tokens::default();
        }
        out.window.add(turn.tokens);
    }
    let end = start + WINDOW_MS;
    if now < end {
        out.window_resets_at_ms = Some(end);
    } else {
        out.window = Tokens::default();
    }
    out
}

/// Claude Code's folder: `CLAUDE_CONFIG_DIR`, or `.claude` in `home`.
pub fn claude_dir(env_dir: Option<&str>, home: &Path) -> PathBuf {
    env_dir
        .map(str::trim)
        .filter(|d| !d.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".claude"))
}

/// Every reply in the transcripts under `claude_dir/projects` written to in
/// the last 7 days and 5 hours. Older files aren't opened.
/// Each file is read a line at a time, and a line that isn't whole UTF-8 (one
/// Claude Code is still writing) is skipped, not the file.
pub fn read_turns(claude_dir: &Path, now: u64) -> Vec<Turn> {
    use std::io::BufRead;
    let mut files = Vec::new();
    collect(&claude_dir.join("projects"), 0, read_from(now), &mut files);
    let mut turns = Vec::new();
    for path in files {
        let Ok(file) = std::fs::File::open(&path) else {
            continue;
        };
        for line in std::io::BufReader::new(file).split(b'\n') {
            let Ok(line) = line else {
                break;
            };
            if let Some(turn) = std::str::from_utf8(&line).ok().and_then(parse_line) {
                turns.push(turn);
            }
        }
    }
    turns
}

fn collect(dir: &Path, depth: usize, since: u64, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        let path = entry.path();
        if kind.is_dir() && depth < MAX_DEPTH {
            collect(&path, depth + 1, since, out);
        } else if kind.is_file() && path.extension().is_some_and(|e| e == "jsonl") {
            let modified = entry
                .metadata()
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_millis() as u64);
            if modified.is_some_and(|m| m >= since) {
                out.push(path);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 2026-11-16T12:00:00Z.
    const NOON: u64 = 1_794_830_400_000;

    fn line(ts: &str, id: &str, input: u64, output: u64) -> String {
        format!(
            r#"{{"type":"assistant","timestamp":"{ts}","requestId":"req_{id}","message":{{"id":"msg_{id}","usage":{{"input_tokens":{input},"output_tokens":{output},"cache_creation_input_tokens":10,"cache_read_input_tokens":100}}}}}}"#
        )
    }

    fn turn(at_ms: u64, id: &str, output: u64) -> Turn {
        Turn {
            at_ms,
            key: format!("msg_{id}:req_{id}"),
            tokens: Tokens {
                input: 1,
                output,
                cache_write: 0,
                cache_read: 0,
            },
        }
    }

    #[test]
    fn an_assistant_line_gives_its_time_ids_and_tokens_and_other_lines_none() {
        assert_eq!(
            parse_line(&line("2026-11-16T12:00:00.500Z", "a", 3, 40)),
            Some(Turn {
                at_ms: NOON,
                key: "msg_a:req_a".into(),
                tokens: Tokens {
                    input: 3,
                    output: 40,
                    cache_write: 10,
                    cache_read: 100
                },
            })
        );
        let user = r#"{"type":"user","timestamp":"2026-11-16T12:00:00Z","message":{"usage":{}}}"#;
        assert_eq!(parse_line(user), None);
        assert_eq!(parse_line(r#"{"type":"summary","summary":"x"}"#), None);
        assert_eq!(parse_line("not json \"usage\""), None);
    }

    #[test]
    fn the_open_window_starts_on_the_hour_and_a_repeated_reply_counts_once() {
        let at = |h: u64, m: u64| NOON + h * HOUR_MS + m * 60_000;
        let turns = vec![
            turn(at(0, 20), "a", 100),
            turn(at(0, 20), "a", 100),
            turn(at(2, 0), "b", 50),
        ];
        let got = usage(turns, at(3, 0), 0);
        assert_eq!(got.window.output, 150);
        assert_eq!(got.window_resets_at_ms, Some(NOON + 5 * HOUR_MS));
        assert_eq!(got.week.output, 150);
        assert_eq!(got.last_at_ms, Some(at(2, 0)));
    }

    #[test]
    fn a_reply_after_the_window_ends_opens_a_new_one_and_a_closed_one_reads_zero() {
        let at = |h: u64, m: u64| NOON + h * HOUR_MS + m * 60_000;
        let turns = vec![turn(at(0, 10), "a", 100), turn(at(5, 30), "b", 7)];
        let open = usage(turns.clone(), at(6, 0), 0);
        assert_eq!(
            (open.window.output, open.window_resets_at_ms),
            (7, Some(at(10, 0)))
        );
        assert_eq!(open.week.output, 107);
        let closed = usage(turns, at(10, 0), 0);
        assert_eq!(
            (closed.window, closed.window_resets_at_ms),
            (Tokens::default(), None)
        );
        assert_eq!(closed.week.output, 107);
    }

    #[test]
    fn the_week_drops_replies_older_than_seven_days_and_ones_from_the_future() {
        let turns = vec![
            turn(NOON - WEEK_MS - 1, "old", 1_000),
            turn(NOON - WEEK_MS + 1, "edge", 5),
            turn(NOON + 1, "ahead", 9_999),
        ];
        let got = usage(turns, NOON, 0);
        assert_eq!(got.week.output, 5);
        assert_eq!(got.last_at_ms, Some(NOON - WEEK_MS + 1));
    }

    /// One reply every 2 hours from `NOON` for `hours`.
    fn steady(hours: u64) -> Vec<Turn> {
        (0..=hours / 2)
            .map(|n| turn(NOON + n * 2 * HOUR_MS, &n.to_string(), 1))
            .collect()
    }

    #[test]
    fn steady_use_places_the_window_from_the_first_reply_and_never_guesses_a_phase() {
        let at = |h: u64| NOON + h * HOUR_MS;
        // Windows open at 0h, 6h, 12h...: each reply 6h after a window's
        // start opens the next. At 194h the open one began at 192h.
        let exact = usage(steady(194), at(194), 0);
        assert_eq!(
            (
                exact.window.output,
                exact.window_resets_at_ms,
                exact.window_rolling
            ),
            (2, Some(at(197)), false)
        );
        // The same replies with the first days unread: no 5-hour pause is
        // left to place a window from, so the last 5 hours stand in.
        let partial = usage(
            steady(194).into_iter().filter(|t| t.at_ms >= at(20)),
            at(194),
            at(20),
        );
        assert_eq!(
            (
                partial.window.output,
                partial.window_resets_at_ms,
                partial.window_rolling
            ),
            (3, None, true)
        );
        assert_eq!(partial.week.output, exact.week.output);
    }

    #[test]
    fn a_five_hour_pause_places_the_window_even_with_older_days_unread() {
        let at = |h: u64| NOON + h * HOUR_MS;
        let mut turns: Vec<Turn> = steady(100)
            .into_iter()
            .filter(|t| t.at_ms >= at(20))
            .collect();
        turns.push(turn(at(107) + 30 * 60_000, "after-pause", 10));
        turns.push(turn(at(110), "same-window", 5));
        let got = usage(turns, at(111), at(20));
        assert_eq!(
            (
                got.window.output,
                got.window_resets_at_ms,
                got.window_rolling
            ),
            (15, Some(at(112)), false)
        );
    }

    #[test]
    fn transcripts_are_read_from_projects_only_and_old_files_are_skipped() {
        let dir = std::env::temp_dir().join(format!("claude-usage-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let deep = dir.join("projects/my-app/session-1/subagents/workflows/run-1");
        std::fs::create_dir_all(&deep).unwrap();
        std::fs::write(
            dir.join("projects/my-app/session-1.jsonl"),
            format!(
                "{}\n{{\"type\":\"user\"}}\n",
                line("2026-11-16T11:00:00Z", "a", 1, 20)
            ),
        )
        .unwrap();
        std::fs::write(
            deep.join("agent-1.jsonl"),
            line("2026-11-16T11:30:00Z", "b", 1, 3),
        )
        .unwrap();
        std::fs::write(
            dir.join("history.jsonl"),
            line("2026-11-16T11:40:00Z", "c", 1, 999),
        )
        .unwrap();
        std::fs::write(
            dir.join("projects/my-app/notes.txt"),
            line("2026-11-16T11:40:00Z", "d", 1, 999),
        )
        .unwrap();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        let mut keys: Vec<String> = read_turns(&dir, now).into_iter().map(|t| t.key).collect();
        keys.sort();
        assert_eq!(keys, vec!["msg_a:req_a", "msg_b:req_b"]);
        assert_eq!(
            read_turns(&dir, now + 2 * WEEK_MS),
            vec![],
            "files last written more than a week ago aren't opened"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_folder_is_claude_config_dir_or_dot_claude_in_home() {
        let home = Path::new("/home/user");
        assert_eq!(claude_dir(None, home), PathBuf::from("/home/user/.claude"));
        assert_eq!(
            claude_dir(Some("  "), home),
            PathBuf::from("/home/user/.claude")
        );
        assert_eq!(
            claude_dir(Some("/data/claude"), home),
            PathBuf::from("/data/claude")
        );
    }
}
