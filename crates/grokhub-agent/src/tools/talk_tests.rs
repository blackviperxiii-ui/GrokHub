//! LO-45: which Briefs start a chat on Talk, and how its profile moves.

use super::*;
use crate::brain::{classify, BriefInput};

fn brief(intent: Intent, domain: &str, confidence: u8) -> Brief {
    Brief {
        intent,
        domain: domain.into(),
        confidence,
        ..Default::default()
    }
}

fn sorted(text: &str) -> Brief {
    classify(
        &BriefInput {
            text,
            files: 0,
            tools: &[],
            recent: &[],
            background: false,
        },
        None,
    )
}

#[test]
fn talk_needs_a_talk_intent_and_a_sure_brief() {
    assert!(wants_talk(&brief(Intent::Chat, "general", 100)));
    assert!(wants_talk(&brief(Intent::Creative, "media", TALK_SURE)));
    for domain in ["general", "game-help", "planning"] {
        assert!(wants_talk(&brief(Intent::Question, domain, 90)), "{domain}");
    }
    assert!(!wants_talk(&brief(Intent::Chat, "general", TALK_SURE - 1)));
    assert!(!wants_talk(&brief(Intent::Question, "code", 100)));
    assert!(!wants_talk(&brief(Intent::Question, "system", 100)));
    for intent in [
        Intent::Task,
        Intent::Repair,
        Intent::Research,
        Intent::Background,
    ] {
        assert!(!wants_talk(&brief(intent, "general", 100)), "{intent:?}");
    }
}

#[test]
fn the_local_sort_puts_small_talk_on_talk_and_file_asks_on_full() {
    assert!(wants_talk(&sorted("hi there")), "{:?}", sorted("hi there"));
    assert!(!wants_talk(&sorted("Read my notes.")));
    // The shell smoke test's first prompt must still be offered `write`.
    assert!(!wants_talk(&sorted("write a note from the smoke test")));
}

#[test]
fn first_brief_wins_and_only_a_work_brief_asks_to_upgrade() {
    let s = "talk-unit-first";
    assert_eq!(profile(s), Profile::Full, "no Brief yet");
    assert!(!note_brief(s, &brief(Intent::Chat, "general", 100)));
    assert_eq!(profile(s), Profile::Talk);
    assert!(!note_brief(s, &brief(Intent::Creative, "general", 100)));
    assert!(note_brief(s, &brief(Intent::Task, "files", 100)));
    assert!(mark_upgraded(s));
    assert!(!mark_upgraded(s), "upgrade happens once");
    assert_eq!(profile(s), Profile::Upgraded);
    assert!(!note_brief(s, &brief(Intent::Task, "files", 100)));

    let w = "talk-unit-work-first";
    assert!(!note_brief(w, &brief(Intent::Task, "code", 100)));
    assert!(!note_brief(w, &brief(Intent::Chat, "general", 100)));
    assert_eq!(profile(w), Profile::Full);
    assert!(!mark_upgraded(w), "a Full chat never upgrades");

    assert!(!note_brief("", &brief(Intent::Chat, "general", 100)));
    assert_eq!(profile(""), Profile::Full);
}
