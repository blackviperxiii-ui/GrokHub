//! `/health`: pending updates, failed services and the last dream in one
//! block. Plain text that also reads as a Markdown list in chat.

use crate::what_changed;

/// What `/health` reports. Built by the app from state it already holds, so
/// making the block does no network or disk work.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HealthInput {
    /// Newer GrokHub release, when one is waiting.
    pub cabin_update: Option<String>,
    /// Newer Grok Build CLI alpha, when one is waiting.
    pub cli_update: Option<String>,
    /// Each failed check or service, named.
    pub failed: Vec<String>,
    /// How many checks and services were looked at.
    pub checked: usize,
    /// The newest dream as `(date, report)`.
    pub last_dream: Option<(String, String)>,
}

pub const NO_DREAM_LINE: &str =
    "Last dream: none yet. GrokHub dreams once a night after the review, in memory repo mode.";

fn updates_line(h: &HealthInput) -> String {
    let named: Vec<String> = [
        h.cabin_update.as_deref().map(|v| format!("GrokHub {}", v.trim_start_matches('v'))),
        h.cli_update.as_deref().map(|v| format!("Grok Build CLI {v}")),
    ]
    .into_iter()
    .flatten()
    .collect();
    match named.len() {
        0 => "Updates: none pending.".into(),
        1 => format!("Updates: {} is ready. Run /update.", named[0]),
        _ => format!("Updates: {} are ready. Run /update.", named.join(" and ")),
    }
}

fn services_lines(h: &HealthInput) -> String {
    if h.failed.is_empty() {
        return format!("Services: all {} ok.", h.checked);
    }
    let mut out = format!("Services: {} of {} failed", h.failed.len(), h.checked.max(h.failed.len()));
    for name in &h.failed {
        out.push_str(&format!("\n- {name}"));
    }
    out
}

fn dream_line(h: &HealthInput) -> String {
    let Some((date, report)) = &h.last_dream else {
        return NO_DREAM_LINE.into();
    };
    let c = what_changed::from_dream(report);
    if c.is_empty() {
        format!("Last dream: {date}, nothing changed.")
    } else {
        format!(
            "Last dream: {date}, {} merged, {} retired.",
            c.merged.len(),
            c.removed.len()
        )
    }
}

/// The `/health` block: updates, services, last dream, in that order.
pub fn health_block(h: &HealthInput) -> String {
    format!(
        "Health\n\n{}\n{}\n{}",
        updates_line(h),
        services_lines(h),
        dream_line(h)
    )
}

/// One status-line summary of the same block.
pub fn health_status(h: &HealthInput) -> String {
    let updates = match (h.cabin_update.is_some(), h.cli_update.is_some()) {
        (false, false) => "no updates",
        (true, true) => "2 updates",
        _ => "1 update",
    };
    let failed = match h.failed.len() {
        0 => "services ok".to_string(),
        n => format!("{n} failed"),
    };
    format!("Health: {updates} · {failed}")
}

#[cfg(test)]
mod tests {
    use super::*;

    const DREAM: &str = "# Memory dream 2026-10-08\n\n## Merged\n\n- Kept `n1` \"Uses pnpm\", merged `n2` \"uses pnpm\" because 80% shared words.\n\n## Retired\n\n- Retired `n9` \"old token path\" because confidence 0.2.\n";

    #[test]
    fn health_names_updates_failed_services_and_the_last_dream() {
        let h = HealthInput {
            cabin_update: Some("v2.10.99".into()),
            cli_update: Some("0.1.80".into()),
            failed: vec![
                "xAI auth missing — Connect Grok OAuth in Settings".into(),
                "MCP github: error, timed out".into(),
            ],
            checked: 6,
            last_dream: Some(("2026-10-08".into(), DREAM.into())),
        };
        assert_eq!(
            health_block(&h),
            "Health\n\nUpdates: GrokHub 2.10.99 and Grok Build CLI 0.1.80 are ready. Run /update.\nServices: 2 of 6 failed\n- xAI auth missing — Connect Grok OAuth in Settings\n- MCP github: error, timed out\nLast dream: 2026-10-08, 1 merged, 1 retired."
        );
        assert_eq!(health_status(&h), "Health: 2 updates · 2 failed");
    }

    #[test]
    fn health_has_empty_state_text_for_each_section() {
        let h = HealthInput {
            checked: 4,
            ..Default::default()
        };
        assert_eq!(
            health_block(&h),
            "Health\n\nUpdates: none pending.\nServices: all 4 ok.\nLast dream: none yet. GrokHub dreams once a night after the review, in memory repo mode."
        );
        assert_eq!(health_status(&h), "Health: no updates · services ok");
        let quiet = HealthInput {
            cli_update: Some("0.1.80".into()),
            checked: 4,
            last_dream: Some(("2026-10-07".into(), "# Memory dream 2026-10-07\n\n## Merged\n\nNo duplicates found.\n".into())),
            ..Default::default()
        };
        assert_eq!(
            health_block(&quiet),
            "Health\n\nUpdates: Grok Build CLI 0.1.80 is ready. Run /update.\nServices: all 4 ok.\nLast dream: 2026-10-07, nothing changed."
        );
        assert_eq!(health_status(&quiet), "Health: 1 update · services ok");
    }
}
