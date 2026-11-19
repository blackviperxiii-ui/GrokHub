//! Tiered auto-config for `llama-server`: from the hardware profile, the
//! model's GGUF header, the governor state and the user's overrides, size the
//! GPU offload (partial `-ngl`, MoE expert offload, multi-GPU split), the
//! context and KV cache, and the thread count. Pure apart from the stored
//! profile and the `autotune.json` record of launches that ran out of memory.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

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
    /// Keep every GPU out of it (`--device none`): set after the GPU backend
    /// crashed the engine at start. See [`on_cpu`].
    pub cpu_only: bool,
    /// How the engine shares the machine (poll, priority, batch size).
    pub run: super::local_flags::RunFlags,
    /// The vision projector loaded beside the model, for a chat whose newest
    /// message carries a picture (see [`vision_launch_with`]).
    pub projector: Option<Box<ProjectorArgs>>,
}

/// The vision projector half of one launch (`--mmproj`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectorArgs {
    pub file: PathBuf,
    /// What it takes: its file plus the picture encoder's compute buffer
    /// ([`projector_mb`]).
    pub mb: u64,
    /// The engine lists `--no-mmproj-offload`, which keeps it in RAM.
    pub off_gpu_flag: bool,
}

impl ProjectorArgs {
    /// `--mmproj <file>`, kept off the GPU (`--no-mmproj-offload`) when the
    /// model is (`on_cpu`): `--device none` alone doesn't keep it there,
    /// since the picture encoder starts the GPU backend on its own.
    pub fn to_args(&self, on_cpu: bool) -> Vec<String> {
        let mut a = vec!["--mmproj".to_string(), self.file.display().to_string()];
        if on_cpu && self.off_gpu_flag {
            a.push("--no-mmproj-offload".into());
        }
        a
    }
}

/// Share added on top of a projector's file for its compute buffer.
pub const PROJECTOR_BUFFER_PCT: u64 = 15;

/// MB a projector file of `bytes` takes once loaded.
pub fn projector_mb(bytes: u64) -> u64 {
    let file = bytes.div_ceil(1024 * 1024);
    file + file * PROJECTOR_BUFFER_PCT / 100
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
        a.extend(self.run.to_args());
        if let Some(d) = &self.draft {
            a.extend(d.to_args());
        }
        if let Some(v) = &self.projector {
            a.extend(v.to_args(self.cpu_only || self.ngl == 0));
        }
        if self.cpu_only {
            // `-ngl 0` alone still lets the GPU take big matmuls; this keeps
            // the engine off every GPU device.
            a.extend(["--device".into(), "none".into()]);
        }
        a.push("--cache-prompt".into());
        a
    }
}

/// Whether relaunching `running` as `next` would change anything that
/// matters: the same flags, or (with every layer on the GPU in both) only
/// the CPU thread counts differ, which a whole-GPU launch barely uses.
pub fn same_launch(running: &LaunchArgs, next: &LaunchArgs) -> bool {
    if running == next {
        return true;
    }
    let whole_gpu = |l: &LaunchArgs| !l.cpu_only && l.ngl >= l.all_layers && l.n_cpu_moe.is_none();
    whole_gpu(running)
        && whole_gpu(next)
        && LaunchArgs {
            threads: next.threads,
            threads_batch: next.threads_batch,
            ..running.clone()
        } == *next
}

/// (GPU, RAM) MB one launch takes, for deciding whether a second server
/// fits beside the running one during a warm relaunch.
pub fn launch_need_mb(meta: &GgufMeta, l: &LaunchArgs) -> (u64, u64) {
    let kv = kv_mb_for(meta, l.ctx, l.kv, l.run.seqs());
    let over = overhead_mb(l.ctx);
    if l.cpu_only || l.ngl == 0 {
        return (0, meta.file_mb + kv + over);
    }
    let blocks = u64::from(meta.block_count.max(1));
    let weights_gpu = match l.n_cpu_moe {
        Some(n) => {
            let experts = meta.file_mb * EXPERT_SHARE_PCT / 100;
            let on_gpu = blocks.saturating_sub(u64::from(n).min(blocks));
            meta.file_mb - experts + experts * on_gpu / blocks
        }
        None => meta.file_mb * u64::from(l.ngl.min(l.all_layers)) / u64::from(l.all_layers.max(1)),
    };
    (weights_gpu + kv + over, meta.file_mb - weights_gpu + 512)
}

/// RAM a warm relaunch leaves to the OS on top of both servers.
const BESIDE_RAM_RESERVE_MB: u64 = 2_048;

/// A launch needing `need` (GPU, RAM) MB fits in what is free `now`, with a
/// 10% margin; an unknown reading never fits.
pub fn fits_beside(now: FreeNow, need: (u64, u64)) -> bool {
    let room = |free: Option<u64>, want: u64, reserve: u64| {
        want == 0 || free.is_some_and(|f| f >= want + want / 10 + reserve)
    };
    room(now.vram_free_mb, need.0, 0) && room(now.ram_free_mb, need.1, BESIDE_RAM_RESERVE_MB)
}

/// `prev` with every GPU left out: no layers or experts on a GPU, no split.
/// A drafter stays with its model, in RAM too.
pub fn on_cpu(prev: &LaunchArgs) -> LaunchArgs {
    LaunchArgs {
        backend: Backend::Cpu,
        ngl: 0,
        n_cpu_moe: None,
        tensor_split: None,
        draft: prev.draft.as_ref().map(|d| {
            Box::new(DraftArgs {
                on_gpu: false,
                ..(**d).clone()
            })
        }),
        cpu_only: true,
        run: run_with_slots(1),
        ..prev.clone()
    }
}

/// The speed test is timing the gaming setup at idle (see `local_spec`).
static GAMING_TEST: AtomicBool = AtomicBool::new(false);

/// Plan launches as a game would see them, for the gaming speed test.
pub fn set_gaming_test(on: bool) {
    GAMING_TEST.store(on, Ordering::SeqCst);
}

fn gaming_test() -> bool {
    GAMING_TEST.load(Ordering::SeqCst)
}

/// [`on_cpu`] for Gaming mode: half the threads (at least two), so the
/// game keeps the GPU and most of the CPU.
pub fn gaming_launch(prev: &LaunchArgs) -> LaunchArgs {
    let threads = (prev.threads / 2).max(2).min(prev.threads.max(1));
    LaunchArgs {
        threads,
        threads_batch: prev.threads_batch.map(|t| (t / 2).max(threads)),
        ..on_cpu(prev)
    }
}

/// Context sizes tried, biggest first.
const CTX_STEPS: [u32; 6] = [131_072, 65_536, 32_768, 16_384, 8_192, 4_096];
/// The smallest context worth keeping q8 KV for; below it q4 buys room.
const Q8_FLOOR_CTX: u32 = 8_192;
/// Share of MoE weights that are experts (and may stay in RAM).
pub(crate) const EXPERT_SHARE_PCT: u64 = 80;
/// RAM left to the OS when nothing better is known.
const RAM_RESERVE_MB: u64 = 4_096;

/// Compute buffers grow with context.
pub fn overhead_mb(ctx: u32) -> u64 {
    512 + u64::from(ctx) / 128
}

/// KV cache MB for one sequence; see [`kv_mb_for`].
pub fn kv_mb(meta: &GgufMeta, ctx: u32, kv: KvType) -> u64 {
    kv_mb_for(meta, ctx, kv, 1)
}

/// The engine's default micro-batch, which a sliding-window cache adds room for.
const DEFAULT_UBATCH: u32 = 512;

/// Cells a sliding-window layer keeps for `seqs` sequences in one unified
/// cache: the window per sequence plus a micro-batch, padded to 256 as
/// llama.cpp sizes it, and never more than the context.
pub fn swa_cells(window: u32, seqs: u32, ctx: u32) -> u64 {
    let want = u64::from(window) * u64::from(seqs.max(1)) + u64::from(DEFAULT_UBATCH);
    want.div_ceil(256).saturating_mul(256).min(u64::from(ctx))
}

/// KV cache MB for `ctx` tokens shared by `seqs` slots: full-attention
/// layers hold the whole context, sliding-window layers only their window.
pub fn kv_mb_for(meta: &GgufMeta, ctx: u32, kv: KvType, seqs: u32) -> u64 {
    let cells = swa_cells(meta.sliding_window, seqs, ctx);
    let values = meta.kv_values(u64::from(ctx), cells);
    values / 1_024 * kv.bytes_per_1k() / (1_024 * 1_024)
}

/// (GPU, RAM) memory a launch may plan for in `state`.
pub fn budgets(p: &LocalProfile, backend: Backend, state: GovState) -> (u64, u64) {
    let pct = state.budget_pct();
    let plan = p.device_plan();
    let ram_free = p
        .free_ram_mb
        .unwrap_or_else(|| p.ram_mb.saturating_sub(RAM_RESERVE_MB));
    if backend == Backend::Cpu {
        return (0, ram_free * pct / 100);
    }
    if plan.gpus.is_empty() {
        // An iGPU (Vulkan or SYCL) offloads into shared RAM; give it half the
        // budget so the driver's shared-memory cap still fits.
        if !p.gpus.iter().any(|g| g.integrated) {
            return (0, ram_free * pct / 100);
        }
        let half = ram_free * pct / 100 / 2;
        return (half, half);
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

fn ctx_steps(
    meta: &GgufMeta,
    state: GovState,
    want: Option<u32>,
) -> impl Iterator<Item = u32> + use<> {
    let model_max = if meta.context_length > 0 {
        meta.context_length
    } else {
        4_096
    };
    let cap = state.ctx_cap().min(model_max).min(want.unwrap_or(u32::MAX));
    CTX_STEPS.into_iter().filter(move |c| *c <= cap.max(4_096))
}

/// MB one model (or drafter) takes at `ctx`: its file, KV cache and buffers.
fn need_mb(meta: &GgufMeta, ctx: u32, kv: KvType, seqs: u32) -> u64 {
    meta.file_mb + kv_mb_for(meta, ctx, kv, seqs) + overhead_mb(ctx)
}

/// MB the drafter in `reserve` takes beside the model at `ctx`.
fn reserve_mb(reserve: Option<&GgufMeta>, ctx: u32, kv: KvType, seqs: u32) -> u64 {
    reserve.map_or(0, |d| need_mb(d, ctx, kv, seqs))
}

/// What a launch loads beside the model: the drafter it must load with, and
/// the MB of a vision projector (0 without one).
#[derive(Clone, Copy, Default)]
struct Beside<'a> {
    drafter: Option<&'a GgufMeta>,
    projector_mb: u64,
}

impl Beside<'_> {
    fn mb(&self, ctx: u32, kv: KvType, seqs: u32) -> u64 {
        reserve_mb(self.drafter, ctx, kv, seqs) + self.projector_mb
    }
}

/// Whether the model, its KV and what loads beside it fit `total_mb`, at
/// the slots `slots(ctx)` gives.
fn fits(
    meta: &GgufMeta,
    beside: Beside<'_>,
    ctx: u32,
    kv: KvType,
    total_mb: u64,
    slots: &dyn Fn(u32) -> u32,
) -> bool {
    let seqs = slots(ctx);
    need_mb(meta, ctx, kv, seqs) + beside.mb(ctx, kv, seqs) <= total_mb
}

/// Context past this splits KV across GPU and RAM badly; partial offload stops here.
const PARTIAL_CTX_CAP: u32 = 16_384;
/// CPU-only launches keep prompt processing short.
const CPU_CTX_CAP: u32 = 8_192;

/// The largest context (and KV type) whose plan fits `limit` MB: q8 KV unless
/// only q4 reaches the floor context.
fn pick_ctx(
    meta: &GgufMeta,
    beside: Beside<'_>,
    steps: &[u32],
    limit: u64,
    floor: u32,
    kv: Option<KvType>,
    slots: &dyn Fn(u32) -> u32,
) -> Option<(u32, KvType)> {
    let largest = |kv: KvType| {
        steps
            .iter()
            .copied()
            .find(|c| fits(meta, beside, *c, kv, limit, slots))
    };
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
/// `reserve` is the drafter the model must load with: its file, KV and
/// buffers come out of the room first, so the context shrinks before the
/// drafter is left out.
pub fn autoconfig(
    p: &LocalProfile,
    meta: &GgufMeta,
    backend: Backend,
    state: GovState,
    o: &Overrides,
    reserve: Option<&GgufMeta>,
) -> LaunchArgs {
    autoconfig_beside(
        p,
        meta,
        backend,
        state,
        o,
        Beside {
            drafter: reserve,
            projector_mb: 0,
        },
    )
}

/// [`autoconfig`] with what loads beside the model: a vision projector
/// comes out of the room like the drafter, on the GPU beside a whole model.
fn autoconfig_beside(
    p: &LocalProfile,
    meta: &GgufMeta,
    backend: Backend,
    state: GovState,
    o: &Overrides,
    beside: Beside<'_>,
) -> LaunchArgs {
    let reserve = beside.drafter;
    let (gpu_mb, ram_mb) = budgets(p, backend, state);
    let steps: Vec<u32> = ctx_steps(meta, state, o.ctx).collect();
    let floor = Q8_FLOOR_CTX
        .min(meta.context_length.max(4_096))
        .min(o.ctx.unwrap_or(u32::MAX).max(4_096));
    let on_cpu = gpu_mb == 0;
    let slots = |ctx: u32| super::local_flags::slot_count(state, ctx, on_cpu);
    let full = (gpu_mb > 0)
        .then(|| pick_ctx(meta, beside, &steps, gpu_mb, floor, o.kv, &slots))
        .flatten()
        // A model split across GPU and RAM can't run its drafter, so a pair
        // takes a shorter context to stay whole on the GPU.
        .filter(|(c, _)| *c >= floor || reserve.is_some());
    let cap = if gpu_mb > 0 {
        PARTIAL_CTX_CAP
    } else {
        CPU_CTX_CAP
    };
    let capped: Vec<u32> = steps.iter().copied().filter(|c| *c <= cap).collect();
    let (ctx, kv) = full
        .or_else(|| pick_ctx(meta, beside, &capped, gpu_mb + ram_mb, floor, o.kv, &slots))
        .unwrap_or((4_096, o.kv.unwrap_or(KvType::Q4)));
    let all_layers = meta.block_count + 1;
    let seqs = slots(ctx);
    // The drafter (and a projector) sit on the GPU beside a whole model, so
    // the layers get what they leave.
    let gpu_left = gpu_mb.saturating_sub(beside.mb(ctx, kv, seqs));
    let (ngl, n_cpu_moe) = offload(meta, gpu_left, ctx, kv, seqs);
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
        // llama.cpp refuses a quantized V cache without flash attention, and
        // the CPU backend runs it too.
        flash_attn: true,
        mlock: p.free_ram_mb.is_some_and(|free| free > 2 * meta.file_mb),
        tensor_split: p.device_plan().tensor_split,
        draft: None,
        cpu_only: false,
        run: run_with_slots(seqs),
        projector: None,
    }
}

/// Engine flags with `n` slots (unified KV past one); [`planned`] keeps only
/// the flags the engine lists.
pub fn run_with_slots(n: u32) -> super::local_flags::RunFlags {
    super::local_flags::RunFlags {
        slots: super::local_flags::Slots {
            parallel: Some(n),
            kv_unified: Some(n > 1),
            ..Default::default()
        },
        ..Default::default()
    }
}

/// `-ngl` and `--n-cpu-moe` for the GPU memory left after KV and buffers.
fn offload(meta: &GgufMeta, gpu_mb: u64, ctx: u32, kv: KvType, seqs: u32) -> (u32, Option<u32>) {
    let all = meta.block_count + 1;
    let blocks = u64::from(meta.block_count.max(1));
    let room = gpu_mb.saturating_sub(kv_mb_for(meta, ctx, kv, seqs) + overhead_mb(ctx));
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
/// then the next smaller model. A model with a drafter goes straight to a
/// smaller model: the pair never splits, and fewer layers leave no room for
/// the drafter anyway.
pub fn after_oom(prev: &LaunchArgs, attempt: u8) -> Retry {
    if attempt > MAX_OOM_RETRIES || prev.draft.is_some() {
        return Retry::SmallerModel;
    }
    if prev.ngl == 0 {
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
    models[at + 1..].iter().find(|m| !m.optional && m.is_chat())
}

/// One launch that ran after backing off, kept per model + device + driver.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tuned {
    pub ngl: u32,
    #[serde(default)]
    pub n_cpu_moe: Option<u32>,
    pub ctx: u32,
    pub kv: KvType,
    /// The GPU backend crashed the engine here, so it runs on the CPU.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub cpu_only: bool,
    /// Clean starts at these caps since they were set or last grew.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub clean: u8,
}

fn is_zero(n: &u8) -> bool {
    *n == 0
}

/// Clean starts at a memory cap before it grows back a step.
pub const GROW_AFTER_CLEAN: u8 = 2;

/// The caps one step looser: a tenth more layers on the GPU (or a tenth
/// fewer layers' experts in RAM) and twice the context. `None` once nothing
/// is capped any more, and the record can go.
pub fn grow(t: &Tuned, all_layers: u32) -> Option<Tuned> {
    let step = (all_layers / 10).max(1);
    let blocks = all_layers.saturating_sub(1);
    let ctx_max = CTX_STEPS[0];
    let next = Tuned {
        ngl: (t.ngl + step).min(all_layers),
        n_cpu_moe: t
            .n_cpu_moe
            .and_then(|n| n.checked_sub((blocks / 10).max(1)))
            .filter(|n| *n > 0),
        ctx: t.ctx.saturating_mul(2).min(ctx_max),
        kv: t.kv,
        cpu_only: false,
        clean: 0,
    };
    let free = next.ngl >= all_layers && next.n_cpu_moe.is_none() && next.ctx >= ctx_max;
    (!free).then_some(next)
}

/// A start ran clean at `key`'s stored caps: after [`GROW_AFTER_CLEAN`] of
/// them the caps grow a step, so one bad night doesn't cap a model for good.
/// A CPU-only record (the GPU backend crashed) stays.
pub fn clean_start(dir: &Path, key: &str, all_layers: u32) -> Result<(), String> {
    let mut all = load_tuned(dir);
    let Some(t) = all.get_mut(key).filter(|t| !t.cpu_only) else {
        return Ok(());
    };
    t.clean = t.clean.saturating_add(1);
    if t.clean >= GROW_AFTER_CLEAN {
        match grow(t, all_layers) {
            Some(next) => *t = next,
            None => {
                all.remove(key);
            }
        }
    }
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let json = serde_json::to_string_pretty(&all).map_err(|e| e.to_string())?;
    std::fs::write(dir.join(AUTOTUNE_FILE), json).map_err(|e| e.to_string())
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
    if t.cpu_only {
        return on_cpu(&LaunchArgs {
            ctx: args.ctx.min(t.ctx),
            ..args
        });
    }
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
    /// The cabin's live free-memory read, taken at every launch.
    free_probe: Option<Arc<dyn Fn() -> FreeNow + Send + Sync>>,
    /// Free VRAM once the running model was resident (MB): what is left with
    /// our own model loaded, so only someone else's use counts as a drop.
    launch_free_vram_mb: Option<u64>,
    /// Free memory just before the running launch started.
    before_launch: FreeNow,
    /// What the running model itself takes (before launch minus resident),
    /// added back when planning what a fresh start would get.
    own: FreeNow,
}

/// Free memory right now, from the cabin's probe (`nvidia-smi`, the OS).
/// `None` where a reading isn't available; the hardware profile's value then stands.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FreeNow {
    /// Free VRAM across the discrete GPUs the plan uses.
    pub vram_free_mb: Option<u64>,
    pub ram_free_mb: Option<u64>,
}

/// The cabin's live free-memory read. Each launch (and relaunch after a
/// governor level change) plans against what is free *now*, so a game that
/// took GPU memory leaves the model with fewer layers on the GPU and a
/// shorter context instead of an out-of-memory start, and the next launch
/// after the game ends grows back.
pub fn set_free_probe(probe: Arc<dyn Fn() -> FreeNow + Send + Sync>) {
    if let Ok(mut l) = live().lock() {
        l.free_probe = Some(probe);
    }
}

/// The profile with its free memory replaced by a live reading. Free VRAM
/// goes to the plan's GPUs in proportion to their size.
pub fn with_free_now(mut p: LocalProfile, now: FreeNow) -> LocalProfile {
    if let Some(vram) = now.vram_free_mb {
        let plan = p.device_plan();
        let total: u64 = plan.gpus.iter().map(|g| g.vram_mb).sum();
        for g in p.gpus.iter_mut() {
            if plan
                .gpus
                .iter()
                .any(|pg| pg.name == g.name && pg.vram_mb == g.vram_mb)
            {
                let share = vram
                    .saturating_mul(g.vram_mb)
                    .checked_div(total)
                    .unwrap_or(vram);
                g.free_vram_mb = Some(share.min(g.vram_mb));
            }
        }
    }
    if let Some(ram) = now.ram_free_mb {
        p.free_ram_mb = Some(ram);
    }
    p
}

/// Free VRAM fell well under what the running launch was sized for (a game
/// loaded): the model should relaunch into what is left.
pub fn vram_dropped(launch_free_mb: u64, now_free_mb: u64) -> bool {
    now_free_mb.saturating_mul(100) < launch_free_mb.saturating_mul(VRAM_DROP_PCT)
}

/// A relaunch is worth it once free VRAM is under this share of the launch's.
pub const VRAM_DROP_PCT: u64 = 70;

/// The sidecar is up with the model loaded: free VRAM now is the baseline a
/// later drop is measured against (the model's own memory is already gone
/// from it, so loading it never reads as a game taking the GPU).
pub fn mark_resident() {
    let probe = live().lock().ok().and_then(|l| l.free_probe.clone());
    let now = probe.map(|f| f()).unwrap_or_default();
    if let Ok(mut l) = live().lock() {
        l.launch_free_vram_mb = now.vram_free_mb;
        l.own = taken(l.before_launch, now);
    }
}

/// A warm relaunch retired the old server: free VRAM now is the new
/// baseline (the old model's memory came back), the new model's own use stays.
pub fn refresh_resident() {
    let probe = live().lock().ok().and_then(|l| l.free_probe.clone());
    let now = probe.map(|f| f().vram_free_mb).unwrap_or_default();
    if let Ok(mut l) = live().lock() {
        l.launch_free_vram_mb = now;
    }
}

/// Memory a launch took: free before it minus free once it was resident.
pub fn taken(before: FreeNow, resident: FreeNow) -> FreeNow {
    let diff = |b: Option<u64>, r: Option<u64>| Some(b?.saturating_sub(r?));
    FreeNow {
        vram_free_mb: diff(before.vram_free_mb, resident.vram_free_mb),
        ram_free_mb: diff(before.ram_free_mb, resident.ram_free_mb),
    }
}

/// `now` with the running model's own memory added back: what a fresh start
/// of it would see.
pub fn with_own(now: FreeNow, own: FreeNow) -> FreeNow {
    let add = |n: Option<u64>, o: Option<u64>| n.map(|n| n + o.unwrap_or(0));
    FreeNow {
        vram_free_mb: add(now.vram_free_mb, own.vram_free_mb),
        ram_free_mb: add(now.ram_free_mb, own.ram_free_mb),
    }
}

/// Free memory now, from the probe (both `None` without one).
pub fn free_now() -> FreeNow {
    let probe = live().lock().ok().and_then(|l| l.free_probe.clone());
    probe.map(|f| f()).unwrap_or_default()
}

/// Tests: forget the probe and the baseline, so no later test plans against
/// a fake reading.
#[cfg(test)]
pub(crate) fn reset_free_probe() {
    if let Ok(mut l) = live().lock() {
        l.free_probe = None;
        l.launch_free_vram_mb = None;
        l.before_launch = FreeNow::default();
        l.own = FreeNow::default();
    }
}

/// A launch is starting (or the model left): no baseline until the new
/// model is resident, so the last model's reading never judges this one.
pub fn clear_resident() {
    let probe = live().lock().ok().and_then(|l| l.free_probe.clone());
    let before = probe.map(|f| f()).unwrap_or_default();
    if let Ok(mut l) = live().lock() {
        l.launch_free_vram_mb = None;
        l.before_launch = before;
        l.own = FreeNow::default();
    }
}

/// Under Throttle: free VRAM now is well under what was free once the model
/// was resident, so the governor should restart the model into the smaller room.
pub fn vram_dropped_since_launch() -> bool {
    let (probe, launched) = {
        let Ok(l) = live().lock() else {
            return false;
        };
        (l.free_probe.clone(), l.launch_free_vram_mb)
    };
    match (probe, launched) {
        (Some(probe), Some(launched)) => probe()
            .vram_free_mb
            .is_some_and(|now| vram_dropped(launched, now)),
        _ => false,
    }
}

fn live() -> &'static Mutex<Live> {
    static L: OnceLock<Mutex<Live>> = OnceLock::new();
    L.get_or_init(|| {
        Mutex::new(Live {
            profile: None,
            tune_dir: None,
            state: GovState::Normal,
            overrides: Overrides::default(),
            free_probe: None,
            launch_free_vram_mb: None,
            before_launch: FreeNow::default(),
            own: FreeNow::default(),
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

/// Tests: forget the hardware read.
#[cfg(test)]
pub(crate) fn forget_profile() {
    if let Ok(mut l) = live().lock() {
        l.profile = None;
        l.tune_dir = None;
    }
}

/// The cabin's hardware read, once it has one.
pub fn profile() -> Option<LocalProfile> {
    live().lock().ok()?.profile.clone()
}

pub fn set_state(s: GovState) {
    if let Ok(mut l) = live().lock() {
        l.state = s;
    }
}

/// The governor state launches plan in.
pub fn state() -> GovState {
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

/// How a launch refused because the model and its drafter don't fit
/// together begins. The caller tries a smaller model.
pub const PAIR_NO_ROOM: &str = "there isn't room for the model and its draft model together";

/// Why a pair was refused when the Settings GPU layers override is the cause.
pub const PAIR_OVERRIDE: &str = "the GPU layers override keeps part of the model off the GPU";

/// The refusal for a pair planned as `a`: it names the GPU layers override
/// (`override_ngl`) when that keeps part of a launch that puts the pair on
/// the GPU (`on_gpu`) off it, since a pair only runs whole there.
pub fn pair_refusal(override_ngl: Option<u32>, a: &LaunchArgs, on_gpu: bool) -> String {
    if on_gpu && override_ngl.is_some_and(|n| n < a.all_layers) {
        format!("{PAIR_NO_ROOM}, because {PAIR_OVERRIDE}")
    } else {
        PAIR_NO_ROOM.to_string()
    }
}

/// A planned launch, its `autotune.json` key and folder.
pub type Planned = (LaunchArgs, String, PathBuf);

/// The planned launch for `model` once the cabin has read the hardware;
/// `Ok(None)` (the one-shot `local_perf` defaults) before that or when the
/// GGUF header can't be read. Also returns the `autotune.json` key and
/// folder. `Err` ([`PAIR_NO_ROOM`]) when the model and its drafter don't fit.
pub fn launch_for(binary: &Path, model: &Path) -> Result<Option<Planned>, String> {
    launch_with(binary, model, None)
}

/// [`launch_for`] planned against `free` instead of the probe's reading.
pub fn launch_with(
    binary: &Path,
    model: &Path,
    free: Option<FreeNow>,
) -> Result<Option<Planned>, String> {
    let Some(Planning {
        args,
        plan,
        key,
        dir,
        refusal,
        ..
    }) = planned(binary, model, free)
    else {
        return Ok(None);
    };
    if let Some(r) = refusal {
        if let Ok(mut last) = LAST_REFUSAL.lock() {
            *last = Some((model.to_path_buf(), r));
        }
        let o = live().lock().map_or(None, |l| l.overrides.ngl);
        return Err(pair_refusal(o, &args, r.on_gpu));
    }
    local_spec::launched(&plan, &dir);
    Ok(Some((args, key, dir)))
}

/// Where a launch sized a pair it refused (see [`local_spec::fits`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pool {
    Vram,
    Ram,
}

/// A pair a launch refused, for the hold that keeps the planner from
/// reloading into the same refusal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PairRefusal {
    /// VRAM when the launch planned the pair on a GPU, else RAM.
    pub pool: Pool,
    /// What that pool had free at the refusal (the launch's own live read,
    /// taken after the previous server stopped); `None` without a reading.
    pub free_mb: Option<u64>,
    /// The governor state the launch planned in.
    pub state: GovState,
    /// The launch planned the pair on a GPU (an iGPU too).
    pub on_gpu: bool,
}

/// The refusal of a pair launched as `a` on `p` (its free memory already
/// `now`) in `state`.
fn refusal_of(p: &LocalProfile, a: &LaunchArgs, state: GovState, now: FreeNow) -> PairRefusal {
    let on_gpu = budgets(p, a.backend, state).0 > 0;
    // A GPU launch with no VRAM reading was sized from a fixed figure that
    // more free RAM doesn't change: no baseline, so it holds until unload.
    let (pool, free_mb) = if on_gpu {
        (Pool::Vram, now.vram_free_mb)
    } else {
        (Pool::Ram, now.ram_free_mb)
    };
    PairRefusal {
        pool,
        free_mb,
        state,
        on_gpu,
    }
}

/// The last pair refusal [`launch_with`] gave, and for which model.
static LAST_REFUSAL: Mutex<Option<(PathBuf, PairRefusal)>> = Mutex::new(None);

/// The refusal [`launch_with`] last gave `model`, taken.
pub fn take_refusal(model: &Path) -> Option<PairRefusal> {
    let mut last = LAST_REFUSAL.lock().ok()?;
    match last.take() {
        Some((m, r)) if m == model => Some(r),
        other => {
            *last = other;
            None
        }
    }
}

/// The draft half of [`launch_for`], for the speed test.
pub fn draft_plan(binary: &Path, model: &Path) -> Option<local_spec::Plan> {
    draft_plan_with(binary, model, None)
}

/// [`draft_plan`] planned against `free` instead of the probe's reading.
pub fn draft_plan_with(
    binary: &Path,
    model: &Path,
    free: Option<FreeNow>,
) -> Option<local_spec::Plan> {
    planned(binary, model, free).map(|p| p.plan)
}

/// What a fresh start of the running model would see free now: the probe's
/// reading with the model's own memory added back.
pub fn replan_free() -> FreeNow {
    with_own(free_now(), own_use())
}

/// What the running model itself takes, as measured once it was resident.
pub fn own_use() -> FreeNow {
    live().lock().map(|l| l.own).unwrap_or_default()
}

/// [`launch_for`] as a fresh start of the running model would plan it (see
/// [`replan_free`]), leaving the drafter's record alone. `None` before the
/// profile is read, when the header can't be, or when the model and its
/// drafter no longer fit (the reload then picks a smaller model).
pub fn replan_for(binary: &Path, model: &Path) -> Option<Planned> {
    planned(binary, model, Some(replan_free()))
        .filter(|p| p.refusal.is_none())
        .map(|p| (p.args, p.key, p.dir))
}

/// The second server of a warm relaunch, planned as `next`, fits beside the
/// running one in what is free now. Never with a drafter or a vision
/// projector (their memory isn't counted) or when the header or a reading is
/// missing.
pub fn warm_fits(model: &Path, next: &LaunchArgs) -> bool {
    if next.draft.is_some() || next.projector.is_some() {
        return false;
    }
    let Ok(meta) = gguf::read_meta(model) else {
        return false;
    };
    fits_beside(free_now(), launch_need_mb(&meta, next))
}

/// Where `autotune.json` and the draft verdicts live, once the profile is read.
pub fn tune_dir() -> Option<PathBuf> {
    live().lock().ok()?.tune_dir.clone()
}

/// One planned launch: its args and draft plan, its `autotune.json` key and
/// folder, and the refusal when its pair doesn't fit.
struct Planning {
    args: LaunchArgs,
    plan: local_spec::Plan,
    key: String,
    dir: PathBuf,
    refusal: Option<PairRefusal>,
    /// Planned with a vision projector: whether the model, its drafter and
    /// the projector all fit. `true` without one.
    projector_fits: bool,
}

fn planned(binary: &Path, model: &Path, free: Option<FreeNow>) -> Option<Planning> {
    planned_as(binary, model, free, false, None)
}

/// Why a launch with a vision projector didn't plan; the caller loads the
/// model without one, or a smaller model that has one.
pub const NO_PROJECTOR: &str = "no vision projector for this model is on disk";
/// A vision launch that doesn't fit: the walk tries a smaller model.
pub const PROJECTOR_NO_ROOM: &str =
    "there isn't room for the model, its draft model and its vision projector together";

/// The vision projector a launch of `model` loads: the catalog's projector
/// file beside the model, when this engine lists `--mmproj` and has never
/// refused it with this model ([`super::local_vision::refused`]).
pub fn projector_args(binary: &Path, model: &Path) -> Option<ProjectorArgs> {
    let file = super::local_vision::projector_file(model)?;
    let options = super::local_flags::engine_options(binary);
    if !options.contains("--mmproj") || super::local_vision::refused(binary, model) {
        return None;
    }
    let bytes = std::fs::metadata(&file).ok()?.len();
    Some(ProjectorArgs {
        file,
        mb: projector_mb(bytes),
        off_gpu_flag: options.contains("--no-mmproj-offload"),
    })
}

/// [`launch_with`] with `model`'s vision projector beside it and its
/// drafter. A pair that doesn't fit with the projector is refused
/// ([`PROJECTOR_NO_ROOM`]) without the record [`take_refusal`] reads: the
/// pair itself may well fit, and the walk tries a smaller model.
pub fn vision_launch_with(
    binary: &Path,
    model: &Path,
    free: Option<FreeNow>,
) -> Result<Option<Planned>, String> {
    let projector = projector_args(binary, model).ok_or(NO_PROJECTOR)?;
    let Some(p) = planned_as(binary, model, free, false, Some(&projector)) else {
        return Ok(None);
    };
    if !p.projector_fits {
        return Err(PROJECTOR_NO_ROOM.into());
    }
    local_spec::launched(&p.plan, &p.dir);
    Ok(Some((p.args, p.key, p.dir)))
}

/// [`replan_for`] keeping the running launch's vision projector, so a
/// governor restart with the same plan keeps it loaded. `None` when it no
/// longer fits.
pub fn replan_vision_for(binary: &Path, model: &Path) -> Option<Planned> {
    let projector = projector_args(binary, model)?;
    planned_as(binary, model, Some(replan_free()), false, Some(&projector))
        .filter(|p| p.projector_fits)
        .map(|p| (p.args, p.key, p.dir))
}

/// The ids of the models in `on_disk` that can load a vision projector on
/// this engine ([`projector_args`]), for the swap planner.
pub fn vision_view(binary: &Path, on_disk: &[(&'static LocalModel, PathBuf)]) -> Vec<&'static str> {
    on_disk
        .iter()
        .filter(|(_, path)| projector_args(binary, path).is_some())
        .map(|(m, _)| m.id)
        .collect()
}

/// The backend a launch of `binary` uses on `p`.
fn backend_for(p: &LocalProfile, binary: &Path) -> Backend {
    let files = engine_files(binary);
    let files: Vec<&str> = files.iter().map(String::as_str).collect();
    let available = local_profile::engine_backends(&files);
    // The speed test gates backends once card 5.3 caches it; until then the engine decides.
    local_profile::choose_backend(p, &available, &|_| true)
}

/// The hardware read, and where `autotune.json` lives.
fn hardware_read() -> Option<(LocalProfile, PathBuf)> {
    let l = live().lock().ok()?;
    Some((l.profile.clone()?, l.tune_dir.clone()?))
}

/// The ids of the models in `on_disk` that launch with a separate drafter
/// file on this device and engine (see [`local_spec::pairing`]), so the swap
/// planner sizes them as a pair. Empty before the hardware read, when a
/// launch runs the one-shot defaults alone.
pub fn pair_view(binary: &Path, on_disk: &[(&'static LocalModel, PathBuf)]) -> Vec<&'static str> {
    let Some((p, dir)) = hardware_read() else {
        return Vec::new();
    };
    let device = device_label(&p, backend_for(&p, binary));
    let flags = local_spec::help_flags(binary);
    on_disk
        .iter()
        .filter(|(_, path)| {
            local_spec::header(path).is_some_and(|meta| {
                local_spec::pairing(&flags, path, &meta, &dir, &device)
                    .reserve()
                    .is_some()
            })
        })
        .map(|(m, _)| m.id)
        .collect()
}

/// [`draft_plan`] for `model` in the gaming setup, whether a game runs or not.
pub fn gaming_draft_plan(binary: &Path, model: &Path) -> Option<local_spec::Plan> {
    planned_as(binary, model, None, true, None).map(|p| p.plan)
}

/// [`planned`]; `gaming` plans the gaming setup even with no game running,
/// and `projector` plans the vision projector beside the model.
fn planned_as(
    binary: &Path,
    model: &Path,
    free: Option<FreeNow>,
    gaming: bool,
    projector: Option<&ProjectorArgs>,
) -> Option<Planning> {
    let (p, dir, state, o, probe) = {
        let l = live().lock().ok()?;
        (
            l.profile.clone()?,
            l.tune_dir.clone()?,
            l.state,
            l.overrides.clone(),
            l.free_probe.clone(),
        )
    };
    // Plan against the memory free right now, not the probe's at setup.
    let now = free.unwrap_or_else(|| probe.as_ref().map(|f| f()).unwrap_or_default());
    let p = with_free_now(p, now);
    let meta = gguf::read_meta(model).ok()?;
    let backend = backend_for(&p, binary);
    let bytes = std::fs::metadata(model).map(|m| m.len()).unwrap_or(0);
    let key = tune_key(model, bytes, &p, backend);
    let flags = local_spec::help_flags(binary);
    let device = device_label(&p, backend);
    // A paired model is sized with room for its drafter, and for the vision
    // projector when it loads one.
    let pairing = local_spec::pairing(&flags, model, &meta, &dir, &device);
    let projector_mb = projector.map_or(0, |v| v.mb);
    let beside = Beside {
        drafter: pairing.reserve(),
        projector_mb,
    };
    let args = autoconfig_beside(&p, &meta, backend, state, &o, beside);
    let mut args = match load_tuned(&dir).get(&key) {
        // A pair never splits, so a layer cap from an older back-off would
        // refuse it on every launch: only its context and KV caps count.
        Some(t) if pairing.reserve().is_some() => apply_tuned(
            args,
            &Tuned {
                ngl: u32::MAX,
                n_cpu_moe: None,
                ..t.clone()
            },
        ),
        Some(t) => apply_tuned(args, t),
        None => args,
    };
    // The drafter's room check counts the projector too.
    args.projector = projector.cloned().map(Box::new);
    let launch = |args: &LaunchArgs, device: &str, pairing: local_spec::Pairing| {
        local_spec::plan_with(
            &local_spec::Launch {
                flags: &flags,
                model,
                meta: &meta,
                profile: &p,
                state,
                args,
                tune_dir: &dir,
                device,
            },
            pairing,
        )
    };
    let mut plan = launch(&args, &device, pairing);
    args.draft = plan.draft.clone().map(Box::new);
    // Gaming mode: a game holds the GPU, so the model runs on the CPU with
    // half the threads and the game keeps its frame rate. Its drafter joins
    // on the CPU too, with its own speed test there; when both don't fit in
    // RAM the launch is refused and a smaller model runs.
    if gaming || gaming_test() || super::local_scale::gaming_reason(super::local_gov::reason()) {
        let pairing = local_spec::pairing(&flags, model, &meta, &dir, local_spec::GAMING_DEVICE);
        // The pair moves to RAM: size the context for the RAM budget with
        // the drafter in it, so the context shrinks before the tier does.
        if let Some(d) = pairing.reserve() {
            let q4 = (args.kv == KvType::Q4).then_some(KvType::Q4);
            let capped = Overrides {
                ctx: Some(args.ctx),
                kv: o.kv.or(q4),
                ..o.clone()
            };
            let beside = Beside {
                drafter: Some(d),
                projector_mb,
            };
            let ram = autoconfig_beside(&p, &meta, Backend::Cpu, state, &capped, beside);
            args.ctx = ram.ctx;
            args.kv = ram.kv;
        }
        args = gaming_launch(&args);
        plan = launch(&args, local_spec::GAMING_DEVICE, pairing);
        args.draft = plan.draft.clone().map(Box::new);
    }
    let options = super::local_flags::engine_options(binary);
    // A tuned or gaming launch may have cut the context or left the GPU:
    // never more slots than memory was planned for, nor than now fit.
    let on_cpu = args.cpu_only || args.ngl == 0;
    let n = args
        .run
        .seqs()
        .min(super::local_flags::slot_count(state, args.ctx, on_cpu));
    let slots = super::local_flags::slots(&options, n, p.free_ram_mb);
    let shared = state == GovState::Throttle;
    // Kept up under Throttle, it shares the machine: no spinning threads,
    // low priority, and short GPU submissions so other apps' frames wait less.
    args.run = if state == GovState::Throttle {
        let on_gpu = !args.cpu_only && args.ngl > 0;
        super::local_flags::shared(&options, args.backend, on_gpu)
    } else {
        Default::default()
    };
    args.run.slots = slots;
    if !shared {
        tune(&mut args, &options, &dir, &key, state, p.free_ram_mb);
    }
    let no_room = plan.decision == grokhub_core::local_draft::Decision::NoRoom;
    let refusal = no_room.then(|| refusal_of(&p, &args, state, now));
    // With a projector, the whole launch must fit what the state allows. A
    // pair with a drafter file passed its own room check above, the
    // projector counted; a model alone (or with its own heads) is checked
    // here.
    let projector_fits = projector.is_none()
        || (!no_room && {
            let drafted = args.draft.as_ref().is_some_and(|d| d.model.is_some());
            drafted || {
                let (gpu, ram) = budgets(&p, args.backend, state);
                let seqs = args.run.seqs();
                need_mb(&meta, args.ctx, args.kv, seqs) + projector_mb <= gpu + ram
            }
        });
    Some(Planning {
        args,
        plan,
        key,
        dir,
        refusal,
        projector_fits,
    })
}

/// Runs the auto-tune arm for this device × model × mode on `args` (see
/// [`super::local_tune`]); Throttle launches are never tuned.
fn tune(
    args: &mut LaunchArgs,
    options: &std::collections::BTreeSet<String>,
    dir: &Path,
    key: &str,
    state: GovState,
    ram_free_mb: Option<u64>,
) {
    use super::local_tune::{self as t, Arm};
    let base = Arm {
        slots: args.run.seqs(),
        draft: args.draft.is_some(),
        ubatch: args.run.ubatch.unwrap_or(t::UBATCH_DEFAULT),
    };
    let cands = t::candidates(base, options.contains("--ubatch-size"));
    let key = format!("{key}|{state:?}");
    let arm = t::current(dir, &key, &cands, base);
    if arm.slots != base.slots {
        args.run.slots = super::local_flags::slots(options, arm.slots, ram_free_mb);
    }
    if arm.ubatch != base.ubatch {
        args.run.ubatch = Some(arm.ubatch);
    }
    args.run.tune = Some(Box::new(t::Tag {
        key,
        arm,
        base,
        cands,
    }));
}

/// The catalog row for a model file, for [`next_smaller`].
pub fn catalog_model_for(model: &Path) -> Option<&'static LocalModel> {
    let name = model.file_name()?.to_str()?;
    local_setup::local_models().iter().find(|m| m.file == name)
}

#[cfg(test)]
#[path = "local_autoconfig_tests.rs"]
mod tests;
