use super::*;

fn facts(branch: &str, default: Option<&str>, app_made: bool, pr: Pr) -> Facts {
    Facts {
        branch: branch.into(),
        default_branch: default.map(str::to_string),
        app_made,
        host: Host::GitHub,
        pr,
    }
}

fn auto() -> CodeShip {
    CodeShip {
        mode: ShipMode::AutoBeta,
        ..CodeShip::default()
    }
}

fn into_beta() -> Pr {
    Pr::Open {
        base: "beta".into(),
        url: "https://github.com/o/r/pull/1".into(),
    }
}

#[test]
fn defaults_are_ask_github_only_and_main_protected() {
    let cfg = CodeShip::default();
    assert_eq!(cfg.mode, ShipMode::Ask);
    assert_eq!(cfg.auto_base, "beta");
    assert_eq!(
        cfg.hosts,
        Hosts {
            github: true,
            other: false
        }
    );
    assert_eq!(cfg.extra_protected, vec!["main".to_string()]);
    assert_eq!(cfg.activity_keep, 2000);
    assert_eq!(
        serde_json::to_string(&cfg).unwrap(),
        r#"{"mode":"ask","autoBase":"beta","hosts":{"github":true,"other":false},"extraProtected":["main"],"activityKeep":2000}"#
    );
}

#[test]
fn an_unreadable_block_loads_as_ask() {
    #[derive(Deserialize)]
    struct Wrap {
        #[serde(deserialize_with = "lenient")]
        ship: CodeShip,
    }
    let w: Wrap = serde_json::from_str(r#"{"ship":{"mode":"yolo"}}"#).unwrap();
    assert_eq!(w.ship.mode, ShipMode::Ask);
    let w: Wrap =
        serde_json::from_str(r#"{"ship":{"mode":"autoBeta","activityKeep":"lots"}}"#).unwrap();
    assert_eq!(w.ship, CodeShip::default());
    let w: Wrap = serde_json::from_str(r#"{"ship":{"mode":"autoBeta"}}"#).unwrap();
    assert_eq!(w.ship.mode, ShipMode::AutoBeta);
    assert_eq!(w.ship.auto_base, "beta");
}

#[test]
fn main_and_the_default_branch_are_never_pushed_in_any_mode() {
    for cfg in [CodeShip::default(), auto()] {
        assert_eq!(
            guard(&cfg, &facts("main", Some("main"), true, Pr::None)),
            Err(Blocked::DefaultBranch)
        );
        assert_eq!(
            guard(&cfg, &facts("trunk", Some("trunk"), true, Pr::None)),
            Err(Blocked::DefaultBranch)
        );
        // main stays protected even when the settings list is emptied.
        let bare = CodeShip {
            extra_protected: vec![],
            ..cfg.clone()
        };
        assert_eq!(
            guard(&bare, &facts("main", Some("trunk"), true, Pr::None)),
            Err(Blocked::DefaultBranch)
        );
        assert_eq!(
            guard(&bare, &facts("main", None, true, Pr::None)),
            Err(Blocked::DefaultBranch)
        );
    }
    let extra = CodeShip {
        extra_protected: vec!["release".into()],
        ..auto()
    };
    assert_eq!(
        guard(&extra, &facts("release", Some("main"), true, Pr::None)),
        Err(Blocked::Protected)
    );
}

#[test]
fn auto_beta_skips_the_confirm_only_for_an_app_branch_into_beta() {
    let cfg = auto();
    assert_eq!(
        guard(
            &cfg,
            &facts("code/fix-typo", Some("main"), true, into_beta())
        ),
        Ok(Go::NoConfirm)
    );
    assert_eq!(
        guard(&cfg, &facts("code/fix-typo", Some("main"), true, Pr::None)),
        Ok(Go::NoConfirm)
    );
    assert_eq!(
        guard(
            &cfg,
            &facts("code/fix-typo", Some("main"), false, into_beta())
        ),
        Ok(Go::Confirm)
    );
    assert_eq!(
        guard(
            &cfg,
            &facts(
                "code/fix-typo",
                Some("main"),
                true,
                Pr::Open {
                    base: "release".into(),
                    url: String::new()
                }
            )
        ),
        Ok(Go::Confirm)
    );
    assert_eq!(
        guard(&cfg, &facts("code/fix-typo", None, true, into_beta())),
        Ok(Go::Confirm)
    );
    assert_eq!(
        guard(
            &cfg,
            &facts("code/fix-typo", Some("main"), true, Pr::Unknown)
        ),
        Ok(Go::Confirm)
    );
    let mut other = facts("code/fix-typo", Some("main"), true, Pr::None);
    other.host = Host::Other;
    let both = CodeShip {
        hosts: Hosts {
            github: true,
            other: true,
        },
        ..auto()
    };
    assert_eq!(guard(&both, &other), Ok(Go::Confirm));
    // The auto base itself is pushed only with a confirm.
    assert_eq!(
        guard(&cfg, &facts("beta", Some("main"), true, Pr::None)),
        Ok(Go::Confirm)
    );
}

#[test]
fn ask_mode_confirms_every_push() {
    let cfg = CodeShip::default();
    assert_eq!(
        guard(
            &cfg,
            &facts("code/fix-typo", Some("main"), true, into_beta())
        ),
        Ok(Go::Confirm)
    );
    assert_eq!(
        guard(&cfg, &facts("code/fix-typo", Some("main"), true, Pr::None)),
        Ok(Go::Confirm)
    );
}

#[test]
fn hosts() {
    assert_eq!(host_of("git@github.com:o/r.git"), Host::GitHub);
    assert_eq!(host_of("https://github.com/o/r"), Host::GitHub);
    assert_eq!(host_of("ssh://git@github.com/o/r.git"), Host::GitHub);
    assert_eq!(host_of("https://user:tok@github.com/o/r"), Host::GitHub);
    assert_eq!(host_of("https://gitlab.com/o/r"), Host::Other);
    assert_eq!(host_of("https://github.com.evil.example/o/r"), Host::Other);
    assert_eq!(host_of("/srv/git/r.git"), Host::Other);
    assert_eq!(host_of("https://evil.example/x@github.com/y"), Host::Other);
    assert_eq!(host_of("evil.example:x/y@github.com/z"), Host::Other);
    for other in [
        "evil.example:x@github.com/z",
        "git@evil.example:x@github.com/z",
        "https://evil.example#@github.com/o/r",
        "https://evil.example?@github.com/o/r",
        "github.com/o/r",
        "file://github.com/srv/r.git",
    ] {
        assert_eq!(host_of(other), Host::Other, "{other}");
    }
    for github in [
        "ssh://git@github.com:22/o/r",
        "https://user:p@ss@github.com/o/r",
        "https://GitHub.com/o/r",
    ] {
        assert_eq!(host_of(github), Host::GitHub, "{github}");
    }
    // github.com in front of the `@`: git, ssh or curl connect to the host
    // after it. git also decodes `%2f` in an ssh or git URL before it looks
    // for the host, so the host there ends at the decoded `/`.
    let read_as_github: Vec<&str> = [
        "https://github.com\\@evil.example/o/r",
        "http://github.com\\@evil.example/o/r",
        "ssh://github.com?@evil.example/o/r",
        "ssh://github.com#@evil.example/o/r",
        "ssh://github.com\\x@evil.example/o/r",
        "ssh://git@github.com?x@evil.example:2222/o/r",
        "ssh://evil.example%2f@github.com/o/r",
        "ssh://git%40evil.example%2fx@github.com/o/r",
        "git://evil.example%2f@github.com/o/r",
        "git+ssh://evil.example%2F@github.com/o/r",
    ]
    .into_iter()
    .filter(|url| host_of(url) != Host::Other)
    .collect();
    assert_eq!(read_as_github, Vec::<&str>::new());
    // The other two names git has for ssh, and a sign-in name with an
    // encoded `@`, which curl leaves alone.
    let read_as_other: Vec<&str> = [
        "git+ssh://git@github.com/o/r.git",
        "ssh+git://git@github.com/o/r.git",
        "https://user%40example.com@github.com/o/r",
    ]
    .into_iter()
    .filter(|url| host_of(url) != Host::GitHub)
    .collect();
    assert_eq!(read_as_other, Vec::<&str>::new());
    let mut f = facts("code/x", Some("main"), true, Pr::None);
    f.host = Host::Other;
    assert_eq!(guard(&CodeShip::default(), &f), Err(Blocked::HostOff));
    assert_eq!(pr_guard(&CodeShip::default(), &f), Err(Blocked::HostOff));
    let on = CodeShip {
        hosts: Hosts {
            github: true,
            other: true,
        },
        ..CodeShip::default()
    };
    assert_eq!(pr_guard(&on, &f), Err(Blocked::PrNeedsGitHub));
}

#[test]
fn branch_names() {
    for bad in [
        "+main",
        "a:main",
        "-f",
        "--force",
        "a..b",
        "x@{1}",
        "has space",
        "",
        "a\u{7}b",
        "x.lock",
        "/x",
        "x/",
    ] {
        assert_eq!(branch_ok(bad), Err(Blocked::BadBranchName), "{bad:?}");
    }
    for good in ["code/fix-typo", "feature/x_1", "v2.0"] {
        assert_eq!(branch_ok(good), Ok(()), "{good:?}");
    }
}

#[test]
fn the_push_argv_never_forces() {
    let args = push_args("code/fix");
    assert_eq!(
        args,
        vec!["push", "--porcelain", "origin", "HEAD:refs/heads/code/fix"]
    );
    for a in &args {
        assert!(!a.starts_with('+'), "{a}");
        for flag in [
            "-f",
            "--force",
            "--force-with-lease",
            "--mirror",
            "--delete",
            "-d",
            "--prune",
        ] {
            assert_ne!(a, flag);
        }
    }
    assert_eq!(
        shown("git", &args),
        "git push --porcelain origin HEAD:refs/heads/code/fix"
    );
    assert_eq!(
        shown("gh", &pr_args("beta", "code/fix")),
        "gh pr create --base beta --head code/fix --fill"
    );
    assert_eq!(
        pr_view_args("feat/x"),
        vec![
            "pr",
            "view",
            "--json",
            "state,baseRefName,url",
            "--",
            "feat/x"
        ]
    );
    let src = include_str!("code_ship.rs");
    let prod = src.split("#[cfg(test)]").next().unwrap();
    for flag in [
        "\"--force",
        "\"-f\"",
        "\"--mirror\"",
        "\"--delete\"",
        "\"--prune\"",
        "\"branch\", \"-D\"",
        "\"reset\"",
    ] {
        assert!(!prod.contains(flag), "{flag} in code_ship.rs");
    }
}

#[test]
fn pr_guard_rules() {
    let cfg = auto();
    assert_eq!(
        pr_guard(&cfg, &facts("code/x", Some("main"), true, Pr::None)),
        Ok(Go::NoConfirm)
    );
    assert_eq!(
        pr_guard(&cfg, &facts("code/x", Some("main"), true, into_beta())),
        Err(Blocked::PrOpen)
    );
    assert_eq!(
        pr_guard(&cfg, &facts("code/x", Some("main"), true, Pr::Unknown)),
        Ok(Go::Confirm)
    );
    assert_eq!(
        pr_guard(&cfg, &facts("code/x", Some("main"), false, Pr::None)),
        Ok(Go::Confirm)
    );
    assert_eq!(
        pr_guard(
            &CodeShip::default(),
            &facts("code/x", Some("main"), true, Pr::None)
        ),
        Ok(Go::Confirm)
    );
    assert_eq!(
        pr_guard(&cfg, &facts("main", Some("main"), true, Pr::None)),
        Err(Blocked::DefaultBranch)
    );
}

#[test]
fn checked_settings() {
    let typed = CodeShip {
        auto_base: " beta ".into(),
        extra_protected: vec![" release ".into(), "".into(), "release".into()],
        activity_keep: 1,
        ..CodeShip::default()
    };
    let got = checked(typed).unwrap();
    assert_eq!(got.auto_base, "beta");
    assert_eq!(got.extra_protected, vec!["release".to_string()]);
    assert_eq!(got.activity_keep, 50);
    assert_eq!(
        checked(CodeShip {
            auto_base: "main".into(),
            ..CodeShip::default()
        }),
        Err("The PR base can't be main: main is never pushed to.".into())
    );
    assert_eq!(
        checked(CodeShip {
            extra_protected: vec!["+x".into()],
            ..CodeShip::default()
        }),
        Err("+x isn't a branch name git accepts.".into())
    );
}

#[test]
fn output_loses_url_credentials_and_keys() {
    assert_eq!(
        clean_output("To https://x-access-token:abcd1234XYZ@github.com/o/r.git\n"),
        "To https://[redacted]@github.com/o/r.git\n"
    );
    assert_eq!(
        clean_output("remote: token ghp_abcdefghijklmnop ok"),
        "remote: token [redacted] ok"
    );
    assert_eq!(
        clean_output("To github.com:o/r.git"),
        "To github.com:o/r.git"
    );
    assert!(rejected(
        " ! [rejected]        HEAD -> code/x (fetch first)"
    ));
    assert!(!rejected("=\tHEAD:refs/heads/code/x\t[up to date]"));
}

#[test]
fn pr_view_answers() {
    assert_eq!(
        parse_pr_view(
            true,
            r#"{"state":"OPEN","baseRefName":"beta","url":"https://github.com/o/r/pull/7"}"#,
            ""
        ),
        Pr::Open {
            base: "beta".into(),
            url: "https://github.com/o/r/pull/7".into()
        }
    );
    assert_eq!(
        parse_pr_view(
            true,
            r#"{"state":"MERGED","baseRefName":"beta","url":"u"}"#,
            ""
        ),
        Pr::None
    );
    assert_eq!(
        parse_pr_view(false, "", "no pull requests found for branch \"code/x\""),
        Pr::None
    );
    assert_eq!(parse_pr_view(false, "", "gh: not logged in"), Pr::Unknown);
}

#[cfg(unix)]
mod repo {
    use super::*;
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

    fn fixture(tag: &str) -> Fixture {
        let root = std::env::temp_dir().join(format!("code-ship-{tag}-{}", std::process::id()));
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
            work,
            config,
        }
    }

    fn commit(dir: &Path, text: &str) {
        std::fs::write(dir.join("a.txt"), text).unwrap();
        sh_git(dir, &["commit", "-am", text]);
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

    const GITHUB_NO_PR: Probe = Some((Host::GitHub, Pr::None));

    #[test]
    fn a_push_to_main_is_refused_and_origin_main_stays() {
        let f = fixture("main");
        assert_eq!(default_branch(&f.work).as_deref(), Some("main"));
        let before = origin_sha(&f, "main").unwrap();
        commit(&f.work, "two\n");
        for cfg in [CodeShip::default(), auto()] {
            let err = ship_with(
                &f.config,
                &f.work,
                &cfg,
                Action::Push,
                true,
                Some("git push --porcelain origin HEAD:refs/heads/main"),
                GITHUB_NO_PR,
            )
            .unwrap_err();
            assert_eq!(
                err,
                "main is the main or default branch. The app never pushes to it; make a branch first."
            );
        }
        assert_eq!(origin_sha(&f, "main").unwrap(), before);
        let log = code_activity::list(&f.config, 10);
        assert_eq!(log.len(), 2);
        assert!(log
            .iter()
            .all(|e| e.kind == ActKind::Blocked && e.approved_by == "none" && e.branch == "main"));
        let plan = plan_with(&f.config, &f.work, &auto(), Action::Push, GITHUB_NO_PR);
        assert_eq!(
            plan.blocked.as_deref(),
            Some("main is the main or default branch. The app never pushes to it; make a branch first.")
        );
        let _ = std::fs::remove_dir_all(&f.root);
    }

    #[test]
    fn an_app_branch_pushes_in_auto_beta_without_a_confirm() {
        let f = fixture("auto");
        let cfg = auto();
        assert_eq!(
            new_branch(&f.config, &f.work, &cfg, "code/fix").unwrap(),
            "code/fix"
        );
        assert_eq!(current_branch(&f.work).as_deref(), Some("code/fix"));
        commit(&f.work, "fix\n");
        let head = sh_git(&f.work, &["rev-parse", "HEAD"]);
        let plan = plan_with(&f.config, &f.work, &cfg, Action::Push, GITHUB_NO_PR);
        assert_eq!(
            plan.command,
            "git push --porcelain origin HEAD:refs/heads/code/fix"
        );
        assert!(!plan.needs_confirm);
        assert!(plan.app_made);
        assert_eq!(plan.blocked, None);
        let done = ship_with(
            &f.config,
            &f.work,
            &cfg,
            Action::Push,
            false,
            None,
            GITHUB_NO_PR,
        )
        .unwrap();
        assert_eq!(done.exit, 0, "{}", done.output);
        assert_eq!(done.summary, "Pushed code/fix to origin.");
        assert_eq!(origin_sha(&f, "code/fix").as_deref(), Some(head.as_str()));
        let log = code_activity::list(&f.config, 10);
        assert_eq!(log[0].kind, ActKind::Push);
        assert_eq!(log[0].approved_by, "autoBeta");
        assert_eq!(
            log[0].command,
            "git push --porcelain origin HEAD:refs/heads/code/fix"
        );
        assert_eq!(log[1].kind, ActKind::Branch);
        let _ = std::fs::remove_dir_all(&f.root);
    }

    #[test]
    fn a_hand_made_branch_and_ask_mode_need_a_confirm() {
        let f = fixture("ask");
        sh_git(&f.work, &["switch", "-c", "mine"]);
        commit(&f.work, "mine\n");
        assert_eq!(
            ship_with(
                &f.config,
                &f.work,
                &auto(),
                Action::Push,
                false,
                None,
                GITHUB_NO_PR
            )
            .unwrap_err(),
            "This needs a confirmation first."
        );
        assert_eq!(origin_sha(&f, "mine"), None);
        let done = ship_with(
            &f.config,
            &f.work,
            &CodeShip::default(),
            Action::Push,
            true,
            Some("git push --porcelain origin HEAD:refs/heads/mine"),
            GITHUB_NO_PR,
        )
        .unwrap();
        assert_eq!(done.exit, 0, "{}", done.output);
        assert_eq!(code_activity::list(&f.config, 1)[0].approved_by, "user");
        assert!(origin_sha(&f, "mine").is_some());
        let _ = std::fs::remove_dir_all(&f.root);
    }

    #[test]
    fn a_confirm_for_another_branch_pushes_nothing() {
        let f = fixture("stale");
        let cfg = CodeShip::default();
        sh_git(&f.work, &["switch", "-c", "code/a"]);
        commit(&f.work, "a\n");
        let seen = plan_with(&f.config, &f.work, &cfg, Action::Push, GITHUB_NO_PR);
        assert_eq!(
            seen.command,
            "git push --porcelain origin HEAD:refs/heads/code/a"
        );
        assert!(seen.needs_confirm);
        // The branch changes while the confirm for code/a is still on screen.
        sh_git(&f.work, &["switch", "-c", "code/b"]);
        for stale in [Some(seen.command.as_str()), None] {
            assert_eq!(
                ship_with(
                    &f.config,
                    &f.work,
                    &cfg,
                    Action::Push,
                    true,
                    stale,
                    GITHUB_NO_PR
                )
                .unwrap_err(),
                "The branch changed since you confirmed. Check the command and confirm again."
            );
        }
        assert_eq!(origin_sha(&f, "code/a"), None);
        assert_eq!(origin_sha(&f, "code/b"), None);
        let head = sh_git(&f.work, &["rev-parse", "HEAD"]);
        let done = ship_with(
            &f.config,
            &f.work,
            &cfg,
            Action::Push,
            true,
            Some("git push --porcelain origin HEAD:refs/heads/code/b"),
            GITHUB_NO_PR,
        )
        .unwrap();
        assert_eq!(done.exit, 0, "{}", done.output);
        assert_eq!(done.summary, "Pushed code/b to origin.");
        assert_eq!(origin_sha(&f, "code/b").as_deref(), Some(head.as_str()));
        assert_eq!(origin_sha(&f, "code/a"), None);
        let _ = std::fs::remove_dir_all(&f.root);
    }

    /// A confirm names one command. It must not push another branch even when
    /// that branch would have needed no confirm of its own.
    #[test]
    fn a_confirm_for_another_branch_does_not_push_a_no_confirm_branch() {
        let f = fixture("stale-auto");
        let cfg = auto();
        sh_git(&f.work, &["switch", "-c", "mine"]);
        commit(&f.work, "mine\n");
        let seen = plan_with(&f.config, &f.work, &cfg, Action::Push, GITHUB_NO_PR);
        assert_eq!(
            seen.command,
            "git push --porcelain origin HEAD:refs/heads/mine"
        );
        assert!(seen.needs_confirm);
        // An app-made branch is checked out while that confirm is on screen.
        assert_eq!(
            new_branch(&f.config, &f.work, &cfg, "code/fix").unwrap(),
            "code/fix"
        );
        commit(&f.work, "fix\n");
        assert_eq!(
            ship_with(
                &f.config,
                &f.work,
                &cfg,
                Action::Push,
                true,
                Some(seen.command.as_str()),
                GITHUB_NO_PR
            )
            .unwrap_err(),
            "The branch changed since you confirmed. Check the command and confirm again."
        );
        assert_eq!(origin_sha(&f, "mine"), None);
        assert_eq!(origin_sha(&f, "code/fix"), None);
        let _ = std::fs::remove_dir_all(&f.root);
    }

    /// `git push origin` sends to every push URL of origin, so one that
    /// isn't GitHub makes the push a push to another host.
    #[test]
    fn a_second_push_url_on_another_host_is_not_a_github_push() {
        let f = fixture("pushurls");
        let cfg = CodeShip::default();
        sh_git(&f.work, &["switch", "-c", "code/x"]);
        commit(&f.work, "x\n");
        sh_git(
            &f.work,
            &[
                "remote",
                "set-url",
                "--push",
                "origin",
                "git@github.com:o/r.git",
            ],
        );
        assert_eq!(push_host(&f.work), Some(Host::GitHub));
        sh_git(
            &f.work,
            &[
                "remote",
                "set-url",
                "--add",
                "--push",
                "origin",
                f.bare.to_str().unwrap(),
            ],
        );
        assert_eq!(push_host(&f.work), Some(Host::Other));
        let plan = plan_with(&f.config, &f.work, &cfg, Action::Push, None);
        assert_eq!(
            plan.blocked.as_deref(),
            Some("Pushing to this git host is off in Settings → Code.")
        );
        assert_eq!(
            ship_with(
                &f.config,
                &f.work,
                &cfg,
                Action::Push,
                true,
                Some("git push --porcelain origin HEAD:refs/heads/code/x"),
                None,
            )
            .unwrap_err(),
            "Pushing to this git host is off in Settings → Code."
        );
        assert_eq!(origin_sha(&f, "code/x"), None);
        let _ = std::fs::remove_dir_all(&f.root);
    }

    #[test]
    fn a_branch_named_like_a_flag_is_refused_before_gh_runs() {
        use std::os::unix::fs::PermissionsExt;
        let f = fixture("flag");
        sh_git(&f.work, &["update-ref", "refs/heads/--web", "HEAD"]);
        sh_git(&f.work, &["symbolic-ref", "HEAD", "refs/heads/--web"]);
        sh_git(
            &f.work,
            &[
                "remote",
                "set-url",
                "--push",
                "origin",
                "https://github.com/o/r.git",
            ],
        );
        // A stand-in gh, first on PATH, that leaves a mark when it runs.
        let bin = f.root.join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let gh = bin.join("gh");
        std::fs::write(&gh, "#!/bin/sh\necho ran > \"$0.ran\"\nexit 1\n").unwrap();
        std::fs::set_permissions(&gh, std::fs::Permissions::from_mode(0o755)).unwrap();
        let old = std::env::var_os("PATH").unwrap_or_default();
        let mut path = bin.clone().into_os_string();
        path.push(":");
        path.push(&old);
        // SAFETY: tests run one at a time in this process, so nothing else
        // reads the environment while PATH points at the stand-in.
        unsafe { std::env::set_var("PATH", &path) };
        let plan = plan_with(&f.config, &f.work, &CodeShip::default(), Action::Push, None);
        unsafe { std::env::set_var("PATH", &old) };
        assert_eq!(plan.branch.as_deref(), Some("--web"));
        assert_eq!(
            plan.blocked.as_deref(),
            Some("\"--web\" isn't a branch name the app pushes.")
        );
        assert!(!bin.join("gh.ran").exists(), "gh ran for a refused branch");
        let _ = std::fs::remove_dir_all(&f.root);
    }

    #[test]
    fn a_rejected_push_is_reported_and_never_forced() {
        let f = fixture("reject");
        let cfg = auto();
        new_branch(&f.config, &f.work, &cfg, "code/race").unwrap();
        commit(&f.work, "local\n");
        // Someone else pushed to the same branch first.
        let other = f.root.join("other");
        sh_git(
            &f.root,
            &["clone", f.bare.to_str().unwrap(), other.to_str().unwrap()],
        );
        sh_git(&other, &["switch", "-c", "code/race"]);
        commit(&other, "theirs\n");
        sh_git(&other, &["push", "origin", "code/race"]);
        let theirs = origin_sha(&f, "code/race").unwrap();
        let done = ship_with(
            &f.config,
            &f.work,
            &cfg,
            Action::Push,
            false,
            None,
            GITHUB_NO_PR,
        )
        .unwrap();
        assert_ne!(done.exit, 0);
        assert_eq!(
            done.summary,
            "Rejected: origin has commits this branch doesn't. Pull them first; the app never force-pushes."
        );
        assert_eq!(origin_sha(&f, "code/race").unwrap(), theirs);
        let _ = std::fs::remove_dir_all(&f.root);
    }

    #[test]
    fn new_branch_refuses_protected_names() {
        let f = fixture("names");
        let cfg = CodeShip::default();
        assert_eq!(
            new_branch(&f.config, &f.work, &cfg, "main").unwrap_err(),
            "main is the main or default branch. The app never pushes to it; make a branch first."
        );
        assert_eq!(
            new_branch(&f.config, &f.work, &cfg, "beta").unwrap_err(),
            "beta is the PR base. Pick another name for your work."
        );
        assert_eq!(
            new_branch(&f.config, &f.work, &cfg, "-f").unwrap_err(),
            "\"-f\" isn't a branch name the app pushes."
        );
        assert_eq!(current_branch(&f.work).as_deref(), Some("main"));
        assert!(!f.config.join(BRANCHES_FILE).exists());
        let _ = std::fs::remove_dir_all(&f.root);
    }

    #[test]
    fn a_detached_head_is_not_pushed() {
        let f = fixture("detached");
        let head = sh_git(&f.work, &["rev-parse", "HEAD"]);
        sh_git(&f.work, &["checkout", "--detach", &head]);
        let plan = plan_with(&f.config, &f.work, &auto(), Action::Push, GITHUB_NO_PR);
        assert_eq!(
            plan.blocked.as_deref(),
            Some("Check out a branch first; a detached HEAD isn't pushed.")
        );
        let _ = std::fs::remove_dir_all(&f.root);
    }
}
