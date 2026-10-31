//! Command Centre card 60: LAN 3D printer alerts in the cabin.
//!
//! Discovers Moonraker/OctoPrint on the LAN (opt-in), polls status read-only,
//! posts Pulse cards + OS notify on finish/fail/pause/filament, and parks
//! pause/cancel/resume behind the hard approval card. Hosts and optional
//! API keys live only under the user's config dir — never in the repo.

use std::io::Read;
use std::net::Ipv4Addr;
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;

use grokhub_agent::HardClass;
use grokhub_core::print_alerts::{
    self as pa, alert_card, control_action_json, control_request, identify_api,
    parse_control_action, parse_moonraker_status, parse_octoprint_job, ControlKind,
    PrintAlertsLedger, PrinterKind, PrinterStatus, SavedPrinter, CONTROL_TOOL, POLL_EVERY_MS,
};

use super::*;
use crate::config;

fn ledger_path() -> PathBuf {
    config::config_dir().join(pa::LEDGER_FILE)
}

fn load_ledger() -> PrintAlertsLedger {
    std::fs::read_to_string(ledger_path())
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

fn save_ledger(ledger: &PrintAlertsLedger) {
    if let Ok(text) = serde_json::to_string_pretty(ledger) {
        let path = ledger_path();
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(path, text);
    }
}

/// No redirects: `egress_ok` checks only the printer URL, so a redirect
/// could send the request (and its API key) off the LAN.
fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_millis(pa::DISCOVER_TIMEOUT_MS))
        .timeout_read(Duration::from_millis(2_000))
        .redirects(0)
        .build()
}

fn base_url(endpoint: &str) -> String {
    let e = endpoint.trim().trim_end_matches('/');
    if e.starts_with("http://") || e.starts_with("https://") {
        e.to_string()
    } else {
        format!("http://{e}")
    }
}

fn printer_key(endpoint: &str) -> String {
    endpoint
        .trim()
        .trim_start_matches("http://")
        .trim_start_matches("https://")
        .trim_end_matches('/')
        .to_ascii_lowercase()
}

/// Save the printer's webcam snapshot where Pulse looks for card images.
/// Only a 2xx body that decodes as an image is kept.
fn cache_snapshot(agent: &ureq::Agent, url: &str) -> bool {
    let path = super::pulse_ui::pulse_image_path(url);
    if crate::xai::egress_ok(url, &[]).is_err() {
        return false;
    }
    let Ok(resp) = agent.get(url).call() else {
        return false;
    };
    if !(200..300).contains(&resp.status()) {
        return false;
    }
    let mut buf = Vec::new();
    if resp
        .into_reader()
        .take(4 * 1024 * 1024)
        .read_to_end(&mut buf)
        .is_err()
        || image::load_from_memory(&buf).is_err()
    {
        return false;
    }
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    config::atomic_write(&path, &buf).is_ok()
}

fn get_json(agent: &ureq::Agent, url: &str, api_key: Option<&str>) -> Option<String> {
    crate::xai::egress_ok(url, &[]).ok()?;
    let mut req = agent.get(url);
    if let Some(key) = api_key.map(str::trim).filter(|k| !k.is_empty()) {
        req = req.set("X-Api-Key", key);
    }
    let resp = req.call().ok()?;
    if !(200..300).contains(&resp.status()) {
        return None;
    }
    let mut body = String::new();
    resp.into_reader()
        .take(512 * 1024)
        .read_to_string(&mut body)
        .ok()?;
    Some(body)
}

fn post_control(
    agent: &ureq::Agent,
    endpoint: &str,
    kind: PrinterKind,
    control: ControlKind,
    api_key: Option<&str>,
) -> Result<(), String> {
    let (method, path, body) = control_request(kind, control);
    if method != "POST" {
        return Err("unsupported control method".into());
    }
    let url = format!("{}{path}", base_url(endpoint));
    crate::xai::egress_ok(&url, &[])?;
    let mut req = agent.post(&url);
    if let Some(key) = api_key.map(str::trim).filter(|k| !k.is_empty()) {
        req = req.set("X-Api-Key", key);
    }
    let resp = if body.is_empty() {
        req.call()
    } else {
        req.set("Content-Type", "application/json")
            .send_string(body)
    };
    match resp {
        Ok(_) => Ok(()),
        Err(ureq::Error::Status(code, _)) => Err(format!("printer returned HTTP {code}")),
        Err(e) => Err(format!("printer control failed: {e}")),
    }
}

fn fetch_status(agent: &ureq::Agent, saved: &SavedPrinter) -> Option<PrinterStatus> {
    let base = base_url(&saved.endpoint);
    let key = saved.api_key.as_deref();
    match saved.kind {
        PrinterKind::Moonraker => {
            let url =
                format!("{base}/printer/objects/query?print_stats&display_status&virtual_sdcard");
            let body = get_json(agent, &url, key)?;
            parse_moonraker_status(&body)
        }
        PrinterKind::OctoPrint => {
            let url = format!("{base}/api/job");
            let body = get_json(agent, &url, key)?;
            let mut status = parse_octoprint_job(&body)?;
            if !saved.name.trim().is_empty() {
                status.name = saved.name.clone();
            }
            Some(status)
        }
    }
}

fn probe_endpoint(agent: &ureq::Agent, host: &str, port: u16) -> Option<SavedPrinter> {
    let endpoint = format!("{host}:{port}");
    let base = base_url(&endpoint);
    // Moonraker first on typical ports.
    for (path, expect) in [
        ("/printer/info", Some(PrinterKind::Moonraker)),
        ("/server/info", Some(PrinterKind::Moonraker)),
        ("/api/version", Some(PrinterKind::OctoPrint)),
        ("/api/job", Some(PrinterKind::OctoPrint)),
    ] {
        let url = format!("{base}{path}");
        if let Some(body) = get_json(agent, &url, None) {
            let kind = identify_api(path, &body).or(expect)?;
            let name = match kind {
                PrinterKind::Moonraker => serde_json::from_str::<serde_json::Value>(&body)
                    .ok()
                    .and_then(|v| {
                        v.pointer("/result/hostname")
                            .or_else(|| v.pointer("/result/printer_info/hostname"))
                            .and_then(|h| h.as_str())
                            .map(str::to_string)
                    })
                    .unwrap_or_else(|| format!("Moonraker @ {endpoint}")),
                PrinterKind::OctoPrint => format!("OctoPrint @ {endpoint}"),
            };
            return Some(SavedPrinter {
                endpoint,
                kind,
                name,
                enabled: true,
                api_key: None,
            });
        }
    }
    None
}

/// Local IPv4 addresses (private) for discovery — never logged with credentials.
fn local_private_v4() -> Vec<Ipv4Addr> {
    let mut out = Vec::new();
    // hostname -I is common on Linux; ignore failures on Windows.
    if let Some(text) = super::setup_wizard::run("hostname", &["-I"]) {
        for tok in text.split_whitespace() {
            if let Ok(ip) = tok.parse::<Ipv4Addr>() {
                if ip.is_private() && !ip.is_loopback() {
                    out.push(ip);
                }
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

/// Endpoints to probe: localhost + a capped slice of each local /24.
pub(super) fn discover_targets(locals: &[Ipv4Addr]) -> Vec<(String, u16)> {
    let mut out = Vec::new();
    for port in pa::discover_ports() {
        out.push(("127.0.0.1".into(), *port));
    }
    for ip in locals {
        let o = ip.octets();
        // Cap: gateway-ish .1, .50, .100, and the host itself — avoid a full /24 storm.
        let hosts = [1u8, 50, 100, o[3]];
        for h in hosts {
            let host = format!("{}.{}.{}.{}", o[0], o[1], o[2], h);
            for port in [7125u16, 5000, 4408] {
                out.push((host.clone(), port));
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

#[derive(Default)]
pub(super) struct PrintAlertsState {
    last_poll_ms: u64,
    add_host: String,
    discovering: bool,
    discover_rx: Option<mpsc::Receiver<Vec<SavedPrinter>>>,
}

impl Cabin {
    pub(super) fn set_print_alerts_enabled(&mut self, on: bool) {
        if self.cfg.print_alerts.enabled == on {
            return;
        }
        self.cfg.print_alerts.enabled = on;
        self.persist_cfg();
        self.status = "Saved".into();
    }

    pub(super) fn set_print_alerts_camera(&mut self, on: bool) {
        if self.cfg.print_alerts.camera == on {
            return;
        }
        self.cfg.print_alerts.camera = on;
        self.persist_cfg();
        self.status = "Saved".into();
    }

    pub(super) fn ui_print_alerts(&mut self, ui: &mut egui::Ui) {
        crate::cards::section_label(ui, "3D printer alerts");
        crate::cards::help_text(
            ui,
            "Opt-in. Discover Moonraker/Klipper or OctoPrint on your LAN and get a Pulse card plus notification when a print finishes, fails, pauses, or runs out of filament. Pause/cancel always need the approval card. Hosts stay on this PC only.",
        );
        ui.add_space(8.0);
        let mut on = self.cfg.print_alerts.enabled;
        if crate::cards::settings_toggle(
            ui,
            "Watch LAN printers",
            "Poll saved printers for status (read-only). Off by default.",
            &mut on,
        ) {
            self.set_print_alerts_enabled(on);
        }
        let mut cam = self.cfg.print_alerts.camera;
        if crate::cards::settings_toggle(
            ui,
            "Include camera snapshot",
            "When the printer exposes a webcam and an alert fires, attach a snapshot. Off by default.",
            &mut cam,
        ) {
            self.set_print_alerts_camera(cam);
        }
        if !self.cfg.print_alerts.enabled {
            return;
        }
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new("Add host:port")
                    .size(crate::theme::FONT_BODY)
                    .color(crate::theme::fg()),
            );
            let resp = ui.add(
                egui::TextEdit::singleline(&mut self.print_alerts.add_host)
                    .desired_width(180.0)
                    .hint_text("192.168.1.50:7125"),
            );
            if ui.button("Add").clicked()
                || (resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)))
            {
                self.add_print_printer();
            }
            let discover_label = if self.print_alerts.discovering {
                "Scanning…"
            } else {
                "Discover"
            };
            if ui
                .add_enabled(
                    !self.print_alerts.discovering,
                    egui::Button::new(discover_label),
                )
                .clicked()
            {
                self.start_print_discover();
            }
        });
        ui.add_space(6.0);
        let mut ledger = load_ledger();
        let keys: Vec<String> = ledger.printers.keys().cloned().collect();
        if keys.is_empty() {
            crate::cards::help_text(
                ui,
                "No printers saved yet. Add a host:port or run Discover on your LAN.",
            );
        }
        let mut dirty = false;
        let mut remove: Option<String> = None;
        for key in keys {
            let Some(p) = ledger.printers.get_mut(&key) else {
                continue;
            };
            ui.horizontal(|ui| {
                let mut en = p.enabled;
                if ui.checkbox(&mut en, "").changed() {
                    p.enabled = en;
                    dirty = true;
                }
                ui.label(
                    egui::RichText::new(format!("{} · {}", p.name, p.kind.label()))
                        .size(crate::theme::FONT_BODY)
                        .color(crate::theme::fg()),
                );
                if ui.small_button("Remove").clicked() {
                    remove = Some(key.clone());
                }
            });
            // Never paint the raw endpoint in a copyable way that leaves the machine —
            // show a short host-only hint.
            let host_hint = p
                .endpoint
                .split(':')
                .next()
                .unwrap_or("printer")
                .to_string();
            ui.label(
                egui::RichText::new(format!("  {host_hint}"))
                    .size(crate::theme::FONT_META)
                    .color(crate::theme::MUTED),
            );
        }
        if let Some(k) = remove {
            ledger.printers.remove(&k);
            dirty = true;
        }
        if dirty {
            save_ledger(&ledger);
            self.status = "Saved".into();
        }
    }

    fn add_print_printer(&mut self) {
        let raw = self.print_alerts.add_host.trim().to_string();
        if raw.is_empty() {
            return;
        }
        let endpoint = raw
            .trim_start_matches("http://")
            .trim_start_matches("https://")
            .trim_end_matches('/')
            .to_string();
        let agent = agent();
        let (host, port) = match endpoint.rsplit_once(':') {
            Some((h, p)) => (h.to_string(), p.parse::<u16>().unwrap_or(7125)),
            None => (endpoint.clone(), 7125),
        };
        let probed = probe_endpoint(&agent, &host, port);
        let reachable = probed.is_some();
        let found = probed.unwrap_or_else(|| SavedPrinter {
            endpoint: format!("{host}:{port}"),
            kind: if port == 5000 || port == 80 {
                PrinterKind::OctoPrint
            } else {
                PrinterKind::Moonraker
            },
            name: format!("Printer @ {host}"),
            enabled: true,
            api_key: None,
        });
        let mut ledger = load_ledger();
        let key = printer_key(&found.endpoint);
        ledger.printers.insert(key, found);
        save_ledger(&ledger);
        self.print_alerts.add_host.clear();
        self.status = if reachable {
            "Printer saved".into()
        } else {
            "Printer saved — polling when reachable.".into()
        };
    }

    fn start_print_discover(&mut self) {
        if self.print_alerts.discovering {
            return;
        }
        self.print_alerts.discovering = true;
        let (tx, rx) = mpsc::channel();
        self.print_alerts.discover_rx = Some(rx);
        let locals = local_private_v4();
        let targets = discover_targets(&locals);
        std::thread::Builder::new()
            .name("print-discover".into())
            .spawn(move || {
                let agent = agent();
                let mut found = Vec::new();
                let mut seen = std::collections::BTreeSet::new();
                for (host, port) in targets {
                    if let Some(p) = probe_endpoint(&agent, &host, port) {
                        let key = printer_key(&p.endpoint);
                        if seen.insert(key) {
                            found.push(p);
                        }
                    }
                }
                let _ = tx.send(found);
            })
            .ok();
    }

    fn drain_print_discover(&mut self) {
        let Some(rx) = &self.print_alerts.discover_rx else {
            return;
        };
        match rx.try_recv() {
            Ok(found) => {
                self.print_alerts.discovering = false;
                self.print_alerts.discover_rx = None;
                if found.is_empty() {
                    self.status = "No printers found on this LAN yet. Add a host:port.".into();
                    return;
                }
                let mut ledger = load_ledger();
                let mut n = 0;
                for p in found {
                    let key = printer_key(&p.endpoint);
                    if ledger.printers.contains_key(&key) {
                        continue;
                    }
                    ledger.printers.insert(key, p);
                    n += 1;
                }
                save_ledger(&ledger);
                self.status = if n == 0 {
                    "Printers already saved.".into()
                } else {
                    format!("Found {n} printer(s).")
                };
            }
            Err(mpsc::TryRecvError::Empty) => {}
            Err(mpsc::TryRecvError::Disconnected) => {
                self.print_alerts.discovering = false;
                self.print_alerts.discover_rx = None;
            }
        }
    }

    /// Heartbeat / feed tick: poll enabled printers and post alerts.
    pub(super) fn tick_print_alerts(&mut self) {
        self.drain_print_discover();
        if !self.cfg.print_alerts.enabled {
            return;
        }
        let now = now_ms();
        if now.saturating_sub(self.print_alerts.last_poll_ms) < POLL_EVERY_MS {
            return;
        }
        self.print_alerts.last_poll_ms = now;
        let mut ledger = load_ledger();
        ledger.last_poll_ms = now;
        let agent = agent();
        let settings = self.cfg.print_alerts.clone();
        let printers: Vec<(String, SavedPrinter)> = ledger
            .printers
            .iter()
            .filter(|(_, p)| p.enabled)
            .map(|(k, p)| (k.clone(), p.clone()))
            .collect();
        let mut cards = Vec::new();
        for (key, saved) in printers {
            let Some(mut status) = fetch_status(&agent, &saved) else {
                continue;
            };
            if !saved.name.trim().is_empty() {
                status.name = saved.name.clone();
            }
            if let Some(alert) = pa::observe(&settings, &mut ledger, &key, &status, now) {
                let mut card = alert_card(&alert, now);
                if alert.camera_enabled {
                    let base = base_url(&saved.endpoint);
                    let query = format!("{base}{}", pa::webcam_query_path(saved.kind));
                    status.camera_path = get_json(&agent, &query, saved.api_key.as_deref())
                        .and_then(|body| pa::parse_webcam_path(saved.kind, &body));
                    if let Some(path) = status.camera_path.as_deref() {
                        let snap = format!("{base}{path}");
                        // Pulse only downloads public images, so the poll caches
                        // the LAN snapshot itself; cabin never embeds credentials.
                        if cache_snapshot(&agent, &snap) {
                            card.pulse.image_urls = vec![snap];
                        }
                    }
                }
                cards.push(card);
            }
        }
        save_ledger(&ledger);
        for card in cards {
            let title = card.title.clone();
            let body = card.body.clone().unwrap_or_default();
            self.post_feed_card(card);
            if !self.quiet_now() {
                crate::notify::ping(grokhub_core::brand::product_name(), &title);
                if !body.is_empty() {
                    self.status = format!("{title} · {body}");
                }
            }
        }
    }

    /// Run on a print-alert card: park pause/cancel/resume on the hard card.
    pub(super) fn print_alert_run(&mut self, id: &str) -> bool {
        let Some(card) = self.pulse_card(id) else {
            return false;
        };
        let Some(meta) = card.pulse.print_alert.clone() else {
            return false;
        };
        let control = meta
            .controls
            .first()
            .and_then(|k| match k.as_str() {
                "pause" => Some(ControlKind::Pause),
                "cancel" => Some(ControlKind::Cancel),
                "resume" => Some(ControlKind::Resume),
                _ => None,
            })
            .or_else(|| {
                card.prompt.as_deref().and_then(|p| {
                    let l = p.to_ascii_lowercase();
                    if l.contains("cancel") {
                        Some(ControlKind::Cancel)
                    } else if l.contains("resume") {
                        Some(ControlKind::Resume)
                    } else if l.contains("pause") {
                        Some(ControlKind::Pause)
                    } else {
                        None
                    }
                })
            });
        let Some(control) = control else {
            return false;
        };
        assert!(control.requires_approval());
        let action = control_action_json(&meta.printer_key, control);
        self.park_hard(
            harness_ui::ParkSource::Proactive(card.id.clone()),
            HardClass::IrreversibleOs,
            "print",
            CONTROL_TOOL.into(),
            action,
        );
        self.nav = Nav::Chat;
        self.status = format!(
            "{} for {} — waiting on the approval card.",
            control.label(),
            meta.printer_name
        );
        true
    }

    /// After Approve on a `printer_control` hard card, send the HTTP command.
    pub(super) fn execute_print_control(&mut self, action: &str) {
        let Some((key, control)) = parse_control_action(action) else {
            self.status = "Printer control was missing details.".into();
            return;
        };
        if !control.requires_approval() {
            self.status = "Printer control refused without approval.".into();
            return;
        }
        let ledger = load_ledger();
        let Some(saved) = ledger.printers.get(&key) else {
            self.status = "That printer is no longer saved.".into();
            return;
        };
        let agent = agent();
        match post_control(
            &agent,
            &saved.endpoint,
            saved.kind,
            control,
            saved.api_key.as_deref(),
        ) {
            Ok(()) => {
                self.status = format!("{} sent.", control.label());
            }
            Err(e) => {
                self.status = e;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use grokhub_core::print_alerts::{
        alert_card, observe, parse_moonraker_status, AlertKind, PrintAlertsSettings, PrintState,
    };

    #[test]
    fn printer_requests_never_follow_a_redirect() {
        use std::io::Write;
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let served = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buf = [0u8; 1024];
            let _ = stream.read(&mut buf);
            let reply = format!(
                "HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:{port}/landed\r\n\
                 Content-Length: 2\r\nConnection: close\r\n\r\n{{}}"
            );
            let _ = stream.write_all(reply.as_bytes());
            listener
        });
        let url = format!("http://127.0.0.1:{port}/api/job");
        let got = get_json(&agent(), &url, Some("sk-abcdefghijklmnopqrstuv"));
        assert_eq!(got, None, "a 302 is not a printer reply");
        let listener = served.join().unwrap();
        listener.set_nonblocking(true).unwrap();
        assert_eq!(
            listener.accept().map(|_| ()).map_err(|e| e.kind()),
            Err(std::io::ErrorKind::WouldBlock),
            "the redirect was followed"
        );
    }

    /// Answer each of `replies` once, in order, on a loopback port.
    fn serve(replies: Vec<Vec<u8>>) -> (u16, std::thread::JoinHandle<()>) {
        use std::io::Write;
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let served = std::thread::spawn(move || {
            for reply in replies {
                let (mut stream, _) = listener.accept().unwrap();
                let mut buf = [0u8; 1024];
                let _ = stream.read(&mut buf);
                let _ = stream.write_all(&reply);
            }
        });
        (port, served)
    }

    fn http_ok(body: &[u8]) -> Vec<u8> {
        let mut r = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        )
        .into_bytes();
        r.extend_from_slice(body);
        r
    }

    #[test]
    fn lan_snapshot_lands_in_the_pulse_image_cache() {
        let _g = crate::config::hold_test_config();
        let root = crate::config::test_config_root("printer-snapshot");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        std::env::set_var("GROKHUB_CONFIG", &root);
        let mut png = Vec::new();
        image::RgbaImage::from_pixel(4, 2, image::Rgba([10, 20, 30, 255]))
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        let (port, served) = serve(vec![http_ok(&png), http_ok(b"not an image")]);
        let snap = format!("http://127.0.0.1:{port}/webcam/?action=snapshot");
        assert!(cache_snapshot(&agent(), &snap));
        let cached = super::super::pulse_ui::pulse_image_path(&snap);
        assert!(cached.starts_with(&root));
        assert_eq!(std::fs::read(&cached).unwrap(), png);
        let junk = format!("http://127.0.0.1:{port}/webcam/junk");
        assert!(!cache_snapshot(&agent(), &junk));
        assert!(!super::super::pulse_ui::pulse_image_path(&junk).exists());
        served.join().unwrap();
        std::env::remove_var("GROKHUB_CONFIG");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn discover_targets_include_loopback_and_cap_lan() {
        let locals = [Ipv4Addr::new(192, 168, 1, 42)];
        let t = discover_targets(&locals);
        assert!(t.iter().any(|(h, p)| h == "127.0.0.1" && *p == 7125));
        assert!(t.iter().any(|(h, _)| h == "192.168.1.42"));
        assert!(t.len() < 40, "discovery must stay capped: {}", t.len());
    }

    #[test]
    fn controls_require_approval_before_http() {
        for c in [ControlKind::Pause, ControlKind::Cancel, ControlKind::Resume] {
            assert!(c.requires_approval());
        }
        let (method, path, _) = control_request(PrinterKind::Moonraker, ControlKind::Pause);
        assert_eq!(method, "POST");
        assert!(path.contains("pause"));
    }

    #[test]
    fn mock_moonraker_transition_posts_finished_card() {
        let settings = PrintAlertsSettings {
            enabled: true,
            camera: false,
        };
        let mut ledger = PrintAlertsLedger::default();
        let printing = parse_moonraker_status(
            r#"{"result":{"status":{"print_stats":{"state":"printing","filename":"a.gcode","message":""},"display_status":{"progress":0.5}}}}"#,
        )
        .unwrap();
        assert_eq!(printing.state, PrintState::Printing);
        assert!(observe(&settings, &mut ledger, "p1", &printing, 1).is_none());
        let done = parse_moonraker_status(
            r#"{"result":{"status":{"print_stats":{"state":"complete","filename":"a.gcode","message":""},"display_status":{"progress":1.0}}}}"#,
        )
        .unwrap();
        let alert = observe(&settings, &mut ledger, "p1", &done, 2).unwrap();
        assert_eq!(alert.kind, AlertKind::Finished);
        let card = alert_card(&alert, 2);
        assert!(card.pulse.print_alert.is_some());
        assert!(card.body.as_deref().unwrap().contains("approval"));
    }
}
