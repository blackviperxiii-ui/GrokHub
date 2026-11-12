//! Settings → Privacy & data → Storage, as the cabin's `storage_ui.rs` has it:
//! where your files go (Open, Change, Reset to default), one Open per folder,
//! where local models sit, and Bring them over from the folders older
//! versions saved into. The files folder is the same `folders.json` setting
//! the cabin reads. Change and Reset move nothing; Bring them over moves (same
//! drive) or copies (another drive, originals kept), and the page asks first.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use grokhub_core::user_files::{self, BringOver, UserFolder};
use serde::Serialize;

/// The files folder before a Change or Reset since the app started, so its
/// files can be brought along (the cabin keeps the same for its session).
static PREVIOUS_ROOT: Mutex<Option<PathBuf>> = Mutex::new(None);

fn previous_root() -> Option<PathBuf> {
    PREVIOUS_ROOT
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
}

fn remember_previous(before: PathBuf, now: &Path) {
    if before != now {
        *PREVIOUS_ROOT.lock().unwrap_or_else(|e| e.into_inner()) = Some(before);
    }
}

/// Where on-device models go: `~/GrokHub/models`.
pub fn models_dir(home: Option<&Path>) -> PathBuf {
    home.map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."))
        .join(grokhub_core::brand::INTERNAL_ID)
        .join("models")
}

/// Folders older versions saved into, each with the folder its files belong
/// in now. A Change or Reset adds the old files folder's own folders.
pub fn older_spots(home: Option<&Path>, previous: Option<&Path>) -> Vec<(PathBuf, UserFolder)> {
    let mut spots = Vec::new();
    if let Some(home) = home {
        spots.push((
            grokhub_core::screen_record::old_recordings_root(home),
            UserFolder::Recordings,
        ));
        spots.push((
            home.join(format!("{}-Work", grokhub_core::brand::INTERNAL_ID))
                .join("imagine"),
            UserFolder::Images,
        ));
    }
    if let Some(old) = previous {
        spots.extend(UserFolder::ALL.map(|kind| (old.join(kind.name()), kind)));
    }
    spots
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FolderRow {
    /// What Open sends back: a folder's name, or "models".
    pub key: String,
    pub line: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageView {
    pub files_line: String,
    pub can_reset: bool,
    pub folders: Vec<FolderRow>,
    pub models: FolderRow,
    /// "3 older items are still in …", when any wait.
    pub waiting: Option<String>,
    /// What Bring them over would do, named for the ask: from and to.
    pub moves: Vec<String>,
}

fn waiting_line(n: usize) -> Option<String> {
    let items = match n {
        0 => return None,
        1 => "1 older item is".to_string(),
        n => format!("{n} older items are"),
    };
    Some(format!(
        "{items} still in the folders earlier versions used. Bringing them over moves them on the same drive, or copies them and keeps the originals on another drive."
    ))
}

pub fn view(config: &Path, home: Option<&Path>, previous: Option<&Path>) -> StorageView {
    let root = user_files::files_root(config);
    let folders = UserFolder::ALL
        .iter()
        .map(|kind| FolderRow {
            key: kind.name().to_string(),
            line: format!("{}: {}", kind.name(), kind.what()),
        })
        .collect();
    let models = models_dir(home);
    let mut waiting = 0;
    let mut moves = Vec::new();
    for (from, kind) in older_spots(home, previous) {
        let to = user_files::folder_path(config, kind);
        let n = user_files::waiting_to_bring_over(&from, &to);
        if n > 0 {
            waiting += n;
            moves.push(format!("{} → {}", from.display(), to.display()));
        }
    }
    StorageView {
        files_line: format!(
            "Your files: {}. Exports, recordings, images and reports go here.",
            root.display()
        ),
        can_reset: user_files::files_root_is_custom(config)
            && root != user_files::default_files_root(config),
        folders,
        models: FolderRow {
            key: "models".into(),
            line: format!("Local models: {}", models.display()),
        },
        waiting: waiting_line(waiting),
        moves,
    }
}

/// Make `dir` the files folder. Files already saved stay where they are.
pub fn change(config: &Path, dir: &Path) -> String {
    let before = user_files::files_root(config);
    match user_files::set_files_root(config, dir) {
        Ok(()) => {
            remember_previous(before, dir);
            format!(
                "New files go to {}. Files already saved stay where they are.",
                dir.display()
            )
        }
        Err(problem) => format!("Can't use that folder: {problem}."),
    }
}

pub fn reset(config: &Path) -> String {
    let before = user_files::files_root(config);
    match user_files::reset_files_root(config) {
        Ok(()) => {
            let now = user_files::files_root(config);
            remember_previous(before, &now);
            format!("New files go to {} again.", now.display())
        }
        Err(e) => format!("Couldn't reset it: {}.", user_files::plain_io_error(&e)),
    }
}

/// Bring every older spot into its folder under the files folder.
pub fn bring_all_over(config: &Path, spots: &[(PathBuf, UserFolder)]) -> BringOver {
    let mut all = BringOver::default();
    for (from, kind) in spots {
        if !from.is_dir() {
            continue;
        }
        let to = match user_files::folder(config, *kind) {
            Ok(dir) => dir,
            Err(e) => {
                all.failed.push(format!(
                    "{}: {}",
                    kind.name(),
                    user_files::plain_io_error(&e)
                ));
                continue;
            }
        };
        let done = user_files::bring_over(from, &to);
        all.moved += done.moved;
        all.copied += done.copied;
        all.already += done.already;
        all.failed.extend(done.failed);
    }
    all
}

/// The folder Open on `key` opens, made first when missing.
pub fn folder_for(config: &Path, home: Option<&Path>, key: &str) -> Result<PathBuf, String> {
    match key {
        "files" => Ok(user_files::files_root(config)),
        "models" => Ok(models_dir(home)),
        _ => {
            let kind = UserFolder::ALL
                .into_iter()
                .find(|k| k.name() == key)
                .ok_or_else(|| format!("No folder called {key}."))?;
            user_files::folder(config, kind).map_err(|e| {
                format!(
                    "Couldn't open {}: {}",
                    kind.name(),
                    user_files::plain_io_error(&e)
                )
            })
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Answer {
    pub view: StorageView,
    pub status: String,
}

fn home() -> Option<PathBuf> {
    grokhub_core::paths::user_home()
}

fn answer(config: &Path, status: String) -> Answer {
    Answer {
        view: view(config, home().as_deref(), previous_root().as_deref()),
        status,
    }
}

#[tauri::command]
pub(crate) async fn get_storage(
    state: tauri::State<'_, crate::ShellState>,
) -> Result<StorageView, String> {
    let dir = state.config_dir.clone();
    crate::off_main(move || view(&dir, home().as_deref(), previous_root().as_deref())).await
}

/// Change…: the system folder picker. Nothing changes when it's closed.
#[tauri::command]
pub(crate) async fn storage_change(
    app: tauri::AppHandle,
    state: tauri::State<'_, crate::ShellState>,
) -> Result<Answer, String> {
    let dir = state.config_dir.clone();
    #[cfg(desktop)]
    {
        use tauri_plugin_dialog::DialogExt;
        let start = user_files::files_root(&dir);
        let picked = crate::off_main(move || {
            app.dialog()
                .file()
                .set_title("Where should your files go?")
                .set_directory(start)
                .blocking_pick_folder()
                .and_then(|p| p.into_path().ok())
        })
        .await?;
        crate::off_main(move || {
            let status = picked.map(|p| change(&dir, &p)).unwrap_or_default();
            answer(&dir, status)
        })
        .await
    }
    #[cfg(not(desktop))]
    {
        let _ = app;
        Ok(answer(&dir, String::new()))
    }
}

#[tauri::command]
pub(crate) async fn storage_reset(
    state: tauri::State<'_, crate::ShellState>,
) -> Result<Answer, String> {
    let dir = state.config_dir.clone();
    crate::off_main(move || {
        let status = reset(&dir);
        answer(&dir, status)
    })
    .await
}

#[tauri::command]
pub(crate) async fn storage_bring_over(
    state: tauri::State<'_, crate::ShellState>,
) -> Result<Answer, String> {
    let dir = state.config_dir.clone();
    crate::off_main(move || {
        let spots = older_spots(home().as_deref(), previous_root().as_deref());
        let status = bring_all_over(&dir, &spots).summary();
        answer(&dir, status)
    })
    .await
}

#[tauri::command]
pub(crate) async fn storage_open(
    state: tauri::State<'_, crate::ShellState>,
    key: String,
) -> Result<(), String> {
    crate::capabilities::needs(cfg!(desktop), "Opening a folder")?;
    let dir = state.config_dir.clone();
    crate::off_main(move || {
        let folder = folder_for(&dir, home().as_deref(), &key)?;
        crate::privacy::open_folder(&folder)
    })
    .await?
}

#[cfg(test)]
#[path = "storage_tests.rs"]
mod tests;
