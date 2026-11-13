//! How warm a load was: how much of the model file sat in the OS page cache
//! before the server started, and how long the first reply after a load took
//! to read its prompt. Both go into `local_health.jsonl`, so the preloader
//! and the KV snapshots can show what they saved on each device.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use serde_json::Value;

/// Percent of `path` resident in the page cache, or `None` when the file
/// can't be read. Linux and macOS ask the kernel (`mincore` on a map, which
/// reads nothing from disk). Windows has no public call for this, so it
/// times a spread of small reads instead ([`windows`]); those warm about
/// half a megabyte of the file.
pub fn resident_pct(path: &Path) -> Option<u8> {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        unix::resident_pct(path)
    }
    #[cfg(windows)]
    {
        windows::resident_pct(path)
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    {
        let _ = path;
        None
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
mod unix {
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
        // one byte per page of it (macOS types the bytes as `c_char`).
        let ok = unsafe { libc::mincore(addr, len, vec.as_mut_ptr().cast()) } == 0;
        // SAFETY: unmaps exactly the mapping made above.
        unsafe { libc::munmap(addr, len) };
        if !ok {
            return None;
        }
        // Bit 0 is "in core" on both (MINCORE_INCORE on macOS).
        let resident = vec.iter().filter(|b| **b & 1 == 1).count();
        u8::try_from(resident * 100 / pages).ok()
    }
}

/// Reads this fast came from the cache: a cached 4 KiB read is a copy plus a
/// system call (a few microseconds), while even a fast NVMe drive takes tens
/// of microseconds for a random 4 KiB read that misses.
pub const CACHED_READ_US: u64 = 20;
/// Small reads spread over the file for the Windows estimate.
pub const SAMPLES: u64 = 128;

/// Percent of timed reads that came back at cache speed.
pub fn cached_share(read_us: &[u64]) -> Option<u8> {
    if read_us.is_empty() {
        return None;
    }
    let hits = read_us.iter().filter(|us| **us < CACHED_READ_US).count();
    u8::try_from(hits * 100 / read_us.len()).ok()
}

/// Where the `i`th of [`SAMPLES`] reads lands in a file of `len` bytes:
/// evenly spaced, each on a 4 KiB boundary.
pub fn sample_offset(i: u64, len: u64) -> u64 {
    (len / SAMPLES).saturating_mul(i) & !4095
}

#[cfg(windows)]
mod windows {
    use std::os::windows::fs::{FileExt, OpenOptionsExt};
    use std::path::Path;
    use std::time::Instant;

    /// `FILE_FLAG_RANDOM_ACCESS`: no read-ahead, so each sample reads only
    /// its own 4 KiB and a miss stays a miss.
    const RANDOM_ACCESS: u32 = 0x1000_0000;

    pub fn resident_pct(path: &Path) -> Option<u8> {
        let file = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(RANDOM_ACCESS)
            .open(path)
            .ok()?;
        let len = file.metadata().ok()?.len();
        if len < 4096 * super::SAMPLES {
            return None;
        }
        let mut buf = [0u8; 4096];
        let mut times = Vec::with_capacity(super::SAMPLES as usize);
        for i in 0..super::SAMPLES {
            let began = Instant::now();
            file.seek_read(&mut buf, super::sample_offset(i, len))
                .ok()?;
            times.push(u64::try_from(began.elapsed().as_micros()).unwrap_or(u64::MAX));
        }
        super::cached_share(&times)
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

    #[test]
    fn the_windows_estimate_counts_cache_speed_reads() {
        assert_eq!(cached_share(&[]), None);
        assert_eq!(cached_share(&[3, 4, 5, 6]), Some(100));
        assert_eq!(cached_share(&[3, 90, 4, 400]), Some(50));
        assert_eq!(cached_share(&[CACHED_READ_US, 2_000]), Some(0));
        let len = 7 * 1024 * 1024 * 1024 + 123;
        assert_eq!(sample_offset(0, len), 0);
        for i in 0..SAMPLES {
            let at = sample_offset(i, len);
            assert_eq!(at % 4096, 0);
            assert!(at + 4096 <= len);
        }
        assert!(sample_offset(SAMPLES - 1, len) > len / 2);
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn a_file_just_written_reads_as_cached() {
        let dir = std::env::temp_dir().join(format!("gh-local-warm-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("tmp");
        let file = dir.join("m.gguf");
        std::fs::write(&file, vec![7u8; 64 * 1024]).expect("write");
        let pct = resident_pct(&file).expect("the kernel reports residency");
        assert!(pct > 50, "freshly written pages sit in the cache: {pct}%");
        assert_eq!(resident_pct(&dir.join("missing")), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
