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
