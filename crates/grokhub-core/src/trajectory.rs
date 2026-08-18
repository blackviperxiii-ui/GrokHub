//! Append-only host/hands receipts for the nightly skill patch.

use crate::redact::redact_secrets;
use serde::{Deserialize, Serialize};

pub const TRAJECTORY_MAX_BYTES: usize = 2 * 1024 * 1024;
pub const TRAJECTORY_EXCERPT_CHARS: usize = 200;
const DAY_MS: u64 = 36 * 60 * 60 * 1000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrajectoryEvent {
    pub ts: u64,
    pub cmds: Vec<String>,
    pub ok: bool,
    pub excerpt: String,
}

pub fn clip_excerpt(raw: &str) -> String {
    redact_secrets(raw).chars().take(TRAJECTORY_EXCERPT_CHARS).collect()
}

pub fn trajectory_jsonl_line(ts: u64, cmds: &[String], ok: bool, excerpt: &str) -> String {
    let ev = TrajectoryEvent {
        ts,
        cmds: cmds
            .iter()
            .map(|c| redact_secrets(c).chars().take(120).collect())
            .collect(),
        ok,
        excerpt: clip_excerpt(excerpt),
    };
    serde_json::to_string(&ev).unwrap_or_default()
}

pub fn parse_trajectory_jsonl(raw: &str) -> Vec<TrajectoryEvent> {
    raw.lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() {
                return None;
            }
            serde_json::from_str(line).ok()
        })
        .collect()
}

/// Drop the oldest half when the file grows past `max_bytes`.
pub fn rotate_trajectory(raw: &str, max_bytes: usize) -> String {
    if raw.len() <= max_bytes {
        return raw.to_string();
    }
    let keep_from = raw.len().saturating_sub(max_bytes / 2);
    if let Some(i) = raw[keep_from..].find('\n') {
        raw[keep_from + i + 1..].to_string()
    } else if let Some(i) = raw[..keep_from].rfind('\n') {
        raw[i + 1..].to_string()
    } else {
        raw[keep_from..].to_string()
    }
}

pub fn yesterday_ms(now_ms: u64) -> u64 {
    now_ms.saturating_sub(DAY_MS)
}

/// Compact cabin-real lines for the nightly digest.
pub fn summarize_trajectory(events: &[TrajectoryEvent], since_ms: u64, cap: usize) -> String {
    let mut out = String::new();
    let mut n = 0usize;
    for ev in events.iter().rev() {
        if ev.ts < since_ms {
            continue;
        }
        if n >= cap {
            break;
        }
        let cmd = ev
            .cmds
            .first()
            .map(|s| s.as_str())
            .unwrap_or("")
            .trim();
        if cmd.is_empty() && ev.excerpt.trim().is_empty() {
            continue;
        }
        if !out.is_empty() {
            out.push('\n');
        }
        let mark = if ev.ok { "ok" } else { "fail" };
        out.push_str(&format!("{mark}: {cmd} — {}", ev.excerpt.trim()));
        n += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jsonl_roundtrip_redacts() {
        let line = trajectory_jsonl_line(
            10,
            &["HOST_CMD: echo ghp_abcdefghijklmnopqrstuvwx".into()],
            true,
            "token ghp_abcdefghijklmnopqrstuvwx landed",
        );
        assert!(!line.contains("ghp_abcdefghijklmnopqrstuvwx"), "{line}");
        let evs = parse_trajectory_jsonl(&format!("{line}\n"));
        assert_eq!(evs.len(), 1);
        assert!(evs[0].ok);
        assert!(evs[0].excerpt.contains("[redacted]"));
    }

    #[test]
    fn rotate_keeps_tail() {
        let raw: String = (0..20).map(|i| format!("line-{i:02}\n")).collect();
        let kept = rotate_trajectory(&raw, 80);
        assert!(kept.contains("line-19"), "{kept}");
        assert!(!kept.contains("line-00"), "{kept}");
        assert!(kept.len() < raw.len(), "{} vs {}", kept.len(), raw.len());
    }

    #[test]
    fn summarize_skips_old_and_banned() {
        let evs = vec![
            TrajectoryEvent {
                ts: 1,
                cmds: vec!["HOST_CMD: old".into()],
                ok: true,
                excerpt: "ancient".into(),
            },
            TrajectoryEvent {
                ts: 50,
                cmds: vec!["COMPUTER_CMD: tab close github".into()],
                ok: true,
                excerpt: "closed GitHub".into(),
            },
        ];
        let s = summarize_trajectory(&evs, 10, 8);
        assert!(s.contains("tab close github"), "{s}");
        assert!(!s.contains("ancient"), "{s}");
    }
}
