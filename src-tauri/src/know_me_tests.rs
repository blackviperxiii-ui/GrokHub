use super::*;
use grokhub_core::know_me::Tag;
use std::collections::BTreeMap;
use std::path::PathBuf;

struct Dir(PathBuf);

impl std::ops::Deref for Dir {
    type Target = Path;
    fn deref(&self) -> &Path {
        &self.0
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn dir(label: &str) -> Dir {
    let root = std::env::temp_dir().join(format!(
        "grokhub-know-me-{label}-{}-{}",
        std::process::id(),
        grokhub_core::now_ms()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("dir");
    Dir(root)
}

fn scan() -> Scan {
    let mut counts = BTreeMap::new();
    counts.insert(Signal::Games, 3);
    counts.insert(Signal::Dev, 2);
    Scan {
        tags: vec![
            Tag {
                tag: "Stardew Valley".into(),
                signal: Signal::Games,
                topic: Some("Stardew Valley".into()),
            },
            Tag {
                tag: "Rust".into(),
                signal: Signal::Dev,
                topic: Some("Rust".into()),
            },
        ],
        counts,
    }
}

/// The interests closure, over a profile the test can read.
fn into(profile: &mut InterestProfile) -> impl FnOnce(Edit<'_>) -> Result<(), String> + '_ {
    move |f| {
        f(profile);
        Ok(())
    }
}

#[test]
fn know_me_starts_off_with_every_signal_listed() {
    let d = dir("off");
    let v = view(&d, 1);
    assert!(!v.on);
    let labels: Vec<&str> = v.signals.iter().map(|s| s.label.as_str()).collect();
    assert_eq!(
        labels,
        [
            "Game library",
            "Installed apps",
            "Dev tools",
            "File types",
            "Hardware",
            "Language and region"
        ]
    );
    assert_eq!(v.signals[0].id, "games");
    assert!(v.signals.iter().all(|s| !s.on));
    assert_eq!(v.last_scan, None);
    assert_eq!(v.tags, Vec::new());
}

#[test]
fn on_and_a_signal_switch_round_trip_through_the_file() {
    let d = dir("switch");
    set_on(&d, true).expect("on");
    set_signal(&d, "dev", false).expect("dev off");
    let v = view(&d, 1);
    assert!(v.on);
    let dev = v.signals.iter().find(|s| s.id == "dev").expect("dev");
    assert!(!dev.on);
    assert!(
        v.signals
            .iter()
            .find(|s| s.id == "games")
            .expect("games")
            .on
    );
    assert_eq!(
        set_signal(&d, "mail", true).unwrap_err(),
        "No signal called mail."
    );
    let text = std::fs::read_to_string(d.join("know-me.json")).expect("file");
    assert!(text.contains("\"on\": true"), "{text}");
}

#[test]
fn a_scan_keeps_tags_and_adds_their_topics_from_your_pc() {
    let d = dir("scan");
    set_on(&d, true).expect("on");
    let mut profile = InterestProfile::default();
    apply(&d, scan(), 1_000, into(&mut profile)).expect("apply");
    let v = view(&d, 61_000);
    assert_eq!(
        v.tags,
        [
            TagRow {
                tag: "Stardew Valley".into(),
                from: "from game library".into()
            },
            TagRow {
                tag: "Rust".into(),
                from: "from dev tools".into()
            },
        ]
    );
    assert_eq!(
        v.last_scan.as_deref(),
        Some("Last scan 1m ago: looked at game library 3, dev tools 2.")
    );
    assert_eq!(
        profile.find("Stardew Valley").map(|t| t.source),
        Some(InterestSource::KnowMe)
    );
}

#[test]
fn a_scan_while_off_keeps_nothing() {
    let d = dir("scan-off");
    let mut profile = InterestProfile::default();
    apply(&d, scan(), 1_000, into(&mut profile)).expect("apply");
    assert_eq!(view(&d, 1).tags, Vec::new());
    assert!(profile.find("Rust").is_none());
}

#[test]
fn delete_removes_the_tag_and_its_pc_topic_but_not_one_you_added() {
    let d = dir("delete");
    set_on(&d, true).expect("on");
    let mut profile = InterestProfile::default();
    profile.add("Rust", InterestSource::Explicit, 1);
    apply(&d, scan(), 1_000, into(&mut profile)).expect("apply");
    assert_eq!(
        delete(&d, "Stardew Valley", into(&mut profile)).expect("delete"),
        ""
    );
    delete(&d, "Rust", into(&mut profile)).expect("delete rust");
    assert_eq!(view(&d, 2_000).tags, Vec::new());
    assert!(profile.find("Stardew Valley").is_none());
    assert_eq!(
        profile.find("Rust").map(|t| t.source),
        Some(InterestSource::Explicit)
    );
    // A deleted tag never comes back on the next scan.
    apply(&d, scan(), 3_000, into(&mut profile)).expect("rescan");
    assert_eq!(view(&d, 3_000).tags, Vec::new());
    assert_eq!(
        delete(&d, "Stardew Valley", into(&mut profile)).expect("again"),
        "Stardew Valley is already gone."
    );
}

#[test]
fn wipe_turns_it_off_and_forgets_everything() {
    let d = dir("wipe");
    set_on(&d, true).expect("on");
    let mut profile = InterestProfile::default();
    apply(&d, scan(), 1_000, into(&mut profile)).expect("apply");
    assert_eq!(
        wipe(&d, into(&mut profile)).expect("wipe"),
        "Know me is off and everything it learned is gone."
    );
    let v = view(&d, 2_000);
    assert!(!v.on);
    assert_eq!(v.tags, Vec::new());
    assert!(profile.find("Rust").is_none());
}
