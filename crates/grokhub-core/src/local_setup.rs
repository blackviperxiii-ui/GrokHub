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

/// What a catalog model is for. Only [`ModelKind::Chat`] counts toward the
/// [`STANDARD_MODEL_CAP`]; every other kind is a specialty model.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ModelKind {
    /// A model you chat with.
    #[default]
    Chat,
    /// A speculative-decoding draft model for one chat model.
    Drafter,
    /// The image reader a chat model needs to see pictures.
    VisionProjector,
    /// Makes pictures.
    Image,
    /// Checks pictures and picture asks before anything is shown.
    ImageGuard,
    /// Turns speech into text.
    SpeechToText,
    /// Reads text aloud.
    TextToSpeech,
    /// Turns text into search vectors.
    Embed,
    /// Re-orders search results.
    Rerank,
    /// Reads text out of pictures and scans.
    Ocr,
    /// A small classifier the app's own checks run.
    Guard,
}

/// The tab a kind lists under in Download models.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ModelTab {
    Chat,
    Drafts,
    Image,
    Voice,
    Other,
}

impl ModelTab {
    pub const ALL: [ModelTab; 5] = [
        ModelTab::Chat,
        ModelTab::Drafts,
        ModelTab::Image,
        ModelTab::Voice,
        ModelTab::Other,
    ];

    pub fn label(self) -> &'static str {
        match self {
            ModelTab::Chat => "Chat",
            ModelTab::Drafts => "Drafts",
            ModelTab::Image => "Image",
            ModelTab::Voice => "Voice",
            ModelTab::Other => "Other",
        }
    }
}

impl ModelKind {
    pub fn tab(self) -> ModelTab {
        match self {
            ModelKind::Chat => ModelTab::Chat,
            ModelKind::Drafter => ModelTab::Drafts,
            ModelKind::Image | ModelKind::ImageGuard => ModelTab::Image,
            ModelKind::SpeechToText | ModelKind::TextToSpeech => ModelTab::Voice,
            ModelKind::VisionProjector
            | ModelKind::Embed
            | ModelKind::Rerank
            | ModelKind::Ocr
            | ModelKind::Guard => ModelTab::Other,
        }
    }
}

/// The most standard (chat) models the catalog lists. Specialty kinds don't
/// count.
pub const STANDARD_MODEL_CAP: usize = 25;

/// Who removed the model's refusals, and how.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Abliteration {
    /// The publisher of the uncensored build (`huihui-ai`, `llmfan46`).
    pub by: &'static str,
    /// `abliteration` or `heretic`.
    pub method: &'static str,
}

/// How a downloaded file is checked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileCheck {
    /// The Hugging Face LFS oid, lowercase hex.
    Sha256(&'static str),
    /// The git blob oid (SHA-1 of `blob <len>\0` and the bytes): a small
    /// file kept in git, outside LFS, has no SHA-256 in the listing.
    GitOid(&'static str),
}

/// A further file a model needs besides its main `file`: a speech model's
/// decoder and token list, or an image model's text encoder and VAE.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModelPart {
    pub repo: &'static str,
    /// The name in the repo.
    pub remote_file: &'static str,
    /// The path on disk, under the model's folder.
    pub file: &'static str,
    pub check: FileCheck,
    /// Exact size, for progress across the parts.
    pub bytes: u64,
}

/// One on-device model in `data/local_catalog.toml`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LocalModel {
    pub id: &'static str,
    pub name: &'static str,
    /// The Hugging Face repo and GGUF file it downloads from.
    pub repo: &'static str,
    pub file: &'static str,
    /// The name in the repo when it isn't `file` (see [`LocalModel::remote_name`]).
    pub remote_file: Option<&'static str>,
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
    /// What the model is for.
    pub kind: ModelKind,
    /// For a drafter or vision projector: the catalog id it works with.
    pub pairs_with: Option<&'static str>,
    /// Set on every uncensored build; a chat model without it can't ship.
    pub abliterated: Option<Abliteration>,
    /// Share (0-100) of the refusal check's prompts it answered, once measured.
    pub refusal_score: Option<u8>,
    /// The id of its drafter, when it has one.
    pub drafter: Option<&'static str>,
    pub min_budget: Budgets,
    pub tok_s: TokS,
    /// Files it needs besides `file`, downloaded after it; empty for a GGUF.
    pub parts: &'static [ModelPart],
    /// What it is good at (`local_skills`); unset scores are worked out.
    pub skills: crate::local_skills::Skills,
    /// What the catalog knows of it before it runs here.
    pub profile: LocalProfile,
}

/// A chat model's built-in profile, derived by `.github/scripts/local_profiles.py`
/// from the catalog row. Every figure is an estimate until measured (the row's
/// `estimate` list says which); the speed test replaces any tok/s here.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LocalProfile {
    /// The context window it was trained for, in tokens, known before download.
    pub ctx_max: Option<u32>,
    /// Generation speed on each hardware tier's reference GPU, where it fits.
    pub tok_s_tier: TierSpeeds,
    /// Light enough to run beside a game and good enough to be worth it: the
    /// gaming swap's pick.
    pub gaming_safe: bool,
}

/// Estimated tok/s per hardware tier (`autotune::device::Tier`); `None` where
/// the model doesn't fit that tier's VRAM.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct TierSpeeds {
    #[serde(rename = "T1")]
    pub t1: Option<u32>,
    #[serde(rename = "T2")]
    pub t2: Option<u32>,
    #[serde(rename = "T3")]
    pub t3: Option<u32>,
    #[serde(rename = "T4")]
    pub t4: Option<u32>,
}

impl TierSpeeds {
    pub fn for_tier(&self, tier: crate::autotune::device::Tier) -> Option<u32> {
        use crate::autotune::device::Tier;
        match tier {
            Tier::T0 => None,
            Tier::T1 => self.t1,
            Tier::T2 => self.t2,
            Tier::T3 => self.t3,
            Tier::T4 => self.t4,
        }
    }
}

#[derive(Deserialize)]
struct CatalogFile {
    /// Raised with every catalog change, so a newer signed catalog from a
    /// release can replace the one built in (`catalog_update`).
    #[serde(default)]
    version: u32,
    model: Vec<CatalogRow>,
}

#[derive(Deserialize)]
struct CatalogRow {
    id: String,
    name: String,
    repo: String,
    file: String,
    #[serde(default)]
    remote_file: Option<String>,
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
    #[serde(default)]
    kind: Option<ModelKind>,
    #[serde(default)]
    pairs_with: Option<String>,
    #[serde(default)]
    abliterated: Option<AbliterationRow>,
    #[serde(default)]
    refusal_score: Option<u8>,
    drafter: Option<String>,
    min_budget: Budgets,
    tok_s: TokS,
    #[serde(default)]
    ctx_max: Option<u32>,
    #[serde(default)]
    tok_s_tier: TierSpeeds,
    #[serde(default)]
    gaming_safe: bool,
    #[serde(default)]
    parts: Vec<PartRow>,
    #[serde(default)]
    skills: crate::local_skills::Skills,
}

/// `[[model.parts]]`: one of `sha256` (LFS) or `git_oid` (a small git file).
#[derive(Deserialize)]
struct PartRow {
    #[serde(default)]
    repo: Option<String>,
    remote_file: String,
    file: String,
    #[serde(default)]
    sha256: Option<String>,
    #[serde(default)]
    git_oid: Option<String>,
    bytes: u64,
}

#[derive(Deserialize)]
struct AbliterationRow {
    by: String,
    method: String,
}

const CATALOG_TOML: &str = include_str!("../data/local_catalog.toml");

fn leak(s: String) -> &'static str {
    s.leak()
}

fn to_model(r: CatalogRow) -> Option<LocalModel> {
    // The older `role = "drafter"` still marks a drafter.
    let kind = r.kind.unwrap_or(if r.role.as_deref() == Some("drafter") {
        ModelKind::Drafter
    } else {
        ModelKind::Chat
    });
    let repo = leak(r.repo);
    // A part with neither check, or both, would download unchecked:
    // the row is dropped (a test pins that every row loads).
    let parts = r
        .parts
        .into_iter()
        .map(|p| {
            let check = match (p.sha256, p.git_oid) {
                (Some(h), None) => FileCheck::Sha256(leak(h)),
                (None, Some(o)) => FileCheck::GitOid(leak(o)),
                _ => return None,
            };
            Some(ModelPart {
                repo: p.repo.map(leak).unwrap_or(repo),
                remote_file: leak(p.remote_file),
                file: leak(p.file),
                check,
                bytes: p.bytes,
            })
        })
        .collect::<Option<Vec<_>>>()?
        .leak();
    Some(LocalModel {
        id: leak(r.id),
        name: leak(r.name),
        repo,
        file: leak(r.file),
        remote_file: r.remote_file.map(leak),
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
        is_drafter: kind == ModelKind::Drafter,
        kind,
        pairs_with: r.pairs_with.map(leak),
        abliterated: r.abliterated.map(|a| Abliteration {
            by: leak(a.by),
            method: leak(a.method),
        }),
        refusal_score: r.refusal_score,
        drafter: r.drafter.map(leak),
        min_budget: r.min_budget,
        tok_s: r.tok_s,
        profile: LocalProfile {
            ctx_max: r.ctx_max,
            tok_s_tier: r.tok_s_tier,
            gaming_safe: r.gaming_safe,
        },
        parts,
        skills: r.skills,
    })
}

/// A whole catalog: its version and every row, or `None` when it doesn't
/// parse, any row doesn't load, or it has no default chat model. A catalog
/// from outside the app is used only whole.
pub(crate) fn parse_catalog(text: &str) -> Option<(u32, Vec<LocalModel>)> {
    let file = toml::from_str::<CatalogFile>(text).ok()?;
    let version = file.version;
    let rows = file
        .model
        .into_iter()
        .map(to_model)
        .collect::<Option<Vec<_>>>()?;
    rows.iter()
        .any(|m| m.is_chat() && !m.optional)
        .then_some((version, rows))
}

/// The version of the catalog built into this app.
pub fn bundled_version() -> u32 {
    parse_catalog(CATALOG_TOML).map_or(0, |(v, _)| v)
}

/// Every catalog row, biggest first. Parsed once; the strings live for the
/// run. A newer signed catalog saved from a release wins over the built-in
/// one; anything else uses the built-in one (a test pins that it parses).
pub fn local_models() -> &'static [LocalModel] {
    static MODELS: LazyLock<Vec<LocalModel>> = LazyLock::new(|| {
        crate::catalog_update::saved(&crate::paths::config_dir())
            .and_then(|text| parse_catalog(&text))
            .or_else(|| parse_catalog(CATALOG_TOML))
            .map(|(_, rows)| rows)
            .unwrap_or_default()
    });
    &MODELS
}

impl LocalModel {
    /// Calls tools (the catalog's `tools` badge).
    pub fn tools(&self) -> bool {
        self.badges.contains(&"tools")
    }

    /// The file's name in the Hugging Face repo: `remote_file` when set, else
    /// `file`. Several repos ship the same name, so a drafter can download
    /// under a name of its own.
    pub fn remote_name(&self) -> &'static str {
        self.remote_file.unwrap_or(self.file)
    }

    /// A model you chat with: the picker, the router and the 25 cap list
    /// only these, never a drafter, voice, image or other specialty row.
    pub fn is_chat(&self) -> bool {
        self.kind == ModelKind::Chat
    }
}

/// A catalog id; an older install (a Qwen 2.5 or stock Gemma 4 id) finds
/// nothing and loads by file.
pub fn model_by_id(id: &str) -> Option<&'static LocalModel> {
    local_models().iter().find(|m| m.id == id)
}

/// The standard models: what you chat with, the ones the 25 cap counts.
pub fn standard_models() -> impl Iterator<Item = &'static LocalModel> {
    local_models().iter().filter(|m| counts_toward_cap(m))
}

/// Only chat models use up the 25; drafters, projectors, image, voice and
/// the rest don't.
pub fn counts_toward_cap(m: &LocalModel) -> bool {
    m.is_chat()
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
    fn every_chat_model_ships_with_a_profile() {
        for m in standard_models() {
            let p = m.profile;
            let ctx = p
                .ctx_max
                .unwrap_or_else(|| panic!("{} has no ctx_max", m.id));
            assert!(
                ctx >= m.ctx_default,
                "{} trained window under its default",
                m.id
            );
            if m.run_mb <= 20_000 {
                assert!(p.tok_s_tier.t4.is_some(), "{} fits a 24 GB card", m.id);
            }
            // Speeds rise with the tier, and a tier it fits implies every bigger one.
            let tiers = [
                p.tok_s_tier.t1,
                p.tok_s_tier.t2,
                p.tok_s_tier.t3,
                p.tok_s_tier.t4,
            ];
            for pair in tiers.windows(2) {
                if let Some(low) = pair[0] {
                    assert!(
                        pair[1].is_some_and(|high| high > low),
                        "{}: {tiers:?}",
                        m.id
                    );
                }
            }
            if p.gaming_safe {
                assert!(
                    m.run_mb <= 4_608,
                    "{} is too heavy to share with a game",
                    m.id
                );
            }
        }
        assert!(
            standard_models().any(|m| m.profile.gaming_safe && !m.optional),
            "a game always has a recommended model to swap to"
        );
        assert_eq!(
            model_by_id("gemma-4-e2b-it-abliterated-q4_k").map(|m| m.tools()),
            Some(true)
        );
    }

    #[test]
    fn the_catalog_stays_inside_25_uncensored_chat_models() {
        let all = local_models();
        assert_eq!(all.len(), 39, "catalog parsed");
        let chat: Vec<_> = standard_models().map(|m| m.id).collect();
        assert_eq!(
            chat.len(),
            STANDARD_MODEL_CAP,
            "the drafters, speech, picture and guard models don't count"
        );
        // Every model that writes replies is uncensored (abliterated, or
        // Heretic's refusal-direction removal); a speech or image model only
        // reads or draws, so it has no refusals to remove.
        for m in all
            .iter()
            .filter(|m| matches!(m.kind, ModelKind::Chat | ModelKind::Drafter))
        {
            let a = m
                .abliterated
                .unwrap_or_else(|| panic!("{} must be an uncensored build", m.id));
            assert!(
                matches!(a.method, "abliteration" | "heretic"),
                "{}: {}",
                m.id,
                a.method
            );
            // No row claims vision until its projector downloads with it.
            assert!(!m.badges.contains(&"vision"), "{}", m.id);
        }
        // The defaults stay huihui-ai's Gemma 4 builds.
        for m in all.iter().filter(|m| m.is_chat() && !m.optional) {
            assert_eq!(m.abliterated.map(|a| a.by), Some("huihui-ai"), "{}", m.id);
        }
        // Chat rows run biggest first (the picker and the step-down read them
        // in order).
        let sizes: Vec<u64> = all
            .iter()
            .filter(|m| m.is_chat())
            .map(|m| m.run_mb)
            .collect();
        assert!(sizes.windows(2).all(|w| w[0] >= w[1]), "{sizes:?}");
        // Every drafter names the chat model it drafts for, and that model
        // names it back.
        for d in all.iter().filter(|m| m.kind == ModelKind::Drafter) {
            let t = model_by_id(d.pairs_with.unwrap()).unwrap();
            assert_eq!(t.drafter, Some(d.id), "{}", d.id);
        }
        // Every drafter a chat row names is in the catalog.
        for m in all.iter().filter(|m| m.is_chat()) {
            if let Some(d) = m.drafter {
                assert!(
                    model_by_id(d).is_some_and(|d| d.kind == ModelKind::Drafter),
                    "{d}"
                );
            }
        }
    }

    #[test]
    fn specialty_models_never_count_toward_the_25() {
        let mut m = *model_by_id("gemma-4-e2b-it-abliterated-q4_k").unwrap();
        assert!(counts_toward_cap(&m));
        for kind in [
            ModelKind::Drafter,
            ModelKind::VisionProjector,
            ModelKind::Image,
            ModelKind::ImageGuard,
            ModelKind::SpeechToText,
            ModelKind::TextToSpeech,
            ModelKind::Embed,
            ModelKind::Rerank,
            ModelKind::Ocr,
            ModelKind::Guard,
        ] {
            m.kind = kind;
            assert!(!counts_toward_cap(&m), "{kind:?}");
        }
        assert_eq!(ModelKind::Drafter.tab().label(), "Drafts");
        assert_eq!(ModelKind::TextToSpeech.tab().label(), "Voice");
        assert_eq!(ModelKind::ImageGuard.tab().label(), "Image");
        assert_eq!(ModelKind::Embed.tab().label(), "Other");
    }

    #[test]
    fn a_kind_row_and_the_older_role_row_both_parse() {
        let toml = r#"
[[model]]
id = "x-drafter"
name = "X drafter"
repo = "acme/x"
file = "x.gguf"
sha256 = "00"
download_mb = 1
quant = "Q4_K"
license = "Apache-2.0"
badges = []
tier = "T1"
route = "local:tiny"
ctx_default = 1
run_mb = 1
needs_avx2 = false
kind = "drafter"
pairs_with = "x-chat"
min_budget = {}
tok_s = { gpu = 1, unified = 1, cpu = 1 }
"#;
        let f: CatalogFile = toml::from_str(toml).expect("parses");
        let r = &f.model[0];
        assert_eq!(r.kind, Some(ModelKind::Drafter));
        assert_eq!(r.pairs_with.as_deref(), Some("x-chat"));
        let bad = toml.replace("kind = \"drafter\"", "kind = \"toaster\"");
        assert!(toml::from_str::<CatalogFile>(&bad).is_err());
    }

    #[test]
    fn a_part_needs_exactly_one_check_or_its_row_never_loads() {
        let row = |part: &str| {
            format!(
                r#"
[[model]]
id = "x-stt"
name = "X speech"
repo = "acme/x"
file = "x/encoder.onnx"
sha256 = "00"
download_mb = 1
quant = "int8"
license = "CC-BY-4.0"
badges = []
tier = "T1"
route = "local:voice"
ctx_default = 0
run_mb = 1
needs_avx2 = false
kind = "speech-to-text"
min_budget = {{}}
tok_s = {{ gpu = 0, unified = 0, cpu = 0 }}
[[model.parts]]
remote_file = "tokens.txt"
file = "x/tokens.txt"
bytes = 9
{part}
"#
            )
        };
        let load = |part: &str| {
            let f: CatalogFile = toml::from_str(&row(part)).expect("parses");
            f.model.into_iter().filter_map(to_model).collect::<Vec<_>>()
        };
        let m = load(r#"git_oid = "ab""#);
        assert_eq!(m.len(), 1);
        assert!(!m[0].is_chat());
        assert_eq!(
            m[0].parts,
            &[ModelPart {
                repo: "acme/x",
                remote_file: "tokens.txt",
                file: "x/tokens.txt",
                check: FileCheck::GitOid("ab"),
                bytes: 9,
            }]
        );
        assert_eq!(m[0].parts.len(), load(r#"sha256 = "cd""#)[0].parts.len());
        assert!(load("").is_empty(), "no check");
        assert!(
            load("sha256 = \"cd\"\ngit_oid = \"ab\"").is_empty(),
            "two checks"
        );
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
