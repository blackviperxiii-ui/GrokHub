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
    let like = one(Reaction::Like);
    for r in [
        Reaction::Engage,
        Reaction::Use,
        Reaction::Dwell,
        Reaction::Snooze,
    ] {
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
fn home_events_map_to_taste_and_closing_carries_no_weight() {
    use crate::CardEvent as E;
    assert_eq!(from_card_event(E::Opened), Some(Reaction::Engage));
    assert_eq!(from_card_event(E::FollowUp), Some(Reaction::Engage));
    assert_eq!(from_card_event(E::More), Some(Reaction::Like));
    assert_eq!(from_card_event(E::Less), Some(Reaction::Dislike));
    assert_eq!(from_card_event(E::Ran), Some(Reaction::Use));
    for closed in [E::Dismissed, E::Rejected, E::Unhidden] {
        assert_eq!(from_card_event(closed), None);
    }
}

#[test]
fn home_ranking_reads_the_same_taste_as_pulse() {
    let liked = crate::automation_done_card("a", "Loop", "ran", 1_000);
    let other = crate::automation_done_card("b", "Loop", "ran", 1_000);
    let traits = card_traits(&liked);
    let mut t = Taste::default();
    for i in 0..4 {
        t.record(&format!("x{i}"), traits.clone(), Reaction::Like, 0);
    }
    let mut prefs = crate::CardPrefs::default();
    let plain = crate::card_score(&other, &prefs, 2_000, false);
    prefs.taste = std::sync::Arc::new(t.weights());
    let lifted = crate::card_score(&liked, &prefs, 2_000, false);
    assert!(lifted > plain, "{lifted} > {plain}");
    assert!(
        lifted - plain
            <= crate::card_prefs::HOME_TASTE_CAP * crate::card_prefs::HOME_TASTE_PER_POINT + 1e-9
    );
}
