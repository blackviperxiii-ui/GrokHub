//! Task recap: the short "Finished" block an agent reply ends with after
//! real work. One line per task (done, partly done or not done), what still
//! needs the person, and where to undo. The cabin paints it as its own card
//! under the reply ([`split_recap`]) instead of more chat text.

/// The line that opens a recap.
pub const RECAP_HEAD: &str = "Finished";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecapStatus {
    Done,
    Partly,
    NotDone,
}

impl RecapStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Done => "Done",
            Self::Partly => "Partly done",
            Self::NotDone => "Not done",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecapLine {
    pub status: RecapStatus,
    /// The task and a few words of result, e.g. "Chips scrubbed, 10 removed".
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Recap {
    pub lines: Vec<RecapLine>,
    /// What is left over or needs the person.
    pub needs_you: Vec<String>,
    /// Where to undo, when backups were made.
    pub undo: Vec<String>,
}

fn is_head(line: &str) -> bool {
    let t = line
        .trim()
        .trim_start_matches('#')
        .trim()
        .trim_matches('*')
        .trim()
        .trim_end_matches(':')
        .trim();
    t.eq_ignore_ascii_case(RECAP_HEAD)
}

/// Splits `label: rest` when the label (case-insensitive) is one of `names`.
fn labeled<'a>(line: &'a str, names: &[&str]) -> Option<&'a str> {
    let (head, rest) = line.split_once(':')?;
    let head = head.trim().trim_matches('*').trim();
    names
        .iter()
        .any(|n| head.eq_ignore_ascii_case(n))
        .then(|| rest.trim())
        .filter(|r| !r.is_empty())
}

enum Row {
    Line(RecapLine),
    NeedsYou(String),
    Undo(String),
}

fn row(line: &str) -> Option<Row> {
    let line = line.trim();
    let bare = line
        .strip_prefix("- ")
        .or_else(|| line.strip_prefix("* "))
        .unwrap_or(line);
    for (status, names) in [
        (RecapStatus::Done, &["done"][..]),
        (
            RecapStatus::Partly,
            &["partly", "partly done", "partial"][..],
        ),
        (RecapStatus::NotDone, &["not done", "failed", "skipped"][..]),
    ] {
        if let Some(text) = labeled(bare, names) {
            return Some(Row::Line(RecapLine {
                status,
                text: text.to_string(),
            }));
        }
    }
    if let Some(text) = labeled(bare, &["needs you", "left over", "leftover"]) {
        return Some(Row::NeedsYou(text.to_string()));
    }
    labeled(bare, &["undo"]).map(|t| Row::Undo(t.to_string()))
}

/// The reply without its recap, and the recap, when the reply ends with a
/// `Finished` block whose every line reads as a recap line and at least one
/// names a task. Anything else stays plain chat.
pub fn split_recap(reply: &str) -> Option<(String, Recap)> {
    let lines: Vec<&str> = reply.lines().collect();
    let at = lines.iter().rposition(|l| is_head(l))?;
    let mut recap = Recap::default();
    for line in &lines[at + 1..] {
        if line.trim().is_empty() {
            continue;
        }
        match row(line)? {
            Row::Line(l) => recap.lines.push(l),
            Row::NeedsYou(t) => recap.needs_you.push(t),
            Row::Undo(t) => recap.undo.push(t),
        }
    }
    if recap.lines.is_empty() {
        return None;
    }
    let body = lines[..at].join("\n").trim_end().to_string();
    Some((body, recap))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(status: RecapStatus, text: &str) -> RecapLine {
        RecapLine {
            status,
            text: text.into(),
        }
    }

    #[test]
    fn a_finished_block_splits_off_the_reply() {
        let reply = "All three fixes ran.\n\n**Finished**\n\
            - Done: Chips scrubbed, 10 removed\n\
            - Partly done: Pulse noise tamed, i2c lines still logged\n\
            - Not done: Soul file, no write access\n\
            Needs you: Local AI still shows loaded, restart Loadout\n\
            Undo: Settings, Labs, Backups (chips.json.bak)\n";
        let (body, recap) = split_recap(reply).expect("a recap");
        assert_eq!(body, "All three fixes ran.");
        assert_eq!(
            recap,
            Recap {
                lines: vec![
                    line(RecapStatus::Done, "Chips scrubbed, 10 removed"),
                    line(
                        RecapStatus::Partly,
                        "Pulse noise tamed, i2c lines still logged"
                    ),
                    line(RecapStatus::NotDone, "Soul file, no write access"),
                ],
                needs_you: vec!["Local AI still shows loaded, restart Loadout".into()],
                undo: vec!["Settings, Labs, Backups (chips.json.bak)".into()],
            }
        );
        // A recap can be the whole reply.
        let (body, recap) = split_recap("## Finished:\n* failed: Reinstall bind").unwrap();
        assert_eq!(body, "");
        assert_eq!(recap.lines, [line(RecapStatus::NotDone, "Reinstall bind")]);
    }

    #[test]
    fn plain_replies_and_loose_blocks_stay_chat() {
        for reply in [
            "",
            "All good, nothing to change.",
            // The word alone, mid-reply, with prose after it.
            "Finished\nI looked at the logs and they are fine.",
            // A head with no task lines.
            "Here you go.\nFinished\nNeeds you: reboot",
            // A task line with nothing after the colon.
            "Finished\n- Done:",
            // "Finished" inside a sentence is not a head.
            "Finished the scan.\n- Done: x",
        ] {
            assert_eq!(split_recap(reply), None, "{reply:?}");
        }
        assert_eq!(RecapStatus::Partly.label(), "Partly done");
    }
}
