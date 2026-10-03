// Portions derived from xai-org/grok-build (Apache-2.0, © SpaceXAI), commit 2bdd1d6a; modified.

//! `SKILL.md` discovery and the `skill` tool.
//!
//! Project skills (`.grok/skills`, then `.claude/skills`, from the workspace up to the
//! git root) win over the user directory. Plugin directories are an extension point
//! for later phases and lose to both. Same-name duplicates keep the winner; the
//! walk order is sorted so the choice does not depend on directory iteration.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use crate::tools::{confine, str_field, ToolOutput};

const MAX_NAME: usize = 64;
const DESC_CAP: usize = 200;
const REMINDER_CAP: usize = 4_000;
const BODY_CAP: usize = 48 * 1024;
const LIST_CAP: usize = 200;
const WALK_DEPTH: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkillSource {
    Project,
    User,
    Plugin,
}

impl SkillSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Project => "project",
            Self::User => "user",
            Self::Plugin => "plugin",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skill {
    pub name: String,
    pub description: String,
    pub allowed_tools: Vec<String>,
    pub source: SkillSource,
    pub path: PathBuf,
    pub dir: PathBuf,
}

pub fn schema() -> Value {
    json!({
        "type": "function",
        "name": "skill",
        "description": "Load a discovered skill by name. Returns its instructions and the files in its directory. An optional path must stay inside that directory.",
        "parameters": {
            "type": "object",
            "properties": {
                "name": {"type": "string", "description": "Skill name from the discovery list."},
                "path": {"type": "string", "description": "Optional file inside the skill directory."}
            },
            "required": ["name"],
            "additionalProperties": false
        }
    })
}

pub fn discover(workspace: &Path, home: Option<&Path>, plugin_dirs: &[PathBuf]) -> Vec<Skill> {
    let mut found: Vec<Found> = Vec::new();
    let mut rank = 0u32;
    for dir in project_chain(workspace) {
        push_skill_root(
            &mut found,
            &dir.join(".grok").join("skills"),
            SkillSource::Project,
            rank,
        );
        rank = rank.saturating_add(1);
        push_skill_root(
            &mut found,
            &dir.join(".claude").join("skills"),
            SkillSource::Project,
            rank,
        );
        rank = rank.saturating_add(1);
    }
    if let Some(home) = home {
        push_skill_root(
            &mut found,
            &home.join(".grok").join("skills"),
            SkillSource::User,
            10_000,
        );
        push_skill_root(
            &mut found,
            &home.join(".claude").join("skills"),
            SkillSource::User,
            10_001,
        );
    }
    for (i, dir) in plugin_dirs.iter().enumerate() {
        let rank = 20_000u32.saturating_add(u32::try_from(i).unwrap_or(u32::MAX));
        push_skill_root(&mut found, dir, SkillSource::Plugin, rank);
    }
    found.sort_by(|a, b| a.rank.cmp(&b.rank).then_with(|| a.path.cmp(&b.path)));
    let mut kept: BTreeMap<String, Skill> = BTreeMap::new();
    let mut order: Vec<String> = Vec::new();
    for item in found {
        if kept.contains_key(&item.skill.name) {
            continue;
        }
        order.push(item.skill.name.clone());
        kept.insert(item.skill.name.clone(), item.skill);
    }
    let mut skills: Vec<Skill> = order
        .into_iter()
        .filter_map(|name| kept.remove(&name))
        .collect();
    skills.sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.path.cmp(&b.path)));
    skills
}

pub fn reminder(skills: &[Skill]) -> String {
    if skills.is_empty() {
        return String::new();
    }
    let mut out =
        String::from("<skills>\nLoad one with the skill tool when it matches the task.\n");
    for skill in skills.iter().take(40) {
        let desc = clip_chars(&skill.description, 160);
        let line = format!("- {} ({}): {}\n", skill.name, skill.source.as_str(), desc);
        if out.len().saturating_add(line.len()) + 10 > REMINDER_CAP {
            out.push_str("- …\n");
            break;
        }
        out.push_str(&line);
    }
    out.push_str("</skills>");
    out
}

pub fn tool_run(workspace: &Path, args: &Value) -> ToolOutput {
    let mut name = str_field(args, "name");
    if name.is_empty() {
        name = str_field(args, "skill");
    }
    if name.is_empty() {
        return ToolOutput::err("name is required");
    }
    let home = grokhub_core::user_home();
    let skills = discover(workspace, home.as_deref(), &[]);
    let Some(skill) = skills.into_iter().find(|skill| skill.name == name) else {
        return ToolOutput::err(format!("unknown skill `{name}`"));
    };
    let rel = {
        let raw = str_field(args, "path");
        if raw.is_empty() {
            None
        } else {
            Some(raw)
        }
    };
    render(&skill, rel.as_deref())
}

fn render(skill: &Skill, rel: Option<&str>) -> ToolOutput {
    let body = match fs::read_to_string(&skill.path) {
        Ok(text) => clip_chars(&skill_body(&text), BODY_CAP),
        Err(err) => return ToolOutput::err(format!("read {}: {err}", skill.path.display())),
    };
    let mut out = format!(
        "<skill name=\"{}\" source=\"{}\" path=\"{}\">\n{}\n</skill>\n",
        skill.name,
        skill.source.as_str(),
        skill.path.display(),
        body
    );
    if !skill.allowed_tools.is_empty() {
        out.push_str("allowed-tools:");
        for tool in &skill.allowed_tools {
            out.push(' ');
            out.push_str(tool);
        }
        out.push('\n');
    }
    out.push_str("Files:\n");
    for file in list_files(&skill.dir) {
        out.push_str("- ");
        out.push_str(&file);
        out.push('\n');
    }
    if let Some(rel) = rel {
        if credential_name(
            Path::new(rel)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(rel),
        ) {
            return ToolOutput::err("path is not available");
        }
        let path = match confine(&skill.dir, rel) {
            Ok(path) => path,
            Err(err) => return ToolOutput::err(err.replace("workspace", "skill directory")),
        };
        if !path.starts_with(&skill.dir) {
            return ToolOutput::err("path escapes the skill directory");
        }
        match fs::read_to_string(&path) {
            Ok(text) => {
                out.push_str("\nFile ");
                out.push_str(rel);
                out.push_str(":\n");
                out.push_str(&clip_chars(&text, BODY_CAP));
            }
            Err(err) => return ToolOutput::err(format!("read {}: {err}", path.display())),
        }
    }
    ToolOutput::ok(out)
}

struct Found {
    rank: u32,
    path: String,
    skill: Skill,
}

fn push_skill_root(out: &mut Vec<Found>, root: &Path, source: SkillSource, rank: u32) {
    if !real_dir(root) {
        return;
    }
    let mut paths = Vec::new();
    walk_skills(root, &mut paths, 0);
    paths.sort();
    for path in paths {
        let Ok(text) = fs::read_to_string(&path) else {
            continue;
        };
        let Some(parsed) = parse_skill(
            &text,
            path.parent()
                .and_then(|dir| dir.file_name())
                .and_then(|n| n.to_str()),
        ) else {
            continue;
        };
        let dir = path.parent().unwrap_or(root).to_path_buf();
        out.push(Found {
            rank,
            path: path.display().to_string(),
            skill: Skill {
                name: parsed.name,
                description: parsed.description,
                allowed_tools: parsed.allowed_tools,
                source,
                path,
                dir,
            },
        });
    }
}

fn walk_skills(dir: &Path, out: &mut Vec<PathBuf>, depth: usize) {
    if depth > WALK_DEPTH || !real_dir(dir) {
        return;
    }
    let Ok(rd) = fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = rd.filter_map(|ent| ent.ok()).collect();
    entries.sort_by_key(|ent| ent.file_name());
    for ent in entries {
        let path = ent.path();
        let name = ent.file_name();
        let name = name.to_string_lossy();
        if name.starts_with('.') {
            continue;
        }
        if name == "SKILL.md" && regular_file(&path) {
            out.push(path);
            continue;
        }
        if real_dir(&path) {
            walk_skills(&path, out, depth + 1);
        }
    }
}

struct Parsed {
    name: String,
    description: String,
    allowed_tools: Vec<String>,
}

fn parse_skill(text: &str, fallback: Option<&str>) -> Option<Parsed> {
    let (yaml, body) = split_frontmatter(text)?;
    let fields = parse_fields(yaml);
    let from_file = fields.get("name").map(String::as_str);
    let name = [from_file, fallback]
        .into_iter()
        .flatten()
        .map(normalize_name)
        .find(|name| valid_name(name))?;
    let description = fields
        .get("description")
        .map(|text| clip_chars(text.trim(), DESC_CAP))
        .filter(|text| !text.is_empty())
        .unwrap_or_else(|| first_line(body));
    let allowed = fields
        .get("allowed-tools")
        .or_else(|| fields.get("allowed_tools"))
        .map(|raw| split_tools(raw))
        .unwrap_or_default();
    Some(Parsed {
        name,
        description,
        allowed_tools: allowed,
    })
}

fn split_frontmatter(text: &str) -> Option<(&str, &str)> {
    let text = text.trim_start_matches('\u{feff}').trim_start();
    let rest = text.strip_prefix("---")?;
    let rest = rest
        .strip_prefix("\r\n")
        .or_else(|| rest.strip_prefix('\n'))?;
    let (yaml, after) = rest.split_once("\n---")?;
    let body = after
        .strip_prefix("\r\n")
        .or_else(|| after.strip_prefix('\n'))
        .unwrap_or(after);
    Some((yaml.trim_end_matches('\r'), body))
}

fn skill_body(text: &str) -> String {
    split_frontmatter(text)
        .map(|(_, body)| body.trim())
        .unwrap_or("")
        .to_string()
}

fn parse_fields(yaml: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let mut list_key: Option<String> = None;
    let mut list: Vec<String> = Vec::new();
    let flush =
        |key: &mut Option<String>, list: &mut Vec<String>, out: &mut BTreeMap<String, String>| {
            if let Some(key) = key.take() {
                out.insert(key, list.join(", "));
                list.clear();
            }
        };
    for line in yaml.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if let Some(item) = trimmed.strip_prefix("- ") {
            if list_key.is_some() {
                list.push(unquote(item.trim()).to_string());
            }
            continue;
        }
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        if key.starts_with(' ') || key.starts_with('\t') {
            continue;
        }
        flush(&mut list_key, &mut list, &mut out);
        let key = key.trim();
        if key.is_empty() {
            continue;
        }
        let value = value.trim();
        if value.is_empty() {
            list_key = Some(key.to_string());
            continue;
        }
        out.insert(key.to_string(), unquote(value).to_string());
    }
    flush(&mut list_key, &mut list, &mut out);
    out
}

fn unquote(value: &str) -> &str {
    let value = value.trim();
    if value.len() >= 2 {
        let bytes = value.as_bytes();
        if (bytes[0] == b'"' && bytes[value.len() - 1] == b'"')
            || (bytes[0] == b'\'' && bytes[value.len() - 1] == b'\'')
        {
            return &value[1..value.len() - 1];
        }
    }
    value
}

fn split_tools(raw: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut depth = 0i32;
    for ch in raw.chars() {
        if ch == '(' {
            depth += 1;
            current.push(ch);
        } else if ch == ')' {
            depth -= 1;
            current.push(ch);
        } else if depth <= 0 && (ch == ',' || ch.is_whitespace()) {
            let item = current.trim();
            if !item.is_empty() {
                parts.push(item.to_string());
            }
            current.clear();
        } else {
            current.push(ch);
        }
    }
    let item = current.trim();
    if !item.is_empty() {
        parts.push(item.to_string());
    }
    parts
}

fn normalize_name(name: &str) -> String {
    let mut out = String::new();
    for ch in name.trim().chars() {
        let ch = ch.to_ascii_lowercase();
        let ch = if ch.is_ascii_lowercase() || ch.is_ascii_digit() {
            ch
        } else {
            '-'
        };
        if ch == '-' && out.ends_with('-') {
            continue;
        }
        out.push(ch);
    }
    out.trim_matches('-').chars().take(MAX_NAME).collect()
}

fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_NAME
        && !name.starts_with('-')
        && !name.ends_with('-')
        && name
            .chars()
            .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-')
}

fn first_line(body: &str) -> String {
    body.lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|line| clip_chars(line, DESC_CAP))
        .unwrap_or_default()
}

fn list_files(dir: &Path) -> Vec<String> {
    let mut out = Vec::new();
    list_files_at(dir, dir, &mut out, 0);
    out.sort();
    out.truncate(LIST_CAP);
    out
}

fn list_files_at(root: &Path, dir: &Path, out: &mut Vec<String>, depth: usize) {
    if depth > WALK_DEPTH || out.len() >= LIST_CAP || !real_dir(dir) {
        return;
    }
    let Ok(rd) = fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = rd.filter_map(|ent| ent.ok()).collect();
    entries.sort_by_key(|ent| ent.file_name());
    for ent in entries {
        if out.len() >= LIST_CAP {
            return;
        }
        let path = ent.path();
        let name = ent.file_name();
        let name = name.to_string_lossy();
        if name.starts_with('.') {
            continue;
        }
        if real_dir(&path) {
            list_files_at(root, &path, out, depth + 1);
            continue;
        }
        if !regular_file(&path) {
            continue;
        }
        if let Ok(rel) = path.strip_prefix(root) {
            out.push(rel.display().to_string());
        }
    }
}

pub(crate) fn project_chain(workspace: &Path) -> Vec<PathBuf> {
    let workspace = workspace.to_path_buf();
    let root = git_root(&workspace).unwrap_or_else(|| workspace.clone());
    let mut dirs = Vec::new();
    if let Ok(rel) = workspace.strip_prefix(&root) {
        let mut acc = root;
        dirs.push(acc.clone());
        for comp in rel.components() {
            acc.push(comp);
            dirs.push(acc.clone());
        }
    } else {
        dirs.push(workspace);
    }
    dirs
}

fn git_root(start: &Path) -> Option<PathBuf> {
    let mut dir = if start.is_dir() {
        start.to_path_buf()
    } else {
        start.parent()?.to_path_buf()
    };
    loop {
        if dir.join(".git").exists() {
            return Some(dir);
        }
        if !dir.pop() {
            return None;
        }
    }
}

fn real_dir(path: &Path) -> bool {
    fs::symlink_metadata(path)
        .map(|meta| meta.file_type().is_dir())
        .unwrap_or(false)
}

fn regular_file(path: &Path) -> bool {
    fs::symlink_metadata(path)
        .map(|meta| meta.file_type().is_file())
        .unwrap_or(false)
}

fn credential_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.contains("secret")
        || lower.contains("token")
        || lower.contains("credential")
        || lower.ends_with(".pem")
        || lower.ends_with(".key")
        || lower.ends_with(".toml")
        || (lower.contains("auth") && lower.ends_with(".json"))
}

fn clip_chars(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    text.chars().take(max).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "gh-skill-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn skill_md(dir: &Path, body: &str) {
        fs::create_dir_all(dir).unwrap();
        fs::write(dir.join("SKILL.md"), body).unwrap();
    }

    #[test]
    fn project_skill_wins_over_user_and_names_dedupe() {
        let root = scratch("dedupe");
        let home = root.join("home");
        let repo = root.join("repo");
        fs::create_dir_all(repo.join(".git")).unwrap();
        skill_md(
            &repo.join(".grok/skills/alpha"),
            "---\nname: alpha\ndescription: from project grok\n---\nProject body.\n",
        );
        skill_md(
            &repo.join(".claude/skills/alpha"),
            "---\nname: alpha\ndescription: from project claude\n---\nClaude body.\n",
        );
        skill_md(
            &repo.join(".grok/skills/zeta"),
            "---\nname: zeta\ndescription: only project\n---\nZeta.\n",
        );
        skill_md(
            &home.join(".grok/skills/alpha"),
            "---\nname: alpha\ndescription: from user\n---\nUser body.\n",
        );
        skill_md(
            &home.join(".claude/skills/beta"),
            "---\nname: beta\ndescription: user only\n---\nBeta.\n",
        );
        let plugin = root.join("plugin");
        skill_md(
            &plugin.join("alpha"),
            "---\nname: alpha\ndescription: from plugin\n---\nPlugin.\n",
        );
        skill_md(
            &plugin.join("gamma"),
            "---\nname: gamma\ndescription: plugin only\n---\nGamma.\n",
        );
        let skills = discover(&repo, Some(&home), &[plugin]);
        let alpha = skills.iter().find(|skill| skill.name == "alpha").unwrap();
        assert_eq!(alpha.description, "from project grok");
        assert_eq!(alpha.source, SkillSource::Project);
        assert!(skills
            .iter()
            .any(|skill| skill.name == "beta" && skill.source == SkillSource::User));
        assert!(skills
            .iter()
            .any(|skill| skill.name == "gamma" && skill.source == SkillSource::Plugin));
        assert!(skills.iter().any(|skill| skill.name == "zeta"));
        let names: Vec<_> = skills.iter().map(|skill| skill.name.as_str()).collect();
        let mut sorted = names.clone();
        sorted.sort();
        assert_eq!(names, sorted, "listing order is deterministic");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn frontmatter_parses_name_description_and_allowed_tools() {
        let text = "---\nname: My Skill\ndescription: \"Deploy: prod\"\nallowed-tools: Read, Bash(git diff:*)\n---\nDo the deploy.\n";
        let parsed = parse_skill(text, Some("ignored")).unwrap();
        assert_eq!(parsed.name, "my-skill");
        assert_eq!(parsed.description, "Deploy: prod");
        assert_eq!(
            parsed.allowed_tools,
            vec!["Read".to_string(), "Bash(git diff:*)".to_string()]
        );
        let listed = "---\nname: list-skill\ndescription: listed\nallowed-tools:\n  - read_file\n  - grep\n---\nBody.\n";
        let parsed = parse_skill(listed, None).unwrap();
        assert_eq!(
            parsed.allowed_tools,
            vec!["read_file".to_string(), "grep".to_string()]
        );
        let fallback =
            parse_skill("---\ndescription: no name\n---\nHello.\n", Some("Dir Name")).unwrap();
        assert_eq!(fallback.name, "dir-name");
        assert_eq!(fallback.description, "no name");
    }

    #[test]
    fn skill_tool_keeps_paths_inside_the_skill_dir() {
        let root = scratch("confine");
        let repo = root.join("repo");
        let dir = repo.join(".grok/skills/demo");
        skill_md(
            &dir,
            "---\nname: demo\ndescription: demo skill\n---\nFollow these steps.\n",
        );
        fs::write(dir.join("notes.txt"), "inside note").unwrap();
        let outside = root.join("secret.txt");
        fs::write(&outside, "secret-marker").unwrap();
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&outside, dir.join("leak")).unwrap();
        }
        let home = root.join("empty-home");
        fs::create_dir_all(&home).unwrap();
        let skills = discover(&repo, Some(&home), &[]);
        let skill = skills
            .into_iter()
            .find(|skill| skill.name == "demo")
            .unwrap();
        let listed = render(&skill, None);
        assert!(!listed.failed, "{}", listed.text);
        assert!(listed.text.contains("Follow these steps."));
        assert!(listed.text.contains("notes.txt"));
        assert!(!listed.text.contains("secret-marker"));
        let escaped = render(&skill, Some("../secret.txt"));
        assert!(escaped.failed, "{}", escaped.text);
        assert!(!escaped.text.contains("secret-marker"));
        let nested = render(&skill, Some("notes.txt"));
        assert!(!nested.failed, "{}", nested.text);
        assert!(nested.text.contains("inside note"));
        #[cfg(unix)]
        {
            let leak = render(&skill, Some("leak"));
            assert!(leak.failed, "{}", leak.text);
            assert!(!leak.text.contains("secret-marker"));
        }
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn reminder_lists_names_and_caps() {
        let mut skills = Vec::new();
        for i in 0..50 {
            skills.push(Skill {
                name: format!("skill-{i:02}"),
                description: "d".repeat(400),
                allowed_tools: Vec::new(),
                source: SkillSource::User,
                path: PathBuf::from("SKILL.md"),
                dir: PathBuf::from("."),
            });
        }
        let text = reminder(&skills);
        assert!(text.contains("skill-00"));
        assert!(text.contains("(user)"));
        assert!(text.len() <= REMINDER_CAP + 20, "{}", text.len());
        assert!(!text.contains(&"d".repeat(400)));
    }
}
