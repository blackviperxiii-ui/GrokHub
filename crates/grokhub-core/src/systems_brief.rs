//! Weekly state-of-systems briefing. Deterministic aggregation of local
//! records into a decision-aid Digest card. Runs on the same Sunday night
//! slot as self-review (`dreamHour`); `/brief` regenerates on demand.
//! Pure: no files, no model, no network. The cabin injects inputs and posts.

use crate::organs::LocalClock;
use crate::self_review::{self_review_due, SkillStats, WEEK_MS};
use crate::what_changed::WhatChanged;

/// At most this many recommended next actions.
pub const MAX_ACTIONS: usize = 3;
/// Failures of one skill before it counts as recurring (not a one-off).
pub const RECURRING_MIN_FAILURES: usize = 3;
/// Digests kept in the Signals section.
pub const SIGNAL_CAP: usize = 5;
/// Source ids of systems-brief cards start with this.
pub const CARD_SOURCE_PREFIX: &str = "systems-brief:";
/// Pulse card title.
pub const CARD_TITLE: &str = "Weekly systems briefing";

/// Re-export the self-review due rule so callers share one schedule.
pub use crate::self_review::SELF_REVIEW_WEEKDAY;

/// The weekly briefing is due: Sunday, at or after the review hour, not run today.
pub fn systems_brief_due(
    last_day: Option<&str>,
    today: &str,
    clock: &LocalClock,
    night_hour: u32,
) -> bool {
    self_review_due(last_day, today, clock, night_hour)
}

/// One claim with a stable source id for "What changed" inspection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BriefClaim {
    pub text: String,
    /// e.g. `outcome:skill:weekly-report`, `health:service:0`, `local:none`.
    pub source: String,
}

/// A bounded next action (at most [`MAX_ACTIONS`] in a brief).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BriefAction {
    pub text: String,
    pub rationale: String,
    pub source: String,
}

/// Injected week snapshot. Built by the cabin from state it already holds.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BriefInputs {
    /// Local day key (`YYYY-MM-DD`) written into the store.
    pub day: String,
    pub generated_at: u64,
    /// Per-skill outcome stats for the week (`WEEK_MS`).
    pub skill_stats: Vec<SkillStats>,
    /// Failed doctor / automation / MCP checks (names or lines).
    pub failed_services: Vec<String>,
    pub services_checked: usize,
    pub cabin_update: Option<String>,
    /// Failing automation health lines.
    pub automation_health: Vec<String>,
    /// High-value Pulse digest titles from the week (caller filters).
    pub digest_titles: Vec<String>,
    /// Newest dream `(date, WhatChanged)`. Empty WhatChanged means quiet dream.
    pub last_dream: Option<(String, WhatChanged)>,
    /// Meaningful wins (applied self-review, ledger notes), newest first.
    pub progress_wins: Vec<String>,
    /// Exact blocked / waiting lines (parked hard approvals, etc.).
    pub blocked: Vec<String>,
    /// Questions still open in the Journal (`journal::open_questions`).
    pub open_decisions: Vec<String>,
}

/// One generated briefing. Every sentence in the summary maps to claims.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemsBrief {
    pub day: String,
    pub generated_at: u64,
    pub executive_summary: String,
    pub progress: Vec<BriefClaim>,
    pub reliability: Vec<BriefClaim>,
    pub blocked: Vec<BriefClaim>,
    pub decisions: Vec<BriefClaim>,
    pub signals: Vec<BriefClaim>,
    pub next_actions: Vec<BriefAction>,
    /// Inspectable source list behind every claim (flat).
    pub sources: Vec<BriefClaim>,
}

impl SystemsBrief {
    /// Full markdown for chat (`/brief`) and card `details`.
    pub fn render(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!("# {}\n\n", CARD_TITLE));
        out.push_str(&format!("Week of {}.\n\n", self.day));
        out.push_str("## Executive summary\n\n");
        out.push_str(&self.executive_summary);
        out.push_str("\n\n");
        section(&mut out, "Progress", &self.progress);
        section(&mut out, "Reliability", &self.reliability);
        section(&mut out, "Blocked and waiting", &self.blocked);
        section(&mut out, "Decisions", &self.decisions);
        section(&mut out, "Signals", &self.signals);
        out.push_str("## Recommended next actions\n\n");
        if self.next_actions.is_empty() {
            out.push_str("- None. Nothing local needs a decision this week.\n\n");
        } else {
            for (i, a) in self.next_actions.iter().enumerate() {
                out.push_str(&format!(
                    "{}. {} — {} (`{}`)\n",
                    i + 1,
                    a.text,
                    a.rationale,
                    a.source
                ));
            }
            out.push('\n');
        }
        out.push_str("## What changed\n\n");
        if self.sources.is_empty() {
            out.push_str("- No source records this week.\n");
        } else {
            for s in &self.sources {
                out.push_str(&format!("- {} (`{}`)\n", s.text, s.source));
            }
        }
        out
    }

    /// Short Pulse body (under digest clip). Summary + up to three actions.
    pub fn card_body(&self) -> String {
        let mut lines = vec![self.executive_summary.clone()];
        for (i, a) in self.next_actions.iter().take(MAX_ACTIONS).enumerate() {
            lines.push(format!("{}. {}", i + 1, a.text));
        }
        lines.join("\n")
    }

    pub fn source_id(&self) -> String {
        format!("{CARD_SOURCE_PREFIX}{}", self.day)
    }
}

fn section(out: &mut String, title: &str, claims: &[BriefClaim]) {
    out.push_str(&format!("## {title}\n\n"));
    if claims.is_empty() {
        out.push_str("- Nothing to report.\n\n");
        return;
    }
    for c in claims {
        out.push_str(&format!("- {} (`{}`)\n", c.text, c.source));
    }
    out.push('\n');
}

fn claim(text: impl Into<String>, source: impl Into<String>) -> BriefClaim {
    BriefClaim {
        text: text.into(),
        source: source.into(),
    }
}

/// Build a briefing from injected local records. Never invents urgency.
pub fn build_brief(input: &BriefInputs) -> SystemsBrief {
    let mut progress = Vec::new();
    let mut reliability = Vec::new();
    let mut blocked = Vec::new();
    let mut decisions = Vec::new();
    let mut signals = Vec::new();
    let mut sources = Vec::new();
    let mut actions = Vec::new();

    // Progress
    let ok_skills: Vec<&SkillStats> = input
        .skill_stats
        .iter()
        .filter(|s| s.runs > 0 && s.failures == 0)
        .collect();
    let decided: usize = input.skill_stats.iter().map(|s| s.runs).sum();
    let failed_runs: usize = input.skill_stats.iter().map(|s| s.failures).sum();
    if decided == 0 && input.progress_wins.is_empty() {
        progress.push(claim(
            "No completed task outcomes this week.",
            "outcome:none",
        ));
        sources.push(claim(
            "outcomes empty or outside the week window",
            "outcome:none",
        ));
    } else {
        if decided > 0 {
            let line = format!(
                "{decided} skill run{} recorded; {} needed a fix.",
                if decided == 1 { "" } else { "s" },
                failed_runs
            );
            progress.push(claim(line, "outcome:week"));
            sources.push(claim(
                format!("{decided} decided / {failed_runs} failed"),
                "outcome:week",
            ));
        }
        for w in input.progress_wins.iter().take(5) {
            let src = format!("win:{}", slug(w));
            progress.push(claim(w.clone(), src.clone()));
            sources.push(claim(w.clone(), src));
        }
        for s in ok_skills.iter().take(3) {
            let src = format!("outcome:skill:{}", s.skill);
            progress.push(claim(
                format!(
                    "{}: {} clean run{}",
                    s.skill,
                    s.runs,
                    if s.runs == 1 { "" } else { "s" }
                ),
                src.clone(),
            ));
            sources.push(claim(format!("{} ok", s.skill), src));
        }
    }

    // Reliability
    let recurring: Vec<&SkillStats> = input
        .skill_stats
        .iter()
        .filter(|s| s.failures >= RECURRING_MIN_FAILURES)
        .collect();
    let one_offs: Vec<&SkillStats> = input
        .skill_stats
        .iter()
        .filter(|s| s.failures > 0 && s.failures < RECURRING_MIN_FAILURES)
        .collect();
    if recurring.is_empty() && one_offs.is_empty() && input.failed_services.is_empty() {
        let checked = input.services_checked;
        reliability.push(claim(
            if checked == 0 {
                "No failure records and no service checks yet.".into()
            } else {
                format!("No recurring skill failures; all {checked} service checks ok.")
            },
            "reliability:ok",
        ));
        sources.push(claim("no recurring failures", "reliability:ok"));
    } else {
        for s in &recurring {
            let src = format!("outcome:fail:{}", s.skill);
            let top = s
                .reasons
                .iter()
                .max_by_key(|(_, n)| *n)
                .map(|(r, n)| format!(" ({r} ×{n})"))
                .unwrap_or_default();
            reliability.push(claim(
                format!(
                    "Recurring: {} failed {} of {} runs{top}.",
                    s.skill, s.failures, s.runs
                ),
                src.clone(),
            ));
            sources.push(claim(format!("{} {}/{}", s.skill, s.failures, s.runs), src));
        }
        for s in &one_offs {
            let src = format!("outcome:once:{}", s.skill);
            reliability.push(claim(
                format!(
                    "One-off: {} failed {} of {} (not enough to call a trend).",
                    s.skill, s.failures, s.runs
                ),
                src.clone(),
            ));
            sources.push(claim(
                format!("{} {}/{} once", s.skill, s.failures, s.runs),
                src,
            ));
        }
        for (i, name) in input.failed_services.iter().enumerate() {
            let src = format!("health:service:{i}");
            reliability.push(claim(format!("Service check: {name}"), src.clone()));
            sources.push(claim(name.clone(), src));
        }
    }
    for (i, line) in input.automation_health.iter().enumerate() {
        let src = format!("automation:{i}");
        reliability.push(claim(format!("Automation: {line}"), src.clone()));
        sources.push(claim(line.clone(), src));
    }
    if let Some(v) = &input.cabin_update {
        let src = "health:update";
        reliability.push(claim(
            format!(
                "{} update ready: {}.",
                crate::brand::product_name(),
                v.trim_start_matches('v')
            ),
            src,
        ));
        sources.push(claim(v.clone(), src));
    }

    // Blocked
    if input.blocked.is_empty() {
        blocked.push(claim(
            "Nothing local is waiting on your input.",
            "blocked:none",
        ));
        sources.push(claim("no blocked lines", "blocked:none"));
    } else {
        for (i, b) in input.blocked.iter().enumerate() {
            let src = format!("blocked:{i}");
            blocked.push(claim(b.clone(), src.clone()));
            sources.push(claim(b.clone(), src));
        }
    }

    // Decisions: the Journal's open questions
    if input.open_decisions.is_empty() {
        decisions.push(claim("No open questions in the Journal.", "decisions:none"));
        sources.push(claim("journal has no open questions", "decisions:none"));
    } else {
        for (i, d) in input.open_decisions.iter().enumerate() {
            let src = format!("decision:{i}");
            decisions.push(claim(d.clone(), src.clone()));
            sources.push(claim(d.clone(), src));
        }
    }

    // Signals
    if input.digest_titles.is_empty() {
        signals.push(claim(
            "No high-value Pulse digests mapped this week.",
            "pulse:none",
        ));
        sources.push(claim("no digest titles", "pulse:none"));
    } else {
        for (i, t) in input.digest_titles.iter().take(SIGNAL_CAP).enumerate() {
            let src = format!("pulse:digest:{i}");
            signals.push(claim(t.clone(), src.clone()));
            sources.push(claim(t.clone(), src));
        }
    }
    if let Some((date, changed)) = &input.last_dream {
        let src = "dream:latest";
        let text = if changed.is_empty() {
            format!("Last dream {date}: nothing changed.")
        } else {
            format!(
                "Last dream {date}: {} merged, {} retired.",
                changed.merged.len(),
                changed.removed.len()
            )
        };
        signals.push(claim(text.clone(), src));
        sources.push(claim(text, src));
    }

    // Actions (bounded, evidence-backed)
    for s in recurring.iter().take(MAX_ACTIONS) {
        if actions.len() >= MAX_ACTIONS {
            break;
        }
        actions.push(BriefAction {
            text: format!("Review skill {}", s.skill),
            rationale: format!("{} of {} runs needed a fix this week", s.failures, s.runs),
            source: format!("outcome:fail:{}", s.skill),
        });
    }
    if actions.len() < MAX_ACTIONS && !input.blocked.is_empty() {
        actions.push(BriefAction {
            text: "Clear the oldest blocked item".into(),
            rationale: input.blocked[0].clone(),
            source: "blocked:0".into(),
        });
    }
    if actions.len() < MAX_ACTIONS {
        if let Some(v) = &input.cabin_update {
            actions.push(BriefAction {
                text: "Run /update".into(),
                rationale: format!(
                    "{product} {} is ready",
                    v.trim_start_matches('v'),
                    product = crate::brand::product_name()
                ),
                source: "health:update".into(),
            });
        }
    }
    if actions.len() < MAX_ACTIONS && !input.automation_health.is_empty() {
        actions.push(BriefAction {
            text: "Fix or stop the failing automation".into(),
            rationale: input.automation_health[0].clone(),
            source: "automation:0".into(),
        });
    }

    let executive_summary = summarize(
        decided,
        failed_runs,
        recurring.len(),
        input.blocked.len(),
        input.failed_services.len() + input.automation_health.len(),
        input.cabin_update.is_some(),
        input.progress_wins.len(),
    );

    SystemsBrief {
        day: input.day.clone(),
        generated_at: input.generated_at,
        executive_summary,
        progress,
        reliability,
        blocked,
        decisions,
        signals,
        next_actions: actions,
        sources,
    }
}

fn summarize(
    decided: usize,
    failed_runs: usize,
    recurring: usize,
    blocked_n: usize,
    service_fails: usize,
    update_ready: bool,
    wins: usize,
) -> String {
    let mut parts = Vec::new();
    if decided == 0 && wins == 0 && blocked_n == 0 && service_fails == 0 && !update_ready {
        return "Quiet week: no local task outcomes, failures, or blocks to report. Nothing needs a decision.".into();
    }
    if decided > 0 {
        parts.push(format!(
            "{decided} skill run{} this week ({} needed a fix)",
            if decided == 1 { "" } else { "s" },
            failed_runs
        ));
    } else if wins > 0 {
        parts.push(format!(
            "{wins} recorded win{}",
            if wins == 1 { "" } else { "s" }
        ));
    } else {
        parts.push("No skill outcomes recorded".into());
    }
    if recurring > 0 {
        parts.push(format!(
            "{recurring} recurring failure{}",
            if recurring == 1 { "" } else { "s" }
        ));
    } else if failed_runs > 0 {
        parts.push("failures look like one-offs, not trends".into());
    } else {
        parts.push("reliability looks steady".into());
    }
    if blocked_n > 0 {
        parts.push(format!(
            "{blocked_n} item{} waiting on you",
            if blocked_n == 1 { "" } else { "s" }
        ));
    } else {
        parts.push("nothing blocked".into());
    }
    if service_fails > 0 {
        parts.push(format!(
            "{service_fails} service/automation issue{}",
            if service_fails == 1 { "" } else { "s" }
        ));
    }
    if update_ready {
        parts.push(format!(
            "a {} update is ready",
            crate::brand::product_name()
        ));
    }
    let mut s = parts.join("; ");
    s.push('.');
    // Cap to ~5 short sentences by keeping as one compound sentence + period.
    s
}

fn slug(s: &str) -> String {
    let words: Vec<String> = s
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_ascii_lowercase)
        .take(4)
        .collect();
    let mut out = words.join("-");
    out.truncate(40);
    if out.is_empty() {
        "item".into()
    } else {
        out
    }
}

/// Week window constant shared with self-review.
pub fn week_ms() -> u64 {
    WEEK_MS
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::organs::LocalClock;
    use std::collections::BTreeMap;

    fn sunday(hour: u32) -> LocalClock {
        LocalClock {
            now_ms: 1,
            weekday: 0,
            hour,
            minute: 0,
        }
    }

    fn skill(name: &str, runs: usize, failures: usize) -> SkillStats {
        let mut reasons = BTreeMap::new();
        if failures > 0 {
            reasons.insert("finding".into(), failures);
        }
        SkillStats {
            skill: name.into(),
            runs,
            failures,
            reasons,
        }
    }

    #[test]
    fn due_only_on_sunday_after_the_hour_once() {
        assert!(!systems_brief_due(None, "2026-10-12", &sunday(20), 21));
        assert!(systems_brief_due(None, "2026-10-12", &sunday(21), 21));
        assert!(!systems_brief_due(
            Some("2026-10-12"),
            "2026-10-12",
            &sunday(22),
            21
        ));
        let monday = LocalClock {
            now_ms: 1,
            weekday: 1,
            hour: 22,
            minute: 0,
        };
        assert!(!systems_brief_due(None, "2026-10-13", &monday, 21));
    }

    #[test]
    fn empty_week_is_honest_and_short() {
        let brief = build_brief(&BriefInputs {
            day: "2026-10-12".into(),
            generated_at: 100,
            services_checked: 4,
            ..Default::default()
        });
        assert!(brief.executive_summary.contains("Quiet week"));
        assert!(brief.progress[0].text.contains("No completed"));
        assert!(brief.reliability[0].text.contains("all 4 service"));
        assert!(brief.blocked[0].source == "blocked:none");
        assert_eq!(brief.decisions[0].text, "No open questions in the Journal.");
        assert_eq!(brief.decisions[0].source, "decisions:none");
        assert!(brief.next_actions.is_empty());
        assert!(brief.render().contains("## What changed"));
        assert!(brief.sources.iter().all(|s| !s.source.is_empty()));
    }

    #[test]
    fn productive_week_traces_claims_to_sources() {
        let brief = build_brief(&BriefInputs {
            day: "2026-10-12".into(),
            generated_at: 100,
            skill_stats: vec![skill("weekly-report", 5, 0), skill("invoice", 3, 0)],
            progress_wins: vec!["Applied skill patch for notes".into()],
            services_checked: 6,
            digest_titles: vec!["Rust 1.92 ships".into()],
            last_dream: Some((
                "2026-10-11".into(),
                WhatChanged {
                    merged: vec!["Uses pnpm".into()],
                    removed: vec!["old path".into()],
                    removed_word: "retired",
                    ..Default::default()
                },
            )),
            ..Default::default()
        });
        assert!(brief.executive_summary.contains("8 skill runs"));
        assert!(brief.executive_summary.contains("reliability looks steady"));
        assert_eq!(brief.next_actions.len(), 0);
        assert!(brief.signals.iter().any(|c| c.text.contains("Rust 1.92")));
        assert!(brief.signals.iter().any(|c| c.text.contains("1 merged")));
        for c in brief
            .progress
            .iter()
            .chain(brief.reliability.iter())
            .chain(brief.signals.iter())
        {
            assert!(
                brief.sources.iter().any(|s| s.source == c.source),
                "orphan claim {}",
                c.source
            );
        }
        assert!(brief.card_body().contains("8 skill runs"));
        assert_eq!(brief.source_id(), "systems-brief:2026-10-12");
    }

    #[test]
    fn blocked_week_surfaces_exact_wait_and_caps_actions() {
        let brief = build_brief(&BriefInputs {
            day: "2026-10-12".into(),
            generated_at: 100,
            skill_stats: vec![skill("deploy", 6, 4), skill("lint", 2, 1)],
            blocked: vec![
                "Approve: send invoice email (hard)".into(),
                "Need API key for Stripe".into(),
            ],
            cabin_update: Some("v3.0.2".into()),
            automation_health: vec!["night-check failed twice".into()],
            failed_services: vec!["xAI auth missing".into()],
            services_checked: 5,
            ..Default::default()
        });
        assert!(brief
            .reliability
            .iter()
            .any(|c| c.text.contains("Recurring: deploy")));
        assert!(brief
            .reliability
            .iter()
            .any(|c| c.text.contains("One-off: lint")));
        assert!(brief
            .reliability
            .iter()
            .any(|c| c.text.contains("not enough to call a trend")));
        assert_eq!(brief.next_actions.len(), MAX_ACTIONS);
        assert_eq!(brief.next_actions[0].text, "Review skill deploy");
        assert!(brief
            .next_actions
            .iter()
            .any(|a| a.text.contains("blocked")));
        assert!(brief.executive_summary.contains("waiting on you"));
        assert!(brief.render().contains("Approve: send invoice email"));
    }

    #[test]
    fn many_news_items_cap_signals() {
        let titles: Vec<String> = (0..20).map(|i| format!("Story {i}")).collect();
        let brief = build_brief(&BriefInputs {
            day: "2026-10-12".into(),
            generated_at: 1,
            digest_titles: titles,
            ..Default::default()
        });
        let pulse = brief
            .signals
            .iter()
            .filter(|c| c.source.starts_with("pulse:digest:"))
            .count();
        assert_eq!(pulse, SIGNAL_CAP);
    }

    #[test]
    fn conflicting_and_stale_still_cite_only_injected_sources() {
        // Stale/conflicting: caller injects both a clean skill and a fail line;
        // briefing must not invent a causal link — only list both with sources.
        let brief = build_brief(&BriefInputs {
            day: "2026-10-12".into(),
            generated_at: 1,
            skill_stats: vec![skill("notes", 4, 0)],
            failed_services: vec!["MCP github: timed out".into()],
            open_decisions: vec!["Ship beta → main?".into()],
            ..Default::default()
        });
        assert!(brief.progress.iter().any(|c| c.text.contains("notes")));
        assert!(brief
            .reliability
            .iter()
            .any(|c| c.source == "health:service:0"));
        assert_eq!(brief.decisions[0].text, "Ship beta → main?");
        assert!(!brief.executive_summary.to_lowercase().contains("because"));
    }

    #[test]
    fn same_inputs_same_brief() {
        let input = BriefInputs {
            day: "2026-10-12".into(),
            generated_at: 42,
            skill_stats: vec![skill("a", 2, 2)],
            services_checked: 1,
            ..Default::default()
        };
        assert_eq!(build_brief(&input), build_brief(&input));
    }
}
