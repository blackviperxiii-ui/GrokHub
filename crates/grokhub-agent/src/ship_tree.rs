//! What the review and the ship gates read from the person's project.
//!
//! Every call goes through [`project_git`]: the `GIT_*` variables the app
//! was started with are removed, replace refs and a grafts file are off, no
//! transport is allowed, and no hook and no fsmonitor runs. Nothing here
//! writes the project's index, refs or object store. A snapshot hashes the
//! working folder into an index and an object folder of its own, in the
//! app's config folder, and borrows the project's objects read-only. The
//! system's temp folder is not used: an agent's command may write there, and
//! could swap the index while a snapshot is taken.
//!
//! Git does not check that a stored file's bytes are the ones its id names
//! when it reads one for a diff. Every diff read here checks that first, so
//! a stored copy that was written over cannot show a line as old that the
//! change adds.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use sha1::Sha1;
use sha2::{Digest, Sha256};

use crate::work::{plain, project_git, random_base36, run, run_fed};

const NOT_A_REPO: &str = "this folder is not a git repository";
const NO_COMMITS: &str = "this repository has no commits";
/// What [`commits_since`] says when the base is not an ancestor of the tip.
pub const NOT_AN_ANCESTOR: &str = "the base commit is not in this history";
const BAD_PATCH: &str = "git printed a diff that could not be read";

/// The whole working folder as one tree id: staged, unstaged, untracked and
/// deleted files, without the ignored ones. The tree's objects live in a
/// folder of the snapshot's own that is removed when the snapshot is dropped.
#[derive(Debug)]
pub struct Snapshot {
    /// The repository's top level.
    pub top: PathBuf,
    /// The commit HEAD named when the snapshot was taken.
    pub head: String,
    pub tree: String,
    /// The project's object folder.
    objects: PathBuf,
    scratch: Scratch,
}

/// One changed path. `status` is `A`, `M`, `D` or `T`: a rename is a `D`
/// and an `A`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Change {
    pub path: String,
    pub status: char,
}

/// A line a change adds, with its number in the new file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AddedLine {
    pub path: String,
    pub line: u32,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Commit {
    pub sha: String,
    pub subject: String,
}

/// A snapshot's own index and object folder.
#[derive(Debug)]
struct Scratch(PathBuf);

/// How long a snapshot's folder may be left behind by a launch that ended
/// part way before the next snapshot removes it. No snapshot lives so long.
const SCRATCH_KEEP: Duration = Duration::from_secs(60 * 60);

/// `<config>/ship-scratch`, where each snapshot has a folder.
fn scratch_root() -> PathBuf {
    crate::perm::config_dir().join("ship-scratch")
}

/// Removes the snapshot folders under `root` that were made more than
/// `keep` ago.
fn sweep(root: &Path, keep: Duration) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let old = entry
            .metadata()
            .and_then(|meta| meta.modified())
            .ok()
            .and_then(|made| made.elapsed().ok())
            .is_some_and(|age| age >= keep);
        if old {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
}

impl Scratch {
    fn new() -> Result<Self, String> {
        let root = scratch_root();
        sweep(&root, SCRATCH_KEEP);
        let dir = root.join(random_base36(16));
        let failed = |err: std::io::Error| format!("{}: {err}", dir.display());
        std::fs::create_dir_all(&root).map_err(failed)?;
        private_dir(&dir).map_err(failed)?;
        let scratch = Self(dir.clone());
        // `read-tree` fails when the object folder is missing.
        std::fs::create_dir(scratch.objects()).map_err(failed)?;
        Ok(scratch)
    }

    fn index(&self) -> PathBuf {
        self.0.join("idx")
    }

    fn objects(&self) -> PathBuf {
        self.0.join("obj")
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A new folder only this user can read. An existing name is an error.
#[cfg(unix)]
fn private_dir(dir: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    std::fs::DirBuilder::new().mode(0o700).create(dir)
}

#[cfg(not(unix))]
fn private_dir(dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir(dir)
}

/// A full object id, so a value read from a record is never an option or a
/// revision expression.
fn is_id(text: &str) -> bool {
    matches!(text.len(), 40 | 64)
        && text
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn need_id(text: &str) -> Result<(), String> {
    if is_id(text) {
        return Ok(());
    }
    Err(format!("not a commit or tree id: {}", plain(text)))
}

/// Runs a project call that must exit 0.
fn git(top: &Path, args: &[&str]) -> Result<Vec<u8>, String> {
    let mut cmd = project_git(top);
    cmd.args(args);
    run(cmd)?.ok(args[0])
}

fn one_line(out: Vec<u8>, what: &str) -> Result<String, String> {
    String::from_utf8(out)
        .map(|text| text.trim().to_string())
        .map_err(|_| format!("git {what} printed text that is not valid UTF-8"))
}

fn path_text(bytes: &[u8]) -> Result<String, String> {
    std::str::from_utf8(bytes).map(str::to_string).map_err(|_| {
        format!(
            "a file name is not valid UTF-8: {}",
            plain(&String::from_utf8_lossy(bytes))
        )
    })
}

/// Whether two paths name one folder, by their real paths. A path that
/// cannot be resolved is no folder.
pub fn same_folder(one: &Path, other: &Path) -> bool {
    std::fs::canonicalize(one)
        .ok()
        .zip(std::fs::canonicalize(other).ok())
        .is_some_and(|(one, other)| one == other)
}

/// The repository top of `folder` and the commit HEAD names.
pub fn head(folder: &Path) -> Result<(PathBuf, String), String> {
    let mut cmd = project_git(folder);
    cmd.args(["rev-parse", "--path-format=absolute", "--show-toplevel"]);
    let out = run(cmd)?;
    if out.code != Some(0) {
        let stderr = out.stderr.trim();
        if stderr.is_empty() || stderr.contains("not a git repository") {
            return Err(NOT_A_REPO.into());
        }
        return Err(format!("git could not read this folder: {}", plain(stderr)));
    }
    let top = PathBuf::from(one_line(out.stdout, "rev-parse")?);
    let mut cmd = project_git(&top);
    cmd.args(["rev-parse", "--verify", "--quiet", "HEAD^{commit}"]);
    let out = run(cmd)?;
    if out.code != Some(0) {
        return Err(NO_COMMITS.into());
    }
    let sha = one_line(out.stdout, "rev-parse")?;
    need_id(&sha)?;
    Ok((top, sha))
}

/// What a symbolic ref points at, or `None` when `name` is not one (a
/// detached HEAD) or is not there.
fn symbolic(top: &Path, name: &str) -> Result<Option<String>, String> {
    let mut cmd = project_git(top);
    cmd.args(["symbolic-ref", "--quiet", name]);
    let out = run(cmd)?;
    match out.code {
        Some(0) => one_line(out.stdout, "symbolic-ref").map(Some),
        Some(1) => Ok(None),
        _ => Err(format!(
            "git symbolic-ref failed: {}",
            plain(out.stderr.trim())
        )),
    }
}

/// The branch HEAD is on, or `None` on a detached HEAD.
pub fn branch(top: &Path) -> Result<Option<String>, String> {
    Ok(
        symbolic(top, "HEAD")?
            .and_then(|full| full.strip_prefix("refs/heads/").map(str::to_string)),
    )
}

/// The default branch of `origin`, from the local `origin/HEAD` (no
/// network). `None` when it was never set.
pub fn default_branch(top: &Path) -> Result<Option<String>, String> {
    Ok(symbolic(top, "refs/remotes/origin/HEAD")?.and_then(|full| {
        full.strip_prefix("refs/remotes/origin/")
            .map(str::to_string)
    }))
}

/// The address of the remote `origin` as git would use it, or `None` when
/// the repository has no such remote.
pub fn origin(top: &Path) -> Result<Option<String>, String> {
    let mut cmd = project_git(top);
    cmd.args(["remote", "get-url", "origin"]);
    let out = run(cmd)?;
    match out.code {
        Some(0) => Ok(Some(one_line(out.stdout, "remote")?).filter(|url| !url.is_empty())),
        // What `git remote get-url` exits with for a remote that is not set.
        Some(2) => Ok(None),
        _ => Err(format!(
            "git remote get-url failed: {}",
            plain(out.stderr.trim())
        )),
    }
}

/// Who a commit made here would be by, and whether git would sign it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Author {
    /// `user.name`, when it is set to more than blanks.
    pub name: Option<String>,
    /// `user.email`, when it is set to more than blanks.
    pub email: Option<String>,
    /// `commit.gpgsign` is true.
    pub signs: bool,
}

/// One config value as git resolves it, or `None` when the key is not set.
fn config_value(top: &Path, args: &[&str], key: &str) -> Result<Option<String>, String> {
    let mut cmd = project_git(top);
    cmd.arg("config").args(args).args(["--get", key]);
    let out = run(cmd)?;
    match out.code {
        Some(0) => one_line(out.stdout, "config").map(Some),
        Some(1) => Ok(None),
        _ => Err(format!(
            "git config could not read {key}: {}",
            plain(out.stderr.trim())
        )),
    }
}

/// The author git would give a commit in this repository, from its config
/// and the person's own.
pub fn author(top: &Path) -> Result<Author, String> {
    let set =
        |key: &str| config_value(top, &[], key).map(|value| value.filter(|text| !text.is_empty()));
    Ok(Author {
        name: set("user.name")?,
        email: set("user.email")?,
        signs: config_value(top, &["--type=bool"], "commit.gpgsign")?.as_deref() == Some("true"),
    })
}

/// The config keys that name a program git would run, that send a push
/// somewhere else, or that allow a transport, when the repository's own
/// config sets them.
fn runs_a_program(key: &str) -> bool {
    let Some((section, rest)) = key.split_once('.') else {
        return false;
    };
    // A key with a subsection is `section.<anything>.name`.
    let (scoped, name) = match rest.rsplit_once('.') {
        Some((_, name)) => (true, name),
        None => (false, rest),
    };
    match (section, scoped) {
        ("filter", true) => matches!(name, "clean" | "smudge" | "process"),
        ("core", false) => matches!(
            name,
            "sshcommand" | "gitproxy" | "askpass" | "alternaterefscommand" | "worktree"
        ),
        ("credential", _) => name == "helper",
        ("http", _) => matches!(name, "proxy" | "sslverify"),
        ("remote", true) => matches!(name, "proxy" | "receivepack" | "uploadpack" | "mirror"),
        // `protocol.allow` and `protocol.<name>.allow`: with `ext` allowed, a
        // remote's address is a command line.
        ("protocol", _) => name == "allow",
        _ => false,
    }
}

/// How git reads a boolean: a key with no value is true, and so is every
/// value but the ones that say no.
fn is_true(value: Option<&[u8]>) -> bool {
    let Some(value) = value else {
        return true;
    };
    let value = String::from_utf8_lossy(value).trim().to_ascii_lowercase();
    !matches!(value.as_str(), "" | "false" | "no" | "off" | "0")
}

/// The first refused key in the output of
/// `git config --show-scope --includes -z --list`: a list of
/// `<scope> NUL <key> LF <value> NUL`. Only the repository's own scopes
/// count; the person's global and system config are theirs.
///
/// A sparse checkout is refused as well. With one, `add -A` passes over
/// every path the patterns leave out, so an edit to a tracked file there
/// would be in no snapshot, no review and no test record.
fn first_untrusted(listing: &[u8]) -> Option<String> {
    let mut fields = listing.split(|b| *b == 0);
    while let (Some(scope), Some(entry)) = (fields.next(), fields.next()) {
        if scope != b"local" && scope != b"worktree" {
            continue;
        }
        let mut parts = entry.splitn(2, |b| *b == b'\n');
        let key = String::from_utf8_lossy(parts.next().unwrap_or(entry));
        // Git prints the section and the name in lower case; a subsection
        // keeps its case, and no refused name is read from it.
        if runs_a_program(&key) || (key == "core.sparsecheckout" && is_true(parts.next())) {
            return Some(key.into_owned());
        }
    }
    None
}

/// The first key of the repository's own config (its `config`, a file that
/// one includes, or `config.worktree`) that would run a program or redirect
/// a push. `None` when there is none.
pub fn untrusted_config(top: &Path) -> Result<Option<String>, String> {
    let listing = git(
        top,
        &["config", "--show-scope", "--includes", "-z", "--list"],
    )?;
    Ok(first_untrusted(&listing))
}

fn refuse_untrusted(top: &Path) -> Result<(), String> {
    match untrusted_config(top)? {
        Some(key) => Err(format!(
            "this repository's own git config sets {}; commit from your terminal",
            plain(&key)
        )),
        None => Ok(()),
    }
}

/// `GIT_ALTERNATE_OBJECT_DIRECTORIES` holds a list. A folder whose path
/// holds the list's separator is written in quotes, which git reads as one
/// path.
fn as_alternate(objects: &Path) -> std::ffi::OsString {
    let separator = if cfg!(windows) { ';' } else { ':' };
    let text = objects.to_string_lossy();
    if !text.contains(separator) && !text.starts_with('"') {
        return objects.as_os_str().to_os_string();
    }
    let mut quoted = String::from("\"");
    for c in text.chars() {
        if c == '"' || c == '\\' {
            quoted.push('\\');
        }
        quoted.push(c);
    }
    quoted.push('"');
    quoted.into()
}

/// A project call that writes new objects into `scratch` and reads the
/// project's.
fn scratch_git(top: &Path, objects: &Path, scratch: &Scratch) -> Command {
    let mut cmd = project_git(top);
    cmd.env("GIT_OBJECT_DIRECTORY", scratch.objects())
        .env("GIT_ALTERNATE_OBJECT_DIRECTORIES", as_alternate(objects));
    cmd
}

/// Takes a snapshot of the repository `folder` is in. The repository is
/// only read: every file under its `.git` keeps its bytes.
pub fn snapshot(folder: &Path) -> Result<Snapshot, String> {
    let (top, head) = head(folder)?;
    refuse_untrusted(&top)?;
    let objects = PathBuf::from(one_line(
        git(
            &top,
            &[
                "rev-parse",
                "--path-format=absolute",
                "--git-path",
                "objects",
            ],
        )?,
        "rev-parse",
    )?);
    let scratch = Scratch::new()?;
    let index = |args: &[&str]| {
        let mut cmd = scratch_git(&top, &objects, &scratch);
        // A split index keeps its shared part in the repository's own git
        // folder, wherever the index file is.
        cmd.env("GIT_INDEX_FILE", scratch.index())
            .args(["-c", "core.splitIndex=false"])
            .args(args);
        run(cmd)?.ok(args[0])
    };
    index(&["read-tree", &head])?;
    // `add` runs clean filters, so the config is read again right before it.
    refuse_untrusted(&top)?;
    index(&["add", "-A"])?;
    let tree = one_line(index(&["write-tree"])?, "write-tree")?;
    need_id(&tree)?;
    Ok(Snapshot {
        top,
        head,
        tree,
        objects,
        scratch,
    })
}

/// The tree id of the working folder as it is now.
pub fn tree_id(folder: &Path) -> Result<String, String> {
    snapshot(folder).map(|snap| snap.tree)
}

/// The commits of `base..tip`, newest first. An error when `base` is not an
/// ancestor of `tip`. Replace refs and a grafts file are off, so these are
/// the commits a push of `tip` sends on top of `base`. Git checks a commit's
/// bytes against its id when it reads one, and a shallow mark on a commit
/// takes all its parents away, so `base` is then no ancestor.
pub fn commits_since(top: &Path, base: &str, tip: &str) -> Result<Vec<Commit>, String> {
    need_id(base)?;
    need_id(tip)?;
    let mut cmd = project_git(top);
    cmd.args(["merge-base", "--is-ancestor", base, tip]);
    let out = run(cmd)?;
    match out.code {
        Some(0) => {}
        Some(1) => return Err(NOT_AN_ANCESTOR.into()),
        _ => {
            return Err(format!(
                "git merge-base failed: {}",
                plain(out.stderr.trim())
            ))
        }
    }
    let range = format!("{base}..{tip}");
    let out = git(top, &["rev-list", "--format=%H%x09%s", &range])?;
    let text = String::from_utf8_lossy(&out);
    let mut commits = Vec::new();
    let mut lines = text.lines();
    // Two lines for each commit: git's own `commit <sha>`, then the format.
    while let Some(first) = lines.next() {
        let row = lines.next().and_then(|row| row.split_once('\t'));
        match row {
            Some((sha, subject)) if is_id(sha) && first.strip_prefix("commit ") == Some(sha) => {
                commits.push(Commit {
                    sha: sha.to_string(),
                    subject: subject.to_string(),
                });
            }
            _ => return Err("git printed a commit list that could not be read".into()),
        }
    }
    Ok(commits)
}

/// The changed paths that the project's index marks skip-worktree or
/// assume-unchanged. `git status` does not show such a file as changed, and
/// a snapshot includes it.
pub fn hidden_changed(top: &Path, changes: &[Change]) -> Result<Vec<String>, String> {
    let listing = git(top, &["ls-files", "-v", "-z"])?;
    let mut hidden = HashSet::new();
    for row in listing.split(|b| *b == 0) {
        // `<tag> <path>`: `S` is skip-worktree, a lower-case tag is
        // assume-unchanged.
        let [tag, b' ', path @ ..] = row else {
            continue;
        };
        if *tag == b'S' || tag.is_ascii_lowercase() {
            hidden.insert(path);
        }
    }
    Ok(changes
        .iter()
        .filter(|change| hidden.contains(change.path.as_bytes()))
        .map(|change| change.path.clone())
        .collect())
}

/// How every patch is asked for: no rename pairing, no program of the
/// repository's choosing, no colour, and the `a/` and `b/` prefixes whatever
/// the config says.
const PATCH: [&str; 9] = [
    "-r",
    "-p",
    "--no-renames",
    "--no-ext-diff",
    "--no-textconv",
    "--no-color",
    "--submodule=short",
    "--src-prefix=a/",
    "--dst-prefix=b/",
];

/// The ids of the files a `--raw --no-abbrev -z` diff reads, on both sides:
/// `:<mode> <mode> <id> <id> <status> NUL <path> NUL`. A side that is not
/// there has an id of zeros, and a submodule's id names a commit that is not
/// in this repository; neither is a file to read.
fn blob_ids(raw: &[u8]) -> Result<Vec<String>, String> {
    let mut ids = Vec::new();
    let mut seen = HashSet::new();
    let mut fields = raw.split(|b| *b == 0);
    while let Some(head) = fields.next() {
        if head.is_empty() {
            continue;
        }
        let row = std::str::from_utf8(head)
            .ok()
            .and_then(|head| head.strip_prefix(':'))
            .map(|head| head.split(' ').collect::<Vec<_>>());
        let path = fields.next().filter(|path| !path.is_empty());
        let (Some([old_mode, new_mode, old, new, _status]), Some(_)) = (row.as_deref(), path)
        else {
            return Err(BAD_PATCH.into());
        };
        for (mode, id) in [(old_mode, old), (new_mode, new)] {
            if !is_id(id) {
                return Err(BAD_PATCH.into());
            }
            let absent = id.bytes().all(|b| b == b'0');
            if !absent && *mode != "160000" && seen.insert(*id) {
                ids.push(id.to_string());
            }
        }
    }
    Ok(ids)
}

/// The id git gives a file with these bytes, in the hash `like` is written
/// in.
fn blob_id(like: &str, body: &[u8]) -> String {
    let head = format!("blob {}\0", body.len());
    let hex = |bytes: &[u8]| bytes.iter().map(|b| format!("{b:02x}")).collect();
    if like.len() == 40 {
        hex(&Sha1::new()
            .chain_update(&head)
            .chain_update(body)
            .finalize())
    } else {
        hex(&Sha256::new()
            .chain_update(&head)
            .chain_update(body)
            .finalize())
    }
}

/// Checks that each file a diff reads holds the bytes its id names. `cmd`
/// is the git call that can read them; `raw` is the diff's `--raw` form.
fn check_blobs(mut cmd: Command, raw: &[u8]) -> Result<(), String> {
    let ids = blob_ids(raw)?;
    if ids.is_empty() {
        return Ok(());
    }
    cmd.args(["cat-file", "--batch"]);
    let out = run_fed(cmd, (ids.join("\n") + "\n").as_bytes())?.ok("cat-file")?;
    let mut rest = &out[..];
    for id in &ids {
        let bad = || {
            format!(
                "a file stored in this repository does not match its id ({}); run git fsck",
                &id[..12]
            )
        };
        // `<id> blob <size> LF <bytes> LF`
        let end = rest.iter().position(|b| *b == b'\n').ok_or_else(bad)?;
        let head = String::from_utf8_lossy(&rest[..end]).into_owned();
        let size = head
            .strip_prefix(id.as_str())
            .and_then(|head| head.strip_prefix(" blob "))
            .and_then(|size| size.parse::<usize>().ok())
            .ok_or_else(bad)?;
        let body = rest.get(end + 1..end + 1 + size).ok_or_else(bad)?;
        if rest.get(end + 1 + size) != Some(&b'\n') || blob_id(id, body) != *id {
            return Err(bad());
        }
        rest = &rest[end + 2 + size..];
    }
    Ok(())
}

/// How a diff is asked for the ids of the files it reads.
const RAW: [&str; 5] = ["-r", "-z", "--no-renames", "--raw", "--no-abbrev"];

/// The file list of a `--name-status -z` diff: `<status> NUL <path> NUL`.
fn parse_changes(out: &[u8]) -> Result<Vec<Change>, String> {
    let mut changes = Vec::new();
    let mut fields = out.split(|b| *b == 0);
    while let Some(status) = fields.next() {
        if status.is_empty() {
            continue;
        }
        let (&[status], Some(path)) = (status, fields.next()) else {
            return Err("git printed a file list that could not be read".into());
        };
        changes.push(Change {
            path: path_text(path)?,
            status: char::from(status),
        });
    }
    Ok(changes)
}

/// The paths a `--numstat -z` diff counts no lines for: git treats them as
/// binary, so a patch shows no content for them.
fn parse_binary(out: &[u8]) -> Result<Vec<String>, String> {
    out.split(|b| *b == 0)
        .filter_map(|row| row.strip_prefix(b"-\t-\t"))
        .map(path_text)
        .collect()
}

/// Undoes git's quoting of a path: `"b/a\tb.txt"` without its quotes.
fn unquote(quoted: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let mut bytes = quoted.iter().copied();
    while let Some(byte) = bytes.next() {
        if byte != b'\\' {
            out.push(byte);
            continue;
        }
        let escaped = bytes.next()?;
        out.push(match escaped {
            b'a' => 0x07,
            b'b' => 0x08,
            b'f' => 0x0c,
            b'n' => b'\n',
            b'r' => b'\r',
            b't' => b'\t',
            b'v' => 0x0b,
            b'\\' | b'"' => escaped,
            b'0'..=b'3' => {
                let mut value = u32::from(escaped - b'0');
                for _ in 0..2 {
                    let digit = bytes.next().filter(|digit| (b'0'..=b'7').contains(digit))?;
                    value = value * 8 + u32::from(digit - b'0');
                }
                u8::try_from(value).ok()?
            }
            _ => return None,
        });
    }
    Some(out)
}

/// The path a `+++ ` line names, or `None` for a deleted file.
fn new_side(rest: &[u8]) -> Result<Option<String>, String> {
    if rest == b"/dev/null" {
        return Ok(None);
    }
    // Git ends a name that holds a space with a tab, after the closing
    // quote when there is one; a name that holds a tab is always quoted.
    let rest = rest.strip_suffix(b"\t").unwrap_or(rest);
    let name = match rest.strip_prefix(b"\"") {
        Some(quoted) => quoted.strip_suffix(b"\"").and_then(unquote),
        None => Some(rest.to_vec()),
    };
    let name = name.ok_or(BAD_PATCH)?;
    let path = name.strip_prefix(b"b/").ok_or(BAD_PATCH)?;
    path_text(path).map(Some)
}

/// `@@ -3,2 +5,4 @@`: how many old lines, the first new line, how many new.
/// `None` for a line that is not a hunk header.
fn hunk(line: &[u8]) -> Result<Option<(u32, u32, u32)>, String> {
    let Some(rest) = line.strip_prefix(b"@@ -") else {
        return Ok(None);
    };
    // The text after the second `@@` is file content and may be any bytes.
    let spans = rest
        .windows(3)
        .position(|window| window == b" @@")
        .and_then(|end| std::str::from_utf8(&rest[..end]).ok())
        .and_then(|spans| spans.split_once(" +"));
    let span = |side: &str| -> Option<(u32, u32)> {
        match side.split_once(',') {
            Some((start, count)) => Some((start.parse().ok()?, count.parse().ok()?)),
            None => Some((side.parse().ok()?, 1)),
        }
    };
    match spans.and_then(|(old, new)| span(old).zip(span(new))) {
        Some(((_, old_count), (new_start, new_count))) => {
            Ok(Some((old_count, new_start, new_count)))
        }
        None => Err(BAD_PATCH.into()),
    }
}

/// The added lines of a patch made with `--text -U0`. Each hunk is read by
/// the counts in its header, so a line of content is never taken for a
/// header and no added line is passed over.
fn parse_added(patch: &[u8]) -> Result<Vec<AddedLine>, String> {
    let mut added = Vec::new();
    let mut path: Option<String> = None;
    let mut lines = patch.split(|b| *b == b'\n');
    while let Some(line) = lines.next() {
        if line.starts_with(b"diff --git ") {
            path = None;
            continue;
        }
        if let Some(rest) = line.strip_prefix(b"+++ ") {
            path = new_side(rest)?;
            continue;
        }
        let Some((mut old_left, mut number, mut new_left)) = hunk(line)? else {
            // Between hunks git prints only headers. A line of content here
            // would be one the counts did not cover.
            let header =
                line.starts_with(b"--- ") || !matches!(line.first(), Some(b'+' | b'-' | b' '));
            if !header {
                return Err(BAD_PATCH.into());
            }
            continue;
        };
        while old_left > 0 || new_left > 0 {
            let line = lines.next().ok_or(BAD_PATCH)?;
            match line.split_first() {
                Some((b'-', _)) if old_left > 0 => old_left -= 1,
                Some((b'+', text)) if new_left > 0 => {
                    added.push(AddedLine {
                        path: path.clone().ok_or(BAD_PATCH)?,
                        line: number,
                        text: String::from_utf8_lossy(text).into_owned(),
                    });
                    number = number.saturating_add(1);
                    new_left -= 1;
                }
                // "\ No newline at end of file"
                Some((b'\\', _)) => {}
                _ => return Err(BAD_PATCH.into()),
            }
        }
    }
    Ok(added)
}

impl Snapshot {
    /// A diff call that reads the snapshot's tree.
    fn diff_tree(&self, options: &[&str], from: &str) -> Result<Vec<u8>, String> {
        need_id(from)?;
        let mut cmd = scratch_git(&self.top, &self.objects, &self.scratch);
        cmd.arg("diff-tree")
            .args(options)
            .args([from, self.tree.as_str()]);
        run(cmd)?.ok("diff-tree")
    }

    /// Checks the stored bytes of every file a diff from `from` reads.
    fn check(&self, from: &str) -> Result<(), String> {
        let raw = self.diff_tree(&RAW, from)?;
        check_blobs(scratch_git(&self.top, &self.objects, &self.scratch), &raw)
    }

    /// The paths that differ between the commit `from` and the snapshot,
    /// in path order.
    pub fn changes(&self, from: &str) -> Result<Vec<Change>, String> {
        parse_changes(&self.diff_tree(&["-r", "-z", "--no-renames", "--name-status"], from)?)
    }

    /// The diff from the commit `from` to the snapshot, for a reader: the
    /// text, cut after `cap` characters; whether it was cut; and the files
    /// git shows no content for.
    pub fn diff(&self, from: &str, cap: usize) -> Result<(String, bool, Vec<String>), String> {
        self.check(from)?;
        let not_shown =
            parse_binary(&self.diff_tree(&["-r", "-z", "--no-renames", "--numstat"], from)?)?;
        let patch = self.diff_tree(&PATCH, from)?;
        let mut text = String::from_utf8_lossy(&patch).into_owned();
        let cut = match text.char_indices().nth(cap) {
            Some((at, _)) => {
                text.truncate(at);
                true
            }
            None => false,
        };
        Ok((text, cut, not_shown))
    }

    /// Every line the snapshot adds to the commit `from`. A file git would
    /// call binary is read as text, so no attribute hides a line.
    pub fn added_lines(&self, from: &str) -> Result<Vec<AddedLine>, String> {
        self.check(from)?;
        let mut options = PATCH.to_vec();
        options.extend(["--text", "-U0"]);
        parse_added(&self.diff_tree(&options, from)?)
    }
}

/// What the commit `sha` changed and the lines it added, against each of
/// its parents; a first commit is read against nothing.
pub fn commit_added_lines(top: &Path, sha: &str) -> Result<(Vec<Change>, Vec<AddedLine>), String> {
    need_id(sha)?;
    let own = ["--root", "-m", "--no-commit-id"];
    let mut raw = vec!["diff-tree"];
    raw.extend(RAW);
    raw.extend(own);
    raw.push(sha);
    check_blobs(project_git(top), &git(top, &raw)?)?;
    let mut names = vec!["diff-tree", "-r", "-z", "--no-renames", "--name-status"];
    names.extend(own);
    names.push(sha);
    // A merge lists a path once for each parent it differs from.
    let mut seen = HashSet::new();
    let mut changes = parse_changes(&git(top, &names)?)?;
    changes.retain(|change| seen.insert((change.path.clone(), change.status)));
    let mut patch = vec!["diff-tree"];
    patch.extend(PATCH);
    patch.extend(["--text", "-U0"]);
    patch.extend(own);
    patch.push(sha);
    Ok((changes, parse_added(&git(top, &patch)?)?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::work::test_support::{git, git_repo, scratch};
    use std::collections::BTreeMap;

    fn write(path: &Path, bytes: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, bytes).unwrap();
    }

    /// A repository with one commit that holds `README` and `old.txt`.
    fn project(name: &str) -> PathBuf {
        let dir = scratch(name);
        git_repo(&dir);
        write(&dir.join("old.txt"), "old\n");
        git(&dir, &["add", "old.txt"]);
        git(&dir, &["commit", "-m", "add old"]);
        dir
    }

    fn head_of(dir: &Path) -> String {
        git(dir, &["rev-parse", "HEAD"]).trim().to_string()
    }

    /// Every file under `dir`, by its path from `dir`, with its bytes.
    fn files_under(dir: &Path) -> BTreeMap<String, Vec<u8>> {
        fn walk(dir: &Path, from: &Path, out: &mut BTreeMap<String, Vec<u8>>) {
            for entry in std::fs::read_dir(dir).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    walk(&path, from, out);
                } else {
                    let name = path.strip_prefix(from).unwrap().to_string_lossy();
                    out.insert(name.replace('\\', "/"), std::fs::read(&path).unwrap());
                }
            }
        }
        let mut out = BTreeMap::new();
        walk(dir, dir, &mut out);
        out
    }

    fn change(status: char, path: &str) -> Change {
        Change {
            path: path.to_string(),
            status,
        }
    }

    fn added(path: &str, line: u32, text: &str) -> AddedLine {
        AddedLine {
            path: path.to_string(),
            line,
            text: text.to_string(),
        }
    }

    // -- snapshot ------------------------------------------------------------

    #[test]
    fn a_snapshot_is_one_tree_id_of_the_whole_folder_and_leaves_git_untouched() {
        let dir = project("snap");
        write(&dir.join("README"), "hi\nthere\n");
        write(&dir.join("src/a.txt"), "one\n");
        write(&dir.join("staged.txt"), "staged\n");
        git(&dir, &["add", "staged.txt"]);
        std::fs::remove_file(dir.join("old.txt")).unwrap();
        write(&dir.join(".gitignore"), "target/\n");
        write(&dir.join("target/out.bin"), "built\n");
        let before = files_under(&dir.join(".git"));
        let head = head_of(&dir);

        let snap = snapshot(&dir).unwrap();
        assert_eq!(snap.tree, "98eeb2bb5cc20755bbf73147beabb5106d331219");
        assert_eq!(snap.head, head);
        assert_eq!(
            std::fs::canonicalize(&snap.top).unwrap(),
            std::fs::canonicalize(&dir).unwrap()
        );
        // Staged, unstaged, untracked and deleted; nothing that is ignored.
        assert_eq!(
            snap.changes(&head).unwrap(),
            [
                change('A', ".gitignore"),
                change('M', "README"),
                change('D', "old.txt"),
                change('A', "src/a.txt"),
                change('A', "staged.txt"),
            ]
        );
        assert_eq!(
            tree_id(&dir).unwrap(),
            "98eeb2bb5cc20755bbf73147beabb5106d331219"
        );
        assert_eq!(files_under(&dir.join(".git")), before);

        // The snapshot's own index and objects go when it is dropped.
        let own = snap.scratch.0.clone();
        assert!(own.join("idx").is_file());
        drop(snap);
        assert!(!own.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_edit_changes_the_tree_id_and_undoing_it_brings_the_first_id_back() {
        let dir = project("undo");
        let first = tree_id(&dir).unwrap();
        // Nothing changed: the tree is the commit's own.
        assert_eq!(first, git(&dir, &["rev-parse", "HEAD^{tree}"]).trim());
        write(&dir.join("README"), "hi\nedited\n");
        let edited = tree_id(&dir).unwrap();
        assert_ne!(edited, first);
        write(&dir.join("README"), "hi\n");
        assert_eq!(tree_id(&dir).unwrap(), first);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_snapshot_from_a_subfolder_covers_the_whole_repository() {
        let dir = project("subfolder");
        write(&dir.join("src/a.txt"), "one\n");
        write(&dir.join("top.txt"), "top\n");
        let snap = snapshot(&dir.join("src")).unwrap();
        assert_eq!(
            snap.changes(&snap.head).unwrap(),
            [change('A', "src/a.txt"), change('A', "top.txt")]
        );
        assert_eq!(snap.tree, tree_id(&dir).unwrap());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_linked_worktree_is_snapshotted_at_its_own_head() {
        let base = scratch("linked");
        let dir = base.join("my-app");
        std::fs::create_dir_all(&dir).unwrap();
        git_repo(&dir);
        let tree = base.join("second");
        git(
            &dir,
            &["worktree", "add", "--detach", tree.to_str().unwrap()],
        );
        write(&dir.join("only-main.txt"), "main\n");
        write(&tree.join("only-second.txt"), "second\n");
        let before = files_under(&dir.join(".git"));
        let snap = snapshot(&tree).unwrap();
        assert_eq!(
            std::fs::canonicalize(&snap.top).unwrap(),
            std::fs::canonicalize(&tree).unwrap()
        );
        assert_eq!(
            snap.changes(&snap.head).unwrap(),
            [change('A', "only-second.txt")]
        );
        assert_eq!(files_under(&dir.join(".git")), before);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn a_sparse_checkout_is_refused_by_name() {
        const REFUSED: &str =
            "this repository's own git config sets core.sparsecheckout; commit from your terminal";
        let dir = scratch("sparse");
        git_repo(&dir);
        write(&dir.join("kept/a.txt"), "a\n");
        write(&dir.join("tests/t.txt"), "assert works\n");
        git(&dir, &["add", "."]);
        git(&dir, &["commit", "-m", "two folders"]);
        let head = head_of(&dir);

        // The setting alone, with the patterns written by hand: every file
        // is still there, and an edit under `tests` would be passed over.
        git(&dir, &["config", "core.sparseCheckout", "true"]);
        write(&dir.join(".git/info/sparse-checkout"), "/*\n!/tests/\n");
        write(&dir.join("tests/t.txt"), "assert true\n");
        assert_eq!(tree_id(&dir), Err(REFUSED.to_string()));

        // Switched off, the edit is in the snapshot.
        git(&dir, &["config", "core.sparseCheckout", "false"]);
        let snap = snapshot(&dir).unwrap();
        assert_eq!(snap.changes(&head).unwrap(), [change('M', "tests/t.txt")]);
        drop(snap);

        // What `git sparse-checkout` sets up is refused the same way.
        git(&dir, &["checkout", "-q", "--", "tests/t.txt"]);
        git(&dir, &["sparse-checkout", "set", "kept"]);
        assert!(!dir.join("tests").exists());
        assert_eq!(tree_id(&dir), Err(REFUSED.to_string()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_snapshot_keeps_its_index_in_the_config_folder_and_takes_it_away() {
        let dir = project("scratch-place");
        let cfg = scratch("scratch-place-cfg");
        let _config = crate::perm::ConfigGuard::set(&cfg);
        let root = cfg.join("ship-scratch");
        let folders = || std::fs::read_dir(&root).map_or(0, Iterator::count);
        assert_eq!(folders(), 0);
        let snap = snapshot(&dir).unwrap();
        assert_eq!(folders(), 1);
        assert!(snap.scratch.index().starts_with(&root));
        assert!(snap.scratch.objects().starts_with(&root));
        drop(snap);
        assert_eq!(folders(), 0);

        // A folder left by a launch that ended part way goes once it is old.
        std::fs::create_dir_all(root.join("left-behind/obj")).unwrap();
        sweep(&root, SCRATCH_KEEP);
        assert_eq!(folders(), 1);
        sweep(&root, Duration::ZERO);
        assert_eq!(folders(), 0);
        let _ = std::fs::remove_dir_all(&cfg);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_split_index_setting_leaves_no_file_in_the_repository() {
        let dir = project("split-index");
        git(&dir, &["config", "core.splitIndex", "true"]);
        write(&dir.join("old.txt"), "old\nnew\n");
        let before = files_under(&dir.join(".git"));
        let snap = snapshot(&dir).unwrap();
        assert_eq!(snap.changes(&snap.head).unwrap(), [change('M', "old.txt")]);
        assert_eq!(files_under(&dir.join(".git")), before);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_folder_that_is_no_repository_or_has_no_commits_says_so() {
        let dir = scratch("norepo");
        assert_eq!(
            snapshot(&dir).map(|snap| snap.tree),
            Err("this folder is not a git repository".to_string())
        );
        assert_eq!(
            head(&dir),
            Err("this folder is not a git repository".to_string())
        );
        git(&dir, &["init"]);
        write(&dir.join("a.txt"), "a\n");
        assert_eq!(
            tree_id(&dir),
            Err("this repository has no commits".to_string())
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn head_is_the_repository_top_and_the_commit() {
        let dir = project("head");
        std::fs::create_dir_all(dir.join("deep/er")).unwrap();
        let (top, sha) = head(&dir.join("deep/er")).unwrap();
        assert_eq!(
            std::fs::canonicalize(top).unwrap(),
            std::fs::canonicalize(&dir).unwrap()
        );
        assert_eq!(sha, head_of(&dir));
        let _ = std::fs::remove_dir_all(&dir);
    }

    // -- the repository's own config -----------------------------------------

    #[test]
    fn only_the_repositorys_own_scopes_are_refused() {
        // The person's global large-file filter and the app's own `-c` pins.
        let theirs = b"system\0filter.lfs.clean\ngit-lfs clean -- %f\0\
            global\0filter.lfs.process\ngit-lfs filter-process\0\
            global\0core.sshcommand\nssh -i key\0\
            command\0core.hookspath\n/dev/null\0\
            local\0core.bare\nfalse\0\
            local\0remote.origin.url\nhttps://example.com/my-app.git\0\
            local\0filter.lfs.required\ntrue\0\
            local\0user.name\ntest\0";
        assert_eq!(first_untrusted(theirs), None);
        assert_eq!(first_untrusted(b""), None);
        // A sparse checkout that is switched off, and a promisor remote: no
        // transport is allowed, so the remote is never asked.
        let off = b"local\0core.sparsecheckout\nfalse\0\
            worktree\0core.sparsecheckout\n0\0\
            local\0remote.origin.promisor\ntrue\0\
            local\0extensions.partialclone\norigin\0";
        assert_eq!(first_untrusted(off), None);

        for (entry, key) in [
            ("filter.x.clean\ncat", "filter.x.clean"),
            ("filter.a.b.smudge\ncat", "filter.a.b.smudge"),
            ("filter.x.process\ncat", "filter.x.process"),
            // A key with no value.
            ("core.sshcommand", "core.sshcommand"),
            ("core.gitproxy\nproxy", "core.gitproxy"),
            ("core.askpass\nask", "core.askpass"),
            (
                "core.alternaterefscommand\nrefs",
                "core.alternaterefscommand",
            ),
            ("core.worktree\n/home/user/other", "core.worktree"),
            ("credential.helper\nstore", "credential.helper"),
            (
                "credential.https://example.com.helper\nstore",
                "credential.https://example.com.helper",
            ),
            ("http.proxy\nhttp://127.0.0.1:1", "http.proxy"),
            (
                "http.https://example.com/.proxy\nhttp://127.0.0.1:1",
                "http.https://example.com/.proxy",
            ),
            ("http.sslverify\nfalse", "http.sslverify"),
            (
                "http.https://example.com/.sslverify\nfalse",
                "http.https://example.com/.sslverify",
            ),
            (
                "remote.origin.proxy\nhttp://127.0.0.1:1",
                "remote.origin.proxy",
            ),
            (
                "remote.origin.receivepack\nrun",
                "remote.origin.receivepack",
            ),
            ("remote.origin.uploadpack\nrun", "remote.origin.uploadpack"),
            ("remote.origin.mirror\ntrue", "remote.origin.mirror"),
            ("protocol.allow\nalways", "protocol.allow"),
            ("protocol.ext.allow\nalways", "protocol.ext.allow"),
            ("core.sparsecheckout\ntrue", "core.sparsecheckout"),
            // A key with no value is true.
            ("core.sparsecheckout", "core.sparsecheckout"),
        ] {
            for scope in ["local", "worktree"] {
                let listing = format!("global\0user.name\ntest\0{scope}\0{entry}\0");
                assert_eq!(
                    first_untrusted(listing.as_bytes()).as_deref(),
                    Some(key),
                    "{scope} {entry}"
                );
            }
            let listing = format!("global\0{entry}\0");
            assert_eq!(first_untrusted(listing.as_bytes()), None, "global {entry}");
        }
    }

    /// A clean filter that would leave `marker` behind when it runs, on
    /// every file.
    fn filter_command(marker: &Path) -> String {
        format!(
            "touch '{}'; cat",
            marker.to_string_lossy().replace('\\', "/")
        )
    }

    #[test]
    fn a_filter_in_the_local_config_stops_the_snapshot_before_it_runs() {
        let dir = project("filter-local");
        let marker = dir.join("filter-ran");
        git(
            &dir,
            &["config", "filter.x.clean", &filter_command(&marker)],
        );
        write(&dir.join(".git/info/attributes"), "* filter=x\n");
        write(&dir.join("README"), "hi\nedited\n");
        assert_eq!(
            untrusted_config(&dir),
            Ok(Some("filter.x.clean".to_string()))
        );
        assert_eq!(
            snapshot(&dir).map(|snap| snap.tree),
            Err(
                "this repository's own git config sets filter.x.clean; commit from your terminal"
                    .to_string()
            )
        );
        assert!(!marker.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_filter_behind_an_include_stops_the_snapshot_before_it_runs() {
        let dir = project("filter-include");
        let marker = dir.join("filter-ran");
        let extra = dir.join(".git/extra.config");
        git(
            &dir,
            &[
                "config",
                "--file",
                extra.to_str().unwrap(),
                "filter.y.clean",
                &filter_command(&marker),
            ],
        );
        git(&dir, &["config", "include.path", "extra.config"]);
        write(&dir.join(".git/info/attributes"), "* filter=y\n");
        write(&dir.join("README"), "hi\nedited\n");
        assert_eq!(
            snapshot(&dir).map(|snap| snap.tree),
            Err(
                "this repository's own git config sets filter.y.clean; commit from your terminal"
                    .to_string()
            )
        );
        assert!(!marker.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_filter_in_the_worktree_config_stops_the_snapshot_before_it_runs() {
        let dir = project("filter-worktree");
        let marker = dir.join("filter-ran");
        git(&dir, &["config", "extensions.worktreeConfig", "true"]);
        git(
            &dir,
            &[
                "config",
                "--worktree",
                "filter.z.clean",
                &filter_command(&marker),
            ],
        );
        write(&dir.join(".git/info/attributes"), "* filter=z\n");
        write(&dir.join("README"), "hi\nedited\n");
        assert_eq!(
            snapshot(&dir).map(|snap| snap.tree),
            Err(
                "this repository's own git config sets filter.z.clean; commit from your terminal"
                    .to_string()
            )
        );
        assert!(!marker.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_local_ssh_program_is_refused_and_a_plain_repository_is_not() {
        let dir = project("ssh");
        assert_eq!(untrusted_config(&dir), Ok(None));
        git(
            &dir,
            &["config", "core.sshCommand", "ssh -i /home/user/key"],
        );
        assert_eq!(
            untrusted_config(&dir),
            Ok(Some("core.sshcommand".to_string()))
        );
        assert_eq!(
            tree_id(&dir),
            Err(
                "this repository's own git config sets core.sshcommand; commit from your terminal"
                    .to_string()
            )
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    // -- branch and author ---------------------------------------------------

    #[test]
    fn the_branch_is_read_by_name_and_a_detached_head_has_none() {
        let dir = project("branch");
        git(&dir, &["switch", "-c", "feature/one"]);
        assert_eq!(branch(&dir), Ok(Some("feature/one".to_string())));
        git(&dir, &["checkout", "--detach"]);
        assert_eq!(branch(&dir), Ok(None));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_default_branch_is_the_one_origin_head_names_and_none_when_unset() {
        let dir = project("default");
        assert_eq!(default_branch(&dir), Ok(None));
        let head = head_of(&dir);
        git(&dir, &["update-ref", "refs/remotes/origin/trunk", &head]);
        git(
            &dir,
            &[
                "symbolic-ref",
                "refs/remotes/origin/HEAD",
                "refs/remotes/origin/trunk",
            ],
        );
        assert_eq!(default_branch(&dir), Ok(Some("trunk".to_string())));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_author_is_the_configured_name_and_email_and_signing_is_seen() {
        let dir = project("author");
        assert_eq!(
            author(&dir),
            Ok(Author {
                name: Some("test".to_string()),
                email: Some("test@example.com".to_string()),
                signs: false,
            })
        );
        // An empty value in the repository's config hides the person's own.
        git(&dir, &["config", "user.name", ""]);
        git(&dir, &["config", "commit.gpgsign", "true"]);
        assert_eq!(
            author(&dir),
            Ok(Author {
                name: None,
                email: Some("test@example.com".to_string()),
                signs: true,
            })
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    // -- diffs ---------------------------------------------------------------

    #[test]
    fn a_rename_is_listed_as_a_delete_and_an_add() {
        let dir = project("rename");
        std::fs::rename(dir.join("old.txt"), dir.join("new.txt")).unwrap();
        let snap = snapshot(&dir).unwrap();
        assert_eq!(
            snap.changes(&snap.head).unwrap(),
            [change('A', "new.txt"), change('D', "old.txt")]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn added_lines_carry_their_path_and_number_whatever_the_diff_config_says() {
        let dir = project("added");
        git(&dir, &["config", "color.diff", "always"]);
        git(&dir, &["config", "color.ui", "always"]);
        git(&dir, &["config", "diff.noprefix", "true"]);
        git(&dir, &["config", "diff.mnemonicPrefix", "true"]);
        write(&dir.join("lines.txt"), "1\n2\n3\n4\n5\n6\n7\n8\n9\n");
        git(&dir, &["add", "lines.txt"]);
        git(&dir, &["commit", "-m", "lines"]);
        // Two edits far apart, a file with a space in its name, one that
        // does not end in a line break, and content shaped like headers.
        write(
            &dir.join("lines.txt"),
            "1\ntwo\n3\n4\n5\n6\n7\n8\n9\nten\neleven\n",
        );
        write(&dir.join("my notes.txt"), "a b\n");
        write(&dir.join("open.txt"), "no line break");
        write(
            &dir.join("tricky.txt"),
            "++ b/README\n@@ -1 +1,2 @@\n-- a/README\n+real\n",
        );
        std::fs::remove_file(dir.join("old.txt")).unwrap();
        let snap = snapshot(&dir).unwrap();
        assert_eq!(
            snap.added_lines(&snap.head).unwrap(),
            [
                added("lines.txt", 2, "two"),
                added("lines.txt", 10, "ten"),
                added("lines.txt", 11, "eleven"),
                added("my notes.txt", 1, "a b"),
                added("open.txt", 1, "no line break"),
                added("tricky.txt", 1, "++ b/README"),
                added("tricky.txt", 2, "@@ -1 +1,2 @@"),
                added("tricky.txt", 3, "-- a/README"),
                added("tricky.txt", 4, "+real"),
            ]
        );
        let (text, cut, not_shown) = snap.diff(&snap.head, 60_000).unwrap();
        assert!(!cut);
        assert_eq!(not_shown, Vec::<String>::new());
        assert!(!text.contains('\u{1b}'), "{text}");
        assert!(
            text.contains("--- a/lines.txt\n+++ b/lines.txt\n@@ -1,5 +1,5 @@\n 1\n-2\n+two\n 3\n"),
            "{text}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn a_file_name_git_writes_in_quotes_is_read_back_whole() {
        let dir = project("quoted");
        write(&dir.join("a\tb \"c\".txt"), "tab\n");
        write(&dir.join("\u{fc}ber.txt"), "umlaut\n");
        let snap = snapshot(&dir).unwrap();
        assert_eq!(
            snap.added_lines(&snap.head).unwrap(),
            [
                added("a\tb \"c\".txt", 1, "tab"),
                added("\u{fc}ber.txt", 1, "umlaut"),
            ]
        );
        assert_eq!(
            snap.changes(&snap.head).unwrap(),
            [change('A', "a\tb \"c\".txt"), change('A', "\u{fc}ber.txt")]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn quoted_names_and_hunk_headers_are_parsed_strictly() {
        assert_eq!(
            new_side(b"\"b/a\\tb \\\"c\\\" \\303\\274.txt\""),
            Ok(Some("a\tb \"c\" \u{fc}.txt".to_string()))
        );
        assert_eq!(
            new_side(b"b/my notes.txt\t"),
            Ok(Some("my notes.txt".to_string()))
        );
        assert_eq!(
            new_side(b"\"b/a\\tb.txt\"\t"),
            Ok(Some("a\tb.txt".to_string()))
        );
        assert_eq!(new_side(b"/dev/null"), Ok(None));
        assert_eq!(new_side(b"README"), Err(BAD_PATCH.to_string()));
        assert_eq!(new_side(b"\"b/a\\q.txt\""), Err(BAD_PATCH.to_string()));
        assert_eq!(hunk(b"@@ -3,2 +5,4 @@ fn main() {"), Ok(Some((2, 5, 4))));
        assert_eq!(hunk(b"@@ -0,0 +1 @@"), Ok(Some((0, 1, 1))));
        assert_eq!(hunk(b"@@ -7 +6,0 @@ \xff\xfe"), Ok(Some((1, 6, 0))));
        assert_eq!(hunk(b"index 0000000..b2901ea"), Ok(None));
        assert_eq!(hunk(b"@@ -x +1 @@"), Err(BAD_PATCH.to_string()));
        // A hunk shorter than its header says, and content outside a hunk.
        assert_eq!(
            parse_added(b"+++ b/a.txt\n@@ -0,0 +1,2 @@\n+one\n"),
            Err(BAD_PATCH.to_string())
        );
        assert_eq!(
            parse_added(b"+++ b/a.txt\n@@ -0,0 +1 @@\n+one\n+two\n"),
            Err(BAD_PATCH.to_string())
        );
        assert_eq!(
            parse_added(b"@@ -0,0 +1 @@\n+one\n"),
            Err(BAD_PATCH.to_string())
        );
        assert_eq!(
            parse_added(
                b"--- /dev/null\n+++ b/a.txt\n@@ -0,0 +1 @@\n+one\n\\ No newline at end of file\n"
            ),
            Ok(vec![added("a.txt", 1, "one")])
        );
    }

    #[test]
    fn an_attribute_that_hides_a_file_from_diffs_does_not_hide_its_lines_from_the_scan() {
        let dir = project("nodiff");
        write(&dir.join(".git/info/attributes"), "settings.txt -diff\n");
        write(
            &dir.join("settings.txt"),
            "name = my-app\nkey = sk-abcdefghijklmnopqrstuv\n",
        );
        let snap = snapshot(&dir).unwrap();
        assert_eq!(
            snap.added_lines(&snap.head).unwrap(),
            [
                added("settings.txt", 1, "name = my-app"),
                added("settings.txt", 2, "key = sk-abcdefghijklmnopqrstuv"),
            ]
        );
        // The reader's diff has no content for it, and says which file.
        let (text, cut, not_shown) = snap.diff(&snap.head, 60_000).unwrap();
        assert!(!cut);
        assert_eq!(not_shown, ["settings.txt"]);
        assert!(!text.contains("sk-abcdefghijklmnopqrstuv"), "{text}");
        assert!(
            text.contains("Binary files /dev/null and b/settings.txt differ\n"),
            "{text}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_long_diff_is_cut_at_the_cap_and_says_so() {
        let dir = project("cut");
        write(&dir.join("a.txt"), "\u{e9}\u{e9}\u{e9}\u{e9}\n");
        let snap = snapshot(&dir).unwrap();
        let (whole, cut, _) = snap.diff(&snap.head, 60_000).unwrap();
        assert!(!cut);
        assert!(
            whole.ends_with("@@ -0,0 +1 @@\n+\u{e9}\u{e9}\u{e9}\u{e9}\n"),
            "{whole}"
        );
        let count = whole.chars().count();
        // The cap counts characters, so a cut never splits one.
        let (text, cut, not_shown) = snap.diff(&snap.head, count - 3).unwrap();
        assert!(cut);
        assert_eq!(not_shown, Vec::<String>::new());
        assert_eq!(text.chars().count(), count - 3);
        assert!(text.ends_with("@@ -0,0 +1 @@\n+\u{e9}\u{e9}"), "{text}");
        assert!(!snap.diff(&snap.head, count).unwrap().1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_diff_takes_only_a_full_object_id() {
        let dir = project("ids");
        let snap = snapshot(&dir).unwrap();
        for bad in ["HEAD", "--output=/home/user/x", "abc123", ""] {
            assert_eq!(
                snap.changes(bad),
                Err(format!("not a commit or tree id: {bad}"))
            );
        }
        assert_eq!(
            commits_since(&dir, "HEAD~1", &snap.head),
            Err("not a commit or tree id: HEAD~1".to_string())
        );
        assert_eq!(
            commit_added_lines(&dir, "HEAD").map(|(changes, _)| changes),
            Err("not a commit or tree id: HEAD".to_string())
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    // -- files git status hides ------------------------------------------------

    #[test]
    fn a_changed_file_marked_skip_worktree_or_assume_unchanged_is_reported() {
        let dir = project("hidden");
        write(&dir.join("third.txt"), "third\n");
        git(&dir, &["add", "third.txt"]);
        git(&dir, &["commit", "-m", "third"]);
        git(&dir, &["update-index", "--skip-worktree", "README"]);
        git(&dir, &["update-index", "--assume-unchanged", "old.txt"]);
        write(&dir.join("README"), "hi\nhidden\n");
        write(&dir.join("old.txt"), "old\nhidden\n");
        write(&dir.join("third.txt"), "third\nseen\n");
        // git status shows only the plain one.
        assert_eq!(git(&dir, &["status", "--porcelain"]), " M third.txt\n");
        let snap = snapshot(&dir).unwrap();
        let changes = snap.changes(&snap.head).unwrap();
        assert_eq!(
            changes,
            [
                change('M', "README"),
                change('M', "old.txt"),
                change('M', "third.txt"),
            ]
        );
        assert_eq!(
            hidden_changed(&dir, &changes).unwrap(),
            ["README", "old.txt"]
        );
        assert_eq!(
            hidden_changed(&dir, &[change('M', "third.txt")]).unwrap(),
            Vec::<String>::new()
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    // -- commits ---------------------------------------------------------------

    #[test]
    fn commits_since_lists_the_newer_commits_and_refuses_a_base_off_the_history() {
        let dir = project("since");
        let base = head_of(&dir);
        assert_eq!(commits_since(&dir, &base, &base), Ok(Vec::new()));
        write(&dir.join("a.txt"), "a\n");
        git(&dir, &["add", "a.txt"]);
        git(&dir, &["commit", "-m", "add a\tfile", "-m", "A body line."]);
        let first = head_of(&dir);
        write(&dir.join("b.txt"), "b\n");
        git(&dir, &["add", "b.txt"]);
        git(
            &dir,
            &[
                "commit",
                "-m",
                "commit 0123456789012345678901234567890123456789",
            ],
        );
        let second = head_of(&dir);
        assert_eq!(
            commits_since(&dir, &base, &second),
            Ok(vec![
                Commit {
                    sha: second.clone(),
                    subject: "commit 0123456789012345678901234567890123456789".to_string(),
                },
                Commit {
                    sha: first.clone(),
                    subject: "add a\tfile".to_string(),
                },
            ])
        );

        // A commit beside the history: the base is not an ancestor of it.
        git(&dir, &["checkout", "-q", "--detach", &base]);
        write(&dir.join("c.txt"), "c\n");
        git(&dir, &["add", "c.txt"]);
        git(&dir, &["commit", "-m", "side"]);
        let side = head_of(&dir);
        assert_eq!(
            commits_since(&dir, &second, &side),
            Err("the base commit is not in this history".to_string())
        );
        assert_eq!(
            commits_since(&dir, &"0".repeat(40), &side)
                .map_err(|err| err.starts_with("git merge-base failed: ")),
            Err(true)
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_grafts_file_and_a_replace_ref_neither_hide_nor_add_a_commit() {
        let dir = project("grafts");
        let base = head_of(&dir);
        write(&dir.join(".env"), "KEY=sk-abcdefghijklmnopqrstuv\n");
        git(&dir, &["add", "-f", ".env"]);
        git(&dir, &["commit", "-m", "add env"]);
        let with_env = head_of(&dir);
        git(&dir, &["rm", "-q", ".env"]);
        git(&dir, &["commit", "-m", "tidy"]);
        let tidy = head_of(&dir);
        git(&dir, &["checkout", "-q", "--detach", &base]);
        write(&dir.join("s.txt"), "s\n");
        git(&dir, &["add", "s.txt"]);
        git(&dir, &["commit", "-m", "side"]);
        let side = head_of(&dir);
        let shas = |list: Vec<Commit>| list.into_iter().map(|c| c.sha).collect::<Vec<_>>();
        let both = vec![tidy.clone(), with_env.clone()];
        assert_eq!(
            commits_since(&dir, &base, &tidy).map(shas),
            Ok(both.clone())
        );

        // A replace ref that swaps the newest commit for the base is not
        // followed.
        git(&dir, &["replace", &tidy, &base]);
        assert_eq!(
            commits_since(&dir, &base, &tidy).map(shas),
            Ok(both.clone())
        );
        git(&dir, &["replace", "-d", &tidy]);

        // A grafts line that makes the base the newest commit's parent would
        // hide the commit that added the key, which a push still sends.
        write(&dir.join(".git/info/grafts"), &format!("{tidy} {base}\n"));
        assert_eq!(
            commits_since(&dir, &base, &tidy).map(shas),
            Ok(both.clone())
        );
        assert_eq!(
            commit_added_lines(&dir, &with_env),
            Ok((
                vec![change('A', ".env")],
                vec![added(".env", 1, "KEY=sk-abcdefghijklmnopqrstuv")]
            ))
        );
        assert_eq!(
            commit_added_lines(&dir, &tidy),
            Ok((vec![change('D', ".env")], vec![]))
        );

        // One that gives it a second parent adds nothing either: the file
        // is not read.
        write(
            &dir.join(".git/info/grafts"),
            &format!("{tidy} {with_env} {side}\n"),
        );
        assert_eq!(commits_since(&dir, &base, &tidy).map(shas), Ok(both));
        std::fs::remove_file(dir.join(".git/info/grafts")).unwrap();

        // A shallow mark takes a commit's parents away, so the base is no
        // longer in the history and the read is refused.
        write(&dir.join(".git/shallow"), &format!("{tidy}\n"));
        assert_eq!(
            commits_since(&dir, &base, &tidy),
            Err(NOT_AN_ANCESTOR.to_string())
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    // -- stored files ----------------------------------------------------------

    /// Writes `body` over the stored copy of the file `id`, as a program
    /// with the run of the git folder could: the copy keeps its name and no
    /// longer holds the bytes the name stands for.
    fn write_over(dir: &Path, id: &str, body: &str) {
        let scratch = dir.join(".git/forged.tmp");
        write(&scratch, body);
        let forged = git(
            dir,
            &["hash-object", "-w", "--no-filters", ".git/forged.tmp"],
        );
        std::fs::remove_file(&scratch).unwrap();
        let stored = |id: &str| dir.join(".git/objects").join(&id[..2]).join(&id[2..]);
        let target = stored(id);
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        // A stored file is read-only, so it is moved away, not written to.
        if target.exists() {
            std::fs::rename(&target, dir.join(".git/forged.old")).unwrap();
        }
        std::fs::copy(stored(forged.trim()), &target).unwrap();
    }

    const KEY_LINE: &str = "let key = \"sk-abcdefghijklmnopqrstuv\";";

    #[test]
    fn a_stored_file_written_over_cannot_show_an_added_line_as_old() {
        const REFUSED: &str =
            "a file stored in this repository does not match its id (f328e4d9d04c); run git fsck";
        let dir = scratch("forged-old");
        git_repo(&dir);
        write(&dir.join("app.rs"), "fn main() {}\n");
        git(&dir, &["add", "app.rs"]);
        git(&dir, &["commit", "-m", "app"]);
        let base = head_of(&dir);
        let with_key = format!("fn main() {{}}\n{KEY_LINE}\n");
        write(&dir.join("app.rs"), &format!("{with_key}fn extra() {{}}\n"));

        let snap = snapshot(&dir).unwrap();
        assert_eq!(
            snap.added_lines(&base),
            Ok(vec![
                added("app.rs", 2, KEY_LINE),
                added("app.rs", 3, "fn extra() {}"),
            ])
        );
        // The base's copy of the file now holds the key line already, so a
        // diff would show only the last line as added.
        write_over(&dir, "f328e4d9d04c31d0d70d16d21a07d1613be9d577", &with_key);
        assert_eq!(snap.added_lines(&base), Err(REFUSED.to_string()));
        assert_eq!(
            snap.diff(&base, 60_000).map(|(text, _, _)| text),
            Err(REFUSED.to_string())
        );
        // The file list reads no file.
        assert_eq!(snap.changes(&base).unwrap(), [change('M', "app.rs")]);
        drop(snap);

        // A commit of the change is read against the same stored copy.
        git(&dir, &["commit", "-a", "-m", "more"]);
        assert_eq!(
            commit_added_lines(&dir, &head_of(&dir)),
            Err(REFUSED.to_string())
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_stored_copy_planted_for_the_new_file_cannot_stand_in_for_it() {
        let dir = scratch("forged-new");
        git_repo(&dir);
        write(&dir.join("app.rs"), "fn main() {}\n");
        git(&dir, &["add", "app.rs"]);
        git(&dir, &["commit", "-m", "app"]);
        let base = head_of(&dir);
        // The id the changed file will have is already in the repository,
        // over other bytes, so `add` does not store the file again.
        write(
            &dir.join("app.rs"),
            &format!("fn main() {{}}\n{KEY_LINE}\n"),
        );
        write_over(
            &dir,
            "dbe9df1eaef173b420b39f47b2b7cf123104aa93",
            "fn main() {}\n// fine\n",
        );
        let snap = snapshot(&dir).unwrap();
        assert_eq!(
            snap.added_lines(&base),
            Err(
                "a file stored in this repository does not match its id (dbe9df1eaef1); \
                 run git fsck"
                    .to_string()
            )
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_ids_of_a_raw_diff_are_the_files_on_both_sides() {
        let one = "1".repeat(40);
        let two = "2".repeat(40);
        let zero = "0".repeat(40);
        let raw = format!(
            ":100644 100644 {one} {two} M\0a.txt\0\
             :000000 100644 {zero} {one} A\0b.txt\0\
             :100644 000000 {two} {zero} D\0c d.txt\0\
             :160000 160000 {one} {two} M\0sub\0\
             :000000 160000 {zero} {} A\0sub2\0",
            "3".repeat(40)
        );
        assert_eq!(blob_ids(raw.as_bytes()), Ok(vec![one.clone(), two.clone()]));
        assert_eq!(blob_ids(b""), Ok(vec![]));
        for bad in [
            format!(":100644 100644 {one} {two} M\0"),
            format!(":100644 100644 {one} HEAD M\0a.txt\0"),
            "a.txt\0".to_string(),
        ] {
            assert_eq!(
                blob_ids(bad.as_bytes()),
                Err(BAD_PATCH.to_string()),
                "{bad:?}"
            );
        }
        // The id of a file is the hash of its bytes behind git's header.
        assert_eq!(
            blob_id(&one, b"hi\n"),
            "45b983be36b73c0788dc9cbcb76cbb80fc7bb057"
        );
        assert_eq!(
            blob_id(&"1".repeat(64), b"hi\n"),
            "96c18f0297e38d01f4b2dacddea4259aea6b2961eb0822bd2c0c3f6029030045"
        );
    }

    /// A promisor remote whose address is a command line that makes `marker`.
    #[cfg(unix)]
    fn promisor(dir: &Path, marker: &Path) {
        git(dir, &["config", "extensions.partialClone", "far"]);
        git(dir, &["config", "remote.far.promisor", "true"]);
        let url = format!("ext::sh -c touch% {}", marker.display());
        git(dir, &["config", "remote.far.url", &url]);
        git(dir, &["config", "protocol.ext.allow", "always"]);
    }

    #[cfg(unix)]
    #[test]
    fn a_remote_named_in_the_repositorys_config_is_never_asked_for_a_missing_file() {
        let dir = project("promisor");
        let base = head_of(&dir);
        let marker = dir.join("ran.marker");
        write(&dir.join(".gitignore"), "ran.marker\n");
        write(&dir.join("old.txt"), "old\nnew\n");
        let missing = dir.join(".git/objects/33/67afdbbf91e638efe983616377c60477cc6612");

        // Written after the snapshot's own check: the calls themselves allow
        // no transport, so git cannot fetch the file it misses.
        let snap = snapshot(&dir).unwrap();
        promisor(&dir, &marker);
        std::fs::remove_file(&missing).unwrap();
        assert!(snap.diff(&base, 60_000).is_err());
        assert!(snap.added_lines(&base).is_err());
        assert!(commit_added_lines(&dir, &base).is_err());
        assert!(!marker.exists());
        drop(snap);

        // There before the snapshot: refused by the key's name.
        assert_eq!(
            tree_id(&dir),
            Err(
                "this repository's own git config sets protocol.ext.allow; commit from your \
                 terminal"
                    .to_string()
            )
        );
        assert!(!marker.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_origin_address_is_read_and_a_repository_with_no_origin_has_none() {
        let dir = project("origin");
        assert_eq!(origin(&dir), Ok(None));
        git(
            &dir,
            &["remote", "add", "origin", "https://example.com/my-app.git"],
        );
        assert_eq!(
            origin(&dir),
            Ok(Some("https://example.com/my-app.git".to_string()))
        );
        // The address git would use, after the repository's own rewrites.
        git(
            &dir,
            &[
                "config",
                "url.https://example.org/.insteadOf",
                "https://example.com/",
            ],
        );
        assert_eq!(
            origin(&dir),
            Ok(Some("https://example.org/my-app.git".to_string()))
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_commits_added_lines_are_read_against_its_parents() {
        let dir = scratch("commit-lines");
        git_repo(&dir);
        let root = head_of(&dir);
        // The first commit is read against nothing.
        assert_eq!(
            commit_added_lines(&dir, &root),
            Ok((vec![change('A', "README")], vec![added("README", 1, "hi")]))
        );

        // A secret added in one commit and removed in the next is still in
        // the first one.
        write(&dir.join(".env"), "KEY=sk-abcdefghijklmnopqrstuv\n");
        write(&dir.join("README"), "hi\nmore\n");
        git(&dir, &["add", "-f", ".env", "README"]);
        git(&dir, &["commit", "-m", "add env"]);
        let with_env = head_of(&dir);
        git(&dir, &["rm", "-q", ".env"]);
        git(&dir, &["commit", "-m", "drop env"]);
        let without = head_of(&dir);
        assert_eq!(
            commit_added_lines(&dir, &with_env),
            Ok((
                vec![change('A', ".env"), change('M', "README")],
                vec![
                    added(".env", 1, "KEY=sk-abcdefghijklmnopqrstuv"),
                    added("README", 2, "more"),
                ]
            ))
        );
        assert_eq!(
            commit_added_lines(&dir, &without),
            Ok((vec![change('D', ".env")], Vec::new()))
        );

        // A merge is read against each parent, and a path is listed once.
        git(&dir, &["checkout", "-q", "-b", "side", &with_env]);
        write(&dir.join("side.txt"), "side\n");
        git(&dir, &["add", "side.txt"]);
        git(&dir, &["commit", "-m", "side"]);
        git(&dir, &["merge", "-q", "--no-ff", "-m", "merge", &without]);
        let merge = head_of(&dir);
        assert_eq!(
            commit_added_lines(&dir, &merge),
            Ok((
                vec![change('D', ".env"), change('A', "side.txt")],
                vec![added("side.txt", 1, "side")]
            ))
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    // -- the object folder list ------------------------------------------------

    #[test]
    fn an_object_folder_whose_path_holds_the_list_separator_is_quoted() {
        assert_eq!(
            as_alternate(Path::new("/home/user/my-app/.git/objects")),
            "/home/user/my-app/.git/objects"
        );
        let odd = if cfg!(windows) {
            "C:/Users/user/my;app/.git/objects"
        } else {
            "/home/user/my:app/.git/objects"
        };
        assert_eq!(as_alternate(Path::new(odd)), format!("\"{odd}\"").as_str());
        assert_eq!(
            as_alternate(Path::new("\"odd\\name:x")),
            "\"\\\"odd\\\\name:x\""
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_project_in_a_folder_with_a_colon_in_its_name_is_snapshotted() {
        let base = scratch("colon");
        let dir = base.join("my:app");
        std::fs::create_dir_all(&dir).unwrap();
        git_repo(&dir);
        write(&dir.join("a.txt"), "a\n");
        let snap = snapshot(&dir).unwrap();
        assert_eq!(snap.changes(&snap.head).unwrap(), [change('A', "a.txt")]);
        let _ = std::fs::remove_dir_all(&base);
    }
}
