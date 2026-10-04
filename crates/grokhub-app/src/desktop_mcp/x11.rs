//! Linux X11: RandR monitors, root GetImage, XTEST input. Pure Rust via x11rb.

use std::time::{Duration, Instant};

use grokhub_core::desktop_mcp::{
    union_monitor, CapturedShot, DesktopBackend, KeyCombo, KeyName, MonitorGeom, MouseButton,
    ShotGeom,
};
use x11rb::connection::{Connection, RequestConnection};
use x11rb::protocol::randr::ConnectionExt as RandrExt;
use x11rb::protocol::xproto::{
    ConnectionExt as XprotoExt, ImageFormat, ImageOrder, KEY_PRESS_EVENT, KEY_RELEASE_EVENT,
    BUTTON_PRESS_EVENT, BUTTON_RELEASE_EVENT, MOTION_NOTIFY_EVENT,
};
use x11rb::protocol::xtest::ConnectionExt as XtestExt;
use x11rb::rust_connection::RustConnection;

use super::keys::{keysym_mods, keysym_of};
use super::pixels::{bgrx_to_rgba, crop_rgba};

pub(crate) struct X11Backend {
    conn: RustConnection,
    screen: usize,
    lock_at: Option<(Instant, bool)>,
}

impl X11Backend {
    pub(crate) fn connect() -> Result<Self, String> {
        let (conn, screen) = RustConnection::connect(None).map_err(|e| format!("X11: {e}"))?;
        Ok(Self {
            conn,
            screen,
            lock_at: None,
        })
    }

    fn root(&self) -> u32 {
        self.conn.setup().roots[self.screen].root
    }

    fn root_size(&self) -> (u16, u16) {
        let screen = &self.conn.setup().roots[self.screen];
        (screen.width_in_pixels, screen.height_in_pixels)
    }

    fn monitors(&self) -> Result<Vec<MonitorGeom>, String> {
        let root = self.root();
        let reply = self
            .conn
            .randr_get_monitors(root, true)
            .map_err(|e| format!("RandR: {e}"))?
            .reply()
            .map_err(|e| format!("RandR: {e}"))?;
        let mut out = Vec::new();
        for (i, mon) in reply.monitors.iter().enumerate() {
            let name = self
                .conn
                .get_atom_name(mon.name)
                .ok()
                .and_then(|c| c.reply().ok())
                .map(|r| String::from_utf8_lossy(&r.name).into_owned())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| format!("monitor-{i}"));
            out.push(MonitorGeom {
                id: name.clone(),
                name,
                x: mon.x as i32,
                y: mon.y as i32,
                width: mon.width as u32,
                height: mon.height as u32,
                scale_factor: 1.0,
                primary: mon.primary,
            });
        }
        if !out.is_empty() && !out.iter().any(|m| m.primary) {
            out[0].primary = true;
        }
        Ok(out)
    }

    fn masks_and_bpp(&self, depth: u8) -> (u32, u32, u32, u8, bool) {
        let setup = self.conn.setup();
        let screen = &setup.roots[self.screen];
        let mut red = 0x00ff_0000u32;
        let mut green = 0x0000_ff00u32;
        let mut blue = 0x0000_00ffu32;
        for depth_info in &screen.allowed_depths {
            for vis in &depth_info.visuals {
                if vis.visual_id == screen.root_visual {
                    red = vis.red_mask;
                    green = vis.green_mask;
                    blue = vis.blue_mask;
                }
            }
        }
        let bpp = setup
            .pixmap_formats
            .iter()
            .find(|f| f.depth == depth)
            .map(|f| f.bits_per_pixel)
            .unwrap_or(32);
        let msb = setup.image_byte_order == ImageOrder::MSB_FIRST;
        (red, green, blue, bpp, msb)
    }

    fn capture_root_rect(&self, x: i32, y: i32, w: u32, h: u32) -> Result<(Vec<u8>, u32, u32), String> {
        if w == 0 || h == 0 {
            return Err("empty capture".into());
        }
        let (rw, rh) = self.root_size();
        let (x, y, w, h) = clip_to_root(x, y, w, h, rw as u32, rh as u32)
            .ok_or_else(|| "screenshot is outside the screen".to_string())?;
        let root = self.root();
        let max_bytes = self.conn.maximum_request_bytes().saturating_sub(256);
        let mut rgba = vec![0u8; (w as usize) * (h as usize) * 4];
        let mut y_off = 0u32;
        let mut bpp = 32u8;
        let mut masks = (0x00ff_0000u32, 0x0000_ff00u32, 0x0000_00ffu32, false);
        while y_off < h {
            let bytes_pp = (bpp / 8).max(1) as usize;
            let row_bytes = (w as usize).saturating_mul(bytes_pp).max(1);
            let max_rows = (max_bytes / row_bytes).clamp(1, (h - y_off) as usize) as u32;
            let rows = max_rows.min(h - y_off);
            let reply = self
                .conn
                .get_image(
                    ImageFormat::Z_PIXMAP,
                    root,
                    x as i16,
                    (y + y_off as i32) as i16,
                    w as u16,
                    rows as u16,
                    u32::MAX,
                )
                .map_err(|e| format!("GetImage: {e}"))?
                .reply()
                .map_err(|e| format!("GetImage: {e}"))?;
            if y_off == 0 {
                let (red, green, blue, bits, msb) = self.masks_and_bpp(reply.depth);
                bpp = if bits >= 24 { bits } else { 32 };
                masks = (red, green, blue, msb);
            }
            let chunk = bgrx_to_rgba(
                &reply.data,
                w,
                rows,
                bpp,
                masks.3,
                masks.0,
                masks.1,
                masks.2,
            )?;
            let dest = (y_off as usize) * (w as usize) * 4;
            let end = dest + chunk.len();
            if end <= rgba.len() {
                rgba[dest..end].copy_from_slice(&chunk);
            }
            y_off += rows;
        }
        let cropped = crop_rgba(&rgba, w, h, 0, 0, w, h)?;
        Ok((cropped, w, h))
    }

    fn fake(&self, kind: u8, detail: u8, x: i16, y: i16) -> Result<(), String> {
        self.conn
            .xtest_fake_input(kind, detail, 0, self.root(), x, y, 0)
            .map_err(|e| format!("XTEST: {e}"))?
            .check()
            .map_err(|e| format!("XTEST: {e}"))?;
        self.conn.flush().map_err(|e| format!("X11: {e}"))?;
        Ok(())
    }

    fn move_root(&self, x: i32, y: i32) -> Result<(), String> {
        self.fake(MOTION_NOTIFY_EVENT, 0, clamp_i16(x), clamp_i16(y))
    }

    fn tap_button(&self, button: u8, down: bool) -> Result<(), String> {
        let kind = if down {
            BUTTON_PRESS_EVENT
        } else {
            BUTTON_RELEASE_EVENT
        };
        self.fake(kind, button, 0, 0)
    }

    fn keycode_map(&self) -> Result<(u8, u8, Vec<u32>), String> {
        let setup = self.conn.setup();
        let first = setup.min_keycode;
        let count = setup.max_keycode.saturating_sub(first).saturating_add(1);
        let reply = self
            .conn
            .get_keyboard_mapping(first, count)
            .map_err(|e| format!("keymap: {e}"))?
            .reply()
            .map_err(|e| format!("keymap: {e}"))?;
        Ok((first, reply.keysyms_per_keycode, reply.keysyms))
    }

    fn with_keycode(&self, keysym: u32, press: impl FnOnce(u8) -> Result<(), String>) -> Result<(), String> {
        let (first, per, syms) = self.keycode_map()?;
        let per = per.max(1) as usize;
        match find_keycode(first, per, &syms, keysym) {
            Some((kc, 0)) => return press(kc),
            Some((kc, _)) => {
                // Second level (capitals, shifted symbols): hold Shift around the press.
                if let Some((shift, 0)) = find_keycode(first, per, &syms, SHIFT_L) {
                    self.fake(KEY_PRESS_EVENT, shift, 0, 0)?;
                    let result = press(kc);
                    let up = self.fake(KEY_RELEASE_EVENT, shift, 0, 0);
                    return result.and(up);
                }
            }
            None => {}
        }
        let spare = find_spare(first, per, &syms)
            .ok_or_else(|| "no free keycode to type this character".to_string())?;
        let old = syms_at(&syms, first, per, spare);
        // Every level holds the keysym, so a lone capital is not read as its lowercase.
        let next = vec![keysym; per];
        self.remap(spare, per as u8, &next)?;
        let result = press(spare);
        let _ = self.remap(spare, per as u8, &old);
        result
    }

    fn remap(&self, keycode: u8, per: u8, syms: &[u32]) -> Result<(), String> {
        self.conn
            .change_keyboard_mapping(1, keycode, per, syms)
            .map_err(|e| format!("keymap: {e}"))?
            .check()
            .map_err(|e| format!("keymap: {e}"))?;
        self.conn.flush().map_err(|e| format!("X11: {e}"))?;
        Ok(())
    }

    fn tap_keysym(&self, keysym: u32) -> Result<(), String> {
        self.with_keycode(keysym, |kc| {
            self.fake(KEY_PRESS_EVENT, kc, 0, 0)?;
            self.fake(KEY_RELEASE_EVENT, kc, 0, 0)
        })
    }

    fn locked_now(&self) -> bool {
        let titles = crate::desktop::lock_titles();
        let refs: Vec<&str> = titles.iter().map(String::as_str).collect();
        grokhub_core::hands_blocked_by_lock(&grokhub_core::ComputerOp::Move { x: 0, y: 0 }, &refs)
    }
}

impl DesktopBackend for X11Backend {
    fn list_monitors(&mut self) -> Result<Vec<MonitorGeom>, String> {
        let mut mons = self.monitors()?;
        if mons.is_empty() {
            let (w, h) = self.root_size();
            mons.push(MonitorGeom {
                id: "all".into(),
                name: "all".into(),
                x: 0,
                y: 0,
                width: w as u32,
                height: h as u32,
                scale_factor: 1.0,
                primary: true,
            });
        }
        Ok(mons)
    }

    fn screenshot(&mut self, monitor: &str) -> Result<CapturedShot, String> {
        let mons = self.list_monitors()?;
        if monitor == "all" {
            let union = union_monitor(&mons).ok_or_else(|| "No monitors.".to_string())?;
            // RandR monitor positions are already root-window coordinates.
            let (rgba, cw, ch) =
                self.capture_root_rect(union.x, union.y, union.physical_w, union.physical_h)?;
            return finish_shot(
                "all",
                union.x,
                union.y,
                cw,
                ch,
                union.scale_factor,
                &rgba,
            );
        }
        let mon = mons
            .iter()
            .find(|m| m.id == monitor || m.name == monitor)
            .cloned()
            .ok_or_else(|| format!("No monitor \"{monitor}\"."))?;
        let (rgba, cw, ch) = self.capture_root_rect(mon.x, mon.y, mon.width, mon.height)?;
        finish_shot(&mon.id, mon.x, mon.y, cw, ch, mon.scale_factor, &rgba)
    }

    fn move_abs(&mut self, x: i32, y: i32) -> Result<(), String> {
        self.move_root(x, y)
    }

    fn button(&mut self, button: MouseButton, down: bool) -> Result<(), String> {
        let code = match button {
            MouseButton::Left => 1,
            MouseButton::Middle => 2,
            MouseButton::Right => 3,
        };
        self.tap_button(code, down)
    }

    fn scroll(&mut self, dx: i32, dy: i32) -> Result<(), String> {
        wheel(self, dy, 4, 5)?;
        wheel(self, dx, 7, 6)?;
        Ok(())
    }

    fn type_text(&mut self, text: &str) -> Result<(), String> {
        let mut chars = text.chars().peekable();
        while let Some(ch) = chars.next() {
            let key = match ch {
                // CRLF is one line break, not two Enters.
                '\r' if chars.peek() == Some(&'\n') => continue,
                '\n' | '\r' => KeyName::Return,
                '\t' => KeyName::Tab,
                c => KeyName::Char(c),
            };
            self.tap_keysym(keysym_of(&key))?;
        }
        Ok(())
    }

    fn key_combo(&mut self, combo: &KeyCombo) -> Result<(), String> {
        let mods = keysym_mods(combo);
        let mut held = 0;
        let mut result = Ok(());
        for sym in &mods {
            result = self.tap_keysym_down(*sym, true);
            if result.is_err() {
                break;
            }
            held += 1;
        }
        if result.is_ok() {
            let keysym = keysym_of(&combo.key);
            result = self.with_keycode(keysym, |kc| {
                self.fake(KEY_PRESS_EVENT, kc, 0, 0)?;
                self.fake(KEY_RELEASE_EVENT, kc, 0, 0)
            });
        }
        // Release whatever went down, even on an error, so no modifier stays stuck.
        for sym in mods[..held].iter().rev() {
            let up = self.tap_keysym_down(*sym, false);
            result = result.and(up);
        }
        result
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

impl X11Backend {
    fn tap_keysym_down(&self, keysym: u32, down: bool) -> Result<(), String> {
        self.with_keycode(keysym, |kc| {
            let kind = if down { KEY_PRESS_EVENT } else { KEY_RELEASE_EVENT };
            self.fake(kind, kc, 0, 0)
        })
    }
}

fn wheel(backend: &X11Backend, delta: i32, positive: u8, negative: u8) -> Result<(), String> {
    let n = delta.clamp(-30, 30);
    let button = if n > 0 { positive } else { negative };
    for _ in 0..n.unsigned_abs() {
        backend.tap_button(button, true)?;
        backend.tap_button(button, false)?;
    }
    Ok(())
}

fn finish_shot(
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

fn clip_to_root(x: i32, y: i32, w: u32, h: u32, rw: u32, rh: u32) -> Option<(i32, i32, u32, u32)> {
    let x1 = x.max(0);
    let y1 = y.max(0);
    let x2 = x.saturating_add(w as i32).min(rw as i32).max(0);
    let y2 = y.saturating_add(h as i32).min(rh as i32).max(0);
    if x2 <= x1 || y2 <= y1 {
        return None;
    }
    Some((x1, y1, (x2 - x1) as u32, (y2 - y1) as u32))
}

fn clamp_i16(v: i32) -> i16 {
    v.clamp(i16::MIN as i32, i16::MAX as i32) as i16
}

const SHIFT_L: u32 = 0xffe1;

/// Keycode and level (0 plain, 1 shifted) of the first key that has `keysym`.
/// Higher levels need modifiers this backend does not press, so they are skipped.
fn find_keycode(first: u8, per: usize, syms: &[u32], keysym: u32) -> Option<(u8, usize)> {
    for level in 0..per.min(2) {
        for (i, chunk) in syms.chunks(per).enumerate() {
            if chunk.get(level) == Some(&keysym) {
                return Some((first.saturating_add(i as u8), level));
            }
        }
    }
    None
}

fn find_spare(first: u8, per: usize, syms: &[u32]) -> Option<u8> {
    for (i, chunk) in syms.chunks(per).enumerate() {
        if chunk.iter().all(|k| *k == 0) {
            let kc = first.saturating_add(i as u8);
            if kc != 0 {
                return Some(kc);
            }
        }
    }
    None
}

fn syms_at(syms: &[u32], first: u8, per: usize, keycode: u8) -> Vec<u32> {
    let index = keycode.saturating_sub(first) as usize;
    let start = index.saturating_mul(per);
    let mut out = vec![0u32; per];
    if start < syms.len() {
        let end = (start + per).min(syms.len());
        out[..end - start].copy_from_slice(&syms[start..end]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktop_mcp_x11_clip_and_keycode_helpers() {
        assert_eq!(clip_to_root(-10, -4, 20, 8, 100, 50), Some((0, 0, 10, 4)));
        assert!(clip_to_root(200, 0, 10, 10, 100, 50).is_none());
        assert_eq!(clamp_i16(40_000), i16::MAX);
        let syms = vec![0u32, 0, 0x61, 0, 0, 0];
        assert_eq!(find_keycode(8, 2, &syms, 0x61), Some((9, 0)));
        // 'a'/'A' and '1'/'!' keys: the shifted keysym reports level 1.
        let us = vec![0x61u32, 0x41, 0x31, 0x21];
        assert_eq!(find_keycode(8, 2, &us, 0x41), Some((8, 1)));
        assert_eq!(find_keycode(8, 2, &us, 0x21), Some((9, 1)));
        assert_eq!(find_keycode(8, 2, &us, 0x31), Some((9, 0)));
        assert_eq!(find_spare(8, 2, &syms), Some(8));
        assert_eq!(syms_at(&syms, 8, 2, 9), vec![0x61, 0]);
        let cropped = crop_rgba(&[1, 2, 3, 255, 4, 5, 6, 255], 2, 1, 1, 0, 1, 1).unwrap();
        assert_eq!(cropped, vec![4, 5, 6, 255]);
    }
}
