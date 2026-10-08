//! Local card signal log. One JSON line per event under the config directory.
//! Lines carry ids and the event name only. No card text, and no network.

use std::fs::{self, OpenOptions};
use std::io::{ErrorKind, Write};
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::update_feed::{feed_group_key, UpdateCard, UpdateKind};

/// Active log length that triggers a rotate into `card_signals.jsonl.1`.
pub const SIGNAL_CAP: usize = 2_000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CardSignal {
    pub ts: u64,
    pub card_id: String,
    pub kind: String,
    pub group: String,
    pub source_id: String,
    pub event: CardEvent,
    /// Set only for [`CardEvent::Dismissed`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after_open: Option<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CardEvent {
    Opened,
    Dismissed,
    More,
    Less,
    Hidden,
    Unhidden,
    FollowUp,
    /// Closed with X before it was ever opened or used.
    Rejected,
    /// The card's action ran: an idea filed, an offer turned into an automation.
    Ran,
    /// What the card started is done, e.g. its workboard card reached Done.
    Completed,
}

fn kind_name(kind: UpdateKind) -> &'static str {
    match kind {
        UpdateKind::AutomationDone => "automation_done",
        UpdateKind::ScheduleCreated => "schedule_created",
        UpdateKind::Suggestion => "suggestion",
        UpdateKind::AutomateOffer => "automate_offer",
        UpdateKind::Idea => "idea",
        UpdateKind::Digest => "digest",
        UpdateKind::SelfChange => "self_change",
        UpdateKind::DoneForYou => "done_for_you",
    }
}

/// Group key, or `other:<source or id>` when the card is not an event group.
pub fn signal_group(card: &UpdateCard) -> String {
    if let Some(key) = feed_group_key(card) {
        return key;
    }
    let source = card.source_id.trim();
    if source.is_empty() {
        format!("other:{}", card.id)
    } else {
        format!("other:{source}")
    }
}

/// A log row. `after_open` is kept only for a dismiss.
pub fn signal_for(card: &UpdateCard, event: CardEvent, ts: u64, after_open: Option<bool>) -> CardSignal {
    CardSignal {
        ts,
        card_id: card.id.clone(),
        kind: kind_name(card.kind).to_string(),
        group: signal_group(card),
        source_id: card.source_id.clone(),
        event,
        after_open: if event == CardEvent::Dismissed {
            after_open
        } else {
            None
        },
    }
}

/// Append one line to `<dir>/card_signals.jsonl`. At 2,000 lines the file
/// rotates to `card_signals.jsonl.1` (replacing any older `.1`) and a new
/// file starts. IO errors are ignored.
pub fn append_signal(dir: &Path, signal: &CardSignal) {
    if let Err(_err) = append_signal_inner(dir, signal) {
        // A full disk or a locked folder must not panic or stall the UI.
    }
}

fn append_signal_inner(dir: &Path, signal: &CardSignal) -> std::io::Result<()> {
    fs::create_dir_all(dir)?;
    let path = dir.join("card_signals.jsonl");
    let lines = count_lines(&path)?;
    if lines >= SIGNAL_CAP {
        let rotated = dir.join("card_signals.jsonl.1");
        let _ = fs::remove_file(&rotated);
        fs::rename(&path, &rotated)?;
    }
    let line = serde_json::to_string(signal)
        .map_err(|err| std::io::Error::new(ErrorKind::InvalidData, err))?;
    let mut file = OpenOptions::new().create(true).append(true).open(&path)?;
    writeln!(file, "{line}")?;
    Ok(())
}

fn count_lines(path: &Path) -> std::io::Result<usize> {
    match fs::read_to_string(path) {
        Ok(text) => Ok(text.lines().filter(|line| !line.trim().is_empty()).count()),
        Err(err) if err.kind() == ErrorKind::NotFound => Ok(0),
        Err(err) => Err(err),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::update_feed::automation_done_card;

    #[test]
    fn signal_log_rotates_at_cap() {
        let dir = std::env::temp_dir().join(format!(
            "grokhub-signals-{}-{}",
            std::process::id(),
            SIGNAL_CAP
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let card = automation_done_card("src", "Name", "body", 1);
        for n in 0..SIGNAL_CAP {
            append_signal(&dir, &signal_for(&card, CardEvent::Opened, n as u64, None));
        }
        let active = fs::read_to_string(dir.join("card_signals.jsonl")).unwrap();
        assert_eq!(active.lines().filter(|line| !line.is_empty()).count(), SIGNAL_CAP);
        assert!(!dir.join("card_signals.jsonl.1").exists());
        append_signal(
            &dir,
            &signal_for(&card, CardEvent::More, SIGNAL_CAP as u64, None),
        );
        let rotated = fs::read_to_string(dir.join("card_signals.jsonl.1")).unwrap();
        assert_eq!(rotated.lines().filter(|line| !line.is_empty()).count(), SIGNAL_CAP);
        let fresh = fs::read_to_string(dir.join("card_signals.jsonl")).unwrap();
        assert_eq!(fresh.lines().filter(|line| !line.is_empty()).count(), 1);
        assert!(fresh.contains("\"event\":\"more\""));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn signal_lines_hold_no_card_text() {
        let dir = std::env::temp_dir().join(format!(
            "grokhub-signals-text-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        let mut card = automation_done_card("src-plain", "placeholder", "placeholder", 7);
        card.title = "ZephyrTitle9f3a".into();
        card.body = Some("ZephyrBody9f3a".into());
        card.why = Some("ZephyrWhy9f3a".into());
        card.prompt = Some("ZephyrPrompt9f3a".into());
        card.details = Some("ZephyrDetails9f3a".into());
        let events = [
            (CardEvent::Opened, None),
            (CardEvent::Dismissed, Some(true)),
            (CardEvent::Dismissed, Some(false)),
            (CardEvent::More, None),
            (CardEvent::Less, None),
            (CardEvent::Hidden, None),
            (CardEvent::Unhidden, None),
            (CardEvent::FollowUp, None),
        ];
        for (event, after) in events {
            append_signal(&dir, &signal_for(&card, event, 9, after));
        }
        let text = fs::read_to_string(dir.join("card_signals.jsonl")).unwrap();
        for secret in [
            "ZephyrTitle9f3a",
            "ZephyrBody9f3a",
            "ZephyrWhy9f3a",
            "ZephyrPrompt9f3a",
            "ZephyrDetails9f3a",
        ] {
            assert!(!text.contains(secret), "{secret} leaked into {text}");
        }
        let allowed = ["ts", "card_id", "kind", "group", "source_id", "event", "after_open"];
        for line in text.lines().filter(|line| !line.trim().is_empty()) {
            let value: serde_json::Value = serde_json::from_str(line).unwrap();
            let obj = value.as_object().unwrap();
            for key in obj.keys() {
                assert!(allowed.contains(&key.as_str()), "unexpected key {key} in {line}");
            }
            for key in ["ts", "card_id", "kind", "group", "source_id", "event"] {
                assert!(obj.contains_key(key), "missing {key} in {line}");
            }
            let event = obj.get("event").and_then(|v| v.as_str()).unwrap();
            if event == "dismissed" {
                assert!(obj.get("after_open").and_then(|v| v.as_bool()).is_some());
            } else {
                assert!(!obj.contains_key("after_open"), "{line}");
            }
            assert_eq!(obj.get("kind").and_then(|v| v.as_str()), Some("automation_done"));
            assert_eq!(obj.get("group").and_then(|v| v.as_str()), Some("run:src-plain"));
            assert!(!line.contains("title"));
            assert!(!line.contains("body"));
            assert!(!line.contains("prompt"));
            assert!(!line.contains("details"));
            assert!(!line.contains("why"));
            assert!(!line.contains("topic"));
        }
        let _ = fs::remove_dir_all(&dir);
    }
}
