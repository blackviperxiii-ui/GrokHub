//! The agent's own `git push` from a Code tab chat, through the same push
//! guard as the Code tab's Push button (`ship::guard`).
//!
//! Every `git push` in a shell command is read before it runs. Only the plain
//! form is let through to the guard: `git push [-u] origin <branch>`, with
//! `HEAD`, `HEAD:<branch>` or `HEAD:refs/heads/<branch>` as the branch. A
//! force (`-f`, `--force`, `--force-with-lease`, `--force-if-includes`,
//! `+refspec`), `--mirror`, `--delete`, `--all`, `--tags`, `--prune`, a
//! remote other than origin, `git -C`/`-c` and anything the reader can't
//! follow is refused. A destination git would match as `refs/heads/<name>`
//! (`heads/main`) is read as that branch, and `HEAD` is read as the
//! checked-out branch only when nothing before the push in the same
//! command could switch it. A git word that isn't one of git's own commands
//! (an alias, from the command, a config file or the environment, or a
//! `git-<name>` program) may push anything, so it is refused too, and so is
//! a push in a command one of git's runners is given (`rebase -x`,
//! `submodule foreach`, `-c core.editor=…`). The guard then refuses main,
//! the default branch and any protected branch, in every mode. What it lets
//! through still asks: an agent's push is hard class `ship`, which no mode
//! skips.

use crate::activity::{self as code_activity, Entry, Kind as ActKind};
use crate::ship::{self, Blocked, CodeShip, Go, Probe};
use grokhub_agent::harness::{GIT_OWN_COMMANDS, GIT_RUNNERS};
use std::path::Path;

pub const FORCE: &str =
    "Force-pushing is never allowed, in any mode. Push without it, or make a new branch.";
pub const DELETE: &str = "Deleting a branch on the remote isn't allowed from chat.";
pub const NAME_IT: &str = "Name the branch: git push origin <branch>.";
pub const UNCLEAR: &str = "This command pushes in a way the push guard can't read. Run git push origin <branch> on its own.";
pub const ONE_BRANCH: &str = "Push one branch at a time: git push origin <branch>.";
pub const HEAD_MAY_MOVE: &str = "A command before git push origin HEAD may switch the branch. Name the branch: git push origin <branch>.";

/// Where one `git push` goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// `git push origin HEAD`: the checked-out branch, same name on origin.
    Current,
    Branch(String),
}

/// What a shell command does with `git push`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Read {
    NoPush,
    Pushes(Vec<Target>),
    Refused(String),
}

/// What the agent's tool call gets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    Run,
    Ask,
    Refuse(String),
}

/// Flags that change nothing about where a push goes.
const PLAIN_FLAGS: &[&str] = &[
    "-u",
    "--set-upstream",
    "--porcelain",
    "-q",
    "--quiet",
    "-v",
    "--verbose",
    "--progress",
    "--no-verify",
];

/// git's own options before the subcommand that move where it works, or
/// what it reads and runs.
const MOVING_GIT_OPTIONS: &[&str] = &[
    "-C",
    "-c",
    "--git-dir",
    "--work-tree",
    "--namespace",
    "--exec-path",
];

/// git's own options before the subcommand that take the next word as their
/// value when it isn't joined with `=`.
const GIT_VALUE_OPTIONS: &[&str] = &[
    "-C",
    "-c",
    "--git-dir",
    "--work-tree",
    "--namespace",
    "--super-prefix",
    "--attr-source",
];

/// git's own commands other than `push` that push. The reader doesn't
/// follow them.
const OTHER_PUSHES: &[&str] = &["send-pack", "http-push"];

/// git's own commands that can leave HEAD on another branch.
const SWITCHES_BRANCH: &[&str] = &[
    "checkout",
    "switch",
    "branch",
    "symbolic-ref",
    "stash",
    "update-ref",
];

fn not_gits_own(verb: &str) -> String {
    format!(
        "git {verb} isn't one of git's own commands, so it may be an alias that pushes, which the push guard can't read. Use git's own commands, and push with git push origin <branch>."
    )
}

fn is_git(word: &str) -> bool {
    let leaf = word.rsplit(['/', '\\']).next().unwrap_or(word);
    leaf.eq_ignore_ascii_case("git") || leaf.eq_ignore_ascii_case("git.exe")
}

/// One simple command between `;`, `&&`, `||`, `|`, `&` or a newline.
struct Seg {
    words: Vec<String>,
    raw: String,
    /// Substitutions, escapes or redirections the reader doesn't follow.
    odd: bool,
}

/// The command split into segments. Separators inside quotes don't split.
fn segments(command: &str) -> Vec<Seg> {
    let mut out = Vec::new();
    let mut seg = Seg {
        words: Vec::new(),
        raw: String::new(),
        odd: false,
    };
    let mut word = String::new();
    let mut quote: Option<char> = None;
    let end_word = |seg: &mut Seg, word: &mut String| {
        if !word.is_empty() {
            seg.words.push(std::mem::take(word));
        }
    };
    for c in command.chars() {
        if let Some(q) = quote {
            seg.raw.push(c);
            if q == '"' && "`$\\".contains(c) {
                seg.odd = true;
            }
            if c == q {
                quote = None;
            } else {
                word.push(c);
            }
            continue;
        }
        match c {
            '\'' | '"' => {
                quote = Some(c);
                seg.raw.push(c);
            }
            '\n' | ';' | '&' | '|' => {
                end_word(&mut seg, &mut word);
                if !seg.words.is_empty() || seg.odd {
                    out.push(std::mem::replace(
                        &mut seg,
                        Seg {
                            words: Vec::new(),
                            raw: String::new(),
                            odd: false,
                        },
                    ));
                }
            }
            c if c.is_whitespace() => {
                end_word(&mut seg, &mut word);
                seg.raw.push(c);
            }
            c => {
                if "`\\(){}<>$".contains(c) {
                    seg.odd = true;
                }
                word.push(c);
                seg.raw.push(c);
            }
        }
    }
    if quote.is_some() {
        seg.odd = true;
    }
    end_word(&mut seg, &mut word);
    if !seg.words.is_empty() || seg.odd {
        out.push(seg);
    }
    out
}

fn target_of(refspec: &str) -> Result<Target, String> {
    if refspec.starts_with('+') {
        return Err(FORCE.into());
    }
    let dst = match refspec.split_once(':') {
        Some(("", _)) => return Err(DELETE.into()),
        Some((src, dst)) => {
            if src != "HEAD" && !src.is_empty() && ship::branch_ok(src).is_err() {
                return Err(UNCLEAR.into());
            }
            dst
        }
        None if refspec == "HEAD" => return Ok(Target::Current),
        None => refspec,
    };
    let name = if let Some(name) = dst.strip_prefix("refs/heads/") {
        name
    } else if let Some(name) = dst.strip_prefix("heads/") {
        // git matches a short destination on origin as `refs/<dst>` too, so
        // `heads/main` updates main.
        name
    } else if ["refs/", "tags/", "remotes/"]
        .iter()
        .any(|p| dst.starts_with(p))
    {
        return Err(UNCLEAR.into());
    } else {
        dst
    };
    if name.starts_with("refs/") || name == "HEAD" || ship::branch_ok(name).is_err() {
        return Err(UNCLEAR.into());
    }
    Ok(Target::Branch(name.to_string()))
}

/// One `git push`'s words after `push`.
fn read_push(args: &[String]) -> Result<Target, String> {
    let mut plain = Vec::new();
    for arg in args {
        let a = arg.as_str();
        if a == "-f"
            || a.starts_with("--force")
            || (a.starts_with('-') && !a.starts_with("--") && a.contains('f'))
        {
            return Err(FORCE.into());
        }
        if a == "-d" || a == "--delete" || a == "--prune" || a == "--mirror" {
            return Err(DELETE.into());
        }
        if a.starts_with('-') {
            if PLAIN_FLAGS.contains(&a) {
                continue;
            }
            return Err(format!(
                "git push {a} isn't a push the guard can check. {NAME_IT}"
            ));
        }
        plain.push(a);
    }
    match plain.as_slice() {
        [] | ["origin"] => Err(NAME_IT.into()),
        ["origin", refspec] => target_of(refspec),
        [remote, _] => Err(format!(
            "Push to origin only; the guard doesn't check {remote}."
        )),
        _ => Err(ONE_BRANCH.into()),
    }
}

/// A quoted word that may be a command git runs pushes, or can't be read:
/// a script one of git's runners is given, or a value before git's verb
/// (`core.editor=<command>`, `GIT_CONFIG_VALUE_0=<command>`).
fn hides_a_push(word: &str) -> bool {
    if !word.contains(char::is_whitespace) {
        return false;
    }
    let script = match word.split_once('=') {
        Some((key, value)) if !key.contains(char::is_whitespace) => value,
        _ => word,
    };
    read(script) != Read::NoPush
}

/// The words one of git's runners is given hold a push, or a command the
/// reader can't follow: a quoted script, or a git command among them
/// (`submodule foreach git push`).
fn runs_a_push(args: &[String]) -> bool {
    args.iter()
        .enumerate()
        .any(|(k, w)| hides_a_push(w) || (is_git(w) && read(&args[k..].join(" ")) != Read::NoPush))
}

/// Every `git push` in `command`, or why it is refused.
pub fn read(command: &str) -> Read {
    let mut pushes = Vec::new();
    let segs = segments(command);
    // A push after `cd` runs in another folder than the one the guard reads.
    let moves = segs.iter().any(|seg| {
        seg.words
            .first()
            .is_some_and(|w| ["cd", "pushd", "popd"].contains(&w.as_str()))
    });
    // `git push origin HEAD` is read as the branch checked out now, so
    // nothing before it in the command may switch the branch: a script or
    // another program may, as may git's own `checkout`, `switch` and kin.
    let mut switched = false;
    for seg in segs {
        let words = &seg.words;
        // Every word, a quoted one split again: `bash -c "git push …"`.
        let tokens: Vec<&str> = words.iter().flat_map(|w| w.split_whitespace()).collect();
        let says_push = tokens.contains(&"push");
        let says_git = tokens.iter().any(|w| is_git(w));
        if seg.odd && seg.raw.contains("push") && seg.raw.contains("git") {
            return Read::Refused(UNCLEAR.into());
        }
        let Some(at) = words.iter().position(|w| !w.contains('=')) else {
            continue;
        };
        let head = &words[at];
        if moves && says_push && says_git {
            return Read::Refused(UNCLEAR.into());
        }
        if !is_git(head) {
            // `sudo git push`, `bash -c "git push"`, `xargs git push`: not plain.
            if says_push && says_git {
                return Read::Refused(UNCLEAR.into());
            }
            switched = true;
            continue;
        }
        if words[..at].iter().any(|w| w.starts_with("GIT_")) && says_push {
            return Read::Refused(UNCLEAR.into());
        }
        let mut i = at + 1;
        let mut moved = false;
        while let Some(w) = words.get(i).filter(|w| w.starts_with('-')) {
            let name = w.split_once('=').map_or(w.as_str(), |(name, _)| name);
            // Sets any config, an alias or an editor too, from a variable
            // the reader can't see.
            if name == "--config-env" {
                return Read::Refused(UNCLEAR.into());
            }
            moved |= MOVING_GIT_OPTIONS.contains(&name);
            i += if GIT_VALUE_OPTIONS.contains(&w.as_str()) {
                2
            } else {
                1
            };
        }
        let before_verb = &words[..i.min(words.len())];
        if (moved && says_push) || before_verb.iter().any(|w| hides_a_push(w)) {
            return Read::Refused(UNCLEAR.into());
        }
        let Some(verb) = words.get(i).map(String::as_str) else {
            continue;
        };
        let args = &words[i + 1..];
        if verb == "push" {
            match read_push(args) {
                Ok(Target::Current) if switched => {
                    return Read::Refused(HEAD_MAY_MOVE.into());
                }
                Ok(t) => pushes.push(t),
                Err(why) => return Read::Refused(why),
            }
        } else if OTHER_PUSHES.contains(&verb) {
            return Read::Refused(UNCLEAR.into());
        } else if GIT_RUNNERS.contains(&verb) {
            if says_push || runs_a_push(args) {
                return Read::Refused(UNCLEAR.into());
            }
            switched = true;
        } else if GIT_OWN_COMMANDS.contains(&verb) {
            switched |= SWITCHES_BRANCH.contains(&verb);
        } else {
            return Read::Refused(not_gits_own(verb));
        }
    }
    if pushes.is_empty() {
        Read::NoPush
    } else {
        Read::Pushes(pushes)
    }
}

/// The verdict for one shell command the agent wants to run in `dir`.
/// `None` when it has no `git push`.
pub fn decide(config_dir: &Path, dir: &Path, cfg: &CodeShip, command: &str) -> Option<Verdict> {
    decide_with(config_dir, dir, cfg, command, None)
}

fn blocked(config_dir: &Path, cfg: &CodeShip, command: &str, branch: &str, why: &str) {
    log(
        config_dir,
        cfg,
        ActKind::Blocked,
        command,
        branch,
        "none",
        why,
    );
}

fn log(
    config_dir: &Path,
    cfg: &CodeShip,
    kind: ActKind,
    command: &str,
    branch: &str,
    approved_by: &str,
    summary: &str,
) {
    let _ = code_activity::append(
        config_dir,
        &Entry {
            ts_ms: grokhub_core::now_ms(),
            kind,
            repo_key: String::new(),
            host: String::new(),
            branch: branch.to_string(),
            command: command.to_string(),
            mode: cfg.mode.id().into(),
            approved_by: approved_by.into(),
            exit: None,
            summary: format!("From chat: {summary}"),
        },
        cfg.activity_keep,
    );
}

pub(crate) fn decide_with(
    config_dir: &Path,
    dir: &Path,
    cfg: &CodeShip,
    command: &str,
    probe: Probe,
) -> Option<Verdict> {
    let targets = match read(command) {
        Read::NoPush => return None,
        Read::Refused(why) => {
            blocked(config_dir, cfg, command, "", &why);
            return Some(Verdict::Refuse(why));
        }
        Read::Pushes(t) => t,
    };
    let mut ask = false;
    let mut branches = Vec::new();
    for target in targets {
        let branch = match target {
            Target::Branch(b) => b,
            Target::Current => match ship::current_branch(dir) {
                Some(b) => b,
                None => {
                    let why = Blocked::Detached.message("");
                    blocked(config_dir, cfg, command, "", &why);
                    return Some(Verdict::Refuse(why));
                }
            },
        };
        let verdict = ship::facts_for(config_dir, dir, &branch, &probe)
            .and_then(|(_, facts)| ship::guard(cfg, &facts));
        match verdict {
            Ok(Go::NoConfirm) => {}
            Ok(Go::Confirm) => ask = true,
            Err(b) => {
                let why = b.message(&branch);
                blocked(config_dir, cfg, command, &branch, &why);
                return Some(Verdict::Refuse(why));
            }
        }
        branches.push(branch);
    }
    if ask {
        return Some(Verdict::Ask);
    }
    log(
        config_dir,
        cfg,
        ActKind::Push,
        command,
        &branches.join(", "),
        "autoBeta",
        "the agent's push ran with no confirm",
    );
    Some(Verdict::Run)
}

#[cfg(test)]
#[path = "chat_push_tests.rs"]
mod tests;
