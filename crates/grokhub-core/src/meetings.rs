//! Live meetings: join/record consent, followable notes, in-meeting Talk,
//! and searchable transcripts. Local-first product surface (Order 976).
//!
//! Nothing here joins Meet/Zoom/Teams or opens a mic. The cabin stores what
//! the person allows, the notes they follow, Talk turns they send during a
//! live meeting, and speech lines they save. Search runs over those lines.

use serde::{Deserialize, Serialize};

use crate::{now_ms, uid};

/// Refuse start/join until the person allows joining.
pub const JOIN_OFF: &str = "Meeting join is off. Allow Join on the Meetings page, then try again.";
/// Refuse recording/transcript/Talk that needs capture until record is allowed.
pub const RECORD_OFF: &str =
    "Meeting recording is off. Allow Record on the Meetings page, then try again.";
/// Only one live meeting at a time.
pub const ALREADY_LIVE: &str = "A meeting is already live. End it first.";
/// No live meeting for a live-only action.
pub const NO_LIVE: &str = "No live meeting. Start one first.";
/// Unknown id.
pub const MISSING: &str = "That meeting is gone.";
/// Talk / speech with no words.
pub const EMPTY_WORDS: &str = "Talk needs some words.";

/// Explicit privacy grants. Both start off; only a user click turns them on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct MeetingConsent {
    /// May start or join a meeting session in the cabin.
    #[serde(default)]
    pub join: bool,
    /// May keep a transcript and accept Talk / speech lines.
    #[serde(default)]
    pub record: bool,
    /// Send xAI STT `redact=pii` and surface `redacted_text` in transcripts.
    #[serde(default)]
    pub redact_pii: bool,
}

/// Lifecycle of one meeting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MeetingStatus {
    Live,
    Ended,
}

impl MeetingStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Live => "live",
            Self::Ended => "ended",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Live => "Live",
            Self::Ended => "Ended",
        }
    }
}

/// What one transcript line is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LineKind {
    /// A live note the person (or notes follow) added.
    Note,
    /// An in-meeting Talk turn from the person to the agent.
    Talk,
    /// Captured or pasted speech attributed to a speaker.
    Speech,
}

impl LineKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Note => "note",
            Self::Talk => "talk",
            Self::Speech => "speech",
        }
    }
}

/// One searchable transcript / notes line.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptLine {
    pub id: String,
    pub at_ms: u64,
    pub kind: LineKind,
    /// Speaker label for speech; empty for note/talk.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub speaker: String,
    pub text: String,
}

impl TranscriptLine {
    pub fn new(kind: LineKind, text: &str, speaker: &str, at_ms: u64) -> Self {
        Self {
            id: uid("ml"),
            at_ms,
            kind,
            speaker: speaker.trim().to_string(),
            text: text.trim().to_string(),
        }
    }
}

/// One meeting the person followed.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Meeting {
    pub id: String,
    pub title: String,
    pub status: MeetingStatus,
    pub started_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ended_ms: Option<u64>,
    /// Latest live notes blob the person is following (also mirrored as Note lines).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub live_notes: String,
    #[serde(default)]
    pub transcript: Vec<TranscriptLine>,
}

impl Meeting {
    pub fn is_live(&self) -> bool {
        self.status == MeetingStatus::Live
    }
}

/// On-disk / in-memory store under the cabin config dir.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct MeetingsStore {
    #[serde(default)]
    pub consent: MeetingConsent,
    #[serde(default)]
    pub meetings: Vec<Meeting>,
    /// Id of the current live meeting, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub live_id: Option<String>,
}

/// A search hit in a meeting's transcript.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptHit {
    pub meeting_id: String,
    pub meeting_title: String,
    pub line_id: String,
    pub kind: LineKind,
    pub speaker: String,
    pub text: String,
    pub at_ms: u64,
}

/// Turn join consent on or off. Call only from a user click path.
pub fn set_join_consent(store: &mut MeetingsStore, on: bool) {
    store.consent.join = on;
}

/// Turn record consent on or off. Call only from a user click path.
pub fn set_record_consent(store: &mut MeetingsStore, on: bool) {
    store.consent.record = on;
}

/// Turn PII redaction on or off. Off by default; only a user click turns it on.
pub fn set_redact_pii(store: &mut MeetingsStore, on: bool) {
    store.consent.redact_pii = on;
}

/// Start a live meeting. Needs join consent. Recording/Talk need record too
/// later; start itself only requires join so the person can sit in without
/// capturing until they allow Record.
pub fn start_meeting<'a>(
    store: &'a mut MeetingsStore,
    title: &str,
    now: Option<u64>,
) -> Result<&'a Meeting, &'static str> {
    if !store.consent.join {
        return Err(JOIN_OFF);
    }
    if store.live_id.is_some() {
        return Err(ALREADY_LIVE);
    }
    let title = title.trim();
    let title = if title.is_empty() {
        "Untitled meeting".to_string()
    } else {
        title.to_string()
    };
    let started_ms = now.unwrap_or_else(now_ms);
    let meeting = Meeting {
        id: uid("mtg"),
        title,
        status: MeetingStatus::Live,
        started_ms,
        ended_ms: None,
        live_notes: String::new(),
        transcript: Vec::new(),
    };
    let id = meeting.id.clone();
    store.meetings.insert(0, meeting);
    store.live_id = Some(id.clone());
    store.meetings.iter().find(|m| m.id == id).ok_or(MISSING)
}

fn live_mut(store: &mut MeetingsStore) -> Result<&mut Meeting, &'static str> {
    let id = store.live_id.as_deref().ok_or(NO_LIVE)?;
    store
        .meetings
        .iter_mut()
        .find(|m| m.id == id)
        .ok_or(MISSING)
}

/// Replace the live notes the person is following. Needs record consent.
/// Appends a Note line when the text changed and is non-empty.
pub fn follow_notes<'a>(
    store: &'a mut MeetingsStore,
    notes: &str,
    now: Option<u64>,
) -> Result<&'a Meeting, &'static str> {
    if !store.consent.record {
        return Err(RECORD_OFF);
    }
    let at = now.unwrap_or_else(now_ms);
    let notes = notes.trim();
    let meeting = live_mut(store)?;
    if notes == meeting.live_notes {
        return Ok(meeting);
    }
    meeting.live_notes = notes.to_string();
    if !notes.is_empty() {
        meeting
            .transcript
            .push(TranscriptLine::new(LineKind::Note, notes, "", at));
    }
    Ok(meeting)
}

/// In-meeting Talk: the person's turn during a live meeting. Needs join + record.
pub fn talk<'a>(
    store: &'a mut MeetingsStore,
    text: &str,
    now: Option<u64>,
) -> Result<&'a Meeting, &'static str> {
    if !store.consent.join {
        return Err(JOIN_OFF);
    }
    if !store.consent.record {
        return Err(RECORD_OFF);
    }
    let text = text.trim();
    if text.is_empty() {
        return Err(EMPTY_WORDS);
    }
    let at = now.unwrap_or_else(now_ms);
    let meeting = live_mut(store)?;
    meeting
        .transcript
        .push(TranscriptLine::new(LineKind::Talk, text, "", at));
    Ok(meeting)
}

/// Append a speech line (participant said …). Needs record consent.
pub fn append_speech<'a>(
    store: &'a mut MeetingsStore,
    speaker: &str,
    text: &str,
    now: Option<u64>,
) -> Result<&'a Meeting, &'static str> {
    if !store.consent.record {
        return Err(RECORD_OFF);
    }
    let text = text.trim();
    if text.is_empty() {
        return Err(EMPTY_WORDS);
    }
    let at = now.unwrap_or_else(now_ms);
    let meeting = live_mut(store)?;
    meeting
        .transcript
        .push(TranscriptLine::new(LineKind::Speech, text, speaker, at));
    Ok(meeting)
}

/// End the live meeting.
pub fn end_meeting(store: &mut MeetingsStore, now: Option<u64>) -> Result<&Meeting, &'static str> {
    let id = store.live_id.clone().ok_or(NO_LIVE)?;
    let at = now.unwrap_or_else(now_ms);
    let meeting = store
        .meetings
        .iter_mut()
        .find(|m| m.id == id)
        .ok_or(MISSING)?;
    meeting.status = MeetingStatus::Ended;
    meeting.ended_ms = Some(at);
    store.live_id = None;
    Ok(meeting)
}

/// Case-insensitive search over every meeting's transcript. Blank finds nothing.
pub fn search_transcripts(store: &MeetingsStore, query: &str) -> Vec<TranscriptHit> {
    let needle = query.trim().to_lowercase();
    if needle.is_empty() {
        return Vec::new();
    }
    let mut hits = Vec::new();
    for meeting in &store.meetings {
        for line in &meeting.transcript {
            let hay = if line.speaker.is_empty() {
                line.text.to_lowercase()
            } else {
                format!("{} {}", line.speaker, line.text).to_lowercase()
            };
            if hay.contains(&needle) {
                hits.push(TranscriptHit {
                    meeting_id: meeting.id.clone(),
                    meeting_title: meeting.title.clone(),
                    line_id: line.id.clone(),
                    kind: line.kind,
                    speaker: line.speaker.clone(),
                    text: line.text.clone(),
                    at_ms: line.at_ms,
                });
            }
        }
    }
    hits
}

/// Look up one meeting by id.
pub fn get_meeting<'a>(store: &'a MeetingsStore, id: &str) -> Option<&'a Meeting> {
    store.meetings.iter().find(|m| m.id == id)
}

/// One post-transcript action item. `due` is empty when the line named none.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionItem {
    pub owner: String,
    pub action: String,
    pub due: String,
    pub source_quote: String,
}

const OWNER_SKIP: &[&str] = &[
    "a", "an", "the", "this", "that", "we", "they", "it", "yes", "no", "ok",
];

/// Pull `{owner, action, due, source_quote}` items from a finished transcript.
/// No lines matching the patterns yields an empty list, not a placeholder.
pub fn extract_action_items(lines: &[TranscriptLine]) -> Vec<ActionItem> {
    let mut items = Vec::new();
    for line in lines {
        if let Some(item) = parse_action_line(line) {
            items.push(item);
        }
    }
    items
}

fn parse_action_line(line: &TranscriptLine) -> Option<ActionItem> {
    let quote = line.text.trim();
    if quote.is_empty() || quote.ends_with('?') {
        return None;
    }
    if let Some(rest) = strip_prefix_ci(quote, "action:") {
        return labeled_action(rest, quote, line);
    }
    if let Some(rest) = strip_prefix_ci(quote, "action item:") {
        return labeled_action(rest, quote, line);
    }
    if let Some(rest) = strip_prefix_ci(quote, "i'll ") {
        let owner = if line.speaker.trim().is_empty() {
            "user".to_string()
        } else {
            line.speaker.trim().to_string()
        };
        return split_due(rest, owner, quote);
    }
    if let Some(rest) = strip_prefix_ci(quote, "i will ") {
        let owner = if line.speaker.trim().is_empty() {
            "user".to_string()
        } else {
            line.speaker.trim().to_string()
        };
        return split_due(rest, owner, quote);
    }
    will_or_to(quote)
}

fn labeled_action(rest: &str, quote: &str, line: &TranscriptLine) -> Option<ActionItem> {
    let rest = rest.trim().trim_end_matches('.');
    if rest.is_empty() {
        return None;
    }
    let (body, owner_paren, due_paren) = split_owner_due_paren(rest);
    let action = body.trim().trim_end_matches('.').trim();
    if action.is_empty() {
        return None;
    }
    let owner = owner_paren
        .or_else(|| {
            let s = line.speaker.trim();
            (!s.is_empty()).then(|| s.to_string())
        })
        .unwrap_or_else(|| "user".to_string());
    Some(ActionItem {
        owner,
        action: action.to_string(),
        due: due_paren.unwrap_or_default(),
        source_quote: quote.to_string(),
    })
}

fn split_owner_due_paren(rest: &str) -> (String, Option<String>, Option<String>) {
    let Some(open) = rest.rfind('(') else {
        return (rest.to_string(), None, None);
    };
    if !rest.ends_with(')') {
        return (rest.to_string(), None, None);
    }
    let inner = rest[open + 1..rest.len() - 1].trim();
    let body = rest[..open].trim().to_string();
    let mut owner = None;
    let mut due = None;
    for part in inner.split(',') {
        let part = part.trim();
        if let Some(v) = strip_prefix_ci(part, "owner:") {
            let v = v.trim();
            if !v.is_empty() {
                owner = Some(v.to_string());
            }
        } else if let Some(v) = strip_prefix_ci(part, "due:") {
            let v = v.trim();
            if !v.is_empty() {
                due = Some(v.to_string());
            }
        }
    }
    (body, owner, due)
}

fn will_or_to(quote: &str) -> Option<ActionItem> {
    let lower = quote.to_ascii_lowercase();
    let (owner_end, rest) = if let Some(i) = lower.find(" will ") {
        (i, &quote[i + 6..])
    } else {
        let i = lower.find(" to ")?;
        (i, &quote[i + 4..])
    };
    let owner = quote[..owner_end].trim();
    if !owner_ok(owner) {
        return None;
    }
    split_due(rest, owner.to_string(), quote)
}

fn owner_ok(owner: &str) -> bool {
    let owner = owner.trim();
    if owner.is_empty() || owner.contains(' ') || owner.len() > 32 {
        return false;
    }
    let first = owner.chars().next().unwrap_or(' ');
    if !first.is_ascii_alphabetic() {
        return false;
    }
    let low = owner.to_ascii_lowercase();
    !OWNER_SKIP.contains(&low.as_str())
}

fn split_due(rest: &str, owner: String, quote: &str) -> Option<ActionItem> {
    let rest = rest.trim().trim_end_matches('.');
    if rest.is_empty() {
        return None;
    }
    let lower = rest.to_ascii_lowercase();
    let (action, due) = if let Some(i) = lower.rfind(" by ") {
        (rest[..i].trim(), rest[i + 4..].trim())
    } else {
        (rest, "")
    };
    if action.is_empty() {
        return None;
    }
    Some(ActionItem {
        owner,
        action: action.to_string(),
        due: due.to_string(),
        source_quote: quote.to_string(),
    })
}

fn strip_prefix_ci<'a>(text: &'a str, prefix: &str) -> Option<&'a str> {
    let t = text.as_bytes();
    let p = prefix.as_bytes();
    if t.len() < p.len() {
        return None;
    }
    if t[..p.len()].eq_ignore_ascii_case(p) {
        Some(&text[p.len()..])
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ready() -> MeetingsStore {
        let mut store = MeetingsStore::default();
        set_join_consent(&mut store, true);
        set_record_consent(&mut store, true);
        store
    }

    #[test]
    fn start_needs_join_consent() {
        let mut store = MeetingsStore::default();
        assert_eq!(start_meeting(&mut store, "Standup", Some(1)), Err(JOIN_OFF));
        set_join_consent(&mut store, true);
        let id = {
            let m = start_meeting(&mut store, "Standup", Some(1)).unwrap();
            assert_eq!(m.title, "Standup");
            assert!(m.is_live());
            m.id.clone()
        };
        assert_eq!(store.live_id.as_deref(), Some(id.as_str()));
    }

    #[test]
    fn only_one_live_meeting() {
        let mut store = ready();
        start_meeting(&mut store, "A", Some(1)).unwrap();
        assert_eq!(start_meeting(&mut store, "B", Some(2)), Err(ALREADY_LIVE));
    }

    #[test]
    fn follow_notes_and_talk_need_record_and_live() {
        let mut store = MeetingsStore::default();
        set_join_consent(&mut store, true);
        start_meeting(&mut store, "A", Some(1)).unwrap();
        assert_eq!(follow_notes(&mut store, "hello", Some(2)), Err(RECORD_OFF));
        assert_eq!(talk(&mut store, "hi there", Some(3)), Err(RECORD_OFF));
        set_record_consent(&mut store, true);
        follow_notes(&mut store, "Ship beta Friday", Some(4)).unwrap();
        talk(&mut store, "What are the blockers?", Some(5)).unwrap();
        append_speech(&mut store, "Alex", "No blockers", Some(6)).unwrap();
        let m = store.meetings.first().unwrap();
        assert_eq!(m.live_notes, "Ship beta Friday");
        assert_eq!(m.transcript.len(), 3);
        assert_eq!(m.transcript[0].kind, LineKind::Note);
        assert_eq!(m.transcript[1].kind, LineKind::Talk);
        assert_eq!(m.transcript[2].speaker, "Alex");
    }

    #[test]
    fn end_then_search_transcripts() {
        let mut store = ready();
        start_meeting(&mut store, "Planning", Some(10)).unwrap();
        follow_notes(&mut store, "Decide on rollout", Some(11)).unwrap();
        talk(&mut store, "Can we ship tomorrow?", Some(12)).unwrap();
        append_speech(&mut store, "Sam", "Yes if CI is green", Some(13)).unwrap();
        end_meeting(&mut store, Some(14)).unwrap();
        assert!(store.live_id.is_none());
        assert_eq!(store.meetings[0].status, MeetingStatus::Ended);

        let hits = search_transcripts(&store, "ship");
        assert_eq!(hits.len(), 1);
        assert!(hits[0].text.contains("tomorrow"));
        let green = search_transcripts(&store, "green");
        assert_eq!(green.len(), 1);
        assert_eq!(green[0].speaker, "Sam");
        assert!(search_transcripts(&store, "  ").is_empty());
        assert!(search_transcripts(&store, "zzz-nope").is_empty());
    }

    #[test]
    fn redact_pii_starts_off_and_flips() {
        let mut store = MeetingsStore::default();
        assert!(!store.consent.redact_pii);
        set_redact_pii(&mut store, true);
        assert!(store.consent.redact_pii);
        set_redact_pii(&mut store, false);
        assert!(!store.consent.redact_pii);
    }

    #[test]
    fn empty_title_becomes_untitled() {
        let mut store = ready();
        let m = start_meeting(&mut store, "   ", Some(1)).unwrap();
        assert_eq!(m.title, "Untitled meeting");
    }

    fn speech(speaker: &str, text: &str) -> TranscriptLine {
        TranscriptLine::new(LineKind::Speech, text, speaker, 1)
    }

    #[test]
    fn extract_action_items_from_transcript_lines() {
        let lines = [
            speech("host", "user will file the bug by Friday"),
            speech(
                "host",
                "Action: ship the beta (owner: user, due: 2026-12-01)",
            ),
            speech("user", "I'll send the notes by EOD"),
            speech("host", "host to merge the PR"),
            speech("host", "Yes if CI is green"),
            speech("host", "Can we ship tomorrow?"),
        ];
        let items = extract_action_items(&lines);
        assert_eq!(
            items,
            vec![
                ActionItem {
                    owner: "user".into(),
                    action: "file the bug".into(),
                    due: "Friday".into(),
                    source_quote: "user will file the bug by Friday".into(),
                },
                ActionItem {
                    owner: "user".into(),
                    action: "ship the beta".into(),
                    due: "2026-12-01".into(),
                    source_quote: "Action: ship the beta (owner: user, due: 2026-12-01)".into(),
                },
                ActionItem {
                    owner: "user".into(),
                    action: "send the notes".into(),
                    due: "EOD".into(),
                    source_quote: "I'll send the notes by EOD".into(),
                },
                ActionItem {
                    owner: "host".into(),
                    action: "merge the PR".into(),
                    due: "".into(),
                    source_quote: "host to merge the PR".into(),
                },
            ]
        );
    }

    #[test]
    fn extract_action_items_empty_when_none() {
        let lines = [
            speech("host", "No blockers"),
            TranscriptLine::new(LineKind::Note, "Decide on rollout", "", 2),
        ];
        assert!(extract_action_items(&lines).is_empty());
        assert!(extract_action_items(&[]).is_empty());
    }

    #[test]
    fn legacy_empty_store_deserializes() {
        let store: MeetingsStore = serde_json::from_str("{}").unwrap();
        assert!(!store.consent.join);
        assert!(!store.consent.record);
        assert!(!store.consent.redact_pii);
        assert!(store.meetings.is_empty());
        assert!(store.live_id.is_none());
    }
}
