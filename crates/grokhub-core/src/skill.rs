use crate::is_plain_text;

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

pub fn bump_skill_run(runs: u32) -> u32 {
    runs.saturating_add(1)
}

pub fn is_hard_run(tool_calls: u32, recovered_error: bool, user_corrected: bool, scratch: bool) -> bool {
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
    let q = words(t);
    let mut best: Option<&SkillMd> = None;
    let mut best_score = 0.0f32;
    for s in skills {
        let trig = words(if s.trigger.is_empty() { &s.name } else { &s.trigger });
        let score = jaccard(&q, &trig);
        if score >= 0.5 && score > best_score {
            best = Some(s);
            best_score = score;
        }
    }
    best
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
    if let Some(hit) = existing.iter().find(|s| s.slash.to_ascii_lowercase() == slash) {
        return Some(hit.name.clone());
    }
    let prop = words(&proposed.trigger);
    let mut best: Option<&SkillMd> = None;
    let mut best_score = 0.0f32;
    for s in existing {
        let score = jaccard(&prop, &words(if s.trigger.is_empty() { &s.name } else { &s.trigger }));
        if score >= 0.5 && score > best_score {
            best = Some(s);
            best_score = score;
        }
    }
    best.map(|s| s.name.clone())
}

pub fn propose_skill_from_turn(user_text: &str, assistant_text: &str, host_commands: &[String]) -> SkillMd {
    let user = user_text.replace('\n', " ");
    let user: String = user.chars().take(120).collect();
    let name = skill_dir_name(if user.is_empty() { "saved-run" } else { &user });
    let first_word = user
        .split_whitespace()
        .find(|w| w.chars().filter(|c| c.is_ascii_alphabetic()).count() >= 3)
        .map(|w| {
            w.chars()
                .filter(|c| c.is_ascii_alphanumeric())
                .flat_map(|c| c.to_lowercase())
                .collect::<String>()
        })
        .unwrap_or_else(|| name.replace('-', "").chars().take(24).collect());
    let steps = if host_commands.is_empty() {
        let bit: String = assistant_text.chars().take(200).collect();
        format!("1. {}", if bit.is_empty() { "repeat the successful host steps" } else { &bit })
    } else {
        host_commands
            .iter()
            .enumerate()
            .map(|(i, c)| format!("{}. `{c}`", i + 1))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let verify = host_commands
        .last()
        .cloned()
        .unwrap_or_else(|| "echo ok".into());
    SkillMd {
        name: name.clone(),
        description: if user.is_empty() {
            "Saved host procedure".into()
        } else {
            user.clone()
        },
        slash: format!("/{}", if first_word.is_empty() { "skill" } else { &first_word }),
        trigger: if user.is_empty() { name } else { user },
        instructions: steps,
        pitfalls: "Do not run destructive commands without a receipt and confirm.".into(),
        verify: format!("{verify} exits 0"),
        runs: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert!(
            match_skill("Follow skill flash-pi", &skills).is_none(),
            "Follow skill <name> is below the Jaccard gate"
        );
        let proposed = propose_skill_from_turn("flash the pi", "ok", &["dd if=a".into()]);
        assert_eq!(proposed.slash, "/flash");
        assert_eq!(prefer_patch(&[flash.clone()], &proposed), Some("flash-pi".into()));
        let patched = patch_skill(&flash, &proposed);
        assert_eq!(patched.name, "flash-pi");
        assert_eq!(patched.slash, "/flash");
        assert!(patched.instructions.contains("dd if=a"));
        let follow = skill_follow_block(&flash);
        assert!(follow.contains("Active skill flash-pi"));
        assert!(follow.contains("## Steps"));
        assert!(follow.contains("dd"));
    }
}
