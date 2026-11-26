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

/// The only options a routine `fetch` or `pull` may carry, spelled exactly.
/// None takes a value from the next word, so every word of the command is
/// read on its own: an option that takes one (`--depth`, `-j`, whose `pull`
/// value is optional and must be stuck to it), an abbreviation
/// (`--upload-p`), a cluster (`-pq`), `--`, and every option that names a
/// program, a merge strategy, an editor or signing go to the review.
const FETCH_FLAGS: &[&str] = &[
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

/// `git fetch` / `git pull` as `[<flag>...] [<repository> [<refspec>...]]`
/// with every flag on [`FETCH_FLAGS`], a plain repository and plain
/// refspecs. Flags and operands may mix, as git allows. `--multiple`, which
/// reads every operand as a repository, is not listed.
fn is_routine_fetch(args: &[String]) -> bool {
    let mut operands = args
        .iter()
        .map(String::as_str)
        .filter(|word| !FETCH_FLAGS.contains(word));
    operands.next().is_none_or(is_plain_repository) && operands.all(is_plain_refspec)
}

/// A remote name or a local path (no `:`, so git reads no transport into
/// it), an `https://` or `ssh://` URL, or an scp-style `user@host:path`.
/// Every part starts with a letter, a digit or `_` where ssh would read a
/// leading `-` as an option, and no word holds a character the shell
/// expands (`*`, `?`, `[`, `{`, `~`, `$`) or quotes. A local path never
/// starts with `//`: on Windows that is a network share (`//server/share`),
/// which git reaches over SMB on another machine.
fn is_plain_repository(word: &str) -> bool {
    if let Some(rest) = word
        .strip_prefix("https://")
        .or_else(|| word.strip_prefix("ssh://"))
    {
        let (authority, path) = rest.split_once('/').unwrap_or((rest, ""));
        let (login, port) = authority.split_once(':').unwrap_or((authority, ""));
        let (user, host) = login
            .split_once('@')
            .map_or((None, login), |(u, h)| (Some(u), h));
        return user.is_none_or(is_plain_name)
            && is_plain_host(host)
            && port.bytes().all(|b| b.is_ascii_digit())
            && path.bytes().all(path_byte);
    }
    if let Some((login, path)) = word.split_once(':') {
        let Some((user, host)) = login.split_once('@') else {
            return false;
        };
        return is_plain_name(user)
            && is_plain_host(host)
            && path
                .bytes()
                .next()
                .is_some_and(|b| b == b'/' || name_start(b))
            && path.bytes().all(path_byte);
    }
    word.bytes()
        .next()
        .is_some_and(|b| b == b'.' || b == b'/' || name_start(b))
        && !word.starts_with("//")
        && word.bytes().all(path_byte)
}

/// `[+]<ref>[:<ref>]`, each ref a `/`-joined list of plain names: no glob,
/// no `::` or `://` (a URL `--multiple` would read as a repository), no
/// leading `-` or `.`.
fn is_plain_refspec(word: &str) -> bool {
    let spec = word.strip_prefix('+').unwrap_or(word);
    let is_ref = |r: &str| r.split('/').all(is_plain_name);
    match spec.split_once(':') {
        Some((src, dst)) => is_ref(src) && is_ref(dst),
        None => is_ref(spec),
    }
}

/// A remote, branch, user or path component: a letter, digit or `_`, then
/// letters, digits, `_`, `.` and `-`.
fn is_plain_name(name: &str) -> bool {
    name.bytes().next().is_some_and(name_start)
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'-'))
}

fn is_plain_host(host: &str) -> bool {
    host.bytes()
        .next()
        .is_some_and(|b| b.is_ascii_alphanumeric())
        && host
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-'))
}

fn name_start(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

fn path_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'-' | b'/')
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
