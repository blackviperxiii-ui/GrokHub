//! Bot Desktop: a private virtual screen on Linux for background GUI work,
//! so the desktop tools never move the user's own cursor, type into their
//! windows, or wait for a fullscreen game to end.
//!
//! [`BotDesktop::start`] runs its own X server: `Xvnc` when it is installed
//! (so the app can preview the screen over a VNC socket) or `Xvfb`. It
//! listens only on a local socket, never TCP, and accepts only clients that
//! hold its random MIT-MAGIC-COOKIE, which lives in a mode-0600 file in the
//! config folder. Programs started on it get `DISPLAY` and `XAUTHORITY` from
//! [`BotDesktop::env`]. The server stops when the handle drops, or at
//! [`BotDesktop::stop`].
//!
//! The app keeps one shared screen ([`start_shared`], [`shared`],
//! [`stop_shared`]); while it runs, [`crate::AppDesktop`] sends the desktop
//! tools there.

use std::io::Read;
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use grokhub_core::desktop_mcp::DesktopBackend;
use x11rb::rust_connection::{DefaultStream, RustConnection};

/// Where X servers put their sockets.
pub const SOCKET_DIR: &str = "/tmp/.X11-unix";
/// Display numbers tried, high enough to stay clear of real sessions.
pub const DISPLAYS: std::ops::RangeInclusive<u32> = 90..=199;
const COOKIE_NAME: &[u8] = b"MIT-MAGIC-COOKIE-1";
/// Xauthority's wildcard family: the entry matches any host.
const FAMILY_WILD: u16 = 0xffff;
const READY_WITHIN: Duration = Duration::from_secs(10);
/// The shared screen's size.
pub const SHARED_SIZE: (u32, u32) = (1280, 800);

static SHARED: Mutex<Option<Arc<BotDesktop>>> = Mutex::new(None);

/// The app's Bot Desktop while it runs.
pub fn shared() -> Option<Arc<BotDesktop>> {
    let mut slot = SHARED.lock().unwrap_or_else(|e| e.into_inner());
    if slot.as_ref().is_some_and(|desk| !desk.running()) {
        *slot = None;
    }
    slot.clone()
}

/// Starts the app's Bot Desktop, keeping its files under `dir`, or hands back
/// the one already running.
pub fn start_shared(dir: &Path) -> Result<Arc<BotDesktop>, String> {
    if let Some(desk) = shared() {
        return Ok(desk);
    }
    let desk = Arc::new(BotDesktop::start(dir, SHARED_SIZE)?);
    let mut slot = SHARED.lock().unwrap_or_else(|e| e.into_inner());
    // Two starts at once: keep the first, and this one stops as it drops.
    if let Some(first) = slot.as_ref().filter(|d| d.running()) {
        return Ok(first.clone());
    }
    *slot = Some(desk.clone());
    Ok(desk)
}

/// Stops the app's Bot Desktop. True when one was running.
pub fn stop_shared() -> bool {
    let desk = SHARED.lock().unwrap_or_else(|e| e.into_inner()).take();
    desk.is_some_and(|desk| {
        let was = desk.running();
        desk.stop();
        was
    })
}

/// Which X server runs the screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Server {
    /// TigerVNC's X server: the screen can be previewed over `rfb.sock`.
    Xvnc,
    /// A plain virtual framebuffer, with no preview.
    Xvfb,
}

impl Server {
    pub fn program(self) -> &'static str {
        match self {
            Self::Xvnc => "Xvnc",
            Self::Xvfb => "Xvfb",
        }
    }

    /// The installed server, Xvnc first.
    pub fn installed() -> Option<Self> {
        [Self::Xvnc, Self::Xvfb]
            .into_iter()
            .find(|s| crate::resolve_bin(s.program()).is_some())
    }
}

/// The server's command line. Only the local socket listens, only cookie
/// holders get in, and Xvnc's VNC side is a Unix socket with no TCP port.
pub fn server_args(
    server: Server,
    display: u32,
    auth: &Path,
    size: (u32, u32),
    rfb: &Path,
) -> Vec<String> {
    let (w, h) = size;
    let mut args = vec![
        format!(":{display}"),
        "-auth".into(),
        auth.display().to_string(),
        "-nolisten".into(),
        "tcp".into(),
        "-noreset".into(),
    ];
    match server {
        Server::Xvnc => args.extend([
            "-geometry".into(),
            format!("{w}x{h}"),
            "-depth".into(),
            "24".into(),
            "-rfbport".into(),
            "-1".into(),
            "-rfbunixpath".into(),
            rfb.display().to_string(),
            "-rfbunixmode".into(),
            "0600".into(),
            "-SecurityTypes".into(),
            "None".into(),
        ]),
        Server::Xvfb => args.extend(["-screen".into(), "0".into(), format!("{w}x{h}x24")]),
    }
    args
}

/// One Xauthority entry: the wildcard family, `display`'s number, the cookie.
pub fn xauth_entry(display: u32, cookie: &[u8]) -> Vec<u8> {
    fn field(out: &mut Vec<u8>, bytes: &[u8]) {
        out.extend_from_slice(&(bytes.len() as u16).to_be_bytes());
        out.extend_from_slice(bytes);
    }
    let mut out = FAMILY_WILD.to_be_bytes().to_vec();
    field(&mut out, b"");
    field(&mut out, display.to_string().as_bytes());
    field(&mut out, COOKIE_NAME);
    field(&mut out, cookie);
    out
}

/// The first display number in `range` that no server holds in `socket_dir`
/// (its socket) or `/tmp` (its lock file).
pub fn free_display(
    socket_dir: &Path,
    lock_dir: &Path,
    range: std::ops::RangeInclusive<u32>,
) -> Option<u32> {
    range.into_iter().find(|n| {
        !socket_dir.join(format!("X{n}")).exists() && !lock_dir.join(format!(".X{n}-lock")).exists()
    })
}

/// A running Bot Desktop.
pub struct BotDesktop {
    child: Mutex<Child>,
    server: Server,
    display: u32,
    auth: PathBuf,
    rfb: PathBuf,
    cookie: Vec<u8>,
    size: (u32, u32),
}

impl BotDesktop {
    /// Starts a `size` screen with the installed server, keeping its cookie
    /// and VNC socket under `dir`, and waits until it takes connections.
    pub fn start(dir: &Path, size: (u32, u32)) -> Result<Self, String> {
        let server = Server::installed().ok_or(
            "Bot Desktop needs Xvnc (tigervnc) or Xvfb; install one, for example: sudo apt-get install tigervnc-standalone-server",
        )?;
        Self::start_with(server, dir, size)
    }

    /// [`BotDesktop::start`] with a chosen server.
    pub fn start_with(server: Server, dir: &Path, size: (u32, u32)) -> Result<Self, String> {
        let display = free_display(Path::new(SOCKET_DIR), Path::new("/tmp"), DISPLAYS)
            .ok_or("no free display number for Bot Desktop")?;
        std::fs::create_dir_all(dir).map_err(|e| format!("Bot Desktop folder: {e}"))?;
        let cookie = random_cookie()?;
        let auth = dir.join(format!("bot-desktop-{display}.xauth"));
        let rfb = dir.join(format!("bot-desktop-{display}.rfb.sock"));
        write_private(&auth, &xauth_entry(display, &cookie))?;
        let _ = std::fs::remove_file(&rfb);
        let program = crate::resolve_bin(server.program())
            .ok_or_else(|| format!("{} isn't installed", server.program()))?;
        let child = Command::new(program)
            .args(server_args(server, display, &auth, size, &rfb))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("couldn't start {}: {e}", server.program()))?;
        let desk = Self {
            child: Mutex::new(child),
            server,
            display,
            auth,
            rfb,
            cookie,
            size,
        };
        desk.wait_ready()?;
        Ok(desk)
    }

    fn wait_ready(&self) -> Result<(), String> {
        let started = Instant::now();
        loop {
            if self.connect().is_ok() {
                return Ok(());
            }
            let exited = self.child().try_wait();
            if let Ok(Some(status)) = exited {
                return Err(format!(
                    "{} stopped at start ({status})",
                    self.server.program()
                ));
            }
            if started.elapsed() > READY_WITHIN {
                return Err(format!("{} didn't start in time", self.server.program()));
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    fn child(&self) -> std::sync::MutexGuard<'_, Child> {
        self.child.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The X server is still up.
    pub fn running(&self) -> bool {
        matches!(self.child().try_wait(), Ok(None))
    }

    /// Stops the X server and removes its cookie and socket files. Programs
    /// on the screen lose their display and end. Safe to call twice.
    pub fn stop(&self) {
        let mut child = self.child();
        if matches!(child.try_wait(), Ok(None)) {
            let _ = child.kill();
        }
        let _ = child.wait();
        drop(child);
        let _ = std::fs::remove_file(&self.auth);
        let _ = std::fs::remove_file(&self.rfb);
    }

    pub fn server(&self) -> Server {
        self.server
    }

    /// `:N`, for `DISPLAY`.
    pub fn display_name(&self) -> String {
        format!(":{}", self.display)
    }

    pub fn size(&self) -> (u32, u32) {
        self.size
    }

    /// The cookie file, for `XAUTHORITY`.
    pub fn xauthority(&self) -> &Path {
        &self.auth
    }

    /// The VNC socket for the preview (Xvnc only).
    pub fn preview_socket(&self) -> Option<&Path> {
        (self.server == Server::Xvnc).then_some(self.rfb.as_path())
    }

    /// The environment for a program that runs on this screen. Wayland's
    /// variables are cleared so a toolkit can't pick the user's session.
    pub fn env(&self) -> Vec<(&'static str, Option<String>)> {
        vec![
            ("DISPLAY", Some(self.display_name())),
            ("XAUTHORITY", Some(self.auth.display().to_string())),
            ("WAYLAND_DISPLAY", None),
            ("XDG_SESSION_TYPE", Some("x11".into())),
        ]
    }

    /// An X connection to this screen, with its cookie.
    pub fn connect(&self) -> Result<(RustConnection, usize), String> {
        let sock = Path::new(SOCKET_DIR).join(format!("X{}", self.display));
        let stream = UnixStream::connect(&sock).map_err(|e| format!("Bot Desktop: {e}"))?;
        let (stream, _) =
            DefaultStream::from_unix_stream(stream).map_err(|e| format!("Bot Desktop: {e}"))?;
        let conn = RustConnection::connect_to_stream_with_auth_info(
            stream,
            0,
            COOKIE_NAME.to_vec(),
            self.cookie.clone(),
        )
        .map_err(|e| format!("Bot Desktop: {e}"))?;
        Ok((conn, 0))
    }

    /// Starts `program` (a name on PATH, no arguments) on this screen.
    pub fn launch(&self, program: &str) -> Result<(), String> {
        if program.is_empty()
            || program.contains(['/', '\\'])
            || program.contains(char::is_whitespace)
        {
            return Err(format!("\"{program}\" isn't a program name"));
        }
        let path = crate::resolve_bin(program).ok_or_else(|| {
            format!("{program} isn't installed, so it can't open on the Bot Desktop")
        })?;
        let mut cmd = Command::new(path);
        for (key, value) in self.env() {
            match value {
                Some(v) => cmd.env(key, v),
                None => cmd.env_remove(key),
            };
        }
        cmd.stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map(|_| ())
            .map_err(|e| format!("{program}: {e}"))
    }

    /// The desktop backend, driving this screen instead of the user's.
    pub fn backend(&self) -> Result<Box<dyn DesktopBackend + Send>, String> {
        let (conn, screen) = self.connect()?;
        Ok(Box::new(crate::x11::X11Backend::over(conn, screen)))
    }
}

impl Drop for BotDesktop {
    fn drop(&mut self) {
        self.stop();
    }
}

fn random_cookie() -> Result<Vec<u8>, String> {
    let mut cookie = vec![0u8; 16];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut cookie))
        .map_err(|e| format!("Bot Desktop cookie: {e}"))?;
    Ok(cookie)
}

fn write_private(path: &Path, bytes: &[u8]) -> Result<(), String> {
    use std::io::Write;
    let _ = std::fs::remove_file(path);
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .and_then(|mut f| f.write_all(bytes))
        .map_err(|e| format!("Bot Desktop cookie file: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_server_listens_only_locally_and_only_for_the_cookie() {
        let auth = Path::new("/home/user/.config/GrokHub/bot-desktop-90.xauth");
        let rfb = Path::new("/home/user/.config/GrokHub/bot-desktop-90.rfb.sock");
        assert_eq!(
            server_args(Server::Xvfb, 90, auth, (1280, 800), rfb),
            [
                ":90",
                "-auth",
                "/home/user/.config/GrokHub/bot-desktop-90.xauth",
                "-nolisten",
                "tcp",
                "-noreset",
                "-screen",
                "0",
                "1280x800x24",
            ]
        );
        assert_eq!(
            server_args(Server::Xvnc, 91, auth, (1280, 800), rfb)[6..],
            [
                "-geometry",
                "1280x800",
                "-depth",
                "24",
                "-rfbport",
                "-1",
                "-rfbunixpath",
                "/home/user/.config/GrokHub/bot-desktop-90.rfb.sock",
                "-rfbunixmode",
                "0600",
                "-SecurityTypes",
                "None",
            ]
        );
    }

    #[test]
    fn the_cookie_entry_is_the_xauthority_format() {
        let entry = xauth_entry(90, &[0xab; 16]);
        let mut want = vec![0xff, 0xff, 0, 0, 0, 2, b'9', b'0', 0, 18];
        want.extend_from_slice(b"MIT-MAGIC-COOKIE-1");
        want.extend_from_slice(&[0, 16]);
        want.extend_from_slice(&[0xab; 16]);
        assert_eq!(entry, want);
    }

    #[test]
    fn a_display_number_in_use_is_skipped() {
        let dir = std::env::temp_dir().join(format!("grokhub-botdesk-free-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("sock")).unwrap();
        std::fs::write(dir.join("sock/X90"), "").unwrap();
        std::fs::write(dir.join(".X91-lock"), "").unwrap();
        assert_eq!(free_display(&dir.join("sock"), &dir, 90..=99), Some(92));
        assert_eq!(free_display(&dir.join("sock"), &dir, 90..=91), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Starts a real Xvfb (skipped where none is installed, as on Windows
    /// runners): the backend moves the bot screen's pointer and reads its
    /// pixels, and a client without the cookie is turned away.
    #[test]
    fn the_backend_drives_the_bot_screen_and_strangers_are_refused() {
        if crate::resolve_bin("Xvfb").is_none() {
            eprintln!("skipped: no Xvfb");
            return;
        }
        use x11rb::connection::Connection;
        use x11rb::protocol::xproto::ConnectionExt;
        let dir = std::env::temp_dir().join(format!("grokhub-botdesk-run-{}", std::process::id()));
        let desk = BotDesktop::start_with(Server::Xvfb, &dir, (640, 480)).expect("starts");
        assert_eq!(desk.preview_socket(), None);
        let mode = std::fs::metadata(desk.xauthority()).unwrap().permissions();
        assert_eq!(
            std::os::unix::fs::PermissionsExt::mode(&mode) & 0o777,
            0o600
        );

        let mut backend = desk.backend().expect("backend");
        let monitors = backend.list_monitors().expect("monitors");
        assert_eq!(
            monitors
                .iter()
                .map(|m| (m.width, m.height))
                .collect::<Vec<_>>(),
            [(640, 480)]
        );
        backend.move_abs(123, 45).expect("move");
        let (conn, screen) = desk.connect().unwrap();
        let root = conn.setup().roots[screen].root;
        let at = conn.query_pointer(root).unwrap().reply().unwrap();
        assert_eq!((at.root_x, at.root_y), (123, 45));
        let shot = backend.screenshot("all").expect("shot");
        assert_eq!((shot.geom.physical_w, shot.geom.physical_h), (640, 480));
        assert!(!shot.bytes.is_empty());

        // No cookie, no connection.
        let sock = Path::new(SOCKET_DIR).join(format!("X{}", &desk.display_name()[1..]));
        let (stream, _) =
            DefaultStream::from_unix_stream(UnixStream::connect(sock).unwrap()).unwrap();
        assert!(RustConnection::connect_to_stream(stream, 0).is_err());

        let auth = desk.xauthority().to_path_buf();
        drop(desk);
        assert!(!auth.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The native desktop tools, pointed at a real Xvfb Bot Desktop: a
    /// screenshot and a move land there, apps open only by name, and the
    /// window tools say they don't reach it.
    #[test]
    fn the_desktop_tools_run_on_the_bot_desktop() {
        if crate::resolve_bin("Xvfb").is_none() {
            eprintln!("skipped: no Xvfb");
            return;
        }
        use grokhub_agent::DesktopOps;
        use x11rb::connection::Connection;
        use x11rb::protocol::xproto::ConnectionExt;
        let dir =
            std::env::temp_dir().join(format!("grokhub-botdesk-tools-{}", std::process::id()));
        let desk = std::sync::Arc::new(
            BotDesktop::start_with(Server::Xvfb, &dir, (800, 600)).expect("starts"),
        );
        let tools = crate::NativeDesktop::on_bot_desktop(desk.clone());
        let shot = tools.call("screenshot", &serde_json::json!({}));
        assert!(!shot.failed, "{}", shot.text);
        assert!(shot.image_data_url.is_some());
        let moved = tools.call("move", &serde_json::json!({"x": 200, "y": 150}));
        assert!(!moved.failed, "{}", moved.text);
        let (conn, screen) = desk.connect().unwrap();
        let root = conn.setup().roots[screen].root;
        let at = conn.query_pointer(root).unwrap().reply().unwrap();
        assert_eq!((at.root_x, at.root_y), (200, 150));

        let windows = tools.call("focus_window", &serde_json::json!({"title": "Files"}));
        assert!(windows.failed);
        assert!(
            windows
                .text
                .contains("The Bot Desktop has no window list yet"),
            "{}",
            windows.text
        );
        let open = tools.call(
            "open_app",
            &serde_json::json!({"app": "grokhub-no-such-app"}),
        );
        assert!(open.failed);
        assert!(
            open.text.contains(
                "grokhub-no-such-app isn't installed, so it can't open on the Bot Desktop"
            ),
            "{}",
            open.text
        );
        assert_eq!(desk.launch("true"), Ok(()));
        assert_eq!(
            desk.launch("sh -c id"),
            Err("\"sh -c id\" isn't a program name".to_string())
        );
        drop(tools);
        drop(desk);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The app's tools follow the shared Bot Desktop: on, a move lands on
    /// that screen; stopped, the server and its cookie are gone and the next
    /// call goes back to the user's screen.
    #[test]
    fn the_app_tools_follow_the_shared_bot_desktop_on_and_off() {
        if Server::installed().is_none() {
            eprintln!("skipped: no Xvnc or Xvfb");
            return;
        }
        use grokhub_agent::DesktopOps;
        use x11rb::connection::Connection;
        use x11rb::protocol::xproto::ConnectionExt;
        let dir =
            std::env::temp_dir().join(format!("grokhub-botdesk-shared-{}", std::process::id()));
        let app = crate::AppDesktop::new();
        assert!(shared().is_none());
        assert!(!stop_shared());

        let desk = start_shared(&dir).expect("starts");
        assert_eq!(desk.size(), SHARED_SIZE);
        assert!(Arc::ptr_eq(&desk, &start_shared(&dir).unwrap()));
        let moved = app.call("move", &serde_json::json!({"x": 300, "y": 200}));
        assert!(!moved.failed, "{}", moved.text);
        let (conn, screen) = desk.connect().unwrap();
        let root = conn.setup().roots[screen].root;
        let at = conn.query_pointer(root).unwrap().reply().unwrap();
        assert_eq!((at.root_x, at.root_y), (300, 200));
        drop(conn);
        let on_bot = app.call("focus_window", &serde_json::json!({"title": "Files"}));
        assert!(on_bot.failed);
        assert_eq!(on_bot.text, crate::BOT_NO_WINDOWS);

        let auth = desk.xauthority().to_path_buf();
        assert!(stop_shared());
        assert!(!desk.running());
        assert!(!auth.exists());
        assert!(shared().is_none());
        assert!(desk.connect().is_err());
        let after = app.call("focus_window", &serde_json::json!({"title": "Files"}));
        assert_ne!(after.text, crate::BOT_NO_WINDOWS);
        drop(desk);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
