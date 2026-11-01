//! Offline card PR C: hardware-aware llama-server defaults and idle unload.
//!
//! Pure from [`Hardware`](grokhub_core::local_setup::Hardware): GPU layers,
//! thread count, context size, flash-attn / KV q8 flags, and the idle unload
//! timeout (default 10 minutes). Live defaults can be stored for the sidecar.

use std::sync::{Mutex, OnceLock};

use grokhub_core::local_setup::Hardware;

/// Default idle unload: free the model after this many seconds with no calls.
pub const IDLE_UNLOAD_SECS: u64 = 600;

/// Runtime knobs passed into `llama-server` / context caps.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeDefaults {
    /// `-ngl`: all layers when VRAM fits the model, else 0 (CPU).
    pub n_gpu_layers: i32,
    /// `--threads`: physical-core estimate (half of reported logical when unknown).
    pub threads: u32,
    /// Context tokens for the local route / `-c`.
    pub ctx: u64,
    /// `--flash-attn` when a discrete GPU is present.
    pub flash_attn: bool,
    /// `--cache-type-k` / `--cache-type-v` q8_0 when memory allows.
    pub kv_cache_q8: bool,
    /// Seconds of idle before unload ([`IDLE_UNLOAD_SECS`] by default).
    pub idle_unload_secs: u64,
}

impl Default for RuntimeDefaults {
    fn default() -> Self {
        Self {
            n_gpu_layers: 0,
            threads: 2,
            ctx: 4_096,
            flash_attn: false,
            kv_cache_q8: false,
            idle_unload_secs: IDLE_UNLOAD_SECS,
        }
    }
}

fn stored() -> &'static Mutex<RuntimeDefaults> {
    static S: OnceLock<Mutex<RuntimeDefaults>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(RuntimeDefaults::default()))
}

/// Remember defaults from a hardware probe (wizard / Settings).
pub fn set_defaults(d: RuntimeDefaults) {
    *stored().lock().expect("local_perf defaults") = d;
}

/// Settings → Local AI → "Keep the model loaded": `None` restores the
/// default idle unload, `Some(secs)` the given one (`u64::MAX` never unloads).
/// A hardware probe after this keeps the choice.
pub fn set_idle_unload_secs(secs: Option<u64>) {
    let mut d = stored().lock().expect("local_perf defaults");
    d.idle_unload_secs = secs.unwrap_or(IDLE_UNLOAD_SECS);
    IDLE_OVERRIDE.store(secs.unwrap_or(0), std::sync::atomic::Ordering::SeqCst);
}

static IDLE_OVERRIDE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

fn idle_override() -> Option<u64> {
    match IDLE_OVERRIDE.load(std::sync::atomic::Ordering::SeqCst) {
        0 => None,
        s => Some(s),
    }
}

/// Current stored defaults (CPU-safe floor until a probe runs).
pub fn current_defaults() -> RuntimeDefaults {
    stored().lock().expect("local_perf defaults").clone()
}

/// Context cap from available memory (raises the fixed 4k floor when RAM/VRAM allow).
///
/// With enough discrete VRAM for GPU layers, size from VRAM (KV lives on the GPU).
/// Otherwise size from system RAM (CPU / unified-memory machines).
pub fn ctx_for(hw: &Hardware) -> u64 {
    let mem = if hw.vram_mb >= 3_072 {
        hw.vram_mb
    } else {
        hw.ram_mb.max(hw.vram_mb)
    };
    if mem >= 16_384 {
        16_384
    } else if mem >= 8_192 {
        8_192
    } else {
        4_096
    }
}

fn threads_from_ram(ram_mb: u64) -> u32 {
    if ram_mb >= 32_768 {
        16
    } else if ram_mb >= 16_384 {
        8
    } else if ram_mb >= 8_192 {
        4
    } else {
        2
    }
}

/// Best-effort physical core count (unique Linux core_ids, else logical CPUs).
pub fn physical_cores() -> Option<u32> {
    #[cfg(target_os = "linux")]
    {
        use std::collections::BTreeSet;
        let mut cores = BTreeSet::new();
        if let Ok(entries) = std::fs::read_dir("/sys/devices/system/cpu") {
            for e in entries.flatten() {
                let name = e.file_name();
                let name = name.to_string_lossy();
                if !name.starts_with("cpu") || !name[3..].chars().all(|c| c.is_ascii_digit()) {
                    continue;
                }
                let path = e.path().join("topology/core_id");
                if let Ok(s) = std::fs::read_to_string(path) {
                    if let Ok(id) = s.trim().parse::<u32>() {
                        cores.insert(id);
                    }
                }
            }
        }
        if !cores.is_empty() {
            return Some(cores.len() as u32);
        }
    }
    std::thread::available_parallelism()
        .ok()
        .map(|n| (n.get() as u32).max(1))
}

/// Defaults for this machine. Fixture-friendly when `cores` is `None` (RAM stand-in).
pub fn defaults_for(hw: &Hardware) -> RuntimeDefaults {
    defaults_for_with_cores(hw, None)
}

/// Like [`defaults_for`], but uses a real core count when provided.
pub fn defaults_for_with_cores(hw: &Hardware, cores: Option<u32>) -> RuntimeDefaults {
    let has_gpu = hw.vram_mb >= 2_048 || !hw.gpu.trim().is_empty();
    let n_gpu_layers = if hw.vram_mb >= 3_072 { 99 } else { 0 };
    let threads = cores
        .filter(|c| *c > 0)
        .unwrap_or_else(|| threads_from_ram(hw.ram_mb))
        .max(1);
    let ctx = ctx_for(hw);
    RuntimeDefaults {
        n_gpu_layers,
        threads,
        ctx,
        flash_attn: has_gpu && n_gpu_layers > 0,
        kv_cache_q8: hw.vram_mb.max(hw.ram_mb) >= 8_192,
        idle_unload_secs: IDLE_UNLOAD_SECS,
    }
}

/// Extra `llama-server` flags from [`RuntimeDefaults`] (after host/port/key).
pub fn llama_perf_args(d: &RuntimeDefaults) -> Vec<String> {
    let mut args = vec![
        "-ngl".into(),
        d.n_gpu_layers.to_string(),
        "--threads".into(),
        d.threads.to_string(),
        "-c".into(),
        d.ctx.to_string(),
    ];
    if d.flash_attn {
        // The pinned engine takes a value; a bare flag swallows the next one.
        args.extend(["--flash-attn".into(), "on".into()]);
    }
    if d.kv_cache_q8 {
        args.extend([
            "--cache-type-k".into(),
            "q8_0".into(),
            "--cache-type-v".into(),
            "q8_0".into(),
        ]);
    }
    // Reuse the prompt/KV cache across turns (stable prefix / card 41 order).
    args.push("--cache-prompt".into());
    args
}

/// Whether idle unload should fire: `now_secs >= last_used_secs + idle_unload_secs`.
pub fn idle_unload_due(last_used_secs: u64, now_secs: u64, idle_unload_secs: u64) -> bool {
    now_secs.saturating_sub(last_used_secs) >= idle_unload_secs
}

/// Apply probed hardware: store defaults and raise the local context cap.
pub fn apply_hardware(hw: &Hardware) {
    let mut d = defaults_for_with_cores(hw, physical_cores());
    if let Some(secs) = idle_override() {
        d.idle_unload_secs = secs;
    }
    super::local::set_ctx_cap(d.ctx);
    set_defaults(d);
}

#[cfg(test)]
mod tests {
    use super::*;
    use grokhub_core::local_setup::Hardware;

    fn hw(gpu: &str, vram_mb: u64, ram_mb: u64) -> Hardware {
        Hardware {
            gpu: gpu.into(),
            vram_mb,
            ram_mb,
            free_disk_mb: 100_000,
        }
    }

    #[test]
    fn fixture_hardware_gets_sensible_defaults() {
        // RDNA4 16 GB
        let d = defaults_for(&hw("AMD Radeon RX 9070 XT", 16_384, 32_768));
        assert_eq!(d.n_gpu_layers, 99);
        assert_eq!(d.ctx, 16_384);
        assert!(d.flash_attn && d.kv_cache_q8);
        assert_eq!(d.idle_unload_secs, 600);
        assert_eq!(d.threads, 16);

        // NVIDIA 8 GB
        let d = defaults_for(&hw("NVIDIA GeForce RTX 3070", 8_192, 16_384));
        assert_eq!(d.n_gpu_layers, 99);
        assert_eq!(d.ctx, 8_192);
        assert!(d.flash_attn && d.kv_cache_q8);

        // 16 GB Apple-class (unified mem as RAM, no discrete VRAM)
        let d = defaults_for(&hw("Apple M2", 0, 16_384));
        assert_eq!(d.n_gpu_layers, 0);
        assert_eq!(d.ctx, 16_384);
        assert!(!d.flash_attn);
        assert!(d.kv_cache_q8);

        // CPU-only low end
        let d = defaults_for(&hw("", 0, 4_096));
        assert_eq!(d.n_gpu_layers, 0);
        assert_eq!(d.ctx, 4_096);
        assert!(!d.flash_attn && !d.kv_cache_q8);
        assert_eq!(d.threads, 2);
    }

    #[test]
    fn llama_perf_args_include_ngl_threads_ctx_and_cache_prompt() {
        let d = defaults_for(&hw("NVIDIA", 8_192, 16_384));
        let args = llama_perf_args(&d);
        assert!(args.windows(2).any(|w| w == ["-ngl", "99"]));
        assert!(args.windows(2).any(|w| w == ["--threads", "8"]));
        assert!(args.windows(2).any(|w| w == ["-c", "8192"]));
        assert!(args.windows(2).any(|w| w == ["--flash-attn", "on"]));
        assert!(args.windows(2).any(|w| w == ["--cache-type-k", "q8_0"]));
        assert!(args.iter().any(|a| a == "--cache-prompt"));
    }

    #[test]
    fn idle_unload_fires_after_timeout() {
        assert!(!idle_unload_due(100, 699, 600));
        assert!(idle_unload_due(100, 700, 600));
        assert!(idle_unload_due(100, 1_000, 600));
    }

    #[test]
    fn defaults_for_with_cores_overrides_ram_stand_in() {
        let d = defaults_for_with_cores(&hw("CPU", 0, 32_768), Some(6));
        assert_eq!(d.threads, 6);
    }

    #[test]
    fn apply_hardware_raises_ctx_cap() {
        apply_hardware(&hw("AMD", 16_384, 32_768));
        assert_eq!(super::super::local::ctx_cap(), 16_384);
        assert_eq!(current_defaults().n_gpu_layers, 99);
        // Reset for other tests in this process.
        apply_hardware(&hw("", 0, 4_096));
        assert_eq!(super::super::local::ctx_cap(), 4_096);
    }
}
