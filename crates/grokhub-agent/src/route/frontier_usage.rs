//! Frontier usage meter: local vs frontier turns and tokens, per project and
//! per day. Append-only training log for the later learned router (card 12),
//! plus a short rollup the Home Usage tile reads for a weekly line.
//!
//! Ids, counts and a plain route reason only. Never prompt text.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use grokhub_core::model_registry::store::{append_line, models_dir, read_json, write_json};

use super::budget::DAY_MS;
use super::local::{self, PROVIDER_LOCAL};
use super::log::{RouteRecord, RouteTokens};
use super::session_metrics;

pub const LOG_FILE: &str = "frontier_usage.jsonl";
pub const ROLLUP_FILE: &str = "frontier_usage.json";
pub const SCHEMA: u32 = 1;
/// Newest log lines kept; older ones drop on the next write past the cap.
pub const LOG_CAP: usize = 20_000;
/// Calendar days kept in the rollup (about five weeks).
pub const DAY_CAP: usize = 35;
/// Unbound chats (no project folder) land under this key.
pub const APP_PROJECT: &str = "_app_";
/// Rollup key that sums every project for a day.
pub const ALL_PROJECT: &str = "_all";

/// Where the call ran: on this computer, or on a cloud / frontier model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Lane {
    Local,
    Frontier,
}

impl Lane {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::Frontier => "frontier",
        }
    }
}

/// Classify the model that answered. Local ids and the local provider are
/// local; everything else counts as frontier for this meter.
pub fn lane_of(provider: &str, model: &str) -> Lane {
    if local::is_local(model) || provider.trim() == PROVIDER_LOCAL {
        Lane::Local
    } else {
        Lane::Frontier
    }
}

/// One finished routed call. Written as a JSONL line.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Event {
    pub ts_ms: u64,
    /// `YYYY-MM-DD` UTC.
    pub day: String,
    /// Project folder key, or [`APP_PROJECT`].
    pub project: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub episode: String,
    pub lane: Lane,
    pub provider: String,
    pub model: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub class: String,
    /// The call moved to a stronger model (D13 escalation rules).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub escalated: bool,
    /// Plain route reason (ids and short phrases only).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub reason: String,
    pub tokens: RouteTokens,
    #[serde(default, skip_serializing_if = "is_zero_f64")]
    pub cost_usd: f64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rules: Vec<String>,
}

fn is_zero_f64(n: &f64) -> bool {
    *n == 0.0
}

/// Per-day, per-project counters.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DayBucket {
    #[serde(default)]
    pub local_turns: u32,
    #[serde(default)]
    pub frontier_turns: u32,
    #[serde(default)]
    pub local_tokens: u64,
    #[serde(default)]
    pub frontier_tokens: u64,
    #[serde(default)]
    pub escalations: u32,
}

impl DayBucket {
    pub fn add(&mut self, lane: Lane, tokens: u64, escalated: bool) {
        match lane {
            Lane::Local => {
                self.local_turns = self.local_turns.saturating_add(1);
                self.local_tokens = self.local_tokens.saturating_add(tokens);
            }
            Lane::Frontier => {
                self.frontier_turns = self.frontier_turns.saturating_add(1);
                self.frontier_tokens = self.frontier_tokens.saturating_add(tokens);
            }
        }
        if escalated {
            self.escalations = self.escalations.saturating_add(1);
        }
    }

    pub fn merge(&mut self, other: &DayBucket) {
        self.local_turns = self.local_turns.saturating_add(other.local_turns);
        self.frontier_turns = self.frontier_turns.saturating_add(other.frontier_turns);
        self.local_tokens = self.local_tokens.saturating_add(other.local_tokens);
        self.frontier_tokens = self.frontier_tokens.saturating_add(other.frontier_tokens);
        self.escalations = self.escalations.saturating_add(other.escalations);
    }

    pub fn turns(&self) -> u32 {
        self.local_turns.saturating_add(self.frontier_turns)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Store {
    #[serde(default)]
    pub schema: u32,
    /// `day` → `project` → bucket. `_all` is the day total.
    #[serde(default)]
    pub days: BTreeMap<String, BTreeMap<String, DayBucket>>,
}

impl Store {
    pub fn note(&mut self, event: &Event) {
        let tokens = event.tokens.input.saturating_add(event.tokens.out);
        let day = self.days.entry(event.day.clone()).or_default();
        day.entry(event.project.clone())
            .or_default()
            .add(event.lane, tokens, event.escalated);
        day.entry(ALL_PROJECT.to_string())
            .or_default()
            .add(event.lane, tokens, event.escalated);
        while self.days.len() > DAY_CAP {
            if let Some(oldest) = self.days.keys().next().cloned() {
                self.days.remove(&oldest);
            } else {
                break;
            }
        }
    }
}

/// What the Home Usage tile shows for the last seven UTC days.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WeekLine {
    pub local_turns: u32,
    pub frontier_turns: u32,
    pub escalations: u32,
    pub local_tokens: u64,
    pub frontier_tokens: u64,
}

impl WeekLine {
    pub fn turns(self) -> u32 {
        self.local_turns.saturating_add(self.frontier_turns)
    }

    /// Local share of turns, percent. `None` when nothing ran.
    pub fn local_pct(self) -> Option<u8> {
        let n = self.turns();
        if n == 0 {
            None
        } else {
            Some(((self.local_turns as u64 * 100) / u64::from(n)) as u8)
        }
    }

    /// Escalations over all turns, percent. `None` when nothing ran.
    pub fn escalation_pct(self) -> Option<u8> {
        let n = self.turns();
        if n == 0 {
            None
        } else {
            Some(((u64::from(self.escalations) * 100) / u64::from(n)) as u8)
        }
    }

    /// One short line for the Usage tile detail, or `None` with no data.
    pub fn detail(self) -> Option<String> {
        let local = self.local_pct()?;
        let esc = self.escalation_pct().unwrap_or(0);
        Some(format!(
            "Local {local}% · frontier escalations {esc}% this week"
        ))
    }
}

/// `YYYY-MM-DD` for `ts_ms` in UTC.
pub fn day_key(ts_ms: u64) -> String {
    let days = ts_ms / DAY_MS;
    let (y, m, d) = civil_from_days(days as i64);
    format!("{y:04}-{m:02}-{d:02}")
}

/// Proleptic Gregorian date from days since Unix epoch (Howard Hinnant).
fn civil_from_days(z: i64) -> (i32, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u32;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = (yoe as i64) + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y as i32, m, d)
}

/// Sum the last seven UTC days (today inclusive) for `project`, or [`ALL_PROJECT`].
pub fn week_line(store: &Store, now_ms: u64, project: &str) -> WeekLine {
    let mut out = WeekLine::default();
    let key = if project.is_empty() {
        ALL_PROJECT
    } else {
        project
    };
    for i in 0..7u64 {
        let day = day_key(now_ms.saturating_sub(i * DAY_MS));
        if let Some(bucket) = store.days.get(&day).and_then(|d| d.get(key)) {
            out.local_turns = out.local_turns.saturating_add(bucket.local_turns);
            out.frontier_turns = out.frontier_turns.saturating_add(bucket.frontier_turns);
            out.escalations = out.escalations.saturating_add(bucket.escalations);
            out.local_tokens = out.local_tokens.saturating_add(bucket.local_tokens);
            out.frontier_tokens = out.frontier_tokens.saturating_add(bucket.frontier_tokens);
        }
    }
    out
}

pub fn log_path(config_dir: &Path) -> PathBuf {
    models_dir(config_dir).join(LOG_FILE)
}

pub fn rollup_path(config_dir: &Path) -> PathBuf {
    models_dir(config_dir).join(ROLLUP_FILE)
}

pub fn load(config_dir: &Path) -> Store {
    let mut s: Store = read_json(&rollup_path(config_dir)).unwrap_or_default();
    if s.schema == 0 {
        s.schema = SCHEMA;
    }
    s
}

/// Build an [`Event`] from a finished route record.
pub fn event_from(rec: &RouteRecord, project: &str, ts_ms: u64, cost_usd: f64) -> Event {
    let provider = if rec.used.provider.is_empty() {
        rec.chosen.provider.clone()
    } else {
        rec.used.provider.clone()
    };
    let model = if rec.used.model.is_empty() {
        rec.chosen.model.clone()
    } else {
        rec.used.model.clone()
    };
    let tokens = rec.outcome.as_ref().map(|o| o.tokens).unwrap_or_default();
    let escalated = session_metrics::is_escalation(&rec.rule_ids);
    Event {
        ts_ms,
        day: day_key(ts_ms),
        project: if project.trim().is_empty() {
            APP_PROJECT.to_string()
        } else {
            project.trim().to_string()
        },
        episode: rec.episode.clone(),
        lane: lane_of(&provider, &model),
        provider,
        model,
        class: rec.class.clone(),
        escalated,
        reason: rec.reason.clone(),
        tokens,
        cost_usd,
        rules: rec.rule_ids.clone(),
    }
}

/// Project key for a chat: Code tab binding when present, else [`APP_PROJECT`].
pub fn project_for_session(session: &str, episode: &str) -> String {
    let id = if !session.is_empty() {
        session
    } else {
        episode
    };
    if let Some((_, workspace, _)) = crate::code_panel::session_settings(id) {
        return crate::code_panel::project_key(&workspace);
    }
    APP_PROJECT.to_string()
}

static WRITE: Mutex<()> = Mutex::new(());

/// Append one call to the log and fold it into the rollup. Never fails the call.
pub fn note(config_dir: &Path, event: &Event) {
    if cfg!(test) && !crate::perm::config_pinned() {
        return;
    }
    let _g = WRITE.lock().unwrap_or_else(|e| e.into_inner());
    let _ = append_line(&log_path(config_dir), event);
    trim_log(config_dir);
    let mut store = load(config_dir);
    store.schema = SCHEMA;
    store.note(event);
    let _ = write_json(&rollup_path(config_dir), &store);
}

fn trim_log(config_dir: &Path) {
    let path = log_path(config_dir);
    let Ok(text) = std::fs::read_to_string(&path) else {
        return;
    };
    let lines: Vec<&str> = text.lines().filter(|l| !l.is_empty()).collect();
    if lines.len() <= LOG_CAP {
        return;
    }
    let keep = lines[lines.len() - LOG_CAP..].join("\n") + "\n";
    let _ = crate::harness::private_write(&path, keep.as_bytes());
}

/// This week's line from disk. Pure over the loaded rollup.
pub fn week_from_disk(config_dir: &Path, now_ms: u64) -> WeekLine {
    week_line(&load(config_dir), now_ms, ALL_PROJECT)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::route::log::{Chosen, RouteOutcome};

    fn rec(provider: &str, model: &str, rules: &[&str], reason: &str) -> RouteRecord {
        RouteRecord {
            episode: "ep1".into(),
            class: "chat:default".into(),
            chosen: Chosen {
                provider: provider.into(),
                model: model.into(),
                effort: None,
            },
            used: Chosen {
                provider: provider.into(),
                model: model.into(),
                effort: None,
            },
            rule_ids: rules.iter().map(|s| (*s).to_string()).collect(),
            reason: reason.into(),
            outcome: Some(RouteOutcome {
                ok: true,
                tokens: RouteTokens {
                    input: 100,
                    cached: 40,
                    out: 50,
                    reasoning: 0,
                },
                cost_usd: 0.001,
                ..RouteOutcome::default()
            }),
            ..RouteRecord::default()
        }
    }

    #[test]
    fn local_model_ids_are_local_lane() {
        assert_eq!(lane_of("local", "local:small"), Lane::Local);
        assert_eq!(lane_of("xai", "local:qwen"), Lane::Local);
        assert_eq!(lane_of("xai", "grok-4"), Lane::Frontier);
        assert_eq!(lane_of("anthropic", "claude-opus"), Lane::Frontier);
    }

    #[test]
    fn day_key_is_utc_ymd() {
        // 2026-11-26 00:00:00 UTC
        assert_eq!(day_key(1_795_651_200_000), "2026-11-26");
        // end of that day
        assert_eq!(day_key(1_795_651_200_000 + DAY_MS - 1), "2026-11-26");
        assert_eq!(day_key(1_795_651_200_000 + DAY_MS), "2026-11-27");
    }

    #[test]
    fn event_marks_escalation_and_frontier() {
        let r = rec(
            "xai",
            "grok-4",
            &["local:escalate_verify"],
            "verify failed, moved up",
        );
        let e = event_from(&r, "/home/user/my-app", 1_795_651_200_000, 0.001);
        assert_eq!(e.lane, Lane::Frontier);
        assert!(e.escalated);
        assert_eq!(e.project, "/home/user/my-app");
        assert_eq!(e.day, "2026-11-26");
        assert_eq!(e.tokens.input, 100);
        assert_eq!(e.reason, "verify failed, moved up");
        assert_eq!(e.rules, vec!["local:escalate_verify".to_string()]);
    }

    #[test]
    fn empty_project_becomes_app_sentinel() {
        let r = rec("local", "local:small", &[], "on device");
        let e = event_from(&r, "  ", 1_795_651_200_000, 0.0);
        assert_eq!(e.project, APP_PROJECT);
        assert_eq!(e.lane, Lane::Local);
        assert!(!e.escalated);
    }

    #[test]
    fn rollup_and_week_line_count_local_share_and_escalations() {
        let mut store = Store {
            schema: SCHEMA,
            days: BTreeMap::new(),
        };
        let now = 1_795_651_200_000u64; // 2026-11-26
        let local = Event {
            ts_ms: now,
            day: day_key(now),
            project: "/home/user/a".into(),
            episode: "e1".into(),
            lane: Lane::Local,
            provider: "local".into(),
            model: "local:small".into(),
            class: "chat:default".into(),
            escalated: false,
            reason: "on device".into(),
            tokens: RouteTokens {
                input: 10,
                cached: 0,
                out: 5,
                reasoning: 0,
            },
            cost_usd: 0.0,
            rules: vec![],
        };
        let frontier = Event {
            lane: Lane::Frontier,
            provider: "xai".into(),
            model: "grok-4".into(),
            escalated: true,
            reason: "verify escalate".into(),
            tokens: RouteTokens {
                input: 20,
                cached: 0,
                out: 10,
                reasoning: 0,
            },
            rules: vec!["local:escalate_verify".into()],
            ..local.clone()
        };
        store.note(&local);
        store.note(&local);
        store.note(&frontier);
        let week = week_line(&store, now, ALL_PROJECT);
        assert_eq!(week.local_turns, 2);
        assert_eq!(week.frontier_turns, 1);
        assert_eq!(week.escalations, 1);
        assert_eq!(week.local_pct(), Some(66));
        assert_eq!(week.escalation_pct(), Some(33));
        assert_eq!(
            week.detail().as_deref(),
            Some("Local 66% · frontier escalations 33% this week")
        );
        let proj = week_line(&store, now, "/home/user/a");
        assert_eq!(proj.turns(), 3);
        assert_eq!(week_line(&store, now, "/home/user/other").turns(), 0);
        assert_eq!(
            week_line(&Store::default(), now, ALL_PROJECT).detail(),
            None
        );
    }

    #[test]
    fn day_cap_drops_oldest() {
        let mut store = Store {
            schema: SCHEMA,
            days: BTreeMap::new(),
        };
        let base = 1_795_651_200_000u64;
        for i in 0..(DAY_CAP as u64 + 3) {
            let ts = base + i * DAY_MS;
            store.note(&Event {
                ts_ms: ts,
                day: day_key(ts),
                project: APP_PROJECT.into(),
                episode: String::new(),
                lane: Lane::Local,
                provider: "local".into(),
                model: "local:small".into(),
                class: String::new(),
                escalated: false,
                reason: String::new(),
                tokens: RouteTokens::default(),
                cost_usd: 0.0,
                rules: vec![],
            });
        }
        assert_eq!(store.days.len(), DAY_CAP);
        assert!(!store.days.contains_key(&day_key(base)));
        assert!(store.days.contains_key(&day_key(base + 3 * DAY_MS)));
    }

    #[test]
    fn note_writes_jsonl_and_rollup_on_disk() {
        let dir = std::env::temp_dir().join(format!(
            "grokhub-frontier-usage-{}-{}",
            std::process::id(),
            grokhub_core::now_ms()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("dir");
        let _pin = crate::perm::ConfigGuard::set(&dir);
        let now = 1_795_651_200_000u64;
        let event = Event {
            ts_ms: now,
            day: day_key(now),
            project: "/home/user/my-app".into(),
            episode: "ep".into(),
            lane: Lane::Frontier,
            provider: "xai".into(),
            model: "grok-4".into(),
            class: "chat:default".into(),
            escalated: true,
            reason: "verify escalate".into(),
            tokens: RouteTokens {
                input: 40,
                cached: 0,
                out: 10,
                reasoning: 0,
            },
            cost_usd: 0.002,
            rules: vec!["local:escalate_verify".into()],
        };
        note(&dir, &event);
        let log = std::fs::read_to_string(log_path(&dir)).expect("log");
        assert!(log.contains("\"lane\":\"frontier\""));
        assert!(log.contains("\"escalated\":true"));
        assert!(log.contains("/home/user/my-app"));
        assert!(!log.contains("sk-"));
        let week = week_from_disk(&dir, now);
        assert_eq!(week.frontier_turns, 1);
        assert_eq!(week.escalations, 1);
        assert_eq!(
            week.detail().as_deref(),
            Some("Local 0% · frontier escalations 100% this week")
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
