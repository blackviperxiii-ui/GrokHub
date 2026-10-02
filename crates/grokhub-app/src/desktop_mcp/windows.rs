//! Windows screen: xcap capture, SendInput, per-monitor DPI v2.

use std::time::{Duration, Instant};

use grokhub_core::desktop_mcp::{
    union_monitor, CapturedShot, DesktopBackend, KeyCombo, MonitorGeom, MouseButton, ShotGeom,
};

use super::keys::{vk_mods, vk_of};

use windows_sys::Win32::System::StationsAndDesktops::{
    CloseDesktop, OpenInputDesktop, DESKTOP_READOBJECTS,
};
use windows_sys::Win32::UI::HiDpi::{
    SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE, KEYBDINPUT, KEYEVENTF_KEYUP,
    KEYEVENTF_UNICODE, MOUSEEVENTF_HWHEEL, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP,
    MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP, MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP,
    MOUSEEVENTF_WHEEL, MOUSEINPUT, VK_RETURN,
};
use windows_sys::Win32::UI::WindowsAndMessaging::SetCursorPos;

const LOCK_MSG: &str = "The lock screen is up. Unlock this computer, then try again.";

pub(crate) fn enable_per_monitor_dpi() {
    unsafe {
        SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
}

pub(crate) struct WindowsBackend {
    lock_at: Option<(Instant, bool)>,
}

impl WindowsBackend {
    pub(crate) fn new() -> Self {
        Self { lock_at: None }
    }

    fn desktop_open(&self) -> bool {
        unsafe {
            let desk = OpenInputDesktop(0, 0, DESKTOP_READOBJECTS);
            if desk.is_null() {
                return false;
            }
            CloseDesktop(desk);
            true
        }
    }

    fn send(&self, inputs: &[INPUT]) -> Result<(), String> {
        if inputs.is_empty() {
            return Ok(());
        }
        let sent = unsafe {
            SendInput(
                inputs.len() as u32,
                inputs.as_ptr(),
                std::mem::size_of::<INPUT>() as i32,
            )
        };
        if sent == inputs.len() as u32 {
            Ok(())
        } else {
            Err(LOCK_MSG.into())
        }
    }

    fn key_input(vk: u16, scan: u16, flags: u32) -> INPUT {
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: vk,
                    wScan: scan,
                    dwFlags: flags,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        }
    }

    fn mouse_input(flags: u32, data: u32) -> INPUT {
        INPUT {
            r#type: INPUT_MOUSE,
            Anonymous: INPUT_0 {
                mi: MOUSEINPUT {
                    dx: 0,
                    dy: 0,
                    mouseData: data,
                    dwFlags: flags,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        }
    }
}

impl DesktopBackend for WindowsBackend {
    fn list_monitors(&mut self) -> Result<Vec<MonitorGeom>, String> {
        let monitors = xcap::Monitor::all().map_err(|e| e.to_string())?;
        let mut out = Vec::new();
        for mon in monitors {
            let id = mon.id().map_err(|e| e.to_string())?;
            let name = mon
                .friendly_name()
                .or_else(|_| mon.name())
                .unwrap_or_else(|_| format!("monitor-{id}"));
            let scale = mon.scale_factor().unwrap_or(1.0) as f64;
            out.push(MonitorGeom {
                id: id.to_string(),
                name,
                x: mon.x().map_err(|e| e.to_string())?,
                y: mon.y().map_err(|e| e.to_string())?,
                width: mon.width().map_err(|e| e.to_string())?,
                height: mon.height().map_err(|e| e.to_string())?,
                scale_factor: if scale.is_finite() && scale > 0.0 { scale } else { 1.0 },
                primary: mon.is_primary().unwrap_or(false),
            });
        }
        Ok(out)
    }

    fn screenshot(&mut self, monitor: &str) -> Result<CapturedShot, String> {
        let mons = self.list_monitors()?;
        if monitor == "all" {
            let geom = union_monitor(&mons).ok_or_else(|| "No monitors.".to_string())?;
            let mut canvas = vec![0u8; (geom.physical_w as usize) * (geom.physical_h as usize) * 4];
            let captured = xcap::Monitor::all().map_err(|e| e.to_string())?;
            for (mon, cap) in mons.iter().zip(captured) {
                let image = cap.capture_image().map_err(|e| e.to_string())?;
                blit(
                    &mut canvas,
                    geom.physical_w,
                    geom.physical_h,
                    mon.x - geom.x,
                    mon.y - geom.y,
                    image.as_raw(),
                    image.width(),
                    image.height(),
                );
            }
            return shot_from("all", geom.x, geom.y, geom.physical_w, geom.physical_h, geom.scale_factor, &canvas);
        }
        let want = mons
            .iter()
            .find(|m| m.id == monitor || m.name == monitor)
            .cloned()
            .ok_or_else(|| format!("No monitor \"{monitor}\"."))?;
        let captured = xcap::Monitor::all().map_err(|e| e.to_string())?;
        let cap = captured
            .into_iter()
            .find(|m| m.id().ok().map(|id| id.to_string()) == Some(want.id.clone()))
            .ok_or_else(|| format!("No monitor \"{monitor}\"."))?;
        let image = cap.capture_image().map_err(|e| e.to_string())?;
        shot_from(
            &want.id,
            want.x,
            want.y,
            image.width(),
            image.height(),
            want.scale_factor,
            image.as_raw(),
        )
    }

    fn move_abs(&mut self, x: i32, y: i32) -> Result<(), String> {
        if !self.desktop_open() {
            return Err(LOCK_MSG.into());
        }
        let ok = unsafe { SetCursorPos(x, y) } != 0;
        if ok {
            Ok(())
        } else {
            Err(LOCK_MSG.into())
        }
    }

    fn button(&mut self, button: MouseButton, down: bool) -> Result<(), String> {
        if !self.desktop_open() {
            return Err(LOCK_MSG.into());
        }
        let flags = match (button, down) {
            (MouseButton::Left, true) => MOUSEEVENTF_LEFTDOWN,
            (MouseButton::Left, false) => MOUSEEVENTF_LEFTUP,
            (MouseButton::Right, true) => MOUSEEVENTF_RIGHTDOWN,
            (MouseButton::Right, false) => MOUSEEVENTF_RIGHTUP,
            (MouseButton::Middle, true) => MOUSEEVENTF_MIDDLEDOWN,
            (MouseButton::Middle, false) => MOUSEEVENTF_MIDDLEUP,
        };
        self.send(&[Self::mouse_input(flags, 0)])
    }

    fn scroll(&mut self, dx: i32, dy: i32) -> Result<(), String> {
        if !self.desktop_open() {
            return Err(LOCK_MSG.into());
        }
        let mut inputs = Vec::new();
        if dy != 0 {
            inputs.push(Self::mouse_input(MOUSEEVENTF_WHEEL, wheel_data(dy)));
        }
        if dx != 0 {
            inputs.push(Self::mouse_input(MOUSEEVENTF_HWHEEL, wheel_data(dx)));
        }
        self.send(&inputs)
    }

    fn type_text(&mut self, text: &str) -> Result<(), String> {
        if !self.desktop_open() {
            return Err(LOCK_MSG.into());
        }
        let mut inputs = Vec::new();
        for ch in text.chars() {
            if ch == '\n' || ch == '\r' {
                inputs.push(Self::key_input(VK_RETURN, 0, 0));
                inputs.push(Self::key_input(VK_RETURN, 0, KEYEVENTF_KEYUP));
                continue;
            }
            for unit in utf16_units(ch) {
                inputs.push(Self::key_input(0, unit, KEYEVENTF_UNICODE));
                inputs.push(Self::key_input(0, unit, KEYEVENTF_UNICODE | KEYEVENTF_KEYUP));
            }
        }
        self.send(&inputs)
    }

    fn key_combo(&mut self, combo: &KeyCombo) -> Result<(), String> {
        if !self.desktop_open() {
            return Err(LOCK_MSG.into());
        }
        let mods = vk_mods(combo);
        let mut inputs = Vec::new();
        for vk in &mods {
            inputs.push(Self::key_input(*vk, 0, 0));
        }
        if let Some(vk) = vk_of(&combo.key) {
            inputs.push(Self::key_input(vk, 0, 0));
            inputs.push(Self::key_input(vk, 0, KEYEVENTF_KEYUP));
        } else if let grokhub_core::desktop_mcp::KeyName::Char(ch) = combo.key {
            for unit in utf16_units(ch) {
                inputs.push(Self::key_input(0, unit, KEYEVENTF_UNICODE));
                inputs.push(Self::key_input(0, unit, KEYEVENTF_UNICODE | KEYEVENTF_KEYUP));
            }
        } else {
            return Err("That key has no Windows virtual-key code.".into());
        }
        for vk in mods.iter().rev() {
            inputs.push(Self::key_input(*vk, 0, KEYEVENTF_KEYUP));
        }
        self.send(&inputs)
    }

    fn is_locked(&mut self) -> bool {
        if let Some((at, locked)) = self.lock_at {
            if at.elapsed() < Duration::from_secs(2) {
                return locked;
            }
        }
        let locked = !self.desktop_open();
        self.lock_at = Some((Instant::now(), locked));
        locked
    }

    fn pace(&mut self) {
        std::thread::sleep(Duration::from_millis(30));
    }
}

fn wheel_data(notches: i32) -> u32 {
    let notches = notches.clamp(-30, 30);
    (notches.saturating_mul(120)) as u32
}

fn utf16_units(ch: char) -> Vec<u16> {
    let mut buf = [0u16; 2];
    ch.encode_utf16(&mut buf).to_vec()
}

fn shot_from(
    id: &str,
    x: i32,
    y: i32,
    w: u32,
    h: u32,
    scale_factor: f64,
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
            scale_factor,
            image_w: iw,
            image_h: ih,
        },
    })
}

fn blit(dst: &mut [u8], dw: u32, dh: u32, x: i32, y: i32, src: &[u8], sw: u32, sh: u32) {
    for row in 0..sh {
        let dy = y.saturating_add(row as i32);
        if dy < 0 || dy >= dh as i32 {
            continue;
        }
        for col in 0..sw {
            let dx = x.saturating_add(col as i32);
            if dx < 0 || dx >= dw as i32 {
                continue;
            }
            let si = (row as usize * sw as usize + col as usize) * 4;
            let di = (dy as usize * dw as usize + dx as usize) * 4;
            if si + 4 <= src.len() && di + 4 <= dst.len() {
                dst[di..di + 4].copy_from_slice(&src[si..si + 4]);
            }
        }
    }
}

