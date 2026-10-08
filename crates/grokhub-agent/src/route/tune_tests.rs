//! Self-tuning guardrails on a fake clock: the promotion thresholds, rollback,
//! the canary's reach, the candidate filters and the weekly line.

use super::*;
use crate::route::ladder::rung;
use crate::route::policy::class_row;

const NOW: u64 = 1_000 * DAY_MS;

fn snap(n: u32, pass_pct: f64, rework_pct: f64, cost: f64, p50: u64) -> Snap {
    Snap { n, pass_pct, rework_pct, cost_per_step: cost, p50_ms: p50, p95_ms: p50 * 2 }
}

fn canary(id: &str) -> Candidate {
    Candidate {
        id: id.into(),
        class: "chat:default".into(),
        change: Some(Change::Order { model: "grok-4.6".into() }),
        stage: Stage::Canary,
        created_at: NOW - 3 * DAY_MS,
        canary_at: Some(NOW - DAY_MS),
        ..Candidate::default()
    }
}

#[test]
fn the_threshold_constants_are_the_spec_values() {
    assert_eq!((SHADOW_MIN_STEPS, CANARY_MIN_N, ROLLBACK_MIN_N, MAX_PROMOTIONS_PER_WEEK, WATCH_DAYS), (200, 50, 30, 2, 7));
    assert_eq!((CANARY_SHARE, QUALITY_MAX_DROP_PTS, REWORK_MAX_RISE_PTS, MIN_GAIN_PCT), (0.05, 2.0, 2.0, 10.0));
}

#[test]
fn equal_quality_and_12_percent_cheaper_promotes_at_n_50_not_49() {
    let live = snap(400, 90.0, 4.0, 0.0100, 2_000);
    let cheaper = |n| Evidence { canary: snap(n, 90.0, 4.0, 0.0088, 2_000), live, ..Evidence::default() };
    assert_eq!(judge(&canary("c"), &cheaper(49), 0, NOW), Verdict::Wait);
    match judge(&canary("c"), &cheaper(50), 0, NOW) {
        Verdict::Promote { cost_gain_pct, latency_gain_pct } => {
            assert_eq!(cost_gain_pct.round(), 12.0);
            assert_eq!(latency_gain_pct, 0.0);
        }
        v => panic!("expected a promotion, got {v:?}"),
    }
    // The week's two promotions are used: it waits.
    assert_eq!(judge(&canary("c"), &cheaper(50), MAX_PROMOTIONS_PER_WEEK, NOW), Verdict::Wait);
}

#[test]
fn three_points_worse_is_never_promoted_and_a_5_percent_gain_is_not_enough() {
    let live = snap(400, 90.0, 4.0, 0.0100, 2_000);
    for n in [50, 80, 200] {
        let worse = Evidence { canary: snap(n, 87.0, 4.0, 0.0050, 1_000), live, ..Evidence::default() };
        assert!(matches!(judge(&canary("c"), &worse, 0, NOW), Verdict::Reject { rollback: false, .. }), "n={n}");
    }
    // More rework is a quality drop too.
    let rework = Evidence { canary: snap(60, 90.0, 6.5, 0.0050, 1_000), live, ..Evidence::default() };
    assert!(matches!(judge(&canary("c"), &rework, 0, NOW), Verdict::Reject { .. }));
    let small = Evidence { canary: snap(50, 90.0, 4.0, 0.0095, 1_900), live, ..Evidence::default() };
    assert_eq!(judge(&canary("c"), &small, 0, NOW), Verdict::Reject { why: "it saved only 5% cost and 5% time".into(), rollback: false });
}

#[test]
fn a_regression_after_promotion_rolls_back_and_a_quiet_week_keeps_it() {
    let base = snap(400, 90.0, 4.0, 0.0100, 2_000);
    let mut c = canary("c");
    c.stage = Stage::Watch;
    c.promoted_at = Some(NOW - 2 * DAY_MS);
    c.baseline = Some(base);
    let bad = |n| Evidence { since_promotion: snap(n, 85.0, 4.0, 0.0088, 2_000), ..Evidence::default() };
    assert_eq!(judge(&c, &bad(29), 0, NOW), Verdict::Wait, "under 30 new steps it waits");
    assert!(matches!(judge(&c, &bad(30), 0, NOW), Verdict::Reject { rollback: true, .. }));
    let fine = Evidence { since_promotion: snap(120, 89.0, 4.5, 0.0088, 2_000), ..Evidence::default() };
    assert_eq!(judge(&c, &fine, 0, NOW), Verdict::Wait);
    c.promoted_at = Some(NOW - WATCH_DAYS * DAY_MS);
    assert_eq!(judge(&c, &fine, 0, NOW), Verdict::Keep);
}

#[test]
fn shadow_needs_200_steps_and_no_filter_violation() {
    let mut c = canary("s");
    c.stage = Stage::Shadow;
    assert_eq!(judge(&c, &Evidence { shadow_steps: 199, ..Evidence::default() }, 0, NOW), Verdict::Wait);
    assert_eq!(judge(&c, &Evidence { shadow_steps: 200, ..Evidence::default() }, 0, NOW), Verdict::StartCanary);
    assert!(matches!(judge(&c, &Evidence { shadow_steps: 500, violations: 1, ..Evidence::default() }, 0, NOW), Verdict::Reject { .. }));
}

/// xorshift, so the property tests are repeatable.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn pick<'a>(&mut self, xs: &[&'a str]) -> &'a str {
        xs[(self.next() % xs.len() as u64) as usize]
    }

    fn coin(&mut self, one_in: u64) -> bool {
        self.next().is_multiple_of(one_in)
    }
}

#[test]
fn the_canary_never_touches_hard_holdout_pinned_or_rejected_retry_steps_and_takes_about_5_percent() {
    let classes = ["chat:default", "code:edit-small", "prepare:hard", "repair:diagnose", "background:summarize", "plan", "episode:step"];
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    let (mut eligible, mut served) = (0u32, 0u32);
    for i in 0..20_000 {
        let r = StepRisk { class: rng.pick(&classes), holdout: rng.coin(10), under_reject: rng.coin(8), pinned: rng.coin(6), hard_tool: rng.coin(9) };
        let episode = format!("ep-{i}");
        let on = canary_eligible(&r) && in_canary(&episode, "chat:default#1");
        if on {
            served += 1;
            assert!(r.class != PREPARE_HARD && !r.class.starts_with("repair:"), "{r:?}");
            assert!(!r.holdout && !r.under_reject && !r.pinned && !r.hard_tool, "{r:?}");
        }
        if canary_eligible(&r) {
            eligible += 1;
        }
        // Deterministic by episode: the same episode always lands the same way.
        assert_eq!(in_canary(&episode, "chat:default#1"), in_canary(&episode, "chat:default#1"));
    }
    let share = served as f64 / eligible as f64;
    assert!((0.04..=0.06).contains(&share), "canary share {share:.4} of {eligible}");
    assert!(!in_canary("", "x"));
}

/// Models "a", "b", "c" are routable and included; "prem" is premium with no grant.
struct Fake;

impl Filters for Fake {
    fn allowed(&self, _class: &str, model: &str) -> bool {
        model != "prem"
    }

    fn expected_cost(&self, _class: &str, model: &str) -> Option<f64> {
        Some(match model {
            "a" => 0.010,
            "b" => 0.008,
            "c" => 0.012,
            _ => 0.050,
        })
    }
}

fn steps_for(class: &str, model: &str, effort: &str, n: u32, pass_of_100: u32, cost: f64, latency: u64) -> Vec<Step> {
    (0..n)
        .map(|i| Step {
            at: NOW - i as u64 * 1000,
            class: class.into(),
            model: model.into(),
            effort: Some(effort.into()),
            arm: Arm::Live,
            pass: Some(i * 100 / n < pass_of_100),
            rework: false,
            cost_usd: cost,
            latency_ms: latency,
        })
        .collect()
}

fn view(class: &str, top: &str) -> (BTreeMap<String, String>, BTreeMap<String, Band>) {
    let row = class_row(class).unwrap();
    (BTreeMap::from([(class.to_string(), top.to_string())]), BTreeMap::from([(class.to_string(), Band::of(row, 0))]))
}

#[test]
fn a_cheaper_model_goes_to_shadow_and_a_pricier_one_is_only_ever_a_card() {
    let mut steps = steps_for("chat:default", "a", "medium", 40, 90, 0.010, 2_000);
    steps.extend(steps_for("chat:default", "b", "medium", 40, 90, 0.008, 2_000));
    let (top, bands) = view("chat:default", "a");
    let cards = scorecards(&steps, NOW);
    let p = propose(&cards, &steps, &TuneState::default(), &top, &bands, &Fake, NOW);
    assert_eq!(p.auto.len(), 1);
    assert_eq!((p.auto[0].stage, p.auto[0].change.clone()), (Stage::Shadow, Some(Change::Order { model: "b".into() })));
    assert!(p.cards.is_empty());
    // Only a pricier, better model: a card, never an automatic change.
    let mut steps = steps_for("chat:default", "a", "medium", 40, 85, 0.010, 2_000);
    steps.extend(steps_for("chat:default", "c", "medium", 40, 95, 0.012, 2_000));
    let p = propose(&scorecards(&steps, NOW), &steps, &TuneState::default(), &top, &bands, &Fake, NOW);
    assert!(p.auto.is_empty(), "{:?}", p.auto);
    assert_eq!(p.cards.len(), 1);
    assert_eq!(p.cards[0].stage, Stage::Card);
    assert_eq!(p.cards[0].cost_rise_pct.map(f64::round), Some(20.0));
    assert_eq!(card_text(&p.cards[0]).1, "everyday chat would do better on c, which costs about 20% more. Use it first?");
    // A higher start rung that does better is a card too.
    let mut steps = steps_for("chat:default", "a", "medium", 40, 80, 0.010, 2_000);
    steps.extend(steps_for("chat:default", "a", "high", 40, 95, 0.013, 2_500));
    let p = propose(&scorecards(&steps, NOW), &steps, &TuneState::default(), &top, &bands, &Fake, NOW);
    assert!(p.auto.is_empty());
    assert_eq!(p.cards[0].change, Some(Change::Start { effort: "high".into() }));
    assert_eq!(card_text(&p.cards[0]).1, "everyday chat would do better at High, which costs about 30% more. Start it at High?");
}

#[test]
fn no_candidate_ever_lowers_a_floor_touches_prepare_hard_or_picks_an_ungranted_route() {
    let models = ["a", "b", "c", "prem"];
    let efforts = ["none", "minimal", "low", "medium", "high", "xhigh"];
    let classes = ["chat:default", "code:multi-file", "prepare:hard", "plan", "background:summarize"];
    let mut rng = Rng(42);
    for round in 0..300 {
        let mut steps = Vec::new();
        for _ in 0..6 {
            let class = rng.pick(&classes);
            let n = 20 + (rng.next() % 40) as u32;
            let pass = 60 + (rng.next() % 41) as u32;
            let cost = 0.001 + (rng.next() % 100) as f64 / 1000.0;
            let lat = 500 + rng.next() % 4000;
            steps.extend(steps_for(class, rng.pick(&models), rng.pick(&efforts), n, pass, cost, lat));
        }
        let mut top = BTreeMap::new();
        let mut bands = BTreeMap::new();
        for c in classes {
            top.insert(c.to_string(), rng.pick(&models).to_string());
            bands.insert(c.to_string(), Band::of(class_row(c).unwrap(), 0));
        }
        let p = propose(&scorecards(&steps, NOW), &steps, &TuneState::default(), &top, &bands, &Fake, NOW);
        for c in p.auto.iter().chain(&p.cards) {
            assert_ne!(c.class, PREPARE_HARD, "round {round}");
            let band = bands[&c.class];
            match c.change.as_ref().unwrap() {
                Change::Order { model } => assert!(Fake.allowed(&c.class, model), "round {round}: {c:?}"),
                Change::Start { effort } => {
                    let r = rung(effort).unwrap();
                    assert!(r >= band.floor && r <= band.ceiling, "round {round}: {c:?}");
                }
            }
        }
        for c in &p.auto {
            assert_eq!(c.stage, Stage::Shadow);
            if let Some(Change::Order { model }) = &c.change {
                assert!(Fake.expected_cost(&c.class, model) <= Fake.expected_cost(&c.class, &top[&c.class]), "round {round}: an automatic change never costs more");
            }
            if let Some(Change::Start { effort }) = &c.change {
                assert!(rung(effort).unwrap() < bands[&c.class].start, "round {round}: an automatic start only goes down");
            }
        }
    }
}

#[test]
fn a_tuned_start_is_clamped_inside_the_band_and_prepare_hard_never_starts_lower() {
    let chat = class_row("chat:default").unwrap();
    assert_eq!(Band::tuned(chat, 0, Some("none")).start, rung("low").unwrap(), "below the floor is clamped to it");
    assert_eq!(Band::tuned(chat, 0, Some("max")).start, rung("xhigh").unwrap(), "above the ceiling is clamped to it");
    assert_eq!(Band::tuned(chat, 0, Some("high")), Band { floor: rung("low").unwrap(), start: rung("high").unwrap(), ceiling: rung("xhigh").unwrap() });
    let hard = class_row(PREPARE_HARD).unwrap();
    assert_eq!(Band::tuned(hard, 0, Some("low")).start, rung("high").unwrap());
    let mut tuning = Tuning::default();
    tuning.starts.insert("chat:default".into(), "minimal".into());
    assert_eq!(crate::route::policy::tuned_band(chat, 0, &tuning).start, rung("low").unwrap());
}

#[test]
fn the_weekly_line_uses_real_numbers() {
    let mut kept = canary("k");
    kept.stage = Stage::Watch;
    kept.promoted_at = Some(NOW - DAY_MS);
    kept.baseline = Some(snap(400, 90.0, 4.0, 0.0100, 2_000));
    kept.promoted_snap = Some(snap(50, 90.2, 4.0, 0.0088, 2_000));
    let state = TuneState { candidates: vec![kept], ..TuneState::default() };
    assert_eq!(review_line(&state, NOW), "Router: 1 change kept (everyday chat: \u{2212}12% cost, quality same), 0 rolled back.");
    assert_eq!(review_line(&TuneState::default(), NOW), "Router: 0 changes kept, 0 rolled back.");
}

#[test]
fn the_live_table_merges_tuned_orders_and_a_rebuild_cannot_drop_them() {
    let row = |m: &str| TableRow { model: m.into(), effort: Some("medium".into()), quality: None, est_cost_usd: None, p50_latency_ms: None, why: String::new() };
    let mut t = RoutingTable::default();
    t.classes.insert("chat:default".into(), super::super::table::ClassTable { speed_quality: super::super::table::SpeedQuality::Balanced, ranked: vec![row("a"), row("b")] });
    let mut tuning = Tuning::default();
    tuning.apply("chat:default", &Change::Order { model: "b".into() });
    let live = tuned_table(&t, &tuning);
    let order: Vec<&str> = live.rows("chat:default").iter().map(|r| r.model.as_str()).collect();
    assert_eq!(order, vec!["b", "a"]);
    assert_eq!(t.rows("chat:default")[0].model, "a", "the saved table is untouched");
}
