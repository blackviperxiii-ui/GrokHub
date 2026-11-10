//! How warm a load was: how much of the model file sat in the OS page cache
//! before the server started, and how long the first reply after a load took
//! to read its prompt. Both go into `local_health.jsonl`, so the preloader
//! and the KV snapshots can show what they saved on each device.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use serde_json::Value;

/// Percent of `path` resident in the page cache, or `None` where the OS can't
/// say (Windows and macOS for now) or the file can't be read. Mapping the
/// file reads nothing from disk, so the check doesn't warm it.
pub fn resident_pct(path: &Path) -> Option<u8> {
    #[cfg(target_os = "linux")]
    {
        linux::resident_pct(path)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = path;
        None
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use std::os::fd::AsRawFd;
    use std::path::Path;

    pub fn resident_pct(path: &Path) -> Option<u8> {
        let file = std::fs::File::open(path).ok()?;
        let len = usize::try_from(file.metadata().ok()?.len()).ok()?;
        if len == 0 {
            return None;
        }
        // SAFETY: sysconf has no preconditions.
        let page = usize::try_from(unsafe { libc::sysconf(libc::_SC_PAGESIZE) }).ok()?;
        if page == 0 {
            return None;
        }
        // SAFETY: a read-only shared map of an open file; the result is
        // checked against MAP_FAILED and unmapped below.
        let addr = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                len,
                libc::PROT_READ,
                libc::MAP_SHARED,
                file.as_raw_fd(),
                0,
            )
        };
        if addr == libc::MAP_FAILED {
            return None;
        }
        let pages = len.div_ceil(page);
        let mut vec = vec![0u8; pages];
        // SAFETY: `addr..addr+len` is the mapping made above and `vec` holds
        // one byte per page of it.
        let ok = unsafe { libc::mincore(addr, len, vec.as_mut_ptr()) } == 0;
        // SAFETY: unmaps exactly the mapping made above.
        unsafe { libc::munmap(addr, len) };
        if !ok {
            return None;
        }
        let resident = vec.iter().filter(|b| **b & 1 == 1).count();
        u8::try_from(resident * 100 / pages).ok()
    }
}

/// Set when a server finishes loading; the first chat reply after it logs
/// its prompt time and clears it.
static FIRST_PENDING: AtomicBool = AtomicBool::new(false);

/// A server just finished loading: time the next reply.
pub fn note_loaded() {
    FIRST_PENDING.store(true, Ordering::SeqCst);
}

/// The first reply after a load: `(prompt_ms, prompt tokens, tokens reused
/// from cache)`, once per load. Later replies return `None`.
pub fn take_first_reply(reply: &Value) -> Option<(u64, u64, u64)> {
    let t = &reply["timings"];
    let ms = t["prompt_ms"].as_f64()?;
    if !FIRST_PENDING.swap(false, Ordering::SeqCst) {
        return None;
    }
    Some((
        ms.max(0.0).round() as u64,
        t["prompt_n"].as_u64().unwrap_or(0),
        t["cache_n"].as_u64().unwrap_or(0),
    ))
}

/// The `start` line's detail: the file name plus how much of it was cached.
pub fn start_detail(name: Option<&str>, cached_pct: Option<u8>) -> Option<String> {
    match (name, cached_pct) {
        (Some(n), Some(p)) => Some(format!("{n}, {p}% cached in RAM")),
        (Some(n), None) => Some(n.to_string()),
        (None, Some(p)) => Some(format!("{p}% cached in RAM")),
        (None, None) => None,
    }
}

/// The `first_reply` line's detail.
pub fn first_reply_detail(prompt_n: u64, cache_n: u64) -> String {
    format!("prompt {prompt_n} tokens, {cache_n} from cache")
}

/// Shader and kernel caches for the engine, each only when the user hasn't
/// set it (`is_set`): a bigger Mesa cache (AMD and Intel on Linux) and the
/// largest CUDA JIT cache, so compiled GPU work survives between loads.
pub fn cache_env(is_set: impl Fn(&str) -> bool) -> Vec<(&'static str, &'static str)> {
    [
        ("MESA_SHADER_CACHE_MAX_SIZE", "8G"),
        ("CUDA_CACHE_MAXSIZE", "4294967296"),
    ]
    .into_iter()
    .filter(|(k, _)| !is_set(k))
    .collect()
}

/// Facts for the load mode choice.
#[derive(Debug, Clone, Copy)]
pub struct LoadFacts {
    /// Every layer goes to the GPU, so RAM keeps nothing of the model.
    pub whole_on_gpu: bool,
    pub mlock: bool,
    pub file_mb: u64,
    pub free_ram_mb: Option<u64>,
    pub cached_pct: Option<u8>,
    /// The engine takes `--load-mode dio`.
    pub engine_dio: bool,
}

/// RAM left free after a load before reading through the cache is skipped.
pub const DIO_SPARE_MB: u64 = 4 * 1024;

/// Load straight from disk (`--load-mode dio`), skipping the page cache: a
/// cold file going wholly to the GPU on a machine short of RAM, where a
/// cached copy would only push other programs out. Anything already cached,
/// partly offloaded, locked, or unknown keeps the engine's default.
pub fn direct_load(f: LoadFacts) -> bool {
    f.engine_dio
        && f.whole_on_gpu
        && !f.mlock
        && f.cached_pct.is_some_and(|p| p < 20)
        && f.free_ram_mb
            .is_some_and(|free| free < f.file_mb.saturating_add(DIO_SPARE_MB))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn only_the_first_reply_after_a_load_is_timed() {
        let reply = json!({"timings": {"prompt_ms": 412.6, "prompt_n": 2100, "cache_n": 0}});
        let _ = take_first_reply(&reply);
        assert_eq!(take_first_reply(&reply), None, "nothing pending");
        note_loaded();
        assert_eq!(
            take_first_reply(&json!({})),
            None,
            "no timings keeps it pending"
        );
        assert_eq!(take_first_reply(&reply), Some((413, 2100, 0)));
        assert_eq!(take_first_reply(&reply), None, "second reply is not timed");
    }

    #[test]
    fn details_read_plainly() {
        assert_eq!(
            start_detail(Some("m.gguf"), Some(87)).as_deref(),
            Some("m.gguf, 87% cached in RAM")
        );
        assert_eq!(
            start_detail(Some("m.gguf"), None).as_deref(),
            Some("m.gguf")
        );
        assert_eq!(start_detail(None, None), None);
        assert_eq!(
            first_reply_detail(2100, 40),
            "prompt 2100 tokens, 40 from cache"
        );
    }

    #[test]
    fn caches_the_user_set_are_left_alone() {
        assert_eq!(cache_env(|_| false).len(), 2);
        let only_cuda = cache_env(|k| k == "MESA_SHADER_CACHE_MAX_SIZE");
        assert_eq!(only_cuda, vec![("CUDA_CACHE_MAXSIZE", "4294967296")]);
    }

    #[test]
    fn only_a_cold_whole_gpu_load_on_tight_ram_skips_the_cache() {
        let tight = LoadFacts {
            whole_on_gpu: true,
            mlock: false,
            file_mb: 15_000,
            free_ram_mb: Some(12_000),
            cached_pct: Some(3),
            engine_dio: true,
        };
        assert!(direct_load(tight));
        assert!(!direct_load(LoadFacts {
            free_ram_mb: Some(15_000 + DIO_SPARE_MB),
            ..tight
        }));
        assert!(!direct_load(LoadFacts {
            cached_pct: Some(80),
            ..tight
        }));
        assert!(!direct_load(LoadFacts {
            cached_pct: None,
            ..tight
        }));
        assert!(!direct_load(LoadFacts {
            whole_on_gpu: false,
            ..tight
        }));
        assert!(!direct_load(LoadFacts {
            mlock: true,
            ..tight
        }));
        assert!(!direct_load(LoadFacts {
            engine_dio: false,
            ..tight
        }));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_file_just_written_reads_as_cached() {
        let dir = std::env::temp_dir().join(format!("gh-local-warm-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("tmp");
        let file = dir.join("m.gguf");
        std::fs::write(&file, vec![7u8; 64 * 1024]).expect("write");
        let pct = resident_pct(&file).expect("linux reports residency");
        assert!(pct > 50, "freshly written pages sit in the cache: {pct}%");
        assert_eq!(resident_pct(&dir.join("missing")), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
