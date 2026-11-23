//! `@` context references: expand file, folder, git diff, and URL into the turn.
//!
//! Plus-button attach stays in [`crate::attach`]. This module only expands
//! typed `@file:` / `@folder:` / `@diff` / `@staged` / `@git:N` / `@url:` tokens
//! and appends their bodies under an attached-context section.

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::attach::{bound_scan, TEXT_FILE_CAP};
use crate::git_cmd;

/// Section header appended when at least one reference expands.
pub const ATTACHED_CONTEXT_HEADER: &str = "--- Attached context ---";

/// Most folder entries listed for `@folder:`.
pub const FOLDER_ENTRY_CAP: usize = 200;

/// Most commits `@git:N` may request.
pub const GIT_COMMIT_CAP: usize = 10;

/// Soft ceiling for the whole attached section (bytes of UTF-8).
pub const ATTACHED_TOTAL_CAP: usize = TEXT_FILE_CAP * 4;

const TRAIL_PUNCT: &[char] = &[',', '.', ';', '!', '?'];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AtKind {
    File {
        path: String,
        lines: Option<(usize, usize)>,
    },
    Folder {
        path: String,
    },
    Diff,
    Staged,
    Git {
        count: usize,
    },
    Url {
        url: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AtRef {
    pub kind: AtKind,
    /// Byte range of the `@…` token in the original text (for tests / UI).
    pub span: (usize, usize),
}

/// Find `@` context references in `text`. Email-like `@` and bare `@name` are ignored.
pub fn find_at_refs(text: &str) -> Vec<AtRef> {
    let mut out = Vec::new();
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'@' {
            i += 1;
            continue;
        }
        // Skip mid-token @ (email / word).
        if i > 0 {
            let prev = bytes[i - 1];
            if prev.is_ascii_alphanumeric() || prev == b'_' || prev == b'.' {
                i += 1;
                continue;
            }
        }
        let rest = &text[i + 1..];
        if let Some((kind, consumed)) = parse_one(rest) {
            let end = i + 1 + consumed;
            out.push(AtRef {
                kind,
                span: (i, end),
            });
            i = end;
            continue;
        }
        i += 1;
    }
    out
}

fn parse_one(rest: &str) -> Option<(AtKind, usize)> {
    if let Some(after) = rest.strip_prefix("diff") {
        if token_end(after) {
            return Some((AtKind::Diff, "diff".len() + trail_len(after)));
        }
    }
    if let Some(after) = rest.strip_prefix("staged") {
        if token_end(after) {
            return Some((AtKind::Staged, "staged".len() + trail_len(after)));
        }
    }
    if let Some(tail) = rest.strip_prefix("git:") {
        let (raw, taken) = take_value(tail);
        let n: usize = raw.parse().ok()?;
        let count = n.clamp(1, GIT_COMMIT_CAP);
        return Some((AtKind::Git { count }, "git:".len() + taken));
    }
    if let Some(tail) = rest.strip_prefix("file:") {
        let (raw, taken) = take_value(tail);
        let (path, lines) = split_file_lines(raw)?;
        if path.is_empty() {
            return None;
        }
        return Some((AtKind::File { path, lines }, "file:".len() + taken));
    }
    if let Some(tail) = rest.strip_prefix("folder:") {
        let (raw, taken) = take_value(tail);
        if raw.is_empty() {
            return None;
        }
        return Some((
            AtKind::Folder {
                path: raw.to_string(),
            },
            "folder:".len() + taken,
        ));
    }
    if let Some(tail) = rest.strip_prefix("url:") {
        let (raw, taken) = take_value(tail);
        if raw.is_empty() {
            return None;
        }
        return Some((
            AtKind::Url {
                url: raw.to_string(),
            },
            "url:".len() + taken,
        ));
    }
    None
}

fn token_end(after: &str) -> bool {
    after.is_empty()
        || after.starts_with(|c: char| c.is_whitespace())
        || after.starts_with(|c: char| TRAIL_PUNCT.contains(&c))
}

fn trail_len(after: &str) -> usize {
    after
        .chars()
        .next()
        .filter(|c| TRAIL_PUNCT.contains(c))
        .map(|c| c.len_utf8())
        .unwrap_or(0)
}

/// Value until whitespace; trailing punctuation on the value is stripped.
fn take_value(s: &str) -> (&str, usize) {
    let end = s.find(|c: char| c.is_whitespace()).unwrap_or(s.len());
    let raw = &s[..end];
    let trimmed = raw.trim_end_matches(TRAIL_PUNCT);
    (trimmed, end)
}

fn split_file_lines(raw: &str) -> Option<(String, Option<(usize, usize)>)> {
    // `@file:path:10-25` or `@file:path:42`. The last `:digits(-digits)?` is the range.
    if let Some((path, range)) = raw.rsplit_once(':') {
        if let Some((a, b)) = range.split_once('-') {
            if let (Ok(start), Ok(end)) = (a.parse::<usize>(), b.parse::<usize>()) {
                if !path.is_empty() && start >= 1 && end >= start {
                    return Some((path.to_string(), Some((start, end))));
                }
            }
        } else if let Ok(line) = range.parse::<usize>() {
            if !path.is_empty() && line >= 1 {
                return Some((path.to_string(), Some((line, line))));
            }
        }
    }
    Some((raw.to_string(), None))
}

/// Expand every found reference and append bodies under [`ATTACHED_CONTEXT_HEADER`].
/// Leaves the message unchanged when there are no `@` tokens.
pub fn expand_at_context(text: &str, cwd: &Path) -> String {
    let refs = find_at_refs(text);
    if refs.is_empty() {
        return text.to_string();
    }
    let mut bodies = Vec::new();
    let mut total = 0usize;
    for r in &refs {
        if total >= ATTACHED_TOTAL_CAP {
            bodies.push(format!(
                "### {}\n(skipped: attached context hit the size cap)",
                label_of(&r.kind)
            ));
            break;
        }
        let body = resolve_one(&r.kind, cwd);
        let capped = cap_body(&body);
        total = total.saturating_add(capped.len());
        bodies.push(format!("### {}\n{capped}", label_of(&r.kind)));
    }
    if bodies.is_empty() {
        return text.to_string();
    }
    let mut out = text.trim_end().to_string();
    out.push_str("\n\n");
    out.push_str(ATTACHED_CONTEXT_HEADER);
    out.push('\n');
    out.push_str(&bodies.join("\n\n"));
    out
}

fn label_of(kind: &AtKind) -> String {
    match kind {
        AtKind::File { path, lines } => match lines {
            Some((a, b)) if a == b => format!("file:{path}:{a}"),
            Some((a, b)) => format!("file:{path}:{a}-{b}"),
            None => format!("file:{path}"),
        },
        AtKind::Folder { path } => format!("folder:{path}"),
        AtKind::Diff => "diff".into(),
        AtKind::Staged => "staged".into(),
        AtKind::Git { count } => format!("git:{count}"),
        AtKind::Url { url } => format!("url:{url}"),
    }
}

fn cap_body(s: &str) -> String {
    bound_scan(s).into_owned()
}

fn resolve_one(kind: &AtKind, cwd: &Path) -> String {
    match kind {
        AtKind::File { path, lines } => read_file(cwd, path, *lines),
        AtKind::Folder { path } => list_folder(cwd, path),
        AtKind::Diff => git_out(cwd, &["diff"]),
        AtKind::Staged => git_out(cwd, &["diff", "--staged"]),
        AtKind::Git { count } => git_out(cwd, &["log", &format!("-n{count}"), "-p", "--"]),
        AtKind::Url { url } => fetch_url(url),
    }
}

fn path_blocked(path: &str) -> Option<&'static str> {
    crate::host_safety::forbidden_reason(path)
}

fn resolve_path(cwd: &Path, path: &str) -> Result<PathBuf, String> {
    if let Some(why) = path_blocked(path) {
        return Err(why.to_string());
    }
    let p = Path::new(path);
    let joined = if p.is_absolute() {
        p.to_path_buf()
    } else {
        cwd.join(p)
    };
    let canon = joined.canonicalize().unwrap_or(joined);
    let shown = canon.to_string_lossy();
    if let Some(why) = path_blocked(&shown) {
        return Err(why.to_string());
    }
    Ok(canon)
}

fn read_file(cwd: &Path, path: &str, lines: Option<(usize, usize)>) -> String {
    let path = match resolve_path(cwd, path) {
        Ok(p) => p,
        Err(why) => return why,
    };
    let mut file = match fs::File::open(&path) {
        Ok(f) => f,
        Err(err) => return format!("can't open {}: {err}", path.display()),
    };
    let mut buf = String::new();
    if let Err(err) = file
        .by_ref()
        .take(TEXT_FILE_CAP as u64 + 1)
        .read_to_string(&mut buf)
    {
        return format!("can't read {}: {err}", path.display());
    }
    if buf.len() > TEXT_FILE_CAP {
        buf.truncate(TEXT_FILE_CAP);
        while !buf.is_char_boundary(buf.len()) {
            buf.pop();
        }
        buf.push_str("\n…");
    }
    match lines {
        Some((start, end)) => slice_lines(&buf, start, end),
        None => buf,
    }
}

fn slice_lines(text: &str, start: usize, end: usize) -> String {
    let picked: Vec<&str> = text
        .lines()
        .enumerate()
        .filter_map(|(i, line)| {
            let n = i + 1;
            (n >= start && n <= end).then_some(line)
        })
        .collect();
    if picked.is_empty() {
        // Invalid range: return the whole (already capped) file.
        return text.to_string();
    }
    picked.join("\n")
}

fn list_folder(cwd: &Path, path: &str) -> String {
    let path = match resolve_path(cwd, path) {
        Ok(p) => p,
        Err(why) => return why,
    };
    let rd = match fs::read_dir(&path) {
        Ok(rd) => rd,
        Err(err) => return format!("can't list {}: {err}", path.display()),
    };
    let mut rows: Vec<String> = Vec::new();
    for ent in rd.flatten() {
        if rows.len() >= FOLDER_ENTRY_CAP {
            rows.push("- …".into());
            break;
        }
        let name = ent.file_name().to_string_lossy().into_owned();
        let meta = ent.metadata().ok();
        let kind = match meta.as_ref().map(|m| m.is_dir()) {
            Some(true) => "dir",
            Some(false) => "file",
            None => "?",
        };
        let size = meta
            .as_ref()
            .filter(|m| m.is_file())
            .map(|m| format!(" {}", m.len()))
            .unwrap_or_default();
        rows.push(format!("- {kind} {name}{size}"));
    }
    rows.sort();
    if rows.is_empty() {
        "(empty folder)".into()
    } else {
        rows.join("\n")
    }
}

fn git_out(cwd: &Path, args: &[&str]) -> String {
    let out = match git_cmd::git().current_dir(cwd).args(args).output() {
        Ok(o) => o,
        Err(err) => return format!("git failed: {err}"),
    };
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    if !out.status.success() && stdout.trim().is_empty() {
        let err = stderr.trim();
        if err.is_empty() {
            return "git produced no output".into();
        }
        return err.to_string();
    }
    if stdout.trim().is_empty() {
        "(no changes)".into()
    } else {
        stdout.into_owned()
    }
}

fn fetch_url(url: &str) -> String {
    let url = url.trim();
    if url.is_empty() {
        return "empty url".into();
    }
    if let Err(err) = crate::offline::check_url(url) {
        return err.to_string();
    }
    if !crate::public_http_url(url) {
        return "url is not a public http(s) address".into();
    }
    let agent = crate::http::agent(crate::http::Opts {
        connect: Duration::from_secs(10),
        overall: Some(Duration::from_secs(20)),
        redirects: 3,
        public_only: true,
        ..Default::default()
    });
    match agent.get(url).call() {
        Ok(resp) => match resp.into_string() {
            Ok(body) => cap_body(&body),
            Err(err) => format!("can't read body: {err}"),
        },
        Err(err) => format!("fetch failed: {err}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "grokhub-at-context-{name}-{}-{}",
            std::process::id(),
            crate::now_ms()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp");
        dir
    }

    #[test]
    fn finds_file_folder_diff_staged_git_and_url() {
        let text = "See @file:src/a.rs:2-4, folder @folder:src, changes @diff and @staged; history @git:3. Page @url:https://example.com/x.";
        let refs = find_at_refs(text);
        assert_eq!(refs.len(), 6);
        assert_eq!(
            refs[0].kind,
            AtKind::File {
                path: "src/a.rs".into(),
                lines: Some((2, 4)),
            }
        );
        assert_eq!(refs[1].kind, AtKind::Folder { path: "src".into() });
        assert_eq!(refs[2].kind, AtKind::Diff);
        assert_eq!(refs[3].kind, AtKind::Staged);
        assert_eq!(refs[4].kind, AtKind::Git { count: 3 });
        assert_eq!(
            refs[5].kind,
            AtKind::Url {
                url: "https://example.com/x".into()
            }
        );
    }

    #[test]
    fn ignores_email_and_unknown_at() {
        assert!(find_at_refs("mail user@example.com and @someone").is_empty());
    }

    #[test]
    fn clamps_git_count() {
        let refs = find_at_refs("@git:99");
        assert_eq!(refs[0].kind, AtKind::Git { count: 10 });
        let refs = find_at_refs("@git:0");
        assert_eq!(refs[0].kind, AtKind::Git { count: 1 });
    }

    #[test]
    fn expands_file_and_folder_into_attached_section() {
        let dir = temp_dir("file");
        fs::write(dir.join("note.txt"), "one\ntwo\nthree\n").unwrap();
        fs::create_dir_all(dir.join("sub")).unwrap();
        fs::write(dir.join("sub/a.rs"), "fn a() {}\n").unwrap();
        let msg = "Review @file:note.txt:2-2 and list @folder:sub";
        let out = expand_at_context(msg, &dir);
        assert!(out.contains(ATTACHED_CONTEXT_HEADER), "{out}");
        assert!(out.contains("### file:note.txt:2"), "{out}");
        assert!(out.contains("two"), "{out}");
        assert!(!out.contains("one\n"), "{out}");
        assert!(out.contains("### folder:sub"), "{out}");
        assert!(out.contains("file a.rs"), "{out}");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn plus_button_style_message_without_at_is_unchanged() {
        let dir = temp_dir("noop");
        let msg = "File notes.md is on this computer at /tmp/notes.md. Read it yourself.";
        assert_eq!(expand_at_context(msg, &dir), msg);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn refuses_sensitive_paths() {
        let dir = temp_dir("safe");
        let out = expand_at_context("leak @file:~/.ssh/id_ed25519", &dir);
        assert!(out.contains("forbidden path"), "{out}");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn expands_git_diff_in_a_repo() {
        if Command::new("git").arg("--version").output().is_err() {
            return;
        }
        let dir = temp_dir("git");
        let run = |args: &[&str]| {
            git_cmd::git()
                .current_dir(&dir)
                .args(args)
                .env("GIT_AUTHOR_NAME", "t")
                .env("GIT_AUTHOR_EMAIL", "t@example.invalid")
                .env("GIT_COMMITTER_NAME", "t")
                .env("GIT_COMMITTER_EMAIL", "t@example.invalid")
                .output()
                .unwrap()
        };
        assert!(run(&["init", "-q"]).status.success());
        fs::write(dir.join("a.txt"), "v1\n").unwrap();
        assert!(run(&["add", "a.txt"]).status.success());
        assert!(run(&["commit", "-q", "-m", "one"]).status.success());
        fs::write(dir.join("a.txt"), "v2\n").unwrap();
        let out = expand_at_context("What changed? @diff", &dir);
        assert!(out.contains(ATTACHED_CONTEXT_HEADER), "{out}");
        assert!(out.contains("### diff"), "{out}");
        assert!(
            out.contains("+v2") || out.contains("v2"),
            "expected diff body: {out}"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn public_http_url_gate_blocks_loopback_urls_without_dialing() {
        let dir = temp_dir("url");
        let out = expand_at_context("grab @url:http://127.0.0.1/secret", &dir);
        assert!(out.contains("not a public"), "{out}");
        let _ = fs::remove_dir_all(&dir);
    }
}
