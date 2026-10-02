//! Linux Wayland: grim for capture, ydotool for input. No XWayland fallback.

use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

use grokhub_core::desktop_mcp::{
    union_monitor, CapturedShot, DesktopBackend, KeyCombo, MonitorGeom, MouseButton, ShotGeom,
};
use grokhub_core::{grim_capture_args, ydotool_socket_path};

use super::keys::{evdev_mods, evdev_of};
use super::outputs::{parse_sway_outputs, parse_wlr_randr};

const MISS: &str = "Or log into an X11 session.";

pub(crate) struct WaylandBackend {
    lock_at: Option<(Instant, bool)>,
}

impl WaylandBackend {
    pub(crate) fn new() -> Self {
        Self { lock_at: None }
    }

    fn monitors(&self) -> Result<Vec<MonitorGeom>, String> {
        if crate::desktop::which("swaymsg") {
            if let Ok(out) = run_bin("swaymsg", &["-t".into(), "get_outputs".into()], 3000) {
                let text = String::from_utf8_lossy(&out.stdout);
                if let Some(mons) = parse_sway_outputs(&text) {
                    return Ok(mons);
                }
            }
        }
        if crate::desktop::which("wlr-randr") {
            if let Ok(out) = run_bin("wlr-randr", &[], 3000) {
                let mons = parse_wlr_randr(&String::from_utf8_lossy(&out.stdout));
                if !mons.is_empty() {
                    return Ok(mons);
                }
            }
        }
        Err(format!(
            "Wayland monitor list needs swaymsg or wlr-randr. {MISS}"
        ))
    }

    fn grab(&self, output: Option<&str>) -> Result<(Vec<u8>, u32, u32), String> {
        if !crate::desktop::which("grim") {
            return Err(format!("Wayland capture needs grim. {MISS}"));
        }
        let dest = std::env::temp_dir().join(format!(
            "grokhub-desk-{}-{}.png",
            std::process::id(),
            grokhub_core::now_ms()
        ));
        let args = grim_capture_args(&dest.display().to_string(), output);
        let run = run_bin("grim", &args, 8000);
        let image = run.and_then(|_| {
            image::open(&dest)
                .map(|img| img.to_rgba8())
                .map_err(|e| format!("grim image: {e}"))
        });
        let _ = std::fs::remove_file(&dest);
        let image = image?;
        let (w, h) = image.dimensions();
        Ok((image.into_raw(), w, h))
    }

    fn ydo(&self, args: &[String]) -> Result<(), String> {
        self.input_ready()?;
        let out = run_bin("ydotool", args, 4000)?;
        if out.status.success() {
            Ok(())
        } else {
            Err(stderr_line("ydotool", &out))
        }
    }

    fn input_ready(&self) -> Result<(), String> {
        if !crate::desktop::which("ydotool") {
            return Err(format!(
                "Wayland input needs ydotool and ydotoold, and this user must be in the uinput group. {MISS}"
            ));
        }
        let sock = ydotool_socket_path(
            std::env::var("YDOTOOL_SOCKET").ok().as_deref(),
            std::env::var("XDG_RUNTIME_DIR").ok().as_deref(),
        );
        if sock.exists() || std::path::Path::new("/tmp/.ydotool_socket").exists() {
            return Ok(());
        }
        let uinput = std::path::Path::new("/dev/uinput");
        let writable = uinput.exists()
            && std::fs::OpenOptions::new()
                .write(true)
                .open(uinput)
                .is_ok();
        if writable {
            Err(format!(
                "Wayland input needs ydotoold running (the ydotool socket is missing). {MISS}"
            ))
        } else {
            Err(format!(
                "Wayland input needs ydotool, ydotoold, and write access to /dev/uinput (the uinput group). {MISS}"
            ))
        }
    }

    fn move_to(&self, x: i32, y: i32) -> Result<(), String> {
        self.ydo(&[
            "mousemove".into(),
            "--absolute".into(),
            x.to_string(),
            y.to_string(),
        ])
    }

    fn click_code(&self, code: u8) -> Result<(), String> {
        self.ydo(&["click".into(), format!("0x{code:X}")])
    }

    fn locked_now(&self) -> bool {
        let titles = crate::desktop::lock_titles();
        let refs: Vec<&str> = titles.iter().map(String::as_str).collect();
        grokhub_core::hands_blocked_by_lock(&grokhub_core::ComputerOp::Move { x: 0, y: 0 }, &refs)
    }
}

impl DesktopBackend for WaylandBackend {
    fn list_monitors(&mut self) -> Result<Vec<MonitorGeom>, String> {
        self.monitors()
    }

    fn screenshot(&mut self, monitor: &str) -> Result<CapturedShot, String> {
        let mons = self.monitors().unwrap_or_default();
        if monitor != "all" {
            let mon = mons
                .iter()
                .find(|m| m.id == monitor || m.name == monitor)
                .cloned()
                .ok_or_else(|| format!("No monitor \"{monitor}\"."))?;
            let (rgba, w, h) = self.grab(Some(&mon.name))?;
            return finish(&mon, w, h, &rgba);
        }
        let (rgba, w, h) = self.grab(None)?;
        let origin = union_monitor(&mons);
        let (x, y, scale) = match &origin {
            Some(g) => (g.x, g.y, g.scale_factor),
            None => (0, 0, 1.0),
        };
        let (bytes, mime, iw, ih) = super::encode_rgba(&rgba, w, h)?;
        Ok(CapturedShot {
            bytes,
            mime,
            geom: ShotGeom {
                id: "all".into(),
                x,
                y,
                physical_w: w,
                physical_h: h,
                scale_factor: scale,
                image_w: iw,
                image_h: ih,
            },
        })
    }

    fn move_abs(&mut self, x: i32, y: i32) -> Result<(), String> {
        self.move_to(x, y)
    }

    fn button(&mut self, button: MouseButton, down: bool) -> Result<(), String> {
        let code = match (button, down) {
            (MouseButton::Left, true) => 0x40,
            (MouseButton::Left, false) => 0x80,
            (MouseButton::Right, true) => 0x41,
            (MouseButton::Right, false) => 0x81,
            (MouseButton::Middle, true) => 0x42,
            (MouseButton::Middle, false) => 0x82,
        };
        self.click_code(code)
    }

    fn scroll(&mut self, dx: i32, dy: i32) -> Result<(), String> {
        if dx == 0 && dy == 0 {
            return Ok(());
        }
        self.ydo(&[
            "mousemove".into(),
            "--wheel".into(),
            dx.clamp(-30, 30).to_string(),
            dy.clamp(-30, 30).to_string(),
        ])
    }

    fn type_text(&mut self, text: &str) -> Result<(), String> {
        self.ydo(&["type".into(), "--".into(), text.to_string()])
    }

    fn key_combo(&mut self, combo: &KeyCombo) -> Result<(), String> {
        let mut codes = evdev_mods(combo);
        let key = evdev_of(&combo.key).ok_or_else(|| {
            "That key has no evdev code on Wayland. Use type for characters.".to_string()
        })?;
        codes.push(key);
        let mut args = vec!["key".into()];
        for code in &codes {
            args.push(format!("{code}:1"));
        }
        for code in codes.iter().rev() {
            args.push(format!("{code}:0"));
        }
        self.ydo(&args)
    }

    fn is_locked(&mut self) -> bool {
        if let Some((at, locked)) = self.lock_at {
            if at.elapsed() < Duration::from_secs(2) {
                return locked;
            }
        }
        let locked = self.locked_now();
        self.lock_at = Some((Instant::now(), locked));
        locked
    }

    fn pace(&mut self) {
        std::thread::sleep(Duration::from_millis(15));
    }
}

fn finish(mon: &MonitorGeom, w: u32, h: u32, rgba: &[u8]) -> Result<CapturedShot, String> {
    let (bytes, mime, iw, ih) = super::encode_rgba(rgba, w, h)?;
    Ok(CapturedShot {
        bytes,
        mime,
        geom: ShotGeom {
            id: mon.id.clone(),
            x: mon.x,
            y: mon.y,
            physical_w: w,
            physical_h: h,
            scale_factor: mon.scale_factor,
            image_w: iw,
            image_h: ih,
        },
    })
}

fn run_bin(bin: &str, args: &[String], ms: u64) -> Result<Output, String> {
    let program = crate::desktop::resolve_bin(bin).unwrap_or_else(|| std::path::PathBuf::from(bin));
    let mut cmd = Command::new(program);
    cmd.args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let child = cmd.spawn().map_err(|e| format!("{bin}: {e}"))?;
    let pid = child.id();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(child.wait_with_output());
    });
    match rx.recv_timeout(Duration::from_millis(ms)) {
        Ok(Ok(out)) => {
            if out.status.success() || bin == "swaymsg" || bin == "wlr-randr" {
                Ok(out)
            } else {
                Err(stderr_line(bin, &out))
            }
        }
        Ok(Err(e)) => Err(format!("{bin}: {e}")),
        Err(_) => {
            let _ = Command::new("kill").arg(pid.to_string()).status();
            Err(format!("{bin} timed out. {MISS}"))
        }
    }
}

fn stderr_line(bin: &str, out: &Output) -> String {
    let err = String::from_utf8_lossy(&out.stderr);
    let line = err.lines().next().unwrap_or("").trim();
    if line.is_empty() {
        format!("{bin} failed")
    } else {
        format!("{bin}: {line}")
    }
}
