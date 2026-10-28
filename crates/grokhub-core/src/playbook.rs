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
    include_str!("../assets/playbooks/pc-fix.toml"),
    include_str!("../assets/playbooks/code-dev.toml"),
    include_str!("../assets/playbooks/files.toml"),
    include_str!("../assets/playbooks/research.toml"),
    include_str!("../assets/playbooks/desktop.toml"),
    include_str!("../assets/playbooks/game-news-library.toml"),
    include_str!("../assets/playbooks/gaming-help.toml"),
    include_str!("../assets/playbooks/game-tuning.toml"),
    include_str!("../assets/playbooks/build-loadout.toml"),
    include_str!("../assets/playbooks/chat-hangout.toml"),
    include_str!("../assets/playbooks/wind-down.toml"),
    include_str!("../assets/playbooks/quick-answer.toml"),
    include_str!("../assets/playbooks/plan-day.toml"),
    include_str!("../assets/playbooks/creative.toml"),
];

/// The moods a Brief can carry.
pub const MOODS: &[&str] = &["calm", "urgent", "frustrated", "low"];

/// First words of the playbook paragraph in a turn's system prompt, so a
/// later turn can swap or drop it in place.
pub const PLAYBOOK_HEAD: &str = "Playbook for this turn:";
/// Closes the playbook paragraph: it orders the work and never changes an approval.
pub const PLAYBOOK_LINE: &str =
    "The playbook orders the work; risky steps and hard-class actions still ask first, and the cabin safety rules win.";

pub const DOMAINS: &[&str] = &[
    "system",
    "code",
    "files",
    "desktop",
    "web",
    "media",
    "gaming",
    "game-help",
    "loadout",
    "library",
    "planning",
    "general",
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
    /// Brief moods (`calm`, `urgent`, `frustrated`, `low`); empty matches any.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mood: Vec<String>,
    /// Brief intents that never get this playbook, even when `intent` is empty.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub exclude: Vec<String>,
    /// Breaks a tie between two playbooks that fit equally well; higher wins.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub priority: u8,
}

fn is_zero(n: &u8) -> bool {
    *n == 0
}

/// Talk, not work: a playbook fits these intents only when it names them, so
/// small talk never falls through to a task playbook.
pub const TALK_INTENTS: &[&str] = &["chat"];

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
    /// `linux`, `windows`, `macos`: the step is for these only; empty is every OS.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub os: Vec<String>,
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

    /// Only the steps for `os` (and those for every OS); a phase left with
    /// none is dropped. One family file covers Linux, Windows and macOS.
    pub fn for_os(&self, os: &str) -> Playbook {
        let mut p = self.clone();
        for phase in &mut p.phases {
            phase
                .steps
                .retain(|s| s.os.is_empty() || s.os.iter().any(|o| o == os));
        }
        p.phases.retain(|ph| !ph.steps.is_empty());
        p
    }

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

    /// How closely it fits: one point per named intent, domain, OS and mood
    /// that matches; `None` when any named one doesn't, when it excludes the
    /// intent, or when the intent is talk and the playbook doesn't name it.
    pub fn fit(&self, intent: &str, domain: &str, os: &str, mood: &str) -> Option<usize> {
        let a = &self.applies_when;
        if a.exclude.iter().any(|x| x == intent)
            || (TALK_INTENTS.contains(&intent) && !a.intent.iter().any(|x| x == intent))
        {
            return None;
        }
        let mut score = 0;
        for (list, v) in [
            (&a.intent, intent),
            (&a.domain, domain),
            (&a.os, os),
            (&a.mood, mood),
        ] {
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
        ("mood", &a.mood, MOODS),
        ("exclude", &a.exclude, crate::brain_policy::INTENTS),
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
            if let Some(bad) = s
                .os
                .iter()
                .find(|o| !OSES.contains(&o.as_str()) || (!a.os.is_empty() && !a.os.contains(o)))
            {
                return Err(format!(
                    "step \"{}\": os `{bad}` must be one of {}",
                    s.name,
                    if a.os.is_empty() {
                        OSES.join(", ")
                    } else {
                        a.os.join(", ")
                    }
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
                "pc-fix",
                "code-dev",
                "files",
                "research",
                "desktop",
                "game-news-library",
                "gaming-help",
                "game-tuning",
                "build-loadout",
                "chat-hangout",
                "wind-down",
                "quick-answer",
                "plan-day",
                "creative",
            ]
        );
        let game = parse(BUILTIN[8]).unwrap();
        assert_eq!(
            (
                game.fit("task", "gaming", "windows", "calm"),
                game.fit("chat", "general", "windows", "calm"),
                game.fit("question", "gaming", "linux", "calm")
            ),
            (Some(2), None, Some(2))
        );
        let help = parse(BUILTIN[7]).unwrap();
        assert_eq!(
            (
                help.fit("question", "game-help", "windows", "calm"),
                help.fit("task", "gaming", "windows", "calm"),
                help.fit("question", "gaming", "linux", "calm")
            ),
            (Some(2), None, None)
        );
        let loadout = parse(BUILTIN[9]).unwrap();
        assert_eq!(
            (
                loadout.fit("research", "loadout", "linux", "calm"),
                loadout.fit("task", "gaming", "windows", "calm")
            ),
            (Some(2), None)
        );
        let library = parse(BUILTIN[6]).unwrap();
        assert_eq!(
            (
                library.fit("question", "library", "windows", "calm"),
                library.fit("research", "web", "linux", "calm")
            ),
            (Some(2), None)
        );
        let quick = parse(BUILTIN[12]).unwrap();
        let plan = parse(BUILTIN[13]).unwrap();
        let creative = parse(BUILTIN[14]).unwrap();
        assert_eq!(
            (
                quick.fit("question", "general", "linux", "calm"),
                quick.fit("task", "general", "linux", "calm"),
                plan.fit("task", "planning", "linux", "calm"),
                plan.fit("task", "general", "linux", "calm"),
                creative.fit("creative", "media", "linux", "calm"),
                creative.fit("task", "media", "linux", "calm"),
            ),
            (Some(1), None, Some(2), None, Some(1), None)
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
                p.fit("repair", "system", "linux", "calm"),
                p.fit("chat", "general", "linux", "calm")
            ),
            (Some(1), None)
        );
        // The PC-fix family carries the repair lessons on every OS.
        let fix = parse(BUILTIN[1]).unwrap();
        assert!(fix.verify.ends_with(
            "After a restart you started, wait for the computer to come back, check what you fixed again, and say what you saw."
        ));
        assert!(fix
            .verify
            .contains("Say a fix is approved only after the user said yes, and done only after the check passed."));
        for os in OSES {
            let block = fix.for_os(os).prompt_block();
            for lesson in [
                "Fix the setting itself, like a unit's path, keeping the .bak; a symlink around it is only a stopgap",
                "A permission-denied answer means the check needs admin rights, not that something is missing",
                "Take a restore point before the first change",
            ] {
                assert!(block.contains(lesson), "{os}: {lesson}");
            }
        }
        assert_eq!(
            (
                fix.fit("repair", "system", "macos", "calm"),
                fix.fit("repair", "system", "windows", "calm"),
                fix.fit("task", "system", "linux", "calm")
            ),
            (Some(3), Some(3), None)
        );
    }

    #[test]
    fn a_family_playbook_keeps_only_this_computers_steps() {
        let fix = parse(BUILTIN[1]).unwrap();
        let names = |os: &str| -> Vec<String> {
            fix.for_os(os)
                .steps()
                .map(|(_, s)| s.name.clone())
                .collect()
        };
        let mac = names("macos");
        assert!(mac.contains(&"Install macOS updates".to_string()));
        assert!(!mac
            .iter()
            .any(|n| n.starts_with("On Arch") || n == "Repair system files"));
        let win = names("windows");
        assert!(win.contains(&"Repair system files".to_string()));
        assert!(!win
            .iter()
            .any(|n| n.starts_with("On Fedora") || n == "Install macOS updates"));
        // Windows has nothing in the copy-a-config step's OS list, and keeps the shared ones.
        assert!(!win.contains(&"Copy a config file before editing it".to_string()));
        assert!(win.contains(&"Take a restore point before the first change".to_string()));
        let linux = fix.for_os("linux").prompt_block();
        assert!(linux
            .contains("On Debian or Ubuntu: install pending updates (`sudo apt-get upgrade -y`)"));
        assert!(linux.contains("On Fedora: install pending updates (`sudo dnf upgrade -y`)"));
        assert!(!linux.contains("winget"));
        // A step can only name an OS the playbook applies to.
        let bad = BUILTIN[1].replacen("os = [\"macos\"]", "os = [\"bsd\"]", 1);
        assert_eq!(
            parse(&bad).unwrap_err(),
            "step \"Check disk space (df -h)\": os `bsd` must be one of linux, windows, macos"
        );
    }

    #[test]
    fn the_prompt_block_lists_phases_in_order_and_when_to_stop() {
        let p = parse(BUILTIN[2]).unwrap();
        assert_eq!(
            p.prompt_block(),
            "Playbook for this turn: Code changes. Reproduce first, make the smallest change, prove it with tests, then give a short summary.\n\
             Cover: reproduced, smallest change, tests pass, summary given.\n\
             Reproduce (look only): See what is already changed (`git status`); Reproduce the failure or the ask before editing anything; Read the files and the tests around the change; Run the tests once first to know what already fails.\n\
             Change (safe, reversible): Make the smallest edit that fixes it; Add or update a test for the change; Build and run the tests after each edit; Give a short summary of the change and how it was checked.\n\
             Bigger changes (risky, asks first): Delete files or folders the change no longer needs; Push, publish or deploy the change.\n\
             Back up: Check the working tree is committed or note what was already changed (git status) before the first edit, so each change can be undone on its own.\n\
             Check: After each change, build and run the tests that cover it; a change is done only when they pass and a short summary is ready.\n\
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
        let code = parse(BUILTIN[2]).unwrap().prompt_block();
        let files = parse(BUILTIN[3]).unwrap().prompt_block();
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
