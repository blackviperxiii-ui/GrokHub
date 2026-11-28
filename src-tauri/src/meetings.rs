//! Meetings page (Order 976): the same `meetings.json` store as
//! `grokhub_core::meetings`, so consent, live notes, Talk, and search stay
//! local to this computer.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use grokhub_core::{
    append_speech, end_meeting, follow_notes, search_transcripts, set_join_consent,
    set_record_consent, set_redact_pii, start_meeting, talk, Meeting, MeetingStatus, MeetingsStore,
    SttRedact, TranscriptHit,
};
use serde::{Deserialize, Serialize};

const FILE: &str = "meetings.json";
const STORE_CAP: u64 = 32 * 1024 * 1024;

static STORE: Mutex<()> = Mutex::new(());

fn store() -> MutexGuard<'static, ()> {
    STORE.lock().unwrap_or_else(|err| err.into_inner())
}

fn path(config_dir: &Path) -> PathBuf {
    config_dir.join(FILE)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConsentView {
    pub join: bool,
    pub record: bool,
    pub redact_pii: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LineView {
    pub id: String,
    pub at_ms: u64,
    pub kind: String,
    pub speaker: String,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MeetingView {
    pub id: String,
    pub title: String,
    pub status: String,
    pub started_ms: u64,
    pub ended_ms: Option<u64>,
    pub live_notes: String,
    pub transcript: Vec<LineView>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HitView {
    pub meeting_id: String,
    pub meeting_title: String,
    pub line_id: String,
    pub kind: String,
    pub speaker: String,
    pub text: String,
    pub at_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Page {
    pub consent: ConsentView,
    pub live: Option<MeetingView>,
    pub past: Vec<MeetingView>,
    pub hits: Vec<HitView>,
    pub query: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum Act {
    SetJoin { on: bool },
    SetRecord { on: bool },
    SetRedactPii { on: bool },
    Start { title: String },
    FollowNotes { notes: String },
    Talk { text: String },
    Speech { speaker: String, text: String },
    End,
    Search { query: String },
}

fn line_view(line: &grokhub_core::TranscriptLine) -> LineView {
    LineView {
        id: line.id.clone(),
        at_ms: line.at_ms,
        kind: line.kind.as_str().to_string(),
        speaker: line.speaker.clone(),
        text: line.text.clone(),
    }
}

fn meeting_view(m: &Meeting) -> MeetingView {
    MeetingView {
        id: m.id.clone(),
        title: m.title.clone(),
        status: m.status.as_str().to_string(),
        started_ms: m.started_ms,
        ended_ms: m.ended_ms,
        live_notes: m.live_notes.clone(),
        transcript: m.transcript.iter().map(line_view).collect(),
    }
}

fn hit_view(h: TranscriptHit) -> HitView {
    HitView {
        meeting_id: h.meeting_id,
        meeting_title: h.meeting_title,
        line_id: h.line_id,
        kind: h.kind.as_str().to_string(),
        speaker: h.speaker,
        text: h.text,
        at_ms: h.at_ms,
    }
}

fn page_of(store: &MeetingsStore, query: &str) -> Page {
    let live = store
        .live_id
        .as_ref()
        .and_then(|id| store.meetings.iter().find(|m| &m.id == id))
        .map(meeting_view);
    let past: Vec<_> = store
        .meetings
        .iter()
        .filter(|m| m.status == MeetingStatus::Ended)
        .map(meeting_view)
        .collect();
    let hits = search_transcripts(store, query)
        .into_iter()
        .map(hit_view)
        .collect();
    Page {
        consent: ConsentView {
            join: store.consent.join,
            record: store.consent.record,
            redact_pii: store.consent.redact_pii,
        },
        live,
        past,
        hits,
        query: query.to_string(),
    }
}

fn load_unlocked(config_dir: &Path) -> Result<MeetingsStore, String> {
    let path = path(config_dir);
    if !path.is_file() {
        return Ok(MeetingsStore::default());
    }
    let len = std::fs::metadata(&path).map_err(|e| e.to_string())?.len();
    if len > STORE_CAP {
        return Err(format!("{FILE} is too large to be meetings."));
    }
    let raw = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    if raw.trim().is_empty() {
        return Ok(MeetingsStore::default());
    }
    serde_json::from_str(&raw).map_err(|e| format!("{FILE}: {e}"))
}

fn save(config_dir: &Path, store: &MeetingsStore) -> Result<(), String> {
    std::fs::create_dir_all(config_dir).map_err(|e| e.to_string())?;
    let raw = serde_json::to_string_pretty(store).map_err(|e| e.to_string())?;
    let tmp = config_dir.join(format!("{FILE}.shell-{}.tmp", std::process::id()));
    std::fs::write(&tmp, raw).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, path(config_dir)).map_err(|e| e.to_string())
}

/// STT PII redaction from Meetings settings. `None` when the toggle is off.
pub fn redact_opts(config_dir: &Path) -> Option<SttRedact> {
    let _g = store();
    load_unlocked(config_dir)
        .ok()
        .filter(|d| d.consent.redact_pii)
        .map(|_| SttRedact::pii())
}

/// Load the Meetings page. `query` is the current search box (may be blank).
pub fn load(config_dir: &Path, query: &str) -> Result<Page, String> {
    let _g = store();
    let data = load_unlocked(config_dir)?;
    Ok(page_of(&data, query))
}

pub fn act(config_dir: &Path, act: Act, query: &str) -> Result<Page, String> {
    let _g = store();
    let mut data = load_unlocked(config_dir)?;
    let search_q = match &act {
        Act::Search { query: q } => Some(q.clone()),
        _ => None,
    };
    match act {
        Act::SetJoin { on } => set_join_consent(&mut data, on),
        Act::SetRecord { on } => set_record_consent(&mut data, on),
        Act::SetRedactPii { on } => set_redact_pii(&mut data, on),
        Act::Start { title } => {
            start_meeting(&mut data, &title, None).map_err(|e| e.to_string())?;
        }
        Act::FollowNotes { notes } => {
            follow_notes(&mut data, &notes, None).map_err(|e| e.to_string())?;
        }
        Act::Talk { text } => {
            talk(&mut data, &text, None).map_err(|e| e.to_string())?;
        }
        Act::Speech { speaker, text } => {
            append_speech(&mut data, &speaker, &text, None).map_err(|e| e.to_string())?;
        }
        Act::End => {
            end_meeting(&mut data, None).map_err(|e| e.to_string())?;
        }
        Act::Search { .. } => {}
    }
    save(config_dir, &data)?;
    Ok(page_of(&data, search_q.as_deref().unwrap_or(query)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_config(tag: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "grokhub-meetings-{}-{}-{}",
            tag,
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn consent_start_notes_talk_end_and_search_round_trip() {
        let root = temp_config("round");
        let page = load(&root, "").unwrap();
        assert!(!page.consent.join);
        assert!(!page.consent.redact_pii);
        assert!(page.live.is_none());

        let page = act(&root, Act::SetRedactPii { on: true }, "").unwrap();
        assert!(page.consent.redact_pii);
        assert!(redact_opts(&root).is_some());
        act(&root, Act::SetRedactPii { on: false }, "").unwrap();
        assert!(redact_opts(&root).is_none());

        act(&root, Act::SetJoin { on: true }, "").unwrap();
        act(&root, Act::SetRecord { on: true }, "").unwrap();
        let page = act(
            &root,
            Act::Start {
                title: "Standup".into(),
            },
            "",
        )
        .unwrap();
        assert_eq!(page.live.as_ref().unwrap().title, "Standup");

        act(
            &root,
            Act::FollowNotes {
                notes: "Ship Friday".into(),
            },
            "",
        )
        .unwrap();
        act(
            &root,
            Act::Talk {
                text: "Any blockers?".into(),
            },
            "",
        )
        .unwrap();
        act(
            &root,
            Act::Speech {
                speaker: "Alex".into(),
                text: "None".into(),
            },
            "",
        )
        .unwrap();
        let page = act(&root, Act::End, "").unwrap();
        assert!(page.live.is_none());
        assert_eq!(page.past.len(), 1);

        let page = act(
            &root,
            Act::Search {
                query: "blockers".into(),
            },
            "blockers",
        )
        .unwrap();
        assert_eq!(page.hits.len(), 1);
        assert!(page.hits[0].text.contains("blockers"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn start_without_join_fails() {
        let root = temp_config("nojoin");
        let err = act(
            &root,
            Act::Start {
                title: "Nope".into(),
            },
            "",
        )
        .unwrap_err();
        assert!(err.contains("Join") || err.contains("join"));
        let _ = std::fs::remove_dir_all(&root);
    }
}
