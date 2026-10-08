//! Capture after KWin ScreenShot2: spectacle, then the Screenshot portal.
//! Route tests use fakes and never launch spectacle or a portal.

use std::process::{Command, Stdio};
use std::time::Duration;

use grokhub_core::desktop_mcp::{
    capture_route_label, capture_route_order, crop_rgba, monitor_crop_rect, monitor_index,
    spectacle_argv, spectacle_help_supports_screen, union_monitor, CaptureRouteId, CapturedShot,
    MonitorGeom, ShotGeom,
};

pub(crate) trait ShotRoute: Send {
    fn capture(&mut self, monitor: &str, monitors: &[MonitorGeom]) -> Result<CapturedShot, String>;
}

pub(crate) struct ShotChain {
    routes: Vec<(CaptureRouteId, Box<dyn ShotRoute>)>,
    active: Option<CaptureRouteId>,
}

impl ShotChain {
    pub(crate) fn live() -> Self {
        let routes: Vec<(CaptureRouteId, Box<dyn ShotRoute>)> = vec![
            (CaptureRouteId::ScreenShot2, Box::new(KwinRoute)),
            (CaptureRouteId::Spectacle, Box::new(SpectacleRoute)),
            (CaptureRouteId::PortalScreenshot, Box::new(PortalShotRoute)),
        ];
        debug_assert_eq!(
            routes.iter().map(|(id, _)| *id).collect::<Vec<_>>(),
            capture_route_order()
        );
        Self { routes, active: None }
    }

    #[cfg(test)]
    pub(crate) fn from_routes(routes: Vec<(CaptureRouteId, Box<dyn ShotRoute>)>) -> Self {
        Self { routes, active: None }
    }

    #[cfg(test)]
    pub(crate) fn label(&self) -> Option<&'static str> {
        self.active.map(capture_route_label)
    }

    pub(crate) fn capture(&mut self, monitor: &str, monitors: &[MonitorGeom]) -> Result<CapturedShot, String> {
        if let Some(id) = self.active {
            let route = self.routes.iter_mut().find(|(route_id, _)| *route_id == id);
            if let Some((_, route)) = route {
                return route.capture(monitor, monitors);
            }
        }
        let mut failures = Vec::new();
        for index in 0..self.routes.len() {
            let id = self.routes[index].0;
            match self.routes[index].1.capture(monitor, monitors) {
                Ok(shot) => {
                    self.active = Some(id);
                    super::publish_capture_backend(capture_route_label(id));
                    return Ok(shot);
                }
                Err(err) => failures.push(err),
            }
        }
        Err(failures.join(" "))
    }
}

struct KwinRoute;

impl ShotRoute for KwinRoute {
    fn capture(&mut self, monitor: &str, monitors: &[MonitorGeom]) -> Result<CapturedShot, String> {
        if monitor != "all" {
            let mon = monitors
                .iter()
                .find(|item| item.id == monitor || item.name == monitor)
                .cloned()
                .ok_or_else(|| format!("No monitor \"{monitor}\"."))?;
            let (rgba, meta) = super::kwin_shot::capture(Some(&mon.name))?;
            return shot_from(&mon.id, mon.x, mon.y, meta.scale, meta.width, meta.height, &rgba);
        }
        let (rgba, meta) = super::kwin_shot::capture(None)?;
        let origin = union_monitor(monitors);
        let (x, y) = match &origin {
            Some(geom) => (geom.x, geom.y),
            None => (0, 0),
        };
        shot_from("all", x, y, meta.scale, meta.width, meta.height, &rgba)
    }
}

struct SpectacleRoute;

impl ShotRoute for SpectacleRoute {
    fn capture(&mut self, monitor: &str, monitors: &[MonitorGeom]) -> Result<CapturedShot, String> {
        let dest = std::env::temp_dir().join(format!(
            "grokhub-spectacle-{}-{}.png",
            std::process::id(),
            grokhub_core::now_ms()
        ));
        let dest_text = dest.display().to_string();
        let help = command_text("spectacle", &["--help".into()]);
        let screen = if spectacle_help_supports_screen(&help) && monitor != "all" {
            monitor_index(monitors, monitor)
        } else {
            None
        };
        let argv = spectacle_argv(&dest_text, screen);
        let run = run_bin("spectacle", &argv[1..], 8000);
        let loaded = run.and_then(|_| {
            image::open(&dest)
                .map(|img| img.to_rgba8())
                .map_err(|err| format!("spectacle image: {err}"))
        });
        let _ = std::fs::remove_file(&dest);
        let image = loaded?;
        let (width, height) = image.dimensions();
        let rgba = image.into_raw();
        if screen.is_some() {
            let mon = monitors
                .iter()
                .find(|item| item.id == monitor || item.name == monitor)
                .cloned()
                .ok_or_else(|| format!("No monitor \"{monitor}\"."))?;
            return shot_from(&mon.id, mon.x, mon.y, mon.scale_factor, width, height, &rgba);
        }
        let desktop = union_monitor(monitors).ok_or_else(|| "No monitors for spectacle.".to_string())?;
        if monitor == "all" {
            return shot_from(
                "all",
                desktop.x,
                desktop.y,
                desktop.scale_factor,
                width,
                height,
                &rgba,
            );
        }
        let mon = monitors
            .iter()
            .find(|item| item.id == monitor || item.name == monitor)
            .cloned()
            .ok_or_else(|| format!("No monitor \"{monitor}\"."))?;
        let (x, y, w, h) = monitor_crop_rect(&desktop, width, height, &mon)?;
        let cropped = crop_rgba(&rgba, width, height, x, y, w, h)?;
        shot_from(&mon.id, mon.x, mon.y, mon.scale_factor, w, h, &cropped)
    }
}

struct PortalShotRoute;

impl ShotRoute for PortalShotRoute {
    fn capture(&mut self, monitor: &str, monitors: &[MonitorGeom]) -> Result<CapturedShot, String> {
        let uri = portal_screenshot_uri()?;
        let path = file_uri_path(&uri)?;
        let image = image::open(&path)
            .map(|img| img.to_rgba8())
            .map_err(|err| format!("portal screenshot: {err}"))?;
        let _ = std::fs::remove_file(&path);
        let (width, height) = image.dimensions();
        let rgba = image.into_raw();
        let desktop = union_monitor(monitors).unwrap_or(ShotGeom {
            id: "all".into(),
            x: 0,
            y: 0,
            physical_w: width,
            physical_h: height,
            scale_factor: 1.0,
            image_w: width,
            image_h: height,
        });
        if monitor == "all" || monitors.is_empty() {
            return shot_from(
                "all",
                desktop.x,
                desktop.y,
                desktop.scale_factor,
                width,
                height,
                &rgba,
            );
        }
        let mon = monitors
            .iter()
            .find(|item| item.id == monitor || item.name == monitor)
            .cloned()
            .ok_or_else(|| format!("No monitor \"{monitor}\"."))?;
        let (x, y, w, h) = monitor_crop_rect(&desktop, width, height, &mon)?;
        let cropped = crop_rgba(&rgba, width, height, x, y, w, h)?;
        shot_from(&mon.id, mon.x, mon.y, mon.scale_factor, w, h, &cropped)
    }
}

fn portal_screenshot_uri() -> Result<String, String> {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .map_err(|err| format!("portal screenshot: {err}"))?;
    rt.block_on(async {
        let shot = ashpd::desktop::screenshot::Screenshot::request()
            .interactive(false)
            .send()
            .await
            .map_err(|err| format!("portal screenshot: {err}"))?;
        let shot = shot
            .response()
            .map_err(|err| format!("portal screenshot: {err}"))?;
        Ok(shot.uri().to_string())
    })
}

fn file_uri_path(uri: &str) -> Result<std::path::PathBuf, String> {
    let rest = uri
        .strip_prefix("file://")
        .ok_or_else(|| format!("Screenshot URI is not a local file ({uri})."))?;
    let mut out = Vec::with_capacity(rest.len());
    let bytes = rest.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or("");
            if let Ok(value) = u8::from_str_radix(hex, 16) {
                out.push(value);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    // Escapes are UTF-8 bytes, not Latin-1 chars: jos%C3%A9 is josé.
    let out = String::from_utf8(out)
        .map_err(|_| format!("Screenshot URI is not valid UTF-8 ({uri})."))?;
    Ok(std::path::PathBuf::from(out))
}

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

fn command_text(bin: &str, args: &[String]) -> String {
    match run_bin(bin, args, 3000) {
        Ok(out) => String::from_utf8_lossy(&out.stdout).into_owned() + &String::from_utf8_lossy(&out.stderr),
        Err(err) => err,
    }
}

fn run_bin(bin: &str, args: &[String], ms: u64) -> Result<std::process::Output, String> {
    let program = crate::desktop::resolve_bin(bin).unwrap_or_else(|| std::path::PathBuf::from(bin));
    let mut cmd = Command::new(program);
    cmd.args(args).stdout(Stdio::piped()).stderr(Stdio::piped());
    let child = cmd.spawn().map_err(|err| format!("{bin}: {err}"))?;
    let pid = child.id();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(child.wait_with_output());
    });
    match rx.recv_timeout(Duration::from_millis(ms)) {
        Ok(Ok(out)) => {
            if out.status.success() {
                Ok(out)
            } else {
                let err = String::from_utf8_lossy(&out.stderr);
                let line = err.lines().next().unwrap_or("").trim();
                if line.is_empty() {
                    Err(format!("{bin} failed"))
                } else {
                    Err(format!("{bin}: {line}"))
                }
            }
        }
        Ok(Err(err)) => Err(format!("{bin}: {err}")),
        Err(_) => {
            let _ = Command::new("kill").arg(pid.to_string()).status();
            Err(format!("{bin} timed out."))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_uri_path_decodes_utf8_escapes() {
        let path =
            file_uri_path("file:///home/jos%C3%A9/Im%C3%A1genes/Screenshot%20x.png").unwrap();
        assert_eq!(
            path,
            std::path::PathBuf::from("/home/josé/Imágenes/Screenshot x.png")
        );
    }

    struct Script {
        name: &'static str,
        fail: bool,
        log: std::sync::Arc<std::sync::Mutex<Vec<&'static str>>>,
    }

    impl ShotRoute for Script {
        fn capture(&mut self, _monitor: &str, _monitors: &[MonitorGeom]) -> Result<CapturedShot, String> {
            self.log.lock().unwrap_or_else(|err| err.into_inner()).push(self.name);
            if self.fail {
                Err(format!("{} failed", self.name))
            } else {
                Ok(CapturedShot {
                    bytes: vec![1],
                    mime: "image/png".into(),
                    geom: ShotGeom {
                        id: self.name.into(),
                        x: 0,
                        y: 0,
                        physical_w: 2,
                        physical_h: 2,
                        scale_factor: 1.0,
                        image_w: 2,
                        image_h: 2,
                    },
                })
            }
        }
    }

    #[test]
    fn capture_fallback_order_when_each_route_fails() {
        let log = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let mut chain = ShotChain::from_routes(vec![
            (CaptureRouteId::ScreenShot2, Box::new(Script { name: "kwin", fail: true, log: log.clone() })),
            (CaptureRouteId::Spectacle, Box::new(Script { name: "spectacle", fail: true, log: log.clone() })),
            (CaptureRouteId::PortalScreenshot, Box::new(Script { name: "portal", fail: false, log: log.clone() })),
        ]);
        let shot = chain.capture("all", &[]).unwrap();
        assert_eq!(shot.geom.id, "portal");
        assert_eq!(chain.label(), Some("portal Screenshot"));
        let seen = log.lock().unwrap_or_else(|err| err.into_inner()).clone();
        assert_eq!(seen, vec!["kwin", "spectacle", "portal"]);
        chain.capture("all", &[]).unwrap();
        let seen = log.lock().unwrap_or_else(|err| err.into_inner()).clone();
        assert_eq!(seen, vec!["kwin", "spectacle", "portal", "portal"]);
    }
}
