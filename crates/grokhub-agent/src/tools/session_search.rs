//! `session_search`: find something said in an earlier chat, with no model
//! call. The user's and the assistant's messages from `sessions/*.jsonl` are
//! kept in an SQLite FTS5 table with the trigram tokenizer, so any piece of
//! a word matches. The session files stay the truth: before each search,
//! files that changed are read again and deleted chats leave the index, so
//! a deleted chat is never found.
//!
//! What is indexed passes [`grokhub_core::redact_secrets`] first, so a key
//! pasted into a chat is neither stored here nor found. Tool calls and their
//! output are not indexed, only what was said. Scratch chats are never
//! indexed, and a scratch chat can't search (see `crate::scratch`).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use rusqlite::{params, Connection};
use serde_json::{json, Value};

use super::ToolOutput;

pub const NAME: &str = "session_search";
/// Hits on one page.
const PAGE_HITS: usize = 10;
/// Messages one read around a hit shows.
const AROUND: usize = 8;
/// Characters a hit's excerpt keeps on each side of the match.
const EXCERPT_SIDE: usize = 80;
/// Characters one message shows when reading around a hit.
const MESSAGE_SHOWN: usize = 1_200;
/// The trigram tokenizer needs three characters to match.
const QUERY_MIN: usize = 3;

pub fn schema() -> Value {
    json!({
        "type": "function",
        "name": NAME,
        "description": "Search the user's earlier chats on this computer for something that was said, with no model call. Matches any piece of a word (3 or more characters) in past user and assistant messages, newest chats first, never this chat. Each hit names its chat and line. To read the messages around a hit, call again with that chat and line and no query. Quote what it returns; don't guess at old chats.",
        "parameters": {
            "type": "object",
            "properties": {
                "query": {"type": "string", "description": "Text to find, 3 or more characters."},
                "page": {"type": "integer", "minimum": 1, "description": "Page of hits, from 1."},
                "chat": {"type": "string", "description": "A chat id from a hit, to read around it."},
                "line": {"type": "integer", "minimum": 0, "description": "The hit's line in that chat."}
            },
            "required": [],
            "additionalProperties": false
        }
    })
}

fn index_path(config_dir: &Path) -> PathBuf {
    config_dir.join("session_search.sqlite")
}

fn open(config_dir: &Path) -> rusqlite::Result<Connection> {
    let db = Connection::open(index_path(config_dir))?;
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS chats(id TEXT PRIMARY KEY, stamp TEXT NOT NULL, title TEXT NOT NULL, created INTEGER NOT NULL);
         CREATE VIRTUAL TABLE IF NOT EXISTS said USING fts5(chat UNINDEXED, line UNINDEXED, role UNINDEXED, text, tokenize='trigram');",
    )?;
    Ok(db)
}

/// The session files now on disk, by id, with a stamp that changes on write.
fn on_disk(sessions: &Path) -> HashMap<String, String> {
    let Ok(rd) = std::fs::read_dir(sessions) else {
        return HashMap::new();
    };
    rd.flatten()
        .filter_map(|entry| {
            let path = entry.path();
            if path.extension().and_then(|x| x.to_str()) != Some("jsonl") {
                return None;
            }
            let id = path.file_stem()?.to_str()?.to_string();
            if crate::scratch::is_scratch(&id) {
                return None;
            }
            let meta = entry.metadata().ok()?;
            let modified = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_nanos())
                .unwrap_or(0);
            Some((id, format!("{}-{modified}", meta.len())))
        })
        .collect()
}

/// Bring the index up to date with the session files.
fn refresh(db: &mut Connection) -> rusqlite::Result<()> {
    let disk = on_disk(&crate::session::sessions_dir());
    let known: HashMap<String, String> = db
        .prepare("SELECT id, stamp FROM chats")?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    let tx = db.transaction()?;
    for id in known.keys().filter(|id| !disk.contains_key(*id)) {
        tx.execute("DELETE FROM said WHERE chat = ?1", [id])?;
        tx.execute("DELETE FROM chats WHERE id = ?1", [id])?;
    }
    for (id, stamp) in &disk {
        if known.get(id) == Some(stamp) {
            continue;
        }
        tx.execute("DELETE FROM said WHERE chat = ?1", [id])?;
        tx.execute("DELETE FROM chats WHERE id = ?1", [id])?;
        let Ok(info) = crate::session::load_session(id) else {
            continue;
        };
        for (line, (role, text)) in crate::session::message_pairs(&info).iter().enumerate() {
            let text = grokhub_core::redact_secrets(text);
            if text.trim().is_empty() {
                continue;
            }
            tx.execute(
                "INSERT INTO said(chat, line, role, text) VALUES (?1, ?2, ?3, ?4)",
                params![id, line as i64, role, text],
            )?;
        }
        tx.execute(
            "INSERT INTO chats(id, stamp, title, created) VALUES (?1, ?2, ?3, ?4)",
            params![id, stamp, info.title, info.created as i64],
        )?;
    }
    tx.commit()
}

/// `text` cut to about [`EXCERPT_SIDE`] characters each side of `needle`.
fn excerpt(text: &str, needle: &str) -> String {
    let flat: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let lower = flat.to_lowercase();
    let at = lower.find(&needle.to_lowercase()).unwrap_or(0);
    // Lowercasing can change byte lengths, so work in characters.
    let at_char = lower[..at].chars().count();
    let chars: Vec<char> = flat.chars().collect();
    let start = at_char.saturating_sub(EXCERPT_SIDE);
    let end = (at_char + needle.chars().count() + EXCERPT_SIDE).min(chars.len());
    let mut out: String = chars[start..end].iter().collect();
    if start > 0 {
        out.insert(0, '…');
    }
    if end < chars.len() {
        out.push('…');
    }
    out
}

/// `YYYY-MM-DD` (UTC) for a time in milliseconds.
fn day(created_ms: i64) -> String {
    // Days to a civil date (Howard Hinnant's algorithm).
    let z = created_ms.max(0) / 86_400_000 + 719_468;
    let era = z / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

/// One page of hits for `query`, newest chats first, leaving out `skip`.
pub fn search(
    config_dir: &Path,
    query: &str,
    page: usize,
    skip: Option<&str>,
) -> Result<String, String> {
    let query = query.trim();
    if query.chars().count() < QUERY_MIN {
        return Err(format!("Search for {QUERY_MIN} or more characters."));
    }
    let mut db = open(config_dir).map_err(|e| e.to_string())?;
    refresh(&mut db).map_err(|e| e.to_string())?;
    let phrase = format!("\"{}\"", query.replace('"', "\"\""));
    let skip = skip.unwrap_or_default();
    let total: i64 = db
        .query_row(
            "SELECT count(*) FROM said WHERE said MATCH ?1 AND chat != ?2",
            params![phrase, skip],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    if total == 0 {
        return Ok(format!("No earlier chat has \"{query}\"."));
    }
    let pages = (total as usize).div_ceil(PAGE_HITS);
    let page = page.clamp(1, pages);
    let mut stmt = db
        .prepare(
            "SELECT s.chat, s.line, s.role, s.text, c.title, c.created FROM said s JOIN chats c ON c.id = s.chat
             WHERE said MATCH ?1 AND s.chat != ?2 ORDER BY c.created DESC, s.chat, s.line LIMIT ?3 OFFSET ?4",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(
            params![
                phrase,
                skip,
                PAGE_HITS as i64,
                ((page - 1) * PAGE_HITS) as i64
            ],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, String>(4)?,
                    r.get::<_, i64>(5)?,
                ))
            },
        )
        .map_err(|e| e.to_string())?;
    let mut out = format!("{total} hits for \"{query}\", page {page} of {pages}.\n");
    for row in rows {
        let (chat, line, role, text, title, created) = row.map_err(|e| e.to_string())?;
        out.push_str(&format!(
            "\n- chat {chat} \"{title}\" ({}), line {line}, {role}: {}",
            day(created),
            excerpt(&text, query)
        ));
    }
    if page < pages {
        out.push_str(&format!("\n\nMore: page {}.", page + 1));
    }
    Ok(out)
}

/// The messages around `line` in one chat.
pub fn around(config_dir: &Path, chat: &str, line: usize) -> Result<String, String> {
    let mut db = open(config_dir).map_err(|e| e.to_string())?;
    refresh(&mut db).map_err(|e| e.to_string())?;
    let title: String = db
        .query_row("SELECT title FROM chats WHERE id = ?1", [chat], |r| {
            r.get(0)
        })
        .map_err(|_| format!("No earlier chat has the id {chat}."))?;
    let from = line.saturating_sub(AROUND / 2);
    let mut stmt = db
        .prepare(
            "SELECT line, role, text FROM said WHERE chat = ?1 AND line >= ?2 ORDER BY line LIMIT ?3",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![chat, from as i64, AROUND as i64], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        })
        .map_err(|e| e.to_string())?;
    let mut out = format!("Chat {chat} \"{title}\", from line {from}:\n");
    let mut last = None;
    for row in rows {
        let (n, role, text) = row.map_err(|e| e.to_string())?;
        let mut shown: String = text.chars().take(MESSAGE_SHOWN).collect();
        if shown.len() < text.len() {
            shown.push('…');
        }
        out.push_str(&format!("\n[{n}] {role}: {shown}\n"));
        last = Some(n);
    }
    match last {
        None => Ok(format!("Chat {chat} has nothing from line {from} on.")),
        Some(n) => {
            out.push_str(&format!("\nMore: line {}.", n + 1 + (AROUND / 2) as i64));
            Ok(out)
        }
    }
}

pub fn run(session: Option<&str>, args: &Value) -> ToolOutput {
    let dir = crate::perm::config_dir();
    let text = |k: &str| {
        args.get(k)
            .and_then(Value::as_str)
            .map(str::trim)
            .unwrap_or_default()
    };
    let number = |k: &str| args.get(k).and_then(Value::as_u64);
    let result = match (text("chat"), number("line")) {
        (chat, Some(line)) if !chat.is_empty() && text("query").is_empty() => {
            around(&dir, chat, line as usize)
        }
        _ => search(
            &dir,
            text("query"),
            number("page").unwrap_or(1) as usize,
            session,
        ),
    };
    match result {
        Ok(text) => ToolOutput::ok(text),
        Err(why) => ToolOutput::err(why),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::record_turn;
    use crate::{ContentPart, InputItem, Usage};

    fn said(role: &str, text: &str) -> InputItem {
        InputItem::Message {
            role: role.into(),
            content: vec![ContentPart::InputText(text.into())],
        }
    }

    fn turn(id: &str, ask: &str, answer: &str) {
        record_turn(
            id,
            "/work",
            "grok-4.7",
            &[said("user", ask), said("assistant", answer)],
            &Usage::default(),
            "pool",
            ask,
        )
        .unwrap();
    }

    #[test]
    fn past_chats_are_found_by_any_piece_of_a_word_and_this_chat_is_left_out() {
        let dir = crate::harness::test_dir("session-search-find");
        let _config = crate::perm::ConfigGuard::set(&dir);
        turn(
            "shell-a",
            "where did I park the trailer",
            "Lot B, by the harbor ferry.",
        );
        turn(
            "shell-b",
            "remind me about the ferry times",
            "The harbor ferry leaves at 7:15.",
        );
        turn("shell-now", "what about the ferry", "Let me look.");

        let found = search(&dir, "ferr", 1, Some("shell-now")).unwrap();
        assert!(
            found.starts_with("3 hits for \"ferr\", page 1 of 1.\n"),
            "{found}"
        );
        assert!(
            found.contains("line 1, assistant: Lot B, by the harbor ferry."),
            "{found}"
        );
        assert!(
            found.contains("line 0, user: remind me about the ferry times"),
            "{found}"
        );
        assert!(!found.contains("shell-now"), "{found}");

        assert_eq!(
            search(&dir, "submarine", 1, None).unwrap(),
            "No earlier chat has \"submarine\"."
        );
        assert_eq!(
            search(&dir, "fe", 1, None).unwrap_err(),
            "Search for 3 or more characters."
        );
    }

    #[test]
    fn reading_around_a_hit_shows_its_neighbours() {
        let dir = crate::harness::test_dir("session-search-around");
        let _config = crate::perm::ConfigGuard::set(&dir);
        turn("shell-a", "first question", "first answer");
        turn("shell-a", "second question", "second answer");
        let shown = around(&dir, "shell-a", 2).unwrap();
        assert_eq!(
            shown,
            "Chat shell-a \"first question\", from line 0:\n\n[0] user: first question\n\n[1] assistant: first answer\n\n[2] user: second question\n\n[3] assistant: second answer\n\nMore: line 8."
        );
        assert_eq!(
            around(&dir, "shell-gone", 0).unwrap_err(),
            "No earlier chat has the id shell-gone."
        );
    }

    #[test]
    fn a_deleted_chat_leaves_the_index_and_an_edited_one_is_read_again() {
        let dir = crate::harness::test_dir("session-search-fresh");
        let _config = crate::perm::ConfigGuard::set(&dir);
        turn("shell-a", "the walrus question", "walrus answer");
        assert!(search(&dir, "walrus", 1, None)
            .unwrap()
            .starts_with("2 hits"));
        turn("shell-a", "more walrus", "still walrus");
        assert!(search(&dir, "walrus", 1, None)
            .unwrap()
            .starts_with("4 hits"));
        std::fs::remove_file(crate::session::sessions_dir().join("shell-a.jsonl")).unwrap();
        assert_eq!(
            search(&dir, "walrus", 1, None).unwrap(),
            "No earlier chat has \"walrus\"."
        );
    }

    #[test]
    fn keys_and_scratch_chats_are_never_indexed() {
        let dir = crate::harness::test_dir("session-search-private");
        let _config = crate::perm::ConfigGuard::set(&dir);
        turn("shell-a", "my key is sk-abcdefghijklmnopqrstuv ok", "noted");
        turn("shell-scratch-b", "secret plans for the walrus", "ok");
        assert_eq!(
            search(&dir, "abcdefghij", 1, None).unwrap(),
            "No earlier chat has \"abcdefghij\"."
        );
        assert_eq!(
            search(&dir, "walrus", 1, None).unwrap(),
            "No earlier chat has \"walrus\"."
        );
    }

    #[test]
    fn hits_page_ten_at_a_time() {
        let dir = crate::harness::test_dir("session-search-pages");
        let _config = crate::perm::ConfigGuard::set(&dir);
        for n in 0..6 {
            turn(&format!("shell-{n}"), &format!("otter {n}"), "otter reply");
        }
        let first = search(&dir, "otter", 1, None).unwrap();
        assert!(
            first.starts_with("12 hits for \"otter\", page 1 of 2."),
            "{first}"
        );
        assert!(first.ends_with("More: page 2."), "{first}");
        assert_eq!(first.matches("\n- chat ").count(), 10);
        let second = search(&dir, "otter", 9, None).unwrap();
        assert!(
            second.starts_with("12 hits for \"otter\", page 2 of 2."),
            "{second}"
        );
        assert_eq!(second.matches("\n- chat ").count(), 2);
    }

    #[test]
    fn a_day_reads_as_its_utc_date() {
        assert_eq!(day(0), "1970-01-01");
        assert_eq!(day(1_795_651_200_000), "2026-11-26");
        assert_eq!(day(1_795_651_199_999), "2026-11-25");
    }
}
