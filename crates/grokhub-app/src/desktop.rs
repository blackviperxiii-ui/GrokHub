use grokhub_core::{
    act_window_search_bin, capture_kinds, clip_image_args, computer_cmd_line, computer_drive_for, diagnose_hands,
    empty_hands_steps_error, ffmpeg_webcam_args, ffmpeg_x11_args, filter_atspi_rows, frame_is_blank,
    frame_origin_for, gnome_shell_screenshot_args, grim_capture_args, hands_backend_name, hands_blocked_by_lock,
    hands_chip_label, hands_chip_live, hands_down_receipt, image_to_global, infer_wayland_display,
    jpeg_data_url, layout_prompt, live_pcm_argv, live_pcm_frame_bytes, luma_mean_var,
    parse_atspi_line, parse_picker_stdout, parse_wmctrl_line, parse_xdotool_mouse,
    parse_xrandr_outputs, pcm_from_capture, pick_capture_output, pick_hands_backend, pick_named_row,
    picker_args, rank_atspi_rows, resolve_bin_in, session_is_wayland, take_text_body, IMAGE_FILE_CAP,
    TEXT_FILE_CAP,
    virtual_desktop_size, windshield_frame_geom, x11_grab_size, ydotool_socket_path, AtspiRow, CaptureKind, ComputerDrive,
    ComputerOp, DisplayOutput, HandsBackend, HandsDown, RECORDERS, TRANSCRIBERS, PYATSPI_MISSING,
};
use image::GenericImageView;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

const ATSPI_PY: &str = r#"
import sys
try:
    import pyatspi
except Exception:
    sys.exit(2)
def walk(acc, n=0):
    if n > 8:
        return
    try:
        name = (acc.name or "").replace(" ", "_")
        role = (acc.getRoleName() or "object").replace(" ", "-")
        ext = acc.queryComponent().getExtents(0)
        w, h = int(ext.width), int(ext.height)
        if w > 0 and h > 0:
            print(f"role={role} name={name} x={int(ext.x)} y={int(ext.y)} w={w} h={h}")
    except Exception:
        pass
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
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let mut stdout = Vec::new();
                let mut stderr = Vec::new();
                if let Some(mut s) = child.stdout.take() {
                    let _ = s.read_to_end(&mut stdout);
                }
                if let Some(mut e) = child.stderr.take() {
                    let _ = e.read_to_end(&mut stderr);
                }
                return Some(Output {
                    status,
                    stdout,
                    stderr,
                });
            }
            Ok(None) if Instant::now() >= deadline => {
                kill_limited(&mut child);
                let _ = child.wait();
                return None;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(15)),
            Err(_) => {
                kill_limited(&mut child);
                let _ = child.wait();
                return None;
            }
        }
    }
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
    (ranked, layout_prompt(&outputs, fw, fh, ox, oy))
}

fn remember_from_jpeg(bytes: &[u8], outputs: &[DisplayOutput], grim_name: Option<&str>) {
    let Ok(img) = image::load_from_memory(bytes) else {
        return;
    };
    let (w, h) = img.dimensions();
    let origin = frame_origin_for(w, h, outputs, grim_name);
    remember_desk_frame(w, h, origin.0, origin.1, outputs.to_vec());
}

fn map_pointer_xy(x: i32, y: i32) -> (i32, i32) {
    let Ok(g) = LAST_DESK_FRAME.lock() else {
        return (x, y);
    };
    let Some(f) = g.as_ref() else {
        return (x, y);
    };
    image_to_global(
        x,
        y,
        f.jpeg_w,
        f.jpeg_h,
        &f.outputs,
        Some((f.origin_x, f.origin_y)),
    )
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
    let mut mouse = spawn_bin("xdotool");
    mouse.args(["getmouselocation"]);
    if let Some(out) = run_limited(mouse, DESK_LIST_TIMEOUT) {
        if let Some(r) = parse_xdotool_mouse(&String::from_utf8_lossy(&out.stdout)) {
            rows.push(r);
        }
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

pub fn lock_titles() -> Vec<String> {
    desk_scan().lock
}

pub fn lock_titles_from_stdout(atspi: &str, wmctrl: &str) -> Vec<String> {
    let lines: Vec<&str> = atspi.lines().chain(wmctrl.lines()).collect();
    grokhub_core::lock_check_titles(&lines)
}

fn act_click(name: &str, cancel: Option<&AtomicBool>) -> Result<(i32, i32), String> {
    if cancelled(cancel) {
        return Err("halted".into());
    }
    let rows = collect_rows();
    if let Some(r) = named_row(&rows, name) {
        let (x, y) = row_center(r);
        match pointer_drive(&ComputerOp::Click { x, y }) {
            ComputerDrive::Xdotool(steps) | ComputerDrive::Ydotool(steps) => {
                run_pointer_steps(&steps, cancel)?;
            }
            ComputerDrive::Act(_) | ComputerDrive::WaitFor(_) => {}
        }
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
    let cx = x + w / 2;
    let cy = y + h / 2;
    match pointer_drive(&ComputerOp::Click { x: cx, y: cy }) {
        ComputerDrive::Xdotool(steps) | ComputerDrive::Ydotool(steps) => {
            run_pointer_steps(&steps, cancel)?;
        }
        ComputerDrive::Act(_) | ComputerDrive::WaitFor(_) => {}
    }
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
            match run_pointer_steps(&steps, cancel) {
                Ok(()) => {
                    let detail = match &op {
                        ComputerOp::Click { x, y } => format!("clicked {x},{y}"),
                        ComputerOp::DoubleClick { x, y } => format!("double-clicked {x},{y}"),
                        ComputerOp::Move { x, y } => format!("moved {x},{y}"),
                        ComputerOp::Type { text } => format!("typed {} chars", text.chars().count()),
                        ComputerOp::Key { name } => format!("key {name}"),
                        ComputerOp::Scroll { dy } => format!("scrolled {dy}"),
                        ComputerOp::Act { .. } | ComputerOp::WaitFor { .. } => "ok".into(),
                    };
                    hands_receipt(&line, started, true, &detail)
                }
                Err(e) => hands_receipt(&line, started, false, &e),
            }
        }
        ComputerDrive::Act(name) => match act_click(name.as_str(), cancel) {
            Ok((x, y)) => hands_receipt(&line, started, true, &format!("act {name} @{x},{y}")),
            Err(e) => hands_receipt(&line, started, false, &e),
        },
        ComputerDrive::WaitFor(title) => match wait_for_title(title.as_deref(), cancel) {
            Ok(detail) => hands_receipt(&line, started, true, &detail),
            Err(e) => hands_receipt(&line, started, false, &e),
        },
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

fn image_file_to_jpeg(path: &Path) -> Result<Vec<u8>, String> {
    let len = std::fs::metadata(path).map(|m| m.len()).unwrap_or(u64::MAX);
    if len > IMAGE_FILE_CAP {
        return Err("image too large".into());
    }
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
    child: Child,
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
        Some(Self { child })
    }

    pub fn read_frame(&mut self) -> Option<Vec<u8>> {
        let stdout = self.child.stdout.as_mut()?;
        let mut buf = vec![0u8; live_pcm_frame_bytes()];
        stdout.read_exact(&mut buf).ok()?;
        Some(buf)
    }
}

impl Drop for LivePcm {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
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
    std::fs::read_to_string(&txt)
        .or_else(|_| std::fs::read_to_string(alt))
        .map_err(|e| e.to_string())
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
    let bytes = std::fs::read(&dest).unwrap_or_default();
    let _ = std::fs::remove_file(&dest);
    let pcm = pcm_from_capture(&bytes);
    if pcm.len() < 64 {
        vec![]
    } else {
        vec![pcm.to_vec()]
    }
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
        let loc = Command::new("xdotool")
            .args(["getmouselocation"])
            .output()
            .unwrap();
        let row = parse_xdotool_mouse(&String::from_utf8_lossy(&loc.stdout)).unwrap();
        assert_eq!((row.x, row.y), (dest_x, dest_y), "{out} {} {}", row.x, row.y);
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
        let pcm = src
            .split("pub fn record_pcm_chunks(")
            .nth(1)
            .and_then(|s| s.split("\npub fn pick_file(").next())
            .expect("record_pcm_chunks");
        assert!(
            pcm.contains("run_limited(") && !pcm.contains(".status()"),
            "live mic arecord must time out so Voice halt can finish: {pcm}"
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
}
