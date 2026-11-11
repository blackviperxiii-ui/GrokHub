//! The work behind the PC helpers' file, system and printer switches, for the
//! shell while the cabin is closed. Every job reads its own switch first and
//! does nothing while it is off, and none of them runs while the cabin's
//! scheduler beat is fresh (the cabin runs the same jobs when it is open).
//!
//! Downloads tidy only suggests: files move to the Trash after the person
//! ticks them and approves the exact list. Model drop keeps only names with a
//! model extension. Printers are only ever asked for their status, and only
//! when they are on this network.

use std::collections::BTreeSet;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use grokhub_core::backup_nudge::{self as bn, Reminder};
use grokhub_core::code_resume::{self as cr, RepoStatus};
use grokhub_core::downloads_tidy::{self as dt, Finding};
use grokhub_core::http;
use grokhub_core::model_drop::{self as md, Slicer};
use grokhub_core::print_alerts::{
    self as pa, alert_card, identify_api, parse_moonraker_status, parse_octoprint_job, PrinterKind,
    PrinterStatus, SavedPrinter,
};
use grokhub_core::scheduler_lease::{read_lease, shell_may_fire};
use serde::Serialize;

use crate::pc_files;

/// How often the loop wakes to see which job is due.
const BEAT: Duration = Duration::from_secs(5);
/// A new model in Downloads is looked for this often while the switch is on.
const MODEL_EVERY_MS: u64 = 20_000;

/// What each job last did, kept between beats.
#[derive(Default)]
pub struct Run {
    tidy_checked: bool,
    models_seen: BTreeSet<String>,
    models_first_done: bool,
    models_ms: u64,
    backup_checked: bool,
    print_ms: u64,
}

/// What one beat did, for the tests.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Did {
    pub tidy_posted: bool,
    pub models_posted: usize,
    pub backup_posted: bool,
    pub print_posted: usize,
}

fn home() -> PathBuf {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_default()
}

fn downloads() -> PathBuf {
    grokhub_core::downloads_dir().unwrap_or_else(|| home().join("Downloads"))
}

/// Where installed apps leave a name: Start Menu shortcuts on Windows,
/// desktop entries on Linux.
fn app_roots() -> Vec<PathBuf> {
    let start_menu = PathBuf::from("Microsoft")
        .join("Windows")
        .join("Start Menu")
        .join("Programs");
    let mut roots: Vec<PathBuf> = ["ProgramData", "APPDATA"]
        .iter()
        .filter_map(std::env::var_os)
        .map(|v| PathBuf::from(v).join(&start_menu))
        .collect();
    if !cfg!(windows) {
        let home = home();
        roots.extend([
            PathBuf::from("/usr/share/applications"),
            PathBuf::from("/var/lib/flatpak/exports/share/applications"),
            home.join(".local/share/applications"),
            home.join(".local/share/flatpak/exports/share/applications"),
        ]);
    }
    roots
}

// ---- Downloads tidy -------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TidyRow {
    pub name: String,
    pub bytes: u64,
    pub size: String,
    pub kind: String,
    pub why: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TidyDone {
    pub moved: usize,
    pub note: String,
}

fn scan_dir(dir: &Path, roots: &[PathBuf], now: u64) -> Vec<Finding> {
    let (files, dirs) = dt::list_dir(dir);
    let installed = dt::installed_names(roots);
    dt::classify(&files, &dirs, &installed, now, &mut |p| dt::sha256_file(p))
}

fn rows(found: &[Finding]) -> Vec<TidyRow> {
    found
        .iter()
        .map(|f| TidyRow {
            name: f.file.name.clone(),
            bytes: f.file.bytes,
            size: dt::human_bytes(f.file.bytes),
            kind: f.kind.label().to_string(),
            why: f.why.clone(),
        })
        .collect()
}

/// What could go from Downloads right now. Nothing is changed.
pub fn tidy_review() -> Vec<TidyRow> {
    rows(&scan_dir(
        &downloads(),
        &app_roots(),
        grokhub_core::now_ms(),
    ))
}

/// Move the ticked files to the Trash. The folder is read again first, so
/// only a file that is still a suggestion can go, whatever the page sends.
pub fn tidy_trash(names: &[String]) -> Result<TidyDone, String> {
    trash_from(
        &downloads(),
        &app_roots(),
        names,
        &grokhub_desktop::trash_paths,
    )
}

pub fn trash_from(
    dir: &Path,
    roots: &[PathBuf],
    names: &[String],
    trash: &dyn Fn(&[PathBuf]) -> Result<(), String>,
) -> Result<TidyDone, String> {
    let found = scan_dir(dir, roots, grokhub_core::now_ms());
    let picked: Vec<&Finding> = found
        .iter()
        .filter(|f| names.contains(&f.file.name))
        .collect();
    if picked.is_empty() {
        return Err("None of those files are suggestions any more. Nothing was moved.".into());
    }
    let paths: Vec<PathBuf> = picked.iter().map(|f| f.file.path.clone()).collect();
    let bytes: u64 = picked.iter().map(|f| f.file.bytes).sum();
    trash(&paths)?;
    Ok(TidyDone {
        moved: paths.len(),
        note: format!(
            "Moved {} {} ({}) to the Trash.",
            paths.len(),
            if paths.len() == 1 { "file" } else { "files" },
            dt::human_bytes(bytes)
        ),
    })
}

/// The weekly check, once per launch while it is due.
fn tidy_tick(config_dir: &Path, now: u64, scan: &dyn Fn() -> Vec<Finding>) -> bool {
    if !dt::load(config_dir).due(now) {
        return false;
    }
    let found = scan();
    let _ = dt::save(config_dir, &dt::Tidy { last_check_ms: now });
    dt::card(&found, now)
        .is_some_and(|card| crate::pulse::add_post(config_dir, card).unwrap_or(false))
}

// ---- Model drop -----------------------------------------------------------

fn detect_slicers() -> Vec<Slicer> {
    if cfg!(windows) {
        let roots: Vec<PathBuf> = ["ProgramFiles", "ProgramFiles(x86)"]
            .iter()
            .filter_map(std::env::var_os)
            .map(PathBuf::from)
            .collect();
        md::windows_slicers(&roots)
    } else {
        let flatpak = [
            home().join(".local/share/flatpak/app"),
            PathBuf::from("/var/lib/flatpak/app"),
        ];
        md::linux_slicers(&grokhub_core::runtime_download::find_on_path, &flatpak)
    }
}

/// Size a model (when small enough) and post its card; no preview picture.
fn offer(file: md::ModelFile) -> md::ModelOffer {
    let mut offer = md::ModelOffer {
        dims: None,
        thumb: None,
        file,
    };
    if offer.file.bytes <= md::PREVIEW_MAX_BYTES {
        if let Ok(bytes) = std::fs::read(&offer.file.path) {
            let (_, mesh) = md::mesh_for(&offer.file.name, &bytes);
            offer.dims = mesh.as_ref().and_then(md::Mesh::size);
        }
    }
    offer
}

fn models_tick(
    config_dir: &Path,
    dir: &Path,
    seen: &mut BTreeSet<String>,
    first: bool,
    now: u64,
    slicer: Option<&Slicer>,
) -> usize {
    let mut posted = 0;
    for file in md::scan_new(dir, seen, first) {
        let card = offer(file).card(slicer, now);
        if crate::pulse::add_post(config_dir, card).unwrap_or(false) {
            posted += 1;
        }
    }
    posted
}

// ---- Code resume and backup ----------------------------------------------

/// One read-only git call. The shell is a windows-subsystem app, so a git
/// started without `hide_windows_console` would flash a console window.
fn run_git(args: &[&str]) -> Option<String> {
    let mut cmd = grokhub_core::git_cmd::git();
    cmd.args(args).stdin(Stdio::null()).stderr(Stdio::null());
    grokhub_core::proc_util::hide_windows_console(&mut cmd);
    let out = cmd.output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Git metadata for every repo in `folders`; hooks and fsmonitor stay off.
pub fn scan_repos(folders: &[String], now_s: u64) -> Vec<RepoStatus> {
    folders
        .iter()
        .flat_map(|folder| cr::repo_dirs(Path::new(folder)))
        .filter_map(|repo| cr::read_repo(&repo, now_s, &mut |args| run_git(args)))
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResumeRow {
    pub name: String,
    pub line: String,
    /// Merged branches old enough to tidy.
    pub stale_branches: usize,
}

/// Where the person left off, from the code folders they picked.
pub fn resume_rows(config_dir: &Path) -> Vec<ResumeRow> {
    let repos = scan_repos(
        &pc_files::code_folders(config_dir),
        grokhub_core::now_ms() / 1000,
    );
    repos
        .iter()
        .filter(|r| r.pending() || !r.stale_merged.is_empty())
        .map(|r| ResumeRow {
            name: r.name.clone(),
            line: r.offer_line(),
            stale_branches: r.stale_merged.len(),
        })
        .collect()
}

fn backup_check(config_dir: &Path, now: u64) -> Option<Reminder> {
    let repos = scan_repos(&pc_files::code_folders(config_dir), now / 1000);
    let folders: Vec<(PathBuf, u64)> = pc_files::important_folders(config_dir)
        .iter()
        .map(|f| {
            let path = PathBuf::from(f);
            let changed = bn::newest_change_ms(&path);
            (path, changed)
        })
        .collect();
    let root = if cfg!(windows) {
        PathBuf::from("C:\\")
    } else {
        PathBuf::from("/")
    };
    let local = std::env::var_os("LOCALAPPDATA").map(PathBuf::from);
    let tools = bn::detect_tools(&root, local.as_deref());
    bn::reminder(&repos, &folders, &tools, now)
}

/// The launch check, at most once a day and never while snoozed.
fn backup_tick(config_dir: &Path, now: u64, check: &dyn Fn() -> Option<Reminder>) -> bool {
    let mut state = bn::load(config_dir);
    if !state.may_show(now) {
        return false;
    }
    let Some(reminder) = check() else {
        return false;
    };
    let card = reminder.card(now);
    let source = card.source_id.clone();
    let posted = crate::pulse::add_post(config_dir, card).unwrap_or(false);
    state.last_shown_ms = now;
    state.reminder = Some(reminder);
    state.card_source = source;
    let _ = bn::save(config_dir, &state);
    posted
}

// ---- Internet log: speed test and export ---------------------------------

/// Time one download and log it. Returns the Mbit/s.
pub fn speed_test(config_dir: &Path) -> Result<f64, String> {
    if !pc_files::switch_on(config_dir, "netLog") {
        return Err("Turn the internet log on first.".into());
    }
    grokhub_core::offline::refuse()?;
    let client = grokhub_core::connectivity::default_http_client();
    let start = std::time::Instant::now();
    let resp = client
        .get(grokhub_core::net_log::SPEED_URL)
        .timeout(Duration::from_secs(60))
        .any_status()
        .call()
        .map_err(|e| e.to_string())?;
    if !(200..300).contains(&resp.status()) {
        return Err(format!("the test server answered {}", resp.status()));
    }
    let bytes =
        std::io::copy(&mut resp.into_reader(), &mut std::io::sink()).map_err(|e| e.to_string())?;
    let ms = u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX);
    let mut log = grokhub_core::net_log::load(config_dir);
    let mbps = log
        .note_speed(bytes, ms, grokhub_core::now_ms())
        .ok_or("The test finished too fast to measure.")?;
    grokhub_core::net_log::save(config_dir, &log)?;
    Ok(mbps)
}

// ---- LAN printers ---------------------------------------------------------

fn agent() -> http::Agent {
    // No redirects: only the printer's own address is allowed, so a redirect
    // could send the request (and its API key) off the LAN.
    http::agent(http::Opts {
        connect: Duration::from_millis(pa::DISCOVER_TIMEOUT_MS),
        read: Some(Duration::from_millis(2_000)),
        redirects: 0,
        ..Default::default()
    })
}

fn base_url(endpoint: &str) -> String {
    format!("http://{}", pc_files::printer_key(endpoint))
}

fn on_lan(endpoint: &str) -> bool {
    let key = pc_files::printer_key(endpoint);
    key.rsplit_once(':')
        .and_then(|(host, _)| host.parse::<std::net::IpAddr>().ok())
        .is_some_and(|ip| pc_files::is_lan(&ip))
}

fn get_text(agent: &http::Agent, url: &str, api_key: Option<&str>) -> Option<String> {
    let host_part = url.trim_start_matches("http://");
    if !on_lan(host_part.split('/').next().unwrap_or("")) {
        return None;
    }
    let mut req = agent.get(url);
    if let Some(key) = api_key.map(str::trim).filter(|k| !k.is_empty()) {
        req = req.header("X-Api-Key", key);
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

fn fetch_status(agent: &http::Agent, saved: &SavedPrinter) -> Option<PrinterStatus> {
    let base = base_url(&saved.endpoint);
    let key = saved.api_key.as_deref();
    let mut status = match saved.kind {
        PrinterKind::Moonraker => parse_moonraker_status(&get_text(
            agent,
            &format!("{base}/printer/objects/query?print_stats&display_status&virtual_sdcard"),
            key,
        )?)?,
        PrinterKind::OctoPrint => {
            parse_octoprint_job(&get_text(agent, &format!("{base}/api/job"), key)?)?
        }
    };
    if !saved.name.trim().is_empty() {
        status.name = saved.name.clone();
    }
    Some(status)
}

/// Ask a LAN address whether it is a Moonraker or OctoPrint printer.
pub fn identify_printer(endpoint: &str) -> Result<SavedPrinter, String> {
    let endpoint = pc_files::lan_endpoint(endpoint)?;
    let agent = agent();
    let base = base_url(&endpoint);
    for (path, expect) in [
        ("/printer/info", PrinterKind::Moonraker),
        ("/server/info", PrinterKind::Moonraker),
        ("/api/version", PrinterKind::OctoPrint),
        ("/api/job", PrinterKind::OctoPrint),
    ] {
        let Some(body) = get_text(&agent, &format!("{base}{path}"), None) else {
            continue;
        };
        let kind = identify_api(path, &body).unwrap_or(expect);
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
        return Ok(SavedPrinter {
            endpoint,
            kind,
            name,
            enabled: true,
            api_key: None,
        });
    }
    Err("No Moonraker or OctoPrint printer answered at that address.".into())
}

/// Keep the printer's webcam frame where Pulse reads card images; only a
/// small body that decodes as a picture is kept. Returns the cache key.
fn cache_snapshot(config_dir: &Path, agent: &http::Agent, saved: &SavedPrinter) -> Option<String> {
    let base = base_url(&saved.endpoint);
    let query = format!("{base}{}", pa::webcam_query_path(saved.kind));
    let path = pa::parse_webcam_path(
        saved.kind,
        &get_text(agent, &query, saved.api_key.as_deref())?,
    )?;
    let url = format!("{base}{path}");
    if !on_lan(url.trim_start_matches("http://").split('/').next()?) {
        return None;
    }
    let resp = agent.get(&url).call().ok()?;
    if !(200..300).contains(&resp.status()) {
        return None;
    }
    let mut buf = Vec::new();
    resp.into_reader()
        .take(4 * 1024 * 1024)
        .read_to_end(&mut buf)
        .ok()?;
    image::load_from_memory(&buf).ok()?;
    let dir = crate::pulse_feed::images_dir(config_dir);
    std::fs::create_dir_all(&dir).ok()?;
    std::fs::write(dir.join(grokhub_core::pulse::image_cache_name(&url)), buf).ok()?;
    Some(url)
}

/// One poll of the saved printers that are on. `fetch` reads a printer.
fn print_tick(
    config_dir: &Path,
    now: u64,
    fetch: &dyn Fn(&SavedPrinter) -> Option<PrinterStatus>,
    snapshot: &dyn Fn(&SavedPrinter) -> Option<String>,
) -> usize {
    let settings = pc_files::print_switches(config_dir);
    if !settings.enabled {
        return 0;
    }
    let mut ledger = pc_files::load_ledger(config_dir);
    ledger.last_poll_ms = now;
    let printers: Vec<(String, SavedPrinter)> = ledger
        .printers
        .iter()
        .filter(|(_, p)| p.enabled)
        .map(|(k, p)| (k.clone(), p.clone()))
        .collect();
    let mut cards = Vec::new();
    for (key, saved) in printers {
        let Some(status) = fetch(&saved) else {
            continue;
        };
        if let Some(alert) = pa::observe(&settings, &mut ledger, &key, &status, now) {
            let mut card = alert_card(&alert, now);
            if alert.camera_enabled {
                if let Some(url) = snapshot(&saved) {
                    card.pulse.image_urls = vec![url];
                }
            }
            cards.push(card);
        }
    }
    let _ = pc_files::merge_poll(config_dir, ledger);
    cards
        .into_iter()
        .filter(|card| crate::pulse::add_post(config_dir, card.clone()).unwrap_or(false))
        .count()
}

// ---- The loop -------------------------------------------------------------

/// Start the helper thread.
pub fn start(config_dir: PathBuf) {
    let _ = std::thread::Builder::new()
        .name("pc-files".into())
        .spawn(move || {
            let mut run = Run::default();
            loop {
                std::thread::sleep(BEAT);
                beat(
                    &config_dir,
                    std::process::id(),
                    grokhub_core::now_ms(),
                    &mut run,
                );
            }
        });
}

/// One look at every switch; a job runs only when its switch is on.
pub fn beat(config_dir: &Path, pid: u32, now: u64, run: &mut Run) -> Did {
    let mut did = Did::default();
    if !shell_may_fire(read_lease(config_dir).as_ref(), pid, now) {
        return did;
    }
    if pc_files::switch_on(config_dir, "downloadsTidy") && !run.tidy_checked {
        run.tidy_checked = true;
        did.tidy_posted = tidy_tick(config_dir, now, &|| {
            scan_dir(&downloads(), &app_roots(), now)
        });
    }
    if pc_files::switch_on(config_dir, "modelDrop")
        && now.saturating_sub(run.models_ms) >= MODEL_EVERY_MS
    {
        run.models_ms = now;
        let first = !std::mem::replace(&mut run.models_first_done, true);
        let slicer = if first {
            None
        } else {
            detect_slicers().into_iter().next()
        };
        did.models_posted = models_tick(
            config_dir,
            &downloads(),
            &mut run.models_seen,
            first,
            now,
            slicer.as_ref(),
        );
    }
    if pc_files::switch_on(config_dir, "backupNudge") && !run.backup_checked {
        run.backup_checked = true;
        did.backup_posted = backup_tick(config_dir, now, &|| backup_check(config_dir, now));
    }
    if now.saturating_sub(run.print_ms) >= pa::POLL_EVERY_MS
        && pc_files::print_switches(config_dir).enabled
    {
        run.print_ms = now;
        let agent = agent();
        did.print_posted = print_tick(config_dir, now, &|p| fetch_status(&agent, p), &|p| {
            cache_snapshot(config_dir, &agent, p)
        });
    }
    did
}

#[cfg(test)]
#[path = "pc_files_run_tests.rs"]
mod tests;
