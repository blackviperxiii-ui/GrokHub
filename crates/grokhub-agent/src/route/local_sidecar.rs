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
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant};

use rand_token::random_token;

use super::local_autoconfig::{self as autoconfig, LaunchArgs, Retry};
use super::local_gov;
use super::local_health as health;
use super::local_swap::Plan;
use grokhub_core::local_setup::LocalModel;

/// The shortest wait for `/health` (a small model on a fast disk).
pub const HEALTH_TIMEOUT: Duration = Duration::from_secs(30);
/// The longest wait for `/health`, whatever the file size.
pub const HEALTH_TIMEOUT_MAX: Duration = Duration::from_secs(300);
/// Disk speed a slow drive still reads a model at, in MB per second.
const SLOW_DISK_MB_S: u64 = 200;

/// How long a start waits for `/health`: [`HEALTH_TIMEOUT`] plus the time a
/// slow disk takes to read `model_bytes`, at most [`HEALTH_TIMEOUT_MAX`].
pub fn health_timeout(model_bytes: u64) -> Duration {
    let read = model_bytes / (SLOW_DISK_MB_S * 1_024 * 1_024);
    (HEALTH_TIMEOUT + Duration::from_secs(read)).min(HEALTH_TIMEOUT_MAX)
}

/// How long a retired server may finish the requests it still runs after a
/// warm relaunch switched to the new one; past it they are cut.
pub const DRAIN_DEADLINE: Duration = Duration::from_secs(20);
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
    /// The one-shot defaults with every GPU left out (`launch` is `None` and
    /// the GPU backend crashed the first start).
    bare_cpu: bool,
    /// How the model's chat template thinks (its `reasoning_effort` levels
    /// and thinking switch), read from `/props` once the server is up;
    /// empty sends no effort.
    pub thinking: super::local_effort::Thinking,
    /// Why this isn't the Settings pick, when it isn't.
    stand_in: Option<super::local_swap::StandIn>,
    /// Requests on this server; a retired one stops once it reaches 0.
    in_flight: Arc<AtomicUsize>,
    /// See [`health_timeout`].
    health_timeout: Duration,
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
            .field("thinking", &self.thinking)
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

/// Unix seconds when the model was last needed as the fallback (offline, or no
/// cloud model answering). Idle time counts from this too, so it unloads only
/// [`super::local_perf::IDLE_UNLOAD_SECS`] after the cloud is back.
static LAST_NEEDED_SECS: AtomicU64 = AtomicU64::new(0);
/// A keep-warm load is running on its own thread.
static WARMING: AtomicBool = AtomicBool::new(false);
/// Set when Local AI is turned off while a request runs: the sidecar stops as
/// soon as the last request in flight finishes instead of after the idle timeout.
static STOP_PENDING: AtomicBool = AtomicBool::new(false);
/// Bumped by every stop (see [`retire`]). A keep-warm load started before a
/// stop sees the change under the slot lock and loads nothing.
static WARM_GEN: AtomicU64 = AtomicU64::new(0);

/// Held for the length of one request (see [`busy`]). Counts toward the
/// server it was taken on, so a warm relaunch retires the old server only
/// once its own requests are done.
pub struct Busy {
    side: Option<Arc<AtomicUsize>>,
    end: Option<Endpoint>,
}

impl Busy {
    /// The server this request was admitted to, when it was taken there.
    pub fn endpoint(&self) -> Option<&Endpoint> {
        self.end.as_ref()
    }
}

impl Drop for Busy {
    fn drop(&mut self) {
        if let Some(side) = &self.side {
            side.fetch_sub(1, Ordering::SeqCst);
        }
        LAST_DONE_SECS.store(unix_secs(), Ordering::SeqCst);
        if IN_FLIGHT.fetch_sub(1, Ordering::SeqCst) == 1 && STOP_PENDING.load(Ordering::SeqCst) {
            // Never block a request's teardown on the slot; if it's busy, the
            // next idle tick does the pending stop.
            if let Ok(mut slot) = global_slot().try_lock() {
                stop_pending_locked(&mut slot);
            }
        }
    }
}

/// Marks a request in flight until the guard drops, on the running server
/// when there is one.
pub fn busy() -> Busy {
    let slot = global_slot().lock().unwrap_or_else(|e| e.into_inner());
    match slot.as_ref() {
        Some(side) => side.busy(),
        None => {
            IN_FLIGHT.fetch_add(1, Ordering::SeqCst);
            Busy {
                side: None,
                end: None,
            }
        }
    }
}

pub fn in_flight() -> usize {
    IN_FLIGHT.load(Ordering::SeqCst)
}

impl Sidecar {
    /// Base URL for OpenAI-compatible calls (`http://127.0.0.1:<port>`).
    pub fn base_url(&self) -> String {
        format!("http://{}:{}", self.host, self.port)
    }

    /// A request on this server (see [`Busy`]).
    fn busy(&self) -> Busy {
        IN_FLIGHT.fetch_add(1, Ordering::SeqCst);
        self.in_flight.fetch_add(1, Ordering::SeqCst);
        Busy {
            side: Some(self.in_flight.clone()),
            end: Some(self.endpoint()),
        }
    }

    fn endpoint(&self) -> Endpoint {
        let slots = self.launch.as_ref().map(|l| &l.run.slots);
        Endpoint {
            base_url: self.base_url(),
            token: self.token.clone(),
            thinking: self.thinking.clone(),
            slots: slots.and_then(|s| s.parallel).unwrap_or(1).max(1),
            unified: slots.is_some_and(|s| s.kv_unified == Some(true)),
            in_flight: self.in_flight.clone(),
        }
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

    /// Block until healthy or its [`health_timeout`].
    pub fn wait_healthy(&self) -> Result<(), String> {
        let deadline = Instant::now() + self.health_timeout;
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

    /// This launch can put work on a GPU (the one-shot defaults offload
    /// every layer unless they were started bare on the CPU).
    pub fn on_gpu(&self) -> bool {
        match &self.launch {
            Some(l) => !l.cpu_only,
            None => !self.bare_cpu,
        }
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
        // The model's thinking comes back in `reasoning_content`, apart from
        // the answer, so it never reaches the chat as reply text.
        "--reasoning-format".into(),
        "deepseek".into(),
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

/// The flags one start passes: the planned launch, else the one-shot
/// defaults, with every GPU left out when `bare_cpu`.
fn spawn_args(
    model: &Path,
    host: &str,
    port: u16,
    launch: Option<&LaunchArgs>,
    bare_cpu: bool,
) -> Vec<String> {
    match launch {
        Some(l) => llama_args_with_launch(model, host, port, l),
        None => {
            let mut a = llama_args(model, host, port);
            if bare_cpu {
                a.extend(CPU_ONLY_ARGS.iter().map(|s| s.to_string()));
            }
            a
        }
    }
}

fn spawn_llama(
    binary: &Path,
    model: &Path,
    host: &str,
    port: u16,
    token: &str,
    launch: Option<&LaunchArgs>,
    bare_cpu: bool,
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
    let mut args = spawn_args(model, host, port, launch, bare_cpu);
    // A planned launch can save and restore its KV cache (see `local_kv`).
    if launch.is_some() {
        let options = super::local_flags::engine_options(binary);
        if let Some(dir) = super::local_kv::spawn_dir(&options) {
            args.extend([
                super::local_kv::FLAG.to_string(),
                dir.to_string_lossy().into_owned(),
            ]);
        }
    }
    // The load log names out-of-memory failures; without a file it is dropped.
    let log = std::fs::File::create(server_log_path(binary))
        .map(Stdio::from)
        .unwrap_or_else(|_| Stdio::null());
    let mut cmd = llama_command(binary, &args, token);
    cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(log);
    grokhub_core::proc_util::hide_windows_console(&mut cmd);
    let run = launch.map(|l| l.run.clone()).unwrap_or_default();
    cmd.envs(run.env());
    if run.low_priority {
        super::local_flags::below_normal(&mut cmd);
    }
    let child = cmd
        .spawn()
        .map_err(|e| format!("could not start local model server: {e}"))?;
    if run.low_priority {
        super::local_flags::lower_priority(child.id());
    }
    Ok(child)
}

/// Appended to the one-shot defaults to keep the engine off every GPU.
const CPU_ONLY_ARGS: [&str; 4] = ["-ngl", "0", "--device", "none"];

/// Whether a failed start should be tried again with every GPU left out: the
/// engine died while loading (a GPU driver or backend crash, not a slow load
/// or out-of-memory) and this launch could still reach a GPU.
pub fn retry_on_cpu(err: &str, log: &str, launch: Option<&LaunchArgs>, bare_cpu: bool) -> bool {
    let died = err.contains("stopped before it was ready");
    let on_gpu = match launch {
        Some(l) => !l.cpu_only,
        None => !bare_cpu,
    };
    died && on_gpu && !autoconfig::oom_in_log(log)
}

/// Start llama-server on a fresh loopback port.
pub fn start(paths: &SidecarPaths) -> Result<Sidecar, String> {
    let planned = autoconfig::launch_for(&paths.binary, &paths.model);
    start_planned(paths, planned)
}

/// [`start`] with the launch already planned (a warm relaunch plans it as a
/// fresh start would, while the old server still holds its memory).
fn start_planned(
    paths: &SidecarPaths,
    planned: Option<(LaunchArgs, String, PathBuf)>,
) -> Result<Sidecar, String> {
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
    autoconfig::clear_resident();
    let wait = health_timeout(std::fs::metadata(&paths.model).map_or(0, |m| m.len()));
    let mut launch = planned.as_ref().map(|(l, _, _)| l.clone());
    let idle = super::local_perf::current_defaults().idle_unload_secs;
    let mut attempt: u8 = 0;
    // A game holds the GPU: the one-shot defaults would offload every layer,
    // so they start on the CPU (a planned launch already did, see
    // `autoconfig::gaming_launch`).
    let mut bare_cpu = launch.is_none() && local_gov::gpu_contended();
    // The first start's error, kept when the CPU retry fails too.
    let mut gpu_err: Option<String> = None;
    // The drafter came off after it stopped a start (see below).
    let mut dropped_draft = false;
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
            bare_cpu,
        )?;
        let mut side = Sidecar {
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
            bare_cpu,
            thinking: Default::default(),
            stand_in: None,
            in_flight: Arc::new(AtomicUsize::new(0)),
            health_timeout: wait,
        };
        let err = match side.wait_healthy() {
            Ok(()) => {
                // Free VRAM with the model loaded: the governor's drop baseline.
                autoconfig::mark_resident();
                // The router sizes local turns by the context this server
                // really has, not a fixed default.
                if let Some(l) = launch.as_ref() {
                    super::local::set_ctx_cap(u64::from(l.ctx));
                }
                side.thinking = super::local_effort::fetch_thinking(
                    &side.host,
                    side.port,
                    &side.token,
                    super::local_effort::PROPS_TIMEOUT,
                );
                if let Some(why) = gpu_err.as_deref() {
                    health::log("gpu_fallback", Some(why));
                }
                if dropped_draft {
                    super::local_spec::draft_broke_start();
                }
                if let (Some(l), Some((_, key, dir))) = (launch.as_ref(), planned.as_ref()) {
                    if attempt > 0 || gpu_err.is_some() {
                        let tuned = autoconfig::Tuned {
                            ngl: l.ngl,
                            n_cpu_moe: l.n_cpu_moe,
                            ctx: l.ctx,
                            kv: l.kv,
                            cpu_only: l.cpu_only,
                            clean: 0,
                        };
                        let _ = autoconfig::save_tuned(dir, key, &tuned);
                    } else {
                        let _ = autoconfig::clean_start(dir, key, l.all_layers);
                    }
                    if let Some(tag) = &l.run.tune {
                        let on_gpu = !l.cpu_only && l.ngl > 0;
                        let headroom = on_gpu
                            .then(|| autoconfig::free_now().vram_free_mb)
                            .flatten();
                        super::local_tune::started(dir, tag, headroom);
                    }
                }
                return Ok(side);
            }
            Err(e) => e,
        };
        side.stop();
        let log = std::fs::read_to_string(server_log_path(&paths.binary)).unwrap_or_default();
        // A drafter that won't load beside this model (another tokenizer, a
        // broken or half-written file) stops the whole start, and the model
        // and drafter on disk read as not ready. The model starts alone first,
        // before a CPU retry; once that works the pair stays off until a retest.
        if !autoconfig::oom_in_log(&log)
            && let Some(l) = launch.as_mut().filter(|l| l.draft.is_some())
        {
            health::log("draft_off", Some(&err));
            l.draft = None;
            dropped_draft = true;
            continue;
        }
        if gpu_err.is_none() && retry_on_cpu(&err, &log, launch.as_ref(), bare_cpu) {
            gpu_err = Some(err);
            match launch.as_ref() {
                Some(l) => launch = Some(autoconfig::on_cpu(l)),
                None => bare_cpu = true,
            }
            continue;
        }
        // The CPU died the same way, so the GPU was not why: report the first start.
        if let Some(first) = gpu_err.take() {
            return Err(first);
        }
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
                    "{OUT_OF_MEMORY} even with fewer GPU layers.{smaller}"
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
    let agent = grokhub_core::http::agent(grokhub_core::http::Opts {
        connect: timeout,
        read: Some(timeout),
        ..Default::default()
    });
    match agent
        .get(&url)
        .header("Authorization", &format!("Bearer {token}"))
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
    let stamp = paths_stamp(config_dir);
    let mut cache = PATHS_CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((dir, at, when, paths)) = cache.as_ref() {
        // Nothing found looks again now and then: an engine put on `PATH`
        // changes no time stamped here.
        let still = match paths {
            Some(p) => p.binary.is_file() && p.model.is_file(),
            None => when.elapsed() < MISSING_RECHECK,
        };
        if dir == config_dir && *at == stamp && still {
            return paths.clone();
        }
    }
    let paths = find_binary(config_dir)
        .zip(find_model(config_dir))
        .map(|(binary, model)| SidecarPaths { binary, model });
    *cache = Some((
        config_dir.to_path_buf(),
        stamp,
        Instant::now(),
        paths.clone(),
    ));
    paths
}

/// How long "no engine or no model" stands before looking again.
const MISSING_RECHECK: Duration = Duration::from_secs(5);

/// What [`resolve_paths`] reads: the override variables, `PATH`, and the
/// modification times of the folders and files it looks in. The routing
/// hot path asks many times a second; while none of these changed, the
/// answer it found last stands.
#[derive(Debug, Clone, PartialEq, Eq)]
struct PathsStamp {
    env: [Option<std::ffi::OsString>; 3],
    mtimes: Vec<Option<std::time::SystemTime>>,
}

fn paths_stamp(config_dir: &Path) -> PathsStamp {
    let mtime = |p: PathBuf| std::fs::metadata(p).and_then(|m| m.modified()).ok();
    let mut looked = vec![
        config_dir.join("bin"),
        config_dir.join("local_ai.json"),
        config_dir.join("models"),
        config_dir
            .join("models")
            .join(grokhub_core::model_download::INSTALLED_FILE),
    ];
    if let Some(user) = user_models_dir() {
        looked.push(user.join(grokhub_core::model_download::INSTALLED_FILE));
        looked.push(user);
    }
    PathsStamp {
        env: [
            std::env::var_os("GROKHUB_LLAMA_SERVER"),
            std::env::var_os("GROKHUB_LOCAL_MODEL"),
            std::env::var_os("PATH"),
        ],
        mtimes: looked.into_iter().map(mtime).collect(),
    }
}

type PathsCache = Mutex<Option<(PathBuf, PathsStamp, Instant, Option<SidecarPaths>)>>;
static PATHS_CACHE: PathsCache = Mutex::new(None);

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

/// A GGUF that can't be the chat model: a catalog drafter or vision
/// projector, or any `mmproj` file. Loading one as the model fails the start.
pub fn helper_gguf(path: &Path) -> bool {
    use grokhub_core::local_setup::{local_models, ModelKind};
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    name.to_ascii_lowercase().contains("mmproj")
        || local_models().iter().any(|m| {
            matches!(m.kind, ModelKind::Drafter | ModelKind::VisionProjector)
                && m.file.eq_ignore_ascii_case(name)
        })
}

fn first_gguf(dir: &Path) -> Option<PathBuf> {
    let mut entries: Vec<_> = std::fs::read_dir(dir)
        .ok()?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("gguf"))
        .filter(|p| p.is_file() && !helper_gguf(p))
        .collect();
    entries.sort();
    entries.into_iter().next()
}

/// Process-wide sidecar. Started lazily when paths resolve.
static GLOBAL: OnceLock<Mutex<Option<Sidecar>>> = OnceLock::new();

/// Held while the draft speed test runs servers of its own, so a request
/// waits for it rather than loading a second copy of the model beside it.
static SPEED_TEST: Mutex<()> = Mutex::new(());
/// Requests waiting on [`SPEED_TEST`]; the test stops early for them.
static WAITING: AtomicUsize = AtomicUsize::new(0);

/// Hold while timing the model with servers of your own.
pub fn speed_test_gate() -> std::sync::MutexGuard<'static, ()> {
    SPEED_TEST.lock().unwrap_or_else(|e| e.into_inner())
}

/// Someone is waiting for the speed test to finish.
pub fn speed_test_wanted_back() -> bool {
    WAITING.load(Ordering::SeqCst) > 0
}

fn wait_for_speed_test(want: Want) {
    if SPEED_TEST.try_lock().is_ok() {
        return;
    }
    // A keep-warm load waits too, but doesn't hurry the test along.
    let counted = !matches!(want, Want::Warm(_));
    if counted {
        WAITING.fetch_add(1, Ordering::SeqCst);
    }
    drop(speed_test_gate());
    if counted {
        WAITING.fetch_sub(1, Ordering::SeqCst);
    }
}

/// Keep the model loaded through idle time ("Keep loaded" in Settings),
/// until the app closes. The governor still unloads it for a game or when
/// memory runs short.
static KEEP_LOADED: AtomicBool = AtomicBool::new(false);

pub fn set_keep_loaded(on: bool) {
    KEEP_LOADED.store(on, Ordering::SeqCst);
}

pub fn keep_loaded() -> bool {
    KEEP_LOADED.load(Ordering::SeqCst)
}

/// What the loaded model runs with, for the running-model rows.
#[derive(Debug, Clone)]
pub struct Loaded {
    pub model: PathBuf,
    pub launch: Option<LaunchArgs>,
    /// The one-shot defaults with every GPU left out.
    pub bare_cpu: bool,
    pub idle_unload_secs: u64,
    /// Why this isn't the Settings pick, when it isn't.
    pub stand_in: Option<super::local_swap::StandIn>,
}

/// The loaded model, if one is.
pub fn loaded() -> Option<Loaded> {
    let slot = global_slot().lock().ok()?;
    let side = slot.as_ref()?;
    Some(Loaded {
        model: side.model.clone(),
        launch: side.launch.clone(),
        bare_cpu: side.bare_cpu,
        idle_unload_secs: side.idle_unload_secs,
        stand_in: side.stand_in,
    })
}

/// Seconds since the model was last asked, answered or needed as the
/// fallback.
pub fn secs_since_use() -> u64 {
    let side = global_slot()
        .lock()
        .ok()
        .and_then(|s| s.as_ref().map(|s| s.last_used_secs.load(Ordering::SeqCst)))
        .unwrap_or(0);
    let last = side
        .max(LAST_DONE_SECS.load(Ordering::SeqCst))
        .max(LAST_NEEDED_SECS.load(Ordering::SeqCst));
    unix_secs().saturating_sub(last)
}

fn global_slot() -> &'static Mutex<Option<Sidecar>> {
    GLOBAL.get_or_init(|| Mutex::new(None))
}

/// Ensure a healthy global sidecar when assets exist under `config_dir`.
pub fn ensure_global(config_dir: &Path) -> Result<(), String> {
    acquire(config_dir).map(drop)
}

/// Who is asking [`load`] for the sidecar.
#[derive(Clone, Copy)]
enum Want {
    /// Tests and [`ensure_global`]: start it whatever the toggle says.
    Any,
    /// A routed request: refused once Local AI is off or a stop is pending, so
    /// a request that checked the toggle before it went off starts nothing.
    Request,
    /// A keep-warm load started under this [`WARM_GEN`].
    Warm(u64),
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
    load(config_dir, Want::Any)
}

/// [`acquire`] for a routed request: refused when Local AI was turned off
/// since the request checked the toggle (a request already holding its
/// [`Busy`] still finishes).
pub fn acquire_for_request(config_dir: &Path) -> Result<Busy, String> {
    load(config_dir, Want::Request)
}

/// [`acquire`]; a keep-warm load passes the [`WARM_GEN`] it started under and
/// gives up when Local AI was turned off or the sidecar stopped since.
fn load(config_dir: &Path, want: Want) -> Result<Busy, String> {
    wait_for_speed_test(want);
    tick_idle_unload();
    *LAST_DIR.lock().unwrap_or_else(|e| e.into_inner()) = Some(config_dir.to_path_buf());
    loop {
        if local_gov::paused() {
            tick_governor();
            return Err(format!(
                "the on-device model is paused because {}",
                local_gov::reason()
            ));
        }
        let paths = match runtime_paths(config_dir) {
            Ok(p) => p,
            Err(why) => {
                health::log_to(config_dir, health::start_failure(&why), Some(&why), None);
                return Err(why);
            }
        };
        let mut slot = global_slot().lock().expect("sidecar global");
        // Another load is starting a server: wait for it rather than start a
        // second copy, without holding the slot through its cold start.
        while STARTING.load(Ordering::SeqCst) {
            slot = LOAD_DONE.wait(slot).unwrap_or_else(|e| e.into_inner());
        }
        if let Some(why) = refused(want) {
            return Err(why.into());
        }
        // A game wants the GPU: no new work goes to a server on it. It stops
        // and this request starts one on the CPU.
        if local_gov::gpu_contended() && slot.as_ref().is_some_and(Sidecar::on_gpu) {
            cut_for_game(&mut slot);
        }
        if local_gov::restart_due() && in_flight() == 0 {
            restart_locked(&mut slot, false);
        }
        // A dead sidecar starts again as it was (the crash breaker counts it).
        let swap = match slot.as_ref() {
            Some(side) if !side.alive() => {
                if let Err(e) = local_gov::may_start(unix_ms()) {
                    if let Some(dead) = slot.take() {
                        retire(dead);
                    }
                    return Err(e);
                }
                local_gov::start_failed(unix_ms());
                let dead = slot.take().expect("dead sidecar");
                let again = Plan::Load {
                    model: dead.model.clone(),
                    stand_in: dead.stand_in,
                };
                retire(dead);
                again
            }
            running => swap_plan(config_dir, &paths.model, running, want),
        };
        // A step up waits for a quiet chat, so a reply never sits through a
        // reload; a request after a quiet spell is a new task and takes it now.
        let swap = match swap {
            Plan::Later { model, stand_in } if secs_since_use() < QUIET_SECS => {
                *PENDING_STEP_UP.lock().expect("pending step up") =
                    Some((model, stand_in, want_need(want)));
                Plan::Keep
            }
            Plan::Later { model, stand_in } => Plan::Load { model, stand_in },
            other => other,
        };
        let (model, stand_in) = match swap {
            Plan::Keep | Plan::Later { .. } => (paths.model.clone(), None),
            Plan::Load { model, stand_in } => {
                PENDING_STEP_UP.lock().expect("pending step up").take();
                if let Some(side) = slot.take() {
                    health::log_to(config_dir, "swap", Some(&swap_note(&model, stand_in)), None);
                    LAST_SWAP_SECS.store(unix_secs(), Ordering::SeqCst);
                    retire(side);
                }
                (model, stand_in)
            }
        };
        if let Some(side) = slot.as_ref() {
            side.last_used_secs.store(unix_secs(), Ordering::SeqCst);
            return Ok(side.busy());
        }
        // Background work never wakes the model: with none loaded it gets
        // no server, and the caller answers without the device.
        if matches!(want, Want::Request) && want_need(want) == super::local_scale::Need::Background
        {
            drop(slot);
            return Ok(busy());
        }
        local_gov::may_start(unix_ms())?;
        STARTING.store(true, Ordering::SeqCst);
        let generation = WARM_GEN.load(Ordering::SeqCst);
        // The slot is free while the server loads: Settings, the governor
        // and a stop never wait for a cold start.
        drop(slot);
        let began = Instant::now();
        let paths = SidecarPaths {
            binary: paths.binary,
            model,
        };
        let measured = paths.model.clone();
        let cached = super::local_warm::resident_pct(&measured);
        let (started, model, stand_in) = start_or_stand_in(config_dir, paths, stand_in);
        let mut slot = global_slot().lock().expect("sidecar global");
        STARTING.store(false, Ordering::SeqCst);
        LOAD_DONE.notify_all();
        match started {
            // A stop or a switch came while it loaded: this server is out of
            // date. Drop it and plan again (a request for a switched model
            // loads the new one; a cancelled warm-up or Local AI off refuses).
            Ok(side) if WARM_GEN.load(Ordering::SeqCst) != generation => drop(side),
            Ok(mut side) => {
                side.stand_in = stand_in;
                side.idle_unload_secs =
                    learned_idle(config_dir, &side.model, side.idle_unload_secs);
                let name = model.file_name().map(|n| n.to_string_lossy());
                let ms = u64::try_from(began.elapsed().as_millis()).unwrap_or(u64::MAX);
                // A stand-in loaded another file than the one measured.
                let cached = cached.filter(|_| model == measured);
                let detail = super::local_warm::start_detail(name.as_deref(), cached);
                health::log_to(config_dir, "start", detail.as_deref(), Some(ms));
                super::local_warm::note_loaded();
                if let Some(a) = side.launch.clone() {
                    super::local_kv::after_load(
                        side.base_url(),
                        side.token.clone(),
                        side.binary.clone(),
                        side.model.clone(),
                        a,
                    );
                }
                set_last_error(None);
                let busy = side.busy();
                *slot = Some(side);
                return Ok(busy);
            }
            Err(e) => {
                local_gov::start_failed(unix_ms());
                health::log_to(config_dir, health::start_failure(&e), Some(&e), None);
                set_last_error(Some(e.clone()));
                return Err(e);
            }
        }
    }
}

/// Binary and model for a start, or why there are none.
fn runtime_paths(config_dir: &Path) -> Result<SidecarPaths, String> {
    match runtime(config_dir) {
        r @ (Runtime::Missing | Runtime::NoModel) => Err(runtime_label(&r)),
        _ => resolve_paths(config_dir).ok_or_else(|| runtime_label(&Runtime::Missing)),
    }
}

/// [`start`], and on out-of-memory a smaller model on disk in its place.
/// Returns what started, the model it loaded and why it stands in.
fn start_or_stand_in(
    config_dir: &Path,
    mut paths: SidecarPaths,
    mut stand_in: Option<super::local_swap::StandIn>,
) -> (
    Result<Sidecar, String>,
    PathBuf,
    Option<super::local_swap::StandIn>,
) {
    let mut started = start(&paths);
    if let Err(e) = &started {
        let stand = e
            .starts_with(OUT_OF_MEMORY)
            .then(|| super::local_swap::out_of_memory_stand_in(&paths.model, &on_disk(config_dir)))
            .flatten();
        if let Some(model) = stand {
            health::log_to(config_dir, health::start_failure(e), Some(e), None);
            stand_in = Some(super::local_swap::StandIn::OutOfMemory);
            health::log_to(config_dir, "swap", Some(&swap_note(&model, stand_in)), None);
            paths.model = model;
            started = start(&paths);
        }
    }
    (started, paths.model, stand_in)
}

/// Wakes loads waiting for another load to finish (see [`load`]).
static LOAD_DONE: Condvar = Condvar::new();
/// A warm relaunch is starting its new server on its own thread.
static RELAUNCHING: AtomicBool = AtomicBool::new(false);
/// The config folder the last load used, for a reload after a governor restart.
static LAST_DIR: Mutex<Option<PathBuf>> = Mutex::new(None);

/// How a governor restart reaches the new launch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Relaunch {
    /// The new plan matches the running one: nothing restarts.
    Skip,
    /// Start the new server beside the running one, switch, then retire the old.
    Warm,
    /// Not enough memory for both: stop, then start.
    Cold,
}

/// How to relaunch from `running` to `next` (`None` is the one-shot
/// defaults, which a level change never alters); `fits_beside` is only
/// asked when the launches differ.
pub fn relaunch_kind(
    running: Option<&LaunchArgs>,
    next: Option<&LaunchArgs>,
    fits_beside: impl FnOnce(&LaunchArgs) -> bool,
) -> Relaunch {
    match (running, next) {
        (None, None) => Relaunch::Skip,
        (Some(r), Some(n)) if autoconfig::same_launch(r, n) => Relaunch::Skip,
        (_, Some(n)) if fits_beside(n) => Relaunch::Warm,
        _ => Relaunch::Cold,
    }
}

/// A restart is due and the slot is locked: skip it when the plan is the
/// same, start a warm relaunch when both servers fit, else stop the running
/// one (only with nothing in flight). `reload` starts the stopped model
/// again in the background, so the next request finds it warm.
fn restart_locked(slot: &mut Option<Sidecar>, reload: bool) {
    let Some(side) = slot.as_ref().filter(|s| s.alive()) else {
        if in_flight() == 0 {
            local_gov::clear_restart();
        }
        return;
    };
    if RELAUNCHING.load(Ordering::SeqCst) {
        return;
    }
    let next = autoconfig::replan_for(&side.binary, &side.model);
    let kind = relaunch_kind(
        side.launch.as_ref(),
        next.as_ref().map(|(l, _, _)| l),
        |n| autoconfig::warm_fits(&side.model, n),
    );
    match kind {
        Relaunch::Skip => {
            local_gov::clear_restart();
            health::log("relaunch", Some("skipped: same launch"));
        }
        Relaunch::Warm => {
            local_gov::clear_restart();
            let paths = SidecarPaths {
                binary: side.binary.clone(),
                model: side.model.clone(),
            };
            relaunch_warm(paths, side.port, side.stand_in, side.idle_unload_secs);
        }
        Relaunch::Cold if in_flight() == 0 => {
            local_gov::clear_restart();
            if let Some(side) = slot.take() {
                retire(side);
                health::log("stop", Some("the governor restarted it"));
            }
            if reload {
                reload_in_background();
            }
        }
        Relaunch::Cold => {}
    }
}

/// Start the relaunched server beside the running one on its own thread,
/// then switch requests to it and drain the old one.
fn relaunch_warm(
    paths: SidecarPaths,
    old_port: u16,
    stand_in: Option<super::local_swap::StandIn>,
    idle: u64,
) {
    if RELAUNCHING.swap(true, Ordering::SeqCst) {
        return;
    }
    let generation = WARM_GEN.load(Ordering::SeqCst);
    std::thread::spawn(move || {
        let planned =
            autoconfig::launch_with(&paths.binary, &paths.model, Some(autoconfig::replan_free()));
        let started = start_planned(&paths, planned);
        let mut slot = global_slot().lock().expect("sidecar global");
        match started {
            Ok(mut new) => {
                let current = slot.as_ref().is_some_and(|s| s.port == old_port)
                    && WARM_GEN.load(Ordering::SeqCst) == generation;
                if current {
                    new.stand_in = stand_in;
                    new.idle_unload_secs = idle;
                    let old = slot.replace(new);
                    drop(slot);
                    health::log("relaunch", Some("warm"));
                    if let Some(old) = old {
                        drain(old, DRAIN_DEADLINE);
                    }
                } else {
                    drop(slot);
                    drop(new);
                }
            }
            // The old server keeps answering; the next level change tries again.
            Err(e) => {
                drop(slot);
                health::log("relaunch", Some(&e));
            }
        }
        RELAUNCHING.store(false, Ordering::SeqCst);
    });
}

/// Stop `old` once its own requests are done, or at `deadline` (the
/// requests still running on it are cut), then take the free-VRAM baseline
/// again now its memory is back.
fn drain(old: Sidecar, deadline: Duration) {
    std::thread::spawn(move || {
        let until = Instant::now() + deadline;
        while old.in_flight.load(Ordering::SeqCst) > 0 && Instant::now() < until {
            std::thread::sleep(HEALTH_POLL);
        }
        old.stop();
        drop(old);
        autoconfig::refresh_resident();
    });
}

/// Load the model again on its own thread after a governor stop.
fn reload_in_background() {
    let Some(dir) = LAST_DIR.lock().unwrap_or_else(|e| e.into_inner()).clone() else {
        return;
    };
    if WARMING.swap(true, Ordering::SeqCst) {
        return;
    }
    let generation = WARM_GEN.load(Ordering::SeqCst);
    std::thread::spawn(move || {
        if load(&dir, Want::Warm(generation)).is_ok() {
            health::log_to(
                &dir,
                "warm",
                Some("reloaded after a governor restart"),
                None,
            );
        }
        WARMING.store(false, Ordering::SeqCst);
    });
}

/// How [`start`]'s out-of-memory error begins.
const OUT_OF_MEMORY: &str = "the local model ran out of memory";

/// A chat quiet this long has finished its task: a step up may load.
pub const QUIET_SECS: u64 = 20;

/// A bigger model [`super::local_swap::plan`] wants once the chat is quiet,
/// and the need it was for.
static PENDING_STEP_UP: Mutex<
    Option<(
        PathBuf,
        Option<super::local_swap::StandIn>,
        super::local_scale::Need,
    )>,
> = Mutex::new(None);

/// A keep-warm load plans for the need it warms for; a request for its own.
fn want_need(want: Want) -> super::local_scale::Need {
    match want {
        Want::Warm(_) => *WARM_NEED.lock().expect("warm need"),
        Want::Any | Want::Request => super::local_scale::current_need(),
    }
}

/// The need a keep-warm load plans for: a step up's, else Normal.
static WARM_NEED: Mutex<super::local_scale::Need> = Mutex::new(super::local_scale::Need::Normal);

/// When the running model was last swapped for another.
static LAST_SWAP_SECS: AtomicU64 = AtomicU64::new(0);

/// The model the idle timer last unloaded, and when.
static LAST_IDLE_UNLOAD: Mutex<Option<(PathBuf, u64)>> = Mutex::new(None);

/// The chat models downloaded where [`picked_model`] looks.
fn on_disk(config_dir: &Path) -> Vec<(&'static LocalModel, PathBuf)> {
    let dirs: Vec<PathBuf> = [user_models_dir(), Some(config_dir.join("models"))]
        .into_iter()
        .flatten()
        .collect();
    super::local_swap::on_disk(&dirs)
}

/// What the next load runs: the pick, or another model on disk for a reason
/// [`super::local_swap::plan`] gives. The caller holds the slot.
fn swap_plan(config_dir: &Path, home: &Path, running: Option<&Sidecar>, want: Want) -> Plan {
    let loaded = running.map(|s| (s.model.as_path(), s.stand_in));
    let on_disk = on_disk(config_dir);
    super::local_swap::plan(super::local_swap::Now {
        home,
        loaded,
        on_disk: &on_disk,
        low_battery: super::local::low_battery(),
        kept_loaded: keep_loaded(),
        forced: std::env::var_os("GROKHUB_LOCAL_MODEL").is_some(),
        in_flight: in_flight(),
        secs_since_swap: unix_secs().saturating_sub(LAST_SWAP_SECS.load(Ordering::SeqCst)),
        need: want_need(want),
        scale: super::local_scale::scale_now(unix_ms()),
    })
}

/// The model a game would swap `home` for, among the models on disk.
pub fn gaming_model_for(config_dir: &Path, home: &Path) -> Option<PathBuf> {
    super::local_swap::gaming_model(home, &on_disk(config_dir))
}

/// When [`tick_scale`] last saw a game running (unix ms).
static LAST_GAME_MS: AtomicU64 = AtomicU64::new(0);
/// How long after a game the pick waits to come back: post-match screens
/// and launchers finish, and a quick alt-tab back into the game swaps nothing.
pub const GAME_OVER_SECS: u64 = 45;

/// A game was last seen at `last_game_ms` and has been gone [`GAME_OVER_SECS`].
pub fn game_settled(now_ms: u64, last_game_ms: u64) -> bool {
    last_game_ms > 0 && now_ms.saturating_sub(last_game_ms) >= GAME_OVER_SECS * 1_000
}

/// Unix seconds of the last [`tick_prewarm`] look.
static LAST_PREWARM_LOOK: AtomicU64 = AtomicU64::new(0);
const PREWARM_LOOK_SECS: u64 = 60;

/// The cabin sampler's tick for RAM-warming: once a minute, read the files
/// the next load will need into the OS cache (see `local_prewarm`). That is
/// the pick and its drafter when they aren't running (after a game, an idle
/// unload or at app start), and the model a game swaps to.
pub fn tick_prewarm(config_dir: &Path) {
    let now = unix_secs();
    if !super::local::switched_on()
        || super::local_prewarm::busy()
        || local_gov::paused()
        || STARTING.load(Ordering::SeqCst)
        || now.saturating_sub(LAST_PREWARM_LOOK.load(Ordering::SeqCst)) < PREWARM_LOOK_SECS
    {
        return;
    }
    LAST_PREWARM_LOOK.store(now, Ordering::SeqCst);
    let (Some(binary), Some(home)) = (find_binary(config_dir), find_model(config_dir)) else {
        return;
    };
    let Ok(slot) = global_slot().try_lock() else {
        return;
    };
    let loaded = slot.as_ref().filter(|s| s.alive()).map(|s| s.model.clone());
    drop(slot);
    let gaming =
        local_gov::gpu_contended() || super::local_scale::gaming_reason(local_gov::reason());
    let gaming_pick = gaming_model_for(config_dir, &home);
    let mut files = Vec::new();
    for model in [Some(home), gaming_pick].into_iter().flatten() {
        if loaded.as_ref() == Some(&model) {
            continue;
        }
        let drafter = autoconfig::draft_plan(&binary, &model)
            .and_then(|p| p.draft)
            .and_then(|d| d.model);
        files.push(model);
        files.extend(drafter);
    }
    super::local_prewarm::start(files, gaming);
}

/// The cabin sampler's tick for scaling, between requests: a step up the
/// last task wanted loads once the chat is quiet, and a bigger model that no
/// longer fits (the computer got busy) or a model that isn't the lightest
/// while a game runs stops, so the next request starts the right one.
pub fn tick_scale(config_dir: &Path) {
    let scale = super::local_scale::scale_now(unix_ms());
    if scale.gaming {
        LAST_GAME_MS.store(unix_ms(), Ordering::SeqCst);
    }
    if in_flight() > 0 || local_gov::paused() || STARTING.load(Ordering::SeqCst) {
        return;
    }
    let quiet = secs_since_use() >= QUIET_SECS;
    let pending = PENDING_STEP_UP.lock().expect("pending step up").clone();
    let mut slot = global_slot().lock().expect("sidecar global");
    if in_flight() > 0 {
        return;
    }
    let Some(side) = slot.as_ref().filter(|s| s.alive()) else {
        return;
    };
    let step_down = side.stand_in == Some(super::local_swap::StandIn::Performance)
        && super::local_swap::catalog_for(&side.model)
            .is_some_and(|m| super::local_scale::step_down_due(m, &scale));
    let gaming = scale.gaming && side.stand_in != Some(super::local_swap::StandIn::Gaming);
    if step_down || gaming {
        if let Some(side) = slot.take() {
            let why = if gaming {
                "a game started"
            } else {
                "the computer got busy"
            };
            health::log_to(
                config_dir,
                "swap",
                Some(&format!("stopped the bigger model: {why}")),
                None,
            );
            LAST_SWAP_SECS.store(unix_secs(), Ordering::SeqCst);
            retire(side);
        }
        PENDING_STEP_UP.lock().expect("pending step up").take();
        return;
    }
    // The game ended and the machine settled: the pick comes back now, in
    // the background, so the next chat doesn't run on the gaming model.
    let after_game = side.stand_in == Some(super::local_swap::StandIn::Gaming)
        && !scale.gaming
        && local_gov::level() == grokhub_core::local_governor::Level::Normal
        && game_settled(unix_ms(), LAST_GAME_MS.load(Ordering::SeqCst));
    let pending = if after_game {
        find_model(config_dir).map(|home| (home, None, super::local_scale::Need::Normal))
    } else {
        pending
    };
    let Some((model, stand_in, need)) = pending.filter(|_| quiet) else {
        return;
    };
    if side.model == model || keep_loaded() || WARMING.swap(true, Ordering::SeqCst) {
        PENDING_STEP_UP.lock().expect("pending step up").take();
        return;
    }
    PENDING_STEP_UP.lock().expect("pending step up").take();
    *WARM_NEED.lock().expect("warm need") = need;
    if let Some(side) = slot.take() {
        health::log_to(config_dir, "swap", Some(&swap_note(&model, stand_in)), None);
        LAST_SWAP_SECS.store(unix_secs(), Ordering::SeqCst);
        save_kv(&side);
        retire(side);
    }
    drop(slot);
    let dir = config_dir.to_path_buf();
    let generation = WARM_GEN.load(Ordering::SeqCst);
    let why = if after_game {
        "back after the game"
    } else {
        "stepped up for a hard task"
    };
    std::thread::spawn(move || {
        if load(&dir, Want::Warm(generation)).is_ok() {
            health::log_to(&dir, "warm", Some(why), None);
        }
        *WARM_NEED.lock().expect("warm need") = super::local_scale::Need::Normal;
        WARMING.store(false, Ordering::SeqCst);
    });
}

/// The health log's note for a swap to `model`.
fn swap_note(model: &Path, why: Option<super::local_swap::StandIn>) -> String {
    let name = model
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    match why {
        Some(super::local_swap::StandIn::LowBattery) => format!("{name} while the battery is low"),
        Some(super::local_swap::StandIn::OutOfMemory) => {
            format!("{name} because the pick ran out of memory")
        }
        Some(super::local_swap::StandIn::Gaming) => format!("{name} while a game runs"),
        Some(super::local_swap::StandIn::Lighter) => format!("{name} for a light task"),
        Some(super::local_swap::StandIn::Tight) => format!("{name} because less memory is free"),
        Some(super::local_swap::StandIn::Performance) => format!("{name} for a hard task"),
        None => format!("{name}, the Settings pick"),
    }
}

/// The idle timer for `model` just started: learned from how soon it came
/// back after the last idle unload, unless Settings set one.
fn learned_idle(config_dir: &Path, model: &Path, base: u64) -> u64 {
    if !super::local_perf::idle_is_default() {
        return base;
    }
    let last = LAST_IDLE_UNLOAD.lock().expect("last idle unload").take();
    let current = super::local_swap::learned_secs(config_dir, model, base);
    let Some((unloaded, at)) = last.filter(|(m, _)| m == model) else {
        return current;
    };
    let next = super::local_swap::learn(current, base, unix_secs().saturating_sub(at));
    if next != current {
        let _ = super::local_swap::save_learned(config_dir, model, next);
        health::log_to(
            config_dir,
            "idle_timer",
            unloaded.file_stem().and_then(|s| s.to_str()),
            Some(next.saturating_mul(1_000)),
        );
    }
    next
}

/// The governor's sample tick. A game wanting the GPU cuts a server running
/// on it at once, requests in flight included, and (unless paused) loads it
/// again on the CPU in the background. Otherwise, between requests, stop the
/// sidecar when the level is Unload; when a restart is due, skip it for the
/// same plan, relaunch warm when both servers fit, else stop it and load it
/// again in the background.
pub fn tick_governor() {
    let paused = local_gov::paused();
    let contended = local_gov::gpu_contended();
    if !(paused || contended || local_gov::restart_due()) {
        return;
    }
    let mut slot = global_slot().lock().expect("sidecar global");
    if contended && slot.as_ref().is_some_and(Sidecar::on_gpu) {
        cut_for_game(&mut slot);
        local_gov::clear_restart();
        if !paused {
            reload_in_background();
        }
        return;
    }
    if paused {
        // A request may have taken its guard while this waited for the lock.
        if in_flight() > 0 {
            return;
        }
        if let Some(side) = slot.take() {
            retire(side);
            health::log("stop", Some("the governor paused it"));
        }
        local_gov::clear_restart();
        return;
    }
    restart_locked(&mut slot, true);
}

/// Unix ms when [`cut_for_game`] last cut requests in flight.
static CUT_AT_MS: AtomicU64 = AtomicU64::new(0);
/// A request that failed within this long of a cut failed because of it.
const CUT_NOTE_MS: u64 = 30_000;

/// Stop the server in `slot` now, requests in flight included: killing the
/// child closes their sockets and frees the GPU at once.
fn cut_for_game(slot: &mut Option<Sidecar>) {
    let Some(side) = slot.take() else {
        return;
    };
    let cut = side.in_flight.load(Ordering::SeqCst);
    if cut > 0 {
        CUT_AT_MS.store(unix_ms(), Ordering::SeqCst);
    }
    retire(side);
    health::log(
        "stop",
        Some(&format!("a game wanted the GPU; cut {cut} running")),
    );
}

/// Why a local request just failed, when a game cut it.
pub fn cut_reason() -> Option<&'static str> {
    let at = CUT_AT_MS.load(Ordering::SeqCst);
    (at > 0 && unix_ms().saturating_sub(at) < CUT_NOTE_MS)
        .then_some("the on-device model stopped because a game started")
}

/// Process id of the running sidecar.
pub fn global_pid() -> Option<u32> {
    let slot = global_slot().lock().expect("sidecar global");
    slot.as_ref().map(Sidecar::pid)
}

/// Where a request reaches the global sidecar.
#[derive(Clone)]
pub struct Endpoint {
    pub base_url: String,
    pub token: String,
    /// [`Sidecar::thinking`].
    pub thinking: super::local_effort::Thinking,
    /// Server slots (`-np`); 1 when the launch set none.
    pub slots: u32,
    /// The slots share one KV pool, so busy slots split its context.
    pub unified: bool,
    /// Requests on this server right now.
    pub in_flight: Arc<AtomicUsize>,
}

/// Base URL, token and how the model thinks of the global sidecar, when running.
pub fn global_endpoint() -> Option<Endpoint> {
    tick_idle_unload();
    let slot = global_slot().lock().expect("sidecar global");
    slot.as_ref().map(Sidecar::endpoint)
}

/// Mark the global sidecar as just used (keeps it warm).
pub fn touch_global() {
    let slot = global_slot().lock().expect("sidecar global");
    if let Some(side) = slot.as_ref() {
        side.last_used_secs.store(unix_secs(), Ordering::SeqCst);
    }
}

/// The model the device answers with, by file name (its GGUF stem): the
/// running sidecar's, else the one the next start would load.
pub fn model_name(config_dir: &Path) -> Option<String> {
    let running = global_slot()
        .lock()
        .ok()
        .and_then(|s| s.as_ref().filter(|s| s.alive()).map(|s| s.model.clone()));
    running
        .or_else(|| resolve_paths(config_dir).map(|p| p.model))
        .and_then(|m| m.file_stem().map(|s| s.to_string_lossy().into_owned()))
}

/// Settings → Local AI → "Keep the model loaded": the running sidecar takes
/// the new idle timeout at once; the next start reads it from `local_perf`.
pub fn set_idle_unload_secs(secs: u64) {
    if let Ok(mut slot) = global_slot().lock() {
        if let Some(side) = slot.as_mut() {
            side.idle_unload_secs = secs;
        }
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
    if stop_pending_locked(&mut slot) {
        return;
    }
    let Some(side) = slot.as_ref() else {
        return;
    };
    // A request may have taken its guard while this waited for the lock.
    if in_flight() > 0 || keep_loaded() {
        return;
    }
    let last = side
        .last_used_secs
        .load(Ordering::SeqCst)
        .max(LAST_DONE_SECS.load(Ordering::SeqCst))
        .max(LAST_NEEDED_SECS.load(Ordering::SeqCst));
    if super::local_perf::idle_unload_due(last, unix_secs(), side.idle_unload_secs) {
        if let Some(side) = slot.take() {
            *LAST_IDLE_UNLOAD.lock().expect("last idle unload") =
                Some((side.model.clone(), unix_secs()));
            save_kv(&side);
            retire(side);
            health::log("stop", Some("idle"));
        }
    }
}

/// Keep warm only when needed: while the model covers for the cloud (`needed`),
/// load it ahead of the next request and keep it loaded. Once it isn't needed,
/// the idle rule unloads it [`super::local_perf::IDLE_UNLOAD_SECS`] later.
/// The load runs on its own thread; a governor pause or the crash breaker
/// still refuses it.
pub fn tick_keep_warm(config_dir: &Path, needed: bool) {
    if !needed {
        return;
    }
    LAST_NEEDED_SECS.store(unix_secs(), Ordering::SeqCst);
    if local_gov::paused() || STARTING.load(Ordering::SeqCst) {
        return;
    }
    let loaded = global_slot()
        .lock()
        .expect("sidecar global")
        .as_ref()
        .is_some_and(Sidecar::alive);
    if loaded || WARMING.swap(true, Ordering::SeqCst) {
        return;
    }
    let dir = config_dir.to_path_buf();
    let generation = WARM_GEN.load(Ordering::SeqCst);
    std::thread::spawn(move || {
        if load(&dir, Want::Warm(generation)).is_ok() {
            health::log_to(&dir, "warm", Some("covering for the cloud"), None);
        }
        WARMING.store(false, Ordering::SeqCst);
    });
}

/// Why [`load`] refuses `want` now, checked under the slot lock: a keep-warm
/// load is out of date once a stop came since it started or Local AI is off;
/// a request is refused once Local AI is off or a stop is pending.
fn refused(want: Want) -> Option<&'static str> {
    let off = !super::local::toggle_on();
    match want {
        Want::Any => None,
        Want::Request if off || STOP_PENDING.load(Ordering::SeqCst) => Some("Local AI is off"),
        Want::Request => None,
        Want::Warm(g) if off || g != WARM_GEN.load(Ordering::SeqCst) => {
            Some("the warm-up was cancelled")
        }
        Want::Warm(_) => None,
    }
}

/// Stops a sidecar taken from the slot. Every stop goes through here so it
/// bumps [`WARM_GEN`] and cancels a keep-warm load started before it.
/// Before a quiet unload: keep the chat slot's cache for the next load of
/// this model (see `local_kv`). Never on a cut for a game.
fn save_kv(side: &Sidecar) {
    if let Some(a) = side.launch.as_ref().filter(|_| side.alive()) {
        super::local_kv::save(&side.base_url(), &side.token, &side.binary, &side.model, a);
    }
}

fn retire(side: Sidecar) {
    WARM_GEN.fetch_add(1, Ordering::SeqCst);
    side.stop();
}

/// A stop asked for by [`stop_when_idle`] is waiting on a request in flight.
pub fn stop_pending() -> bool {
    STOP_PENDING.load(Ordering::SeqCst)
}

/// Stop the sidecar now, or right after the requests in flight finish.
///
/// Checked under the slot lock that [`acquire`] holds while it takes a
/// request's [`Busy`] guard, so a request can't start between the check and
/// the stop.
pub fn stop_when_idle() {
    let mut slot = global_slot().lock().expect("sidecar global");
    WARM_GEN.fetch_add(1, Ordering::SeqCst);
    STOP_PENDING.store(true, Ordering::SeqCst);
    stop_pending_locked(&mut slot);
}

/// Drop a stop asked for by [`stop_when_idle`] (Local AI turned back on).
pub fn cancel_pending_stop() {
    STOP_PENDING.store(false, Ordering::SeqCst);
}

/// Does a pending stop when nothing is in flight; the caller holds the slot.
fn stop_pending_locked(slot: &mut Option<Sidecar>) -> bool {
    if !STOP_PENDING.load(Ordering::SeqCst) || in_flight() > 0 {
        return false;
    }
    STOP_PENDING.store(false, Ordering::SeqCst);
    if let Some(side) = slot.take() {
        retire(side);
        health::log("stop", Some("local AI turned off"));
    }
    true
}

/// Stop the global sidecar (tests / quit).
pub fn stop_global() {
    set_last_error(None);
    let mut slot = global_slot().lock().expect("sidecar global");
    WARM_GEN.fetch_add(1, Ordering::SeqCst);
    if let Some(side) = slot.take() {
        retire(side);
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

/// Minimal HTTP peer for contract tests: answers `/health`, `/props` and
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
        if first.starts_with("GET /props") {
            if !auth_ok {
                write_resp(stream, "401 Unauthorized", r#"{"error":"bad token"}"#);
                return Ok(());
            }
            let body = r#"{"default_generation_settings":{"n_ctx":8192},"total_slots":1,"chat_template_caps":{"supports_reasoning_effort":true,"reasoning_effort_levels":["low","high"]}}"#;
            write_resp(stream, "200 OK", body);
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

    #[test]
    fn the_pick_comes_back_only_once_a_game_has_been_gone_a_while() {
        let gone = GAME_OVER_SECS * 1_000;
        assert!(!game_settled(1_000_000, 0), "no game seen");
        assert!(!game_settled(1_000_000 + gone - 1, 1_000_000), "alt-tab");
        assert!(game_settled(1_000_000 + gone, 1_000_000));
    }

    /// A long-running stand-in child (not llama-server) in a [`Sidecar`].
    #[test]
    fn a_request_waits_for_the_speed_test_and_asks_it_to_stop() {
        let gate = speed_test_gate();
        assert!(!speed_test_wanted_back());
        // A keep-warm load waits too, but doesn't count as someone waiting.
        let warm = std::thread::spawn(|| wait_for_speed_test(Want::Warm(0)));
        let req = std::thread::spawn(|| wait_for_speed_test(Want::Request));
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while !speed_test_wanted_back() {
            assert!(
                std::time::Instant::now() < deadline,
                "the request never waited"
            );
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert_eq!(WAITING.load(Ordering::SeqCst), 1);
        drop(gate);
        req.join().unwrap();
        warm.join().unwrap();
        assert!(!speed_test_wanted_back());
    }

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
            bare_cpu: false,
            thinking: Default::default(),
            stand_in: None,
            in_flight: Arc::new(AtomicUsize::new(0)),
            health_timeout: HEALTH_TIMEOUT,
        }
    }

    #[test]
    fn a_server_that_exited_reads_ready_and_is_reaped() {
        let _one = SLOT_TESTS.lock().unwrap_or_else(|e| e.into_inner());
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
        // Windows ends a killed process asynchronously, so give it a moment to exit.
        let deadline = Instant::now() + Duration::from_secs(10);
        while runtime(&dir) == Runtime::Running && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
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

    /// The tests below share the process-wide slot; `cargo test` runs them on threads.
    static SLOT_TESTS: Mutex<()> = Mutex::new(());

    #[test]
    fn keep_warm_holds_the_model_while_needed_and_unloads_after_the_timeout() {
        let _one = SLOT_TESTS.lock().unwrap_or_else(|e| e.into_inner());
        stop_global();
        let dir = std::env::temp_dir().join(format!("gh-sidecar-warm-{}", std::process::id()));
        let now = unix_secs();
        *global_slot().lock().unwrap() = Some(idle_child_sidecar(600, now - 1_000));
        LAST_DONE_SECS.store(now - 1_000, Ordering::SeqCst);
        // Online with the cloud up: nothing to keep warm.
        LAST_NEEDED_SECS.store(0, Ordering::SeqCst);
        tick_keep_warm(&dir, false);
        assert_eq!(LAST_NEEDED_SECS.load(Ordering::SeqCst), 0);
        // Offline or the cloud down: the loaded model stays, however long it sat idle.
        tick_keep_warm(&dir, true);
        assert!(
            !WARMING.load(Ordering::SeqCst),
            "already loaded: no second load"
        );
        tick_idle_unload();
        assert!(
            global_pid().is_some(),
            "needed as the fallback, so it stays loaded"
        );
        // The cloud is back: it stays for the timeout, counted from then.
        tick_keep_warm(&dir, false);
        tick_idle_unload();
        assert!(global_pid().is_some(), "unloads only after the timeout");
        LAST_NEEDED_SECS.store(now - 1_000, Ordering::SeqCst);
        tick_idle_unload();
        assert_eq!(
            global_pid(),
            None,
            "idle past the timeout once it isn't needed"
        );
        LAST_NEEDED_SECS.store(0, Ordering::SeqCst);
    }

    #[test]
    fn idle_unload_waits_for_requests_in_flight_and_counts_from_the_last_one_done() {
        let _one = SLOT_TESTS.lock().unwrap_or_else(|e| e.into_inner());
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

    #[test]
    fn a_reload_soon_after_an_idle_unload_doubles_that_models_timer() {
        let _one = SLOT_TESTS.lock().unwrap_or_else(|e| e.into_inner());
        stop_global();
        let dir = std::env::temp_dir().join(format!("grokhub-learn-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let now = unix_secs();
        *global_slot().lock().unwrap() = Some(idle_child_sidecar(600, now - 1_000));
        LAST_DONE_SECS.store(now - 1_000, Ordering::SeqCst);
        tick_idle_unload();
        assert_eq!(global_pid(), None);
        let m = Path::new("m.gguf");
        assert_eq!(learned_idle(&dir, m, 600), 1_200, "back within a minute");
        assert_eq!(learned_idle(&dir, m, 600), 1_200, "kept for the next start");
        assert_eq!(learned_idle(&dir, Path::new("other.gguf"), 600), 600);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_running_model_is_swapped_only_with_nothing_in_flight() {
        let _one = SLOT_TESTS.lock().unwrap_or_else(|e| e.into_inner());
        stop_global();
        let dir = std::env::temp_dir();
        let side = idle_child_sidecar(600, unix_secs());
        let home = Path::new("m.gguf");
        assert_eq!(
            swap_plan(&dir, home, Some(&side), Want::Request),
            Plan::Keep
        );
        assert_eq!(
            swap_plan(&dir, Path::new("other.gguf"), Some(&side), Want::Request),
            Plan::Load {
                model: PathBuf::from("other.gguf"),
                stand_in: None
            },
            "a new pick replaces the running model"
        );
        let held = busy();
        assert_eq!(
            swap_plan(&dir, Path::new("other.gguf"), Some(&side), Want::Request),
            Plan::Keep,
            "a request in flight keeps its model"
        );
        drop(held);
    }

    #[test]
    fn a_kept_loaded_model_outlasts_the_idle_timeout_and_shows_as_loaded() {
        let _one = SLOT_TESTS.lock().unwrap_or_else(|e| e.into_inner());
        stop_global();
        let now = unix_secs();
        *global_slot().lock().unwrap() = Some(idle_child_sidecar(600, now - 1_000));
        LAST_DONE_SECS.store(now - 1_000, Ordering::SeqCst);
        let l = loaded().expect("a running model shows as loaded");
        assert_eq!(
            (l.model, l.idle_unload_secs),
            (PathBuf::from("m.gguf"), 600)
        );
        set_keep_loaded(true);
        tick_idle_unload();
        assert!(global_pid().is_some(), "keep loaded skips the idle unload");
        set_keep_loaded(false);
        tick_idle_unload();
        assert_eq!(
            global_pid(),
            None,
            "letting it unload goes back to the timeout"
        );
        assert!(loaded().is_none());
    }

    #[test]
    fn turning_local_ai_off_stops_now_or_right_after_the_request_in_flight() {
        let _one = SLOT_TESTS.lock().unwrap_or_else(|e| e.into_inner());
        stop_global();
        let now = unix_secs();
        *global_slot().lock().unwrap() = Some(idle_child_sidecar(600, now));
        stop_when_idle();
        assert_eq!(global_pid(), None, "off with nothing in flight stops now");

        *global_slot().lock().unwrap() = Some(idle_child_sidecar(600, now));
        let held = busy();
        stop_when_idle();
        assert!(
            global_pid().is_some(),
            "off never cuts off a request in flight"
        );
        drop(held);
        assert_eq!(
            global_pid(),
            None,
            "the stop happens as soon as the last request finishes, not after the idle timeout"
        );

        *global_slot().lock().unwrap() = Some(idle_child_sidecar(600, now));
        let held = busy();
        stop_when_idle();
        cancel_pending_stop();
        drop(held);
        assert!(global_pid().is_some(), "turning it back on keeps the model");
        stop_global();
    }

    #[test]
    fn a_keep_warm_load_started_before_a_stop_loads_nothing() {
        let _one = SLOT_TESTS.lock().unwrap_or_else(|e| e.into_inner());
        stop_global();
        assert_eq!(
            refused(Want::Any),
            None,
            "tests and ensure_global always load"
        );
        let started = WARM_GEN.load(Ordering::SeqCst);
        stop_when_idle();
        assert!(
            refused(Want::Warm(started)).is_some(),
            "Local AI turned off after the warm-up began"
        );
        let started = WARM_GEN.load(Ordering::SeqCst);
        stop_global();
        assert!(
            refused(Want::Warm(started)).is_some(),
            "model switched or app quitting"
        );

        let now = unix_secs();
        *global_slot().lock().unwrap() = Some(idle_child_sidecar(1, now - 1_000));
        LAST_DONE_SECS.store(now - 1_000, Ordering::SeqCst);
        LAST_NEEDED_SECS.store(0, Ordering::SeqCst);
        let started = WARM_GEN.load(Ordering::SeqCst);
        tick_idle_unload();
        assert_eq!(global_pid(), None, "idle past the timeout unloads");
        assert!(
            refused(Want::Warm(started)).is_some(),
            "an idle unload cancels a warm-up started before it too"
        );
    }

    #[test]
    fn a_request_that_checked_the_toggle_before_it_went_off_starts_nothing() {
        let _one = SLOT_TESTS.lock().unwrap_or_else(|e| e.into_inner());
        stop_global();
        let now = unix_secs();
        *global_slot().lock().unwrap() = Some(idle_child_sidecar(600, now));
        let held = busy();
        stop_when_idle();
        assert_eq!(
            refused(Want::Request),
            Some("Local AI is off"),
            "a new request waits out the pending stop instead of joining it"
        );
        assert_eq!(
            refused(Want::Any),
            None,
            "only routed requests check the toggle"
        );
        drop(held);
        assert_eq!(
            global_pid(),
            None,
            "the request in flight finished, so it stopped"
        );
        cancel_pending_stop();
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
        assert!(args
            .windows(2)
            .any(|w| w == ["--reasoning-format", "deepseek"]));
        // Perf defaults (PR C) ride along even with the CPU floor.
        assert!(args.windows(2).any(|w| w[0] == "-ngl"));
        assert!(args.iter().any(|a| a == "--cache-prompt"));
    }

    fn gpu_launch() -> LaunchArgs {
        LaunchArgs {
            backend: grokhub_core::local_profile::Backend::Vulkan,
            ngl: 49,
            all_layers: 49,
            n_cpu_moe: Some(4),
            ctx: 8192,
            kv: autoconfig::KvType::Q8,
            threads: 4,
            threads_batch: None,
            flash_attn: true,
            mlock: false,
            tensor_split: Some(vec![3, 1]),
            draft: None,
            cpu_only: false,
            run: Default::default(),
        }
    }

    #[test]
    fn gaming_runs_the_model_on_the_cpu_with_half_the_threads() {
        let g = autoconfig::gaming_launch(&LaunchArgs {
            threads: 16,
            threads_batch: Some(16),
            ..gpu_launch()
        });
        assert!(g.cpu_only);
        assert_eq!(
            (g.ngl, g.n_cpu_moe, g.tensor_split.clone()),
            (0, None, None)
        );
        assert_eq!((g.threads, g.threads_batch), (8, Some(8)));
        assert!(g.to_args().windows(2).any(|w| w == ["--device", "none"]));
        let two = autoconfig::gaming_launch(&gpu_launch());
        assert_eq!(two.threads, 2, "never under two threads");
    }

    #[test]
    fn a_start_that_crashed_on_the_gpu_is_retried_on_the_cpu_once() {
        let died = "local model server stopped before it was ready";
        let vk = "ggml_vulkan: Failed to create device";
        let gpu = gpu_launch();
        let cpu = autoconfig::on_cpu(&gpu);
        // A crash while a GPU was in play: try the CPU.
        assert!(retry_on_cpu(died, vk, Some(&gpu), false));
        assert!(retry_on_cpu(died, "", None, false));
        // Already on the CPU: nothing left to drop.
        assert!(!retry_on_cpu(died, vk, Some(&cpu), false));
        assert!(!retry_on_cpu(died, vk, None, true));
        // Out of memory keeps its own retry (fewer layers, smaller model).
        assert!(!retry_on_cpu(
            died,
            "ggml_vulkan: Device memory allocation of size 1 failed: ErrorOutOfDeviceMemory",
            Some(&gpu),
            false
        ));
        // A slow load is not a crash.
        assert!(!retry_on_cpu(
            "local model server did not become healthy in time",
            "",
            Some(&gpu),
            false
        ));
    }

    #[test]
    fn the_cpu_retry_leaves_every_gpu_out() {
        let m = Path::new("/m.gguf");
        let cpu = autoconfig::on_cpu(&gpu_launch());
        let a = spawn_args(m, "127.0.0.1", 4711, Some(&cpu), false);
        assert!(a.windows(2).any(|w| w == ["-ngl", "0"]));
        assert!(a.windows(2).any(|w| w == ["--device", "none"]));
        assert!(!a
            .iter()
            .any(|x| x == "--tensor-split" || x == "--n-cpu-moe"));
        // The one-shot defaults get the same flags.
        let bare = spawn_args(m, "127.0.0.1", 4711, None, true);
        assert!(bare.ends_with(&CPU_ONLY_ARGS.map(String::from)));
        // A normal GPU start is unchanged.
        let gpu = spawn_args(m, "127.0.0.1", 4711, Some(&gpu_launch()), false);
        assert!(!gpu.iter().any(|x| x == "--device"));
        assert!(!spawn_args(m, "127.0.0.1", 4711, None, false)
            .iter()
            .any(|x| x == "--device"));
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
            cpu_only: false,
            run: Default::default(),
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
        let err = spawn_llama(
            &evil,
            Path::new("/m.gguf"),
            "127.0.0.1",
            9,
            "tok",
            None,
            false,
        )
        .unwrap_err();
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
        let err = spawn_llama(
            &named,
            Path::new("/m.gguf"),
            "0.0.0.0",
            9,
            "tok",
            None,
            false,
        )
        .unwrap_err();
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
    fn resolved_paths_are_reused_until_a_folder_or_file_they_came_from_changes() {
        let dir = crate::harness::test_dir("sidecar-paths-cache");
        std::fs::create_dir_all(dir.join("bin")).unwrap();
        std::fs::create_dir_all(dir.join("models")).unwrap();
        std::fs::write(dir.join("bin").join(llama_server_name()), b"x").unwrap();
        std::fs::write(dir.join("models").join("a.gguf"), b"gguf").unwrap();
        let first = resolve_paths(&dir).expect("paths");
        assert_eq!(resolve_paths(&dir), Some(first.clone()));
        assert_eq!(paths_stamp(&dir), paths_stamp(&dir), "nothing changed");
        // Swapping the model file changes the folder: the new file is found.
        std::fs::remove_file(dir.join("models").join("a.gguf")).unwrap();
        std::fs::write(dir.join("models").join("b.gguf"), b"gguf").unwrap();
        let next = resolve_paths(&dir).expect("paths");
        assert!(next.model.ends_with("b.gguf"), "{:?}", next.model);
        // A pick saved in Settings is a file it reads, so it counts too.
        let before = paths_stamp(&dir);
        set_pick(&dir, "x").unwrap();
        assert_ne!(paths_stamp(&dir), before);
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
                    Some(v) => unsafe { std::env::set_var("PATH", v) },
                    None => unsafe { std::env::remove_var("PATH") },
                }
            }
        }
        let path = RestorePath(std::env::var_os("PATH"));
        unsafe { std::env::set_var("PATH", dir.join("empty")) };
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
        unsafe { std::env::set_var("PATH", &path_dir) };
        let p = resolve_paths(&dir).expect("paths");
        assert!(p.binary.ends_with(llama_server_name()));
        match prev {
            Some(v) => unsafe { std::env::set_var("PATH", v) },
            None => unsafe { std::env::remove_var("PATH") },
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
            .find(|m| m.is_chat())
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

    #[test]
    fn a_start_waits_longer_for_a_bigger_model_file() {
        assert_eq!(health_timeout(0), Duration::from_secs(30));
        assert_eq!(health_timeout(16 << 30), Duration::from_secs(111));
        assert_eq!(health_timeout(100 << 30), Duration::from_secs(300));
    }

    #[test]
    fn an_identical_plan_never_restarts_and_a_changed_one_relaunches_warm_when_both_fit() {
        let a = gpu_launch();
        let fits = |_: &LaunchArgs| true;
        let full = |_: &LaunchArgs| false;
        assert_eq!(
            relaunch_kind(Some(&a), Some(&a.clone()), full),
            Relaunch::Skip
        );
        assert_eq!(relaunch_kind(None, None, fits), Relaunch::Skip);
        let b = LaunchArgs {
            ctx: 4_096,
            ..gpu_launch()
        };
        assert_eq!(relaunch_kind(Some(&a), Some(&b), fits), Relaunch::Warm);
        assert_eq!(relaunch_kind(Some(&a), Some(&b), full), Relaunch::Cold);
        assert_eq!(relaunch_kind(Some(&a), None, fits), Relaunch::Cold);
    }

    #[test]
    fn a_level_change_with_the_same_launch_keeps_the_model_loaded() {
        let _one = SLOT_TESTS.lock().unwrap_or_else(|e| e.into_inner());
        stop_global();
        local_gov::reset_for_test();
        *global_slot().lock().unwrap() = Some(idle_child_sidecar(600, unix_secs()));
        let pid = global_pid();
        local_gov::request_restart();
        assert!(local_gov::restart_due());
        tick_governor();
        assert_eq!(global_pid(), pid, "the same launch never restarts");
        assert!(!local_gov::restart_due());
        stop_global();
        local_gov::reset_for_test();
    }

    #[test]
    fn a_request_counts_toward_the_server_it_was_admitted_to() {
        let side = idle_child_sidecar(600, unix_secs());
        let before = in_flight();
        let held = side.busy();
        assert_eq!(side.in_flight.load(Ordering::SeqCst), 1);
        assert_eq!(in_flight(), before + 1);
        assert_eq!(
            held.endpoint().map(|e| e.base_url.as_str()),
            Some("http://127.0.0.1:9")
        );
        drop(held);
        assert_eq!(side.in_flight.load(Ordering::SeqCst), 0);
    }

    #[cfg(target_os = "linux")]
    fn running(pid: u32) -> bool {
        std::fs::read_to_string(format!("/proc/{pid}/stat")).is_ok_and(|st| !st.contains(") Z "))
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_retired_server_stops_only_once_its_requests_are_done_or_the_deadline_passes() {
        let old = idle_child_sidecar(600, unix_secs());
        let pid = old.pid();
        let held = old.busy();
        drain(old, Duration::from_secs(10));
        std::thread::sleep(Duration::from_millis(300));
        assert!(running(pid), "a request still runs on it");
        drop(held);
        let until = Instant::now() + Duration::from_secs(5);
        while running(pid) && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(!running(pid), "stopped once its last request finished");

        let stuck = idle_child_sidecar(600, unix_secs());
        let pid = stuck.pid();
        let _held = stuck.busy();
        drain(stuck, Duration::from_millis(200));
        let until = Instant::now() + Duration::from_secs(5);
        while running(pid) && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(!running(pid), "cut at the deadline");
    }

    #[cfg(unix)]
    #[test]
    fn a_cold_start_never_holds_the_slot_so_a_stop_answers_at_once() {
        use std::os::unix::fs::PermissionsExt;
        let _one = SLOT_TESTS.lock().unwrap_or_else(|e| e.into_inner());
        stop_global();
        local_gov::reset_for_test();
        let dir = crate::harness::test_dir("sidecar-slow-start");
        std::fs::create_dir_all(dir.join("bin")).unwrap();
        std::fs::create_dir_all(dir.join("models")).unwrap();
        let bin = dir.join("bin").join("llama-server");
        std::fs::write(&bin, "#!/bin/sh\nsleep 2\nexit 1\n").unwrap();
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::write(dir.join("models").join("m.gguf"), b"x").unwrap();
        let load_dir = dir.clone();
        let loading = std::thread::spawn(move || acquire(&load_dir).map(drop));
        // Wait for the cold start itself: a check that only sees the slot
        // locked for a moment also reads as Loading.
        let until = Instant::now() + Duration::from_secs(5);
        while !STARTING.load(Ordering::SeqCst) && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(runtime(&dir), Runtime::Loading);
        let began = Instant::now();
        stop_global();
        assert_eq!(global_pid(), None);
        assert!(
            began.elapsed() < Duration::from_millis(500),
            "waited {:?} on a loading model",
            began.elapsed()
        );
        assert_eq!(
            loading.join().unwrap(),
            Err("local model server stopped before it was ready".into())
        );
        local_gov::reset_for_test();
        set_last_error(None);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_game_cuts_a_running_gpu_turn_at_once_and_says_why() {
        let _one = SLOT_TESTS.lock().unwrap_or_else(|e| e.into_inner());
        stop_global();
        local_gov::reset_for_test();
        *global_slot().lock().unwrap() = Some(idle_child_sidecar(600, unix_secs()));
        let pid = global_pid().unwrap();
        let turn = busy();
        local_gov::apply(
            grokhub_core::local_governor::Level::Unload,
            "a game is running",
        );
        let began = Instant::now();
        tick_governor();
        assert_eq!(global_pid(), None, "cut with a request still in flight");
        assert!(!running(pid));
        assert!(began.elapsed() < Duration::from_secs(2));
        assert_eq!(
            cut_reason(),
            Some("the on-device model stopped because a game started")
        );
        drop(turn);
        CUT_AT_MS.store(0, Ordering::SeqCst);
        local_gov::reset_for_test();
    }

    #[cfg(unix)]
    #[test]
    fn while_a_game_holds_the_gpu_a_new_request_starts_the_model_on_the_cpu() {
        use std::os::unix::fs::PermissionsExt;
        let _one = SLOT_TESTS.lock().unwrap_or_else(|e| e.into_inner());
        stop_global();
        local_gov::reset_for_test();
        let dir = crate::harness::test_dir("sidecar-game-cpu");
        std::fs::create_dir_all(dir.join("bin")).unwrap();
        std::fs::create_dir_all(dir.join("models")).unwrap();
        let bin = dir.join("bin").join("llama-server");
        let seen = dir.join("args.txt");
        std::fs::write(
            &bin,
            format!("#!/bin/sh\necho \"$@\" > {}\nexit 1\n", seen.display()),
        )
        .unwrap();
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::write(dir.join("models").join("m.gguf"), b"x").unwrap();
        *global_slot().lock().unwrap() = Some(idle_child_sidecar(600, unix_secs()));
        let gpu_pid = global_pid().unwrap();
        local_gov::apply(
            grokhub_core::local_governor::Level::Throttle,
            "a game is running",
        );
        assert!(acquire(&dir).is_err());
        assert!(!running(gpu_pid), "the server on the GPU stopped");
        let args = std::fs::read_to_string(&seen).unwrap();
        assert!(args.contains("--device none"), "started on the CPU: {args}");
        local_gov::reset_for_test();
        set_last_error(None);
    }

    #[cfg(unix)]
    #[test]
    fn background_work_never_starts_the_model_and_rides_on_one_already_loaded() {
        use std::os::unix::fs::PermissionsExt;
        let _one = SLOT_TESTS.lock().unwrap_or_else(|e| e.into_inner());
        stop_global();
        local_gov::reset_for_test();
        let dir = crate::harness::test_dir("sidecar-background");
        std::fs::create_dir_all(dir.join("bin")).unwrap();
        std::fs::create_dir_all(dir.join("models")).unwrap();
        let bin = dir.join("bin").join("llama-server");
        let seen = dir.join("args.txt");
        std::fs::write(
            &bin,
            format!("#!/bin/sh\necho \"$@\" > {}\nexit 1\n", seen.display()),
        )
        .unwrap();
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::write(dir.join("models").join("m.gguf"), b"x").unwrap();
        let _on = crate::route::local::ForceOn::with(None);
        let _need =
            super::super::local_scale::NeedGuard::set(super::super::local_scale::Need::Background);
        let none = acquire_for_request(&dir).expect("no error, just no server");
        assert!(none.endpoint().is_none());
        drop(none);
        assert!(!seen.exists(), "no engine started");
        assert!(global_pid().is_none());
        *global_slot().lock().unwrap() = Some(idle_child_sidecar(600, unix_secs()));
        let pid = global_pid().unwrap();
        let on = acquire_for_request(&dir).expect("the loaded model answers");
        assert!(on.endpoint().is_some());
        drop(on);
        assert_eq!(global_pid(), Some(pid), "the loaded model stayed");
        assert!(!seen.exists(), "still no engine started");
        stop_global();
        set_last_error(None);
    }
}
