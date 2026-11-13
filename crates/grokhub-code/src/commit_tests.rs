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
        at_ms: 1_700_000_000_000,
        plan_given: true,
        body,
        blockers,
        cut: false,
    };
    record(fx, "review.json", &serde_json::to_string(&review).unwrap());
}

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
    }
}

// -- the whole status ---------------------------------------------------------

#[test]
fn a_reviewed_and_tested_change_on_a_work_branch_has_nothing_in_the_way() {
    let fx = ready("clean");
    let got = status(&fx);
    assert_eq!(
        got,
        ShipStatus {
            blocked: vec![],
            checks: Some(Checks {
                branch: Some("work/one".into()),
                review: ReviewCheck::Clean,
                tests: TestsCheck::Passed {
                    command: "echo ok".into()
                },
                secrets: Some(vec![]),
            }),
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
    assert_eq!(got.checks.expect("checks").branch, None);
}

#[test]
fn with_no_default_branch_on_record_only_a_branch_the_app_made_takes_a_commit() {
    let fx = ready("nodefault");
    git(
        &fx.project,
        &["symbolic-ref", "--delete", "refs/remotes/origin/HEAD"],
    );
    assert_eq!(
        status(&fx).blocked,
        ["This project's default branch is not known. Use New branch."]
    );
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
