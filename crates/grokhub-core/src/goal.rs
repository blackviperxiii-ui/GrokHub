//! Goal pin survives compact. Incomplete turns stay open.

pub fn looks_incomplete(assistant_text: &str) -> bool {
    let t = assistant_text.to_ascii_lowercase();
    if t.trim().is_empty() {
        return true;
    }
    if regexish_open(&t) {
        return true;
    }
    if regexish_done(&t) {
        return false;
    }
    if t.len() < 80 && done_word(&t) {
        return false;
    }
    t.len() < 80
}

fn regexish_open(t: &str) -> bool {
    [
        "next step",
        "still need",
        "todo",
        "to-do",
        "remaining",
        "i'll continue",
        "continue with",
        "partially",
        "in progress",
        "not done yet",
        "blocked on",
        "let me know if",
        "want me to",
    ]
    .iter()
    .any(|p| t.contains(p))
}

fn regexish_done(t: &str) -> bool {
    [
        "all done",
        "completed successfully",
        "nothing else",
        "task is complete",
        "fully done",
        "goal_complete",
        "you're all set",
        "no further action",
        "shipped",
    ]
    .iter()
    .any(|p| t.contains(p))
}

fn done_word(t: &str) -> bool {
    ["done", "fixed", "complete", "shipped", "resolved", "applied"]
        .iter()
        .any(|w| t.split(|c: char| !c.is_ascii_alphabetic()).any(|x| x == *w))
}

pub fn parse_goal_outcome(text: &str) -> &'static str {
    if text.to_ascii_uppercase().contains("GOAL_COMPLETE") {
        return "complete";
    }
    if text.to_ascii_uppercase().contains("GOAL_BLOCKED:") {
        return "blocked";
    }
    if looks_incomplete(text) {
        "continue"
    } else {
        "complete"
    }
}

/// Keep the last `keep` turns. Re-insert the goal pin as a system line so compact cannot drop it.
pub fn compact_keep_pin(
    messages: &[(String, String)],
    keep: usize,
    pin: Option<&str>,
) -> Vec<(String, String)> {
    let keep = keep.max(1);
    let mut out = if messages.len() > keep {
        messages[messages.len() - keep..].to_vec()
    } else {
        messages.to_vec()
    };
    let Some(pin) = pin.map(str::trim).filter(|s| !s.is_empty()) else {
        return out;
    };
    let marked = format!("GOAL PIN: {pin}");
    if !out.iter().any(|(_, c)| c.contains(pin)) {
        out.insert(0, ("system".into(), marked));
    }
    out
}

pub const GOAL_MAX_STEPS: u32 = 6;

pub fn next_goal_prompt(pin: &str, prior: &str, step: u32, max_steps: u32) -> Option<String> {
    if pin.trim().is_empty() {
        return None;
    }
    if step >= max_steps.max(1) {
        return None;
    }
    Some(format!(
        "[Goal step {}/{}]\nTask: {}\nLast progress:\n{}\n\nContinue autonomously. Use HOST_CMD as needed.\nWhen fully finished, say clearly: GOAL_COMPLETE\nIf blocked on the user, say: GOAL_BLOCKED: <reason>",
        step + 1,
        max_steps.max(1),
        pin.trim(),
        prior.chars().take(1500).collect::<String>()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pin_survives_and_outcome() {
        let msgs = (0..12)
            .map(|i| ("user".into(), format!("turn {i}")))
            .collect::<Vec<_>>();
        let out = compact_keep_pin(&msgs, 8, Some("flash the pi"));
        assert_eq!(out.len(), 9);
        assert_eq!(out[0], ("system".into(), "GOAL PIN: flash the pi".into()));
        assert!(out.iter().any(|(_, c)| c == "turn 11"));
        assert!(!out.iter().any(|(_, c)| c == "turn 0"));
        assert_eq!(parse_goal_outcome("GOAL_COMPLETE verify ok"), "complete");
        assert_eq!(parse_goal_outcome("GOAL_BLOCKED: need serial"), "blocked");
        assert_eq!(parse_goal_outcome("next step is flashing"), "continue");
        assert!(!looks_incomplete("All done. GOAL_COMPLETE"));
        let p = next_goal_prompt("flash the pi", "wrote image", 0, 6).unwrap();
        assert!(p.contains("Goal step 1/6"));
        assert!(next_goal_prompt("flash the pi", "x", 6, 6).is_none());
    }
}
