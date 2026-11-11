//! `sudo` asks for your password once per session, through an OS-style
//! password dialog, never a GrokHub permission card (card 34).
//!
//! The cabin listens on a private socket. Shell commands run with no
//! terminal, `SUDO_ASKPASS` set to a small script that calls
//! `grokhub --askpass`, and a `sudo` first on `PATH` that is the real one
//! with `-A` (sudo only uses the helper when told to), so `sudo` asks it. The helper asks the cabin:
//! the first time, the cabin shows the dialog; after that it answers from
//! memory until GrokHub closes. A dialog closes when the command that asked is
//! stopped, and a password is remembered only once it went to a `sudo` that
//! was still there. The password is never written to disk and is
//! wiped from memory on drop. Only our own helper, started by a real `sudo`
//! (running as root) and answering into a pipe, gets an answer. Linux only;
//! elsewhere nothing changes.

use std::time::Duration;

use zeroize::Zeroizing;

/// The dialog's words.
pub static DIALOG_TEXT: std::sync::LazyLock<&'static str> = std::sync::LazyLock::new(|| {
    grokhub_core::brand::filled("{product} needs your password to run an admin command (sudo). It asks once and keeps it in memory until {product} closes.")
});

/// After you cancel the dialog, sudo steps fail at once for this long instead of asking again.
pub const DECLINE_HOLD: Duration = Duration::from_secs(300);

/// What the cabin remembers for the session.
#[derive(Default)]
pub struct Keeper {
    password: Option<Zeroizing<String>>,
    /// The `sudo` process that got the last answer.
    last_sudo: Option<u32>,
    declined_at: Option<std::time::Instant>,
}

impl Keeper {
    /// Answer `sudo` process `sudo_pid`. The same `sudo` asking again means the
    /// remembered password was wrong, so it asks you again. `dialog` shows the
    /// OS dialog and returns `None` when you cancel. What you type is not
    /// remembered here: [`delivered`](Self::delivered) does that, once the
    /// answer reached the `sudo` that asked.
    pub fn answer(
        &mut self,
        sudo_pid: u32,
        dialog: &mut dyn FnMut() -> Option<Zeroizing<String>>,
    ) -> Option<Zeroizing<String>> {
        if self.last_sudo == Some(sudo_pid) {
            self.password = None;
        }
        self.last_sudo = Some(sudo_pid);
        if let Some(pw) = &self.password {
            return Some(pw.clone());
        }
        if self
            .declined_at
            .is_some_and(|at| at.elapsed() < DECLINE_HOLD)
        {
            return None;
        }
        match dialog().filter(|pw| !pw.is_empty()) {
            Some(pw) => {
                self.declined_at = None;
                Some(pw)
            }
            None => {
                self.declined_at = Some(std::time::Instant::now());
                None
            }
        }
    }

    /// The answer went out to `sudo_pid`'s helper: remember it for the next
    /// `sudo`. A password typed for a command that was stopped meanwhile is
    /// never passed here, so it is not kept.
    pub fn delivered(&mut self, sudo_pid: u32, password: Zeroizing<String>) {
        if self.last_sudo == Some(sudo_pid) {
            self.password = Some(password);
        }
    }

    /// The dialog was closed because the command that asked is gone. Nobody
    /// said no, so the next `sudo` asks again instead of failing.
    pub fn asker_left(&mut self) {
        self.declined_at = None;
    }

    pub fn remembers(&self) -> bool {
        self.password.is_some()
    }
}

fn sh_quote(p: &std::path::Path) -> String {
    format!("'{}'", p.to_string_lossy().replace('\'', r"'\''"))
}

/// The helper script: `exec '<exe>' --askpass '<socket>' "$@"`.
pub fn askpass_script(exe: &std::path::Path, socket: &std::path::Path) -> String {
    format!(
        "#!/bin/sh\nexec {} --askpass {} \"$@\"\n",
        sh_quote(exe),
        sh_quote(socket)
    )
}

/// The `sudo` agent commands find first: the real one, told to use the helper.
pub fn sudo_script(real: &std::path::Path) -> String {
    format!("#!/bin/sh\nexec {} -A \"$@\"\n", sh_quote(real))
}

/// The dialog program and its args, best first for this desktop.
pub fn dialog_argv(kde: bool, has: &dyn Fn(&str) -> bool) -> Option<(String, Vec<String>)> {
    let kdialog = (
        "kdialog",
        vec![
            "--title".into(),
            grokhub_core::brand::product_name().to_string(),
            "--password".into(),
            (*DIALOG_TEXT).into(),
        ],
    );
    let entry = |bin: &'static str| {
        (
            bin,
            vec![
                "--entry".into(),
                "--hide-text".into(),
                format!(
                    "--title={product}",
                    product = grokhub_core::brand::product_name()
                ),
                format!("--text={}", *DIALOG_TEXT),
            ],
        )
    };
    let order: Vec<(&str, Vec<String>)> = if kde {
        vec![kdialog, entry("zenity"), entry("qarma"), entry("yad")]
    } else {
        vec![entry("zenity"), kdialog, entry("qarma"), entry("yad")]
    };
    order
        .into_iter()
        .find(|(bin, _)| has(bin))
        .map(|(bin, args)| (bin.to_string(), args))
}

/// Words an agent command may not use: the helper's env var, its flag and
/// its directory. Only `sudo` itself runs the helper; a command that names it
/// is reaching for the password, so it is refused before it runs.
const HELPER_MARKERS: &[&str] = &["SUDO_ASKPASS", "--askpass", "grokhub-askpass-"];

/// What an agent command that names the sudo helper gets instead of running.
pub static HELPER_REFUSAL: std::sync::LazyLock<&'static str> = std::sync::LazyLock::new(|| {
    grokhub_core::brand::filled("This command reaches {product}'s sudo password helper directly. Run `sudo <command>` and {product} asks for the password itself.")
});

/// True when an agent command names the sudo helper (see [`HELPER_MARKERS`]).
pub fn names_helper(command: &str) -> bool {
    let upper = command.to_ascii_uppercase();
    HELPER_MARKERS
        .iter()
        .any(|m| upper.contains(&m.to_ascii_uppercase()))
}

#[cfg(target_os = "linux")]
pub use linux::{client, install, shell_env};

#[cfg(not(target_os = "linux"))]
pub fn install(_client_exe: std::path::PathBuf) -> std::io::Result<()> {
    Ok(())
}

#[cfg(not(target_os = "linux"))]
pub fn shell_env() -> Vec<(String, String)> {
    Vec::new()
}

#[cfg(not(target_os = "linux"))]
pub fn client(_socket: &str) -> i32 {
    1
}

#[cfg(target_os = "linux")]
mod linux {
    use std::io::{BufRead, BufReader, Read, Write};
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    use std::os::unix::io::AsRawFd;
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::path::{Path, PathBuf};
    use std::sync::{Mutex, OnceLock};

    use zeroize::Zeroizing;

    use super::{askpass_script, dialog_argv, Keeper};

    struct Server {
        dir: PathBuf,
        script: PathBuf,
        /// A `sudo` wrapper was written (a real `sudo` is on `PATH`).
        wrapped: bool,
    }

    static SERVER: OnceLock<Server> = OnceLock::new();

    /// Start answering `sudo` for this session. `client_exe` is the program
    /// the helper script runs (`grokhub` itself). Once per process.
    pub fn install(client_exe: PathBuf) -> std::io::Result<()> {
        if SERVER.get().is_some() {
            return Ok(());
        }
        let config_dir = crate::perm::config_dir();
        let dir = helper_dir(
            std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from),
            &shared_roots(&config_dir),
            &config_dir,
        )?;
        let socket = dir.join("s");
        let script = dir.join("askpass");
        let listener = bind_socket(&socket)?;
        std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600))?;
        std::fs::write(&script, askpass_script(&client_exe, &socket))?;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700))?;
        let real = real_sudo(&dir);
        if let Some(real) = &real {
            let wrapper = dir.join("sudo");
            std::fs::write(&wrapper, super::sudo_script(real))?;
            std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o700))?;
        }
        let exe = std::fs::canonicalize(&client_exe).unwrap_or(client_exe);
        let keeper = Mutex::new(Keeper::default());
        std::thread::Builder::new()
            .name("sudo-askpass".into())
            .spawn(move || {
                for stream in listener.incoming().flatten() {
                    serve(stream, &exe, &keeper);
                }
            })?;
        let _ = SERVER.set(Server {
            dir,
            script,
            wrapped: real.is_some(),
        });
        Ok(())
    }

    /// The folders every sandboxed command may write, whatever its workspace:
    /// the sandbox's own list, asked with no workspace and no home.
    pub(super) fn shared_roots(config_dir: &Path) -> Vec<PathBuf> {
        crate::sandbox::writable_roots(Path::new(""), None, config_dir, &[])
    }

    /// `XDG_RUNTIME_DIR` when the helper may live there: a folder that is
    /// this user's alone, as a login session makes it, and outside `shared`.
    /// A session without a login manager often exports one under the temp
    /// folder, and after `su` the variable can still name the other user's.
    pub(super) fn runtime_dir(xdg: Option<PathBuf>, shared: &[PathBuf]) -> Option<PathBuf> {
        let dir = xdg?.canonicalize().ok()?;
        let meta = std::fs::metadata(&dir).ok()?;
        // SAFETY: geteuid has no preconditions.
        let me = unsafe { libc::geteuid() };
        let ours = meta.is_dir() && private_to(meta.uid(), meta.mode(), me);
        (ours && !shared.iter().any(|root| dir.starts_with(root))).then_some(dir)
    }

    /// Owned by `me`, with nothing allowed to the group or to others.
    pub(super) fn private_to(owner: u32, mode: u32, me: u32) -> bool {
        owner == me && mode & 0o077 == 0
    }

    /// This process's helper dir, made new and private: in the runtime folder
    /// when [`runtime_dir`] takes it and the dir can be made there, else in
    /// `run` in the config folder.
    pub(super) fn helper_dir(
        xdg: Option<PathBuf>,
        shared: &[PathBuf],
        config_dir: &Path,
    ) -> std::io::Result<PathBuf> {
        let run = config_dir.join("run");
        let base = helper_base(runtime_dir(xdg, shared), config_dir);
        match make_helper_dir(&base) {
            Err(_) if base != run => make_helper_dir(&run),
            made => made,
        }
    }

    fn make_helper_dir(base: &Path) -> std::io::Result<PathBuf> {
        if !base.is_dir() {
            std::fs::create_dir_all(base)?;
            std::fs::set_permissions(base, std::fs::Permissions::from_mode(0o700))?;
        }
        sweep(base);
        let dir = base.join(format!("grokhub-askpass-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir(&dir)?;
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))?;
        Ok(dir)
    }

    /// The folder the helper dir is made in: the runtime folder when there is
    /// one, else `run` in the config folder. Never a temp folder: sandboxed
    /// agent commands may write those, and one that rewrote the `sudo` wrapper
    /// would decide what the next approved `sudo` runs. The config folder is
    /// read-only to them. [`runtime_dir`] is what keeps a runtime folder they
    /// may write from getting here.
    pub(super) fn helper_base(xdg: Option<PathBuf>, config_dir: &Path) -> PathBuf {
        xdg.filter(|p| p.is_dir())
            .unwrap_or_else(|| config_dir.join("run"))
    }

    /// The longest path a socket address holds.
    const SOCKET_PATH_MAX: usize = 107;

    /// `socket` as a path that fits a socket address. A config folder under a
    /// long home path makes the plain one too long; then its folder is opened
    /// and the socket is named through `/proc/self/fd`. Keep the returned
    /// folder handle until the bind or connect is done.
    fn socket_addr(socket: &Path) -> std::io::Result<(Option<std::fs::File>, PathBuf)> {
        if socket.as_os_str().len() <= SOCKET_PATH_MAX {
            return Ok((None, socket.to_path_buf()));
        }
        let (Some(dir), Some(name)) = (socket.parent(), socket.file_name()) else {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "the sudo helper socket has no folder",
            ));
        };
        let dir = std::fs::File::open(dir)?;
        let short = PathBuf::from(format!("/proc/self/fd/{}", dir.as_raw_fd())).join(name);
        Ok((Some(dir), short))
    }

    pub(super) fn bind_socket(socket: &Path) -> std::io::Result<UnixListener> {
        let (_dir, addr) = socket_addr(socket)?;
        UnixListener::bind(addr)
    }

    pub(super) fn connect_socket(socket: &Path) -> std::io::Result<UnixStream> {
        let (_dir, addr) = socket_addr(socket)?;
        UnixStream::connect(addr)
    }

    /// The first `sudo` on `PATH` outside `ours`.
    pub(super) fn real_sudo(ours: &Path) -> Option<PathBuf> {
        let path = std::env::var_os("PATH")?;
        std::env::split_paths(&path)
            .filter(|d| d != ours)
            .map(|d| d.join("sudo"))
            .find(|p| p.is_file())
    }

    /// Env for a shell command once [`install`] ran: `SUDO_ASKPASS`, and
    /// `PATH` with the `sudo` wrapper first.
    pub fn shell_env() -> Vec<(String, String)> {
        let Some(s) = SERVER.get() else {
            return Vec::new();
        };
        let mut env = vec![(
            "SUDO_ASKPASS".to_string(),
            s.script.to_string_lossy().into_owned(),
        )];
        if s.wrapped {
            let mut dirs = vec![s.dir.clone()];
            dirs.extend(
                std::env::var_os("PATH")
                    .map(|p| std::env::split_paths(&p).collect::<Vec<_>>())
                    .unwrap_or_default(),
            );
            if let Ok(path) = std::env::join_paths(dirs) {
                env.push(("PATH".to_string(), path.to_string_lossy().into_owned()));
            }
        }
        env
    }

    /// `grokhub --askpass <socket>`: print the password for `sudo`, or exit 1.
    pub fn client(socket: &str) -> i32 {
        let Ok(mut stream) = connect_socket(Path::new(socket)) else {
            return 1;
        };
        let mut reply = Zeroizing::new(Vec::new());
        if stream.write_all(b"ask\n").is_err() || stream.read_to_end(&mut reply).is_err() {
            return 1;
        }
        let Some(pw) = reply.strip_prefix(b"ok\n") else {
            return 1;
        };
        let mut out = std::io::stdout().lock();
        if out
            .write_all(pw)
            .and_then(|_| out.write_all(b"\n"))
            .and_then(|_| out.flush())
            .is_err()
        {
            return 1;
        }
        0
    }

    /// Remove helper dirs left by GrokHub processes that are gone.
    fn sweep(base: &Path) {
        let Ok(entries) = std::fs::read_dir(base) else {
            return;
        };
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            let Some(pid) = name
                .strip_prefix("grokhub-askpass-")
                .and_then(|p| p.parse::<u32>().ok())
            else {
                continue;
            };
            if !Path::new(&format!("/proc/{pid}")).exists() {
                let _ = std::fs::remove_dir_all(e.path());
            }
        }
    }

    fn serve(mut stream: UnixStream, exe: &Path, keeper: &Mutex<Keeper>) {
        let mut line = String::new();
        if BufReader::new(&stream).read_line(&mut line).is_err() || line.trim() != "ask" {
            return;
        }
        match peer_pid(&stream).and_then(|pid| sudo_parent(pid, exe)) {
            Some(sudo) => answer_sudo(&stream, sudo, keeper, &mut show_dialog),
            None => {
                let _ = stream.write_all(b"no\n");
            }
        }
    }

    /// Answers the helper of `sudo` on `stream`. `dialog` shows the OS dialog
    /// for that helper and sets its flag when it closed the dialog because the
    /// helper went away. The password is remembered only when the answer went
    /// out to a helper that was still there.
    pub(super) fn answer_sudo(
        mut stream: &UnixStream,
        sudo: u32,
        keeper: &Mutex<Keeper>,
        dialog: &mut dyn FnMut(&UnixStream, &mut bool) -> Option<Zeroizing<String>>,
    ) {
        let mut k = keeper.lock().unwrap_or_else(|e| e.into_inner());
        let mut gone = false;
        let answer = k.answer(sudo, &mut || dialog(stream, &mut gone));
        if gone {
            k.asker_left();
        }
        match answer {
            Some(pw) => {
                let mut msg = Zeroizing::new(b"ok\n".to_vec());
                msg.extend_from_slice(pw.as_bytes());
                if stream.write_all(&msg).is_ok() {
                    k.delivered(sudo, pw);
                }
            }
            None => {
                let _ = stream.write_all(b"no\n");
            }
        }
    }

    fn peer_pid(stream: &UnixStream) -> Option<u32> {
        let mut cred = libc::ucred {
            pid: 0,
            uid: 0,
            gid: 0,
        };
        let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
        // SAFETY: `cred` and `len` are valid for writes of the sizes passed.
        let rc = unsafe {
            libc::getsockopt(
                stream.as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_PEERCRED,
                (&mut cred as *mut libc::ucred).cast(),
                &mut len,
            )
        };
        // SAFETY: getuid has no preconditions.
        let me = unsafe { libc::getuid() };
        (rc == 0 && cred.pid > 0 && cred.uid == me).then_some(cred.pid as u32)
    }

    /// The `sudo` pid when `pid` is our own helper, its parent is a `sudo`
    /// running as root, and its answer goes into a pipe (sudo reads it).
    /// A program you or the model start by hand gets nothing.
    pub(super) fn sudo_parent(pid: u32, exe: &Path) -> Option<u32> {
        let own = std::fs::read_link(format!("/proc/{pid}/exe")).ok()?;
        if own != exe {
            return None;
        }
        let out = std::fs::read_link(format!("/proc/{pid}/fd/1"))
            .ok()?
            .to_string_lossy()
            .into_owned();
        if !out.starts_with("pipe:") {
            return None;
        }
        let ppid = proc_ppid(pid)?;
        let comm = std::fs::read_to_string(format!("/proc/{ppid}/comm")).ok()?;
        if comm.trim() != "sudo" || proc_euid(ppid)? != 0 {
            return None;
        }
        // sudo's own prompt reads the helper through a pipe only that root
        // `sudo` holds. `sudo -u "$USER" "$SUDO_ASKPASS"` instead hands the
        // helper sudo's stdout, which a process of ours reads (GrokHub's tool
        // output, `cat`, ...): any such reader means no answer.
        (!pipe_held_elsewhere(pid, &out)?).then_some(ppid)
    }

    /// Whether a process of this user other than `pid` holds `pipe` (a
    /// `pipe:[inode]` link). `None` when `/proc` can't be read: fail closed.
    pub(super) fn pipe_held_elsewhere(pid: u32, pipe: &str) -> Option<bool> {
        // SAFETY: getuid has no preconditions.
        let me = unsafe { libc::getuid() };
        for entry in std::fs::read_dir("/proc").ok()?.flatten() {
            let Some(other) = entry
                .file_name()
                .to_str()
                .and_then(|n| n.parse::<u32>().ok())
            else {
                continue;
            };
            if other == pid || proc_uid(other) != Some(me) {
                continue;
            }
            let Ok(fds) = std::fs::read_dir(format!("/proc/{other}/fd")) else {
                continue;
            };
            if fds
                .flatten()
                .any(|fd| std::fs::read_link(fd.path()).is_ok_and(|l| l.to_string_lossy() == pipe))
            {
                return Some(true);
            }
        }
        Some(false)
    }

    fn proc_uid(pid: u32) -> Option<u32> {
        let status = std::fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
        status
            .lines()
            .find_map(|l| l.strip_prefix("Uid:"))?
            .split_whitespace()
            .next()?
            .parse()
            .ok()
    }

    fn proc_ppid(pid: u32) -> Option<u32> {
        let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
        // `pid (comm) state ppid …`; comm may hold spaces or parens.
        stat.rsplit_once(')')?
            .1
            .split_whitespace()
            .nth(1)?
            .parse()
            .ok()
    }

    fn proc_euid(pid: u32) -> Option<u32> {
        let status = std::fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
        status
            .lines()
            .find_map(|l| l.strip_prefix("Uid:"))?
            .split_whitespace()
            .nth(1)?
            .parse()
            .ok()
    }

    fn on_path(bin: &str) -> bool {
        std::env::var_os("PATH")
            .is_some_and(|p| std::env::split_paths(&p).any(|d| d.join(bin).is_file()))
    }

    fn show_dialog(asker: &UnixStream, gone: &mut bool) -> Option<Zeroizing<String>> {
        let kde = std::env::var("XDG_CURRENT_DESKTOP")
            .is_ok_and(|d| d.to_ascii_uppercase().contains("KDE"));
        let (bin, args) = dialog_argv(kde, &on_path)?;
        run_dialog(std::process::Command::new(bin).args(args), asker, gone)
    }

    /// How often an open dialog checks that its asker is still there.
    const ASKER_CHECK: std::time::Duration = std::time::Duration::from_millis(200);

    /// Runs the dialog program `cmd` for the helper on `asker` and returns
    /// what was typed, or `None` on cancel. A dialog must not outlive the
    /// command that asked: when the helper goes away first (its command was
    /// stopped or timed out), the dialog is closed, `gone` is set and nothing
    /// is returned.
    pub(super) fn run_dialog(
        cmd: &mut std::process::Command,
        asker: &UnixStream,
        gone: &mut bool,
    ) -> Option<Zeroizing<String>> {
        use std::process::Stdio;
        let mut child = cmd
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;
        let mut pipe = child.stdout.take()?;
        let typed = std::thread::spawn(move || {
            let mut raw = Zeroizing::new(Vec::new());
            let _ = pipe.read_to_end(&mut raw);
            raw
        });
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break Some(status),
                Ok(None) if hung_up(asker, ASKER_CHECK) => {
                    *gone = true;
                    break None;
                }
                Ok(None) => {}
                Err(_) => break None,
            }
        };
        let Some(status) = status else {
            let _ = child.kill();
            let _ = child.wait();
            return None;
        };
        let raw = typed.join().ok()?;
        if !status.success() {
            return None;
        }
        let text = Zeroizing::new(String::from_utf8_lossy(&raw).into_owned());
        Some(Zeroizing::new(
            text.trim_end_matches(['\n', '\r']).to_string(),
        ))
    }

    /// Waits up to `wait` for the process on the other end of `stream` to go
    /// away. True when it has.
    fn hung_up(stream: &UnixStream, wait: std::time::Duration) -> bool {
        let mut fd = libc::pollfd {
            fd: stream.as_raw_fd(),
            events: libc::POLLRDHUP,
            revents: 0,
        };
        // SAFETY: `fd` is one valid pollfd, and the count passed is 1.
        let rc = unsafe { libc::poll(&mut fd, 1, wait.as_millis() as libc::c_int) };
        if rc < 0 {
            // Interrupted: wait it out here so the caller's loop never spins.
            std::thread::sleep(wait);
            return false;
        }
        fd.revents & (libc::POLLRDHUP | libc::POLLHUP | libc::POLLERR | libc::POLLNVAL) != 0
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn only_our_helper_under_a_root_sudo_is_answered() {
            let me = std::process::id();
            let exe = std::fs::read_link(format!("/proc/{me}/exe")).unwrap();
            // The test runner is our exe, but its parent is not a root sudo.
            assert_eq!(sudo_parent(me, &exe), None);
            assert_eq!(sudo_parent(me, Path::new("/usr/bin/grokhub")), None);
            assert_eq!(proc_ppid(me), Some(std::os::unix::process::parent_id()));
            // SAFETY: geteuid has no preconditions.
            assert_eq!(proc_euid(me), Some(unsafe { libc::geteuid() }));
        }

        #[test]
        fn a_pipe_that_grokhub_or_another_program_reads_gets_no_password() {
            // `sudo -u "$USER" "$SUDO_ASKPASS"` in an agent command: the helper's
            // stdout is the pipe Loadout reads for tool output. Here a child
            // writes into a pipe this test process reads, as Loadout would.
            let mut child = std::process::Command::new("sleep")
                .arg("5")
                .stdout(std::process::Stdio::piped())
                .spawn()
                .unwrap();
            let pid = child.id();
            let link = std::fs::read_link(format!("/proc/{pid}/fd/1"))
                .unwrap()
                .to_string_lossy()
                .into_owned();
            assert!(link.starts_with("pipe:"), "{link}");
            assert_eq!(pipe_held_elsewhere(pid, &link), Some(true));
            // `... | cat`: another program of ours reads it.
            let mut cat = std::process::Command::new("cat")
                .stdin(child.stdout.take().unwrap())
                .stdout(std::process::Stdio::null())
                .spawn()
                .unwrap();
            assert_eq!(pipe_held_elsewhere(pid, &link), Some(true));
            let exe = std::fs::read_link(format!("/proc/{pid}/exe")).unwrap();
            assert_eq!(sudo_parent(pid, &exe), None);
            // A pipe no process of ours holds (sudo's own, read by root) passes.
            assert_eq!(pipe_held_elsewhere(pid, "pipe:[0]"), Some(false));
            let _ = child.kill();
            let _ = cat.kill();
            let _ = child.wait();
            let _ = cat.wait();
        }

        fn scratch(name: &str) -> PathBuf {
            let dir =
                std::env::temp_dir().join(format!("grokhub-sudo-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            dir
        }

        fn sh(script: &str) -> std::process::Command {
            let mut cmd = std::process::Command::new("sh");
            cmd.arg("-c").arg(script);
            cmd
        }

        fn hunter() -> Option<Zeroizing<String>> {
            Some(Zeroizing::new("hunter22".to_string()))
        }

        /// What the helper end of a socket got, once the server end is closed.
        fn got(asker: UnixStream, mut helper: UnixStream) -> String {
            drop(asker);
            let mut reply = String::new();
            helper.read_to_string(&mut reply).unwrap();
            reply
        }

        #[test]
        fn a_dialog_closes_when_the_command_that_asked_is_gone() {
            let dir = scratch("gone");
            let pidfile = dir.join("pid");
            let (asker, helper) = UnixStream::pair().unwrap();
            // The dialog is on screen, then the command that asked is stopped:
            // its helper dies and its end of the socket closes.
            let seen = pidfile.clone();
            let stop = std::thread::spawn(move || {
                while std::fs::read_to_string(&seen).map_or(true, |t| !t.ends_with('\n')) {
                    std::thread::sleep(std::time::Duration::from_millis(20));
                }
                drop(helper);
            });
            let mut gone = false;
            let started = std::time::Instant::now();
            let typed = run_dialog(
                &mut sh(&format!("echo $$ > '{}'; exec sleep 20", pidfile.display())),
                &asker,
                &mut gone,
            );
            stop.join().unwrap();
            assert_eq!(typed, None);
            assert!(gone, "closed for a gone asker, not cancelled by the person");
            assert!(
                started.elapsed() < std::time::Duration::from_secs(10),
                "{:?}",
                started.elapsed()
            );
            let pid = std::fs::read_to_string(&pidfile).unwrap();
            assert!(
                !Path::new(&format!("/proc/{}", pid.trim())).exists(),
                "the dialog program is ended and reaped"
            );
            let _ = std::fs::remove_dir_all(dir);
        }

        #[test]
        fn a_dialog_gives_what_was_typed_and_nothing_on_cancel() {
            let (asker, _helper) = UnixStream::pair().unwrap();
            let mut gone = false;
            assert_eq!(
                run_dialog(&mut sh("printf 'hunter22\\n'"), &asker, &mut gone),
                hunter()
            );
            assert_eq!(
                run_dialog(&mut sh("printf 'hunter22\\n'; exit 1"), &asker, &mut gone),
                None
            );
            assert_eq!(
                run_dialog(
                    &mut std::process::Command::new("/nonexistent/grokhub-dialog"),
                    &asker,
                    &mut gone
                ),
                None
            );
            assert!(!gone, "the asker was there all along");
        }

        #[test]
        fn a_password_is_remembered_only_once_it_went_to_a_live_helper() {
            let keeper = Mutex::new(Keeper::default());
            let mut shown = 0;
            let mut dialog = |_: &UnixStream, _: &mut bool| {
                shown += 1;
                hunter()
            };
            // sudo 11's command was stopped before the answer could go out.
            let (asker, helper) = UnixStream::pair().unwrap();
            drop(helper);
            answer_sudo(&asker, 11, &keeper, &mut dialog);
            assert!(!keeper.lock().unwrap().remembers());
            // sudo 12 is there: it gets the password, and now it is remembered.
            let (asker, helper) = UnixStream::pair().unwrap();
            answer_sudo(&asker, 12, &keeper, &mut dialog);
            assert_eq!(got(asker, helper), "ok\nhunter22");
            assert!(keeper.lock().unwrap().remembers());
            let (asker, helper) = UnixStream::pair().unwrap();
            answer_sudo(&asker, 13, &keeper, &mut dialog);
            assert_eq!(got(asker, helper), "ok\nhunter22");
            assert_eq!(shown, 2, "sudo 11 and 12 asked; 13 came from memory");
        }

        #[test]
        fn a_dialog_closed_for_a_gone_command_is_not_a_no() {
            let keeper = Mutex::new(Keeper::default());
            let mut shown = 0;
            let mut dialog = |_: &UnixStream, gone: &mut bool| {
                shown += 1;
                if shown == 1 {
                    *gone = true;
                    return None;
                }
                hunter()
            };
            let (asker, helper) = UnixStream::pair().unwrap();
            drop(helper);
            answer_sudo(&asker, 21, &keeper, &mut dialog);
            // Nobody said no, so the next sudo asks instead of failing for
            // five minutes.
            let (asker, helper) = UnixStream::pair().unwrap();
            answer_sudo(&asker, 22, &keeper, &mut dialog);
            assert_eq!(got(asker, helper), "ok\nhunter22");
            assert_eq!(shown, 2);
            // A real cancel still holds off.
            let keeper = Mutex::new(Keeper::default());
            let mut shown = 0;
            let mut cancel = |_: &UnixStream, _: &mut bool| {
                shown += 1;
                None
            };
            for sudo in [31, 32] {
                let (asker, helper) = UnixStream::pair().unwrap();
                answer_sudo(&asker, sudo, &keeper, &mut cancel);
                assert_eq!(got(asker, helper), "no\n");
            }
            assert_eq!(shown, 1);
        }

        #[test]
        fn the_helper_folder_is_never_one_that_sandboxed_commands_may_write() {
            let cfg = Path::new("/home/user/.config/GrokHub");
            assert_eq!(
                helper_base(None, cfg),
                PathBuf::from("/home/user/.config/GrokHub/run")
            );
            assert_eq!(
                helper_base(Some(PathBuf::from("/nonexistent/run/user/1000")), cfg),
                PathBuf::from("/home/user/.config/GrokHub/run"),
                "a runtime folder that is not there is not used"
            );
            let ws = scratch("roots");
            assert_eq!(helper_base(Some(ws.clone()), cfg), ws);
            let roots = crate::sandbox::writable_roots(&ws, None, cfg, &[]);
            let tmp = std::env::temp_dir().canonicalize().unwrap();
            assert!(
                roots.iter().any(|r| tmp.starts_with(r)),
                "sandboxed commands may write the temp folder: {roots:?}"
            );
            let base = helper_base(None, cfg);
            assert!(
                !roots.iter().any(|r| base.starts_with(r)),
                "{} is inside one of {roots:?}",
                base.display()
            );
            let _ = std::fs::remove_dir_all(ws);
        }

        #[test]
        fn a_runtime_folder_sandboxed_commands_may_write_is_not_used() {
            // A session without a login manager often exports a runtime
            // folder under the temp folder.
            let xdg = scratch("xdg-tmp");
            std::fs::set_permissions(&xdg, std::fs::Permissions::from_mode(0o700)).unwrap();
            let cfg = scratch("xdg-tmp-cfg");
            let tmp = std::env::temp_dir().canonicalize().unwrap();
            let name = format!("grokhub-askpass-{}", std::process::id());
            assert_eq!(
                helper_dir(Some(xdg.clone()), std::slice::from_ref(&tmp), &cfg).unwrap(),
                cfg.join("run").join(&name)
            );
            assert!(!xdg.join(&name).exists());
            assert_eq!(
                std::fs::metadata(cfg.join("run").join(&name))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o700
            );
            // The temp folder is one of the folders the sandbox opens.
            let roots = shared_roots(Path::new("/home/user/.config/GrokHub"));
            assert!(roots.iter().any(|r| tmp.starts_with(r)), "{roots:?}");
            assert_eq!(runtime_dir(Some(xdg.clone()), &roots), None);
            let _ = std::fs::remove_dir_all(xdg);
            let _ = std::fs::remove_dir_all(cfg);
        }

        #[test]
        fn a_runtime_folder_is_used_only_when_it_is_this_users_alone() {
            let xdg = scratch("xdg-own");
            let real = xdg.canonicalize().unwrap();
            let mode = |m| std::fs::set_permissions(&xdg, std::fs::Permissions::from_mode(m));
            mode(0o700).unwrap();
            assert_eq!(runtime_dir(Some(xdg.clone()), &[]), Some(real.clone()));
            // Open to the group or to others, it is not a runtime folder.
            mode(0o750).unwrap();
            assert_eq!(runtime_dir(Some(xdg.clone()), &[]), None);
            mode(0o701).unwrap();
            assert_eq!(runtime_dir(Some(xdg.clone()), &[]), None);
            // Another user's (after `su`, the variable can still name it).
            assert!(private_to(1000, 0o40700, 1000));
            assert!(!private_to(1001, 0o40700, 1000));
            assert!(!private_to(0, 0o40700, 1000));
            assert!(!private_to(1000, 0o40770, 1000));
            // Not there, not a folder, not set.
            assert_eq!(
                runtime_dir(Some(PathBuf::from("/nonexistent/run/user/1000")), &[]),
                None
            );
            mode(0o700).unwrap();
            std::fs::write(xdg.join("file"), "").unwrap();
            std::fs::set_permissions(xdg.join("file"), std::fs::Permissions::from_mode(0o600))
                .unwrap();
            assert_eq!(runtime_dir(Some(xdg.join("file")), &[]), None);
            assert_eq!(runtime_dir(None, &[]), None);
            // Used: the helper dir is made in it.
            let cfg = scratch("xdg-own-cfg");
            let name = format!("grokhub-askpass-{}", std::process::id());
            assert_eq!(
                helper_dir(Some(xdg.clone()), &[], &cfg).unwrap(),
                real.join(&name)
            );
            assert!(!cfg.join("run").exists());
            let _ = std::fs::remove_dir_all(xdg);
            let _ = std::fs::remove_dir_all(cfg);
        }

        #[test]
        fn a_runtime_folder_that_cannot_hold_the_helper_gives_way_to_the_config_folder() {
            // This process's own, private, and nothing can be made in it.
            let xdg = PathBuf::from("/proc/self/fd");
            assert_eq!(
                runtime_dir(Some(xdg.clone()), &[]),
                Some(PathBuf::from(format!("/proc/{}/fd", std::process::id())))
            );
            let cfg = scratch("xdg-full-cfg");
            assert_eq!(
                helper_dir(Some(xdg), &[], &cfg).unwrap(),
                cfg.join("run")
                    .join(format!("grokhub-askpass-{}", std::process::id()))
            );
            // With nowhere else to go the error comes back.
            let file = cfg.join("not-a-folder");
            std::fs::write(&file, "").unwrap();
            assert!(helper_dir(None, &[], &file).is_err());
            let _ = std::fs::remove_dir_all(cfg);
        }

        #[test]
        fn a_socket_in_a_deep_folder_still_binds_and_connects() {
            let deep = scratch("deep").join("d".repeat(120));
            std::fs::create_dir_all(&deep).unwrap();
            let socket = deep.join("s");
            assert!(socket.as_os_str().len() > 107, "{}", socket.display());
            assert!(
                UnixListener::bind(&socket).is_err(),
                "the plain path is too long for a socket address"
            );
            let listener = bind_socket(&socket).unwrap();
            assert!(socket.exists(), "the socket is where the script says");
            let mut helper = connect_socket(&socket).unwrap();
            helper.write_all(b"ask\n").unwrap();
            drop(helper);
            let mut line = String::new();
            listener
                .accept()
                .unwrap()
                .0
                .read_to_string(&mut line)
                .unwrap();
            assert_eq!(line, "ask\n");
            let _ = std::fs::remove_dir_all(deep.parent().unwrap());
        }

        #[test]
        fn the_helper_dir_holds_a_socket_and_the_script_and_no_password() {
            // Without a runtime folder the helper dir is made in the config
            // folder: a scratch one here, never the real one.
            let cfg = scratch("install");
            let _cfg = crate::perm::ConfigGuard::set(&cfg);
            let exe = PathBuf::from("/opt/Grok Hub/grokhub");
            install(exe.clone()).unwrap();
            let env = shell_env();
            assert_eq!(env[0].0, "SUDO_ASKPASS");
            let script = PathBuf::from(&env[0].1);
            let dir = script.parent().unwrap();
            let real = real_sudo(dir);
            match &real {
                Some(real) => {
                    assert_eq!(env.len(), 2);
                    assert_eq!(env[1].0, "PATH");
                    assert!(
                        env[1].1.starts_with(&format!("{}:", dir.display())),
                        "{}",
                        env[1].1
                    );
                    assert_eq!(
                        std::fs::read_to_string(dir.join("sudo")).unwrap(),
                        format!("#!/bin/sh\nexec '{}' -A \"$@\"\n", real.display())
                    );
                }
                None => assert_eq!(env.len(), 1),
            }
            assert!(dir
                .file_name()
                .unwrap()
                .to_string_lossy()
                .ends_with(&format!("-{}", std::process::id())));
            assert_eq!(
                std::fs::metadata(dir).unwrap().permissions().mode() & 0o777,
                0o700
            );
            let mut names: Vec<String> = std::fs::read_dir(dir)
                .unwrap()
                .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                .collect();
            names.sort();
            let want: &[&str] = if real.is_some() {
                &["askpass", "s", "sudo"]
            } else {
                &["askpass", "s"]
            };
            assert_eq!(names, want);
            assert_eq!(
                std::fs::read_to_string(&script).unwrap(),
                format!(
                    "#!/bin/sh\nexec '/opt/Grok Hub/grokhub' --askpass '{}' \"$@\"\n",
                    dir.join("s").display()
                )
            );
            // A caller that is not our helper under sudo gets "no", never a password.
            let mut s = connect_socket(&dir.join("s")).unwrap();
            s.write_all(b"ask\n").unwrap();
            let mut reply = String::new();
            s.read_to_string(&mut reply).unwrap();
            assert_eq!(reply, "no\n");
            assert_eq!(client(&dir.join("s").to_string_lossy()), 1);
            let _ = std::fs::remove_dir_all(dir);
            let _ = std::fs::remove_dir_all(cfg);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_commands_that_name_the_helper_are_refused() {
        for cmd in [
            r#"sudo -u "$USER" "$SUDO_ASKPASS""#,
            "$SUDO_ASKPASS",
            r#""${SUDO_ASKPASS}" | cat"#,
            "echo $sudo_askpass",
            "printenv SUDO_ASKPASS",
            "/run/user/1000/grokhub-askpass-42/askpass",
            "grokhub --askpass /run/user/1000/grokhub-askpass-42/s",
        ] {
            assert!(names_helper(cmd), "{cmd}");
        }
        for cmd in [
            "sudo pacman -Syu",
            "sudo -u postgres psql",
            "ls ~/.ssh",
            "echo ask pass",
        ] {
            assert!(!names_helper(cmd), "{cmd}");
        }
    }

    fn typed(pw: &str) -> Option<Zeroizing<String>> {
        Some(Zeroizing::new(pw.to_string()))
    }

    /// [`Keeper::answer`] for a `sudo` that is still there: the answer reaches
    /// it, as the server reports with [`Keeper::delivered`].
    fn answered(
        k: &mut Keeper,
        sudo: u32,
        dialog: &mut dyn FnMut() -> Option<Zeroizing<String>>,
    ) -> Option<Zeroizing<String>> {
        let pw = k.answer(sudo, dialog);
        if let Some(pw) = &pw {
            k.delivered(sudo, pw.clone());
        }
        pw
    }

    #[test]
    fn the_password_is_asked_once_per_session() {
        let mut k = Keeper::default();
        let mut shown = 0;
        let mut dialog = || {
            shown += 1;
            typed("hunter22")
        };
        for sudo in [101, 202, 303, 404] {
            assert_eq!(
                answered(&mut k, sudo, &mut dialog)
                    .as_deref()
                    .map(String::as_str),
                Some("hunter22")
            );
        }
        assert_eq!(shown, 1, "one dialog for four sudo commands");
        assert!(k.remembers());
    }

    #[test]
    fn a_password_that_never_reached_sudo_is_not_remembered() {
        let mut k = Keeper::default();
        let shown = std::cell::Cell::new(0);
        let mut dialog = || {
            shown.set(shown.get() + 1);
            typed("hunter22")
        };
        // sudo 1's command was stopped while the dialog was open: the typed
        // password had nobody to go to.
        assert_eq!(
            k.answer(1, &mut dialog).as_deref().map(String::as_str),
            Some("hunter22")
        );
        assert!(!k.remembers());
        assert_eq!(
            k.answer(2, &mut dialog).as_deref().map(String::as_str),
            Some("hunter22")
        );
        assert_eq!(shown.get(), 2, "the next sudo asks again");
        assert!(!k.remembers());
        // sudo 2 got its answer: from here on it is remembered.
        k.delivered(2, Zeroizing::new("hunter22".to_string()));
        assert!(k.remembers());
        assert_eq!(
            k.answer(3, &mut dialog).as_deref().map(String::as_str),
            Some("hunter22")
        );
        assert_eq!(shown.get(), 2);
        // An answer reported for a sudo other than the one asking is not kept.
        let mut k = Keeper::default();
        assert_eq!(
            k.answer(5, &mut dialog).as_deref().map(String::as_str),
            Some("hunter22")
        );
        k.delivered(6, Zeroizing::new("hunter22".to_string()));
        assert!(!k.remembers());
    }

    #[test]
    fn a_dialog_closed_because_its_command_left_does_not_hold_off() {
        let mut k = Keeper::default();
        let mut shown = 0;
        let mut closed = || {
            shown += 1;
            None
        };
        assert_eq!(k.answer(1, &mut closed), None);
        k.asker_left();
        assert_eq!(k.answer(2, &mut closed), None);
        assert_eq!(shown, 2, "no five-minute hold after an asker left");
    }

    #[test]
    fn a_wrong_password_asks_again_and_cancel_holds_off() {
        let mut k = Keeper::default();
        let mut typed_in = vec![typed("right"), typed("wrong")];
        let mut shown = 0;
        let mut dialog = || {
            shown += 1;
            typed_in.pop().flatten()
        };
        assert_eq!(
            answered(&mut k, 7, &mut dialog)
                .as_deref()
                .map(String::as_str),
            Some("wrong")
        );
        // sudo 7 asks again: the remembered one was wrong.
        assert_eq!(
            answered(&mut k, 7, &mut dialog)
                .as_deref()
                .map(String::as_str),
            Some("right")
        );
        assert_eq!(
            answered(&mut k, 8, &mut dialog)
                .as_deref()
                .map(String::as_str),
            Some("right")
        );
        assert_eq!(shown, 2);

        let mut k = Keeper::default();
        let mut shown = 0;
        let mut cancel = || {
            shown += 1;
            None
        };
        assert_eq!(k.answer(1, &mut cancel), None);
        assert_eq!(k.answer(2, &mut cancel), None);
        assert_eq!(k.answer(3, &mut cancel), None);
        assert_eq!(
            shown, 1,
            "after a cancel, sudo fails at once instead of asking again"
        );
        assert!(!k.remembers());
        let mut empty = || typed("");
        let mut k = Keeper::default();
        assert_eq!(k.answer(1, &mut empty), None, "an empty entry is a cancel");
    }

    #[test]
    fn the_dialog_fits_the_desktop() {
        let all = |_: &str| true;
        let (bin, args) = dialog_argv(true, &all).unwrap();
        assert_eq!(bin, "kdialog");
        assert_eq!(
            args,
            vec!["--title", "Loadout", "--password", (*DIALOG_TEXT)]
        );
        let (bin, args) = dialog_argv(false, &all).unwrap();
        assert_eq!(bin, "zenity");
        assert_eq!(
            args,
            vec![
                "--entry".to_string(),
                "--hide-text".into(),
                "--title=Loadout".into(),
                format!("--text={}", *DIALOG_TEXT)
            ]
        );
        assert_eq!(
            dialog_argv(true, &|b: &str| b == "yad")
                .map(|d| d.0)
                .as_deref(),
            Some("yad")
        );
        assert_eq!(dialog_argv(false, &|_: &str| false), None);
        assert_eq!(
            askpass_script(
                std::path::Path::new("/a/it's/grokhub"),
                std::path::Path::new("/run/s")
            ),
            "#!/bin/sh\nexec '/a/it'\\''s/grokhub' --askpass '/run/s' \"$@\"\n"
        );
        assert_eq!(
            sudo_script(std::path::Path::new("/usr/bin/sudo")),
            "#!/bin/sh\nexec '/usr/bin/sudo' -A \"$@\"\n"
        );
    }
}
