//! The onboarding wizard's local AI step: what this machine has and which
//! on-device model fits it. Pure: the cabin reads the hardware (memory, disk,
//! `nvidia-smi`) and hands the text here.

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

/// One on-device model the wizard can suggest.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LocalModel {
    pub id: &'static str,
    pub name: &'static str,
    pub download_mb: u64,
    /// Enough RAM to run it on the CPU.
    pub min_ram_mb: u64,
    /// Or enough VRAM to run it on the GPU.
    pub min_vram_mb: u64,
    /// The router tier it serves (`route::local::TIERS`).
    pub tier: &'static str,
}

/// Biggest first: the wizard suggests the first one that fits.
pub const LOCAL_MODELS: &[LocalModel] = &[
    LocalModel { id: "qwen2.5-7b-instruct-q4_k_m", name: "Qwen 2.5 7B", download_mb: 4_680, min_ram_mb: 16_384, min_vram_mb: 6_144, tier: "local:small" },
    LocalModel { id: "qwen2.5-3b-instruct-q4_k_m", name: "Qwen 2.5 3B", download_mb: 1_930, min_ram_mb: 8_192, min_vram_mb: 3_072, tier: "local:small" },
    LocalModel { id: "qwen2.5-1.5b-instruct-q4_k_m", name: "Qwen 2.5 1.5B", download_mb: 1_120, min_ram_mb: 4_096, min_vram_mb: 2_048, tier: "local:tiny" },
];

/// Free disk kept spare after a download.
pub const DISK_HEADROOM_MB: u64 = 2_048;

/// Free disk a model needs: its download plus headroom.
pub fn disk_needed_mb(m: &LocalModel) -> u64 {
    m.download_mb + DISK_HEADROOM_MB
}

pub fn fits(m: &LocalModel, hw: &Hardware) -> bool {
    hw.free_disk_mb >= disk_needed_mb(m) && (hw.ram_mb >= m.min_ram_mb || hw.vram_mb >= m.min_vram_mb)
}

/// The biggest model that fits, or none.
pub fn suggest(hw: &Hardware) -> Option<&'static LocalModel> {
    LOCAL_MODELS.iter().find(|m| fits(m, hw))
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
    let ram = if hw.ram_mb > 0 { format!("{} RAM", size_label(hw.ram_mb)) } else { "RAM unknown".into() };
    let disk = if hw.free_disk_mb > 0 { format!("{} free", size_label(hw.free_disk_mb)) } else { "free disk unknown".into() };
    format!("{gpu}, {ram}, {disk}")
}

/// What the wizard suggests, with the download size and the disk it needs.
pub fn suggestion_line(hw: &Hardware) -> String {
    match suggest(hw) {
        Some(m) => format!(
            "{} fits this machine: a {} download that needs {} free.",
            m.name,
            size_label(m.download_mb),
            size_label(disk_needed_mb(m))
        ),
        None => {
            let small = LOCAL_MODELS[LOCAL_MODELS.len() - 1];
            format!(
                "No on-device model fits this machine. The smallest needs {} of RAM and {} free. You can skip this.",
                size_label(small.min_ram_mb),
                size_label(disk_needed_mb(&small))
            )
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn hw(gpu: &str, vram_mb: u64, ram_mb: u64, free_disk_mb: u64) -> Hardware {
        Hardware { gpu: gpu.into(), vram_mb, ram_mb, free_disk_mb }
    }

    #[test]
    fn the_biggest_model_that_fits_ram_or_vram_and_disk_is_suggested() {
        let pick = |h: &Hardware| suggest(h).map(|m| m.name);
        assert_eq!(pick(&hw("NVIDIA GeForce RTX 4070", 12_282, 32_768, 225_000)), Some("Qwen 2.5 7B"));
        assert_eq!(pick(&hw("", 0, 16_384, 6_728)), Some("Qwen 2.5 7B"));
        // 1 MB short of the 7B's download plus headroom.
        assert_eq!(pick(&hw("", 0, 16_384, 6_727)), Some("Qwen 2.5 3B"));
        // A 6 GB GPU carries the 7B on a 4 GB laptop.
        assert_eq!(pick(&hw("NVIDIA GeForce RTX 3060 Laptop GPU", 6_144, 4_096, 100_000)), Some("Qwen 2.5 7B"));
        assert_eq!(pick(&hw("", 0, 8_192, 100_000)), Some("Qwen 2.5 3B"));
        assert_eq!(pick(&hw("", 0, 8_191, 100_000)), Some("Qwen 2.5 1.5B"));
        assert_eq!(pick(&hw("", 0, 4_096, 3_168)), Some("Qwen 2.5 1.5B"));
        assert_eq!(pick(&hw("", 0, 4_095, 100_000)), None);
        assert_eq!(pick(&hw("", 0, 65_536, 3_167)), None, "no room on disk");
        assert_eq!(pick(&Hardware::default()), None, "nothing read");
        assert_eq!(suggest(&hw("", 0, 8_192, 100_000)).map(|m| (m.id, m.tier)), Some(("qwen2.5-3b-instruct-q4_k_m", "local:small")));
        assert_eq!(suggest(&hw("", 0, 4_096, 100_000)).map(|m| m.tier), Some("local:tiny"));
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
        assert_eq!(size_label(4_680), "4.6 GB");
        assert_eq!(size_label(6_728), "6.6 GB");
        assert_eq!(size_label(32_768), "32 GB");
        assert_eq!(
            hardware_line(&hw("NVIDIA GeForce RTX 4070", 12_282, 32_768, 225_280)),
            "NVIDIA GeForce RTX 4070 (12 GB), 32 GB RAM, 220 GB free"
        );
        assert_eq!(hardware_line(&hw("", 0, 8_192, 40_960)), "No GPU found, 8 GB RAM, 40 GB free");
        assert_eq!(hardware_line(&Hardware::default()), "No GPU found, RAM unknown, free disk unknown");
        assert_eq!(
            suggestion_line(&hw("", 0, 32_768, 225_280)),
            "Qwen 2.5 7B fits this machine: a 4.6 GB download that needs 6.6 GB free."
        );
        assert_eq!(
            suggestion_line(&hw("", 0, 2_048, 225_280)),
            "No on-device model fits this machine. The smallest needs 4 GB of RAM and 3.1 GB free. You can skip this."
        );
    }

    #[test]
    fn the_hardware_parsers_read_meminfo_nvidia_smi_df_and_sysctl() {
        assert_eq!(parse_meminfo("MemTotal:       16314252 kB\nMemFree:  1 kB\n"), Some(15_931));
        assert_eq!(parse_meminfo("MemFree: 1 kB\n"), None);
        assert_eq!(parse_nvidia_smi("NVIDIA GeForce RTX 4070, 12282\nNVIDIA T400, 2048\n"), Some(("NVIDIA GeForce RTX 4070".into(), 12_282)));
        assert_eq!(parse_nvidia_smi("NVIDIA-SMI has failed\n"), None);
        assert_eq!(parse_nvidia_smi(""), None);
        let df = "Filesystem     1024-blocks      Used Available Capacity Mounted on\n/dev/nvme0n1p2  490617784 250000000 230686720      53% /\n";
        assert_eq!(parse_df_avail_mb(df), Some(225_280));
        assert_eq!(parse_df_avail_mb("Filesystem 1024-blocks\n"), None);
        assert_eq!(parse_memsize_bytes("17179869184\n"), Some(16_384));
        assert_eq!(parse_memsize_bytes("n/a"), None);
    }
}
