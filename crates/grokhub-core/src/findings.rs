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

/// True when `needle` occurs in `hay` starting at a word boundary, so `load `
/// does not match inside "download " and `steam` not inside "upstream".
fn word_start(hay: &str, needle: &str) -> bool {
    hay.match_indices(needle).any(|(i, _)| {
        hay[..i]
            .chars()
            .next_back()
            .is_none_or(|c| !c.is_alphanumeric())
    })
}

/// One-tap pills that ask the agent to grant OS/workspace paths or "activate"
/// a skill cannot succeed and doom-loop. Refuse them at card build time
/// (`from_json`) and drop them when painting a stored card (`parse`) or when
/// a stalled run builds a fallback card (`is_impossible_one_tap`).
fn impossible_host_fix_reason(label: &str, goal: &str) -> Option<String> {
    let blob = format!("{label} {goal}").to_ascii_lowercase();
    const NEEDLES: &[&str] = &[
        "grant workspace",
        "grant access to ~",
        "grant access to /",
        "workspace access to",
        "path restriction",
        "activate skill",
        "enable skill",
        "os permission",
        "~/.steam",
        "escape the workspace",
    ];
    if let Some(n) = NEEDLES.iter().find(|n| blob.contains(*n)) {
        return Some(format!(
            "fix `{label}` is not a one-tap the agent can do (`{n}`). \
Use Memory, sealed learnings, or a loadable skill instead of OS permission pills."
        ));
    }
    // Paraphrases that skip the exact needles still doom-loop (Activate steamlibreview,
    // Allow Steam reads, Load X from cabin skills).
    let skill_verb = ["activate ", "enable ", "load ", "allow "]
        .iter()
        .any(|v| word_start(&blob, v));
    if skill_verb
        && (word_start(&blob, "skill")
            || word_start(&blob, "steamlib")
            || blob.contains("from cabin")
            || blob.contains("cabin skills"))
    {
        return Some(format!(
            "fix `{label}` is not a one-tap the agent can do (skill activate family). \
Use Memory, sealed learnings, or a loadable skill instead of OS permission pills."
        ));
    }
    let host_verb = ["grant", "allow ", "enable ", "unblock"]
        .iter()
        .any(|v| word_start(&blob, v));
    if host_verb
        && (word_start(&blob, "steam")
            || word_start(&blob, "workspace")
            || blob.contains("~/.")
            || blob.contains("host path")
            || blob.contains("os path"))
    {
        return Some(format!(
            "fix `{label}` is not a one-tap the agent can do (host/workspace grant family). \
Use Memory, sealed learnings, or a loadable skill instead of OS permission pills."
        ));
    }
    None
}

/// True when a one-tap label/goal (or open-todo text used as a goal) is an
/// impossible host/OS ask the agent cannot fulfill.
pub fn is_impossible_one_tap(label: &str, goal: &str) -> bool {
    impossible_host_fix_reason(label, goal).is_some()
}

impl Findings {
    /// From the tool's arguments: `{"findings":[{"severity","text"}],
    /// "fixes":[{"label","goal"}]}`. Findings come out most severe first.
    pub fn from_json(value: &Value) -> Result<Self, String> {
        let rows = value
            .get("findings")
            .and_then(Value::as_array)
            .ok_or("findings must be an array")?;
        if rows.is_empty() || rows.len() > MAX_FINDINGS {
            return Err(format!("give 1 to {MAX_FINDINGS} findings"));
        }
        let mut items = Vec::new();
        for row in rows {
            let sev = row.get("severity").and_then(Value::as_str).unwrap_or("");
            let severity = Severity::parse(sev).ok_or_else(|| {
                format!("unknown severity `{sev}`: use high, medium, low or info")
            })?;
            let text = one_line(row.get("text").and_then(Value::as_str).unwrap_or(""));
            if text.is_empty() {
                return Err("each finding needs text".into());
            }
            items.push(Finding { severity, text });
        }
        items.sort_by_key(|f| f.severity);
        let fixes_in = value
            .get("fixes")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
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
                return Err(format!(
                    "fix label `{label}` must be at most {LABEL_MAX} characters with no colon"
                ));
            }
            if let Some(why) = impossible_host_fix_reason(&label, &goal) {
                return Err(why);
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
            let Some((head, rest)) = line.strip_prefix("- ").and_then(|l| l.split_once(": "))
            else {
                continue;
            };
            if in_fixes {
                // Drop doom-loop pills that may already be on disk from older
                // runs or from a fallback card that skipped from_json.
                if impossible_host_fix_reason(head, rest).is_none() {
                    fixes.push(Fix {
                        label: head.to_string(),
                        goal: rest.to_string(),
                    });
                }
            } else if let Some(severity) = Severity::parse(head) {
                items.push(Finding {
                    severity,
                    text: rest.to_string(),
                });
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
        let no_fixes =
            Findings::from_json(&json!({"findings":[{"severity":"info","text":"all clear"}]}))
                .unwrap();
        assert_eq!(
            Findings::parse(&no_fixes.to_body()).unwrap().fixes,
            Vec::<Fix>::new()
        );
    }

    #[test]
    fn bad_cards_are_refused_with_a_reason() {
        let err = |v: Value| Findings::from_json(&v).unwrap_err();
        assert_eq!(err(json!({"findings": []})), "give 1 to 30 findings");
        assert_eq!(
            err(json!({"findings": [{"severity": "urgent", "text": "x"}]})),
            "unknown severity `urgent`: use high, medium, low or info"
        );
        assert_eq!(
            err(json!({"findings": [{"severity": "low", "text": " "}]})),
            "each finding needs text"
        );
        assert_eq!(
            err(
                json!({"findings": [{"severity": "low", "text": "x"}], "fixes": [{"label": "Fix: now", "goal": "g"}]})
            ),
            "fix label `Fix: now` must be at most 40 characters with no colon"
        );
        assert_eq!(
            err(
                json!({"findings": [{"severity": "low", "text": "x"}], "fixes": [{"label": "Go"}]})
            ),
            "each fix needs a label and a goal"
        );
        let seven: Vec<Value> = (0..7)
            .map(|n| json!({"label": format!("f{n}"), "goal": "g"}))
            .collect();
        assert_eq!(
            err(json!({"findings": [{"severity": "low", "text": "x"}], "fixes": seven})),
            "give at most 6 fixes"
        );
        let steam = err(json!({
            "findings": [{"severity": "medium", "text": "path escapes the workspace"}],
            "fixes": [{
                "label": "Fix path restriction",
                "goal": "Grant workspace access to ~/.steam path"
            }]
        }));
        assert!(
            steam.contains("not a one-tap") && steam.contains("grant workspace"),
            "{steam}"
        );
        let skill = err(json!({
            "findings": [{"severity": "low", "text": "unknown skill"}],
            "fixes": [{
                "label": "Activate steamlibreview",
                "goal": "Activate skill steamlibreview so it loads"
            }]
        }));
        assert!(skill.contains("activate skill"), "{skill}");
    }

    #[test]
    fn malformed_inputs_never_make_a_card() {
        let err = |v: Value| Findings::from_json(&v).is_err();
        assert!(err(json!(null)));
        assert!(err(json!([])));
        assert!(err(json!({})));
        assert!(err(json!({"findings": "none"})));
        assert!(err(json!({"findings": {"severity": "low", "text": "x"}})));
        assert!(err(json!({"findings": [null]})));
        assert!(err(json!({"findings": ["low: x"]})));
        assert!(err(json!({"findings": [{"severity": 3, "text": "x"}]})));
        assert!(err(json!({"findings": [{"severity": "low", "text": 7}]})));
        assert!(err(json!({"findings": [{"text": "no severity"}]})));
        assert!(err(
            json!({"findings": [{"severity": "highest", "text": "x"}]})
        ));
        assert!(err(json!({"findings": [{"severity": "", "text": "x"}]})));
        let too_many: Vec<Value> = (0..=MAX_FINDINGS)
            .map(|n| json!({"severity": "info", "text": format!("f{n}")}))
            .collect();
        assert!(err(json!({"findings": too_many})));
        let long = "x".repeat(LABEL_MAX + 1);
        assert!(err(json!({
            "findings": [{"severity": "low", "text": "x"}],
            "fixes": [{"label": long, "goal": "g"}]
        })));
        assert!(err(json!({
            "findings": [{"severity": "low", "text": "x"}],
            "fixes": [{"label": "Go", "goal": "  "}]
        })));
        // a non-array fixes value is ignored, not a card of its own
        let card = Findings::from_json(
            &json!({"findings": [{"severity": "low", "text": "x"}], "fixes": "oops"}),
        )
        .unwrap();
        assert!(card.fixes.is_empty());
    }

    #[test]
    fn parse_refuses_bodies_that_are_not_cards() {
        for body in [
            "",
            "\n",
            "findings from this run\n- High: x",
            "Findings from this runs\n- High: x",
            "Findings from this run\n\nnothing here",
            "Findings from this run\n- Urgent: not a severity",
            "Findings from this run\n- High:no space after colon",
            "Findings from this run\nHigh: no dash",
            "Findings from this run\n\nOne-tap fixes\n\n- Fix nginx: Restart it.",
        ] {
            assert_eq!(Findings::parse(body), None, "{body:?}");
        }
        // fixes-section rows never become findings
        let card = Findings::parse(
            "Findings from this run\n- Low: x\n\nOne-tap fixes\n\n- High: not a finding",
        )
        .unwrap();
        assert_eq!(card.items.len(), 1);
        assert_eq!(card.fixes[0].label, "High");
    }

    #[test]
    fn ordinary_fixes_are_not_mistaken_for_host_grants() {
        // Substrings inside longer words must not trip the doom-loop guard.
        for (label, goal) in [
            (
                "Unblock upstream DNS",
                "Unblock the upstream resolver on port 53.",
            ),
            ("Download skill report", "Download the skills usage report."),
            (
                "Reload nginx",
                "Reload nginx so the new workspacey config applies.",
            ),
            ("Restart bind", "Restart bind and confirm it answers."),
            ("Fix upstream", "Enable upstream retries in nginx."),
        ] {
            assert!(
                !is_impossible_one_tap(label, goal),
                "{label}: {goal} should be allowed"
            );
        }
        assert!(Findings::from_json(&json!({
            "findings": [{"severity": "low", "text": "x"}],
            "fixes": [{"label": "Unblock upstream DNS", "goal": "Unblock the upstream resolver."}]
        }))
        .is_ok());
    }

    #[test]
    fn parse_drops_impossible_one_taps_already_on_disk() {
        let body = "Findings from this run\n\n\
             - Medium: Not finished: Activate skill steamlibreview so it loads\n\n\
             One-tap fixes\n\n\
             - Finish the rest: Finish what the last run left open: Activate skill steamlibreview so it loads.\n\
             - Fix nginx: Restart nginx and confirm it is listening.";
        let card = Findings::parse(body).unwrap();
        assert_eq!(card.fixes.len(), 1);
        assert_eq!(card.fixes[0].label, "Fix nginx");
        assert!(
            is_impossible_one_tap(
                "Finish the rest",
                "Finish what the last run left open: Activate skill steamlibreview so it loads."
            ),
            "needle must still catch activate skill in fallback goals"
        );
        assert!(
            is_impossible_one_tap("Activate steamlibreview", "Enable steamlibreview in cabin"),
            "paraphrase without the exact 'activate skill' needle must still block"
        );
        assert!(
            is_impossible_one_tap(
                "Allow Steam reads",
                "Allow Steam workspace reads for the agent"
            ),
            "host/workspace grant paraphrase must still block"
        );
        assert!(
            is_impossible_one_tap(
                "Load steamlibreview",
                "Load steamlibreview from cabin skills"
            ),
            "load-from-cabin-skills paraphrase must still block"
        );
    }
}
