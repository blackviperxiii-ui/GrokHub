//! Spike-5a acceptance tests for the user model (core side).

use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use super::*;
use crate::hub_sync::{build_hub_snapshot, HubMemoryFile};

const NOW: u64 = 1_791_374_400_000;

struct Tmp(PathBuf);

impl Tmp {
    fn new(label: &str) -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("grokhub-umodel-{label}-{}-{n}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        Self(path)
    }

    fn store(&self) -> AmrStore {
        let store = AmrStore::at(self.0.join("amr"));
        store.init().unwrap();
        store
    }
}

impl Drop for Tmp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct HexSealer;

impl Sealer for HexSealer {
    fn seal(&self, aad: &str, plain: &str) -> Result<String, String> {
        Ok(format!("hex:{}:{}", hex::encode(aad), hex::encode(plain)))
    }

    fn open(&self, aad: &str, sealed: &str) -> Result<String, String> {
        let rest = sealed.strip_prefix(&format!("hex:{}:", hex::encode(aad))).ok_or("wrong node")?;
        String::from_utf8(hex::decode(rest).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
    }
}

fn chat(text: &str, turn: u32) -> ChatLine<'_> {
    ChatLine { thread: "t-standup", turn, text }
}

/// Every file under `nodes/`, as text.
fn node_texts(store: &AmrStore) -> Vec<String> {
    let Ok(read) = fs::read_dir(store.root().join("nodes")) else {
        return Vec::new();
    };
    read.flatten().map(|e| fs::read_to_string(e.path()).unwrap()).collect()
}

#[test]
fn old_nodes_parse_and_new_fields_round_trip() {
    let old = "---\nid: pref-a\ntype: preference\ncreated: 2026-10-06T00:00:00Z\nupdated: 2026-10-06T00:00:00Z\nsource: user\nconfidence: 0.5\ntags: []\n---\nTabs.\n";
    let node = Node::from_markdown(old).unwrap();
    assert_eq!(node.consent_ref, "");
    assert_eq!(node.sensitivity, Sensitivity::Plain);
    assert_eq!(node.to_markdown(), old);
    let new = "---\nid: prior-x\ntype: mind_prior\ncreated: 2026-10-06T00:00:00Z\nupdated: 2026-10-06T00:00:00Z\nsource: scope:g-1\nconfidence: 0.6\ntags: []\nconsent_ref: g-1\nsensitivity: personal\n---\nAsk first.\n";
    let node = Node::from_markdown(new).unwrap();
    assert_eq!(node.node_type, NodeType::MindPrior);
    assert_eq!(node.consent_ref, "g-1");
    assert_eq!(node.sensitivity, Sensitivity::Personal);
    assert_eq!(node.to_markdown(), new);
    for (raw, ty) in [("routine", NodeType::Routine), ("need", NodeType::Need), ("mind_prior", NodeType::MindPrior)] {
        assert_eq!(NodeType::parse(raw).unwrap(), ty);
        assert_eq!(ty.as_str(), raw);
    }
}

#[test]
fn correction_supersedes_and_recall_returns_the_fix() {
    let tmp = Tmp::new("correct");
    let store = tmp.store();
    let first = note_chat(&store, &chat("My standup is at 9", 1), NOW).unwrap();
    let Noted::New(old_id) = first else { panic!("{first:?}") };
    assert_eq!(store.recall("standup")[0].line, "My standup is at 9");
    let fixed = note_chat(&store, &chat("my standup is 9:30, not 9", 2), NOW + 1).unwrap();
    let Noted::New(new_id) = fixed else { panic!("{fixed:?}") };
    let hits = store.recall("standup");
    assert_eq!(hits, vec![NodeHit { id: new_id.clone(), line: "my standup is 9:30".into() }]);
    assert_eq!(MemoryEngine::recall(&store, "standup"), vec![format!("amr:{new_id}: my standup is 9:30")]);
    assert_eq!(store.edges().unwrap(), vec![Edge { from: new_id.clone(), to: old_id.clone(), rel: EdgeRel::Supersedes }]);
    assert!(store.superseded_ids().contains(&old_id));
    let (nodes, _) = store.recallable();
    let fix = nodes.iter().find(|n| n.id == new_id).unwrap();
    assert_eq!(fix.node_type, NodeType::Routine);
    assert_eq!(fix.source, "chat:t-standup#2");
    assert_eq!(fix.tags, vec!["correction".to_string()]);
}

#[test]
fn correction_rules_read_the_usual_phrasings() {
    assert_eq!(
        detect_correction("No, I meant Thursday."),
        Some(Correction { fixed: "Thursday".into(), old_value: None })
    );
    assert_eq!(
        detect_correction("my standup is 9:30, not 9"),
        Some(Correction { fixed: "my standup is 9:30".into(), old_value: Some("9".into()) })
    );
    assert_eq!(
        detect_correction("actually, I take oat milk, not soy"),
        Some(Correction { fixed: "I take oat milk".into(), old_value: Some("soy".into()) })
    );
    assert_eq!(detect_correction("Book the 9:30 standup"), None);
}

#[test]
fn a_correction_two_nodes_fit_equally_contradicts_both() {
    let tmp = Tmp::new("contradict");
    let store = tmp.store();
    let a = note_chat(&store, &chat("Lunch is at noon on Monday", 1), NOW).unwrap();
    let b = note_chat(&store, &chat("Lunch is at noon on Friday", 2), NOW).unwrap();
    let fixed = note_chat(&store, &chat("lunch is at 1pm, not noon", 3), NOW).unwrap();
    let new_id = fixed.id().unwrap().to_string();
    let mut edges = store.edges().unwrap();
    edges.sort_by(|x, y| x.to.cmp(&y.to));
    let mut want = vec![
        Edge { from: new_id.clone(), to: a.id().unwrap().into(), rel: EdgeRel::Contradicts },
        Edge { from: new_id.clone(), to: b.id().unwrap().into(), rel: EdgeRel::Contradicts },
    ];
    want.sort_by(|x, y| x.to.cmp(&y.to));
    assert_eq!(edges, want);
    assert_eq!(store.recall("lunch").len(), 3, "contradicts keeps both for the user to sort out");
}

#[test]
fn user_edit_of_agent_output_supersedes_the_old_line() {
    let tmp = Tmp::new("edit-out");
    let store = tmp.store();
    let old = note_chat(&store, &chat("You prefer window seats", 1), NOW).unwrap();
    let new = note_user_edit(&store, &chat("", 4), "You prefer window seats", "You prefer aisle seats", NOW).unwrap();
    assert_eq!(
        store.edges().unwrap(),
        vec![Edge { from: new.id().unwrap().into(), to: old.id().unwrap().into(), rel: EdgeRel::Supersedes }]
    );
    assert_eq!(store.recall("seats")[0].line, "You prefer aisle seats");
    assert_eq!(store.recall("seats").len(), 1);
}

#[test]
fn forget_leaves_recall_the_index_and_the_hub_snapshot() {
    let tmp = Tmp::new("forget");
    let store = tmp.store();
    let keep = note_chat(&store, &chat("I like dark roast coffee", 1), NOW).unwrap();
    let gone = note_chat(&store, &chat("My boat is called Keel", 2), NOW).unwrap();
    let gone = gone.id().unwrap().to_string();
    let index = store.rebuild_index().unwrap();
    let mut both = vec![keep.id().unwrap().to_string(), gone.clone()];
    both.sort();
    assert_eq!(index.ids().unwrap(), both);
    assert_eq!(index.search("keel").unwrap(), vec![gone.clone()]);
    drop(index);
    let user_md = "# You\n- My boat is called Keel\n- I like dark roast coffee\n";
    let files = || vec![HubMemoryFile { name: "USER.md".into(), content: user_md.into(), updated_at: 1 }];

    forget_node(&store, &gone, UserForget::from_click(), NOW).unwrap();

    assert_eq!(store.recall("keel"), Vec::new());
    let index = AmrIndex::open(&store.index_path()).unwrap();
    assert_eq!(index.ids().unwrap(), vec![keep.id().unwrap().to_string()]);
    assert_eq!(index.search("keel").unwrap(), Vec::<String>::new());
    let snap = build_hub_snapshot("d1", "cabin", 9, vec![], serde_json::json!({}), vec![], vec![], strip_forgotten(&store, files()));
    assert_eq!(snap.memory_files[0].content, "# You\n- I like dark roast coffee\n");
    let json = serde_json::to_string(&snap).unwrap();
    assert!(!json.contains("Keel"), "{json}");
    let note = fs::read_to_string(store.root().join("dreams").join("forget-2026-10-07.md")).unwrap();
    assert_eq!(note, format!("- forgot {gone} (you asked)\n"));
    // A rebuild from files keeps it out too.
    let rebuilt = store.rebuild_index().unwrap();
    assert_eq!(rebuilt.ids().unwrap(), vec![keep.id().unwrap().to_string()]);
}

#[test]
fn scratch_chat_writes_no_node() {
    let tmp = Tmp::new("scratch");
    let mut store = tmp.store();
    store.set_scratch(true);
    for (turn, line) in ["I like dark roast coffee", "no, I meant light roast", "My standup is 9"].iter().enumerate() {
        assert_eq!(note_chat(&store, &chat(line, turn as u32), NOW), Err(AmrError::Scratch));
    }
    assert!(MemoryEngine::scratch(&store));
    assert_eq!(store.node_file_count(), 0);
    assert_eq!(import_pulse_ledger(&store, "- [2026-10-04] pulse: dismissed \"x\" reason=not-this", NOW), Err(AmrError::Scratch));
    assert_eq!(store.node_file_count(), 0);
}

#[test]
fn secrets_passwords_and_codes_never_reach_a_node() {
    let tmp = Tmp::new("secrets");
    let store = tmp.store().with_sealer(Arc::new(HexSealer));
    let fixture = [
        "Use sk-abcdefghijklmnopqrstuv for the xAI key",
        "my password is hunter2-boat",
        "The 2FA code is 482913",
        "verification code 771204 came by text",
        "my PIN is 4471",
    ];
    for (turn, line) in fixture.iter().enumerate() {
        let got = note_chat(&store, &chat(line, turn as u32), NOW).unwrap();
        assert!(matches!(got, Noted::Skipped(_)), "{line}: {got:?}");
    }
    note_chat(&store, &chat("I like dark roast coffee", 9), NOW).unwrap();
    let texts = node_texts(&store).join("\n");
    for needle in ["sk-abcdefghijklmnopqrstuv", "abcdefghijklmnopqrstuv", "hunter2", "482913", "771204", "4471"] {
        assert!(!texts.contains(needle), "{needle} leaked");
    }
    assert_eq!(store.node_file_count(), 1);
    assert_eq!(unsafe_to_learn("Book the 9:30 standup"), None);
    assert_eq!(unsafe_to_learn("Review code 4 PRs"), None);
}

#[test]
fn every_learned_row_has_a_source_link() {
    let tmp = Tmp::new("rows");
    let store = tmp.store();
    note_chat(&store, &chat("My standup is at 9", 1), NOW).unwrap();
    import_pulse_ledger(&store, "- [2026-10-04] pulse: dismissed \"tidy downloads\" reason=not-this\n", NOW + 1_000).unwrap();
    note_usage(&store, Usage::SkillRun { name: "inbox-zero" }, "s1:42", NOW + 2_000).unwrap();
    let (rows, locked) = memory_rows(&store);
    assert_eq!(locked, 0);
    assert_eq!(rows.len(), 3);
    assert!(rows.iter().all(|r| !r.source.target().is_empty()));
    assert_eq!(
        memory_text(&store),
        "- You use the inbox-zero skill. · why: [from what you approved, denied or undid](span:s1:42)\n\
         - [2026-10-04] pulse: dismissed \"tidy downloads\" reason=not-this · why: [from a card you reacted to](pulse:2026-10-04)\n\
         - My standup is at 9 · why: [you said it in chat (turn 1)](chat:t-standup#1)"
    );
    let empty = NodeDraft {
        id: "fact-x".into(),
        node_type: NodeType::Fact,
        created: "c".into(),
        updated: "u".into(),
        source: "  ".into(),
        confidence: 0.5,
        tags: vec![],
        body: "x\n".into(),
        sensitivity: Sensitivity::Plain,
        consent_ref: String::new(),
    };
    assert_eq!(store.remember(&empty), Err(AmrError::BadFrontmatter("empty source".into())));
}

#[test]
fn edit_writes_a_user_node_that_supersedes() {
    let tmp = Tmp::new("edit");
    let store = tmp.store();
    let old = note_chat(&store, &chat("I drink black tea", 1), NOW).unwrap();
    let old = old.id().unwrap().to_string();
    let new = edit_node(&store, &old, "I drink green tea", NOW + 5).unwrap();
    assert_eq!(store.edges().unwrap(), vec![Edge { from: new.clone(), to: old, rel: EdgeRel::Supersedes }]);
    let (rows, _) = memory_rows(&store);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].text, "I drink green tea");
    assert_eq!(rows[0].source, SourceLink::User);
    assert_eq!(rows[0].confidence, 1.0);
    assert_eq!(rows[0].line(), "- I drink green tea · why: [you wrote it](user)");
}

#[test]
fn no_keyring_pauses_a_personal_note_and_writes_no_file() {
    let tmp = Tmp::new("paused");
    let store = tmp.store();
    let got = note_chat(&store, &chat("Email me at viper@example.com", 1), NOW);
    assert_eq!(got, Err(AmrError::Paused("private memory has no key store".into())));
    assert_eq!(store.node_file_count(), 0);
    assert!(!store.index_path().exists());
}

#[test]
fn sealed_nodes_index_in_memory_only() {
    let tmp = Tmp::new("sealed-index");
    let store = tmp.store().with_sealer(Arc::new(HexSealer));
    let private = note_chat(&store, &chat("Email me at viper@example.com", 1), NOW).unwrap();
    note_chat(&store, &chat("I like dark roast coffee", 2), NOW).unwrap();
    let disk = store.rebuild_index().unwrap();
    assert_eq!(disk.search("viper").unwrap(), Vec::<String>::new());
    let raw = fs::read(store.index_path()).unwrap();
    assert!(!String::from_utf8_lossy(&raw).contains("example.com"));
    let unlocked = store.sealed_index().unwrap();
    assert_eq!(unlocked.search("viper").unwrap(), vec![private.id().unwrap().to_string()]);
    let (rows, _) = memory_rows(&store);
    let row = rows.iter().find(|r| r.id == private.id().unwrap()).unwrap();
    assert_eq!(row.sensitivity, Sensitivity::Personal);
}

#[test]
fn reflect_returns_a_diff_and_writes_nothing() {
    let tmp = Tmp::new("reflect");
    let store = tmp.store();
    note_chat(&store, &chat("My standup is at 9", 1), NOW).unwrap();
    note_chat(&store, &chat("my standup is 9:30, not 9", 2), NOW).unwrap();
    note_chat(&store, &chat("I prefer short replies", 3), NOW).unwrap();
    let user_md = "# You\n- My standup is at 9\n- I prefer short replies\n";
    let before = store.node_file_count();
    let diff = MemoryEngine::reflect(&store, user_md);
    assert_eq!(
        diff,
        ReflectDiff { add: vec!["my standup is 9:30".into()], remove: vec!["- My standup is at 9".into()] }
    );
    assert_eq!(diff.render(), "- - My standup is at 9\n+ my standup is 9:30");
    assert_eq!(store.node_file_count(), before);
}

#[test]
fn pulse_ledger_reads_from_memory_md_and_nodes() {
    let tmp = Tmp::new("pulse");
    let store = tmp.store();
    let old = "- [2026-10-03] pulse: dismissed \"tidy downloads\" reason=not-this\n";
    let first = import_pulse_ledger(&store, old, NOW).unwrap();
    assert!(matches!(first[0], Noted::New(_)));
    let again = import_pulse_ledger(&store, old, NOW).unwrap();
    assert!(matches!(again[0], Noted::Known(_)));
    let newer = "- [2026-10-05] pulse: liked \"weekly digest\" reason=liked\n";
    let both = pulse_ledger_dual(&store, newer);
    let titles: Vec<&str> = both.iter().map(|e| e.title.as_str()).collect();
    assert_eq!(titles, vec!["weekly digest", "tidy downloads"]);
    assert_eq!(pulse_ledger_dual(&store, old).len(), 1, "a line in both places counts once");
}

#[test]
fn card_signals_and_usage_are_deterministic() {
    let tmp = Tmp::new("usage");
    let store = tmp.store();
    let hid = crate::card_signals::CardSignal {
        ts: NOW,
        card_id: "c1".into(),
        kind: "automate_offer".into(),
        group: "offer:downloads".into(),
        source_id: String::new(),
        event: crate::card_signals::CardEvent::Hidden,
        after_open: None,
    };
    let first = note_card_signal(&store, &hid).unwrap();
    assert!(matches!(first, Noted::New(_)));
    assert_eq!(note_card_signal(&store, &hid).unwrap(), Noted::Known(first.id().unwrap().into()));
    let opened = crate::card_signals::CardSignal { event: crate::card_signals::CardEvent::Opened, ..hid.clone() };
    assert_eq!(note_card_signal(&store, &opened).unwrap(), Noted::Skipped("not a taste signal"));
    assert_eq!(store.recall("downloads")[0].line, "You hid automate offer cards (offer:downloads).");
    let ok = Usage::Automation { name: "nightly-backup", outcome: "done" };
    let a = note_usage(&store, ok, "s2:7", NOW).unwrap();
    assert_eq!(note_usage(&store, ok, "s2:9", NOW).unwrap(), Noted::Known(a.id().unwrap().into()));
}
