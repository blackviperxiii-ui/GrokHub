//! C8–C11: per-user learning on top of the ask read ([`super::ask`],
//! [`super::knn`]).
//!
//! Every typed chat turn has arms: a model at an effort ([`ArmSpec`]), built
//! from the registry and the routing table the router already ranks. The xAI
//! models in Auto's included pool, each at the efforts its listing names inside
//! the lane's band, plus one model on a provider you set up when the table
//! ranks one for the lane. The rules and the neighbours still pick the turn's
//! route (the base arm); the learner keeps a Gaussian posterior of the reward
//! of each arm in each context (lane, kind, difficulty band) and Thompson
//! samples among:
//!
//! - the base arm and arms with [`MIN_EVIDENCE`] settled turns behind them;
//! - on a guarded turn (stakes, a Flagship lane, hard, agentic, a redo or a
//!   climb), only arms at or above the base in both effort and model strength;
//! - on [`EXPLORE_RATE`] of the other turns, once the context has
//!   [`EXPLORE_AFTER`] settled turns, one arm at or above the base, never
//!   below it.
//!
//! The read is the prior: the base arm starts at [`Q_PRIOR`], an arm below it
//! loses [`UNDER_STEP`] a rung and [`WEAKER`] for a weaker model, and every arm
//! pays its expected cost at [`LAMBDA_PER_USD`]. Reward is quality minus
//! that cost. Quality is settled on the next typed turn ([`quality`]).
//!
//! Every routing choice is logged on this computer with its propensity
//! (`models/route_bandit.jsonl`: ids, counts and probabilities, never a word
//! of the turn) so the tuner ([`super::tune`]) can score a shadow candidate
//! offline ([`ope`]) and the R1 guard can read the same rewards
//! ([`guard_stats`]). Nothing here is shown or offered to you.
//!
//! The learner only steers a route [`Router::choose`] serves again with the
//! arm's model and rung, so every eligibility, health, cost, privacy and safety
//! filter still holds, and a model change on a pinned long chat must pass
//! [`policy::pin_switch`].

use std::cmp::Reverse;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::SystemTime;

use grokhub_core::model_registry::cost_class::{fast_base, CostClass};
use grokhub_core::model_registry::profile::ModelProfile;
use grokhub_core::model_registry::store::{append_line, models_dir, read_json, write_json};
use grokhub_core::model_registry::{ModelState, Prices, Registry, EFFORT_LADDER};
use serde::{Deserialize, Serialize};

use super::guard::{self, ClassStats};
use super::ladder::{self, rung, turn_hash, Band, Move};
use super::policy::{self, Fit};
use super::table::{self as rtable, RoutingTable};
use super::{attention, local, providers, Route, RouteInput, Router};

pub const STATE_FILE: &str = "route_bandit.json";
pub const LOG_FILE: &str = "route_bandit.jsonl";
pub const STATE_SCHEMA: u32 = 1;
/// Propensity lines kept; the oldest go first.
pub const LOG_CAP: usize = 5_000;
/// Share of low-stakes turns that explore, once a context has history.
pub const EXPLORE_RATE: f64 = 0.03;
/// A dollar spent weighs as much as one answer you had to ask for again.
pub const LAMBDA_PER_USD: f64 = 1.0;
/// Settled turns an arm needs before it may be picked on what it learned.
pub const MIN_EVIDENCE: u32 = 20;
/// Settled turns a context needs before it explores at all.
pub const EXPLORE_AFTER: u32 = 20;
/// Pseudo-turns the read's prior counts for, and their spread.
pub const PRIOR_N: f64 = 2.0;
pub const SIGMA0: f64 = 0.5;
/// The prior quality of the read's own arm (a turn you moved on from).
pub const Q_PRIOR: f64 = 0.8;
/// Prior quality an arm loses per rung below the read, and for a weaker model.
pub const UNDER_STEP: f64 = 0.15;
pub const WEAKER: f64 = 0.10;
/// Prior quality a provider arm loses on a turn xAI took: xAI goes first.
pub const XAI_FIRST: f64 = 0.05;
/// Thompson draws that estimate a choice's propensity.
pub const MC_DRAWS: usize = 512;
/// Propensities below this are read as this in [`ope`].
pub const P_FLOOR: f64 = 0.01;
/// An arm's counts are scaled down past this, so old turns fade.
pub const N_CAP: f64 = 200.0;
/// Costs above this count as this.
pub const COST_CAP_USD: f64 = 1.0;
/// xAI models in a turn's arms, the base one included.
pub const MAX_MODELS: usize = 3;
/// Rough reasoning tokens per rung of [`EFFORT_LADDER`].
pub const REASONING_TOKENS: [u64; 7] = [0, 100, 400, 1_200, 3_500, 8_000, 16_000];
/// A model weaker than the lane's strongest gets arms only up to medium.
const TOP_BELOW_STRONGEST: usize = 3;

/// Quality of a settled turn.
pub const Q_REDO: f64 = 0.0;
pub const Q_TROUBLE: f64 = 0.5;
pub const Q_MOVED_ON: f64 = 0.8;
/// You copied code from the answer.
pub const Q_COPY_BONUS: f64 = 0.2;

/// The rules a learned or exploring arm adds to the route.
pub const LEARNED_RULE: &str = "bandit:learned";
pub const EXPLORE_RULE: &str = "bandit:explore";
/// A guarded turn's reason when attention already moved it: the learner stays out.
pub const ATTENTION_GUARD: &str = "attention";

// ---------------------------------------------------------------- the RNG

/// SplitMix64: small, fast and seeded, so every draw replays in a test.
#[derive(Debug, Clone)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed)
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform in [0, 1).
    pub fn f64(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }

    /// Uniform in 0..n (n ≥ 1).
    pub fn below(&mut self, n: usize) -> usize {
        ((self.f64() * n as f64) as usize).min(n.saturating_sub(1))
    }

    /// Standard normal (Box–Muller).
    pub fn normal(&mut self) -> f64 {
        let u1 = self.f64().max(f64::MIN_POSITIVE);
        let u2 = self.f64();
        (-2.0 * u1.ln()).sqrt() * (std::f64::consts::TAU * u2).cos()
    }
}

// ---------------------------------------------------------------- arms and posteriors

/// One arm: a model at a rung, its strength and its expected cost this turn.
#[derive(Debug, Clone, PartialEq)]
pub struct ArmSpec {
    pub model: String,
    /// Rung on [`EFFORT_LADDER`].
    pub rung: usize,
    /// [`attention::reasoning_of`]: higher is a stronger model.
    pub strength: u8,
    /// A model on a provider you added.
    pub provider: bool,
    pub cost_usd: f64,
}

impl ArmSpec {
    pub fn id(&self) -> String {
        arm_id(
            &self.model,
            Some(EFFORT_LADDER[self.rung.min(EFFORT_LADDER.len() - 1)]),
        )
    }
}

/// `model@effort` (`-` for none): the arm's id in the state and the log.
pub fn arm_id(model: &str, effort: Option<&str>) -> String {
    format!("{model}@{}", effort.unwrap_or("-"))
}

/// Rewards seen for one arm in one context.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct Stats {
    pub n: f64,
    pub sum: f64,
    pub sumsq: f64,
}

impl Stats {
    pub fn add(&mut self, r: f64) {
        self.n += 1.0;
        self.sum += r;
        self.sumsq += r * r;
        if self.n > N_CAP {
            let k = N_CAP / self.n;
            self.n *= k;
            self.sum *= k;
            self.sumsq *= k;
        }
    }

    pub fn mean(&self) -> Option<f64> {
        (self.n > 0.0).then(|| self.sum / self.n)
    }
}

/// One context's arms.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Ctx {
    /// Settled turns, any arm.
    #[serde(default)]
    pub settled: u32,
    #[serde(default)]
    pub arms: BTreeMap<String, Stats>,
}

/// `{config}/models/route_bandit.json`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct State {
    #[serde(default)]
    pub schema: u32,
    /// D13: the measured cache hit rate the tuner last read, percent.
    #[serde(default)]
    pub cache_hit_pct: Option<f64>,
    #[serde(default)]
    pub contexts: BTreeMap<String, Ctx>,
}

impl State {
    /// One settled turn of `arm` in `ctx` earned `r`.
    pub fn learn(&mut self, ctx: &str, arm: &str, r: f64) {
        let c = self.contexts.entry(ctx.to_string()).or_default();
        c.settled = c.settled.saturating_add(1);
        c.arms.entry(arm.to_string()).or_default().add(r);
    }
}

/// The context a turn learns in: its lane, its kind and a difficulty band.
pub fn context(lane: &str, kind: &str, d: f64) -> String {
    let band = if d < 0.4 {
        0
    } else if d < 0.7 {
        1
    } else {
        2
    };
    format!("{lane}|{kind}|d{band}")
}

/// The prior mean of `arm`'s reward on a turn the read sent to `base`.
pub fn prior(arm: &ArmSpec, base: &ArmSpec) -> f64 {
    let under = base.rung.saturating_sub(arm.rung) as f64 * UNDER_STEP;
    let weaker = if arm.strength < base.strength {
        WEAKER
    } else {
        0.0
    };
    let theirs = if arm.provider && !base.provider {
        XAI_FIRST
    } else {
        0.0
    };
    Q_PRIOR - under - weaker - theirs - LAMBDA_PER_USD * arm.cost_usd.clamp(0.0, COST_CAP_USD)
}

/// Posterior mean and standard deviation of an arm's mean reward: [`PRIOR_N`]
/// pseudo-turns at `mu0` (spread [`SIGMA0`]) pooled with what `s` saw.
pub fn posterior(s: Stats, mu0: f64) -> (f64, f64) {
    let n = PRIOR_N + s.n;
    let mean = (PRIOR_N * mu0 + s.sum) / n;
    let seen = (s.sumsq - 2.0 * mean * s.sum + s.n * mean * mean).max(0.0);
    let pseudo = PRIOR_N * (SIGMA0 * SIGMA0 + (mu0 - mean).powi(2));
    let var = (pseudo + seen) / n;
    (mean, (var / n).sqrt())
}

/// One turn's arms. `guard` names why the turn may not go below its base.
#[derive(Debug, Clone, PartialEq)]
pub struct Turn {
    pub arms: Vec<ArmSpec>,
    pub base: usize,
    pub guard: Option<&'static str>,
}

impl Turn {
    /// At or above the base in both effort and model strength.
    pub fn up(&self, i: usize) -> bool {
        let (a, b) = (&self.arms[i], &self.arms[self.base]);
        a.rung >= b.rung && a.strength >= b.strength
    }
}

/// What the learner picked, and the probability it had of picking it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Choice {
    pub arm: usize,
    pub p: f64,
    pub explore: bool,
}

fn thompson(cands: &[usize], post: &[(f64, f64)], rng: &mut Rng) -> usize {
    let mut best = (f64::NEG_INFINITY, cands[0]);
    for (k, &i) in cands.iter().enumerate() {
        let (mean, sd) = post[k];
        let x = mean + sd * rng.normal();
        if x > best.0 {
            best = (x, i);
        }
    }
    best.1
}

/// Pick an arm for `turn` from what `ctx` learned. Pure over `rng`.
pub fn choose(turn: &Turn, ctx: &Ctx, rng: &mut Rng) -> Choice {
    let base = &turn.arms[turn.base];
    let stats = |i: usize| {
        ctx.arms
            .get(&turn.arms[i].id())
            .copied()
            .unwrap_or_default()
    };
    let exploit: Vec<usize> = (0..turn.arms.len())
        .filter(|&i| {
            i == turn.base
                || stats(i).n >= MIN_EVIDENCE as f64 && (turn.guard.is_none() || turn.up(i))
        })
        .collect();
    let explore: Vec<usize> = if turn.guard.is_none() && ctx.settled >= EXPLORE_AFTER {
        (0..turn.arms.len())
            .filter(|&i| i != turn.base && turn.up(i))
            .collect()
    } else {
        Vec::new()
    };
    let eps = if explore.is_empty() {
        0.0
    } else {
        EXPLORE_RATE
    };
    let post: Vec<(f64, f64)> = exploit
        .iter()
        .map(|&i| posterior(stats(i), prior(&turn.arms[i], base)))
        .collect();
    let roll = rng.f64();
    let mut mc = Rng::new(rng.next_u64());
    let (arm, explored) = if roll < eps {
        (explore[rng.below(explore.len())], true)
    } else {
        (thompson(&exploit, &post, rng), false)
    };
    let p_ts = if exploit.len() == 1 {
        f64::from(exploit[0] == arm)
    } else {
        let wins = (0..MC_DRAWS)
            .filter(|_| thompson(&exploit, &post, &mut mc) == arm)
            .count();
        wins as f64 / MC_DRAWS as f64
    };
    let p_x = if explore.contains(&arm) {
        1.0 / explore.len() as f64
    } else {
        0.0
    };
    Choice {
        arm,
        p: eps * p_x + (1.0 - eps) * p_ts,
        explore: explored,
    }
}

/// C10: a settled turn's quality. A redo wins over everything; a copied code
/// block lifts a turn you moved on from.
pub fn quality(redo: bool, trouble: bool, copied: bool) -> f64 {
    if redo {
        return Q_REDO;
    }
    let q = if trouble { Q_TROUBLE } else { Q_MOVED_ON };
    if copied {
        (q + Q_COPY_BONUS).min(1.0)
    } else {
        q
    }
}

/// Reward = quality − λ·cost.
pub fn reward(q: f64, cost_usd: f64) -> f64 {
    q - LAMBDA_PER_USD * cost_usd.clamp(0.0, COST_CAP_USD)
}

/// One turn's expected cost on `prices`: the lane's predicted tokens (the
/// prompt at least the chat so far), `hit_pct` of the prompt cached, and the
/// rung's reasoning tokens. `None` when the listing has no prices.
pub fn est_cost_usd(
    prices: &Prices,
    lane: &str,
    ctx_tokens: u64,
    rung: usize,
    hit_pct: u64,
) -> Option<f64> {
    let (input, out, _) = policy::predicted_tokens(lane);
    let input = input.max(ctx_tokens);
    let (prompt, cached, completion) = prices.rates_at(input)?;
    let hit = input * hit_pct.min(100) / 100;
    let out = out + REASONING_TOKENS[rung.min(REASONING_TOKENS.len() - 1)];
    // Prices are USD cents per 100M tokens.
    Some(((input - hit) * prompt + hit * cached + out * completion) as f64 / 1e10)
}

// ---------------------------------------------------------------- the propensity log and OPE

fn is_false(b: &bool) -> bool {
    !*b
}

/// One settled routing choice: ids, numbers and probabilities only.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Line {
    pub at: u64,
    pub ctx: String,
    pub arm: String,
    /// The arm the read picked.
    pub base: String,
    /// The probability the logging policy had of picking `arm`.
    pub p: f64,
    #[serde(default, skip_serializing_if = "is_false")]
    pub explore: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub guard: Option<String>,
    /// The R1 holdout's fixed-High turn (a different policy: never learned from).
    #[serde(default, skip_serializing_if = "is_false")]
    pub hold: bool,
    pub q: f64,
    pub cost: f64,
    pub r: f64,
    /// Shadow candidate id → the arm it would have picked.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub shadow: BTreeMap<String, String>,
}

/// A candidate's offline score over the lines that logged its pick.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Ope {
    /// Lines with the candidate's pick.
    pub n: u32,
    /// Of those, lines where the live arm was the candidate's.
    pub matched: u32,
    /// Mean reward the live policy earned on them.
    pub live: f64,
    /// Inverse propensity score.
    pub ips: f64,
    /// Doubly robust score.
    pub dr: f64,
}

/// r̂(context, arm): the mean reward logged for the pair, else for the arm,
/// else for every line.
struct RewardModel<'a> {
    pair: BTreeMap<(&'a str, &'a str), (f64, u32)>,
    arm: BTreeMap<&'a str, (f64, u32)>,
    all: f64,
}

impl<'a> RewardModel<'a> {
    fn fit(lines: &[&'a Line]) -> Self {
        let mut pair: BTreeMap<(&str, &str), (f64, u32)> = BTreeMap::new();
        let mut arm: BTreeMap<&str, (f64, u32)> = BTreeMap::new();
        let mut total = 0.0;
        for l in lines {
            let e = pair.entry((l.ctx.as_str(), l.arm.as_str())).or_default();
            e.0 += l.r;
            e.1 += 1;
            let e = arm.entry(l.arm.as_str()).or_default();
            e.0 += l.r;
            e.1 += 1;
            total += l.r;
        }
        Self {
            pair,
            arm,
            all: if lines.is_empty() {
                0.0
            } else {
                total / lines.len() as f64
            },
        }
    }

    fn at(&self, ctx: &str, arm: &str) -> f64 {
        let mean = |(s, n): &(f64, u32)| s / *n as f64;
        self.pair
            .get(&(ctx, arm))
            .map(mean)
            .or_else(|| self.arm.get(arm).map(mean))
            .unwrap_or(self.all)
    }
}

/// Score shadow candidate `cand` offline: inverse propensity (IPS) and doubly
/// robust (DR) estimates of its mean reward, next to what the live policy
/// earned on the same turns. Holdout lines are left out.
pub fn ope(lines: &[Line], cand: &str) -> Ope {
    let pool: Vec<&Line> = lines.iter().filter(|l| !l.hold).collect();
    let model = RewardModel::fit(&pool);
    let mine: Vec<(&Line, &str)> = pool
        .iter()
        .filter_map(|l| l.shadow.get(cand).map(|c| (*l, c.as_str())))
        .collect();
    if mine.is_empty() {
        return Ope::default();
    }
    let n = mine.len() as f64;
    let mut out = Ope {
        n: mine.len() as u32,
        ..Ope::default()
    };
    for (l, c) in &mine {
        let w = if l.arm == *c {
            out.matched += 1;
            1.0 / l.p.max(P_FLOOR)
        } else {
            0.0
        };
        out.live += l.r / n;
        out.ips += w * l.r / n;
        out.dr += (model.at(&l.ctx, c) + w * (l.r - model.at(&l.ctx, &l.arm))) / n;
    }
    out
}

/// The R1 guard on the same rewards: per lane, the live turns against the
/// holdout's fixed-High turns, a turn counting as a success when you moved on
/// without a redo or trouble. Lines before `since(lane)` don't count.
pub fn guard_stats(lines: &[Line], since: impl Fn(&str) -> u64) -> BTreeMap<String, ClassStats> {
    let mut out: BTreeMap<String, ClassStats> = BTreeMap::new();
    for l in lines {
        let Some(lane) = l.ctx.split('|').next() else {
            continue;
        };
        if !guard::holdout_eligible(lane) || l.at < since(lane) {
            continue;
        }
        let s = out.entry(lane.to_string()).or_default();
        let arm = if l.hold { &mut s.control } else { &mut s.auto };
        arm.n += 1;
        arm.ok += u32::from(l.q >= Q_MOVED_ON);
    }
    out
}

// ---------------------------------------------------------------- files

pub fn state_path(config_dir: &Path) -> PathBuf {
    models_dir(config_dir).join(STATE_FILE)
}

pub fn log_path(config_dir: &Path) -> PathBuf {
    models_dir(config_dir).join(LOG_FILE)
}

pub fn load_state(config_dir: &Path) -> State {
    read_json(&state_path(config_dir)).unwrap_or_default()
}

fn save_state(config_dir: &Path, state: &State) {
    let mut s = state.clone();
    s.schema = STATE_SCHEMA;
    let _ = write_json(&state_path(config_dir), &s);
}

/// Every readable line of the propensity log, oldest first.
pub fn read_log(config_dir: &Path) -> Vec<Line> {
    std::fs::read_to_string(log_path(config_dir))
        .unwrap_or_default()
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect()
}

/// Keep the newest [`LOG_CAP`] lines.
pub fn trim_log(config_dir: &Path) {
    let lines = read_log(config_dir);
    if lines.len() <= LOG_CAP {
        return;
    }
    let keep: String = lines[lines.len() - LOG_CAP..]
        .iter()
        .filter_map(|l| serde_json::to_string(l).ok())
        .map(|l| l + "\n")
        .collect();
    let _ = crate::harness::private_write(&log_path(config_dir), keep.as_bytes());
}

type Stamp = (PathBuf, Option<SystemTime>);
static STATE: Mutex<Option<(Stamp, State)>> = Mutex::new(None);

fn stamp(config_dir: &Path) -> Stamp {
    let mtime = std::fs::metadata(state_path(config_dir))
        .and_then(|m| m.modified())
        .ok();
    (config_dir.to_path_buf(), mtime)
}

/// The state in memory, re-read when the file changed under it. A change
/// `f` saves is taken as read.
fn with_state<T>(config_dir: &Path, f: impl FnOnce(&mut State) -> T) -> T {
    let mut slot = STATE.lock().unwrap_or_else(|e| e.into_inner());
    let now = stamp(config_dir);
    if !matches!(slot.as_ref(), Some((s, _)) if *s == now) {
        *slot = Some((now, load_state(config_dir)));
    }
    let (s, state) = slot.as_mut().expect("loaded above");
    let out = f(state);
    *s = stamp(config_dir);
    out
}

/// D13 as a tuner input: the measured cache hit rate the cost estimates use.
pub fn set_cache_hit(config_dir: &Path, pct: Option<f64>) {
    with_state(config_dir, |s| {
        if s.cache_hit_pct != pct {
            s.cache_hit_pct = pct;
            save_state(config_dir, s);
        }
    });
}

// ---------------------------------------------------------------- arms from the registry

/// Efforts `model` may be sent at, as rungs, from its listing.
fn rungs_of(reg: &Registry, profiles: &BTreeMap<String, ModelProfile>, model: &str) -> Vec<usize> {
    let menu = profiles
        .get(model)
        .and_then(|p| p.metadata.efforts.clone())
        .or_else(|| reg.get(model).and_then(|r| r.meta.efforts.clone()));
    match menu {
        Some(menu) => menu.iter().filter_map(|e| rung(e)).collect(),
        None if policy::is_new_provider(reg, model) => Vec::new(),
        None if grokhub_core::accepts_reasoning_effort(model) => {
            (super::ask::LOW..EFFORT_LADDER.len()).collect()
        }
        None => Vec::new(),
    }
}

/// C8: the turn's arms, the base arm (`base_model` at `base_rung`, the route
/// the read picked) among them. Pure over the registry, the profiles and the
/// routing table.
#[allow(clippy::too_many_arguments)]
pub fn arms(
    reg: &Registry,
    profiles: &BTreeMap<String, ModelProfile>,
    table: &RoutingTable,
    input: &RouteInput<'_>,
    base_model: &str,
    base_rung: usize,
    band: Band,
    hit_pct: u64,
    now_ms: u64,
) -> Turn {
    let lane = input.lane();
    let spend = &input.spend;
    let fit = Fit {
        needs_image: input.needs_image,
        needs_tools: input.needs_tools,
        ctx_tokens: input.ctx_tokens,
        grok_build: false,
    };
    let theirs = |m: &str| policy::is_new_provider(reg, m);
    let usable = |m: &str| {
        !local::is_local(m)
            && policy::fits(reg, profiles, m, fit, spend, now_ms)
            && reg.get(m).is_some_and(|r| r.state != ModelState::Degraded)
            && fast_base(reg, m).is_none()
            && !super::is_multi_agent(m)
            && super::in_auto_pool(reg, m, spend.settings.ceiling_usd_per_m)
            && if theirs(m) {
                reg.get(m)
                    .is_some_and(|r| providers::takes_reasoning(m, &r.meta))
            } else {
                rtable::is_reasoning_model(reg, profiles, m)
                    && policy::route_class(reg, m, false, spend) == CostClass::Included
            }
    };
    let prices = |m: &str| reg.get(m).map(|r| r.meta.prices.clone());
    let mut models = vec![base_model.to_string()];
    if !theirs(base_model) {
        let mut xai: Vec<&str> = reg
            .models
            .keys()
            .map(String::as_str)
            .filter(|m| *m != base_model && !theirs(m) && usable(m))
            .collect();
        // The strongest and the cheapest of the rest.
        xai.sort_by_key(|m| (Reverse(attention::reasoning_of(reg, m)), *m));
        let cost = |m: &str| {
            prices(m)
                .and_then(|p| est_cost_usd(&p, lane, input.ctx_tokens, base_rung, 0))
                .unwrap_or(f64::MAX)
        };
        let cheapest = xai
            .iter()
            .copied()
            .min_by(|a, b| cost(a).total_cmp(&cost(b)));
        for m in xai.first().copied().into_iter().chain(cheapest) {
            if models.len() < MAX_MODELS && !models.iter().any(|x| x == m) {
                models.push(m.to_string());
            }
        }
        // One provider model a lane: the first the routing table ranks for it.
        if let Some(m) = table
            .rows(lane)
            .iter()
            .map(|r| r.model.as_str())
            .find(|m| theirs(m) && usable(m))
        {
            models.push(m.to_string());
        }
    }
    if let Some(turns) = input.pin_turns {
        // D12: a long chat changes model only when the saving beats the re-send.
        let old = prices(base_model);
        models.retain(|m| {
            m == base_model
                || old
                    .as_ref()
                    .zip(prices(m))
                    .and_then(|(o, n)| policy::pin_switch(o, &n, lane, input.ctx_tokens, turns))
                    .is_some_and(|s| s.pays())
        });
    }
    let strongest = models
        .iter()
        .map(|m| attention::reasoning_of(reg, m))
        .max()
        .unwrap_or(0);
    let mut out = Vec::new();
    let mut base = 0;
    for m in &models {
        let strength = attention::reasoning_of(reg, m);
        let top = if strength < strongest {
            band.ceiling.min(TOP_BELOW_STRONGEST)
        } else {
            band.ceiling
        };
        let mut rungs: Vec<usize> = rungs_of(reg, profiles, m)
            .into_iter()
            .filter(|r| (band.floor..=top).contains(r))
            .filter(|r| !matches!(EFFORT_LADDER[*r], "minimal" | "max"))
            .collect();
        if m == base_model {
            rungs.push(base_rung);
        }
        rungs.sort_unstable();
        rungs.dedup();
        let hit = if m == base_model { hit_pct } else { 0 };
        for r in rungs {
            let cost = prices(m).and_then(|p| est_cost_usd(&p, lane, input.ctx_tokens, r, hit));
            let is_base = m == base_model && r == base_rung;
            let Some(cost_usd) = cost.or(is_base.then_some(0.0)) else {
                continue;
            };
            if is_base {
                base = out.len();
            }
            out.push(ArmSpec {
                model: m.clone(),
                rung: r,
                strength,
                provider: theirs(m),
                cost_usd,
            });
        }
    }
    Turn {
        arms: out,
        base,
        guard: None,
    }
}

/// The route [`Router::choose`] serves for `arm`, or `None` when the filters
/// won't serve that exact model and effort.
#[allow(clippy::too_many_arguments)]
fn serve(
    arm: &ArmSpec,
    explore: bool,
    base: &Route,
    input: &RouteInput<'_>,
    reg: &Registry,
    profiles: &BTreeMap<String, ModelProfile>,
    table: &RoutingTable,
    now_ms: u64,
) -> Option<Route> {
    let mut pick = input.pick.clone()?;
    pick.moved = match arm.rung.cmp(&pick.rung) {
        std::cmp::Ordering::Greater => Move::Up,
        std::cmp::Ordering::Less => Move::Down,
        std::cmp::Ordering::Equal => pick.moved,
    };
    pick.rung = arm.rung;
    pick.rules
        .push(if explore { EXPLORE_RULE } else { LEARNED_RULE }.into());
    let mut alt = input.clone();
    alt.pick = Some(pick);
    let base_model = base.base.as_deref().unwrap_or(&base.model);
    if arm.model != base_model {
        alt.episode_model = Some(arm.model.as_str());
        alt.pin_turns = None;
    }
    let r = Router::choose(&alt, reg, profiles, table, now_ms);
    let got = r.base.as_deref().unwrap_or(&r.model);
    (!r.no_route
        && got == arm.model
        && r.effort.as_deref() == Some(EFFORT_LADDER[arm.rung])
        && matches!(r.cost_class, CostClass::Included | CostClass::NewProvider))
    .then_some(r)
}

// ---------------------------------------------------------------- live turns

/// This crate's own tests route with made-up models: only a test that pinned
/// a scratch config folder learns.
fn live_on() -> bool {
    !cfg!(test) || crate::perm::config_pinned()
}

/// A typed chat turn waiting on its outcome.
#[derive(Debug, Clone, Default)]
struct Pending {
    turn: u64,
    ctx: String,
    arm: String,
    base: String,
    p: f64,
    explore: bool,
    guard: Option<&'static str>,
    hold: bool,
    est_cost: f64,
    ticks: i64,
    calls: u32,
    ok: bool,
    cancelled: bool,
    trouble: bool,
    copied: bool,
    shadow: BTreeMap<String, String>,
    /// Not the learner's to log (a canary, an 18+ or private turn, a refused arm).
    skip: bool,
}

static PENDING: Mutex<BTreeMap<String, Pending>> = Mutex::new(BTreeMap::new());
const PENDING_CAP: usize = 256;

fn pending<T>(f: impl FnOnce(&mut BTreeMap<String, Pending>) -> T) -> T {
    f(&mut PENDING.lock().unwrap_or_else(|e| e.into_inner()))
}

/// The settled line for `p`, or `None` when the turn teaches nothing (its last
/// call errored or was cancelled, or no call finished).
fn settle(p: &Pending, redo: bool, now_ms: u64) -> Option<Line> {
    if p.skip || p.calls == 0 || !p.ok || p.cancelled {
        return None;
    }
    let q = quality(redo, p.trouble, p.copied);
    let cost = if p.ticks > 0 {
        p.ticks as f64 / 1e10
    } else {
        p.est_cost
    };
    Some(Line {
        at: now_ms,
        ctx: p.ctx.clone(),
        arm: p.arm.clone(),
        base: p.base.clone(),
        p: p.p,
        explore: p.explore,
        guard: p.guard.map(str::to_string),
        hold: p.hold,
        q,
        cost,
        r: reward(q, cost),
        shadow: p.shadow.clone(),
    })
}

/// C10, every routed call of a typed chat turn: `trouble` marks the waiting
/// turn (a tool error read at a new turn is the last turn's), and a new turn
/// settles the one before it (`redo`: this turn says the last was wrong).
pub fn on_call(config_dir: &Path, key: &str, turn: u64, trouble: bool, redo: bool, now_ms: u64) {
    if !live_on() || key.is_empty() {
        return;
    }
    let prev = pending(|map| {
        if trouble && let Some(p) = map.get_mut(key) {
            p.trouble = true;
        }
        match map.get(key) {
            Some(p) if p.turn != turn => map.remove(key),
            _ => None,
        }
    });
    let Some(line) = prev.and_then(|p| settle(&p, redo, now_ms)) else {
        return;
    };
    let _ = append_line(&log_path(config_dir), &line);
    if !line.hold {
        with_state(config_dir, |s| {
            s.learn(&line.ctx, &line.arm, line.r);
            save_state(config_dir, s);
        });
    }
}

/// One turn the learner may steer.
#[derive(Debug, Clone)]
pub struct Steer<'a> {
    pub config_dir: &'a Path,
    pub key: &'a str,
    pub turn: u64,
    /// The ladder's class key for the chat.
    pub own: &'a str,
    pub ctx: String,
    pub band: Band,
    pub guard: Option<&'static str>,
    /// The R1 holdout runs this turn at its fixed rung: logged, never steered.
    pub hold: bool,
}

/// C9 on a typed chat turn's first call: maybe move `route` to a learned or
/// exploring arm, and log the choice with its propensity for when it settles.
pub fn steer(
    s: &Steer<'_>,
    route: Route,
    input: &RouteInput<'_>,
    reg: &Registry,
    profiles: &BTreeMap<String, ModelProfile>,
    table: &RoutingTable,
    now_ms: u64,
) -> Route {
    if !live_on()
        || s.key.is_empty()
        || pending(|map| map.get(s.key).is_some_and(|p| p.turn == s.turn))
    {
        // Later steps of a turn keep what its first call picked.
        return route;
    }
    let model = route.base.clone().unwrap_or_else(|| route.model.clone());
    let mut wait = Pending {
        turn: s.turn,
        ctx: s.ctx.clone(),
        arm: arm_id(&model, route.effort.as_deref()),
        p: 1.0,
        guard: s.guard,
        hold: s.hold,
        ..Pending::default()
    };
    wait.base = wait.arm.clone();
    let Some(base_rung) = route.effort.as_deref().and_then(rung).filter(|_| !s.hold) else {
        put(s.key, wait);
        return route;
    };
    let (ctx, hit) = with_state(s.config_dir, |st| {
        (
            st.contexts.get(&s.ctx).cloned().unwrap_or_default(),
            st.cache_hit_pct,
        )
    });
    let hit = hit.map_or(policy::predicted_tokens(input.lane()).2, |h| {
        h.round().clamp(0.0, 100.0) as u64
    });
    let mut turn = arms(
        reg, profiles, table, input, &model, base_rung, s.band, hit, now_ms,
    );
    turn.guard = s.guard;
    wait.est_cost = turn.arms.get(turn.base).map_or(0.0, |a| a.cost_usd);
    if turn.arms.len() < 2 || s.guard == Some(ATTENTION_GUARD) {
        put(s.key, wait);
        return route;
    }
    let mut rng = Rng::new(turn_hash(&format!("{}\u{1f}{}", s.key, s.turn)));
    let c = choose(&turn, &ctx, &mut rng);
    let arm = &turn.arms[c.arm];
    let mut route = route;
    if c.arm != turn.base {
        let Some(r) = serve(arm, c.explore, &route, input, reg, profiles, table, now_ms) else {
            // The filters won't serve it: the read's route stands, unlogged.
            skip(s.key, s.turn);
            return route;
        };
        route = r;
        ladder::set_rung(s.key, s.own, arm.rung);
        if c.explore {
            super::knn::forget(s.key);
        } else {
            super::knn::note_start(s.key, s.turn, arm.rung);
        }
    }
    wait.arm = arm.id();
    wait.base = turn.arms[turn.base].id();
    wait.p = c.p;
    wait.explore = c.explore;
    wait.est_cost = arm.cost_usd;
    put(s.key, wait);
    route
}

fn put(key: &str, p: Pending) {
    pending(|map| {
        if map.len() >= PENDING_CAP && !map.contains_key(key) {
            map.clear();
        }
        map.insert(key.to_string(), p);
    });
}

/// How a call of `key`'s turn ended, and what it cost (xAI ticks, 1e-10 USD).
pub fn note_call(key: &str, ok: bool, cancelled: bool, ticks: i64) {
    pending(|map| {
        if let Some(p) = map.get_mut(key) {
            p.calls += 1;
            p.ok = ok;
            p.cancelled |= cancelled;
            p.ticks = p.ticks.saturating_add(ticks.max(0));
        }
    });
}

/// C10: you copied a code block from `key`'s latest answer.
pub fn note_copy(key: &str) {
    pending(|map| {
        if let Some(p) = map.get_mut(key) {
            p.copied = true;
        }
    });
}

/// C11: shadow candidate `cand` would have picked `arm` on `key`'s turn.
pub fn note_shadow(key: &str, turn: u64, cand: &str, arm: String) {
    pending(|map| {
        if let Some(p) = map.get_mut(key).filter(|p| p.turn == turn) {
            p.shadow.insert(cand.to_string(), arm);
        }
    });
}

/// `key`'s turn `turn` is not the learner's: nothing about it is logged or
/// learned, and its later steps are left alone.
pub fn skip(key: &str, turn: u64) {
    if !live_on() || key.is_empty() {
        return;
    }
    put(
        key,
        Pending {
            turn,
            skip: true,
            ..Pending::default()
        },
    );
}

#[cfg(test)]
#[path = "bandit_tests.rs"]
mod tests;
