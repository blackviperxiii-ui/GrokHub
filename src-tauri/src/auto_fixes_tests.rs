use super::*;
use grokhub_core::card_report::{CardReport, CardSubject, FixOffer, OfferKind};
use grokhub_core::proactive::{self as pro, Candidate, CandidateSource, ProactiveRoute};
use std::cell::Cell;
use std::path::PathBuf;

const NOW: u64 = 1_800_000_000_000;
const MIN: u64 = 60_000;

/// A test's scratch folder, removed when the test ends, pass or fail.
struct Scratch(PathBuf);

impl std::ops::Deref for Scratch {
    type Target = Path;
    fn deref(&self) -> &Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn scratch(name: &str) -> Scratch {
    let dir = std::env::temp_dir().join(format!(
        "grokhub-auto-fixes-{name}-{}-{}",
        std::process::id(),
        grokhub_core::now_ms()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    Scratch(dir)
}

/// A crash card for `program`, as the crash watcher writes it.
fn crash_card(id: &str, program: &str) -> UpdateCard {
    let mut card =
        grokhub_core::suggestion_card("nudge:crash", "Crashes", "It keeps crashing", NOW - MIN);
    card.id = id.into();
    card.pulse.report = Some(CardReport {
        found: format!("{program} crashed 4 times today"),
        subject: Some(CardSubject {
            kind: SubjectKind::Program,
            name: program.into(),
        }),
        offers: vec![FixOffer {
            label: "Look into the crashes".into(),
            prompt: format!("Read {program}'s crash history."),
            kind: OfferKind::Look,
            fix_type: Some(FixType::CrashCheck),
        }],
        ..Default::default()
    });
    card
}

fn found(lines: &[&str]) -> repair::DiagnoseReport {
    repair::DiagnoseReport {
        ask: None,
        findings: lines
            .iter()
            .map(|plain| repair::Finding {
                severity: repair::Severity::Warning,
                plain: (*plain).into(),
                detail: String::new(),
                probe: repair::ProbeId::FailedServices,
            })
            .collect(),
        ran: vec![repair::ProbeId::FailedServices],
    }
}

fn receipts(dir: &Path, id: &str) -> Vec<CardReceipt> {
    crate::pulse::cards(dir)
        .unwrap()
        .into_iter()
        .find(|c| c.id == id)
        .and_then(|c| c.pulse.report)
        .map(|r| r.receipts)
        .unwrap_or_default()
}

#[test]
fn every_switch_starts_off() {
    let d = scratch("off");
    assert_eq!(
        view(&d),
        View {
            fixes: vec![FixRow {
                id: "crashCheck".into(),
                label: "Check a crashing program's crash history".into(),
                note: "Read only. Runs when the card shows up, and what it finds goes on the card."
                    .into(),
                on: false,
            }],
            reminders: false,
        }
    );
}

#[test]
fn switches_save_under_the_cabins_keys_and_keep_other_keys() {
    let d = scratch("keys");
    std::fs::write(d.join("app.json"), r#"{"focusHelp":true}"#).unwrap();
    let on = set_fix(&d, "crashCheck", true).unwrap();
    assert!(on.fixes[0].on);
    let on = set_reminders(&d, true).unwrap();
    assert!(on.reminders && on.fixes[0].on);
    let saved: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(d.join("app.json")).unwrap()).unwrap();
    assert_eq!(
        saved,
        json!({"focusHelp": true, "autoFixes": {"on": ["crashCheck"]}, "proactiveReminders": true})
    );
    let off = set_fix(&d, "crashCheck", false).unwrap();
    assert!(!off.fixes[0].on);
    assert_eq!(fixes(&d), AutoFixes::default());
    assert_eq!(
        set_fix(&d, "wipeDisk", true).unwrap_err(),
        "That switch is gone. Settings now shows what is left."
    );
}

#[test]
fn a_broken_switch_value_reads_as_all_off() {
    let d = scratch("broken");
    std::fs::write(d.join("app.json"), r#"{"autoFixes":{"on":["nope"]}}"#).unwrap();
    assert_eq!(fixes(&d), AutoFixes::default());
}

#[test]
fn with_the_switch_off_a_crash_card_waits_for_its_tap() {
    let d = scratch("tap");
    let card = crash_card("crash-1", "bluetoothd");
    assert!(crate::pulse::add_post(&d, card.clone()).unwrap());
    let looked = Cell::new(0);
    let look = |_: &repair::ProgramName| {
        looked.set(looked.get() + 1);
        found(&["Bluetooth stopped."])
    };
    assert_eq!(after_post(&d, &[card], NOW, &look), 0);
    assert_eq!(looked.get(), 0);
    assert!(receipts(&d, "crash-1").is_empty());
}

#[test]
fn a_switched_on_look_runs_once_and_its_receipt_lands_on_the_card() {
    let d = scratch("look");
    set_fix(&d, "crashCheck", true).unwrap();
    let card = crash_card("crash-1", "bluetoothd");
    assert!(crate::pulse::add_post(&d, card.clone()).unwrap());
    let asked = std::cell::RefCell::new(Vec::new());
    let look = |name: &repair::ProgramName| {
        asked.borrow_mut().push(name.as_str().to_string());
        found(&["The Bluetooth service stopped 4 times."])
    };
    assert_eq!(after_post(&d, std::slice::from_ref(&card), NOW, &look), 1);
    assert_eq!(*asked.borrow(), vec!["bluetoothd".to_string()]);
    assert_eq!(
        receipts(&d, "crash-1"),
        vec![CardReceipt {
            at_ms: NOW,
            what: "Checked bluetoothd's crash history".into(),
            result: vec!["The Bluetooth service stopped 4 times.".into()],
        }]
    );
    // The page shows it in the card's report.
    let stored = crate::pulse::cards(&d).unwrap();
    let more = grokhub_core::card_report::report_more(&stored[0]).unwrap();
    assert!(
        more.ends_with(
            "**Checked bluetoothd's crash history**\n- The Bluetooth service stopped 4 times."
        ),
        "{more}"
    );
    // A second pass over the same card does not look again.
    assert_eq!(after_post(&d, &[card], NOW + MIN, &look), 0);
    assert_eq!(receipts(&d, "crash-1").len(), 1);
}

#[test]
fn without_the_system_state_grant_the_receipt_says_what_to_turn_on() {
    let d = scratch("scope");
    set_fix(&d, "crashCheck", true).unwrap();
    let card = crash_card("crash-1", "bluetoothd");
    assert!(crate::pulse::add_post(&d, card.clone()).unwrap());
    // The real look, with no grant in this config folder: it asks and runs nothing.
    let look = |name: &repair::ProgramName| crash_look(&d, name);
    assert_eq!(after_post(&d, &[card], NOW, &look), 1);
    assert_eq!(
        receipts(&d, "crash-1")[0].result,
        vec![LOOK_NEEDS_SCOPE.to_string()]
    );
}

#[test]
fn a_look_that_finds_nothing_says_so_and_a_gone_card_gets_nothing() {
    let d = scratch("nothing");
    set_fix(&d, "crashCheck", true).unwrap();
    let kept = crash_card("crash-1", "bluetoothd");
    let gone = crash_card("crash-2", "pipewire");
    assert!(crate::pulse::add_post(&d, kept.clone()).unwrap());
    let look = |_: &repair::ProgramName| found(&[]);
    assert_eq!(after_post(&d, &[kept, gone], NOW, &look), 1);
    assert_eq!(
        receipts(&d, "crash-1")[0].result,
        vec![LOOK_FOUND_NOTHING.to_string()]
    );
    assert!(receipts(&d, "crash-2").is_empty());
}

#[test]
fn a_card_with_no_program_or_a_chat_offer_never_looks_on_its_own() {
    let d = scratch("noprog");
    set_fix(&d, "crashCheck", true).unwrap();
    let mut chat = crash_card("crash-1", "bluetoothd");
    chat.pulse.report.as_mut().unwrap().offers[0].kind = OfferKind::Chat;
    let mut nameless = crash_card("crash-2", "bluetoothd");
    nameless.pulse.report.as_mut().unwrap().subject = None;
    for card in [&chat, &nameless] {
        assert!(crate::pulse::add_post(&d, card.clone()).unwrap());
    }
    let look = |_: &repair::ProgramName| -> repair::DiagnoseReport {
        panic!("no look should run");
    };
    assert_eq!(after_post(&d, &[chat, nameless], NOW, &look), 0);
}

fn due_card(due_in: Option<u64>) -> UpdateCard {
    let mut c = Candidate::soft(
        CandidateSource::Workboard,
        "renew the domain",
        "domain renewal",
        0.9,
        1.0,
        "It runs out today.",
    );
    c.due_at = due_in.map(|d| NOW + d);
    pro::proactive_card(&c, ProactiveRoute::Suggest, NOW)
}

#[test]
fn a_reminder_goes_out_only_when_on_and_due_within_the_hour() {
    let soon = due_card(Some(30 * MIN));
    let title = grokhub_core::pulse::i_can_title(&soon);
    assert!(!title.is_empty());
    assert_eq!(reminder(&soon, NOW, true), Some(title));
    // Off by default: no notice.
    assert_eq!(reminder(&soon, NOW, false), None);
    // An hour or more away, already past, or no due time: no notice.
    assert_eq!(reminder(&due_card(Some(60 * MIN)), NOW, true), None);
    assert_eq!(reminder(&due_card(None), NOW, true), None);
    assert_eq!(reminder(&soon, NOW + 31 * MIN, true), None);
    // A card that is not a proactive one is not reminded about.
    let mut plain = crash_card("crash-1", "bluetoothd");
    plain.pulse.due_at = Some(NOW + 10 * MIN);
    assert_eq!(reminder(&plain, NOW, true), None);
}

#[test]
fn the_crash_watchers_own_card_gets_its_look_when_switched_on() {
    use grokhub_core::nudge_watchers::{CrashCount, NudgeWatcherSettings, WatcherSnapshot};
    let d = scratch("watcher");
    set_fix(&d, "crashCheck", true).unwrap();
    let settings = NudgeWatcherSettings {
        crash_spike: true,
        ..NudgeWatcherSettings::default()
    };
    let snap = WatcherSnapshot {
        crashes: vec![CrashCount {
            app: "bluetoothd".into(),
            count: 4,
        }],
        ..WatcherSnapshot::default()
    };
    let posted = crate::nudges::tick_with(&d, &settings, &snap, "2027-01-15", NOW).unwrap();
    assert_eq!(posted.len(), 1);
    let look = |name: &repair::ProgramName| {
        assert_eq!(name.as_str(), "bluetoothd");
        found(&["The Bluetooth service stopped 4 times."])
    };
    assert_eq!(after_post(&d, &posted, NOW, &look), 1);
    assert_eq!(
        receipts(&d, &posted[0].id),
        vec![CardReceipt {
            at_ms: NOW,
            what: "Checked bluetoothd's crash history".into(),
            result: vec!["The Bluetooth service stopped 4 times.".into()],
        }]
    );
}
