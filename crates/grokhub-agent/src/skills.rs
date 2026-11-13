// Portions derived from xai-org/grok-build (Apache-2.0, © SpaceXAI), commit 2bdd1d6a; modified.

//! `SKILL.md` discovery and the `skill` tool.
//!
//! Project skills (`.grok/skills`, then `.claude/skills`, from the workspace up to the
//! git root) win over the user directory, then cabin skills under
//! `{GROKHUB_CONFIG}/skills` (the same store `skill_list` / `skill_create` use),
//! then plugins, then Loadout built-ins shipped under `assets/skills/` (the
//! vstack skills listed in `assets/skills/vstack.json`). Same-name duplicates keep the winner; the walk
//! order is sorted so the choice does not depend on directory iteration.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::SystemTime;

use serde_json::{json, Value};

use crate::tools::{confine, str_field, ToolOutput};

struct DiscoverCache {
    key: Vec<(PathBuf, Option<SystemTime>)>,
    skills: Vec<Skill>,
}

static DISCOVER_CACHE: Mutex<Option<DiscoverCache>> = Mutex::new(None);

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
    /// `{GROKHUB_CONFIG}/skills` — same store as self-manage `skill_list`.
    Cabin,
    Plugin,
    /// Shipped with the app (`assets/skills/`). Overridden by any same-name
    /// project, user, cabin, or plugin skill.
    Builtin,
}

impl SkillSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Project => "project",
            Self::User => "user",
            Self::Cabin => "cabin",
            Self::Plugin => "plugin",
            Self::Builtin => "builtin",
        }
    }
}

// Bundled vstack skills (MIT; see `assets/skills/CREDITS.md`). The table is
// generated from `assets/skills/vstack.json` by `.github/scripts/vstack_sync.py`.
pub use crate::builtin_skills::{BUILTIN_ALWAYS_ON, BUILTIN_SKILL_BODIES, BUILTIN_SKILL_FILES};

fn builtin_body(name: &str) -> Option<&'static str> {
    BUILTIN_SKILL_BODIES
        .iter()
        .find(|(n, _)| *n == name || normalize_name(n) == normalize_name(name))
        .map(|(_, body)| *body)
}

/// The other files in a built-in skill's folder: path inside it and text.
fn builtin_files(name: &str) -> impl Iterator<Item = (&'static str, &'static str)> + '_ {
    BUILTIN_SKILL_FILES
        .iter()
        .filter(move |(n, _, _)| *n == name || normalize_name(n) == normalize_name(name))
        .map(|(_, rel, text)| (*rel, *text))
}

fn builtin_skills() -> Vec<Skill> {
    let mut out = Vec::with_capacity(BUILTIN_SKILL_BODIES.len());
    for (folder, text) in BUILTIN_SKILL_BODIES {
        let Some(parsed) = parse_skill(text, Some(folder)) else {
            continue;
        };
        let dir = PathBuf::from(format!("builtin:{folder}"));
        out.push(Skill {
            name: parsed.name,
            description: parsed.description,
            allowed_tools: parsed.allowed_tools,
            source: SkillSource::Builtin,
            path: dir.join("SKILL.md"),
            dir,
        });
    }
    out
}

/// Built-in skill names for slash / skill_list without a full discover walk.
pub fn builtin_names() -> Vec<&'static str> {
    BUILTIN_SKILL_BODIES.iter().map(|(n, _)| *n).collect()
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

fn skill_root_mtime(root: &Path) -> Option<SystemTime> {
    let meta = fs::symlink_metadata(root).ok()?;
    if !meta.file_type().is_dir() {
        return None;
    }
    let mut newest = meta.modified().ok();
    walk_skill_mtimes(root, 0, &mut newest);
    newest
}

/// Newest mtime under a skills root, matching [`walk_skills`] depth so a nested
/// `SKILL.md` edit busts the discover cache.
fn walk_skill_mtimes(dir: &Path, depth: usize, newest: &mut Option<SystemTime>) {
    if depth > WALK_DEPTH || !real_dir(dir) {
        return;
    }
    let Ok(rd) = fs::read_dir(dir) else {
        return;
    };
    for ent in rd.flatten() {
        let path = ent.path();
        let name = ent.file_name();
        let name = name.to_string_lossy();
        if name.starts_with('.') {
            continue;
        }
        let Ok(meta) = fs::symlink_metadata(&path) else {
            continue;
        };
        if let Ok(mtime) = meta.modified() {
            *newest = Some(match *newest {
                Some(prev) if prev >= mtime => prev,
                _ => mtime,
            });
        }
        if name == "SKILL.md" {
            continue;
        }
        if meta.file_type().is_dir() {
            walk_skill_mtimes(&path, depth + 1, newest);
        }
    }
}

fn cabin_skills_dir() -> PathBuf {
    crate::perm::config_dir().join("skills")
}

fn discover_cache_key(
    workspace: &Path,
    home: Option<&Path>,
    plugin_dirs: &[PathBuf],
) -> Vec<(PathBuf, Option<SystemTime>)> {
    let mut keys = Vec::new();
    for dir in project_chain(workspace) {
        let grok = dir.join(".grok").join("skills");
        let claude = dir.join(".claude").join("skills");
        keys.push((grok.clone(), skill_root_mtime(&grok)));
        keys.push((claude.clone(), skill_root_mtime(&claude)));
    }
    if let Some(home) = home {
        let grok = home.join(".grok").join("skills");
        let claude = home.join(".claude").join("skills");
        keys.push((grok.clone(), skill_root_mtime(&grok)));
        keys.push((claude.clone(), skill_root_mtime(&claude)));
    }
    let cabin = cabin_skills_dir();
    keys.push((cabin.clone(), skill_root_mtime(&cabin)));
    for dir in plugin_dirs {
        keys.push((dir.clone(), skill_root_mtime(dir)));
    }
    keys
}

/// Skills switched off on the Skills tab are listed in `<skills dir>/.off`,
/// one name per line. The dot name keeps it out of the skill walk and the
/// discover cache key. Each fn takes the skills dir so the desktop app can
/// pass its own (its tests pin a different config dir than this crate's).
fn skills_off_file(skills_dir: &Path) -> PathBuf {
    skills_dir.join(".off")
}

/// Names switched off, normalized the way the skill tool matches names
/// ("Trim Clips" and "trim-clips" are one skill). A missing or unreadable
/// file means every skill is on.
pub fn skills_off_in(skills_dir: &Path) -> BTreeSet<String> {
    fs::read_to_string(skills_off_file(skills_dir))
        .unwrap_or_default()
        .lines()
        .filter(|l| !l.trim().starts_with('#'))
        .map(normalize_name)
        .filter(|l| !l.is_empty())
        .collect()
}

/// The key a skill name matches by, in the off list and the skill tool.
pub fn skill_name_key(name: &str) -> String {
    normalize_name(name)
}

/// Whether `name` is switched off in `skills_dir`.
pub fn skill_is_off_in(skills_dir: &Path, name: &str) -> bool {
    skills_off_in(skills_dir).contains(&normalize_name(name))
}

/// Switch a skill off (or back on). An off skill stays on disk and on the
/// Skills tab, but chat never loads or matches it.
pub fn set_skill_off_in(skills_dir: &Path, name: &str, off: bool) -> std::io::Result<()> {
    let key = normalize_name(name);
    if key.is_empty() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "skill name is empty",
        ));
    }
    let mut names = skills_off_in(skills_dir);
    if off {
        names.insert(key);
    } else {
        names.remove(&key);
    }
    fs::create_dir_all(skills_dir)?;
    let mut body = String::new();
    for n in &names {
        body.push_str(n);
        body.push('\n');
    }
    let path = skills_off_file(skills_dir);
    let tmp = skills_dir.join(".off.tmp");
    fs::write(&tmp, body)?;
    fs::rename(&tmp, &path)
}

pub fn discover(workspace: &Path, home: Option<&Path>, plugin_dirs: &[PathBuf]) -> Vec<Skill> {
    let mut skills = discover_all(workspace, home, plugin_dirs);
    // A skill switched off on the Skills tab never loads. Checked past the
    // cache so a switch takes hold on the next turn. Built-ins can't be off.
    let off = skills_off_in(&cabin_skills_dir());
    if !off.is_empty() {
        skills.retain(|s| {
            s.source == SkillSource::Builtin || !off.contains(&normalize_name(&s.name))
        });
    }
    skills
}

/// Every skill chat could load, switched-off ones included, for a Skills page
/// that lists an off skill so it can be switched back on.
pub fn discover_all(workspace: &Path, home: Option<&Path>, plugin_dirs: &[PathBuf]) -> Vec<Skill> {
    let mut skills = discover_cached(workspace, home, plugin_dirs);
    // Dream lessons show only once a paired eval passed them as they stand
    // (round 3 item 39). Checked past the cache so a pass shows at once.
    skills.retain(|s| {
        s.source != SkillSource::Cabin
            || !s.name.eq_ignore_ascii_case(crate::dream_gate::LESSON_SKILL)
            || crate::dream_gate::proven(&s.dir)
    });
    skills
}

fn discover_cached(workspace: &Path, home: Option<&Path>, plugin_dirs: &[PathBuf]) -> Vec<Skill> {
    let key = discover_cache_key(workspace, home, plugin_dirs);
    if let Ok(guard) = DISCOVER_CACHE.lock() {
        if let Some(cache) = guard.as_ref() {
            if cache.key == key {
                return cache.skills.clone();
            }
        }
    }
    let skills = discover_uncached(workspace, home, plugin_dirs);
    if let Ok(mut guard) = DISCOVER_CACHE.lock() {
        *guard = Some(DiscoverCache {
            key,
            skills: skills.clone(),
        });
    }
    skills
}

fn discover_uncached(workspace: &Path, home: Option<&Path>, plugin_dirs: &[PathBuf]) -> Vec<Skill> {
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
    // Same folder skill_list / skill_create use — must load via `skill` too.
    push_skill_root(&mut found, &cabin_skills_dir(), SkillSource::Cabin, 15_000);
    for (i, dir) in plugin_dirs.iter().enumerate() {
        let rank = 20_000u32.saturating_add(u32::try_from(i).unwrap_or(u32::MAX));
        push_skill_root(&mut found, dir, SkillSource::Plugin, rank);
    }
    // Built-ins last: a same-name project/user/cabin/plugin skill wins.
    for skill in builtin_skills() {
        found.push(Found {
            rank: 30_000,
            path: skill.path.display().to_string(),
            skill,
        });
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
    // Built-ins stay discoverable via the skill tool and plan_skills wiring.
    // Listing them on every empty-workspace prompt tipped the R6 prefix-cache
    // ratio below baseline; moment / close-check / playbook name them when needed.
    let listed: Vec<&Skill> = skills
        .iter()
        .filter(|s| s.source != SkillSource::Builtin)
        .collect();
    if listed.is_empty() {
        return String::new();
    }
    let mut out =
        String::from("<skills>\nLoad one with the skill tool when it matches the task.\n");
    for skill in listed.into_iter().take(40) {
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
    let want = normalize_name(&name);
    let home = grokhub_core::user_home();
    let skills = discover(
        workspace,
        home.as_deref(),
        &crate::plugins::skill_dirs(workspace),
    );
    let Some(skill) = skills.into_iter().find(|skill| {
        skill.name == want || skill.name == name || normalize_name(&skill.name) == want
    }) else {
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
    let body = if skill.source == SkillSource::Builtin {
        match builtin_body(&skill.name) {
            Some(text) => clip_chars(&skill_body(text), BODY_CAP),
            None => {
                return ToolOutput::err(format!("unknown builtin skill `{}`", skill.name));
            }
        }
    } else {
        match fs::read_to_string(&skill.path) {
            Ok(text) => clip_chars(&skill_body(&text), BODY_CAP),
            Err(err) => {
                return ToolOutput::err(format!("read {}: {err}", skill.path.display()));
            }
        }
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
    if skill.source == SkillSource::Builtin {
        out.push_str("- SKILL.md\n");
        for (file, _) in builtin_files(&skill.name) {
            out.push_str("- ");
            out.push_str(file);
            out.push('\n');
        }
        if let Some(rel) = rel {
            let rel = rel.strip_prefix("./").unwrap_or(rel);
            let text = if rel == "SKILL.md" {
                body
            } else {
                match builtin_files(&skill.name).find(|(file, _)| *file == rel) {
                    Some((_, text)) => clip_chars(text, BODY_CAP),
                    None => return ToolOutput::err("path is not available"),
                }
            };
            out.push_str("\nFile ");
            out.push_str(rel);
            out.push_str(":\n");
            out.push_str(&text);
        }
        return ToolOutput::ok(out);
    }
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
        // `confine` returns a canonical path (`\\?\C:\...` on Windows), so compare
        // against the canonical skill folder, not the path as discovered.
        let root = skill
            .dir
            .canonicalize()
            .unwrap_or_else(|_| skill.dir.clone());
        if !path.starts_with(&root) {
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

pub(crate) fn clip_chars(text: &str, max: usize) -> String {
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
    fn a_skill_switched_off_is_not_discovered_until_it_is_back_on() {
        let root = scratch("skills-off");
        let repo = root.join("repo");
        let home = root.join("home");
        fs::create_dir_all(repo.join(".git")).unwrap();
        fs::create_dir_all(&home).unwrap();
        let cfg = root.join("cfg");
        fs::create_dir_all(&cfg).unwrap();
        let _guard = crate::perm::ConfigGuard::set(&cfg);
        skill_md(
            &cfg.join("skills/trim-clips"),
            "---\nname: trim-clips\ndescription: Trim clips\n---\nTrim them.\n",
        );
        skill_md(
            &repo.join(".grok/skills/notes"),
            "---\nname: notes\ndescription: Notes\n---\nTake notes.\n",
        );
        let names =
            |skills: Vec<Skill>| -> Vec<String> { skills.into_iter().map(|s| s.name).collect() };
        assert!(names(discover(&repo, Some(&home), &[])).contains(&"trim-clips".to_string()));
        set_skill_off_in(&cfg.join("skills"), "Trim Clips", true).unwrap();
        set_skill_off_in(&cfg.join("skills"), "notes", true).unwrap();
        assert!(skill_is_off_in(&cfg.join("skills"), "TRIM clips"));
        let now = names(discover(&repo, Some(&home), &[]));
        assert!(!now.contains(&"trim-clips".to_string()), "{now:?}");
        assert!(!now.contains(&"notes".to_string()), "{now:?}");
        let all = names(discover_all(&repo, Some(&home), &[]));
        assert!(
            all.contains(&"trim-clips".to_string()) && all.contains(&"notes".to_string()),
            "the Skills page still lists an off skill so it can come back on: {all:?}"
        );
        let loaded = tool_run(&repo, &serde_json::json!({"name": "trim-clips"}));
        assert!(
            loaded.failed,
            "an off skill can't be loaded: {}",
            loaded.text
        );
        set_skill_off_in(&cfg.join("skills"), "trim-clips", false).unwrap();
        let back = names(discover(&repo, Some(&home), &[]));
        assert!(back.contains(&"trim-clips".to_string()), "{back:?}");
        assert!(!back.contains(&"notes".to_string()), "{back:?}");
        assert!(set_skill_off_in(&cfg.join("skills"), "  ", true).is_err());
    }

    #[test]
    fn cabin_skills_are_discovered_and_loadable_by_skill_tool() {
        let root = scratch("cabin-skills");
        let repo = root.join("repo");
        let home = root.join("home");
        fs::create_dir_all(repo.join(".git")).unwrap();
        fs::create_dir_all(&home).unwrap();
        let cfg = root.join("cfg");
        fs::create_dir_all(&cfg).unwrap();
        let _guard = crate::perm::ConfigGuard::set(&cfg);
        skill_md(
            &cfg.join("skills/steamlibreview"),
            "---\nname: steamlibreview\ndescription: Review the Steam library from Memory\n---\nUse sealed Installed app learnings.\n",
        );
        skill_md(
            &cfg.join("skills/GamingProfile"),
            "---\nname: GamingProfile\ndescription: Gaming profile notes\n---\nTailor answers to installed games.\n",
        );
        let skills = discover(&repo, Some(&home), &[]);
        assert!(
            skills
                .iter()
                .any(|s| s.name == "steamlibreview" && s.source == SkillSource::Cabin),
            "{skills:?}"
        );
        assert!(
            skills
                .iter()
                .any(|s| s.name == "gamingprofile" && s.source == SkillSource::Cabin),
            "PascalCase frontmatter normalizes: {skills:?}"
        );
        let loaded = tool_run(&repo, &serde_json::json!({"name": "steamlibreview"}));
        assert!(!loaded.failed, "{}", loaded.text);
        assert!(
            loaded.text.contains("Use sealed Installed app learnings."),
            "{}",
            loaded.text
        );
        let pascal = tool_run(&repo, &serde_json::json!({"name": "GamingProfile"}));
        assert!(!pascal.failed, "{}", pascal.text);
        let _ = fs::remove_dir_all(&root);
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

    fn bundled_names(doc: &Value, key: &str) -> Vec<String> {
        doc[key]
            .as_array()
            .unwrap_or_else(|| panic!("vstack.json `{key}`"))
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect()
    }

    /// The bullet list under `heading` in CREDITS.md, up to the next blank line.
    fn credit_list(credits: &str, heading: &str) -> Vec<String> {
        let (_, rest) = credits
            .split_once(&format!("{heading}\n\n"))
            .unwrap_or_else(|| panic!("CREDITS.md has no `{heading}`"));
        rest.lines()
            .take_while(|line| line.starts_with("- "))
            .map(|line| line[2..].split(':').next().unwrap().to_string())
            .collect()
    }

    #[test]
    fn every_manifest_skill_is_bundled_loads_and_is_credited() {
        let assets = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/skills");
        let doc: Value =
            serde_json::from_str(&fs::read_to_string(assets.join("vstack.json")).unwrap()).unwrap();
        let skills = bundled_names(&doc, "skills");
        let table: Vec<&str> = BUILTIN_SKILL_BODIES.iter().map(|(n, _)| *n).collect();
        assert_eq!(table, skills, "run .github/scripts/vstack_sync.py sync");
        assert_eq!(BUILTIN_ALWAYS_ON, bundled_names(&doc, "always_on").as_slice());
        assert!(skills.iter().any(|s| s == "elevate"), "{skills:?}");

        let root = scratch("manifest");
        let repo = root.join("repo");
        let home = root.join("empty-home");
        fs::create_dir_all(repo.join(".git")).unwrap();
        fs::create_dir_all(&home).unwrap();
        let found = discover(&repo, Some(&home), &[]);
        for name in &skills {
            let skill = found
                .iter()
                .find(|s| &s.name == name)
                .unwrap_or_else(|| panic!("{name} is not discovered"));
            assert_eq!(skill.source, SkillSource::Builtin, "{name}");
            assert!(!skill.description.is_empty(), "{name}");
            let loaded = tool_run(&repo, &json!({ "name": name }));
            assert!(!loaded.failed, "{name}: {}", loaded.text);
            assert!(
                loaded
                    .text
                    .starts_with(&format!("<skill name=\"{name}\" source=\"builtin\"")),
                "{name}: {}",
                loaded.text
            );
        }
        for (name, rel, text) in BUILTIN_SKILL_FILES {
            let on_disk = fs::read_to_string(assets.join(name).join(rel)).unwrap();
            assert_eq!(*text, on_disk, "{name}/{rel}");
            let listed = tool_run(&repo, &json!({ "name": name }));
            assert!(listed.text.contains(&format!("\n- {rel}\n")), "{}", listed.text);
            let read = tool_run(&repo, &json!({ "name": name, "path": rel }));
            assert!(!read.failed, "{name}/{rel}: {}", read.text);
            assert!(
                read.text.ends_with(&format!("\nFile {rel}:\n{text}")),
                "{name}/{rel}"
            );
        }
        let guide = tool_run(&repo, &json!({ "name": "elevate", "path": "GUIDE.md" }));
        assert!(!guide.failed, "{}", guide.text);
        let missing = tool_run(&repo, &json!({ "name": "elevate", "path": "NOPE.md" }));
        assert!(missing.failed);
        assert_eq!(missing.text, "path is not available");
        // Left-out skills stay out.
        for name in ["setup-vstack", "stageshow"] {
            assert!(doc["exclude"][name].is_string(), "{name}");
            assert!(tool_run(&repo, &json!({ "name": name })).failed, "{name}");
        }

        let credits = fs::read_to_string(assets.join("CREDITS.md")).unwrap();
        let tag = doc["tag"].as_str().unwrap();
        let commit = doc["commit"].as_str().unwrap();
        assert!(credits.contains(&format!("release\n[`{tag}`]")), "{credits}");
        assert!(credits.contains(&format!("(commit `{commit}`)")), "{credits}");
        assert_eq!(
            credit_list(&credits, "Skills shipped here (read-only built-ins inside Loadout / GrokHub):"),
            skills
        );
        let left: Vec<String> = doc["exclude"].as_object().unwrap().keys().cloned().collect();
        assert_eq!(credit_list(&credits, "Left out:"), left);
        assert!(assets.join("LICENSE-upstream").is_file());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn builtins_are_discovered_and_loadable_via_skill_tool() {
        let root = scratch("builtins");
        let repo = root.join("repo");
        let home = root.join("empty-home");
        fs::create_dir_all(repo.join(".git")).unwrap();
        fs::create_dir_all(&home).unwrap();
        let skills = discover(&repo, Some(&home), &[]);
        for name in [
            "qa-walkthrough",
            "av-qa-walkthrough",
            "create-verification-skill",
            "maintain-verification-skill",
            "follow-through",
        ] {
            let skill = skills
                .iter()
                .find(|s| s.name == name)
                .unwrap_or_else(|| panic!("missing builtin {name}: {skills:?}"));
            assert_eq!(skill.source, SkillSource::Builtin);
            let loaded = tool_run(&repo, &serde_json::json!({ "name": name }));
            assert!(!loaded.failed, "{name}: {}", loaded.text);
            assert!(
                loaded.text.contains("source=\"builtin\""),
                "{name}: {}",
                loaded.text
            );
            assert!(
                loaded.text.contains("<skill name="),
                "{name}: {}",
                loaded.text
            );
        }
        assert!(skills
            .iter()
            .any(|s| s.name == "qa-walkthrough" && s.source == SkillSource::Builtin));
        // Reminder skips built-ins so empty-workspace prompts stay R6-stable.
        assert!(!reminder(&skills).contains("qa-walkthrough"));
        // Project skill with the same name wins over the builtin.
        skill_md(
            &repo.join(".grok/skills/qa-walkthrough"),
            "---\nname: qa-walkthrough\ndescription: project override\n---\nProject QA.\n",
        );
        // Bust discover cache (mtime).
        std::thread::sleep(std::time::Duration::from_millis(1100));
        let again = discover(&repo, Some(&home), &[]);
        let qa = again.iter().find(|s| s.name == "qa-walkthrough").unwrap();
        assert_eq!(qa.source, SkillSource::Project);
        assert_eq!(qa.description, "project override");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn discover_cache_invalidates_when_skill_md_changes() {
        let root = scratch("cache-mtime");
        let repo = root.join("repo");
        let home = root.join("empty-home");
        fs::create_dir_all(&home).unwrap();
        let dir = repo.join(".grok/skills/demo");
        skill_md(
            &dir,
            "---\nname: demo\ndescription: first description\n---\nFirst body.\n",
        );
        let first = discover(&repo, Some(&home), &[]);
        let demo = first.iter().find(|s| s.name == "demo").unwrap();
        assert_eq!(demo.description, "first description");
        // Coarse filesystems (Windows CI) may keep the same second for mtime.
        std::thread::sleep(std::time::Duration::from_millis(1100));
        skill_md(
            &dir,
            "---\nname: demo\ndescription: second description\n---\nSecond body.\n",
        );
        let second = discover(&repo, Some(&home), &[]);
        let demo = second.iter().find(|s| s.name == "demo").unwrap();
        assert_eq!(demo.description, "second description");
        let again = discover(&repo, Some(&home), &[]);
        assert_eq!(
            again.iter().find(|s| s.name == "demo").unwrap().description,
            "second description"
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn discover_cache_invalidates_when_nested_skill_md_changes() {
        let root = scratch("cache-nested");
        let repo = root.join("repo");
        let home = root.join("empty-home");
        fs::create_dir_all(&home).unwrap();
        // Nested under pack/demo so a shallow root-only mtime would miss it.
        let dir = repo.join(".grok/skills/pack/demo");
        skill_md(
            &dir,
            "---\nname: nested-demo\ndescription: nested first\n---\nFirst nested.\n",
        );
        let first = discover(&repo, Some(&home), &[]);
        assert_eq!(
            first
                .iter()
                .find(|s| s.name == "nested-demo")
                .unwrap()
                .description,
            "nested first"
        );
        std::thread::sleep(std::time::Duration::from_millis(1100));
        skill_md(
            &dir,
            "---\nname: nested-demo\ndescription: nested second\n---\nSecond nested.\n",
        );
        let second = discover(&repo, Some(&home), &[]);
        assert_eq!(
            second
                .iter()
                .find(|s| s.name == "nested-demo")
                .unwrap()
                .description,
            "nested second"
        );
        let _ = fs::remove_dir_all(&root);
    }
}
