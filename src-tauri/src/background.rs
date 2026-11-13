//! Settings → Background (shell parity): what runs on its own, from the
//! `automations.json` the cabin shares with this app. This app has no
//! scheduler of its own yet: the cabin fires due jobs while it is open, and
//! here a job runs only on Run now. The page says who fires jobs, the next one
//! due, when each last ran, and the quiet hours and budget saved in Behavior.

use grokhub_core::Automation;
use serde::Serialize;

use crate::settings::Behavior;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Row {
    pub id: String,
    pub name: String,
    /// "Due now", "Next check in 12m", or "Paused".
    pub next: String,
    /// "Last ran 5m ago" or "Hasn't run yet".
    pub last: String,
    /// Red line when the last run failed.
    pub health: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Page {
    /// Who fires scheduled jobs today.
    pub runner: String,
    /// "Morning brief · Next check in 12m", while any job is on.
    pub next_due: Option<String>,
    /// "Quiet hours 22:00–07:00" or "No quiet hours".
    pub quiet: String,
    /// "Daily token budget: Off".
    pub budget: String,
    pub rows: Vec<Row>,
}

fn span(ms: u64) -> String {
    let secs = ms.div_ceil(1000);
    match secs {
        0..60 => format!("{secs}s"),
        60..3_600 => format!("{}m", secs.div_ceil(60)),
        3_600..172_800 => format!("{}h", secs / 3_600),
        _ => format!("{}d", secs / 86_400),
    }
}

/// When a job runs next, in the cabin's words.
pub fn next_text(next_ms: u64, now_ms: u64) -> String {
    if next_ms <= now_ms {
        "Due now".into()
    } else {
        format!("Next check in {}", span(next_ms - now_ms))
    }
}

pub fn last_text(last_ms: Option<u64>, now_ms: u64) -> String {
    match last_ms {
        None => "Hasn't run yet".into(),
        Some(at) if now_ms.saturating_sub(at) < 60_000 => "Last ran just now".into(),
        Some(at) => format!("Last ran {} ago", span(now_ms - at)),
    }
}

fn runner() -> String {
    format!(
        "The {} cabin runs automations on their schedule while it's open. Here a job runs when you choose Run now.",
        grokhub_core::brand::product_name()
    )
}

pub fn page(list: &[Automation], behavior: &Behavior, now_ms: u64) -> Page {
    let next_due = list
        .iter()
        .filter(|a| a.enabled)
        .filter_map(|a| a.next_run.map(|at| (at, a)))
        .min_by_key(|(at, _)| *at)
        .map(|(at, a)| format!("{} · {}", a.name, next_text(at, now_ms)));
    let quiet = if behavior.quiet_start.is_empty() {
        "No quiet hours".to_string()
    } else {
        format!(
            "Quiet hours {}–{}",
            behavior.quiet_start, behavior.quiet_end
        )
    };
    Page {
        runner: runner(),
        next_due,
        quiet,
        budget: format!(
            "Daily token budget: {}",
            grokhub_core::token_budget_label(behavior.daily_token_budget)
        ),
        rows: list
            .iter()
            .map(|a| Row {
                id: a.id.clone(),
                name: a.name.clone(),
                next: match (a.enabled, a.next_run) {
                    (false, _) => "Paused".into(),
                    (true, Some(at)) => next_text(at, now_ms),
                    (true, None) => "Not scheduled".into(),
                },
                last: last_text(a.last_run, now_ms),
                health: grokhub_core::automation_health_line(a),
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: u64 = 1_800_000_000_000;
    const MIN: u64 = 60_000;

    fn job(
        id: &str,
        name: &str,
        enabled: bool,
        next: Option<u64>,
        last: Option<u64>,
    ) -> Automation {
        serde_json::from_value(serde_json::json!({
            "id": id,
            "name": name,
            "schedule": "daily",
            "time": "09:00",
            "instructions": "summarize my inbox",
            "enabled": enabled,
            "nextRun": next,
            "lastRun": last,
        }))
        .expect("an automation")
    }

    #[test]
    fn times_read_like_the_cabin() {
        assert_eq!(next_text(NOW, NOW), "Due now");
        assert_eq!(next_text(NOW - MIN, NOW), "Due now");
        assert_eq!(next_text(NOW + 12_000, NOW), "Next check in 12s");
        assert_eq!(next_text(NOW + 12 * MIN, NOW), "Next check in 12m");
        assert_eq!(next_text(NOW + 5 * 60 * MIN, NOW), "Next check in 5h");
        assert_eq!(next_text(NOW + 3 * 24 * 60 * MIN, NOW), "Next check in 3d");
        assert_eq!(last_text(None, NOW), "Hasn't run yet");
        assert_eq!(last_text(Some(NOW - 20_000), NOW), "Last ran just now");
        assert_eq!(last_text(Some(NOW - 5 * MIN), NOW), "Last ran 5m ago");
    }

    #[test]
    fn the_page_names_the_next_job_and_each_last_run() {
        let list = [
            job(
                "a",
                "Inbox",
                true,
                Some(NOW + 45 * MIN),
                Some(NOW - 2 * 60 * MIN),
            ),
            job("b", "Build check", true, Some(NOW + 12 * MIN), None),
            job(
                "c",
                "Old report",
                false,
                Some(NOW + MIN),
                Some(NOW - 30 * MIN),
            ),
        ];
        let behavior = Behavior {
            quiet_start: "22:00".into(),
            quiet_end: "07:00".into(),
            daily_token_budget: 1_000_000,
            ..Behavior::default()
        };
        let got = page(&list, &behavior, NOW);
        assert_eq!(
            got.next_due.as_deref(),
            Some("Build check · Next check in 12m"),
            "a paused job is never next"
        );
        assert_eq!(got.quiet, "Quiet hours 22:00–07:00");
        assert_eq!(got.budget, "Daily token budget: 1.0M tokens a day");
        assert_eq!(
            got.rows
                .iter()
                .map(|r| (r.name.as_str(), r.next.as_str(), r.last.as_str()))
                .collect::<Vec<_>>(),
            vec![
                ("Inbox", "Next check in 45m", "Last ran 2h ago"),
                ("Build check", "Next check in 12m", "Hasn't run yet"),
                ("Old report", "Paused", "Last ran 30m ago"),
            ]
        );
        assert!(got
            .runner
            .ends_with("Here a job runs when you choose Run now."));
    }

    #[test]
    fn no_jobs_and_no_limits() {
        let got = page(&[], &Behavior::default(), NOW);
        assert_eq!(
            (
                got.next_due,
                got.quiet.as_str(),
                got.budget.as_str(),
                got.rows
            ),
            (None, "No quiet hours", "Daily token budget: Off", vec![])
        );
    }
}
