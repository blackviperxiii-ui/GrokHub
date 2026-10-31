//! The live game guard (rules in `grokhub_core::game_guard`). Every reader
//! here opens no handle to another process, so a game's anti-cheat has
//! nothing to log:
//!
//! - Windows: a Toolhelp32 snapshot (exe names and pids only), the Service
//!   Control Manager's list of running services and drivers, Steam's
//!   `RunningAppID`, the Epic launcher's manifests on disk, and the shell's
//!   notification state (exclusive fullscreen Direct3D).
//! - Linux: `/proc/*/comm` and `/proc/*/cmdline`.
//! - macOS and others: nothing yet, so the guard stays clear.
//!
//! [`current`] caches the reading for 2 s. [`peek`] never blocks: the UI
//! reads it every frame and a stale reading refreshes on a worker thread.
//! Refreshes run one at a time, and a reading is stamped with when it
//! started, so an older reading never replaces a newer one.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

#[cfg(any(target_os = "linux", windows))]
use grokhub_core::game_guard::{decide, scan_anticheats, GuardSignals};
pub use grokhub_core::game_guard::{GameGuard, Guarded};

/// How long one reading is reused.
const FRESH: Duration = Duration::from_secs(2);

/// One cached reading and the lock that lets one refresh run at a time.
struct Cache {
    /// The last reading and when its read started.
    last: Mutex<Option<(Instant, GameGuard)>>,
    /// Held while the readers run, so stale callers wait for one reading
    /// instead of each reading in parallel.
    refresh: Mutex<()>,
    /// A [`peek`] worker is on its way.
    refreshing: AtomicBool,
}

impl Cache {
    const fn new() -> Self {
        Self {
            last: Mutex::new(None),
            refresh: Mutex::new(()),
            refreshing: AtomicBool::new(false),
        }
    }

    fn cached(&self) -> Option<(Instant, GameGuard)> {
        *self.last.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn fresh(&self) -> Option<GameGuard> {
        self.cached()
            .filter(|(at, _)| at.elapsed() < FRESH)
            .map(|(_, g)| g)
    }

    /// Keep `guard`, read from `started` on, unless a reading that started
    /// later is already kept. Returns the reading kept.
    fn store(&self, started: Instant, guard: GameGuard) -> GameGuard {
        let mut last = self.last.lock().unwrap_or_else(|e| e.into_inner());
        match *last {
            Some((at, newer)) if at > started => newer,
            _ => {
                *last = Some((started, guard));
                guard
            }
        }
    }

    /// The fresh reading, or a new one. One refresh runs at a time; a caller
    /// that waited for it uses the reading it left.
    fn current(&self, read: impl FnOnce() -> GameGuard) -> GameGuard {
        if let Some(g) = self.fresh() {
            return g;
        }
        let _one = self.refresh.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(g) = self.fresh() {
            return g;
        }
        let started = Instant::now();
        self.store(started, read())
    }
}

static CACHE: Cache = Cache::new();

/// The guard now, read at most every 2 s. Blocks on the readers when the
/// last reading is stale (a few ms), so call it off the UI thread.
pub fn current() -> GameGuard {
    CACHE.current(read)
}

/// The last reading, without blocking. A stale one is refreshed on a worker
/// thread for the next call. Clear until the first reading lands.
pub fn peek() -> GameGuard {
    let last = CACHE.cached();
    if last.is_none_or(|(at, _)| at.elapsed() >= FRESH)
        && !CACHE.refreshing.swap(true, Ordering::SeqCst)
    {
        let spawned = std::thread::Builder::new()
            .name("game-guard".into())
            .spawn(|| {
                CACHE.current(read);
                CACHE.refreshing.store(false, Ordering::SeqCst);
            });
        if spawned.is_err() {
            CACHE.refreshing.store(false, Ordering::SeqCst);
        }
    }
    last.map(|(_, g)| g).unwrap_or_default()
}

/// The governor's deny list ("always counts as a game"), lowercase.
#[cfg(any(target_os = "linux", windows))]
fn deny_list() -> Vec<String> {
    let path = crate::host::config_dir()
        .join("local")
        .join("governor.json");
    std::fs::read_to_string(path)
        .map(|t| grokhub_core::local_governor::Rules::from_json(&t).deny)
        .unwrap_or_default()
        .into_iter()
        .map(|d| d.trim().to_ascii_lowercase())
        .filter(|d| !d.is_empty())
        .collect()
}

#[cfg(any(target_os = "linux", windows))]
fn listed(names: &[String], deny: &[String]) -> bool {
    names
        .iter()
        .any(|n| deny.iter().any(|d| n.eq_ignore_ascii_case(d)))
}

// ---------------------------------------------------------------- Linux

#[cfg(target_os = "linux")]
fn read() -> GameGuard {
    let procs = processes().unwrap_or_default();
    // Wine and Proton cut `comm` to 15 bytes, so the first argument's file
    // name is checked as well.
    let mut names: Vec<String> = Vec::with_capacity(procs.len() * 2);
    for (comm, cmdline) in &procs {
        names.push(comm.to_ascii_lowercase());
        if let Some(first) = cmdline.split_whitespace().next() {
            names.push(grokhub_core::game_guard::file_name_lower(first));
        }
    }
    let anticheats = scan_anticheats::<_, &str>(&names, &[]);
    decide(&GuardSignals {
        anticheat: anticheats.running,
        idle_anticheat: anticheats.idle,
        steam_app: grokhub_core::game_companion::running_from_cmdlines(&procs),
        game_process: grokhub_core::local_governor::game_in_processes(&procs),
        listed_game: listed(&names, &deny_list()),
        exclusive_fullscreen: false,
    })
}

/// Every process's `comm` and command line, from `/proc`. Reading these
/// opens no handle to the process and needs no ptrace access.
#[cfg(target_os = "linux")]
pub fn processes() -> Option<Vec<(String, String)>> {
    let entries = std::fs::read_dir("/proc").ok()?;
    Some(
        entries
            .flatten()
            .filter(|e| {
                e.file_name()
                    .to_string_lossy()
                    .bytes()
                    .all(|b| b.is_ascii_digit())
            })
            .filter_map(|e| {
                let comm = std::fs::read_to_string(e.path().join("comm"))
                    .ok()?
                    .trim()
                    .to_string();
                let cmdline = std::fs::read_to_string(e.path().join("cmdline"))
                    .unwrap_or_default()
                    .replace('\0', " ");
                Some((comm, cmdline))
            })
            .collect(),
    )
}

// ---------------------------------------------------------------- Windows

#[cfg(windows)]
fn read() -> GameGuard {
    let procs = process_names().unwrap_or_default();
    let names: Vec<String> = procs.into_iter().map(|(_, n)| n).collect();
    let services = running_services();
    let anticheats = scan_anticheats(&names, &services);
    decide(&GuardSignals {
        anticheat: anticheats.running,
        idle_anticheat: anticheats.idle,
        steam_app: steam_running_app(),
        game_process: grokhub_core::game_guard::windows_game_exe(&names, &epic_game_exes())
            .is_some(),
        listed_game: listed(&names, &deny_list()),
        exclusive_fullscreen: exclusive_fullscreen(),
    })
}

/// Whether a game runs, for the wind-down check: Steam's running game or a
/// known game exe. `None` when the process snapshot fails.
#[cfg(windows)]
pub fn windows_game() -> Option<bool> {
    let names: Vec<String> = process_names()?.into_iter().map(|(_, n)| n).collect();
    let exe = grokhub_core::game_guard::windows_game_exe(&names, &epic_game_exes());
    Some(exe.is_some() || steam_running_app().is_some())
}

/// The running game's label (exe name without `.exe`), for PC health.
#[cfg(windows)]
pub fn windows_game_label() -> Option<String> {
    let names: Vec<String> = process_names()?.into_iter().map(|(_, n)| n).collect();
    grokhub_core::game_guard::windows_game_exe(&names, &epic_game_exes())
        .map(|exe| grokhub_core::game_guard::game_label(&exe))
}

/// `(pid, exe file name)` of every process, from a Toolhelp32 snapshot.
/// The snapshot is a kernel copy of the process list: no process is opened.
#[cfg(windows)]
pub fn process_names() -> Option<Vec<(u32, String)>> {
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };
    // SAFETY: the snapshot handle is checked and closed here; the entry is a
    // zeroed local with dwSize set, as Process32FirstW/NextW require.
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snap == INVALID_HANDLE_VALUE || snap.is_null() {
            return None;
        }
        let mut entry: PROCESSENTRY32W = std::mem::zeroed();
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut out = Vec::new();
        let mut ok = Process32FirstW(snap, &mut entry) != 0;
        while ok {
            let len = entry
                .szExeFile
                .iter()
                .position(|&c| c == 0)
                .unwrap_or(entry.szExeFile.len());
            out.push((
                entry.th32ProcessID,
                String::from_utf16_lossy(&entry.szExeFile[..len]),
            ));
            ok = Process32NextW(snap, &mut entry) != 0;
        }
        CloseHandle(snap);
        Some(out)
    }
}

/// The exe file name (lowercase) of one process, looked up in a Toolhelp
/// snapshot instead of opening it.
#[cfg(windows)]
pub fn exe_name_of(pid: u32) -> Option<String> {
    process_names()?
        .into_iter()
        .find(|(p, _)| *p == pid)
        .map(|(_, n)| n.to_ascii_lowercase())
}

/// Names of the running services and kernel drivers, from the Service
/// Control Manager. This asks the SCM, never a game process.
#[cfg(windows)]
fn running_services() -> Vec<String> {
    use windows_sys::Win32::Foundation::{GetLastError, ERROR_MORE_DATA};
    use windows_sys::Win32::System::Services::{
        CloseServiceHandle, EnumServicesStatusExW, OpenSCManagerW, ENUM_SERVICE_STATUS_PROCESSW,
        SC_ENUM_PROCESS_INFO, SC_MANAGER_ENUMERATE_SERVICE, SERVICE_ACTIVE, SERVICE_DRIVER,
        SERVICE_WIN32,
    };
    let mut out = Vec::new();
    // SAFETY: the SCM handle is checked and closed here. The buffer is
    // u64-aligned and as long as `size` says; every name pointer the call
    // returns points into it and is read before the next call reuses it.
    unsafe {
        let scm = OpenSCManagerW(
            std::ptr::null(),
            std::ptr::null(),
            SC_MANAGER_ENUMERATE_SERVICE,
        );
        if scm.is_null() {
            return out;
        }
        let mut resume = 0u32;
        let mut buf: Vec<u64> = vec![0; 8 * 1024];
        for _ in 0..64 {
            let size = (buf.len() * 8) as u32;
            let (mut needed, mut count) = (0u32, 0u32);
            let ok = EnumServicesStatusExW(
                scm,
                SC_ENUM_PROCESS_INFO,
                SERVICE_WIN32 | SERVICE_DRIVER,
                SERVICE_ACTIVE,
                buf.as_mut_ptr().cast::<u8>(),
                size,
                &mut needed,
                &mut count,
                &mut resume,
                std::ptr::null(),
            ) != 0;
            let more = !ok && GetLastError() == ERROR_MORE_DATA;
            let rows = std::slice::from_raw_parts(
                buf.as_ptr().cast::<ENUM_SERVICE_STATUS_PROCESSW>(),
                count as usize,
            );
            for row in rows {
                out.push(wide_ptr(row.lpServiceName));
            }
            if !more {
                break;
            }
            if count == 0 {
                // Not even one row fits: grow the buffer to what was asked.
                let want = (needed as usize).div_ceil(8).max(buf.len() * 2);
                if want > 1 << 20 {
                    break;
                }
                buf = vec![0; want];
            }
        }
        CloseServiceHandle(scm);
    }
    out
}

/// A NUL-terminated UTF-16 string.
///
/// # Safety
/// `p` is null or points at a NUL-terminated UTF-16 string.
#[cfg(windows)]
unsafe fn wide_ptr(p: *const u16) -> String {
    if p.is_null() {
        return String::new();
    }
    let mut n = 0;
    while *p.add(n) != 0 {
        n += 1;
    }
    String::from_utf16_lossy(std::slice::from_raw_parts(p, n))
}

/// Steam's `HKCU\Software\Valve\Steam\RunningAppID`: the running game's app
/// id, `None` when no Steam game runs.
#[cfg(windows)]
pub fn steam_running_app() -> Option<u32> {
    use windows_sys::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};
    let wide = |s: &str| s.encode_utf16().chain(Some(0)).collect::<Vec<u16>>();
    let sub = wide(r"Software\Valve\Steam");
    let name = wide("RunningAppID");
    let mut data = 0u32;
    let mut len = std::mem::size_of::<u32>() as u32;
    // SAFETY: NUL-terminated strings and a DWORD out-buffer of `len` bytes.
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            sub.as_ptr(),
            name.as_ptr(),
            RRF_RT_REG_DWORD,
            std::ptr::null_mut(),
            &mut data as *mut u32 as *mut core::ffi::c_void,
            &mut len,
        )
    };
    (status == 0 && data != 0).then_some(data)
}

/// An exclusive fullscreen Direct3D app owns the screen (the shell's
/// `QUNS_RUNNING_D3D_FULL_SCREEN`). A video or browser in fullscreen is not.
#[cfg(windows)]
fn exclusive_fullscreen() -> bool {
    use windows_sys::Win32::UI::Shell::{
        SHQueryUserNotificationState, QUNS_RUNNING_D3D_FULL_SCREEN,
    };
    let mut state = 0;
    // SAFETY: an out-pointer to a local i32.
    let ok = unsafe { SHQueryUserNotificationState(&mut state) } >= 0;
    ok && state == QUNS_RUNNING_D3D_FULL_SCREEN
}

/// Game exes the Epic launcher installed, from its manifests on disk, read
/// at most once a minute.
#[cfg(windows)]
fn epic_game_exes() -> Vec<String> {
    static EPIC: Mutex<Option<(Instant, Vec<String>)>> = Mutex::new(None);
    let mut slot = EPIC.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((at, exes)) = slot.as_ref() {
        if at.elapsed() < Duration::from_secs(60) {
            return exes.clone();
        }
    }
    let dir = std::env::var_os("ProgramData")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from(r"C:\ProgramData"))
        .join(r"Epic\EpicGamesLauncher\Data\Manifests");
    let exes: Vec<String> = std::fs::read_dir(dir)
        .map(|it| {
            it.flatten()
                .filter(|e| e.path().extension().is_some_and(|x| x == "item"))
                .take(500)
                .filter_map(|e| std::fs::read_to_string(e.path()).ok())
                .filter_map(|t| grokhub_core::game_guard::epic_manifest_exe(&t))
                .collect()
        })
        .unwrap_or_default();
    *slot = Some((Instant::now(), exes.clone()));
    exes
}

// ---------------------------------------------------------------- other

#[cfg(not(any(target_os = "linux", windows)))]
fn read() -> GameGuard {
    GameGuard::default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn this_machine_reads_without_panicking_and_the_cache_answers_peek() {
        let g = current();
        // An anti-cheat always counts as gaming.
        assert!(g.anticheat.is_none() || g.gaming, "{g:?}");
        assert_eq!(peek(), current());
    }

    const FACEIT: GameGuard = GameGuard {
        anticheat: Some("FACEIT"),
        gaming: true,
        idle_anticheat: None,
    };

    #[test]
    fn an_older_reading_never_replaces_a_newer_one() {
        let cache = Cache::new();
        let older = Instant::now();
        let newer = older + Duration::from_millis(5);
        // The newer read (an anti-cheat) lands first; the slower, older
        // read that saw nothing lands after it and is dropped.
        assert_eq!(cache.store(newer, FACEIT), FACEIT);
        assert_eq!(cache.store(older, GameGuard::default()), FACEIT);
        assert_eq!(cache.cached(), Some((newer, FACEIT)));
        // A reading that started later still replaces it.
        let later = newer + Duration::from_millis(5);
        assert_eq!(
            cache.store(later, GameGuard::default()),
            GameGuard::default()
        );
    }

    #[test]
    fn stale_callers_wait_for_one_refresh_instead_of_reading_in_parallel() {
        use std::sync::atomic::AtomicUsize;
        use std::sync::mpsc;
        static SLOW: Cache = Cache::new();
        static SECOND_READS: AtomicUsize = AtomicUsize::new(0);
        let (started_tx, started_rx) = mpsc::channel::<()>();
        let (go_tx, go_rx) = mpsc::channel::<()>();
        let first = std::thread::spawn(move || {
            SLOW.current(move || {
                started_tx.send(()).unwrap();
                go_rx.recv().unwrap();
                FACEIT
            })
        });
        started_rx.recv().unwrap();
        // The first refresh is mid-read: a second stale caller waits for it
        // and takes its reading rather than running the readers again.
        let second = std::thread::spawn(|| {
            SLOW.current(|| {
                SECOND_READS.fetch_add(1, Ordering::SeqCst);
                GameGuard::default()
            })
        });
        go_tx.send(()).unwrap();
        assert_eq!(first.join().unwrap(), FACEIT);
        assert_eq!(second.join().unwrap(), FACEIT);
        assert_eq!(SECOND_READS.load(Ordering::SeqCst), 0);
    }

    #[cfg(any(target_os = "linux", windows))]
    #[test]
    fn deny_listed_names_match_in_any_case() {
        let deny = vec!["hades.exe".to_string()];
        assert!(listed(&["explorer.exe".into(), "Hades.exe".into()], &deny));
        assert!(!listed(&["explorer.exe".into()], &deny));
        assert!(!listed(&["hades.exe".into()], &[]));
    }
}
