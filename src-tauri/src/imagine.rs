//! The Imagine page: images, and short videos when chosen, from a prompt.
//!
//! Generation runs the chat's own media tool (`grokhub_agent::run_media_tool`):
//! the same request builders, EgressGuard and download caps, so this page adds
//! no HTTP client of its own. It signs with chat's bearer (Sign in with Grok,
//! else the API key). Imagine's own sign-in lives in the cabin, so a missing
//! or refused bearer points there.
//!
//! Results land in `{config}/imagine/` and are pinned to the cabin's wall file
//! (`{config}/imagine-wall.json`), which is the page's gallery. The wall keeps
//! its own copy of each file in `{config}/imagine-wall/` (a hard link when the
//! disk allows), as the cabin does, so evicting a wall row never deletes the
//! original.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use grokhub_agent::ImagineApi;
use grokhub_core::{ImagineWall, WallGif};
use serde::{Deserialize, Serialize};
use serde_json::json;

/// What the page says when the key is missing or Imagine refuses it.
pub const NEED_SIGNIN: &str = "Sign in to Imagine in the cabin.";
pub const WALL_FILE: &str = "imagine-wall.json";
/// The aspect chips, square first (the default).
pub const ASPECTS: &[&str] = &["1:1", "2:3", "3:2", "9:16", "16:9"];
/// The count chips go 1 to this. Videos are always one.
pub const MAX_COUNT: u32 = 4;
pub const BUSY: &str = "Imagine is already making something. Stop it or wait for it.";
pub const STOPPED: &str = "Stopped.";
/// A wall file bigger than this is not the cabin's: it is left alone.
const WALL_CAP: u64 = 4 * 1024 * 1024;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Kind {
    #[default]
    Image,
    Video,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Request {
    pub prompt: String,
    pub aspect: String,
    pub count: u32,
    pub kind: Kind,
}

/// One gallery tile; its file is read with [`image`] by `name`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Tile {
    pub id: String,
    pub name: String,
    pub title: String,
    pub prompt: String,
    pub created_ms: u64,
    pub video: bool,
    pub tall: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Page {
    /// Newest first.
    pub tiles: Vec<Tile>,
    /// A Grok sign-in or a key is there to make something with.
    pub ready: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Made {
    pub page: Page,
    pub made: usize,
    /// Empty, or which results could not be saved.
    pub note: String,
}

pub fn media_dir(config_dir: &Path) -> PathBuf {
    config_dir.join("imagine")
}

pub fn wall_dir(config_dir: &Path) -> PathBuf {
    config_dir.join("imagine-wall")
}

fn wall_path(config_dir: &Path) -> PathBuf {
    config_dir.join(WALL_FILE)
}

fn key() -> Result<String, String> {
    crate::credential::load_key().map_err(|err| {
        if err == grokhub_core::XAI_NEED_SIGNIN {
            NEED_SIGNIN.to_string()
        } else {
            err
        }
    })
}

/// The bearer chat would send: a live Grok sign-in first, else the API key.
/// The loopback test bearer never goes to xAI.
pub fn bearer() -> Result<String, String> {
    if crate::credential::test_responses_url().is_some() {
        return Err(NEED_SIGNIN.into());
    }
    match crate::oauth::bearer(&crate::oauth::Keychain) {
        Ok(Some(bearer)) => Ok(bearer),
        Ok(None) => key(),
        Err(ended) => key().map_err(|_| ended),
    }
}

/// There is something to sign the call with. Reads the keychain only; no
/// refresh goes out.
pub fn ready() -> bool {
    crate::credential::test_responses_url().is_none()
        && (crate::oauth::signed_in(&crate::oauth::Keychain).is_some()
            || crate::credential::load_key().is_ok())
}

/// Holds the wall file between its read and its write in this process; the
/// cabin writes it whole through a rename, so a reload just before each write
/// keeps its rows.
static STORE: Mutex<()> = Mutex::new(());

fn read_wall(config_dir: &Path) -> Result<ImagineWall, String> {
    let path = wall_path(config_dir);
    match std::fs::metadata(&path) {
        Err(_) => return Ok(ImagineWall::default()),
        Ok(meta) if meta.len() > WALL_CAP => {
            return Err(format!("{WALL_FILE} is too big to read."));
        }
        Ok(_) => {}
    }
    let raw = std::fs::read_to_string(&path).map_err(|e| format!("{WALL_FILE}: {e}"))?;
    if raw.trim().is_empty() {
        return Ok(ImagineWall::default());
    }
    serde_json::from_str(&raw).map_err(|e| format!("{WALL_FILE}: {e}"))
}

fn write_wall(config_dir: &Path, wall: &ImagineWall) -> Result<(), String> {
    std::fs::create_dir_all(config_dir).map_err(|e| e.to_string())?;
    let raw = serde_json::to_string_pretty(wall).map_err(|e| e.to_string())?;
    let tmp = config_dir.join(format!("{WALL_FILE}.shell-{}.tmp", std::process::id()));
    std::fs::write(&tmp, raw).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, wall_path(config_dir)).map_err(|e| e.to_string())
}

/// The file name of `path` when it sits right inside one of the page's two
/// folders, the only places [`image`] reads from.
fn served_name(config_dir: &Path, path: &str) -> Option<String> {
    let path = Path::new(path.trim());
    let parent = path.parent()?;
    let name = path.file_name()?.to_str()?.to_string();
    let inside = [wall_dir(config_dir), media_dir(config_dir)]
        .iter()
        .any(|dir| same_dir(parent, dir));
    (inside && path.is_file()).then_some(name)
}

fn same_dir(a: &Path, b: &Path) -> bool {
    if a == b {
        return true;
    }
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

fn tiles(config_dir: &Path, wall: &ImagineWall) -> Vec<Tile> {
    let mut gifs: Vec<&WallGif> = wall.gifs.iter().collect();
    gifs.sort_by(|a, b| b.created_ms.cmp(&a.created_ms).then(b.id.cmp(&a.id)));
    gifs.into_iter()
        .filter_map(|g| {
            let name = served_name(config_dir, &g.path_a)?;
            Some(Tile {
                id: g.id.clone(),
                video: grokhub_core::imagine_is_video_path(&name),
                name,
                title: g.title.clone(),
                prompt: g.prompt.clone(),
                created_ms: g.created_ms,
                tall: g.tall,
            })
        })
        .collect()
}

/// The gallery: the wall's rows whose files are still there.
pub fn load(config_dir: &Path, ready: bool) -> Result<Page, String> {
    let wall = read_wall(config_dir)?;
    Ok(Page {
        tiles: tiles(config_dir, &wall),
        ready,
    })
}

fn friendly(err: &str) -> String {
    let err = err.trim();
    if err == grokhub_core::XAI_NEED_SIGNIN
        || err.starts_with("HTTP 401")
        || err.starts_with("HTTP 403")
    {
        NEED_SIGNIN.into()
    } else if err == "cancelled" {
        STOPPED.into()
    } else {
        err.to_string()
    }
}

fn tool_args(req: &Request) -> Result<(&'static str, serde_json::Value), String> {
    let prompt = req.prompt.trim();
    if prompt.is_empty() {
        return Err("Say what to make first.".into());
    }
    let aspect = req.aspect.trim();
    if !ASPECTS.contains(&aspect) {
        return Err(format!("Pick an aspect: {}.", ASPECTS.join(", ")));
    }
    Ok(match req.kind {
        Kind::Video => (
            "video_generate",
            json!({"prompt": prompt, "aspect_ratio": aspect}),
        ),
        Kind::Image => {
            if !(1..=MAX_COUNT).contains(&req.count) {
                return Err(format!("Make 1 to {MAX_COUNT} at a time."));
            }
            (
                "image_generate",
                json!({"prompt": prompt, "n": req.count, "aspect_ratio": aspect}),
            )
        }
    })
}

/// Runs one Imagine request through the media tool and pins what it saved.
/// `stop` is the page's Stop; a video poll checks it as chat's does.
pub fn generate(
    config_dir: &Path,
    req: &Request,
    api: &dyn ImagineApi,
    stop: &dyn Fn() -> bool,
    now_ms: u64,
) -> Result<Made, String> {
    let (tool, args) = tool_args(req)?;
    let dir = media_dir(config_dir);
    let out = grokhub_agent::run_media_tool(tool, &args, api, stop, &dir);
    if out.failed {
        return Err(friendly(&out.text));
    }
    let mut saved = Vec::new();
    let mut lost = 0;
    for line in out.text.lines() {
        if let Some(path) = line.strip_prefix("IMAGINE: ").map(str::trim) {
            if Path::new(path).starts_with(&dir) {
                saved.push(path.to_string());
            }
        } else if line.starts_with("(not saved: ") {
            lost += 1;
        }
    }
    if saved.is_empty() {
        return Err(if lost > 0 {
            "Imagine made it, but it could not be saved here.".into()
        } else {
            "Imagine sent nothing back.".into()
        });
    }
    pin(
        config_dir,
        &saved,
        req.prompt.trim(),
        req.aspect.trim(),
        now_ms,
    )?;
    let note = if lost > 0 {
        format!("{lost} of {} could not be saved.", saved.len() + lost)
    } else {
        String::new()
    };
    Ok(Made {
        page: load(config_dir, true)?,
        made: saved.len(),
        note,
    })
}

/// Hard link where the disk allows (no second copy), else a copy.
fn wall_copy(from: &Path, to: &Path) -> bool {
    std::fs::hard_link(from, to).is_ok() || std::fs::copy(from, to).is_ok()
}

/// Adds one wall row per saved file, reloading the wall first, and drops the
/// oldest past the wall's size with their wall copies.
fn pin(
    config_dir: &Path,
    saved: &[String],
    prompt: &str,
    aspect: &str,
    now_ms: u64,
) -> Result<(), String> {
    let _store = STORE.lock().unwrap_or_else(|e| e.into_inner());
    let mut wall = read_wall(config_dir)?;
    let wdir = wall_dir(config_dir);
    std::fs::create_dir_all(&wdir).map_err(|e| e.to_string())?;
    let mut at = now_ms;
    for path in saved {
        if wall.gifs.iter().any(|g| g.path_a == *path) {
            continue;
        }
        // Ids are `gen-<ms>`: a batch, or a cabin pin in the same ms, steps on.
        while wall.gifs.iter().any(|g| g.id == format!("gen-{at:x}")) {
            at += 1;
        }
        let mut gif = grokhub_core::wall_gif_from_generation(path, prompt, at, aspect);
        let ext = Path::new(path)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("png");
        let dest = wdir.join(format!("{}_a.{ext}", gif.id));
        if wall_copy(Path::new(path), &dest) {
            gif.path_a = dest.display().to_string();
            gif.path_b = gif.path_a.clone();
        }
        wall.gifs.push(gif);
        at += 1;
    }
    let (kept, evicted) =
        grokhub_core::wall_evict(std::mem::take(&mut wall.gifs), grokhub_core::WALL_GIF_MAX);
    wall.gifs = kept;
    wall.last_ms = now_ms;
    write_wall(config_dir, &wall)?;
    for old in evicted {
        for file in [&old.path_a, &old.path_b] {
            let file = Path::new(file);
            if file.parent().is_some_and(|p| same_dir(p, &wdir)) {
                let _ = std::fs::remove_file(file);
            }
        }
    }
    Ok(())
}

fn mime(name: &str) -> Option<&'static str> {
    let ext = name.rsplit_once('.')?.1.to_ascii_lowercase();
    Some(match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        _ => return None,
    })
}

/// A gallery file as a data URL, by its bare name. A name with a path in it
/// is refused before anything is read.
pub fn image(config_dir: &Path, name: &str) -> Result<String, String> {
    const NOT_OURS: &str = "Not an Imagine file.";
    let bare = !name.is_empty()
        && !name.starts_with('.')
        && !name.contains(['/', '\\', ':', '\0'])
        && !name.contains("..");
    if !bare {
        return Err(NOT_OURS.into());
    }
    let mime = mime(name).ok_or(NOT_OURS)?;
    let path = [wall_dir(config_dir), media_dir(config_dir)]
        .into_iter()
        .map(|dir| dir.join(name))
        .find(|p| p.is_file())
        .ok_or_else(|| "That file is gone.".to_string())?;
    let len = std::fs::metadata(&path).map_err(|e| e.to_string())?.len();
    if len > grokhub_core::MEDIA_FILE_CAP {
        return Err("That file is too big to show.".into());
    }
    let bytes = std::fs::read(&path).map_err(|_| "That file is gone.".to_string())?;
    use base64::Engine as _;
    Ok(format!(
        "data:{mime};base64,{}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    ))
}

/// The run in flight, so Stop reaches it. One at a time.
static RUN: Mutex<Option<Arc<AtomicBool>>> = Mutex::new(None);

pub struct Running(Arc<AtomicBool>);

impl Running {
    pub fn stopped(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        let mut run = RUN.lock().unwrap_or_else(|e| e.into_inner());
        if run.as_ref().is_some_and(|flag| Arc::ptr_eq(flag, &self.0)) {
            *run = None;
        }
    }
}

pub fn start() -> Result<Running, String> {
    let mut run = RUN.lock().unwrap_or_else(|e| e.into_inner());
    if run.is_some() {
        return Err(BUSY.into());
    }
    let flag = Arc::new(AtomicBool::new(false));
    *run = Some(Arc::clone(&flag));
    Ok(Running(flag))
}

/// Stop: the run in flight ends at its next check. `false` when none runs.
pub fn cancel() -> bool {
    match RUN.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
        Some(flag) => {
            flag.store(true, Ordering::Relaxed);
            true
        }
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use std::sync::atomic::AtomicUsize;

    const NOW: u64 = 1_800_000_000_000;

    struct Dirs {
        config: PathBuf,
        _guard: grokhub_agent::perm::ConfigGuard,
    }

    impl Drop for Dirs {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.config);
        }
    }

    fn dirs(name: &str) -> Dirs {
        let config = std::env::temp_dir().join(format!(
            "grokhub-shell-imagine-{name}-{}-{}",
            std::process::id(),
            grokhub_core::now_ms()
        ));
        let _ = std::fs::remove_dir_all(&config);
        std::fs::create_dir_all(&config).expect("cfg");
        let guard = grokhub_agent::perm::ConfigGuard::set(&config);
        Dirs {
            config,
            _guard: guard,
        }
    }

    fn png() -> Vec<u8> {
        let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();
        bytes.resize(64, 7);
        bytes
    }

    /// Imagine with no network: `n` results per image call, a video that is
    /// done at once unless `pending`, and an optional error for every post.
    #[derive(Default)]
    struct FakeImagine {
        posts: Mutex<Vec<(String, Value)>>,
        downloads: AtomicUsize,
        pending: bool,
        fail: Option<&'static str>,
    }

    impl ImagineApi for FakeImagine {
        fn post_json(&self, path: &str, body: &Value) -> Result<Value, String> {
            self.posts
                .lock()
                .unwrap()
                .push((path.to_string(), body.clone()));
            if let Some(err) = self.fail {
                return Err(err.into());
            }
            if path.starts_with("videos/") {
                return Ok(json!({"request_id": "req-1"}));
            }
            let n = body["n"].as_u64().unwrap_or(1);
            let data: Vec<Value> = (0..n)
                .map(|i| json!({"url": format!("https://imgen.x.ai/r{i}.png")}))
                .collect();
            Ok(json!({ "data": data }))
        }

        fn get_json(&self, _path: &str) -> Result<Value, String> {
            if self.pending {
                Ok(json!({"status": "pending"}))
            } else {
                Ok(json!({"status": "done", "video": {"url": "https://vidgen.x.ai/v.mp4"}}))
            }
        }

        fn download(&self, url: &str) -> Result<Vec<u8>, String> {
            self.downloads.fetch_add(1, Ordering::Relaxed);
            if url.ends_with(".mp4") {
                let mut bytes = b"\0\0\0\x18ftypmp42".to_vec();
                bytes.resize(64, 0);
                return Ok(bytes);
            }
            Ok(png())
        }
    }

    fn req(prompt: &str, count: u32, kind: Kind) -> Request {
        Request {
            prompt: prompt.into(),
            aspect: "1:1".into(),
            count,
            kind,
        }
    }

    fn files(dir: &Path) -> usize {
        std::fs::read_dir(dir).map(|d| d.count()).unwrap_or(0)
    }

    #[test]
    fn a_prompt_saves_n_files_and_n_wall_rows() {
        let d = dirs("make");
        let api = FakeImagine::default();
        let made = generate(
            &d.config,
            &req("a snowy cabin", 3, Kind::Image),
            &api,
            &|| false,
            NOW,
        )
        .expect("made");
        assert_eq!(made.made, 3);
        assert_eq!(made.note, "");
        assert!(made.page.ready);
        assert_eq!(files(&media_dir(&d.config)), 3);
        assert_eq!(files(&wall_dir(&d.config)), 3);

        let posts = api.posts.lock().unwrap();
        assert_eq!(posts.len(), 1);
        assert_eq!(posts[0].0, "images/generations");
        assert_eq!(posts[0].1["n"], 3);
        assert_eq!(posts[0].1["prompt"], "a snowy cabin");
        assert_eq!(posts[0].1["aspect_ratio"], "1:1");
        assert_eq!(api.downloads.load(Ordering::Relaxed), 3);

        let wall = read_wall(&d.config).expect("wall");
        assert_eq!(wall.gifs.len(), 3);
        assert_eq!(wall.last_ms, NOW);
        let ids: Vec<&str> = wall.gifs.iter().map(|g| g.id.as_str()).collect();
        assert_eq!(
            ids,
            ["gen-1a3185c5000", "gen-1a3185c5001", "gen-1a3185c5002"]
        );
        assert!(wall.gifs.iter().all(|g| g.title == "a snowy cabin"
            && g.prompt == "a snowy cabin"
            && !g.tall
            && g.path_a == g.path_b));

        let names: Vec<&str> = made.page.tiles.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "gen-1a3185c5002_a.png",
                "gen-1a3185c5001_a.png",
                "gen-1a3185c5000_a.png"
            ]
        );
        assert!(made.page.tiles.iter().all(|t| !t.video));
        let url = image(&d.config, "gen-1a3185c5000_a.png").expect("image");
        assert!(
            url.starts_with("data:image/png;base64,iVBORw0KGgo"),
            "{url}"
        );
    }

    #[test]
    fn a_video_is_one_mp4_tile() {
        let d = dirs("video");
        let api = FakeImagine::default();
        let mut ask = req("waves at night", 4, Kind::Video);
        ask.aspect = "9:16".into();
        let made = generate(&d.config, &ask, &api, &|| false, NOW).expect("made");
        assert_eq!(made.made, 1);
        assert_eq!(api.posts.lock().unwrap()[0].0, "videos/generations");
        let tile = &made.page.tiles[0];
        assert_eq!(tile.name, "gen-1a3185c5000_a.mp4");
        assert!(tile.video);
        assert!(tile.tall);
        assert!(image(&d.config, &tile.name)
            .unwrap()
            .starts_with("data:video/mp4;base64,"));
    }

    #[test]
    fn a_cabin_written_wall_loads() {
        let d = dirs("cabin");
        let wdir = wall_dir(&d.config);
        std::fs::create_dir_all(&wdir).unwrap();
        std::fs::write(wdir.join("gen-9_a.png"), png()).unwrap();
        let a = wdir.join("gen-9_a.png").display().to_string();
        let raw = json!({
            "lastMs": 9,
            "gifs": [
                {"id": "gen-9", "title": "ember night", "prompt": "embers in a stove",
                 "createdMs": 9, "pathA": a, "pathB": a, "tall": true},
                {"id": "gen-8", "title": "elsewhere", "prompt": "x", "createdMs": 8,
                 "pathA": "/home/user/Pictures/x.png", "pathB": "/home/user/Pictures/x.png",
                 "tall": false}
            ]
        });
        std::fs::write(d.config.join(WALL_FILE), raw.to_string()).unwrap();
        let page = load(&d.config, false).expect("page");
        assert_eq!(
            page,
            Page {
                tiles: vec![Tile {
                    id: "gen-9".into(),
                    name: "gen-9_a.png".into(),
                    title: "ember night".into(),
                    prompt: "embers in a stove".into(),
                    created_ms: 9,
                    video: false,
                    tall: true,
                }],
                ready: false,
            }
        );

        // A new pin keeps the cabin's rows.
        let api = FakeImagine::default();
        generate(
            &d.config,
            &req("frost", 1, Kind::Image),
            &api,
            &|| false,
            NOW,
        )
        .unwrap();
        let ids: Vec<String> = read_wall(&d.config)
            .unwrap()
            .gifs
            .into_iter()
            .map(|g| g.id)
            .collect();
        assert_eq!(ids, ["gen-8", "gen-9", "gen-1a3185c5000"]);
    }

    #[test]
    fn a_full_wall_drops_the_oldest_wall_copy_only() {
        let d = dirs("evict");
        let wdir = wall_dir(&d.config);
        std::fs::create_dir_all(&wdir).unwrap();
        let mut wall = ImagineWall::default();
        for i in 0..grokhub_core::WALL_GIF_MAX as u64 {
            let file = wdir.join(format!("gen-{i}_a.png"));
            std::fs::write(&file, png()).unwrap();
            let path = file.display().to_string();
            wall.gifs.push(WallGif {
                id: format!("gen-{i}"),
                title: "old".into(),
                prompt: "old".into(),
                created_ms: i + 1,
                path_a: path.clone(),
                path_b: path,
                tall: false,
            });
        }
        write_wall(&d.config, &wall).unwrap();
        let api = FakeImagine::default();
        generate(&d.config, &req("new", 1, Kind::Image), &api, &|| false, NOW).unwrap();
        let wall = read_wall(&d.config).unwrap();
        assert_eq!(wall.gifs.len(), 20);
        assert_eq!(wall.gifs[0].id, "gen-1");
        assert!(!wdir.join("gen-0_a.png").exists());
        assert!(wdir.join("gen-1_a.png").exists());
        assert_eq!(files(&media_dir(&d.config)), 1);
    }

    #[test]
    fn offline_only_refuses_before_anything_is_sent() {
        let d = dirs("offline");
        let _on = grokhub_core::offline::force_for_thread(true);
        let api = FakeImagine::default();
        let err = generate(
            &d.config,
            &req("a cabin", 1, Kind::Image),
            &api,
            &|| false,
            NOW,
        )
        .unwrap_err();
        assert_eq!(err, *grokhub_core::offline::OFFLINE_MSG);
        assert!(api.posts.lock().unwrap().is_empty());
        assert_eq!(files(&media_dir(&d.config)), 0);
        assert!(!d.config.join(WALL_FILE).exists());
    }

    #[test]
    fn a_missing_or_refused_key_points_to_the_cabin() {
        let d = dirs("signin");
        let blocked = grokhub_agent::imagine_client("  ");
        let err = generate(
            &d.config,
            &req("a cabin", 1, Kind::Image),
            blocked.as_ref(),
            &|| false,
            NOW,
        )
        .unwrap_err();
        assert_eq!(err, "Sign in to Imagine in the cabin.");
        let refused = FakeImagine {
            fail: Some("HTTP 403 {\"error\":\"scope\"}"),
            ..Default::default()
        };
        let err = generate(
            &d.config,
            &req("a cabin", 1, Kind::Image),
            &refused,
            &|| false,
            NOW,
        )
        .unwrap_err();
        assert_eq!(err, "Sign in to Imagine in the cabin.");
        let moderated = FakeImagine {
            fail: Some("image blocked by moderation"),
            ..Default::default()
        };
        let err = generate(
            &d.config,
            &req("x", 1, Kind::Image),
            &moderated,
            &|| false,
            NOW,
        )
        .unwrap_err();
        assert_eq!(err, "image blocked by moderation");
    }

    #[test]
    fn bad_asks_are_refused_before_the_call() {
        let d = dirs("bad");
        let api = FakeImagine::default();
        let mut ask = req("  ", 1, Kind::Image);
        assert_eq!(
            generate(&d.config, &ask, &api, &|| false, NOW).unwrap_err(),
            "Say what to make first."
        );
        ask.prompt = "a cabin".into();
        ask.count = 5;
        assert_eq!(
            generate(&d.config, &ask, &api, &|| false, NOW).unwrap_err(),
            "Make 1 to 4 at a time."
        );
        ask.count = 1;
        ask.aspect = "7:3".into();
        assert_eq!(
            generate(&d.config, &ask, &api, &|| false, NOW).unwrap_err(),
            "Pick an aspect: 1:1, 2:3, 3:2, 9:16, 16:9."
        );
        assert!(api.posts.lock().unwrap().is_empty());
    }

    #[test]
    fn stop_ends_a_video_poll() {
        let d = dirs("stop");
        let api = FakeImagine {
            pending: true,
            ..Default::default()
        };
        let stop_at = std::time::Instant::now() + std::time::Duration::from_millis(300);
        let err = generate(
            &d.config,
            &req("waves", 1, Kind::Video),
            &api,
            &|| std::time::Instant::now() >= stop_at,
            NOW,
        )
        .unwrap_err();
        assert_eq!(err, "Stopped.");
        assert!(!d.config.join(WALL_FILE).exists());
    }

    #[test]
    fn one_run_at_a_time_and_cancel_reaches_it() {
        assert!(!cancel());
        let run = start().expect("first");
        assert_eq!(start().err().as_deref(), Some(BUSY));
        assert!(!run.stopped());
        assert!(cancel());
        assert!(run.stopped());
        drop(run);
        assert!(!cancel());
        let again = start().expect("free again");
        assert!(!again.stopped());
    }

    #[test]
    fn imagine_image_refuses_names_with_paths() {
        let d = dirs("names");
        std::fs::create_dir_all(media_dir(&d.config)).unwrap();
        std::fs::write(d.config.join("secret.png"), png()).unwrap();
        for name in [
            "../secret.png",
            "imagine/../secret.png",
            "a/b.png",
            "a\\b.png",
            "C:secret.png",
            ".secret.png",
            "",
            "notes.txt",
        ] {
            assert_eq!(
                image(&d.config, name).unwrap_err(),
                "Not an Imagine file.",
                "{name}"
            );
        }
        assert_eq!(
            image(&d.config, "gone.png").unwrap_err(),
            "That file is gone."
        );
    }
}
