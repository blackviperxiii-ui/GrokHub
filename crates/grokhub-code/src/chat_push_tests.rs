use super::*;

fn refused(command: &str) -> String {
    match read(command) {
        Read::Refused(why) => why,
        other => panic!("{command}: {other:?}"),
    }
}

fn branch(name: &str) -> Target {
    Target::Branch(name.into())
}

#[test]
fn a_force_push_is_refused_in_every_form() {
    for command in [
        "git push --force origin code/x",
        "git push -f origin code/x",
        "git push -uf origin code/x",
        "git push --force-with-lease origin code/x",
        "git push --force-with-lease=code/x:abc origin code/x",
        "git push --force-if-includes origin code/x",
        "git push origin +code/x",
        "git push origin +HEAD:code/x",
        "cargo test && git push --force origin code/x",
    ] {
        assert_eq!(
            refused(command),
            "Force-pushing is never allowed, in any mode. Push without it, or make a new branch.",
            "{command}"
        );
    }
}

#[test]
fn deleting_or_mirroring_the_remote_is_refused() {
    for command in [
        "git push origin --delete code/x",
        "git push -d origin code/x",
        "git push origin :code/x",
        "git push --mirror origin",
        "git push --prune origin code/x",
    ] {
        assert_eq!(
            refused(command),
            "Deleting a branch on the remote isn't allowed from chat.",
            "{command}"
        );
    }
}

#[test]
fn only_the_plain_form_reaches_the_guard() {
    assert_eq!(
        read("git push origin code/x"),
        Read::Pushes(vec![branch("code/x")])
    );
    assert_eq!(
        read("git push -u origin code/x"),
        Read::Pushes(vec![branch("code/x")])
    );
    assert_eq!(
        read("git push origin HEAD"),
        Read::Pushes(vec![Target::Current])
    );
    assert_eq!(
        read("git push origin HEAD:main"),
        Read::Pushes(vec![branch("main")])
    );
    assert_eq!(
        read("git push origin HEAD:refs/heads/main"),
        Read::Pushes(vec![branch("main")])
    );
    assert_eq!(
        read("git add -A && git commit -m \"fix: push the right thing\" && git push origin code/x"),
        Read::Pushes(vec![branch("code/x")])
    );
    assert_eq!(read("git status && cargo test"), Read::NoPush);
    assert_eq!(read("git log --grep push"), Read::NoPush);
    assert_eq!(read("cargo test git_push"), Read::NoPush);
    assert_eq!(
        refused("git push"),
        "Name the branch: git push origin <branch>."
    );
    assert_eq!(
        refused("git push origin"),
        "Name the branch: git push origin <branch>."
    );
    assert_eq!(
        refused("git push upstream code/x"),
        "Push to origin only; the guard doesn't check upstream."
    );
    assert_eq!(
        refused("git push https://example.invalid/r.git code/x"),
        "Push to origin only; the guard doesn't check https://example.invalid/r.git."
    );
    assert_eq!(
        refused("git push origin code/x code/y"),
        "Push one branch at a time: git push origin <branch>."
    );
    assert_eq!(
        refused("git push --all origin"),
        "git push --all isn't a push the guard can check. Name the branch: git push origin <branch>."
    );
    assert_eq!(
        refused("git push origin refs/tags/v1"),
        "This command pushes in a way the push guard can't read. Run git push origin <branch> on its own."
    );
    for command in [
        "git -C ../other push origin code/x",
        "git -c push.default=matching push origin code/x",
        "sudo git push origin code/x",
        "bash -c \"git push --force origin main\"",
        "echo $(git push origin main)",
        "GIT_DIR=../x/.git git push origin code/x",
        "cd ../other && git push origin main",
    ] {
        assert_eq!(
            refused(command),
            "This command pushes in a way the push guard can't read. Run git push origin <branch> on its own.",
            "{command}"
        );
    }
}

#[cfg(unix)]
mod repo {
    use super::*;
    use crate::ship::{Host, Pr, ShipMode};
    use std::path::{Path, PathBuf};
    use std::process::Command;

    fn sh_git(dir: &Path, args: &[&str]) -> String {
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

    fn fixture(tag: &str) -> Fixture {
        let root = std::env::temp_dir().join(format!("chat-push-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let bare = root.join("origin.git");
        let work = root.join("work");
        let config = root.join("config");
        std::fs::create_dir_all(&config).unwrap();
        std::fs::create_dir_all(&root).unwrap();
        sh_git(
            &root,
            &["init", "--bare", "-b", "main", bare.to_str().unwrap()],
        );
        sh_git(
            &root,
            &["clone", bare.to_str().unwrap(), work.to_str().unwrap()],
        );
        std::fs::write(work.join("a.txt"), "one\n").unwrap();
        sh_git(&work, &["add", "a.txt"]);
        sh_git(&work, &["commit", "-m", "one"]);
        sh_git(&work, &["push", "origin", "main"]);
        sh_git(&work, &["remote", "set-head", "origin", "main"]);
        Fixture {
            root,
            bare,
            work,
            config,
        }
    }

    fn origin_sha(f: &Fixture, branch: &str) -> Option<String> {
        let out = Command::new("git")
            .args([
                "rev-parse",
                "--verify",
                "--quiet",
                &format!("refs/heads/{branch}"),
            ])
            .current_dir(&f.bare)
            .output()
            .unwrap();
        out.status
            .success()
            .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
    }

    /// What the agent's tool call does: runs only on `Run`, or on `Ask`
    /// when the person says yes.
    fn agent_runs(f: &Fixture, cfg: &CodeShip, command: &str, yes: bool) -> Option<Verdict> {
        let probe = Some((Host::GitHub, Pr::None));
        let verdict = decide_with(&f.config, &f.work, cfg, command, probe);
        let runs = match &verdict {
            Some(Verdict::Run) => true,
            Some(Verdict::Ask) => yes,
            Some(Verdict::Refuse(_)) => false,
            None => true,
        };
        if runs {
            let out = Command::new("sh")
                .args(["-c", command])
                .current_dir(&f.work)
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "{command}: {}",
                String::from_utf8_lossy(&out.stderr)
            );
        }
        verdict
    }

    fn auto() -> CodeShip {
        CodeShip {
            mode: ShipMode::AutoBeta,
            ..CodeShip::default()
        }
    }

    #[test]
    fn a_push_to_main_from_chat_is_refused_and_origin_main_stays() {
        let f = fixture("main");
        let before = origin_sha(&f, "main").unwrap();
        std::fs::write(f.work.join("a.txt"), "two\n").unwrap();
        sh_git(&f.work, &["commit", "-am", "two"]);
        let main =
            "main is the main or default branch. The app never pushes to it; make a branch first.";
        for cfg in [CodeShip::default(), auto()] {
            for command in [
                "git push origin main",
                "git push -u origin main",
                "git push origin HEAD",
                "git push origin HEAD:main",
                "git push origin HEAD:refs/heads/main",
            ] {
                assert_eq!(
                    agent_runs(&f, &cfg, command, true),
                    Some(Verdict::Refuse(main.into())),
                    "{command}"
                );
            }
        }
        assert_eq!(origin_sha(&f, "main"), Some(before));
        let log = std::fs::read_to_string(f.config.join(crate::activity::FILE)).unwrap();
        assert!(log.contains("\"kind\":\"blocked\""), "{log}");
        assert!(
            log.contains("From chat: main is the main or default branch."),
            "{log}"
        );
    }

    #[test]
    fn a_force_push_from_chat_is_refused_and_the_branch_stays() {
        let f = fixture("force");
        sh_git(&f.work, &["checkout", "-b", "code/fix"]);
        std::fs::write(f.work.join("a.txt"), "two\n").unwrap();
        sh_git(&f.work, &["commit", "-am", "two"]);
        sh_git(&f.work, &["push", "origin", "code/fix"]);
        let pushed = origin_sha(&f, "code/fix").unwrap();
        // History rewritten locally; only a force would land it.
        sh_git(&f.work, &["reset", "--hard", "HEAD~1"]);
        std::fs::write(f.work.join("a.txt"), "other\n").unwrap();
        sh_git(&f.work, &["commit", "-am", "other"]);
        for command in [
            "git push --force origin code/fix",
            "git push -f origin code/fix",
            "git push --force-with-lease origin code/fix",
            "git push origin +code/fix",
        ] {
            assert_eq!(
                agent_runs(&f, &auto(), command, true),
                Some(Verdict::Refuse(FORCE.into())),
                "{command}"
            );
        }
        assert_eq!(origin_sha(&f, "code/fix"), Some(pushed));
    }

    #[test]
    fn an_app_made_branch_runs_with_no_card_in_auto_beta_and_asks_in_ask() {
        let f = fixture("auto");
        let made = ship::new_branch(&f.config, &f.work, &CodeShip::default(), "code/fix").unwrap();
        assert_eq!(made, "code/fix");
        std::fs::write(f.work.join("a.txt"), "two\n").unwrap();
        sh_git(&f.work, &["commit", "-am", "two"]);
        assert_eq!(
            agent_runs(&f, &CodeShip::default(), "git push origin code/fix", false),
            Some(Verdict::Ask)
        );
        assert_eq!(
            origin_sha(&f, "code/fix"),
            None,
            "Ask without a yes pushes nothing"
        );
        assert_eq!(
            agent_runs(&f, &auto(), "git push -u origin code/fix", false),
            Some(Verdict::Run)
        );
        assert_eq!(
            origin_sha(&f, "code/fix").as_deref(),
            Some(sh_git(&f.work, &["rev-parse", "HEAD"]).as_str())
        );
        // A branch made by hand still asks in Auto-beta.
        sh_git(&f.work, &["checkout", "-b", "by-hand"]);
        assert_eq!(
            agent_runs(&f, &auto(), "git push origin by-hand", false),
            Some(Verdict::Ask)
        );
        assert_eq!(agent_runs(&f, &auto(), "git status", false), None);
    }
}
