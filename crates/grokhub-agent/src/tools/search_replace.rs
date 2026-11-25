// Portions derived from xai-org/grok-build (Apache-2.0, © SpaceXAI), commit 2bdd1d6a; modified.

use std::borrow::Cow;
use std::path::Path;

use serde_json::{json, Value};

use super::lock::with_file_lock;
use super::{confine, str_field, ToolOutput};

pub fn schema() -> Value {
    json!({
        "type": "function",
        "name": "search_replace",
        "description": "Replace an exact string in a workspace file. Fails when the string is missing or appears more than once, unless replace_all is set. CRLF files stay CRLF.",
        "parameters": {
            "type": "object",
            "properties": {
                "file_path": {"type": "string"},
                "old_string": {"type": "string"},
                "new_string": {"type": "string"},
                "replace_all": {"type": "boolean", "description": "Replace every match. Default false."}
            },
            "required": ["file_path", "old_string", "new_string"],
            "additionalProperties": false
        }
    })
}

/// Lines of context shown around each replacement.
const CONTEXT_LINES: usize = 3;
/// A window longer than this shows its first lines and says how many follow.
const WINDOW_LINES: usize = 30;
/// Windows shown at most; the answer says how many replacements it left out.
const MAX_WINDOWS: usize = 3;
/// Line numbers named at most when the string is found more than once.
const MAX_SITES_NAMED: usize = 10;
/// A miss looks for the closest lines only below this many comparisons.
const CLOSEST_WORK: usize = 2_000_000;

pub fn run(workspace: &Path, session: Option<&str>, args: &Value) -> ToolOutput {
    let raw = path_arg(args);
    if raw.is_empty() {
        return ToolOutput::err("file_path is required");
    }
    let Some(old) = args.get("old_string").and_then(|v| v.as_str()) else {
        return ToolOutput::err("old_string is required");
    };
    let Some(new) = args.get("new_string").and_then(|v| v.as_str()) else {
        return ToolOutput::err("new_string is required");
    };
    if old.is_empty() {
        return ToolOutput::err("old_string must not be empty");
    }
    let needle = old.replace("\r\n", "\n");
    let replacement = new.replace("\r\n", "\n");
    if needle == replacement {
        return ToolOutput::err("Old string and new string are the same");
    }
    let replace_all = args
        .get("replace_all")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let path = match confine(workspace, &raw) {
        Ok(path) => path,
        Err(err) => return ToolOutput::err(err),
    };
    if let Some(why) = super::person_only_file(&grokhub_core::paths::config_dir(), &path) {
        return ToolOutput::err(why);
    }
    let redacted = crate::memory::redact_model_text(&path, &replacement);
    let note = if redacted != replacement {
        " (secrets redacted)"
    } else {
        ""
    };
    let edit = Edit {
        path: &path,
        raw: &raw,
        session,
        needle: &needle,
        replacement: &redacted,
        replace_all,
    };
    crate::checkpoint::before_write(&path);
    let replaced = with_file_lock(&path, || apply(&edit));
    match replaced {
        Ok(Edited { count, shown }) => {
            crate::memory::after_model_write(workspace, &path);
            super::taint::after_write(workspace, &path, false);
            let head = format!("replaced {count} in {raw}{note}");
            if shown.is_empty() {
                ToolOutput::ok(head)
            } else {
                ToolOutput::ok(format!("{head}\n{}", file_lines(workspace, &path, &shown)))
            }
        }
        Err(Missed { why, near }) if near.is_empty() => ToolOutput::err(why),
        Err(Missed { why, near }) => {
            ToolOutput::err(format!("{why}{}", file_lines(workspace, &path, &near)))
        }
    }
}

/// Numbered lines of the file, fenced like `read_file` fences them when the
/// file holds stored outside text, so an edit's answer is no way around it.
fn file_lines(workspace: &Path, path: &Path, lines: &str) -> String {
    if super::taint::is_tainted(workspace, path) {
        format!(
            "{}\n",
            super::fence::fence("search_replace:stored", lines.trim_end_matches('\n'))
        )
    } else {
        lines.to_string()
    }
}

fn path_arg(args: &Value) -> String {
    for key in ["file_path", "path", "target_file"] {
        let value = str_field(args, key);
        if !value.is_empty() {
            return value;
        }
    }
    String::new()
}

struct Edit<'a> {
    path: &'a Path,
    /// The path as the model wrote it, for the answer.
    raw: &'a str,
    session: Option<&'a str>,
    needle: &'a str,
    replacement: &'a str,
    replace_all: bool,
}

/// An edit that applied: how many replacements, and the changed lines
/// numbered with a little context.
struct Edited {
    count: usize,
    shown: String,
}

/// Why an edit did not apply, and numbered lines of the file that help
/// (empty when there are none).
struct Missed {
    why: String,
    near: String,
}

impl From<String> for Missed {
    fn from(why: String) -> Self {
        Self {
            why,
            near: String::new(),
        }
    }
}

fn apply(edit: &Edit<'_>) -> Result<Edited, Missed> {
    let path = edit.path;
    if path.is_dir() {
        return Err(Missed::from("path is a directory".to_string()));
    }
    let bytes = std::fs::read(path).map_err(|err| {
        if err.kind() == std::io::ErrorKind::NotFound {
            "file not found".to_string()
        } else {
            format!("could not read file: {err}")
        }
    })?;
    if super::seen::changed(edit.session, path, &bytes) {
        return Err(Missed::from(super::seen::refusal(edit.raw)));
    }
    let text = String::from_utf8(bytes).map_err(|_| "file is not utf-8".to_string())?;
    let has_crlf = text.contains("\r\n");
    let match_text = if has_crlf {
        text.replace("\r\n", "\n")
    } else {
        text
    };
    let sparse = crate::diet::current().sparse_line_numbers;
    let positions: Vec<usize> = match_text
        .match_indices(edit.needle)
        .map(|(index, _)| index)
        .collect();
    if positions.is_empty() {
        let lines: Vec<&str> = match_text.lines().collect();
        let (head, near) = closest(&lines, edit.needle, sparse).unwrap_or_default();
        return Err(Missed {
            why: format!(
                "The string to replace was not found in the file, use the read_file tool to see the correct string.{head}"
            ),
            near,
        });
    }
    if positions.len() > 1 && !edit.replace_all {
        return Err(Missed::from(format!(
            "The string to replace was found multiple times in the file ({}). Use replace_all to replace all occurrences, or include more context to only edit one occurrence.",
            line_list(&match_text, &positions)
        )));
    }
    let used = if edit.replace_all {
        positions.as_slice()
    } else {
        &positions[..1]
    };
    let (new_text, starts) =
        replace_using_positions(&match_text, used, edit.needle, edit.replacement);
    let write_text = if has_crlf {
        Cow::Owned(new_text.replace("\r\n", "\n").replace('\n', "\r\n"))
    } else {
        Cow::Borrowed(new_text.as_str())
    };
    std::fs::write(path, write_text.as_bytes())
        .map_err(|err| format!("could not write file: {err}"))?;
    // The chat's own edit is what it knows of the file now: its next edit
    // needs no read first.
    super::seen::note(edit.session, path, write_text.as_bytes());
    let span = edit
        .replacement
        .strip_suffix('\n')
        .unwrap_or(edit.replacement)
        .matches('\n')
        .count();
    let mut sites = Vec::with_capacity(starts.len());
    let mut line = 0usize;
    let mut from = 0usize;
    for start in starts {
        line += new_text[from..start].matches('\n').count();
        from = start;
        sites.push((line, line + span));
    }
    Ok(Edited {
        count: used.len(),
        shown: changed_lines(&new_text, &sites, sparse),
    })
}

/// "lines 1, 3": the 1-based lines that `positions` start on, at most
/// [`MAX_SITES_NAMED`] of them.
fn line_list(text: &str, positions: &[usize]) -> String {
    let mut names = Vec::new();
    let mut line = 1usize;
    let mut from = 0usize;
    for pos in positions.iter().take(MAX_SITES_NAMED) {
        line += text[from..*pos].matches('\n').count();
        from = *pos;
        names.push(line.to_string());
    }
    let more = if positions.len() > MAX_SITES_NAMED {
        ", …"
    } else {
        ""
    };
    format!("lines {}{more}", names.join(", "))
}

/// `new_text`'s lines around each replacement, numbered like `read_file`.
/// `sites` are the 0-based first and last line of each replacement, in
/// order. Windows that touch merge; past [`MAX_WINDOWS`] the rest are
/// counted, and a window past [`WINDOW_LINES`] shows its start.
fn changed_lines(new_text: &str, sites: &[(usize, usize)], sparse: bool) -> String {
    let lines: Vec<&str> = new_text.lines().collect();
    let Some(last_line) = lines.len().checked_sub(1) else {
        return String::new();
    };
    // (first line, last line, replacements inside) of each window.
    let mut windows: Vec<(usize, usize, usize)> = Vec::new();
    for &(first, last) in sites {
        let first = first.min(last_line);
        let from = first.saturating_sub(CONTEXT_LINES);
        let to = last.saturating_add(CONTEXT_LINES).min(last_line).max(first);
        match windows.last_mut() {
            Some(open) if from <= open.1 + 1 => {
                open.1 = open.1.max(to);
                open.2 += 1;
            }
            _ => windows.push((from, to, 1)),
        }
    }
    let mut parts = Vec::new();
    for &(from, to, _) in windows.iter().take(MAX_WINDOWS) {
        let len = to - from + 1;
        let shown = len.min(WINDOW_LINES);
        let mut part = super::read_file::numbered_lines(&lines, from, from + shown, sparse);
        if len > shown {
            part.push_str(&format!(
                "(… {} more lines; read_file to see them)\n",
                len - shown
            ));
        }
        parts.push(part);
    }
    let mut out = parts.join("…\n");
    let left: usize = windows.iter().skip(MAX_WINDOWS).map(|w| w.2).sum();
    match left {
        0 => {}
        1 => out.push_str("(and 1 more replacement)\n"),
        _ => out.push_str(&format!("(and {left} more replacements)\n")),
    }
    out
}

/// On a miss: the lines that best match `needle` line by line, ignoring
/// spacing at either end. The heading says whether every non-blank line
/// matched; the rows are numbered. `None` when no start matches at least
/// half the needle's non-blank lines, so nothing is guessed.
fn closest(lines: &[&str], needle: &str, sparse: bool) -> Option<(String, String)> {
    let wanted: Vec<&str> = needle.lines().map(str::trim).collect();
    let k = wanted.len();
    let nonblank = wanted.iter().filter(|line| !line.is_empty()).count();
    if nonblank == 0 || lines.is_empty() || lines.len().saturating_mul(k) > CLOSEST_WORK {
        return None;
    }
    let need = nonblank.div_ceil(2).max(1);
    let mut best: Option<(usize, usize)> = None;
    for i in 0..lines.len() {
        let score = wanted
            .iter()
            .zip(&lines[i..])
            .filter(|(want, have)| !want.is_empty() && **want == have.trim())
            .count();
        if score >= need && best.is_none_or(|(_, top)| score > top) {
            best = Some((i, score));
        }
    }
    let (i, score) = best?;
    let end = (i + k).min(lines.len());
    let (a, b) = (i + 1, end);
    let head = match (score == nonblank, a == b) {
        (true, true) => format!("\nLine {a} matches it except for spacing:\n"),
        (true, false) => format!("\nLines {a}-{b} match it except for spacing:\n"),
        (false, _) => {
            format!("\nThe closest lines, {a}-{b} ({score} of {nonblank} lines match):\n")
        }
    };
    Some((
        head,
        super::read_file::numbered_lines(lines, i, end, sparse),
    ))
}

/// `text` with `old` at each of `positions` replaced by `new`, and where each
/// replacement starts in the result.
fn replace_using_positions(
    text: &str,
    positions: &[usize],
    old: &str,
    new: &str,
) -> (String, Vec<usize>) {
    let mut out = String::with_capacity(text.len().saturating_add(new.len()));
    let mut starts = Vec::with_capacity(positions.len());
    let mut last = 0usize;
    for pos in positions {
        out.push_str(&text[last..*pos]);
        starts.push(out.len());
        out.push_str(new);
        last = pos.saturating_add(old.len());
    }
    out.push_str(&text[last..]);
    (out, starts)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "gh-edit-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn edit(
        dir: &std::path::Path,
        file: &str,
        old: &str,
        new: &str,
        replace_all: bool,
    ) -> ToolOutput {
        let args = serde_json::json!({
            "file_path": file,
            "old_string": old,
            "new_string": new,
            "replace_all": replace_all,
        });
        run(dir, None, &args)
    }

    #[test]
    fn no_match_multiple_replace_all_and_crlf() {
        let dir = scratch("fix");
        let path = dir.join("note.txt");
        std::fs::write(&path, "one\ntwo\none\n").unwrap();

        let missing = edit(&dir, "note.txt", "gone", "x", false);
        assert!(missing.failed);
        assert!(missing.text.contains("not found"), "{}", missing.text);
        assert_eq!(std::fs::read(&path).unwrap(), b"one\ntwo\none\n");

        let many = edit(&dir, "note.txt", "one", "ONE", false);
        assert!(many.failed, "{}", many.text);
        assert!(many.text.contains("multiple"), "{}", many.text);
        assert_eq!(std::fs::read(&path).unwrap(), b"one\ntwo\none\n");

        let all = edit(&dir, "note.txt", "one", "ONE", true);
        assert!(!all.failed, "{}", all.text);
        assert_eq!(std::fs::read(&path).unwrap(), b"ONE\ntwo\nONE\n");

        let lf = dir.join("lf.txt");
        std::fs::write(&lf, b"alpha\nbeta\n").unwrap();
        let kept = edit(&dir, "lf.txt", "alpha", "ALPHA", false);
        assert!(!kept.failed, "{}", kept.text);
        assert_eq!(std::fs::read(&lf).unwrap(), b"ALPHA\nbeta\n");

        let crlf = dir.join("crlf.txt");
        std::fs::write(&crlf, b"line1\r\nline2\r\nline3\r\n").unwrap();
        let once = edit(&dir, "crlf.txt", "line2", "LINE2", false);
        assert!(!once.failed, "{}", once.text);
        assert_eq!(
            std::fs::read(&crlf).unwrap(),
            b"line1\r\nLINE2\r\nline3\r\n"
        );

        let crlf_needle = dir.join("needle.txt");
        std::fs::write(&crlf_needle, b"a\r\nb\r\n").unwrap();
        let from_cr = edit(&dir, "needle.txt", "a\r\n", "A\r\n", false);
        assert!(!from_cr.failed, "{}", from_cr.text);
        assert_eq!(std::fs::read(&crlf_needle).unwrap(), b"A\r\nb\r\n");

        let mixed = dir.join("mixed.txt");
        std::fs::write(&mixed, b"line1\r\nline2\nline3\r\nline4\n").unwrap();
        let mid = edit(&dir, "mixed.txt", "line2\nline3", "REPLACED", false);
        assert!(!mid.failed, "{}", mid.text);
        assert_eq!(
            std::fs::read(&mixed).unwrap(),
            b"line1\r\nREPLACED\r\nline4\r\n"
        );

        let all_crlf = dir.join("all.txt");
        std::fs::write(&all_crlf, b"x\r\nx\r\n").unwrap();
        let both = edit(&dir, "all.txt", "x", "y", true);
        assert!(!both.failed, "{}", both.text);
        assert_eq!(std::fs::read(&all_crlf).unwrap(), b"y\r\ny\r\n");

        let same = edit(&dir, "note.txt", "ONE", "ONE", false);
        assert!(same.failed);
        assert!(same.text.contains("same"), "{}", same.text);

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A workspace and a config folder side by side, the config pinned for
    /// this thread so a chat's tool calls write nothing in the real one.
    fn pinned(tag: &str) -> (std::path::PathBuf, crate::perm::ConfigGuard) {
        let root = scratch(tag);
        let ws = root.join("ws");
        std::fs::create_dir_all(&ws).unwrap();
        let cfg = crate::perm::ConfigGuard::set(root.join("cfg"));
        (ws, cfg)
    }

    /// One tool call through the real dispatch, as chat `session`.
    fn call(ws: &std::path::Path, session: Option<&str>, name: &str, args: Value) -> ToolOutput {
        let ctx = crate::tools::ToolCtx {
            workspace: ws,
            desktop: None,
            stop: &|| false,
            tasks: None,
            owner: None,
            session,
        };
        crate::tools::dispatch(&ctx, name, &args.to_string())
    }

    fn replace(
        ws: &std::path::Path,
        session: Option<&str>,
        file: &str,
        old: &str,
        new: &str,
    ) -> ToolOutput {
        call(
            ws,
            session,
            "search_replace",
            serde_json::json!({"file_path": file, "old_string": old, "new_string": new}),
        )
    }

    #[test]
    fn an_edit_answers_with_the_changed_lines_numbered() {
        let (ws, _cfg) = pinned("window");
        std::fs::write(ws.join("note.txt"), "a\nb\nc\nd\ne\nf\ng\nh\ni\n").unwrap();
        let one = replace(&ws, None, "note.txt", "e", "E");
        assert!(!one.failed, "{}", one.text);
        assert_eq!(
            one.text,
            "replaced 1 in note.txt\n2|b\n3|c\n4|d\n5|E\n6|f\n7|g\n8|h\n"
        );
        let grown = replace(&ws, None, "note.txt", "d\nE", "D\nE\nX");
        assert_eq!(
            grown.text,
            "replaced 1 in note.txt\n1|a\n2|b\n3|c\n4|D\n5|E\n6|X\n7|f\n8|g\n9|h\n"
        );

        std::fs::write(ws.join("crlf.txt"), "l1\r\nl2\r\nl3\r\n").unwrap();
        let crlf = replace(&ws, None, "crlf.txt", "l2", "L2");
        assert_eq!(crlf.text, "replaced 1 in crlf.txt\n1|l1\n2|L2\n3|l3\n");
        assert_eq!(
            std::fs::read(ws.join("crlf.txt")).unwrap(),
            b"l1\r\nL2\r\nl3\r\n"
        );

        std::fs::write(ws.join("long.txt"), "x\n").unwrap();
        let forty: Vec<String> = (1..=40).map(|n| format!("n{n}")).collect();
        let long = replace(&ws, None, "long.txt", "x", &forty.join("\n"));
        let rows: String = (1..=30).map(|n| format!("{n}|n{n}\n")).collect();
        assert_eq!(
            long.text,
            format!("replaced 1 in long.txt\n{rows}(… 10 more lines; read_file to see them)\n")
        );

        // Replacements far apart get a window each; the fourth is counted.
        let body: String = (1..=40)
            .map(|n| {
                if n % 10 == 0 {
                    "hit\n".to_string()
                } else {
                    format!("{n}\n")
                }
            })
            .collect();
        std::fs::write(ws.join("many.txt"), body).unwrap();
        let all = call(
            &ws,
            None,
            "search_replace",
            serde_json::json!({"file_path": "many.txt", "old_string": "hit", "new_string": "HIT", "replace_all": true}),
        );
        assert_eq!(
            all.text,
            "replaced 4 in many.txt\n7|7\n8|8\n9|9\n10|HIT\n11|11\n12|12\n13|13\n…\n17|17\n18|18\n19|19\n20|HIT\n21|21\n22|22\n23|23\n…\n27|27\n28|28\n29|29\n30|HIT\n31|31\n32|32\n33|33\n(and 1 more replacement)\n"
        );
    }

    #[test]
    fn a_miss_shows_the_closest_lines_and_many_matches_name_their_lines() {
        let (ws, _cfg) = pinned("miss");
        std::fs::write(ws.join("main.rs"), "fn main() {\n    let x = 1;\n}\n").unwrap();
        let spaced = replace(
            &ws,
            None,
            "main.rs",
            "fn main() {\n  let x = 1;",
            "fn main() {\n  let x = 2;",
        );
        assert!(spaced.failed, "{}", spaced.text);
        assert_eq!(
            spaced.text,
            "The string to replace was not found in the file, use the read_file tool to see the correct string.\nLines 1-2 match it except for spacing:\n1|fn main() {\n2|    let x = 1;\n"
        );
        let near = replace(&ws, None, "main.rs", "fn main() {\nlet y = 1;\n}", "x");
        assert_eq!(
            near.text,
            "The string to replace was not found in the file, use the read_file tool to see the correct string.\nThe closest lines, 1-3 (2 of 3 lines match):\n1|fn main() {\n2|    let x = 1;\n3|}\n"
        );
        let gone = replace(&ws, None, "main.rs", "let y = 2;", "let y = 3;");
        assert!(gone.failed);
        assert_eq!(
            gone.text,
            "The string to replace was not found in the file, use the read_file tool to see the correct string."
        );
        assert_eq!(
            std::fs::read_to_string(ws.join("main.rs")).unwrap(),
            "fn main() {\n    let x = 1;\n}\n"
        );

        std::fs::write(ws.join("twice.txt"), "one\ntwo\none\n").unwrap();
        let many = replace(&ws, None, "twice.txt", "one", "ONE");
        assert!(many.failed);
        assert_eq!(
            many.text,
            "The string to replace was found multiple times in the file (lines 1, 3). Use replace_all to replace all occurrences, or include more context to only edit one occurrence."
        );
        std::fs::write(ws.join("dozen.txt"), "z\n".repeat(12)).unwrap();
        let dozen = replace(&ws, None, "dozen.txt", "z", "Z");
        assert!(
            dozen
                .text
                .contains("(lines 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, …)"),
            "{}",
            dozen.text
        );
    }

    #[test]
    fn an_edit_refuses_a_file_changed_since_the_chat_read_it() {
        let (ws, _cfg) = pinned("stale");
        let note = ws.join("note.txt");
        std::fs::write(&note, "one\n").unwrap();
        let read = call(
            &ws,
            Some("chat-a"),
            "read_file",
            serde_json::json!({"target_file": "note.txt"}),
        );
        assert_eq!(read.text, "1|one\n");
        std::fs::write(&note, "one\nadded by the user\n").unwrap();
        let refusal = "note.txt changed since you last read it (the person, a helper or a command edited it). Nothing was changed: read it again with read_file, then edit what is there now.";

        let stale = replace(&ws, Some("chat-a"), "note.txt", "one", "ONE");
        assert!(stale.failed, "{}", stale.text);
        assert_eq!(stale.text, refusal);
        assert_eq!(
            std::fs::read_to_string(&note).unwrap(),
            "one\nadded by the user\n"
        );
        let blind = call(
            &ws,
            Some("chat-a"),
            "write",
            serde_json::json!({"path": "note.txt", "content": "x"}),
        );
        assert!(blind.failed, "{}", blind.text);
        assert_eq!(blind.text, refusal);
        assert_eq!(
            std::fs::read_to_string(&note).unwrap(),
            "one\nadded by the user\n"
        );

        // Another chat never read it, and a call with no chat (or an empty
        // one) is never refused.
        let other = replace(&ws, Some("chat-b"), "note.txt", "added", "ADDED");
        assert!(!other.failed, "{}", other.text);
        let bare = replace(&ws, None, "note.txt", "user", "person");
        assert!(!bare.failed, "{}", bare.text);
        let empty = replace(&ws, Some(""), "note.txt", "person", "user");
        assert!(!empty.failed, "{}", empty.text);

        // A fresh read lets the edit through, and the chat's own edit needs no re-read.
        let again = call(
            &ws,
            Some("chat-a"),
            "read_file",
            serde_json::json!({"target_file": "note.txt"}),
        );
        assert_eq!(again.text, "1|one\n2|ADDED by the user\n");
        let first = replace(&ws, Some("chat-a"), "note.txt", "one", "ONE");
        assert!(!first.failed, "{}", first.text);
        let second = replace(&ws, Some("chat-a"), "note.txt", "ONE", "Two");
        assert!(!second.failed, "{}", second.text);
        let whole = call(
            &ws,
            Some("chat-a"),
            "write",
            serde_json::json!({"path": "note.txt", "content": "Two\nADDED by the user\n"}),
        );
        assert!(!whole.failed, "{}", whole.text);
        assert_eq!(
            std::fs::read_to_string(&note).unwrap(),
            "Two\nADDED by the user\n"
        );

        let fresh = call(
            &ws,
            Some("chat-a"),
            "write",
            serde_json::json!({"path": "new.txt", "content": "hi"}),
        );
        assert!(!fresh.failed, "{}", fresh.text);
        assert_eq!(std::fs::read_to_string(ws.join("new.txt")).unwrap(), "hi");
    }

    #[test]
    fn a_formatter_hook_after_an_edit_costs_no_refusal() {
        let (ws, _cfg) = pinned("hook");
        let session = format!("hook-{}", std::process::id());
        let note = ws.join("note.txt");
        std::fs::write(&note, "one\n").unwrap();
        let abs = note.display().to_string();
        let append = if cfg!(windows) {
            format!("[IO.File]::AppendAllText('{abs}', \"formatted`n\")")
        } else {
            format!("printf 'formatted\\n' >> '{abs}'")
        };
        let _hooks = crate::hooks::install_test_hooks(
            &session,
            vec![crate::hooks::TestHook {
                event: "PostToolUse".into(),
                matcher: "search_replace".into(),
                command: append,
                timeout: std::time::Duration::from_secs(10),
                source_dir: ws.clone(),
            }],
        );
        let chat = Some(session.as_str());
        call(
            &ws,
            chat,
            "read_file",
            serde_json::json!({"target_file": "note.txt"}),
        );
        let args =
            serde_json::json!({"file_path": "note.txt", "old_string": "one", "new_string": "ONE"});
        let out = call(&ws, chat, "search_replace", args.clone());
        assert!(!out.failed, "{}", out.text);
        // The loop runs PostToolUse hooks right after the tool.
        crate::hooks::after_tool(
            &session,
            &ws,
            "search_replace",
            &args.to_string(),
            "t1",
            false,
            "",
        );
        assert_eq!(std::fs::read_to_string(&note).unwrap(), "ONE\nformatted\n");
        let next = replace(&ws, chat, "note.txt", "ONE", "Two");
        assert!(!next.failed, "{}", next.text);
        assert_eq!(std::fs::read_to_string(&note).unwrap(), "Two\nformatted\n");
        // A change the hook did not make still costs a re-read.
        std::fs::write(&note, "Two\nby hand\n").unwrap();
        let stale = replace(&ws, chat, "note.txt", "Two", "Three");
        assert!(stale.failed, "{}", stale.text);
        assert!(
            stale
                .text
                .starts_with("note.txt changed since you last read it"),
            "{}",
            stale.text
        );
    }

    #[test]
    fn a_window_of_a_marked_file_stays_fenced() {
        let (ws, _cfg) = pinned("fenced");
        crate::tools::fence::reset_run_nonce_for_tests();
        let _ = crate::tools::fence::fence("web_fetch", "a page");
        let path = ws.join("notes.md");
        std::fs::write(&path, "from the page\nmore\n").unwrap();
        crate::tools::taint::after_write(&ws, &path, false);
        let out = replace(&ws, None, "notes.md", "more", "MORE");
        assert!(!out.failed, "{}", out.text);
        assert!(
            out.text.starts_with("replaced 1 in notes.md\n"),
            "{}",
            out.text
        );
        assert!(crate::tools::fence::looks_fenced(&out.text), "{}", out.text);
        assert!(out.text.contains("search_replace:stored"), "{}", out.text);
        assert!(out.text.contains("2|MORE"), "{}", out.text);
        crate::tools::fence::reset_run_nonce_for_tests();
    }
}
