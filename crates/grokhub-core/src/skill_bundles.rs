//! Skill bundles: small YAML files that group several skills under one slash
//! command (Hermes parity). Built-in `backend-dev` ships with the app; user
//! bundles live in `{config}/skill-bundles/<slug>.yaml`.

use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::skill::skill_dir_name;

/// One slash command that loads several skills together.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillBundle {
    pub name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    pub skills: Vec<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub instruction: String,
}

impl SkillBundle {
    pub fn slug(&self) -> String {
        skill_dir_name(&self.name)
    }
}

/// Built-in bundles always available (no YAML file required).
pub fn builtin_bundles() -> Vec<SkillBundle> {
    vec![SkillBundle {
        name: "backend-dev".into(),
        description: "Backend feature work — tests, land the change, follow through."
            .into(),
        skills: vec![
            "tdd".into(),
            "land-it".into(),
            "follow-through".into(),
        ],
        instruction: "Always start by writing failing tests, then implement. Land the change with a clean PR, and keep a follow-through ledger until done.".into(),
    }]
}

/// `{config}/skill-bundles`.
pub fn skill_bundles_dir(config_dir: &Path) -> PathBuf {
    config_dir.join("skill-bundles")
}

/// Built-ins plus any YAML bundles under the config dir (user YAML wins on slug).
pub fn list_bundles(config_dir: &Path) -> Vec<SkillBundle> {
    let mut out = builtin_bundles();
    let dir = skill_bundles_dir(config_dir);
    let Ok(rd) = fs::read_dir(&dir) else {
        return out;
    };
    for ent in rd.flatten() {
        let path = ent.path();
        let Some(ext) = path.extension() else {
            continue;
        };
        if ext != "yaml" && ext != "yml" {
            continue;
        }
        let Ok(text) = fs::read_to_string(&path) else {
            continue;
        };
        let stem = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let Ok(mut bundle) = parse_bundle_yaml(&text) else {
            continue;
        };
        if bundle.name.trim().is_empty() {
            bundle.name = stem;
        }
        let slug = bundle.slug();
        out.retain(|b| b.slug() != slug);
        out.push(bundle);
    }
    out.sort_by_key(|a| a.slug());
    out
}

pub fn bundle_by_slug(config_dir: &Path, slug: &str) -> Option<SkillBundle> {
    let want = skill_dir_name(slug.trim().trim_start_matches('/'));
    list_bundles(config_dir)
        .into_iter()
        .find(|b| b.slug() == want)
}

/// Prompt that loads every skill in the bundle at once (for `/backend-dev …`).
pub fn bundle_load_prompt(bundle: &SkillBundle, user_args: &str) -> String {
    let mut lines = Vec::new();
    lines.push(format!(
        "Active skill bundle {} — load these skills with the skill tool now, in order:",
        bundle.slug()
    ));
    for (i, name) in bundle.skills.iter().enumerate() {
        lines.push(format!("{}. {name}", i + 1));
    }
    if !bundle.instruction.trim().is_empty() {
        lines.push(String::new());
        lines.push(bundle.instruction.trim().to_string());
    }
    let args = user_args.trim();
    if !args.is_empty() {
        lines.push(String::new());
        lines.push(format!("User request: {args}"));
    }
    lines.join("\n")
}

/// Minimal YAML subset for bundle files (name, description, skills list,
/// optional instruction block).
pub fn parse_bundle_yaml(text: &str) -> Result<SkillBundle, String> {
    let mut name = String::new();
    let mut description = String::new();
    let mut instruction = String::new();
    let mut skills = Vec::new();
    let mut mode = Field::None;
    let mut instr_indent: Option<usize> = None;

    for raw in text.lines() {
        let line = raw.trim_end();
        if mode == Field::Instruction {
            let indent = line.chars().take_while(|c| *c == ' ').count();
            if line.trim().is_empty() {
                instruction.push('\n');
                continue;
            }
            let floor = instr_indent.unwrap_or(indent);
            if indent >= floor && (line.starts_with(' ') || instr_indent.is_none()) {
                instr_indent = Some(floor);
                let body = if line.len() >= floor {
                    &line[floor..]
                } else {
                    line.trim()
                };
                if !instruction.is_empty() {
                    instruction.push('\n');
                }
                instruction.push_str(body);
                continue;
            }
            mode = Field::None;
            instr_indent = None;
        }

        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("- ") {
            if mode == Field::Skills {
                let skill = rest.trim().trim_matches('"').trim_matches('\'');
                if !skill.is_empty() {
                    skills.push(skill.to_string());
                }
            }
            continue;
        }
        if let Some((key, val)) = trimmed.split_once(':') {
            let key = key.trim();
            let val = val.trim();
            match key {
                "name" => {
                    mode = Field::None;
                    name = unquote(val);
                }
                "description" => {
                    mode = Field::None;
                    description = unquote(val);
                }
                "skills" => {
                    mode = Field::Skills;
                    if !val.is_empty() && val != "|" && val != ">" {
                        for part in val.split([',', ' ']) {
                            let s = part.trim().trim_matches('"').trim_matches('\'');
                            if !s.is_empty() {
                                skills.push(s.to_string());
                            }
                        }
                    }
                }
                "instruction" => {
                    if val == "|" || val == ">" || val.is_empty() {
                        mode = Field::Instruction;
                        instr_indent = None;
                    } else {
                        mode = Field::None;
                        instruction = unquote(val);
                    }
                }
                _ => mode = Field::None,
            }
        }
    }

    if skills.is_empty() {
        return Err("bundle YAML needs a non-empty skills list".into());
    }
    if name.trim().is_empty() {
        return Err("bundle YAML needs a name".into());
    }
    Ok(SkillBundle {
        name: name.trim().to_string(),
        description: description.trim().to_string(),
        skills,
        instruction: instruction.trim().to_string(),
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Field {
    None,
    Skills,
    Instruction,
}

fn unquote(s: &str) -> String {
    let s = s.trim();
    if (s.starts_with('"') && s.ends_with('"')) || (s.starts_with('\'') && s.ends_with('\'')) {
        s[1..s.len() - 1].to_string()
    } else {
        s.to_string()
    }
}

/// Write a bundle YAML into `{config}/skill-bundles/<slug>.yaml`.
pub fn write_bundle_yaml(config_dir: &Path, bundle: &SkillBundle) -> Result<PathBuf, String> {
    let dir = skill_bundles_dir(config_dir);
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join(format!("{}.yaml", bundle.slug()));
    let mut body = format!(
        "name: {}\ndescription: {}\nskills:\n",
        bundle.name, bundle.description
    );
    for skill in &bundle.skills {
        body.push_str("  - ");
        body.push_str(skill);
        body.push('\n');
    }
    if !bundle.instruction.trim().is_empty() {
        body.push_str("instruction: |\n");
        for line in bundle.instruction.lines() {
            body.push_str("  ");
            body.push_str(line);
            body.push('\n');
        }
    }
    fs::write(&path, body).map_err(|e| e.to_string())?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backend_dev_builtin_has_three_skills() {
        let b = builtin_bundles()
            .into_iter()
            .find(|b| b.slug() == "backend-dev")
            .unwrap();
        assert_eq!(b.skills, vec!["tdd", "land-it", "follow-through"]);
        let prompt = bundle_load_prompt(&b, "refactor the auth middleware");
        assert!(prompt.contains("1. tdd"), "{prompt}");
        assert!(prompt.contains("2. land-it"), "{prompt}");
        assert!(prompt.contains("3. follow-through"), "{prompt}");
        assert!(
            prompt.contains("User request: refactor the auth middleware"),
            "{prompt}"
        );
    }

    #[test]
    fn parses_bundle_yaml_with_instruction_block() {
        let text = r#"
name: backend-dev
description: Backend feature work
skills:
  - tdd
  - land-it
  - follow-through
instruction: |
  Always start with failing tests.
  Open the PR cleanly.
"#;
        let b = parse_bundle_yaml(text).unwrap();
        assert_eq!(b.slug(), "backend-dev");
        assert_eq!(b.skills.len(), 3);
        assert!(b.instruction.contains("failing tests"));
        assert!(b.instruction.contains("Open the PR cleanly"));
    }

    #[test]
    fn user_yaml_overrides_builtin_slug() {
        let root = std::env::temp_dir().join(format!("gh-bundles-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let dir = skill_bundles_dir(&root);
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("backend-dev.yaml"),
            "name: backend-dev\ndescription: Custom\nskills:\n  - tdd\n  - land-it\n  - blast-radius\n",
        )
        .unwrap();
        let b = bundle_by_slug(&root, "backend-dev").unwrap();
        assert_eq!(b.skills, vec!["tdd", "land-it", "blast-radius"]);
        let _ = fs::remove_dir_all(&root);
    }
}
