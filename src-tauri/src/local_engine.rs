//! Local AI engine + draft-model downloads for Settings → Local AI.
//! Mirrors the cabin's Get engine / Install-and-run draft paths using the
//! same `runtime_download` and `model_download` cores, with progress events
//! for the React panel.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use grokhub_agent::route::local_sidecar;
use grokhub_core::local_draft as ld;
use grokhub_core::local_setup::{self as ls, LocalModel};
use grokhub_core::model_download::{self as md, Control, Progress};
use grokhub_core::runtime_download::{self as rd, Manifest, Outcome as EngineOutcome};
use grokhub_core::ssrf::runtime_download_url_ok;
use serde::Serialize;
use tauri::{AppHandle, Emitter};

pub const ENGINE_EVENT: &str = "local-ai-engine";
pub const DRAFTER_EVENT: &str = "local-ai-drafter";

const REDIRECTS_MAX: u32 = 8;
const MIB: u64 = 1_024 * 1_024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineInfo {
    /// `ready`, `missing`, `downloading`, `noBuild`.
    pub state: String,
    pub version: String,
    pub size_label: String,
    pub hint: String,
    pub button: String,
    pub progress: Option<EngineProgress>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineProgress {
    pub done: u64,
    pub total: Option<u64>,
    pub verifying: bool,
    pub line: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DraftOffer {
    pub id: String,
    pub name: String,
    pub size_label: String,
    pub line: String,
    pub button: String,
    /// Catalog target model this draft pairs with.
    pub target_id: String,
    pub target_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DraftProgress {
    pub id: String,
    pub done: u64,
    pub total: Option<u64>,
    pub verifying: bool,
    pub line: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineEvent {
    pub state: String,
    pub progress: Option<EngineProgress>,
    pub error: Option<String>,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DrafterEvent {
    pub state: String,
    pub progress: Option<DraftProgress>,
    pub error: Option<String>,
    pub message: Option<String>,
}

struct EngineJob {
    ctl: Arc<Control>,
    done: u64,
    total: Option<u64>,
    verifying: bool,
    version: String,
}

struct DrafterJob {
    ctl: Arc<Control>,
    id: String,
    name: String,
    done: u64,
    total: Option<u64>,
    verifying: bool,
}

struct State {
    engine: Option<EngineJob>,
    engine_error: Option<String>,
    drafter: Option<DrafterJob>,
    drafter_error: Option<String>,
}

fn state() -> &'static Mutex<State> {
    static S: OnceLock<Mutex<State>> = OnceLock::new();
    S.get_or_init(|| {
        Mutex::new(State {
            engine: None,
            engine_error: None,
            drafter: None,
            drafter_error: None,
        })
    })
}

/// Where new draft downloads land: beside the config models folder (same
/// tree as the engine under `{config}/bin`).
fn drafters_dir(config_dir: &Path) -> PathBuf {
    config_dir.join("models").join(ld::DRAFTERS_DIR)
}

/// Config drafters first, then the home models tree — same places the
/// picker discovers GGUFs.
fn drafter_dirs(config_dir: &Path) -> Vec<PathBuf> {
    let mut dirs = vec![drafters_dir(config_dir)];
    if let Some(home) = grokhub_core::paths::user_home() {
        let alt = home
            .join(grokhub_core::brand::INTERNAL_ID)
            .join("models")
            .join(ld::DRAFTERS_DIR);
        if alt != dirs[0] {
            dirs.push(alt);
        }
    }
    dirs
}

fn offer_for(config_dir: &Path, m: &LocalModel) -> Option<DraftOffer> {
    if m.is_drafter {
        return None;
    }
    // Offer only when every known drafters/ folder is missing the file.
    let dirs = drafter_dirs(config_dir);
    let d = dirs.iter().find_map(|dir| ld::offer(m.file, dir))?;
    // If any folder already has it, ld::offer on that folder is None and we
    // would still see Some from an empty sibling — require all dirs to agree.
    if dirs.iter().any(|dir| dir.join(d.file).is_file()) {
        return None;
    }
    let primary = drafters_dir(config_dir);
    let partial = md::partial_bytes(&primary, d);
    let button = match partial {
        Some(b) => format!(
            "Resume {} ({})",
            d.name,
            md::progress_line(b, Some(d.download_mb * MIB))
        ),
        None => format!(
            "Install and run draft model ({})",
            ls::size_label(d.download_mb)
        ),
    };
    Some(DraftOffer {
        id: d.id.into(),
        name: d.name.into(),
        size_label: ls::size_label(d.download_mb),
        line: format!(
            "Optional: {} can draft replies for {}, which can make them faster on some machines. {} only downloads it if you ask, and keeps it only if the speed test finds it helps.",
            d.name,
            m.name,
            grokhub_core::brand::product_name()
        ),
        button,
        target_id: m.id.into(),
        target_name: m.name.into(),
    })
}

/// Selected catalog id → draft offer when its paired drafter is not on disk.
pub fn draft_offer(config_dir: &Path, selected: &str) -> Option<DraftOffer> {
    offer_for(config_dir, ls::model_by_id(selected)?)
}

/// Every catalog chat model whose paired drafter is still missing.
pub fn draft_offers(config_dir: &Path) -> Vec<DraftOffer> {
    ls::local_models()
        .iter()
        .filter(|m| !m.is_drafter)
        .filter_map(|m| offer_for(config_dir, m))
        .collect()
}

pub fn engine_info(config_dir: &Path) -> EngineInfo {
    let ready = rd::engine_binary_available(config_dir);
    let st = state().lock().ok();
    let downloading = st.as_ref().is_some_and(|s| s.engine.is_some());
    let error = st.as_ref().and_then(|s| s.engine_error.clone());
    let progress = st.as_ref().and_then(|s| {
        s.engine.as_ref().map(|j| EngineProgress {
            done: j.done,
            total: j.total,
            verifying: j.verifying,
            line: if j.verifying {
                format!("Downloading engine {}: checking…", j.version)
            } else {
                format!(
                    "Downloading engine {}: {}",
                    j.version,
                    md::progress_line(j.done, j.total)
                )
            },
        })
    });

    if ready && !downloading {
        let version = rd::manifest_for_host()
            .map(|m| m.version)
            .unwrap_or_else(|| "installed".into());
        return EngineInfo {
            state: "ready".into(),
            version,
            size_label: String::new(),
            hint: format!(
                "llama-server is ready at {}",
                rd::server_path(config_dir).display()
            ),
            button: String::new(),
            progress: None,
            error: None,
        };
    }

    let Some(manifest) = rd::manifest_for_host() else {
        return EngineInfo {
            state: "noBuild".into(),
            version: String::new(),
            size_label: String::new(),
            hint: "Runtime missing: there's no llama.cpp build for this computer. Install llama-server and put it on PATH, or set GROKHUB_LLAMA_SERVER to its path.".into(),
            button: String::new(),
            progress: None,
            error,
        };
    };

    if downloading {
        return EngineInfo {
            state: "downloading".into(),
            version: manifest.version.clone(),
            size_label: ls::size_label(manifest.asset.size / MIB),
            hint: format!(
                "llama.cpp {} · from {}",
                manifest.version,
                manifest.asset.source_host().unwrap_or("github.com")
            ),
            button: "Cancel".into(),
            progress,
            error: None,
        };
    }

    let partial = rd::partial_bytes(&rd::bin_dir(config_dir), &manifest.asset);
    let button = match partial {
        Some(b) => format!(
            "Resume engine {} ({})",
            manifest.version,
            md::progress_line(b, Some(manifest.asset.size))
        ),
        None => format!(
            "Get engine {} ({})",
            manifest.version,
            ls::size_label(manifest.asset.size / MIB)
        ),
    };
    EngineInfo {
        state: "missing".into(),
        version: manifest.version.clone(),
        size_label: ls::size_label(manifest.asset.size / MIB),
        hint: format!(
            "llama.cpp {} · {} · from {}",
            manifest.version,
            ls::size_label(manifest.asset.size / MIB),
            manifest.asset.source_host().unwrap_or("github.com")
        ),
        button,
        progress: None,
        error,
    }
}

pub fn draft_progress() -> Option<DraftProgress> {
    let s = state().lock().ok()?;
    let j = s.drafter.as_ref()?;
    Some(DraftProgress {
        id: j.id.clone(),
        done: j.done,
        total: j.total,
        verifying: j.verifying,
        line: if j.verifying {
            format!("Checking {}…", j.name)
        } else {
            format!(
                "Downloading {}: {}",
                j.name,
                md::progress_line(j.done, j.total)
            )
        },
    })
}

/// The engine archive download; `download_engine` names each asset's URL.
struct HttpEngineFetch;

fn runtime_agent() -> grokhub_core::http::Agent {
    // Redirects are followed by hand so every hop is checked; public IPs only.
    grokhub_core::http::agent(grokhub_core::http::Opts {
        connect: Duration::from_secs(20),
        read: Some(Duration::from_secs(120)),
        redirects: 0,
        public_only: true,
        ..Default::default()
    })
}

fn redirect_target(from: &str, location: &str) -> String {
    if location.starts_with("https://") || location.starts_with("http://") {
        return location.to_string();
    }
    let (scheme, rest) = from.split_once("://").unwrap_or(("https", from));
    let host = rest.split('/').next().unwrap_or("");
    if location.starts_with('/') {
        format!("{scheme}://{host}{location}")
    } else {
        format!("{scheme}://{host}/{location}")
    }
}

fn is_redirect(status: u16) -> bool {
    matches!(status, 301 | 302 | 303 | 307 | 308)
}

impl rd::Fetch for HttpEngineFetch {
    fn open(&self, url: &str, offset: u64) -> Result<md::Opened, String> {
        let mut url = url.to_string();
        for hop in 0..=REDIRECTS_MAX {
            runtime_download_url_ok(&url)?;
            let mut req = runtime_agent()
                .get(&url)
                .header("user-agent", grokhub_agent::USER_AGENT);
            if hop == 0 && offset > 0 {
                req = req.header("range", &format!("bytes={offset}-"));
            }
            // With redirects: 0 a 3xx comes back as the answer, handled below.
            let resp = req.call().map_err(|e| e.to_string())?;
            let status = resp.status();
            if is_redirect(status) {
                let loc = resp.header("location").ok_or_else(|| {
                    format!("engine download redirect missing Location (HTTP {status})")
                })?;
                url = redirect_target(&url, loc);
                continue;
            }
            let (start, total) = if resp.status() == 206 {
                (
                    offset,
                    resp.header("content-range")
                        .and_then(md::content_range_total),
                )
            } else {
                (
                    0,
                    resp.header("content-length")
                        .and_then(|v| v.trim().parse().ok()),
                )
            };
            return Ok((Box::new(resp.into_reader()), start, total));
        }
        Err("too many redirects while downloading the engine".into())
    }
}

struct HfFetch;

/// Hosts a draft model download may touch: the Hub and its LFS CDN, which the
/// Hub redirects to (`cdn-lfs.hf.co`, `cdn-lfs-us-1.hf.co`, …).
const MODEL_DOWNLOAD_HOSTS: &[&str] = &["huggingface.co", "hf.co"];

/// Refuse non-HTTPS model downloads, loopback names, and hosts outside the
/// Hugging Face Hub. Checked again on every redirect hop.
fn model_download_url_ok(url: &str) -> Result<(), String> {
    let url = url.trim();
    let rest = url
        .strip_prefix("https://")
        .ok_or_else(|| "model download must use HTTPS".to_string())?;
    let host = rest
        .split(['/', '?', '#'])
        .next()
        .filter(|h| !h.is_empty())
        .ok_or_else(|| "model download URL is missing a host".to_string())?;
    let host = host.rsplit_once('@').map_or(host, |(_, h)| h);
    let host = host.rsplit_once(':').map_or(host, |(h, _)| h);
    let name = host.trim_end_matches('.').to_ascii_lowercase();
    if grokhub_core::ssrf::host_name_blocked(&name) {
        return Err(format!("model download host {host} is not allowed"));
    }
    let allowed = MODEL_DOWNLOAD_HOSTS
        .iter()
        .any(|h| name == *h || name.ends_with(&format!(".{h}")));
    if !allowed {
        return Err(format!(
            "model download host {host} is not on the allowlist"
        ));
    }
    Ok(())
}

/// Like [`runtime_agent`]: no automatic redirects, public addresses only.
/// A public-only agent never dials an env proxy (the proxy, not the Hub,
/// would reach the public-only resolver).
fn hf_agent() -> grokhub_core::http::Agent {
    grokhub_core::http::agent(grokhub_core::http::Opts {
        connect: Duration::from_secs(15),
        read: Some(Duration::from_secs(60)),
        redirects: 0,
        public_only: true,
        ..Default::default()
    })
}

/// GET `url`, following redirects by hand so every hop passes
/// [`model_download_url_ok`]. The Range header only goes to the first hop.
fn hf_get(url: &str, range: Option<&str>) -> Result<grokhub_core::http::Response, String> {
    let mut url = url.to_string();
    for hop in 0..=REDIRECTS_MAX {
        model_download_url_ok(&url)?;
        let mut req = hf_agent()
            .get(&url)
            .header("user-agent", grokhub_agent::USER_AGENT);
        if let (0, Some(range)) = (hop, range) {
            req = req.header("range", range);
        }
        // With redirects: 0 a 3xx comes back as the answer, handled below.
        let resp = req.call().map_err(|e| e.to_string())?;
        let status = resp.status();
        if !is_redirect(status) {
            return Ok(resp);
        }
        let loc = resp
            .header("location")
            .ok_or_else(|| format!("model download redirect missing Location (HTTP {status})"))?;
        url = redirect_target(&url, loc);
    }
    Err("too many redirects while downloading the model".into())
}

impl md::Fetch for HfFetch {
    fn open(&self, url: &str, offset: u64) -> Result<md::Opened, String> {
        let range = (offset > 0).then(|| format!("bytes={offset}-"));
        let resp = hf_get(url, range.as_deref())?;
        let (start, total) = if resp.status() == 206 {
            (
                offset,
                resp.header("content-range")
                    .and_then(md::content_range_total),
            )
        } else {
            (
                0,
                resp.header("content-length")
                    .and_then(|v| v.trim().parse().ok()),
            )
        };
        Ok((Box::new(resp.into_reader()), start, total))
    }

    fn sha256(&self, repo: &str, file: &str) -> Result<String, String> {
        let resp = hf_get(&md::tree_url(repo), None)?;
        let mut body = String::new();
        resp.into_reader()
            .read_to_string(&mut body)
            .map_err(|e| e.to_string())?;
        md::sha256_from_tree(&body, file).ok_or_else(|| format!("No SHA-256 for {file} in {repo}"))
    }
}

fn outcome_message(manifest: &Manifest, out: &EngineOutcome) -> String {
    match out {
        EngineOutcome::Done { bytes, server } => format!(
            "Installed llama.cpp {} ({}) at {}",
            manifest.version,
            ls::size_label(bytes / MIB),
            server.display()
        ),
        EngineOutcome::Paused { done } => format!(
            "Paused llama.cpp {} at {}. Resume picks up where it stopped.",
            manifest.version,
            md::progress_line(*done, Some(manifest.asset.size))
        ),
        EngineOutcome::Cancelled => format!("Cancelled the llama.cpp {} download.", manifest.version),
        EngineOutcome::BadChecksum => {
            "The engine download didn't match its published checksum, so it was deleted. Try Get engine again.".into()
        }
        EngineOutcome::Failed(e) => e.clone(),
    }
}

static ENGINE_BUSY: AtomicBool = AtomicBool::new(false);
static DRAFTER_BUSY: AtomicBool = AtomicBool::new(false);

/// One download at a time. The flag clears when the guard drops, so an early
/// error before the worker thread starts (a poisoned lock, say) never leaves
/// the button wedged on a job that isn't running.
struct Busy(&'static AtomicBool);

impl Busy {
    /// `None` when a download already holds the flag.
    fn claim(flag: &'static AtomicBool) -> Option<Self> {
        flag.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
            .then_some(Busy(flag))
    }
}

impl Drop for Busy {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

/// Records the engine job in `st` while `busy` is held.
fn claim_engine_job(
    st: &Mutex<State>,
    manifest: &Manifest,
    ctl: &Arc<Control>,
    done: u64,
) -> Result<(), String> {
    let mut s = st.lock().map_err(|e| e.to_string())?;
    s.engine_error = None;
    s.engine = Some(EngineJob {
        ctl: ctl.clone(),
        done,
        total: Some(manifest.asset.size),
        verifying: false,
        version: manifest.version.clone(),
    });
    Ok(())
}

/// Records the drafter job in `st` while `busy` is held.
fn claim_drafter_job(
    st: &Mutex<State>,
    d: &LocalModel,
    ctl: &Arc<Control>,
    done: u64,
) -> Result<(), String> {
    let mut s = st.lock().map_err(|e| e.to_string())?;
    s.drafter_error = None;
    s.drafter = Some(DrafterJob {
        ctl: ctl.clone(),
        id: d.id.into(),
        name: d.name.into(),
        done,
        total: Some(d.download_mb * MIB),
        verifying: false,
    });
    Ok(())
}

pub fn start_engine(app: AppHandle, config_dir: PathBuf) -> Result<EngineInfo, String> {
    if rd::engine_binary_available(&config_dir) {
        return Ok(engine_info(&config_dir));
    }
    let manifest = rd::manifest_for_host().ok_or_else(|| {
        "There's no llama.cpp build for this computer. Install llama-server and put it on PATH, or set GROKHUB_LLAMA_SERVER to its path.".to_string()
    })?;
    let Some(busy) = Busy::claim(&ENGINE_BUSY) else {
        return Ok(engine_info(&config_dir));
    };
    let ctl = Arc::new(Control::default());
    let done = rd::partial_bytes(&rd::bin_dir(&config_dir), &manifest.asset).unwrap_or(0);
    claim_engine_job(state(), &manifest, &ctl, done)?;
    let _ = app.emit(
        ENGINE_EVENT,
        EngineEvent {
            state: "downloading".into(),
            progress: Some(EngineProgress {
                done,
                total: Some(manifest.asset.size),
                verifying: false,
                line: format!(
                    "Downloading engine {}: {}",
                    manifest.version,
                    md::progress_line(done, Some(manifest.asset.size))
                ),
            }),
            error: None,
            message: None,
        },
    );
    let fetch = HttpEngineFetch;
    let man = manifest.clone();
    let cfg = config_dir.clone();
    let app2 = app.clone();
    std::thread::spawn(move || {
        let out = rd::download_engine(&cfg, &man, &fetch, &ctl, &mut |p| {
            if let Ok(mut s) = state().lock() {
                if let Some(j) = s.engine.as_mut() {
                    match p {
                        Progress::Bytes { done, total } => {
                            j.done = done;
                            if total.is_some() {
                                j.total = total;
                            }
                            j.verifying = false;
                        }
                        Progress::Verifying => j.verifying = true,
                    }
                }
            }
            let progress = state().lock().ok().and_then(|s| {
                s.engine.as_ref().map(|j| EngineProgress {
                    done: j.done,
                    total: j.total,
                    verifying: j.verifying,
                    line: if j.verifying {
                        format!("Downloading engine {}: checking…", j.version)
                    } else {
                        format!(
                            "Downloading engine {}: {}",
                            j.version,
                            md::progress_line(j.done, j.total)
                        )
                    },
                })
            });
            let _ = app2.emit(
                ENGINE_EVENT,
                EngineEvent {
                    state: "downloading".into(),
                    progress,
                    error: None,
                    message: None,
                },
            );
        });
        let message = outcome_message(&man, &out);
        let (state_name, error) = match &out {
            EngineOutcome::Done { .. } => {
                local_sidecar::stop_global();
                ("ready", None)
            }
            EngineOutcome::Failed(_) | EngineOutcome::BadChecksum => {
                ("missing", Some(message.clone()))
            }
            EngineOutcome::Paused { .. } | EngineOutcome::Cancelled => ("missing", None),
        };
        if let Ok(mut s) = state().lock() {
            s.engine = None;
            s.engine_error = error.clone();
        }
        drop(busy);
        let _ = app2.emit(
            ENGINE_EVENT,
            EngineEvent {
                state: state_name.into(),
                progress: None,
                error,
                message: Some(message),
            },
        );
    });
    Ok(engine_info(&config_dir))
}

pub fn cancel_engine() -> Result<(), String> {
    let s = state().lock().map_err(|e| e.to_string())?;
    if let Some(j) = &s.engine {
        j.ctl.cancel();
    }
    Ok(())
}

pub fn start_drafter(
    app: AppHandle,
    config_dir: PathBuf,
    target_id: String,
) -> Result<Option<DraftOffer>, String> {
    let m = ls::model_by_id(&target_id).ok_or_else(|| "Unknown model.".to_string())?;
    let d = offer_for(&config_dir, m)
        .and_then(|o| ls::model_by_id(&o.id))
        .ok_or_else(|| {
            "That draft model is already installed, or this model has no draft pair.".to_string()
        })?;
    let Some(busy) = Busy::claim(&DRAFTER_BUSY) else {
        return Ok(draft_offer(&config_dir, &target_id));
    };
    let dir = drafters_dir(&config_dir);
    let ctl = Arc::new(Control::default());
    let done = md::partial_bytes(&dir, d).unwrap_or(0);
    claim_drafter_job(state(), d, &ctl, done)?;
    let _ = app.emit(
        DRAFTER_EVENT,
        DrafterEvent {
            state: "downloading".into(),
            progress: draft_progress(),
            error: None,
            message: None,
        },
    );
    let model: &'static LocalModel = d;
    let app2 = app.clone();
    let target = target_id.clone();
    std::thread::spawn(move || {
        let out = md::download(&dir, model, &HfFetch, &ctl, &mut |p| {
            if let Ok(mut s) = state().lock() {
                if let Some(j) = s.drafter.as_mut() {
                    match p {
                        Progress::Bytes { done, total } => {
                            j.done = done;
                            if total.is_some() {
                                j.total = total;
                            }
                            j.verifying = false;
                        }
                        Progress::Verifying => j.verifying = true,
                    }
                }
            }
            let _ = app2.emit(
                DRAFTER_EVENT,
                DrafterEvent {
                    state: "downloading".into(),
                    progress: draft_progress(),
                    error: None,
                    message: None,
                },
            );
        });
        let (state_name, error, message) = match &out {
            md::Outcome::Done(_) => {
                local_sidecar::stop_global();
                (
                    "done",
                    None,
                    Some(format!(
                        "Downloaded {} as a draft model. Test speed in Settings checks whether it makes replies faster; it only stays on if it does. The next chat turn uses it once that test keeps it (or Manual draft is On).",
                        model.name
                    )),
                )
            }
            md::Outcome::Failed(e) => ("error", Some(e.clone()), Some(e.clone())),
            md::Outcome::BadChecksum => {
                let msg = format!(
                    "{} didn't match its published checksum, so it was deleted. Try the download again.",
                    model.name
                );
                ("error", Some(msg.clone()), Some(msg))
            }
            md::Outcome::Paused { done } => (
                "paused",
                None,
                Some(format!(
                    "Paused {} at {}. Resume picks up where it stopped.",
                    model.name,
                    md::progress_line(*done, Some(model.download_mb * MIB))
                )),
            ),
            md::Outcome::Cancelled => (
                "cancelled",
                None,
                Some(format!("Cancelled the {} download.", model.name)),
            ),
        };
        if let Ok(mut s) = state().lock() {
            s.drafter = None;
            s.drafter_error = error.clone();
        }
        drop(busy);
        let _ = app2.emit(
            DRAFTER_EVENT,
            DrafterEvent {
                state: state_name.into(),
                progress: None,
                error,
                message,
            },
        );
        let _ = target;
    });
    Ok(draft_offer(&config_dir, &target_id))
}

pub fn cancel_drafter() -> Result<(), String> {
    let s = state().lock().map_err(|e| e.to_string())?;
    if let Some(j) = &s.drafter {
        j.ctl.cancel();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_gemma_selected_offers_e2b_when_missing() {
        let dir = std::env::temp_dir().join(format!("gh-shell-offer-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(drafters_dir(&dir)).unwrap();
        let offer = draft_offer(&dir, "gemma-4-12b-it-abliterated-q4_k").expect("offer");
        assert_eq!(offer.id, "gemma-4-e2b-it-abliterated-q4_k");
        assert!(offer.button.starts_with("Install and run draft model"));
        assert!(offer
            .line
            .contains("Gemma 4 E2B can draft replies for Gemma 4 12B"));
        // A stock Gemma 4 id from the older catalog is offered nothing.
        assert!(draft_offer(&dir, "gemma-4-12b-it-q4_k_m").is_none());
        let e2b = ls::model_by_id("gemma-4-e2b-it-abliterated-q4_k").unwrap();
        let dest = drafters_dir(&dir);
        std::fs::write(dest.join(e2b.file), b"x").unwrap();
        assert!(draft_offer(&dir, "gemma-4-12b-it-abliterated-q4_k").is_none());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_draft_download_stays_on_the_hub_over_https() {
        assert_eq!(
            model_download_url_ok("https://huggingface.co/acme/test-GGUF/resolve/main/test.gguf"),
            Ok(())
        );
        assert_eq!(
            model_download_url_ok("https://cdn-lfs-us-1.hf.co/repos/ab/cd/ef?X-Amz=1"),
            Ok(())
        );
        assert_eq!(
            model_download_url_ok("https://HF.co./api/models/acme/x/tree/main"),
            Ok(())
        );
        assert_eq!(
            model_download_url_ok("http://huggingface.co/acme/x/resolve/main/x.gguf"),
            Err("model download must use HTTPS".into())
        );
        assert_eq!(
            model_download_url_ok("https://huggingface.co.evil.example/x.gguf"),
            Err("model download host huggingface.co.evil.example is not on the allowlist".into())
        );
        assert_eq!(
            model_download_url_ok("https://huggingface.co@evil.example/x.gguf"),
            Err("model download host evil.example is not on the allowlist".into())
        );
        assert_eq!(
            model_download_url_ok("https://localhost:8443/x.gguf"),
            Err("model download host localhost is not allowed".into())
        );
        assert_eq!(
            model_download_url_ok("https:///x.gguf"),
            Err("model download URL is missing a host".into())
        );
        assert_eq!(
            redirect_target(
                "https://huggingface.co/acme/x/resolve/main/x.gguf",
                "/acme/x/resolve/abc/x.gguf"
            ),
            "https://huggingface.co/acme/x/resolve/abc/x.gguf"
        );
    }

    #[test]
    fn the_busy_flag_clears_when_a_download_fails_to_start() {
        static FLAG: AtomicBool = AtomicBool::new(false);
        let first = Busy::claim(&FLAG).expect("free");
        assert!(Busy::claim(&FLAG).is_none(), "one download at a time");
        drop(first);
        assert!(!FLAG.load(Ordering::SeqCst));

        // A poisoned state lock fails the start; the flag must not stay set.
        let poisoned = Mutex::new(State {
            engine: None,
            engine_error: None,
            drafter: None,
            drafter_error: None,
        });
        std::thread::scope(|s| {
            let _ = s
                .spawn(|| {
                    let _held = poisoned.lock().unwrap();
                    panic!("poison the lock");
                })
                .join();
        });
        let d = ls::model_by_id("gemma-4-e2b-it-abliterated-q4_k").expect("catalog drafter");
        let started = (|| -> Result<(), String> {
            let _busy = Busy::claim(&FLAG).ok_or("busy")?;
            claim_drafter_job(&poisoned, d, &Arc::new(Control::default()), 0)
        })();
        assert_eq!(
            started,
            Err("poisoned lock: another task failed inside".into())
        );
        assert!(
            !FLAG.load(Ordering::SeqCst),
            "busy flag wedged after a failed start"
        );
        assert!(Busy::claim(&FLAG).is_some());
    }

    #[test]
    fn engine_info_reports_ready_when_binary_exists() {
        let dir = std::env::temp_dir().join(format!("gh-shell-eng-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let bin = rd::bin_dir(&dir);
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(rd::server_path(&dir), b"fake").unwrap();
        let info = engine_info(&dir);
        assert_eq!(info.state, "ready");
        assert!(info.button.is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }
}
