//! Spike-5a signal writers: what you say in chat, your corrections and edits,
//! and what you use become user-model nodes. Rules only, no model call, so
//! the same input always writes the same node. Approvals, denials and undos
//! are read from the span log by the harness (`harness::mindcheck`).
//!
//! Sources: `chat:<thread>#<turn>`, `span:<session>@<ms>`, `pulse:<date>`,
//! `scope:<grant>`, or `user`. Scratch writes nothing, and a line holding a
//! secret, a password, a PIN or a one-time code is skipped whole.

use crate::card_signals::{CardEvent, CardSignal};
use crate::pulse::{ledger_line, parse_ledger, LedgerEntry};
use crate::redact::redact_secrets;

use super::schema::{EdgeRel, Node, NodeDraft, NodeType};
use super::store::AmrStore;
use super::write::{hashed_id, line_id, node_type_for, sensitivity_for};
use super::AmrError;

/// Words that mark a line as holding a credential. The whole line is skipped.
const CREDENTIAL_WORDS: &[&str] = &[
    "password", "passwd", "passcode", "passphrase", "pin", "2fa", "otp", "one-time code", "verification code",
    "security code", "auth code", "login code", "mfa", "totp",
];

/// What a signal writer did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Noted {
    /// A new node file.
    New(String),
    /// The node was already there; nothing new was written.
    Known(String),
    /// Nothing was written, and why.
    Skipped(&'static str),
}

impl Noted {
    pub fn id(&self) -> Option<&str> {
        match self {
            Self::New(id) | Self::Known(id) => Some(id),
            Self::Skipped(_) => None,
        }
    }
}

/// Why a line must never become a node, or `None` when it may.
pub fn unsafe_to_learn(text: &str) -> Option<&'static str> {
    if redact_secrets(text) != text {
        return Some("holds a secret");
    }
    let lower = text.to_ascii_lowercase();
    let words: Vec<&str> = lower
        .split(|c: char| !(c.is_ascii_alphanumeric() || c == '-'))
        .filter(|w| !w.is_empty())
        .collect();
    let hit = CREDENTIAL_WORDS.iter().any(|w| {
        if w.contains(' ') {
            lower.contains(w)
        } else {
            words.contains(w)
        }
    });
    if hit {
        return Some("holds a credential");
    }
    let code_word = words.iter().any(|w| matches!(*w, "code" | "codes"));
    let digits = words.iter().any(|w| (6..=8).contains(&w.len()) && w.bytes().all(|b| b.is_ascii_digit()));
    if code_word && digits {
        return Some("holds a one-time code");
    }
    None
}

/// One chat line to note, from the turn it came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChatLine<'a> {
    pub thread: &'a str,
    pub turn: u32,
    pub text: &'a str,
}

impl ChatLine<'_> {
    pub fn source(&self) -> String {
        format!("chat:{}#{}", self.thread, self.turn)
    }
}

/// `routine` for a repeating time, `need` for an ask, else
/// [`node_type_for`] (`preference` or `fact`).
pub fn user_model_type(text: &str) -> NodeType {
    let lower = text.to_ascii_lowercase();
    let need = ["i need ", "i have to ", "remind me", "i must "].iter().any(|p| lower.starts_with(p) || lower.contains(&format!(" {p}")));
    if need {
        return NodeType::Need;
    }
    let routine = [
        "every ", "each morning", "each day", "daily", "weekly", "monthly", "standup", "stand-up", "on mondays",
        "on weekdays", "each week", "usually at",
    ]
    .iter()
    .any(|p| lower.contains(p));
    if routine {
        return NodeType::Routine;
    }
    node_type_for(text)
}

/// A correction found by rules: the statement as it should read, plus the
/// value it replaces when the user named one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Correction {
    pub fixed: String,
    pub old_value: Option<String>,
}

/// "no, I meant …", "I meant …", "actually, …", and "<fixed>, not <old>".
pub fn detect_correction(text: &str) -> Option<Correction> {
    let trimmed = text.trim().trim_end_matches(['.', '!']);
    let lower = trimmed.to_ascii_lowercase();
    for prefix in ["no, i meant ", "no i meant ", "i meant ", "actually, ", "actually "] {
        if lower.starts_with(prefix) {
            let rest = trimmed[prefix.len()..].trim();
            return (!rest.is_empty()).then(|| split_not(rest));
        }
    }
    if let Some(idx) = lower.rfind(", not ") {
        let fixed = trimmed[..idx].trim();
        let old = trimmed[idx + ", not ".len()..].trim();
        if !fixed.is_empty() && !old.is_empty() {
            return Some(Correction { fixed: fixed.to_string(), old_value: Some(old.to_string()) });
        }
    }
    None
}

fn split_not(rest: &str) -> Correction {
    let lower = rest.to_ascii_lowercase();
    match lower.rfind(", not ") {
        Some(idx) => Correction {
            fixed: rest[..idx].trim().to_string(),
            old_value: Some(rest[idx + ", not ".len()..].trim().to_string()),
        },
        None => Correction { fixed: rest.to_string(), old_value: None },
    }
}

/// Note one chat line. A correction becomes a new node with a `supersedes`
/// edge to the node it fixes (or `contradicts` edges when two fit equally).
pub fn note_chat(store: &AmrStore, line: &ChatLine<'_>, now_ms: u64) -> Result<Noted, AmrError> {
    if store.is_scratch() {
        return Err(AmrError::Scratch);
    }
    let text = line.text.trim();
    if let Some(why) = unsafe_to_learn(text) {
        return Ok(Noted::Skipped(why));
    }
    if let Some(correction) = detect_correction(text) {
        return note_correction(store, &line.source(), &correction, now_ms);
    }
    write_line(store, text, &line.source(), &["chat"], 0.7, now_ms)
}

/// The user edited agent output from `before` to `after`. The edit is a new
/// node, and a node whose body was `before` is superseded by it.
pub fn note_user_edit(store: &AmrStore, line: &ChatLine<'_>, before: &str, after: &str, now_ms: u64) -> Result<Noted, AmrError> {
    if store.is_scratch() {
        return Err(AmrError::Scratch);
    }
    let after = after.trim();
    if after.is_empty() || after == before.trim() {
        return Ok(Noted::Skipped("no change"));
    }
    if let Some(why) = unsafe_to_learn(after) {
        return Ok(Noted::Skipped(why));
    }
    let noted = write_line(store, after, &line.source(), &["edit"], 0.9, now_ms)?;
    if let Some(new_id) = noted.id() {
        let (nodes, _) = store.recallable();
        let before = norm(before);
        for node in nodes.iter().filter(|n| n.id != new_id && norm(&n.body) == before) {
            store.link(new_id, &node.id, EdgeRel::Supersedes)?;
        }
    }
    Ok(noted)
}

fn note_correction(store: &AmrStore, source: &str, correction: &Correction, now_ms: u64) -> Result<Noted, AmrError> {
    if let Some(why) = unsafe_to_learn(&correction.fixed) {
        return Ok(Noted::Skipped(why));
    }
    let (nodes, _) = store.recallable();
    let targets = correction_targets(&nodes, correction);
    let noted = write_line(store, &correction.fixed, source, &["correction"], 0.9, now_ms)?;
    let Some(new_id) = noted.id() else {
        return Ok(noted);
    };
    let rel = if targets.len() == 1 { EdgeRel::Supersedes } else { EdgeRel::Contradicts };
    for target in targets.iter().filter(|id| id.as_str() != new_id) {
        store.link(new_id, target, rel)?;
    }
    Ok(noted)
}

/// The best-scoring nodes the correction is about: shared subject words,
/// plus a point for holding the old value as a whole word.
fn correction_targets(nodes: &[Node], correction: &Correction) -> Vec<String> {
    let fixed = words(&correction.fixed);
    let old: Vec<String> = correction.old_value.as_deref().map(words).unwrap_or_default();
    let subject: Vec<&String> = fixed.iter().filter(|w| w.len() > 2 && !STOP.contains(&w.as_str())).collect();
    let mut best = 0usize;
    let mut out = Vec::new();
    for node in nodes.iter().filter(|n| n.node_type != NodeType::MindPrior) {
        let body = words(&node.body);
        let shared = subject.iter().filter(|w| body.contains(w)).count();
        if shared == 0 {
            continue;
        }
        let has_old = !old.is_empty() && old.iter().all(|w| body.contains(w));
        let score = shared * 2 + usize::from(has_old);
        if score > best {
            best = score;
            out.clear();
        }
        if score == best {
            out.push(node.id.clone());
        }
    }
    out
}

const STOP: &[&str] = &["the", "and", "not", "but", "you", "your", "are", "was", "for", "with", "that", "this", "its", "it's"];

fn words(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|c: char| !(c.is_alphanumeric() || c == ':' || c == '\''))
        .map(|w| w.trim_matches(':').to_string())
        .filter(|w| !w.is_empty())
        .collect()
}

fn norm(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

/// Where a written node came from and how sure the writer is.
struct Meta<'a> {
    source: &'a str,
    tags: &'a [&'a str],
    confidence: f32,
}

fn write_line(store: &AmrStore, text: &str, source: &str, tags: &[&str], confidence: f32, now_ms: u64) -> Result<Noted, AmrError> {
    let meta = Meta { source, tags, confidence };
    write_node(store, line_id(text), user_model_type(text), text, &meta, now_ms)
}

fn write_node(store: &AmrStore, id: String, node_type: NodeType, text: &str, meta: &Meta<'_>, now_ms: u64) -> Result<Noted, AmrError> {
    let text = text.trim();
    if text.is_empty() || text.contains(['\n', '\r']) {
        return Ok(Noted::Skipped("not one line"));
    }
    let stamp = crate::oauth::unix_ms_to_rfc3339(now_ms);
    let draft = NodeDraft {
        id,
        node_type,
        created: stamp.clone(),
        updated: stamp,
        source: meta.source.to_string(),
        confidence: meta.confidence,
        tags: meta.tags.iter().map(|t| t.to_string()).collect(),
        body: format!("{text}\n"),
        sensitivity: sensitivity_for(text),
        consent_ref: String::new(),
    };
    match store.remember(&draft) {
        Ok(id) => Ok(Noted::New(id.as_str().to_string())),
        Err(AmrError::DuplicateId(id)) => Ok(Noted::Known(id)),
        Err(err) => Err(err),
    }
}

/// Copy each Pulse ledger line in `memory_md` into a `pulse:<date>` node
/// (body is the ledger line without its `- ` bullet). Ids come from the
/// line, so a rerun writes nothing new.
pub fn import_pulse_ledger(store: &AmrStore, memory_md: &str, now_ms: u64) -> Result<Vec<Noted>, AmrError> {
    if store.is_scratch() {
        return Err(AmrError::Scratch);
    }
    let mut out = Vec::new();
    for entry in parse_ledger(memory_md) {
        let full = ledger_line(&entry.date, &entry.title, entry.reason);
        let line = full.strip_prefix("- ").unwrap_or(&full);
        let id = hashed_id("pulse", &[line]);
        let source = format!("pulse:{}", entry.date);
        let meta = Meta { source: &source, tags: &["pulse", entry.reason.key()], confidence: 0.6 };
        out.push(write_node(store, id, NodeType::Preference, line, &meta, now_ms)?);
    }
    Ok(out)
}

/// The Pulse ledger read from both places while it moves: MEMORY.md lines
/// first, then `pulse:` nodes not already listed. Oldest first per source.
pub fn pulse_ledger_dual(store: &AmrStore, memory_md: &str) -> Vec<LedgerEntry> {
    let mut out = parse_ledger(memory_md);
    let (nodes, _) = store.recallable();
    let mut from_nodes: Vec<&Node> = nodes.iter().filter(|n| n.source.starts_with("pulse:")).collect();
    from_nodes.sort_by(|a, b| a.created.cmp(&b.created).then(a.id.cmp(&b.id)));
    for node in from_nodes {
        let bulleted: String = node.body.lines().map(|l| format!("- {}\n", l.trim())).collect();
        for entry in parse_ledger(&bulleted) {
            if !out.contains(&entry) {
                out.push(entry);
            }
        }
    }
    out
}

/// One taste signal from a card: hidden, rejected, ran or completed. Opens,
/// More/Less and plain dismisses stay in `card_signals.jsonl` only.
pub fn note_card_signal(store: &AmrStore, signal: &CardSignal) -> Result<Noted, AmrError> {
    let verb = match signal.event {
        CardEvent::Hidden => "hid",
        CardEvent::Rejected => "turned down",
        CardEvent::Ran => "ran",
        CardEvent::Completed => "finished",
        _ => return Ok(Noted::Skipped("not a taste signal")),
    };
    let group = signal.group.trim();
    if group.is_empty() {
        return Ok(Noted::Skipped("no card group"));
    }
    let text = format!("You {verb} {} cards ({group}).", signal.kind.replace('_', " "));
    let id = hashed_id("card", &[group, verb]);
    let source = format!("pulse:card:{}", signal.card_id);
    let meta = Meta { source: &source, tags: &["card"], confidence: 0.6 };
    write_node(store, id, NodeType::Preference, &text, &meta, signal.ts)
}

/// A skill run or an automation outcome, from its span. One node per name and
/// outcome, so repeats are `Known`.
pub fn note_usage(store: &AmrStore, what: Usage<'_>, span_ref: &str, now_ms: u64) -> Result<Noted, AmrError> {
    let (prefix, text) = match what {
        Usage::SkillRun { name } => ("skill", format!("You use the {name} skill.")),
        Usage::Automation { name, outcome } => ("auto", format!("Automation {name}: {outcome}.")),
    };
    if let Some(why) = unsafe_to_learn(&text) {
        return Ok(Noted::Skipped(why));
    }
    let id = hashed_id(prefix, &[&text]);
    let source = format!("span:{span_ref}");
    let meta = Meta { source: &source, tags: &[prefix], confidence: 0.5 };
    write_node(store, id, NodeType::Routine, &text, &meta, now_ms)
}

/// Usage that is not a chat line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Usage<'a> {
    SkillRun { name: &'a str },
    Automation { name: &'a str, outcome: &'a str },
}
