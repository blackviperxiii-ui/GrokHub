//! HX-11: model-written shell commands run in a sandbox by default.
//!
//! Writes are allowed only in the workspace, temp folders, `/dev` and the
//! usual tool caches (Landlock on Linux, a Seatbelt profile on macOS, a
//! write-restricted token on Windows). Network goes through the egress
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
#[cfg(unix)]
pub mod mask;
#[cfg(unix)]
pub mod mitm;
pub mod mxc;
pub mod net_allow;
#[cfg(target_os = "linux")]
pub mod netns;
pub mod proxy;
#[cfg(any(target_os = "macos", test))]
pub mod seatbelt;
#[cfg(target_os = "linux")]
pub mod seccomp;
pub mod secrets;
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
    /// Run an ordinary command without asking when the sandbox fully holds
    /// it (writes, network and secrets). `auto_allow = false` asks as before.
    pub auto_allow: bool,
    pub writable: Vec<PathBuf>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            enabled: true,
            network_guard: true,
            auto_allow: true,
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
    let file = crate::config_file::parse(
        CONFIG_NAME,
        text,
        &["writable", "enabled", "network_guard", "auto_allow"],
    );
    let defaults = Settings::default();
    let writable = file
        .get("writable")
        .map(|value| {
            value
                .trim_start_matches('[')
                .trim_end_matches(']')
                .split(',')
                .map(|s| s.trim().trim_matches(['"', '\'']))
                .filter(|s| !s.is_empty())
                .filter_map(|s| expand_home(s, home))
                .collect()
        })
        .unwrap_or_default();
    Settings {
        enabled: file.flag("enabled", defaults.enabled),
        network_guard: file.flag("network_guard", defaults.network_guard),
        auto_allow: file.flag("auto_allow", defaults.auto_allow),
        writable,
    }
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
    /// Linux, where unprivileged namespaces are allowed: the command's own
    /// namespaces (see [`netns`]). macOS: its Seatbelt profile (see
    /// [`seatbelt`]). Windows 11: its process container (see [`mxc`]). None
    /// elsewhere.
    pub isolation: Option<Isolation>,
}

impl Plan {
    /// Whether the command is fully held: writes confined, the network only
    /// through the proxy and secret folders hidden. Linux needs Landlock and
    /// its own network namespace; macOS its Seatbelt profile with the proxy
    /// port; Windows its process container with the proxy port.
    pub fn enforced(&self) -> bool {
        let net = self.isolation.as_ref().is_some_and(|i| i.net_confined);
        #[cfg(target_os = "linux")]
        let writes = landlock::abi().is_some();
        // macOS and Windows have isolation only with their write fence.
        #[cfg(any(target_os = "macos", windows))]
        let writes = true;
        #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
        let writes = false;
        net && writes
    }

    /// The port the proxy variables name.
    pub fn proxy_port(&self) -> Option<u16> {
        self.proxy.as_deref()?.rsplit(':').next()?.parse().ok()
    }
}

/// What hides secrets and fences the network in: on Linux the command's own
/// namespaces, on macOS its Seatbelt profile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Isolation {
    pub hide: Vec<PathBuf>,
    /// Linux: the proxy's Unix socket and the port the proxy variables name,
    /// for the relay in the command's own network.
    pub bridge: Option<(PathBuf, u16)>,
    /// Only the proxy reaches out: no direct connection, no DNS.
    pub net_confined: bool,
}

/// The plan for a shell tool call (`run_terminal_command` or `monitor`) in
/// `workspace`, from the config folder in effect.
pub fn plan_for_call(tool: &str, args: &Value, workspace: &Path) -> Option<Plan> {
    plan_in(&crate::perm::config_dir(), tool, args, workspace)
}

/// Whether the gate may run this shell call without asking: auto-allow is on
/// and the sandbox will fully hold it (see [`Plan::enforced`]). A call that
/// asks to run outside the sandbox, or raises privileges, never qualifies.
pub fn auto_allows(config_dir: &Path, tool: &str, args: &Value, workspace: &Path) -> bool {
    settings(config_dir).auto_allow
        && plan_in(config_dir, tool, args, workspace).is_some_and(|plan| plan.enforced())
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
    let isolation = isolation(config_dir, workspace, home.as_deref(), proxy.as_deref());
    Some(Plan {
        writable,
        proxy,
        isolation,
    })
}

#[cfg(target_os = "linux")]
fn isolation(
    config_dir: &Path,
    workspace: &Path,
    home: Option<&Path>,
    proxy: Option<&str>,
) -> Option<Isolation> {
    if !netns::available() {
        return None;
    }
    let bridge = proxy.and_then(|addr| {
        let port = addr.rsplit(':').next()?.parse().ok()?;
        Some((proxy::unix_bridge(config_dir).ok()?, port))
    });
    // The network guard is on but the bridge can't start: no namespace, so
    // the command isn't cut off; the proxy variables still apply.
    if proxy.is_some() && bridge.is_none() {
        return None;
    }
    let net_confined = bridge.is_some();
    Some(Isolation {
        hide: secrets::hidden(home, config_dir, workspace),
        bridge,
        net_confined,
    })
}

#[cfg(target_os = "macos")]
fn isolation(
    config_dir: &Path,
    workspace: &Path,
    home: Option<&Path>,
    proxy: Option<&str>,
) -> Option<Isolation> {
    seatbelt::available().then(|| Isolation {
        hide: secrets::hidden(home, config_dir, workspace),
        bridge: None,
        net_confined: proxy
            .and_then(|a| a.rsplit(':').next()?.parse::<u16>().ok())
            .is_some(),
    })
}

#[cfg(windows)]
fn isolation(
    config_dir: &Path,
    workspace: &Path,
    home: Option<&Path>,
    proxy: Option<&str>,
) -> Option<Isolation> {
    mxc::available().then(|| Isolation {
        hide: secrets::hidden(home, config_dir, workspace),
        bridge: None,
        net_confined: proxy
            .and_then(|a| a.rsplit(':').next()?.parse::<u16>().ok())
            .is_some(),
    })
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
fn isolation(_: &Path, _: &Path, _: Option<&Path>, _: Option<&str>) -> Option<Isolation> {
    None
}

/// After a failed command in its own network: when the output reads like a
/// connection that had no way out (SSH, a raw socket, a host service), a
/// note that only the HTTP proxy reaches out and how to get the whole
/// machine. None otherwise.
pub fn network_note(plan: Option<&Plan>, failed: bool, output: &str) -> Option<&'static str> {
    // The proxy's refusal, as curl, git, pip and the like report it.
    const REFUSED: &[&str] = &[
        "not on the sandbox's network list",
        "CONNECT tunnel failed, response 403",
        "HTTP code 403 from proxy",
        "Tunnel connection failed: 403",
        "403 Forbidden",
    ];
    const SIGNS: &[&str] = &[
        "Could not resolve host",
        "Could not resolve hostname",
        "Temporary failure in name resolution",
        "Name or service not known",
        "Network is unreachable",
        "Connection refused",
        "ssh: connect to host",
        // curl, and macOS, where a fenced-in connect fails with EPERM.
        "Failed to connect to",
        "connect: Operation not permitted",
        "nodename nor servname provided",
    ];
    let plan = plan.filter(|_| failed)?;
    if plan.proxy.is_some() && REFUSED.iter().any(|s| output.contains(s)) {
        return Some(
            "[sandbox] The proxy only lets sandboxed commands reach package registries, code hosts, model hosts and hosts your WebFetch allow rules name. For another host, rerun with outside_sandbox (asks the user).",
        );
    }
    let isolated = plan.isolation.as_ref().is_some_and(|i| i.net_confined);
    (isolated && SIGNS.iter().any(|s| output.contains(s))).then_some(
        "[sandbox] This command's network is fenced in: only HTTP(S) through the proxy gets out, so SSH, DNS lookups, raw sockets and host services such as Docker or an SSH agent don't. If it needs them, rerun with outside_sandbox (asks the user).",
    )
}

/// One line on how commands are confined on this machine, for the doctor
/// and the docs.
pub fn status(config_dir: &Path) -> String {
    let settings = settings(config_dir);
    if !settings.enabled {
        return "Shell sandbox: off (shell_sandbox.toml)".into();
    }
    #[cfg(target_os = "linux")]
    {
        let writes = if landlock::abi().is_some() {
            "writes confined (Landlock)"
        } else {
            "writes not confined (no Landlock on this kernel)"
        };
        let (net, reads) = if netns::available() {
            let net = if settings.network_guard {
                "network only through the egress proxy (own namespace)"
            } else {
                "network open (network_guard = false)"
            };
            (net, "secret folders hidden")
        } else {
            let net = if settings.network_guard {
                "network through the proxy variables only (this system doesn't allow unprivileged namespaces)"
            } else {
                "network open (network_guard = false)"
            };
            (net, "reads open")
        };
        format!(
            "Shell sandbox: {writes}; {net}; {reads}{}",
            masked_clause(settings.network_guard)
        )
    }
    #[cfg(windows)]
    {
        if mxc::available() {
            let net = if settings.network_guard {
                "network only through the egress proxy (process container)"
            } else {
                "network open (network_guard = false)"
            };
            return format!(
                "Shell sandbox: writes confined (Windows process container); {net}; secret folders hidden"
            );
        }
        let net = if settings.network_guard {
            "network through the proxy variables only"
        } else {
            "network open (network_guard = false)"
        };
        format!("Shell sandbox: writes confined (restricted token); {net}; reads open (no process container on this Windows)")
    }
    #[cfg(target_os = "macos")]
    {
        if !seatbelt::available() {
            return format!(
                "Shell sandbox: proxy variables only (sandbox-exec is off or missing){}",
                masked_clause(settings.network_guard)
            );
        }
        let net = if settings.network_guard {
            "network only through the egress proxy (Seatbelt)"
        } else {
            "network open (network_guard = false)"
        };
        format!(
            "Shell sandbox: writes confined (Seatbelt); {net}; secret folders hidden{}",
            masked_clause(settings.network_guard)
        )
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    {
        format!(
            "Shell sandbox: proxy variables only on this system{}",
            masked_clause(settings.network_guard)
        )
    }
}

/// How many of the app's tokens commands see only as stand-ins.
#[cfg(unix)]
fn masked_clause(network_guard: bool) -> String {
    let n = mask::session().command_env().len();
    match (network_guard, n) {
        (false, _) | (_, 0) => String::new(),
        (true, 1) => "; 1 token masked".into(),
        (true, n) => format!("; {n} tokens masked"),
    }
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
    // macOS homes keep them under Library.
    "Library/Caches",
    "Library/Developer/Xcode/DerivedData",
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
/// on Linux, in the child just before exec, the namespaces (with the relay),
/// the seccomp filter and the Landlock ruleset, in that order: the relay
/// needs the Unix socket the filter then refuses. Called after any other
/// `pre_exec` (so `setsid` runs first).
#[cfg(unix)]
pub fn apply(cmd: &mut std::process::Command, plan: &Plan) -> Result<(), String> {
    if let Some(addr) = &plan.proxy {
        for key in proxy::UNSET {
            cmd.env_remove(key);
        }
        cmd.envs(proxy::env(addr));
        // Tokens reach the command only as stand-ins; the proxy puts the
        // real one back for its own hosts.
        cmd.envs(mask::session().command_env());
        if let Some(mitm) = mitm::session() {
            cmd.envs(mitm.command_env());
        }
    }
    #[cfg(target_os = "linux")]
    {
        use std::os::fd::AsRawFd;
        use std::os::unix::process::CommandExt;
        let setup = match &plan.isolation {
            Some(iso) => Some(netns::Setup::new(
                &iso.hide,
                iso.bridge
                    .as_ref()
                    .map(|(sock, port)| (sock.as_path(), *port)),
            )?),
            None => None,
        };
        let filter = if plan.isolation.as_ref().is_some_and(|i| i.bridge.is_some()) {
            seccomp::program()
        } else {
            Vec::new()
        };
        // No Landlock on this kernel: writes are not confined.
        let ruleset = landlock::ruleset(&plan.writable).ok();
        if setup.is_none() && filter.is_empty() && ruleset.is_none() {
            return Ok(());
        }
        // SAFETY: enter, install and restrict_self only make async-signal-safe
        // syscalls; the closure owns the ruleset fd while `cmd` lives.
        unsafe {
            cmd.pre_exec(move || {
                if let Some(setup) = &setup {
                    netns::enter(setup)?;
                }
                seccomp::install(&filter)?;
                if let Some(ruleset) = &ruleset {
                    landlock::restrict_self(ruleset.as_raw_fd())?;
                }
                Ok(())
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
