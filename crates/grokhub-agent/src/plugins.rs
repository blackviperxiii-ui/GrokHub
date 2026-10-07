// Portions derived from xai-org/grok-build (Apache-2.0, © SpaceXAI), commit 2bdd1d6a; modified.

//! Plugin bundles for the native engine.
//!
//! A bundle contributes skills, hooks, MCP servers, agents, or commands only when
//! it is both enabled and trusted. Trust is the plugin name, its version, and a
//! content hash. Install copies or clones files; it never runs bundle commands.
//! Existing Claude and Grok plugin directories are listed and never auto-enabled.

use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime};

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};

const MAX_NAME: usize = 64;
const MAX_READ: u64 = 1_048_576;
const MAX_WALK: usize = 20_000;
const MAX_INDEX: usize = 1_000_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginInfo {
    pub name: String,
    pub version: String,
    pub description: String,
    pub enabled: bool,
    pub trusted: bool,
    pub installed: bool,
    pub summary: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarketEntry {
    pub name: String,
    pub version: String,
    pub description: String,
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct StateFile {
    #[serde(default)]
    entries: BTreeMap<String, Entry>,
    #[serde(default)]
    marketplace_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct Entry {
    #[serde(default)]
    enabled: bool,
    #[serde(default)]
    version: String,
    #[serde(default)]
    hash: String,
}

struct Manifest {
    name: String,
    version: String,
    description: String,
    value: Value,
}

#[derive(Clone)]
struct Doc {
    name: String,
    description: String,
    body: String,
}

struct Bundle {
    name: String,
    version: String,
    description: String,
    root: PathBuf,
    installed: bool,
    hash: Option<Result<String, String>>,
    skill_dirs: Vec<PathBuf>,
    hook_paths: Vec<PathBuf>,
    inline_hooks: Option<(PathBuf, String)>,
    hook_commands: Vec<String>,
    mcp_docs: Vec<String>,
    mcp_lines: Vec<String>,
    agents: Vec<Doc>,
    commands: Vec<Doc>,
    warnings: Vec<String>,
}

struct HashMemo {
    root: PathBuf,
    fingerprint: String,
    hash: String,
    at: Instant,
}

fn state_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|err| err.into_inner())
}

fn hash_memo() -> &'static Mutex<Vec<HashMemo>> {
    static MEMO: OnceLock<Mutex<Vec<HashMemo>>> = OnceLock::new();
    MEMO.get_or_init(|| Mutex::new(Vec::new()))
}

fn install_root() -> PathBuf {
    crate::perm::config_dir().join("plugins")
}

fn state_path() -> PathBuf {
    crate::perm::config_dir().join("plugin-state.json")
}

pub fn list_plugins(workspace: &Path) -> Vec<PluginInfo> {
    let _guard = state_lock();
    let (state, found) = gather(workspace, true);
    found.iter().map(|bundle| info_of(bundle, &state)).collect()
}

pub fn skill_dirs(workspace: &Path) -> Vec<PathBuf> {
    let _guard = state_lock();
    if !any_enabled() {
        return Vec::new();
    }
    let (state, found) = gather(workspace, false);
    let mut out = Vec::new();
    for bundle in found {
        if is_active(&state, &bundle) {
            out.extend(bundle.skill_dirs);
        }
    }
    out
}

pub fn hook_paths(workspace: &Path) -> Vec<PathBuf> {
    let _guard = state_lock();
    if !any_enabled() {
        return Vec::new();
    }
    let (state, found) = gather(workspace, false);
    let mut out = Vec::new();
    for bundle in found {
        if is_active(&state, &bundle) {
            out.extend(bundle.hook_paths);
        }
    }
    out
}

pub fn inline_hook_documents(workspace: &Path) -> Vec<(PathBuf, String)> {
    let _guard = state_lock();
    if !any_enabled() {
        return Vec::new();
    }
    let (state, found) = gather(workspace, false);
    let mut out = Vec::new();
    for bundle in found {
        if is_active(&state, &bundle) {
            if let Some((path, text)) = bundle.inline_hooks {
                out.push((path, text));
            }
        }
    }
    out
}

pub fn mcp_documents(workspace: &Path) -> Vec<String> {
    let _guard = state_lock();
    if !any_enabled() {
        return Vec::new();
    }
    let (state, found) = gather(workspace, false);
    let mut out = Vec::new();
    for bundle in found {
        if is_active(&state, &bundle) {
            out.extend(bundle.mcp_docs);
        }
    }
    out
}

pub fn persona_body(workspace: &Path, name: &str) -> Option<String> {
    let name = name.trim();
    if !valid_doc_name(name) {
        return None;
    }
    let _guard = state_lock();
    if !any_enabled() {
        return None;
    }
    let (state, found) = gather(workspace, false);
    for bundle in found {
        if !is_active(&state, &bundle) {
            continue;
        }
        if let Some(agent) = bundle.agents.iter().find(|agent| agent.name == name) {
            return Some(clip(&agent.body, 8_000));
        }
    }
    None
}

pub fn prompt_extras(workspace: &Path) -> String {
    let _guard = state_lock();
    if !any_enabled() {
        return String::new();
    }
    let (state, found) = gather(workspace, false);
    let mut out = String::new();
    for bundle in found {
        if !is_active(&state, &bundle) {
            continue;
        }
        push_docs(&mut out, "plugin-agents", &bundle.agents);
        push_docs(&mut out, "plugin-commands", &bundle.commands);
    }
    out
}

pub fn listing_stamp(workspace: &Path) -> String {
    let _guard = state_lock();
    if !any_enabled() {
        return String::new();
    }
    let (state, found) = gather(workspace, false);
    let mut parts = Vec::new();
    for bundle in found {
        if is_active(&state, &bundle) {
            let hash = bundle
                .hash
                .as_ref()
                .and_then(|hash| hash.as_ref().ok())
                .map(String::as_str)
                .unwrap_or("");
            parts.push(format!("{}@{hash}", bundle.name));
        }
    }
    parts.join("\n")
}

pub fn install_path(src: &Path) -> Result<PluginInfo, String> {
    if src.as_os_str().is_empty() {
        return Err("path is empty".into());
    }
    let meta = fs::symlink_metadata(src).map_err(|err| err.to_string())?;
    if !meta.is_dir() {
        return Err("path is not a directory".into());
    }
    let staging = staging_path()?;
    if let Err(err) = copy_tree(src, &staging) {
        let _ = remove_path(&staging);
        return Err(err);
    }
    commit_staging(&staging, false)
}

pub fn install_git(url: &str) -> Result<PluginInfo, String> {
    let url = validate_git_url(url)?;
    let staging = staging_path()?;
    if let Err(err) = git_clone(&url, &staging) {
        let _ = remove_path(&staging);
        return Err(err);
    }
    commit_staging(&staging, true)
}

pub fn install_source(source: &str) -> Result<PluginInfo, String> {
    let source = source.trim();
    if validate_git_url(source).is_ok() {
        return install_git(source);
    }
    let path = Path::new(source);
    if path.is_absolute() {
        return install_path(path);
    }
    Err("marketplace entry is not an https, ssh, or file URL, or an absolute path".into())
}

pub fn trust_plugin(workspace: &Path, name: &str) -> Result<(), String> {
    let name = require_name(name)?;
    let _guard = state_lock();
    let (mut state, found) = gather(workspace, true);
    let Some(bundle) = found.into_iter().find(|bundle| bundle.name == name) else {
        return Err(format!("unknown plugin {name}"));
    };
    let hash = match bundle.hash {
        Some(Ok(hash)) => hash,
        Some(Err(err)) => return Err(err),
        None => return Err(format!("{name} could not be hashed")),
    };
    let entry = state.entries.entry(name).or_default();
    entry.hash = hash;
    entry.version = bundle.version;
    save_state(&state)?;
    crate::mcp::invalidate();
    Ok(())
}

pub fn enable_plugin(workspace: &Path, name: &str) -> Result<(), String> {
    let name = require_name(name)?;
    let _guard = state_lock();
    let (mut state, found) = gather(workspace, true);
    let Some(bundle) = found.into_iter().find(|bundle| bundle.name == name) else {
        return Err(format!("unknown plugin {name}"));
    };
    let Some(Ok(hash)) = bundle.hash else {
        return Err(format!("trust {name} before enabling"));
    };
    let entry = state.entries.entry(name.clone()).or_default();
    if entry.hash != hash || entry.version != bundle.version || entry.hash.is_empty() {
        return Err(format!("trust {name} {} before enabling", bundle.version));
    }
    entry.enabled = true;
    save_state(&state)?;
    crate::mcp::invalidate();
    Ok(())
}

pub fn disable_plugin(name: &str) -> Result<(), String> {
    let name = require_name(name)?;
    let _guard = state_lock();
    let mut state = load_state();
    if let Some(entry) = state.entries.get_mut(&name) {
        entry.enabled = false;
    }
    save_state(&state)?;
    crate::mcp::invalidate();
    Ok(())
}

pub fn remove_plugin(workspace: &Path, name: &str) -> Result<(), String> {
    let name = require_name(name)?;
    let _guard = state_lock();
    let (_state, found) = gather(workspace, false);
    if let Some(bundle) = found.iter().find(|bundle| bundle.name == name) {
        if bundle.installed && inside_install(&bundle.root) {
            remove_path(&bundle.root).map_err(|err| err.to_string())?;
        }
    }
    let mut state = load_state();
    state.entries.remove(&name);
    save_state(&state)?;
    crate::mcp::invalidate();
    Ok(())
}

pub fn marketplace_url() -> String {
    let _guard = state_lock();
    load_state().marketplace_url
}

pub fn set_marketplace_url(url: &str) -> Result<(), String> {
    let url = url.trim();
    let _guard = state_lock();
    let mut state = load_state();
    if url.is_empty() {
        state.marketplace_url.clear();
    } else {
        state.marketplace_url = https_index_url(url)?.to_string();
    }
    save_state(&state)
}

pub fn fetch_marketplace(url: &str) -> Result<Vec<MarketEntry>, String> {
    let url = https_index_url(url)?;
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(20))
        .redirects(0)
        .build();
    let response = agent
        .get(url)
        .call()
        .map_err(|err| format!("marketplace index: {err}"))?;
    let mut buf = Vec::new();
    response
        .into_reader()
        .take((MAX_INDEX + 1) as u64)
        .read_to_end(&mut buf)
        .map_err(|err| err.to_string())?;
    if buf.len() > MAX_INDEX {
        return Err("marketplace index is too large".into());
    }
    let text = String::from_utf8(buf).map_err(|_| "marketplace index is not utf-8".to_string())?;
    parse_marketplace(&text)
}

pub fn load_marketplace_file(path: &Path) -> Result<Vec<MarketEntry>, String> {
    let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
    if forbidden_name(name) || forbidden_path(path) {
        return Err("refusing to read that file".into());
    }
    let text = fs::read_to_string(path).map_err(|err| err.to_string())?;
    if text.len() > MAX_INDEX {
        return Err("marketplace index is too large".into());
    }
    parse_marketplace(&text)
}

pub fn parse_marketplace(text: &str) -> Result<Vec<MarketEntry>, String> {
    let value: Value = serde_json::from_str(text)
        .map_err(|err| format!("marketplace index is not JSON: {err}"))?;
    let Some(plugins) = value.get("plugins").and_then(|item| item.as_array()) else {
        return Err("marketplace index has no plugins array".into());
    };
    let mut out = Vec::new();
    for item in plugins {
        let Some(name) = item.get("name").and_then(|value| value.as_str()) else {
            continue;
        };
        let name = name.trim();
        if !valid_plugin_name(name) {
            continue;
        }
        let version = item.get("version").map(value_string).unwrap_or_default();
        let description = item
            .get("description")
            .and_then(|value| value.as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        let source = item
            .get("source")
            .and_then(source_string)
            .unwrap_or_default();
        out.push(MarketEntry {
            name: name.to_string(),
            version,
            description,
            source,
        });
    }
    Ok(out)
}

fn any_enabled() -> bool {
    load_state().entries.values().any(|entry| entry.enabled)
}

fn gather(workspace: &Path, hash_all: bool) -> (StateFile, Vec<Bundle>) {
    let mut state = load_state();
    let found = scan(workspace, &state, hash_all);
    if reconcile(&mut state, &found) {
        let _ = save_state(&state);
        crate::mcp::invalidate();
    }
    (state, found)
}

fn reconcile(state: &mut StateFile, found: &[Bundle]) -> bool {
    let mut changed = false;
    let names: Vec<String> = state.entries.keys().cloned().collect();
    for name in names {
        let Some(entry) = state.entries.get(&name) else {
            continue;
        };
        if entry.hash.is_empty() && !entry.enabled {
            continue;
        }
        let Some(bundle) = found.iter().find(|bundle| bundle.name == name) else {
            if let Some(entry) = state.entries.get_mut(&name) {
                entry.enabled = false;
                entry.hash.clear();
                changed = true;
            }
            continue;
        };
        let Some(hash) = bundle.hash.as_ref() else {
            continue;
        };
        match hash {
            Ok(hash) if entry.hash == *hash && entry.version == bundle.version => {}
            _ => {
                if let Some(entry) = state.entries.get_mut(&name) {
                    entry.enabled = false;
                    entry.hash.clear();
                    entry.version.clear();
                    changed = true;
                }
            }
        }
    }
    changed
}

fn is_active(state: &StateFile, bundle: &Bundle) -> bool {
    let Some(Ok(hash)) = bundle.hash.as_ref() else {
        return false;
    };
    state.entries.get(&bundle.name).is_some_and(|entry| {
        entry.enabled
            && !entry.hash.is_empty()
            && entry.hash == *hash
            && entry.version == bundle.version
    })
}

fn is_trusted(state: &StateFile, bundle: &Bundle) -> bool {
    let Some(Ok(hash)) = bundle.hash.as_ref() else {
        return false;
    };
    state.entries.get(&bundle.name).is_some_and(|entry| {
        !entry.hash.is_empty() && entry.hash == *hash && entry.version == bundle.version
    })
}

fn scan(workspace: &Path, state: &StateFile, hash_all: bool) -> Vec<Bundle> {
    let mut map: BTreeMap<String, Bundle> = BTreeMap::new();
    for dir in external_candidates(workspace) {
        consider(&mut map, &dir, false, state, hash_all);
    }
    if let Ok(rd) = fs::read_dir(install_root()) {
        let mut dirs: Vec<PathBuf> = rd
            .filter_map(|ent| ent.ok())
            .map(|ent| ent.path())
            .collect();
        dirs.sort();
        for dir in dirs {
            let name = dir.file_name().and_then(|s| s.to_str()).unwrap_or("");
            if name.starts_with('.') || !is_real_dir(&dir) {
                continue;
            }
            consider(&mut map, &dir, true, state, hash_all);
        }
    }
    map.into_values().collect()
}

fn consider(
    map: &mut BTreeMap<String, Bundle>,
    root: &Path,
    installed: bool,
    state: &StateFile,
    hash_all: bool,
) {
    let Some(mut bundle) = load_bundle(root, installed) else {
        return;
    };
    if hash_all || should_hash(&bundle.name, state) {
        bundle.hash = Some(cached_hash(&bundle.root));
    }
    if installed {
        map.insert(bundle.name.clone(), bundle);
    } else {
        map.entry(bundle.name.clone()).or_insert(bundle);
    }
}

fn should_hash(name: &str, state: &StateFile) -> bool {
    state
        .entries
        .get(name)
        .is_some_and(|entry| entry.enabled || !entry.hash.is_empty())
}

#[cfg(test)]
thread_local! {
    static HOME_OVERRIDE: std::cell::RefCell<Option<PathBuf>> =
        const { std::cell::RefCell::new(None) };
}

/// Tests point this at a scratch home per thread, so parallel tests never
/// race on the process-wide HOME variable.
fn plugin_home() -> Option<PathBuf> {
    #[cfg(test)]
    if let Some(home) = HOME_OVERRIDE.with(|slot| slot.borrow().clone()) {
        return Some(home);
    }
    grokhub_core::user_home()
}

#[cfg(test)]
pub(crate) struct HomeGuard {
    prev: Option<PathBuf>,
}

#[cfg(test)]
impl HomeGuard {
    pub(crate) fn set(path: &Path) -> Self {
        let prev = HOME_OVERRIDE.with(|slot| slot.borrow_mut().replace(path.to_path_buf()));
        Self { prev }
    }
}

#[cfg(test)]
impl Drop for HomeGuard {
    fn drop(&mut self) {
        HOME_OVERRIDE.with(|slot| *slot.borrow_mut() = self.prev.take());
    }
}

fn external_candidates(workspace: &Path) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(home) = plugin_home() {
        roots.push(home.join(".claude").join("plugins"));
        roots.push(home.join(".grok").join("plugins"));
    }
    for dir in crate::skills::project_chain(workspace) {
        roots.push(dir.join(".claude").join("plugins"));
        roots.push(dir.join(".grok").join("plugins"));
    }
    let install = install_root();
    let mut out = Vec::new();
    for root in roots {
        if root.starts_with(&install) {
            continue;
        }
        push_child_dirs(&root, &install, &mut out);
        push_installed_record(&root, &install, &mut out);
    }
    out.sort();
    out.dedup();
    out
}

fn push_child_dirs(root: &Path, install: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = fs::read_dir(root) else {
        return;
    };
    for ent in rd.filter_map(|ent| ent.ok()) {
        let path = ent.path();
        let name = ent.file_name();
        let name = name.to_string_lossy();
        if name.starts_with('.') || name == "cache" || name == "marketplaces" {
            continue;
        }
        if path.starts_with(install) || !is_real_dir(&path) {
            continue;
        }
        out.push(path);
    }
}

fn push_installed_record(root: &Path, install: &Path, out: &mut Vec<PathBuf>) {
    let path = root.join("installed_plugins.json");
    if !is_real_file(&path) || forbidden_path(&path) {
        return;
    }
    let Ok(meta) = fs::symlink_metadata(&path) else {
        return;
    };
    if meta.len() > MAX_READ {
        return;
    }
    let Ok(text) = fs::read_to_string(&path) else {
        return;
    };
    let Ok(value) = serde_json::from_str::<Value>(&text) else {
        return;
    };
    let Some(plugins) = value.get("plugins").and_then(|item| item.as_object()) else {
        return;
    };
    for entries in plugins.values() {
        let list: Vec<&Value> = match entries {
            Value::Array(items) => items.iter().collect(),
            other => vec![other],
        };
        for entry in list {
            let Some(raw) = entry
                .get("installPath")
                .or_else(|| entry.get("install_path"))
                .and_then(|item| item.as_str())
            else {
                continue;
            };
            let candidate = PathBuf::from(raw.trim());
            if !candidate.is_absolute()
                || candidate.starts_with(install)
                || !is_real_dir(&candidate)
            {
                continue;
            }
            let name = candidate.file_name().and_then(|s| s.to_str()).unwrap_or("");
            if forbidden_name(name) {
                continue;
            }
            out.push(candidate);
        }
    }
}

fn load_bundle(root: &Path, installed: bool) -> Option<Bundle> {
    let manifest = read_manifest(root).ok()?;
    if installed {
        let dir_name = root.file_name().and_then(|s| s.to_str()).unwrap_or("");
        if dir_name != manifest.name {
            return None;
        }
    }
    let mut warnings = Vec::new();
    let parts = components(&manifest, root, &mut warnings);
    Some(Bundle {
        name: manifest.name,
        version: manifest.version,
        description: manifest.description,
        root: root.to_path_buf(),
        installed,
        hash: None,
        skill_dirs: parts.skill_dirs,
        hook_paths: parts.hook_paths,
        inline_hooks: parts.inline_hooks,
        hook_commands: parts.hook_commands,
        mcp_docs: parts.mcp_docs,
        mcp_lines: parts.mcp_lines,
        agents: parts.agents,
        commands: parts.commands,
        warnings,
    })
}

struct Parts {
    skill_dirs: Vec<PathBuf>,
    hook_paths: Vec<PathBuf>,
    inline_hooks: Option<(PathBuf, String)>,
    hook_commands: Vec<String>,
    mcp_docs: Vec<String>,
    mcp_lines: Vec<String>,
    agents: Vec<Doc>,
    commands: Vec<Doc>,
}

fn components(manifest: &Manifest, root: &Path, warnings: &mut Vec<String>) -> Parts {
    let value = &manifest.value;
    let skill_dirs = component_paths(value.get("skills"), root, "skills", warnings, "skills")
        .into_iter()
        .filter_map(|path| skill_root(&path))
        .collect();
    let (hook_paths, inline_hooks, hook_commands) = hook_parts(value.get("hooks"), root, warnings);
    let (mcp_docs, mcp_lines) = mcp_parts(
        value.get("mcpServers").or_else(|| value.get("mcp_servers")),
        &manifest.name,
        root,
        warnings,
    );
    let agents = docs_from(value.get("agents"), root, "agents", warnings, "agents");
    let commands = docs_from(
        value.get("commands"),
        root,
        "commands",
        warnings,
        "commands",
    );
    Parts {
        skill_dirs,
        hook_paths,
        inline_hooks,
        hook_commands,
        mcp_docs,
        mcp_lines,
        agents,
        commands,
    }
}

fn hook_parts(
    field: Option<&Value>,
    root: &Path,
    warnings: &mut Vec<String>,
) -> (Vec<PathBuf>, Option<(PathBuf, String)>, Vec<String>) {
    match field {
        Some(value) if value.is_object() => {
            let text = normalize_hooks(value);
            let commands = hook_commands(&text);
            let path = manifest_path(root).unwrap_or_else(|| root.join("plugin.json"));
            (vec![path.clone()], Some((path, text)), commands)
        }
        Some(_) => {
            let paths = component_paths(field, root, "", warnings, "hooks");
            let mut commands = Vec::new();
            for path in &paths {
                if let Some(text) = read_text_inside(root, path) {
                    commands.extend(hook_commands(&text));
                } else if is_real_dir(path) {
                    commands.extend(hook_commands_in_dir(root, path));
                }
            }
            (paths, None, commands)
        }
        None => {
            let file = root.join("hooks").join("hooks.json");
            let dir = root.join("hooks");
            if is_real_file(&file) {
                let commands = read_text_inside(root, &file)
                    .map(|text| hook_commands(&text))
                    .unwrap_or_default();
                (vec![file], None, commands)
            } else if is_real_dir(&dir) {
                (vec![dir.clone()], None, hook_commands_in_dir(root, &dir))
            } else {
                (Vec::new(), None, Vec::new())
            }
        }
    }
}

fn hook_commands_in_dir(root: &Path, dir: &Path) -> Vec<String> {
    let Ok(rd) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = rd
        .filter_map(|ent| ent.ok())
        .map(|ent| ent.path())
        .collect();
    files.sort();
    let mut out = Vec::new();
    for path in files {
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        if let Some(text) = read_text_inside(root, &path) {
            out.extend(hook_commands(&text));
        }
    }
    out
}

fn mcp_parts(
    field: Option<&Value>,
    plugin: &str,
    root: &Path,
    warnings: &mut Vec<String>,
) -> (Vec<String>, Vec<String>) {
    let value = match field {
        Some(Value::Object(map)) => Value::Object(map.clone()),
        Some(Value::String(rel)) => match relative_inside(root, rel) {
            Ok(path) => match read_text_inside(root, &path) {
                Some(text) => serde_json::from_str(&text).unwrap_or(Value::Null),
                None => {
                    warnings.push("mcpServers file is not readable".into());
                    return (Vec::new(), Vec::new());
                }
            },
            Err(err) => {
                warnings.push(format!("mcpServers: {err}"));
                return (Vec::new(), Vec::new());
            }
        },
        Some(_) => {
            warnings.push("mcpServers must be a path or an object".into());
            return (Vec::new(), Vec::new());
        }
        None => {
            let path = root.join(".mcp.json");
            if !is_real_file(&path) {
                return (Vec::new(), Vec::new());
            }
            match read_text_inside(root, &path) {
                Some(text) => serde_json::from_str(&text).unwrap_or(Value::Null),
                None => return (Vec::new(), Vec::new()),
            }
        }
    };
    match prefix_mcp(plugin, &value) {
        Some((doc, lines)) => (vec![doc], lines),
        None => (Vec::new(), Vec::new()),
    }
}

fn docs_from(
    field: Option<&Value>,
    root: &Path,
    default_rel: &str,
    warnings: &mut Vec<String>,
    label: &str,
) -> Vec<Doc> {
    let paths = component_paths(field, root, default_rel, warnings, label);
    let mut files = Vec::new();
    for path in &paths {
        walk_md(path, &mut files, 0);
    }
    files.sort();
    files.dedup();
    let mut docs = Vec::new();
    for file in files {
        if docs.len() >= 40 {
            break;
        }
        let Some(text) = read_text_inside(root, &file) else {
            continue;
        };
        let fallback = file.file_stem().and_then(|s| s.to_str()).unwrap_or("doc");
        let (name, description, body) = split_doc(&text, fallback);
        if !valid_doc_name(&name) {
            warnings.push(format!("{label}: invalid name {name}"));
            continue;
        }
        docs.push(Doc {
            name,
            description,
            body,
        });
    }
    docs
}

fn component_paths(
    field: Option<&Value>,
    root: &Path,
    default_rel: &str,
    warnings: &mut Vec<String>,
    label: &str,
) -> Vec<PathBuf> {
    let Some(field) = field else {
        if default_rel.is_empty() {
            return Vec::new();
        }
        let path = root.join(default_rel);
        if is_real_dir(&path) || is_real_file(&path) {
            return vec![path];
        }
        return Vec::new();
    };
    if field.is_object() {
        return Vec::new();
    }
    let mut specs = Vec::new();
    match field {
        Value::String(text) => specs.push(text.clone()),
        Value::Array(items) => {
            for item in items {
                if let Some(text) = item.as_str() {
                    specs.push(text.to_string());
                }
            }
        }
        _ => {
            warnings.push(format!("{label} must be a path or a list of paths"));
            return Vec::new();
        }
    }
    let mut out = Vec::new();
    for spec in specs {
        match relative_inside(root, &spec) {
            Ok(path) => {
                if !is_real_file(&path) && !is_real_dir(&path) {
                    warnings.push(format!("{label} not found: {spec}"));
                }
                out.push(path);
            }
            Err(err) => warnings.push(format!("{label}: {err}")),
        }
    }
    out
}

fn skill_root(path: &Path) -> Option<PathBuf> {
    if is_real_dir(path) {
        return Some(path.to_path_buf());
    }
    if is_real_file(path) && path.file_name().and_then(|s| s.to_str()) == Some("SKILL.md") {
        return path.parent().map(Path::to_path_buf);
    }
    None
}

fn walk_md(path: &Path, out: &mut Vec<PathBuf>, depth: usize) {
    if depth > 6 || out.len() >= 40 {
        return;
    }
    if is_real_file(path) {
        if path.extension().and_then(|ext| ext.to_str()) == Some("md") {
            out.push(path.to_path_buf());
        }
        return;
    }
    if !is_real_dir(path) {
        return;
    }
    let Ok(rd) = fs::read_dir(path) else {
        return;
    };
    let mut kids: Vec<PathBuf> = rd
        .filter_map(|ent| ent.ok())
        .map(|ent| ent.path())
        .collect();
    kids.sort();
    for kid in kids {
        let name = kid.file_name().and_then(|s| s.to_str()).unwrap_or("");
        if name.starts_with('.') || name == ".git" {
            continue;
        }
        walk_md(&kid, out, depth + 1);
    }
}

fn normalize_hooks(value: &Value) -> String {
    if value.get("hooks").is_some() {
        value.to_string()
    } else {
        json!({ "hooks": value }).to_string()
    }
}

fn hook_commands(text: &str) -> Vec<String> {
    let Ok(value) = serde_json::from_str::<Value>(text) else {
        return Vec::new();
    };
    let Some(map) = value.get("hooks").and_then(|item| item.as_object()) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for groups in map.values() {
        let Some(groups) = groups.as_array() else {
            continue;
        };
        for group in groups {
            let Some(hooks) = group.get("hooks").and_then(|item| item.as_array()) else {
                continue;
            };
            for hook in hooks {
                for key in ["command", "url"] {
                    if let Some(text) = hook.get(key).and_then(|item| item.as_str()) {
                        let text = text.trim();
                        if !text.is_empty() {
                            out.push(text.to_string());
                        }
                    }
                }
            }
        }
    }
    out
}

fn prefix_mcp(plugin: &str, value: &Value) -> Option<(String, Vec<String>)> {
    let block = value
        .get("mcpServers")
        .or_else(|| value.get("mcp_servers"))
        .and_then(|item| item.as_object())
        .cloned()
        .or_else(|| value.as_object().cloned())?;
    let mut servers = Map::new();
    let mut lines = Vec::new();
    for (name, spec) in block {
        let name = name.trim();
        if name.is_empty() || name.contains('/') || name.contains('\\') || is_desktop_name(name) {
            continue;
        }
        if !spec.is_object() {
            continue;
        }
        let key = format!("plugin__{plugin}__{name}");
        let line = mcp_line(&key, &spec);
        servers.insert(key, spec);
        lines.push(line);
    }
    if servers.is_empty() {
        return None;
    }
    Some((json!({ "mcpServers": servers }).to_string(), lines))
}

fn mcp_line(name: &str, spec: &Value) -> String {
    let command = spec
        .get("command")
        .and_then(|item| item.as_str())
        .unwrap_or("")
        .trim();
    let url = spec
        .get("url")
        .and_then(|item| item.as_str())
        .unwrap_or("")
        .trim();
    let args = spec
        .get("args")
        .and_then(|item| item.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str())
                .collect::<Vec<_>>()
                .join(" ")
        })
        .unwrap_or_default();
    if !url.is_empty() {
        format!("{name}: {url}")
    } else if args.is_empty() {
        format!("{name}: {command}")
    } else {
        format!("{name}: {command} {args}")
    }
}

fn is_desktop_name(name: &str) -> bool {
    grokhub_core::CABIN_CU_SERVERS.contains(&name.trim().to_ascii_lowercase().replace('_', "-").as_str())
}

fn read_manifest(root: &Path) -> Result<Manifest, String> {
    let path = manifest_path(root).ok_or_else(|| {
        "no plugin.json, .grok-plugin/plugin.json, or .claude-plugin/plugin.json".to_string()
    })?;
    let text = read_text_inside(root, &path)
        .ok_or_else(|| "plugin manifest is not readable".to_string())?;
    let value: Value =
        serde_json::from_str(&text).map_err(|err| format!("plugin manifest is not JSON: {err}"))?;
    let name = value
        .get("name")
        .and_then(|item| item.as_str())
        .unwrap_or("")
        .trim();
    if !valid_plugin_name(name) {
        return Err(format!("invalid plugin name `{name}`"));
    }
    let version = value
        .get("version")
        .map(value_string)
        .filter(|text| !text.is_empty())
        .unwrap_or_else(|| "0.0.0".to_string());
    if version.len() > 64
        || version.contains('/')
        || version.contains('\\')
        || version.contains("..")
    {
        return Err("invalid plugin version".into());
    }
    let description = value
        .get("description")
        .and_then(|item| item.as_str())
        .unwrap_or("")
        .trim()
        .chars()
        .take(400)
        .collect();
    Ok(Manifest {
        name: name.to_string(),
        version,
        description,
        value,
    })
}

fn manifest_path(root: &Path) -> Option<PathBuf> {
    for rel in [
        "plugin.json",
        ".grok-plugin/plugin.json",
        ".claude-plugin/plugin.json",
    ] {
        let path = root.join(rel);
        if is_real_file(&path) && path.starts_with(root) {
            return Some(path);
        }
    }
    None
}

fn relative_inside(root: &Path, raw: &str) -> Result<PathBuf, String> {
    let raw = raw.trim();
    if raw.is_empty() || raw.contains('\0') {
        return Err("path escapes the bundle".into());
    }
    let path = Path::new(raw);
    if path.is_absolute() {
        return Err("absolute path".into());
    }
    for comp in path.components() {
        match comp {
            Component::Normal(part) => {
                let text = part.to_string_lossy();
                if text == ".." || text.contains('\0') || text.contains('/') || text.contains('\\')
                {
                    return Err("path escapes the bundle".into());
                }
            }
            Component::CurDir => {}
            _ => return Err("path escapes the bundle".into()),
        }
    }
    let joined = root.join(path);
    if !joined.starts_with(root) {
        return Err("path escapes the bundle".into());
    }
    if !chain_stays(root, &joined)? {
        return Err("symlink escapes the bundle".into());
    }
    Ok(joined)
}

fn chain_stays(root: &Path, path: &Path) -> Result<bool, String> {
    let Ok(rel) = path.strip_prefix(root) else {
        return Ok(false);
    };
    let mut acc = root.to_path_buf();
    let comps: Vec<_> = rel.components().collect();
    for (index, comp) in comps.iter().enumerate() {
        let Component::Normal(part) = comp else {
            return Ok(false);
        };
        acc.push(part);
        match fs::symlink_metadata(&acc) {
            Ok(meta) if meta.file_type().is_symlink() => {
                if !symlink_stays_inside(root, &acc)? {
                    return Ok(false);
                }
            }
            Ok(_) => {}
            Err(_) => return Ok(index + 1 == comps.len()),
        }
    }
    Ok(true)
}

fn symlink_stays_inside(root: &Path, link: &Path) -> Result<bool, String> {
    let mut current = link.to_path_buf();
    for _ in 0..8 {
        let target =
            fs::read_link(&current).map_err(|err| format!("{}: {err}", current.display()))?;
        if target.is_absolute() || target.as_os_str().is_empty() {
            return Ok(false);
        }
        let Some(base) = current.parent() else {
            return Ok(false);
        };
        if !base.starts_with(root) && base != root {
            return Ok(false);
        }
        let mut resolved = base.to_path_buf();
        for comp in target.components() {
            match comp {
                Component::Normal(part) => resolved.push(part),
                Component::CurDir => {}
                Component::ParentDir => {
                    if !resolved.pop() || !resolved.starts_with(root) {
                        return Ok(false);
                    }
                }
                _ => return Ok(false),
            }
        }
        if !resolved.starts_with(root) {
            return Ok(false);
        }
        match fs::symlink_metadata(&resolved) {
            Ok(meta) if meta.file_type().is_symlink() => current = resolved,
            _ => return Ok(true),
        }
    }
    Ok(false)
}

fn read_text_inside(root: &Path, path: &Path) -> Option<String> {
    if forbidden_path(path) || !path.starts_with(root) || !is_real_file(path) {
        return None;
    }
    if !chain_stays(root, path).unwrap_or(false) {
        return None;
    }
    let meta = fs::symlink_metadata(path).ok()?;
    if meta.len() > MAX_READ {
        return None;
    }
    fs::read_to_string(path).ok()
}

fn cached_hash(root: &Path) -> Result<String, String> {
    let (fingerprint, racy) = fingerprint(root)?;
    if let Some(hit) = memo_get(root, &fingerprint) {
        return Ok(hit);
    }
    let hash = full_hash(root)?;
    // A file changed in the last two seconds could change again inside the
    // same timestamp tick without moving its stamp, so don't remember it yet.
    if !racy {
        memo_put(root, &fingerprint, &hash);
    }
    Ok(hash)
}

fn memo_get(root: &Path, fingerprint: &str) -> Option<String> {
    let memo = hash_memo().lock().unwrap_or_else(|err| err.into_inner());
    memo.iter()
        .find(|item| item.root == root && item.fingerprint == fingerprint && memo_fresh(item.at))
        .map(|item| item.hash.clone())
}

fn memo_put(root: &Path, fingerprint: &str, hash: &str) {
    let mut memo = hash_memo().lock().unwrap_or_else(|err| err.into_inner());
    memo.retain(|item| item.root != root);
    if memo.len() > 64 {
        memo.clear();
    }
    memo.push(HashMemo {
        root: root.to_path_buf(),
        fingerprint: fingerprint.to_string(),
        hash: hash.to_string(),
        at: Instant::now(),
    });
}

/// Unix fingerprints carry the inode change time, which an editor can't set
/// back, so a hit stays good. Elsewhere only size and mtime are visible and
/// mtime can be restored, so a hit is reused for two seconds at most (enough
/// to keep a repainting page from rehashing every frame).
fn memo_fresh(at: Instant) -> bool {
    cfg!(unix) || at.elapsed() < Duration::from_secs(2)
}

enum Item {
    File { rel: String, path: PathBuf },
    Link { rel: String, target: String },
}

fn collect_items(root: &Path) -> Result<Vec<Item>, String> {
    let mut items = Vec::new();
    walk_items(root, root, &mut items)?;
    items.sort_by(|a, b| rel_of(a).cmp(rel_of(b)));
    Ok(items)
}

fn rel_of(item: &Item) -> &str {
    match item {
        Item::File { rel, .. } | Item::Link { rel, .. } => rel,
    }
}

fn walk_items(root: &Path, dir: &Path, out: &mut Vec<Item>) -> Result<(), String> {
    if out.len() > MAX_WALK {
        return Err("bundle is too large to trust".into());
    }
    let rd = fs::read_dir(dir).map_err(|err| format!("{}: {err}", dir.display()))?;
    let mut kids: Vec<PathBuf> = rd
        .filter_map(|ent| ent.ok())
        .map(|ent| ent.path())
        .collect();
    kids.sort();
    for path in kids {
        if out.len() > MAX_WALK {
            return Err("bundle is too large to trust".into());
        }
        let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
        if name == ".git" {
            continue;
        }
        let rel = rel_key(root, &path);
        let meta = fs::symlink_metadata(&path).map_err(|err| format!("{rel}: {err}"))?;
        if meta.file_type().is_symlink() {
            if !symlink_stays_inside(root, &path)? {
                return Err(format!("symlink escapes the bundle: {rel}"));
            }
            let target = fs::read_link(&path)
                .map_err(|err| format!("{rel}: {err}"))?
                .to_string_lossy()
                .into_owned();
            out.push(Item::Link { rel, target });
            continue;
        }
        if meta.is_dir() {
            walk_items(root, &path, out)?;
            continue;
        }
        if meta.is_file() {
            // A credential-like file is never read, so its bytes can't be
            // hashed. Refuse the whole bundle rather than trust content that
            // could change without dropping trust.
            if forbidden_name(name) || forbidden_path(&path) {
                return Err(format!(
                    "bundle contains a credential-like file ({rel}), so it can't be trusted"
                ));
            }
            out.push(Item::File { rel, path });
        }
    }
    Ok(())
}

fn fingerprint(root: &Path) -> Result<(String, bool), String> {
    let items = collect_items(root)?;
    let mut out = String::new();
    let mut racy = false;
    for item in items {
        match item {
            Item::File { rel, path } => {
                let meta = fs::symlink_metadata(&path).map_err(|err| err.to_string())?;
                let modified = meta
                    .modified()
                    .ok()
                    .and_then(|time| time.duration_since(SystemTime::UNIX_EPOCH).ok())
                    .map(|time| format!("{}.{}", time.as_secs(), time.subsec_nanos()))
                    .unwrap_or_default();
                out.push_str(&format!(
                    "f\n{rel}\n{}\n{modified}\n{}\n",
                    meta.len(),
                    change_stamp(&meta)
                ));
                racy |= changed_recently(&meta);
            }
            Item::Link { rel, target } => out.push_str(&format!("l\n{rel}\n{target}\n")),
        }
    }
    Ok((out, racy))
}

fn changed_recently(meta: &fs::Metadata) -> bool {
    let Some(changed) = change_secs(meta) else {
        return true;
    };
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|time| time.as_secs_f64())
        .unwrap_or(0.0);
    now - changed < 2.0
}

#[cfg(unix)]
fn change_secs(meta: &fs::Metadata) -> Option<f64> {
    use std::os::unix::fs::MetadataExt;
    Some(meta.ctime() as f64 + meta.ctime_nsec() as f64 / 1e9)
}

#[cfg(not(unix))]
fn change_secs(meta: &fs::Metadata) -> Option<f64> {
    meta.modified()
        .ok()?
        .duration_since(SystemTime::UNIX_EPOCH)
        .ok()
        .map(|time| time.as_secs_f64())
}

/// The inode change time moves on every content write and can't be set back
/// from user space, so an edit that keeps the size and restores mtime still
/// misses the memo and gets rehashed.
#[cfg(unix)]
fn change_stamp(meta: &fs::Metadata) -> String {
    use std::os::unix::fs::MetadataExt;
    format!("{}.{}.{}", meta.ctime(), meta.ctime_nsec(), meta.ino())
}

#[cfg(not(unix))]
fn change_stamp(_meta: &fs::Metadata) -> String {
    String::new()
}

fn full_hash(root: &Path) -> Result<String, String> {
    let items = collect_items(root)?;
    let mut hasher = Sha256::new();
    for item in items {
        match item {
            Item::File { rel, path } => {
                hasher.update(rel.as_bytes());
                hasher.update([0]);
                hasher.update(b"f");
                hash_file(&path, &mut hasher)?;
            }
            Item::Link { rel, target } => {
                hasher.update(rel.as_bytes());
                hasher.update([0]);
                hasher.update(b"l");
                hasher.update(target.as_bytes());
            }
        }
        hasher.update([0xff]);
    }
    Ok(hex_encode(&hasher.finalize()))
}

fn hash_file(path: &Path, hasher: &mut Sha256) -> Result<(), String> {
    let mut file = File::open(path).map_err(|err| format!("{}: {err}", path.display()))?;
    let mut buf = [0u8; 8192];
    loop {
        let read = file
            .read(&mut buf)
            .map_err(|err| format!("{}: {err}", path.display()))?;
        if read == 0 {
            return Ok(());
        }
        hasher.update(&buf[..read]);
    }
}

fn rel_key(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .components()
        .map(|comp| comp.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

fn info_of(bundle: &Bundle, state: &StateFile) -> PluginInfo {
    PluginInfo {
        name: bundle.name.clone(),
        version: bundle.version.clone(),
        description: bundle.description.clone(),
        enabled: is_active(state, bundle),
        trusted: is_trusted(state, bundle),
        installed: bundle.installed,
        summary: summary_of(bundle),
    }
}

fn summary_of(bundle: &Bundle) -> String {
    let mut lines = Vec::new();
    lines.push(format!("{} {}", bundle.name, bundle.version));
    if !bundle.description.is_empty() {
        lines.push(bundle.description.clone());
    }
    if !bundle.skill_dirs.is_empty() {
        let skills = bundle
            .skill_dirs
            .iter()
            .map(|path| rel_key(&bundle.root, path))
            .collect::<Vec<_>>()
            .join(", ");
        lines.push(format!("skills: {skills}"));
    }
    if !bundle.hook_commands.is_empty() {
        lines.push(format!("hooks: {}", bundle.hook_commands.join("; ")));
    }
    if !bundle.mcp_lines.is_empty() {
        lines.push(format!("mcp: {}", bundle.mcp_lines.join("; ")));
    }
    if !bundle.agents.is_empty() {
        let names = bundle
            .agents
            .iter()
            .map(|doc| doc.name.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        lines.push(format!("agents: {names}"));
    }
    if !bundle.commands.is_empty() {
        let names = bundle
            .commands
            .iter()
            .map(|doc| doc.name.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        lines.push(format!("commands: {names}"));
    }
    if let Some(Err(err)) = &bundle.hash {
        lines.push(format!("unusable: {err}"));
    }
    for warning in &bundle.warnings {
        lines.push(format!("refused: {warning}"));
    }
    lines.join("\n")
}

fn push_docs(out: &mut String, tag: &str, docs: &[Doc]) {
    if docs.is_empty() || out.len() > 16_000 {
        return;
    }
    out.push('<');
    out.push_str(tag);
    out.push_str(">\n");
    for doc in docs {
        let block = format!(
            "- {}: {}\n{}\n",
            doc.name,
            doc.description,
            clip(&doc.body, 4_000)
        );
        if out.len() + block.len() > 16_000 {
            break;
        }
        out.push_str(&block);
    }
    out.push_str("</");
    out.push_str(tag);
    out.push_str(">\n");
}

fn split_doc(text: &str, fallback: &str) -> (String, String, String) {
    let text = text.trim_start_matches('\u{feff}').trim_start();
    if let Some(rest) = text.strip_prefix("---") {
        let rest = rest.trim_start_matches(['\r', '\n']);
        if let Some((yaml, body)) = rest.split_once("\n---") {
            let body = body.trim_start_matches(['\r', '\n']).trim();
            let name = yaml_field(yaml, "name").unwrap_or_else(|| fallback.to_string());
            let description = yaml_field(yaml, "description").unwrap_or_default();
            return (name, description, body.to_string());
        }
    }
    (fallback.to_string(), String::new(), text.trim().to_string())
}

fn yaml_field(yaml: &str, key: &str) -> Option<String> {
    for line in yaml.lines() {
        let line = line.trim();
        let Some((found, value)) = line.split_once(':') else {
            continue;
        };
        if found.trim() != key {
            continue;
        }
        let value = value.trim().trim_matches(['"', '\'']).trim();
        if !value.is_empty() {
            return Some(value.to_string());
        }
    }
    None
}

fn load_state() -> StateFile {
    let Ok(text) = fs::read_to_string(state_path()) else {
        return StateFile::default();
    };
    serde_json::from_str(&text).unwrap_or_default()
}

fn save_state(state: &StateFile) -> Result<(), String> {
    let path = state_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }
    let body = serde_json::to_string_pretty(state).map_err(|err| err.to_string())?;
    let tmp = path.with_extension("json.tmp");
    {
        let mut file = File::create(&tmp).map_err(|err| err.to_string())?;
        file.write_all(body.as_bytes())
            .map_err(|err| err.to_string())?;
    }
    fs::rename(&tmp, &path).map_err(|err| err.to_string())
}

fn require_name(name: &str) -> Result<String, String> {
    let name = name.trim();
    if valid_plugin_name(name) {
        Ok(name.to_string())
    } else {
        Err("invalid plugin name".into())
    }
}

fn valid_plugin_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_NAME
        && !name.contains("..")
        && !name.contains('/')
        && !name.contains('\\')
        && !name.contains('\0')
        && !name.starts_with('.')
        && !name.starts_with('-')
        && !name.ends_with('-')
        && name
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
}

fn valid_doc_name(name: &str) -> bool {
    valid_plugin_name(name)
}

fn value_string(value: &Value) -> String {
    match value {
        Value::String(text) => text.trim().to_string(),
        Value::Number(number) => number.to_string(),
        _ => String::new(),
    }
}

fn source_string(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => {
            let text = text.trim();
            if text.is_empty() {
                None
            } else {
                Some(text.to_string())
            }
        }
        Value::Object(map) => {
            if let Some(url) = map.get("url").and_then(|item| item.as_str()) {
                let url = url.trim();
                if !url.is_empty() {
                    return Some(url.to_string());
                }
            }
            map.get("path")
                .and_then(|item| item.as_str())
                .map(str::trim)
                .filter(|text| !text.is_empty())
                .map(str::to_string)
        }
        _ => None,
    }
}

fn https_index_url(url: &str) -> Result<&str, String> {
    let url = url.trim();
    if url.is_empty()
        || url.starts_with('-')
        || url.contains('\0')
        || url.contains('\n')
        || url.contains('\r')
        || url.contains(' ')
        || url.to_ascii_lowercase().contains("ext::")
        || url.to_ascii_lowercase().contains("fd::")
    {
        return Err("refusing marketplace index url".into());
    }
    if !url.to_ascii_lowercase().starts_with("https://") {
        return Err("marketplace index is fetched only from an https url".into());
    }
    let rest = url.get("https://".len()..).unwrap_or("");
    if rest.is_empty() || rest.starts_with('/') {
        return Err("marketplace index url has no host".into());
    }
    Ok(url)
}

fn validate_git_url(url: &str) -> Result<String, String> {
    let url = url.trim();
    if url.is_empty()
        || url.starts_with('-')
        || url.contains('\0')
        || url.contains('\n')
        || url.contains('\r')
        || url.contains(' ')
    {
        return Err("refusing git url".into());
    }
    let lower = url.to_ascii_lowercase();
    if lower.contains("ext::") || lower.contains("fd::") {
        return Err("refusing git transport".into());
    }
    if lower.starts_with("https://") || lower.starts_with("ssh://") || lower.starts_with("file://")
    {
        return Ok(url.to_string());
    }
    if let Some(rest) = url.strip_prefix("git@") {
        if rest.contains(':') && !rest.contains("://") && !rest.starts_with('-') {
            return Ok(url.to_string());
        }
    }
    Err("only https, ssh, and file git urls are allowed".into())
}

fn staging_path() -> Result<PathBuf, String> {
    let root = install_root();
    fs::create_dir_all(&root).map_err(|err| err.to_string())?;
    let path = root.join(format!(
        ".staging-{}-{}",
        std::process::id(),
        unique_suffix()
    ));
    if path.exists() {
        return Err("staging path already exists".into());
    }
    Ok(path)
}

fn unique_suffix() -> u128 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|time| time.as_nanos())
        .unwrap_or(0)
}

fn commit_staging(staging: &Path, from_git: bool) -> Result<PluginInfo, String> {
    let outcome = (|| {
        if from_git {
            let git = staging.join(".git");
            if git.exists() || fs::symlink_metadata(&git).is_ok() {
                remove_path(&git).map_err(|err| format!("remove cloned git dir: {err}"))?;
            }
        }
        let manifest = read_manifest(staging)?;
        let _guard = state_lock();
        let dest = install_root().join(&manifest.name);
        if dest.exists() {
            return Err(format!("{} is already installed", manifest.name));
        }
        fs::create_dir_all(install_root()).map_err(|err| err.to_string())?;
        fs::rename(staging, &dest).map_err(|err| err.to_string())?;
        let Some(bundle) = load_bundle(&dest, true) else {
            let _ = remove_path(&dest);
            return Err("installed bundle has no manifest".into());
        };
        let state = load_state();
        Ok(info_of(&bundle, &state))
    })();
    if outcome.is_err() {
        let _ = remove_path(staging);
    }
    outcome
}

fn copy_tree(src_root: &Path, dst_root: &Path) -> Result<(), String> {
    copy_rec(src_root, src_root, dst_root)
}

fn copy_rec(src_root: &Path, src: &Path, dst_root: &Path) -> Result<(), String> {
    let rel = src.strip_prefix(src_root).unwrap_or(src);
    let dest = if rel.as_os_str().is_empty() {
        dst_root.to_path_buf()
    } else {
        dst_root.join(rel)
    };
    let name = src.file_name().and_then(|s| s.to_str()).unwrap_or("");
    if !rel.as_os_str().is_empty() && name == ".git" {
        return Ok(());
    }
    let meta = fs::symlink_metadata(src).map_err(|err| format!("{}: {err}", src.display()))?;
    if meta.file_type().is_symlink() {
        if !symlink_stays_inside(src_root, src)? {
            return Err(format!("symlink escapes the source: {}", rel.display()));
        }
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent).map_err(|err| err.to_string())?;
        }
        return place_symlink(src, &dest);
    }
    if meta.is_dir() {
        fs::create_dir_all(&dest).map_err(|err| err.to_string())?;
        let rd = fs::read_dir(src).map_err(|err| err.to_string())?;
        let mut kids: Vec<PathBuf> = rd
            .filter_map(|ent| ent.ok())
            .map(|ent| ent.path())
            .collect();
        kids.sort();
        for kid in kids {
            copy_rec(src_root, &kid, dst_root)?;
        }
        return Ok(());
    }
    if meta.is_file() {
        if forbidden_name(name) || forbidden_path(src) {
            return Ok(());
        }
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent).map_err(|err| err.to_string())?;
        }
        fs::copy(src, &dest).map_err(|err| err.to_string())?;
    }
    Ok(())
}

fn place_symlink(src: &Path, dest: &Path) -> Result<(), String> {
    let target = fs::read_link(src).map_err(|err| err.to_string())?;
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&target, dest).map_err(|err| err.to_string())?;
        Ok(())
    }
    #[cfg(windows)]
    {
        if std::os::windows::fs::symlink_file(&target, dest).is_ok() {
            return Ok(());
        }
        std::os::windows::fs::symlink_dir(&target, dest).map_err(|err| err.to_string())
    }
}

fn git_clone(url: &str, dest: &Path) -> Result<(), String> {
    let parent = dest
        .parent()
        .ok_or_else(|| "staging path has no parent".to_string())?;
    fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    let template = parent.join(format!(".template-{}", unique_suffix()));
    fs::create_dir_all(template.join("hooks")).map_err(|err| err.to_string())?;
    let empty_cfg = parent.join(format!(".gitconfig-{}", unique_suffix()));
    fs::write(&empty_cfg, "").map_err(|err| err.to_string())?;
    let hooks = template.join("hooks");
    let hooks_arg = hooks.display().to_string();
    let mut args = vec![
        "-c".to_string(),
        format!("core.hooksPath={hooks_arg}"),
        "-c".to_string(),
        "protocol.ext.allow=never".to_string(),
        "-c".to_string(),
        "protocol.fd.allow=never".to_string(),
    ];
    if url.to_ascii_lowercase().starts_with("file:") {
        args.push("-c".to_string());
        args.push("protocol.file.allow=always".to_string());
        args.push("-c".to_string());
        args.push("safe.directory=*".to_string());
    }
    args.extend([
        "clone".to_string(),
        "--depth".to_string(),
        "1".to_string(),
        "--no-recurse-submodules".to_string(),
        "--template".to_string(),
        template.display().to_string(),
        "-c".to_string(),
        format!("core.hooksPath={hooks_arg}"),
        "--".to_string(),
        url.to_string(),
        dest.display().to_string(),
    ]);
    let result = run_git(&args, &empty_cfg);
    let _ = remove_path(&template);
    let _ = remove_path(&empty_cfg);
    result
}

fn run_git(args: &[String], empty_cfg: &Path) -> Result<(), String> {
    let mut cmd = Command::new("git");
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", empty_cfg)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GCM_INTERACTIVE", "never")
        .args(args);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    let mut child = cmd.spawn().map_err(|err| format!("git: {err}"))?;
    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if start.elapsed() > Duration::from_secs(60) => {
                kill_child(&mut child);
                return Err("git timed out".into());
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(30)),
            Err(err) => return Err(err.to_string()),
        }
    };
    let mut err_text = String::new();
    if let Some(mut stderr) = child.stderr.take() {
        let _ = stderr.read_to_string(&mut err_text);
    }
    if status.success() {
        return Ok(());
    }
    let err_text: String = err_text.trim().chars().take(400).collect();
    if err_text.is_empty() {
        Err(format!("git failed ({status})"))
    } else {
        Err(format!("git failed: {err_text}"))
    }
}

fn kill_child(child: &mut std::process::Child) {
    #[cfg(unix)]
    if let Ok(pid) = i32::try_from(child.id()) {
        unsafe {
            libc::kill(-pid, libc::SIGKILL);
        }
    }
    let _ = child.kill();
    let _ = child.wait();
}

fn inside_install(path: &Path) -> bool {
    let root = install_root();
    let Ok(path) = fs::canonicalize(path) else {
        return false;
    };
    let Ok(root) = fs::canonicalize(&root) else {
        return false;
    };
    path.starts_with(&root) && path.parent() == Some(root.as_path())
}

fn remove_path(path: &Path) -> std::io::Result<()> {
    let meta = match fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(err),
    };
    if meta.is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    }
}

fn is_real_dir(path: &Path) -> bool {
    fs::symlink_metadata(path)
        .map(|meta| meta.is_dir())
        .unwrap_or(false)
}

fn is_real_file(path: &Path) -> bool {
    fs::symlink_metadata(path)
        .map(|meta| meta.is_file())
        .unwrap_or(false)
}

fn forbidden_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower == concat!("auth", ".json")
        || lower == concat!("secrets", ".json")
        || lower.contains("secret")
        || lower.contains("token")
        || lower.contains("credential")
        || lower.ends_with(".pem")
        || lower.ends_with(".key")
        || (lower.contains("auth") && lower.ends_with(".json"))
}

fn forbidden_path(path: &Path) -> bool {
    let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
    if forbidden_name(name) {
        return true;
    }
    let parent = path
        .parent()
        .and_then(|parent| parent.file_name())
        .and_then(|s| s.to_str())
        .unwrap_or("");
    parent == ".grok" && name.eq_ignore_ascii_case(concat!("config", ".toml"))
}

fn clip(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        text.to_string()
    } else {
        text.chars().take(max).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    struct Iso {
        _cfg: crate::perm::ConfigGuard,
        _home: HomeGuard,
        root: PathBuf,
        workspace: PathBuf,
    }

    fn iso(tag: &str) -> Iso {
        let root = scratch(tag);
        let cfg = root.join("cfg");
        let home = root.join("home");
        let workspace = root.join("ws");
        fs::create_dir_all(&cfg).unwrap();
        fs::create_dir_all(&home).unwrap();
        fs::create_dir_all(workspace.join(".git")).unwrap();
        Iso {
            _cfg: crate::perm::ConfigGuard::set(&cfg),
            _home: HomeGuard::set(&home),
            root,
            workspace,
        }
    }

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "gh-plugin-{tag}-{}-{}",
            std::process::id(),
            unique_suffix()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write_manifest(dir: &Path, folder: &str, value: Value) {
        let path = if folder.is_empty() {
            dir.join("plugin.json")
        } else {
            dir.join(folder).join("plugin.json")
        };
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, serde_json::to_string_pretty(&value).unwrap()).unwrap();
    }

    fn demo_manifest(name: &str, version: &str) -> Value {
        json!({
            "name": name,
            "version": version,
            "description": "Demo plugin",
            "skills": "skills",
            "hooks": "hooks/hooks.json",
            "mcpServers": ".mcp.json",
            "agents": "agents",
            "commands": "commands",
            "permissions": {"allow": ["Bash(*)"]}
        })
    }

    fn write_parts(dir: &Path, hook: &str) {
        let skill = dir.join("skills").join("ship");
        fs::create_dir_all(&skill).unwrap();
        fs::write(
            skill.join("SKILL.md"),
            "---\nname: ship\ndescription: Ship the change\n---\nShip body.\n",
        )
        .unwrap();
        let hooks = dir.join("hooks");
        fs::create_dir_all(&hooks).unwrap();
        fs::write(
            hooks.join("hooks.json"),
            serde_json::to_string(&json!({
                "hooks": {"PreToolUse": [{"matcher": "Bash", "hooks": [{"type": "command", "command": hook}]}]}
            }))
            .unwrap(),
        )
        .unwrap();
        fs::write(
            dir.join(".mcp.json"),
            serde_json::to_string(&json!({
                "mcpServers": {"files": {"command": "plugin-mcp", "args": ["--stdio"]}}
            }))
            .unwrap(),
        )
        .unwrap();
        let agents = dir.join("agents");
        fs::create_dir_all(&agents).unwrap();
        fs::write(
            agents.join("reviewer.md"),
            "---\nname: reviewer\ndescription: Reviews patches\n---\nReview the diff carefully.\n",
        )
        .unwrap();
        let commands = dir.join("commands");
        fs::create_dir_all(&commands).unwrap();
        fs::write(
            commands.join("ship.md"),
            "---\nname: ship\ndescription: Ship the change\n---\nShip with tests.\n",
        )
        .unwrap();
    }

    fn file_git_url(path: &Path) -> String {
        let abs = fs::canonicalize(path).unwrap();
        let text = abs.display().to_string().replace('\\', "/");
        let text = text.trim_start_matches("//?/").to_string();
        if text.starts_with('/') {
            format!("file://{text}")
        } else {
            format!("file:///{text}")
        }
    }

    fn git(dir: &Path, args: &[&str]) {
        let output = Command::new("git")
            .args(args)
            .current_dir(dir)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{args:?} {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn contributes(iso: &Iso) -> (bool, bool, bool, bool) {
        let skills = crate::skills::discover(&iso.workspace, None, &skill_dirs(&iso.workspace));
        let skill = skills.iter().any(|skill| skill.name == "ship");
        let hooks = crate::hooks::discover_hooks(&iso.workspace, None, &hook_paths(&iso.workspace));
        let hook = hooks.iter().any(|hook| hook.command == "echo plugin-hook");
        let mcp = crate::mcp::load_servers_for(&iso.workspace).contains_key("plugin__demo__files");
        let persona = persona_body(&iso.workspace, "reviewer")
            .is_some_and(|body| body.contains("Review the diff carefully."));
        (skill, hook, mcp, persona)
    }

    #[test]
    fn both_manifest_dirs_parse_name_version_and_components() {
        let iso = iso("both");
        let grok = iso.root.join("grok");
        let claude = iso.root.join("claude");
        fs::create_dir_all(&grok).unwrap();
        fs::create_dir_all(&claude).unwrap();
        write_manifest(&grok, ".grok-plugin", demo_manifest("from-grok", "1.2.3"));
        write_parts(&grok, "echo grok-hook");
        write_manifest(
            &claude,
            ".claude-plugin",
            demo_manifest("from-claude", "9.9.9"),
        );
        write_parts(&claude, "echo claude-hook");
        install_path(&grok).unwrap();
        install_path(&claude).unwrap();
        let listed = list_plugins(&iso.workspace);
        let grok = listed.iter().find(|info| info.name == "from-grok").unwrap();
        let claude = listed
            .iter()
            .find(|info| info.name == "from-claude")
            .unwrap();
        assert_eq!(grok.version, "1.2.3");
        assert_eq!(claude.version, "9.9.9");
        assert!(grok.summary.contains("skills:"));
        assert!(grok.summary.contains("echo grok-hook"));
        assert!(claude.summary.contains("echo claude-hook"));
        assert!(grok.summary.contains("agents: reviewer"));
        assert!(!grok.enabled && !grok.trusted);
        assert!(!claude.enabled && !claude.trusted);
        let _ = fs::remove_dir_all(&iso.root);
    }

    #[test]
    fn untrusted_and_disabled_plugins_contribute_nothing() {
        let iso = iso("off");
        let src = iso.root.join("src");
        fs::create_dir_all(&src).unwrap();
        write_manifest(&src, ".grok-plugin", demo_manifest("demo", "1.0.0"));
        write_parts(&src, "echo plugin-hook");
        install_path(&src).unwrap();
        assert_eq!(contributes(&iso), (false, false, false, false));
        let state = state_path();
        fs::write(
            &state,
            r#"{"entries":{"demo":{"enabled":true,"version":"1.0.0","hash":""}}}"#,
        )
        .unwrap();
        assert_eq!(contributes(&iso), (false, false, false, false));
        let saved = fs::read_to_string(state_path()).unwrap();
        assert!(!saved.contains("\"enabled\": true") && !saved.contains("\"enabled\":true"));
        assert!(enable_plugin(&iso.workspace, "demo").is_err());
        let _ = fs::remove_dir_all(&iso.root);
    }

    #[test]
    fn enable_and_disable_change_skill_hook_mcp_and_persona_discovery() {
        let iso = iso("toggle");
        let src = iso.root.join("src");
        fs::create_dir_all(&src).unwrap();
        write_manifest(&src, ".claude-plugin", demo_manifest("demo", "1.0.0"));
        write_parts(&src, "echo plugin-hook");
        install_path(&src).unwrap();
        trust_plugin(&iso.workspace, "demo").unwrap();
        let listed = list_plugins(&iso.workspace);
        let info = listed.iter().find(|info| info.name == "demo").unwrap();
        assert!(info.trusted && !info.enabled);
        assert!(info.summary.contains("echo plugin-hook"));
        assert!(info.summary.contains("plugin-mcp"));
        enable_plugin(&iso.workspace, "demo").unwrap();
        assert_eq!(contributes(&iso), (true, true, true, true));
        let prompt = crate::system_prompt("", &iso.workspace);
        assert!(prompt.contains("Ship with tests."), "{prompt}");
        assert!(prompt.contains("ship"));
        disable_plugin("demo").unwrap();
        assert_eq!(contributes(&iso), (false, false, false, false));
        assert!(!crate::system_prompt("", &iso.workspace).contains("Ship with tests."));
        enable_plugin(&iso.workspace, "demo").unwrap();
        assert_eq!(contributes(&iso), (true, true, true, true));
        let rules = iso.root.join("cfg").join("permission-rules.json");
        assert!(!rules.exists(), "a plugin must not write allow rules");
        let _ = fs::remove_dir_all(&iso.root);
    }

    #[test]
    fn changed_bundle_drops_trust_and_turns_off() {
        let iso = iso("hash");
        let src = iso.root.join("src");
        fs::create_dir_all(&src).unwrap();
        write_manifest(&src, ".grok-plugin", demo_manifest("demo", "1.0.0"));
        write_parts(&src, "echo plugin-hook");
        install_path(&src).unwrap();
        trust_plugin(&iso.workspace, "demo").unwrap();
        enable_plugin(&iso.workspace, "demo").unwrap();
        assert!(contributes(&iso).0);
        let skill = install_root()
            .join("demo")
            .join("skills")
            .join("ship")
            .join("SKILL.md");
        fs::write(
            &skill,
            "---\nname: ship\ndescription: changed\n---\nChanged body.\n",
        )
        .unwrap();
        assert_eq!(contributes(&iso), (false, false, false, false));
        let info = list_plugins(&iso.workspace)
            .into_iter()
            .find(|info| info.name == "demo")
            .unwrap();
        assert!(!info.trusted && !info.enabled);
        assert!(enable_plugin(&iso.workspace, "demo").is_err());
        trust_plugin(&iso.workspace, "demo").unwrap();
        enable_plugin(&iso.workspace, "demo").unwrap();
        assert!(contributes(&iso).0);
        let _ = fs::remove_dir_all(&iso.root);
    }

    #[cfg(unix)]
    #[test]
    fn same_size_edit_with_restored_mtime_still_drops_trust() {
        let iso = iso("stamp");
        let src = iso.root.join("src");
        fs::create_dir_all(&src).unwrap();
        write_manifest(&src, ".grok-plugin", demo_manifest("demo", "1.0.0"));
        write_parts(&src, "echo plugin-hook");
        install_path(&src).unwrap();
        trust_plugin(&iso.workspace, "demo").unwrap();
        enable_plugin(&iso.workspace, "demo").unwrap();
        assert_eq!(contributes(&iso), (true, true, true, true));
        let hooks = install_root().join("demo").join("hooks").join("hooks.json");
        let before = fs::read_to_string(&hooks).unwrap();
        let mtime = fs::metadata(&hooks).unwrap().modified().unwrap();
        let after = before.replace("echo plugin-hook", "echo plugin-hoox");
        assert_eq!(after.len(), before.len());
        fs::write(&hooks, &after).unwrap();
        File::options()
            .write(true)
            .open(&hooks)
            .unwrap()
            .set_modified(mtime)
            .unwrap();
        assert_eq!(fs::metadata(&hooks).unwrap().modified().unwrap(), mtime);
        assert_eq!(contributes(&iso), (false, false, false, false));
        let info = list_plugins(&iso.workspace)
            .into_iter()
            .find(|info| info.name == "demo")
            .unwrap();
        assert_eq!((info.trusted, info.enabled), (false, false));
        assert_eq!(
            enable_plugin(&iso.workspace, "demo").unwrap_err(),
            "trust demo 1.0.0 before enabling"
        );
        let _ = fs::remove_dir_all(&iso.root);
    }

    #[test]
    fn a_just_written_bundle_is_never_memoized() {
        let dir = scratch("racy");
        fs::write(dir.join("a.txt"), "one").unwrap();
        let (print, racy) = fingerprint(&dir).unwrap();
        assert!(racy);
        assert!(print.starts_with("f\na.txt\n3\n"), "{print}");
        let first = cached_hash(&dir).unwrap();
        assert_eq!(memo_get(&dir, &print), None);
        fs::write(dir.join("a.txt"), "two").unwrap();
        assert_ne!(cached_hash(&dir).unwrap(), first);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn bundle_with_a_credential_like_file_cannot_be_trusted() {
        let iso = iso("cred");
        let ext = iso
            .root
            .join("home")
            .join(".claude")
            .join("plugins")
            .join("ext");
        fs::create_dir_all(&ext).unwrap();
        write_manifest(&ext, ".claude-plugin", demo_manifest("ext", "1.0.0"));
        write_parts(&ext, "echo plugin-hook");
        fs::write(ext.join("tokenize.sh"), "echo hi\n").unwrap();
        assert_eq!(
            trust_plugin(&iso.workspace, "ext").unwrap_err(),
            "bundle contains a credential-like file (tokenize.sh), so it can't be trusted"
        );
        assert_eq!(
            enable_plugin(&iso.workspace, "ext").unwrap_err(),
            "trust ext before enabling"
        );
        let info = list_plugins(&iso.workspace)
            .into_iter()
            .find(|info| info.name == "ext")
            .unwrap();
        assert_eq!((info.trusted, info.enabled), (false, false));
        assert!(
            !state_path().exists()
                || !fs::read_to_string(state_path())
                    .unwrap()
                    .contains("\"ext\"")
        );
        let _ = fs::remove_dir_all(&iso.root);
    }

    #[test]
    fn install_local_path_copies_without_enabling_or_running_hooks() {
        let iso = iso("copy");
        let src = iso.root.join("src");
        fs::create_dir_all(&src).unwrap();
        write_manifest(&src, ".grok-plugin", demo_manifest("demo", "1.0.0"));
        let marker = iso.root.join("hook-ran");
        write_parts(&src, &format!("sh -c 'echo ran > {}'", marker.display()));
        fs::write(src.join(concat!("auth", ".json")), "SECRET-MARKER").unwrap();
        let info = install_path(&src).unwrap();
        assert_eq!(info.name, "demo");
        assert!(!info.enabled && !info.trusted);
        let dest = install_root().join("demo");
        assert!(dest.join(".grok-plugin").join("plugin.json").is_file());
        assert!(!dest.join(concat!("auth", ".json")).exists());
        assert!(!marker.exists());
        assert_eq!(contributes(&iso), (false, false, false, false));
        let _ = fs::remove_dir_all(&iso.root);
    }

    #[test]
    fn install_local_git_file_url_does_not_run_hooks() {
        let iso = iso("git");
        let repo = iso.root.join("repo");
        fs::create_dir_all(&repo).unwrap();
        write_manifest(&repo, ".grok-plugin", demo_manifest("demo", "2.0.0"));
        write_parts(&repo, "echo plugin-hook");
        git(&repo, &["init"]);
        git(&repo, &["config", "user.email", "plugin-test@example.com"]);
        git(&repo, &["config", "user.name", "Plugin Test"]);
        git(&repo, &["config", "commit.gpgsign", "false"]);
        let marker = iso.root.join("clone-hook-ran");
        let hook = repo.join(".git").join("hooks").join("post-checkout");
        fs::create_dir_all(hook.parent().unwrap()).unwrap();
        fs::write(
            &hook,
            format!("#!/bin/sh\necho ran > {}\n", marker.display()),
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(&hook).unwrap().permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&hook, perms).unwrap();
        }
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-m", "init", "--no-verify"]);
        let info = install_git(&file_git_url(&repo)).unwrap();
        assert_eq!(info.version, "2.0.0");
        assert!(!info.enabled && !info.trusted);
        let dest = install_root().join("demo");
        assert!(dest.join(".grok-plugin").join("plugin.json").is_file());
        assert!(!dest.join(".git").exists());
        assert!(!marker.exists());
        let _ = fs::remove_dir_all(&iso.root);
    }

    #[test]
    fn manifest_path_traversal_is_refused() {
        let iso = iso("trav");
        let outside = install_root().join("outside").join("escaped");
        fs::create_dir_all(&outside).unwrap();
        fs::write(
            outside.join("SKILL.md"),
            "---\nname: escaped\ndescription: outside\n---\nOutside.\n",
        )
        .unwrap();
        let src = iso.root.join("src");
        fs::create_dir_all(&src).unwrap();
        write_parts(&src, "echo plugin-hook");
        write_manifest(
            &src,
            ".claude-plugin",
            json!({
                "name": "demo",
                "version": "1.0.0",
                "description": "paths",
                "skills": ["../outside", "/tmp/escaped-skills", "skills"],
                "hooks": "../../etc/hooks.json",
                "mcpServers": "../mcp.json",
                "agents": "agents"
            }),
        );
        install_path(&src).unwrap();
        trust_plugin(&iso.workspace, "demo").unwrap();
        enable_plugin(&iso.workspace, "demo").unwrap();
        let skills = crate::skills::discover(&iso.workspace, None, &skill_dirs(&iso.workspace));
        assert!(skills.iter().any(|skill| skill.name == "ship"));
        assert!(skills.iter().all(|skill| skill.name != "escaped"));
        for dir in skill_dirs(&iso.workspace) {
            assert!(
                dir.starts_with(install_root().join("demo")),
                "{}",
                dir.display()
            );
        }
        let info = list_plugins(&iso.workspace)
            .into_iter()
            .find(|info| info.name == "demo")
            .unwrap();
        assert!(info.summary.contains("refused:"), "{info:?}");
        assert!(enable_plugin(&iso.workspace, "../demo").is_err());
        assert!(require_name("..").is_err());
        assert!(require_name("a/b").is_err());
        assert!(require_name(r"a\b").is_err());
        let bad = iso.root.join("bad");
        fs::create_dir_all(&bad).unwrap();
        write_manifest(
            &bad,
            ".grok-plugin",
            json!({"name": "../evil", "version": "1.0.0"}),
        );
        assert!(install_path(&bad).is_err());
        let _ = fs::remove_dir_all(&iso.root);
    }

    #[test]
    fn git_url_rejects_options_and_ext_transports() {
        assert!(validate_git_url("-c").is_err());
        assert!(validate_git_url("--upload-pack=touch").is_err());
        assert!(validate_git_url("ext::sh").is_err());
        assert!(validate_git_url("fd::3").is_err());
        assert!(validate_git_url("http://example.invalid/plugin.git").is_err());
        assert!(validate_git_url("https://example.invalid/plugin.git").is_ok());
        assert!(validate_git_url("ssh://git@example.invalid/plugin.git").is_ok());
        assert!(validate_git_url("git@example.invalid:plugin.git").is_ok());
        assert!(validate_git_url("file:///tmp/plugin").is_ok());
        assert!(fetch_marketplace("file:///tmp/index.json").is_err());
        assert!(fetch_marketplace("http://127.0.0.1/index.json").is_err());
        assert!(fetch_marketplace("-https://example.invalid/index.json").is_err());
    }

    #[test]
    fn marketplace_index_parses_from_a_fixture_without_installing() {
        let iso = iso("market");
        let fixture = iso.root.join("market.json");
        fs::write(
            &fixture,
            r#"{"name":"local","plugins":[
                {"name":"demo","version":"1.2.0","description":"A demo","source":{"url":"https://example.invalid/demo.git","ref":"main"}},
                {"name":"other","version":"0.1.0","description":"path","source":"./plugins/other"},
                {"name":"../bad","version":"1","description":"no","source":"https://example.invalid/bad.git"}
            ]}"#,
        )
        .unwrap();
        let entries = load_marketplace_file(&fixture).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].name, "demo");
        assert_eq!(entries[0].source, "https://example.invalid/demo.git");
        assert_eq!(entries[1].source, "./plugins/other");
        assert!(install_source(&entries[1].source).is_err());
        assert!(!install_root().join("demo").exists());
        assert!(list_plugins(&iso.workspace).is_empty());
        let _ = fs::remove_dir_all(&iso.root);
    }

    #[test]
    fn external_claude_and_grok_plugins_are_listed_not_enabled() {
        let iso = iso("ext");
        let claude = iso
            .root
            .join("home")
            .join(".claude")
            .join("plugins")
            .join("ext");
        let grok = iso
            .root
            .join("home")
            .join(".grok")
            .join("plugins")
            .join("grokext");
        fs::create_dir_all(&claude).unwrap();
        fs::create_dir_all(&grok).unwrap();
        write_manifest(&claude, ".claude-plugin", demo_manifest("ext", "3.0.0"));
        write_parts(&claude, "echo external-hook");
        write_manifest(&grok, ".grok-plugin", demo_manifest("grokext", "4.0.0"));
        write_parts(&grok, "echo grok-external");
        let secret = iso
            .root
            .join("home")
            .join(".grok")
            .join(concat!("auth", ".json"));
        fs::write(&secret, "SHOULD_NOT_APPEAR").unwrap();
        let listed = list_plugins(&iso.workspace);
        let blob = format!("{listed:?}");
        assert!(!blob.contains("SHOULD_NOT_APPEAR"));
        let ext = listed.iter().find(|info| info.name == "ext").unwrap();
        let grok = listed.iter().find(|info| info.name == "grokext").unwrap();
        assert!(!ext.installed && !ext.enabled && !ext.trusted);
        assert!(!grok.installed && !grok.enabled);
        assert_eq!(contributes(&iso), (false, false, false, false));
        assert!(!install_root().join("ext").exists());
        let _ = fs::remove_dir_all(&iso.root);
    }

    #[cfg(unix)]
    #[test]
    fn symlink_escape_is_refused() {
        let iso = iso("link");
        let src = iso.root.join("src");
        fs::create_dir_all(&src).unwrap();
        write_manifest(&src, ".grok-plugin", demo_manifest("demo", "1.0.0"));
        write_parts(&src, "echo plugin-hook");
        std::os::unix::fs::symlink("/tmp", src.join("escape")).unwrap();
        let err = install_path(&src).unwrap_err();
        assert!(err.contains("symlink"), "{err}");
        assert!(!install_root().join("demo").exists());
        let _ = fs::remove_dir_all(&iso.root);
    }

    #[test]
    fn remove_deletes_only_an_installed_bundle() {
        let iso = iso("rm");
        let src = iso.root.join("src");
        fs::create_dir_all(&src).unwrap();
        write_manifest(&src, ".grok-plugin", demo_manifest("demo", "1.0.0"));
        write_parts(&src, "echo plugin-hook");
        install_path(&src).unwrap();
        trust_plugin(&iso.workspace, "demo").unwrap();
        enable_plugin(&iso.workspace, "demo").unwrap();
        remove_plugin(&iso.workspace, "demo").unwrap();
        assert!(!install_root().join("demo").exists());
        assert_eq!(contributes(&iso), (false, false, false, false));
        assert!(remove_plugin(&iso.workspace, "../demo").is_err());
        let _ = fs::remove_dir_all(&iso.root);
    }
}
