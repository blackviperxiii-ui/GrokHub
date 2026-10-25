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
    /// The cabin's AMR store is on, so these files are not what recall reads.
    pub amr: bool,
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
    })
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
        assert!(!notes.amr);

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
