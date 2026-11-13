//! The small local embedding model behind hybrid memory recall (HX-12).
//!
//! Recall works without it: word search (BM25 and a stemmer) is the base.
//! When `potion-base-8M` is on disk and passes its pins, recall also finds
//! notes by meaning ("which GPU do I have" finds "graphics card is an RTX
//! 4080"). The model is fetched once in the background, about 30 MB from
//! Hugging Face, through the egress guard and never while Offline only is on.
//! A failed fetch waits a day before the next try. `"memory_embeddings":
//! false` in `app.json` turns all of it off: no fetch, word search only.
//! The auto router's nearest-neighbour memory ([`crate::route::knn`]) uses
//! the same model and the same switch.
//!
//! Each file is checked against its pin before it is kept, and again before
//! it loads, so a swapped or truncated file never reaches recall.

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use grokhub_core::static_embed::{self, EmbedModel, StaticModel, POTION_BASE_8M};

/// The model recall uses.
pub const MODEL: EmbedModel = POTION_BASE_8M;
/// `app.json` switch; anything but `false` leaves it on.
pub const SETTING: &str = "memory_embeddings";
/// Seconds a failed fetch waits before the next try.
pub const RETRY_SECS: i64 = 24 * 60 * 60;
const FAILED_STAMP: &str = "fetch-failed";

/// The loaded model, keyed by the config dir it came from.
static LOADED: Mutex<Option<(PathBuf, Arc<StaticModel>)>> = Mutex::new(None);
static FETCHING: AtomicBool = AtomicBool::new(false);

/// Whether `app.json` in `dir` leaves embeddings on.
pub fn enabled_at(dir: &Path) -> bool {
    let Ok(text) = fs::read_to_string(dir.join("app.json")) else {
        return true;
    };
    serde_json::from_str::<serde_json::Value>(&text)
        .ok()
        .and_then(|v| v.get(SETTING).and_then(|b| b.as_bool()))
        != Some(false)
}

/// The model for recall right now, or `None` while it isn't on disk (a
/// background fetch is started then, at most one at a time).
pub fn embedder() -> Option<Arc<StaticModel>> {
    #[cfg(test)]
    if let Some(model) = TEST_MODEL.with(|m| m.borrow().clone()) {
        return Some(model);
    }
    // Tests never reach the network: they set a model with `set_test_model`.
    embedder_at(&crate::perm::config_dir(), !cfg!(test))
}

#[cfg(test)]
thread_local! {
    static TEST_MODEL: std::cell::RefCell<Option<Arc<StaticModel>>> =
        const { std::cell::RefCell::new(None) };
}

/// The model [`embedder`] returns on this test thread (`None` clears it).
#[cfg(test)]
pub(crate) fn set_test_model(model: Option<Arc<StaticModel>>) {
    TEST_MODEL.with(|m| *m.borrow_mut() = model);
}

pub(crate) fn embedder_at(config_dir: &Path, fetch: bool) -> Option<Arc<StaticModel>> {
    if !enabled_at(config_dir) {
        return None;
    }
    let mut slot = LOADED.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((dir, model)) = slot.as_ref()
        && dir == config_dir
    {
        return Some(model.clone());
    }
    let model_dir = MODEL.dir(config_dir);
    if MODEL.present(&model_dir) {
        match StaticModel::load(&MODEL, &model_dir) {
            Ok(model) => {
                let model = Arc::new(model);
                *slot = Some((config_dir.to_path_buf(), model.clone()));
                return Some(model);
            }
            // A file that fails its pin goes, so the next fetch replaces it.
            Err(_) => {
                for file in MODEL.files {
                    let path = model_dir.join(file.name);
                    if !static_embed::verify_path(file, &path) {
                        let _ = fs::remove_file(path);
                    }
                }
            }
        }
    }
    drop(slot);
    if fetch {
        start_fetch(config_dir);
    }
    None
}

/// Where each file comes from. The app reads Hugging Face; tests a map.
pub(crate) trait Source {
    fn get(&self, url: &str, max_bytes: u64) -> Result<Vec<u8>, String>;
}

/// Fetch every missing or bad file of [`MODEL`] into `dir`, checking each
/// against its pin before it is renamed into place.
pub(crate) fn fetch_into(dir: &Path, source: &dyn Source) -> Result<(), String> {
    grokhub_core::offline::refuse()?;
    fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    for file in MODEL.files {
        let path = dir.join(file.name);
        if static_embed::verify_path(file, &path) {
            continue;
        }
        let bytes = source.get(&MODEL.url(file), file.bytes)?;
        if !static_embed::verify(file, &bytes) {
            return Err(format!(
                "{} from Hugging Face didn't match the pinned file, so it wasn't kept",
                file.name
            ));
        }
        let part = dir.join(format!("{}.part", file.name));
        fs::write(&part, &bytes).map_err(|e| e.to_string())?;
        fs::rename(&part, &path).map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn stamp_path(config_dir: &Path) -> PathBuf {
    MODEL.dir(config_dir).join(FAILED_STAMP)
}

/// True when a fetch failed less than [`RETRY_SECS`] before `now`.
pub(crate) fn waiting_after_failure(config_dir: &Path, now: i64) -> bool {
    fs::read_to_string(stamp_path(config_dir))
        .ok()
        .and_then(|t| t.trim().parse::<i64>().ok())
        .is_some_and(|at| now.saturating_sub(at) < RETRY_SECS)
}

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn start_fetch(config_dir: &Path) {
    if grokhub_core::offline::enabled() || waiting_after_failure(config_dir, now_secs()) {
        return;
    }
    if FETCHING.swap(true, Ordering::SeqCst) {
        return;
    }
    let config_dir = config_dir.to_path_buf();
    let spawned = std::thread::Builder::new()
        .name("memory-embed-fetch".into())
        .spawn(move || {
            let source = HuggingFace {
                config_dir: config_dir.clone(),
            };
            let result = fetch_into(&MODEL.dir(&config_dir), &source);
            let stamp = stamp_path(&config_dir);
            match result {
                Ok(()) => {
                    let _ = fs::remove_file(stamp);
                }
                Err(_) => {
                    if let Some(parent) = stamp.parent() {
                        let _ = fs::create_dir_all(parent);
                    }
                    let _ = fs::write(stamp, now_secs().to_string());
                }
            }
            FETCHING.store(false, Ordering::SeqCst);
        });
    if spawned.is_err() {
        FETCHING.store(false, Ordering::SeqCst);
    }
}

struct HuggingFace {
    config_dir: PathBuf,
}

impl Source for HuggingFace {
    fn get(&self, url: &str, max_bytes: u64) -> Result<Vec<u8>, String> {
        crate::harness::guard_quiet(&self.config_dir, &crate::harness::EgressReq::new(url, &[]))?;
        // Hugging Face answers a file with a redirect to its CDN; each hop
        // dials public addresses only (and nothing while Offline only is on).
        let agent = grokhub_core::http::agent(grokhub_core::http::Opts {
            overall: Some(Duration::from_secs(300)),
            redirects: 6,
            public_only: true,
            ..Default::default()
        });
        let response = agent
            .get(url)
            .header("user-agent", grokhub_core::brand::product_name())
            .call()
            .map_err(|e| format!("embedding model: {e}"))?;
        let mut bytes = Vec::new();
        response
            .into_reader()
            .take(max_bytes + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| format!("embedding model: {e}"))?;
        Ok(bytes)
    }
}

/// A file of [`MODEL`] by name (tests).
#[cfg(test)]
pub(crate) fn model_file(name: &str) -> &'static static_embed::ModelFile {
    MODEL
        .files
        .iter()
        .find(|f| f.name == name)
        .expect("pinned file")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    struct Fake(HashMap<String, Vec<u8>>);

    impl Source for Fake {
        fn get(&self, url: &str, max_bytes: u64) -> Result<Vec<u8>, String> {
            let bytes = self.0.get(url).cloned().ok_or("404")?;
            Ok(bytes.into_iter().take(max_bytes as usize + 1).collect())
        }
    }

    fn tmp(tag: &str) -> PathBuf {
        let n = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!("gh-embed-{tag}-{}-{n}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_file_that_misses_its_pin_is_never_kept() {
        let dir = tmp("pin");
        let vocab = model_file(static_embed::VOCAB_FILE);
        let fake = Fake(HashMap::from([(
            MODEL.url(vocab),
            vec![b'x'; vocab.bytes as usize],
        )]));
        let err = fetch_into(&dir, &fake).unwrap_err();
        assert!(err.contains("didn't match the pinned file"), "{err}");
        assert!(!dir.join(vocab.name).exists());
        assert!(!dir.join(format!("{}.part", vocab.name)).exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_missing_file_fails_the_fetch_and_nothing_loads() {
        let dir = tmp("missing");
        assert_eq!(fetch_into(&dir, &Fake(HashMap::new())).unwrap_err(), "404");
        assert!(!MODEL.present(&dir));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_setting_turns_embeddings_off_and_a_missing_model_loads_nothing() {
        let cfg = tmp("setting");
        assert!(enabled_at(&cfg));
        assert!(embedder_at(&cfg, false).is_none());
        fs::write(cfg.join("app.json"), "{\"memory_embeddings\":false}\n").unwrap();
        assert!(!enabled_at(&cfg));
        fs::write(cfg.join("app.json"), "{\"memory_embeddings\":true}\n").unwrap();
        assert!(enabled_at(&cfg));
        fs::write(cfg.join("app.json"), "not json").unwrap();
        assert!(enabled_at(&cfg));
        let _ = fs::remove_dir_all(&cfg);
    }

    #[test]
    fn a_failed_fetch_waits_a_day() {
        let cfg = tmp("stamp");
        assert!(!waiting_after_failure(&cfg, 1_000));
        fs::create_dir_all(MODEL.dir(&cfg)).unwrap();
        fs::write(stamp_path(&cfg), "1000").unwrap();
        assert!(waiting_after_failure(&cfg, 1_000 + RETRY_SECS - 1));
        assert!(!waiting_after_failure(&cfg, 1_000 + RETRY_SECS));
        let _ = fs::remove_dir_all(&cfg);
    }

    #[test]
    fn the_fetch_dials_through_the_guard_and_public_only() {
        let src = include_str!("memory_embed.rs");
        let arm = &src[src.find("impl Source for HuggingFace").unwrap()..];
        let arm = &arm[..arm.find("#[cfg(test)]").unwrap()];
        let guard = arm.find("guard_quiet(").unwrap();
        let dial = arm.find("http::agent(").unwrap();
        assert!(guard < dial, "the egress guard runs before the dial");
        assert!(arm.contains("public_only: true"));
        assert!(!arm.contains("proxy_from_env"));
    }
}
