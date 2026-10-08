//! Spike-3a session search over spans: History and the palette find tool
//! steps, not only chat text. Rows come straight from `spans/*.jsonl` (a span
//! carries `chat_id` and `turn`), so there is no second index. Newest files
//! first, with caps on files and lines. The claim is redacted and capped again
//! before it is matched; args and results are never read into a row.

use std::collections::HashMap;
use std::path::Path;

use crate::harness::span::{read_spans_tail, REPLY_TOOL};

/// At most this many span files, newest first.
pub const SPAN_SEARCH_FILES: usize = 200;
/// At most this many span lines across those files.
pub const SPAN_SEARCH_LINES: usize = 20_000;
/// At most this many rows come back.
pub const SPAN_SEARCH_HITS: usize = 40;
/// How much of a claim a row keeps.
const ROW_CLAIM_CAP: usize = 120;

/// One tool step that matched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpanHit {
    pub chat_id: String,
    pub turn: u32,
    pub tool: String,
    pub decision: String,
    /// `chat · turn N · tool · decision`
    pub line: String,
}

impl SpanHit {
    /// History / palette target: `step:<turn>:<chat_id>`.
    pub fn target(&self) -> String {
        format!("step:{}:{}", self.turn, self.chat_id)
    }
}

/// Rows plus what the caps let through.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SpanSearch {
    pub hits: Vec<SpanHit>,
    pub files_read: usize,
    pub lines_read: usize,
}

/// `(mtime, file stem)` of every `spans/*.jsonl`, newest first.
fn span_files(config_dir: &Path) -> Vec<String> {
    let Ok(read) = std::fs::read_dir(config_dir.join("spans")) else {
        return Vec::new();
    };
    let mut files: Vec<(std::time::SystemTime, String)> = read
        .flatten()
        .filter_map(|e| {
            let path = e.path();
            if path.extension().and_then(|x| x.to_str()) != Some("jsonl") {
                return None;
            }
            let stem = path.file_stem()?.to_str()?.to_string();
            let at = e.metadata().and_then(|m| m.modified()).unwrap_or(std::time::UNIX_EPOCH);
            Some((at, stem))
        })
        .collect();
    files.sort_by(|a, b| b.cmp(a));
    files.into_iter().map(|(_, stem)| stem).collect()
}

/// Steps whose chat title, tool, decision or claim hold every word of `q`.
/// Only chats in `titles` (id → title) are searched, so scratch and deleted
/// chats stay out. Reads files, so call it off the UI thread.
pub fn search_spans(config_dir: &Path, q: &str, titles: &HashMap<String, String>, held: &[String]) -> SpanSearch {
    let words: Vec<String> = q.split_whitespace().map(|w| w.to_lowercase()).collect();
    let mut out = SpanSearch::default();
    if words.is_empty() {
        return out;
    }
    for stem in span_files(config_dir).into_iter().take(SPAN_SEARCH_FILES) {
        let budget = SPAN_SEARCH_LINES - out.lines_read;
        if budget == 0 || out.hits.len() == SPAN_SEARCH_HITS {
            break;
        }
        let (spans, read) = read_spans_tail(config_dir, &stem, budget);
        out.files_read += 1;
        out.lines_read += read;
        for span in spans.iter().rev() {
            if span.tool == REPLY_TOOL {
                continue;
            }
            let Some(title) = titles.get(&span.chat_id) else {
                continue;
            };
            let claim = grokhub_core::redact_held_secrets(&grokhub_core::redact_secrets(&span.claim), held);
            let claim: String = claim.chars().take(ROW_CLAIM_CAP).collect();
            let hay = format!("{title} {} {} {claim}", span.tool, span.decision).to_lowercase();
            if !words.iter().all(|w| hay.contains(w.as_str())) {
                continue;
            }
            let line = format!("{title} · turn {} · {} · {}", span.turn, span.tool, span.decision);
            // Two chats can share a title ("New chat"): one row per chat.
            if out.hits.iter().any(|h| {
                h.chat_id == span.chat_id && h.turn == span.turn && h.tool == span.tool && h.decision == span.decision
            }) {
                continue;
            }
            out.hits.push(SpanHit {
                chat_id: span.chat_id.clone(),
                turn: span.turn,
                tool: span.tool.clone(),
                decision: span.decision.clone(),
                line,
            });
            if out.hits.len() == SPAN_SEARCH_HITS {
                break;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::access::AccessMode;
    use crate::harness::span::{append_span, span_path, Span};

    fn step(chat: &str, ts: u64, turn: u32, tool: &str, decision: &str, claim: &str) -> Span {
        let mut s = Span::soft_allow(chat, tool, "{}", "ok", claim, AccessMode::Supervised, "grok_build").in_turn(chat, turn);
        s.ts_ms = ts;
        s.decision = decision.into();
        s
    }

    fn titles(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect()
    }

    #[test]
    fn click_ok_finds_the_step_row_newest_first_and_skips_unknown_chats() {
        let dir = crate::harness::test_dir("span-search");
        append_span(&dir, &step("chat-a", 1, 2, "click", "allow", "click OK")).unwrap();
        append_span(&dir, &step("chat-a", 2, 2, "type", "allow", "grokhub-desktop type")).unwrap();
        append_span(&dir, &step("chat-a", 3, 3, "click", "deny", "click OK on the pay sheet sk-abcdefghijklmnopqrstuv")).unwrap();
        append_span(&dir, &Span::reply("chat-a", "I clicked OK.", &[]).in_turn("chat-a", 3)).unwrap();
        append_span(&dir, &step("scratch-1", 4, 1, "click", "allow", "click OK")).unwrap();
        let names = titles(&[("chat-a", "Harbor")]);

        let got = search_spans(&dir, "click OK", &names, &[]);
        let lines: Vec<&str> = got.hits.iter().map(|h| h.line.as_str()).collect();
        assert_eq!(lines, vec!["Harbor · turn 3 · click · deny", "Harbor · turn 2 · click · allow"]);
        assert_eq!(got.hits[1].target(), "step:2:chat-a");
        assert_eq!((got.files_read, got.lines_read), (2, 5));
        assert_eq!(search_spans(&dir, "harbor deny", &names, &[]).hits.len(), 1);
        assert!(search_spans(&dir, "sk-abcdefghijklmnopqrstuv", &names, &[]).hits.is_empty(), "a secret in a claim is not searchable");
        assert!(search_spans(&dir, "  ", &names, &[]).hits.is_empty());
        assert!(search_spans(&dir, "nothing like this", &names, &[]).hits.is_empty());

        // A bad line does not hide the rest of the file.
        let path = span_path(&dir, "chat-a");
        let mut text = std::fs::read_to_string(&path).unwrap();
        text.push_str("not a span\n");
        std::fs::write(&path, text).unwrap();
        assert_eq!(search_spans(&dir, "click OK", &names, &[]).hits.len(), 2);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn two_chats_with_the_same_title_each_keep_their_row() {
        let dir = crate::harness::test_dir("span-search-same-title");
        append_span(&dir, &step("chat-a", 1, 1, "click", "allow", "click OK")).unwrap();
        append_span(&dir, &step("chat-b", 2, 1, "click", "allow", "click OK")).unwrap();
        let names = titles(&[("chat-a", "New chat"), ("chat-b", "New chat")]);
        let got = search_spans(&dir, "click OK", &names, &[]);
        let mut targets: Vec<String> = got.hits.iter().map(|h| h.target()).collect();
        targets.sort();
        assert_eq!(targets, vec!["step:1:chat-a".to_string(), "step:1:chat-b".to_string()]);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn caps_hold_on_three_hundred_files() {
        let dir = crate::harness::test_dir("span-search-caps");
        let spans = dir.join("spans");
        std::fs::create_dir_all(&spans).unwrap();
        let mut names = HashMap::new();
        let base = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000);
        for i in 0..300u32 {
            let chat = format!("chat-{i:03}");
            let line = serde_json::to_string(&step(&chat, 1, 1, "scroll", "allow", "scroll the list")).unwrap();
            let path = spans.join(format!("{chat}.jsonl"));
            std::fs::write(&path, format!("{line}\n").repeat(150)).unwrap();
            let f = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
            f.set_modified(base + std::time::Duration::from_secs(u64::from(i))).unwrap();
            names.insert(chat.clone(), chat);
        }
        let got = search_spans(&dir, "scroll", &names, &[]);
        assert_eq!(got.hits.len(), SPAN_SEARCH_HITS);
        assert_eq!(got.hits[0].chat_id, "chat-299", "newest file first");
        assert_eq!(got.hits[39].chat_id, "chat-260");
        // A query with no hits walks to the caps: 200 files would be 30,000
        // lines, so the line cap stops it first.
        let miss = search_spans(&dir, "zzz", &names, &[]);
        assert!(miss.hits.is_empty());
        assert_eq!(miss.lines_read, SPAN_SEARCH_LINES);
        assert_eq!(miss.files_read, 134);

        // One line per file: the file cap stops it at 200 of 300.
        for i in 0..300u32 {
            let chat = format!("chat-{i:03}");
            let line = serde_json::to_string(&step(&chat, 1, 1, "scroll", "allow", "scroll the list")).unwrap();
            std::fs::write(spans.join(format!("{chat}.jsonl")), format!("{line}\n")).unwrap();
        }
        let few = search_spans(&dir, "zzz", &names, &[]);
        assert_eq!((few.files_read, few.lines_read), (SPAN_SEARCH_FILES, SPAN_SEARCH_FILES));
        let _ = std::fs::remove_dir_all(dir);
    }
}
