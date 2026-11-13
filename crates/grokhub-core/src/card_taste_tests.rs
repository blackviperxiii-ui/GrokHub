use super::*;

fn ev(t: &mut Taste, id: &str, traits: &[&str], r: Reaction) {
    t.record(id, traits.iter().map(|s| s.to_string()).collect(), r, 0);
}

fn w(t: &Taste, k: &str) -> f64 {
    t.weights().get(k).copied().unwrap_or(0.0)
}

#[test]
fn no_reactions_means_no_weights() {
    let t = Taste::default();
    assert!(t.weights().is_empty());
    assert_eq!(t.preferences(), Preferences::default());
    assert_eq!(writer_line(&t.preferences()), "");
}

#[test]
fn one_or_two_dislikes_move_a_little_and_repeats_move_more() {
    let mut t = Taste::default();
    let mut seen = Vec::new();
    for i in 0..6 {
        ev(&mut t, &format!("c{i}"), &["topic:a"], Reaction::Dislike);
        seen.push(w(&t, "topic:a"));
    }
    assert!(
        seen[0] < 0.0 && seen[0] > -0.6,
        "first click is small: {seen:?}"
    );
    for pair in seen.windows(2) {
        assert!(pair[1] < pair[0], "each click adds: {seen:?}");
    }
    assert!(seen[5] < 2.0 * seen[0], "repeats grow the effect: {seen:?}");
}

#[test]
fn likes_grow_the_same_way_in_reverse() {
    let mut t = Taste::default();
    ev(&mut t, "a", &["topic:a"], Reaction::Like);
    let one = w(&t, "topic:a");
    for i in 0..4 {
        ev(&mut t, &format!("b{i}"), &["topic:a"], Reaction::Like);
    }
    let five = w(&t, "topic:a");
    assert!(one > 0.0 && one < 0.6, "{one}");
    assert!(five > 2.0 * one, "{one} {five}");
}

#[test]
fn shared_traits_find_the_real_cause_across_cards() {
    // Liked: picture trait P with topic X. Disliked: P with topic Y.
    // Liked: P with topic Z. P is on liked and disliked cards, so the
    // dislike lands on Y; P, X and Z all rise.
    let mut t = Taste::default();
    ev(&mut t, "1", &["image:p", "topic:x"], Reaction::Like);
    ev(&mut t, "2", &["image:p", "topic:y"], Reaction::Dislike);
    let before = w(&t, "image:p");
    ev(&mut t, "3", &["image:p", "topic:z"], Reaction::Like);
    assert!(w(&t, "topic:y") < 0.0, "Y takes the blame");
    assert!(w(&t, "image:p") > before, "the third card clears P");
    assert!(w(&t, "image:p") > 0.0);
    assert!(w(&t, "topic:z") > 0.0);
    assert!(w(&t, "topic:x") > 0.0);
    assert!(
        w(&t, "topic:y") < w(&t, "image:p") - 0.3,
        "Y sits well below P"
    );
    let prefs = t.preferences();
    assert!(prefs.liked.iter().all(|(k, _)| k != "topic:y"), "{prefs:?}");
    assert!(
        prefs.avoided.iter().any(|(k, _)| k == "topic:y"),
        "{prefs:?}"
    );
    assert!(prefs.liked.iter().any(|(k, _)| k == "image:p"), "{prefs:?}");
    let line = writer_line(&prefs);
    assert!(line.contains("Less of: topic:y"), "{line}");
    assert!(line.contains("image:p"), "{line}");
}

#[test]
fn a_dislike_on_a_card_with_a_liked_trait_blames_the_other_trait_more() {
    let mut t = Taste::default();
    ev(&mut t, "1", &["image:p"], Reaction::Like);
    ev(&mut t, "2", &["image:p"], Reaction::Like);
    ev(&mut t, "3", &["image:p", "topic:y"], Reaction::Dislike);
    assert!(w(&t, "topic:y") < 0.0);
    assert!(
        w(&t, "image:p") > 0.0,
        "already liked, so it absorbs little"
    );
}

#[test]
fn undo_takes_back_one_reaction() {
    let mut t = Taste::default();
    ev(&mut t, "1", &["topic:a"], Reaction::Like);
    assert!(t.undo("1", Reaction::Like));
    assert!(!t.undo("1", Reaction::Like));
    assert!(t.events.is_empty());
}

#[test]
fn traits_are_generic_and_canonical() {
    assert_eq!(
        parse_trait(" Topic : Some  Thing "),
        Some("topic:some thing".into())
    );
    assert_eq!(parse_trait("nocolon"), None);
    assert_eq!(parse_trait("topic:"), None);
    assert_eq!(trait_key("a:b", "c"), None);
    let mut card = crate::update_feed::digest_card("x:1", "Title", "Body", 1);
    card.pulse.traits = vec!["topic:t1".into(), "TOPIC:t1".into(), "tone:calm".into()];
    card.pulse.source_name = Some("Site".into());
    let traits = card_traits(&card);
    assert_eq!(traits[0], "topic:t1");
    assert_eq!(traits[1], "tone:calm");
    assert!(traits.contains(&"source:site".to_string()), "{traits:?}");
    assert!(traits.iter().any(|t| t.starts_with("kind:")), "{traits:?}");
    assert_eq!(traits.iter().filter(|t| *t == "topic:t1").count(), 1);
}

#[test]
fn ranking_lifts_liked_kinds_sinks_disliked_and_hides_the_worst() {
    let mk = |id: &str, tag: &str, at: u64| {
        let mut c = crate::update_feed::digest_card(&format!("s:{id}"), id, "b", at);
        c.id = id.to_string();
        c.pulse.traits = vec![tag.to_string()];
        c
    };
    let hour = 3_600_000;
    let cards = vec![
        mk("new-disliked", "topic:y", 10 * hour),
        mk("old-liked", "topic:x", 7 * hour),
        mk("plain", "topic:q", 9 * hour),
    ];
    let mut t = Taste::default();
    assert_eq!(rank(cards.clone(), &t), cards, "no reactions: unchanged");
    for i in 0..3 {
        ev(&mut t, &format!("l{i}"), &["topic:x"], Reaction::Like);
        ev(&mut t, &format!("d{i}"), &["topic:y"], Reaction::Dislike);
    }
    let ids: Vec<String> = rank(cards.clone(), &t).into_iter().map(|c| c.id).collect();
    assert_eq!(
        ids.first().map(String::as_str),
        Some("old-liked"),
        "{ids:?}"
    );
    assert_eq!(
        ids.last().map(String::as_str),
        Some("new-disliked"),
        "{ids:?}"
    );
    for i in 3..30 {
        ev(&mut t, &format!("d{i}"), &["topic:y"], Reaction::Dislike);
    }
    let ids: Vec<String> = rank(cards, &t).into_iter().map(|c| c.id).collect();
    assert!(!ids.contains(&"new-disliked".to_string()), "{ids:?}");
}

#[test]
fn dismiss_is_not_a_reaction_and_events_are_capped() {
    // There is no Dismiss variant: nothing to record.
    let mut t = Taste::default();
    for i in 0..(EVENTS_MAX + 5) {
        ev(&mut t, &format!("{i}"), &["topic:a"], Reaction::Like);
    }
    assert_eq!(t.events.len(), EVENTS_MAX);
    assert_eq!(t.events[0].card_id, "5");
    t.record("empty", Vec::new(), Reaction::Dislike, 0);
    assert_eq!(
        t.events.len(),
        EVENTS_MAX,
        "a card with no traits teaches nothing"
    );
}

#[test]
fn taste_round_trips_on_disk() {
    let dir = std::env::temp_dir().join(format!("grokhub-card-taste-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mut t = Taste::default();
    ev(&mut t, "1", &["topic:a"], Reaction::Dislike);
    save(&dir, &t).expect("save");
    assert_eq!(load(&dir), t);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn weak_signals_lean_less_than_a_like_or_dislike() {
    let one = |r: Reaction| {
        let mut t = Taste::default();
        ev(&mut t, "c", &["topic:a"], r);
        w(&t, "topic:a")
    };
    assert_eq!(Reaction::Dwell.lean(), (1.0, 0.25));
    let like = one(Reaction::Like);
    for r in [Reaction::Engage, Reaction::Snooze, Reaction::Dwell] {
        let v = one(r);
        assert!(v > 0.0 && v < like, "{r:?}: {v} vs like {like}");
    }
    let dislike = one(Reaction::Dislike);
    let delete = one(Reaction::Delete);
    assert!(delete < 0.0 && delete > dislike, "{delete} vs {dislike}");
}

#[test]
fn opens_across_cards_add_up_and_a_dislike_still_wins_its_trait() {
    let mut t = Taste::default();
    for i in 0..6 {
        ev(
            &mut t,
            &format!("o{i}"),
            &["topic:a", "tone:calm"],
            Reaction::Engage,
        );
    }
    assert!(w(&t, "topic:a") > LIKE_AT, "many opens make a like");
    ev(&mut t, "d1", &["tone:calm", "topic:b"], Reaction::Dislike);
    ev(&mut t, "d2", &["tone:calm", "topic:c"], Reaction::Dislike);
    ev(&mut t, "d3", &["tone:calm", "topic:d"], Reaction::Dislike);
    assert!(
        w(&t, "topic:a") > w(&t, "tone:calm"),
        "the shared tone carries the blame"
    );
}

#[test]
fn old_files_with_only_likes_and_dislikes_still_load() {
    let raw = r#"{"events":[{"cardId":"c","traits":["topic:a"],"reaction":"like","atMs":1}]}"#;
    let t: Taste = serde_json::from_str(raw).expect("old file");
    assert_eq!(t.events[0].reaction, Reaction::Like);
}

#[test]
fn each_reaction_becomes_one_engine_signal_per_trait() {
    use crate::learn::signal::Kind;
    use crate::learn::sources::{taste, trait_id};
    let traits = vec![
        "topic:rust".to_string(),
        "subject:attractive women".to_string(),
    ];
    for (reaction, kind) in [
        (Reaction::Like, Kind::Liked),
        (Reaction::Dislike, Kind::Disliked),
        (Reaction::Engage, Kind::Used),
        (Reaction::Dwell, Kind::Used),
        (Reaction::Snooze, Kind::Picked),
        (Reaction::Delete, Kind::Declined),
    ] {
        let signals = taste(&traits, reaction, 7);
        assert_eq!(signals.len(), 2, "{reaction:?}");
        assert!(signals.iter().all(|s| s.kind == kind && s.at_ms == 7));
    }
    let signals = taste(&traits, Reaction::Like, 7);
    let subjects: Vec<&str> = signals.iter().map(|s| s.subject.as_str()).collect();
    // The kind stays readable; the words never reach the engine.
    assert!(subjects[0].starts_with("trait:topic/"), "{subjects:?}");
    assert!(subjects[1].starts_with("trait:subject/"), "{subjects:?}");
    assert!(subjects
        .iter()
        .all(|s| !s.contains("rust") && !s.contains("women")));
    assert_eq!(trait_id("topic:rust"), trait_id("topic:rust"));
    assert_ne!(trait_id("topic:rust"), trait_id("topic:go"));
    // A kind that isn't a short ASCII word is skipped, not mangled.
    assert_eq!(trait_id("tópico:rust"), None);
    assert_eq!(trait_id("no colon"), None);
}

fn tagged(id: &str, at: u64, traits: &[&str]) -> crate::update_feed::UpdateCard {
    let mut c = crate::update_feed::blank_card(
        id.into(),
        crate::update_feed::UpdateKind::Digest,
        id.into(),
        None,
        at,
    );
    c.pulse.traits = traits.iter().map(|t| (*t).to_string()).collect();
    c
}

fn trait_subject(key: &str) -> crate::learn::Subject {
    let id = crate::learn::sources::trait_id(key).expect(key);
    crate::learn::Subject::new("trait", &id).expect(key)
}

#[test]
fn belief_rank_lifts_liked_traits_and_hides_the_worst() {
    use crate::learn::Beliefs;
    let now = 1_000_000u64;
    let mut beliefs = Beliefs::default();
    let liked = trait_subject("topic:rust");
    for key in ["topic:spam", "tone:bad", "source:junk"] {
        let hated = trait_subject(key);
        for i in 0..8 {
            beliefs.vote(&liked, true, now + i, "signals");
            beliefs.vote(&hated, false, now + i, "signals");
        }
    }
    let rust = tagged("rust", 100, &["topic:rust"]);
    let spam = tagged("spam", 200, &["topic:spam", "tone:bad", "source:junk"]);
    let plain = tagged("plain", 150, &["topic:other"]);
    let order: Vec<String> = rank_from_beliefs(
        vec![spam.clone(), plain.clone(), rust.clone()],
        &beliefs,
        now,
    )
    .into_iter()
    .map(|c| c.id)
    .collect();
    assert_eq!(order, vec!["rust".to_string(), "plain".to_string()]);
    assert!(
        !order.iter().any(|id| id == "spam"),
        "enough disliked traits leave the page: {order:?}"
    );
    let empty = Beliefs::default();
    let kept: Vec<String> = rank_from_beliefs(vec![spam, plain, rust], &empty, now)
        .into_iter()
        .map(|c| c.id)
        .collect();
    assert_eq!(
        kept,
        vec!["spam".to_string(), "plain".to_string(), "rust".to_string()],
        "no beliefs keeps the given order"
    );
}

#[test]
fn home_belief_boost_is_capped_and_quiet_without_the_hub() {
    use crate::learn::Beliefs;
    assert_eq!(home_belief_boost(&tagged("a", 1, &["topic:x"]), 1), 0.0);
    let now = 50u64;
    let mut beliefs = Beliefs::default();
    let keys = ["topic:x", "tone:y", "source:z"];
    for key in keys {
        let sub = trait_subject(key);
        for i in 0..12 {
            beliefs.vote(&sub, true, now + i, "signals");
        }
    }
    let traits: Vec<String> = keys.iter().map(|k| (*k).to_string()).collect();
    let raw = score_from_beliefs(&beliefs, &traits, now);
    assert!(raw > HOME_BELIEF_CAP, "raw {raw}");
    assert_eq!(
        raw.clamp(-HOME_BELIEF_CAP, HOME_BELIEF_CAP),
        HOME_BELIEF_CAP
    );
}

#[test]
fn interest_and_automation_signals_are_hashed_traits() {
    use crate::learn::signal::Kind;
    use crate::learn::sources::{automation_toggle, interest_topic};
    let liked = interest_topic("  Rust Lang ", true, 9);
    assert_eq!(liked.len(), 1);
    assert_eq!(liked[0].kind, Kind::Liked);
    assert!(liked[0].subject.as_str().starts_with("trait:interest/"));
    assert!(!liked[0].subject.as_str().contains("rust"));
    let gone = interest_topic("Rust Lang", false, 9);
    assert_eq!(gone[0].kind, Kind::Disliked);
    assert_eq!(gone[0].subject, liked[0].subject);
    let on = automation_toggle("auto-1", true, 3);
    let off = automation_toggle("auto-1", false, 4);
    assert_eq!(on[0].kind, Kind::Picked);
    assert_eq!(off[0].kind, Kind::Declined);
    assert_eq!(on[0].subject, off[0].subject);
    assert!(on[0].subject.as_str().starts_with("trait:auto/"));
    assert!(interest_topic("  ", true, 1).is_empty());
    assert!(automation_toggle("", true, 1).is_empty());
}
