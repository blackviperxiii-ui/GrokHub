//! HX-11: model-written shell commands run in a sandbox by default.
//!
//! Writes are allowed only in the workspace, temp folders, `/dev` and the
//! usual tool caches (Landlock on Linux, a write-restricted token on Windows). Network goes through the egress
//! proxy, so the egress guard (offline mode, the egress log) sees every host.
//! The gate stays on top and decides first.
//!
//! A command runs outside the sandbox only when it really needs the whole
//! machine: it raises privileges (`sudo`, `pkexec`, `doas`, `su`, `run0`,
//! which no-new-privileges would break; the OS password dialog still asks),
//! or the gate classed it hard (the user approved it on a hard card), which
//! includes a call that sets `outside_sandbox`.
//!
//! Loadout's config folder stays read-only to commands: it holds the
//! permission policy and the consent ledger, so a command that could write it
//! could loosen its own gate.

pub mod git_guard;
#[cfg(target_os = "linux")]
pub mod landlock;
pub mod proxy;
#[cfg(windows)]
pub mod windows;

use std::path::{Path, PathBuf};

use serde_json::Value;

pub const CONFIG_NAME: &str = "shell_sandbox.toml";

/// The call arg that asks to run outside the sandbox. It always asks first:
/// the hard gate classes it as irreversible OS.
pub const OUTSIDE_ARG: &str = "outside_sandbox";

/// `shell_sandbox.toml` in the config folder:
///
/// ```toml
/// enabled = true          # false runs commands as before
/// network_guard = true    # false leaves the proxy variables alone
/// writable = ["~/src/other-repo", "/opt/sdk"]
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    pub enabled: bool,
    pub network_guard: bool,
    pub writable: Vec<PathBuf>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            enabled: true,
            network_guard: true,
            writable: Vec::new(),
        }
    }
}

pub fn settings(config_dir: &Path) -> Settings {
    match std::fs::read_to_string(config_dir.join(CONFIG_NAME)) {
        Ok(text) => parse_file(&text, grokhub_core::user_home().as_deref()),
        Err(_) => Settings::default(),
    }
}

fn parse_file(text: &str, home: Option<&Path>) -> Settings {
    let mut out = Settings::default();
    for line in text.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim().to_ascii_lowercase();
        let value = value.trim();
        let off = value.eq_ignore_ascii_case("false");
        match key.as_str() {
            "enabled" => out.enabled = !off,
            "network_guard" => out.network_guard = !off,
            "writable" => {
                let inner = value.trim_start_matches('[').trim_end_matches(']');
                out.writable = inner
                    .split(',')
                    .map(|s| s.trim().trim_matches(['"', '\'']))
                    .filter(|s| !s.is_empty())
                    .filter_map(|s| expand_home(s, home))
                    .collect();
            }
            _ => {}
        }
    }
    out
}

fn expand_home(raw: &str, home: Option<&Path>) -> Option<PathBuf> {
    if raw == "~" {
        return home.map(Path::to_path_buf);
    }
    if let Some(rest) = raw.strip_prefix("~/") {
        return home.map(|h| h.join(rest));
    }
    let path = PathBuf::from(raw);
    path.is_absolute().then_some(path)
}

/// How a shell command is confined. `None` from [`plan`] runs it as before.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    /// Write access only beneath these (canonical, existing) folders.
    pub writable: Vec<PathBuf>,
    /// `127.0.0.1:port` of the egress proxy, when the network guard is on.
    pub proxy: Option<String>,
}

/// The plan for a shell tool call (`run_terminal_command` or `monitor`) in
/// `workspace`, from the config folder in effect.
pub fn plan_for_call(tool: &str, args: &Value, workspace: &Path) -> Option<Plan> {
    plan_in(&crate::perm::config_dir(), tool, args, workspace)
}

pub fn plan_in(config_dir: &Path, tool: &str, args: &Value, workspace: &Path) -> Option<Plan> {
    let settings = settings(config_dir);
    if !settings.enabled || runs_outside(tool, args) {
        return None;
    }
    let home = grokhub_core::user_home();
    let writable = writable_roots(workspace, home.as_deref(), config_dir, &settings.writable);
    let proxy = if settings.network_guard {
        proxy::address(config_dir).ok()
    } else {
        None
    };
    Some(Plan { writable, proxy })
}

/// Whether this call runs on the whole machine: it raises privileges, or the
/// gate classed it hard (so the user approved it on a card).
pub fn runs_outside(tool: &str, args: &Value) -> bool {
    let command = args.get("command").and_then(Value::as_str).unwrap_or("");
    if raises_privileges(command) {
        return true;
    }
    matches!(
        crate::harness::classify(tool, &args.to_string()),
        crate::harness::HardHit::Class(_)
    )
}

/// Heads that raise privileges; no-new-privileges would make them fail.
const PRIVILEGE_HEADS: &[&str] = &["sudo", "pkexec", "doas", "su", "run0"];
/// Wrappers skipped to find the real head of a segment.
const WRAPPERS: &[&str] = &["env", "nice", "nohup", "time", "command", "exec", "stdbuf"];

pub fn raises_privileges(command: &str) -> bool {
    let cmd = command.replace("$(", ";").replace("<(", ";");
    cmd.split([';', '|', '&', '\n', '\r', '`', '(', ')', '{', '}'])
        .any(|seg| {
            let head = seg.split_whitespace().find(|w| {
                let leaf = w.rsplit('/').next().unwrap_or(w);
                !(w.contains('=') && !w.starts_with('='))
                    && !WRAPPERS.contains(&leaf)
                    && !w.starts_with('-')
            });
            head.is_some_and(|w| {
                let w = w.trim_matches(['"', '\'']);
                PRIVILEGE_HEADS.contains(&w.rsplit('/').next().unwrap_or(w))
            })
        })
}

/// Folders the tool caches write to, under the home folder. Only the data
/// parts: a tool's `bin`, its config (cargo's config file,
/// `~/.m2/settings.xml`) and `~/.gradle/init.d` stay read-only, since code a
/// sandboxed command left there would later run outside the sandbox.
pub const CACHE_DIRS: &[&str] = &[
    ".cache",
    ".cargo/registry",
    ".cargo/git",
    ".rustup/downloads",
    ".rustup/tmp",
    ".npm/_cacache",
    ".npm/_logs",
    ".pnpm-store",
    ".yarn/berry/cache",
    ".bun/install/cache",
    "go/pkg/mod",
    ".gradle/caches",
    ".gradle/daemon",
    ".m2/repository",
    ".nuget/packages",
    ".local/share/pnpm/store",
    // Windows homes keep the caches under AppData\Local.
    "AppData/Local/npm-cache",
    "AppData/Local/pip/Cache",
    "AppData/Local/go-build",
    "AppData/Local/Yarn/Cache",
    "AppData/Local/pnpm/store",
    "AppData/Local/NuGet",
];

/// The writable roots, canonical and existing. A root that holds the config
/// folder is dropped (it would expose the permission policy), except the
/// workspace itself, without which no work gets done.
pub fn writable_roots(
    workspace: &Path,
    home: Option<&Path>,
    config_dir: &Path,
    extra: &[PathBuf],
) -> Vec<PathBuf> {
    let config = config_dir
        .canonicalize()
        .unwrap_or_else(|_| config_dir.to_path_buf());
    let mut wanted: Vec<PathBuf> = vec![
        std::env::temp_dir(),
        PathBuf::from("/tmp"),
        PathBuf::from("/var/tmp"),
        PathBuf::from("/dev"),
    ];
    if let Some(home) = home {
        for dir in CACHE_DIRS {
            let path = home.join(dir);
            // A tool that is installed gets its data folder made up front, so
            // its first sandboxed build doesn't fail on a missing folder. Only
            // when the folder just above it is there (`~/.cargo` for
            // `~/.cargo/registry`), so no folder appears for a tool that isn't.
            if dir.contains('/') && !path.exists() && path.parent().is_some_and(Path::is_dir) {
                let _ = std::fs::create_dir_all(&path);
            }
            wanted.push(path);
        }
    }
    wanted.extend(extra.iter().cloned());
    let mut out: Vec<PathBuf> = Vec::new();
    if let Ok(ws) = workspace.canonicalize() {
        out.push(ws);
    }
    for path in wanted {
        let Ok(path) = path.canonicalize() else {
            continue;
        };
        if config.starts_with(&path) || out.contains(&path) {
            continue;
        }
        out.push(path);
    }
    out
}

/// Apply a plan to a command before it is spawned: the proxy variables, and
/// on Linux the Landlock ruleset, which the child turns on just before exec.
/// Called after any other `pre_exec` (so `setsid` runs first).
#[cfg(unix)]
pub fn apply(cmd: &mut std::process::Command, plan: &Plan) -> Result<(), String> {
    if let Some(addr) = &plan.proxy {
        for key in proxy::UNSET {
            cmd.env_remove(key);
        }
        cmd.envs(proxy::env(addr));
    }
    #[cfg(target_os = "linux")]
    {
        use std::os::fd::AsRawFd;
        use std::os::unix::process::CommandExt;
        let Ok(ruleset) = landlock::ruleset(&plan.writable) else {
            // No Landlock on this kernel: writes are not confined.
            return Ok(());
        };
        let ruleset = std::sync::Arc::new(ruleset);
        let raw = ruleset.as_raw_fd();
        // SAFETY: restrict_self only calls prctl and one syscall, both
        // async-signal-safe; the Arc keeps the fd open while `cmd` lives.
        unsafe {
            cmd.pre_exec(move || {
                let _keep = &ruleset;
                landlock::restrict_self(raw)
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
