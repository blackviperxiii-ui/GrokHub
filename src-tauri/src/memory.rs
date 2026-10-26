//! Memory (cross-platform recode, Phase 3c): the same `MEMORY.md` notes the
//! cabin and the agent loop use. Global notes live in the GrokHub config
//! folder, project notes in the work folder. Secrets never reach the files.

use std::path::Path;

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
    /// The AMR store is on (the default); recall reads it before these files.
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
    /// Claims this line corrects, joined for the panel.
    pub corrects: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Hit {
    pub scope: String,
    pub body: String,
}

const LINKED: &str = "This MEMORY.md is a link to another file. GrokHub does not follow it.";

/// A link is refused both ways, as `remember` refuses to write through one,
/// so the editor never shows or replaces the file it points at.
fn read(path: &Path) -> Result<String, String> {
    match std::fs::symlink_metadata(path) {
        Err(_) => Ok(String::new()),
        Ok(meta) if meta.file_type().is_symlink() => Err(LINKED.into()),
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
        // AMR is the default; the legacy flag in app.json turns it off.
        assert!(notes.amr);
        let config = grokhub_agent::perm::config_dir();
        std::fs::write(config.join("app.json"), r#"{"memory_backend":"legacy"}"#).expect("flag");
        assert!(!load(&d.workspace).expect("load legacy").amr);

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

    #[cfg(unix)]
    #[test]
    fn a_linked_memory_file_is_not_shown_or_replaced() {
        let d = dirs("link");
        let target = d.workspace.join("secrets.json");
        std::fs::write(&target, "{\"token\":\"fake\"}").expect("target");
        std::os::unix::fs::symlink(&target, grokhub_agent::workspace_memory_path(&d.workspace))
            .expect("link");
        assert_eq!(load(&d.workspace).err().as_deref(), Some(LINKED));
        assert_eq!(
            save(&d.workspace, false, "overwritten\n", "")
                .err()
                .as_deref(),
            Some(LINKED)
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
