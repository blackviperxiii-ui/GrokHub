//! Keeps the router's model registry current on its own. The router picks
//! only from this registry, so with nothing refreshing it no on-device tier
//! or added provider is ever routed to: an empty registry keeps every turn on
//! the chat's model, and Offline only has nothing to answer with.
//!
//! - A refresh lists what each source offers (xAI for your sign-in, Local AI,
//!   the providers you added) on [`RefreshClock`]'s schedule, and again when
//!   the sign-in, Local AI or the added providers change. A plan sign-in then
//!   probes the models it hasn't met and rebuilds the routing table.
//! - Passive health from real calls is folded in every [`FOLD_EVERY_MS`].
//! - Every [`LEARN_EVERY_MS`] the accuracy guard reverts a class whose
//!   start trails, and self-tuning moves its shadows, canaries and
//!   promotions on.
//! - What a refresh or fold has to tell (a plan lost models, new models
//!   joined) comes back once per incident as [`Ran::notices`].
//!
//! The app calls [`Upkeep::tick`] from one background thread and posts the
//! notices on Home.

use std::path::Path;

use grokhub_core::model_registry::profile::read_profiles;
use grokhub_core::model_registry::store::load_registry;
use grokhub_core::model_registry::{CatalogSource, Credential, RefreshClock, RegistryEvent};

use super::heal::{heal_messages, joined_messages, HealMsg, HealNotes};
use super::local::{self, LocalSource};
use super::providers;
use super::refresh::{fold_health, probe_next, run_refresh, RefreshDone, RefreshJob};
use super::signals::{LedgerOutcomeSource, SpanVerifySource};
use super::sources::{probe_model, rebuild_table, XaiApiSource};

/// Passive health is folded into the registry this often.
pub const FOLD_EVERY_MS: u64 = 60_000;
/// The accuracy guard and self-tuning run this often.
pub const LEARN_EVERY_MS: u64 = 60 * 60 * 1000;

/// The xAI sign-in a refresh lists with: the bearer and its kind.
pub type SignIn = Option<(String, Credential)>;

/// What one [`Upkeep::tick`] ran.
#[derive(Debug, Default)]
pub struct Ran {
    pub folded: bool,
    pub refreshed: Option<RefreshDone>,
    /// The guard and self-tuning ran.
    pub learned: bool,
    /// Home updates not told before, each incident once.
    pub notices: Vec<HealMsg>,
}

/// The schedule between ticks.
#[derive(Debug, Clone)]
pub struct Upkeep {
    clock: RefreshClock,
    last_fold_ms: Option<u64>,
    last_learn_ms: Option<u64>,
}

impl Upkeep {
    pub fn new(now_ms: u64) -> Self {
        Self {
            clock: RefreshClock::new(now_ms),
            last_fold_ms: None,
            last_learn_ms: None,
        }
    }

    /// Fold health when a minute has passed, and refresh when one is due.
    pub fn tick(&mut self, config_dir: &Path, now_ms: u64, sign_in: &SignIn) -> Ran {
        let mut ran = Ran::default();
        if self
            .last_fold_ms
            .is_none_or(|t| now_ms.saturating_sub(t) >= FOLD_EVERY_MS)
        {
            self.last_fold_ms = Some(now_ms);
            let (events, signal) = fold_health(config_dir);
            self.clock.signal |= signal;
            ran.folded = true;
            ran.notices
                .extend(heal_after(config_dir, &events, &[], now_ms));
        }
        let stamp = stamp(config_dir, sign_in);
        if self.clock.due(now_ms, true, None, &stamp).is_some() {
            self.clock.ran(now_ms, None, &stamp);
            let (done, joined) = refresh(config_dir, now_ms, sign_in);
            self.clock.signal |= done.signal;
            ran.notices
                .extend(heal_after(config_dir, &done.events, &joined, now_ms));
            ran.refreshed = Some(done);
        }
        if self
            .last_learn_ms
            .is_none_or(|t| now_ms.saturating_sub(t) >= LEARN_EVERY_MS)
        {
            self.last_learn_ms = Some(now_ms);
            learn(config_dir, now_ms);
            ran.learned = true;
        }
        ran
    }
}

/// The accuracy guard, then self-tuning. Both write their own ledgers and
/// tell nothing: which effort a class starts at shows nowhere.
fn learn(config_dir: &Path, now_ms: u64) {
    super::guard::run_guard(config_dir, now_ms);
    let verify = SpanVerifySource {
        config_dir: config_dir.to_path_buf(),
        session: String::new(),
    };
    let _ = super::learn::tick(
        config_dir,
        &verify,
        &LedgerOutcomeSource::new(config_dir),
        now_ms,
    );
}

/// The Home updates a batch of registry events and newly ranked models ask
/// for, less the incidents already told.
fn heal_after(
    config_dir: &Path,
    events: &[RegistryEvent],
    joined: &[String],
    now_ms: u64,
) -> Vec<HealMsg> {
    let mut heard = if events.is_empty() {
        Default::default()
    } else {
        heal_messages(
            events,
            &load_registry(config_dir),
            &read_profiles(config_dir),
            now_ms,
        )
    };
    heard.msgs.extend(joined_messages(joined));
    if heard.msgs.is_empty() && heard.cleared.is_empty() {
        return Vec::new();
    }
    let mut notes = HealNotes::load(config_dir);
    let msgs = notes.admit(heard, now_ms);
    notes.save(config_dir);
    msgs
}

/// What a change of refreshes: the sign-in's kind (never the token), Local
/// AI, and the providers added.
fn stamp(config_dir: &Path, sign_in: &SignIn) -> String {
    let kind = match sign_in {
        Some((_, Credential::Plan)) => "plan",
        Some((_, Credential::ApiKey)) => "key",
        _ => "none",
    };
    format!(
        "{kind};local={};providers={}",
        local::available(),
        providers::load_providers(config_dir).len()
    )
}

/// Ids a default names, kept as ghosts when no source lists them.
fn named_models() -> Vec<String> {
    let mut named: Vec<String> = [
        grokhub_core::CABIN_FAST_MODEL,
        grokhub_core::CABIN_FAST_FALLBACK,
        grokhub_core::DEFAULT_MODEL,
        crate::DEFAULT_MODEL,
    ]
    .iter()
    .filter(|m| !m.is_empty())
    .map(|m| m.to_string())
    .collect();
    named.sort();
    named.dedup();
    named
}

/// A refresh, and the models the rebuilt table ranked for the first time.
fn refresh(config_dir: &Path, now_ms: u64, sign_in: &SignIn) -> (RefreshDone, Vec<String>) {
    // No sign-in read: the credential the last refresh proved stands until a
    // sign-in replaces it.
    let credential = sign_in
        .as_ref()
        .map_or_else(|| load_registry(config_dir).entitlement.credential, |s| s.1);
    let bearer = sign_in.as_ref().map(|s| s.0.clone());
    let api = bearer.clone().map(|bearer| XaiApiSource {
        config_dir: config_dir.to_path_buf(),
        bearer,
    });
    let added = providers::sources(config_dir);
    let mut sources: Vec<&dyn CatalogSource> = Vec::new();
    if let Some(a) = api.as_ref() {
        sources.push(a);
    }
    // Lists nothing while Local AI is off.
    sources.push(&LocalSource);
    for p in &added {
        sources.push(p);
    }
    let done = run_refresh(&RefreshJob {
        config_dir: config_dir.to_path_buf(),
        sources,
        credential,
        named: named_models(),
        default_model: crate::DEFAULT_MODEL.to_string(),
        now_ms,
        xai_bearer: bearer.clone(),
    });
    // Probes and evals run only on a plan sign-in (included routes); a key
    // rebuilds the table from metadata.
    let plan = bearer.as_deref().filter(|_| credential == Credential::Plan);
    if let Some(b) = plan {
        while probe_next(
            config_dir,
            credential,
            grokhub_core::now_ms(),
            &mut |m, c| probe_model(config_dir, b, m, c),
        )
        .is_some()
        {}
    }
    let joined = rebuild_table(config_dir, plan, false, now_ms).joined;
    (done, joined)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use serde_json::{json, Value};

    use super::*;
    use crate::route::connectivity::ForceOffline;
    use crate::route::local::{ForceOn, LocalGate, LocalRuntime};
    use crate::route::{RouteInput, Router};
    use grokhub_core::model_registry::FIRST_REFRESH_MS;

    struct FakeRt;

    fn temp(name: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("grokhub-upkeep-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    impl LocalRuntime for FakeRt {
        fn classify(&self, _text: &str, labels: &[&str]) -> Result<Value, String> {
            Ok(json!({ "label": labels[0], "score": 0.9 }))
        }
        fn summarize(&self, _text: &str, _max_words: u32) -> Result<Value, String> {
            Ok(json!({ "summary": "two lines" }))
        }
        fn difficulty(&self, _text: &str) -> Result<Value, String> {
            Ok(json!({ "d": 0.4 }))
        }
    }

    /// With Local AI on, the first refresh lists its tiers, and Offline only
    /// then answers a chat on the device instead of waiting with no route.
    #[test]
    fn the_first_refresh_lists_local_ai_so_offline_only_routes_on_device() {
        let dir = temp("offline");
        let _on = ForceOn::with(Some(Arc::new(FakeRt)));
        let _off = ForceOffline::on();
        let gate = LocalGate::now(false, false, false);
        let route = |dir: &Path| {
            let (reg, profiles) = crate::route::live::snapshot(dir);
            let table = crate::route::table::RoutingTable::default();
            let input = RouteInput {
                provider: "xai",
                class: "chat:default",
                current_model: "grok-4.7",
                d: 0.5,
                local: gate.clone(),
                ..RouteInput::default()
            };
            let r = Router::choose(&input, &reg, &profiles, &table, 10);
            (r.model, r.rule_ids)
        };
        // Never refreshed: no local tier to pick, so the turn has no route.
        let (_, rules) = route(&dir);
        assert_eq!(rules, vec!["offline:no_local", "class:chat:default"]);

        let mut upkeep = Upkeep::new(1_000);
        let early = upkeep.tick(&dir, 1_000, &None);
        assert!(early.folded);
        assert!(early.learned, "the guard and tuning run at start");
        assert!(early.refreshed.is_none(), "the first refresh waits 30 s");
        let first = upkeep.tick(&dir, 1_000 + FIRST_REFRESH_MS, &None);
        assert!(!first.folded, "folded under a minute ago");
        assert!(!first.learned, "tuned under an hour ago");
        assert!(first.notices.is_empty(), "the tiers joining tell nothing");
        assert_eq!(first.refreshed.map(|d| d.errors), Some(Vec::new()));
        let reg = load_registry(&dir);
        assert_eq!(
            reg.models.keys().cloned().collect::<Vec<_>>(),
            [
                "grok-4.3",
                "grok-4.5",
                "grok-4.7",
                "local:small",
                "local:tiny"
            ],
            "the defaults stay as ghosts beside the tiers"
        );
        let (model, rules) = route(&dir);
        assert_eq!(model, "local:small");
        assert!(rules.contains(&"offline:local".to_string()), "{rules:?}");

        let later = upkeep.tick(&dir, 2_000 + FIRST_REFRESH_MS + FOLD_EVERY_MS, &None);
        assert!(later.folded);
        assert!(later.refreshed.is_none(), "nothing changed");
        let hour = upkeep.tick(&dir, 1_000 + LEARN_EVERY_MS, &None);
        assert!(hour.learned, "an hour on, they run again");
        assert!(hour.notices.is_empty());
    }

    /// Models the table ranks for the first time are told once.
    #[test]
    fn a_new_model_is_told_once() {
        let dir = temp("heal");
        let joined = ["grok-5".to_string()];
        let told = heal_after(&dir, &[], &joined, 1);
        assert_eq!(
            told.iter()
                .map(|m| (m.title.as_str(), m.text.as_str()))
                .collect::<Vec<_>>(),
            [(
                "New in your plan",
                "Your plan has a new AI model. Auto uses it where it does best."
            )]
        );
        assert!(heal_after(&dir, &[], &joined, 2).is_empty(), "told already");
    }

    /// A new sign-in kind or Local AI going off refreshes at once.
    #[test]
    fn a_sign_in_or_local_ai_change_refreshes_now() {
        let dir = temp("change");
        let mut upkeep = Upkeep::new(0);
        let on = ForceOn::with(Some(Arc::new(FakeRt)));
        assert!(upkeep
            .tick(&dir, FIRST_REFRESH_MS, &None)
            .refreshed
            .is_some());
        drop(on);
        assert!(!local::available());
        let t = FIRST_REFRESH_MS + 1;
        assert!(
            upkeep.tick(&dir, t, &None).refreshed.is_some(),
            "Local AI went off"
        );
        assert!(upkeep.tick(&dir, t + 1, &None).refreshed.is_none());
        assert_eq!(
            stamp(
                &dir,
                &Some(("sk-abcdefghijklmnopqrstuv".into(), Credential::ApiKey))
            ),
            "key;local=false;providers=0"
        );
    }
}
