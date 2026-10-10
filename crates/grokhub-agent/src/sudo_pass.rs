//! `sudo` asks for your password once per session, through an OS-style
//! password dialog, never a GrokHub permission card (card 34).
//!
//! The cabin listens on a private socket. Shell commands run with no
//! terminal, `SUDO_ASKPASS` set to a small script that calls
//! `grokhub --askpass`, and a `sudo` first on `PATH` that is the real one
//! with `-A` (sudo only uses the helper when told to), so `sudo` asks it. The helper asks the cabin:
//! the first time, the cabin shows the dialog; after that it answers from
//! memory until GrokHub closes. The password is never written to disk and is
//! wiped from memory on drop. Only our own helper, started by a real `sudo`
//! (running as root) and answering into a pipe, gets an answer. Linux only;
//! elsewhere nothing changes.

use std::time::Duration;

use zeroize::Zeroizing;

/// The dialog's words.
pub const DIALOG_TEXT: &str =
    "GrokHub needs your password to run an admin command (sudo). It asks once and keeps it in memory until GrokHub closes.";

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
    /// OS dialog and returns `None` when you cancel.
    pub fn answer(&mut self, sudo_pid: u32, dialog: &mut dyn FnMut() -> Option<Zeroizing<String>>) -> Option<Zeroizing<String>> {
        if self.last_sudo == Some(sudo_pid) {
            self.password = None;
        }
        self.last_sudo = Some(sudo_pid);
        if let Some(pw) = &self.password {
            return Some(pw.clone());
        }
        if self.declined_at.is_some_and(|at| at.elapsed() < DECLINE_HOLD) {
            return None;
        }
        match dialog().filter(|pw| !pw.is_empty()) {
            Some(pw) => {
                self.declined_at = None;
                self.password = Some(pw.clone());
                Some(pw)
            }
            None => {
                self.declined_at = Some(std::time::Instant::now());
                None
            }
        }
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
    format!("#!/bin/sh\nexec {} --askpass {} \"$@\"\n", sh_quote(exe), sh_quote(socket))
}

/// The `sudo` agent commands find first: the real one, told to use the helper.
pub fn sudo_script(real: &std::path::Path) -> String {
    format!("#!/bin/sh\nexec {} -A \"$@\"\n", sh_quote(real))
}

/// The dialog program and its args, best first for this desktop.
pub fn dialog_argv(kde: bool, has: &dyn Fn(&str) -> bool) -> Option<(String, Vec<String>)> {
    let kdialog = ("kdialog", vec!["--title".into(), "GrokHub".into(), "--password".into(), DIALOG_TEXT.into()]);
    let entry = |bin: &'static str| (bin, vec!["--entry".into(), "--hide-text".into(), "--title=GrokHub".into(), format!("--text={DIALOG_TEXT}")]);
    let order: Vec<(&str, Vec<String>)> = if kde {
        vec![kdialog, entry("zenity"), entry("qarma"), entry("yad")]
    } else {
        vec![entry("zenity"), kdialog, entry("qarma"), entry("yad")]
    };
    order.into_iter().find(|(bin, _)| has(bin)).map(|(bin, args)| (bin.to_string(), args))
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
    use std::os::unix::fs::PermissionsExt;
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
        let base = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).filter(|p| p.is_dir()).unwrap_or_else(std::env::temp_dir);
        sweep(&base);
        let dir = base.join(format!("grokhub-askpass-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir(&dir)?;
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))?;
        let socket = dir.join("s");
        let script = dir.join("askpass");
        let listener = UnixListener::bind(&socket)?;
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
        std::thread::Builder::new().name("sudo-askpass".into()).spawn(move || {
            for stream in listener.incoming().flatten() {
                serve(stream, &exe, &keeper);
            }
        })?;
        let _ = SERVER.set(Server { dir, script, wrapped: real.is_some() });
        Ok(())
    }

    /// The first `sudo` on `PATH` outside `ours`.
    pub(super) fn real_sudo(ours: &Path) -> Option<PathBuf> {
        let path = std::env::var_os("PATH")?;
        std::env::split_paths(&path).filter(|d| d != ours).map(|d| d.join("sudo")).find(|p| p.is_file())
    }

    /// Env for a shell command once [`install`] ran: `SUDO_ASKPASS`, and
    /// `PATH` with the `sudo` wrapper first.
    pub fn shell_env() -> Vec<(String, String)> {
        let Some(s) = SERVER.get() else {
            return Vec::new();
        };
        let mut env = vec![("SUDO_ASKPASS".to_string(), s.script.to_string_lossy().into_owned())];
        if s.wrapped {
            let mut dirs = vec![s.dir.clone()];
            dirs.extend(std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect::<Vec<_>>()).unwrap_or_default());
            if let Ok(path) = std::env::join_paths(dirs) {
                env.push(("PATH".to_string(), path.to_string_lossy().into_owned()));
            }
        }
        env
    }

    /// `grokhub --askpass <socket>`: print the password for `sudo`, or exit 1.
    pub fn client(socket: &str) -> i32 {
        let Ok(mut stream) = UnixStream::connect(socket) else {
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
        if out.write_all(pw).and_then(|_| out.write_all(b"\n")).and_then(|_| out.flush()).is_err() {
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
            let Some(pid) = name.strip_prefix("grokhub-askpass-").and_then(|p| p.parse::<u32>().ok()) else {
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
        let answer = peer_pid(&stream).and_then(|pid| sudo_parent(pid, exe)).and_then(|sudo| {
            let mut k = keeper.lock().unwrap_or_else(|e| e.into_inner());
            k.answer(sudo, &mut show_dialog)
        });
        let _ = match answer {
            Some(pw) => {
                let mut msg = Zeroizing::new(b"ok\n".to_vec());
                msg.extend_from_slice(pw.as_bytes());
                stream.write_all(&msg)
            }
            None => stream.write_all(b"no\n"),
        };
    }

    fn peer_pid(stream: &UnixStream) -> Option<u32> {
        let mut cred = libc::ucred { pid: 0, uid: 0, gid: 0 };
        let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
        // SAFETY: `cred` and `len` are valid for writes of the sizes passed.
        let rc = unsafe {
            libc::getsockopt(stream.as_raw_fd(), libc::SOL_SOCKET, libc::SO_PEERCRED, (&mut cred as *mut libc::ucred).cast(), &mut len)
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
        let out = std::fs::read_link(format!("/proc/{pid}/fd/1")).ok()?;
        if !out.to_string_lossy().starts_with("pipe:") {
            return None;
        }
        let ppid = proc_ppid(pid)?;
        let comm = std::fs::read_to_string(format!("/proc/{ppid}/comm")).ok()?;
        (comm.trim() == "sudo" && proc_euid(ppid)? == 0).then_some(ppid)
    }

    fn proc_ppid(pid: u32) -> Option<u32> {
        let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
        // `pid (comm) state ppid …`; comm may hold spaces or parens.
        stat.rsplit_once(')')?.1.split_whitespace().nth(1)?.parse().ok()
    }

    fn proc_euid(pid: u32) -> Option<u32> {
        let status = std::fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
        status.lines().find_map(|l| l.strip_prefix("Uid:"))?.split_whitespace().nth(1)?.parse().ok()
    }

    fn on_path(bin: &str) -> bool {
        std::env::var_os("PATH").is_some_and(|p| std::env::split_paths(&p).any(|d| d.join(bin).is_file()))
    }

    fn show_dialog() -> Option<Zeroizing<String>> {
        let kde = std::env::var("XDG_CURRENT_DESKTOP").is_ok_and(|d| d.to_ascii_uppercase().contains("KDE"));
        let (bin, args) = dialog_argv(kde, &on_path)?;
        let out = std::process::Command::new(bin).args(args).stdin(std::process::Stdio::null()).output().ok()?;
        let raw = Zeroizing::new(out.stdout);
        if !out.status.success() {
            return None;
        }
        let text = Zeroizing::new(String::from_utf8_lossy(&raw).into_owned());
        Some(Zeroizing::new(text.trim_end_matches(['\n', '\r']).to_string()))
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
        fn the_helper_dir_holds_a_socket_and_the_script_and_no_password() {
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
                    assert!(env[1].1.starts_with(&format!("{}:", dir.display())), "{}", env[1].1);
                    assert_eq!(std::fs::read_to_string(dir.join("sudo")).unwrap(), format!("#!/bin/sh\nexec '{}' -A \"$@\"\n", real.display()));
                }
                None => assert_eq!(env.len(), 1),
            }
            assert!(dir.file_name().unwrap().to_string_lossy().ends_with(&format!("-{}", std::process::id())));
            assert_eq!(std::fs::metadata(dir).unwrap().permissions().mode() & 0o777, 0o700);
            let mut names: Vec<String> = std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
            names.sort();
            let want: &[&str] = if real.is_some() { &["askpass", "s", "sudo"] } else { &["askpass", "s"] };
            assert_eq!(names, want);
            assert_eq!(
                std::fs::read_to_string(&script).unwrap(),
                format!("#!/bin/sh\nexec '/opt/Grok Hub/grokhub' --askpass '{}' \"$@\"\n", dir.join("s").display())
            );
            // A caller that is not our helper under sudo gets "no", never a password.
            let mut s = UnixStream::connect(dir.join("s")).unwrap();
            s.write_all(b"ask\n").unwrap();
            let mut reply = String::new();
            s.read_to_string(&mut reply).unwrap();
            assert_eq!(reply, "no\n");
            assert_eq!(client(&dir.join("s").to_string_lossy()), 1);
            let _ = std::fs::remove_dir_all(dir);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn typed(pw: &str) -> Option<Zeroizing<String>> {
        Some(Zeroizing::new(pw.to_string()))
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
            assert_eq!(k.answer(sudo, &mut dialog).as_deref().map(String::as_str), Some("hunter22"));
        }
        assert_eq!(shown, 1, "one dialog for four sudo commands");
        assert!(k.remembers());
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
        assert_eq!(k.answer(7, &mut dialog).as_deref().map(String::as_str), Some("wrong"));
        // sudo 7 asks again: the remembered one was wrong.
        assert_eq!(k.answer(7, &mut dialog).as_deref().map(String::as_str), Some("right"));
        assert_eq!(k.answer(8, &mut dialog).as_deref().map(String::as_str), Some("right"));
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
        assert_eq!(shown, 1, "after a cancel, sudo fails at once instead of asking again");
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
        assert_eq!(args, vec!["--title", "GrokHub", "--password", DIALOG_TEXT]);
        let (bin, args) = dialog_argv(false, &all).unwrap();
        assert_eq!(bin, "zenity");
        assert_eq!(args, vec!["--entry".to_string(), "--hide-text".into(), "--title=GrokHub".into(), format!("--text={DIALOG_TEXT}")]);
        assert_eq!(dialog_argv(true, &|b: &str| b == "yad").map(|d| d.0).as_deref(), Some("yad"));
        assert_eq!(dialog_argv(false, &|_: &str| false), None);
        assert_eq!(
            askpass_script(std::path::Path::new("/a/it's/grokhub"), std::path::Path::new("/run/s")),
            "#!/bin/sh\nexec '/a/it'\\''s/grokhub' --askpass '/run/s' \"$@\"\n"
        );
        assert_eq!(sudo_script(std::path::Path::new("/usr/bin/sudo")), "#!/bin/sh\nexec '/usr/bin/sudo' -A \"$@\"\n");
    }
}
