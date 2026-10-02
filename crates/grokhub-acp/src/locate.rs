use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{mpsc, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

use crate::protocol::{PermissionMode, SessionMode};

/// `(env key, scanned at, grok path, refresh in flight)`.
type GrokBinCache = Option<(String, Instant, Option<PathBuf>, bool)>;
/// `(settings path, settings mtime, read at, api key, refresh in flight)`.
type GrokKeyCache = Option<(PathBuf, Option<std::time::SystemTime>, Instant, Option<String>, bool)>;
/// `(grok path, probed at, ok, doctor line, probe in flight)`.
type DoctorLineCache = Option<(Option<PathBuf>, Instant, bool, String, bool)>;

/// Resolve the Grok Build CLI. `GROKHUB_GROK` wins, then PATH, then common install dirs.
pub fn find_grok() -> Option<PathBuf> {
    let key = format!(
        "{:?}|{:?}",
        std::env::var_os("GROKHUB_GROK"),
        std::env::var_os("PATH")
    );
    if let Ok(held) = grok_bin_cache().lock() {
        if let Some((k, at, path, inflight)) = held.as_ref() {
            if *k == key {
                let hit = path.clone();
                let fresh = at.elapsed() < Duration::from_secs(2);
                let busy = *inflight;
                drop(held);
                if !fresh && !busy {
                    kick_find_grok(key);
                }
                return hit;
            }
        }
    }
    let path = find_grok_scan();
    if let Ok(mut held) = grok_bin_cache().lock() {
        *held = Some((key, Instant::now(), path.clone(), false));
    }
    path
}

fn grok_bin_cache() -> &'static Mutex<GrokBinCache> {
    static C: OnceLock<Mutex<GrokBinCache>> = OnceLock::new();
    C.get_or_init(|| Mutex::new(None))
}

/// Hide a Windows console for spawned CLI tools (`grok.exe`, powershell).
///
/// `grokhub.exe` is `windows_subsystem = "windows"`. Spawning a console-subsystem
/// binary without `CREATE_NO_WINDOW` allocates a visible terminal. Closing that
/// window kills the child with `STATUS_CONTROL_C_EXIT`.
///
/// Also silences the loader MessageBox (missing DLL / bad image) so a broken
/// `grok.exe` becomes one cabin error instead of a looping system dialog.
pub fn hide_windows_console(cmd: &mut Command) {
    silence_windows_hard_errors();
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let _ = cmd;
}

/// Process-wide: do not show Windows critical-error / missing-DLL dialogs.
/// Children inherit this. Safe to call from any thread, including Linux (no-op).
pub fn silence_windows_hard_errors() {
    #[cfg(windows)]
    {
        const SEM_FAILCRITICALERRORS: u32 = 0x0001;
        const SEM_NOGPFAULTERRORBOX: u32 = 0x0002;
        const SEM_NOOPENFILEERRORBOX: u32 = 0x8000;
        const MODE: u32 = SEM_FAILCRITICALERRORS | SEM_NOGPFAULTERRORBOX | SEM_NOOPENFILEERRORBOX;
        #[link(name = "kernel32")]
        extern "system" {
            fn SetErrorMode(u_mode: u32) -> u32;
            fn SetThreadErrorMode(dw_new_mode: u32, lp_old_mode: *mut u32) -> i32;
        }
        unsafe {
            SetErrorMode(MODE);
            let mut old = 0u32;
            let _ = SetThreadErrorMode(MODE, &mut old);
        }
    }
}

/// NTSTATUS / Win32 codes and spawn text that mean "do not spawn this grok again".
pub fn is_cli_hard_failure(code: Option<i32>, text: &str) -> bool {
    const HARD: &[u32] = &[
        0xC0000135, // STATUS_DLL_NOT_FOUND
        0xC0000138, // STATUS_ORDINAL_NOT_FOUND
        0xC0000139, // STATUS_ENTRYPOINT_NOT_FOUND
        0xC000007B, // STATUS_INVALID_IMAGE_FORMAT
        0xC0000142, // STATUS_DLL_INIT_FAILED
        0xC0000020, // STATUS_INVALID_FILE_FOR_SECTION
    ];
    if let Some(c) = code {
        let u = c as u32;
        if HARD.contains(&u) || c == 193 || c == 126 {
            return true;
        }
    }
    let t = text.to_ascii_lowercase();
    t.contains("dll was not found")
        || t.contains("dll not found")
        || t.contains("the specified module could not be found")
        || t.contains("not a valid win32")
        || t.contains("%1 is not a valid")
        || t.contains("code execution cannot proceed")
        || t.contains("status_dll_not_found")
}

fn grok_unusable_cache() -> &'static Mutex<Option<(PathBuf, String)>> {
    static C: OnceLock<Mutex<Option<(PathBuf, String)>>> = OnceLock::new();
    C.get_or_init(|| Mutex::new(None))
}

fn same_grok_path(a: &Path, b: &Path) -> bool {
    if cfg!(windows) {
        a.to_string_lossy().eq_ignore_ascii_case(&b.to_string_lossy())
    } else {
        a == b
    }
}

pub fn mark_grok_unusable(path: &Path, reason: &str) {
    if let Ok(mut held) = grok_unusable_cache().lock() {
        *held = Some((path.to_path_buf(), reason.to_string()));
    }
    invalidate_grok_bin_cache();
}

pub fn grok_unusable_reason(path: &Path) -> Option<String> {
    grok_unusable_cache()
        .lock()
        .ok()
        .and_then(|g| g.as_ref().and_then(|(p, r)| same_grok_path(p, path).then(|| r.clone())))
}

pub fn grok_marked_unusable(path: &Path) -> bool {
    grok_unusable_reason(path).is_some()
}

pub fn clear_grok_unusable() {
    if let Ok(mut held) = grok_unusable_cache().lock() {
        *held = None;
    }
}

pub fn doctor_broken_hint() -> &'static str {
    "Grok Build CLI is broken (missing DLL or bad image). First launch / reinstall installs alpha."
}

/// True after a successful `grok --version` in this process (doctor, install
/// skip, or finish). Missing or marked-broken binaries are not ready — first
/// launch and Settings → Update treat them as "install alpha".
pub fn grok_cli_known_good() -> bool {
    let Some(p) = find_grok() else {
        return false;
    };
    if grok_marked_unusable(&p) {
        return false;
    }
    doctor_line_cache()
        .lock()
        .ok()
        .and_then(|g| {
            g.as_ref().and_then(|(path, _, ok, _, inflight)| {
                (*ok && !*inflight && path.as_deref().is_some_and(|c| same_grok_path(c, &p)))
                    .then_some(true)
            })
        })
        .unwrap_or(false)
}

/// Skip the official installer only when this binary actually runs.
pub fn cli_install_should_skip(found: Option<&Path>) -> bool {
    let Some(p) = found else {
        return false;
    };
    if !grok_bin_is_native(p) || !grok_bin_looks_complete(p) || grok_marked_unusable(p) {
        return false;
    }
    grok_cli_is_runnable(p)
}

pub fn grok_cli_is_runnable(path: &Path) -> bool {
    if grok_marked_unusable(path) || !grok_bin_looks_complete(path) {
        return false;
    }
    match grok_version(path) {
        Ok(v) => {
            remember_doctor_result(Some(path.to_path_buf()), true, doctor_ok_line(&v));
            true
        }
        Err(e) => {
            if is_cli_hard_failure(None, &e) {
                mark_grok_unusable(path, &e);
            }
            false
        }
    }
}

/// Incomplete downloads / 4-byte MZ stubs are not an installed CLI.
pub fn grok_bin_looks_complete(path: &Path) -> bool {
    let Ok(meta) = std::fs::metadata(path) else {
        return false;
    };
    if cfg!(windows) {
        meta.len() >= 4096
    } else {
        meta.len() > 0
    }
}

/// Drop the grok PATH cache after a first-run install.
pub fn invalidate_grok_bin_cache() {
    if let Ok(mut held) = grok_bin_cache().lock() {
        *held = None;
    }
}

/// Serialize tests that mutate `GROKHUB_GROK` / `PATH`.
#[cfg(test)]
pub(crate) fn grok_env_test_lock() -> std::sync::MutexGuard<'static, ()> {
    static L: OnceLock<Mutex<()>> = OnceLock::new();
    L.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

fn grok_bin_name() -> &'static str {
    if cfg!(windows) {
        "grok.exe"
    } else {
        "grok"
    }
}

/// Windows runners often have a Unix/ELF `grok` on PATH (Git bash, leftover Linux).
/// That is not a Grok Build we can spawn — error 193 is "not a valid Win32 application".
fn grok_bin_is_native(path: &Path) -> bool {
    if !cfg!(windows) {
        return path.is_file();
    }
    let Ok(mut f) = std::fs::File::open(path) else {
        return false;
    };
    let mut magic = [0u8; 4];
    if f.read(&mut magic).unwrap_or(0) < 2 {
        return false;
    }
    magic[0] == b'M' && magic[1] == b'Z'
}

fn take_grok_bin(path: PathBuf) -> Option<PathBuf> {
    path.is_file()
        .then_some(path)
        .filter(|p| grok_bin_is_native(p))
        .filter(|p| grok_bin_looks_complete(p))
        .filter(|p| !grok_marked_unusable(p))
}

fn find_grok_scan() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("GROKHUB_GROK") {
        let p = PathBuf::from(p);
        if p.is_file() {
            return take_grok_bin(p);
        }
        // Explicit override: do not fall through to PATH / ~/.local/bin/grok.
        return None;
    }
    if let Some(p) = which("grok") {
        return Some(p);
    }
    if let Some(home) = grokhub_core::user_home() {
        if cfg!(windows) {
            if let Some(p) = take_grok_bin(home.join(".grok").join("bin").join(grok_bin_name())) {
                return Some(p);
            }
        } else {
            for rel in [".local/bin", ".grok/bin"] {
                if let Some(p) = take_grok_bin(home.join(rel).join(grok_bin_name())) {
                    return Some(p);
                }
            }
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            if let Some(p) = take_grok_bin(dir.join(grok_bin_name())) {
                return Some(p);
            }
        }
    }
    None
}

fn kick_find_grok(key: String) {
    if let Ok(mut held) = grok_bin_cache().lock() {
        if let Some(slot) = held.as_mut() {
            if slot.0 == key {
                if slot.3 {
                    return;
                }
                slot.3 = true;
            }
        }
    }
    thread::spawn(move || {
        let path = find_grok_scan();
        if let Ok(mut held) = grok_bin_cache().lock() {
            *held = Some((key, Instant::now(), path, false));
        }
    });
}

pub fn which(name: &str) -> Option<PathBuf> {
    let paths = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&paths) {
        if cfg!(windows) && !name.ends_with(".exe") {
            if let Some(p) = take_grok_bin(dir.join(format!("{name}.exe"))) {
                return Some(p);
            }
        }
        if let Some(p) = take_grok_bin(dir.join(name)) {
            return Some(p);
        }
    }
    None
}

pub fn grok_home() -> Option<PathBuf> {
    Some(grokhub_core::user_home()?.join(".grok"))
}

/// Channel `grok update` will follow. Prefers `~/.grok/config.toml` `[cli] channel`
/// so a cabin launch does not hit the network. Falls back to
/// `grok update --check --json`.
pub fn grok_cli_channel(bin: &Path) -> Option<String> {
    if let Some(home) = grok_home() {
        let cfg = home.join("config.toml");
        if let Ok(text) = std::fs::read_to_string(&cfg) {
            if let Some(ch) = grokhub_core::parse_cli_config_channel(&text) {
                return Some(ch);
            }
        }
    }
    let cwd = grok_home().unwrap_or_else(std::env::temp_dir);
    let text = grok_user_stdout_timeout(bin, &cwd, &["update", "--check", "--json"], 20).ok()?;
    grokhub_core::parse_cli_update_check_channel(&text)
}

/// Socket for cabin `grok agent stdio`. Must not be `~/.grok/leader.sock` or the
/// interactive CLI leader SIGTERMs the cabin child (wait status 143).
pub fn cabin_leader_socket() -> Option<PathBuf> {
    Some(cabin_grok_home()?.join("leader.sock"))
}

/// Isolated grok home for the cabin child. Sharing `~/.grok` loads the CLI's
/// chrome-devtools MCP plugin and the running CLI can SIGTERM this process
/// (exit 143) while it pushes the model catalog.
pub fn cabin_grok_home() -> Option<PathBuf> {
    Some(cabin_config_root()?.join("grok-home"))
}

fn cabin_config_root() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("GROKHUB_CONFIG") {
        return Some(PathBuf::from(p));
    }
    if cfg!(windows) {
        let app = std::env::var_os("APPDATA")?;
        return Some(PathBuf::from(app).join("GrokHub"));
    }
    Some(grokhub_core::user_home()?.join(".config/GrokHub"))
}

pub fn doctor_missing_hint() -> &'static str {
    if cfg!(windows) {
        "Grok Build CLI missing — first launch installs alpha. Retry: $env:GROK_CHANNEL='alpha'; irm https://x.ai/cli/install.ps1 | iex"
    } else {
        "Grok Build CLI missing — first launch installs alpha. Retry: curl -fsSL https://x.ai/cli/install.sh | GROK_CHANNEL=alpha bash"
    }
}

/// Make `GROK_HOME` usable: directory plus a symlink to the real `grok login`.
pub fn prepare_cabin_grok_home() -> Option<PathBuf> {
    let dir = cabin_grok_home()?;
    std::fs::create_dir_all(&dir).ok()?;
    if let Some(src) = grok_auth_path() {
        let dst = dir.join("auth.json");
        #[cfg(unix)]
        {
            if !dst.exists() {
                let _ = std::os::unix::fs::symlink(&src, &dst);
            }
        }
        #[cfg(not(unix))]
        {
            // Copy is not a live link — refresh after a later `grok login`.
            if src.is_file() {
                let _ = std::fs::copy(&src, &dst);
            }
        }
    }
    Some(dir)
}

pub fn grok_auth_path() -> Option<PathBuf> {
    Some(grok_home()?.join("auth.json"))
}

pub fn invalidate_grok_key_cache() {
    if let Ok(mut held) = grok_key_cache().lock() {
        *held = None;
    }
}

fn create_private_file(path: &Path) -> std::io::Result<std::fs::File> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        let _ = std::fs::remove_file(path);
        std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)
    }
    #[cfg(not(unix))]
    {
        std::fs::File::create(path)
    }
}

/// Write cabin OAuth into `~/.grok/auth.json` when the CLI has no session.
/// Returns `Ok(true)` if a file was written.
pub fn write_cli_auth_if_needed(tokens: &grokhub_core::XaiOAuthTokens) -> Result<bool, String> {
    if !grokhub_core::should_sync_cli_auth(grok_cli_key().is_some()) {
        return Ok(false);
    }
    let path = grok_auth_path().ok_or_else(|| "no grok home".to_string())?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let existing = if path.is_file() {
        read_file_capped(&path, 64 * 1024)
    } else {
        String::new()
    };
    if !grokhub_core::should_sync_cli_auth(parse_grok_auth_key(&existing).is_some()) {
        return Ok(false);
    }
    let body = grokhub_core::merge_cli_auth_json(&existing, tokens)?;
    let tmp = path.with_extension("json.tmp");
    {
        use std::io::Write;
        let mut f = create_private_file(&tmp).map_err(|e| e.to_string())?;
        f.write_all(body.as_bytes()).map_err(|e| e.to_string())?;
        f.sync_all().map_err(|e| e.to_string())?;
    }
    std::fs::rename(&tmp, &path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        e.to_string()
    })?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }
    invalidate_grok_key_cache();
    let _ = prepare_cabin_grok_home();
    Ok(true)
}

/// Cached `grok login` bearer from `~/.grok/auth.json`. Never logs the secret.
pub fn grok_cli_key() -> Option<String> {
    let path = grok_auth_path()?;
    if let Ok(held) = grok_key_cache().lock() {
        if let Some((p, _, at, key, inflight)) = held.as_ref() {
            if *p == path {
                let hit = key.clone();
                let fresh = at.elapsed() < Duration::from_secs(2);
                let busy = *inflight;
                drop(held);
                if !fresh && !busy {
                    kick_grok_cli_key(path);
                }
                return hit;
            }
        }
    }
    let (modified, key) = grok_cli_key_now(&path);
    if let Ok(mut held) = grok_key_cache().lock() {
        *held = Some((path, modified, Instant::now(), key.clone(), false));
    }
    key
}

fn grok_key_cache() -> &'static Mutex<GrokKeyCache> {
    static C: OnceLock<Mutex<GrokKeyCache>> = OnceLock::new();
    C.get_or_init(|| Mutex::new(None))
}

fn grok_cli_key_now(path: &Path) -> (Option<std::time::SystemTime>, Option<String>) {
    let modified = std::fs::metadata(path).ok().and_then(|m| m.modified().ok());
    let raw = read_file_capped(path, 64 * 1024);
    (modified, parse_grok_auth_key(&raw))
}

fn kick_grok_cli_key(path: PathBuf) {
    if let Ok(mut held) = grok_key_cache().lock() {
        if let Some(slot) = held.as_mut() {
            if slot.0 == path {
                if slot.4 {
                    return;
                }
                slot.4 = true;
            }
        }
    }
    thread::spawn(move || {
        let (modified, key) = grok_cli_key_now(&path);
        if let Ok(mut held) = grok_key_cache().lock() {
            *held = Some((path, modified, Instant::now(), key, false));
        }
    });
}

fn read_file_capped(path: &Path, cap: usize) -> String {
    let mut f = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(_) => return String::new(),
    };
    let mut buf = vec![0u8; cap];
    let n = match std::io::Read::read(&mut f, &mut buf) {
        Ok(n) => n,
        Err(_) => return String::new(),
    };
    String::from_utf8_lossy(&buf[..n]).into_owned()
}

pub fn parse_grok_auth_key(raw: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(raw).ok()?;
    if let Some(key) = grok_key_from_value(&v) {
        return Some(key);
    }
    let obj = v.as_object()?;
    let mut best: Option<(String, String)> = None;
    for rec in obj.values() {
        let Some(key) = grok_key_from_value(rec) else {
            continue;
        };
        let exp = rec
            .get("expires_at")
            .and_then(|x| x.as_str())
            .or_else(|| rec.get("expiresAt").and_then(|x| x.as_str()))
            .unwrap_or("")
            .to_string();
        let take = match &best {
            None => true,
            Some((prev, _)) => exp > *prev,
        };
        if take {
            best = Some((exp, key));
        }
    }
    best.map(|(_, k)| k)
}

fn grok_key_from_value(v: &serde_json::Value) -> Option<String> {
    for field in ["key", "access_token", "accessToken", "token"] {
        if let Some(k) = v.get(field).and_then(|x| x.as_str()).map(str::trim) {
            if !k.is_empty() {
                return Some(k.to_string());
            }
        }
    }
    None
}

pub fn grok_version(bin: &Path) -> Result<String, String> {
    let cwd = std::env::temp_dir();
    let text = grok_stdout_timeout(bin, &cwd, &["--version"], 3)?;
    let line = text.lines().next().unwrap_or(text.trim()).trim();
    if line.is_empty() {
        return Err("grok --version empty".into());
    }
    Ok(line.to_string())
}

fn doctor_line_cache() -> &'static Mutex<DoctorLineCache> {
    static C: OnceLock<Mutex<DoctorLineCache>> = OnceLock::new();
    C.get_or_init(|| Mutex::new(None))
}

/// True while a background `grok --version` is in flight.
pub fn doctor_line_busy() -> bool {
    doctor_line_cache()
        .lock()
        .ok()
        .and_then(|g| g.as_ref().map(|c| c.4))
        .unwrap_or(false)
}

/// Blocking locate result for `grokhub --doctor`. Same missing/present text as Settings.
pub fn doctor_grok_line_blocking(bin: Option<&Path>) -> (bool, String) {
    match bin {
        None => (false, doctor_missing_hint().into()),
        Some(p) if !grok_bin_is_native(p) || !grok_bin_looks_complete(p) => {
            (false, doctor_missing_hint().into())
        }
        Some(p) if grok_marked_unusable(p) => (false, doctor_broken_hint().into()),
        Some(p) => match grok_version(p) {
            Ok(v) => (true, doctor_ok_line(&v)),
            Err(e) => {
                if is_cli_hard_failure(None, &e) {
                    mark_grok_unusable(p, &e);
                    (false, doctor_broken_hint().into())
                } else {
                    (false, format!("Grok Build present but unreadable: {e}"))
                }
            }
        },
    }
}

pub fn doctor_grok_line(bin: Option<&Path>) -> (bool, String) {
    if bin.is_none() {
        return doctor_grok_line_blocking(None);
    }
    if let Some(p) = bin {
        if grok_marked_unusable(p) {
            return (false, doctor_broken_hint().into());
        }
    }
    if let Ok(held) = doctor_line_cache().lock() {
        if let Some((path, at, ok, text, inflight)) = held.as_ref() {
            if path.as_deref() == bin && (*inflight || at.elapsed() < Duration::from_secs(8)) {
                return (*ok, text.clone());
            }
        }
    }
    let path = bin.map(|p| p.to_path_buf());
    let last = if let Ok(mut held) = doctor_line_cache().lock() {
        let last = match held.as_ref() {
            Some((p, _, ok, text, _)) if p == &path => (*ok, text.clone()),
            _ => (false, "Checking Grok Build CLI…".into()),
        };
        *held = Some((path.clone(), Instant::now(), last.0, last.1.clone(), true));
        last
    } else {
        (false, "Checking Grok Build CLI…".into())
    };
    thread::spawn(move || {
        let (ok, text) = doctor_grok_line_blocking(path.as_deref());
        remember_doctor_result(path, ok, text);
    });
    last
}

fn doctor_ok_line(version: &str) -> String {
    let v = version.trim().strip_prefix("grok ").unwrap_or(version.trim());
    format!("Grok Build {v}")
}

fn remember_doctor_result(path: Option<PathBuf>, ok: bool, text: String) {
    if let Ok(mut held) = doctor_line_cache().lock() {
        *held = Some((path, Instant::now(), ok, text, false));
    }
}

pub fn grok_stdout(bin: &Path, cwd: &Path, args: &[&str]) -> Result<String, String> {
    grok_stdout_timeout(bin, cwd, args, 60)
}

/// Run `grok` and cap how long we wait so History cannot freeze the cabin.
pub fn grok_stdout_timeout(bin: &Path, cwd: &Path, args: &[&str], secs: u64) -> Result<String, String> {
    grok_stdout_inner(bin, cwd, args, Some(secs), true, false)
}

/// Skills / MCP / marketplace live in the user's `~/.grok`, not cabin GROK_HOME.
pub fn grok_user_stdout_timeout(
    bin: &Path,
    cwd: &Path,
    args: &[&str],
    secs: u64,
) -> Result<String, String> {
    grok_stdout_inner(bin, cwd, args, Some(secs), false, false)
}

/// Same as [`grok_user_stdout_timeout`] but the child runs until it exits.
/// Cabin `/loop` used to die at 300 seconds.
pub fn grok_user_stdout_wait(bin: &Path, cwd: &Path, args: &[&str]) -> Result<String, String> {
    grok_stdout_inner(bin, cwd, args, None, false, false)
}

/// Like [`grok_user_stdout_timeout`], but a non-zero exit still returns output
/// when the CLI did not hard-fail. `grok mcp doctor --json` exits 1 and prints
/// JSON on stdout. Stdout wins over stderr so that JSON is not dropped.
pub fn grok_user_stdout_allow_fail(
    bin: &Path,
    cwd: &Path,
    args: &[&str],
    secs: u64,
) -> Result<String, String> {
    grok_stdout_inner(bin, cwd, args, Some(secs), false, true)
}

fn grok_stdout_inner(
    bin: &Path,
    cwd: &Path,
    args: &[&str],
    kill_after: Option<u64>,
    isolate_cabin: bool,
    keep_fail_stdout: bool,
) -> Result<String, String> {
    if grok_marked_unusable(bin) {
        return Err(doctor_broken_hint().into());
    }
    let bin = bin.to_path_buf();
    let cwd = cwd.to_path_buf();
    let owned: Vec<String> = args.iter().map(|s| s.to_string()).collect();
    let mut cmd = Command::new(&bin);
    cmd.args(&owned)
        .current_dir(&cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    hide_windows_console(&mut cmd);
    cmd.env("GROK_NO_AUTO_UPDATE", "1");
    if isolate_cabin {
        if let Some(dir) = prepare_cabin_grok_home() {
            cmd.env("GROK_HOME", dir);
        }
        if let Some(sock) = cabin_leader_socket() {
            cmd.env("GROK_LEADER_SOCKET", sock);
        }
    }
    let child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            let msg = e.to_string();
            if is_cli_hard_failure(e.raw_os_error(), &msg) {
                mark_grok_unusable(&bin, &msg);
                return Err(doctor_broken_hint().into());
            }
            return Err(msg);
        }
    };
    let pid = child.id();
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let out = child.wait_with_output();
        let _ = tx.send(out);
    });
    let out = match kill_after {
        None => match rx.recv() {
            Ok(Ok(o)) => o,
            Ok(Err(e)) => return Err(e.to_string()),
            Err(_) => return Err(format!("grok {} ended", args.join(" "))),
        },
        Some(secs) => match rx.recv_timeout(Duration::from_secs(secs.max(1))) {
            Ok(Ok(o)) => o,
            Ok(Err(e)) => return Err(e.to_string()),
            Err(_) => {
                #[cfg(windows)]
                {
                    let mut kill = Command::new("taskkill");
                    kill.args(["/PID", &pid.to_string(), "/T", "/F"])
                        .stdin(Stdio::null())
                        .stdout(Stdio::null())
                        .stderr(Stdio::null());
                    hide_windows_console(&mut kill);
                    let _ = kill.status();
                }
                #[cfg(not(windows))]
                {
                    let _ = Command::new("kill")
                        .args(["-TERM", &pid.to_string()])
                        .status();
                    thread::sleep(Duration::from_millis(80));
                    let _ = Command::new("kill")
                        .args(["-KILL", &pid.to_string()])
                        .status();
                }
                return Err(format!("grok {} timed out", args.join(" ")));
            }
        },
    };
    let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
    if !out.status.success() {
        let detail = if !stderr.is_empty() {
            stderr.clone()
        } else if !stdout.is_empty() {
            stdout.clone()
        } else {
            format!("grok {} failed", args.join(" "))
        };
        if is_cli_hard_failure(out.status.code(), &detail) {
            mark_grok_unusable(&bin, &detail);
            return Err(doctor_broken_hint().into());
        }
        if keep_fail_stdout {
            return Ok(if stdout.is_empty() { stderr } else { stdout });
        }
        return Err(detail);
    }
    if stdout.is_empty() {
        Ok(stderr)
    } else {
        Ok(stdout)
    }
}

pub fn agent_args(always_approve: bool, reasoning_effort: Option<&str>) -> Vec<String> {
    let mut a = vec!["--no-auto-update".into(), "agent".into()];
    if let Some(effort) = reasoning_effort {
        let effort = effort.trim();
        if !effort.is_empty() {
            a.push("--reasoning-effort".into());
            a.push(effort.into());
        }
    }
    if always_approve {
        a.push("--always-approve".into());
    }
    a.push("stdio".into());
    a
}

/// Headless `grok -p` so a cabin chat maps 1:1 onto a Grok Build session
/// without a long-lived `agent stdio` child of the GUI (exit 143).
/// Grok Build 1.0.38 `--help` for these flags is byte-identical to 1.0.36.
pub fn single_turn_args(
    prompt: &str,
    cwd: &str,
    resume: Option<&str>,
    always_approve: bool,
    auto: bool,
) -> Vec<String> {
    let mut a = vec![
        "--no-auto-update".into(),
        "-p".into(),
        prompt.to_string(),
        "--cwd".into(),
        cwd.to_string(),
        "--output-format".into(),
        "streaming-json".into(),
    ];
    if always_approve {
        a.push("--always-approve".into());
    } else if auto {
        a.push("--permission-mode".into());
        a.push("auto".into());
    }
    if let Some(id) = resume.map(str::trim).filter(|s| !s.is_empty()) {
        a.push("--resume".into());
        a.push(id.to_string());
    }
    if let Some(sock) = cabin_leader_socket() {
        a.push("--leader-socket".into());
        a.push(sock.display().to_string());
    }
    a
}

pub fn single_turn_args_full(
    prompt: &str,
    cwd: &str,
    resume: Option<&str>,
    always_approve: bool,
    auto: bool,
    model: Option<&str>,
    effort: Option<&str>,
    mode: SessionMode,
) -> Vec<String> {
    let plan = mode == SessionMode::Plan;
    let look = mode == SessionMode::Ask;
    let mut a = single_turn_args(
        prompt,
        cwd,
        resume,
        always_approve && !plan && !look,
        auto && !plan && !look,
    );
    if plan {
        a.push("--permission-mode".into());
        a.push("plan".into());
    } else if look {
        // SessionMode::Ask is the btw pill (saved id `ask`). CLI allows
        // default | acceptEdits | auto | dontAsk | bypassPermissions | plan.
        // Literal "ask" is invalid (Grok Build CLI exit 2).
        a.push("--permission-mode".into());
        a.push("default".into());
    }
    // btw (saved as ask) stays look-only: do not remap to --always-approve.
    // Composer Ask leftover flags match scheduled Ask (no yolo).
    // Night / loop / phone inherit the pill via PermissionMode::scheduled_flags.
    if let Some(m) = model.map(str::trim).filter(|s| !s.is_empty()) {
        a.push("--model".into());
        a.push(m.to_string());
    }
    if let Some(e) = effort.map(str::trim).filter(|s| !s.is_empty()) {
        a.push("--reasoning-effort".into());
        a.push(e.to_string());
    }
    if !look {
        a.push("--sandbox".into());
        a.push("off".into());
        a.push("--rules".into());
        a.push(cabin_rules(""));
    }
    a
}

/// Base rules, plus a short learned brief when the cabin already knows them.
/// Empty brief stays byte-identical to `CABIN_DESKTOP_RULES`.
pub fn cabin_rules(learned: &str) -> String {
    let learned: String = learned.trim().chars().take(360).collect();
    if learned.is_empty() {
        return CABIN_DESKTOP_RULES.to_string();
    }
    format!(
        "{CABIN_DESKTOP_RULES}\nWhat you have learned about them. Use it. Do not recite it.\n{learned}"
    )
}

/// One line on the cabin rules while Settings → desktop control is on.
/// Off stays byte-identical to [`cabin_rules`].
pub const DESKTOP_CABIN_LINE: &str =
    "Prefer the grokhub-desktop tools over shell xdotool or PowerShell for the screen.";

pub fn cabin_rules_for(learned: &str, desktop: bool) -> String {
    let base = cabin_rules(learned);
    if !desktop {
        return base;
    }
    format!("{base}\n{DESKTOP_CABIN_LINE}")
}

/// Headless GrokHub chat is the cabin assistant on this Linux box, not grok.com.
/// One argv for `grok -p --rules`. Look mode (btw) does not receive this.
pub const CABIN_DESKTOP_RULES: &str = "You are the cabin assistant on this Linux desktop through GrokHub. You can do what this computer can do: files, shell, browser, and the desktop. Never say you lack access to this computer, files, or desktop. Do the next step with tools. Ask only before sending a message, paying, deleting something they did not name, or publishing. Be brief and warm. Do not repeat the chat. Do not paste code, diffs, or logs unless they asked to see it. When they hand you work, track it with WORK_PIN and WORK_UPDATE and keep going. A paused workboard card is still yours. Resume it. When a tool, a page, or a first pass comes back empty or wrong, try one other path. Then say what blocked you and the next useful step. Do not end the turn on that first miss. Do not invent a source, a count, or a fact. A stable preference or routine is one line: USER_FACT: why they asked and what would help next time, not a copy of their sentence. Separate long work that should not hold up this chat (a full test run, a big search, a batch elsewhere) is one line per task: BACKGROUND_TASK: complete, self-contained instructions. The cabin runs it beside this chat and posts the result here; do not wait for it.";

/// Swap `-p <prompt>` for `--prompt-json` when a still is attached.
pub fn with_prompt_json(mut args: Vec<String>, json: &str) -> Vec<String> {
    if let Some(i) = args.iter().position(|a| a == "-p") {
        args.remove(i);
        if i < args.len() {
            args.remove(i);
        }
        args.push("--prompt-json".into());
        args.push(json.to_string());
    }
    args
}

pub fn with_fork_session(mut args: Vec<String>, fork: bool) -> Vec<String> {
    if fork && args.iter().any(|a| a == "--resume") {
        args.push("--fork-session".into());
    }
    args
}

pub fn with_worktree(mut args: Vec<String>, on: bool) -> Vec<String> {
    if on && !args.iter().any(|a| a == "--worktree") {
        args.push("--worktree".into());
    }
    args
}

/// Shell, edit, write, and the desktop MCP server. An unwatched Ask run
/// denies these: nobody can answer the prompt grok would otherwise show.
pub const ASK_DENY_RULES: &[&str] = &[
    "Bash",
    "Edit",
    "Write",
    grokhub_core::DESKTOP_MCP_RULE,
];

/// Fail closed on an unwatched `grok -p` while Ask is on.
///
/// Nobody can answer a prompt there, so Ask must fail closed. When `deny` is
/// set, drop `--always-approve`, pass `--permission-mode dontAsk` only if args
/// do not already carry `--permission-mode` (the flag may appear once; Plan and
/// look already set one), then `--deny` each [`ASK_DENY_RULES`] entry. Deny
/// always wins. A no-op when `deny` is false.
pub fn with_ask_deny(mut args: Vec<String>, deny: bool) -> Vec<String> {
    if !deny {
        return args;
    }
    // Drop the flag, never a prompt value (the word after `-p`) that reads the same.
    let mut after_p = false;
    args.retain(|a| {
        let keep = after_p || a != "--always-approve";
        after_p = a == "-p";
        keep
    });
    if !args.iter().any(|a| a == "--permission-mode") {
        args.push("--permission-mode".into());
        args.push("dontAsk".into());
    }
    for rule in ASK_DENY_RULES {
        args.push("--deny".into());
        args.push((*rule).to_string());
    }
    args
}

/// Desktop allow/deny for every headless `grok -p`. Plan and btw (Ask) deny
/// the desktop tools even on Auto or Always. Attended Ask never reaches here.
pub fn apply_desktop_spawn_args(
    args: Vec<String>,
    permission: PermissionMode,
    session: SessionMode,
    enabled: bool,
) -> Vec<String> {
    let mode = match session {
        SessionMode::Plan | SessionMode::Ask => grokhub_core::DesktopPermMode::Ask,
        SessionMode::Chat => match permission {
            PermissionMode::AlwaysApprove => grokhub_core::DesktopPermMode::Always,
            PermissionMode::Auto => grokhub_core::DesktopPermMode::Auto,
            PermissionMode::Ask => grokhub_core::DesktopPermMode::Ask,
        },
    };
    grokhub_core::apply_desktop_mcp_args(args, mode, false, enabled)
}

pub fn agent_args_resume(
    always_approve: bool,
    resume: Option<&str>,
    reasoning_effort: Option<&str>,
) -> Vec<String> {
    let _ = resume;
    agent_args(always_approve, reasoning_effort)
}

/// `grok mcp add grokhub-desktop -- <exe> --mcp-desktop`
pub fn desktop_mcp_add_argv(exe: &str) -> Vec<String> {
    vec![
        "mcp".into(),
        "add".into(),
        grokhub_core::DESKTOP_MCP_SERVER.into(),
        "--".into(),
        exe.into(),
        "--mcp-desktop".into(),
    ]
}

/// `grok mcp remove grokhub-desktop`
pub fn desktop_mcp_remove_argv() -> Vec<String> {
    vec![
        "mcp".into(),
        "remove".into(),
        grokhub_core::DESKTOP_MCP_SERVER.into(),
    ]
}

/// Register the stdio server in the cabin `GROK_HOME`, never `~/.grok`.
pub fn register_desktop_mcp(bin: &Path, cwd: &Path, exe: &Path) -> Result<String, String> {
    let argv = desktop_mcp_add_argv(&exe.display().to_string());
    let refs: Vec<&str> = argv.iter().map(String::as_str).collect();
    grok_stdout_timeout(bin, cwd, &refs, 20)
}

/// Remove the cabin registration. Same isolated runner as [`register_desktop_mcp`].
pub fn unregister_desktop_mcp(bin: &Path, cwd: &Path) -> Result<String, String> {
    let argv = desktop_mcp_remove_argv();
    let refs: Vec<&str> = argv.iter().map(String::as_str).collect();
    grok_stdout_timeout(bin, cwd, &refs, 20)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::PermissionMode;

    #[test]
    fn env_override_missing_is_none() {
        let _lock = grok_env_test_lock();
        let prev = std::env::var_os("GROKHUB_GROK");
        std::env::set_var("GROKHUB_GROK", "/no/such/grok-binary-xyz");
        let hit = find_grok();
        if let Some(p) = prev {
            std::env::set_var("GROKHUB_GROK", p);
        } else {
            std::env::remove_var("GROKHUB_GROK");
        }
        assert!(
            hit.is_none(),
            "GROKHUB_GROK must not fall through to ~/.local/bin/grok: {hit:?}"
        );
    }

    #[test]
    fn doctor_missing() {
        let (ok, text) = doctor_grok_line_blocking(None);
        assert!(!ok);
        assert!(text.contains("x.ai/cli"));
        let (cached_ok, cached_text) = doctor_grok_line(None);
        assert_eq!((cached_ok, cached_text), (ok, text.clone()));
        let src = include_str!("locate.rs");
        let ver = src
            .split("pub fn grok_version(")
            .nth(1)
            .and_then(|s| s.split("pub fn doctor_grok_line(").next())
            .expect("grok_version");
        assert!(
            ver.contains("grok_stdout_timeout"),
            "grok --version must not hang the settings overlay: {ver}"
        );
        let doc = src
            .split("pub fn doctor_grok_line(")
            .nth(1)
            .and_then(|s| s.split("pub fn grok_stdout(").next())
            .expect("doctor_grok_line");
        assert!(
            doc.contains("elapsed"),
            "Settings must not spawn grok --version every frame: {doc}"
        );
        assert!(
            !doc.contains("sticky_fail"),
            "soft grok --version failures must expire so Settings can recover: {doc}"
        );
        assert!(
            doc.contains("thread::spawn") && doc.contains("inflight"),
            "Settings must not freeze on grok --version: {doc}"
        );
        let blocking = src
            .split("pub fn doctor_grok_line_blocking(")
            .nth(1)
            .and_then(|s| s.split("pub fn doctor_grok_line(").next())
            .expect("doctor_grok_line_blocking");
        assert!(
            blocking.contains("grok_version") && blocking.contains("doctor_missing_hint"),
            "CLI doctor must use grok_version, not a placeholder: {blocking}"
        );
        assert!(
            src.contains("grok_bin_is_native") && src.contains("b'M'"),
            "Windows must skip Unix/ELF grok on PATH: {src}"
        );
        assert!(
            doctor_broken_hint().contains("First launch / reinstall installs alpha")
                && doctor_missing_hint().contains("first launch installs alpha")
                && doctor_missing_hint().contains("x.ai/cli"),
            "doctor must say first launch installs alpha: {} / {}",
            doctor_broken_hint(),
            doctor_missing_hint()
        );
        let fake = std::env::temp_dir().join(format!(
            "grokhub-fake-grok-{}",
            std::process::id()
        ));
        std::fs::write(&fake, "#!/bin/sh\necho '9.9.9-test (deadbeef)'\n").expect("fake grok");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut p = std::fs::metadata(&fake).expect("meta").permissions();
            p.set_mode(0o755);
            std::fs::set_permissions(&fake, p).expect("chmod");
        }
        let (ok, text) = doctor_grok_line_blocking(Some(fake.as_path()));
        let _ = std::fs::remove_file(&fake);
        #[cfg(unix)]
        {
            assert!(ok, "{text}");
            assert!(
                text.contains("9.9.9-test"),
                "blocking doctor must print grok --version from the located binary: {text}"
            );
        }
        #[cfg(windows)]
        {
            assert!(!ok, "{text}");
            assert!(
                text.contains("x.ai/cli"),
                "a Unix/ELF grok on Windows is missing, not present-but-unreadable: {text}"
            );
            assert!(
                !text.contains("unreadable"),
                "foreign grok must not look installed: {text}"
            );
        }
        let find = src
            .split("pub fn find_grok(")
            .nth(1)
            .and_then(|s| s.split("pub fn which(").next())
            .expect("find_grok");
        assert!(
            find.contains("elapsed"),
            "the composer must not walk PATH every frame: {find}"
        );
        assert!(
            find.contains("thread::spawn") && find.contains("inflight"),
            "a stale grok PATH cache must refresh off the UI thread: {find}"
        );
        let key = src
            .split("pub fn grok_cli_key(")
            .nth(1)
            .and_then(|s| s.split("pub fn parse_grok_auth_key(").next())
            .expect("grok_cli_key");
        assert!(
            key.contains("read_file_capped") && key.contains("elapsed") && !key.contains("read_to_string"),
            "grok login must not slurp auth.json every paint: {key}"
        );
        assert!(
            key.contains("thread::spawn") && key.contains("inflight"),
            "stale grok login must refresh auth.json off the UI thread: {key}"
        );
        let prep = include_str!("locate.rs")
            .split("pub fn prepare_cabin_grok_home(")
            .nth(1)
            .and_then(|s| s.split("pub fn grok_auth_path(").next())
            .expect("prepare_cabin_grok_home");
        let win = prep
            .split("#[cfg(not(unix))]")
            .nth(1)
            .expect("windows auth copy");
        assert!(
            win.contains("std::fs::copy") && !win.contains("dst.exists()"),
            "Windows auth.json copy must refresh after grok login: {win}"
        );
    }

    #[test]
    fn grok_cmd_fails_on_nonzero_even_with_stdout() {
        let inner = include_str!("locate.rs")
            .split("fn grok_stdout_inner(")
            .nth(1)
            .and_then(|s| s.split("pub fn agent_args(").next())
            .expect("grok_stdout_inner");
        assert!(
            inner.contains("if !out.status.success()") && !inner.contains("&& stdout.is_empty()"),
            "grok sessions delete must fail on a non-zero exit even when it printed a reason: {inner}"
        );
    }

    #[test]
    fn doctor_stdout_survives_nonzero_exit() {
        let src = include_str!("locate.rs");
        let allow = src
            .split("pub fn grok_user_stdout_allow_fail(")
            .nth(1)
            .and_then(|s| s.split("fn grok_stdout_inner(").next())
            .expect("grok_user_stdout_allow_fail");
        assert!(
            allow.contains("keep") || allow.contains("true"),
            "doctor must ask for stdout on a non-zero exit: {allow}"
        );
        let inner = src
            .split("fn grok_stdout_inner(")
            .nth(1)
            .and_then(|s| s.split("pub fn agent_args(").next())
            .expect("inner");
        assert!(
            inner.contains("keep_fail_stdout") && inner.contains("if !out.status.success()"),
            "a non-zero doctor exit must still be able to return stdout: {inner}"
        );
    }

    #[test]
    fn foreign_elf_grok_is_not_native_on_windows() {
        let path = std::env::temp_dir().join(format!(
            "grokhub-elf-grok-{}",
            std::process::id()
        ));
        std::fs::write(&path, b"\x7fELFnot-a-windows-grok").expect("elf");
        assert_eq!(grok_bin_is_native(&path), !cfg!(windows));
        let (ok, text) = doctor_grok_line_blocking(Some(path.as_path()));
        let _ = std::fs::remove_file(&path);
        #[cfg(windows)]
        {
            assert!(!ok, "{text}");
            assert!(text.contains("x.ai/cli"), "{text}");
            assert!(!text.contains("unreadable"), "{text}");
        }
        let _ = (ok, text);
    }

    #[test]
    fn cabin_leader_socket_is_not_the_cli_leader() {
        let p = cabin_leader_socket().expect("HOME");
        let s = p.to_string_lossy().replace('\\', "/");
        assert!(
            s.contains("GrokHub/grok-home") && s.ends_with("leader.sock"),
            "{s}"
        );
        assert!(
            !s.contains("/.grok/leader.sock"),
            "sharing ~/.grok/leader.sock lets the CLI SIGTERM cabin grok: {s}"
        );
        let connect = include_str!("client.rs");
        assert!(
            connect.contains("cabin_leader_socket")
                && connect.contains("GROK_LEADER_SOCKET")
                && connect.contains("leader-socket")
                && connect.contains("GROK_HOME")
                && connect.contains("prepare_cabin_grok_home"),
            "connect() must isolate GROK_HOME or chrome-devtools MCP / CLI SIGTERM cabin grok (exit 143)"
        );
    }

    #[test]
    fn single_turn_args_bind_resume_and_json() {
        let fresh = single_turn_args("hi", "/tmp/work", None, false, true);
        assert!(fresh.contains(&"-p".into()), "{fresh:?}");
        assert!(fresh.contains(&"hi".into()), "{fresh:?}");
        assert!(
            fresh.windows(2).any(|w| w[0] == "--output-format" && w[1] == "streaming-json"),
            "headless streaming-json so the cabin can paint live tokens: {fresh:?}"
        );
        let pj = with_prompt_json(fresh.clone(), r#"[{"type":"text","text":"hi"}]"#);
        assert!(pj.iter().any(|a| a == "--prompt-json"), "{pj:?}");
        assert!(!pj.iter().any(|a| a == "-p"), "{pj:?}");
        assert!(
            !fresh.iter().any(|a| a == "--resume"),
            "a new chat must create a Grok Build session: {fresh:?}"
        );
        assert!(
            fresh.windows(2).any(|w| w[0] == "--permission-mode" && w[1] == "auto"),
            "{fresh:?}"
        );
        let resume = single_turn_args("again", "/tmp/work", Some("01abc"), true, false);
        assert!(
            resume.windows(2).any(|w| w[0] == "--resume" && w[1] == "01abc"),
            "later turns resume the attached session: {resume:?}"
        );
        let full = single_turn_args_full(
            "hi",
            "/tmp/work",
            None,
            false,
            false,
            Some("grok-4.7"),
            Some("high"),
            SessionMode::Plan,
        );
        assert!(
            full.windows(2).any(|w| w[0] == "--model" && w[1] == "grok-4.7"),
            "{full:?}"
        );
        assert!(
            full.windows(2)
                .any(|w| w[0] == "--reasoning-effort" && w[1] == "high"),
            "{full:?}"
        );
        assert!(
            full.windows(2)
                .any(|w| w[0] == "--permission-mode" && w[1] == "plan"),
            "{full:?}"
        );
        assert!(resume.iter().any(|a| a == "--always-approve"), "{resume:?}");
        let ask = single_turn_args_full(
            "hi",
            "/tmp/work",
            None,
            false,
            false,
            None,
            None,
            SessionMode::Chat,
        );
        assert!(
            !ask.iter().any(|a| a == "--always-approve"),
            "Ask inherit is fail-closed — no silent always-approve: {ask:?}"
        );
        let always = single_turn_args_full(
            "hi",
            "/tmp/work",
            None,
            true,
            false,
            None,
            None,
            SessionMode::Chat,
        );
        assert!(
            always.iter().any(|a| a == "--always-approve"),
            "Always maps to --always-approve: {always:?}"
        );
        let auto = single_turn_args_full(
            "hi",
            "/tmp/work",
            None,
            false,
            true,
            None,
            None,
            SessionMode::Chat,
        );
        assert!(
            auto.windows(2)
                .any(|w| w[0] == "--permission-mode" && w[1] == "auto"),
            "Auto maps to --permission-mode auto: {auto:?}"
        );
        assert!(
            !ask.windows(2)
                .any(|w| w[0] == "--permission-mode" && w[1] == "plan"),
            "{ask:?}"
        );
        assert!(
            ask.windows(2).any(|w| w[0] == "--sandbox" && w[1] == "off"),
            "cabin grok -p must not sandbox away the desktop: {ask:?}"
        );
        assert!(
            ask.windows(2).any(|w| w[0] == "--rules" && w[1] == CABIN_DESKTOP_RULES),
            "cabin grok -p must tell Grok it has this computer: {ask:?}"
        );
        assert_eq!(cabin_rules(""), CABIN_DESKTOP_RULES);
        assert_eq!(cabin_rules_for("", false), CABIN_DESKTOP_RULES);
        let armed = cabin_rules_for("", true);
        assert!(armed.starts_with(CABIN_DESKTOP_RULES));
        assert!(armed.contains(DESKTOP_CABIN_LINE));
        assert_ne!(armed, CABIN_DESKTOP_RULES);
        let with = cabin_rules("Around 21:00 they skip night.");
        assert_eq!(cabin_rules_for("Around 21:00 they skip night.", false), with);
        assert!(with.starts_with(CABIN_DESKTOP_RULES));
        assert!(with.contains("skip night"));
        assert!(with.contains("Do not recite"));
        assert!(
            CABIN_DESKTOP_RULES.contains("this computer")
                && CABIN_DESKTOP_RULES.contains("WORK_PIN")
                && CABIN_DESKTOP_RULES.contains("Resume it")
                && CABIN_DESKTOP_RULES.contains("USER_FACT:")
                && CABIN_DESKTOP_RULES.contains("BACKGROUND_TASK:")
                && CABIN_DESKTOP_RULES.contains("or publishing")
                && CABIN_DESKTOP_RULES.contains("try one other path")
                && CABIN_DESKTOP_RULES.contains("Do not invent a source"),
            "desktop rules must stay a proactive assistant: {CABIN_DESKTOP_RULES}"
        );
        let look = single_turn_args_full(
            "hi",
            "/tmp/work",
            None,
            true,
            true,
            None,
            None,
            SessionMode::Ask,
        );
        assert!(
            look.windows(2)
                .any(|w| w[0] == "--permission-mode" && w[1] == "default"),
            "btw (saved as ask) on Auto/Always is --permission-mode default: {look:?}"
        );
        assert!(
            !look.windows(2)
                .any(|w| w[0] == "--permission-mode" && w[1] == "ask"),
            "CLI rejects --permission-mode ask: {look:?}"
        );
        assert!(
            !look.iter().any(|a| a == "--always-approve"),
            "Look must not yolo: {look:?}"
        );
        assert!(
            !look.windows(2).any(|w| w[0] == "--permission-mode" && w[1] == "auto"),
            "Look must not inherit Auto: {look:?}"
        );
        assert!(
            !look.iter().any(|a| a == "--sandbox")
                && !look.iter().any(|a| a == CABIN_DESKTOP_RULES),
            "Look must not inject desktop-do-the-work: {look:?}"
        );
        let plan_only = single_turn_args_full(
            "hi",
            "/tmp/work",
            None,
            false,
            false,
            None,
            None,
            SessionMode::Plan,
        );
        assert!(
            plan_only
                .windows(2)
                .any(|w| w[0] == "--permission-mode" && w[1] == "plan"),
            "Plan stays plan: {plan_only:?}"
        );
        assert!(
            plan_only
                .windows(2)
                .any(|w| w[0] == "--rules" && w[1] == CABIN_DESKTOP_RULES),
            "Plan keeps desktop rules: {plan_only:?}"
        );
        // 1.0.38 --help: same headless surface as 1.0.36. Do not switch to
        // streaming-messages-json / --include-partial-messages (TUI Messages wire).
        // clone --cone was dropped in 1.0.37; cabin never spawns grok clone.
        assert!(
            !ask.iter().any(|a| a == "streaming-messages-json"
                || a == "--include-partial-messages"
                || a == "--stable"
                || a == "--cone"),
            "cabin must stay on streaming-json + alpha: {ask:?}"
        );
        assert!(
            single_turn_args("hi", "/tmp/work", None, false, true)
                .iter()
                .any(|a| a == "--no-auto-update"),
            "1.0.38 still accepts hidden --no-auto-update"
        );
    }

    #[test]
    fn session_mode_ask_never_emits_cli_permission_ask() {
        for (yolo, auto) in [(false, false), (false, true), (true, false), (true, true)] {
            for mode in [SessionMode::Chat, SessionMode::Plan, SessionMode::Ask] {
                let args = single_turn_args_full(
                    "hi",
                    "/tmp/work",
                    None,
                    yolo,
                    auto,
                    None,
                    None,
                    mode,
                );
                assert!(
                    !args
                        .windows(2)
                        .any(|w| w[0] == "--permission-mode" && w[1] == "ask"),
                    "SessionMode::{mode:?} yolo={yolo} auto={auto} must not emit --permission-mode ask: {args:?}"
                );
            }
        }
        let look = single_turn_args_full(
            "hi",
            "/tmp/work",
            None,
            true,
            true,
            None,
            None,
            SessionMode::Ask,
        );
        assert!(
            look.windows(2)
                .any(|w| w[0] == "--permission-mode" && w[1] == "default"),
            "btw (saved as ask) is --permission-mode default: {look:?}"
        );
        assert_eq!(PermissionMode::Ask.composer_headless_flags(), (false, false));
        assert_eq!(PermissionMode::Auto.composer_headless_flags(), (false, true));
        assert_eq!(
            PermissionMode::AlwaysApprove.composer_headless_flags(),
            (true, false)
        );
        assert!(PermissionMode::Ask.scheduled_args().is_empty());
        assert_eq!(
            PermissionMode::Auto.scheduled_args(),
            vec!["--permission-mode".to_string(), "auto".into()]
        );
        assert_eq!(
            PermissionMode::AlwaysApprove.scheduled_args(),
            vec!["--always-approve".to_string()]
        );
    }

    #[test]
    fn agent_args_yolo() {
        assert_eq!(
            agent_args(true, None),
            vec!["--no-auto-update", "agent", "--always-approve", "stdio"]
        );
        assert_eq!(
            agent_args(false, None),
            vec!["--no-auto-update", "agent", "stdio"]
        );
        assert_eq!(
            agent_args(false, Some("high")),
            vec![
                "--no-auto-update",
                "agent",
                "--reasoning-effort",
                "high",
                "stdio"
            ]
        );
        assert_eq!(
            agent_args_resume(false, Some("abc-123"), Some("xhigh")),
            vec![
                "--no-auto-update",
                "agent",
                "--reasoning-effort",
                "xhigh",
                "stdio"
            ]
        );
        assert!(
            !agent_args_resume(true, Some("abc-123"), None)
                .iter()
                .any(|a| a == "--resume"),
            "CLI --resume plus session/new mixed sessions"
        );
    }

    #[test]
    fn grok_auth_key_picks_the_login_token() {
        let raw = r#"{
            "https://auth.x.ai::one": {
                "auth_mode": "oidc",
                "expires_at": "2026-01-01T00:00:00Z",
                "key": "old-token"
            },
            "https://auth.x.ai::two": {
                "auth_mode": "oidc",
                "expires_at": "2026-12-01T00:00:00Z",
                "key": "fresh-token"
            }
        }"#;
        assert_eq!(parse_grok_auth_key(raw).as_deref(), Some("fresh-token"));
        assert!(parse_grok_auth_key("{}").is_none());
        assert!(parse_grok_auth_key("not-json").is_none());
        assert_eq!(
            parse_grok_auth_key(r#"{"access_token":"top-level"}"#).as_deref(),
            Some("top-level")
        );
    }

    fn should_write_cli_auth_raw(raw: &str) -> bool {
        grokhub_core::should_sync_cli_auth(parse_grok_auth_key(raw).is_some())
    }

    struct MergeOut {
        wrote: bool,
        body: String,
    }

    fn merge_and_decide(
        raw: &str,
        tokens: &grokhub_core::XaiOAuthTokens,
    ) -> Result<MergeOut, String> {
        if !should_write_cli_auth_raw(raw) {
            return Ok(MergeOut {
                wrote: false,
                body: raw.to_string(),
            });
        }
        Ok(MergeOut {
            wrote: true,
            body: grokhub_core::merge_cli_auth_json(raw, tokens)?,
        })
    }

    #[test]
    fn write_cli_auth_skips_when_login_exists() {
        let dir = std::env::temp_dir().join(format!("grokhub-auth-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("auth.json");
        std::fs::write(
            &path,
            r#"{
                "https://auth.x.ai::existing": {
                    "auth_mode": "oidc",
                    "key": "already-in"
                }
            }"#,
        )
        .unwrap();
        let tokens = grokhub_core::XaiOAuthTokens {
            access_token: "new-cabin".into(),
            refresh_token: Some("new-ref".into()),
            ..Default::default()
        };
        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(parse_grok_auth_key(&raw).is_some());
        assert!(
            !should_write_cli_auth_raw(&raw),
            "existing grok login must win"
        );
        let empty = merge_and_decide("", &tokens).unwrap();
        assert!(empty.wrote);
        assert!(empty.body.contains("new-cabin"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn hard_failure_codes_are_sticky() {
        assert!(is_cli_hard_failure(Some(-1073741515), ""));
        assert!(is_cli_hard_failure(Some(0xC0000135u32 as i32), ""));
        assert!(is_cli_hard_failure(Some(193), ""));
        assert!(is_cli_hard_failure(
            None,
            "The code execution cannot proceed because vcruntime140.dll was not found"
        ));
        assert!(!is_cli_hard_failure(Some(1), "usage: grok --help"));
        let src = include_str!("locate.rs");
        assert!(
            src.contains("SetErrorMode") && src.contains("SEM_FAILCRITICALERRORS")
                || src.contains("0x0001"),
            "Windows must silence the loader MessageBox: {src}"
        );
        assert!(
            src.contains("grok_marked_unusable") && src.contains("doctor_broken_hint"),
            "a broken grok.exe must not be spawned again: {src}"
        );
    }

    #[test]
    fn stub_mz_is_not_an_installed_cli() {
        let _lock = grok_env_test_lock();
        let dir = std::env::temp_dir().join(format!("grokhub-stub-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let stub = dir.join(if cfg!(windows) { "grok.exe" } else { "grok" });
        std::fs::write(&stub, if cfg!(windows) { &b"MZ\0\0"[..] } else { &b""[..] }).unwrap();
        assert!(
            !grok_bin_looks_complete(&stub),
            "a 4-byte MZ / empty file is not Grok Build"
        );
        assert!(!cli_install_should_skip(Some(&stub)));
        mark_grok_unusable(&stub, "dll was not found");
        assert!(grok_marked_unusable(&stub));
        assert!(!cli_install_should_skip(Some(&stub)));
        let prev = std::env::var_os("GROKHUB_GROK");
        std::env::set_var("GROKHUB_GROK", &stub);
        invalidate_grok_bin_cache();
        assert!(
            !grok_cli_known_good(),
            "a stub GROKHUB_GROK must not look ready"
        );
        match prev {
            Some(v) => std::env::set_var("GROKHUB_GROK", v),
            None => std::env::remove_var("GROKHUB_GROK"),
        }
        invalidate_grok_bin_cache();
        let (ok, text) = doctor_grok_line_blocking(Some(&stub));
        assert!(!ok, "{text}");
        assert!(
            text.contains("broken") || text.contains("x.ai/cli") || text.contains("Install"),
            "{text}"
        );
        clear_grok_unusable();
        invalidate_grok_bin_cache();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn runnable_cli_warms_known_good() {
        let _lock = grok_env_test_lock();
        let src = include_str!("locate.rs");
        assert!(
            src.contains("remember_doctor_result") && src.contains("doctor_ok_line"),
            "a live grok --version must warm doctor_line_cache: {src}"
        );
        let runnable = src
            .split("pub fn grok_cli_is_runnable(")
            .nth(1)
            .and_then(|s| s.split("pub fn grok_bin_looks_complete(").next())
            .expect("grok_cli_is_runnable");
        assert!(
            runnable.contains("remember_doctor_result"),
            "install skip / finish must mark grok_cli_known_good: {runnable}"
        );
        #[cfg(unix)]
        {
            let dir = std::env::temp_dir().join(format!("grokhub-warm-{}", std::process::id()));
            let _ = std::fs::create_dir_all(&dir);
            let bin = dir.join("grok");
            std::fs::write(&bin, "#!/bin/sh\necho '9.9.9-warm (deadbeef)'\n").unwrap();
            use std::os::unix::fs::PermissionsExt;
            let mut p = std::fs::metadata(&bin).unwrap().permissions();
            p.set_mode(0o755);
            std::fs::set_permissions(&bin, p).unwrap();
            let prev = std::env::var_os("GROKHUB_GROK");
            std::env::set_var("GROKHUB_GROK", &bin);
            invalidate_grok_bin_cache();
            assert!(grok_cli_is_runnable(&bin), "fake grok must run");
            assert!(
                grok_cli_known_good(),
                "successful grok --version must hide Install Grok Build CLI"
            );
            match prev {
                Some(v) => std::env::set_var("GROKHUB_GROK", v),
                None => std::env::remove_var("GROKHUB_GROK"),
            }
            invalidate_grok_bin_cache();
            let _ = std::fs::remove_dir_all(&dir);
        }
    }

    #[test]
    fn grok_cli_channel_reads_config_toml() {
        let _lock = grok_env_test_lock();
        let dir = std::env::temp_dir().join(format!("grokhub-ch-{}", std::process::id()));
        let _ = std::fs::create_dir_all(dir.join(".grok"));
        std::fs::write(dir.join(".grok").join("config.toml"), "[cli]\nchannel = \"stable\"\n")
            .unwrap();
        let prev = std::env::var_os("HOME");
        std::env::set_var("HOME", &dir);
        let bin = dir.join("missing-grok");
        let ch = grok_cli_channel(&bin);
        match prev {
            Some(v) => std::env::set_var("HOME", v),
            None => std::env::remove_var("HOME"),
        }
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(ch.as_deref(), Some("stable"));
        assert!(grokhub_core::should_switch_cli_to_alpha(ch.as_deref()));
        let src = include_str!("locate.rs");
        assert!(
            src.contains("parse_cli_config_channel")
                && src.contains("update")
                && src.contains("--check"),
            "channel probe must prefer config.toml then grok update --check --json: {src}"
        );
    }

    #[test]
    fn cabin_defaults_feed_single_turn_args() {
        use crate::protocol::{PermissionMode, SessionMode};

        let (always, auto) = PermissionMode::Auto.composer_headless_flags();
        assert!(!always && auto);
        let pinned = single_turn_args_full(
            "hi",
            "/tmp/work",
            None,
            always,
            auto,
            Some("grok-4.6"),
            Some("xhigh"),
            SessionMode::Chat,
        );
        assert!(
            pinned
                .windows(2)
                .any(|w| w[0] == "--model" && w[1] == "grok-4.6"),
            "{pinned:?}"
        );
        assert!(
            pinned
                .windows(2)
                .any(|w| w[0] == "--reasoning-effort" && w[1] == "xhigh"),
            "{pinned:?}"
        );
        assert!(
            pinned
                .windows(2)
                .any(|w| w[0] == "--permission-mode" && w[1] == "auto"),
            "{pinned:?}"
        );
        assert!(
            !pinned.iter().any(|a| a == "--always-approve"),
            "{pinned:?}"
        );

        let (always, auto) = PermissionMode::Ask.composer_headless_flags();
        assert!(!always && !auto);
        assert!(PermissionMode::Ask.uses_acp(), "Ask stays on the ACP path");
        assert!(!PermissionMode::Auto.uses_acp());
        assert!(!PermissionMode::AlwaysApprove.uses_acp());
        let ask = single_turn_args_full(
            "hi",
            "/tmp/work",
            None,
            always,
            auto,
            Some(grokhub_core::cabin_spawn_model("")),
            Some("high"),
            SessionMode::Chat,
        );
        assert_eq!(grokhub_core::cabin_spawn_model(""), "grok-4.7");
        assert!(
            ask.windows(2)
                .any(|w| w[0] == "--model" && w[1] == "grok-4.7"),
            "an empty Settings model pin still uses the cabin spawn model: {ask:?}"
        );
        assert!(
            ask.windows(2)
                .any(|w| w[0] == "--reasoning-effort" && w[1] == "high"),
            "{ask:?}"
        );
        assert!(
            !ask.iter().any(|a| a == "--always-approve"),
            "Ask must not send --always-approve: {ask:?}"
        );
        assert!(
            !ask.windows(2).any(|w| w[0] == "--permission-mode"),
            "Chat Ask does not put a permission flag on grok -p: {ask:?}"
        );

        let look = single_turn_args_full(
            "hi",
            "/tmp/work",
            None,
            false,
            true,
            Some("grok-4.7"),
            Some("low"),
            SessionMode::Ask,
        );
        assert!(
            look.windows(2)
                .any(|w| w[0] == "--permission-mode" && w[1] == "default"),
            "Questions stays look-only, not the invalid CLI value ask: {look:?}"
        );
        assert!(!look.iter().any(|a| a == "--always-approve"), "{look:?}");
    }

    #[test]
    fn ask_deny_keeps_a_prompt_that_reads_like_the_flag() {
        let args: Vec<String> = ["-p", "--always-approve", "--always-approve", "--cwd", "/w"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let out = with_ask_deny(args, true);
        assert_eq!(&out[..4], ["-p", "--always-approve", "--cwd", "/w"]);
    }

    #[test]
    fn ask_deny_fails_closed_when_nobody_can_answer() {
        let plain = single_turn_args_full(
            "hi",
            "/tmp/work",
            None,
            false,
            false,
            None,
            None,
            SessionMode::Chat,
        );
        assert_eq!(
            with_ask_deny(plain.clone(), false),
            plain,
            "deny false is a no-op"
        );

        let denied = with_ask_deny(plain.clone(), true);
        assert_eq!(
            denied.iter().filter(|a| *a == "--permission-mode").count(),
            1,
            "{denied:?}"
        );
        assert!(
            denied
                .windows(2)
                .any(|w| w[0] == "--permission-mode" && w[1] == "dontAsk"),
            "{denied:?}"
        );
        assert_eq!(
            ASK_DENY_RULES,
            [
                "Bash",
                "Edit",
                "Write",
                grokhub_core::DESKTOP_MCP_RULE,
            ]
            .as_slice()
        );
        for rule in ASK_DENY_RULES {
            assert!(
                denied.windows(2).any(|w| w[0] == "--deny" && w[1] == *rule),
                "missing --deny {rule}: {denied:?}"
            );
        }

        let plan = with_ask_deny(
            single_turn_args_full(
                "hi",
                "/tmp/work",
                None,
                true,
                true,
                None,
                None,
                SessionMode::Plan,
            ),
            true,
        );
        assert_eq!(
            plan.iter().filter(|a| *a == "--permission-mode").count(),
            1,
            "{plan:?}"
        );
        assert!(
            plan.windows(2)
                .any(|w| w[0] == "--permission-mode" && w[1] == "plan"),
            "Plan keeps its mode: {plan:?}"
        );
        assert!(!plan.iter().any(|a| a == "dontAsk"), "{plan:?}");
        assert!(
            plan.windows(2).any(|w| w[0] == "--deny" && w[1] == "Bash"),
            "{plan:?}"
        );
        assert!(
            plan.windows(2).any(|w| w[0] == "--deny" && w[1] == "Edit"),
            "{plan:?}"
        );
        assert!(
            plan.windows(2).any(|w| w[0] == "--deny" && w[1] == "Write"),
            "{plan:?}"
        );
        assert!(
            plan.windows(2)
                .any(|w| w[0] == "--deny" && w[1] == grokhub_core::DESKTOP_MCP_RULE),
            "{plan:?}"
        );

        let look = with_ask_deny(
            single_turn_args_full(
                "hi",
                "/tmp/work",
                None,
                true,
                true,
                None,
                None,
                SessionMode::Ask,
            ),
            true,
        );
        assert_eq!(
            look.iter().filter(|a| *a == "--permission-mode").count(),
            1,
            "{look:?}"
        );
        assert!(
            look.windows(2)
                .any(|w| w[0] == "--permission-mode" && w[1] == "default"),
            "look keeps default: {look:?}"
        );
        assert!(
            look.windows(2).any(|w| w[0] == "--deny" && w[1] == "Write"),
            "{look:?}"
        );
        assert!(
            look.windows(2)
                .any(|w| w[0] == "--deny" && w[1] == grokhub_core::DESKTOP_MCP_RULE),
            "{look:?}"
        );
        assert!(!look.iter().any(|a| a == "dontAsk"), "{look:?}");

        let mut yolo = plain;
        yolo.push("--always-approve".into());
        let kept = with_ask_deny(yolo.clone(), false);
        assert!(
            kept.iter().any(|a| a == "--always-approve"),
            "deny false leaves --always-approve: {kept:?}"
        );
        let stripped = with_ask_deny(yolo, true);
        assert!(
            !stripped.iter().any(|a| a == "--always-approve"),
            "{stripped:?}"
        );
        assert!(
            stripped
                .windows(2)
                .any(|w| w[0] == "--permission-mode" && w[1] == "dontAsk"),
            "{stripped:?}"
        );
    }

    #[test]
    fn desktop_mcp_register_uses_isolated_runner() {
        let add = desktop_mcp_add_argv("/usr/bin/grokhub");
        assert_eq!(
            add,
            [
                "mcp",
                "add",
                "grokhub-desktop",
                "--",
                "/usr/bin/grokhub",
                "--mcp-desktop",
            ]
        );
        assert_eq!(
            desktop_mcp_remove_argv(),
            ["mcp", "remove", "grokhub-desktop"]
        );
        let src = include_str!("locate.rs");
        let reg = src
            .split("pub fn register_desktop_mcp(")
            .nth(1)
            .and_then(|s| s.split("pub fn unregister_desktop_mcp(").next())
            .expect("register_desktop_mcp");
        assert!(
            reg.contains("grok_stdout_timeout") && !reg.contains("grok_user_stdout"),
            "desktop MCP add must use the cabin GROK_HOME runner: {reg}"
        );
        let unreg = src
            .split("pub fn unregister_desktop_mcp(")
            .nth(1)
            .and_then(|s| s.split("#[cfg(test)]").next())
            .expect("unregister_desktop_mcp");
        assert!(
            unreg.contains("grok_stdout_timeout") && !unreg.contains("grok_user_stdout"),
            "desktop MCP remove must use the cabin GROK_HOME runner: {unreg}"
        );
    }

    #[test]
    fn desktop_mcp_plan_and_btw_deny_even_when_allowed() {
        let plan = apply_desktop_spawn_args(
            vec!["--always-approve".into()],
            PermissionMode::AlwaysApprove,
            SessionMode::Plan,
            true,
        );
        assert!(
            plan.windows(2)
                .any(|w| w[0] == "--deny" && w[1] == grokhub_core::DESKTOP_MCP_RULE),
            "{plan:?}"
        );
        assert!(
            !plan
                .windows(2)
                .any(|w| w[0] == "--allow" && w[1] == grokhub_core::DESKTOP_MCP_RULE),
            "{plan:?}"
        );
        let look = apply_desktop_spawn_args(
            Vec::new(),
            PermissionMode::Auto,
            SessionMode::Ask,
            true,
        );
        assert!(
            look.windows(2)
                .any(|w| w[0] == "--deny" && w[1] == grokhub_core::DESKTOP_MCP_RULE),
            "{look:?}"
        );
        let allow = apply_desktop_spawn_args(
            Vec::new(),
            PermissionMode::Auto,
            SessionMode::Chat,
            true,
        );
        assert!(
            allow
                .windows(2)
                .any(|w| w[0] == "--allow" && w[1] == grokhub_core::DESKTOP_MCP_RULE),
            "{allow:?}"
        );
        let off = apply_desktop_spawn_args(
            vec![
                "--allow".into(),
                grokhub_core::DESKTOP_MCP_RULE.into(),
            ],
            PermissionMode::AlwaysApprove,
            SessionMode::Chat,
            false,
        );
        assert!(
            !off.windows(2)
                .any(|w| w[0] == "--allow" && w[1] == grokhub_core::DESKTOP_MCP_RULE),
            "{off:?}"
        );
        assert!(
            off.windows(2)
                .any(|w| w[0] == "--deny" && w[1] == grokhub_core::DESKTOP_MCP_RULE),
            "{off:?}"
        );
    }
}