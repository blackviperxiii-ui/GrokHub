//! Code tab slice 5a: push the current branch and open a PR into the auto
//! base, behind a guard. Two modes, both settings: Ask (the default) confirms
//! every push and PR; Auto-beta skips the confirm only for a branch this app
//! made, on GitHub, with no PR yet or a PR into the auto base.
//!
//! Fixed rules in every mode, not settings: never push `main` or the repo's
//! detected default branch, never force-push. The app builds every git and
//! gh argv itself (no shell, no free text), never emits a force, mirror,
//! delete or prune flag, and never deletes a branch. A rejected push is
//! reported, never retried. Hooks and fsmonitor are off for every git call.
//!
//! The agent window's Tasks panel pushes through [`plan_at`] and [`ship_at`]:
//! always Ask, whatever the setting, and the push names the one commit made
//! there, never `HEAD`.

use crate::activity::{self as code_activity, Entry, Kind as ActKind};
use crate::runs::repo_key;
use grokhub_agent::ship_tree;
use grokhub_core::proc_util::hide_windows_console;
use grokhub_core::redact::redact_secrets;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// App-made branches, in the config folder.
pub const BRANCHES_FILE: &str = "code-branches.json";
/// Always protected, in every mode, whatever the settings say.
pub const ALWAYS_PROTECTED: &str = "main";
/// The most push or PR output kept for the window.
pub const OUTPUT_CAP: usize = 64 * 1024;

/// Settings → Code → Pushes and PRs.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ShipMode {
    /// Skip the confirm for an app-made branch into the auto base on GitHub.
    AutoBeta,
    /// Confirm every push and PR. A value this build doesn't know reads as Ask.
    #[default]
    #[serde(other)]
    Ask,
}

impl ShipMode {
    pub fn id(self) -> &'static str {
        match self {
            Self::Ask => "ask",
            Self::AutoBeta => "autoBeta",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Hosts {
    pub github: bool,
    /// Any other host. Off by default; when on, it still always asks.
    pub other: bool,
}

impl Default for Hosts {
    fn default() -> Self {
        Self {
            github: true,
            other: false,
        }
    }
}

/// `shell.json` → `codeShip`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct CodeShip {
    pub mode: ShipMode,
    /// The only PR base Auto-beta works with, and the base of "Open PR into".
    pub auto_base: String,
    pub hosts: Hosts,
    /// Protected on top of `main` and the detected default branch.
    pub extra_protected: Vec<String>,
    /// Activity log lines kept.
    pub activity_keep: usize,
}

impl Default for CodeShip {
    fn default() -> Self {
        Self {
            mode: ShipMode::Ask,
            auto_base: "beta".into(),
            hosts: Hosts::default(),
            extra_protected: vec![ALWAYS_PROTECTED.into()],
            activity_keep: code_activity::KEEP_DEFAULT,
        }
    }
}

/// A block this build can't read loads as the defaults (Ask), never an error.
pub fn lenient<'de, D: Deserializer<'de>>(d: D) -> Result<CodeShip, D::Error> {
    let raw = Value::deserialize(d)?;
    Ok(serde_json::from_value(raw).unwrap_or_default())
}

/// The shell's `codeShip` block, read straight from `shell.json` in the
/// config folder, so the cabin follows the same push settings. A missing
/// file, block or unreadable value gives the defaults (Ask).
pub fn load_from_shell_json(config_dir: &Path) -> CodeShip {
    std::fs::read_to_string(config_dir.join("shell.json"))
        .ok()
        .and_then(|raw| serde_json::from_str::<Value>(&raw).ok())
        .and_then(|v| v.get("codeShip").cloned())
        .and_then(|block| serde_json::from_value(block).ok())
        .unwrap_or_default()
}

/// The block as typed in Settings, tidied: names trimmed, empty ones dropped.
pub fn checked(mut cfg: CodeShip) -> Result<CodeShip, String> {
    cfg.auto_base = cfg.auto_base.trim().to_string();
    branch_ok(&cfg.auto_base)
        .map_err(|_| "The PR base isn't a branch name git accepts.".to_string())?;
    if cfg.auto_base == ALWAYS_PROTECTED {
        return Err("The PR base can't be main: main is never pushed to.".into());
    }
    let mut extra = Vec::new();
    for name in cfg
        .extra_protected
        .iter()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
    {
        branch_ok(name).map_err(|_| format!("{name} isn't a branch name git accepts."))?;
        if !extra.iter().any(|e: &String| e == name) {
            extra.push(name.to_string());
        }
    }
    cfg.extra_protected = extra;
    cfg.activity_keep = cfg.activity_keep.clamp(50, 100_000);
    Ok(cfg)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Host {
    GitHub,
    Other,
}

impl Host {
    pub fn id(self) -> &'static str {
        match self {
            Self::GitHub => "github",
            Self::Other => "other",
        }
    }
}

/// Why a push or PR doesn't run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Blocked {
    DefaultBranch,
    Protected,
    BadBranchName,
    HostOff,
    NoRemote,
    Detached,
    PrNeedsGitHub,
    PrOpen,
}

impl Blocked {
    pub fn message(self, branch: &str) -> String {
        match self {
            Self::DefaultBranch => format!(
                "{branch} is the main or default branch. The app never pushes to it; make a branch first."
            ),
            Self::Protected => format!("{branch} is protected in Settings → Code, so it isn't pushed."),
            Self::BadBranchName => format!("\"{branch}\" isn't a branch name the app pushes."),
            Self::HostOff => "Pushing to this git host is off in Settings → Code.".into(),
            Self::NoRemote => "This repo has no origin remote to push to.".into(),
            Self::Detached => "Check out a branch first; a detached HEAD isn't pushed.".into(),
            Self::PrNeedsGitHub => "Opening a PR works on GitHub only.".into(),
            Self::PrOpen => format!("{branch} already has an open PR."),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Go {
    Confirm,
    NoConfirm,
}

/// What the guard knows about one push.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Facts {
    pub branch: String,
    /// The repo's default branch, when it could be read locally.
    pub default_branch: Option<String>,
    pub app_made: bool,
    pub host: Host,
    pub pr: Pr,
}

/// The branch's PR on the host, as far as we know.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pr {
    None,
    Open {
        base: String,
        url: String,
    },
    /// gh missing, offline, or its answer unreadable.
    Unknown,
}

/// A name git accepts and the app pushes: no leading `+` or `-`, no `:`,
/// `..`, `@{`, spaces, control characters or the other refname specials.
pub fn branch_ok(name: &str) -> Result<(), Blocked> {
    let bad = name.is_empty()
        || name.len() > 200
        || name.starts_with(['+', '-', '/', '.'])
        || name.ends_with(['/', '.'])
        || name.ends_with(".lock")
        || name.contains("..")
        || name.contains("@{")
        || name.contains("//")
        || name == "@"
        || name.contains("/.")
        || name
            .chars()
            .any(|c| c.is_control() || c.is_whitespace() || ":~^?*[\\".contains(c));
    if bad {
        Err(Blocked::BadBranchName)
    } else {
        Ok(())
    }
}

/// The host in a remote URL: `https://github.com/…`, `git@github.com:…`,
/// `ssh://git@github.com/…`. Anything else, a form this doesn't know
/// included, is [`Host::Other`].
pub fn host_of(url: &str) -> Host {
    let url = url.trim();
    let host = match url.split_once("://") {
        Some((scheme, rest)) => {
            let scheme = scheme.to_ascii_lowercase();
            // `git+ssh` and `ssh+git` are git's other names for ssh.
            let known = ["http", "https", "ssh", "git", "git+ssh", "ssh+git"];
            if !known.contains(&scheme.as_str()) {
                return Host::Other;
            }
            // The host is what follows the last `@` before the first `/`.
            // git, ssh and curl disagree on where that part ends when it has
            // a `?`, `#` or `\`, and git decodes `%2f` in an ssh or git URL
            // before it looks, so such a URL is never GitHub.
            let authority = rest.split('/').next().unwrap_or("");
            let web = scheme == "http" || scheme == "https";
            if authority.contains(['?', '#', '\\']) || (!web && authority.contains('%')) {
                return Host::Other;
            }
            let host = authority
                .rsplit_once('@')
                .map(|(_, h)| h)
                .unwrap_or(authority);
            host.split(':').next().unwrap_or("")
        }
        // `user@host:path`. With no `:` before the first `/` git reads a
        // local path, wherever it says github.com.
        None => match url.split_once(':') {
            Some((host, _)) if !host.contains('/') => {
                host.rsplit_once('@').map(|(_, h)| h).unwrap_or(host)
            }
            _ => return Host::Other,
        },
    }
    .to_ascii_lowercase();
    if host == "github.com" || host == "www.github.com" {
        Host::GitHub
    } else {
        Host::Other
    }
}

/// A branch the app never pushes or commits to: `main`, the repo's default
/// branch, and the ones protected in Settings.
pub(crate) fn protected(
    cfg: &CodeShip,
    branch: &str,
    default_branch: Option<&str>,
) -> Result<(), Blocked> {
    branch_ok(branch)?;
    if branch == ALWAYS_PROTECTED || default_branch == Some(branch) {
        return Err(Blocked::DefaultBranch);
    }
    if cfg.extra_protected.iter().any(|p| p == branch) {
        return Err(Blocked::Protected);
    }
    Ok(())
}

fn host_on(cfg: &CodeShip, host: Host) -> Result<(), Blocked> {
    let on = match host {
        Host::GitHub => cfg.hosts.github,
        Host::Other => cfg.hosts.other,
    };
    if on {
        Ok(())
    } else {
        Err(Blocked::HostOff)
    }
}

/// Auto-beta skips the confirm only when every one of these holds.
fn auto_ok(cfg: &CodeShip, f: &Facts) -> bool {
    cfg.mode == ShipMode::AutoBeta
        && f.app_made
        && f.host == Host::GitHub
        && f.default_branch.is_some()
        && branch_ok(&cfg.auto_base).is_ok()
        && cfg.auto_base != ALWAYS_PROTECTED
        && f.default_branch.as_deref() != Some(cfg.auto_base.as_str())
        && f.branch != cfg.auto_base
}

/// The push guard.
pub fn guard(cfg: &CodeShip, f: &Facts) -> Result<Go, Blocked> {
    protected(cfg, &f.branch, f.default_branch.as_deref())?;
    host_on(cfg, f.host)?;
    let pr_fits = match &f.pr {
        Pr::None => true,
        Pr::Open { base, .. } => base == &cfg.auto_base,
        Pr::Unknown => false,
    };
    Ok(if auto_ok(cfg, f) && pr_fits {
        Go::NoConfirm
    } else {
        Go::Confirm
    })
}

/// The PR guard: a PR from this branch into the auto base, GitHub only.
pub fn pr_guard(cfg: &CodeShip, f: &Facts) -> Result<Go, Blocked> {
    protected(cfg, &f.branch, f.default_branch.as_deref())?;
    host_on(cfg, f.host)?;
    if f.host != Host::GitHub {
        return Err(Blocked::PrNeedsGitHub);
    }
    if matches!(f.pr, Pr::Open { .. }) {
        return Err(Blocked::PrOpen);
    }
    if branch_ok(&cfg.auto_base).is_err() || cfg.auto_base == f.branch {
        return Err(Blocked::BadBranchName);
    }
    Ok(if auto_ok(cfg, f) && f.pr == Pr::None {
        Go::NoConfirm
    } else {
        Go::Confirm
    })
}

/// The push argv after `git`: the current commit to the same-named branch on
/// origin. Never a force, mirror, delete or prune flag.
pub fn push_args(branch: &str) -> Vec<String> {
    vec![
        "push".into(),
        "--porcelain".into(),
        "origin".into(),
        format!("HEAD:refs/heads/{branch}"),
    ]
}

/// The push argv after `git` from the agent window: the one commit `sha` to
/// the same-named branch on origin, with no submodule pushed and no signed
/// push. Never a force, mirror, delete or prune flag.
pub fn push_commit_args(sha: &str, branch: &str) -> Vec<String> {
    vec![
        "push".into(),
        "--porcelain".into(),
        "--no-recurse-submodules".into(),
        "--no-signed".into(),
        "origin".into(),
        format!("{sha}:refs/heads/{branch}"),
    ]
}

/// What every push from the agent window sets before its verb, whatever the
/// repository's config says: no tags along with the commit, no signed push,
/// and no hint about the grafts file the push is told not to read.
const PUSH_PINS: [&str; 6] = [
    "-c",
    "push.followTags=false",
    "-c",
    "push.gpgSign=false",
    "-c",
    "advice.graftFileDeprecated=false",
];

/// The null device, which git reads as "no hooks folder".
#[cfg(windows)]
const NULL_DEVICE: &str = "NUL";
#[cfg(not(windows))]
const NULL_DEVICE: &str = "/dev/null";

/// The git config the agent window's `gh` runs its own git calls with. It is
/// set in the environment, which git reads after the repository's file.
const GH_GIT_PINS: [(&str, &str); 4] = [
    ("core.hooksPath", NULL_DEVICE),
    ("core.fsmonitor", "false"),
    ("push.followTags", "false"),
    ("advice.graftFileDeprecated", "false"),
];

/// Sets [`GH_GIT_PINS`] on `cmd` through `GIT_CONFIG_COUNT`, and turns
/// replace refs and a grafts file off.
fn pin_git_env(cmd: &mut Command) {
    cmd.env("GIT_CONFIG_COUNT", GH_GIT_PINS.len().to_string());
    for (at, (key, value)) in GH_GIT_PINS.iter().enumerate() {
        cmd.env(format!("GIT_CONFIG_KEY_{at}"), key)
            .env(format!("GIT_CONFIG_VALUE_{at}"), value);
    }
    cmd.env("GIT_NO_REPLACE_OBJECTS", "1")
        .env("GIT_GRAFT_FILE", NULL_DEVICE);
}

/// A full commit id in lower-case hex, SHA-1 or SHA-256.
fn commit_id(sha: &str) -> bool {
    (sha.len() == 40 || sha.len() == 64)
        && sha
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// What [`plan_at`] and [`ship_at`] say for a `sha` that is no commit id.
pub const NOT_A_COMMIT: &str = "The commit to push is not a commit id.";

/// What a push or pull request from the agent window says when the
/// repository's own git config sets `key`.
pub fn untrusted_sentence(key: &str) -> String {
    let key: String = key.chars().filter(|c| !c.is_control()).take(120).collect();
    format!("This repository's own git config sets {key}; push from your terminal.")
}

/// Refuses when the repository's own git config names a program to run or
/// redirects a push; read just before the agent window spawns git or gh.
fn refuse_untrusted(dir: &Path) -> Result<(), String> {
    let (top, _) =
        ship_tree::head(dir).map_err(|err| format!("The repository could not be read: {err}."))?;
    match ship_tree::untrusted_config(&top) {
        Ok(None) => Ok(()),
        Ok(Some(key)) => Err(untrusted_sentence(&key)),
        Err(err) => Err(format!(
            "The repository's git config could not be read: {err}."
        )),
    }
}

/// What `refs/heads/<branch>` names on origin, read with the agent window's
/// pins: the commit a pull request for `branch` would carry. `None` when
/// origin has no such branch. It talks to origin, so Offline only refuses it.
pub(crate) fn origin_branch(dir: &Path, branch: &str) -> Result<Option<String>, String> {
    grokhub_core::offline::refuse()?;
    refuse_untrusted(dir)?;
    let refname = format!("refs/heads/{branch}");
    let (code, out) = git_with(
        dir,
        &PUSH_PINS,
        &["ls-remote", "--refs", "origin", &refname],
    )?;
    if code != 0 {
        return Err(format!(
            "origin could not be read (git exited with {code}); open the pull request from your \
             terminal."
        ));
    }
    Ok(out.lines().find_map(|line| {
        let (sha, name) = line.split_once('\t')?;
        (name == refname && commit_id(sha)).then(|| sha.to_string())
    }))
}

/// The gh argv for "Open PR into": base and head fixed, title and body from
/// the commits.
pub fn pr_args(base: &str, branch: &str) -> Vec<String> {
    ["pr", "create", "--base", base, "--head", branch, "--fill"]
        .iter()
        .map(|s| s.to_string())
        .collect()
}

/// The gh argv that reads the branch's PR.
pub fn pr_view_args(branch: &str) -> Vec<String> {
    [
        "pr",
        "view",
        "--json",
        "state,baseRefName,url",
        "--",
        branch,
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

/// The command as the confirm shows it and the activity log keeps it.
pub fn shown(program: &str, args: &[String]) -> String {
    std::iter::once(program.to_string())
        .chain(args.iter().cloned())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Redacts key-shaped text and the credentials part of any URL
/// (`https://user:token@host` → `https://[redacted]@host`).
pub fn clean_output(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find("://") {
        let (head, tail) = rest.split_at(at + 3);
        out.push_str(head);
        let end = tail
            .find(|c: char| c.is_whitespace() || c == '/')
            .unwrap_or(tail.len());
        match tail[..end].rfind('@') {
            Some(cred) => {
                out.push_str("[redacted]");
                rest = &tail[cred..];
            }
            None => rest = tail,
        }
    }
    out.push_str(rest);
    let mut out = redact_secrets(&out);
    if out.len() > OUTPUT_CAP {
        let mut cut = OUTPUT_CAP;
        while !out.is_char_boundary(cut) {
            cut -= 1;
        }
        out.truncate(cut);
    }
    out
}

/// A push git turned away because the remote moved on.
pub fn rejected(output: &str) -> bool {
    output.contains("[rejected]")
        || output.contains("non-fast-forward")
        || output.contains("fetch first")
}

// ---- the repo, read through git and gh ----

fn git(dir: &Path, args: &[&str]) -> Result<(i32, String), String> {
    git_with(dir, &[], args)
}

/// [`git`] with `pins` before the verb. With pins, replace refs and a
/// grafts file are off too: the agent window's push sends the history the
/// commits themselves record, which is the one its checks read.
fn git_with(dir: &Path, pins: &[&str], args: &[&str]) -> Result<(i32, String), String> {
    let mut c = grokhub_core::git_cmd::git();
    c.args(["-c", "core.quotepath=false"])
        .args(pins)
        .args(args)
        .current_dir(dir)
        .stdin(Stdio::null())
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_TERMINAL_PROMPT", "0");
    if !pins.is_empty() {
        c.env("GIT_NO_REPLACE_OBJECTS", "1")
            .env("GIT_GRAFT_FILE", NULL_DEVICE);
    }
    hide_windows_console(&mut c);
    let out = c
        .output()
        .map_err(|_| "git isn't installed or couldn't start".to_string())?;
    let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&out.stderr));
    Ok((out.status.code().unwrap_or(-1), text))
}

fn git_line(dir: &Path, args: &[&str]) -> Option<String> {
    let mut c = grokhub_core::git_cmd::git();
    c.args(args)
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

/// The checked-out branch, or `None` on a detached HEAD. Read as the full
/// ref: git's short name becomes `heads/<name>` when a tag has the same name.
pub fn current_branch(dir: &Path) -> Option<String> {
    git_line(dir, &["symbolic-ref", "--quiet", "HEAD"])?
        .strip_prefix("refs/heads/")
        .map(str::to_string)
}

/// The default branch from the local `origin/HEAD` (no network). `None`
/// when it was never set; Auto-beta then asks.
pub fn default_branch(dir: &Path) -> Option<String> {
    git_line(
        dir,
        &["symbolic-ref", "--quiet", "refs/remotes/origin/HEAD"],
    )?
    .strip_prefix("refs/remotes/origin/")
    .map(str::to_string)
}

/// True when the repo has a pre-push hook that git would run. The app's own
/// push runs no hooks, so the confirm can say this one is passed over.
fn has_pre_push_hook(dir: &Path) -> bool {
    // Not `git_cmd::git()`: that points the hooks folder at the null device,
    // and this asks where the repo's own hooks folder is. Nothing is run.
    let mut c = Command::new("git");
    c.args(["rev-parse", "--git-path", "hooks/pre-push"])
        .current_dir(dir)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_TERMINAL_PROMPT", "0");
    hide_windows_console(&mut c);
    let Ok(out) = c.output() else {
        return false;
    };
    let line = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if !out.status.success() || line.is_empty() {
        return false;
    }
    let Ok(meta) = std::fs::metadata(dir.join(line)) else {
        return false;
    };
    #[cfg(unix)]
    let runs = std::os::unix::fs::PermissionsExt::mode(&meta.permissions()) & 0o111 != 0;
    #[cfg(not(unix))]
    let runs = true;
    meta.is_file() && runs
}

/// The host a push to origin goes to, `None` with no origin. `git push
/// origin` sends to every push URL, so it is GitHub only when all are.
pub fn push_host(dir: &Path) -> Option<Host> {
    let urls = git_line(dir, &["remote", "get-url", "--push", "--all", "origin"])?;
    Some(if urls.lines().all(|url| host_of(url) == Host::GitHub) {
        Host::GitHub
    } else {
        Host::Other
    })
}

fn head_sha(dir: &Path) -> Option<String> {
    git_line(dir, &["rev-parse", "HEAD"])
}

/// `gh` with `args` in `dir`, never prompting. `pinned`: from the agent
/// window, with [`GH_GIT_PINS`] for the git calls gh makes itself.
fn gh(dir: &Path, args: &[String], pinned: bool) -> Command {
    let mut c = Command::new("gh");
    c.args(args)
        .current_dir(dir)
        .stdin(Stdio::null())
        .env("GH_PROMPT_DISABLED", "1")
        .env("GIT_TERMINAL_PROMPT", "0");
    if pinned {
        pin_git_env(&mut c);
    }
    hide_windows_console(&mut c);
    c
}

/// The branch's open PR, through the user's own gh login. Read only.
fn pr_of(dir: &Path, branch: &str, pinned: bool) -> Pr {
    if grokhub_core::offline::refuse().is_err() {
        return Pr::Unknown;
    }
    let Ok(out) = gh(dir, &pr_view_args(branch), pinned).output() else {
        return Pr::Unknown;
    };
    parse_pr_view(
        out.status.success(),
        &String::from_utf8_lossy(&out.stdout),
        &String::from_utf8_lossy(&out.stderr),
    )
}

/// `gh pr view --json state,baseRefName,url`: an open PR, none, or unknown.
pub fn parse_pr_view(ok: bool, stdout: &str, stderr: &str) -> Pr {
    if !ok {
        return if stderr.contains("no pull requests found") {
            Pr::None
        } else {
            Pr::Unknown
        };
    }
    let Ok(v) = serde_json::from_str::<Value>(stdout) else {
        return Pr::Unknown;
    };
    let text = |k: &str| v.get(k).and_then(Value::as_str).unwrap_or("").to_string();
    if text("state") != "OPEN" {
        return Pr::None;
    }
    Pr::Open {
        base: text("baseRefName"),
        url: text("url"),
    }
}

// ---- app-made branches ----

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Made {
    repo_key: String,
    branch: String,
    base_sha: String,
    created_ms: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct MadeStore {
    entries: Vec<Made>,
}

fn made_path(config_dir: &Path) -> PathBuf {
    config_dir.join(BRANCHES_FILE)
}

fn load_made(config_dir: &Path) -> MadeStore {
    std::fs::read_to_string(made_path(config_dir))
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

fn save_made(config_dir: &Path, store: &MadeStore) -> Result<(), String> {
    std::fs::create_dir_all(config_dir).map_err(|e| e.to_string())?;
    let raw = serde_json::to_string_pretty(store).map_err(|e| e.to_string())?;
    let tmp = config_dir.join(format!("{BRANCHES_FILE}.tmp"));
    std::fs::write(&tmp, raw).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, made_path(config_dir)).map_err(|e| e.to_string())
}

/// This branch was made by the Code tab in this repo. A branch made by hand never counts.
pub fn app_made(config_dir: &Path, key: &str, branch: &str) -> bool {
    load_made(config_dir)
        .entries
        .iter()
        .any(|m| m.repo_key == key && m.branch == branch)
}

fn key_of(dir: &Path) -> String {
    repo_key(dir, crate::repo::origin_url(dir).as_deref())
}

// ---- what the Code tab calls ----

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Action {
    Push,
    Pr,
}

/// What the Code tab shows before a push or PR.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Plan {
    pub action: Action,
    pub branch: Option<String>,
    /// The exact command, as the confirm shows it.
    pub command: String,
    pub needs_confirm: bool,
    /// Why it won't run, when it won't.
    pub blocked: Option<String>,
    pub mode: ShipMode,
    pub auto_base: String,
    pub app_made: bool,
    pub pr_url: Option<String>,
    /// A push only: the repo has a pre-push hook, and the app's push does
    /// not run it.
    pub skips_hook: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Shipped {
    pub command: String,
    pub exit: i32,
    pub output: String,
    pub summary: String,
}

struct Ctx {
    key: String,
    facts: Facts,
}

/// Test seam: a local bare repo as origin stands in for this host and PR.
pub(crate) type Probe = Option<(Host, Pr)>;

/// The facts for the checked-out branch. `pinned`: for the agent window.
fn context(dir: &Path, config_dir: &Path, probe: &Probe, pinned: bool) -> Result<Ctx, Blocked> {
    let branch = current_branch(dir).ok_or(Blocked::Detached)?;
    branch_ok(&branch)?;
    let host = push_host(dir).ok_or(Blocked::NoRemote)?;
    let (host, pr) = match probe {
        Some((host, pr)) => (*host, pr.clone()),
        None => {
            let pr = if host == Host::GitHub {
                pr_of(dir, &branch, pinned)
            } else {
                Pr::None
            };
            (host, pr)
        }
    };
    let key = key_of(dir);
    Ok(Ctx {
        facts: Facts {
            app_made: app_made(config_dir, &key, &branch),
            default_branch: default_branch(dir),
            branch,
            host,
            pr,
        },
        key,
    })
}

/// The facts and repo key for pushing `branch` from `dir`, checked out or
/// not. The agent's own pushes from chat (`chat_push`) use this; `probe`
/// stands in for the host and PR in tests.
pub(crate) fn facts_for(
    config_dir: &Path,
    dir: &Path,
    branch: &str,
    probe: &Probe,
) -> Result<(String, Facts), Blocked> {
    branch_ok(branch)?;
    let host = push_host(dir).ok_or(Blocked::NoRemote)?;
    let (host, pr) = match probe {
        Some((host, pr)) => (*host, pr.clone()),
        None if host == Host::GitHub => (host, pr_of(dir, branch, false)),
        None => (host, Pr::None),
    };
    let key = key_of(dir);
    let facts = Facts {
        app_made: app_made(config_dir, &key, branch),
        default_branch: default_branch(dir),
        branch: branch.to_string(),
        host,
        pr,
    };
    Ok((key, facts))
}

/// The command for `action`. With `sha`, a push sends that one commit.
fn command_for(
    action: Action,
    cfg: &CodeShip,
    branch: &str,
    sha: Option<&str>,
) -> (&'static str, Vec<String>) {
    match (action, sha) {
        (Action::Push, Some(sha)) => ("git", push_commit_args(sha, branch)),
        (Action::Push, None) => ("git", push_args(branch)),
        (Action::Pr, _) => ("gh", pr_args(&cfg.auto_base, branch)),
    }
}

/// The settings with every push and PR confirmed: the agent window's.
fn asking(cfg: &CodeShip) -> CodeShip {
    CodeShip {
        mode: ShipMode::Ask,
        ..cfg.clone()
    }
}

pub fn plan(config_dir: &Path, dir: &Path, cfg: &CodeShip, action: Action) -> Plan {
    plan_with(config_dir, dir, cfg, action, None)
}

/// What the agent window's push of the commit `sha`, or its PR, would run.
/// It always asks: the settings are read as Ask, whatever they say.
pub fn plan_at(config_dir: &Path, dir: &Path, cfg: &CodeShip, action: Action, sha: &str) -> Plan {
    plan_at_with(config_dir, dir, cfg, action, sha, None)
}

pub(crate) fn plan_at_with(
    config_dir: &Path,
    dir: &Path,
    cfg: &CodeShip,
    action: Action,
    sha: &str,
    probe: Probe,
) -> Plan {
    plan_on(config_dir, dir, &asking(cfg), action, Some(sha), probe)
}

fn plan_with(config_dir: &Path, dir: &Path, cfg: &CodeShip, action: Action, probe: Probe) -> Plan {
    plan_on(config_dir, dir, cfg, action, None, probe)
}

fn plan_on(
    config_dir: &Path,
    dir: &Path,
    cfg: &CodeShip,
    action: Action,
    sha: Option<&str>,
    probe: Probe,
) -> Plan {
    let mut out = Plan {
        action,
        branch: current_branch(dir),
        command: String::new(),
        needs_confirm: true,
        blocked: None,
        mode: cfg.mode,
        auto_base: cfg.auto_base.clone(),
        app_made: false,
        pr_url: None,
        skips_hook: action == Action::Push && has_pre_push_hook(dir),
    };
    if sha.is_some_and(|sha| !commit_id(sha)) {
        out.blocked = Some(NOT_A_COMMIT.into());
        return out;
    }
    let ctx = match context(dir, config_dir, &probe, sha.is_some()) {
        Ok(ctx) => ctx,
        Err(b) => {
            out.blocked = Some(b.message(out.branch.as_deref().unwrap_or("")));
            return out;
        }
    };
    let f = &ctx.facts;
    let (program, args) = command_for(action, cfg, &f.branch, sha);
    out.command = shown(program, &args);
    out.app_made = f.app_made;
    if let Pr::Open { url, .. } = &f.pr {
        out.pr_url = Some(url.clone());
    }
    let verdict = match action {
        Action::Push => guard(cfg, f),
        Action::Pr => pr_guard(cfg, f),
    };
    match verdict {
        Ok(go) => out.needs_confirm = go == Go::Confirm,
        Err(b) => out.blocked = Some(b.message(&f.branch)),
    }
    out
}

fn log(
    config_dir: &Path,
    cfg: &CodeShip,
    ctx: &Ctx,
    kind: ActKind,
    command: &str,
    approved_by: &str,
    exit: Option<i32>,
    summary: &str,
) {
    let _ = code_activity::append(
        config_dir,
        &Entry {
            ts_ms: grokhub_core::now_ms(),
            kind,
            repo_key: ctx.key.clone(),
            host: ctx.facts.host.id().into(),
            branch: ctx.facts.branch.clone(),
            command: command.into(),
            mode: cfg.mode.id().into(),
            approved_by: approved_by.into(),
            exit,
            summary: summary.into(),
        },
        cfg.activity_keep,
    );
}

/// Push or open the PR. The guard runs again here, so a stale window can't
/// skip it. Unless the guard said no confirm, `confirmed` must be true and
/// `expected` must be the command the person saw, so a yes given for one
/// branch never runs for another.
pub fn ship(
    config_dir: &Path,
    dir: &Path,
    cfg: &CodeShip,
    action: Action,
    confirmed: bool,
    expected: Option<&str>,
) -> Result<Shipped, String> {
    ship_with(config_dir, dir, cfg, action, confirmed, expected, None)
}

/// The agent window's push of the commit `sha`, or its PR. Always Ask:
/// `confirmed` must be true and `expected` the command the confirm showed.
/// The push names `sha`, never `HEAD`, with [`PUSH_PINS`]; gh runs its git
/// calls with [`GH_GIT_PINS`]. Right before either spawns, the repository's
/// own config is read again for a program it would run.
pub fn ship_at(
    config_dir: &Path,
    dir: &Path,
    cfg: &CodeShip,
    action: Action,
    sha: &str,
    confirmed: bool,
    expected: Option<&str>,
) -> Result<Shipped, String> {
    ship_at_with(config_dir, dir, cfg, action, sha, confirmed, expected, None)
}

pub(crate) fn ship_at_with(
    config_dir: &Path,
    dir: &Path,
    cfg: &CodeShip,
    action: Action,
    sha: &str,
    confirmed: bool,
    expected: Option<&str>,
    probe: Probe,
) -> Result<Shipped, String> {
    if !commit_id(sha) {
        return Err(NOT_A_COMMIT.into());
    }
    let cfg = asking(cfg);
    ship_on(
        config_dir,
        dir,
        &cfg,
        action,
        Some(sha),
        confirmed,
        expected,
        probe,
    )
}

fn ship_with(
    config_dir: &Path,
    dir: &Path,
    cfg: &CodeShip,
    action: Action,
    confirmed: bool,
    expected: Option<&str>,
    probe: Probe,
) -> Result<Shipped, String> {
    ship_on(
        config_dir, dir, cfg, action, None, confirmed, expected, probe,
    )
}

fn ship_on(
    config_dir: &Path,
    dir: &Path,
    cfg: &CodeShip,
    action: Action,
    sha: Option<&str>,
    confirmed: bool,
    expected: Option<&str>,
    probe: Probe,
) -> Result<Shipped, String> {
    let pinned = sha.is_some();
    let ctx = context(dir, config_dir, &probe, pinned)
        .map_err(|b| b.message(&current_branch(dir).unwrap_or_default()))?;
    let f = &ctx.facts;
    let (program, args) = command_for(action, cfg, &f.branch, sha);
    let command = shown(program, &args);
    let verdict = match action {
        Action::Push => guard(cfg, f),
        Action::Pr => pr_guard(cfg, f),
    };
    let go = match verdict {
        Ok(go) => go,
        Err(b) => {
            let msg = b.message(&f.branch);
            log(
                config_dir,
                cfg,
                &ctx,
                ActKind::Blocked,
                &command,
                "none",
                None,
                &msg,
            );
            return Err(msg);
        }
    };
    // A confirm names one command. It is refused for any other, also one
    // that would have needed no confirm.
    let stale = match expected {
        Some(seen) => seen != command.as_str(),
        None => go == Go::Confirm && confirmed,
    };
    if go == Go::Confirm && !confirmed {
        return Err("This needs a confirmation first.".into());
    }
    if stale {
        return Err(
            "The branch changed since you confirmed. Check the command and confirm again.".into(),
        );
    }
    grokhub_core::offline::refuse()?;
    let approved_by = if go == Go::NoConfirm {
        "autoBeta"
    } else {
        "user"
    };
    if pinned {
        refuse_untrusted(dir)?;
    }
    let (exit, raw) = match action {
        Action::Push => {
            let argv: Vec<&str> = args.iter().map(String::as_str).collect();
            let pins: &[&str] = if pinned { &PUSH_PINS } else { &[] };
            git_with(dir, pins, &argv)?
        }
        Action::Pr => {
            let out = gh(dir, &args, pinned)
                .output()
                .map_err(|_| "gh isn't installed or couldn't start. Install the GitHub CLI and sign in with gh auth login.".to_string())?;
            let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
            text.push_str(&String::from_utf8_lossy(&out.stderr));
            (out.status.code().unwrap_or(-1), text)
        }
    };
    let output = clean_output(&raw);
    let summary = match (action, exit) {
        (Action::Push, 0) => match sha {
            Some(sha) => format!("Pushed {} to {} on origin.", &sha[..8], f.branch),
            None => format!("Pushed {} to origin.", f.branch),
        },
        (Action::Push, _) if rejected(&output) => {
            "Rejected: origin has commits this branch doesn't. Pull them first; the app never force-pushes.".into()
        }
        (Action::Push, n) => format!("The push failed (git exited with {n})."),
        (Action::Pr, 0) => format!("Opened a PR from {} into {}.", f.branch, cfg.auto_base),
        (Action::Pr, n) => format!("The PR didn't open (gh exited with {n})."),
    };
    let kind = match action {
        Action::Push => ActKind::Push,
        Action::Pr => ActKind::Pr,
    };
    log(
        config_dir,
        cfg,
        &ctx,
        kind,
        &command,
        approved_by,
        Some(exit),
        &summary,
    );
    Ok(Shipped {
        command,
        exit,
        output,
        summary,
    })
}

/// Make and check out a branch from HEAD, and remember the app made it.
/// Refuses protected names and the auto base. Never deletes or resets.
pub fn new_branch(
    config_dir: &Path,
    dir: &Path,
    cfg: &CodeShip,
    name: &str,
) -> Result<String, String> {
    let name = name.trim();
    branch_ok(name).map_err(|b| b.message(name))?;
    let facts = Facts {
        branch: name.to_string(),
        default_branch: default_branch(dir),
        app_made: true,
        host: Host::GitHub,
        pr: Pr::None,
    };
    protected(cfg, &facts.branch, facts.default_branch.as_deref()).map_err(|b| b.message(name))?;
    if name == cfg.auto_base {
        return Err(format!(
            "{name} is the PR base. Pick another name for your work."
        ));
    }
    let base_sha = head_sha(dir).ok_or("This repo has no commit to branch from yet.")?;
    let (code, out) = git(dir, &["switch", "-c", name])?;
    if code != 0 {
        return Err(clean_output(out.trim()));
    }
    let key = key_of(dir);
    let mut store = load_made(config_dir);
    store
        .entries
        .retain(|m| !(m.repo_key == key && m.branch == name));
    store.entries.push(Made {
        repo_key: key.clone(),
        branch: name.to_string(),
        base_sha,
        created_ms: grokhub_core::now_ms(),
    });
    save_made(config_dir, &store)?;
    let ctx = Ctx { key, facts };
    let ctx = Ctx {
        facts: Facts {
            host: push_host(dir).unwrap_or(Host::Other),
            ..ctx.facts
        },
        ..ctx
    };
    log(
        config_dir,
        cfg,
        &ctx,
        ActKind::Branch,
        &format!("git switch -c {name}"),
        "user",
        Some(0),
        &format!("Made branch {name}."),
    );
    Ok(name.to_string())
}

#[cfg(test)]
#[path = "ship_tests.rs"]
mod tests;
