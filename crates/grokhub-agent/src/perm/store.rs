// Portions derived from xai-org/grok-build (Apache-2.0, © SpaceXAI), commit 2bdd1d6a; modified.

//! Rules and per-project remembered grants live in the GrokHub config folder.
//! `GROKHUB_CONFIG` wins, the same way the rest of the app finds that folder.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::perm::rules::{parse_rule, Action, Rule};

const RULES_FILE: &str = "permission-rules.json";
const GRANTS_FILE: &str = "permission-grants.json";

thread_local! {
    static OVERRIDE: RefCell<Option<PathBuf>> = const { RefCell::new(None) };
}

static IO: Mutex<()> = Mutex::new(());

/// Pins the config folder for this thread. Tests use it so they do not share a process env.
pub struct ConfigGuard {
    prev: Option<PathBuf>,
}

impl ConfigGuard {
    pub fn set(dir: impl Into<PathBuf>) -> Self {
        let prev = OVERRIDE.with(|slot| slot.borrow_mut().replace(dir.into()));
        Self { prev }
    }
}

impl Drop for ConfigGuard {
    fn drop(&mut self) {
        OVERRIDE.with(|slot| *slot.borrow_mut() = self.prev.take());
    }
}

pub fn config_dir() -> PathBuf {
    if let Some(dir) = OVERRIDE.with(|slot| slot.borrow().clone()) {
        return dir;
    }
    if let Ok(raw) = std::env::var("GROKHUB_CONFIG") {
        let raw = raw.trim();
        if !raw.is_empty() {
            return PathBuf::from(raw);
        }
    }
    if cfg!(windows) {
        if let Ok(app) = std::env::var("APPDATA") {
            if !app.trim().is_empty() {
                return PathBuf::from(app).join("GrokHub");
            }
        }
    }
    if let Some(home) = grokhub_core::user_home() {
        return home.join(".config").join("GrokHub");
    }
    PathBuf::from(".grokhub")
}

pub fn project_key(workspace: &Path) -> String {
    let path = workspace
        .canonicalize()
        .unwrap_or_else(|_| workspace.to_path_buf());
    path.to_string_lossy().replace('\\', "/")
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct RulesFile {
    #[serde(default)]
    allow: Vec<String>,
    #[serde(default)]
    ask: Vec<String>,
    #[serde(default)]
    deny: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct GrantsFile {
    #[serde(default)]
    projects: BTreeMap<String, Vec<String>>,
}

pub fn load_rules(dir: &Path) -> Vec<Rule> {
    let _guard = IO.lock().unwrap_or_else(|err| err.into_inner());
    load_rules_unlocked(dir)
}

fn load_rules_unlocked(dir: &Path) -> Vec<Rule> {
    let Ok(text) = fs::read_to_string(dir.join(RULES_FILE)) else {
        return Vec::new();
    };
    let Ok(file) = serde_json::from_str::<RulesFile>(&text) else {
        return Vec::new();
    };
    let mut rules = Vec::new();
    for (action, entries) in [
        (Action::Allow, file.allow),
        (Action::Deny, file.deny),
        (Action::Ask, file.ask),
    ] {
        for entry in entries {
            if let Ok(rule) = parse_rule(&entry, action) {
                rules.push(rule);
            }
        }
    }
    rules
}

pub fn save_rules(dir: &Path, rules: &[Rule]) -> Result<(), String> {
    let _guard = IO.lock().unwrap_or_else(|err| err.into_inner());
    let mut file = RulesFile::default();
    for rule in rules {
        let slot = match rule.action {
            Action::Allow => &mut file.allow,
            Action::Deny => &mut file.deny,
            Action::Ask => &mut file.ask,
        };
        if !slot.iter().any(|existing| existing == &rule.source) {
            slot.push(rule.source.clone());
        }
    }
    let body = serde_json::to_string_pretty(&file).map_err(|err| err.to_string())?;
    write_atomic(&dir.join(RULES_FILE), body.as_bytes())
}

pub fn load_grants(dir: &Path, workspace: &Path) -> Vec<String> {
    let _guard = IO.lock().unwrap_or_else(|err| err.into_inner());
    let key = project_key(workspace);
    load_grants_file(dir)
        .projects
        .get(&key)
        .cloned()
        .unwrap_or_default()
}

pub fn load_all_grants(dir: &Path) -> BTreeMap<String, Vec<String>> {
    let _guard = IO.lock().unwrap_or_else(|err| err.into_inner());
    load_grants_file(dir).projects
}

pub fn remember_grant(workspace: &Path, command: &str) -> Result<(), String> {
    let command = command.trim();
    if command.is_empty() {
        return Ok(());
    }
    let _guard = IO.lock().unwrap_or_else(|err| err.into_inner());
    let dir = config_dir();
    let key = project_key(workspace);
    let mut file = load_grants_file(&dir);
    let slot = file.projects.entry(key).or_default();
    if !slot.iter().any(|existing| existing == command) {
        slot.push(command.to_string());
    }
    write_grants(&dir, &file)
}

pub fn remove_grant(workspace: &Path, command: &str) -> Result<(), String> {
    let _guard = IO.lock().unwrap_or_else(|err| err.into_inner());
    let dir = config_dir();
    let key = project_key(workspace);
    let mut file = load_grants_file(&dir);
    if let Some(slot) = file.projects.get_mut(&key) {
        slot.retain(|existing| existing != command);
        if slot.is_empty() {
            file.projects.remove(&key);
        }
    }
    write_grants(&dir, &file)
}

pub fn add_grant_at(dir: &Path, workspace: &Path, command: &str) -> Result<(), String> {
    let command = command.trim();
    if command.is_empty() {
        return Err("grant is empty".into());
    }
    let _guard = IO.lock().unwrap_or_else(|err| err.into_inner());
    let key = project_key(workspace);
    let mut file = load_grants_file(dir);
    let slot = file.projects.entry(key).or_default();
    if !slot.iter().any(|existing| existing == command) {
        slot.push(command.to_string());
    }
    write_grants(dir, &file)
}

fn load_grants_file(dir: &Path) -> GrantsFile {
    let Ok(text) = fs::read_to_string(dir.join(GRANTS_FILE)) else {
        return GrantsFile::default();
    };
    serde_json::from_str(&text).unwrap_or_default()
}

fn write_grants(dir: &Path, file: &GrantsFile) -> Result<(), String> {
    let body = serde_json::to_string_pretty(file).map_err(|err| err.to_string())?;
    write_atomic(&dir.join(GRANTS_FILE), body.as_bytes())
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, bytes).map_err(|err| err.to_string())?;
    match fs::rename(&tmp, path) {
        Ok(()) => Ok(()),
        Err(_) => {
            let _ = fs::remove_file(path);
            fs::rename(&tmp, path).map_err(|err| err.to_string())
        }
    }
}
