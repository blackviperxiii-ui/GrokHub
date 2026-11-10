//! RAM-warm the model files the next load will read: one background thread
//! reads them into the OS page cache, so a load after a game, after an idle
//! unload or at app start reads from RAM instead of disk. Nothing loads, no
//! memory is locked, and the read is low priority, paced, and stops when the
//! machine is short on memory or busy with disk.
//!
//! While a game runs only memory nothing else is using counts as room, so
//! warming never pushes the game's own files out of the cache.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use super::local_health as health;

/// Room left over after a warmed file, so the cache never crowds the OS.
pub const SPARE_MB: u64 = 8 * 1024;
const CHUNK: usize = 8 << 20;
/// Read rates: plenty for a fast load, never a disk hog.
const RATE_MB_S: u64 = 500;
const GAME_RATE_MB_S: u64 = 100;
/// A warmed file is looked at again after this long (the cache may have
/// dropped it); one still cached is left alone (see `local_warm`).
pub const REWARM_SECS: u64 = 30 * 60;
/// Pressure readings (`some avg10`, percent) past which reading waits or stops.
const IO_BUSY_PCT: f64 = 10.0;
const MEM_BUSY_PCT: f64 = 2.0;
/// A file this much in the cache already is left alone.
const CACHED_PCT: u8 = 90;
/// Waits on a busy disk before giving the file up.
const IO_WAITS: u32 = 30;

static BUSY: AtomicBool = AtomicBool::new(false);
/// Files warmed: path, size, when (unix seconds).
static WARMED: Mutex<Vec<(PathBuf, u64, u64)>> = Mutex::new(Vec::new());

/// A file of `file_mb` fits in `free_mb` with [`SPARE_MB`] to spare.
pub fn room_for(file_mb: u64, free_mb: u64) -> bool {
    free_mb >= file_mb.saturating_add(SPARE_MB)
}

/// How long to wait so `bytes` read in `elapsed` stay under `rate_mb_s`.
pub fn pace(bytes: u64, elapsed: Duration, rate_mb_s: u64) -> Duration {
    let rate = rate_mb_s.max(1) * 1024 * 1024;
    let due = Duration::from_secs_f64(bytes as f64 / rate as f64);
    due.saturating_sub(elapsed)
}

/// `some avg10` from a `/proc/pressure/*` file.
pub fn pressure_avg10(text: &str) -> Option<f64> {
    let line = text.lines().find(|l| l.starts_with("some "))?;
    line.split_whitespace()
        .find_map(|f| f.strip_prefix("avg10="))?
        .parse()
        .ok()
}

/// `path` (now `len` bytes) needs reading: never warmed, changed since, or
/// warmed longer than [`REWARM_SECS`] ago.
pub fn due(warmed: &[(PathBuf, u64, u64)], path: &Path, len: u64, now_secs: u64) -> bool {
    !warmed
        .iter()
        .any(|(p, l, at)| p == path && *l == len && now_secs.saturating_sub(*at) < REWARM_SECS)
}

/// A warm-up thread is reading.
pub fn busy() -> bool {
    BUSY.load(Ordering::SeqCst)
}

fn now_secs() -> u64 {
    grokhub_core::now_ms() / 1000
}

/// Read the `files` that are [`due`] on one background thread. `gaming`
/// paces it slower and counts only unused memory as room; `fresh` (a usual
/// chat time is near) looks at every file however recently it was warmed.
/// A file still cached is skipped either way. Returns whether a thread
/// started.
pub fn start(files: Vec<PathBuf>, gaming: bool, fresh: bool) -> bool {
    let now = now_secs();
    let files: Vec<PathBuf> = {
        let warmed = WARMED.lock().unwrap_or_else(|e| e.into_inner());
        let mut seen = Vec::new();
        for f in files {
            let len = std::fs::metadata(&f).map_or(0, |m| m.len());
            if len > 0 && !seen.contains(&f) && (fresh || due(&warmed, &f, len, now)) {
                seen.push(f);
            }
        }
        // Already in the cache: nothing to read.
        seen.retain(|f| !super::local_warm::resident_pct(f).is_some_and(|p| p >= CACHED_PCT));
        seen
    };
    if files.is_empty() || BUSY.swap(true, Ordering::SeqCst) {
        return false;
    }
    let spawned = std::thread::Builder::new()
        .name("model-prewarm".into())
        .spawn(move || {
            lower_priority();
            for f in files {
                warm_one(&f, gaming);
            }
            BUSY.store(false, Ordering::SeqCst);
        });
    if spawned.is_err() {
        BUSY.store(false, Ordering::SeqCst);
        return false;
    }
    true
}

fn warm_one(path: &Path, gaming: bool) {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let began = Instant::now();
    match read_into_cache(path, gaming, free_mb) {
        Ok(len) => {
            let at = now_secs();
            let mut warmed = WARMED.lock().unwrap_or_else(|e| e.into_inner());
            warmed.retain(|(p, _, _)| p != path);
            warmed.push((path.to_path_buf(), len, at));
            let ms = u64::try_from(began.elapsed().as_millis()).unwrap_or(u64::MAX);
            let detail = format!("{name}, {} MB into RAM", len / (1024 * 1024));
            health::log_ms("prewarm", Some(&detail), Some(ms));
        }
        Err(why) => health::log("prewarm_skipped", Some(&format!("{name}: {why}"))),
    }
}

/// Read `path` through the cache in paced chunks, with `free` saying how
/// much memory is free. Returns its size.
fn read_into_cache(
    path: &Path,
    gaming: bool,
    free: fn(bool) -> Option<u64>,
) -> Result<u64, &'static str> {
    let mut file = std::fs::File::open(path).map_err(|_| "can't open the file")?;
    let len = file.metadata().map_err(|_| "can't read the file")?.len();
    let file_mb = len / (1024 * 1024);
    if !free(gaming).is_some_and(|f| room_for(file_mb, f)) {
        return Err("not enough free memory");
    }
    let rate = if gaming { GAME_RATE_MB_S } else { RATE_MB_S };
    let mut buf = vec![0u8; CHUNK];
    let mut read: u64 = 0;
    let began = Instant::now();
    let mut chunks: u32 = 0;
    loop {
        let n = file.read(&mut buf).map_err(|_| "read failed")?;
        if n == 0 {
            return Ok(len);
        }
        read += n as u64;
        chunks += 1;
        std::thread::sleep(pace(read, began.elapsed(), rate));
        if chunks.is_multiple_of(8) {
            wait_for_room(gaming, free)?;
        }
    }
}

/// Between chunks: stop when memory runs short, wait while the disk is busy.
fn wait_for_room(gaming: bool, free: fn(bool) -> Option<u64>) -> Result<(), &'static str> {
    for _ in 0..IO_WAITS {
        if pressure("memory").is_some_and(|p| p > MEM_BUSY_PCT)
            || free(gaming).is_some_and(|f| f < SPARE_MB / 2)
        {
            return Err("memory got busy");
        }
        if !pressure("io").is_some_and(|p| p > IO_BUSY_PCT) {
            return Ok(());
        }
        std::thread::sleep(Duration::from_secs(2));
    }
    Err("disk stayed busy")
}

#[cfg(target_os = "linux")]
fn pressure(kind: &str) -> Option<f64> {
    pressure_avg10(&std::fs::read_to_string(format!("/proc/pressure/{kind}")).ok()?)
}

#[cfg(not(target_os = "linux"))]
fn pressure(_kind: &str) -> Option<f64> {
    None
}

/// Free memory for warming. Linux while gaming: only `MemFree`, so nothing
/// the game has cached is pushed out. Otherwise what the planner sees free.
fn free_mb(gaming: bool) -> Option<u64> {
    #[cfg(target_os = "linux")]
    if gaming {
        let text = std::fs::read_to_string("/proc/meminfo").ok()?;
        return grokhub_core::local_profile::parse_meminfo_mb(&text, "MemFree:");
    }
    let _ = gaming;
    super::local_autoconfig::free_now().ram_free_mb
}

/// This thread's disk reads and cached pages go to the back of the line.
fn lower_priority() {
    #[cfg(target_os = "linux")]
    {
        const IOPRIO_WHO_PROCESS: libc::c_long = 1;
        const IOPRIO_CLASS_IDLE: libc::c_long = 3;
        // SAFETY: ioprio_set on the calling thread (who 0) only changes its
        // own I/O class; a failure leaves the default.
        unsafe {
            libc::syscall(
                libc::SYS_ioprio_set,
                IOPRIO_WHO_PROCESS,
                0 as libc::c_long,
                IOPRIO_CLASS_IDLE << 13,
            );
            libc::nice(19);
        }
    }
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::Threading::{
            GetCurrentThread, SetThreadPriority, THREAD_MODE_BACKGROUND_BEGIN,
        };
        // SAFETY: background mode on the calling thread lowers its CPU, I/O
        // and memory priority; a failure leaves the default.
        unsafe {
            SetThreadPriority(GetCurrentThread(), THREAD_MODE_BACKGROUND_BEGIN);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_needs_eight_gigabytes_to_spare() {
        assert!(room_for(15_000, 15_000 + SPARE_MB));
        assert!(!room_for(15_000, 15_000 + SPARE_MB - 1));
        assert!(!room_for(4_000, 8_000), "an 8 GB machine warms nothing big");
    }

    #[test]
    fn reads_are_paced_to_the_rate() {
        let mb = 1024 * 1024;
        assert_eq!(
            pace(100 * mb, Duration::from_millis(100), 100),
            Duration::from_millis(900)
        );
        assert_eq!(pace(10 * mb, Duration::from_secs(1), 100), Duration::ZERO);
    }

    #[test]
    fn pressure_reads_some_avg10() {
        let text = "some avg10=12.50 avg60=3.00 avg300=1.00 total=99\nfull avg10=1.00 avg60=0.00 avg300=0.00 total=5\n";
        assert_eq!(pressure_avg10(text), Some(12.5));
        assert_eq!(pressure_avg10("full avg10=1.00"), None);
    }

    #[test]
    fn a_file_reads_through_only_with_room() {
        let dir = std::env::temp_dir().join(format!("gh-prewarm-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("tmp");
        let file = dir.join("m.gguf");
        // Under eight chunks, so the pressure check (the real machine's) never runs.
        std::fs::write(&file, vec![1u8; CHUNK * 6 + 5]).expect("write");
        let len = (CHUNK * 6 + 5) as u64;
        assert_eq!(
            read_into_cache(&file, false, |_| Some(SPARE_MB * 4)),
            Ok(len)
        );
        assert_eq!(
            read_into_cache(&file, false, |_| Some(SPARE_MB - 1)),
            Err("not enough free memory")
        );
        assert_eq!(
            read_into_cache(&file, true, |_| None),
            Err("not enough free memory"),
            "no reading means no warming"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_file_is_read_again_only_when_changed_or_stale() {
        let p = PathBuf::from("/m/a.gguf");
        let warmed = vec![(p.clone(), 100, 1_000)];
        assert!(!due(&warmed, &p, 100, 1_000 + REWARM_SECS - 1));
        assert!(due(&warmed, &p, 100, 1_000 + REWARM_SECS), "stale");
        assert!(due(&warmed, &p, 200, 1_001), "changed size");
        assert!(
            due(&warmed, Path::new("/m/b.gguf"), 100, 1_001),
            "never warmed"
        );
    }
}
