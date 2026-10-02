//! Local card signals. One JSON line per event in `<config>/card_signals.jsonl`.
//! The file stays on this machine. A line names the card, never its text.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::update_feed::{group_key, topic_tokens, UpdateCard, UpdateKind};

pub const SIGNAL_CAP: usize = 2_000;
const IGNORE_SESSIONS: usize = 3;
const IGNORE_MS: u64 = 48 * 60 * 60 * 1000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CardEvent {
    Shown,
    Opened,
    Replied,
    Acted,
    Dismissed,
    Less,
    More,
    Muted,
    Ignored,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CardSignal {
    pub ts: u64,
    pub card_id: String,
    pub kind: String,
    pub group: String,
    pub topic: Vec<String>,
    pub event: CardEvent,
    pub ms_since_shown: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after_open: Option<bool>,
}

pub fn signals_path(dir: &Path) -> PathBuf {
    dir.join("card_signals.jsonl")
}

pub fn signal_group(card: &UpdateCard) -> String {
    if let Some(key) = group_key(card) {
        return key;
    }
    let source = card.source_id.trim();
    if source.is_empty() {
        format!("other:{}", card.id)
    } else {
        format!("other:{source}")
    }
}

fn kind_name(kind: UpdateKind) -> &'static str {
    match kind {
        UpdateKind::AutomationDone => "automation_done",
        UpdateKind::ScheduleCreated => "schedule_created",
        UpdateKind::Suggestion => "suggestion",
        UpdateKind::AutomateOffer => "automate_offer",
        UpdateKind::Idea => "idea",
        UpdateKind::Digest => "digest",
    }
}

pub fn signal_from_card(
    card: &UpdateCard,
    event: CardEvent,
    ts: u64,
    ms_since_shown: u64,
    after_open: Option<bool>,
) -> CardSignal {
    let after_open = if event == CardEvent::Dismissed {
        Some(after_open.unwrap_or(false))
    } else {
        None
    };
    CardSignal {
        ts,
        card_id: card.id.clone(),
        kind: kind_name(card.kind).to_string(),
        group: signal_group(card),
        topic: topic_tokens(&card.title),
        event,
        ms_since_shown,
        after_open,
    }
}

pub fn ms_since_shown(log: &[CardSignal], card_id: &str, now: u64) -> u64 {
    log.iter()
        .filter(|row| row.card_id == card_id && row.event == CardEvent::Shown)
        .map(|row| row.ts)
        .min()
        .map(|ts| now.saturating_sub(ts))
        .unwrap_or(0)
}

fn interacted(event: CardEvent) -> bool {
    matches!(
        event,
        CardEvent::Opened
            | CardEvent::Replied
            | CardEvent::Acted
            | CardEvent::Dismissed
            | CardEvent::Less
            | CardEvent::More
            | CardEvent::Muted
    )
}

/// Cards still on screen with no interaction after 3 sessions or 48 hours.
/// `live_ids` is the set still visible. One `Ignored` per card.
pub fn ignored_signals(log: &[CardSignal], now: u64, live_ids: &[String]) -> Vec<CardSignal> {
    let mut out = Vec::new();
    for id in live_ids {
        if log
            .iter()
            .any(|row| &row.card_id == id && row.event == CardEvent::Ignored)
        {
            continue;
        }
        let shown: Vec<&CardSignal> = log
            .iter()
            .filter(|row| &row.card_id == id && row.event == CardEvent::Shown)
            .collect();
        if shown.is_empty() {
            continue;
        }
        if log.iter().any(|row| &row.card_id == id && interacted(row.event)) {
            continue;
        }
        let first = shown.iter().map(|row| row.ts).min().unwrap_or(now);
        let aged = now.saturating_sub(first) >= IGNORE_MS;
        if shown.len() < IGNORE_SESSIONS && !aged {
            continue;
        }
        let sample = shown[0];
        out.push(CardSignal {
            ts: now,
            card_id: id.clone(),
            kind: sample.kind.clone(),
            group: sample.group.clone(),
            topic: sample.topic.clone(),
            event: CardEvent::Ignored,
            ms_since_shown: now.saturating_sub(first),
            after_open: None,
        });
    }
    out
}

pub fn parse_signals(raw: &str) -> Vec<CardSignal> {
    raw.lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() {
                None
            } else {
                serde_json::from_str(line).ok()
            }
        })
        .collect()
}

pub fn cap_lines(lines: &[String]) -> Vec<String> {
    if lines.len() <= SIGNAL_CAP {
        return lines.to_vec();
    }
    lines[lines.len() - SIGNAL_CAP..].to_vec()
}

pub fn load_signals(dir: &Path) -> Vec<CardSignal> {
    let raw = std::fs::read_to_string(signals_path(dir)).unwrap_or_default();
    parse_signals(&raw)
}

pub fn append_signals(dir: &Path, extra: &[CardSignal]) -> Result<(), String> {
    if extra.is_empty() {
        return Ok(());
    }
    let path = signals_path(dir);
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).map_err(|err| err.to_string())?;
        }
    }
    let mut lines: Vec<String> = std::fs::read_to_string(&path)
        .unwrap_or_default()
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(str::to_string)
        .collect();
    for row in extra {
        lines.push(serde_json::to_string(row).map_err(|err| err.to_string())?);
    }
    let lines = cap_lines(&lines);
    let mut out = lines.join("\n");
    if !out.is_empty() {
        out.push('\n');
    }
    std::fs::write(&path, out).map_err(|err| err.to_string())
}

pub fn append_signal(dir: &Path, row: &CardSignal) -> Result<(), String> {
    append_signals(dir, std::slice::from_ref(row))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::automation_done_card;

    fn row(id: &str, event: CardEvent, ts: u64) -> CardSignal {
        CardSignal {
            ts,
            card_id: id.into(),
            kind: "automation_done".into(),
            group: "run:loop-a".into(),
            topic: vec!["snapshot".into()],
            event,
            ms_since_shown: 0,
            after_open: None,
        }
    }

    #[test]
    fn signal_log_caps_at_two_thousand_and_holds_no_card_text() {
        let src = include_str!("card_signals.rs");
        let code = src.split("mod tests").next().expect("module");
        assert!(!code.contains(".body"), "{code}");
        assert!(!code.contains("\"body\""));
        let dir = std::env::temp_dir().join(format!("grokhub-signals-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for n in 0..SIGNAL_CAP + 5 {
            append_signal(&dir, &row("loop-card", CardEvent::Shown, n as u64)).unwrap();
        }
        let loaded = load_signals(&dir);
        assert_eq!(loaded.len(), SIGNAL_CAP);
        assert_eq!(loaded[0].ts, 5);
        assert_eq!(loaded.last().unwrap().ts, (SIGNAL_CAP + 4) as u64);
        let raw = std::fs::read_to_string(signals_path(&dir)).unwrap();
        assert!(!raw.contains("\"body\""));
        assert!(!raw.contains("report word"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn ignored_after_three_sessions_or_two_days_unless_someone_acted() {
        let live = vec!["c1".to_string()];
        let quiet = vec![row("c1", CardEvent::Shown, 1), row("c1", CardEvent::Shown, 2)];
        assert!(ignored_signals(&quiet, 3, &live).is_empty());
        let three = vec![
            row("c1", CardEvent::Shown, 1),
            row("c1", CardEvent::Shown, 2),
            row("c1", CardEvent::Shown, 3),
        ];
        let found = ignored_signals(&three, 4, &live);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].event, CardEvent::Ignored);
        let mut with_open = three.clone();
        with_open.push(row("c1", CardEvent::Opened, 4));
        assert!(ignored_signals(&with_open, 5, &live).is_empty());
        let aged = vec![row("c1", CardEvent::Shown, 10)];
        let later = ignored_signals(&aged, 10 + IGNORE_MS, &live);
        assert_eq!(later.len(), 1);
        assert!(ignored_signals(&aged, 10 + IGNORE_MS, &[]).is_empty());
    }

    #[test]
    fn signal_from_a_card_keeps_five_tokens_and_the_group() {
        let card = automation_done_card("loop-a", "Host snapshot report for the cabin", "notes", 9);
        let row = signal_from_card(&card, CardEvent::Dismissed, 20, 11, Some(true));
        assert_eq!(row.group, "run:loop-a");
        assert_eq!(row.kind, "automation_done");
        assert!(row.topic.len() <= 5);
        assert_eq!(row.after_open, Some(true));
        assert_eq!(row.ms_since_shown, 11);
        let opened = signal_from_card(&card, CardEvent::Opened, 20, 0, Some(true));
        assert!(opened.after_open.is_none());
    }
}
