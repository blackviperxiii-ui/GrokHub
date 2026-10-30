//! llama.cpp `llama-server` sidecar for the on-device runtime (offline card PR A).
//!
//! The child listens on **127.0.0.1 only**, with a random free port and a
//! bearer token. Health is polled until ready; a dead child can be restarted.
//! Production builds resolve the binary and a downloaded GGUF; tests drive a
//! fake server through [`HttpLocalRuntime`](super::local_http::HttpLocalRuntime).

use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use rand_token::random_token;

use super::local_autoconfig::{self as autoconfig, LaunchArgs, Retry};
use super::local_gov;
use super::local_health as health;

/// How long [`Sidecar::wait_healthy`] waits for `/health`.
pub const HEALTH_TIMEOUT: Duration = Duration::from_secs(30);
/// Gap between health polls.
pub const HEALTH_POLL: Duration = Duration::from_millis(50);

/// Resolved paths for a production sidecar launch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SidecarPaths {
    pub binary: PathBuf,
    pub model: PathBuf,
}

/// Live child: loopback host, port, token.
///
/// `Debug` redacts the bearer token so it never lands in logs.
pub struct Sidecar {
    child: Mutex<Child>,
    pub host: String,
    pub port: u16,
    pub token: String,
    pub binary: PathBuf,
    pub model: PathBuf,
    restarts: AtomicU64,
    /// Unix seconds of the last successful use (idle unload).
    last_used_secs: AtomicU64,
    idle_unload_secs: u64,
    /// The auto-configured flags; `None` runs the one-shot `local_perf` defaults.
    launch: Option<LaunchArgs>,
}

impl std::fmt::Debug for Sidecar {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Sidecar")
            .field("host", &self.host)
            .field("port", &self.port)
            .field("token", &"[redacted]")
            .field("binary", &self.binary)
            .field("model", &self.model)
            .field("restarts", &self.restart_count())
            .field("idle_unload_secs", &self.idle_unload_secs)
            .finish()
    }
}

fn unix_secs() -> u64 {
    unix_ms() / 1_000
}

pub(crate) fn unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Requests to the sidecar in flight; the governor restarts or stops it only at 0.
static IN_FLIGHT: AtomicUsize = AtomicUsize::new(0);

/// Unix seconds when the last request finished. Idle time counts from the
/// later of this and the last start, so a long request doesn't end idle.
static LAST_DONE_SECS: AtomicU64 = AtomicU64::new(0);

/// Held for the length of one request (see [`busy`]).
pub struct Busy(());

impl Drop for Busy {
    fn drop(&mut self) {
        LAST_DONE_SECS.store(unix_secs(), Ordering::SeqCst);
        IN_FLIGHT.fetch_sub(1, Ordering::SeqCst);
    }
}

/// Marks a request in flight until the guard drops.
pub fn busy() -> Busy {
    IN_FLIGHT.fetch_add(1, Ordering::SeqCst);
    Busy(())
}

pub fn in_flight() -> usize {
    IN_FLIGHT.load(Ordering::SeqCst)
}

impl Sidecar {
    /// Base URL for OpenAI-compatible calls (`http://127.0.0.1:<port>`).
    pub fn base_url(&self) -> String {
        format!("http://{}:{}", self.host, self.port)
    }

    /// Restarts so far (crash recovery).
    pub fn restart_count(&self) -> u64 {
        self.restarts.load(Ordering::SeqCst)
    }

    /// `true` when `/health` answers 200 with the bearer token.
    pub fn healthy(&self) -> bool {
        health_ok(
            &self.host,
            self.port,
            &self.token,
            Duration::from_millis(400),
        )
    }

    /// Block until healthy or [`HEALTH_TIMEOUT`].
    pub fn wait_healthy(&self) -> Result<(), String> {
        let deadline = Instant::now() + HEALTH_TIMEOUT;
        while Instant::now() < deadline {
            if self.healthy() {
                return Ok(());
            }
            if !self.alive() {
                return Err("local model server stopped before it was ready".into());
            }
            std::thread::sleep(HEALTH_POLL);
        }
        Err("local model server did not become healthy in time".into())
    }

    /// Child still running.
    pub fn alive(&self) -> bool {
        let mut child = self.child.lock().expect("sidecar lock");
        match child.try_wait() {
            Ok(None) => true,
            Ok(Some(_)) | Err(_) => false,
        }
    }

    /// If the child has exited, spawn it again on the same port and token.
    pub fn ensure_alive(&self) -> Result<(), String> {
        if self.alive() {
            return Ok(());
        }
        let child = spawn_llama(
            &self.binary,
            &self.model,
            &self.host,
            self.port,
            &self.token,
            self.launch.as_ref(),
        )?;
        *self.child.lock().expect("sidecar lock") = child;
        self.restarts.fetch_add(1, Ordering::SeqCst);
        self.wait_healthy()
    }

    /// This launch runs a draft model (speculative decoding).
    pub fn drafting(&self) -> bool {
        self.launch.as_ref().is_some_and(|l| l.draft.is_some())
    }

    /// The child's process id, so the governor can leave its load out.
    pub fn pid(&self) -> u32 {
        self.child.lock().expect("sidecar lock").id()
    }

    /// Stop the child (best effort). Called from [`Drop`].
    pub fn stop(&self) {
        let mut child = self.child.lock().expect("sidecar lock");
        let _ = child.kill();
        let _ = child.wait();
    }
}

impl Drop for Sidecar {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Pick a free port on 127.0.0.1 (released before the child binds it).
pub fn free_loopback_port() -> Result<u16, String> {
    let listener =
        TcpListener::bind(("127.0.0.1", 0)).map_err(|e| format!("loopback bind: {e}"))?;
    let port = listener
        .local_addr()
        .map_err(|e| format!("loopback addr: {e}"))?
        .port();
    Ok(port)
}

/// Assert a listener is on loopback only (contract for tests and docs).
pub fn is_loopback_host(host: &str) -> bool {
    matches!(host, "127.0.0.1" | "localhost" | "::1" | "[::1]")
}

/// Command line for llama-server: host locked to loopback, plus
/// hardware-aware perf flags from [`super::local_perf`]. The API key never
/// goes on the command line: `llama_command` passes it as `LLAMA_API_KEY`
/// in the child's environment, out of the process list.
pub fn llama_args(model: &Path, host: &str, port: u16) -> Vec<String> {
    llama_args_with_perf(model, host, port, &super::local_perf::current_defaults())
}

/// Same as [`llama_args`] with an explicit [`super::local_perf::RuntimeDefaults`].
pub fn llama_args_with_perf(
    model: &Path,
    host: &str,
    port: u16,
    perf: &super::local_perf::RuntimeDefaults,
) -> Vec<String> {
    let mut args = base_args(model, host, port);
    args.extend(super::local_perf::llama_perf_args(perf));
    args
}

fn base_args(model: &Path, host: &str, port: u16) -> Vec<String> {
    vec![
        "-m".into(),
        model.display().to_string(),
        "--host".into(),
        host.into(),
        "--port".into(),
        port.to_string(),
    ]
}

/// Same as [`llama_args`] with auto-configured flags in place of the defaults.
pub fn llama_args_with_launch(
    model: &Path,
    host: &str,
    port: u16,
    launch: &LaunchArgs,
) -> Vec<String> {
    let mut args = base_args(model, host, port);
    args.extend(launch.to_args());
    args
}

/// Where the server's load log goes, next to the binary (read for out-of-memory).
pub fn server_log_path(binary: &Path) -> PathBuf {
    binary.with_file_name("llama-server.log")
}

fn llama_command(binary: &Path, args: &[String], token: &str) -> Command {
    let mut cmd = Command::new(binary);
    // llama-server reads this from its child environment, not the process list.
    cmd.args(args).env("LLAMA_API_KEY", token);
    cmd
}

fn spawn_llama(
    binary: &Path,
    model: &Path,
    host: &str,
    port: u16,
    token: &str,
    launch: Option<&LaunchArgs>,
) -> Result<Child, String> {
    if !is_loopback_host(host) {
        return Err(format!("local model server must bind loopback, not {host}"));
    }
    // Only the known server name — env overrides must still be llama-server.
    let name = binary.file_name().and_then(|n| n.to_str()).unwrap_or("");
    if name != llama_server_name() {
        return Err(format!(
            "local model binary must be named {}, not {name}",
            llama_server_name()
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(binary)
            .map_err(|e| format!("local model binary: {e}"))?
            .permissions()
            .mode();
        if mode & 0o111 == 0 {
            return Err(format!(
                "local model binary is not executable: {}",
                binary.display()
            ));
        }
    }
    let args = match launch {
        Some(l) => llama_args_with_launch(model, host, port, l),
        None => llama_args(model, host, port),
    };
    // The load log names out-of-memory failures; without a file it is dropped.
    let log = std::fs::File::create(server_log_path(binary))
        .map(Stdio::from)
        .unwrap_or_else(|_| Stdio::null());
    let mut cmd = llama_command(binary, &args, token);
    cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(log);
    grokhub_core::proc_util::hide_windows_console(&mut cmd);
    cmd.spawn()
        .map_err(|e| format!("could not start local model server: {e}"))
}

/// Start llama-server on a fresh loopback port.
pub fn start(paths: &SidecarPaths) -> Result<Sidecar, String> {
    if !paths.binary.is_file() {
        return Err(format!(
            "local model binary missing: {}. In Settings, turn on Local AI or use Get engine to download it.",
            paths.binary.display()
        ));
    }
    if !paths.model.is_file() {
        return Err(format!(
            "local model file missing: {}",
            paths.model.display()
        ));
    }
    let host = "127.0.0.1".to_string();
    let planned = autoconfig::launch_for(&paths.binary, &paths.model);
    let mut launch = planned.as_ref().map(|(l, _, _)| l.clone());
    let idle = super::local_perf::current_defaults().idle_unload_secs;
    let mut attempt: u8 = 0;
    loop {
        let port = free_loopback_port()?;
        let token = random_token();
        let child = spawn_llama(
            &paths.binary,
            &paths.model,
            &host,
            port,
            &token,
            launch.as_ref(),
        )?;
        let side = Sidecar {
            child: Mutex::new(child),
            host: host.clone(),
            port,
            token,
            binary: paths.binary.clone(),
            model: paths.model.clone(),
            restarts: AtomicU64::new(0),
            last_used_secs: AtomicU64::new(unix_secs()),
            idle_unload_secs: idle,
            launch: launch.clone(),
        };
        let err = match side.wait_healthy() {
            Ok(()) => {
                if let (Some(l), Some((_, key, dir))) = (launch.as_ref(), planned.as_ref()) {
                    if attempt > 0 {
                        let tuned = autoconfig::Tuned {
                            ngl: l.ngl,
                            n_cpu_moe: l.n_cpu_moe,
                            ctx: l.ctx,
                            kv: l.kv,
                        };
                        let _ = autoconfig::save_tuned(dir, key, &tuned);
                    }
                }
                return Ok(side);
            }
            Err(e) => e,
        };
        side.stop();
        let log = std::fs::read_to_string(server_log_path(&paths.binary)).unwrap_or_default();
        let Some(prev) = launch.as_ref().filter(|_| autoconfig::oom_in_log(&log)) else {
            return Err(err);
        };
        attempt += 1;
        match autoconfig::after_oom(prev, attempt) {
            Retry::Again(next) => launch = Some(next),
            Retry::SmallerModel => {
                let smaller = autoconfig::catalog_model_for(&paths.model)
                    .and_then(|m| autoconfig::next_smaller(m.id))
                    .map(|m| format!(" Try {} in Settings → Local model.", m.name))
                    .unwrap_or_default();
                return Err(format!(
                    "the local model ran out of memory even with fewer GPU layers.{smaller}"
                ));
            }
        }
    }
}

/// GET `/health` with `Authorization: Bearer <token>`.
pub fn health_ok(host: &str, port: u16, token: &str, timeout: Duration) -> bool {
    if !is_loopback_host(host) {
        return false;
    }
    let url = format!("http://{host}:{port}/health");
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(timeout)
        .timeout_read(timeout)
        .build();
    match agent
        .get(&url)
        .set("Authorization", &format!("Bearer {token}"))
        .call()
    {
        Ok(resp) => resp.status() == 200,
        Err(_) => false,
    }
}

/// Resolve binary + GGUF from the environment or the usual install layout.
///
/// - `GROKHUB_LLAMA_SERVER`: path to `llama-server`
/// - `GROKHUB_LOCAL_MODEL`: path to a `.gguf`
/// - binary else: `{config}/bin/llama-server`, else `llama-server` on `PATH`
/// - model else: the pick in `{config}/local_ai.json` (`localModel.selected`,
///   a model file's path or a downloaded catalog id), then
///   `model_download::installed` under `~/GrokHub/models` (or
///   `{config}/models`), then the first `*.gguf` in those folders
pub fn resolve_paths(config_dir: &Path) -> Option<SidecarPaths> {
    let binary = find_binary(config_dir)?;
    let model = find_model(config_dir)?;
    Some(SidecarPaths { binary, model })
}

/// `llama-server` from `GROKHUB_LLAMA_SERVER`, `{config}/bin`, then `PATH`.
pub fn find_binary(config_dir: &Path) -> Option<PathBuf> {
    std::env::var_os("GROKHUB_LLAMA_SERVER")
        .map(PathBuf::from)
        .filter(|p| p.is_file())
        .or_else(|| {
            let p = config_dir.join("bin").join(llama_server_name());
            p.is_file().then_some(p)
        })
        .or_else(|| grokhub_core::runtime_download::find_on_path(llama_server_name()))
}

/// The model file the runtime would load (see [`resolve_paths`]).
pub fn find_model(config_dir: &Path) -> Option<PathBuf> {
    std::env::var_os("GROKHUB_LOCAL_MODEL")
        .map(PathBuf::from)
        .filter(|p| p.is_file())
        .or_else(|| resolve_model_file(config_dir))
}

/// The local engine's state, as Settings shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Runtime {
    /// No `llama-server` in `GROKHUB_LLAMA_SERVER`, `{config}/bin` or `PATH`.
    Missing,
    /// The engine is there but no model file is.
    NoModel,
    Ready,
    Loading,
    Running,
    /// The last start failed; cleared by a good start or a stop.
    Error(String),
}

/// Settings' runtime line for `r`.
pub fn runtime_label(r: &Runtime) -> String {
    match r {
        Runtime::Missing => {
            "Runtime missing: llama-server isn't installed. Turning on Local AI downloads it."
                .into()
        }
        Runtime::NoModel => "Runtime ready, no model yet: pick or download one below.".into(),
        Runtime::Ready => "Runtime ready".into(),
        Runtime::Loading => "Loading the local model…".into(),
        Runtime::Running => "Runtime ready · running".into(),
        Runtime::Error(why) => format!("Error: {why}"),
    }
}

static STARTING: AtomicBool = AtomicBool::new(false);
static LAST_ERROR: Mutex<Option<String>> = Mutex::new(None);

fn set_last_error(e: Option<String>) {
    *LAST_ERROR.lock().expect("sidecar error") = e;
}

/// Where the local engine stands now. Never waits on a starting sidecar.
pub fn runtime(config_dir: &Path) -> Runtime {
    if STARTING.load(Ordering::SeqCst) {
        return Runtime::Loading;
    }
    if find_binary(config_dir).is_none() {
        return Runtime::Missing;
    }
    if find_model(config_dir).is_none() {
        return Runtime::NoModel;
    }
    if let Some(e) = LAST_ERROR.lock().expect("sidecar error").clone() {
        return Runtime::Error(e);
    }
    match global_slot().try_lock() {
        // `alive` asks the OS (and reaps a child that already exited), so a
        // stopped or killed server never reads as running.
        Ok(slot) => match slot.as_ref() {
            Some(side) if side.alive() => Runtime::Running,
            _ => Runtime::Ready,
        },
        Err(_) => Runtime::Loading,
    }
}

fn user_models_dir() -> Option<PathBuf> {
    Some(
        grokhub_core::paths::user_home()?
            .join("GrokHub")
            .join("models"),
    )
}

/// The model picked in Settings → Local AI, when its file is on disk.
pub fn picked_model(config_dir: &Path) -> Option<PathBuf> {
    let text = std::fs::read_to_string(config_dir.join("local_ai.json")).ok()?;
    let v: serde_json::Value = serde_json::from_str(&text).ok()?;
    let pick = v.get("localModel")?.get("selected")?.as_str()?.trim();
    if pick.is_empty() {
        return None;
    }
    let path = PathBuf::from(pick);
    if path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("gguf"))
    {
        return path.is_file().then_some(path);
    }
    let m = grokhub_core::local_setup::model_by_id(pick)?;
    [user_models_dir(), Some(config_dir.join("models"))]
        .into_iter()
        .flatten()
        .map(|dir| grokhub_core::model_download::final_path(&dir, m))
        .find(|p| p.is_file())
}

/// Record `pick` (a catalog id or a model file's path) as the Settings pick in
/// `{config}/local_ai.json`, keeping the file's other keys.
pub fn set_pick(config_dir: &Path, pick: &str) -> Result<(), String> {
    let path = config_dir.join("local_ai.json");
    let mut v: serde_json::Value = std::fs::read_to_string(&path)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .filter(serde_json::Value::is_object)
        .unwrap_or_else(|| serde_json::json!({}));
    v["localModel"] = serde_json::json!({ "selected": pick });
    let text = serde_json::to_string_pretty(&v).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(config_dir).map_err(|e| e.to_string())?;
    std::fs::write(&path, format!("{text}\n")).map_err(|e| format!("Couldn't save the pick: {e}"))
}

fn resolve_model_file(config_dir: &Path) -> Option<PathBuf> {
    if let Some(p) = picked_model(config_dir) {
        return Some(p);
    }
    let dirs = [user_models_dir(), Some(config_dir.join("models"))];
    for dir in dirs.into_iter().flatten() {
        if let Some(rec) = grokhub_core::model_download::installed(&dir) {
            let path = dir.join(&rec.file);
            if path.is_file() {
                return Some(path);
            }
        }
        if let Some(p) = first_gguf(&dir) {
            return Some(p);
        }
    }
    None
}

fn llama_server_name() -> &'static str {
    if cfg!(windows) {
        "llama-server.exe"
    } else {
        "llama-server"
    }
}

fn first_gguf(dir: &Path) -> Option<PathBuf> {
    let mut entries: Vec<_> = std::fs::read_dir(dir)
        .ok()?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("gguf"))
        .collect();
    entries.sort();
    entries.into_iter().next()
}

/// Process-wide sidecar. Started lazily when paths resolve.
static GLOBAL: OnceLock<Mutex<Option<Sidecar>>> = OnceLock::new();

fn global_slot() -> &'static Mutex<Option<Sidecar>> {
    GLOBAL.get_or_init(|| Mutex::new(None))
}

/// Ensure a healthy global sidecar when assets exist under `config_dir`.
pub fn ensure_global(config_dir: &Path) -> Result<(), String> {
    acquire(config_dir).map(drop)
}

/// [`ensure_global`] for one request: the [`Busy`] guard is taken while the
/// slot is still locked, so the governor can't stop the sidecar between the
/// start and the request.
///
/// While the governor is at Unload this stops nothing mid-request and starts
/// nothing; a restart the governor asked for happens here when no other
/// request is in flight; a crash-loop breaker refuses starts after 3 failures
/// and drops a dead sidecar from the slot.
pub fn acquire(config_dir: &Path) -> Result<Busy, String> {
    tick_idle_unload();
    if local_gov::paused() {
        tick_governor();
        return Err(format!(
            "the on-device model is paused because {}",
            local_gov::reason()
        ));
    }
    let paths = match runtime(config_dir) {
        r @ (Runtime::Missing | Runtime::NoModel) => {
            let why = runtime_label(&r);
            health::log_to(config_dir, health::start_failure(&why), Some(&why), None);
            return Err(why);
        }
        _ => resolve_paths(config_dir).ok_or_else(|| runtime_label(&Runtime::Missing))?,
    };
    let mut slot = global_slot().lock().expect("sidecar global");
    if local_gov::restart_due() && in_flight() == 0 {
        if let Some(side) = slot.take() {
            side.stop();
        }
        local_gov::clear_restart();
    }
    if let Some(side) = slot.as_ref() {
        if !side.alive() {
            if let Err(e) = local_gov::may_start(unix_ms()) {
                if let Some(dead) = slot.take() {
                    dead.stop();
                }
                return Err(e);
            }
            local_gov::start_failed(unix_ms());
        }
        side.ensure_alive()?;
        side.last_used_secs.store(unix_secs(), Ordering::SeqCst);
        return Ok(busy());
    }
    local_gov::may_start(unix_ms())?;
    STARTING.store(true, Ordering::SeqCst);
    let began = Instant::now();
    let started = start(&paths);
    STARTING.store(false, Ordering::SeqCst);
    match started {
        Ok(side) => {
            let name = paths.model.file_name().map(|n| n.to_string_lossy());
            let ms = u64::try_from(began.elapsed().as_millis()).unwrap_or(u64::MAX);
            health::log_to(config_dir, "start", name.as_deref(), Some(ms));
            set_last_error(None);
            *slot = Some(side);
            Ok(busy())
        }
        Err(e) => {
            local_gov::start_failed(unix_ms());
            health::log_to(config_dir, health::start_failure(&e), Some(&e), None);
            set_last_error(Some(e.clone()));
            Err(e)
        }
    }
}

/// The governor's sample tick: between requests, stop the sidecar when the
/// level is Unload or a restart is due (the next request starts it with the
/// new launch args).
pub fn tick_governor() {
    if in_flight() > 0 || !(local_gov::paused() || local_gov::restart_due()) {
        return;
    }
    let mut slot = global_slot().lock().expect("sidecar global");
    // A request may have taken its guard while this waited for the lock.
    if in_flight() > 0 {
        return;
    }
    if let Some(side) = slot.take() {
        side.stop();
        health::log("stop", Some("the governor paused or restarted it"));
    }
    local_gov::clear_restart();
}

/// Process id of the running sidecar.
pub fn global_pid() -> Option<u32> {
    let slot = global_slot().lock().expect("sidecar global");
    slot.as_ref().map(Sidecar::pid)
}

/// Base URL + token for the global sidecar, when running.
pub fn global_endpoint() -> Option<(String, String)> {
    tick_idle_unload();
    let slot = global_slot().lock().expect("sidecar global");
    slot.as_ref().map(|s| (s.base_url(), s.token.clone()))
}

/// Mark the global sidecar as just used (keeps it warm).
pub fn touch_global() {
    let slot = global_slot().lock().expect("sidecar global");
    if let Some(side) = slot.as_ref() {
        side.last_used_secs.store(unix_secs(), Ordering::SeqCst);
    }
}

/// Stop the sidecar when it has been idle longer than its unload timeout.
///
/// Idle means no request in flight and none started or finished within the
/// timeout, so a long request is never cut off and the next one doesn't
/// reload a model that was just stopped.
pub fn tick_idle_unload() {
    if in_flight() > 0 {
        return;
    }
    let mut slot = global_slot().lock().expect("sidecar global");
    let Some(side) = slot.as_ref() else {
        return;
    };
    // A request may have taken its guard while this waited for the lock.
    if in_flight() > 0 {
        return;
    }
    let last = side
        .last_used_secs
        .load(Ordering::SeqCst)
        .max(LAST_DONE_SECS.load(Ordering::SeqCst));
    if super::local_perf::idle_unload_due(last, unix_secs(), side.idle_unload_secs) {
        if let Some(side) = slot.take() {
            side.stop();
            health::log("stop", Some("idle"));
        }
    }
}

/// Stop the global sidecar (tests / quit).
pub fn stop_global() {
    set_last_error(None);
    let mut slot = global_slot().lock().expect("sidecar global");
    if let Some(side) = slot.take() {
        side.stop();
        health::log("stop", Some("switched model, new engine or quit"));
    }
}

/// Whether production assets for a sidecar are present (binary + GGUF).
pub fn assets_present(config_dir: &Path) -> bool {
    resolve_paths(config_dir).is_some()
}

/// 128-bit bearer token from the process CSPRNG (`ring`), hex-encoded.
mod rand_token {
    use ring::rand::{SecureRandom, SystemRandom};

    pub fn random_token() -> String {
        let mut bytes = [0u8; 16];
        let _ = SystemRandom::new().fill(&mut bytes);
        let mut out = String::with_capacity(32);
        for b in bytes {
            out.push_str(&format!("{b:02x}"));
        }
        out
    }
}

/// Minimal HTTP peer for contract tests: answers `/health` and
/// `/v1/chat/completions` on 127.0.0.1, optional exit after N completions.
#[cfg(test)]
pub mod fake_server {
    use super::*;
    use std::io::{Read, Write};
    use std::net::{Shutdown, TcpStream};
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering as AtomicOrdering};
    use std::thread::JoinHandle;

    pub struct FakeServer {
        pub host: String,
        pub port: u16,
        pub token: String,
        stop: std::sync::Arc<AtomicBool>,
        hits: std::sync::Arc<AtomicUsize>,
        join: Option<JoinHandle<()>>,
    }

    impl FakeServer {
        pub fn start(exit_after_completions: Option<usize>) -> Self {
            let listener = TcpListener::bind(("127.0.0.1", 0)).expect("fake bind");
            let port = listener.local_addr().expect("addr").port();
            let token = random_token();
            let stop = std::sync::Arc::new(AtomicBool::new(false));
            let hits = std::sync::Arc::new(AtomicUsize::new(0));
            let stop_t = stop.clone();
            let hits_t = hits.clone();
            let token_t = token.clone();
            let join = std::thread::spawn(move || {
                serve_loop(listener, token_t, stop_t, hits_t, exit_after_completions)
            });
            Self {
                host: "127.0.0.1".into(),
                port,
                token,
                stop,
                hits,
                join: Some(join),
            }
        }

        pub fn base_url(&self) -> String {
            format!("http://{}:{}", self.host, self.port)
        }

        pub fn completion_hits(&self) -> usize {
            self.hits.load(AtomicOrdering::SeqCst)
        }
    }

    impl Drop for FakeServer {
        fn drop(&mut self) {
            self.stop.store(true, AtomicOrdering::SeqCst);
            let _ = TcpStream::connect((self.host.as_str(), self.port)).map(|s| {
                let _ = s.shutdown(Shutdown::Both);
            });
            if let Some(j) = self.join.take() {
                let _ = j.join();
            }
        }
    }

    fn serve_loop(
        listener: TcpListener,
        token: String,
        stop: std::sync::Arc<AtomicBool>,
        hits: std::sync::Arc<AtomicUsize>,
        exit_after: Option<usize>,
    ) {
        listener.set_nonblocking(true).ok();
        while !stop.load(AtomicOrdering::SeqCst) {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    // Accepted sockets inherit nonblocking on Windows; force blocking
                    // or the first read can WouldBlock and we drop with no response
                    // (client sees WSAECONNABORTED / os error 10053).
                    stream.set_nonblocking(false).ok();
                    let _ = handle_one(&mut stream, &token, &hits);
                    if let Some(n) = exit_after {
                        if hits.load(AtomicOrdering::SeqCst) >= n {
                            break;
                        }
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(5));
                }
                Err(_) => break,
            }
        }
    }

    fn read_exactish(stream: &mut TcpStream, chunk: &mut [u8]) -> Result<usize, ()> {
        for _ in 0..50 {
            match stream.read(chunk) {
                Ok(n) => return Ok(n),
                Err(e)
                    if e.kind() == std::io::ErrorKind::WouldBlock
                        || e.kind() == std::io::ErrorKind::Interrupted =>
                {
                    // Should be rare after set_nonblocking(false); retry briefly.
                    std::thread::sleep(Duration::from_millis(2));
                }
                Err(_) => return Err(()),
            }
        }
        Err(())
    }

    /// Read one HTTP/1.1 request (headers + Content-Length body). Windows often
    /// delivers headers and body in separate TCP packets; a single `read` is not enough.
    fn read_http_request(stream: &mut TcpStream) -> Result<String, ()> {
        stream
            .set_read_timeout(Some(Duration::from_millis(2_000)))
            .ok();
        let mut raw = Vec::with_capacity(4096);
        let mut chunk = [0u8; 4096];
        let header_end = loop {
            let n = read_exactish(stream, &mut chunk)?;
            if n == 0 {
                break None;
            }
            raw.extend_from_slice(&chunk[..n]);
            if let Some(pos) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
                break Some(pos + 4);
            }
            if raw.len() > 256 * 1024 {
                return Err(());
            }
        };
        let Some(header_end) = header_end else {
            return Ok(String::from_utf8_lossy(&raw).into_owned());
        };
        let headers = String::from_utf8_lossy(&raw[..header_end]);
        let content_len = headers
            .lines()
            .find_map(|l| {
                let (k, v) = l.split_once(':')?;
                if k.eq_ignore_ascii_case("content-length") {
                    v.trim().parse::<usize>().ok()
                } else {
                    None
                }
            })
            .unwrap_or(0);
        while raw.len() < header_end + content_len {
            let n = read_exactish(stream, &mut chunk)?;
            if n == 0 {
                break;
            }
            raw.extend_from_slice(&chunk[..n]);
            if raw.len() > 256 * 1024 {
                return Err(());
            }
        }
        Ok(String::from_utf8_lossy(&raw).into_owned())
    }

    fn handle_one(
        stream: &mut TcpStream,
        token: &str,
        hits: &std::sync::Arc<AtomicUsize>,
    ) -> Result<(), ()> {
        let req = read_http_request(stream)?;
        let first = req.lines().next().unwrap_or("");
        let auth_ok = req
            .lines()
            .any(|l| l.eq_ignore_ascii_case(&format!("Authorization: Bearer {token}")));
        if first.starts_with("GET /health") {
            let body = if auth_ok { "ok" } else { "unauthorized" };
            let code = if auth_ok {
                "200 OK"
            } else {
                "401 Unauthorized"
            };
            write_resp(stream, code, body);
            return Ok(());
        }
        if first.starts_with("POST /v1/chat/completions") {
            if !auth_ok {
                write_resp(stream, "401 Unauthorized", r#"{"error":"bad token"}"#);
                return Ok(());
            }
            hits.fetch_add(1, AtomicOrdering::SeqCst);
            // Match helper prompts from the body (must be fully read — see read_http_request).
            let inner = if req.contains("\"summary\"") || req.contains("at most") {
                serde_json::json!({"summary": "fake summary"})
            } else if req.contains("difficulty") {
                serde_json::json!({"d": 0.5})
            } else if req.contains("\"role\":\"user\"") && !req.contains("JSON object") {
                // plain chat: return text, not a helper object
                serde_json::Value::String("hello from local".into())
            } else {
                serde_json::json!({"label": "routine", "score": 0.91})
            };
            let content = match inner {
                serde_json::Value::String(s) => s,
                other => other.to_string(),
            };
            let body = serde_json::json!({
                "choices": [{"message": {"role": "assistant", "content": content}}],
                "usage": {"prompt_tokens": 1, "completion_tokens": 1}
            })
            .to_string();
            write_resp(stream, "200 OK", &body);
            return Ok(());
        }
        write_resp(stream, "404 Not Found", "missing");
        Ok(())
    }

    fn write_resp(stream: &mut TcpStream, status: &str, body: &str) {
        let resp = format!(
            "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        let _ = stream.write_all(resp.as_bytes());
        let _ = stream.flush();
        // Prefer FIN over RST so ureq finishes reading the status line on Windows.
        let _ = stream.shutdown(Shutdown::Write);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A long-running stand-in child (not llama-server) in a [`Sidecar`].
    fn idle_child_sidecar(idle_unload_secs: u64, last_used_secs: u64) -> Sidecar {
        #[cfg(windows)]
        let child = Command::new("ping")
            .args(["-n", "60", "127.0.0.1"])
            .stdout(Stdio::null())
            .spawn()
            .unwrap();
        #[cfg(not(windows))]
        let child = Command::new("sleep").arg("60").spawn().unwrap();
        Sidecar {
            child: Mutex::new(child),
            host: "127.0.0.1".into(),
            port: 9,
            token: "t".into(),
            binary: PathBuf::from("llama-server"),
            model: PathBuf::from("m.gguf"),
            restarts: AtomicU64::new(0),
            last_used_secs: AtomicU64::new(last_used_secs),
            idle_unload_secs,
            launch: None,
        }
    }

    #[test]
    fn a_server_that_exited_reads_ready_and_is_reaped() {
        let dir = std::env::temp_dir().join(format!("gh-sidecar-dead-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("bin")).unwrap();
        std::fs::create_dir_all(dir.join("models")).unwrap();
        std::fs::write(dir.join("bin").join(llama_server_name()), b"x").unwrap();
        std::fs::write(dir.join("models").join("a.gguf"), b"gguf").unwrap();
        stop_global();
        *global_slot().lock().unwrap() = Some(idle_child_sidecar(600, unix_secs()));
        assert_eq!(runtime(&dir), Runtime::Running);
        // Killed from outside (a shell `kill`, a crash): the OS has it as exited
        // but nobody has waited on it yet.
        {
            let slot = global_slot().lock().unwrap();
            let mut child = slot.as_ref().unwrap().child.lock().unwrap();
            child.kill().unwrap();
            let deadline = Instant::now() + Duration::from_secs(10);
            #[cfg(unix)]
            {
                let pid = child.id();
                while std::fs::read_to_string(format!("/proc/{pid}/stat"))
                    .is_ok_and(|st| !st.contains(") Z "))
                    && Instant::now() < deadline
                {
                    std::thread::sleep(Duration::from_millis(20));
                }
            }
            let _ = deadline;
        }
        assert_eq!(runtime(&dir), Runtime::Ready);
        #[cfg(target_os = "linux")]
        {
            let pid = global_pid().unwrap();
            assert!(
                std::fs::metadata(format!("/proc/{pid}")).is_err(),
                "the status read must reap the exited child, not leave it defunct"
            );
        }
        stop_global();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn idle_unload_waits_for_requests_in_flight_and_counts_from_the_last_one_done() {
        stop_global();
        let now = unix_secs();
        *global_slot().lock().unwrap() = Some(idle_child_sidecar(600, now - 1_000));
        LAST_DONE_SECS.store(now - 1_000, Ordering::SeqCst);
        let held = busy();
        tick_idle_unload();
        assert!(
            global_pid().is_some(),
            "a request in flight is never cut off by the idle rule"
        );
        drop(held);
        tick_idle_unload();
        assert!(
            global_pid().is_some(),
            "a request that just finished keeps the model warm for the timeout"
        );
        LAST_DONE_SECS.store(now - 1_000, Ordering::SeqCst);
        tick_idle_unload();
        assert_eq!(global_pid(), None, "idle past the timeout unloads");
    }
    use crate::route::local::LocalRuntime;
    use fake_server::FakeServer;

    #[test]
    fn llama_args_pin_loopback_without_api_key() {
        let args = llama_args(Path::new("/m.gguf"), "127.0.0.1", 4711);
        assert!(args.windows(2).any(|w| w == ["--host", "127.0.0.1"]));
        assert!(args.windows(2).any(|w| w == ["--port", "4711"]));
        assert!(!args.iter().any(|a| a.contains("--api-key")));
        assert!(args.windows(2).any(|w| w == ["-m", "/m.gguf"]));
        // Perf defaults (PR C) ride along even with the CPU floor.
        assert!(args.windows(2).any(|w| w[0] == "-ngl"));
        assert!(args.iter().any(|a| a == "--cache-prompt"));
    }

    #[test]
    fn llama_command_passes_api_key_only_in_child_environment() {
        let launch = LaunchArgs {
            backend: grokhub_core::local_profile::Backend::Cpu,
            ngl: 0,
            all_layers: 1,
            n_cpu_moe: None,
            ctx: 4096,
            kv: autoconfig::KvType::Q8,
            threads: 2,
            threads_batch: None,
            flash_attn: false,
            mlock: false,
            tensor_split: None,
            draft: None,
        };
        for args in [
            llama_args(Path::new("/m.gguf"), "127.0.0.1", 4711),
            llama_args_with_launch(Path::new("/m.gguf"), "127.0.0.1", 4711, &launch),
        ] {
            let cmd = llama_command(Path::new(llama_server_name()), &args, "fake-local-token");
            let args: Vec<_> = cmd.get_args().map(|a| a.to_str().unwrap()).collect();
            assert!(!args.iter().any(|a| a.contains("fake-local-token")));
            assert!(
                !args
                    .iter()
                    .any(|a| *a == "--api-key" || a.starts_with("--api-key=")),
                "{args:?}"
            );
            assert!(args.windows(2).any(|w| w == ["--host", "127.0.0.1"]));
            assert!(args.windows(2).any(|w| w == ["--port", "4711"]));
            let key = cmd
                .get_envs()
                .find(|(k, _)| *k == std::ffi::OsStr::new("LLAMA_API_KEY"))
                .and_then(|(_, v)| v);
            assert_eq!(key, Some(std::ffi::OsStr::new("fake-local-token")));
        }
    }

    #[test]
    fn llama_args_with_perf_include_gpu_defaults() {
        let perf = super::super::local_perf::defaults_for(&grokhub_core::local_setup::Hardware {
            gpu: "AMD Radeon RX 9070 XT".into(),
            vram_mb: 16_384,
            ram_mb: 32_768,
            free_disk_mb: 100_000,
        });
        let args = llama_args_with_perf(Path::new("/m.gguf"), "127.0.0.1", 9, &perf);
        assert!(args.windows(2).any(|w| w == ["-ngl", "99"]));
        assert!(args.windows(2).any(|w| w == ["-c", "16384"]));
        assert!(args.windows(2).any(|w| w == ["--flash-attn", "on"]));
    }

    #[test]
    fn spawn_rejects_wrong_binary_name_and_non_loopback() {
        let dir = std::env::temp_dir().join(format!("gh-sidecar-name-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let evil = dir.join("not-llama");
        std::fs::write(&evil, b"x").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut p = std::fs::metadata(&evil).unwrap().permissions();
            p.set_mode(0o755);
            std::fs::set_permissions(&evil, p).unwrap();
        }
        let err =
            spawn_llama(&evil, Path::new("/m.gguf"), "127.0.0.1", 9, "tok", None).unwrap_err();
        assert!(err.contains("must be named"), "{err}");
        let named = dir.join(llama_server_name());
        std::fs::write(&named, b"x").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut p = std::fs::metadata(&named).unwrap().permissions();
            p.set_mode(0o755);
            std::fs::set_permissions(&named, p).unwrap();
        }
        let err = spawn_llama(&named, Path::new("/m.gguf"), "0.0.0.0", 9, "tok", None).unwrap_err();
        assert!(err.contains("loopback"), "{err}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn free_port_is_on_loopback_only() {
        let port = free_loopback_port().expect("port");
        assert!(port > 0);
        assert!(is_loopback_host("127.0.0.1"));
        assert!(!is_loopback_host("0.0.0.0"));
        assert!(!is_loopback_host("192.168.1.1"));
    }

    #[test]
    fn fake_server_health_and_chat_contract() {
        let fake = FakeServer::start(None);
        assert!(health_ok(
            &fake.host,
            fake.port,
            &fake.token,
            Duration::from_secs(2)
        ));
        assert!(!health_ok(
            &fake.host,
            fake.port,
            "wrong",
            Duration::from_secs(1)
        ));
        let rt =
            super::super::local_http::HttpLocalRuntime::new(fake.base_url(), fake.token.clone());
        let v = rt
            .classify("hello", &["routine", "sensitive"])
            .expect("classify");
        assert_eq!(v["label"], "routine");
        assert_eq!(v["score"], 0.91);
        let s = rt.summarize("long text", 40).expect("summarize");
        assert_eq!(s["summary"], "fake summary");
        let mut deltas = String::new();
        let chat = rt
            .chat_stream(
                &[super::super::local::ChatMessage {
                    role: "user".into(),
                    content: "hi".into(),
                }],
                &mut |d| deltas.push_str(d),
            )
            .expect("chat");
        assert!(!chat.is_empty());
        assert!(!deltas.is_empty());
        assert!(fake.completion_hits() >= 3);
    }

    #[test]
    fn resolve_paths_needs_binary_and_gguf() {
        let dir = std::env::temp_dir().join(format!("gh-sidecar-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("bin")).unwrap();
        std::fs::create_dir_all(dir.join("models")).unwrap();
        assert!(resolve_paths(&dir).is_none());
        std::fs::write(dir.join("bin").join(llama_server_name()), b"x").unwrap();
        assert!(resolve_paths(&dir).is_none());
        std::fs::write(dir.join("models").join("a.gguf"), b"gguf").unwrap();
        let p = resolve_paths(&dir).expect("paths");
        assert!(p.binary.ends_with(llama_server_name()));
        assert!(p.model.ends_with("a.gguf"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_missing_engine_says_runtime_missing_instead_of_failing_blank() {
        let dir = std::env::temp_dir().join(format!("gh-sidecar-rt-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("models")).unwrap();
        std::fs::write(dir.join("models").join("a.gguf"), b"gguf").unwrap();
        // PATH comes back even when an assert fails, or later shell tests break.
        struct RestorePath(Option<std::ffi::OsString>);
        impl Drop for RestorePath {
            fn drop(&mut self) {
                match self.0.take() {
                    Some(v) => std::env::set_var("PATH", v),
                    None => std::env::remove_var("PATH"),
                }
            }
        }
        let path = RestorePath(std::env::var_os("PATH"));
        std::env::set_var("PATH", dir.join("empty"));
        // No sidecar or start error left over from another test.
        stop_global();
        assert_eq!(runtime(&dir), Runtime::Missing);
        let err = acquire(&dir).map(drop).unwrap_err();
        assert_eq!(
            err,
            "Runtime missing: llama-server isn't installed. Turning on Local AI downloads it."
        );
        // The health log names it once, however often it's asked.
        let _ = acquire(&dir).map(drop);
        let logged: Vec<String> = health::read(&dir).into_iter().map(|e| e.event).collect();
        assert_eq!(logged, vec!["missing_binary".to_string()]);
        std::fs::create_dir_all(dir.join("bin")).unwrap();
        std::fs::write(dir.join("bin").join(llama_server_name()), b"x").unwrap();
        assert_eq!(runtime(&dir), Runtime::Ready);
        assert_eq!(runtime_label(&Runtime::Ready), "Runtime ready");
        std::fs::remove_file(dir.join("models").join("a.gguf")).unwrap();
        if user_models_dir().is_none_or(|d| first_gguf(&d).is_none()) {
            assert_eq!(runtime(&dir), Runtime::NoModel);
        }
        drop(path);
        assert_eq!(
            runtime_label(&Runtime::Error("the local model ran out of memory".into())),
            "Error: the local model ran out of memory"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn resolve_paths_falls_back_to_path_after_config_bin() {
        let dir = std::env::temp_dir().join(format!("gh-sidecar-path-{}", std::process::id()));
        let path_dir = dir.join("pathbin");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("models")).unwrap();
        std::fs::create_dir_all(&path_dir).unwrap();
        std::fs::write(path_dir.join(llama_server_name()), b"x").unwrap();
        std::fs::write(dir.join("models").join("a.gguf"), b"gguf").unwrap();
        let prev = std::env::var_os("PATH");
        std::env::set_var("PATH", &path_dir);
        let p = resolve_paths(&dir).expect("paths");
        assert!(p.binary.ends_with(llama_server_name()));
        match prev {
            Some(v) => std::env::set_var("PATH", v),
            None => std::env::remove_var("PATH"),
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_binary_error_mentions_get_engine() {
        let paths = SidecarPaths {
            binary: PathBuf::from("/no/such/llama-server"),
            model: PathBuf::from("/m.gguf"),
        };
        let err = start(&paths).unwrap_err();
        assert!(
            err.contains("Get engine"),
            "missing binary should name Settings action: {err}"
        );
    }

    #[test]
    fn resolve_paths_prefers_model_download_installed_json() {
        let dir = std::env::temp_dir().join(format!("gh-sidecar-inst-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("bin")).unwrap();
        std::fs::create_dir_all(dir.join("models")).unwrap();
        std::fs::write(dir.join("bin").join(llama_server_name()), b"x").unwrap();
        let gguf = dir.join("models").join("picked.gguf");
        std::fs::write(&gguf, b"1234567890").unwrap(); // 10 bytes
        std::fs::write(
            dir.join("models").join("installed.json"),
            r#"{"id":"qwen","file":"picked.gguf","sha256":"abc","bytes":10}"#,
        )
        .unwrap();
        std::fs::write(dir.join("models").join("other.gguf"), b"other").unwrap();
        let p = resolve_paths(&dir).expect("paths");
        assert!(p.model.ends_with("picked.gguf"), "{}", p.model.display());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_settings_pick_wins_over_the_last_download() {
        let dir = std::env::temp_dir().join(format!("gh-sidecar-pick-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let models = dir.join("models");
        std::fs::create_dir_all(&models).unwrap();
        std::fs::write(models.join("last.gguf"), b"12345").unwrap();
        std::fs::write(
            models.join("installed.json"),
            r#"{"id":"x","file":"last.gguf","sha256":"abc","bytes":5}"#,
        )
        .unwrap();
        std::fs::write(dir.join("local_ai.json"), r#"{"mode":"manual"}"#).unwrap();
        let pick = |selected: &str| {
            set_pick(&dir, selected).unwrap();
            resolve_model_file(&dir)
        };
        let own = dir.join("mine.gguf");
        std::fs::write(&own, b"gguf").unwrap();
        assert_eq!(pick(&own.display().to_string()), Some(own.clone()));
        let m = grokhub_core::local_setup::local_models()
            .iter()
            .find(|m| !m.is_drafter)
            .unwrap();
        std::fs::write(models.join(m.file), b"gguf").unwrap();
        assert_eq!(pick(m.id), Some(models.join(m.file)));
        // A pick whose file is gone falls back to the last download.
        assert_eq!(
            pick(&dir.join("gone.gguf").display().to_string()),
            Some(models.join("last.gguf"))
        );
        let saved = std::fs::read_to_string(dir.join("local_ai.json")).unwrap();
        assert!(saved.contains(r#""mode": "manual""#), "{saved}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
