//! Linux Wayland. KDE uses the RemoteDesktop portal and KWin ScreenShot2.
//! wlroots keeps grim and ydotool.

use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

use grokhub_core::desktop_mcp::{
    current_desktop_is_kde, join_kscreen_with_eis, mapped_point_to_eis, parse_kscreen_doctor,
    pick_eis_region, plan_eis_text, select_desktop_stack, union_monitor, CapturedShot,
    DesktopBackend, DesktopCaptureKind, DesktopInputKind, DesktopMonitorKind, DesktopProbes,
    DesktopSessionEnv, DesktopStack, EisRegion, EisTextPlan, KeyCombo, KeyName, MonitorGeom,
    MouseButton, ShotGeom,
};
use grokhub_core::{grim_capture_args, ydotool_socket_path};

use super::keys::{evdev_mods, evdev_of};
use super::outputs::{parse_sway_outputs, parse_wlr_randr};

#[cfg(target_os = "linux")]
use super::portal::{self, PortalDevice, RestoreStore};

const MISS: &str = "Or log into an X11 session.";

trait Injector {
    fn run(&mut self, args: &[String]) -> Result<(), String>;
}

struct RealInjector;

impl Injector for RealInjector {
    fn run(&mut self, args: &[String]) -> Result<(), String> {
        input_ready()?;
        let out = run_bin("ydotool", args, 4000)?;
        if out.status.success() {
            Ok(())
        } else {
            Err(stderr_line("ydotool", &out))
        }
    }
}

#[cfg(target_os = "linux")]
struct PortalSlot {
    device: Box<dyn PortalDevice>,
    store: Box<dyn RestoreStore>,
    open: bool,
    text: bool,
    keyboard: bool,
}

pub(crate) struct WaylandBackend {
    lock_at: Option<(Instant, bool)>,
    stack: DesktopStack,
    regions: Vec<EisRegion>,
    note: Option<String>,
    needs_restart: bool,
    was_locked: bool,
    fallback_ydotool: bool,
    injector: Box<dyn Injector>,
    #[cfg(target_os = "linux")]
    portal: Option<PortalSlot>,
}

impl WaylandBackend {
    pub(crate) fn new() -> Self {
        let env = session_env();
        let probes = probes_for(&env);
        let stack = host_stack(&env, &probes);
        Self {
            lock_at: None,
            stack,
            regions: Vec::new(),
            note: None,
            needs_restart: false,
            was_locked: false,
            fallback_ydotool: false,
            injector: Box::new(RealInjector),
            #[cfg(target_os = "linux")]
            portal: portal_slot(&stack),
        }
    }

    #[cfg(all(test, target_os = "linux"))]
    fn from_fakes(
        device: impl PortalDevice + 'static,
        store: impl RestoreStore + 'static,
        injector: impl Injector + 'static,
    ) -> Self {
        Self {
            lock_at: None,
            stack: DesktopStack {
                input: DesktopInputKind::Portal,
                capture: DesktopCaptureKind::ScreenShot2,
                monitors: DesktopMonitorKind::Kscreen,
            },
            regions: Vec::new(),
            note: None,
            needs_restart: false,
            was_locked: false,
            fallback_ydotool: false,
            injector: Box::new(injector),
            portal: Some(PortalSlot {
                device: Box::new(device),
                store: Box::new(store),
                open: false,
                text: false,
                keyboard: false,
            }),
        }
    }

    fn wants_portal(&self) -> bool {
        self.stack.input == DesktopInputKind::Portal && !self.fallback_ydotool
    }

    fn prepare_input(&mut self) -> Result<(), String> {
        if self.needs_restart {
            return Err("Desktop control was halted. The portal session is closed.".into());
        }
        if !self.wants_portal() {
            return Ok(());
        }
        #[cfg(target_os = "linux")]
        {
            if self.portal.as_ref().is_some_and(|slot| slot.open) {
                return Ok(());
            }
            self.open_portal();
            if self.needs_restart {
                return Err("Desktop control was halted. The portal session is closed.".into());
            }
        }
        Ok(())
    }

    #[cfg(target_os = "linux")]
    fn open_portal(&mut self) {
        let outcome = {
            let Some(slot) = self.portal.as_mut() else {
                self.fallback_ydotool = true;
                self.note_imprecise("The remote-control portal is unavailable.");
                return;
            };
            portal::open_with_restore(slot.device.as_mut(), slot.store.as_ref())
        };
        match outcome {
            Ok(opened) => {
                if opened.regions.is_empty() {
                    if let Some(slot) = self.portal.as_mut() {
                        slot.device.close();
                        slot.open = false;
                    }
                    self.fallback_ydotool = true;
                    self.note_imprecise("The portal EIS connection has no pointer region.");
                    return;
                }
                self.regions = opened.regions;
                if let Some(slot) = self.portal.as_mut() {
                    slot.text = opened.text;
                    slot.keyboard = opened.keyboard;
                    slot.open = true;
                }
                if let Some(notice) = opened.notice {
                    self.note = Some(notice);
                }
            }
            Err(err) => {
                if let Some(slot) = self.portal.as_mut() {
                    slot.device.close();
                    slot.open = false;
                }
                self.fallback_ydotool = true;
                self.note_imprecise(err.text());
            }
        }
    }

    fn note_imprecise(&mut self, reason: &str) {
        let note = format!(
            "{reason} Pointer and keyboard input fell back to ydotool and is imprecise."
        );
        eprintln!("desktop-mcp: {note}");
        self.note = Some(note);
    }

    fn move_to(&mut self, x: i32, y: i32) -> Result<(), String> {
        self.injector.run(&[
            "mousemove".into(),
            "--absolute".into(),
            x.to_string(),
            y.to_string(),
        ])
    }

    #[cfg(target_os = "linux")]
    fn portal_pointer(&mut self, x: f32, y: f32) -> Result<(), String> {
        let Some(slot) = self.portal.as_mut() else {
            return Err("The portal session is closed.".into());
        };
        slot.device.pointer_absolute(x, y)
    }

    #[cfg(target_os = "linux")]
    fn type_portal(&mut self, text: &str) -> Result<(), String> {
        let offered = self.portal.as_ref().is_some_and(|slot| slot.text);
        let plan = plan_eis_text(offered, text, char_code)?;
        let Some(slot) = self.portal.as_mut() else {
            return Err("The portal session is closed.".into());
        };
        match plan {
            EisTextPlan::Text => slot.device.text(text),
            EisTextPlan::Keycodes(codes) => {
                if !slot.keyboard {
                    return Err(
                        "EIS TEXT is not offered and the device has no keyboard.".into(),
                    );
                }
                for code in codes {
                    slot.device.key(u32::from(code), true)?;
                    slot.device.key(u32::from(code), false)?;
                }
                Ok(())
            }
        }
    }

    #[cfg(target_os = "linux")]
    fn key_portal(&mut self, combo: &KeyCombo) -> Result<(), String> {
        let mods = evdev_mods(combo);
        let key = evdev_of(&combo.key).ok_or_else(|| {
            "That key has no evdev code on Wayland. Use type for characters.".to_string()
        })?;
        let Some(slot) = self.portal.as_mut() else {
            return Err("The portal session is closed.".into());
        };
        if !slot.keyboard {
            return Err("The EIS device has no keyboard capability.".into());
        }
        for code in &mods {
            slot.device.key(u32::from(*code), true)?;
        }
        slot.device.key(u32::from(key), true)?;
        slot.device.key(u32::from(key), false)?;
        for code in mods.iter().rev() {
            slot.device.key(u32::from(*code), false)?;
        }
        Ok(())
    }

    fn wl_monitors(&self) -> Result<Vec<MonitorGeom>, String> {
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

    #[cfg(target_os = "linux")]
    fn kde_monitors(&mut self) -> Result<Vec<MonitorGeom>, String> {
        let parsed = self.kscreen();
        if self.regions.is_empty() {
            return parsed;
        }
        let base = match parsed {
            Ok(mons) => mons,
            Err(err) => {
                eprintln!("desktop-mcp: {err}");
                Vec::new()
            }
        };
        let joined = join_kscreen_with_eis(&base, &self.regions);
        if joined.is_empty() {
            return Err("kscreen-doctor listed no enabled outputs.".into());
        }
        Ok(joined)
    }

    #[cfg(target_os = "linux")]
    fn kscreen(&self) -> Result<Vec<MonitorGeom>, String> {
        if !crate::desktop::which("kscreen-doctor") {
            return Err(format!(
                "Wayland monitor list on KDE needs kscreen-doctor. {MISS}"
            ));
        }
        let out = run_bin("kscreen-doctor", &["-j".into()], 3000)?;
        parse_kscreen_doctor(&String::from_utf8_lossy(&out.stdout))
    }

    fn grab_grim(&self, output: Option<&str>) -> Result<(Vec<u8>, u32, u32), String> {
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
                .map_err(|err| format!("grim image: {err}"))
        });
        let _ = std::fs::remove_file(&dest);
        let image = image?;
        let (w, h) = image.dimensions();
        Ok((image.into_raw(), w, h))
    }

    #[cfg(target_os = "linux")]
    fn screenshot_kwin(&mut self, monitor: &str) -> Result<CapturedShot, String> {
        let mons = self.kde_monitors().unwrap_or_default();
        if monitor != "all" {
            let mon = mons
                .iter()
                .find(|item| item.id == monitor || item.name == monitor)
                .cloned()
                .ok_or_else(|| format!("No monitor \"{monitor}\"."))?;
            let (rgba, meta) = super::kwin_shot::capture(Some(&mon.name))?;
            return shot_from(&mon.id, mon.x, mon.y, meta.scale, meta.width, meta.height, &rgba);
        }
        let (rgba, meta) = super::kwin_shot::capture(None)?;
        let origin = union_monitor(&mons);
        let (x, y) = match &origin {
            Some(geom) => (geom.x, geom.y),
            None => (0, 0),
        };
        shot_from("all", x, y, meta.scale, meta.width, meta.height, &rgba)
    }

    fn locked_now(&self) -> bool {
        let titles = crate::desktop::lock_titles();
        let refs: Vec<&str> = titles.iter().map(String::as_str).collect();
        grokhub_core::hands_blocked_by_lock(&grokhub_core::ComputerOp::Move { x: 0, y: 0 }, &refs)
    }
}

impl DesktopBackend for WaylandBackend {
    fn list_monitors(&mut self) -> Result<Vec<MonitorGeom>, String> {
        if self.stack.monitors == DesktopMonitorKind::Kscreen {
            #[cfg(target_os = "linux")]
            {
                return self.kde_monitors();
            }
            #[cfg(not(target_os = "linux"))]
            {
                return Err(format!("Wayland monitor list on KDE needs kscreen-doctor. {MISS}"));
            }
        }
        self.wl_monitors()
    }

    fn screenshot(&mut self, monitor: &str) -> Result<CapturedShot, String> {
        if self.stack.capture == DesktopCaptureKind::ScreenShot2 {
            #[cfg(target_os = "linux")]
            {
                return self.screenshot_kwin(monitor);
            }
            #[cfg(not(target_os = "linux"))]
            {
                return Err("KWin ScreenShot2 is only available on Linux.".into());
            }
        }
        let mons = self.wl_monitors().unwrap_or_default();
        if monitor != "all" {
            let mon = mons
                .iter()
                .find(|item| item.id == monitor || item.name == monitor)
                .cloned()
                .ok_or_else(|| format!("No monitor \"{monitor}\"."))?;
            let (rgba, w, h) = self.grab_grim(Some(&mon.name))?;
            return finish(&mon, w, h, &rgba);
        }
        let (rgba, w, h) = self.grab_grim(None)?;
        let origin = union_monitor(&mons);
        let (x, y, scale) = match &origin {
            Some(geom) => (geom.x, geom.y, geom.scale_factor),
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
        self.prepare_input()?;
        if self.wants_portal() {
            #[cfg(target_os = "linux")]
            {
                if self.regions.is_empty() {
                    return Err("The portal session has no pointer region.".into());
                }
                let abs = pick_eis_region(&self.regions, f64::from(x), f64::from(y));
                return self.portal_pointer(abs.x, abs.y);
            }
        }
        self.move_to(x, y)
    }

    fn move_abs_on(&mut self, geom: &ShotGeom, x: i32, y: i32) -> Result<(), String> {
        self.prepare_input()?;
        if self.wants_portal() {
            #[cfg(target_os = "linux")]
            {
                let abs = mapped_point_to_eis(geom, &self.regions, x, y)
                    .ok_or_else(|| "The portal session has no pointer region.".to_string())?;
                return self.portal_pointer(abs.x, abs.y);
            }
        }
        self.move_to(x, y)
    }

    fn button(&mut self, button: MouseButton, down: bool) -> Result<(), String> {
        self.prepare_input()?;
        if self.wants_portal() {
            #[cfg(target_os = "linux")]
            {
                let code = match button {
                    MouseButton::Left => 0x110,
                    MouseButton::Right => 0x111,
                    MouseButton::Middle => 0x112,
                };
                let Some(slot) = self.portal.as_mut() else {
                    return Err("The portal session is closed.".into());
                };
                return slot.device.button(code, down);
            }
        }
        let code = match (button, down) {
            (MouseButton::Left, true) => 0x40,
            (MouseButton::Left, false) => 0x80,
            (MouseButton::Right, true) => 0x41,
            (MouseButton::Right, false) => 0x81,
            (MouseButton::Middle, true) => 0x42,
            (MouseButton::Middle, false) => 0x82,
        };
        self.injector
            .run(&["click".into(), format!("0x{code:X}")])
    }

    fn scroll(&mut self, dx: i32, dy: i32) -> Result<(), String> {
        if dx == 0 && dy == 0 {
            return Ok(());
        }
        self.prepare_input()?;
        if self.wants_portal() {
            #[cfg(target_os = "linux")]
            {
                let Some(slot) = self.portal.as_mut() else {
                    return Err("The portal session is closed.".into());
                };
                return slot.device.scroll(dx, dy);
            }
        }
        self.injector.run(&[
            "mousemove".into(),
            "--wheel".into(),
            dx.clamp(-30, 30).to_string(),
            dy.clamp(-30, 30).to_string(),
        ])
    }

    fn type_text(&mut self, text: &str) -> Result<(), String> {
        self.prepare_input()?;
        if self.wants_portal() {
            #[cfg(target_os = "linux")]
            {
                return self.type_portal(text);
            }
        }
        self.injector
            .run(&["type".into(), "--".into(), text.to_string()])
    }

    fn key_combo(&mut self, combo: &KeyCombo) -> Result<(), String> {
        self.prepare_input()?;
        if self.wants_portal() {
            #[cfg(target_os = "linux")]
            {
                return self.key_portal(combo);
            }
        }
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
        self.injector.run(&args)
    }

    fn is_locked(&mut self) -> bool {
        if let Some((at, locked)) = self.lock_at {
            if at.elapsed() < Duration::from_secs(2) {
                return locked;
            }
        }
        let locked = self.locked_now();
        if self.was_locked && !locked {
            self.needs_restart = false;
        }
        self.was_locked = locked;
        self.lock_at = Some((Instant::now(), locked));
        locked
    }

    fn pace(&mut self) {
        std::thread::sleep(Duration::from_millis(15));
    }

    fn release_input(&mut self) {
        let portal_route = self.wants_portal();
        #[cfg(target_os = "linux")]
        if let Some(slot) = self.portal.as_mut() {
            slot.device.close();
            slot.open = false;
        }
        if portal_route {
            self.needs_restart = true;
        }
        self.regions.clear();
    }

    fn status_note(&mut self) -> Option<String> {
        self.note.clone()
    }
}

fn char_code(ch: char) -> Option<u16> {
    let name = match ch {
        '\n' | '\r' => KeyName::Return,
        '\t' => KeyName::Tab,
        ' ' => KeyName::Space,
        other => KeyName::Char(other),
    };
    evdev_of(&name)
}

fn session_env() -> DesktopSessionEnv {
    DesktopSessionEnv {
        wayland_display: std::env::var("WAYLAND_DISPLAY").ok(),
        session_type: std::env::var("XDG_SESSION_TYPE").ok(),
        current_desktop: std::env::var("XDG_CURRENT_DESKTOP").ok(),
        display: std::env::var("DISPLAY").ok(),
    }
}

fn probes_for(env: &DesktopSessionEnv) -> DesktopProbes {
    #[cfg(target_os = "linux")]
    {
        if current_desktop_is_kde(env.current_desktop.as_deref()) {
            return DesktopProbes { kwin: false };
        }
        DesktopProbes {
            kwin: super::kwin_shot::kwin_running(),
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = env;
        DesktopProbes { kwin: false }
    }
}

/// Portal input needs the Linux RemoteDesktop worker. Other Unix builds keep
/// the grim and ydotool route the selector would have used on wlroots.
fn host_stack(env: &DesktopSessionEnv, probes: &DesktopProbes) -> DesktopStack {
    let stack = select_desktop_stack(env, probes);
    #[cfg(target_os = "linux")]
    {
        stack
    }
    #[cfg(not(target_os = "linux"))]
    {
        if stack.input == DesktopInputKind::Portal {
            DesktopStack {
                input: DesktopInputKind::Ydotool,
                capture: DesktopCaptureKind::Grim,
                monitors: DesktopMonitorKind::Wlroots,
            }
        } else {
            stack
        }
    }
}

#[cfg(target_os = "linux")]
fn portal_slot(stack: &DesktopStack) -> Option<PortalSlot> {
    if stack.input != DesktopInputKind::Portal {
        return None;
    }
    Some(PortalSlot {
        device: Box::new(portal::AshpdPortal::new()),
        store: Box::new(portal::KeychainRestoreStore),
        open: false,
        text: false,
        keyboard: false,
    })
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

#[cfg(target_os = "linux")]
fn shot_from(
    id: &str,
    x: i32,
    y: i32,
    scale: f64,
    w: u32,
    h: u32,
    rgba: &[u8],
) -> Result<CapturedShot, String> {
    let (bytes, mime, iw, ih) = super::encode_rgba(rgba, w, h)?;
    Ok(CapturedShot {
        bytes,
        mime,
        geom: ShotGeom {
            id: id.into(),
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

fn input_ready() -> Result<(), String> {
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

fn run_bin(bin: &str, args: &[String], ms: u64) -> Result<Output, String> {
    let program = crate::desktop::resolve_bin(bin).unwrap_or_else(|| std::path::PathBuf::from(bin));
    let mut cmd = Command::new(program);
    cmd.args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let child = cmd.spawn().map_err(|err| format!("{bin}: {err}"))?;
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
        Ok(Err(err)) => Err(format!("{bin}: {err}")),
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

#[cfg(all(test, target_os = "linux"))]
mod route_tests {
    use super::*;
    use crate::desktop_mcp::portal::doubles::{FakePortal, FakeReply, MemoryRestoreStore};
    use std::sync::{Arc, Mutex};

    struct RecordingInjector {
        log: Arc<Mutex<Vec<Vec<String>>>>,
    }

    impl RecordingInjector {
        fn new() -> Self {
            Self {
                log: Arc::new(Mutex::new(Vec::new())),
            }
        }

        fn calls(&self) -> Vec<Vec<String>> {
            self.log.lock().unwrap_or_else(|err| err.into_inner()).clone()
        }
    }

    impl Injector for RecordingInjector {
        fn run(&mut self, args: &[String]) -> Result<(), String> {
            self.log
                .lock()
                .unwrap_or_else(|err| err.into_inner())
                .push(args.to_vec());
            Ok(())
        }
    }

    #[test]
    fn denied_portal_falls_back_to_ydotool_cleanly() {
        let portal = FakePortal::script(vec![FakeReply::Deny("user cancelled the dialog".into())]);
        let ydotool = RecordingInjector::new();
        let calls = Arc::clone(&ydotool.log);
        let mut backend =
            WaylandBackend::from_fakes(portal.clone(), MemoryRestoreStore::new(), ydotool);
        backend.move_abs(10, 20).unwrap();
        let note = backend.status_note().expect("fallback note");
        assert!(note.contains("imprecise"), "{note}");
        assert!(
            note.contains("user cancelled"),
            "{note}"
        );
        let log = calls.lock().unwrap_or_else(|err| err.into_inner()).clone();
        assert!(
            log.iter().any(|args| args.iter().any(|arg| arg == "mousemove")),
            "{log:?}"
        );
        assert_eq!(portal.start_count(), 1);
        backend.move_abs(12, 22).unwrap();
        assert_eq!(portal.start_count(), 1);
        assert!(calls.lock().unwrap_or_else(|err| err.into_inner()).len() >= 2);
    }

    #[test]
    fn halt_closes_the_portal_session() {
        let portal = FakePortal::script(vec![FakeReply::Token("token-1".into())]);
        let ydotool = RecordingInjector::new();
        let calls = Arc::clone(&ydotool.log);
        let mut backend =
            WaylandBackend::from_fakes(portal.clone(), MemoryRestoreStore::new(), ydotool);
        backend.move_abs(10, 10).unwrap();
        assert_eq!(portal.pointers().len(), 1);
        assert!(
            calls.lock().unwrap_or_else(|err| err.into_inner()).is_empty(),
            "halted portal input must not reach ydotool"
        );
        backend.release_input();
        assert!(portal.closed_count() >= 1);
        let err = backend.move_abs(11, 11).unwrap_err();
        assert!(err.contains("halted") || err.contains("closed"), "{err}");
        assert_eq!(portal.start_count(), 1);
        assert_eq!(portal.pointers().len(), 1);
        assert!(calls.lock().unwrap_or_else(|err| err.into_inner()).is_empty());
    }
}
