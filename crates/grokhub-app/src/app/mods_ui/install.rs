//! Install, update and uninstall from the Mods page. Every step that writes
//! waits for a pointer click on the sheet that lists each file to add,
//! replace, delete or restore; keys, typed text and the model never reach
//! it. Downloads, hashing and unpacking run off the UI thread, and the game
//! must be closed both when the plan is made and right before the first
//! write.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::SystemTime;

use eframe::egui;
use grokhub_core::mods::install::{
    self as engine, Installer, Layout, Manifest, ModInfo, Plan, Removal,
};
use grokhub_core::mods::{archive, managers, ModFile, ModSummary, Request, Source};

use super::ModGame;

/// Why the game can't be written now ("Close Skyrim first."), asked again
/// right before the first write.
pub(super) type Running = Arc<dyn Fn() -> Option<String> + Send + Sync>;

/// What waits for the user's click.
#[derive(Clone)]
pub(super) enum Sheet {
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

/// Where the one install at a time stands.
#[derive(Clone)]
pub(super) enum Step {
    Working(String),
    Ready(Box<Sheet>),
    Done(String),
    Failed(String),
}

#[derive(Default)]
struct Work {
    step: Option<Step>,
    /// A browser download Loadout is waiting to find in Downloads.
    pickup: Option<Pickup>,
    /// Bumped whenever what is installed changes.
    generation: u64,
    recovered: bool,
    /// The mod the chat asked to set up, waiting on the Mods page.
    requested: Option<Requested>,
}

/// A mod the chat put in front of the user: one file, nothing fetched yet.
#[derive(Clone)]
pub(super) struct Requested {
    pub game: ModGame,
    pub source: Source,
    pub m: ModSummary,
    pub f: ModFile,
}

/// Show an Install card for this mod; a newer one replaces it.
pub(super) fn request(r: Requested) {
    work().requested = Some(r);
}

pub(super) fn requested() -> Option<Requested> {
    work().requested.clone()
}

pub(super) fn drop_request() {
    work().requested = None;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Pickup {
    pub key: String,
    pub file_name: String,
    pub size: u64,
    pub since: SystemTime,
}

fn work() -> std::sync::MutexGuard<'static, Work> {
    static WORK: OnceLock<Mutex<Work>> = OnceLock::new();
    WORK.get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

pub(super) fn step() -> Option<Step> {
    work().step.clone()
}

fn set(ctx: &egui::Context, step: Option<Step>) {
    let mut w = work();
    if matches!(step, Some(Step::Done(_))) {
        w.generation += 1;
    }
    w.step = step;
    drop(w);
    ctx.request_repaint();
}

pub(super) fn generation() -> u64 {
    work().generation
}

pub(super) fn busy() -> bool {
    matches!(work().step, Some(Step::Working(_)) | Some(Step::Ready(_)))
}

pub(super) fn dismiss() {
    let mut w = work();
    if !matches!(w.step, Some(Step::Working(_))) {
        w.step = None;
    }
}

pub(super) fn pickup() -> Option<Pickup> {
    work().pickup.clone()
}

/// The mod's key in Loadout's records: `source:id`.
pub(super) fn mod_key(source: Source, id: &str) -> String {
    format!("{}:{id}", source.id())
}

fn config() -> PathBuf {
    crate::config::config_dir()
}

fn staging_for(key: &str) -> PathBuf {
    let slug: String = key
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    grokhub_core::mods::cache::mods_dir(&config())
        .join("staging")
        .join(slug)
}

/// Roll back an install or uninstall a crash cut short, once per run.
pub(super) fn recover_once(ctx: &egui::Context) {
    {
        let mut w = work();
        if w.recovered {
            return;
        }
        w.recovered = true;
    }
    let ctx = ctx.clone();
    std::thread::spawn(move || {
        let lines = engine::recover(&config());
        let _ =
            std::fs::remove_dir_all(grokhub_core::mods::cache::mods_dir(&config()).join("staging"));
        if !lines.is_empty() {
            set(&ctx, Some(Step::Done(lines.join(" "))));
        }
    });
}

/// Where a game's mods go on this PC.
#[derive(Clone)]
pub(super) struct Target {
    pub layout: Layout,
    pub game_dir: PathBuf,
    pub root: PathBuf,
    pub build: Option<String>,
    pub running: Running,
}

fn wow_dir(game: &ModGame) -> Option<PathBuf> {
    game.installs.iter().find_map(|i| {
        grokhub_core::battlenet::wow_flavors(i)
            .first()
            .map(|f| f.dir(i))
    })
}

/// Why a game's mods can't be written now: the game is running.
fn running_line(name: &str, app_id: Option<u32>, game_dir: &Path, wow: bool) -> Option<String> {
    let procs = super::super::wow_ui::processes_now();
    let close = || Some(format!("Close {name} first."));
    if wow {
        return grokhub_core::battlenet::wow_running(&procs).and(close());
    }
    if let Some(id) = app_id {
        #[cfg(windows)]
        if grokhub_desktop::game_guard::steam_running_app() == Some(id) {
            return close();
        }
        if grokhub_core::game_companion::running_from_cmdlines(&procs) == Some(id) {
            return close();
        }
    }
    let dir = game_dir.to_string_lossy().to_ascii_lowercase();
    let exes = game_exes(game_dir);
    procs
        .iter()
        .any(|(name, cmdline)| {
            let n = name.to_ascii_lowercase();
            (!cmdline.is_empty() && cmdline.to_ascii_lowercase().contains(&dir))
                || exes
                    .iter()
                    .any(|e| *e == n || (n.len() >= 15 && e.starts_with(&n)))
        })
        .then(|| format!("Close {name} first."))
        .or_else(|| {
            // Minecraft runs as Java, named only by its launcher.
            (app_id.is_none()
                && procs.iter().any(|(n, c)| {
                    let n = n.to_ascii_lowercase();
                    n.starts_with("minecraft") || c.contains(".minecraft")
                }))
            .then(|| format!("Close {name} first."))
        })
}

/// The game's exe names, lowercase, from its folder and one level down.
fn game_exes(dir: &Path) -> Vec<String> {
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

fn app_data() -> (Option<PathBuf>, Option<PathBuf>) {
    let roaming = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .or_else(|| grokhub_core::user_home().map(|h| h.join(".config")));
    let local = std::env::var_os("LOCALAPPDATA").map(PathBuf::from);
    (roaming, local)
}

/// The game's own folder on this PC.
pub(super) fn game_dir(game: &ModGame, steam_root: Option<&Path>) -> Option<PathBuf> {
    let entry = game.entry;
    match (entry.steam_app_id, steam_root) {
        (Some(id), Some(root)) => grokhub_core::steam::install_dir(root, id),
        _ if entry.id == "wow" => wow_dir(game),
        _ if entry.id == "minecraft" => super::minecraft_dir(),
        _ => None,
    }
    .filter(|d| d.is_dir())
}

/// Where this game's mods go, or why Loadout won't install here.
pub(super) fn target(game: &ModGame) -> Result<Target, String> {
    let entry = game.entry;
    let layout =
        engine::layout(entry.id).ok_or("Loadout doesn't install mods for this game yet.")?;
    match layout {
        Layout::Steam => return Err("Steam installs Workshop items: subscribe in Steam.".into()),
        Layout::Manager(why) => return Err(why.into()),
        _ => {}
    }
    let steam_root = entry
        .steam_app_id
        .and_then(|_| grokhub_core::steam::detect_root());
    let wow = entry.id == "wow";
    let game_dir = game_dir(game, steam_root.as_deref())
        .ok_or_else(|| format!("Loadout couldn't find {}'s folder on this PC.", entry.name))?;
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
    let root = if layout == Layout::Data {
        game_dir.join("Data")
    } else {
        game_dir.clone()
    };
    let build = match (entry.steam_app_id, steam_root.as_deref()) {
        (Some(id), Some(r)) => grokhub_core::steam::build_id(r, id),
        _ => None,
    };
    let (name, app_id, dir) = (entry.name.to_string(), entry.steam_app_id, game_dir.clone());
    Ok(Target {
        layout,
        game_dir,
        root,
        build,
        running: Arc::new(move || running_line(&name, app_id, &dir, wow)),
    })
}

/// Why the Install button can't be offered for a game, if it can't. Read
/// once per game per run.
pub(super) fn blocked(game: &ModGame) -> Option<String> {
    static SEEN: OnceLock<Mutex<std::collections::HashMap<&'static str, Option<String>>>> =
        OnceLock::new();
    let map = SEEN.get_or_init(Default::default);
    if let Some(v) = map
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(game.entry.id)
    {
        return v.clone();
    }
    let v = target(game).err();
    map.lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(game.entry.id, v.clone());
    v
}

/// Mods Loadout installed for a game.
pub(super) fn installed(game_id: &str) -> Vec<Manifest> {
    engine::installed(&config(), Some(game_id))
}

/// Where the file comes from.
pub(super) enum Origin {
    /// Download it from the source after the click.
    Net(String),
    /// The user downloaded it in the browser.
    File(PathBuf),
}

/// Remember that the user went to download a file in the browser.
pub(super) fn wait_for_browser(source: Source, m: &ModSummary, f: &ModFile) {
    work().pickup = Some(Pickup {
        key: mod_key(source, &m.id),
        file_name: f.file_name.clone(),
        size: f.size,
        since: SystemTime::now() - std::time::Duration::from_secs(5),
    });
}

/// The browser download for a file, if it is in Downloads.
fn find_download(p: &Pickup) -> Option<PathBuf> {
    let dir = grokhub_core::mods::pickup::downloads_folder()?;
    grokhub_core::mods::pickup::find(&dir, &p.file_name, p.size, p.since)
}

/// The user says the browser download finished: take it from Downloads,
/// or say it isn't there yet.
pub(super) fn picked_up(
    ctx: &egui::Context,
    game: &ModGame,
    source: Source,
    m: &ModSummary,
    f: &ModFile,
) {
    let Some(p) = pickup().filter(|p| p.key == mod_key(source, &m.id)) else {
        return;
    };
    match find_download(&p) {
        Some(file) => start(
            ctx,
            game.clone(),
            source,
            m.clone(),
            f.clone(),
            Origin::File(file),
        ),
        None => set(
            ctx,
            Some(Step::Failed(format!(
                "{} isn't in Downloads yet. Let the download finish, then try again.",
                p.file_name
            ))),
        ),
    }
}

/// Mods Loadout installed for a game, read again whenever an install or
/// uninstall finishes.
pub(super) fn installed_now(game_id: &str) -> Vec<Manifest> {
    type Seen = std::collections::HashMap<String, (u64, Vec<Manifest>)>;
    static SEEN: OnceLock<Mutex<Seen>> = OnceLock::new();
    let current = generation();
    let mut map = SEEN
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    match map.get(game_id) {
        Some((g, rows)) if *g == current => rows.clone(),
        _ => {
            let rows = installed(game_id);
            map.insert(game_id.to_string(), (current, rows.clone()));
            rows
        }
    }
}

/// Download (or take the browser's copy), check, unpack and plan; the
/// plan then waits on the sheet.
pub(super) fn start(
    ctx: &egui::Context,
    game: ModGame,
    source: Source,
    m: ModSummary,
    f: ModFile,
    from: Origin,
) {
    if busy() {
        return;
    }
    {
        let mut w = work();
        let key = mod_key(source, &m.id);
        if w.requested
            .as_ref()
            .is_some_and(|r| mod_key(r.source, &r.m.id) == key)
        {
            w.requested = None;
        }
    }
    set(
        ctx,
        Some(Step::Working(format!("Getting {} ready…", m.name))),
    );
    if matches!(from, Origin::File(_)) {
        work().pickup = None;
    }
    let ctx = ctx.clone();
    std::thread::spawn(move || {
        let key = mod_key(source, &m.id);
        let staging = staging_for(&key);
        let step = match prepare(&ctx, &game, source, &m, &f, from, &staging) {
            Ok(sheet) => Step::Ready(Box::new(sheet)),
            Err(e) => {
                let _ = std::fs::remove_dir_all(&staging);
                Step::Failed(e)
            }
        };
        set(&ctx, Some(step));
    });
}

fn prepare(
    ctx: &egui::Context,
    game: &ModGame,
    source: Source,
    m: &ModSummary,
    f: &ModFile,
    from: Origin,
    staging: &Path,
) -> Result<Sheet, String> {
    let t = target(game)?;
    if let Some(why) = (t.running)() {
        return Err(why);
    }
    let _ = std::fs::remove_dir_all(staging);
    std::fs::create_dir_all(staging).map_err(|e| e.to_string())?;
    let file = match from {
        Origin::Net(url) => {
            set(
                ctx,
                Some(Step::Working(format!("Downloading {}…", f.file_name))),
            );
            let dest = staging.join("download");
            let req = Request::get(source, url)
                .header("User-Agent", &grokhub_core::mods::modrinth::user_agent());
            crate::mods_fetch::ModsNet
                .download(&req, &dest)
                .map_err(|e| e.line())?;
            dest
        }
        Origin::File(p) => p,
    };
    set(ctx, Some(Step::Working("Checking the download…".into())));
    let checked = archive::verify(&file, f.hash.as_ref())?;
    let unpacked = staging.join("files");
    let staged = archive::stage(&file, &f.file_name, &unpacked).map_err(|e| e.line())?;
    let folder = if t.layout == Layout::BepInEx {
        &m.id
    } else {
        &m.name
    };
    let placed = engine::place(t.layout, &staged, folder)?;
    let pack = m.id.to_ascii_lowercase().contains("bepinexpack");
    if t.layout == Layout::BepInEx && !pack && !t.game_dir.join("BepInEx").is_dir() {
        return Err("This mod needs BepInEx. Install BepInExPack for this game first.".into());
    }
    let key = mod_key(source, &m.id);
    let info = ModInfo {
        key: key.clone(),
        name: m.name.clone(),
        version: if f.version.is_empty() {
            f.name.clone()
        } else {
            f.version.clone()
        },
        build: t.build.clone(),
    };
    let plan = engine::plan(&config(), game.entry.id, &t.root, &unpacked, &placed, &info)?;
    let update = installed(game.entry.id).iter().any(|i| i.mod_key == key);
    Ok(Sheet::Install {
        plan,
        update,
        note: (!checked).then(|| {
            format!(
                "{} publishes no hash Loadout can check for this file.",
                source.label()
            )
        }),
        running: t.running,
        staging: staging.to_path_buf(),
    })
}

/// Ask what removing a mod would do; the answer waits on the sheet.
pub(super) fn start_remove(ctx: &egui::Context, game: &ModGame, m: &Manifest) {
    if busy() {
        return;
    }
    set(ctx, Some(Step::Working(format!("Checking {}…", m.name))));
    let ctx = ctx.clone();
    let game = game.clone();
    let (game_id, key, name) = (m.game.clone(), m.mod_key.clone(), m.name.clone());
    std::thread::spawn(move || {
        let running: Running = target(&game)
            .map(|t| t.running)
            .unwrap_or_else(|_| Arc::new(|| None));
        let build = target(&game).ok().and_then(|t| t.build);
        let idle = || None;
        let step = match Installer::new(&config(), &idle).preview(&game_id, &key, build.as_deref())
        {
            Ok((removal, warn)) => Step::Ready(Box::new(Sheet::Remove {
                game: game_id,
                key,
                name,
                removal,
                warn,
                running,
            })),
            Err(e) => Step::Failed(e),
        };
        set(&ctx, Some(step));
    });
}

/// The user clicked the sheet's primary button: write, on a thread.
fn confirm(ctx: &egui::Context, sheet: Sheet) {
    set(ctx, Some(Step::Working("Working…".into())));
    let ctx = ctx.clone();
    std::thread::spawn(move || {
        let cfg = config();
        let step = match sheet {
            Sheet::Install {
                plan,
                update,
                running,
                staging,
                ..
            } => {
                let check = || running();
                let inst = Installer::new(&cfg, &check);
                let now = grokhub_core::now_ms();
                let done = if update {
                    inst.update(&plan, now).map(|(_, m)| m)
                } else {
                    inst.apply(&plan, now)
                };
                let _ = std::fs::remove_dir_all(&staging);
                match done {
                    Ok(m) => Step::Done(format!(
                        "{} {} is in. Uninstall puts every file back.",
                        m.name, m.version
                    )),
                    Err(e) => Step::Failed(e),
                }
            }
            Sheet::Remove {
                game,
                key,
                name,
                running,
                ..
            } => {
                let check = || running();
                match Installer::new(&cfg, &check).uninstall(&game, &key) {
                    Ok(r) if r.kept.is_empty() => {
                        Step::Done(format!("{name} is out; the folder is as it was."))
                    }
                    Ok(r) => Step::Done(format!(
                        "{name} is out. Kept {} you changed since: {}.",
                        if r.kept.len() == 1 {
                            "one file"
                        } else {
                            "files"
                        },
                        r.kept.join(", ")
                    )),
                    Err(e) => Step::Failed(e),
                }
            }
        };
        set(&ctx, Some(step));
    });
}

fn cancel(ctx: &egui::Context, sheet: &Sheet) {
    if let Sheet::Install { staging, .. } = sheet {
        let _ = std::fs::remove_dir_all(staging);
    }
    set(ctx, None);
}

fn list(ui: &mut egui::Ui, title: &str, rows: &[String]) {
    if rows.is_empty() {
        return;
    }
    ui.label(
        egui::RichText::new(format!("{title} ({})", rows.len()))
            .size(crate::theme::TYPE_CAPTION)
            .strong()
            .color(crate::theme::muted()),
    );
    for r in rows {
        ui.label(
            egui::RichText::new(r)
                .size(crate::theme::TYPE_CAPTION)
                .monospace()
                .color(crate::theme::fg()),
        );
    }
    ui.add_space(6.0);
}

/// The sheet that lists every file and waits for a pointer click.
pub(super) fn sheet(ctx: &egui::Context) {
    let Some(Step::Ready(sheet)) = step() else {
        return;
    };
    let sheet = *sheet;
    let mut act = None;
    egui::Modal::new(egui::Id::new("mods-install-sheet"))
        .backdrop_color(egui::Color32::from_black_alpha(153))
        .frame(
            egui::Frame::NONE
                .fill(crate::theme::surface())
                .stroke(egui::Stroke::new(1.0, crate::theme::border()))
                .corner_radius(12.0)
                .inner_margin(egui::Margin::same(20)),
        )
        .show(ctx, |ui| {
            ui.set_width(560.0_f32.min(ctx.content_rect().width() - 80.0));
            let (title, primary) = match &sheet {
                Sheet::Install { plan, update, .. } => (
                    format!(
                        "{} {} {}?",
                        if *update { "Update" } else { "Install" },
                        plan.name,
                        plan.version
                    ),
                    if *update { "Update" } else { "Install" },
                ),
                Sheet::Remove { name, .. } => (format!("Uninstall {name}?"), "Uninstall"),
            };
            ui.label(
                egui::RichText::new(title)
                    .font(crate::theme::title_font(20.0))
                    .color(crate::theme::fg()),
            );
            ui.add_space(6.0);
            egui::ScrollArea::vertical()
                .max_height(320.0)
                .show(ui, |ui| match &sheet {
                    Sheet::Install {
                        plan, update, note, ..
                    } => {
                        ui.label(
                            egui::RichText::new(format!("Into {}", plan.root.display()))
                                .size(crate::theme::TYPE_CAPTION)
                                .color(crate::theme::muted()),
                        );
                        if *update {
                            list(ui, "The installed version comes out first", &[]);
                        }
                        let adds: Vec<String> = plan.adds().map(|f| f.to.clone()).collect();
                        let reps: Vec<String> = plan.replaces().map(|f| f.to.clone()).collect();
                        list(ui, "Adds", &adds);
                        list(ui, "Replaces (backed up first)", &reps);
                        if let Some(n) = note {
                            ui.label(
                                egui::RichText::new(n)
                                    .size(crate::theme::TYPE_CAPTION)
                                    .color(crate::theme::amber()),
                            );
                        }
                    }
                    Sheet::Remove { removal, warn, .. } => {
                        list(ui, "Deletes", &removal.deletes);
                        list(ui, "Puts back", &removal.restores);
                        list(ui, "Keeps (you changed these since)", &removal.kept);
                        if let Some(w) = warn {
                            ui.label(
                                egui::RichText::new(w)
                                    .size(crate::theme::TYPE_CAPTION)
                                    .color(crate::theme::amber()),
                            );
                        }
                    }
                });
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                if super::super::change_undo::click_pill(ui, primary) {
                    act = Some(true);
                }
                if super::super::change_undo::click_pill(ui, "Cancel") {
                    act = Some(false);
                }
            });
        });
    match act {
        Some(true) => confirm(ctx, sheet),
        Some(false) => cancel(ctx, &sheet),
        None => {}
    }
}

/// One line for where the work stands, under the game's name.
pub(super) fn status(ui: &mut egui::Ui) {
    let Some(step) = step() else {
        return;
    };
    let (text, color, spin) = match &step {
        Step::Working(t) => (t.clone(), crate::theme::muted(), true),
        Step::Ready(_) => (
            "Waiting for you on the sheet.".into(),
            crate::theme::muted(),
            false,
        ),
        Step::Done(t) => (t.clone(), crate::theme::amber(), false),
        Step::Failed(t) => (t.clone(), crate::theme::signal_red(), false),
    };
    ui.horizontal(|ui| {
        if spin {
            ui.spinner();
        }
        ui.label(
            egui::RichText::new(text)
                .size(crate::theme::TYPE_BODY)
                .color(color),
        );
        if !spin && !matches!(step, Step::Ready(_)) && crate::cards::ghost_pill(ui, "OK") {
            dismiss();
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_workshop_or_manager_game_offers_no_install() {
        let rail = super::super::mod_rail(
            &[grokhub_core::steam::SteamGame {
                app_id: 294100,
                name: "RimWorld".into(),
                last_played: 1,
                playtime_min: 1,
            }],
            &[],
            false,
        );
        let err = target(&rail.games[0]).err().unwrap();
        assert!(err.contains("subscribe in Steam"), "{err}");
        assert_eq!(mod_key(Source::Nexus, "12604"), "nexus:12604");
        assert!(staging_for("thunderstore:a/b").ends_with("thunderstore_a_b"));
    }

    #[test]
    fn exes_are_read_from_the_game_folder() {
        let dir = std::env::temp_dir().join(format!("mods-exes-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("bin/x64")).unwrap();
        std::fs::write(dir.join("SkyrimSE.exe"), b"").unwrap();
        std::fs::write(dir.join("bin/x64/Cyberpunk2077.exe"), b"").unwrap();
        std::fs::write(dir.join("readme.txt"), b"").unwrap();
        let mut exes = game_exes(&dir);
        exes.sort();
        assert_eq!(exes, ["cyberpunk2077.exe", "skyrimse.exe"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_sheet_lists_every_file_and_keys_never_confirm_it() {
        let ctx = egui::Context::default();
        crate::theme::install_fonts_on(&ctx);
        set(
            &ctx,
            Some(Step::Ready(Box::new(Sheet::Remove {
                game: "stardew".into(),
                key: "nexus:1".into(),
                name: "Better Crafting".into(),
                removal: Removal {
                    deletes: vec!["Mods/BetterCrafting/manifest.json".into()],
                    restores: vec![],
                    kept: vec!["Mods/BetterCrafting/config.json".into()],
                },
                warn: None,
                running: Arc::new(|| None),
            }))),
        );
        let mut shown = Vec::new();
        for frame in 0..3 {
            let mut raw = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1200.0, 800.0),
                )),
                time: Some(f64::from(frame) / 60.0),
                ..Default::default()
            };
            for key in [egui::Key::Enter, egui::Key::Space] {
                raw.events.push(egui::Event::Key {
                    key,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: Default::default(),
                });
            }
            let out = crate::theme::test_pass(&ctx, raw, |ui| sheet(ui.ctx()));
            shown.clear();
            for c in &out.shapes {
                texts(&c.shape, &mut shown);
            }
        }
        for want in [
            "Uninstall Better Crafting?",
            "Deletes (1)",
            "Mods/BetterCrafting/manifest.json",
            "Keeps (you changed these since) (1)",
        ] {
            assert!(shown.iter().any(|t| t == want), "{want}: {shown:?}");
        }
        assert!(
            matches!(step(), Some(Step::Ready(_))),
            "Enter and Space leave the sheet waiting for a click"
        );
        set(&ctx, None);
    }

    fn texts(shape: &egui::Shape, out: &mut Vec<String>) {
        match shape {
            egui::Shape::Text(t) => out.push(t.galley.job.text.clone()),
            egui::Shape::Vec(v) => v.iter().for_each(|s| texts(s, out)),
            _ => {}
        }
    }
}
