//! `@` context references: expand file, folder, git diff, and URL into the turn.
//!
//! Plus-button attach stays in [`crate::attach`]. This module only expands
//! typed `@file:` / `@folder:` / `@diff` / `@staged` / `@git:N` / `@url:` tokens
//! and appends their bodies under an attached-context section. A token a
//! model wrote stays text ([`Typed`]), `@file:` reads only a
//! regular file, and the git tokens run no program the repository's config
//! names.

use std::fs;
use std::io::{self, BufRead, Read};
use std::path::{Path, PathBuf};
use std::process::Command;
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

/// Words a person typed, the only text whose `@` references
/// [`attach_at_context`] reads. One comes only from the app's boundary: the
/// composer's or a side question's text as the window sends it (it
/// deserializes from that string), or the part of another kept with
/// [`Typed::keep`]. Text a model wrote or a card carried has no way in, so
/// its references stay as written.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Deserialize)]
#[serde(transparent)]
pub struct Typed(String);

impl Typed {
    /// No typed words: a message the app, a card or a model composed.
    pub fn nothing() -> Self {
        Self::default()
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The part of these words that `pick` returns. It can only narrow them:
    /// a result that is not a part of them keeps nothing.
    pub fn keep(&self, pick: impl FnOnce(&str) -> &str) -> Self {
        let kept = pick(&self.0);
        let start = kept.as_ptr().addr().wrapping_sub(self.0.as_ptr().addr());
        self.0
            .get(start..start.wrapping_add(kept.len()))
            .map_or_else(Self::nothing, |part| Self(part.to_string()))
    }
}

/// `text` with the bodies of the references in `typed`, the part of it the
/// person typed, under [`ATTACHED_CONTEXT_HEADER`]. A reference anywhere else
/// in `text` (a reply a model wrote, a card's report) stays as written and
/// nothing it names is read. Leaves `text` unchanged when `typed` has no `@`
/// tokens.
pub fn attach_at_context(text: &str, typed: &Typed, cwd: &Path) -> String {
    let refs = find_at_refs(typed.as_str());
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
        AtKind::Diff => work_tree_diff(cwd),
        AtKind::Staged => git_diff(in_dir(cwd), &["diff", "--staged"]),
        // `--pretty=medium` over a `format.pretty` whose `%G?` would check
        // signatures with the `gpg.program` the config names.
        AtKind::Git { count } => git_diff(
            in_dir(cwd),
            &[
                "log",
                &format!("-n{count}"),
                "-p",
                "--no-show-signature",
                "--pretty=medium",
            ],
        ),
        AtKind::Url { url } => fetch_url(url),
    }
}

/// [`git_cmd::git`] in `cwd`.
fn in_dir(cwd: &Path) -> Command {
    let mut cmd = git_cmd::git();
    cmd.current_dir(cwd);
    cmd
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
    let file =
        open_nonblocking(path).map_err(|err| format!("can't open {}: {err}", path.display()))?;
    regular(file).map_err(|err| format!("can't read {}: {err}", path.display()))
}

/// `path` opened for reading without waiting for a FIFO's writer.
fn open_nonblocking(path: &Path) -> io::Result<fs::File> {
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NONBLOCK);
    }
    options.open(path)
}

/// `file` when the opened handle is a regular file.
fn regular(file: fs::File) -> io::Result<fs::File> {
    if file.metadata()?.is_file() {
        Ok(file)
    } else {
        Err(io::Error::other("not a regular file"))
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

/// A git diff or log that runs no program the repository's config names.
/// [`git_cmd::git`] keeps hooks and fsmonitor off and [`DIFF_OPTS`] the diff
/// programs. A partial clone doesn't fetch what it lacks.
fn git_diff(mut cmd: Command, args: &[&str]) -> String {
    let out = match cmd
        .env("GIT_NO_LAZY_FETCH", "1")
        .arg("--no-pager")
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

/// `@diff`: the work tree against the index. It is the one git call here
/// that reads work tree files, and git reads them through the clean filter
/// a `.gitattributes` names, so it runs on an [`OwnGitDir`] that switches
/// every filter off. `@staged` and `@git:N` read only objects.
fn work_tree_diff(cwd: &Path) -> String {
    let own = match OwnGitDir::new(cwd) {
        Ok(own) => own,
        Err(why) => return why,
    };
    let mut cmd = git_cmd::git();
    cmd.current_dir(cwd)
        .env_remove("GIT_COMMON_DIR")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", own.dir.join(NO_GLOBAL_CONFIG))
        .env("GIT_DIR", &own.dir)
        .env("GIT_WORK_TREE", &own.top)
        .env("GIT_OBJECT_DIRECTORY", &own.objects)
        .env("GIT_INDEX_FILE", own.dir.join("index"));
    git_diff(cmd, &["diff"])
}

/// Where [`OwnGitDir`]s go, under the app's config folder.
const OWN_GIT_DIRS: &str = "at-diff";

/// The empty file an [`OwnGitDir`]'s diff reads as its global config: its
/// own config already holds the global settings, as the repository saw them.
const NO_GLOBAL_CONFIG: &str = "no-global-config";

/// Sections of the config a work tree diff never reads and that can hold a
/// credential (a token in a URL or a header), left out of an
/// [`OwnGitDir`]'s copy of it.
const UNREAD_SECTIONS: [&[u8]; 6] = [
    b"credential",
    b"http",
    b"url",
    b"remote",
    b"sendemail",
    b"imap",
];

/// How long an [`OwnGitDir`] a crash left behind stays before the next
/// `@diff` removes it. No diff takes so long.
const OWN_GIT_DIR_KEEP: Duration = Duration::from_secs(60 * 60);

/// The line that ends an [`OwnGitDir`]'s `info/attributes`. git reads that
/// file after every `.gitattributes`, and the last line that sets an
/// attribute wins, so no path has a filter driver, under any name the config
/// gives one, whenever it was added. A macro can't set `filter` back: git
/// fills an attribute once, from the last line that names it.
const NO_FILTERS: &[u8] = b"* -filter\n";

/// The most of the repository's own `info/attributes` an [`OwnGitDir`]
/// takes. git drops a whole attributes file of 100 MB or more, and with it
/// [`NO_FILTERS`], so a bigger one fails `@diff` instead.
const REPO_ATTRIBUTES_CAP: u64 = 1024 * 1024;

/// A git dir of the app's own for one `@diff` of the repository at `cwd`,
/// read with the repository's objects (and their alternates) and work tree.
/// Its config holds the repository's format and the settings git reads in
/// the repository ([`effective_config`]), and its diff reads no other config
/// file, so an include that matches the repository's folder or branch still
/// applies. Its index is a copy of the repository's, with the shared index a
/// split index names and the sparse-checkout patterns beside it. Its
/// `info/attributes` is the repository's followed by [`NO_FILTERS`]. It has
/// no `commondir`, since git reads `info/attributes` from the common dir. It
/// sits in the app's config folder, which a sandboxed command can't write,
/// and is removed when dropped. git writes a refreshed index to the copy,
/// never to the repository.
struct OwnGitDir {
    dir: PathBuf,
    top: PathBuf,
    objects: PathBuf,
}

impl OwnGitDir {
    fn new(cwd: &Path) -> Result<Self, String> {
        let paths = git_paths(
            git_cmd::git()
                .current_dir(cwd)
                .args(["rev-parse", "--show-toplevel"])
                .args(["--git-path", "objects", "--git-path", "index"])
                .args(["--git-path", "config", "--git-path", "info/attributes"])
                .args(["--git-path", "info/sparse-checkout"]),
            cwd,
        )?;
        let [top, objects, index, config, attributes, sparse] = &paths[..] else {
            return Err("git didn't say where the repository is".into());
        };
        let mut own_config = repository_format(cwd, config)?;
        own_config.extend_from_slice(&effective_config(cwd)?);
        let attributes = own_attributes(attributes)?;
        let root = crate::paths::config_dir().join(OWN_GIT_DIRS);
        sweep_old(&root);
        let failed = |err: io::Error| format!("can't set up @diff in {}: {err}", root.display());
        fs::create_dir_all(&root).map_err(failed)?;
        let dir = root.join(crate::uid("diff"));
        private_dir(&dir).map_err(failed)?;
        let own = Self {
            dir,
            top: top.clone(),
            objects: objects.clone(),
        };
        fs::create_dir(own.dir.join("refs")).map_err(failed)?;
        fs::create_dir(own.dir.join("info")).map_err(failed)?;
        fs::write(own.dir.join("HEAD"), "ref: refs/heads/main\n").map_err(failed)?;
        fs::write(own.dir.join("info").join("attributes"), attributes).map_err(failed)?;
        fs::write(own.dir.join("config"), own_config).map_err(failed)?;
        fs::write(own.dir.join(NO_GLOBAL_CONFIG), "").map_err(failed)?;
        let uncopied =
            |from: &Path, err: io::Error| format!("can't copy {} for @diff: {err}", from.display());
        let own_index = own.dir.join("index");
        for (from, to) in [
            (index, &own_index),
            (sparse, &own.dir.join("info").join("sparse-checkout")),
        ] {
            match copy_with_mtime(from, to) {
                Ok(()) => {}
                Err(err) if err.kind() == io::ErrorKind::NotFound => {}
                Err(err) => return Err(uncopied(from, err)),
            }
        }
        // A split index names its shared index, which git looks for in the
        // git dir: ask for the one the copy names, and copy it beside it.
        if own_index.is_file() {
            let shared = git_paths(
                git_cmd::git()
                    .current_dir(cwd)
                    .env("GIT_INDEX_FILE", &own_index)
                    .args(["rev-parse", "--shared-index-path"]),
                cwd,
            )?;
            if let Some(shared) = shared.first() {
                let name = shared.file_name().ok_or("git named no shared index file")?;
                copy_with_mtime(shared, &own.dir.join(name))
                    .map_err(|err| uncopied(shared, err))?;
            }
        }
        Ok(own)
    }
}

/// The paths a `git rev-parse` prints, one a line, from `cwd`.
fn git_paths(cmd: &mut Command, cwd: &Path) -> Result<Vec<PathBuf>, String> {
    let out = cmd.output().map_err(|err| format!("git failed: {err}"))?;
    if !out.status.success() {
        return Err(git_error(&out.stderr));
    }
    Ok(out
        .stdout
        .split(|b| *b == b'\n')
        .filter(|line| !line.is_empty())
        .map(|line| cwd.join(path_from_git(line)))
        .collect())
}

fn git_error(stderr: &[u8]) -> String {
    let err = String::from_utf8_lossy(stderr).trim().to_string();
    if err.is_empty() {
        "git produced no output".into()
    } else {
        err
    }
}

/// The repository's format as config lines: its version and `extensions.*`,
/// which git reads from the git dir's own config file, not from an include.
/// `extensions.refStorage` stays out: the own dir keeps its one ref in files.
fn repository_format(cwd: &Path, config: &Path) -> Result<Vec<u8>, String> {
    let out = git_cmd::git()
        .current_dir(cwd)
        .arg("config")
        .arg("--file")
        .arg(config)
        .args([
            "--null",
            "--get-regexp",
            r"^(core\.repositoryformatversion|extensions\..*)$",
        ])
        .output()
        .map_err(|err| format!("git failed: {err}"))?;
    // 1: none of those keys is set.
    match out.status.code() {
        Some(0) => {}
        Some(1) if out.stdout.is_empty() => return Ok(Vec::new()),
        _ => return Err(git_error(&out.stderr)),
    }
    let mut core = Vec::new();
    let mut extensions = Vec::new();
    for entry in out.stdout.split(|b| *b == 0).filter(|e| !e.is_empty()) {
        let (key, value) = match entry.iter().position(|b| *b == b'\n') {
            Some(at) => (&entry[..at], Some(&entry[at + 1..])),
            None => (entry, None),
        };
        let (section, name) = if key == b"core.repositoryformatversion" {
            (&mut core, &key[b"core.".len()..])
        } else if let Some(name) = key.strip_prefix(b"extensions.") {
            if name == b"refstorage" {
                continue;
            }
            (&mut extensions, name)
        } else {
            continue;
        };
        if !name.iter().all(|b| b.is_ascii_alphanumeric() || *b == b'-') {
            return Err("git named a repository format setting it can't have".into());
        }
        section.push(b'\t');
        section.extend_from_slice(name);
        if let Some(value) = value {
            section.extend_from_slice(b" = ");
            section.extend_from_slice(&config_quote(value));
        }
        section.push(b'\n');
    }
    let mut out = Vec::new();
    for (header, lines) in [
        (&b"[core]\n"[..], core),
        (&b"[extensions]\n"[..], extensions),
    ] {
        if !lines.is_empty() {
            out.extend_from_slice(header);
            out.extend_from_slice(&lines);
        }
    }
    Ok(out)
}

/// The settings git reads in the repository at `cwd`, as config lines in
/// the order git reads them: system, global, the repository's own and its
/// worktree's. git follows every include there, so a conditional one
/// (`includeIf "gitdir:..."`, `"onbranch:..."`) matches the repository, not
/// an [`OwnGitDir`]. Left out: the include lines (already followed), the
/// repository format ([`repository_format`] has it), [`UNREAD_SECTIONS`],
/// and the settings the app's environment adds, which still reach the diff
/// through that environment.
fn effective_config(cwd: &Path) -> Result<Vec<u8>, String> {
    let out = git_cmd::git()
        .current_dir(cwd)
        .env_remove("GIT_CONFIG_PARAMETERS")
        .env_remove("GIT_CONFIG_COUNT")
        .args(["config", "--list", "--includes", "--null"])
        .output()
        .map_err(|err| format!("git failed: {err}"))?;
    if !out.status.success() {
        return Err(git_error(&out.stderr));
    }
    let mut lines = Vec::new();
    let mut header: &[u8] = b"";
    for entry in out.stdout.split(|b| *b == 0).filter(|e| !e.is_empty()) {
        let (key, value) = match entry.iter().position(|b| *b == b'\n') {
            Some(at) => (&entry[..at], Some(&entry[at + 1..])),
            None => (entry, None),
        };
        // `section[.subsection].name`: the subsection may hold dots.
        let (Some(first), Some(last)) = (
            key.iter().position(|b| *b == b'.'),
            key.iter().rposition(|b| *b == b'.'),
        ) else {
            return Err("git named a setting it can't have".into());
        };
        let (section, name) = (&key[..first], &key[last + 1..]);
        let plain = |part: &[u8]| {
            !part.is_empty() && part.iter().all(|b| b.is_ascii_alphanumeric() || *b == b'-')
        };
        if !plain(section) || !plain(name) {
            return Err("git named a setting it can't have".into());
        }
        if matches!(section, b"include" | b"includeif" | b"extensions")
            || key == b"core.repositoryformatversion"
            || UNREAD_SECTIONS.contains(&section)
        {
            continue;
        }
        let at = &key[..last];
        if at != header {
            header = at;
            lines.push(b'[');
            lines.extend_from_slice(section);
            if first < last {
                lines.extend_from_slice(b" \"");
                for &b in &key[first + 1..last] {
                    if matches!(b, b'\\' | b'"') {
                        lines.push(b'\\');
                    }
                    lines.push(b);
                }
                lines.push(b'"');
            }
            lines.extend_from_slice(b"]\n");
        }
        lines.push(b'\t');
        lines.extend_from_slice(name);
        if let Some(value) = value {
            lines.extend_from_slice(b" = ");
            lines.extend_from_slice(&config_quote(value));
        }
        lines.push(b'\n');
    }
    Ok(lines)
}

/// `value` as a quoted config value: git reads it back byte for byte.
fn config_quote(value: &[u8]) -> Vec<u8> {
    let mut out = vec![b'"'];
    for &b in value {
        match b {
            b'\\' => out.extend_from_slice(b"\\\\"),
            b'"' => out.extend_from_slice(b"\\\""),
            b'\n' => out.extend_from_slice(b"\\n"),
            b'\t' => out.extend_from_slice(b"\\t"),
            b'\x08' => out.extend_from_slice(b"\\b"),
            _ => out.push(b),
        }
    }
    out.push(b'"');
    out
}

/// The repository's `info/attributes` (none when it has none), then
/// [`NO_FILTERS`] on a line of its own. A file that isn't a regular file, or
/// is over [`REPO_ATTRIBUTES_CAP`], fails `@diff`: git would read it, and
/// leaving it out would show what it hides from diffs.
fn own_attributes(path: &Path) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    match fs::symlink_metadata(path) {
        Err(err) if err.kind() == io::ErrorKind::NotFound => {}
        Err(err) => return Err(format!("can't read {}: {err}", path.display())),
        Ok(_) => {
            open_regular(path)?
                .take(REPO_ATTRIBUTES_CAP + 1)
                .read_to_end(&mut bytes)
                .map_err(|err| format!("can't read {}: {err}", path.display()))?;
            if bytes.len() as u64 > REPO_ATTRIBUTES_CAP {
                return Err(format!(
                    "@diff can't use {}: it is over 1 MiB",
                    path.display()
                ));
            }
        }
    }
    if !bytes.is_empty() && !bytes.ends_with(b"\n") {
        bytes.push(b'\n');
    }
    bytes.extend_from_slice(NO_FILTERS);
    Ok(bytes)
}

/// `from` copied to a new `to` with its modified time: git reads an index
/// entry as unchanged when its stat data matches and the index file is newer
/// than the work tree file, so a copy dated now would hide a fresh edit.
/// Only a regular file is copied, so a pipe planted in its place can't hold
/// `@diff` up.
fn copy_with_mtime(from: &Path, to: &Path) -> io::Result<()> {
    let mut src = regular(open_nonblocking(from)?)?;
    let modified = src.metadata()?.modified()?;
    let mut dst = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(to)?;
    io::copy(&mut src, &mut dst)?;
    dst.set_modified(modified)
}

impl Drop for OwnGitDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

/// A path line `git rev-parse` printed.
#[cfg(unix)]
fn path_from_git(line: &[u8]) -> PathBuf {
    use std::os::unix::ffi::OsStrExt;
    PathBuf::from(std::ffi::OsStr::from_bytes(line))
}

#[cfg(not(unix))]
fn path_from_git(line: &[u8]) -> PathBuf {
    PathBuf::from(String::from_utf8_lossy(line).trim_end_matches('\r'))
}

/// A new folder only this user can read. An existing name is an error.
#[cfg(unix)]
fn private_dir(dir: &Path) -> io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    fs::DirBuilder::new().mode(0o700).create(dir)
}

#[cfg(not(unix))]
fn private_dir(dir: &Path) -> io::Result<()> {
    fs::create_dir(dir)
}

/// Removes the folders under `root` made more than [`OWN_GIT_DIR_KEEP`] ago.
fn sweep_old(root: &Path) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let old = entry
            .metadata()
            .and_then(|meta| meta.modified())
            .ok()
            .and_then(|made| made.elapsed().ok())
            .is_some_and(|age| age >= OWN_GIT_DIR_KEEP);
        if old {
            let _ = fs::remove_dir_all(entry.path());
        }
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

    /// `text` expanded as if the person typed all of it.
    fn expand(text: &str, cwd: &Path) -> String {
        attach_at_context(text, &Typed(text.into()), cwd)
    }

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
        let out = expand(msg, &dir);
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
        assert_eq!(expand(msg, &dir), msg);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn refuses_sensitive_paths() {
        let dir = temp_dir("safe");
        let out = expand("leak @file:~/.ssh/id_ed25519", &dir);
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
        let out = expand("What changed? @diff", &dir);
        assert!(out.contains(ATTACHED_CONTEXT_HEADER), "{out}");
        assert!(out.contains("### diff"), "{out}");
        assert!(
            out.contains("+v2") || out.contains("v2"),
            "expected diff body: {out}"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    /// `@diff` runs on a git dir of its own, which still finds the index,
    /// objects and config of a linked worktree, from a subfolder, in a
    /// SHA-256 repository, and says plainly when there is no repository.
    #[test]
    fn at_diff_finds_a_linked_worktree_a_subfolder_and_a_sha256_repo() {
        if Command::new("git").arg("--version").output().is_err() {
            return;
        }
        let dir = temp_dir("own-git-dir");
        let repo = dir.join("repo");
        fs::create_dir_all(repo.join("sub")).unwrap();
        assert!(git_in(&repo, &["init", "-q"]).status.success());
        fs::write(repo.join("a.txt"), "a1\n").unwrap();
        fs::write(repo.join("sub/b.txt"), "b1\n").unwrap();
        assert!(git_in(&repo, &["add", "-A"]).status.success());
        assert!(git_in(&repo, &["commit", "-q", "-m", "one"])
            .status
            .success());
        let added = git_in(&repo, &["worktree", "add", "-q", "--detach", "../wt"]);
        assert!(
            added.status.success(),
            "{}",
            String::from_utf8_lossy(&added.stderr)
        );
        fs::write(repo.join("a.txt"), "a2\n").unwrap();
        fs::write(dir.join("wt/sub/b.txt"), "b2\n").unwrap();
        let main = body_of(&expand("@diff", &repo), "diff");
        let main_sub = body_of(&expand("@diff", &repo.join("sub")), "diff");
        let linked = body_of(&expand("@diff", &dir.join("wt/sub")), "diff");
        let sha256 = dir.join("sha256");
        fs::create_dir_all(&sha256).unwrap();
        assert!(git_in(&sha256, &["init", "-q", "--object-format=sha256"])
            .status
            .success());
        fs::write(sha256.join("c.txt"), "c1\n").unwrap();
        assert!(git_in(&sha256, &["add", "c.txt"]).status.success());
        assert!(git_in(&sha256, &["commit", "-q", "-m", "one"])
            .status
            .success());
        fs::write(sha256.join("c.txt"), "c2\n").unwrap();
        let sha256_body = body_of(&expand("@diff", &sha256), "diff");
        let plain = dir.join("plain");
        fs::create_dir_all(&plain).unwrap();
        let none = body_of(&expand("@diff", &plain), "diff");
        let _ = fs::remove_dir_all(&dir);
        assert!(
            main.contains("+++ b/a.txt\n@@ -1 +1 @@\n-a1\n+a2\n"),
            "{main}"
        );
        assert!(!main.contains("b.txt"), "{main}");
        assert_eq!(main_sub, main);
        assert!(
            linked.contains("+++ b/sub/b.txt\n@@ -1 +1 @@\n-b1\n+b2\n"),
            "{linked}"
        );
        assert!(!linked.contains("a.txt"), "{linked}");
        assert!(
            sha256_body.contains("+++ b/c.txt\n@@ -1 +1 @@\n-c1\n+c2\n"),
            "{sha256_body}"
        );
        assert!(none.contains("not a git repository"), "{none}");
    }

    /// An edit that keeps the size and the modified time matches its index
    /// entry's stat data. git still reads it, because the index file is no
    /// newer than the entry ("racily clean"), and the copy `@diff` reads
    /// keeps the index's modified time so git still does.
    #[cfg(unix)]
    #[test]
    fn at_diff_sees_an_edit_the_stat_data_hides() {
        let dir = temp_dir("racy");
        let repo = dir.join("repo");
        fs::create_dir_all(&repo).unwrap();
        assert!(git_in(&repo, &["init", "-q"]).status.success());
        assert!(git_in(&repo, &["config", "core.trustctime", "false"])
            .status
            .success());
        let then = std::time::SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        let stamp = |path: &Path| {
            fs::OpenOptions::new()
                .write(true)
                .open(path)
                .unwrap()
                .set_modified(then)
                .unwrap();
        };
        let file = repo.join("a.txt");
        fs::write(&file, "v1\n").unwrap();
        stamp(&file);
        assert!(git_in(&repo, &["add", "a.txt"]).status.success());
        fs::write(&file, "v2\n").unwrap();
        stamp(&file);
        stamp(&repo.join(".git/index"));
        let out = expand("What changed? @diff", &repo);
        let _ = fs::remove_dir_all(&dir);
        assert!(body_of(&out, "diff").contains("\n-v1\n+v2\n"), "{out}");
    }

    /// `git args` in `repo`, which must work.
    fn plain_git(repo: &Path, args: &[&str]) {
        let out = git_in(repo, args);
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    /// A repository at `dir/name` with `files` committed, then each
    /// rewritten with `2` in place of `1`.
    fn edited_repo(dir: &Path, name: &str, files: &[&str]) -> PathBuf {
        let repo = dir.join(name);
        fs::create_dir_all(&repo).unwrap();
        plain_git(&repo, &["init", "-q"]);
        for file in files {
            let path = repo.join(file);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, format!("{}1\n", file.replace('/', "-"))).unwrap();
        }
        plain_git(&repo, &["add", "-A"]);
        plain_git(&repo, &["commit", "-q", "-m", "one"]);
        for file in files {
            fs::write(repo.join(file), format!("{}2\n", file.replace('/', "-"))).unwrap();
        }
        repo
    }

    /// A split index keeps its entries in `.git/sharedindex.<hash>`, and
    /// `.git/info/attributes` holds the person's own attributes. `@diff`
    /// reads both: a path marked `-diff`, by a macro or by `binary` shows as
    /// binary, and the repository's index files stay as they were.
    #[test]
    fn at_diff_reads_a_split_index_and_the_repo_info_attributes() {
        if Command::new("git").arg("--version").output().is_err() {
            return;
        }
        let dir = temp_dir("split-attributes");
        let repo = edited_repo(&dir, "repo", &["a.txt", "d1/x.txt", "b.dat", "open.txt"]);
        plain_git(&repo, &["update-index", "--split-index"]);
        fs::write(
            repo.join(".git/info/attributes"),
            "a.txt -diff\n[attr]secret -diff -text\nd1/x.txt secret\nb.dat binary",
        )
        .unwrap();
        let shared: Vec<PathBuf> = fs::read_dir(repo.join(".git"))
            .unwrap()
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| {
                path.file_name()
                    .is_some_and(|name| name.to_string_lossy().starts_with("sharedindex."))
            })
            .collect();
        let read_all = || -> Vec<Vec<u8>> {
            [repo.join(".git/index")]
                .iter()
                .chain(&shared)
                .map(|path| fs::read(path).unwrap())
                .collect()
        };
        let before = read_all();
        let body = body_of(&expand("@diff", &repo), "diff");
        let after = read_all();
        let _ = fs::remove_dir_all(&dir);
        assert_eq!(shared.len(), 1, "{shared:?}");
        for binary in ["a.txt", "d1/x.txt", "b.dat"] {
            assert!(
                body.contains(&format!("Binary files a/{binary} and b/{binary} differ\n")),
                "{binary}: {body}"
            );
        }
        assert!(
            body.contains("+++ b/open.txt\n@@ -1 +1 @@\n-open.txt1\n+open.txt2\n"),
            "{body}"
        );
        for hidden in ["+a.txt2", "+d1-x.txt2", "+b.dat2"] {
            assert!(!body.contains(hidden), "{hidden}: {body}");
        }
        assert!(after == before, "@diff wrote the repository's index files");
    }

    /// An `info/attributes` too big for git to read with the line that
    /// switches filters off after it fails `@diff`, and says why.
    #[test]
    fn an_oversized_info_attributes_fails_at_diff() {
        if Command::new("git").arg("--version").output().is_err() {
            return;
        }
        let dir = temp_dir("big-attributes");
        let repo = edited_repo(&dir, "repo", &["a.txt"]);
        let attributes = repo.join(".git/info/attributes");
        fs::write(&attributes, "# a note\n".repeat(120_000)).unwrap();
        let body = body_of(&expand("@diff", &repo), "diff");
        let _ = fs::remove_dir_all(&dir);
        assert_eq!(
            body,
            format!("@diff can't use {}: it is over 1 MiB", attributes.display())
        );
    }

    /// `@diff` in a sparse checkout with a sparse index, in a clone that
    /// borrows its objects (`clone --shared`), with a relative
    /// `include.path` in the repository's config, and with a submodule
    /// checked out at another commit.
    #[test]
    fn at_diff_reads_a_sparse_index_alternates_includes_and_a_submodule() {
        if Command::new("git").arg("--version").output().is_err() {
            return;
        }
        let dir = temp_dir("repo-shapes");
        let sparse = edited_repo(&dir, "sparse", &["in/i.txt", "out/o.txt", "top.txt"]);
        plain_git(&sparse, &["checkout", "-q", "--", "."]);
        plain_git(
            &sparse,
            &["sparse-checkout", "set", "--cone", "--sparse-index", "in"],
        );
        fs::write(sparse.join("in/i.txt"), "in-i.txt2\n").unwrap();
        let sparse_body = body_of(&expand("@diff", &sparse), "diff");

        let shared = dir.join("shared");
        let source = edited_repo(&dir, "source", &["s.txt"]);
        plain_git(
            &dir,
            &[
                "clone",
                "-q",
                "--shared",
                source.to_str().unwrap(),
                "shared",
            ],
        );
        fs::write(shared.join("s.txt"), "s.txt2\n").unwrap();
        let shared_body = body_of(&expand("@diff", &shared), "diff");
        let borrows = shared.join(".git/objects/info/alternates").is_file();

        let included = edited_repo(&dir, "included", &["c.txt"]);
        fs::write(
            included.join(".git/extra.inc"),
            "[diff]\n\tmnemonicPrefix = true\n",
        )
        .unwrap();
        plain_git(&included, &["config", "include.path", "extra.inc"]);
        let included_body = body_of(&expand("@diff", &included), "diff");

        let sub_src = edited_repo(&dir, "sub-src", &["s.txt"]);
        plain_git(&sub_src, &["commit", "-q", "-am", "two"]);
        let sup = dir.join("sup");
        fs::create_dir_all(&sup).unwrap();
        plain_git(&sup, &["init", "-q"]);
        plain_git(
            &sup,
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
        plain_git(&sup, &["commit", "-q", "-m", "sub"]);
        let ids = |repo: &Path| {
            let out = git_in(repo, &["rev-parse", "HEAD", "HEAD~1"]).stdout;
            String::from_utf8(out).unwrap()
        };
        let sub_ids = ids(&sup.join("sub"));
        let (now, before) = sub_ids.trim().split_once('\n').unwrap();
        plain_git(&sup.join("sub"), &["checkout", "-q", before]);
        let sub_body = body_of(&expand("@diff", &sup), "diff");
        let _ = fs::remove_dir_all(&dir);

        assert!(
            sparse_body.contains("+++ b/in/i.txt\n@@ -1 +1 @@\n-in-i.txt1\n+in-i.txt2\n"),
            "{sparse_body}"
        );
        assert!(!sparse_body.contains("out/"), "{sparse_body}");
        assert!(borrows, "the clone has no alternates");
        assert!(
            shared_body.contains("+++ b/s.txt\n@@ -1 +1 @@\n-s.txt1\n+s.txt2\n"),
            "{shared_body}"
        );
        assert!(
            included_body.contains("--- i/c.txt\n+++ w/c.txt\n@@ -1 +1 @@\n-c.txt1\n+c.txt2\n"),
            "{included_body}"
        );
        assert!(
            sub_body.contains(&format!(
                "-Subproject commit {now}\n+Subproject commit {before}\n"
            )),
            "{sub_body}"
        );
    }

    /// Config scoped to a folder of repositories (`includeIf "gitdir:"` in
    /// the global config) or to a branch (`includeIf "onbranch:"` in the
    /// repository's) applies to `@diff` as it does to `git diff`: here the
    /// folder's attributes file marks `*.env` `-diff`, and the branch turns
    /// on mnemonic prefixes.
    #[test]
    fn at_diff_keeps_config_an_include_scopes_to_the_repository() {
        if Command::new("git").arg("--version").output().is_err() {
            return;
        }
        let dir = temp_dir("include-if");
        let slashed = |path: &Path| path.display().to_string().replace('\\', "/");
        let repo = edited_repo(&dir, "repo", &["secret.env", "plain.txt"]);
        plain_git(&repo, &["checkout", "-q", "-b", "topic"]);
        let global = dir.join("global.gitconfig");
        // The folder by its own name, which a short or resolved spelling of
        // the temp folder above it keeps.
        let folder = dir.file_name().unwrap().to_string_lossy().into_owned();
        fs::write(
            &global,
            format!(
                "[includeIf \"gitdir/i:**/{folder}/\"]\n\tpath = {}\n",
                slashed(&dir.join("work.inc"))
            ),
        )
        .unwrap();
        fs::write(
            dir.join("work.inc"),
            format!(
                "[core]\n\tattributesFile = {}\n",
                slashed(&dir.join("work.attributes"))
            ),
        )
        .unwrap();
        fs::write(dir.join("work.attributes"), "*.env -diff\n").unwrap();
        let mut config = fs::read_to_string(repo.join(".git/config")).unwrap();
        config.push_str("[includeIf \"onbranch:topic\"]\n\tpath = branch.inc\n");
        fs::write(repo.join(".git/config"), config).unwrap();
        fs::write(
            repo.join(".git/branch.inc"),
            "[diff]\n\tmnemonicPrefix = true\n",
        )
        .unwrap();
        let before = std::env::var_os("GIT_CONFIG_GLOBAL");
        unsafe { std::env::set_var("GIT_CONFIG_GLOBAL", &global) };
        let plain = git_in(&repo, &["--no-pager", "diff", "--no-color"]);
        let body = body_of(&expand("@diff", &repo), "diff");
        match before {
            Some(value) => unsafe { std::env::set_var("GIT_CONFIG_GLOBAL", value) },
            None => unsafe { std::env::remove_var("GIT_CONFIG_GLOBAL") },
        }
        let _ = fs::remove_dir_all(&dir);
        let plain = String::from_utf8_lossy(&plain.stdout);
        for out in [plain.as_ref(), body.as_str()] {
            assert!(
                out.contains("Binary files i/secret.env and w/secret.env differ\n"),
                "{out}"
            );
            assert!(
                out.contains(
                    "--- i/plain.txt\n+++ w/plain.txt\n@@ -1 +1 @@\n-plain.txt1\n+plain.txt2\n"
                ),
                "{out}"
            );
            assert!(!out.contains("+secret.env2"), "{out}");
        }
    }

    /// The own dir's copy of the config keeps each setting under its
    /// section and subsection (dots and quotes too) with its value byte for
    /// byte, so git reads it back the same, and leaves out the sections that
    /// can hold a credential.
    #[test]
    fn the_own_config_copies_settings_but_not_credentials() {
        if Command::new("git").arg("--version").output().is_err() {
            return;
        }
        let dir = temp_dir("own-config");
        let repo = edited_repo(&dir, "repo", &["a.txt"]);
        let mut config = fs::read_to_string(repo.join(".git/config")).unwrap();
        config.push_str(
            "[diff \"my.\\\"Drv\"]\n\tbinary\n\txfuncname = \"^fn \\\\w\"\n\
             [http]\n\textraHeader = Authorization: Bearer sk-abcdefghijklmnopqrstuv\n\
             [remote \"origin\"]\n\turl = https://user:sk-abcdefghijklmnopqrstuv@example.com/x.git\n",
        );
        fs::write(repo.join(".git/config"), config).unwrap();
        let own = effective_config(&repo).unwrap();
        let copy = dir.join("copy.gitconfig");
        fs::write(&copy, &own).unwrap();
        let read_back = git_in(
            &dir,
            &[
                "config",
                "--file",
                "copy.gitconfig",
                "--get",
                "diff.my.\"Drv.xfuncname",
            ],
        );
        let _ = fs::remove_dir_all(&dir);
        let own = String::from_utf8_lossy(&own);
        assert!(
            own.contains("[diff \"my.\\\"Drv\"]\n\tbinary\n\txfuncname = \"^fn \\\\w\"\n"),
            "{own}"
        );
        assert_eq!(String::from_utf8_lossy(&read_back.stdout), "^fn \\w\n");
        assert!(!own.contains("sk-abcdefghijklmnopqrstuv"), "{own}");
        assert!(!own.contains("[include"), "{own}");
    }

    #[test]
    fn public_http_url_gate_blocks_loopback_urls_without_dialing() {
        let dir = temp_dir("url");
        let out = expand("grab @url:http://127.0.0.1/secret", &dir);
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
    /// check, asked for by `log.showSignature` or a `%G?` in `format.pretty`.
    /// `@diff`, `@staged` and `@git:N` run none of them.
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
            ("format.pretty", "%H %G?%n%s".into()),
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
        let diff = expand("What changed? @diff", &repo);
        let after_diff = ran();
        let staged = expand("staged? @staged", &repo);
        let after_staged = ran();
        let log = expand("history @git:1", &repo);
        let after_git = ran();
        let _ = fs::remove_dir_all(&dir);
        assert_eq!(after_diff, Vec::<String>::new(), "@diff");
        assert_eq!(after_staged, Vec::<String>::new(), "@staged");
        assert_eq!(after_git, Vec::<String>::new(), "@git:1");
        assert!(body_of(&diff, "diff").contains("\n-v1\n+v2\n"), "{diff}");
        assert_eq!(body_of(&staged, "staged"), "(no changes)");
        assert!(body_of(&log, "git:1").contains("\n    signed\n"), "{log}");
    }

    /// The program settings reach the repository's config through a relative
    /// `include.path` and an `includeIf "gitdir:"` that matches any git dir,
    /// and `.git/info/attributes` turns filters on for every path, also
    /// through a macro: a process filter, a diff command, a pager, an
    /// fsmonitor and a hook that runs when the index is written. `@diff`,
    /// `@staged` and `@git:N` run none of them, though plain `git diff` in the
    /// same repository runs the first it meets.
    #[cfg(unix)]
    #[test]
    fn no_program_an_include_or_info_attributes_names_runs() {
        let dir = temp_dir("include-programs");
        let markers = dir.join("markers");
        let hooks = dir.join("hooks");
        fs::create_dir_all(&markers).unwrap();
        fs::create_dir_all(&hooks).unwrap();
        let names = [
            "external",
            "command",
            "textconv",
            "clean",
            "smudge",
            "process",
            "pclean",
            "pager",
            "fsmonitor",
            "hook",
            "git-hook",
        ];
        for name in names {
            marker_script(&dir.join(format!("{name}.sh")), &markers.join(name));
        }
        let script = |name: &str| dir.join(format!("{name}.sh")).display().to_string();
        let repo = edited_repo(&dir, "repo", &["a.txt", "b.txt"]);
        fs::write(repo.join(".gitattributes"), "*.txt diff=x\n").unwrap();
        fs::write(
            repo.join(".git/info/attributes"),
            "[attr]evil filter=f diff=x\n* evil\nb.txt filter=p\n",
        )
        .unwrap();
        fs::write(
            repo.join(".git/programs.inc"),
            format!(
                "[diff]\n\texternal = {}\n[diff \"x\"]\n\tcommand = {}\n\ttextconv = {}\n\
                 [filter \"f\"]\n\tclean = {}\n\tsmudge = {}\n\tprocess = {}\n\trequired = true\n\
                 [core]\n\tpager = {}\n[pager]\n\tdiff = {}\n\tlog = {}\n",
                script("external"),
                script("command"),
                script("textconv"),
                script("clean"),
                script("smudge"),
                script("process"),
                script("pager"),
                script("pager"),
                script("pager"),
            ),
        )
        .unwrap();
        fs::write(
            dir.join("gitdir.inc"),
            format!(
                "[core]\n\tfsmonitor = {}\n\thooksPath = {}\n[filter \"p\"]\n\tclean = {}\n\trequired = true\n",
                script("fsmonitor"),
                hooks.display(),
                script("pclean"),
            ),
        )
        .unwrap();
        plain_git(&repo, &["config", "include.path", "programs.inc"]);
        plain_git(
            &repo,
            &[
                "config",
                "includeIf.gitdir:/.path",
                dir.join("gitdir.inc").to_str().unwrap(),
            ],
        );
        for (hook, name) in [
            (hooks.join("post-index-change"), "hook"),
            (repo.join(".git/hooks/post-index-change"), "git-hook"),
        ] {
            fs::copy(dir.join(format!("{name}.sh")), hook).unwrap();
        }
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
        let diff = expand("What changed? @diff", &repo);
        let after_diff = ran();
        let staged = expand("staged? @staged", &repo);
        let after_staged = ran();
        let log = expand("history @git:1", &repo);
        let after_git = ran();
        let plain = Command::new("git")
            .current_dir(&repo)
            .args(["diff", "--stat"])
            .output()
            .unwrap();
        let after_plain = ran();
        let _ = fs::remove_dir_all(&dir);
        assert_eq!(after_diff, Vec::<String>::new(), "@diff: {diff}");
        assert_eq!(after_staged, Vec::<String>::new(), "@staged: {staged}");
        assert_eq!(after_git, Vec::<String>::new(), "@git:1: {log}");
        assert!(
            body_of(&diff, "diff").contains("+++ b/a.txt\n@@ -1 +1 @@\n-a.txt1\n+a.txt2\n"),
            "{diff}"
        );
        assert!(
            body_of(&diff, "diff").contains("+++ b/b.txt\n@@ -1 +1 @@\n-b.txt1\n+b.txt2\n"),
            "{diff}"
        );
        assert_eq!(body_of(&staged, "staged"), "(no changes)");
        assert!(body_of(&log, "git:1").contains("\n    one\n"), "{log}");
        // Plain git queries the fsmonitor, then fails on the first file's
        // process filter, which isn't a real one.
        assert_eq!(
            after_plain,
            ["fsmonitor", "process"],
            "plain git diff: {}",
            String::from_utf8_lossy(&plain.stderr)
        );
    }

    /// A repository with `a.txt` committed under `attributes` and changed
    /// since, in `dir/repo`.
    #[cfg(unix)]
    fn filtered_repo(dir: &Path, attributes: &[u8]) -> PathBuf {
        let repo = dir.join("repo");
        fs::create_dir_all(&repo).unwrap();
        assert!(git_in(&repo, &["init", "-q"]).status.success());
        fs::write(repo.join("a.txt"), "v1\n").unwrap();
        fs::write(repo.join(".gitattributes"), attributes).unwrap();
        assert!(git_in(&repo, &["add", "-A"]).status.success());
        assert!(git_in(&repo, &["commit", "-q", "-m", "one"])
            .status
            .success());
        fs::write(repo.join("a.txt"), "v2\n").unwrap();
        repo
    }

    /// `-c filter.a=b.clean=` would set `filter.a`, not switch `a=b` off, so
    /// no filter is switched off by its name.
    #[cfg(unix)]
    #[test]
    fn a_filter_named_with_an_equals_sign_does_not_run() {
        let dir = temp_dir("filter-eq");
        let marker = dir.join("clean-ran");
        marker_script(&dir.join("clean.sh"), &marker);
        let repo = filtered_repo(&dir, b"*.txt filter=a=b\n");
        let clean = dir.join("clean.sh").display().to_string();
        assert!(git_in(&repo, &["config", "filter.a=b.clean", &clean])
            .status
            .success());
        let out = expand("What changed? @diff", &repo);
        let ran = marker.exists();
        let _ = fs::remove_dir_all(&dir);
        assert!(!ran, "the filter ran: {out}");
        assert!(body_of(&out, "diff").contains("\n-v1\n+v2\n"), "{out}");
    }

    /// A driver named in bytes that aren't UTF-8 stays off too, and the
    /// repository's own index is left as it was.
    #[cfg(unix)]
    #[test]
    fn a_filter_whose_name_is_not_utf8_does_not_run() {
        let dir = temp_dir("filter-bytes");
        let marker = dir.join("clean-ran");
        marker_script(&dir.join("clean.sh"), &marker);
        let repo = filtered_repo(&dir, b"*.txt filter=x\xff\n");
        let mut config = fs::read(repo.join(".git/config")).unwrap();
        config.extend_from_slice(b"[filter \"x\xff\"]\n\tclean = ");
        config.extend_from_slice(dir.join("clean.sh").display().to_string().as_bytes());
        config.extend_from_slice(b"\n\trequired = true\n");
        fs::write(repo.join(".git/config"), config).unwrap();
        let index = fs::read(repo.join(".git/index")).unwrap();
        let out = expand("What changed? @diff", &repo);
        let ran = marker.exists();
        let index_after = fs::read(repo.join(".git/index")).unwrap();
        let _ = fs::remove_dir_all(&dir);
        assert!(!ran, "the filter ran: {out}");
        assert!(body_of(&out, "diff").contains("\n-v1\n+v2\n"), "{out}");
        assert!(index_after == index, "@diff wrote the repository's index");
    }

    /// A filter that shows up in `.git/config` while `@diff` runs (a command
    /// left running in the sandbox keeps rewriting it) stays off: nothing
    /// reads the config first to learn which filters to switch off.
    #[cfg(unix)]
    #[test]
    fn a_filter_added_while_at_diff_runs_stays_off() {
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc;
        let dir = temp_dir("filter-race");
        let marker = dir.join("clean-ran");
        marker_script(&dir.join("clean.sh"), &marker);
        let repo = filtered_repo(&dir, b"*.txt filter=f\n");
        let config = repo.join(".git/config");
        let without = fs::read(&config).unwrap();
        let mut with = without.clone();
        with.extend_from_slice(
            format!(
                "[filter \"f\"]\n\tclean = {}\n",
                dir.join("clean.sh").display()
            )
            .as_bytes(),
        );
        let stop = Arc::new(AtomicBool::new(false));
        let swapper = {
            let (stop, config) = (Arc::clone(&stop), config.clone());
            let next = config.with_extension("next");
            std::thread::spawn(move || {
                let mut flip = false;
                while !stop.load(Ordering::Relaxed) {
                    flip = !flip;
                    fs::write(&next, if flip { &with } else { &without }).unwrap();
                    fs::rename(&next, &config).unwrap();
                    std::thread::sleep(Duration::from_micros(300));
                }
            })
        };
        let bodies: Vec<String> = (0..100)
            .map(|_| body_of(&expand("What changed? @diff", &repo), "diff"))
            .collect();
        stop.store(true, Ordering::Relaxed);
        swapper.join().unwrap();
        let ran = marker.exists();
        let _ = fs::remove_dir_all(&dir);
        assert!(!ran, "the clean filter ran");
        for body in &bodies {
            assert!(body.contains("\n-v1\n+v2\n"), "{body}");
        }
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
        let out = expand("What changed? @diff", &repo);
        let ran = marker.exists();
        let _ = fs::remove_dir_all(&dir);
        assert!(!ran, "the submodule's clean filter ran: {out}");
        assert_eq!(body_of(&out, "diff"), "(no changes)");
    }

    /// A pipe planted where `@diff` reads the repository's attributes or
    /// index is refused at once instead of holding the turn.
    #[cfg(unix)]
    #[test]
    fn a_pipe_in_the_git_dir_fails_at_diff_without_blocking() {
        let dir = temp_dir("git-dir-fifo");
        let repo = edited_repo(&dir, "repo", &["a.txt"]);
        let attributes = repo.join(".git/info/attributes");
        let index = repo.join(".git/index");
        let mut bodies = Vec::new();
        for planted in [&attributes, &index] {
            let _ = fs::remove_file(planted);
            assert!(Command::new("mkfifo")
                .arg(planted)
                .status()
                .unwrap()
                .success());
            let (tx, rx) = std::sync::mpsc::channel();
            let at = repo.clone();
            std::thread::spawn(move || {
                let _ = tx.send(expand("@diff", &at));
            });
            let got = rx.recv_timeout(Duration::from_secs(5));
            if got.is_err() {
                // Unblock the reader so the process can exit.
                let _ = fs::OpenOptions::new().write(true).open(planted);
                let _ = rx.recv_timeout(Duration::from_secs(5));
            }
            bodies.push(got.map(|out| body_of(&out, "diff")));
            let _ = fs::remove_file(planted);
        }
        let _ = fs::remove_dir_all(&dir);
        assert_eq!(
            bodies,
            [
                Ok(format!(
                    "can't read {}: not a regular file",
                    attributes.display()
                )),
                Ok(format!(
                    "can't copy {} for @diff: not a regular file",
                    index.display()
                )),
            ]
        );
    }

    #[test]
    fn big_utf8_file_is_capped_at_a_char_boundary() {
        let dir = temp_dir("utf8cap");
        // 90,000 bytes of 3-byte chars; a byte cap lands mid-character.
        fs::write(dir.join("notes.txt"), "€".repeat(30_000)).unwrap();
        let out = expand("summarize @file:notes.txt", &dir);
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
        let out = expand("why @file:big.log:15000-15001", &dir);
        let one = expand("and @file:big.log:20000", &dir);
        let past = expand("and @file:big.log:20001-20005", &dir);
        let past_one = expand("and @file:big.log:20001", &dir);
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
        let out = attach_at_context(text, &Typed("what is @file:mine.txt".into()), &dir);
        let untouched = attach_at_context(text, &Typed("what is in it?".into()), &dir);
        let _ = fs::remove_dir_all(&dir);
        assert_eq!(
            out,
            format!("{text}\n\n{ATTACHED_CONTEXT_HEADER}\n### file:mine.txt\nMINE\n")
        );
        assert_eq!(untouched, text);
    }

    /// A [`Typed`] comes from the window's string, and [`Typed::keep`] only
    /// narrows it: a part of the words stays, words from anywhere else keep
    /// nothing, and with nothing typed no reference is read.
    #[test]
    fn typed_words_come_from_the_window_and_only_narrow() {
        let dir = temp_dir("typed-narrow");
        fs::write(dir.join("a.txt"), "A\n").unwrap();
        let sent: Typed = serde_json::from_str(
            r#""I asked on the side: @file:a.txt\nThe answer was: @file:b.txt""#,
        )
        .unwrap();
        let question = sent.keep(|t| &t["I asked on the side: ".len()..t.find('\n').unwrap()]);
        let foreign = sent.keep(|_| "@file:/home/user/private-notes.txt");
        let whole = sent.keep(|t| t);
        let untyped = attach_at_context("see @file:a.txt", &Typed::nothing(), &dir);
        let _ = fs::remove_dir_all(&dir);
        assert_eq!(
            sent.as_str(),
            "I asked on the side: @file:a.txt\nThe answer was: @file:b.txt"
        );
        assert_eq!(question.as_str(), "@file:a.txt");
        assert_eq!(foreign, Typed::nothing());
        assert_eq!(whole, sent);
        assert_eq!(untyped, "see @file:a.txt");
    }

    #[test]
    fn a_file_that_is_not_utf8_is_refused() {
        let dir = temp_dir("not-utf8");
        fs::write(dir.join("blob.bin"), [0x66, 0xff, 0xfe, 0x00]).unwrap();
        let shown = dir.join("blob.bin").canonicalize().unwrap();
        let out = expand("what is @file:blob.bin", &dir);
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
            let _ = tx.send(expand("look @file:pipe", &d2));
        });
        let got = rx.recv_timeout(Duration::from_secs(5));
        if got.is_err() {
            // Unblock the reader so the process can exit.
            let _ = fs::OpenOptions::new().write(true).open(&fifo);
            let _ = rx.recv_timeout(Duration::from_secs(5));
        }
        let _ = fs::remove_dir_all(&dir);
        let out = got.expect("@file:pipe still blocked after 5s");
        assert_eq!(
            body_of(&out, "file:pipe"),
            format!("can't read {}: not a regular file", shown.display())
        );
    }
}
