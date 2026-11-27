//! Per-skill runtime invocation metrics. Keyed by skill id; counts, turns,
//! tool calls, tokens, cost ticks, and stop reasons only, never prompt text.
//! Stored atomically in `models/skill_metrics.json` capped at
//! [`SKILL_METRICS_CAP`] skills so Skill Curator can track skill efficiency
//! without disturbing [`super::session_metrics`].

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use grokhub_core::model_registry::store::{models_dir, read_json, write_json};

pub const SKILL_METRICS_FILE: &str = "skill_metrics.json";
pub const SKILL_METRICS_SCHEMA: u32 = 1;
/// Distinct skills kept; the least recently used goes first.
pub const SKILL_METRICS_CAP: usize = 200;

const TICKS_PER_USD: f64 = 10_000_000_000.0;

/// Map a loop [`crate::StopReason`] into a compact label for skill telemetry.
pub fn stop_reason_label(stop: &crate::StopReason) -> &'static str {
    match stop {
        crate::StopReason::EndTurn => "end_turn",
        crate::StopReason::MaxTurns => "max_turns",
        crate::StopReason::Cancelled => "cancelled",
        crate::StopReason::Halted => "halted",
        crate::StopReason::Error(_) => "error",
    }
}

/// Cumulative runtime telemetry for one skill.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillStats {
    #[serde(default)]
    pub invocations: u32,
    #[serde(default)]
    pub passed: u32,
    #[serde(default)]
    pub failed: u32,
    #[serde(default)]
    pub turns: u64,
    #[serde(default)]
    pub tool_calls: u64,
    #[serde(default)]
    pub input_tokens: u64,
    #[serde(default)]
    pub output_tokens: u64,
    #[serde(default)]
    pub cost_in_usd_ticks: i64,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub stop_reasons: BTreeMap<String, u32>,
    #[serde(default)]
    pub last_used_ms: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillMetrics {
    #[serde(default)]
    pub schema: u32,
    #[serde(default)]
    pub skills: BTreeMap<String, SkillStats>,
}

/// Telemetry from one finished agent turn that loaded one or more skills.
#[derive(Debug, Clone, Copy, Default)]
pub struct SkillTurnFacts<'a> {
    pub skill_id: &'a str,
    pub turns: u32,
    pub tool_calls: u32,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cost_in_usd_ticks: i64,
    pub stop_reason: &'a str,
    pub passed: bool,
}

/// Flattened summary row for one tracked skill.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SkillSummaryRow {
    pub skill_id: String,
    pub invocations: u32,
    pub passed: u32,
    pub failed: u32,
    pub pass_rate: f64,
    pub avg_turns: f64,
    pub avg_tool_calls: f64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cost_usd: f64,
    pub last_used_ms: u64,
}

impl SkillMetrics {
    /// Entry for `skill_id`, evicting the oldest skill first if at capacity.
    fn skill_mut(&mut self, skill_id: &str) -> &mut SkillStats {
        if self.skills.len() >= SKILL_METRICS_CAP && !self.skills.contains_key(skill_id) {
            let oldest = self
                .skills
                .iter()
                .min_by_key(|(id, stats)| (stats.last_used_ms, (*id).clone()))
                .map(|(id, _)| id.clone());
            if let Some(id) = oldest {
                self.skills.remove(&id);
            }
        }
        self.skills.entry(skill_id.to_string()).or_default()
    }

    /// Fold one completed agent turn into `facts.skill_id`.
    pub fn note(&mut self, facts: &SkillTurnFacts<'_>, now_ms: u64) {
        let skill_id = facts.skill_id.trim();
        if skill_id.is_empty() {
            return;
        }
        let s = self.skill_mut(skill_id);
        s.invocations = s.invocations.saturating_add(1);
        if facts.passed {
            s.passed = s.passed.saturating_add(1);
        } else {
            s.failed = s.failed.saturating_add(1);
        }
        s.turns = s.turns.saturating_add(u64::from(facts.turns));
        s.tool_calls = s.tool_calls.saturating_add(u64::from(facts.tool_calls));
        s.input_tokens = s.input_tokens.saturating_add(facts.input_tokens);
        s.output_tokens = s.output_tokens.saturating_add(facts.output_tokens);
        s.cost_in_usd_ticks = s.cost_in_usd_ticks.saturating_add(facts.cost_in_usd_ticks);
        let reason = facts.stop_reason.trim();
        if !reason.is_empty() {
            let count = s.stop_reasons.entry(reason.to_string()).or_insert(0);
            *count = count.saturating_add(1);
        }
        s.last_used_ms = now_ms;
    }

    /// Per-skill summary rows sorted by skill id.
    pub fn summary(&self) -> Vec<SkillSummaryRow> {
        self.skills
            .iter()
            .map(|(skill_id, s)| {
                let denom = f64::from(s.invocations);
                SkillSummaryRow {
                    skill_id: skill_id.clone(),
                    invocations: s.invocations,
                    passed: s.passed,
                    failed: s.failed,
                    pass_rate: if s.invocations == 0 {
                        0.0
                    } else {
                        f64::from(s.passed) / denom
                    },
                    avg_turns: if s.invocations == 0 {
                        0.0
                    } else {
                        s.turns as f64 / denom
                    },
                    avg_tool_calls: if s.invocations == 0 {
                        0.0
                    } else {
                        s.tool_calls as f64 / denom
                    },
                    input_tokens: s.input_tokens,
                    output_tokens: s.output_tokens,
                    cost_usd: s.cost_in_usd_ticks as f64 / TICKS_PER_USD,
                    last_used_ms: s.last_used_ms,
                }
            })
            .collect()
    }
}

pub fn metrics_path(config_dir: &Path) -> PathBuf {
    models_dir(config_dir).join(SKILL_METRICS_FILE)
}

pub fn load(config_dir: &Path) -> SkillMetrics {
    let mut m: SkillMetrics = read_json(&metrics_path(config_dir)).unwrap_or_default();
    if m.schema == 0 {
        m.schema = SKILL_METRICS_SCHEMA;
    }
    m
}

static WRITE: Mutex<()> = Mutex::new(());

/// Fold one skill-using turn into `models/skill_metrics.json`. Never fails the turn.
pub fn note_turn(config_dir: &Path, facts: &SkillTurnFacts<'_>, now_ms: u64) {
    if facts.skill_id.trim().is_empty() || (cfg!(test) && !crate::perm::config_pinned()) {
        return;
    }
    let _g = WRITE.lock().unwrap_or_else(|e| e.into_inner());
    let mut m = load(config_dir);
    m.note(facts, now_ms);
    m.schema = SKILL_METRICS_SCHEMA;
    let _ = write_json(&metrics_path(config_dir), &m);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fact<'a>(
        skill_id: &'a str,
        turns: u32,
        tool_calls: u32,
        input_tokens: u64,
        output_tokens: u64,
        cost_in_usd_ticks: i64,
        stop_reason: &'a str,
        passed: bool,
    ) -> SkillTurnFacts<'a> {
        SkillTurnFacts {
            skill_id,
            turns,
            tool_calls,
            input_tokens,
            output_tokens,
            cost_in_usd_ticks,
            stop_reason,
            passed,
        }
    }

    #[test]
    fn notes_multiple_skills_and_computes_summary() {
        let mut m = SkillMetrics::default();
        m.note(
            &fact(
                "release-check",
                2,
                3,
                1_200,
                300,
                500_000_000,
                "end_turn",
                true,
            ),
            10,
        );
        m.note(
            &fact(
                "release-check",
                4,
                5,
                2_000,
                500,
                1_500_000_000,
                "max_turns",
                false,
            ),
            20,
        );
        m.note(
            &fact(
                "schema-audit",
                1,
                1,
                800,
                200,
                250_000_000,
                "end_turn",
                true,
            ),
            30,
        );
        m.note(&fact("  ", 1, 1, 100, 10, 0, "end_turn", true), 40);

        assert_eq!(m.skills.len(), 2);
        let rc = &m.skills["release-check"];
        assert_eq!(rc.invocations, 2);
        assert_eq!(rc.passed, 1);
        assert_eq!(rc.failed, 1);
        assert_eq!(rc.turns, 6);
        assert_eq!(rc.tool_calls, 8);
        assert_eq!(rc.input_tokens, 3_200);
        assert_eq!(rc.output_tokens, 800);
        assert_eq!(rc.cost_in_usd_ticks, 2_000_000_000);
        assert_eq!(rc.stop_reasons["end_turn"], 1);
        assert_eq!(rc.stop_reasons["max_turns"], 1);
        assert_eq!(rc.last_used_ms, 20);

        let rows = m.summary();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].skill_id, "release-check");
        assert_eq!(rows[0].pass_rate, 0.5);
        assert_eq!(rows[0].avg_turns, 3.0);
        assert_eq!(rows[0].avg_tool_calls, 4.0);
        assert!((rows[0].cost_usd - 0.2).abs() < 1e-12);
        assert_eq!(rows[1].skill_id, "schema-audit");
        assert_eq!(rows[1].pass_rate, 1.0);
        assert!((rows[1].cost_usd - 0.025).abs() < 1e-12);
    }

    #[test]
    fn evicts_oldest_skill_at_cap_and_round_trips_json() {
        let mut m = SkillMetrics::default();
        for i in 0..SKILL_METRICS_CAP as u64 {
            m.note(
                &fact(&format!("skill-{i:03}"), 1, 1, 10, 5, 100, "end_turn", true),
                100 + i,
            );
        }
        m.note(
            &fact("skill-000", 1, 1, 10, 5, 100, "end_turn", true),
            1_000,
        );
        m.note(
            &fact("skill-new", 1, 1, 10, 5, 100, "end_turn", true),
            1_001,
        );
        assert_eq!(m.skills.len(), SKILL_METRICS_CAP);
        assert!(m.skills.contains_key("skill-000"));
        assert!(!m.skills.contains_key("skill-001"));
        assert!(m.skills.contains_key("skill-new"));

        let encoded = serde_json::to_string(&m).unwrap();
        let decoded: SkillMetrics = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, m);
    }

    #[test]
    fn note_turn_writes_models_file_atomically_without_touching_session_metrics() {
        let dir = std::env::temp_dir().join(format!("gh-skillm-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let _guard = crate::perm::ConfigGuard::set(&dir);

        note_turn(
            &dir,
            &fact(
                "changelog-entry",
                2,
                2,
                1_500,
                400,
                1_000_000_000,
                "end_turn",
                true,
            ),
            77,
        );
        let path = metrics_path(&dir);
        assert!(path.ends_with("models/skill_metrics.json"), "{path:?}");
        assert!(!path.with_extension("json.tmp").exists());
        assert!(!super::super::session_metrics::metrics_path(&dir).exists());

        let loaded = load(&dir);
        assert_eq!(loaded.schema, SKILL_METRICS_SCHEMA);
        assert_eq!(loaded.skills["changelog-entry"].invocations, 1);
        assert_eq!(loaded.skills["changelog-entry"].turns, 2);
        assert_eq!(loaded.skills["changelog-entry"].tool_calls, 2);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
