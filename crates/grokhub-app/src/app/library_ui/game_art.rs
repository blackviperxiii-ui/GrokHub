//! Real cover art for the Library. A cover comes from Steam's own cache on
//! this PC first, then from the copy kept under the config folder, and only
//! then from Steam's public store CDN (logged in the off-device ledger). Every
//! cover found is kept in the config folder, so it survives Steam pruning its
//! own cache, and a cover the CDN does not have is not asked for again for a
//! week. A few worker threads read and decode covers, disk hits before any
//! network fetch; the paint path only uploads ready pixels. Covers from disk
//! appear at once, and only a freshly fetched cover fades in. A game with no
//! art keeps the painted cover.

use std::collections::{HashMap, HashSet, VecDeque};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use grokhub_core::steam::{self, CoverKind};

use super::super::*;

/// Largest art file read from the CDN.
const ART_CAP: u64 = 4 * 1024 * 1024;
/// How long a freshly fetched cover takes to fade in.
const FADE_SECS: f32 = 0.25;
const ART_DIR: &str = "game-art";
const ART_SLOTS_ID: &str = "library-game-art";
/// Art threads at most. Disk reads and decodes run side by side.
const WORKERS: usize = 3;
/// A cover the CDN did not have is asked for again after this long.
const MISS_RETRY_SECS: u64 = 7 * 24 * 60 * 60;

type Key = (u32, CoverKind);

/// Where the config folder keeps one cover.
pub(in crate::app) fn art_path(config_dir: &Path, app_id: u32, kind: CoverKind) -> PathBuf {
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

/// The other shape, tried when the wanted one is missing.
fn other(kind: CoverKind) -> CoverKind {
    match kind {
        CoverKind::Tall => CoverKind::Wide,
        CoverKind::Wide => CoverKind::Tall,
    }
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Keep a cover's bytes in the config folder unless the same bytes are there.
fn keep(config_dir: &Path, app_id: u32, kind: CoverKind, bytes: &[u8]) {
    let path = art_path(config_dir, app_id, kind);
    if std::fs::read(&path).is_ok_and(|kept| kept == bytes) {
        return;
    }
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = crate::config::atomic_write(&path, bytes);
}

/// True while the CDN's "no cover" for this game is less than a week old.
fn recent_miss(config_dir: &Path, app_id: u32, kind: CoverKind, now: u64) -> bool {
    std::fs::read_to_string(miss_path(config_dir, app_id, kind))
        .ok()
        .and_then(|s| s.trim().parse::<u64>().ok())
        .is_some_and(|at| now.saturating_sub(at) < MISS_RETRY_SECS)
}

/// A file that really decodes, so a cut-off or corrupt file never hides a good one.
fn read_image(path: &Path) -> Option<(Vec<u8>, image::DynamicImage)> {
    let bytes = std::fs::read(path).ok()?;
    let img = image::load_from_memory(&bytes).ok()?;
    Some((bytes, img))
}

/// One cover from this PC: Steam's cache first, then the kept copy. A Steam
/// hit is copied into the config folder so it outlives Steam's own cache.
pub(in crate::app) fn resolve_local(
    app_id: u32,
    kind: CoverKind,
    steam_root: Option<&Path>,
    config_dir: &Path,
) -> Option<(image::DynamicImage, CoverKind)> {
    for k in [kind, other(kind)] {
        if let Some((bytes, img)) = steam_root
            .and_then(|root| steam::local_cover(root, app_id, k))
            .and_then(|path| read_image(&path))
        {
            keep(config_dir, app_id, k, &bytes);
            return Some((img, k));
        }
        if let Some((_, img)) = read_image(&art_path(config_dir, app_id, k)) {
            return Some((img, k));
        }
    }
    None
}

/// What one CDN request gave.
pub(in crate::app) enum Fetched {
    Bytes(Vec<u8>),
    /// The CDN answered that it has no such file.
    NotFound,
    /// No answer: offline, refused or cut off. Not a reason to stop asking.
    Failed,
}

/// What the network side found for one cover.
pub(in crate::app) enum NetArt {
    Found(image::DynamicImage, CoverKind),
    /// The CDN has neither shape; noted so the next launches skip it for a week.
    Missing,
    /// The CDN could not be reached; asked again next launch.
    Unreachable,
}

/// One cover from Steam's store CDN, kept on disk when it decodes.
pub(in crate::app) fn resolve_net(
    app_id: u32,
    kind: CoverKind,
    config_dir: &Path,
    now: u64,
    fetch: &mut dyn FnMut(&str) -> Fetched,
) -> NetArt {
    let mut unreachable = false;
    for k in [kind, other(kind)] {
        match fetch(&steam::cover_url(app_id, k)) {
            Fetched::Bytes(bytes) => {
                if let Ok(img) = image::load_from_memory(&bytes) {
                    keep(config_dir, app_id, k, &bytes);
                    let _ = std::fs::remove_file(miss_path(config_dir, app_id, kind));
                    return NetArt::Found(img, k);
                }
            }
            Fetched::NotFound => {}
            Fetched::Failed => unreachable = true,
        }
    }
    if unreachable {
        return NetArt::Unreachable;
    }
    let path = miss_path(config_dir, app_id, kind);
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = crate::config::atomic_write(&path, now.to_string().as_bytes());
    NetArt::Missing
}

/// One cover's bytes and the shape they are. Every file on this PC is
/// tried before the network; a fetched file that decodes is kept on disk.
#[cfg(test)]
pub(in crate::app) fn resolve_art(
    app_id: u32,
    kind: CoverKind,
    steam_root: Option<&Path>,
    config_dir: &Path,
    fetch: &mut dyn FnMut(&str) -> Fetched,
) -> Option<CoverKind> {
    if let Some((_, k)) = resolve_local(app_id, kind, steam_root, config_dir) {
        return Some(k);
    }
    if recent_miss(config_dir, app_id, kind, now_secs()) {
        return None;
    }
    match resolve_net(app_id, kind, config_dir, now_secs(), fetch) {
        NetArt::Found(_, k) => Some(k),
        NetArt::Missing | NetArt::Unreachable => None,
    }
}

/// The part of an image that fills a box without stretching (cover fit).
pub(in crate::app) fn cover_uv(image: [usize; 2], slot: egui::Vec2) -> egui::Rect {
    let img = image[0] as f32 / (image[1] as f32).max(1.0);
    let want = slot.x / slot.y.max(1.0);
    if img > want {
        let w = want / img;
        egui::Rect::from_min_max(
            egui::pos2(0.5 - w / 2.0, 0.0),
            egui::pos2(0.5 + w / 2.0, 1.0),
        )
    } else {
        let h = img / want;
        egui::Rect::from_min_max(
            egui::pos2(0.0, 0.5 - h / 2.0),
            egui::pos2(1.0, 0.5 + h / 2.0),
        )
    }
}

/// A decoded cover, and whether it came from this PC (drawn at once) or the CDN.
struct Done {
    art: Option<(image::RgbaImage, CoverKind)>,
    from_disk: bool,
}

/// Work for the art threads, and what they finished.
#[derive(Default)]
struct Jobs {
    /// New covers: look on this PC first.
    disk: VecDeque<Key>,
    /// Covers not on this PC: ask the CDN, after every disk job.
    net: VecDeque<Key>,
    /// Being read or fetched right now, so a repaint never queues it twice.
    in_flight: HashSet<Key>,
    workers: usize,
    ready: HashMap<Key, Done>,
}

/// Which step a job is at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stage {
    Disk,
    Net,
}

impl Jobs {
    /// Queue a cover unless it is already queued, running or done. True when queued.
    fn enqueue(&mut self, key: Key) -> bool {
        if self.ready.contains_key(&key)
            || self.in_flight.contains(&key)
            || self.disk.contains(&key)
            || self.net.contains(&key)
        {
            return false;
        }
        self.disk.push_back(key);
        true
    }

    /// The next job, disk work first. `None` retires the asking worker.
    fn next(&mut self) -> Option<(Key, Stage)> {
        let job = match self.disk.pop_front() {
            Some(key) => Some((key, Stage::Disk)),
            None => self.net.pop_front().map(|key| (key, Stage::Net)),
        };
        match job {
            Some((key, _)) => {
                self.in_flight.insert(key);
            }
            None => self.workers = self.workers.saturating_sub(1),
        }
        job
    }

    fn finish(&mut self, key: Key, done: Option<Done>) {
        self.in_flight.remove(&key);
        match done {
            Some(done) => {
                self.ready.insert(key, done);
            }
            None => self.net.push_back(key),
        }
    }
}

fn jobs() -> &'static Mutex<Jobs> {
    static J: OnceLock<Mutex<Jobs>> = OnceLock::new();
    J.get_or_init(Mutex::default)
}

fn fetch_agent() -> grokhub_core::http::Agent {
    // Same rules as Pulse images: no redirects, no env proxy, and the dial
    // refuses a name that resolves to a private address.
    grokhub_core::http::agent(grokhub_core::http::Opts {
        redirects: 0,
        overall: Some(std::time::Duration::from_secs(8)),
        public_only: true,
        ..Default::default()
    })
}

fn fetch_art(url: &str) -> Fetched {
    if cfg!(test) || !grokhub_core::public_http_url(url) {
        return Fetched::Failed;
    }
    if crate::xai::egress_ok(url, &[]).is_err() {
        return Fetched::Failed;
    }
    let resp = match fetch_agent().get(url).call() {
        Ok(resp) => resp,
        Err(grokhub_core::http::Error::Status { code, .. }) if (400..500).contains(&code) => {
            return Fetched::NotFound
        }
        Err(_) => return Fetched::Failed,
    };
    let mut buf = Vec::new();
    match resp.into_reader().take(ART_CAP).read_to_end(&mut buf) {
        Ok(_) => Fetched::Bytes(buf),
        Err(_) => Fetched::Failed,
    }
}

fn decode(img: image::DynamicImage, kind: CoverKind) -> image::RgbaImage {
    let (w, h) = match kind {
        CoverKind::Tall => (600, 900),
        CoverKind::Wide => (460, 215),
    };
    let img = if img.width() > w || img.height() > h {
        img.thumbnail(w, h)
    } else {
        img
    };
    img.to_rgba8()
}

/// One step of one job. `None` from the disk step sends it on to the CDN.
fn run_job(
    (app_id, kind): Key,
    stage: Stage,
    steam_root: Option<&Path>,
    config_dir: &Path,
    fetch: &mut dyn FnMut(&str) -> Fetched,
) -> Option<Done> {
    let now = now_secs();
    match stage {
        Stage::Disk => {
            if let Some((img, got)) = resolve_local(app_id, kind, steam_root, config_dir) {
                return Some(Done {
                    art: Some((decode(img, got), got)),
                    from_disk: true,
                });
            }
            if recent_miss(config_dir, app_id, kind, now) {
                return Some(Done {
                    art: None,
                    from_disk: true,
                });
            }
            None
        }
        Stage::Net => {
            let art = match resolve_net(app_id, kind, config_dir, now, fetch) {
                NetArt::Found(img, got) => Some((decode(img, got), got)),
                NetArt::Missing | NetArt::Unreachable => None,
            };
            Some(Done {
                art,
                from_disk: false,
            })
        }
    }
}

/// Queue a cover, and start another art thread when there is room.
fn request(ctx: &egui::Context, key: Key) {
    let Ok(mut j) = jobs().lock() else {
        return;
    };
    if !j.enqueue(key) || j.workers >= WORKERS {
        return;
    }
    j.workers += 1;
    let ctx = ctx.clone();
    std::thread::spawn(move || {
        let root = steam::detect_root();
        let dir = crate::config::config_dir();
        loop {
            let Some((key, stage)) = jobs().lock().ok().and_then(|mut j| j.next()) else {
                return;
            };
            let done = run_job(key, stage, root.as_deref(), &dir, &mut fetch_art);
            let shown = done.is_some();
            if let Ok(mut j) = jobs().lock() {
                j.finish(key, done);
            }
            if shown {
                ctx.request_repaint();
            }
        }
    });
}

#[derive(Clone)]
enum Slot {
    Loading,
    Ready(egui::TextureHandle, Instant),
    Missing,
}

/// A cover ready to draw: the texture and how far it has faded in.
pub(in crate::app) struct Cover {
    pub tex: egui::TextureHandle,
    pub alpha: f32,
}

/// The cover for one game, or `None` while it loads or when it has none.
pub(in crate::app) fn game_cover(
    ctx: &egui::Context,
    app_id: u32,
    kind: CoverKind,
) -> Option<Cover> {
    let id = egui::Id::new(ART_SLOTS_ID);
    let key = (app_id, kind);
    let mut slots: HashMap<Key, Slot> = ctx.data(|d| d.get_temp(id)).unwrap_or_default();
    let slot = match slots.get(&key).cloned() {
        Some(Slot::Loading) | None => {
            let done = jobs().lock().ok().and_then(|mut j| j.ready.remove(&key));
            let slot = match done {
                None => {
                    request(ctx, key);
                    Slot::Loading
                }
                Some(Done { art: None, .. }) => Slot::Missing,
                Some(Done {
                    art: Some((img, got)),
                    from_disk,
                }) => {
                    let size = [img.width() as usize, img.height() as usize];
                    let tex = ctx.load_texture(
                        format!("game-art-{app_id}-{}", got.file()),
                        egui::ColorImage::from_rgba_unmultiplied(size, img.as_raw()),
                        egui::TextureOptions::LINEAR,
                    );
                    // A cover from this PC is drawn at once; only a fetched one fades in.
                    let now = Instant::now();
                    let shown = if from_disk {
                        now.checked_sub(Duration::from_secs_f32(FADE_SECS))
                            .unwrap_or(now)
                    } else {
                        now
                    };
                    Slot::Ready(tex, shown)
                }
            };
            slots.insert(key, slot.clone());
            ctx.data_mut(|d| d.insert_temp(id, slots));
            slot
        }
        Some(slot) => slot,
    };
    match slot {
        Slot::Ready(tex, at) => {
            let alpha = (at.elapsed().as_secs_f32() / FADE_SECS).min(1.0);
            if alpha < 1.0 {
                ctx.request_repaint();
            }
            Some(Cover { tex, alpha })
        }
        Slot::Loading | Slot::Missing => None,
    }
}

/// The art over the painted cover, cover-fit and rounded, faded by `alpha`,
/// with a dark scrim at the bottom so the title overlay stays readable.
pub(in crate::app) fn paint_art(ui: &egui::Ui, rect: egui::Rect, cover: &Cover, radius: f32) {
    let tint = egui::Color32::from_white_alpha((cover.alpha * 255.0).round() as u8);
    egui::Image::from_texture(&cover.tex)
        .uv(cover_uv(cover.tex.size(), rect.size()))
        .corner_radius(radius)
        .tint(tint)
        .paint_at(ui, rect);
    let top = egui::pos2(rect.left(), rect.bottom() - rect.height() * 0.4);
    let shade = egui::Color32::from_black_alpha((cover.alpha * 200.0).round() as u8);
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(top, egui::Color32::TRANSPARENT);
    mesh.colored_vertex(egui::pos2(rect.right(), top.y), egui::Color32::TRANSPARENT);
    mesh.colored_vertex(rect.right_bottom(), shade);
    mesh.colored_vertex(rect.left_bottom(), shade);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    ui.painter_at(rect).add(egui::Shape::mesh(mesh));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn jpeg() -> Vec<u8> {
        let img = image::RgbImage::from_pixel(6, 9, image::Rgb([200, 40, 60]));
        let mut out = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgb8(img)
            .write_to(&mut out, image::ImageFormat::Jpeg)
            .unwrap();
        out.into_inner()
    }

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("gh-art-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn bytes(b: Vec<u8>) -> Fetched {
        Fetched::Bytes(b)
    }

    #[test]
    fn steams_own_cache_wins_and_the_network_is_last() {
        let root = temp("root");
        let cfg = temp("cfg");
        let cache = root.join("appcache/librarycache/570");
        std::fs::create_dir_all(&cache).unwrap();
        std::fs::write(cache.join("library_600x900.jpg"), jpeg()).unwrap();
        let asked = std::cell::RefCell::new(Vec::<String>::new());
        let mut fetch = |url: &str| {
            asked.borrow_mut().push(url.to_string());
            bytes(jpeg())
        };
        let got = resolve_art(570, CoverKind::Tall, Some(&root), &cfg, &mut fetch);
        assert_eq!(got, Some(CoverKind::Tall));
        // A game Steam has only the header for: the local header beats the CDN.
        std::fs::write(root.join("appcache/librarycache/730_header.jpg"), jpeg()).unwrap();
        let got = resolve_art(730, CoverKind::Tall, Some(&root), &cfg, &mut fetch);
        assert_eq!(got, Some(CoverKind::Wide));
        assert!(asked.borrow().is_empty(), "{asked:?}");
        // Nothing on this PC: the CDN's tall art, kept for next time.
        let got = resolve_art(1245620, CoverKind::Tall, None, &cfg, &mut fetch);
        assert_eq!(got, Some(CoverKind::Tall));
        assert_eq!(
            *asked.borrow(),
            vec!["https://shared.akamai.steamstatic.com/store_item_assets/steam/apps/1245620/library_600x900.jpg"]
        );
        assert!(art_path(&cfg, 1245620, CoverKind::Tall).is_file());
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&cfg);
    }

    #[test]
    fn a_second_resolve_makes_no_network_call() {
        let cfg = temp("second");
        let calls = std::cell::Cell::new(0);
        let mut fetch = |_: &str| {
            calls.set(calls.get() + 1);
            bytes(jpeg())
        };
        assert_eq!(
            resolve_art(42, CoverKind::Tall, None, &cfg, &mut fetch),
            Some(CoverKind::Tall)
        );
        assert_eq!(
            resolve_art(42, CoverKind::Tall, None, &cfg, &mut fetch),
            Some(CoverKind::Tall)
        );
        assert_eq!(calls.get(), 1);
        let _ = std::fs::remove_dir_all(&cfg);
    }

    #[test]
    fn a_steam_cache_hit_is_copied_so_it_outlives_steams_cache() {
        let root = temp("copy-root");
        let cfg = temp("copy-cfg");
        let cache = root.join("appcache/librarycache");
        std::fs::create_dir_all(&cache).unwrap();
        std::fs::write(cache.join("440_library_600x900.jpg"), jpeg()).unwrap();
        assert!(resolve_local(440, CoverKind::Tall, Some(&root), &cfg).is_some());
        assert_eq!(
            std::fs::read(art_path(&cfg, 440, CoverKind::Tall)).unwrap(),
            jpeg()
        );
        // Steam prunes its cache: the kept copy still answers, with no network.
        std::fs::remove_dir_all(&root).unwrap();
        let got = resolve_art(440, CoverKind::Tall, Some(&root), &cfg, &mut |_| {
            panic!("no network for a kept cover")
        });
        assert_eq!(got, Some(CoverKind::Tall));
        let _ = std::fs::remove_dir_all(&cfg);
    }

    #[test]
    fn a_corrupt_file_never_hides_good_art() {
        let root = temp("corrupt-root");
        let cfg = temp("corrupt-cfg");
        let cache = root.join("appcache/librarycache");
        std::fs::create_dir_all(&cache).unwrap();
        // Right magic bytes, cut off: it looks like a JPEG but does not decode.
        let cut = jpeg()[..12].to_vec();
        std::fs::write(cache.join("77_library_600x900.jpg"), &cut).unwrap();
        std::fs::create_dir_all(cfg.join(ART_DIR)).unwrap();
        std::fs::write(art_path(&cfg, 77, CoverKind::Tall), &cut).unwrap();
        let got = resolve_art(77, CoverKind::Tall, Some(&root), &cfg, &mut |_| {
            bytes(jpeg())
        });
        assert_eq!(got, Some(CoverKind::Tall));
        assert_eq!(
            std::fs::read(art_path(&cfg, 77, CoverKind::Tall)).unwrap(),
            jpeg()
        );
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&cfg);
    }

    #[test]
    fn offline_or_bad_art_falls_back_to_the_painted_cover() {
        let cfg = temp("offline");
        let mut tried = 0;
        let got = resolve_art(1001, CoverKind::Tall, None, &cfg, &mut |_| {
            tried += 1;
            Fetched::Failed
        });
        assert!(got.is_none());
        assert_eq!(tried, 2, "tall art, then the header");
        // Offline is not a miss: the next launch asks again.
        assert!(!miss_path(&cfg, 1001, CoverKind::Tall).exists());
        let got = resolve_art(1001, CoverKind::Wide, None, &cfg, &mut |_| {
            bytes(b"<html>not found</html>".to_vec())
        });
        assert!(got.is_none());
        assert!(!art_path(&cfg, 1001, CoverKind::Wide).exists());
        let _ = std::fs::remove_dir_all(&cfg);
    }

    #[test]
    fn a_cover_the_cdn_lacks_is_skipped_for_a_week_then_asked_again() {
        let cfg = temp("miss");
        let calls = std::cell::Cell::new(0);
        let mut fetch = |_: &str| {
            calls.set(calls.get() + 1);
            Fetched::NotFound
        };
        assert_eq!(
            resolve_art(9, CoverKind::Tall, None, &cfg, &mut fetch),
            None
        );
        assert_eq!(calls.get(), 2);
        assert_eq!(
            resolve_art(9, CoverKind::Tall, None, &cfg, &mut fetch),
            None
        );
        assert_eq!(calls.get(), 2, "a fresh miss is not asked again");
        let eight_days = now_secs() - 8 * 24 * 60 * 60;
        std::fs::write(miss_path(&cfg, 9, CoverKind::Tall), eight_days.to_string()).unwrap();
        let got = resolve_art(9, CoverKind::Tall, None, &cfg, &mut |_| bytes(jpeg()));
        assert_eq!(got, Some(CoverKind::Tall));
        assert!(!miss_path(&cfg, 9, CoverKind::Tall).exists());
        let _ = std::fs::remove_dir_all(&cfg);
    }

    #[test]
    fn a_cover_in_flight_is_never_queued_twice_and_disk_work_goes_first() {
        let mut j = Jobs::default();
        let a = (1, CoverKind::Tall);
        let b = (2, CoverKind::Tall);
        assert!(j.enqueue(a));
        assert!(!j.enqueue(a), "already queued");
        assert_eq!(j.next(), Some((a, Stage::Disk)));
        assert!(!j.enqueue(a), "in flight");
        // Not on this PC: it moves to the network queue, behind new disk work.
        j.finish(a, None);
        assert!(!j.enqueue(a), "waiting for the network");
        assert!(j.enqueue(b));
        assert_eq!(j.next(), Some((b, Stage::Disk)));
        assert_eq!(j.next(), Some((a, Stage::Net)));
        j.finish(
            a,
            Some(Done {
                art: None,
                from_disk: false,
            }),
        );
        assert!(!j.enqueue(a), "done and waiting to be drawn");
        assert!(j.ready.remove(&a).is_some());
        assert!(j.ready.is_empty());
    }

    #[test]
    fn a_disk_job_decodes_once_and_marks_the_cover_from_disk() {
        let cfg = temp("job");
        std::fs::create_dir_all(cfg.join(ART_DIR)).unwrap();
        std::fs::write(art_path(&cfg, 5, CoverKind::Tall), jpeg()).unwrap();
        let done = run_job((5, CoverKind::Tall), Stage::Disk, None, &cfg, &mut |_| {
            panic!("no network")
        })
        .unwrap();
        assert!(done.from_disk);
        assert_eq!(
            done.art.map(|(img, k)| (img.dimensions(), k)),
            Some(((6, 9), CoverKind::Tall))
        );
        // Nothing on disk and no miss noted: on to the network.
        assert!(
            run_job((6, CoverKind::Tall), Stage::Disk, None, &cfg, &mut |_| {
                panic!("disk step never fetches")
            })
            .is_none()
        );
        let _ = std::fs::remove_dir_all(&cfg);
    }

    #[test]
    fn covers_fill_the_box_without_stretching() {
        // 600x900 art in a 150x228 tile: a thin crop off each side.
        let uv = cover_uv([600, 900], egui::vec2(150.0, 228.0));
        assert!((uv.width() - (150.0 / 228.0) / (600.0 / 900.0)).abs() < 1e-4);
        assert_eq!((uv.top(), uv.bottom()), (0.0, 1.0));
        assert!((uv.center().x - 0.5).abs() < 1e-6);
        // A 460x215 header in a tall tile: full height, middle slice.
        let uv = cover_uv([460, 215], egui::vec2(150.0, 228.0));
        assert_eq!((uv.top(), uv.bottom()), (0.0, 1.0));
        assert!(uv.width() < 0.31 && uv.width() > 0.30, "{uv:?}");
        // A tall cover in a wide box: full width, middle band.
        let uv = cover_uv([600, 900], egui::vec2(320.0, 192.0));
        assert_eq!((uv.left(), uv.right()), (0.0, 1.0));
        assert!((uv.center().y - 0.5).abs() < 1e-6);
    }
}
