//! Mods 2 of 2: a mod's files, install, update and uninstall, the same as
//! the cabin's Mods page (`mods_ui/{install,drawer}.rs`). Every write waits
//! for a click on the sheet that lists each file to add, replace, delete or
//! restore. Downloads, hashing and unpacking run off the main thread, and the
//! game must be closed both when the plan is made and right before the first
//! write. The core does every write ([`grokhub_core::mods::install`]); the
//! anti-cheat rules judge each plan before the sheet shows it. Desktop only
//! (`capabilities::mods`).

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::time::SystemTime;

use grokhub_agent::mods_net::ModsNet;
use grokhub_core::game_profile::Profile;
use grokhub_core::mods::anticheat::Screen;
use grokhub_core::mods::install::{
    self as engine, Installer, Layout, Manifest, ModInfo, Plan, Removal,
};
use grokhub_core::mods::rail::{installed_mods, ModGame};
use grokhub_core::mods::{archive, managers, Download, Entry, ModFile, Request, Source};
use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

use crate::{capabilities, mods, ShellState};

/// Progress lines for the one install at a time ("Downloading x.zip…").
pub const JOB_EVENT: &str = "mods://job";

/// How many files the drawer lists, newest first as the source sends them.
pub const FILES_SHOWN: usize = 6;

/// Why the game can't be written now ("Close Skyrim first."), asked again
/// right before the first write.
pub type Running = Arc<dyn Fn() -> Option<String> + Send + Sync>;

/// What waits for the user's click.
#[derive(Clone)]
pub enum Sheet {
    Install {
        plan: Plan,
        update: bool,
        note: Option<String>,
        running: Running,
        staging: PathBuf,
    },
    Remove {
        game: String,
        key: String,
        name: String,
        removal: Removal,
        warn: Option<String>,
        running: Running,
    },
}

/// One titled list on the sheet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SheetList {
    pub title: String,
    pub rows: Vec<String>,
}

/// The sheet as the page shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SheetView {
    pub token: u64,
    pub title: String,
    pub primary: String,
    /// "Into <folder>" for an install.
    pub into: Option<String>,
    pub lists: Vec<SheetList>,
    /// Amber lines: offline only, no published hash, game updated since.
    pub notes: Vec<String>,
}

impl Sheet {
    pub fn view(&self, token: u64) -> SheetView {
        fn list(title: &str, rows: Vec<String>) -> Option<SheetList> {
            (!rows.is_empty()).then(|| SheetList {
                title: format!("{title} ({})", rows.len()),
                rows,
            })
        }
        match self {
            Sheet::Install {
                plan, update, note, ..
            } => {
                let verb = if *update { "Update" } else { "Install" };
                let primary = match (*update, plan.offline_only.is_some()) {
                    (true, _) => "Update",
                    (false, true) => "Install for offline play",
                    (false, false) => "Install",
                };
                let mut lists = Vec::new();
                if *update {
                    lists.push(SheetList {
                        title: "The installed version comes out first".into(),
                        rows: Vec::new(),
                    });
                }
                lists.extend(list("Adds", plan.adds().map(|f| f.to.clone()).collect()));
                lists.extend(list(
                    "Replaces (backed up first)",
                    plan.replaces().map(|f| f.to.clone()).collect(),
                ));
                SheetView {
                    token,
                    title: format!("{verb} {} {}?", plan.name, plan.version),
                    primary: primary.into(),
                    into: Some(format!("Into {}", plan.root.display())),
                    lists,
                    notes: plan.offline_only.iter().chain(note).cloned().collect(),
                }
            }
            Sheet::Remove {
                name,
                removal,
                warn,
                ..
            } => SheetView {
                token,
                title: format!("Uninstall {name}?"),
                primary: "Uninstall".into(),
                into: None,
                lists: [
                    list("Deletes", removal.deletes.clone()),
                    list("Puts back", removal.restores.clone()),
                    list("Keeps (you changed these since)", removal.kept.clone()),
                ]
                .into_iter()
                .flatten()
                .collect(),
                notes: warn.iter().cloned().collect(),
            },
        }
    }
}

/// A browser download the page waits to find in Downloads.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pickup {
    pub key: String,
    pub file_name: String,
    pub size: u64,
    pub since: SystemTime,
}

#[derive(Default)]
struct Work {
    /// The sheet waiting for a click, by token. One at a time.
    sheet: Option<(u64, Sheet)>,
    /// Set while a plan or a write runs.
    busy: bool,
    next: u64,
    pickup: Option<Pickup>,
    recovered: bool,
}

fn work() -> std::sync::MutexGuard<'static, Work> {
    static WORK: OnceLock<Mutex<Work>> = OnceLock::new();
    WORK.get_or_init(Default::default)
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
}

/// The mod's key in the records: `source:id`.
pub fn mod_key(source: Source, id: &str) -> String {
    format!("{}:{id}", source.id())
}

fn staging_for(config: &Path, key: &str) -> PathBuf {
    let slug: String = key
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    grokhub_core::mods::cache::mods_dir(config)
        .join("staging")
        .join(slug)
}

/// Roll back an install or uninstall a crash cut short, once per run.
/// Returns what it put right.
pub fn recover_once(config: &Path) -> Vec<String> {
    {
        let mut w = work();
        if w.recovered {
            return Vec::new();
        }
        w.recovered = true;
    }
    let lines = engine::recover(config);
    let _ = std::fs::remove_dir_all(grokhub_core::mods::cache::mods_dir(config).join("staging"));
    lines
}

// ------------------------------------------------------------ the target

/// Where a game's mods go on this PC.
#[derive(Clone)]
pub struct Target {
    pub layout: Layout,
    pub game_dir: PathBuf,
    pub root: PathBuf,
    pub build: Option<String>,
    pub running: Running,
}

/// Every process's name and command line, the same no-handle reads the
/// game guard uses.
fn processes_now() -> Vec<(String, String)> {
    #[cfg(target_os = "linux")]
    {
        grokhub_desktop::game_guard::processes().unwrap_or_default()
    }
    #[cfg(windows)]
    {
        grokhub_desktop::game_guard::process_names()
            .unwrap_or_default()
            .into_iter()
            .map(|(_, name)| (name, String::new()))
            .collect()
    }
    #[cfg(not(any(target_os = "linux", windows)))]
    {
        Vec::new()
    }
}

fn service_names() -> Vec<String> {
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    {
        grokhub_desktop::game_guard::service_names()
    }
    #[cfg(any(target_os = "android", target_os = "ios"))]
    {
        Vec::new()
    }
}

/// The game's exe names, lowercase, from its folder and one level down.
pub fn game_exes(dir: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let mut scan = |d: &Path| {
        for e in std::fs::read_dir(d).into_iter().flatten().flatten() {
            let n = e.file_name().to_string_lossy().to_ascii_lowercase();
            if n.ends_with(".exe") {
                out.push(n);
            }
        }
    };
    scan(dir);
    for sub in ["bin", "bin/x64", "x64"] {
        scan(&dir.join(sub));
    }
    out
}

/// Why a game's mods can't be written now: the game is running.
pub fn running_line(
    procs: &[(String, String)],
    name: &str,
    app_id: Option<u32>,
    game_dir: &Path,
    wow: bool,
) -> Option<String> {
    let close = || Some(format!("Close {name} first."));
    if wow {
        return grokhub_core::battlenet::wow_running(procs).and(close());
    }
    if let Some(id) = app_id {
        #[cfg(windows)]
        if grokhub_desktop::game_guard::steam_running_app() == Some(id) {
            return close();
        }
        if grokhub_core::game_companion::running_from_cmdlines(procs) == Some(id) {
            return close();
        }
    }
    let dir = game_dir.to_string_lossy().to_ascii_lowercase();
    let exes = game_exes(game_dir);
    let hit = procs.iter().any(|(n, cmdline)| {
        let n = n.to_ascii_lowercase();
        (!cmdline.is_empty() && cmdline.to_ascii_lowercase().contains(&dir))
            || exes
                .iter()
                .any(|e| *e == n || (n.len() >= 15 && e.starts_with(&n)))
    });
    // Minecraft runs as Java, named only by its launcher.
    let minecraft = app_id.is_none()
        && procs.iter().any(|(n, c)| {
            let n = n.to_ascii_lowercase();
            n.starts_with("minecraft") || c.contains(".minecraft")
        });
    (hit || minecraft).then(|| format!("Close {name} first."))
}

fn app_data() -> (Option<PathBuf>, Option<PathBuf>) {
    let roaming = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .or_else(|| grokhub_core::user_home().map(|h| h.join(".config")));
    let local = std::env::var_os("LOCALAPPDATA").map(PathBuf::from);
    (roaming, local)
}

/// The game's own folder on this PC.
fn game_dir(game: &ModGame, steam_root: Option<&Path>) -> Option<PathBuf> {
    let entry = game.entry;
    match (entry.steam_app_id, steam_root) {
        (Some(id), Some(root)) => grokhub_core::steam::install_dir(root, id),
        _ if entry.id == "wow" => game.installs.iter().find_map(|i| {
            grokhub_core::battlenet::wow_flavors(i)
                .first()
                .map(|f| f.dir(i))
        }),
        _ if entry.id == "minecraft" => grokhub_core::mods::rail::minecraft_dir(),
        _ => None,
    }
    .filter(|d| d.is_dir())
}

/// Where `layout` puts mods under the game's folder.
pub fn root_of(layout: Layout, game_dir: &Path) -> PathBuf {
    if layout == Layout::Data {
        game_dir.join("Data")
    } else {
        game_dir.to_path_buf()
    }
}

/// Why the app won't install for a game at all, before any folder is read.
pub fn layout_for(entry: &Entry) -> Result<Layout, String> {
    let app = grokhub_core::brand::product_name();
    let layout =
        engine::layout(entry.id).ok_or(format!("{app} doesn't install mods for this game yet."))?;
    match layout {
        Layout::Steam => Err("Steam installs Workshop items: subscribe in Steam.".into()),
        Layout::Manager(why) => Err(why.into()),
        _ => Ok(layout),
    }
}

/// Where this game's mods go, or why the app won't install here.
pub fn target(game: &ModGame) -> Result<Target, String> {
    let entry = game.entry;
    let layout = layout_for(entry)?;
    let steam_root = entry
        .steam_app_id
        .and_then(|_| grokhub_core::steam::detect_root());
    let wow = entry.id == "wow";
    let game_dir = game_dir(game, steam_root.as_deref()).ok_or_else(|| {
        format!(
            "{} couldn't find {}'s folder on this PC.",
            grokhub_core::brand::product_name(),
            entry.name
        )
    })?;
    let (roaming, local) = app_data();
    let places = managers::Places {
        game_dir: Some(&game_dir),
        appdata: roaming.as_deref(),
        local_appdata: local.as_deref(),
    };
    if let Some(m) = managers::managing(entry.id, places) {
        return Err(format!(
            "{m} looks after {} on this PC, so install its mods there.",
            entry.name
        ));
    }
    let build = match (entry.steam_app_id, steam_root.as_deref()) {
        (Some(id), Some(r)) => grokhub_core::steam::build_id(r, id),
        _ => None,
    };
    let (name, app_id, dir) = (entry.name.to_string(), entry.steam_app_id, game_dir.clone());
    Ok(Target {
        layout,
        root: root_of(layout, &game_dir),
        game_dir,
        build,
        running: Arc::new(move || running_line(&processes_now(), &name, app_id, &dir, wow)),
    })
}

// ------------------------------------------------------------ the drawer

/// What a file's row offers, given what is installed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Offer {
    Install,
    Update {
        was: String,
    },
    Installed,
    Browser {
        url: String,
    },
    Steam {
        url: String,
    },
    /// The app won't write this game; the drawer's line says why.
    Blocked,
}

/// The cabin's `drawer::offer`.
pub fn offer(f: &ModFile, key: &str, installed: &[Manifest], blocked: bool) -> Offer {
    if let Download::Steam(id) = f.download {
        return Offer::Steam {
            url: grokhub_core::mods::workshop::item_url(&id.to_string()),
        };
    }
    if blocked {
        return Offer::Blocked;
    }
    let version = file_version(f);
    match installed.iter().find(|i| i.mod_key == key) {
        Some(i) if i.version == version => Offer::Installed,
        // An update comes the same way the file does: a browser download
        // goes through the pickup again, only for the mod's main file.
        Some(i) => match &f.download {
            Download::Browser(url) if f.primary => Offer::Browser { url: url.clone() },
            _ => Offer::Update {
                was: i.version.clone(),
            },
        },
        None => match &f.download {
            Download::Browser(url) => Offer::Browser { url: url.clone() },
            _ => Offer::Install,
        },
    }
}

fn file_version(f: &ModFile) -> String {
    if f.version.is_empty() {
        f.name.clone()
    } else {
        f.version.clone()
    }
}

/// "4.2 MB" or "880 KB".
pub fn size_label(bytes: u64) -> String {
    if bytes >= 1_000_000 {
        format!("{:.1} MB", bytes as f64 / 1e6)
    } else {
        format!("{} KB", bytes.div_ceil(1_000))
    }
}

/// What getting a file would take, in a few words.
pub fn file_line(f: &ModFile) -> String {
    let mut parts = Vec::new();
    if !f.version.is_empty() {
        parts.push(f.version.clone());
    }
    if f.size > 0 {
        parts.push(size_label(f.size));
    }
    match f.download {
        Download::Direct(_) => {}
        Download::Browser(_) => parts.push("download on the site".into()),
        Download::Steam(_) => parts.push("subscribe in Steam".into()),
    }
    parts.join(" · ")
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileRow {
    pub id: String,
    pub name: String,
    pub line: String,
    pub offer: Offer,
    /// "I downloaded it" shows: the page is waiting for this browser download.
    pub waiting: bool,
}

/// The open mod's drawer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DrawerView {
    pub files: Vec<FileRow>,
    /// Why the app won't install for this game, shown above the files.
    pub blocked: Option<String>,
    /// Ids of the mods the main file needs.
    pub needs: Vec<String>,
    pub about: String,
    /// The source failed, or Offline only is on.
    pub note: Option<String>,
}

/// The drawer's rows from the source's files and what is installed.
pub fn drawer_rows(
    files: &[ModFile],
    key: &str,
    installed: &[Manifest],
    blocked: bool,
    pickup: Option<&Pickup>,
) -> (Vec<FileRow>, Vec<String>) {
    let rows = files
        .iter()
        .take(FILES_SHOWN)
        .map(|f| FileRow {
            id: f.id.clone(),
            name: if f.name.is_empty() {
                f.file_name.clone()
            } else {
                f.name.clone()
            },
            line: file_line(f),
            offer: offer(f, key, installed, blocked),
            waiting: pickup.is_some_and(|p| p.key == key && p.file_name == f.file_name),
        })
        .collect();
    let needs = files
        .iter()
        .find(|f| f.primary)
        .or(files.first())
        .map(|f| f.deps.clone())
        .unwrap_or_default();
    (rows, needs)
}

// ------------------------------------------------------------ the plan

/// Download (or take the browser's copy), check, unpack, place, plan and
/// screen. The plan then waits on the sheet.
#[allow(clippy::too_many_arguments)]
pub fn prepare(
    config: &Path,
    t: &Target,
    entry: &Entry,
    profile: &Profile,
    source: Source,
    (id, name): (&str, &str),
    f: &ModFile,
    file: &Path,
    staging: &Path,
) -> Result<Sheet, String> {
    let checked = archive::verify(file, f.hash.as_ref())?;
    let unpacked = staging.join("files");
    let staged = archive::stage(file, &f.file_name, &unpacked).map_err(|e| e.line())?;
    let folder = if t.layout == Layout::BepInEx {
        id
    } else {
        name
    };
    let placed = engine::place(t.layout, &staged, folder)?;
    let pack = id.to_ascii_lowercase().contains("bepinexpack");
    if t.layout == Layout::BepInEx && !pack && !t.game_dir.join("BepInEx").is_dir() {
        return Err("This mod needs BepInEx. Install BepInExPack for this game first.".into());
    }
    let key = mod_key(source, id);
    let info = ModInfo {
        key: key.clone(),
        name: name.to_string(),
        version: file_version(f),
        build: t.build.clone(),
    };
    let mut plan = engine::plan(config, entry.id, &t.root, &unpacked, &placed, &info)?;
    match grokhub_core::mods::anticheat::screen(&plan, entry, profile) {
        Screen::Clear => {}
        Screen::OfflineOnly(note) => plan.offline_only = Some(note),
        Screen::Refused(why) => return Err(why),
    }
    let update = engine::installed(config, Some(entry.id))
        .iter()
        .any(|i| i.mod_key == key);
    Ok(Sheet::Install {
        plan,
        update,
        note: (!checked).then(|| {
            format!(
                "{} publishes no hash {} can check for this file.",
                source.label(),
                grokhub_core::brand::product_name()
            )
        }),
        running: t.running.clone(),
        staging: staging.to_path_buf(),
    })
}

/// Park a sheet for the click; returns what the page shows.
pub fn park(sheet: Sheet) -> SheetView {
    let mut w = work();
    w.next += 1;
    let token = w.next;
    let view = sheet.view(token);
    if let Some((_, Sheet::Install { staging, .. })) = w.sheet.replace((token, sheet)) {
        let _ = std::fs::remove_dir_all(staging);
    }
    view
}

/// Run the parked sheet's write, after its button was clicked. The line
/// says what happened.
pub fn confirm(config: &Path, token: u64) -> Result<String, String> {
    let sheet = {
        let mut w = work();
        match w.sheet.take() {
            Some((t, s)) if t == token => s,
            other => {
                w.sheet = other;
                return Err("That sheet is gone. Start again.".into());
            }
        }
    };
    match sheet {
        Sheet::Install {
            plan,
            update,
            running,
            staging,
            ..
        } => {
            let check = || running();
            let inst = Installer::new(config, &check);
            let now = grokhub_core::now_ms();
            let done = if update {
                inst.update(&plan, now).map(|(_, m)| m)
            } else {
                inst.apply(&plan, now)
            };
            let _ = std::fs::remove_dir_all(&staging);
            done.map(|m| {
                format!(
                    "{} {} is in. Uninstall puts every file back.",
                    m.name, m.version
                )
            })
        }
        Sheet::Remove {
            game,
            key,
            name,
            running,
            ..
        } => {
            let check = || running();
            let r = Installer::new(config, &check).uninstall(&game, &key)?;
            Ok(if r.kept.is_empty() {
                format!("{name} is out; the folder is as it was.")
            } else {
                format!(
                    "{name} is out. Kept {} you changed since: {}.",
                    if r.kept.len() == 1 {
                        "one file"
                    } else {
                        "files"
                    },
                    r.kept.join(", ")
                )
            })
        }
    }
}

/// Close the sheet without writing.
pub fn cancel(token: u64) {
    let mut w = work();
    if w.sheet.as_ref().is_some_and(|(t, _)| *t == token) {
        if let Some((_, Sheet::Install { staging, .. })) = w.sheet.take() {
            let _ = std::fs::remove_dir_all(staging);
        }
    }
}

/// The uninstall sheet for an installed mod.
pub fn remove_sheet(
    config: &Path,
    game: &str,
    key: &str,
    build: Option<&str>,
    running: Running,
) -> Result<Sheet, String> {
    let m = engine::installed(config, Some(game))
        .into_iter()
        .find(|m| m.mod_key == key)
        .ok_or_else(|| {
            format!(
                "That mod isn't installed by {}.",
                grokhub_core::brand::product_name()
            )
        })?;
    let idle = || None;
    let (removal, warn) = Installer::new(config, &idle).preview(game, key, build)?;
    Ok(Sheet::Remove {
        game: game.to_string(),
        key: key.to_string(),
        name: m.name,
        removal,
        warn,
        running,
    })
}

/// The browser download for `p` in `downloads`, if it is there.
pub fn find_download(downloads: &Path, p: &Pickup) -> Option<PathBuf> {
    grokhub_core::mods::pickup::find(downloads, &p.file_name, p.size, p.since)
}

// ------------------------------------------------------------ installed

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OursRow {
    pub key: String,
    pub name: String,
    pub version: String,
    pub offline_only: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OtherRow {
    pub name: String,
    pub detail: String,
    pub link: Option<String>,
}

/// The Installed tab.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstalledView {
    pub ours: Vec<OursRow>,
    pub others: Vec<OtherRow>,
    /// An install or uninstall a crash cut short, put right on first open.
    pub recovered: Option<String>,
}

/// Ours, and what the game has that the app didn't put there (an addon
/// folder a mod of ours made is listed once, as ours).
pub fn installed_view(ours: Vec<Manifest>, others: Vec<OtherRow>) -> InstalledView {
    let others = others
        .into_iter()
        .filter(|r| {
            !ours
                .iter()
                .any(|m| m.dirs.iter().any(|d| d.ends_with(&format!("/{}", r.name))))
        })
        .collect();
    InstalledView {
        ours: ours
            .into_iter()
            .map(|m| OursRow {
                key: m.mod_key,
                name: m.name,
                version: m.version,
                offline_only: m.offline_only,
            })
            .collect(),
        others,
        recovered: None,
    }
}

// ------------------------------------------------------------ commands

fn game_of(game: &str) -> Result<ModGame, String> {
    let rail = mods::rail_now(grokhub_core::now_ms());
    rail.games
        .iter()
        .find(|g| g.entry.id == game)
        .cloned()
        .ok_or_else(|| {
            format!(
                "{} doesn't mod that game.",
                grokhub_core::brand::product_name()
            )
        })
}

fn file_of(
    config: &Path,
    game: &str,
    source: Source,
    id: &str,
    file: &str,
) -> Result<ModFile, String> {
    let got = mods::client(config)
        .files(game, source, id, grokhub_core::now_ms())
        .map_err(|e| e.line())?;
    got.value
        .into_iter()
        .find(|f| f.id == file)
        .ok_or_else(|| "That file isn't on the mod's list any more.".into())
}

/// Plan an install with the busy flag held; frees staging on failure.
fn plan_job(
    app: &AppHandle,
    config: &Path,
    game: &ModGame,
    source: Source,
    mod_ref: (&str, &str),
    f: &ModFile,
    from: Option<PathBuf>,
) -> Result<SheetView, String> {
    {
        let mut w = work();
        if w.busy {
            return Err("Another install is getting ready. Wait for it first.".into());
        }
        w.busy = true;
    }
    let say = |line: String| {
        let _ = app.emit(JOB_EVENT, line);
    };
    say(format!("Getting {} ready…", mod_ref.1));
    let staging = staging_for(config, &mod_key(source, mod_ref.0));
    let run = || -> Result<Sheet, String> {
        let t = target(game)?;
        if let Some(why) = (t.running)() {
            return Err(why);
        }
        let _ = std::fs::remove_dir_all(&staging);
        std::fs::create_dir_all(&staging).map_err(|e| e.to_string())?;
        let file = match from {
            Some(p) => p,
            None => {
                let Download::Direct(url) = &f.download else {
                    return Err("This file downloads on the site.".into());
                };
                say(format!("Downloading {}…", f.file_name));
                let dest = staging.join("download");
                let req = Request::get(source, url.clone())
                    .header("User-Agent", &grokhub_core::mods::modrinth::user_agent());
                ModsNet::new(config.to_path_buf(), true)
                    .download(&req, &dest)
                    .map_err(|e| e.line())?;
                dest
            }
        };
        say("Checking the download…".into());
        let profile = grokhub_core::game_profile::profile(
            game.entry.protection,
            Some(&t.game_dir),
            &service_names(),
        );
        prepare(
            config, &t, game.entry, &profile, source, mod_ref, f, &file, &staging,
        )
    };
    let out = run();
    work().busy = false;
    match out {
        Ok(sheet) => Ok(park(sheet)),
        Err(e) => {
            let _ = std::fs::remove_dir_all(&staging);
            Err(e)
        }
    }
}

fn source_of(source: &str) -> Result<Source, String> {
    Source::from_id(source).ok_or_else(|| format!("No source {source}."))
}

/// The open mod's drawer: its files with what each offers, what the main
/// file needs, and the description.
#[tauri::command]
pub(crate) async fn mods_files(
    state: State<'_, ShellState>,
    game: String,
    source: String,
    id: String,
    summary: String,
) -> Result<DrawerView, String> {
    capabilities::needs(capabilities::current().mods, "Mods")?;
    let dir = state.config_dir.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let (g, src) = mods::game_source(&mods::rail_now(grokhub_core::now_ms()), &game, &source)?;
        if grokhub_core::offline::enabled() {
            return Ok(DrawerView {
                files: Vec::new(),
                blocked: None,
                needs: Vec::new(),
                about: summary,
                note: Some("Offline only is on.".into()),
            });
        }
        let client = mods::client(&dir);
        let now = grokhub_core::now_ms();
        let detail = client.detail(&game, src, &id, now);
        let files = client.files(&game, src, &id, now);
        let note = match (&detail, &files) {
            (Err(e), _) | (_, Err(e)) => Some(e.line()),
            _ => None,
        };
        let files = files.map(|g| g.value).unwrap_or_default();
        let about = detail
            .ok()
            .map(|d| d.value.description)
            .filter(|t| !t.trim().is_empty())
            .unwrap_or(summary);
        let blocked = target(&g).err();
        let key = mod_key(src, &id);
        let installed = engine::installed(&dir, Some(&game));
        let pickup = work().pickup.clone();
        let (rows, needs) =
            drawer_rows(&files, &key, &installed, blocked.is_some(), pickup.as_ref());
        Ok(DrawerView {
            files: rows,
            blocked,
            needs,
            about,
            note,
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Install or Update on a file with a direct link: download, check, unpack
/// and plan. The sheet then waits for its button.
#[tauri::command]
pub(crate) async fn mods_plan_install(
    app: AppHandle,
    state: State<'_, ShellState>,
    game: String,
    source: String,
    id: String,
    name: String,
    file: String,
) -> Result<SheetView, String> {
    capabilities::needs(capabilities::current().mods, "Mods")?;
    let dir = state.config_dir.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let src = source_of(&source)?;
        let g = game_of(&game)?;
        let f = file_of(&dir, &game, src, &id, &file)?;
        plan_job(&app, &dir, &g, src, (&id, &name), &f, None)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// "Download on <site>": remember the file, so "I downloaded it" can take
/// it from Downloads. Opening the page is the page's own link.
#[tauri::command]
pub(crate) async fn mods_wait_for_browser(
    state: State<'_, ShellState>,
    game: String,
    source: String,
    id: String,
    file: String,
) -> Result<(), String> {
    capabilities::needs(capabilities::current().mods, "Mods")?;
    let dir = state.config_dir.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let src = source_of(&source)?;
        let f = file_of(&dir, &game, src, &id, &file)?;
        work().pickup = Some(Pickup {
            key: mod_key(src, &id),
            file_name: f.file_name,
            size: f.size,
            since: SystemTime::now() - std::time::Duration::from_secs(5),
        });
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// "I downloaded it": take the browser's copy from Downloads and plan, or
/// say it isn't there yet.
#[tauri::command]
pub(crate) async fn mods_pickup_download(
    app: AppHandle,
    state: State<'_, ShellState>,
    game: String,
    source: String,
    id: String,
    name: String,
    file: String,
) -> Result<SheetView, String> {
    capabilities::needs(capabilities::current().mods, "Mods")?;
    let dir = state.config_dir.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let src = source_of(&source)?;
        let key = mod_key(src, &id);
        let p = work()
            .pickup
            .clone()
            .filter(|p| p.key == key)
            .ok_or("Download it on the site first.")?;
        let found = grokhub_core::mods::pickup::downloads_folder()
            .and_then(|d| find_download(&d, &p))
            .ok_or_else(|| {
                format!(
                    "{} isn't in Downloads yet. Let the download finish, then try again.",
                    p.file_name
                )
            })?;
        work().pickup = None;
        let g = game_of(&game)?;
        let f = file_of(&dir, &game, src, &id, &file)?;
        plan_job(&app, &dir, &g, src, (&id, &name), &f, Some(found))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Uninstall on an installed mod: what it would delete and put back.
#[tauri::command]
pub(crate) async fn mods_plan_uninstall(
    state: State<'_, ShellState>,
    game: String,
    key: String,
) -> Result<SheetView, String> {
    capabilities::needs(capabilities::current().mods, "Mods")?;
    let dir = state.config_dir.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let g = game_of(&game)?;
        let t = target(&g).ok();
        let running: Running = t
            .as_ref()
            .map(|t| t.running.clone())
            .unwrap_or_else(|| Arc::new(|| None));
        let build = t.and_then(|t| t.build);
        remove_sheet(&dir, &game, &key, build.as_deref(), running).map(park)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// The sheet's primary button: write. The line says what happened.
#[tauri::command]
pub(crate) async fn mods_confirm(
    state: State<'_, ShellState>,
    token: u64,
) -> Result<String, String> {
    capabilities::needs(capabilities::current().mods, "Mods")?;
    let dir = state.config_dir.clone();
    tauri::async_runtime::spawn_blocking(move || confirm(&dir, token))
        .await
        .map_err(|e| e.to_string())?
}

/// The sheet's Cancel.
#[tauri::command]
pub(crate) fn mods_cancel(token: u64) -> Result<(), String> {
    capabilities::needs(capabilities::current().mods, "Mods")?;
    cancel(token);
    Ok(())
}

/// The Installed tab for one game.
#[tauri::command]
pub(crate) async fn mods_installed(
    state: State<'_, ShellState>,
    game: String,
) -> Result<InstalledView, String> {
    capabilities::needs(capabilities::current().mods, "Mods")?;
    let dir = state.config_dir.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let recovered = recover_once(&dir);
        let g = game_of(&game)?;
        let others = installed_mods(&g, grokhub_core::steam::detect_root().as_deref())
            .into_iter()
            .map(|r| OtherRow {
                name: r.name,
                detail: r.detail,
                link: r.link,
            })
            .collect();
        let mut view = installed_view(engine::installed(&dir, Some(&game)), others);
        view.recovered = (!recovered.is_empty()).then(|| recovered.join(" "));
        Ok(view)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
#[path = "mods_install_tests.rs"]
mod tests;
