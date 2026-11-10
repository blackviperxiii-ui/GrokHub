//! The shared rule every card must pass: it shows real details, not just
//! counts. A card that says "2 updates" names both; a card never ends on a
//! cut-off step. Both UIs paint the same text, so one check covers them.

use crate::update_feed::UpdateCard;

/// Nouns a count is about. "8 skill runs" is a fact; "2 issues" needs names.
const COUNTED: &[&str] = &[
    "update",
    "issue",
    "win",
    "item",
    "card",
    "service",
    "automation",
    "failure",
    "change",
    "task",
    "thing",
    "idea",
    "alert",
    "error",
    "notification",
    "problem",
    "result",
    "post",
    "story",
    "message",
];

/// Why a card is too vague to act on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Vague {
    /// Nothing under the title.
    Empty,
    /// It counts things ("2 updates") without naming that many.
    CountOnly { said: String, named: usize },
    /// A line or step was cut off ("2." with nothing after it, or "…").
    CutOff(String),
}

/// What a card shows, in order: headline, summary, then each report row.
pub fn visible_text(card: &UpdateCard) -> String {
    let mut lines = vec![
        crate::card_report::card_headline(card),
        crate::card_report::card_summary(card),
    ];
    if let Some(report) = &card.pulse.report {
        for item in &report.items {
            lines.push(format!("• {}: {}", item.found, item.summary));
        }
    }
    lines.join("\n")
}

/// `None` when the card names what it is about; else the first reason it doesn't.
pub fn vague(card: &UpdateCard) -> Option<Vague> {
    let text = visible_text(card);
    let title = card.title.trim();
    let rest: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && *l != title)
        .collect();
    if rest.is_empty() {
        return Some(Vague::Empty);
    }
    if let Some(cut) = rest.iter().find(|l| cut_off(l)) {
        return Some(Vague::CutOff((*cut).to_string()));
    }
    let named = named_entries(&rest);
    let (n, said) = rest
        .iter()
        .filter_map(|l| largest_count(l))
        .max_by_key(|(n, _)| *n)?;
    (named < n).then_some(Vague::CountOnly { said, named })
}

/// A bare step number, a trailing ellipsis, or a dangling "and N more".
fn cut_off(line: &str) -> bool {
    let bare_step = line
        .strip_suffix('.')
        .is_some_and(|d| !d.is_empty() && d.chars().all(|c| c.is_ascii_digit()));
    bare_step
        || line.ends_with('…')
        || line.ends_with("...")
        || line.split_whitespace().rev().nth(2) == Some("and") && line.ends_with("more")
}

/// Lines that name one thing each: bullets, numbered steps with words, and
/// "Label: a; b" lines (one per part) unless the label is itself the count.
fn named_entries(lines: &[&str]) -> usize {
    lines
        .iter()
        .map(|l| {
            if let Some(rest) = l.strip_prefix("• ").or_else(|| l.strip_prefix("- ")) {
                usize::from(!rest.trim().is_empty())
            } else if let Some((num, rest)) = l.split_once(". ") {
                usize::from(num.chars().all(|c| c.is_ascii_digit()) && !rest.trim().is_empty())
            } else if let Some((label, rest)) = l.split_once(": ") {
                // "2 updates: A · B" only repeats the count with titles.
                if label.split_whitespace().count() <= 4
                    && largest_count(label).is_none()
                    && !rest.trim().is_empty()
                {
                    rest.split(['·', ';'])
                        .filter(|p| !p.trim().is_empty())
                        .count()
                } else {
                    0
                }
            } else {
                0
            }
        })
        .sum()
}

/// The biggest "N <counted noun>" in a line, allowing one word between
/// ("2 service/automation issues").
fn largest_count(line: &str) -> Option<(usize, String)> {
    let words: Vec<&str> = line.split_whitespace().collect();
    let mut best: Option<(usize, String)> = None;
    for (i, w) in words.iter().enumerate() {
        let Ok(n) = w
            .trim_matches(|c: char| !c.is_ascii_digit())
            .parse::<usize>()
        else {
            continue;
        };
        if n < 2 || !w.chars().next().is_some_and(|c| c.is_ascii_digit()) {
            continue;
        }
        for gap in 1..=2 {
            let Some(noun) = words.get(i + gap) else {
                break;
            };
            let noun = noun
                .trim_matches(|c: char| !c.is_alphanumeric())
                .to_ascii_lowercase();
            let single = noun.strip_suffix('s').unwrap_or(&noun);
            if COUNTED.contains(&single) {
                let said = words[i..=i + gap].join(" ");
                if best.as_ref().is_none_or(|(b, _)| n > *b) {
                    best = Some((n, said));
                }
                break;
            }
        }
    }
    best
}

#[cfg(test)]
#[path = "card_quality_tests.rs"]
mod tests;
