//! Get to know you, on disk: the progress file, the three memory files and,
//! when AMR is on, the Personal rows. The desktop app and the cabin both call
//! this, so the two stay in sync. Drafts come from
//! `grokhub_core::onboard_identity`; nothing here guesses at the person.

use std::path::{Path, PathBuf};

use grokhub_core::amr::PersonalGroup;
use grokhub_core::onboard_identity::{
    self as oi, Answers, Drafts, Existing, Progress, Stage, Step,
};
use grokhub_core::openclaw::{default_openclaw_paths, import_memory_file, is_openclaw_workspace};
use serde::Serialize;

/// The three files, in the order Review shows them.
pub const FILES: [&str; 3] = ["USER.md", "SOUL.md", "MEMORY.md"];

/// Bigger than this is not a notes file; it is neither read nor replaced.
const FILE_CAP: u64 = 1024 * 1024;

/// What Save did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Saved {
    /// The files whose text changed, by name.
    pub files: Vec<String>,
    /// Personal rows added (0 with AMR off).
    pub rows: usize,
    pub amr: bool,
}

fn memory_dir(config_dir: &Path) -> PathBuf {
    config_dir.join("memory")
}

fn progress_path(config_dir: &Path) -> PathBuf {
    config_dir.join(oi::PROGRESS_FILE)
}

/// A file's text, or empty when it is missing, a link or too big.
fn read_file(path: &Path) -> String {
    match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.is_file() && meta.len() <= FILE_CAP => {
            std::fs::read_to_string(path).unwrap_or_default()
        }
        _ => String::new(),
    }
}

/// The three files as they are now.
pub fn existing(config_dir: &Path) -> Existing {
    let dir = memory_dir(config_dir);
    Existing {
        user: read_file(&dir.join("USER.md")),
        soul: read_file(&dir.join("SOUL.md")),
        memory: read_file(&dir.join("MEMORY.md")),
    }
}

/// Where the person is. A missing or broken file starts over.
pub fn load(config_dir: &Path) -> Progress {
    std::fs::read_to_string(progress_path(config_dir))
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

/// Keep where the person is and what they typed, so a quit resumes there.
pub fn keep(config_dir: &Path, step: Step, answers: &Answers) -> Result<Progress, String> {
    let mut p = load(config_dir);
    p.step = step;
    p.answers = answers.clone();
    save_progress(config_dir, &p)?;
    Ok(p)
}

fn save_progress(config_dir: &Path, p: &Progress) -> Result<(), String> {
    let raw = serde_json::to_string_pretty(p).map_err(|e| e.to_string())?;
    grokhub_core::pulse_writer::write_atomic(&progress_path(config_dir), &raw)
}

/// AMR is on, so Save adds Personal rows too.
pub fn amr_on(config_dir: &Path) -> bool {
    crate::memory::amr_enabled_at(config_dir)
}

/// Whether the stage shows.
pub fn pending(config_dir: &Path) -> bool {
    oi::pending(&load(config_dir), &existing(config_dir))
}

/// The drafts Review shows for `answers`, from the files as they are now.
pub fn draft(config_dir: &Path, answers: &Answers, now_ms: u64) -> Drafts {
    oi::drafts(answers, &existing(config_dir), &day(now_ms))
}

fn day(now_ms: u64) -> String {
    grokhub_core::oauth::unix_ms_to_rfc3339(now_ms)
        .chars()
        .take(10)
        .collect()
}

fn group(key: &str) -> Option<PersonalGroup> {
    Some(match key {
        "about" => PersonalGroup::About,
        "likes" => PersonalGroup::Likes,
        "routines" => PersonalGroup::Routines,
        "needs" => PersonalGroup::Needs,
        "thinking" => PersonalGroup::Thinking,
        _ => return None,
    })
}

/// Write one memory file: never through a link, the old text kept as
/// `{name}.prev`, and only when the text changed. Returns whether it did.
fn write_file(dir: &Path, name: &str, body: &str) -> Result<bool, String> {
    let path = dir.join(name);
    if let Ok(meta) = std::fs::symlink_metadata(&path) {
        if meta.file_type().is_symlink() {
            return Err(format!(
                "{name} is a link to another file; it was left alone."
            ));
        }
    }
    let before = read_file(&path);
    if before == body {
        return Ok(false);
    }
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    if path.exists() {
        let _ = std::fs::copy(&path, dir.join(format!("{name}.prev")));
    }
    grokhub_core::pulse_writer::write_atomic(&path, body)?;
    Ok(true)
}

/// Save what the person reviewed: the three files always, and the Personal
/// rows too when AMR is on. Then the stage is done, the typed answers are
/// cleared from the progress file, and the engine hears ids only.
pub fn commit(config_dir: &Path, drafts: &Drafts, now_ms: u64) -> Result<Saved, String> {
    let d = oi::reviewed(drafts);
    let dir = memory_dir(config_dir);
    let mut files = Vec::new();
    for (name, body) in FILES.iter().zip([&d.user, &d.soul, &d.memory]) {
        if !body.trim().is_empty() && write_file(&dir, name, body)? {
            files.push((*name).to_string());
        }
    }
    let amr = crate::memory::amr_enabled_at(config_dir);
    let mut rows = 0;
    if amr && !d.rows.is_empty() {
        let store = crate::memory::amr_store_at(config_dir);
        store.init().map_err(|e| e.to_string())?;
        for row in &d.rows {
            let Some(g) = group(&row.group) else { continue };
            grokhub_core::amr::add_fact(&store, &row.text, g, now_ms).map_err(|e| e.to_string())?;
            rows += 1;
        }
    }
    let answers = load(config_dir).answers;
    save_progress(
        config_dir,
        &Progress {
            stage: Stage::Done,
            step: Step::Done,
            ..Progress::default()
        },
    )?;
    let interests: Vec<&str> = answers.interests.split([',', ';']).collect();
    grokhub_core::learn::hub::note_all(grokhub_core::learn::sources::onboard_saved(
        &answers.tone,
        interests,
        now_ms,
    ));
    Ok(Saved { files, rows, amr })
}

/// Skip for now: the stage stays closed until Settings opens it again.
pub fn skip(config_dir: &Path, now_ms: u64) -> Result<Progress, String> {
    let p = Progress {
        stage: Stage::Skipped,
        ..Progress::default()
    };
    save_progress(config_dir, &p)?;
    grokhub_core::learn::hub::note_all(grokhub_core::learn::sources::onboard_skipped(now_ms));
    Ok(p)
}

/// Settings → Personal → Get to know you: start again from the welcome. The
/// drafts start from the files as they are, so nothing already written goes.
pub fn reopen(config_dir: &Path) -> Result<Progress, String> {
    let p = Progress {
        reopened: true,
        ..Progress::default()
    };
    save_progress(config_dir, &p)?;
    Ok(p)
}

/// The first OpenClaw or Hermes workspace under `home`, if there is one.
pub fn openclaw_workspace(home: &Path) -> Option<PathBuf> {
    default_openclaw_paths(&home.to_string_lossy())
        .into_iter()
        .map(PathBuf::from)
        .find(|p| {
            let names: Vec<String> = std::fs::read_dir(p)
                .map(|rd| {
                    rd.flatten()
                        .map(|e| e.file_name().to_string_lossy().into_owned())
                        .collect()
                })
                .unwrap_or_default();
            let refs: Vec<&str> = names.iter().map(String::as_str).collect();
            is_openclaw_workspace(&refs)
        })
}

/// `drafts` with an OpenClaw workspace's files added for Review: each lands
/// under its own heading below what is there, so nothing is replaced.
/// `TOOLS.md` and files with secrets stay out. `None` without a workspace.
pub fn openclaw_preview(home: &Path, drafts: &Drafts) -> Option<Drafts> {
    let root = openclaw_workspace(home)?;
    let mut names: Vec<(String, PathBuf)> = std::fs::read_dir(&root)
        .ok()?
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .map(|e| (e.file_name().to_string_lossy().into_owned(), e.path()))
        .collect();
    names.sort();
    let mut out = drafts.clone();
    for (name, path) in names {
        let Some((dest, content)) = import_memory_file(&name, &read_file(&path)) else {
            continue;
        };
        let heading = format!("From OpenClaw {name}");
        let (slot, title) = match dest.as_str() {
            "USER.md" => (&mut out.user, "# User"),
            "SOUL.md" => (&mut out.soul, "# Soul"),
            _ => (&mut out.memory, "# Memory"),
        };
        let base = if oi::is_seed(slot) {
            title
        } else {
            slot.as_str()
        };
        *slot = format!(
            "{}\n",
            grokhub_core::openclaw::merge_imported_memory(base, &content, &heading).trim_end()
        );
    }
    Some(out)
}

#[cfg(test)]
#[path = "onboard_tests.rs"]
mod tests;
