//! Spike-3a (AMR M4): one turn's harness spans become one `trail` node.
//!
//! A trail is a summary, not a copy: tool names with counts, decisions,
//! `ui_changed` outcomes, the Spike-1a findings, the verify result, and the
//! span refs. It is built only from span fields that are already redacted
//! (`args_redacted` of a typing step holds `{"chars":N}`, never the text),
//! then passed through `redact_secrets` and `redact_held_secrets`. Raw spans
//! stay in `spans/<session>.jsonl`. A turn that touched a hard-class step
//! (a credential field is hard class `credentials`) is sealed as personal,
//! so with no key the write is `Paused` and nothing lands on disk.

use std::collections::BTreeMap;
use std::path::Path;

use grokhub_core::amr::{trail_id, AmrError, AmrStore, EdgeRel, NodeDraft, NodeType, Sensitivity};

use crate::harness::audit::audit_spans;
use crate::harness::hard::HardClass;
use crate::harness::span::{read_spans_tail, Span, REPLY_TOOL, VERIFY_TOOL};
use crate::harness::span_search::SPAN_SEARCH_LINES;

/// A trail body is at most this many characters.
pub const TRAIL_BODY_CAP: usize = 1_200;

/// Decisions in the order a trail lists them; anything else follows by name.
const DECISION_ORDER: &[&str] = &["allow", "approve", "park", "deny", "refuse", "skip"];

/// What [`write_trail`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrailWrite {
    pub id: String,
    /// False when this turn already had its trail (nothing new was written).
    pub new: bool,
    pub sealed: bool,
    /// Nodes from this chat learned during the turn, linked `learned_from_span`.
    pub learned: Vec<String>,
    /// Other nodes from this chat, linked `references`.
    pub references: Vec<String>,
}

fn is_typing(tool: &str) -> bool {
    tool.rsplit("__").next().unwrap_or(tool) == "type"
}

/// `chars` from a typing step's redacted args (`{"chars":4}`), else 0.
fn typed_chars(span: &Span) -> u64 {
    serde_json::from_str::<serde_json::Value>(&span.args_redacted)
        .ok()
        .and_then(|v| v.get("chars").and_then(|n| n.as_u64()))
        .unwrap_or(0)
}

/// The steps of one turn, in file order. The reply's claim is not a step.
fn turn_steps<'a>(spans: &'a [Span], chat_id: &str, turn: u32) -> Vec<&'a Span> {
    spans
        .iter()
        .filter(|s| s.chat_id == chat_id && s.turn == turn && s.tool != REPLY_TOOL)
        .collect()
}

/// The trail text for one turn, or `None` when the turn has no harness step.
/// Capped at [`TRAIL_BODY_CAP`]; the span ref list gives way first.
pub fn trail_body(spans: &[Span], chat_id: &str, turn: u32) -> Option<String> {
    let steps = turn_steps(spans, chat_id, turn);
    if steps.is_empty() {
        return None;
    }
    let mut tools: Vec<(&str, usize, u64)> = Vec::new();
    let mut decisions: BTreeMap<&str, usize> = BTreeMap::new();
    let (mut changed, mut still) = (0usize, 0usize);
    let mut verify: Option<bool> = None;
    for s in &steps {
        match tools.iter_mut().find(|t| t.0 == s.tool) {
            Some(t) => {
                t.1 += 1;
                t.2 += typed_chars(s);
            }
            None => tools.push((s.tool.as_str(), 1, typed_chars(s))),
        }
        *decisions.entry(s.decision.as_str()).or_default() += 1;
        match s.ui_changed {
            Some(true) => changed += 1,
            Some(false) => still += 1,
            None => {}
        }
        if s.tool == VERIFY_TOOL {
            let pass = crate::harness::detect::verify_passed(s);
            verify = Some(verify.unwrap_or(true) && pass);
        }
    }
    let tool_line = tools
        .iter()
        .map(|(tool, n, chars)| {
            if is_typing(tool) {
                format!("{tool} x{n} ({chars} chars)")
            } else {
                format!("{tool} x{n}")
            }
        })
        .collect::<Vec<_>>()
        .join(", ");
    let mut decision_parts: Vec<String> = DECISION_ORDER
        .iter()
        .filter_map(|d| decisions.get(d).map(|n| format!("{d} {n}")))
        .collect();
    decision_parts.extend(
        decisions
            .iter()
            .filter(|(d, _)| !DECISION_ORDER.contains(d))
            .map(|(d, n)| format!("{d} {n}")),
    );
    let screen = if changed + still == 0 {
        "not measured".to_string()
    } else {
        format!("yes {changed}, no {still}")
    };
    let turn_spans: Vec<Span> = spans
        .iter()
        .filter(|s| s.chat_id == chat_id && s.turn == turn)
        .cloned()
        .collect();
    let mut findings: Vec<String> = Vec::new();
    for f in audit_spans(&turn_spans).findings() {
        if !findings.contains(&f.detector) {
            findings.push(f.detector.clone());
        }
    }
    let findings = if findings.is_empty() { "none".to_string() } else { findings.join(", ") };
    let verify = match verify {
        Some(true) => "pass",
        Some(false) => "fail",
        None => "none",
    };
    let head = format!(
        "Turn {turn}: {n} {word}.\nTools: {tool_line}\nDecisions: {decisions}\nScreen changed: {screen}\nFindings: {findings}\nVerify: {verify}\n",
        n = steps.len(),
        word = if steps.len() == 1 { "step" } else { "steps" },
        decisions = decision_parts.join(", "),
    );
    let head: String = head.chars().take(TRAIL_BODY_CAP).collect();
    let mut refs = String::from("Spans:");
    for (i, s) in steps.iter().enumerate() {
        let piece = format!(" {}{}", s.span_ref(), if i + 1 < steps.len() { "," } else { "" });
        let tail = format!(" +{} more", steps.len() - i);
        let len = head.chars().count() + refs.chars().count() + piece.chars().count() + tail.chars().count() + 1;
        if len > TRAIL_BODY_CAP {
            refs.push_str(&tail);
            break;
        }
        refs.push_str(&piece);
    }
    if head.chars().count() + refs.chars().count() + 1 > TRAIL_BODY_CAP {
        return Some(head);
    }
    Some(format!("{head}{refs}\n"))
}

/// The node to write for one turn: redacted body, sealed when a step was
/// hard class. `None` when the turn has no harness step.
pub fn trail_draft(spans: &[Span], chat_id: &str, turn: u32, held: &[String], now_ms: u64) -> Option<NodeDraft> {
    let body = trail_body(spans, chat_id, turn)?;
    let body = grokhub_core::redact_held_secrets(&grokhub_core::redact_secrets(&body), held);
    let steps = turn_steps(spans, chat_id, turn);
    let hard = steps.iter().any(|s| HardClass::parse(&s.approval_class).is_some());
    let stamp = grokhub_core::oauth::unix_ms_to_rfc3339(now_ms);
    Some(NodeDraft {
        id: trail_id(chat_id, turn),
        node_type: NodeType::Trail,
        created: stamp.clone(),
        updated: stamp,
        source: format!("span:{}", steps[0].span_ref()),
        confidence: 1.0,
        tags: vec!["trail".into(), format!("turn-{turn}")],
        body,
        sensitivity: if hard { Sensitivity::Personal } else { Sensitivity::Plain },
    })
}

/// At turn end: write this turn's trail and link it to the chat's other
/// nodes. Nodes from the chat created since the turn's first step were
/// learned from it (`fact → trail`, `learned_from_span`); older ones are
/// `references` from the trail. `Ok(None)` when the turn had no harness
/// step. Scratch is [`AmrError::Scratch`]; a sealed trail with no key is
/// [`AmrError::Paused`], and neither writes anything.
pub fn write_trail(
    store: &AmrStore,
    config_dir: &Path,
    chat_id: &str,
    turn: u32,
    held: &[String],
    now_ms: u64,
) -> Result<Option<TrailWrite>, AmrError> {
    if store.is_scratch() {
        return Err(AmrError::Scratch);
    }
    let (spans, _) = read_spans_tail(config_dir, chat_id, SPAN_SEARCH_LINES);
    let Some(draft) = trail_draft(&spans, chat_id, turn, held, now_ms) else {
        return Ok(None);
    };
    let sealed = draft.sensitivity.sealed();
    let new = match store.remember(&draft) {
        Ok(_) => true,
        Err(AmrError::DuplicateId(_)) => false,
        Err(err) => return Err(err),
    };
    let id = draft.id.clone();
    let first_ms = turn_steps(&spans, chat_id, turn).first().map_or(0, |s| s.ts_ms);
    let since = grokhub_core::oauth::unix_ms_to_rfc3339(first_ms - first_ms % 1000);
    let mut out = TrailWrite { id: id.clone(), new, sealed, learned: Vec::new(), references: Vec::new() };
    for node in store.nodes_from(&format!("chat:{chat_id}")) {
        let learned = matches!(node.node_type, NodeType::Fact | NodeType::Preference) && node.created >= since;
        if learned {
            store.link_once(&node.id, &id, EdgeRel::LearnedFromSpan)?;
            out.learned.push(node.id);
        } else {
            store.link_once(&id, &node.id, EdgeRel::References)?;
            out.references.push(node.id);
        }
    }
    Ok(Some(out))
}

/// A fact learned from a turn, written after that turn's trail: link it
/// `fact → trail` (`learned_from_span`). False when the turn has no trail.
pub fn link_learned(store: &AmrStore, fact_id: &str, chat_id: &str, turn: u32) -> Result<bool, AmrError> {
    let trail = trail_id(chat_id, turn);
    if !store.has_node(&trail) {
        return Ok(false);
    }
    store.link_once(fact_id, &trail, EdgeRel::LearnedFromSpan)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::access::AccessMode;
    use crate::harness::span::append_span;

    fn at(mut s: Span, ts: u64, turn: u32) -> Span {
        s.ts_ms = ts;
        s.chat_id = "chat-7".into();
        s.session_id = "chat-7".into();
        s.turn = turn;
        s
    }

    /// 2 clicks, 1 type, 1 park, 1 deny, 1 verify, then the reply's claim.
    fn six(turn: u32, base: u64) -> Vec<Span> {
        let click = |ui: bool| {
            Span::soft_allow("chat-7", "click", r#"{"x":40,"y":12}"#, "clicked", "click OK", AccessMode::Supervised, "grok_build")
                .with_ui_changed(Some(ui))
        };
        let typed = Span::soft_allow("chat-7", "type", r#"{"chars":4}"#, "typed", "grokhub-desktop type", AccessMode::Supervised, "grok_build");
        let park = Span::hard_park("chat-7", "run_command", r#"{"command":"rm -f draft.md"}"#, HardClass::Delete);
        let deny = Span::deny("chat-7", "send_email", "{}", "hard floor", "send");
        let mut verify = Span::deny("chat-7", VERIFY_TOOL, "{}", "pass", "soft");
        verify.decision = "allow".into();
        let reply = Span::reply("chat-7", "Clicked OK and typed the name. VERIFY_OK", &[]);
        vec![
            at(click(true), base + 1, turn),
            at(click(false), base + 2, turn),
            at(typed, base + 3, turn),
            at(park, base + 4, turn),
            at(deny, base + 5, turn),
            at(verify, base + 6, turn),
            at(reply, base + 7, turn),
        ]
    }

    fn vault_store(dir: &Path) -> AmrStore {
        AmrStore::at(dir.join("amr")).with_sealer(std::sync::Arc::new(crate::harness::LearnedVault::new(dir)))
    }

    fn node_files(dir: &Path) -> Vec<String> {
        let mut out: Vec<String> = std::fs::read_dir(dir.join("amr").join("nodes"))
            .map(|r| r.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect())
            .unwrap_or_default();
        out.sort();
        out
    }

    #[test]
    fn six_spans_make_one_trail_with_a_literal_body_and_a_second_turn_adds_one() {
        let dir = crate::harness::test_dir("trail-six");
        for s in six(3, 1000) {
            append_span(&dir, &s).unwrap();
        }
        let spans = crate::harness::read_spans(&dir, "chat-7").unwrap();
        assert_eq!(
            trail_body(&spans, "chat-7", 3).unwrap(),
            "Turn 3: 6 steps.\n\
             Tools: click x2, type x1 (4 chars), run_command x1, send_email x1, verify_script x1\n\
             Decisions: allow 4, park 1, deny 1\n\
             Screen changed: yes 1, no 1\n\
             Findings: none\n\
             Verify: pass\n\
             Spans: chat-7:1001, chat-7:1002, chat-7:1003, chat-7:1004, chat-7:1005, chat-7:1006\n"
        );
        let store = vault_store(&dir);
        store.init().unwrap();
        let w = write_trail(&store, &dir, "chat-7", 3, &[], 1_791_331_200_000).unwrap().unwrap();
        assert_eq!(w.id, trail_id("chat-7", 3));
        assert!(w.new && w.sealed, "a parked delete is hard class, so the trail is sealed");
        assert_eq!(node_files(&dir), vec![format!("{}.sealed", w.id)]);
        // The same turn again writes nothing new.
        let again = write_trail(&store, &dir, "chat-7", 3, &[], 1_791_331_300_000).unwrap().unwrap();
        assert!(!again.new);
        assert_eq!(node_files(&dir).len(), 1);
        // /recall reads the sealed trail with the key.
        let hits: Vec<String> = grokhub_core::amr::MemoryEngine::recall(&store, "run_command");
        assert_eq!(
            hits,
            vec![format!(
                "amr:{}: Tools: click x2, type x1 (4 chars), run_command x1, send_email x1, verify_script x1",
                w.id
            )]
        );

        // A second turn with a plain click is a second, plain node.
        let mut click = Span::soft_allow("chat-7", "click", "{}", "clicked", "click Save", AccessMode::Supervised, "grok_build");
        click = at(click.with_ui_changed(Some(true)), 2001, 4);
        append_span(&dir, &click).unwrap();
        let second = write_trail(&store, &dir, "chat-7", 4, &[], 1_791_331_400_000).unwrap().unwrap();
        assert!(second.new && !second.sealed);
        assert_eq!(node_files(&dir), {
            let mut v = vec![format!("{}.sealed", w.id), format!("{}.md", second.id)];
            v.sort();
            v
        });
        let md = std::fs::read_to_string(dir.join("amr/nodes").join(format!("{}.md", second.id))).unwrap();
        assert_eq!(
            md,
            format!(
                "---\nid: {}\ntype: trail\ncreated: 2026-10-07T00:03:20Z\nupdated: 2026-10-07T00:03:20Z\nsource: span:chat-7:2001\nconfidence: 1\ntags: [trail, turn-4]\n---\nTurn 4: 1 step.\nTools: click x1\nDecisions: allow 1\nScreen changed: yes 1, no 0\nFindings: none\nVerify: none\nSpans: chat-7:2001\n",
                second.id
            )
        );
        // A turn with no harness step writes nothing.
        assert_eq!(write_trail(&store, &dir, "chat-7", 9, &[], 1).unwrap(), None);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_credential_field_keeps_only_the_length_and_seals_or_pauses() {
        let dir = crate::harness::test_dir("trail-cred");
        let pin = "4821";
        let typed = Span::hard_park("chat-7", "type", r#"{"chars":4}"#, HardClass::Credentials);
        let held = vec![pin.to_string()];
        append_span(&dir, &at(typed, 11, 1)).unwrap();
        append_span(&dir, &at(Span::reply("chat-7", "I typed 4821 into the PIN field.", &held), 12, 1)).unwrap();

        // No key store at all: Paused, and no file of either kind.
        let bare = AmrStore::at(dir.join("amr"));
        bare.init().unwrap();
        assert!(matches!(write_trail(&bare, &dir, "chat-7", 1, &held, 1), Err(AmrError::Paused(_))));
        assert!(node_files(&dir).is_empty());

        let store = vault_store(&dir);
        let w = write_trail(&store, &dir, "chat-7", 1, &held, 1).unwrap().unwrap();
        assert!(w.sealed);
        let hits = store.recall("chars");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].line, "Tools: type x1 (4 chars)");
        for entry in walk(&dir.join("amr")) {
            let bytes = std::fs::read(&entry).unwrap();
            assert!(
                !String::from_utf8_lossy(&bytes).contains(pin),
                "the typed PIN reached {}",
                entry.display()
            );
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    fn walk(root: &Path) -> Vec<std::path::PathBuf> {
        let mut out = Vec::new();
        let mut stack = vec![root.to_path_buf()];
        while let Some(p) = stack.pop() {
            for e in std::fs::read_dir(&p).into_iter().flatten().flatten() {
                let path = e.path();
                if path.is_dir() {
                    stack.push(path);
                } else {
                    out.push(path);
                }
            }
        }
        out
    }

    #[test]
    fn scratch_writes_no_trail_and_facts_link_learned_from_span() {
        let dir = crate::harness::test_dir("trail-links");
        let click = Span::soft_allow("chat-7", "click", "{}", "clicked", "click OK", AccessMode::Supervised, "grok_build");
        append_span(&dir, &at(click, 1_791_331_200_000, 2)).unwrap();
        let mut store = vault_store(&dir);
        store.init().unwrap();
        let old = |id: &str, created: &str| NodeDraft {
            id: id.into(),
            node_type: NodeType::Fact,
            created: created.into(),
            updated: created.into(),
            source: "chat:chat-7".into(),
            confidence: 0.7,
            tags: vec!["insight".into()],
            body: format!("{id} body\n"),
            sensitivity: Sensitivity::Plain,
        };
        store.remember(&old("fact-before", "2026-10-06T00:00:00Z")).unwrap();
        store.remember(&old("fact-during", "2026-10-07T00:00:00Z")).unwrap();

        store.set_scratch(true);
        assert_eq!(write_trail(&store, &dir, "chat-7", 2, &[], 1), Err(AmrError::Scratch));
        assert!(!store.has_node(&trail_id("chat-7", 2)));
        store.set_scratch(false);

        let w = write_trail(&store, &dir, "chat-7", 2, &[], 1_791_331_201_000).unwrap().unwrap();
        assert_eq!(w.learned, vec!["fact-during".to_string()]);
        assert_eq!(w.references, vec!["fact-before".to_string()]);
        let edges = store.edges().unwrap();
        assert_eq!(edges.len(), 2);
        assert_eq!(
            edges,
            vec![
                grokhub_core::amr::Edge { from: w.id.clone(), to: "fact-before".into(), rel: EdgeRel::References },
                grokhub_core::amr::Edge { from: "fact-during".into(), to: w.id.clone(), rel: EdgeRel::LearnedFromSpan },
            ]
        );
        // A fact written after the trail links once, however often it is asked.
        store.remember(&old("fact-after", "2026-10-07T00:00:05Z")).unwrap();
        assert!(link_learned(&store, "fact-after", "chat-7", 2).unwrap());
        assert!(!link_learned(&store, "fact-after", "chat-7", 2).unwrap());
        assert!(!link_learned(&store, "fact-after", "chat-7", 3).unwrap(), "turn 3 has no trail");
        assert_eq!(store.edges().unwrap().len(), 3);
        let _ = std::fs::remove_dir_all(dir);
    }
}
