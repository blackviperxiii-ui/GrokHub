//! App-side usage buckets. Not xAI billing.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct UsageDay {
    pub day: String,
    pub messages: u32,
    pub imagine: u32,
    pub host: u32,
    pub automation: u32,
    /// Grok Build tokens spent today. Server-reported, rolled with the day.
    #[serde(default)]
    pub tokens_in: u64,
    #[serde(default)]
    pub tokens_out: u64,
    #[serde(default)]
    pub tokens_think: u64,
    /// Highest budget warning already shown today (0 none, 1 near, 2 over).
    #[serde(default)]
    pub budget_noted: u8,
}

/// Settings presets for the daily token budget. 0 is off.
pub const TOKEN_BUDGETS: &[u64] = &[
    0,
    250_000,
    500_000,
    1_000_000,
    2_000_000,
    5_000_000,
    10_000_000,
    25_000_000,
];

/// Warn once the day passes this share of the budget.
pub const BUDGET_NEAR_PCT: u64 = 80;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum BudgetLevel {
    Under = 0,
    Near = 1,
    Over = 2,
}

/// Everything Grok Build reported today: input, output, and reasoning.
pub fn tokens_today(day: &UsageDay) -> u64 {
    day.tokens_in
        .saturating_add(day.tokens_out)
        .saturating_add(day.tokens_think)
}

/// `None` while the budget is off.
pub fn budget_level(used: u64, cap: u64) -> Option<BudgetLevel> {
    if cap == 0 {
        return None;
    }
    Some(if used >= cap {
        BudgetLevel::Over
    } else if used.saturating_mul(100) >= cap.saturating_mul(BUDGET_NEAR_PCT) {
        BudgetLevel::Near
    } else {
        BudgetLevel::Under
    })
}

/// A warning not yet shown today. Marks it shown. Near then Over, each once a day.
pub fn take_budget_note(day: &mut UsageDay, cap: u64) -> Option<BudgetLevel> {
    let level = budget_level(tokens_today(day), cap)?;
    if level == BudgetLevel::Under || (level as u8) <= day.budget_noted {
        return None;
    }
    day.budget_noted = level as u8;
    Some(level)
}

/// Scheduled work waits for tomorrow once today is over budget, when you asked it to.
pub fn budget_holds_scheduled(day: &UsageDay, cap: u64, pause: bool) -> bool {
    pause && budget_level(tokens_today(day), cap) == Some(BudgetLevel::Over)
}

/// "Off" or "1.0M tokens a day".
pub fn token_budget_label(cap: u64) -> String {
    if cap == 0 {
        "Off".into()
    } else {
        format!("{} tokens a day", compact_tokens(cap))
    }
}

/// "812k of 1.0M tokens today (81%)".
pub fn budget_line(day: &UsageDay, cap: u64) -> String {
    let used = tokens_today(day);
    if cap == 0 {
        return format!("{} tokens today", compact_tokens(used));
    }
    let pct = used.saturating_mul(100) / cap.max(1);
    format!(
        "{} of {} tokens today ({pct}%)",
        compact_tokens(used),
        compact_tokens(cap)
    )
}

pub fn bump_usage(day: &mut UsageDay, bucket: &str) {
    match bucket {
        "imagine" => day.imagine = day.imagine.saturating_add(1),
        "host" => day.host = day.host.saturating_add(1),
        "automation" => day.automation = day.automation.saturating_add(1),
        _ => day.messages = day.messages.saturating_add(1),
    }
}

/// Grok reports session totals, not per-turn deltas, and a new session restarts the
/// count. A number smaller than the last one is a fresh session, not a refund.
pub fn token_delta(seen: u64, now: u64) -> u64 {
    if now >= seen {
        now - seen
    } else {
        now
    }
}

pub fn add_tokens(day: &mut UsageDay, input: u64, output: u64, reasoning: u64) {
    day.tokens_in = day.tokens_in.saturating_add(input);
    day.tokens_out = day.tokens_out.saturating_add(output);
    day.tokens_think = day.tokens_think.saturating_add(reasoning);
}

pub fn roll_usage_day(day: &mut UsageDay, today: &str) {
    let today = today.trim();
    if today.is_empty() || day.day == today {
        return;
    }
    if day.day.is_empty() {
        day.day = today.to_string();
        return;
    }
    *day = UsageDay {
        day: today.to_string(),
        ..Default::default()
    };
}

pub fn usage_line(day: &UsageDay) -> String {
    let mut s = format!(
        "today {} · chat {} · imagine {} · host {} · night {}",
        day.day, day.messages, day.imagine, day.host, day.automation
    );
    if day.tokens_in + day.tokens_out + day.tokens_think > 0 {
        s.push_str(&format!(
            " · tokens {} in / {} out",
            compact_tokens(day.tokens_in),
            compact_tokens(day.tokens_out)
        ));
        if day.tokens_think > 0 {
            s.push_str(&format!(" / {} think", compact_tokens(day.tokens_think)));
        }
    }
    s
}

fn compact_tokens(n: u64) -> String {
    match n {
        0..=9_999 => n.to_string(),
        10_000..=999_999 => format!("{}k", n / 1_000),
        _ => format!("{:.1}M", n as f64 / 1_000_000.0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buckets() {
        let mut d = UsageDay {
            day: "2026-08-14".into(),
            ..Default::default()
        };
        bump_usage(&mut d, "message");
        bump_usage(&mut d, "host");
        bump_usage(&mut d, "automation");
        assert_eq!(d.messages, 1);
        assert_eq!(d.host, 1);
        assert_eq!(d.automation, 1);
        assert_eq!(d.imagine, 0);
        assert!(usage_line(&d).contains("chat 1"));
        roll_usage_day(&mut d, "2026-08-15");
        assert_eq!(d.messages, 0);
        assert_eq!(d.automation, 0);
        assert_eq!(d.day, "2026-08-15");
        let mut unbound = UsageDay {
            day: String::new(),
            messages: 3,
            ..Default::default()
        };
        roll_usage_day(&mut unbound, "2026-08-16");
        assert_eq!(unbound.messages, 3, "first bind must not wipe early bumps");
        assert_eq!(unbound.day, "2026-08-16");
    }

    #[test]
    fn a_day_counts_grok_tokens_without_double_counting_a_session() {
        // Grok reports the session running total on every turn.
        assert_eq!(token_delta(0, 1_200), 1_200);
        assert_eq!(token_delta(1_200, 3_000), 1_800);
        assert_eq!(token_delta(1_200, 1_200), 0);
        assert_eq!(
            token_delta(9_000, 400),
            400,
            "a new session restarts the count — that is not a refund"
        );
        let mut d = UsageDay {
            day: "2026-08-30".into(),
            ..Default::default()
        };
        add_tokens(&mut d, 1_200, 300, 0);
        add_tokens(&mut d, 1_800, 900, 450);
        assert_eq!(d.tokens_in, 3_000);
        assert_eq!(d.tokens_out, 1_200);
        assert_eq!(d.tokens_think, 450);
        let line = usage_line(&d);
        assert!(line.contains("tokens 3000 in / 1200 out / 450 think"), "{line}");
        add_tokens(&mut d, 2_000_000, 40_000, 0);
        let big = usage_line(&d);
        assert!(big.contains("2.0M in / 41k out"), "{big}");
        roll_usage_day(&mut d, "2026-08-31");
        assert_eq!(d.tokens_in, 0, "a new day starts a new token count");
        assert!(
            !usage_line(&d).contains("tokens"),
            "a quiet day must not print an empty token line"
        );
    }

    #[test]
    fn budget_warns_near_then_over_once_a_day() {
        let mut d = UsageDay {
            day: "2026-09-29".into(),
            ..Default::default()
        };
        assert_eq!(budget_level(10, 0), None, "off");
        assert_eq!(take_budget_note(&mut d, 0), None);
        add_tokens(&mut d, 700_000, 50_000, 0);
        assert_eq!(budget_level(tokens_today(&d), 1_000_000), Some(BudgetLevel::Under));
        assert_eq!(take_budget_note(&mut d, 1_000_000), None);
        add_tokens(&mut d, 0, 60_000, 0);
        assert_eq!(take_budget_note(&mut d, 1_000_000), Some(BudgetLevel::Near));
        assert_eq!(take_budget_note(&mut d, 1_000_000), None, "near shows once");
        assert!(!budget_holds_scheduled(&d, 1_000_000, true));
        add_tokens(&mut d, 0, 0, 200_000);
        assert_eq!(take_budget_note(&mut d, 1_000_000), Some(BudgetLevel::Over));
        assert_eq!(take_budget_note(&mut d, 1_000_000), None, "over shows once");
        assert!(budget_holds_scheduled(&d, 1_000_000, true));
        assert!(!budget_holds_scheduled(&d, 1_000_000, false), "pause is opt-in");
        assert!(!budget_holds_scheduled(&d, 0, true), "no budget, no hold");
        assert_eq!(budget_line(&d, 1_000_000), "1.0M of 1.0M tokens today (101%)");
        roll_usage_day(&mut d, "2026-09-30");
        assert_eq!(d.budget_noted, 0, "a new day warns again");
        assert!(!budget_holds_scheduled(&d, 1_000_000, true));
    }

    #[test]
    fn budget_jumps_straight_to_over_and_labels_read_plainly() {
        let mut d = UsageDay::default();
        add_tokens(&mut d, 3_000_000, 0, 0);
        assert_eq!(take_budget_note(&mut d, 1_000_000), Some(BudgetLevel::Over));
        assert_eq!(take_budget_note(&mut d, 1_000_000), None, "no late Near after Over");
        assert_eq!(token_budget_label(0), "Off");
        assert_eq!(token_budget_label(500_000), "500k tokens a day");
        assert_eq!(token_budget_label(2_000_000), "2.0M tokens a day");
        assert!(TOKEN_BUDGETS[0] == 0 && TOKEN_BUDGETS.windows(2).all(|w| w[0] < w[1]));
        let old: UsageDay = serde_json::from_str(r#"{"day":"d","messages":1,"imagine":0,"host":0,"automation":0}"#).unwrap();
        assert_eq!(old.budget_noted, 0);
    }
}
