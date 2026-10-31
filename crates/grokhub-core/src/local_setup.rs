//! The onboarding wizard's local AI step: what this machine has and which
//! on-device model fits it. Pure: the cabin reads the hardware (memory, disk,
//! `nvidia-smi`) and hands the text here.

pub use crate::local_picker::CpuSimd;
use crate::local_picker::{self, Profile};
use serde::Deserialize;
use std::sync::LazyLock;

/// What the wizard found on this machine. Zero means unknown.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Hardware {
    /// The GPU's name, empty when none was found.
    pub gpu: String,
    pub vram_mb: u64,
    pub ram_mb: u64,
    /// Free space on the drive the models folder is on.
    pub free_disk_mb: u64,
}

/// Usable memory (MiB) a model needs to be Recommended on each kind of
/// memory. `None`: never Recommended there.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
pub struct Budgets {
    pub gpu: Option<u64>,
    pub unified: Option<u64>,
    pub cpu: Option<u64>,
}

/// Rough generation speed (tok/s) on each kind of memory.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
pub struct TokS {
    pub gpu: u32,
    pub unified: u32,
    pub cpu: u32,
}

/// One on-device model in `data/local_catalog.toml`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LocalModel {
    pub id: &'static str,
    pub name: &'static str,
    /// The Hugging Face repo and GGUF file it downloads from.
    pub repo: &'static str,
    pub file: &'static str,
    /// The file's pinned SHA-256 (its LFS oid).
    pub sha256: &'static str,
    pub download_mb: u64,
    pub quant: &'static str,
    pub license: &'static str,
    pub badges: &'static [&'static str],
    /// The hardware tier it is the default for (`T1`…`T4`).
    pub tier: &'static str,
    /// The router tier it serves (`route::local::TIERS`).
    pub route: &'static str,
    pub ctx_default: u32,
    /// Weights + KV at `ctx_default` + overhead, in MiB.
    pub run_mb: u64,
    pub needs_avx2: bool,
    pub min_llama_build: Option<u32>,
    /// Offered, never picked for the user.
    pub optional: bool,
    /// A speculative-decoding drafter, not a model to chat with.
    pub is_drafter: bool,
    /// The id of its drafter, when it has one.
    pub drafter: Option<&'static str>,
    pub min_budget: Budgets,
    pub tok_s: TokS,
}

#[derive(Deserialize)]
struct CatalogFile {
    model: Vec<CatalogRow>,
}

#[derive(Deserialize)]
struct CatalogRow {
    id: String,
    name: String,
    repo: String,
    file: String,
    sha256: String,
    download_mb: u64,
    quant: String,
    license: String,
    badges: Vec<String>,
    tier: String,
    route: String,
    ctx_default: u32,
    run_mb: u64,
    needs_avx2: bool,
    min_llama_build: Option<u32>,
    #[serde(default)]
    optional: bool,
    #[serde(default)]
    role: Option<String>,
    drafter: Option<String>,
    min_budget: Budgets,
    tok_s: TokS,
}

const CATALOG_TOML: &str = include_str!("../data/local_catalog.toml");

fn leak(s: String) -> &'static str {
    s.leak()
}

/// Every catalog row, biggest first. Parsed once; the strings live for the run.
/// A catalog that fails to parse lists nothing (a test pins that it parses).
pub fn local_models() -> &'static [LocalModel] {
    static MODELS: LazyLock<Vec<LocalModel>> = LazyLock::new(|| {
        toml::from_str::<CatalogFile>(CATALOG_TOML)
            .map(|f| f.model)
            .unwrap_or_default()
            .into_iter()
            .map(|r| LocalModel {
                id: leak(r.id),
                name: leak(r.name),
                repo: leak(r.repo),
                file: leak(r.file),
                sha256: leak(r.sha256),
                download_mb: r.download_mb,
                quant: leak(r.quant),
                license: leak(r.license),
                badges: r.badges.into_iter().map(leak).collect::<Vec<_>>().leak(),
                tier: leak(r.tier),
                route: leak(r.route),
                ctx_default: r.ctx_default,
                run_mb: r.run_mb,
                needs_avx2: r.needs_avx2,
                min_llama_build: r.min_llama_build,
                optional: r.optional,
                is_drafter: r.role.as_deref() == Some("drafter"),
                drafter: r.drafter.map(leak),
                min_budget: r.min_budget,
                tok_s: r.tok_s,
            })
            .collect()
    });
    &MODELS
}

/// A catalog id; an older install (a Qwen 2.5 or stock Gemma 4 id) finds
/// nothing and loads by file.
pub fn model_by_id(id: &str) -> Option<&'static LocalModel> {
    local_models().iter().find(|m| m.id == id)
}

/// Free disk kept spare after a download.
pub const DISK_HEADROOM_MB: u64 = 2_048;

/// Free disk a model needs: its download plus headroom.
pub fn disk_needed_mb(m: &LocalModel) -> u64 {
    m.download_mb + DISK_HEADROOM_MB
}

/// The tier default for this machine: the picker's pre-selected model, or none
/// (T0, cloud only).
pub fn suggest(hw: &Hardware) -> Option<&'static LocalModel> {
    suggest_for(hw, CpuSimd::detect())
}

pub fn suggest_for(hw: &Hardware, simd: CpuSimd) -> Option<&'static LocalModel> {
    local_picker::preselected(&Profile::from_hardware(hw, simd))
}

/// Under 8 GB of RAM and no 4 GB GPU: the router's downshift ladder runs one tier lower.
pub fn low_end(hw: &Hardware) -> bool {
    hw.ram_mb < 8_192 && hw.vram_mb < 4_096
}

/// `4680` → "4.7 GB"; under 1 GB in MB.
pub fn size_label(mb: u64) -> String {
    if mb < 1_024 {
        return format!("{mb} MB");
    }
    let tenths = (mb * 10 + 512) / 1_024;
    if tenths.is_multiple_of(10) {
        format!("{} GB", tenths / 10)
    } else {
        format!("{}.{} GB", tenths / 10, tenths % 10)
    }
}

/// "NVIDIA GeForce RTX 4070 (12 GB), 32 GB RAM, 220 GB free".
pub fn hardware_line(hw: &Hardware) -> String {
    let gpu = if hw.gpu.trim().is_empty() {
        "No GPU found".to_string()
    } else if hw.vram_mb > 0 {
        format!("{} ({})", hw.gpu.trim(), size_label(hw.vram_mb))
    } else {
        hw.gpu.trim().to_string()
    };
    let ram = if hw.ram_mb > 0 {
        format!("{} RAM", size_label(hw.ram_mb))
    } else {
        "RAM unknown".into()
    };
    let disk = if hw.free_disk_mb > 0 {
        format!("{} free", size_label(hw.free_disk_mb))
    } else {
        "free disk unknown".into()
    };
    format!("{gpu}, {ram}, {disk}")
}

/// What the wizard suggests, with the download size and the disk it needs.
pub fn suggestion_line(hw: &Hardware) -> String {
    suggestion_line_for(hw, CpuSimd::detect())
}

pub fn suggestion_line_for(hw: &Hardware, simd: CpuSimd) -> String {
    if simd == CpuSimd::NoAvx {
        return format!("This CPU has no AVX, so no on-device model can run here. {product} uses the cloud; you can skip this.", product = crate::brand::product_name());
    }
    match suggest_for(hw, simd) {
        Some(m) => format!(
            "{} fits this machine: a {} download that needs {} free.",
            m.name,
            size_label(m.download_mb),
            size_label(disk_needed_mb(m))
        ),
        None => match local_picker::smallest() {
            Some(small) => format!(
                "No on-device model fits this machine. The smallest needs {} of RAM and {} free. You can skip this.",
                size_label(small.min_budget.cpu.unwrap_or(0) + local_picker::SYSTEM_RESERVE_MB),
                size_label(disk_needed_mb(small))
            ),
            None => "No on-device model fits this machine. You can skip this.".into(),
        },
    }
}

/// `MemTotal:  16314252 kB` in `/proc/meminfo` → MB.
pub fn parse_meminfo(text: &str) -> Option<u64> {
    let line = text.lines().find(|l| l.starts_with("MemTotal:"))?;
    let kb: u64 = line.split_whitespace().nth(1)?.parse().ok()?;
    Some(kb / 1_024)
}

/// `nvidia-smi --query-gpu=name,memory.total --format=csv,noheader,nounits`:
/// the first GPU's name and VRAM in MB.
pub fn parse_nvidia_smi(text: &str) -> Option<(String, u64)> {
    let line = text.lines().map(str::trim).find(|l| !l.is_empty())?;
    let (name, mb) = line.rsplit_once(',')?;
    let mb: u64 = mb.trim().parse().ok()?;
    let name = name.trim();
    (!name.is_empty()).then(|| (name.to_string(), mb))
}

/// `df -Pk <dir>`: the Available column of the data row, in MB.
pub fn parse_df_avail_mb(text: &str) -> Option<u64> {
    let row = text.lines().skip(1).find(|l| !l.trim().is_empty())?;
    let kb: u64 = row.split_whitespace().nth(3)?.parse().ok()?;
    Some(kb / 1_024)
}

/// `sysctl -n hw.memsize` (bytes) → MB.
pub fn parse_memsize_bytes(text: &str) -> Option<u64> {
    text.trim().parse::<u64>().ok().map(|b| b / (1_024 * 1_024))
}

/// Linux DRM `mem_info_vram_total` (bytes as a decimal string) → MB.
///
/// Used for AMD (and other DRM) GPUs when `nvidia-smi` is missing.
pub fn parse_drm_vram_total_bytes(text: &str) -> Option<u64> {
    let bytes: u64 = text.trim().parse().ok()?;
    (bytes > 0).then_some(bytes / (1_024 * 1_024))
}

/// Best VRAM MB from several DRM `mem_info_vram_total` file bodies (one per card).
pub fn max_drm_vram_mb(files: &[&str]) -> Option<u64> {
    files
        .iter()
        .filter_map(|t| parse_drm_vram_total_bytes(t))
        .max()
}

/// Windows DXGI / CIM CSV: `Name,DedicatedVideoMemory` (bytes) → (name, MB).
///
/// First row with a positive dedicated size wins (same shape as `nvidia-smi` CSV).
pub fn parse_dxgi_dedicated_mb(text: &str) -> Option<(String, u64)> {
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.to_ascii_lowercase().starts_with("name") {
            continue;
        }
        let Some((name, mem)) = line.rsplit_once(',') else {
            continue;
        };
        let name = name.trim().trim_matches('"');
        let Ok(bytes) = mem.trim().trim_matches('"').parse::<u64>() else {
            continue;
        };
        if bytes == 0 || name.is_empty() {
            continue;
        }
        return Some((name.into(), bytes / (1_024 * 1_024)));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hw(gpu: &str, vram_mb: u64, ram_mb: u64, free_disk_mb: u64) -> Hardware {
        Hardware {
            gpu: gpu.into(),
            vram_mb,
            ram_mb,
            free_disk_mb,
        }
    }

    #[test]
    fn the_wizard_suggests_the_gemma_4_tier_default_for_its_reading() {
        let pick = |h: &Hardware| suggest_for(h, CpuSimd::Avx2).map(|m| m.name);
        assert_eq!(
            pick(&hw("NVIDIA GeForce RTX 4070", 12_282, 32_768, 225_000)),
            Some("Gemma 4 12B")
        );
        assert_eq!(pick(&hw("", 0, 16_384, 7_105)), Some("Gemma 4 E4B"));
        // 1 MB short of the E4B's download plus headroom.
        assert_eq!(pick(&hw("", 0, 16_384, 7_104)), Some("Gemma 4 E2B"));
        // A 6 GB GPU carries the E4B on a 4 GB laptop.
        assert_eq!(
            pick(&hw(
                "NVIDIA GeForce RTX 3060 Laptop GPU",
                6_144,
                4_096,
                100_000
            )),
            Some("Gemma 4 E4B")
        );
        assert_eq!(pick(&hw("", 0, 8_192, 100_000)), Some("Gemma 4 E2B"));
        // RAM minus the 4 GB the OS keeps must hold 3 GB.
        assert_eq!(pick(&hw("", 0, 7_168, 100_000)), Some("Gemma 4 E2B"));
        assert_eq!(pick(&hw("", 0, 7_167, 100_000)), None);
        assert_eq!(pick(&hw("", 0, 65_536, 5_305)), None, "no room on disk");
        assert_eq!(pick(&Hardware::default()), None, "nothing read");
        assert_eq!(
            suggest_for(&hw("", 0, 8_192, 100_000), CpuSimd::NoAvx),
            None,
            "no AVX: cloud only"
        );
        assert_eq!(
            suggest_for(&hw("", 0, 16_384, 100_000), CpuSimd::Avx2).map(|m| (m.id, m.route)),
            Some(("gemma-4-e4b-it-abliterated-q4_k", "local:small"))
        );
        assert_eq!(
            suggest_for(&hw("", 0, 8_192, 100_000), CpuSimd::Avx2).map(|m| m.route),
            Some("local:tiny")
        );
        // An older Qwen 2.5 or stock Gemma 4 install isn't in the catalog any more.
        assert_eq!(model_by_id("qwen2.5-7b-instruct-q4_k_m"), None);
        assert_eq!(model_by_id("gemma-4-e4b-it-q4_k_m"), None);
    }

    #[test]
    fn low_end_is_under_8_gb_ram_with_no_4_gb_gpu() {
        assert!(low_end(&hw("", 0, 4_096, 0)));
        assert!(low_end(&hw("Intel UHD", 2_048, 8_191, 0)));
        assert!(!low_end(&hw("", 0, 8_192, 0)));
        assert!(!low_end(&hw("NVIDIA GeForce GTX 1650", 4_096, 4_096, 0)));
    }

    #[test]
    fn sizes_hardware_and_suggestion_read_in_plain_words() {
        assert_eq!(size_label(512), "512 MB");
        assert_eq!(size_label(1_024), "1 GB");
        assert_eq!(size_label(5_089), "5 GB");
        assert_eq!(size_label(6_728), "6.6 GB");
        assert_eq!(size_label(32_768), "32 GB");
        assert_eq!(
            hardware_line(&hw("NVIDIA GeForce RTX 4070", 12_282, 32_768, 225_280)),
            "NVIDIA GeForce RTX 4070 (12 GB), 32 GB RAM, 220 GB free"
        );
        assert_eq!(
            hardware_line(&hw("", 0, 8_192, 40_960)),
            "No GPU found, 8 GB RAM, 40 GB free"
        );
        assert_eq!(
            hardware_line(&Hardware::default()),
            "No GPU found, RAM unknown, free disk unknown"
        );
        assert_eq!(
            suggestion_line_for(&hw("", 0, 32_768, 225_280), CpuSimd::Avx2),
            "Gemma 4 E4B fits this machine: a 4.9 GB download that needs 6.9 GB free."
        );
        assert_eq!(
            suggestion_line_for(&hw("", 0, 2_048, 225_280), CpuSimd::Avx2),
            "No on-device model fits this machine. The smallest needs 7 GB of RAM and 5.2 GB free. You can skip this."
        );
        assert_eq!(
            suggestion_line_for(&hw("", 0, 32_768, 225_280), CpuSimd::NoAvx),
            "This CPU has no AVX, so no on-device model can run here. Loadout uses the cloud; you can skip this."
        );
    }

    #[test]
    fn the_hardware_parsers_read_meminfo_nvidia_smi_df_and_sysctl() {
        assert_eq!(
            parse_meminfo("MemTotal:       16314252 kB\nMemFree:  1 kB\n"),
            Some(15_931)
        );
        assert_eq!(parse_meminfo("MemFree: 1 kB\n"), None);
        assert_eq!(
            parse_nvidia_smi("NVIDIA GeForce RTX 4070, 12282\nNVIDIA T400, 2048\n"),
            Some(("NVIDIA GeForce RTX 4070".into(), 12_282))
        );
        assert_eq!(parse_nvidia_smi("NVIDIA-SMI has failed\n"), None);
        assert_eq!(parse_nvidia_smi(""), None);
        let df = "Filesystem     1024-blocks      Used Available Capacity Mounted on\n/dev/nvme0n1p2  490617784 250000000 230686720      53% /\n";
        assert_eq!(parse_df_avail_mb(df), Some(225_280));
        assert_eq!(parse_df_avail_mb("Filesystem 1024-blocks\n"), None);
        assert_eq!(parse_memsize_bytes("17179869184\n"), Some(16_384));
        assert_eq!(parse_memsize_bytes("n/a"), None);
        // RDNA4-class 16 GB as DRM bytes.
        assert_eq!(parse_drm_vram_total_bytes("17179869184\n"), Some(16_384));
        assert_eq!(parse_drm_vram_total_bytes("0\n"), None);
        assert_eq!(
            max_drm_vram_mb(&["8589934592", "17179869184", "bad"]),
            Some(16_384)
        );
        assert_eq!(
            parse_dxgi_dedicated_mb(
                "Name,DedicatedVideoMemory\n\"AMD Radeon RX 9070 XT\",17179869184\n"
            ),
            Some(("AMD Radeon RX 9070 XT".into(), 16_384))
        );
        assert_eq!(
            parse_dxgi_dedicated_mb("Name,DedicatedVideoMemory\nIntel UHD,0\n"),
            None
        );
    }
}
