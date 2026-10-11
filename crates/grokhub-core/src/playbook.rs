//! Brain 3 playbooks: how a kind of work is done. A playbook names when it
//! applies (the Brief's intent and domain, and the OS), the checklist the run
//! must cover, ordered phases of steps from read-only to safe to risky, and
//! how to back up, verify, roll back and when to give up. Data, not code:
//! shipped files in `assets/playbooks/`, user copies in `{config}/playbooks/`.
//! This module checks the shape; `grokhub-agent` also refuses any step whose
//! command is a hard-class action not marked risky.

use serde::{Deserialize, Serialize};

/// The playbooks GrokHub ships, as TOML text.
pub const BUILTIN: &[&str] = &[include_str!("../assets/playbooks/task-general.toml")];

pub const DOMAINS: &[&str] = &["system", "code", "files", "web", "media", "general"];
pub const OSES: &[&str] = &["linux", "windows", "macos"];
pub const ID_MAX: usize = 48;
pub const NAME_MAX: usize = 60;
/// Longest summary, backup, verify, rollback or step text.
pub const TEXT_MAX: usize = 300;
pub const STEPS_MAX: usize = 60;

/// Words no playbook text may use: a playbook can't drop an approval.
const REFUSED: &[&str] = &[
    "skip approval",
    "no approval",
    "without approval",
    "bypass",
    "don't ask",
    "never ask",
    "without asking",
];

/// How much a step can change. Phases and steps run in this order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tier {
    /// Looks only.
    ReadOnly,
    /// Changes that can be undone (with the backup kept).
    Safe,
    /// Anything else. Hard-class actions still ask.
    Risky,
}

impl Tier {
    pub fn label(self) -> &'static str {
        match self {
            Self::ReadOnly => "Look only",
            Self::Safe => "Safe, reversible",
            Self::Risky => "Risky, asks first",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppliesWhen {
    /// Brief intents; empty matches any.
    #[serde(default)]
    pub intent: Vec<String>,
    /// Brief domains; empty matches any.
    #[serde(default)]
    pub domain: Vec<String>,
    /// `linux`, `windows`, `macos`; empty matches any.
    #[serde(default)]
    pub os: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Step {
    /// Plain words, for example "List failed services".
    pub name: String,
    /// A shell command the step runs, when it has one.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub run: String,
    /// Defaults to the phase's tier.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tier: Option<Tier>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Phase {
    pub name: String,
    pub tier: Tier,
    #[serde(default, rename = "step")]
    pub steps: Vec<Step>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Playbook {
    pub id: String,
    pub name: String,
    pub summary: String,
    #[serde(default)]
    pub applies_when: AppliesWhen,
    #[serde(default)]
    pub checklist: Vec<String>,
    pub backup: String,
    pub verify: String,
    pub rollback: String,
    #[serde(default)]
    pub give_up: Vec<String>,
    #[serde(default, rename = "phase")]
    pub phases: Vec<Phase>,
}

impl Step {
    pub fn tier_in(&self, phase: &Phase) -> Tier {
        self.tier.unwrap_or(phase.tier)
    }
}

impl Playbook {
    /// Every step with its phase, in run order.
    pub fn steps(&self) -> impl Iterator<Item = (&Phase, &Step)> {
        self.phases
            .iter()
            .flat_map(|p| p.steps.iter().map(move |s| (p, s)))
    }

    /// How closely it fits: one point per named intent, domain and OS that
    /// matches; `None` when any named one doesn't.
    pub fn fit(&self, intent: &str, domain: &str, os: &str) -> Option<usize> {
        let a = &self.applies_when;
        let mut score = 0;
        for (list, v) in [(&a.intent, intent), (&a.domain, domain), (&a.os, os)] {
            if list.is_empty() {
                continue;
            }
            if !list.iter().any(|x| x == v) {
                return None;
            }
            score += 1;
        }
        Some(score)
    }
}

fn text(field: &str, s: &str, max: usize) -> Result<(), String> {
    if s.trim().is_empty() || s.chars().count() > max {
        return Err(format!("{field} must be 1 to {max} characters"));
    }
    let lower = s.to_ascii_lowercase();
    if let Some(w) = REFUSED.iter().find(|w| lower.contains(*w)) {
        return Err(format!(
            "{field} says \"{w}\"; a playbook can't drop an approval"
        ));
    }
    Ok(())
}

/// The shape checks: ids, lengths, known intents/domains/OSes, at least one
/// step, and phases and steps ordered read-only, then safe, then risky.
pub fn check(p: &Playbook) -> Result<(), String> {
    let id_ok = !p.id.is_empty()
        && p.id.len() <= ID_MAX
        && p.id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    if !id_ok {
        return Err(format!("id must be 1 to {ID_MAX} of a-z, 0-9 and -"));
    }
    text("name", &p.name, NAME_MAX)?;
    for (field, s) in [
        ("summary", &p.summary),
        ("backup", &p.backup),
        ("verify", &p.verify),
        ("rollback", &p.rollback),
    ] {
        text(field, s, TEXT_MAX)?;
    }
    for item in p.checklist.iter().chain(p.give_up.iter()) {
        text("a checklist or give_up item", item, TEXT_MAX)?;
    }
    let a = &p.applies_when;
    for (field, list, known) in [
        ("intent", &a.intent, crate::brain_policy::INTENTS),
        ("domain", &a.domain, DOMAINS),
        ("os", &a.os, OSES),
    ] {
        if let Some(bad) = list.iter().find(|v| !known.contains(&v.as_str())) {
            return Err(format!(
                "applies_when.{field}: unknown `{bad}` (one of {})",
                known.join(", ")
            ));
        }
    }
    let count = p.steps().count();
    if count == 0 || count > STEPS_MAX {
        return Err(format!("a playbook needs 1 to {STEPS_MAX} steps"));
    }
    let mut last = Tier::ReadOnly;
    for phase in &p.phases {
        text("a phase name", &phase.name, NAME_MAX)?;
        for s in &phase.steps {
            text("a step name", &s.name, TEXT_MAX)?;
            if s.run.chars().count() > TEXT_MAX {
                return Err(format!(
                    "step \"{}\": run must be at most {TEXT_MAX} characters",
                    s.name
                ));
            }
            let tier = s.tier_in(phase);
            if tier < last {
                return Err(format!(
                    "step \"{}\" is {} after a {} step; order steps look-only, then safe, then risky",
                    s.name,
                    tier.label().to_lowercase(),
                    last.label().to_lowercase()
                ));
            }
            last = tier;
        }
    }
    Ok(())
}

/// Parse one playbook file and check its shape.
pub fn parse(text: &str) -> Result<Playbook, String> {
    let p: Playbook = toml::from_str(text).map_err(|e| e.message().to_string())?;
    check(&p)?;
    Ok(p)
}

/// The same file with a new top-level `id` and `name`, comments kept.
pub fn renamed(text: &str, id: &str, name: &str) -> String {
    let mut top = true;
    let lines: Vec<String> = text
        .lines()
        .map(|line| {
            let t = line.trim_start();
            if t.starts_with('[') {
                top = false;
            }
            let key = t.split('=').next().unwrap_or("").trim();
            match (top, key) {
                (true, "id") => format!("id = {}", toml::Value::String(id.into())),
                (true, "name") => format!("name = {}", toml::Value::String(name.into())),
                _ => line.to_string(),
            }
        })
        .collect();
    lines.join("\n") + "\n"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shipped_playbooks_parse() {
        let ids: Vec<String> = BUILTIN.iter().map(|t| parse(t).unwrap().id).collect();
        assert_eq!(ids, vec!["task-general".to_string()]);
        let p = parse(BUILTIN[0]).unwrap();
        assert_eq!(
            p.steps()
                .map(|(ph, s)| (ph.name.as_str(), s.tier_in(ph)))
                .collect::<Vec<_>>(),
            vec![
                ("Look", Tier::ReadOnly),
                ("Change", Tier::Safe),
                ("Change", Tier::Safe)
            ]
        );
        assert_eq!(
            (
                p.fit("repair", "system", "linux"),
                p.fit("chat", "general", "linux")
            ),
            (Some(1), None)
        );
    }

    #[test]
    fn a_renamed_copy_keeps_its_steps_and_comments() {
        let copy = renamed(BUILTIN[0], "task-general-copy", "Any task (copy)");
        let p = parse(&copy).unwrap();
        assert_eq!(
            (p.id.as_str(), p.name.as_str()),
            ("task-general-copy", "Any task (copy)")
        );
        assert_eq!(p.phases, parse(BUILTIN[0]).unwrap().phases);
        assert!(copy.starts_with("# GrokHub playbook"));
        // A step named "name" inside a phase is left alone.
        assert_eq!(copy.matches("name = \"Look\"").count(), 1);
    }

    fn base() -> String {
        BUILTIN[0].to_string()
    }

    #[test]
    fn bad_shapes_are_refused_with_the_reason() {
        let cases: &[(&str, &str, &str)] = &[
            ("id = \"task-general\"", "id = \"Task General\"", "id must be 1 to 48 of a-z, 0-9 and -"),
            ("intent = [\"task\", \"repair\"]", "intent = [\"fixing\"]", "applies_when.intent: unknown `fixing` (one of chat, question, task, repair, creative, research, background)"),
            ("name = \"Check the change before the next one\"", "name = \"x\"\ntier = \"read_only\"", "step \"x\" is look only after a safe, reversible step; order steps look-only, then safe, then risky"),
            ("rollback = \"If", "rollback = \"Bypass approvals. If", "rollback says \"bypass\"; a playbook can't drop an approval"),
            ("id = \"task-general\"", "id = \"task-general\"\nrequire_approval = false", "unknown field `require_approval`, expected one of `id`, `name`, `summary`, `applies_when`, `checklist`, `backup`, `verify`, `rollback`, `give_up`, `phase`"),
        ];
        for (from, to, want) in cases {
            let text = base().replacen(from, to, 1);
            assert_ne!(text, base(), "{from}");
            assert_eq!(parse(&text).unwrap_err(), *want, "{to}");
        }
    }
}
