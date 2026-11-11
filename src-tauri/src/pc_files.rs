//! Settings → Your PC → PC helpers, part two: the file, system and printer
//! helpers the cabin has (Downloads tidy, model drop, backup reminders, code
//! resume, the internet log and LAN printer alerts). The switches live in the
//! same `app.json` keys with the same defaults (all off), and the printers and
//! logs in the cores' own files, so the two apps agree. Nothing here reads a
//! file or the network: `pc_files_run` does that, and only for switches that
//! are on.

use std::collections::BTreeMap;
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use grokhub_core::net_log::{self, NetLog};
use grokhub_core::print_alerts::{self as pa, PrintAlertsLedger, SavedPrinter};
use serde::Serialize;
use serde_json::{json, Value};

const SWITCHES: [&str; 5] = [
    "downloadsTidy",
    "modelDrop",
    "backupNudge",
    "resumeCode",
    "netLog",
];
const PRINT_ALERTS: &str = "printAlerts";
const MODELS_FOLDER: &str = "modelsFolder";
/// The most folders one list keeps.
const MAX_FOLDERS: usize = 24;

static FILES: Mutex<()> = Mutex::new(());

fn files() -> MutexGuard<'static, ()> {
    FILES.lock().unwrap_or_else(|e| e.into_inner())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PrinterRow {
    pub key: String,
    pub name: String,
    /// "Moonraker" or "OctoPrint".
    pub kind: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FilesHelpers {
    pub downloads_tidy: bool,
    pub model_drop: bool,
    /// Where "Move to models" puts a model; empty means `~/Models`.
    pub models_folder: String,
    pub backup_nudge: bool,
    pub important_folders: Vec<String>,
    pub resume_code: bool,
    pub code_folders: Vec<String>,
    pub net_log: bool,
    /// Drops in the last 30 days, and how many fell in each hour of the day.
    pub net_drops: u32,
    pub net_by_hour: Vec<u32>,
    pub net_down: bool,
    pub net_last_speed_mbps: Option<f64>,
    pub print_alerts: bool,
    pub print_camera: bool,
    pub printers: Vec<PrinterRow>,
}

fn strings(config_dir: &Path, key: &str) -> Vec<String> {
    grokhub_core::app_json::read(config_dir)
        .ok()
        .and_then(|m| m.get(key).cloned())
        .and_then(|v| serde_json::from_value::<Vec<String>>(v).ok())
        .unwrap_or_default()
}

pub fn code_folders(config_dir: &Path) -> Vec<String> {
    strings(config_dir, "codeFolders")
}

pub fn important_folders(config_dir: &Path) -> Vec<String> {
    strings(config_dir, "importantFolders")
}

pub fn models_folder(config_dir: &Path) -> String {
    grokhub_core::app_json::read(config_dir)
        .ok()
        .and_then(|m| {
            m.get(MODELS_FOLDER)
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .unwrap_or_default()
}

/// A switch in `app.json`; every one defaults to off.
pub fn switch_on(config_dir: &Path, key: &str) -> bool {
    grokhub_core::app_json::bool_or(config_dir, key, false)
}

/// The print-alert switches: (watch, camera). The camera is its own opt-in.
pub fn print_switches(config_dir: &Path) -> pa::PrintAlertsSettings {
    grokhub_core::app_json::read(config_dir)
        .ok()
        .and_then(|m| m.get(PRINT_ALERTS).cloned())
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default()
}

pub fn ledger_path(config_dir: &Path) -> PathBuf {
    config_dir.join(pa::LEDGER_FILE)
}

pub fn load_ledger(config_dir: &Path) -> PrintAlertsLedger {
    std::fs::read_to_string(ledger_path(config_dir))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

pub fn save_ledger(config_dir: &Path, ledger: &PrintAlertsLedger) -> Result<(), String> {
    let text = serde_json::to_string_pretty(ledger).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(config_dir).map_err(|e| e.to_string())?;
    grokhub_core::pulse_writer::write_atomic(&ledger_path(config_dir), &text)
}

pub fn read(config_dir: &Path) -> FilesHelpers {
    let _files = files();
    view(config_dir)
}

fn view(config_dir: &Path) -> FilesHelpers {
    let log = net_log::load(config_dir);
    let by_hour = log.by_hour();
    let print = print_switches(config_dir);
    FilesHelpers {
        downloads_tidy: switch_on(config_dir, "downloadsTidy"),
        model_drop: switch_on(config_dir, "modelDrop"),
        models_folder: models_folder(config_dir),
        backup_nudge: switch_on(config_dir, "backupNudge"),
        important_folders: important_folders(config_dir),
        resume_code: switch_on(config_dir, "resumeCode"),
        code_folders: code_folders(config_dir),
        net_log: switch_on(config_dir, "netLog"),
        net_drops: by_hour.iter().sum(),
        net_by_hour: by_hour.to_vec(),
        net_down: log.down(),
        net_last_speed_mbps: log.speed.last().map(|s| s.mbps),
        print_alerts: print.enabled,
        print_camera: print.camera,
        printers: load_ledger(config_dir)
            .printers
            .iter()
            .map(|(key, p)| PrinterRow {
                key: key.clone(),
                name: printer_name(p),
                kind: p.kind.label().to_string(),
                enabled: p.enabled,
            })
            .collect(),
    }
}

fn printer_name(p: &SavedPrinter) -> String {
    if p.name.trim().is_empty() {
        p.endpoint.clone()
    } else {
        p.name.clone()
    }
}

/// Turn one switch on or off: `downloadsTidy`, `modelDrop`, `backupNudge`,
/// `resumeCode`, `netLog`, `printAlerts` or `printCamera`. Nothing changes
/// when the file can't be written.
pub fn set_switch(config_dir: &Path, key: &str, on: bool) -> Result<FilesHelpers, String> {
    let _files = files();
    match key {
        k if SWITCHES.contains(&k) => {
            grokhub_core::app_json::patch(config_dir, &[(k, on.into())])?;
            if k == "netLog" {
                // The log file carries the switch too, so the tiles see it.
                let mut log = net_log::load(config_dir);
                log.on = on;
                net_log::save(config_dir, &log)?;
            }
        }
        "printAlerts" | "printCamera" => {
            let mut s = print_switches(config_dir);
            if key == "printAlerts" {
                s.enabled = on;
            } else {
                s.camera = on;
            }
            grokhub_core::app_json::patch(
                config_dir,
                &[(
                    PRINT_ALERTS,
                    json!({ "enabled": s.enabled, "camera": s.camera }),
                )],
            )?;
        }
        other => return Err(format!("Unknown PC helper: {other}")),
    }
    Ok(view(config_dir))
}

/// Clean a folder list: trimmed, existing folders only, no repeats.
fn clean_folders(list: &[String]) -> Result<Vec<String>, String> {
    let mut out: Vec<String> = Vec::new();
    for raw in list {
        let path = raw.trim();
        if path.is_empty() {
            continue;
        }
        if !Path::new(path).is_absolute() {
            return Err(format!("{path} isn't a full folder path."));
        }
        if !Path::new(path).is_dir() {
            return Err(format!("{path} isn't a folder on this PC."));
        }
        if !out.iter().any(|p| p == path) {
            out.push(path.to_string());
        }
    }
    if out.len() > MAX_FOLDERS {
        return Err(format!("Pick at most {MAX_FOLDERS} folders."));
    }
    Ok(out)
}

/// Save a folder list: `codeFolders` or `importantFolders`.
pub fn set_folders(config_dir: &Path, key: &str, list: &[String]) -> Result<FilesHelpers, String> {
    if key != "codeFolders" && key != "importantFolders" {
        return Err(format!("Unknown folder list: {key}"));
    }
    let clean = clean_folders(list)?;
    let _files = files();
    grokhub_core::app_json::patch(config_dir, &[(key, json!(clean))])?;
    Ok(view(config_dir))
}

/// Where a model moves to; empty goes back to `~/Models`.
pub fn set_models_folder(config_dir: &Path, folder: &str) -> Result<FilesHelpers, String> {
    let folder = folder.trim();
    if !folder.is_empty() {
        clean_folders(&[folder.to_string()])?;
    }
    let _files = files();
    grokhub_core::app_json::patch(config_dir, &[(MODELS_FOLDER, json!(folder))])?;
    Ok(view(config_dir))
}

/// `host:port` of a printer on this network, or why it can't be one. Only
/// this PC and private LAN addresses are ever polled.
pub fn lan_endpoint(raw: &str) -> Result<String, String> {
    let text = raw
        .trim()
        .trim_start_matches("http://")
        .trim_end_matches('/');
    let (host, port) = text
        .rsplit_once(':')
        .ok_or("Type the printer as address:port, like 192.168.1.50:7125.")?;
    let port: u16 = port.parse().map_err(|_| "That port isn't a number.")?;
    let ip: IpAddr = host
        .parse()
        .map_err(|_| "Type the printer's number address, like 192.168.1.50.")?;
    if !is_lan(&ip) {
        return Err("Only printers on this network (or this PC) can be watched.".into());
    }
    Ok(format!("{host}:{port}"))
}

pub fn is_lan(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => v4.is_private() || v4.is_loopback() || v4.is_link_local(),
        IpAddr::V6(v6) => v6.is_loopback(),
    }
}

/// Printer ids are the lowercase `host:port`, the cabin's own.
pub fn printer_key(endpoint: &str) -> String {
    endpoint
        .trim()
        .trim_start_matches("http://")
        .trim_start_matches("https://")
        .trim_end_matches('/')
        .to_ascii_lowercase()
}

/// Save a printer the caller already identified.
pub fn save_printer(config_dir: &Path, printer: SavedPrinter) -> Result<FilesHelpers, String> {
    let _files = files();
    let mut ledger = load_ledger(config_dir);
    ledger
        .printers
        .insert(printer_key(&printer.endpoint), printer);
    save_ledger(config_dir, &ledger)?;
    Ok(view(config_dir))
}

pub fn remove_printer(config_dir: &Path, key: &str) -> Result<FilesHelpers, String> {
    let _files = files();
    let mut ledger = load_ledger(config_dir);
    ledger.printers.remove(key);
    ledger.last_state.remove(key);
    save_ledger(config_dir, &ledger)?;
    Ok(view(config_dir))
}

pub fn set_printer_enabled(config_dir: &Path, key: &str, on: bool) -> Result<FilesHelpers, String> {
    let _files = files();
    let mut ledger = load_ledger(config_dir);
    ledger
        .printers
        .get_mut(key)
        .ok_or("That printer isn't saved.")?
        .enabled = on;
    save_ledger(config_dir, &ledger)?;
    Ok(view(config_dir))
}

/// Record a poll's states and cooldowns; the saved printers stay as they are
/// even if the page changed them meanwhile.
pub fn merge_poll(config_dir: &Path, polled: PrintAlertsLedger) -> Result<(), String> {
    let _files = files();
    let mut ledger = load_ledger(config_dir);
    ledger.last_poll_ms = polled.last_poll_ms;
    ledger.last_state = polled
        .last_state
        .into_iter()
        .filter(|(k, _)| ledger.printers.contains_key(k))
        .collect::<BTreeMap<_, _>>();
    ledger.last_alert_ms = polled.last_alert_ms;
    save_ledger(config_dir, &ledger)
}

/// Save the internet log as a CSV in Reports (the app folder if that can't
/// be written). A second export the same day gets `-2`, never replacing the
/// first. Returns the saved file's path.
pub fn export_net_csv(config_dir: &Path, now_ms: u64) -> Result<String, String> {
    let log: NetLog = net_log::load(config_dir);
    let name = format!("internet-log-{}.csv", &net_log::iso_utc(now_ms)[..10]);
    let csv = log.csv(now_ms);
    let reports =
        grokhub_core::user_files::folder(config_dir, grokhub_core::user_files::UserFolder::Reports);
    let mut first_err = None;
    for dir in [reports, Ok(config_dir.to_path_buf())] {
        match dir.and_then(|d| grokhub_core::user_files::write_new_file(&d, &name, csv.as_bytes()))
        {
            Ok(path) => return Ok(path.to_string_lossy().into_owned()),
            Err(e) => {
                first_err.get_or_insert_with(|| grokhub_core::user_files::plain_io_error(&e));
            }
        }
    }
    Err(format!(
        "Couldn't save the log: {}. Nothing was saved.",
        first_err.unwrap_or_default()
    ))
}

#[cfg(test)]
#[path = "pc_files_tests.rs"]
mod tests;
