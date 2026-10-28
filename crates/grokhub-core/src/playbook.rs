//! Brain 3 playbooks: how a kind of work is done. A playbook names when it
//! applies (the Brief's intent and domain, and the OS), the checklist the run
//! must cover, ordered phases of steps from read-only to safe to risky, and
//! how to back up, verify, roll back and when to give up. Data, not code:
//! shipped files in `assets/playbooks/`, user copies in `{config}/playbooks/`.
//! This module checks the shape; `grokhub-agent` also refuses any step whose
//! command is a hard-class action not marked risky.

use serde::{Deserialize, Serialize};

/// The playbooks GrokHub ships, as TOML text.
pub const BUILTIN: &[&str] = &[
    include_str!("../assets/playbooks/task-general.toml"),
    include_str!("../assets/playbooks/repair-linux.toml"),
    include_str!("../assets/playbooks/repair-windows.toml"),
    include_str!("../assets/playbooks/code-dev.toml"),
    include_str!("../assets/playbooks/files.toml"),
    include_str!("../assets/playbooks/research.toml"),
    include_str!("../assets/playbooks/desktop.toml"),
    include_str!("../assets/playbooks/game-tuning.toml"),
];

/// First words of the playbook paragraph in a turn's system prompt, so a
/// later turn can swap or drop it in place.
pub const PLAYBOOK_HEAD: &str = "Playbook for this turn:";
/// Closes the playbook paragraph: it orders the work and never changes an approval.
pub const PLAYBOOK_LINE: &str =
    "The playbook orders the work; risky steps and hard-class actions still ask first, and the cabin safety rules win.";

pub const DOMAINS: &[&str] = &[
    "system", "code", "files", "desktop", "web", "media", "gaming", "general",
];
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

/// Reasons that are not a real blocker: a run never gives up because work is
/// long or hard, only when it is truly blocked.
const SOFT_STOPS: &[&str] = &[
    "too long",
    "too many steps",
    "taking long",
    "slow",
    "not sure",
    "unsure",
    "difficult",
    "complex",
    "hard to",
    "step limit",
    "step cap",
    "tired",
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
    /// A repair probe id (for example `failed_services`) the step runs
    /// instead of a command: its fixed, read-only argv from the repair module.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub probe: String,
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
    /// The playbook as one paragraph for the turn's prompt: phases in order
    /// with their steps, then how to back up, check, undo and when to stop.
    pub fn prompt_block(&self) -> String {
        let mut out = vec![format!("{PLAYBOOK_HEAD} {}. {}", self.name, self.summary)];
        if !self.checklist.is_empty() {
            out.push(format!("Cover: {}.", self.checklist.join(", ")));
        }
        for phase in &self.phases {
            let steps: Vec<String> = phase
                .steps
                .iter()
                .map(|s| match s.run.as_str() {
                    "" => s.name.clone(),
                    run => format!("{} (`{run}`)", s.name),
                })
                .collect();
            out.push(format!(
                "{} ({}): {}.",
                phase.name,
                phase.tier.label().to_lowercase(),
                steps.join("; ")
            ));
        }
        out.push(format!("Back up: {}", self.backup));
        out.push(format!("Check: {}", self.verify));
        out.push(format!("Undo: {}", self.rollback));
        if !self.give_up.is_empty() {
            out.push(format!("Stop only when {}.", self.give_up.join(", or ")));
        }
        out.push(PLAYBOOK_LINE.to_string());
        // One paragraph: a blank line inside would split it from its head.
        out.join("\n").replace("\n\n", "\n")
    }

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

/// A playbook id: 1 to [`ID_MAX`] of a-z, 0-9 and -.
pub fn id_ok(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= ID_MAX
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// The shape checks: ids, lengths, known intents/domains/OSes, at least one
/// step, and phases and steps ordered read-only, then safe, then risky.
pub fn check(p: &Playbook) -> Result<(), String> {
    if !id_ok(&p.id) {
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
    if p.give_up.is_empty() {
        return Err("give_up must name when the run is truly blocked".into());
    }
    for item in &p.give_up {
        let lower = item.to_ascii_lowercase();
        if SOFT_STOPS.iter().any(|w| lower.contains(w)) {
            return Err(format!(
                "give_up \"{item}\" is not a real blocker; give up only when an approval is refused, something needed is missing, or every approach failed"
            ));
        }
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
            if !s.probe.is_empty() && (!s.run.is_empty() || tier != Tier::ReadOnly) {
                return Err(format!(
                    "step \"{}\": a probe step only looks, so it has no run and is look-only",
                    s.name
                ));
            }
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
/// The system prompt with this turn's playbook paragraph: an earlier one is
/// dropped, and the new one sits before the volatile tail's closing safety
/// line (added when the prompt has none). `None` only drops the old one.
/// Paragraphs are split on blank lines, so the rest is kept byte for byte.
pub fn with_playbook(system: &str, block: Option<&str>) -> String {
    let mut paras: Vec<&str> = if system.is_empty() {
        Vec::new()
    } else {
        system.split("\n\n").collect()
    };
    paras.retain(|p| !p.starts_with(PLAYBOOK_HEAD));
    if let Some(block) = block {
        let closer = crate::talk::VOLATILE_SAFETY_CLOSER;
        match paras.iter().rposition(|p| *p == closer) {
            Some(at) => paras.insert(at, block),
            None => {
                paras.push(block);
                paras.push(closer);
            }
        }
    }
    paras.join("\n\n")
}

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
        assert_eq!(
            ids,
            vec![
                "task-general",
                "repair-linux",
                "repair-windows",
                "code-dev",
                "files",
                "research",
                "desktop",
                "game-tuning"
            ]
        );
        let game = parse(BUILTIN[7]).unwrap();
        assert_eq!(
            (
                game.fit("task", "gaming", "windows"),
                game.fit("chat", "general", "windows"),
                game.fit("question", "gaming", "linux")
            ),
            (Some(2), None, Some(2))
        );
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
    fn the_prompt_block_lists_phases_in_order_and_when_to_stop() {
        let p = parse(BUILTIN[3]).unwrap();
        assert_eq!(
            p.prompt_block(),
            "Playbook for this turn: Code changes. Read the code and run the tests before editing, change one thing at a time, and run the build and tests after each change.\n\
             Cover: read before edit, it builds, tests pass.\n\
             Look (look only): See what is already changed (`git status`); Read the files and the tests around the change; Run the tests once first to know what already fails.\n\
             Change (safe, reversible): Make one small edit at a time; Add or update a test for the change; Build and run the tests after each edit.\n\
             Bigger changes (risky, asks first): Delete files or folders the change no longer needs; Push, publish or deploy the change.\n\
             Back up: Check the working tree is committed or note what was already changed (git status) before the first edit, so each change can be undone on its own.\n\
             Check: After each change, build and run the tests that cover it; a change is done only when they pass.\n\
             Undo: Undo the change that broke the build or tests (restore the file from git or the kept copy) and say what failed.\n\
             Stop only when a hard approval was refused, or hardware, access or a sign-in is missing, or every approach in the playbook failed and was checked.\n\
             The playbook orders the work; risky steps and hard-class actions still ask first, and the cabin safety rules win."
        );
        // Every shipped playbook is one paragraph, so it can be swapped whole.
        for text in BUILTIN {
            let block = parse(text).unwrap().prompt_block();
            assert!(
                !block.contains("\n\n") && block.starts_with(PLAYBOOK_HEAD),
                "{block}"
            );
        }
    }

    #[test]
    fn a_turns_playbook_paragraph_is_swapped_in_place_and_dropped_when_none() {
        let closer = crate::talk::VOLATILE_SAFETY_CLOSER;
        let style = "Talk style for this turn: short.";
        let system = format!("Rules.\n\nNotes.\n\n{style}\n\n{closer}");
        let code = parse(BUILTIN[3]).unwrap().prompt_block();
        let files = parse(BUILTIN[4]).unwrap().prompt_block();
        let one = with_playbook(&system, Some(&code));
        assert_eq!(
            one,
            format!("Rules.\n\nNotes.\n\n{style}\n\n{code}\n\n{closer}")
        );
        let two = with_playbook(&one, Some(&files));
        assert_eq!(
            two,
            format!("Rules.\n\nNotes.\n\n{style}\n\n{files}\n\n{closer}")
        );
        assert_eq!(with_playbook(&two, None), system);
        // A prompt with no closing line gets one after the playbook.
        assert_eq!(
            with_playbook("Rules.", Some(&files)),
            format!("Rules.\n\n{files}\n\n{closer}")
        );
        assert_eq!(with_playbook("Rules.", None), "Rules.");
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
            ("\"a hard approval was refused\",", "\"it is taking long\",", "give_up \"it is taking long\" is not a real blocker; give up only when an approval is refused, something needed is missing, or every approach failed"),
            ("name = \"Read what is there before changing it\"", "name = \"Look\"\nprobe = \"memory\"\nrun = \"free -b\"", "step \"Look\": a probe step only looks, so it has no run and is look-only"),
            ("name = \"Check the change before the next one\"", "name = \"Check\"\nprobe = \"memory\"", "step \"Check\": a probe step only looks, so it has no run and is look-only"),
            ("id = \"task-general\"", "id = \"task-general\"\nrequire_approval = false", "unknown field `require_approval`, expected one of `id`, `name`, `summary`, `applies_when`, `checklist`, `backup`, `verify`, `rollback`, `give_up`, `phase`"),
        ];
        for (from, to, want) in cases {
            let text = base().replacen(from, to, 1);
            assert_ne!(text, base(), "{from}");
            assert_eq!(parse(&text).unwrap_err(), *want, "{to}");
        }
        let start = base().find("give_up = [").unwrap();
        let end = start + base()[start..].find(']').unwrap() + 1;
        let none = format!("{}give_up = []{}", &base()[..start], &base()[end..]);
        assert_eq!(
            parse(&none).unwrap_err(),
            "give_up must name when the run is truly blocked"
        );
    }
}
