//! The Command page: one shell line on the bound project, the Tauri side of
//! the cabin's Command page. A line never runs here. It is checked, then
//! queued to the agent as a chat message, the way the cabin queues `/sh`, so
//! it runs through the agent's own approval gate like any step it takes.
//!
//! Before it goes, the line passes the cabin's host checks: the forbidden
//! list, the bound-project fence and, for a typed `git push`, the Code tab's
//! push guard. The first run of an exact line asks on the page and shows it;
//! after a yes it is remembered for this repo, and changed text asks again.
//! A destructive line (`rm`, `--force`, `sudo`) asks every time, as the
//! cabin's host confirm does.

use std::path::Path;

use grokhub_core::HostRisk;
use serde::{Deserialize, Serialize};

use crate::code_ship::{self, Blocked, CodeShip, Facts, Host, Pr};

/// The remembered lines, in the app config folder.
pub const FILE: &str = "command-box.json";
/// The most remembered lines kept per repo; the oldest go first.
pub const KEEP_PER_REPO: usize = 100;

pub const EMPTY: &str = "Type a command first.";
pub const ONE_LINE: &str = "One line at a time: the box runs a single command.";
pub const OUTSIDE: &str = "That reaches outside the project folder, so it doesn't run from here.";
pub const FORCE_PUSH: &str = "Force pushes never run from here.";

/// What the page does with a line.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(
    tag = "verdict",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum Verdict {
    /// Remembered for this repo: it goes to the agent now.
    Queue { command: String, prompt: String },
    /// The first run of this exact text, or a destructive line: show it and
    /// ask before it goes.
    Confirm { command: String, destructive: bool },
    /// It doesn't run, and `message` says why.
    Blocked { command: String, message: String },
}

/// What the push guard needs about the repo a typed `git push` runs in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Repo {
    pub ship: CodeShip,
    pub current_branch: Option<String>,
    pub default_branch: Option<String>,
    /// Where `origin` pushes; `None` with no origin.
    pub host: Option<Host>,
}

/// The chat message that asks the agent to run `command`. The fence is
/// longer than any run of backticks in the command, so it can't close early.
pub fn agent_prompt(command: &str) -> String {
    let mut longest = 0;
    let mut run = 0;
    for c in command.chars() {
        run = if c == '`' { run + 1 } else { 0 };
        longest = longest.max(run);
    }
    let fence = "`".repeat((longest + 1).max(3));
    format!(
        "Run this shell command in the project folder, exactly as written and once, \
         then show me what it printed and its exit code. Don't change it or run \
         anything else.\n\n{fence}sh\n{command}\n{fence}"
    )
}

/// Reads `line` against the host checks and the remembered lines.
/// `project_root` is the bound project; `home` expands `~` in paths.
pub fn judge(
    line: &str,
    project_root: &str,
    home: Option<&str>,
    remembered: &[String],
    repo: &Repo,
) -> Verdict {
    let command = line.trim().to_string();
    if let Some(message) = refusal(&command, project_root, home, repo) {
        return Verdict::Blocked { command, message };
    }
    let destructive = grokhub_core::host_risk(&command) == HostRisk::Destructive;
    if destructive || !remembered.iter().any(|r| r == &command) {
        return Verdict::Confirm {
            command,
            destructive,
        };
    }
    Verdict::Queue {
        prompt: agent_prompt(&command),
        command,
    }
}

/// Why `command` doesn't run at all, if it doesn't.
fn refusal(command: &str, project_root: &str, home: Option<&str>, repo: &Repo) -> Option<String> {
    if command.is_empty() {
        return Some(EMPTY.into());
    }
    if command.contains(['\n', '\r']) {
        return Some(ONE_LINE.into());
    }
    if let Some(why) = grokhub_core::forbidden_reason(command) {
        return Some(format!("That doesn't run: {why}."));
    }
    if grokhub_core::project::host_cmd_leaves_project_in(command, project_root, home) {
        return Some(OUTSIDE.into());
    }
    push_refusal(command, repo)
}

/// One `git push` in a line: its flags and its positional words.
#[derive(Debug, Default, PartialEq, Eq)]
struct Push {
    force: bool,
    words: Vec<String>,
}

/// Every `git push` in `command`, across `&&`, `||`, `;` and `|`.
fn pushes(command: &str) -> Vec<Push> {
    let mut out = Vec::new();
    for seg in command.split(['&', '|', ';']) {
        let words: Vec<&str> = seg.split_whitespace().collect();
        let Some(git) = words
            .iter()
            .position(|w| *w == "git" || w.ends_with("/git") || w.ends_with("\\git.exe"))
        else {
            continue;
        };
        // `git -C dir -c k=v push`: skip git's own options to the subcommand.
        let mut i = git + 1;
        while let Some(w) = words.get(i) {
            if *w == "-C" || *w == "-c" {
                i += 2;
            } else if w.starts_with('-') {
                i += 1;
            } else {
                break;
            }
        }
        if words.get(i) != Some(&"push") {
            continue;
        }
        let mut push = Push::default();
        for w in &words[i + 1..] {
            let w = w.trim_matches(['"', '\'']);
            if let Some(long) = w.strip_prefix("--") {
                if long.starts_with("force") || long == "mirror" {
                    push.force = true;
                }
            } else if let Some(short) = w.strip_prefix('-') {
                if short.contains('f') {
                    push.force = true;
                }
            } else {
                if w.starts_with('+') {
                    push.force = true;
                }
                push.words.push(w.to_string());
            }
        }
        out.push(push);
    }
    out
}

/// The branches a push's refspecs land on: `src:dst` lands on `dst`,
/// `HEAD` and a bare `git push` on the current branch.
fn targets(words: &[String], current: Option<&str>) -> Vec<String> {
    let specs = words.get(1..).unwrap_or(&[]);
    if specs.is_empty() {
        return current.map(str::to_string).into_iter().collect();
    }
    specs
        .iter()
        .filter_map(|spec| {
            let spec = spec.trim_start_matches('+');
            let dst = spec.rsplit_once(':').map_or(spec, |(_, d)| d);
            let dst = dst.strip_prefix("refs/heads/").unwrap_or(dst);
            if dst == "HEAD" || dst.is_empty() {
                current.map(str::to_string)
            } else {
                Some(dst.to_string())
            }
        })
        .collect()
}

/// The Code tab's push guard for a typed `git push`.
fn push_refusal(command: &str, repo: &Repo) -> Option<String> {
    for push in pushes(command) {
        if push.force {
            return Some(FORCE_PUSH.into());
        }
        let Some(host) = repo.host else {
            return Some(Blocked::NoRemote.message(""));
        };
        let branches = targets(&push.words, repo.current_branch.as_deref());
        if branches.is_empty() {
            return Some(Blocked::Detached.message(""));
        }
        for branch in branches {
            let facts = Facts {
                branch: branch.clone(),
                default_branch: repo.default_branch.clone(),
                app_made: false,
                host,
                pr: Pr::None,
            };
            if let Err(why) = code_ship::guard(&repo.ship, &facts) {
                return Some(why.message(&branch));
            }
        }
    }
    None
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Store {
    #[serde(default)]
    repos: Vec<Entry>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Entry {
    repo_key: String,
    #[serde(default)]
    commands: Vec<String>,
}

fn load(config_dir: &Path) -> Store {
    std::fs::read_to_string(config_dir.join(FILE))
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

fn save(config_dir: &Path, store: &Store) -> Result<(), String> {
    std::fs::create_dir_all(config_dir).map_err(|e| e.to_string())?;
    let raw = serde_json::to_string_pretty(store).map_err(|e| e.to_string())?;
    let tmp = config_dir.join(format!("{FILE}.tmp"));
    std::fs::write(&tmp, raw).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, config_dir.join(FILE)).map_err(|e| e.to_string())
}

/// The lines remembered for `repo_key`, oldest first.
pub fn remembered(config_dir: &Path, repo_key: &str) -> Vec<String> {
    load(config_dir)
        .repos
        .into_iter()
        .find(|e| e.repo_key == repo_key)
        .map(|e| e.commands)
        .unwrap_or_default()
}

/// Remembers `command` for `repo_key` after a yes on the page. A
/// destructive line is never remembered: it asks every time.
pub fn remember(config_dir: &Path, repo_key: &str, command: &str) -> Result<(), String> {
    let command = command.trim();
    if command.is_empty() || grokhub_core::host_risk(command) == HostRisk::Destructive {
        return Ok(());
    }
    let mut store = load(config_dir);
    let at = match store.repos.iter().position(|e| e.repo_key == repo_key) {
        Some(at) => at,
        None => {
            store.repos.push(Entry {
                repo_key: repo_key.to_string(),
                commands: Vec::new(),
            });
            store.repos.len() - 1
        }
    };
    let commands = &mut store.repos[at].commands;
    commands.retain(|c| c != command);
    commands.push(command.to_string());
    let over = commands.len().saturating_sub(KEEP_PER_REPO);
    commands.drain(..over);
    save(config_dir, &store)
}

/// Forgets every line remembered for `repo_key`, so each asks again.
pub fn forget(config_dir: &Path, repo_key: &str) -> Result<(), String> {
    let mut store = load(config_dir);
    let before = store.repos.len();
    store.repos.retain(|e| e.repo_key != repo_key);
    if store.repos.len() == before {
        return Ok(());
    }
    save(config_dir, &store)
}

/// This project's id in the store, as the Code tab's runs use.
pub fn repo_key(workspace: &Path) -> String {
    crate::code_runs::repo_key(workspace, crate::code::origin_url(workspace).as_deref())
}

/// The push guard's facts for the repo in `workspace`.
pub fn repo_facts(workspace: &Path, ship: CodeShip) -> Repo {
    Repo {
        ship,
        current_branch: code_ship::current_branch(workspace),
        default_branch: code_ship::default_branch(workspace),
        host: code_ship::push_host(workspace),
    }
}

/// What the page shows: the project folder's name and the lines it remembers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct View {
    pub project: String,
    pub remembered: Vec<String>,
}

pub fn view(config_dir: &Path, workspace: &Path) -> View {
    View {
        project: workspace
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| workspace.to_string_lossy().into_owned()),
        remembered: remembered(config_dir, &repo_key(workspace)),
    }
}

/// Checks `line` for the page.
pub fn check(config_dir: &Path, workspace: &Path, line: &str, ship: CodeShip) -> Verdict {
    let home = std::env::var("HOME")
        .ok()
        .or_else(|| std::env::var("USERPROFILE").ok());
    let root = workspace.to_string_lossy().replace('\\', "/");
    judge(
        line,
        &root,
        home.as_deref(),
        &remembered(config_dir, &repo_key(workspace)),
        &repo_facts(workspace, ship),
    )
}

/// The page's yes: the checks run again (a stale page can't skip them), the
/// line is remembered, and the agent's message comes back to queue.
pub fn approve(
    config_dir: &Path,
    workspace: &Path,
    line: &str,
    ship: CodeShip,
) -> Result<String, String> {
    let command = line.trim();
    match check(config_dir, workspace, command, ship) {
        Verdict::Blocked { message, .. } => Err(message),
        Verdict::Queue { prompt, .. } => Ok(prompt),
        Verdict::Confirm { .. } => {
            remember(config_dir, &repo_key(workspace), command)?;
            Ok(agent_prompt(command))
        }
    }
}

#[cfg(test)]
#[path = "command_box_tests.rs"]
mod tests;
