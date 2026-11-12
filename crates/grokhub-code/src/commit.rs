//! What stands between a session's change and a commit: the checks the Tasks
//! panel lists. Read only. Nothing here commits, stages or pushes, and every
//! file under the project's `.git` keeps its bytes.
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

use crate::runs::{self, Kind, LastRun};
use crate::secrets::{self, Vault};
use crate::ship::{self, Blocked, CodeShip};
use grokhub_agent::review::{self, Base};
use grokhub_agent::ship_tree::{self, same_folder, AddedLine, Change, Commit, Snapshot};
use grokhub_core::redact::{holds_secret, redact_held_secrets};
use serde::Serialize;
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
}

/// The checks on a change that could be read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Checks {
    /// The branch HEAD is on; none on a detached HEAD.
    pub branch: Option<String>,
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
}

/// A `ship_tree` error as a sentence of its own.
fn unread(err: &str) -> String {
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

fn review_check(
    session: &str,
    snap: &Snapshot,
    since_base: Option<&(Base, Vec<Commit>)>,
    blocked: &mut Vec<String>,
) -> ReviewCheck {
    let review = match review::review_for(session) {
        Ok(Some(review)) => review,
        Ok(None) => {
            blocked.push(NOT_REVIEWED.into());
            return ReviewCheck::None;
        }
        Err(err) => {
            blocked.push(format!("The review can't be read: {err}"));
            return ReviewCheck::None;
        }
    };
    let fresh = since_base.is_some_and(|(base, commits)| {
        same_folder(Path::new(&review.project), &snap.top)
            && review.base == base.head
            && review.head == snap.head
            && review.tree == snap.tree
            && review.commits.iter().eq(commits.iter().map(|c| &c.sha))
    });
    if !fresh {
        blocked.push(REVIEW_STALE.into());
        return ReviewCheck::Stale;
    }
    if review.blockers > 0 {
        blocked.push(format!(
            "The review found {}.",
            counted(review.blockers as usize, "blocker")
        ));
        return ReviewCheck::Blockers {
            count: review.blockers,
        };
    }
    ReviewCheck::Clean
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

/// The checks on the session's change in `folder`, with the project's
/// secret values read from `vault`.
pub fn status_with(
    config_dir: &Path,
    folder: &Path,
    session: &str,
    cfg: &CodeShip,
    vault: &dyn Vault,
) -> ShipStatus {
    // The session's records are read from this config folder.
    let _config = grokhub_agent::perm::ConfigGuard::set(config_dir);
    let snap = match ship_tree::snapshot(folder) {
        Ok(snap) => snap,
        Err(err) => {
            return ShipStatus {
                blocked: vec![unread(&err)],
                checks: None,
            }
        }
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
        Some((base, _)) => base.origin.as_deref(),
        None => origin.as_deref(),
    };
    let repo_key = runs::repo_key(folder, origin);
    let commits = since_base
        .as_ref()
        .map_or(&[][..], |(_, commits)| commits.as_slice());
    if commits.len() > COMMITS_MAX {
        blocked.push(TOO_MANY.into());
    }

    let branch = match ship_tree::branch(top) {
        Ok(Some(branch)) => {
            if let Err(why) = branch_block(config_dir, cfg, top, &repo_key, &branch) {
                blocked.push(why);
            }
            Some(branch)
        }
        Ok(None) => {
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
        Err(err) => {
            return ShipStatus {
                blocked: vec![unread(&err)],
                checks: None,
            }
        }
    };
    if changes.is_empty() {
        blocked.push(NOTHING.into());
    }

    let review = review_check(session, &snap, since_base.as_ref(), &mut blocked);
    let tests = tests_check(config_dir, folder, &repo_key, &snap.tree, &mut blocked);

    let hidden = secrets::hidden(&secrets::held_values(config_dir, vault, &repo_key));
    // Past the limit the change is refused whole; its commits are not read.
    let scanned = if commits.len() > COMMITS_MAX {
        &[][..]
    } else {
        commits
    };
    let secrets = match scan_change(&snap, &changes, scanned, &hidden) {
        Ok(hits) => {
            if !hits.is_empty() {
                blocked.push(format!(
                    "The secret scan flagged {} in the change.",
                    counted(hits.len(), "place")
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

    match ship_tree::author(top) {
        Ok(author) => {
            if author.name.is_none() || author.email.is_none() {
                blocked.push(NO_AUTHOR.into());
            }
            if author.signs {
                blocked.push(SIGNED.into());
            }
        }
        Err(err) => blocked.push(unread(&err)),
    }

    ShipStatus {
        blocked,
        checks: Some(Checks {
            branch,
            review,
            tests,
            secrets,
        }),
    }
}

#[cfg(test)]
#[path = "commit_tests.rs"]
mod tests;
