//! The agent's own `git push` from a Code tab chat, through the same push
//! guard as the Code tab's Push button (`ship::guard`).
//!
//! Every `git push` in a shell command is read before it runs. Only the plain
//! form is let through to the guard: `git push [-u] origin <branch>`, with
//! `HEAD`, `HEAD:<branch>` or `HEAD:refs/heads/<branch>` as the branch. A
//! force (`-f`, `--force`, `--force-with-lease`, `--force-if-includes`,
//! `+refspec`), `--mirror`, `--delete`, `--all`, `--tags`, `--prune`, a
//! remote other than origin, `git -C`/`-c` and anything the reader can't
//! follow is refused. The guard then refuses main, the default branch and
//! any protected branch, in every mode. What it lets through runs with no
//! card in Auto-beta and asks otherwise.

use crate::activity::{self as code_activity, Entry, Kind as ActKind};
use crate::ship::{self, Blocked, CodeShip, Go, Probe};
use std::path::Path;

pub const FORCE: &str =
    "Force-pushing is never allowed, in any mode. Push without it, or make a new branch.";
pub const DELETE: &str = "Deleting a branch on the remote isn't allowed from chat.";
pub const NAME_IT: &str = "Name the branch: git push origin <branch>.";
pub const UNCLEAR: &str = "This command pushes in a way the push guard can't read. Run git push origin <branch> on its own.";
pub const ONE_BRANCH: &str = "Push one branch at a time: git push origin <branch>.";

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

/// git's own options before the subcommand that move where it works.
const MOVING_GIT_OPTIONS: &[&str] = &["-C", "-c", "--git-dir", "--work-tree", "--namespace"];

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
    let name = dst.strip_prefix("refs/heads/").unwrap_or(dst);
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
            continue;
        }
        if words[..at].iter().any(|w| w.starts_with("GIT_")) && says_push {
            return Read::Refused(UNCLEAR.into());
        }
        let mut i = at + 1;
        while let Some(w) = words.get(i) {
            if !w.starts_with('-') {
                break;
            }
            if MOVING_GIT_OPTIONS
                .iter()
                .any(|o| w == o || w.starts_with(&format!("{o}=")))
            {
                if says_push {
                    return Read::Refused(UNCLEAR.into());
                }
                break;
            }
            i += 1;
        }
        if words.get(i).map(String::as_str) != Some("push") {
            continue;
        }
        match read_push(&words[i + 1..]) {
            Ok(t) => pushes.push(t),
            Err(why) => return Read::Refused(why),
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
