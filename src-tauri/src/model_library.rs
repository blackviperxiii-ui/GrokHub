//! Settings → Local AI → Download models for the desktop app: the same rows,
//! tabs and Download all as the cabin (`grokhub_core::model_library`), with
//! one background worker that downloads the queue a model at a time from
//! Hugging Face and reports progress on [`LIBRARY_EVENT`].

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use grokhub_agent::route::local_sidecar;
use grokhub_core::local_picker::{BenchCache, Profile};
use grokhub_core::local_setup::{self as ls, LocalModel, ModelKind, ModelTab};
use grokhub_core::model_download::{self as md, Control, Outcome, Progress};
use grokhub_core::model_library::{self as lib, Fit, Row};
use grokhub_core::model_lineup as lineup;
use grokhub_core::runtime_download as rd;
use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::local_engine::{self, Busy, HfFetch, DRAFTER_BUSY};

pub const LIBRARY_EVENT: &str = "model-library";
const MIB: u64 = 1_024 * 1_024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryRow {
    pub id: String,
    pub name: String,
    /// Name, size, fit, Uncensored, licence.
    pub line: String,
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
        for m in models {
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
    };
    if start {
        let app2 = app.clone();
        let dir = config_dir.clone();
        std::thread::spawn(move || work(&app2, &dir));
    }
    emit(&app);
    Ok(view(&config_dir))
}

fn work(app: &AppHandle, config_dir: &Path) {
    loop {
        let next = match state().lock() {
            Ok(mut s) => take_next(&mut s.queue, |m| on_disk(config_dir, m)),
            Err(_) => None,
        };
        let Some(m) = next else { break };
        let message = fetch_one(app, config_dir, m);
        if let Ok(mut s) = state().lock() {
            s.run = None;
            if let Err(stop) = message {
                s.queue.clear();
                s.message = Some(stop);
            } else {
                s.message = message.ok();
            }
        }
        emit(app);
    }
    if let Ok(mut s) = state().lock() {
        s.worker = false;
    }
    emit(app);
}

/// Download one model. `Ok` with what to say, or `Err` when the queue stops.
fn fetch_one(app: &AppHandle, config_dir: &Path, m: &'static LocalModel) -> Result<String, String> {
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
    if let Some(free) = crate::local_ai::free_disk_mb(&crate::local_ai::existing(&dir)) {
        if free < ls::disk_needed_mb(m) {
            return Err(format!(
                "Stopped: {} needs {} free, and this drive has {}.",
                m.name,
                ls::size_label(ls::disk_needed_mb(m)),
                ls::size_label(free)
            ));
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
                Err(rd::Outcome::BadChecksum) => Outcome::BadChecksum,
                Err(rd::Outcome::Failed(e)) => Outcome::Failed(e),
            },
            None => Outcome::Done(r),
        },
        (out, _) => out,
    };
    match out {
        Outcome::Done(_) => {
            if m.kind == ModelKind::Drafter {
                // The next reply loads with it; the speed test decides.
                local_sidecar::stop_global();
            }
            Ok(format!("Downloaded {}.", m.name))
        }
        Outcome::Paused { done } => Err(format!(
            "Paused {} at {}. Download it again to pick up where it stopped.",
            m.name,
            md::progress_line(done, Some(m.download_mb * MIB))
        )),
        Outcome::Cancelled => Err(format!("Cancelled the {} download.", m.name)),
        Outcome::BadChecksum => Err(format!(
            "{} didn't match its published checksum, so it was deleted.",
            m.name
        )),
        Outcome::Failed(e) => Err(e),
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
            l.line.contains("Balance: ") && l.line.contains("Video: "),
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
}
