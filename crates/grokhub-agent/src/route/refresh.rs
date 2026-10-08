//! One registry refresh, run off the UI thread by the cabin's heartbeat: read
//! the sources, fold passive health, diff, write span events, build model
//! profiles for added or changed models, and run at most one onboarding probe.
//!
//! R2a: a model is probed only when it is first seen, when its effort list or
//! endpoint changed, or when its breaker goes half-open; every probe shares
//! the onboarding queue's daily cap. Any other change (a price, a notice)
//! rebuilds the profile around the probe it already has.

use std::path::{Path, PathBuf};

use grokhub_core::model_registry::cost_class::{probe_class, CostClass};
use grokhub_core::model_registry::probe::{Onboarding, ProbeLogLine};
use grokhub_core::model_registry::profile::{read_profile, write_profile, ModelProfile, ProbeResult};
use grokhub_core::model_registry::store::{
    append_line, load_registry, onboarding_path, probe_log_path, read_json, save_registry, take_observations, write_json,
};
use grokhub_core::model_registry::{CatalogSource, Credential, ModelMeta, Registry, RegistryEvent, SourceKind};

use crate::harness::{append_span, AccessMode, Origin, OriginScope, Span};

/// Span session for registry events.
pub const REGISTRY_TRACE: &str = "model-registry";
pub const REGISTRY_TOOL: &str = "model.registry";

/// What the cabin hands a refresh.
pub struct RefreshJob<'a> {
    pub config_dir: PathBuf,
    pub sources: Vec<&'a dyn CatalogSource>,
    pub credential: Credential,
    /// Ids a pin, default, skill or automation names.
    pub named: Vec<String>,
    /// Today's default model: probed first.
    pub default_model: String,
    pub now_ms: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RefreshDone {
    pub events: Vec<RegistryEvent>,
    pub errors: Vec<String>,
    pub gb_version: Option<String>,
    /// A not-found or redirect was seen: refresh again soon.
    pub signal: bool,
    pub profiles_written: usize,
}

/// One span per registry event: `{"event":"changed","id":…,"what":[…]}`. No content.
pub fn event_span(ev: &RegistryEvent) -> Span {
    let args = serde_json::to_string(ev).unwrap_or_default();
    let mut s = Span::soft_allow(REGISTRY_TRACE, REGISTRY_TOOL, &args, ev.kind(), "", AccessMode::Supervised, "cabin")
        .from_origin(Origin::SelfManage)
        .on_path("model");
    s.access = String::new();
    s
}

/// The profile's probe field before any probe ran.
fn first_probe(rec_sources: &[SourceKind], cost: CostClass) -> ProbeResult {
    if rec_sources.iter().any(|s| s.is_new_provider()) {
        // R3b: probes stay on the plan pool, so a provider you added ranks on its listing alone.
        return ProbeResult::status("not_run: cost_class");
    }
    if !rec_sources.contains(&SourceKind::XaiApi) {
        return ProbeResult::status("not_run: gb_only");
    }
    if cost != CostClass::Included {
        return ProbeResult::status("not_run: cost_class");
    }
    ProbeResult::status("queued")
}

/// Fold `health.jsonl` into the saved registry. Returns the state events and the signal.
pub fn fold_health(config_dir: &Path) -> (Vec<RegistryEvent>, bool) {
    let obs = take_observations(config_dir);
    if obs.is_empty() {
        return (Vec::new(), false);
    }
    let mut reg = load_registry(config_dir);
    let (events, signal) = reg.fold(&obs);
    let _ = save_registry(config_dir, &reg);
    for ev in &events {
        let _ = append_span(config_dir, &event_span(ev));
    }
    (events, signal)
}

/// A change that needs a new probe: the effort list or the endpoint.
fn reprobe(what: &[String]) -> bool {
    what.iter().any(|w| w == "efforts" || w == "endpoint")
}

/// Build and write profiles for added or changed models; queue the ones to probe.
fn onboard(config_dir: &Path, reg: &mut Registry, events: &[RegistryEvent], cred: Credential, default_model: &str, now_ms: u64) -> usize {
    let ids: Vec<(&String, bool)> = events
        .iter()
        .filter_map(|e| match e {
            RegistryEvent::Added { id } => Some((id, true)),
            RegistryEvent::Changed { id, what } => Some((id, reprobe(what))),
            _ => None,
        })
        .collect();
    let mut queue: Onboarding = read_json(&onboarding_path(config_dir)).unwrap_or_default();
    let mut to_probe = Vec::new();
    let mut written = 0;
    for (id, fresh) in ids {
        let Some(rec) = reg.get(id) else { continue };
        if !rec.state.routable() {
            continue;
        }
        let kept = (!fresh).then(|| read_profile(config_dir, id)).flatten().map(|p| p.probe);
        let probe = kept.unwrap_or_else(|| first_probe(&rec.sources, probe_class(reg, cred, id)));
        if probe.status == "queued" {
            to_probe.push(id.clone());
        }
        let mut p = ModelProfile::build(&rec.meta, probe, now_ms);
        if write_profile(config_dir, &mut p).unwrap_or(false) {
            written += 1;
        }
    }
    // A failed probe is retried after a day (a `changed` event retries it sooner).
    for (id, p) in grokhub_core::model_registry::profile::read_profiles(config_dir) {
        let due = now_ms.saturating_sub(p.built_at) >= grokhub_core::model_registry::probe::PROBE_RETRY_MS;
        if p.probe.status == "failed" && due && reg.get(&id).is_some_and(|r| r.state.routable()) && !to_probe.contains(&id) {
            to_probe.push(id);
        }
    }
    // A quarantined model whose backoff ran out gets its one half-open check here.
    for id in reg.half_open_due(now_ms) {
        if cred == Credential::Plan && !to_probe.contains(&id) {
            reg.mark_half_open(&id);
            to_probe.push(id);
        }
    }
    queue.enqueue(&to_probe, default_model);
    let _ = write_json(&onboarding_path(config_dir), &queue);
    written
}

/// Take the next queued model (inside today's cap) and probe it with `probe`.
/// A model whose profile was retried in the last 24 h without a change waits.
pub fn probe_next(
    config_dir: &Path,
    cred: Credential,
    now_ms: u64,
    probe: &mut dyn FnMut(&ModelMeta, CostClass) -> ProbeResult,
) -> Option<ModelProfile> {
    let path = onboarding_path(config_dir);
    let mut queue: Onboarding = read_json(&path).unwrap_or_default();
    let id = queue.take_next(now_ms)?;
    let _ = write_json(&path, &queue);
    let mut reg = load_registry(config_dir);
    let rec = reg.get(&id)?;
    let cost = probe_class(&reg, cred, &id);
    let result = probe(&rec.meta, cost);
    let mut p = ModelProfile::build(&rec.meta, result, now_ms);
    let _ = write_profile(config_dir, &mut p);
    if rec.breaker.half_open {
        // The half-open check: only a probe that ran and passed closes the breaker.
        if let Some(ev) = reg.half_open_result(&id, p.probe.status == "ok", now_ms) {
            let _ = append_span(config_dir, &event_span(&ev));
        }
        let _ = save_registry(config_dir, &reg);
    }
    let line = ProbeLogLine {
        ts_ms: now_ms,
        model: id,
        status: p.probe.status.clone(),
        calls: p.probe.calls,
        tokens: p.probe.tokens,
        cost_ticks: p.probe.cost_ticks,
        usable: p.usable,
        reason: p.unusable_reason.clone(),
    };
    let _ = append_line(&probe_log_path(config_dir), &line);
    Some(read_profile(config_dir, &p.model).unwrap_or(p))
}

/// One refresh. No source answering changes nothing (the registry keeps its last list).
pub fn run_refresh(job: &RefreshJob<'_>) -> RefreshDone {
    let _origin = OriginScope::enter(Origin::SelfManage);
    let dir = &job.config_dir;
    let mut listings = Vec::new();
    let mut errors = Vec::new();
    for s in &job.sources {
        match s.fetch() {
            Ok(l) => listings.push(l),
            Err(e) => errors.push(format!("{}: {e}", s.kind().as_str())),
        }
    }
    let gb_version = listings.iter().find_map(|l| l.gb_version.clone());
    let mut reg = load_registry(dir);
    reg.entitlement.credential = job.credential;
    let (mut events, signal) = reg.fold(&take_observations(dir));
    events.extend(reg.apply_refresh(&listings, &job.named, job.now_ms));
    for ev in &events {
        let _ = append_span(dir, &event_span(ev));
    }
    let profiles_written = onboard(dir, &mut reg, &events, job.credential, &job.default_model, job.now_ms);
    let _ = save_registry(dir, &reg);
    RefreshDone { events, errors, gb_version, signal, profiles_written }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::{read_spans, test_dir};
    use grokhub_core::model_registry::discover::Listing;
    use grokhub_core::model_registry::probe::{run_probe, ProbeCall, ProbeEnv, ProbeKind, ProbeReply, ProbeTransport, PROBE_TOOL_NAME};
    use grokhub_core::model_registry::profile::read_profiles;
    use grokhub_core::model_registry::Prices;
    use std::cell::Cell;

    struct Fixed(SourceKind, Vec<ModelMeta>, Cell<u32>);
    impl CatalogSource for Fixed {
        fn kind(&self) -> SourceKind {
            self.0
        }
        fn fetch(&self) -> Result<Listing, String> {
            self.2.set(self.2.get() + 1);
            Ok(Listing::new(self.0, self.1.clone()))
        }
    }

    fn meta(id: &str) -> ModelMeta {
        ModelMeta {
            id: id.into(),
            context_length: Some(500_000),
            prices: Prices { prompt: Some(1), cached: Some(1), completion: Some(1), ..Prices::default() },
            efforts: Some(vec!["low".into(), "high".into()]),
            tool_calling: Some(true),
            ..ModelMeta::default()
        }
    }

    /// Counting fake transport: proves the probe path makes no network call.
    struct Counting(u32, bool);
    impl ProbeTransport for Counting {
        fn send(&mut self, call: &ProbeCall) -> Result<ProbeReply, String> {
            self.0 += 1;
            let mut r = ProbeReply { status: 200, latency_ms: 300, input_tokens: 50, output_tokens: 8, cost_ticks: 900, ..ProbeReply::default() };
            match call.kind {
                ProbeKind::Tool if self.1 => r.tool_calls = vec![(PROBE_TOOL_NAME.into(), "not json".into())],
                ProbeKind::Tool => r.tool_calls = vec![(PROBE_TOOL_NAME.into(), r#"{"word":"ping"}"#.into())],
                ProbeKind::Json => r.text = r#"{"a":"ok","b":3,"c":true}"#.into(),
                _ => {}
            }
            Ok(r)
        }
    }

    #[test]
    fn a_refresh_writes_events_profiles_and_queues_the_default_first_then_probes_with_a_fake() {
        let dir = test_dir("route-refresh");
        let api = Fixed(SourceKind::XaiApi, vec![meta("grok-4.5"), meta("grok-4.7"), ModelMeta::bare("grok-3-mini-fast")], Cell::new(0));
        let gb = Fixed(SourceKind::GrokBuild, vec![ModelMeta::bare("grok-build-x")], Cell::new(0));
        let job = RefreshJob {
            config_dir: dir.clone(),
            sources: vec![&api, &gb],
            credential: Credential::Plan,
            named: vec!["grok-2-pinned".into()],
            default_model: "grok-4.7".into(),
            now_ms: 1_000,
        };
        let done = run_refresh(&job);
        assert_eq!(done.errors, Vec::<String>::new());
        assert_eq!(done.events.iter().filter(|e| e.kind() == "added").count(), 4);
        let spans = read_spans(&dir, REGISTRY_TRACE).unwrap();
        assert_eq!(spans.len(), done.events.len());
        assert!(spans.iter().all(|s| s.origin == Origin::SelfManage));
        let profiles = read_profiles(&dir);
        // Ghosts (the placeholder and the pinned id) get no profile.
        assert_eq!(profiles.keys().cloned().collect::<Vec<_>>(), vec!["grok-4.5", "grok-4.7", "grok-build-x"]);
        assert_eq!(profiles["grok-build-x"].probe.status, "not_run: gb_only");
        assert_eq!(profiles["grok-4.7"].probe.status, "queued");
        assert!(!profiles["grok-4.7"].usable);
        let queue: Onboarding = read_json(&onboarding_path(&dir)).unwrap();
        assert_eq!(queue.queue, vec!["grok-4.7".to_string(), "grok-4.5".into()]);
        // An unchanged refresh writes no profile.
        let again = run_refresh(&RefreshJob { now_ms: 2_000, ..job });
        assert_eq!((again.events.len(), again.profiles_written), (0, 0));
        // The probe runs on the fake, one guard per call, and logs one line.
        let mut fake = Counting(0, false);
        let mut guards = 0;
        let p = probe_next(&dir, Credential::Plan, 3_000, &mut |m, c| {
            let mut g = || {
                guards += 1;
                true
            };
            run_probe(m, c, ProbeEnv { transport: &mut fake, guard: &mut g, wait: &mut |_| {}, on_call: &mut |_, _| {} })
        })
        .unwrap();
        assert_eq!((p.model.as_str(), p.usable, p.version), ("grok-4.7", true, 2));
        assert_eq!(fake.0, guards);
        let log = std::fs::read_to_string(probe_log_path(&dir)).unwrap();
        assert_eq!(log.lines().count(), 1);
        assert!(log.contains("\"model\":\"grok-4.7\"") && log.contains("\"status\":\"ok\""), "{log}");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_bad_tool_shape_is_unusable_and_why_models_says_so() {
        let dir = test_dir("route-refresh-bad");
        let api = Fixed(SourceKind::XaiApi, vec![meta("grok-4.7")], Cell::new(0));
        run_refresh(&RefreshJob { config_dir: dir.clone(), sources: vec![&api], credential: Credential::Plan, named: vec![], default_model: "grok-4.7".into(), now_ms: 1 });
        let mut fake = Counting(0, true);
        let p = probe_next(&dir, Credential::Plan, 2, &mut |m, c| {
            run_probe(m, c, ProbeEnv { transport: &mut fake, guard: &mut || true, wait: &mut |_| {}, on_call: &mut |_, _| {} })
        })
        .unwrap();
        assert_eq!((p.usable, p.unusable_reason.as_deref()), (false, Some("Tool calls didn't come back in the right shape.")));
        let text = crate::route::why_models_text(&load_registry(&dir), &read_profiles(&dir));
        assert_eq!(text, "grok-4.7 · live · not usable. In your model list. Tool calls didn't come back in the right shape.");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn half_open_checks_share_the_daily_probe_cap() {
        use grokhub_core::model_registry::probe::PROBE_RUNS_PER_DAY;
        use grokhub_core::model_registry::{CallStatus, ModelState, Observation};
        let dir = test_dir("route-refresh-half-open");
        let ids: Vec<String> = (1..=6).map(|i| format!("grok-4.{i}")).collect();
        let mut reg = Registry::default();
        reg.apply_refresh(&[Listing::new(SourceKind::XaiApi, ids.iter().map(|i| meta(i)).collect())], &[], 1);
        for id in &ids {
            let fails: Vec<Observation> = (0..3)
                .map(|t| Observation { model: id.clone(), ts_ms: 10 + t, status: CallStatus::Error, latency_ms: 0, served_model: None, endpoint_ok: true, reasoning_tokens: 0, cost_ticks: 0, http: 503 })
                .collect();
            reg.fold(&fails);
            assert_eq!(reg.get(id).unwrap().state, ModelState::Quarantined);
        }
        // A day later every backoff ran out: six half-open checks are due.
        let now = 1 + grokhub_core::model_registry::probe::DAY_MS;
        assert_eq!(reg.half_open_due(now).len(), 6);
        onboard(&dir, &mut reg, &[], Credential::Plan, "", now);
        save_registry(&dir, &reg).unwrap();
        let mut ran = 0;
        while probe_next(&dir, Credential::Plan, now, &mut |_, _| ProbeResult { status: "ok".into(), ..ProbeResult::default() }).is_some() {
            ran += 1;
        }
        assert_eq!(ran, PROBE_RUNS_PER_DAY as usize, "the sixth waits for tomorrow");
        let reg = load_registry(&dir);
        let live = ids.iter().filter(|i| reg.get(i).unwrap().state == ModelState::Live).count();
        assert_eq!((live, ids.len() - live), (5, 1));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn an_api_key_route_is_metered_and_never_queued() {
        let dir = test_dir("route-refresh-key");
        let api = Fixed(SourceKind::XaiApi, vec![meta("grok-4.7")], Cell::new(0));
        run_refresh(&RefreshJob { config_dir: dir.clone(), sources: vec![&api], credential: Credential::ApiKey, named: vec![], default_model: "grok-4.7".into(), now_ms: 1 });
        assert_eq!(read_profiles(&dir)["grok-4.7"].probe.status, "not_run: cost_class");
        assert_eq!(probe_next(&dir, Credential::ApiKey, 2, &mut |_, _| panic!("no probe")), None);
        let _ = std::fs::remove_dir_all(dir);
    }
}
