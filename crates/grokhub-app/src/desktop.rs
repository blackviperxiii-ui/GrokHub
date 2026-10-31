use grokhub_core::{
    act_window_search_bin, capture_kinds, cdp_new_tab_path, cdp_page_close_payload,
    cdp_page_focus_payload, clamp_to_desktop, clip_image_args, computer_cmd_line,
    computer_drive_for, cursor_on_output, diagnose_hands, empty_hands_steps_error,
    ffmpeg_webcam_args, ffmpeg_x11_args, filter_atspi_rows, format_cursor_line_miss,
    format_tab_list, frame_is_blank, frame_origin_for, gnome_shell_screenshot_args,
    grim_capture_args, hands_blocked_by_lock, hands_down_receipt, image_pixels_ok, image_to_global,
    infer_wayland_display, jpeg_data_url, luma_mean_var, monitor_local_to_global, parse_atspi_line,
    parse_cdp_targets, parse_picker_stdout, parse_wmctrl_line, parse_xdotool_mouse,
    parse_xrandr_outputs, pick_browser_tab, pick_capture_output, pick_hands_backend,
    pick_named_row, picker_args, png_ihdr_size, pointer_slop_miss, relative_move_steps,
    session_is_wayland, tab_list_from_rows, take_text_body, virtual_desktop_size, x11_grab_size,
    ydotool_socket_path, AtspiRow, BrowserTab, CaptureKind, ComputerDrive, ComputerOp,
    DisplayOutput, HandsBackend, HandsDown, TabAction, ATSPI_PY, CDP_DOWN, CDP_PORTS,
    IMAGE_FILE_CAP, RECORDERS, TEXT_FILE_CAP, TRANSCRIBERS,
};
use grokhub_desktop::game_guard::{GameGuard, Guarded};
use image::GenericImageView;
use std::io::Read;
use std::net::{TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

struct LastDeskFrame {
    jpeg_w: u32,
    jpeg_h: u32,
    origin_x: i32,
    origin_y: i32,
    outputs: Vec<DisplayOutput>,
}

static LAST_DESK_FRAME: Mutex<Option<LastDeskFrame>> = Mutex::new(None);

#[derive(Clone, Default)]
struct DeskScan {
    rows: Vec<AtspiRow>,
    lock: Vec<String>,
}

static LAST_DESK_SCAN: Mutex<Option<(Instant, DeskScan)>> = Mutex::new(None);
const DESK_SCAN_TTL: Duration = Duration::from_millis(400);

/// Listing bins (AT-SPI, wmctrl, xrandr) on the UI thread.
const DESK_LIST_TIMEOUT: Duration = Duration::from_millis(1500);
/// Screenshot / grim / ffmpeg on the UI thread.
const DESK_CAPTURE_TIMEOUT: Duration = Duration::from_secs(8);
/// Webcam ffmpeg on the UI thread.
const DESK_WEBCAM_TIMEOUT: Duration = Duration::from_secs(4);
static CAPTURE_SEQ: AtomicU64 = AtomicU64::new(0);

fn capture_temp(kind: &str, ext: &str) -> PathBuf {
    let n = CAPTURE_SEQ.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("grokhub-{kind}-{n}.{ext}"))
}
/// Local whisper on a worker thread — still must not hang halt forever.
const DESK_TRANSCRIBE_TIMEOUT: Duration = Duration::from_secs(20);

fn kill_limited(child: &mut Child) {
    #[cfg(unix)]
    {
        let pid = child.id() as i32;
        let _ = Command::new("kill")
            .args(["-KILL", "--", &format!("-{pid}")])
            .status();
    }
    #[cfg(windows)]
    {
        let pid = child.id().to_string();
        let mut kill = Command::new("taskkill");
        kill.args(["/F", "/T", "/PID", &pid]);
        kill.stdout(Stdio::null()).stderr(Stdio::null());
        crate::host::hide_windows_console(&mut kill);
        let _ = kill.status();
    }
    let _ = child.kill();
}

fn read_capped(r: &mut impl Read, buf: &mut Vec<u8>, cap: usize) -> std::io::Result<()> {
    let mut tmp = [0u8; 8192];
    loop {
        if buf.len() >= cap {
            return Ok(());
        }
        let n = match r.read(&mut tmp) {
            Ok(0) => return Ok(()),
            Ok(n) => n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        };
        let room = cap - buf.len();
        buf.extend_from_slice(&tmp[..n.min(room)]);
    }
}

fn read_pipe_capped(mut r: impl Read, cap: usize, overflow: &AtomicBool) -> Vec<u8> {
    let mut buf = Vec::new();
    let _ = read_capped(&mut r, &mut buf, cap);
    if buf.len() >= cap {
        overflow.store(true, Ordering::SeqCst);
    }
    buf
}

/// Spawn `cmd` and kill the process group if it exceeds `timeout`.
/// Used by presence / windshield paths that run on the UI thread.
pub(crate) fn run_limited(mut cmd: Command, timeout: Duration) -> Option<Output> {
    cmd.stdin(Stdio::null());
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());
    crate::host::hide_windows_console(&mut cmd);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    let mut child = cmd.spawn().ok()?;
    let cap = (IMAGE_FILE_CAP as usize).saturating_add(1);
    let overflow = Arc::new(AtomicBool::new(false));
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let out_flag = overflow.clone();
    let err_flag = overflow.clone();
    let h_out = std::thread::spawn(move || match stdout {
        Some(s) => read_pipe_capped(s, cap, &out_flag),
        None => Vec::new(),
    });
    let h_err = std::thread::spawn(move || match stderr {
        Some(s) => read_pipe_capped(s, cap, &err_flag),
        None => Vec::new(),
    });
    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() >= deadline || overflow.load(Ordering::SeqCst) => {
                kill_limited(&mut child);
                match child.wait() {
                    Ok(status) if overflow.load(Ordering::SeqCst) => break status,
                    _ => {
                        let _ = h_out.join();
                        let _ = h_err.join();
                        return None;
                    }
                }
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(15)),
            Err(_) => {
                kill_limited(&mut child);
                let _ = child.wait();
                let _ = h_out.join();
                let _ = h_err.join();
                return None;
            }
        }
    };
    Some(Output {
        status,
        stdout: h_out.join().unwrap_or_default(),
        stderr: h_err.join().unwrap_or_default(),
    })
}

fn remember_desk_frame(
    jpeg_w: u32,
    jpeg_h: u32,
    origin_x: i32,
    origin_y: i32,
    outputs: Vec<DisplayOutput>,
) {
    if let Ok(mut g) = LAST_DESK_FRAME.lock() {
        *g = Some(LastDeskFrame {
            jpeg_w,
            jpeg_h,
            origin_x,
            origin_y,
            outputs,
        });
    }
}

pub fn read_display_outputs() -> Vec<DisplayOutput> {
    let mut cmd = Command::new("xrandr");
    cmd.arg("-q");
    run_limited(cmd, DESK_LIST_TIMEOUT)
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|t| parse_xrandr_outputs(&t))
        .unwrap_or_default()
}

pub fn probe_hub_health_body(port: u16) -> Option<String> {
    let url = format!("http://127.0.0.1:{port}/v1/health");
    // Loopback is local, not egress: the guard allows it and logs nothing.
    crate::xai::egress_ok(&url, &[]).ok()?;
    ureq::get(&url)
        .timeout(Duration::from_millis(400))
        .call()
        .ok()
        .and_then(|r| r.into_string().ok())
}

fn cdp_http(port: u16, path: &str) -> Result<String, String> {
    let url = format!("http://127.0.0.1:{port}{path}");
    crate::xai::egress_ok(&url, &[])?;
    let resp = ureq::get(&url)
        .timeout(Duration::from_millis(400))
        .call()
        .map_err(|e| e.to_string())?;
    let mut buf = Vec::new();
    resp.into_reader()
        .take(TEXT_FILE_CAP as u64)
        .read_to_end(&mut buf)
        .map_err(|e| e.to_string())?;
    String::from_utf8(buf).map_err(|e| e.to_string())
}

fn probe_cdp() -> Option<(u16, Vec<BrowserTab>)> {
    for port in CDP_PORTS {
        for path in ["/json/list", "/json"] {
            let Ok(raw) = cdp_http(*port, path) else {
                continue;
            };
            let Ok(tabs) = parse_cdp_targets(&raw) else {
                continue;
            };
            return Some((*port, tabs));
        }
    }
    None
}

fn run_tab_op(
    action: TabAction,
    query: &str,
    cancel: Option<&AtomicBool>,
) -> Result<String, String> {
    if cancelled(cancel) {
        return Err("halted".into());
    }
    if let Some((port, tabs)) = probe_cdp() {
        return run_tab_op_cdp(port, &tabs, action, query);
    }
    run_tab_op_fallback(action, query, cancel)
}

fn run_tab_op_cdp(
    port: u16,
    tabs: &[BrowserTab],
    action: TabAction,
    query: &str,
) -> Result<String, String> {
    match action {
        TabAction::List => Ok(format_tab_list(tabs)),
        TabAction::Close => {
            let tab = pick_browser_tab(tabs, query)?;
            close_cdp_tab(port, tab)?;
            Ok(format!("closed {}", tab.title))
        }
        TabAction::Focus => {
            let tab = pick_browser_tab(tabs, query)?;
            focus_cdp_tab(port, tab)?;
            Ok(format!("focused {}", tab.title))
        }
        TabAction::New => {
            let path = cdp_new_tab_path(query);
            cdp_http(port, &path).map_err(|e| format!("tab new: {e}"))?;
            if query.trim().is_empty() {
                Ok("opened new tab".into())
            } else {
                Ok(format!("opened {query}"))
            }
        }
    }
}

fn run_tab_op_fallback(
    action: TabAction,
    query: &str,
    cancel: Option<&AtomicBool>,
) -> Result<String, String> {
    match action {
        TabAction::List => {
            let rows = collect_rows();
            let listed = tab_list_from_rows(&rows);
            if listed != "no page tabs" {
                return Ok(listed);
            }
            let mut wins = String::new();
            for r in &rows {
                if r.role == "window" || r.role == "frame" {
                    if !wins.is_empty() {
                        wins.push('\n');
                    }
                    wins.push_str(&format!("- {}", r.name));
                }
            }
            if wins.is_empty() {
                Ok(format!(
                    "no page tabs — {CDP_DOWN}; use act New Tab or wait_for then key ctrl+t"
                ))
            } else {
                Ok(wins)
            }
        }
        TabAction::New => {
            focus_browser_window(query, cancel);
            run_pointer_op(
                &ComputerOp::Key {
                    name: "ctrl+t".into(),
                },
                cancel,
            )?;
            let url = query.trim();
            if !url.is_empty() && !is_browser_app_name(url) {
                run_pointer_op(
                    &ComputerOp::Type {
                        text: url.to_string(),
                    },
                    cancel,
                )?;
                run_pointer_op(
                    &ComputerOp::Key {
                        name: "Return".into(),
                    },
                    cancel,
                )?;
                Ok(format!("opened {url} (key ctrl+t)"))
            } else {
                Ok("opened new tab (key ctrl+t)".into())
            }
        }
        TabAction::Close => {
            if act_click(query, cancel).is_ok() {
                return Ok(format!("closed {query} via act"));
            }
            let _ = wait_for_title(Some(query), cancel);
            run_pointer_op(
                &ComputerOp::Key {
                    name: "ctrl+w".into(),
                },
                cancel,
            )?;
            Ok(format!("closed {query} (key ctrl+w)"))
        }
        TabAction::Focus => {
            if let Ok((x, y)) = act_click(query, cancel) {
                return Ok(format!("focused {query} via act @{x},{y}"));
            }
            let rows = collect_rows();
            let names: Vec<&str> = rows
                .iter()
                .map(|r| r.name.as_str())
                .filter(|n| !n.is_empty() && *n != "cursor")
                .collect();
            Err(format!(
                "no tab matched {query} — {CDP_DOWN}; saw: {}",
                names.join(", ")
            ))
        }
    }
}

fn is_browser_app_name(s: &str) -> bool {
    matches!(
        s.trim().to_ascii_lowercase().as_str(),
        "firefox" | "chrome" | "chromium" | "browser" | "brave"
    )
}

fn focus_browser_window(hint: &str, cancel: Option<&AtomicBool>) {
    if !hint.trim().is_empty() && act_click(hint, cancel).is_ok() {
        return;
    }
    for name in ["Firefox", "Chrome", "Chromium", "Brave"] {
        if act_click(name, cancel).is_ok() {
            return;
        }
    }
}

fn run_pointer_op(op: &ComputerOp, cancel: Option<&AtomicBool>) -> Result<(), String> {
    match pointer_drive(op) {
        ComputerDrive::Xdotool(steps) | ComputerDrive::Ydotool(steps) => {
            if let Some(detail) = empty_hands_steps_error(op, &steps) {
                return Err(detail);
            }
            run_pointer_steps(&steps, cancel)
        }
        ComputerDrive::Act(_)
        | ComputerDrive::WaitFor(_)
        | ComputerDrive::Tab(_, _)
        | ComputerDrive::Cursor
        | ComputerDrive::MoveMonitor { .. } => Err("not a pointer op".into()),
    }
}

fn close_cdp_tab(port: u16, tab: &BrowserTab) -> Result<(), String> {
    if cdp_http(port, &format!("/json/close/{}", tab.id)).is_ok() {
        return Ok(());
    }
    cdp_ws_method(&tab.ws_url, cdp_page_close_payload())
}

fn focus_cdp_tab(port: u16, tab: &BrowserTab) -> Result<(), String> {
    if cdp_http(port, &format!("/json/activate/{}", tab.id)).is_ok() {
        return Ok(());
    }
    cdp_ws_method(&tab.ws_url, cdp_page_focus_payload())
}

fn cdp_ws_method(ws_url: &str, payload: &str) -> Result<(), String> {
    if ws_url.is_empty() {
        return Err("cdp target has no websocket".into());
    }
    let rest = ws_url
        .strip_prefix("ws://")
        .ok_or_else(|| "cdp websocket must be ws://".to_string())?;
    // Localhost CDP is loopback: allowed, no egress line.
    crate::xai::egress_ok(ws_url, &[])?;
    let (host, _) = rest
        .split_once('/')
        .ok_or_else(|| "cdp websocket path missing".to_string())?;
    let addr = host
        .to_socket_addrs()
        .map_err(|e| e.to_string())?
        .next()
        .ok_or_else(|| "cdp websocket host".to_string())?;
    let stream =
        TcpStream::connect_timeout(&addr, Duration::from_millis(800)).map_err(|e| e.to_string())?;
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .map_err(|e| e.to_string())?;
    stream
        .set_write_timeout(Some(Duration::from_secs(2)))
        .map_err(|e| e.to_string())?;
    let (mut socket, _) = tungstenite::client(ws_url, stream).map_err(|e| e.to_string())?;
    socket
        .send(tungstenite::Message::Text(payload.to_string()))
        .map_err(|e| e.to_string())?;
    let _ = socket.read();
    Ok(())
}

fn remember_from_jpeg(bytes: &[u8], outputs: &[DisplayOutput], grim_name: Option<&str>) {
    if !image_pixels_ok_for_bytes(bytes) {
        return;
    }
    let Ok(img) = image::load_from_memory(bytes) else {
        return;
    };
    let (w, h) = img.dimensions();
    let origin = frame_origin_for(w, h, outputs, grim_name);
    remember_desk_frame(w, h, origin.0, origin.1, outputs.to_vec());
}

fn map_pointer_xy(x: i32, y: i32) -> (i32, i32) {
    let (mapped, outputs) = if let Ok(g) = LAST_DESK_FRAME.lock() {
        if let Some(f) = g.as_ref() {
            (
                image_to_global(
                    x,
                    y,
                    f.jpeg_w,
                    f.jpeg_h,
                    &f.outputs,
                    Some((f.origin_x, f.origin_y)),
                ),
                f.outputs.clone(),
            )
        } else {
            ((x, y), read_display_outputs())
        }
    } else {
        ((x, y), read_display_outputs())
    };
    clamp_to_desktop(mapped.0, mapped.1, &outputs)
}

fn map_pointer_op(op: &ComputerOp) -> ComputerOp {
    match op {
        ComputerOp::Click { x, y } => {
            let (x, y) = map_pointer_xy(*x, *y);
            ComputerOp::Click { x, y }
        }
        ComputerOp::DoubleClick { x, y } => {
            let (x, y) = map_pointer_xy(*x, *y);
            ComputerOp::DoubleClick { x, y }
        }
        ComputerOp::Move { x, y } => {
            let (x, y) = map_pointer_xy(*x, *y);
            ComputerOp::Move { x, y }
        }
        other => other.clone(),
    }
}

fn push_bash_cand(p: PathBuf, out: &mut Vec<PathBuf>) {
    if p.is_file() && !is_wsl_bash_stub(&p) && !out.iter().any(|e| e == &p) {
        out.push(p);
    }
}

fn is_wsl_bash_stub(p: &Path) -> bool {
    let s = p.to_string_lossy().to_ascii_lowercase();
    s.contains("windowsapps") || s.contains("system32\\bash") || s.contains("system32/bash")
}

fn bash_probe_ok(p: &Path) -> bool {
    let mut cmd = Command::new(p);
    cmd.args(["-lc", "echo grokhub-bash"]);
    crate::host::hide_windows_console(&mut cmd);
    run_limited(cmd, Duration::from_secs(2))
        .is_some_and(|o| String::from_utf8_lossy(&o.stdout).contains("grokhub-bash"))
}

/// Git bash on Windows, a real bash on PATH — never the WSL stub.
pub fn find_real_bash() -> Option<PathBuf> {
    let mut cands = Vec::new();
    #[cfg(windows)]
    {
        for key in ["ProgramFiles", "ProgramW6432", "ProgramFiles(x86)"] {
            if let Ok(root) = std::env::var(key) {
                let root = PathBuf::from(root);
                push_bash_cand(root.join(r"Git\bin\bash.exe"), &mut cands);
                push_bash_cand(root.join(r"Git\usr\bin\bash.exe"), &mut cands);
            }
        }
        if let Ok(local) = std::env::var("LOCALAPPDATA") {
            push_bash_cand(
                PathBuf::from(local).join(r"Programs\Git\bin\bash.exe"),
                &mut cands,
            );
        }
    }
    if let Some(paths) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&paths) {
            push_bash_cand(dir.join("bash"), &mut cands);
            if cfg!(windows) {
                push_bash_cand(dir.join("bash.exe"), &mut cands);
            }
        }
    }
    cands.into_iter().find(|p| bash_probe_ok(p))
}

pub use grokhub_desktop::{resolve_bin, which};

pub fn first_bin(names: &[&str]) -> Option<String> {
    names.iter().find(|n| which(n)).map(|s| (*s).to_string())
}

fn spawn_bin(name: &str) -> Command {
    match resolve_bin(name) {
        Some(p) => Command::new(p),
        None => Command::new(name),
    }
}

fn uinput_writable() -> bool {
    let p = Path::new("/dev/uinput");
    if !p.exists() {
        return false;
    }
    std::fs::OpenOptions::new().write(true).open(p).is_ok()
}

fn ydotool_sock() -> PathBuf {
    ydotool_socket_path(
        std::env::var("YDOTOOL_SOCKET").ok().as_deref(),
        std::env::var("XDG_RUNTIME_DIR").ok().as_deref(),
    )
}

fn ydotool_socket_ready() -> bool {
    let sock = ydotool_sock();
    sock.exists() || Path::new("/tmp/.ydotool_socket").exists()
}

fn start_ydotoold() {
    let sock = ydotool_sock();
    if let Some(parent) = sock.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    std::env::set_var("YDOTOOL_SOCKET", &sock);
    let mut sys = Command::new("systemctl");
    sys.args(["--user", "start", "ydotoold"]);
    let _ = run_limited(sys, DESK_LIST_TIMEOUT);
    if ydotool_socket_ready() {
        return;
    }
    let Some(daemon) = resolve_bin("ydotoold") else {
        return;
    };
    for args in [
        vec![format!("--socket-path={}", sock.display())],
        vec!["-p".into(), sock.display().to_string()],
        vec![],
    ] {
        let mut cmd = Command::new(&daemon);
        cmd.args(&args)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .env("YDOTOOL_SOCKET", &sock);
        if cmd.spawn().is_ok() {
            let deadline = Instant::now() + Duration::from_millis(400);
            while Instant::now() < deadline {
                if ydotool_socket_ready() {
                    return;
                }
                std::thread::sleep(Duration::from_millis(40));
            }
        }
    }
}

fn hands_facts() -> (bool, bool, Option<bool>, Option<bool>) {
    let has_ydotool = resolve_bin("ydotool").is_some();
    let has_xdotool = resolve_bin("xdotool").is_some();
    if !has_ydotool {
        return (false, has_xdotool, None, None);
    }
    let uinput = uinput_writable();
    let daemon = ydotool_socket_ready();
    (true, has_xdotool, Some(uinput), Some(daemon))
}

pub fn hands_peek() -> HandsDown {
    let (yd, xd, uinput, daemon) = hands_facts();
    diagnose_hands(yd, xd, uinput, daemon)
}

pub fn ensure_hands() -> HandsDown {
    let (has_ydotool, has_xdotool, uinput, daemon) = hands_facts();
    if has_ydotool && uinput == Some(true) && daemon == Some(false) {
        start_ydotoold();
    }
    let _ = (has_xdotool,);
    hands_peek()
}

/// The shared window scan (AT-SPI, wmctrl) behind [`collect_rows`] and
/// [`lock_titles`]. Reading other windows' titles waits while an anti-cheat
/// runs: every caller then gets an empty scan and no helper is started.
fn desk_scan() -> DeskScan {
    desk_scan_under(crate::desktop_mcp::game_guard(), desk_scan_cached)
}

/// [`desk_scan`] with the game guard the caller read.
fn desk_scan_under(guard: GameGuard, scan: impl FnOnce() -> DeskScan) -> DeskScan {
    if guard.blocks(Guarded::WindowCheck).is_some() {
        return DeskScan::default();
    }
    scan()
}

fn desk_scan_cached() -> DeskScan {
    if let Ok(g) = LAST_DESK_SCAN.lock() {
        if let Some((at, scan)) = g.as_ref() {
            if at.elapsed() < DESK_SCAN_TTL {
                return scan.clone();
            }
        }
    }
    let scan = desk_scan_now();
    if let Ok(mut g) = LAST_DESK_SCAN.lock() {
        *g = Some((Instant::now(), scan.clone()));
    }
    scan
}

fn desk_scan_now() -> DeskScan {
    let mut atspi_cmd = Command::new("python3");
    atspi_cmd.args(["-c", ATSPI_PY]);
    let atspi = run_limited(atspi_cmd, DESK_LIST_TIMEOUT)
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default();
    let mut wmctrl_cmd = spawn_bin("wmctrl");
    wmctrl_cmd.args(["-lG"]);
    let wmctrl = run_limited(wmctrl_cmd, DESK_LIST_TIMEOUT)
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default();
    let mut rows = Vec::new();
    for line in atspi.lines() {
        if let Some(r) = parse_atspi_line(line) {
            rows.push(r);
        }
    }
    if rows.is_empty() {
        for line in wmctrl.lines() {
            if let Some(r) = parse_wmctrl_line(line) {
                rows.push(r);
            }
        }
    }
    if let Some(r) = read_cursor_row() {
        rows.push(r);
    }
    let outputs = read_display_outputs();
    let (dw, dh) = virtual_desktop_size(&outputs)
        .map(|(w, h)| (w as i32, h as i32))
        .unwrap_or((0, 0));
    DeskScan {
        rows: filter_atspi_rows(&rows, dw, dh),
        lock: lock_titles_from_stdout(&atspi, &wmctrl),
    }
}

/// Spike-2b path A: how long the click-target read may take before the
/// click counts as unknown (soft, `target:"unknown"`).
#[cfg(target_os = "linux")]
pub(crate) const ATSPI_HIT_CAP: Duration = Duration::from_millis(300);

/// Spike-2b path A: the control at a screen point, read-only, through the
/// same AT-SPI walk as the desk scan and capped at [`ATSPI_HIT_CAP`]. `None`
/// on a timeout, no python3 or pyatspi, or no control there.
#[cfg(target_os = "linux")]
pub fn atspi_target_at(x: i32, y: i32) -> Option<grokhub_core::desktop_mcp::ClickTarget> {
    let mut cmd = Command::new("python3");
    cmd.args(["-c", ATSPI_PY]);
    let out = run_limited(cmd, ATSPI_HIT_CAP).filter(|o| o.status.success())?;
    let rows: Vec<AtspiRow> = String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(parse_atspi_line)
        .collect();
    grokhub_core::control_at(&rows, x, y).map(|r| grokhub_core::desktop_mcp::ClickTarget {
        label: r.name.clone(),
        role: r.role.clone(),
    })
}

pub fn collect_rows() -> Vec<AtspiRow> {
    desk_scan().rows
}

pub fn named_row<'a>(rows: &'a [AtspiRow], name: &str) -> Option<&'a AtspiRow> {
    pick_named_row(rows, name)
}

pub fn row_center(row: &AtspiRow) -> (i32, i32) {
    (row.x + row.w / 2, row.y + row.h / 2)
}

pub fn parse_getwindowgeometry(text: &str) -> Option<(i32, i32, i32, i32)> {
    let mut px = None;
    let mut py = None;
    let mut w = None;
    let mut h = None;
    for line in text.lines() {
        let l = line.trim();
        if let Some(rest) = l.strip_prefix("Position:") {
            let rest = rest.split('(').next().unwrap_or(rest);
            let (x, y) = rest.trim().split_once(',')?;
            px = Some(x.trim().parse().ok()?);
            py = Some(y.trim().parse().ok()?);
        } else if let Some(rest) = l.strip_prefix("Geometry:") {
            let (a, b) = rest.trim().split_once('x')?;
            w = Some(a.trim().parse().ok()?);
            h = Some(b.trim().parse().ok()?);
        }
    }
    Some((px?, py?, w?, h?))
}

fn hands_receipt(line: &str, start: Instant, ok: bool, detail: &str) -> String {
    format!(
        "$ {line}\nexit {} · {}ms\n{detail}",
        if ok { 0 } else { 1 },
        start.elapsed().as_millis()
    )
}

fn cancelled(cancel: Option<&AtomicBool>) -> bool {
    cancel.is_some_and(|c| c.load(Ordering::SeqCst))
}

pub fn live_hands_backend() -> Option<HandsBackend> {
    let wayland = session_is_wayland(
        std::env::var("WAYLAND_DISPLAY").ok().as_deref(),
        std::env::var("XDG_SESSION_TYPE").ok().as_deref(),
    );
    pick_hands_backend(wayland, which("ydotool"), which("xdotool"))
}

fn run_bin_steps(
    bin: &str,
    steps: &[Vec<String>],
    cancel: Option<&AtomicBool>,
) -> Result<(), String> {
    if cancelled(cancel) {
        return Err("halted".into());
    }
    let path = resolve_bin(bin).ok_or_else(|| format!("{bin} missing"))?;
    for (i, step) in steps.iter().enumerate() {
        if cancelled(cancel) {
            return Err("halted".into());
        }
        if i > 0 {
            std::thread::sleep(Duration::from_millis(25));
        }
        let mut cmd = Command::new(&path);
        if bin == "ydotool" {
            cmd.env("YDOTOOL_SOCKET", ydotool_sock());
        }
        cmd.args(step);
        match run_limited(cmd, DESK_LIST_TIMEOUT) {
            Some(out) if out.status.success() => {}
            Some(_) => return Err(format!("{bin} {} failed", step.join(" "))),
            None => return Err(format!("{bin} {} timed out", step.join(" "))),
        }
    }
    Ok(())
}

fn run_pointer_steps(steps: &[Vec<String>], cancel: Option<&AtomicBool>) -> Result<(), String> {
    match ensure_hands() {
        HandsDown::Ready => {}
        reason => return Err(hands_down_receipt(reason).into()),
    }
    match live_hands_backend() {
        Some(HandsBackend::Ydotool) => run_bin_steps("ydotool", steps, cancel),
        Some(HandsBackend::Xdotool) => run_bin_steps("xdotool", steps, cancel),
        None => Err(hands_down_receipt(HandsDown::Missing).into()),
    }
}

fn pointer_drive(op: &ComputerOp) -> ComputerDrive {
    computer_drive_for(live_hands_backend().unwrap_or(HandsBackend::Xdotool), op)
}

const POINTER_SLOP: i32 = 8;

fn read_cursor_row() -> Option<AtspiRow> {
    let mut mouse = spawn_bin("xdotool");
    mouse.args(["getmouselocation"]);
    let out = run_limited(mouse, DESK_LIST_TIMEOUT)?;
    parse_xdotool_mouse(&String::from_utf8_lossy(&out.stdout))
}

fn read_cursor_xy() -> Option<(i32, i32)> {
    read_cursor_row().map(|r| (r.x, r.y))
}

fn cursor_monitor_name(x: i32, y: i32) -> Option<String> {
    cursor_on_output(&read_display_outputs(), x, y).map(|o| o.name.clone())
}

fn cursor_detail_line(x: i32, y: i32, miss: bool) -> String {
    format_cursor_line_miss(x, y, cursor_monitor_name(x, y).as_deref(), miss)
}

fn append_cursor_detail(detail: &str, intended: (i32, i32), actual: Option<(i32, i32)>) -> String {
    let Some((ax, ay)) = actual else {
        return format!("{detail}\ncursor unread");
    };
    let miss = pointer_slop_miss(intended, (ax, ay), POINTER_SLOP);
    format!("{detail}\n{}", cursor_detail_line(ax, ay, miss))
}

fn after_move_click_steps(op: &ComputerOp) -> Vec<Vec<String>> {
    match pointer_drive(op) {
        ComputerDrive::Xdotool(steps) | ComputerDrive::Ydotool(steps) => {
            steps.into_iter().skip(1).collect()
        }
        ComputerDrive::Act(_)
        | ComputerDrive::WaitFor(_)
        | ComputerDrive::Tab(_, _)
        | ComputerDrive::Cursor
        | ComputerDrive::MoveMonitor { .. } => vec![],
    }
}

fn drive_pointer_to(x: i32, y: i32, cancel: Option<&AtomicBool>) -> Result<(i32, i32), String> {
    let outputs = read_display_outputs();
    let (x, y) = clamp_to_desktop(x, y, &outputs);
    match pointer_drive(&ComputerOp::Move { x, y }) {
        ComputerDrive::Xdotool(steps) | ComputerDrive::Ydotool(steps) => {
            run_pointer_steps(&steps, cancel)?;
        }
        ComputerDrive::Act(_)
        | ComputerDrive::WaitFor(_)
        | ComputerDrive::Tab(_, _)
        | ComputerDrive::Cursor
        | ComputerDrive::MoveMonitor { .. } => return Err("not a pointer op".into()),
    }
    if let Some((ax, ay)) = read_cursor_xy() {
        if pointer_slop_miss((x, y), (ax, ay), POINTER_SLOP) {
            let backend = live_hands_backend().unwrap_or(HandsBackend::Xdotool);
            let steps = relative_move_steps(backend, x - ax, y - ay);
            if !steps.is_empty() {
                run_pointer_steps(&steps, cancel)?;
            }
        }
    }
    Ok((x, y))
}

pub fn lock_titles() -> Vec<String> {
    desk_scan().lock
}

pub fn lock_titles_from_stdout(atspi: &str, wmctrl: &str) -> Vec<String> {
    let lines: Vec<&str> = atspi.lines().chain(wmctrl.lines()).collect();
    grokhub_core::lock_check_titles(&lines)
}

fn click_after_move(x: i32, y: i32, cancel: Option<&AtomicBool>) -> Result<(), String> {
    let steps = after_move_click_steps(&ComputerOp::Click { x, y });
    if steps.is_empty() {
        return Ok(());
    }
    run_pointer_steps(&steps, cancel)
}

fn act_click(name: &str, cancel: Option<&AtomicBool>) -> Result<(i32, i32), String> {
    if cancelled(cancel) {
        return Err("halted".into());
    }
    let rows = collect_rows();
    if let Some(r) = named_row(&rows, name) {
        let (cx, cy) = row_center(r);
        let (x, y) = clamp_to_desktop(cx, cy, &read_display_outputs());
        drive_pointer_to(x, y, cancel)?;
        click_after_move(x, y, cancel)?;
        return Ok((x, y));
    }
    if live_hands_backend().is_none() {
        return Err(format!("act {name}: not found"));
    }
    let Some(bin) = act_window_search_bin(which("xdotool")) else {
        return Err(format!("act {name}: not found"));
    };
    let mut search = spawn_bin(bin);
    search.args(["search", "--onlyvisible", "--name", name]);
    let out =
        run_limited(search, DESK_LIST_TIMEOUT).ok_or_else(|| format!("act {name}: not found"))?;
    let id = String::from_utf8_lossy(&out.stdout)
        .lines()
        .next()
        .unwrap_or("")
        .trim()
        .to_string();
    if id.is_empty() {
        return Err(format!("act {name}: not found"));
    }
    let mut geo_cmd = spawn_bin(bin);
    geo_cmd.args(["getwindowgeometry", &id]);
    let geo = run_limited(geo_cmd, DESK_LIST_TIMEOUT)
        .ok_or_else(|| format!("act {name}: no geometry"))?;
    let text = String::from_utf8_lossy(&geo.stdout);
    let (x, y, w, h) =
        parse_getwindowgeometry(&text).ok_or_else(|| format!("act {name}: no geometry"))?;
    let (cx, cy) = clamp_to_desktop(x + w / 2, y + h / 2, &read_display_outputs());
    drive_pointer_to(cx, cy, cancel)?;
    click_after_move(cx, cy, cancel)?;
    Ok((cx, cy))
}

fn wait_for_title(title: Option<&str>, cancel: Option<&AtomicBool>) -> Result<String, String> {
    if cancelled(cancel) {
        return Err("halted".into());
    }
    let Some(want) = title.filter(|s| !s.is_empty()) else {
        std::thread::sleep(Duration::from_millis(400));
        return Ok("waited".into());
    };
    let q = want.to_ascii_lowercase();
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        if cancelled(cancel) {
            return Err("halted".into());
        }
        let rows = collect_rows();
        if rows
            .iter()
            .any(|r| r.role != "cursor" && r.name.to_ascii_lowercase().contains(&q))
        {
            return Ok(format!("saw {want}"));
        }
        if Instant::now() >= deadline {
            return Err(format!("wait_for timed out: {want}"));
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

pub fn run_computer_op_cancel(op: &ComputerOp, cancel: Option<&AtomicBool>) -> String {
    run_computer_op_under(op, cancel, crate::desktop_mcp::game_guard())
}

/// [`run_computer_op_cancel`] with the game guard the caller read. A held
/// op reads no window titles and drives nothing.
fn run_computer_op_under(op: &ComputerOp, cancel: Option<&AtomicBool>, guard: GameGuard) -> String {
    let started = Instant::now();
    let line = computer_cmd_line(op);
    if cancelled(cancel) {
        return hands_receipt(&line, started, false, "halted");
    }
    if let Some(msg) = guard.blocks(Guarded::DesktopControl) {
        return hands_receipt(&line, started, false, &format!("blocked: {msg}"));
    }
    let titles = lock_titles();
    let title_refs: Vec<&str> = titles.iter().map(|s| s.as_str()).collect();
    if hands_blocked_by_lock(op, &title_refs) {
        return hands_receipt(&line, started, false, "blocked: lock screen");
    }
    let op = map_pointer_op(op);
    #[cfg(windows)]
    if let Some(done) = crate::desktop_mcp::windows_hands(&op) {
        return match done {
            Ok(detail) => hands_receipt(&line, started, true, &detail),
            Err(e) => hands_receipt(&line, started, false, &e),
        };
    }
    match pointer_drive(&op) {
        ComputerDrive::Xdotool(steps) | ComputerDrive::Ydotool(steps) => {
            if let Some(detail) = empty_hands_steps_error(&op, &steps) {
                return hands_receipt(&line, started, false, &detail);
            }
            match &op {
                ComputerOp::Click { x, y }
                | ComputerOp::DoubleClick { x, y }
                | ComputerOp::Move { x, y } => match drive_pointer_to(*x, *y, cancel) {
                    Ok((x, y)) => {
                        let click_steps = after_move_click_steps(&op);
                        if let Err(e) = run_pointer_steps(&click_steps, cancel) {
                            return hands_receipt(&line, started, false, &e);
                        }
                        let detail = match &op {
                            ComputerOp::Click { .. } => format!("clicked {x},{y}"),
                            ComputerOp::DoubleClick { .. } => {
                                format!("double-clicked {x},{y}")
                            }
                            ComputerOp::Move { .. } => format!("moved {x},{y}"),
                            ComputerOp::Type { .. }
                            | ComputerOp::Key { .. }
                            | ComputerOp::Scroll { .. }
                            | ComputerOp::Act { .. }
                            | ComputerOp::WaitFor { .. }
                            | ComputerOp::Tab { .. }
                            | ComputerOp::Cursor
                            | ComputerOp::MoveMonitor { .. } => "ok".into(),
                        };
                        hands_receipt(
                            &line,
                            started,
                            true,
                            &append_cursor_detail(&detail, (x, y), read_cursor_xy()),
                        )
                    }
                    Err(e) => hands_receipt(&line, started, false, &e),
                },
                other => match run_pointer_steps(&steps, cancel) {
                    Ok(()) => {
                        let detail = match other {
                            ComputerOp::Type { text } => {
                                format!("typed {} chars", text.chars().count())
                            }
                            ComputerOp::Key { name } => format!("key {name}"),
                            ComputerOp::Scroll { dy } => format!("scrolled {dy}"),
                            ComputerOp::Click { .. }
                            | ComputerOp::DoubleClick { .. }
                            | ComputerOp::Move { .. }
                            | ComputerOp::Act { .. }
                            | ComputerOp::WaitFor { .. }
                            | ComputerOp::Tab { .. }
                            | ComputerOp::Cursor
                            | ComputerOp::MoveMonitor { .. } => "ok".into(),
                        };
                        hands_receipt(&line, started, true, &detail)
                    }
                    Err(e) => hands_receipt(&line, started, false, &e),
                },
            }
        }
        ComputerDrive::Act(name) => match act_click(name.as_str(), cancel) {
            Ok((x, y)) => hands_receipt(
                &line,
                started,
                true,
                &append_cursor_detail(&format!("act {name} @{x},{y}"), (x, y), read_cursor_xy()),
            ),
            Err(e) => hands_receipt(&line, started, false, &e),
        },
        ComputerDrive::WaitFor(title) => match wait_for_title(title.as_deref(), cancel) {
            Ok(detail) => hands_receipt(&line, started, true, &detail),
            Err(e) => hands_receipt(&line, started, false, &e),
        },
        ComputerDrive::Tab(action, query) => match run_tab_op(action, &query, cancel) {
            Ok(detail) => hands_receipt(&line, started, true, &detail),
            Err(e) => hands_receipt(&line, started, false, &e),
        },
        ComputerDrive::Cursor => match read_cursor_xy() {
            Some((x, y)) => hands_receipt(&line, started, true, &cursor_detail_line(x, y, false)),
            None => hands_receipt(&line, started, false, "cursor unread"),
        },
        ComputerDrive::MoveMonitor { name, x, y } => {
            let local = match (x, y) {
                (Some(a), Some(b)) => Some((a, b)),
                _ => None,
            };
            match monitor_local_to_global(&read_display_outputs(), &name, local) {
                Some((gx, gy)) => match drive_pointer_to(gx, gy, cancel) {
                    Ok((gx, gy)) => hands_receipt(
                        &line,
                        started,
                        true,
                        &append_cursor_detail(
                            &format!("moved {gx},{gy}"),
                            (gx, gy),
                            read_cursor_xy(),
                        ),
                    ),
                    Err(e) => hands_receipt(&line, started, false, &e),
                },
                None => hands_receipt(&line, started, false, &format!("unknown monitor {name}")),
            }
        }
    }
}

const CAPTURE_BINS: &[&str] = &[
    "grim",
    "gnome-screenshot",
    "spectacle",
    "gdbus",
    "maim",
    "scrot",
    "import",
    "ffmpeg",
];

fn pin_wayland_for_capture() {
    if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        return;
    }
    let runtime = std::env::var("XDG_RUNTIME_DIR").ok();
    if let Some(name) = infer_wayland_display(None, runtime.as_deref()) {
        std::env::set_var("WAYLAND_DISPLAY", name);
    }
}

fn x11_size() -> (u32, u32) {
    let xdpy = run_limited(Command::new("xdpyinfo"), DESK_LIST_TIMEOUT)
        .and_then(|o| String::from_utf8(o.stdout).ok());
    let mut xrandr_cmd = Command::new("xrandr");
    xrandr_cmd.arg("-q");
    let xrandr =
        run_limited(xrandr_cmd, DESK_LIST_TIMEOUT).and_then(|o| String::from_utf8(o.stdout).ok());
    x11_grab_size(xdpy.as_deref(), xrandr.as_deref())
}

fn run_capture_kind(
    kind: CaptureKind,
    dest: &Path,
    grim_output: Option<&str>,
) -> Result<(PathBuf, Option<String>), String> {
    let jpg = dest.to_path_buf();
    let png = dest.with_extension("png");
    match kind {
        CaptureKind::Grim => {
            let p = png.to_string_lossy().to_string();
            if let Some(name) = grim_output {
                let args = grim_capture_args(&p, Some(name));
                let refs: Vec<&str> = args.iter().map(String::as_str).collect();
                if run_ok("grim", &refs).is_ok() {
                    return Ok((png, Some(name.to_string())));
                }
            }
            let args = grim_capture_args(&p, None);
            let refs: Vec<&str> = args.iter().map(String::as_str).collect();
            run_ok("grim", &refs)?;
            Ok((png, None))
        }
        CaptureKind::GnomeScreenshot => {
            let p = png.to_string_lossy().to_string();
            run_ok("gnome-screenshot", &["-f", &p])?;
            Ok((png, None))
        }
        CaptureKind::Spectacle => {
            let p = png.to_string_lossy().to_string();
            run_ok("spectacle", &["-b", "-n", "-o", &p])?;
            Ok((png, None))
        }
        CaptureKind::GnomeShell => {
            let p = png.to_string_lossy().to_string();
            let args = gnome_shell_screenshot_args(&p);
            let refs: Vec<&str> = args.iter().map(String::as_str).collect();
            run_ok("gdbus", &refs)?;
            Ok((png, None))
        }
        CaptureKind::Maim => {
            let p = png.to_string_lossy().to_string();
            run_ok("maim", &[&p])?;
            Ok((png, None))
        }
        CaptureKind::Scrot => {
            let p = jpg.to_string_lossy().to_string();
            run_ok("scrot", &["-o", &p])?;
            Ok((jpg, None))
        }
        CaptureKind::Import => {
            let p = png.to_string_lossy().to_string();
            run_ok("import", &["-window", "root", &p])?;
            Ok((png, None))
        }
        CaptureKind::FfmpegX11 => {
            let display = std::env::var("DISPLAY").unwrap_or_else(|_| ":0".into());
            let (w, h) = x11_size();
            let p = jpg.to_string_lossy().to_string();
            let args = ffmpeg_x11_args(&display, w, h, &p);
            let refs: Vec<&str> = args.iter().map(String::as_str).collect();
            run_ok("ffmpeg", &refs)?;
            Ok((jpg, None))
        }
    }
}

fn image_pixels_ok_for_path(path: &Path) -> Result<(), String> {
    let mut hdr = [0u8; 24];
    if let Ok(n) = std::fs::File::open(path).and_then(|mut f| f.read(&mut hdr)) {
        if let Some((w, h)) = png_ihdr_size(&hdr[..n]) {
            if !image_pixels_ok(w, h) {
                return Err("image too large".into());
            }
            return Ok(());
        }
    }
    let (w, h) = image::image_dimensions(path).map_err(|e| e.to_string())?;
    if !image_pixels_ok(w, h) {
        return Err("image too large".into());
    }
    Ok(())
}

pub(crate) fn image_pixels_ok_for_bytes(bytes: &[u8]) -> bool {
    if let Some((w, h)) = png_ihdr_size(bytes) {
        return image_pixels_ok(w, h);
    }
    image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .ok()
        .and_then(|r| r.into_dimensions().ok())
        .map(|(w, h)| image_pixels_ok(w, h))
        .unwrap_or(true)
}

fn image_file_to_jpeg(path: &Path) -> Result<Vec<u8>, String> {
    let len = std::fs::metadata(path).map(|m| m.len()).unwrap_or(u64::MAX);
    if len > IMAGE_FILE_CAP {
        return Err("image too large".into());
    }
    image_pixels_ok_for_path(path)?;
    let img = image::open(path).map_err(|e| e.to_string())?;
    let mut buf = Vec::new();
    let mut cur = std::io::Cursor::new(&mut buf);
    img.write_to(&mut cur, image::ImageFormat::Jpeg)
        .map_err(|e| e.to_string())?;
    if buf.len() < 32 {
        return Err("empty frame".into());
    }
    Ok(buf)
}

pub fn frame_bytes_are_blank(bytes: &[u8]) -> bool {
    if !image_pixels_ok_for_bytes(bytes) {
        return false;
    }
    let Ok(img) = image::load_from_memory(bytes) else {
        return false;
    };
    let rgb = img.to_rgb8();
    let w = rgb.width().max(1);
    let h = rgb.height().max(1);
    let step_x = (w / 32).max(1);
    let step_y = (h / 32).max(1);
    let mut samples = Vec::new();
    for y in (0..h).step_by(step_y as usize) {
        for x in (0..w).step_by(step_x as usize) {
            let p = rgb.get_pixel(x, y);
            samples.push([p[0], p[1], p[2]]);
        }
    }
    let (mean, var) = luma_mean_var(&samples);
    frame_is_blank(mean, var)
}

/// A captured RGBA screen as a JPEG for Eyes: alpha dropped, capped at
/// [`IMAGE_FILE_CAP`], and an all-black frame (a locked or secure desktop)
/// refused like the Linux grabbers' black frames.
#[cfg(any(windows, test))]
fn rgba_frame_to_jpeg(frame: image::RgbaImage) -> Result<Vec<u8>, String> {
    let rgb = image::DynamicImage::ImageRgba8(frame).into_rgb8();
    let mut buf = Vec::new();
    rgb.write_to(
        &mut std::io::Cursor::new(&mut buf),
        image::ImageFormat::Jpeg,
    )
    .map_err(|e| e.to_string())?;
    if buf.len() as u64 > IMAGE_FILE_CAP {
        return Err("screen frame too large".into());
    }
    if frame_bytes_are_blank(&buf) {
        return Err("the screen was a black frame".into());
    }
    Ok(buf)
}

pub fn capture_jpeg(path: &Path) -> Result<Vec<u8>, String> {
    capture_jpeg_under(path, crate::desktop_mcp::game_guard())
}

/// [`capture_jpeg`] with the game guard the caller read. A held capture
/// grabs no frame and writes no file.
fn capture_jpeg_under(path: &Path, guard: GameGuard) -> Result<Vec<u8>, String> {
    if let Some(msg) = guard.blocks(Guarded::ScreenCapture) {
        return Err(msg);
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    #[cfg(windows)]
    let win_err = match crate::desktop_mcp::windows_primary_frame().and_then(rgba_frame_to_jpeg) {
        Ok(jpeg) => {
            let _ = std::fs::write(path, &jpeg);
            return Ok(jpeg);
        }
        Err(e) => e,
    };
    pin_wayland_for_capture();
    let bins: Vec<&str> = CAPTURE_BINS.iter().copied().filter(|n| which(n)).collect();
    let wayland = session_is_wayland(
        std::env::var("WAYLAND_DISPLAY").ok().as_deref(),
        std::env::var("XDG_SESSION_TYPE").ok().as_deref(),
    );
    let x11 = std::env::var_os("DISPLAY").is_some();
    let plan = capture_kinds(&bins, wayland, x11);
    if plan.is_empty() {
        #[cfg(windows)]
        let why = format!("Windows screen capture failed: {win_err}");
        #[cfg(not(windows))]
        let why = "no grim/gnome-screenshot/ffmpeg/scrot for a desktop frame".to_string();
        return Err(why);
    }
    let outputs = read_display_outputs();
    let points: Vec<(i32, i32)> = collect_rows()
        .iter()
        .filter(|r| r.role != "cursor")
        .map(|r| (r.x + r.w / 2, r.y + r.h / 2))
        .collect();
    let grim_out = pick_capture_output(&outputs, &points).map(|o| o.name.clone());
    let mut last = "no desktop frame".to_string();
    for kind in plan {
        match run_capture_kind(kind, path, grim_out.as_deref()) {
            Ok((written, captured_output)) => {
                let jpeg = if written
                    .extension()
                    .and_then(|s| s.to_str())
                    .is_some_and(|e| e == "jpg" || e == "jpeg")
                {
                    let len = std::fs::metadata(&written)
                        .map(|m| m.len())
                        .unwrap_or(u64::MAX);
                    if len > IMAGE_FILE_CAP {
                        last = format!("{kind:?} too large");
                        let _ = std::fs::remove_file(&written);
                        continue;
                    }
                    match std::fs::read(&written) {
                        Ok(b) if b.len() >= 32 => b,
                        Ok(_) => {
                            last = format!("{kind:?} empty");
                            let _ = std::fs::remove_file(&written);
                            continue;
                        }
                        Err(e) => {
                            last = e.to_string();
                            let _ = std::fs::remove_file(&written);
                            continue;
                        }
                    }
                } else {
                    match image_file_to_jpeg(&written) {
                        Ok(b) => b,
                        Err(e) => {
                            last = e;
                            let _ = std::fs::remove_file(&written);
                            continue;
                        }
                    }
                };
                if written != path {
                    let _ = std::fs::remove_file(&written);
                }
                if frame_bytes_are_blank(&jpeg) {
                    last = format!("{kind:?} was a black frame");
                    continue;
                }
                remember_from_jpeg(&jpeg, &outputs, captured_output.as_deref());
                let _ = std::fs::write(path, &jpeg);
                return Ok(jpeg);
            }
            Err(e) => last = e,
        }
    }
    Err(last)
}

pub fn capture_data_url() -> Result<String, String> {
    let path = capture_temp("desk", "jpg");
    let bytes = capture_jpeg(&path)?;
    let _ = std::fs::remove_file(&path);
    if bytes.len() < 32 {
        return Err("empty frame".into());
    }
    Ok(jpeg_data_url(&bytes))
}

/// Make `dir` for this user alone: it must not exist yet, and on Unix only
/// the owner can enter it. A shared temp folder lets anyone pre-create a
/// fixed name and then read or swap what is written inside.
pub(crate) fn create_private_dir(dir: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        std::fs::DirBuilder::new().mode(0o700).create(dir)
    }
    #[cfg(not(unix))]
    {
        std::fs::create_dir(dir)
    }
}

/// A file in this run's private temp folder, made on first use under a
/// random name. Voice clips, pasted pictures and spoken replies go here
/// rather than under a fixed name in the shared temp folder.
pub(crate) fn private_temp(name: &str) -> Result<PathBuf, String> {
    static DIR: std::sync::OnceLock<Result<PathBuf, String>> = std::sync::OnceLock::new();
    let dir = DIR.get_or_init(|| {
        let dir = std::env::temp_dir().join(format!("grokhub-{}", grokhub_core::new_token()));
        create_private_dir(&dir)
            .map(|()| dir)
            .map_err(|e| format!("temp folder: {e}"))
    });
    dir.as_ref().map(|d| d.join(name)).map_err(Clone::clone)
}

pub const PLAYERS: &[&str] = &["ffplay", "mpv", "paplay"];

pub fn record_once() -> Result<PathBuf, String> {
    let wav = private_temp("voice.wav")?;
    record_wav(&wav)?;
    Ok(wav)
}

pub fn transcribe_local(wav: &Path) -> Result<String, String> {
    transcribe(wav)
}

pub fn play_audio(path: &Path) -> Result<(), String> {
    #[cfg(windows)]
    {
        if crate::win_audio::play_file(path).is_ok() {
            return Ok(());
        }
    }
    let dest = path.to_str().ok_or("audio path")?;
    match first_bin(PLAYERS).as_deref() {
        Some("ffplay") => run_ok(
            "ffplay",
            &["-nodisp", "-autoexit", "-loglevel", "error", dest],
        ),
        Some("mpv") => run_ok("mpv", &["--no-video", "--really-quiet", dest]),
        Some("paplay") => run_ok("paplay", &[dest]),
        _ => {
            #[cfg(windows)]
            {
                crate::win_audio::play_file(path)
            }
            #[cfg(not(windows))]
            {
                Err("no ffplay/mpv/paplay to speak".into())
            }
        }
    }
}

fn record_wav(path: &Path) -> Result<(), String> {
    #[cfg(windows)]
    {
        match crate::win_audio::record_wav(path, 4) {
            Ok(()) => return Ok(()),
            Err(e) => {
                if first_bin(RECORDERS).is_none() {
                    return Err(e);
                }
            }
        }
    }
    let dest = path.to_str().ok_or("wav path")?;
    match first_bin(RECORDERS).as_deref() {
        Some("arecord") => run_ok("arecord", &["-q", "-d", "4", "-f", "cd", "-t", "wav", dest]),
        Some("ffmpeg") => run_ok(
            "ffmpeg",
            &[
                "-y",
                "-hide_banner",
                "-loglevel",
                "error",
                "-f",
                "pulse",
                "-i",
                "default",
                "-t",
                "4",
                "-ac",
                "1",
                "-ar",
                "16000",
                dest,
            ],
        ),
        Some("sox") | Some("rec") => run_ok("rec", &[dest, "trim", "0", "4"]),
        _ => Err("no arecord/ffmpeg/sox — install alsa-utils or ffmpeg".into()),
    }
}

/// `pactl` output in the C locale, or `None` when pactl is missing or fails.
pub fn pactl_text(args: &[&str]) -> Option<String> {
    if !which("pactl") {
        return None;
    }
    let mut cmd = spawn_bin("pactl");
    cmd.args(args).env("LC_ALL", "C");
    let out = run_limited(cmd, Duration::from_secs(4))?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// The audio check's clip: `CLIP_SECS` of raw s16le mono from `device`
/// (`None` = the default input), kept in memory only. Returns the sample
/// rate and the bytes. The recorder is stopped once it has enough, or at the
/// deadline if it stalls.
pub fn record_clip(device: Option<&str>) -> Result<(u32, Vec<u8>), String> {
    use grokhub_core::audio_check as ac;
    #[cfg(windows)]
    {
        if device.is_none() {
            if let Ok(pcm) = crate::win_audio::record_pcm_secs(ac::CLIP_SECS) {
                return Ok((crate::win_audio::RATE, pcm));
            }
        }
    }
    let picked = ac::CLIP_RECORDERS
        .iter()
        .filter(|b| which(b))
        .find_map(|b| ac::clip_argv(b, device).map(|argv| (*b, argv)));
    let Some((bin, argv)) = picked else {
        return Err(if device.is_some() {
            "No recorder here can pick an input: install pulseaudio-utils (parecord) or pipewire (pw-record).".into()
        } else {
            "No recorder found: install pulseaudio-utils (parecord), pipewire (pw-record), ffmpeg or alsa-utils (arecord).".into()
        });
    };
    let want = (ac::CLIP_RATE * 2 * ac::CLIP_SECS) as usize;
    let mut cmd = spawn_bin(bin);
    cmd.args(&argv)
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    crate::host::hide_windows_console(&mut cmd);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("{bin} didn't start: {e}"))?;
    let mut out = child.stdout.take().ok_or("the recorder gave no output")?;
    let reader = std::thread::spawn(move || {
        let mut buf = Vec::with_capacity(want);
        let mut chunk = [0u8; 8192];
        while buf.len() < want {
            match out.read(&mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(n) => buf.extend_from_slice(&chunk[..n]),
            }
        }
        buf.truncate(want);
        buf
    });
    let deadline = Instant::now() + Duration::from_secs(u64::from(ac::CLIP_SECS) + 3);
    while !reader.is_finished() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
    kill_limited(&mut child);
    let _ = child.wait();
    let pcm = reader.join().unwrap_or_default();
    if pcm.len() < 64 {
        return Err(format!("{bin} recorded no sound data"));
    }
    Ok((ac::CLIP_RATE, pcm))
}

fn transcribe(wav: &Path) -> Result<String, String> {
    let dest = wav.to_str().ok_or("wav")?;
    let bin = first_bin(TRANSCRIBERS).ok_or("install whisper (openai-whisper or whisper.cpp)")?;
    let out_dir = std::env::temp_dir();
    let mut cmd = Command::new(&bin);
    match bin.as_str() {
        "whisper-cli" | "whisper.cpp" => {
            cmd.args([
                dest,
                "-otxt",
                "-of",
                out_dir
                    .join("grokhub-voice")
                    .to_str()
                    .unwrap_or("/tmp/grokhub-voice"),
            ]);
        }
        _ => {
            cmd.args([
                dest,
                "--output_format",
                "txt",
                "--output_dir",
                out_dir.to_str().unwrap_or("/tmp"),
            ]);
        }
    }
    let out = match run_limited(cmd, DESK_TRANSCRIBE_TIMEOUT) {
        Some(o) => o,
        None => return Err(format!("{bin} timed out")),
    };
    if !out.status.success() {
        return Err(format!("{bin} failed"));
    }
    let txt = wav.with_extension("txt");
    let alt = out_dir.join("grokhub-voice.txt");
    read_text_capped(&txt).or_else(|_| read_text_capped(&alt))
}

fn run_ok(bin: &str, args: &[&str]) -> Result<(), String> {
    let mut cmd = spawn_bin(bin);
    cmd.args(args);
    match run_limited(cmd, DESK_CAPTURE_TIMEOUT) {
        Some(out) if out.status.success() => Ok(()),
        Some(_) => Err(format!("{bin} failed")),
        None => Err(format!("{bin} timed out")),
    }
}

pub fn imagine_save_path_ext(slug: &str, ext: &str) -> PathBuf {
    let ext = ext.trim().trim_start_matches('.').to_ascii_lowercase();
    let ext = if ext.is_empty() { "png".into() } else { ext };
    crate::config::imagine_dir().join(format!("{slug}.{ext}"))
}

/// Second frame for a wall cover when the second Imagine call fails.
pub fn sibling_still(src: &std::path::Path, dest: &std::path::Path) -> Result<(), String> {
    let len = std::fs::metadata(src).map(|m| m.len()).unwrap_or(u64::MAX);
    if len > IMAGE_FILE_CAP {
        return Err("image too large".into());
    }
    image_pixels_ok_for_path(src)?;
    let img = image::open(src).map_err(|e| e.to_string())?;
    let (w, h) = img.dimensions();
    let x = ((w as f32) * 0.05) as u32;
    let y = ((h as f32) * 0.04) as u32;
    let cw = w.saturating_sub(x + ((w as f32) * 0.02) as u32).max(8);
    let ch = h.saturating_sub(y + ((h as f32) * 0.06) as u32).max(8);
    let crop = img
        .crop_imm(x, y, cw, ch)
        .resize_exact(w, h, image::imageops::FilterType::Lanczos3);
    if let Some(dir) = dest.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    crop.save(dest).map_err(|e| e.to_string())
}

pub fn capture_webcam() -> Result<String, String> {
    if !std::path::Path::new("/dev/video0").exists() {
        return Err("no /dev/video0".into());
    }
    if !which("ffmpeg") {
        return Err("ffmpeg missing for webcam".into());
    }
    let path = capture_temp("cam", "jpg");
    let dest = path.to_string_lossy().to_string();
    let args = ffmpeg_webcam_args("/dev/video0", &dest);
    let mut cam = Command::new("ffmpeg");
    cam.args(&args);
    let ok = run_limited(cam, DESK_WEBCAM_TIMEOUT).is_some_and(|o| o.status.success());
    if !ok {
        return Err("webcam capture failed".into());
    }
    let len = std::fs::metadata(&path)
        .map(|m| m.len())
        .unwrap_or(u64::MAX);
    if len > IMAGE_FILE_CAP {
        let _ = std::fs::remove_file(&path);
        return Err("image too large".into());
    }
    let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
    let _ = std::fs::remove_file(&path);
    if bytes.len() < 32 {
        return Err("empty webcam frame".into());
    }
    if frame_bytes_are_blank(&bytes) {
        return Err("webcam frame was black".into());
    }
    Ok(jpeg_data_url(&bytes))
}

/// Start a helper nobody waits on (an opener, a player, a notification) and reap it on a
/// thread when it exits. A dropped `Child` is never waited for, so on Linux each one stayed
/// a zombie until the cabin quit.
pub(crate) fn spawn_reaped(cmd: &mut Command) -> std::io::Result<u32> {
    let mut child = cmd.spawn()?;
    let pid = child.id();
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(pid)
}

pub fn open_path(path: &str) -> Result<(), String> {
    let path = path.trim();
    if path.is_empty() {
        return Err("empty path".into());
    }
    #[cfg(windows)]
    {
        crate::win_native::open_path(path)
    }
    #[cfg(not(windows))]
    {
        spawn_reaped(std::process::Command::new("xdg-open").arg(path))
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
}

/// Caret, pipe, backtick, backslash, braces, and quotes, plus whitespace or control.
/// Ampersand, question mark, equals, percent, and hash stay: one argument, never cmd.
fn url_shell_meta(url: &str) -> bool {
    url.chars().any(|c| {
        c.is_whitespace()
            || c.is_control()
            || matches!(c, '^' | '|' | '`' | '\\' | '{' | '}' | '\'' | '"')
    })
}

/// http(s) with a non-empty host, and no shell or cmd metacharacters.
pub fn url_safe_to_open(url: &str) -> bool {
    grokhub_core::md_link_ok(url) && !url_shell_meta(url)
}

/// Open an http(s) URL in the browser. Windows uses ShellExecuteW via
/// `win_native::open_path`. Other platforms pass the URL as one `xdg-open`
/// argument. Never `cmd` and never a shell string.
pub fn open_url(url: &str) -> Result<(), String> {
    if !url_safe_to_open(url) {
        return Err("refusing to open this URL".into());
    }
    #[cfg(windows)]
    {
        crate::win_native::open_path(url)
    }
    #[cfg(not(windows))]
    {
        spawn_reaped(std::process::Command::new("xdg-open").arg(url))
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
}

pub const VIDEO_PLAYERS: &[&str] = &["mpv", "ffplay"];

/// Play a saved Imagine clip (or open a still) without blocking the UI.
pub fn play_media(path: &str) -> Result<(), String> {
    let path = path.trim();
    if path.is_empty() {
        return Err("empty path".into());
    }
    if grokhub_core::imagine_is_video_path(path) {
        if let Some(player) = first_bin(VIDEO_PLAYERS) {
            let mut cmd = spawn_bin(&player);
            match player.as_str() {
                "mpv" => {
                    cmd.args(["--really-quiet", path]);
                }
                "ffplay" => {
                    cmd.args(["-autoexit", "-loglevel", "error", path]);
                }
                _ => return open_path(path),
            }
            if spawn_reaped(&mut cmd).is_ok() {
                return Ok(());
            }
        }
    }
    open_path(path)
}

pub fn video_poster_ready(video: &str) -> Option<String> {
    let dest = grokhub_core::imagine_video_poster_path(video);
    let len = std::fs::metadata(&dest).map(|m| m.len()).unwrap_or(0);
    if len > 32 {
        Some(dest)
    } else {
        None
    }
}

/// First-frame JPEG beside the `.mp4`. `ffmpeg` — call off the UI thread.
pub fn ensure_video_poster(video: &str) -> Option<String> {
    if let Some(dest) = video_poster_ready(video) {
        return Some(dest);
    }
    if !grokhub_core::imagine_is_video_path(video) || !which("ffmpeg") {
        return None;
    }
    let dest = grokhub_core::imagine_video_poster_path(video);
    let tmp = format!("{dest}.tmp.jpg");
    let args = grokhub_core::imagine_video_poster_args(video, &tmp);
    let mut cmd = spawn_bin("ffmpeg");
    cmd.args(&args);
    let ok = run_limited(cmd, DESK_CAPTURE_TIMEOUT).is_some_and(|o| o.status.success());
    if !ok {
        let _ = std::fs::remove_file(&tmp);
        return None;
    }
    let len = std::fs::metadata(&tmp).map(|m| m.len()).unwrap_or(0);
    if len <= 32 || len > IMAGE_FILE_CAP {
        let _ = std::fs::remove_file(&tmp);
        return None;
    }
    if std::fs::rename(&tmp, &dest).is_err() {
        let _ = std::fs::remove_file(&tmp);
        return None;
    }
    video_poster_ready(video)
}

pub fn save_file_dialog(suggested: &str) -> Option<PathBuf> {
    let name = std::path::Path::new(suggested)
        .file_name()
        .and_then(|s| s.to_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("imagine.png");
    #[cfg(windows)]
    {
        if let Some(p) = crate::win_native::save_file_dialog(name) {
            return Some(p);
        }
    }
    for bin in ["zenity", "kdialog", "yad", "qarma"] {
        let Some(args) = grokhub_core::picker_save_args(bin, name) else {
            continue;
        };
        if !which(bin) {
            continue;
        }
        if let Ok(o) = Command::new(bin).args(args).output() {
            if o.status.success() {
                if let Some(p) =
                    grokhub_core::parse_picker_stdout(&String::from_utf8_lossy(&o.stdout))
                {
                    let path = PathBuf::from(p);
                    if !path.as_os_str().is_empty() {
                        return Some(path);
                    }
                }
            }
        }
    }
    None
}

pub fn pick_file() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        if let Some(p) = crate::win_native::open_file_dialog() {
            return Some(p);
        }
    }
    for bin in ["zenity", "kdialog", "yad", "qarma"] {
        let Some(args) = picker_args(bin) else {
            continue;
        };
        if !which(bin) {
            continue;
        }
        if let Ok(o) = Command::new(bin).args(args).output() {
            if o.status.success() {
                if let Some(p) = parse_picker_stdout(&String::from_utf8_lossy(&o.stdout)) {
                    let path = PathBuf::from(p);
                    if path.exists() {
                        return Some(path);
                    }
                }
            }
        }
    }
    None
}

/// A clipboard DIB (`CF_DIB`: a bitmap header, then pixels, no file
/// header) as PNG bytes. The 14-byte BMP file header is put back in front so
/// the BMP decoder can read it.
#[cfg(any(windows, test))]
fn dib_to_png(dib: &[u8]) -> Option<Vec<u8>> {
    let u32_at = |at: usize| {
        dib.get(at..at + 4)
            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    };
    let header = u32_at(0)?;
    if header < 40 || dib.len() as u64 > IMAGE_FILE_CAP * 8 {
        return None;
    }
    let bits = u16::from_le_bytes([*dib.get(14)?, *dib.get(15)?]);
    // BI_BITFIELDS after a plain 40-byte header carries three masks.
    let masks = if header == 40 && u32_at(16)? == 3 {
        12
    } else {
        0
    };
    let colors = match u32_at(32)? {
        0 if bits <= 8 => 1u32 << bits,
        n => n,
    };
    let pixels_at = 14u32
        .checked_add(header)?
        .checked_add(masks)?
        .checked_add(colors.checked_mul(4)?)?;
    let mut bmp = Vec::with_capacity(dib.len() + 14);
    bmp.extend_from_slice(b"BM");
    bmp.extend_from_slice(&u32::try_from(dib.len() + 14).ok()?.to_le_bytes());
    bmp.extend_from_slice(&[0; 4]);
    bmp.extend_from_slice(&pixels_at.to_le_bytes());
    bmp.extend_from_slice(dib);
    if !image_pixels_ok_for_bytes(&bmp) {
        return None;
    }
    let img = image::load_from_memory_with_format(&bmp, image::ImageFormat::Bmp).ok()?;
    let mut png = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .ok()?;
    Some(png)
}

/// The Wayland socket for a tool the cabin starts. Startup drops
/// `WAYLAND_DISPLAY` for the window, so `wl-paste` would find no compositor.
fn wayland_display_for_tools() -> Option<String> {
    let runtime = std::env::var("XDG_RUNTIME_DIR").ok();
    infer_wayland_display(
        std::env::var("WAYLAND_DISPLAY").ok().as_deref(),
        runtime.as_deref(),
    )
}

pub fn clipboard_image() -> Option<PathBuf> {
    let dest = private_temp("clip.png").ok()?;
    #[cfg(windows)]
    {
        let png = crate::win_native::clipboard_image().and_then(|(bytes, is_png)| {
            if is_png {
                Some(bytes)
            } else {
                dib_to_png(&bytes)
            }
        });
        if let Some(png) = png.filter(|b| b.len() as u64 <= IMAGE_FILE_CAP) {
            if std::fs::write(&dest, &png).is_ok() {
                return Some(dest);
            }
        }
    }
    for bin in ["xclip", "wl-paste"] {
        let Some(args) = clip_image_args(bin) else {
            continue;
        };
        if !which(bin) {
            continue;
        }
        let mut cmd = Command::new(bin);
        cmd.args(args);
        if bin == "wl-paste" {
            if let Some(name) = wayland_display_for_tools() {
                cmd.env("WAYLAND_DISPLAY", name);
            }
        }
        if let Some(o) = run_limited(cmd, DESK_LIST_TIMEOUT) {
            if !o.status.success() || o.stdout.len() < 24 {
                continue;
            }
            if o.stdout.len() as u64 > IMAGE_FILE_CAP {
                continue;
            }
            let b = &o.stdout;
            let png = b[0] == 0x89 && b[1] == b'P';
            let jpg = b[0] == 0xFF && b[1] == 0xD8;
            if !png && !jpg {
                continue;
            }
            if std::fs::write(&dest, b).is_ok() {
                return Some(dest);
            }
        }
    }
    None
}

pub fn load_image_data_url(path: &Path) -> Result<String, String> {
    let len = std::fs::metadata(path).map(|m| m.len()).unwrap_or(u64::MAX);
    if len > IMAGE_FILE_CAP {
        return Err("image too large".into());
    }
    image_pixels_ok_for_path(path)?;
    let img = image::open(path).map_err(|e| e.to_string())?;
    let mut buf = Vec::new();
    let mut cur = std::io::Cursor::new(&mut buf);
    img.write_to(&mut cur, image::ImageFormat::Jpeg)
        .map_err(|e| e.to_string())?;
    if buf.len() < 32 {
        return Err("empty image".into());
    }
    Ok(jpeg_data_url(&buf))
}

pub fn read_text_capped(path: &Path) -> Result<String, String> {
    let mut f = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut buf = vec![0u8; TEXT_FILE_CAP];
    let n = f.read(&mut buf).map_err(|e| e.to_string())?;
    buf.truncate(n);
    while !buf.is_empty() && std::str::from_utf8(&buf).is_err() {
        buf.pop();
    }
    let s = String::from_utf8(buf).map_err(|e| e.to_string())?;
    Ok(take_text_body(&s))
}

pub fn clipboard_once() -> Option<String> {
    for (bin, args) in [
        ("wl-paste", &[] as &[&str]),
        ("xclip", &["-o", "-selection", "clipboard"]),
        ("xsel", &["-ob"]),
    ] {
        let mut cmd = Command::new(bin);
        cmd.args(args);
        if let Some(o) = run_limited(cmd, DESK_LIST_TIMEOUT) {
            if o.status.success() {
                let n = o.stdout.len().min(TEXT_FILE_CAP);
                let s = String::from_utf8_lossy(&o.stdout[..n]).trim().to_string();
                if !s.is_empty() {
                    return Some(s);
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_private_dir_refuses_a_folder_that_already_exists() {
        let dir = std::env::temp_dir().join(format!("gh-private-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        create_private_dir(&dir).expect("fresh folder");
        let again = create_private_dir(&dir).unwrap_err();
        assert_eq!(again.kind(), std::io::ErrorKind::AlreadyExists);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&dir).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o700);
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn private_temp_files_share_one_random_folder() {
        let voice = private_temp("voice.wav").unwrap();
        let clip = private_temp("clip.png").unwrap();
        assert_eq!(voice.parent(), clip.parent());
        assert_eq!(voice.file_name().unwrap(), "voice.wav");
        let folder = voice.parent().unwrap();
        assert_eq!(folder.parent().unwrap(), std::env::temp_dir());
        let name = folder.file_name().unwrap().to_str().unwrap();
        assert!(
            name.starts_with("grokhub-") && name.len() == "grokhub-".len() + 48,
            "{name}"
        );
        assert!(folder.is_dir());
    }

    fn png_crc(data: &[u8]) -> u32 {
        let mut crc = 0xffff_ffffu32;
        for &b in data {
            crc ^= u32::from(b);
            for _ in 0..8 {
                crc = if crc & 1 != 0 {
                    (crc >> 1) ^ 0xedb8_8320
                } else {
                    crc >> 1
                };
            }
        }
        crc ^ 0xffff_ffff
    }

    fn png_ihdr_only(width: u32, height: u32) -> Vec<u8> {
        let mut ihdr = Vec::from(*b"IHDR");
        ihdr.extend_from_slice(&width.to_be_bytes());
        ihdr.extend_from_slice(&height.to_be_bytes());
        ihdr.extend_from_slice(&[8, 2, 0, 0, 0]);
        let crc = png_crc(&ihdr);
        let mut out = vec![0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a];
        out.extend_from_slice(&13u32.to_be_bytes());
        out.extend_from_slice(&ihdr);
        out.extend_from_slice(&crc.to_be_bytes());
        out.extend_from_slice(&0u32.to_be_bytes());
        out.extend_from_slice(b"IEND");
        out.extend_from_slice(&png_crc(b"IEND").to_be_bytes());
        out
    }

    #[test]
    fn bins_are_named() {
        assert!(RECORDERS.contains(&"arecord"));
        assert!(TRANSCRIBERS.contains(&"whisper"));
        assert!(PLAYERS.contains(&"ffplay"));
        assert!(VIDEO_PLAYERS.contains(&"mpv") && VIDEO_PLAYERS.contains(&"ffplay"));
        let play = include_str!("desktop.rs");
        let play = play
            .split("pub fn play_media(")
            .nth(1)
            .and_then(|s| s.split("pub fn video_poster_ready(").next())
            .expect("play_media");
        assert!(
            play.contains("imagine_is_video_path")
                && play.contains("VIDEO_PLAYERS")
                && play.contains("open_path")
                && !play.contains("-nodisp")
                && !play.contains("--no-video"),
            "Imagine video must open a real player, not the speech path: {play}"
        );
        let poster_fn = include_str!("desktop.rs");
        let poster_fn = poster_fn
            .split("pub fn ensure_video_poster(")
            .nth(1)
            .and_then(|s| s.split("/// Short PCM").next())
            .expect("ensure_video_poster");
        assert!(
            poster_fn.contains(".tmp.jpg") && poster_fn.contains("run_limited"),
            "ffmpeg needs a .jpg suffix to mux the Imagine poster: {poster_fn}"
        );
        assert!(first_bin(&["definitely-not-a-bin-grokhub"]).is_none());
        assert!(hands_down_receipt(HandsDown::Missing).contains("lib/grokhub/bin"));
        assert!(hands_down_receipt(HandsDown::Uinput).contains("uinput"));
        assert!(hands_down_receipt(HandsDown::Daemon).contains("ydotoold"));
        assert_ne!(
            hands_down_receipt(HandsDown::Missing),
            hands_down_receipt(HandsDown::Daemon)
        );
        let a = grokhub_core::live_pcm_argv("arecord").unwrap();
        assert!(a.contains(&"raw"));
        assert!(play_media("").is_err());
        assert!(video_poster_ready("/tmp/still.png").is_none());
        assert!(ensure_video_poster("/tmp/still.png").is_none());
        if which("ffmpeg") {
            let dir = std::env::temp_dir().join(format!("grokhub-poster-{}", std::process::id()));
            let _ = std::fs::create_dir_all(&dir);
            let mp4 = dir.join("clip.mp4");
            let dest = mp4.to_string_lossy().to_string();
            let mut cmd = spawn_bin("ffmpeg");
            cmd.args([
                "-y",
                "-hide_banner",
                "-loglevel",
                "error",
                "-f",
                "lavfi",
                "-i",
                "color=c=red:s=160x90:d=1",
                "-pix_fmt",
                "yuv420p",
                &dest,
            ]);
            let ok = run_limited(cmd, DESK_CAPTURE_TIMEOUT).is_some_and(|o| o.status.success());
            assert!(ok, "ffmpeg color source for Imagine poster");
            let bytes = std::fs::read(&mp4).expect("mp4");
            assert_eq!(grokhub_core::media_ext_from_bytes(&bytes, "png"), "mp4");
            let poster = ensure_video_poster(&dest).expect("poster");
            let jpeg = std::fs::read(&poster).expect("poster jpeg");
            assert!(jpeg.len() > 32 && jpeg[0] == 0xFF && jpeg[1] == 0xD8 && jpeg[2] == 0xFF);
            let _ = std::fs::remove_dir_all(&dir);
        }
    }

    #[test]
    fn frame_decode_rejects_a_pixel_bomb() {
        let src = include_str!("desktop.rs");
        let remember = src
            .split("fn remember_from_jpeg(")
            .nth(1)
            .and_then(|s| s.split("fn map_pointer_xy(").next())
            .expect("remember_from_jpeg");
        assert!(
            remember.contains("image_pixels_ok"),
            "desk-frame remember must not decode a pixel bomb: {remember}"
        );
        let blank = src
            .split("pub fn frame_bytes_are_blank(")
            .nth(1)
            .and_then(|s| s.split("pub fn capture_jpeg(").next())
            .expect("frame_bytes_are_blank");
        assert!(
            blank.contains("image_pixels_ok"),
            "blank-frame check must not decode a pixel bomb: {blank}"
        );
        let mut hdr = vec![0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a];
        hdr.extend_from_slice(&13u32.to_be_bytes());
        hdr.extend_from_slice(b"IHDR");
        hdr.extend_from_slice(&50_000u32.to_be_bytes());
        hdr.extend_from_slice(&50_000u32.to_be_bytes());
        assert!(!frame_bytes_are_blank(&hdr));
    }

    #[test]
    fn black_jpeg_is_a_blank_frame() {
        let black = image::RgbImage::from_pixel(24, 24, image::Rgb([0, 0, 0]));
        let mut buf = Vec::new();
        image::DynamicImage::ImageRgb8(black)
            .write_to(
                &mut std::io::Cursor::new(&mut buf),
                image::ImageFormat::Jpeg,
            )
            .unwrap();
        assert!(frame_bytes_are_blank(&buf));
        let color = image::RgbImage::from_pixel(24, 24, image::Rgb([80, 140, 200]));
        buf.clear();
        image::DynamicImage::ImageRgb8(color)
            .write_to(
                &mut std::io::Cursor::new(&mut buf),
                image::ImageFormat::Jpeg,
            )
            .unwrap();
        assert!(!frame_bytes_are_blank(&buf));
    }

    #[test]
    fn loads_jpeg_data_url_from_png() {
        let dir = std::env::temp_dir().join("grokhub-attach-test");
        let _ = std::fs::create_dir_all(&dir);
        let p = dir.join("dot.png");
        let img = image::RgbImage::from_pixel(2, 2, image::Rgb([10, 20, 30]));
        img.save(&p).unwrap();
        let url = load_image_data_url(&p).unwrap();
        assert!(url.starts_with("data:image/jpeg;base64,"));
        let txt = dir.join("note.txt");
        std::fs::write(&txt, "hello cabin").unwrap();
        assert_eq!(read_text_capped(&txt).unwrap(), "hello cabin");
    }

    #[test]
    fn load_image_data_url_rejects_a_huge_file() {
        let src = include_str!("desktop.rs");
        let load = src
            .split("pub fn load_image_data_url(")
            .nth(1)
            .and_then(|s| s.split("pub fn read_text_capped(").next())
            .expect("load_image_data_url");
        let meta = load.find("metadata").expect("size check before decode");
        let open = load.find("image::open").expect("decode");
        assert!(
            meta < open && load.contains("IMAGE_FILE_CAP"),
            "a huge attach must not decode on the UI thread: {load}"
        );
        let dir = std::env::temp_dir().join("grokhub-img-cap-test");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("huge.bin");
        std::fs::write(&path, vec![0u8; (IMAGE_FILE_CAP as usize) + 1]).unwrap();
        assert_eq!(load_image_data_url(&path).unwrap_err(), "image too large");
        assert!(
            load.contains("IMAGE_PIXEL_CAP") || load.contains("image_pixels_ok"),
            "a tiny PNG with huge pixels must not decode on the UI thread: {load}"
        );
        let bomb = dir.join("bomb.png");
        std::fs::write(&bomb, png_ihdr_only(50_000, 50_000)).unwrap();
        assert_eq!(load_image_data_url(&bomb).unwrap_err(), "image too large");
    }

    #[test]
    fn read_text_capped_does_not_slurp_the_whole_file() {
        let src = include_str!("desktop.rs");
        let read = src
            .split("pub fn read_text_capped(")
            .nth(1)
            .and_then(|s| s.split("pub fn clipboard_once(").next())
            .expect("read_text_capped");
        assert!(
            !read.contains("read_to_string"),
            "attaching a huge log must not load the whole file on the UI thread: {read}"
        );
        assert!(
            read.contains("TEXT_FILE_CAP"),
            "capped attach must stop at TEXT_FILE_CAP: {read}"
        );
        let dir = std::env::temp_dir().join("grokhub-cap-test");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("huge.txt");
        std::fs::write(&path, "x".repeat(grokhub_core::TEXT_FILE_CAP + 4096)).unwrap();
        let out = read_text_capped(&path).unwrap();
        assert_eq!(out.len(), grokhub_core::TEXT_FILE_CAP);
    }

    #[test]
    fn named_row_center_and_geometry() {
        let rows = vec![
            AtspiRow {
                name: "Firefox".into(),
                role: "frame".into(),
                x: 1920,
                y: 0,
                w: 1920,
                h: 1080,
            },
            AtspiRow {
                name: "Close".into(),
                role: "push button".into(),
                x: 3720,
                y: 12,
                w: 16,
                h: 16,
            },
            AtspiRow {
                name: "Save".into(),
                role: "push button".into(),
                x: 10,
                y: 20,
                w: 80,
                h: 40,
            },
        ];
        let r = named_row(&rows, "save").unwrap();
        assert_eq!(row_center(r), (50, 40));
        let close = named_row(&rows, "Close").unwrap();
        assert_eq!(row_center(close), (3728, 20));
        assert_ne!(row_center(named_row(&rows, "Firefox").unwrap()), (3728, 20));
        let g = parse_getwindowgeometry(
            "Window 1\n  Position: 10,20 (screen: 0)\n  Geometry: 100x40\n",
        )
        .unwrap();
        assert_eq!(g, (10, 20, 100, 40));
    }

    #[test]
    fn lock_titles_include_filtered_lock_windows() {
        let titles = lock_titles_from_stdout(
            "role=window name=GrokHub x=10 y=20 w=800 h=600\n",
            "0x02 0 0 0 1920 1080 Lock screen\n0x01 0 10 20 800 600 Terminal\n",
        );
        assert!(titles.iter().any(|t| t.eq_ignore_ascii_case("Lock screen")));
        assert!(grokhub_core::hands_blocked_by_lock(
            &ComputerOp::Click { x: 10, y: 20 },
            &titles.iter().map(|s| s.as_str()).collect::<Vec<_>>(),
        ));
    }

    #[test]
    fn the_game_guard_holds_every_computer_cmd_and_the_eyes_capture() {
        let gaming = GameGuard {
            anticheat: None,
            gaming: true,
            idle_anticheat: None,
        };
        let ops = [
            ComputerOp::Click { x: 10, y: 20 },
            ComputerOp::Type { text: "hi".into() },
            ComputerOp::Key {
                name: "ctrl+l".into(),
            },
            ComputerOp::Act {
                name: "Save".into(),
            },
            ComputerOp::Tab {
                action: TabAction::List,
                query: String::new(),
            },
            ComputerOp::Cursor,
        ];
        for op in &ops {
            let started = Instant::now();
            let out = run_computer_op_under(op, None, gaming);
            assert!(out.contains("exit 1"), "{out}");
            assert!(
                out.ends_with("\nblocked: Desktop control is paused while you're gaming so the game's anti-cheat doesn't flag this PC."),
                "{out}"
            );
            assert!(
                started.elapsed() < Duration::from_secs(1),
                "{op:?} did work"
            );
        }
        let eac = GameGuard {
            anticheat: Some("Easy Anti-Cheat"),
            gaming: true,
            idle_anticheat: None,
        };
        let out = run_computer_op_under(&ComputerOp::Click { x: 1, y: 2 }, None, eac);
        assert!(
            out.ends_with("\nblocked: Desktop control is blocked while Easy Anti-Cheat is running so it doesn't flag this PC."),
            "{out}"
        );
        let dir = std::env::temp_dir().join(format!("grokhub-guard-shot-{}", std::process::id()));
        let path = dir.join("desk.jpg");
        assert_eq!(
            capture_jpeg_under(&path, eac),
            Err("Screen capture is blocked while Easy Anti-Cheat is running so it doesn't flag this PC.".to_string())
        );
        assert_eq!(
            capture_jpeg_under(&path, gaming),
            Err("Screen capture is paused while you're gaming so the game's anti-cheat doesn't flag this PC.".to_string())
        );
        assert!(!dir.exists(), "a held capture writes nothing");
    }

    #[test]
    fn an_anticheat_stops_the_shared_window_scan_for_every_caller() {
        let eac = GameGuard {
            anticheat: Some("Easy Anti-Cheat"),
            gaming: true,
            idle_anticheat: None,
        };
        let seen = || DeskScan {
            rows: vec![AtspiRow {
                name: "Game".into(),
                role: "frame".into(),
                x: 0,
                y: 0,
                w: 10,
                h: 10,
            }],
            lock: vec!["Lock".into()],
        };
        let held = desk_scan_under(eac, || panic!("no window scan while an anti-cheat runs"));
        assert!(held.rows.is_empty() && held.lock.is_empty());
        // Gaming alone pauses desktop control, not window checks.
        let gaming = GameGuard {
            anticheat: None,
            gaming: true,
            idle_anticheat: None,
        };
        assert_eq!(desk_scan_under(gaming, seen).lock, ["Lock"]);
        assert_eq!(desk_scan_under(GameGuard::default(), seen).rows.len(), 1);
        // collect_rows and lock_titles share the guarded scan.
        let src = include_str!("desktop.rs");
        let scan = src
            .split("\nfn desk_scan() -> DeskScan {")
            .nth(1)
            .and_then(|s| s.split("\n}").next())
            .expect("desk_scan");
        assert!(scan.contains("desk_scan_under(crate::desktop_mcp::game_guard()"));
    }

    #[test]
    fn halt_skips_wait_for_without_driving() {
        let stop = AtomicBool::new(true);
        let started = Instant::now();
        let out = run_computer_op_cancel(
            &ComputerOp::WaitFor {
                title: Some("definitely-not-a-grokhub-window".into()),
            },
            Some(&stop),
        );
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "{:?}",
            started.elapsed()
        );
        assert!(out.contains("halted"), "{out}");
        assert!(out.contains("exit 1"), "{out}");
    }

    #[test]
    fn hands_move_pointer_when_display() {
        // A live move follows the real pointer. XWayland on a scaled desktop
        // reports a different pixel than the one we asked for, and cargo test
        // must not drag the pointer. Set GROKHUB_HANDS_LIVE=1 to run it.
        if std::env::var("GROKHUB_HANDS_LIVE").is_ok()
            && std::env::var("DISPLAY").is_ok()
            && which("xdotool")
        {
            let dest_x = 1500;
            let dest_y = 400;
            let out = run_computer_op_cancel(
                &ComputerOp::Move {
                    x: dest_x,
                    y: dest_y,
                },
                None,
            );
            assert!(out.contains("exit 0"), "{out}");
            assert!(out.contains("moved 1500,400"), "{out}");
            assert!(out.contains("cursor"), "{out}");
            let loc = Command::new("xdotool")
                .args(["getmouselocation"])
                .output()
                .unwrap();
            let row = parse_xdotool_mouse(&String::from_utf8_lossy(&loc.stdout)).unwrap();
            assert_eq!(
                (row.x, row.y),
                (dest_x, dest_y),
                "{out} {} {}",
                row.x,
                row.y
            );
            let cursor = run_computer_op_cancel(&ComputerOp::Cursor, None);
            assert!(cursor.contains("exit 0"), "{cursor}");
            assert!(cursor.contains("cursor"), "{cursor}");
        }
        match grokhub_core::computer_drive(&ComputerOp::Click { x: 1, y: 2 }) {
            ComputerDrive::Xdotool(steps) => {
                assert!(!steps.iter().any(|s| s.iter().any(|a| a == "--sync")));
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn act_fallback_does_not_spawn_missing_xdotool() {
        assert!(
            include_str!("desktop.rs").contains("act_window_search_bin"),
            "act must not spawn xdotool when it is missing"
        );
        assert_eq!(act_window_search_bin(false), None);
        assert_eq!(act_window_search_bin(true), Some("xdotool"));
    }

    #[test]
    fn run_limited_kills_a_hung_desktop_command() {
        let mut cmd = Command::new("sleep");
        cmd.arg("30");
        let started = Instant::now();
        let out = run_limited(cmd, Duration::from_millis(250));
        assert!(
            out.is_none(),
            "hung desktop spawn must time out, got {out:?}"
        );
        // `sleep 30` is the child. Windows `taskkill` startup on a loaded
        // runner can take a few seconds; the bound only has to prove the UI
        // thread returned before that child finished.
        let kill_budget = if cfg!(windows) {
            Duration::from_secs(12)
        } else {
            Duration::from_secs(3)
        };
        assert!(
            started.elapsed() < kill_budget,
            "UI-thread desktop spawn must not wait out the child: {:?}",
            started.elapsed()
        );
        let limited = include_str!("desktop.rs")
            .split("pub(crate) fn run_limited(")
            .nth(1)
            .and_then(|s| s.split("\nfn remember_desk_frame(").next())
            .expect("run_limited");
        assert!(
            !limited.contains("read_to_end"),
            "a huge clipboard dump must not slurp the whole pipe on the UI thread: {limited}"
        );
        assert!(
            limited.contains("IMAGE_FILE_CAP"),
            "run_limited must stop reading past IMAGE_FILE_CAP: {limited}"
        );
        let mut dump = Command::new("head");
        dump.args([
            "-c",
            &(IMAGE_FILE_CAP + 2 * 1024 * 1024).to_string(),
            "/dev/zero",
        ]);
        let read_budget = Duration::from_secs(if cfg!(windows) { 12 } else { 3 });
        let out = run_limited(dump, read_budget).expect("head exited");
        assert!(
            out.stdout.len() as u64 <= IMAGE_FILE_CAP + 1,
            "huge stdout must stay capped, got {}",
            out.stdout.len()
        );
    }

    #[test]
    fn capture_paths_must_not_share_one_temp_file() {
        let src = include_str!("desktop.rs");
        let desk = src
            .split("pub fn capture_data_url(")
            .nth(1)
            .and_then(|s| s.split("pub const PLAYERS").next())
            .expect("capture_data_url");
        assert!(
            !desk.contains("\"grokhub-desk.jpg\"") && desk.contains("capture_temp("),
            "live presence and chat capture must not write the same JPEG: {desk}"
        );
        let cam = src
            .split("pub fn capture_webcam(")
            .nth(1)
            .and_then(|s| s.split("\npub fn save_file_dialog(").next())
            .expect("capture_webcam");
        assert!(
            !cam.contains("\"grokhub-cam.jpg\"") && cam.contains("capture_temp("),
            "webcam capture must not collide with a second ffmpeg: {cam}"
        );
    }

    #[test]
    fn live_room_desktop_spawns_must_time_out() {
        let src = include_str!("desktop.rs");
        let scan = src
            .split("fn desk_scan_now(")
            .nth(1)
            .and_then(|s| s.split("\npub fn collect_rows(").next())
            .expect("desk_scan_now");
        assert!(
            scan.contains("run_limited(") && !scan.contains(".output()"),
            "desk scan must kill hung ATSPI/wmctrl: {scan}"
        );
        let collect = src
            .split("pub fn collect_rows(")
            .nth(1)
            .and_then(|s| s.split("\npub fn named_row").next())
            .expect("collect_rows");
        assert!(
            collect.contains("desk_scan("),
            "collect_rows must reuse the shared desk scan: {collect}"
        );
        let lock = src
            .split("pub fn lock_titles(")
            .nth(1)
            .and_then(|s| s.split("\npub fn lock_titles_from_stdout").next())
            .expect("lock_titles");
        assert!(
            lock.contains("desk_scan("),
            "lock_titles must reuse the shared desk scan: {lock}"
        );
        let cam = src
            .split("pub fn capture_webcam(")
            .nth(1)
            .and_then(|s| s.split("\n/// Short PCM").next())
            .expect("capture_webcam");
        assert!(
            cam.contains("run_limited("),
            "webcam ffmpeg must time out: {cam}"
        );
        assert!(
            !cam.contains(".status()"),
            "webcam ffmpeg must not block the UI on Command::status: {cam}"
        );
        let run_ok = src
            .split("fn run_ok(")
            .nth(1)
            .and_then(|s| s.split("\npub fn imagine_save_path_ext(").next())
            .expect("run_ok");
        assert!(
            run_ok.contains("run_limited("),
            "screenshot bins must time out: {run_ok}"
        );
        let cap = src
            .split("fn run_capture_kind(")
            .nth(1)
            .and_then(|s| s.split("\nfn image_file_to_jpeg").next())
            .expect("run_capture_kind");
        assert!(
            !cap.contains(".status()"),
            "capture bins must not block the UI on Command::status: {cap}"
        );
        let xrandr = src
            .split("pub fn read_display_outputs(")
            .nth(1)
            .and_then(|s| s.split("\npub fn prepare_windshield").next())
            .expect("read_display_outputs");
        assert!(
            xrandr.contains("run_limited("),
            "xrandr must time out: {xrandr}"
        );
        let steps = src
            .split("fn run_bin_steps(")
            .nth(1)
            .and_then(|s| s.split("\nfn run_pointer_steps").next())
            .expect("run_bin_steps");
        assert!(
            steps.contains("run_limited(") && !steps.contains(".status()"),
            "hung xdotool/ydotool must not freeze the UI: {steps}"
        );
        let clip = src
            .split("pub fn clipboard_once(")
            .nth(1)
            .and_then(|s| s.split("\n#[cfg(test)]").next())
            .expect("clipboard_once");
        assert!(
            clip.contains("run_limited(") && !clip.contains(".output()"),
            "clipboard paste must time out: {clip}"
        );
        let img = src
            .split("pub fn clipboard_image(")
            .nth(1)
            .and_then(|s| s.split("\npub fn load_image_data_url").next())
            .expect("clipboard_image");
        assert!(
            img.contains("run_limited(") && !img.contains(".output()"),
            "clipboard image paste must time out: {img}"
        );
        assert!(
            img.contains("IMAGE_FILE_CAP"),
            "clipboard image paste must not keep a huge bitmap: {img}"
        );
        let text = src
            .split("pub fn clipboard_once(")
            .nth(1)
            .and_then(|s| s.split("\n#[cfg(test)]").next())
            .expect("clipboard_once");
        assert!(
            text.contains("TEXT_FILE_CAP"),
            "clipboard text paste must not keep a huge paste: {text}"
        );
        let ydo = src
            .split("fn start_ydotoold(")
            .nth(1)
            .and_then(|s| s.split("\nfn hands_facts(").next())
            .expect("start_ydotoold");
        assert!(
            ydo.contains("run_limited(") && !ydo.contains(".status()"),
            "systemctl start ydotoold must not freeze hands: {ydo}"
        );
        let tr = src
            .split("fn transcribe(")
            .nth(1)
            .and_then(|s| s.split("\nfn run_ok(").next())
            .expect("transcribe");
        assert!(
            tr.contains("run_limited(") && !tr.contains(".status()"),
            "whisper must not hang forever: {tr}"
        );
        assert!(
            tr.contains("read_text_capped") && !tr.contains("read_to_string"),
            "whisper must not slurp a huge transcript: {tr}"
        );
    }

    #[test]
    fn sibling_still_rejects_a_huge_file() {
        let src = include_str!("desktop.rs");
        let still = src
            .split("pub fn sibling_still(")
            .nth(1)
            .and_then(|s| s.split("pub fn capture_webcam(").next())
            .expect("sibling_still");
        let meta = still.find("metadata").expect("size check before decode");
        let open = still.find("image::open").expect("decode");
        assert!(
            meta < open && still.contains("IMAGE_FILE_CAP"),
            "wall cover fallback must not decode a huge still: {still}"
        );
        let jpeg = src
            .split("fn image_file_to_jpeg(")
            .nth(1)
            .and_then(|s| s.split("pub fn frame_bytes_are_blank(").next())
            .expect("image_file_to_jpeg");
        let jmeta = jpeg.find("metadata").expect("jpeg size check");
        let jopen = jpeg.find("image::open").expect("jpeg decode");
        assert!(
            jmeta < jopen && jpeg.contains("IMAGE_FILE_CAP"),
            "capture JPEG convert must not decode a huge file: {jpeg}"
        );
        assert!(
            still.contains("image_pixels_ok") && jpeg.contains("image_pixels_ok"),
            "a tiny still with huge pixels must not decode on the UI thread: {still} {jpeg}"
        );
    }

    #[test]
    fn capture_jpeg_reads_reject_a_huge_file() {
        let src = include_str!("desktop.rs");
        let cap = src
            .split("pub fn capture_jpeg(")
            .nth(1)
            .and_then(|s| s.split("\npub fn capture_data_url(").next())
            .expect("capture_jpeg");
        let jpg_read = cap.find("std::fs::read(&written)").expect("jpg read");
        assert!(
            cap.contains("IMAGE_FILE_CAP") && cap.find("IMAGE_FILE_CAP").expect("cap") < jpg_read,
            "grim/ffmpeg JPEG must not slurp a huge file: {cap}"
        );
        let cam = src
            .split("pub fn capture_webcam(")
            .nth(1)
            .and_then(|s| s.split("\npub fn save_file_dialog(").next())
            .expect("capture_webcam");
        let cam_read = cam.find("std::fs::read(&path)").expect("cam read");
        assert!(
            cam.contains("IMAGE_FILE_CAP")
                && cam.find("IMAGE_FILE_CAP").expect("cam cap") < cam_read,
            "webcam JPEG must not slurp a huge file: {cam}"
        );
    }

    #[test]
    fn tab_list_without_cdp_says_down() {
        let out = run_computer_op_cancel(
            &ComputerOp::Tab {
                action: TabAction::List,
                query: String::new(),
            },
            None,
        );
        assert!(
            out.contains("browser hand down")
                || out.contains("exit 0")
                || out.contains("ctrl+t")
                || out.contains("page tab")
                || out.contains("no page tabs")
                || out.contains("act "),
            "tab list must run or fall back, not stall: {out}"
        );
        let cdp = include_str!("desktop.rs")
            .split("fn cdp_http(")
            .nth(1)
            .and_then(|s| s.split("fn probe_cdp(").next())
            .expect("cdp_http");
        assert!(
            cdp.contains(".take(") && !cdp.contains("into_string()"),
            "CDP list on the windshield must not slurp a huge /json/list: {cdp}"
        );
        assert!(
            cdp.contains("TEXT_FILE_CAP") || cdp.contains("IMAGE_FILE_CAP"),
            "CDP HTTP must stop at a cabin cap: {cdp}"
        );
    }

    #[test]
    fn url_safe_to_open_rejects_schemes_and_shell_meta() {
        for bad in [
            "",
            "javascript:alert(1)",
            "file:///C:/Windows/System32/calc.exe",
            "ftp://x.com",
            "data:text/html,x",
            "https://",
            "https:///x",
            "https://a.com/ & calc",
            "https://a.com/x&calc ",
            "https://a.com/|calc",
            "https://a.com/^&calc",
            "https://a.com/\"&calc",
            "https://a.com/`calc`",
            "https://a.com/\ncalc",
            "http://a.com\\..\\x",
            "https://a.com/{x}",
            "https://a.com/'x",
        ] {
            assert!(!url_safe_to_open(bad), "should reject {bad:?}");
        }
        for good in [
            "https://a.com/%0a",
            "https://example.com",
            "http://example.com/a?b=1&c=2#frag",
            "HTTPS://Example.com/path",
        ] {
            assert!(url_safe_to_open(good), "should accept {good:?}");
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn spawn_reaped_leaves_no_zombie() {
        let pid = spawn_reaped(&mut Command::new("true")).expect("spawn true");
        let proc_dir = std::path::PathBuf::from(format!("/proc/{pid}"));
        let start = Instant::now();
        // A zombie keeps its /proc entry until someone waits on it.
        while proc_dir.exists() {
            assert!(
                start.elapsed() < Duration::from_secs(5),
                "{pid} was never reaped: {:?}",
                std::fs::read_to_string(proc_dir.join("stat")).ok()
            );
            std::thread::sleep(Duration::from_millis(20));
        }
        let desktop = include_str!("desktop.rs");
        for name in [
            "pub fn open_path(",
            "pub fn open_url(",
            "pub fn play_media(",
        ] {
            let body = desktop
                .split(name)
                .nth(1)
                .and_then(|s| s.split("\npub ").next())
                .expect(name);
            assert!(
                body.contains("spawn_reaped(") && !body.contains(".spawn()"),
                "{name} must reap its helper: {body}"
            );
        }
        assert!(include_str!("notify.rs").contains("spawn_reaped("));
    }

    #[test]
    fn open_url_never_shells_out_through_cmd() {
        let desktop = include_str!("desktop.rs");
        let chat = include_str!("app/chat_ui.rs");
        let oauth = include_str!("oauth.rs");
        let markdown = include_str!("markdown.rs");
        let feed = include_str!("app/feed_ui.rs");
        for (name, src) in [
            ("desktop", desktop),
            ("chat", chat),
            ("oauth", oauth),
            ("markdown", markdown),
            ("feed", feed),
        ] {
            assert!(
                !src.contains("\"/C\", \"start\""),
                "{name} must not hand a URL to cmd /C start"
            );
        }
        let opener = desktop
            .split("pub fn open_url(")
            .nth(1)
            .and_then(|s| s.split("pub const VIDEO_PLAYERS").next())
            .expect("open_url");
        assert!(
            opener.contains("url_safe_to_open")
                && opener.contains("win_native::open_path")
                && opener.contains("xdg-open")
                && !opener.contains("Command::new(\"cmd\"")
                && !opener.contains("sh -c"),
            "{opener}"
        );
        assert!(
            markdown.contains(".link(RichText::new(text)")
                && markdown.contains("desktop::open_url"),
            "chat markdown links open through desktop::open_url"
        );
        assert!(
            chat.contains("desktop::open_url") && oauth.contains("desktop::open_url"),
            "elicit Open and OAuth share desktop::open_url"
        );
        assert!(
            oauth.contains("trusted_xai_url(url)?")
                && oauth
                    .split("pub fn open_browser(")
                    .nth(1)
                    .and_then(|s| s.split("pub fn poll_until_ready(").next())
                    .is_some_and(|s| {
                        s.find("trusted_xai_url").unwrap_or(usize::MAX)
                            < s.find("open_url").unwrap_or(0)
                    }),
            "OAuth still checks the xAI host before opening"
        );
    }

    /// A CF_DIB as Windows puts it on the clipboard: a 40-byte header, then
    /// bottom-up rows padded to four bytes.
    fn dib(bits: u16, compression: u32, extra: &[u8], rows: &[&[u8]]) -> Vec<u8> {
        let mut d = Vec::new();
        d.extend_from_slice(&40u32.to_le_bytes());
        d.extend_from_slice(&2i32.to_le_bytes());
        d.extend_from_slice(&(rows.len() as i32).to_le_bytes());
        d.extend_from_slice(&1u16.to_le_bytes());
        d.extend_from_slice(&bits.to_le_bytes());
        d.extend_from_slice(&compression.to_le_bytes());
        d.extend_from_slice(&[0; 20]);
        d.extend_from_slice(extra);
        for r in rows.iter().rev() {
            d.extend_from_slice(r);
            d.resize(d.len().next_multiple_of(4), 0);
        }
        d
    }

    #[test]
    fn clipboard_dib_becomes_a_png_with_the_same_pixels() {
        // 24-bit BGR, top row red then green, bottom row blue then white.
        let d = dib(
            24,
            0,
            &[],
            &[&[0, 0, 255, 0, 255, 0], &[255, 0, 0, 255, 255, 255]],
        );
        let png = dib_to_png(&d).expect("24-bit DIB");
        assert_eq!(&png[..4], b"\x89PNG");
        let img = image::load_from_memory(&png).unwrap().to_rgb8();
        assert_eq!((img.width(), img.height()), (2, 2));
        assert_eq!(img.get_pixel(0, 0).0, [255, 0, 0]);
        assert_eq!(img.get_pixel(1, 0).0, [0, 255, 0]);
        assert_eq!(img.get_pixel(0, 1).0, [0, 0, 255]);
        assert_eq!(img.get_pixel(1, 1).0, [255, 255, 255]);

        // 32-bit BI_BITFIELDS: three masks sit between the header and the pixels.
        let masks: Vec<u8> = [0x00ff_0000u32, 0x0000_ff00, 0x0000_00ff]
            .iter()
            .flat_map(|m| m.to_le_bytes())
            .collect();
        let d = dib(32, 3, &masks, &[&[10, 20, 30, 0, 40, 50, 60, 0]]);
        let img = image::load_from_memory(&dib_to_png(&d).expect("bitfields DIB"))
            .unwrap()
            .to_rgb8();
        assert_eq!(img.get_pixel(0, 0).0, [30, 20, 10]);
        assert_eq!(img.get_pixel(1, 0).0, [60, 50, 40]);

        assert_eq!(dib_to_png(&[1, 2, 3]), None, "too short for a header");
        assert_eq!(dib_to_png(&d[..44]), None, "header with no pixels");
    }

    #[test]
    fn a_captured_screen_becomes_a_jpeg_and_a_black_one_is_refused() {
        let lit = image::RgbaImage::from_fn(64, 48, |x, y| {
            image::Rgba([(x * 4) as u8, (y * 5) as u8, 128, 0])
        });
        let jpeg = rgba_frame_to_jpeg(lit).expect("a lit screen");
        assert_eq!(&jpeg[..3], &[0xFF, 0xD8, 0xFF]);
        let back = image::load_from_memory(&jpeg).unwrap();
        assert_eq!((back.width(), back.height()), (64, 48));

        let black = image::RgbaImage::from_pixel(64, 48, image::Rgba([0, 0, 0, 255]));
        assert_eq!(
            rgba_frame_to_jpeg(black),
            Err("the screen was a black frame".to_string())
        );
    }

    /// Smoke the image codecs we enable in Cargo.toml (png/jpeg/webp/ico/bmp).
    #[test]
    fn cabin_image_codecs_round_trip() {
        let rgba = image::RgbaImage::from_pixel(4, 4, image::Rgba([10, 20, 30, 255]));
        let rgb = image::RgbImage::from_pixel(4, 4, image::Rgb([10, 20, 30]));
        for (format, dyn_img) in [
            (
                image::ImageFormat::Png,
                image::DynamicImage::ImageRgba8(rgba.clone()),
            ),
            (
                image::ImageFormat::Jpeg,
                image::DynamicImage::ImageRgb8(rgb.clone()),
            ),
            (
                image::ImageFormat::WebP,
                image::DynamicImage::ImageRgba8(rgba.clone()),
            ),
            (
                image::ImageFormat::Ico,
                image::DynamicImage::ImageRgba8(rgba.clone()),
            ),
            (image::ImageFormat::Bmp, image::DynamicImage::ImageRgb8(rgb)),
        ] {
            let mut buf = Vec::new();
            dyn_img
                .write_to(&mut std::io::Cursor::new(&mut buf), format)
                .unwrap_or_else(|e| panic!("{format:?} encode: {e}"));
            let back = image::load_from_memory_with_format(&buf, format)
                .unwrap_or_else(|e| panic!("{format:?} decode: {e}"));
            assert_eq!((back.width(), back.height()), (4, 4), "{format:?}");
        }
    }

    #[test]
    fn wl_paste_gets_the_wayland_socket_startup_dropped() {
        let src = include_str!("desktop.rs").replace("\r\n", "\n");
        let clip = src
            .split("pub fn clipboard_image()")
            .nth(1)
            .and_then(|s| s.split("\npub fn ").next())
            .expect("clipboard_image");
        let env = clip
            .find("cmd.env(\"WAYLAND_DISPLAY\", name)")
            .expect("wl-paste gets WAYLAND_DISPLAY");
        assert!(
            clip[..env].contains("if bin == \"wl-paste\""),
            "only wl-paste gets it"
        );
        assert!(clip[..env].contains("wayland_display_for_tools()"));
    }
}
