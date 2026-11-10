use super::*;
use crate::card_report::{CardReport, ReportItem};
use crate::update_feed::{automation_done_card, digest_card, suggestion_card};

#[test]
fn a_count_without_names_is_vague() {
    let card = suggestion_card(
        "s",
        "While you were in quiet hours",
        "2 updates: Weekly brief · Start my day",
        1,
    );
    assert_eq!(
        vague(&card),
        Some(Vague::CountOnly {
            said: "2 updates:".into(),
            named: 0
        })
    );
}

#[test]
fn a_cut_off_step_is_vague() {
    let card = digest_card(
        "d",
        "Weekly systems briefing",
        "1 recorded win; 2 service/automation issues.\n1. Run /update\n2.",
        1,
    );
    let mut card = card;
    card.body = Some("1 recorded win; 2 service/automation issues.\n1. Run /update\n2.".into());
    assert_eq!(vague(&card), Some(Vague::CutOff("2.".into())));
    let more = suggestion_card("s", "Feed", "Backup ran · Price check · and 2 more", 1);
    assert!(matches!(vague(&more), Some(Vague::CutOff(_))));
}

#[test]
fn an_empty_card_is_vague() {
    let card = suggestion_card("s", "Something happened", "", 1);
    assert_eq!(vague(&card), Some(Vague::Empty));
}

#[test]
fn named_counts_and_plain_facts_pass() {
    let mut card = suggestion_card("s", "Issues", "", 1);
    card.body = Some("2 services failed:\n- xAI auth missing\n- night-check failed twice".into());
    assert_eq!(vague(&card), None);
    let mut labelled = suggestion_card("s", "Issues", "", 1);
    labelled.body = Some("3 changes landed. Changes: faster sync; new menu; fixed crash".into());
    labelled.body = Some("3 changes landed.\nChanges: faster sync; new menu; fixed crash".into());
    assert_eq!(vague(&labelled), None);
    let fact = automation_done_card("a", "Report", "8 skill runs, all fine since Monday.", 1);
    assert_eq!(vague(&fact), None);
    let mut items = digest_card("d", "Held", "4 updates", 1);
    items.pulse.report = Some(CardReport {
        found: "4 updates held".into(),
        items: (0..4)
            .map(|i| ReportItem {
                card_id: format!("c{i}"),
                found: format!("Update {i}"),
                summary: "ok".into(),
            })
            .collect(),
        ..Default::default()
    });
    assert_eq!(vague(&items), None);
}

// Audit: every card maker below must pass the shared rule.

#[test]
fn the_quiet_hours_roll_up_names_every_held_update() {
    let mut cards: Vec<_> = [
        "Backup ran",
        "Price check ran",
        "Brief ready",
        "Repo synced",
    ]
    .iter()
    .enumerate()
    .map(|(i, t)| {
        let mut c = automation_done_card(&format!("a{i}"), t, "Finished with no errors.", 10);
        c.held = true;
        c
    })
    .collect();
    let (_, id) = crate::pulse::release_quiet_batch(&mut cards, 99);
    let digest = cards
        .iter()
        .find(|c| Some(&c.id) == id.as_ref())
        .expect("digest");
    assert_eq!(vague(digest), None, "{}", visible_text(digest));
}

#[test]
fn the_systems_digest_names_its_wins_issues_and_steps() {
    use crate::self_review::SkillStats;
    use crate::systems_brief::{build_brief, BriefInputs, CARD_TITLE};
    let brief = build_brief(&BriefInputs {
        day: "2026-11-08".into(),
        generated_at: 1,
        skill_stats: vec![SkillStats {
            skill: "deploy".into(),
            runs: 6,
            failures: 4,
            reasons: [("timeout".to_string(), 4)].into(),
        }],
        progress_wins: vec!["Applied skill patch for notes".into()],
        failed_services: vec!["xAI auth missing".into()],
        automation_health: vec!["night-check failed twice".into()],
        cabin_update: Some("v3.0.2".into()),
        services_checked: 5,
        ..Default::default()
    });
    let mut card = digest_card(&brief.source_id(), CARD_TITLE, &brief.card_body(), 1);
    card.body = Some(brief.card_body());
    card.pulse.report = Some(brief.card_report());
    assert_eq!(vague(&card), None, "{}", visible_text(&card));
}
