//! Installing a staged mod into a game folder and taking it out again,
//! leaving the folder as it was.
//!
//! Every install is journaled first: `{config}/mods/installed/<game>/<mod>/
//! manifest.json` lists each file to write with the hash it will have, and
//! each file it replaces is copied to `backups/` before anything is written.
//! Writes go to a temp file and are renamed into place. Uninstall deletes
//! only files whose hash is still what Loadout wrote (anything changed since
//! is kept and listed), restores the backups, and removes only folders
//! Loadout made, only when empty. A crash mid-way is rolled back by
//! [`recover`]. Nothing here runs a file from a mod.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::archive::safe_rel;
use super::cache::mods_dir;

/// Where a game's mods go and how an archive lines up with that place.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layout {
    /// Bethesda games: everything under `Data/`. An archive's own `Data/`
    /// folder, or one wrapper folder, is unwrapped.
    Data,
    /// The game folder itself, as the archive lays it out. Archives that
    /// start with one of `known` are placed as they are; one wrapper folder
    /// around them is unwrapped; anything else goes under `fallback`.
    Root {
        known: &'static [&'static str],
        fallback: &'static str,
    },
    /// Each mod is a folder in this subfolder (`Mods`, `Interface/AddOns`).
    Folders(&'static str),
    /// Single files into this folder (Minecraft's `mods`).
    Files(&'static str),
    /// A Thunderstore package for a BepInEx game.
    BepInEx,
    /// Steam installs these (Workshop). Loadout only reads what Steam put there.
    Steam,
    /// Not in this version; the line says what to use instead.
    Manager(&'static str),
}

/// How each allowlisted game takes mods.
pub fn layout(game_id: &str) -> Option<Layout> {
    Some(match game_id {
        "skyrimse" | "fallout4" | "starfield" => Layout::Data,
        "cyberpunk2077" => Layout::Root {
            known: &["archive", "bin", "r6", "red4ext", "engine", "mods"],
            fallback: "archive/pc/mod",
        },
        "witcher3" => Layout::Root {
            known: &["mods", "dlc", "bin"],
            fallback: "mods",
        },
        "bg3" => Layout::Manager(
            "Baldur's Gate 3 mods need a load order, so use BG3 Mod Manager for them for now.",
        ),
        "stardew" => Layout::Folders("Mods"),
        "wow" => Layout::Folders("Interface/AddOns"),
        "minecraft" => Layout::Files("mods"),
        "valheim" | "lethalcompany" | "ror2" | "repo" => Layout::BepInEx,
        "rimworld" | "cities" | "stellaris" | "tmodloader" => Layout::Steam,
        _ => return None,
    })
}

/// Folders a Bethesda `Data/` holds; a wrapper with one of these names is
/// part of the mod, not a wrapper.
const DATA_DIRS: &[&str] = &[
    "meshes",
    "textures",
    "scripts",
    "interface",
    "skse",
    "f4se",
    "sfse",
    "sound",
    "music",
    "materials",
    "strings",
    "shadersfx",
    "seq",
    "lodsettings",
    "grass",
    "video",
    "source",
    "calientetools",
    "nemesis_engine",
    "netscriptframework",
    "mcm",
    "tools",
    "dialogueviews",
];

/// Thunderstore package files that describe the package, not the mod.
const PACKAGE_META: &[&str] = &[
    "manifest.json",
    "icon.png",
    "readme.md",
    "changelog.md",
    "license",
    "license.md",
    "license.txt",
];

fn lower(s: &std::ffi::OsStr) -> String {
    s.to_string_lossy().to_ascii_lowercase()
}

/// The one folder every file sits in, if there is one.
fn wrapper(files: &[PathBuf]) -> Option<String> {
    let first = files.first()?.components().next()?;
    let name = first.as_os_str().to_string_lossy().to_string();
    files
        .iter()
        .all(|f| f.components().count() > 1 && f.components().next() == Some(first))
        .then_some(name)
}

fn strip_first(p: &Path) -> PathBuf {
    p.components().skip(1).collect()
}

/// Where each staged file goes, relative to the game's mod root.
pub fn place(
    layout: Layout,
    staged: &[PathBuf],
    mod_name: &str,
) -> Result<Vec<(PathBuf, PathBuf)>, String> {
    if staged.is_empty() {
        return Err("The download was empty.".into());
    }
    let folder = mod_folder_name(mod_name);
    let out: Vec<(PathBuf, PathBuf)> = match layout {
        Layout::Steam => {
            return Err("Steam installs Workshop items. Subscribe in Steam.".into());
        }
        Layout::Manager(why) => return Err(why.to_string()),
        Layout::Data => {
            let wrap =
                wrapper(staged).filter(|w| !DATA_DIRS.contains(&w.to_ascii_lowercase().as_str()));
            staged
                .iter()
                .map(|f| {
                    let mut to = f.clone();
                    if let Some(w) = &wrap {
                        if w.eq_ignore_ascii_case("data")
                            || !to.components().any(|c| lower(c.as_os_str()) == "data")
                        {
                            to = strip_first(&to);
                        }
                    }
                    if let Some(i) = to.components().position(|c| lower(c.as_os_str()) == "data") {
                        to = to.components().skip(i + 1).collect();
                    }
                    (f.clone(), to)
                })
                .collect()
        }
        Layout::Root { known, fallback } => {
            let is_known = |p: &Path| {
                p.components()
                    .next()
                    .is_some_and(|c| known.contains(&lower(c.as_os_str()).as_str()))
            };
            let unwrap = wrapper(staged).is_some_and(|_| !staged.iter().all(|f| is_known(f)))
                && staged.iter().all(|f| is_known(&strip_first(f)));
            staged
                .iter()
                .map(|f| {
                    let to = if unwrap { strip_first(f) } else { f.clone() };
                    let to = if is_known(&to) {
                        to
                    } else {
                        Path::new(fallback).join(to)
                    };
                    (f.clone(), to)
                })
                .collect()
        }
        Layout::Folders(sub) => {
            let loose = staged.iter().any(|f| f.components().count() == 1);
            staged
                .iter()
                .map(|f| {
                    let to = if loose {
                        Path::new(sub).join(&folder).join(f)
                    } else {
                        Path::new(sub).join(f)
                    };
                    (f.clone(), to)
                })
                .collect()
        }
        Layout::Files(sub) => {
            let jars: Vec<&PathBuf> = staged
                .iter()
                .filter(|f| f.extension().is_some_and(|e| e.eq_ignore_ascii_case("jar")))
                .collect();
            if jars.is_empty() {
                return Err("This download has no mod file to place.".into());
            }
            jars.into_iter()
                .map(|f| {
                    (
                        f.clone(),
                        Path::new(sub).join(f.file_name().unwrap_or_default()),
                    )
                })
                .collect()
        }
        Layout::BepInEx => {
            let pack = mod_name.to_ascii_lowercase().contains("bepinexpack");
            let files: Vec<&PathBuf> = staged
                .iter()
                .filter(|f| {
                    !(f.components().count() == 1
                        && PACKAGE_META.contains(&lower(f.as_os_str()).as_str()))
                })
                .collect();
            files
                .into_iter()
                .map(|f| {
                    let first = f
                        .components()
                        .next()
                        .map(|c| lower(c.as_os_str()))
                        .unwrap_or_default();
                    let to = if pack {
                        // BepInExPack/BepInEx/..., BepInExPack/winhttp.dll: the game folder.
                        if f.components().count() > 1 && first.starts_with("bepinexpack") {
                            strip_first(f)
                        } else {
                            f.clone()
                        }
                    } else {
                        match first.as_str() {
                            "bepinex" => f.clone(),
                            "plugins" | "patchers" | "core" | "monomod" => Path::new("BepInEx")
                                .join(&first)
                                .join(&folder)
                                .join(strip_first(f)),
                            "config" => Path::new("BepInEx").join("config").join(strip_first(f)),
                            _ => Path::new("BepInEx/plugins").join(&folder).join(f),
                        }
                    };
                    (f.clone(), to)
                })
                .collect()
        }
    };
    if out.iter().any(|(_, to)| to.as_os_str().is_empty()) {
        return Err("This archive's layout doesn't fit the game.".into());
    }
    Ok(out)
}

/// A folder name from a mod's name: letters, digits, `-`, `_`, `.` and spaces.
pub fn mod_folder_name(name: &str) -> String {
    let clean: String = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || "-_. ".contains(c) {
                c
            } else {
                '_'
            }
        })
        .collect();
    let clean = clean.trim().trim_matches('.').to_string();
    if clean.is_empty() {
        "mod".into()
    } else {
        clean.chars().take(80).collect()
    }
}

/// One file the plan writes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Planned {
    /// The staged copy.
    pub from: PathBuf,
    /// Where it goes, relative to the mod root, with `/`.
    pub to: String,
    /// A file is already there and will be backed up and replaced.
    pub replaces: bool,
    pub sha256: String,
}

/// Exactly what an install will do, for the confirm sheet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    pub game: String,
    /// `source:id`, one per mod.
    pub mod_key: String,
    pub name: String,
    pub version: String,
    pub root: PathBuf,
    pub files: Vec<Planned>,
    /// Folders to create, shallowest first, relative to the root.
    pub dirs: Vec<String>,
    /// The game's build when planned (Steam `buildid`), to warn on uninstall.
    pub build: Option<String>,
}

impl Plan {
    pub fn adds(&self) -> impl Iterator<Item = &Planned> {
        self.files.iter().filter(|f| !f.replaces)
    }

    pub fn replaces(&self) -> impl Iterator<Item = &Planned> {
        self.files.iter().filter(|f| f.replaces)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum State {
    Applying,
    Installed,
    Removing,
}

/// One file Loadout wrote.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Written {
    pub to: String,
    pub sha256: String,
    /// The backup of the file this replaced, under the mod's `backups/`.
    #[serde(default)]
    pub backup: Option<String>,
    /// That file's hash.
    #[serde(default)]
    pub was: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    pub game: String,
    pub mod_key: String,
    pub name: String,
    pub version: String,
    pub state: State,
    pub root: PathBuf,
    pub files: Vec<Written>,
    /// Folders Loadout created, shallowest first.
    pub dirs: Vec<String>,
    #[serde(default)]
    pub build: Option<String>,
    pub at: u64,
}

/// What an uninstall did, or would do.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Removal {
    pub deletes: Vec<String>,
    pub restores: Vec<String>,
    /// Changed since install; left alone.
    pub kept: Vec<String>,
}

fn installed_dir(config: &Path) -> PathBuf {
    mods_dir(config).join("installed")
}

fn slug(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '_'
            }
        })
        .take(120)
        .collect()
}

fn mod_dir(config: &Path, game: &str, mod_key: &str) -> PathBuf {
    installed_dir(config).join(slug(game)).join(slug(mod_key))
}

fn io<'p>(what: &str, path: &'p Path) -> impl Fn(std::io::Error) -> String + 'p {
    let what = what.to_string();
    move |e| format!("Couldn't {what} {}: {e}", path.display())
}

fn sha_of(path: &Path) -> Option<String> {
    crate::model_download::sha256_file(path).ok()
}

fn save(dir: &Path, m: &Manifest) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(io("create", dir))?;
    let path = dir.join("manifest.json");
    let tmp = dir.join("manifest.json.tmp");
    let text = serde_json::to_vec_pretty(m).map_err(|e| e.to_string())?;
    std::fs::write(&tmp, text).map_err(io("write", &tmp))?;
    std::fs::rename(&tmp, &path).map_err(io("write", &path))
}

fn load(dir: &Path) -> Option<Manifest> {
    serde_json::from_slice(&std::fs::read(dir.join("manifest.json")).ok()?).ok()
}

/// Every mod Loadout installed for one game, or for all games with `None`.
pub fn installed(config: &Path, game: Option<&str>) -> Vec<Manifest> {
    let mut out = Vec::new();
    let games: Vec<PathBuf> = match game {
        Some(g) => vec![installed_dir(config).join(slug(g))],
        None => std::fs::read_dir(installed_dir(config))
            .map(|rd| rd.flatten().map(|e| e.path()).collect())
            .unwrap_or_default(),
    };
    for g in games {
        let Ok(rd) = std::fs::read_dir(&g) else {
            continue;
        };
        out.extend(rd.flatten().filter_map(|e| load(&e.path())));
    }
    out.sort_by_key(|m| m.name.to_lowercase());
    out
}

/// Whether a path below `root` passes through a link (a write there could
/// land somewhere else).
fn through_link(root: &Path, rel: &Path) -> bool {
    let mut at = root.to_path_buf();
    for c in rel.components() {
        at.push(c);
        if std::fs::symlink_metadata(&at).is_ok_and(|m| m.file_type().is_symlink()) {
            return true;
        }
    }
    false
}

/// The mod a plan is for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModInfo {
    /// `source:id`, one per mod.
    pub key: String,
    pub name: String,
    pub version: String,
    /// The game's build now (Steam `buildid`), when known.
    pub build: Option<String>,
}

/// Plan an install: where each staged file goes, what it replaces, what
/// folders it makes. Refuses a file another Loadout mod wrote, and a path
/// that runs through a link.
pub fn plan(
    config: &Path,
    game: &str,
    root: &Path,
    staged_dir: &Path,
    placed: &[(PathBuf, PathBuf)],
    info: &ModInfo,
) -> Result<Plan, String> {
    let mod_key = info.key.as_str();
    if !root.is_dir() {
        return Err(format!("{} isn't there.", root.display()));
    }
    let (own, others): (Vec<Manifest>, Vec<Manifest>) = installed(config, Some(game))
        .into_iter()
        .partition(|m| m.mod_key == mod_key);
    // An update: what this mod wrote before comes out first.
    let own = own.into_iter().next();
    let was_ours = |to: &str| {
        own.as_ref()
            .and_then(|m| m.files.iter().find(|w| w.to == to))
    };
    let mut files = Vec::new();
    let mut dirs: Vec<String> = Vec::new();
    for (from, to) in placed {
        let to_str = to.to_string_lossy().replace('\\', "/");
        let rel = safe_rel(&to_str).ok_or_else(|| format!("\"{to_str}\" isn't a safe place."))?;
        if let Some(other) = others
            .iter()
            .find(|m| m.files.iter().any(|w| w.to.eq_ignore_ascii_case(&to_str)))
        {
            return Err(format!(
                "{} already put {to_str} there. Uninstall it first; Loadout doesn't order mods yet.",
                other.name
            ));
        }
        if files
            .iter()
            .any(|p: &Planned| p.to.eq_ignore_ascii_case(&to_str))
        {
            return Err(format!("This archive has {to_str} twice."));
        }
        if through_link(root, &rel) {
            return Err(format!(
                "{to_str} goes through a link, so Loadout won't write it."
            ));
        }
        let dest = root.join(&rel);
        if dest.is_dir() {
            return Err(format!("{to_str} is a folder in the game."));
        }
        let mut at = PathBuf::new();
        for c in rel.parent().into_iter().flat_map(|p| p.components()) {
            at.push(c);
            let s = at.to_string_lossy().replace('\\', "/");
            let ours = own.as_ref().is_some_and(|m| m.dirs.contains(&s));
            if (ours || !root.join(&at).exists()) && !dirs.contains(&s) {
                dirs.push(s);
            }
        }
        let src = staged_dir.join(from);
        files.push(Planned {
            sha256: sha_of(&src).ok_or_else(|| format!("Couldn't read {}.", src.display()))?,
            from: src,
            replaces: match was_ours(&to_str) {
                Some(w) => w.backup.is_some(),
                None => dest.exists(),
            },
            to: to_str,
        });
    }
    Ok(Plan {
        game: game.into(),
        mod_key: mod_key.into(),
        name: info.name.clone(),
        version: info.version.clone(),
        root: root.to_path_buf(),
        files,
        dirs,
        build: info.build.clone(),
    })
}

/// Applies plans and uninstalls. `running` says why not now ("Close
/// Skyrim first."), asked at the start and right before the first write.
pub struct Installer<'a> {
    config: &'a Path,
    running: &'a dyn Fn() -> Option<String>,
    /// Tests: fail after this many file writes.
    fail_after: Option<usize>,
}

impl<'a> Installer<'a> {
    pub fn new(config: &'a Path, running: &'a dyn Fn() -> Option<String>) -> Self {
        Self {
            config,
            running,
            fail_after: None,
        }
    }

    fn check_running(&self) -> Result<(), String> {
        match (self.running)() {
            Some(why) => Err(why),
            None => Ok(()),
        }
    }

    /// Install a plan. On any failure the folder is put back as it was.
    pub fn apply(&self, plan: &Plan, now: u64) -> Result<Manifest, String> {
        self.check_running()?;
        let dir = mod_dir(self.config, &plan.game, &plan.mod_key);
        if load(&dir).is_some() {
            return Err(format!("{} is already installed.", plan.name));
        }
        let backups = dir.join("backups");
        let mut m = Manifest {
            game: plan.game.clone(),
            mod_key: plan.mod_key.clone(),
            name: plan.name.clone(),
            version: plan.version.clone(),
            state: State::Applying,
            root: plan.root.clone(),
            files: plan
                .files
                .iter()
                .enumerate()
                .map(|(i, f)| Written {
                    to: f.to.clone(),
                    sha256: f.sha256.clone(),
                    backup: f.replaces.then(|| format!("{i}.bak")),
                    was: None,
                })
                .collect(),
            dirs: plan.dirs.clone(),
            build: plan.build.clone(),
            at: now,
        };
        save(&dir, &m)?;
        match self.write_all(plan, &mut m, &dir, &backups) {
            Ok(()) => {
                m.state = State::Installed;
                save(&dir, &m)?;
                Ok(m)
            }
            Err(e) => {
                let _ = undo(&dir, &m);
                Err(e)
            }
        }
    }

    fn write_all(
        &self,
        plan: &Plan,
        m: &mut Manifest,
        dir: &Path,
        backups: &Path,
    ) -> Result<(), String> {
        std::fs::create_dir_all(backups).map_err(io("create", backups))?;
        for (f, w) in plan.files.iter().zip(m.files.iter_mut()) {
            let dest = plan.root.join(&f.to);
            if !dest.exists() {
                w.backup = None;
            }
            if let Some(b) = &w.backup {
                let bak = backups.join(b);
                std::fs::copy(&dest, &bak).map_err(io("back up", &dest))?;
                w.was = sha_of(&bak);
            }
        }
        save(dir, m)?;
        // The last word before writing: the game may have started meanwhile.
        self.check_running()?;
        for d in &plan.dirs {
            let p = plan.root.join(d);
            std::fs::create_dir_all(&p).map_err(io("create", &p))?;
        }
        for (n, f) in plan.files.iter().enumerate() {
            if self.fail_after == Some(n) {
                return Err("stopped for a test".into());
            }
            let dest = plan.root.join(&f.to);
            let tmp = dest.with_file_name(format!(
                ".{}.loadout-tmp",
                dest.file_name().unwrap_or_default().to_string_lossy()
            ));
            std::fs::copy(&f.from, &tmp)
                .and_then(|_| std::fs::rename(&tmp, &dest))
                .map_err(|e| {
                    let _ = std::fs::remove_file(&tmp);
                    format!("Couldn't write {}: {e}", dest.display())
                })?;
        }
        Ok(())
    }

    /// What uninstalling a mod would do, and a warning when the game was
    /// updated since (restoring backups could undo part of that update).
    pub fn preview(
        &self,
        game: &str,
        mod_key: &str,
        build_now: Option<&str>,
    ) -> Result<(Removal, Option<String>), String> {
        let m = load(&mod_dir(self.config, game, mod_key))
            .ok_or("That mod isn't installed by Loadout.")?;
        let removal = removal(&m);
        let warn = match (&m.build, build_now) {
            (Some(then), Some(now)) if then != now && !removal.restores.is_empty() => Some(
                "The game was updated since this mod went in. Putting back the files it replaced could undo part of that update; Steam can verify the game files afterwards."
                    .to_string(),
            ),
            _ => None,
        };
        Ok((removal, warn))
    }

    /// Take a mod out: its files go, the files it replaced come back,
    /// anything changed since install stays and is listed.
    pub fn uninstall(&self, game: &str, mod_key: &str) -> Result<Removal, String> {
        self.check_running()?;
        let dir = mod_dir(self.config, game, mod_key);
        let mut m = load(&dir).ok_or("That mod isn't installed by Loadout.")?;
        m.state = State::Removing;
        save(&dir, &m)?;
        undo(&dir, &m)
    }

    /// Replace an installed mod with a new plan: the old one comes out
    /// first, then the new one goes in.
    pub fn update(&self, plan: &Plan, now: u64) -> Result<(Removal, Manifest), String> {
        let removed = self.uninstall(&plan.game, &plan.mod_key)?;
        let m = self.apply(plan, now).map_err(|e| {
            format!("The old version was removed, but the new one didn't install: {e}")
        })?;
        Ok((removed, m))
    }
}

/// What undoing a manifest touches, read from the folder as it is now.
fn removal(m: &Manifest) -> Removal {
    let mut r = Removal::default();
    for w in &m.files {
        let now = sha_of(&m.root.join(&w.to));
        let ours = now.as_deref() == Some(w.sha256.as_str());
        let original = now.is_some() && now == w.was;
        match (&w.backup, ours, original, now.is_none()) {
            (Some(_), _, true, _) => {}
            (Some(_), true, _, _) | (Some(_), _, _, true) => r.restores.push(w.to.clone()),
            (None, true, _, _) => r.deletes.push(w.to.clone()),
            (None, _, _, true) => {}
            _ => r.kept.push(w.to.clone()),
        }
    }
    r
}

/// Put the folder back from a manifest, then drop the manifest. Safe to
/// run again after a crash part-way.
fn undo(dir: &Path, m: &Manifest) -> Result<Removal, String> {
    let r = removal(m);
    let backups = dir.join("backups");
    for w in &m.files {
        let dest = m.root.join(&w.to);
        if r.deletes.contains(&w.to) {
            std::fs::remove_file(&dest).map_err(io("remove", &dest))?;
        } else if r.restores.contains(&w.to) {
            let Some(bak) = w
                .backup
                .as_ref()
                .map(|b| backups.join(b))
                .filter(|b| b.is_file())
            else {
                continue;
            };
            let tmp = dest.with_file_name(format!(
                ".{}.loadout-tmp",
                dest.file_name().unwrap_or_default().to_string_lossy()
            ));
            std::fs::copy(&bak, &tmp)
                .and_then(|_| std::fs::rename(&tmp, &dest))
                .map_err(io("restore", &dest))?;
        }
    }
    for d in m.dirs.iter().rev() {
        let p = m.root.join(d);
        if std::fs::read_dir(&p).is_ok_and(|mut rd| rd.next().is_none()) {
            let _ = std::fs::remove_dir(&p);
        }
    }
    std::fs::remove_dir_all(dir).map_err(io("remove", dir))?;
    Ok(r)
}

/// Finish what a crash left: an install part-way is rolled back, an
/// uninstall part-way is finished. Returns one line per mod it touched.
pub fn recover(config: &Path) -> Vec<String> {
    let mut lines = Vec::new();
    let Ok(games) = std::fs::read_dir(installed_dir(config)) else {
        return lines;
    };
    for g in games.flatten() {
        let Ok(mods) = std::fs::read_dir(g.path()) else {
            continue;
        };
        for e in mods.flatten() {
            let dir = e.path();
            match load(&dir) {
                Some(m) if m.state != State::Installed => {
                    let what = if m.state == State::Applying {
                        "rolled back an install of"
                    } else {
                        "finished removing"
                    };
                    match undo(&dir, &m) {
                        Ok(_) => {
                            lines.push(format!("Loadout {what} {} that was cut short.", m.name))
                        }
                        Err(err) => lines.push(format!("Couldn't tidy up {}: {err}", m.name)),
                    }
                }
                Some(_) => {}
                None => {
                    // A manifest that never got written: nothing touched the game.
                    let _ = std::fs::remove_dir_all(&dir);
                }
            }
        }
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mods::testing::temp_config;
    use std::collections::BTreeMap;

    /// Every file under `root` and its hash.
    fn walk(root: &Path) -> BTreeMap<String, String> {
        fn go(root: &Path, at: &Path, out: &mut BTreeMap<String, String>) {
            for e in std::fs::read_dir(at).unwrap().flatten() {
                let p = e.path();
                let rel = p
                    .strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/");
                if p.is_dir() {
                    out.insert(format!("{rel}/"), String::new());
                    go(root, &p, out);
                } else {
                    out.insert(rel, sha_of(&p).unwrap());
                }
            }
        }
        let mut out = BTreeMap::new();
        go(root, root, &mut out);
        out
    }

    struct Rig {
        config: PathBuf,
        game: PathBuf,
        staged: PathBuf,
    }

    fn rig(name: &str) -> Rig {
        let base = temp_config(name);
        let r = Rig {
            config: base.join("config"),
            game: base.join("Skyrim"),
            staged: base.join("staged"),
        };
        std::fs::create_dir_all(r.game.join("Data/textures")).unwrap();
        std::fs::write(r.game.join("Data/Skyrim.esm"), b"base game").unwrap();
        std::fs::write(r.game.join("Data/textures/sky.dds"), b"vanilla sky").unwrap();
        std::fs::create_dir_all(r.staged.join("Better Sky/textures")).unwrap();
        std::fs::create_dir_all(r.staged.join("Better Sky/meshes/sky")).unwrap();
        std::fs::write(r.staged.join("Better Sky/BetterSky.esp"), b"plugin").unwrap();
        std::fs::write(r.staged.join("Better Sky/textures/sky.dds"), b"better sky").unwrap();
        std::fs::write(r.staged.join("Better Sky/meshes/sky/dome.nif"), b"dome").unwrap();
        r
    }

    fn staged_files(r: &Rig) -> Vec<PathBuf> {
        vec![
            PathBuf::from("Better Sky/BetterSky.esp"),
            PathBuf::from("Better Sky/meshes/sky/dome.nif"),
            PathBuf::from("Better Sky/textures/sky.dds"),
        ]
        .into_iter()
        .filter(|p| r.staged.join(p).exists())
        .collect()
    }

    fn sky_plan(r: &Rig, key: &str) -> Plan {
        let placed = place(Layout::Data, &staged_files(r), "Better Sky").unwrap();
        plan(
            &r.config,
            "skyrimse",
            &r.game.join("Data"),
            &r.staged,
            &placed,
            &info(key, "Better Sky", Some("100")),
        )
        .unwrap()
    }

    fn info(key: &str, name: &str, build: Option<&str>) -> ModInfo {
        ModInfo {
            key: key.into(),
            name: name.into(),
            version: "1.0".into(),
            build: build.map(str::to_owned),
        }
    }

    fn idle() -> Option<String> {
        None
    }

    #[test]
    fn install_then_uninstall_leaves_the_folder_byte_identical() {
        let r = rig("mods-install");
        let before = walk(&r.game);
        let p = sky_plan(&r, "nexus:1");
        let adds: Vec<&str> = p.adds().map(|f| f.to.as_str()).collect();
        let replaces: Vec<&str> = p.replaces().map(|f| f.to.as_str()).collect();
        assert_eq!(adds, ["BetterSky.esp", "meshes/sky/dome.nif"]);
        assert_eq!(replaces, ["textures/sky.dds"]);
        assert_eq!(p.dirs, ["meshes", "meshes/sky"]);
        let inst = Installer::new(&r.config, &idle);
        let m = inst.apply(&p, 1).unwrap();
        assert_eq!(m.state, State::Installed);
        assert_eq!(
            std::fs::read(r.game.join("Data/textures/sky.dds")).unwrap(),
            b"better sky"
        );
        assert_eq!(installed(&r.config, Some("skyrimse")).len(), 1);
        assert!(inst.apply(&p, 2).is_err(), "twice is refused");
        let (preview, warn) = inst.preview("skyrimse", "nexus:1", Some("100")).unwrap();
        assert_eq!(preview.restores, ["textures/sky.dds"]);
        assert_eq!(warn, None);
        let (_, warn) = inst.preview("skyrimse", "nexus:1", Some("101")).unwrap();
        assert!(warn.unwrap().contains("updated since"));
        let gone = inst.uninstall("skyrimse", "nexus:1").unwrap();
        assert_eq!(gone.deletes.len(), 2);
        assert!(gone.kept.is_empty());
        assert_eq!(walk(&r.game), before, "byte-identical after uninstall");
        assert!(installed(&r.config, Some("skyrimse")).is_empty());
    }

    #[test]
    fn a_file_changed_since_install_is_kept_and_listed() {
        let r = rig("mods-edited");
        let inst = Installer::new(&r.config, &idle);
        inst.apply(&sky_plan(&r, "nexus:1"), 1).unwrap();
        std::fs::write(r.game.join("Data/BetterSky.esp"), b"user tweaked").unwrap();
        let gone = inst.uninstall("skyrimse", "nexus:1").unwrap();
        assert_eq!(gone.kept, ["BetterSky.esp"]);
        assert_eq!(
            std::fs::read(r.game.join("Data/BetterSky.esp")).unwrap(),
            b"user tweaked"
        );
        assert_eq!(
            std::fs::read(r.game.join("Data/textures/sky.dds")).unwrap(),
            b"vanilla sky"
        );
        assert!(!r.game.join("Data/meshes").exists());
    }

    #[test]
    fn a_crash_mid_install_is_rolled_back() {
        let r = rig("mods-crash");
        let before = walk(&r.game);
        let p = sky_plan(&r, "nexus:1");
        let mut inst = Installer::new(&r.config, &idle);
        inst.fail_after = Some(2);
        assert!(inst.apply(&p, 1).is_err());
        assert_eq!(walk(&r.game), before, "a failed apply undoes itself");
        // A hard crash leaves the journal in Applying with files written.
        let dir = mod_dir(&r.config, "skyrimse", "nexus:1");
        let inst = Installer::new(&r.config, &idle);
        inst.apply(&p, 1).unwrap();
        let mut m = load(&dir).unwrap();
        m.state = State::Applying;
        save(&dir, &m).unwrap();
        let lines = recover(&r.config);
        assert_eq!(lines.len(), 1, "{lines:?}");
        assert!(lines[0].contains("rolled back"));
        assert_eq!(walk(&r.game), before);
        assert!(recover(&r.config).is_empty(), "recovery is idempotent");
    }

    #[test]
    fn a_running_game_is_refused_at_apply_and_uninstall() {
        let r = rig("mods-running");
        let before = walk(&r.game);
        let p = sky_plan(&r, "nexus:1");
        let running = || Some("Close Skyrim first.".to_string());
        let inst = Installer::new(&r.config, &running);
        assert_eq!(inst.apply(&p, 1).unwrap_err(), "Close Skyrim first.");
        assert_eq!(walk(&r.game), before);
        Installer::new(&r.config, &idle).apply(&p, 1).unwrap();
        assert_eq!(
            inst.uninstall("skyrimse", "nexus:1").unwrap_err(),
            "Close Skyrim first."
        );
        // Started between the plan and the first write.
        let calls = std::cell::Cell::new(0);
        let later = || {
            calls.set(calls.get() + 1);
            (calls.get() > 1).then(|| "Close Skyrim first.".to_string())
        };
        Installer::new(&r.config, &idle)
            .uninstall("skyrimse", "nexus:1")
            .unwrap();
        let inst = Installer::new(&r.config, &later);
        assert!(inst.apply(&p, 1).is_err());
        assert_eq!(walk(&r.game), before);
    }

    #[test]
    fn two_mods_writing_one_file_is_refused_with_the_other_named() {
        let r = rig("mods-conflict");
        Installer::new(&r.config, &idle)
            .apply(&sky_plan(&r, "nexus:1"), 1)
            .unwrap();
        let placed = place(Layout::Data, &staged_files(&r), "Other Sky").unwrap();
        let err = plan(
            &r.config,
            "skyrimse",
            &r.game.join("Data"),
            &r.staged,
            &placed,
            &info("nexus:2", "Other Sky", None),
        )
        .unwrap_err();
        assert!(err.starts_with("Better Sky already put"), "{err}");
    }

    #[test]
    fn update_swaps_the_files() {
        let r = rig("mods-update");
        let inst = Installer::new(&r.config, &idle);
        inst.apply(&sky_plan(&r, "nexus:1"), 1).unwrap();
        std::fs::remove_file(r.staged.join("Better Sky/meshes/sky/dome.nif")).unwrap();
        std::fs::write(r.staged.join("Better Sky/BetterSky.esp"), b"plugin v2").unwrap();
        let p2 = sky_plan(&r, "nexus:1");
        let (_, m) = inst.update(&p2, 2).unwrap();
        assert_eq!(m.files.len(), 2);
        assert_eq!(
            std::fs::read(r.game.join("Data/BetterSky.esp")).unwrap(),
            b"plugin v2"
        );
        assert!(!r.game.join("Data/meshes").exists());
        assert_eq!(
            std::fs::read(r.game.join("Data/textures/sky.dds")).unwrap(),
            b"better sky"
        );
        inst.uninstall("skyrimse", "nexus:1").unwrap();
        assert_eq!(
            std::fs::read(r.game.join("Data/textures/sky.dds")).unwrap(),
            b"vanilla sky"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_path_through_a_link_is_refused() {
        let r = rig("mods-link");
        let elsewhere = r.config.parent().unwrap().join("elsewhere");
        std::fs::create_dir_all(&elsewhere).unwrap();
        std::os::unix::fs::symlink(&elsewhere, r.game.join("Data/meshes")).unwrap();
        let placed = place(Layout::Data, &staged_files(&r), "Better Sky").unwrap();
        let err = plan(
            &r.config,
            "skyrimse",
            &r.game.join("Data"),
            &r.staged,
            &placed,
            &info("nexus:1", "Better Sky", None),
        )
        .unwrap_err();
        assert!(err.contains("link"), "{err}");
    }

    fn p(list: &[&str]) -> Vec<PathBuf> {
        list.iter().map(PathBuf::from).collect()
    }

    fn tos(v: Vec<(PathBuf, PathBuf)>) -> Vec<String> {
        v.into_iter()
            .map(|(_, t)| t.to_string_lossy().replace('\\', "/"))
            .collect()
    }

    #[test]
    fn archives_line_up_with_each_game() {
        assert_eq!(
            tos(place(
                Layout::Data,
                &p(&["Data/a.esp", "Data/textures/x.dds"]),
                "M"
            )
            .unwrap()),
            ["a.esp", "textures/x.dds"]
        );
        assert_eq!(
            tos(place(Layout::Data, &p(&["textures/x.dds", "textures/y.dds"]), "M").unwrap()),
            ["textures/x.dds", "textures/y.dds"],
            "a data folder is not a wrapper"
        );
        assert_eq!(
            tos(place(Layout::Data, &p(&["Wrap/Data/a.esp"]), "M").unwrap()),
            ["a.esp"]
        );
        for e in crate::mods::allowlist::ALLOWLIST
            .iter()
            .filter(|e| e.moddable())
        {
            assert!(layout(e.id).is_some(), "{} has no layout", e.id);
        }
        let cp = layout("cyberpunk2077").unwrap();
        assert_eq!(
            tos(place(
                cp,
                &p(&["Mod/archive/pc/mod/a.archive", "Mod/r6/scripts/b.reds"]),
                "M"
            )
            .unwrap()),
            ["archive/pc/mod/a.archive", "r6/scripts/b.reds"]
        );
        assert_eq!(
            tos(place(cp, &p(&["a.archive"]), "M").unwrap()),
            ["archive/pc/mod/a.archive"]
        );
        let w3 = layout("witcher3").unwrap();
        assert_eq!(
            tos(place(w3, &p(&["modFoo/content/x.w3s"]), "M").unwrap()),
            ["mods/modFoo/content/x.w3s"]
        );
        let sdv = layout("stardew").unwrap();
        assert_eq!(
            tos(place(sdv, &p(&["Cool/manifest.json"]), "Cool").unwrap()),
            ["Mods/Cool/manifest.json"]
        );
        assert_eq!(
            tos(place(sdv, &p(&["manifest.json", "Cool.dll"]), "Cool Mod").unwrap()),
            ["Mods/Cool Mod/manifest.json", "Mods/Cool Mod/Cool.dll"]
        );
        let wow = layout("wow").unwrap();
        assert_eq!(
            tos(place(
                wow,
                &p(&["Details/Details.toc", "Details_Data/x.lua"]),
                "Details"
            )
            .unwrap()),
            [
                "Interface/AddOns/Details/Details.toc",
                "Interface/AddOns/Details_Data/x.lua"
            ]
        );
        let mc = layout("minecraft").unwrap();
        assert_eq!(
            tos(place(mc, &p(&["sodium.jar"]), "Sodium").unwrap()),
            ["mods/sodium.jar"]
        );
        assert!(place(mc, &p(&["readme.txt"]), "x").is_err());
        let bep = layout("valheim").unwrap();
        assert_eq!(
            tos(place(
                bep,
                &p(&[
                    "manifest.json",
                    "icon.png",
                    "README.md",
                    "Cool.dll",
                    "plugins/Extra.dll",
                    "config/c.cfg"
                ]),
                "Owner-Cool"
            )
            .unwrap()),
            [
                "BepInEx/plugins/Owner-Cool/Cool.dll",
                "BepInEx/plugins/Owner-Cool/Extra.dll",
                "BepInEx/config/c.cfg"
            ]
        );
        assert_eq!(
            tos(place(
                bep,
                &p(&[
                    "BepInExPack/winhttp.dll",
                    "BepInExPack/BepInEx/core/a.dll",
                    "manifest.json"
                ]),
                "denikson-BepInExPack_Valheim"
            )
            .unwrap()),
            ["winhttp.dll", "BepInEx/core/a.dll"]
        );
        assert!(place(layout("rimworld").unwrap(), &p(&["a"]), "x")
            .unwrap_err()
            .contains("Steam"));
        assert!(place(layout("bg3").unwrap(), &p(&["a"]), "x")
            .unwrap_err()
            .contains("BG3 Mod Manager"));
        assert_eq!(mod_folder_name("Cool/Mod: v2"), "Cool_Mod_ v2");
        assert_eq!(mod_folder_name(".."), "mod");
    }
}
