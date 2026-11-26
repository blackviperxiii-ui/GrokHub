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
use grokhub_core::local_draft as ld;
use grokhub_core::local_setup::LocalModel;
use grokhub_core::local_skills::TaskKind;

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
    /// Managed local embeddings slot model ID (router mode), when resident.
    pub embedding_model: Option<String>,
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
    /// Models a start refused before this one ran.
    refused: super::local_swap::Refusals,
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
            .field("sees_pictures", &self.sees_pictures())
            .field("embedding_model", &self.embedding_model)
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
/// Of [`IN_FLIGHT`], the app's own background requests.
static BACKGROUND_IN_FLIGHT: AtomicUsize = AtomicUsize::new(0);
/// How long a chat's ask waits for background requests in flight before it
/// swaps the model and cuts them.
const ASK_DRAIN: Duration = Duration::from_secs(10);

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
    background: bool,
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
        if self.background {
            BACKGROUND_IN_FLIGHT.fetch_sub(1, Ordering::SeqCst);
        }
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
    super::local_habit::note_request(unix_secs());
    let slot = global_slot().lock().unwrap_or_else(|e| e.into_inner());
    match slot.as_ref() {
        Some(side) => side.busy(),
        None => Busy {
            side: None,
            end: None,
            background: count_in(),
        },
    }
}

/// Loopback `(base_url, token, model_id)` for the running sidecar's managed
/// local embeddings slot (router mode), if an embedding GGUF is resident.
pub fn sidecar_embedding_endpoint() -> Option<(String, String, String)> {
    let slot = global_slot().lock().unwrap_or_else(|e| e.into_inner());
    let side = slot.as_ref()?;
    let model = side.embedding_model.clone()?;
    Some((side.base_url(), side.token.clone(), model))
}

pub fn in_flight() -> usize {
    IN_FLIGHT.load(Ordering::SeqCst)
}

/// Of [`in_flight`], the app's own background requests.
pub fn background_in_flight() -> usize {
    BACKGROUND_IN_FLIGHT.load(Ordering::SeqCst)
}

/// Counts a request in flight; true when it is the app's own background
/// work (this thread's need).
fn count_in() -> bool {
    let background = super::local_scale::current_need() == super::local_scale::Need::Background;
    if background {
        BACKGROUND_IN_FLIGHT.fetch_add(1, Ordering::SeqCst);
    }
    IN_FLIGHT.fetch_add(1, Ordering::SeqCst);
    background
}

/// Waits until nothing is in flight, up to `deadline`: true when nothing is.
fn wait_for_idle(deadline: Duration) -> bool {
    let until = Instant::now() + deadline;
    while in_flight() > 0 {
        if Instant::now() >= until {
            return false;
        }
        std::thread::sleep(HEALTH_POLL);
    }
    true
}

impl Sidecar {
    /// Base URL for OpenAI-compatible calls (`http://127.0.0.1:<port>`).
    pub fn base_url(&self) -> String {
        format!("http://{}:{}", self.host, self.port)
    }

    /// A request on this server (see [`Busy`]).
    fn busy(&self) -> Busy {
        let background = count_in();
        self.in_flight.fetch_add(1, Ordering::SeqCst);
        Busy {
            side: Some(self.in_flight.clone()),
            end: Some(self.endpoint()),
            background,
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
            sees_pictures: self.sees_pictures(),
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
                return Err(DIED.into());
            }
            std::thread::sleep(HEALTH_POLL);
        }
        Err("local model server did not become healthy in time".into())
    }

    /// How the child ended, once it has.
    pub fn exit_status(&self) -> Option<std::process::ExitStatus> {
        let mut child = self.child.lock().expect("sidecar lock");
        child.try_wait().ok().flatten()
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

    /// This launch carries its vision projector, so it reads pictures.
    pub fn sees_pictures(&self) -> bool {
        self.launch.as_ref().is_some_and(|l| l.projector.is_some())
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
        // Tool arguments are constrained to each tool's JSON schema. b12017
        // builds that grammar from the request tools only when jinja is on
        // (it is the default; pass it so a later default cannot drop it).
        "--jinja".into(),
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

/// Spawns llama-server. Router mode (one server for the chat and embedding
/// models) is used when `router_ok`, the engine has it and there is more than
/// one model; the `bool` says whether it was.
#[allow(clippy::too_many_arguments)]
fn spawn_llama(
    binary: &Path,
    model: &Path,
    host: &str,
    port: u16,
    token: &str,
    launch: Option<&LaunchArgs>,
    bare_cpu: bool,
    router_ok: bool,
) -> Result<(Child, bool), String> {
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
    // A vision projector rides in the launch (`launch.projector`), planned
    // and counted for a picture turn: it is never picked up from the folder,
    // where another model's projector may sit.
    let models_dir = model
        .parent()
        .and_then(Path::parent)
        .or_else(|| model.parent());
    let help = super::local_flags::help_text(binary);
    let preset = super::local_preset::RouterPreset::from_sidecar_plan(model, launch, models_dir);
    let use_router =
        router_ok && super::local_preset::supports_router_mode(&help) && preset.models.len() > 1;
    let mut args = if use_router {
        let preset_path = binary.with_file_name("llama-models-preset.ini");
        match preset.write_to(&preset_path) {
            Ok(written) => {
                super::local_preset::llama_router_args(&written, host, port, preset.models_max)
            }
            Err(_) => spawn_args(model, host, port, launch, bare_cpu),
        }
    } else {
        spawn_args(model, host, port, launch, bare_cpu)
    };
    // A planned launch can save and restore its KV cache (see `local_kv`),
    // and a cold one wholly on the GPU with RAM short skips the page cache.
    if let Some(l) = launch {
        let file_mb = std::fs::metadata(model).map_or(0, |m| m.len()) / (1024 * 1024);
        let facts = super::local_warm::LoadFacts {
            whole_on_gpu: !l.cpu_only && l.ngl >= l.all_layers && l.n_cpu_moe.is_none(),
            mlock: l.mlock,
            file_mb,
            free_ram_mb: autoconfig::free_now().ram_free_mb,
            cached_pct: super::local_warm::resident_pct(model),
            engine_dio: help
                .lines()
                .any(|l| l.contains("--load-mode") && l.contains("dio")),
        };
        if super::local_warm::direct_load(facts) {
            args.extend(["--load-mode".to_string(), "dio".to_string()]);
        }
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
    cmd.envs(super::local_warm::cache_env(|k| {
        std::env::var_os(k).is_some()
    }));
    // ROCm: hide the iGPU so HIP can't crash on it; leave a user value alone.
    if let Some(l) = launch {
        if let Some(p) = autoconfig::profile() {
            cmd.envs(super::local_flags::hip_visible_env(
                l.backend,
                &p.gpus,
                |k| std::env::var_os(k).is_some(),
            ));
        }
    }
    if run.low_priority {
        super::local_flags::below_normal(&mut cmd);
    }
    // Tied to the app, so a killed app doesn't leave it holding GPU memory.
    let child = super::local_tie::spawn(cmd)
        .map_err(|e| format!("could not start local model server: {e}"))?;
    if run.low_priority {
        super::local_flags::lower_priority(child.id());
    }
    Ok((child, use_router))
}

/// What [`Sidecar::wait_healthy`] says when the engine exits while loading.
pub const DIED: &str = "local model server stopped before it was ready";

/// Most characters of a log line quoted in a start error.
const LOG_LINE_CHARS: usize = 160;

fn clip_line(line: &str) -> String {
    let line = line.trim();
    match line.char_indices().nth(LOG_LINE_CHARS) {
        Some((cut, _)) => format!("{}…", &line[..cut]),
        None => line.to_string(),
    }
}

/// The first log line holding any of `keys` (lowercase), clipped.
fn log_line(log: &str, keys: &[&str]) -> Option<String> {
    log.lines()
        .find(|l| {
            let l = l.to_ascii_lowercase();
            keys.iter().any(|k| l.contains(k))
        })
        .map(clip_line)
}

/// Why the engine died while loading, from its log and exit, in words a
/// person can act on. `None` when neither says anything.
pub fn death_reason(log: &str, status: Option<std::process::ExitStatus>) -> Option<String> {
    if autoconfig::oom_in_log(log) {
        return Some("it ran out of memory loading the model".into());
    }
    if let Some(line) = log_line(log, &["unknown model architecture"]) {
        return Some(format!(
            "this engine is too old for the model ({line}). Get engine in Settings → Local AI updates it"
        ));
    }
    if let Some(line) = log_line(
        log,
        &[
            "address already in use",
            "couldn't bind",
            "failed to bind",
            "bind failed",
        ],
    ) {
        return Some(format!("its port was taken ({line})"));
    }
    if let Some(line) = log_line(
        log,
        &[
            "invalid argument",
            "unknown argument",
            "unrecognized option",
            "unknown option",
            "error while handling argument",
        ],
    ) {
        return Some(format!("it refused how it was started ({line})"));
    }
    // A backend names itself on every start, so only its error lines count.
    let gpu_error = log.lines().find(|l| {
        let l = l.to_ascii_lowercase();
        ["hip", "rocblas", "rocm", "cuda", "vulkan", "vk::"]
            .iter()
            .any(|k| l.contains(k))
            && ["error", "fail", "abort", "lost", "crash"]
                .iter()
                .any(|k| l.contains(k))
    });
    if let Some(line) = gpu_error.map(clip_line) {
        let l = line.to_ascii_lowercase();
        let backend = if l.contains("hip") || l.contains("roc") {
            "ROCm"
        } else if l.contains("cuda") {
            "CUDA"
        } else {
            "Vulkan"
        };
        return Some(format!("the {backend} GPU backend failed ({line})"));
    }
    if let Some(line) = log_line(
        log,
        &[
            "failed to load model",
            "error loading model",
            "gguf_init",
            "invalid magic",
            "failed to open",
            "no such file",
        ],
    ) {
        return Some(format!("it couldn't load the model file ({line})"));
    }
    let last = log
        .lines()
        .rev()
        .find(|l| !l.trim().is_empty())
        .map(clip_line);
    let how = status.and_then(exit_words);
    match (how, last) {
        (Some(how), Some(last)) => Some(format!("{how} (last log line: {last})")),
        (Some(how), None) => Some(how),
        (None, Some(last)) => Some(format!("it exited (last log line: {last})")),
        (None, None) => None,
    }
}

/// A process end in words: a crash signal or an exit code.
fn exit_words(status: std::process::ExitStatus) -> Option<String> {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(sig) = status.signal() {
            return Some(match sig {
                4 => "it crashed: this processor lacks an instruction the engine was built for (illegal instruction)".into(),
                6 => "it crashed (aborted)".into(),
                9 => "it was killed (SIGKILL), often by the system running out of memory".into(),
                11 => "it crashed (segmentation fault)".into(),
                other => format!("it was stopped by signal {other}"),
            });
        }
    }
    status
        .code()
        .map(|code| format!("it exited with code {code}"))
}

/// Appended to the one-shot defaults to keep the engine off every GPU.
const CPU_ONLY_ARGS: [&str; 4] = ["-ngl", "0", "--device", "none"];

/// The engine stopped while it loaded, rather than taking too long.
fn engine_died(err: &str) -> bool {
    err.contains(DIED)
}

/// Whether a failed start should be tried again with every GPU left out: the
/// engine died while loading (a GPU driver or backend crash, not a slow load
/// or out-of-memory) and this launch could still reach a GPU.
pub fn retry_on_cpu(err: &str, log: &str, launch: Option<&LaunchArgs>, bare_cpu: bool) -> bool {
    let died = engine_died(err);
    let on_gpu = match launch {
        Some(l) => !l.cpu_only,
        None => !bare_cpu,
    };
    died && on_gpu && !autoconfig::oom_in_log(log)
}

/// Start llama-server on a fresh loopback port.
pub fn start(paths: &SidecarPaths) -> Result<Sidecar, String> {
    let mut planned = autoconfig::launch_for(&paths.binary, &paths.model)?;
    // The one-shot defaults never load a drafter: before the hardware read
    // is done, read it here and plan again.
    if planned.is_none() && autoconfig::profile().is_none() && crate::device::read_now() {
        planned = autoconfig::launch_for(&paths.binary, &paths.model)?;
    }
    start_planned(paths, planned)
}

/// [`start`] with the model's vision projector beside it and its drafter.
/// `Err` when the projector isn't on disk or the three don't fit (see
/// [`autoconfig::vision_launch_with`]); a start the projector stops runs
/// without it.
pub fn start_vision(paths: &SidecarPaths) -> Result<Sidecar, String> {
    if autoconfig::profile().is_none() {
        crate::device::read_now();
    }
    match autoconfig::vision_launch_with(&paths.binary, &paths.model, None)? {
        Some(planned) => start_planned(paths, Some(planned)),
        None => Err(autoconfig::NO_PROJECTOR.into()),
    }
}

/// Bytes a start reads off the disk: the model, and the drafter file and
/// vision projector the launch loads beside it.
fn load_bytes(model: &Path, launch: Option<&LaunchArgs>) -> u64 {
    let len = |p: &Path| std::fs::metadata(p).map_or(0, |m| m.len());
    let drafter = launch
        .and_then(|l| l.draft.as_ref()?.model.as_deref())
        .map_or(0, len);
    let projector = launch
        .and_then(|l| l.projector.as_ref())
        .map_or(0, |v| len(&v.file));
    len(model) + drafter + projector
}

/// The error for a start planned as `prev` that ran out of memory;
/// `smaller` names a model to try instead, or is empty.
fn oom_error(prev: &LaunchArgs, smaller: &str) -> String {
    let how = if prev.draft.is_some() {
        "with its draft model"
    } else {
        "even with fewer GPU layers"
    };
    format!("{OUT_OF_MEMORY} {how}.{smaller}")
}

/// [`start`] with the launch already planned (a warm relaunch plans it as a
/// fresh start would, while the old server still holds its memory).
fn start_planned(
    paths: &SidecarPaths,
    planned: Option<autoconfig::Planned>,
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
    let wait = health_timeout(load_bytes(
        &paths.model,
        planned.as_ref().map(|(l, _, _)| l),
    ));
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
    // Whether the projector alone kept the last start from running (see
    // below).
    let mut blame = ProjectorBlame::default();
    // Router mode is tried first; a start it stops runs as one model.
    let mut router_ok = true;
    loop {
        let port = free_loopback_port()?;
        let token = random_token();
        let (child, routed) = spawn_llama(
            &paths.binary,
            &paths.model,
            &host,
            port,
            &token,
            launch.as_ref(),
            bare_cpu,
            router_ok,
        )?;
        let models_dir = paths
            .model
            .parent()
            .and_then(Path::parent)
            .or_else(|| paths.model.parent());
        let embedding_model = models_dir
            .and_then(super::local_preset::discover_embedding_model)
            .map(|_| super::local_preset::SLOT_EMBED.to_string());
        let mut side = Sidecar {
            child: Mutex::new(child),
            host: host.clone(),
            port,
            token,
            binary: paths.binary.clone(),
            model: paths.model.clone(),
            embedding_model,
            restarts: AtomicU64::new(0),
            last_used_secs: AtomicU64::new(unix_secs()),
            idle_unload_secs: idle,
            launch: launch.clone(),
            bare_cpu,
            thinking: Default::default(),
            stand_in: None,
            refused: Default::default(),
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
                    // When Ollama is up, prefer its effective num_ctx (Modelfile
                    // or GGUF), never above this server's window.
                    let _ = super::local::probe_ollama_num_ctx(None, Some(u64::from(l.ctx)));
                } else {
                    let _ = super::local::probe_ollama_num_ctx(None, None);
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
                // The same launch ran without the projector, so this engine
                // won't load it with this model (beside its drafter, when it
                // has one): vision leaves the model alone until the engine
                // changes.
                if blame.ran() {
                    super::local_vision::note_refused(&paths.binary, &paths.model);
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
        let status = side.exit_status();
        side.stop();
        let log = std::fs::read_to_string(server_log_path(&paths.binary)).unwrap_or_default();
        // Say why it died, from its log and exit, not only that it did.
        let err = match death_reason(&log, status).filter(|_| err == DIED) {
            Some(why) => format!("{DIED}: {why}"),
            None => err,
        };
        // Router mode is newer than the one-model start: when it stops a
        // start, the model starts alone before anything else is changed.
        if routed && !autoconfig::oom_in_log(&log) {
            health::log("router_off", Some(&err));
            router_ok = false;
            continue;
        }
        let taken = launch
            .as_mut()
            .and_then(|l| shed(l, autoconfig::oom_in_log(&log)));
        blame.failed(taken, &err);
        match taken {
            Some(Shed::Projector { .. }) => {
                health::log("projector_off", Some(&err));
                continue;
            }
            Some(Shed::Draft) => {
                health::log("draft_off", Some(&err));
                dropped_draft = true;
                continue;
            }
            None => {}
        }
        if gpu_err.is_none() && retry_on_cpu(&err, &log, launch.as_ref(), bare_cpu) {
            gpu_err = Some(err);
            match launch.as_ref() {
                Some(l) => launch = Some(autoconfig::on_cpu(l)),
                None => bare_cpu = true,
            }
            continue;
        }
        // The CPU died too, so the GPU was not why: report the first start,
        // and that the CPU didn't help.
        if let Some(first) = gpu_err.take() {
            return Err(format!("{first} (it didn't start on the CPU either)"));
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
                return Err(oom_error(prev, &smaller));
            }
        }
    }
}

/// What a failed start takes off its launch before trying again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Shed {
    /// The vision projector; `oom` when the start ran out of memory.
    Projector {
        oom: bool,
    },
    Draft,
}

/// Whether the vision projector alone kept a start from running: the engine
/// stopped while loading it (not out of memory, not a slow load), it came
/// off, and the very next start, with nothing else changed, ran.
#[derive(Debug, Default)]
struct ProjectorBlame(bool);

impl ProjectorBlame {
    /// A start failed with `err`, and `shed` came off before the next one.
    fn failed(&mut self, shed: Option<Shed>, err: &str) {
        self.0 = shed == Some(Shed::Projector { oom: false }) && engine_died(err);
    }

    /// The start that just ran is the one right after the projector came off.
    fn ran(&self) -> bool {
        self.0
    }
}

/// Take the next thing off a launch whose start failed (`oom`: out of
/// memory), before a CPU retry or fewer GPU layers. A projector comes off
/// first: the chat still runs, and its pictures are read as a note. Then a
/// drafter that won't load beside this model (another tokenizer, a broken or
/// half-written file), which stops the whole start and makes the model and
/// drafter on disk read as not ready: the model starts alone, and once that
/// works the pair stays off until a retest. Out of memory keeps its drafter
/// for its own back-off. `None` leaves the launch as it was.
fn shed(l: &mut LaunchArgs, oom: bool) -> Option<Shed> {
    if l.projector.take().is_some() {
        return Some(Shed::Projector { oom });
    }
    if !oom && l.draft.take().is_some() {
        return Some(Shed::Draft);
    }
    None
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
/// - model else: Auto's best among downloads and added files
///   ([`resolve_model_file`]), then the first `*.gguf` in those folders
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

/// Drop the cached binary/model paths so a newly downloaded GGUF is seen.
pub fn clear_paths_cache() {
    *PATHS_CACHE.lock().unwrap_or_else(|e| e.into_inner()) = None;
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

/// Model files the user added in Settings → Local AI (`ownFiles` in
/// `{config}/local_ai.json`) that are on disk and can be a chat model.
pub fn own_files(config_dir: &Path) -> Vec<PathBuf> {
    let text = match std::fs::read_to_string(config_dir.join("local_ai.json")) {
        Ok(t) => t,
        Err(_) => return Vec::new(),
    };
    let v: serde_json::Value = match serde_json::from_str(&text) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    v.get("ownFiles")
        .and_then(|x| x.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(serde_json::Value::as_str)
                .map(|p| PathBuf::from(p.trim()))
                .filter(|p| {
                    p.extension()
                        .is_some_and(|e| e.eq_ignore_ascii_case("gguf"))
                        && p.is_file()
                        && !helper_gguf(p)
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The first added model file on disk ([`own_files`]).
pub fn own_model(config_dir: &Path) -> Option<PathBuf> {
    own_files(config_dir).into_iter().next()
}

/// Auto's home model: downloads and added files scored on merit (size,
/// published or measured speed), then the first `*.gguf` in the models
/// folders. There is no picker.
fn resolve_model_file(config_dir: &Path) -> Option<PathBuf> {
    let dirs = [user_models_dir(), Some(config_dir.join("models"))];
    let mut files: Vec<grokhub_core::local_picker::AutoFile> = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for dir in dirs.iter().flatten() {
        for m in grokhub_core::model_download::downloaded(dir) {
            push_auto_file(
                &mut files,
                &mut seen,
                grokhub_core::model_download::final_path(dir, m),
                Some(m),
            );
        }
        if let Some(rec) = grokhub_core::model_download::installed(dir) {
            let path = dir.join(&rec.file);
            let model = grokhub_core::local_setup::model_by_id(&rec.id);
            push_auto_file(&mut files, &mut seen, path, model);
        }
    }
    for path in own_files(config_dir) {
        let model = path.file_name().and_then(|n| n.to_str()).and_then(|name| {
            grokhub_core::local_setup::local_models()
                .iter()
                .find(|m| m.is_chat() && m.file == name)
        });
        push_auto_file(&mut files, &mut seen, path, model);
    }
    if let Some(p) = grokhub_core::local_picker::auto_home(&files) {
        return Some(p);
    }
    for dir in dirs.into_iter().flatten() {
        if let Some(p) = first_gguf(&dir) {
            return Some(p);
        }
    }
    None
}

fn push_auto_file(
    files: &mut Vec<grokhub_core::local_picker::AutoFile>,
    seen: &mut std::collections::BTreeSet<PathBuf>,
    path: PathBuf,
    model: Option<&'static grokhub_core::local_setup::LocalModel>,
) {
    if !path.is_file() || helper_gguf(&path) || !seen.insert(path.clone()) {
        return;
    }
    let bytes = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    files.push(grokhub_core::local_picker::AutoFile {
        path,
        model,
        bytes,
        measured: None,
    });
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
        let (swap, vision) = match slot.as_ref() {
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
                let sees = dead.sees_pictures();
                retire(dead);
                (again, sees)
            }
            running => swap_plan(config_dir, &paths.model, running, want),
        };
        // A step up waits for a quiet chat, so a reply never sits through a
        // reload; a request after a quiet spell is a new task and takes it now.
        let swap = match swap {
            Plan::Later {
                model,
                stand_in: stand_in @ Some(super::local_swap::StandIn::BestFor(_)),
            } if super::local_warm::resident_pct(&model).unwrap_or(0) >= WARM_LOAD_PCT => {
                Plan::Load { model, stand_in }
            }
            Plan::Later { model, stand_in } if secs_since_use() < QUIET_SECS => {
                *PENDING_STEP_UP.lock().expect("pending step up") =
                    Some((model, stand_in, want_need(want), want_kind(want)));
                Plan::Keep
            }
            Plan::Later { model, stand_in } => Plan::Load { model, stand_in },
            other => other,
        };
        let (model, stand_in) = match swap {
            Plan::Keep | Plan::Later { .. } => (paths.model.clone(), None),
            Plan::Load { model, stand_in } => {
                // The same file, already serving, is not a switch. A path
                // that only looks different (a symlink, a prefix) must not
                // stop the server and start it again on the next turn.
                if slot.as_ref().is_some_and(|side| {
                    side.alive() && same_file(&side.model, &model) && side.sees_pictures() == vision
                }) {
                    let side = slot.as_ref().expect("same model file");
                    side.last_used_secs.store(unix_secs(), Ordering::SeqCst);
                    return Ok(side.busy());
                }
                PENDING_STEP_UP.lock().expect("pending step up").take();
                // Only a chat's ask swaps with requests in flight, and only
                // background ones (see `local_swap::plan`): new requests wait
                // while they finish, and any still running at ASK_DRAIN are cut.
                if slot.is_some() && in_flight() > 0 {
                    STARTING.store(true, Ordering::SeqCst);
                    drop(slot);
                    let idle = wait_for_idle(ASK_DRAIN);
                    slot = global_slot().lock().expect("sidecar global");
                    STARTING.store(false, Ordering::SeqCst);
                    LOAD_DONE.notify_all();
                    if !idle {
                        health::log_to(
                            config_dir,
                            "swap",
                            Some("cut background work still running for a chat's ask"),
                            None,
                        );
                    }
                    if let Some(why) = refused(want) {
                        return Err(why.into());
                    }
                }
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
        let home = paths.model.clone();
        let paths = SidecarPaths {
            binary: paths.binary,
            model,
        };
        let measured = paths.model.clone();
        let cached = super::local_warm::resident_pct(&measured);
        let (started, model, stand_in) =
            start_or_stand_in(config_dir, &home, paths, stand_in, vision);
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

/// What a start gave: the server or why not, the model it loaded and why it
/// stands in for the pick.
type Started = (
    Result<Sidecar, String>,
    PathBuf,
    Option<super::local_swap::StandIn>,
);

/// [`start`], and a smaller model on disk in its place when the start ran
/// out of memory (one step) or the model and its drafter don't fit (smaller
/// models until a pair fits). `home` is the Settings pick. Returns what
/// started, the model it loaded and why it stands in; a server that started
/// keeps the models refused on the way. `vision` starts the model with its
/// vision projector first ([`start_seeing`]).
fn start_or_stand_in(
    config_dir: &Path,
    home: &Path,
    mut paths: SidecarPaths,
    planned: Option<super::local_swap::StandIn>,
    vision: bool,
) -> Started {
    use super::local_swap::StandIn;
    let mut holds = Vec::new();
    if vision {
        // Keep loaded and a forced file keep the pick: only it may load its
        // projector.
        let pick_only = keep_loaded() || std::env::var_os("GROKHUB_LOCAL_MODEL").is_some();
        let (found, refused) = start_seeing(config_dir, home, &paths, planned, pick_only);
        holds = refused;
        if let Some(started) = found {
            return with_holds(started, holds);
        }
    }
    let picked = paths.model.clone();
    let disk = on_disk(config_dir);
    let mut walk: Option<StandIn> = None;
    let mut refused = super::local_swap::Refusals::default();
    let mut started = start(&paths);
    while let Err(e) = &started {
        let oom_before = walk == Some(StandIn::OutOfMemory);
        let Some((model, why)) = next_step(e, &paths.model, &disk, oom_before) else {
            break;
        };
        let hold = autoconfig::take_refusal(&paths.model)
            .filter(|_| e.starts_with(autoconfig::PAIR_NO_ROOM))
            .map_or(
                super::local_swap::Hold::UntilUnload,
                super::local_swap::Hold::Pair,
            );
        refused.models.push((paths.model.clone(), hold));
        health::log_to(config_dir, health::start_failure(e), Some(e), None);
        walk = Some(why);
        let label = stand_in_label(&model, home, planned, walk);
        health::log_to(config_dir, "swap", Some(&swap_note(&model, label)), None);
        paths.model = model;
        started = start(&paths);
    }
    if let Err(e) = &started {
        if e.starts_with(autoconfig::PAIR_NO_ROOM) {
            started = Err(pair_no_room_error(&picked, e));
        }
    }
    if let Ok(side) = started.as_mut() {
        note_pick_refused(home, &paths.model, &refused);
        refused.since_ms = unix_ms();
        side.refused = refused;
    }
    let label = stand_in_label(&paths.model, home, planned, walk);
    with_holds((started, paths.model, label), holds)
}

/// Vision launches a picture start found wouldn't run, to hold on the
/// server that runs after it (see [`hold_vision`]).
type VisionHolds = Vec<(PathBuf, super::local_swap::Hold)>;

/// `started` with `holds` kept on its server, when one started.
fn with_holds(started: Started, holds: VisionHolds) -> Started {
    if let Ok(side) = &started.0 {
        for (model, hold) in holds {
            hold_vision(&side.token, model, hold);
        }
    }
    started
}

/// [`start_vision`] for the model in `paths`, else for the next smaller
/// model on disk whose projector this engine loads ([`next_seeing`]), until
/// one starts with its projector. A start that shed its projector tries the
/// next one too; when none is left, the planned model runs as it started,
/// and a smaller one stops so the planned model runs instead: no smaller
/// model stands in without seeing the picture. `None` starts the planned
/// model without a projector. With `pick_only` (Keep loaded, a forced file)
/// only the planned model is tried. Each model that didn't run with its
/// projector comes back to be held, so the next picture turn doesn't try it
/// again (it may still fit without its projector, so the walk holds nothing
/// against it).
fn start_seeing(
    config_dir: &Path,
    home: &Path,
    paths: &SidecarPaths,
    planned: Option<super::local_swap::StandIn>,
    pick_only: bool,
) -> (Option<Started>, VisionHolds) {
    use super::local_swap::{Hold, StandIn};
    let disk = on_disk(config_dir);
    let seeing = autoconfig::vision_view(&paths.binary, &disk);
    let mut holds = VisionHolds::new();
    let mut model = paths.model.clone();
    loop {
        let try_paths = SidecarPaths {
            binary: paths.binary.clone(),
            model: model.clone(),
        };
        let label = if model == paths.model {
            planned
        } else {
            (model != home).then_some(StandIn::Vision)
        };
        let next = next_seeing(&model, &disk, &seeing, pick_only);
        match start_vision(&try_paths) {
            Ok(side) if side.sees_pictures() => return (Some((Ok(side), model, label)), holds),
            // Its projector came off at start.
            Ok(side) => {
                health::log_to(config_dir, "vision_off", Some(PROJECTOR_SHED), None);
                holds.push((model.clone(), Hold::UntilUnload));
                match &next {
                    Some(_) => side.stop(),
                    None if model == paths.model => {
                        return (Some((Ok(side), model, label)), holds);
                    }
                    None => {
                        side.stop();
                        return (None, holds);
                    }
                }
            }
            Err(e) => {
                health::log_to(config_dir, "vision_off", Some(&e), None);
                let hold = match autoconfig::vision_fit(&paths.binary, &model, None) {
                    Err(Some(r)) => Hold::Pair(r),
                    _ => Hold::UntilUnload,
                };
                holds.push((model.clone(), hold));
            }
        }
        let Some(next) = next else {
            return (None, holds);
        };
        health::log_to(
            config_dir,
            "swap",
            Some(&swap_note(&next, Some(StandIn::Vision))),
            None,
        );
        model = next;
    }
}

/// The health log's note when a start ran only once its projector came off.
const PROJECTOR_SHED: &str = "the model started only without its vision projector";

/// The model [`start_seeing`] tries after `model`: the biggest smaller one
/// on disk whose projector this engine loads (`seeing`). None with
/// `pick_only`, or for a model the catalog doesn't list.
fn next_seeing(
    model: &Path,
    disk: &[(&'static LocalModel, PathBuf)],
    seeing: &[&'static str],
    pick_only: bool,
) -> Option<PathBuf> {
    if pick_only {
        return None;
    }
    let size = super::local_swap::catalog_for(model)?.run_mb;
    disk.iter()
        .filter(|(m, _)| m.run_mb < size && seeing.contains(&m.id))
        .max_by_key(|(m, _)| m.run_mb)
        .map(|(_, p)| p.clone())
}

/// The last time the walk refused the pick: the pick, and the model that
/// started in its place. It outlives that model's unload, so the speed test
/// still times the model that runs for the pick; the pick loading clears it.
static PICK_REFUSED: Mutex<Option<(PathBuf, PathBuf)>> = Mutex::new(None);

/// Keep [`PICK_REFUSED`] after a walk for the pick `home` started `started`
/// with `refused` on the way.
pub(crate) fn note_pick_refused(
    home: &Path,
    started: &Path,
    refused: &super::local_swap::Refusals,
) {
    let Ok(mut last) = PICK_REFUSED.lock() else {
        return;
    };
    if started == home {
        *last = None;
    } else if refused.models.iter().any(|(m, _)| m == home) {
        *last = Some((home.to_path_buf(), started.to_path_buf()));
    }
}

/// The model that started in place of `pick` the last time a start refused
/// it, until the pick itself loads.
pub fn stand_in_for_refused(pick: &Path) -> Option<PathBuf> {
    let last = PICK_REFUSED.lock().ok()?;
    last.as_ref()
        .filter(|(p, _)| p == pick)
        .map(|(_, s)| s.clone())
}

/// The model to try after `failed` gave `err`, and why it stands in: a
/// smaller model for a pair that doesn't fit, or one that ran out of memory.
fn stand_in_for(
    err: &str,
    failed: &Path,
    disk: &[(&'static LocalModel, PathBuf)],
) -> Option<(PathBuf, super::local_swap::StandIn)> {
    use super::local_swap::StandIn;
    let why = if err.starts_with(autoconfig::PAIR_NO_ROOM) {
        StandIn::Tight
    } else if err.starts_with(OUT_OF_MEMORY) {
        StandIn::OutOfMemory
    } else {
        return None;
    };
    super::local_swap::out_of_memory_stand_in(failed, disk).map(|m| (m, why))
}

/// One step of [`start_or_stand_in`]'s walk after `failed` gave `err`. Out
/// of memory steps down once: after it (`oom_before`) a second one stops the
/// walk, and a pair that doesn't fit still steps as
/// [`StandIn::OutOfMemory`](super::local_swap::StandIn::OutOfMemory), so the
/// planner holds the stand-in until the next unload.
fn next_step(
    err: &str,
    failed: &Path,
    disk: &[(&'static LocalModel, PathBuf)],
    oom_before: bool,
) -> Option<(PathBuf, super::local_swap::StandIn)> {
    use super::local_swap::StandIn;
    let (model, why) = stand_in_for(err, failed, disk)?;
    match (why, oom_before) {
        (StandIn::OutOfMemory, true) => None,
        (_, true) => Some((model, StandIn::OutOfMemory)),
        _ => Some((model, why)),
    }
}

/// Why `model` runs instead of the pick `home`: the plan's reason
/// (`planned`) when the walk didn't step (`walk` is `None`) or still runs a
/// bigger model than the pick, none when it landed on the pick itself, else
/// the walk's reason.
fn stand_in_label(
    model: &Path,
    home: &Path,
    planned: Option<super::local_swap::StandIn>,
    walk: Option<super::local_swap::StandIn>,
) -> Option<super::local_swap::StandIn> {
    let Some(why) = walk else {
        return planned;
    };
    if model == home {
        return None;
    }
    let size = |p: &Path| super::local_swap::catalog_for(p).map(|m| m.run_mb);
    match (size(model), size(home)) {
        (Some(m), Some(h)) if m > h => planned,
        _ => Some(why),
    }
}

/// The error when no model on disk fits with its drafter; `err` is the last
/// refusal, which may name the Settings GPU layers override as the cause.
fn pair_no_room_error(picked: &Path, err: &str) -> String {
    let name = super::local_swap::catalog_for(picked).map_or_else(
        || {
            picked
                .file_stem()
                .map_or_else(|| "the model".into(), |s| s.to_string_lossy().into_owned())
        },
        |m| m.name.to_string(),
    );
    if err.contains(autoconfig::PAIR_OVERRIDE) {
        return format!(
            "There isn't room for {name} and its draft model together, because the GPU layers setting in Settings → Local model keeps part of the model off the GPU, and no smaller model on disk fits with its draft. Clear the GPU layers setting, or download a smaller model."
        );
    }
    format!(
        "There isn't room for {name} and its draft model together, and no smaller model on disk fits with its draft. Close some apps, or download a smaller model in Settings → Local model."
    )
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
    // A launch that carries its projector keeps it while it still fits.
    let next = side
        .sees_pictures()
        .then(|| autoconfig::replan_vision_for(&side.binary, &side.model))
        .flatten()
        .or_else(|| autoconfig::replan_for(&side.binary, &side.model));
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
            relaunch_warm(
                paths,
                side.port,
                (side.stand_in, side.refused.clone()),
                side.idle_unload_secs,
            );
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
    (stand_in, refused): (
        Option<super::local_swap::StandIn>,
        super::local_swap::Refusals,
    ),
    idle: u64,
) {
    if RELAUNCHING.swap(true, Ordering::SeqCst) {
        return;
    }
    let generation = WARM_GEN.load(Ordering::SeqCst);
    std::thread::spawn(move || {
        let started =
            autoconfig::launch_with(&paths.binary, &paths.model, Some(autoconfig::replan_free()))
                .and_then(|planned| start_planned(&paths, planned));
        let mut slot = global_slot().lock().expect("sidecar global");
        match started {
            Ok(mut new) => {
                let current = slot.as_ref().is_some_and(|s| s.port == old_port)
                    && WARM_GEN.load(Ordering::SeqCst) == generation;
                if current {
                    new.stand_in = stand_in;
                    new.refused = refused;
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
/// A step up waiting for a quiet chat: the model, why, and the need and
/// task kind it was for.
type StepUp = (
    PathBuf,
    Option<super::local_swap::StandIn>,
    super::local_scale::Need,
    Option<TaskKind>,
);

static PENDING_STEP_UP: Mutex<Option<StepUp>> = Mutex::new(None);

/// A keep-warm load plans for the need it warms for; a request for its own.
fn want_need(want: Want) -> super::local_scale::Need {
    match want {
        Want::Warm(_) => *WARM_NEED.lock().expect("warm need"),
        Want::Any | Want::Request => super::local_scale::current_need(),
    }
}

/// A keep-warm load plans for the task kind it warms for; a request for
/// its own.
fn want_kind(want: Want) -> Option<TaskKind> {
    match want {
        Want::Warm(_) => *WARM_KIND.lock().expect("warm kind"),
        Want::Any | Want::Request => super::local_task::current_kind(),
    }
}

/// The task kind a keep-warm load plans for: a step up's, else none.
static WARM_KIND: Mutex<Option<TaskKind>> = Mutex::new(None);

/// The latest task kind other than everyday chat, so the idle tick can
/// RAM-warm the model best at it.
static LAST_TASK_KIND: Mutex<Option<TaskKind>> = Mutex::new(None);

/// The need a keep-warm load plans for: a step up's, else Normal.
static WARM_NEED: Mutex<super::local_scale::Need> = Mutex::new(super::local_scale::Need::Normal);

/// When the running model was last swapped for another.
static LAST_SWAP_SECS: AtomicU64 = AtomicU64::new(0);

/// The model the idle timer last unloaded, and when.
static LAST_IDLE_UNLOAD: Mutex<Option<(PathBuf, u64)>> = Mutex::new(None);

/// The catalog chat models on disk: downloads in the models folders, plus
/// added files that match a catalog chat model so Auto can score them.
fn on_disk(config_dir: &Path) -> Vec<(&'static LocalModel, PathBuf)> {
    let dirs: Vec<PathBuf> = [user_models_dir(), Some(config_dir.join("models"))]
        .into_iter()
        .flatten()
        .collect();
    super::local_swap::with_own(super::local_swap::on_disk(&dirs), &own_files(config_dir))
}

/// The running server loaded a drafter file beside its model.
fn loaded_paired(s: &Sidecar) -> bool {
    s.launch
        .as_ref()
        .and_then(|l| l.draft.as_ref())
        .is_some_and(|d| d.model.is_some())
}

/// What the swap planner needs from a running server: it loaded a drafter
/// file, and the models refused before it still held (see
/// [`super::local_swap::held`]). `room` is the sampler's reading and when it
/// was taken; one older than the server doesn't count.
fn running_view(
    running: Option<&Sidecar>,
    room: Option<(super::local_scale::Room, u64)>,
) -> (bool, Vec<PathBuf>) {
    let Some(s) = running else {
        return (false, Vec::new());
    };
    let now = hold_now(s.refused.since_ms, room);
    (loaded_paired(s), s.refused.holding(&now))
}

/// What a hold taken at `since_ms` is checked against: the governor state,
/// and the room reading (when newer than the hold) with the running model's
/// own memory added back, as a fresh start would see it.
fn hold_now(
    since_ms: u64,
    room: Option<(super::local_scale::Room, u64)>,
) -> super::local_swap::HoldNow {
    let free = room.filter(|(_, at)| *at >= since_ms).map(|(r, _)| {
        let now = autoconfig::FreeNow {
            vram_free_mb: r.free_vram_mb,
            ram_free_mb: Some(r.free_ram_mb),
        };
        autoconfig::with_own(now, autoconfig::own_use())
    });
    super::local_swap::HoldNow {
        state: autoconfig::state(),
        free,
    }
}

/// Vision launches found not to run while one server ran (keyed by its
/// token, new with every start): a picture turn doesn't stop that server for
/// one of them again until its hold releases (more free memory where it was
/// sized, another governor state) or the server stops.
static VISION_HOLDS: Mutex<Option<(String, super::local_swap::Refusals)>> = Mutex::new(None);

/// Hold `model`'s vision launch while the server with `token` runs.
fn hold_vision(token: &str, model: PathBuf, hold: super::local_swap::Hold) {
    let mut holds = VISION_HOLDS.lock().unwrap_or_else(|e| e.into_inner());
    match holds.as_mut() {
        Some((t, r)) if t == token => {
            r.models.retain(|(m, _)| *m != model);
            r.models.push((model, hold));
        }
        _ => {
            *holds = Some((
                token.to_string(),
                super::local_swap::Refusals {
                    models: vec![(model, hold)],
                    since_ms: unix_ms(),
                },
            ));
        }
    }
}

/// The models whose vision launch is held while the server with `token`
/// runs (see [`hold_vision`]).
fn vision_held(token: &str, room: Option<(super::local_scale::Room, u64)>) -> Vec<PathBuf> {
    let holds = VISION_HOLDS.lock().unwrap_or_else(|e| e.into_inner());
    match holds.as_ref() {
        Some((t, r)) if t == token => r.holding(&hold_now(r.since_ms, room)),
        _ => Vec::new(),
    }
}

/// How long a picture turn waits for the requests on a server that can't see
/// it before the swap stops that server anyway. Background helpers (ask
/// offers, the pulse writer) send their own requests right after each
/// person message, so something is nearly always in flight then.
pub const VISION_DRAIN: Duration = Duration::from_secs(20);

/// How a [`wait_for_vision`] ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Drain {
    /// Nothing is in flight, or nothing needed waiting for.
    Free,
    /// Requests were still in flight at the deadline.
    TimedOut,
    Cancelled,
}

/// For a picture turn: when the model loaded now can't see pictures and a
/// model on disk can, wait up to `deadline` for the requests on it to
/// finish, so the swap to a model that sees doesn't cut them. Past the
/// deadline the swap goes ahead (it stops the old server before the new one
/// starts, so the two never share memory). The slot isn't held meanwhile.
pub fn wait_for_vision(
    config_dir: &Path,
    deadline: Duration,
    cancelled: &dyn Fn() -> bool,
) -> Drain {
    let needed = {
        let slot = global_slot().lock().unwrap_or_else(|e| e.into_inner());
        slot.as_ref().is_some_and(|s| {
            // A model that can see, and whose launch isn't held.
            let held = vision_held(&s.token, super::local_scale::room_at(unix_ms()));
            let free = |id: &&'static str| {
                !held
                    .iter()
                    .any(|p| super::local_swap::catalog_for(p).is_some_and(|m| m.id == *id))
            };
            !s.sees_pictures()
                && autoconfig::vision_view(&s.binary, &on_disk(config_dir))
                    .iter()
                    .any(free)
        })
    };
    if !needed {
        return Drain::Free;
    }
    wait_until_idle(deadline, cancelled)
}

/// Wait up to `deadline` for no request to be in flight.
fn wait_until_idle(deadline: Duration, cancelled: &dyn Fn() -> bool) -> Drain {
    let until = Instant::now() + deadline;
    loop {
        if cancelled() {
            return Drain::Cancelled;
        }
        if in_flight() == 0 {
            return Drain::Free;
        }
        if Instant::now() >= until {
            return Drain::TimedOut;
        }
        std::thread::sleep(HEALTH_POLL);
    }
}

/// What the next load runs: the pick, or another model on disk for a reason
/// [`super::local_swap::plan`] gives, and whether it loads its vision
/// projector: a request whose newest message carries a picture plans with
/// [`super::local_swap::vision_plan`] when what runs can't see it, and
/// keeps what runs when it can. The caller holds the slot.
fn swap_plan(
    config_dir: &Path,
    home: &Path,
    running: Option<&Sidecar>,
    want: Want,
) -> (Plan, bool) {
    let loaded = running.map(|s| (s.model.as_path(), s.stand_in));
    let on_disk = on_disk(config_dir);
    // Background work never loads a model, so it sizes no pairs (and never
    // asks the engine for its flags).
    let binary = match running {
        _ if want_need(want) == super::local_scale::Need::Background => None,
        Some(s) => Some(s.binary.clone()),
        None => resolve_paths(config_dir).map(|p| p.binary),
    };
    let paired = binary
        .as_deref()
        .map_or_else(Vec::new, |b| autoconfig::pair_view(b, &on_disk));
    let now_ms = unix_ms();
    let scale = super::local_scale::scale_now(now_ms);
    let (loaded_paired, refused) = running_view(running, super::local_scale::room_at(now_ms));
    let task = want_kind(want);
    if let Some(k) = task.filter(|k| *k != TaskKind::Chat) {
        *LAST_TASK_KIND.lock().expect("last task kind") = Some(k);
    }
    let marks = super::local_task::marks(config_dir);
    let now = super::local_swap::Now {
        home,
        loaded,
        on_disk: &on_disk,
        paired: &paired,
        loaded_paired,
        refused: &refused,
        low_battery: super::local::low_battery(),
        kept_loaded: keep_loaded(),
        forced: std::env::var_os("GROKHUB_LOCAL_MODEL").is_some(),
        in_flight: in_flight(),
        background_in_flight: background_in_flight(),
        secs_since_swap: unix_secs().saturating_sub(LAST_SWAP_SECS.load(Ordering::SeqCst)),
        need: want_need(want),
        scale,
        task,
        dynamic: super::local_task::dynamic(config_dir),
        marks: &marks,
        chat: want_chat(want),
    };
    if !matches!(want, Want::Warm(_)) {
        let ask = super::local_swap::ask_outcome(&now)
            .map(|a| (a, super::local_swap::target_model(&now)));
        ASK_OUTCOME.with(|o| o.set(ask));
    }
    let picture = !matches!(want, Want::Warm(_))
        && want_need(want) != super::local_scale::Need::Background
        && super::local_vision::wants_picture();
    if picture {
        if running.is_some_and(Sidecar::sees_pictures) {
            return (Plan::Keep, false);
        }
        let all_seeing = binary
            .as_deref()
            .map_or_else(Vec::new, |b| autoconfig::vision_view(b, &on_disk));
        // Once the turn waited for them (see [`wait_for_vision`]), requests
        // still on the running server don't keep the picture from loading a
        // model that sees it: the swap stops that server first.
        let vnow = if super::local_vision::waited() {
            super::local_swap::Now {
                in_flight: 0,
                ..now
            }
        } else {
            now
        };
        // The launch planner checks each vision launch before what runs
        // stops: one it refuses is held, and the next is planned without it.
        let mut held = running.map_or_else(Vec::new, |s| {
            vision_held(&s.token, super::local_scale::room_at(now_ms))
        });
        loop {
            let seeing: Vec<&'static str> = all_seeing
                .iter()
                .copied()
                .filter(|id| {
                    !held
                        .iter()
                        .any(|p| super::local_swap::catalog_for(p).is_some_and(|m| m.id == *id))
                })
                .collect();
            let Some(p) = super::local_swap::vision_plan(&vnow, &seeing) else {
                break;
            };
            // With nothing running, the start walks down by itself.
            let (Some(s), Some(b), Plan::Load { model, .. }) = (running, binary.as_deref(), &p)
            else {
                return (p, true);
            };
            match autoconfig::vision_fit(b, model, Some(autoconfig::replan_free())) {
                Ok(()) => return (p, true),
                Err(r) => {
                    let hold = r.map_or(
                        super::local_swap::Hold::UntilUnload,
                        super::local_swap::Hold::Pair,
                    );
                    health::log_to(
                        config_dir,
                        "vision_off",
                        Some(&swap_note(model, None)),
                        None,
                    );
                    hold_vision(&s.token, model.clone(), hold);
                    held.push(model.clone());
                }
            }
        }
    }
    (super::local_swap::plan(now), false)
}

thread_local! {
    /// What this thread's last plan did with its request's ask, and the
    /// model it aimed at, for the line under the reply.
    static ASK_OUTCOME: std::cell::Cell<Option<(super::local_swap::AskOutcome, PathBuf)>> =
        const { std::cell::Cell::new(None) };
}

/// What the last plan on this thread did with its request's ask, and the
/// model it aimed at; it clears.
pub fn take_ask_outcome() -> Option<(super::local_swap::AskOutcome, PathBuf)> {
    ASK_OUTCOME.with(std::cell::Cell::take)
}

/// A request plans for its chat's tier; a keep-warm load for none.
fn want_chat(want: Want) -> Option<super::local_scale::ChatTier> {
    match want {
        Want::Warm(_) => None,
        Want::Any | Want::Request => super::local_scale::current_chat(),
    }
}

/// The model `best_for` would pick for `kind` with nothing loaded, for the
/// idle tick's RAM-warm.
fn best_model_for(config_dir: &Path, home: &Path, kind: TaskKind) -> Option<PathBuf> {
    let on_disk = on_disk(config_dir);
    let paired = resolve_paths(config_dir)
        .map_or_else(Vec::new, |p| autoconfig::pair_view(&p.binary, &on_disk));
    let marks = super::local_task::marks(config_dir);
    match super::local_swap::plan(super::local_swap::Now {
        home,
        loaded: None,
        on_disk: &on_disk,
        paired: &paired,
        loaded_paired: false,
        refused: &[],
        low_battery: super::local::low_battery(),
        kept_loaded: keep_loaded(),
        forced: std::env::var_os("GROKHUB_LOCAL_MODEL").is_some(),
        in_flight: 0,
        background_in_flight: 0,
        secs_since_swap: u64::MAX,
        need: super::local_scale::Need::Normal,
        scale: super::local_scale::scale_now(unix_ms()),
        task: Some(kind),
        dynamic: super::local_task::dynamic(config_dir),
        marks: &marks,
        chat: None,
    }) {
        Plan::Load {
            model,
            stand_in: Some(super::local_swap::StandIn::BestFor(_)),
        } => Some(model),
        _ => None,
    }
}

/// A model picked for the task loads now, without waiting for a quiet
/// chat, when this much of its file is already in memory: the load is then
/// a few seconds, not a read off the disk.
pub const WARM_LOAD_PCT: u8 = 80;

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
/// unload or at app start), and the model a game swaps to. Shortly before a
/// usual chat time (see `local_habit`) it looks at them all again, unless a
/// game is running.
pub fn tick_prewarm(config_dir: &Path) {
    let now = unix_secs();
    if !super::local::switched_on()
        || super::local_prewarm::busy()
        || local_gov::paused()
        || super::local_resident::held()
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
    // The model best at the latest non-chat task, so a swap to it reads
    // from memory, not the disk. Not while a game runs.
    let task_pick = (!gaming)
        .then(|| *LAST_TASK_KIND.lock().expect("last task kind"))
        .flatten()
        .and_then(|k| best_model_for(config_dir, &home, k));
    let mut files = Vec::new();
    for model in [Some(home), gaming_pick, task_pick].into_iter().flatten() {
        if loaded.as_ref() == Some(&model) {
            continue;
        }
        let drafter = autoconfig::draft_plan(&binary, &model)
            .and_then(|p| p.draft)
            .and_then(|d| d.model);
        files.push(model);
        files.extend(drafter);
    }
    let habit = !gaming && super::local_habit::tick(now);
    if super::local_prewarm::start(files, gaming, habit) && habit {
        let why = Some("a usual chat time is near");
        health::log_to(config_dir, "habit_warm", why, None);
    }
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
        && super::local_swap::catalog_for(&side.model).is_some_and(|m| {
            // Sized by what it really loaded.
            let need = ld::pair_mb(m, loaded_paired(side));
            super::local_scale::step_down_due(need, &scale)
        });
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
        find_model(config_dir).map(|home| (home, None, super::local_scale::Need::Normal, None))
    } else {
        pending
    };
    let Some((model, stand_in, need, kind)) = pending.filter(|_| quiet) else {
        return;
    };
    if side.model == model || keep_loaded() || WARMING.swap(true, Ordering::SeqCst) {
        PENDING_STEP_UP.lock().expect("pending step up").take();
        return;
    }
    PENDING_STEP_UP.lock().expect("pending step up").take();
    *WARM_NEED.lock().expect("warm need") = need;
    *WARM_KIND.lock().expect("warm kind") = kind;
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
    } else if matches!(stand_in, Some(super::local_swap::StandIn::BestFor(_))) {
        "switched to the model best at the task"
    } else {
        "stepped up for a hard task"
    };
    std::thread::spawn(move || {
        if load(&dir, Want::Warm(generation)).is_ok() {
            health::log_to(&dir, "warm", Some(why), None);
        }
        *WARM_NEED.lock().expect("warm need") = super::local_scale::Need::Normal;
        *WARM_KIND.lock().expect("warm kind") = None;
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
        Some(super::local_swap::StandIn::BestFor(kind)) => {
            format!("{name}, best on disk for {}", kind.words())
        }
        Some(super::local_swap::StandIn::Asked(tier, _)) => {
            format!(
                "{name} for a chat's {} ask",
                super::local_swap::tier_word(tier)
            )
        }
        Some(super::local_swap::StandIn::Vision) => format!("{name} to see a picture"),
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
    /// [`Sidecar::sees_pictures`].
    pub sees_pictures: bool,
}

/// Two paths name one model file. Equal paths match even when the file is
/// not on disk yet; otherwise the canonical path does, so a symlink is not
/// a different model.
fn same_file(a: &Path, b: &Path) -> bool {
    if a == b {
        return true;
    }
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(left), Ok(right)) => left == right,
        _ => false,
    }
}

/// `0` is not "unload now". Settings' shortest wait is one minute; a missing
/// or cleared timeout keeps the default.
fn idle_limit(secs: u64) -> u64 {
    match secs {
        0 => super::local_perf::IDLE_UNLOAD_SECS,
        s => s,
    }
}

/// The running server would be stopped for idle before this turn uses it.
fn idle_would_unload(side: &Sidecar) -> bool {
    if in_flight() > 0 || keep_loaded() {
        return false;
    }
    let last = side
        .last_used_secs
        .load(Ordering::SeqCst)
        .max(LAST_DONE_SECS.load(Ordering::SeqCst))
        .max(LAST_NEEDED_SECS.load(Ordering::SeqCst));
    super::local_perf::idle_unload_due(last, unix_secs(), idle_limit(side.idle_unload_secs))
}

/// A live server will answer the next request without a start: it is up, and
/// its idle timeout has not run out.
pub(crate) fn answers_now() -> bool {
    let slot = global_slot().lock().expect("sidecar global");
    slot.as_ref()
        .is_some_and(|side| side.alive() && !idle_would_unload(side))
}

/// This turn will cold-start the server, or stop it and start a different
/// model file. Looking does not start or stop anything.
pub(crate) fn will_start_or_switch(config_dir: &Path) -> bool {
    let slot = global_slot().lock().expect("sidecar global");
    let Some(side) = slot.as_ref().filter(|s| s.alive()) else {
        return true;
    };
    if idle_would_unload(side) {
        return true;
    }
    let Ok(paths) = runtime_paths(config_dir) else {
        return false;
    };
    let (plan, vision) = swap_plan(config_dir, &paths.model, Some(side), Want::Request);
    match plan {
        Plan::Load { ref model, .. } => {
            !same_file(&side.model, model) || side.sees_pictures() != vision
        }
        Plan::Keep | Plan::Later { .. } => false,
    }
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
    if super::local_perf::idle_unload_due(last, unix_secs(), idle_limit(side.idle_unload_secs)) {
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
    if local_gov::paused() || super::local_resident::held() || STARTING.load(Ordering::SeqCst) {
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

/// The loaded chat server, for the residency ledger.
#[derive(Debug, Clone)]
pub struct ChatResident {
    pub model: PathBuf,
    pub launch: Option<LaunchArgs>,
    pub bare_cpu: bool,
    /// Unix seconds it was last asked, answered or needed as the fallback.
    pub last_used_secs: u64,
    /// A request is in flight or Keep loaded holds it.
    pub pinned: bool,
}

/// The chat server's place in memory, read under one slot lock.
#[derive(Debug, Clone, Default)]
pub struct Resident {
    pub chat: Option<ChatResident>,
    /// A server is starting: its memory isn't known yet.
    pub starting: bool,
}

/// What the chat server holds now.
pub fn resident() -> Resident {
    let slot = global_slot().lock().unwrap_or_else(|e| e.into_inner());
    let chat = slot.as_ref().map(|side| ChatResident {
        model: side.model.clone(),
        launch: side.launch.clone(),
        bare_cpu: side.bare_cpu,
        last_used_secs: side
            .last_used_secs
            .load(Ordering::SeqCst)
            .max(LAST_DONE_SECS.load(Ordering::SeqCst))
            .max(LAST_NEEDED_SECS.load(Ordering::SeqCst)),
        pinned: in_flight() > 0 || keep_loaded(),
    });
    Resident {
        chat,
        starting: STARTING.load(Ordering::SeqCst),
    }
}

/// Unload the chat server serving `model` to make room for another job,
/// keeping its cache as a quiet unload does. Checked under the slot lock:
/// with a request in flight, Keep loaded on, or another model loaded since
/// it was priced, it stays and this returns false. Nothing loaded is true.
pub fn unload_for(reason: &str, model: &Path) -> bool {
    let mut slot = global_slot().lock().expect("sidecar global");
    match slot.as_ref() {
        None => return true,
        Some(side) if side.model != model || in_flight() > 0 || keep_loaded() => return false,
        Some(_) => {}
    }
    if let Some(side) = slot.take() {
        save_kv(&side);
        retire(side);
        health::log("stop", Some(reason));
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

    #[test]
    fn a_zero_idle_timeout_does_not_unload_a_model_that_just_answered() {
        let _one = SLOT_TESTS.lock().unwrap_or_else(|e| e.into_inner());
        stop_global();
        *global_slot().lock().unwrap() = Some(idle_child_sidecar(0, unix_secs()));
        let pid = global_pid().expect("planted");
        tick_idle_unload();
        assert_eq!(
            global_pid(),
            Some(pid),
            "a cleared timeout waits the default, it does not unload now"
        );
        assert!(answers_now(), "the server is still warm");
        assert!(
            !will_start_or_switch(Path::new("/no/such/config")),
            "a warm server is not a cold load when its paths cannot be read"
        );
        stop_global();
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_to_the_loaded_model_is_not_a_different_file() {
        let dir = crate::harness::test_dir("same-model-file");
        let file = dir.join("model.gguf");
        let link = dir.join("link.gguf");
        std::fs::write(&file, b"gguf").unwrap();
        std::os::unix::fs::symlink(&file, &link).unwrap();
        assert_ne!(file, link);
        assert!(same_file(&file, &link));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[cfg(unix)]
    #[test]
    fn two_chat_turns_keep_one_server() {
        use std::os::unix::fs::PermissionsExt;
        let _one = SLOT_TESTS.lock().unwrap_or_else(|e| e.into_inner());
        stop_global();
        local_gov::reset_for_test();
        let dir = crate::harness::test_dir("sidecar-stay");
        std::fs::create_dir_all(dir.join("bin")).unwrap();
        let bin = dir.join("bin").join("llama-server");
        let seen = dir.join("args.txt");
        std::fs::write(
            &bin,
            format!("#!/bin/sh\necho \"$@\" > {}\nexit 1\n", seen.display()),
        )
        .unwrap();
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        let model = dir.join("models").join("m.gguf");
        std::fs::create_dir_all(model.parent().expect("models")).unwrap();
        std::fs::write(&model, b"gguf").unwrap();
        clear_paths_cache();
        let paths = resolve_paths(&dir).expect("the test engine and model");
        let _on = crate::route::local::ForceOn::with(None);
        let _need =
            super::super::local_scale::NeedGuard::set(super::super::local_scale::Need::Normal);
        let mut planted = idle_child_sidecar(600, unix_secs());
        planted.model = paths.model.clone();
        planted.binary = paths.binary.clone();
        *global_slot().lock().unwrap() = Some(planted);
        let pid = global_pid().expect("planted");
        assert!(
            !will_start_or_switch(&dir),
            "the loaded file is the one this turn would start"
        );
        for _ in 0..2 {
            let busy = acquire_for_request(&dir).expect("the loaded model answers");
            assert!(busy.endpoint().is_some());
            drop(busy);
            assert_eq!(global_pid(), Some(pid), "the server stayed up");
        }
        assert!(!seen.exists(), "llama-server was not started again");
        stop_global();
        clear_paths_cache();
        set_last_error(None);
        let _ = std::fs::remove_dir_all(dir);
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
            embedding_model: None,
            restarts: AtomicU64::new(0),
            last_used_secs: AtomicU64::new(last_used_secs),
            idle_unload_secs,
            launch: None,
            bare_cpu: false,
            thinking: Default::default(),
            stand_in: None,
            refused: Default::default(),
            in_flight: Arc::new(AtomicUsize::new(0)),
            health_timeout: HEALTH_TIMEOUT,
        }
    }

    #[test]
    fn a_refused_pair_holds_only_while_its_server_runs_and_against_newer_readings() {
        use super::super::local_scale::Room;
        use super::super::local_swap::{Hold, Refusals};
        let pick = PathBuf::from("/home/user/models/pick.gguf");
        let pair = Hold::Pair(autoconfig::PairRefusal {
            pool: autoconfig::Pool::Vram,
            free_mb: Some(6_000),
            state: autoconfig::state(),
            on_gpu: true,
        });
        let mut side = idle_child_sidecar(600, 0);
        side.refused = Refusals {
            models: vec![(pick.clone(), pair)],
            since_ms: 5_000,
        };
        let grown = Room {
            free_vram_mb: Some(9_000),
            free_ram_mb: 10_000,
            ..Room::default()
        };
        // The next unload drops the server and its refusals with it.
        assert_eq!(running_view(None, Some((grown, 6_000))), (false, vec![]));
        // Up, it holds the pick until a reading taken since then shows room.
        assert_eq!(
            running_view(Some(&side), Some((grown, 4_999))),
            (false, vec![pick.clone()])
        );
        assert_eq!(running_view(Some(&side), None), (false, vec![pick.clone()]));
        assert_eq!(
            running_view(Some(&side), Some((grown, 5_000))),
            (false, vec![])
        );
    }

    #[test]
    fn the_pick_refused_record_outlives_the_stand_in_and_clears_when_the_pick_loads() {
        use super::super::local_swap::{Hold, Refusals};
        let pick = PathBuf::from("/home/user/models/pick-record.gguf");
        let stand_in = PathBuf::from("/home/user/models/stand-in-record.gguf");
        let refused = Refusals {
            models: vec![(pick.clone(), Hold::UntilUnload)],
            since_ms: 0,
        };
        note_pick_refused(&pick, &stand_in, &refused);
        assert_eq!(stand_in_for_refused(&pick), Some(stand_in.clone()));
        assert_eq!(stand_in_for_refused(&stand_in), None, "only for that pick");
        // A walk that didn't refuse the pick leaves the record alone.
        note_pick_refused(&pick, &stand_in, &Refusals::default());
        assert_eq!(stand_in_for_refused(&pick), Some(stand_in.clone()));
        note_pick_refused(&pick, &pick, &Refusals::default());
        assert_eq!(stand_in_for_refused(&pick), None);
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
            swap_plan(&dir, home, Some(&side), Want::Request).0,
            Plan::Keep
        );
        assert_eq!(
            swap_plan(&dir, Path::new("other.gguf"), Some(&side), Want::Request).0,
            Plan::Load {
                model: PathBuf::from("other.gguf"),
                stand_in: None
            },
            "a new pick replaces the running model"
        );
        let held = busy();
        assert_eq!(
            swap_plan(&dir, Path::new("other.gguf"), Some(&side), Want::Request).0,
            Plan::Keep,
            "a request in flight keeps its model"
        );
        drop(held);
    }

    #[cfg(unix)]
    #[test]
    fn a_picture_loads_a_model_that_can_see_it_and_one_that_can_keeps_it() {
        use super::super::local_swap::StandIn;
        use super::super::local_vision::PictureGuard;
        use std::os::unix::fs::PermissionsExt;
        let _one = SLOT_TESTS.lock().unwrap_or_else(|e| e.into_inner());
        stop_global();
        super::super::local_scale::clear_room();
        let dir = std::env::temp_dir().join(format!("grokhub-vision-plan-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let models = dir.join("models");
        std::fs::create_dir_all(dir.join("bin")).unwrap();
        std::fs::create_dir_all(&models).unwrap();
        let binary = dir.join("bin").join("llama-server");
        std::fs::write(
            &binary,
            "#!/bin/sh\ncat <<'EOF'\n-mm,   --mmproj FILE   path to a multimodal projector file\nEOF\n",
        )
        .unwrap();
        std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755)).unwrap();
        let file = |id: &str| {
            let m = grokhub_core::local_setup::model_by_id(id).expect("catalog model");
            let p = grokhub_core::model_download::final_path(&models, m);
            std::fs::write(&p, b"gguf").unwrap();
            p
        };
        let mid = file("gemma-4-e4b-it-abliterated-q4_k");
        let small = file("gemma-4-e2b-it-abliterated-q4_k");
        let mid_eyes = file("gemma-4-e4b-it-abliterated-mmproj");
        file("gemma-4-e2b-it-abliterated-mmproj");
        let mut side = idle_child_sidecar(600, unix_secs());
        side.model = mid.clone();
        side.binary = binary;
        let plan = |side: &Sidecar| swap_plan(&dir, &mid, Some(side), Want::Request);
        assert_eq!(
            plan(&side),
            (Plan::Keep, false),
            "no picture: the pick stays"
        );
        {
            let _picture = PictureGuard::set(true);
            // The pick's projector is on disk: it reloads with it.
            assert_eq!(
                plan(&side),
                (
                    Plan::Load {
                        model: mid.clone(),
                        stand_in: None
                    },
                    true
                )
            );
            // Without it, the smaller model that can see it loads.
            std::fs::remove_file(&mid_eyes).unwrap();
            assert_eq!(
                plan(&side),
                (
                    Plan::Load {
                        model: small.clone(),
                        stand_in: Some(StandIn::Vision)
                    },
                    true
                )
            );
            // A launch that already reads pictures keeps its model.
            side.launch = Some(LaunchArgs {
                projector: Some(Box::new(autoconfig::ProjectorArgs {
                    file: models.join("mmproj.gguf"),
                    mb: 1_083,
                    off_gpu_flag: false,
                })),
                ..gpu_launch()
            });
            assert!(side.sees_pictures() && side.endpoint().sees_pictures);
            assert_eq!(plan(&side), (Plan::Keep, false));
        }
        // A background request holds the model that can't see: the picture
        // waits for it, then loads the model that can, past it.
        side.launch = None;
        std::fs::write(&mid_eyes, b"gguf").unwrap();
        let held = busy();
        assert_eq!(in_flight(), 1);
        let _picture = PictureGuard::set(true);
        assert_eq!(plan(&side), (Plan::Keep, false), "a reply in flight");
        super::super::local_vision::note_waited();
        assert_eq!(
            plan(&side),
            (
                Plan::Load {
                    model: mid.clone(),
                    stand_in: None
                },
                true
            ),
            "the turn waited: the swap goes ahead"
        );
        drop(held);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_turns_tier_reaches_the_planner_and_its_outcome_is_taken_once() {
        use super::super::local_scale::{ChatGuard, Source, SpeedQuality};
        use super::super::local_swap::{AskOutcome, StandIn};
        let _one = SLOT_TESTS.lock().unwrap_or_else(|e| e.into_inner());
        stop_global();
        super::super::local_scale::clear_room();
        let dir = std::env::temp_dir().join(format!("grokhub-tier-plan-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let models = dir.join("models");
        std::fs::create_dir_all(&models).unwrap();
        let file = |id: &str| {
            let m = grokhub_core::local_setup::model_by_id(id).expect("catalog model");
            let p = grokhub_core::model_download::final_path(&models, m);
            std::fs::write(&p, b"gguf").unwrap();
            p
        };
        let mid = file("gemma-4-e4b-it-abliterated-q4_k");
        let small = file("gemma-4-e2b-it-abliterated-q4_k");
        let mut side = idle_child_sidecar(600, unix_secs());
        side.model = mid.clone();
        take_ask_outcome();
        assert_eq!(
            swap_plan(&dir, &mid, Some(&side), Want::Request).0,
            Plan::Keep,
            "no tier: the pick stays"
        );
        assert_eq!(take_ask_outcome(), None, "no ask, no outcome");
        {
            let _tier = ChatGuard::set(Some((SpeedQuality::Speed, Source::Words)));
            assert_eq!(
                swap_plan(&dir, &mid, Some(&side), Want::Request).0,
                Plan::Load {
                    model: small.clone(),
                    stand_in: Some(StandIn::Asked(SpeedQuality::Speed, Source::Words))
                }
            );
            assert_eq!(
                take_ask_outcome(),
                Some((
                    AskOutcome::Got(SpeedQuality::Speed, Source::Words),
                    small.clone()
                ))
            );
            assert_eq!(take_ask_outcome(), None, "taken once");
            // A background helper's request doesn't hold the ask back...
            let helper = {
                let _bg = super::super::local_scale::NeedGuard::set(
                    super::super::local_scale::Need::Background,
                );
                busy()
            };
            assert_eq!((in_flight(), background_in_flight()), (1, 1));
            assert_eq!(
                swap_plan(&dir, &mid, Some(&side), Want::Request).0,
                Plan::Load {
                    model: small.clone(),
                    stand_in: Some(StandIn::Asked(SpeedQuality::Speed, Source::Words))
                }
            );
            // ...another chat's reply does.
            let reply = busy();
            assert_eq!(
                swap_plan(&dir, &mid, Some(&side), Want::Request).0,
                Plan::Keep
            );
            assert_eq!(
                take_ask_outcome().map(|(a, _)| a),
                Some(AskOutcome::Refused(
                    SpeedQuality::Speed,
                    super::super::local_swap::Refused::Busy(Source::Words)
                ))
            );
            drop((helper, reply));
            assert_eq!((in_flight(), background_in_flight()), (0, 0));
        }
        assert_eq!(
            swap_plan(&dir, &mid, Some(&side), Want::Request).0,
            Plan::Keep,
            "the guard dropped: back on Auto"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_ask_waits_for_background_work_then_stops_waiting_at_the_deadline() {
        let _one = SLOT_TESTS.lock().unwrap_or_else(|e| e.into_inner());
        stop_global();
        let helper = {
            let _bg = super::super::local_scale::NeedGuard::set(
                super::super::local_scale::Need::Background,
            );
            busy()
        };
        let began = Instant::now();
        assert!(!wait_for_idle(Duration::from_millis(200)), "still running");
        assert!(began.elapsed() >= Duration::from_millis(200));
        let done = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(150));
            drop(helper);
        });
        assert!(wait_for_idle(Duration::from_secs(10)), "it finished");
        assert_eq!(background_in_flight(), 0);
        done.join().unwrap();
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
        assert!(args.iter().any(|a| a == "--jinja"));
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
            projector: None,
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
    fn a_drafter_or_vision_projector_file_is_never_the_chat_model() {
        use grokhub_core::local_setup::{local_models, ModelKind};
        let dir = Path::new("/home/user/GrokHub/models");
        let helpers: Vec<_> = local_models()
            .iter()
            .filter(|m| matches!(m.kind, ModelKind::Drafter | ModelKind::VisionProjector))
            .collect();
        assert_eq!(
            helpers
                .iter()
                .filter(|m| m.kind == ModelKind::VisionProjector)
                .count(),
            5
        );
        for m in helpers {
            assert!(helper_gguf(&dir.join(m.file)), "{}", m.file);
        }
        assert!(!helper_gguf(&dir.join(
            "Huihui-gemma-4-E2B-it-qat-q4_0-unquantized-abliterated-Q4_K.gguf"
        )));
        assert!(helper_gguf(&dir.join("mmproj-gemma-4-E4B-it-BF16.gguf")));
    }

    #[test]
    fn only_a_start_the_projector_alone_stopped_blames_the_projector() {
        let died = "local model server stopped before it was ready";
        let slow = "local model server did not become healthy in time";
        let off = Some(Shed::Projector { oom: false });
        let mut blame = ProjectorBlame::default();
        assert!(!blame.ran(), "a clean first start");
        blame.failed(off, died);
        assert!(blame.ran(), "it ran right after the projector came off");
        // Anything else that changed after it clears the blame: the drafter
        // came off, or the CPU retry (nothing shed) ran.
        blame.failed(Some(Shed::Draft), died);
        assert!(!blame.ran());
        blame.failed(off, died);
        blame.failed(None, died);
        assert!(!blame.ran(), "the start that ran was the CPU retry");
        // Out of memory or a slow load isn't the engine refusing it.
        blame.failed(Some(Shed::Projector { oom: true }), died);
        assert!(!blame.ran());
        blame.failed(off, slow);
        assert!(!blame.ran());
    }

    #[test]
    fn a_picture_start_steps_down_only_to_a_smaller_model_that_can_see_it() {
        let m = |id: &str| grokhub_core::local_setup::model_by_id(id).unwrap();
        let file = |id: &str| PathBuf::from("/home/user/models").join(m(id).file);
        let (g26, g12b, mistral, e2b) = (
            "gemma-4-26b-a4b-it-abliterated-q4_k",
            "gemma-4-12b-it-abliterated-q4_k",
            "mistral-small3.2-24b-heretic-q4_k_m",
            "gemma-4-e2b-it-abliterated-q4_k",
        );
        let disk: Vec<_> = [g26, mistral, g12b, e2b]
            .iter()
            .map(|id| (m(id), file(id)))
            .collect();
        let seeing = [g26, g12b, e2b];
        assert_eq!(
            next_seeing(&file(g26), &disk, &seeing, false),
            Some(file(g12b)),
            "past the model that can't see"
        );
        assert_eq!(
            next_seeing(&file(mistral), &disk, &seeing, false),
            Some(file(g12b))
        );
        assert_eq!(next_seeing(&file(e2b), &disk, &seeing, false), None);
        // Keep loaded or a forced file: the pick only.
        assert_eq!(next_seeing(&file(g26), &disk, &seeing, true), None);
        assert_eq!(
            next_seeing(
                Path::new("/home/user/models/own.gguf"),
                &disk,
                &seeing,
                false
            ),
            None
        );
    }

    #[test]
    fn a_picture_waits_for_the_requests_in_flight_until_they_finish_or_the_deadline() {
        let _one = SLOT_TESTS.lock().unwrap_or_else(|e| e.into_inner());
        stop_global();
        assert_eq!(in_flight(), 0);
        assert_eq!(
            wait_until_idle(Duration::from_secs(5), &|| false),
            Drain::Free
        );
        // A background request that ends after 200 ms: the wait ends with it.
        let held = busy();
        let done = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(200));
            drop(held);
        });
        let began = Instant::now();
        assert_eq!(
            wait_until_idle(Duration::from_secs(5), &|| false),
            Drain::Free
        );
        assert!(began.elapsed() < Duration::from_secs(5));
        assert!(began.elapsed() >= Duration::from_millis(150));
        done.join().unwrap();
        // One that never ends: the deadline, then the swap goes ahead.
        let held = busy();
        let began = Instant::now();
        assert_eq!(
            wait_until_idle(Duration::from_millis(300), &|| false),
            Drain::TimedOut
        );
        assert!(began.elapsed() >= Duration::from_millis(300));
        // Stop ends the wait at once.
        assert_eq!(
            wait_until_idle(Duration::from_secs(5), &|| true),
            Drain::Cancelled
        );
        drop(held);
        // Nothing loaded: nothing to wait for.
        let held = busy();
        assert_eq!(
            wait_for_vision(&std::env::temp_dir(), Duration::from_secs(5), &|| false),
            Drain::Free
        );
        drop(held);
        assert_eq!(VISION_DRAIN, Duration::from_secs(20));
    }

    /// A GGUF header the launch planner reads: 16 layers, 8k context.
    #[cfg(unix)]
    fn tiny_gguf() -> Vec<u8> {
        let mut kv = Vec::new();
        let key = |kv: &mut Vec<u8>, k: &str, t: u32| {
            kv.extend((k.len() as u64).to_le_bytes());
            kv.extend(k.as_bytes());
            kv.extend(t.to_le_bytes());
        };
        key(&mut kv, "general.architecture", 8);
        kv.extend(5u64.to_le_bytes());
        kv.extend(b"llama");
        for (k, v) in [
            ("llama.block_count", 16u32),
            ("llama.context_length", 8_192),
            ("llama.embedding_length", 2_048),
            ("llama.attention.head_count", 32),
            ("llama.attention.head_count_kv", 8),
        ] {
            key(&mut kv, k, 4);
            kv.extend(v.to_le_bytes());
        }
        let mut out = b"GGUF".to_vec();
        out.extend(3u32.to_le_bytes());
        out.extend(0u64.to_le_bytes());
        out.extend(6u64.to_le_bytes());
        out.extend(kv);
        out
    }

    /// A machine with no GPU and `free_ram_mb` of RAM free.
    #[cfg(unix)]
    fn cpu_box(free_ram_mb: u64) -> grokhub_core::local_profile::LocalProfile {
        use grokhub_core::local_picker::CpuSimd;
        use grokhub_core::local_profile::{CpuInfo, LocalProfile, PowerInfo, Thermal};
        LocalProfile {
            os: "linux",
            arch: "x86_64",
            gpus: Vec::new(),
            cpu: CpuInfo::new(16, 8, 0, CpuSimd::Avx2),
            ram_mb: 32_768,
            free_ram_mb: Some(free_ram_mb),
            power: PowerInfo::default(),
            thermal: Thermal::Nominal,
            free_disk_mb: 500_000,
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_picture_keeps_the_running_model_when_the_launch_planner_refuses_every_vision_launch() {
        use super::super::local_vision::PictureGuard;
        use std::os::unix::fs::PermissionsExt;
        let _one = SLOT_TESTS.lock().unwrap_or_else(|e| e.into_inner());
        stop_global();
        super::super::local_scale::clear_room();
        let dir = std::env::temp_dir().join(format!("grokhub-vision-fit-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let models = dir.join("models");
        std::fs::create_dir_all(dir.join("bin")).unwrap();
        std::fs::create_dir_all(&models).unwrap();
        let binary = dir.join("bin").join("llama-server");
        std::fs::write(
            &binary,
            "#!/bin/sh\ncat <<'EOF'\n-mm,   --mmproj FILE   path to a multimodal projector file\n--mmproj-offload, --no-mmproj-offload   offload\nEOF\n",
        )
        .unwrap();
        std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755)).unwrap();
        let place = |id: &str, body: Option<&[u8]>| {
            let m = grokhub_core::local_setup::model_by_id(id).expect("catalog model");
            let p = grokhub_core::model_download::final_path(&models, m);
            match body {
                Some(b) => std::fs::write(&p, b).unwrap(),
                // A 200 MB projector, sparse.
                None => std::fs::File::create(&p)
                    .unwrap()
                    .set_len(200 * 1024 * 1024)
                    .unwrap(),
            }
            p
        };
        let mid = place("gemma-4-e4b-it-abliterated-q4_k", Some(&tiny_gguf()));
        let small = place("gemma-4-e2b-it-abliterated-q4_k", Some(&tiny_gguf()));
        place("gemma-4-e4b-it-abliterated-mmproj", None);
        place("gemma-4-e2b-it-abliterated-mmproj", None);
        let mut side = idle_child_sidecar(600, unix_secs());
        side.model = mid.clone();
        side.binary = binary;
        side.token = "vision-fit-server".into();
        // 300 MB of RAM free plans for 240: no model fits with its 230 MB
        // projector, so the running model stays and both launches are held.
        autoconfig::set_profile(cpu_box(300), dir.join("local"));
        autoconfig::set_free_probe(Arc::new(|| autoconfig::FreeNow {
            vram_free_mb: None,
            ram_free_mb: Some(300),
        }));
        let _picture = PictureGuard::set(true);
        let plan = |side: &Sidecar| swap_plan(&dir, &mid, Some(side), Want::Request);
        assert_eq!(plan(&side), (Plan::Keep, false));
        assert_eq!(
            vision_held(&side.token, None),
            vec![mid.clone(), small.clone()]
        );
        // A newer server holds nothing, and with room the pick reloads with
        // its projector.
        side.token = "vision-fit-server-2".into();
        autoconfig::set_free_probe(Arc::new(|| autoconfig::FreeNow {
            vram_free_mb: None,
            ram_free_mb: Some(24_000),
        }));
        autoconfig::set_profile(cpu_box(24_000), dir.join("local"));
        assert_eq!(
            plan(&side),
            (
                Plan::Load {
                    model: mid.clone(),
                    stand_in: None
                },
                true
            )
        );
        *VISION_HOLDS.lock().unwrap() = None;
        autoconfig::reset_free_probe();
        autoconfig::forget_profile();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn a_picture_waits_only_while_a_model_that_can_see_it_could_load() {
        use super::super::local_swap::Hold;
        use std::os::unix::fs::PermissionsExt;
        let _one = SLOT_TESTS.lock().unwrap_or_else(|e| e.into_inner());
        stop_global();
        super::super::local_scale::clear_room();
        let dir = std::env::temp_dir().join(format!("grokhub-vision-wait-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let models = dir.join("models");
        std::fs::create_dir_all(dir.join("bin")).unwrap();
        std::fs::create_dir_all(&models).unwrap();
        let binary = dir.join("bin").join("llama-server");
        std::fs::write(
            &binary,
            "#!/bin/sh\ncat <<'EOF'\n-mm,   --mmproj FILE   path to a multimodal projector file\nEOF\n",
        )
        .unwrap();
        std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755)).unwrap();
        let place = |id: &str| {
            let m = grokhub_core::local_setup::model_by_id(id).expect("catalog model");
            let p = grokhub_core::model_download::final_path(&models, m);
            std::fs::write(&p, b"gguf").unwrap();
            p
        };
        let mid = place("gemma-4-e4b-it-abliterated-q4_k");
        place("gemma-4-e4b-it-abliterated-mmproj");
        let mut side = idle_child_sidecar(600, unix_secs());
        side.model = mid.clone();
        side.binary = binary;
        side.token = "vision-wait-server".into();
        *global_slot().lock().unwrap() = Some(side);
        let held = busy();
        // The E4B can load its projector: the picture waits for the request.
        let began = Instant::now();
        assert_eq!(
            wait_for_vision(&dir, Duration::from_millis(300), &|| false),
            Drain::TimedOut
        );
        assert!(began.elapsed() >= Duration::from_millis(300));
        // Once its vision launch is held there's nothing to wait for.
        hold_vision("vision-wait-server", mid.clone(), Hold::UntilUnload);
        let began = Instant::now();
        assert_eq!(
            wait_for_vision(&dir, Duration::from_secs(5), &|| false),
            Drain::Free
        );
        assert!(began.elapsed() < Duration::from_secs(1));
        drop(held);
        stop_global();
        *VISION_HOLDS.lock().unwrap() = None;
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_held_vision_launch_releases_with_more_room_and_goes_with_its_server() {
        use super::super::local_autoconfig::{PairRefusal, Pool};
        use super::super::local_scale::{Room, HEADROOM_MB};
        use super::super::local_swap::Hold;
        let _one = SLOT_TESTS.lock().unwrap_or_else(|e| e.into_inner());
        let (g12b, e2b) = (
            PathBuf::from("/home/user/models/g12b.gguf"),
            PathBuf::from("/home/user/models/e2b.gguf"),
        );
        let refusal = PairRefusal {
            pool: Pool::Vram,
            free_mb: Some(1_000),
            state: autoconfig::state(),
            on_gpu: true,
        };
        hold_vision("server-a", g12b.clone(), Hold::Pair(refusal));
        hold_vision("server-a", e2b.clone(), Hold::UntilUnload);
        // No newer reading: both hold.
        assert_eq!(
            vision_held("server-a", None),
            vec![g12b.clone(), e2b.clone()]
        );
        let room = |vram: u64| {
            Some((
                Room {
                    free_vram_mb: Some(vram),
                    free_ram_mb: 0,
                    cpu_busy_pct: Some(10),
                    on_battery: false,
                    unified: false,
                },
                unix_ms() + 1_000,
            ))
        };
        let own = autoconfig::own_use().vram_free_mb.unwrap_or(0);
        assert_eq!(
            vision_held("server-a", room(1_000 + HEADROOM_MB - 1 - own)),
            vec![g12b.clone(), e2b.clone()]
        );
        // The pool the vision launch was sized from has room again.
        assert_eq!(
            vision_held("server-a", room(1_000 + HEADROOM_MB - own)),
            vec![e2b.clone()]
        );
        // Another server holds nothing, and its first hold replaces these.
        assert_eq!(vision_held("server-b", None), Vec::<PathBuf>::new());
        hold_vision("server-b", e2b.clone(), Hold::UntilUnload);
        assert_eq!(vision_held("server-b", None), vec![e2b.clone()]);
        assert_eq!(vision_held("server-a", None), Vec::<PathBuf>::new());
        *VISION_HOLDS.lock().unwrap() = None;
    }

    #[test]
    fn a_failed_start_sheds_its_projector_before_its_drafter() {
        let both = LaunchArgs {
            draft: Some(Box::new(super::super::local_spec::DraftArgs {
                kind: ld::DraftKind::Mtp,
                model: Some(PathBuf::from("/home/user/models/drafters/heads.gguf")),
                on_gpu: true,
                flags: Default::default(),
            })),
            projector: Some(Box::new(autoconfig::ProjectorArgs {
                file: PathBuf::from("/home/user/models/mmproj.gguf"),
                mb: 1_083,
                off_gpu_flag: true,
            })),
            ..gpu_launch()
        };
        let mut l = both.clone();
        assert_eq!(shed(&mut l, false), Some(Shed::Projector { oom: false }));
        assert!(l.projector.is_none() && l.draft.is_some());
        assert_eq!(shed(&mut l, false), Some(Shed::Draft));
        assert_eq!(l, gpu_launch());
        assert_eq!(shed(&mut l, false), None);
        // Out of memory sheds the projector too, then leaves the pair to
        // its own back-off.
        let mut l = both;
        assert_eq!(shed(&mut l, true), Some(Shed::Projector { oom: true }));
        assert_eq!(shed(&mut l, true), None);
        assert!(l.draft.is_some());
    }

    #[test]
    fn a_pair_that_does_not_fit_steps_down_until_one_does() {
        use super::super::local_swap::StandIn;
        let m = |id: &str| grokhub_core::local_setup::model_by_id(id).unwrap();
        let file = |id: &str| PathBuf::from("/home/user/models").join(m(id).file);
        let (g12b, e4b, e2b) = (
            "gemma-4-12b-it-abliterated-q4_k",
            "gemma-4-e4b-it-abliterated-q4_k",
            "gemma-4-e2b-it-abliterated-q4_k",
        );
        let disk: Vec<_> = [g12b, e4b, e2b]
            .iter()
            .map(|id| (m(id), file(id)))
            .collect();
        let no_room = autoconfig::PAIR_NO_ROOM;
        // Each pair refused in turn walks down to the smallest model, then stops.
        assert_eq!(
            next_step(no_room, &file(g12b), &disk, false),
            Some((file(e4b), StandIn::Tight))
        );
        assert_eq!(
            next_step(no_room, &file(e4b), &disk, false),
            Some((file(e2b), StandIn::Tight))
        );
        assert_eq!(
            next_step(no_room, &file(e2b), &disk, false),
            None,
            "nothing smaller on disk"
        );
        // Out of memory steps once; a second one ends the walk.
        let drafted = LaunchArgs {
            draft: Some(Box::new(super::super::local_spec::DraftArgs {
                kind: ld::DraftKind::Mtp,
                model: Some(PathBuf::from("/home/user/models/drafters/heads.gguf")),
                on_gpu: true,
                flags: Default::default(),
            })),
            ..gpu_launch()
        };
        let oom = oom_error(&drafted, "");
        assert_eq!(
            next_step(&oom, &file(g12b), &disk, false),
            Some((file(e4b), StandIn::OutOfMemory))
        );
        assert_eq!(next_step(&oom, &file(e4b), &disk, true), None);
        // A pair refused after that still stands in as out of memory, so the
        // planner holds it until the next unload.
        assert_eq!(
            next_step(no_room, &file(e4b), &disk, true),
            Some((file(e2b), StandIn::OutOfMemory))
        );
        assert_eq!(
            next_step(
                "local model server stopped before it was ready",
                &file(g12b),
                &disk,
                false
            ),
            None
        );
        assert_eq!(
            pair_no_room_error(&file(g12b), no_room),
            "There isn't room for Gemma 4 12B and its draft model together, and no smaller model on disk fits with its draft. Close some apps, or download a smaller model in Settings → Local model."
        );
        assert_eq!(
            pair_no_room_error(
                &file(g12b),
                &format!("{no_room}, because {}", autoconfig::PAIR_OVERRIDE)
            ),
            "There isn't room for Gemma 4 12B and its draft model together, because the GPU layers setting in Settings → Local model keeps part of the model off the GPU, and no smaller model on disk fits with its draft. Clear the GPU layers setting, or download a smaller model."
        );
    }

    #[test]
    fn a_stand_in_is_labelled_smaller_only_when_it_is_smaller_than_the_pick() {
        use super::super::local_swap::StandIn;
        let m = |id: &str| grokhub_core::local_setup::model_by_id(id).unwrap();
        let file = |id: &str| PathBuf::from("/home/user/models").join(m(id).file);
        let (moe, g12b, e4b) = (
            "gemma-4-26b-a4b-it-abliterated-q4_k",
            "gemma-4-12b-it-abliterated-q4_k",
            "gemma-4-e4b-it-abliterated-q4_k",
        );
        let perf = Some(StandIn::Performance);
        let tight = Some(StandIn::Tight);
        assert_eq!(stand_in_label(&file(e4b), &file(g12b), None, tight), tight);
        // A refused step up that lands on the pick runs the pick.
        assert_eq!(stand_in_label(&file(g12b), &file(g12b), perf, tight), None);
        assert_eq!(
            stand_in_label(&file(g12b), &file(g12b), perf, Some(StandIn::OutOfMemory)),
            None
        );
        // One still bigger than the pick is still the step up.
        assert_eq!(stand_in_label(&file(moe), &file(e4b), perf, tight), perf);
        assert_eq!(stand_in_label(&file(e4b), &file(g12b), perf, None), perf);
    }

    #[test]
    fn the_out_of_memory_error_says_whether_the_drafter_was_loaded() {
        let drafted = LaunchArgs {
            draft: Some(Box::new(super::super::local_spec::DraftArgs {
                kind: ld::DraftKind::Mtp,
                model: Some(PathBuf::from("/home/user/models/drafters/heads.gguf")),
                on_gpu: true,
                flags: Default::default(),
            })),
            ..gpu_launch()
        };
        let try_e4b = " Try Gemma 4 E4B in Settings → Local model.";
        assert_eq!(
            oom_error(&drafted, try_e4b),
            "the local model ran out of memory with its draft model. Try Gemma 4 E4B in Settings → Local model."
        );
        assert_eq!(
            oom_error(&gpu_launch(), ""),
            "the local model ran out of memory even with fewer GPU layers."
        );
    }

    #[test]
    fn a_start_waits_for_the_drafter_file_too() {
        let dir = std::env::temp_dir().join(format!("gh-load-bytes-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let model = dir.join("model.gguf");
        let heads = dir.join("heads.gguf");
        std::fs::write(&model, [0u8; 3]).unwrap();
        std::fs::write(&heads, [0u8; 5]).unwrap();
        let with = |model: Option<PathBuf>| LaunchArgs {
            draft: Some(Box::new(super::super::local_spec::DraftArgs {
                kind: ld::DraftKind::Mtp,
                model,
                on_gpu: true,
                flags: Default::default(),
            })),
            ..gpu_launch()
        };
        assert_eq!(load_bytes(&model, None), 3);
        assert_eq!(load_bytes(&model, Some(&gpu_launch())), 3);
        assert_eq!(load_bytes(&model, Some(&with(Some(heads.clone())))), 8);
        assert_eq!(
            load_bytes(&model, Some(&with(None))),
            3,
            "heads inside the model"
        );
        let _ = std::fs::remove_dir_all(dir);
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
            projector: None,
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
            true,
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
            true,
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
        // Same timestamp tick: the folder mtime does not move, so the earlier
        // "nothing here" answer would stand. Both files are on disk now.
        clear_paths_cache();
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
        // The added files live in a Settings file it reads, so it counts too.
        let before = paths_stamp(&dir);
        std::fs::write(dir.join("local_ai.json"), r#"{"ownFiles":["/x.gguf"]}"#).unwrap();
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
    fn an_added_model_file_loads_when_it_is_the_only_one_or_scores_best() {
        let dir = std::env::temp_dir().join(format!("gh-sidecar-own-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let models = dir.join("models");
        std::fs::create_dir_all(&models).unwrap();
        let own = dir.join("mine.gguf");
        std::fs::write(&own, b"gguf").unwrap();
        let added = |files: &[&std::path::Path]| {
            let list: Vec<String> = files.iter().map(|p| p.display().to_string()).collect();
            std::fs::write(
                dir.join("local_ai.json"),
                serde_json::json!({ "ownFiles": list }).to_string(),
            )
            .unwrap();
            resolve_model_file(&dir)
        };
        // A file that's gone or a projector is skipped; the next one loads.
        let gone = dir.join("gone.gguf");
        let projector = dir.join("mmproj-x.gguf");
        std::fs::write(&projector, b"gguf").unwrap();
        assert_eq!(added(&[&gone, &projector, &own]), Some(own.clone()));
        // An old Settings pick is no longer read.
        std::fs::write(
            dir.join("local_ai.json"),
            serde_json::json!({ "localModel": { "selected": own.display().to_string() } })
                .to_string(),
        )
        .unwrap();
        assert_ne!(resolve_model_file(&dir), Some(own.clone()));
        // A tiny added file can beat a slow catalog download on merit.
        let slow = grokhub_core::local_setup::model_by_id("qwen3.8-27b-heretic-q4_k_m")
            .expect("slow catalog model");
        let fast = grokhub_core::local_setup::model_by_id("gemma-4-e4b-it-abliterated-q4_k")
            .expect("fast capable catalog model");
        let slow_path = models.join(slow.file);
        std::fs::write(&slow_path, vec![0u8; 64]).unwrap();
        std::fs::write(
            models.join("installed.json"),
            serde_json::json!({
                "id": slow.id,
                "file": slow.file,
                "sha256": "abc",
                "bytes": 64
            })
            .to_string(),
        )
        .unwrap();
        let own_fast = dir.join(fast.file);
        std::fs::write(&own_fast, vec![0u8; 32]).unwrap();
        assert_eq!(
            added(&[&own_fast]),
            Some(own_fast.clone()),
            "the faster added catalog file beats the slow download"
        );
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

    /// Whether `pid` is a live process: there, and not a zombie waiting to
    /// be reaped.
    #[cfg(target_os = "linux")]
    fn running(pid: u32) -> bool {
        std::fs::read_to_string(format!("/proc/{pid}/stat")).is_ok_and(|st| !st.contains(") Z "))
    }

    /// The same answer where there is no `/proc`: `ps` prints the state
    /// letters of a process that is there, and fails for one that is not.
    #[cfg(all(unix, not(target_os = "linux")))]
    fn running(pid: u32) -> bool {
        std::process::Command::new("ps")
            .args(["-o", "stat=", "-p", &pid.to_string()])
            .output()
            .is_ok_and(|out| {
                out.status.success()
                    && !String::from_utf8_lossy(&out.stdout).trim().starts_with('Z')
            })
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
            Err("local model server stopped before it was ready: it exited with code 1 (it didn't start on the CPU either)".into())
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

    #[cfg(unix)]
    #[test]
    fn a_death_names_its_reason_from_the_log_and_the_exit() {
        use std::os::unix::process::ExitStatusExt;
        let code = |c: i32| Some(std::process::ExitStatus::from_raw(c << 8));
        let signal = |s: i32| Some(std::process::ExitStatus::from_raw(s));
        assert_eq!(
            death_reason("llama_model_load: error loading model: unknown model architecture: 'gemma9'\n", code(1)).as_deref(),
            Some("this engine is too old for the model (llama_model_load: error loading model: unknown model architecture: 'gemma9'). Get engine in Settings → Local AI updates it")
        );
        assert_eq!(
            death_reason("main: couldn't bind HTTP server socket, hostname: 127.0.0.1, port: 8080\n", code(1)).as_deref(),
            Some("its port was taken (main: couldn't bind HTTP server socket, hostname: 127.0.0.1, port: 8080)")
        );
        assert_eq!(
            death_reason("error: invalid argument: --models-max\n", code(1)).as_deref(),
            Some("it refused how it was started (error: invalid argument: --models-max)")
        );
        // A backend names itself on a good start too: only its error counts.
        let rocm = "ggml_cuda_init: found 1 ROCm devices:\n  Device 0: AMD Radeon RX 9070 XT, gfx1201\nrocBLAS error: Cannot read TensileLibrary.dat for GPU arch gfx1201\n";
        assert_eq!(
            death_reason(rocm, signal(6)).as_deref(),
            Some("the ROCm GPU backend failed (rocBLAS error: Cannot read TensileLibrary.dat for GPU arch gfx1201)")
        );
        assert_eq!(
            death_reason(
                "ggml_vulkan: Found 1 Vulkan devices:\nload_tensors: loading model\n",
                signal(11)
            )
            .as_deref(),
            Some("it crashed (segmentation fault) (last log line: load_tensors: loading model)")
        );
        assert_eq!(
            death_reason("gguf_init_from_file: invalid magic characters 'xxxx'\n", code(1)).as_deref(),
            Some("it couldn't load the model file (gguf_init_from_file: invalid magic characters 'xxxx')")
        );
        assert_eq!(
            death_reason(
                "ggml_vulkan: Device memory allocation of size 1 failed: ErrorOutOfDeviceMemory\n",
                code(1)
            )
            .as_deref(),
            Some("it ran out of memory loading the model")
        );
        assert_eq!(
            death_reason("", signal(4)).as_deref(),
            Some("it crashed: this processor lacks an instruction the engine was built for (illegal instruction)")
        );
        assert_eq!(
            death_reason("", code(3)).as_deref(),
            Some("it exited with code 3")
        );
        assert_eq!(death_reason("", None), None);
        let long = format!("{}\n", "x".repeat(200));
        assert_eq!(
            death_reason(&long, None),
            Some(format!("it exited (last log line: {}…)", "x".repeat(160)))
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_start_router_mode_stops_runs_the_model_alone_then_says_why() {
        use std::os::unix::fs::PermissionsExt;
        let _one = SLOT_TESTS.lock().unwrap_or_else(|e| e.into_inner());
        stop_global();
        local_gov::reset_for_test();
        let dir = crate::harness::test_dir("sidecar-router-off");
        std::fs::create_dir_all(dir.join("bin")).unwrap();
        std::fs::create_dir_all(dir.join("models")).unwrap();
        let bin = dir.join("bin").join("llama-server");
        let seen = dir.join("args.txt");
        std::fs::write(
            &bin,
            format!(
                "#!/bin/sh\nif [ \"$1\" = --help ]; then echo '--models-preset PATH'; exit 0; fi\necho \"$@\" >> {}\necho 'error: invalid argument: --models-max' >&2\nexit 1\n",
                seen.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::write(dir.join("models").join("m.gguf"), b"x").unwrap();
        std::fs::write(dir.join("models").join("nomic-embed-text.gguf"), b"x").unwrap();
        assert_eq!(
            acquire(&dir).err().as_deref(),
            Some("local model server stopped before it was ready: it refused how it was started (error: invalid argument: --models-max) (it didn't start on the CPU either)")
        );
        let starts = std::fs::read_to_string(&seen).unwrap();
        let starts: Vec<&str> = starts.lines().collect();
        assert_eq!(starts.len(), 3, "{starts:?}");
        assert!(starts[0].contains("--models-preset"), "{}", starts[0]);
        assert!(!starts[1].contains("--models-preset"), "{}", starts[1]);
        assert!(starts[2].contains("--device none"), "{}", starts[2]);
        local_gov::reset_for_test();
        set_last_error(None);
    }
}
