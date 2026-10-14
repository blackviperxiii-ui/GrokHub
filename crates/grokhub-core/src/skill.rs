use crate::is_plain_text;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SkillMd {
    pub name: String,
    pub description: String,
    pub slash: String,
    pub trigger: String,
    pub instructions: String,
    pub pitfalls: String,
    pub verify: String,
    pub runs: u32,
}

pub fn skill_dir_name(name: &str) -> String {
    let s: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let mut out = String::new();
    for c in s.chars() {
        if c == '-' && out.ends_with('-') {
            continue;
        }
        out.push(c);
    }
    out.trim_matches('-').to_string()
}

pub fn render_skill_md(s: &SkillMd) -> String {
    format!(
        "---\nname: {}\ndescription: {}\nslash: {}\ntrigger: {}\nruns: {}\n---\n\n# {}\n\nTrigger when the user says {}.\n\n## Steps\n{}\n\n## Pitfalls\n{}\n\n## Verify\n{}\n",
        s.name,
        s.description,
        s.slash,
        s.trigger,
        s.runs,
        s.name.replace('-', " "),
        if s.trigger.is_empty() { s.slash.as_str() } else { s.trigger.as_str() },
        s.instructions,
        s.pitfalls,
        s.verify
    )
}

pub fn parse_skill_md(raw: &str) -> SkillMd {
    let mut s = SkillMd::default();
    let mut section = String::new();
    let mut in_fm = false;
    for line in raw.lines() {
        if line.trim() == "---" {
            in_fm = !in_fm;
            continue;
        }
        if in_fm {
            if let Some((k, v)) = line.split_once(':') {
                match k.trim() {
                    "name" => s.name = v.trim().to_string(),
                    "description" => s.description = v.trim().to_string(),
                    "slash" => s.slash = v.trim().to_string(),
                    "trigger" => s.trigger = v.trim().to_string(),
                    "runs" => s.runs = v.trim().parse().unwrap_or(0),
                    _ => {}
                }
            }
            continue;
        }
        if let Some(rest) = line.strip_prefix("## ") {
            section = rest.trim().to_ascii_lowercase();
            continue;
        }
        if let Some(rest) = line.strip_prefix("# ") {
            if s.name.is_empty() {
                s.name = skill_dir_name(rest);
            }
            continue;
        }
        if line.to_ascii_lowercase().starts_with("trigger when") && s.trigger.is_empty() {
            s.trigger = line
                .split("says")
                .nth(1)
                .unwrap_or("")
                .trim()
                .trim_end_matches('.')
                .to_string();
            continue;
        }
        let dest = match section.as_str() {
            "steps" => &mut s.instructions,
            "pitfalls" => &mut s.pitfalls,
            "verify" => &mut s.verify,
            _ => continue,
        };
        if !dest.is_empty() {
            dest.push('\n');
        }
        dest.push_str(line);
    }
    s
}

pub fn skill_safe(body: &str) -> bool {
    is_plain_text(body)
}

/// Write a watch-and-learn skill draft under `{skills_dir}/<name>/SKILL.md`.
/// Refuses when the folder already exists (no overwrite) or [`skill_safe`] fails.
/// Pure filesystem helper for SD2 — no ledger, no network.
pub fn save_watched_skill(skills_dir: &Path, skill: &SkillMd) -> io::Result<PathBuf> {
    if !skill_safe(&skill.instructions)
        || !skill_safe(&skill.pitfalls)
        || !skill_safe(&skill.description)
        || !skill_safe(&skill.trigger)
        || !skill_safe(&skill.verify)
        || !skill_safe(&skill.name)
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "skill text is not safe to save",
        ));
    }
    let name = skill_dir_name(&skill.name);
    if name.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "skill name is empty",
        ));
    }
    let dir = skills_dir.join(&name);
    if dir.exists() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("skill `{name}` already exists"),
        ));
    }
    fs::create_dir_all(&dir)?;
    let path = dir.join("SKILL.md");
    fs::write(&path, render_skill_md(skill))?;
    Ok(path)
}

pub fn bump_skill_run(runs: u32) -> u32 {
    runs.saturating_add(1)
}

pub fn is_hard_run(
    tool_calls: u32,
    recovered_error: bool,
    user_corrected: bool,
    scratch: bool,
) -> bool {
    if scratch {
        return false;
    }
    tool_calls >= 5 || recovered_error || user_corrected
}

fn words(s: &str) -> Vec<String> {
    s.to_ascii_lowercase()
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| w.len() >= 2)
        .map(|s| s.to_string())
        .collect()
}

fn jaccard(a: &[String], b: &[String]) -> f32 {
    if a.is_empty() && b.is_empty() {
        return 1.0;
    }
    let mut inter = 0usize;
    for w in a {
        if b.iter().any(|x| x == w) {
            inter += 1;
        }
    }
    let union = a.len() + b.len() - inter;
    if union == 0 {
        0.0
    } else {
        inter as f32 / union as f32
    }
}

pub fn match_skill<'a>(user_text: &str, skills: &'a [SkillMd]) -> Option<&'a SkillMd> {
    let t = user_text.trim();
    if t.is_empty() {
        return None;
    }
    if let Some(tok) = t.split_whitespace().next() {
        if tok.starts_with('/') {
            if let Some(hit) = skills.iter().find(|s| s.slash.eq_ignore_ascii_case(tok)) {
                return Some(hit);
            }
        }
    }
    // Anticipate and Use in chat without a slash send `Follow skill {name}`.
    // Jaccard against the trigger misses that line — match the name first.
    if let Some(name) = follow_skill_name(t) {
        if let Some(hit) = skills.iter().find(|s| skill_named(s, name)) {
            return Some(hit);
        }
    }
    // Whole-phrase / OR-split triggers beat bag-of-words: "deploy OR update the
    // install" must hit "please update the install" without a false Jaccard win.
    let norm = normalize_skill_ask(t);
    let mut phrase_hit: Option<&SkillMd> = None;
    let mut phrase_len = 0usize;
    for s in skills {
        let raw = if s.trigger.trim().is_empty() {
            s.name.as_str()
        } else {
            s.trigger.as_str()
        };
        for phrase in trigger_phrases(raw) {
            let p = normalize_skill_ask(&phrase);
            if p.len() < 4 {
                continue;
            }
            if ask_contains_phrase(&norm, &p) && p.len() > phrase_len {
                phrase_hit = Some(s);
                phrase_len = p.len();
            }
        }
    }
    if phrase_hit.is_some() {
        return phrase_hit;
    }
    let q = words(t);
    // Short asks + low Jaccard are noise; require a longer query or a stronger score.
    let mut best: Option<&SkillMd> = None;
    let mut best_score = 0.0f32;
    for s in skills {
        let trig = words(if s.trigger.is_empty() {
            &s.name
        } else {
            &s.trigger
        });
        let score = jaccard(&q, &trig);
        let ok = if q.len() >= 3 {
            score >= 0.5
        } else {
            score >= 0.66
        };
        if ok && score > best_score {
            best = Some(s);
            best_score = score;
        }
    }
    best
}

fn normalize_skill_ask(s: &str) -> String {
    s.to_ascii_lowercase()
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Split a skill trigger on OR / commas into candidate phrases.
fn trigger_phrases(trigger: &str) -> Vec<String> {
    let mut out = Vec::new();
    for chunk in trigger.split([',', '/']) {
        for part in chunk.split(" OR ").flat_map(|p| p.split(" or ")) {
            let t = part.trim();
            if !t.is_empty() {
                out.push(t.to_string());
            }
        }
    }
    if out.is_empty() {
        let t = trigger.trim();
        if !t.is_empty() {
            out.push(t.to_string());
        }
    }
    out
}

fn ask_contains_phrase(norm_ask: &str, norm_phrase: &str) -> bool {
    if norm_ask == norm_phrase {
        return true;
    }
    let padded = format!(" {norm_ask} ");
    let needle = format!(" {norm_phrase} ");
    padded.contains(&needle)
}

fn follow_skill_name(t: &str) -> Option<&str> {
    let rest = t
        .strip_prefix("Follow skill ")
        .or_else(|| t.strip_prefix("follow skill "))?;
    let name = rest.trim();
    if name.is_empty() {
        None
    } else {
        Some(name)
    }
}

fn skill_named(s: &SkillMd, name: &str) -> bool {
    s.name.eq_ignore_ascii_case(name)
        || s.slash.eq_ignore_ascii_case(name)
        || s.slash.eq_ignore_ascii_case(&format!("/{name}"))
}

/// Skills "Use in chat" must send a line `match_skill` actually hits.
pub fn skill_use_in_chat_prompt(slash: &str, name: &str) -> String {
    let s = slash.trim();
    if s.starts_with('/') {
        s.to_string()
    } else {
        format!("Follow skill {name}")
    }
}

/// Prepend the active skill follow so the engine sees the steps.
pub fn apply_skill_follow(prompt: &str, follow: Option<&str>) -> String {
    match follow.map(str::trim).filter(|s| !s.is_empty()) {
        Some(block) => format!("{block}\n\n{prompt}"),
        None => prompt.to_string(),
    }
}

pub fn skill_follow_block(skill: &SkillMd) -> String {
    format!(
        "Active skill {} — follow these steps:\n## Steps\n{}\n\n## Pitfalls\n{}\n\n## Verify\n{}",
        skill.name,
        skill.instructions.trim(),
        skill.pitfalls.trim(),
        skill.verify.trim()
    )
}

/// Keep name/slash/runs; replace steps and verify from the new run.
pub fn patch_skill(existing: &SkillMd, proposed: &SkillMd) -> SkillMd {
    SkillMd {
        name: existing.name.clone(),
        description: if proposed.description.trim().is_empty() {
            existing.description.clone()
        } else {
            proposed.description.clone()
        },
        slash: existing.slash.clone(),
        trigger: if proposed.trigger.trim().is_empty() {
            existing.trigger.clone()
        } else {
            proposed.trigger.clone()
        },
        instructions: proposed.instructions.clone(),
        pitfalls: if proposed.pitfalls.trim().is_empty() {
            existing.pitfalls.clone()
        } else {
            proposed.pitfalls.clone()
        },
        verify: if proposed.verify.trim().is_empty() {
            existing.verify.clone()
        } else {
            proposed.verify.clone()
        },
        runs: existing.runs,
    }
}

pub fn prefer_patch(existing: &[SkillMd], proposed: &SkillMd) -> Option<String> {
    let slash = proposed.slash.to_ascii_lowercase();
    if let Some(hit) = existing
        .iter()
        .find(|s| s.slash.to_ascii_lowercase() == slash)
    {
        return Some(hit.name.clone());
    }
    let prop = words(&proposed.trigger);
    let mut best: Option<&SkillMd> = None;
    let mut best_score = 0.0f32;
    for s in existing {
        let score = jaccard(
            &prop,
            &words(if s.trigger.is_empty() {
                &s.name
            } else {
                &s.trigger
            }),
        );
        if score >= 0.5 && score > best_score {
            best = Some(s);
            best_score = score;
        }
    }
    best.map(|s| s.name.clone())
}

/// Feedback on the last try ("actually still no…", "good job now…") is not a new task.
/// A prefix that ends in a letter or digit matches only at a word boundary
/// ("stop" is feedback, "stopwatch" is not). Prefixes that already end in a
/// space, comma, apostrophe, or colon keep matching as a raw prefix.
pub fn is_feedback_ask(user: &str) -> bool {
    let t = user.trim().to_ascii_lowercase();
    const STARTS: &[&str] = &[
        "actually",
        "no ",
        "no,",
        "nope",
        "still",
        "ok so",
        "okay so",
        "hey",
        "good job",
        "nice",
        "great",
        "thanks",
        "thank you",
        "that was",
        "you ",
        "you'",
        "almost",
        "close",
        "wrong",
        "not ",
        "stop",
        "wait",
        "again",
        "lets try",
        "let's try",
        "try again",
        "retry",
        "continue",
        "keep going",
        "go on",
        "yes",
        "yep",
        "new update",
        "update:",
    ];
    STARTS.iter().any(|prefix| {
        let Some(rest) = t.strip_prefix(prefix) else {
            return false;
        };
        match prefix.chars().next_back() {
            Some(c) if c.is_ascii_alphanumeric() => !rest
                .chars()
                .next()
                .is_some_and(|n| n.is_ascii_alphanumeric()),
            _ => true,
        }
    })
}

/// Program a host command runs (`cargo`, `git`, `systemctl`), for naming a skill.
fn command_tool(cmd: &str) -> Option<String> {
    cmd.split_whitespace()
        .map(|w| w.trim_matches(|c: char| c == '`' || c == '"' || c == '\''))
        .find(|w| {
            !w.is_empty()
                && !w.contains('=')
                && !matches!(*w, "sudo" | "env" | "cd" | "&&" | "time" | "nice" | "exec")
        })
        .map(|w| w.rsplit('/').next().unwrap_or(w).to_ascii_lowercase())
        .filter(|w| {
            w.len() >= 2
                && w.chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        })
}

/// A skill only when a run left a reusable procedure: at least two host commands,
/// a situation worth repeating (a fix, a build, a routine), and an ask that is a
/// task, not feedback on the last try. Mouse and keyboard driving is never saved:
/// those turns were the source of skills named after whole corrections.
/// Pitfalls on the skill are [`AUTO_SKILL_PITFALL`], the auto-made marker.
pub fn propose_skill_from_turn(
    user_text: &str,
    assistant_text: &str,
    host_commands: &[String],
) -> Option<SkillMd> {
    use crate::situation::MoveKind;
    let user = user_text.replace('\n', " ");
    let user: String = user.chars().take(120).collect();
    if is_feedback_ask(&user) {
        return None;
    }
    let cmds: Vec<&String> = host_commands
        .iter()
        .filter(|c| !c.trim().is_empty())
        .filter(|c| {
            let l = c.to_ascii_lowercase();
            !(l.contains("xdotool")
                || l.contains("ydotool")
                || l.contains("mousemove")
                || l.contains("click"))
        })
        .collect();
    if cmds.len() < 2 {
        return None;
    }
    let learned = crate::situation::learn_from_turns(&user, assistant_text)?;
    if !matches!(
        learned.kind,
        MoveKind::Fix
            | MoveKind::Build
            | MoveKind::Wrap
            | MoveKind::Friday
            | MoveKind::Morning
            | MoveKind::Night
            | MoveKind::Automate
    ) {
        return None;
    }
    let tool = cmds.iter().find_map(|c| command_tool(c));
    let base = learned.skill_name.trim();
    let name = skill_dir_name(&match tool.as_deref() {
        Some(t) => format!("{base}-{t}"),
        None => base.to_string(),
    });
    if name.is_empty() {
        return None;
    }
    let steps = cmds
        .iter()
        .enumerate()
        .map(|(i, c)| format!("{}. `{c}`", i + 1))
        .collect::<Vec<_>>()
        .join("\n");
    let verify = cmds.last().map(|c| c.to_string()).unwrap_or_default();
    let description = match tool.as_deref() {
        Some(t) => format!("{} Uses `{t}`.", learned.skill_description.trim()),
        None => learned.skill_description.clone(),
    };
    Some(SkillMd {
        name: name.clone(),
        description,
        slash: format!("/{name}"),
        trigger: learned.skill_trigger.clone(),
        instructions: steps,
        pitfalls: AUTO_SKILL_PITFALL.into(),
        verify: format!("{verify} exits 0"),
        runs: 0,
    })
}

/// Pitfalls line every skill from [`propose_skill_from_turn`] carries.
/// `is_junk_skill` retires only skills that still contain it, so a hand-written
/// skill with empty or custom pitfalls stays put.
pub const AUTO_SKILL_PITFALL: &str =
    "Do not run destructive commands without a receipt and confirm.";

/// Generic skill names the cabin used to write from a template, with no real steps.
const TEMPLATE_SKILLS: &[&str] = &[
    "take-the-next-step",
    "saved-run",
    "keep-this-way",
    "explain-the-why",
    "do-it-here",
    "choose-and-start",
    "do-the-next-slice",
    "make-the-image",
    "save-the-routine",
];

/// An auto-made skill that never ran and holds nothing reusable: a template name,
/// a name that is a whole sentence someone typed, or a description that is that
/// sentence copied. Auto-made means `pitfalls` contains [`AUTO_SKILL_PITFALL`].
/// A hand-written skill stays, and so does any skill that has already run.
/// The cabin moves junk aside instead of listing it.
pub fn is_junk_skill(s: &SkillMd) -> bool {
    if s.runs > 0 {
        return false;
    }
    if !s.pitfalls.trim().contains(AUTO_SKILL_PITFALL) {
        return false;
    }
    let name = s.name.trim().to_ascii_lowercase();
    if TEMPLATE_SKILLS.contains(&name.as_str()) {
        return true;
    }
    let words = name.split('-').filter(|w| !w.is_empty()).count();
    let spoken = name.replace('-', " ");
    let desc: String = s
        .description
        .to_ascii_lowercase()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || c.is_whitespace())
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let desc_is_name = !desc.is_empty() && (desc.starts_with(&spoken) || spoken.starts_with(&desc));
    words >= 7
        || (words >= 3 && desc_is_name)
        || is_feedback_ask(&spoken)
        || desc == "saved host procedure"
}

#[cfg(test)]
mod tests {
    use super::*;

    fn watched_draft(name: &str, instructions: &str) -> SkillMd {
        SkillMd {
            name: name.into(),
            description: format!("Replay the watched steps for {name}"),
            slash: format!("/{name}"),
            trigger: name.replace('-', " "),
            instructions: instructions.into(),
            pitfalls: "Step 2 asks for approval on replay (delete).".into(),
            verify: "After the last step: Confirm the folder is gone".into(),
            runs: 0,
        }
    }

    #[test]
    fn save_watched_skill_round_trips_and_refuses_existing_or_unsafe() {
        let root = std::env::temp_dir().join(format!(
            "gh-save-watched-{}-{}",
            std::process::id(),
            now_stamp()
        ));
        let _ = fs::remove_dir_all(&root);
        let skills = root.join("skills");
        fs::create_dir_all(&skills).unwrap();

        let draft = watched_draft(
            "open-settings-display",
            "1. Open Settings\n2. Delete the old profile\n3. Confirm the folder is gone",
        );
        let path = save_watched_skill(&skills, &draft).expect("save");
        assert_eq!(path, skills.join("open-settings-display").join("SKILL.md"));
        let raw = fs::read_to_string(&path).unwrap();
        let round = parse_skill_md(&raw);
        assert_eq!(round.name, draft.name);
        assert_eq!(round.runs, 0);
        assert_eq!(round.instructions.trim(), draft.instructions.trim());
        assert!(round.pitfalls.contains("delete"));

        let again = save_watched_skill(&skills, &draft);
        assert_eq!(
            again.unwrap_err().kind(),
            io::ErrorKind::AlreadyExists,
            "never overwrite an existing skill dir"
        );

        let mut unsafe_skill = draft.clone();
        unsafe_skill.instructions = "1. Paste sk-abcdefghijklmnopqrstuv into the field".into();
        assert_eq!(
            save_watched_skill(&skills.join("other"), &unsafe_skill)
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidData
        );

        let _ = fs::remove_dir_all(&root);
    }

    fn now_stamp() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0)
    }

    #[test]
    fn dir_and_roundtrip() {
        assert_eq!(skill_dir_name("Deploy User Install"), "deploy-user-install");
        let src = SkillMd {
            name: "deploy-user-install".into(),
            description: "Sync and restart the user GrokHub install".into(),
            slash: "/deploy".into(),
            trigger: "deploy OR update the install".into(),
            instructions: "1. run sync script\n2. restart".into(),
            pitfalls: "do not sudo rm the running tree".into(),
            verify: "grokhub --version prints the new version".into(),
            runs: 0,
        };
        let md = render_skill_md(&src);
        assert!(md.starts_with("---\nname: deploy-user-install"));
        assert!(md.contains("## Verify"));
        let parsed = parse_skill_md(&md);
        assert_eq!(parsed.name, "deploy-user-install");
        assert_eq!(
            parsed.slash, "/deploy",
            "slash must survive save/reload or /deploy never matches"
        );
        assert_eq!(parsed.trigger, "deploy OR update the install");
        assert!(parsed.verify.contains("version"));
        assert_eq!(bump_skill_run(0), 1);
        assert!(is_hard_run(5, false, false, false));
        assert!(!is_hard_run(5, false, false, true));
        let flash = SkillMd {
            name: "flash-pi".into(),
            description: "write an image".into(),
            slash: "/flash".into(),
            trigger: "flash the pi".into(),
            instructions: "dd".into(),
            pitfalls: "boot disk".into(),
            verify: "lsblk".into(),
            runs: 0,
        };
        let skills = [flash.clone()];
        let hit = match_skill("flash the pi", &skills).unwrap();
        assert_eq!(hit.name, "flash-pi");
        let use_chat = skill_use_in_chat_prompt("/flash", "flash-pi");
        assert_eq!(use_chat, "/flash");
        assert_eq!(
            match_skill(&use_chat, &skills).unwrap().name,
            "flash-pi",
            "Use in chat must activate the skill, not send a vague Follow skill line"
        );
        assert_eq!(
            match_skill("Follow skill flash-pi", &skills).unwrap().name,
            "flash-pi",
            "anticipate / Use in chat must hit Follow skill <name>"
        );
        let deploy = SkillMd {
            name: "deploy-user-install".into(),
            description: "Sync and restart".into(),
            slash: "/deploy".into(),
            trigger: "deploy OR update the install".into(),
            instructions: "1. sync".into(),
            pitfalls: "do not sudo rm".into(),
            verify: "version".into(),
            runs: 0,
        };
        let both = [flash.clone(), deploy];
        assert_eq!(
            match_skill("please update the install tonight", &both)
                .unwrap()
                .name,
            "deploy-user-install",
            "OR-split trigger phrases must activate without Jaccard"
        );
        assert!(
            match_skill("pi", &both).is_none(),
            "a one-token ask must not Jaccard-activate a skill"
        );
        assert!(
            propose_skill_from_turn("flash the pi", "ok", &["dd if=a".into()]).is_none(),
            "one command is not a reusable procedure"
        );
        let proposed = SkillMd {
            name: "flash-pi".into(),
            description: "Writes the image to the card.".into(),
            slash: "/flash".into(),
            trigger: String::new(),
            instructions: "1. `dd if=a`".into(),
            pitfalls: String::new(),
            verify: String::new(),
            runs: 0,
        };
        assert_eq!(
            prefer_patch(std::slice::from_ref(&flash), &proposed),
            Some("flash-pi".into())
        );
        let patched = patch_skill(&flash, &proposed);
        assert_eq!(patched.name, "flash-pi");
        assert_eq!(patched.slash, "/flash");
        assert!(patched.instructions.contains("dd if=a"));
        let follow = skill_follow_block(&flash);
        assert!(follow.contains("Active skill flash-pi"));
        assert!(follow.contains("## Steps"));
        assert!(follow.contains("dd"));
        assert_eq!(
            apply_skill_follow("flash the pi", Some(&follow)),
            format!("{follow}\n\nflash the pi")
        );
        assert_eq!(
            apply_skill_follow("flash the pi", Some("  ")),
            "flash the pi"
        );
    }

    #[test]
    fn feedback_and_mouse_runs_do_not_become_skills() {
        let mouse = vec![
            "xdotool mousemove 10 10".to_string(),
            "xdotool click 1".to_string(),
        ];
        for u in [
            "actually still no you moved the mouse yes but you moved it to the left monitor",
            "good job now use the mouse to close one tab of firefox",
            "continue",
            "control my mouse and use it to move to the other side of the screen",
        ] {
            assert!(propose_skill_from_turn(u, "Done.", &mouse).is_none(), "{u}");
        }
        let one = vec!["cargo build".to_string()];
        assert!(
            propose_skill_from_turn("fix the failing build", "error: E0425", &one).is_none(),
            "one command is not a procedure"
        );
    }

    #[test]
    fn a_real_fix_becomes_a_named_skill() {
        let cmds = vec![
            "cargo build 2>&1 | tail -20".to_string(),
            "cargo test -p grokhub-core".to_string(),
        ];
        let s = propose_skill_from_turn(
            "fix the failing build in grokhub",
            "error[E0425] fixed",
            &cmds,
        )
        .expect("skill");
        assert_eq!(s.name, "fix-the-cause-cargo");
        assert_eq!(s.slash, "/fix-the-cause-cargo");
        assert!(
            s.instructions.contains("1. `cargo build"),
            "{}",
            s.instructions
        );
        assert!(s.pitfalls.contains(AUTO_SKILL_PITFALL));
        assert!(!is_junk_skill(&s));
        assert!(
            propose_skill_from_turn("what is a lifetime", "It is…", &cmds).is_none(),
            "an explanation is not a procedure"
        );
    }

    #[test]
    fn leftover_sentence_and_template_skills_are_junk() {
        let mk = |name: &str, desc: &str, runs: u32| SkillMd {
            name: name.into(),
            description: desc.into(),
            slash: "/x".into(),
            trigger: String::new(),
            instructions: "1. x".into(),
            pitfalls: AUTO_SKILL_PITFALL.into(),
            verify: String::new(),
            runs,
        };
        assert!(is_junk_skill(&mk(
            "actually-still-no-you-moved-the-mouse-yes-but-you-moved-it-to-the-left-monitor",
            "actually still no you moved the mouse yes but",
            0
        )));
        assert!(is_junk_skill(&mk(
            "take-control-of-my-mouse-and-close-firefox",
            "take control of my mouse and close firefox",
            0
        )));
        assert!(is_junk_skill(&mk("continue", "continue", 0)));
        assert!(is_junk_skill(&mk(
            "take-the-next-step",
            "Takes the next concrete step for this kind of task.",
            0
        )));
        assert!(!is_junk_skill(&mk(
            "board-status",
            "List open workboard cards and the next concrete step.",
            0
        )));
        assert!(!is_junk_skill(&mk(
            "deploy-user-install",
            "Sync and restart the user GrokHub install",
            0
        )));
        assert!(
            !is_junk_skill(&mk("take-the-next-step", "x", 3)),
            "a skill that ran is kept"
        );
        let mut hand = mk(
            "actually-still-no-you-moved-the-mouse-yes-but-you-moved-it-to-the-left-monitor",
            "actually still no you moved the mouse yes but",
            0,
        );
        hand.pitfalls.clear();
        assert!(
            !is_junk_skill(&hand),
            "the same junk-looking name with empty pitfalls is hand-written"
        );
        hand.pitfalls = "do not wipe the boot disk".into();
        assert!(
            !is_junk_skill(&hand),
            "the same junk-looking name with hand-written pitfalls is not junk"
        );
        let mut stop = mk(
            "stop-the-staging-server-and-clear-the-cache-now",
            "stop the staging server and clear the cache now",
            0,
        );
        stop.pitfalls.clear();
        assert!(
            !is_junk_skill(&stop),
            "a hand-written skill whose name starts with stop stays"
        );
    }

    #[test]
    fn feedback_prefixes_need_a_word_boundary() {
        for u in [
            "actually still no you moved the mouse yes but you moved it to the left monitor",
            "good job now use the mouse to close one tab of firefox",
            "continue",
            "no, the other one",
            "stop",
            "you're close",
        ] {
            assert!(is_feedback_ask(u), "{u}");
        }
        for u in [
            "yesterday's build log",
            "closed issues report",
            "stopwatch for the build",
            "nicely format the log",
        ] {
            assert!(!is_feedback_ask(u), "{u}");
        }
    }
}
