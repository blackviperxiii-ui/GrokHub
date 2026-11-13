//! What stands between a session's change and a commit: the checks the Tasks
//! panel lists, and the commit the person makes there. Reading the checks
//! changes nothing, and every file under the project's `.git` keeps its
//! bytes. [`commit_with`] runs every check again and commits only after the
//! person's click, with the exact text the confirm showed.
//!
//! The change is named by three ids: the commit the session began at, the
//! commit HEAD names now, and one tree id of the whole working folder. The
//! review and the last test run each hold the ids they saw; a check passes
//! only while they are the ids of the change that is there now. Those records
//! live in the app's config folder. Nothing under the project's `.git` is
//! trusted to say what was reviewed or tested.
//!
//! Every git call made here is one of `grokhub_agent::ship_tree`'s. The
//! project's test record, its secrets and the branches the app made are
//! found by a key made from the address of `origin`. That address is the one
//! the session began with, kept with its starting commit, so a change to the
//! repository's config cannot point the checks at another project's
//! records; an address that changed since then is in the way of a commit.

use crate::activity::{self as code_activity, Entry};
use crate::runs::{self, Kind, LastRun};
use crate::secrets::{self, Vault};
use crate::ship::{self, Blocked, CodeShip};
use grokhub_agent::review::{self, AllowedLine, Base, Review, Shipped};
use grokhub_agent::ship_tree::{self, same_folder, AddedLine, Change, Commit, Snapshot};
use grokhub_core::redact::{holds_secret, redact_held_secrets};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::Path;

/// The most commits a session may have made for the change to ship here.
const COMMITS_MAX: usize = 200;

const NO_BASE: &str = "This session's starting commit is not on record.";
const OTHER_FOLDER: &str = "This session began in another project folder.";
const HISTORY: &str =
    "The branch history changed since this session began; ship from your terminal.";
const TOO_MANY: &str =
    "More than 200 commits were made during this session; ship from your terminal.";
const ORIGIN_CHANGED: &str =
    "This project's origin address changed since this session began; ship from your terminal.";
const NO_DEFAULT: &str = "This project's default branch is not known. Use New branch.";
const NOTHING: &str = "There is nothing to commit.";
const NOT_REVIEWED: &str = "The change has not been reviewed.";
const REVIEW_STALE: &str = "Files or commits changed since the review. Review again.";
const TESTS_NOT_RUN: &str = "The tests have not run on this change.";
const TESTS_STALE: &str = "Files changed since the tests ran. Run them again.";
const NO_AUTHOR: &str = "Set user.name and user.email in git before you commit.";
const SIGNED: &str =
    "This repository signs its commits, which the app cannot do; commit from your terminal.";
const NO_MESSAGE: &str = "Write a commit message.";
const LONG_SUBJECT: &str = "The first line of the commit message is longer than 200 characters.";
const NUL_MESSAGE: &str = "The commit message holds a NUL character.";
/// The commit was asked for without the person's yes.
pub const NOT_CONFIRMED: &str = "Confirm the commit first.";
/// The confirm showed another change than the one there now.
pub const CONFIRM_STALE: &str = "The change is not the one the confirm showed. Look again.";

/// The longest first line a commit message may have.
const SUBJECT_MAX: usize = 200;
/// How many changed files and session commits the page lists.
const LIST_MAX: usize = 200;
/// The message the Commit card starts with.
pub const DEFAULT_MESSAGE: &str = "Change made in this session";
/// What the Commit card always says about the commit.
pub const NOTICES: [&str; 2] = [
    "Every changed file in this folder is included.",
    "Hooks are not run, and the commit is not signed.",
];

/// How the session's review stands against the change that is there now.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum ReviewCheck {
    /// No review, or one that cannot be read.
    None,
    /// The review read another change: a file, HEAD or a commit differs.
    Stale,
    /// The review is of this change and found this many blockers.
    Blockers {
        count: u32,
    },
    Clean,
}

/// How the project's tests stand against the change that is there now.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum TestsCheck {
    /// No test command is remembered and none is detected. This does not
    /// block.
    NoCommand,
    NotRun {
        command: String,
    },
    /// The last run was of other files.
    Stale {
        command: String,
    },
    Failed {
        command: String,
        exit: i32,
    },
    Passed {
        command: String,
    },
}

/// Why the scan flagged a place.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SecretKind {
    /// The file's name says it holds secrets (`.env`, `id_rsa`, `*.pem`).
    Name,
    /// The line holds one of this project's secrets from the keychain.
    Held,
    /// The line has the shape of a key, a token or a password.
    Shape,
}

/// One place the secret scan flagged. It never carries the line's text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SecretHit {
    pub path: String,
    /// The line in the new file; none for a file flagged by its name.
    pub line: Option<u32>,
    pub kind: SecretKind,
    /// The session commit that added it; none for the uncommitted change.
    pub commit: Option<String>,
    /// The person allowed this line: a shape hit only.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub allowed: bool,
}

impl SecretHit {
    /// `path:line`, as the page names the place.
    pub fn place(&self) -> String {
        match self.line {
            Some(line) => format!("{}:{line}", self.path),
            None => self.path.clone(),
        }
    }
}

/// How the page allows a flagged line of the uncommitted change:
/// `path:line@<tree id>`. The tree id names every file's content, so an
/// Allow given for one line holds only while the folder is what the person
/// looked at: another value written on that line later is flagged again.
pub fn allow_key(place: &str, tree: &str) -> String {
    format!("{place}@{tree}")
}

/// One changed file: `A`, `M`, `D` or `T`, and its path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileRow {
    pub status: String,
    pub path: String,
}

/// One commit made during the session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommitRow {
    pub sha: String,
    pub subject: String,
}

/// What a commit of the change would be.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommitPlan {
    /// The working folder's tree id: what the commit holds.
    pub tree: String,
    /// The commit HEAD names: the commit's parent.
    pub head: String,
    /// Every changed file, the first [`LIST_MAX`] of them.
    pub files: Vec<FileRow>,
    /// How many changed files are not listed.
    pub more: usize,
    /// The commits made since the session began, newest first, the first
    /// [`LIST_MAX`]; a push sends them with this one.
    pub commits: Vec<CommitRow>,
    /// `name <email>`, when both are set.
    pub author: Option<String>,
    /// The message the confirm text is for.
    pub message: String,
    pub notices: Vec<String>,
    /// How many review blockers the person waived.
    pub waived: u32,
    /// How many flagged lines the person allowed.
    pub allowed: u32,
    /// The exact text the person confirms; none while anything is in the
    /// way.
    pub confirm: Option<String>,
}

/// What the person set on the Commit card.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Draft {
    /// The commit message; none for [`DEFAULT_MESSAGE`].
    #[serde(default)]
    pub message: Option<String>,
    /// The shape hits the person allowed, each as [`allow_key`] makes it.
    #[serde(default)]
    pub allowed: Vec<String>,
}

/// A commit [`commit_with`] made.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Committed {
    pub sha: String,
    /// What the page says: "Committed 0123abcd to work/one.", and what is
    /// left to do when a step after the ref update failed.
    pub said: String,
    /// The checks after the commit.
    pub status: ShipStatus,
}

/// The checks on a change that could be read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Checks {
    /// The branch HEAD is on; none on a detached HEAD.
    pub branch: Option<String>,
    /// The branch takes no commit from the app (the default branch, a
    /// protected one, a detached HEAD, or no default branch on record), so
    /// the page offers New branch.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub new_branch: bool,
    pub review: ReviewCheck,
    pub tests: TestsCheck,
    /// What the scan flagged, in the uncommitted change first; none when
    /// the scan could not run.
    pub secrets: Option<Vec<SecretHit>>,
}

/// Whether the session's change could be committed, and what is in the way.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShipStatus {
    /// One sentence for each thing in the way. Empty when nothing is.
    pub blocked: Vec<String>,
    /// None when the project could not be read at all.
    pub checks: Option<Checks>,
    /// What a commit would be; none when the project could not be read.
    pub commit: Option<CommitPlan>,
    /// The commit made from the Tasks panel, while HEAD is that commit and
    /// nothing is left to commit.
    pub shipped: Option<String>,
}

impl ShipStatus {
    fn unread(why: String) -> Self {
        Self {
            blocked: vec![why],
            checks: None,
            commit: None,
            shipped: None,
        }
    }
}

/// A `ship_tree` error as a sentence of its own.
pub(crate) fn unread(err: &str) -> String {
    let mut chars = err.chars();
    match chars.next() {
        Some(first) if err.starts_with("this ") => {
            format!("{}{}.", first.to_uppercase(), chars.as_str())
        }
        _ => format!("The project could not be read: {err}"),
    }
}

fn counted(count: usize, noun: &str) -> String {
    if count == 1 {
        format!("1 {noun}")
    } else {
        format!("{count} {noun}s")
    }
}

/// Where the session began and the commits made since, newest first. The
/// error is the sentence that says why they are not known.
fn session_commits(session: &str, snap: &Snapshot) -> Result<(Base, Vec<Commit>), String> {
    let base = review::base_for(session)
        .map_err(|err| format!("This session's starting commit could not be read: {err}"))?
        .ok_or(NO_BASE)?;
    if !same_folder(Path::new(&base.project), &snap.top) {
        return Err(OTHER_FOLDER.into());
    }
    let commits = ship_tree::commits_since(&snap.top, &base.head, &snap.head).map_err(|err| {
        if err == ship_tree::NOT_AN_ANCESTOR {
            HISTORY.to_string()
        } else {
            unread(&err)
        }
    })?;
    Ok((base, commits))
}

/// Why a branch takes no commit, in the words of a commit.
fn no_commit(why: Blocked, branch: &str) -> String {
    match why {
        Blocked::DefaultBranch => format!(
            "{branch} is the main or default branch. The app never commits to it; make a branch \
             first."
        ),
        Blocked::Protected => {
            format!("{branch} is protected in Settings → Code, so the app doesn't commit to it.")
        }
        Blocked::Detached => {
            "Check out a branch first; the app doesn't commit on a detached HEAD.".into()
        }
        Blocked::BadBranchName => format!("\"{branch}\" isn't a branch name the app commits to."),
        // The rest are about a push, and are said as one.
        other => other.message(branch),
    }
}

/// Why this branch takes no commit from the app, if it takes none.
fn branch_block(
    config_dir: &Path,
    cfg: &CodeShip,
    top: &Path,
    repo_key: &str,
    branch: &str,
) -> Result<(), String> {
    // `main` is refused by name, and so is the other usual default.
    let default = ship_tree::default_branch(top).map_err(|err| unread(&err))?;
    if branch == "master" {
        return Err(no_commit(Blocked::DefaultBranch, branch));
    }
    ship::protected(cfg, branch, default.as_deref()).map_err(|why| no_commit(why, branch))?;
    // With no default branch on record, only a branch the app made is known
    // not to be it.
    if default.is_none() && !ship::app_made(config_dir, repo_key, branch) {
        return Err(NO_DEFAULT.into());
    }
    Ok(())
}

/// The commit the Tasks panel made, while it is HEAD and holds the tree that
/// is there now.
fn shipped_now<'a>(shipped: Option<&'a Shipped>, snap: &Snapshot) -> Option<&'a Shipped> {
    shipped.filter(|shipped| {
        same_folder(Path::new(&shipped.project), &snap.top)
            && shipped.sha == snap.head
            && shipped.tree == snap.tree
    })
}

/// How the review stands, and the review when it is of the change there now:
/// the same files, HEAD and session commits, or the commit the Tasks panel
/// made from exactly that.
fn review_check(
    session: &str,
    snap: &Snapshot,
    since_base: Option<&(Base, Vec<Commit>)>,
    shipped: Option<&Shipped>,
    blocked: &mut Vec<String>,
) -> (ReviewCheck, Option<Review>) {
    let review = match review::review_for(session) {
        Ok(Some(review)) => review,
        Ok(None) => {
            blocked.push(NOT_REVIEWED.into());
            return (ReviewCheck::None, None);
        }
        Err(err) => {
            blocked.push(format!("The review can't be read: {err}"));
            return (ReviewCheck::None, None);
        }
    };
    let fresh = since_base.is_some_and(|(base, commits)| {
        let shas = commits.iter().map(|c| &c.sha);
        let same = same_folder(Path::new(&review.project), &snap.top) && review.base == base.head;
        let now = review.head == snap.head
            && review.tree == snap.tree
            && review.commits.iter().eq(shas.clone());
        // After the commit: its parent is the reviewed HEAD, its tree the
        // reviewed tree, and it is the one commit added.
        let committed = shipped_now(shipped, snap).is_some_and(|shipped| {
            review.head == shipped.parent
                && review.tree == shipped.tree
                && std::iter::once(&shipped.sha)
                    .chain(review.commits.iter())
                    .eq(shas)
        });
        same && (now || committed)
    });
    if !fresh {
        blocked.push(REVIEW_STALE.into());
        return (ReviewCheck::Stale, None);
    }
    let open = review.open_blockers();
    if open > 0 {
        blocked.push(format!(
            "The review found {}.",
            counted(open as usize, "blocker")
        ));
        return (ReviewCheck::Blockers { count: open }, Some(review));
    }
    (ReviewCheck::Clean, Some(review))
}

fn tests_check(
    config_dir: &Path,
    folder: &Path,
    repo_key: &str,
    tree: &str,
    blocked: &mut Vec<String>,
) -> TestsCheck {
    let last: Option<LastRun> = runs::last_run(config_dir, repo_key);
    if let Some(last) = last.as_ref().filter(|last| last.tree == tree) {
        let command = last.command.clone();
        if last.exit == 0 {
            return TestsCheck::Passed { command };
        }
        blocked.push(format!("The tests failed (exit {}).", last.exit));
        return TestsCheck::Failed {
            command,
            exit: last.exit,
        };
    }
    let planned = runs::remembered(config_dir, Kind::Test, repo_key)
        .ok()
        .flatten()
        .unwrap_or_else(|| runs::detect_test_command(folder));
    match last {
        Some(last) => {
            blocked.push(TESTS_STALE.into());
            TestsCheck::Stale {
                command: if planned.is_empty() {
                    last.command
                } else {
                    planned
                },
            }
        }
        None if planned.is_empty() => TestsCheck::NoCommand,
        None => {
            blocked.push(TESTS_NOT_RUN.into());
            TestsCheck::NotRun { command: planned }
        }
    }
}

/// A file whose name says it holds secrets: `.env*` except `.env.example`,
/// `auth.json`, `secrets.json`, `*.pem`, `*.key`, `id_rsa*`, `id_ed25519*`.
fn secret_name(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path).to_ascii_lowercase();
    (name.starts_with(".env") && name != ".env.example")
        || name == "auth.json"
        || name == "secrets.json"
        || name.ends_with(".pem")
        || name.ends_with(".key")
        || name.starts_with("id_rsa")
        || name.starts_with("id_ed25519")
}

/// Flags the files of `changes` that are secret by name, and the lines of
/// `added` that hold one of `hidden` or have a secret's shape. A file
/// flagged by name is listed once, without its lines.
fn scan(
    changes: &[Change],
    added: &[AddedLine],
    hidden: &[String],
    commit: Option<&str>,
    hits: &mut Vec<SecretHit>,
) {
    let named: HashSet<&str> = changes
        .iter()
        .filter(|change| change.status != 'D' && secret_name(&change.path))
        .map(|change| change.path.as_str())
        .collect();
    let mut hit = |path: &str, line: Option<u32>, kind: SecretKind| {
        hits.push(SecretHit {
            path: path.to_string(),
            line,
            kind,
            commit: commit.map(str::to_string),
            allowed: false,
        });
    };
    for change in changes {
        if named.contains(change.path.as_str()) {
            hit(&change.path, None, SecretKind::Name);
        }
    }
    for line in added {
        if named.contains(line.path.as_str()) {
            continue;
        }
        if redact_held_secrets(&line.text, hidden) != line.text {
            hit(&line.path, Some(line.line), SecretKind::Held);
        } else if holds_secret(&line.text) {
            hit(&line.path, Some(line.line), SecretKind::Shape);
        }
    }
}

/// Scans the uncommitted change and each session commit against its
/// parents, so a secret one commit added and the next removed is found.
fn scan_change(
    snap: &Snapshot,
    changes: &[Change],
    commits: &[Commit],
    hidden: &[String],
) -> Result<Vec<SecretHit>, String> {
    let mut hits = Vec::new();
    let added = snap.added_lines(&snap.head)?;
    scan(changes, &added, hidden, None, &mut hits);
    for commit in commits {
        let (changes, added) = ship_tree::commit_added_lines(&snap.top, &commit.sha)?;
        scan(&changes, &added, hidden, Some(&commit.sha), &mut hits);
    }
    Ok(hits)
}

/// Marks the shape hits the person allowed: in the uncommitted change, the
/// keys of `allowed` for the tree `tree`; in a commit the Tasks panel made,
/// the places it was made with, which that commit's content still holds. A
/// file flagged by its name, a held secret and a shape hit in any other
/// commit are never allowed.
fn allow(hits: &mut [SecretHit], allowed: &[String], tree: &str, shipped: Option<&Shipped>) {
    for hit in hits.iter_mut().filter(|hit| hit.kind == SecretKind::Shape) {
        let place = hit.place();
        hit.allowed = match &hit.commit {
            None => allowed.contains(&allow_key(&place, tree)),
            Some(sha) => shipped.is_some_and(|shipped| {
                shipped
                    .allowed
                    .iter()
                    .any(|line| line.commit == *sha && line.place == place)
            }),
        };
    }
}

/// The message as it is committed: each line without trailing blanks, the
/// first without leading ones either, and no blank lines at the end. The
/// confirm's subject is its first line, exactly.
fn tidy(message: &str) -> String {
    let mut lines = message.lines().map(str::trim_end);
    let first = lines.next().unwrap_or("").trim_start();
    let mut out = std::iter::once(first)
        .chain(lines)
        .collect::<Vec<_>>()
        .join("\n");
    out.truncate(out.trim_end().len());
    out
}

/// Why `message` cannot be a commit message, if it cannot.
fn message_block(message: &str) -> Option<&'static str> {
    if message.contains('\0') {
        return Some(NUL_MESSAGE);
    }
    let subject = message.lines().next().unwrap_or("").trim();
    if subject.is_empty() {
        return Some(NO_MESSAGE);
    }
    (subject.chars().count() > SUBJECT_MAX).then_some(LONG_SUBJECT)
}

/// The text the person confirms a commit with:
/// `commit <tree> to <branch> on <head>: "<subject>"`, and what else it
/// carries.
fn confirm_text(plan: &CommitPlan, branch: &str, earlier: usize, no_tests: bool) -> String {
    let subject = plan.message.lines().next().unwrap_or("");
    let mut text = format!(
        "commit {} to {branch} on {}: \"{subject}\"",
        &plan.tree[..12],
        &plan.head[..8]
    );
    if earlier > 0 {
        text.push_str(&format!(", {}", counted(earlier, "earlier commit")));
    }
    if plan.allowed > 0 {
        text.push_str(&format!(
            ", {} allowed",
            counted(plan.allowed as usize, "flagged line")
        ));
    }
    if plan.waived > 0 {
        text.push_str(&format!(
            ", {} waived",
            counted(plan.waived as usize, "finding")
        ));
    }
    if no_tests {
        text.push_str(", no tests run");
    }
    text
}

/// What a commit needs beyond the status: the change it is of.
struct Gated {
    snap: Snapshot,
    base: Base,
    review: Review,
    branch: String,
    repo_key: String,
    origin: Option<String>,
    /// The places of the allowed shape hits in the uncommitted change.
    allowed: Vec<String>,
    /// The record of the last commit made from the Tasks panel, if any.
    record: Option<Shipped>,
}

/// Runs every check on the session's change in `folder`. The status, and
/// what a commit needs when nothing is in the way.
fn gate(
    config_dir: &Path,
    folder: &Path,
    session: &str,
    cfg: &CodeShip,
    vault: &dyn Vault,
    draft: &Draft,
) -> (ShipStatus, Option<Gated>) {
    let snap = match ship_tree::snapshot(folder) {
        Ok(snap) => snap,
        Err(err) => return (ShipStatus::unread(unread(&err)), None),
    };
    let top = snap.top.as_path();
    let mut blocked = Vec::new();

    let since_base = match session_commits(session, &snap) {
        Ok(found) => Some(found),
        Err(why) => {
            blocked.push(why);
            None
        }
    };
    // The project's records are found by the address the session began
    // with. One that differs now is in the way, and is never the key.
    let origin = match ship_tree::origin(top) {
        Ok(now) => {
            if since_base
                .as_ref()
                .is_some_and(|(base, _)| base.origin != now)
            {
                blocked.push(ORIGIN_CHANGED.into());
            }
            now
        }
        Err(err) => {
            blocked.push(unread(&err));
            None
        }
    };
    let origin = match &since_base {
        Some((base, _)) => base.origin.clone(),
        None => origin,
    };
    let repo_key = runs::repo_key(folder, origin.as_deref());
    let commits = since_base
        .as_ref()
        .map_or(&[][..], |(_, commits)| commits.as_slice());
    if commits.len() > COMMITS_MAX {
        blocked.push(TOO_MANY.into());
    }

    let mut new_branch = false;
    let branch = match ship_tree::branch(top) {
        Ok(Some(branch)) => {
            if let Err(why) = branch_block(config_dir, cfg, top, &repo_key, &branch) {
                // A config that could not be read is no case for a branch.
                new_branch = !why.starts_with("The project could not be read");
                blocked.push(why);
            }
            Some(branch)
        }
        Ok(None) => {
            new_branch = true;
            blocked.push(no_commit(Blocked::Detached, ""));
            None
        }
        Err(err) => {
            blocked.push(unread(&err));
            None
        }
    };

    if let Err(why) = grokhub_agent::settled(session, folder) {
        blocked.push(why);
    }

    let changes = match snap.changes(&snap.head) {
        Ok(changes) => changes,
        Err(err) => return (ShipStatus::unread(unread(&err)), None),
    };
    if changes.is_empty() {
        blocked.push(NOTHING.into());
    }

    let record = match review::shipped_for(session) {
        Ok(shipped) => shipped.filter(|shipped| same_folder(Path::new(&shipped.project), top)),
        Err(err) => {
            blocked.push(format!("The last commit's record can't be read: {err}"));
            None
        }
    };
    let shipped = shipped_now(record.as_ref(), &snap).cloned();
    let (review, fresh) = review_check(
        session,
        &snap,
        since_base.as_ref(),
        shipped.as_ref(),
        &mut blocked,
    );
    let tests = tests_check(config_dir, folder, &repo_key, &snap.tree, &mut blocked);

    let hidden = secrets::hidden(&secrets::held_values(config_dir, vault, &repo_key));
    // Past the limit the change is refused whole; its commits are not read.
    let scanned = if commits.len() > COMMITS_MAX {
        &[][..]
    } else {
        commits
    };
    let secrets = match scan_change(&snap, &changes, scanned, &hidden) {
        Ok(mut hits) => {
            allow(&mut hits, &draft.allowed, &snap.tree, record.as_ref());
            let flagged = hits.iter().filter(|hit| !hit.allowed).count();
            if flagged > 0 {
                blocked.push(format!(
                    "The secret scan flagged {} in the change.",
                    counted(flagged, "place")
                ));
            }
            Some(hits)
        }
        Err(err) => {
            blocked.push(format!("The secret scan could not run: {err}"));
            None
        }
    };

    match ship_tree::hidden_changed(top, &changes) {
        Ok(paths) => blocked.extend(paths.iter().map(|path| {
            format!(
                "{path} is marked skip-worktree or assume-unchanged and would be committed; \
                 commit from your terminal."
            )
        })),
        Err(err) => blocked.push(unread(&err)),
    }

    let mut author = None;
    match ship_tree::author(top) {
        Ok(found) => {
            if found.name.is_none() || found.email.is_none() {
                blocked.push(NO_AUTHOR.into());
            }
            if found.signs {
                blocked.push(SIGNED.into());
            }
            if let (Some(name), Some(email)) = (found.name, found.email) {
                author = Some(format!("{name} <{email}>"));
            }
        }
        Err(err) => blocked.push(unread(&err)),
    }

    let message = tidy(draft.message.as_deref().unwrap_or(DEFAULT_MESSAGE));
    if let Some(why) = message_block(&message) {
        blocked.push(why.into());
    }

    let allowed: Vec<String> = secrets
        .iter()
        .flatten()
        .filter(|hit| hit.allowed && hit.commit.is_none())
        .map(SecretHit::place)
        .collect();
    let mut plan = CommitPlan {
        tree: snap.tree.clone(),
        head: snap.head.clone(),
        files: changes
            .iter()
            .take(LIST_MAX)
            .map(|change| FileRow {
                status: change.status.to_string(),
                path: change.path.clone(),
            })
            .collect(),
        more: changes.len().saturating_sub(LIST_MAX),
        commits: commits
            .iter()
            .take(LIST_MAX)
            .map(|commit| CommitRow {
                sha: commit.sha.clone(),
                subject: commit.subject.clone(),
            })
            .collect(),
        author,
        message,
        notices: NOTICES.iter().map(|notice| notice.to_string()).collect(),
        waived: fresh.as_ref().map_or(0, Review::waived_blockers),
        allowed: allowed.len() as u32,
        confirm: None,
    };
    let shipped_sha = shipped
        .as_ref()
        .filter(|_| changes.is_empty())
        .map(|shipped| shipped.sha.clone());

    let ready = match (blocked.is_empty(), since_base, fresh, branch.clone()) {
        (true, Some((base, earlier)), Some(review), Some(branch)) => {
            let no_tests = tests == TestsCheck::NoCommand;
            plan.confirm = Some(confirm_text(&plan, &branch, earlier.len(), no_tests));
            Some(Gated {
                snap,
                base,
                review,
                branch,
                repo_key,
                origin,
                allowed,
                record,
            })
        }
        _ => None,
    };
    let status = ShipStatus {
        blocked,
        checks: Some(Checks {
            branch,
            new_branch,
            review,
            tests,
            secrets,
        }),
        commit: Some(plan),
        shipped: shipped_sha,
    };
    (status, ready)
}

/// The checks on the session's change in `folder`, with the project's
/// secret values read from `vault`.
pub fn status_with(
    config_dir: &Path,
    folder: &Path,
    session: &str,
    cfg: &CodeShip,
    vault: &dyn Vault,
) -> ShipStatus {
    status_for(config_dir, folder, session, cfg, vault, &Draft::default())
}

/// [`status_with`] for the message and the allowed lines of `draft`.
pub fn status_for(
    config_dir: &Path,
    folder: &Path,
    session: &str,
    cfg: &CodeShip,
    vault: &dyn Vault,
    draft: &Draft,
) -> ShipStatus {
    // The session's records are read from this config folder.
    let _config = grokhub_agent::perm::ConfigGuard::set(config_dir);
    gate(config_dir, folder, session, cfg, vault, draft).0
}

/// Where an activity line is about.
struct Place<'a> {
    repo_key: &'a str,
    origin: Option<&'a str>,
    branch: &'a str,
}

/// One line of the Code tab's activity log for this project, of a step the
/// person confirmed.
fn log(
    config_dir: &Path,
    cfg: &CodeShip,
    place: &Place,
    kind: code_activity::Kind,
    command: &str,
    exit: Option<i32>,
    summary: &str,
) {
    let host = place.origin.map_or(ship::Host::Other, ship::host_of);
    let _ = code_activity::append(
        config_dir,
        &Entry {
            ts_ms: grokhub_core::now_ms(),
            kind,
            repo_key: place.repo_key.into(),
            host: host.id().into(),
            branch: place.branch.into(),
            command: command.into(),
            mode: ship::ShipMode::Ask.id().into(),
            approved_by: "user".into(),
            exit,
            summary: summary.into(),
        },
        cfg.activity_keep,
    );
}

/// Commits the session's change in `folder`, after the person's click.
/// Every check runs again here, and the commit is made only when nothing is
/// in the way, `confirmed` is true and `command` is the exact text the
/// confirm showed. The commit holds the checked tree, its parent is the
/// reviewed HEAD, and the branch moves only while it still names that HEAD.
/// No hook, no signing program and no editor runs.
pub fn commit_with(
    config_dir: &Path,
    folder: &Path,
    session: &str,
    cfg: &CodeShip,
    vault: &dyn Vault,
    draft: &Draft,
    confirmed: bool,
    command: Option<&str>,
) -> Result<Committed, String> {
    let _config = grokhub_agent::perm::ConfigGuard::set(config_dir);
    let (status, gated) = gate(config_dir, folder, session, cfg, vault, draft);
    let (Some(gated), Some(plan)) = (gated, status.commit.as_ref()) else {
        if status.blocked.is_empty() {
            return Err("The change could not be checked.".into());
        }
        return Err(status.blocked.join(" "));
    };
    if !confirmed {
        return Err(NOT_CONFIRMED.into());
    }
    let confirm = plan.confirm.clone().unwrap_or_default();
    if command != Some(confirm.as_str()) {
        return Err(CONFIRM_STALE.into());
    }
    let sha = ship_tree::commit(&gated.snap, &gated.branch, &plan.message)
        .map_err(|err| format!("Nothing was committed: {err}."))?;
    let short = &sha[..8];
    let mut said = format!("Committed {short} to {}.", gated.branch);

    let shipped = Shipped {
        project: gated.snap.top.to_string_lossy().into_owned(),
        branch: gated.branch.clone(),
        sha: sha.clone(),
        parent: gated.snap.head.clone(),
        tree: gated.snap.tree.clone(),
        base: gated.base.head.clone(),
        commits: std::iter::once(sha.clone())
            .chain(gated.review.commits.iter().cloned())
            .collect(),
        allowed: Vec::new(),
        pushed: false,
    };
    // Lines allowed in an earlier commit made here stay allowed there, while
    // that commit is still one this commit adds on top of the session's start.
    let earlier = gated.record.iter().flat_map(|record| {
        record
            .allowed
            .iter()
            .filter(|line| shipped.commits.contains(&line.commit))
            .cloned()
    });
    let now = gated.allowed.iter().map(|place| AllowedLine {
        commit: sha.clone(),
        place: place.clone(),
    });
    let shipped = Shipped {
        allowed: earlier.chain(now).collect(),
        ..shipped
    };
    if let Err(err) = review::note_shipped(session, &shipped) {
        said.push_str(&format!(
            " Its record could not be kept ({err}), so push it from your terminal."
        ));
    }
    if ship_tree::reset_index(&gated.snap.top).is_err() {
        said.push_str(" The index is out of date: run git reset.");
    }
    let mut summary = format!("Committed {short}");
    if !gated.allowed.is_empty() {
        summary.push_str(&format!("; allowed {}", gated.allowed.join(", ")));
    }
    let place = Place {
        repo_key: &gated.repo_key,
        origin: gated.origin.as_deref(),
        branch: &gated.branch,
    };
    log(
        config_dir,
        cfg,
        &place,
        code_activity::Kind::Commit,
        &confirm,
        Some(0),
        &summary,
    );
    drop(gated);
    let status = gate(config_dir, folder, session, cfg, vault, &Draft::default()).0;
    Ok(Committed { sha, said, status })
}

/// Waives the blocker at `index` of the session's review of the tree
/// `tree` written at `at_ms`, after the person's click, and logs it. A
/// second click on a blocker that is waived already logs nothing.
pub fn waive(
    config_dir: &Path,
    folder: &Path,
    session: &str,
    cfg: &CodeShip,
    tree: &str,
    at_ms: u64,
    index: u32,
) -> Result<(), String> {
    let _config = grokhub_agent::perm::ConfigGuard::set(config_dir);
    let before = review::review_for(session).ok().flatten();
    let review = review::waive(session, tree, at_ms, index)?;
    if before.is_some_and(|before| before.at_ms == at_ms && before.waived.contains(&index)) {
        return Ok(());
    }
    let finding = grokhub_core::findings::Findings::parse(&review.body)
        .and_then(|card| card.items.into_iter().nth(index as usize))
        .map(|item| item.text)
        .unwrap_or_default();
    let origin = review::base_for(session)
        .ok()
        .flatten()
        .and_then(|base| base.origin);
    let repo_key = runs::repo_key(folder, origin.as_deref());
    let branch = ship_tree::head(folder)
        .ok()
        .and_then(|(top, _)| ship_tree::branch(&top).ok().flatten())
        .unwrap_or_default();
    let text: String = finding.chars().take(200).collect();
    let place = Place {
        repo_key: &repo_key,
        origin: origin.as_deref(),
        branch: &branch,
    };
    let command = format!(
        "waive finding {} of the review of {}",
        index + 1,
        &tree[..tree.len().min(12)]
    );
    log(
        config_dir,
        cfg,
        &place,
        code_activity::Kind::Review,
        &command,
        None,
        &format!("Waived: {text}"),
    );
    Ok(())
}

#[cfg(test)]
#[path = "commit_tests.rs"]
mod tests;
