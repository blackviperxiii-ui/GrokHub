//! Round 3 item 28: exact request replay. Every model request body is kept,
//! sealed, by content hash, so a turn can be sent again byte for byte in the
//! eval, or read back to see why a reply went wrong.
//!
//! A request is split into parts before it is stored: the tool list is one
//! part and each input item is one part. A chat's stable prefix (system
//! prompt, tools, earlier turns) is then stored once per chat, not once per
//! call. A small manifest, stored under the hash of the exact bytes sent,
//! names the parts; [`load_request`] puts them back and checks that hash.
//!
//! Everything here is personal text, so it goes through the learned-tier key
//! like consent and egress: one sealed line per file, no plaintext fallback.
//! No key means nothing is stored. Parts a request hasn't used for
//! [`KEEP_DAYS`] are removed, and the whole store stays under [`CAP_BYTES`].
//! `request_replay.toml` with `enabled = false` turns it off.

use std::cell::RefCell;
use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime};

use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use super::at_rest::{self, LearnedKey};

/// The switch file.
pub const CONFIG_NAME: &str = "request_replay.toml";
/// Associated data for every replay file, so a sealed line can't be moved here.
pub const AAD_REPLAY: &str = "grokhub:replay:v1";
/// Folder under the config dir.
pub const DIR: &str = "replay";
/// Days a part is kept after the last request that used it.
pub const KEEP_DAYS: u64 = 14;
/// The store never grows past this; the least recently used parts go first.
pub const CAP_BYTES: u64 = 256 * 1024 * 1024;
const SWEEP_EVERY: Duration = Duration::from_secs(60 * 60);
/// A part seen again is touched at most this often, so its age means "last used".
const TOUCH_EVERY: Duration = Duration::from_secs(6 * 60 * 60);
const KNOWN_CAP: usize = 8192;
const MANIFEST_VERSION: u64 = 1;

/// What `request_replay.toml` says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Settings {
    pub enabled: bool,
    pub keep_days: u64,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            enabled: true,
            keep_days: KEEP_DAYS,
        }
    }
}

pub fn settings(config_dir: &Path) -> Settings {
    let Ok(text) = fs::read_to_string(config_dir.join(CONFIG_NAME)) else {
        return Settings::default();
    };
    let file = crate::config_file::parse(CONFIG_NAME, &text, &["enabled", "keep_days"]);
    Settings {
        enabled: file.flag("enabled", true),
        keep_days: file.count("keep_days", 1, KEEP_DAYS as usize) as u64,
    }
}

thread_local! {
    /// The hash of the last request this thread stored, for its span.
    static LAST: RefCell<String> = const { RefCell::new(String::new()) };
}

/// Forget this thread's last request, before a call that may not send one.
pub fn clear_last() {
    LAST.with(|l| l.borrow_mut().clear());
}

/// The last request this thread stored, cleared as it is read.
pub fn take_last() -> String {
    LAST.with(|l| std::mem::take(&mut *l.borrow_mut()))
}

/// The last request this thread stored, left in place.
pub fn last_request() -> String {
    LAST.with(|l| l.borrow().clone())
}

fn digest(bytes: &[u8]) -> String {
    let d = Sha256::digest(bytes);
    d.iter().map(|b| format!("{b:02x}")).collect()
}

/// The hash a request is filed under: the SHA-256 of the exact bytes sent.
pub fn request_hash(body: &Value) -> String {
    digest(&serde_json::to_vec(body).unwrap_or_default())
}

fn store_dir(config_dir: &Path) -> PathBuf {
    config_dir.join(DIR)
}

fn part_path(config_dir: &Path, hash: &str) -> PathBuf {
    let shard = hash.get(..2).unwrap_or("00");
    store_dir(config_dir).join(shard).join(hash)
}

fn is_hash(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}

fn known() -> &'static Mutex<HashMap<PathBuf, Instant>> {
    static KNOWN: OnceLock<Mutex<HashMap<PathBuf, Instant>>> = OnceLock::new();
    KNOWN.get_or_init(Default::default)
}

/// Write one part unless it is there; touch it when it is (at most every
/// [`TOUCH_EVERY`]), so the sweep sees when a request last used it.
fn put_part(key: &LearnedKey, path: &Path, bytes: &[u8]) -> Result<(), String> {
    {
        let known = known().lock().unwrap_or_else(|e| e.into_inner());
        if known.get(path).is_some_and(|at| at.elapsed() < TOUCH_EVERY) {
            return Ok(());
        }
    }
    if path.exists() {
        if let Ok(f) = OpenOptions::new().append(true).open(path) {
            let _ = f.set_modified(SystemTime::now());
        }
    } else {
        let line = at_rest::seal_with(key, AAD_REPLAY, bytes)?;
        let dir = path.parent().ok_or("no parent dir")?;
        fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("part");
        let tmp = dir.join(format!(".{name}.{}.tmp", std::process::id()));
        let mut opts = OpenOptions::new();
        opts.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        let mut f = opts.open(&tmp).map_err(|e| e.to_string())?;
        f.write_all(line.as_bytes()).map_err(|e| e.to_string())?;
        drop(f);
        fs::rename(&tmp, path).map_err(|e| {
            let _ = fs::remove_file(&tmp);
            e.to_string()
        })?;
    }
    let mut known = known().lock().unwrap_or_else(|e| e.into_inner());
    if known.len() >= KNOWN_CAP {
        known.clear();
    }
    known.insert(path.to_path_buf(), Instant::now());
    Ok(())
}

fn store_part(key: &LearnedKey, config_dir: &Path, value: &Value) -> Result<String, String> {
    let bytes = serde_json::to_vec(value).map_err(|e| e.to_string())?;
    let hash = digest(&bytes);
    put_part(key, &part_path(config_dir, &hash), &bytes)?;
    Ok(hash)
}

/// Store `body` (exactly as it will be sent) and remember its hash for this
/// thread's span. Returns the hash, or `None` when the store is off or the
/// learned-tier key isn't there (nothing is written then).
pub fn note_request(config_dir: &Path, body: &Value) -> Option<String> {
    let _lap = crate::timing::lap("replay:note");
    clear_last();
    let set = settings(config_dir);
    if !set.enabled {
        return None;
    }
    let key = at_rest::write_key(config_dir).ok()?;
    let hash = store(&key, config_dir, body).ok()?;
    LAST.with(|l| *l.borrow_mut() = hash.clone());
    sweep_now_and_then(config_dir, set.keep_days);
    Some(hash)
}

fn store(key: &LearnedKey, config_dir: &Path, body: &Value) -> Result<String, String> {
    let hash = request_hash(body);
    let path = part_path(config_dir, &hash);
    let mut shell = body.clone();
    let mut parts = serde_json::Map::new();
    if let Some(items) = body.get("input").and_then(Value::as_array) {
        let hashes = items
            .iter()
            .map(|item| store_part(key, config_dir, item).map(Value::String))
            .collect::<Result<Vec<_>, _>>()?;
        parts.insert("input".into(), Value::Array(hashes));
        shell["input"] = Value::Null;
    }
    if let Some(tools) = body.get("tools").filter(|t| t.is_array()) {
        parts.insert(
            "tools".into(),
            Value::String(store_part(key, config_dir, tools)?),
        );
        shell["tools"] = Value::Null;
    }
    let manifest = json!({"v": MANIFEST_VERSION, "body": shell, "parts": parts});
    let bytes = serde_json::to_vec(&manifest).map_err(|e| e.to_string())?;
    put_part(key, &path, &bytes)?;
    Ok(hash)
}

fn open_part(key: &LearnedKey, config_dir: &Path, hash: &str) -> Result<Value, String> {
    if !is_hash(hash) {
        return Err(format!("`{hash}` isn't a request hash"));
    }
    let text = fs::read_to_string(part_path(config_dir, hash))
        .map_err(|_| format!("part {hash} is gone (aged out or never stored)"))?;
    let plain = at_rest::open_with(key, AAD_REPLAY, text.trim())
        .ok_or_else(|| format!("part {hash} doesn't open with this computer's key"))?;
    serde_json::from_slice(&plain).map_err(|e| e.to_string())
}

/// The exact bytes of a stored request, checked against its hash.
pub fn load_request(config_dir: &Path, hash: &str) -> Result<Vec<u8>, String> {
    let key = match at_rest::read_key(config_dir, true) {
        Some(Ok(Some(k))) => k,
        Some(Ok(None)) => return Err("nothing is stored on this computer yet".into()),
        Some(Err(why)) => return Err(format!("the store is locked: {why:?}")),
        None => return Err("the keyring didn't answer".into()),
    };
    let manifest = open_part(&key, config_dir, hash)?;
    if manifest["v"].as_u64() != Some(MANIFEST_VERSION) {
        return Err(format!("{hash} isn't a stored request"));
    }
    let mut body = manifest["body"].clone();
    let parts = &manifest["parts"];
    if let Some(items) = parts["input"].as_array() {
        let mut input = Vec::with_capacity(items.len());
        for h in items {
            input.push(open_part(&key, config_dir, h.as_str().unwrap_or(""))?);
        }
        body["input"] = Value::Array(input);
    }
    if let Some(h) = parts["tools"].as_str() {
        body["tools"] = open_part(&key, config_dir, h)?;
    }
    let bytes = serde_json::to_vec(&body).map_err(|e| e.to_string())?;
    if digest(&bytes) != hash {
        return Err(format!("{hash} rebuilt to different bytes"));
    }
    Ok(bytes)
}

fn sweep_now_and_then(config_dir: &Path, keep_days: u64) {
    static LAST_SWEEP: Mutex<Option<Instant>> = Mutex::new(None);
    {
        let mut last = LAST_SWEEP.lock().unwrap_or_else(|e| e.into_inner());
        if last.is_some_and(|at| at.elapsed() < SWEEP_EVERY) {
            return;
        }
        *last = Some(Instant::now());
    }
    let _ = sweep(config_dir, keep_days, CAP_BYTES, SystemTime::now());
}

/// Remove parts unused for `keep_days`, then the least recently used ones
/// until the store fits `cap`. Returns how many files went. A manifest can
/// outlive a part it names; [`load_request`] says so instead of guessing.
pub fn sweep(config_dir: &Path, keep_days: u64, cap: u64, now: SystemTime) -> usize {
    let _lap = crate::timing::lap("replay:sweep");
    let keep = Duration::from_secs(keep_days * 24 * 60 * 60);
    let mut files: Vec<(SystemTime, u64, PathBuf)> = Vec::new();
    let Ok(shards) = fs::read_dir(store_dir(config_dir)) else {
        return 0;
    };
    for shard in shards.flatten() {
        let Ok(entries) = fs::read_dir(shard.path()) else {
            continue;
        };
        for e in entries.flatten() {
            let Ok(meta) = e.metadata() else { continue };
            if meta.is_file() {
                let at = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
                files.push((at, meta.len(), e.path()));
            }
        }
    }
    files.sort();
    let mut total: u64 = files.iter().map(|f| f.1).sum();
    let mut removed = 0;
    let mut drop_known = Vec::new();
    for (at, len, path) in files {
        let old = now.duration_since(at).is_ok_and(|age| age > keep);
        if !old && total <= cap {
            break;
        }
        if fs::remove_file(&path).is_ok() {
            total = total.saturating_sub(len);
            removed += 1;
            drop_known.push(path);
        }
    }
    let mut known = known().lock().unwrap_or_else(|e| e.into_inner());
    for p in drop_known {
        known.remove(&p);
    }
    removed
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::harness::{use_key_store_for, MemoryKeyStore};

    fn keyed(label: &str) -> PathBuf {
        let dir = crate::harness::test_dir(label);
        use_key_store_for(&dir, Arc::new(MemoryKeyStore::new()));
        dir
    }

    fn files(dir: &Path) -> Vec<PathBuf> {
        let mut out = Vec::new();
        for shard in fs::read_dir(store_dir(dir)).into_iter().flatten().flatten() {
            for e in fs::read_dir(shard.path()).into_iter().flatten().flatten() {
                out.push(e.path());
            }
        }
        out.sort();
        out
    }

    fn body(turns: usize) -> Value {
        let mut input = vec![json!({"role": "system", "content": "You are Loadout."})];
        for i in 0..turns {
            input.push(json!({"role": "user", "content": format!("secret question {i}")}));
        }
        json!({
            "model": "grok-4.7",
            "stream": true,
            "input": input,
            "tools": [{"type": "function", "name": "read", "parameters": {"type": "object"}}],
            "reasoning": {"effort": "high"},
            "prompt_cache_key": "chat-1",
        })
    }

    #[test]
    fn a_request_comes_back_byte_for_byte_and_a_shared_prefix_is_stored_once() {
        let dir = keyed("replay-round-trip");
        let first = body(1);
        let hash = note_request(&dir, &first).expect("stored");
        assert_eq!(hash, request_hash(&first));
        assert_eq!(last_request(), hash);
        assert_eq!(
            load_request(&dir, &hash).unwrap(),
            serde_json::to_vec(&first).unwrap()
        );
        // system item, one user item, tools, manifest
        assert_eq!(files(&dir).len(), 4);

        let second = body(2);
        let next = note_request(&dir, &second).expect("stored");
        assert_ne!(next, hash);
        assert_eq!(
            load_request(&dir, &next).unwrap(),
            serde_json::to_vec(&second).unwrap()
        );
        // Only the new user item and the new manifest were added.
        assert_eq!(files(&dir).len(), 6);
        assert_eq!(take_last(), next);
        assert_eq!(last_request(), "");

        for f in files(&dir) {
            let raw = fs::read_to_string(&f).unwrap();
            assert!(raw.starts_with(at_rest::SEALED_PREFIX), "{raw}");
            assert!(!raw.contains("secret") && !raw.contains("Loadout"), "{raw}");
        }
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn nothing_is_stored_without_a_key_or_with_the_switch_off() {
        let locked = crate::harness::test_dir("replay-locked");
        use_key_store_for(&locked, Arc::new(MemoryKeyStore::unavailable()));
        assert_eq!(note_request(&locked, &body(1)), None);
        assert_eq!(last_request(), "");
        assert!(!store_dir(&locked).exists());

        let off = keyed("replay-off");
        fs::write(off.join(CONFIG_NAME), "enabled = false\n").unwrap();
        assert_eq!(note_request(&off, &body(1)), None);
        assert!(!store_dir(&off).exists());
        let _ = fs::remove_dir_all(locked);
        let _ = fs::remove_dir_all(off);
    }

    #[test]
    fn an_edited_or_missing_part_is_reported_not_guessed() {
        let dir = keyed("replay-damage");
        let b = body(1);
        let hash = note_request(&dir, &b).unwrap();
        let part = files(&dir)
            .into_iter()
            .find(|p| !p.ends_with(&hash))
            .unwrap();
        let mut raw = fs::read_to_string(&part).unwrap();
        raw.insert(raw.len() / 2, 'x');
        fs::write(&part, raw).unwrap();
        let err = load_request(&dir, &hash).unwrap_err();
        assert!(err.contains("doesn't open"), "{err}");
        fs::remove_file(&part).unwrap();
        let err = load_request(&dir, &hash).unwrap_err();
        assert!(err.contains("is gone"), "{err}");
        assert!(load_request(&dir, "../../etc")
            .unwrap_err()
            .contains("isn't a request hash"));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn parts_age_out_and_the_store_keeps_under_its_cap() {
        let dir = keyed("replay-sweep");
        let hash = note_request(&dir, &body(3)).unwrap();
        let all = files(&dir).len();
        let now = SystemTime::now();
        // Nothing is old yet and the store is far under the cap.
        assert_eq!(sweep(&dir, KEEP_DAYS, CAP_BYTES, now), 0);
        // Fifteen days on, every part is past the keep window.
        let later = now + Duration::from_secs(15 * 24 * 60 * 60);
        assert_eq!(sweep(&dir, KEEP_DAYS, CAP_BYTES, later), all);
        assert!(load_request(&dir, &hash).unwrap_err().contains("is gone"));
        // A cap of zero empties a fresh store too, oldest first.
        note_request(&dir, &body(1)).unwrap();
        assert!(sweep(&dir, KEEP_DAYS, 0, now) > 0);
        assert!(files(&dir).is_empty());
        let _ = fs::remove_dir_all(dir);
    }
}
