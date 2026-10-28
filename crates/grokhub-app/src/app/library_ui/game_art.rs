//! Real cover art for the Library. A cover comes from Steam's own cache on
//! this PC first, then from the copy kept under the config folder, and only
//! then from Steam's public store CDN (logged in the off-device ledger). One
//! worker thread fetches and decodes, one cover at a time; the paint path
//! only uploads ready pixels and fades them in. A game with no art keeps the
//! painted cover.

use std::collections::{HashMap, VecDeque};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

use grokhub_core::steam::{self, CoverKind};

use super::super::*;

/// Largest art file read from the CDN.
const ART_CAP: u64 = 4 * 1024 * 1024;
/// How long a cover takes to fade in.
const FADE_SECS: f32 = 0.25;
const ART_DIR: &str = "game-art";
const ART_SLOTS_ID: &str = "library-game-art";

type Key = (u32, CoverKind);

/// Where the config folder keeps one cover.
pub(in crate::app) fn art_path(config_dir: &Path, app_id: u32, kind: CoverKind) -> PathBuf {
    config_dir
        .join(ART_DIR)
        .join(format!("{app_id}-{}", kind.file()))
}

/// The other shape, tried when the wanted one is missing.
fn other(kind: CoverKind) -> CoverKind {
    match kind {
        CoverKind::Tall => CoverKind::Wide,
        CoverKind::Wide => CoverKind::Tall,
    }
}

/// One cover's bytes and the shape they are. Every file on this PC is
/// tried before the network; a fetched file that decodes is kept on disk.
pub(in crate::app) fn resolve_art(
    app_id: u32,
    kind: CoverKind,
    steam_root: Option<&Path>,
    config_dir: &Path,
    fetch: &mut dyn FnMut(&str) -> Option<Vec<u8>>,
) -> Option<(Vec<u8>, CoverKind)> {
    for k in [kind, other(kind)] {
        let local = steam_root.and_then(|root| steam::local_cover(root, app_id, k));
        for path in local.into_iter().chain([art_path(config_dir, app_id, k)]) {
            if let Ok(bytes) = std::fs::read(&path) {
                if image::guess_format(&bytes).is_ok() {
                    return Some((bytes, k));
                }
            }
        }
    }
    for k in [kind, other(kind)] {
        let Some(bytes) = fetch(&steam::cover_url(app_id, k)) else {
            continue;
        };
        if image::load_from_memory(&bytes).is_err() {
            continue;
        }
        let path = art_path(config_dir, app_id, k);
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = crate::config::atomic_write(&path, &bytes);
        return Some((bytes, k));
    }
    None
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

/// Work for the one art thread, and what it finished.
#[derive(Default)]
struct Jobs {
    queue: VecDeque<Key>,
    running: bool,
    /// `None` when no art was found or it did not decode.
    ready: HashMap<Key, Option<(image::RgbaImage, CoverKind)>>,
}

fn jobs() -> &'static Mutex<Jobs> {
    static J: OnceLock<Mutex<Jobs>> = OnceLock::new();
    J.get_or_init(Mutex::default)
}

fn fetch_agent() -> ureq::Agent {
    // Same rules as Pulse images: no redirects, no env proxy, and the dial
    // refuses a name that resolves to a private address.
    ureq::AgentBuilder::new()
        .redirects(0)
        .timeout(std::time::Duration::from_secs(8))
        .resolver(grokhub_agent::public_only_resolve)
        .build()
}

fn fetch_art(url: &str) -> Option<Vec<u8>> {
    if cfg!(test) || !grokhub_core::public_http_url(url) {
        return None;
    }
    crate::xai::egress_ok(url, &[]).ok()?;
    let resp = fetch_agent().get(url).call().ok()?;
    let mut buf = Vec::new();
    resp.into_reader()
        .take(ART_CAP)
        .read_to_end(&mut buf)
        .ok()?;
    Some(buf)
}

fn decode(bytes: &[u8], kind: CoverKind) -> Option<image::RgbaImage> {
    let img = image::load_from_memory(bytes).ok()?;
    let (w, h) = match kind {
        CoverKind::Tall => (600, 900),
        CoverKind::Wide => (460, 215),
    };
    let img = if img.width() > w || img.height() > h {
        img.thumbnail(w, h)
    } else {
        img
    };
    Some(img.to_rgba8())
}

/// Queue a cover, and start the art thread when it is idle.
fn request(ctx: &egui::Context, key: Key) {
    let Ok(mut j) = jobs().lock() else {
        return;
    };
    if j.ready.contains_key(&key) || j.queue.contains(&key) {
        return;
    }
    j.queue.push_back(key);
    if j.running {
        return;
    }
    j.running = true;
    let ctx = ctx.clone();
    std::thread::spawn(move || {
        let root = steam::detect_root();
        let dir = crate::config::config_dir();
        loop {
            let next = jobs().lock().ok().and_then(|mut j| {
                let next = j.queue.pop_front();
                if next.is_none() {
                    j.running = false;
                }
                next
            });
            let Some((app_id, kind)) = next else {
                return;
            };
            let art = resolve_art(app_id, kind, root.as_deref(), &dir, &mut fetch_art)
                .and_then(|(bytes, got)| decode(&bytes, got).map(|img| (img, got)));
            if let Ok(mut j) = jobs().lock() {
                j.ready.insert((app_id, kind), art);
            }
            ctx.request_repaint();
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
                Some(None) => Slot::Missing,
                Some(Some((img, got))) => {
                    let size = [img.width() as usize, img.height() as usize];
                    let tex = ctx.load_texture(
                        format!("game-art-{app_id}-{}", got.file()),
                        egui::ColorImage::from_rgba_unmultiplied(size, img.as_raw()),
                        egui::TextureOptions::LINEAR,
                    );
                    Slot::Ready(tex, Instant::now())
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
            Some(jpeg())
        };
        let got = resolve_art(570, CoverKind::Tall, Some(&root), &cfg, &mut fetch);
        assert_eq!(got.map(|(_, k)| k), Some(CoverKind::Tall));
        // A game Steam has only the header for: the local header beats the CDN.
        std::fs::write(root.join("appcache/librarycache/730_header.jpg"), jpeg()).unwrap();
        let got = resolve_art(730, CoverKind::Tall, Some(&root), &cfg, &mut fetch);
        assert_eq!(got.map(|(_, k)| k), Some(CoverKind::Wide));
        assert!(asked.borrow().is_empty(), "{asked:?}");
        // Nothing on this PC: the CDN's tall art, kept for next time.
        let got = resolve_art(1245620, CoverKind::Tall, None, &cfg, &mut fetch);
        assert_eq!(got.map(|(_, k)| k), Some(CoverKind::Tall));
        assert_eq!(
            *asked.borrow(),
            vec!["https://shared.akamai.steamstatic.com/store_item_assets/steam/apps/1245620/library_600x900.jpg"]
        );
        assert!(art_path(&cfg, 1245620, CoverKind::Tall).is_file());
        let got = resolve_art(1245620, CoverKind::Tall, None, &cfg, &mut |_| None);
        assert_eq!(got.map(|(_, k)| k), Some(CoverKind::Tall));
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&cfg);
    }

    #[test]
    fn offline_or_bad_art_falls_back_to_the_painted_cover() {
        let cfg = temp("offline");
        let mut tried = 0;
        let got = resolve_art(1001, CoverKind::Tall, None, &cfg, &mut |_| {
            tried += 1;
            None
        });
        assert!(got.is_none());
        assert_eq!(tried, 2, "tall art, then the header");
        let got = resolve_art(1001, CoverKind::Wide, None, &cfg, &mut |_| {
            Some(b"<html>not found</html>".to_vec())
        });
        assert!(got.is_none());
        assert!(!art_path(&cfg, 1001, CoverKind::Wide).exists());
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
