use serde::{Deserialize, Serialize};

use crate::uid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BoardStatus {
    Proposed,
    Approved,
    Staged,
    InProgress,
    Done,
    Dismissed,
}

impl BoardStatus {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "proposed" => Some(Self::Proposed),
            "approved" => Some(Self::Approved),
            "staged" => Some(Self::Staged),
            "in_progress" | "in-progress" | "progress" => Some(Self::InProgress),
            "done" => Some(Self::Done),
            "dismissed" | "dismiss" => Some(Self::Dismissed),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Proposed => "proposed",
            Self::Approved => "approved",
            Self::Staged => "staged",
            Self::InProgress => "in_progress",
            Self::Done => "done",
            Self::Dismissed => "dismissed",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BoardCard {
    pub id: String,
    pub title: String,
    pub detail: String,
    pub status: BoardStatus,
    #[serde(default)]
    pub priority: String,
}

impl BoardCard {
    pub fn new(title: &str, detail: &str, priority: &str) -> Self {
        Self {
            id: uid("w"),
            title: title.trim().chars().take(120).collect(),
            detail: detail.trim().chars().take(2000).collect(),
            status: BoardStatus::Proposed,
            priority: priority.trim().chars().take(16).collect(),
        }
    }
}

/// `WORK_PIN: title | detail | priority=high`
pub fn parse_work_pin(line: &str) -> Option<BoardCard> {
    let rest = line.trim().strip_prefix("WORK_PIN:")?;
    let mut parts = rest.split('|').map(|s| s.trim());
    let title = parts.next().filter(|s| !s.is_empty())?;
    let detail = parts.next().unwrap_or("").to_string();
    let mut priority = String::new();
    for p in parts {
        if let Some(v) = p.strip_prefix("priority=") {
            priority = v.to_string();
        }
    }
    Some(BoardCard::new(title, &detail, &priority))
}

/// `WORK_UPDATE: id-or-title | status=in_progress`
pub fn parse_work_update(line: &str) -> Option<(String, BoardStatus)> {
    let rest = line.trim().strip_prefix("WORK_UPDATE:")?;
    let mut parts = rest.split('|').map(|s| s.trim());
    let key = parts.next().filter(|s| !s.is_empty())?.to_string();
    let mut status = None;
    for p in parts {
        if let Some(v) = p.strip_prefix("status=") {
            status = BoardStatus::parse(v);
        }
    }
    Some((key, status?))
}

pub fn apply_work_update(cards: &mut [BoardCard], key: &str, status: BoardStatus) -> bool {
    if let Some(c) = cards.iter_mut().find(|c| c.id == key || c.title.eq_ignore_ascii_case(key)) {
        c.status = status;
        return true;
    }
    false
}

pub fn extract_work_pins(text: &str) -> Vec<BoardCard> {
    text.lines().filter_map(parse_work_pin).collect()
}

pub fn extract_work_updates(text: &str) -> Vec<(String, BoardStatus)> {
    text.lines().filter_map(parse_work_update).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pin_and_update() {
        let card = parse_work_pin("WORK_PIN: Flash the pi | write image | priority=high").unwrap();
        assert_eq!(card.title, "Flash the pi");
        assert_eq!(card.priority, "high");
        assert_eq!(card.status, BoardStatus::Proposed);
        let (key, st) = parse_work_update("WORK_UPDATE: Flash the pi | status=in_progress").unwrap();
        let mut cards = vec![card];
        assert!(apply_work_update(&mut cards, &key, st));
        assert_eq!(cards[0].status, BoardStatus::InProgress);
    }
}
