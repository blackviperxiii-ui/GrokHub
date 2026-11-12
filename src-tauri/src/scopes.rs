//! Settings → Permissions, "What Loadout can read": one row per learning
//! scope, every one off until the user allows it. The same consent ledger,
//! words and checks as the cabin's `scope_ui.rs`, so a scope allowed in one
//! app shows in the other.
//!
//! A grant is written only from Allow (the page sends it from a pointer click).
//! Revoke only takes access away. While private data is locked, nothing here
//! grants or revokes.

use std::path::Path;

use grokhub_agent::harness::{self as hx, ConsentLedger, UserClick};
use serde::Serialize;

/// Browsers the history row offers: (scope id, name the user reads).
pub const BROWSERS: &[(&str, &str)] = &[
    ("firefox", "Firefox"),
    ("chrome", "Chrome"),
    ("chromium", "Chromium"),
    ("edge", "Edge"),
    ("brave", "Brave"),
];

pub const CHOOSE_FOLDER: &str = "Choose folder…";
pub const ADD_FOLDER: &str = "Add a folder";
const ADD_BROWSER: &str = "Add a browser";
const PATH_HINT_CHARS: usize = 48;
pub const NO_FOLDER_DIALOG: &str =
    "No folder dialog opened on this computer. Type the folder's path instead.";

pub fn heading() -> String {
    format!("What {} can read", grokhub_core::brand::product_name())
}

pub fn note() -> &'static str {
    grokhub_core::brand::filled("Each is off until you allow it here. What you allow is read on this computer only, sealed, and never on battery or in quiet hours. Screen access is \"Let {product} control the desktop\" in Settings → Defaults.")
}

/// Placeholder in the folder field, in this OS's shape.
pub const fn folder_hint_for(os: hx::KeyringOs) -> &'static str {
    match os {
        hx::KeyringOs::Windows => "C:\\Users\\you\\Notes",
        hx::KeyringOs::MacOs => "/Users/you/Notes",
        hx::KeyringOs::Linux => "/home/you/Notes",
    }
}

fn no_reader(what: &str) -> String {
    format!(
        "Not read yet: {} has no {what} reader, so allowing it reads nothing today.",
        grokhub_core::brand::product_name()
    )
}

/// What one scope would read, for its row hint.
pub fn scope_reads(kind: &str) -> String {
    match kind {
        "files" => "A folder you pick. Never your whole home folder, keys or logins.".into(),
        "apps" => "Installed apps and how often you open them.".into(),
        "browser_history" => {
            "Pages you visited in one browser. Never cookies or saved logins.".into()
        }
        "calendar" => no_reader("calendar"),
        "mail" => no_reader("mail"),
        "system_state" => "Disk, services, logs and updates. Read only.".into(),
        _ => String::new(),
    }
}

/// The last part of a folder path. A root keeps its whole path.
pub fn folder_name(dir: &str) -> &str {
    let trimmed = dir.trim_end_matches(['/', '\\']);
    match trimmed.rsplit(['/', '\\']).next() {
        Some(name) if !name.is_empty() && !name.ends_with(':') => name,
        _ => dir,
    }
}

/// `text` cut to at most `max` characters with "…" in the middle, keeping
/// more of the end than the start.
pub fn middle_ellipsis(text: &str, max: usize) -> String {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= max || max < 3 {
        return text.to_string();
    }
    let keep = max - 1;
    let head = keep * 2 / 5;
    let tail = keep - head;
    let mut out: String = chars[..head].iter().collect();
    out.push('…');
    out.extend(&chars[chars.len() - tail..]);
    out
}

pub fn scope_label(source: &str) -> String {
    if let Some(dir) = source.strip_prefix("files:") {
        return format!("Files in {}", folder_name(dir));
    }
    if let Some(id) = source.strip_prefix("browser_history:") {
        let name = BROWSERS
            .iter()
            .find(|(b, _)| *b == id)
            .map(|(_, n)| *n)
            .unwrap_or(id);
        return format!("Browser history ({name})");
    }
    hx::SCOPE_KINDS
        .iter()
        .find(|(kind, _)| *kind == source)
        .map(|(_, label)| (*label).to_string())
        .unwrap_or_else(|| source.to_string())
}

/// A typed folder as the scope stores it: trimmed, no trailing slash.
pub fn folder_scope(text: &str) -> hx::Scope {
    let t = text.trim();
    let t = if t.len() > 1 {
        t.trim_end_matches(['/', '\\'])
    } else {
        t
    };
    hx::Scope::Files(t.to_string())
}

/// The short words on a disabled pill while private data is locked.
pub fn lock_hover(why: &hx::Locked) -> &'static str {
    match why {
        hx::Locked::Unavailable => "Private data is unavailable",
        hx::Locked::Missing => "Private data key is missing",
        hx::Locked::WrongKey => "Private data key doesn't match",
        hx::Locked::Busy => "Private data is being prepared",
        hx::Locked::Unwritable => "Private data can't be saved",
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Granted {
    pub id: String,
    pub title: String,
    pub hint: String,
    /// The whole folder path, for hover.
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ScopeRow {
    pub kind: String,
    pub granted: Vec<Granted>,
    /// The Allow row, when one shows: files and browsers always offer one
    /// more; the others only while off.
    pub add: Option<AddRow>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AddRow {
    pub title: String,
    pub hint: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ScopesView {
    pub heading: String,
    pub note: String,
    pub folder_hint: String,
    pub browsers: Vec<(String, String)>,
    pub rows: Vec<ScopeRow>,
    /// Said once at the top while locked; every pill is disabled.
    pub locked: Option<String>,
    pub lock_hover: Option<String>,
}

pub fn view(config_dir: &Path, now: u64) -> ScopesView {
    let ledger = ConsentLedger::load(config_dir);
    let rows = hx::SCOPE_KINDS
        .iter()
        .map(|(kind, label)| {
            let granted: Vec<Granted> = ledger
                .active()
                .filter(|g| g.destination.is_empty())
                .filter(|g| g.source == *kind || g.source.starts_with(&format!("{kind}:")))
                .map(|g| {
                    let since = grokhub_core::pulse::ago_label(g.granted_at, now);
                    let detail = g.source.strip_prefix("files:").map(str::to_string);
                    let rest = match &detail {
                        Some(dir) => middle_ellipsis(dir, PATH_HINT_CHARS),
                        None => scope_reads(kind),
                    };
                    Granted {
                        id: g.id.clone(),
                        title: scope_label(&g.source),
                        hint: format!("On since {since}. {rest}"),
                        detail,
                    }
                })
                .collect();
            let off = AddRow {
                title: (*label).to_string(),
                hint: format!("Off. {}", scope_reads(kind)),
            };
            let add = match (*kind, granted.is_empty()) {
                (_, true) => Some(off),
                ("files", false) => Some(AddRow {
                    title: ADD_FOLDER.into(),
                    hint:
                        "Pick or type another folder. Never your whole home folder, keys or logins."
                            .into(),
                }),
                ("browser_history", false) => Some(AddRow {
                    title: ADD_BROWSER.into(),
                    hint: "Pick another browser. Never cookies or saved logins.".into(),
                }),
                _ => None,
            };
            ScopeRow {
                kind: (*kind).to_string(),
                granted,
                add,
            }
        })
        .collect();
    let locked = ledger.locked();
    ScopesView {
        heading: heading(),
        note: note().to_string(),
        folder_hint: folder_hint_for(hx::KeyringOs::current()).to_string(),
        browsers: BROWSERS
            .iter()
            .map(|(id, n)| ((*id).to_string(), (*n).to_string()))
            .collect(),
        rows,
        locked: locked.map(hx::Locked::message),
        lock_hover: locked.map(|l| lock_hover(l).to_string()),
    }
}

/// What the page sends on Allow: the row's kind, plus the folder or browser.
pub fn scope_for(kind: &str, value: &str) -> Option<hx::Scope> {
    match kind {
        "files" => Some(folder_scope(value)),
        "browser_history" => {
            let id = BROWSERS
                .iter()
                .find(|(b, _)| *b == value)
                .map(|(b, _)| *b)
                .unwrap_or("firefox");
            Some(hx::Scope::BrowserHistory(id.to_string()))
        }
        _ => hx::Scope::parse(kind),
    }
}

/// The Allow click. The same checks and lines as the cabin's.
pub fn allow(config_dir: &Path, home: Option<&Path>, kind: &str, value: &str) -> String {
    let ledger = ConsentLedger::load(config_dir);
    if let Some(why) = ledger.locked() {
        return why.message();
    }
    let Some(scope) = scope_for(kind, value) else {
        return "Not allowed: that isn't a scope.".into();
    };
    let label = scope_label(&scope.key());
    if ledger.scope_grant(&scope).is_some() {
        return format!("{label} is already allowed.");
    }
    match hx::grant_scope(config_dir, &scope, home, UserClick::from_click()) {
        Ok(_) if grokhub_agent::indexers::has_reader(&scope) => format!(
            "{label} allowed. {} learns from it on this computer. Revoke it here any time.",
            grokhub_core::brand::product_name()
        ),
        Ok(_) => format!("{label} allowed. Nothing reads it yet. Revoke it here any time."),
        Err(e) => format!("Not allowed: {e}"),
    }
}

/// Revoke one scope grant. It only takes access away.
pub fn revoke(config_dir: &Path, id: &str) -> String {
    let ledger = ConsentLedger::load(config_dir);
    let label = ledger
        .active()
        .find(|g| g.id == id && g.destination.is_empty())
        .map(|g| scope_label(&g.source));
    let Some(label) = label else {
        return "That grant is already gone.".into();
    };
    match hx::revoke_grant(config_dir, id) {
        Ok(true) => format!("{label} revoked. It's off until you allow it again."),
        Ok(false) => "That grant is already gone.".into(),
        Err(e) => format!("Could not revoke: {e}"),
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Answer {
    pub view: ScopesView,
    pub status: String,
}

fn home_dir() -> Option<std::path::PathBuf> {
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
        .map(std::path::PathBuf::from)
}

#[tauri::command]
pub(crate) async fn get_scopes(
    state: tauri::State<'_, crate::ShellState>,
) -> Result<ScopesView, String> {
    let dir = state.config_dir.clone();
    crate::off_main(move || view(&dir, grokhub_core::now_ms())).await
}

#[tauri::command]
pub(crate) async fn allow_scope(
    state: tauri::State<'_, crate::ShellState>,
    kind: String,
    value: String,
) -> Result<Answer, String> {
    let dir = state.config_dir.clone();
    crate::off_main(move || {
        let status = allow(&dir, home_dir().as_deref(), &kind, &value);
        Answer {
            view: view(&dir, grokhub_core::now_ms()),
            status,
        }
    })
    .await
}

#[tauri::command]
pub(crate) async fn revoke_scope(
    state: tauri::State<'_, crate::ShellState>,
    id: String,
) -> Result<Answer, String> {
    let dir = state.config_dir.clone();
    crate::off_main(move || {
        let status = revoke(&dir, &id);
        Answer {
            view: view(&dir, grokhub_core::now_ms()),
            status,
        }
    })
    .await
}

/// "Choose folder…": the system folder dialog. Picking only fills the field;
/// the grant is still Allow. `None` is a cancel, or no dialog on this computer.
#[tauri::command]
pub(crate) async fn pick_scope_folder(app: tauri::AppHandle) -> Result<Option<String>, String> {
    #[cfg(desktop)]
    {
        use tauri_plugin_dialog::DialogExt;
        let title = format!(
            "Choose a folder {} may read",
            grokhub_core::brand::product_name()
        );
        let picked = crate::off_main(move || {
            app.dialog()
                .file()
                .set_title(title)
                .blocking_pick_folder()
                .and_then(|p| p.into_path().ok())
                .map(|p| p.display().to_string())
        })
        .await?;
        Ok(picked)
    }
    #[cfg(not(desktop))]
    {
        let _ = app;
        Ok(None)
    }
}

#[cfg(test)]
#[path = "scopes_tests.rs"]
mod tests;
