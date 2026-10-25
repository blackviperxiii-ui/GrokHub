//! Tiered auto-config for `llama-server`: from the hardware profile, the
//! model's GGUF header, the governor state and the user's overrides, size the
//! GPU offload (partial `-ngl`, MoE expert offload, multi-GPU split), the
//! context and KV cache, and the thread count. Pure apart from the stored
//! profile and the `autotune.json` record of launches that ran out of memory.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use grokhub_core::gguf::{self, GgufMeta};
use grokhub_core::local_profile::{self, Backend, LocalProfile};
use grokhub_core::local_setup::{self, LocalModel};
use serde::{Deserialize, Serialize};

use super::local_spec::{self, DraftArgs};

/// What the governor allows right now. `Unload` never launches, so it isn't here.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum GovState {
    RampUp,
    #[default]
    Normal,
    Throttle,
}

impl GovState {
    /// Share of free memory a launch may plan for, in percent.
    fn budget_pct(self) -> u64 {
        match self {
            GovState::RampUp => 90,
            GovState::Normal => 80,
            GovState::Throttle => 50,
        }
    }

    fn ctx_cap(self) -> u32 {
        match self {
            GovState::RampUp => u32::MAX,
            GovState::Normal => 32_768,
            GovState::Throttle => 8_192,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KvType {
    Q8,
    Q4,
}

impl KvType {
    /// Bytes per 1,024 cached values (`q8_0` is 34 bytes per 32, `q4_0` 18).
    fn bytes_per_1k(self) -> u64 {
        match self {
            KvType::Q8 => 1_088,
            KvType::Q4 => 576,
        }
    }

    fn flag(self) -> &'static str {
        match self {
            KvType::Q8 => "q8_0",
            KvType::Q4 => "q4_0",
        }
    }
}

/// Manual values from Settings. Each is clamped to what fits.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Overrides {
    pub ngl: Option<u32>,
    pub ctx: Option<u32>,
    pub threads: Option<u32>,
    pub kv: Option<KvType>,
}

/// The `llama-server` flags one launch uses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchArgs {
    pub backend: Backend,
    /// Layers on the GPU; `block_count + 1` is every layer plus the output.
    pub ngl: u32,
    pub all_layers: u32,
    /// MoE: layers whose experts stay in RAM (`--n-cpu-moe`).
    pub n_cpu_moe: Option<u32>,
    pub ctx: u32,
    pub kv: KvType,
    pub threads: u32,
    pub threads_batch: Option<u32>,
    pub flash_attn: bool,
    pub mlock: bool,
    pub tensor_split: Option<Vec<u64>>,
    /// Speculative decoding, when [`local_spec::plan`] turned it on.
    pub draft: Option<Box<DraftArgs>>,
}

impl LaunchArgs {
    pub fn to_args(&self) -> Vec<String> {
        let mut a = vec![
            "-ngl".to_string(),
            self.ngl.to_string(),
            "-c".into(),
            self.ctx.to_string(),
            "--threads".into(),
            self.threads.to_string(),
            "--cache-type-k".into(),
            self.kv.flag().into(),
            "--cache-type-v".into(),
            self.kv.flag().into(),
            "--flash-attn".into(),
            if self.flash_attn { "on" } else { "off" }.into(),
        ];
        if let Some(n) = self.n_cpu_moe {
            a.extend(["--n-cpu-moe".into(), n.to_string()]);
        }
        if let Some(t) = self.threads_batch {
            a.extend(["--threads-batch".into(), t.to_string()]);
        }
        if self.mlock {
            // The pinned engine folds `--mlock` into the load mode.
            a.extend(["--load-mode".into(), "mmap+mlock".into()]);
        }
        if let Some(split) = self.tensor_split.as_ref().filter(|_| self.ngl > 0) {
            let s: Vec<String> = split.iter().map(u64::to_string).collect();
            a.extend(["--split-mode".into(), "layer".into()]);
            a.extend(["--tensor-split".into(), s.join(",")]);
        }
        if let Some(d) = &self.draft {
            a.extend(d.to_args());
        }
        a.push("--cache-prompt".into());
        a
    }
}

/// Context sizes tried, biggest first.
const CTX_STEPS: [u32; 6] = [131_072, 65_536, 32_768, 16_384, 8_192, 4_096];
/// The smallest context worth keeping q8 KV for; below it q4 buys room.
const Q8_FLOOR_CTX: u32 = 8_192;
/// Share of MoE weights that are experts (and may stay in RAM).
const EXPERT_SHARE_PCT: u64 = 80;
/// RAM left to the OS when nothing better is known.
const RAM_RESERVE_MB: u64 = 4_096;

/// Compute buffers grow with context.
pub fn overhead_mb(ctx: u32) -> u64 {
    512 + u64::from(ctx) / 128
}

pub fn kv_mb(meta: &GgufMeta, ctx: u32, kv: KvType) -> u64 {
    let values = u64::from(meta.block_count) * meta.kv_width() * u64::from(ctx);
    values / 1_024 * kv.bytes_per_1k() / (1_024 * 1_024)
}

/// (GPU, RAM) memory a launch may plan for in `state`.
pub fn budgets(p: &LocalProfile, backend: Backend, state: GovState) -> (u64, u64) {
    let pct = state.budget_pct();
    let plan = p.device_plan();
    let ram_free = p
        .free_ram_mb
        .unwrap_or_else(|| p.ram_mb.saturating_sub(RAM_RESERVE_MB));
    if backend == Backend::Cpu || plan.gpus.is_empty() {
        return (0, ram_free * pct / 100);
    }
    let gpu_free: u64 = plan
        .gpus
        .iter()
        .map(|g| g.free_vram_mb.unwrap_or(g.vram_mb))
        .sum();
    // Unified memory is one pool: what the GPU takes, RAM doesn't have.
    let ram = if plan.unified() {
        0
    } else {
        ram_free * pct / 100
    };
    (gpu_free * pct / 100, ram)
}

fn ctx_steps(meta: &GgufMeta, state: GovState, want: Option<u32>) -> impl Iterator<Item = u32> {
    let model_max = if meta.context_length > 0 {
        meta.context_length
    } else {
        4_096
    };
    let cap = state.ctx_cap().min(model_max).min(want.unwrap_or(u32::MAX));
    CTX_STEPS.into_iter().filter(move |c| *c <= cap.max(4_096))
}

fn fits(meta: &GgufMeta, ctx: u32, kv: KvType, total_mb: u64) -> bool {
    meta.file_mb + kv_mb(meta, ctx, kv) + overhead_mb(ctx) <= total_mb
}

/// Context past this splits KV across GPU and RAM badly; partial offload stops here.
const PARTIAL_CTX_CAP: u32 = 16_384;
/// CPU-only launches keep prompt processing short.
const CPU_CTX_CAP: u32 = 8_192;

/// The largest context (and KV type) whose plan fits `limit` MB: q8 KV unless
/// only q4 reaches the floor context.
fn pick_ctx(
    meta: &GgufMeta,
    steps: &[u32],
    limit: u64,
    floor: u32,
    kv: Option<KvType>,
) -> Option<(u32, KvType)> {
    let largest = |kv: KvType| steps.iter().copied().find(|c| fits(meta, *c, kv, limit));
    if let Some(kv) = kv {
        return largest(kv).map(|c| (c, kv));
    }
    match (largest(KvType::Q8), largest(KvType::Q4)) {
        (Some(c), _) if c >= floor => Some((c, KvType::Q8)),
        (q8, Some(c4)) if Some(c4) > q8 => Some((c4, KvType::Q4)),
        (Some(c), _) => Some((c, KvType::Q8)),
        _ => None,
    }
}

/// The launch for this machine, model and state. The whole model on the GPU
/// wins over a longer context; past that, the longest context that fits.
pub fn autoconfig(
    p: &LocalProfile,
    meta: &GgufMeta,
    backend: Backend,
    state: GovState,
    o: &Overrides,
) -> LaunchArgs {
    let (gpu_mb, ram_mb) = budgets(p, backend, state);
    let steps: Vec<u32> = ctx_steps(meta, state, o.ctx).collect();
    let floor = Q8_FLOOR_CTX
        .min(meta.context_length.max(4_096))
        .min(o.ctx.unwrap_or(u32::MAX).max(4_096));
    let full = (gpu_mb > 0)
        .then(|| pick_ctx(meta, &steps, gpu_mb, floor, o.kv))
        .flatten()
        .filter(|(c, _)| *c >= floor);
    let cap = if gpu_mb > 0 {
        PARTIAL_CTX_CAP
    } else {
        CPU_CTX_CAP
    };
    let capped: Vec<u32> = steps.iter().copied().filter(|c| *c <= cap).collect();
    let (ctx, kv) = full
        .or_else(|| pick_ctx(meta, &capped, gpu_mb + ram_mb, floor, o.kv))
        .unwrap_or((4_096, o.kv.unwrap_or(KvType::Q4)));
    let all_layers = meta.block_count + 1;
    let (ngl, n_cpu_moe) = offload(meta, gpu_mb, ctx, kv);
    let ngl = o.ngl.map_or(ngl, |n| n.min(all_layers));
    let p_cores = p.cpu.p_cores.max(1);
    let threads = match state {
        GovState::RampUp => p_cores,
        GovState::Normal => (p_cores / 2).max(1),
        GovState::Throttle => (p_cores / 4).max(1),
    };
    let threads = o
        .threads
        .map_or(threads, |t| t.clamp(1, p.cpu.logical.max(1)));
    LaunchArgs {
        backend,
        ngl,
        all_layers,
        n_cpu_moe,
        ctx,
        kv,
        threads,
        threads_batch: (state == GovState::RampUp).then_some(p.cpu.logical.max(1)),
        flash_attn: backend != Backend::Cpu,
        mlock: p.free_ram_mb.is_some_and(|free| free > 2 * meta.file_mb),
        tensor_split: p.device_plan().tensor_split,
        draft: None,
    }
}

/// `-ngl` and `--n-cpu-moe` for the GPU memory left after KV and buffers.
fn offload(meta: &GgufMeta, gpu_mb: u64, ctx: u32, kv: KvType) -> (u32, Option<u32>) {
    let all = meta.block_count + 1;
    let blocks = u64::from(meta.block_count.max(1));
    let room = gpu_mb.saturating_sub(kv_mb(meta, ctx, kv) + overhead_mb(ctx));
    if gpu_mb == 0 {
        return (0, None);
    }
    if room >= meta.file_mb {
        return (all, None);
    }
    if meta.is_moe() {
        let experts = meta.file_mb * EXPERT_SHARE_PCT / 100;
        let dense = meta.file_mb - experts;
        if room >= dense {
            let per_layer = (experts / blocks).max(1);
            let on_gpu = (room - dense) / per_layer;
            let cpu_layers = blocks.saturating_sub(on_gpu);
            return (all, Some(u32::try_from(cpu_layers).unwrap_or(u32::MAX)));
        }
    }
    let per_layer = (meta.file_mb / blocks).max(1);
    let n = (room / per_layer).min(u64::from(all));
    (u32::try_from(n).unwrap_or(0), None)
}

/// Load-log lines that mean the launch ran out of memory.
pub fn oom_in_log(log: &str) -> bool {
    let l = log.to_ascii_lowercase();
    [
        "out of memory",
        "failed to allocate",
        "unable to allocate",
        "cudamalloc failed",
        "hipmalloc failed",
        "erroroutofdevicememory",
        "insufficient memory",
    ]
    .iter()
    .any(|k| l.contains(k))
}

/// Out-of-memory retries before dropping to a smaller model.
pub const MAX_OOM_RETRIES: u8 = 3;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Retry {
    Again(LaunchArgs),
    SmallerModel,
}

/// After the `attempt`th out-of-memory failure: 10% fewer GPU layers (for
/// MoE, 10% more layers' experts in RAM), up to [`MAX_OOM_RETRIES`] times,
/// then the next smaller model.
pub fn after_oom(prev: &LaunchArgs, attempt: u8) -> Retry {
    if attempt > MAX_OOM_RETRIES || prev.ngl == 0 {
        return Retry::SmallerModel;
    }
    let blocks = prev.all_layers.saturating_sub(1);
    if let Some(n) = prev.n_cpu_moe.filter(|n| *n < blocks) {
        return Retry::Again(LaunchArgs {
            n_cpu_moe: Some((n + (blocks / 10).max(1)).min(blocks)),
            ..prev.clone()
        });
    }
    Retry::Again(LaunchArgs {
        ngl: (prev.ngl * 9 / 10).min(prev.ngl - 1),
        n_cpu_moe: None,
        ..prev.clone()
    })
}

/// The next smaller catalog model to chat with (never an optional one).
pub fn next_smaller(id: &str) -> Option<&'static LocalModel> {
    let models = local_setup::local_models();
    let at = models.iter().position(|m| m.id == id)?;
    models[at + 1..]
        .iter()
        .find(|m| !m.optional && !m.is_drafter)
}

/// One launch that ran after backing off, kept per model + device + driver.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tuned {
    pub ngl: u32,
    #[serde(default)]
    pub n_cpu_moe: Option<u32>,
    pub ctx: u32,
    pub kv: KvType,
}

pub const AUTOTUNE_FILE: &str = "autotune.json";

/// The GPUs a launch runs on (`cpu` without one), for per-device records.
pub fn device_label(p: &LocalProfile, backend: Backend) -> String {
    let plan = p.device_plan();
    if plan.gpus.is_empty() || backend == Backend::Cpu {
        "cpu".to_string()
    } else {
        plan.gpus
            .iter()
            .map(|g| g.name.as_str())
            .collect::<Vec<_>>()
            .join("+")
    }
}

pub fn tune_key(model: &Path, model_bytes: u64, p: &LocalProfile, backend: Backend) -> String {
    let plan = p.device_plan();
    let device = device_label(p, backend);
    let driver = plan
        .gpus
        .first()
        .map(|g| g.driver.as_str())
        .filter(|d| !d.is_empty())
        .unwrap_or("-");
    let file = model
        .file_name()
        .map(|f| f.to_string_lossy().into_owned())
        .unwrap_or_default();
    format!("{file}:{model_bytes}|{device}|{}|{driver}", backend.label())
}

pub fn load_tuned(dir: &Path) -> BTreeMap<String, Tuned> {
    std::fs::read_to_string(dir.join(AUTOTUNE_FILE))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save_tuned(dir: &Path, key: &str, t: &Tuned) -> Result<(), String> {
    let mut all = load_tuned(dir);
    all.insert(key.to_string(), t.clone());
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let json = serde_json::to_string_pretty(&all).map_err(|e| e.to_string())?;
    std::fs::write(dir.join(AUTOTUNE_FILE), json).map_err(|e| e.to_string())
}

/// A stored launch caps the fresh plan: never more layers or context than ran.
pub fn apply_tuned(args: LaunchArgs, t: &Tuned) -> LaunchArgs {
    LaunchArgs {
        ngl: args.ngl.min(t.ngl),
        ctx: args.ctx.min(t.ctx),
        kv: if t.kv == KvType::Q4 {
            KvType::Q4
        } else {
            args.kv
        },
        n_cpu_moe: match (args.n_cpu_moe, t.n_cpu_moe) {
            (Some(a), Some(b)) => Some(a.max(b)),
            (a, _) => a,
        },
        ..args
    }
}

struct Live {
    profile: Option<LocalProfile>,
    tune_dir: Option<PathBuf>,
    state: GovState,
    overrides: Overrides,
}

fn live() -> &'static Mutex<Live> {
    static L: OnceLock<Mutex<Live>> = OnceLock::new();
    L.get_or_init(|| {
        Mutex::new(Live {
            profile: None,
            tune_dir: None,
            state: GovState::Normal,
            overrides: Overrides::default(),
        })
    })
}

/// The cabin's hardware read, and where `autotune.json` lives.
pub fn set_profile(p: LocalProfile, tune_dir: PathBuf) {
    if let Ok(mut l) = live().lock() {
        l.profile = Some(p);
        l.tune_dir = Some(tune_dir);
    }
}

pub fn set_state(s: GovState) {
    if let Ok(mut l) = live().lock() {
        l.state = s;
    }
}

#[cfg(test)]
pub(crate) fn state() -> GovState {
    live().lock().map(|l| l.state).unwrap_or_default()
}

pub fn set_overrides(o: Overrides) {
    if let Ok(mut l) = live().lock() {
        l.overrides = o;
    }
}

/// Files next to `llama-server`, for [`local_profile::engine_backends`].
fn engine_files(binary: &Path) -> Vec<String> {
    binary
        .parent()
        .and_then(|d| std::fs::read_dir(d).ok())
        .map(|it| {
            it.flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default()
}

/// The planned launch for `model` once the cabin has read the hardware;
/// `None` (the one-shot `local_perf` defaults) before that or when the GGUF
/// header can't be read. Also returns the `autotune.json` key and folder.
pub fn launch_for(binary: &Path, model: &Path) -> Option<(LaunchArgs, String, PathBuf)> {
    planned(binary, model).map(|(args, _, key, dir)| (args, key, dir))
}

/// The draft half of [`launch_for`], for the speed test.
pub fn draft_plan(binary: &Path, model: &Path) -> Option<local_spec::Plan> {
    planned(binary, model).map(|(_, plan, _, _)| plan)
}

/// Where `autotune.json` and the draft verdicts live, once the profile is read.
pub fn tune_dir() -> Option<PathBuf> {
    live().lock().ok()?.tune_dir.clone()
}

fn planned(binary: &Path, model: &Path) -> Option<(LaunchArgs, local_spec::Plan, String, PathBuf)> {
    let (p, dir, state, o) = {
        let l = live().lock().ok()?;
        (
            l.profile.clone()?,
            l.tune_dir.clone()?,
            l.state,
            l.overrides.clone(),
        )
    };
    let meta = gguf::read_meta(model).ok()?;
    let files = engine_files(binary);
    let files: Vec<&str> = files.iter().map(String::as_str).collect();
    let available = local_profile::engine_backends(&files);
    // The speed test gates backends once card 5.3 caches it; until then the engine decides.
    let backend = local_profile::choose_backend(&p, &available, &|_| true);
    let bytes = std::fs::metadata(model).map(|m| m.len()).unwrap_or(0);
    let key = tune_key(model, bytes, &p, backend);
    let args = autoconfig(&p, &meta, backend, state, &o);
    let mut args = match load_tuned(&dir).get(&key) {
        Some(t) => apply_tuned(args, t),
        None => args,
    };
    let flags = local_spec::help_flags(binary);
    let device = device_label(&p, backend);
    let plan = local_spec::plan(&local_spec::Launch {
        flags: &flags,
        model,
        meta: &meta,
        profile: &p,
        state,
        args: &args,
        tune_dir: &dir,
        device: &device,
    });
    args.draft = plan.draft.clone().map(Box::new);
    Some((args, plan, key, dir))
}

/// The catalog row for a model file, for [`next_smaller`].
pub fn catalog_model_for(model: &Path) -> Option<&'static LocalModel> {
    let name = model.file_name()?.to_str()?;
    local_setup::local_models().iter().find(|m| m.file == name)
}

#[cfg(test)]
#[path = "local_autoconfig_tests.rs"]
mod tests;
