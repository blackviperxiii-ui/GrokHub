//! Command Centre card 60: opt-in LAN 3D printer alerts (Moonraker/Klipper
//! and OctoPrint). Pure parsers, transition detect, Pulse card shape, and
//! approval-gated control rules. The cabin discovers, polls, and posts;
//! credentials and host IPs live only in the user's config dir.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::pulse::PulseMeta;
use crate::update_feed::{suggestion_card, UpdateCard};

/// How often the cabin should poll when enabled.
pub const POLL_EVERY_MS: u64 = 15_000;
/// No repeat alert for the same printer + kind within this window.
pub const ALERT_COOLDOWN_MS: u64 = 5 * 60 * 1000;
/// Discovery probe timeout budget (cabin enforces per-request).
pub const DISCOVER_TIMEOUT_MS: u64 = 800;

pub const SOURCE_PREFIX: &str = "printalert:";
pub const LEDGER_FILE: &str = "print-alerts.json";
/// Tool name parked on the hard approval card for pause/cancel.
pub const CONTROL_TOOL: &str = "printer_control";

/// Settings → Pulse. Off by default; camera snapshots are a second opt-in.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrintAlertsSettings {
    #[serde(default)]
    pub enabled: bool,
    /// Fetch a camera frame when the printer exposes one and an alert fires.
    #[serde(default)]
    pub camera: bool,
}

impl PrintAlertsSettings {
    pub fn is_default(&self) -> bool {
        !self.enabled && !self.camera
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrinterKind {
    Moonraker,
    OctoPrint,
}

impl PrinterKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Moonraker => "Moonraker",
            Self::OctoPrint => "OctoPrint",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrintState {
    Idle,
    Printing,
    Paused,
    Complete,
    Error,
    Filament,
}

impl PrintState {
    pub fn label(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Printing => "printing",
            Self::Paused => "paused",
            Self::Complete => "finished",
            Self::Error => "failed",
            Self::Filament => "out of filament",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AlertKind {
    Finished,
    Failed,
    Paused,
    Filament,
}

impl AlertKind {
    pub fn key(self) -> &'static str {
        match self {
            Self::Finished => "finished",
            Self::Failed => "failed",
            Self::Paused => "paused",
            Self::Filament => "filament",
        }
    }

    pub fn title(self, printer: &str) -> String {
        let name = printer.trim();
        let name = if name.is_empty() { "Printer" } else { name };
        match self {
            Self::Finished => format!("{name} finished a print"),
            Self::Failed => format!("{name} print failed"),
            Self::Paused => format!("{name} print paused"),
            Self::Filament => format!("{name} needs filament"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ControlKind {
    Pause,
    Cancel,
    Resume,
}

impl ControlKind {
    pub fn key(self) -> &'static str {
        match self {
            Self::Pause => "pause",
            Self::Cancel => "cancel",
            Self::Resume => "resume",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Pause => "Pause print",
            Self::Cancel => "Cancel print",
            Self::Resume => "Resume print",
        }
    }

    /// Pause/cancel/resume always need the hard approval card.
    pub fn requires_approval(self) -> bool {
        true
    }
}

/// One polled status sample (no host or credentials — those stay in the cabin).
#[derive(Debug, Clone, PartialEq)]
pub struct PrinterStatus {
    pub name: String,
    pub kind: PrinterKind,
    pub state: PrintState,
    pub progress_pct: Option<f32>,
    pub eta_secs: Option<u64>,
    pub filename: Option<String>,
    pub message: Option<String>,
    /// Relative or absolute webcam snapshot path when the API exposes one.
    pub camera_path: Option<String>,
}

impl PrinterStatus {
    pub fn progress_line(&self) -> String {
        let mut parts = Vec::new();
        if let Some(p) = self.progress_pct {
            parts.push(format!("{:.0}%", p.clamp(0.0, 100.0)));
        }
        if let Some(eta) = self.eta_secs {
            parts.push(format!("ETA {}", format_eta(eta)));
        }
        if let Some(f) = self
            .filename
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            parts.push(f.to_string());
        }
        if parts.is_empty() {
            self.state.label().into()
        } else {
            parts.join(" · ")
        }
    }
}

fn format_eta(secs: u64) -> String {
    if secs < 60 {
        format!("{secs}s")
    } else if secs < 3600 {
        format!("{}m", secs / 60)
    } else {
        format!("{}h{}m", secs / 3600, (secs % 3600) / 60)
    }
}

/// Card meta so the cabin can park pause/cancel behind the approval card.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrintAlertMeta {
    pub printer_key: String,
    pub alert: String,
    pub printer_name: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub controls: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub progress: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrintAlertsLedger {
    #[serde(default)]
    pub last_poll_ms: u64,
    /// Last seen [`PrintState`] label per printer key.
    #[serde(default)]
    pub last_state: BTreeMap<String, String>,
    /// Last alert post time per `printer_key:alert_kind`.
    #[serde(default)]
    pub last_alert_ms: BTreeMap<String, u64>,
    /// User-saved printers (host:port only; no credentials). Keys are stable ids.
    #[serde(default)]
    pub printers: BTreeMap<String, SavedPrinter>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedPrinter {
    /// `host:port` — never committed; lives only under the user's config dir.
    pub endpoint: String,
    pub kind: PrinterKind,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    #[serde(default)]
    pub enabled: bool,
    /// Optional API key for OctoPrint (local config only; never logged).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AlertResult {
    pub kind: AlertKind,
    pub printer_key: String,
    pub status: PrinterStatus,
    pub camera_enabled: bool,
}

pub fn is_print_alert_card(card: &UpdateCard) -> bool {
    card.source_id.starts_with(SOURCE_PREFIX)
}

pub fn alert_source_id(printer_key: &str, kind: AlertKind) -> String {
    format!("{SOURCE_PREFIX}{}:{}", printer_key.trim(), kind.key())
}

pub fn dedupe_key(printer_key: &str, kind: AlertKind) -> String {
    format!("{}:{}", printer_key.trim(), kind.key())
}

/// Ports commonly used by Moonraker (7125 / Creality) and OctoPrint (80/5000).
pub fn discover_ports() -> &'static [u16] {
    &[7125, 4408, 80, 5000]
}

/// Identify a printer API from a successful HTTP body (path-relative).
pub fn identify_api(path: &str, body: &str) -> Option<PrinterKind> {
    let path = path.to_ascii_lowercase();
    let body_l = body.to_ascii_lowercase();
    let looks_moonraker = path.contains("printer/info")
        || path.contains("server/info")
        || body_l.contains("\"klipper_path\"")
        || body_l.contains("\"moonraker\"")
        || (body_l.contains("\"result\"") && body_l.contains("\"state\""));
    if looks_moonraker
        && (body_l.contains("\"klippy_state\"")
            || body_l.contains("moonraker")
            || path.contains("/printer/")
            || path.contains("/server/"))
    {
        return Some(PrinterKind::Moonraker);
    }
    if path.contains("/api/version")
        || path.contains("/api/job")
        || (body_l.contains("\"api\"")
            && body_l.contains("\"server\"")
            && body_l.contains("octoprint"))
        || (body_l.contains("\"job\"")
            && body_l.contains("\"progress\"")
            && body_l.contains("\"state\""))
    {
        return Some(PrinterKind::OctoPrint);
    }
    if body_l.contains("octoprint") {
        return Some(PrinterKind::OctoPrint);
    }
    if body_l.contains("moonraker") || body_l.contains("klipper") {
        return Some(PrinterKind::Moonraker);
    }
    None
}

fn filament_hint(text: &str) -> bool {
    let t = text.to_ascii_lowercase();
    t.contains("filament")
        || t.contains("runout")
        || t.contains("out of filament")
        || t.contains("filament_sensor")
}

fn moonraker_state(raw: &str, message: &str) -> PrintState {
    let s = raw.trim().to_ascii_lowercase();
    if filament_hint(message) || filament_hint(&s) {
        return PrintState::Filament;
    }
    match s.as_str() {
        "printing" => PrintState::Printing,
        "paused" | "pausing" => PrintState::Paused,
        "complete" | "completed" | "done" => PrintState::Complete,
        "error" | "cancelled" | "canceled" | "klippy_shutdown" => PrintState::Error,
        "standby" | "ready" | "" => PrintState::Idle,
        _ if s.contains("error") || s.contains("cancel") => PrintState::Error,
        _ if s.contains("pause") => PrintState::Paused,
        _ if s.contains("print") => PrintState::Printing,
        _ => PrintState::Idle,
    }
}

/// Parse a Moonraker `/printer/objects/query` (or flattened print_stats) JSON body.
pub fn parse_moonraker_status(body: &str) -> Option<PrinterStatus> {
    let v: serde_json::Value = serde_json::from_str(body).ok()?;
    let status = v.get("result").and_then(|r| r.get("status")).unwrap_or(&v);
    let stats = status
        .get("print_stats")
        .or_else(|| status.get("printStats"))?;
    let state_raw = stats.get("state").and_then(|s| s.as_str()).unwrap_or("");
    let filename = stats
        .get("filename")
        .and_then(|s| s.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let message = stats
        .get("message")
        .and_then(|s| s.as_str())
        .map(str::to_string)
        .filter(|s| !s.trim().is_empty());
    let msg = message.as_deref().unwrap_or("");
    let state = moonraker_state(state_raw, msg);

    let progress = status
        .get("display_status")
        .and_then(|d| d.get("progress"))
        .and_then(|p| p.as_f64())
        .or_else(|| {
            status
                .get("virtual_sdcard")
                .and_then(|d| d.get("progress"))
                .and_then(|p| p.as_f64())
        })
        .map(|p| {
            // Moonraker progress is 0.0..=1.0.
            if p <= 1.0 {
                (p as f32) * 100.0
            } else {
                p as f32
            }
        });

    let print_duration = stats.get("print_duration").and_then(|d| d.as_f64());
    let total_duration = stats.get("total_duration").and_then(|d| d.as_f64());
    let eta_secs = match (progress, print_duration) {
        (Some(pct), Some(done)) if pct > 0.5 && pct < 100.0 => {
            let remaining = done * ((100.0 - pct) as f64) / (pct as f64);
            Some(remaining.max(0.0) as u64)
        }
        _ => total_duration
            .zip(print_duration)
            .map(|(t, d)| (t - d).max(0.0) as u64)
            .filter(|&s| s > 0),
    };

    let name = v
        .get("result")
        .and_then(|r| r.get("hostname"))
        .and_then(|h| h.as_str())
        .or_else(|| status.get("hostname").and_then(|h| h.as_str()))
        .unwrap_or("Moonraker printer")
        .trim()
        .to_string();

    let camera_path = if body.to_ascii_lowercase().contains("webcam") {
        Some("/webcam/?action=snapshot".into())
    } else {
        // Common Fluidd/Mainsail default; cabin only fetches when Settings → camera is on.
        Some("/webcam/?action=snapshot".into())
    };

    Some(PrinterStatus {
        name: if name.is_empty() {
            "Moonraker printer".into()
        } else {
            name
        },
        kind: PrinterKind::Moonraker,
        state,
        progress_pct: progress,
        eta_secs,
        filename,
        message,
        camera_path,
    })
}

/// Parse an OctoPrint `/api/job` JSON body.
pub fn parse_octoprint_job(body: &str) -> Option<PrinterStatus> {
    let v: serde_json::Value = serde_json::from_str(body).ok()?;
    let state_obj = v.get("state")?;
    let flags = state_obj.get("flags");
    let text = state_obj
        .get("text")
        .and_then(|t| t.as_str())
        .unwrap_or("")
        .to_string();
    let printing = flags
        .and_then(|f| f.get("printing"))
        .and_then(|b| b.as_bool())
        .unwrap_or(false);
    let paused = flags
        .and_then(|f| f.get("paused"))
        .and_then(|b| b.as_bool())
        .unwrap_or(false);
    let error = flags
        .and_then(|f| f.get("error"))
        .and_then(|b| b.as_bool())
        .unwrap_or(false)
        || text.to_ascii_lowercase().contains("error");
    let operational = flags
        .and_then(|f| f.get("operational"))
        .and_then(|b| b.as_bool())
        .unwrap_or(false);

    let progress = v
        .get("progress")
        .and_then(|p| p.get("completion"))
        .and_then(|c| c.as_f64())
        .map(|c| c as f32);
    let print_time_left = v
        .get("progress")
        .and_then(|p| p.get("printTimeLeft"))
        .and_then(|t| t.as_u64());
    let filename = v
        .get("job")
        .and_then(|j| j.get("file"))
        .and_then(|f| f.get("name").or_else(|| f.get("display")))
        .and_then(|n| n.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);

    let state = if filament_hint(&text) {
        PrintState::Filament
    } else if error {
        PrintState::Error
    } else if paused {
        PrintState::Paused
    } else if printing {
        PrintState::Printing
    } else if progress.is_some_and(|p| (p - 100.0).abs() < 0.5)
        || text.eq_ignore_ascii_case("operational")
            && !printing
            && progress.is_some_and(|p| p > 99.0)
    {
        PrintState::Complete
    } else if operational || text.to_ascii_lowercase().contains("operational") {
        // Job just finished: completion ~100 and not printing.
        if progress.is_some_and(|p| p >= 99.5) {
            PrintState::Complete
        } else {
            PrintState::Idle
        }
    } else {
        PrintState::Idle
    };

    Some(PrinterStatus {
        name: "OctoPrint".into(),
        kind: PrinterKind::OctoPrint,
        state,
        progress_pct: progress,
        eta_secs: print_time_left,
        filename,
        message: if text.is_empty() { None } else { Some(text) },
        camera_path: Some("/webcam/?action=snapshot".into()),
    })
}

/// Detect a user-visible alert from a previous → current state transition.
pub fn detect_alert(prev: Option<PrintState>, curr: PrintState) -> Option<AlertKind> {
    match (prev, curr) {
        // First sample while already finished/failed: no alert (missed mid-print).
        (None, _) => None,
        (Some(PrintState::Printing), PrintState::Complete) => Some(AlertKind::Finished),
        (Some(PrintState::Paused), PrintState::Complete) => Some(AlertKind::Finished),
        (Some(PrintState::Printing), PrintState::Error) => Some(AlertKind::Failed),
        (Some(PrintState::Paused), PrintState::Error) => Some(AlertKind::Failed),
        (Some(PrintState::Printing), PrintState::Paused) => Some(AlertKind::Paused),
        (Some(PrintState::Printing), PrintState::Filament)
        | (Some(PrintState::Paused), PrintState::Filament) => Some(AlertKind::Filament),
        (Some(PrintState::Filament), PrintState::Filament) => None,
        _ => None,
    }
}

pub fn controls_for(state: PrintState) -> Vec<ControlKind> {
    match state {
        PrintState::Printing => vec![ControlKind::Pause, ControlKind::Cancel],
        PrintState::Paused | PrintState::Filament => {
            vec![ControlKind::Resume, ControlKind::Cancel]
        }
        PrintState::Error => vec![ControlKind::Cancel],
        PrintState::Idle | PrintState::Complete => Vec::new(),
    }
}

/// Moonraker / OctoPrint HTTP path + method for a control (no host).
pub fn control_request(
    kind: PrinterKind,
    control: ControlKind,
) -> (&'static str, &'static str, &'static str) {
    match (kind, control) {
        (PrinterKind::Moonraker, ControlKind::Pause) => ("POST", "/printer/print/pause", ""),
        (PrinterKind::Moonraker, ControlKind::Cancel) => ("POST", "/printer/print/cancel", ""),
        (PrinterKind::Moonraker, ControlKind::Resume) => ("POST", "/printer/print/resume", ""),
        (PrinterKind::OctoPrint, ControlKind::Pause) => (
            "POST",
            "/api/job",
            r#"{"command":"pause","action":"pause"}"#,
        ),
        (PrinterKind::OctoPrint, ControlKind::Cancel) => {
            ("POST", "/api/job", r#"{"command":"cancel"}"#)
        }
        (PrinterKind::OctoPrint, ControlKind::Resume) => (
            "POST",
            "/api/job",
            r#"{"command":"pause","action":"resume"}"#,
        ),
    }
}

pub fn may_alert(
    ledger: &PrintAlertsLedger,
    printer_key: &str,
    kind: AlertKind,
    now_ms: u64,
) -> bool {
    let key = dedupe_key(printer_key, kind);
    !ledger
        .last_alert_ms
        .get(&key)
        .is_some_and(|t| now_ms.saturating_sub(*t) < ALERT_COOLDOWN_MS)
}

pub fn record_alert(
    ledger: &mut PrintAlertsLedger,
    printer_key: &str,
    kind: AlertKind,
    now_ms: u64,
) {
    ledger
        .last_alert_ms
        .insert(dedupe_key(printer_key, kind), now_ms);
}

pub fn record_state(ledger: &mut PrintAlertsLedger, printer_key: &str, state: PrintState) {
    ledger
        .last_state
        .insert(printer_key.to_string(), state.label().into());
}

pub fn last_state(ledger: &PrintAlertsLedger, printer_key: &str) -> Option<PrintState> {
    let raw = ledger.last_state.get(printer_key)?;
    match raw.as_str() {
        "idle" => Some(PrintState::Idle),
        "printing" => Some(PrintState::Printing),
        "paused" => Some(PrintState::Paused),
        "finished" => Some(PrintState::Complete),
        "failed" => Some(PrintState::Error),
        "out of filament" => Some(PrintState::Filament),
        _ => None,
    }
}

/// One poll of a printer: update ledger and optionally produce an alert.
pub fn observe(
    settings: &PrintAlertsSettings,
    ledger: &mut PrintAlertsLedger,
    printer_key: &str,
    status: &PrinterStatus,
    now_ms: u64,
) -> Option<AlertResult> {
    if !settings.enabled {
        return None;
    }
    let prev = last_state(ledger, printer_key);
    let alert = detect_alert(prev, status.state);
    record_state(ledger, printer_key, status.state);
    let kind = alert?;
    if !may_alert(ledger, printer_key, kind, now_ms) {
        return None;
    }
    record_alert(ledger, printer_key, kind, now_ms);
    Some(AlertResult {
        kind,
        printer_key: printer_key.to_string(),
        status: status.clone(),
        camera_enabled: settings.camera,
    })
}

pub fn alert_card(result: &AlertResult, now_ms: u64) -> UpdateCard {
    let name = if result.status.name.trim().is_empty() {
        "Printer"
    } else {
        result.status.name.trim()
    };
    let source = alert_source_id(&result.printer_key, result.kind);
    let progress = result.status.progress_line();
    let mut body = progress.clone();
    if let Some(msg) = result
        .status
        .message
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        body = format!("{body}\n{msg}");
    }
    body.push_str("\nPause and cancel need the approval card.");

    let mut card = suggestion_card(&source, &result.kind.title(name), &body, now_ms);
    let controls = controls_for(result.status.state);
    // Primary Run action: first control when any; else none.
    if let Some(c) = controls.first() {
        card.prompt = Some(c.label().into());
    }
    let meta = PrintAlertMeta {
        printer_key: result.printer_key.clone(),
        alert: result.kind.key().into(),
        printer_name: name.into(),
        controls: controls.iter().map(|c| c.key().to_string()).collect(),
        progress: Some(progress.clone()),
    };
    let mut why = format!(
        "LAN printer ({})\nState: {}\n{}",
        result.status.kind.label(),
        result.status.state.label(),
        progress
    );
    if result.camera_enabled {
        why.push_str("\nCamera snapshot: on (when the printer exposes one).");
    }
    card.why = Some(why);
    card.details = Some(format!(
        "{}\n\nControls require the approval card before GrokHub sends pause/cancel/resume to the printer.",
        body
    ));
    card.pulse = PulseMeta {
        category: Some("maker".into()),
        because: Some("3D printer on your LAN".into()),
        source_name: Some(name.into()),
        print_alert: Some(meta),
        ..Default::default()
    };
    card
}

/// JSON action string stored on the parked hard card.
pub fn control_action_json(printer_key: &str, control: ControlKind) -> String {
    serde_json::json!({
        "printer": printer_key,
        "control": control.key(),
    })
    .to_string()
}

pub fn parse_control_action(action: &str) -> Option<(String, ControlKind)> {
    let v: serde_json::Value = serde_json::from_str(action).ok()?;
    let printer = v.get("printer")?.as_str()?.trim().to_string();
    if printer.is_empty() {
        return None;
    }
    let control = match v.get("control")?.as_str()? {
        "pause" => ControlKind::Pause,
        "cancel" => ControlKind::Cancel,
        "resume" => ControlKind::Resume,
        _ => return None,
    };
    Some((printer, control))
}

#[cfg(test)]
mod tests {
    use super::*;

    const MOONRAKER_PRINTING: &str = r#"{
      "result": {
        "status": {
          "print_stats": {
            "state": "printing",
            "filename": "bracket.gcode",
            "print_duration": 600.0,
            "total_duration": 3600.0,
            "message": ""
          },
          "display_status": { "progress": 0.42 }
        }
      }
    }"#;

    const MOONRAKER_COMPLETE: &str = r#"{
      "result": {
        "status": {
          "print_stats": {
            "state": "complete",
            "filename": "bracket.gcode",
            "print_duration": 3500.0,
            "total_duration": 3500.0,
            "message": ""
          },
          "display_status": { "progress": 1.0 }
        }
      }
    }"#;

    const MOONRAKER_ERROR: &str = r#"{
      "result": {
        "status": {
          "print_stats": {
            "state": "error",
            "filename": "bracket.gcode",
            "message": "Move exceeds maximum extrusion"
          },
          "display_status": { "progress": 0.55 }
        }
      }
    }"#;

    const MOONRAKER_PAUSED: &str = r#"{
      "result": {
        "status": {
          "print_stats": {
            "state": "paused",
            "filename": "bracket.gcode",
            "message": ""
          },
          "display_status": { "progress": 0.60 }
        }
      }
    }"#;

    const MOONRAKER_FILAMENT: &str = r#"{
      "result": {
        "status": {
          "print_stats": {
            "state": "paused",
            "filename": "bracket.gcode",
            "message": "Filament runout detected"
          },
          "display_status": { "progress": 0.33 }
        }
      }
    }"#;

    const OCTO_PRINTING: &str = r#"{
      "job": { "file": { "name": "benchy.gcode" } },
      "progress": { "completion": 25.5, "printTimeLeft": 4200 },
      "state": {
        "text": "Printing",
        "flags": { "operational": true, "printing": true, "paused": false, "error": false }
      }
    }"#;

    const OCTO_FAILED: &str = r#"{
      "job": { "file": { "name": "benchy.gcode" } },
      "progress": { "completion": 40.0, "printTimeLeft": null },
      "state": {
        "text": "Error",
        "flags": { "operational": false, "printing": false, "paused": false, "error": true }
      }
    }"#;

    #[test]
    fn moonraker_printing_progress_and_eta() {
        let s = parse_moonraker_status(MOONRAKER_PRINTING).expect("parse");
        assert_eq!(s.state, PrintState::Printing);
        assert_eq!(s.filename.as_deref(), Some("bracket.gcode"));
        assert!((s.progress_pct.unwrap() - 42.0).abs() < 0.1);
        assert!(s.progress_line().contains("42%"));
        assert!(s.progress_line().contains("bracket.gcode"));
    }

    #[test]
    fn moonraker_complete_failed_paused_filament() {
        assert_eq!(
            parse_moonraker_status(MOONRAKER_COMPLETE).unwrap().state,
            PrintState::Complete
        );
        assert_eq!(
            parse_moonraker_status(MOONRAKER_ERROR).unwrap().state,
            PrintState::Error
        );
        assert_eq!(
            parse_moonraker_status(MOONRAKER_PAUSED).unwrap().state,
            PrintState::Paused
        );
        assert_eq!(
            parse_moonraker_status(MOONRAKER_FILAMENT).unwrap().state,
            PrintState::Filament
        );
    }

    #[test]
    fn octoprint_job_parse() {
        let s = parse_octoprint_job(OCTO_PRINTING).expect("parse");
        assert_eq!(s.state, PrintState::Printing);
        assert!((s.progress_pct.unwrap() - 25.5).abs() < 0.01);
        assert_eq!(s.eta_secs, Some(4200));
        assert_eq!(
            parse_octoprint_job(OCTO_FAILED).unwrap().state,
            PrintState::Error
        );
    }

    #[test]
    fn mock_responses_produce_correct_alerts() {
        let mut settings = PrintAlertsSettings {
            enabled: true,
            camera: false,
        };
        let mut ledger = PrintAlertsLedger::default();
        let printing = parse_moonraker_status(MOONRAKER_PRINTING).unwrap();
        assert!(observe(&settings, &mut ledger, "k1", &printing, 1_000).is_none());

        let done = parse_moonraker_status(MOONRAKER_COMPLETE).unwrap();
        let alert = observe(&settings, &mut ledger, "k1", &done, 2_000).expect("finished");
        assert_eq!(alert.kind, AlertKind::Finished);
        let card = alert_card(&alert, 2_000);
        assert!(is_print_alert_card(&card));
        assert!(card.title.contains("finished"));
        assert!(card.body.as_deref().unwrap().contains("100%"));

        // Cooldown: same finish again is suppressed.
        ledger.last_state.insert("k1".into(), "printing".into());
        assert!(observe(&settings, &mut ledger, "k1", &done, 2_500).is_none());

        // Fail path
        let mut ledger2 = PrintAlertsLedger::default();
        let _ = observe(&settings, &mut ledger2, "op", &printing, 1);
        let failed = parse_moonraker_status(MOONRAKER_ERROR).unwrap();
        let fail = observe(&settings, &mut ledger2, "op", &failed, 2).expect("fail");
        assert_eq!(fail.kind, AlertKind::Failed);

        // Pause + filament
        let mut ledger3 = PrintAlertsLedger::default();
        let _ = observe(&settings, &mut ledger3, "p", &printing, 1);
        let paused = parse_moonraker_status(MOONRAKER_PAUSED).unwrap();
        assert_eq!(
            observe(&settings, &mut ledger3, "p", &paused, 2)
                .unwrap()
                .kind,
            AlertKind::Paused
        );
        let mut ledger4 = PrintAlertsLedger::default();
        let _ = observe(&settings, &mut ledger4, "f", &printing, 1);
        let fil = parse_moonraker_status(MOONRAKER_FILAMENT).unwrap();
        assert_eq!(
            observe(&settings, &mut ledger4, "f", &fil, 2).unwrap().kind,
            AlertKind::Filament
        );

        // Disabled: never alerts
        settings.enabled = false;
        let mut ledger5 = PrintAlertsLedger::default();
        let _ = observe(&settings, &mut ledger5, "x", &printing, 1);
        assert!(observe(&settings, &mut ledger5, "x", &done, 2).is_none());

        // OctoPrint fail
        let mut ledger6 = PrintAlertsLedger::default();
        let op = parse_octoprint_job(OCTO_PRINTING).unwrap();
        let _ = observe(
            &PrintAlertsSettings {
                enabled: true,
                camera: true,
            },
            &mut ledger6,
            "octo",
            &op,
            1,
        );
        let bad = parse_octoprint_job(OCTO_FAILED).unwrap();
        let a = observe(
            &PrintAlertsSettings {
                enabled: true,
                camera: true,
            },
            &mut ledger6,
            "octo",
            &bad,
            2,
        )
        .expect("octo fail");
        assert_eq!(a.kind, AlertKind::Failed);
        assert!(a.camera_enabled);
    }

    #[test]
    fn controls_always_require_approval() {
        for c in [ControlKind::Pause, ControlKind::Cancel, ControlKind::Resume] {
            assert!(c.requires_approval(), "{c:?}");
        }
        let printing = controls_for(PrintState::Printing);
        assert_eq!(printing, vec![ControlKind::Pause, ControlKind::Cancel]);
        let (method, path, _) = control_request(PrinterKind::Moonraker, ControlKind::Cancel);
        assert_eq!(method, "POST");
        assert!(path.contains("cancel"));
        let (m2, p2, body) = control_request(PrinterKind::OctoPrint, ControlKind::Pause);
        assert_eq!(m2, "POST");
        assert_eq!(p2, "/api/job");
        assert!(body.contains("pause"));

        let json = control_action_json("k1", ControlKind::Cancel);
        let (key, ctrl) = parse_control_action(&json).unwrap();
        assert_eq!(key, "k1");
        assert_eq!(ctrl, ControlKind::Cancel);
        assert!(parse_control_action(r#"{"printer":"","control":"pause"}"#).is_none());
    }

    #[test]
    fn identify_api_from_fixture_bodies() {
        assert_eq!(
            identify_api("/printer/objects/query", MOONRAKER_PRINTING),
            Some(PrinterKind::Moonraker)
        );
        assert_eq!(
            identify_api("/api/job", OCTO_PRINTING),
            Some(PrinterKind::OctoPrint)
        );
        assert_eq!(identify_api("/health", "ok"), None);
    }

    #[test]
    fn settings_default_omits_and_first_sample_skips_alert() {
        assert!(PrintAlertsSettings::default().is_default());
        assert!(detect_alert(None, PrintState::Complete).is_none());
        assert_eq!(
            detect_alert(Some(PrintState::Printing), PrintState::Complete),
            Some(AlertKind::Finished)
        );
    }
}
