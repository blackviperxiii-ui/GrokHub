//! Claude Code usage for the Fleet page, from its local transcripts
//! (`<claude dir>/projects/**/*.jsonl`). Each assistant line carries the
//! reply's token counts and time, so the page can show the current 5-hour
//! window and the last 7 days. Read-only: only `.jsonl` files under
//! `projects/` are opened, never Claude Code's settings or sign-in.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::Value;

pub const WINDOW_MS: u64 = 5 * 60 * 60 * 1000;
pub const WEEK_MS: u64 = 7 * 24 * 60 * 60 * 1000;
const HOUR_MS: u64 = 60 * 60 * 1000;
/// How deep under `projects/` transcripts are looked for (a project's
/// subagent transcripts sit two folders below it).
const MAX_DEPTH: usize = 4;

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
    /// When the open window ends; `None` when no window is open.
    pub window_resets_at_ms: Option<u64>,
    pub week: Tokens,
    pub last_at_ms: Option<u64>,
}

/// The reply on one transcript line, or `None` for any other line.
pub fn parse_line(line: &str) -> Option<Turn> {
    if !line.contains("\"usage\"") {
        return None;
    }
    let v: Value = serde_json::from_str(line).ok()?;
    if v.get("type")?.as_str()? != "assistant" {
        return None;
    }
    let at_ms = crate::pulse_sources::parse_date_ms(v.get("timestamp")?.as_str()?)?;
    let message = v.get("message")?;
    let usage = message.get("usage")?;
    let n = |key: &str| usage.get(key).and_then(Value::as_u64).unwrap_or(0);
    let id = |v: Option<&Value>| v.and_then(Value::as_str).unwrap_or("").to_string();
    Some(Turn {
        at_ms,
        key: format!("{}:{}", id(message.get("id")), id(v.get("requestId"))),
        tokens: Tokens {
            input: n("input_tokens"),
            output: n("output_tokens"),
            cache_write: n("cache_creation_input_tokens"),
            cache_read: n("cache_read_input_tokens"),
        },
    })
}

/// The 5-hour window and the 7 days up to `now`. A window opens on the hour
/// of the first reply after the last one ended, as Claude's plan limits
/// count them.
pub fn usage(turns: impl IntoIterator<Item = Turn>, now: u64) -> ClaudeUsage {
    let from = now.saturating_sub(WEEK_MS + WINDOW_MS);
    let mut seen = HashSet::new();
    let mut turns: Vec<Turn> = turns
        .into_iter()
        .filter(|t| t.at_ms >= from && t.at_ms <= now)
        .filter(|t| t.key == ":" || seen.insert(t.key.clone()))
        .collect();
    turns.sort_by_key(|t| t.at_ms);
    let mut out = ClaudeUsage {
        last_at_ms: turns.last().map(|t| t.at_ms),
        ..Default::default()
    };
    let mut start: Option<u64> = None;
    for turn in &turns {
        if start.is_none_or(|s| turn.at_ms >= s + WINDOW_MS) {
            start = Some(turn.at_ms - turn.at_ms % HOUR_MS);
            out.window = Tokens::default();
        }
        out.window.add(turn.tokens);
        if turn.at_ms >= now.saturating_sub(WEEK_MS) {
            out.week.add(turn.tokens);
        }
    }
    match start.map(|s| s + WINDOW_MS).filter(|end| now < *end) {
        Some(end) => out.window_resets_at_ms = Some(end),
        None => out.window = Tokens::default(),
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
pub fn read_turns(claude_dir: &Path, now: u64) -> Vec<Turn> {
    let since = now.saturating_sub(WEEK_MS + WINDOW_MS);
    let mut files = Vec::new();
    collect(&claude_dir.join("projects"), 0, since, &mut files);
    files
        .iter()
        .filter_map(|path| std::fs::read_to_string(path).ok())
        .flat_map(|text| text.lines().filter_map(parse_line).collect::<Vec<_>>())
        .collect()
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
        let got = usage(turns, at(3, 0));
        assert_eq!(got.window.output, 150);
        assert_eq!(got.window_resets_at_ms, Some(NOON + 5 * HOUR_MS));
        assert_eq!(got.week.output, 150);
        assert_eq!(got.last_at_ms, Some(at(2, 0)));
    }

    #[test]
    fn a_reply_after_the_window_ends_opens_a_new_one_and_a_closed_one_reads_zero() {
        let at = |h: u64, m: u64| NOON + h * HOUR_MS + m * 60_000;
        let turns = vec![turn(at(0, 10), "a", 100), turn(at(5, 30), "b", 7)];
        let open = usage(turns.clone(), at(6, 0));
        assert_eq!(
            (open.window.output, open.window_resets_at_ms),
            (7, Some(at(10, 0)))
        );
        assert_eq!(open.week.output, 107);
        let closed = usage(turns, at(10, 0));
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
        let got = usage(turns, NOON);
        assert_eq!(got.week.output, 5);
        assert_eq!(got.last_at_ms, Some(NOON - WEEK_MS + 1));
    }

    #[test]
    fn transcripts_are_read_from_projects_only_and_old_files_are_skipped() {
        let dir = std::env::temp_dir().join(format!("claude-usage-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let deep = dir.join("projects/my-app/session-1/subagents");
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
