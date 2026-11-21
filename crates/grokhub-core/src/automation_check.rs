//! Script-first automations: the check command collects the data, and the
//! model only sees what it printed, or nothing at all.
//!
//! A job's check command runs first (see [`crate::skip_night_check_receipt`]
//! for the receipt shape). Its [`CheckGate`] then picks the next step:
//!
//! - the check failed or printed nothing: wait for the next slot;
//! - `on_change` and the output hashes the same as last time: wait, with
//!   no model call (0 tokens);
//! - `no_agent`: the output is the run's result and no model is woken;
//! - otherwise the model gets the job's ask plus what the check printed
//!   (with `on_change`, only the lines that are new since last time).
//!
//! The gate is set from the person's own words ([`script_words`]), never
//! from a reply.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Most check output kept on the row to diff against next time.
pub const KEEP_CHARS: usize = 16 * 1024;
/// Most check output put in front of the model.
pub const ASK_CHARS: usize = 8 * 1024;

/// How a job's check command output is used.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CheckGate {
    /// Wake the model only when the output changed since the last run.
    #[serde(default, skip_serializing_if = "is_false")]
    pub on_change: bool,
    /// Never wake a model: the output is the run's result.
    #[serde(default, skip_serializing_if = "is_false")]
    pub no_agent: bool,
    /// SHA-256 of the last output that ran, hex.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub last_hash: String,
    /// The last output that ran, clipped to [`KEEP_CHARS`].
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub last_output: String,
}

fn is_false(b: &bool) -> bool {
    !*b
}

impl CheckGate {
    /// Nothing set: the row saves as it did before gates existed.
    pub fn is_plain(&self) -> bool {
        *self == Self::default()
    }

    /// The gate after `output` ran: its hash and text, for the next diff.
    pub fn remember(&self, output: &str) -> Self {
        let output = normalize(output);
        Self {
            last_hash: output_hash(&output),
            last_output: clip(&output, KEEP_CHARS),
            ..self.clone()
        }
    }
}

/// What a due job with a check command does next.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckStep {
    /// The check failed or printed nothing.
    Skip,
    /// `on_change`, and the output is the same as last time.
    Unchanged,
    /// `no_agent`: record this output as the run, wake no model.
    ScriptOnly { output: String },
    /// Wake the model with this text from the check: the whole output, or
    /// with `on_change` only the new lines.
    Ask { output: String, shown: String },
}

impl CheckStep {
    /// The job waits for its next slot without running.
    pub fn waits(&self) -> bool {
        matches!(self, Self::Skip | Self::Unchanged)
    }
}

/// The next step for a check `receipt` under `gate`.
pub fn check_step(gate: &CheckGate, receipt: &str) -> CheckStep {
    if crate::skip_night_check_receipt(receipt) {
        return CheckStep::Skip;
    }
    let output = normalize(crate::night_check_stdout(receipt));
    if gate.on_change && output_hash(&output) == gate.last_hash {
        return CheckStep::Unchanged;
    }
    if gate.no_agent {
        return CheckStep::ScriptOnly { output };
    }
    let shown = if gate.on_change && !gate.last_hash.is_empty() {
        new_lines(&gate.last_output, &output)
    } else {
        output.clone()
    };
    CheckStep::Ask { output, shown }
}

/// The ask the model gets for [`CheckStep::Ask`]: the job's own words, then
/// what the check printed.
pub fn check_prompt(instructions: &str, cmd: &str, shown: &str, on_change: bool) -> String {
    let what = if on_change {
        "printed these new lines since the last run"
    } else {
        "printed this"
    };
    format!(
        "{}\n\nThe check command `{}` {what}:\n```\n{}\n```",
        instructions.trim(),
        cmd.trim(),
        clip(shown, ASK_CHARS)
    )
}

/// SHA-256 of `output`, hex.
pub fn output_hash(output: &str) -> String {
    Sha256::digest(normalize(output).as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Lines of `after` that `before` didn't have, each counted once per time it
/// was there before, in `after`'s order.
pub fn new_lines(before: &str, after: &str) -> String {
    let mut seen: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    for line in before.lines() {
        *seen.entry(line).or_default() += 1;
    }
    let mut out = Vec::new();
    for line in after.lines() {
        match seen.get_mut(line) {
            Some(n) if *n > 0 => *n -= 1,
            _ => out.push(line),
        }
    }
    out.join("\n")
}

/// A check command and gate read from the person's words: a command in
/// backticks after "check" or "run", "only when it changes", and "without
/// AI" (or "no AI", "script only").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptWords {
    pub command: String,
    pub gate: CheckGate,
}

pub fn script_words(text: &str) -> Option<ScriptWords> {
    let lower = text.to_ascii_lowercase();
    let open = ["check `", "check with `", "run `", "runs `"]
        .iter()
        .filter_map(|cue| lower.find(cue).map(|at| at + cue.len()))
        .min()?;
    let len = text[open..].find('`')?;
    let command = text[open..open + len].trim();
    if command.is_empty() {
        return None;
    }
    // Whole words only: "no ai" is not in "casino aisle".
    let words: String = lower
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '\'' {
                c
            } else {
                ' '
            }
        })
        .collect();
    let words = format!(
        " {} ",
        words.split_whitespace().collect::<Vec<_>>().join(" ")
    );
    let has = |cues: &[&str]| cues.iter().any(|c| words.contains(&format!(" {c} ")));
    Some(ScriptWords {
        command: command.to_string(),
        gate: CheckGate {
            on_change: has(&[
                "only when it changes",
                "only when something changes",
                "only if it changes",
                "only if something changed",
                "when it changes",
                "only on change",
            ]),
            no_agent: has(&["without ai", "no ai", "script only", "don't wake the ai"]),
            ..CheckGate::default()
        },
    })
}

fn normalize(text: &str) -> String {
    text.replace("\r\n", "\n").trim().to_string()
}

fn clip(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_string();
    }
    let mut end = max;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}\n…", &text[..end])
}

#[cfg(test)]
mod tests {
    use super::*;

    const PASS: &str = "$ df -h /\nexit 0 · 4ms\n/dev/sda1 80%\n/dev/sdb1 12%\n";

    #[test]
    fn a_failed_or_empty_check_waits_whatever_the_gate() {
        let gate = CheckGate {
            no_agent: true,
            ..CheckGate::default()
        };
        assert_eq!(
            check_step(&gate, "$ false\nexit 1 · 2ms\n"),
            CheckStep::Skip
        );
        assert_eq!(check_step(&gate, "$ true\nexit 0 · 2ms\n"), CheckStep::Skip);
    }

    #[test]
    fn on_change_waits_with_no_model_call_when_the_output_is_the_same() {
        let gate = CheckGate {
            on_change: true,
            ..CheckGate::default()
        };
        let first = check_step(&gate, PASS);
        assert_eq!(
            first,
            CheckStep::Ask {
                output: "/dev/sda1 80%\n/dev/sdb1 12%".into(),
                shown: "/dev/sda1 80%\n/dev/sdb1 12%".into(),
            }
        );
        let gate = gate.remember("/dev/sda1 80%\r\n/dev/sdb1 12%\r\n");
        assert_eq!(check_step(&gate, PASS), CheckStep::Unchanged);
        let moved = "$ df -h /\nexit 0 · 4ms\n/dev/sda1 91%\n/dev/sdb1 12%\n";
        assert_eq!(
            check_step(&gate, moved),
            CheckStep::Ask {
                output: "/dev/sda1 91%\n/dev/sdb1 12%".into(),
                shown: "/dev/sda1 91%".into(),
            }
        );
    }

    #[test]
    fn no_agent_records_the_output_and_never_asks() {
        let gate = CheckGate {
            no_agent: true,
            ..CheckGate::default()
        };
        assert_eq!(
            check_step(&gate, PASS),
            CheckStep::ScriptOnly {
                output: "/dev/sda1 80%\n/dev/sdb1 12%".into()
            }
        );
    }

    #[test]
    fn the_hash_is_sha256_of_the_trimmed_output() {
        assert_eq!(
            output_hash("abc\n"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        let gate = CheckGate::default().remember("abc\r\n");
        assert_eq!(gate.last_output, "abc");
        assert_eq!(gate.last_hash, output_hash("abc"));
        assert!(!gate.is_plain());
        assert!(CheckGate::default().is_plain());
    }

    #[test]
    fn new_lines_counts_repeats_and_keeps_order() {
        assert_eq!(new_lines("a\nb\nb", "b\nc\nb\nb\na"), "c\nb");
        assert_eq!(new_lines("", "x\ny"), "x\ny");
        assert_eq!(new_lines("x\ny", "y\nx"), "");
    }

    #[test]
    fn the_prompt_puts_the_ask_first_then_the_output() {
        assert_eq!(
            check_prompt("  Tell me if a disk is nearly full. ", "df -h", "/dev/sda1 91%", true),
            "Tell me if a disk is nearly full.\n\nThe check command `df -h` printed these new lines since the last run:\n```\n/dev/sda1 91%\n```"
        );
        let long = "x".repeat(ASK_CHARS + 10);
        let prompt = check_prompt("Look.", "cat log", &long, false);
        assert!(prompt.starts_with("Look.\n\nThe check command `cat log` printed this:\n```\n"));
        assert!(prompt.ends_with(&format!("{}\n…\n```", "x".repeat(ASK_CHARS))));
    }

    #[test]
    fn script_words_read_the_command_and_the_gate_from_the_ask() {
        assert_eq!(
            script_words("every day at 9, check `df -h /` and tell me only when it changes"),
            Some(ScriptWords {
                command: "df -h /".into(),
                gate: CheckGate {
                    on_change: true,
                    ..CheckGate::default()
                },
            })
        );
        assert_eq!(
            script_words("every weekday at 7 run `backup.sh` without AI"),
            Some(ScriptWords {
                command: "backup.sh".into(),
                gate: CheckGate {
                    no_agent: true,
                    ..CheckGate::default()
                },
            })
        );
        assert_eq!(
            script_words("every day at 9, run `ls` at the casino aisle").map(|w| w.gate),
            Some(CheckGate::default())
        );
        assert_eq!(script_words("every day at 9, summarize the board"), None);
        assert_eq!(script_words("every day at 9, run `` please"), None);
        assert_eq!(script_words("every day at 9, run `unclosed"), None);
    }
}
