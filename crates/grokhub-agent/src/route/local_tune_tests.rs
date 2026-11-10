use super::*;

fn arm(slots: u32, draft: bool, ubatch: u32) -> Arm {
    Arm {
        slots,
        draft,
        ubatch,
    }
}

#[test]
fn the_shadow_check_offers_only_arms_the_plan_has_memory_for() {
    let base = arm(2, true, 512);
    let c = candidates(base, true);
    assert_eq!(c.len(), 8);
    assert!(
        c.iter().all(|a| a.slots <= 2),
        "never more slots than sized"
    );
    assert!(c.contains(&arm(1, false, 256)));
    // No drafter planned: none tried. No `--ubatch-size`: the default only.
    assert_eq!(
        candidates(arm(1, false, 512), false),
        vec![arm(1, false, 512)]
    );
    assert_eq!(arm(4, true, 256).label(), "np4-draft-ub256");
}

#[test]
fn reward_is_decode_speed_less_twice_the_seconds_for_a_1k_prompt() {
    assert_eq!(reward(40.0, 500.0), 36.0);
    assert_eq!(reward(40.0, 0.0), 40.0);
}

#[test]
fn a_canary_that_wins_is_promoted_and_one_that_loses_rolls_back() {
    let base = arm(2, false, 512);
    let cands = vec![base, arm(1, false, 512)];
    let mut r = Record::default();
    // The first start promotes the planner's arm and picks a canary.
    r.started(&base.label(), &base.label(), Some(4_000), &cands);
    assert_eq!(r.promoted.as_deref(), Some("np2-plain-ub512"));
    assert_eq!(r.canary.as_deref(), Some("np1-plain-ub512"));
    // It runs only once the next launch comes: until then, the promoted arm.
    for _ in 0..CANARY_SAMPLES {
        r.observe(&base.label(), 30.0, &cands);
    }
    assert_eq!(r.current(&cands, base), arm(1, false, 512));
    for _ in 0..CANARY_SAMPLES {
        r.observe("np1-plain-ub512", 33.0, &cands);
    }
    assert_eq!(r.promoted.as_deref(), Some("np1-plain-ub512"), "10% better");
    assert_eq!(r.current(&cands, base), arm(1, false, 512));

    // A canary that does worse rolls back, and isn't tried again while its
    // upper bound can't beat the promoted arm.
    let mut r = Record::default();
    r.started(&base.label(), &base.label(), None, &cands);
    for _ in 0..CANARY_SAMPLES {
        r.observe(&base.label(), 30.0, &cands);
    }
    for _ in 0..CANARY_SAMPLES {
        r.observe("np1-plain-ub512", 20.0, &cands);
    }
    assert_eq!(r.promoted.as_deref(), Some("np2-plain-ub512"));
    assert_eq!(r.canary, None);
    assert_eq!(r.current(&cands, base), base);
}

#[test]
fn an_arm_that_leaves_too_little_vram_is_banned_and_rolled_back_at_once() {
    let base = arm(2, true, 512);
    let cands = candidates(base, false);
    let mut r = Record::default();
    r.started(&base.label(), &base.label(), Some(4_000), &cands);
    let canary = r.canary.clone().unwrap();
    r.started(&canary, &base.label(), Some(300), &cands);
    assert!(r.banned.contains(&canary));
    assert_ne!(r.canary.as_deref(), Some(canary.as_str()));
    assert_ne!(r.current(&cands, base).label(), canary);
    // The planner's own arm can be banned too, and the next one runs instead.
    let mut r = Record::default();
    r.started(&base.label(), &base.label(), Some(100), &cands);
    assert!(r.banned.contains(&base.label()));
    assert_ne!(r.current(&cands, base), base);
}

#[test]
fn the_record_lives_in_the_tune_folder_and_counts_only_real_chat_replies() {
    let dir = crate::harness::test_dir("local-tune");
    let base = arm(1, false, 512);
    let tag = Tag {
        key: "m.gguf|gpu|Normal".into(),
        arm: base,
        base,
        cands: candidates(base, true),
    };
    assert_eq!(current(&dir, &tag.key, &tag.cands, base), base);
    started(&dir, &tag, Some(2_000));
    observe(40.0, 500.0, 200);
    observe(90.0, 500.0, 8);
    let rec = load(&dir).remove(&tag.key).unwrap();
    assert_eq!(
        rec.arms.get("np1-plain-ub512"),
        Some(&Stats { n: 1, mean: 36.0 }),
        "a short reply doesn't count"
    );
    assert_eq!(rec.canary.as_deref(), Some("np1-plain-ub256"));
    assert_eq!(
        current(&dir, &tag.key, &tag.cands, base),
        arm(1, false, 256)
    );
    // With the Local AI dial off act, the canary waits and the launch stays
    // on the promoted arm. An unknown key runs the planner's own either way.
    assert_eq!(
        current_under(&dir, &tag.key, &tag.cands, base, true),
        arm(1, false, 256)
    );
    assert_eq!(current_under(&dir, &tag.key, &tag.cands, base, false), base);
    assert_eq!(
        current_under(&dir, "other|cpu|Normal", &tag.cands, base, false),
        base
    );
}

#[test]
fn with_no_new_tries_the_launch_keeps_the_promoted_arm_unless_it_is_banned() {
    let base = arm(2, false, 512);
    let cands = vec![base, arm(1, false, 512), arm(2, false, 256)];
    let r = Record {
        promoted: Some("np1-plain-ub512".into()),
        canary: Some("np2-plain-ub256".into()),
        ..Record::default()
    };
    assert_eq!(r.current(&cands, base), arm(2, false, 256));
    assert_eq!(r.settled(&cands, base), arm(1, false, 512));
    let mut banned = r.clone();
    banned.banned.insert("np1-plain-ub512".into());
    assert_eq!(banned.settled(&cands, base), base);
    // A promoted arm the plan no longer offers falls back to the plan's own.
    assert_eq!(r.settled(&[base], base), base);
    assert_eq!(Record::default().settled(&cands, base), base);
}
