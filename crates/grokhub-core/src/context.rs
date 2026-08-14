//! Context budget. Compact the API window, not the on-screen chat.

pub const CONTEXT_BUDGET_TOKENS: u32 = 96_000;
pub const COMPACT_THRESHOLD: f32 = 0.72;
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
    messages
        .iter()
        .map(|(_, c)| 4 + estimate_tokens(c))
        .sum()
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
    }
}
