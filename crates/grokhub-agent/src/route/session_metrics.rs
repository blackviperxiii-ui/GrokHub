//! D13: per-chat routing metrics. Keyed by the chat's episode id; counts and
//! model ids only, never text. Kept next to the other route state in the
//! models folder (`models/session_metrics.json`), capped at
//! [`SESSION_METRICS_CAP`] chats and written atomically.
//!
//! Per chat: the prompt-cache hit rate (cached prompt tokens over prompt
//! tokens, from the usage the client returns), model switches, escalations
//! and their precision (an escalated call that answered counts as precise).
//! [`summary`] is the pure read the eval and the tuner use. Nothing here is
//! shown in the app. Each chat also counts the malformed tool calls the loop
//! repaired, by model and tool ([`note_repair`]): silent repair helps some
//! models more than others, so it is kept per model, not just counted.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use grokhub_core::model_registry::store::{models_dir, read_json, write_json};

pub const SESSION_METRICS_FILE: &str = "session_metrics.json";
pub const SESSION_METRICS_SCHEMA: u32 = 1;
/// Chats kept; the one updated longest ago goes first.
pub const SESSION_METRICS_CAP: usize = 200;

/// Rules that move a call to a stronger model than the chat was on.
pub const ESCALATION_RULES: &[&str] = &[
    "model:stronger",
    "model:next",
    super::STEP_UP_RULE,
    "pin:upgrade",
    "local:escalate_verify",
    "local:escalate_hard",
];

/// The call escalated: one of [`ESCALATION_RULES`], or attention breaking a
/// chat's model mid-flow (`…:mid_flow`).
pub fn is_escalation(rule_ids: &[String]) -> bool {
    rule_ids
        .iter()
        .any(|r| ESCALATION_RULES.contains(&r.as_str()) || r.ends_with(":mid_flow"))
}

/// One chat's counters.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatMetrics {
    #[serde(default)]
    pub calls: u32,
    #[serde(default)]
    pub prompt_tokens: u64,
    #[serde(default)]
    pub cached_tokens: u64,
    /// Calls that went to another model than the chat's call before.
    #[serde(default)]
    pub switches: u32,
    #[serde(default)]
    pub escalations: u32,
    /// Escalated calls that answered.
    #[serde(default)]
    pub precise: u32,
    /// The model the chat's last call went to.
    #[serde(default)]
    pub last_model: String,
    #[serde(default)]
    pub updated_ms: u64,
    /// Tool calls the loop repaired (a lookup typed into the shell), by the
    /// model that wrote them, then by the tool they ran as.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub repairs: BTreeMap<String, BTreeMap<String, u32>>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionMetrics {
    #[serde(default)]
    pub schema: u32,
    #[serde(default)]
    pub chats: BTreeMap<String, ChatMetrics>,
}

/// What one finished call tells the metrics.
#[derive(Debug, Clone, Copy, Default)]
pub struct CallFacts<'a> {
    /// The model the call went to.
    pub model: &'a str,
    pub rule_ids: &'a [String],
    pub prompt_tokens: u64,
    pub cached_tokens: u64,
    pub ok: bool,
}

impl SessionMetrics {
    /// `chat`'s counters, making room for a new chat first.
    fn chat_mut(&mut self, chat: &str) -> &mut ChatMetrics {
        if self.chats.len() >= SESSION_METRICS_CAP && !self.chats.contains_key(chat) {
            let oldest = self
                .chats
                .iter()
                .min_by_key(|(id, c)| (c.updated_ms, (*id).clone()))
                .map(|(id, _)| id.clone());
            if let Some(id) = oldest {
                self.chats.remove(&id);
            }
        }
        self.chats.entry(chat.to_string()).or_default()
    }

    /// Fold one finished call into `chat`'s counters.
    pub fn note(&mut self, chat: &str, facts: &CallFacts<'_>, now_ms: u64) {
        if chat.is_empty() {
            return;
        }
        let c = self.chat_mut(chat);
        c.calls = c.calls.saturating_add(1);
        c.prompt_tokens = c.prompt_tokens.saturating_add(facts.prompt_tokens);
        c.cached_tokens = c
            .cached_tokens
            .saturating_add(facts.cached_tokens.min(facts.prompt_tokens));
        let model = facts.model.trim();
        if !c.last_model.is_empty() && !model.is_empty() && c.last_model != model {
            c.switches = c.switches.saturating_add(1);
        }
        if !model.is_empty() {
            c.last_model = model.to_string();
        }
        if is_escalation(facts.rule_ids) {
            c.escalations = c.escalations.saturating_add(1);
            if facts.ok {
                c.precise = c.precise.saturating_add(1);
            }
        }
        c.updated_ms = now_ms;
    }

    /// One tool call `model` wrote that the loop repaired into `tool`.
    pub fn note_repair(&mut self, chat: &str, model: &str, tool: &str, now_ms: u64) {
        if chat.is_empty() || tool.is_empty() {
            return;
        }
        let model = match model.trim() {
            "" => "unknown",
            m => m,
        };
        let c = self.chat_mut(chat);
        let n = c
            .repairs
            .entry(model.to_string())
            .or_default()
            .entry(tool.to_string())
            .or_insert(0);
        *n = n.saturating_add(1);
        c.updated_ms = now_ms;
    }
}

/// Every chat's counters added up, as rates.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct SessionSummary {
    pub chats: u32,
    pub calls: u32,
    /// Cached prompt tokens over prompt tokens, percent.
    pub cache_hit_pct: f64,
    pub switches: u32,
    pub switches_per_chat: f64,
    pub escalations: u32,
    /// Escalated calls over calls, percent.
    pub escalation_pct: f64,
    /// Escalated calls that answered over escalated calls, percent.
    pub precision_pct: f64,
}

fn pct(part: u64, whole: u64) -> f64 {
    if whole == 0 {
        0.0
    } else {
        part as f64 * 100.0 / whole as f64
    }
}

/// The metrics added up over every chat kept. Pure.
pub fn summary(m: &SessionMetrics) -> SessionSummary {
    let sum = |f: fn(&ChatMetrics) -> u64| m.chats.values().map(f).sum::<u64>();
    let calls = sum(|c| c.calls as u64);
    let switches = sum(|c| c.switches as u64);
    let escalations = sum(|c| c.escalations as u64);
    let chats = m.chats.len() as u64;
    SessionSummary {
        chats: chats as u32,
        calls: calls as u32,
        cache_hit_pct: pct(sum(|c| c.cached_tokens), sum(|c| c.prompt_tokens)),
        switches: switches as u32,
        switches_per_chat: if chats == 0 {
            0.0
        } else {
            switches as f64 / chats as f64
        },
        escalations: escalations as u32,
        escalation_pct: pct(escalations, calls),
        precision_pct: pct(sum(|c| c.precise as u64), escalations),
    }
}

pub fn metrics_path(config_dir: &Path) -> PathBuf {
    models_dir(config_dir).join(SESSION_METRICS_FILE)
}

pub fn load(config_dir: &Path) -> SessionMetrics {
    let mut m: SessionMetrics = read_json(&metrics_path(config_dir)).unwrap_or_default();
    if m.schema == 0 {
        m.schema = SESSION_METRICS_SCHEMA;
    }
    m
}

/// One writer at a time, so two calls finishing together both count.
static WRITE: Mutex<()> = Mutex::new(());

/// Fold one finished call into the file. Never fails the call.
pub fn note_call(config_dir: &Path, chat: &str, facts: &CallFacts<'_>, now_ms: u64) {
    if chat.is_empty() {
        return;
    }
    let _g = WRITE.lock().unwrap_or_else(|e| e.into_inner());
    let mut m = load(config_dir);
    m.note(chat, facts, now_ms);
    m.schema = SESSION_METRICS_SCHEMA;
    let _ = write_json(&metrics_path(config_dir), &m);
}

/// Fold one repaired tool call into the file. Never fails the call. This
/// crate's own tests write only into a pinned scratch config folder.
pub fn note_repair(config_dir: &Path, chat: &str, model: &str, tool: &str, now_ms: u64) {
    if chat.is_empty() || (cfg!(test) && !crate::perm::config_pinned()) {
        return;
    }
    let _g = WRITE.lock().unwrap_or_else(|e| e.into_inner());
    let mut m = load(config_dir);
    m.note_repair(chat, model, tool, now_ms);
    m.schema = SESSION_METRICS_SCHEMA;
    let _ = write_json(&metrics_path(config_dir), &m);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rules(r: &[&str]) -> Vec<String> {
        r.iter().map(|s| s.to_string()).collect()
    }

    fn call<'a>(model: &'a str, rule_ids: &'a [String], p: u64, c: u64, ok: bool) -> CallFacts<'a> {
        CallFacts {
            model,
            rule_ids,
            prompt_tokens: p,
            cached_tokens: c,
            ok,
        }
    }

    #[test]
    fn a_chat_counts_cache_hits_switches_and_precise_escalations() {
        let mut m = SessionMetrics::default();
        let keep = rules(&["episode:keep"]);
        let up = rules(&["pin:upgrade", "attn:escalate:steer:mid_flow"]);
        let stronger = rules(&["E15:next_model", "model:stronger"]);
        m.note("chat-a", &call("grok-4.3", &keep, 1_000, 0, true), 10);
        m.note("chat-a", &call("grok-4.3", &keep, 2_000, 1_500, true), 20);
        m.note("chat-a", &call("grok-4.7", &up, 3_000, 0, true), 30);
        m.note(
            "chat-a",
            &call("grok-4.7", &stronger, 2_000, 1_500, false),
            40,
        );
        let c = &m.chats["chat-a"];
        assert_eq!(
            (c.calls, c.prompt_tokens, c.cached_tokens, c.switches),
            (4, 8_000, 3_000, 1)
        );
        assert_eq!((c.escalations, c.precise), (2, 1));
        assert_eq!((c.last_model.as_str(), c.updated_ms), ("grok-4.7", 40));
        let s = summary(&m);
        assert_eq!(
            s,
            SessionSummary {
                chats: 1,
                calls: 4,
                cache_hit_pct: 37.5,
                switches: 1,
                switches_per_chat: 1.0,
                escalations: 2,
                escalation_pct: 50.0,
                precision_pct: 50.0,
            }
        );
    }

    #[test]
    fn repairs_are_counted_per_model_and_tool_and_round_trip() {
        let mut m = SessionMetrics::default();
        m.note_repair("chat-r", "openrouter/free-a", "web_search", 10);
        m.note_repair("chat-r", "openrouter/free-a", "web_search", 20);
        m.note_repair("chat-r", "grok-4.7", "web_fetch", 30);
        m.note_repair("chat-r", " ", "web_search", 40);
        m.note_repair("", "grok-4.7", "web_search", 50);
        let c = &m.chats["chat-r"];
        assert_eq!(c.repairs["openrouter/free-a"]["web_search"], 2);
        assert_eq!(c.repairs["grok-4.7"]["web_fetch"], 1);
        assert_eq!(c.repairs["unknown"]["web_search"], 1);
        assert_eq!((c.calls, c.updated_ms), (0, 40));
        assert_eq!(m.chats.len(), 1);
        let json = serde_json::to_string(&m).unwrap();
        let back: SessionMetrics = serde_json::from_str(&json).unwrap();
        assert_eq!(back, m);
        // A chat with no repairs writes no field, so old files read the same.
        let mut plain = SessionMetrics::default();
        plain.note("chat-p", &call("grok-4.3", &[], 10, 0, true), 1);
        assert!(!serde_json::to_string(&plain).unwrap().contains("repairs"));
    }

    #[test]
    fn escalation_rules_are_the_stronger_moves_only() {
        assert!(is_escalation(&rules(&["model:stronger"])));
        assert!(is_escalation(&rules(&["replan:step_up"])));
        assert!(is_escalation(&rules(&["attn:escalate:host:mid_flow"])));
        assert!(is_escalation(&rules(&["local:escalate_verify"])));
        assert!(!is_escalation(&rules(&["pin:downgrade", "pin:keep"])));
        assert!(!is_escalation(&rules(&["attn:escalate:host", "table"])));
        assert!(!is_escalation(&[]));
    }

    #[test]
    fn the_store_drops_the_chat_updated_longest_ago_at_the_cap() {
        let mut m = SessionMetrics::default();
        let none = rules(&[]);
        for i in 0..SESSION_METRICS_CAP as u64 {
            m.note(
                &format!("chat-{i:03}"),
                &call("grok-4.3", &none, 10, 5, true),
                100 + i,
            );
        }
        // chat-000 is touched again, so chat-001 is the oldest now.
        m.note("chat-000", &call("grok-4.3", &none, 10, 5, true), 1_000);
        m.note("chat-new", &call("grok-4.3", &none, 10, 5, true), 1_001);
        assert_eq!(SESSION_METRICS_CAP, 200);
        assert_eq!(m.chats.len(), 200);
        assert!(m.chats.contains_key("chat-000"));
        assert!(!m.chats.contains_key("chat-001"));
        assert!(m.chats.contains_key("chat-new"));
        // An empty chat id is never stored.
        m.note("", &call("grok-4.3", &none, 10, 5, true), 1_002);
        assert_eq!(m.chats.len(), 200);
    }

    #[test]
    fn note_call_writes_the_models_folder_file_with_counts_and_no_text() {
        let dir = std::env::temp_dir().join(format!("gh-sessm-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let up = rules(&["model:stronger"]);
        note_call(&dir, "ep-1", &call("grok-4.7", &up, 4_000, 3_000, true), 7);
        note_call(&dir, "", &call("grok-4.7", &up, 4_000, 3_000, true), 8);
        let path = metrics_path(&dir);
        assert!(path.ends_with("models/session_metrics.json"), "{path:?}");
        assert!(!path.with_extension("json.tmp").exists());
        let m = load(&dir);
        assert_eq!(m.schema, 1);
        assert_eq!(m.chats.len(), 1);
        let s = summary(&m);
        assert_eq!(
            (s.calls, s.cache_hit_pct, s.escalations, s.precision_pct),
            (1, 75.0, 1, 100.0)
        );
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("\"ep-1\""));
        assert!(!text.contains("model:stronger"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_empty_store_summarises_to_zeroes() {
        assert_eq!(
            summary(&SessionMetrics::default()),
            SessionSummary::default()
        );
    }
}
