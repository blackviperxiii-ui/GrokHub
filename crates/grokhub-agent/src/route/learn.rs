//! Router R3a: the I/O around the pure tuner ([`super::tune`]) and DE2
//! ([`super::ladder::routine_rung`]). Reads route records (`model-calls`
//! spans) and Spike-7 outcome records, writes `models/scorecards.json`,
//! `models/tune_state.json` and, only through the ChangeLedger,
//! `{config}/route_tuning.json` (kind `model`, origin `self_manage`, Undo puts
//! the old bytes back). Self-tuning refuses to run while the VerifyGate or
//! outcome hook is a stub.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use grokhub_core::model_registry::profile::{read_profiles, ModelProfile};
use grokhub_core::model_registry::store::{load_registry, models_dir, read_json, write_json};
use grokhub_core::model_registry::Registry;
use grokhub_core::outcome::{read_outcomes, OutcomeResult, TaskOutcome};

use super::guard::load_overrides;
use super::ladder::{rung, Band, Routine, ROUTINE_WINDOW_MS};
use super::local::is_local;
use super::log::{RouteRecord, ROUTE_TRACE};
use super::policy::{approved, class_row, expected_cost_usd, fits_any_cost, Fit, CLASS_TABLE};
use super::signals::{OutcomeSource, VerifySource};
use super::spend::{premium_grants, spend_settings, Spend};
use super::table::{load_table, profiles_hash};
use super::tune::{self, Arm, Candidate, Change, Evidence, Filters, Snap, Stage, Step, TuneState, Tuning, Verdict, DAY_MS, REJECT_DAYS, WEEK_MS};
use crate::harness::{private_write, read_spans_tail, record_change, ChangeKind, ChangeTarget, Origin};

pub const TUNING_FILE: &str = "route_tuning.json";
/// The ledger id of the tuning file.
pub const TUNING_ID: &str = "route_tuning";
pub const STATE_FILE: &str = "tune_state.json";
pub const SCORECARDS_FILE: &str = "scorecards.json";
pub const TUNING_SCHEMA: u32 = 1;
/// Route records the tuner reads from the end of the model-call log.
pub const TUNE_SCAN_LINES: usize = 20_000;
/// Route records DE2 reads to find what rung a task ran at.
pub const ROUTINE_SCAN_LINES: usize = 4_000;
/// What a tick says when a hook is a stub.
pub const NEEDS_DATA: &str = "self-tuning waits: the VerifyGate or outcome hook has no real source";

/// `{config}/route_tuning.json`.
pub fn tuning_path(config_dir: &Path) -> PathBuf {
    config_dir.join(TUNING_FILE)
}

pub fn state_path(config_dir: &Path) -> PathBuf {
    models_dir(config_dir).join(STATE_FILE)
}

pub fn scorecards_path(config_dir: &Path) -> PathBuf {
    models_dir(config_dir).join(SCORECARDS_FILE)
}

pub fn load_tuning(config_dir: &Path) -> Tuning {
    read_json(&tuning_path(config_dir)).unwrap_or_default()
}

pub fn load_state(config_dir: &Path) -> TuneState {
    read_json(&state_path(config_dir)).unwrap_or_default()
}

/// `route_tuning.json` as one ChangeLedger target, so Undo puts back the exact bytes.
pub struct TuningFile<'a> {
    pub path: &'a Path,
}

impl ChangeTarget for TuningFile<'_> {
    fn kind(&self) -> ChangeKind {
        ChangeKind::Model
    }

    fn id(&self) -> Result<String, String> {
        Ok(TUNING_ID.into())
    }

    fn file(&self) -> Result<PathBuf, String> {
        Ok(self.path.to_path_buf())
    }

    fn read(&self) -> Result<Option<Vec<u8>>, String> {
        match fs::read(self.path) {
            Ok(b) => Ok(Some(b)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.to_string()),
        }
    }

    fn put(&self, bytes: Option<&[u8]>) -> Result<(), String> {
        match bytes {
            Some(b) => private_write(self.path, b),
            None => match fs::remove_file(self.path) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.to_string()),
                _ => Ok(()),
            },
        }
    }

    fn label_of(&self, _bytes: &[u8]) -> String {
        "router tuning".into()
    }
}

/// Write `tuning` through the ledger (an empty tuning removes the file).
pub fn write_tuning(config_dir: &Path, tuning: &Tuning, reason: &str) -> Result<(), String> {
    let path = tuning_path(config_dir);
    let mut t = tuning.clone();
    t.schema = TUNING_SCHEMA;
    let bytes = if t.is_empty() { None } else { Some(serde_json::to_string_pretty(&t).map_err(|e| e.to_string())?) };
    let target = TuningFile { path: &path };
    record_change(config_dir, &target, Origin::SelfManage, reason, || target.put(bytes.as_deref().map(str::as_bytes)))?;
    Ok(())
}

/// The tuning and tuner state the live router reads, re-read only when a file changes.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LiveTune {
    pub tuning: Tuning,
    pub state: TuneState,
}

type Stamp = (PathBuf, Option<SystemTime>, Option<SystemTime>);
static LIVE: Mutex<Option<(Stamp, Arc<LiveTune>)>> = Mutex::new(None);

fn mtime(p: &Path) -> Option<SystemTime> {
    fs::metadata(p).and_then(|m| m.modified()).ok()
}

pub fn live_tune(config_dir: &Path) -> Arc<LiveTune> {
    let stamp = (config_dir.to_path_buf(), mtime(&tuning_path(config_dir)), mtime(&state_path(config_dir)));
    let mut c = LIVE.lock().unwrap_or_else(|e| e.into_inner());
    match &*c {
        Some((s, t)) if *s == stamp => t.clone(),
        _ => {
            let t = Arc::new(LiveTune { tuning: load_tuning(config_dir), state: load_state(config_dir) });
            *c = Some((stamp, t.clone()));
            t
        }
    }
}

fn table_class(class: &str) -> Option<&'static str> {
    class_row(class).map(|r| r.class)
}

/// The outcome a route record's episode finished with, if any.
fn outcome_of<'a>(outcomes: &'a [TaskOutcome], episode: &str) -> Option<&'a TaskOutcome> {
    if episode.is_empty() {
        return None;
    }
    let prefix = format!("{episode}:");
    outcomes.iter().rev().find(|o| (!o.episode.is_empty() && o.episode == episode) || o.task.starts_with(&prefix))
}

/// The record's cost: the reported ticks, else list prices × tokens.
fn cost_of(rec: &RouteRecord, reg: &Registry) -> f64 {
    let Some(out) = &rec.outcome else {
        return 0.0;
    };
    if out.cost_usd > 0.0 {
        return out.cost_usd;
    }
    let Some(p) = reg.get(&rec.used.model).map(|r| r.meta.prices.clone()) else {
        return 0.0;
    };
    let t = out.tokens;
    let (prompt, completion) = (p.prompt.unwrap_or(0), p.completion.unwrap_or(0));
    let cached = p.cached.unwrap_or(prompt);
    // Prices are USD cents per 100M tokens.
    let cents = (t.input.saturating_sub(t.cached) * prompt + t.cached * cached + (t.out + t.reasoning) * completion) as f64 / 100_000_000.0;
    cents / 100.0
}

/// What the route log says, as tuner steps plus shadow counts.
#[derive(Debug, Clone, Default)]
pub struct Log {
    pub steps: Vec<Step>,
    /// Per candidate id: shadow steps (time-stamped) and violations.
    pub shadow: BTreeMap<String, Vec<(u64, bool)>>,
}

/// Route records and outcomes as tuner steps. Holdout and shadow-only records
/// and calls with no outcome (cancelled, Grok Build) are left out.
pub fn steps_from(records: &[(u64, RouteRecord)], outcomes: &[TaskOutcome], reg: &Registry, now_ms: u64) -> Log {
    let mut log = Log::default();
    for (ts, r) in records {
        if let Some(mark) = r.tune.as_deref().and_then(|t| t.strip_prefix("shadow:")) {
            let (id, bad) = match mark.strip_suffix('!') {
                Some(id) => (id, true),
                None => (mark, false),
            };
            log.shadow.entry(id.to_string()).or_default().push((*ts, bad));
        }
        let (Some(class), Some(out)) = (table_class(&r.class), r.outcome.as_ref()) else {
            continue;
        };
        if r.shadow || r.holdout {
            continue;
        }
        let pass = match out.verify.as_deref() {
            Some("ok") => Some(true),
            Some("reject") => Some(false),
            _ => match outcome_of(outcomes, &r.episode).map(|o| o.effective(now_ms)) {
                Some(OutcomeResult::Success) => Some(true),
                Some(OutcomeResult::Failure) => Some(false),
                _ => None,
            },
        };
        let arm = match r.tune.as_deref().and_then(|t| t.strip_prefix("canary:")) {
            Some(id) => Arm::Canary(id.to_string()),
            None => Arm::Live,
        };
        log.steps.push(Step {
            at: *ts,
            class: class.to_string(),
            model: r.used.model.clone(),
            effort: r.used.effort.clone(),
            arm,
            pass,
            rework: r.rule_ids.iter().any(|x| matches!(x.as_str(), "E1" | "E2" | "E3" | "retry")),
            cost_usd: cost_of(r, reg),
            latency_ms: r.signals.latency.unwrap_or(0),
        });
    }
    log
}

fn read_records(config_dir: &Path, lines: usize) -> Vec<(u64, RouteRecord)> {
    let (spans, _) = read_spans_tail(config_dir, ROUTE_TRACE, lines);
    spans.into_iter().filter_map(|s| s.route.map(|r| (s.ts_ms, *r))).collect()
}

/// DE2: this signature's finished runs in `class` and the rung each ran at
/// (the highest effort its episode sent).
pub fn routine_from(records: &[(u64, RouteRecord)], outcomes: &[TaskOutcome], signature: &str, class: &str, now_ms: u64) -> Routine {
    let mut out = Routine::default();
    if signature.is_empty() {
        return out;
    }
    let class = table_class(class).unwrap_or(class);
    let since = now_ms.saturating_sub(ROUTINE_WINDOW_MS);
    for o in outcomes.iter().filter(|o| o.signature == signature && o.finished_at >= since) {
        let ran = records
            .iter()
            .filter(|(_, r)| table_class(&r.class) == Some(class) && !r.holdout && outcome_of(std::slice::from_ref(o), &r.episode).is_some())
            .filter_map(|(_, r)| r.used.effort.as_deref().and_then(rung))
            .max();
        match (o.effective(now_ms), ran) {
            (OutcomeResult::Success, Some(r)) => out.successes.push((r, o.finished_at)),
            (OutcomeResult::Failure, _) => out.last_failure = out.last_failure.max(Some(o.finished_at)),
            _ => {}
        }
    }
    out
}

/// DE2 for one call: read the outcomes and the route log's tail.
pub fn routine_for(config_dir: &Path, signature: &str, class: &str, now_ms: u64) -> Routine {
    if signature.is_empty() {
        return Routine::default();
    }
    let outcomes = read_outcomes(config_dir);
    if !outcomes.iter().any(|o| o.signature == signature) {
        return Routine::default();
    }
    routine_from(&read_records(config_dir, ROUTINE_SCAN_LINES), &outcomes, signature, class, now_ms)
}

/// The registry and R2b spend rules as tuner filters.
pub struct RegistryFilters<'a> {
    pub reg: &'a Registry,
    pub profiles: &'a BTreeMap<String, ModelProfile>,
    pub spend: Spend,
    pub now_ms: u64,
}

impl Filters for RegistryFilters<'_> {
    fn allowed(&self, class: &str, model: &str) -> bool {
        let fit = Fit { needs_tools: !class.starts_with("background:"), ..Fit::default() };
        !is_local(model) && fits_any_cost(self.reg, self.profiles, model, fit, self.now_ms) && approved(self.reg, model, false, &self.spend)
    }

    fn expected_cost(&self, class: &str, model: &str) -> Option<f64> {
        expected_cost_usd(&self.reg.get(model)?.meta.prices, class, 0)
    }
}

/// Each class's live first model and tuned band.
pub fn live_view(config_dir: &Path, tuning: &Tuning) -> (BTreeMap<String, String>, BTreeMap<String, Band>) {
    let table = tune::tuned_table(&load_table(config_dir), tuning);
    let overrides = load_overrides(config_dir);
    let mut top = BTreeMap::new();
    let mut bands = BTreeMap::new();
    for row in CLASS_TABLE {
        if let Some(first) = table.rows(row.class).first() {
            top.insert(row.class.to_string(), first.model.clone());
        }
        let bump = overrides.get(row.class).map(|o| o.steps).unwrap_or(0);
        bands.insert(row.class.to_string(), Band::tuned(row, bump, tuning.starts.get(row.class).map(String::as_str)));
    }
    (top, bands)
}

/// Evidence for one candidate from the log.
pub fn evidence(c: &Candidate, log: &Log) -> Evidence {
    let shadow = log.shadow.get(&c.id).map(Vec::as_slice).unwrap_or_default();
    let since_canary = c.canary_at.unwrap_or(u64::MAX);
    let in_class = |s: &&Step| s.class == c.class;
    Evidence {
        shadow_steps: shadow.iter().filter(|(t, _)| *t >= c.created_at).count() as u32,
        violations: shadow.iter().filter(|(t, bad)| *t >= c.created_at && *bad).count() as u32,
        canary: Snap::of(log.steps.iter().filter(in_class).filter(|s| s.arm == Arm::Canary(c.id.clone()) && s.at >= since_canary)),
        live: Snap::of(log.steps.iter().filter(in_class).filter(|s| s.arm == Arm::Live && s.at >= since_canary)),
        since_promotion: Snap::of(log.steps.iter().filter(in_class).filter(|s| s.arm == Arm::Live && c.promoted_at.is_some_and(|t| s.at >= t))),
    }
}

/// What one tick did.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TuneRun {
    /// Candidate ids that moved from shadow to canary.
    pub canaries: Vec<String>,
    pub promoted: Vec<String>,
    pub rolled_back: Vec<String>,
    pub rejected: Vec<String>,
    /// New cost-raising cards waiting for the weekly self-review.
    pub cards: Vec<String>,
}

fn plain(class: &str) -> &str {
    class_row(class).map(|r| r.plain).unwrap_or(class)
}

/// One self-tuning pass: scorecards, every candidate judged (promote, roll
/// back, keep), then the weekly candidate build when due (or when the model
/// list or a profile changed). Refuses to run on a stub hook.
pub fn tick(config_dir: &Path, verify: &dyn VerifySource, outcome_hook: &dyn OutcomeSource, now_ms: u64) -> Result<TuneRun, String> {
    if !verify.ready() || !outcome_hook.ready() {
        return Err(NEEDS_DATA.into());
    }
    let reg = load_registry(config_dir);
    let profiles = read_profiles(config_dir);
    let outcomes = read_outcomes(config_dir);
    let log = steps_from(&read_records(config_dir, TUNE_SCAN_LINES), &outcomes, &reg, now_ms);
    let cards = tune::scorecards(&log.steps, now_ms);
    write_json(&scorecards_path(config_dir), &cards)?;
    let mut state = load_state(config_dir);
    let mut tuning = load_tuning(config_dir);
    let mut run = TuneRun::default();
    for i in 0..state.candidates.len() {
        let c = state.candidates[i].clone();
        let Some(change) = c.change.clone() else {
            continue;
        };
        // You undid it (Undo on the ledger line): it stays off for a while.
        if matches!(c.stage, Stage::Watch | Stage::Kept | Stage::Accepted) && !tuning.has(&c.class, &change) {
            let c = &mut state.candidates[i];
            c.stage = Stage::Rejected;
            c.reason = "undone".into();
            c.ended_at = Some(now_ms);
            c.until = Some(now_ms + REJECT_DAYS * DAY_MS);
            continue;
        }
        if !c.active() {
            continue;
        }
        let ev = evidence(&c, &log);
        let week = state.promotions_since(now_ms.saturating_sub(WEEK_MS));
        match tune::judge(&c, &ev, week, now_ms) {
            Verdict::Wait => {}
            Verdict::StartCanary => {
                let c = &mut state.candidates[i];
                c.stage = Stage::Canary;
                c.canary_at = Some(now_ms);
                run.canaries.push(c.id.clone());
            }
            Verdict::Promote { cost_gain_pct, latency_gain_pct } => {
                let before = tuning.value(&c.class, &change);
                tuning.apply(&c.class, &change);
                let reason = format!("router tuned {}: {} ({cost_gain_pct:.0}% cheaper, {latency_gain_pct:.0}% faster, quality held)", plain(&c.class), change.label());
                write_tuning(config_dir, &tuning, &reason)?;
                let c = &mut state.candidates[i];
                c.stage = Stage::Watch;
                c.promoted_at = Some(now_ms);
                c.baseline = Some(ev.live);
                c.promoted_snap = Some(ev.canary);
                c.before = before;
                run.promoted.push(c.id.clone());
            }
            Verdict::Reject { why, rollback } => {
                if rollback {
                    tuning.set(&c.class, &change, c.before.clone());
                    write_tuning(config_dir, &tuning, &format!("router rolled back {}: {why}", plain(&c.class)))?;
                    run.rolled_back.push(c.id.clone());
                } else {
                    run.rejected.push(c.id.clone());
                }
                let c = &mut state.candidates[i];
                c.stage = Stage::Rejected;
                c.reason = why;
                c.ended_at = Some(now_ms);
                c.until = Some(now_ms + REJECT_DAYS * DAY_MS);
            }
            Verdict::Keep => state.candidates[i].stage = Stage::Kept,
        }
    }
    let inputs = format!("{}:{}", reg.hash, profiles_hash(&profiles));
    if now_ms.saturating_sub(state.built_at) >= WEEK_MS || state.inputs_hash != inputs {
        let (top, bands) = live_view(config_dir, &tuning);
        let filters = RegistryFilters { reg: &reg, profiles: &profiles, spend: Spend { settings: spend_settings(), grants: premium_grants(config_dir), ..Spend::default() }, now_ms };
        let p = tune::propose(&cards, &log.steps, &state, &top, &bands, &filters, now_ms);
        state.seq += (p.auto.len() + p.cards.len()) as u64;
        run.cards.extend(p.cards.iter().map(|c| c.id.clone()));
        state.candidates.extend(p.auto);
        state.candidates.extend(p.cards);
        state.built_at = now_ms;
        state.inputs_hash = inputs;
    }
    // Ended candidates older than their block, and their history, stay 8 weeks.
    state.candidates.retain(|c| c.active() || c.ended_at.or(c.promoted_at).unwrap_or(c.created_at) + 8 * WEEK_MS > now_ms);
    write_json(&state_path(config_dir), &state)?;
    Ok(run)
}

/// Cost-raising cards the weekly self-review may post now: at most
/// [`tune::ROUTER_CARDS_PER_WEEK`] a week. Each is `(source id, title, body)`.
pub fn cards_due(config_dir: &Path, now_ms: u64) -> Vec<(String, String, String)> {
    let state = load_state(config_dir);
    let sent = state.cards_at.iter().filter(|t| **t + WEEK_MS > now_ms).count();
    let room = tune::ROUTER_CARDS_PER_WEEK.saturating_sub(sent);
    state
        .candidates
        .iter()
        .filter(|c| c.stage == Stage::Card)
        .take(room)
        .map(|c| {
            let (title, body) = tune::card_text(c);
            (grokhub_core::self_review::router_source(&c.id), title, body)
        })
        .collect()
}

/// The weekly self-review posted these cards: they count against this week's two.
pub fn cards_posted(config_dir: &Path, n: usize, now_ms: u64) {
    if n == 0 {
        return;
    }
    let mut state = load_state(config_dir);
    state.cards_at.retain(|t| *t + WEEK_MS > now_ms);
    state.cards_at.extend(std::iter::repeat_n(now_ms, n));
    let _ = write_json(&state_path(config_dir), &state);
}

/// Your Accept click on a cost-raising card: the change goes in through the
/// ledger (Undo restores the file byte-identical). Nothing else applies one.
pub fn accept_card(config_dir: &Path, id: &str, now_ms: u64) -> Result<String, String> {
    let mut state = load_state(config_dir);
    let Some(c) = state.candidates.iter_mut().find(|c| c.id == id && c.stage == Stage::Card) else {
        return Err("That suggestion is no longer open.".into());
    };
    let change = c.change.clone().ok_or("That suggestion has no change.")?;
    let mut tuning = load_tuning(config_dir);
    c.before = tuning.value(&c.class, &change);
    tuning.apply(&c.class, &change);
    let what = format!("{}: {}", plain(&c.class), change.label());
    write_tuning(config_dir, &tuning, &format!("you accepted: {what}"))?;
    c.stage = Stage::Accepted;
    c.promoted_at = Some(now_ms);
    write_json(&state_path(config_dir), &state)?;
    Ok(format!("Router changed ({what}). Undo is on the change list."))
}

/// The weekly self-review's router line.
pub fn review_line(config_dir: &Path, now_ms: u64) -> String {
    tune::review_line(&load_state(config_dir), now_ms)
}

/// `/why table`'s tuner lines: the live tuning, candidates and canary status.
pub fn why_table_lines(config_dir: &Path, now_ms: u64) -> Vec<String> {
    let state = load_state(config_dir);
    let tuning = load_tuning(config_dir);
    let version = crate::harness::ChangeLedger::load_kind(config_dir, ChangeKind::Model).all().iter().filter(|c| c.id == TUNING_ID).count() as u32;
    let mut ev = BTreeMap::new();
    if state.candidates.iter().any(|c| matches!(c.stage, Stage::Shadow | Stage::Canary)) {
        let log = steps_from(&read_records(config_dir, TUNE_SCAN_LINES), &read_outcomes(config_dir), &load_registry(config_dir), now_ms);
        for c in &state.candidates {
            ev.insert(c.id.clone(), evidence(c, &log));
        }
    }
    tune::why_lines(&tuning, version, &state, &ev)
}

/// What the router should do with one step's tuning: the start to use, the
/// order to merge, and the mark for its route record.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct StepTune {
    /// The class start to use (tuned, or the canary's).
    pub start: Option<String>,
    /// A canary's model to put first.
    pub canary_order: Option<String>,
    /// `canary:<id>` when this step is in a canary.
    pub mark: Option<String>,
    /// A shadow candidate to compute next to the live pick.
    pub shadow: Option<Candidate>,
}

/// The tuning for one step of `class` in `episode` (pure over `live`).
pub fn step_tune(live: &LiveTune, class: &str, episode: &str, risk: &tune::StepRisk<'_>) -> StepTune {
    let Some(class) = table_class(class) else {
        return StepTune::default();
    };
    let mut out = StepTune { start: live.tuning.starts.get(class).cloned(), ..StepTune::default() };
    let Some(c) = live.state.testing(class) else {
        return out;
    };
    let eligible = tune::canary_eligible(risk);
    match (c.stage, &c.change) {
        (Stage::Canary, Some(change)) if eligible && tune::in_canary(episode, &c.id) => {
            match change {
                Change::Start { effort } => out.start = Some(effort.clone()),
                Change::Order { model } => out.canary_order = Some(model.clone()),
            }
            out.mark = Some(format!("canary:{}", c.id));
        }
        (Stage::Shadow, Some(_)) if eligible => out.shadow = Some(c.clone()),
        _ => {}
    }
    out
}
