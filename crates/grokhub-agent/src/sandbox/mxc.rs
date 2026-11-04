//! Windows 11: a sandboxed command runs in its own process container through
//! Microsoft Execution Containers (MXC), the same OS feature Windows' own
//! agent tooling uses. No administrator rights and no install step.
//!
//! - Network: outbound connections are blocked except to the egress proxy's
//!   loopback port (MXC's identity-less proxy mode), so `curl --noproxy`, raw
//!   sockets and direct DNS don't get out. The OS also points the container's
//!   WinHTTP at the proxy.
//! - Writes: only beneath the writable roots (the workspace, temp folders and
//!   the tool caches).
//! - Reads: a container reads nothing by default. It gets the system and
//!   tool folders, and the home folder with the secret folders taken out
//!   ([`super::secrets`]) where the OS can deny paths; elsewhere a short list
//!   of config folders in the home folder instead of all of it.
//!
//! Only Windows 11 builds with the process container (24H2 build 26100.9278
//! and later) qualify; [`available`] asks the OS. Elsewhere, or with
//! `GROKHUB_SANDBOX_NO_MXC=1`, commands keep the write-restricted token
//! ([`super::windows`]) and the proxy variables. A container that fails to
//! start turns MXC off for the rest of the session, so later commands use the
//! token and the gate asks again.

use std::path::{Path, PathBuf};

use super::Plan;

/// Home-folder entries a command commonly reads (git and package manager
/// config, toolchains) when the OS can't deny paths, so the home folder as a
/// whole stays closed. Missing ones are skipped; any that holds a secret is
/// dropped.
pub const HOME_READS: &[&str] = &[
    ".gitconfig",
    ".config/git",
    ".npmrc",
    ".yarnrc",
    ".yarnrc.yml",
    "AppData/Roaming/npm",
    "AppData/Local/pnpm",
    ".cargo",
    ".rustup",
    ".nuget/packages",
    "go",
    ".m2",
    ".gradle",
    "scoop",
];

/// The folders and network for one command's container. Plain data, so it is
/// built and tested on every platform.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Layout {
    pub readwrite: Vec<String>,
    pub readonly: Vec<String>,
    pub denied: Vec<String>,
    /// `http://127.0.0.1:PORT` of the egress proxy; None when the network
    /// guard is off (the container's network is then open).
    pub proxy: Option<String>,
}

/// Adds `path` in its full, long form, so a denied path inside a writable
/// root names it the same way (Windows also has short `RUNNER~1` names).
fn push(list: &mut Vec<String>, path: &Path) {
    let path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let s = crate::tools::shell::plain_dir(&path.display().to_string());
    if !s.is_empty() && !list.contains(&s) {
        list.push(s);
    }
}

/// Whether `path` holds a hidden one (or is one).
fn holds_secret(path: &Path, hide: &[PathBuf]) -> bool {
    hide.iter().any(|h| h.starts_with(path))
}

/// The container's layout for `plan`. `tools` and `temp` are the OS's own
/// lists of tool folders (read) and temp folders (write); `deny` is whether
/// the OS enforces denied paths.
pub fn layout(
    plan: &Plan,
    home: Option<&Path>,
    deny: bool,
    tools: &[String],
    temp: &[String],
) -> Layout {
    let hide: Vec<PathBuf> = plan
        .isolation
        .as_ref()
        .map(|i| i.hide.clone())
        .unwrap_or_default();
    let mut out = Layout::default();
    for root in &plan.writable {
        push(&mut out.readwrite, root);
    }
    for t in temp {
        push(&mut out.readwrite, Path::new(t));
    }
    let mut reads: Vec<PathBuf> = tools.iter().map(PathBuf::from).collect();
    if let Some(home) = home {
        if deny {
            reads.push(home.to_path_buf());
        } else {
            reads.extend(
                HOME_READS
                    .iter()
                    .map(|rel| home.join(rel))
                    .filter(|p| p.exists()),
            );
        }
    }
    for path in reads {
        let path = path.canonicalize().unwrap_or(path);
        // Without denied paths, a folder that holds a secret stays closed.
        if !deny && holds_secret(&path, &hide) {
            continue;
        }
        push(&mut out.readonly, &path);
    }
    if deny {
        for path in &hide {
            push(&mut out.denied, path);
        }
    }
    out.proxy = plan.proxy.as_ref().map(|addr| format!("http://{addr}"));
    out
}

#[cfg(windows)]
pub use win::{available, mark_broken, spawn};

#[cfg(windows)]
mod win {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::OnceLock;

    use mxc_sdk::v1::policy::filesystem::{
        available_tools_policy, temporary_files_policy, ToolsPolicyContainerType,
        ToolsPolicyOptions,
    };
    use mxc_sdk::v1::{
        self, BackendCapability, ContainerRequest, FilesystemPolicy, MxcProcess, NetworkAction,
        NetworkEgressPolicy, NetworkIngressPolicy, NetworkPolicy, NetworkRuntimeConfig,
        SpawnOptions,
    };

    use super::super::Plan;

    /// What this machine's process container supports. None: no container.
    struct Support {
        deny: bool,
    }

    static BROKEN: AtomicBool = AtomicBool::new(false);

    fn support() -> Option<&'static Support> {
        static SUPPORT: OnceLock<Option<Support>> = OnceLock::new();
        SUPPORT
            .get_or_init(|| {
                let backend = v1::available_backends()
                    .into_iter()
                    .find(|b| b.backend == "processcontainer")?;
                // The proxy mode Loadout's own (unpackaged) proxy needs.
                backend
                    .capabilities
                    .contains(&BackendCapability::IdentitylessLoopbackProxy)
                    .then(|| Support {
                        deny: backend
                            .capabilities
                            .contains(&BackendCapability::FilesystemDeniedPaths),
                    })
            })
            .as_ref()
    }

    /// Whether commands run in a process container on this machine.
    pub fn available() -> bool {
        std::env::var_os("GROKHUB_SANDBOX_NO_MXC").is_none()
            && !BROKEN.load(Ordering::Relaxed)
            && support().is_some()
    }

    /// A container failed to start: the token for the rest of the session.
    pub fn mark_broken() {
        BROKEN.store(true, Ordering::Relaxed);
    }

    fn request(
        plan: &Plan,
        cwd: &str,
        command_line: String,
        env: &[(String, String)],
    ) -> ContainerRequest {
        let deny = support().is_some_and(|s| s.deny);
        let tools = available_tools_policy(
            None,
            ToolsPolicyOptions {
                container_type: Some(ToolsPolicyContainerType::ProcessContainer),
            },
        );
        let temp = temporary_files_policy(None);
        let mut tool_reads = tools.readonly_paths;
        tool_reads
            .extend(mxc_sdk::v1::policy::filesystem::user_profile_policy(None).readonly_paths);
        let mut temp_writes = temp.readwrite_paths;
        // The tools policy's own writable folders (PowerShell's history).
        temp_writes.extend(tools.readwrite_paths);
        let home = grokhub_core::user_home();
        let layout = super::layout(plan, home.as_deref(), deny, &tool_reads, &temp_writes);
        let network = match &layout.proxy {
            // Identity-less proxy: egress only to the proxy's loopback port.
            Some(proxy) => NetworkPolicy {
                egress: Some(NetworkEgressPolicy {
                    default: Some(NetworkAction::Deny),
                    ..Default::default()
                }),
                ingress: Some(NetworkIngressPolicy {
                    default: Some(NetworkAction::Allow),
                    host_loopback: Some(NetworkAction::Allow),
                }),
                runtime_config: Some(NetworkRuntimeConfig {
                    network_proxy: Some(proxy.clone()),
                }),
            },
            None => NetworkPolicy {
                egress: Some(NetworkEgressPolicy {
                    default: Some(NetworkAction::Allow),
                    ..Default::default()
                }),
                ingress: Some(NetworkIngressPolicy {
                    default: Some(NetworkAction::Allow),
                    host_loopback: Some(NetworkAction::Allow),
                }),
                runtime_config: None,
            },
        };
        let mut req = ContainerRequest::new(command_line);
        req.filesystem = Some(FilesystemPolicy {
            readwrite_paths: layout.readwrite,
            readonly_paths: layout.readonly,
            denied_paths: layout.denied,
            clear_policy_on_exit: None,
        });
        req.network = Some(network);
        req.working_directory = Some(cwd.to_string());
        if !env.is_empty() {
            req.environment = Some(env.to_vec());
            req.inherit_default_environment = Some(true);
        }
        req
    }

    /// Start `command_line` in a container for `plan`, in `cwd`.
    pub fn spawn(
        plan: &Plan,
        cwd: &str,
        command_line: String,
        env: &[(String, String)],
    ) -> Result<MxcProcess, String> {
        v1::spawn(
            request(plan, cwd, command_line, env),
            SpawnOptions::default(),
        )
        .map_err(|e| match &e.remediation {
            Some(hint) => format!("the Windows process container did not start: {e} ({hint})"),
            None => format!("the Windows process container did not start: {e}"),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sandbox::Isolation;

    fn plan(base: &Path, proxy: bool) -> Plan {
        Plan {
            writable: vec![base.join("ws")],
            proxy: proxy.then(|| "127.0.0.1:8123".to_string()),
            isolation: Some(Isolation {
                hide: vec![
                    base.join("home/.ssh").canonicalize().unwrap(),
                    base.join("home/.cargo/credentials.toml")
                        .canonicalize()
                        .unwrap(),
                ],
                bridge: None,
                net_confined: proxy,
            }),
        }
    }

    #[test]
    fn the_layout_writes_the_roots_reads_tools_and_keeps_secrets_closed() {
        let base = std::env::temp_dir().join(format!("loadout-mxc-{}", std::process::id()));
        let home = base.join("home");
        for dir in [
            "ws",
            "home/.ssh",
            "home/.cargo",
            "home/.config/git",
            "tools",
        ] {
            std::fs::create_dir_all(base.join(dir)).unwrap();
        }
        std::fs::write(home.join(".cargo/credentials.toml"), "x").unwrap();
        std::fs::write(home.join(".gitconfig"), "x").unwrap();
        let tools = vec![base.join("tools").display().to_string()];
        let temp = vec![base.join("tmp").display().to_string()];
        let s = |p: PathBuf| {
            let p = p.canonicalize().unwrap_or(p);
            crate::tools::shell::plain_dir(&p.display().to_string())
        };

        // The OS denies paths: the whole home folder, minus the secrets.
        let l = layout(&plan(&base, true), Some(&home), true, &tools, &temp);
        assert!(l.readwrite.contains(&s(base.join("ws"))), "{l:?}");
        assert!(l.readwrite.contains(&temp[0]), "{l:?}");
        assert!(l.readonly.contains(&s(home.clone())), "{l:?}");
        assert!(l.readonly.contains(&s(base.join("tools"))), "{l:?}");
        assert!(l.denied.contains(&s(home.join(".ssh"))), "{l:?}");
        assert!(l.denied.contains(&s(home.join(".cargo/credentials.toml"))));
        assert_eq!(l.proxy.as_deref(), Some("http://127.0.0.1:8123"));

        // It doesn't: only config folders, and never one holding a secret.
        let l = layout(&plan(&base, true), Some(&home), false, &tools, &temp);
        assert!(!l.readonly.contains(&s(home.clone())), "{l:?}");
        assert!(l.readonly.contains(&s(home.join(".gitconfig"))), "{l:?}");
        assert!(l.readonly.contains(&s(home.join(".config/git"))), "{l:?}");
        assert!(!l.readonly.iter().any(|p| p.ends_with(".cargo")), "{l:?}");
        assert!(l.denied.is_empty());

        // The network guard is off: no proxy.
        let l = layout(&plan(&base, false), Some(&home), true, &tools, &temp);
        assert_eq!(l.proxy, None);
        let _ = std::fs::remove_dir_all(&base);
    }

    /// The real container, where this Windows has one (Windows 11 24H2 build
    /// 26100.9278 or later; CI's Windows Server skips it).
    #[cfg(windows)]
    #[test]
    fn a_container_command_writes_reads_and_connects_only_where_allowed() {
        use base64::Engine as _;
        use std::io::Write as _;
        use std::net::TcpListener;

        if !available() {
            eprintln!("skipped: no process container on this Windows");
            return;
        }
        // Outside the temp folders, which a real plan leaves writable.
        let home = grokhub_core::user_home().expect("home");
        let base = home.join(format!("gh-sbx-win-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        for dir in ["ws", "out", "secret"] {
            std::fs::create_dir_all(base.join(dir)).unwrap();
        }
        let ws = base.join("ws").canonicalize().unwrap();
        let outside = base.join("out").canonicalize().unwrap();
        let secret_dir = base.join("secret").canonicalize().unwrap();
        std::fs::write(secret_dir.join("id_ed25519"), "key").unwrap();
        let secret_file = base.join("netrc");
        std::fs::write(&secret_file, "machine x password y").unwrap();
        let secret_file = secret_file.canonicalize().unwrap();

        let serve = || {
            let l = TcpListener::bind("127.0.0.1:0").unwrap();
            let port = l.local_addr().unwrap().port();
            std::thread::spawn(move || {
                for mut s in l.incoming().flatten() {
                    let _ = s.write_all(b"hello\n");
                }
            });
            port
        };
        let proxy_port = serve();
        let other_port = serve();
        let plan = Plan {
            writable: vec![ws.clone()],
            proxy: Some(format!("127.0.0.1:{proxy_port}")),
            isolation: Some(crate::sandbox::Isolation {
                hide: vec![secret_dir.clone(), secret_file.clone()],
                bridge: None,
                net_confined: true,
            }),
        };
        let plain = |p: &Path| crate::tools::shell::plain_dir(&p.display().to_string());
        let ps = |script: String| {
            let script = format!("$ErrorActionPreference = 'Stop'; {script}");
            let bytes: Vec<u8> = script
                .encode_utf16()
                .flat_map(|unit| unit.to_le_bytes())
                .collect();
            let exe = format!(
                r"{}\System32\WindowsPowerShell\v1.0\powershell.exe",
                std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into())
            );
            let line = format!(
                "\"{exe}\" -NoProfile -NonInteractive -EncodedCommand {}",
                base64::engine::general_purpose::STANDARD.encode(bytes)
            );
            let proc = spawn(&plan, &plain(&ws), line, &[]).expect("the container starts");
            let out = proc.wait_with_output().expect("the command ends");
            let ok = matches!(out.outcome, mxc_sdk::v1::WaitResult::Exited(0));
            let text = format!(
                "{}{}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            );
            (ok, text)
        };

        let (ok, text) = ps("Set-Content inside.txt in; Get-Content inside.txt".into());
        assert!(
            ok && text.contains("in"),
            "a write in the workspace: {text}"
        );

        let target = outside.join("new.txt");
        let (ok, text) = ps(format!("Set-Content -LiteralPath '{}' out", plain(&target)));
        assert!(
            !ok && !target.exists(),
            "a write outside the workspace went through: {text}"
        );

        let (ok, text) = ps(format!(
            "Get-Content -LiteralPath '{}'",
            plain(&secret_file)
        ));
        assert!(
            !ok && !text.contains("password"),
            "a hidden file was read: {text}"
        );
        let (ok, text) = ps(format!(
            "Get-ChildItem -LiteralPath '{}'",
            plain(&secret_dir)
        ));
        assert!(
            !ok && !text.contains("id_ed25519"),
            "a hidden folder was listed: {text}"
        );

        let connect = |port: u16| {
            format!(
                "$c = [Net.Sockets.TcpClient]::new('127.0.0.1', {port}); \
                 [IO.StreamReader]::new($c.GetStream()).ReadLine()"
            )
        };
        let (ok, text) = ps(connect(proxy_port));
        assert!(ok && text.contains("hello"), "the proxy's port: {text}");
        let (ok, text) = ps(connect(other_port));
        assert!(
            !ok && !text.contains("hello"),
            "a port other than the proxy's: {text}"
        );
        let (ok, text) = ps("[Net.Sockets.TcpClient]::new('example.com', 443).Connected".into());
        assert!(!ok, "a direct connection went out: {text}");
        let _ = std::fs::remove_dir_all(&base);
    }
}
