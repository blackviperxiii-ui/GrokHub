//! Slot-filled starter Blueprints for common script-first automations.
//! Clean-room catalog: each tile is a seed template with 2–3 fillable fields.

use serde::Serialize;

/// One fillable field on a Blueprint card.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BlueprintSlot {
    pub key: String,
    pub label: String,
    pub placeholder: String,
}

/// A starter tile on the Automations Blueprints gallery.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Blueprint {
    pub id: String,
    pub title: String,
    pub body: String,
    /// Seed with `{key}` slots; filling every slot yields words `add` accepts.
    pub seed: String,
    pub slots: Vec<BlueprintSlot>,
}

fn slot(key: &str, label: &str, placeholder: &str) -> BlueprintSlot {
    BlueprintSlot {
        key: key.into(),
        label: label.into(),
        placeholder: placeholder.into(),
    }
}

fn tile(id: &str, title: &str, body: &str, seed: &str, slots: Vec<BlueprintSlot>) -> Blueprint {
    Blueprint {
        id: id.into(),
        title: title.into(),
        body: body.into(),
        seed: seed.into(),
        slots,
    }
}

/// Built-in Blueprints. Script-first where a check command fits.
pub fn catalog() -> Vec<Blueprint> {
    vec![
        tile(
            "disk-watch",
            "Disk watch",
            "Check free space on a path; wake the model only when output changes.",
            "every day at {time} run `df -h {path}` only when it changes and summarize what filled up",
            vec![
                slot("path", "Path", "/"),
                slot("time", "Time", "09:00"),
            ],
        ),
        tile(
            "git-dirty",
            "Git dirty files",
            "Watch a repo for uncommitted changes; report only when something changed.",
            "every day at {time} run `git -C {repo} status --porcelain` only when it changes and summarize the dirty files for {focus}",
            vec![
                slot("repo", "Repo path", "/home/user/my-app"),
                slot("time", "Time", "09:00"),
                slot("focus", "Focus", "open pull requests"),
            ],
        ),
        tile(
            "host-pulse",
            "Host pulse",
            "Script-only host check on an interval — no model call.",
            "heartbeat every {mins} min run `{cmd}` without AI",
            vec![
                slot("mins", "Minutes", "60"),
                slot("cmd", "Check command", "uptime"),
            ],
        ),
        tile(
            "folder-digest",
            "Folder digest",
            "List a folder on a weekday schedule and summarize what needs attention.",
            "every weekday at {time} run `ls -lt {folder}` and summarize what needs attention for {topic}",
            vec![
                slot("folder", "Folder", "/home/user/Downloads"),
                slot("time", "Time", "09:00"),
                slot("topic", "Topic", "stale downloads"),
            ],
        ),
    ]
}

/// Replace `{key}` slots. Every slot must be filled with a non-empty value.
pub fn fill(seed: &str, slots: &[(impl AsRef<str>, impl AsRef<str>)]) -> Result<String, String> {
    let mut out = seed.to_string();
    for (key, value) in slots {
        let key = key.as_ref().trim();
        let value = value.as_ref().trim();
        if key.is_empty() {
            return Err("A Blueprint field is missing its name.".into());
        }
        if value.is_empty() {
            return Err(format!("Fill in {key} first."));
        }
        // Refuse values that would break the backticked check command.
        if value.contains('`') || value.contains('\n') || value.contains('\0') {
            return Err(format!(
                "The value for {key} cannot include backticks or new lines."
            ));
        }
        let needle = format!("{{{key}}}");
        if !out.contains(&needle) {
            return Err(format!("This Blueprint has no {{{key}}} field."));
        }
        out = out.replace(&needle, value);
    }
    if out.contains('{') && out.contains('}') {
        return Err("Fill in every field first.".into());
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::automation_check::script_words;
    use crate::parse_nl_automation;

    #[test]
    fn every_blueprint_seed_parses_and_keeps_its_slots() {
        for tile in catalog() {
            assert!(
                (2..=3).contains(&tile.slots.len()),
                "{} should have 2–3 slots",
                tile.id
            );
            let pairs: Vec<(String, String)> = tile
                .slots
                .iter()
                .map(|s| (s.key.clone(), s.placeholder.clone()))
                .collect();
            let refs: Vec<(&str, &str)> = pairs
                .iter()
                .map(|(k, v)| (k.as_str(), v.as_str()))
                .collect();
            let filled = fill(&tile.seed, &refs).expect(&tile.id);
            assert!(
                parse_nl_automation(&filled).is_some(),
                "{} seed must parse: {filled}",
                tile.id
            );
            if tile.seed.contains("run `") {
                let words = script_words(&filled).expect(&tile.id);
                assert!(!words.command.is_empty(), "{}", tile.id);
            }
        }
    }

    #[test]
    fn fill_refuses_empty_or_dangerous_values() {
        assert_eq!(
            fill(
                "every day at {time} run `df -h {path}`",
                &[("time", ""), ("path", "/")]
            ),
            Err("Fill in time first.".into())
        );
        assert_eq!(
            fill(
                "every day at {time} run `df -h {path}`",
                &[("time", "09:00"), ("path", "/tmp`rm")]
            ),
            Err("The value for path cannot include backticks or new lines.".into())
        );
        assert_eq!(
            fill("every day at {time} x", &[("time", "09:00")]),
            Ok("every day at 09:00 x".into())
        );
    }
}
