//! Opt-in container backend (M3): with `backend = "container"` in
//! `shell_sandbox.toml`, each sandboxed command runs in a throwaway Podman or
//! Docker container instead of the host sandbox.
//!
//! - Files: only the workspace and the `writable` folders are mounted,
//!   read-write, at the same paths. Nothing else of the host is there, so the
//!   secret folders the host sandbox hides are simply missing.
//! - Network: `--network none`, so the container has only its own loopback.
//!   With the network guard on, the egress proxy's Unix socket folder is
//!   mounted and a small relay inside listens on `127.0.0.1:<proxy port>`,
//!   as `netns` does on the host: the proxy variables work and nothing else
//!   gets out (no DNS, no direct connections). The relay is Perl, which
//!   Debian-based images carry in their base; an image without it runs the
//!   command with no network and says so. A command that needs more asks to
//!   run with `outside_sandbox`, as on the host.
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

/// The image when `image` isn't set. It has bash, coreutils and nothing else.
pub const DEFAULT_IMAGE: &str = "docker.io/library/debian:stable-slim";

/// Every container this backend starts carries this label, so `ps --filter
/// label=grokhub.sandbox` lists them.
pub const LABEL: &str = "grokhub.sandbox=shell";

/// Runs `$0` with its stdin cut off and kills it once the container's own
/// stdin closes, then exits with its status. `--init` reaps what is left.
/// The stdin is kept on fd 3 first: `sh` gives a background job `/dev/null`
/// for stdin unless it is redirected.
/// With an egress proxy, `$1` and `$2` are its socket and port and `$3` is
/// [`RELAY`], started first (it returns once it listens).
const WATCHDOG: &str = "if [ -n \"$1\" ] && ! perl -e \"$3\" \"$1\" \"$2\" </dev/null 2>/dev/null; then \
echo '[sandbox] The network relay needs perl, which this image lacks, so this command has no network.' >&2; fi; \
exec 3<&0; bash --noprofile --norc -c \"$0\" </dev/null 3<&- & p=$!; \
(cat <&3 >/dev/null; kill -KILL $p 2>/dev/null) & exec 3<&-; wait $p";

/// Listens on the container's `127.0.0.1:<port>` and carries each
/// connection to the proxy's Unix socket; returns once it listens and keeps
/// serving in a child that ends with the container. Only the `Socket` module,
/// which `perl-base` has.
pub const RELAY: &str = "use Socket;my($s,$p)=@ARGV;\
socket(my $l,PF_INET,SOCK_STREAM,0)or die;setsockopt($l,SOL_SOCKET,SO_REUSEADDR,1);\
bind($l,sockaddr_in($p,inet_aton('127.0.0.1')))or die;listen($l,64)or die;\
$SIG{CHLD}='IGNORE';exit 0 if fork;\
while(1){accept(my $c,$l)or next;if(fork){close $c;next}close $l;\
socket(my $u,PF_UNIX,SOCK_STREAM,0)or exit;connect($u,sockaddr_un($s))or exit;\
if(fork){cp($c,$u);shutdown($u,1);exit}cp($u,$c);shutdown($c,1);exit}\
sub cp{my($a,$b)=@_;my $d;while((sysread($a,$d,65536)//0)>0){my $o=0;\
while($o<length $d){my $w=syswrite($b,$d,length($d)-$o,$o);return unless $w;$o+=$w}}}";

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
    /// Mounted read-write at the same path, from [`layout`]. The workspace
    /// comes first; the folders between a mount and a cover come last.
    pub mounts: Vec<PathBuf>,
    /// Secret folders and files inside a mount (see [`super::secrets`]),
    /// with the app's own config folder: covered inside, so the container
    /// sees what the host sandbox lets a command see. A folder shows as an
    /// empty scratch folder, a file as an empty one.
    pub hide: Vec<PathBuf>,
    /// The way out through the egress proxy, when the network guard is on.
    pub egress: Option<Egress>,
}

/// What a container needs to reach the egress proxy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Egress {
    /// The proxy's Unix socket; its folder is mounted at the same path.
    pub socket: PathBuf,
    /// The port the proxy variables name; the relay listens on it inside.
    pub port: u16,
    /// The proxy variables and the token stand-ins, as the host sandbox
    /// sets them.
    pub env: Vec<(String, String)>,
    /// Mounted read-only at the same path: the session CA bundle the
    /// variables point at.
    pub files: Vec<PathBuf>,
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
    static FOUND: std::sync::OnceLock<Option<Runtime>> = std::sync::OnceLock::new();
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

/// What the container mounts and covers, for the workspace, the extra
/// writable folders and the paths the host sandbox hides
/// ([`super::secrets::hidden`]), so it holds the same fence as the host:
///
/// - An extra folder that holds the config folder is left out, as
///   `writable_roots` leaves it out on the host: a command could rewrite the
///   app's rules, or leave code there that later runs outside the sandbox.
///   The workspace always stays (a helper's tree sits inside the config
///   folder).
/// - An extra folder that is a hidden path, or sits inside one, is left out:
///   the host hides it.
/// - A hidden path inside a mount is covered (the second list).
/// - Each folder between a mount and a covered path is mounted again on
///   itself: a mount point can't be renamed, so a cover can't be moved off
///   its path (`mv ~/.config ~/.c`) to leave the real folder uncovered.
/// - A hidden path inside another is already under that one's cover, so it
///   gets neither a cover nor a pin of its own: each path is mounted once.
/// - A mount whose covers or pins would need a path with a comma (which
///   `--mount` can't name) is left out.
pub fn layout(
    workspace: &Path,
    extra: &[PathBuf],
    hidden: &[PathBuf],
    config_dir: &Path,
) -> (Vec<PathBuf>, Vec<PathBuf>) {
    let ws = workspace.canonicalize().ok();
    let config = config_dir
        .canonicalize()
        .unwrap_or_else(|_| config_dir.to_path_buf());
    let mut keep: Vec<PathBuf> = Vec::new();
    let mut pins: Vec<PathBuf> = Vec::new();
    let mut hide: Vec<PathBuf> = Vec::new();
    for m in mounts(workspace, extra) {
        let is_ws = ws.as_ref() == Some(&m);
        if !is_ws && (config.starts_with(&m) || hidden.iter().any(|h| m.starts_with(h))) {
            continue;
        }
        let inside: Vec<&PathBuf> = hidden
            .iter()
            .filter(|h| h.starts_with(&m) && **h != m)
            .collect();
        let between: Vec<PathBuf> = inside
            .iter()
            .flat_map(|h| h.ancestors().skip(1))
            .filter(|a| a.starts_with(&m) && *a != m.as_path())
            .map(Path::to_path_buf)
            .collect();
        let named = |p: &PathBuf| !p.to_string_lossy().contains(',');
        if !inside.iter().all(|h| named(h)) || !between.iter().all(named) {
            continue;
        }
        for h in inside {
            if !hide.contains(h) {
                hide.push(h.clone());
            }
        }
        for a in between {
            if !pins.contains(&a) {
                pins.push(a);
            }
        }
        keep.push(m);
    }
    let outer = |p: &Path, hide: &[PathBuf]| !hide.iter().any(|h| p.starts_with(h) && p != h);
    let hide: Vec<PathBuf> = hide.iter().filter(|h| outer(h, &hide)).cloned().collect();
    for a in pins {
        let covered = hide.iter().any(|h| a.starts_with(h));
        if !covered && !keep.contains(&a) {
            keep.push(a);
        }
    }
    (keep, hide)
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
    for path in &c.hide {
        let cover = if path.is_dir() {
            format!("type=tmpfs,destination={}", path.display())
        } else {
            format!(
                "type=bind,source=/dev/null,target={},readonly",
                path.display()
            )
        };
        out.extend(["--mount".into(), cover]);
    }
    let relay = c.egress.as_ref().and_then(|e| {
        let folder = e.socket.parent()?.display().to_string();
        out.extend([
            "--mount".into(),
            format!("type=bind,source={folder},target={folder}"),
        ]);
        for file in &e.files {
            let file = file.display();
            out.extend([
                "--mount".into(),
                format!("type=bind,source={file},target={file},readonly"),
            ]);
        }
        for (key, value) in &e.env {
            out.extend(["--env".into(), format!("{key}={value}")]);
        }
        Some([
            e.socket.display().to_string(),
            e.port.to_string(),
            RELAY.into(),
        ])
    });
    out.extend([
        "--workdir".into(),
        cwd.display().to_string(),
        c.image.clone(),
        "sh".into(),
        "-c".into(),
        WATCHDOG.into(),
        command.into(),
    ]);
    out.extend(relay.into_iter().flatten());
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
            hide: Vec::new(),
            egress: None,
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
    fn with_the_proxy_the_socket_folder_ca_and_variables_go_in_and_the_relay_starts() {
        let mut c = docker(vec![PathBuf::from("/home/user/my-app")]);
        c.egress = Some(Egress {
            socket: PathBuf::from("/tmp/egress-7-0/s"),
            port: 4000,
            env: vec![
                ("HTTPS_PROXY".into(), "http://127.0.0.1:4000".into()),
                ("GH_TOKEN".into(), "ghp_stand-in".into()),
            ],
            files: vec![PathBuf::from("/tmp/grokhub-mitm/bundle.pem")],
        });
        let args = args(&c, Path::new("/home/user/my-app"), 1000, 1000, "npm ci");
        let at = args.iter().position(|a| a == "--workdir").unwrap();
        assert_eq!(
            args[at - 8..at],
            [
                "--mount",
                "type=bind,source=/tmp/egress-7-0,target=/tmp/egress-7-0",
                "--mount",
                "type=bind,source=/tmp/grokhub-mitm/bundle.pem,target=/tmp/grokhub-mitm/bundle.pem,readonly",
                "--env",
                "HTTPS_PROXY=http://127.0.0.1:4000",
                "--env",
                "GH_TOKEN=ghp_stand-in",
            ]
            .map(String::from)
        );
        assert_eq!(
            args[at + 3..],
            [
                "sh",
                "-c",
                WATCHDOG,
                "npm ci",
                "/tmp/egress-7-0/s",
                "4000",
                RELAY,
            ]
            .map(String::from)
        );
        // Still no network of its own: the relay is the only way out.
        assert!(args.windows(2).any(|w| w == ["--network", "none"]));
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

    /// A home with a workspace, two secret folders (one nested) and a file.
    fn scratch_home(label: &str) -> PathBuf {
        let home = std::env::temp_dir()
            .join(format!("grokhub-ctr-layout-{label}-{}", std::process::id()))
            .join("home");
        let _ = std::fs::remove_dir_all(home.parent().unwrap());
        for dir in [
            "my-app",
            ".ssh",
            ".config/gh",
            ".config/GrokHub/helper-work/k1",
            ".kube",
        ] {
            std::fs::create_dir_all(home.join(dir)).unwrap();
        }
        std::fs::write(home.join(".netrc"), "machine x").unwrap();
        home.canonicalize().unwrap()
    }

    #[test]
    fn a_home_mount_that_holds_the_config_folder_is_left_out_as_on_the_host() {
        let home = scratch_home("config-home");
        let config = home.join(".config/GrokHub");
        let ws = home.join("my-app");
        let hidden = crate::sandbox::secrets::hidden(Some(&home), &config, &ws);
        assert_eq!(
            layout(&ws, std::slice::from_ref(&home), &hidden, &config),
            (vec![ws.clone()], vec![])
        );
        // A helper works in a tree inside the config folder: its own tree
        // mounts, the home that holds the config folder still doesn't.
        let helper = config.join("helper-work/k1");
        let hidden = crate::sandbox::secrets::hidden(Some(&home), &config, &helper);
        assert_eq!(
            layout(&helper, std::slice::from_ref(&home), &hidden, &config),
            (vec![helper], vec![])
        );
        let _ = std::fs::remove_dir_all(home.parent().unwrap());
    }

    #[test]
    fn secrets_in_a_home_mount_are_covered_and_the_folders_above_them_pinned() {
        let home = scratch_home("covered");
        // The config folder lives elsewhere, so the home folder may mount.
        let config = home.parent().unwrap().join("cfg");
        std::fs::create_dir_all(&config).unwrap();
        let ws = home.join("my-app");
        let hidden = crate::sandbox::secrets::hidden(Some(&home), &config, &ws);
        let (mounts, hide) = layout(&ws, std::slice::from_ref(&home), &hidden, &config);
        // `.config` is mounted on itself, so `mv ~/.config ~/.c` fails and
        // the cover stays on `.config/gh`.
        assert_eq!(mounts, vec![ws.clone(), home.clone(), home.join(".config")]);
        assert_eq!(
            hide,
            vec![
                home.join(".ssh"),
                home.join(".kube"),
                home.join(".config/gh"),
                home.join(".netrc")
            ]
        );
        // A secret folder asked for as a writable folder stays hidden, and
        // gives no second mount at the same path.
        assert_eq!(
            layout(&ws, &[home.join(".kube")], &hidden, &config),
            (vec![ws.clone()], vec![])
        );
        let _ = std::fs::remove_dir_all(home.parent().unwrap());
    }

    #[test]
    fn a_mount_whose_cover_would_need_a_comma_is_left_out() {
        let base = std::env::temp_dir().join(format!("grokhub-ctr-comma-{}", std::process::id()));
        let odd = base.join("a,b/.ssh");
        std::fs::create_dir_all(&odd).unwrap();
        std::fs::create_dir_all(base.join("ws")).unwrap();
        let base = base.canonicalize().unwrap();
        let ws = base.join("ws");
        let hidden = [base.join("a,b/.ssh")];
        assert_eq!(
            layout(&ws, std::slice::from_ref(&base), &hidden, &base.join("cfg")),
            (vec![ws], vec![])
        );
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn a_hidden_path_inside_another_is_mounted_once_under_the_outer_cover() {
        let base = std::env::temp_dir().join(format!("grokhub-ctr-nest-{}", std::process::id()));
        let hub = base.join(".grok/hub");
        std::fs::create_dir_all(&hub).unwrap();
        let home = base.canonicalize().unwrap();
        // The workspace is home, and the config folder sits inside the
        // sign-in folder: both are hidden, so ~/.grok gets one cover and
        // no pin (Docker refuses two mounts on one path).
        let hidden = [home.join(".grok"), home.join(".grok/hub")];
        assert_eq!(
            layout(&home, &[], &hidden, &home.join(".grok/hub")),
            (vec![home.clone()], vec![home.join(".grok")])
        );
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn a_covered_folder_is_an_empty_scratch_folder_and_a_file_dev_null() {
        let base = std::env::temp_dir().join(format!("grokhub-ctr-hide-{}", std::process::id()));
        let ssh = base.join(".ssh");
        let netrc = base.join(".netrc");
        std::fs::create_dir_all(&ssh).unwrap();
        std::fs::write(&netrc, "machine x").unwrap();
        let mut c = docker(vec![base.clone()]);
        c.hide = vec![ssh.clone(), netrc.clone()];
        let args = args(&c, &base, 1000, 1000, "ls");
        let after = |flag: &str| {
            args.windows(2)
                .filter(|w| w[0] == flag)
                .map(|w| w[1].clone())
                .collect::<Vec<_>>()
        };
        assert_eq!(
            after("--mount"),
            [
                format!("type=bind,source={0},target={0}", base.display()),
                format!("type=tmpfs,destination={}", ssh.display()),
                format!(
                    "type=bind,source=/dev/null,target={},readonly",
                    netrc.display()
                ),
            ]
        );
        let _ = std::fs::remove_dir_all(base);
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
