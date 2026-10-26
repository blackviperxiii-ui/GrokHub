//! Settings → Playbooks (shell parity): the shipped playbooks with the
//! person's own on top, from the same `{config}/playbooks/` files the cabin
//! and the agent read. Save refuses a playbook that breaks the format or the
//! hard-class lock and says why.

use std::path::Path;

use grokhub_agent::brain::playbook::{self as pb, Loaded, Source};
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Row {
    pub id: String,
    pub name: String,
    pub summary: String,
    /// "shipped", "edited" (Reset puts the shipped one back) or "yours".
    pub source: &'static str,
    /// "Fits repair · linux", or "Fits any task".
    pub fits: String,
    /// "3 steps, up to Safe, reversible".
    pub steps: String,
    /// The TOML the editor shows.
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Page {
    pub rows: Vec<Row>,
    /// Files of yours that could not be used, each naming the file and why.
    pub problems: Vec<String>,
}

fn fits_line(l: &Loaded) -> String {
    let a = &l.playbook.applies_when;
    let parts: Vec<String> = [&a.intent, &a.domain, &a.os]
        .into_iter()
        .filter(|v| !v.is_empty())
        .map(|v| v.join(", "))
        .collect();
    if parts.is_empty() {
        "Fits any task".into()
    } else {
        format!("Fits {}", parts.join(" · "))
    }
}

fn steps_line(l: &Loaded) -> String {
    let tiers: Vec<_> = l
        .playbook
        .steps()
        .map(|(phase, s)| s.tier_in(phase))
        .collect();
    let n = tiers.len();
    let noun = if n == 1 { "step" } else { "steps" };
    match tiers.iter().max() {
        Some(t) => format!("{n} {noun}, up to {}", t.label()),
        None => format!("{n} {noun}"),
    }
}

fn row(l: Loaded) -> Row {
    Row {
        fits: fits_line(&l),
        steps: steps_line(&l),
        source: match l.source {
            Source::Shipped => "shipped",
            Source::Edited => "edited",
            Source::Yours => "yours",
        },
        id: l.playbook.id,
        name: l.playbook.name,
        summary: l.playbook.summary,
        text: l.text,
    }
}

pub fn load(config_dir: &Path) -> Page {
    let (all, problems) = pb::load(config_dir);
    Page {
        rows: all.into_iter().map(row).collect(),
        problems,
    }
}

/// Saves the editor's TOML; returns the playbook's name.
pub fn save(config_dir: &Path, text: &str) -> Result<String, String> {
    pb::save(config_dir, text)
        .map(|p| p.name)
        .map_err(|e| format!("Not saved: {e}"))
}

/// A copy of yours under a new id; returns that id.
pub fn duplicate(config_dir: &Path, id: &str) -> Result<String, String> {
    pb::duplicate(config_dir, id)
        .map(|p| p.id)
        .map_err(|e| format!("Couldn't duplicate: {e}"))
}

/// Removes your file: an edited shipped playbook goes back to the shipped
/// one, and one of your own is gone.
pub fn reset(config_dir: &Path, id: &str) -> Result<(), String> {
    pb::reset(config_dir, id).map_err(|e| format!("Couldn't reset: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("gh-shell-pb-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn find<'a>(page: &'a Page, id: &str) -> &'a Row {
        page.rows
            .iter()
            .find(|r| r.id == id)
            .unwrap_or_else(|| panic!("{id} not in the page"))
    }

    #[test]
    fn the_shipped_playbooks_list_with_what_they_fit() {
        let cfg = scratch("list");
        let page = load(&cfg);
        let ids: Vec<&str> = page.rows.iter().map(|r| r.id.as_str()).collect();
        for id in ["files", "research", "code-dev"] {
            assert!(ids.contains(&id), "{ids:?}");
        }
        let files = find(&page, "files");
        assert_eq!((files.name.as_str(), files.source), ("Files", "shipped"));
        assert_eq!(files.fits, "Fits task · files");
        assert_eq!(files.steps, "6 steps, up to Risky, asks first");
        assert!(files.text.contains("id = \"files\""));
        assert_eq!(page.problems, Vec::<String>::new());
        let _ = std::fs::remove_dir_all(&cfg);
    }

    #[test]
    fn duplicate_edit_then_remove_and_an_edited_shipped_one_resets() {
        let cfg = scratch("flow");
        let id = duplicate(&cfg, "files").expect("duplicate");
        assert_eq!(id, "files-copy");
        let page = load(&cfg);
        let copy = find(&page, &id);
        assert_eq!((copy.name.as_str(), copy.source), ("Files (copy)", "yours"));

        let edited = copy
            .text
            .replace("name = \"Files (copy)\"", "name = \"Tidy files\"");
        assert_eq!(save(&cfg, &edited), Ok("Tidy files".to_string()));
        assert_eq!(find(&load(&cfg), &id).name, "Tidy files");

        reset(&cfg, &id).expect("remove");
        assert!(load(&cfg).rows.iter().all(|r| r.id != id));

        let shipped = find(&load(&cfg), "files").text.clone();
        let mine = shipped.replace("name = \"Files\"", "name = \"My files\"");
        assert_eq!(save(&cfg, &mine), Ok("My files".to_string()));
        assert_eq!(find(&load(&cfg), "files").source, "edited");
        reset(&cfg, "files").expect("reset");
        let back = load(&cfg);
        assert_eq!(
            (
                find(&back, "files").name.as_str(),
                find(&back, "files").source
            ),
            ("Files", "shipped")
        );
        let _ = std::fs::remove_dir_all(&cfg);
    }

    #[test]
    fn bad_toml_and_resetting_a_shipped_one_are_refused_in_words() {
        let cfg = scratch("refuse");
        let err = save(&cfg, "id = ").unwrap_err();
        assert!(err.starts_with("Not saved: "), "{err}");
        assert_eq!(
            reset(&cfg, "files"),
            Err("Couldn't reset: `files` has no copy of yours to reset".to_string())
        );
        assert_eq!(
            duplicate(&cfg, "nope"),
            Err("Couldn't duplicate: no playbook `nope`".to_string())
        );
        let _ = std::fs::remove_dir_all(&cfg);
    }
}
