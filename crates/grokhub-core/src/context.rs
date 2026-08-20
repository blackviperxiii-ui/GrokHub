//! Context budget. Compact the API window, not the on-screen chat.

pub const CONTEXT_BUDGET_TOKENS: u32 = 96_000;
pub const COMPACT_THRESHOLD: f32 = 0.72;
pub const RESULT_TRIM_THRESHOLD: f32 = 0.50;
pub const RESULT_TRIM_KEEP_HOPS: usize = 4;
const RESULT_TRIM_HEAD: usize = 6;
const RESULT_TRIM_TAIL: usize = 6;
pub const RECENT_MIN_MESSAGES: usize = 8;

pub fn estimate_tokens(text: &str) -> u32 {
    if text.is_empty() {
        return 0;
    }
    let code: usize = text
        .split("```")
        .enumerate()
        .filter(|(i, _)| i % 2 == 1)
        .map(|(_, s)| s.len())
        .sum();
    let rest = text.len().saturating_sub(code);
    ((rest as f32 / 4.0) + (code as f32 / 3.2)).ceil().max(1.0) as u32
}

pub fn estimate_messages(messages: &[(String, String)]) -> u32 {
    estimate_messages_from(messages.iter().map(|(r, c)| (r.as_str(), c.as_str())))
}

/// Same estimate without cloning an 8MB transcript onto the UI thread.
pub fn estimate_messages_from<'a, I>(messages: I) -> u32
where
    I: IntoIterator<Item = (&'a str, &'a str)>,
{
    messages.into_iter().map(|(_, c)| 4 + estimate_tokens(c)).sum()
}

pub fn context_percent(tokens: u32, budget: u32) -> u32 {
    if budget == 0 {
        return 100;
    }
    ((tokens as u64 * 100) / budget as u64).min(100) as u32
}

pub fn should_auto_compact(tokens: u32, budget: u32) -> bool {
    tokens as f32 >= budget as f32 * COMPACT_THRESHOLD
}

/// Goal continuations need the early turns. Compact only after the goal is idle.
pub fn should_auto_compact_now(tokens: u32, budget: u32, goal_step: u32) -> bool {
    goal_step == 0 && should_auto_compact(tokens, budget)
}

pub fn should_trim_result_bodies(tokens: u32, budget: u32) -> bool {
    tokens as f32 >= budget as f32 * RESULT_TRIM_THRESHOLD
}

pub fn is_result_turn(role: &str, content: &str) -> bool {
    if role != "user" {
        return false;
    }
    let t = content.trim_start();
    t.starts_with("HOST_RESULT")
        || t.starts_with("COMPUTER_RESULT")
        || t.starts_with("CONNECTOR_RESULT")
}

fn trim_result_body(content: &str) -> String {
    let lines: Vec<&str> = content.lines().collect();
    if lines.len() <= RESULT_TRIM_HEAD + RESULT_TRIM_TAIL + 2 {
        return content.to_string();
    }
    let head_end = RESULT_TRIM_HEAD.min(lines.len());
    let tail_start = lines.len().saturating_sub(RESULT_TRIM_TAIL);
    if tail_start <= head_end {
        return content.to_string();
    }
    let mut out = lines[..head_end].join("\n");
    out.push_str("\n…\n");
    out.push_str(&lines[tail_start..].join("\n"));
    out
}

/// Shrink older HOST/COMPUTER/CONNECTOR dumps. Keep the last hops and any GOAL PIN.
pub fn trim_result_bodies(
    messages: &[(String, String)],
    keep_recent_hops: usize,
) -> Vec<(String, String)> {
    let mut out = messages.to_vec();
    trim_result_bodies_in_place(out.iter_mut().map(|(r, c)| (r.as_str(), c)), keep_recent_hops);
    out
}

/// Rewrite old dumps in place so HostDone does not clone an 8MB pane.
pub fn trim_result_bodies_in_place<'a, I>(messages: I, keep_recent_hops: usize)
where
    I: IntoIterator<Item = (&'a str, &'a mut String)>,
{
    let mut items: Vec<(&'a str, &'a mut String)> = messages.into_iter().collect();
    let keep = keep_recent_hops.max(1);
    let result_idx: Vec<usize> = items
        .iter()
        .enumerate()
        .filter(|(_, (role, content))| is_result_turn(role, content))
        .map(|(i, _)| i)
        .collect();
    let keep_from = result_idx.len().saturating_sub(keep);
    let keep_set = &result_idx[keep_from..];
    for (i, (role, content)) in items.iter_mut().enumerate() {
        if *role == "system" && content.trim_start().starts_with("GOAL PIN:") {
            continue;
        }
        if is_result_turn(role, content) && !keep_set.contains(&i) {
            let trimmed = trim_result_body(content);
            if trimmed.len() != content.len() || trimmed != **content {
                **content = trimmed;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn budget_math() {
        assert!(estimate_tokens("abcd") >= 1);
        let msgs = vec![("user".into(), "hello world".into())];
        assert!(estimate_messages(&msgs) > 4);
        assert_eq!(context_percent(48_000, CONTEXT_BUDGET_TOKENS), 50);
        assert!(should_auto_compact(70_000, CONTEXT_BUDGET_TOKENS));
        assert!(!should_auto_compact(1_000, CONTEXT_BUDGET_TOKENS));
        assert!(
            !should_auto_compact_now(70_000, CONTEXT_BUDGET_TOKENS, 2),
            "mid-goal compact would drop the early steps"
        );
        assert!(should_auto_compact_now(70_000, CONTEXT_BUDGET_TOKENS, 0));
        assert!(should_trim_result_bodies(50_000, CONTEXT_BUDGET_TOKENS));
        assert!(!should_trim_result_bodies(10_000, CONTEXT_BUDGET_TOKENS));
    }

    #[test]
    fn trim_result_bodies_keeps_pin_and_last_hops() {
        let mut msgs = vec![(
            "system".into(),
            "GOAL PIN: close the other-monitor tab".into(),
        )];
        msgs.push(("user".into(), "close the firefox tab".into()));
        for i in 0..6 {
            let mut body = format!("HOST_RESULT (facts only):\nhead {i}\n");
            for n in 0..20 {
                body.push_str(&format!("dump line {i}-{n}\n"));
            }
            body.push_str("tail fact\n");
            msgs.push(("user".into(), body));
            msgs.push(("assistant".into(), format!("next {i}")));
        }
        let out = trim_result_bodies(&msgs, RESULT_TRIM_KEEP_HOPS);
        assert_eq!(out[0].1, "GOAL PIN: close the other-monitor tab");
        assert_eq!(out[1].1, "close the firefox tab");
        let dumps: Vec<&(String, String)> = out
            .iter()
            .filter(|(r, c)| is_result_turn(r, c))
            .collect();
        assert_eq!(dumps.len(), 6);
        assert!(dumps[0].1.contains("…"), "{}", dumps[0].1);
        assert!(dumps[0].1.contains("head 0"), "{}", dumps[0].1);
        assert!(dumps[0].1.contains("tail fact"), "{}", dumps[0].1);
        assert!(!dumps[5].1.contains("…"), "{}", dumps[5].1);
        assert!(dumps[5].1.contains("dump line 5-10"), "{}", dumps[5].1);

        let mut in_place = msgs.clone();
        trim_result_bodies_in_place(
            in_place.iter_mut().map(|(r, c)| (r.as_str(), c)),
            RESULT_TRIM_KEEP_HOPS,
        );
        assert_eq!(in_place, out);
    }
}
