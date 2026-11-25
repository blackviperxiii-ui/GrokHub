use super::*;

/// The words of each command bash runs for `command`.
fn bash_runs(command: &str) -> Vec<Vec<String>> {
    let read = read_shell_as(command, false);
    assert_eq!(read.hidden, None, "{command}");
    read.runs.into_iter().map(|r| r.words).collect()
}

fn ps_runs(command: &str) -> Vec<Vec<String>> {
    let read = read_shell_as(command, true);
    assert_eq!(read.hidden, None, "{command}");
    read.runs.into_iter().map(|r| r.words).collect()
}

fn words(line: &str) -> Vec<String> {
    line.split_whitespace().map(str::to_string).collect()
}

fn run(line: &str) -> Run {
    Run {
        env: Vec::new(),
        words: words(line),
        nested: false,
        more_args: false,
    }
}

#[test]
fn a_word_is_read_as_bash_reads_it() {
    let git_push = vec![words("git push")];
    for command in [
        "git push",
        "g\\it push",
        "g''it pu\"\"sh",
        "\"git\" 'push'",
        "$'\\x67it' push",
        "$'\\147it' push",
        "$'\\u0067it' push",
        "$\"git\" push",
        "git \\\npush",
        "  git push   # git push --force origin main",
    ] {
        assert_eq!(bash_runs(command), git_push, "{command}");
    }
    assert_eq!(
        bash_runs("git commit -m \"Fix: push the \\\"right\\\" thing\" && echo 'a  b'"),
        vec![
            vec![
                "git".to_string(),
                "commit".into(),
                "-m".into(),
                "Fix: push the \"right\" thing".into()
            ],
            vec!["echo".to_string(), "a  b".into()],
        ]
    );
    // A substitution runs first; what it gives can't be seen.
    assert_eq!(
        bash_runs("echo \"$(git rev-parse HEAD)\" `git log -1`"),
        vec![
            words("git rev-parse HEAD"),
            words("git log -1"),
            vec!["echo".to_string(), HIDDEN.to_string(), HIDDEN.to_string()],
        ]
    );
    // Separators inside quotes don't split.
    assert_eq!(
        bash_runs("git commit -m 'a; git push && b' | cat"),
        vec![
            vec![
                "git".to_string(),
                "commit".into(),
                "-m".into(),
                "a; git push && b".into()
            ],
            words("cat"),
        ]
    );
}

#[test]
fn wrappers_scripts_and_here_documents_are_read_through() {
    let read = read_shell_as("sudo -u user env -i FOO=1 nice -n 5 git status", false);
    assert_eq!(read.hidden, None);
    assert_eq!(
        read.runs,
        vec![Run {
            env: vec!["FOO=1".into()],
            words: words("git status"),
            nested: true,
            more_args: false,
        }]
    );
    assert_eq!(
        bash_runs("bash -lc 'git status; git log' && sh -ec \"git diff\""),
        vec![
            vec![
                "bash".to_string(),
                "-lc".into(),
                "git status; git log".into()
            ],
            words("git status"),
            words("git log"),
            vec!["sh".to_string(), "-ec".into(), "git diff".into()],
            words("git diff"),
        ]
    );
    assert_eq!(
        bash_runs("sh <<'EOF'\ngit status\nEOF\ngit log"),
        vec![words("sh"), words("git status"), words("git log")]
    );
    assert_eq!(
        bash_runs("bash <<< 'git status'"),
        vec![words("bash"), words("git status")]
    );
    assert_eq!(
        bash_runs("eval git log"),
        vec![words("eval git log"), words("git log")]
    );
    for command in [
        "setsid -w git status",
        "stdbuf -oL git status",
        "flock /tmp/l git status",
        "flock -w 5 /tmp/l -c 'git status'",
        "nohup git status",
        "timeout 30 git status",
        "timeout -s KILL 30 git status",
        "command git status",
        "if true; then git status; fi",
        "! git status",
    ] {
        let read = read_shell_as(command, false);
        assert_eq!(read.hidden, None, "{command}");
        assert!(
            read.runs.iter().any(|r| r.words == words("git status")),
            "{command}: {:?}",
            read.runs
        );
    }
    let read = read_shell_as("git log --format=%H | xargs -n1 git show", false);
    assert_eq!(read.runs[1].words, words("git show"));
    assert!(read.runs[1].more_args);
    // A variable set on its own is kept for what comes after.
    assert_eq!(
        read_shell_as(
            "PATH=/usr/lib/git-core; export EDITOR=vi; git status",
            false
        )
        .sets,
        vec!["PATH=/usr/lib/git-core".to_string(), "EDITOR=vi".into()]
    );
}

#[test]
fn a_word_is_read_as_powershell_reads_it() {
    let git_push = vec![words("git push")];
    for command in [
        "git push",
        "g`it push",
        "g''it push",
        "& git push",
        "& 'git' push",
        "git.exe push",
    ] {
        let words: Vec<Vec<String>> = ps_runs(command)
            .into_iter()
            .filter(|w| w.first().is_none_or(|h| h != "&"))
            .map(|w| {
                let mut w = w;
                if w[0] == "git.exe" {
                    w[0] = "git".into();
                }
                w
            })
            .collect();
        assert_eq!(words, git_push, "{command}");
    }
    assert_eq!(
        ps_runs("git commit -m 'it''s' -m \"say \"\"hi\"\"\""),
        vec![vec![
            "git".to_string(),
            "commit".into(),
            "-m".into(),
            "it's".into(),
            "-m".into(),
            "say \"hi\"".into()
        ]]
    );
    // A backslash is a path separator, not an escape.
    assert_eq!(
        ps_runs("C:\\Tools\\git.exe status"),
        vec![vec!["C:\\Tools\\git.exe".to_string(), "status".into()]]
    );
    for command in [
        "iex 'git push'",
        "Invoke-Expression \"git push\"",
        "powershell -Command \"git push\"",
        "cmd /c git push",
        "$env:PATH = 'C:\\x'; git lfs push",
        "git commit -m \u{201c}x\u{201d}",
    ] {
        let read = read_shell_as(command, true);
        let pushes = read
            .runs
            .iter()
            .any(|r| r.words.first().is_some_and(|w| w == "git"));
        assert!(read.hidden.is_some() || pushes, "{command}: {read:?}");
    }
    // A pipeline's `$_` is a program's output, as `$n = @(...)` sets a
    // value from parts that hold no git.
    for command in [
        "Get-ChildItem build | % { ri $_ -Recurse -Force }",
        "$n = @(Get-ChildItem build).Count; Write-Output $n",
    ] {
        assert_eq!(read_shell_as(command, true).hidden, None, "{command}");
    }
}

/// Anything in which the shell could run git that the reader doesn't read
/// is hidden, and only that.
#[test]
fn what_the_reader_cant_follow_is_hidden() {
    for (command, why) in [
        ("G=git; $G push", BUILT),
        ("G=$(echo git); \"$G\" push", BUILT),
        ("x=g; ${x}it push", BUILT),
        ("a=g b=it; $a$b push", BUILT),
        ("a=xgit; ${a#x} push", BUILT),
        ("a=g; b=${a}it; $b push", BUILT),
        ("for p in g; do ${p}it push; done", BUILT),
        ("IFS=x; c=gitxpush; $c", BUILT),
        ("echo git; $_ push", BUILT),
        ("/usr/bin/g?t push", BUILT),
        ("/usr/bin/gi[t] push", BUILT),
        ("eval \"$(echo git push)\"", BUILT),
        ("bash -c \"$(echo git push)\"", BUILT),
        ("echo git push | sh", FED),
        ("echo git push | bash -s", FED),
        ("git log --format=%s | sh", FED),
        ("printf '\\x67it push' | sh", FED),
        ("echo Z2l0IHB1c2g= | base64 -d | sh", FED),
        ("xargs git push < x", "none"),
        ("find . -exec git push ';'", GIVEN),
        ("watch git push", GIVEN),
        ("alias g=git", GIVEN),
        ("ssh host git push", GIVEN),
        ("x=gi; watch ${x}t push", GIVEN),
        ("git log -1 --format=%s | read m; watch $m", GIVEN),
        ("printf 'git push' > x.sh", WRITTEN),
        ("ln -s /usr/bin/git g", WRITTEN),
        ("echo Z2l0IHB1c2g= | base64 -d > x.sh; sh x.sh", WRITTEN),
        ("git log --format=%s > x.sh; ./x.sh", WRITTEN),
        ("EDITOR='git push'", KEPT),
        ("export GIT_EDITOR='git push'", KEPT),
        ("git push 'origin", UNREAD),
    ] {
        let read = read_shell_as(command, false);
        if why == "none" {
            assert_eq!(read.hidden, None, "{command}");
        } else {
            assert_eq!(read.hidden, Some(why), "{command}");
        }
    }
    for command in [
        "cat docs/git-push.md",
        "ls *.rs",
        "$HOME/.cargo/bin/cargo test",
        "git log | head -3",
        "echo git push",
        "grep -rn git src/",
        "find . -name '*.rs'",
        "cargo test git_push",
        "git show HEAD:a.txt > /tmp/a.txt",
        "sha=$(git rev-parse HEAD); git log -1 \"$sha\"",
        "SHA=$(git rev-parse HEAD); git log -1 \"$SHA\"",
        // A variable the command doesn't set has the app's own value, and a
        // substitution a program's output: outside what text shows.
        "$X push",
        "\"$G\" push",
        "cargo test $ARGS",
        "for u in $(systemctl --failed --plain | cut -d' ' -f1); do systemctl status $u; done",
        "systemctl status \"$u.service\"",
        "cp \"$f\" \"$dir/\"",
        "G=git",
        // `$_` is the last word of the command before, git only when the
        // command holds git text (`echo git; $_ push`).
        "ls build; $_/run.sh",
        "Get-ChildItem build | % { ri $_ -Recurse -Force }",
    ] {
        assert_eq!(read_shell_as(command, false).hidden, None, "{command}");
    }
    // Neither bash nor git reads a lower-case variable; on Windows names
    // don't tell case apart.
    assert_eq!(
        read_shell_as("editor='git push'", false).hidden,
        if cfg!(windows) { Some(KEPT) } else { None }
    );
    // `$HOME` set by the command is not the system's: next to git text it
    // may be git, as a value built from parts may.
    assert_eq!(
        read_shell_as("git status; $HOME/bin/cargo test", false).hidden,
        None
    );
    assert_eq!(
        read_shell_as("git status; HOME=/tmp/x; $HOME/bin/cargo test", false).hidden,
        Some(BUILT)
    );
    assert_eq!(
        read_shell_as("HOME=gi; ${HOME}t push", false).hidden,
        Some(BUILT)
    );
    assert_eq!(
        read_shell_as("HOME=/tmp/x; $HOME/bin/cargo test", false).hidden,
        None
    );
}

#[test]
fn a_git_command_can_push_unless_every_part_is_read_as_one_that_cant() {
    let newer = |verb: &str| matches!(verb, "switch" | "restore" | "worktree");
    let cant = |line: &str| !may_push(&run(line), &[], &newer);
    for line in [
        "git status",
        "git log --oneline -5",
        "git commit -m git-push",
        "git -c user.name=user -c color.ui=never commit",
        "git -C ../x --no-pager diff",
        "git config user.email user@example.com",
        "git config --get remote.origin.push",
        "git config --list",
        "git config get alias.x",
        "git fetch origin",
        "git clone https://example.com/r.git",
        "git switch -c code/fix",
        "git stash pop",
        "git rebase -i main",
        "git submodule update --init",
        "git grep -o -e push",
        "git --version",
        "git",
    ] {
        assert!(cant(line), "{line}");
    }
    for line in [
        "git push origin x",
        "git ship",
        "git STATUS",
        "git lfs pull",
        "/usr/lib/git-core/git-push origin x",
        "git send-pack x",
        "git -c alias.st=status st",
        "git -c core.editor=sh commit",
        "git --config-env=core.editor=ED commit",
        "git --exec-path=/tmp/x status",
        "git --frobnicate status",
        "git config alias.ship push",
        "git config core.editor sh",
        "git config sequence.editor sh",
        "git config remote.origin.push refs/heads/x:refs/heads/main",
        "git config --unset alias.ship",
        "git config --edit",
        "git config set alias.ship push",
        "git rebase -x true main",
        "git rebase --exec=true main",
        "git bisect run sh",
        "git grep -O vim -e x",
        "git fetch --upload-pack=x origin",
        "git fetch --upl=x origin",
        "git fetch ext::sh",
        "git clone fd::3 x",
        "git ls-remote --exec=x origin",
        "git archive --remote=ext::sh HEAD",
        "git submodule foreach true",
        "git remote add o ext::sh",
        "git restore-everything",
    ] {
        assert!(!cant(line), "{line}");
    }
    // An old git has no `switch`: an alias of that name runs.
    assert!(may_push(&run("git switch main"), &[], &|_| false));
    // What the run or the command sets.
    let with = |env: &[&str], sets: &[&str]| {
        let mut r = run("git status");
        r.env = env.iter().map(|s| s.to_string()).collect();
        let sets: Vec<String> = sets.iter().map(|s| s.to_string()).collect();
        may_push(&r, &sets, &newer)
    };
    assert!(!with(&["GIT_TRACE=1", "LC_ALL=C", "TZ=UTC", "FOO=1"], &[]));
    assert!(with(&["PATH=/x"], &[]));
    assert!(with(&["EDITOR=sh"], &[]));
    assert!(with(&["GIT_EDITOR=sh"], &[]));
    assert!(with(&["HOME=/tmp/x"], &[]));
    assert!(with(&["XDG_CONFIG_HOME=/tmp/x"], &[]));
    assert!(with(&["GIT_CONFIG_GLOBAL=/tmp/x"], &[]));
    assert!(with(&["GIT_SSH_COMMAND=sh"], &[]));
    assert!(with(&["SSH_ASKPASS=sh"], &[]));
    assert!(with(&["LD_PRELOAD=/tmp/x.so"], &[]));
    assert!(with(&[], &["GIT_EXEC_PATH=/x"]));
    let mut more = run("git");
    more.more_args = true;
    assert!(may_push(&more, &[], &newer));
    more.words = words("git config");
    assert!(may_push(&more, &[], &newer));
    more.words = words("git show");
    assert!(!may_push(&more, &[], &newer));
}

/// Which words git runs itself depends on its version and on what is
/// installed. On a git without `refs` (before 2.46), an alias named `refs`
/// runs; `lfs` and `subtree` are programs that can push whatever is
/// installed.
#[test]
fn a_newer_command_is_read_as_gits_own_only_when_the_installed_git_has_it() {
    let git_2_43 = |verb: &str| matches!(verb, "switch");
    let git_2_46 = |verb: &str| matches!(verb, "switch" | "refs" | "replay");
    for line in ["git refs list", "git replay --onto main topic~2 topic"] {
        assert!(may_push(&run(line), &[], &git_2_43), "{line}");
        assert!(!may_push(&run(line), &[], &git_2_46), "{line}");
    }
    for line in [
        "git lfs pull",
        "git lfs install",
        "git subtree split --prefix=lib",
        "git backfill",
        "git maintenance run",
        "git hook run pre-push",
    ] {
        assert!(may_push(&run(line), &[], &git_2_46), "{line}");
    }
    assert!(!may_push(&run("git switch -c code/fix"), &[], &git_2_43));
}

#[test]
fn a_path_change_for_git_is_read_from_the_run_and_the_command() {
    let mut r = run("git lfs push");
    assert!(!git_path_changed(&r, &[]));
    assert!(git_path_changed(&r, &["PATH=/usr/lib/git-core".into()]));
    r.env = vec!["GIT_EXEC_PATH=/x".into()];
    assert!(git_path_changed(&r, &[]));
    assert!(git_path_changed(&run("git --exec-path=/x lfs push"), &[]));
    assert!(!git_path_changed(&run("git --exec-path"), &[]));
    let read = read_shell_as("env PATH=/usr/lib/git-core git lfs push", false);
    assert!(git_path_changed(&read.runs[0], &read.sets));
}

#[test]
fn a_command_may_push_by_the_closed_rule() {
    for command in [
        "git push origin code/fix",
        "git config alias.lfs push && env PATH=/usr/lib/git-core git lfs --force origin main",
        "echo 'git ship' | sh",
        "find . -exec git ship ';'",
    ] {
        assert!(command_may_push(command), "{command}");
    }
    for command in [
        "git add -A && git commit -m \"Add tests\"",
        "git commit -am \"Commit the fix\"",
        "git status && echo \"Status ok\"",
        "git diff --stat -- \"Diff Notes.md\"",
        "cargo test",
    ] {
        assert!(!command_may_push(command), "{command}");
    }
}

#[test]
fn a_git_name_is_git_or_a_git_program_in_any_case() {
    for leaf in [
        "git",
        "GIT",
        "git.exe",
        "Git.EXE",
        "git-push",
        "git-lfs.exe",
    ] {
        assert!(is_git_name(leaf), "{leaf}");
    }
    for leaf in ["git-", "gitk", "legit", "git_words", "github", "digit"] {
        assert!(!is_git_name(leaf), "{leaf}");
    }
    let call = |line: &str| {
        let r = run(line);
        git_call(&r).map(|c| (c.verb.map(str::to_string), c.dashed, c.odd))
    };
    assert_eq!(
        call("git -C x -c a=b --no-pager status"),
        Some((Some("status".into()), false, false))
    );
    assert_eq!(
        call("/usr/lib/git-core/git-push origin"),
        Some((Some("push".into()), true, false))
    );
    assert_eq!(
        call("git --help"),
        Some((Some("help".into()), false, false))
    );
    assert_eq!(
        call("git --odd status"),
        Some((Some("status".into()), false, true))
    );
    assert_eq!(call("cargo test"), None);
}

/// git itself is the judge: give each word an alias on the line, run it,
/// and see whether the alias ran. A word whose alias runs is not git's own;
/// a word whose alias git ignores is.
#[test]
fn a_word_is_gits_own_exactly_when_the_installed_git_ignores_an_alias_of_it() {
    let dir = std::env::temp_dir().join(format!("git-words-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let alias_runs = |word: &str| -> Option<bool> {
        let mut cmd = std::process::Command::new("git");
        cmd.arg("-c")
            .arg(format!("alias.{word}=!echo ALIAS-RAN"))
            .arg(word)
            .current_dir(&dir)
            .env("GIT_CEILING_DIRECTORIES", &dir)
            .stdin(std::process::Stdio::null());
        let out = cmd.output().ok()?;
        Some(String::from_utf8_lossy(&out.stdout).contains("ALIAS-RAN"))
    };
    let Some(false) = alias_runs("status") else {
        // No git here to ask.
        let _ = std::fs::remove_dir_all(&dir);
        return;
    };
    let mut seen = Vec::new();
    for word in [
        "status",
        "log",
        "rebase",
        "stash",
        "switch",
        "worktree",
        "submodule",
        "refs",
        "replay",
        "backfill",
        "hook",
        "for-each-repo",
        "lfs",
        "subtree",
        "ship",
        "zz-not-a-git-command",
    ] {
        let runs_alias = alias_runs(word).unwrap();
        seen.push((word, runs_alias));
        assert_eq!(git_runs_own(word), !runs_alias, "{word}");
    }
    let _ = std::fs::remove_dir_all(&dir);
    // The sample holds both kinds on any git.
    assert!(seen.contains(&("status", false)), "{seen:?}");
    assert!(seen.contains(&("ship", true)), "{seen:?}");
    assert!(seen.contains(&("zz-not-a-git-command", true)), "{seen:?}");
}
