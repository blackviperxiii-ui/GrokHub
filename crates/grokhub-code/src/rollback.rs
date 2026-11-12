//! Code tab slice 5c: roll a commit back as a new revert commit, then push
//! it through the same push guard as the Push button.
//!
//! Fixed rules: a revert always asks first, in every mode; it never resets,
//! never rewrites history and never force-pushes. A conflict aborts the
//! revert, so the tree is left as it was. Merge commits aren't reverted.

use crate::activity::{self as code_activity, Entry, Kind};
use crate::ship::{self, Action, CodeShip};
use grokhub_core::proc_util::hide_windows_console;
use serde::Serialize;
use std::path::Path;
use std::process::Stdio;

/// The newest commits the Code tab offers to revert.
pub const SHOW: usize = 10;

pub const ALWAYS_ASKS: &str = "A revert always asks first.";
pub const DIRTY: &str = "Commit or stash your changes first; a revert needs a clean working tree.";
pub const MERGE: &str = "That's a merge commit. The Code tab reverts one commit at a time.";
pub const NOT_HERE: &str = "That commit isn't on this branch.";
pub const STALE: &str =
    "The branch changed since you confirmed. Check the commands and confirm again.";
pub const CONFLICT: &str = "The revert hit a conflict, so it was stopped and nothing changed. Fix it by hand, or revert a newer commit first.";

/// One commit on the current branch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Commit {
    pub sha: String,
    pub short: String,
    pub subject: String,
    pub merge: bool,
}

/// What the confirm shows before a revert.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RevertPlan {
    pub sha: String,
    pub short: String,
    pub subject: String,
    pub branch: Option<String>,
    /// The revert, exactly as it runs.
    pub revert_command: String,
    /// The guarded push that follows, exactly as it runs.
    pub push_command: String,
    /// Why it won't run, when it won't.
    pub blocked: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Reverted {
    /// The new revert commit.
    pub commit: String,
    pub summary: String,
    pub output: String,
}

fn git(dir: &Path, args: &[&str]) -> Result<(i32, String), String> {
    let mut c = grokhub_core::git_cmd::git();
    c.args(["-c", "core.quotepath=false"])
        .args(args)
        .current_dir(dir)
        .stdin(Stdio::null())
        .env("GIT_TERMINAL_PROMPT", "0");
    hide_windows_console(&mut c);
    let out = c
        .output()
        .map_err(|_| "git isn't installed or couldn't start".to_string())?;
    let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&out.stderr));
    Ok((out.status.code().unwrap_or(-1), text))
}

fn git_ok(dir: &Path, args: &[&str]) -> Option<String> {
    match git(dir, args) {
        Ok((0, text)) => Some(text.trim().to_string()),
        _ => None,
    }
}

/// The newest `limit` commits on HEAD, newest first.
pub fn commits(dir: &Path, limit: usize) -> Vec<Commit> {
    let n = format!("-n{limit}");
    let Some(raw) = git_ok(dir, &["log", &n, "--format=%H%x1f%h%x1f%P%x1f%s"]) else {
        return Vec::new();
    };
    raw.lines()
        .filter_map(|line| {
            let mut parts = line.splitn(4, '\u{1f}');
            let sha = parts.next()?.to_string();
            let short = parts.next()?.to_string();
            let merge = parts.next()?.split_whitespace().count() > 1;
            let subject = parts.next().unwrap_or("").to_string();
            Some(Commit {
                sha,
                short,
                subject,
                merge,
            })
        })
        .collect()
}

/// One commit, read the same way as `commits`.
fn one(dir: &Path, sha: &str) -> Option<Commit> {
    let raw = git_ok(
        dir,
        &["log", "-n1", "--format=%H%x1f%h%x1f%P%x1f%s", sha, "--"],
    )?;
    let mut parts = raw.splitn(4, '\u{1f}');
    Some(Commit {
        sha: parts.next()?.to_string(),
        short: parts.next()?.to_string(),
        merge: parts.next()?.split_whitespace().count() > 1,
        subject: parts.next().unwrap_or("").to_string(),
    })
}

fn full_sha(sha: &str) -> bool {
    sha.len() == 40 && sha.bytes().all(|b| b.is_ascii_hexdigit())
}

pub fn revert_command(sha: &str) -> String {
    format!("git revert --no-edit {sha}")
}

/// What reverting `sha` would run, and why it won't when it won't.
pub fn plan(config_dir: &Path, dir: &Path, cfg: &CodeShip, sha: &str) -> RevertPlan {
    let push = ship::plan(config_dir, dir, cfg, Action::Push);
    let mut out = RevertPlan {
        sha: sha.to_string(),
        short: sha.chars().take(7).collect(),
        subject: String::new(),
        branch: push.branch.clone(),
        revert_command: revert_command(sha),
        push_command: push.command.clone(),
        blocked: None,
    };
    let found = full_sha(sha)
        .then(|| one(dir, sha))
        .flatten()
        .filter(|_| git_ok(dir, &["merge-base", "--is-ancestor", sha, "HEAD"]).is_some());
    let blocked = match &found {
        _ if push.blocked.is_some() => push.blocked.clone(),
        None => Some(NOT_HERE.to_string()),
        Some(c) if c.merge => Some(MERGE.to_string()),
        Some(_) if !clean(dir) => Some(DIRTY.to_string()),
        Some(_) => None,
    };
    if let Some(c) = found {
        out.short = c.short;
        out.subject = c.subject;
    }
    out.blocked = blocked;
    out
}

fn clean(dir: &Path) -> bool {
    matches!(git_ok(dir, &["status", "--porcelain", "--untracked-files=no"]), Some(s) if s.is_empty())
}

fn log(
    config_dir: &Path,
    cfg: &CodeShip,
    dir: &Path,
    p: &RevertPlan,
    approved_by: &str,
    exit: Option<i32>,
    summary: &str,
) {
    let host = ship::push_host(dir).unwrap_or(ship::Host::Other);
    // No kind of its own: the cabin's Code tab matches on every kind, so a
    // revert is logged as a push-side entry with its own command.
    let kind = if approved_by == "none" {
        Kind::Blocked
    } else {
        Kind::Push
    };
    let _ = code_activity::append(
        config_dir,
        &Entry {
            ts_ms: grokhub_core::now_ms(),
            kind,
            repo_key: crate::runs::repo_key(dir, crate::repo::origin_url(dir).as_deref()),
            host: host.id().into(),
            branch: p.branch.clone().unwrap_or_default(),
            command: p.revert_command.clone(),
            mode: cfg.mode.id().into(),
            approved_by: approved_by.into(),
            exit,
            summary: summary.into(),
        },
        cfg.activity_keep,
    );
}

/// Revert `sha` as a new commit, then push it through the push guard. Always
/// needs `confirmed`, and the two commands the person saw, so a yes given for
/// one branch or commit never runs for another.
pub fn revert(
    config_dir: &Path,
    dir: &Path,
    cfg: &CodeShip,
    sha: &str,
    confirmed: bool,
    seen_revert: &str,
    seen_push: &str,
) -> Result<Reverted, String> {
    if !confirmed {
        return Err(ALWAYS_ASKS.into());
    }
    let p = plan(config_dir, dir, cfg, sha);
    if let Some(why) = p.blocked.clone() {
        log(
            config_dir,
            cfg,
            dir,
            &p,
            "none",
            None,
            &format!("Revert {}: {why}", p.short),
        );
        return Err(why);
    }
    if seen_revert != p.revert_command || seen_push != p.push_command {
        return Err(STALE.into());
    }
    let (exit, raw) = git(dir, &["revert", "--no-edit", sha])?;
    if exit != 0 {
        let _ = git(dir, &["revert", "--abort"]);
        log(config_dir, cfg, dir, &p, "user", Some(exit), CONFLICT);
        return Err(CONFLICT.into());
    }
    let commit = git_ok(dir, &["rev-parse", "--short", "HEAD"]).unwrap_or_default();
    let made = format!("Reverted {} as new commit {commit}.", p.short);
    log(config_dir, cfg, dir, &p, "user", Some(0), &made);
    let mut output = ship::clean_output(&raw);
    // The push runs the guard again, with the command the person saw.
    let summary = match ship::ship(
        config_dir,
        dir,
        cfg,
        Action::Push,
        true,
        Some(&p.push_command),
    ) {
        Ok(done) => {
            output.push_str(&done.output);
            format!("{made} {}", done.summary)
        }
        Err(why) => format!("{made} It wasn't pushed: {why}"),
    };
    Ok(Reverted {
        commit,
        summary,
        output,
    })
}

#[cfg(test)]
#[path = "rollback_tests.rs"]
mod tests;
