#![cfg(unix)]

use super::*;
use crate::ship::{Hosts, ShipMode};
use std::path::PathBuf;
use std::process::Command;

fn sh_git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(["-c", "init.defaultBranch=main"])
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

struct Fixture {
    root: PathBuf,
    bare: PathBuf,
    work: PathBuf,
    config: PathBuf,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn commit(f: &Fixture, text: &str, message: &str) -> String {
    std::fs::write(f.work.join("a.txt"), text).unwrap();
    sh_git(&f.work, &["add", "a.txt"]);
    sh_git(&f.work, &["commit", "-q", "-m", message]);
    sh_git(&f.work, &["rev-parse", "HEAD"])
}

/// A repo with a local bare origin, `main` pushed, and `fix` checked out
/// with two commits pushed.
fn fixture(tag: &str) -> Fixture {
    let root = std::env::temp_dir().join(format!("code-revert-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let bare = root.join("origin.git");
    let work = root.join("work");
    let config = root.join("config");
    std::fs::create_dir_all(&config).unwrap();
    sh_git(
        &root,
        &["init", "-q", "--bare", "-b", "main", bare.to_str().unwrap()],
    );
    sh_git(
        &root,
        &[
            "clone",
            "-q",
            bare.to_str().unwrap(),
            work.to_str().unwrap(),
        ],
    );
    sh_git(&work, &["config", "user.name", "t"]);
    sh_git(&work, &["config", "user.email", "t@example.invalid"]);
    let f = Fixture {
        root,
        bare,
        work,
        config,
    };
    commit(&f, "one\n", "one");
    sh_git(&f.work, &["push", "-q", "origin", "main"]);
    sh_git(&f.work, &["remote", "set-head", "origin", "main"]);
    sh_git(&f.work, &["switch", "-q", "-c", "fix"]);
    commit(&f, "two\n", "two");
    commit(&f, "three\n", "three");
    sh_git(&f.work, &["push", "-q", "origin", "fix"]);
    f
}

fn origin_sha(f: &Fixture, branch: &str) -> String {
    sh_git(&f.bare, &["rev-parse", &format!("refs/heads/{branch}")])
}

fn head(f: &Fixture) -> String {
    sh_git(&f.work, &["rev-parse", "HEAD"])
}

/// Ask mode, with the local origin allowed as a host.
fn ask() -> CodeShip {
    CodeShip {
        mode: ShipMode::Ask,
        hosts: Hosts {
            github: true,
            other: true,
        },
        ..CodeShip::default()
    }
}

fn go(f: &Fixture, cfg: &CodeShip, sha: &str) -> Result<Reverted, String> {
    let p = plan(&f.config, &f.work, cfg, sha);
    revert(
        &f.config,
        &f.work,
        cfg,
        sha,
        true,
        &p.revert_command,
        &p.push_command,
    )
}

#[test]
fn a_revert_is_a_new_commit_pushed_through_the_guard() {
    let f = fixture("ok");
    let three = head(&f);
    let short = sh_git(&f.work, &["rev-parse", "--short", &three]);
    let p = plan(&f.config, &f.work, &ask(), &three);
    assert_eq!(p.blocked, None);
    assert_eq!(p.subject, "three");
    assert_eq!(p.revert_command, format!("git revert --no-edit {three}"));
    assert_eq!(
        p.push_command,
        "git push --porcelain origin HEAD:refs/heads/fix"
    );

    let done = go(&f, &ask(), &three).unwrap();
    let now = head(&f);
    assert_eq!(
        done.summary,
        format!(
            "Reverted {short} as new commit {}. Pushed fix to origin.",
            done.commit
        )
    );
    // History is kept: the reverted commit is the new one's parent, and the
    // file is back to what it was before it.
    assert_eq!(sh_git(&f.work, &["rev-parse", "HEAD~1"]), three);
    assert_eq!(
        std::fs::read_to_string(f.work.join("a.txt")).unwrap(),
        "two\n"
    );
    assert_eq!(
        sh_git(&f.work, &["log", "-n1", "--format=%s"]),
        "Revert \"three\""
    );
    // The push landed, fast-forward only.
    assert_eq!(origin_sha(&f, "fix"), now);
    sh_git(&f.bare, &["merge-base", "--is-ancestor", &three, &now]);

    let log = crate::activity::list_for(&f.config, &f.work, 10);
    assert_eq!(log[0].summary, "Pushed fix to origin.");
    assert_eq!(log[1].command, format!("git revert --no-edit {three}"));
    assert_eq!(log[1].approved_by, "user");
}

#[test]
fn main_and_the_default_branch_are_never_reverted_and_pushed() {
    let f = fixture("main");
    sh_git(&f.work, &["switch", "-q", "main"]);
    let one = head(&f);
    let before = origin_sha(&f, "main");
    let p = plan(&f.config, &f.work, &ask(), &one);
    let why =
        "main is the main or default branch. The app never pushes to it; make a branch first.";
    assert_eq!(p.blocked.as_deref(), Some(why));
    assert_eq!(go(&f, &ask(), &one), Err(why.to_string()));
    // Nothing changed, here or on origin.
    assert_eq!(head(&f), one);
    assert_eq!(origin_sha(&f, "main"), before);
    let log = crate::activity::list_for(&f.config, &f.work, 10);
    assert_eq!(log[0].approved_by, "none");
    assert_eq!(log[0].kind, crate::activity::Kind::Blocked);
}

#[test]
fn it_always_asks_even_in_auto_beta() {
    let f = fixture("ask");
    let three = head(&f);
    let cfg = CodeShip {
        mode: ShipMode::AutoBeta,
        ..ask()
    };
    let p = plan(&f.config, &f.work, &cfg, &three);
    let out = revert(
        &f.config,
        &f.work,
        &cfg,
        &three,
        false,
        &p.revert_command,
        &p.push_command,
    );
    assert_eq!(out, Err("A revert always asks first.".to_string()));
    assert_eq!(head(&f), three);
}

#[test]
fn a_confirm_for_another_commit_or_branch_is_refused() {
    let f = fixture("stale");
    let three = head(&f);
    let two = sh_git(&f.work, &["rev-parse", "HEAD~1"]);
    let p = plan(&f.config, &f.work, &ask(), &two);
    let wrong = revert(
        &f.config,
        &f.work,
        &ask(),
        &three,
        true,
        &p.revert_command,
        &p.push_command,
    );
    assert_eq!(
        wrong,
        Err("The branch changed since you confirmed. Check the commands and confirm again.".into())
    );
    let p = plan(&f.config, &f.work, &ask(), &three);
    let other = revert(
        &f.config,
        &f.work,
        &ask(),
        &three,
        true,
        &p.revert_command,
        "git push origin other",
    );
    assert!(other.unwrap_err().starts_with("The branch changed"));
    assert_eq!(head(&f), three);
    assert_eq!(origin_sha(&f, "fix"), three);
}

#[test]
fn dirty_trees_merges_and_strangers_are_refused() {
    let f = fixture("refuse");
    let three = head(&f);
    std::fs::write(f.work.join("a.txt"), "local edit\n").unwrap();
    assert_eq!(
        plan(&f.config, &f.work, &ask(), &three).blocked.as_deref(),
        Some("Commit or stash your changes first; a revert needs a clean working tree.")
    );
    sh_git(&f.work, &["checkout", "--", "a.txt"]);

    assert_eq!(
        plan(
            &f.config,
            &f.work,
            &ask(),
            "0123456789abcdef0123456789abcdef01234567"
        )
        .blocked
        .as_deref(),
        Some("That commit isn't on this branch.")
    );
    assert_eq!(
        plan(&f.config, &f.work, &ask(), "HEAD").blocked.as_deref(),
        Some("That commit isn't on this branch.")
    );

    sh_git(&f.work, &["switch", "-q", "-c", "side", "main"]);
    std::fs::write(f.work.join("b.txt"), "b\n").unwrap();
    sh_git(&f.work, &["add", "b.txt"]);
    sh_git(&f.work, &["commit", "-q", "-m", "side"]);
    sh_git(&f.work, &["switch", "-q", "fix"]);
    sh_git(
        &f.work,
        &["merge", "-q", "--no-ff", "-m", "merge side", "side"],
    );
    let merge = head(&f);
    assert_eq!(
        plan(&f.config, &f.work, &ask(), &merge).blocked.as_deref(),
        Some("That's a merge commit. The Code tab reverts one commit at a time.")
    );
    assert_eq!(head(&f), merge);
}

#[test]
fn a_conflict_stops_and_leaves_the_tree_as_it_was() {
    let f = fixture("conflict");
    let two = sh_git(&f.work, &["rev-parse", "HEAD~1"]);
    let three = head(&f);
    assert_eq!(
        go(&f, &ask(), &two),
        Err("The revert hit a conflict, so it was stopped and nothing changed. Fix it by hand, or revert a newer commit first.".into())
    );
    assert_eq!(head(&f), three);
    assert_eq!(sh_git(&f.work, &["status", "--porcelain"]), "");
    assert!(!f.work.join(".git/REVERT_HEAD").exists());
    assert_eq!(origin_sha(&f, "fix"), three);
}

#[test]
fn commits_lists_the_branch_newest_first() {
    let f = fixture("list");
    let list = commits(&f.work, SHOW);
    let subjects: Vec<&str> = list.iter().map(|c| c.subject.as_str()).collect();
    assert_eq!(subjects, ["three", "two", "one"]);
    assert_eq!(list[0].sha, head(&f));
    assert!(list.iter().all(|c| !c.merge && c.short.len() >= 7));
}
