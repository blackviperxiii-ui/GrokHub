//! Router R0 model registry: which Grok models exist, which this plan or key can
//! use, and which are healthy. Sources refresh it (see [`RefreshClock`]); real
//! traffic moves health ([`health`]); nothing here touches the network.
//! Stored at `{config}/models/registry.json`.

pub mod builtin;
pub mod card;
pub mod cost_class;
pub mod discover;
pub mod entitlement;
pub mod health;
pub mod probation;
pub mod probe;
pub mod profile;
pub mod quota;
pub mod record;
pub mod seed;
pub mod store;

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub use cost_class::{CostClass, RouteOpts};
pub use discover::{
    gb_listing, parse_anthropic_catalog, parse_model_row, parse_openai_compatible_catalog,
    parse_xai_catalog, provider_model_id, split_provider_model, CatalogSource, Listing,
};
pub use entitlement::{tier_notice, Credential, Entitlement};
pub use health::{classify_status, CallStatus, Observation};
pub use record::{
    model_id_stem, Breaker, CapabilityScores, DataPolicy, ModelCard, ModelMeta, ModelRecord,
    ModelState, Notice, Prices, Reliability, Route, SourceKind,
};

pub const REGISTRY_SCHEMA: u32 = 1;
/// First refresh after start, once the cabin is idle.
pub const FIRST_REFRESH_MS: u64 = 30_000;
/// Then every 6 hours.
pub const REFRESH_EVERY_MS: u64 = 6 * 60 * 60 * 1000;
/// Gone from every source this long: pruned. A retired or pruned row stays
/// as a tombstone this long again, then is deleted.
pub const PRUNE_AFTER_MS: u64 = 30 * 24 * 60 * 60 * 1000;

/// The canonical effort ladder, low to high.
pub const EFFORT_LADDER: &[&str] = &["none", "minimal", "low", "medium", "high", "xhigh", "max"];

/// What a refresh changed. Each becomes one span event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum RegistryEvent {
    Added {
        id: String,
    },
    Removed {
        id: String,
    },
    /// `what` is any of `price`, `context`, `efforts`, `endpoint`, `notice`, `card`.
    Changed {
        id: String,
        what: Vec<String>,
    },
    State {
        id: String,
        from: ModelState,
        to: ModelState,
        reason: String,
    },
    Notice {
        text: String,
    },
    /// A tombstone's 30 days ran out: the row left `registry.json`.
    Deleted {
        id: String,
    },
    /// A new model's tryout moved: started, graduated, failed or retried.
    Probation {
        id: String,
        stage: probation::Stage,
        reason: String,
    },
    /// A routable model's announced expiry is close: routing to it has
    /// stopped (the scorer's expiry bar); logged once per date.
    Expiring {
        id: String,
        expires: String,
    },
}

impl RegistryEvent {
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Added { .. } => "added",
            Self::Removed { .. } => "removed",
            Self::Changed { .. } => "changed",
            Self::State { .. } => "state",
            Self::Notice { .. } => "notice",
            Self::Deleted { .. } => "deleted",
            Self::Probation { .. } => "probation",
            Self::Expiring { .. } => "expiring",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Registry {
    #[serde(default)]
    pub schema: u32,
    #[serde(default)]
    pub refreshed_ms: u64,
    #[serde(default)]
    pub refreshes: u64,
    /// Sha-256 over every record's content hash, for "did anything change".
    #[serde(default)]
    pub hash: String,
    #[serde(default)]
    pub entitlement: Entitlement,
    #[serde(default)]
    pub gb_version: Option<String>,
    #[serde(default)]
    pub models: BTreeMap<String, ModelRecord>,
}

fn changed_kinds(old: &ModelMeta, new: &ModelMeta) -> Vec<String> {
    let mut what = Vec::new();
    if old.prices != new.prices {
        what.push("price".to_string());
    }
    if (old.context_length, old.max_output) != (new.context_length, new.max_output) {
        what.push("context".into());
    }
    if (&old.efforts, &old.default_effort) != (&new.efforts, &new.default_effort) {
        what.push("efforts".into());
    }
    if old.api_shape != new.api_shape {
        what.push("endpoint".into());
    }
    if (&old.notice, old.deprecated) != (&new.notice, new.deprecated) {
        what.push("notice".into());
    }
    if old.card != new.card {
        what.push("card".into());
    }
    what
}

/// The state a fresh listing gives, and why.
fn listed_state(
    meta: &ModelMeta,
    api: bool,
    rec: &ModelRecord,
    now_ms: u64,
) -> (ModelState, String) {
    if meta
        .notice
        .as_ref()
        .is_some_and(Notice::is_critical_retired)
    {
        return (ModelState::Ghost, "Grok Build marked it retired.".into());
    }
    if api && meta.is_placeholder() {
        return (
            ModelState::Ghost,
            "Its listing has no prices, context or efforts.".into(),
        );
    }
    if meta.deprecated == Some(true) {
        return (
            ModelState::Retired,
            "Its source marks it deprecated.".into(),
        );
    }
    let strikes = rec
        .not_found
        .iter()
        .filter(|t| now_ms.saturating_sub(**t) <= health::STRIKE_WINDOW_MS)
        .count();
    if rec.state == ModelState::Ghost && strikes >= health::GHOST_STRIKES {
        return (rec.state, rec.reason.clone());
    }
    if matches!(
        rec.state,
        ModelState::Degraded | ModelState::Quarantined | ModelState::Probing
    ) {
        return (rec.state, rec.reason.clone());
    }
    (ModelState::Live, "In your model list.".into())
}

/// A call that answered OK this recently keeps a named model out of Ghost.
const ANSWERED_RECENTLY_MS: u64 = 24 * 60 * 60 * 1000;

fn answered_since(rec: &ModelRecord, since_ms: u64) -> bool {
    rec.health
        .recent
        .iter()
        .any(|m| m.ok && m.ts_ms >= since_ms)
}

impl Registry {
    /// The record for `id`, or for the listed model that carries `id` as an
    /// alias when `id` itself isn't listed (a pin on `grok-4.7-latest`).
    pub fn get(&self, id: &str) -> Option<&ModelRecord> {
        self.models.get(self.canonical(id))
    }

    /// `id`, or the listed model that names it as an alias.
    pub fn canonical<'a>(&'a self, id: &'a str) -> &'a str {
        let id = id.trim();
        if self
            .models
            .get(id)
            .is_some_and(|r| !r.sources.is_empty() && r.absent_since_ms == 0)
        {
            return id;
        }
        self.models
            .iter()
            .find(|(key, r)| {
                key.as_str() != id
                    && !r.sources.is_empty()
                    && r.absent_since_ms == 0
                    && r.meta.aliases.iter().any(|a| a == id)
            })
            .map_or(id, |(key, _)| key.as_str())
    }

    /// In your own fresh list right now.
    pub fn listed(&self, id: &str) -> bool {
        self.get(id)
            .is_some_and(|r| !r.sources.is_empty() && r.absent_since_ms == 0)
    }

    fn set_state(
        rec: &mut ModelRecord,
        to: ModelState,
        reason: String,
        now_ms: u64,
        events: &mut Vec<RegistryEvent>,
    ) {
        if rec.state != to {
            events.push(RegistryEvent::State {
                id: rec.meta.id.clone(),
                from: rec.state,
                to,
                reason: reason.clone(),
            });
        }
        if !to.tombstone() {
            rec.tombstone_ms = 0;
        } else if rec.tombstone_ms == 0 {
            rec.tombstone_ms = now_ms;
        }
        rec.state = to;
        rec.reason = reason;
    }

    /// Fold fresh listings in. `named` is every id a pin, default, skill or
    /// automation names. No listing (every source failed) changes nothing.
    /// A source that doesn't answer keeps the models it listed before: a
    /// record only goes stale once every source that ever listed it has
    /// checked in without it.
    pub fn apply_refresh(
        &mut self,
        listings: &[Listing],
        named: &[String],
        now_ms: u64,
    ) -> Vec<RegistryEvent> {
        let mut events = Vec::new();
        if listings.is_empty() {
            return events;
        }
        let mut ordered: Vec<&Listing> = listings.iter().collect();
        ordered.sort_by_key(|l| l.kind);
        // The kinds that answered this refresh. A record only goes stale
        // once every source that ever listed it checked in without it: a
        // source that failed or stayed silent keeps its models, so one
        // source's bad day can't wipe another's list.
        let answered: Vec<SourceKind> = ordered.iter().filter_map(|l| l.kind).collect();
        let mut merged: BTreeMap<String, (ModelMeta, Vec<SourceKind>)> = BTreeMap::new();
        let mut tier = None;
        for listing in ordered {
            if tier.is_none() {
                tier.clone_from(&listing.tier);
            }
            if listing.gb_version.is_some() {
                self.gb_version.clone_from(&listing.gb_version);
            }
            for meta in &listing.models {
                let entry = merged
                    .entry(meta.id.clone())
                    .or_insert_with(|| (meta.clone(), Vec::new()));
                if entry.1.is_empty() {
                    entry.0 = meta.clone();
                } else {
                    entry.0.merge_from(meta);
                }
                if let Some(kind) = listing.kind {
                    if !entry.1.contains(&kind) {
                        entry.1.push(kind);
                    }
                }
            }
        }
        if let Some(text) = tier_notice(self.entitlement.tier.as_deref(), tier.as_deref()) {
            events.push(RegistryEvent::Notice { text });
        }
        if tier.is_some() {
            self.entitlement.tier = tier;
        }
        // The providers that had listed models before this refresh: a model
        // one of them starts listing now is new, and goes on a tryout. A
        // provider's first listing is its baseline, not a batch of new models.
        let known_providers: std::collections::BTreeSet<String> = self
            .models
            .iter()
            .filter(|(_, r)| r.sources.iter().any(|k| k.is_new_provider()))
            .filter_map(|(id, _)| id.split_once('/').map(|(p, _)| p.to_string()))
            .collect();
        for (id, (meta, sources)) in &mut merged {
            let api = sources.contains(&SourceKind::XaiApi);
            if !api {
                // No xAI API listing describes it (a Grok Build sign-in lists bare
                // ids): the known prices and window stand in. An API row keeps its own.
                seed::fill(meta);
            }
            // The model card: stamp the route and fill the priors the
            // bundled table ships (Claude's prices among them — its own
            // listing names none). A listing's own figures stay.
            card::fill(meta, sources);
            // The tryout: kept as it stands, or started for a model the
            // table marks as on probation or a known provider newly lists.
            // Its stage, not the table's prior, is what the card says.
            let fresh_from_provider = sources.iter().any(|k| k.is_new_provider())
                && id
                    .split_once('/')
                    .is_some_and(|(p, _)| known_providers.contains(p));
            let trial = match self.models.get(id) {
                Some(rec) => rec.probation.clone().or_else(|| {
                    meta.card
                        .probation
                        .then(|| probation::Probation::start(now_ms))
                }),
                None => (meta.card.probation || fresh_from_provider)
                    .then(|| probation::Probation::start(now_ms)),
            };
            let had = self.models.get(id).is_some_and(|r| r.probation.is_some());
            if let Some(t) = &trial {
                meta.card.probation = t.active();
                if !had {
                    events.push(RegistryEvent::Probation {
                        id: id.clone(),
                        stage: t.stage,
                        reason: t.reason.clone(),
                    });
                }
            }
            let hash = meta.content_hash();
            let rec = match self.models.get_mut(id) {
                Some(rec) => {
                    let what = changed_kinds(&rec.meta, meta);
                    if !what.is_empty() {
                        events.push(RegistryEvent::Changed {
                            id: id.clone(),
                            what,
                        });
                    }
                    rec
                }
                None => {
                    events.push(RegistryEvent::Added { id: id.clone() });
                    self.models
                        .entry(id.clone())
                        .or_insert_with(|| ModelRecord {
                            first_seen_ms: now_ms,
                            ..ModelRecord::default()
                        })
                }
            };
            rec.meta = meta.clone();
            rec.probation = trial;
            rec.sources = sources.clone();
            rec.content_hash = hash;
            rec.absent_since_ms = 0;
            let (to, reason) = listed_state(meta, api, rec, now_ms);
            Self::set_state(rec, to, reason, now_ms, &mut events);
        }
        // An id the fresh list only carries as another model's alias.
        let aliased: Vec<&str> = merged
            .values()
            .flat_map(|(meta, _)| meta.aliases.iter().map(String::as_str))
            .filter(|a| !merged.contains_key(*a))
            .collect();
        let stale: Vec<String> = self
            .models
            .keys()
            .filter(|id| aliased.contains(&id.as_str()))
            .cloned()
            .collect();
        for id in stale {
            // A bare record a call or pin made for an alias: the listed model stands for it.
            self.models.remove(&id);
            events.push(RegistryEvent::Deleted { id });
        }
        for (id, rec) in self.models.iter_mut() {
            if merged.contains_key(id) {
                continue;
            }
            if !rec.sources.is_empty() && rec.sources.iter().any(|k| !answered.contains(k)) {
                // One of its sources never answered: its list stands.
                continue;
            }
            let was_absent = rec.absent_since_ms != 0 || rec.sources.is_empty();
            if rec.absent_since_ms == 0 {
                rec.absent_since_ms = now_ms;
            }
            let named_here = named.iter().any(|n| n.trim() == id);
            if named_here
                && was_absent
                && answered_since(rec, now_ms.saturating_sub(ANSWERED_RECENTLY_MS))
            {
                Self::set_state(
                    rec,
                    ModelState::Live,
                    "It isn't listed, but it answered in the last day.".into(),
                    now_ms,
                    &mut events,
                );
            } else if named_here && was_absent {
                Self::set_state(
                    rec,
                    ModelState::Ghost,
                    "A pin, default, skill or automation names it, but no source lists it.".into(),
                    now_ms,
                    &mut events,
                );
            } else if now_ms.saturating_sub(rec.absent_since_ms) >= PRUNE_AFTER_MS {
                Self::set_state(
                    rec,
                    ModelState::Pruned,
                    "No source has listed it for 30 days.".into(),
                    now_ms,
                    &mut events,
                );
            } else if was_absent
                && rec.state == ModelState::Live
                && answered_since(rec, now_ms.saturating_sub(ANSWERED_RECENTLY_MS))
            {
                // Unlisted but answering as itself (see `health::observe`): it stays routable.
            } else if !matches!(
                rec.state,
                ModelState::NotInPlan | ModelState::Ghost | ModelState::Pruned
            ) {
                events.push(RegistryEvent::Removed { id: id.clone() });
                Self::set_state(
                    rec,
                    ModelState::NotInPlan,
                    "It isn't in your own fresh model list.".into(),
                    now_ms,
                    &mut events,
                );
            }
        }
        let gone: Vec<String> = self
            .models
            .iter()
            .filter(|(id, r)| {
                r.state.tombstone()
                    && r.tombstone_ms > 0
                    && now_ms.saturating_sub(r.tombstone_ms) >= PRUNE_AFTER_MS
                    && !named.iter().any(|n| n.trim() == id.as_str())
            })
            .map(|(id, _)| id.clone())
            .collect();
        for id in gone {
            self.models.remove(&id);
            events.push(RegistryEvent::Deleted { id });
        }
        for name in named {
            let id = name.trim();
            if id.is_empty()
                || self.models.contains_key(id)
                || merged.contains_key(id)
                || aliased.contains(&id)
            {
                continue;
            }
            let mut rec = ModelRecord {
                meta: ModelMeta::bare(id),
                first_seen_ms: now_ms,
                absent_since_ms: now_ms,
                ..ModelRecord::default()
            };
            Self::set_state(
                &mut rec,
                ModelState::Ghost,
                "A pin, default, skill or automation names it, but no source lists it.".into(),
                now_ms,
                &mut events,
            );
            self.models.insert(id.to_string(), rec);
        }
        self.schema = REGISTRY_SCHEMA;
        self.refreshed_ms = now_ms;
        self.refreshes = self.refreshes.saturating_add(1);
        self.hash = self.compute_hash();
        events
    }

    /// Fold real calls into health. Returns state events and whether a
    /// not-found or redirect signal asks for an early refresh.
    pub fn fold(&mut self, observations: &[Observation]) -> (Vec<RegistryEvent>, bool) {
        let mut events = Vec::new();
        let mut signal = false;
        for obs in observations {
            let id = self.canonical(obs.model.trim()).to_string();
            let id = id.as_str();
            if id.is_empty() {
                continue;
            }
            let listed = self.listed(id);
            let rec = self
                .models
                .entry(id.to_string())
                .or_insert_with(|| ModelRecord {
                    meta: ModelMeta::bare(id),
                    state: ModelState::NotInPlan,
                    reason: "It isn't in your own fresh model list.".into(),
                    first_seen_ms: obs.ts_ms,
                    absent_since_ms: obs.ts_ms,
                    ..ModelRecord::default()
                });
            let from = rec.state;
            health::observe(rec, listed, obs);
            health::breaker_observe(rec, listed, obs);
            if let Some(p) = rec.probation.as_mut() {
                p.observe(obs);
            }
            if rec.state != from {
                events.push(RegistryEvent::State {
                    id: id.to_string(),
                    from,
                    to: rec.state,
                    reason: rec.reason.clone(),
                });
            }
            signal |=
                matches!(obs.status, CallStatus::NotFound) || rec.state == ModelState::Redirected;
        }
        (events, signal)
    }

    /// Judge every tryout (graduate, fail, retry) with the learner's word
    /// on each model's answers, and keep each card's probation flag on
    /// its stage. The tryout events.
    pub fn judge_probation(
        &mut self,
        quality: &dyn Fn(&str) -> probation::Quality,
        now_ms: u64,
    ) -> Vec<RegistryEvent> {
        let mut events = Vec::new();
        for (id, rec) in self.models.iter_mut() {
            let Some(p) = rec.probation.as_mut() else {
                continue;
            };
            if let Some(v) = p.judge(quality(id), now_ms) {
                events.push(RegistryEvent::Probation {
                    id: id.clone(),
                    stage: v.to,
                    reason: v.reason,
                });
            }
            rec.meta.card.probation = p.active();
        }
        events
    }

    /// Routable models whose announced expiry is inside the scorer's stop
    /// window (`stop_day`: the day key that many days on): one event per
    /// model and date, so the log says why routing to it stopped.
    pub fn note_expiring(&mut self, stop_day: &str) -> Vec<RegistryEvent> {
        let mut events = Vec::new();
        for (id, rec) in self.models.iter_mut() {
            let Some(expires) = rec.meta.card.expires.clone() else {
                continue;
            };
            if rec.state.routable() && expires.as_str() <= stop_day && rec.expiry_noted != expires {
                rec.expiry_noted.clone_from(&expires);
                events.push(RegistryEvent::Expiring {
                    id: id.clone(),
                    expires,
                });
            }
        }
        events
    }

    /// Quarantined models whose open backoff ran out: each gets one check.
    pub fn half_open_due(&self, now_ms: u64) -> Vec<String> {
        self.models
            .iter()
            .filter(|(_, r)| health::half_open_due(r, now_ms))
            .map(|(id, _)| id.clone())
            .collect()
    }

    /// Hand out a model's one half-open check.
    pub fn mark_half_open(&mut self, id: &str) {
        if let Some(r) = self.models.get_mut(id.trim()) {
            r.breaker.half_open = true;
        }
    }

    /// The half-open check's answer. The state event, when it changed.
    pub fn half_open_result(&mut self, id: &str, ok: bool, now_ms: u64) -> Option<RegistryEvent> {
        let rec = self.models.get_mut(id.trim())?;
        let from = rec.state;
        health::half_open_result(rec, ok, now_ms);
        (rec.state != from).then(|| RegistryEvent::State {
            id: rec.meta.id.clone(),
            from,
            to: rec.state,
            reason: rec.reason.clone(),
        })
    }

    pub fn compute_hash(&self) -> String {
        let mut h = Sha256::new();
        for (id, rec) in &self.models {
            h.update(id.as_bytes());
            h.update(rec.content_hash.as_bytes());
        }
        hex::encode(h.finalize())
    }
}

/// Why a refresh runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefreshReason {
    Startup,
    Interval,
    GbVersion,
    Auth,
    Signal,
    Demand,
}

impl RefreshReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Startup => "startup",
            Self::Interval => "interval",
            Self::GbVersion => "gb_version",
            Self::Auth => "auth",
            Self::Signal => "signal",
            Self::Demand => "demand",
        }
    }
}

/// The refresh schedule, driven by the heartbeat with an injected clock.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RefreshClock {
    pub started_ms: u64,
    pub last_ms: Option<u64>,
    pub gb_version: Option<String>,
    /// A stamp of the sign-in kind and tier; a change refreshes.
    pub auth_stamp: String,
    pub signal: bool,
    pub demand: bool,
}

impl RefreshClock {
    pub fn new(started_ms: u64) -> Self {
        Self {
            started_ms,
            ..Self::default()
        }
    }

    /// The reason a refresh is due now, if any. The first one waits for idle.
    pub fn due(
        &self,
        now_ms: u64,
        idle: bool,
        gb_version: Option<&str>,
        auth_stamp: &str,
    ) -> Option<RefreshReason> {
        if self.demand {
            return Some(RefreshReason::Demand);
        }
        let Some(last) = self.last_ms else {
            return (idle && now_ms.saturating_sub(self.started_ms) >= FIRST_REFRESH_MS)
                .then_some(RefreshReason::Startup);
        };
        if gb_version.is_some() && gb_version != self.gb_version.as_deref() {
            return Some(RefreshReason::GbVersion);
        }
        if auth_stamp != self.auth_stamp {
            return Some(RefreshReason::Auth);
        }
        if self.signal {
            return Some(RefreshReason::Signal);
        }
        (now_ms.saturating_sub(last) >= REFRESH_EVERY_MS).then_some(RefreshReason::Interval)
    }

    /// Record a refresh that just started.
    pub fn ran(&mut self, now_ms: u64, gb_version: Option<&str>, auth_stamp: &str) {
        self.last_ms = Some(now_ms);
        if gb_version.is_some() {
            self.gb_version = gb_version.map(str::to_string);
        }
        self.auth_stamp = auth_stamp.to_string();
        self.signal = false;
        self.demand = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meta(id: &str) -> ModelMeta {
        ModelMeta {
            id: id.into(),
            context_length: Some(256_000),
            prices: Prices {
                prompt: Some(20_000),
                cached: Some(5_000),
                completion: Some(100_000),
                ..Prices::default()
            },
            efforts: Some(vec!["low".into(), "high".into()]),
            ..ModelMeta::default()
        }
    }

    fn api(models: Vec<ModelMeta>) -> Listing {
        Listing::new(SourceKind::XaiApi, models)
    }

    #[test]
    fn first_refresh_adds_every_model_and_a_placeholder_is_a_ghost() {
        let mut reg = Registry::default();
        let ev = reg.apply_refresh(
            &[api(vec![
                meta("grok-4.7"),
                ModelMeta::bare("grok-3-mini-fast"),
            ])],
            &[],
            1_000,
        );
        assert!(ev.contains(&RegistryEvent::Added {
            id: "grok-4.7".into()
        }));
        assert!(ev.contains(&RegistryEvent::Added {
            id: "grok-3-mini-fast".into()
        }));
        assert_eq!(reg.get("grok-4.7").unwrap().state, ModelState::Live);
        let ghost = reg.get("grok-3-mini-fast").unwrap();
        assert_eq!(
            (ghost.state, ghost.reason.as_str()),
            (
                ModelState::Ghost,
                "Its listing has no prices, context or efforts."
            )
        );
        assert_eq!(reg.refreshes, 1);
        assert_eq!(reg.hash.len(), 64);
    }

    #[test]
    fn an_unchanged_refresh_emits_nothing_and_a_change_names_what_changed() {
        let mut reg = Registry::default();
        reg.apply_refresh(&[api(vec![meta("grok-4.7")])], &[], 1);
        let hash = reg.hash.clone();
        assert_eq!(
            reg.apply_refresh(&[api(vec![meta("grok-4.7")])], &[], 2),
            vec![]
        );
        assert_eq!(reg.hash, hash);
        let mut m = meta("grok-4.7");
        m.efforts = Some(vec!["low".into(), "medium".into(), "high".into()]);
        m.prices.prompt = Some(30_000);
        let ev = reg.apply_refresh(&[api(vec![m])], &[], 3);
        assert_eq!(
            ev,
            vec![RegistryEvent::Changed {
                id: "grok-4.7".into(),
                what: vec!["price".into(), "efforts".into()]
            }]
        );
        assert_ne!(reg.hash, hash);
    }

    #[test]
    fn absent_from_your_own_list_is_not_in_plan_with_no_strikes() {
        let mut reg = Registry::default();
        reg.apply_refresh(&[api(vec![meta("grok-4.7"), meta("grok-4.5")])], &[], 1);
        let ev = reg.apply_refresh(&[api(vec![meta("grok-4.7")])], &[], 2);
        let r = reg.get("grok-4.5").unwrap();
        assert_eq!((r.state, r.not_found.len()), (ModelState::NotInPlan, 0));
        assert_eq!(ev.iter().filter(|e| e.kind() == "removed").count(), 1);
        assert!(!reg.listed("grok-4.5"));
        // A failed refresh (no listing) changes nothing.
        let before = reg.clone();
        assert_eq!(reg.apply_refresh(&[], &[], 3), vec![]);
        assert_eq!(reg, before);
    }

    #[test]
    fn a_tier_downgrade_removes_a_model_and_emits_exactly_one_notice() {
        let mut reg = Registry::default();
        let mut heavy = api(vec![meta("grok-4.7"), meta("grok-4.7-heavy")]);
        heavy.tier = Some("SuperGrok Heavy".into());
        reg.apply_refresh(&[heavy], &[], 1);
        let mut plain = api(vec![meta("grok-4.7")]);
        plain.tier = Some("SuperGrok".into());
        let ev = reg.apply_refresh(&[plain], &[], 2);
        let notices: Vec<_> = ev.iter().filter(|e| e.kind() == "notice").collect();
        assert_eq!(
            notices,
            vec![&RegistryEvent::Notice {
                text: "Your Grok plan changed from SuperGrok Heavy to SuperGrok.".into()
            }]
        );
        assert!(ev.contains(&RegistryEvent::Removed {
            id: "grok-4.7-heavy".into()
        }));
        assert_eq!(
            reg.get("grok-4.7-heavy").unwrap().state,
            ModelState::NotInPlan
        );
        assert_eq!(reg.entitlement.tier.as_deref(), Some("SuperGrok"));
    }

    #[test]
    fn a_named_id_no_source_lists_is_a_ghost_and_critical_retired_is_a_ghost() {
        let mut reg = Registry::default();
        let mut retired = meta("grok-4.1");
        retired.notice = Some(Notice {
            severity: "critical".into(),
            text: "Retired on 2026-09-01".into(),
        });
        reg.apply_refresh(
            &[api(vec![meta("grok-4.7"), retired])],
            &["grok-2-pinned".into()],
            1,
        );
        assert_eq!(reg.get("grok-2-pinned").unwrap().state, ModelState::Ghost);
        assert_eq!(reg.get("grok-4.1").unwrap().state, ModelState::Ghost);
        assert_eq!(
            reg.get("grok-4.1").unwrap().reason,
            "Grok Build marked it retired."
        );
    }

    #[test]
    fn gb_and_api_rows_merge_and_a_gb_only_row_is_not_a_placeholder_ghost() {
        let mut reg = Registry::default();
        let gb = gb_listing(&["grok-4.7".into(), "grok-build-1".into()], Some("1.0.49"));
        reg.apply_refresh(&[gb, api(vec![meta("grok-4.7")])], &[], 1);
        let both = reg.get("grok-4.7").unwrap();
        assert_eq!(
            both.sources,
            vec![SourceKind::XaiApi, SourceKind::GrokBuild]
        );
        assert_eq!(both.meta.context_length, Some(256_000));
        let gb_only = reg.get("grok-build-1").unwrap();
        assert_eq!(
            (gb_only.state, gb_only.sources.clone()),
            (ModelState::Live, vec![SourceKind::GrokBuild])
        );
        assert_eq!(reg.gb_version.as_deref(), Some("1.0.49"));
    }

    #[test]
    fn fold_moves_health_and_flags_a_signal() {
        let mut reg = Registry::default();
        reg.apply_refresh(&[api(vec![meta("grok-4.7")])], &[], 1);
        let o = |status, ts_ms| Observation {
            model: "grok-4.7".into(),
            ts_ms,
            status,
            latency_ms: 500,
            served_model: None,
            endpoint_ok: true,
            reasoning_tokens: 0,
            cost_ticks: 0,
            http: 0,
        };
        let (ev, signal) = reg.fold(&[o(CallStatus::ContentSafety, 10)]);
        assert_eq!((ev, signal), (vec![], false));
        let (ev, signal) = reg.fold(&[o(CallStatus::NotFound, 10), o(CallStatus::NotFound, 20)]);
        assert!(signal);
        // The first strike quarantines it, the second makes it a ghost.
        assert_eq!(ev.len(), 2);
        assert_eq!(reg.get("grok-4.7").unwrap().state, ModelState::Ghost);
        // An unlisted slug that answers as itself stays routable (2.10.97: it was an
        // unroutable Redirected with no successor, which dropped a working pin).
        let mut unlisted = o(CallStatus::Ok, 30);
        unlisted.model = "grok-secret".into();
        let (_, signal) = reg.fold(&[unlisted]);
        assert!(!signal, "no not-found and no redirect");
        let rec = reg.get("grok-secret").unwrap();
        assert_eq!(
            (rec.state, rec.reason.as_str()),
            (
                ModelState::Live,
                "It isn't listed, but it answers as itself."
            )
        );
        assert!(rec.state.routable());
    }

    #[test]
    fn refresh_clock_waits_for_idle_then_every_six_hours_and_on_changes() {
        let mut c = RefreshClock::new(0);
        assert_eq!(c.due(29_999, true, None, "plan"), None);
        assert_eq!(c.due(30_000, false, None, "plan"), None);
        assert_eq!(
            c.due(30_000, true, None, "plan"),
            Some(RefreshReason::Startup)
        );
        c.ran(30_000, Some("1.0.49"), "plan");
        assert_eq!(c.due(30_001, false, Some("1.0.49"), "plan"), None);
        assert_eq!(
            c.due(30_000 + REFRESH_EVERY_MS, false, Some("1.0.49"), "plan"),
            Some(RefreshReason::Interval)
        );
        assert_eq!(
            c.due(40_000, false, Some("1.0.50"), "plan"),
            Some(RefreshReason::GbVersion)
        );
        assert_eq!(
            c.due(40_000, false, None, "api_key"),
            Some(RefreshReason::Auth)
        );
        c.signal = true;
        assert_eq!(
            c.due(40_000, false, None, "plan"),
            Some(RefreshReason::Signal)
        );
        c.demand = true;
        assert_eq!(
            c.due(40_000, false, None, "plan"),
            Some(RefreshReason::Demand)
        );
    }

    #[test]
    fn a_retired_row_is_a_tombstone_for_30_days_then_deleted() {
        let mut reg = Registry::default();
        let mut old = meta("grok-4.5");
        old.deprecated = Some(true);
        reg.apply_refresh(&[api(vec![meta("grok-4.7"), old.clone()])], &[], 1_000);
        let r = reg.get("grok-4.5").unwrap();
        assert_eq!((r.state, r.tombstone_ms), (ModelState::Retired, 1_000));
        assert!(!r.state.routable());
        assert!(reg.get("grok-4.7").unwrap().state.routable());
        let ev = reg.apply_refresh(
            &[api(vec![meta("grok-4.7"), old.clone()])],
            &[],
            1_000 + PRUNE_AFTER_MS - 1,
        );
        assert!(ev.is_empty() && reg.get("grok-4.5").is_some());
        let ev = reg.apply_refresh(
            &[api(vec![meta("grok-4.7"), old])],
            &[],
            1_000 + PRUNE_AFTER_MS,
        );
        assert_eq!(
            ev,
            vec![RegistryEvent::Deleted {
                id: "grok-4.5".into()
            }]
        );
        assert!(reg.get("grok-4.5").is_none());
    }

    #[test]
    fn an_endpoint_change_is_named_and_the_breaker_survives_a_refresh() {
        let mut reg = Registry::default();
        reg.apply_refresh(&[api(vec![meta("grok-4.7")])], &[], 1);
        let mut m = meta("grok-4.7");
        m.api_shape = Some("chat_completions".into());
        let ev = reg.apply_refresh(&[api(vec![m.clone()])], &[], 2);
        assert_eq!(
            ev,
            vec![RegistryEvent::Changed {
                id: "grok-4.7".into(),
                what: vec!["endpoint".into()]
            }]
        );
        let bad = |ts_ms| Observation {
            model: "grok-4.7".into(),
            ts_ms,
            status: CallStatus::Error,
            latency_ms: 30_000,
            served_model: None,
            endpoint_ok: true,
            reasoning_tokens: 0,
            cost_ticks: 0,
            http: 0,
        };
        let (ev, _) = reg.fold(&[bad(10), bad(11), bad(12)]);
        assert_eq!(ev.len(), 1);
        assert_eq!(reg.get("grok-4.7").unwrap().state, ModelState::Quarantined);
        reg.apply_refresh(&[api(vec![m])], &[], 13);
        assert_eq!(
            reg.get("grok-4.7").unwrap().state,
            ModelState::Quarantined,
            "a fresh listing doesn't lift the quarantine"
        );
        let until = reg.get("grok-4.7").unwrap().breaker.open_until_ms;
        assert_eq!(reg.half_open_due(until), vec!["grok-4.7".to_string()]);
        reg.mark_half_open("grok-4.7");
        assert!(reg.half_open_due(until).is_empty());
        let ev = reg.half_open_result("grok-4.7", true, until + 1).unwrap();
        assert_eq!(ev.kind(), "state");
        assert_eq!(reg.get("grok-4.7").unwrap().state, ModelState::Live);
    }

    #[test]
    fn a_source_that_does_not_answer_keeps_its_models() {
        let mut reg = Registry::default();
        let gb = gb_listing(&["grok-build-1".into()], Some("1.0.49"));
        reg.apply_refresh(&[gb, api(vec![meta("grok-4.7")])], &[], 1);
        assert_eq!(reg.get("grok-build-1").unwrap().state, ModelState::Live);
        // The Grok Build listing never arrives (its fetch failed) while the
        // API listing answers: the GB-only model keeps its place and the
        // refresh reports no change for it.
        let ev = reg.apply_refresh(&[api(vec![meta("grok-4.7")])], &[], 2);
        assert_eq!(ev, vec![]);
        let kept = reg.get("grok-build-1").unwrap();
        assert_eq!(kept.state, ModelState::Live);
        assert!(reg.listed("grok-build-1"));
        // Once GB answers again without it, it goes stale as before.
        let gb = gb_listing(&["grok-4.7".into()], Some("1.0.49"));
        let ev = reg.apply_refresh(&[gb, api(vec![meta("grok-4.7")])], &[], 3);
        assert!(ev.contains(&RegistryEvent::Removed {
            id: "grok-build-1".into()
        }));
        assert_eq!(
            reg.get("grok-build-1").unwrap().state,
            ModelState::NotInPlan
        );
    }

    #[test]
    fn a_model_kept_by_a_silent_source_survives_the_answering_source_dropping_it() {
        let mut reg = Registry::default();
        let gb = gb_listing(&["grok-dual".into()], Some("1.0.49"));
        reg.apply_refresh(
            &[gb, api(vec![meta("grok-dual"), meta("grok-4.7")])],
            &[],
            1,
        );
        assert_eq!(
            reg.get("grok-dual").unwrap().sources,
            vec![SourceKind::XaiApi, SourceKind::GrokBuild]
        );
        // The API answers and drops it, but GB never answered: it stands.
        reg.apply_refresh(&[api(vec![meta("grok-4.7")])], &[], 2);
        assert_eq!(reg.get("grok-dual").unwrap().state, ModelState::Live);
        assert!(reg.listed("grok-dual"));
    }

    fn or(models: &[&str]) -> Listing {
        Listing::new(
            SourceKind::OpenAiCompatible,
            models.iter().map(|id| ModelMeta::bare(id)).collect(),
        )
    }

    fn tryout(reg: &Registry, id: &str) -> Option<probation::Stage> {
        reg.get(id)
            .and_then(|r| r.probation.as_ref())
            .map(|p| p.stage)
    }

    #[test]
    fn a_new_model_from_a_known_provider_goes_on_a_tryout() {
        let mut reg = Registry::default();
        // The provider's first listing is its baseline: no tryouts.
        let ev = reg.apply_refresh(&[or(&["openrouter/vendor/old"])], &[], 1);
        assert_eq!(tryout(&reg, "openrouter/vendor/old"), None);
        assert!(!ev.iter().any(|e| e.kind() == "probation"));
        // A model it starts listing later is new.
        let ev = reg.apply_refresh(
            &[or(&["openrouter/vendor/old", "openrouter/vendor/new"])],
            &[],
            2,
        );
        assert_eq!(
            tryout(&reg, "openrouter/vendor/new"),
            Some(probation::Stage::Trial)
        );
        assert!(
            reg.get("openrouter/vendor/new")
                .unwrap()
                .meta
                .card
                .probation
        );
        assert_eq!(tryout(&reg, "openrouter/vendor/old"), None);
        let started: Vec<_> = ev.iter().filter(|e| e.kind() == "probation").collect();
        assert_eq!(started.len(), 1, "{ev:?}");
        // The next refresh keeps the tryout as it stands, with no new event.
        let ev = reg.apply_refresh(
            &[or(&["openrouter/vendor/old", "openrouter/vendor/new"])],
            &[],
            3,
        );
        assert!(!ev.iter().any(|e| e.kind() == "probation"));
        assert_eq!(
            reg.get("openrouter/vendor/new")
                .and_then(|r| r.probation.as_ref())
                .map(|p| p.since_ms),
            Some(2)
        );
    }

    #[test]
    fn a_model_the_card_table_marks_starts_on_a_tryout() {
        let mut reg = Registry::default();
        let id = "openrouter/nvidia/nemotron-3-super-120b-a12b:free";
        reg.apply_refresh(&[or(&[id])], &[], 1);
        assert_eq!(tryout(&reg, id), Some(probation::Stage::Trial));
        // xAI's own listing is not a provider tryout.
        reg.apply_refresh(&[api(vec![meta("grok-4.7")])], &[], 2);
        assert_eq!(tryout(&reg, "grok-4.7"), None);
    }

    #[test]
    fn tryout_calls_count_and_the_judge_graduates_or_fails_on_them() {
        let mut reg = Registry::default();
        reg.apply_refresh(&[or(&["openrouter/vendor/old"])], &[], 1);
        reg.apply_refresh(
            &[or(&["openrouter/vendor/old", "openrouter/vendor/new"])],
            &[],
            2,
        );
        let o = |status| Observation {
            model: "openrouter/vendor/new".into(),
            ts_ms: 10,
            status,
            latency_ms: 500,
            served_model: None,
            endpoint_ok: true,
            reasoning_tokens: 0,
            cost_ticks: 0,
            http: 0,
        };
        let obs: Vec<_> = (0..5).map(|_| o(CallStatus::Error)).collect();
        reg.fold(&obs);
        assert_eq!(
            reg.get("openrouter/vendor/new")
                .and_then(|r| r.probation.as_ref())
                .map(|p| p.calls),
            Some(5)
        );
        let unknown = |_: &str| probation::Quality::Unknown;
        let ev = reg.judge_probation(&unknown, 3);
        assert_eq!(ev.len(), 1, "{ev:?}");
        let rec = reg.get("openrouter/vendor/new").unwrap();
        assert!(rec.probation_failed());
        assert!(rec.meta.card.probation);
        // Judged again with nothing new: no second event.
        assert!(reg.judge_probation(&unknown, 4).is_empty());
        // A refresh keeps the failed stage (the card table can't undo it).
        reg.apply_refresh(
            &[or(&["openrouter/vendor/old", "openrouter/vendor/new"])],
            &[],
            5,
        );
        assert!(reg.get("openrouter/vendor/new").unwrap().probation_failed());
    }

    #[test]
    fn a_close_expiry_is_logged_once_per_date() {
        let mut reg = Registry::default();
        reg.apply_refresh(&[api(vec![meta("grok-4.7")])], &[], 1);
        reg.models.get_mut("grok-4.7").unwrap().meta.card.expires = Some("2026-11-20".into());
        assert!(reg.note_expiring("2026-11-19").is_empty());
        let ev = reg.note_expiring("2026-11-22");
        assert_eq!(
            ev,
            vec![RegistryEvent::Expiring {
                id: "grok-4.7".into(),
                expires: "2026-11-20".into()
            }]
        );
        assert!(reg.note_expiring("2026-11-23").is_empty());
        // A new date is news again.
        reg.models.get_mut("grok-4.7").unwrap().meta.card.expires = Some("2026-11-21".into());
        assert_eq!(reg.note_expiring("2026-11-23").len(), 1);
    }
}
