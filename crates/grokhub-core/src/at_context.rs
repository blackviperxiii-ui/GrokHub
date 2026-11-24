//! `@` context references: expand file, folder, git diff, and URL into the turn.
//!
//! Plus-button attach stays in [`crate::attach`]. This module only expands
//! typed `@file:` / `@folder:` / `@diff` / `@staged` / `@git:N` / `@url:` tokens
//! and appends their bodies under an attached-context section. A token a
//! model wrote stays text ([`attach_at_context`]), `@file:` reads only a
//! regular file, and the git tokens run no program the repository's config
//! names.

use std::collections::BTreeSet;
use std::fs;
use std::io::{self, BufRead, Read};
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

/// How far `@file:path:N-M` reads into a file to find line N.
const LINE_SCAN_CAP: u64 = 64 * 1024 * 1024;

const TRAIL_PUNCT: &[char] = &[',', '.', ';', '!', '?'];

/// What a cut `@file:` body ends with. The cap counts it, so a cut file
/// still fits [`TEXT_FILE_CAP`] whole and [`bound_scan`] leaves it alone.
const CUT_MARK: &str = "\n…";

/// Why a file that isn't UTF-8 text is refused.
const NOT_UTF8: &str = "stream did not contain valid UTF-8";

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
/// Leaves the message unchanged when there are no `@` tokens. For a message
/// only part of which the person typed, use [`attach_at_context`].
pub fn expand_at_context(text: &str, cwd: &Path) -> String {
    attach_at_context(text, text, cwd)
}

/// `text` with the bodies of the references in `typed`, the part of it the
/// person typed, under [`ATTACHED_CONTEXT_HEADER`]. A reference anywhere else
/// in `text` (a reply a model wrote, quoted in) stays as written and nothing
/// it names is read. Leaves `text` unchanged when `typed` has no `@` tokens.
pub fn attach_at_context(text: &str, typed: &str, cwd: &Path) -> String {
    let refs = find_at_refs(typed);
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
        AtKind::Diff => git_diff(cwd, &["diff"]),
        AtKind::Staged => git_diff(cwd, &["diff", "--staged"]),
        AtKind::Git { count } => git_diff(
            cwd,
            &["log", &format!("-n{count}"), "-p", "--no-show-signature"],
        ),
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
    let file = match open_regular(&path) {
        Ok(file) => file,
        Err(why) => return why,
    };
    let read = match lines {
        Some((start, end)) => read_lines(file, start, end, LINE_SCAN_CAP),
        None => read_capped(file),
    };
    read.unwrap_or_else(|err| format!("can't read {}: {err}", path.display()))
}

/// `path` opened for reading when it is a regular file. The open doesn't wait
/// for a FIFO's writer, and the check reads the opened handle, so a pipe,
/// socket or device swapped in after a look at the path is still refused.
fn open_regular(path: &Path) -> Result<fs::File, String> {
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NONBLOCK);
    }
    let file = options
        .open(path)
        .map_err(|err| format!("can't open {}: {err}", path.display()))?;
    match file.metadata() {
        Ok(meta) if meta.is_file() => Ok(file),
        Ok(_) => Err(format!("can't read {}: not a regular file", path.display())),
        Err(err) => Err(format!("can't read {}: {err}", path.display())),
    }
}

/// The whole file, or as much of it as fits [`TEXT_FILE_CAP`].
fn read_capped(file: fs::File) -> io::Result<String> {
    let mut bytes = Vec::new();
    file.take(TEXT_FILE_CAP as u64 + 1)
        .read_to_end(&mut bytes)?;
    let (mut text, cut) = capped_text(bytes)?;
    if cut {
        text.push_str(CUT_MARK);
    }
    Ok(text)
}

/// `bytes` as text, and whether they ran past [`TEXT_FILE_CAP`]. A cut keeps
/// the whole characters that fit beside [`CUT_MARK`]: a character the cut
/// splits is the cap's doing, not bad text.
fn capped_text(mut bytes: Vec<u8>) -> io::Result<(String, bool)> {
    let cut = bytes.len() > TEXT_FILE_CAP;
    if cut {
        bytes.truncate(TEXT_FILE_CAP - CUT_MARK.len());
        if let Err(err) = std::str::from_utf8(&bytes) {
            if err.error_len().is_none() {
                bytes.truncate(err.valid_up_to());
            }
        }
    }
    let text = String::from_utf8(bytes)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, NOT_UTF8))?;
    Ok((text, cut))
}

/// Lines `start..=end` (from 1) of `file`. Lines before `start` are counted,
/// not kept, so a range deep in a big file returns those lines, up to
/// `scan_cap` bytes into the file. A range past the end says so.
fn read_lines(file: impl Read, start: usize, end: usize, scan_cap: u64) -> io::Result<String> {
    let mut reader = io::BufReader::new(file.take(scan_cap + 1));
    // The line the next byte is on, and whether bytes follow its last newline.
    let (mut line, mut partial) = (1usize, false);
    let mut picked = Vec::new();
    let mut scanned = 0u64;
    while line <= end && picked.len() <= TEXT_FILE_CAP {
        let chunk = reader.fill_buf()?;
        if chunk.is_empty() {
            break;
        }
        let len = chunk.len();
        let mut rest = chunk;
        while !rest.is_empty() && line <= end && picked.len() <= TEXT_FILE_CAP {
            let newline = rest.iter().position(|&b| b == b'\n');
            let piece = &rest[..newline.map_or(rest.len(), |at| at + 1)];
            if line >= start {
                let room = TEXT_FILE_CAP + 1 - picked.len();
                picked.extend_from_slice(&piece[..piece.len().min(room)]);
            }
            partial = newline.is_none();
            if !partial {
                line += 1;
            }
            rest = &rest[piece.len()..];
        }
        scanned += len as u64;
        reader.consume(len);
    }
    let lines_seen = line - 1 + usize::from(partial);
    let lines = if start == end {
        format!("line {start} is")
    } else {
        format!("lines {start}-{end} are")
    };
    if lines_seen < start {
        if scanned > scan_cap {
            return Ok(format!(
                "({lines} past the first {} MB of the file, which is as far as a line range reads)",
                scan_cap / (1024 * 1024)
            ));
        }
        let has = if lines_seen == 1 { "line" } else { "lines" };
        return Ok(format!(
            "({lines} past the end: the file has {lines_seen} {has})"
        ));
    }
    let (text, cut) = capped_text(picked)?;
    let mut out = text.lines().collect::<Vec<_>>().join("\n");
    // Cut by the cap, or by the scan ending inside the range.
    if cut || (scanned > scan_cap && line <= end) {
        out.push_str(CUT_MARK);
    }
    Ok(out)
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

/// What `@diff`, `@staged` and `@git:N` add to their subcommand, as the
/// app's other diffs do: no external diff or text conversion program the
/// config names, no colour, and a submodule shown only by its commit, so its
/// work tree, and with it its own config, is never read.
const DIFF_OPTS: [&str; 5] = [
    "--no-ext-diff",
    "--no-textconv",
    "--no-color",
    "--submodule=short",
    "--ignore-submodules=dirty",
];

/// A git diff or log of `cwd` that runs no program the repository's config
/// names. [`git_cmd::git`] keeps hooks and fsmonitor off, [`DIFF_OPTS`] the
/// diff programs, and [`filters_off`] the clean filter git runs on a work
/// tree file it reads. A partial clone doesn't fetch what it lacks.
fn git_diff(cwd: &Path, args: &[&str]) -> String {
    let filters = match filters_off(cwd) {
        Ok(filters) => filters,
        Err(why) => return why.into(),
    };
    let out = match git_cmd::git()
        .current_dir(cwd)
        .env("GIT_NO_LAZY_FETCH", "1")
        .arg("--no-pager")
        .args(&filters)
        .args(args)
        .args(DIFF_OPTS)
        .arg("--")
        .output()
    {
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

/// `-c` pairs that switch off every filter driver the config of `cwd` names.
/// `Err` when a driver's name holds `=`, which `-c` can't spell: git would
/// read `-c filter.a=b.clean=` as setting `filter.a`.
fn filters_off(cwd: &Path) -> Result<Vec<String>, &'static str> {
    // Reading config runs nothing; exit 1 only means no filter is set.
    let listed = git_cmd::git()
        .current_dir(cwd)
        .args(["config", "-z", "--name-only", "--get-regexp", r"^filter\."])
        .output()
        .map(|out| out.stdout)
        .unwrap_or_default();
    let names: BTreeSet<String> = listed
        .split(|b| *b == 0)
        .filter_map(|key| {
            let key = String::from_utf8_lossy(key);
            let (name, _var) = key.strip_prefix("filter.")?.rsplit_once('.')?;
            Some(name.to_string())
        })
        .collect();
    if names.iter().any(|name| name.contains('=')) {
        return Err("the repository's config names a filter that can't be switched off");
    }
    Ok(names
        .iter()
        .flat_map(|name| {
            ["clean=", "smudge=", "process=", "required=false"]
                .map(|setting| ["-c".to_string(), format!("filter.{name}.{setting}")])
        })
        .flatten()
        .collect())
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

    /// The body under `label` in an expanded message.
    fn body_of(out: &str, label: &str) -> String {
        out.split(&format!("### {label}\n"))
            .nth(1)
            .unwrap_or("")
            .to_string()
    }

    fn git_in(dir: &Path, args: &[&str]) -> std::process::Output {
        git_cmd::git()
            .current_dir(dir)
            .args(args)
            .env("GIT_AUTHOR_NAME", "t")
            .env("GIT_AUTHOR_EMAIL", "t@example.invalid")
            .env("GIT_COMMITTER_NAME", "t")
            .env("GIT_COMMITTER_EMAIL", "t@example.invalid")
            .output()
            .unwrap()
    }

    /// A script that touches `marker` and passes its input file through.
    #[cfg(unix)]
    fn marker_script(path: &Path, marker: &Path) {
        use std::os::unix::fs::PermissionsExt;
        fs::write(
            path,
            format!(
                "#!/bin/sh\ntouch '{}'\n[ -f \"$1\" ] && cat \"$1\"\nexit 0\n",
                marker.display()
            ),
        )
        .unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    }

    /// What a sandboxed command in the workspace could plant in `.git/config`:
    /// an external diff, a text conversion, a clean filter and a signature
    /// check. `@diff`, `@staged` and `@git:N` run none of them.
    #[cfg(unix)]
    #[test]
    fn diff_staged_and_git_run_no_program_the_repo_config_names() {
        let dir = temp_dir("repo-programs");
        let markers = dir.join("markers");
        fs::create_dir_all(&markers).unwrap();
        let repo = dir.join("repo");
        fs::create_dir_all(&repo).unwrap();
        assert!(git_in(&repo, &["init", "-q"]).status.success());
        fs::write(repo.join("a.txt"), "v1\n").unwrap();
        fs::write(repo.join(".gitattributes"), "*.txt diff=x filter=f\n").unwrap();
        assert!(git_in(&repo, &["add", "a.txt", ".gitattributes"])
            .status
            .success());
        assert!(git_in(&repo, &["commit", "-q", "-m", "one"])
            .status
            .success());
        // A signed commit on top, so `log.showSignature` has a signature to check.
        let tree = String::from_utf8(git_in(&repo, &["write-tree"]).stdout).unwrap();
        let parent = String::from_utf8(git_in(&repo, &["rev-parse", "HEAD"]).stdout).unwrap();
        let raw = dir.join("signed.txt");
        fs::write(
            &raw,
            format!(
                "tree {}\nparent {}\nauthor t <t@example.invalid> 1700000000 +0000\n\
                 committer t <t@example.invalid> 1700000000 +0000\n\
                 gpgsig -----BEGIN PGP SIGNATURE-----\n \n abc\n -----END PGP SIGNATURE-----\n\nsigned\n",
                tree.trim(),
                parent.trim()
            ),
        )
        .unwrap();
        let signed = git_in(
            &repo,
            &["hash-object", "-t", "commit", "-w", raw.to_str().unwrap()],
        );
        let signed = String::from_utf8(signed.stdout).unwrap();
        assert!(git_in(&repo, &["update-ref", "HEAD", signed.trim()])
            .status
            .success());
        let names = ["external", "textconv", "clean", "smudge", "gpg"];
        for name in names {
            marker_script(&dir.join(format!("{name}.sh")), &markers.join(name));
        }
        let script = |name: &str| dir.join(format!("{name}.sh")).display().to_string();
        for (key, value) in [
            ("diff.external", script("external")),
            ("diff.x.textconv", script("textconv")),
            ("filter.f.clean", script("clean")),
            ("filter.f.smudge", script("smudge")),
            ("filter.f.required", "true".into()),
            ("log.showSignature", "true".into()),
            ("gpg.program", script("gpg")),
        ] {
            assert!(git_in(&repo, &["config", key, &value]).status.success());
        }
        // Racy-clean stat data makes git read the file through its filter.
        fs::write(repo.join("a.txt"), "v2\n").unwrap();
        let ran = || {
            let mut ran: Vec<String> = fs::read_dir(&markers)
                .unwrap()
                .flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect();
            ran.sort();
            for name in &ran {
                let _ = fs::remove_file(markers.join(name));
            }
            ran
        };
        let diff = expand_at_context("What changed? @diff", &repo);
        let after_diff = ran();
        let staged = expand_at_context("staged? @staged", &repo);
        let after_staged = ran();
        let log = expand_at_context("history @git:1", &repo);
        let after_git = ran();
        let _ = fs::remove_dir_all(&dir);
        assert_eq!(after_diff, Vec::<String>::new(), "@diff");
        assert_eq!(after_staged, Vec::<String>::new(), "@staged");
        assert_eq!(after_git, Vec::<String>::new(), "@git:1");
        assert!(body_of(&diff, "diff").contains("\n-v1\n+v2\n"), "{diff}");
        assert_eq!(body_of(&staged, "staged"), "(no changes)");
        assert!(body_of(&log, "git:1").contains("\n    signed\n"), "{log}");
    }

    /// `-c filter.a=b.clean=` would set `filter.a`, not switch `a=b` off.
    #[cfg(unix)]
    #[test]
    fn a_filter_name_git_cannot_switch_off_fails_the_diff() {
        let dir = temp_dir("filter-eq");
        let marker = dir.join("clean-ran");
        marker_script(&dir.join("clean.sh"), &marker);
        let repo = dir.join("repo");
        fs::create_dir_all(&repo).unwrap();
        assert!(git_in(&repo, &["init", "-q"]).status.success());
        fs::write(repo.join("a.txt"), "v1\n").unwrap();
        fs::write(repo.join(".gitattributes"), "*.txt filter=a=b\n").unwrap();
        assert!(git_in(&repo, &["add", "-A"]).status.success());
        assert!(git_in(&repo, &["commit", "-q", "-m", "one"])
            .status
            .success());
        let clean = dir.join("clean.sh").display().to_string();
        assert!(git_in(&repo, &["config", "filter.a=b.clean", &clean])
            .status
            .success());
        fs::write(repo.join("a.txt"), "v2\n").unwrap();
        let out = expand_at_context("What changed? @diff", &repo);
        let ran = marker.exists();
        let _ = fs::remove_dir_all(&dir);
        assert!(!ran, "the filter ran: {out}");
        assert_eq!(
            body_of(&out, "diff"),
            "the repository's config names a filter that can't be switched off"
        );
    }

    /// A submodule with changes of its own keeps its config's programs too.
    #[cfg(unix)]
    #[test]
    fn at_diff_runs_no_program_a_dirty_submodule_s_config_names() {
        let dir = temp_dir("submodule-programs");
        let marker = dir.join("clean-ran");
        marker_script(&dir.join("clean.sh"), &marker);
        let sub_src = dir.join("sub-src");
        fs::create_dir_all(&sub_src).unwrap();
        assert!(git_in(&sub_src, &["init", "-q"]).status.success());
        fs::write(sub_src.join("s.txt"), "s1\n").unwrap();
        fs::write(sub_src.join(".gitattributes"), "* filter=g\n").unwrap();
        assert!(git_in(&sub_src, &["add", "-A"]).status.success());
        assert!(git_in(&sub_src, &["commit", "-q", "-m", "s"])
            .status
            .success());
        let repo = dir.join("repo");
        fs::create_dir_all(&repo).unwrap();
        assert!(git_in(&repo, &["init", "-q"]).status.success());
        let added = git_in(
            &repo,
            &[
                "-c",
                "protocol.file.allow=always",
                "submodule",
                "add",
                "-q",
                sub_src.to_str().unwrap(),
                "sub",
            ],
        );
        assert!(
            added.status.success(),
            "{}",
            String::from_utf8_lossy(&added.stderr)
        );
        assert!(git_in(&repo, &["commit", "-q", "-m", "sub"])
            .status
            .success());
        let sub_config = repo.join(".git/modules/sub/config");
        let mut config = fs::read_to_string(&sub_config).unwrap();
        config.push_str(&format!(
            "[filter \"g\"]\n\tclean = {}\n\trequired = true\n",
            dir.join("clean.sh").display()
        ));
        fs::write(&sub_config, config).unwrap();
        fs::write(repo.join("sub/s.txt"), "s2\n").unwrap();
        let out = expand_at_context("What changed? @diff", &repo);
        let ran = marker.exists();
        let _ = fs::remove_dir_all(&dir);
        assert!(!ran, "the submodule's clean filter ran: {out}");
        assert_eq!(body_of(&out, "diff"), "(no changes)");
    }

    #[test]
    fn big_utf8_file_is_capped_at_a_char_boundary() {
        let dir = temp_dir("utf8cap");
        // 90,000 bytes of 3-byte chars; a byte cap lands mid-character.
        fs::write(dir.join("notes.txt"), "€".repeat(30_000)).unwrap();
        let out = expand_at_context("summarize @file:notes.txt", &dir);
        let _ = fs::remove_dir_all(&dir);
        let body = body_of(&out, "file:notes.txt");
        // 65,532 bytes of text and the 4-byte cut mark fill the cap exactly.
        assert_eq!(body, format!("{}\n…", "€".repeat(21_844)));
        assert_eq!(body.len(), TEXT_FILE_CAP);
    }

    #[test]
    fn line_range_past_the_cap_returns_the_requested_lines() {
        let dir = temp_dir("range");
        let text: String = (1..=20_000).map(|n| format!("line {n}\n")).collect();
        assert!(text.len() > TEXT_FILE_CAP);
        fs::write(dir.join("big.log"), &text).unwrap();
        let out = expand_at_context("why @file:big.log:15000-15001", &dir);
        let one = expand_at_context("and @file:big.log:20000", &dir);
        let past = expand_at_context("and @file:big.log:20001-20005", &dir);
        let past_one = expand_at_context("and @file:big.log:20001", &dir);
        let _ = fs::remove_dir_all(&dir);
        assert_eq!(
            body_of(&out, "file:big.log:15000-15001"),
            "line 15000\nline 15001"
        );
        assert_eq!(body_of(&one, "file:big.log:20000"), "line 20000");
        assert_eq!(
            body_of(&past, "file:big.log:20001-20005"),
            "(lines 20001-20005 are past the end: the file has 20000 lines)"
        );
        assert_eq!(
            body_of(&past_one, "file:big.log:20001"),
            "(line 20001 is past the end: the file has 20000 lines)"
        );
    }

    /// Lines of 8 bytes, so a 2 MB scan ends 1 byte into line 262,145.
    #[test]
    fn a_line_range_the_scan_cannot_reach_says_so() {
        let text: String = (1..=300_000).map(|n| format!("{n:07}\n")).collect();
        let scan = |start, end| read_lines(text.as_bytes(), start, end, 2 * 1024 * 1024).unwrap();
        assert_eq!(scan(262_143, 262_144), "0262143\n0262144");
        assert_eq!(scan(262_143, 262_146), "0262143\n0262144\n0\n…");
        assert_eq!(
            scan(262_146, 262_147),
            "(lines 262146-262147 are past the first 2 MB of the file, which is as far as a line range reads)"
        );
    }

    /// A side prompt quotes a reply a model wrote: only the words the person
    /// typed have their references read.
    #[test]
    fn only_the_typed_words_have_their_references_read() {
        let dir = temp_dir("typed");
        fs::write(dir.join("mine.txt"), "MINE\n").unwrap();
        fs::write(dir.join("other.txt"), "OTHER\n").unwrap();
        let text = "Its reply so far: see @file:other.txt\nSide question: what is @file:mine.txt";
        let out = attach_at_context(text, "what is @file:mine.txt", &dir);
        let untouched = attach_at_context(text, "what is in it?", &dir);
        let _ = fs::remove_dir_all(&dir);
        assert_eq!(
            out,
            format!("{text}\n\n{ATTACHED_CONTEXT_HEADER}\n### file:mine.txt\nMINE\n")
        );
        assert_eq!(untouched, text);
    }

    #[test]
    fn a_file_that_is_not_utf8_is_refused() {
        let dir = temp_dir("not-utf8");
        fs::write(dir.join("blob.bin"), [0x66, 0xff, 0xfe, 0x00]).unwrap();
        let shown = dir.join("blob.bin").canonicalize().unwrap();
        let out = expand_at_context("what is @file:blob.bin", &dir);
        let _ = fs::remove_dir_all(&dir);
        assert_eq!(
            body_of(&out, "file:blob.bin"),
            format!(
                "can't read {}: stream did not contain valid UTF-8",
                shown.display()
            )
        );
    }

    #[cfg(unix)]
    #[test]
    fn file_ref_to_a_fifo_is_refused_without_blocking() {
        let dir = temp_dir("fifo");
        let fifo = dir.join("pipe");
        assert!(Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .unwrap()
            .success());
        let shown = fifo.canonicalize().unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        let d2 = dir.clone();
        std::thread::spawn(move || {
            let _ = tx.send(expand_at_context("look @file:pipe", &d2));
        });
        let got = rx.recv_timeout(Duration::from_secs(5));
        if got.is_err() {
            // Unblock the reader so the process can exit.
            let _ = fs::OpenOptions::new().write(true).open(&fifo);
            let _ = rx.recv_timeout(Duration::from_secs(5));
        }
        let _ = fs::remove_dir_all(&dir);
        let out = got.expect("expand_at_context still blocked on @file:pipe after 5s");
        assert_eq!(
            body_of(&out, "file:pipe"),
            format!("can't read {}: not a regular file", shown.display())
        );
    }
}
