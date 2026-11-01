//! The Library page: the Steam games on this computer as a cover grid, and
//! Play. Desktop only (`capabilities::Capabilities::library`).
//!
//! No-cheat: only Steam's own library files are read (`libraryfolders.vdf`,
//! each `appmanifest_*.acf`, each account's `localconfig.vdf` for play stats,
//! and the art Steam cached under `appcache/librarycache`), plus Steam's
//! public store CDN for art Steam has not cached. Nothing reads a game's
//! process, memory or files. Play hands `steam://rungameid/<digits>` to the
//! OS, which passes it to Steam; the id is a number checked against the
//! library, never a URL from the UI.
//!
//! Art: Steam's cache first, then the copy kept under `{config}/library-art/`,
//! then the CDN (public hosts only, no redirects, refused under Offline
//! only). Every cover found is kept, and a cover the CDN does not have is not
//! asked for again for a week.

use std::collections::BTreeSet;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Condvar, Mutex, PoisonError};
use std::time::Duration;

use grokhub_agent::harness as hx;
use grokhub_core::steam::{self, CoverKind, Vdf};
use serde::{Deserialize, Serialize};

/// Where covers are kept, under the config folder.
pub const ART_DIR: &str = "library-art";
/// Largest art file (or store details answer) read from Steam.
const ART_CAP: u64 = 4 * 1024 * 1024;
/// A cover the CDN did not have is asked for again after this long.
const MISS_RETRY_SECS: u64 = 7 * 24 * 60 * 60;
/// Whole request, connect to last byte.
const FETCH_TIMEOUT: Duration = Duration::from_secs(8);
/// CDN requests at once; the grid asks for many covers together.
const NET_SLOTS: usize = 3;
/// `StateFlags` bit Steam sets once every file of a game is on disk.
const FULLY_INSTALLED: u64 = 4;

/// One game on the page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Game {
    pub app_id: u32,
    pub name: String,
    /// Steam has every file; false while it downloads or updates the install.
    pub installed: bool,
    /// Unix seconds, 0 when never played.
    pub last_played: u64,
    /// Minutes played, 0 when never played.
    pub playtime_min: u64,
}

/// The whole page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Page {
    /// A Steam library folder was found on this computer.
    pub steam_found: bool,
    /// Most recently played first (`steam::rank_games`).
    pub games: Vec<Game>,
}

/// The games under a Steam root, or an empty page when there is none.
pub fn page(root: Option<&Path>) -> Page {
    let Some(root) = root else {
        return Page {
            steam_found: false,
            games: Vec::new(),
        };
    };
    let partial = partly_installed(root);
    let games = steam::read_library(root)
        .into_iter()
        .map(|g| Game {
            installed: !partial.contains(&g.app_id),
            app_id: g.app_id,
            name: g.name,
            last_played: g.last_played,
            playtime_min: g.playtime_min,
        })
        .collect();
    Page {
        steam_found: true,
        games,
    }
}

/// App ids whose manifest says some files are still missing. A manifest
/// without `StateFlags` counts as installed.
fn partly_installed(root: &Path) -> BTreeSet<u32> {
    let mut out = BTreeSet::new();
    for folder in steam::library_folders(root) {
        let Ok(dir) = std::fs::read_dir(folder.join("steamapps")) else {
            continue;
        };
        for entry in dir.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if !(name.starts_with("appmanifest_") && name.ends_with(".acf")) {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(entry.path()) else {
                continue;
            };
            let vdf = steam::parse_vdf(&text);
            let Some(state) = vdf.get("AppState") else {
                continue;
            };
            let id = state
                .get("appid")
                .and_then(Vdf::str)
                .and_then(|s| s.parse::<u32>().ok());
            let flags = state
                .get("StateFlags")
                .and_then(Vdf::str)
                .and_then(|s| s.trim().parse::<u64>().ok());
            if let (Some(id), Some(flags)) = (id, flags) {
                if flags & FULLY_INSTALLED == 0 {
                    out.insert(id);
                }
            }
        }
    }
    out
}

/// Which cover the UI wants: tall box art for the grid, the wide store
/// header for the detail column.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Shape {
    Tall,
    Wide,
}

impl Shape {
    fn kind(self) -> CoverKind {
        match self {
            Shape::Tall => CoverKind::Tall,
            Shape::Wide => CoverKind::Wide,
        }
    }
}

/// The other shape, tried when the wanted one is missing.
fn other(kind: CoverKind) -> CoverKind {
    match kind {
        CoverKind::Tall => CoverKind::Wide,
        CoverKind::Wide => CoverKind::Tall,
    }
}

/// Where the config folder keeps one cover.
pub fn art_path(config_dir: &Path, app_id: u32, kind: CoverKind) -> PathBuf {
    config_dir
        .join(ART_DIR)
        .join(format!("{app_id}-{}", kind.file()))
}

/// The note that the CDN had no cover for this game, holding when it said so.
fn miss_path(config_dir: &Path, app_id: u32, kind: CoverKind) -> PathBuf {
    config_dir
        .join(ART_DIR)
        .join(format!("{app_id}-{}.miss", kind.file()))
}

/// The image type of these bytes, from their first bytes, if they are one.
fn image_mime(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("image/jpeg")
    } else if bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) {
        Some("image/png")
    } else if bytes.len() > 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some("image/webp")
    } else {
        None
    }
}

/// An image file's bytes as a `data:` URL the page can show.
fn data_url(bytes: &[u8]) -> Option<String> {
    use base64::Engine as _;
    let mime = image_mime(bytes)?;
    Some(format!(
        "data:{mime};base64,{}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    ))
}

fn write_file(path: &Path, bytes: &[u8]) {
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let tmp = path.with_extension("tmp");
    if std::fs::write(&tmp, bytes).is_ok() && std::fs::rename(&tmp, path).is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
}

/// Keep a cover in the config folder unless the same bytes are there.
fn keep(config_dir: &Path, app_id: u32, kind: CoverKind, bytes: &[u8]) {
    let path = art_path(config_dir, app_id, kind);
    if std::fs::read(&path).is_ok_and(|kept| kept == bytes) {
        return;
    }
    write_file(&path, bytes);
}

/// A file that is really an image, as a `data:` URL.
fn read_image(path: &Path) -> Option<(Vec<u8>, String)> {
    let bytes = std::fs::read(path).ok()?;
    let url = data_url(&bytes)?;
    Some((bytes, url))
}

/// True while the CDN's "no cover" for this game is less than a week old.
fn recent_miss(config_dir: &Path, app_id: u32, kind: CoverKind, now: u64) -> bool {
    std::fs::read_to_string(miss_path(config_dir, app_id, kind))
        .ok()
        .and_then(|s| s.trim().parse::<u64>().ok())
        .is_some_and(|at| now.saturating_sub(at) < MISS_RETRY_SECS)
}

/// What one request to Steam's store gave.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fetched {
    Bytes(Vec<u8>),
    /// Steam answered that it has no such file.
    NotFound,
    /// No answer: refused, offline or cut off. Asked again next time.
    Failed(String),
}

/// The network the art comes from. The live one is Steam's public store.
pub trait ArtNet {
    fn get(&self, url: &str) -> Fetched;
}

/// One cover as a `data:` URL: Steam's cache, then the kept copy, then the
/// store CDN. `None` means the page paints its placeholder.
pub fn cover(
    config_dir: &Path,
    steam_root: Option<&Path>,
    app_id: u32,
    shape: Shape,
    net: &dyn ArtNet,
    now: u64,
) -> Option<String> {
    let kind = shape.kind();
    for k in [kind, other(kind)] {
        if let Some((bytes, url)) = steam_root
            .and_then(|root| steam::local_cover(root, app_id, k))
            .and_then(|path| read_image(&path))
        {
            keep(config_dir, app_id, k, &bytes);
            return Some(url);
        }
        if let Some((_, url)) = read_image(&art_path(config_dir, app_id, k)) {
            return Some(url);
        }
    }
    if recent_miss(config_dir, app_id, kind, now) {
        return None;
    }
    from_store(config_dir, app_id, kind, net, now)
}

/// One cover from the store, kept on disk when it is an image: the plain
/// names for both shapes, then the hashed header the store details name.
fn from_store(
    config_dir: &Path,
    app_id: u32,
    kind: CoverKind,
    net: &dyn ArtNet,
    now: u64,
) -> Option<String> {
    let mut unreachable = false;
    let found = |k: CoverKind, got: Fetched, unreachable: &mut bool| match got {
        Fetched::Bytes(bytes) => {
            let url = data_url(&bytes)?;
            keep(config_dir, app_id, k, &bytes);
            let _ = std::fs::remove_file(miss_path(config_dir, app_id, kind));
            Some(url)
        }
        Fetched::NotFound => None,
        Fetched::Failed(_) => {
            *unreachable = true;
            None
        }
    };
    for k in [kind, other(kind)] {
        if let Some(url) = found(k, net.get(&steam::cover_url(app_id, k)), &mut unreachable) {
            return Some(url);
        }
    }
    if unreachable {
        return None;
    }
    // Newer games keep their art under hashed paths the plain names miss;
    // the store's details name them (header shape only, Steam's hosts only).
    match net.get(&steam::store_details_url(app_id)) {
        Fetched::Bytes(details) => {
            for url in steam::store_art_urls(&details, app_id) {
                if let Some(art) = found(CoverKind::Wide, net.get(&url), &mut unreachable) {
                    return Some(art);
                }
            }
        }
        Fetched::NotFound => {}
        Fetched::Failed(_) => unreachable = true,
    }
    if !unreachable {
        write_file(
            &miss_path(config_dir, app_id, kind),
            now.to_string().as_bytes(),
        );
    }
    None
}

/// Steam's public store over HTTPS, a few requests at a time.
pub struct LiveNet {
    pub config_dir: PathBuf,
}

impl ArtNet for LiveNet {
    fn get(&self, url: &str) -> Fetched {
        let _slot = Slot::take();
        store_get(&self.config_dir, url)
    }
}

/// One GET: a public URL only, through the egress guard (which refuses
/// everything off this computer under Offline only), dialed through the
/// public-only resolver, with no redirects and a size cap.
pub fn store_get(config_dir: &Path, url: &str) -> Fetched {
    if !grokhub_core::public_http_url(url) {
        return Fetched::Failed("not a public address".into());
    }
    if let Err(why) = hx::guard_quiet(config_dir, &hx::EgressReq::new(url, &[])) {
        return Fetched::Failed(why);
    }
    let agent = grokhub_core::http::agent(grokhub_core::http::Opts {
        redirects: 0,
        overall: Some(FETCH_TIMEOUT),
        public_only: true,
        ..Default::default()
    });
    let resp = match agent
        .get(url)
        .header("user-agent", grokhub_agent::USER_AGENT)
        .call()
    {
        Ok(resp) => resp,
        Err(e) => {
            return match e.status() {
                Some(404 | 403 | 410) => Fetched::NotFound,
                Some(code) => Fetched::Failed(format!("HTTP {code}")),
                None => Fetched::Failed("network error".into()),
            };
        }
    };
    // A redirect is not followed, so it is no art.
    if resp.status() != 200 {
        return Fetched::NotFound;
    }
    let mut body = Vec::new();
    if resp
        .into_reader()
        .take(ART_CAP + 1)
        .read_to_end(&mut body)
        .is_err()
    {
        return Fetched::Failed("timed out or dropped".into());
    }
    if body.len() as u64 > ART_CAP {
        return Fetched::NotFound;
    }
    Fetched::Bytes(body)
}

/// A place in the [`NET_SLOTS`] CDN requests running at once.
struct Slot;

static SLOTS_USED: Mutex<usize> = Mutex::new(0);
static SLOT_FREED: Condvar = Condvar::new();

impl Slot {
    fn take() -> Slot {
        let mut used = SLOTS_USED.lock().unwrap_or_else(PoisonError::into_inner);
        while *used >= NET_SLOTS {
            used = SLOT_FREED
                .wait(used)
                .unwrap_or_else(PoisonError::into_inner);
        }
        *used += 1;
        Slot
    }
}

impl Drop for Slot {
    fn drop(&mut self) {
        let mut used = SLOTS_USED.lock().unwrap_or_else(PoisonError::into_inner);
        *used = used.saturating_sub(1);
        SLOT_FREED.notify_one();
    }
}

/// The only thing Play ever opens.
pub fn play_url(app_id: u32) -> Result<String, String> {
    if app_id == 0 {
        return Err("That isn't a Steam game.".into());
    }
    Ok(format!("steam://rungameid/{app_id}"))
}

/// The program and arguments that hand a Play URL to Steam on this OS, or
/// `None` where there is no Steam client.
pub fn launcher(os: &str, url: &str) -> Option<(&'static str, Vec<String>)> {
    let digits = url.strip_prefix("steam://rungameid/")?;
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    match os {
        // `start`'s first quoted argument is the window title.
        "windows" => Some((
            "cmd",
            vec!["/C".into(), "start".into(), String::new(), url.into()],
        )),
        "macos" => Some(("open", vec![url.into()])),
        "linux" | "freebsd" | "openbsd" | "netbsd" | "dragonfly" => {
            Some(("xdg-open", vec![url.into()]))
        }
        _ => None,
    }
}

/// Play: start a game that is in this computer's library through Steam.
pub fn play(steam_root: Option<&Path>, app_id: u32) -> Result<String, String> {
    let game = page(steam_root)
        .games
        .into_iter()
        .find(|g| g.app_id == app_id)
        .ok_or("That game isn't in this computer's Steam library.")?;
    let url = play_url(game.app_id)?;
    let (program, args) = launcher(std::env::consts::OS, &url)
        .ok_or("Steam games can't be started on this device.")?;
    let mut cmd = std::process::Command::new(program);
    cmd.args(&args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt as _;
        // CREATE_NO_WINDOW: no console flashes up for `cmd`.
        cmd.creation_flags(0x0800_0000);
    }
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("Couldn't ask Steam to start {}: {e}", game.name))?;
    // Reap the opener so it never lingers; the game is Steam's from here.
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(format!("Asked Steam to start {}.", game.name))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    const JPEG: &[u8] = &[0xFF, 0xD8, 0xFF, 0xE0, 1, 2, 3];
    const PNG: &[u8] = &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 9];

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "grokhub-shell-library-{name}-{}-{}",
            std::process::id(),
            grokhub_core::now_ms()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    fn manifest(dir: &Path, id: u32, name: &str, flags: Option<u32>) {
        let flags = flags
            .map(|f| format!("\t\"StateFlags\"\t\t\"{f}\"\n"))
            .unwrap_or_default();
        let text = format!(
            "\"AppState\"\n{{\n\t\"appid\"\t\t\"{id}\"\n\t\"name\"\t\t\"{name}\"\n{flags}}}\n"
        );
        std::fs::create_dir_all(dir.join("steamapps")).expect("steamapps");
        std::fs::write(
            dir.join("steamapps").join(format!("appmanifest_{id}.acf")),
            text,
        )
        .expect("manifest");
    }

    /// A Steam root with three installed games (one mid-download), one in a
    /// second library folder, a Proton runtime, and one account's play stats.
    fn fixture(name: &str) -> PathBuf {
        let root = temp(name);
        let second = root.join("second-drive");
        manifest(&root, 620, "Portal 2", Some(4));
        manifest(&root, 1145360, "Hades", Some(1026));
        manifest(&root, 1493710, "Proton Experimental", Some(4));
        manifest(&second, 413150, "Stardew Valley", None);
        std::fs::write(
            root.join("steamapps").join("libraryfolders.vdf"),
            format!(
                "\"libraryfolders\"\n{{\n\t\"0\"\n\t{{\n\t\t\"path\"\t\t\"{}\"\n\t}}\n\t\"1\"\n\t{{\n\t\t\"path\"\t\t\"{}\"\n\t}}\n}}\n",
                root.display(),
                second.display()
            ),
        )
        .expect("libraryfolders");
        let cfg = root.join("userdata").join("12345").join("config");
        std::fs::create_dir_all(&cfg).expect("userdata");
        std::fs::write(
            cfg.join("localconfig.vdf"),
            "\"UserLocalConfigStore\"\n{\n\"Software\"\n{\n\"Valve\"\n{\n\"Steam\"\n{\n\"apps\"\n{\n\"620\"\n{\n\"LastPlayed\"\t\"1700000000\"\n\"Playtime\"\t\"754\"\n}\n\"1145360\"\n{\n\"LastPlayed\"\t\"1600000000\"\n\"Playtime\"\t\"90\"\n}\n}\n}\n}\n}\n}\n",
        )
        .expect("localconfig");
        root
    }

    fn game(app_id: u32, name: &str, installed: bool, last: u64, min: u64) -> Game {
        Game {
            app_id,
            name: name.into(),
            installed,
            last_played: last,
            playtime_min: min,
        }
    }

    #[test]
    fn a_steam_folder_lists_its_games_with_install_state_and_play_time() {
        let root = fixture("list");
        assert_eq!(
            page(Some(&root)),
            Page {
                steam_found: true,
                games: vec![
                    game(620, "Portal 2", true, 1_700_000_000, 754),
                    game(1145360, "Hades", false, 1_600_000_000, 90),
                    game(413150, "Stardew Valley", true, 0, 0),
                ],
            }
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn no_steam_root_gives_an_empty_page() {
        assert_eq!(
            page(None),
            Page {
                steam_found: false,
                games: Vec::new(),
            }
        );
        let empty = temp("empty");
        assert_eq!(page(Some(&empty)).games, Vec::<Game>::new());
        let _ = std::fs::remove_dir_all(empty);
    }

    #[test]
    fn the_page_reaches_the_ui_in_camel_case() {
        let json = serde_json::to_value(Page {
            steam_found: true,
            games: vec![game(620, "Portal 2", true, 5, 6)],
        })
        .expect("serializes");
        assert_eq!(
            json,
            serde_json::json!({
                "steamFound": true,
                "games": [{
                    "appId": 620,
                    "name": "Portal 2",
                    "installed": true,
                    "lastPlayed": 5,
                    "playtimeMin": 6,
                }],
            })
        );
    }

    /// Answers from a fixed list and records every URL asked for.
    struct FakeNet {
        answers: Vec<(String, Fetched)>,
        asked: RefCell<Vec<String>>,
    }

    impl FakeNet {
        fn new(answers: Vec<(String, Fetched)>) -> Self {
            Self {
                answers,
                asked: RefCell::new(Vec::new()),
            }
        }
    }

    impl ArtNet for FakeNet {
        fn get(&self, url: &str) -> Fetched {
            self.asked.borrow_mut().push(url.to_string());
            self.answers
                .iter()
                .find(|(u, _)| u == url)
                .map(|(_, f)| f.clone())
                .unwrap_or(Fetched::NotFound)
        }
    }

    const TALL_620: &str =
        "https://shared.akamai.steamstatic.com/store_item_assets/steam/apps/620/library_600x900.jpg";
    const WIDE_620: &str =
        "https://shared.akamai.steamstatic.com/store_item_assets/steam/apps/620/header.jpg";
    const DETAILS_620: &str =
        "https://store.steampowered.com/api/appdetails?appids=620&filters=basic";

    #[test]
    fn steams_own_cached_cover_wins_and_is_kept() {
        let root = temp("local-root");
        let config = temp("local-config");
        let cache = root.join("appcache").join("librarycache");
        std::fs::create_dir_all(&cache).expect("cache");
        std::fs::write(cache.join("620_library_600x900.jpg"), JPEG).expect("art");
        let net = FakeNet::new(Vec::new());
        assert_eq!(
            cover(&config, Some(&root), 620, Shape::Tall, &net, 0).as_deref(),
            Some("data:image/jpeg;base64,/9j/4AECAw==")
        );
        assert_eq!(
            std::fs::read(config.join("library-art").join("620-library_600x900.jpg"))
                .expect("kept"),
            JPEG
        );
        assert_eq!(net.asked.borrow().len(), 0);
        let _ = std::fs::remove_dir_all(root);
        let _ = std::fs::remove_dir_all(config);
    }

    #[test]
    fn a_missing_cover_comes_from_the_store_and_is_cached() {
        let config = temp("store");
        let net = FakeNet::new(vec![(WIDE_620.into(), Fetched::Bytes(PNG.to_vec()))]);
        let got = cover(&config, None, 620, Shape::Tall, &net, 100);
        assert_eq!(got.as_deref(), Some("data:image/png;base64,iVBORw0KGgoJ"));
        assert_eq!(
            *net.asked.borrow(),
            vec![TALL_620.to_string(), WIDE_620.to_string()]
        );
        assert_eq!(
            std::fs::read(config.join("library-art").join("620-header.jpg")).expect("kept"),
            PNG
        );
        // The kept copy answers the next ask without the network.
        let offline = FakeNet::new(Vec::new());
        assert_eq!(
            cover(&config, None, 620, Shape::Tall, &offline, 200).as_deref(),
            Some("data:image/png;base64,iVBORw0KGgoJ")
        );
        assert_eq!(offline.asked.borrow().len(), 0);
        let _ = std::fs::remove_dir_all(config);
    }

    #[test]
    fn a_cover_the_store_lacks_is_not_asked_for_again_for_a_week() {
        let config = temp("miss");
        let net = FakeNet::new(vec![(DETAILS_620.into(), Fetched::Bytes(b"{}".to_vec()))]);
        assert_eq!(cover(&config, None, 620, Shape::Tall, &net, 1_000), None);
        assert_eq!(net.asked.borrow().len(), 3);
        let again = FakeNet::new(Vec::new());
        assert_eq!(
            cover(&config, None, 620, Shape::Tall, &again, 1_000 + 3_600),
            None
        );
        assert_eq!(again.asked.borrow().len(), 0);
        let later = FakeNet::new(Vec::new());
        assert_eq!(
            cover(
                &config,
                None,
                620,
                Shape::Tall,
                &later,
                1_000 + MISS_RETRY_SECS
            ),
            None
        );
        assert_eq!(later.asked.borrow().len(), 3);
        let _ = std::fs::remove_dir_all(config);
    }

    #[test]
    fn an_unreachable_store_is_asked_again_and_a_non_image_is_dropped() {
        let config = temp("unreachable");
        let net = FakeNet::new(vec![
            (TALL_620.into(), Fetched::Bytes(b"<html>".to_vec())),
            (WIDE_620.into(), Fetched::Failed("network error".into())),
        ]);
        assert_eq!(cover(&config, None, 620, Shape::Tall, &net, 1_000), None);
        assert!(!miss_path(&config, 620, CoverKind::Tall).exists());
        assert!(!art_path(&config, 620, CoverKind::Tall).exists());
        let _ = std::fs::remove_dir_all(config);
    }

    #[test]
    fn the_art_fetch_refuses_a_private_address() {
        let config = temp("private");
        assert_eq!(
            store_get(&config, "https://10.0.0.1/apps/620/header.jpg"),
            Fetched::Failed("not a public address".into())
        );
        assert_eq!(
            store_get(&config, "http://127.0.0.1:9/header.jpg"),
            Fetched::Failed("not a public address".into())
        );
        let _ = std::fs::remove_dir_all(config);
    }

    #[test]
    fn the_art_fetch_sends_nothing_under_offline_only() {
        let config = temp("offline");
        let _on = grokhub_core::offline::force_for_thread(true);
        assert_eq!(
            store_get(&config, WIDE_620),
            Fetched::Failed(grokhub_core::offline::OFFLINE_MSG.to_string())
        );
        let _ = std::fs::remove_dir_all(config);
    }

    #[test]
    fn play_builds_only_a_steam_run_game_url() {
        assert_eq!(play_url(620), Ok("steam://rungameid/620".to_string()));
        assert_eq!(play_url(0), Err("That isn't a Steam game.".to_string()));
        let url = "steam://rungameid/620".to_string();
        assert_eq!(
            launcher("linux", &url),
            Some(("xdg-open", vec![url.clone()]))
        );
        assert_eq!(launcher("macos", &url), Some(("open", vec![url.clone()])));
        assert_eq!(
            launcher("windows", &url),
            Some((
                "cmd",
                vec!["/C".into(), "start".into(), String::new(), url.clone()]
            ))
        );
        assert_eq!(launcher("android", &url), None);
        for bad in [
            "https://example.com/",
            "steam://rungameid/",
            "steam://rungameid/620&calc",
            "steam://install/620",
        ] {
            assert_eq!(launcher("linux", bad), None, "{bad}");
        }
    }

    #[test]
    fn play_refuses_a_game_that_is_not_in_the_library() {
        let root = fixture("play");
        assert_eq!(
            play(Some(&root), 440),
            Err("That game isn't in this computer's Steam library.".to_string())
        );
        assert_eq!(
            play(None, 620),
            Err("That game isn't in this computer's Steam library.".to_string())
        );
        let _ = std::fs::remove_dir_all(root);
    }
}
