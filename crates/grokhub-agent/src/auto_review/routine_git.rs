// Portions derived from xai-org/grok-build (Apache-2.0, © SpaceXAI), commit 2bdd1d6a; modified.

//! Fail-closed `git` arm of the routine-bash heuristic: any shape not recognized here goes to the model.
//! Callers pass wrapper-peeled words with `words[0]` literally `git`; a case-variant or path-qualified spelling may resolve to a different binary.

use crate::perm::{git_words_are_read_only_query, git_words_have_unsafe_query_option};

pub(super) fn git_words_are_routine(words: &[String]) -> bool {
    if words.first().map(String::as_str) != Some("git") {
        return false;
    }
    if git_words_are_read_only_query(words) {
        return true;
    }
    if git_words_have_unsafe_query_option(words) {
        return false;
    }
    let Some(verb) = words.get(1).map(String::as_str) else {
        return false;
    };
    let Some(args) = words.get(2..) else {
        return false;
    };
    match verb {
        "add" | "commit" => true,
        "fetch" | "pull" => is_routine_fetch(args),
        "worktree" => args.first().map(String::as_str) == Some("list"),
        "checkout" | "switch" => is_routine_branch_switch(args),
        "stash" => is_routine_stash(args),
        _ => false,
    }
}

/// `fetch` and `pull` options that pick what is fetched or how it is merged.
/// None names a program, a config value or a transport: `--upload-pack` (and
/// `-u`, which is `--upload-pack` to `clone`), `-s` (it runs
/// `git-merge-<name>`), `--edit`, `--gpg-sign` and an interactive rebase
/// fail closed with every other unlisted spelling.
const FETCH_BENIGN_FLAGS: &[&str] = &[
    "-q",
    "--quiet",
    "-v",
    "--verbose",
    "--progress",
    "--no-progress",
    "--all",
    "-a",
    "--append",
    "-f",
    "--force",
    "-t",
    "--tags",
    "-n",
    "--no-tags",
    "-p",
    "--prune",
    "--no-prune",
    "-P",
    "--prune-tags",
    "-m",
    "--multiple",
    "--atomic",
    "--dry-run",
    "-k",
    "--keep",
    "--unshallow",
    "--update-shallow",
    "--set-upstream",
    "--show-forced-updates",
    "--no-show-forced-updates",
    "--recurse-submodules",
    "--no-recurse-submodules",
    "--recurse-submodules=yes",
    "--recurse-submodules=on-demand",
    "--recurse-submodules=no",
    "-4",
    "--ipv4",
    "-6",
    "--ipv6",
    // `pull` only.
    "-r",
    "--rebase",
    "--rebase=true",
    "--rebase=false",
    "--rebase=merges",
    "--no-rebase",
    "--ff",
    "--no-ff",
    "--ff-only",
    "--stat",
    "--no-stat",
    "--log",
    "--no-log",
    "--squash",
    "--no-squash",
    "--commit",
    "--no-commit",
    "--no-edit",
    "--autostash",
    "--no-autostash",
    "--allow-unrelated-histories",
    "--signoff",
];

/// Options whose value (the next word, or after `=`) is a count, a date, a
/// revision, a refspec or an option for the merge, never a program.
const FETCH_VALUE_FLAGS: &[&str] = &[
    "-j",
    "--jobs",
    "--depth",
    "--deepen",
    "--shallow-since",
    "--shallow-exclude",
    "--negotiation-tip",
    "--refmap",
    "--filter",
    "-X",
    "--strategy-option",
];

/// The remote helpers git ships, by URL scheme. Any other `<scheme>://` or
/// `<transport>::` remote runs a `git-remote-<name>` program.
const FETCH_SCHEMES: &[&str] = &["https", "http", "ssh", "git", "file", "git+ssh", "ssh+git"];

/// `git fetch` / `git pull` with listed options and plain remote or refspec
/// operands. `--` fails closed: after it, a word that reads as an option is
/// an operand.
fn is_routine_fetch(args: &[String]) -> bool {
    let mut it = args.iter().map(String::as_str);
    while let Some(word) = it.next() {
        if shell_may_expand(word) {
            return false;
        }
        if FETCH_BENIGN_FLAGS.contains(&word) {
            continue;
        }
        if FETCH_VALUE_FLAGS.contains(&word) {
            if it.next().is_none_or(shell_may_expand) {
                return false;
            }
            continue;
        }
        if word.starts_with('-') {
            // `--depth=1`, `--log=5`: a listed option with its value after `=`.
            let listed = word
                .split_once('=')
                .is_some_and(|(flag, _)| FETCH_VALUE_FLAGS.contains(&flag) || flag == "--log");
            if !listed {
                return false;
            }
            continue;
        }
        if !is_plain_fetch_operand(word) {
            return false;
        }
    }
    true
}

/// A glob the shell expands into the workspace's file names before git sees
/// them, and a cloned repo can hold a file named `--upload-pack=<program>`.
/// A refspec pattern (`+refs/heads/*:refs/remotes/origin/*`) stays: whatever
/// it matches starts with `refs/`, never with `-`.
fn shell_may_expand(word: &str) -> bool {
    word.contains(['*', '?', '[']) && !word.strip_prefix('+').unwrap_or(word).starts_with("refs/")
}

/// A remote name, a refspec, a local path, or the address of a transport git
/// ships. ssh reads a user, host or port that starts with `-` as an option of
/// its own, so that fails closed too.
fn is_plain_fetch_operand(word: &str) -> bool {
    if word.contains("::") || word.contains(char::is_whitespace) {
        return false;
    }
    let address = match word.split_once("://") {
        Some((scheme, rest)) if FETCH_SCHEMES.contains(&scheme) => rest,
        Some(_) => return false,
        None => word,
    };
    let authority = address.split('/').next().unwrap_or_default();
    !authority
        .split(['@', ':'])
        .any(|part| part.starts_with('-'))
}

/// Exact spellings only, so clusters (`-qf`) and abbreviations (`--fo`) fail closed.
const BRANCH_SWITCH_BENIGN_FLAGS: &[&str] = &[
    "-q",
    "--quiet",
    "-d",
    "--detach",
    "-t",
    "--track",
    "--no-track",
    "--guess",
    "--no-guess",
    "--progress",
    "--no-progress",
    "--recurse-submodules",
    "--no-recurse-submodules",
];

/// An unlisted option or a second operand means path mode or a discard, so both fail closed.
fn is_routine_branch_switch(args: &[String]) -> bool {
    let mut operands = 0;
    let mut it = args.iter().map(String::as_str);
    while let Some(word) = it.next() {
        match word {
            "-b" | "-B" | "-c" | "-C" | "--orphan" => {
                it.next();
            }
            // Bare `-` is the previous branch
            "-" => operands += 1,
            _ if BRANCH_SWITCH_BENIGN_FLAGS.contains(&word) => {}
            _ if word.starts_with('-') => return false,
            _ => {
                if operand_reads_as_path(word) {
                    return false;
                }
                operands += 1;
            }
        }
    }
    operands <= 1
}

/// `drop` and `clear` throw away a stash, so only the recoverable subcommands are listed.
fn is_routine_stash(args: &[String]) -> bool {
    let mut it = args.iter().map(String::as_str);
    let subcommand = loop {
        match it.next() {
            Some("-m" | "--message") => {
                it.next();
            }
            Some("-q" | "--quiet") => {}
            Some(word) if word.starts_with('-') => return false,
            other => break other,
        }
    };
    matches!(
        subcommand,
        None | Some("push" | "save" | "pop" | "apply" | "list" | "show" | "branch")
    )
}

/// A dotless bare name (`Makefile`, `src`) is indistinguishable from a branch without a repo lookup and stays routine.
fn operand_reads_as_path(op: &str) -> bool {
    // git-check-ref-format rejects these in a branch name, so the operand is a pathspec or a revision expression like `HEAD~1`
    if op
        .split('/')
        .any(|c| c.starts_with('.') || c.ends_with(".lock"))
        || op.ends_with('/')
        || op.ends_with('.')
        || op.contains("..")
        || op.contains("@{")
        || op.contains(['~', '^', ':', '\\'])
        || op.contains(char::is_whitespace)
    {
        return true;
    }
    if op.starts_with('/') || op.contains(['*', '?', '[']) {
        return true;
    }
    let Some((stem, ext)) = op.rsplit_once('.') else {
        return false;
    };
    // `2.x` / `release/1.2.x` are maintenance branches, not a one-letter file extension
    let stem_leaf = stem.rsplit('/').next().unwrap_or(stem);
    if ext == "x"
        && stem_leaf.bytes().any(|b| b.is_ascii_digit())
        && stem_leaf.bytes().all(|b| b.is_ascii_digit() || b == b'.')
    {
        return false;
    }
    (1..=5).contains(&ext.len())
        && ext.chars().all(|c| c.is_ascii_alphanumeric())
        && ext.chars().any(|c| c.is_ascii_alphabetic())
}

#[cfg(test)]
#[path = "routine_git_tests.rs"]
mod tests;
