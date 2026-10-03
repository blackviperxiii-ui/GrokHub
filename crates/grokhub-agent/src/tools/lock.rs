//! One mutex per workspace path. The map lock is not held while the file lock is.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

fn locks() -> &'static Mutex<HashMap<PathBuf, Arc<Mutex<()>>>> {
    static LOCKS: OnceLock<Mutex<HashMap<PathBuf, Arc<Mutex<()>>>>> = OnceLock::new();
    LOCKS.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn with_file_lock<T>(path: &Path, f: impl FnOnce() -> T) -> T {
    let mutex = {
        let mut map = locks().lock().unwrap_or_else(|err| err.into_inner());
        map.entry(path.to_path_buf())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone()
    };
    let _guard = mutex.lock().unwrap_or_else(|err| err.into_inner());
    f()
}
