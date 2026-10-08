//! Model onboarding: a versioned profile per model, built from its listing and
//! its capability probe, at `{config}/models/model_profiles/<id>.json`.
//! [`RuntimeSettings::from_profile`] turns it into the settings a call would use.
//! In R0 those only go into the shadow route record; live calls keep today's values.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::record::{ModelMeta, Prices};
use super::store::{profiles_dir, read_json, safe_file_stem, write_json};
use super::EFFORT_LADDER;

pub const PROFILE_SCHEMA: u32 = 1;
/// Older versions kept beside the current file as `<id>.v<n>.json`.
pub const PROFILE_KEEP_VERSIONS: usize = 3;
/// Per-call timeout for background classes and for everything else.
pub const BACKGROUND_TIMEOUT_SECS: u64 = 120;
pub const FOREGROUND_TIMEOUT_SECS: u64 = 600;

/// The listing fields a profile keeps. Unknown stays `null`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProfileMeta {
    pub context_length: Option<u64>,
    pub max_output: Option<u64>,
    pub input_modalities: Option<Vec<String>>,
    pub output_modalities: Option<Vec<String>>,
    pub tool_calling: Option<bool>,
    pub structured_output: Option<bool>,
    pub efforts: Option<Vec<String>>,
    pub default_effort: Option<String>,
    pub api_shape: Option<String>,
    pub prices: Prices,
}

impl From<&ModelMeta> for ProfileMeta {
    fn from(m: &ModelMeta) -> Self {
        Self {
            context_length: m.context_length,
            max_output: m.max_output,
            input_modalities: m.input_modalities.clone(),
            output_modalities: m.output_modalities.clone(),
            tool_calling: m.tool_calling,
            structured_output: m.structured_output,
            efforts: m.efforts.clone(),
            default_effort: m.default_effort.clone(),
            api_shape: m.api_shape.clone(),
            prices: m.prices.clone(),
        }
    }
}

/// One effort's measured speed. `measured` is `probe`, or `passive` when the
/// run's token cap was reached first (real traffic fills it later).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EffortTiming {
    pub effort: String,
    pub measured: String,
    /// Time to the whole (non-streamed) reply: an upper bound on time to first token.
    #[serde(default)]
    pub reply_ms: Option<u64>,
    #[serde(default)]
    pub tokens_per_sec: Option<u64>,
}

/// What the capability probe found. `status` is `queued`, `ok`, `failed`, or `not_run: <why>`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProbeResult {
    pub status: String,
    #[serde(default)]
    pub tool_call: Option<bool>,
    #[serde(default)]
    pub json_mode: Option<bool>,
    #[serde(default)]
    pub caching: Option<bool>,
    #[serde(default)]
    pub cached_tokens: Option<u64>,
    #[serde(default)]
    pub efforts: Vec<EffortTiming>,
    #[serde(default)]
    pub calls: u32,
    /// In + out + reasoning over the run.
    #[serde(default)]
    pub tokens: u64,
    #[serde(default)]
    pub cost_ticks: i64,
    /// The plain reason a failed probe makes the model unusable.
    #[serde(default)]
    pub failure: Option<String>,
}

impl ProbeResult {
    pub fn status(status: &str) -> Self {
        Self { status: status.into(), ..Self::default() }
    }
}

/// Settings a call to this model would use for a class.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeSettings {
    pub max_output_tokens: Option<u64>,
    /// Canonical ladder rung → this model's effort (clamped), or `None` to send none.
    pub effort_map: BTreeMap<String, Option<String>>,
    pub api_shape: String,
    /// `native` or `none`.
    pub tool_adapter: String,
    /// `native` (JSON schema) or `prompt`.
    pub json_adapter: String,
    pub timeout_secs: u64,
    pub sends_reasoning_effort: bool,
}

fn rung(effort: &str) -> Option<usize> {
    EFFORT_LADDER.iter().position(|e| e.eq_ignore_ascii_case(effort.trim()))
}

/// Clamp a canonical rung to a model's menu: the highest offered level at or
/// below it, else the lowest offered. An empty menu sends no effort.
pub fn clamp_effort(effort: &str, menu: &[String]) -> Option<String> {
    let want = rung(effort)?;
    let mut offered: Vec<(usize, &String)> = menu.iter().filter_map(|m| rung(m).map(|r| (r, m))).collect();
    offered.sort_by_key(|(r, _)| *r);
    offered
        .iter()
        .rev()
        .find(|(r, _)| *r <= want)
        .or_else(|| offered.first())
        .map(|(_, m)| m.to_string())
}

impl RuntimeSettings {
    pub fn from_profile(p: &ModelProfile, class: &str) -> Self {
        let menu = p.metadata.efforts.clone().unwrap_or_default();
        let effort_map = EFFORT_LADDER.iter().map(|e| (e.to_string(), clamp_effort(e, &menu))).collect();
        let tools_off = p.metadata.tool_calling == Some(false) || p.probe.tool_call == Some(false);
        let json_off = p.metadata.structured_output == Some(false) || p.probe.json_mode == Some(false);
        Self {
            max_output_tokens: p.metadata.max_output,
            effort_map,
            api_shape: p.metadata.api_shape.clone().unwrap_or_else(|| "responses".into()),
            tool_adapter: if tools_off { "none" } else { "native" }.into(),
            json_adapter: if json_off { "prompt" } else { "native" }.into(),
            timeout_secs: if class.starts_with("background:") { BACKGROUND_TIMEOUT_SECS } else { FOREGROUND_TIMEOUT_SECS },
            sends_reasoning_effort: menu.iter().any(|m| rung(m).is_some()),
        }
    }
}

/// `{schema: 1, version, source_hash, built_at, metadata, probe, runtime, usable, unusable_reason}`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelProfile {
    pub schema: u32,
    pub model: String,
    pub version: u32,
    pub source_hash: String,
    pub built_at: u64,
    pub metadata: ProfileMeta,
    pub probe: ProbeResult,
    pub runtime: RuntimeSettings,
    pub usable: bool,
    pub unusable_reason: Option<String>,
}

impl ModelProfile {
    /// A profile from the listing and a probe result. A failed probe is unusable;
    /// a queued one is not usable yet.
    pub fn build(meta: &ModelMeta, probe: ProbeResult, now_ms: u64) -> Self {
        let metadata = ProfileMeta::from(meta);
        let source_hash = {
            let text = format!(
                "{}\n{}",
                serde_json::to_string(&metadata).unwrap_or_default(),
                serde_json::to_string(&probe).unwrap_or_default()
            );
            hex::encode(Sha256::digest(text.as_bytes()))
        };
        let (usable, unusable_reason) = match (probe.status.as_str(), &probe.failure) {
            (_, Some(why)) => (false, Some(why.clone())),
            ("queued", _) => (false, Some("Waiting for its first check.".to_string())),
            _ => (true, None),
        };
        let mut p = Self {
            schema: PROFILE_SCHEMA,
            model: meta.id.clone(),
            version: 1,
            source_hash,
            built_at: now_ms,
            metadata,
            probe,
            runtime: RuntimeSettings::default(),
            usable,
            unusable_reason,
        };
        p.runtime = RuntimeSettings::from_profile(&p, "chat:default");
        p
    }
}

pub fn profile_path(config_dir: &Path, id: &str) -> PathBuf {
    profiles_dir(config_dir).join(format!("{}.json", safe_file_stem(id)))
}

pub fn read_profile(config_dir: &Path, id: &str) -> Option<ModelProfile> {
    read_json(&profile_path(config_dir, id))
}

/// Every current profile, by model id.
pub fn read_profiles(config_dir: &Path) -> BTreeMap<String, ModelProfile> {
    let mut out = BTreeMap::new();
    let Ok(rd) = fs::read_dir(profiles_dir(config_dir)) else {
        return out;
    };
    for entry in rd.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some(stem) = name.strip_suffix(".json") else {
            continue;
        };
        if version_of(stem).is_some() {
            continue;
        }
        if let Some(p) = read_json::<ModelProfile>(&entry.path()) {
            out.insert(p.model.clone(), p);
        }
    }
    out
}

fn version_of(stem: &str) -> Option<u32> {
    let (_, v) = stem.rsplit_once(".v")?;
    v.parse().ok()
}

/// Write `profile` as the model's current file. Same `source_hash` as the
/// current file writes nothing (`Ok(false)`). Otherwise the current file moves
/// to `<id>.v<n>.json`, the new one gets version n+1, and only the newest
/// [`PROFILE_KEEP_VERSIONS`] old versions are kept.
pub fn write_profile(config_dir: &Path, profile: &mut ModelProfile) -> Result<bool, String> {
    let path = profile_path(config_dir, &profile.model);
    let stem = safe_file_stem(&profile.model);
    let dir = profiles_dir(config_dir);
    if let Some(old) = read_json::<ModelProfile>(&path) {
        if old.source_hash == profile.source_hash {
            return Ok(false);
        }
        profile.version = old.version.saturating_add(1);
        fs::rename(&path, dir.join(format!("{stem}.v{}.json", old.version))).map_err(|e| e.to_string())?;
    } else {
        profile.version = 1;
    }
    write_json(&path, profile)?;
    let mut old: Vec<(u32, PathBuf)> = fs::read_dir(&dir)
        .map_err(|e| e.to_string())?
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let rest = name.strip_prefix(&format!("{stem}.v"))?.strip_suffix(".json")?;
            Some((rest.parse().ok()?, e.path()))
        })
        .collect();
    old.sort_by_key(|(v, _)| std::cmp::Reverse(*v));
    for (_, p) in old.into_iter().skip(PROFILE_KEEP_VERSIONS) {
        let _ = fs::remove_file(p);
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("grokhub-profile-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        d
    }

    fn meta(efforts: &[&str]) -> ModelMeta {
        ModelMeta {
            id: "grok/4.7:beta".into(),
            context_length: Some(500_000),
            max_output: Some(64_000),
            input_modalities: Some(vec!["text".into(), "image".into()]),
            output_modalities: Some(vec!["text".into()]),
            tool_calling: Some(true),
            structured_output: Some(true),
            efforts: Some(efforts.iter().map(|s| s.to_string()).collect()),
            default_effort: Some("medium".into()),
            api_shape: Some("responses".into()),
            prices: Prices { prompt: Some(30_000), cached: Some(7_500), completion: Some(150_000), prompt_long: None, completion_long: None, long_context_threshold: Some(200_000) },
            ..ModelMeta::default()
        }
    }

    #[test]
    fn clamp_rounds_down_to_the_menu_and_up_from_below_it() {
        let menu: Vec<String> = ["low", "medium", "high"].iter().map(|s| s.to_string()).collect();
        assert_eq!(clamp_effort("xhigh", &menu).as_deref(), Some("high"));
        assert_eq!(clamp_effort("max", &menu).as_deref(), Some("high"));
        assert_eq!(clamp_effort("medium", &menu).as_deref(), Some("medium"));
        assert_eq!(clamp_effort("minimal", &menu).as_deref(), Some("low"));
        assert_eq!(clamp_effort("none", &menu).as_deref(), Some("low"));
        assert_eq!(clamp_effort("high", &[]), None);
        assert_eq!(clamp_effort("bogus", &menu), None);
    }

    #[test]
    fn from_profile_maps_the_ladder_onto_the_fixture_menu() {
        let p = ModelProfile::build(&meta(&["low", "high"]), ProbeResult::status("ok"), 1);
        let rs = RuntimeSettings::from_profile(&p, "background:judge");
        let want: BTreeMap<String, Option<String>> = [
            ("high", "high"), ("low", "low"), ("max", "high"), ("medium", "low"), ("minimal", "low"), ("none", "low"), ("xhigh", "high"),
        ]
        .iter()
        .map(|(k, v)| (k.to_string(), Some(v.to_string())))
        .collect();
        assert_eq!(rs.effort_map, want);
        assert_eq!(rs.max_output_tokens, Some(64_000));
        assert_eq!((rs.api_shape.as_str(), rs.tool_adapter.as_str(), rs.json_adapter.as_str()), ("responses", "native", "native"));
        assert_eq!((rs.timeout_secs, rs.sends_reasoning_effort), (BACKGROUND_TIMEOUT_SECS, true));
        let mut bad = ProbeResult::status("failed");
        bad.tool_call = Some(false);
        bad.failure = Some("Tool calls didn't come back in the right shape.".into());
        let q = ModelProfile::build(&meta(&[]), bad, 1);
        assert_eq!((q.usable, q.unusable_reason.as_deref()), (false, Some("Tool calls didn't come back in the right shape.")));
        assert_eq!((q.runtime.tool_adapter.as_str(), q.runtime.sends_reasoning_effort), ("none", false));
    }

    #[test]
    fn an_added_model_writes_a_full_profile_a_change_bumps_and_keeps_old_and_same_writes_nothing() {
        let d = dir("versions");
        let mut p1 = ModelProfile::build(&meta(&["low", "high"]), ProbeResult::status("not_run: gb_only"), 10);
        assert_eq!(write_profile(&d, &mut p1), Ok(true));
        let path = profile_path(&d, "grok/4.7:beta");
        assert_eq!(path.file_name().unwrap().to_string_lossy(), "grok_4.7_beta.json");
        let back = read_profile(&d, "grok/4.7:beta").unwrap();
        assert_eq!(back.version, 1);
        assert_eq!(back.metadata.context_length, Some(500_000));
        assert_eq!(back.metadata.prices.prompt_long, None);
        let raw = fs::read_to_string(&path).unwrap();
        for key in ["\"context_length\"", "\"max_output\"", "\"input_modalities\"", "\"tool_calling\"", "\"structured_output\"", "\"efforts\"", "\"api_shape\"", "\"long_context_threshold\""] {
            assert!(raw.contains(key), "{key} missing: {raw}");
        }
        let mut same = ModelProfile::build(&meta(&["low", "high"]), ProbeResult::status("not_run: gb_only"), 99);
        assert_eq!(write_profile(&d, &mut same), Ok(false));
        let mut changed = ModelProfile::build(&meta(&["low", "medium", "high"]), ProbeResult::status("not_run: gb_only"), 20);
        assert_eq!(write_profile(&d, &mut changed), Ok(true));
        assert_eq!(read_profile(&d, "grok/4.7:beta").unwrap().version, 2);
        assert!(profiles_dir(&d).join("grok_4.7_beta.v1.json").is_file());
        for n in 0..5 {
            let mut next = ModelProfile::build(&meta(&["low"]), ProbeResult::status(&format!("ok{n}")), 30);
            write_profile(&d, &mut next).unwrap();
        }
        assert_eq!(read_profile(&d, "grok/4.7:beta").unwrap().version, 7);
        let olds: Vec<u32> = (1..=7).filter(|v| profiles_dir(&d).join(format!("grok_4.7_beta.v{v}.json")).is_file()).collect();
        assert_eq!(olds, vec![4, 5, 6]);
        assert_eq!(read_profiles(&d).len(), 1);
        let _ = fs::remove_dir_all(d);
    }
}
