//! The on-device model picker: scores every catalog model against this
//! machine's memory, CPU, engine and disk, sorts them into Recommended, May be
//! slow and Hidden with a plain reason, and pre-selects the tier default.
//! Pure: the hardware probe and the speed test hand their readings here.

use crate::local_setup::{disk_needed_mb, local_models, size_label, Hardware, LocalModel};
use std::collections::BTreeMap;

/// RAM left to the OS when a model runs from RAM.
pub const SYSTEM_RESERVE_MB: u64 = 4_096;
/// RAM kept free when a model spills past its budget ("May be slow").
pub const SPILL_RESERVE_MB: u64 = 2_048;
/// Share of Apple unified memory a model may use.
pub const UNIFIED_PERCENT: u64 = 70;
/// The smallest dedicated GPU worth running a model on; below it the RAM budget wins.
pub const MIN_GPU_MB: u64 = 3_072;
/// Above this budget the 31B tier default counts as T5.
pub const T5_BUDGET_MB: u64 = 24_576;

/// What the CPU can run. llama.cpp's x86 builds need AVX at least; models
/// marked `needs_avx2` need AVX2.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CpuSimd {
    Avx2,
    AvxOnly,
    NoAvx,
    Neon,
}

impl CpuSimd {
    pub fn detect() -> Self {
        #[cfg(target_arch = "x86_64")]
        {
            if std::arch::is_x86_feature_detected!("avx2") {
                return Self::Avx2;
            }
            if std::arch::is_x86_feature_detected!("avx") {
                return Self::AvxOnly;
            }
            Self::NoAvx
        }
        #[cfg(target_arch = "aarch64")]
        {
            Self::Neon
        }
        #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
        {
            Self::NoAvx
        }
    }
}

/// Where a model's budget comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Memory {
    Gpu,
    Unified,
    System,
}

impl Memory {
    fn words(self) -> &'static str {
        match self {
            Memory::Gpu => "GPU memory",
            Memory::Unified => "unified memory",
            Memory::System => "spare RAM",
        }
    }
}

/// What the picker knows about this machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Profile {
    /// Dedicated GPUs on one backend, VRAM in MiB. iGPUs are left out: they share RAM.
    pub gpus_vram_mb: Vec<u64>,
    /// Apple Silicon: the GPU uses system RAM.
    pub unified: bool,
    pub ram_mb: u64,
    pub free_disk_mb: u64,
    pub simd: CpuSimd,
    /// The llama.cpp build number (`b11602` → 11602); `None` when unknown.
    pub llama_build: Option<u32>,
}

impl Profile {
    /// The wizard's reading. Apple Silicon is unified; the engine is the pinned one.
    pub fn from_hardware(hw: &Hardware, simd: CpuSimd) -> Self {
        Profile {
            gpus_vram_mb: if hw.vram_mb > 0 {
                vec![hw.vram_mb]
            } else {
                Vec::new()
            },
            unified: cfg!(all(target_os = "macos", target_arch = "aarch64")) && hw.vram_mb == 0,
            ram_mb: hw.ram_mb,
            free_disk_mb: hw.free_disk_mb,
            simd,
            llama_build: crate::runtime_download::manifest_for_host()
                .and_then(|m| build_number(&m.version)),
        }
    }

    /// The usable memory budget and where it comes from: dedicated VRAM (summed
    /// across GPUs), 70% of Apple unified memory, else RAM minus 4 GB.
    pub fn budget(&self) -> (Memory, u64) {
        if self.unified {
            return (Memory::Unified, self.ram_mb * UNIFIED_PERCENT / 100);
        }
        let vram: u64 = self.gpus_vram_mb.iter().sum();
        if vram >= MIN_GPU_MB {
            (Memory::Gpu, vram)
        } else {
            (
                Memory::System,
                self.ram_mb.saturating_sub(SYSTEM_RESERVE_MB),
            )
        }
    }

    /// The most a model may spill into: VRAM plus RAM, less what the OS keeps.
    pub fn capacity(&self) -> u64 {
        let ram = self.ram_mb.saturating_sub(SPILL_RESERVE_MB);
        match self.budget() {
            (Memory::Gpu, vram) => vram + ram,
            _ => ram,
        }
    }
}

/// `b11602` → 11602.
pub fn build_number(version: &str) -> Option<u32> {
    version.trim().strip_prefix('b')?.parse().ok()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bucket {
    Recommended,
    MaySlow,
    Hidden,
}

/// One scored catalog model.
#[derive(Debug, Clone, PartialEq)]
pub struct Pick {
    pub model: &'static LocalModel,
    pub bucket: Bucket,
    pub reason: String,
    /// The tier default. At most one per machine, never an optional model.
    pub preselected: bool,
}

/// Measured generation tok/s on this machine, by model id (the speed test's cache).
pub type BenchCache = BTreeMap<String, f64>;

/// Every catalog model but the drafters, scored for `p`, biggest first.
pub fn score(p: &Profile, catalog: &'static [LocalModel], bench: &BenchCache) -> Vec<Pick> {
    let mut picks: Vec<Pick> = catalog
        .iter()
        .filter(|m| m.is_chat())
        .map(|m| {
            let (bucket, reason) = rate(p, m, bench.get(m.id).copied());
            Pick {
                model: m,
                bucket,
                reason,
                preselected: false,
            }
        })
        .collect();
    let best = recommended_default(&picks, quick_floor(p), |m| {
        speed_here(p, m, bench.get(m.id).copied())
    });
    if let Some(top) = picks.iter_mut().find(|x| Some(x.model.id) == best) {
        top.preselected = true;
    }
    picks
}

/// A default this fast counts as quick here, by where it runs (tok/s).
pub const QUICK_GPU_TOK_S: f64 = 30.0;
pub const QUICK_UNIFIED_TOK_S: f64 = 20.0;
pub const QUICK_CPU_TOK_S: f64 = 8.0;
/// A default at least this capable (`local_skills` quality) can carry
/// everyday chat on its own.
pub const CAPABLE_QUALITY: u8 = 50;
/// How much speed counts against capability when the default is picked:
/// `quality x tok/s ^ SPEED_WEIGHT`. Under 1, so a much faster model must
/// still be close in quality; high enough that a fast MoE beats a slower
/// dense model one size up.
pub const SPEED_WEIGHT: f64 = 0.75;

/// Generation speed for `m` on `p`: the speed test's reading, else the
/// catalog's estimate for where it runs.
pub fn speed_here(p: &Profile, m: &LocalModel, measured: Option<f64>) -> f64 {
    if let Some(t) = measured {
        return t;
    }
    let (mem, _) = p.budget();
    f64::from(match mem {
        Memory::Gpu => m.tok_s.gpu,
        Memory::Unified => m.tok_s.unified,
        Memory::System => m.tok_s.cpu,
    })
}

/// The tok/s a default should reach on `p` to count as quick.
pub fn quick_floor(p: &Profile) -> f64 {
    match p.budget().0 {
        Memory::Gpu => QUICK_GPU_TOK_S,
        Memory::Unified => QUICK_UNIFIED_TOK_S,
        Memory::System => QUICK_CPU_TOK_S,
    }
}

/// The recommended everyday model: speedy first, then capable. Among the
/// Recommended rows the app may pick (never an optional one), keep those
/// that are quick here (`floor` tok/s) and capable enough, when any are,
/// then take the best `quality x speed ^ SPEED_WEIGHT`; a tie goes to the
/// faster. The router steps up to a more capable model on disk when a task
/// needs it, so the default doesn't have to be the biggest that fits.
fn recommended_default(
    picks: &[Pick],
    floor: f64,
    speed: impl Fn(&LocalModel) -> f64,
) -> Option<&'static str> {
    let pool: Vec<(&'static str, f64, u8)> = picks
        .iter()
        .filter(|x| x.bucket == Bucket::Recommended && !x.model.optional)
        .map(|x| {
            let q = crate::local_skills::resolved(x.model).quality;
            (x.model.id, speed(x.model), q)
        })
        .collect();
    best_default(pool, floor)
}

/// The recommended everyday model among `models` (all assumed to run well
/// on `p`), by the catalog's speed estimates: the same rule as the
/// picker's pre-selection, for the lineup.
pub fn default_among(p: &Profile, models: &[&'static LocalModel]) -> Option<&'static LocalModel> {
    let pool = models
        .iter()
        .filter(|m| m.is_chat() && !m.optional)
        .map(|m| {
            (
                m.id,
                speed_here(p, m, None),
                crate::local_skills::resolved(m).quality,
            )
        })
        .collect();
    let id = best_default(pool, quick_floor(p))?;
    models.iter().copied().find(|m| m.id == id)
}

fn best_default(pool: Vec<(&'static str, f64, u8)>, floor: f64) -> Option<&'static str> {
    let narrowed = |keep: &dyn Fn(&(&'static str, f64, u8)) -> bool,
                    from: Vec<(&'static str, f64, u8)>| {
        let kept: Vec<_> = from.iter().copied().filter(|r| keep(r)).collect();
        if kept.is_empty() {
            from
        } else {
            kept
        }
    };
    let pool = narrowed(&|r| r.1 >= floor, pool);
    let pool = narrowed(&|r| r.2 >= CAPABLE_QUALITY, pool);
    let value = |r: &(&'static str, f64, u8)| f64::from(r.2) * r.1.max(0.0).powf(SPEED_WEIGHT);
    pool.into_iter()
        .max_by(|a, b| {
            value(a)
                .total_cmp(&value(b))
                .then(a.1.total_cmp(&b.1))
        })
        .map(|r| r.0)
}

fn rate(p: &Profile, m: &LocalModel, measured: Option<f64>) -> (Bucket, String) {
    let hidden = |why: String| (Bucket::Hidden, why);
    match p.simd {
        CpuSimd::NoAvx => {
            return hidden(format!(
                "This CPU has no AVX, so local AI is off and {product} uses the cloud.",
                product = crate::brand::product_name()
            ))
        }
        CpuSimd::AvxOnly if m.needs_avx2 => {
            return hidden("Needs AVX2; this CPU has AVX only.".into())
        }
        _ => {}
    }
    if let (Some(need), Some(have)) = (m.min_llama_build, p.llama_build) {
        if have < need {
            return hidden(format!(
                "Needs llama.cpp b{need} or newer; this engine is b{have}."
            ));
        }
    }
    if p.free_disk_mb < disk_needed_mb(m) {
        return hidden(format!(
            "Needs {} of free disk; this PC has {}.",
            size_label(disk_needed_mb(m)),
            size_label(p.free_disk_mb)
        ));
    }
    if p.capacity() < m.run_mb {
        return hidden(format!(
            "Needs {} of memory; this PC has {}.",
            size_label(m.run_mb),
            size_label(p.capacity())
        ));
    }
    let (mem, budget) = p.budget();
    let (min, est) = match mem {
        Memory::Gpu => (m.min_budget.gpu, m.tok_s.gpu),
        Memory::Unified => (m.min_budget.unified, m.tok_s.unified),
        Memory::System => (m.min_budget.cpu, m.tok_s.cpu),
    };
    let slow = |why: String| (Bucket::MaySlow, why);
    let Some(min) = min else {
        return slow(format!(
            "Runs on the CPU here at about {est} tok/s, so it may be slow."
        ));
    };
    if budget < min {
        let tail = if mem == Memory::Gpu {
            "so part of it runs on the CPU and it may be slow"
        } else {
            "so it may be slow"
        };
        return slow(format!(
            "Needs {} of {}; this PC has {}, {tail}.",
            size_label(min),
            mem.words(),
            size_label(budget)
        ));
    }
    let floor = if mem == Memory::System { 5.0 } else { 10.0 };
    let speed = match measured {
        Some(t) if t < floor => {
            return slow(format!("Measured {t} tok/s on this PC, so it may be slow."))
        }
        Some(t) => format!("measured {t} tok/s"),
        None => format!("about {est} tok/s"),
    };
    let optional = if m.optional {
        crate::brand::filled(" Optional: {product} never picks it for you.")
    } else {
        ""
    };
    (
        Bucket::Recommended,
        format!(
            "Fits in {} of {}, {speed}.{optional}",
            size_label(budget),
            mem.words()
        ),
    )
}

/// The tier default for `p`, scored without a speed test.
pub fn preselected(p: &Profile) -> Option<&'static LocalModel> {
    score(p, local_models(), &BenchCache::new())
        .into_iter()
        .find(|x| x.preselected)
        .map(|x| x.model)
}

/// `T0` (cloud only) to `T5`: what this machine can run, from the biggest
/// model the app may pick that runs well here (not the default, which
/// favours speed).
pub fn tier(p: &Profile) -> &'static str {
    let biggest = score(p, local_models(), &BenchCache::new())
        .into_iter()
        .find(|x| x.bucket == Bucket::Recommended && !x.model.optional)
        .map(|x| x.model);
    match biggest {
        None => "T0",
        Some(m) if m.tier == "T4" && p.budget().1 > T5_BUDGET_MB => "T5",
        Some(m) => m.tier,
    }
}

/// The smallest model to chat with: what the wizard quotes when nothing fits
/// (never an optional one, since nothing picks those for you).
pub fn smallest() -> Option<&'static LocalModel> {
    local_models()
        .iter()
        .rev()
        .find(|m| m.is_chat() && !m.optional)
}

#[cfg(test)]
#[path = "local_picker_tests.rs"]
mod tests;
