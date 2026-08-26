use grokhub_core::{
    act_window_search_bin, browser_windshield_line, capture_kinds, cdp_new_tab_path,
    cdp_page_close_payload, cdp_page_focus_payload, clamp_to_desktop, clip_image_args,
    computer_cmd_line, computer_drive_for, cursor_on_output, diagnose_hands, empty_hands_steps_error,
    ffmpeg_webcam_args, ffmpeg_x11_args, filter_atspi_rows, format_cursor_line_miss,
    format_tab_list, frame_is_blank, frame_origin_for, gnome_shell_screenshot_args, grim_capture_args,
    hands_backend_name, hands_blocked_by_lock, hands_chip_label, hands_chip_live, hands_down_receipt,
    hands_windshield_line, image_pixels_ok, image_to_global, infer_wayland_display, jpeg_data_url,
    layout_prompt, live_pcm_argv, live_pcm_frame_bytes, luma_mean_var, monitor_local_to_global,
    parse_atspi_line, parse_cdp_targets, parse_picker_stdout, parse_wmctrl_line, parse_xdotool_mouse,
    parse_xrandr_outputs, pcm_from_capture, pick_browser_tab, pick_capture_output, pick_hands_backend,
    pick_named_row, picker_args, png_ihdr_size, pointer_slop_miss, rank_atspi_rows, relative_move_steps,
    resolve_bin_in, session_is_wayland, tab_list_from_rows, take_text_body, IMAGE_FILE_CAP,
    TEXT_FILE_CAP, virtual_desktop_size, windshield_frame_geom, x11_grab_size, ydotool_socket_path,
    AtspiRow, BrowserTab, CaptureKind, ComputerDrive, ComputerOp, DisplayOutput, HandsBackend,
    HandsDown, TabAction, CDP_DOWN, CDP_PORTS, RECORDERS, TRANSCRIBERS, PYATSPI_MISSING,
};
use image::GenericImageView;
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const ATSPI_PY: &str = r#"
import sys
try:
    import pyatspi
except Exception:
    sys.exit(2)
def walk(acc, n=0):
    role = "object"
    try:
        name = (acc.name or "").replace(" ", "_")
        role = (acc.getRoleName() or "object").replace(" ", "-")
        ext = acc.queryComponent().getExtents(0)
        w, h = int(ext.width), int(ext.height)
        keep = n <= 8 or "tab" in role or "button" in role
        if w > 0 and h > 0 and keep:
            print(f"role={role} name={name} x={int(ext.x)} y={int(ext.y)} w={w} h={h}")
    except Exception:
        pass
    if n > 12:
        return
    if n > 8 and "tab" not in role and "frame" not in role and "panel" not in role:
        return
    try:
        for i in range(acc.childCount):
            walk(acc.getChildAtIndex(i), n + 1)
    except Exception:
        pass
walk(pyatspi.Registry.getDesktop(0))
"#;

struct LastDeskFrame {
    jpeg_w: u32,
    jpeg_h: u32,
    origin_x: i32,
    origin_y: i32,
    outputs: Vec<DisplayOutput>,
}

static LAST_DESK_FRAME: Mutex<Option<LastDeskFrame>> = Mutex::new(None);

#[derive(Clone)]
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
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());
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

fn last_desk_frame_geom() -> (u32, u32, i32, i32) {
    LAST_DESK_FRAME
        .lock()
        .ok()
        .and_then(|g| {
            g.as_ref()
                .map(|f| (f.jpeg_w, f.jpeg_h, f.origin_x, f.origin_y))
        })
        .unwrap_or((0, 0, 0, 0))
}

pub fn read_display_outputs() -> Vec<DisplayOutput> {
    let mut cmd = Command::new("xrandr");
    cmd.arg("-q");
    run_limited(cmd, DESK_LIST_TIMEOUT)
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|t| parse_xrandr_outputs(&t))
        .unwrap_or_default()
}

pub fn prepare_windshield(
    rows: &[AtspiRow],
    ask: Option<&str>,
    captured_this_turn: bool,
) -> (Vec<AtspiRow>, String) {
    let outputs = read_display_outputs();
    let (dw, dh) = virtual_desktop_size(&outputs)
        .map(|(w, h)| (w as i32, h as i32))
        .unwrap_or((0, 0));
    let kept = filter_atspi_rows(rows, dw, dh);
    let ranked = rank_atspi_rows(&kept, ask, 40);
    let (fw, fh, ox, oy) = windshield_frame_geom(captured_this_turn, last_desk_frame_geom());
    let mut header = layout_prompt(&outputs, fw, fh, ox, oy, read_cursor_xy());
    let (up, n) = cached_cdp_status();
    header.push_str(&browser_windshield_line(up, n));
    header.push_str(&hands_windshield_line(hands_peek(), hands_driver_name()));
    (ranked, header)
}

struct CdpCache {
    at: Instant,
    up: bool,
    n: usize,
    inflight: bool,
}

static CDP_CACHE: Mutex<Option<CdpCache>> = Mutex::new(None);

fn cached_cdp_status() -> (bool, usize) {
    if let Ok(g) = CDP_CACHE.lock() {
        if let Some(c) = g.as_ref() {
            let hit = (c.up, c.n);
            let fresh = c.at.elapsed() < Duration::from_secs(2);
            let busy = c.inflight;
            drop(g);
            if !fresh && !busy {
                kick_cdp_status();
            }
            return hit;
        }
    }
    cdp_status_now()
}

fn cdp_status_now() -> (bool, usize) {
    let hit = probe_cdp();
    let (up, n) = match &hit {
        Some((_, tabs)) => (true, tabs.len()),
        None => (false, 0),
    };
    if let Ok(mut g) = CDP_CACHE.lock() {
        *g = Some(CdpCache {
            at: Instant::now(),
            up,
            n,
            inflight: false,
        });
    }
    (up, n)
}

fn kick_cdp_status() {
    if let Ok(mut g) = CDP_CACHE.lock() {
        if let Some(c) = g.as_mut() {
            if c.inflight {
                return;
            }
            c.inflight = true;
        }
    }
    std::thread::spawn(|| {
        let _ = cdp_status_now();
    });
}

fn invalidate_cdp_cache() {
    if let Ok(mut g) = CDP_CACHE.lock() {
        *g = None;
    }
}

pub fn probe_hub_health_body(port: u16) -> Option<String> {
    let url = format!("http://127.0.0.1:{port}/v1/health");
    ureq::get(&url)
        .timeout(Duration::from_millis(400))
        .call()
        .ok()
        .and_then(|r| r.into_string().ok())
}

fn cdp_http(port: u16, path: &str) -> Result<String, String> {
    let url = format!("http://127.0.0.1:{port}{path}");
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

fn run_tab_op(action: TabAction, query: &str, cancel: Option<&AtomicBool>) -> Result<String, String> {
    if cancelled(cancel) {
        return Err("halted".into());
    }
    if let Some((port, tabs)) = probe_cdp() {
        invalidate_cdp_cache();
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
    let (host, _) = rest
        .split_once('/')
        .ok_or_else(|| "cdp websocket path missing".to_string())?;
    let addr = host
        .to_socket_addrs()
        .map_err(|e| e.to_string())?
        .next()
        .ok_or_else(|| "cdp websocket host".to_string())?;
    let stream = TcpStream::connect_timeout(&addr, Duration::from_millis(800))
        .map_err(|e| e.to_string())?;
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

pub fn resolve_bin(name: &str) -> Option<PathBuf> {
    resolve_bin_in(
        name,
        std::env::var("PATH").ok().as_deref(),
        std::env::var("HOME").ok().as_deref(),
    )
}

pub fn which(name: &str) -> bool {
    resolve_bin(name).is_some()
}

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
    sock.exists()
        || Path::new("/tmp/.ydotool_socket").exists()
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

pub fn hands_chip_text() -> String {
    hands_chip_label(hands_peek(), hands_driver_name())
}

pub fn hands_ready() -> bool {
    hands_chip_live(hands_peek())
}

fn pyatspi_import_ok() -> bool {
    let mut cmd = Command::new("python3");
    cmd.args(["-c", "import pyatspi"]);
    run_limited(cmd, DESK_LIST_TIMEOUT).is_some_and(|o| o.status.success())
}

pub fn install_hands_status() -> String {
    let reason = ensure_hands();
    let mut out = hands_down_receipt(reason).to_string();
    if !pyatspi_import_ok() {
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(PYATSPI_MISSING);
    }
    out
}

fn desk_scan() -> DeskScan {
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

pub fn hands_driver_name() -> &'static str {
    hands_backend_name(live_hands_backend())
}

fn run_bin_steps(bin: &str, steps: &[Vec<String>], cancel: Option<&AtomicBool>) -> Result<(), String> {
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
    computer_drive_for(
        live_hands_backend().unwrap_or(HandsBackend::Xdotool),
        op,
    )
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
    let out = run_limited(search, DESK_LIST_TIMEOUT)
        .ok_or_else(|| format!("act {name}: not found"))?;
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
    let (x, y, w, h) = parse_getwindowgeometry(&text).ok_or_else(|| {
        format!("act {name}: no geometry")
    })?;
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

pub fn run_computer_op(op: &ComputerOp) -> String {
    run_computer_op_cancel(op, None)
}

pub fn run_computer_op_cancel(op: &ComputerOp, cancel: Option<&AtomicBool>) -> String {
    let started = Instant::now();
    let line = computer_cmd_line(op);
    if cancelled(cancel) {
        return hands_receipt(&line, started, false, "halted");
    }
    let titles = lock_titles();
    let title_refs: Vec<&str> = titles.iter().map(|s| s.as_str()).collect();
    if hands_blocked_by_lock(op, &title_refs) {
        return hands_receipt(&line, started, false, "blocked: lock screen");
    }
    let op = map_pointer_op(op);
    match pointer_drive(&op) {
        ComputerDrive::Xdotool(steps) | ComputerDrive::Ydotool(steps) => {
            if let Some(detail) = empty_hands_steps_error(&op, &steps) {
                return hands_receipt(&line, started, false, &detail);
            }
            match &op {
                ComputerOp::Click { x, y }
                | ComputerOp::DoubleClick { x, y }
                | ComputerOp::Move { x, y } => {
                    match drive_pointer_to(*x, *y, cancel) {
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
                    }
                }
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
                        &append_cursor_detail(&format!("moved {gx},{gy}"), (gx, gy), read_cursor_xy()),
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
    let xrandr = run_limited(xrandr_cmd, DESK_LIST_TIMEOUT)
        .and_then(|o| String::from_utf8(o.stdout).ok());
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

pub fn capture_jpeg(path: &Path) -> Result<Vec<u8>, String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    pin_wayland_for_capture();
    let bins: Vec<&str> = CAPTURE_BINS.iter().copied().filter(|n| which(n)).collect();
    let wayland = session_is_wayland(
        std::env::var("WAYLAND_DISPLAY").ok().as_deref(),
        std::env::var("XDG_SESSION_TYPE").ok().as_deref(),
    );
    let x11 = std::env::var_os("DISPLAY").is_some();
    let plan = capture_kinds(&bins, wayland, x11);
    if plan.is_empty() {
        return Err("no grim/gnome-screenshot/ffmpeg/scrot for a desktop frame".into());
    }
    let outputs = read_display_outputs();
    let points: Vec<(i32, i32)> = collect_rows()
        .iter()
        .filter(|r| r.role != "cursor")
        .map(|r| (r.x + r.w / 2, r.y + r.h / 2))
        .collect();
    let grim_out = pick_capture_output(&outputs, &points)
        .map(|o| o.name.clone());
    let mut last = "no desktop frame".to_string();
    for kind in plan {
        match run_capture_kind(kind, path, grim_out.as_deref()) {
            Ok((written, captured_output)) => {
                let jpeg = if written
                    .extension()
                    .and_then(|s| s.to_str())
                    .is_some_and(|e| e == "jpg" || e == "jpeg")
                {
                    let len = std::fs::metadata(&written).map(|m| m.len()).unwrap_or(u64::MAX);
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

pub const PLAYERS: &[&str] = &["ffplay", "mpv", "paplay"];

pub fn record_once() -> Result<PathBuf, String> {
    let wav = std::env::temp_dir().join("grokhub-voice.wav");
    record_wav(&wav)?;
    Ok(wav)
}

pub fn transcribe_local(wav: &Path) -> Result<String, String> {
    transcribe(wav)
}

pub fn play_audio(path: &Path) -> Result<(), String> {
    let dest = path.to_str().ok_or("audio path")?;
    match first_bin(PLAYERS).as_deref() {
        Some("ffplay") => run_ok("ffplay", &["-nodisp", "-autoexit", "-loglevel", "error", dest]),
        Some("mpv") => run_ok("mpv", &["--no-video", "--really-quiet", dest]),
        Some("paplay") => run_ok("paplay", &[dest]),
        _ => Err("no ffplay/mpv/paplay to speak".into()),
    }
}

/// Stream 24 kHz s16le mono PCM to the speakers (realtime Voice output).
pub struct PcmSink {
    child: Option<Child>,
}

impl Default for PcmSink {
    fn default() -> Self {
        Self { child: None }
    }
}

impl PcmSink {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, pcm: &[u8]) {
        if pcm.is_empty() {
            return;
        }
        if self.ensure().is_err() {
            return;
        }
        if let Some(child) = self.child.as_mut() {
            if let Some(stdin) = child.stdin.as_mut() {
                let _ = stdin.write_all(pcm);
            }
        }
    }

    fn ensure(&mut self) -> Result<(), String> {
        if let Some(c) = self.child.as_mut() {
            match c.try_wait() {
                Ok(None) => return Ok(()),
                _ => {
                    self.child = None;
                }
            }
        }
        let child = if which("paplay") {
            Command::new("paplay")
                .args(["--raw", "--rate=24000", "--channels=1", "--format=s16le"])
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .map_err(|e| e.to_string())?
        } else if which("ffplay") {
            Command::new("ffplay")
                .args([
                    "-nodisp",
                    "-loglevel",
                    "error",
                    "-f",
                    "s16le",
                    "-ar",
                    "24000",
                    "-ac",
                    "1",
                    "-i",
                    "pipe:0",
                ])
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .map_err(|e| e.to_string())?
        } else {
            return Err("no paplay/ffplay for pcm".into());
        };
        self.child = Some(child);
        Ok(())
    }
}

impl Drop for PcmSink {
    fn drop(&mut self) {
        if let Some(mut c) = self.child.take() {
            let _ = c.stdin.take();
            let _ = c.kill();
        }
    }
}

/// Long-running raw PCM capture for duplex Voice. Kill on drop.
pub struct LivePcm {
    child: Option<Child>,
}

impl LivePcm {
    pub fn start() -> Option<Self> {
        let bin = first_bin(RECORDERS)?;
        let args = live_pcm_argv(bin.as_str())?;
        let child = Command::new(&bin)
            .args(args)
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;
        Some(Self { child: Some(child) })
    }

    pub fn read_frame(&mut self) -> Option<Vec<u8>> {
        let stdout = self.child.as_mut()?.stdout.as_mut()?;
        let mut buf = vec![0u8; live_pcm_frame_bytes()];
        stdout.read_exact(&mut buf).ok()?;
        Some(buf)
    }
}

impl Drop for LivePcm {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            std::thread::spawn(move || {
                let _ = child.wait();
            });
        }
    }
}

fn record_wav(path: &Path) -> Result<(), String> {
    let dest = path.to_str().ok_or("wav path")?;
    match first_bin(RECORDERS).as_deref() {
        Some("arecord") => run_ok(
            "arecord",
            &["-q", "-d", "4", "-f", "cd", "-t", "wav", dest],
        ),
        Some("ffmpeg") => run_ok(
            "ffmpeg",
            &[
                "-y", "-hide_banner", "-loglevel", "error",
                "-f", "pulse", "-i", "default", "-t", "4", "-ac", "1", "-ar", "16000", dest,
            ],
        ),
        Some("sox") | Some("rec") => run_ok("rec", &[dest, "trim", "0", "4"]),
        _ => Err("no arecord/ffmpeg/sox — install alsa-utils or ffmpeg".into()),
    }
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

pub fn imagine_save_path(slug: &str) -> PathBuf {
    imagine_save_path_ext(slug, "png")
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
    let len = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(u64::MAX);
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

/// Short PCM chunks for the realtime socket. Empty iterator if no recorder.
pub fn record_pcm_chunks() -> Vec<Vec<u8>> {
    let dest = std::env::temp_dir().join("grokhub-voice-live.wav");
    let path = dest.to_string_lossy().to_string();
    let ok = match first_bin(RECORDERS).as_deref() {
        Some("arecord") => {
            let mut cmd = Command::new("arecord");
            cmd.args([
                "-q", "-d", "1", "-f", "S16_LE", "-r", "24000", "-c", "1", "-t", "wav", &path,
            ]);
            run_limited(cmd, DESK_CAPTURE_TIMEOUT).is_some_and(|o| o.status.success())
        }
        Some("ffmpeg") => {
            let mut cmd = Command::new("ffmpeg");
            cmd.args([
                "-y", "-hide_banner", "-loglevel", "error",
                "-f", "pulse", "-i", "default", "-t", "1", "-ac", "1", "-ar", "24000",
                "-f", "s16le", &path,
            ]);
            run_limited(cmd, DESK_CAPTURE_TIMEOUT).is_some_and(|o| o.status.success())
        }
        _ => false,
    };
    if !ok {
        return vec![];
    }
    let len = std::fs::metadata(&dest).map(|m| m.len()).unwrap_or(u64::MAX);
    if len > IMAGE_FILE_CAP {
        let _ = std::fs::remove_file(&dest);
        return vec![];
    }
    let bytes = std::fs::read(&dest).unwrap_or_default();
    let _ = std::fs::remove_file(&dest);
    let pcm = pcm_from_capture(&bytes);
    if pcm.len() < 64 {
        vec![]
    } else {
        vec![pcm.to_vec()]
    }
}

pub fn save_file_dialog(suggested: &str) -> Option<PathBuf> {
    let name = std::path::Path::new(suggested)
        .file_name()
        .and_then(|s| s.to_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("imagine.png");
    for bin in ["zenity", "kdialog", "yad", "qarma"] {
        let Some(args) = grokhub_core::picker_save_args(bin, name) else {
            continue;
        };
        if !which(bin) {
            continue;
        }
        if let Ok(o) = Command::new(bin).args(args).output() {
            if o.status.success() {
                if let Some(p) = grokhub_core::parse_picker_stdout(&String::from_utf8_lossy(&o.stdout)) {
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

pub fn clipboard_image() -> Option<PathBuf> {
    let dest = std::env::temp_dir().join("grokhub-clip.png");
    for bin in ["xclip", "wl-paste"] {
        let Some(args) = clip_image_args(bin) else {
            continue;
        };
        if !which(bin) {
            continue;
        }
        let mut cmd = Command::new(bin);
        cmd.args(args);
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
        assert!(first_bin(&["definitely-not-a-bin-grokhub"]).is_none());
        assert!(hands_down_receipt(HandsDown::Missing).contains("lib/grokhub/bin"));
        assert!(hands_down_receipt(HandsDown::Uinput).contains("uinput"));
        assert!(hands_down_receipt(HandsDown::Daemon).contains("ydotoold"));
        assert_ne!(
            hands_down_receipt(HandsDown::Missing),
            hands_down_receipt(HandsDown::Daemon)
        );
        let a = grokhub_core::live_pcm_argv("arecord").unwrap();
        assert!(a.iter().any(|x| *x == "raw"));
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
            .write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Jpeg)
            .unwrap();
        assert!(frame_bytes_are_blank(&buf));
        let color = image::RgbImage::from_pixel(24, 24, image::Rgb([80, 140, 200]));
        buf.clear();
        image::DynamicImage::ImageRgb8(color)
            .write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Jpeg)
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
        assert_eq!(
            load_image_data_url(&path).unwrap_err(),
            "image too large"
        );
        assert!(
            load.contains("IMAGE_PIXEL_CAP") || load.contains("image_pixels_ok"),
            "a tiny PNG with huge pixels must not decode on the UI thread: {load}"
        );
        let bomb = dir.join("bomb.png");
        std::fs::write(&bomb, png_ihdr_only(50_000, 50_000)).unwrap();
        assert_eq!(
            load_image_data_url(&bomb).unwrap_err(),
            "image too large"
        );
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
        assert!(
            grokhub_core::hands_blocked_by_lock(
                &ComputerOp::Click { x: 10, y: 20 },
                &titles.iter().map(|s| s.as_str()).collect::<Vec<_>>(),
            )
        );
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
        assert!(started.elapsed() < Duration::from_secs(1), "{:?}", started.elapsed());
        assert!(out.contains("halted"), "{out}");
        assert!(out.contains("exit 1"), "{out}");
    }

    #[test]
    fn hands_move_pointer_when_display() {
        if std::env::var("DISPLAY").is_err() || !which("xdotool") {
            return;
        }
        let dest_x = 1500;
        let dest_y = 400;
        let out = run_computer_op(&ComputerOp::Move {
            x: dest_x,
            y: dest_y,
        });
        assert!(out.contains("exit 0"), "{out}");
        assert!(out.contains("moved 1500,400"), "{out}");
        assert!(out.contains("cursor"), "{out}");
        let loc = Command::new("xdotool")
            .args(["getmouselocation"])
            .output()
            .unwrap();
        let row = parse_xdotool_mouse(&String::from_utf8_lossy(&loc.stdout)).unwrap();
        assert_eq!((row.x, row.y), (dest_x, dest_y), "{out} {} {}", row.x, row.y);
        let cursor = run_computer_op(&ComputerOp::Cursor);
        assert!(cursor.contains("exit 0"), "{cursor}");
        assert!(cursor.contains("cursor"), "{cursor}");
        match grokhub_core::computer_drive(&ComputerOp::Click { x: 1, y: 2 }) {
            ComputerDrive::Xdotool(steps) => {
                assert!(!steps.iter().any(|s| s.iter().any(|a| a == "--sync")));
            }
            other => panic!("{other:?}"),
        }
        assert!(["ydotool", "xdotool", "missing"].contains(&hands_driver_name()));
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
        assert!(
            started.elapsed() < Duration::from_secs(3),
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
        dump.args(["-c", &(IMAGE_FILE_CAP + 2 * 1024 * 1024).to_string(), "/dev/zero"]);
        let out = run_limited(dump, Duration::from_secs(3)).expect("head exited");
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
            .and_then(|s| s.split("\npub fn record_pcm_chunks(").next())
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
            .and_then(|s| s.split("\npub fn imagine_save_path(").next())
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
        let py = src
            .split("fn pyatspi_import_ok(")
            .nth(1)
            .and_then(|s| s.split("\npub fn install_hands_status").next())
            .expect("pyatspi_import_ok");
        assert!(
            py.contains("run_limited(") && !py.contains(".status()"),
            "pyatspi import must time out: {py}"
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
        let pcm = src
            .split("pub fn record_pcm_chunks(")
            .nth(1)
            .and_then(|s| s.split("\npub fn pick_file(").next())
            .expect("record_pcm_chunks");
        assert!(
            pcm.contains("run_limited(") && !pcm.contains(".status()"),
            "live mic arecord must time out so Voice halt can finish: {pcm}"
        );
        let pcm_read = pcm.find("std::fs::read(&dest)").expect("pcm read");
        assert!(
            pcm.contains("IMAGE_FILE_CAP") && pcm.find("IMAGE_FILE_CAP").expect("pcm cap") < pcm_read,
            "live mic must not slurp a huge wav: {pcm}"
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
            .and_then(|s| s.split("\npub fn record_pcm_chunks(").next())
            .expect("capture_webcam");
        let cam_read = cam.find("std::fs::read(&path)").expect("cam read");
        assert!(
            cam.contains("IMAGE_FILE_CAP") && cam.find("IMAGE_FILE_CAP").expect("cam cap") < cam_read,
            "webcam JPEG must not slurp a huge file: {cam}"
        );
    }

    #[test]
    fn tab_list_without_cdp_says_down() {
        let out = run_computer_op(&ComputerOp::Tab {
            action: TabAction::List,
            query: String::new(),
        });
        assert!(
            out.contains("browser hand down")
                || out.contains("exit 0")
                || out.contains("ctrl+t")
                || out.contains("page tab")
                || out.contains("no page tabs")
                || out.contains("act "),
            "tab list must run or fall back, not stall: {out}"
        );
        let (_, header) = prepare_windshield(&[], None, false);
        assert!(
            header.contains("browser: cdp"),
            "windshield must report cdp up or down: {header}"
        );
        assert!(
            header.contains("hands:"),
            "windshield must report hands health: {header}"
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
        let status = include_str!("desktop.rs")
            .split("fn cached_cdp_status()")
            .nth(1)
            .and_then(|s| s.split("fn probe_cdp(").next())
            .expect("cached_cdp_status");
        assert!(
            status.contains("thread::spawn") && status.contains("inflight"),
            "stale CDP windshield probe must refresh off the UI thread: {status}"
        );
    }
}
