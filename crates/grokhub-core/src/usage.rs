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

pub fn usage_blocked(day: &UsageDay, bucket: &str, cap: u32) -> bool {
    if cap == 0 {
        return false;
    }
    let used = match bucket {
        "imagine" => day.imagine,
        "host" => day.host,
        "automation" => day.automation,
        _ => day.messages,
    };
    used >= cap
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
    format!(
        "today {} · chat {} · imagine {} · host {} · night {}",
        day.day, day.messages, day.imagine, day.host, day.automation
    )
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
        assert!(usage_blocked(&d, "host", 1));
        assert!(usage_blocked(&d, "automation", 1));
        assert!(!usage_blocked(&d, "imagine", 5));
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
}
