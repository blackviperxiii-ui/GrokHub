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
    std::fs::write(&path, body).map_err(|e| e.to_string())?;
    Ok((load(workspace)?, redacted))
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

fn trial() -> Result<Trial, String> {
    let config = grokhub_agent::perm::config_dir();
    let root = std::env::temp_dir().join(format!(
        "grokhub-amr-trial-{}-{}",
        std::process::id(),
        grokhub_core::now_ms()
    ));
    let amr = root.join("amr");
    let source = config.join("amr");
    if source.is_dir() {
        copy_tree(&source, &amr).map_err(|e| e.to_string())?;
    } else {
        std::fs::create_dir_all(&amr).map_err(|e| e.to_string())?;
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

/// Learned lines whose text a forget on `topic` would tombstone, plus the
/// MEMORY.md lines it would drop.
fn forget_hits(topic: &str) -> Result<(Vec<String>, Vec<String>), String> {
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
    let lines = before
        .lines()
        .filter(|l| !l.trim().is_empty() && !after.lines().any(|k| k == *l))
        .map(clip)
        .collect();
    Ok((learned, lines))
}

/// `/forget [topic]` without a yes: what would go.
pub fn forget_plan(topic: Option<&str>) -> Result<Plan, String> {
    let Some(topic) = topic.map(str::trim).filter(|t| !t.is_empty()) else {
        let lines = global_notes()?
            .lines()
            .filter(|l| !l.trim().is_empty())
            .count();
        if lines == 0 {
            return Ok(Plan::Nothing("MEMORY.md is already empty.".into()));
        }
        return Ok(Plan::Ask(format!(
            "Empty MEMORY.md? All {lines} {} go. Learned lines stay; /forget <topic> removes those.",
            if lines == 1 { "line" } else { "lines" }
        )));
    };
    let (learned, lines) = forget_hits(topic)?;
    if learned.is_empty() && lines.is_empty() {
        return Ok(Plan::Nothing(format!(
            "Nothing in memory mentions {topic}."
        )));
    }
    let mut all: Vec<String> = learned.iter().map(|l| format!("learned: {l}")).collect();
    all.extend(lines.iter().map(|l| format!("MEMORY.md: {l}")));
    Ok(Plan::Ask(format!(
        "Forget {topic}? These go:\n{}",
        listed(&all)
    )))
}

/// `/forget [topic] --yes`: the cabin's own pair of steps. Learned lines get a
/// tombstone and the topic's lines leave the global MEMORY.md; with no topic
/// the file is emptied.
pub fn forget(topic: Option<&str>) -> Result<String, String> {
    let path = grokhub_agent::global_memory_path();
    let Some(topic) = topic.map(str::trim).filter(|t| !t.is_empty()) else {
        read(&path)?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        std::fs::write(&path, "").map_err(|e| e.to_string())?;
        return Ok("Forgot MEMORY.md".into());
    };
    let mut line = format!("Forgot {topic}");
    if grokhub_agent::amr_enabled() {
        let store = open_store()?;
        let done = store.forget_matching(topic).map_err(|e| e.to_string())?;
        let n = done.forgotten.len();
        line = format!(
            "Forgot {topic}: {n} {} in the memory repo",
            if n == 1 { "note" } else { "notes" }
        );
        if done.locked > 0 {
            line.push_str(&format!(", {} private not checked", done.locked));
        }
    }
    let before = read(&path)?;
    let after = grokhub_core::forget_topic(&before, topic);
    if after != before {
        std::fs::write(&path, after).map_err(|e| e.to_string())?;
    }
    Ok(line)
}

/// Memory retention in days, from `app.json` (the cabin's setting). 0 keeps
/// everything; the default is 90.
pub fn retention_days() -> u64 {
    let path = grokhub_agent::perm::config_dir().join("app.json");
    std::fs::read_to_string(path)
        .ok()
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        .and_then(|v| v.get("amr_retention_days").and_then(|n| n.as_u64()))
        .map_or(90, |n| n.min(3650))
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
}
