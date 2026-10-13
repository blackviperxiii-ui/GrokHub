//! Weekly state-of-systems briefing: cabin glue. Aggregates local records,
//! stores the latest brief, posts one Pulse Digest card on Sunday night (or
//! from `/brief`). Does not call a model or create work without a click.

use grokhub_core::outcome;
use grokhub_core::self_review as sr;
use grokhub_core::systems_brief::{self as sb, BriefInputs, SystemsBrief, CARD_TITLE};
use grokhub_core::update_feed::{self as feed, UpdateKind};
use grokhub_core::what_changed as wc;
use serde::{Deserialize, Serialize};

use super::Cabin;
use crate::config;

pub(super) const BRIEF_FILE: &str = "systems_brief.json";

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct BriefFile {
    #[serde(default)]
    pub last_day: Option<String>,
    /// Full markdown of the newest brief (for later review).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest_md: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest_day: Option<String>,
    #[serde(default)]
    pub latest_at: u64,
}

fn state_path() -> std::path::PathBuf {
    config::config_dir().join(BRIEF_FILE)
}

pub(super) fn load_state() -> BriefFile {
    std::fs::read_to_string(state_path())
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

pub(super) fn save_state(s: &BriefFile) {
    if let Ok(text) = serde_json::to_string_pretty(s) {
        let _ = std::fs::write(state_path(), text);
    }
}

impl Cabin {
    /// Sunday night slot: one briefing after the hour, once per local day.
    pub(super) fn systems_brief_tonight(
        &mut self,
        today: &str,
        clock: &grokhub_core::LocalClock,
        now: u64,
    ) -> bool {
        let state = load_state();
        if !sb::systems_brief_due(state.last_day.as_deref(), today, clock, self.dream_hour()) {
            return false;
        }
        self.run_systems_brief_now(today, now, false);
        true
    }

    /// Build, store, and post (or refresh) the briefing. `force` skips the
    /// Sunday gate so `/brief` can regenerate from the same inputs.
    pub(super) fn run_systems_brief_now(&mut self, today: &str, now: u64, force: bool) {
        let mut state = load_state();
        if !force && state.last_day.as_deref() == Some(today) {
            return;
        }
        let brief = self.build_systems_brief(today, now);
        let md = brief.render();
        state.last_day = Some(today.to_string());
        state.latest_day = Some(brief.day.clone());
        state.latest_md = Some(md.clone());
        state.latest_at = now;
        save_state(&state);
        self.post_systems_brief_card(&brief, &md, now);
    }

    /// Pure aggregation from cabin state + disk outcomes (no model).
    pub(super) fn build_systems_brief(&self, today: &str, now: u64) -> SystemsBrief {
        let dir = config::config_dir();
        let outcomes = outcome::read_outcomes(&dir);
        let skill_stats = sr::skill_stats(&outcomes, now, sb::week_ms());
        let health = self.health_input();
        let automation_health: Vec<String> = self
            .automations
            .iter()
            .filter_map(grokhub_core::automation_health_line)
            .collect();
        let digest_titles = week_digest_titles(&self.updates, now);
        let last_dream = health
            .last_dream
            .as_ref()
            .map(|(date, report)| (date.clone(), wc::from_dream(report)));
        let progress_wins = recent_progress_wins(&self.updates, now);
        let blocked = blocked_lines(self);
        let input = BriefInputs {
            day: today.to_string(),
            generated_at: now,
            skill_stats,
            failed_services: health.failed.clone(),
            services_checked: health.checked,
            cabin_update: health.cabin_update.clone(),
            automation_health,
            digest_titles,
            last_dream,
            progress_wins,
            blocked,
            open_decisions: Vec::new(),
        };
        sb::build_brief(&input)
    }

    fn post_systems_brief_card(&mut self, brief: &SystemsBrief, details: &str, now: u64) {
        let source = brief.source_id();
        // Drop a prior live card for this week so regenerate replaces it.
        self.updates.retain(|c| {
            !(c.source_id == source
                && c.kind == UpdateKind::Digest
                && c.status != feed::UpdateStatus::Dismissed)
        });
        let mut card = feed::digest_card(&source, CARD_TITLE, &brief.card_body(), now);
        card.details = Some(details.to_string());
        grokhub_core::hold_if_quiet(&mut card, self.quiet_now());
        self.updates.insert(0, card);
        self.persist_updates();
    }

    /// `/brief`: regenerate and show the full markdown in chat.
    pub(super) fn run_systems_brief_slash(&mut self) {
        let now = grokhub_core::now_ms();
        let today = Self::local_day();
        self.run_systems_brief_now(&today, now, true);
        let state = load_state();
        let text = state.latest_md.unwrap_or_else(|| "No briefing yet.".into());
        self.live_mut()
            .push(("assistant".into(), grokhub_core::mark_slash_result(&text)));
        self.stamp_current_access();
        self.persist();
        self.status = format!("{CARD_TITLE} · {}", state.latest_day.unwrap_or(today));
    }
}

fn week_digest_titles(cards: &[feed::UpdateCard], now: u64) -> Vec<String> {
    let mut out = Vec::new();
    for c in cards {
        if c.kind != UpdateKind::Digest {
            continue;
        }
        if c.source_id.starts_with(sb::CARD_SOURCE_PREFIX) {
            continue;
        }
        if c.created_at > now || now.saturating_sub(c.created_at) > sb::week_ms() {
            continue;
        }
        if c.status == feed::UpdateStatus::Dismissed {
            continue;
        }
        let title = c.title.trim();
        if title.is_empty() {
            continue;
        }
        out.push(title.to_string());
        if out.len() >= sb::SIGNAL_CAP {
            break;
        }
    }
    out
}

fn recent_progress_wins(cards: &[feed::UpdateCard], now: u64) -> Vec<String> {
    let mut out = Vec::new();
    for c in cards {
        if c.kind != UpdateKind::SelfChange && c.kind != UpdateKind::DoneForYou {
            continue;
        }
        if c.created_at > now || now.saturating_sub(c.created_at) > sb::week_ms() {
            continue;
        }
        let title = c.title.trim();
        if !title.is_empty() {
            out.push(title.to_string());
        }
        if out.len() >= 5 {
            break;
        }
    }
    out
}

fn blocked_lines(cabin: &Cabin) -> Vec<String> {
    let mut out = Vec::new();
    // Parked hard asks waiting on the user (permission cards in the live thread).
    for m in cabin.messages.iter().rev().take(40) {
        if m.0 != "assistant" {
            continue;
        }
        let t = m.1.as_str();
        if t.contains("parked") || t.contains("Needs approval") || t.contains("hard class") {
            let line = t.lines().next().unwrap_or(t).trim();
            if !line.is_empty() {
                out.push(grokhub_core::redact_secrets(line));
            }
        }
        if out.len() >= 5 {
            break;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use grokhub_core::systems_brief::BriefInputs;
    use grokhub_core::update_feed::UpdateKind;

    #[test]
    fn night_and_manual_match_on_same_inputs() {
        // The pure builder is the shared path; cabin wrappers only gather.
        let a = sb::build_brief(&BriefInputs {
            day: "2026-10-12".into(),
            generated_at: 9,
            services_checked: 2,
            ..Default::default()
        });
        let b = sb::build_brief(&BriefInputs {
            day: "2026-10-12".into(),
            generated_at: 9,
            services_checked: 2,
            ..Default::default()
        });
        assert_eq!(a.render(), b.render());
    }

    #[test]
    fn sunday_night_posts_one_digest_and_slash_regenerates() {
        let root = crate::config::test_config_root("systems-brief");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("config root");
        let _pin = crate::config::TestConfigDir::set(root.clone());
        std::env::set_var("GROKHUB_CONFIG", &root);
        let mut cabin = Cabin::quiet_for_test();
        cabin.cfg.dream_hour = 21;
        let now = 1_700_000_000_000u64;
        let sunday = grokhub_core::LocalClock {
            now_ms: now,
            weekday: 0,
            hour: 21,
            minute: 0,
        };
        let today = "2026-10-12";
        let _ = cabin.night_passes(today, &sunday, now);
        let state = load_state();
        assert_eq!(state.last_day.as_deref(), Some(today));
        assert!(
            state
                .latest_md
                .as_deref()
                .unwrap_or("")
                .contains("Weekly systems briefing"),
            "{:?}",
            state.latest_md
        );
        let digests: Vec<_> = cabin
            .updates
            .iter()
            .filter(|c| {
                c.kind == UpdateKind::Digest && c.source_id.starts_with(sb::CARD_SOURCE_PREFIX)
            })
            .collect();
        assert_eq!(digests.len(), 1, "{:?}", cabin.updates);
        assert_eq!(digests[0].title, CARD_TITLE);
        assert!(digests[0]
            .details
            .as_ref()
            .unwrap()
            .contains("## What changed"));

        // Second Sunday tick does not duplicate.
        let _ = cabin.night_passes(today, &sunday, now + 60_000);
        let again: usize = cabin
            .updates
            .iter()
            .filter(|c| c.source_id.starts_with(sb::CARD_SOURCE_PREFIX))
            .count();
        assert_eq!(again, 1);

        // Manual /brief regenerates from the same gather path.
        cabin.run_systems_brief_now(today, now + 120_000, true);
        let forced: usize = cabin
            .updates
            .iter()
            .filter(|c| {
                c.kind == UpdateKind::Digest && c.source_id.starts_with(sb::CARD_SOURCE_PREFIX)
            })
            .count();
        assert_eq!(forced, 1);
        drop(cabin);
        drop(_pin);
        let _ = std::fs::remove_dir_all(&root);
        std::env::remove_var("GROKHUB_CONFIG");
    }
}
