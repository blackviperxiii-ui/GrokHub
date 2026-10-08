//! Difficulty `d` in 0.0..=1.0 from plain rules on the step. The text is read
//! here and never stored: route records keep only `d`.

/// What the rules look at.
#[derive(Debug, Clone, Copy, Default)]
pub struct DifficultyInput<'a> {
    /// The newest user text, if the call has one.
    pub text: &'a str,
    pub planned_tools: u32,
    pub plan_mode: bool,
    pub ctx_tokens: u64,
}

const HARD_WORDS: &[&str] = &["debug", "refactor", "design", "why", "prove", "architecture", "migrate", "race", "deadlock"];
const STEP_WORDS: &[&str] = &["then", "after that", "step", "first", "finally"];

/// Rule-based difficulty, rounded to hundredths so the same input always gives the same `d`.
pub fn difficulty(input: &DifficultyInput<'_>) -> f64 {
    let t = input.text.to_ascii_lowercase();
    let mut d: f64 = 0.0;
    let chars = t.chars().count();
    if chars > 2_000 {
        d += 0.25;
    } else if chars > 400 {
        d += 0.15;
    } else if chars > 120 {
        d += 0.05;
    }
    let paths = t
        .split_whitespace()
        .filter(|w| (w.contains('/') || w.contains('\\')) && w.contains('.') || w.ends_with(".rs") || w.ends_with(".py"))
        .count();
    d += 0.05 * paths.min(4) as f64;
    if t.contains("```") {
        d += 0.1;
    }
    let hard = HARD_WORDS.iter().filter(|w| t.split(|c: char| !c.is_alphanumeric()).any(|x| x == **w)).count();
    d += 0.15 * hard.min(2) as f64;
    if STEP_WORDS.iter().filter(|w| t.contains(**w)).count() >= 2 {
        d += 0.1;
    }
    d += 0.05 * input.planned_tools.min(4) as f64;
    if input.plan_mode {
        d += 0.2;
    }
    if input.ctx_tokens > 100_000 {
        d += 0.1;
    }
    (d.min(1.0) * 100.0).round() / 100.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn routine_is_low_and_hard_work_is_high_and_repeatable() {
        assert_eq!(difficulty(&DifficultyInput { text: "hi there", ..Default::default() }), 0.0);
        let hard = DifficultyInput {
            text: "Debug why crates/a/src/lib.rs deadlocks, then refactor it. First read it, finally prove the fix. ```rust\nfn x(){}\n```",
            planned_tools: 3,
            plan_mode: true,
            ctx_tokens: 200_000,
        };
        let d = difficulty(&hard);
        assert_eq!(d, 1.0);
        assert_eq!(difficulty(&hard), d);
        assert_eq!(difficulty(&DifficultyInput { text: "why is the sky blue", ..Default::default() }), 0.15);
    }
}
