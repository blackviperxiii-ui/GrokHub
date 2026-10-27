//! `{config}/models/local_health.jsonl`: what the on-device runtime and the
//! connection did, one small JSON line each, so "how did my offline test go"
//! has facts. `health.jsonl` next to it is the router's queue of cloud calls
//! and is emptied each time it is folded into the registry, so these go in
//! their own file. Lines carry no prompts, answers or keys, and the home
//! folder is written as `~`.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

pub const FILE: &str = "local_health.jsonl";
/// Past this size the log moves to `local_health.jsonl.1` and starts over.
const MAX_BYTES: u64 = 256 * 1024;
const DETAIL_MAX: usize = 240;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Event {
    pub ts_ms: u64,
    pub event: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    /// Load time for `start`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ms: Option<u64>,
}

pub fn path(config_dir: &Path) -> PathBuf {
    grokhub_core::model_registry::store::models_dir(config_dir).join(FILE)
}

/// The event name for a failed start: `missing_binary`, `missing_model`,
/// `oom`, `gpu_backend` or `start_failed`.
pub fn start_failure(err: &str) -> &'static str {
    let e = err.to_ascii_lowercase();
    if e.contains("runtime missing") || e.contains("binary missing") {
        "missing_binary"
    } else if e.contains("no model") || e.contains("model file missing") {
        "missing_model"
    } else if e.contains("out of memory") || e.contains("outofdevicememory") {
        "oom"
    } else if ["vulkan", "cuda", "metal", "ggml_", "gpu"]
        .iter()
        .any(|k| e.contains(k))
    {
        "gpu_backend"
    } else {
        "start_failed"
    }
}

/// Short, one line, home folder as `~`.
fn tidy(detail: &str) -> String {
    let mut text = detail.replace(['\n', '\r'], " ");
    if let Some(home) = grokhub_core::paths::user_home() {
        let home = home.display().to_string();
        if !home.is_empty() {
            text = text.replace(&home, "~");
        }
    }
    if text.chars().count() > DETAIL_MAX {
        text = text.chars().take(DETAIL_MAX).collect::<String>() + "…";
    }
    text
}

static LAST: Mutex<Option<(PathBuf, String, Option<String>)>> = Mutex::new(None);

/// Failures asked about on every request: the same one twice in a row is written once.
const FAILURES: &[&str] = &[
    "missing_binary",
    "missing_model",
    "oom",
    "gpu_backend",
    "start_failed",
];

/// Append one event under `config_dir`.
pub fn log_to(config_dir: &Path, event: &str, detail: Option<&str>, ms: Option<u64>) {
    let detail = detail.map(tidy);
    {
        let mut last = LAST.lock().unwrap_or_else(|e| e.into_inner());
        let key = (config_dir.to_path_buf(), event.to_string(), detail.clone());
        if FAILURES.contains(&event) && last.as_ref() == Some(&key) {
            return;
        }
        *last = Some(key);
    }
    let file = path(config_dir);
    if std::fs::metadata(&file).is_ok_and(|m| m.len() > MAX_BYTES) {
        let _ = std::fs::rename(&file, file.with_extension("jsonl.1"));
    }
    let line = Event {
        ts_ms: grokhub_core::now_ms(),
        event: event.into(),
        detail,
        ms,
    };
    let _ = grokhub_core::model_registry::store::append_line(&file, &line);
}

/// [`log_to`] the cabin's config folder. Tests write nothing here; they call
/// [`log_to`] with their own folder.
pub fn log(event: &str, detail: Option<&str>) {
    #[cfg(not(test))]
    log_to(&crate::perm::config_dir(), event, detail, None);
    #[cfg(test)]
    let _ = (event, detail);
}

/// The events in `config_dir`'s log, oldest first (bad lines skipped).
pub fn read(config_dir: &Path) -> Vec<Event> {
    std::fs::read_to_string(path(config_dir))
        .unwrap_or_default()
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn start_failures_get_a_name_a_person_can_search_for() {
        assert_eq!(
            start_failure("Runtime missing: llama-server isn't installed."),
            "missing_binary"
        );
        assert_eq!(
            start_failure("Runtime ready, no model yet: pick or download one below."),
            "missing_model"
        );
        assert_eq!(
            start_failure("the local model ran out of memory even with fewer GPU layers."),
            "oom"
        );
        assert_eq!(
            start_failure("ggml_vulkan: Failed to create device"),
            "gpu_backend"
        );
        assert_eq!(
            start_failure("local model server stopped before it was ready"),
            "start_failed"
        );
    }

    #[test]
    fn events_append_once_each_and_read_back_without_the_home_folder() {
        let dir = std::env::temp_dir().join(format!("gh-local-health-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let home = grokhub_core::paths::user_home()
            .map(|h| h.display().to_string())
            .unwrap_or_default();
        let missing = format!("local model binary missing: {home}/bin/llama-server");
        log_to(&dir, "missing_binary", Some(&missing), None);
        // Asked again on the next request: not written twice.
        log_to(&dir, "missing_binary", Some(&missing), None);
        log_to(&dir, "start", Some("tiny.gguf"), Some(840));
        log_to(&dir, "start", Some("tiny.gguf"), Some(790));
        log_to(&dir, "net", Some("offline"), None);
        let got: Vec<(String, Option<String>, Option<u64>)> = read(&dir)
            .into_iter()
            .map(|e| (e.event, e.detail, e.ms))
            .collect();
        let shown = if home.is_empty() {
            missing.clone()
        } else {
            "local model binary missing: ~/bin/llama-server".to_string()
        };
        assert_eq!(
            got,
            vec![
                ("missing_binary".into(), Some(shown), None),
                ("start".into(), Some("tiny.gguf".into()), Some(840)),
                ("start".into(), Some("tiny.gguf".into()), Some(790)),
                ("net".into(), Some("offline".into()), None),
            ]
        );
        let text = std::fs::read_to_string(path(&dir)).unwrap();
        assert!(text.lines().all(|l| l.starts_with("{\"ts_ms\":")), "{text}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
