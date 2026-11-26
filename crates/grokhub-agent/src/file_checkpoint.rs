//! Per-step copy-before-write for agent file edits.
//!
//! Store layout: `<config>/checkpoints/<conversation>/<step>/` with a
//! `manifest.json` and opaque blobs. Before the first shell command of a
//! step that may write, the whole workspace (minus ignored folders, `.git`
//! and big files) is snapshotted into a content-addressed `objects/` store
//! shared by the conversation's steps, so `/rewind step` can put back every
//! file a shell command changed or deleted and move aside the ones it made.
//! Unchanged files are found by size and mtime and cost no copy. Sensitive `$HOME` leaves (via [`grokhub_core::rewind_sensitive`]) are never
//! copied. Retention keeps the last [`MAX_RUNS`] conversation runs or
//! [`MAX_BYTES`] of store, whichever bound hits first.

use std::cell::RefCell;
use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::perm;

/// Keep at most this many conversation runs under `checkpoints/`.
pub const MAX_RUNS: usize = 20;
/// Soft size cap for the whole checkpoints tree (200 MB).
pub const MAX_BYTES: u64 = 200 * 1024 * 1024;
/// Most files a workspace snapshot walks. Past it the snapshot is partial,
/// and a restore of it moves no new files aside, since it can't tell them
/// from files it never saw.
const SNAPSHOT_MAX_FILES: usize = 20_000;
/// Most new bytes one workspace snapshot copies into the store. Files past it
/// are left out (and a restore leaves them alone).
const SNAPSHOT_MAX_NEW_BYTES: u64 = 64 * 1024 * 1024;
/// Files larger than this are left out of a workspace snapshot.
const SNAPSHOT_MAX_FILE_BYTES: u64 = 4 * 1024 * 1024;
/// A cached object is reused only for a file whose mtime is at least this
/// much older than when it was hashed, so a write in the same clock tick as
/// the hash can't hide behind an unchanged size and mtime (git's "racy" rule).
const RACY_NANOS: u128 = 2_000_000_000;

thread_local! {
    static ACTIVE: RefCell<Option<Active>> = const { RefCell::new(None) };
}

#[derive(Clone)]
struct Active {
    conversation: String,
    step: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct Manifest {
    step: u64,
    created_at: u64,
    files: Vec<FileEntry>,
    /// The workspace as it stood before the step's first shell command.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    snapshot: Option<Snapshot>,
}

/// The workspace before a shell command: every walked file, by path under
/// `root`, with its object in the conversation's `objects/` store.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct Snapshot {
    /// Canonical workspace root with `/` separators.
    root: String,
    /// The command it was taken before, redacted and cut, for the listing.
    command: String,
    files: Vec<SnapFile>,
    /// Files walked but not copied (too big, over budget, unreadable or
    /// sensitive): a restore neither writes them nor moves them aside.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    left_out: Vec<String>,
    /// The walk saw the whole workspace; only then may a restore move aside
    /// files the snapshot doesn't hold.
    complete: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct SnapFile {
    /// Path under the snapshot root, `/` separated.
    path: String,
    /// SHA-256 hex of the bytes, the file's name under `objects/`.
    object: String,
}

/// Last known object for a path, so an unchanged file isn't read again.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct IndexEntry {
    len: u64,
    mtime: u128,
    hashed_at: u128,
    object: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct FileEntry {
    /// Absolute path with `/` separators.
    path: String,
    kind: EntryKind,
    /// Blob filename under `blobs/` when `kind` is `Content`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    blob: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
enum EntryKind {
    /// File existed; bytes are in `blob`.
    Content,
    /// File did not exist before the write (restore removes it).
    Created,
}

/// One step as shown by `/rewind step`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepInfo {
    pub step: u64,
    pub created_at: u64,
    pub files: Vec<String>,
}

/// Begin a new step for this conversation's next tool batch. Returns the step id.
pub fn begin_step(conversation_id: &str) -> Option<u64> {
    let conv = sanitize_id(conversation_id);
    if conv.is_empty() {
        return None;
    }
    let step = next_step_id(&conv_dir(&conv));
    let dir = step_dir(&conv, step);
    if let Err(err) = fs::create_dir_all(dir.join("blobs")) {
        eprintln!("file_checkpoint: could not create step dir: {err}");
        return None;
    }
    let manifest = Manifest {
        step,
        created_at: now_secs(),
        files: Vec::new(),
        snapshot: None,
    };
    if let Err(err) = save_manifest(&dir, &manifest) {
        eprintln!("file_checkpoint: could not write manifest: {err}");
        return None;
    }
    ACTIVE.with(|slot| {
        *slot.borrow_mut() = Some(Active {
            conversation: conv,
            step,
        });
    });
    Some(step)
}

/// Clear the active step (tests).
#[cfg(test)]
pub fn clear_active_for_tests() {
    ACTIVE.with(|slot| *slot.borrow_mut() = None);
}

/// Snapshot `path` before an agent write. No-op when there is no active step,
/// the path is sensitive, or this path was already snapshotted in the step.
pub fn before_write(path: &Path) {
    let Some(active) = ACTIVE.with(|slot| slot.borrow().clone()) else {
        return;
    };
    let homes = home_keys();
    let key = path_key(path);
    if key.is_empty() || sensitive(&key, &homes) || sensitive(&canonical_key(path), &homes) {
        return;
    }
    let step_dir = step_dir(&active.conversation, active.step);
    if let Err(err) = fs::create_dir_all(step_dir.join("blobs")) {
        eprintln!("file_checkpoint: could not create step dir: {err}");
        return;
    }
    let mut manifest = load_manifest(&step_dir).unwrap_or_else(|| Manifest {
        step: active.step,
        created_at: now_secs(),
        files: Vec::new(),
        snapshot: None,
    });
    if manifest.files.iter().any(|f| f.path == key) || in_snapshot(&manifest, &canonical_key(path))
    {
        return;
    }
    let entry = if path.is_file() {
        let blob = blob_name(&key);
        let dest = step_dir.join("blobs").join(&blob);
        match copy_file_bytes(path, &dest) {
            Ok(()) => FileEntry {
                path: key,
                kind: EntryKind::Content,
                blob: Some(blob),
            },
            Err(err) => {
                eprintln!("file_checkpoint: could not copy {}: {err}", path.display());
                return;
            }
        }
    } else {
        FileEntry {
            path: key,
            kind: EntryKind::Created,
            blob: None,
        }
    };
    manifest.files.push(entry);
    if let Err(err) = save_manifest(&step_dir, &manifest) {
        eprintln!("file_checkpoint: could not write manifest: {err}");
        return;
    }
    prune_keeping(Some(&active.conversation));
}

/// Commands that write files whatever their arguments, lowercased: the sh
/// ones and PowerShell's, with Windows PowerShell 5.1's short names.
const WRITERS: &[&str] = &[
    "rm",
    "mv",
    "cp",
    "tee",
    "truncate",
    "patch",
    "touch",
    "dd",
    "mkdir",
    "set-content",
    "add-content",
    "clear-content",
    "out-file",
    "tee-object",
    "new-item",
    "remove-item",
    "copy-item",
    "move-item",
    "rename-item",
    "del",
    "erase",
    "ri",
    "copy",
    "cpi",
    "move",
    "mi",
    "ren",
    "rni",
    "ni",
    "ac",
    "clc",
];
/// `git` subcommands that change the working tree.
const GIT_WRITERS: &[&str] = &[
    "checkout",
    "switch",
    "reset",
    "restore",
    "apply",
    "am",
    "clean",
    "rm",
    "mv",
    "stash",
    "pull",
    "merge",
    "rebase",
    "cherry-pick",
    "revert",
];
/// `git stash` subcommands that only read.
const STASH_READS: &[&str] = &["list", "show"];
/// Package managers' subcommands that change the project's files or the
/// packages installed in it.
const PACKAGE_WRITERS: &[(&str, &[&str])] = &[
    ("cargo", &["add", "remove", "rm", "fix"]),
    (
        "npm",
        &["install", "i", "ci", "add", "remove", "uninstall", "rm"],
    ),
    ("pnpm", &["install", "i", "add", "remove", "rm"]),
    ("yarn", &["install", "add", "remove"]),
    ("pip", &["install", "uninstall"]),
    ("pip3", &["install", "uninstall"]),
];
/// Shell words that come before the command they start: `then rm a.txt`.
const KEYWORDS: &[&str] = &["if", "then", "else", "elif", "do", "while", "until", "!"];

/// True when `command` clearly changes files: a redirect that writes a file
/// (not `2>&1` or `> /dev/null`), or a command whose program writes
/// ([`WRITERS`]), `sed`, `perl` or `ruby` with `-i`, a `git` or package
/// manager subcommand that changes the tree, a formatter that writes, or
/// `find` with `-delete` or an `-exec` that writes. Only
/// the program's own position counts (`sudo rm` does, `ls rm` doesn't), names
/// are read lowercased, and quoted text, comments and here-document bodies
/// are data. An interpreter (`python -m pytest`, `bash -c …`) may or may not
/// write, so it doesn't count.
pub(crate) fn changes_files(command: &str) -> bool {
    if crate::shell_redirect::redirects(command)
        .iter()
        .any(|r| r.file.is_some())
    {
        return true;
    }
    commands(&without_heredoc_bodies(command))
        .iter()
        .any(|words| writes(words))
}

/// `command` is one command alone: no `&&`, `;`, pipe or line break joins
/// another to it.
pub(crate) fn one_command(command: &str) -> bool {
    commands(&without_heredoc_bodies(command)).len() == 1
}

/// One command, as its words, clearly writes files.
fn writes(words: &[&str]) -> bool {
    let Some((name, args)) = program(words) else {
        return false;
    };
    // A formatter told to check or show a diff writes nothing.
    let checks = args.iter().any(|w| matches!(*w, "--check" | "--diff"));
    let first = args.first().copied().unwrap_or("");
    match name.as_str() {
        "git" => git_subcommand(args).is_some_and(|(sub, rest)| {
            GIT_WRITERS.contains(&sub.as_str())
                && !(sub == "stash" && rest.first().is_some_and(|w| STASH_READS.contains(w)))
        }),
        "sed" | "perl" | "ruby" => args.iter().any(|w| in_place(w)),
        "cargo" if first == "fmt" => !checks,
        "rustfmt" | "black" => !checks,
        "prettier" => args.iter().any(|w| matches!(*w, "--write" | "-w")),
        "eslint" => args.contains(&"--fix"),
        "gofmt" => args.contains(&"-w"),
        "find" => {
            args.contains(&"-delete")
                || args
                    .iter()
                    .position(|w| matches!(*w, "-exec" | "-execdir" | "-ok" | "-okdir"))
                    .is_some_and(|at| writes(&args[at + 1..]))
        }
        name => {
            WRITERS.contains(&name)
                || PACKAGE_WRITERS
                    .iter()
                    .any(|(tool, subs)| *tool == name && subs.contains(&first))
        }
    }
}

/// Words that run the command after them: `sudo rm`, `xargs rm`,
/// `timeout 10 rm`.
const WRAPPERS: &[&str] = &[
    "sudo", "doas", "env", "time", "nohup", "nice", "ionice", "timeout", "xargs", "command",
    "exec", "stdbuf", "chrt", "builtin",
];

/// The program a command runs, lowercased and without its folder, and the
/// words after it. `NAME=value` settings, shell keywords ([`KEYWORDS`]) and
/// wrappers before it are passed over, with a wrapper's flags and numbers
/// (`nice -n 5`, `timeout 10`).
fn program<'a, 'w>(words: &'a [&'w str]) -> Option<(String, &'a [&'w str])> {
    let mut i = 0;
    let mut after_wrapper = false;
    while let Some(word) = words.get(i) {
        let setting = !word.starts_with('-')
            && word.split_once('=').is_some_and(|(name, _)| {
                !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
            });
        let wrapper_arg = after_wrapper
            && (word.starts_with('-') || word.chars().all(|c| c.is_ascii_digit() || c == '.'));
        let name = base_name(word);
        if setting || wrapper_arg || KEYWORDS.contains(word) {
            i += 1;
        } else if WRAPPERS.contains(&name.as_str()) {
            after_wrapper = true;
            i += 1;
        } else {
            return Some((name, &words[i + 1..]));
        }
    }
    None
}

/// `word` lowercased, without any folder before it.
fn base_name(word: &str) -> String {
    word.rsplit(['/', '\\'])
        .next()
        .unwrap_or("")
        .to_ascii_lowercase()
}

/// The subcommand of a `git` command whose arguments are `args`, past the
/// options before it (`git -C sub reset`), and the words after it.
fn git_subcommand<'a, 'w>(args: &'a [&'w str]) -> Option<(String, &'a [&'w str])> {
    let mut i = 0;
    while let Some(word) = args.get(i) {
        if matches!(
            *word,
            "-C" | "-c" | "--git-dir" | "--work-tree" | "--namespace"
        ) {
            i += 2;
        } else if word.starts_with('-') {
            i += 1;
        } else {
            return Some((word.to_ascii_lowercase(), &args[i + 1..]));
        }
    }
    None
}

/// The edit-in-place flag of `sed`, `perl` and `ruby`: `-i`, `-i.bak`,
/// `--in-place`, or a short cluster such as `-pi` or `-Ei`. Case matters:
/// perl's `-I` adds an include path.
fn in_place(word: &str) -> bool {
    if word == "--in-place" || word.starts_with("--in-place=") {
        return true;
    }
    word.strip_prefix('-')
        .filter(|flags| !flags.starts_with('-'))
        .is_some_and(|flags| {
            let letters: String = flags
                .chars()
                .take_while(char::is_ascii_alphabetic)
                .collect();
            letters.len() <= 3 && letters.contains('i')
        })
}

/// `command` with each here-document's body left out: the lines after a
/// line that opens one (`<<EOF`, `<<-'EOF'`), up to its closing word.
fn without_heredoc_bodies(command: &str) -> String {
    let mut kept = String::new();
    let mut lines = command.lines();
    while let Some(line) = lines.next() {
        kept.push_str(line);
        kept.push('\n');
        for delimiter in heredoc_delimiters(line) {
            for body in lines.by_ref() {
                if body.trim() == delimiter {
                    break;
                }
            }
        }
    }
    kept
}

/// The closing words of the here-documents `line` opens, in order. A `<<<`
/// here-string and a shift (`$((1<<2))`) open none.
fn heredoc_delimiters(line: &str) -> Vec<String> {
    line.match_indices("<<")
        .filter(|(at, _)| !line[..*at].ends_with('<') && !line[at + 2..].starts_with('<'))
        .filter_map(|(at, _)| {
            let rest = line[at + 2..]
                .trim_start_matches('-')
                .trim_start()
                .trim_start_matches(['\'', '"']);
            let word: String = rest
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                .collect();
            (!word.is_empty() && !word.chars().all(|c| c.is_ascii_digit())).then_some(word)
        })
        .collect()
}

/// Each command of `command` as its words outside quotes, in order. A new
/// command starts after `;`, `|`, `&`, a newline, a paren, a brace or a
/// backtick. Quoted text and a comment make no word.
fn commands(command: &str) -> Vec<Vec<&str>> {
    let mut out = Vec::new();
    let mut words = Vec::new();
    let mut start = None;
    let mut quote = None;
    let mut comment = false;
    for (i, c) in command.char_indices() {
        if comment && c != '\n' {
            continue;
        }
        comment = false;
        if let Some(open) = quote {
            if c == open {
                quote = None;
            }
            continue;
        }
        let splits = matches!(c, ';' | '|' | '&' | '(' | ')' | '{' | '}' | '`' | '\n');
        if c.is_whitespace() || splits || matches!(c, '\'' | '"') {
            if let Some(at) = start.take() {
                words.push(&command[at..i]);
            }
            if matches!(c, '\'' | '"') {
                quote = Some(c);
            } else if splits && !words.is_empty() {
                out.push(std::mem::take(&mut words));
            }
        } else if c == '#' && start.is_none() {
            comment = true;
        } else if start.is_none() {
            start = Some(i);
        }
    }
    if let Some(at) = start {
        words.push(&command[at..]);
    }
    if !words.is_empty() {
        out.push(words);
    }
    out
}

/// Snapshot the whole workspace before a shell command that may write
/// (Card 957), so `/rewind step` can undo anything it did to the tree:
/// changed and deleted files come back, new ones are moved aside.
///
/// No-op when there is no active step, the command is read-only, or the step
/// already holds a snapshot (the first one is the state before the step).
/// Returns the number of files the step's snapshot holds.
pub fn before_shell(workspace: &Path, command: &str) -> usize {
    let Some(active) = ACTIVE.with(|slot| slot.borrow().clone()) else {
        return 0;
    };
    if reads_only(command) {
        return 0;
    }
    let Ok(root) = workspace.canonicalize() else {
        return 0;
    };
    let dir = step_dir(&active.conversation, active.step);
    let mut manifest = load_manifest(&dir).unwrap_or_else(|| Manifest {
        step: active.step,
        created_at: now_secs(),
        files: Vec::new(),
        snapshot: None,
    });
    if let Some(snap) = &manifest.snapshot {
        return snap.files.len();
    }
    let Some(snap) = take_snapshot(&active.conversation, &root, command) else {
        return 0;
    };
    let count = snap.files.len();
    manifest.snapshot = Some(snap);
    if let Err(err) = save_manifest(&dir, &manifest) {
        eprintln!("file_checkpoint: could not write manifest: {err}");
        return 0;
    }
    prune_keeping(Some(&active.conversation));
    count
}

/// Programs that only read the tree whatever their arguments, lowercased.
const READERS: &[&str] = &[
    "ls",
    "dir",
    "cat",
    "type",
    "head",
    "tail",
    "less",
    "more",
    "grep",
    "egrep",
    "fgrep",
    "rg",
    "wc",
    "pwd",
    "echo",
    "printf",
    "which",
    "where",
    "whereis",
    "stat",
    "file",
    "du",
    "df",
    "tree",
    "sort",
    "uniq",
    "cut",
    "tr",
    "jq",
    "diff",
    "cmp",
    "basename",
    "dirname",
    "realpath",
    "readlink",
    "date",
    "env",
    "printenv",
    "true",
    "false",
    "test",
    "uname",
    "whoami",
    "hostname",
    "get-content",
    "gc",
    "get-childitem",
    "gci",
    "select-string",
    "sls",
    "get-item",
    "gi",
    "get-location",
    "gl",
    "test-path",
    "resolve-path",
    "measure-object",
    "write-output",
    "write-host",
];

/// `git` subcommands that only read the tree.
const GIT_READERS: &[&str] = &[
    "status",
    "log",
    "diff",
    "show",
    "blame",
    "ls-files",
    "ls-tree",
    "rev-parse",
    "rev-list",
    "describe",
    "shortlog",
    "grep",
    "cat-file",
    "whatchanged",
];

/// True when every command in `command` plainly only reads (`git status`,
/// `cat a | grep b`); anything else, a build or test runner included, gets a
/// workspace snapshot first. `sort -o`, `find`, and git with `-c` or
/// `--output` may write, so they don't count as reads.
fn reads_only(command: &str) -> bool {
    let trimmed = command.trim();
    if trimmed.is_empty() {
        return true;
    }
    if trimmed.contains('>') || changes_files(trimmed) {
        return false;
    }
    let body = without_heredoc_bodies(trimmed);
    let cmds = commands(&body);
    !cmds.is_empty()
        && cmds.iter().all(|words| {
            let Some((name, args)) = program(words) else {
                return true;
            };
            if name == "git" {
                let risky = args
                    .iter()
                    .any(|a| matches!(*a, "-c" | "--config-env") || a.starts_with("--output"));
                return !risky
                    && git_subcommand(args)
                        .is_some_and(|(sub, _)| GIT_READERS.contains(&sub.as_str()));
            }
            let writes_out = name == "sort"
                && args
                    .iter()
                    .any(|a| a.starts_with("-o") || a.starts_with("--output"));
            READERS.contains(&name.as_str()) && !writes_out
        })
}

/// `key` is a file the step's workspace snapshot already holds, so its
/// earlier bytes are kept and a later copy-before-write would only keep a
/// newer state.
fn in_snapshot(manifest: &Manifest, key: &str) -> bool {
    let Some(snap) = &manifest.snapshot else {
        return false;
    };
    key.strip_prefix(snap.root.as_str())
        .and_then(|rest| rest.strip_prefix('/'))
        .is_some_and(|rel| snap.files.iter().any(|f| f.path == rel))
}

/// A root a snapshot may walk: not a drive or filesystem root, not `$HOME`
/// itself and not a sensitive leaf under it.
fn snapshot_root_ok(key: &str, home: &str) -> bool {
    let plain = key
        .strip_prefix("//?/")
        .unwrap_or(key)
        .trim_end_matches('/');
    let home = home.strip_prefix("//?/").unwrap_or(home).trim_end_matches('/');
    let drive_root = plain.len() <= 2 && plain.ends_with(':');
    !(plain.is_empty()
        || drive_root
        || (!home.is_empty() && plain.eq_ignore_ascii_case(home))
        || grokhub_core::rewind_sensitive(plain, home))
}

/// The workspace's regular files, as the grep tool walks it: `.gitignore`d
/// and `.git` left out, dotfiles in, links not followed. The flag is false
/// when the walk stopped at `limit` files.
fn walk_files(root: &Path, limit: usize) -> (Vec<(PathBuf, String)>, bool) {
    let walker = ignore::WalkBuilder::new(root)
        .standard_filters(false)
        .follow_links(false)
        .git_ignore(true)
        .git_exclude(true)
        .require_git(false)
        .hidden(false)
        .sort_by_file_name(|a, b| a.cmp(b))
        .filter_entry(|ent| ent.file_name() != ".git")
        .build();
    let mut out = Vec::new();
    for ent in walker.flatten() {
        if !ent.file_type().is_some_and(|t| t.is_file()) {
            continue;
        }
        let Ok(rel) = ent.path().strip_prefix(root) else {
            continue;
        };
        if out.len() >= limit {
            return (out, false);
        }
        let rel = rel
            .components()
            .map(|c| c.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/");
        out.push((ent.path().to_path_buf(), rel));
    }
    (out, true)
}

fn take_snapshot(conv: &str, root: &Path, command: &str) -> Option<Snapshot> {
    let homes = home_keys();
    let root_key = path_key(root);
    if !homes.iter().all(|h| snapshot_root_ok(&root_key, h)) {
        return None;
    }
    let objects = conv_dir(conv).join("objects");
    if let Err(err) = fs::create_dir_all(&objects) {
        eprintln!("file_checkpoint: could not create object store: {err}");
        return None;
    }
    let mut index = load_index(conv);
    let (walked, complete) = walk_files(root, SNAPSHOT_MAX_FILES);
    let mut files = Vec::new();
    let mut left_out = Vec::new();
    let mut new_bytes = 0u64;
    for (path, rel) in walked {
        let key = path_key(&path);
        let meta = fs::metadata(&path).ok();
        let len = meta.as_ref().map(|m| m.len());
        if sensitive(&key, &homes)
            || len.is_none_or(|n| n > SNAPSHOT_MAX_FILE_BYTES)
        {
            left_out.push(rel);
            continue;
        }
        let len = len.unwrap_or(0);
        let mtime = meta.as_ref().map(mtime_nanos).unwrap_or(0);
        let cached = index.get(&key).filter(|e| {
            e.len == len
                && e.mtime == mtime
                && e.hashed_at >= mtime.saturating_add(RACY_NANOS)
                && objects.join(&e.object).is_file()
        });
        if let Some(hit) = cached {
            files.push(SnapFile {
                path: rel,
                object: hit.object.clone(),
            });
            continue;
        }
        if new_bytes.saturating_add(len) > SNAPSHOT_MAX_NEW_BYTES {
            left_out.push(rel);
            continue;
        }
        let Ok(bytes) = fs::read(&path) else {
            left_out.push(rel);
            continue;
        };
        let object = sha_hex(&bytes);
        let dest = objects.join(&object);
        if !dest.is_file() {
            if let Err(err) = write_object(&dest, &bytes) {
                eprintln!("file_checkpoint: could not store {}: {err}", path.display());
                left_out.push(rel);
                continue;
            }
            new_bytes = new_bytes.saturating_add(bytes.len() as u64);
        }
        index.insert(
            key,
            IndexEntry {
                len: bytes.len() as u64,
                mtime,
                hashed_at: now_nanos(),
                object: object.clone(),
            },
        );
        files.push(SnapFile { path: rel, object });
    }
    save_index(conv, &index);
    let command = grokhub_core::redact_secrets(command.trim());
    let mut shown: String = command.chars().take(60).collect();
    if shown.len() < command.len() {
        shown.push('…');
    }
    Some(Snapshot {
        root: root_key,
        command: shown,
        files,
        left_out,
        complete,
    })
}

/// Put the workspace back as `snap` holds it. Files it holds are rewritten
/// when they differ; when the walk was complete, files it never saw are moved
/// into `stash`. Returns (written, moved aside).
fn restore_snapshot(
    conv: &str,
    snap: &Snapshot,
    stash: &Path,
    homes: &[String],
) -> Result<(usize, usize), String> {
    let root = PathBuf::from(snap.root.replace('/', std::path::MAIN_SEPARATOR_STR));
    let objects = conv_dir(conv).join("objects");
    let mut written = 0usize;
    for f in &snap.files {
        let path = root.join(f.path.replace('/', std::path::MAIN_SEPARATOR_STR));
        if sensitive(&path_key(&path), homes) {
            continue;
        }
        let bytes = fs::read(objects.join(&f.object))
            .map_err(|e| format!("missing snapshot bytes for {}: {e}", f.path))?;
        if fs::read(&path).is_ok_and(|now| now == bytes) {
            continue;
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("create parent: {e}"))?;
        }
        fs::write(&path, &bytes).map_err(|e| format!("restore {}: {e}", f.path))?;
        written += 1;
    }
    let mut moved = 0usize;
    if !snap.complete {
        return Ok((written, moved));
    }
    let known: std::collections::HashSet<&str> = snap
        .files
        .iter()
        .map(|f| f.path.as_str())
        .chain(snap.left_out.iter().map(String::as_str))
        .collect();
    let (now, _) = walk_files(&root, SNAPSHOT_MAX_FILES.saturating_mul(2));
    for (path, rel) in now {
        let key = path_key(&path);
        if known.contains(rel.as_str()) || sensitive(&key, homes) {
            continue;
        }
        // A big new file stays where it is: the undo snapshot can't hold it.
        if fs::metadata(&path).is_ok_and(|m| m.len() > SNAPSHOT_MAX_FILE_BYTES) {
            continue;
        }
        let dest = stash.join(blob_name(&key));
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        if fs::rename(&path, &dest).is_err() {
            copy_file_bytes(&path, &dest)?;
            fs::remove_file(&path).map_err(|e| e.to_string())?;
        }
        moved += 1;
    }
    Ok((written, moved))
}

fn index_path(conv: &str) -> PathBuf {
    conv_dir(conv).join("objects").join("index.json")
}

fn load_index(conv: &str) -> std::collections::HashMap<String, IndexEntry> {
    fs::read(index_path(conv))
        .ok()
        .and_then(|raw| serde_json::from_slice(&raw).ok())
        .unwrap_or_default()
}

fn save_index(conv: &str, index: &std::collections::HashMap<String, IndexEntry>) {
    if let Ok(raw) = serde_json::to_vec(index) {
        let _ = fs::write(index_path(conv), raw);
    }
}

/// Write an object whole or not at all, so a crash can't leave a torn one
/// under its hash.
fn write_object(dest: &Path, bytes: &[u8]) -> Result<(), String> {
    let tmp = dest.with_extension(format!("tmp{}", std::process::id()));
    fs::write(&tmp, bytes).map_err(|e| e.to_string())?;
    fs::rename(&tmp, dest).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        e.to_string()
    })
}

fn sha_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn mtime_nanos(meta: &fs::Metadata) -> u128 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_nanos())
        .unwrap_or(0)
}

fn now_nanos() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0)
}

/// Steps for a conversation, oldest first.
pub fn list_steps(conversation_id: &str) -> Vec<StepInfo> {
    let conv = sanitize_id(conversation_id);
    if conv.is_empty() {
        return Vec::new();
    }
    let root = conv_dir(&conv);
    let Ok(rd) = fs::read_dir(&root) else {
        return Vec::new();
    };
    let mut steps: Vec<u64> = rd
        .flatten()
        .filter_map(|e| e.file_name().to_string_lossy().parse::<u64>().ok())
        .collect();
    steps.sort_unstable();
    steps
        .into_iter()
        .filter_map(|step| {
            let dir = step_dir(&conv, step);
            let m = load_manifest(&dir)?;
            if m.files.is_empty() && m.snapshot.is_none() {
                return None;
            }
            let mut files: Vec<String> = m.files.iter().map(|f| display_path(&f.path)).collect();
            if let Some(snap) = &m.snapshot {
                files.insert(
                    0,
                    format!(
                        "workspace before `{}` ({} files)",
                        snap.command,
                        snap.files.len()
                    ),
                );
            }
            Some(StepInfo {
                step: m.step,
                created_at: m.created_at,
                files,
            })
        })
        .collect()
}

/// Human-readable listing for `/rewind step`.
pub fn format_steps(conversation_id: &str) -> String {
    let steps = list_steps(conversation_id);
    if steps.is_empty() {
        return "No agent file checkpoints yet. Edit a file in this chat, then /rewind step again."
            .into();
    }
    let mut lines = vec![
        "Agent file checkpoints — /rewind step <n> restores that step (current files are saved first so you can undo):"
            .to_string(),
    ];
    for s in &steps {
        let files = if s.files.is_empty() {
            "(no files)".to_string()
        } else {
            s.files.join(", ")
        };
        lines.push(format!("  step {}: {files}", s.step));
    }
    lines.join("\n")
}

/// Restore files from `step`. Checkpoints the current state first so restore is undoable.
pub fn restore_step(conversation_id: &str, step: u64) -> Result<String, String> {
    let conv = sanitize_id(conversation_id);
    if conv.is_empty() {
        return Err("No conversation id".into());
    }
    let src_dir = step_dir(&conv, step);
    let manifest =
        load_manifest(&src_dir).ok_or_else(|| format!("No checkpoint for step {step}"))?;
    if manifest.files.is_empty() && manifest.snapshot.is_none() {
        return Err(format!("Step {step} has no files"));
    }
    // Undo step: snapshot current bytes for every path we are about to change,
    // and the whole workspace when the step holds a workspace snapshot.
    let undo = begin_step(&conv).ok_or_else(|| "Could not open undo step".to_string())?;
    for entry in &manifest.files {
        let path = PathBuf::from(entry.path.replace('/', std::path::MAIN_SEPARATOR_STR));
        before_write(&path);
    }
    if let Some(snap) = &manifest.snapshot {
        let root = PathBuf::from(snap.root.replace('/', std::path::MAIN_SEPARATOR_STR));
        let undo_dir = step_dir(&conv, undo);
        if let Some(mut undo_manifest) = load_manifest(&undo_dir)
            && let Some(now) = take_snapshot(&conv, &root, "/rewind step")
        {
            undo_manifest.snapshot = Some(now);
            save_manifest(&undo_dir, &undo_manifest)?;
        }
    }
    ACTIVE.with(|slot| *slot.borrow_mut() = None);

    let homes = home_keys();
    let mut restored = 0usize;
    let mut removed = 0usize;
    // The snapshot is the state before the step's first shell command; a
    // file's own copy, taken before any edit, is older still, so it goes last.
    if let Some(snap) = &manifest.snapshot {
        let (written, moved) = restore_snapshot(&conv, snap, &src_dir.join("removed"), &homes)?;
        restored += written;
        removed += moved;
    }
    for entry in &manifest.files {
        if sensitive(&entry.path, &homes) {
            continue;
        }
        let path = PathBuf::from(entry.path.replace('/', std::path::MAIN_SEPARATOR_STR));
        match entry.kind {
            EntryKind::Content => {
                let blob = entry
                    .blob
                    .as_deref()
                    .ok_or_else(|| format!("missing blob for {}", entry.path))?;
                let src = src_dir.join("blobs").join(blob);
                if let Some(parent) = path.parent() {
                    if !parent.as_os_str().is_empty() {
                        fs::create_dir_all(parent).map_err(|e| format!("create parent: {e}"))?;
                    }
                }
                copy_file_bytes(&src, &path).map_err(|e| format!("restore {}: {e}", entry.path))?;
                restored += 1;
            }
            EntryKind::Created => {
                if path.is_file() {
                    // Bytes already sit in the undo step; move the live file into the store.
                    let stash = src_dir.join("removed").join(blob_name(&entry.path));
                    if let Some(parent) = stash.parent() {
                        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
                    }
                    if fs::rename(&path, &stash).is_err() {
                        copy_file_bytes(&path, &stash)?;
                        fs::remove_file(&path).map_err(|e| e.to_string())?;
                    }
                    removed += 1;
                }
            }
        }
    }
    prune_keeping(Some(&conv));
    Ok(format!(
        "Restored step {step} ({restored} file(s), {removed} new file(s) moved aside). Undo with /rewind step {undo}."
    ))
}

/// Drop oldest conversation runs past [`MAX_RUNS`] or over [`MAX_BYTES`], but
/// never the run named `keep`: the one being written or restored right now
/// still counts toward the caps, it just isn't a victim.
/// Oldest first by sub-second mtime, ties broken by name, so runs made in the
/// same second don't drop in directory order.
fn prune_keeping(keep: Option<&str>) {
    let root = checkpoints_root();
    let Ok(rd) = fs::read_dir(&root) else {
        return;
    };
    let mut runs: Vec<(PathBuf, u128, u64)> = Vec::new();
    let mut kept = 0usize;
    let mut total: u64 = 0;
    for ent in rd.flatten() {
        let path = ent.path();
        if !path.is_dir() {
            continue;
        }
        let size = dir_size(&path);
        total += size;
        if keep.is_some_and(|k| ent.file_name() == k) {
            kept += 1;
            continue;
        }
        let mtime = dir_mtime_nanos(&path);
        runs.push((path, mtime, size));
    }
    runs.sort_by(|a, b| a.1.cmp(&b.1).then_with(|| a.0.cmp(&b.0)));
    let mut count = runs.len() + kept;
    for (path, _, size) in runs {
        if count <= MAX_RUNS && total <= MAX_BYTES {
            break;
        }
        let _ = fs::remove_dir_all(&path);
        total = total.saturating_sub(size);
        count -= 1;
    }
}

fn checkpoints_root() -> PathBuf {
    perm::config_dir().join("checkpoints")
}

fn conv_dir(conv: &str) -> PathBuf {
    checkpoints_root().join(conv)
}

fn step_dir(conv: &str, step: u64) -> PathBuf {
    conv_dir(conv).join(step.to_string())
}

fn next_step_id(dir: &Path) -> u64 {
    let Ok(rd) = fs::read_dir(dir) else {
        return 1;
    };
    let mut max = 0u64;
    for ent in rd.flatten() {
        if let Ok(n) = ent.file_name().to_string_lossy().parse::<u64>() {
            max = max.max(n);
        }
    }
    max.saturating_add(1).max(1)
}

fn sanitize_id(raw: &str) -> String {
    let s: String = raw
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' {
                c
            } else {
                '_'
            }
        })
        .collect();
    s.chars().take(120).collect()
}

fn path_key(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

/// Every spelling of the user's home a path key may start with: as set, and
/// canonical (a Windows short name like `RUNNER~1`, a verbatim prefix, a link).
fn home_keys() -> Vec<String> {
    let Some(home) = grokhub_core::user_home() else {
        return vec![String::new()];
    };
    let mut keys = vec![path_key(&home)];
    if let Ok(full) = home.canonicalize() {
        let full = path_key(&full);
        if !keys.contains(&full) {
            keys.push(full);
        }
    }
    keys
}

/// `key` is a sensitive `$HOME` leaf under any spelling of home.
fn sensitive(key: &str, homes: &[String]) -> bool {
    homes.iter().any(|h| grokhub_core::rewind_sensitive(key, h))
}

/// `path` spelled the way a snapshot's canonical root is, so a short or
/// differently cased spelling (Windows `RUNNER~1`) still matches. Falls back
/// to the parent's canonical form for a path that does not exist yet.
fn canonical_key(path: &Path) -> String {
    if let Ok(full) = path.canonicalize() {
        return path_key(&full);
    }
    match (
        path.parent().and_then(|p| p.canonicalize().ok()),
        path.file_name(),
    ) {
        (Some(parent), Some(name)) => path_key(&parent.join(name)),
        _ => path_key(path),
    }
}

fn display_path(key: &str) -> String {
    // Prefer the leaf name for the list; keep full when short.
    let leaf = key.rsplit('/').next().unwrap_or(key);
    if key.len() <= 48 {
        key.to_string()
    } else {
        format!("…/{leaf}")
    }
}

fn blob_name(key: &str) -> String {
    let mut h = DefaultHasher::new();
    key.hash(&mut h);
    format!("{:016x}", h.finish())
}

fn manifest_path(step_dir: &Path) -> PathBuf {
    step_dir.join("manifest.json")
}

fn load_manifest(step_dir: &Path) -> Option<Manifest> {
    let raw = fs::read(manifest_path(step_dir)).ok()?;
    serde_json::from_slice(&raw).ok()
}

fn save_manifest(step_dir: &Path, manifest: &Manifest) -> Result<(), String> {
    let raw = serde_json::to_vec_pretty(manifest).map_err(|e| e.to_string())?;
    fs::write(manifest_path(step_dir), raw).map_err(|e| e.to_string())
}

fn copy_file_bytes(src: &Path, dest: &Path) -> Result<(), String> {
    if let Some(parent) = dest.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
    }
    let bytes = fs::read(src).map_err(|e| e.to_string())?;
    fs::write(dest, &bytes).map_err(|e| e.to_string())
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn dir_mtime_nanos(path: &Path) -> u128 {
    fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_nanos())
        .unwrap_or(0)
}

fn dir_size(path: &Path) -> u64 {
    let mut total = 0u64;
    let Ok(rd) = fs::read_dir(path) else {
        return 0;
    };
    for ent in rd.flatten() {
        let p = ent.path();
        if p.is_dir() {
            total = total.saturating_add(dir_size(&p));
        } else if let Ok(meta) = ent.metadata() {
            total = total.saturating_add(meta.len());
        }
    }
    total
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::perm::ConfigGuard;

    fn scratch(tag: &str) -> (PathBuf, PathBuf, ConfigGuard) {
        let base = std::env::temp_dir().join(format!(
            "gh-fcp-{tag}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let _ = fs::remove_dir_all(&base);
        let cfg = base.join("cfg");
        let ws = base.join("ws");
        fs::create_dir_all(&cfg).unwrap();
        fs::create_dir_all(&ws).unwrap();
        let guard = ConfigGuard::set(&cfg);
        clear_active_for_tests();
        (base, ws, guard)
    }

    #[test]
    fn edit_twice_restore_step_one_gives_original_bytes() {
        let (base, ws, _guard) = scratch("twice");
        let path = ws.join("note.txt");
        fs::write(&path, b"original").unwrap();

        assert_eq!(begin_step("conv-a"), Some(1));
        before_write(&path);
        fs::write(&path, b"first-edit").unwrap();

        assert_eq!(begin_step("conv-a"), Some(2));
        before_write(&path);
        fs::write(&path, b"second-edit").unwrap();

        assert_eq!(fs::read(&path).unwrap(), b"second-edit");
        let msg = restore_step("conv-a", 1).unwrap();
        assert!(msg.contains("Restored step 1"), "{msg}");
        assert_eq!(fs::read(&path).unwrap(), b"original");

        // Restore created an undo step; re-restoring that undoes the restore.
        let steps = list_steps("conv-a");
        assert!(steps.len() >= 3, "{steps:?}");
        let undo = steps.last().unwrap().step;
        let _ = restore_step("conv-a", undo).unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"second-edit");
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn new_file_restore_moves_aside_not_hard_delete_only() {
        let (base, ws, _guard) = scratch("new");
        let path = ws.join("fresh.txt");

        assert_eq!(begin_step("conv-b"), Some(1));
        before_write(&path);
        assert!(!path.exists());
        fs::write(&path, b"brand-new").unwrap();
        assert!(path.is_file());

        let msg = restore_step("conv-b", 1).unwrap();
        assert!(msg.contains("Restored"), "{msg}");
        assert!(!path.exists(), "new file should be gone from workspace");
        // Undo step kept the bytes.
        let undo_steps = list_steps("conv-b");
        let undo = undo_steps.last().unwrap().step;
        let undo_dir = step_dir("conv-b", undo);
        let m = load_manifest(&undo_dir).unwrap();
        assert_eq!(m.files.len(), 1);
        assert_eq!(m.files[0].kind, EntryKind::Content);
        let blob = m.files[0].blob.as_ref().unwrap();
        let bytes = fs::read(undo_dir.join("blobs").join(blob)).unwrap();
        assert_eq!(bytes, b"brand-new");
        let _ = fs::remove_dir_all(&base);
    }

    /// Home set as one spelling (here a link; on Windows a short name like
    /// `RUNNER~1`) and the key written through another is still left out.
    #[cfg(unix)]
    #[test]
    fn a_sensitive_path_under_another_spelling_of_home_is_never_copied() {
        let (base, _ws, _guard) = scratch("sens-link");
        let real_home = base.join("home");
        let ssh = real_home.join(".ssh").join("id_test_checkpoint");
        fs::create_dir_all(ssh.parent().unwrap()).unwrap();
        fs::write(&ssh, b"secret-key-material").unwrap();
        let link = base.join("home-link");
        std::os::unix::fs::symlink(&real_home, &link).unwrap();

        let prev_home = std::env::var_os("HOME");
        unsafe { std::env::set_var("HOME", &link) };
        assert_eq!(begin_step("conv-l"), Some(1));
        before_write(&ssh);
        let m = load_manifest(&step_dir("conv-l", 1));
        match prev_home {
            Some(v) => unsafe { std::env::set_var("HOME", v) },
            None => unsafe { std::env::remove_var("HOME") },
        }
        assert!(
            m.as_ref().map(|m| m.files.is_empty()).unwrap_or(true),
            "sensitive must not be copied: {m:?}"
        );
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn sensitive_paths_are_never_copied() {
        let (base, _ws, _guard) = scratch("sens");
        let fake_home = base.join("home");
        let ssh = fake_home.join(".ssh").join("id_test_checkpoint");
        fs::create_dir_all(ssh.parent().unwrap()).unwrap();
        fs::write(&ssh, b"secret-key-material").unwrap();

        // Windows reads home from USERPROFILE first.
        let prev_home = std::env::var_os("HOME");
        let prev_profile = std::env::var_os("USERPROFILE");
        unsafe { std::env::set_var("HOME", &fake_home) };
        unsafe { std::env::set_var("USERPROFILE", &fake_home) };

        assert_eq!(begin_step("conv-c"), Some(1));
        before_write(&ssh);
        let dir = step_dir("conv-c", 1);
        let m = load_manifest(&dir);
        assert!(
            m.as_ref().map(|m| m.files.is_empty()).unwrap_or(true),
            "sensitive must not be copied: {m:?}"
        );

        for (name, prev) in [("HOME", prev_home), ("USERPROFILE", prev_profile)] {
            match prev {
                Some(v) => unsafe { std::env::set_var(name, v) },
                None => unsafe { std::env::remove_var(name) },
            }
        }
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn retention_prunes_oldest_runs() {
        let (base, ws, _guard) = scratch("prune");
        let path = ws.join("x.txt");
        fs::write(&path, b"v").unwrap();
        for i in 0..(MAX_RUNS + 3) {
            let id = format!("run-{i:02}");
            assert!(begin_step(&id).is_some());
            before_write(&path);
            let marker = conv_dir(&sanitize_id(&id)).join("touched");
            fs::write(&marker, [i as u8]).unwrap();
            // Ensure distinct mtimes across platforms.
            std::thread::sleep(std::time::Duration::from_millis(15));
        }
        prune_keeping(None);
        let root = checkpoints_root();
        let count = fs::read_dir(&root)
            .unwrap()
            .flatten()
            .filter(|e| e.path().is_dir())
            .count();
        assert!(count <= MAX_RUNS, "kept {count} runs, cap is {MAX_RUNS}");
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn the_run_being_written_is_never_pruned_even_when_it_is_the_oldest() {
        let (base, _ws, _guard) = scratch("keep");
        let root = checkpoints_root();
        for i in 0..(MAX_RUNS + 3) {
            let run = root.join(format!("run-{i:02}"));
            fs::create_dir_all(&run).unwrap();
            fs::write(run.join("touched"), [i as u8]).unwrap();
        }
        // run-00 is the oldest by mtime and by name: plain pruning drops it.
        prune_keeping(Some("run-00"));
        assert!(root.join("run-00").is_dir(), "the active run was pruned");
        let left = fs::read_dir(&root).unwrap().flatten().count();
        assert_eq!(left, MAX_RUNS, "the kept run still counts toward the cap");
        assert!(
            !root.join("run-01").exists(),
            "the next oldest goes instead"
        );
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn format_steps_lists_files() {
        let (base, ws, _guard) = scratch("fmt");
        let path = ws.join("a.txt");
        fs::write(&path, b"hi").unwrap();
        begin_step("conv-fmt").unwrap();
        before_write(&path);
        let text = format_steps("conv-fmt");
        assert!(text.contains("step 1"), "{text}");
        assert!(text.contains("a.txt"), "{text}");
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn before_shell_snapshots_mutated_files_and_rewinds_them() {
        let (base, ws, _guard) = scratch("shell");
        let path = ws.join("notes.txt");
        fs::write(&path, b"original content").unwrap();
        let step = begin_step("conv-shell").unwrap();

        // Read-only command does not snapshot.
        assert!(reads_only("git status"));
        assert_eq!(before_shell(&ws, "cat notes.txt"), 0);

        // Mutating shell command snapshots referenced workspace file.
        assert!(!reads_only("rm -f notes.txt"));
        assert_eq!(before_shell(&ws, "rm -f notes.txt"), 1);

        // Simulate shell mutation (deletion) and rewind.
        fs::remove_file(&path).unwrap();
        assert!(!path.exists());
        let restored = restore_step("conv-shell", step).unwrap();
        assert!(restored.contains("1 file"), "{restored}");
        assert_eq!(fs::read(&path).unwrap(), b"original content");

        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn powershell_writers_and_any_case_get_a_snapshot() {
        assert!(!reads_only("Set-Content a.txt x"));
        assert!(!reads_only(
            "(Get-Content a.rs) -replace 'a','b' | Set-Content a.rs"
        ));
        assert!(!reads_only("Remove-Item -Recurse build"));
        assert!(!reads_only("RM -f notes.txt"));
        // Windows PowerShell 5.1's short names.
        assert!(!reads_only("del a.txt"));
        assert!(!reads_only("copy a b"));
        assert!(!reads_only("Clear-Content a.txt"));
        assert!(!reads_only("Tee-Object -FilePath a.txt"));
        assert!(reads_only("Get-Content a.txt"));
        assert!(reads_only("git status"));
        // Still as wide as before: an interpreter, or any `>`.
        assert!(!reads_only("python -m pytest"));
        assert!(!reads_only("cargo test 2>&1 | tail -5"));
    }

    #[test]
    fn only_a_command_that_clearly_writes_changes_files() {
        for cmd in [
            "echo v1 > a.txt",
            "cat a >> log.txt",
            "sed -i s/a/b/ a.rs",
            "sed -i.bak 's/a/b/' a.rs",
            "perl -pi -e 's/a/b/' a.rs",
            "ruby -i -pe 'sub(/a/, \"b\")' a.txt",
            "printf x | tee a.txt",
            "rm -f a.txt",
            "/bin/cp a b",
            "sudo rm -rf build",
            "FOO=1 timeout 10 touch a.txt",
            "find . -name '*.tmp' -delete",
            "find . -name '*.tmp' -exec rm {} \\;",
            "cargo test && git checkout -- a.rs",
            "git -C sub reset --hard",
            "Set-Content a.txt v1",
            "(Get-Content a.rs) -replace 'a','b' | Set-Content a.rs",
            "Get-ChildItem *.tmp | Remove-Item",
            "New-Item -ItemType File a.txt",
            "Rename-Item a.txt b.txt",
            // Subcommands that change the tree or the project's files.
            "git stash",
            "git stash pop",
            "git switch other",
            "git pull",
            "git merge x",
            "git rebase main",
            "git cherry-pick abc123",
            "git revert HEAD",
            "git am fix.patch",
            "cargo add serde",
            "cargo fmt",
            "cargo fix --allow-dirty",
            "npm install x",
            "pnpm add x",
            "yarn remove x",
            "pip install x",
            "mkdir d",
            "rustfmt src/a.rs",
            "prettier --write .",
            // A command after a shell keyword.
            "for f in src/*.rs; do sed -i 's/old/new/' \"$f\"; done",
            "if true; then rm a.txt; fi",
            "while read f; do rm \"$f\"; done",
            "if test -f a.txt; then echo ok; else touch a.txt; fi",
            // PowerShell 5.1's short names and the rest of its writers.
            "del src\\old.rs",
            "erase a.txt",
            "copy a.rs.bak a.rs",
            "move a b",
            "ren a b",
            "ri a.txt",
            "ni a.txt",
            "ac a.txt x",
            "Tee-Object -FilePath a.txt",
            "Clear-Content a.txt",
        ] {
            assert!(changes_files(cmd), "{cmd}");
            assert!(!reads_only(cmd), "{cmd}");
        }
        for cmd in [
            "cargo test 2>&1 | tail -5",
            "cargo build > /dev/null 2>&1",
            "npm test *> $null",
            "python -m pytest",
            "node test.js",
            "bash -c 'rm a.txt'",
            "sed -n 1,20p a.rs",
            "cat a.rs | sed -n p; grep -i main a.rs",
            "perl -I lib -e 'print 1'",
            "grep -rn \"rm\" src",
            "ls cp",
            "echo rm a.txt",
            "git log --oneline -- reset.rs",
            "git status",
            "find . -name '*.rs'",
            "# rm a.txt\necho ok",
            "cat <<EOF\nrm a.txt\nEOF\necho ok",
            "Get-Content a.txt",
            "git stash list",
            "git stash show -p",
            "cargo fmt --check",
            "cargo fmt -- --check",
            "rustfmt --check src/a.rs",
            "cargo build",
            "npm test",
            "pip list",
            "prettier --check .",
            "for f in src/*.rs; do cat \"$f\"; done",
            "if test -f a.txt; then echo yes; fi",
        ] {
            assert!(!changes_files(cmd), "{cmd}");
        }
    }

    #[test]
    fn a_shell_step_rewinds_the_whole_workspace_and_undoes_cleanly() {
        let (base, ws, _guard) = scratch("wsnap");
        fs::write(ws.join(".gitignore"), "build/\n").unwrap();
        fs::create_dir_all(ws.join("src")).unwrap();
        fs::create_dir_all(ws.join("build")).unwrap();
        fs::write(ws.join("src/lib.rs"), b"fn old() {}").unwrap();
        fs::write(ws.join("keep.txt"), b"keep me").unwrap();
        fs::write(ws.join("build/out.bin"), b"ignored output").unwrap();

        let step = begin_step("conv-ws").unwrap();
        // The command names no file: the snapshot doesn't guess from it.
        assert_eq!(before_shell(&ws, "python tool.py"), 3);
        // What the command did: changed one file, deleted one, made two
        // (one in a new folder), and rebuilt the ignored output.
        fs::write(ws.join("src/lib.rs"), b"fn new() {}").unwrap();
        fs::remove_file(ws.join("keep.txt")).unwrap();
        fs::write(ws.join("made.txt"), b"made").unwrap();
        fs::create_dir_all(ws.join("gen")).unwrap();
        fs::write(ws.join("gen/x.rs"), b"generated").unwrap();
        fs::write(ws.join("build/out.bin"), b"rebuilt").unwrap();

        let shown = format_steps("conv-ws");
        assert!(
            shown.contains("workspace before `python tool.py` (3 files)"),
            "{shown}"
        );
        let msg = restore_step("conv-ws", step).unwrap();
        assert!(
            msg.contains("2 file(s), 2 new file(s) moved aside"),
            "{msg}"
        );
        assert_eq!(fs::read(ws.join("src/lib.rs")).unwrap(), b"fn old() {}");
        assert_eq!(fs::read(ws.join("keep.txt")).unwrap(), b"keep me");
        assert!(!ws.join("made.txt").exists());
        assert!(!ws.join("gen/x.rs").exists());
        // Ignored folders are neither snapshotted nor touched.
        assert_eq!(fs::read(ws.join("build/out.bin")).unwrap(), b"rebuilt");

        // The restore's own undo step puts the command's result back.
        let undo = list_steps("conv-ws").last().unwrap().step;
        assert_ne!(undo, step);
        restore_step("conv-ws", undo).unwrap();
        assert_eq!(fs::read(ws.join("src/lib.rs")).unwrap(), b"fn new() {}");
        assert!(!ws.join("keep.txt").exists());
        assert_eq!(fs::read(ws.join("made.txt")).unwrap(), b"made");
        assert_eq!(fs::read(ws.join("gen/x.rs")).unwrap(), b"generated");
        let _ = fs::remove_dir_all(&base);
    }

    /// A workspace reached by another spelling (a link here, a short
    /// `RUNNER~1` name on Windows) still matches the snapshot's canonical root.
    #[cfg(unix)]
    #[test]
    fn an_edit_through_another_spelling_of_the_workspace_keeps_the_snapshot() {
        let (base, ws, _guard) = scratch("wlink");
        let link = base.join("ws-link");
        std::os::unix::fs::symlink(&ws, &link).unwrap();
        let path = link.join("a.txt");
        fs::write(&path, b"v1").unwrap();
        let step = begin_step("conv-link").unwrap();
        assert_eq!(before_shell(&link, "rm -f b.txt"), 1);
        fs::write(&path, b"v2").unwrap();
        before_write(&path);
        let m = load_manifest(&step_dir("conv-link", step)).unwrap();
        assert!(m.files.is_empty(), "{:?}", m.files);
        restore_step("conv-link", step).unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"v1");
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn a_step_keeps_its_first_snapshot_and_its_earliest_bytes() {
        let (base, ws, _guard) = scratch("wfirst");
        let path = ws.join("a.txt");
        fs::write(&path, b"v1").unwrap();
        let step = begin_step("conv-first").unwrap();
        assert_eq!(before_shell(&ws, "rm -f b.txt"), 1);
        fs::write(&path, b"v2").unwrap();
        fs::write(ws.join("b.txt"), b"later").unwrap();
        // A second shell command and an edit in the same step keep v1.
        assert_eq!(before_shell(&ws, "rm -f a.txt"), 1);
        before_write(&path);
        fs::write(&path, b"v3").unwrap();
        let m = load_manifest(&step_dir("conv-first", step)).unwrap();
        assert!(m.files.is_empty(), "{:?}", m.files);
        restore_step("conv-first", step).unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"v1");
        assert!(!ws.join("b.txt").exists());
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn unchanged_files_add_no_bytes_to_the_store() {
        let (base, ws, _guard) = scratch("wdedupe");
        fs::write(ws.join("a.txt"), b"same").unwrap();
        fs::write(ws.join("b.txt"), b"same").unwrap();
        let objects = conv_dir("conv-dedupe").join("objects");
        let count = || {
            fs::read_dir(&objects)
                .unwrap()
                .flatten()
                .filter(|e| e.file_name() != "index.json")
                .count()
        };
        begin_step("conv-dedupe").unwrap();
        assert_eq!(before_shell(&ws, "make"), 2);
        assert_eq!(count(), 1);
        begin_step("conv-dedupe").unwrap();
        assert_eq!(before_shell(&ws, "make"), 2);
        assert_eq!(count(), 1);
        fs::write(ws.join("a.txt"), b"changed").unwrap();
        begin_step("conv-dedupe").unwrap();
        before_shell(&ws, "make");
        assert_eq!(count(), 2);
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn a_big_file_is_left_out_and_left_alone() {
        let (base, ws, _guard) = scratch("wbig");
        let big = vec![b'x'; (SNAPSHOT_MAX_FILE_BYTES + 1) as usize];
        fs::write(ws.join("big.bin"), &big).unwrap();
        fs::write(ws.join("small.txt"), b"s").unwrap();
        let step = begin_step("conv-big").unwrap();
        assert_eq!(before_shell(&ws, "make"), 1);
        fs::write(ws.join("big.bin"), b"shrunk").unwrap();
        restore_step("conv-big", step).unwrap();
        // Not in the snapshot, so not moved aside as new either.
        assert_eq!(fs::read(ws.join("big.bin")).unwrap(), b"shrunk");
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn only_a_plain_read_skips_the_snapshot() {
        for cmd in [
            "git status",
            "cat a.txt | grep x",
            "ls -la && git diff HEAD~1",
            "Get-Content a.txt",
            "rg foo src",
            "",
        ] {
            assert!(reads_only(cmd), "{cmd}");
        }
        for cmd in [
            "make",
            "cargo build",
            "npm run build",
            "./gen.sh",
            "cat a > b",
            "git checkout -- a.txt",
            "git -c diff.external=x diff",
            "git diff --output=patch.txt",
            "sort -o a.txt a.txt",
            "find . -delete",
            "ls && rm a",
        ] {
            assert!(!reads_only(cmd), "{cmd}");
        }
    }

    #[test]
    fn a_snapshot_never_walks_home_or_a_filesystem_root() {
        assert!(!snapshot_root_ok("/", "/home/v"));
        assert!(!snapshot_root_ok("/home/v", "/home/v"));
        assert!(!snapshot_root_ok("C:", "C:/Users/v"));
        assert!(!snapshot_root_ok("//?/C:/Users/V", "C:/Users/v"));
        assert!(!snapshot_root_ok("/home/v/.ssh", "/home/v"));
        assert!(!snapshot_root_ok("//?/C:/Users/V", "//?/C:/Users/v"));
        assert!(!snapshot_root_ok("//?/C:/Users/V/.ssh", "C:/Users/v"));
        assert!(snapshot_root_ok("/home/v/code/app", "/home/v"));
        assert!(snapshot_root_ok("//?/C:/Users/v/code", "C:/Users/v"));
    }
}
