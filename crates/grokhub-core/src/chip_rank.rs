//! The chip ranker: which few chips a row shows, learned from what the person
//! picks and hides, with no model call and no setting.
//!
//! `build_quick_chips` gathers the candidate pool. This module turns it into
//! a row:
//!
//! 1. **Fatigue**: a chip shown five times in three days and never picked
//!    rests for three days.
//! 2. **Score**: a linear model over a dozen cached features, learned online
//!    (what the person did with each chip before comes from the learning
//!    engine's belief about that chip, so the row and the "What I've
//!    learned" page read the same numbers)
//!    as a diagonal Bayesian logistic regression (Chapelle and Li, "An
//!    Empirical Evaluation of Thompson Sampling", 2011). Each row draws its
//!    weights from the posterior (Thompson sampling), so a chip that might be
//!    better gets tried now and then. The spread grows with evidence as
//!    n/(n+50): with no history the draw is the prior mean, which ranks
//!    exactly by the engine's own score, as before.
//!    A chip the person marked wrong is never shown.
//! 3. **Diversify**: maximal marginal relevance (λ 0.7), one chip per intent.
//! 4. **Gate**: once the person has seen enough rows, a row whose best chip
//!    is unlikely to be picked is not shown at all.
//! 5. **Propensity**: how often each chip would land in its slot over many
//!    draws, logged with the row so offline evaluation can correct for it.
//! 6. **Holdout**: one boundary in twenty shows no row, logged, so
//!    `chip_health` can tell whether chips make the next step faster.
//!
//! The draw is seeded per boundary (a turn ending, a page opening), never per
//! keystroke, so a row does not reshuffle while the person types.

use serde::{Deserialize, Serialize};

use crate::chip_store::{chip_log_key, ChipOutcome};
use crate::chips::{chip_intent, chip_memory_key, ChipKind, ChipMemory, QuickChip};
use crate::learn::{Beliefs, Standing, Subject};

/// Chips a row shows at most, on every surface.
pub const CHIP_ROW_MAX: usize = 3;
/// Feature count. A stored model of another size is reset to the prior.
const DIMS: usize = 12;
/// Evidence at which the draw's spread reaches half its full width.
const SPREAD_HALF_AT: f32 = 50.0;
const MMR_LAMBDA: f32 = 0.7;
const FATIGUE_SHOWS: usize = 5;
const FATIGUE_WINDOW_MS: u64 = 3 * 24 * 60 * 60 * 1000;
/// Rows the person must have seen before the gate can hide one.
const GATE_MIN_SHOWS: u32 = 50;
/// Draws used to estimate each chip's propensity.
const PROPENSITY_DRAWS: u64 = 32;
/// Weight of the "this row was passed over" signal on its first chip.
const IGNORED_WEIGHT: f32 = 0.25;

/// What each feature means, in [`features`] order.
const FEATURE_NAMES: [&str; DIMS] = [
    "bias",
    "engine score",
    "picked before",
    "next-step odds",
    "turned down",
    "ask",
    "page",
    "command",
    "time of day",
    "predicted",
    "learned habit",
    "from the reply",
];

/// The learned model, kept in chip memory.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChipRanker {
    /// Posterior means.
    pub mean: Vec<f32>,
    /// Posterior precisions (1 / variance).
    pub precision: Vec<f32>,
    /// Labelled chips learned from.
    pub learned: u32,
    /// Rows shown and rows with a pick, for the gate.
    pub rows: u32,
    pub picks: u32,
    /// The last ranked rows, per surface: what the log writes when the row is
    /// shown. Rebuilt every boundary, never saved.
    #[serde(skip)]
    pub last: Vec<RankedChip>,
}

/// A ranked chip's features and propensity, waiting for its row to be shown.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct RankedChip {
    pub surface: String,
    pub key: String,
    pub p: f32,
    pub x: Vec<f32>,
}

impl Default for ChipRanker {
    fn default() -> Self {
        let mut mean = vec![0.0; DIMS];
        let mut precision = vec![1.0; DIMS];
        // Day one: the engine's own score decides (high weight, held firm),
        // everything else starts neutral and is learned.
        mean[0] = -2.0;
        mean[1] = 4.0;
        precision[1] = 25.0;
        Self {
            mean,
            precision,
            learned: 0,
            rows: 0,
            picks: 0,
            last: Vec::new(),
        }
    }
}

impl ChipRanker {
    fn fit(&mut self) {
        if self.mean.len() != DIMS || self.precision.len() != DIMS {
            let keep = (self.rows, self.picks);
            *self = Self::default();
            (self.rows, self.picks) = keep;
        }
    }

    /// How far a draw may stray from the mean: none with no evidence.
    fn spread(&self) -> f32 {
        let n = self.learned as f32;
        n / (n + SPREAD_HALF_AT)
    }

    /// One online Laplace step for a labelled chip.
    fn learn(&mut self, x: &[f32], picked: bool, weight: f32) {
        self.fit();
        if x.len() != DIMS {
            return;
        }
        let p = sigmoid(dot(&self.mean, x));
        let y = if picked { 1.0 } else { 0.0 };
        for ((mean, precision), &xi) in self.mean.iter_mut().zip(self.precision.iter_mut()).zip(x) {
            *precision += weight * xi * xi * p * (1.0 - p);
            *mean += weight * (y - p) * xi / *precision;
        }
        self.learned = self.learned.saturating_add(1);
    }

    /// The model's weights in plain words, strongest first (for diagnostics).
    pub fn weights(&self) -> Vec<(&'static str, f32)> {
        let mut out: Vec<_> = FEATURE_NAMES
            .iter()
            .copied()
            .zip(self.mean.iter().copied())
            .collect();
        out.sort_by(|a, b| b.1.abs().total_cmp(&a.1.abs()));
        out
    }
}

/// When and where a row is ranked.
#[derive(Debug, Clone, Copy)]
pub struct RankCtx<'a> {
    pub surface: &'a str,
    pub hour: u8,
    pub now_ms: u64,
    /// Same boundary, same seed: the row holds still while the person types.
    pub seed: u64,
    pub max: usize,
}

/// What the learning engine holds about one chip, read once per row.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct Heard {
    /// 0 to 1: how surely the person picks it.
    picked: f32,
    /// 0 to 1: how surely the person passes it over or hides it.
    turned_down: f32,
    /// The person marked it wrong: it is never shown.
    wrong: bool,
}

fn heard(beliefs: &Beliefs, chip: &QuickChip, now_ms: u64) -> Heard {
    let Some(belief) = Subject::new("chip", &chip_log_key(chip)).and_then(|s| beliefs.get(&s))
    else {
        return Heard::default();
    };
    let Some(rate) = belief.rate() else {
        return Heard {
            wrong: belief.standing == Standing::Wrong,
            ..Heard::default()
        };
    };
    let sure = belief.confidence(now_ms);
    Heard {
        picked: rate * sure,
        turned_down: (1.0 - rate) * sure,
        wrong: false,
    }
}

/// Rank `pool` into a row of at most `ctx.max` (and [`CHIP_ROW_MAX`]) chips.
/// Notes each chip's features and propensity for the log. What the person
/// did with each chip before is read from the learning engine.
pub fn rank_quick_chips(
    mem: &mut ChipMemory,
    pool: Vec<QuickChip>,
    ctx: RankCtx<'_>,
) -> Vec<QuickChip> {
    // Read and let go of the engine before ranking, which may log to it.
    let known = crate::learn::hub::read(|e| {
        pool.iter()
            .map(|c| heard(e.beliefs(), c, ctx.now_ms))
            .collect()
    })
    .unwrap_or_else(|| vec![Heard::default(); pool.len()]);
    rank(mem, pool, known, ctx)
}

/// [`rank_quick_chips`] over `beliefs` in place of the running engine's.
#[cfg(test)]
pub(crate) fn rank_quick_chips_from(
    beliefs: &Beliefs,
    mem: &mut ChipMemory,
    pool: Vec<QuickChip>,
    ctx: RankCtx<'_>,
) -> Vec<QuickChip> {
    let known = pool.iter().map(|c| heard(beliefs, c, ctx.now_ms)).collect();
    rank(mem, pool, known, ctx)
}

fn rank(
    mem: &mut ChipMemory,
    pool: Vec<QuickChip>,
    known: Vec<Heard>,
    ctx: RankCtx<'_>,
) -> Vec<QuickChip> {
    mem.ranker.fit();
    mem.ranker.last.retain(|r| r.surface != ctx.surface);
    let max = ctx.max.clamp(1, CHIP_ROW_MAX);
    let (pool, known): (Vec<QuickChip>, Vec<Heard>) = pool
        .into_iter()
        .zip(known)
        .filter(|(c, h)| !h.wrong && !fatigued(mem, c, ctx.now_ms))
        .unzip();
    if pool.is_empty() {
        return pool;
    }
    let top = pool.iter().map(|c| c.score).fold(f32::MIN, f32::max);
    let xs: Vec<Vec<f32>> = pool
        .iter()
        .zip(&known)
        .map(|(c, h)| features(mem, c, *h, top, ctx))
        .collect();
    let pick = |seed: u64| select(&mem.ranker, &pool, &xs, seed, max);
    let row = pick(ctx.seed);
    if gate_closed(&mem.ranker, &pool, &xs, &row) {
        return Vec::new();
    }
    if crate::chip_health::in_holdout(ctx.seed) && crate::chip_health::holdout_allowed(&pool) {
        crate::chip_health::log_holdout(mem, ctx.surface, ctx.now_ms);
        return Vec::new();
    }
    // Propensity: share of draws that put the same chip in the same slot.
    let mut hits = vec![0u32; row.len()];
    if mem.ranker.spread() > 0.0 {
        for d in 1..=PROPENSITY_DRAWS {
            let other = pick(ctx.seed ^ d.wrapping_mul(0x9e37_79b9_7f4a_7c15));
            for (slot, &i) in row.iter().enumerate() {
                if other.get(slot) == Some(&i) {
                    hits[slot] += 1;
                }
            }
        }
    }
    let mut out = Vec::with_capacity(row.len());
    for (slot, &i) in row.iter().enumerate() {
        let p = if mem.ranker.spread() > 0.0 {
            // The shown draw counts too, so p is never 0.
            (hits[slot] + 1) as f32 / (PROPENSITY_DRAWS + 1) as f32
        } else {
            1.0
        };
        mem.ranker.last.push(RankedChip {
            surface: ctx.surface.to_string(),
            key: chip_log_key(&pool[i]),
            p,
            x: xs[i].iter().map(|v| (v * 100.0).round() / 100.0).collect(),
        });
        let mut c = pool[i].clone();
        c.primary = slot == 0;
        out.push(c);
    }
    out
}

/// One draw: sampled scores, then MMR with one chip per intent.
fn select(
    ranker: &ChipRanker,
    pool: &[QuickChip],
    xs: &[Vec<f32>],
    seed: u64,
    max: usize,
) -> Vec<usize> {
    let mut rng = SplitMix(seed);
    let spread = ranker.spread();
    let w: Vec<f32> = (0..DIMS)
        .map(|i| {
            let sd = (spread / ranker.precision[i].max(1e-3)).sqrt();
            ranker.mean[i] + sd * rng.normal()
        })
        .collect();
    let scores: Vec<f32> = xs.iter().map(|x| dot(&w, x)).collect();
    let lo = scores.iter().copied().fold(f32::MAX, f32::min);
    let hi = scores.iter().copied().fold(f32::MIN, f32::max);
    let norm = |s: f32| if hi > lo { (s - lo) / (hi - lo) } else { 1.0 };
    let intents: Vec<Option<&'static str>> = pool
        .iter()
        .map(|c| chip_intent(&c.label, &c.value))
        .collect();
    let mut picked: Vec<usize> = Vec::new();
    while picked.len() < max {
        let mut best: Option<(usize, f32)> = None;
        for i in 0..pool.len() {
            if picked.contains(&i) {
                continue;
            }
            if intents[i].is_some() && picked.iter().any(|&j| intents[j] == intents[i]) {
                continue;
            }
            let sim = picked
                .iter()
                .map(|&j| label_overlap(&pool[i].label, &pool[j].label))
                .fold(0.0, f32::max);
            let mmr = MMR_LAMBDA * norm(scores[i]) - (1.0 - MMR_LAMBDA) * sim;
            // Strictly greater: equal chips keep the engine's order.
            if best.is_none_or(|(_, b)| mmr > b) {
                best = Some((i, mmr));
            }
        }
        match best {
            Some((i, _)) => picked.push(i),
            None => break,
        }
    }
    picked
}

/// Shared words between two labels (Jaccard), 0 to 1.
fn label_overlap(a: &str, b: &str) -> f32 {
    let words = |s: &str| -> Vec<String> {
        s.split_whitespace()
            .map(|w| {
                w.trim_matches(|c: char| !c.is_alphanumeric())
                    .to_lowercase()
            })
            .filter(|w| w.len() > 2)
            .collect()
    };
    let (a, b) = (words(a), words(b));
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    let shared = a.iter().filter(|w| b.contains(w)).count() as f32;
    let all = (a.len() + b.len()) as f32 - shared;
    shared / all
}

/// The gate: with enough rows behind it, hide a row whose best chip is
/// clearly less likely to be picked than this person's usual pick rate.
fn gate_closed(ranker: &ChipRanker, pool: &[QuickChip], xs: &[Vec<f32>], row: &[usize]) -> bool {
    if ranker.rows < GATE_MIN_SHOWS || row.is_empty() || pool.is_empty() {
        return false;
    }
    let rate = ranker.picks as f32 / ranker.rows.max(1) as f32;
    let bar = (rate * 0.5).clamp(0.01, 0.15);
    let best = row
        .iter()
        .map(|&i| sigmoid(dot(&ranker.mean, &xs[i])))
        .fold(0.0, f32::max);
    best < bar
}

/// Shown at least five times in three days and never picked.
fn fatigued(mem: &ChipMemory, chip: &QuickChip, now: u64) -> bool {
    let key = chip_log_key(chip);
    let mut shown = 0;
    for e in mem.events.iter().rev() {
        if now.saturating_sub(e.at) > FATIGUE_WINDOW_MS {
            break;
        }
        if matches!(&e.outcome, Some(ChipOutcome::Picked { key: k, .. }) if *k == key) {
            return false;
        }
        if e.shown.iter().any(|s| s.key == key) {
            shown += 1;
        }
    }
    shown >= FATIGUE_SHOWS
}

/// The chip's features, each about 0 to 1, in [`FEATURE_NAMES`] order.
fn features(
    mem: &ChipMemory,
    chip: &QuickChip,
    heard: Heard,
    top: f32,
    ctx: RankCtx<'_>,
) -> Vec<f32> {
    let key = chip_memory_key(chip);
    let hit = mem.hits.iter().find(|h| h.key == key);
    let engine = if top > 0.0 {
        (chip.score / top).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let next = mem
        .last_chip_key
        .as_ref()
        .and_then(|from| mem.transitions.get(from))
        .map_or(0.0, |dests| {
            let total: u32 = dests.values().sum();
            dests
                .get(&key)
                .map_or(0.0, |n| *n as f32 / total.max(1) as f32)
        });
    let at_hour = hit.map_or(0.0, |h| {
        let peak = h.hour_hits.iter().copied().max().unwrap_or(0);
        let now = h.hour_hits.get(usize::from(ctx.hour)).copied().unwrap_or(0);
        if peak > 0 {
            now as f32 / peak as f32
        } else {
            0.0
        }
    });
    let flag = |b: bool| if b { 1.0 } else { 0.0 };
    vec![
        1.0,
        engine,
        heard.picked,
        next,
        heard.turned_down,
        flag(chip.kind == ChipKind::Chat),
        flag(chip.kind == ChipKind::Nav),
        flag(chip.kind == ChipKind::Shell),
        at_hour,
        flag(chip.id.starts_with("pred-")),
        flag(chip.id.starts_with("learn-") || chip.id.starts_with("pred-habit")),
        flag(chip.id.starts_with(crate::chip_trailer::REPLY_CHIP_PREFIX)),
    ]
}

/// What the ranker noted for `key` on `surface` when it built the row.
pub(crate) fn ranked_note(mem: &ChipMemory, surface: &str, key: &str) -> Option<RankedChip> {
    mem.ranker
        .last
        .iter()
        .find(|r| r.surface == surface && r.key == key)
        .cloned()
}

/// Learn from a closed row: the picked chip is a yes, chips before it and a
/// hidden chip are a no. A row passed over teaches a weak no on its first chip.
pub(crate) fn learn_from_row(
    ranker: &mut ChipRanker,
    shown: &[crate::chip_store::ShownChip],
    outcome: Option<&ChipOutcome>,
) {
    ranker.rows = ranker.rows.saturating_add(1);
    match outcome {
        Some(ChipOutcome::Picked { slot, .. }) => {
            ranker.picks = ranker.picks.saturating_add(1);
            for s in shown {
                if s.slot < *slot {
                    ranker.learn(&s.x, false, 1.0);
                } else if s.slot == *slot {
                    ranker.learn(&s.x, true, 1.0);
                }
            }
        }
        Some(ChipOutcome::Dismissed { slot, .. }) => {
            if let Some(s) = shown.iter().find(|s| s.slot == *slot) {
                ranker.learn(&s.x, false, 1.0);
            }
        }
        None => {
            if let Some(s) = shown.iter().find(|s| s.slot == 0) {
                ranker.learn(&s.x, false, IGNORED_WEIGHT);
            }
        }
    }
}

fn dot(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

fn sigmoid(z: f32) -> f32 {
    1.0 / (1.0 + (-z).exp())
}

/// A small seeded generator (SplitMix64): the same boundary draws the same row.
struct SplitMix(u64);

impl SplitMix {
    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    fn unit(&mut self) -> f32 {
        ((self.next_u64() >> 40) as f32 + 0.5) / (1u64 << 24) as f32
    }

    /// Standard normal (Box-Muller).
    fn normal(&mut self) -> f32 {
        let (u, v) = (self.unit(), self.unit());
        (-2.0 * u.ln()).sqrt() * (std::f32::consts::TAU * v).cos()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chip_store::{log_chip_outcome, log_chips_shown};

    fn chip(id: &str, label: &str, score: f32) -> QuickChip {
        QuickChip {
            id: id.into(),
            label: label.into(),
            value: label.into(),
            kind: ChipKind::Chat,
            score,
            hint: String::new(),
            primary: false,
        }
    }

    fn ctx(seed: u64) -> RankCtx<'static> {
        RankCtx {
            surface: "composer",
            hour: 10,
            now_ms: 1_000_000,
            seed,
            max: CHIP_ROW_MAX,
        }
    }

    fn ids(chips: &[QuickChip]) -> Vec<&str> {
        chips.iter().map(|c| c.id.as_str()).collect()
    }

    #[test]
    fn with_no_history_the_engine_score_decides() {
        let mut mem = ChipMemory::default();
        let pool = vec![
            chip("a", "Brief me", 5.0),
            chip("b", "Plan the week", 9.0),
            chip("c", "Show my photos", 1.0),
            chip("d", "Tidy downloads", 7.0),
        ];
        for seed in [1, 2, 99] {
            let row = rank_quick_chips(&mut mem, pool.clone(), ctx(seed));
            assert_eq!(ids(&row), ["b", "d", "a"], "seed {seed}");
            assert!(row[0].primary && !row[1].primary && !row[2].primary);
        }
        assert!(mem.ranker.last.iter().all(|r| r.p == 1.0));
    }

    #[test]
    fn equal_scores_keep_the_engine_order() {
        let mut mem = ChipMemory::default();
        let pool = vec![
            chip("a", "Brief me", 1.0),
            chip("b", "Plan the week", 1.0),
            chip("c", "Show my photos", 1.0),
        ];
        let row = rank_quick_chips(&mut mem, pool, ctx(7));
        assert_eq!(ids(&row), ["a", "b", "c"]);
    }

    #[test]
    fn one_chip_per_intent() {
        let mut mem = ChipMemory::default();
        let pool = vec![
            chip("a", "Fix the build", 9.0),
            chip("b", "Fix the test", 8.0),
            chip("c", "Explain this", 7.0),
        ];
        let row = rank_quick_chips(&mut mem, pool, ctx(1));
        assert_eq!(ids(&row), ["a", "c"]);
    }

    #[test]
    fn a_chip_passed_over_five_times_rests() {
        let mut mem = ChipMemory::default();
        let stale = chip("a", "Brief me", 9.0);
        let other = chip("b", "Plan the week", 5.0);
        for i in 0..5 {
            log_chips_shown(
                &mut mem,
                "composer",
                std::slice::from_ref(&stale),
                999_000 + i,
            );
            log_chips_shown(&mut mem, "home", std::slice::from_ref(&stale), 999_000 + i);
        }
        let row = rank_quick_chips(&mut mem, vec![stale.clone(), other], ctx(1));
        assert_eq!(ids(&row), ["b"]);
        // Picked once in the window: it is wanted, it stays.
        log_chip_outcome(&mut mem, "home", &stale, true, 999_900);
        let row = rank_quick_chips(&mut mem, vec![stale, chip("b", "Plan", 5.0)], ctx(1));
        assert_eq!(ids(&row)[0], "a");
    }

    #[test]
    fn picks_teach_the_ranker() {
        let mut mem = ChipMemory::default();
        // A habit chip the engine scores a little lower keeps getting picked.
        let habit = chip("learn-brief", "Brief me", 8.0);
        let engine = chip("pred-plan", "Plan the week", 9.0);
        let mut t = 1_000_000;
        let mut shown = 0;
        // Forty rows that showed; a held-back boundary teaches nothing.
        while shown < 40 {
            t += 1;
            let row = rank_quick_chips(
                &mut mem,
                vec![engine.clone(), habit.clone()],
                RankCtx {
                    now_ms: t,
                    seed: t,
                    ..ctx(t)
                },
            );
            if row.is_empty() {
                assert!(crate::chip_health::in_holdout(t));
                continue;
            }
            shown += 1;
            log_chips_shown(&mut mem, "composer", &row, t);
            log_chip_outcome(&mut mem, "composer", &habit, true, t);
        }
        assert!(mem.ranker.learned >= 40);
        let row = rank_quick_chips(&mut mem, vec![engine, habit], ctx(12345));
        assert_eq!(row[0].id, "learn-brief", "{:?}", mem.ranker.weights());
    }

    fn about(c: &QuickChip) -> Subject {
        Subject::new("chip", &chip_log_key(c)).unwrap()
    }

    #[test]
    fn a_chips_history_is_what_the_learning_engine_holds() {
        let (liked, passed, fresh) = (
            chip("a", "Brief me", 5.0),
            chip("b", "Plan the week", 5.0),
            chip("c", "Show my photos", 5.0),
        );
        let mut beliefs = Beliefs::default();
        for _ in 0..20 {
            beliefs.vote(&about(&liked), true, 1_000_000, "signals");
            beliefs.vote(&about(&passed), false, 1_000_000, "signals");
        }
        let mut mem = ChipMemory::default();
        let pool = vec![liked.clone(), passed.clone(), fresh.clone()];
        let row = rank_quick_chips_from(&beliefs, &mut mem, pool.clone(), ctx(1));
        assert_eq!(ids(&row), ["a", "b", "c"]);
        // Picked before (slot 2) and turned down (slot 4), as logged.
        let x = |mem: &ChipMemory, i: usize| (mem.ranker.last[i].x[2], mem.ranker.last[i].x[4]);
        assert_eq!(x(&mem, 0), (0.88, 0.0));
        assert_eq!(x(&mem, 1), (0.0, 0.88));
        assert_eq!(x(&mem, 2), (0.0, 0.0));
        // "Always" is a sure yes; forgetting it leaves nothing.
        assert!(beliefs.pin(&about(&passed), 1_000_000));
        let _ = rank_quick_chips_from(&beliefs, &mut mem, pool.clone(), ctx(1));
        assert_eq!(x(&mem, 1), (1.0, 0.0));
        assert!(beliefs.forget(&about(&passed), 1_000_000));
        let _ = rank_quick_chips_from(&beliefs, &mut mem, pool, ctx(1));
        assert_eq!(x(&mem, 1), (0.0, 0.0));
    }

    #[test]
    fn a_chip_marked_wrong_is_never_shown() {
        let mut beliefs = Beliefs::default();
        let (top, other) = (chip("a", "Brief me", 9.0), chip("b", "Plan the week", 5.0));
        let mut mem = ChipMemory::default();
        let pool = vec![top.clone(), other];
        let row = rank_quick_chips_from(&beliefs, &mut mem, pool.clone(), ctx(1));
        assert_eq!(ids(&row), ["a", "b"]);
        beliefs.mark_wrong(&about(&top), 1_000_000);
        let row = rank_quick_chips_from(&beliefs, &mut mem, pool, ctx(1));
        assert_eq!(ids(&row), ["b"]);
        let only = rank_quick_chips_from(&beliefs, &mut mem, vec![top], ctx(1));
        assert_eq!(only, Vec::new());
    }

    #[test]
    fn the_engines_numbers_move_a_picked_chip_up() {
        use crate::chip_store::{outcome_signals, shown_signals};
        let mut mem = ChipMemory::default();
        let mut beliefs = Beliefs::default();
        // Two plain chips: the person always takes the one scored lower.
        let (first, taken) = (chip("a", "Brief me", 9.0), chip("b", "Plan the week", 8.0));
        let pool = vec![first, taken.clone()];
        let mut t = 1_000_000;
        let mut shown = 0;
        while shown < 40 {
            t += 1;
            let at = RankCtx {
                now_ms: t,
                seed: t,
                ..ctx(t)
            };
            let row = rank_quick_chips_from(&beliefs, &mut mem, pool.clone(), at);
            if row.is_empty() {
                assert!(crate::chip_health::in_holdout(t));
                continue;
            }
            shown += 1;
            let mut told = shown_signals(&mut mem, "composer", &row, t);
            told.extend(outcome_signals(&mut mem, "composer", &taken, true, t));
            for s in &told {
                beliefs.apply(s);
            }
        }
        // Four days on, so the chip that was passed over is done resting.
        let at = RankCtx {
            now_ms: t + FATIGUE_WINDOW_MS + 24 * 60 * 60 * 1000,
            ..ctx(12345)
        };
        let row = rank_quick_chips_from(&beliefs, &mut mem, pool.clone(), at);
        assert_eq!(ids(&row), ["b", "a"], "{:?}", mem.ranker.weights());
        // Same learned weights, nothing held about the chips ("Forget
        // everything"): the engine's own score decides again.
        mem.ranker.learned = 0;
        let row = rank_quick_chips_from(&Beliefs::default(), &mut mem, pool, at);
        assert_eq!(ids(&row), ["a", "b"], "{:?}", mem.ranker.weights());
    }

    #[test]
    fn propensity_is_logged_once_the_ranker_explores() {
        let mut mem = ChipMemory::default();
        mem.ranker.learned = 500;
        let pool = vec![
            chip("a", "Brief me", 5.0),
            chip("b", "Plan the week", 5.0),
            chip("c", "Show my photos", 5.0),
        ];
        let row = rank_quick_chips(&mut mem, pool, ctx(3));
        log_chips_shown(&mut mem, "composer", &row, 1);
        let shown = &mem.events[0].shown;
        assert_eq!(shown.len(), row.len());
        for s in shown {
            assert!(s.p > 0.0 && s.p <= 1.0, "{s:?}");
            assert_eq!(s.x.len(), DIMS);
        }
    }

    #[test]
    fn the_same_boundary_draws_the_same_row() {
        let mut mem = ChipMemory::default();
        mem.ranker.learned = 500;
        let pool: Vec<_> = [
            "Brief me",
            "Plan the week",
            "Show my photos",
            "Tidy downloads",
        ]
        .iter()
        .enumerate()
        .map(|(i, l)| chip(&format!("c{i}"), l, 5.0))
        .collect();
        let first = rank_quick_chips(&mut mem, pool.clone(), ctx(42));
        for _ in 0..5 {
            assert_eq!(rank_quick_chips(&mut mem, pool.clone(), ctx(42)), first);
        }
    }

    #[test]
    fn the_gate_waits_for_history_then_hides_hopeless_rows() {
        let mut mem = ChipMemory::default();
        let pool = vec![chip("a", "Brief me", 1.0)];
        mem.ranker.mean = vec![-9.0; DIMS];
        mem.ranker.rows = GATE_MIN_SHOWS - 1;
        mem.ranker.picks = 20;
        assert_eq!(rank_quick_chips(&mut mem, pool.clone(), ctx(1)).len(), 1);
        mem.ranker.rows = GATE_MIN_SHOWS;
        assert!(rank_quick_chips(&mut mem, pool, ctx(1)).is_empty());
    }

    #[test]
    fn an_old_model_shape_resets_to_the_prior() {
        let mut mem = ChipMemory::default();
        mem.ranker.mean = vec![1.0; 3];
        mem.ranker.rows = 9;
        let _ = rank_quick_chips(&mut mem, vec![chip("a", "Brief me", 1.0)], ctx(1));
        assert_eq!(mem.ranker.mean.len(), DIMS);
        assert_eq!(mem.ranker.rows, 9);
    }
}
