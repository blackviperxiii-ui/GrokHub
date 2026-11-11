use super::*;
use grokhub_code::ship::Hosts;

fn plan(action: Action, command: &str, needs_confirm: bool, blocked: Option<&str>) -> Plan {
    Plan {
        action,
        branch: Some("code/fix".into()),
        command: command.into(),
        needs_confirm,
        blocked: blocked.map(str::to_string),
        mode: ShipMode::Ask,
        auto_base: "beta".into(),
        app_made: true,
        pr_url: None,
    }
}

const PUSH_CMD: &str = "git push --porcelain origin HEAD:refs/heads/code/fix";
const PR_CMD: &str = "gh pr create --base beta --head code/fix --fill";

fn plans(push: Plan, pr: Plan, then: Option<Action>) -> ShipOut {
    ShipOut::Plans {
        push: Box::new(push),
        pr: Box::new(pr),
        cfg: CodeShip::default(),
        then,
    }
}

#[test]
fn the_mode_and_protected_lines_read_the_shared_settings() {
    let cfg = CodeShip::default();
    assert_eq!(
        mode_note(&cfg),
        "Ask: every push and PR shows the exact command first."
    );
    assert_eq!(
        protected_line(&cfg),
        "Protected: main, and the repo's default branch. PRs open into beta."
    );
    let auto = CodeShip {
        mode: ShipMode::AutoBeta,
        auto_base: "next".into(),
        extra_protected: vec!["main".into(), "release".into()],
        ..CodeShip::default()
    };
    assert_eq!(
        mode_note(&auto),
        "Auto-beta: a branch made here pushes into next without asking. Everything else asks."
    );
    assert_eq!(
        protected_line(&auto),
        "Protected: main, release, and the repo's default branch. PRs open into next."
    );
}

#[test]
fn the_settings_come_from_the_shell_file_and_are_never_written() {
    let dir = std::env::temp_dir().join(format!("cabin-ship-settings-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    assert_eq!(settings(&dir), CodeShip::default());
    let raw = r#"{"codeShip":{"mode":"autoBeta","autoBase":"next"}}"#;
    std::fs::write(dir.join("shell.json"), raw).unwrap();
    let got = settings(&dir);
    assert_eq!(got.mode, ShipMode::AutoBeta);
    assert_eq!(got.auto_base, "next");
    assert_eq!(
        std::fs::read_to_string(dir.join("shell.json")).unwrap(),
        raw
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_blocked_push_shows_why_and_asks_nothing() {
    let mut p = ShipPanel::default();
    assert_eq!(
        p.press(Action::Push),
        Some(ShipJob::Refresh {
            then: Some(Action::Push)
        })
    );
    // A second press while the first reads is ignored.
    assert_eq!(p.press(Action::Push), None);
    let why =
        "main is the main or default branch. The app never pushes to it; make a branch first.";
    let next = p.apply(plans(
        plan(Action::Push, PUSH_CMD, true, Some(why)),
        plan(Action::Pr, PR_CMD, true, None),
        Some(Action::Push),
    ));
    assert_eq!(next, None);
    assert_eq!(p.note.as_deref(), Some(why));
    assert_eq!(p.confirming, None);
    assert!(!p.busy);
}

#[test]
fn ask_mode_shows_the_exact_command_and_runs_only_that_one() {
    let mut p = ShipPanel::default();
    p.press(Action::Pr);
    let next = p.apply(plans(
        plan(Action::Push, PUSH_CMD, true, None),
        plan(Action::Pr, PR_CMD, true, None),
        Some(Action::Pr),
    ));
    assert_eq!(next, None);
    assert_eq!(
        p.confirming.as_ref().map(|c| c.command.as_str()),
        Some(PR_CMD)
    );
    assert_eq!(
        p.confirm(),
        Some(ShipJob::Run {
            action: Action::Pr,
            confirmed: true,
            expected: Some(PR_CMD.into()),
        })
    );
    assert_eq!(p.confirming, None);
    // Cancel drops the card with nothing run.
    let mut p = ShipPanel::default();
    p.press(Action::Push);
    p.apply(plans(
        plan(Action::Push, PUSH_CMD, true, None),
        plan(Action::Pr, PR_CMD, true, None),
        Some(Action::Push),
    ));
    p.cancel();
    assert_eq!(p.confirm(), None);
}

#[test]
fn auto_beta_runs_without_a_confirm_only_when_the_guard_says_so() {
    let mut p = ShipPanel::default();
    p.press(Action::Push);
    let next = p.apply(plans(
        plan(Action::Push, PUSH_CMD, false, None),
        plan(Action::Pr, PR_CMD, false, None),
        Some(Action::Push),
    ));
    assert_eq!(
        next,
        Some(ShipJob::Run {
            action: Action::Push,
            confirmed: false,
            expected: None,
        })
    );
    assert_eq!(p.confirming, None);
    assert!(p.busy);
}

#[test]
fn a_confirm_for_a_command_that_changed_goes_away() {
    let mut p = ShipPanel::default();
    p.press(Action::Push);
    p.apply(plans(
        plan(Action::Push, PUSH_CMD, true, None),
        plan(Action::Pr, PR_CMD, true, None),
        Some(Action::Push),
    ));
    assert!(p.confirming.is_some());
    // The branch was switched elsewhere; the tab shows again.
    assert_eq!(p.shown(), Some(ShipJob::Refresh { then: None }));
    p.apply(plans(
        plan(
            Action::Push,
            "git push --porcelain origin HEAD:refs/heads/other",
            true,
            None,
        ),
        plan(Action::Pr, PR_CMD, true, None),
        None,
    ));
    assert_eq!(p.confirming, None);
    assert!(!p.busy);
}

#[test]
fn a_result_or_an_error_shows_then_reads_again() {
    let mut p = ShipPanel {
        busy: true,
        ..ShipPanel::default()
    };
    let next = p.apply(ShipOut::Shipped(Err(
        "This needs a confirmation first.".into()
    )));
    assert_eq!(next, Some(ShipJob::Refresh { then: None }));
    assert_eq!(p.note.as_deref(), Some("This needs a confirmation first."));
    // The tab reads the repo and its activity again after any try.
    assert!(p.changed);
    let next = p.apply(ShipOut::Branch(Ok("code/fix".into())));
    assert_eq!(next, Some(ShipJob::Refresh { then: None }));
    assert_eq!(p.note.as_deref(), Some("On code/fix now."));
    assert_eq!(p.branch_name, "");
}

#[cfg(unix)]
mod repo {
    use super::*;
    use grokhub_code::activity::{self, Kind};
    use std::process::Command;

    fn sh_git(dir: &Path, args: &[&str]) {
        let out = Command::new("git")
            .args([
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@example.invalid",
                "-c",
                "init.defaultBranch=main",
            ])
            .args(args)
            .current_dir(dir)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    struct Fixture {
        root: PathBuf,
        bare: PathBuf,
        seed: PathBuf,
        work: PathBuf,
        config: PathBuf,
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    fn fixture(tag: &str) -> Fixture {
        let root = std::env::temp_dir().join(format!("cabin-ship-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let bare = root.join("origin.git");
        let seed = root.join("seed");
        let work = root.join("work");
        let config = root.join("config");
        std::fs::create_dir_all(&seed).unwrap();
        std::fs::create_dir_all(&config).unwrap();
        sh_git(
            &root,
            &["init", "--bare", "-b", "main", bare.to_str().unwrap()],
        );
        sh_git(&seed, &["init", "-b", "main"]);
        std::fs::write(seed.join("a.txt"), "one\n").unwrap();
        sh_git(&seed, &["add", "a.txt"]);
        sh_git(&seed, &["commit", "-m", "one"]);
        sh_git(&seed, &["push", bare.to_str().unwrap(), "main"]);
        sh_git(
            &root,
            &["clone", bare.to_str().unwrap(), work.to_str().unwrap()],
        );
        Fixture {
            root,
            bare,
            seed,
            work,
            config,
        }
    }

    /// The local bare origin is not GitHub; let pushes to it through.
    fn other_hosts_on(config: &Path) {
        std::fs::write(
            config.join("shell.json"),
            r#"{"codeShip":{"hosts":{"github":true,"other":true}}}"#,
        )
        .unwrap();
    }

    fn commit(dir: &Path, text: &str) {
        std::fs::write(dir.join("a.txt"), text).unwrap();
        sh_git(dir, &["commit", "-am", text]);
    }

    fn none() -> Vec<String> {
        Vec::new()
    }

    fn read(f: &Fixture) -> (Plan, Plan, Vec<Entry>) {
        match (ShipJob::Refresh { then: None }).run(&f.config, &f.work, &none) {
            ShipOut::Plans { push, pr, .. } => (*push, *pr, activity::list(&f.config, 10)),
            other => panic!("expected plans, got {other:?}"),
        }
    }

    #[test]
    fn main_is_never_pushed_from_the_cabin() {
        let _online = grokhub_core::offline::force_for_thread(false);
        let f = fixture("main");
        other_hosts_on(&f.config);
        commit(&f.work, "two\n");
        let (push, pr, _) = read(&f);
        let why =
            "main is the main or default branch. The app never pushes to it; make a branch first.";
        assert_eq!(push.blocked.as_deref(), Some(why));
        assert_eq!(pr.blocked.as_deref(), Some(why));
        // Even a confirmed run with the shown command is refused, and logged.
        let out = ShipJob::Run {
            action: Action::Push,
            confirmed: true,
            expected: Some(push.command.clone()),
        }
        .run(&f.config, &f.work, &none);
        assert_eq!(out, ShipOut::Shipped(Err(why.into())));
        let (_, _, activity) = read(&f);
        assert_eq!(activity.first().map(|e| e.kind), Some(Kind::Blocked));
        assert_eq!(
            activity.first().map(|e| e.approved_by.as_str()),
            Some("none")
        );
    }

    #[test]
    fn a_pr_opens_into_beta_from_a_branch_made_here() {
        let _online = grokhub_core::offline::force_for_thread(false);
        let f = fixture("pr");
        let out = (ShipJob::NewBranch {
            name: "code/fix".into(),
        })
        .run(&f.config, &f.work, &none);
        assert_eq!(out, ShipOut::Branch(Ok("code/fix".into())));
        let (push, pr, _) = read(&f);
        assert_eq!(
            pr.command,
            "gh pr create --base beta --head code/fix --fill"
        );
        assert_eq!(
            push.command,
            "git push --porcelain origin HEAD:refs/heads/code/fix"
        );
        // A non-GitHub origin is off by default, so nothing runs.
        assert_eq!(
            push.blocked.as_deref(),
            Some("Pushing to this git host is off in Settings → Code.")
        );
        // The PR base can't be made a branch to work on.
        let out = (ShipJob::NewBranch {
            name: "beta".into(),
        })
        .run(&f.config, &f.work, &none);
        assert_eq!(
            out,
            ShipOut::Branch(Err(
                "beta is the PR base. Pick another name for your work.".into()
            ))
        );
    }

    #[test]
    fn a_push_needs_the_confirm_for_the_same_command() {
        let _online = grokhub_core::offline::force_for_thread(false);
        let f = fixture("confirm");
        other_hosts_on(&f.config);
        sh_git(&f.work, &["switch", "-c", "code/fix"]);
        commit(&f.work, "two\n");
        let (push, _, _) = read(&f);
        assert!(push.needs_confirm);
        assert_eq!(push.blocked, None);
        let unconfirmed = ShipJob::Run {
            action: Action::Push,
            confirmed: false,
            expected: None,
        }
        .run(&f.config, &f.work, &none);
        assert_eq!(
            unconfirmed,
            ShipOut::Shipped(Err("This needs a confirmation first.".into()))
        );
        let stale = ShipJob::Run {
            action: Action::Push,
            confirmed: true,
            expected: Some("git push --porcelain origin HEAD:refs/heads/other".into()),
        }
        .run(&f.config, &f.work, &none);
        assert_eq!(
            stale,
            ShipOut::Shipped(Err(
                "The branch changed since you confirmed. Check the command and confirm again."
                    .into()
            ))
        );
        let done = ShipJob::Run {
            action: Action::Push,
            confirmed: true,
            expected: Some(push.command),
        }
        .run(&f.config, &f.work, &none);
        match done {
            ShipOut::Shipped(Ok((summary, _))) => assert_eq!(summary, "Pushed code/fix to origin."),
            other => panic!("expected a push, got {other:?}"),
        }
        let out = Command::new("git")
            .args(["rev-parse", "--verify", "--quiet", "refs/heads/code/fix"])
            .current_dir(&f.bare)
            .output()
            .unwrap();
        assert!(out.status.success(), "origin has the branch now");
    }

    #[test]
    fn a_rejected_push_shows_a_clean_redacted_message() {
        let _online = grokhub_core::offline::force_for_thread(false);
        let f = fixture("rejected");
        other_hosts_on(&f.config);
        // origin gets a commit on the branch this clone doesn't have.
        sh_git(&f.seed, &["switch", "-c", "code/fix"]);
        commit(&f.seed, "theirs\n");
        sh_git(&f.seed, &["push", f.bare.to_str().unwrap(), "code/fix"]);
        sh_git(&f.work, &["switch", "-c", "code/fix"]);
        commit(&f.work, "mine\n");
        let (push, _, _) = read(&f);
        // A held project secret that happens to show in git's output is hidden.
        let held = || vec!["code/fix".to_string()];
        let out = ShipJob::Run {
            action: Action::Push,
            confirmed: true,
            expected: Some(push.command),
        }
        .run(&f.config, &f.work, &held);
        let ShipOut::Shipped(Ok((summary, output))) = out else {
            panic!("expected a finished push, got {out:?}");
        };
        assert_eq!(
            summary,
            "Rejected: origin has commits this branch doesn't. Pull them first; the app never force-pushes."
        );
        assert!(output.contains("[rejected]"), "{output}");
        assert!(!output.contains("code/fix"), "{output}");
        assert!(output.contains("[redacted]"), "{output}");
    }

    #[test]
    fn held_secrets_are_hidden_in_plans_and_activity() {
        let _online = grokhub_core::offline::force_for_thread(false);
        let f = fixture("held");
        sh_git(&f.work, &["switch", "-c", "tok-9f8e7d6c"]);
        let held = || vec!["tok-9f8e7d6c".to_string()];
        let out = (ShipJob::Run {
            action: Action::Push,
            confirmed: true,
            expected: None,
        })
        .run(&f.config, &f.work, &held);
        assert_eq!(
            out,
            ShipOut::Shipped(Err(
                "Pushing to this git host is off in Settings → Code.".into()
            ))
        );
        let ShipOut::Plans { push, .. } =
            (ShipJob::Refresh { then: None }).run(&f.config, &f.work, &held)
        else {
            panic!("expected plans");
        };
        let activity: Vec<Entry> = activity::list(&f.config, 10)
            .into_iter()
            .map(|e| clean_entry(e, &held()))
            .collect();
        assert_eq!(
            push.command,
            "git push --porcelain origin HEAD:refs/heads/[redacted]"
        );
        let blocked = activity.first().expect("the blocked try is logged");
        assert_eq!(blocked.branch, "[redacted]");
        assert!(!blocked.command.contains("tok-9f8e7d6c"));
    }

    #[test]
    fn hosts_default_keeps_other_hosts_off() {
        assert_eq!(
            CodeShip::default().hosts,
            Hosts {
                github: true,
                other: false
            }
        );
    }
}
