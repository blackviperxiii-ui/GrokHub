//! What a Pulse card found, at three depths: a headline that says what was
//! found, a short summary of why the card is here, and the full report the
//! card expands to. Producers fill it before the card posts, and chat opened
//! from the card starts from all of it.

use serde::{Deserialize, Serialize};

use crate::update_feed::UpdateCard;

/// Longest headline a report keeps.
pub const FOUND_MAX: usize = 96;
/// Longest summary a report keeps: two or three short sentences.
pub const SUMMARY_MAX: usize = 280;
/// Longest full report a card stores.
pub const DETAILS_MAX: usize = 6000;

/// The thing a card is about. Chat opened from the card reads "it" as this.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SubjectKind {
    Automation,
    /// A program or background service on this computer (`bluetoothd`).
    Program,
}

impl SubjectKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Automation => "the automation",
            Self::Program => "the program",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CardSubject {
    pub kind: SubjectKind,
    pub name: String,
}

/// One update inside a digest card: what it found, and the card to open.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportItem {
    pub card_id: String,
    pub found: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub summary: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CardReport {
    /// One line: what was found.
    pub found: String,
    /// What it is, why the card showed up, why it matters.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub summary: String,
    /// The full background report, markdown.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub details: String,
    /// Short facts the finding rests on.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub evidence: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subject: Option<CardSubject>,
    /// A digest's updates, each with the card it opens.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub items: Vec<ReportItem>,
    /// Up to two fixes the card offers, best first. Nothing runs until tapped.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub offers: Vec<FixOffer>,
    /// What the app did for this card, oldest first: its audit trail.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub receipts: Vec<CardReceipt>,
}

/// Receipts a card keeps; older ones drop off.
pub const RECEIPTS_MAX: usize = 5;

/// One thing the app did for a card, and what came of it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CardReceipt {
    pub at_ms: u64,
    /// "Checked bluetoothd's crash history".
    pub what: String,
    /// Plain lines: what it found, or why it couldn't.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub result: Vec<String>,
}

/// How a fix offer runs. A chat offer is asked in the card's chat; a look is
/// read only and runs quietly, with its receipt on the card.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum OfferKind {
    #[default]
    Chat,
    /// Read the card's program's crash history and services. Changes nothing.
    Look,
}

impl OfferKind {
    fn is_chat(&self) -> bool {
        *self == Self::Chat
    }
}

/// Add `receipt` to the card's report, keeping the last [`RECEIPTS_MAX`].
pub fn add_receipt(card: &mut UpdateCard, receipt: CardReceipt) {
    let report = card.pulse.report.get_or_insert_with(Default::default);
    report.receipts.push(receipt);
    let extra = report.receipts.len().saturating_sub(RECEIPTS_MAX);
    report.receipts.drain(..extra);
}

/// One fix a card offers: the chip's words and the request it starts.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FixOffer {
    /// Short and says what it does: "Find out why", "Restart bluetooth".
    pub label: String,
    /// What the fix run is asked to do, in the card's own chat.
    pub prompt: String,
    #[serde(default, skip_serializing_if = "OfferKind::is_chat")]
    pub kind: OfferKind,
    /// Its kind of fix, for the switch that lets it run without a tap.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fix_type: Option<crate::auto_fix::FixType>,
}

/// What the card found: the report's headline, else the card's title.
pub fn card_headline(card: &UpdateCard) -> String {
    card.pulse
        .report
        .as_ref()
        .map(|r| r.found.trim())
        .filter(|found| !found.is_empty())
        .unwrap_or(card.title.trim())
        .to_string()
}

/// The short summary under the headline: the report's, else the card's body.
pub fn card_summary(card: &UpdateCard) -> String {
    card.pulse
        .report
        .as_ref()
        .map(|r| r.summary.trim())
        .filter(|summary| !summary.is_empty())
        .or(card.body.as_deref().map(str::trim))
        .unwrap_or("")
        .to_string()
}

/// What the card opens to under its summary: the full report and the facts
/// behind it, as markdown. `None` when there is nothing past the summary.
/// A digest's updates are listed by the page itself, one row per card.
pub fn report_more(card: &UpdateCard) -> Option<String> {
    let report = card.pulse.report.as_ref()?;
    let summary = report.summary.trim();
    let mut out = String::new();
    let details = report.details.trim();
    if !details.is_empty() && details != summary {
        out.push_str(details);
    }
    if !report.evidence.is_empty() {
        if !out.is_empty() {
            out.push_str("\n\n");
        }
        out.push_str("**What was checked**");
        for fact in &report.evidence {
            out.push_str("\n- ");
            out.push_str(fact.trim());
        }
    }
    for receipt in &report.receipts {
        if !out.is_empty() {
            out.push_str("\n\n");
        }
        out.push_str(&format!("**{}**", receipt.what.trim()));
        for line in &receipt.result {
            out.push_str("\n- ");
            out.push_str(line.trim());
        }
    }
    (!out.is_empty()).then_some(out)
}

/// The card opens to more than its summary: a report, its facts, or a digest's updates.
pub fn has_more(card: &UpdateCard) -> bool {
    report_more(card).is_some()
        || card
            .pulse
            .report
            .as_ref()
            .is_some_and(|r| !r.items.is_empty())
}

/// Heads a card's context on the first turn of a chat opened from that card.
pub const CARD_CARRY_HEAD: &str =
    "The person opened this chat from a card in their feed. The card:";
/// Ends the card's context; the person's own message follows it.
pub const CARD_CARRY_END: &str = "(End of the card.)";

/// The card's context as the first turn of its chat carries it.
pub fn card_carry(context: &str) -> String {
    format!(
        "{CARD_CARRY_HEAD}\n{}\n{CARD_CARRY_END}\n\n",
        context.trim()
    )
}

/// A turn that carries a card: the card's finding (its "Pulse card:" or
/// "Feed post:" line) and the person's own words after it.
pub fn split_card_carry(text: &str) -> Option<(&str, &str)> {
    let rest = text.trim_start().strip_prefix(CARD_CARRY_HEAD)?;
    let (card, user) = rest.split_once(CARD_CARRY_END)?;
    let found = card.lines().find_map(|line| {
        line.strip_prefix("Pulse card: ")
            .or_else(|| line.strip_prefix("Feed post: "))
    })?;
    Some((found.trim(), user.trim_start()))
}

/// Everything a chat opened from a reported card starts from: the finding,
/// what it is about, the facts behind it and the full report. `None` when the
/// card has no report.
pub fn card_context(card: &UpdateCard) -> Option<String> {
    let report = card.pulse.report.as_ref()?;
    let found = card_headline(card);
    let mut out = format!("Pulse card: {found}");
    if let Some(subject) = report
        .subject
        .as_ref()
        .filter(|s| !s.name.trim().is_empty())
    {
        let name = subject.name.trim();
        out.push_str(&format!(
            "\nAbout: {} \"{name}\". \"It\" and \"this\" mean \"{name}\".",
            subject.kind.label()
        ));
    }
    let summary = report.summary.trim();
    if !summary.is_empty() {
        out.push_str("\nSummary: ");
        out.push_str(summary);
    }
    if !report.evidence.is_empty() {
        out.push_str("\nWhat was checked:");
        for fact in &report.evidence {
            out.push_str("\n- ");
            out.push_str(fact.trim());
        }
    }
    if !report.items.is_empty() {
        out.push_str("\nUpdates:");
        for item in &report.items {
            out.push_str("\n- ");
            out.push_str(item.found.trim());
            if !item.summary.trim().is_empty() {
                out.push_str(": ");
                out.push_str(item.summary.trim());
            }
        }
    }
    let details = report.details.trim();
    if !details.is_empty() && details != summary {
        out.push_str("\n\nFull report:\n");
        out.push_str(details);
    }
    if let Some(url) = card
        .citations
        .first()
        .map(|u| u.trim())
        .filter(|u| !u.is_empty())
    {
        out.push_str("\nSource: ");
        out.push_str(url);
    }
    out.push_str("\n\nYou opened this from your feed and want to act on it.");
    Some(out)
}

/// The report of an automation or loop run, from its reply. The headline is
/// the reply's first finding, the summary names the run and gives the
/// sentences after it, and the full reply is the report.
pub fn report_from_run(name: &str, reply: &str) -> Option<CardReport> {
    let prose = crate::chat_view::assistant_prose(reply);
    let prose = prose.trim();
    if prose.is_empty() {
        return None;
    }
    let lines: Vec<&str> = prose
        .lines()
        .map(plain_line)
        .filter(|l| !l.is_empty())
        .collect();
    let headings: Vec<bool> = prose
        .lines()
        .filter(|l| !plain_line(l).is_empty())
        .map(|l| l.trim_start().starts_with('#'))
        .collect();
    let mut sentences: Vec<String> = Vec::new();
    for (line, heading) in lines.iter().zip(&headings) {
        if *heading || line.ends_with(':') {
            continue;
        }
        sentences.extend(crate::update_feed::split_sentences(line));
    }
    let sentences = crate::update_feed::skip_lead_ins(sentences);
    let found = sentences
        .first()
        .cloned()
        .or_else(|| lines.first().map(|l| l.to_string()))?;
    let rest: Vec<&str> = sentences
        .iter()
        .skip(1)
        .take(2)
        .map(String::as_str)
        .collect();
    let name = name.trim();
    let summary = if rest.is_empty() {
        if name.is_empty() {
            "The latest run reported this.".to_string()
        } else {
            format!("The latest run of \"{name}\" reported this.")
        }
    } else if name.is_empty() {
        clip(&rest.join(" "), SUMMARY_MAX)
    } else {
        // The card names its automation up front, where a short body keeps it.
        clip(&format!("From \"{name}\": {}", rest.join(" ")), SUMMARY_MAX)
    };
    Some(CardReport {
        found: clip(found.trim_end_matches('.'), FOUND_MAX),
        summary,
        details: prose.chars().take(DETAILS_MAX).collect(),
        evidence: Vec::new(),
        subject: (!name.is_empty()).then(|| CardSubject {
            kind: SubjectKind::Automation,
            name: name.to_string(),
        }),
        items: Vec::new(),
        offers: Vec::new(),
        receipts: Vec::new(),
    })
}

/// A line without its markdown markers.
fn plain_line(line: &str) -> &str {
    line.trim()
        .trim_start_matches(['#', '*', '-', '>', ' '])
        .trim_end_matches('*')
        .trim()
}

/// Whitespace folded, `**` dropped, cut on a word with `…` past `max` chars.
pub(crate) fn clip(text: &str, max: usize) -> String {
    let flat = text
        .replace("**", "")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if flat.chars().count() <= max {
        return flat;
    }
    let cut: String = flat.chars().take(max.saturating_sub(1)).collect();
    let cut = match cut.rfind(' ') {
        Some(at) if at > max / 2 => cut[..at].to_string(),
        _ => cut,
    };
    format!("{}…", cut.trim_end_matches([',', ';', ':', ' ']))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_carried_card_splits_back_into_its_finding_and_the_ask() {
        let carried = format!(
            "{}Help me investigate why bluetoothd keeps crashing.",
            card_carry("Pulse card: bluetoothd crashed several times recently\nAbout: the program \"bluetoothd\".")
        );
        assert_eq!(
            carried,
            "The person opened this chat from a card in their feed. The card:\nPulse card: bluetoothd crashed several times recently\nAbout: the program \"bluetoothd\".\n(End of the card.)\n\nHelp me investigate why bluetoothd keeps crashing."
        );
        assert_eq!(
            split_card_carry(&carried),
            Some((
                "bluetoothd crashed several times recently",
                "Help me investigate why bluetoothd keeps crashing."
            ))
        );
        assert_eq!(split_card_carry("check into it for me"), None);
    }
    use crate::update_feed::{blank_card, UpdateKind};

    #[test]
    fn a_run_report_leads_with_its_finding_not_the_heading_or_lead_in() {
        let reply = "## Open issues\nHere's what I found.\n\
Three issues opened overnight: #12, #13 and #14. Two are crashes on resume. \
The third asks for a dark theme.\n- #12 crash on resume";
        let r = report_from_run("Morning issues", reply).expect("report");
        assert_eq!(r.found, "Three issues opened overnight: #12, #13 and #14");
        assert_eq!(
            r.summary,
            "From \"Morning issues\": Two are crashes on resume. The third asks for a dark theme."
        );
        assert_eq!(r.details, reply);
        assert_eq!(
            r.subject,
            Some(CardSubject {
                kind: SubjectKind::Automation,
                name: "Morning issues".into()
            })
        );
    }

    #[test]
    fn a_one_line_run_says_where_it_came_from() {
        let r = report_from_run(
            "Morning issues",
            "## Open issues\nReport 3: crash on resume.",
        )
        .expect("report");
        assert_eq!(r.found, "Report 3: crash on resume");
        assert_eq!(
            r.summary,
            "The latest run of \"Morning issues\" reported this."
        );
        assert_eq!(report_from_run("x", "   "), None);
    }

    #[test]
    fn a_long_finding_is_cut_on_a_word() {
        let long = "word ".repeat(40);
        let r = report_from_run("", &long).expect("report");
        assert_eq!(r.found.chars().count(), 95);
        assert!(r.found.ends_with("word…"), "{}", r.found);
        assert_eq!(r.summary, "The latest run reported this.");
        assert_eq!(r.subject, None);
    }

    #[test]
    fn headline_and_summary_fall_back_to_title_and_body() {
        let mut card = blank_card(
            "c".into(),
            UpdateKind::Suggestion,
            "Old title".into(),
            Some("Old body".into()),
            1,
        );
        assert_eq!(card_headline(&card), "Old title");
        assert_eq!(card_summary(&card), "Old body");
        assert_eq!(card_context(&card), None);
        card.pulse.report = Some(CardReport {
            found: "bluetoothd crashed 4 times today".into(),
            summary: "It is the Bluetooth service. Headsets drop when it crashes.".into(),
            details: "Crash times: 09:12, 10:40, 13:05, 15:30.".into(),
            evidence: vec!["crash_reports=4".into()],
            subject: Some(CardSubject {
                kind: SubjectKind::Program,
                name: "bluetoothd".into(),
            }),
            items: Vec::new(),
            offers: Vec::new(),
            receipts: Vec::new(),
        });
        card.citations = vec!["https://example.com/a".into()];
        assert_eq!(card_headline(&card), "bluetoothd crashed 4 times today");
        assert_eq!(
            card_summary(&card),
            "It is the Bluetooth service. Headsets drop when it crashes."
        );
        assert_eq!(
            card_context(&card).as_deref(),
            Some(
                "Pulse card: bluetoothd crashed 4 times today\n\
About: the program \"bluetoothd\". \"It\" and \"this\" mean \"bluetoothd\".\n\
Summary: It is the Bluetooth service. Headsets drop when it crashes.\n\
What was checked:\n- crash_reports=4\n\n\
Full report:\nCrash times: 09:12, 10:40, 13:05, 15:30.\n\
Source: https://example.com/a\n\n\
You opened this from your feed and want to act on it."
            )
        );
    }

    #[test]
    fn the_card_opens_to_its_report_and_facts() {
        let mut card = blank_card("c".into(), UpdateKind::Suggestion, "T".into(), None, 1);
        assert_eq!(report_more(&card), None);
        assert!(!has_more(&card));
        card.pulse.report = Some(CardReport {
            found: "Backup failed".into(),
            summary: "Disk full.".into(),
            details: "Disk full.".into(),
            ..Default::default()
        });
        assert_eq!(
            report_more(&card),
            None,
            "a report that is only its summary"
        );
        card.pulse.report = Some(CardReport {
            found: "Backup failed".into(),
            summary: "Disk full.".into(),
            details: "## Disk\n/dev/sda1 is 100% used.".into(),
            evidence: vec![
                "Failed runs in a row: 3".into(),
                "Last error: disk full".into(),
            ],
            ..Default::default()
        });
        assert_eq!(
            report_more(&card).as_deref(),
            Some(
                "## Disk\n/dev/sda1 is 100% used.\n\n**What was checked**\n\
- Failed runs in a row: 3\n- Last error: disk full"
            )
        );
        assert!(has_more(&card));
        card.pulse.report = Some(CardReport {
            found: "2 updates".into(),
            items: vec![ReportItem {
                card_id: "a".into(),
                found: "A".into(),
                summary: String::new(),
            }],
            ..Default::default()
        });
        assert_eq!(report_more(&card), None);
        assert!(has_more(&card), "a digest opens to its updates");
    }

    #[test]
    fn receipts_go_on_the_card_and_the_oldest_drop_off() {
        let mut card = blank_card("c".into(), UpdateKind::Suggestion, "T".into(), None, 1);
        add_receipt(
            &mut card,
            CardReceipt {
                at_ms: 5,
                what: "Checked bluetoothd's crash history".into(),
                result: vec![
                    "bluetoothd has crashed: 2 crash dumps on record.".into(),
                    "All background services are running.".into(),
                ],
            },
        );
        assert_eq!(
            report_more(&card).as_deref(),
            Some(
                "**Checked bluetoothd's crash history**\n\
- bluetoothd has crashed: 2 crash dumps on record.\n\
- All background services are running."
            )
        );
        for i in 0..6 {
            add_receipt(
                &mut card,
                CardReceipt {
                    at_ms: 10 + i,
                    what: format!("Check {i}"),
                    result: Vec::new(),
                },
            );
        }
        let kept: Vec<u64> = card
            .pulse
            .report
            .as_ref()
            .unwrap()
            .receipts
            .iter()
            .map(|r| r.at_ms)
            .collect();
        assert_eq!(kept, vec![11, 12, 13, 14, 15]);
        // An offer saved before kinds existed loads as a chat offer.
        let old: FixOffer =
            serde_json::from_str(r#"{"label":"Find out why","prompt":"Why?"}"#).unwrap();
        assert_eq!(old.kind, OfferKind::Chat);
        let look = FixOffer {
            kind: OfferKind::Look,
            fix_type: None,
            ..old
        };
        assert_eq!(
            serde_json::to_string(&look).unwrap(),
            r#"{"label":"Find out why","prompt":"Why?","kind":"look"}"#
        );
    }

    #[test]
    fn a_digest_context_lists_each_update() {
        let mut card = blank_card("d".into(), UpdateKind::Suggestion, "Digest".into(), None, 1);
        card.pulse.report = Some(CardReport {
            found: "2 updates came in during quiet hours".into(),
            items: vec![
                ReportItem {
                    card_id: "a".into(),
                    found: "Backup finished".into(),
                    summary: "Copied 3 files.".into(),
                },
                ReportItem {
                    card_id: "b".into(),
                    found: "Rent is due Friday".into(),
                    summary: String::new(),
                },
            ],
            ..Default::default()
        });
        assert_eq!(
            card_context(&card).as_deref(),
            Some(
                "Pulse card: 2 updates came in during quiet hours\n\
Updates:\n- Backup finished: Copied 3 files.\n- Rent is due Friday\n\n\
You opened this from your feed and want to act on it."
            )
        );
    }

    #[test]
    fn an_old_card_without_a_report_loads() {
        let json = r#"{"id":"x","kind":"suggestion","title":"T","createdAt":1,"status":"unread"}"#;
        let card: UpdateCard = serde_json::from_str(json).expect("loads");
        assert_eq!(card.pulse.report, None);
        let out = serde_json::to_string(&card).expect("saves");
        assert!(!out.contains("report"), "{out}");
    }
}
