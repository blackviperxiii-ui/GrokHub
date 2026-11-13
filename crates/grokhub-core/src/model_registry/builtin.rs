//! The built-in model profiles: what the router knows about every cloud
//! model Loadout reaches before it has measured anything — capability
//! priors, roles, intelligence tier, prices, window, modalities and tool
//! support. The rows live in `data/model_profiles.toml`, ship with the
//! app, and are kept current by `.github/scripts/model_profiles.py`
//! (see `docs/model-profiles.md`). Local models keep their own profiles in
//! `data/local_catalog.toml`.
//!
//! A listing's own figures win over a row, except the fields a row pins.
//! A user's `{config}/models/profile_overrides.toml` (same schema) wins
//! over both: its fields replace the bundled row's and count as pinned.
//!
//! Prices are USD cents per 100M tokens ($1 per 1M is 10_000).

use std::path::{Path, PathBuf};
use std::sync::{OnceLock, RwLock};

use serde::Deserialize;

use super::record::{model_id_stem, CapabilityScores, DataPolicy, ModelCard, Prices};

const BUNDLED: &str = include_str!("../../data/model_profiles.toml");
pub const SCHEMA: u32 = 1;
/// The user's override file, under `{config}/models/`.
pub const OVERRIDE_FILE: &str = "profile_overrides.toml";

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Caps {
    pub chat: Option<u8>,
    pub code: Option<u8>,
    pub tools: Option<u8>,
    pub reasoning: Option<u8>,
    pub long_context: Option<u8>,
    pub vision: Option<u8>,
    pub audio: Option<u8>,
}

/// The Grok lane picker's seed scores (`route::capabilities`): set on the
/// derived lane caps for this family; `speed_floor` only raises speed.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Lane {
    pub reasoning: Option<u8>,
    pub code: Option<u8>,
    pub long_context: Option<u8>,
    pub tools: Option<u8>,
    pub speed_floor: Option<u8>,
}

/// One model's profile. Every field but `key` may be left out; a missing
/// field is unknown, never zero.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    /// The wire id: `grok-4.7`, `claude-sonnet-5.5`, `deepseek/deepseek-v4.1-flash-1017`.
    pub key: String,
    pub name: Option<String>,
    /// `xai`, `anthropic` or `openrouter`.
    pub provider: Option<String>,
    /// The same model's OpenRouter id, for the updater's cross-check.
    pub mirror: Option<String>,
    #[serde(default)]
    pub aliases: Vec<String>,
    /// `frontier`, `flagship`, `workhorse`, `fast`, `paid` or `free`.
    pub tier: Option<String>,
    /// Artificial Analysis Intelligence Index (0-100), where published.
    pub intelligence: Option<u8>,
    pub roles: Option<Vec<String>>,
    pub caps: Option<Caps>,
    pub lane: Option<Lane>,
    pub prompt: Option<u64>,
    pub cached: Option<u64>,
    pub cache_write: Option<u64>,
    pub completion: Option<u64>,
    pub prompt_long: Option<u64>,
    pub completion_long: Option<u64>,
    pub long_threshold: Option<u64>,
    pub context: Option<u64>,
    pub max_output: Option<u64>,
    pub input: Option<Vec<String>>,
    pub output: Option<Vec<String>>,
    pub tools: Option<bool>,
    pub structured: Option<bool>,
    pub reasoning: Option<bool>,
    /// Reference only: efforts are applied from a provider's own listing.
    pub efforts: Option<Vec<String>>,
    pub default_effort: Option<String>,
    /// Training data cutoff, `YYYY-MM`.
    pub cutoff: Option<String>,
    pub trains: Option<bool>,
    pub retains: Option<bool>,
    pub moderated: Option<bool>,
    pub expires: Option<String>,
    pub hosts: Option<u32>,
    pub probation: Option<bool>,
    pub speed_tps: Option<u64>,
    pub source: Option<String>,
    /// The day a person or the updater last checked the facts.
    pub verified: Option<String>,
    /// Fields that are judgment, not a provider's stated fact.
    #[serde(default)]
    pub estimate: Vec<String>,
    /// Fields that beat a listing's own figure.
    #[serde(default)]
    pub pin: Vec<String>,
    /// The day the provider stopped listing it.
    pub gone_since: Option<String>,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct Doc {
    schema: u32,
    #[serde(default)]
    #[allow(dead_code)]
    updated: Option<String>,
    #[serde(default)]
    model: Vec<Profile>,
}

/// Parse a profiles document (the bundled file or a user's overrides).
pub fn parse(text: &str) -> Result<Vec<Profile>, String> {
    let doc: Doc = toml::from_str(text).map_err(|e| e.to_string())?;
    if doc.schema != SCHEMA {
        return Err(format!("schema {} (this build reads {SCHEMA})", doc.schema));
    }
    Ok(doc.model)
}

fn bundled_rows() -> &'static [Profile] {
    static ROWS: OnceLock<Vec<Profile>> = OnceLock::new();
    // The file is compiled in and its parse is tested: an empty table here
    // would only mean every model starts from live learning.
    ROWS.get_or_init(|| parse(BUNDLED).unwrap_or_default())
}

/// Merge override rows onto base rows: an override's set fields replace
/// the base row's and join its pins; an unknown key adds a row.
pub fn apply_overrides(base: &[Profile], overrides: &[Profile]) -> Vec<Profile> {
    let mut out = base.to_vec();
    for o in overrides {
        match out.iter_mut().find(|p| p.key == o.key) {
            Some(p) => p.overlay(o),
            None => {
                let mut row = o.clone();
                row.pin = row.set_fields();
                out.push(row);
            }
        }
    }
    out
}

struct Installed {
    rows: Vec<Profile>,
    error: Option<String>,
}

fn installed() -> &'static RwLock<Option<Installed>> {
    static CELL: OnceLock<RwLock<Option<Installed>>> = OnceLock::new();
    CELL.get_or_init(|| RwLock::new(None))
}

pub fn override_path(config_dir: &Path) -> PathBuf {
    super::store::models_dir(config_dir).join(OVERRIDE_FILE)
}

/// Read the user's override file (if any) and make it part of every
/// lookup from now on. A file that doesn't parse is ignored as a whole
/// and its error kept for [`override_error`]; the bundled rows stand.
pub fn install_overrides(config_dir: &Path) {
    let path = override_path(config_dir);
    let (rows, error) = match std::fs::read_to_string(&path) {
        Ok(text) => match parse(&text) {
            Ok(over) => (apply_overrides(bundled_rows(), &over), None),
            Err(e) => (
                bundled_rows().to_vec(),
                Some(format!("{}: {e}", path.display())),
            ),
        },
        Err(_) => (bundled_rows().to_vec(), None),
    };
    if let Ok(mut cell) = installed().write() {
        *cell = Some(Installed { rows, error });
    }
}

/// Why the user's override file was ignored, if it was.
pub fn override_error() -> Option<String> {
    installed().read().ok()?.as_ref()?.error.clone()
}

/// Every profile in effect: bundled rows with the user's overrides.
pub fn all() -> Vec<Profile> {
    if let Ok(cell) = installed().read() {
        if let Some(i) = cell.as_ref() {
            return i.rows.clone();
        }
    }
    bundled_rows().to_vec()
}

/// The bundled rows alone, before any override.
pub fn bundled() -> &'static [Profile] {
    bundled_rows()
}

/// Lowercase, `-latest` and a trailing 8-digit date stamp dropped.
fn trim_suffixes(id: &str) -> String {
    let mut s = id.trim().to_ascii_lowercase();
    loop {
        if let Some(rest) = s.strip_suffix("-latest") {
            s = rest.to_string();
            continue;
        }
        if let Some((head, tail)) = s.rsplit_once('-') {
            if tail.len() == 8 && tail.bytes().all(|b| b.is_ascii_digit()) {
                s = head.to_string();
                continue;
            }
        }
        return s;
    }
}

/// `claude-sonnet-5-5` -> `claude-sonnet-5.5`: Anthropic's API writes the
/// version with a hyphen, OpenRouter and the profiles with a dot.
fn dotted_version(id: &str) -> Option<String> {
    let (head, minor) = id.rsplit_once('-')?;
    let (stem, major) = head.rsplit_once('-')?;
    let digits = |s: &str| !s.is_empty() && s.len() <= 2 && s.bytes().all(|b| b.is_ascii_digit());
    (digits(major) && digits(minor)).then(|| format!("{stem}-{major}.{minor}"))
}

/// The names a registry id may go by, most specific first.
fn candidates(id: &str) -> Vec<String> {
    let wire = super::discover::split_provider_model(id).map_or(id, |(_, m)| m);
    let wire = wire.to_ascii_lowercase();
    let mut out = vec![wire.clone()];
    let last = wire.rsplit('/').next().unwrap_or(&wire).to_string();
    for base in [wire.clone(), last] {
        let trimmed = trim_suffixes(&base);
        for c in [
            Some(base.clone()),
            Some(trimmed.clone()),
            dotted_version(&trimmed),
            Some(model_id_stem(&base)),
        ]
        .into_iter()
        .flatten()
        {
            if !out.contains(&c) {
                out.push(c);
            }
        }
    }
    out
}

fn find_in<'a>(rows: &'a [Profile], id: &str) -> Option<&'a Profile> {
    let names = |p: &'a Profile| std::iter::once(&p.key).chain(p.aliases.iter());
    for (i, c) in candidates(id).iter().enumerate() {
        // Only the full wire id may match a key with a slash; the shorter
        // forms match slash-less keys (Claude and xAI rows) and aliases.
        let hit = rows
            .iter()
            .find(|p| names(p).any(|n| n.eq_ignore_ascii_case(c) && (i == 0 || !n.contains('/'))));
        if hit.is_some() {
            return hit;
        }
    }
    None
}

/// The profile for a registry id (`grok-4.7-0414`, `anthropic/claude-sonnet-5-5`,
/// `openrouter/anthropic/claude-sonnet-5.5`, `openrouter/x-ai/grok-4.7`, …).
pub fn get(id: &str) -> Option<Profile> {
    find_in(&all(), id).cloned()
}

/// The Grok lane seed for an id: its own row's, or a family row's when
/// the id's stem extends a key (`grok-4.7-fast` takes `grok-4.7`'s).
pub fn lane_for(id: &str) -> Option<Lane> {
    let rows = all();
    if let Some(p) = find_in(&rows, id) {
        return p.lane.clone();
    }
    let stem = model_id_stem(id).to_ascii_lowercase();
    rows.iter()
        .filter(|p| p.lane.is_some() && stem.starts_with(&format!("{}-", p.key)))
        .max_by_key(|p| p.key.len())
        .and_then(|p| p.lane.clone())
}

impl Profile {
    /// Is this field pinned (does it beat a listing)?
    pub fn pins(&self, field: &str) -> bool {
        self.pin.iter().any(|f| f == field)
    }

    pub fn is_estimate(&self, field: &str) -> bool {
        self.estimate.iter().any(|f| f == field)
    }

    pub fn prices(&self) -> Prices {
        Prices {
            prompt: self.prompt,
            cached: self.cached,
            cache_write: self.cache_write,
            completion: self.completion,
            prompt_long: self.prompt_long,
            completion_long: self.completion_long,
            long_context_threshold: self.long_threshold,
        }
    }

    pub fn card(&self) -> ModelCard {
        let c = self.caps.clone().unwrap_or_default();
        ModelCard {
            route: None,
            caps: CapabilityScores {
                chat: c.chat,
                code: c.code,
                tools: c.tools,
                reasoning: c.reasoning,
                long_context: c.long_context,
                vision: c.vision,
                audio: c.audio,
            },
            roles: self.roles.clone().unwrap_or_default(),
            data_policy: DataPolicy {
                trains: self.trains,
                retains: self.retains,
                moderated: self.moderated,
            },
            expires: self.expires.clone(),
            hosts: self.hosts,
            probation: self.probation.unwrap_or(false),
            speed_tps: self.speed_tps,
            ttft_ms: None,
            intelligence: self.intelligence,
            tier: self.tier.clone(),
        }
    }

    /// The names of the fields this row sets (beyond its key), as the
    /// file spells them. Override rows pin exactly these.
    pub fn set_fields(&self) -> Vec<String> {
        let mut out = Vec::new();
        macro_rules! check {
            ($($f:ident),*) => {$(
                if self.$f.is_some() { out.push(stringify!($f).to_string()); }
            )*};
        }
        check!(
            name,
            provider,
            mirror,
            tier,
            intelligence,
            roles,
            caps,
            lane,
            prompt,
            cached,
            cache_write,
            completion,
            prompt_long,
            completion_long,
            long_threshold,
            context,
            max_output,
            input,
            output,
            tools,
            structured,
            reasoning,
            efforts,
            default_effort,
            cutoff,
            trains,
            retains,
            moderated,
            expires,
            hosts,
            probation,
            speed_tps,
            source,
            verified,
            gone_since,
            note
        );
        if !self.aliases.is_empty() {
            out.push("aliases".into());
        }
        out
    }

    /// Lay an override row over this one: its set fields replace ours and
    /// are pinned.
    fn overlay(&mut self, o: &Profile) {
        macro_rules! take {
            ($($f:ident),*) => {$(
                if o.$f.is_some() { self.$f.clone_from(&o.$f); }
            )*};
        }
        take!(
            name,
            provider,
            mirror,
            tier,
            intelligence,
            roles,
            caps,
            lane,
            prompt,
            cached,
            cache_write,
            completion,
            prompt_long,
            completion_long,
            long_threshold,
            context,
            max_output,
            input,
            output,
            tools,
            structured,
            reasoning,
            efforts,
            default_effort,
            cutoff,
            trains,
            retains,
            moderated,
            expires,
            hosts,
            probation,
            speed_tps,
            source,
            verified,
            gone_since,
            note
        );
        for a in &o.aliases {
            if !self.aliases.contains(a) {
                self.aliases.push(a.clone());
            }
        }
        for f in o.set_fields().into_iter().chain(o.pin.iter().cloned()) {
            if !self.pin.contains(&f) {
                self.pin.push(f);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every field name a row may list in `estimate` or `pin`.
    const FIELD_NAMES: &[&str] = &[
        "name",
        "provider",
        "mirror",
        "aliases",
        "tier",
        "intelligence",
        "roles",
        "caps",
        "lane",
        "prompt",
        "cached",
        "cache_write",
        "completion",
        "prompt_long",
        "completion_long",
        "long_threshold",
        "context",
        "max_output",
        "input",
        "output",
        "tools",
        "structured",
        "reasoning",
        "efforts",
        "default_effort",
        "cutoff",
        "trains",
        "retains",
        "moderated",
        "expires",
        "hosts",
        "probation",
        "speed_tps",
        "source",
        "verified",
        "gone_since",
        "note",
    ];

    #[test]
    fn the_bundled_file_parses_and_every_row_is_sound() {
        let rows = bundled();
        assert!(rows.len() >= 40, "{} rows", rows.len());
        let mut keys = std::collections::BTreeSet::new();
        for p in rows {
            assert!(keys.insert(p.key.clone()), "duplicate key {}", p.key);
            for a in &p.aliases {
                assert!(keys.insert(a.clone()), "{}: alias {a} reused", p.key);
            }
            let provider = p.provider.as_deref().unwrap_or("");
            assert!(
                ["xai", "anthropic", "openrouter"].contains(&provider),
                "{}: provider {provider:?}",
                p.key
            );
            assert!(
                p.source.is_some() && p.verified.is_some(),
                "{}: source and date",
                p.key
            );
            for f in p.estimate.iter().chain(&p.pin) {
                assert!(
                    FIELD_NAMES.contains(&f.as_str()),
                    "{}: unknown field {f}",
                    p.key
                );
            }
            if p.caps.is_some() {
                assert!(
                    p.is_estimate("caps"),
                    "{}: caps are judgment, mark them",
                    p.key
                );
            }
            if let (Some(a), Some(b)) = (p.prompt, p.prompt_long) {
                assert!(b >= a, "{}: long rate below short", p.key);
            }
            for v in [p.intelligence].into_iter().flatten() {
                assert!(v <= 100, "{}", p.key);
            }
            if provider == "openrouter" && p.key.ends_with(":free") {
                assert_eq!((p.prompt, p.completion), (Some(0), Some(0)), "{}", p.key);
            }
        }
    }

    #[test]
    fn ids_match_across_the_forms_each_provider_writes() {
        let key = |id: &str| get(id).map(|p| p.key).unwrap_or_default();
        // Anthropic's API (hyphen), OpenRouter (dot), with prefixes and dates.
        for id in [
            "anthropic/claude-sonnet-5-5",
            "anthropic/claude-sonnet-5.5",
            "openrouter/anthropic/claude-sonnet-5.5",
            "claude-sonnet-5-5-20261001",
        ] {
            assert_eq!(key(id), "claude-sonnet-5.5", "{id}");
        }
        assert_eq!(key("anthropic/claude-fable-5-2"), "claude-fable-5.2");
        // xAI: dated, -latest and aliases.
        assert_eq!(key("grok-4.7-0414"), "grok-4.7");
        assert_eq!(key("grok-4.3-latest"), "grok-4.3");
        assert_eq!(key("grok-code-fast-1"), "grok-build-0.1");
        // OpenRouter's Grok is its own row, not the direct one.
        assert_eq!(key("openrouter/x-ai/grok-4.7"), "x-ai/grok-4.7");
        assert_eq!(
            key("openrouter/nvidia/nemotron-3.5-super-vl:free"),
            "nvidia/nemotron-3.5-super-vl:free"
        );
        // Unknown and look-alike ids match nothing.
        assert_eq!(get("grok-4.7-fast"), None);
        assert_eq!(get("openrouter/openrouter/auto"), None);
        assert_eq!(get("anthropic/claude-sonnet-4"), None);
        assert_eq!(get("openrouter/other/claude-sonnet-5.5-distill"), None);
    }

    #[test]
    fn lanes_follow_the_family_and_skip_unknown_lines() {
        let l = lane_for("grok-4.7-fast").unwrap();
        assert_eq!((l.reasoning, l.code), (Some(96), Some(80)));
        assert_eq!(lane_for("grok-4.3-0201").unwrap().code, Some(99));
        assert_eq!(lane_for("grok-4.5").unwrap().speed_floor, Some(55));
        assert_eq!(lane_for("grok-4.20-0309-reasoning"), None);
        assert_eq!(lane_for("grok-4.8"), None);
    }

    #[test]
    fn an_override_replaces_fields_pins_them_and_can_add_a_row() {
        let over = parse(
            r#"
            schema = 1
            [[model]]
            key = "claude-sonnet-5.5"
            prompt = 15000
            roles = ["hard_code"]

            [[model]]
            key = "acme/new-model"
            provider = "openrouter"
            context = 64000
            "#,
        )
        .unwrap();
        let rows = apply_overrides(bundled(), &over);
        let sonnet = rows.iter().find(|p| p.key == "claude-sonnet-5.5").unwrap();
        assert_eq!(sonnet.prompt, Some(15_000));
        assert_eq!(sonnet.completion, Some(100_000), "fields it left out stay");
        assert!(sonnet.pins("prompt") && sonnet.pins("roles") && !sonnet.pins("completion"));
        let new = rows.iter().find(|p| p.key == "acme/new-model").unwrap();
        assert_eq!(new.context, Some(64_000));
        assert!(new.pins("context"));
    }

    #[test]
    fn a_bad_override_file_is_refused_whole() {
        assert!(parse("schema = 1\n[[model]]\nkey = \"x\"\nprise = 1\n").is_err());
        assert!(parse("schema = 2\n").is_err());
    }
}
