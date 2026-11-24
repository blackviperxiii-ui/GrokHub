//! Which words after `git` are git's own commands, and which of git's own
//! commands and options run a command they are given.
//!
//! git runs `git <word>` as a command compiled into it if it has one, else
//! as a `git-<word>` program in its exec path or on PATH, and only then as
//! an alias. So a word the installed git runs itself can't be an alias, and
//! any other word may be one (set on the line with `-c alias.<word>=`, in a
//! config file or through the environment) that pushes anything. Which words
//! git runs itself depends on its version and on what is installed: `refs`
//! is a command from git 2.46 on, `lfs` only with git-lfs installed. So the
//! installed git is asked, not a fixed list.

use std::sync::OnceLock;
use std::time::Duration;

/// Commands compiled into every git since 2.0 (2014). They are git's own on
/// any git a person runs, so these are read without asking git.
const BUILTINS_SINCE_2_0: &[&str] = &[
    "add",
    "annotate",
    "apply",
    "archive",
    "blame",
    "branch",
    "bundle",
    "cat-file",
    "check-attr",
    "check-ignore",
    "check-mailmap",
    "check-ref-format",
    "checkout",
    "checkout-index",
    "cherry",
    "cherry-pick",
    "clean",
    "clone",
    "column",
    "commit",
    "commit-tree",
    "config",
    "count-objects",
    "credential",
    "describe",
    "diff",
    "diff-files",
    "diff-index",
    "diff-tree",
    "fast-export",
    "fetch",
    "fetch-pack",
    "fmt-merge-msg",
    "for-each-ref",
    "format-patch",
    "fsck",
    "gc",
    "grep",
    "hash-object",
    "help",
    "index-pack",
    "init",
    "log",
    "ls-files",
    "ls-remote",
    "ls-tree",
    "mailinfo",
    "mailsplit",
    "merge",
    "merge-base",
    "merge-file",
    "merge-tree",
    "mktag",
    "mktree",
    "mv",
    "name-rev",
    "notes",
    "pack-objects",
    "pack-refs",
    "patch-id",
    "prune",
    "push",
    "read-tree",
    "reflog",
    "remote",
    "repack",
    "replace",
    "rerere",
    "reset",
    "rev-list",
    "rev-parse",
    "revert",
    "rm",
    "send-pack",
    "shortlog",
    "show",
    "show-branch",
    "show-ref",
    "status",
    "stripspace",
    "symbolic-ref",
    "tag",
    "unpack-objects",
    "update-index",
    "update-ref",
    "var",
    "verify-pack",
    "verify-tag",
    "write-tree",
];

/// git's own commands that run a command they are given: `rebase -x`,
/// `submodule foreach`, `bisect run`, `hook run`, `for-each-repo`, a
/// difftool or mergetool command, a `filter-branch` filter.
pub const GIT_RUNNERS: &[&str] = &[
    "bisect",
    "difftool",
    "filter-branch",
    "for-each-repo",
    "hook",
    "mergetool",
    "rebase",
    "submodule",
];

/// git's own options whose value is a command git runs, by command:
/// `grep -O<pager>`, `fetch --upload-pack=<command>`, `rebase -x <command>`.
pub const GIT_COMMAND_OPTIONS: &[(&str, &[&str])] = &[
    ("archive", &["--exec"]),
    ("clone", &["-u", "--upload-pack"]),
    ("difftool", &["-x", "--extcmd"]),
    ("fetch", &["--upload-pack"]),
    ("fetch-pack", &["--upload-pack", "--exec"]),
    ("grep", &["-O", "--open-files-in-pager"]),
    ("instaweb", &["-d", "--httpd", "-b", "--browser"]),
    ("ls-remote", &["--upload-pack", "--exec"]),
    ("pull", &["--upload-pack"]),
    ("rebase", &["-x", "--exec"]),
    (
        "send-email",
        &["--sendmail-cmd", "--to-cmd", "--cc-cmd", "--header-cmd"],
    ),
];

/// What the installed git lists for `git --list-cmds=<kinds>`, or `None`
/// when git can't be run or lists nothing. It runs outside any repository,
/// so no repository's config is read.
fn list_cmds(kinds: &str) -> Option<Vec<String>> {
    let mut cmd = std::process::Command::new("git");
    cmd.arg(format!("--list-cmds={kinds}"))
        .current_dir(std::env::temp_dir());
    let out = grokhub_core::proc_util::output_within(&mut cmd, Duration::from_secs(3), 1 << 20)?;
    let list: Vec<String> = out
        .lines()
        .map(str::trim)
        .filter(|w| !w.is_empty())
        .map(str::to_string)
        .collect();
    (!list.is_empty()).then_some(list)
}

/// The commands compiled into the installed git. They don't change while
/// the app runs, so they are asked for once; a failed ask is asked again.
fn installed_builtins() -> Option<&'static [String]> {
    static BUILTINS: OnceLock<Vec<String>> = OnceLock::new();
    if let Some(list) = BUILTINS.get() {
        return Some(list);
    }
    let list = list_cmds("builtins")?;
    Some(BUILTINS.get_or_init(|| list))
}

/// Whether `git <verb>` runs git's own command on this machine: a command
/// compiled into the installed git, or a `git-<verb>` program in its exec
/// path or on PATH. git runs either before it looks for an alias, so no
/// alias can stand in for one. Any other word may be an alias that pushes.
///
/// Programs are looked up on every call, not kept: one removed since could
/// have left an alias of its name in its place.
pub fn git_runs_own(verb: &str) -> bool {
    if BUILTINS_SINCE_2_0.contains(&verb) {
        return true;
    }
    // Not a name git could look up as a command.
    if verb.is_empty()
        || verb.starts_with('-')
        || verb.contains(|c: char| c.is_whitespace() || c == '/' || c == '\\' || c == '=')
    {
        return false;
    }
    installed_builtins().is_some_and(|list| list.iter().any(|c| c == verb))
        || list_cmds("main,others").is_some_and(|list| list.iter().any(|c| c == verb))
}

/// The values `args` gives the options in `opts`, as git's option parser
/// reads them: `--opt=<value>`, `--opt <value>`, the start of a long option
/// (`--upload=<value>`), `-X<value>`, `-X <value>`, and `-X` at the end of
/// a cluster of short options (`-nO<value>`). A word taken that git would
/// read otherwise is only read too, never let through.
pub fn git_option_values<'a>(args: &'a [String], opts: &[&str]) -> Vec<&'a str> {
    let mut out = Vec::new();
    let mut k = 0;
    while let Some(word) = args.get(k) {
        k += 1;
        if word == "--" {
            break;
        }
        if let Some(long) = word.strip_prefix("--") {
            let (name, value) = match long.split_once('=') {
                Some((name, value)) => (name, Some(value)),
                None => (long, None),
            };
            let named = !name.is_empty()
                && opts
                    .iter()
                    .filter_map(|o| o.strip_prefix("--"))
                    .any(|o| o.starts_with(name));
            if named {
                match value {
                    Some(value) => out.push(value),
                    None => {
                        if let Some(next) = args.get(k) {
                            out.push(next.as_str());
                            k += 1;
                        }
                    }
                }
            }
            continue;
        }
        let Some(cluster) = word.strip_prefix('-') else {
            continue;
        };
        let short = cluster.char_indices().find(|&(_, c)| {
            opts.iter().any(|o| {
                let mut chars = o.chars();
                chars.next() == Some('-') && chars.next() == Some(c) && chars.next().is_none()
            })
        });
        if let Some((at, c)) = short {
            let rest = &cluster[at + c.len_utf8()..];
            if !rest.is_empty() {
                out.push(rest);
            } else if let Some(next) = args.get(k) {
                out.push(next.as_str());
                k += 1;
            }
        }
    }
    out
}

/// The command a value of git's runs: a `!` at its start (an alias or a
/// credential helper) says to run the rest in a shell.
pub fn git_shell_value(value: &str) -> &str {
    value.trim_start().trim_start_matches('!')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(line: &str) -> Vec<String> {
        line.split_whitespace().map(str::to_string).collect()
    }

    #[test]
    fn an_option_value_is_read_in_every_form_git_takes() {
        let opts = &["-u", "--upload-pack"][..];
        for (line, expected) in [
            ("--upload-pack=x origin", vec!["x"]),
            ("--upload-pack x origin", vec!["x"]),
            ("--upload=x origin", vec!["x"]),
            ("-ux origin", vec!["x"]),
            ("-u x origin", vec!["x"]),
            ("-qu x origin", vec!["x"]),
            ("-q --depth=1 origin", vec![]),
            ("-- --upload-pack=x", vec![]),
        ] {
            assert_eq!(git_option_values(&words(line), opts), expected, "{line}");
        }
        let pager = &["-O", "--open-files-in-pager"][..];
        assert_eq!(
            git_option_values(&words("-n -Ovim -e x"), pager),
            vec!["vim"]
        );
        // `-o` is grep's --only-matching, not `-O`.
        assert_eq!(
            git_option_values(&words("-n -o -e x"), pager),
            Vec::<&str>::new()
        );
        assert_eq!(git_shell_value("!git push"), "git push");
        assert_eq!(git_shell_value("vim"), "vim");
    }

    /// git itself is the judge: give each word an alias on the line, run
    /// it, and see whether the alias ran. A word whose alias runs is not
    /// git's own; a word whose alias git ignores is.
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
}
