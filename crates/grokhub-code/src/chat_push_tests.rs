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

/// The harness's hard class for the command as the agent's shell tool.
fn hard(command: &str) -> Option<grokhub_agent::harness::HardClass> {
    grokhub_agent::harness::hard_class(
        "run_terminal_command",
        &serde_json::json!({ "command": command }).to_string(),
    )
}

const SHIP: Option<grokhub_agent::harness::HardClass> =
    Some(grokhub_agent::harness::HardClass::Ship);

fn not_own(verb: &str) -> String {
    format!(
        "git {verb} isn't one of git's own commands, so it may be an alias that pushes, which the push guard can't read. Use git's own commands, and push with git push origin <branch>."
    )
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

#[test]
fn a_git_word_that_isnt_gits_own_command_is_refused() {
    assert_eq!(
        refused("git config alias.ship push && git ship --force origin main"),
        "git ship isn't one of git's own commands, so it may be an alias that pushes, which the push guard can't read. Use git's own commands, and push with git push origin <branch>."
    );
    assert_eq!(
        refused("git -c alias.p=push p origin main"),
        "git p isn't one of git's own commands, so it may be an alias that pushes, which the push guard can't read. Use git's own commands, and push with git push origin <branch>."
    );
    // Behind a wrapper, or a script given to a shell.
    for command in [
        "git config alias.ship push && env git ship --force origin main",
        "command git ship --force origin main",
        "nice -n 5 git ship --force origin main",
        "env GIT_TRACE=0 git ship --force origin main",
        "bash -c \"git ship --force origin main\"",
    ] {
        assert_eq!(
            refused(command),
            "git ship isn't one of git's own commands, so it may be an alias that pushes, which the push guard can't read. Use git's own commands, and push with git push origin <branch>.",
            "{command}"
        );
    }
    // git's own push program, named by its path.
    assert_eq!(
        refused("/usr/lib/git-core/git-push --force origin main"),
        "Force-pushing is never allowed, in any mode. Push without it, or make a new branch."
    );
    // git's own push programs this guard doesn't read.
    for command in ["git send-pack ../origin.git main", "git-push origin code/x"] {
        assert_eq!(
            refused(command),
            "This command pushes in a way the push guard can't read. Run git push origin <branch> on its own.",
            "{command}"
        );
    }
    assert_eq!(
        refused("bash -lc 'git ship --force origin main'"),
        not_own("ship")
    );
    // git's own commands that run a command, an option that names one, a
    // command saved for later or set in a variable git reads: not a push
    // this guard reads, so the harness parks it, a card in every mode.
    for command in [
        "git rebase -x 'git push --force origin main' HEAD~1",
        "git rebase --exec 'git ship' HEAD~1",
        "git submodule foreach git push --force origin main",
        "git bisect run sh -c 'git push -f origin main'",
        "git for-each-repo --config=maintenance.repo push -f origin main",
        "git -c core.editor='git ship' commit",
        "git rebase -x'git ship --force origin main' HEAD~1",
        "git for-each-repo --config=maintenance.repo ship -f origin main",
        "git grep -O'git push --force origin main #' -e agent",
        "git grep --open-files-in-pager='git push --force origin main #' agent",
        "git fetch --upload-pack='git push --force origin main #' .",
        "git fetch --upload='git ship' .",
        "git ls-remote --upload-pack 'git push --force origin main #' .",
        "git clone -u 'git push --force origin main #' . ../copy",
        "git archive --remote=. --exec='git push --force origin main #' HEAD",
        "git config core.editor 'git push --force origin main #' && git commit -q --allow-empty",
        "git config --global sequence.editor 'git ship --force origin main #'",
        "git config alias.st '!git push --force origin main'",
        "EDITOR='git push --force origin main #' git commit",
        "export GIT_SEQUENCE_EDITOR='git push --force origin main #'; git rebase -i HEAD~1",
        "PAGER='git ship --force origin main #'; git log",
    ] {
        assert_eq!(read(command), Read::NoPush, "{command}");
        assert_eq!(hard(command), SHIP, "{command}");
    }
    for command in [
        "git config alias.ship push",
        "git rebase -x 'cargo test' HEAD~1",
        "git submodule foreach git pull",
        "git -c color.ui=never log --oneline && git --no-pager diff",
        "git -C ../other status",
        "git -c \"user.name=a b\" commit -m x",
        "git help push",
        "git config core.editor 'code --wait'",
        "git config user.name \"Some User\"",
        "EDITOR='code --wait' git commit",
        "git grep -n -e agent",
        "git grep -o -e \"git push --force\" -- src",
        "git fetch --upload-pack=git-upload-pack origin",
        "git commit -m \"git ship: refuse an alias\"",
        "ls /usr/lib/git-core",
        "grep -rn git src/",
        "apt-get install -y git curl",
        "env git status",
    ] {
        assert_eq!(read(command), Read::NoPush, "{command}");
    }
}

/// git runs a `git-<name>` program before it looks for an alias, but only
/// on the command's own `PATH`: a command that changes it, or git's exec
/// path, can put an alias where git-lfs was. Only a command compiled into
/// git is its own then, however the change is spelled.
#[test]
fn an_alias_git_finds_after_a_path_change_is_refused() {
    let with_lfs = |verb: &str| matches!(verb, "config" | "status" | "lfs");
    for command in [
        "git config alias.lfs push && env PATH=/usr/lib/git-core git lfs --force origin main",
        "git config alias.lfs push && PATH=/usr/lib/git-core git lfs --force origin main",
        "PATH=/usr/lib/git-core; git lfs --force origin main",
        "export PATH=/usr/lib/git-core && git lfs --force origin main",
        "GIT_EXEC_PATH=/usr/lib/git-core git lfs --force origin main",
        "git --exec-path=/usr/lib/git-core lfs --force origin main",
        "env -i PATH=/usr/lib/git-core git lfs --force origin main",
    ] {
        assert_eq!(
            read_with(command, &with_lfs),
            Read::Refused(not_own("lfs")),
            "{command}"
        );
        assert_eq!(hard(command), SHIP, "{command}");
    }
    // With git's own `PATH`, git-lfs is git-lfs.
    assert_eq!(read_with("git lfs pull", &with_lfs), Read::NoPush);
    // A command compiled into git stays git's own after the change.
    assert_eq!(
        read_with("PATH=/usr/bin git status", &with_lfs),
        Read::NoPush
    );
}

/// Every way to reach an alias push the reviews found, behind a wrapper,
/// a runner, a pipe into a shell or an escape: each is refused here or
/// parked by the harness.
#[test]
fn an_alias_push_behind_any_wrapper_or_runner_is_refused_or_parked() {
    for command in [
        "git config alias.ship push && setsid -w git ship --force origin main",
        "setsid -w git ship --force origin main",
        "stdbuf -oL git ship --force origin main",
        "stdbuf -o 0 git ship --force origin main",
        "flock /tmp/l git ship --force origin main",
        "flock -w 5 /tmp/l -c 'git ship --force origin main'",
        "nohup git ship --force origin main",
        "timeout 30 git ship --force origin main",
        "timeout -k 5 30 git ship --force origin main",
        "bash -c 'git ship --force origin main'",
        "sudo -u user git ship --force origin main",
    ] {
        assert_eq!(read(command), Read::Refused(not_own("ship")), "{command}");
        assert_eq!(hard(command), SHIP, "{command}");
    }
    for command in [
        "setsid -w git push --force origin main",
        "chrt -o 0 git push --force origin main",
        "nohup git push origin main",
        "timeout 30 git push origin main",
        "bash -c 'git push --force origin main'",
        "xargs git push < args.txt",
    ] {
        assert_eq!(refused(command), UNCLEAR, "{command}");
        assert_eq!(hard(command), SHIP, "{command}");
    }
    for command in [
        "find . -maxdepth 0 -exec git ship --force origin main ';'",
        "echo 'git ship --force origin main' | sh",
        "echo git ship --force origin main | bash",
        "echo ship | xargs git",
        "printf 'git ship --force origin main' > ship.sh; sh ship.sh",
    ] {
        assert_eq!(read(command), Read::NoPush, "{command}");
        assert_eq!(hard(command), SHIP, "{command}");
    }
    // bash reads a backslash as an escape: `g\it` is git.
    if cfg!(unix) {
        assert_eq!(
            read("g\\it ship --force origin main"),
            Read::Refused(not_own("ship"))
        );
        assert_eq!(refused("g\\it push --force origin main"), FORCE);
        assert_eq!(refused("$'\\x67it' push -f origin main"), FORCE);
        assert_eq!(hard("g\\it ship --force origin main"), SHIP);
        assert_eq!(hard("g\\it push --force origin main"), SHIP);
    }
}

/// An editor that is a shell, a pager, a transport that runs a program and
/// any config but a few plain keys may run a push git can't show: the
/// harness parks the command, and a push after one is refused here.
#[test]
fn an_editor_a_transport_or_config_that_runs_a_program_is_parked() {
    for command in [
        "git config core.editor sh && git commit",
        "git -c core.editor=sh commit",
        "git -c core.editor=sh rebase -i HEAD~1",
        "EDITOR=sh git commit",
        "GIT_EDITOR=sh git commit",
        "VISUAL=sh git commit",
        "git config sequence.editor sh && git rebase -i HEAD~1",
        "git -c sequence.editor=sh rebase -i HEAD~1",
        "git --config-env=core.editor=ED commit",
        "git fetch 'ext::sh -c git% ship% --force% origin% main' main",
        "git clone fd::3 copy",
        "git -c core.pager=sh log",
        "git config include.path ../evil.cfg",
    ] {
        assert_eq!(read(command), Read::NoPush, "{command}");
        assert_eq!(hard(command), SHIP, "{command}");
    }
    for command in [
        "git config core.editor sh && git push origin code/fix",
        "git config remote.origin.push +refs/heads/code/fix:refs/heads/main && git push origin code/fix",
        "git config push.default upstream && git push origin code/fix",
        "EDITOR=sh; git push origin code/fix",
        "echo git | sh; git push origin code/fix",
    ] {
        assert_eq!(refused(command), UNCLEAR, "{command}");
        assert_eq!(hard(command), SHIP, "{command}");
    }
    // Plain keys are only data.
    for command in [
        "git -c user.name=user -c user.email=user@example.com commit -m x",
        "git config user.email user@example.com",
        "git config --get remote.origin.push",
    ] {
        assert_eq!(read(command), Read::NoPush, "{command}");
        assert_eq!(hard(command), None, "{command}");
    }
}

/// The commands an agent runs all day are neither refused nor parked,
/// whatever their quoted words say, and a plain push of an app branch
/// reaches the guard as that branch.
#[test]
fn ordinary_commands_are_neither_refused_nor_parked() {
    for command in [
        "git add -A && git commit -m \"Add tests\"",
        "git commit -am \"Commit the fix\"",
        "git status && echo \"Status ok\"",
        "git diff --stat -- \"Diff Notes.md\"",
        "git status",
        "git log --oneline -5",
        "git diff",
        "git add -A && git commit -m \"git push --force origin main\"",
        "cargo test",
        "ls",
        "grep -r push .",
        "find . -name '*.rs'",
        "git log -1 --format=%s | sed 's/feat/feature/'",
        "git status --short | awk '{print $2}'",
        "git diff --name-only | xargs rustfmt --check",
        "GIT_PAGER=cat git log -3",
        "GIT_EDITOR=true git rebase --continue",
    ] {
        assert_eq!(read(command), Read::NoPush, "{command}");
        assert_eq!(hard(command), None, "{command}");
    }
    assert_eq!(
        read("git push origin code/fix"),
        Read::Pushes(vec![branch("code/fix")])
    );
}

/// Which words git runs itself depends on its version and on what is
/// installed. git 2.43 has no `refs`, `replay` or `backfill`, so an alias
/// of one of those names runs; with git-lfs and git-subtree installed, no
/// alias can stand in for `lfs` or `subtree`, and without them one can.
#[test]
fn a_word_is_read_as_gits_own_only_when_the_installed_git_runs_it() {
    let git_2_43 = |verb: &str| matches!(verb, "config" | "status" | "commit" | "log" | "switch");
    let with_programs =
        |verb: &str| git_2_43(verb) || matches!(verb, "lfs" | "subtree" | "clang-format" | "refs");
    let alias = |verb: &str| {
        format!(
            "git {verb} isn't one of git's own commands, so it may be an alias that pushes, which the push guard can't read. Use git's own commands, and push with git push origin <branch>."
        )
    };
    for (command, verb) in [
        (
            "git config alias.refs push && git refs --force origin main",
            "refs",
        ),
        (
            "git config alias.replay push && git replay --force origin main",
            "replay",
        ),
        (
            "git config alias.backfill push && git backfill --force origin main",
            "backfill",
        ),
        ("git lfs pull", "lfs"),
        ("git lfs install", "lfs"),
        ("git subtree split --prefix=lib", "subtree"),
        ("git clang-format --diff", "clang-format"),
    ] {
        assert_eq!(
            read_with(command, &git_2_43),
            Read::Refused(alias(verb)),
            "{command}"
        );
    }
    for command in [
        "git lfs pull",
        "git lfs install",
        "git subtree split --prefix=lib",
        "git clang-format --diff",
        "git refs list",
    ] {
        assert_eq!(
            read_with(command, &with_programs),
            Read::NoPush,
            "{command}"
        );
    }
    // A program git runs can push, so the harness parks it.
    for command in [
        "git subtree push --prefix=lib origin main",
        "git lfs push origin main",
    ] {
        assert_eq!(
            read_with(command, &with_programs),
            Read::NoPush,
            "{command}"
        );
        assert_eq!(hard(command), SHIP, "{command}");
    }
}

#[test]
fn a_destination_git_reads_as_a_branch_is_that_branch() {
    assert_eq!(
        read("git push origin code/fix:heads/main"),
        Read::Pushes(vec![branch("main")])
    );
    assert_eq!(
        read("git push origin heads/main"),
        Read::Pushes(vec![branch("main")])
    );
    assert_eq!(
        read("git push origin HEAD:heads/main"),
        Read::Pushes(vec![branch("main")])
    );
    // A full name is taken as written: this is refs/heads/heads/x.
    assert_eq!(
        read("git push origin HEAD:refs/heads/heads/x"),
        Read::Pushes(vec![branch("heads/x")])
    );
    for command in [
        "git push origin code/x:tags/v1",
        "git push origin remotes/origin/main",
    ] {
        assert_eq!(
            refused(command),
            "This command pushes in a way the push guard can't read. Run git push origin <branch> on its own.",
            "{command}"
        );
    }
}

#[test]
fn head_is_read_only_when_nothing_before_the_push_can_switch_the_branch() {
    assert_eq!(
        read("git add -A && git commit -m x && git push origin HEAD"),
        Read::Pushes(vec![Target::Current])
    );
    for command in [
        "git checkout main && git push origin HEAD",
        "git switch -q main; git push origin HEAD",
        "git branch -m main && git push origin HEAD",
        "git symbolic-ref HEAD refs/heads/main && git push -u origin HEAD",
        "./switch.sh && git push origin HEAD",
    ] {
        assert_eq!(
            refused(command),
            "A command before git push origin HEAD may switch the branch. Name the branch: git push origin <branch>.",
            "{command}"
        );
    }
}

/// git sends a branch named on its own through a symbolic ref, so after a
/// command that may point one somewhere else the push can't be read.
#[test]
fn a_push_after_a_symbolic_ref_change_is_refused_as_unreadable() {
    for command in [
        "git symbolic-ref refs/heads/code/fix refs/heads/main && git push origin code/fix",
        "git symbolic-ref refs/heads/code/fix refs/heads/main; git push -u origin code/fix",
        "git update-ref --stdin < refs.txt && git push origin code/fix",
    ] {
        assert_eq!(refused(command), UNCLEAR, "{command}");
    }
    for command in [
        "git symbolic-ref --short HEAD && git push origin code/fix",
        "git checkout -b code/fix && git push -u origin code/fix",
        "git update-ref refs/heads/code/fix HEAD && git push origin code/fix",
    ] {
        assert_eq!(
            read(command),
            Read::Pushes(vec![branch("code/fix")]),
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

    /// `sh -c <command>` as on a desktop: no editor or pager set in the
    /// environment, so git runs the configured one.
    fn desktop_sh(dir: &Path, command: &str) -> std::process::Output {
        Command::new("sh")
            .args(["-c", command])
            .current_dir(dir)
            .env_remove("GIT_EDITOR")
            .env_remove("GIT_SEQUENCE_EDITOR")
            .env_remove("GIT_PAGER")
            .env_remove("EDITOR")
            .env_remove("VISUAL")
            .env("GIT_AUTHOR_NAME", "t")
            .env("GIT_AUTHOR_EMAIL", "t@example.invalid")
            .env("GIT_COMMITTER_NAME", "t")
            .env("GIT_COMMITTER_EMAIL", "t@example.invalid")
            .output()
            .unwrap()
    }

    /// `bash -c <command>` with no profile, no editor or pager in the
    /// environment and nothing on stdin.
    fn bash_runs(dir: &Path, command: &str) -> std::process::Output {
        Command::new("bash")
            .args(["--noprofile", "--norc", "-c", command])
            .current_dir(dir)
            .env_remove("GIT_EDITOR")
            .env_remove("GIT_SEQUENCE_EDITOR")
            .env_remove("GIT_PAGER")
            .env_remove("EDITOR")
            .env_remove("VISUAL")
            .env("GIT_AUTHOR_NAME", "t")
            .env("GIT_AUTHOR_EMAIL", "t@example.invalid")
            .env("GIT_COMMITTER_NAME", "t")
            .env("GIT_COMMITTER_EMAIL", "t@example.invalid")
            .stdin(std::process::Stdio::null())
            .output()
            .unwrap()
    }

    fn bash_ok(dir: &Path, command: &str) {
        let out = bash_runs(dir, command);
        assert!(
            out.status.success(),
            "{command}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    /// Local main one commit past origin main: rewritten (`force`), so only
    /// a force lands it, or ahead, so a plain push does.
    fn diverge(f: &Fixture, force: bool) {
        std::fs::write(f.work.join("a.txt"), "other\n").unwrap();
        if force {
            sh_git(&f.work, &["commit", "-q", "--amend", "-am", "other"]);
        } else {
            sh_git(&f.work, &["commit", "-qam", "other"]);
        }
    }

    /// What the agent's tool call does: runs on this guard's `Run`, or on
    /// its `Ask` when the person says yes. A command it has no say on runs
    /// as the harness lets it: one the harness parks (a hard class card)
    /// only when the person says yes.
    fn agent_runs(f: &Fixture, cfg: &CodeShip, command: &str, yes: bool) -> Option<Verdict> {
        let probe = Some((Host::GitHub, Pr::None));
        let verdict = decide_with(&f.config, &f.work, cfg, command, probe);
        let runs = match &verdict {
            Some(Verdict::Run) => true,
            Some(Verdict::Ask) => yes,
            Some(Verdict::Refuse(_)) => false,
            None => yes || hard(command).is_none(),
        };
        if runs {
            let out = desktop_sh(&f.work, command);
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

    /// An alias set on the line, in the environment or in the repo's config
    /// can push anything, so it is refused before it runs.
    #[test]
    fn a_push_through_a_git_alias_is_refused_and_origin_main_stays() {
        let f = fixture("alias");
        let before = origin_sha(&f, "main").unwrap();
        std::fs::write(f.work.join("a.txt"), "agent\n").unwrap();
        sh_git(&f.work, &["commit", "-am", "agent"]);
        let alias = "git ship isn't one of git's own commands, so it may be an alias that pushes, which the push guard can't read. Use git's own commands, and push with git push origin <branch>.";
        for cfg in [CodeShip::default(), auto()] {
            for command in [
                "git config alias.ship push && git ship --force origin main",
                "git -c alias.ship=push ship --force origin main",
                "GIT_CONFIG_COUNT=1 GIT_CONFIG_KEY_0=alias.ship GIT_CONFIG_VALUE_0=push git ship --force origin main",
            ] {
                assert_eq!(
                    agent_runs(&f, &cfg, command, true),
                    Some(Verdict::Refuse(alias.into())),
                    "{command}"
                );
            }
        }
        // Set in the repo's config by an earlier command.
        sh_git(&f.work, &["config", "alias.ship", "push"]);
        assert_eq!(
            agent_runs(&f, &auto(), "git ship --force origin main", true),
            Some(Verdict::Refuse(alias.into()))
        );
        assert_eq!(origin_sha(&f, "main"), Some(before));
    }

    /// An alias named for a command this git doesn't have yet (`refs` is
    /// git 2.46's) runs as the alias, so it is refused; on a git that has
    /// the command, the command runs and pushes nothing. Either way origin
    /// main stays. Behind a wrapper, or through git-push by its path, the
    /// push is refused too.
    #[test]
    fn a_push_through_an_alias_git_may_run_or_a_wrapper_is_refused_and_origin_main_stays() {
        let f = fixture("alias-any");
        let before = origin_sha(&f, "main").unwrap();
        // History rewritten locally; only a force would land it.
        std::fs::write(f.work.join("a.txt"), "agent\n").unwrap();
        sh_git(&f.work, &["commit", "-am", "agent"]);
        sh_git(&f.work, &["reset", "--hard", "HEAD~1"]);
        std::fs::write(f.work.join("a.txt"), "other\n").unwrap();
        sh_git(&f.work, &["commit", "-am", "other"]);
        for verb in ["refs", "replay", "backfill", "ship"] {
            let command = format!("git config alias.{verb} push && git {verb} --force origin main");
            let probe = Some((Host::GitHub, Pr::None));
            let verdict = decide_with(&f.config, &f.work, &auto(), &command, probe);
            if grokhub_agent::harness::git_runs_own(verb) {
                assert_eq!(verdict, None, "{command}");
                // git's own command runs; it pushes nothing.
                let _ = desktop_sh(&f.work, &command);
                sh_git(&f.work, &["config", "--unset", &format!("alias.{verb}")]);
            } else {
                assert_eq!(
                    verdict,
                    Some(Verdict::Refuse(format!(
                        "git {verb} isn't one of git's own commands, so it may be an alias that pushes, which the push guard can't read. Use git's own commands, and push with git push origin <branch>."
                    ))),
                    "{command}"
                );
            }
        }
        assert!(!grokhub_agent::harness::git_runs_own("ship"));
        let exec_path = sh_git(&f.work, &["--exec-path"]);
        for (command, why) in [
            (
                "git config alias.ship push && env git ship --force origin main".to_string(),
                "git ship isn't one of git's own commands, so it may be an alias that pushes, which the push guard can't read. Use git's own commands, and push with git push origin <branch>.",
            ),
            (
                "git config alias.ship push && command git ship --force origin main".to_string(),
                "git ship isn't one of git's own commands, so it may be an alias that pushes, which the push guard can't read. Use git's own commands, and push with git push origin <branch>.",
            ),
            (format!("{exec_path}/git-push --force origin main"), FORCE),
            (format!("{exec_path}/git-push origin main"), UNCLEAR),
        ] {
            for cfg in [CodeShip::default(), auto()] {
                assert_eq!(
                    agent_runs(&f, &cfg, &command, true),
                    Some(Verdict::Refuse(why.into())),
                    "{command}"
                );
            }
        }
        assert_eq!(origin_sha(&f, "main"), Some(before));
    }

    /// A command git runs for one of its own commands: a pager `grep -O`
    /// opens, the upload-pack `fetch` runs, an editor saved in config. Not a
    /// push this guard reads, so the harness parks it: with no yes on the
    /// card, nothing runs.
    #[test]
    fn a_push_git_runs_for_its_own_command_is_parked_and_origin_main_stays() {
        let f = fixture("runs");
        let before = origin_sha(&f, "main").unwrap();
        std::fs::write(f.work.join("a.txt"), "agent\n").unwrap();
        sh_git(&f.work, &["commit", "-am", "agent"]);
        sh_git(&f.work, &["reset", "--hard", "HEAD~1"]);
        std::fs::write(f.work.join("a.txt"), "agent again\n").unwrap();
        sh_git(&f.work, &["commit", "-am", "agent again"]);
        for command in [
            "git grep -O'git push --force origin main #' -e agent",
            "git fetch --upload-pack='git push --force origin main #' . ; true",
            "git config core.editor 'git push --force origin main #' && git commit -q --allow-empty --allow-empty-message",
            "EDITOR='git push --force origin main #' git commit -q --allow-empty --allow-empty-message",
        ] {
            assert_eq!(hard(command), SHIP, "{command}");
            for cfg in [CodeShip::default(), auto()] {
                assert_eq!(agent_runs(&f, &cfg, command, false), None, "{command}");
            }
        }
        assert_eq!(origin_sha(&f, "main"), Some(before));
    }

    /// git-lfs is found on the command's `PATH`, so after a `PATH` change an
    /// alias named `lfs` runs in its place. Run as written, past the guard,
    /// the command does push main; through the guard it is refused.
    #[test]
    fn an_alias_found_after_a_path_change_is_refused_and_origin_main_stays() {
        let f = fixture("path");
        std::fs::write(f.work.join("a.txt"), "agent\n").unwrap();
        sh_git(&f.work, &["commit", "-am", "agent"]);
        let before = origin_sha(&f, "main").unwrap();
        let exec_path = sh_git(&f.work, &["--exec-path"]);
        let commands = [
            format!(
                "git config alias.lfs push && env PATH={exec_path} git lfs --force origin main"
            ),
            format!("git config alias.lfs push && PATH={exec_path} git lfs --force origin main"),
            format!("PATH={exec_path}; git lfs --force origin main"),
        ];
        for command in &commands {
            for cfg in [CodeShip::default(), auto()] {
                assert_eq!(
                    agent_runs(&f, &cfg, command, true),
                    Some(Verdict::Refuse(not_own("lfs"))),
                    "{command}"
                );
            }
            assert_eq!(hard(command), SHIP, "{command}");
        }
        assert_eq!(origin_sha(&f, "main"), Some(before.clone()));
        // The attack is real: past the guard, main moves.
        let out = desktop_sh(&f.work, &commands[0]);
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let head = sh_git(&f.work, &["rev-parse", "HEAD"]);
        assert_eq!(origin_sha(&f, "main"), Some(head));
        assert_ne!(origin_sha(&f, "main"), Some(before));
    }

    /// git sends a branch named on its own where the repository's config
    /// says: through `remote.origin.push`, or with `push.default=upstream`
    /// onto the branch it tracks. `git push origin code/fix` then updates
    /// main, which the guard refuses like main.
    #[test]
    fn a_branch_the_repos_config_sends_to_main_is_refused_and_origin_main_stays() {
        let f = fixture("mapped");
        let before = origin_sha(&f, "main").unwrap();
        let made = ship::new_branch(&f.config, &f.work, &CodeShip::default(), "code/fix").unwrap();
        assert_eq!(made, "code/fix");
        std::fs::write(f.work.join("a.txt"), "agent\n").unwrap();
        sh_git(&f.work, &["commit", "-am", "agent"]);
        let main =
            "main is the main or default branch. The app never pushes to it; make a branch first.";
        // Where git itself would send it.
        let lands = || {
            sh_git(
                &f.work,
                &["push", "--dry-run", "--porcelain", "origin", "code/fix"],
            )
        };
        let cases: [(&[(&str, &str)], &str); 4] = [
            (
                &[("remote.origin.push", "refs/heads/code/fix:refs/heads/main")],
                main,
            ),
            (
                &[(
                    "remote.origin.push",
                    "refs/heads/code/fix*:refs/heads/main*",
                )],
                main,
            ),
            (
                &[("remote.origin.push", "+refs/heads/code/fix:refs/heads/main")],
                FORCE,
            ),
            (
                &[
                    ("push.default", "upstream"),
                    ("branch.code/fix.remote", "origin"),
                    ("branch.code/fix.merge", "refs/heads/main"),
                ],
                main,
            ),
        ];
        for (config, why) in cases {
            for (key, value) in config {
                sh_git(&f.work, &["config", key, value]);
            }
            assert!(
                lands().contains("refs/heads/code/fix:refs/heads/main"),
                "{config:?}: {}",
                lands()
            );
            for cfg in [CodeShip::default(), auto()] {
                assert_eq!(
                    agent_runs(&f, &cfg, "git push origin code/fix", true),
                    Some(Verdict::Refuse(why.into())),
                    "{config:?}"
                );
            }
            for (key, _) in config {
                sh_git(&f.work, &["config", "--unset-all", key]);
            }
        }
        assert_eq!(origin_sha(&f, "main"), Some(before));
        // With nothing in config it lands on its own name, and asks.
        assert!(lands().contains("refs/heads/code/fix:refs/heads/code/fix"));
        assert_eq!(
            agent_runs(&f, &CodeShip::default(), "git push origin code/fix", false),
            Some(Verdict::Ask)
        );
    }

    #[test]
    fn a_push_to_heads_main_is_refused_like_main_and_origin_main_stays() {
        let f = fixture("heads");
        let before = origin_sha(&f, "main").unwrap();
        sh_git(&f.work, &["checkout", "-b", "code/fix"]);
        std::fs::write(f.work.join("a.txt"), "agent\n").unwrap();
        sh_git(&f.work, &["commit", "-am", "agent"]);
        let main =
            "main is the main or default branch. The app never pushes to it; make a branch first.";
        for cfg in [CodeShip::default(), auto()] {
            for command in [
                "git push origin code/fix:heads/main",
                "git push origin HEAD:heads/main",
                "git push origin heads/main",
            ] {
                assert_eq!(
                    agent_runs(&f, &cfg, command, true),
                    Some(Verdict::Refuse(main.into())),
                    "{command}"
                );
            }
        }
        assert_eq!(origin_sha(&f, "main"), Some(before));
    }

    /// `HEAD` is read before the command runs, so a switch to main earlier
    /// in the same command would push main.
    #[test]
    fn a_push_of_head_after_a_switch_to_main_is_refused_and_origin_main_stays() {
        let f = fixture("switch");
        let before = origin_sha(&f, "main").unwrap();
        let made = ship::new_branch(&f.config, &f.work, &CodeShip::default(), "code/fix").unwrap();
        assert_eq!(made, "code/fix");
        let command = "git checkout -q main && echo agent > a.txt && git -c user.name=t -c user.email=t@example.invalid commit -qam agent && git push origin HEAD";
        for cfg in [auto(), CodeShip::default()] {
            assert_eq!(
                agent_runs(&f, &cfg, command, true),
                Some(Verdict::Refuse(HEAD_MAY_MOVE.into())),
                "{}",
                cfg.mode.id()
            );
        }
        assert_eq!(origin_sha(&f, "main"), Some(before));
        assert_eq!(ship::current_branch(&f.work).as_deref(), Some("code/fix"));
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

    /// Each command pushes main past the guard: through text a later shell
    /// reads, words the shell uses out of text order, git run under another
    /// name, a variable set by more than `NAME=value`, a hook file the
    /// command writes, or text bash reads as arithmetic. Through the guard
    /// each is refused, or parked by the harness, so with no yes on a card
    /// nothing runs, in either mode.
    #[test]
    fn a_push_the_shell_hides_from_the_text_is_refused_or_parked_and_origin_main_stays() {
        let main =
            "main is the main or default branch. The app never pushes to it; make a branch first.";
        let force = Some(Verdict::Refuse(FORCE.into()));
        // (force only lands it, setup, command, the guard's verdict)
        let cases: &[(bool, &str, &str, Option<Verdict>)] = &[
            (true, "", "echo 'gi\"\"t push --force origin main' | sh", None),
            (true, "", "echo \"g'i't push --force origin main\" | bash", None),
            (true, "", "cat <<'EOF' | sh\ngi\"\"t push --force origin main\nEOF", None),
            (true, "", "printf 'set -e\\ngit push --force origin main\\n' | sh", None),
            (true, "", "echo -e 'cd .\\ngit push --force origin main' | bash", None),
            (true, "", "printf 'set -e\\ngit push --force origin main\\n' > x.sh; sh x.sh", None),
            (true, "", "x='gi\"\"t push --force origin main'; eval \"$x\"", None),
            (true, "", "x='gi\"\"t push --force origin main'; sh -c \"$x\"", None),
            (true, "", "bash -c \"$(echo 'gi\"\"t push --force origin main')\"", None),
            (true, "", "trap 'gi\"\"t push --force origin main' EXIT", None),
            (true, "", "source /dev/stdin <<< 'gi\"\"t push --force origin main'", None),
            (true, "", "script -qc 'gi\"\"t push --force origin main' /dev/null", None),
            (true, "", "PS4='$(gi\"\"t push --force origin main >&2)'; set -x; true", None),
            (
                true,
                "",
                "printf '#!/bin/sh\\ngit push --force origin main\\n' > .git/hooks/post-commit && chmod +x .git/hooks/post-commit && git commit -q --allow-empty -m x",
                None,
            ),
            (true, "", "function f { git push --force origin main; }; f", force.clone()),
            (
                true,
                "",
                "bash -c 'function f { git push --force origin main; }; f'",
                Some(Verdict::Refuse(UNCLEAR.into())),
            ),
            (true, "", "function f() { git push --force origin main; }; f", force.clone()),
            (false, "", "function f { git push origin main; }; f", Some(Verdict::Refuse(main.into()))),
            (true, "", "bash -c '$0 push --force origin main' git", None),
            (true, "", "sh -c '\"$@\"' sh git push --force origin main", None),
            (true, "", "while read -r g; do \"$g\" push --force origin main; done <<< git", None),
            (true, "", "{ read -r g; \"$g\" push --force origin main; } <<< git", None),
            (true, "", "exec 3<<< git; read -r -u3 g; \"$g\" push --force origin main", None),
            (true, "", "echo() { \"$1\" push --force origin main; }; echo git", None),
            (
                true,
                "",
                "git remote add status ../origin.git && exec -a git-push git status +HEAD:main",
                None,
            ),
            (false, "git branch -q -u origin/main", "exec -a git-push git", None),
            (
                true,
                "",
                "read -r GIT_EDITOR <<< sh; export GIT_EDITOR; git commit -q --allow-empty -e -m 'git push --force origin main'",
                None,
            ),
            (
                true,
                "",
                "printf -v GIT_EDITOR sh; export GIT_EDITOR; git commit -q --allow-empty -e -m 'git push --force origin main'",
                None,
            ),
            (
                true,
                "",
                "printf '[core]\\n\\teditor = sh\\n' >> .git/config && git commit -q --allow-empty -e -m 'git push --force origin main'",
                None,
            ),
            (true, "", "x='a[$(git push --force origin main >&2)]'; echo $((x))", None),
            (true, "", "x='a[$(git push --force origin main >&2)]'; [[ $x -eq 0 ]]; true", None),
        ];
        for (n, (only_force, setup, command, verdict)) in cases.iter().enumerate() {
            let f = fixture(&format!("hidden-{n}"));
            diverge(&f, *only_force);
            if !setup.is_empty() {
                bash_ok(&f.work, setup);
            }
            let before = origin_sha(&f, "main").unwrap();
            if verdict.is_none() {
                assert_eq!(hard(command), SHIP, "{command}");
            }
            for cfg in [CodeShip::default(), auto()] {
                assert_eq!(agent_runs(&f, &cfg, command, false), *verdict, "{command}");
            }
            assert_eq!(origin_sha(&f, "main"), Some(before), "{command}");
            // The attack is real: run past the guard with bash 4 and
            // util-linux `script`, as on Linux, main moves.
            if cfg!(target_os = "linux") {
                let g = fixture(&format!("hidden-real-{n}"));
                diverge(&g, *only_force);
                if !setup.is_empty() {
                    bash_ok(&g.work, setup);
                }
                let before = origin_sha(&g, "main").unwrap();
                let local = sh_git(&g.work, &["rev-parse", "refs/heads/main"]);
                bash_ok(&g.work, command);
                // origin main now holds the local history: an editor pushes
                // before its commit lands, a hook after.
                let after = origin_sha(&g, "main").unwrap();
                assert_ne!(after, before, "{command}");
                sh_git(&g.bare, &["merge-base", "--is-ancestor", &local, &after]);
            }
        }
    }

    /// git sends a branch named on its own through a symbolic ref, and with
    /// `push.default=upstream` onto the branch it tracks. A branch the
    /// repository already points at main is refused like main; a command
    /// that may change either before its push is refused as unreadable.
    /// Past the guard, each push lands on main.
    #[test]
    fn a_branch_a_symbolic_ref_or_an_upstream_sends_to_main_is_refused_and_origin_main_stays() {
        let main =
            "main is the main or default branch. The app never pushes to it; make a branch first.";
        let cases = [
            (
                "git symbolic-ref refs/heads/code/fix refs/heads/main",
                "git push origin code/fix",
                main,
            ),
            (
                "",
                "git symbolic-ref refs/heads/code/fix refs/heads/main && git push origin code/fix",
                UNCLEAR,
            ),
            (
                "git config push.default upstream",
                "git branch -q -u origin/main code/fix && git push origin code/fix",
                UNCLEAR,
            ),
        ];
        let prepare = |tag: &str, setup: &str| {
            let f = fixture(tag);
            diverge(&f, false);
            let made =
                ship::new_branch(&f.config, &f.work, &CodeShip::default(), "code/fix").unwrap();
            assert_eq!(made, "code/fix");
            if !setup.is_empty() {
                bash_ok(&f.work, setup);
            }
            std::fs::write(f.work.join("b.txt"), "agent\n").unwrap();
            sh_git(&f.work, &["add", "b.txt"]);
            sh_git(&f.work, &["commit", "-qm", "agent"]);
            f
        };
        for (n, (setup, command, why)) in cases.into_iter().enumerate() {
            let f = prepare(&format!("symref-{n}"), setup);
            let before = origin_sha(&f, "main").unwrap();
            for cfg in [CodeShip::default(), auto()] {
                assert_eq!(
                    agent_runs(&f, &cfg, command, true),
                    Some(Verdict::Refuse(why.into())),
                    "{command}"
                );
            }
            assert_eq!(origin_sha(&f, "main"), Some(before), "{command}");
            let g = prepare(&format!("symref-real-{n}"), setup);
            let before = origin_sha(&g, "main").unwrap();
            bash_ok(&g.work, command);
            assert_ne!(origin_sha(&g, "main"), Some(before), "{command}");
        }
        // With neither, the app's branch runs in Auto-beta.
        let f = prepare("symref-none", "");
        assert_eq!(
            agent_runs(&f, &auto(), "git push origin code/fix", false),
            Some(Verdict::Run)
        );
        assert_eq!(
            origin_sha(&f, "code/fix").as_deref(),
            Some(sh_git(&f.work, &["rev-parse", "HEAD"]).as_str())
        );
    }
}
