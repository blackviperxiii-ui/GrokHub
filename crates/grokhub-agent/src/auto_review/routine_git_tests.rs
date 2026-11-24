// Portions derived from xai-org/grok-build (Apache-2.0, © SpaceXAI), commit 2bdd1d6a; modified.

use super::*;

fn words(cmd: &str) -> Vec<String> {
    cmd.split_whitespace().map(str::to_owned).collect()
}

#[test]
fn discards_and_unrecognized_shapes_are_not_routine() {
    for cmd in [
        "git checkout HEAD -- src/schema.rs src/config.rs src/connections.rs Cargo.toml",
        "git checkout -- app.py",
        "git checkout -- .",
        "git checkout --",
        "git checkout -f main",
        "git checkout --force main",
        "git checkout --fo main",
        "git checkout -qf main",
        "git checkout -p HEAD~1",
        "git checkout --ours src/lib.rs",
        "git checkout main src/lib.rs",
        "git checkout main Makefile",
        "git checkout HEAD~3 -- src/",
        "git checkout --pathspec-from-file=paths.txt",
        "git checkout --pathspec-from-file paths.txt",
        "git checkout src/",
        "git checkout .gitignore",
        "git checkout src/.gitignore",
        "git checkout src/foo.lock",
        "git checkout a..b",
        "git checkout Cargo.toml",
        "git switch --discard-changes main",
        "git switch -f main",
        "git switch --disc main",
        "git stash drop",
        "git stash -q drop",
        "git stash clear",
        "git stash drop stash@{2}",
        "Git checkout main",
        "/usr/bin/git checkout main",
        "git worktree remove ../x",
        "git push origin main",
        "git reset --hard",
        "git clean -fd",
        "git restore .",
    ] {
        assert!(!git_words_are_routine(&words(cmd)), "{cmd}");
    }
    // The shell dequotes `"My Folder"` into one word, which the whitespace splitter above cannot express
    let quoted = [
        "git".to_owned(),
        "checkout".to_owned(),
        "My Folder".to_owned(),
    ];
    assert!(!git_words_are_routine(&quoted));
}

#[test]
fn branch_switching_and_local_workflow_are_routine() {
    for cmd in [
        "git checkout main",
        "git checkout",
        "git checkout -",
        "git checkout -q main",
        "git checkout --detach main",
        "git checkout -b feature/x",
        "git checkout -B feature/x origin/feature/x",
        "git checkout --orphan gh-pages",
        "git checkout release-1.0.3",
        "git checkout 2.x",
        "git checkout 1.2.x",
        "git checkout release/3.x",
        "git checkout release/1.2.x",
        "git checkout user/name/PROJ-123-topic-guard",
        "git switch feature/x",
        "git switch -",
        "git switch -c feature/y",
        "git stash",
        "git stash -m wip",
        "git stash push -m wip",
        "git stash pop",
        "git stash apply",
        "git stash list",
        "git stash show -p",
        "git add -A",
        "git commit -m x",
        "git pull",
        "git fetch origin",
        "git worktree list",
        "git status",
        "git diff HEAD",
    ] {
        assert!(git_words_are_routine(&words(cmd)), "{cmd}");
    }
}

#[test]
fn fetch_and_pull_that_name_a_program_or_transport_are_not_routine() {
    for cmd in [
        "git fetch --upload-pack=touch .",
        "git fetch --upload-pack touch .",
        "git fetch --upload-pack=/tmp/x origin",
        "git fetch --upl=touch .",
        "git fetch --no-upload-pack .",
        "git pull --upload-pack=touch .",
        "git pull -u .",
        "git fetch -u origin",
        "git fetch --exec=touch .",
        "git fetch ext::sh",
        "git fetch fd::3",
        "git pull evil::example.com/repo",
        "git fetch evil://example.com/repo",
        "git fetch ssh://-oProxyCommand=touch/x",
        "git pull --rebase=interactive",
        "git pull --rebase=i",
        "git pull -s evil",
        "git pull --strategy=evil",
        "git pull --edit",
        "git pull -e",
        "git pull --gpg-sign",
        "git pull -S",
        "git pull --verify-signatures",
        "git fetch -- origin",
        "git fetch -pq",
        "git fetch --stdin",
    ] {
        assert!(!git_words_are_routine(&words(cmd)), "{cmd}");
    }
}

/// The shell expands a glob into the workspace's file names before git sees
/// it, and a file can be named `--upload-pack=<program>`. A refspec pattern
/// is no exception: the fast path takes no glob at all.
#[test]
fn fetch_and_pull_words_the_shell_expands_are_not_routine() {
    for cmd in [
        "git fetch *",
        "git fetch origin *",
        "git fetch .[!.]*",
        "git fetch [-]-upload-pack=x",
        "git pull ?*",
        "git pull origin main*",
        "git fetch ssh://[-]oProxyCommand=x/repo",
        "git fetch --depth *",
        "git fetch -j [0-9]*",
        "git fetch --depth=*",
        "git fetch origin feature/*:feature/*",
        "git fetch origin +refs/heads/*:refs/remotes/origin/*",
        "git fetch origin refs/heads/*:refs/remotes/origin/*",
        "git fetch --refmap refs/heads/*:refs/remotes/mine/* origin",
        "git fetch ~/my-app",
        "git fetch origin {main,dev}",
        "git fetch $REMOTE",
    ] {
        assert!(!git_words_are_routine(&words(cmd)), "{cmd}");
    }
}

/// Every word is read on its own, so an option that takes a value, in any
/// spelling, an abbreviation, `--`, a global option before the verb and an
/// alias all go to the review.
#[test]
fn fetch_and_pull_outside_the_closed_grammar_are_not_routine() {
    for cmd in [
        // `pull`'s `-j[=<n>]` value is optional: a bare `-j` leaves the
        // next word to be read as an option of its own.
        "git pull -j --upload-pack=touch 4 .",
        "git pull --jobs --upload-pack=touch 4 .",
        "git pull -j -S 4 .",
        "git fetch -j --upload-pack=touch 4 .",
        "git fetch -j 4 origin",
        "git fetch --jobs 4",
        "git fetch -j4",
        "git fetch --jobs=4",
        "git pull -j4",
        "git pull --jobs=4",
        "git fetch --depth 1 origin",
        "git fetch --depth=1 origin main",
        "git fetch --deepen=1",
        "git fetch --shallow-since=2020-01-01",
        "git fetch --filter=blob:none",
        "git fetch -o x origin",
        "git fetch --server-option=x origin",
        "git fetch --negotiation-tip=main origin",
        "git fetch --refmap refs/heads/main:refs/remotes/mine/main origin",
        "git pull -X ours origin main",
        "git pull -X --upload-pack=touch .",
        "git pull --strategy-option=ours",
        "git pull --log=5",
        "git pull --cleanup=strip",
        // Abbreviations of long options, the dangerous and the plain.
        "git fetch --upload-p=touch .",
        "git fetch --upload=touch .",
        "git pull --upload-pa touch .",
        "git pull --rebase=interactiv",
        "git pull --reb=i",
        "git pull --strat=evil",
        "git pull --gpg=x",
        "git pull --verify-sig",
        "git fetch --quie",
        "git fetch --prun",
        "git pull --no-reb",
        // `--` makes every word after it an operand, even one that reads as an option.
        "git fetch -- origin",
        "git pull origin -- main",
        "git fetch --",
        // `-u` is `--upload-pack` to `pull` and `--update-head-ok` to `fetch`.
        "git pull -u .",
        "git pull -u touch .",
        "git fetch -u origin",
        // Global options and aliases never reach the fetch grammar.
        "git -c core.sshCommand=touch fetch",
        "git -c alias.fetch=!touch fetch",
        "git -c protocol.ext.allow=always fetch ext::sh",
        "git -ccore.sshCommand=touch pull",
        "git --config-env=core.sshCommand=EVIL fetch",
        "git --config-env core.sshCommand=EVIL pull",
        "git --exec-path=/tmp fetch",
        "git -C .. fetch",
        "git --no-pager fetch",
        "git f origin",
        "git sync",
        "git pul",
        "git fetch -c core.sshCommand=touch",
        // `--multiple` reads every operand as a repository.
        "git fetch -m origin upstream",
        "git fetch --multiple origin upstream",
        // Other transports and remote spellings.
        "git fetch http://example.com/my-app.git",
        "git fetch git://example.com/my-app.git",
        "git fetch file:///tmp/my-app",
        "git fetch example.com:my-app.git",
        "git fetch https://user:secret@example.com/my-app.git",
        "git fetch https://example.com:abc/my-app.git",
        "git fetch https://-oProxyCommand=x/repo",
        "git fetch ssh://-oProxyCommand=x/repo",
        "git fetch ssh://-user@example.com/repo",
        "git fetch -oProxyCommand=x@example.com:repo",
        "git fetch user@-oProxyCommand=x:repo",
        "git fetch git@example.com:-repo",
        "git fetch origin evil://example.com/repo",
        "git fetch origin ext::sh",
        "git fetch origin -upload",
        "git fetch origin .hidden",
        "git fetch origin main:",
        "git fetch origin :main",
        "git fetch origin ^refs/heads/main",
        "git fetch origin HEAD~1",
        "git fetch origin main@{1}",
    ] {
        assert!(!git_words_are_routine(&words(cmd)), "{cmd}");
    }
}

#[test]
fn everyday_fetch_and_pull_stay_routine() {
    for cmd in [
        "git fetch",
        "git fetch origin main",
        "git fetch --all --prune",
        "git fetch -p",
        "git fetch --tags origin",
        "git fetch --recurse-submodules=on-demand --no-tags",
        "git fetch origin +refs/heads/main:refs/remotes/origin/main",
        "git fetch origin refs/tags/v1.2.3:refs/tags/v1.2.3",
        "git fetch origin tag v1.2.3",
        "git fetch https://example.com/my-app.git main",
        "git fetch https://example.com:8443/team/my-app.git",
        "git fetch ssh://git@example.com:2222/team/my-app.git",
        "git fetch git@example.com:team/my-app.git main",
        "git fetch user@example.com:/srv/my-app.git",
        "git fetch ../my-app",
        "git fetch /srv/my-app.git",
        "git fetch .",
        "git pull --rebase",
        "git pull --rebase=merges",
        "git pull --no-rebase",
        "git pull --ff-only origin main",
        "git pull -q --autostash",
        "git pull --no-edit",
        "git pull origin main --rebase",
        "git pull -r upstream feature/x",
        "git pull --ff --log --stat origin",
    ] {
        assert!(git_words_are_routine(&words(cmd)), "{cmd}");
    }
}
