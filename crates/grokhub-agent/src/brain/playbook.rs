//! Brain 3: playbooks on disk. The shipped ones come from
//! `grokhub_core::playbook::BUILTIN`; the user's live in `{config}/playbooks/`
//! as `<id>.toml`, and one with a shipped id replaces it. On top of the shape
//! checks, a playbook is refused when a step runs a command GrokHub never runs
//! (the hard floor, for example `mkfs`) or marks a hard-class action (money,
//! send, delete, credentials, irreversible system) as anything but risky.
//! Risky steps still ask: a playbook never changes an approval.

use std::path::{Path, PathBuf};

use grokhub_core::playbook::{self as core_pb, Playbook, Step, Tier};

use crate::harness::{record_change, ChangeTarget, HardHit, Origin, PlaybookTarget};
use crate::repair::{repair_hit, Os, ProbeId};

/// Where a loaded playbook came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Shipped,
    /// The user's copy of a shipped playbook (Reset puts the shipped one back).
    Edited,
    /// The user's own.
    Yours,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Loaded {
    pub playbook: Playbook,
    pub source: Source,
    /// The file's text, as the editor shows it.
    pub text: String,
}

/// `{config}/playbooks`
pub fn playbooks_dir(config_dir: &Path) -> PathBuf {
    config_dir.join("playbooks")
}

fn file_for(config_dir: &Path, id: &str) -> PathBuf {
    playbooks_dir(config_dir).join(format!("{id}.toml"))
}

/// The hard-class lock: no step may run a hard-floor command, and a step that
/// runs a hard-class one must be risky. Commands go through the repair rules
/// too, so clearing caches or removing packages counts the way a repair fix
/// does. A probe step must name a repair probe for every OS the playbook
/// applies to.
pub fn lock_check(p: &Playbook) -> Result<(), String> {
    for (phase, step) in p.steps() {
        if !step.probe.is_empty() {
            probe_check(p, step)?;
        }
        if step.run.trim().is_empty() {
            continue;
        }
        match repair_hit(&step.run) {
            HardHit::Floor(f) => {
                return Err(format!(
                    "step \"{}\" runs a command GrokHub never runs ({})",
                    step.name, f.reason
                ));
            }
            HardHit::Class(c) if step.tier_in(phase) != Tier::Risky => {
                return Err(format!(
                    "step \"{}\" is a hard-class action ({}) and must be marked risky; it still asks you first",
                    step.name,
                    c.label()
                ));
            }
            _ => {}
        }
        if step.tier_in(phase) == Tier::ReadOnly && !crate::perm::shell_is_readonly(&step.run) {
            return Err(format!(
                "step \"{}\" is marked look-only but `{}` can change things",
                step.name, step.run
            ));
        }
    }
    Ok(())
}

fn probe_check(p: &Playbook, s: &Step) -> Result<(), String> {
    let (step, probe) = (s.name.as_str(), s.probe.as_str());
    // A family playbook names the OS on the step; a single-OS one on the playbook.
    let named = if s.os.is_empty() {
        &p.applies_when.os
    } else {
        &s.os
    };
    let oses: Vec<Os> = named
        .iter()
        .filter_map(|o| match o.as_str() {
            "linux" => Some(Os::Linux),
            "windows" => Some(Os::Windows),
            _ => None,
        })
        .collect();
    if oses.is_empty() || oses.len() != named.len() {
        return Err(format!(
            "step \"{step}\" names a probe, so its os (or applies_when.os) must list only linux or windows"
        ));
    }
    for os in oses {
        if ProbeId::parse(probe, os).is_none() {
            let known: Vec<&str> = ProbeId::all(os).iter().map(|p| p.key()).collect();
            return Err(format!(
                "step \"{step}\": no {os:?} probe `{probe}` (one of {})",
                known.join(", ")
            ));
        }
    }
    Ok(())
}

/// Parse, check the shape and the hard-class lock.
pub fn parse(text: &str) -> Result<Playbook, String> {
    let p = core_pb::parse(text)?;
    lock_check(&p)?;
    Ok(p)
}

fn shipped() -> Vec<(Playbook, &'static str)> {
    core_pb::BUILTIN
        .iter()
        .filter_map(|t| parse(t).ok().map(|p| (p, *t)))
        .collect()
}

/// The shipped playbooks with the user's on top, by id, and the user files
/// that were refused, each naming the file and why.
pub fn load(config_dir: &Path) -> (Vec<Loaded>, Vec<String>) {
    let mut out: Vec<Loaded> = shipped()
        .into_iter()
        .map(|(p, t)| Loaded {
            playbook: p,
            source: Source::Shipped,
            text: t.to_string(),
        })
        .collect();
    let mut problems = Vec::new();
    let mut files: Vec<PathBuf> = std::fs::read_dir(playbooks_dir(config_dir))
        .map(|rd| {
            rd.flatten()
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|e| e == "toml"))
                .collect()
        })
        .unwrap_or_default();
    files.sort_by_key(|p| p.file_stem().map(|s| s.to_os_string()));
    for path in files {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let p = match parse(&text) {
            Ok(p) => p,
            Err(e) => {
                problems.push(format!("{name}: {e}"));
                continue;
            }
        };
        if name != format!("{}.toml", p.id) {
            problems.push(format!(
                "{name}: its id is `{}`; name the file {}.toml",
                p.id, p.id
            ));
            continue;
        }
        match out.iter_mut().find(|l| l.playbook.id == p.id) {
            Some(l) => {
                *l = Loaded {
                    playbook: p,
                    source: Source::Edited,
                    text,
                }
            }
            None => out.push(Loaded {
                playbook: p,
                source: Source::Yours,
                text,
            }),
        }
    }
    (out, problems)
}

/// Check and save the editor's text as `{config}/playbooks/<id>.toml`. The
/// save is the user's version in the ChangeLedger (Brain 6), so a tweak the
/// self-improvement loop proposed on an older one is never applied over it.
pub fn save(config_dir: &Path, text: &str) -> Result<Playbook, String> {
    let p = parse(text)?;
    let target = PlaybookTarget {
        config_dir,
        id: &p.id,
    };
    record_change(
        config_dir,
        &target,
        Origin::User,
        "saved in Settings",
        || target.put(Some(text.as_bytes())),
    )?;
    Ok(p)
}

/// Copy a playbook to a new id of the user's ("<id>-copy", then "-copy-2", …).
pub fn duplicate(config_dir: &Path, id: &str) -> Result<Playbook, String> {
    let (all, _) = load(config_dir);
    let src = all
        .iter()
        .find(|l| l.playbook.id == id)
        .ok_or_else(|| format!("no playbook `{id}`"))?;
    let taken = |cand: &str| {
        all.iter().any(|l| l.playbook.id == cand) || file_for(config_dir, cand).exists()
    };
    let base: String = format!("{id}-copy")
        .chars()
        .take(core_pb::ID_MAX - 3)
        .collect();
    let new_id = std::iter::once(base.clone())
        .chain((2..100).map(|n| format!("{base}-{n}")))
        .find(|c| !taken(c))
        .ok_or("too many copies")?;
    let name: String = format!("{} (copy)", src.playbook.name)
        .chars()
        .take(core_pb::NAME_MAX)
        .collect();
    save(config_dir, &core_pb::renamed(&src.text, &new_id, &name))
}

/// Remove the user's file: an edited shipped playbook goes back to the
/// shipped one, and one of the user's own is gone.
pub fn reset(config_dir: &Path, id: &str) -> Result<(), String> {
    let path = file_for(config_dir, id);
    if !path.exists() {
        return Err(format!("`{id}` has no copy of yours to reset"));
    }
    let target = PlaybookTarget { config_dir, id };
    record_change(
        config_dir,
        &target,
        Origin::User,
        "reset in Settings",
        || target.put(None),
    )
    .map(|_| ())
}

/// This computer's OS, as playbooks name it.
pub fn host_os() -> &'static str {
    if cfg!(windows) {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else {
        "linux"
    }
}

/// The playbook that fits a Brief best, in this order: the safety checks in
/// [`Playbook::fit`] first (an excluded intent, or talk the playbook doesn't
/// name, never fits), then a specialist naming the most of intent, domain,
/// OS and mood, then the default for that kind of ask (intent only), then a generic
/// fallback that names nothing. `priority` breaks a tie, then the first listed.
pub fn pick<'a>(
    all: &'a [Loaded],
    intent: &str,
    domain: &str,
    os: &str,
    mood: &str,
) -> Option<&'a Playbook> {
    let mut best: Option<((usize, u8), &Playbook)> = None;
    for l in all {
        if let Some(score) = l.playbook.fit(intent, domain, os, mood) {
            let key = (score, l.playbook.applies_when.priority);
            if best.is_none_or(|(b, _)| key > b) {
                best = Some((key, &l.playbook));
            }
        }
    }
    best.map(|(_, p)| p)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHIPPED: &str = "task-general";

    fn with_step(tier: &str, run: &str) -> String {
        core_pb::BUILTIN[0].replacen(
            "name = \"Check the change before the next one\"",
            &format!("name = \"Check the change before the next one\"\n\n[[phase.step]]\nname = \"Do it\"\ntier = \"{tier}\"\nrun = \"{run}\""),
            1,
        )
    }

    #[test]
    fn a_playbook_can_never_mark_a_hard_class_or_floor_action_safe() {
        assert_eq!(
            parse(&with_step("risky", "mkfs.ext4 /dev/sdb1")).unwrap_err(),
            "step \"Do it\" runs a command GrokHub never runs (hard floor: mkfs)"
        );
        assert_eq!(
            parse(&with_step("safe", "rm -rf ~/Downloads/old")).unwrap_err(),
            "step \"Do it\" is a hard-class action (Delete) and must be marked risky; it still asks you first"
        );
        // Risky is allowed: it still asks.
        assert_eq!(
            parse(&with_step("risky", "rm -rf ~/Downloads/old")).map(|p| p.id),
            Ok(SHIPPED.to_string())
        );
        assert_eq!(
            parse(&with_step("safe", "systemctl restart cups")).map(|p| p.id),
            Ok(SHIPPED.to_string())
        );
        let look = core_pb::BUILTIN[0].replacen(
            "name = \"Read what is there before changing it\"",
            "name = \"Read what is there before changing it\"\nrun = \"sed -i s/a/b/ /etc/hosts\"",
            1,
        );
        assert_eq!(parse(&look).unwrap_err(), "step \"Read what is there before changing it\" is marked look-only but `sed -i s/a/b/ /etc/hosts` can change things");
    }

    #[test]
    fn edit_duplicate_and_reset_on_disk() {
        let dir = crate::harness::test_dir("playbooks");
        let (all, problems) = load(&dir);
        assert_eq!(
            (all.len(), all[0].source, problems.len()),
            (15, Source::Shipped, 0)
        );
        // Edit the shipped one: it is now yours, and Reset puts it back.
        let edited = all[0]
            .text
            .replace("name = \"Any task\"", "name = \"Any job\"");
        save(&dir, &edited).unwrap();
        let (all, _) = load(&dir);
        assert_eq!(
            (all[0].playbook.name.as_str(), all[0].source),
            ("Any job", Source::Edited)
        );
        // Duplicate makes a new id of yours.
        let copy = duplicate(&dir, SHIPPED).unwrap();
        assert_eq!(
            (copy.id.as_str(), copy.name.as_str()),
            ("task-general-copy", "Any job (copy)")
        );
        assert_eq!(duplicate(&dir, SHIPPED).unwrap().id, "task-general-copy-2");
        let (all, _) = load(&dir);
        let ids: Vec<(&str, Source)> = all
            .iter()
            .map(|l| (l.playbook.id.as_str(), l.source))
            .collect();
        assert_eq!(
            ids,
            vec![
                (SHIPPED, Source::Edited),
                ("pc-fix", Source::Shipped),
                ("code-dev", Source::Shipped),
                ("files", Source::Shipped),
                ("research", Source::Shipped),
                ("desktop", Source::Shipped),
                ("game-news-library", Source::Shipped),
                ("gaming-help", Source::Shipped),
                ("game-tuning", Source::Shipped),
                ("build-loadout", Source::Shipped),
                ("chat-hangout", Source::Shipped),
                ("wind-down", Source::Shipped),
                ("quick-answer", Source::Shipped),
                ("plan-day", Source::Shipped),
                ("creative", Source::Shipped),
                ("task-general-copy", Source::Yours),
                ("task-general-copy-2", Source::Yours)
            ]
        );
        reset(&dir, SHIPPED).unwrap();
        reset(&dir, "task-general-copy-2").unwrap();
        let (all, _) = load(&dir);
        let ids: Vec<(&str, Source)> = all
            .iter()
            .map(|l| (l.playbook.id.as_str(), l.source))
            .collect();
        assert_eq!(
            ids,
            vec![
                (SHIPPED, Source::Shipped),
                ("pc-fix", Source::Shipped),
                ("code-dev", Source::Shipped),
                ("files", Source::Shipped),
                ("research", Source::Shipped),
                ("desktop", Source::Shipped),
                ("game-news-library", Source::Shipped),
                ("gaming-help", Source::Shipped),
                ("game-tuning", Source::Shipped),
                ("build-loadout", Source::Shipped),
                ("chat-hangout", Source::Shipped),
                ("wind-down", Source::Shipped),
                ("quick-answer", Source::Shipped),
                ("plan-day", Source::Shipped),
                ("creative", Source::Shipped),
                ("task-general-copy", Source::Yours)
            ]
        );
        assert_eq!(
            reset(&dir, SHIPPED).unwrap_err(),
            "`task-general` has no copy of yours to reset"
        );
        // Brain 6: every save and reset is the user's version in the ledger.
        let ledger =
            crate::harness::ChangeLedger::load_kind(&dir, crate::harness::ChangeKind::Playbook);
        let lines: Vec<(String, crate::harness::ChangeOp, Origin, String)> = ledger
            .all()
            .iter()
            .map(|c| (c.id.clone(), c.op, c.origin, c.label.clone()))
            .collect();
        use crate::harness::ChangeOp::{Create, Delete};
        // An id the redactor reads as a key (`sk-general-copy`) is kept with dots.
        assert_eq!(
            lines,
            vec![
                (SHIPPED.into(), Create, Origin::User, "Any job".into()),
                (
                    "task.general.copy".into(),
                    Create,
                    Origin::User,
                    "Any job (copy)".into()
                ),
                (
                    "task.general.copy.2".into(),
                    Create,
                    Origin::User,
                    "Any job (copy)".into()
                ),
                (SHIPPED.into(), Delete, Origin::User, "Any job".into()),
                (
                    "task.general.copy.2".into(),
                    Delete,
                    Origin::User,
                    "Any job (copy)".into()
                ),
            ]
        );
        let dotted = PlaybookTarget {
            config_dir: &dir,
            id: "task.general.copy",
        };
        assert_eq!(
            dotted.file().unwrap(),
            playbooks_dir(&dir).join("task-general-copy.toml"),
            "the ledger id names the same file"
        );
        // A refused or misnamed file is skipped with its reason; the rest still load.
        std::fs::write(
            playbooks_dir(&dir).join("bad.toml"),
            with_step("safe", "mkfs /dev/sda"),
        )
        .unwrap();
        std::fs::write(
            playbooks_dir(&dir).join("other.toml"),
            core_pb::renamed(core_pb::BUILTIN[0], "mine", "Mine"),
        )
        .unwrap();
        let (all, problems) = load(&dir);
        assert_eq!(all.len(), 16);
        assert_eq!(
            problems,
            vec![
                "bad.toml: step \"Do it\" runs a command GrokHub never runs (hard floor: mkfs)"
                    .to_string(),
                "other.toml: its id is `mine`; name the file mine.toml".to_string(),
            ]
        );
        // Saving refuses the same way and writes nothing.
        assert!(save(&dir, &with_step("safe", "rm -rf ~/x")).is_err());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_best_fitting_playbook_is_picked() {
        let dir = crate::harness::test_dir("playbook-pick");
        let specific = core_pb::renamed(core_pb::BUILTIN[0], "repair-code-test", "Repair code")
            .replacen(
                "intent = [\"task\", \"repair\"]",
                "intent = [\"repair\"]\ndomain = [\"code\"]\nos = [\"linux\"]",
                1,
            );
        save(&dir, &specific).unwrap();
        let (all, _) = load(&dir);
        let id = |i, d, o| pick(&all, i, d, o, "calm").map(|p| p.id.clone());
        assert_eq!(id("repair", "system", "linux").as_deref(), Some("pc-fix"));
        assert_eq!(id("repair", "system", "windows").as_deref(), Some("pc-fix"));
        assert_eq!(
            id("repair", "code", "linux").as_deref(),
            Some("repair-code-test")
        );
        // One PC-fix family covers macOS too; the any-task playbook no longer does.
        assert_eq!(id("repair", "system", "macos").as_deref(), Some("pc-fix"));
        assert_eq!(id("task", "code", "linux").as_deref(), Some("code-dev"));
        assert_eq!(id("repair", "code", "windows").as_deref(), Some("code-dev"));
        assert_eq!(id("task", "files", "linux").as_deref(), Some("files"));
        assert_eq!(
            id("research", "web", "windows").as_deref(),
            Some("research")
        );
        assert_eq!(id("research", "code", "linux").as_deref(), Some("research"));
        assert_eq!(id("task", "desktop", "linux").as_deref(), Some("desktop"));
        assert_eq!(id("task", "general", "linux").as_deref(), Some(SHIPPED));
        assert_eq!(
            id("task", "gaming", "windows").as_deref(),
            Some("game-tuning")
        );
        assert_eq!(
            id("repair", "gaming", "windows").as_deref(),
            Some("game-tuning")
        );
        assert_eq!(
            id("question", "gaming", "linux").as_deref(),
            Some("gaming-help")
        );
        assert_eq!(
            id("question", "general", "linux").as_deref(),
            Some("quick-answer")
        );
        assert_eq!(id("task", "planning", "linux").as_deref(), Some("plan-day"));
        assert_eq!(
            id("question", "planning", "windows").as_deref(),
            Some("plan-day")
        );
        assert_eq!(
            id("creative", "media", "linux").as_deref(),
            Some("creative")
        );
        // Chat talking about games still gets the hangout playbook.
        assert_eq!(
            id("chat", "gaming", "windows").as_deref(),
            Some("chat-hangout")
        );
        assert_eq!(
            id("chat", "general", "linux").as_deref(),
            Some("chat-hangout")
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn chat_never_falls_through_to_a_task_playbook_and_priority_breaks_ties() {
        let dir = crate::harness::test_dir("playbook-order");
        // A generic fallback that names nothing fits any work, never small talk.
        let generic = core_pb::renamed(core_pb::BUILTIN[0], "any-work", "Any work").replacen(
            "intent = [\"task\", \"repair\"]\n# Small talk never gets a task playbook, even in a copy that names no intent.\nexclude = [\"chat\"]",
            "",
            1,
        );
        assert!(!generic.contains("exclude"));
        save(&dir, &generic).unwrap();
        // A talk playbook names chat, so chat gets it.
        let talk = generic
            .replacen("[applies_when]", "[applies_when]\nintent = [\"chat\"]", 1)
            .replace("any-work", "hangout-test");
        let (all, _) = load(&dir);
        let id = |all: &[Loaded], i, d| pick(all, i, d, "linux", "calm").map(|p| p.id.clone());
        // Chat gets the hangout playbook, a low mood the wind-down one.
        assert_eq!(id(&all, "chat", "general").as_deref(), Some("chat-hangout"));
        assert_eq!(
            pick(&all, "chat", "general", "linux", "low").map(|p| p.id.as_str()),
            Some("wind-down")
        );
        assert_eq!(
            pick(&all, "task", "general", "linux", "low").map(|p| p.id.as_str()),
            Some(SHIPPED)
        );
        // The shipped quick answer takes questions; the generic one takes the rest.
        assert_eq!(
            id(&all, "question", "general").as_deref(),
            Some("quick-answer")
        );
        assert_eq!(
            id(&all, "background", "general").as_deref(),
            Some("any-work")
        );
        // The default for its kind beats the generic fallback.
        assert_eq!(id(&all, "task", "general").as_deref(), Some(SHIPPED));
        // Excluding an intent drops it even when nothing else is named.
        let shut = generic.replacen(
            "[applies_when]",
            "[applies_when]\nexclude = [\"background\"]",
            1,
        );
        save(&dir, &shut).unwrap();
        let (all, _) = load(&dir);
        assert_eq!(id(&all, "background", "general"), None);
        // A talk playbook of the user's that names chat ties with the
        // shipped one, so its priority decides.
        save(&dir, &talk).unwrap();
        let (all, _) = load(&dir);
        assert_eq!(id(&all, "chat", "general").as_deref(), Some("chat-hangout"));
        save(
            &dir,
            &talk.replacen(
                "intent = [\"chat\"]",
                "intent = [\"chat\"]\npriority = 1",
                1,
            ),
        )
        .unwrap();
        let (all, _) = load(&dir);
        assert_eq!(id(&all, "chat", "general").as_deref(), Some("hangout-test"));
        // Two that fit equally: the higher priority wins over the first listed.
        let first = core_pb::renamed(core_pb::BUILTIN[0], "files-alt", "Files alt").replacen(
            "intent = [\"task\", \"repair\"]",
            "intent = [\"task\"]\ndomain = [\"files\"]\npriority = 5",
            1,
        );
        save(&dir, &first).unwrap();
        let (all, _) = load(&dir);
        assert_eq!(id(&all, "task", "files").as_deref(), Some("files-alt"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn work_specialists_are_picked_for_questions_plans_and_drafts() {
        let (all, _) = load(&crate::harness::test_dir("playbook-work-specialists-pick"));
        let pick_id = |text: &str| {
            let b = crate::brain::rules(&crate::brain::BriefInput {
                text,
                ..Default::default()
            })
            .unwrap_or_else(|| panic!("{text}"));
            pick(&all, b.intent.as_str(), &b.domain, "linux", b.mood.as_str()).map(|p| p.id.clone())
        };
        assert_eq!(
            pick_id("what is the capital of France?").as_deref(),
            Some("quick-answer")
        );
        assert_eq!(
            pick_id("plan my day around standup and a 3pm call").as_deref(),
            Some("plan-day")
        );
        assert_eq!(
            pick_id("write a poem about autumn").as_deref(),
            Some("creative")
        );
        assert_eq!(
            pick_id("add a retry to fetch() in src/net.rs").as_deref(),
            Some("code-dev")
        );
        assert_eq!(
            pick_id("compare the latest mid-range laptops").as_deref(),
            Some("research")
        );
        assert_eq!(
            pick_id("organize the downloads folder by date").as_deref(),
            Some("files")
        );
        assert_eq!(pick_id("hi there").as_deref(), Some("chat-hangout"));
    }

    #[test]
    fn game_tuning_is_picked_for_fps_asks_and_not_for_casual_chat() {
        let (all, _) = load(&crate::harness::test_dir("playbook-game-tuning-pick"));
        let pick_id = |text: &str| {
            let b = crate::brain::rules(&crate::brain::BriefInput {
                text,
                ..Default::default()
            })
            .unwrap_or_else(|| panic!("{text}"));
            pick(&all, b.intent.as_str(), &b.domain, "windows", &b.mood).map(|p| p.id.clone())
        };
        assert_eq!(
            pick_id("tune Cyberpunk 2077 graphics for this PC").as_deref(),
            Some("game-tuning")
        );
        assert_eq!(
            pick_id("boost FPS in Fortnite, aim for 144").as_deref(),
            Some("game-tuning")
        );
        // Casual chat gets the hangout playbook, never game tuning.
        assert_eq!(pick_id("hi there").as_deref(), Some("chat-hangout"));
        assert_eq!(pick_id("thanks!").as_deref(), Some("chat-hangout"));
        assert_eq!(
            pick_id("do you like games?").as_deref(),
            Some("chat-hangout")
        );
    }

    fn shipped_one(id: &str) -> Playbook {
        shipped()
            .into_iter()
            .map(|(p, _)| p)
            .find(|p| p.id == id)
            .unwrap()
    }

    /// The PC-fix family sits on the repair module: on each OS, every look
    /// step is one of its probes for that OS (fixed argv, read-only), and the
    /// risky steps are exactly the ones its rules call hard.
    #[test]
    fn repair_playbooks_look_first_with_repair_probes_and_ask_before_hard_steps() {
        let family = shipped_one("pc-fix");
        for (name, os) in [("linux", Os::Linux), ("windows", Os::Windows)] {
            let p = family.for_os(name);
            assert!(
                !p.backup.is_empty() && !p.verify.is_empty() && !p.rollback.is_empty(),
                "{name}"
            );
            let tiers: Vec<Tier> = p.steps().map(|(ph, s)| s.tier_in(ph)).collect();
            assert!(tiers.windows(2).all(|w| w[0] <= w[1]), "{name}: {tiers:?}");
            let mut probes: Vec<&str> = p
                .steps()
                .filter(|(_, s)| !s.probe.is_empty())
                .map(|(_, s)| s.probe.as_str())
                .collect();
            let mut all: Vec<&str> = ProbeId::all(os).iter().map(|p| p.key()).collect();
            probes.sort_unstable();
            all.sort_unstable();
            assert_eq!(probes, all, "{name} looks with every repair probe");
            for key in probes {
                let spec = crate::repair::probe_spec_with(
                    ProbeId::parse(key, os).unwrap(),
                    os,
                    &|b: &str| b == "pacman",
                );
                assert_eq!(
                    spec.as_ref().and_then(crate::repair::read_only_violation),
                    None,
                    "{name} {key}"
                );
            }
        }
        assert!(
            family
                .for_os("macos")
                .steps()
                .all(|(_, s)| s.probe.is_empty()),
            "macOS has no repair probes yet, so its look steps are plain"
        );
        let risky = |os: &str| -> Vec<(String, String)> {
            family
                .for_os(os)
                .steps()
                .filter(|(ph, s)| s.tier_in(ph) == Tier::Risky)
                .map(|(_, s)| (s.name.clone(), hit_label(&s.run)))
                .collect()
        };
        let pairs = |v: &[(&str, &str)]| -> Vec<(String, String)> {
            v.iter()
                .map(|(a, b)| (a.to_string(), b.to_string()))
                .collect()
        };
        assert_eq!(
            risky("linux"),
            pairs(&[
                ("On Arch: reinstall packages with missing files", "none"),
                ("On Arch: install pending updates", "none"),
                (
                    "On Arch: delete old installer copies to free space",
                    "Delete"
                ),
                ("On Debian or Ubuntu: reinstall a broken package", "none"),
                ("On Debian or Ubuntu: install pending updates", "none"),
                // apt's autoclean only drops outdated copies; it is risky by tier.
                (
                    "On Debian or Ubuntu: delete old installer copies to free space",
                    "none"
                ),
                ("On Fedora: reinstall a broken package", "none"),
                ("On Fedora: install pending updates", "none"),
                (
                    "On Fedora: delete old installer copies to free space",
                    "Delete"
                ),
            ])
        );
        assert_eq!(
            risky("windows"),
            pairs(&[
                ("Repair system files", "none"),
                ("Repair the Windows image", "none"),
                ("Install app updates", "none"),
                ("Delete temporary files to free space", "Delete"),
            ])
        );
        assert_eq!(
            risky("macos"),
            pairs(&[
                ("Install macOS updates", "none"),
                ("Run First Aid on the startup disk", "none"),
            ])
        );
    }

    fn hit_label(run: &str) -> String {
        match repair_hit(run) {
            HardHit::None => "none".into(),
            HardHit::Class(c) => c.label().into(),
            HardHit::Floor(f) => f.reason,
        }
    }

    #[test]
    fn a_repair_step_marked_safe_or_a_probe_for_the_wrong_os_is_refused() {
        let linux = core_pb::BUILTIN[1];
        let safe_clean = linux.replacen("name = \"Re-check with the same probe\"", "name = \"Re-check with the same probe\"\n\n[[phase.step]]\nname = \"Free space\"\nrun = \"sudo paccache -rk1\"", 1);
        assert_eq!(
            parse(&safe_clean).unwrap_err(),
            "step \"Free space\" is a hard-class action (Delete) and must be marked risky; it still asks you first"
        );
        let mkfs = linux.replacen(
            "run = \"sudo pacman -Syu --noconfirm\"",
            "run = \"mkfs.ext4 /dev/sdb1\"",
            1,
        );
        assert_eq!(
            parse(&mkfs).unwrap_err(),
            "step \"On Arch: install pending updates\" runs a command GrokHub never runs (hard floor: mkfs)"
        );
        let wrong = linux.replacen("probe = \"memory\"", "probe = \"volumes\"", 1);
        assert_eq!(
            parse(&wrong).unwrap_err(),
            "step \"Check memory\": no Linux probe `volumes` (one of disk_usage, memory, failed_services, boot_errors, network_links, dns, package_health, pending_updates)"
        );
        let any_os = linux.replacen("os = [\"linux\"]", "", 1);
        assert_eq!(
            parse(&any_os).unwrap_err(),
            "step \"List failed services\" names a probe, so its os (or applies_when.os) must list only linux or windows"
        );
        // macOS has no repair probes yet.
        let mac_probe = linux.replacen("os = [\"linux\"]", "os = [\"macos\"]", 1);
        assert_eq!(
            parse(&mac_probe).unwrap_err(),
            "step \"List failed services\" names a probe, so its os (or applies_when.os) must list only linux or windows"
        );
    }
}
