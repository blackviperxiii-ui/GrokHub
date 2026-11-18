//! KV snapshots across reloads. The engine starts with `--slot-save-path`
//! (when the build has it); before a quiet unload the chat slot's cache is
//! saved, and right after the same model loads again with the same context
//! and cache settings it is restored, so the next chat skips reading its
//! system prompt and tools again. A restore the first reply didn't use is
//! deleted and that model stops restoring for the rest of the run.
//!
//! With no snapshot, a GPU load gets one tiny warm-up reply instead, so the
//! first real reply doesn't also pay for the engine's first GPU work.
//!
//! The files hold the cached conversation, so they stay on the device next
//! to the chat history and the folder is capped ([`CAP_BYTES`]).

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use super::local_autoconfig::LaunchArgs;
use super::local_health as health;

pub const FLAG: &str = "--slot-save-path";
const EXT: &str = "kv";
/// The folder's size cap; the oldest snapshots go first.
pub const CAP_BYTES: u64 = 4 * 1024 * 1024 * 1024;
const SAVE_TIMEOUT: Duration = Duration::from_secs(15);

/// Restored snapshot waiting on the first reply's verdict: its file and model.
static PENDING: Mutex<Option<(PathBuf, String)>> = Mutex::new(None);
/// Models whose restore the first reply didn't use, this run.
static NO_RESTORE: Mutex<BTreeSet<String>> = Mutex::new(BTreeSet::new());

/// The snapshot folder, next to the auto-tune data.
pub fn dir() -> Option<PathBuf> {
    super::local_autoconfig::tune_dir().map(|d| d.join("kv"))
}

/// The folder to pass as [`FLAG`] when `options` (the engine's) has it.
pub fn spawn_dir(options: &BTreeSet<String>) -> Option<PathBuf> {
    if !options.contains(FLAG) {
        return None;
    }
    let d = dir()?;
    std::fs::create_dir_all(&d).ok()?;
    Some(d)
}

fn model_name(model: &Path) -> String {
    model
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// The snapshot's file name: one per model file, engine build and the cache
/// settings a restore needs to match (context, KV type, flash attention,
/// slots). Layers on the GPU don't matter.
pub fn snapshot_name(model: &Path, model_len: u64, engine_len: u64, a: &LaunchArgs) -> String {
    let mut h = Sha256::new();
    h.update(model_name(model).as_bytes());
    h.update(model_len.to_le_bytes());
    h.update(engine_len.to_le_bytes());
    h.update(a.ctx.to_le_bytes());
    h.update(format!("{:?}|{}|{}", a.kv, a.flash_attn, a.run.seqs()).as_bytes());
    let hex: String = h.finalize()[..8]
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    format!("{hex}.{EXT}")
}

/// [`snapshot_name`] for a server started from `binary` and `model`.
fn name_for(binary: &Path, model: &Path, a: &LaunchArgs) -> Option<String> {
    let len = |p: &Path| std::fs::metadata(p).map(|m| m.len()).ok();
    Some(snapshot_name(model, len(model)?, len(binary)?, a))
}

fn slot_call(base_url: &str, token: &str, action: &str, file: &str) -> Result<Value, String> {
    let agent = grokhub_core::http::agent(grokhub_core::http::Opts {
        connect: Duration::from_secs(2),
        read: Some(SAVE_TIMEOUT),
        ..Default::default()
    });
    agent
        .post(&format!("{base_url}/slots/0?action={action}"))
        .header("Authorization", &format!("Bearer {token}"))
        .send_json(json!({ "filename": file }))
        .map_err(|e| format!("slot {action}: {e}"))?
        .into_json()
        .map_err(|e| format!("slot {action} reply: {e}"))
}

/// Before a quiet unload: save the chat slot's cache, then trim the folder.
pub fn save(base_url: &str, token: &str, binary: &Path, model: &Path, a: &LaunchArgs) {
    let (Some(dir), Some(name)) = (dir(), name_for(binary, model, a)) else {
        return;
    };
    if !super::local_flags::engine_options(binary).contains(FLAG) {
        return;
    }
    match slot_call(base_url, token, "save", &name) {
        Ok(v) if v["n_saved"].as_u64().unwrap_or(0) > 0 => {
            let ms = v["timings"]["save_ms"].as_f64().map(|m| m.round() as u64);
            let detail = format!("{}, {} tokens", model_name(model), v["n_saved"]);
            health::log_ms("kv_saved", Some(&detail), ms);
            prune(&dir, CAP_BYTES);
        }
        // An empty slot saved nothing worth keeping.
        Ok(_) => {
            let _ = std::fs::remove_file(dir.join(&name));
        }
        Err(_) => {}
    }
}

/// Right after a load, in the background: restore the snapshot for this
/// launch, or warm a GPU load with one tiny reply.
pub fn after_load(base_url: String, token: String, binary: PathBuf, model: PathBuf, a: LaunchArgs) {
    // Before the device profile is read there is nowhere to keep snapshots,
    // and nothing measured to warm for.
    if dir().is_none() {
        return;
    }
    let _ = std::thread::Builder::new()
        .name("model-warmup".into())
        .spawn(move || {
            if !restore(&base_url, &token, &binary, &model, &a) && !a.cpu_only && a.ngl > 0 {
                warm_up(&base_url, &token);
            }
        });
}

fn restore(base_url: &str, token: &str, binary: &Path, model: &Path, a: &LaunchArgs) -> bool {
    let (Some(dir), Some(name)) = (dir(), name_for(binary, model, a)) else {
        return false;
    };
    let file = dir.join(&name);
    let model = model_name(model);
    let skipped = NO_RESTORE
        .lock()
        .map(|s| s.contains(&model))
        .unwrap_or(true);
    if skipped || !file.is_file() || !super::local_flags::engine_options(binary).contains(FLAG) {
        return false;
    }
    match slot_call(base_url, token, "restore", &name) {
        Ok(v) if v["n_restored"].as_u64().unwrap_or(0) > 0 => {
            let ms = v["timings"]["restore_ms"]
                .as_f64()
                .map(|m| m.round() as u64);
            let detail = format!("{model}, {} tokens", v["n_restored"]);
            health::log_ms("kv_restored", Some(&detail), ms);
            *PENDING.lock().unwrap_or_else(|e| e.into_inner()) = Some((file, model));
            true
        }
        // Restored nothing: the file doesn't fit this launch.
        Ok(_) => {
            let _ = std::fs::remove_file(&file);
            false
        }
        // A busy slot or a slow server: try again next load.
        Err(_) => false,
    }
}

/// One reply of one token: the engine does its first GPU work now.
fn warm_up(base_url: &str, token: &str) {
    let agent = grokhub_core::http::agent(grokhub_core::http::Opts {
        connect: Duration::from_secs(2),
        read: Some(Duration::from_secs(60)),
        ..Default::default()
    });
    let started = std::time::Instant::now();
    let ok = agent
        .post(&format!("{base_url}/v1/chat/completions"))
        .header("Authorization", &format!("Bearer {token}"))
        .send_json(json!({
            "messages": [{"role": "user", "content": "Hi"}],
            "max_tokens": 1,
            "stream": false,
        }))
        .is_ok();
    if ok {
        let ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
        health::log_ms("warmup", None, Some(ms));
    }
}

/// The first reply after a restore: a restore it reused nothing from (the
/// prompt changed, or the model's cache can't be restored) is deleted, and
/// that model stops restoring this run. Returns whether it missed.
pub fn judge_restore(prompt_n: u64, cache_n: u64) -> bool {
    let Some((file, model)) = PENDING.lock().unwrap_or_else(|e| e.into_inner()).take() else {
        return false;
    };
    let missed = missed(prompt_n, cache_n);
    if missed {
        let _ = std::fs::remove_file(&file);
        if let Ok(mut s) = NO_RESTORE.lock() {
            s.insert(model.clone());
        }
        health::log("kv_restore_missed", Some(&model));
    }
    missed
}

/// A reply that read a prompt and took none of it from the cache.
pub fn missed(prompt_n: u64, cache_n: u64) -> bool {
    prompt_n > 0 && cache_n == 0
}

/// Delete the oldest snapshots in `dir` until it holds at most `cap` bytes.
pub fn prune(dir: &Path, cap: u64) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut files: Vec<(std::time::SystemTime, u64, PathBuf)> = entries
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == EXT))
        .filter_map(|e| {
            let m = e.metadata().ok()?;
            Some((m.modified().ok()?, m.len(), e.path()))
        })
        .collect();
    files.sort();
    let mut total: u64 = files.iter().map(|(_, l, _)| l).sum();
    for (_, len, path) in files {
        if total <= cap {
            break;
        }
        if std::fs::remove_file(&path).is_ok() {
            total -= len;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args() -> LaunchArgs {
        LaunchArgs {
            backend: grokhub_core::local_profile::Backend::Vulkan,
            ngl: 49,
            all_layers: 49,
            n_cpu_moe: None,
            ctx: 16_384,
            kv: super::super::local_autoconfig::KvType::Q8,
            threads: 8,
            threads_batch: None,
            flash_attn: true,
            mlock: false,
            tensor_split: None,
            draft: None,
            cpu_only: false,
            run: Default::default(),
        }
    }

    #[test]
    fn a_snapshot_is_named_for_its_model_engine_and_cache_settings() {
        let m = Path::new("/models/a.gguf");
        let a = args();
        let name = snapshot_name(m, 100, 7, &a);
        assert!(name.ends_with(".kv") && !name.contains('/'), "{name}");
        assert_eq!(name, snapshot_name(m, 100, 7, &a), "stable");
        let more_layers = LaunchArgs { ngl: 99, ..args() };
        assert_eq!(
            name,
            snapshot_name(m, 100, 7, &more_layers),
            "layers don't matter"
        );
        let other_ctx = LaunchArgs {
            ctx: 8_192,
            ..args()
        };
        assert_ne!(name, snapshot_name(m, 100, 7, &other_ctx));
        assert_ne!(name, snapshot_name(m, 100, 8, &a), "engine update");
        assert_ne!(name, snapshot_name(Path::new("/models/b.gguf"), 100, 7, &a));
    }

    #[test]
    fn a_restore_the_reply_used_nothing_from_is_dropped() {
        assert!(missed(2_000, 0));
        assert!(!missed(400, 1_600));
        assert!(!missed(0, 0), "nothing read, nothing to judge");
        let dir = std::env::temp_dir().join(format!("gh-kv-judge-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("x.kv");
        std::fs::write(&file, b"kv").unwrap();
        *PENDING.lock().unwrap() = Some((file.clone(), "judge.gguf".into()));
        assert!(judge_restore(2_000, 0));
        assert!(!file.exists(), "the missed snapshot is deleted");
        assert!(NO_RESTORE.lock().unwrap().contains("judge.gguf"));
        assert!(!judge_restore(2_000, 0), "judged once");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_folder_keeps_the_newest_snapshots_under_the_cap() {
        let dir = std::env::temp_dir().join(format!("gh-kv-prune-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for (i, name) in ["old.kv", "mid.kv", "new.kv"].iter().enumerate() {
            let p = dir.join(name);
            std::fs::write(&p, vec![0u8; 10]).unwrap();
            let t = std::time::SystemTime::UNIX_EPOCH + Duration::from_secs(1_000 + i as u64);
            std::fs::File::options()
                .write(true)
                .open(&p)
                .unwrap()
                .set_modified(t)
                .unwrap();
        }
        std::fs::write(dir.join("notes.txt"), vec![0u8; 50]).unwrap();
        prune(&dir, 20);
        assert!(!dir.join("old.kv").exists());
        assert!(dir.join("mid.kv").exists() && dir.join("new.kv").exists());
        assert!(dir.join("notes.txt").exists(), "only snapshots are trimmed");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
