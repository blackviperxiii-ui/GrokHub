//! Wayland input routes, in order: libei, Notify*, absolute uinput, ydotool.
//! The chain is what the fallback tests drive. Fakes never open /dev/uinput.

use std::sync::{Arc, Mutex};

use grokhub_core::desktop_mcp::{
    cast_stream_for_point, imprecise_fallback_note, input_route_is_imprecise, input_route_label,
    input_route_order, mapped_point_to_eis, pick_eis_region, plan_eis_text,
    CastStream, EisRegion, EisTextPlan, InputRouteId, KeyCombo, KeyName, MonitorGeom, MouseButton,
    ShotGeom,
};

use super::keys::{evdev_mods, evdev_of, keysym_mods, keysym_of};
use super::portal::{self, PortalDevice, RestoreStore};
use super::uinput_dev::UinputPointer;

const HALTED: &str = "Desktop control was halted. The portal session is closed.";
const EIS_MISSING: &str = "ConnectToEIS is not available on this portal.";
const NO_STREAM: &str = "The portal session has no ScreenCast stream.";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ShareMode {
    Closed,
    Eis,
    Notify,
}

pub(crate) struct PortalShare {
    device: Box<dyn PortalDevice>,
    store: Box<dyn RestoreStore>,
    tried: bool,
    open: bool,
    mode: ShareMode,
    regions: Vec<EisRegion>,
    streams: Vec<CastStream>,
    text: bool,
    keyboard: bool,
    notice: Option<String>,
    error: Option<String>,
    eis_error: Option<String>,
}

impl PortalShare {
    pub(crate) fn live() -> Self {
        Self::from_parts(
            Box::new(portal::AshpdPortal::new()),
            Box::new(portal::KeychainRestoreStore),
        )
    }

    pub(crate) fn from_parts(device: Box<dyn PortalDevice>, store: Box<dyn RestoreStore>) -> Self {
        Self {
            device,
            store,
            tried: false,
            open: false,
            mode: ShareMode::Closed,
            regions: Vec::new(),
            streams: Vec::new(),
            text: false,
            keyboard: false,
            notice: None,
            error: None,
            eis_error: None,
        }
    }

    fn ensure(&mut self) -> Result<ShareMode, String> {
        if self.tried {
            return if self.open {
                Ok(self.mode)
            } else {
                Err(self.error.clone().unwrap_or_else(|| "The portal session is closed.".into()))
            };
        }
        self.tried = true;
        match portal::open_with_restore(self.device.as_mut(), self.store.as_ref()) {
            Ok(opened) => {
                self.regions = opened.regions;
                self.streams = opened.streams;
                self.text = opened.text;
                self.keyboard = opened.keyboard;
                self.notice = opened.notice;
                if !self.regions.is_empty() && !opened.notify {
                    self.mode = ShareMode::Eis;
                    self.open = true;
                    Ok(ShareMode::Eis)
                } else if opened.notify && !self.streams.is_empty() {
                    self.mode = ShareMode::Notify;
                    self.open = true;
                    self.eis_error = Some(EIS_MISSING.to_string());
                    Ok(ShareMode::Notify)
                } else {
                    self.device.close();
                    self.fail("The portal EIS connection has no pointer region.")
                }
            }
            Err(err) => {
                self.device.close();
                self.fail(err.text())
            }
        }
    }

    fn fail(&mut self, text: &str) -> Result<ShareMode, String> {
        self.open = false;
        self.mode = ShareMode::Closed;
        self.error = Some(text.to_string());
        Err(text.to_string())
    }

    fn close_device(&mut self) {
        self.device.close();
        self.open = false;
        self.mode = ShareMode::Closed;
        self.regions.clear();
        self.streams.clear();
    }

    fn reset(&mut self) {
        self.close_device();
        self.tried = false;
        self.error = None;
        self.eis_error = None;
    }
}

pub(crate) trait CommandRun: Send {
    fn run(&mut self, args: &[String]) -> Result<(), String>;
}

pub(crate) trait DesktopRoute: Send {
    fn open(&mut self) -> Result<(), String>;
    fn move_abs(&mut self, x: i32, y: i32) -> Result<(), String>;
    fn move_abs_on(&mut self, geom: &ShotGeom, x: i32, y: i32) -> Result<(), String> {
        let _ = geom;
        self.move_abs(x, y)
    }
    fn button(&mut self, button: MouseButton, down: bool) -> Result<(), String>;
    fn scroll(&mut self, dx: i32, dy: i32) -> Result<(), String>;
    fn type_text(&mut self, text: &str) -> Result<(), String>;
    fn key_combo(&mut self, combo: &KeyCombo) -> Result<(), String>;
    fn close(&mut self);
    fn notice(&self) -> Option<String> {
        None
    }
}

pub(crate) struct InputChain {
    routes: Vec<(InputRouteId, Box<dyn DesktopRoute>)>,
    active: Option<usize>,
    halted: bool,
    note: Option<String>,
    share: Arc<Mutex<PortalShare>>,
}

impl InputChain {
    pub(crate) fn live(share: Arc<Mutex<PortalShare>>, injector: Box<dyn CommandRun>) -> Self {
        let portal = Arc::clone(&share);
        let routes: Vec<(InputRouteId, Box<dyn DesktopRoute>)> = vec![
            (InputRouteId::Libei, Box::new(LibeiRoute { portal: Arc::clone(&portal) })),
            (InputRouteId::Notify, Box::new(NotifyRoute { portal })),
            (InputRouteId::Uinput, Box::new(LiveUinput::new(super::wayland::host_monitors))),
            (InputRouteId::Ydotool, Box::new(YdotoolRoute { injector })),
        ];
        debug_assert_eq!(
            routes.iter().map(|(id, _)| *id).collect::<Vec<_>>(),
            input_route_order()
        );
        Self {
            routes,
            active: None,
            halted: false,
            note: None,
            share,
        }
    }

    /// Test helper. Real routes with a recording ydotool and a denied uinput.
    #[cfg(test)]
    pub(crate) fn fakes(share: Arc<Mutex<PortalShare>>, injector: Box<dyn CommandRun>) -> Self {
        let portal = Arc::clone(&share);
        Self {
            routes: vec![
                (InputRouteId::Libei, Box::new(LibeiRoute { portal: Arc::clone(&portal) })),
                (InputRouteId::Notify, Box::new(NotifyRoute { portal })),
                (InputRouteId::Uinput, Box::new(DeniedUinput)),
                (InputRouteId::Ydotool, Box::new(YdotoolRoute { injector })),
            ],
            active: None,
            halted: false,
            note: None,
            share,
        }
    }

    /// Test helper. The vec order is the fallback order.
    #[cfg(test)]
    pub(crate) fn from_routes(routes: Vec<(InputRouteId, Box<dyn DesktopRoute>)>) -> Self {
        let share = Arc::new(Mutex::new(PortalShare::from_parts(
            Box::new(IdlePortal),
            Box::new(portal::doubles::MemoryRestoreStore::new()),
        )));
        Self {
            routes,
            active: None,
            halted: false,
            note: None,
            share,
        }
    }

    pub(crate) fn note(&self) -> Option<String> {
        self.note.clone()
    }

    #[cfg(test)]
    pub(crate) fn label(&self) -> Option<&'static str> {
        self.active.map(|index| input_route_label(self.routes[index].0))
    }

    pub(crate) fn regions(&self) -> Vec<EisRegion> {
        lock(&self.share).regions.clone()
    }

    pub(crate) fn release(&mut self) {
        self.close_active();
        self.halted = true;
    }

    pub(crate) fn suspend(&mut self) {
        self.close_active();
        self.halted = false;
        lock(&self.share).reset();
    }

    pub(crate) fn clear_halt(&mut self) {
        self.halted = false;
        let mut share = lock(&self.share);
        share.tried = false;
        share.open = false;
        share.error = None;
    }

    pub(crate) fn move_abs(&mut self, x: i32, y: i32) -> Result<(), String> {
        let index = self.prepare()?;
        self.routes[index].1.move_abs(x, y)
    }

    pub(crate) fn move_abs_on(&mut self, geom: &ShotGeom, x: i32, y: i32) -> Result<(), String> {
        let index = self.prepare()?;
        self.routes[index].1.move_abs_on(geom, x, y)
    }

    pub(crate) fn button(&mut self, button: MouseButton, down: bool) -> Result<(), String> {
        let index = self.prepare()?;
        self.routes[index].1.button(button, down)
    }

    pub(crate) fn scroll(&mut self, dx: i32, dy: i32) -> Result<(), String> {
        let index = self.prepare()?;
        self.routes[index].1.scroll(dx, dy)
    }

    pub(crate) fn type_text(&mut self, text: &str) -> Result<(), String> {
        let index = self.prepare()?;
        self.routes[index].1.type_text(text)
    }

    pub(crate) fn key_combo(&mut self, combo: &KeyCombo) -> Result<(), String> {
        let index = self.prepare()?;
        self.routes[index].1.key_combo(combo)
    }

    fn close_active(&mut self) {
        if let Some(index) = self.active.take() {
            self.routes[index].1.close();
        }
        lock(&self.share).close_device();
    }

    fn prepare(&mut self) -> Result<usize, String> {
        if self.halted {
            return Err(HALTED.into());
        }
        if let Some(index) = self.active {
            return Ok(index);
        }
        let mut failures = Vec::new();
        for index in 0..self.routes.len() {
            let id = self.routes[index].0;
            match self.routes[index].1.open() {
                Ok(()) => {
                    self.active = Some(index);
                    if input_route_is_imprecise(id) {
                        let note = imprecise_fallback_note(&failures);
                        eprintln!("desktop-mcp: {note}");
                        self.note = Some(note);
                    } else if let Some(notice) = self.routes[index].1.notice() {
                        self.note = Some(notice);
                    }
                    super::publish_input_backend(input_route_label(id));
                    return Ok(index);
                }
                Err(err) => failures.push(err),
            }
        }
        Err(failures.pop().unwrap_or_else(|| "No Wayland input route is available.".into()))
    }
}

fn lock(share: &Arc<Mutex<PortalShare>>) -> std::sync::MutexGuard<'_, PortalShare> {
    share.lock().unwrap_or_else(|err| err.into_inner())
}

struct LibeiRoute {
    portal: Arc<Mutex<PortalShare>>,
}

impl DesktopRoute for LibeiRoute {
    fn open(&mut self) -> Result<(), String> {
        let mut portal = lock(&self.portal);
        let mode = portal.ensure()?;
        if mode == ShareMode::Eis {
            Ok(())
        } else {
            Err(portal.eis_error.clone().unwrap_or_else(|| EIS_MISSING.into()))
        }
    }

    fn move_abs(&mut self, x: i32, y: i32) -> Result<(), String> {
        let mut portal = lock(&self.portal);
        if portal.mode != ShareMode::Eis || !portal.open {
            return Err("The portal session is closed.".into());
        }
        let abs = pick_eis_region(&portal.regions, f64::from(x), f64::from(y));
        portal.device.pointer_absolute(abs.x, abs.y)
    }

    fn move_abs_on(&mut self, geom: &ShotGeom, x: i32, y: i32) -> Result<(), String> {
        let mut portal = lock(&self.portal);
        if portal.mode != ShareMode::Eis || !portal.open {
            return Err("The portal session is closed.".into());
        }
        let abs = mapped_point_to_eis(geom, &portal.regions, x, y)
            .ok_or_else(|| "The portal session has no pointer region.".to_string())?;
        portal.device.pointer_absolute(abs.x, abs.y)
    }

    fn button(&mut self, button: MouseButton, down: bool) -> Result<(), String> {
        let code = button_code(button);
        let mut portal = lock(&self.portal);
        portal.device.button(code, down)
    }

    fn scroll(&mut self, dx: i32, dy: i32) -> Result<(), String> {
        lock(&self.portal).device.scroll(dx, dy)
    }

    fn type_text(&mut self, text: &str) -> Result<(), String> {
        let mut portal = lock(&self.portal);
        let plan = plan_eis_text(portal.text, text, char_code)?;
        match plan {
            EisTextPlan::Text => portal.device.text(text),
            EisTextPlan::Keycodes(codes) => {
                if !portal.keyboard {
                    return Err("EIS TEXT is not offered and the device has no keyboard.".into());
                }
                for code in codes {
                    portal.device.key(u32::from(code), true)?;
                    portal.device.key(u32::from(code), false)?;
                }
                Ok(())
            }
        }
    }

    fn key_combo(&mut self, combo: &KeyCombo) -> Result<(), String> {
        let mods = evdev_mods(combo);
        let key = evdev_of(&combo.key).ok_or_else(|| {
            "That key has no evdev code on Wayland. Use type for characters.".to_string()
        })?;
        let mut portal = lock(&self.portal);
        if !portal.keyboard {
            return Err("The EIS device has no keyboard capability.".into());
        }
        for code in &mods {
            portal.device.key(u32::from(*code), true)?;
        }
        portal.device.key(u32::from(key), true)?;
        portal.device.key(u32::from(key), false)?;
        for code in mods.iter().rev() {
            portal.device.key(u32::from(*code), false)?;
        }
        Ok(())
    }

    fn close(&mut self) {
        lock(&self.portal).close_device();
    }

    fn notice(&self) -> Option<String> {
        lock(&self.portal).notice.clone()
    }
}

struct NotifyRoute {
    portal: Arc<Mutex<PortalShare>>,
}

impl DesktopRoute for NotifyRoute {
    fn open(&mut self) -> Result<(), String> {
        let mut portal = lock(&self.portal);
        let mode = portal.ensure()?;
        if mode == ShareMode::Notify && !portal.streams.is_empty() {
            Ok(())
        } else {
            Err(portal
                .error
                .clone()
                .or_else(|| portal.eis_error.clone())
                .unwrap_or_else(|| NO_STREAM.into()))
        }
    }

    fn move_abs(&mut self, x: i32, y: i32) -> Result<(), String> {
        let mut portal = lock(&self.portal);
        let (node, lx, ly) = cast_stream_for_point(&portal.streams, x, y)
            .ok_or_else(|| NO_STREAM.to_string())?;
        portal.device.notify_pointer(node, lx, ly)
    }

    fn move_abs_on(&mut self, geom: &ShotGeom, x: i32, y: i32) -> Result<(), String> {
        let scale = if geom.scale_factor.is_finite() && geom.scale_factor > 0.0 {
            geom.scale_factor
        } else {
            1.0
        };
        let px = x.saturating_sub(geom.x);
        let py = y.saturating_sub(geom.y);
        let gx = (geom.x as f64 + (px as f64) / scale).round() as i32;
        let gy = (geom.y as f64 + (py as f64) / scale).round() as i32;
        self.move_abs(gx, gy)
    }

    fn button(&mut self, button: MouseButton, down: bool) -> Result<(), String> {
        lock(&self.portal)
            .device
            .notify_button(button_code(button) as i32, down)
    }

    fn scroll(&mut self, dx: i32, dy: i32) -> Result<(), String> {
        let mut portal = lock(&self.portal);
        if dx != 0 {
            portal.device.notify_axis(true, dx)?;
        }
        if dy != 0 {
            portal.device.notify_axis(false, dy)?;
        }
        Ok(())
    }

    fn type_text(&mut self, text: &str) -> Result<(), String> {
        let mut portal = lock(&self.portal);
        for ch in text.chars() {
            let sym = keysym_of(&KeyName::Char(ch)) as i32;
            portal.device.notify_keysym(sym, true)?;
            portal.device.notify_keysym(sym, false)?;
        }
        Ok(())
    }

    fn key_combo(&mut self, combo: &KeyCombo) -> Result<(), String> {
        let mods = keysym_mods(combo);
        let key = keysym_of(&combo.key) as i32;
        let mut portal = lock(&self.portal);
        for sym in &mods {
            portal.device.notify_keysym(*sym as i32, true)?;
        }
        portal.device.notify_keysym(key, true)?;
        portal.device.notify_keysym(key, false)?;
        for sym in mods.iter().rev() {
            portal.device.notify_keysym(*sym as i32, false)?;
        }
        Ok(())
    }

    fn close(&mut self) {
        lock(&self.portal).close_device();
    }
}

struct LiveUinput {
    dev: Option<UinputPointer>,
    monitors: fn() -> Result<Vec<MonitorGeom>, String>,
}

impl LiveUinput {
    fn new(monitors: fn() -> Result<Vec<MonitorGeom>, String>) -> Self {
        Self { dev: None, monitors }
    }

    fn dev(&mut self) -> Result<&mut UinputPointer, String> {
        self.dev.as_mut().ok_or_else(|| "The absolute uinput device is closed.".to_string())
    }
}

impl DesktopRoute for LiveUinput {
    fn open(&mut self) -> Result<(), String> {
        let monitors = (self.monitors)()?;
        self.dev = Some(UinputPointer::open(&monitors)?);
        Ok(())
    }

    fn move_abs(&mut self, x: i32, y: i32) -> Result<(), String> {
        self.dev()?.move_abs(x, y)
    }

    fn button(&mut self, button: MouseButton, down: bool) -> Result<(), String> {
        self.dev()?.button(button_code(button) as u16, down)
    }

    fn scroll(&mut self, dx: i32, dy: i32) -> Result<(), String> {
        self.dev()?.scroll(dx, dy)
    }

    fn type_text(&mut self, text: &str) -> Result<(), String> {
        for ch in text.chars() {
            let name = match ch {
                '\n' | '\r' => KeyName::Return,
                '\t' => KeyName::Tab,
                ' ' => KeyName::Space,
                other => KeyName::Char(other),
            };
            let code = evdev_of(&name).ok_or_else(|| {
                format!("That character has no evdev code for absolute uinput ({ch}).")
            })?;
            self.dev()?.key(code, true)?;
            self.dev()?.key(code, false)?;
        }
        Ok(())
    }

    fn key_combo(&mut self, combo: &KeyCombo) -> Result<(), String> {
        let mods = evdev_mods(combo);
        let key = evdev_of(&combo.key).ok_or_else(|| {
            "That key has no evdev code for absolute uinput. Use type for characters.".to_string()
        })?;
        for code in &mods {
            self.dev()?.key(*code, true)?;
        }
        self.dev()?.key(key, true)?;
        self.dev()?.key(key, false)?;
        for code in mods.iter().rev() {
            self.dev()?.key(*code, false)?;
        }
        Ok(())
    }

    fn close(&mut self) {
        self.dev.take();
    }
}

#[cfg(test)]
use grokhub_core::desktop_mcp::uinput_access_denied_message;

#[cfg(test)]
struct DeniedUinput;

#[cfg(test)]
impl DesktopRoute for DeniedUinput {
    fn open(&mut self) -> Result<(), String> {
        Err(uinput_access_denied_message().to_string())
    }
    fn move_abs(&mut self, _x: i32, _y: i32) -> Result<(), String> {
        Err(uinput_access_denied_message().to_string())
    }
    fn button(&mut self, _button: MouseButton, _down: bool) -> Result<(), String> {
        Err(uinput_access_denied_message().to_string())
    }
    fn scroll(&mut self, _dx: i32, _dy: i32) -> Result<(), String> {
        Err(uinput_access_denied_message().to_string())
    }
    fn type_text(&mut self, _text: &str) -> Result<(), String> {
        Err(uinput_access_denied_message().to_string())
    }
    fn key_combo(&mut self, _combo: &KeyCombo) -> Result<(), String> {
        Err(uinput_access_denied_message().to_string())
    }
    fn close(&mut self) {}
}

struct YdotoolRoute {
    injector: Box<dyn CommandRun>,
}

impl DesktopRoute for YdotoolRoute {
    fn open(&mut self) -> Result<(), String> {
        Ok(())
    }

    fn move_abs(&mut self, x: i32, y: i32) -> Result<(), String> {
        self.injector.run(&[
            "mousemove".into(),
            "--absolute".into(),
            x.to_string(),
            y.to_string(),
        ])
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
        self.injector.run(&["click".into(), format!("0x{code:X}")])
    }

    fn scroll(&mut self, dx: i32, dy: i32) -> Result<(), String> {
        if dx == 0 && dy == 0 {
            return Ok(());
        }
        self.injector.run(&[
            "mousemove".into(),
            "--wheel".into(),
            dx.clamp(-30, 30).to_string(),
            dy.clamp(-30, 30).to_string(),
        ])
    }

    fn type_text(&mut self, text: &str) -> Result<(), String> {
        self.injector.run(&["type".into(), "--".into(), text.to_string()])
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
        self.injector.run(&args)
    }

    fn close(&mut self) {}
}

fn button_code(button: MouseButton) -> u32 {
    match button {
        MouseButton::Left => 0x110,
        MouseButton::Right => 0x111,
        MouseButton::Middle => 0x112,
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

/// Portal stand-in for chains built only from scripted routes.
#[cfg(test)]
struct IdlePortal;

#[cfg(test)]
impl portal::RemoteDesktopPortal for IdlePortal {
    fn start(&mut self, _restore_token: Option<String>) -> Result<portal::PortalStart, portal::PortalFail> {
        Err(portal::PortalFail::Denied("idle portal".into()))
    }
    fn close(&mut self) {}
}

#[cfg(test)]
impl portal::EisInput for IdlePortal {
    fn pointer_absolute(&mut self, _x: f32, _y: f32) -> Result<(), String> {
        Ok(())
    }
    fn button(&mut self, _code: u32, _down: bool) -> Result<(), String> {
        Ok(())
    }
    fn scroll(&mut self, _dx: i32, _dy: i32) -> Result<(), String> {
        Ok(())
    }
    fn key(&mut self, _code: u32, _down: bool) -> Result<(), String> {
        Ok(())
    }
    fn text(&mut self, _text: &str) -> Result<(), String> {
        Ok(())
    }
}

#[cfg(test)]
impl portal::NotifyInput for IdlePortal {
    fn notify_pointer(&mut self, _stream: u32, _x: f64, _y: f64) -> Result<(), String> {
        Ok(())
    }
    fn notify_button(&mut self, _button: i32, _down: bool) -> Result<(), String> {
        Ok(())
    }
    fn notify_axis(&mut self, _horizontal: bool, _steps: i32) -> Result<(), String> {
        Ok(())
    }
    fn notify_keysym(&mut self, _keysym: i32, _down: bool) -> Result<(), String> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Script {
        name: &'static str,
        open_err: Option<String>,
        log: Arc<Mutex<Vec<String>>>,
    }

    impl Script {
        fn new(name: &'static str, open_err: Option<&str>, log: Arc<Mutex<Vec<String>>>) -> Self {
            Self {
                name,
                open_err: open_err.map(str::to_string),
                log,
            }
        }
    }

    impl DesktopRoute for Script {
        fn open(&mut self) -> Result<(), String> {
            self.log.lock().unwrap_or_else(|err| err.into_inner()).push(format!("open:{}", self.name));
            match &self.open_err {
                Some(err) => Err(err.clone()),
                None => Ok(()),
            }
        }
        fn move_abs(&mut self, x: i32, y: i32) -> Result<(), String> {
            self.log.lock().unwrap_or_else(|err| err.into_inner()).push(format!("move:{}:{x},{y}", self.name));
            Ok(())
        }
        fn button(&mut self, _button: MouseButton, _down: bool) -> Result<(), String> {
            Ok(())
        }
        fn scroll(&mut self, _dx: i32, _dy: i32) -> Result<(), String> {
            Ok(())
        }
        fn type_text(&mut self, _text: &str) -> Result<(), String> {
            Ok(())
        }
        fn key_combo(&mut self, _combo: &KeyCombo) -> Result<(), String> {
            Ok(())
        }
        fn close(&mut self) {
            self.log.lock().unwrap_or_else(|err| err.into_inner()).push(format!("close:{}", self.name));
        }
    }

    fn script(name: &'static str, err: Option<&str>, log: &Arc<Mutex<Vec<String>>>) -> Box<dyn DesktopRoute> {
        Box::new(Script::new(name, err, Arc::clone(log)))
    }

    fn banned() -> String {
        ["xdo", "tool"].concat()
    }

    #[test]
    fn input_fallback_order_when_each_route_fails() {
        let log = Arc::new(Mutex::new(Vec::new()));
        let mut chain = InputChain::from_routes(vec![
            (InputRouteId::Libei, script("libei", Some("eis down"), &log)),
            (InputRouteId::Notify, script("notify", None, &log)),
            (InputRouteId::Uinput, script("uinput", None, &log)),
            (InputRouteId::Ydotool, script("ydotool", None, &log)),
        ]);
        chain.move_abs(3, 4).unwrap();
        chain.move_abs(5, 6).unwrap();
        let text = log.lock().unwrap_or_else(|err| err.into_inner()).join(" ");
        assert!(text.contains("open:libei"), "{text}");
        assert!(text.contains("open:notify"), "{text}");
        assert!(text.contains("move:notify:3,4"), "{text}");
        assert!(text.contains("move:notify:5,6"), "{text}");
        assert!(!text.contains("open:uinput"), "{text}");
        assert!(!text.contains("open:ydotool"), "{text}");
        assert!(!text.contains(&banned()), "{text}");
        assert_eq!(chain.label(), Some("RemoteDesktop portal (Notify)"));

        let log = Arc::new(Mutex::new(Vec::new()));
        let mut chain = InputChain::from_routes(vec![
            (InputRouteId::Libei, script("libei", Some("eis down"), &log)),
            (InputRouteId::Notify, script("notify", Some("notify down"), &log)),
            (InputRouteId::Uinput, script("uinput", None, &log)),
            (InputRouteId::Ydotool, script("ydotool", None, &log)),
        ]);
        chain.move_abs(1, 1).unwrap();
        let text = log.lock().unwrap_or_else(|err| err.into_inner()).join(" ");
        assert!(text.contains("open:uinput"), "{text}");
        assert!(text.contains("move:uinput:1,1"), "{text}");
        assert!(!text.contains("open:ydotool"), "{text}");
        assert!(!text.contains(&banned()), "{text}");
        assert_eq!(chain.label(), Some("absolute uinput"));

        let log = Arc::new(Mutex::new(Vec::new()));
        let mut chain = InputChain::from_routes(vec![
            (InputRouteId::Libei, script("libei", Some("user cancelled the dialog"), &log)),
            (InputRouteId::Notify, script("notify", Some("no stream"), &log)),
            (InputRouteId::Uinput, script("uinput", Some(uinput_access_denied_message()), &log)),
            (InputRouteId::Ydotool, script("ydotool", None, &log)),
        ]);
        chain.move_abs(9, 8).unwrap();
        let note = chain.note().expect("imprecise note");
        assert!(note.contains("imprecise"), "{note}");
        assert!(note.contains("user cancelled"), "{note}");
        assert!(note.contains("/dev/uinput"), "{note}");
        let text = log.lock().unwrap_or_else(|err| err.into_inner()).join(" ");
        assert!(text.contains("move:ydotool:9,8"), "{text}");
        assert!(!text.contains(&banned()), "{text}");
        assert_eq!(chain.label(), Some("ydotool (imprecise)"));
    }

    #[test]
    fn halt_stops_every_route() {
        let log = Arc::new(Mutex::new(Vec::new()));
        let mut notify = InputChain::from_routes(vec![
            (InputRouteId::Libei, script("libei", Some("eis down"), &log)),
            (InputRouteId::Notify, script("notify", None, &log)),
            (InputRouteId::Uinput, script("uinput", None, &log)),
            (InputRouteId::Ydotool, script("ydotool", None, &log)),
        ]);
        notify.move_abs(1, 2).unwrap();
        notify.release();
        let err = notify.move_abs(3, 4).unwrap_err();
        assert!(err.contains("halted") || err.contains("closed"), "{err}");
        let text = log.lock().unwrap_or_else(|err| err.into_inner()).join(" ");
        assert!(text.contains("close:notify"), "{text}");
        assert!(!text.contains("move:notify:3,4"), "{text}");
        assert!(!text.contains(&banned()), "{text}");

        let log = Arc::new(Mutex::new(Vec::new()));
        let mut uinput = InputChain::from_routes(vec![
            (InputRouteId::Libei, script("libei", Some("eis down"), &log)),
            (InputRouteId::Notify, script("notify", Some("notify down"), &log)),
            (InputRouteId::Uinput, script("uinput", None, &log)),
            (InputRouteId::Ydotool, script("ydotool", None, &log)),
        ]);
        uinput.move_abs(1, 1).unwrap();
        uinput.release();
        let err = uinput.move_abs(2, 2).unwrap_err();
        assert!(err.contains("halted") || err.contains("closed"), "{err}");
        let text = log.lock().unwrap_or_else(|err| err.into_inner()).join(" ");
        assert!(text.contains("close:uinput"), "{text}");
        assert!(!text.contains("move:uinput:2,2"), "{text}");

        let mut fake = BrokerFake::default();
        let halted = grokhub_core::desktop_mcp::apply_desk_request(
            &mut fake,
            grokhub_core::desktop_mcp::CallGate { enabled: true, halted: true },
            &grokhub_core::desktop_mcp::DeskRequest::MoveAbs { id: 1, x: 4, y: 5 },
        );
        assert!(!halted.ok);
        assert!(halted.exit);
        assert_eq!(fake.released, 1);
        assert_eq!(fake.moves, 0);
        assert!(halted.error.unwrap_or_default().contains("halted"));
    }

    #[derive(Default)]
    struct BrokerFake {
        released: u32,
        moves: u32,
    }

    impl grokhub_core::desktop_mcp::DesktopBackend for BrokerFake {
        fn list_monitors(&mut self) -> Result<Vec<MonitorGeom>, String> {
            Ok(Vec::new())
        }
        fn screenshot(&mut self, _: &str) -> Result<grokhub_core::desktop_mcp::CapturedShot, String> {
            Err("no".into())
        }
        fn move_abs(&mut self, _: i32, _: i32) -> Result<(), String> {
            self.moves += 1;
            Ok(())
        }
        fn button(&mut self, _: MouseButton, _: bool) -> Result<(), String> {
            Ok(())
        }
        fn scroll(&mut self, _: i32, _: i32) -> Result<(), String> {
            Ok(())
        }
        fn type_text(&mut self, _: &str) -> Result<(), String> {
            Ok(())
        }
        fn key_combo(&mut self, _: &KeyCombo) -> Result<(), String> {
            Ok(())
        }
        fn is_locked(&mut self) -> bool {
            false
        }
        fn release_input(&mut self) {
            self.released += 1;
        }
    }
}
