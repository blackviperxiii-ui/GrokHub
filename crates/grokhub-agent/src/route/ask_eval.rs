//! Offline routing eval for the ask reader ([`super::ask`]).
//!
//! A hand-labeled set of chat turns ([`super::ask_set`]), each with the kind a careful engineer
//! would file it under and the effort range that serves it (lowest rung that
//! does the job, highest that is still worth paying for). The test reports:
//!
//! - kind accuracy (exact kind, and "same lane" which is what routing uses);
//! - effort in range;
//! - under-effort rate (quality risk; weighted heavier) and over-effort rate
//!   (wasted tokens);
//! - expected reasoning spend against always sending High.
//!
//! The same sets run again with the nearest-neighbour blend ([`super::knn`])
//! on, over a deterministic fake embedder (no real model is on disk here).
//!
//! Run with `cargo test -p grokhub-agent --lib route::ask_eval -- --nocapture`
//! to see the table and every miss.

use super::ask::{read, AskContext, AskKind, STAKES_LANE};
use super::ask_set::{HELD_OUT, SET, STAKES_HELD_OUT, STAKES_SET};

fn rung(e: &str) -> usize {
    grokhub_core::model_registry::EFFORT_LADDER
        .iter()
        .position(|r| *r == e)
        .expect("effort on the ladder")
}

/// Rough reasoning tokens per rung, for the spend comparison only.
fn reasoning_cost(r: usize) -> f64 {
    [0.0, 100.0, 400.0, 1_200.0, 3_500.0, 8_000.0, 16_000.0][r.min(6)]
}

pub(crate) struct Score {
    pub n: usize,
    pub kind_exact: usize,
    pub lane_same: usize,
    pub effort_ok: usize,
    pub under: usize,
    pub over: usize,
    pub spend: f64,
    pub spend_high: f64,
    pub misses: Vec<String>,
}

/// Score `set` (each turn on its kind's lane) and `stakes` (each on [`STAKES_LANE`]).
pub(crate) fn score_set(
    set: &[(&str, AskKind, &str, &str)],
    stakes: &[(&str, AskKind, &str, &str)],
) -> Score {
    score_by(set, stakes, |_, text| read(text, &AskContext::default()))
}

/// [`score_set`] with `read_row(row index, text)` doing the reading.
pub(crate) fn score_by(
    set: &[(&str, AskKind, &str, &str)],
    stakes: &[(&str, AskKind, &str, &str)],
    mut read_row: impl FnMut(usize, &str) -> super::ask::AskRead,
) -> Score {
    let rows: Vec<(&str, AskKind, &str, &str, &str)> = set
        .iter()
        .map(|(t, k, lo, hi)| (*t, *k, k.lane(), *lo, *hi))
        .chain(
            stakes
                .iter()
                .map(|(t, k, lo, hi)| (*t, *k, STAKES_LANE, *lo, *hi)),
        )
        .collect();
    let mut s = Score {
        n: rows.len(),
        kind_exact: 0,
        lane_same: 0,
        effort_ok: 0,
        under: 0,
        over: 0,
        spend: 0.0,
        spend_high: 0.0,
        misses: Vec::new(),
    };
    for (i, (text, kind, lane, lo, hi)) in rows.iter().enumerate() {
        let r = read_row(i, text);
        let (lo, hi) = (rung(lo), rung(hi));
        if r.kind == *kind {
            s.kind_exact += 1;
        }
        if r.lane() == *lane {
            s.lane_same += 1;
        }
        if r.effort < lo {
            s.under += 1;
        } else if r.effort > hi {
            s.over += 1;
        } else {
            s.effort_ok += 1;
        }
        s.spend += reasoning_cost(r.effort);
        s.spend_high += reasoning_cost(rung("high"));
        if r.kind != *kind || r.lane() != *lane || r.effort < lo || r.effort > hi {
            let short: String = text.chars().take(60).collect();
            s.misses.push(format!(
                "{short:?}: got {} {} {} (d {:.2}), want {} {} {}..{}",
                r.kind.id(),
                r.lane(),
                r.effort_name(),
                r.d,
                kind.id(),
                lane,
                grokhub_core::model_registry::EFFORT_LADDER[lo],
                grokhub_core::model_registry::EFFORT_LADDER[hi],
            ));
        }
    }
    s
}

fn report(name: &str, s: &Score) {
    let pct = |x: usize| 100.0 * x as f64 / s.n as f64;
    println!(
        "ask eval {name}: {} turns · kind {:.1}% · lane {:.1}% · effort in range {:.1}% · under {:.1}% · over {:.1}% · reasoning spend {:.0}% of always-High",
        s.n,
        pct(s.kind_exact),
        pct(s.lane_same),
        pct(s.effort_ok),
        pct(s.under),
        pct(s.over),
        100.0 * s.spend / s.spend_high,
    );
    for m in &s.misses {
        println!("  miss {m}");
    }
}

#[test]
fn ask_reader_routes_the_labeled_set() {
    let s = score_set(SET, STAKES_SET);
    report("tuning", &s);
    let pct = |x: usize| 100.0 * x as f64 / s.n as f64;
    assert!(s.n >= 80, "set too small: {}", s.n);
    assert!(
        pct(s.lane_same) >= 90.0,
        "lane accuracy {:.1}%",
        pct(s.lane_same)
    );
    assert!(
        pct(s.effort_ok) >= 90.0,
        "effort in range {:.1}%",
        pct(s.effort_ok)
    );
    // Under-thinking costs quality; keep it rare.
    assert!(pct(s.under) <= 5.0, "under-effort {:.1}%", pct(s.under));
    // And the whole point: far less reasoning than always-High.
    assert!(s.spend < 0.6 * s.spend_high, "spend {:.0}", s.spend);
}

#[test]
fn ask_reader_holds_up_on_turns_it_was_not_tuned_on() {
    let s = score_set(HELD_OUT, STAKES_HELD_OUT);
    report("held-out", &s);
    let pct = |x: usize| 100.0 * x as f64 / s.n as f64;
    assert!(s.n >= 40, "held-out too small: {}", s.n);
    assert!(
        pct(s.lane_same) >= 80.0,
        "lane accuracy {:.1}%",
        pct(s.lane_same)
    );
    assert!(pct(s.under) <= 10.0, "under-effort {:.1}%", pct(s.under));
}

#[test]
fn every_kind_has_labeled_turns() {
    for k in AskKind::ALL {
        let n = SET.iter().filter(|(_, kind, _, _)| *kind == k).count();
        assert!(n >= 4, "{} has {n} labeled turns", k.id());
    }
}

// ---------------------------------------------------------------- with the kNN blend

/// The labeled rows of `set` then `stakes` as bank entries, in scoring order.
fn rows_as_entries(
    model: &grokhub_core::static_embed::StaticModel,
    set: &[(&str, AskKind, &str, &str)],
    stakes: &[(&str, AskKind, &str, &str)],
) -> Vec<Option<super::knn::Entry>> {
    set.iter()
        .map(|r| super::knn::seed_entry(model, r, false))
        .chain(
            stakes
                .iter()
                .map(|r| super::knn::seed_entry(model, r, true)),
        )
        .collect()
}

/// Score with the blend on: each row reads against `bank` minus its own entry
/// (`own[i]` is row `i`'s index in `bank`, if it is there). Returns the score
/// and how often each `knn:` rule fired.
fn score_with_knn(
    set: &[(&str, AskKind, &str, &str)],
    stakes: &[(&str, AskKind, &str, &str)],
    bank: &[Option<super::knn::Entry>],
    own: impl Fn(usize) -> Option<usize>,
) -> (Score, std::collections::BTreeMap<String, usize>) {
    let model = crate::memory_embed::embedder().expect("the test model is set");
    let mut fired = std::collections::BTreeMap::new();
    let s = score_by(set, stakes, |i, text| {
        let cx = AskContext::default();
        let r = read(text, &cx);
        let Some(v) = model.encode(super::ask::own_words(text)) else {
            return r;
        };
        let skip = own(i);
        let others = bank
            .iter()
            .enumerate()
            .filter(|(j, _)| Some(*j) != skip)
            .filter_map(|(_, e)| e.as_ref());
        let r = super::knn::blend(r, &v, others, &cx);
        for rule in r.rules.iter().filter(|x| x.starts_with("knn:")) {
            *fired.entry(rule.clone()).or_insert(0) += 1;
        }
        r
    });
    (s, fired)
}

fn assert_whole(name: &str, s: &Score) {
    let pct = |x: usize| 100.0 * x as f64 / s.n as f64;
    assert_eq!(pct(s.lane_same), 100.0, "{name} lane");
    assert_eq!(pct(s.effort_ok), 100.0, "{name} effort in range");
    assert_eq!(s.under, 0, "{name} under-effort");
}

/// Tuning set, leave one out: every row reads against both labeled sets
/// without itself. Lane, effort range and under-effort stay where the rules
/// alone put them.
#[test]
fn knn_blend_keeps_the_tuning_set_whole() {
    crate::memory_embed::set_test_model(Some(super::knn_tests::toy_model()));
    let model = crate::memory_embed::embedder().unwrap();
    let mut bank = rows_as_entries(&model, SET, STAKES_SET);
    let n = bank.len();
    bank.extend(rows_as_entries(&model, HELD_OUT, STAKES_HELD_OUT));
    let (s, fired) = score_with_knn(SET, STAKES_SET, &bank, |i| (i < n).then_some(i));
    crate::memory_embed::set_test_model(None);
    report("tuning+knn", &s);
    println!("  knn rules: {fired:?}");
    assert_eq!(s.n, 100);
    assert_whole("tuning+knn", &s);
    let fired_total: usize = fired.values().sum();
    assert_eq!(
        fired_total, 100,
        "every row with an embedding logs one knn verdict"
    );
}

/// Held out, honestly: the bank holds the tuning rows only.
#[test]
fn knn_blend_keeps_the_held_out_set_whole() {
    crate::memory_embed::set_test_model(Some(super::knn_tests::toy_model()));
    let model = crate::memory_embed::embedder().unwrap();
    let bank = rows_as_entries(&model, SET, STAKES_SET);
    let (s, fired) = score_with_knn(HELD_OUT, STAKES_HELD_OUT, &bank, |_| None);
    crate::memory_embed::set_test_model(None);
    report("held-out+knn", &s);
    println!("  knn rules: {fired:?}");
    assert_eq!(s.n, 48);
    assert_whole("held-out+knn", &s);
}

// ---------------------------------------------------------------- M37: the learner

use super::bandit::{self, choose, ArmSpec, Ctx, Line, Rng, Turn};

fn arm(model: &str, effort: &str, strength: u8, cost_usd: f64) -> ArmSpec {
    ArmSpec {
        model: model.into(),
        rung: rung(effort),
        strength,
        provider: false,
        cost_usd,
    }
}

/// A cheaper rung below the read, the read's own arm, and a stronger model above it.
fn three(guard: Option<&'static str>) -> Turn {
    Turn {
        arms: vec![
            arm("grok-4.3", "low", 1, 0.001),
            arm("grok-4.3", "medium", 1, 0.002),
            arm("grok-4.7", "high", 3, 0.020),
        ],
        base: 1,
        guard,
    }
}

/// `settled` turns of history, `n` rewards of `r` for each arm given.
fn history(settled: u32, arms: &[(&Turn, usize, u32, f64)]) -> Ctx {
    let mut c = Ctx {
        settled,
        ..Ctx::default()
    };
    for (t, i, n, r) in arms {
        let s = c.arms.entry(t.arms[*i].id()).or_default();
        (0..*n).for_each(|_| s.add(*r));
    }
    c
}

#[test]
fn the_learner_never_explores_down_on_a_hard_or_stakes_turn() {
    // The cheaper rung has done far better than the read's arm.
    let low = three(None);
    let ctx = history(200, &[(&low, 0, 60, 0.95), (&low, 1, 60, 0.40)]);
    for guard in ["stakes", "hard", "agentic", "flagship", "redo", "climb"] {
        let t = three(Some(guard));
        let mut rng = Rng::new(11);
        for _ in 0..2_000 {
            let c = choose(&t, &ctx, &mut rng);
            assert!(!c.explore, "{guard}");
            assert!(t.up(c.arm), "{guard}: picked {}", t.arms[c.arm].id());
        }
    }
    // A low-stakes turn may use what it learned, but explores only upward.
    let mut rng = Rng::new(11);
    let (mut learned_down, mut explored) = (0, 0);
    for _ in 0..2_000 {
        let c = choose(&low, &ctx, &mut rng);
        if c.explore {
            explored += 1;
            assert!(low.up(c.arm) && c.arm != low.base);
        } else if c.arm == 0 {
            learned_down += 1;
        }
    }
    assert_eq!((learned_down, explored), (1_942, 58));
}

#[test]
fn the_learner_explores_at_its_rate_on_low_stakes_turns_with_a_fixed_seed() {
    assert_eq!(
        (
            bandit::EXPLORE_RATE,
            bandit::EXPLORE_AFTER,
            bandit::MIN_EVIDENCE
        ),
        (0.03, 20, 20)
    );
    let t = three(None);
    let count = |settled: u32| {
        let ctx = history(settled, &[]);
        let mut rng = Rng::new(42);
        (0..10_000)
            .map(|_| choose(&t, &ctx, &mut rng))
            .filter(|c| c.explore)
            .count()
    };
    assert_eq!(count(20), 299);
    // A context with too little history only follows the read.
    assert_eq!(count(19), 0);
    // With no evidence anywhere, an exploit is the read's own arm, p = 0.97.
    let mut rng = Rng::new(42);
    let c = (0..100)
        .map(|_| choose(&t, &history(20, &[]), &mut rng))
        .find(|c| !c.explore)
        .unwrap();
    assert_eq!((c.arm, c.p), (1, 0.97));
}

/// Run `turns` turns where arm i earns `quality[i]` less its cost; the picks of the last 500.
fn converge(quality: [f64; 3], turns: usize) -> [usize; 3] {
    let t = three(None);
    let mut ctx = history(bandit::EXPLORE_AFTER, &[]);
    let mut rng = Rng::new(5);
    let mut last = [0; 3];
    for i in 0..turns {
        let c = choose(&t, &ctx, &mut rng);
        let a = &t.arms[c.arm];
        let r = bandit::reward(quality[c.arm], a.cost_usd);
        ctx.settled += 1;
        ctx.arms.entry(a.id()).or_default().add(r);
        if i + 500 >= turns {
            last[c.arm] += 1;
        }
    }
    last
}

#[test]
fn the_learner_settles_on_the_cheaper_arm_when_it_is_as_good() {
    // As good: the read's arm keeps the turns; the pricier model is only explored.
    assert_eq!(converge([0.8, 0.8, 0.8], 4_000), [0, 488, 12]);
    // Clearly better: the stronger model takes over once it has the evidence.
    assert_eq!(converge([0.8, 0.5, 1.0], 4_000), [0, 0, 500]);
}

fn line(ctx: &str, arm: &str, p: f64, r: f64, shadow: &[(&str, &str)]) -> Line {
    Line {
        ctx: ctx.into(),
        arm: arm.into(),
        base: arm.into(),
        p,
        r,
        shadow: shadow
            .iter()
            .map(|(c, a)| (c.to_string(), a.to_string()))
            .collect(),
        ..Line::default()
    }
}

#[test]
fn ips_and_dr_on_a_small_hand_built_log() {
    let mut held = line("A", "a", 1.0, 5.0, &[("c", "a")]);
    held.hold = true;
    let log = vec![
        line("A", "a", 0.5, 1.0, &[("c", "a")]),
        line("A", "b", 0.5, 0.0, &[("c", "a")]),
        line("B", "a", 0.25, 0.5, &[("c", "b")]),
        line("B", "b", 1.0, 0.4, &[]),
        held,
    ];
    let o = bandit::ope(&log, "c");
    assert_eq!((o.n, o.matched), (3, 1));
    // Live earned (1.0 + 0.0 + 0.5) / 3; IPS = (1.0 / 0.5) / 3; DR = (1.0 + 1.0 + 0.4) / 3.
    assert!((o.live - 0.5).abs() < 1e-12, "{o:?}");
    assert!((o.ips - 2.0 / 3.0).abs() < 1e-12, "{o:?}");
    assert!((o.dr - 0.8).abs() < 1e-12, "{o:?}");
    assert_eq!(bandit::ope(&log, "nobody"), bandit::Ope::default());
}

#[test]
fn shadow_mode_promotes_a_candidate_the_log_scores_no_worse_and_rejects_a_worse_one() {
    use super::tune::{judge, Candidate, Change, Evidence, Stage, Verdict, DAY_MS};
    // Live splits its turns between a good arm and a poor one.
    let log: Vec<Line> = (0..40)
        .map(|i| {
            let (arm, r) = if i % 2 == 0 { ("a", 0.9) } else { ("b", 0.3) };
            line("A", arm, 0.5, r, &[("good", "a"), ("poor", "b")])
        })
        .collect();
    let c = |id: &str| Candidate {
        id: id.into(),
        class: "chat:default".into(),
        change: Some(Change::Start {
            effort: "low".into(),
        }),
        stage: Stage::Shadow,
        created_at: 100 * DAY_MS,
        ..Candidate::default()
    };
    let ev = |id: &str, lines: &[Line]| Evidence {
        shadow_steps: 200,
        ope: bandit::ope(lines, id),
        ..Evidence::default()
    };
    let now = 101 * DAY_MS;
    let good = ev("good", &log);
    assert_eq!(good.ope.matched, 20);
    assert!((good.ope.live - 0.6).abs() < 1e-12);
    assert!((good.ope.dr - 0.9).abs() < 1e-12);
    assert_eq!(judge(&c("good"), &good, 0, now), Verdict::StartCanary);
    assert_eq!(
        judge(&c("poor"), &ev("poor", &log), 0, now),
        Verdict::Reject {
            why: "its logged score fell short in shadow (0.30 vs 0.60)".into(),
            rollback: false
        }
    );
    // Half the log is too few matched turns to judge yet.
    assert_eq!(
        judge(&c("good"), &ev("good", &log[..30]), 0, now),
        Verdict::Wait
    );
}
