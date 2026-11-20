//! Opt-in container backend (M3): with `backend = "container"` in
//! `shell_sandbox.toml`, each sandboxed command runs in a throwaway Podman or
//! Docker container instead of the host sandbox.
//!
//! - Files: only the workspace and the `writable` folders are mounted,
//!   read-write, at the same paths. Nothing else of the host is there, so the
//!   secret folders the host sandbox hides are simply missing.
//! - Network: `--network none`. Nothing leaves the container, so the egress
//!   proxy's rules hold by refusing every host. A command that needs the
//!   network asks to run with `outside_sandbox`, as on the host.
//! - Image: never pulled here (`--pull never`); the user pulls it once, so
//!   no download happens outside the egress proxy.
//! - Privileges: every capability dropped, no-new-privileges, a read-only
//!   root, a fresh `/tmp`, and the user's own uid and gid.
//! - Stop: the command's stdin is a pipe the app holds. When the runtime's
//!   client is killed (a stop or a timeout) or the app exits, the pipe closes
//!   and the watchdog inside kills the command, so no container outlives it.
//!
//! Only a rootless runtime is used: Podman run by a normal user, or rootless
//! Docker. A root daemon would give the command root on the host through its
//! mounts, so one is refused unless the app already runs as root.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

/// The image when `image` isn't set. It has bash, coreutils and nothing else.
pub const DEFAULT_IMAGE: &str = "docker.io/library/debian:stable-slim";

/// Every container this backend starts carries this label, so `ps --filter
/// label=grokhub.sandbox` lists them.
pub const LABEL: &str = "grokhub.sandbox=shell";

/// Runs `$0` with its stdin cut off and kills it once the container's own
/// stdin closes, then exits with its status. `--init` reaps what is left.
/// The stdin is kept on fd 3 first: `sh` gives a background job `/dev/null`
/// for stdin unless it is redirected.
const WATCHDOG: &str = "exec 3<&0; bash --noprofile --norc -c \"$0\" </dev/null 3<&- & p=$!; \
(cat <&3 >/dev/null; kill -KILL $p 2>/dev/null) & exec 3<&-; wait $p";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Podman,
    Docker,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Runtime {
    pub kind: Kind,
    pub program: PathBuf,
}

/// One command's container.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Container {
    pub runtime: Runtime,
    pub image: String,
    /// Mounted read-write at the same path. The workspace comes first.
    pub mounts: Vec<PathBuf>,
}

/// True when `info` (the runtime's own report, see [`probe`]) says it runs
/// rootless, or the app is already root (`uid == 0`), so the runtime grants
/// nothing more.
pub fn rootless_ok(kind: Kind, info: &str, uid: u32) -> bool {
    if uid == 0 {
        return true;
    }
    match kind {
        Kind::Podman => info.trim() == "true",
        Kind::Docker => info.contains("name=rootless"),
    }
}

/// The first rootless runtime on `PATH`, Podman before Docker. Probed once
/// per run; a runtime that doesn't answer in a few seconds counts as missing.
/// Never on Windows: Docker Desktop there isn't rootless, and bind mounts of
/// Windows paths don't line up with the Linux paths inside.
#[cfg(not(unix))]
pub fn detect() -> Option<Runtime> {
    None
}

/// The first rootless runtime on `PATH`, Podman before Docker. Probed once
/// per run; a runtime that doesn't answer in a few seconds counts as missing.
#[cfg(unix)]
pub fn detect() -> Option<Runtime> {
    static FOUND: OnceLock<Option<Runtime>> = OnceLock::new();
    FOUND
        .get_or_init(|| {
            let (uid, _) = ids();
            [Kind::Podman, Kind::Docker].into_iter().find_map(|kind| {
                let program = on_path(name(kind))?;
                let info = probe(kind, &program)?;
                rootless_ok(kind, &info, uid).then_some(Runtime { kind, program })
            })
        })
        .clone()
}

/// The app's own uid and gid.
#[cfg(unix)]
fn ids() -> (u32, u32) {
    // SAFETY: getuid and getgid can't fail.
    unsafe { (libc::getuid(), libc::getgid()) }
}

/// Windows has no uid; [`detect`] finds nothing there (see below).
#[cfg(not(unix))]
fn ids() -> (u32, u32) {
    (u32::MAX, u32::MAX)
}

#[cfg(unix)]
fn name(kind: Kind) -> &'static str {
    match kind {
        Kind::Podman => "podman",
        Kind::Docker => "docker",
    }
}

#[cfg(unix)]
fn on_path(program: &str) -> Option<PathBuf> {
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|dir| dir.join(program))
        .find(|path| path.is_file())
}

/// Podman's `Host.Security.Rootless` or Docker's `SecurityOptions`.
#[cfg(unix)]
fn probe(kind: Kind, program: &Path) -> Option<String> {
    let format = match kind {
        Kind::Podman => "{{.Host.Security.Rootless}}",
        Kind::Docker => "{{.SecurityOptions}}",
    };
    let mut child = Command::new(program)
        .args(["info", "--format", format])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .ok()?;
    let started = std::time::Instant::now();
    loop {
        match child.try_wait().ok()? {
            Some(status) if status.success() => break,
            Some(_) => return None,
            None if started.elapsed() > std::time::Duration::from_secs(5) => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
            None => std::thread::sleep(std::time::Duration::from_millis(50)),
        }
    }
    let mut out = String::new();
    std::io::Read::read_to_string(&mut child.stdout.take()?, &mut out).ok()?;
    Some(out)
}

/// The workspace, then each extra writable folder that is there, made
/// absolute. A path with a comma is left out: the runtime's `--mount` syntax
/// can't carry one.
pub fn mounts(workspace: &Path, extra: &[PathBuf]) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    for path in std::iter::once(workspace).chain(extra.iter().map(PathBuf::as_path)) {
        let Ok(path) = path.canonicalize() else {
            continue;
        };
        if path.to_string_lossy().contains(',') || out.contains(&path) {
            continue;
        }
        out.push(path);
    }
    out
}

/// The runtime's arguments for `command` in `cwd`, run as `uid:gid`.
pub fn args(c: &Container, cwd: &Path, uid: u32, gid: u32, command: &str) -> Vec<String> {
    let mut out: Vec<String> = [
        "run",
        "--rm",
        "-i",
        "--init",
        "--pull",
        "never",
        "--label",
        LABEL,
        "--network",
        "none",
        "--cap-drop",
        "ALL",
        "--security-opt",
        "no-new-privileges",
        "--read-only",
        "--tmpfs",
        "/tmp:rw,exec,mode=1777",
        "--env",
        "HOME=/tmp",
    ]
    .map(String::from)
    .to_vec();
    match c.runtime.kind {
        // keep-id maps the user to the same uid and gid inside.
        Kind::Podman => out.push("--userns=keep-id".into()),
        Kind::Docker => out.extend(["--user".into(), format!("{uid}:{gid}")]),
    }
    for path in &c.mounts {
        let path = path.display();
        out.extend([
            "--mount".into(),
            format!("type=bind,source={path},target={path}"),
        ]);
    }
    out.extend([
        "--workdir".into(),
        cwd.display().to_string(),
        c.image.clone(),
        "sh".into(),
        "-c".into(),
        WATCHDOG.into(),
        command.into(),
    ]);
    out
}

/// The runtime's client for `command`. The caller sets stdin to a pipe it
/// keeps open while the command runs.
pub fn command(c: &Container, cwd: &Path, command: &str) -> Command {
    let (uid, gid) = ids();
    let mut cmd = Command::new(&c.runtime.program);
    cmd.args(args(c, cwd, uid, gid, command));
    cmd
}

#[cfg(test)]
mod tests {
    use super::*;

    fn docker(mounts: Vec<PathBuf>) -> Container {
        Container {
            runtime: Runtime {
                kind: Kind::Docker,
                program: PathBuf::from("/usr/bin/docker"),
            },
            image: DEFAULT_IMAGE.into(),
            mounts,
        }
    }

    #[test]
    fn docker_runs_with_no_network_no_capabilities_and_the_users_ids() {
        let c = docker(vec![
            PathBuf::from("/home/user/my-app"),
            PathBuf::from("/opt/sdk"),
        ]);
        let args = args(
            &c,
            Path::new("/home/user/my-app/src"),
            1000,
            1000,
            "make test",
        );
        assert_eq!(
            args,
            [
                "run",
                "--rm",
                "-i",
                "--init",
                "--pull",
                "never",
                "--label",
                "grokhub.sandbox=shell",
                "--network",
                "none",
                "--cap-drop",
                "ALL",
                "--security-opt",
                "no-new-privileges",
                "--read-only",
                "--tmpfs",
                "/tmp:rw,exec,mode=1777",
                "--env",
                "HOME=/tmp",
                "--user",
                "1000:1000",
                "--mount",
                "type=bind,source=/home/user/my-app,target=/home/user/my-app",
                "--mount",
                "type=bind,source=/opt/sdk,target=/opt/sdk",
                "--workdir",
                "/home/user/my-app/src",
                "docker.io/library/debian:stable-slim",
                "sh",
                "-c",
                WATCHDOG,
                "make test",
            ]
            .map(String::from)
            .to_vec()
        );
    }

    #[test]
    fn podman_keeps_the_users_id_instead_of_a_user_flag() {
        let mut c = docker(vec![PathBuf::from("/home/user/my-app")]);
        c.runtime.kind = Kind::Podman;
        let args = args(&c, Path::new("/home/user/my-app"), 1000, 1000, "ls");
        assert!(args.contains(&"--userns=keep-id".to_string()));
        assert!(!args.contains(&"--user".to_string()));
    }

    #[test]
    fn only_a_rootless_runtime_is_used_unless_the_app_is_root() {
        assert!(rootless_ok(Kind::Podman, "true\n", 1000));
        assert!(!rootless_ok(Kind::Podman, "false\n", 1000));
        assert!(rootless_ok(
            Kind::Docker,
            "[name=seccomp,profile=builtin name=rootless name=cgroupns]",
            1000
        ));
        assert!(!rootless_ok(
            Kind::Docker,
            "[name=seccomp,profile=builtin]",
            1000
        ));
        assert!(rootless_ok(
            Kind::Docker,
            "[name=seccomp,profile=builtin]",
            0
        ));
    }

    #[test]
    fn mounts_are_the_workspace_then_extras_that_exist_without_commas() {
        let base = std::env::temp_dir().join(format!("grokhub-ctr-{}", std::process::id()));
        let ws = base.join("ws");
        let sdk = base.join("sdk");
        let comma = base.join("a,b");
        for dir in [&ws, &sdk, &comma] {
            std::fs::create_dir_all(dir).unwrap();
        }
        let ws = ws.canonicalize().unwrap();
        let sdk = sdk.canonicalize().unwrap();
        assert_eq!(
            mounts(&ws, &[sdk.clone(), comma, base.join("missing"), ws.clone()]),
            vec![ws, sdk]
        );
        let _ = std::fs::remove_dir_all(base);
    }
}
