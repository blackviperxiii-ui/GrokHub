//! Findings card: what a run found, by severity, and the one-tap fixes it
//! offers. The agent's `report_findings` tool builds it; the cabin stores it
//! as a result bubble ([`Findings::to_body`]) and reads it back
//! ([`Findings::parse`]) to draw a pill per fix. A tapped pill sends
//! [`Fix::prompt`] as a follow-up run in the same chat.

use serde_json::Value;

/// First line of a findings bubble.
pub const FINDINGS_HEAD: &str = "Findings from this run";
const FIXES_HEAD: &str = "One-tap fixes";
pub const MAX_FINDINGS: usize = 30;
pub const MAX_FIXES: usize = 6;
/// Pill labels stay short enough for one row.
pub const LABEL_MAX: usize = 40;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    High,
    Medium,
    Low,
    Info,
}

impl Severity {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "high" | "critical" => Some(Self::High),
            "medium" => Some(Self::Medium),
            "low" => Some(Self::Low),
            "info" => Some(Self::Info),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::High => "High",
            Self::Medium => "Medium",
            Self::Low => "Low",
            Self::Info => "Info",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub severity: Severity,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fix {
    /// The pill text, e.g. "Fix port 53 conflict".
    pub label: String,
    /// What the follow-up run should get done.
    pub goal: String,
}

impl Fix {
    /// The follow-up run's prompt.
    pub fn prompt(&self) -> String {
        format!("{}: {}", self.label, self.goal)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Findings {
    pub items: Vec<Finding>,
    pub fixes: Vec<Fix>,
}

fn one_line(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

impl Findings {
    /// From the tool's arguments: `{"findings":[{"severity","text"}],
    /// "fixes":[{"label","goal"}]}`. Findings come out most severe first.
    pub fn from_json(value: &Value) -> Result<Self, String> {
        let rows = value.get("findings").and_then(Value::as_array).ok_or("findings must be an array")?;
        if rows.is_empty() || rows.len() > MAX_FINDINGS {
            return Err(format!("give 1 to {MAX_FINDINGS} findings"));
        }
        let mut items = Vec::new();
        for row in rows {
            let sev = row.get("severity").and_then(Value::as_str).unwrap_or("");
            let severity = Severity::parse(sev).ok_or_else(|| format!("unknown severity `{sev}`: use high, medium, low or info"))?;
            let text = one_line(row.get("text").and_then(Value::as_str).unwrap_or(""));
            if text.is_empty() {
                return Err("each finding needs text".into());
            }
            items.push(Finding { severity, text });
        }
        items.sort_by_key(|f| f.severity);
        let fixes_in = value.get("fixes").and_then(Value::as_array).cloned().unwrap_or_default();
        if fixes_in.len() > MAX_FIXES {
            return Err(format!("give at most {MAX_FIXES} fixes"));
        }
        let mut fixes = Vec::new();
        for row in &fixes_in {
            let label = one_line(row.get("label").and_then(Value::as_str).unwrap_or(""));
            let goal = one_line(row.get("goal").and_then(Value::as_str).unwrap_or(""));
            if label.is_empty() || goal.is_empty() {
                return Err("each fix needs a label and a goal".into());
            }
            if label.contains(':') || label.chars().count() > LABEL_MAX {
                return Err(format!("fix label `{label}` must be at most {LABEL_MAX} characters with no colon"));
            }
            fixes.push(Fix { label, goal });
        }
        Ok(Self { items, fixes })
    }

    /// The bubble text. It reads as a plain report, and [`Self::parse`]
    /// gets the same card back.
    pub fn to_body(&self) -> String {
        let mut out = vec![FINDINGS_HEAD.to_string(), String::new()];
        for f in &self.items {
            out.push(format!("- {}: {}", f.severity.label(), f.text));
        }
        if !self.fixes.is_empty() {
            out.push(String::new());
            out.push(FIXES_HEAD.to_string());
            out.push(String::new());
            for fix in &self.fixes {
                out.push(format!("- {}: {}", fix.label, fix.goal));
            }
        }
        out.join("\n")
    }

    pub fn parse(body: &str) -> Option<Self> {
        let mut lines = body.lines();
        if lines.next()?.trim() != FINDINGS_HEAD {
            return None;
        }
        let (mut items, mut fixes, mut in_fixes) = (Vec::new(), Vec::new(), false);
        for line in lines {
            let line = line.trim();
            if line == FIXES_HEAD {
                in_fixes = true;
                continue;
            }
            let Some((head, rest)) = line.strip_prefix("- ").and_then(|l| l.split_once(": ")) else {
                continue;
            };
            if in_fixes {
                fixes.push(Fix { label: head.to_string(), goal: rest.to_string() });
            } else if let Some(severity) = Severity::parse(head) {
                items.push(Finding { severity, text: rest.to_string() });
            }
        }
        (!items.is_empty()).then_some(Self { items, fixes })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn scan() -> Value {
        json!({
            "findings": [
                {"severity": "medium", "text": "6 packages have missing files (amd-ucode, bind, cups, linux-cachyos, linux-cachyos-lts, nfs-utils)"},
                {"severity": "high", "text": "nextdns can't bind port 53:\n systemd-resolved owns it"},
                {"severity": "low", "text": "20 boot errors (Bluetooth, PipeWire)"}
            ],
            "fixes": [
                {"label": "Fix port 53 conflict", "goal": "Stop systemd-resolved's stub listener so nextdns can bind port 53, then confirm nextdns is running."},
                {"label": "Reinstall 6 packages", "goal": "Reinstall the 6 packages with missing files and re-run pacman -Qk."},
                {"label": "Investigate Bluetooth errors", "goal": "Find the cause of the Bluetooth boot errors."},
                {"label": "Explain all", "goal": "Explain each finding in plain words."}
            ]
        })
    }

    #[test]
    fn the_card_lists_findings_most_severe_first_with_its_fixes() {
        let card = Findings::from_json(&scan()).unwrap();
        assert_eq!(
            card.to_body(),
            "Findings from this run\n\n\
             - High: nextdns can't bind port 53: systemd-resolved owns it\n\
             - Medium: 6 packages have missing files (amd-ucode, bind, cups, linux-cachyos, linux-cachyos-lts, nfs-utils)\n\
             - Low: 20 boot errors (Bluetooth, PipeWire)\n\n\
             One-tap fixes\n\n\
             - Fix port 53 conflict: Stop systemd-resolved's stub listener so nextdns can bind port 53, then confirm nextdns is running.\n\
             - Reinstall 6 packages: Reinstall the 6 packages with missing files and re-run pacman -Qk.\n\
             - Investigate Bluetooth errors: Find the cause of the Bluetooth boot errors.\n\
             - Explain all: Explain each finding in plain words."
        );
        assert_eq!(card.fixes[1].prompt(), "Reinstall 6 packages: Reinstall the 6 packages with missing files and re-run pacman -Qk.");
    }

    #[test]
    fn the_bubble_reads_back_as_the_same_card() {
        let card = Findings::from_json(&scan()).unwrap();
        assert_eq!(Findings::parse(&card.to_body()), Some(card));
        assert_eq!(Findings::parse("Some other reply\n- High: x"), None);
        assert_eq!(Findings::parse(FINDINGS_HEAD), None);
        let no_fixes = Findings::from_json(&json!({"findings":[{"severity":"info","text":"all clear"}]})).unwrap();
        assert_eq!(Findings::parse(&no_fixes.to_body()).unwrap().fixes, Vec::<Fix>::new());
    }

    #[test]
    fn bad_cards_are_refused_with_a_reason() {
        let err = |v: Value| Findings::from_json(&v).unwrap_err();
        assert_eq!(err(json!({"findings": []})), "give 1 to 30 findings");
        assert_eq!(err(json!({"findings": [{"severity": "urgent", "text": "x"}]})), "unknown severity `urgent`: use high, medium, low or info");
        assert_eq!(err(json!({"findings": [{"severity": "low", "text": " "}]})), "each finding needs text");
        assert_eq!(
            err(json!({"findings": [{"severity": "low", "text": "x"}], "fixes": [{"label": "Fix: now", "goal": "g"}]})),
            "fix label `Fix: now` must be at most 40 characters with no colon"
        );
        assert_eq!(err(json!({"findings": [{"severity": "low", "text": "x"}], "fixes": [{"label": "Go"}]})), "each fix needs a label and a goal");
        let seven: Vec<Value> = (0..7).map(|n| json!({"label": format!("f{n}"), "goal": "g"})).collect();
        assert_eq!(err(json!({"findings": [{"severity": "low", "text": "x"}], "fixes": seven})), "give at most 6 fixes");
    }
}
