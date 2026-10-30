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

/// One-tap pills that ask the agent to grant OS/workspace paths, "activate"
/// or invent a skill, or just "reference memory" cannot succeed and doom-loop.
/// Drop them at card build time (`from_json`), when painting a stored card
/// (`parse`) and when a stalled run builds a fallback card (`is_impossible_one_tap`).
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
Only list fixes a follow-up run can actually carry out."
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
Only list fixes a follow-up run can actually carry out."
        ));
    }
    // Memory is context, not an action, and a run can't conjure a skill.
    let memory_only = [
        "reference memory",
        "use memory",
        "sealed learning",
        "memory in ",
    ]
    .iter()
    .any(|n| blob.contains(n));
    let invent_skill = ["create ", "make ", "write "]
        .iter()
        .any(|v| word_start(&blob, v))
        && word_start(&blob, "skill");
    if memory_only || invent_skill {
        return Some(format!(
            "fix `{label}` is not a one-tap the agent can do (memory or new-skill family). \
Only list fixes a follow-up run can actually carry out."
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
Only list fixes a follow-up run can actually carry out."
        ));
    }
    None
}

/// True when a one-tap label/goal (or open-todo text used as a goal) is an
/// impossible host/OS ask the agent cannot fulfill.
pub fn is_impossible_one_tap(label: &str, goal: &str) -> bool {
    impossible_host_fix_reason(label, goal).is_some()
}

/// The batch pill's label.
pub const DO_ALL: &str = "Do all";

/// Fixes that destroy, overwrite or send something still need their own tap.
/// Whole words only, so "shared" or "deleted" in a goal don't count.
fn risky_fix(fix: &Fix) -> bool {
    const RISKY: &[&str] = &[
        "delete",
        "remove",
        "uninstall",
        "wipe",
        "erase",
        "purge",
        "rm",
        "format",
        "reset",
        "overwrite",
        "force",
        "drop",
        "kill",
        "send",
        "email",
        "post",
        "publish",
        "push",
        "upload",
        "share",
        "deploy",
        "merge",
        "pay",
        "buy",
    ];
    let blob = format!("{} {}", fix.label, fix.goal).to_ascii_lowercase();
    blob.split(|c: char| !c.is_alphanumeric()).any(|w| {
        RISKY.iter().any(|r| {
            w.strip_prefix(r)
                .is_some_and(|rest| ["", "s", "es", "ing"].contains(&rest))
        })
    })
}

/// Fixes that are choices (use A or B) rather than a list to work through.
fn fixes_are_alternatives(fixes: &[Fix]) -> bool {
    const CHOICE: &[&str] = &[
        " or ",
        "instead",
        "alternatively",
        "either ",
        "option ",
        "pick one",
    ];
    if fixes.iter().any(|f| {
        let label = f.label.to_ascii_lowercase();
        let goal = f.goal.to_ascii_lowercase();
        CHOICE.iter().any(|w| label.contains(w))
            || ["instead", "alternatively", "pick one"]
                .iter()
                .any(|w| goal.contains(w))
    }) {
        return true;
    }
    const PICK_VERBS: &[&str] = &[
        "use ",
        "switch to ",
        "try ",
        "choose ",
        "pick ",
        "go with ",
        "keep ",
    ];
    PICK_VERBS.iter().any(|v| {
        fixes
            .iter()
            .filter(|f| f.label.to_ascii_lowercase().starts_with(v))
            .count()
            >= 2
    })
}

/// Read-only steps read best once the changes are made.
fn explains(fix: &Fix) -> bool {
    let label = fix.label.to_ascii_lowercase();
    [
        "explain",
        "investigate",
        "show",
        "summarize",
        "summarise",
        "review",
        "list",
        "find",
        "why",
    ]
    .iter()
    .any(|v| label.starts_with(v))
}

/// What a Do all tap runs: the fixes in order, and the ones it leaves for
/// their own tap.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DoAll {
    pub run: Vec<Fix>,
    pub skipped: Vec<Fix>,
}

impl DoAll {
    /// One prompt per fix, numbered so the queue reads as progress, then the
    /// summary ask.
    pub fn prompts(&self) -> Vec<String> {
        let n = self.run.len();
        let mut out: Vec<String> = self
            .run
            .iter()
            .enumerate()
            .map(|(i, f)| format!("{DO_ALL} {}/{n} · {}", i + 1, f.prompt()))
            .collect();
        let labels = |fixes: &[Fix]| {
            fixes
                .iter()
                .map(|f| f.label.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        };
        let mut summary = format!(
            "{DO_ALL} summary · In a few lines, say which of these got done and which didn't, and why: {}.",
            labels(&self.run)
        );
        if !self.skipped.is_empty() {
            summary.push_str(&format!(
                " Left for their own tap because they delete, overwrite or send something: {}.",
                labels(&self.skipped)
            ));
        }
        out.push(summary);
        out
    }
}

impl Findings {
    /// The Do all batch, when 2 or more independent fixes can run together.
    /// None when the fixes are alternatives to pick from, or fewer than two
    /// are safe to run without their own tap.
    pub fn do_all(&self) -> Option<DoAll> {
        if self.fixes.len() < 2 || fixes_are_alternatives(&self.fixes) {
            return None;
        }
        let (skipped, safe): (Vec<Fix>, Vec<Fix>) = self.fixes.iter().cloned().partition(risky_fix);
        if safe.len() < 2 {
            return None;
        }
        let (last, mut run): (Vec<Fix>, Vec<Fix>) = safe.into_iter().partition(explains);
        run.extend(last);
        Some(DoAll { run, skipped })
    }
}

impl Findings {
    /// From the tool's arguments: `{"findings":[{"severity","text"}],
    /// "fixes":[{"label","goal"}]}`. Findings come out most severe first.
    /// Fixes that aren't a real action are left out; see [`Self::from_json_dropping`].
    pub fn from_json(value: &Value) -> Result<Self, String> {
        Self::from_json_dropping(value).map(|(card, _)| card)
    }

    /// [`Self::from_json`] plus the labels of the fixes it left out because no
    /// follow-up run could carry them out.
    pub fn from_json_dropping(value: &Value) -> Result<(Self, Vec<String>), String> {
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
        let (mut fixes, mut dropped) = (Vec::new(), Vec::new());
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
            if impossible_host_fix_reason(&label, &goal).is_some() {
                dropped.push(label);
                continue;
            }
            fixes.push(Fix { label, goal });
        }
        Ok((Self { items, fixes }, dropped))
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

    fn card_with(fixes: &[(&str, &str)]) -> Findings {
        let fixes: Vec<Value> = fixes
            .iter()
            .map(|(label, goal)| json!({"label": label, "goal": goal}))
            .collect();
        Findings::from_json(&json!({
            "findings": [{"severity": "high", "text": "nextdns can't bind port 53"}],
            "fixes": fixes
        }))
        .unwrap()
    }

    #[test]
    fn do_all_runs_the_fixes_in_order_and_leaves_risky_ones_for_their_own_tap() {
        let card = card_with(&[
            ("Explain all", "Explain each finding in plain words."),
            (
                "Fix port 53 conflict",
                "Free port 53 for nextdns, then confirm nextdns is running.",
            ),
            (
                "Delete old journal logs",
                "Delete the 2 GB of journal logs older than a week.",
            ),
            (
                "Reinstall 6 packages",
                "Reinstall the 6 packages with missing files.",
            ),
        ]);
        let batch = card.do_all().expect("three safe fixes make a batch");
        let labels = |fixes: &[Fix]| fixes.iter().map(|f| f.label.clone()).collect::<Vec<_>>();
        // Changes first, the explanation once they're made.
        assert_eq!(
            labels(&batch.run),
            [
                "Fix port 53 conflict",
                "Reinstall 6 packages",
                "Explain all"
            ]
        );
        assert_eq!(labels(&batch.skipped), ["Delete old journal logs"]);
        assert_eq!(
            batch.prompts(),
            [
                "Do all 1/3 · Fix port 53 conflict: Free port 53 for nextdns, then confirm nextdns is running.",
                "Do all 2/3 · Reinstall 6 packages: Reinstall the 6 packages with missing files.",
                "Do all 3/3 · Explain all: Explain each finding in plain words.",
                "Do all summary · In a few lines, say which of these got done and which didn't, and why: \
Fix port 53 conflict, Reinstall 6 packages, Explain all. Left for their own tap because they \
delete, overwrite or send something: Delete old journal logs.",
            ]
        );
    }

    #[test]
    fn do_all_stays_off_for_alternatives_and_lone_safe_fixes() {
        for fixes in [
            // Pick one, not a list.
            vec![
                ("Use NextDNS", "Point DNS at NextDNS."),
                (
                    "Use systemd-resolved",
                    "Keep systemd-resolved as the resolver.",
                ),
            ],
            vec![
                ("Restart or reinstall nginx", "Restart nginx."),
                ("Fix the config", "Fix the nginx config."),
            ],
            vec![
                ("Pin the old driver", "Pin driver 550."),
                ("Roll back the kernel", "Boot the LTS kernel instead."),
            ],
            // Only one fix is safe to batch.
            vec![
                ("Fix port 53 conflict", "Free port 53 for nextdns."),
                ("Push the fix", "Commit and push the DNS fix."),
            ],
            vec![("Fix port 53 conflict", "Free port 53 for nextdns.")],
        ] {
            assert_eq!(card_with(&fixes).do_all(), None, "{fixes:?}");
        }
    }

    #[test]
    fn do_all_reads_whole_words_for_risk() {
        let card = card_with(&[
            (
                "Restore the shared config",
                "Restore the deleted shared nginx config from the backup.",
            ),
            ("Restart nginx", "Restart nginx and confirm it answers."),
            ("Send the report", "Email the report to the team."),
        ]);
        let batch = card.do_all().expect("two safe fixes");
        assert_eq!(batch.run.len(), 2);
        assert_eq!(batch.skipped[0].label, "Send the report");
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
    }

    #[test]
    fn fixes_that_are_not_real_actions_are_dropped_not_refused() {
        let (card, dropped) = Findings::from_json_dropping(&json!({
            "findings": [{"severity": "medium", "text": "Pulse can't read the Steam library"}],
            "fixes": [
                {"label": "Fix path restriction", "goal": "Grant workspace access to ~/.steam path"},
                {"label": "Activate steamlibreview", "goal": "Activate skill steamlibreview so it loads"},
                {"label": "Reference Memory in pulse logic", "goal": "Reference memory in pulse logic so it uses the library"},
                {"label": "Create pulse skill for library", "goal": "Create a pulse skill that reads the library"},
                {"label": "Restart Pulse", "goal": "Restart the Pulse feed and confirm it refreshes."}
            ]
        }))
        .unwrap();
        assert_eq!(
            dropped,
            [
                "Fix path restriction",
                "Activate steamlibreview",
                "Reference Memory in pulse logic",
                "Create pulse skill for library"
            ]
        );
        assert_eq!(
            card.to_body(),
            "Findings from this run\n\n\
             - Medium: Pulse can't read the Steam library\n\n\
             One-tap fixes\n\n\
             - Restart Pulse: Restart the Pulse feed and confirm it refreshes."
        );
        assert_eq!(
            impossible_host_fix_reason("Make pulse skill", "Make a skill for Pulse").unwrap(),
            "fix `Make pulse skill` is not a one-tap the agent can do (memory or new-skill family). Only list fixes a follow-up run can actually carry out."
        );
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
