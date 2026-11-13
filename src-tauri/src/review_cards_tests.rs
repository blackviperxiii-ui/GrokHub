use std::path::{Path, PathBuf};

use grokhub_agent::harness::weekly_review::{self as wr, Budget, ProposalCard};
use grokhub_agent::harness::{self as hx, Origin};
use grokhub_core::{SkillMd, UpdateCard, UpdateStatus};

use super::*;

const NOW: u64 = 1_794_711_600_000;

fn temp_dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("shell-review-cards-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// Post one self-review card the way the pass does; returns its id.
fn posted(dir: &Path, card: ProposalCard) -> String {
    let mut out = String::new();
    crate::pulse::edit_cards(dir, &mut |cards| {
        let mut budget = Budget::new(false, "2026-11-15");
        let n = usize::from(wr::post_card(dir, cards, &mut budget, card.clone(), NOW));
        out = cards.last().map(|c| c.id.clone()).unwrap_or_default();
        n
    })
    .unwrap();
    out
}

fn feed(dir: &Path) -> Vec<UpdateCard> {
    crate::pulse::cards(dir).unwrap()
}

fn live(dir: &Path) -> Vec<String> {
    feed(dir)
        .into_iter()
        .filter(|c| c.status != UpdateStatus::Dismissed)
        .map(|c| c.title)
        .collect()
}

fn idea_card() -> ProposalCard {
    wr::code_idea_card(&grokhub_core::self_review::CodeIdea {
        slug: "faster-export".into(),
        title: "Faster export".into(),
        problem: "Export waits on the disk".into(),
        evidence: "3 of 5 runs timed out".into(),
    })
}

#[test]
fn the_buttons_read_like_the_cabins() {
    let mk = |source: &str| grokhub_core::suggestion_card(source, "t", "b", 1);
    assert_eq!(label(&mk("self-review:patch:report")), "Apply change");
    assert_eq!(label(&mk("self-review:draft:report")), "Apply change");
    assert_eq!(label(&mk("self-review:snippet:be-brief")), "Apply change");
    assert_eq!(label(&mk("self-review:revert:report")), "Revert");
    assert_eq!(label(&mk("self-review:code:faster-export")), "Save idea");
    assert_eq!(label(&mk("help:disk")), "");
    assert!(copies_issue(&mk("self-review:code:faster-export")));
    assert!(!copies_issue(&mk("self-review:patch:report")));
    let mut card = mk("self-review:patch:report");
    card.details = Some("3 of 5 runs needed a fix.\n+ 3. wait".into());
    assert_eq!(
        details(&card).as_deref(),
        Some("```text\n3 of 5 runs needed a fix.\n+ 3. wait\n```")
    );
    let mut other = mk("help:disk");
    other.details = Some("x".into());
    assert_eq!(details(&other), None);
}

#[test]
fn save_idea_keeps_it_and_the_card_is_done() {
    let dir = temp_dir("idea");
    let id = posted(&dir, idea_card());
    assert_eq!(live(&dir), vec!["Faster export"]);
    let line = apply(&dir, &id, NOW + 1).unwrap();
    assert_eq!(line, wr::idea_saved_line());
    assert!(line.starts_with("Idea saved in Settings → Your PC → Automations & limits."));
    assert!(live(&dir).is_empty());
    assert!(wr::pending(&dir).is_empty());
    let raw: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join(wr::SELF_REVIEW_FILE)).unwrap())
            .unwrap();
    assert_eq!(raw["saved_ideas"][0]["title"], "Faster export");
    assert_eq!(
        raw["saved_ideas"][0]["text"],
        "Problem: Export waits on the disk\n\nEvidence: 3 of 5 runs timed out"
    );
    // Applied once: a second click finds nothing waiting.
    assert_eq!(apply(&dir, &id, NOW + 2), Err(NOT_WAITING.to_string()));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_chat_note_goes_on_trial_and_a_trial_note_lands() {
    let dir = temp_dir("note");
    let note = wr::snippet_card(&grokhub_core::self_review::RankedProposal {
        skill: "be-brief".into(),
        snippet: true,
        title: "Be brief".into(),
        steps: "Answer in two lines.".into(),
        numbers: "4 of 9 chats ran long".into(),
        failures: 4,
    });
    let id = posted(&dir, note);
    let line = apply(&dir, &id, NOW + 1).unwrap();
    assert_eq!(
        line,
        "Chat note be-brief added: one chat in five tries it first."
    );
    assert_eq!(
        std::fs::read_to_string(hx::snippet_path(&dir, "be-brief")).unwrap(),
        "Answer in two lines.\n"
    );
    assert_eq!(live(&dir), vec!["Trying: Be brief (chat note)"]);
    let _ = std::fs::remove_dir_all(&dir);
}

fn skill(steps: &str) -> SkillMd {
    SkillMd {
        name: "weekly-report".into(),
        description: "the report".into(),
        instructions: steps.into(),
        ..Default::default()
    }
}

#[test]
fn a_patch_the_replay_refuses_keeps_the_card_with_the_reason() {
    let dir = temp_dir("held");
    let skills = dir.join("skills");
    let v1 = skill("1. Open report.md\n2. click Save\n");
    wr::write_skill_md(&skills, &v1).unwrap();
    let path = skills.join("weekly-report").join("SKILL.md");
    let before = std::fs::read(&path).unwrap();
    let card = wr::patch_card(
        &v1,
        &grokhub_core::self_review::RankedProposal {
            skill: "weekly-report".into(),
            snippet: false,
            title: "Use the keyboard".into(),
            steps: "1. Open report.md 2. press Ctrl+S".into(),
            numbers: "1 of 2 weekly-report runs needed a fix".into(),
            failures: 1,
        },
    );
    let id = posted(&dir, card);
    let line = apply(&dir, &id, NOW + 1).unwrap();
    assert!(
        line.starts_with("Replay failed, so the change to weekly-report was not applied:"),
        "{line}"
    );
    assert_eq!(
        std::fs::read(&path).unwrap(),
        before,
        "SKILL.md is byte-identical"
    );
    assert_eq!(live(&dir), vec!["Use the keyboard"], "the card stays");
    let shown = feed(&dir).into_iter().find(|c| c.id == id).unwrap();
    assert!(shown
        .details
        .unwrap_or_default()
        .starts_with("Replay failed"));
    assert_eq!(wr::pending(&dir).len(), 1, "still waiting");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn revert_puts_the_version_before_back_through_the_ledger() {
    let dir = temp_dir("revert");
    let skills = dir.join("skills");
    let v1 = skill("1. Open report.md\n2. click Save\n");
    wr::write_skill_md(&skills, &v1).unwrap();
    let path = skills.join("weekly-report").join("SKILL.md");
    let first = std::fs::read_to_string(&path).unwrap();
    let v2 = skill("1. Open report.md\n2. click Save\n3. wait\n");
    hx::record_skill_change(
        &dir,
        &skills,
        "weekly-report",
        Origin::SelfManage,
        "weekly self-review: Wait",
        || wr::write_skill_md(&skills, &v2),
    )
    .unwrap();
    assert!(std::fs::read_to_string(&path).unwrap().contains("3. wait"));
    let card = wr::revert_card(&grokhub_core::self_review::RevertCandidate {
        skill: "weekly-report".into(),
        version: 1,
        before: (4, 4),
        after: (1, 4),
    });
    let id = posted(&dir, card);
    let line = apply(&dir, &id, NOW + 1).unwrap();
    assert_eq!(
        line,
        "Undid the newest change to weekly-report. The version before it is back."
    );
    assert_eq!(std::fs::read_to_string(&path).unwrap(), first);
    assert!(live(&dir).is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_gone_card_or_an_ordinary_card_has_nothing_to_apply() {
    let dir = temp_dir("gone");
    assert_eq!(apply(&dir, "nope", NOW), Err(GONE.to_string()));
    crate::pulse::edit_cards(&dir, &mut |cards| {
        usize::from(grokhub_core::post_update(
            cards,
            grokhub_core::suggestion_card("help:disk", "Disk is full", "", NOW),
        ))
    })
    .unwrap();
    let id = feed(&dir)[0].id.clone();
    assert_eq!(
        apply(&dir, &id, NOW),
        Err("This card has nothing to apply.".into())
    );
    assert_eq!(
        issue(&dir, &id, None),
        Err("This card has nothing to apply.".into())
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn copy_as_issue_takes_the_home_path_out_and_keeps_the_card() {
    let dir = temp_dir("issue");
    let mut card = idea_card();
    card.md = "Problem: /home/sam/app is slow\n\nEvidence: 3 runs".into();
    let id = posted(&dir, card);
    let text = issue(&dir, &id, Some("/home/sam")).unwrap();
    assert!(text.contains("Faster export"), "{text}");
    assert!(!text.contains("/home/sam"), "{text}");
    assert_eq!(live(&dir), vec!["Faster export"]);
    let _ = std::fs::remove_dir_all(&dir);
}
