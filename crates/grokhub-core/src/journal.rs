//! Decision backlog / experiment journal — local-first open questions.
//!
//! A journal entry is not a workboard task. It captures what is uncertain,
//! which options exist, what evidence would change the call, and the cheapest
//! next experiment. Promote into a Workboard Todo when an experiment or
//! implementation is chosen (Mission v1 exists; promote-to-mission is not wired).

use serde::{Deserialize, Serialize};

use crate::{now_ms, uid, BoardCard};

/// Lifecycle of one open question.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JournalStatus {
    Open,
    Investigating,
    ReadyToDecide,
    Decided,
    Deferred,
    Abandoned,
}

impl JournalStatus {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "open" => Some(Self::Open),
            "investigating" | "investigate" => Some(Self::Investigating),
            "ready_to_decide" | "ready-to-decide" | "ready" => Some(Self::ReadyToDecide),
            "decided" | "decide" => Some(Self::Decided),
            "deferred" | "defer" => Some(Self::Deferred),
            "abandoned" | "abandon" => Some(Self::Abandoned),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Investigating => "investigating",
            Self::ReadyToDecide => "ready_to_decide",
            Self::Decided => "decided",
            Self::Deferred => "deferred",
            Self::Abandoned => "abandoned",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Open => "Open",
            Self::Investigating => "Investigating",
            Self::ReadyToDecide => "Ready to decide",
            Self::Decided => "Decided",
            Self::Deferred => "Deferred",
            Self::Abandoned => "Abandoned",
        }
    }

    /// Still on the active list (not archived history).
    pub fn is_active(self) -> bool {
        !matches!(self, Self::Decided | Self::Abandoned)
    }
}

/// One viable choice on a decision, including "do nothing."
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct JournalOption {
    pub id: String,
    pub label: String,
}

impl JournalOption {
    pub fn new(label: &str) -> Self {
        Self {
            id: uid("opt"),
            label: label.trim().to_string(),
        }
    }
}

/// Kind of evidence attached to a decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceKind {
    Url,
    Chat,
    Span,
    Note,
}

impl EvidenceKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Url => "url",
            Self::Chat => "chat",
            Self::Span => "span",
            Self::Note => "note",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "url" | "link" | "http" | "https" => Some(Self::Url),
            "chat" | "thread" => Some(Self::Chat),
            "span" | "run" => Some(Self::Span),
            "note" | "text" => Some(Self::Note),
            _ => None,
        }
    }
}

/// A link to a run, note, chat, or research URL.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EvidenceLink {
    pub id: String,
    pub kind: EvidenceKind,
    /// URL, chat id, span id, or free-text note.
    #[serde(rename = "ref")]
    pub reference: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub label: String,
}

impl EvidenceLink {
    pub fn new(kind: EvidenceKind, reference: &str, label: &str) -> Self {
        Self {
            id: uid("ev"),
            kind,
            reference: reference.trim().to_string(),
            label: label.trim().to_string(),
        }
    }
}

/// Plain-language leaning (no forced confidence number).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct JournalLeaning {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub option_id: String,
    /// Free text, e.g. "leaning yes" or "low confidence".
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub note: String,
}

/// What was chosen (or that nothing was chosen) and what was learned.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct JournalOutcome {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub chosen_option_id: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rejected_option_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub learned: String,
}

/// One decision / experiment journal record.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct JournalEntry {
    pub id: String,
    /// Single decision-shaped sentence.
    pub question: String,
    /// Why it matters now.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub context: String,
    #[serde(default)]
    pub options: Vec<JournalOption>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub leaning: Option<JournalLeaning>,
    /// Facts that could change the choice.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unknowns: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub evidence: Vec<EvidenceLink>,
    /// Smallest useful next test.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub next_experiment: String,
    /// When to revisit or decide (unix ms). None = no nudge.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trigger_ms: Option<u64>,
    /// User dismissed the quiet due banner for this trigger.
    #[serde(default, skip_serializing_if = "is_false")]
    pub nudge_dismissed: bool,
    pub status: JournalStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcome: Option<JournalOutcome>,
    /// Workboard card filed when promoted (promote-to-mission is not wired).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub board_id: Option<String>,
    /// Optional chat that spawned this decision.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<String>,
    /// Append-only audit lines so removals do not erase history.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub history: Vec<String>,
    pub created_ms: u64,
    pub updated_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decided_ms: Option<u64>,
}

fn is_false(v: &bool) -> bool {
    !*v
}

impl JournalEntry {
    pub fn new(question: &str) -> Self {
        let now = now_ms();
        let q = question.trim().to_string();
        Self {
            id: uid("dec"),
            question: q.clone(),
            context: String::new(),
            options: vec![
                JournalOption::new("Do nothing"),
                JournalOption::new("Try a small experiment"),
            ],
            leaning: None,
            unknowns: Vec::new(),
            evidence: Vec::new(),
            next_experiment: String::new(),
            trigger_ms: None,
            nudge_dismissed: false,
            status: JournalStatus::Open,
            outcome: None,
            board_id: None,
            thread_id: None,
            history: vec![format!("created: {q}")],
            created_ms: now,
            updated_ms: now,
            decided_ms: None,
        }
    }

    fn touch(&mut self) {
        self.updated_ms = now_ms();
    }

    fn note(&mut self, line: impl Into<String>) {
        self.history.push(line.into());
        if self.history.len() > 200 {
            let drop = self.history.len() - 200;
            self.history.drain(0..drop);
        }
    }
}

/// Create a new open decision. Question must be non-empty.
pub fn create_entry(entries: &mut Vec<JournalEntry>, question: &str) -> Option<String> {
    let q = question.trim();
    if q.is_empty() {
        return None;
    }
    let entry = JournalEntry::new(q);
    let id = entry.id.clone();
    entries.insert(0, entry);
    Some(id)
}

/// Edit the question / context / next experiment / unknowns / trigger.
pub fn edit_entry(
    entries: &mut [JournalEntry],
    id: &str,
    question: Option<&str>,
    context: Option<&str>,
    next_experiment: Option<&str>,
    unknowns: Option<Vec<String>>,
    trigger_ms: Option<Option<u64>>,
) -> bool {
    let Some(e) = entries.iter_mut().find(|e| e.id == id) else {
        return false;
    };
    if let Some(q) = question {
        let q = q.trim();
        if !q.is_empty() && q != e.question {
            e.note(format!("question → {q}"));
            e.question = q.to_string();
        }
    }
    if let Some(c) = context {
        let c = c.trim().to_string();
        if c != e.context {
            e.note("context updated");
            e.context = c;
        }
    }
    if let Some(n) = next_experiment {
        let n = n.trim().to_string();
        if n != e.next_experiment {
            e.note("next experiment updated");
            e.next_experiment = n;
        }
    }
    if let Some(u) = unknowns {
        e.unknowns = u
            .into_iter()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        e.note("unknowns updated");
    }
    if let Some(t) = trigger_ms {
        e.trigger_ms = t;
        e.nudge_dismissed = false;
        e.note(match t {
            Some(ms) => format!("trigger set {ms}"),
            None => "trigger cleared".into(),
        });
    }
    e.touch();
    true
}

/// Substring search over question, context, options, experiment, unknowns.
pub fn search_entries<'a>(entries: &'a [JournalEntry], query: &str) -> Vec<&'a JournalEntry> {
    let q = query.trim().to_ascii_lowercase();
    if q.is_empty() {
        return entries.iter().collect();
    }
    entries
        .iter()
        .filter(|e| {
            e.question.to_ascii_lowercase().contains(&q)
                || e.context.to_ascii_lowercase().contains(&q)
                || e.next_experiment.to_ascii_lowercase().contains(&q)
                || e.unknowns
                    .iter()
                    .any(|u| u.to_ascii_lowercase().contains(&q))
                || e.options
                    .iter()
                    .any(|o| o.label.to_ascii_lowercase().contains(&q))
                || e.evidence.iter().any(|ev| {
                    ev.reference.to_ascii_lowercase().contains(&q)
                        || ev.label.to_ascii_lowercase().contains(&q)
                })
                || e.status.as_str().contains(&q)
                || e.status.label().to_ascii_lowercase().contains(&q)
        })
        .collect()
}

pub fn set_status(entries: &mut [JournalEntry], id: &str, status: JournalStatus) -> bool {
    let Some(e) = entries.iter_mut().find(|e| e.id == id) else {
        return false;
    };
    if e.status == status {
        return true;
    }
    e.note(format!(
        "status {} → {}",
        e.status.as_str(),
        status.as_str()
    ));
    e.status = status;
    if matches!(status, JournalStatus::Decided) {
        e.decided_ms = Some(now_ms());
    }
    e.touch();
    true
}

pub fn defer_entry(entries: &mut [JournalEntry], id: &str) -> bool {
    set_status(entries, id, JournalStatus::Deferred)
}

pub fn abandon_entry(entries: &mut [JournalEntry], id: &str) -> bool {
    set_status(entries, id, JournalStatus::Abandoned)
}

/// Mark decided with optional chosen option and learned note.
pub fn decide_entry(
    entries: &mut [JournalEntry],
    id: &str,
    chosen_option_id: Option<&str>,
    learned: &str,
) -> bool {
    let Some(e) = entries.iter_mut().find(|e| e.id == id) else {
        return false;
    };
    let chosen = chosen_option_id.unwrap_or("").trim().to_string();
    let rejected: Vec<String> = e
        .options
        .iter()
        .filter(|o| !chosen.is_empty() && o.id != chosen)
        .map(|o| o.id.clone())
        .collect();
    e.outcome = Some(JournalOutcome {
        chosen_option_id: chosen.clone(),
        rejected_option_ids: rejected,
        learned: learned.trim().to_string(),
    });
    e.status = JournalStatus::Decided;
    e.decided_ms = Some(now_ms());
    e.note(if chosen.is_empty() {
        "decided: no option chosen".into()
    } else {
        format!("decided: option {chosen}")
    });
    e.touch();
    true
}

pub fn add_option(entries: &mut [JournalEntry], id: &str, label: &str) -> Option<String> {
    let label = label.trim();
    if label.is_empty() {
        return None;
    }
    let e = entries.iter_mut().find(|e| e.id == id)?;
    if e.options.len() >= 5 {
        return None;
    }
    let opt = JournalOption::new(label);
    let oid = opt.id.clone();
    e.note(format!("option added: {label}"));
    e.options.push(opt);
    e.touch();
    Some(oid)
}

pub fn remove_option(entries: &mut [JournalEntry], id: &str, option_id: &str) -> bool {
    let Some(e) = entries.iter_mut().find(|e| e.id == id) else {
        return false;
    };
    let Some(pos) = e.options.iter().position(|o| o.id == option_id) else {
        return false;
    };
    if e.options.len() <= 1 {
        return false;
    }
    let removed = e.options.remove(pos);
    e.note(format!("option removed: {}", removed.label));
    if let Some(lean) = e.leaning.as_mut() {
        if lean.option_id == option_id {
            lean.option_id.clear();
        }
    }
    e.touch();
    true
}

pub fn add_evidence(
    entries: &mut [JournalEntry],
    id: &str,
    kind: EvidenceKind,
    reference: &str,
    label: &str,
) -> Option<String> {
    let reference = reference.trim();
    if reference.is_empty() {
        return None;
    }
    let e = entries.iter_mut().find(|e| e.id == id)?;
    let ev = EvidenceLink::new(kind, reference, label);
    let eid = ev.id.clone();
    e.note(format!("evidence added: {} {reference}", kind.as_str()));
    e.evidence.push(ev);
    e.touch();
    Some(eid)
}

pub fn remove_evidence(entries: &mut [JournalEntry], id: &str, evidence_id: &str) -> bool {
    let Some(e) = entries.iter_mut().find(|e| e.id == id) else {
        return false;
    };
    let Some(pos) = e.evidence.iter().position(|ev| ev.id == evidence_id) else {
        return false;
    };
    let removed = e.evidence.remove(pos);
    e.note(format!(
        "evidence removed: {} {}",
        removed.kind.as_str(),
        removed.reference
    ));
    e.touch();
    true
}

pub fn set_leaning(entries: &mut [JournalEntry], id: &str, option_id: &str, note: &str) -> bool {
    let Some(e) = entries.iter_mut().find(|e| e.id == id) else {
        return false;
    };
    e.leaning = Some(JournalLeaning {
        option_id: option_id.trim().to_string(),
        note: note.trim().to_string(),
    });
    e.note("leaning updated");
    e.touch();
    true
}

/// File a Workboard Todo from this decision and link `board_id`.
/// Files a Workboard Todo; promote-to-mission is not wired.
/// Uses the question as the card title (decision questions are not work-verbs,
/// so `file_idea_todo` / `idea_todo_title` would fall back to a generic label).
pub fn promote_to_board(
    entries: &mut [JournalEntry],
    board: &mut Vec<BoardCard>,
    id: &str,
) -> Option<String> {
    let e = entries.iter_mut().find(|e| e.id == id)?;
    if let Some(existing) = e.board_id.as_ref() {
        if board.iter().any(|c| c.id == *existing) {
            return Some(existing.clone());
        }
    }
    let title: String = e.question.chars().take(80).collect();
    let detail = {
        let mut parts = Vec::new();
        if !e.context.trim().is_empty() {
            parts.push(e.context.trim().to_string());
        }
        if !e.next_experiment.trim().is_empty() {
            parts.push(format!("Next experiment: {}", e.next_experiment.trim()));
        }
        parts.push(format!("From journal {}", e.id));
        parts.join("\n\n")
    };
    // Prefer an existing open card with the same title.
    if let Some(existing) = board
        .iter()
        .find(|c| c.title.eq_ignore_ascii_case(&title) && c.status != crate::BoardStatus::Dismissed)
    {
        let board_id = existing.id.clone();
        e.board_id = Some(board_id.clone());
        e.note(format!("promoted to workboard {board_id}"));
        e.touch();
        return Some(board_id);
    }
    let mut card = BoardCard::new(&title, &detail, "");
    card.status = crate::BoardStatus::Todo;
    let board_id = card.id.clone();
    board.push(card);
    e.board_id = Some(board_id.clone());
    e.note(format!("promoted to workboard {board_id}"));
    e.touch();
    Some(board_id)
}

/// Quiet due nudge: active entries whose trigger has passed and were not dismissed.
pub fn due_nudges(entries: &[JournalEntry], now: u64) -> Vec<&JournalEntry> {
    entries
        .iter()
        .filter(|e| {
            e.status.is_active()
                && !e.nudge_dismissed
                && e.trigger_ms.map(|t| t <= now).unwrap_or(false)
        })
        .collect()
}

pub fn dismiss_nudge(entries: &mut [JournalEntry], id: &str) -> bool {
    let Some(e) = entries.iter_mut().find(|e| e.id == id) else {
        return false;
    };
    e.nudge_dismissed = true;
    e.note("nudge dismissed");
    e.touch();
    true
}

/// Attach a completed workboard card id back as evidence (mission → decision).
pub fn link_board_evidence(
    entries: &mut [JournalEntry],
    id: &str,
    board_card_id: &str,
    note: &str,
) -> Option<String> {
    let label = if note.trim().is_empty() {
        format!("workboard {board_card_id}")
    } else {
        note.trim().to_string()
    };
    add_evidence(
        entries,
        id,
        EvidenceKind::Note,
        &format!("board:{board_card_id}"),
        &label,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_edit_search_defer_decide_abandon() {
        let mut entries = Vec::new();
        let id = create_entry(&mut entries, "Should we ship Pulse digests?").expect("id");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].status, JournalStatus::Open);
        assert!(entries[0].options.iter().any(|o| o.label == "Do nothing"));

        assert!(edit_entry(
            &mut entries,
            &id,
            None,
            Some("Users ask for a digest."),
            Some("Ship a one-week A/B on beta."),
            Some(vec!["Will digests feel noisy?".into()]),
            Some(Some(1_700_000_000_000)),
        ));
        assert_eq!(entries[0].context, "Users ask for a digest.");
        assert_eq!(entries[0].trigger_ms, Some(1_700_000_000_000));

        let hits = search_entries(&entries, "digest");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].id, id);
        assert!(search_entries(&entries, "zzznomatch").is_empty());

        assert!(defer_entry(&mut entries, &id));
        assert_eq!(entries[0].status, JournalStatus::Deferred);

        assert!(set_status(&mut entries, &id, JournalStatus::ReadyToDecide));
        let opt = entries[0].options[1].id.clone();
        assert!(decide_entry(
            &mut entries,
            &id,
            Some(&opt),
            "A/B looked quiet enough."
        ));
        assert_eq!(entries[0].status, JournalStatus::Decided);
        assert_eq!(entries[0].outcome.as_ref().unwrap().chosen_option_id, opt);
        assert!(entries[0].decided_ms.is_some());

        let id2 = create_entry(&mut entries, "Another question?").expect("id2");
        assert!(abandon_entry(&mut entries, &id2));
        assert_eq!(
            entries.iter().find(|e| e.id == id2).unwrap().status,
            JournalStatus::Abandoned
        );
    }

    #[test]
    fn options_and_evidence_keep_history_on_remove() {
        let mut entries = Vec::new();
        let id = create_entry(&mut entries, "Pick a store?").unwrap();
        let oid = add_option(&mut entries, &id, "JSON file").unwrap();
        assert_eq!(entries[0].options.len(), 3);
        assert!(remove_option(&mut entries, &id, &oid));
        assert!(entries[0]
            .history
            .iter()
            .any(|h| h.contains("option removed: JSON file")));
        assert!(!entries[0].options.iter().any(|o| o.id == oid));

        let eid = add_evidence(
            &mut entries,
            &id,
            EvidenceKind::Url,
            "https://example.com/note",
            "research",
        )
        .unwrap();
        assert!(remove_evidence(&mut entries, &id, &eid));
        assert!(entries[0]
            .history
            .iter()
            .any(|h| h.contains("evidence removed")));
        assert!(entries[0].evidence.is_empty());
    }

    #[test]
    fn promote_to_board_links_and_survives_context() {
        let mut entries = Vec::new();
        let mut board = Vec::new();
        let id = create_entry(&mut entries, "Should we add Journal?").unwrap();
        edit_entry(
            &mut entries,
            &id,
            None,
            Some("Open questions pile up."),
            Some("Ship local JSON store."),
            None,
            None,
        );
        let board_id = promote_to_board(&mut entries, &mut board, &id).unwrap();
        assert_eq!(entries[0].board_id.as_deref(), Some(board_id.as_str()));
        let card = board.iter().find(|c| c.id == board_id).unwrap();
        assert!(card.title.contains("Journal") || card.title.contains("add"));
        assert!(
            card.detail.contains("Open questions")
                || card.detail.contains("Next experiment")
                || card.detail.contains("journal"),
            "context should survive on the card: {}",
            card.detail
        );
        // Second promote returns the same card.
        let again = promote_to_board(&mut entries, &mut board, &id).unwrap();
        assert_eq!(again, board_id);
        assert_eq!(board.len(), 1);
    }

    #[test]
    fn due_nudge_is_quiet_and_dismissible() {
        let mut entries = Vec::new();
        let id = create_entry(&mut entries, "Revisit auth?").unwrap();
        edit_entry(&mut entries, &id, None, None, None, None, Some(Some(100)));
        assert_eq!(due_nudges(&entries, 99).len(), 0);
        assert_eq!(due_nudges(&entries, 100).len(), 1);
        assert!(dismiss_nudge(&mut entries, &id));
        assert!(due_nudges(&entries, 200).is_empty());
    }

    #[test]
    fn json_round_trip_preserves_fields() {
        let mut entries = Vec::new();
        let id = create_entry(&mut entries, "Keep camelCase?").unwrap();
        edit_entry(
            &mut entries,
            &id,
            None,
            None,
            Some("Try a one-week beta ship."),
            None,
            None,
        );
        add_evidence(&mut entries, &id, EvidenceKind::Note, "span:abc", "run").unwrap();
        let s = serde_json::to_string_pretty(&entries).unwrap();
        assert!(s.contains("\"question\""));
        assert!(
            s.contains("\"nextExperiment\""),
            "camelCase nextExperiment on disk: {s}"
        );
        let back: Vec<JournalEntry> = serde_json::from_str(&s).unwrap();
        assert_eq!(back[0].question, "Keep camelCase?");
        assert_eq!(back[0].next_experiment, "Try a one-week beta ship.");
        assert_eq!(back[0].evidence[0].reference, "span:abc");
    }

    #[test]
    fn empty_question_rejected_and_option_cap() {
        let mut entries = Vec::new();
        assert!(create_entry(&mut entries, "   ").is_none());
        let id = create_entry(&mut entries, "Cap options?").unwrap();
        assert!(add_option(&mut entries, &id, "A").is_some());
        assert!(add_option(&mut entries, &id, "B").is_some());
        assert!(add_option(&mut entries, &id, "C").is_some());
        // started with 2 defaults + 3 = 5; sixth fails
        assert!(add_option(&mut entries, &id, "D").is_none());
        assert_eq!(entries[0].options.len(), 5);
    }

    #[test]
    fn link_board_evidence_attaches_note() {
        let mut entries = Vec::new();
        let id = create_entry(&mut entries, "Ship it?").unwrap();
        let eid = link_board_evidence(&mut entries, &id, "card_1", "done on beta").unwrap();
        let ev = entries[0].evidence.iter().find(|e| e.id == eid).unwrap();
        assert_eq!(ev.reference, "board:card_1");
        assert_eq!(ev.label, "done on beta");
    }
}
