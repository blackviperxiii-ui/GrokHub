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
}

pub fn usage_day_key(clock_ymd: &str) -> String {
    clock_ymd.trim().to_string()
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
}
