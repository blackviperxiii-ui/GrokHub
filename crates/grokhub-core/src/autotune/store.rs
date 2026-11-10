//! What auto setup did and learned, kept on this PC only.
//!
//! `{config}/auto_setup.json` holds, per setting, the value auto setup last
//! put there and why, and whether the user has taken it over. A value that no
//! longer matches what auto setup put there means the user changed it, so the
//! setting is theirs from then on (no hook on every setter needed).
//! `{config}/auto_setup_journal.jsonl` is the change history the Auto setup
//! page shows, capped.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::questions::{Answers, Pace};
use super::settings::{Key, Value};

pub const FILE: &str = "auto_setup.json";
pub const JOURNAL: &str = "auto_setup_journal.jsonl";
/// Journal lines kept.
pub const JOURNAL_CAP: usize = 400;

const DAY_MS: u64 = 24 * 60 * 60 * 1_000;
/// An undo this soon after the change counts as a surprise.
pub const SURPRISE_MS: u64 = 10 * 60 * 1_000;
/// After a surprise, fewer visible changes for two weeks.
pub const SURPRISE_COOLDOWN_MS: u64 = 14 * DAY_MS;
/// A rule undone this many times is off for this user.
pub const UNDOS_TO_TURN_OFF: u8 = 2;
/// A dismissed suggestion comes back after this long at the earliest.
pub const SUGGESTION_REST_MS: u64 = 30 * DAY_MS;

/// Where a setting's value came from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Source {
    /// Auto setup worked it out.
    Inferred { rule: String },
    /// A setup answer.
    Asked { rule: String },
    /// The user set it; auto setup leaves it alone.
    UserSet,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    /// What auto setup put there (or what the user had when they took over).
    pub applied: Value,
    pub source: Source,
    pub at_ms: u64,
    /// The value before auto setup changed it, for Undo.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before: Option<Value>,
    pub reason: String,
}

impl Entry {
    pub fn locked(&self) -> bool {
        self.source == Source::UserSet
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AutoStore {
    pub entries: BTreeMap<Key, Entry>,
    pub answers: Answers,
    pub pace: Pace,
    /// Rule ids the user undid twice: never proposed again.
    pub rules_off: BTreeSet<String>,
    pub undo_counts: BTreeMap<String, u8>,
    /// When each visible change was made (the last week's).
    pub visible_at: Vec<u64>,
    /// Fewer visible changes until then.
    pub surprise_until_ms: u64,
    /// Suggestions dismissed, by rule, and when.
    pub dismissed: BTreeMap<String, u64>,
    /// The device fingerprint the last pass saw.
    pub fingerprint: String,
    /// The first-run setup has finished.
    pub setup_done: bool,
}

/// One line of the change history.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JournalLine {
    pub at_ms: u64,
    pub key: Key,
    pub what: What,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<Value>,
    pub to: Value,
    pub reason: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum What {
    /// Auto setup changed it.
    Changed,
    /// The user undid an auto change.
    Undone,
    /// The user changed it themselves; auto setup stops touching it.
    TookOver,
    /// The user asked in chat.
    Asked,
    /// The user handed it back to auto setup.
    HandedBack,
}

pub fn path(config_dir: &Path) -> PathBuf {
    config_dir.join(FILE)
}

pub fn journal_path(config_dir: &Path) -> PathBuf {
    config_dir.join(JOURNAL)
}

pub fn load(config_dir: &Path) -> AutoStore {
    std::fs::read_to_string(path(config_dir))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save(config_dir: &Path, s: &AutoStore) -> Result<(), String> {
    let text = serde_json::to_string_pretty(s).map_err(|e| e.to_string())?;
    crate::pulse_writer::write_atomic(&path(config_dir), &text)
}

/// The change history, oldest first.
pub fn read_journal(config_dir: &Path) -> Vec<JournalLine> {
    std::fs::read_to_string(journal_path(config_dir))
        .map(|s| {
            s.lines()
                .filter_map(|l| serde_json::from_str(l).ok())
                .collect()
        })
        .unwrap_or_default()
}

/// Add lines and keep the newest [`JOURNAL_CAP`].
pub fn append_journal(config_dir: &Path, lines: &[JournalLine]) -> Result<(), String> {
    if lines.is_empty() {
        return Ok(());
    }
    let mut all = read_journal(config_dir);
    all.extend_from_slice(lines);
    let start = all.len().saturating_sub(JOURNAL_CAP);
    let mut text = String::new();
    for l in &all[start..] {
        text.push_str(&serde_json::to_string(l).map_err(|e| e.to_string())?);
        text.push('\n');
    }
    crate::pulse_writer::write_atomic(&journal_path(config_dir), &text)
}

/// Forget everything auto setup learned: answers, history, what it set.
/// The settings themselves stay as they are.
pub fn forget(config_dir: &Path) -> Result<(), String> {
    for p in [path(config_dir), journal_path(config_dir)] {
        match std::fs::remove_file(&p) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.to_string()),
        }
    }
    Ok(())
}

impl AutoStore {
    pub fn locked(&self, key: Key) -> bool {
        self.entries.get(&key).is_some_and(Entry::locked)
    }

    /// Settings the user changed since auto setup set them: they become the
    /// user's. Returns the journal lines for them.
    pub fn take_over_changed(
        &mut self,
        current: &BTreeMap<Key, Value>,
        now_ms: u64,
    ) -> Vec<JournalLine> {
        let mut lines = Vec::new();
        for (key, e) in self.entries.iter_mut() {
            if e.locked() {
                continue;
            }
            let Some(now) = current.get(key) else {
                continue;
            };
            if *now != e.applied {
                lines.push(JournalLine {
                    at_ms: now_ms,
                    key: *key,
                    what: What::TookOver,
                    from: Some(e.applied.clone()),
                    to: now.clone(),
                    reason: "You changed this yourself, so I leave it alone.".into(),
                });
                e.applied = now.clone();
                e.source = Source::UserSet;
                e.before = None;
                e.at_ms = now_ms;
                e.reason = "You set this.".into();
            }
        }
        lines
    }

    /// Record a change auto setup just made.
    pub fn record(
        &mut self,
        key: Key,
        before: Option<Value>,
        to: Value,
        source: Source,
        reason: &str,
        now_ms: u64,
    ) -> JournalLine {
        let what = match source {
            Source::UserSet => What::Asked,
            _ => What::Changed,
        };
        self.entries.insert(
            key,
            Entry {
                applied: to.clone(),
                source,
                at_ms: now_ms,
                before: before.clone(),
                reason: reason.to_string(),
            },
        );
        JournalLine {
            at_ms: now_ms,
            key,
            what,
            from: before,
            to,
            reason: reason.to_string(),
        }
    }

    /// Undo an auto change: returns the value to put back. The setting is the
    /// user's from then on; a rule undone twice is off; a quick undo slows
    /// down visible changes for two weeks.
    pub fn undo(&mut self, key: Key, now_ms: u64) -> Option<(Value, JournalLine)> {
        let e = self.entries.get_mut(&key)?;
        let back = e.before.clone()?;
        // A change the app made on its own and the user took back.
        if let Source::Inferred { rule } = &e.source {
            crate::learn::hub::setup_outcome(rule, crate::learn::decide::Outcome::Undone, now_ms);
        }
        if let Source::Inferred { rule } | Source::Asked { rule } = &e.source {
            let n = self.undo_counts.entry(rule.clone()).or_insert(0);
            *n = n.saturating_add(1);
            if *n >= UNDOS_TO_TURN_OFF {
                self.rules_off.insert(rule.clone());
            }
        }
        if now_ms.saturating_sub(e.at_ms) <= SURPRISE_MS {
            self.surprise_until_ms = now_ms + SURPRISE_COOLDOWN_MS;
        }
        let line = JournalLine {
            at_ms: now_ms,
            key,
            what: What::Undone,
            from: Some(e.applied.clone()),
            to: back.clone(),
            reason: "You undid this.".into(),
        };
        e.applied = back.clone();
        e.source = Source::UserSet;
        e.before = None;
        e.at_ms = now_ms;
        e.reason = "You set this.".into();
        Some((back, line))
    }

    /// The newest auto change that can still be undone.
    pub fn last_undoable(&self) -> Option<Key> {
        self.entries
            .iter()
            .filter(|(_, e)| e.before.is_some())
            .max_by_key(|(_, e)| e.at_ms)
            .map(|(k, _)| *k)
    }

    /// Hand one setting back to auto setup.
    pub fn manage_again(&mut self, key: Key) -> bool {
        let was = self.entries.remove(&key).is_some();
        self.rules_off.retain(|r| {
            let back = rule_is_for(r, key);
            if back {
                crate::learn::hub::setup_forgive(r);
            }
            !back
        });
        was
    }

    /// Hand every setting back.
    pub fn manage_all_again(&mut self) {
        self.entries.retain(|_, e| !e.locked());
        for rule in &self.rules_off {
            crate::learn::hub::setup_forgive(rule);
        }
        self.rules_off.clear();
        self.undo_counts.clear();
    }

    /// Count one visible change auto setup made on its own toward the budget.
    pub fn spend(&mut self, now_ms: u64) {
        self.visible_at.push(now_ms);
        self.visible_at
            .retain(|t| now_ms.saturating_sub(*t) < 7 * DAY_MS);
    }

    pub fn dismiss(&mut self, rule: &str, now_ms: u64) {
        crate::learn::hub::setup_outcome(rule, crate::learn::decide::Outcome::Declined, now_ms);
        self.dismissed.insert(rule.to_string(), now_ms);
    }

    pub fn resting(&self, rule: &str, now_ms: u64) -> bool {
        self.dismissed
            .get(rule)
            .is_some_and(|t| now_ms.saturating_sub(*t) < SUGGESTION_REST_MS)
    }

    /// Visible changes still allowed now: 1 a day, 3 a week (1 a week for two
    /// weeks after a quick undo).
    pub fn visible_left(&self, now_ms: u64) -> usize {
        let day = self
            .visible_at
            .iter()
            .filter(|t| now_ms.saturating_sub(**t) < DAY_MS)
            .count();
        let week = self
            .visible_at
            .iter()
            .filter(|t| now_ms.saturating_sub(**t) < 7 * DAY_MS)
            .count();
        let week_cap = if now_ms < self.surprise_until_ms {
            1
        } else {
            3
        };
        1usize
            .saturating_sub(day)
            .min(week_cap - week.min(week_cap))
    }
}

/// Rule ids start with the key they move, for the rules that one key owns.
fn rule_is_for(rule: &str, key: Key) -> bool {
    super::rules::rule_keys(rule).contains(&key)
}

#[cfg(test)]
mod tests {
    use super::*;

    const T: u64 = 1_800_000_000_000;

    fn applied(s: &mut AutoStore, key: Key, before: Value, to: Value, rule: &str, at: u64) {
        s.record(
            key,
            Some(before),
            to,
            Source::Inferred { rule: rule.into() },
            "why",
            at,
        );
    }

    #[test]
    fn a_setting_the_user_changed_becomes_theirs() {
        let mut s = AutoStore::default();
        applied(
            &mut s,
            Key::TextSize,
            Value::Pct(100),
            Value::Pct(130),
            "os_text_size",
            T,
        );
        let mut now = BTreeMap::new();
        now.insert(Key::TextSize, Value::Pct(130));
        assert!(s.take_over_changed(&now, T + 1).is_empty());
        now.insert(Key::TextSize, Value::Pct(115));
        let lines = s.take_over_changed(&now, T + 2);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].what, What::TookOver);
        assert!(s.locked(Key::TextSize));
        assert_eq!(s.entries[&Key::TextSize].applied, Value::Pct(115));
        // Theirs stays theirs.
        now.insert(Key::TextSize, Value::Pct(150));
        assert!(s.take_over_changed(&now, T + 3).is_empty());
    }

    #[test]
    fn two_undos_turn_a_rule_off_and_a_quick_undo_slows_changes() {
        let mut s = AutoStore::default();
        applied(
            &mut s,
            Key::Effects,
            Value::Bool(true),
            Value::Bool(false),
            "light_pc_effects",
            T,
        );
        let (back, line) = s.undo(Key::Effects, T + 60_000).expect("undo");
        assert_eq!(back, Value::Bool(true));
        assert_eq!(line.what, What::Undone);
        assert!(s.locked(Key::Effects));
        assert!(s.rules_off.is_empty());
        assert_eq!(s.surprise_until_ms, T + 60_000 + SURPRISE_COOLDOWN_MS);
        assert!(
            s.undo(Key::Effects, T + 70_000).is_none(),
            "nothing left to undo"
        );
        // Handed back, applied again, undone again much later.
        assert!(s.manage_again(Key::Effects));
        applied(
            &mut s,
            Key::Effects,
            Value::Bool(true),
            Value::Bool(false),
            "light_pc_effects",
            T + DAY_MS,
        );
        s.undo(Key::Effects, T + 3 * DAY_MS).expect("undo");
        assert!(s.rules_off.contains("light_pc_effects"));
        assert_eq!(
            s.surprise_until_ms,
            T + 60_000 + SURPRISE_COOLDOWN_MS,
            "a late undo is no surprise"
        );
    }

    #[test]
    fn the_change_budget_is_one_a_day_and_three_a_week() {
        let mut s = AutoStore::default();
        assert_eq!(s.visible_left(T), 1);
        s.spend(T);
        assert_eq!(s.visible_left(T + 1), 0);
        assert_eq!(s.visible_left(T + DAY_MS), 1);
        s.spend(T + DAY_MS);
        s.spend(T + 2 * DAY_MS);
        assert_eq!(s.visible_left(T + 3 * DAY_MS), 0, "three this week");
        assert_eq!(s.visible_left(T + 7 * DAY_MS + 1), 1);
        // Recording a change alone spends nothing.
        let mut q = AutoStore::default();
        applied(
            &mut q,
            Key::TextSize,
            Value::Pct(100),
            Value::Pct(115),
            "r",
            T,
        );
        assert_eq!(q.visible_left(T), 1);
        // After a surprise: one a week.
        let mut r = AutoStore {
            surprise_until_ms: T + SURPRISE_COOLDOWN_MS,
            ..AutoStore::default()
        };
        r.spend(T);
        assert_eq!(r.visible_left(T + 2 * DAY_MS), 0);
    }

    #[test]
    fn the_last_undoable_change_is_the_newest() {
        let mut s = AutoStore::default();
        applied(
            &mut s,
            Key::TextSize,
            Value::Pct(100),
            Value::Pct(115),
            "a",
            T,
        );
        applied(
            &mut s,
            Key::Effects,
            Value::Bool(true),
            Value::Bool(false),
            "b",
            T + 5,
        );
        assert_eq!(s.last_undoable(), Some(Key::Effects));
        s.undo(Key::Effects, T + 10).expect("undo");
        assert_eq!(s.last_undoable(), Some(Key::TextSize));
    }

    #[test]
    fn dismissed_suggestions_rest_for_a_month() {
        let mut s = AutoStore::default();
        s.dismiss("wide_screen", T);
        assert!(s.resting("wide_screen", T + DAY_MS));
        assert!(!s.resting("wide_screen", T + SUGGESTION_REST_MS));
        assert!(!s.resting("two_screens", T));
    }

    #[test]
    fn store_and_journal_round_trip_and_forget_clears_both() {
        let dir = std::env::temp_dir().join(format!("grokhub-auto-store-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("dir");
        let mut s = AutoStore::default();
        let line = s.record(
            Key::TextSize,
            Some(Value::Pct(100)),
            Value::Pct(130),
            Source::Inferred {
                rule: "os_text_size".into(),
            },
            "Text is bigger because Windows text size is 150%.",
            T,
        );
        s.answers
            .insert(super::super::questions::Qid::Talk, "direct".into());
        save(&dir, &s).expect("save");
        assert_eq!(load(&dir), s);
        for i in 0..(JOURNAL_CAP + 5) {
            let mut l = line.clone();
            l.at_ms = T + i as u64;
            append_journal(&dir, &[l]).expect("append");
        }
        let j = read_journal(&dir);
        assert_eq!(j.len(), JOURNAL_CAP);
        assert_eq!(j[0].at_ms, T + 5);
        forget(&dir).expect("forget");
        assert_eq!(load(&dir), AutoStore::default());
        assert!(read_journal(&dir).is_empty());
        forget(&dir).expect("forget twice is fine");
        let _ = std::fs::remove_dir_all(dir);
    }
}
