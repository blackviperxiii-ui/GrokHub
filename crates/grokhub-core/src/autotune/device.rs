//! What this machine is, in one plain record both apps share: the hardware
//! profile (GPUs, CPU, memory, power), what the OS says about how the person
//! likes things to look (text size, motion, contrast, dark mode), the clock and
//! language, what kind of machine it is, and a few things installed on it.
//!
//! Pure: `grokhub_agent::device` runs the readers and hands the raw text here.
//! The record is cached in `{config}/device.json` with a fingerprint, so the
//! auto setup knows when the hardware changed and it should tune again. The
//! Local models card reads the same record and [`Tier`].

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::local_profile::{GpuVendor, LocalProfile};

/// `{config}/device.json`.
pub const FILE: &str = "device.json";

/// One GPU, as the setup and the model picker need it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GpuFact {
    pub name: String,
    /// `nvidia`, `amd`, `intel`, `apple` or `other`.
    pub vendor: String,
    pub vram_mb: u64,
    pub integrated: bool,
    pub unified: bool,
}

/// What the OS says about how things should look. `None`: the OS didn't say.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OsLook {
    /// The OS text size, 100 = normal (Windows Make text bigger, GNOME Large
    /// text, KDE font DPI).
    pub text_scale_pct: Option<u16>,
    pub reduce_motion: Option<bool>,
    pub high_contrast: Option<bool>,
    pub prefers_dark: Option<bool>,
}

impl OsLook {
    /// Fill what `self` didn't read from `other` (several readers per OS).
    pub fn or(self, other: OsLook) -> OsLook {
        OsLook {
            text_scale_pct: self.text_scale_pct.or(other.text_scale_pct),
            reduce_motion: self.reduce_motion.or(other.reduce_motion),
            high_contrast: self.high_contrast.or(other.high_contrast),
            prefers_dark: self.prefers_dark.or(other.prefers_dark),
        }
    }
}

/// The screen the app opened on. The UI fills this in (egui viewport, the
/// webview's `screen`); the readers can't see it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Display {
    /// Display scale, 100 = 1x (150 on a 4K laptop at 150 %).
    pub scale_pct: u16,
    /// Width of the screen in physical pixels.
    pub width_px: u32,
    pub monitors: u8,
}

/// What kind of machine this is.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FormFactor {
    #[default]
    Desktop,
    Laptop,
    Tablet,
    Handheld,
}

impl FormFactor {
    pub fn label(self) -> &'static str {
        match self {
            FormFactor::Desktop => "desktop",
            FormFactor::Laptop => "laptop",
            FormFactor::Tablet => "tablet",
            FormFactor::Handheld => "handheld",
        }
    }

    /// It runs on a battery (so power matters).
    pub fn portable(self) -> bool {
        self != FormFactor::Desktop
    }
}

/// How much this machine can do on its own, from weakest to strongest.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Tier {
    /// 8 GB of memory and no graphics card (a Surface Pro 7): cloud first.
    #[default]
    T0,
    /// 16 GB of memory, or a small graphics card (up to 6 GB).
    T1,
    /// An 8-12 GB graphics card, or 16 GB of shared memory.
    T2,
    /// A 16 GB graphics card (RX 9070 XT, RTX 4080), or 32 GB shared.
    T3,
    /// 24 GB or more of graphics memory, or several cards.
    T4,
}

impl Tier {
    /// The tier dedicated GPUs (VRAM in MiB each) alone reach: the card's
    /// cut-offs, with several cards counting together only for T4.
    pub fn of_vram(vram_mb: &[u64]) -> Tier {
        let gb = |mb: u64| mb as f64 / 1_024.0;
        let best = gb(vram_mb.iter().copied().max().unwrap_or(0));
        let total = gb(vram_mb.iter().sum());
        if best >= 23.0 || (vram_mb.len() > 1 && total >= 23.0) {
            Tier::T4
        } else if best >= 14.0 {
            Tier::T3
        } else if best >= 7.5 {
            Tier::T2
        } else if best >= 3.5 {
            Tier::T1
        } else {
            Tier::T0
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Tier::T0 => "T0",
            Tier::T1 => "T1",
            Tier::T2 => "T2",
            Tier::T3 => "T3",
            Tier::T4 => "T4",
        }
    }

    /// What this tier means for answers on this PC, honestly.
    pub fn plain(self) -> &'static str {
        match self {
            Tier::T0 => "This PC is light, so answers come from the cloud. Offline answers are small and slow here.",
            Tier::T1 => "This PC can run a small model on its own for background jobs. Online answers are best.",
            Tier::T2 => "This PC can run a good model on its own for background jobs and simple chats.",
            Tier::T3 => "This PC can run a strong model on its own, fast enough for most chats.",
            Tier::T4 => "This PC can run the biggest on-device models.",
        }
    }
}

/// The machine, as the auto setup sees it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct DeviceProfile {
    /// `linux`, `windows`, `macos`.
    pub os: String,
    pub arch: String,
    /// The maker's model name (`Surface Pro 7`, `Alienware m18 R2`), when the
    /// OS says.
    pub model: String,
    pub gpus: Vec<GpuFact>,
    pub ram_mb: u64,
    pub cpu_logical: u32,
    pub cpu_physical: u32,
    pub battery: bool,
    pub on_ac: bool,
    pub battery_pct: Option<u8>,
    pub power_saver: bool,
    pub free_disk_mb: u64,
    pub look: OsLook,
    /// `en_US`, `de_DE`; empty when unknown.
    pub locale: String,
    /// The OS shows 12-hour times.
    pub clock_12h: bool,
    /// Steam is installed.
    pub steam: bool,
    /// A folder of git projects is in the home folder.
    pub dev_folders: bool,
    /// A microphone is plugged in. `None`: couldn't tell.
    pub mic: Option<bool>,
    pub display: Option<Display>,
    pub read_at_ms: u64,
}

fn vendor_name(v: GpuVendor) -> &'static str {
    match v {
        GpuVendor::Nvidia => "nvidia",
        GpuVendor::Amd => "amd",
        GpuVendor::Intel => "intel",
        GpuVendor::Apple => "apple",
        GpuVendor::Other => "other",
    }
}

impl DeviceProfile {
    /// The hardware half from the local-model profile; the rest stays default
    /// for the readers to fill in.
    pub fn from_local(p: &LocalProfile) -> DeviceProfile {
        DeviceProfile {
            os: p.os.to_string(),
            arch: p.arch.to_string(),
            gpus: p
                .gpus
                .iter()
                .map(|g| GpuFact {
                    name: g.name.clone(),
                    vendor: vendor_name(g.vendor).into(),
                    vram_mb: g.vram_mb,
                    integrated: g.integrated,
                    unified: g.unified,
                })
                .collect(),
            ram_mb: p.ram_mb,
            cpu_logical: p.cpu.logical,
            cpu_physical: p.cpu.physical,
            battery: p.power.battery,
            on_ac: p.power.on_ac,
            battery_pct: p.power.percent,
            power_saver: p.power.power_saver,
            free_disk_mb: p.free_disk_mb,
            ..DeviceProfile::default()
        }
    }

    /// Graphics cards with their own memory.
    pub fn discrete(&self) -> impl Iterator<Item = &GpuFact> {
        self.gpus.iter().filter(|g| !g.integrated && !g.unified)
    }

    /// The biggest discrete card's memory, 0 without one.
    pub fn best_vram_mb(&self) -> u64 {
        self.discrete().map(|g| g.vram_mb).max().unwrap_or(0)
    }

    pub fn has_dgpu(&self) -> bool {
        self.best_vram_mb() >= 2_048
    }

    /// What this machine can run. Cards report a little under their size
    /// (a 16 GB card reads 16_368 MB, 16 GB of RAM reads about 15_800 MB).
    pub fn tier(&self) -> Tier {
        let gb = |mb: u64| mb as f64 / 1_024.0;
        let dgpus: Vec<u64> = self.discrete().map(|g| g.vram_mb).collect();
        let ram = gb(self.ram_mb);
        if self.gpus.iter().any(|g| g.unified) {
            return if ram >= 60.0 {
                Tier::T4
            } else if ram >= 30.0 {
                Tier::T3
            } else if ram >= 15.0 {
                Tier::T2
            } else if ram >= 7.5 {
                Tier::T1
            } else {
                Tier::T0
            };
        }
        match Tier::of_vram(&dgpus) {
            Tier::T0 if ram >= 15.0 => Tier::T1,
            tier => tier,
        }
    }

    /// Too light for background extras (the router's low-end rung): under
    /// 8 GB of RAM and no 4 GB graphics card.
    pub fn low_end(&self) -> bool {
        self.ram_mb < 8_192 && self.best_vram_mb() < 4_096
    }

    /// On battery and running low, or in battery saver.
    pub fn battery_low(&self) -> bool {
        self.battery
            && !self.on_ac
            && (self.power_saver || self.battery_pct.is_some_and(|p| p <= 30))
    }

    /// Laptop, tablet or handheld from the model name and the battery.
    pub fn form_factor(&self) -> FormFactor {
        form_factor(&self.model, &self.gpus, self.battery)
    }

    /// A stable id for the hardware: GPUs and their memory, RAM and cores,
    /// rounded so a reading a few MB off is the same machine. A new value
    /// means new hardware, so the setup tunes again.
    pub fn fingerprint(&self) -> String {
        let mut gpus: Vec<String> = self
            .gpus
            .iter()
            .map(|g| {
                format!(
                    "{}:{}",
                    g.name.trim().to_ascii_lowercase(),
                    (g.vram_mb + 512) / 1_024
                )
            })
            .collect();
        gpus.sort();
        let key = format!(
            "{}|{}|{}|{}|{}",
            self.os,
            gpus.join(","),
            (self.ram_mb + 512) / 1_024,
            self.cpu_logical,
            self.model.trim().to_ascii_lowercase()
        );
        format!("{:016x}", fnv1a(key.as_bytes()))
    }

    /// One line for Settings: "Desktop, Radeon RX 9070 XT (16 GB), 64 GB RAM, T3".
    pub fn summary_line(&self) -> String {
        let form = self.form_factor().label();
        let mut form_cap = form.to_string();
        if let Some(c) = form_cap.get_mut(0..1) {
            c.make_ascii_uppercase();
        }
        let gpu = self
            .discrete()
            .max_by_key(|g| g.vram_mb)
            .map(|g| format!("{} ({} GB)", g.name, (g.vram_mb + 512) / 1_024))
            .or_else(|| self.gpus.first().map(|g| g.name.clone()))
            .unwrap_or_else(|| "no graphics card found".into());
        format!(
            "{form_cap}, {gpu}, {} GB RAM, {}",
            (self.ram_mb + 512) / 1_024,
            self.tier().label()
        )
    }
}

/// FNV-1a 64: stable across builds and platforms (std's hasher isn't).
pub fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

/// Model names of tablets and handhelds. Anything else with a battery is a
/// laptop.
pub fn form_factor(model: &str, gpus: &[GpuFact], battery: bool) -> FormFactor {
    let m = model.to_ascii_lowercase();
    const HANDHELD: &[&str] = &[
        "steam deck",
        "jupiter",
        "galileo",
        "rog ally",
        "rc71l",
        "rc72l",
        "legion go",
        "claw",
        "ayaneo",
        "gpd win",
        "onexplayer",
    ];
    const TABLET: &[&str] = &["surface pro", "surface go", "tablet", "ipad"];
    let gpu_handheld = gpus.iter().any(|g| {
        let n = g.name.to_ascii_lowercase();
        n.contains("van gogh") || n.contains("sephiroth") || n.contains("z1 extreme")
    });
    if HANDHELD.iter().any(|h| m.contains(h)) || gpu_handheld {
        FormFactor::Handheld
    } else if TABLET.iter().any(|t| m.contains(t)) {
        FormFactor::Tablet
    } else if battery {
        FormFactor::Laptop
    } else {
        FormFactor::Desktop
    }
}

/// `{config}/device.json`.
pub fn path(config_dir: &Path) -> PathBuf {
    config_dir.join(FILE)
}

/// The last reading, if any.
pub fn load(config_dir: &Path) -> Option<DeviceProfile> {
    let raw = std::fs::read_to_string(path(config_dir)).ok()?;
    serde_json::from_str(&raw).ok()
}

pub fn save(config_dir: &Path, p: &DeviceProfile) -> Result<(), String> {
    let raw = serde_json::to_string_pretty(p).map_err(|e| e.to_string())?;
    crate::pulse_writer::write_atomic(&path(config_dir), &raw)
}

/// Locales that show 12-hour times by default, by language and country.
pub fn locale_uses_12h(locale: &str) -> bool {
    let l = locale
        .split(['.', '@'])
        .next()
        .unwrap_or("")
        .replace('-', "_");
    const TWELVE: &[&str] = &[
        "en_US", "en_CA", "en_AU", "en_NZ", "en_IN", "en_PH", "en_PK", "en_EG", "hi_IN", "bn_BD",
        "bn_IN", "ur_PK", "ar_SA", "ar_EG", "ko_KR", "fil_PH", "es_MX", "es_US", "es_CO",
    ];
    TWELVE.iter().any(|t| l.eq_ignore_ascii_case(t))
        || (l.len() == 2 && l.eq_ignore_ascii_case("en"))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    fn gpu(name: &str, vram_mb: u64, integrated: bool) -> GpuFact {
        GpuFact {
            name: name.into(),
            vendor: "amd".into(),
            vram_mb,
            integrated,
            unified: false,
        }
    }

    /// Viper's machines and the ones between them.
    pub(crate) fn surface_pro_7() -> DeviceProfile {
        DeviceProfile {
            os: "windows".into(),
            model: "Surface Pro 7".into(),
            gpus: vec![gpu("Intel(R) Iris(R) Plus Graphics", 128, true)],
            ram_mb: 7_936,
            cpu_logical: 8,
            cpu_physical: 4,
            battery: true,
            on_ac: false,
            battery_pct: Some(64),
            ..DeviceProfile::default()
        }
    }

    pub(crate) fn beast() -> DeviceProfile {
        DeviceProfile {
            os: "linux".into(),
            gpus: vec![
                gpu("AMD Radeon RX 9070 XT", 16_368, false),
                gpu("AMD Radeon Graphics", 512, true),
            ],
            ram_mb: 63_400,
            cpu_logical: 32,
            cpu_physical: 16,
            on_ac: true,
            steam: true,
            ..DeviceProfile::default()
        }
    }

    #[test]
    fn tiers_match_the_machines_on_the_card() {
        assert_eq!(surface_pro_7().tier(), Tier::T0);
        assert_eq!(beast().tier(), Tier::T3);
        let laptop16 = DeviceProfile {
            gpus: vec![gpu("Intel Iris Xe", 128, true)],
            ram_mb: 15_800,
            ..DeviceProfile::default()
        };
        assert_eq!(laptop16.tier(), Tier::T1);
        let rtx4070 = DeviceProfile {
            gpus: vec![gpu("NVIDIA GeForce RTX 4070", 12_282, false)],
            ram_mb: 32_000,
            ..DeviceProfile::default()
        };
        assert_eq!(rtx4070.tier(), Tier::T2);
        let rtx4090 = DeviceProfile {
            gpus: vec![gpu("NVIDIA GeForce RTX 4090", 24_564, false)],
            ram_mb: 64_000,
            ..DeviceProfile::default()
        };
        assert_eq!(rtx4090.tier(), Tier::T4);
        let two_cards = DeviceProfile {
            gpus: vec![
                gpu("RTX 3060", 12_288, false),
                gpu("RTX 3060", 12_288, false),
            ],
            ram_mb: 32_000,
            ..DeviceProfile::default()
        };
        assert_eq!(two_cards.tier(), Tier::T4);
        let m2 = DeviceProfile {
            gpus: vec![GpuFact {
                name: "Apple GPU".into(),
                unified: true,
                vram_mb: 11_000,
                ..GpuFact::default()
            }],
            ram_mb: 16_384,
            ..DeviceProfile::default()
        };
        assert_eq!(m2.tier(), Tier::T2);
    }

    #[test]
    fn low_end_and_battery_low_read_the_way_the_router_needs() {
        let sp7 = surface_pro_7();
        assert!(sp7.low_end());
        assert!(!sp7.battery_low(), "64 % on battery isn't low");
        let low = DeviceProfile {
            battery_pct: Some(22),
            ..surface_pro_7()
        };
        assert!(low.battery_low());
        let saver = DeviceProfile {
            power_saver: true,
            ..surface_pro_7()
        };
        assert!(saver.battery_low());
        let plugged = DeviceProfile {
            on_ac: true,
            battery_pct: Some(10),
            ..surface_pro_7()
        };
        assert!(!plugged.battery_low(), "plugged in is never battery low");
        assert!(!beast().low_end());
        assert!(!beast().battery_low());
    }

    #[test]
    fn form_factor_reads_model_names_then_the_battery() {
        assert_eq!(surface_pro_7().form_factor(), FormFactor::Tablet);
        assert_eq!(beast().form_factor(), FormFactor::Desktop);
        assert_eq!(form_factor("Jupiter", &[], true), FormFactor::Handheld);
        assert_eq!(
            form_factor("ROG Ally RC71L", &[], true),
            FormFactor::Handheld
        );
        assert_eq!(
            form_factor("Alienware m18 R2", &[], true),
            FormFactor::Laptop
        );
        assert_eq!(
            form_factor(
                "",
                &[gpu("AMD Custom GPU 0405 (Van Gogh)", 1_024, true)],
                true
            ),
            FormFactor::Handheld
        );
        assert!(FormFactor::Laptop.portable());
        assert!(!FormFactor::Desktop.portable());
    }

    #[test]
    fn the_fingerprint_ignores_small_drifts_and_changes_with_hardware() {
        let a = beast();
        let drift = DeviceProfile {
            ram_mb: 63_380,
            free_disk_mb: 9_999,
            battery_pct: Some(3),
            ..beast()
        };
        assert_eq!(a.fingerprint(), drift.fingerprint());
        assert_eq!(a.fingerprint().len(), 16);
        let mut upgraded = beast();
        upgraded.gpus[0] = gpu("NVIDIA GeForce RTX 5090", 32_607, false);
        assert_ne!(a.fingerprint(), upgraded.fingerprint());
        let more_ram = DeviceProfile {
            ram_mb: 126_000,
            ..beast()
        };
        assert_ne!(a.fingerprint(), more_ram.fingerprint());
        // FNV-1a test vector: "a" -> af63dc4c8601ec8c.
        assert_eq!(fnv1a(b"a"), 0xaf63_dc4c_8601_ec8c);
    }

    #[test]
    fn summary_line_names_the_card_ram_and_tier() {
        assert_eq!(
            beast().summary_line(),
            "Desktop, AMD Radeon RX 9070 XT (16 GB), 62 GB RAM, T3"
        );
        assert_eq!(
            surface_pro_7().summary_line(),
            "Tablet, Intel(R) Iris(R) Plus Graphics, 8 GB RAM, T0"
        );
    }

    #[test]
    fn it_saves_and_loads_from_device_json() {
        let dir = std::env::temp_dir().join(format!("grokhub-device-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(load(&dir), None);
        let mut p = beast();
        p.look.text_scale_pct = Some(125);
        p.display = Some(Display {
            scale_pct: 150,
            width_px: 3_840,
            monitors: 2,
        });
        save(&dir, &p).expect("save");
        let raw = std::fs::read_to_string(dir.join("device.json")).expect("file");
        assert!(raw.contains("\"textScalePct\": 125"), "{raw}");
        assert_eq!(load(&dir), Some(p));
        // An older file with fewer fields still loads.
        std::fs::write(dir.join("device.json"), r#"{"os":"linux","ramMb":8000}"#).expect("w");
        let old = load(&dir).expect("old");
        assert_eq!(
            (old.os.as_str(), old.ram_mb, old.steam),
            ("linux", 8_000, false)
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn twelve_hour_locales() {
        assert!(locale_uses_12h("en_US.UTF-8"));
        assert!(locale_uses_12h("en-US"));
        assert!(locale_uses_12h("en"));
        assert!(!locale_uses_12h("en_GB.UTF-8"));
        assert!(!locale_uses_12h("de_DE"));
        assert!(!locale_uses_12h(""));
    }

    #[test]
    fn the_os_look_fills_gaps_from_a_second_reader() {
        let a = OsLook {
            text_scale_pct: Some(150),
            ..OsLook::default()
        };
        let b = OsLook {
            text_scale_pct: Some(100),
            prefers_dark: Some(true),
            ..OsLook::default()
        };
        assert_eq!(
            a.or(b),
            OsLook {
                text_scale_pct: Some(150),
                reduce_motion: None,
                high_contrast: None,
                prefers_dark: Some(true),
            }
        );
    }
}
