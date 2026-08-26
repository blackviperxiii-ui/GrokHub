//! Grok Build `/loop` scheduler — interval prompts, not cabin night cron.

use serde::{Deserialize, Serialize};

pub const LOOP_MIN_MS: u64 = 60_000;
pub const LOOP_MAX: usize = 50;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GrokLoop {
    #[serde(default)]
    pub id: String,
    pub interval: String,
    pub prompt: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub created_ms: u64,
    #[serde(default)]
    pub last_run: Option<u64>,
    #[serde(default)]
    pub next_run: Option<u64>,
    #[serde(default)]
    pub run_count: u32,
    #[serde(default)]
    pub session_id: Option<String>,
}

fn default_true() -> bool {
    true
}

pub fn new_loop(interval: String, prompt: String, now_ms: u64) -> GrokLoop {
    GrokLoop {
        id: String::new(),
        interval,
        prompt,
        enabled: true,
        created_ms: now_ms,
        last_run: None,
        next_run: Some(now_ms),
        run_count: 0,
        session_id: None,
    }
}

pub fn mark_loop_ran(mut row: GrokLoop, now_ms: u64) -> GrokLoop {
    let ms = loop_interval_ms(&row.interval).unwrap_or(LOOP_MIN_MS);
    row.last_run = Some(now_ms);
    row.next_run = Some(now_ms.saturating_add(ms));
    row.run_count = row.run_count.saturating_add(1);
    row
}

/// Parse `/loop 30m check deploy`, `every 2 hours …`, or `check deploy every hour`.
pub fn parse_loop_line(text: &str) -> Option<(String, String)> {
    let mut t = text.trim();
    if let Some(rest) = t.strip_prefix("/loop") {
        t = rest.trim();
    }
    if t.is_empty() {
        return None;
    }
    if let Some(pair) = parse_every_prefix(t) {
        return Some(pair);
    }
    if let Some(pair) = parse_leading_interval(t) {
        return Some(pair);
    }
    parse_trailing_every(t)
}

fn parse_every_prefix(t: &str) -> Option<(String, String)> {
    let lower = t.to_ascii_lowercase();
    if !lower.starts_with("every ") {
        return None;
    }
    let orig = t["every ".len()..].trim();
    let mut parts = orig.splitn(2, char::is_whitespace);
    let tok = parts.next()?.trim();
    let after = parts.next()?.trim();
    if after.is_empty() {
        return None;
    }
    if let Some(iv) = normalize_interval(tok) {
        return Some((iv, after.to_string()));
    }
    let (unit, prompt) = after.split_once(char::is_whitespace)?;
    if prompt.trim().is_empty() {
        return None;
    }
    let packed = format!("{tok}{unit}");
    let iv = normalize_interval(&packed)?;
    Some((iv, prompt.trim().to_string()))
}

fn parse_leading_interval(t: &str) -> Option<(String, String)> {
    let mut parts = t.splitn(2, char::is_whitespace);
    let first = parts.next()?.trim();
    let rest = parts.next()?.trim();
    if rest.is_empty() {
        return None;
    }
    if let Some(iv) = normalize_interval(first) {
        return Some((iv, rest.to_string()));
    }
    if !first.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let (unit, prompt) = rest.split_once(char::is_whitespace)?;
    if prompt.trim().is_empty() {
        return None;
    }
    let iv = normalize_interval(&format!("{first}{unit}"))?;
    Some((iv, prompt.trim().to_string()))
}

fn parse_trailing_every(t: &str) -> Option<(String, String)> {
    let lower = t.to_ascii_lowercase();
    let i = lower.rfind(" every ")?;
    let prompt = t[..i].trim();
    if prompt.is_empty() {
        return None;
    }
    let after = t[i + " every ".len()..].trim();
    if after.is_empty() {
        return None;
    }
    if let Some(iv) = normalize_interval(after) {
        return Some((iv, prompt.to_string()));
    }
    let mut parts = after.splitn(2, char::is_whitespace);
    let n = parts.next()?.trim();
    let unit = parts.next()?.trim();
    if unit.is_empty() || unit.contains(char::is_whitespace) {
        return None;
    }
    let iv = normalize_interval(&format!("{n}{unit}"))?;
    Some((iv, prompt.to_string()))
}

pub fn normalize_interval(s: &str) -> Option<String> {
    let mut s = s.trim().to_ascii_lowercase();
    s.retain(|c| !c.is_whitespace());
    if s.chars().next().is_some_and(|c| !c.is_ascii_digit()) {
        s = format!("1{s}");
    }
    let (n, unit) = split_interval(&s)?;
    if n == 0 {
        return None;
    }
    let short = match unit {
        "s" | "sec" | "secs" | "second" | "seconds" => "s",
        "m" | "min" | "mins" | "minute" | "minutes" => "m",
        "h" | "hr" | "hrs" | "hour" | "hours" => "h",
        "d" | "day" | "days" => "d",
        _ => return None,
    };
    Some(format!("{n}{short}"))
}

fn split_interval(s: &str) -> Option<(u64, &str)> {
    let i = s.find(|c: char| !c.is_ascii_digit())?;
    if i == 0 {
        return None;
    }
    let n: u64 = s[..i].parse().ok()?;
    Some((n, s[i..].trim_start_matches('-')))
}

pub fn loop_interval_ms(interval: &str) -> Option<u64> {
    let iv = normalize_interval(interval)?;
    let (n, unit) = split_interval(&iv)?;
    let ms = match unit {
        "s" => n.saturating_mul(1_000),
        "m" => n.saturating_mul(60_000),
        "h" => n.saturating_mul(3_600_000),
        "d" => n.saturating_mul(86_400_000),
        _ => return None,
    };
    Some(ms.max(LOOP_MIN_MS))
}

pub fn loop_next_run(interval_ms: u64, last_run: Option<u64>, now_ms: u64) -> u64 {
    match last_run {
        Some(last) => last.saturating_add(interval_ms.max(LOOP_MIN_MS)),
        None => now_ms,
    }
}

pub fn due_loops(list: &[GrokLoop], now_ms: u64) -> Vec<GrokLoop> {
    list.iter()
        .filter(|l| l.enabled)
        .filter(|l| l.next_run.unwrap_or(0) <= now_ms)
        .cloned()
        .collect()
}

pub fn loop_slash(interval: &str, prompt: &str) -> String {
    format!("/loop {interval} {prompt}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_loop_line_reads_grok_slash() {
        let (iv, p) = parse_loop_line("/loop 30m check deploy status").unwrap();
        assert_eq!(iv, "30m");
        assert_eq!(p, "check deploy status");
        let (iv, p) = parse_loop_line("5m run tests").unwrap();
        assert_eq!(iv, "5m");
        assert_eq!(p, "run tests");
        let (iv, p) = parse_loop_line("every 2 hours summarize commits").unwrap();
        assert_eq!(iv, "2h");
        assert_eq!(p, "summarize commits");
        let (iv, p) = parse_loop_line("/loop 1 hour check deploy").unwrap();
        assert_eq!(iv, "1h");
        assert_eq!(p, "check deploy");
        let (iv, p) = parse_loop_line("/loop check deploy status every hour").unwrap();
        assert_eq!(iv, "1h");
        assert_eq!(p, "check deploy status");
        assert!(parse_loop_line("/loop").is_none());
        assert!(parse_loop_line("hello world").is_none());
    }

    #[test]
    fn new_loop_is_due_immediately_and_advances_after_run() {
        let row = new_loop("30m".into(), "check deploy".into(), 100);
        assert_eq!(row.next_run, Some(100));
        assert!(row.enabled);
        let ran = mark_loop_ran(row, 100);
        assert_eq!(ran.run_count, 1);
        assert_eq!(ran.last_run, Some(100));
        assert_eq!(ran.next_run, Some(100 + 30 * 60_000));
        assert_eq!(LOOP_MAX, 50);
    }

    #[test]
    fn loop_interval_has_a_one_minute_floor() {
        assert_eq!(loop_interval_ms("30s"), Some(60_000));
        assert_eq!(loop_interval_ms("5m"), Some(5 * 60_000));
        assert_eq!(loop_interval_ms("2h"), Some(2 * 3_600_000));
        assert_eq!(loop_interval_ms("1d"), Some(86_400_000));
        assert!(loop_interval_ms("nope").is_none());
    }

    #[test]
    fn due_loops_respect_enabled_and_next_run() {
        let a = GrokLoop {
            id: "a".into(),
            interval: "5m".into(),
            prompt: "ping".into(),
            enabled: true,
            created_ms: 0,
            last_run: None,
            next_run: Some(10),
            run_count: 0,
            session_id: None,
        };
        let mut b = a.clone();
        b.id = "b".into();
        b.enabled = false;
        let mut c = a.clone();
        c.id = "c".into();
        c.next_run = Some(50);
        let due = due_loops(&[a, b, c], 20);
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].id, "a");
        assert_eq!(loop_next_run(5_000, Some(10), 20), 10 + LOOP_MIN_MS);
        assert_eq!(loop_slash("30m", "check deploy"), "/loop 30m check deploy");
    }
}
