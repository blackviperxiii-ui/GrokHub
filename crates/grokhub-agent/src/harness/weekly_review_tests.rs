use std::path::PathBuf;

use grokhub_core::outcome::{self as oc, OutcomeSignals, TaskOutcome};
use grokhub_core::self_review::{CodeIdea, QuietEntry, RankedProposal, RevertCandidate};
use grokhub_core::{LocalClock, SkillMd, UpdateKind};

use super::*;
use crate::harness::{append_span, fixture_span};

const DAY: u64 = 24 * 60 * 60 * 1000;

fn dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!(
        "grokhub-weekly-review-{tag}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn clock(weekday: u8, hour: u32) -> LocalClock {
    LocalClock {
        now_ms: 0,
        weekday,
        hour,
        minute: 0,
    }
}

fn raw(d: &std::path::Path) -> serde_json::Value {
    serde_json::from_str(&std::fs::read_to_string(d.join(SELF_REVIEW_FILE)).unwrap()).unwrap()
}

#[test]
fn the_guard_claims_sunday_night_once_and_keeps_the_cabins_other_fields() {
    let d = dir("claim");
    std::fs::write(
        d.join(SELF_REVIEW_FILE),
        r#"{"last_day":"2026-11-08","pending":[{"source":"self-review:draft:a","skill":"a","md":"x"}],"later_field":7}"#,
    )
    .unwrap();
    // Saturday night, and Sunday before the dream hour: not due.
    assert!(!claim_week(&d, "2026-11-14", &clock(6, 23), 3));
    assert!(!claim_week(&d, "2026-11-15", &clock(0, 2), 3));
    assert_eq!(last_day(&d).as_deref(), Some("2026-11-08"));
    // Sunday at the dream hour: claimed, then refused for the rest of the night.
    assert!(claim_week(&d, "2026-11-15", &clock(0, 3), 3));
    assert!(!claim_week(&d, "2026-11-15", &clock(0, 4), 3));
    let v = raw(&d);
    assert_eq!(v["last_day"], "2026-11-15");
    assert_eq!(v["later_field"], 7);
    assert_eq!(v["pending"][0]["source"], "self-review:draft:a");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn a_night_the_cabin_already_started_is_not_claimed_again() {
    let d = dir("cabin-ran");
    // What the cabin writes before its own pass (pretty JSON, snake_case).
    std::fs::write(
        d.join(SELF_REVIEW_FILE),
        "{\n  \"last_day\": \"2026-11-15\",\n  \"pending\": []\n}",
    )
    .unwrap();
    assert!(!claim_week(&d, "2026-11-15", &clock(0, 5), 3));
    // No file yet: the first app to look claims it.
    let fresh = dir("no-file");
    assert!(claim_week(&fresh, "2026-11-15", &clock(0, 5), 3));
    assert_eq!(last_day(&fresh).as_deref(), Some("2026-11-15"));
    let _ = std::fs::remove_dir_all(&d);
    let _ = std::fs::remove_dir_all(&fresh);
}

#[test]
fn pending_quiet_log_and_saved_ideas_round_trip_in_the_cabins_shape() {
    let d = dir("pending");
    add_pending(
        &d,
        Pending {
            source: "self-review:patch:report".into(),
            skill: "report".into(),
            md: "v1".into(),
        },
    )
    .unwrap();
    add_pending(
        &d,
        Pending {
            source: "self-review:patch:report".into(),
            skill: "report".into(),
            md: "v2".into(),
        },
    )
    .unwrap();
    assert_eq!(
        pending(&d),
        vec![Pending {
            source: "self-review:patch:report".into(),
            skill: "report".into(),
            md: "v2".into(),
        }]
    );
    assert_eq!(
        pending_for(&d, "self-review:patch:report").map(|p| p.md),
        Some("v2".to_string())
    );
    log_quiet(
        &d,
        QuietEntry {
            day: "2026-11-15".into(),
            source: "self-review:draft:deploy".into(),
            title: "Make a skill: deploy".into(),
            body: "You did this 4 times in the last two weeks".into(),
        },
    )
    .unwrap();
    save_idea(&d, "Faster export", "Problem: slow\n\nEvidence: 3 runs", 42).unwrap();
    remove_pending(&d, "self-review:patch:report").unwrap();
    let v = raw(&d);
    assert_eq!(v["pending"], serde_json::json!([]));
    assert_eq!(v["quiet_log"][0]["title"], "Make a skill: deploy");
    assert_eq!(v["quiet_log"][0]["day"], "2026-11-15");
    assert_eq!(
        v["saved_ideas"],
        serde_json::json!([{"title": "Faster export", "text": "Problem: slow\n\nEvidence: 3 runs", "at": 42}])
    );
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn saved_ideas_keep_the_newest_fifty() {
    let d = dir("ideas-cap");
    for i in 0..52u64 {
        save_idea(&d, &format!("idea {i}"), "t", i).unwrap();
    }
    let v = raw(&d);
    let ideas = v["saved_ideas"].as_array().unwrap();
    assert_eq!(ideas.len(), 50);
    assert_eq!(ideas[0]["title"], "idea 2");
    assert_eq!(ideas[49]["title"], "idea 51");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn revert_and_code_idea_cards_use_the_cabins_words() {
    let r = RevertCandidate {
        skill: "weekly-report".into(),
        version: 1,
        before: (4, 4),
        after: (1, 4),
    };
    let c = revert_card(&r);
    assert_eq!(c.source, "self-review:revert:weekly-report");
    assert_eq!(c.title, "Revert weekly-report to v1?");
    assert_eq!(c.body, "4 of 4 runs worked before the change, 1 of 4 since");
    assert_eq!(
        c.details,
        "4 of 4 runs worked before the change, 1 of 4 since.\n\nRevert puts back v1 of the skill. /skills changes lists every version."
    );
    assert_eq!(c.md, "");

    let idea = CodeIdea {
        slug: "faster-export".into(),
        title: "Faster export".into(),
        problem: "Export waits on the disk".into(),
        evidence: "3 of 5 runs timed out".into(),
    };
    let c = code_idea_card(&idea);
    assert_eq!(c.source, "self-review:code:faster-export");
    assert_eq!(c.body, "3 of 5 runs timed out");
    assert_eq!(
        c.md,
        "Problem: Export waits on the disk\n\nEvidence: 3 of 5 runs timed out"
    );
    assert!(c.details.ends_with(
        "never changes its own code. Save idea keeps this in Settings → Your PC → Automations & limits; Copy as GitHub issue puts it on the clipboard with home paths, emails and keys taken out, for you to paste."
    ));
}

#[test]
fn a_patch_card_replaces_the_steps_and_shows_what_changed() {
    let existing = SkillMd {
        name: "report-0".into(),
        description: "the report".into(),
        instructions: "1. Open report.md\n2. click Export\n".into(),
        ..Default::default()
    };
    let p = RankedProposal {
        skill: "report-0".into(),
        snippet: false,
        title: "Wait for Export".into(),
        steps: "1. Open report.md 2. click Export 3. wait for the file".into(),
        numbers: "3 of 5 report-0 runs needed a fix".into(),
        failures: 3,
    };
    let c = patch_card(&existing, &p);
    assert_eq!(c.source, "self-review:patch:report-0");
    assert_eq!(c.title, "Wait for Export");
    assert_eq!(c.body, "3 of 5 report-0 runs needed a fix");
    assert!(
        c.details.starts_with(
            "What changed: 1 added, 0 merged, 0 removed\n- Added: \"3. wait for the file\"\n\n3 of 5"
        ),
        "{}",
        c.details
    );
    assert!(
        c.details
            .contains("  2. click Export\n+ 3. wait for the file\n"),
        "{}",
        c.details
    );
    let after = grokhub_core::parse_skill_md(&c.md);
    assert_eq!(
        after.instructions,
        "1. Open report.md\n2. click Export\n3. wait for the file\n\n"
    );
    assert_eq!(after.description, "the report");
}

#[test]
fn three_similar_runs_give_one_draft_card_with_its_steps() {
    let d = dir("draft");
    let now = 100 * DAY;
    for (i, ask) in [
        "Summarize the March invoices",
        "summarize invoices for March",
        "Summarize March invoices again",
    ]
    .iter()
    .enumerate()
    {
        let mut s = fixture_span(
            10 + i as u64,
            "open_file",
            "{}",
            "allow",
            "opened",
            Some(true),
            "",
        );
        s.session_id = format!("chat-d{i}");
        append_span(&d, &s).unwrap();
        let sig = OutcomeSignals {
            verify_ok: true,
            ..Default::default()
        };
        let o = TaskOutcome::finished(
            &format!("chat-d{i}:1"),
            &oc::topic_signature(ask),
            &sig,
            now - (i as u64 + 1) * DAY,
        )
        .with_spans([s.span_ref()]);
        oc::append_outcome(&d, &o).unwrap();
    }
    let cards = first_cards(&d, &oc::read_outcomes(&d), &[], now);
    assert_eq!(cards.len(), 1);
    let c = &cards[0];
    assert_eq!(c.source, "self-review:draft:summarize-march-invoic");
    assert_eq!(c.title, "Make a skill: summarize-march-invoic");
    assert_eq!(c.body, "You did this 3 times in the last two weeks");
    assert!(
        c.details.starts_with("What changed: 1 added, 0 merged, 0 removed\n- Added: \"1. open_file\"\n\nYou did this 3 times"),
        "{}",
        c.details
    );
    assert!(
        c.details.contains("+ name: summarize-march-invoic\n")
            && c.details.contains("+ 1. open_file\n"),
        "{}",
        c.details
    );
    assert_eq!(
        grokhub_core::parse_skill_md(&c.md).instructions,
        "1. open_file\n"
    );
    // A skill by that name already exists: no draft.
    let have = SkillMd {
        name: "summarize-march-invoic".into(),
        ..Default::default()
    };
    assert!(first_cards(&d, &oc::read_outcomes(&d), &[have], now).is_empty());
    let _ = std::fs::remove_dir_all(&d);
}

fn card(n: usize) -> ProposalCard {
    ProposalCard {
        source: format!("self-review:draft:skill-{n}"),
        skill: format!("skill-{n}"),
        title: format!("Make a skill: skill-{n}"),
        body: "You did this 3 times in the last two weeks".into(),
        details: "details".into(),
        md: format!("md {n}"),
    }
}

#[test]
fn posting_stops_at_five_cards_and_skips_a_live_card_for_the_same_source() {
    let d = dir("post");
    let mut cards = vec![grokhub_core::suggestion_card(
        "self-review:draft:skill-0",
        "Make a skill: skill-0",
        "",
        1,
    )];
    let mut budget = Budget::new(false, "2026-11-15");
    let posted: Vec<bool> = (0..7)
        .map(|n| post_card(&d, &mut cards, &mut budget, card(n), 1_000))
        .collect();
    assert_eq!(posted, vec![false, true, true, true, true, true, false]);
    assert_eq!(budget.left, 0);
    assert!(budget.spent());
    assert_eq!(cards.len(), 6);
    assert!(cards.iter().all(|c| c.kind == UpdateKind::Suggestion));
    let c1 = cards
        .iter()
        .find(|c| c.source_id == "self-review:draft:skill-1")
        .unwrap();
    assert_eq!(c1.details.as_deref(), Some("details"));
    let sources: Vec<String> = pending(&d).into_iter().map(|p| p.source).collect();
    assert_eq!(
        sources,
        vec![
            "self-review:draft:skill-1",
            "self-review:draft:skill-2",
            "self-review:draft:skill-3",
            "self-review:draft:skill-4",
            "self-review:draft:skill-5",
        ]
    );
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn quiet_mode_logs_the_card_and_posts_nothing() {
    let d = dir("quiet");
    let mut cards = Vec::new();
    let mut budget = Budget::new(true, "2026-11-15");
    assert!(!post_card(&d, &mut cards, &mut budget, card(1), 1_000));
    assert!(cards.is_empty());
    assert!(pending(&d).is_empty());
    assert_eq!(budget.left, 4);
    let v = raw(&d);
    assert_eq!(
        v["quiet_log"],
        serde_json::json!([{
            "day": "2026-11-15",
            "source": "self-review:draft:skill-1",
            "title": "Make a skill: skill-1",
            "body": "You did this 3 times in the last two weeks"
        }])
    );
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn the_model_call_needs_budget_and_a_failure_or_a_brain_ask() {
    let ok = grokhub_core::self_review::SkillStats {
        skill: "a".into(),
        runs: 3,
        failures: 0,
        ..Default::default()
    };
    let failed = grokhub_core::self_review::SkillStats {
        failures: 1,
        ..ok.clone()
    };
    let mut b = Budget::new(false, "2026-11-15");
    assert!(!wants_model(&b, std::slice::from_ref(&ok), false));
    assert!(wants_model(&b, std::slice::from_ref(&failed), false));
    assert!(wants_model(&b, std::slice::from_ref(&ok), true));
    b.left = 0;
    assert!(!wants_model(&b, &[failed], true));
}

#[test]
fn pass_cards_keep_order_drop_a_missing_skill_and_end_with_ideas() {
    let skills = vec![SkillMd {
        name: "report".into(),
        instructions: "1. open".into(),
        ..Default::default()
    }];
    let pass = WeeklyPass {
        kept: vec![
            RankedProposal {
                skill: "gone".into(),
                title: "Gone".into(),
                steps: "1. x".into(),
                ..Default::default()
            },
            RankedProposal {
                skill: "be-brief".into(),
                snippet: true,
                title: "Be brief".into(),
                steps: "Answer in two lines.".into(),
                numbers: "4 of 9 chats ran long".into(),
                failures: 4,
            },
            RankedProposal {
                skill: "report".into(),
                snippet: false,
                title: "Open then save".into(),
                steps: "1. open 2. save".into(),
                numbers: "2 of 4 report runs needed a fix".into(),
                failures: 2,
            },
        ],
        ideas: vec![CodeIdea {
            slug: "idea".into(),
            title: "Idea".into(),
            problem: "p".into(),
            evidence: "e".into(),
        }],
        ..Default::default()
    };
    let sources: Vec<String> = pass_cards(&pass, &skills)
        .into_iter()
        .map(|c| c.source)
        .collect();
    assert_eq!(
        sources,
        vec![
            "self-review:snippet:be-brief",
            "self-review:patch:report",
            "self-review:code:idea"
        ]
    );
    let note = snippet_card(&pass.kept[1]);
    assert_eq!(note.md, "Answer in two lines.");
    assert!(note.details.starts_with("4 of 9 chats ran long.\n\nApply adds this note to the instructions of every chat; the safety and approval rules still win:\n\n“Answer in two lines.”\n\nOne chat in five tries it first; "));
}

#[test]
fn cabin_skills_lists_switched_on_skills_by_name() {
    let d = dir("skills");
    for (folder, name) in [
        ("b-skill", "b-skill"),
        ("a-skill", "a-skill"),
        ("off-one", "off-one"),
    ] {
        std::fs::create_dir_all(d.join(folder)).unwrap();
        let md = grokhub_core::render_skill_md(&SkillMd {
            name: name.into(),
            instructions: "1. go".into(),
            ..Default::default()
        });
        std::fs::write(d.join(folder).join("SKILL.md"), md).unwrap();
    }
    crate::set_skill_off_in(&d, "off-one", true).unwrap();
    let names: Vec<String> = cabin_skills(&d).into_iter().map(|s| s.name).collect();
    assert_eq!(names, vec!["a-skill", "b-skill"]);
    assert!(cabin_skills(&d.join("missing")).is_empty());
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn trial_cards_say_trying_kept_or_rolled_back() {
    let mut v = Version {
        version: "skill:report@abc".into(),
        label: "Wait for Export in report".into(),
        diff: "+1 line".into(),
        target: crate::harness::Metric::Steps,
        share_pct: 20,
        min_runs: 5,
        at: 9,
        ..Default::default()
    };
    let c = trial_card(&v);
    assert_eq!(c.title, "Trying: Wait for Export in report");
    assert_eq!(
        c.body.as_deref(),
        Some("+1 line. One run in 5 uses it; it stays if it takes fewer steps over 5 runs.")
    );
    assert_eq!(c.source_id, "canary:skill:report@abc:canary");
    v.status = VersionStatus::Kept;
    v.note = "Kept: 4 fewer steps".into();
    assert_eq!(trial_card(&v).title, "Kept: Wait for Export in report");
    v.status = VersionStatus::RolledBack;
    assert_eq!(
        trial_card(&v).title,
        "Rolled back: Wait for Export in report"
    );
}

#[test]
fn a_quiet_brain_tweak_is_logged_and_spends_one_card() {
    let d = dir("brain-quiet");
    let tweak = crate::harness::BrainTweak {
        proposal: grokhub_core::self_review::BrainProposal {
            target: "policy:quick".into(),
            title: "Shorter plans".into(),
            text: "x\n".into(),
        },
        ..Default::default()
    };
    let mut b = Budget::new(true, "2026-11-15");
    assert_eq!(brain_step(&d, &tweak, &mut b, 1), TweakDone::Logged);
    assert_eq!(b.left, 4);
    assert_eq!(raw(&d)["quiet_log"][0]["title"], "Shorter plans");
    b.left = 0;
    assert_eq!(brain_step(&d, &tweak, &mut b, 1), TweakDone::Skipped);
    let _ = std::fs::remove_dir_all(&d);
}
