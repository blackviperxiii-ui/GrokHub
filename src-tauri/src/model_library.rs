//! Settings → Local AI → Download models for the desktop app: the same rows,
//! tabs and Download all as the cabin (`grokhub_core::model_library`), with
//! one background worker that downloads the queue a model at a time from
//! Hugging Face and reports progress on [`LIBRARY_EVENT`].

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use grokhub_agent::route::local_sidecar;
use grokhub_core::local_picker::{BenchCache, Profile};
use grokhub_core::local_setup::{self as ls, LocalModel, ModelKind, ModelTab};
use grokhub_core::local_skills::TaskKind;
use grokhub_core::model_download::{self as md, Control, Outcome, Progress, Step};
use grokhub_core::model_library::{self as lib, Fit, Row, RowError};
use grokhub_core::model_lineup as lineup;
use grokhub_core::model_offer as offer;
use grokhub_core::runtime_download as rd;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};

use crate::local_engine::{self, Busy, HfFetch, DRAFTER_BUSY};

pub const LIBRARY_EVENT: &str = "model-library";
const MIB: u64 = 1_024 * 1_024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryRow {
    pub id: String,
    pub name: String,
    /// Name, size, fit, job, Uncensored, licence.
    pub line: String,
    /// The catalog job, in the words the row shows (`Coding`, `Video gen`).
    pub use_case: String,
    /// What it is for, and why when the fit isn't clean.
    pub about: String,
    /// `download`, `resume`, `queued`, `downloading`, `downloaded`, `inUse`
    /// or `wontRun`.
    pub state: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QueueOffer {
    pub ids: Vec<String>,
    pub line: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryTab {
    pub label: &'static str,
    pub rows: Vec<LibraryRow>,
    /// Said when the tab has no models yet.
    pub empty: Option<&'static str>,
    pub all: Option<QueueOffer>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryJob {
    pub id: String,
    pub line: String,
    pub pct: u8,
}

/// The download worker's state, also the event payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryStatus {
    pub job: Option<LibraryJob>,
    pub queued: usize,
    pub message: Option<String>,
    /// Why a model's last download stopped, by model id, for under its row.
    pub errors: BTreeMap<String, RowError>,
    /// The worker is running; when it turns false, read the page again.
    pub running: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryView {
    pub tabs: Vec<LibraryTab>,
    /// The best lineup for this PC, one model per goal for hot-swap: what it
    /// fetches (`ids`, maybe none left) and the line that says so first.
    pub lineup: Option<QueueOffer>,
    pub everything: Option<QueueOffer>,
    pub status: LibraryStatus,
}

struct Run {
    ctl: Arc<Control>,
    model: &'static LocalModel,
    done: u64,
    total: Option<u64>,
    verifying: bool,
}

#[derive(Default)]
struct State {
    queue: Vec<&'static LocalModel>,
    run: Option<Run>,
    message: Option<String>,
    /// Each model's last failure until it is downloaded again or finishes.
    errors: BTreeMap<&'static str, RowError>,
    worker: bool,
}

fn state() -> &'static Mutex<State> {
    static S: OnceLock<Mutex<State>> = OnceLock::new();
    S.get_or_init(Default::default)
}

/// The folder a catalog model downloads into, shared with the cabin.
pub fn dir_for(config_dir: &Path, m: &LocalModel) -> PathBuf {
    if m.kind == ModelKind::Drafter {
        local_engine::drafters_dir(config_dir)
    } else {
        let models = crate::local_ai::models_dir().unwrap_or_else(|| config_dir.join("models"));
        md::kind_dir(&models, m.kind)
    }
}

fn on_disk(config_dir: &Path, m: &LocalModel) -> bool {
    md::complete(&dir_for(config_dir, m), m)
}

/// When each model, and any model for each task kind, was last offered
/// (ms), so neither is asked about again within [`offer::OFFER_EVERY_MS`].
#[derive(Debug, Default, Serialize, Deserialize)]
struct Offered {
    #[serde(default)]
    models: BTreeMap<String, u64>,
    #[serde(default)]
    kinds: BTreeMap<String, u64>,
}

fn offers_path(config_dir: &Path) -> PathBuf {
    config_dir.join("local").join("install_offers.json")
}

fn read_offers(config_dir: &Path) -> Offered {
    std::fs::read_to_string(offers_path(config_dir))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

/// A model nothing on this PC fits well is looked for again at most this
/// often per task kind, since reading this PC runs the GPU probes.
const FIT_CHECK_EVERY_MS: u64 = 60 * 60 * 1000;

/// The question and the line its Yes runs, for the chat model not on this
/// PC that would do `kind` clearly better than every chat model here and
/// fits this PC well. Whatever the answer, neither that model nor another
/// one for `kind` is offered again for 30 days; a model already queued or
/// downloading isn't offered. Reads this
/// PC only when the disk leaves one worth offering, so call it off the main
/// thread.
pub fn install_offer(config_dir: &Path, kind: TaskKind, now_ms: u64) -> Option<(String, String)> {
    let here = |m: &LocalModel| on_disk(config_dir, m);
    offer_with(config_dir, kind, now_ms, here, || {
        let p = crate::local_ai::read_machine(config_dir).profile();
        lib::rows(&p, &BenchCache::new(), here)
    })
}

/// [`install_offer`] with what's downloaded told by `on_disk` and this PC's
/// rows read by `rows`, only when needed.
fn offer_with(
    config_dir: &Path,
    kind: TaskKind,
    now_ms: u64,
    on_disk: impl Fn(&LocalModel) -> bool,
    rows: impl FnOnce() -> Vec<Row>,
) -> Option<(String, String)> {
    static CHECKED: Mutex<BTreeMap<(PathBuf, &'static str), u64>> = Mutex::new(BTreeMap::new());
    let mut shown = read_offers(config_dir);
    if !offer::due(&shown.kinds, kind.key(), now_ms) {
        return None;
    }
    let pending: Vec<&str> = match state().lock() {
        Ok(s) => s
            .queue
            .iter()
            .map(|m| m.id)
            .chain(s.run.as_ref().map(|r| r.model.id))
            .collect(),
        Err(_) => return None,
    };
    let wanted: Vec<&'static LocalModel> = offer::worth_offering(ls::local_models(), kind, on_disk)
        .into_iter()
        .filter(|m| offer::due(&shown.models, m.id, now_ms) && !pending.contains(&m.id))
        .collect();
    if wanted.is_empty() {
        return None;
    }
    let mut checked = CHECKED.lock().ok()?;
    let key = (config_dir.to_path_buf(), kind.key());
    if checked
        .get(&key)
        .is_some_and(|&at| now_ms >= at && now_ms - at < FIT_CHECK_EVERY_MS)
    {
        return None;
    }
    let Some(m) = offer::first_fitting(&wanted, &rows()) else {
        checked.insert(key, now_ms);
        return None;
    };
    shown.models.insert(m.id.to_string(), now_ms);
    shown.kinds.insert(kind.key().to_string(), now_ms);
    let path = offers_path(config_dir);
    let text = serde_json::to_string_pretty(&shown).ok()?;
    // Asked only once it is kept: an offer that can't be remembered would
    // come back after every reply.
    grokhub_core::pulse_writer::write_atomic(&path, &text).ok()?;
    Some((offer::prompt(m, kind), format!("/download {}", m.id)))
}

fn status_of(s: &State) -> LibraryStatus {
    LibraryStatus {
        job: s.run.as_ref().map(|r| LibraryJob {
            id: r.model.id.into(),
            line: if r.verifying {
                format!("Checking {}…", r.model.name)
            } else {
                format!(
                    "Downloading {}: {}",
                    r.model.name,
                    md::progress_line(r.done, r.total)
                )
            },
            pct: md::percent(r.done, r.total),
        }),
        queued: s.queue.len(),
        message: s.message.clone(),
        errors: s
            .errors
            .iter()
            .map(|(id, e)| ((*id).to_string(), e.clone()))
            .collect(),
        running: s.worker,
    }
}

fn offer(q: Vec<&'static LocalModel>) -> Option<QueueOffer> {
    (!q.is_empty()).then(|| QueueOffer {
        line: lib::queue_line(&q),
        ids: q.iter().map(|m| m.id.to_string()).collect(),
    })
}

/// The page from catalog rows: pure, so the tests drive it.
fn build(
    rows: &[Row],
    p: &Profile,
    free_mb: Option<u64>,
    in_use: Option<&str>,
    partial: impl Fn(&LocalModel) -> bool,
    status: LibraryStatus,
    queued: &[&str],
) -> LibraryView {
    let busy = status.job.as_ref().map(|j| j.id.clone());
    let row = |r: &Row| {
        let m = r.model;
        let mut about = lib::good_for(m);
        if r.fit != Fit::Yes && !r.why.is_empty() {
            about = format!("{about} {}", r.why);
        }
        let state = if busy.as_deref() == Some(m.id) {
            "downloading"
        } else if in_use == Some(m.file) {
            "inUse"
        } else if r.on_disk {
            "downloaded"
        } else if r.fit == Fit::No {
            "wontRun"
        } else if queued.contains(&m.id) {
            "queued"
        } else if partial(m) {
            "resume"
        } else {
            "download"
        };
        LibraryRow {
            id: m.id.into(),
            name: m.name.into(),
            line: lib::row_line(r),
            use_case: ls::use_case(m.id).label().into(),
            about,
            state,
        }
    };
    let tabs = ModelTab::ALL
        .into_iter()
        .map(|t| {
            let shown: Vec<LibraryRow> = rows.iter().filter(|r| r.tab() == t).map(row).collect();
            LibraryTab {
                label: t.label(),
                empty: shown.is_empty().then(|| lib::empty_tab_line(t)),
                rows: shown,
                all: offer(lib::queue(rows, Some(t), free_mb)),
            }
        })
        .collect();
    let l = lineup::recommend(rows, p, free_mb);
    LibraryView {
        tabs,
        lineup: Some(QueueOffer {
            line: lineup::lineup_line(&l),
            ids: l.queue.iter().map(|m| m.id.to_string()).collect(),
        }),
        everything: offer(lib::queue(rows, None, free_mb)),
        status,
    }
}

fn free_mb(config_dir: &Path) -> Option<u64> {
    let dir = dir_for(config_dir, &ls::local_models()[0]);
    crate::local_ai::free_disk_mb(&crate::local_ai::existing(&dir))
}

/// The model file the engine runs: the pick, or the one it found.
fn in_use_file(config_dir: &Path) -> Option<String> {
    local_sidecar::find_model(config_dir)
        .and_then(|p| p.file_name().map(|f| f.to_string_lossy().into_owned()))
}

/// The page: reads this PC, so call it off the main thread.
pub fn view(config_dir: &Path) -> LibraryView {
    let p = crate::local_ai::read_machine(config_dir).profile();
    let rows = lib::rows(&p, &BenchCache::new(), |m| on_disk(config_dir, m));
    let loaded = in_use_file(config_dir);
    let (status, queued) = match state().lock() {
        Ok(s) => (status_of(&s), s.queue.iter().map(|m| m.id).collect()),
        Err(_) => (status_of(&State::default()), Vec::new()),
    };
    build(
        &rows,
        &p,
        free_mb(config_dir),
        loaded.as_deref(),
        |m| md::partial_bytes(&dir_for(config_dir, m), m).is_some(),
        status,
        &queued,
    )
}

/// The next queued model not on disk yet, taken off the queue.
fn take_next(
    queue: &mut Vec<&'static LocalModel>,
    on_disk: impl Fn(&LocalModel) -> bool,
) -> Option<&'static LocalModel> {
    while !queue.is_empty() {
        let m = queue.remove(0);
        if !on_disk(m) {
            return Some(m);
        }
    }
    None
}

fn emit(app: &AppHandle) {
    if let Ok(s) = state().lock() {
        let _ = app.emit(LIBRARY_EVENT, status_of(&s));
    }
}

/// Queue `ids` behind anything queued and start the worker when idle.
pub fn download(
    app: AppHandle,
    config_dir: PathBuf,
    ids: Vec<String>,
) -> Result<LibraryView, String> {
    let models = ids
        .iter()
        .map(|id| ls::model_by_id(id).ok_or_else(|| format!("{id} isn't in the catalog.")))
        .collect::<Result<Vec<_>, _>>()?;
    let start = {
        let mut s = state().lock().map_err(|e| e.to_string())?;
        enqueue(&mut s, models)
    };
    if start {
        let app2 = app.clone();
        let dir = config_dir.clone();
        std::thread::spawn(move || work(&app2, &dir));
    }
    emit(&app);
    Ok(view(&config_dir))
}

/// Queue `models` behind anything queued; each one's last error goes, since
/// it is being tried again. `true` when the worker has to start.
fn enqueue(s: &mut State, models: Vec<&'static LocalModel>) -> bool {
    for m in models {
        s.errors.remove(m.id);
        if !s.queue.iter().any(|q| q.id == m.id)
            && s.run.as_ref().is_none_or(|r| r.model.id != m.id)
        {
            s.queue.push(m);
        }
    }
    s.message = None;
    let start = !s.worker;
    s.worker = true;
    start
}

/// Why one model's download didn't finish.
enum Halt {
    /// The user paused or cancelled it, so the queue stops.
    Stop(String),
    /// It failed: the reason goes on its row and the queue goes on.
    Failed(RowError),
}

/// Record how `m` ended. A failure stays on that model's row and the rest
/// of the queue still downloads; the user's pause or cancel stops the queue.
fn settle(s: &mut State, m: &'static LocalModel, end: Result<String, Halt>) {
    s.run = None;
    match end {
        Ok(said) => {
            s.errors.remove(m.id);
            s.message = Some(said);
        }
        Err(Halt::Stop(said)) => {
            s.queue.clear();
            s.message = Some(said);
        }
        Err(Halt::Failed(e)) => {
            s.message = Some(format!("{} didn't download. Its row says why.", m.name));
            s.errors.insert(m.id, e);
        }
    }
}

fn work(app: &AppHandle, config_dir: &Path) {
    loop {
        let next = match state().lock() {
            Ok(mut s) => take_next(&mut s.queue, |m| on_disk(config_dir, m)),
            Err(_) => None,
        };
        let Some(m) = next else { break };
        let end = fetch_one(app, config_dir, m);
        if let Ok(mut s) = state().lock() {
            settle(&mut s, m, end);
        }
        emit(app);
    }
    if let Ok(mut s) = state().lock() {
        s.worker = false;
    }
    emit(app);
}

/// Download one model: `Ok` with what to say, or how it stopped.
fn fetch_one(app: &AppHandle, config_dir: &Path, m: &'static LocalModel) -> Result<String, Halt> {
    // The draft model offer's own download may hold this file already.
    let _busy = if m.kind == ModelKind::Drafter {
        match Busy::claim(&DRAFTER_BUSY) {
            Some(b) => Some(b),
            None => return Ok(format!("{} is already downloading.", m.name)),
        }
    } else {
        None
    };
    let dir = dir_for(config_dir, m);
    // A resumed download only needs room for what is left.
    let need = md::space_needed_mb(&dir, m);
    if let Some(free) = crate::local_ai::free_disk_mb(&crate::local_ai::existing(&dir)) {
        if free < need {
            return Err(Halt::Failed(lib::space_error(m, free, need)));
        }
    }
    let ctl = Arc::new(Control::default());
    if let Ok(mut s) = state().lock() {
        s.run = Some(Run {
            ctl: ctl.clone(),
            model: m,
            done: md::partial_bytes(&dir, m).unwrap_or(0),
            total: Some(m.download_mb * MIB),
            verifying: false,
        });
    }
    emit(app);
    let out = md::download(&dir, m, &HfFetch, &ctl, &mut |p| {
        if let Ok(mut s) = state().lock() {
            if let Some(r) = s.run.as_mut() {
                match p {
                    Progress::Bytes { done, total } => {
                        r.done = done;
                        if total.is_some() {
                            r.total = total;
                        }
                    }
                    Progress::Verifying => r.verifying = true,
                }
            }
        }
        emit(app);
    });
    // A speech or picture model needs its engine (about 30 MB) beside
    // llama.cpp; its files stay if this stops, and the next try fetches only
    // the engine.
    let out = match (out, grokhub_core::tool_engine::engine_for(m.kind)) {
        (Outcome::Done(r), Some(tool)) => match rd::platform_key() {
            Some(key) => match tool.download(
                config_dir,
                key,
                &local_engine::HttpEngineFetch,
                &ctl,
                &mut |_| {},
            ) {
                Ok(_) | Err(rd::Outcome::Done { .. }) => Outcome::Done(r),
                Err(rd::Outcome::Paused { done }) => Outcome::Paused { done },
                Err(rd::Outcome::Cancelled) => Outcome::Cancelled,
                Err(rd::Outcome::BadChecksum) => Outcome::Failed {
                    step: Step::Engine,
                    why: format!(
                        "The engine {} runs on didn't match its published checksum, so it was deleted. Download it again to retry.",
                        m.name
                    ),
                },
                Err(rd::Outcome::Failed(why)) => Outcome::Failed {
                    step: Step::Engine,
                    why,
                },
            },
            None => Outcome::Done(r),
        },
        (out, _) => out,
    };
    if let Some(e) = lib::row_error(m, &out) {
        return Err(Halt::Failed(e));
    }
    match out {
        Outcome::Paused { done } => Err(Halt::Stop(format!(
            "Paused {} at {}. Download it again to pick up where it stopped.",
            m.name,
            md::progress_line(done, Some(m.download_mb * MIB))
        ))),
        Outcome::Cancelled => Err(Halt::Stop(format!("Cancelled the {} download.", m.name))),
        // Done: a failure already returned above.
        _ => {
            if m.kind == ModelKind::Drafter {
                // The next reply loads with it; the speed test decides.
                local_sidecar::stop_global();
            }
            Ok(format!("Downloaded {}.", m.name))
        }
    }
}

/// Pause the running download; the queue stops and the `.part` stays.
pub fn pause() -> Result<(), String> {
    let s = state().lock().map_err(|e| e.to_string())?;
    if let Some(r) = &s.run {
        r.ctl.pause();
    }
    Ok(())
}

/// Remove a downloaded model that isn't in use.
pub fn remove(config_dir: &Path, id: &str) -> Result<LibraryView, String> {
    let m = ls::model_by_id(id).ok_or_else(|| format!("{id} isn't in the catalog."))?;
    if in_use_file(config_dir).as_deref() == Some(m.file) {
        return Err(format!("{} is the model in use, so it stays.", m.name));
    }
    if m.kind == ModelKind::Drafter {
        // A running model may hold its draft model open.
        local_sidecar::stop_global();
    }
    md::remove(&dir_for(config_dir, m), m)?;
    Ok(view(config_dir))
}

// --- Live Hugging Face GGUF browse (Auto pool only; no runtime picker) ---

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HfHit {
    pub repo: String,
    pub downloads: u64,
    pub likes: u64,
    pub updated: String,
    pub gated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HfFileRow {
    pub label: String,
    pub paths: Vec<String>,
    pub size: String,
    /// Fits this PC / Fits, may be slow / Won't run well here.
    pub fit: &'static str,
    pub fit_kind: &'static str,
}

fn hf_get_json(url: &str) -> Result<String, String> {
    use std::io::Read;
    use std::time::Duration;
    grokhub_core::offline::refuse()?;
    let agent = grokhub_core::http::agent(grokhub_core::http::Opts {
        connect: Duration::from_secs(15),
        read: Some(Duration::from_secs(30)),
        redirects: 0,
        public_only: true,
        ..Default::default()
    });
    let resp = agent
        .get(url)
        .header("user-agent", grokhub_agent::USER_AGENT)
        .call()
        .map_err(|e| e.to_string())?;
    if resp.status() >= 400 {
        return Err(format!("Hugging Face returned HTTP {}.", resp.status()));
    }
    let mut body = String::new();
    resp.into_reader()
        .read_to_string(&mut body)
        .map_err(|e| e.to_string())?;
    Ok(body)
}

fn budget_mb(config_dir: &Path) -> (u64, u64) {
    let p = crate::local_ai::read_machine(config_dir).profile();
    let (mem, usable) = p.budget();
    let ram = match mem {
        grokhub_core::local_picker::Memory::Gpu => p.ram_mb.saturating_sub(4_096),
        _ => 0,
    };
    (usable, ram)
}

/// Search Hugging Face for GGUF models (most downloaded first).
pub fn search_hf(query: &str, limit: usize) -> Result<Vec<HfHit>, String> {
    let q = query.trim();
    if q.is_empty() {
        return Ok(Vec::new());
    }
    let body = hf_get_json(&grokhub_core::hf_gguf::search_url(q, limit))?;
    Ok(grokhub_core::hf_gguf::parse_search(&body)
        .into_iter()
        .map(|h| HfHit {
            repo: h.repo,
            downloads: h.downloads,
            likes: h.likes,
            updated: h.updated,
            gated: h.gated,
        })
        .collect())
}

/// GGUF quants in `repo`, priced for this PC.
pub fn hf_files(config_dir: &Path, repo: &str) -> Result<Vec<HfFileRow>, String> {
    let repo = repo.trim().trim_matches('/');
    if !repo.contains('/') {
        return Err("Pick a Hugging Face repo like owner/name.".into());
    }
    let body = hf_get_json(&grokhub_core::hf_gguf::tree_url(repo))?;
    let (vram, ram) = budget_mb(config_dir);
    let groups =
        grokhub_core::hf_gguf::price_groups(&grokhub_core::hf_gguf::parse_tree(&body), vram, ram);
    Ok(groups
        .into_iter()
        .map(|g| {
            let fit_kind = match g.fit {
                Fit::Yes => "yes",
                Fit::Tight => "tight",
                Fit::No => "no",
            };
            HfFileRow {
                label: g.label,
                paths: g.paths,
                size: ls::size_label(g.total_bytes / MIB),
                fit: g.fit.label(),
                fit_kind,
            }
        })
        .collect())
}

/// Download one GGUF (first path of a group) into the models folder and add it
/// to the Auto pool (`ownFiles`). No runtime model picker.
pub fn download_hf(config_dir: &Path, repo: &str, path: &str) -> Result<String, String> {
    use md::Fetch as _;
    use std::io::{Read, Write};
    let repo = repo.trim().trim_matches('/');
    let path = path.trim().trim_start_matches('/');
    if !repo.contains('/') || path.is_empty() || !path.to_ascii_lowercase().ends_with(".gguf") {
        return Err("Pick a .gguf file in a Hugging Face repo.".into());
    }
    if path.contains("..") {
        return Err("That file path isn't allowed.".into());
    }
    let name = path.rsplit('/').next().unwrap_or(path);
    let dest_dir = crate::local_ai::models_dir().unwrap_or_else(|| config_dir.join("models"));
    std::fs::create_dir_all(&dest_dir).map_err(|e| e.to_string())?;
    let dest = dest_dir.join(name);
    if dest.exists() {
        register_own_file(config_dir, &dest)?;
        return Ok(format!("{name} is already on this PC. Added to Auto."));
    }
    let url = grokhub_core::hf_gguf::file_url(repo, path);
    let fetch = HfFetch;
    let (mut reader, _, total) = fetch.open(&url, 0)?;
    let part = {
        let mut s = dest.as_os_str().to_owned();
        s.push(".part");
        PathBuf::from(s)
    };
    let mut out = std::fs::File::create(&part).map_err(|e| e.to_string())?;
    let mut buf = vec![0u8; 256 * 1_024];
    let mut done = 0u64;
    loop {
        let n = reader.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        out.write_all(&buf[..n]).map_err(|e| e.to_string())?;
        done += n as u64;
        if total.is_some_and(|t| done > t.saturating_mul(2)) {
            let _ = std::fs::remove_file(&part);
            return Err("Download grew past the size Hugging Face reported.".into());
        }
    }
    drop(out);
    // Prefer the Hub LFS oid when the tree lists it.
    if let Ok(want) = fetch.sha256(repo, path) {
        let got = md::sha256_file(&part).map_err(|e| e.to_string())?;
        if !got.eq_ignore_ascii_case(&want) {
            let _ = std::fs::remove_file(&part);
            return Err("The downloaded file didn't match Hugging Face's checksum.".into());
        }
    }
    std::fs::rename(&part, &dest).map_err(|e| e.to_string())?;
    register_own_file(config_dir, &dest)?;
    // Auto may pick it up without a restart once assets refresh.
    local_sidecar::clear_paths_cache();
    Ok(format!("Downloaded {name} into Auto."))
}

fn register_own_file(config_dir: &Path, path: &Path) -> Result<(), String> {
    let shown = path.display().to_string();
    let mut config = crate::local_ai::load(config_dir);
    if !config.own_files.iter().any(|f| f == &shown) {
        config.own_files.push(shown);
        crate::local_ai::save(config_dir, &config)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use grokhub_core::local_setup::CpuSimd;

    const MID: &str = "gemma-4-12b-it-abliterated-q4_k";
    const MID_DRAFT: &str = "gemma-4-12b-it-abliterated-mtp";

    fn pc() -> Profile {
        Profile {
            gpus_vram_mb: vec![12_288],
            unified: false,
            ram_mb: 32_768,
            free_disk_mb: 500_000,
            simd: CpuSimd::Avx2,
            llama_build: None,
        }
    }

    fn rows(on: &[&str]) -> Vec<Row> {
        lib::rows(&pc(), &BenchCache::new(), |m| on.contains(&m.id))
    }

    fn idle() -> LibraryStatus {
        status_of(&State::default())
    }

    fn row<'a>(v: &'a LibraryView, id: &str) -> &'a LibraryRow {
        v.tabs
            .iter()
            .flat_map(|t| &t.rows)
            .find(|r| r.id == id)
            .unwrap()
    }

    #[test]
    fn the_page_has_five_tabs_and_each_row_says_what_it_can_do() {
        let mid = ls::model_by_id(MID).unwrap();
        let v = build(
            &rows(&[MID]),
            &pc(),
            Some(500_000),
            Some(mid.file),
            |_| false,
            idle(),
            &[],
        );
        let labels: Vec<&str> = v.tabs.iter().map(|t| t.label).collect();
        assert_eq!(labels, ["Chat", "Drafts", "Image", "Voice", "Other"]);
        assert_eq!(row(&v, MID).state, "inUse");
        assert_eq!(row(&v, MID_DRAFT).state, "download");
        // The picture model and the guard that checks its pictures.
        assert_eq!(v.tabs[2].empty, None);
        assert_eq!(row(&v, "z-image-turbo-q4_k").state, "download");
        assert_eq!(row(&v, "qwen3-vl-2b-guard-q8_0").state, "download");
        assert_eq!(
            v.tabs[1].all.as_ref().map(|o| o.ids.clone()),
            Some(vec![MID_DRAFT.to_string()])
        );
        let v = build(
            &rows(&[MID]),
            &pc(),
            Some(500_000),
            None,
            |m| m.id == MID_DRAFT,
            idle(),
            &[],
        );
        assert_eq!(row(&v, MID).state, "downloaded");
        assert_eq!(row(&v, MID_DRAFT).state, "resume");
        let v = build(
            &rows(&[]),
            &pc(),
            Some(500_000),
            None,
            |_| false,
            idle(),
            &[MID],
        );
        assert_eq!(row(&v, MID).state, "queued");
        assert!(v.everything.is_some());
        // The lineup says what it fetches and for which goal before the click.
        let l = v.lineup.expect("a lineup");
        assert!(!l.ids.is_empty());
        assert!(
            l.line.contains("Everyday: ") && l.line.contains("Video: "),
            "{}",
            l.line
        );
    }

    #[test]
    fn the_worker_takes_the_next_model_not_on_disk() {
        let mid = ls::model_by_id(MID).unwrap();
        let draft = ls::model_by_id(MID_DRAFT).unwrap();
        let mut q = vec![mid, draft];
        assert_eq!(
            take_next(&mut q, |m| m.id == MID).map(|m| m.id),
            Some(MID_DRAFT)
        );
        assert!(q.is_empty());
        assert_eq!(take_next(&mut q, |_| false), None);
    }
    #[test]
    fn a_failed_model_keeps_the_rest_of_the_queue_and_its_reason_on_its_row() {
        let mid = ls::model_by_id(MID).unwrap();
        let draft = ls::model_by_id(MID_DRAFT).unwrap();
        let mut s = State::default();
        assert!(enqueue(&mut s, vec![mid, draft]), "the worker starts");
        assert_eq!(take_next(&mut s.queue, |_| false).map(|m| m.id), Some(MID));
        settle(
            &mut s,
            mid,
            Err(Halt::Failed(lib::space_error(mid, 1_024, 9_000))),
        );
        // The next model still downloads.
        assert_eq!(
            s.queue.iter().map(|m| m.id).collect::<Vec<_>>(),
            [MID_DRAFT]
        );
        let st = status_of(&s);
        assert_eq!(
            st.message.as_deref(),
            Some("Gemma 4 12B didn't download. Its row says why.")
        );
        // The reason is keyed by the model, so the page shows it on that row.
        let json = serde_json::to_value(&st).unwrap();
        assert_eq!(json["errors"][MID]["step"], "free space");
        assert_eq!(
            json["errors"][MID]["detail"],
            "Failed at: free space (1 GB free, 8.8 GB needed)"
        );
        // Downloading it again clears the old reason.
        assert!(!enqueue(&mut s, vec![mid]), "the worker already runs");
        assert!(status_of(&s).errors.is_empty());
        assert_eq!(
            s.queue.iter().map(|m| m.id).collect::<Vec<_>>(),
            [MID_DRAFT, MID]
        );
        // A pause from the user still stops the whole queue.
        settle(&mut s, draft, Err(Halt::Stop("Paused.".into())));
        assert!(s.queue.is_empty());
        assert_eq!(status_of(&s).message.as_deref(), Some("Paused."));
        // A model that finishes drops any reason it had.
        s.errors
            .insert(MID, lib::row_error(mid, &Outcome::BadChecksum).unwrap());
        settle(&mut s, mid, Ok("Downloaded Gemma 4 12B.".into()));
        assert!(s.errors.is_empty());
    }

    /// An empty config folder for the offer record.
    fn config(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("gh-offer-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn on(ids: &'static [&'static str]) -> impl Fn(&LocalModel) -> bool {
        move |m| ids.contains(&m.id)
    }

    #[test]
    fn a_device_reply_offers_one_better_model_per_task_and_reads_this_pc_only_when_needed() {
        // Qwen3 Coder 30B codes at 90, ahead of Qwen 3.6 at 85, and this
        // 12 GB GPU still has room for it.
        const CODER: &str = "qwen3-coder-30b-a3b-heretic-q4_k_m";
        let dir = config("offer");
        let read = std::cell::Cell::new(0);
        let pc_rows = || {
            read.set(read.get() + 1);
            rows(&[MID])
        };
        // Everyday chat stays on the model here, and this PC isn't read.
        assert_eq!(
            offer_with(&dir, TaskKind::Chat, 1_000, on(&[MID]), pc_rows),
            None
        );
        assert_eq!(read.get(), 0);
        let coder = ls::model_by_id(CODER).unwrap();
        assert_eq!(
            offer_with(&dir, TaskKind::Code, 1_000, on(&[MID]), pc_rows),
            Some((
                format!(
                    "{} would handle coding better than the models on this computer. Download it? ({})",
                    coder.name,
                    ls::size_label(coder.download_mb)
                ),
                format!("/download {CODER}"),
            ))
        );
        assert_eq!(read.get(), 1);
        // Asked once: no other coding offer for 30 days, and no PC read for it.
        assert_eq!(
            offer_with(&dir, TaskKind::Code, 2_000, on(&[MID]), pc_rows),
            None
        );
        assert_eq!(read.get(), 1);
        // Another task has its own offer.
        assert_eq!(
            offer_with(&dir, TaskKind::Reason, 3_000, on(&[MID]), pc_rows).map(|(_, yes)| yes),
            Some("/download qwen3.8-27b-heretic-q4_k_m".to_string())
        );
        // Kept on disk, so a restart doesn't ask again.
        let kept = read_offers(&dir);
        assert_eq!(kept.kinds.get("code"), Some(&1_000));
        assert_eq!(kept.models.get(CODER), Some(&1_000));
        // Thirty days on, coding is due again and the same model with it.
        let later = 1_000 + offer::OFFER_EVERY_MS;
        assert_eq!(
            offer_with(&dir, TaskKind::Code, later, on(&[MID]), pc_rows).map(|(_, yes)| yes),
            Some(format!("/download {CODER}"))
        );
        // Downloaded now: nothing beats it at coding by enough to ask.
        let dir2 = config("offer-have");
        assert_eq!(
            offer_with(&dir2, TaskKind::Code, later, on(&[MID, CODER]), || {
                rows(&[MID, CODER])
            }),
            None
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn when_nothing_fits_well_this_pc_is_read_again_only_after_an_hour() {
        let dir = config("offer-none");
        let read = std::cell::Cell::new(0);
        let none_fit = || {
            read.set(read.get() + 1);
            Vec::new()
        };
        assert_eq!(
            offer_with(&dir, TaskKind::Long, 1_000, on(&[MID]), none_fit),
            None
        );
        assert_eq!(
            offer_with(&dir, TaskKind::Long, 2_000, on(&[MID]), none_fit),
            None
        );
        assert_eq!(read.get(), 1);
        assert_eq!(
            offer_with(
                &dir,
                TaskKind::Long,
                1_000 + FIT_CHECK_EVERY_MS,
                on(&[MID]),
                none_fit
            ),
            None
        );
        assert_eq!(read.get(), 2);
        // Nothing was offered, so nothing was kept.
        assert!(!offers_path(&dir).exists());
        let _ = std::fs::remove_dir_all(dir);
    }
}
