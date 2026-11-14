//! Memory (cross-platform recode, Phase 3c): the same `MEMORY.md` notes the
//! cabin and the agent loop use. Global notes live in the GrokHub config
//! folder, project notes in the work folder. Secrets never reach the files.

use std::path::Path;

use grokhub_core::amr::PersonalGroup;
use serde::Serialize;

/// Bigger than this is not a notes file; the editor refuses it.
const NOTES_CAP: u64 = 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Notes {
    pub global: String,
    pub workspace: String,
    pub global_path: String,
    pub workspace_path: String,
    /// The cabin's AMR store is on, so these files are not what recall reads.
    pub amr: bool,
    /// Learned-memory writes. Off stops notes, proposals and edits.
    pub writes_on: bool,
    /// User-model rows, proposed ones included, newest first.
    pub rows: Vec<Row>,
}

/// One learned line for the memory panel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Row {
    pub id: String,
    pub text: String,
    /// Why this line is here, including the chat turn when it has one.
    pub source: String,
    /// When the line was written or last changed.
    pub updated: String,
    pub proposed: bool,
    /// Kept, but chat, the profile block and the nightly review skip it.
    pub paused: bool,
    /// Which Personal group it shows under, by its plain name.
    pub group: String,
    /// Claims this line corrects, joined for the panel.
    pub corrects: String,
}

/// The groups a line can be added to, as (key the page sends, name it shows).
/// "Waiting for your OK" is not one: only learning puts a line there.
pub const ADD_GROUPS: [(&str, PersonalGroup); 5] = [
    ("about", PersonalGroup::About),
    ("likes", PersonalGroup::Likes),
    ("routines", PersonalGroup::Routines),
    ("needs", PersonalGroup::Needs),
    ("thinking", PersonalGroup::Thinking),
];

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Hit {
    pub scope: String,
    pub body: String,
}

fn linked() -> String {
    format!(
        "This MEMORY.md is a link to another file. {} does not follow it.",
        grokhub_core::brand::product_name()
    )
}

/// A link is refused both ways, as `remember` refuses to write through one,
/// so the editor never shows or replaces the file it points at.
fn read(path: &Path) -> Result<String, String> {
    match std::fs::symlink_metadata(path) {
        Err(_) => Ok(String::new()),
        Ok(meta) if meta.file_type().is_symlink() => Err(linked()),
        Ok(meta) if meta.len() > NOTES_CAP => {
            Err(format!("{} is too large to edit here.", path.display()))
        }
        Ok(_) => std::fs::read_to_string(path).map_err(|e| e.to_string()),
    }
}

pub fn load(workspace: &Path) -> Result<Notes, String> {
    let global = grokhub_agent::global_memory_path();
    let local = grokhub_agent::workspace_memory_path(workspace);
    Ok(Notes {
        global: read(&global)?,
        workspace: read(&local)?,
        global_path: global.display().to_string(),
        workspace_path: local.display().to_string(),
        amr: grokhub_agent::amr_enabled(),
        writes_on: grokhub_agent::memory_writes_enabled(),
        rows: learned_rows(),
    })
}

fn learned_rows() -> Vec<Row> {
    let store = grokhub_agent::amr_store();
    let (rows, _) = grokhub_core::amr::memory_rows(&store);
    rows.into_iter()
        .map(|row| Row {
            group: row.group().label().to_string(),
            paused: row.paused,
            id: row.id,
            text: row.text,
            source: row.source.label(),
            updated: row.updated,
            proposed: row.proposed,
            corrects: row.beside.join("; "),
        })
        .collect()
}

fn open_store() -> Result<grokhub_core::amr::AmrStore, String> {
    let store = grokhub_agent::amr_store();
    store.init().map_err(|err| err.to_string())?;
    Ok(store)
}

/// Replace a learned line with the user's words.
pub fn edit_row(workspace: &Path, id: &str, text: &str) -> Result<Notes, String> {
    let store = open_store()?;
    grokhub_core::amr::edit_node(&store, id, text, grokhub_core::now_ms())
        .map_err(|err| err.to_string())?;
    load(workspace)
}

/// Forget a learned line because the user asked.
pub fn forget_row(workspace: &Path, id: &str) -> Result<Notes, String> {
    let store = open_store()?;
    grokhub_core::amr::forget_node(
        &store,
        id,
        grokhub_core::amr::UserForget::from_click(),
        grokhub_core::now_ms(),
    )
    .map_err(|err| err.to_string())?;
    load(workspace)
}

/// Activate a proposed line.
pub fn approve_row(workspace: &Path, id: &str) -> Result<Notes, String> {
    let store = open_store()?;
    grokhub_core::amr::approve_node(&store, id, grokhub_core::now_ms())
        .map_err(|err| err.to_string())?;
    load(workspace)
}

/// Drop a proposed line.
pub fn reject_row(workspace: &Path, id: &str) -> Result<Notes, String> {
    let store = open_store()?;
    grokhub_core::amr::reject_node(
        &store,
        id,
        grokhub_core::amr::UserForget::from_click(),
        grokhub_core::now_ms(),
    )
    .map_err(|err| err.to_string())?;
    load(workspace)
}

/// Keep a line but stop using it (`paused`), or use it again.
pub fn pause_row(workspace: &Path, id: &str, paused: bool) -> Result<Notes, String> {
    let store = open_store()?;
    grokhub_core::amr::pause_node(&store, id, paused).map_err(|err| err.to_string())?;
    load(workspace)
}

/// Add a line about the user in their own words, under one group.
pub fn add_fact(workspace: &Path, text: &str, group: &str) -> Result<Notes, String> {
    let Some((_, group)) = ADD_GROUPS.iter().find(|(key, _)| *key == group) else {
        return Err("Pick a group for the line.".into());
    };
    if text.trim().is_empty() {
        return Err("Write the line first.".into());
    }
    let store = open_store()?;
    grokhub_core::amr::add_fact(&store, text, *group, grokhub_core::now_ms())
        .map_err(|err| err.to_string())?;
    load(workspace)
}

/// The export file's name: dated, so a second export never replaces the first.
fn export_name(now_ms: u64) -> String {
    let day: String = grokhub_core::oauth::unix_ms_to_rfc3339(now_ms)
        .chars()
        .take(10)
        .collect();
    format!(
        "{} - what it knows about you {day}.md",
        grokhub_core::brand::product_name()
    )
}

/// Everything learned about the user, as one readable file in `dir`.
/// Returns where it went.
pub fn export_to(dir: &Path) -> Result<String, String> {
    let store = grokhub_agent::amr_store();
    let now = grokhub_core::now_ms();
    let text = grokhub_core::amr::personal_export(&store, now);
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let path = dir.join(export_name(now));
    std::fs::write(&path, text).map_err(|e| e.to_string())?;
    Ok(path.display().to_string())
}

/// What Wipe did, for the page's line.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Wiped {
    pub removed: usize,
    pub locked: usize,
    pub notes: Notes,
}

/// The notes files about the user that Wipe empties, beside the learned lines.
/// SOUL.md is how the app talks, not about the user, so it stays.
const WIPED_NOTES: [&str; 2] = ["USER.md", "MEMORY.md"];

/// Delete every learned line about the user for good, and empty the global
/// USER.md and MEMORY.md (and the `.prev` copy a save keeps). Project notes
/// in the work folder are the project's, so they stay.
pub fn wipe(workspace: &Path) -> Result<Wiped, String> {
    let store = open_store()?;
    let report =
        grokhub_core::amr::wipe_user_model(&store, grokhub_core::amr::UserForget::from_click())
            .map_err(|err| err.to_string())?;
    let dir = grokhub_agent::global_memory_path()
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_default();
    for name in WIPED_NOTES {
        let _ = std::fs::remove_file(dir.join(format!("{name}.prev")));
        let path = dir.join(name);
        if std::fs::symlink_metadata(&path).is_ok_and(|m| m.is_file()) {
            std::fs::write(&path, format!("# {name}\n")).map_err(|e| e.to_string())?;
        }
    }
    Ok(Wiped {
        removed: report.removed,
        locked: report.locked,
        notes: load(workspace)?,
    })
}

/// The memory off switch. On removes the key. Returns the notes as they read now.
pub fn set_writes(workspace: &Path, on: bool) -> Result<Notes, String> {
    grokhub_agent::set_memory_writes(on)?;
    load(workspace)
}

pub const CHANGED: &str =
    "This file changed since you opened it, maybe from chat. Your edit was not saved; the page now shows the new text.";

/// Saves one file as edited. `base` is the text the editor started from; if
/// the file no longer holds it (chat remembered something meanwhile), nothing
/// is written. Secrets are redacted first; the bool says so.
pub fn save(
    workspace: &Path,
    global: bool,
    text: &str,
    base: &str,
) -> Result<(Notes, bool), String> {
    let path = if global {
        grokhub_agent::global_memory_path()
    } else {
        grokhub_agent::workspace_memory_path(workspace)
    };
    if read(&path)? != base {
        return Err(CHANGED.into());
    }
    let stored = grokhub_core::redact_secrets(text);
    if !stored.is_empty() && !grokhub_core::is_plain_text(&stored) {
        return Err("Notes must be plain text.".into());
    }
    let redacted = stored != text;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let mut body = stored.trim_end().to_string();
    if !body.is_empty() {
        body.push('\n');
    }
    if body.len() as u64 > NOTES_CAP {
        return Err("Notes are limited to 1 MB. Nothing was saved.".into());
    }
    write_notes_file(&path, &body)?;
    Ok((load(workspace)?, redacted))
}

/// Write `body` to `path`, keeping `{name}.prev` when the current file fits the
/// notes cap — same safety as the cabin's `write_memory` (plain text already
/// checked by the caller; no symlink follow on the replace).
fn write_notes_file(path: &Path, body: &str) -> Result<(), String> {
    if path
        .symlink_metadata()
        .is_ok_and(|m| m.file_type().is_symlink())
    {
        return Err(linked());
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
        let prev = path.with_file_name(format!("{name}.prev"));
        if path.exists()
            && std::fs::metadata(path).map(|m| m.len()).unwrap_or(u64::MAX) <= NOTES_CAP
        {
            let _ = std::fs::copy(path, &prev);
        }
    }
    std::fs::write(path, body).map_err(|e| e.to_string())
}

/// Adds one line, like `/remember`. `global` files it in the global notes.
pub fn add(workspace: &Path, text: &str, global: bool) -> Result<String, String> {
    let line = if global {
        format!("global: {}", text.trim())
    } else {
        text.trim().to_string()
    };
    grokhub_agent::remember(workspace, &line)
}

pub fn search(workspace: &Path, query: &str) -> Result<Vec<Hit>, String> {
    if query.trim().is_empty() {
        return Ok(Vec::new());
    }
    Ok(grokhub_agent::search_memory(workspace, query)?
        .into_iter()
        .map(|hit| Hit {
            scope: hit.scope,
            body: hit.body,
        })
        .collect())
}

/// How many lines a recall or a preview shows before it says "and N more".
const SHOWN: usize = 8;

fn clip(text: &str) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= 100 {
        return flat;
    }
    let cut: String = flat.chars().take(99).collect();
    format!("{}…", cut.trim_end())
}

fn listed(lines: &[String]) -> String {
    let mut out: Vec<String> = lines.iter().take(SHOWN).map(|l| format!("- {l}")).collect();
    if lines.len() > SHOWN {
        out.push(format!("…and {} more", lines.len() - SHOWN));
    }
    out.join("\n")
}

/// `/recall <query>`: what the notes and the learned lines say about it,
/// through the same search the agent's own recall uses.
pub fn recall(workspace: &Path, query: &str) -> Result<String, String> {
    let query = query.trim();
    if query.is_empty() {
        return Err("Type what to recall after /recall.".into());
    }
    let hits = search(workspace, query)?;
    if hits.is_empty() {
        return Ok(format!("Nothing in memory matches {query}."));
    }
    let lines: Vec<String> = hits
        .iter()
        .map(|h| format!("{}: {}", h.scope, clip(&h.body)))
        .collect();
    Ok(format!("Recalled for {query}\n{}", listed(&lines)))
}

/// A throwaway copy of the learned-notes store, so a command can show what it
/// would remove by running for real on the copy. Gone when dropped.
struct Trial {
    root: std::path::PathBuf,
    store: grokhub_core::amr::AmrStore,
}

impl Drop for Trial {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn copy_tree(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        let target = to.join(entry.file_name());
        if kind.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else if kind.is_file() {
            std::fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

/// A trial folder's name starts with this. It sits beside `amr` in the
/// config folder, never inside it and never in the shared temp folder.
const TRIAL_PREFIX: &str = "amr-trial-";
/// A trial lives for milliseconds. One older than this was left by a run
/// that was killed mid-command.
const TRIAL_STALE: std::time::Duration = std::time::Duration::from_secs(600);

/// Removes the trial folders a killed run left in `config`.
fn sweep_trials(config: &Path, now: std::time::SystemTime) {
    let Ok(entries) = std::fs::read_dir(config) else {
        return;
    };
    for entry in entries.flatten() {
        let ours = entry
            .file_name()
            .to_string_lossy()
            .starts_with(TRIAL_PREFIX)
            && entry.file_type().is_ok_and(|kind| kind.is_dir());
        let stale = entry
            .metadata()
            .ok()
            .and_then(|meta| meta.modified().ok())
            .and_then(|at| now.duration_since(at).ok())
            .is_some_and(|age| age > TRIAL_STALE);
        if ours && stale {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
}

fn trial() -> Result<Trial, String> {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let config = grokhub_agent::perm::config_dir();
    std::fs::create_dir_all(&config).map_err(|e| e.to_string())?;
    sweep_trials(&config, std::time::SystemTime::now());
    let root = config.join(format!(
        "{TRIAL_PREFIX}{}-{}-{}",
        std::process::id(),
        grokhub_core::now_ms(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    // A new folder only the owner can open; a name already taken is an error.
    #[cfg(unix)]
    let folder = {
        let mut folder = std::fs::DirBuilder::new();
        std::os::unix::fs::DirBuilderExt::mode(&mut folder, 0o700);
        folder
    };
    #[cfg(not(unix))]
    let folder = std::fs::DirBuilder::new();
    folder.create(&root).map_err(|e| e.to_string())?;
    let amr = root.join("amr");
    let source = config.join("amr");
    let filled = if source.is_dir() {
        copy_tree(&source, &amr)
    } else {
        std::fs::create_dir_all(&amr)
    };
    if let Err(err) = filled {
        let _ = std::fs::remove_dir_all(&root);
        return Err(err.to_string());
    }
    let vault = grokhub_agent::harness::LearnedVault::new(&config);
    let store = grokhub_core::amr::AmrStore::at(amr).with_sealer(std::sync::Arc::new(vault));
    Ok(Trial { root, store })
}

/// What a destructive memory command answers before it changes anything.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Plan {
    /// Nothing would change; say so.
    Nothing(String),
    /// Ask with this text, listing what goes.
    Ask(String),
}

fn global_notes() -> Result<String, String> {
    read(&grokhub_agent::global_memory_path())
}

/// The project's own notes, where `/remember` files a line.
fn project_notes(workspace: &Path) -> Result<String, String> {
    read(&grokhub_agent::workspace_memory_path(workspace))
}

fn count_lines(text: &str) -> usize {
    text.lines().filter(|l| !l.trim().is_empty()).count()
}

fn lines_word(n: usize) -> &'static str {
    if n == 1 {
        "line"
    } else {
        "lines"
    }
}

/// `text` without the lines a forget on `topic` drops, and those lines.
/// Every other byte stays as it was (line endings, blank lines): the project
/// file is one the person also edits by hand.
fn without_topic(text: &str, topic: &str) -> (String, Vec<String>) {
    let mut kept = String::new();
    let mut gone = Vec::new();
    for line in text.split_inclusive('\n') {
        let bare = line.trim_end_matches(['\n', '\r']);
        if !bare.trim().is_empty() && grokhub_core::forget_topic(bare, topic).is_empty() {
            gone.push(clip(bare));
        } else {
            kept.push_str(line);
        }
    }
    (kept, gone)
}

/// What a forget on a topic would remove.
struct ForgetHits {
    /// Learned lines it would tombstone.
    learned: Vec<String>,
    /// Lines it would drop from the global MEMORY.md.
    global: Vec<String>,
    /// Lines it would drop from the project's MEMORY.md.
    project: Vec<String>,
}

/// Reads what `/forget <topic>` would remove without changing anything: the
/// learned lines come from running the forget on a throwaway copy.
fn forget_hits(workspace: &Path, topic: &str) -> Result<ForgetHits, String> {
    let mut learned = Vec::new();
    if grokhub_agent::amr_enabled() {
        let trial = trial()?;
        let gone = trial
            .store
            .forget_matching(topic)
            .map_err(|e| e.to_string())?;
        let (rows, _) = grokhub_core::amr::memory_rows(&grokhub_agent::amr_store());
        for id in &gone.forgotten {
            if let Some(row) = rows.iter().find(|r| &r.id == id) {
                learned.push(clip(&row.text));
            }
        }
    }
    let before = global_notes()?;
    let after = grokhub_core::forget_topic(&before, topic);
    let global = before
        .lines()
        .filter(|l| !l.trim().is_empty() && !after.lines().any(|k| k == *l))
        .map(clip)
        .collect();
    let (_, project) = without_topic(&project_notes(workspace)?, topic);
    Ok(ForgetHits {
        learned,
        global,
        project,
    })
}

/// `/forget [topic]` without a yes: what would go. With no topic only the
/// global MEMORY.md is emptied, as in the cabin, and the ask says the
/// project's notes stay.
pub fn forget_plan(workspace: &Path, topic: Option<&str>) -> Result<Plan, String> {
    let Some(topic) = topic.map(str::trim).filter(|t| !t.is_empty()) else {
        let lines = count_lines(&global_notes()?);
        let project = project_notes(workspace).map_or(0, |text| count_lines(&text));
        let stays = format!(
            "The project MEMORY.md ({project} {}) stays",
            lines_word(project)
        );
        if lines == 0 {
            let mut text = "MEMORY.md is already empty.".to_string();
            if project > 0 {
                text.push_str(&format!(" {stays}; /forget <topic> removes lines from it."));
            }
            return Ok(Plan::Nothing(text));
        }
        let mut ask = format!(
            "Empty MEMORY.md? All {lines} {} go. Learned lines stay; /forget <topic> removes those.",
            lines_word(lines)
        );
        if project > 0 {
            ask.push_str(&format!(" {stays} too."));
        }
        return Ok(Plan::Ask(ask));
    };
    let hits = forget_hits(workspace, topic)?;
    if hits.learned.is_empty() && hits.global.is_empty() && hits.project.is_empty() {
        return Ok(Plan::Nothing(format!(
            "Nothing in memory mentions {topic}."
        )));
    }
    let mut all: Vec<String> = hits
        .learned
        .iter()
        .map(|l| format!("learned: {l}"))
        .collect();
    all.extend(hits.global.iter().map(|l| format!("MEMORY.md: {l}")));
    all.extend(
        hits.project
            .iter()
            .map(|l| format!("project MEMORY.md: {l}")),
    );
    Ok(Plan::Ask(format!(
        "Forget {topic}? These go:\n{}",
        listed(&all)
    )))
}

/// `/forget [topic] --yes`. Learned lines get a tombstone and the topic's
/// lines leave the global MEMORY.md, the cabin's own pair of steps, and they
/// leave the project's MEMORY.md too, where `/remember` files a line. With
/// no topic the global file is emptied.
pub fn forget(workspace: &Path, topic: Option<&str>) -> Result<String, String> {
    let path = grokhub_agent::global_memory_path();
    let Some(topic) = topic.map(str::trim).filter(|t| !t.is_empty()) else {
        read(&path)?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        std::fs::write(&path, "").map_err(|e| e.to_string())?;
        return Ok("Forgot MEMORY.md".into());
    };
    // Both files are read before anything changes: one this can't read (a
    // link, not text) stops the command with nothing half done.
    let before = read(&path)?;
    let local = grokhub_agent::workspace_memory_path(workspace);
    let (local_after, local_gone) = without_topic(&read(&local)?, topic);
    let mut parts = Vec::new();
    if grokhub_agent::amr_enabled() {
        let store = open_store()?;
        let done = store.forget_matching(topic).map_err(|e| e.to_string())?;
        let n = done.forgotten.len();
        parts.push(format!(
            "{n} {} in the memory repo",
            if n == 1 { "note" } else { "notes" }
        ));
        if done.locked > 0 {
            parts.push(format!("{} private not checked", done.locked));
        }
    }
    let after = grokhub_core::forget_topic(&before, topic);
    if after != before {
        std::fs::write(&path, after).map_err(|e| e.to_string())?;
    }
    if !local_gone.is_empty() {
        std::fs::write(&local, local_after).map_err(|e| e.to_string())?;
        parts.push(format!(
            "{} {} in the project MEMORY.md",
            local_gone.len(),
            lines_word(local_gone.len())
        ));
    }
    Ok(if parts.is_empty() {
        format!("Forgot {topic}")
    } else {
        format!("Forgot {topic}: {}", parts.join(", "))
    })
}

/// Memory retention in days, from `app.json` (the cabin's setting). 0 keeps
/// everything; the default is 90. The cabin writes camelCase `amrRetentionDays`.
pub fn retention_days() -> u64 {
    retention_setting().days as u64
}

/// The cabin's Settings → Memory retention choices (days; 0 keeps everything).
pub const RETENTION_CHOICES: [u32; 6] = [30, 60, 90, 180, 365, 0];
/// The cabin's Settings → Auto-archive idle chats choices.
pub const ARCHIVE_CHOICES: [u32; 5] = [0, 7, 14, 30, 90];
const RETENTION_KEY: &str = "amrRetentionDays";
const ARCHIVE_KEY: &str = "chatArchiveDays";
const RAW_TIDY_KEY: &str = "memoryRawTidyDone";
const MEMORY_SEED: &str = "# Memory\n\nLong-term notes.\n";
const DEFAULT_RETENTION: u32 = 90;
const DEFAULT_ARCHIVE: u32 = 14;

fn app_u32(key: &str, snake: &str, default: u32) -> u32 {
    let path = grokhub_agent::perm::config_dir().join("app.json");
    let Ok(text) = std::fs::read_to_string(path) else {
        return default;
    };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) else {
        return default;
    };
    v.get(key)
        .or_else(|| v.get(snake))
        .and_then(|n| n.as_i64())
        .map(|n| n.clamp(0, 3650) as u32)
        .unwrap_or(default)
}

fn app_bool(key: &str, default: bool) -> bool {
    let path = grokhub_agent::perm::config_dir().join("app.json");
    std::fs::read_to_string(path)
        .ok()
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        .and_then(|v| v.get(key).and_then(|n| n.as_bool()))
        .unwrap_or(default)
}

fn patch_app(pairs: &[(&str, serde_json::Value)]) -> Result<(), String> {
    grokhub_core::app_json::patch(&grokhub_agent::perm::config_dir(), pairs)
}

fn retention_label(days: u32) -> String {
    if days == 0 {
        "Keep everything".into()
    } else {
        format!("{days} days")
    }
}

fn archive_label(days: u32) -> String {
    if days == 0 {
        "Off".into()
    } else {
        format!("{days} days")
    }
}

/// Settings → Behavior → Memory retention.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RetentionSetting {
    pub days: u32,
    pub label: String,
    pub choices: Vec<RetentionChoice>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RetentionChoice {
    pub days: u32,
    pub label: String,
}

pub fn retention_setting() -> RetentionSetting {
    let days = app_u32(RETENTION_KEY, "amr_retention_days", DEFAULT_RETENTION);
    RetentionSetting {
        days,
        label: retention_label(days),
        choices: RETENTION_CHOICES
            .iter()
            .map(|&d| RetentionChoice {
                days: d,
                label: retention_label(d),
            })
            .collect(),
    }
}

pub fn set_retention(days: u32) -> Result<RetentionSetting, String> {
    if !RETENTION_CHOICES.contains(&days) {
        return Err("Pick a Memory retention choice.".into());
    }
    patch_app(&[(RETENTION_KEY, days.into())])?;
    Ok(retention_setting())
}

/// Settings → Behavior → Auto-archive idle chats.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArchiveSetting {
    pub days: u32,
    pub label: String,
    pub choices: Vec<RetentionChoice>,
}

pub fn archive_setting() -> ArchiveSetting {
    let days = app_u32(ARCHIVE_KEY, "chat_archive_days", DEFAULT_ARCHIVE);
    ArchiveSetting {
        days,
        label: archive_label(days),
        choices: ARCHIVE_CHOICES
            .iter()
            .map(|&d| RetentionChoice {
                days: d,
                label: archive_label(d),
            })
            .collect(),
    }
}

pub fn set_archive(days: u32) -> Result<ArchiveSetting, String> {
    if !ARCHIVE_CHOICES.contains(&days) {
        return Err("Pick an auto-archive choice.".into());
    }
    patch_app(&[(ARCHIVE_KEY, days.into())])?;
    Ok(archive_setting())
}

fn memory_dir() -> std::path::PathBuf {
    grokhub_agent::global_memory_path()
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| grokhub_agent::perm::config_dir().join("memory"))
}

fn tab_path(name: &str) -> Result<std::path::PathBuf, String> {
    match name {
        "SOUL.md" | "USER.md" | "MEMORY.md" => Ok(memory_dir().join(name)),
        _ => Err("Pick SOUL.md, USER.md or MEMORY.md.".into()),
    }
}

/// SOUL.md, USER.md and MEMORY.md as they are on disk, each with when it
/// last changed (Unix ms, 0 when it isn't there), for what `/sync` and
/// `/inhabit` hand to a paired computer. A link or an oversized file is
/// refused, as the Memory page refuses it.
pub fn tab_files() -> Result<Vec<(&'static str, String, u64)>, String> {
    ["SOUL.md", "USER.md", "MEMORY.md"]
        .into_iter()
        .map(|name| {
            let path = tab_path(name)?;
            let body = read(&path)?;
            let at = std::fs::metadata(&path)
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map_or(0, |d| d.as_millis() as u64);
            Ok((name, body, at))
        })
        .collect()
}

/// The three Memory page tabs (global config folder), plus the one-time tidy offer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryFiles {
    pub soul: String,
    pub user: String,
    pub memory: String,
    pub soul_path: String,
    pub user_path: String,
    pub memory_path: String,
    pub soul_prev: bool,
    pub user_prev: bool,
    pub memory_prev: bool,
    /// Old chat lines still in MEMORY.md, offered once for removal.
    pub raw_lines: Vec<String>,
    pub raw_tidy_done: bool,
}

fn prev_exists(name: &str) -> bool {
    let path = memory_dir().join(format!("{name}.prev"));
    std::fs::symlink_metadata(&path).is_ok_and(|m| m.is_file() && m.len() > 0)
}

pub fn load_files() -> Result<MemoryFiles, String> {
    let soul = tab_path("SOUL.md")?;
    let user = tab_path("USER.md")?;
    let memory = tab_path("MEMORY.md")?;
    let memory_body = read(&memory)?;
    let mut raw_tidy_done = app_bool(RAW_TIDY_KEY, false);
    let raw_lines = if raw_tidy_done {
        Vec::new()
    } else {
        grokhub_core::learn_notes::raw_chat_lines(&memory_body, MEMORY_SEED)
    };
    // Nothing from the old reflect: never offer again for lines the user writes.
    if raw_lines.is_empty() && !raw_tidy_done {
        let _ = patch_app(&[(RAW_TIDY_KEY, true.into())]);
        raw_tidy_done = true;
    }
    Ok(MemoryFiles {
        soul: read(&soul)?,
        user: read(&user)?,
        memory: memory_body,
        soul_path: soul.display().to_string(),
        user_path: user.display().to_string(),
        memory_path: memory.display().to_string(),
        soul_prev: prev_exists("SOUL.md"),
        user_prev: prev_exists("USER.md"),
        memory_prev: prev_exists("MEMORY.md"),
        raw_lines,
        raw_tidy_done,
    })
}

/// Save one Memory tab. `base` is the editor's starting text (stale-edit check).
pub fn save_file(name: &str, text: &str, base: &str) -> Result<(MemoryFiles, bool), String> {
    let path = tab_path(name)?;
    if read(&path)? != base {
        return Err(CHANGED.into());
    }
    let stored = grokhub_core::redact_secrets(text);
    if !stored.is_empty() && !grokhub_core::is_plain_text(&stored) {
        return Err("Notes must be plain text.".into());
    }
    let redacted = stored != text;
    let mut body = stored.trim_end().to_string();
    if !body.is_empty() {
        body.push('\n');
    }
    if body.len() as u64 > NOTES_CAP {
        return Err("Notes are limited to 1 MB. Nothing was saved.".into());
    }
    write_notes_file(&path, &body)?;
    Ok((load_files()?, redacted))
}

/// Restore `{name}.prev` over the current tab file.
pub fn restore_file(name: &str) -> Result<MemoryFiles, String> {
    let path = tab_path(name)?;
    let prev_path = memory_dir().join(format!("{name}.prev"));
    let prev = read(&prev_path)?;
    if prev.is_empty() {
        return Err(format!("no {name}.prev"));
    }
    // Restore replaces the current body; keep a .prev of what was open first.
    write_notes_file(&path, &prev)?;
    load_files()
}

/// Move `pulse:` ledger lines still in MEMORY.md into `memory/pulse-ledger.md`.
pub fn tidy_ledger() -> Result<MemoryFiles, String> {
    use grokhub_core::pulse::{merge_ledger, split_ledger, LEDGER_FILE};
    let memory_path = tab_path("MEMORY.md")?;
    let ledger_path = memory_dir().join(LEDGER_FILE);
    if memory_path
        .symlink_metadata()
        .ok()
        .is_some_and(|m| m.len() > NOTES_CAP)
        || ledger_path
            .symlink_metadata()
            .ok()
            .is_some_and(|m| m.len() > NOTES_CAP)
    {
        return load_files();
    }
    let memory = read(&memory_path)?;
    if !memory.contains("pulse:") {
        return load_files();
    }
    let (kept, moved) = split_ledger(&memory);
    if moved.is_empty() {
        return load_files();
    }
    let ledger = read(&ledger_path).unwrap_or_default();
    let mut merged = merge_ledger(&ledger, &moved);
    while merged.len() as u64 > NOTES_CAP {
        match merged.find('\n') {
            Some(i) => {
                merged.drain(..=i);
            }
            None => merged.clear(),
        }
    }
    write_notes_file(&ledger_path, &merged)?;
    write_notes_file(&memory_path, &kept)?;
    load_files()
}

/// Drop the one-time raw-chat tidy offer without changing MEMORY.md.
pub fn dismiss_raw_tidy() -> Result<MemoryFiles, String> {
    patch_app(&[(RAW_TIDY_KEY, true.into())])?;
    load_files()
}

/// Remove the offered raw chat lines from MEMORY.md (keeps `.prev`).
pub fn apply_raw_tidy() -> Result<MemoryFiles, String> {
    let path = tab_path("MEMORY.md")?;
    if path
        .symlink_metadata()
        .ok()
        .is_some_and(|m| m.len() > NOTES_CAP)
    {
        return Err("MEMORY.md is too large to tidy here. Nothing was changed.".into());
    }
    let body = read(&path)?;
    let lines = grokhub_core::learn_notes::raw_chat_lines(&body, MEMORY_SEED);
    patch_app(&[(RAW_TIDY_KEY, true.into())])?;
    if lines.is_empty() {
        return load_files();
    }
    let next = grokhub_core::learn_notes::without_lines(&body, &lines);
    write_notes_file(&path, &next)?;
    load_files()
}

/// What Reflect wrote.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReflectResult {
    pub status: String,
    pub diff: String,
    pub files: MemoryFiles,
}

/// Reflect over recent user lines in `chat_id` (or the last open chat): durable
/// facts become AMR nodes when the memory repo is on, else learn_notes lines.
pub fn reflect(config_dir: &Path, chat_id: Option<&str>) -> Result<ReflectResult, String> {
    if !grokhub_agent::memory_writes_enabled() {
        return Ok(ReflectResult {
            status: "Not learning from chats. Nothing new is saved.".into(),
            diff: String::new(),
            files: load_files()?,
        });
    }
    let id = chat_id
        .map(str::to_string)
        .filter(|s| !s.is_empty())
        .or_else(|| {
            let chat = crate::settings::load(config_dir).last_chat;
            (!chat.is_empty()).then_some(chat)
        });
    if id
        .as_deref()
        .is_some_and(grokhub_agent::scratch::is_scratch)
    {
        return Ok(ReflectResult {
            status: "Scratch — no reflect".into(),
            diff: String::new(),
            files: load_files()?,
        });
    }
    let pairs: Vec<(String, String)> = match &id {
        Some(id) => crate::history::saved(config_dir, id)
            .map(|info| grokhub_agent::transcript_pairs(&info))
            .unwrap_or_default(),
        None => Vec::new(),
    };
    let facts = grokhub_core::reflect::fact_candidates(&pairs);
    if facts.is_empty() {
        return Ok(ReflectResult {
            status: "Reflect: nothing new".into(),
            diff: String::new(),
            files: load_files()?,
        });
    }
    if grokhub_agent::amr_enabled() {
        let store = open_store()?;
        let now = grokhub_core::now_ms();
        let mut added = Vec::new();
        for fact in &facts {
            let write = grokhub_core::amr::LineWrite {
                text: fact,
                source: "reflect",
                tags: Vec::new(),
                confidence: 0.7,
                revive: false,
            };
            if let Ok(grokhub_core::amr::Remembered::New(_)) =
                grokhub_core::amr::remember_line(&store, &write, now)
            {
                added.push(format!("+ {fact}"));
            }
        }
        let diff = added.join("\n");
        let status = if diff.is_empty() {
            "Reflect: nothing new".into()
        } else {
            format!(
                "Reflect: {} new {}",
                added.len(),
                if added.len() == 1 { "note" } else { "notes" }
            )
        };
        return Ok(ReflectResult {
            status,
            diff,
            files: load_files()?,
        });
    }
    let day: String = grokhub_core::oauth::unix_ms_to_rfc3339(grokhub_core::now_ms())
        .chars()
        .take(10)
        .collect();
    let notes: Vec<_> = facts
        .iter()
        .filter_map(|m| grokhub_core::learn_notes::note_from_message(m))
        .collect();
    if notes.is_empty() {
        return Ok(ReflectResult {
            status: "Reflect: nothing new".into(),
            diff: String::new(),
            files: load_files()?,
        });
    }
    let mut diffs = Vec::new();
    for file in [
        grokhub_core::learn_notes::NoteFile::User,
        grokhub_core::learn_notes::NoteFile::Soul,
        grokhub_core::learn_notes::NoteFile::Memory,
    ] {
        if !notes.iter().any(|n| n.file == file) {
            continue;
        }
        let name = file.file_name();
        let path = tab_path(name)?;
        let current = read(&path)?;
        let edit = grokhub_core::learn_notes::apply_notes(&current, file, &notes, &day);
        if edit.diff.is_empty() {
            continue;
        }
        write_notes_file(&path, &edit.next)?;
        diffs.push(edit.diff);
    }
    let diff = diffs.join("\n");
    Ok(ReflectResult {
        status: if diff.is_empty() {
            "Reflect: nothing new".into()
        } else {
            "Reflect: notes updated".into()
        },
        diff,
        files: load_files()?,
    })
}

const PRUNE_OFF: &str = "Memory retention runs in memory repo mode only.";

/// `/memory prune` without a yes: the notes retention would retire.
pub fn prune_plan() -> Result<Plan, String> {
    if !grokhub_agent::amr_enabled() {
        return Ok(Plan::Nothing(PRUNE_OFF.into()));
    }
    let trial = trial()?;
    let report = trial
        .store
        .prune_stale(
            grokhub_core::now_ms(),
            retention_days(),
            grokhub_core::amr::DREAM_STALE_BELOW,
        )
        .map_err(|e| e.to_string())?;
    if report.retired.is_empty() {
        return Ok(Plan::Nothing(report.status_line()));
    }
    let lines: Vec<String> = report
        .retired
        .iter()
        .map(|r| format!("{} ({})", clip(&r.quote), r.because))
        .collect();
    Ok(Plan::Ask(format!(
        "Retire {} unsure {}? They stay on disk, marked forgotten.\n{}",
        lines.len(),
        if lines.len() == 1 { "note" } else { "notes" },
        listed(&lines)
    )))
}

/// `/memory prune yes`: run retention now.
pub fn prune() -> Result<String, String> {
    if !grokhub_agent::amr_enabled() {
        return Ok(PRUNE_OFF.into());
    }
    let store = open_store()?;
    store
        .prune_stale(
            grokhub_core::now_ms(),
            retention_days(),
            grokhub_core::amr::DREAM_STALE_BELOW,
        )
        .map(|report| report.status_line())
        .map_err(|e| format!("Memory retention didn't run: {e}"))
}

/// `/memory dream`: the newest dream report under a "What changed" block.
pub fn dream_report(config_dir: &Path) -> String {
    use grokhub_core::what_changed as wc;
    match grokhub_core::amr::latest_dream(&config_dir.join("amr")) {
        Some((_, text)) => wc::with_block(&wc::from_dream(&text), &text),
        None => format!(
            "No dream yet. {} dreams once a night after the review, in memory repo mode.",
            grokhub_core::brand::product_name()
        ),
    }
}

/// `/dream`: one dream pass now over the learned notes (merge repeats, retire
/// unsure ones, propose missing preferences), then its report.
pub fn dream_now(config_dir: &Path) -> Result<String, String> {
    if !grokhub_agent::amr_enabled() {
        return Ok("Dreaming runs in memory repo mode only.".into());
    }
    let store = open_store()?;
    let days = retention_days();
    let opts = grokhub_core::amr::DreamOpts {
        ttl_days: if days == 0 { u64::MAX } else { days },
        user_md: std::fs::read_to_string(
            grokhub_agent::global_memory_path().with_file_name("USER.md"),
        )
        .ok(),
        ..Default::default()
    };
    let done = store
        .dream_once(grokhub_core::now_ms(), &opts)
        .map_err(|e| e.to_string())?;
    Ok(format!(
        "{}\n\n{}",
        done.status_line(),
        dream_report(config_dir)
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    struct Dirs {
        workspace: PathBuf,
        _guard: grokhub_agent::perm::ConfigGuard,
    }

    fn dirs(name: &str) -> Dirs {
        let root = std::env::temp_dir().join(format!(
            "grokhub-shell-mem-{name}-{}-{}",
            std::process::id(),
            grokhub_core::now_ms()
        ));
        let _ = std::fs::remove_dir_all(&root);
        let (config, workspace) = (root.join("cfg"), root.join("ws"));
        std::fs::create_dir_all(&config).expect("cfg");
        std::fs::create_dir_all(&workspace).expect("ws");
        Dirs {
            workspace,
            _guard: grokhub_agent::perm::ConfigGuard::set(&config),
        }
    }

    /// A preview (`/forget <topic>`, `/memory prune`) runs on a copy of the
    /// learned notes. The copy stays in the config folder, readable by the
    /// owner only, never in the temp folder every local account can list.
    #[cfg(unix)]
    #[test]
    fn a_preview_copies_the_learned_notes_inside_the_config_folder_for_the_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let _d = dirs("trial");
        let config = grokhub_agent::perm::config_dir();
        let store = grokhub_agent::amr_store();
        store.init().expect("init");
        grokhub_core::amr::add_fact(
            &store,
            "likes jazz",
            PersonalGroup::Likes,
            grokhub_core::now_ms(),
        )
        .expect("fact");

        let trial = trial().expect("trial");
        let root = trial.root.clone();
        assert_eq!(root.parent(), Some(config.as_path()));
        let name = root
            .file_name()
            .expect("name")
            .to_string_lossy()
            .into_owned();
        assert!(name.starts_with("amr-trial-"), "{name}");
        let mode = std::fs::metadata(&root).expect("meta").permissions().mode();
        assert_eq!(mode & 0o777, 0o700);
        assert!(root.join("amr").is_dir());
        drop(trial);
        assert!(!root.exists());
    }

    #[test]
    fn a_trial_folder_a_killed_run_left_is_swept_and_a_fresh_one_is_kept() {
        let _d = dirs("sweep");
        let config = grokhub_agent::perm::config_dir();
        let left = config.join("amr-trial-1-1-0");
        std::fs::create_dir_all(left.join("amr")).expect("left");
        let learned = config.join("amr");
        std::fs::create_dir_all(&learned).expect("amr");

        // Another copy of the app may be mid-preview: a fresh folder stays.
        let now = std::time::SystemTime::now();
        sweep_trials(&config, now);
        assert!(left.is_dir());
        sweep_trials(&config, now + std::time::Duration::from_secs(601));
        assert!(!left.exists());
        assert!(learned.is_dir());
    }

    #[test]
    fn forgetting_a_topic_in_the_project_notes_keeps_every_other_byte() {
        assert_eq!(
            without_topic(
                "# Notes\r\n\r\nthe ferry leaves at nine\r\ntea at four",
                "ferry"
            ),
            (
                "# Notes\r\n\r\ntea at four".to_string(),
                vec!["the ferry leaves at nine".to_string()]
            )
        );
        assert_eq!(
            without_topic("tea at four\n", "ferry"),
            ("tea at four\n".to_string(), Vec::new())
        );
    }

    #[test]
    fn notes_added_here_show_in_the_files_and_in_search() {
        let d = dirs("add");
        assert_eq!(
            add(&d.workspace, "the build needs node 22", false),
            Ok("Remembered in workspace MEMORY.md".to_string())
        );
        assert_eq!(
            add(&d.workspace, "I like short answers", true),
            Ok("Remembered in global MEMORY.md".to_string())
        );
        let notes = load(&d.workspace).expect("load");
        assert_eq!(notes.workspace, "the build needs node 22\n");
        assert_eq!(notes.global, "I like short answers\n");
        assert!(notes.global_path.ends_with("MEMORY.md"));
        // Empty config dir (no app.json) is a new install: AMR on by default.
        assert!(notes.amr);

        let hits = search(&d.workspace, "node").expect("search");
        assert_eq!(
            hits,
            vec![Hit {
                scope: "workspace".into(),
                body: "the build needs node 22".into()
            }]
        );
        assert_eq!(search(&d.workspace, "  ").expect("blank"), Vec::new());
    }

    #[test]
    fn an_edit_is_saved_and_found_and_a_secret_is_redacted() {
        let d = dirs("edit");
        let (notes, redacted) = save(
            &d.workspace,
            false,
            "deploys go out on fridays\ntoken sk-abcdefghijklmnopqrstuv\n\n",
            "",
        )
        .expect("save");
        assert!(redacted);
        assert_eq!(
            notes.workspace,
            "deploys go out on fridays\ntoken [redacted]\n"
        );
        // The index keeps paragraphs, so the two lines come back as one hit.
        assert_eq!(
            search(&d.workspace, "fridays")
                .expect("search")
                .iter()
                .map(|h| h.body.as_str())
                .collect::<Vec<_>>(),
            vec!["deploys go out on fridays\ntoken [redacted]"]
        );

        let (cleared, redacted) = save(&d.workspace, false, "", &notes.workspace).expect("clear");
        assert!(!redacted);
        assert_eq!(cleared.workspace, "");
    }

    #[test]
    fn an_edit_over_a_note_chat_just_added_is_refused() {
        let d = dirs("stale");
        let opened = load(&d.workspace).expect("load").global;
        add(&d.workspace, "added from chat", true).expect("chat");
        assert_eq!(
            save(&d.workspace, true, "my edit\n", &opened)
                .err()
                .as_deref(),
            Some(CHANGED)
        );
        assert_eq!(
            load(&d.workspace).expect("reload").global,
            "added from chat\n"
        );
    }

    #[test]
    fn a_save_over_the_cap_writes_nothing() {
        let d = dirs("cap");
        let big = "a".repeat(NOTES_CAP as usize + 1);
        assert_eq!(
            save(&d.workspace, false, &big, "").err().as_deref(),
            Some("Notes are limited to 1 MB. Nothing was saved.")
        );
        assert!(!grokhub_agent::workspace_memory_path(&d.workspace).exists());
    }

    #[test]
    fn a_learned_row_shows_source_and_date_and_approve_activates_it() {
        let d = dirs("rows");
        let store = grokhub_agent::amr_store();
        store.init().unwrap();
        let noted = grokhub_core::amr::propose_line(
            &store,
            "Lunch is at noon on Friday",
            "chat:t-standup#2",
            1_791_374_400_000,
        )
        .unwrap();
        let id = noted.id().unwrap().to_string();
        let notes = load(&d.workspace).unwrap();
        assert!(notes.writes_on);
        assert_eq!(notes.rows.len(), 1);
        assert_eq!(notes.rows[0].text, "Lunch is at noon on Friday");
        assert_eq!(notes.rows[0].source, "you said it in chat (turn 2)");
        assert_eq!(
            notes.rows[0].updated,
            grokhub_core::oauth::unix_ms_to_rfc3339(1_791_374_400_000)
        );
        assert!(notes.rows[0].proposed);
        let notes = approve_row(&d.workspace, &id).unwrap();
        assert_eq!(notes.rows.len(), 1);
        assert!(!notes.rows[0].proposed);
        assert_eq!(notes.rows[0].text, "Lunch is at noon on Friday");
        let notes = set_writes(&d.workspace, false).unwrap();
        assert!(!notes.writes_on);
        assert_eq!(
            edit_row(&d.workspace, &id, "Lunch is at 1")
                .unwrap_err()
                .as_str(),
            "memory writes are off"
        );
        let notes = forget_row(&d.workspace, &id).unwrap();
        assert!(notes.rows.is_empty());
        assert!(!notes.writes_on);
    }

    #[test]
    fn personal_adds_pauses_exports_and_wipes_but_keeps_project_notes() {
        let d = dirs("personal");
        assert_eq!(
            add_fact(&d.workspace, "I play after the kids are in bed", "routines")
                .unwrap()
                .rows
                .iter()
                .map(|r| (r.text.as_str(), r.group.as_str(), r.paused))
                .collect::<Vec<_>>(),
            vec![("I play after the kids are in bed", "Your routines", false)]
        );
        assert!(add_fact(&d.workspace, "x", "waiting").is_err());
        assert!(add_fact(&d.workspace, "  ", "about").is_err());
        let id = load(&d.workspace).unwrap().rows[0].id.clone();
        assert!(pause_row(&d.workspace, &id, true).unwrap().rows[0].paused);

        let out = d.workspace.join("downloads");
        let path = export_to(&out).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("I play after the kids are in bed"), "{text}");
        assert!(path.ends_with(".md"), "{path}");

        save(&d.workspace, false, "the build needs node 22", "").unwrap();
        save(&d.workspace, true, "I like short answers", "").unwrap();
        let wiped = wipe(&d.workspace).unwrap();
        assert_eq!(wiped.removed, 1);
        assert!(wiped.notes.rows.is_empty());
        assert_eq!(wiped.notes.global, "# MEMORY.md\n");
        assert_eq!(wiped.notes.workspace, "the build needs node 22\n");
    }

    #[cfg(unix)]
    #[test]
    fn a_linked_memory_file_is_not_shown_or_replaced() {
        let d = dirs("link");
        let target = d.workspace.join("secrets.json");
        std::fs::write(&target, "{\"token\":\"fake\"}").expect("target");
        std::os::unix::fs::symlink(&target, grokhub_agent::workspace_memory_path(&d.workspace))
            .expect("link");
        assert_eq!(load(&d.workspace).err().as_deref(), Some(linked().as_str()));
        assert_eq!(
            save(&d.workspace, false, "overwritten\n", "")
                .err()
                .as_deref(),
            Some(linked().as_str())
        );
        assert_eq!(
            std::fs::read_to_string(&target).expect("target"),
            "{\"token\":\"fake\"}"
        );
    }

    #[test]
    fn a_blank_note_is_refused() {
        let d = dirs("blank");
        assert_eq!(
            add(&d.workspace, "   ", false),
            Err("Nothing to remember".to_string())
        );
    }

    #[test]
    fn each_tab_saves_its_own_file_with_the_stale_edit_check_and_a_prev_copy() {
        let _d = dirs("tabs");
        let (files, _) = save_file("SOUL.md", "Be brief.\n", "").unwrap();
        assert_eq!(files.soul, "Be brief.\n");
        let (files, _) = save_file("USER.md", "- Likes tea\n", "").unwrap();
        assert_eq!(files.user, "- Likes tea\n");
        let (files, _) = save_file("MEMORY.md", "- 2026-11-01: spare key\n", "").unwrap();
        assert_eq!(files.memory, "- 2026-11-01: spare key\n");
        // Second save of SOUL keeps a .prev of the first.
        let (files, _) = save_file("SOUL.md", "Be brief and kind.\n", "Be brief.\n").unwrap();
        assert_eq!(files.soul, "Be brief and kind.\n");
        assert!(files.soul_prev);
        assert_eq!(
            save_file("SOUL.md", "stale\n", "Be brief.\n")
                .unwrap_err()
                .as_str(),
            CHANGED
        );
        let restored = restore_file("SOUL.md").unwrap();
        assert_eq!(restored.soul, "Be brief.\n");
    }

    #[test]
    fn tidy_moves_fixture_ledger_lines_into_pulse_ledger() {
        let _d = dirs("tidy");
        let memory = "# Memory\n\nLong-term notes.\n- [2026-10-04] pulse: dismissed \"Standup\" reason=dismiss\n- 2026-10-05: the spare key is under the planter\n";
        save_file("MEMORY.md", memory, "").unwrap();
        let files = tidy_ledger().unwrap();
        assert_eq!(
            files.memory,
            "# Memory\n\nLong-term notes.\n- 2026-10-05: the spare key is under the planter\n"
        );
        let ledger = std::fs::read_to_string(memory_dir().join("pulse-ledger.md")).unwrap();
        assert!(
            ledger.contains("pulse: dismissed \"Standup\" reason=dismiss"),
            "{ledger}"
        );
        assert!(files.memory_prev, "the move keeps a copy");
    }

    #[test]
    fn retention_and_archive_round_trip_through_app_json() {
        let _d = dirs("retain");
        assert_eq!(retention_setting().days, 90);
        assert_eq!(archive_setting().days, 14);
        assert_eq!(set_retention(180).unwrap().days, 180);
        assert_eq!(set_archive(30).unwrap().days, 30);
        let text =
            std::fs::read_to_string(grokhub_agent::perm::config_dir().join("app.json")).unwrap();
        assert!(text.contains("\"amrRetentionDays\": 180"), "{text}");
        assert!(text.contains("\"chatArchiveDays\": 30"), "{text}");
        assert_eq!(retention_days(), 180);
        assert!(set_retention(11).is_err());
        assert!(set_archive(11).is_err());
    }

    #[test]
    fn reflect_with_no_chat_reports_nothing_new() {
        let _d = dirs("reflect");
        let done = reflect(&grokhub_agent::perm::config_dir(), None).unwrap();
        assert_eq!(done.status, "Reflect: nothing new");
        assert!(done.diff.is_empty());
    }

    #[test]
    fn reflect_refuses_a_scratch_chat() {
        let _d = dirs("reflect-scratch");
        let done = reflect(
            &grokhub_agent::perm::config_dir(),
            Some("shell-scratch-a-b"),
        )
        .unwrap();
        assert_eq!(done.status, "Scratch — no reflect");
        assert!(done.diff.is_empty());
        // Reflect with no chat named falls back to the last chat: a scratch
        // one is refused the same way.
        let dir = grokhub_agent::perm::config_dir();
        let mut saved = crate::settings::load(&dir);
        saved.last_chat = "shell-scratch-c-d".into();
        crate::settings::save(&dir, &saved).unwrap();
        let fallback = reflect(&dir, None).unwrap();
        assert_eq!(fallback.status, "Scratch — no reflect");
    }

    #[test]
    fn wipe_without_a_confirm_is_the_ui_s_job_and_restore_needs_a_prev() {
        let _d = dirs("restore-miss");
        assert_eq!(
            restore_file("USER.md").unwrap_err().as_str(),
            "no USER.md.prev"
        );
    }
}
