use super::*;
use crate::runs::Mode;
use grokhub_agent::perm::ConfigGuard;
use grokhub_agent::review::Review;
use std::cell::RefCell;
use std::collections::HashMap;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};

/// An in-memory stand-in for the OS keychain.
#[derive(Default)]
struct MemVault {
    items: RefCell<HashMap<String, String>>,
}

impl Vault for MemVault {
    fn get(&self, account: &str) -> Result<Option<String>, String> {
        Ok(self.items.borrow().get(account).cloned())
    }
    fn set(&self, account: &str, value: &str) -> Result<(), String> {
        self.items
            .borrow_mut()
            .insert(account.to_string(), value.to_string());
        Ok(())
    }
    fn delete(&self, account: &str) -> Result<(), String> {
        self.items.borrow_mut().remove(account);
        Ok(())
    }
}

/// Plain git in `dir`, for building a fixture. `input` is its stdin.
fn git_in(dir: &Path, args: &[&str], input: &str) -> String {
    let mut child = Command::new("git")
        .args(["-c", "safe.directory=*"])
        .arg("-C")
        .arg(dir)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("git");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(
        out.status.success(),
        "{args:?} {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn git(dir: &Path, args: &[&str]) -> String {
    git_in(dir, args, "")
}

/// A config folder, a project and a session that began in it.
struct Fixture {
    base: PathBuf,
    cfg: PathBuf,
    project: PathBuf,
    session: String,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.base);
    }
}

/// A repository with one commit on `main`, which `origin/HEAD` names as the
/// default branch, and its author set in its own config. Nothing is noted
/// for the session yet and `main` is checked out.
fn bare_fixture(name: &str) -> Fixture {
    static SERIAL: AtomicU32 = AtomicU32::new(0);
    let base = std::env::temp_dir().join(format!(
        "grokhub-commit-{name}-{}-{}",
        std::process::id(),
        SERIAL.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&base);
    let project = base.join("my-app");
    std::fs::create_dir_all(&project).unwrap();
    git(&project, &["init", "-b", "main"]);
    git(&project, &["config", "user.email", "test@example.com"]);
    git(&project, &["config", "user.name", "test"]);
    git(&project, &["config", "commit.gpgsign", "false"]);
    std::fs::write(project.join("README"), "hi\n").unwrap();
    git(&project, &["add", "README"]);
    git(&project, &["commit", "-m", "init"]);
    let head = git(&project, &["rev-parse", "HEAD"]);
    git(&project, &["update-ref", "refs/remotes/origin/main", &head]);
    git(
        &project,
        &[
            "symbolic-ref",
            "refs/remotes/origin/HEAD",
            "refs/remotes/origin/main",
        ],
    );
    Fixture {
        cfg: base.join("cfg"),
        project,
        session: format!("ship-{name}"),
        base,
    }
}

/// [`bare_fixture`], on the branch `work/one`, with the session begun there.
fn fixture(name: &str) -> Fixture {
    let fx = bare_fixture(name);
    git(&fx.project, &["switch", "-c", "work/one"]);
    begin(&fx);
    fx
}

/// The session's first lead turn: its starting commit goes on record.
fn begin(fx: &Fixture) {
    let _config = ConfigGuard::set(&fx.cfg);
    review::note_base(&fx.session, &fx.project);
}

fn write(fx: &Fixture, file: &str, text: &str) {
    let path = fx.project.join(file);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

/// Commits `file` alone, as an agent's own `git commit` would.
fn commit(fx: &Fixture, file: &str, subject: &str) -> String {
    git(&fx.project, &["add", file]);
    git(&fx.project, &["commit", "-m", subject]);
    git(&fx.project, &["rev-parse", "HEAD"])
}

fn record(fx: &Fixture, name: &str, text: &str) {
    let dir = fx.cfg.join("helper-work").join(&fx.session);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(name), text).unwrap();
}

/// A review of the change as it is now, with this many blockers.
fn reviewed(fx: &Fixture, blockers: u32) {
    let snap = ship_tree::snapshot(&fx.project).unwrap();
    let base = {
        let _config = ConfigGuard::set(&fx.cfg);
        review::base_for(&fx.session).unwrap().expect("a base")
    };
    let commits = ship_tree::commits_since(&snap.top, &base.head, &snap.head).unwrap();
    let body = match blockers {
        0 => "Findings from this run\n\n- Info: README:1 nothing needs fixing".to_string(),
        n => format!(
            "Findings from this run\n\n{}",
            vec!["- High: src/a.txt:1 the total is off by one"; n as usize].join("\n")
        ),
    };
    let review = Review {
        project: snap.top.to_string_lossy().into_owned(),
        base: base.head,
        head: snap.head.clone(),
        commits: commits.into_iter().map(|commit| commit.sha).collect(),
        tree: snap.tree.clone(),
        at_ms: REVIEWED_AT,
        plan_given: true,
        body,
        blockers,
        cut: false,
        waived: Vec::new(),
    };
    record(fx, "review.json", &serde_json::to_string(&review).unwrap());
}

/// When [`reviewed`] says the review was written.
const REVIEWED_AT: u64 = 1_700_000_000_000;

/// Runs `command` as the project's tests, confirmed, through the real runner.
fn run_tests(fx: &Fixture, command: &str) -> runs::RunResult {
    runs::run_with(
        &fx.cfg,
        &fx.project,
        command,
        true,
        Mode::Auto,
        &MemVault::default(),
        |_| {},
    )
    .unwrap()
}

fn status(fx: &Fixture) -> ShipStatus {
    status_with(
        &fx.cfg,
        &fx.project,
        &fx.session,
        &CodeShip::default(),
        &MemVault::default(),
    )
}

/// A change that is reviewed clean and tested: one new file.
fn ready(name: &str) -> Fixture {
    let fx = fixture(name);
    write(&fx, "src/a.txt", "one\n");
    reviewed(&fx, 0);
    assert_eq!(run_tests(&fx, "echo ok").exit, 0);
    fx
}

fn hit(path: &str, line: Option<u32>, kind: SecretKind, commit: Option<&str>) -> SecretHit {
    SecretHit {
        path: path.to_string(),
        line,
        kind,
        commit: commit.map(str::to_string),
        allowed: false,
    }
}

// -- the whole status ---------------------------------------------------------

/// The tree id of the fixture's README and a `src/a.txt` that holds "one".
const READY_TREE: &str = "eccf684b923fb80e723a1ef1cfc1f703070b1084";

#[test]
fn a_reviewed_and_tested_change_on_a_work_branch_has_nothing_in_the_way() {
    let fx = ready("clean");
    let head = git(&fx.project, &["rev-parse", "HEAD"]);
    let got = status(&fx);
    let confirm = format!(
        "commit {} to work/one on {}: \"Change made in this session\"",
        &READY_TREE[..12],
        &head[..8]
    );
    assert_eq!(
        got,
        ShipStatus {
            blocked: vec![],
            checks: Some(Checks {
                branch: Some("work/one".into()),
                new_branch: false,
                review: ReviewCheck::Clean,
                tests: TestsCheck::Passed {
                    command: "echo ok".into()
                },
                secrets: Some(vec![]),
            }),
            commit: Some(CommitPlan {
                tree: READY_TREE.into(),
                head: head.clone(),
                files: vec![FileRow {
                    status: "A".into(),
                    path: "src/a.txt".into()
                }],
                more: 0,
                commits: vec![],
                author: Some("test <test@example.com>".into()),
                message: "Change made in this session".into(),
                notices: vec![
                    "Every changed file in this folder is included.".into(),
                    "Hooks are not run, and the commit is not signed.".into(),
                ],
                waived: 0,
                allowed: 0,
                confirm: Some(confirm.clone()),
            }),
            shipped: None,
        }
    );
    assert_eq!(
        serde_json::to_value(&got).unwrap(),
        serde_json::json!({
            "blocked": [],
            "checks": {
                "branch": "work/one",
                "review": {"state": "clean"},
                "tests": {"state": "passed", "command": "echo ok"},
                "secrets": [],
            },
            "commit": {
                "tree": READY_TREE,
                "head": head,
                "files": [{"status": "A", "path": "src/a.txt"}],
                "more": 0,
                "commits": [],
                "author": "test <test@example.com>",
                "message": "Change made in this session",
                "notices": [
                    "Every changed file in this folder is included.",
                    "Hooks are not run, and the commit is not signed.",
                ],
                "waived": 0,
                "allowed": 0,
                "confirm": confirm,
            },
            "shipped": null,
        })
    );
}

#[test]
fn reading_the_status_leaves_the_project_as_it_was() {
    let fx = ready("readonly");
    let before = (
        git(&fx.project, &["rev-parse", "HEAD"]),
        git(&fx.project, &["status", "--porcelain"]),
        std::fs::read(fx.project.join(".git").join("index")).unwrap(),
    );
    assert_eq!(status(&fx).blocked, Vec::<String>::new());
    let after = (
        git(&fx.project, &["rev-parse", "HEAD"]),
        git(&fx.project, &["status", "--porcelain"]),
        std::fs::read(fx.project.join(".git").join("index")).unwrap(),
    );
    assert_eq!(before, after);
    assert_eq!(before.1, "?? src/");
}

#[test]
fn every_state_of_a_check_has_its_own_word_for_the_page() {
    let json = |value: serde_json::Value| value.to_string();
    assert_eq!(
        json(serde_json::to_value(ReviewCheck::None).unwrap()),
        r#"{"state":"none"}"#
    );
    assert_eq!(
        json(serde_json::to_value(ReviewCheck::Stale).unwrap()),
        r#"{"state":"stale"}"#
    );
    assert_eq!(
        json(serde_json::to_value(ReviewCheck::Blockers { count: 2 }).unwrap()),
        r#"{"count":2,"state":"blockers"}"#
    );
    assert_eq!(
        json(serde_json::to_value(TestsCheck::NoCommand).unwrap()),
        r#"{"state":"noCommand"}"#
    );
    assert_eq!(
        json(
            serde_json::to_value(TestsCheck::NotRun {
                command: "cargo test".into()
            })
            .unwrap()
        ),
        r#"{"command":"cargo test","state":"notRun"}"#
    );
    assert_eq!(
        json(
            serde_json::to_value(TestsCheck::Failed {
                command: "cargo test".into(),
                exit: 101
            })
            .unwrap()
        ),
        r#"{"command":"cargo test","exit":101,"state":"failed"}"#
    );
    assert_eq!(
        json(serde_json::to_value(hit("a/.env", None, SecretKind::Name, None)).unwrap()),
        r#"{"commit":null,"kind":"name","line":null,"path":"a/.env"}"#
    );
    assert_eq!(
        json(serde_json::to_value(SecretKind::Held).unwrap()),
        r#""held""#
    );
    assert_eq!(
        json(serde_json::to_value(SecretKind::Shape).unwrap()),
        r#""shape""#
    );
}

// -- gate 1: the project ------------------------------------------------------

#[test]
fn a_folder_that_is_no_repository_is_one_sentence_and_no_checks() {
    let fx = fixture("norepo");
    let plain = fx.base.join("plain");
    std::fs::create_dir_all(&plain).unwrap();
    assert_eq!(
        status_with(
            &fx.cfg,
            &plain,
            &fx.session,
            &CodeShip::default(),
            &MemVault::default()
        ),
        ShipStatus {
            blocked: vec!["This folder is not a git repository.".into()],
            checks: None,
            commit: None,
            shipped: None,
        }
    );
}

#[test]
fn a_program_named_in_the_repositorys_own_config_stops_the_read() {
    let fx = ready("sshcommand");
    git(
        &fx.project,
        &["config", "core.sshCommand", "ssh -i /home/user/key"],
    );
    assert_eq!(
        status(&fx),
        ShipStatus {
            blocked: vec![
                "This repository's own git config sets core.sshcommand; commit from your terminal."
                    .into()
            ],
            checks: None,
            commit: None,
            shipped: None,
        }
    );
}

// -- gate 2: the session's base -----------------------------------------------

#[test]
fn a_session_with_no_starting_commit_on_record_is_blocked() {
    let fx = bare_fixture("nobase");
    git(&fx.project, &["switch", "-c", "work/one"]);
    write(&fx, "src/a.txt", "one\n");
    let got = status(&fx);
    assert_eq!(
        got.blocked,
        [
            "This session's starting commit is not on record.",
            "The change has not been reviewed.",
        ]
    );
}

#[test]
fn a_session_that_began_in_another_folder_is_blocked() {
    let fx = ready("otherfolder");
    let head = git(&fx.project, &["rev-parse", "HEAD"]);
    record(
        &fx,
        "base.json",
        &serde_json::json!({"project": "/home/user/elsewhere", "head": head}).to_string(),
    );
    assert_eq!(
        status(&fx).blocked,
        [
            "This session began in another project folder.",
            "Files or commits changed since the review. Review again.",
        ]
    );
}

#[test]
fn a_history_rewritten_below_the_sessions_start_is_blocked() {
    let fx = ready("amend");
    // The first commit is replaced: the session's starting commit is gone
    // from this branch's history.
    git(
        &fx.project,
        &["commit", "--amend", "--allow-empty", "-m", "other"],
    );
    assert_eq!(
        status(&fx).blocked,
        [
            "The branch history changed since this session began; ship from your terminal.",
            "Files or commits changed since the review. Review again.",
        ]
    );
}

#[test]
fn more_than_200_session_commits_are_blocked() {
    let fx = fixture("many");
    let mut stream = String::new();
    for n in 1..=201 {
        let (subject, text) = (format!("c{n}"), format!("{n}\n"));
        stream.push_str(&format!(
            "commit refs/heads/work/one\ncommitter t <t@example.com> 1700000000 +0000\n\
             data {}\n{subject}\n",
            subject.len()
        ));
        if n == 1 {
            stream.push_str(&format!(
                "from {}\n",
                git(&fx.project, &["rev-parse", "HEAD"])
            ));
        }
        stream.push_str(&format!(
            "M 100644 inline f.txt\ndata {}\n{text}\n",
            text.len()
        ));
    }
    git_in(&fx.project, &["fast-import", "--quiet"], &stream);
    assert_eq!(git(&fx.project, &["rev-list", "--count", "HEAD"]), "202");
    let got = status(&fx);
    assert_eq!(
        got.blocked,
        [
            "More than 200 commits were made during this session; ship from your terminal.",
            "The change has not been reviewed.",
        ]
    );
}

// -- gate 3: the branch -------------------------------------------------------

/// The one sentence the branch gate adds for a clean, reviewed change whose
/// branch was changed by `arrange`.
fn branch_sentence(name: &str, cfg: &CodeShip, arrange: impl Fn(&Fixture)) -> Vec<String> {
    let fx = ready(name);
    arrange(&fx);
    status_with(&fx.cfg, &fx.project, &fx.session, cfg, &MemVault::default()).blocked
}

#[test]
fn main_master_the_default_branch_and_a_protected_one_take_no_commit() {
    let cfg = CodeShip::default();
    // `git branch -m` renames the branch in place: HEAD and the files stay.
    assert_eq!(
        branch_sentence("b-main", &cfg, |fx| {
            git(&fx.project, &["branch", "-M", "main"]);
        }),
        ["main is the main or default branch. The app never commits to it; make a branch first."]
    );
    assert_eq!(
        branch_sentence("b-master", &cfg, |fx| {
            git(&fx.project, &["branch", "-M", "master"]);
        }),
        ["master is the main or default branch. The app never commits to it; make a branch first."]
    );
    assert_eq!(
        branch_sentence("b-default", &cfg, |fx| {
            git(
                &fx.project,
                &[
                    "symbolic-ref",
                    "refs/remotes/origin/HEAD",
                    "refs/remotes/origin/work/one",
                ],
            );
        }),
        ["work/one is the main or default branch. The app never commits to it; make a branch first."]
    );
    let strict = CodeShip {
        extra_protected: vec!["work/one".into()],
        ..CodeShip::default()
    };
    assert_eq!(
        branch_sentence("b-protected", &strict, |_| {}),
        ["work/one is protected in Settings → Code, so the app doesn't commit to it."]
    );
}

#[test]
fn a_detached_head_takes_no_commit() {
    let fx = ready("detached");
    git(&fx.project, &["checkout", "--detach"]);
    let got = status(&fx);
    assert_eq!(
        got.blocked,
        ["Check out a branch first; the app doesn't commit on a detached HEAD."]
    );
    let checks = got.checks.expect("checks");
    assert_eq!(checks.branch, None);
    // Quality minor 1: the page offers New branch for it.
    assert!(checks.new_branch);
}

#[test]
fn the_main_branch_offers_new_branch_and_a_work_branch_does_not() {
    let fx = ready("new-branch-offer");
    let offer = |fx: &Fixture| status(fx).checks.expect("checks").new_branch;
    assert!(!offer(&fx));
    git(&fx.project, &["branch", "-M", "main"]);
    assert!(offer(&fx));
    let json = serde_json::to_value(status(&fx)).unwrap();
    assert_eq!(json["checks"]["newBranch"], true);
    ship::new_branch(&fx.cfg, &fx.project, &CodeShip::default(), "work/two").unwrap();
    assert!(!offer(&fx));
}

#[test]
fn with_no_default_branch_on_record_only_a_branch_the_app_made_takes_a_commit() {
    let fx = ready("nodefault");
    git(
        &fx.project,
        &["symbolic-ref", "--delete", "refs/remotes/origin/HEAD"],
    );
    let got = status(&fx);
    assert_eq!(
        got.blocked,
        ["This project's default branch is not known. Use New branch."]
    );
    assert!(got.checks.expect("checks").new_branch);
    // New branch keeps the files and HEAD, and the app remembers making it.
    ship::new_branch(&fx.cfg, &fx.project, &CodeShip::default(), "work/two").unwrap();
    let got = status(&fx);
    assert_eq!(got.blocked, Vec::<String>::new());
    assert_eq!(
        got.checks.expect("checks").branch,
        Some("work/two".to_string())
    );
}

// -- gate 4: settled ----------------------------------------------------------

#[test]
fn a_project_in_the_middle_of_a_merge_is_blocked() {
    let fx = fixture("merging");
    write(&fx, "README", "ours\n");
    commit(&fx, "README", "ours");
    git(&fx.project, &["switch", "-c", "other", "HEAD~1"]);
    write(&fx, "README", "theirs\n");
    commit(&fx, "README", "theirs");
    git(&fx.project, &["switch", "work/one"]);
    let merged = Command::new("git")
        .arg("-C")
        .arg(&fx.project)
        .args(["merge", "other"])
        .output()
        .unwrap();
    assert!(!merged.status.success(), "the merge conflicts");
    let got = status(&fx);
    assert!(
        got.blocked.contains(
            &"The project is in the middle of a merge or rebase, or has unresolved conflicts. \
              Finish or abort it first."
                .to_string()
        ),
        "{:?}",
        got.blocked
    );
}

// -- gate 5: something to commit ----------------------------------------------

#[test]
fn a_folder_with_no_change_has_nothing_to_commit() {
    let fx = fixture("nothing");
    assert_eq!(
        status(&fx).blocked,
        [
            "There is nothing to commit.",
            "The change has not been reviewed."
        ]
    );
}

// -- gate 6: the review -------------------------------------------------------

#[test]
fn a_change_with_no_review_is_blocked() {
    let fx = fixture("unreviewed");
    write(&fx, "src/a.txt", "one\n");
    let got = status(&fx);
    assert_eq!(got.blocked, ["The change has not been reviewed."]);
    assert_eq!(got.checks.expect("checks").review, ReviewCheck::None);
}

#[test]
fn an_edit_after_the_review_makes_it_stale_and_undoing_the_edit_brings_it_back() {
    let fx = ready("stale");
    write(&fx, "src/a.txt", "one\ntwo\n");
    let got = status(&fx);
    assert_eq!(
        got.blocked,
        [
            "Files or commits changed since the review. Review again.",
            "Files changed since the tests ran. Run them again.",
        ]
    );
    let checks = got.checks.expect("checks");
    assert_eq!(checks.review, ReviewCheck::Stale);
    assert_eq!(
        checks.tests,
        TestsCheck::Stale {
            command: "echo ok".into()
        }
    );
    write(&fx, "src/a.txt", "one\n");
    assert_eq!(status(&fx).blocked, Vec::<String>::new());
}

#[test]
fn a_review_with_blockers_says_how_many() {
    let fx = fixture("blockers");
    write(&fx, "src/a.txt", "one\n");
    reviewed(&fx, 1);
    let got = status(&fx);
    assert_eq!(got.blocked, ["The review found 1 blocker."]);
    assert_eq!(
        got.checks.expect("checks").review,
        ReviewCheck::Blockers { count: 1 }
    );
    reviewed(&fx, 2);
    assert_eq!(status(&fx).blocked, ["The review found 2 blockers."]);
}

#[test]
fn a_review_that_cannot_be_read_blocks_and_is_not_a_pass() {
    let fx = fixture("badreview");
    write(&fx, "src/a.txt", "one\n");
    record(&fx, "review.json", r#"{"project": "#);
    let got = status(&fx);
    assert_eq!(
        got.blocked,
        [format!(
            "The review can't be read: {}: EOF while parsing a value at line 1 column 12",
            fx.cfg
                .join("helper-work")
                .join(&fx.session)
                .join("review.json")
                .display()
        )]
    );
    assert_eq!(got.checks.expect("checks").review, ReviewCheck::None);
}

#[test]
fn a_commit_made_after_the_review_makes_it_stale_though_no_file_changed() {
    let fx = fixture("b1-after");
    write(&fx, "src/a.txt", "one\n");
    write(&fx, "notes.txt", "kept\n");
    reviewed(&fx, 0);
    let tree = ship_tree::tree_id(&fx.project).unwrap();
    // The working folder holds the same files, so its tree id is the same.
    commit(&fx, "notes.txt", "notes");
    assert_eq!(ship_tree::tree_id(&fx.project).unwrap(), tree);
    let got = status(&fx);
    assert_eq!(
        got.blocked,
        ["Files or commits changed since the review. Review again."]
    );
    assert_eq!(got.checks.expect("checks").review, ReviewCheck::Stale);
}

// -- gate 7: the tests --------------------------------------------------------

#[test]
fn a_project_with_no_test_command_is_not_blocked_and_says_so() {
    let fx = fixture("notests");
    write(&fx, "src/a.txt", "one\n");
    reviewed(&fx, 0);
    let got = status(&fx);
    assert_eq!(got.blocked, Vec::<String>::new());
    assert_eq!(got.checks.expect("checks").tests, TestsCheck::NoCommand);
}

#[test]
fn a_detected_test_command_that_has_not_run_blocks() {
    let fx = fixture("notrun");
    write(&fx, "Cargo.toml", "[package]\nname = \"my-app\"\n");
    reviewed(&fx, 0);
    let got = status(&fx);
    assert_eq!(got.blocked, ["The tests have not run on this change."]);
    assert_eq!(
        got.checks.expect("checks").tests,
        TestsCheck::NotRun {
            command: "cargo test".into()
        }
    );
}

#[test]
fn failed_tests_block_with_their_exit_code() {
    let fx = fixture("failed");
    write(&fx, "src/a.txt", "one\n");
    reviewed(&fx, 0);
    assert_eq!(run_tests(&fx, "exit 3").exit, 3);
    let got = status(&fx);
    assert_eq!(got.blocked, ["The tests failed (exit 3)."]);
    assert_eq!(
        got.checks.expect("checks").tests,
        TestsCheck::Failed {
            command: "exit 3".into(),
            exit: 3
        }
    );
}

#[test]
fn tests_that_change_a_file_do_not_count_as_a_run() {
    let fx = fixture("writes");
    write(&fx, "src/a.txt", "one\n");
    let done = run_tests(&fx, "echo made> made.txt");
    assert_eq!((done.exit, done.changed_files), (0, true));
    reviewed(&fx, 0);
    let got = status(&fx);
    assert_eq!(got.blocked, ["The tests have not run on this change."]);
    assert_eq!(
        got.checks.expect("checks").tests,
        TestsCheck::NotRun {
            command: "echo made> made.txt".into()
        }
    );
}

// -- gate 8: the secret scan --------------------------------------------------

#[test]
fn a_key_shaped_line_is_flagged_by_path_and_line_and_never_by_value() {
    let fx = fixture("shape");
    let key = "sk-abcdefghijklmnopqrstuv";
    write(
        &fx,
        "src/client.txt",
        &format!("first\nsecond\nkey = \"{key}\"\n"),
    );
    reviewed(&fx, 0);
    let got = status(&fx);
    assert_eq!(
        got.blocked,
        ["The secret scan flagged 1 place in the change."]
    );
    assert_eq!(
        got.checks.as_ref().expect("checks").secrets,
        Some(vec![hit(
            "src/client.txt",
            Some(3),
            SecretKind::Shape,
            None
        )])
    );
    let sent = serde_json::to_string(&got).unwrap();
    assert!(!sent.contains(key), "{sent}");
    assert!(!sent.contains("abcdefghij"), "{sent}");
}

#[test]
fn a_secrets_file_is_flagged_by_its_name_and_an_example_file_is_not() {
    let fx = fixture("names");
    write(&fx, ".env", "PORT=3000\n");
    write(&fx, ".env.example", "PORT=\n");
    write(&fx, "deploy/id_ed25519", "not a real key\n");
    write(&fx, "certs/site.pem", "not a real key\n");
    write(&fx, "config/auth.json", "{}\n");
    reviewed(&fx, 0);
    let got = status(&fx);
    assert_eq!(
        got.blocked,
        ["The secret scan flagged 4 places in the change."]
    );
    assert_eq!(
        got.checks.expect("checks").secrets,
        Some(vec![
            hit(".env", None, SecretKind::Name, None),
            hit("certs/site.pem", None, SecretKind::Name, None),
            hit("config/auth.json", None, SecretKind::Name, None),
            hit("deploy/id_ed25519", None, SecretKind::Name, None),
        ])
    );
}

#[test]
fn only_the_listed_names_are_secret_by_name() {
    for name in [
        ".env",
        ".env.local",
        "app/.ENV",
        "auth.json",
        "a/secrets.json",
        "tls/server.key",
        "tls/chain.pem",
        "id_rsa",
        "id_rsa.pub",
        "keys/id_ed25519",
    ] {
        assert!(secret_name(name), "{name}");
    }
    for name in [
        ".env.example",
        "src/env.rs",
        "auth.json.md",
        "secrets.rs",
        "keyboard.txt",
        "docs/pem.md",
        "rsa/notes.txt",
    ] {
        assert!(!secret_name(name), "{name}");
    }
}

#[test]
fn a_line_that_holds_one_of_the_projects_secrets_is_flagged_as_held() {
    let fx = fixture("held");
    let vault = MemVault::default();
    let value = "plain-words-nobody-would-flag";
    secrets::set(
        &fx.cfg,
        &vault,
        &runs::project_key(&fx.project),
        "DEPLOY_TOKEN",
        value,
    )
    .unwrap();
    write(&fx, "src/a.txt", &format!("one\nsend {value} along\n"));
    reviewed(&fx, 0);
    let got = status_with(
        &fx.cfg,
        &fx.project,
        &fx.session,
        &CodeShip::default(),
        &vault,
    );
    assert_eq!(
        got.blocked,
        ["The secret scan flagged 1 place in the change."]
    );
    assert_eq!(
        got.checks.as_ref().expect("checks").secrets,
        Some(vec![hit("src/a.txt", Some(2), SecretKind::Held, None)])
    );
    assert!(!serde_json::to_string(&got).unwrap().contains(value));
    // Without the keychain value the same line is plain text.
    assert_eq!(status(&fx).blocked, Vec::<String>::new());
}

#[test]
fn an_origin_address_changed_since_the_session_began_is_in_the_way_and_hides_no_record() {
    let fx = fixture("origin-changed");
    let vault = MemVault::default();
    let value = "plain-words-nobody-would-flag";
    secrets::set(
        &fx.cfg,
        &vault,
        &runs::project_key(&fx.project),
        "DEPLOY_TOKEN",
        value,
    )
    .unwrap();
    write(&fx, "src/a.txt", &format!("one\nsend {value} along\n"));
    reviewed(&fx, 0);
    assert_eq!(run_tests(&fx, "exit 3").exit, 3);
    // The repository's config now names another address, under which this
    // project has no secrets and no test run on record.
    git(
        &fx.project,
        &["remote", "add", "origin", "https://example.com/other.git"],
    );
    let got = status_with(
        &fx.cfg,
        &fx.project,
        &fx.session,
        &CodeShip::default(),
        &vault,
    );
    assert_eq!(
        got.blocked,
        [
            "This project's origin address changed since this session began; ship from your \
             terminal.",
            "The tests failed (exit 3).",
            "The secret scan flagged 1 place in the change.",
        ]
    );
    let checks = got.checks.expect("checks");
    assert_eq!(
        checks.tests,
        TestsCheck::Failed {
            command: "exit 3".into(),
            exit: 3
        }
    );
    assert_eq!(
        checks.secrets,
        Some(vec![hit("src/a.txt", Some(2), SecretKind::Held, None)])
    );
}

#[test]
fn a_session_that_began_with_an_origin_finds_its_records_under_that_address() {
    let fx = bare_fixture("origin-kept");
    git(
        &fx.project,
        &["remote", "add", "origin", "https://example.com/my-app.git"],
    );
    git(&fx.project, &["switch", "-c", "work/one"]);
    begin(&fx);
    write(&fx, "src/a.txt", "one\n");
    reviewed(&fx, 0);
    assert_eq!(run_tests(&fx, "exit 0").exit, 0);
    let got = status(&fx);
    assert_eq!(got.blocked, Vec::<String>::new());
    assert_eq!(
        got.checks.expect("checks").tests,
        TestsCheck::Passed {
            command: "exit 0".into()
        }
    );
}

#[test]
fn a_secrets_file_committed_before_the_review_is_flagged_in_its_commit() {
    let fx = fixture("b1-before");
    write(&fx, "src/a.txt", "one\n");
    write(&fx, ".env", "PORT=3000\n");
    let sha = commit(&fx, ".env", "config");
    // The review lists the commit, so it is of this change.
    reviewed(&fx, 0);
    let got = status(&fx);
    assert_eq!(
        got.blocked,
        ["The secret scan flagged 1 place in the change."]
    );
    let checks = got.checks.expect("checks");
    assert_eq!(checks.review, ReviewCheck::Clean);
    assert_eq!(
        checks.secrets,
        Some(vec![hit(".env", None, SecretKind::Name, Some(&sha))])
    );
}

#[test]
fn a_key_one_commit_added_and_the_next_removed_is_still_found() {
    let fx = fixture("removed");
    write(
        &fx,
        "src/k.txt",
        "a\ntoken = \"sk-abcdefghijklmnopqrstuv\"\n",
    );
    let added = commit(&fx, "src/k.txt", "add");
    write(&fx, "src/k.txt", "a\n");
    commit(&fx, "src/k.txt", "remove");
    write(&fx, "src/a.txt", "one\n");
    reviewed(&fx, 0);
    let got = status(&fx);
    assert_eq!(
        got.blocked,
        ["The secret scan flagged 1 place in the change."]
    );
    assert_eq!(
        got.checks.expect("checks").secrets,
        Some(vec![hit(
            "src/k.txt",
            Some(2),
            SecretKind::Shape,
            Some(&added)
        )])
    );
}

// -- gate 9: files git status hides -------------------------------------------

#[test]
fn a_changed_file_marked_skip_worktree_is_blocked_by_name() {
    let fx = fixture("hidden");
    git(&fx.project, &["update-index", "--skip-worktree", "README"]);
    write(&fx, "README", "changed\n");
    assert_eq!(git(&fx.project, &["status", "--porcelain"]), "");
    reviewed(&fx, 0);
    assert_eq!(
        status(&fx).blocked,
        [
            "README is marked skip-worktree or assume-unchanged and would be committed; \
             commit from your terminal."
        ]
    );
}

// -- gate 10: the author ------------------------------------------------------

#[test]
fn a_repository_with_no_author_or_that_signs_its_commits_is_blocked() {
    let fx = ready("author");
    git(&fx.project, &["config", "user.email", ""]);
    assert_eq!(
        status(&fx).blocked,
        ["Set user.name and user.email in git before you commit."]
    );
    git(&fx.project, &["config", "user.email", "test@example.com"]);
    git(&fx.project, &["config", "commit.gpgsign", "true"]);
    assert_eq!(
        status(&fx).blocked,
        ["This repository signs its commits, which the app cannot do; commit from your terminal."]
    );
}

// -- gate 11: the message -----------------------------------------------------

fn draft(message: &str, allowed: &[&str]) -> Draft {
    Draft {
        message: Some(message.to_string()),
        allowed: allowed.iter().map(|place| place.to_string()).collect(),
    }
}

/// How the page allows `place` while the folder is as it is now.
fn allowed_now(fx: &Fixture, place: &str) -> String {
    allow_key(place, &ship_tree::tree_id(&fx.project).unwrap())
}

fn status_of(fx: &Fixture, draft: &Draft) -> ShipStatus {
    status_for(
        &fx.cfg,
        &fx.project,
        &fx.session,
        &CodeShip::default(),
        &MemVault::default(),
        draft,
    )
}

fn commit_of(
    fx: &Fixture,
    draft: &Draft,
    confirmed: bool,
    command: Option<&str>,
) -> Result<Committed, String> {
    commit_with(
        &fx.cfg,
        &fx.project,
        &fx.session,
        &CodeShip::default(),
        &MemVault::default(),
        draft,
        confirmed,
        command,
    )
}

/// The confirm text the status offers for `draft`.
fn confirm_of(fx: &Fixture, draft: &Draft) -> String {
    let got = status_of(fx, draft);
    assert_eq!(got.blocked, Vec::<String>::new());
    got.commit.expect("a plan").confirm.expect("a confirm")
}

fn head(fx: &Fixture) -> String {
    git(&fx.project, &["rev-parse", "HEAD"])
}

fn shipped(fx: &Fixture) -> Option<Shipped> {
    let _config = ConfigGuard::set(&fx.cfg);
    review::shipped_for(&fx.session).unwrap()
}

#[test]
fn a_message_with_no_first_line_a_long_one_or_a_nul_is_refused() {
    let fx = ready("message");
    for (message, why) in [
        ("", "Write a commit message."),
        ("   \nbody", "Write a commit message."),
        ("a\0b", "The commit message holds a NUL character."),
    ] {
        assert_eq!(
            status_of(&fx, &draft(message, &[])).blocked,
            [why],
            "{message:?}"
        );
    }
    let long = "x".repeat(201);
    assert_eq!(
        status_of(&fx, &draft(&long, &[])).blocked,
        ["The first line of the commit message is longer than 200 characters."]
    );
    let most = "é".repeat(200);
    assert_eq!(
        status_of(&fx, &draft(&format!("{most}\n\nbody"), &[])).blocked,
        Vec::<String>::new()
    );
}

// -- the commit ---------------------------------------------------------------

#[test]
fn a_commit_advances_head_by_one_with_the_reviewed_tree_and_leaves_a_clean_status() {
    let fx = ready("commit");
    let before = head(&fx);
    let message = draft("Add the total\n\nWith a test.\n", &[]);
    let confirm = confirm_of(&fx, &message);
    assert_eq!(
        confirm,
        format!(
            "commit {} to work/one on {}: \"Add the total\"",
            &READY_TREE[..12],
            &before[..8]
        )
    );

    let done = commit_of(&fx, &message, true, Some(&confirm)).unwrap();
    let sha = head(&fx);
    assert_eq!(done.sha, sha);
    assert_eq!(done.said, format!("Committed {} to work/one.", &sha[..8]));
    assert_eq!(
        git(
            &fx.project,
            &["rev-list", "--count", &format!("{before}..HEAD")]
        ),
        "1"
    );
    assert_eq!(git(&fx.project, &["rev-parse", "HEAD^{tree}"]), READY_TREE);
    assert_eq!(git(&fx.project, &["rev-parse", "HEAD^"]), before);
    assert_eq!(
        git(&fx.project, &["log", "-1", "--format=%s / %b"]),
        "Add the total / With a test."
    );
    assert_eq!(git(&fx.project, &["status", "--porcelain"]), "");

    let review_head = before.clone();
    assert_eq!(
        shipped(&fx),
        Some(Shipped {
            project: fx.project.to_string_lossy().into_owned(),
            branch: "work/one".into(),
            sha: sha.clone(),
            parent: review_head,
            tree: READY_TREE.into(),
            base: before.clone(),
            commits: vec![sha.clone()],
            allowed: vec![],
            pushed: false,
        })
    );
    // After it: nothing to commit, the review is of what was committed, and
    // the page names the commit.
    assert_eq!(done.status.blocked, ["There is nothing to commit."]);
    assert_eq!(done.status.shipped, Some(sha.clone()));
    let checks = done.status.checks.clone().expect("checks");
    assert_eq!(checks.review, ReviewCheck::Clean);
    assert_eq!(
        checks.tests,
        TestsCheck::Passed {
            command: "echo ok".into()
        }
    );
    assert_eq!(status(&fx), done.status);

    let log = code_activity::list(&fx.cfg, 10);
    assert_eq!(log.len(), 1);
    assert_eq!(log[0].kind, code_activity::Kind::Commit);
    assert_eq!(log[0].command, confirm);
    assert_eq!(log[0].approved_by, "user");
    assert_eq!(log[0].branch, "work/one");
    assert_eq!(log[0].host, "other");
    assert_eq!(log[0].exit, Some(0));
    assert_eq!(log[0].summary, format!("Committed {}", &sha[..8]));
}

#[test]
fn a_commit_with_no_yes_or_with_another_confirm_text_commits_nothing() {
    let fx = ready("unconfirmed");
    let before = head(&fx);
    let first = draft("First", &[]);
    let confirm = confirm_of(&fx, &first);
    assert_eq!(
        commit_of(&fx, &first, false, Some(&confirm)),
        Err("Confirm the commit first.".to_string())
    );
    assert_eq!(
        commit_of(&fx, &first, true, None),
        Err("The change is not the one the confirm showed. Look again.".to_string())
    );
    // A yes given for one message is no yes for another.
    assert_eq!(
        commit_of(&fx, &draft("Second", &[]), true, Some(&confirm)),
        Err("The change is not the one the confirm showed. Look again.".to_string())
    );
    // A yes given before a file changed is no yes for the files there now.
    write(&fx, "src/a.txt", "one\ntwo\n");
    write(&fx, "src/a.txt", "one\n");
    write(&fx, "src/b.txt", "two\n");
    assert_eq!(
        commit_of(&fx, &first, true, Some(&confirm)),
        Err("Files or commits changed since the review. Review again. \
             Files changed since the tests ran. Run them again."
            .to_string())
    );
    assert_eq!(head(&fx), before);
    assert_eq!(shipped(&fx), None);
    assert!(code_activity::list(&fx.cfg, 10).is_empty());
}

#[test]
fn a_change_that_is_in_the_way_commits_nothing_and_says_why() {
    let fx = fixture("blocked-commit");
    write(&fx, "src/a.txt", "one\n");
    let before = head(&fx);
    assert_eq!(
        commit_of(&fx, &Draft::default(), true, Some("commit")),
        Err("The change has not been reviewed.".to_string())
    );
    assert_eq!(head(&fx), before);
    assert_eq!(git(&fx.project, &["status", "--porcelain"]), "?? src/");
}

#[test]
fn a_commit_made_after_the_confirm_was_shown_blocks_the_commit_and_moves_nothing() {
    let fx = ready("moved");
    write(&fx, "notes.txt", "kept\n");
    reviewed(&fx, 0);
    assert_eq!(run_tests(&fx, "echo ok").exit, 0);
    let confirm = confirm_of(&fx, &Draft::default());
    // The agent commits by its own route while the person reads the card.
    let theirs = commit(&fx, "notes.txt", "agent");
    assert_eq!(
        commit_of(&fx, &Draft::default(), true, Some(&confirm)),
        Err("Files or commits changed since the review. Review again.".to_string())
    );
    assert_eq!(head(&fx), theirs);
    assert_eq!(shipped(&fx), None);
}

/// Attack B1: a commit of `.env` made with `commit-tree` and `update-ref`
/// after the review, which leaves the working folder as it was.
#[test]
fn a_secrets_file_put_on_the_branch_by_plumbing_after_the_review_blocks_as_stale() {
    let fx = ready("b1-plumbing");
    let confirm = confirm_of(&fx, &Draft::default());
    let blob = git_in(&fx.project, &["hash-object", "-w", "--stdin"], "TOKEN=1\n");
    let index = fx.base.join("other-index");
    let plumb = |args: &[&str]| {
        let out = Command::new("git")
            .arg("-C")
            .arg(&fx.project)
            .args(args)
            .env("GIT_INDEX_FILE", &index)
            .output()
            .unwrap();
        assert!(out.status.success(), "{args:?}");
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };
    plumb(&["read-tree", "HEAD"]);
    plumb(&[
        "update-index",
        "--add",
        "--cacheinfo",
        &format!("100644,{blob},.env"),
    ]);
    let tree = plumb(&["write-tree"]);
    let sneaked = plumb(&["commit-tree", &tree, "-p", "HEAD", "-m", "config"]);
    plumb(&["update-ref", "refs/heads/work/one", &sneaked]);

    let got = status(&fx);
    assert!(
        got.blocked
            .contains(&"Files or commits changed since the review. Review again.".to_string()),
        "{:?}",
        got.blocked
    );
    assert!(got
        .checks
        .expect("checks")
        .secrets
        .expect("a scan")
        .contains(&hit(".env", None, SecretKind::Name, Some(&sneaked))),);
    assert!(commit_of(&fx, &Draft::default(), true, Some(&confirm)).is_err());
    assert_eq!(head(&fx), sneaked);
}

#[test]
fn earlier_session_commits_are_listed_counted_in_the_confirm_and_kept_for_the_push() {
    let fx = fixture("earlier");
    write(&fx, "notes.txt", "kept\n");
    let earlier = commit(&fx, "notes.txt", "Keep the notes");
    write(&fx, "src/a.txt", "one\n");
    reviewed(&fx, 0);
    let got = status(&fx);
    let plan = got.commit.expect("a plan");
    assert_eq!(
        plan.commits,
        [CommitRow {
            sha: earlier.clone(),
            subject: "Keep the notes".into()
        }]
    );
    let confirm = plan.confirm.expect("a confirm");
    assert!(
        confirm.ends_with(": \"Change made in this session\", 1 earlier commit, no tests run"),
        "{confirm}"
    );
    let done = commit_of(&fx, &Draft::default(), true, Some(&confirm)).unwrap();
    let kept = shipped(&fx).expect("a record");
    assert_eq!(kept.commits, [done.sha.clone(), earlier.clone()]);
    assert_eq!(kept.parent, earlier);
    assert_eq!(done.status.blocked, ["There is nothing to commit."]);
    assert_eq!(
        done.status.checks.expect("checks").review,
        ReviewCheck::Clean
    );
}

// -- waive and allow ----------------------------------------------------------

#[test]
fn a_waived_blocker_opens_the_commit_and_is_named_in_the_confirm_and_the_log() {
    let fx = fixture("waive");
    write(&fx, "src/a.txt", "one\n");
    reviewed(&fx, 1);
    assert_eq!(status(&fx).blocked, ["The review found 1 blocker."]);
    let tree = ship_tree::tree_id(&fx.project).unwrap();
    let waive_at = |tree: &str, at_ms: u64| {
        waive(
            &fx.cfg,
            &fx.project,
            &fx.session,
            &CodeShip::default(),
            tree,
            at_ms,
            0,
        )
    };
    assert_eq!(
        waive_at(&"0".repeat(40), REVIEWED_AT),
        Err("The review changed since it was shown. Look again.".to_string())
    );
    // The same tree, but the time of another review.
    assert_eq!(
        waive_at(&tree, REVIEWED_AT - 1),
        Err("The review changed since it was shown. Look again.".to_string())
    );
    waive_at(&tree, REVIEWED_AT).unwrap();
    // A second click, a retry or a double click, logs nothing more.
    waive_at(&tree, REVIEWED_AT).unwrap();

    let got = status(&fx);
    assert_eq!(got.blocked, Vec::<String>::new());
    assert_eq!(got.checks.expect("checks").review, ReviewCheck::Clean);
    let plan = got.commit.expect("a plan");
    assert_eq!(plan.waived, 1);
    let confirm = plan.confirm.expect("a confirm");
    assert!(
        confirm.ends_with(": \"Change made in this session\", 1 finding waived, no tests run"),
        "{confirm}"
    );
    let log = code_activity::list(&fx.cfg, 10);
    assert_eq!(log.len(), 1);
    assert_eq!(log[0].kind, code_activity::Kind::Review);
    assert_eq!(log[0].approved_by, "user");
    assert_eq!(log[0].branch, "work/one");
    assert_eq!(
        log[0].command,
        format!("waive finding 1 of the review of {}", &tree[..12])
    );
    assert_eq!(
        log[0].summary,
        "Waived: src/a.txt:1 the total is off by one"
    );

    // A new review takes the waiver back.
    reviewed(&fx, 1);
    assert_eq!(status(&fx).blocked, ["The review found 1 blocker."]);
}

#[test]
fn a_shape_hit_the_person_allowed_is_named_in_the_confirm_and_stays_allowed_in_the_commit() {
    let fx = fixture("allow");
    write(
        &fx,
        "src/client.txt",
        "first\nsecond\nkey = \"sk-abcdefghijklmnopqrstuv\"\n",
    );
    reviewed(&fx, 0);
    let key = allowed_now(&fx, "src/client.txt:3");
    let allowed = draft("Add the client", &[&key]);
    let got = status_of(&fx, &allowed);
    assert_eq!(got.blocked, Vec::<String>::new());
    let shape = SecretHit {
        allowed: true,
        ..hit("src/client.txt", Some(3), SecretKind::Shape, None)
    };
    assert_eq!(
        got.checks.as_ref().expect("checks").secrets,
        Some(vec![shape.clone()])
    );
    assert_eq!(
        serde_json::to_string(&shape).unwrap(),
        r#"{"path":"src/client.txt","line":3,"kind":"shape","commit":null,"allowed":true}"#
    );
    let plan = got.commit.expect("a plan");
    assert_eq!(plan.allowed, 1);
    let confirm = plan.confirm.expect("a confirm");
    assert!(
        confirm.ends_with(": \"Add the client\", 1 flagged line allowed, no tests run"),
        "{confirm}"
    );
    // Another line's place allows nothing, and neither does the place
    // with no tree id or with another one.
    for other in [
        allowed_now(&fx, "src/client.txt:2"),
        "src/client.txt:3".to_string(),
        allow_key("src/client.txt:3", &"0".repeat(40)),
    ] {
        assert_eq!(
            status_of(&fx, &draft("Add the client", &[&other])).blocked,
            ["The secret scan flagged 1 place in the change."],
            "{other}"
        );
    }

    let done = commit_of(&fx, &allowed, true, Some(&confirm)).unwrap();
    assert_eq!(
        shipped(&fx).expect("a record").allowed,
        [AllowedLine {
            commit: done.sha.clone(),
            place: "src/client.txt:3".into()
        }]
    );
    // The line is in the commit now, and stays allowed there.
    assert_eq!(done.status.blocked, ["There is nothing to commit."]);
    assert_eq!(
        done.status.checks.expect("checks").secrets,
        Some(vec![SecretHit {
            allowed: true,
            ..hit(
                "src/client.txt",
                Some(3),
                SecretKind::Shape,
                Some(&done.sha)
            )
        }])
    );
    let log = code_activity::list(&fx.cfg, 10);
    assert_eq!(
        log[0].summary,
        format!("Committed {}; allowed src/client.txt:3", &done.sha[..8])
    );
    assert!(!log[0].command.contains("abcdefghij"));
}

#[test]
fn a_secrets_file_a_held_secret_and_a_shape_hit_in_a_session_commit_are_never_allowed() {
    let fx = fixture("never-allowed");
    write(&fx, ".env", "PORT=3000\n");
    reviewed(&fx, 0);
    let key = allowed_now(&fx, ".env");
    assert_eq!(
        status_of(&fx, &draft("x", &[&key])).blocked,
        ["The secret scan flagged 1 place in the change."]
    );

    let fx = fixture("never-allowed-commit");
    write(
        &fx,
        "src/k.txt",
        "a\ntoken = \"sk-abcdefghijklmnopqrstuv\"\n",
    );
    commit(&fx, "src/k.txt", "add");
    write(&fx, "src/a.txt", "one\n");
    reviewed(&fx, 0);
    let key = allowed_now(&fx, "src/k.txt:2");
    assert_eq!(
        status_of(&fx, &draft("x", &[&key])).blocked,
        ["The secret scan flagged 1 place in the change."]
    );

    let fx = fixture("never-allowed-held");
    let vault = MemVault::default();
    let value = "plain-words-nobody-would-flag";
    secrets::set(
        &fx.cfg,
        &vault,
        &runs::project_key(&fx.project),
        "DEPLOY_TOKEN",
        value,
    )
    .unwrap();
    write(&fx, "src/a.txt", &format!("one\nsend {value} along\n"));
    reviewed(&fx, 0);
    let key = allowed_now(&fx, "src/a.txt:2");
    let got = status_for(
        &fx.cfg,
        &fx.project,
        &fx.session,
        &CodeShip::default(),
        &vault,
        &draft("x", &[&key]),
    );
    assert_eq!(
        got.blocked,
        ["The secret scan flagged 1 place in the change."]
    );
}

// -- what an Allow and a message are bound to ---------------------------------

/// Trust M3, correctness m1, quality M1: an Allow given for one value on a
/// line does not allow another value written on that line later, after a
/// new review, though the page sends the same allowed list again.
#[test]
fn an_allow_given_for_one_value_does_not_allow_another_written_on_that_line() {
    let fx = fixture("allow-bound");
    write(&fx, "src/a.txt", "key = \"sk-abcdefghijklmnopqrstuv\"\n");
    reviewed(&fx, 0);
    let key = allowed_now(&fx, "src/a.txt:1");
    let allowed = draft("Add", &[&key]);
    assert_eq!(status_of(&fx, &allowed).blocked, Vec::<String>::new());

    write(
        &fx,
        "src/a.txt",
        "key = \"ghp_ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789\"\n",
    );
    reviewed(&fx, 0);
    let got = status_of(&fx, &allowed);
    assert_eq!(
        got.blocked,
        ["The secret scan flagged 1 place in the change."]
    );
    assert_eq!(
        got.checks.expect("checks").secrets,
        Some(vec![hit("src/a.txt", Some(1), SecretKind::Shape, None)])
    );
    assert_eq!(got.commit.expect("a plan").confirm, None);
    let before = head(&fx);
    assert_eq!(
        commit_of(&fx, &allowed, true, Some("commit")),
        Err("The secret scan flagged 1 place in the change.".to_string())
    );
    assert_eq!(head(&fx), before);
    assert_eq!(shipped(&fx), None);
}

/// Correctness M1: a line allowed in the commit made here stays allowed in
/// that commit after more files change, so the session's next change can
/// be committed; and it stays allowed after that second commit too.
#[test]
fn a_line_allowed_in_the_commit_made_here_stays_allowed_after_more_changes() {
    let fx = fixture("allow-later");
    write(
        &fx,
        "src/client.txt",
        "first\nsecond\nkey = \"sk-abcdefghijklmnopqrstuv\"\n",
    );
    reviewed(&fx, 0);
    let first = draft("Add the client", &[&allowed_now(&fx, "src/client.txt:3")]);
    let confirm = confirm_of(&fx, &first);
    let one = commit_of(&fx, &first, true, Some(&confirm)).unwrap();

    write(&fx, "src/b.txt", "two\n");
    reviewed(&fx, 0);
    let got = status(&fx);
    assert_eq!(got.blocked, Vec::<String>::new());
    let allowed_in = |sha: &str| SecretHit {
        allowed: true,
        ..hit("src/client.txt", Some(3), SecretKind::Shape, Some(sha))
    };
    assert_eq!(
        got.checks.expect("checks").secrets,
        Some(vec![allowed_in(&one.sha)])
    );
    let confirm = got.commit.expect("a plan").confirm.expect("a confirm");
    assert!(
        confirm.ends_with(": \"Change made in this session\", 1 earlier commit, no tests run"),
        "{confirm}"
    );
    let two = commit_of(&fx, &Draft::default(), true, Some(&confirm)).unwrap();
    assert_eq!(
        shipped(&fx).expect("a record").allowed,
        [AllowedLine {
            commit: one.sha.clone(),
            place: "src/client.txt:3".into()
        }]
    );
    assert_eq!(two.status.blocked, ["There is nothing to commit."]);
    assert_eq!(
        two.status.checks.expect("checks").secrets,
        Some(vec![allowed_in(&one.sha)])
    );
}

/// Correctness m2: the confirm's subject is the committed subject, to the
/// character.
#[test]
fn the_committed_message_is_the_one_the_confirm_names() {
    let fx = ready("message-tidy");
    let message = draft("   Fix it  \n\nWhy it was wrong.   \n\n", &[]);
    let confirm = confirm_of(&fx, &message);
    assert!(confirm.ends_with(": \"Fix it\""), "{confirm}");
    assert_eq!(
        status_of(&fx, &message).commit.expect("a plan").message,
        "Fix it\n\nWhy it was wrong."
    );
    commit_of(&fx, &message, true, Some(&confirm)).unwrap();
    assert_eq!(
        git(&fx.project, &["log", "-1", "--format=%B"]),
        "Fix it\n\nWhy it was wrong."
    );
    assert_eq!(git(&fx.project, &["log", "-1", "--format=%s"]), "Fix it");
}
