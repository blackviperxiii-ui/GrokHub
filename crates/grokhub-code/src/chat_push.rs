//! The agent's own `git push` from a Code tab chat, through the same push
//! guard as the Code tab's Push button (`ship::guard`).
//!
//! Every git in a shell command is read before it runs, the way the shell
//! reads it (`grokhub_agent::harness::read_shell`: quotes, `g\\it`,
//! `$'…'`, wrappers, `bash -c` scripts). Only the plain form is let
//! through to the guard: `git push [-u] origin <branch>`, with `HEAD`,
//! `HEAD:<branch>` or `HEAD:refs/heads/<branch>` as the branch. A force
//! (`-f`, `--force`, `--force-with-lease`, `--force-if-includes`,
//! `+refspec`), `--mirror`, `--delete`, `--all`, `--tags`, `--prune`, a
//! remote other than origin, git's own options (`-C`, `-c`), a variable
//! that isn't plain, a push behind a wrapper or in a script, and a push in
//! a command with a part that may push in a way the reader can't follow
//! are refused. A git word the installed git doesn't run as its own command
//! (a built-in, or a `git-<name>` program such as git-lfs) may be an alias
//! that pushes, so it is refused too; after a `PATH` or `GIT_EXEC_PATH`
//! change only a built-in counts. A push's destination is found the way
//! git finds it: `heads/<name>` and `refs/heads/<name>` are that branch,
//! `HEAD` is the checked-out branch only when nothing before it in the
//! command could switch it, and a branch named on its own goes where the
//! repository's `remote.origin.push`, or `push.default=upstream` and the
//! branch's `branch.<name>.merge`, send it, or else where its symbolic ref
//! points. A push of a branch named on its own after a command that may
//! change a symbolic ref, or with `push.default=upstream` an upstream, is
//! refused as one the guard can't read. The guard then refuses main, the
//! default branch and any protected branch, in every mode.
//!
//! What can push but isn't read as a push here (an alias set on the line,
//! `-c`, a config write, an editor, `rebase -x`, text fed to `sh`) is left
//! to the harness: it is hard class `ship` by the closed rule in
//! `grokhub_agent::harness::command_may_push`, a card in every mode. What
//! this guard lets through still asks the same way.

use crate::activity::{self as code_activity, Entry, Kind as ActKind};
use crate::ship::{self, Blocked, CodeShip, Go, Probe};
use grokhub_agent::harness::{
    git_call, git_compiled_in, git_may_push, git_path_changed, git_runs_own, plain_assignment,
    read_shell, HIDDEN,
};
use grokhub_core::proc_util::hide_windows_console;
use std::path::Path;
use std::process::Stdio;

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

/// git's own options before the verb that change nothing about a push.
const PLAIN_GIT_OPTIONS: &[&str] = &["--no-pager", "-P", "--no-optional-locks", "--no-advice"];

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
    "rebase",
    "bisect",
    "submodule",
    "worktree",
];

/// Programs that change the folder the commands after them run in.
const MOVES: &[&str] = &["cd", "pushd", "popd", "chdir", "set-location", "sl"];

/// Whether git runs a word after `git` as its own command, not an alias:
/// [`git_runs_own`], or a stand-in in tests.
type Own<'a> = &'a dyn Fn(&str) -> bool;

fn not_gits_own(verb: &str) -> String {
    format!(
        "git {verb} isn't one of git's own commands, so it may be an alias that pushes, which the push guard can't read. Use git's own commands, and push with git push origin <branch>."
    )
}

/// One push the reader found: where it goes, and for a branch named on its
/// own (`git push origin code/fix`) the local branch git may map elsewhere.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Push {
    target: Target,
    local: Option<String>,
    /// A command before it may set a branch's upstream, which
    /// `push.default=upstream` sends a branch named on its own to.
    upstream_moved: bool,
}

/// Whether `git <verb> <args>` may make a branch a symbolic ref, which a
/// push of it follows (`git symbolic-ref refs/heads/x refs/heads/main`).
fn moves_refs(verb: &str, args: &[String]) -> bool {
    let plain = args.iter().filter(|a| !a.starts_with('-')).count();
    match verb {
        "symbolic-ref" => plain >= 2 || has_flag(args, &["-d", "--delete"]),
        "update-ref" => has_flag(args, &["--stdin"]),
        _ => false,
    }
}

/// Whether `git <verb> <args>` may set a branch's upstream: `branch -u`, a
/// branch made or copied (git sets one from its start), `checkout -b`,
/// `switch -c`, `worktree add`, `push -u`, `fetch --set-upstream`,
/// `remote rename`.
fn moves_upstream(verb: &str, args: &[String]) -> bool {
    let words = args.iter().filter(|a| !a.starts_with('-')).count();
    match verb {
        "branch" => {
            let shows = has_flag(
                args,
                &[
                    "-d",
                    "-D",
                    "--delete",
                    "-l",
                    "--list",
                    "-a",
                    "--all",
                    "-r",
                    "--remotes",
                    "--show-current",
                    "--contains",
                    "--no-contains",
                    "--merged",
                    "--no-merged",
                    "--points-at",
                    "--edit-description",
                ],
            );
            (words > 0 && !shows)
                || has_flag(
                    args,
                    &[
                        "-u",
                        "--set-upstream-to",
                        "-t",
                        "--track",
                        "--set-upstream",
                        "-c",
                        "-C",
                        "--copy",
                        "-m",
                        "-M",
                        "--move",
                    ],
                )
        }
        "checkout" => has_flag(args, &["-b", "-B", "-t", "--track"]),
        "switch" => has_flag(
            args,
            &["-c", "-C", "--create", "--force-create", "-t", "--track"],
        ),
        "worktree" => args.iter().any(|a| a == "add"),
        "push" => has_flag(args, &["-u", "--set-upstream"]),
        "fetch" | "pull" => has_flag(args, &["--set-upstream"]),
        "remote" => args.iter().any(|a| a == "rename"),
        _ => false,
    }
}

/// One of `opts` is among `args`: `--name`, `--name=…`, or a short one in a
/// cluster (`-qu`).
fn has_flag(args: &[String], opts: &[&str]) -> bool {
    args.iter().take_while(|a| *a != "--").any(|a| {
        let name = a.split_once('=').map_or(a.as_str(), |(n, _)| n);
        opts.contains(&name)
            || (a.len() > 2
                && a.starts_with('-')
                && !a.starts_with("--")
                && opts
                    .iter()
                    .filter(|o| o.len() == 2 && !o.starts_with("--"))
                    .any(|o| a[1..].contains(&o[1..])))
    })
}

fn target_of(refspec: &str) -> Result<Push, String> {
    if refspec.starts_with('+') {
        return Err(FORCE.into());
    }
    if refspec.contains(HIDDEN) {
        return Err(UNCLEAR.into());
    }
    let (dst, local) = match refspec.split_once(':') {
        Some(("", _)) => return Err(DELETE.into()),
        Some((src, dst)) => {
            if src != "HEAD" && !src.is_empty() && ship::branch_ok(src).is_err() {
                return Err(UNCLEAR.into());
            }
            (dst, None)
        }
        None if refspec == "HEAD" => {
            return Ok(Push {
                target: Target::Current,
                local: None,
                upstream_moved: false,
            })
        }
        None => (refspec, Some(refspec)),
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
    Ok(Push {
        target: Target::Branch(name.to_string()),
        // The local branch a name on its own is: `heads/x` is `x` too.
        local: local.map(|_| name.to_string()),
        upstream_moved: false,
    })
}

/// One `git push`'s words after `push`.
fn read_push(args: &[String]) -> Result<Push, String> {
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
    read_with(command, &git_runs_own)
}

/// [`read`], with `own` saying which words after `git` the installed git
/// runs as its own command.
fn read_with(command: &str, own: Own) -> Read {
    match find(command, own) {
        Ok(pushes) if pushes.is_empty() => Read::NoPush,
        Ok(pushes) => Read::Pushes(pushes.into_iter().map(|p| p.target).collect()),
        Err(why) => Read::Refused(why),
    }
}

/// The pushes in `command`, as [`read_with`] reads them.
fn find(command: &str, own: Own) -> Result<Vec<Push>, String> {
    let shell = read_shell(command);
    let sets = &shell.sets;
    // A part the reader can't follow, or a variable set for what comes
    // after, may change what a push does.
    let mut unsure = shell.hidden.is_some() || !sets.iter().all(|a| plain_assignment(a));
    // A push after `cd` runs in another folder than the one the guard reads.
    let mut moved = false;
    // `git push origin HEAD` is read as the branch checked out now, so
    // nothing before it in the command may switch the branch: a script or
    // another program may, as may git's own `checkout`, `switch` and kin.
    let mut switched = false;
    // A branch named on its own lands where its symbolic ref points, and
    // with `push.default=upstream` where its upstream does: a command
    // before the push may change either.
    let mut refs_moved = false;
    let mut upstream_moved = false;
    let mut pushes = Vec::new();
    for run in &shell.runs {
        let Some(call) = git_call(run) else {
            let head = run.words.first().map(String::as_str).unwrap_or_default();
            let leaf = head.rsplit(['/', '\\']).next().unwrap_or(head);
            if MOVES.contains(&leaf.to_ascii_lowercase().as_str()) {
                moved = true;
            } else {
                switched = true;
            }
            continue;
        };
        let Some(verb) = call.verb else {
            unsure |= git_may_push(run, sets);
            continue;
        };
        if verb == "push" {
            let plain = !call.dashed
                && !call.odd
                && !run.nested
                && !run.more_args
                && call
                    .options
                    .iter()
                    .all(|(name, _)| PLAIN_GIT_OPTIONS.contains(name))
                && run.env.iter().all(|a| plain_assignment(a));
            if !plain {
                // `/usr/lib/git-core/git-push --force …` is still a force.
                return Err(match read_push(call.args) {
                    Err(why) if call.dashed => why,
                    _ => UNCLEAR.into(),
                });
            }
            let mut push = read_push(call.args)?;
            if push.target == Target::Current && switched {
                return Err(HEAD_MAY_MOVE.into());
            }
            if push.local.is_some() && refs_moved {
                return Err(UNCLEAR.into());
            }
            push.upstream_moved = upstream_moved;
            upstream_moved |= moves_upstream(verb, call.args);
            pushes.push(push);
            continue;
        }
        refs_moved |= moves_refs(verb, call.args);
        upstream_moved |= moves_upstream(verb, call.args);
        if OTHER_PUSHES.contains(&verb) {
            return Err(UNCLEAR.into());
        }
        switched |= SWITCHES_BRANCH.contains(&verb);
        if verb.contains(HIDDEN) {
            unsure = true;
            switched = true;
            continue;
        }
        // A `git-<name>` program run by its path is that program, not an
        // alias. After a `PATH` change git may find another program for a
        // word, so only one compiled into git is its own.
        let gits_own = call.dashed
            || if git_path_changed(run, sets) {
                git_compiled_in(verb)
            } else {
                own(verb)
            };
        if !gits_own {
            return Err(not_gits_own(verb));
        }
        if git_may_push(run, sets) {
            unsure = true;
            switched = true;
        }
    }
    if !pushes.is_empty() && (moved || unsure) {
        return Err(UNCLEAR.into());
    }
    Ok(pushes)
}

/// The values git has for a config key in `dir`, read by git itself with
/// no program the repository names run (hooks and fsmonitor off).
fn config(dir: &Path, args: &[&str]) -> Vec<String> {
    let mut c = grokhub_core::git_cmd::git();
    c.arg("config")
        .args(args)
        .current_dir(dir)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_TERMINAL_PROMPT", "0");
    hide_windows_console(&mut c);
    let Ok(out) = c.output() else {
        return Vec::new();
    };
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect()
}

/// `name` through a refspec's `src`, as git matches it: the same name, or
/// one `*` that stands for any part. The destination it maps to.
fn through(src: &str, dst: &str, name: &str) -> Option<String> {
    match src.split_once('*') {
        Some((before, after)) => {
            let fits = name.len() >= before.len() + after.len()
                && name.starts_with(before)
                && name.ends_with(after);
            fits.then(|| dst.replacen('*', &name[before.len()..name.len() - after.len()], 1))
        }
        None => (src == name).then(|| dst.to_string()),
    }
}

/// The branch on origin that `git push origin <local>` updates in `dir`, as
/// git's `push` maps a name given on its own: the first
/// `remote.origin.push` refspec that matches it (unless a `^` one does),
/// else with `push.default` `upstream` the one branch it tracks, else the
/// branch its symbolic ref points to, else the same name. With
/// `upstream_moved`, a command before the push may set the upstream, so a
/// push that follows it can't be read.
fn lands_on(dir: &Path, local: &str, upstream_moved: bool) -> Result<String, String> {
    let branch_of = |dst: &str| match target_of(&format!("HEAD:{dst}")) {
        Ok(Push {
            target: Target::Branch(b),
            ..
        }) => Ok(b),
        Ok(_) => Err(UNCLEAR.to_string()),
        Err(why) => Err(why),
    };
    let full = format!("refs/heads/{local}");
    // git matches the full name; a short one is read too, to be safe.
    let names = [full.as_str(), local];
    let specs = config(dir, &["--get-all", "remote.origin.push"]);
    let omitted = specs
        .iter()
        .filter_map(|spec| spec.strip_prefix('^'))
        .any(|src| names.iter().any(|n| through(src, "", n).is_some()));
    if !omitted {
        for spec in specs.iter().filter(|spec| !spec.starts_with('^')) {
            let (force, spec) = match spec.strip_prefix('+') {
                Some(rest) => (true, rest),
                None => (false, spec.as_str()),
            };
            let (src, dst) = spec.split_once(':').unwrap_or((spec, spec));
            let Some(dst) = names.iter().find_map(|n| through(src, dst, n)) else {
                continue;
            };
            if force {
                return Err(FORCE.into());
            }
            return branch_of(&dst);
        }
    }
    let default = config(dir, &["--get", "push.default"]);
    if default
        .first()
        .is_some_and(|d| matches!(d.to_ascii_lowercase().as_str(), "upstream" | "tracking"))
    {
        if upstream_moved {
            return Err(UNCLEAR.into());
        }
        if let [merge] = config(dir, &["--get-all", &format!("branch.{local}.merge")]).as_slice() {
            return branch_of(merge);
        }
    }
    // git sends a symbolic ref's push where it points: `code/fix` that
    // points at `main` updates main.
    match symref_target(dir, &full) {
        Some(target) => match target.strip_prefix("refs/heads/") {
            Some(_) => branch_of(&target),
            None => Err(UNCLEAR.into()),
        },
        None => Ok(local.to_string()),
    }
}

/// Where the symbolic ref `name` points in `dir`, through every link, as
/// git reads it; `None` when it isn't one.
fn symref_target(dir: &Path, name: &str) -> Option<String> {
    let mut c = grokhub_core::git_cmd::git();
    c.args(["symbolic-ref", "--quiet", name])
        .current_dir(dir)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_TERMINAL_PROMPT", "0");
    hide_windows_console(&mut c);
    let out = c.output().ok()?;
    let line = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (out.status.success() && !line.is_empty()).then_some(line)
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
    let pushes = match find(command, &git_runs_own) {
        Ok(pushes) if pushes.is_empty() => return None,
        Ok(pushes) => pushes,
        Err(why) => {
            blocked(config_dir, cfg, command, "", &why);
            return Some(Verdict::Refuse(why));
        }
    };
    let mut ask = false;
    let mut branches = Vec::new();
    for push in pushes {
        let branch = match (push.target, push.local) {
            (_, Some(local)) => match lands_on(dir, &local, push.upstream_moved) {
                Ok(b) => b,
                Err(why) => {
                    blocked(config_dir, cfg, command, &local, &why);
                    return Some(Verdict::Refuse(why));
                }
            },
            (Target::Branch(b), None) => b,
            (Target::Current, None) => match ship::current_branch(dir) {
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
