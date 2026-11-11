//! A locked read-modify-write for single keys of the desktop app's `app.json`.
//! The shell flips switches the cabin also reads (`focusHelp`) without
//! rewriting the rest of the file: unknown keys stay, two writers in this
//! process or in two processes don't lose each other's change, and a file
//! that can't be read as a JSON object is never overwritten.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, SystemTime};

use serde_json::{Map, Value};

pub const FILE_NAME: &str = "app.json";
const LOCK_NAME: &str = "app.json.lock";
/// A lock file older than this belongs to a writer that died.
const STALE_LOCK: Duration = Duration::from_secs(10);
const TRIES: u32 = 100;
const WAIT: Duration = Duration::from_millis(20);

static IN_PROCESS: Mutex<()> = Mutex::new(());

pub fn path(config_dir: &Path) -> PathBuf {
    config_dir.join(FILE_NAME)
}

/// The file's top-level object. A missing file is an empty object; anything
/// else that doesn't parse as an object is an error, so nothing writes over it.
pub fn read(config_dir: &Path) -> Result<Map<String, Value>, String> {
    let text = match std::fs::read_to_string(path(config_dir)) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Map::new()),
        Err(e) => return Err(format!("Couldn't read {FILE_NAME}: {e}")),
    };
    match serde_json::from_str::<Value>(&text) {
        Ok(Value::Object(map)) => Ok(map),
        _ => Err(format!(
            "{FILE_NAME} isn't readable, so nothing was changed."
        )),
    }
}

/// A true/false key; absent or any other type reads as `default`.
pub fn bool_or(config_dir: &Path, key: &str, default: bool) -> bool {
    read(config_dir)
        .ok()
        .and_then(|m| m.get(key).and_then(Value::as_bool))
        .unwrap_or(default)
}

struct FileLock(PathBuf);

impl Drop for FileLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn lock_file(config_dir: &Path) -> Result<FileLock, String> {
    std::fs::create_dir_all(config_dir).map_err(|e| e.to_string())?;
    let lock = config_dir.join(LOCK_NAME);
    for _ in 0..TRIES {
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&lock)
        {
            Ok(_) => return Ok(FileLock(lock)),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                let stale = std::fs::metadata(&lock)
                    .and_then(|m| m.modified())
                    .ok()
                    .and_then(|t| SystemTime::now().duration_since(t).ok())
                    .is_some_and(|age| age > STALE_LOCK);
                if stale {
                    let _ = std::fs::remove_file(&lock);
                } else {
                    std::thread::sleep(WAIT);
                }
            }
            Err(e) => return Err(e.to_string()),
        }
    }
    Err(format!("{FILE_NAME} is busy. Try again."))
}

/// Set each key, keeping every other key as it is.
pub fn patch(config_dir: &Path, changes: &[(&str, Value)]) -> Result<(), String> {
    let _in_process = IN_PROCESS.lock().unwrap_or_else(|e| e.into_inner());
    let _file = lock_file(config_dir)?;
    let mut map = read(config_dir)?;
    for (key, value) in changes {
        map.insert((*key).to_string(), value.clone());
    }
    let text = serde_json::to_string_pretty(&Value::Object(map)).map_err(|e| e.to_string())?;
    crate::pulse_writer::write_atomic(&path(config_dir), &text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "grokhub-app-json-{name}-{}-{}",
            std::process::id(),
            crate::now_ms()
        ));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn a_patch_keeps_unknown_keys_and_other_values() {
        let d = dir("keep");
        std::fs::write(
            path(&d),
            r#"{"quietStart":"23:00","futureKey":{"a":[1,2]},"focusHelp":false}"#,
        )
        .unwrap();
        patch(&d, &[("focusHelp", json!(true))]).unwrap();
        let map = read(&d).unwrap();
        assert_eq!(map["focusHelp"], json!(true));
        assert_eq!(map["quietStart"], json!("23:00"));
        assert_eq!(map["futureKey"], json!({"a": [1, 2]}));
        assert!(bool_or(&d, "focusHelp", false));
        assert!(!d.join(LOCK_NAME).exists(), "the lock is released");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_missing_file_is_created_with_just_the_key() {
        let d = dir("new");
        assert!(!bool_or(&d, "focusHelp", false));
        patch(&d, &[("focusHelp", json!(true))]).unwrap();
        assert_eq!(read(&d).unwrap().len(), 1);
        assert!(bool_or(&d, "focusHelp", false));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_broken_file_is_never_overwritten() {
        let d = dir("broken");
        for body in ["{not json", "[1,2]", "\"text\""] {
            std::fs::write(path(&d), body).unwrap();
            let err = patch(&d, &[("focusHelp", json!(true))]).unwrap_err();
            assert_eq!(err, "app.json isn't readable, so nothing was changed.");
            assert_eq!(std::fs::read_to_string(path(&d)).unwrap(), body);
        }
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn two_writers_do_not_lose_an_update() {
        let d = dir("race");
        let writers: Vec<_> = (0..4)
            .map(|w| {
                let d = d.clone();
                std::thread::spawn(move || {
                    for i in 0..10 {
                        patch(&d, &[(&format!("key{w}x{i}"), json!(i))]).unwrap();
                    }
                })
            })
            .collect();
        for w in writers {
            w.join().unwrap();
        }
        let map = read(&d).unwrap();
        assert_eq!(map.len(), 40, "every writer's keys survived");
        assert_eq!(map["key3x9"], json!(9));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_dead_writers_lock_does_not_block_forever() {
        let d = dir("stale");
        let lock = d.join(LOCK_NAME);
        std::fs::write(&lock, "").unwrap();
        let old = SystemTime::now() - Duration::from_secs(60);
        std::fs::File::options()
            .write(true)
            .open(&lock)
            .unwrap()
            .set_modified(old)
            .unwrap();
        patch(&d, &[("focusHelp", json!(true))]).unwrap();
        assert!(bool_or(&d, "focusHelp", false));
        let _ = std::fs::remove_dir_all(&d);
    }
}
