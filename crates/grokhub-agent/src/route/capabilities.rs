//! Capability catalog for Auto: what each available model is good at, and
//! which strengths each call class needs. A high-fidelity, low-latency local
//! scorer (versioned seed research + live listing metadata) fills
//! `{config}/models/capability_catalog.json`; rebuild refreshes it when the
//! registry hash changes. Token efficiency is for the *task* model Auto picks
//! — not for weakening this catalog or the attention layer that reads the ask.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use grokhub_core::model_registry::store::{models_dir, read_json, write_json};
use grokhub_core::model_registry::{ModelMeta, Registry};

use super::policy::{self, family};
use super::table::SpeedQuality;

/// On-disk catalog schema.
pub const CATALOG_SCHEMA: u32 = 1;
pub const CATALOG_FILE: &str = "capability_catalog.json";
/// Bump when curated family heuristics or baseline-lane scoring change.
pub const SEED_VERSION: u32 = 2;

/// Baseline lane for table build / Stay. Attention escalate can still move
/// Workhorse → Flagship for one ask; ForceTop classes stay Flagship.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BaselineLane {
    /// Cheap/fast task model (often non-reasoning).
    Cheap,
    /// Everyday mid reasoning — not the plan's strongest model.
    Workhorse,
    /// Coding-strong specialist.
    Code,
    /// Strongest reasoning on the plan (repair / prepare / plan / design / debug).
    Flagship,
}

impl BaselineLane {
    pub fn of(class: &str) -> Self {
        let class = policy::class_row(class).map(|r| r.class).unwrap_or(class);
        match class {
            "background:classify"
            | "background:triage"
            | "background:redact"
            | "background:summarize"
            | "background:compact"
            | "chat:quick" => Self::Cheap,
            "code:multi-file" => Self::Code,
            "plan" | "design" | "debug" | "prepare:hard" | "repair:diagnose" => Self::Flagship,
            // chat:default, code:edit-small, desktop:soft, memory/judge/dream, …
            _ => Self::Workhorse,
        }
    }
}

/// 0..=100 scores on each axis. Higher is better for that strength.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Caps {
    pub reasoning: u8,
    pub code: u8,
    pub speed: u8,
    /// Higher means cheaper for typical traffic.
    pub cost_efficiency: u8,
    pub vision: u8,
    pub long_context: u8,
    pub tools: u8,
}

/// How much a class cares about each axis (0..=10) plus whether reasoning is required.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClassNeed {
    pub reasoning: u8,
    pub code: u8,
    pub speed: u8,
    pub cost: u8,
    pub vision: u8,
    pub long_context: u8,
    pub tools: u8,
    pub require_reasoning: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CapabilityEntry {
    pub model: String,
    pub caps: Caps,
    /// `seed+listing`, `listing`, or `seed`.
    pub source: String,
    pub refreshed_at: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CapabilityCatalog {
    #[serde(default)]
    pub schema: u32,
    #[serde(default)]
    pub seed_version: u32,
    #[serde(default)]
    pub built_at: u64,
    #[serde(default)]
    pub registry_hash: String,
    #[serde(default)]
    pub entries: BTreeMap<String, CapabilityEntry>,
}

impl CapabilityCatalog {
    pub fn get(&self, id: &str) -> Option<&Caps> {
        self.entries.get(id).map(|e| &e.caps)
    }

    pub fn caps_or_derive(&self, reg: &Registry, id: &str) -> Caps {
        self.get(id)
            .copied()
            .or_else(|| reg.get(id).map(|r| derive_caps(&r.meta)))
            .unwrap_or_else(|| derive_caps(&ModelMeta::bare(id)))
    }
}

pub fn catalog_path(config_dir: &Path) -> std::path::PathBuf {
    models_dir(config_dir).join(CATALOG_FILE)
}

pub fn load_catalog(config_dir: &Path) -> CapabilityCatalog {
    read_json(&catalog_path(config_dir)).unwrap_or_default()
}

/// Class need vector. Speed lanes stay cheap/fast; balanced mid-work wants
/// light reasoning; quality lanes escalate only when the task needs it.
pub fn class_need(class: &str) -> ClassNeed {
    let class = policy::class_row(class).map(|r| r.class).unwrap_or(class);
    match class {
        "background:classify"
        | "background:triage"
        | "background:redact"
        | "background:summarize"
        | "background:compact"
        | "chat:quick" => ClassNeed {
            reasoning: 1,
            code: 0,
            speed: 10,
            cost: 10,
            vision: 0,
            long_context: 0,
            tools: 0,
            require_reasoning: false,
        },
        // Workhorse backgrounds: solid mid reasoning, keep the flagship free.
        "background:memory" => ClassNeed {
            reasoning: 5,
            code: 0,
            speed: 5,
            cost: 8,
            vision: 0,
            long_context: 3,
            tools: 0,
            require_reasoning: true,
        },
        "background:judge" => ClassNeed {
            reasoning: 5,
            code: 2,
            speed: 4,
            cost: 7,
            vision: 0,
            long_context: 2,
            tools: 4,
            require_reasoning: true,
        },
        "background:dream" => ClassNeed {
            reasoning: 5,
            code: 0,
            speed: 4,
            cost: 7,
            vision: 0,
            long_context: 5,
            tools: 0,
            require_reasoning: true,
        },
        "chat:default" => ClassNeed {
            reasoning: 5,
            code: 2,
            speed: 5,
            cost: 7,
            vision: 2,
            long_context: 3,
            tools: 4,
            require_reasoning: true,
        },
        "code:edit-small" => ClassNeed {
            reasoning: 4,
            code: 5,
            speed: 5,
            cost: 7,
            vision: 0,
            long_context: 2,
            tools: 4,
            require_reasoning: true,
        },
        "plan" => ClassNeed {
            reasoning: 10,
            code: 2,
            speed: 0,
            cost: 0,
            vision: 0,
            long_context: 6,
            tools: 3,
            require_reasoning: true,
        },
        "design" => ClassNeed {
            reasoning: 9,
            code: 2,
            speed: 0,
            cost: 0,
            vision: 3,
            long_context: 5,
            tools: 2,
            require_reasoning: true,
        },
        "code:multi-file" => ClassNeed {
            reasoning: 6,
            code: 10,
            speed: 1,
            cost: 1,
            vision: 0,
            long_context: 5,
            tools: 8,
            require_reasoning: true,
        },
        "debug" => ClassNeed {
            reasoning: 10,
            code: 4,
            speed: 0,
            cost: 0,
            vision: 0,
            long_context: 4,
            tools: 5,
            require_reasoning: true,
        },
        "desktop:soft" => ClassNeed {
            reasoning: 5,
            code: 2,
            speed: 4,
            cost: 6,
            vision: 4,
            long_context: 2,
            tools: 6,
            require_reasoning: true,
        },
        "prepare:hard" => ClassNeed {
            reasoning: 10,
            code: 3,
            speed: 0,
            cost: 0,
            vision: 0,
            long_context: 3,
            tools: 5,
            require_reasoning: true,
        },
        // Host repair: always the strongest reasoning model. Never cheap/fast.
        "repair:diagnose" => ClassNeed {
            reasoning: 10,
            code: 3,
            speed: 0,
            cost: 0,
            vision: 1,
            long_context: 4,
            tools: 5,
            require_reasoning: true,
        },
        _ => ClassNeed {
            reasoning: 5,
            code: 3,
            speed: 3,
            cost: 4,
            vision: 0,
            long_context: 2,
            tools: 4,
            require_reasoning: SpeedQuality::of(class).needs_reasoning(),
        },
    }
}

/// Fit of `caps` to `need` in \[0, ~7\]. Pure.
pub fn fit_score(caps: &Caps, need: &ClassNeed) -> f64 {
    let term = |w: u8, c: u8| (w as f64) * (c as f64) / 1000.0;
    term(need.reasoning, caps.reasoning)
        + term(need.code, caps.code)
        + term(need.speed, caps.speed)
        + term(need.cost, caps.cost_efficiency)
        + term(need.vision, caps.vision)
        + term(need.long_context, caps.long_context)
        + term(need.tools, caps.tools)
}

/// Strongest reasoning score among candidate caps (for Workhorse flagship penalty).
pub fn plan_max_reasoning<'a>(caps: impl IntoIterator<Item = &'a Caps>) -> u8 {
    caps.into_iter().map(|c| c.reasoning).max().unwrap_or(0)
}

/// Baseline rank score for table build / Stay. Workhorse reserves the plan's
/// strongest reasoner for Flagship classes and attention escalate; Flagship
/// boosts raw reasoning so a coding mid cannot beat repair/debug/plan.
pub fn baseline_score(caps: &Caps, need: &ClassNeed, lane: BaselineLane, plan_max_r: u8) -> f64 {
    let mut s = fit_score(caps, need);
    match lane {
        BaselineLane::Cheap | BaselineLane::Code => s,
        BaselineLane::Workhorse => {
            // Everyday baseline must not burn the flagship when a weaker
            // reasoner exists on the plan (coding mid or dated mid).
            if plan_max_r >= 90 && caps.reasoning + 1 >= plan_max_r {
                s -= 0.40;
            }
            s
        }
        BaselineLane::Flagship => {
            // Hard preference for deepest reasoning (repair / prepare / debug / plan).
            s + (caps.reasoning as f64) * 0.02
        }
    }
}

/// Build (or refresh) the catalog from the live registry + versioned seed.
pub fn build_catalog(reg: &Registry, now_ms: u64) -> CapabilityCatalog {
    let mut entries = BTreeMap::new();
    for (id, rec) in &reg.models {
        let mut caps = derive_caps(&rec.meta);
        let seeded = apply_seed(id, &mut caps);
        let source = if seeded { "seed+listing" } else { "listing" };
        entries.insert(
            id.clone(),
            CapabilityEntry {
                model: id.clone(),
                caps,
                source: source.into(),
                refreshed_at: now_ms,
            },
        );
    }
    CapabilityCatalog {
        schema: CATALOG_SCHEMA,
        seed_version: SEED_VERSION,
        built_at: now_ms,
        registry_hash: reg.hash.clone(),
        entries,
    }
}

/// Refresh when the registry hash or seed version changed (or `force`).
pub fn refresh_catalog(
    config_dir: &Path,
    reg: &Registry,
    force: bool,
    now_ms: u64,
) -> CapabilityCatalog {
    let old = load_catalog(config_dir);
    if !force
        && old.schema == CATALOG_SCHEMA
        && old.seed_version == SEED_VERSION
        && old.registry_hash == reg.hash
        && !old.entries.is_empty()
    {
        return old;
    }
    let catalog = build_catalog(reg, now_ms);
    let _ = write_json(&catalog_path(config_dir), &catalog);
    catalog
}

/// Listing-derived baseline (provider-agnostic). Seed overlays refine known families.
pub fn derive_caps(meta: &ModelMeta) -> Caps {
    let id = meta.id.to_ascii_lowercase();
    let non_reasoning = id.contains("non-reasoning");
    let fast = id.contains("-fast") || id.ends_with("fast");
    let ver_boost = version_boost(&meta.id);

    let mut reasoning = if non_reasoning {
        22
    } else if meta.efforts.as_ref().is_some_and(|e| !e.is_empty()) {
        (58 + ver_boost).min(96)
    } else if grokhub_core::accepts_reasoning_effort(&meta.id) {
        (52 + ver_boost).min(90)
    } else {
        40
    };

    let mut speed = if non_reasoning {
        94
    } else if fast {
        88
    } else {
        (52_i16 - (ver_boost as i16 / 2)).clamp(28, 70) as u8
    };

    let prompt = meta.prices.prompt.unwrap_or(50_000);
    // Cheaper prompt → higher efficiency (rough log scale).
    let mut cost_efficiency = cost_efficiency_from_prompt(prompt);
    if non_reasoning || fast {
        cost_efficiency = cost_efficiency.saturating_add(8).min(98);
    }

    let tools = match meta.tool_calling {
        Some(true) => 82,
        Some(false) => 15,
        None => {
            if non_reasoning {
                70
            } else {
                60
            }
        }
    };

    let mut code = if non_reasoning {
        34
    } else if meta.tool_calling == Some(false) {
        (reasoning as u16 * 5 / 10) as u8
    } else {
        ((reasoning as u16 * 9 / 10) as u8).saturating_add(ver_boost / 3)
    };

    let vision = if meta
        .input_modalities
        .as_ref()
        .is_some_and(|m| m.iter().any(|x| x == "image"))
    {
        80
    } else {
        0
    };

    let long_context = match meta.context_length.unwrap_or(0) {
        0 => 40,
        c if c >= 1_000_000 => 95,
        c if c >= 256_000 => 80,
        c if c >= 128_000 => 65,
        c if c >= 32_000 => 45,
        _ => 25,
    };

    // Mild defaults so empty listing rows still rank.
    if meta.is_placeholder() {
        reasoning = reasoning.max(45);
        speed = speed.max(50);
        code = code.max(40);
    }

    Caps {
        reasoning,
        code,
        speed,
        cost_efficiency,
        vision,
        long_context,
        tools,
    }
}

fn cost_efficiency_from_prompt(prompt_cents_per_100m: u64) -> u8 {
    // Typical Grok prompt ticks are ~10k–100k+. Map into 20..=95.
    let p = prompt_cents_per_100m.max(1) as f64;
    let score = 100.0 - (p.ln() * 6.0);
    score.clamp(20.0, 95.0) as u8
}

fn version_boost(id: &str) -> u8 {
    let Some((_, v)) = family(id) else {
        return 0;
    };
    // grok-4.3 → small; grok-4.7+ → larger. Cap so seed can still differentiate.
    let minor = v.get(1).copied().unwrap_or(0);
    let major = v.first().copied().unwrap_or(0);
    ((major.saturating_mul(2) + minor) as u8).min(20)
}

/// Curated research overlays. Returns true when any axis was changed by seed.
/// Keep this table short; bump [`SEED_VERSION`] when it changes. Known-family
/// rows may lower an axis (e.g. flagship code vs a coding specialist).
pub fn apply_seed(id: &str, caps: &mut Caps) -> bool {
    let lower = id.to_ascii_lowercase();
    let stem = grokhub_core::model_registry::model_id_stem(id).to_ascii_lowercase();
    let mut hit = false;
    let set = |slot: &mut u8, to: u8, hit: &mut bool| {
        if *slot != to {
            *slot = to;
            *hit = true;
        }
    };
    let raise = |slot: &mut u8, to: u8, hit: &mut bool| {
        if to > *slot {
            *slot = to;
            *hit = true;
        }
    };

    if lower.contains("non-reasoning") {
        raise(&mut caps.speed, 96, &mut hit);
        raise(&mut caps.cost_efficiency, 93, &mut hit);
        if caps.reasoning > 28 {
            caps.reasoning = 22;
            hit = true;
        }
        return hit;
    }

    // Known coding-strong mid family (xAI grok-4.3 line).
    if stem == "grok-4.3" || stem.starts_with("grok-4.3-") {
        set(&mut caps.code, 99, &mut hit);
        set(&mut caps.reasoning, 80, &mut hit);
        set(&mut caps.tools, 95, &mut hit);
        return hit;
    }

    // Newest flagship reasoning lines: depth over raw code. Newer minor wins.
    if stem == "grok-4.8" || stem.starts_with("grok-4.8-") {
        set(&mut caps.reasoning, 98, &mut hit);
        set(&mut caps.code, 81, &mut hit);
        set(&mut caps.long_context, 88, &mut hit);
        set(&mut caps.tools, 91, &mut hit);
        return hit;
    }
    if stem == "grok-4.7" || stem.starts_with("grok-4.7-") {
        set(&mut caps.reasoning, 96, &mut hit);
        set(&mut caps.code, 80, &mut hit);
        set(&mut caps.long_context, 85, &mut hit);
        set(&mut caps.tools, 90, &mut hit);
        return hit;
    }

    if stem == "grok-4.6" || stem.starts_with("grok-4.6-") {
        set(&mut caps.reasoning, 90, &mut hit);
        set(&mut caps.code, 86, &mut hit);
        return hit;
    }

    if stem == "grok-4.5" || stem.starts_with("grok-4.5-") {
        set(&mut caps.reasoning, 84, &mut hit);
        set(&mut caps.code, 82, &mut hit);
        raise(&mut caps.speed, 55, &mut hit);
        return hit;
    }

    // Dated 4.20 reasoning: solid mid / balanced workhorse.
    if stem.contains("4.20") && lower.contains("reasoning") && !lower.contains("non-reasoning") {
        set(&mut caps.reasoning, 74, &mut hit);
        set(&mut caps.code, 68, &mut hit);
        set(&mut caps.speed, 68, &mut hit);
        raise(&mut caps.cost_efficiency, 78, &mut hit);
        return hit;
    }

    if lower.contains("-fast") {
        raise(&mut caps.speed, 90, &mut hit);
        raise(&mut caps.cost_efficiency, 88, &mut hit);
    }

    hit
}

#[cfg(test)]
mod tests {
    use super::*;
    use grokhub_core::model_registry::{Listing, ModelState, Prices, SourceKind};

    fn meta_priced(id: &str, prompt: u64, reasoning: bool) -> ModelMeta {
        ModelMeta {
            id: id.into(),
            context_length: Some(256_000),
            prices: Prices {
                prompt: Some(prompt),
                completion: Some(prompt * 5),
                ..Prices::default()
            },
            efforts: reasoning.then(|| vec!["low".into(), "medium".into(), "high".into()]),
            tool_calling: Some(true),
            input_modalities: Some(vec!["text".into(), "image".into()]),
            api_shape: Some("responses".into()),
            ..ModelMeta::default()
        }
    }

    #[test]
    fn seed_differentiates_code_specialist_from_flagship_reasoning() {
        let mut c43 = derive_caps(&meta_priced("grok-4.3", 40_000, true));
        let mut c47 = derive_caps(&meta_priced("grok-4.7", 80_000, true));
        assert!(apply_seed("grok-4.3", &mut c43));
        assert!(apply_seed("grok-4.7", &mut c47));
        assert!(
            c43.code > c47.code,
            "4.3 code {} vs 4.7 {}",
            c43.code,
            c47.code
        );
        assert!(
            c47.reasoning > c43.reasoning,
            "4.7 reasoning {} vs 4.3 {}",
            c47.reasoning,
            c43.reasoning
        );
    }

    #[test]
    fn class_needs_send_light_work_to_speed_and_hard_work_to_reasoning() {
        assert!(!class_need("background:triage").require_reasoning);
        assert!(class_need("background:memory").require_reasoning);
        assert!(class_need("plan").require_reasoning);
        assert!(class_need("repair:diagnose").require_reasoning);
        assert_eq!(class_need("repair:diagnose").cost, 0);
        assert_eq!(class_need("repair:diagnose").speed, 0);
        assert_eq!(
            BaselineLane::of("background:memory"),
            BaselineLane::Workhorse
        );
        assert_eq!(BaselineLane::of("repair:diagnose"), BaselineLane::Flagship);
        assert_eq!(BaselineLane::of("code:multi-file"), BaselineLane::Code);
        assert!(class_need("code:multi-file").code >= class_need("plan").code);
        assert!(class_need("debug").reasoning >= class_need("chat:default").reasoning);
        assert!(class_need("chat:quick").speed >= 8);
        assert!(class_need("chat:quick").cost >= 8);
    }

    #[test]
    fn baseline_score_reserves_flagship_and_prefers_mid_workhorse() {
        let mut non = derive_caps(&meta_priced("grok-4.20-0309-non-reasoning", 8_000, false));
        let mut mid = derive_caps(&meta_priced("grok-4.20-0309-reasoning", 20_000, true));
        let mut c43 = derive_caps(&meta_priced("grok-4.3", 40_000, true));
        let mut c47 = derive_caps(&meta_priced("grok-4.7", 80_000, true));
        apply_seed("grok-4.20-0309-non-reasoning", &mut non);
        apply_seed("grok-4.20-0309-reasoning", &mut mid);
        apply_seed("grok-4.3", &mut c43);
        apply_seed("grok-4.7", &mut c47);
        let max_r = plan_max_reasoning([&non, &mid, &c43, &c47]);

        let triage = class_need("background:triage");
        assert!(fit_score(&non, &triage) > fit_score(&c47, &triage));

        let memory = class_need("background:memory");
        assert!(
            baseline_score(&mid, &memory, BaselineLane::Workhorse, max_r)
                > baseline_score(&c47, &memory, BaselineLane::Workhorse, max_r)
        );
        // SuperGrok-shaped (no mid): workhorse prefers coding mid over flagship.
        let max_sg = plan_max_reasoning([&c43, &c47]);
        assert!(
            baseline_score(&c43, &memory, BaselineLane::Workhorse, max_sg)
                > baseline_score(&c47, &memory, BaselineLane::Workhorse, max_sg)
        );

        let multi = class_need("code:multi-file");
        assert!(fit_score(&c43, &multi) > fit_score(&mid, &multi));

        let repair = class_need("repair:diagnose");
        assert!(
            baseline_score(&c47, &repair, BaselineLane::Flagship, max_r)
                > baseline_score(&c43, &repair, BaselineLane::Flagship, max_r)
        );
    }

    #[test]
    fn catalog_refresh_is_stable_for_same_registry_hash() {
        let mut reg = Registry::default();
        let listing = Listing::new(
            SourceKind::XaiApi,
            vec![meta_priced("grok-4.7", 80_000, true)],
        );
        reg.apply_refresh(&[listing], &[], 1);
        let dir = std::env::temp_dir().join(format!(
            "grokhub-cap-catalog-{}-{}",
            std::process::id(),
            now_unique()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let a = refresh_catalog(&dir, &reg, false, 100);
        let b = refresh_catalog(&dir, &reg, false, 200);
        assert_eq!(a.built_at, b.built_at);
        assert_eq!(a.entries["grok-4.7"].caps, b.entries["grok-4.7"].caps);
        assert_eq!(reg.get("grok-4.7").unwrap().state, ModelState::Live);
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn now_unique() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0)
    }
}
