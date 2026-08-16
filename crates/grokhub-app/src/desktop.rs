use grokhub_core::{
    clip_image_args, computer_cmd_line, computer_drive, hands_blocked_by_lock, jpeg_data_url,
    parse_atspi_line, parse_picker_stdout, parse_wmctrl_line, parse_xdotool_mouse, pcm_from_capture,
    picker_args, take_text_body, AtspiRow, ComputerDrive, ComputerOp, RECORDERS, TRANSCRIBERS,
};
use image::GenericImageView;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const ATSPI_PY: &str = r#"
import sys
try:
    import pyatspi
except Exception:
    sys.exit(2)
def walk(acc, n=0):
    if n > 5:
        return
    try:
        name = (acc.name or "").replace(" ", "_")
        role = (acc.getRoleName() or "object").replace(" ", "-")
        ext = acc.queryComponent().getExtents(0)
        print(f"role={role} name={name} x={int(ext.x)} y={int(ext.y)} w={int(ext.width)} h={int(ext.height)}")
    except Exception:
        pass
    try:
        for i in range(acc.childCount):
            walk(acc.getChildAtIndex(i), n + 1)
    except Exception:
        pass
walk(pyatspi.Registry.getDesktop(0))
"#;

pub fn which(name: &str) -> bool {
    Command::new("which")
        .arg(name)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

pub fn first_bin(names: &[&str]) -> Option<String> {
    names.iter().find(|n| which(n)).map(|s| (*s).to_string())
}

pub fn collect_rows() -> Vec<AtspiRow> {
    let mut rows = Vec::new();
    if let Ok(out) = Command::new("python3").args(["-c", ATSPI_PY]).output() {
        if out.status.success() {
            for line in String::from_utf8_lossy(&out.stdout).lines() {
                if let Some(r) = parse_atspi_line(line) {
                    rows.push(r);
                }
            }
        }
    }
    if rows.is_empty() {
        if let Ok(out) = Command::new("wmctrl").args(["-lG"]).output() {
            for line in String::from_utf8_lossy(&out.stdout).lines() {
                if let Some(r) = parse_wmctrl_line(line) {
                    rows.push(r);
                }
            }
        }
    }
    if let Ok(out) = Command::new("xdotool").args(["getmouselocation"]).output() {
        if let Some(r) = parse_xdotool_mouse(&String::from_utf8_lossy(&out.stdout)) {
            rows.push(r);
        }
    }
    rows
}

pub fn named_row<'a>(rows: &'a [AtspiRow], name: &str) -> Option<&'a AtspiRow> {
    let q = name.to_ascii_lowercase();
    rows.iter().find(|r| {
        r.role != "cursor" && r.name.to_ascii_lowercase().contains(&q)
    })
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

fn run_xdotool_steps(steps: &[Vec<String>]) -> Result<(), String> {
    if !which("xdotool") {
        return Err("xdotool missing".into());
    }
    for (i, step) in steps.iter().enumerate() {
        if i > 0 {
            std::thread::sleep(Duration::from_millis(25));
        }
        let st = Command::new("xdotool")
            .args(step)
            .status()
            .map_err(|e| e.to_string())?;
        if !st.success() {
            return Err(format!("xdotool {} failed", step.join(" ")));
        }
    }
    Ok(())
}

fn lock_titles() -> Vec<String> {
    collect_rows().into_iter().map(|r| r.name).collect()
}

fn act_click(name: &str) -> Result<(i32, i32), String> {
    let rows = collect_rows();
    if let Some(r) = named_row(&rows, name) {
        let (x, y) = row_center(r);
        match computer_drive(&ComputerOp::Click { x, y }) {
            ComputerDrive::Xdotool(steps) => run_xdotool_steps(&steps)?,
            ComputerDrive::Act(_) | ComputerDrive::WaitFor(_) => {}
        }
        return Ok((x, y));
    }
    if !which("xdotool") {
        return Err(format!("act {name}: not found"));
    }
    let out = Command::new("xdotool")
        .args(["search", "--onlyvisible", "--name", name])
        .output()
        .map_err(|e| e.to_string())?;
    let id = String::from_utf8_lossy(&out.stdout)
        .lines()
        .next()
        .unwrap_or("")
        .trim()
        .to_string();
    if id.is_empty() {
        return Err(format!("act {name}: not found"));
    }
    let geo = Command::new("xdotool")
        .args(["getwindowgeometry", &id])
        .output()
        .map_err(|e| e.to_string())?;
    let text = String::from_utf8_lossy(&geo.stdout);
    let (x, y, w, h) = parse_getwindowgeometry(&text).ok_or_else(|| {
        format!("act {name}: no geometry")
    })?;
    let cx = x + w / 2;
    let cy = y + h / 2;
    match computer_drive(&ComputerOp::Click { x: cx, y: cy }) {
        ComputerDrive::Xdotool(steps) => run_xdotool_steps(&steps)?,
        ComputerDrive::Act(_) | ComputerDrive::WaitFor(_) => {}
    }
    Ok((cx, cy))
}

fn wait_for_title(title: Option<&str>) -> Result<String, String> {
    let Some(want) = title.filter(|s| !s.is_empty()) else {
        std::thread::sleep(Duration::from_millis(400));
        return Ok("waited".into());
    };
    let q = want.to_ascii_lowercase();
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
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
    let started = Instant::now();
    let line = computer_cmd_line(op);
    let titles = lock_titles();
    let title_refs: Vec<&str> = titles.iter().map(|s| s.as_str()).collect();
    if hands_blocked_by_lock(op, &title_refs) {
        return hands_receipt(&line, started, false, "blocked: lock screen");
    }
    match computer_drive(op) {
        ComputerDrive::Xdotool(steps) => match run_xdotool_steps(&steps) {
            Ok(()) => {
                let detail = match op {
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
        },
        ComputerDrive::Act(name) => match act_click(&name) {
            Ok((x, y)) => hands_receipt(&line, started, true, &format!("act {name} @{x},{y}")),
            Err(e) => hands_receipt(&line, started, false, &e),
        },
        ComputerDrive::WaitFor(title) => match wait_for_title(title.as_deref()) {
            Ok(detail) => hands_receipt(&line, started, true, &detail),
            Err(e) => hands_receipt(&line, started, false, &e),
        },
    }
}

pub fn capture_jpeg(path: &Path) -> Result<Vec<u8>, String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let dest = path.to_string_lossy().to_string();
    let ok = if which("grim") {
        Command::new("grim").arg(&dest).status().map(|s| s.success()).unwrap_or(false)
    } else if which("ffmpeg") && std::env::var("DISPLAY").is_ok() {
        let display = std::env::var("DISPLAY").unwrap_or_else(|_| ":0".into());
        Command::new("ffmpeg")
            .args([
                "-y", "-hide_banner", "-loglevel", "error",
                "-f", "x11grab", "-video_size", "1280x720", "-i", &display,
                "-frames:v", "1", "-q:v", "5", &dest,
            ])
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    } else if which("scrot") {
        Command::new("scrot").args(["-o", &dest]).status().map(|s| s.success()).unwrap_or(false)
    } else {
        false
    };
    if !ok {
        return Err("no grim/ffmpeg/scrot for a desktop frame".into());
    }
    std::fs::read(path).map_err(|e| e.to_string())
}

pub fn capture_data_url() -> Result<String, String> {
    let path = std::env::temp_dir().join("grokhub-desk.jpg");
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
    let status = match bin.as_str() {
        "whisper-cli" | "whisper.cpp" => Command::new(&bin)
            .args([dest, "-otxt", "-of", out_dir.join("grokhub-voice").to_str().unwrap_or("/tmp/grokhub-voice")])
            .status(),
        _ => Command::new(&bin)
            .args([
                dest,
                "--output_format",
                "txt",
                "--output_dir",
                out_dir.to_str().unwrap_or("/tmp"),
            ])
            .status(),
    }
    .map_err(|e| e.to_string())?;
    if !status.success() {
        return Err(format!("{bin} failed"));
    }
    let txt = wav.with_extension("txt");
    let alt = out_dir.join("grokhub-voice.txt");
    std::fs::read_to_string(&txt)
        .or_else(|_| std::fs::read_to_string(alt))
        .map_err(|e| e.to_string())
}

fn run_ok(bin: &str, args: &[&str]) -> Result<(), String> {
    let st = Command::new(bin).args(args).status().map_err(|e| e.to_string())?;
    if st.success() {
        Ok(())
    } else {
        Err(format!("{bin} failed"))
    }
}

pub fn imagine_save_path(slug: &str) -> PathBuf {
    crate::config::imagine_dir().join(format!("{slug}.png"))
}

/// Second frame for a wall cover when the second Imagine call fails.
pub fn sibling_still(src: &std::path::Path, dest: &std::path::Path) -> Result<(), String> {
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
    let path = std::env::temp_dir().join("grokhub-cam.jpg");
    let dest = path.to_string_lossy().to_string();
    let ok = Command::new("ffmpeg")
        .args([
            "-y",
            "-hide_banner",
            "-loglevel",
            "error",
            "-f",
            "v4l2",
            "-i",
            "/dev/video0",
            "-frames:v",
            "1",
            "-q:v",
            "6",
            &dest,
        ])
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if !ok {
        return Err("webcam capture failed".into());
    }
    let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
    let _ = std::fs::remove_file(&path);
    if bytes.len() < 32 {
        return Err("empty webcam frame".into());
    }
    Ok(jpeg_data_url(&bytes))
}

/// Short PCM chunks for the realtime socket. Empty iterator if no recorder.
pub fn record_pcm_chunks() -> Vec<Vec<u8>> {
    let dest = std::env::temp_dir().join("grokhub-voice-live.wav");
    let path = dest.to_string_lossy().to_string();
    let ok = match first_bin(RECORDERS).as_deref() {
        Some("arecord") => Command::new("arecord")
            .args(["-q", "-d", "1", "-f", "S16_LE", "-r", "24000", "-c", "1", "-t", "wav", &path])
            .status()
            .map(|s| s.success())
            .unwrap_or(false),
        Some("ffmpeg") => Command::new("ffmpeg")
            .args([
                "-y", "-hide_banner", "-loglevel", "error",
                "-f", "pulse", "-i", "default", "-t", "1", "-ac", "1", "-ar", "24000",
                "-f", "s16le", &path,
            ])
            .status()
            .map(|s| s.success())
            .unwrap_or(false),
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
        if let Ok(o) = Command::new(bin).args(args).output() {
            if !o.status.success() || o.stdout.len() < 24 {
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
    let s = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    Ok(take_text_body(&s))
}

pub fn clipboard_once() -> Option<String> {
    for (bin, args) in [
        ("wl-paste", &[] as &[&str]),
        ("xclip", &["-o", "-selection", "clipboard"]),
        ("xsel", &["-ob"]),
    ] {
        if let Ok(o) = Command::new(bin).args(args).output() {
            if o.status.success() {
                let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
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
    fn named_row_center_and_geometry() {
        let rows = vec![AtspiRow {
            name: "Save".into(),
            role: "push button".into(),
            x: 10,
            y: 20,
            w: 80,
            h: 40,
        }];
        let r = named_row(&rows, "save").unwrap();
        assert_eq!(row_center(r), (50, 40));
        let g = parse_getwindowgeometry(
            "Window 1\n  Position: 10,20 (screen: 0)\n  Geometry: 100x40\n",
        )
        .unwrap();
        assert_eq!(g, (10, 20, 100, 40));
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
        match computer_drive(&ComputerOp::Click { x: 1, y: 2 }) {
            ComputerDrive::Xdotool(steps) => {
                assert!(!steps.iter().any(|s| s.iter().any(|a| a == "--sync")));
            }
            other => panic!("{other:?}"),
        }
    }
}
