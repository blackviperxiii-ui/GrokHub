//! Settings → Local AI: what Auto picks for this PC (`local_picker::score`),
//! the model files the user added, the Advanced engine tuning, what hardware
//! detection could not see, and the tray's state chip. There is no model
//! picker: Auto picks the model, and a model file the user adds loads when no
//! downloaded catalog model is there.
//!
//! Choices live in `local_ai.json` in the GrokHub config folder, the added
//! files under `ownFiles`. A file from before (with the old Auto / Manual
//! `mode` and a `localModel.selected` pick) is read by [`load`]: a picked
//! model file becomes an added file, a catalog pick goes back to Auto, and
//! tuning saved under Auto is dropped. The `app.json` `localModel` on/off
//! switch is separate: `local_use` owns it, and the typo tidy (`prompt_tidy`)
//! only reads it. The game, idle and battery switches and the app lists go to
//! `local/governor.json`, which the cabin's governor reads.

use std::path::{Path, PathBuf};

use grokhub_agent::route::{local_autoconfig, local_gov, local_live, local_sidecar, local_task};
use grokhub_core::local_discover::{self, FoundModel, Source};
use grokhub_core::local_governor::{Level, Rules};
use grokhub_core::local_picker::{self, BenchCache, Bucket, CpuSimd, Memory, Pick, Profile};
use grokhub_core::local_profile::{self as lp, Gpu};
use grokhub_core::local_setup::{self as ls, LocalModel};
use serde::{Deserialize, Serialize};

use crate::local_engine::{self, DraftOffer, DraftProgress, EngineInfo};

pub const FILE: &str = "local_ai.json";

/// Per-state memory caps may not go past this share.
pub const CAP_MAX_PCT: u8 = 90;
const CAP_MIN_PCT: u8 = 10;
const CTX_MIN: u32 = 2_048;
const CTX_MAX: u32 = 131_072;
const NGL_MAX: u32 = 999;
const IDLE_MAX_MIN: u32 = 240;

/// Memory share each governor state may plan for, in percent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Caps {
    pub ramp_up: u8,
    pub normal: u8,
    pub throttle: u8,
}

impl Default for Caps {
    fn default() -> Self {
        Caps {
            ramp_up: 90,
            normal: 80,
            throttle: 50,
        }
    }
}

/// Settings → Local AI → Advanced: engine tuning that applies whenever it's
/// set (blank is Auto), and the game app lists. Saved under `manual`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Manual {
    /// `auto`, or one of `cuda`, `rocm`, `vulkan`, `metal`, `sycl`, `cpu`.
    pub backend: String,
    pub ngl: Option<u32>,
    pub ctx: Option<u32>,
    pub threads: Option<u32>,
    /// `auto`, `q8` or `q4`.
    pub kv: String,
    /// Minutes without a request before the model unloads.
    pub idle_minutes: u32,
    pub caps: Caps,
    pub never_unload: bool,
    /// Apps that never count as a game, even fullscreen.
    pub allow_apps: Vec<String>,
    /// Apps that always count as a game.
    pub deny_apps: Vec<String>,
}

impl Default for Manual {
    fn default() -> Self {
        Manual {
            backend: "auto".into(),
            ngl: None,
            ctx: None,
            threads: None,
            kv: "auto".into(),
            idle_minutes: 10,
            caps: Caps::default(),
            never_unload: false,
            allow_apps: Vec::new(),
            deny_apps: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct LocalAiConfig {
    pub pause_for_games: bool,
    pub use_idle_time: bool,
    /// Local AI slows down on battery.
    pub throttle_on_battery: bool,
    pub manual: Manual,
    /// Model files the user added (`.gguf` paths). The engine loads the first
    /// one on disk when no downloaded catalog model is there.
    pub own_files: Vec<String>,
    /// Third-party model files whose notice the user has read.
    pub notice_seen: Vec<String>,
}

impl Default for LocalAiConfig {
    fn default() -> Self {
        LocalAiConfig {
            pause_for_games: true,
            use_idle_time: true,
            throttle_on_battery: true,
            manual: Manual::default(),
            own_files: Vec::new(),
            notice_seen: Vec::new(),
        }
    }
}

fn one_of(value: &str, allowed: &[&str]) -> String {
    let v = value.trim().to_ascii_lowercase();
    if allowed.contains(&v.as_str()) {
        v
    } else {
        "auto".into()
    }
}

/// Trimmed paths, each once, in order.
fn paths(list: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for p in list.iter().map(|p| p.trim()).filter(|p| !p.is_empty()) {
        if !out.iter().any(|o| o == p) {
            out.push(p.to_string());
        }
    }
    out
}

fn app_list(apps: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for app in apps.iter().map(|a| a.trim()).filter(|a| !a.is_empty()) {
        if !out.iter().any(|o| o.eq_ignore_ascii_case(app)) {
            out.push(app.to_string());
        }
    }
    out
}

impl LocalAiConfig {
    /// Every number back in range: caps at most 90% of memory, `threads` at
    /// most the machine's `max_threads`.
    pub fn clamped(mut self, max_threads: u32) -> Self {
        let m = &mut self.manual;
        m.backend = one_of(
            &m.backend,
            &["auto", "cuda", "rocm", "vulkan", "metal", "sycl", "cpu"],
        );
        m.kv = one_of(&m.kv, &["auto", "q8", "q4"]);
        m.ngl = m.ngl.map(|n| n.min(NGL_MAX));
        m.ctx = m.ctx.map(|c| c.clamp(CTX_MIN, CTX_MAX));
        m.threads = m.threads.map(|t| t.clamp(1, max_threads.max(1)));
        m.idle_minutes = m.idle_minutes.clamp(1, IDLE_MAX_MIN);
        for cap in [
            &mut m.caps.ramp_up,
            &mut m.caps.normal,
            &mut m.caps.throttle,
        ] {
            *cap = (*cap).clamp(CAP_MIN_PCT, CAP_MAX_PCT);
        }
        m.allow_apps = app_list(&m.allow_apps);
        m.deny_apps = app_list(&m.deny_apps);
        self.own_files = paths(&self.own_files);
        self.notice_seen = app_list(&self.notice_seen);
        self
    }

    /// What `local/governor.json` holds for these choices.
    pub fn rules(&self) -> Rules {
        Rules {
            allow: self.manual.allow_apps.clone(),
            deny: self.manual.deny_apps.clone(),
            throttle_on_battery: self.throttle_on_battery,
            pause_for_games: self.pause_for_games,
            use_idle_time: self.use_idle_time,
            ..Rules::default()
        }
    }

    /// The launch overrides Advanced sets; a blank one stays on Auto.
    pub fn overrides(&self) -> local_autoconfig::Overrides {
        let m = &self.manual;
        local_autoconfig::Overrides {
            ngl: m.ngl,
            ctx: m.ctx,
            threads: m.threads,
            kv: match m.kv.as_str() {
                "q8" => Some(local_autoconfig::KvType::Q8),
                "q4" => Some(local_autoconfig::KvType::Q4),
                _ => None,
            },
        }
    }
}

pub fn path(config_dir: &Path) -> PathBuf {
    config_dir.join(FILE)
}

/// A missing or broken file gives the defaults. A file from before the
/// model picker went away is read as [`from_old`] says.
pub fn load(config_dir: &Path) -> LocalAiConfig {
    let Some(raw) = std::fs::read_to_string(path(config_dir))
        .ok()
        .and_then(|raw| serde_json::from_str::<serde_json::Value>(&raw).ok())
    else {
        return LocalAiConfig::default();
    };
    let config = serde_json::from_value(raw.clone()).unwrap_or_default();
    from_old(&raw, config)
}

/// The old page had an Auto / Manual `mode` and a model pick
/// (`localModel.selected`). A picked model file becomes an added file; a
/// catalog pick goes back to Auto. Tuning only counted under Manual, so
/// tuning saved under Auto is dropped rather than switched on now.
fn from_old(raw: &serde_json::Value, mut config: LocalAiConfig) -> LocalAiConfig {
    let pick = raw
        .get("localModel")
        .and_then(|m| m.get("selected"))
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .unwrap_or_default();
    let is_file = Path::new(pick)
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("gguf"));
    if is_file && !config.own_files.iter().any(|f| f == pick) {
        config.own_files.push(pick.to_string());
    }
    if raw.get("mode").and_then(serde_json::Value::as_str) == Some("auto") {
        let m = &mut config.manual;
        (m.ngl, m.ctx, m.threads, m.kv) = (None, None, None, "auto".into());
    }
    config
}

fn write(path: &Path, text: &str) -> Result<(), String> {
    let dir = path.parent().ok_or("no folder for the settings file")?;
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, text).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, path).map_err(|e| e.to_string())
}

/// Saves the choices and the governor's rules, and applies the overrides.
pub fn save(config_dir: &Path, config: &LocalAiConfig) -> Result<(), String> {
    let raw = serde_json::to_string_pretty(config).map_err(|e| e.to_string())?;
    write(&path(config_dir), &raw)?;
    let gov = config_dir.join("local").join("governor.json");
    let mut rules = config.rules();
    // The shell has no Unload row; keep the cabin's choice from the file.
    if let Ok(text) = std::fs::read_to_string(&gov) {
        rules.unload_for_games = Rules::from_json(&text).unload_for_games;
    }
    let rules = serde_json::to_string_pretty(&rules).map_err(|e| e.to_string())?;
    write(&gov, &rules)?;
    local_autoconfig::set_overrides(config.overrides());
    Ok(())
}

/// Which reader found the GPUs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GpuReader {
    NvidiaSmi,
    Vulkan,
    None,
}

/// One hardware read. `None` is a reading that failed.
#[derive(Debug, Clone, PartialEq)]
pub struct Machine {
    pub gpus: Vec<Gpu>,
    pub reader: GpuReader,
    pub unified: bool,
    pub ram_mb: Option<u64>,
    pub free_disk_mb: Option<u64>,
    pub simd: CpuSimd,
    pub llama_build: Option<u32>,
}

impl Machine {
    /// The picker's view. Integrated GPUs share RAM and are left out; a disk
    /// that couldn't be read never hides a model.
    pub fn profile(&self) -> Profile {
        Profile {
            gpus_vram_mb: self
                .gpus
                .iter()
                .filter(|g| !g.integrated && !g.unified && g.vram_mb > 0)
                .map(|g| g.vram_mb)
                .collect(),
            unified: self.unified,
            ram_mb: self.ram_mb.unwrap_or(0),
            free_disk_mb: self.free_disk_mb.unwrap_or(u64::MAX),
            simd: self.simd,
            llama_build: self.llama_build,
        }
    }
}

/// What detection couldn't see, in words. `env` reads environment variables.
pub fn limits(m: &Machine, os: &str, env: &dyn Fn(&str) -> Option<String>) -> Vec<String> {
    let mut out = Vec::new();
    let blind_card = m
        .gpus
        .iter()
        .find(|g| !g.integrated && !g.unified && g.vram_mb == 0);
    match (m.reader, blind_card) {
        _ if m.unified => {}
        (GpuReader::None, _) => out.push(
            "No GPU reader found (nvidia-smi or vulkaninfo), so models are sized for RAM.".into(),
        ),
        (GpuReader::Vulkan, Some(g)) => out.push(format!(
            "Vulkan found {} but can't read its memory, so models are sized for RAM.",
            g.name
        )),
        _ => {}
    }
    if m.ram_mb.is_none() {
        out.push("RAM couldn't be read, so every model is sized as if there were none.".into());
    }
    if m.free_disk_mb.is_none() {
        out.push("Free disk space couldn't be read, so models aren't checked against it.".into());
    }
    let wayland = env("XDG_SESSION_TYPE").is_some_and(|s| s.eq_ignore_ascii_case("wayland"));
    let compositor = env("HYPRLAND_INSTANCE_SIGNATURE").is_some() || env("SWAYSOCK").is_some();
    if os == "linux" && wayland && !compositor {
        out.push(format!(
            "On Wayland {} can't see which app is fullscreen, so Pause while gaming only \
             sees games it can detect.",
            grokhub_core::brand::product_name()
        ));
    }
    out
}

fn bucket_name(b: Bucket) -> &'static str {
    match b {
        Bucket::Recommended => "recommended",
        Bucket::MaySlow => "maySlow",
        Bucket::Hidden => "hidden",
    }
}

fn memory_words(mem: Memory) -> &'static str {
    match mem {
        Memory::Gpu => "GPU memory",
        Memory::Unified => "unified memory",
        Memory::System => "spare RAM",
    }
}

/// The catalog speed estimate for `m` on `p`, or the speed test's number.
fn speed(p: &Profile, m: &LocalModel, bench: &BenchCache) -> String {
    if let Some(t) = bench.get(m.id) {
        return format!("{t} tok/s measured");
    }
    let est = match p.budget().0 {
        Memory::Gpu => m.tok_s.gpu,
        Memory::Unified => m.tok_s.unified,
        Memory::System => m.tok_s.cpu,
    };
    format!("about {est} tok/s")
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelRow {
    pub id: String,
    pub name: String,
    pub size: String,
    pub quant: String,
    pub ctx: u32,
    pub tok_s: String,
    pub license: String,
    pub badges: Vec<String>,
    pub bucket: &'static str,
    pub reason: String,
    pub preselected: bool,
    pub optional: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileRow {
    pub path: String,
    pub name: String,
    pub size: String,
    /// "LM Studio", "Ollama", "Loadout models" or "Model file".
    pub source: &'static str,
    /// Not a file from GrokHub's catalog.
    pub third_party: bool,
    /// Its name says its safety training was removed (every catalog build's does).
    pub abliterated: bool,
    /// Picking it shows the notice first: a third-party file not yet seen.
    pub needs_notice: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Picker {
    /// Recommended first, then May be slow, then Hidden (shown under Show all).
    pub models: Vec<ModelRow>,
    pub on_this_pc: Vec<FileRow>,
    /// What Auto picks for this PC: the tier default.
    pub selected: String,
}

fn row(p: &Profile, x: &Pick, bench: &BenchCache) -> ModelRow {
    let m = x.model;
    ModelRow {
        id: m.id.into(),
        name: m.name.into(),
        size: ls::size_label(m.download_mb),
        quant: m.quant.into(),
        ctx: m.ctx_default,
        tok_s: speed(p, m, bench),
        license: m.license.into(),
        badges: m.badges.iter().map(|b| b.to_string()).collect(),
        bucket: bucket_name(x.bucket),
        reason: x.reason.clone(),
        preselected: x.preselected,
        optional: m.optional,
    }
}

const ABLITERATED: [&str; 3] = ["abliterat", "uncensor", "decensor"];

/// A model file the user found or named.
pub fn file_row(path: &Path, bytes: u64, source: &'static str, seen: &[String]) -> FileRow {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let third_party = !ls::local_models().iter().any(|m| m.file == name);
    let lower = name.to_ascii_lowercase();
    let abliterated = ABLITERATED.iter().any(|w| lower.contains(w));
    let shown = path.display().to_string();
    FileRow {
        // Every catalog build is abliterated, so a first-time pick of one sees
        // the safety notice once, like any file from outside the catalog.
        needs_notice: (third_party || abliterated) && !seen.contains(&shown),
        path: shown,
        name,
        size: ls::size_label(bytes / (1_024 * 1_024)),
        source,
        third_party,
        abliterated,
    }
}

/// The models folder keeps its `GrokHub` name on disk; it shows by the brand.
static FOLDER_SOURCE: std::sync::LazyLock<String> =
    std::sync::LazyLock::new(|| format!("{} models", grokhub_core::brand::product_name()));

fn source_name(s: Source) -> &'static str {
    match s {
        Source::LmStudio => "LM Studio",
        Source::Ollama => "Ollama",
        Source::Folder => FOLDER_SOURCE.as_str(),
    }
}

/// The picker for `p`: catalog rows by bucket, then the files on this PC.
pub fn picker(
    p: &Profile,
    bench: &BenchCache,
    found: &[FoundModel],
    config: &LocalAiConfig,
) -> Picker {
    let picks = local_picker::score(p, ls::local_models(), bench);
    let mut models: Vec<ModelRow> = Vec::new();
    // In each bucket the defaults come first, then the optional makers.
    for bucket in [Bucket::Recommended, Bucket::MaySlow, Bucket::Hidden] {
        for optional in [false, true] {
            models.extend(
                picks
                    .iter()
                    .filter(|x| x.bucket == bucket && x.model.optional == optional)
                    .map(|x| row(p, x, bench)),
            );
        }
    }
    let on_this_pc = found
        .iter()
        .map(|f| file_row(&f.path, f.bytes, source_name(f.source), &config.notice_seen))
        .collect();
    let selected = models
        .iter()
        .find(|m| m.preselected)
        .map(|m| m.id.clone())
        .unwrap_or_default();
    Picker {
        models,
        on_this_pc,
        selected,
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub tier: String,
    pub model: String,
    pub state: String,
    pub reason: String,
    pub tok_s: String,
    pub memory: String,
}

pub fn state_word(level: Level) -> &'static str {
    match level {
        Level::Unload => "Paused",
        Level::Throttle => "Throttled",
        Level::Normal => "Normal",
        Level::RampUp => "Ramping up",
    }
}

/// The Auto status line's parts. `on` is the Use Local AI switch; `set_up`
/// is false until an engine and a model are on disk.
pub fn status(
    p: &Profile,
    picker: &Picker,
    bench: &BenchCache,
    on: bool,
    set_up: bool,
    level: Level,
    reason: &str,
) -> Status {
    let (mem, budget) = p.budget();
    let catalog = ls::model_by_id(&picker.selected);
    let model = match catalog {
        Some(m) => m.name.to_string(),
        None => Path::new(&picker.selected)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "None (cloud only)".into()),
    };
    let (state, reason) = if !on {
        (
            "Off".to_string(),
            "Use Local AI on this computer is off.".to_string(),
        )
    } else if set_up {
        (state_word(level).to_string(), reason.to_string())
    } else {
        (
            "Not set up".to_string(),
            "No local engine or model is installed yet.".to_string(),
        )
    };
    Status {
        tier: local_picker::tier(p).into(),
        model,
        state,
        reason,
        tok_s: catalog.map(|m| speed(p, m, bench)).unwrap_or_default(),
        memory: format!("{} of {}", ls::size_label(budget), memory_words(mem)),
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalAiView {
    pub config: LocalAiConfig,
    pub status: Status,
    pub picker: Picker,
    pub limits: Vec<String>,
    pub max_threads: u32,
    /// Pinned llama.cpp install: Get engine when missing.
    pub engine: EngineInfo,
    /// Catalog draft pair for the selected model, when not on disk yet.
    pub draft_offer: Option<DraftOffer>,
    /// Every catalog model that still needs its draft pair installed.
    pub draft_offers: Vec<DraftOffer>,
    /// In-flight draft download, when one is running.
    pub draft_progress: Option<DraftProgress>,
    /// What runs now: model, where, speed, draft model, unload.
    pub running: Running,
    /// Use Local AI on this computer.
    pub local_use: crate::local_use::LocalUseView,
    /// Tidy typos on this computer.
    pub tidy: crate::prompt_tidy::TidyView,
}

/// The running-model rows and their two controls.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Running {
    pub lines: Vec<String>,
    pub loaded: bool,
    pub kept_loaded: bool,
    /// Dynamic: the best model on disk per task.
    pub dynamic: bool,
    pub dynamic_hint: String,
}

/// The Running now line while Use Local AI on this computer is off and no
/// model is loaded: nothing loads with the next reply.
pub const RUNNING_OFF: &str = "Running now: nothing. Local AI is off, so no model loads.";

/// `on` is Use Local AI on this computer. While it is off nothing loads, so
/// the "loads with the next reply" line would be wrong; a model still
/// finishing an answer shows as usual until it stops.
pub fn running(config_dir: &Path, on: bool) -> Running {
    let live = local_live::snapshot(config_dir);
    let lines = if !on && live.model.is_none() {
        vec![RUNNING_OFF.to_string()]
    } else {
        local_live::lines(&live)
    };
    Running {
        lines,
        loaded: live.model.is_some(),
        kept_loaded: live.kept_loaded,
        dynamic: local_task::dynamic(config_dir),
        dynamic_hint: local_task::dynamic_row(local_task::dynamic(config_dir))
            .0
            .to_string(),
    }
}

/// Dynamic on or off, then the page again.
pub fn set_dynamic(config_dir: &Path, on: bool) -> Result<LocalAiView, String> {
    local_task::set_dynamic(config_dir, on)?;
    Ok(refreshed(config_dir))
}

/// Keep loaded / Let it unload, then the page again.
pub fn set_keep_loaded(config_dir: &Path, on: bool) -> LocalAiView {
    local_sidecar::set_keep_loaded(on);
    refreshed(config_dir)
}

/// Unload now, then the page again.
pub fn unload_now(config_dir: &Path) -> LocalAiView {
    local_sidecar::stop_global();
    refreshed(config_dir)
}

fn run(cmd: &str, args: &[&str]) -> Option<String> {
    let mut c = std::process::Command::new(cmd);
    c.args(args)
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        c.creation_flags(CREATE_NO_WINDOW);
    }
    let out = c.output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

fn read_gpus() -> (Vec<Gpu>, GpuReader) {
    let nvidia = run(
        "nvidia-smi",
        &[
            "--query-gpu=name,memory.total,memory.free,driver_version",
            "--format=csv,noheader,nounits",
        ],
    )
    .map(|s| lp::parse_nvidia_smi_all(&s))
    .unwrap_or_default();
    if !nvidia.is_empty() {
        return (nvidia, GpuReader::NvidiaSmi);
    }
    let vulkan = run("vulkaninfo", &["--summary"])
        .map(|s| lp::parse_vulkan_summary(&s))
        .unwrap_or_default();
    if vulkan.is_empty() {
        (vulkan, GpuReader::None)
    } else {
        (vulkan, GpuReader::Vulkan)
    }
}

#[cfg(windows)]
fn powershell_number(script: &str) -> Option<u64> {
    run(
        "powershell",
        &["-NoProfile", "-NonInteractive", "-Command", script],
    )?
    .trim()
    .parse()
    .ok()
}

#[cfg(target_os = "linux")]
fn ram_mb() -> Option<u64> {
    ls::parse_meminfo(&std::fs::read_to_string("/proc/meminfo").ok()?)
}

#[cfg(target_os = "macos")]
fn ram_mb() -> Option<u64> {
    ls::parse_memsize_bytes(&run("sysctl", &["-n", "hw.memsize"])?)
}

#[cfg(windows)]
fn ram_mb() -> Option<u64> {
    powershell_number("(Get-CimInstance Win32_ComputerSystem).TotalPhysicalMemory")
        .map(|b| b / (1_024 * 1_024))
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
fn ram_mb() -> Option<u64> {
    None
}

#[cfg(unix)]
pub(crate) fn free_disk_mb(dir: &Path) -> Option<u64> {
    ls::parse_df_avail_mb(&run("df", &["-Pk", &dir.to_string_lossy()])?)
}

#[cfg(windows)]
pub(crate) fn free_disk_mb(dir: &Path) -> Option<u64> {
    let quoted = dir.display().to_string().replace('\'', "''");
    powershell_number(&format!(
        "([System.IO.DriveInfo]::new('{quoted}')).AvailableFreeSpace"
    ))
    .map(|b| b / (1_024 * 1_024))
}

#[cfg(not(any(unix, windows)))]
pub(crate) fn free_disk_mb(_dir: &Path) -> Option<u64> {
    None
}

pub(crate) fn models_dir() -> Option<PathBuf> {
    Some(
        grokhub_core::paths::user_home()?
            .join(grokhub_core::brand::INTERNAL_ID)
            .join("models"),
    )
}

/// The nearest folder that exists, for the free-space read.
pub(crate) fn existing(dir: &Path) -> PathBuf {
    dir.ancestors()
        .find(|d| d.is_dir())
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Reads this machine: GPUs (nvidia-smi, else Vulkan), RAM, free disk under
/// the models folder, SIMD and the pinned engine build. Runs commands.
pub fn read_machine(config_dir: &Path) -> Machine {
    let unified = cfg!(all(target_os = "macos", target_arch = "aarch64"));
    let (gpus, reader) = if unified {
        (Vec::new(), GpuReader::None)
    } else {
        read_gpus()
    };
    let disk_dir = models_dir().unwrap_or_else(|| config_dir.join("models"));
    Machine {
        gpus,
        reader,
        unified,
        ram_mb: ram_mb(),
        free_disk_mb: free_disk_mb(&existing(&disk_dir)),
        simd: CpuSimd::detect(),
        llama_build: grokhub_core::runtime_download::manifest_for_host()
            .and_then(|m| local_picker::build_number(&m.version)),
    }
}

fn found_models(config_dir: &Path) -> Vec<FoundModel> {
    let Some(home) = grokhub_core::paths::user_home() else {
        return Vec::new();
    };
    let ollama = std::env::var("OLLAMA_MODELS").ok();
    let dirs: Vec<PathBuf> = [models_dir(), Some(config_dir.join("models"))]
        .into_iter()
        .flatten()
        .collect();
    local_discover::discover(&home, ollama.as_deref(), &dirs)
        .into_iter()
        // Drafters and vision projectors pair with a model; they aren't one
        // to chat with, and loading one as the model stops the engine.
        .filter(|f| !local_sidecar::helper_gguf(&f.path))
        .collect()
}

pub fn max_threads() -> u32 {
    std::thread::available_parallelism()
        .map(|n| n.get() as u32)
        .unwrap_or(1)
}

/// Everything the page shows. Runs the hardware readers.
pub fn view(config_dir: &Path) -> LocalAiView {
    let config = load(config_dir).clamped(max_threads());
    let machine = read_machine(config_dir);
    let p = machine.profile();
    let bench = BenchCache::new();
    let picker = picker(&p, &bench, &found_models(config_dir), &config);
    let set_up = local_sidecar::resolve_paths(config_dir).is_some();
    let status = status(
        &p,
        &picker,
        &bench,
        crate::local_use::is_on(config_dir),
        set_up,
        local_gov::level(),
        local_gov::reason(),
    );
    let selected = picker.selected.as_str();
    let env = |k: &str| std::env::var(k).ok();
    LocalAiView {
        limits: limits(&machine, std::env::consts::OS, &env),
        engine: local_engine::engine_info(config_dir),
        draft_offer: local_engine::draft_offer(config_dir, selected),
        draft_offers: local_engine::draft_offers(config_dir),
        draft_progress: local_engine::draft_progress(),
        running: running(config_dir, crate::local_use::is_on(config_dir)),
        local_use: crate::local_use::view(config_dir),
        tidy: crate::prompt_tidy::view(config_dir),
        config,
        status,
        picker,
        max_threads: max_threads(),
    }
}

/// "Add a model file": the path must be a `.gguf` that exists.
pub fn model_file(path: &str, seen: &[String]) -> Result<FileRow, String> {
    let path = PathBuf::from(path.trim());
    let gguf = path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("gguf"));
    if !gguf {
        return Err("Pick a .gguf model file.".into());
    }
    let meta = std::fs::metadata(&path).map_err(|_| format!("{} isn't there.", path.display()))?;
    if !meta.is_file() {
        return Err(format!("{} isn't a file.", path.display()));
    }
    Ok(file_row(&path, meta.len(), "Model file", seen))
}

/// The speed test: the model with and without its draft model.
pub fn speed_test(config_dir: &Path) -> Result<String, String> {
    // The bench switches the router's local route on; while the switch is
    // off it would load the model the user turned off.
    if !crate::local_use::is_on(config_dir) {
        return Err(SPEED_TEST_LOCAL_OFF.into());
    }
    if local_sidecar::resolve_paths(config_dir).is_none() {
        return Err("Set up a local engine and model first, then run the speed test.".into());
    }
    grokhub_agent::route::local_bench::run_for_ui()
}

/// Why the speed test won't run while Use Local AI on this computer is off.
pub const SPEED_TEST_LOCAL_OFF: &str =
    "Turn on Use Local AI on this computer first, then run the speed test.";

/// Saves the page's choices, clamped, and reads the page again.
pub fn save_view(config_dir: &Path, config: LocalAiConfig) -> Result<LocalAiView, String> {
    let before = local_sidecar::find_model(config_dir);
    save(config_dir, &config.clamped(max_threads()))?;
    // A newly added (or removed) model file loads on the next request, not
    // after a restart.
    if local_sidecar::find_model(config_dir) != before {
        local_sidecar::stop_global();
    }
    Ok(refreshed(config_dir))
}

/// Reset Advanced: the tuning and app lists go back to the defaults; the
/// switches, the added files and the notices already read stay.
pub fn reset(config_dir: &Path) -> Result<LocalAiView, String> {
    let old = load(config_dir);
    let config = LocalAiConfig {
        manual: Manual::default(),
        ..old
    };
    save_view(config_dir, config)
}

/// "Add a model file", against the notices already read.
pub fn check_file(config_dir: &Path, path: &str) -> Result<FileRow, String> {
    model_file(path, &load(config_dir).notice_seen)
}

/// [`view`], with the tray chip updated to match.
pub fn refreshed(config_dir: &Path) -> LocalAiView {
    let v = view(config_dir);
    refresh_tray(&v.status);
    v
}

#[cfg(desktop)]
static TRAY_ITEM: std::sync::OnceLock<tauri::menu::MenuItem<tauri::Wry>> =
    std::sync::OnceLock::new();

/// The tray chip's line.
pub fn tray_label(status: &Status) -> String {
    format!("Local AI: {}", status.state)
}

pub const TRAY_CHECKING: &str = "Local AI: checking…";

/// Keeps the tray's state line so a save can update it, and fills it in off
/// the main thread (the hardware read runs commands).
#[cfg(desktop)]
pub fn start_tray(item: tauri::menu::MenuItem<tauri::Wry>, config_dir: PathBuf) {
    if TRAY_ITEM.set(item).is_ok() {
        std::thread::spawn(move || {
            refreshed(&config_dir);
        });
    }
}

#[cfg(desktop)]
fn refresh_tray(status: &Status) {
    if let Some(item) = TRAY_ITEM.get() {
        let _ = item.set_text(tray_label(status));
    }
}

/// Phones have no tray.
#[cfg(mobile)]
fn refresh_tray(_status: &Status) {}

#[cfg(test)]
#[path = "local_ai_tests.rs"]
mod tests;
