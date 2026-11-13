//! Get to know you, on disk: the progress file, the three memory files and,
//! when AMR is on, the Personal rows. The desktop app and the cabin both call
//! this, so the two stay in sync. Drafts come from
//! `grokhub_core::onboard_identity`; nothing here guesses at the person.

use std::path::{Path, PathBuf};

use grokhub_core::amr::PersonalGroup;
use grokhub_core::onboard_identity::{
    self as oi, Answers, Drafts, Existing, Progress, Stage, Step,
};
use grokhub_core::openclaw::{
    default_openclaw_paths, import_dest, import_memory_file, is_core, is_openclaw_workspace,
    merge_imported_memory,
};
use serde::Serialize;

/// The three files, in the order Review shows them.
pub const FILES: [&str; 3] = ["USER.md", "SOUL.md", "MEMORY.md"];

/// Bigger than this is not a notes file; it is neither read nor replaced.
const FILE_CAP: u64 = 1024 * 1024;

/// The most workspace files one import reads.
pub const IMPORT_MAX: usize = 64;

/// What Save did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Saved {
    /// The files whose text changed, by name.
    pub files: Vec<String>,
    /// The files left alone because their text could not be read: too big,
    /// not text, not a file, or no access. [`saved_note`] says why.
    pub skipped: Vec<String>,
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

/// Why a notes file's text can't be read. Review shows such a file empty,
/// and Save never replaces text it could not show.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Why {
    TooBig,
    NotText,
    NoAccess,
    Link,
    NotAFile,
}

impl Why {
    /// What follows the file's name in the note.
    fn says(self) -> &'static str {
        match self {
            Why::TooBig => "is too big to change here",
            Why::NotText => "is not plain text (UTF-8)",
            Why::NoAccess => "can't be opened",
            Why::Link => "is a link to another file",
            Why::NotAFile => "is not a file",
        }
    }
}

/// What is at a notes file's path.
enum Read {
    Missing,
    Text(String),
    Held(Why),
}

fn read_file(path: &Path) -> Read {
    use std::io::ErrorKind;
    let meta = match std::fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(e) if e.kind() == ErrorKind::NotFound => return Read::Missing,
        Err(_) => return Read::Held(Why::NoAccess),
    };
    if meta.file_type().is_symlink() {
        return Read::Held(Why::Link);
    }
    if !meta.is_file() {
        return Read::Held(Why::NotAFile);
    }
    if meta.len() > FILE_CAP {
        return Read::Held(Why::TooBig);
    }
    match std::fs::read_to_string(path) {
        Ok(text) => Read::Text(text),
        Err(e) if e.kind() == ErrorKind::InvalidData => Read::Held(Why::NotText),
        Err(e) if e.kind() == ErrorKind::NotFound => Read::Missing,
        Err(_) => Read::Held(Why::NoAccess),
    }
}

/// A file's text, or empty when it is missing or can't be read.
fn text_of(path: &Path) -> String {
    match read_file(path) {
        Read::Text(text) => text,
        Read::Missing | Read::Held(_) => String::new(),
    }
}

/// The files whose text can't be read, by name, and why.
fn held(config_dir: &Path) -> Vec<(&'static str, Why)> {
    let dir = memory_dir(config_dir);
    FILES
        .iter()
        .filter_map(|name| match read_file(&dir.join(name)) {
            Read::Held(why) => Some((*name, why)),
            Read::Missing | Read::Text(_) => None,
        })
        .collect()
}

/// What Review says under the drafts: one sentence per file it shows empty
/// because the text can't be read, and what Save does about it. `None` when
/// every file could be read.
pub fn review_note(config_dir: &Path) -> Option<String> {
    let lines: Vec<String> = held(config_dir)
        .into_iter()
        .map(|(name, why)| match why {
            Why::Link => format!(
                "{name} {}; Save won't run until its box is empty.",
                why.says()
            ),
            _ => format!("{name} {}; Save leaves it alone.", why.says()),
        })
        .collect();
    (!lines.is_empty()).then(|| lines.join(" "))
}

/// What to say after Save: one sentence per file it left alone, with why.
/// `None` when it skipped none.
pub fn saved_note(config_dir: &Path, saved: &Saved) -> Option<String> {
    let lines: Vec<String> = held(config_dir)
        .into_iter()
        .filter(|(name, _)| saved.skipped.iter().any(|s| s == name))
        .map(|(name, why)| format!("{name} {}; it was left alone.", why.says()))
        .collect();
    (!lines.is_empty()).then(|| lines.join(" "))
}

/// The three files as they are now.
pub fn existing(config_dir: &Path) -> Existing {
    let dir = memory_dir(config_dir);
    Existing {
        user: text_of(&dir.join("USER.md")),
        soul: text_of(&dir.join("SOUL.md")),
        memory: text_of(&dir.join("MEMORY.md")),
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

/// What [`write_file`] did with one file.
#[derive(PartialEq, Eq)]
enum Wrote {
    Changed,
    Same,
    /// The file's text could not be read, so it is not replaced.
    Held,
}

fn link_error(name: &str) -> String {
    format!("{name} {}; it was left alone.", Why::Link.says())
}

/// Write one memory file: never through a link, never over a file whose
/// text could not be read, the old text kept as `{name}.prev`, and only when
/// the text changed.
fn write_file(dir: &Path, name: &str, body: &str) -> Result<Wrote, String> {
    let path = dir.join(name);
    let before = match read_file(&path) {
        Read::Held(Why::Link) => return Err(link_error(name)),
        Read::Held(_) => return Ok(Wrote::Held),
        Read::Text(text) => Some(text),
        Read::Missing => None,
    };
    if before.as_deref() == Some(body) {
        return Ok(Wrote::Same);
    }
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    if before.is_some() {
        let _ = std::fs::copy(&path, dir.join(format!("{name}.prev")));
    }
    grokhub_core::pulse_writer::write_atomic(&path, body)?;
    Ok(Wrote::Changed)
}

/// Save what the person reviewed: the three files always, and the Personal
/// rows too when AMR is on. Then the stage is done, the typed answers are
/// cleared from the progress file, and the engine hears ids only.
pub fn commit(config_dir: &Path, drafts: &Drafts, now_ms: u64) -> Result<Saved, String> {
    let d = oi::reviewed(drafts);
    let dir = memory_dir(config_dir);
    let mut files = Vec::new();
    let mut skipped = Vec::new();
    let bodies: Vec<(&str, &String)> = FILES
        .into_iter()
        .zip([&d.user, &d.soul, &d.memory])
        .filter(|(_, body)| !body.trim().is_empty())
        .collect();
    // A link stops Save, so it is looked for before any file is written:
    // the files before it are not saved without it.
    for (name, _) in &bodies {
        if matches!(read_file(&dir.join(name)), Read::Held(Why::Link)) {
            return Err(link_error(name));
        }
    }
    for (name, body) in bodies {
        match write_file(&dir, name, body)? {
            Wrote::Changed => files.push(name.to_string()),
            Wrote::Held => skipped.push(name.to_string()),
            Wrote::Same => {}
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
    Ok(Saved {
        files,
        skipped,
        rows,
        amr,
    })
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

/// What an import added, and what it could not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Import {
    pub drafts: Drafts,
    /// What was left out and why, for the person.
    pub note: Option<String>,
}

/// Whether `slot` already holds `block` as whole lines.
fn has_block(slot: &str, block: &str) -> bool {
    let (slot, block) = (slot.trim(), block.trim());
    let line_end = |at: usize| slot.as_bytes().get(at).is_none_or(|b| *b == b'\n');
    slot.match_indices(block)
        .any(|(at, _)| (at == 0 || slot.as_bytes()[at - 1] == b'\n') && line_end(at + block.len()))
}

/// `drafts` with an OpenClaw workspace's files added for Review: each lands
/// under its own heading below what is there, so nothing is replaced, and a
/// block that is already there (as read, or as Save wrote it) is not added
/// again. `TOOLS.md` and files with secrets stay out. Only files whose name
/// can be imported are read, at most [`IMPORT_MAX`] of them (the workspace's
/// own files first), and a file that would take its slot past the size cap
/// is left out. `None` without a workspace.
pub fn openclaw_import(home: &Path, drafts: &Drafts) -> Option<Import> {
    let root = openclaw_workspace(home)?;
    let mut names: Vec<(String, PathBuf)> = std::fs::read_dir(&root)
        .ok()?
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .map(|e| (e.file_name().to_string_lossy().into_owned(), e.path()))
        .filter(|(name, _)| import_dest(name).is_some())
        .collect();
    names.sort();
    let found = names.len();
    let mut room = IMPORT_MAX.saturating_sub(names.iter().filter(|(n, _)| is_core(n)).count());
    names.retain(|(name, _)| {
        is_core(name) || {
            let fits = room > 0;
            room = room.saturating_sub(1);
            fits
        }
    });
    names.truncate(IMPORT_MAX);
    let mut out = drafts.clone();
    let mut added = 0;
    let mut already = 0;
    let mut full: Vec<(&str, usize)> = Vec::new();
    for (name, path) in &names {
        let Some((dest, content)) = import_memory_file(name, &text_of(path)) else {
            continue;
        };
        if content.is_empty() {
            continue;
        }
        let heading = format!("From OpenClaw {name}");
        let (slot, title, file) = match dest.as_str() {
            "USER.md" => (&mut out.user, "# User", FILES[0]),
            "SOUL.md" => (&mut out.soul, "# Soul", FILES[1]),
            _ => (&mut out.memory, "# Memory", FILES[2]),
        };
        let block = merge_imported_memory("", &content, &heading);
        let as_saved = oi::reviewed(&Drafts {
            memory: block.clone(),
            ..Drafts::default()
        })
        .memory;
        if has_block(slot, &block) || has_block(slot, &as_saved) {
            already += 1;
            continue;
        }
        let base = if oi::is_seed(slot) {
            title
        } else {
            slot.as_str()
        };
        let next = format!(
            "{}\n",
            merge_imported_memory(base, &content, &heading).trim_end()
        );
        if next.len() as u64 > FILE_CAP {
            match full.iter_mut().find(|(f, _)| *f == file) {
                Some((_, n)) => *n += 1,
                None => full.push((file, 1)),
            }
            continue;
        }
        *slot = next;
        added += 1;
    }
    let mut notes = Vec::new();
    if found > names.len() {
        notes.push(format!(
            "Only the first {IMPORT_MAX} notes files were read; {} were left out.",
            found - names.len()
        ));
    }
    for (file, n) in full {
        let files = match n {
            1 => "1 file was".to_string(),
            n => format!("{n} files were"),
        };
        notes.push(format!("{file} is nearly full; {files} left out."));
    }
    if notes.is_empty() && added == 0 {
        notes.push(if already > 0 {
            "The OpenClaw notes are already in these files; nothing was added.".to_string()
        } else {
            "The OpenClaw workspace has no notes to add.".to_string()
        });
    }
    Some(Import {
        drafts: out,
        note: (!notes.is_empty()).then(|| notes.join(" ")),
    })
}

#[cfg(test)]
#[path = "onboard_tests.rs"]
mod tests;
