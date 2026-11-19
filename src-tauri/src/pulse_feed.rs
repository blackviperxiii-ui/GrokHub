//! The shell's Pulse Feed writer. On the system cadence, and once right after
//! the feed prompt is saved, it fetches the free sources, asks the model for
//! one post whose cites must be pages fetched this run, caches the post's
//! images locally, and adds the post to the same `updates.json` the page
//! reads. The taste summary is rebuilt before the post call when the person
//! loved, discussed, passed on a post or chatted since the last rebuild and
//! there is something new to write about. Every model call spends one of the
//! day's calls, counted on the person's own day.
//!
//! The same clock runs the idea writer (`grokhub_core::pulse_ideas`) after a
//! feed run, at most once every six hours: Muse-style ideas from the
//! person's open journal questions, interest topics, recent chats and feed
//! posts.

use std::borrow::Cow;
use std::collections::HashSet;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Condvar, Mutex, MutexGuard};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use grokhub_agent::harness as hx;
use grokhub_agent::{CancelToken, ContentPart, InputItem, ModelClient, ResponsesRequest};
use grokhub_core::http;
use grokhub_core::pulse::{self as pc};
use grokhub_core::pulse_ideas::{self as pi, IdeasState};
use grokhub_core::pulse_sources::{self as ps, CacheEntry, Fetched, SourceReq};
use grokhub_core::pulse_writer::{self as pw, TasteKind, WriterState};

/// Cached post images, named by `pc::image_cache_name`.
pub const IMAGES_DIR: &str = "pulse-images";
const TICK: Duration = Duration::from_secs(60);
const FETCH_TIMEOUT: Duration = Duration::from_secs(20);
const REDIRECTS_MAX: usize = 4;
const PAGE_CAP: u64 = 2 * 1024 * 1024;
const SOURCE_CAP: u64 = 4 * 1024 * 1024;
/// Longest side a cached image keeps; larger JPEG and PNG files are resized.
const THUMB_MAX_PX: u32 = 1280;
const THUMB_JPEG_QUALITY: u8 = 82;
/// Decode limits for a downloaded image: wider or taller files, or ones that
/// need more memory than this to decode, are refused.
const DECODE_MAX_PX: u32 = 8192;
const DECODE_MAX_ALLOC: u64 = 128 * 1024 * 1024;
/// The image cache is cleaned at most this often.
const PRUNE_EVERY_MS: u64 = 24 * 3_600_000;
/// Most images [`fill_images`] downloads in one tick.
const FILL_PER_TICK: usize = 6;
/// Only cards posted this recently get their images downloaded later.
const FILL_WINDOW_MS: u64 = 24 * 3_600_000;

/// Points the writer's network at a local mock (the shell smoke test). Only a
/// plain-http loopback URL is accepted, so it can never move real traffic.
pub const NET_ENV: &str = "GROKHUB_SHELL_PULSE_NET";

pub const NOTHING_TO_READ: &str =
    "Nothing to look into yet. Add an interest in Settings → Pulse → Interests.";
pub const BUDGET_USED: &str = "Today's post budget is used. The feed picks up tomorrow.";

/// One read-change-write of the writer state at a time.
static STATE: Mutex<()> = Mutex::new(());
/// One read-change-write of the idea state at a time.
static IDEAS: Mutex<()> = Mutex::new(());
/// One run at a time.
static RUNNING: AtomicBool = AtomicBool::new(false);
/// Wakes the clock early (a saved prompt owes a post now).
static WAKE: (Mutex<bool>, Condvar) = (Mutex::new(false), Condvar::new());
/// When this process last cleaned the image cache (0: not yet).
static LAST_PRUNE_MS: AtomicU64 = AtomicU64::new(0);
/// Makes each image temp file name unique within this process.
static TEMP_SEQ: AtomicU64 = AtomicU64::new(0);
/// Held while [`fit_image`] decodes, so at most one full-size image is live.
static DECODE: Mutex<()> = Mutex::new(());

fn state_lock() -> MutexGuard<'static, ()> {
    STATE.lock().unwrap_or_else(|e| e.into_inner())
}

fn state_path(config_dir: &Path) -> PathBuf {
    config_dir.join(pw::STATE_FILE)
}

pub fn images_dir(config_dir: &Path) -> PathBuf {
    config_dir.join(IMAGES_DIR)
}

/// Save a downloaded image into the cache under `pc::image_cache_name(url)`.
/// Only a whole JPEG, PNG or WebP (`pc::image_is_whole`) of at most
/// `pc::IMAGE_MAX_BYTES` is kept. A JPEG or PNG must decode within the
/// decode limits, and one longer than
/// [`THUMB_MAX_PX`] on a side is stored resized; WebP (which this build can't
/// decode) and smaller images keep their original bytes. The file appears
/// through a temp file and a rename, so a reader never sees half of it.
pub(crate) fn store_image(config_dir: &Path, url: &str, bytes: &[u8]) -> bool {
    let Some(mime) = pc::image_mime(bytes) else {
        return false;
    };
    if bytes.len() > pc::IMAGE_MAX_BYTES || !pc::image_is_whole(bytes) {
        return false;
    }
    let kept = match mime {
        "image/jpeg" => fit_image(bytes, image::ImageFormat::Jpeg),
        "image/png" => fit_image(bytes, image::ImageFormat::Png),
        _ => Some(Cow::Borrowed(bytes)),
    };
    let Some(kept) = kept else {
        return false;
    };
    let dir = images_dir(config_dir);
    if std::fs::create_dir_all(&dir).is_err() {
        return false;
    }
    let name = pc::image_cache_name(url);
    let tmp = dir.join(format!(
        "{name}.{}-{}.tmp",
        std::process::id(),
        TEMP_SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    if std::fs::write(&tmp, &kept).is_err() || std::fs::rename(&tmp, dir.join(&name)).is_err() {
        let _ = std::fs::remove_file(&tmp);
        return false;
    }
    true
}

/// Decode a JPEG or PNG within the limits. `None` when it doesn't decode or
/// its pixels would need more than [`DECODE_MAX_ALLOC`]; the original bytes
/// when it already fits; otherwise the upright image shrunk to fit
/// [`THUMB_MAX_PX`] and encoded in its own format with its colour profile.
/// One decode runs at a time, so the Pulse tick and the printer poll never
/// hold two full-size images at once.
fn fit_image(bytes: &[u8], format: image::ImageFormat) -> Option<Cow<'_, [u8]>> {
    use image::{ImageDecoder as _, ImageEncoder as _};
    let _one = DECODE.lock().unwrap_or_else(|e| e.into_inner());
    let mut reader = image::ImageReader::with_format(std::io::Cursor::new(bytes), format);
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(DECODE_MAX_PX);
    limits.max_image_height = Some(DECODE_MAX_PX);
    limits.max_alloc = Some(DECODE_MAX_ALLOC);
    reader.limits(limits);
    let mut decoder = reader.into_decoder().ok()?;
    // The JPEG decoder checks only the side limits, so the pixel budget is
    // checked here, before anything is allocated.
    if decoder.total_bytes() > DECODE_MAX_ALLOC {
        return None;
    }
    let orientation = decoder
        .orientation()
        .unwrap_or(image::metadata::Orientation::NoTransforms);
    let icc = decoder.icc_profile().ok().flatten();
    let img = image::DynamicImage::from_decoder(decoder).ok()?;
    if img.width().max(img.height()) <= THUMB_MAX_PX {
        return Some(Cow::Borrowed(bytes));
    }
    // Shrink first, then turn: the fit box is square, so the result is the
    // same, and only the small image is copied. `thumbnail` averages areas
    // without a full-size float buffer.
    let mut img = img.thumbnail(THUMB_MAX_PX, THUMB_MAX_PX);
    img.apply_orientation(orientation);
    let mut out = Vec::new();
    let written = if format == image::ImageFormat::Jpeg {
        let mut encoder =
            image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, THUMB_JPEG_QUALITY);
        if let Some(icc) = icc {
            encoder.set_icc_profile(icc).ok()?;
        }
        img.to_rgb8().write_with_encoder(encoder)
    } else {
        let mut encoder = image::codecs::png::PngEncoder::new(&mut out);
        if let Some(icc) = icc {
            encoder.set_icc_profile(icc).ok()?;
        }
        img.write_with_encoder(encoder)
    };
    written.ok().map(|()| Cow::Owned(out))
}

/// Delete cache files no card references any more (see `pc::prune_plan`).
/// A store that is missing, blank or can't be read deletes nothing. Returns
/// how many went.
pub(crate) fn prune_images(config_dir: &Path, now_ms: u64) -> usize {
    let Ok(cards) = crate::pulse::stored_cards(config_dir) else {
        return 0;
    };
    // A referenced URL keeps its file and its failure marker.
    let keep: HashSet<String> = cards
        .iter()
        .flat_map(|card| card.pulse.image_urls.iter())
        .flat_map(|url| [pc::image_cache_name(url), pc::image_miss_name(url)])
        .collect();
    let dir = images_dir(config_dir);
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return 0;
    };
    let files: Vec<pc::CachedFile> = entries
        .flatten()
        .filter_map(|entry| {
            let meta = entry.metadata().ok().filter(|m| m.is_file())?;
            let since_epoch = meta.modified().ok()?.duration_since(UNIX_EPOCH).ok()?;
            Some(pc::CachedFile {
                name: entry.file_name().into_string().ok()?,
                modified_ms: u64::try_from(since_epoch.as_millis()).ok()?,
            })
        })
        .collect();
    pc::prune_plan(&files, &keep, now_ms)
        .into_iter()
        // A file another program holds open stays until the next prune.
        .filter(|name| std::fs::remove_file(dir.join(name)).is_ok())
        .count()
}

/// Read, change and write the writer state under its lock.
pub fn with_state<T>(
    config_dir: &Path,
    f: impl FnOnce(&mut WriterState) -> T,
) -> Result<T, String> {
    let _lock = state_lock();
    let path = state_path(config_dir);
    let mut state = pw::load_state(&path);
    let out = f(&mut state);
    pw::save_state(&path, &state)?;
    Ok(out)
}

pub fn state(config_dir: &Path) -> WriterState {
    let _lock = state_lock();
    pw::load_state(&state_path(config_dir))
}

/// Read, change and write the idea writer's state under its lock.
pub fn with_ideas_state<T>(
    config_dir: &Path,
    f: impl FnOnce(&mut IdeasState) -> T,
) -> Result<T, String> {
    let _lock = IDEAS.lock().unwrap_or_else(|e| e.into_inner());
    let path = config_dir.join(pi::STATE_FILE);
    let mut state = pi::load_state(&path);
    let out = f(&mut state);
    pi::save_state(&path, &state)?;
    Ok(out)
}

/// The page's local clock, so quiet hours land on the person's own night.
pub fn note_clock(config_dir: &Path, hour: u32, minute: u32, now_ms: u64) -> Result<(), String> {
    let offset = pw::utc_offset_min(hour, minute, now_ms);
    // The learning engine counts habits in the same local hours.
    grokhub_core::learn::hub::set_utc_offset_min(offset);
    with_state(config_dir, |s| s.utc_offset_min = Some(offset))
}

/// Something the person said in the main chat: taste, never engagement.
pub fn note_chat(config_dir: &Path, text: &str, now_ms: u64) {
    let _ = with_state(config_dir, |s| s.note_taste(TasteKind::Chat, text, now_ms));
}

/// Ask the clock to run now instead of at its next tick.
pub fn wake() {
    let (flag, cv) = &WAKE;
    *flag.lock().unwrap_or_else(|e| e.into_inner()) = true;
    cv.notify_all();
}

/// Download the remote images of recent cards that don't have them cached
/// yet: release radar, wishlist and achievement capsules, daily drop
/// thumbnails, and any other card that names an `image_urls` entry.
///
/// - Nothing at all while Offline only is on.
/// - Live cards from the last [`FILL_WINDOW_MS`], newest first, and at most
///   [`FILL_PER_TICK`] downloads. Only public http(s) URLs; a printer
///   snapshot on the local network is never asked.
/// - Every download goes through `net` (the guarded [`LiveNet`]) and is kept
///   only through [`store_image`].
/// - A failure leaves a `.miss` marker (`pc::ImageMiss`), so a URL that
///   answered for good is never asked again and a passing failure is retried
///   at most twice, later. Cards are never edited.
///
/// The store lock is held only while the cards are read. Returns how many
/// downloads it asked for.
pub(crate) fn fill_images(config_dir: &Path, net: &dyn Net, now_ms: u64) -> usize {
    if grokhub_core::offline::enabled() {
        return 0;
    }
    let Ok(mut cards) = crate::pulse::cards(config_dir) else {
        return 0;
    };
    cards.retain(|c| {
        c.status != grokhub_core::UpdateStatus::Dismissed
            && now_ms.saturating_sub(c.created_at) <= FILL_WINDOW_MS
    });
    cards.sort_by_key(|c| std::cmp::Reverse(c.created_at));
    let dir = images_dir(config_dir);
    let mut seen = HashSet::new();
    let mut asked = 0;
    for url in cards.iter().flat_map(|c| c.pulse.image_urls.iter()) {
        if asked >= FILL_PER_TICK {
            break;
        }
        if !seen.insert(url.as_str())
            || !grokhub_core::public_http_url(url)
            || dir.join(pc::image_cache_name(url)).is_file()
        {
            continue;
        }
        let marker = dir.join(pc::image_miss_name(url));
        let prev = std::fs::read_to_string(&marker)
            .ok()
            .map(|text| pc::ImageMiss::parse(&text));
        if prev.is_some_and(|miss| !miss.due(now_ms)) {
            continue;
        }
        asked += 1;
        let gone = match net.image(url) {
            Ok(bytes) if pc::image_bytes_ok(&bytes) && store_image(config_dir, url, &bytes) => {
                let _ = std::fs::remove_file(&marker);
                continue;
            }
            // Not an image, a tracking pixel, too big, or it won't decode.
            Ok(_) | Err(FetchErr::Gone(_)) => true,
            Err(FetchErr::Later(_)) => false,
        };
        let miss = pc::ImageMiss::after_failure(prev, gone, now_ms);
        if std::fs::create_dir_all(&dir).is_ok() {
            let _ = std::fs::write(&marker, miss.render());
        }
    }
    asked
}

/// Why a fetch failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FetchErr {
    /// Asking again gets the same answer: an HTTP 4xx, an address that
    /// isn't public, a redirect with no target, or too many redirects.
    Gone(String),
    /// Worth asking again later: the network, a timeout, a server error, a
    /// rate limit, or the egress guard.
    Later(String),
}

impl From<FetchErr> for String {
    fn from(err: FetchErr) -> Self {
        match err {
            FetchErr::Gone(why) | FetchErr::Later(why) => why,
        }
    }
}

impl From<http::Error> for FetchErr {
    fn from(err: http::Error) -> Self {
        match err {
            http::Error::Status { code, .. }
                if (400..500).contains(&code) && code != 408 && code != 429 =>
            {
                Self::Gone(format!("HTTP {code}"))
            }
            http::Error::Status { code, .. } => Self::Later(format!("HTTP {code}")),
            http::Error::Transport { .. } => Self::Later("network error".into()),
        }
    }
}

/// The network a run uses. The live one fetches public pages only.
pub trait Net {
    fn source(&self, req: &SourceReq, have: Option<&CacheEntry>) -> Fetched;
    fn page(&self, url: &str) -> Result<String, String>;
    fn image(&self, url: &str) -> Result<Vec<u8>, FetchErr>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Posted(String),
    Nothing(String),
}

/// One writer run. The run counts even when it posts nothing, so a failing
/// source never retries on every tick.
pub fn run_once(
    config_dir: &Path,
    model: &str,
    client: &dyn ModelClient,
    net: &dyn Net,
    now_ms: u64,
) -> Result<Outcome, String> {
    let state = with_state(config_dir, |s| {
        s.take_queued();
        s.last_run_ms = now_ms;
        s.clone()
    })?;
    let outcome = write_post(config_dir, model, client, net, &state, now_ms);
    let note = match &outcome {
        Ok(Outcome::Posted(_)) => "Posted a new story.".to_string(),
        Ok(Outcome::Nothing(why)) => why.clone(),
        Err(err) => format!("The last post failed: {err}"),
    };
    with_state(config_dir, |s| s.note = note)?;
    outcome
}

/// The Steam games that feed Pulse and were played, most recent first.
fn played_games(config_dir: &Path, steam_root: Option<&Path>) -> Vec<String> {
    let Some(root) = steam_root else {
        return Vec::new();
    };
    let games = grokhub_core::steam::read_library(root);
    let prefs = grokhub_core::steam::load_prefs(config_dir);
    grokhub_core::steam::played_names(&games, &prefs, 8)
}

/// One idea run behind its six-hour gate. With nothing personal to go on
/// there is no call and the gate stays open; once asked, the run counts even
/// when it posts nothing. More like this is spent once a reply comes back.
/// Returns how many ideas posted.
pub fn run_ideas(
    config_dir: &Path,
    model: &str,
    client: &dyn ModelClient,
    now_ms: u64,
) -> Result<usize, String> {
    let ideas = with_ideas_state(config_dir, |s| s.clone())?;
    if !ideas.due(now_ms) {
        return Ok(0);
    }
    let cards = crate::pulse::cards(config_dir)?;
    let goals: Vec<String> = crate::journal::load(config_dir)
        .map(|page| page.active.into_iter().map(|q| q.question).collect())
        .unwrap_or_default();
    let profile =
        grokhub_core::interests::load(&config_dir.join(grokhub_core::interests::FILE_NAME));
    let chats: Vec<String> = state(config_dir)
        .taste_events
        .into_iter()
        .rev()
        .filter(|e| e.kind == TasteKind::Chat)
        .map(|e| e.text)
        .collect();
    let posts = pw::recent_titles(&cards);
    let existing: Vec<String> = cards
        .iter()
        .filter(|c| c.kind == grokhub_core::UpdateKind::Idea)
        .map(|c| c.title.clone())
        .collect();
    let games = played_games(config_dir, grokhub_core::steam::detect_root().as_deref());
    let pc = pi::pc_lines(&cards);
    let Some(messages) = pi::idea_messages(&pi::IdeaInput {
        games: &games,
        pc: &pc,
        goals: &goals,
        topics: &profile.prompt_section(),
        chats: &chats,
        posts: &posts,
        more_like: &ideas.more_like,
        turned_down: &ideas.turned_down,
        existing: &existing,
    }) else {
        return Ok(0);
    };
    with_ideas_state(config_dir, |s| s.last_run_ms = now_ms)?;
    let reply = ask(client, model, &messages)?;
    with_ideas_state(config_dir, |s| s.more_like.clear())?;
    let written = pi::parse_ideas(&reply, &existing, &ideas.turned_down);
    crate::pulse::add_ideas(config_dir, &written, now_ms)
}

/// The current taste note, rebuilt first when new taste events landed.
/// The taste note, rebuilt first when it is stale. The rebuild is a model
/// call, so it spends one of `day`'s calls; with none left the current note
/// stays.
fn taste_summary(
    config_dir: &Path,
    model: &str,
    client: &dyn ModelClient,
    state: &WriterState,
    cache: &mut ps::SourceCache,
    day: u64,
    now_ms: u64,
) -> String {
    let path = config_dir.join(pw::TASTE_FILE);
    let current = std::fs::read_to_string(&path).unwrap_or_default();
    if !state.taste_stale() {
        return current;
    }
    let Some(messages) = pw::taste_messages(&current, &state.taste_events) else {
        return current;
    };
    if !cache.take_call_on(day) {
        return current;
    }
    let Some(note) = ask(client, model, &messages)
        .ok()
        .and_then(|r| pw::parse_taste(&r))
    else {
        return current;
    };
    if pw::write_atomic(&path, &note).is_err() {
        return current;
    }
    let _ = with_state(config_dir, |s| s.taste_built_ms = now_ms);
    note
}

/// Settings → Pulse → Sources as the desktop app saved them in `app.json`
/// (`pulseSources`), or every source on when it has none.
fn source_settings(config_dir: &Path) -> ps::SourceSettings {
    std::fs::read_to_string(config_dir.join("app.json"))
        .ok()
        .and_then(|raw| serde_json::from_str::<serde_json::Value>(&raw).ok())
        .and_then(|v| v.get("pulseSources").cloned())
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default()
}

fn write_post(
    config_dir: &Path,
    model: &str,
    client: &dyn ModelClient,
    net: &dyn Net,
    state: &WriterState,
    now_ms: u64,
) -> Result<Outcome, String> {
    crate::interests::seed_once(config_dir, now_ms)?;
    let profile =
        grokhub_core::interests::load(&config_dir.join(grokhub_core::interests::FILE_NAME));
    let topics = profile.search_topics();
    let cache_path = config_dir.join(ps::CACHE_FILE);
    let mut cache = ps::load_cache(&cache_path);
    let settings = source_settings(config_dir);
    let mut reqs = ps::requests_for(&settings, &topics, cache.runs, now_ms);
    // Drain More like this / Like related searches when a search kind is on.
    if !cache.related.is_empty() {
        let related_reqs = ps::related_requests(&settings, &cache.related, now_ms);
        if !related_reqs.is_empty() {
            reqs.extend(related_reqs);
            cache.related.clear();
            let _ = ps::save_cache(&cache_path, &cache);
        }
    }
    if reqs.is_empty() {
        return Ok(Outcome::Nothing(NOTHING_TO_READ.into()));
    }
    let day = ps::local_day(now_ms, state.utc_offset_min);
    if cache.calls_on(day) >= ps::SUMMARY_CALLS_PER_DAY {
        return Ok(Outcome::Nothing(BUDGET_USED.into()));
    }
    let got = ps::gather(&reqs, &mut cache, now_ms, &mut |req, have| {
        net.source(req, have)
    });
    cache.last_run_ms = now_ms;
    // Every check moves the topic rotation on, posted or not, so the same
    // searches never repeat run after run.
    cache.runs = cache.runs.saturating_add(1);
    let ranked = ps::rank_with(
        &got.items,
        &profile,
        &cache.source_bias,
        &cache.source_posts,
        now_ms,
    );
    let cards = crate::pulse::cards(config_dir)?;
    let posted_urls: Vec<String> = cards.iter().flat_map(|c| c.citations.clone()).collect();
    let posted_titles: Vec<String> = cards.iter().map(|c| c.title.clone()).collect();
    let picks = ps::pick_new(&ranked, &cache, &posted_urls, &posted_titles);
    if picks.is_empty() {
        ps::save_cache(&cache_path, &cache)?;
        return Ok(Outcome::Nothing(
            grokhub_core::update_feed::HONEST_EMPTY.into(),
        ));
    }
    let mut taste = taste_summary(config_dir, model, client, state, &mut cache, day, now_ms);
    let liked = grokhub_core::card_taste::writer_line(
        &grokhub_core::card_taste::load(config_dir).preferences(),
    );
    if !liked.is_empty() {
        taste = format!("{}\n{liked}", taste.trim()).trim().to_string();
    }
    if !cache.take_call_on(day) {
        ps::save_cache(&cache_path, &cache)?;
        return Ok(Outcome::Nothing(BUDGET_USED.into()));
    }
    let research = pw::research_from(&picks);
    let recent = pw::recent_titles(&cards);
    let messages = pw::writer_messages(&pw::WriterInput {
        prompt: state.prompt_or_default(),
        taste: &taste,
        topics: &profile.prompt_section(),
        recent_titles: &recent,
        research: &research,
    });
    let reply = match ask(client, model, &messages) {
        Ok(reply) => reply,
        Err(err) => {
            ps::save_cache(&cache_path, &cache)?;
            return Err(err);
        }
    };
    let written = match pw::parse_written(&reply, &research, &recent) {
        Ok(written) => written,
        Err(why) => {
            // Offered and passed over: these items don't come back.
            ps::mark_seen(&mut cache, &picks);
            ps::save_cache(&cache_path, &cache)?;
            return Ok(Outcome::Nothing(why));
        }
    };
    let mut card = pw::post_card(&written, &research, now_ms);
    card.pulse.image_urls = cache_gallery(config_dir, net, &written.cites, &research);
    let id = card.id.clone();
    crate::pulse::add_post(config_dir, card)?;
    let used: Vec<ps::Ranked> = picks
        .into_iter()
        .filter(|p| {
            written
                .cites
                .iter()
                .any(|c| ps::canonical_url(c) == ps::canonical_url(&p.item.url))
        })
        .collect();
    ps::mark_seen(&mut cache, &used);
    cache.mark_source_posts(&used);
    ps::save_cache(&cache_path, &cache)?;
    Ok(Outcome::Posted(id))
}

/// A post's images, downloaded once into the local cache: first the cited
/// items' own feed images (a video thumbnail, a feed's media, a Reddit
/// preview), then the first cited page's gallery, without repeats and at
/// most `pc::GALLERY_MAX`. Returns the image URLs that are cached; the page
/// shows those files and never loads the image from its site.
fn cache_gallery(
    config_dir: &Path,
    net: &dyn Net,
    cites: &[String],
    research: &[pw::Research],
) -> Vec<String> {
    let Some(page_url) = cites.first() else {
        return Vec::new();
    };
    let mut wanted: Vec<String> = cites
        .iter()
        .flat_map(|cite| {
            research
                .iter()
                .filter(|r| ps::canonical_url(&r.url) == ps::canonical_url(cite))
                .flat_map(|r| r.images.iter().cloned())
        })
        .collect();
    if let Ok(html) = net.page(page_url) {
        wanted.extend(pc::gallery_images(&html, page_url));
    }
    let mut seen = HashSet::new();
    wanted.retain(|url| seen.insert(url.clone()));
    let dir = images_dir(config_dir);
    let mut kept = Vec::new();
    for url in wanted {
        if kept.len() >= pc::GALLERY_MAX {
            break;
        }
        let file = dir.join(pc::image_cache_name(&url));
        if file.is_file() {
            // Fresh again, so a prune before the post lands keeps it.
            let _ = std::fs::File::options()
                .write(true)
                .open(&file)
                .and_then(|f| f.set_modified(SystemTime::now()));
        } else {
            let Ok(bytes) = net.image(&url) else {
                continue;
            };
            // The minimum size skips tracking pixels and spacers.
            if !pc::image_bytes_ok(&bytes) || !store_image(config_dir, &url, &bytes) {
                continue;
            }
        }
        kept.push(url);
    }
    kept
}

/// One plain model call: no tools, no hosted search, background effort.
fn ask(
    client: &dyn ModelClient,
    model: &str,
    messages: &[(String, String)],
) -> Result<String, String> {
    ask_as(client, model, "pulse-writer", messages)
}

/// [`ask`] under another conversation id, for other background writers.
pub(crate) fn ask_as(
    client: &dyn ModelClient,
    model: &str,
    conversation_id: &str,
    messages: &[(String, String)],
) -> Result<String, String> {
    let req = ResponsesRequest {
        text_format: None,
        model: model.to_string(),
        effort: Some(grokhub_core::BACKGROUND_EFFORT.to_string()),
        input: messages
            .iter()
            .map(|(role, text)| InputItem::Message {
                role: role.clone(),
                content: vec![ContentPart::InputText(text.clone())],
            })
            .collect(),
        conversation_id: conversation_id.into(),
        tools: Vec::new(),
        hosted_search: false,
        call_timeout: Some(Duration::from_secs(120)),
    };
    let out = client
        .stream(&req, &CancelToken::new(), &mut |_| {})
        .map_err(|e| e.to_string())?;
    let text = out.text.trim().to_string();
    if text.is_empty() {
        return Err("the model sent an empty reply".into());
    }
    Ok(text)
}

/// The background clock: checks every minute (or when woken) and runs the
/// writer when it is due. Skips without a sign-in or with Pulse off.
pub fn start(
    config_dir: PathBuf,
    client: impl Fn() -> Result<(Box<dyn ModelClient>, String), String> + Send + 'static,
) {
    std::thread::spawn(move || {
        let mut just_opened = true;
        loop {
            tick(&config_dir, &client, just_opened);
            just_opened = false;
            let (flag, cv) = &WAKE;
            let woke = flag.lock().unwrap_or_else(|e| e.into_inner());
            let (mut woke, _) = cv
                .wait_timeout_while(woke, TICK, |w| !*w)
                .unwrap_or_else(|e| e.into_inner());
            *woke = false;
        }
    });
}

fn tick(
    config_dir: &Path,
    client: &impl Fn() -> Result<(Box<dyn ModelClient>, String), String>,
    just_opened: bool,
) {
    if !crate::capabilities::current().pulse {
        return;
    }
    let now = grokhub_core::now_ms();
    if now.saturating_sub(LAST_PRUNE_MS.load(Ordering::Relaxed)) >= PRUNE_EVERY_MS {
        LAST_PRUNE_MS.store(now, Ordering::Relaxed);
        prune_images(config_dir, now);
    }
    fill_images(config_dir, net().as_ref(), now);
    let state = state(config_dir);
    let quiet = quiet_now(config_dir, &state, now);
    if !pw::writer_due(&state, now, just_opened, quiet) {
        return;
    }
    let Ok((client, model)) = client() else {
        return;
    };
    if RUNNING.swap(true, Ordering::SeqCst) {
        return;
    }
    let _ = run_once(config_dir, &model, client.as_ref(), net().as_ref(), now);
    let _ = run_ideas(config_dir, &model, client.as_ref(), now);
    let _ = crate::pulse_digests::run_due(config_dir, now);
    RUNNING.store(false, Ordering::SeqCst);
}

/// Quiet hours as saved in Settings → Behavior, on the person's clock. No
/// quiet hours saved, or no clock reported yet, means not quiet.
fn quiet_now(config_dir: &Path, state: &WriterState, now_ms: u64) -> bool {
    let behavior = crate::settings::load(config_dir).behavior;
    if behavior.quiet_start.is_empty() || behavior.quiet_end.is_empty() {
        return false;
    }
    state.utc_offset_min.is_some_and(|offset| {
        grokhub_core::quiet_hours_active(
            &pw::local_hm(now_ms, offset),
            &behavior.quiet_start,
            &behavior.quiet_end,
        )
    })
}

/// The live network, or the loopback mock when [`NET_ENV`] names one.
pub(crate) fn net() -> Box<dyn Net> {
    match std::env::var(NET_ENV)
        .ok()
        .and_then(|raw| LoopbackNet::new(&raw))
    {
        Some(mock) => Box::new(mock),
        None => Box::new(LiveNet),
    }
}

/// Every fetch goes to `{base}/pulse/{source|page|image}?url=…` on a
/// loopback server instead of the internet.
pub struct LoopbackNet {
    base: String,
}

impl LoopbackNet {
    pub fn new(raw: &str) -> Option<Self> {
        let base = raw.trim().trim_end_matches('/');
        let host = base.strip_prefix("http://")?.split(['/', ':']).next()?;
        matches!(host, "127.0.0.1" | "localhost").then(|| Self {
            base: base.to_string(),
        })
    }

    fn get(&self, kind: &str, url: &str) -> Result<Vec<u8>, FetchErr> {
        let at = format!("{}/pulse/{kind}?url={}", self.base, ps::query_value(url));
        let resp = http::agent(http::Opts {
            overall: Some(FETCH_TIMEOUT),
            ..Default::default()
        })
        .get(&at)
        .call()?;
        let mut buf = Vec::new();
        resp.into_reader()
            .take(SOURCE_CAP)
            .read_to_end(&mut buf)
            .map_err(|e| FetchErr::Later(e.to_string()))?;
        Ok(buf)
    }
}

impl Net for LoopbackNet {
    fn source(&self, req: &SourceReq, _have: Option<&CacheEntry>) -> Fetched {
        match self.get("source", &req.url) {
            Ok(body) => Fetched::Body {
                body: String::from_utf8_lossy(&body).into_owned(),
                etag: None,
                last_modified: None,
            },
            Err(why) => Fetched::Failed(why.into()),
        }
    }

    fn page(&self, url: &str) -> Result<String, String> {
        self.get("page", url)
            .map(|body| String::from_utf8_lossy(&body).into_owned())
            .map_err(String::from)
    }

    fn image(&self, url: &str) -> Result<Vec<u8>, FetchErr> {
        self.get("image", url)
    }
}

/// One fetched body: status, bytes, ETag and Last-Modified.
struct Got {
    status: u16,
    body: Vec<u8>,
    etag: Option<String>,
    last_modified: Option<String>,
}

/// Public pages over plain HTTP(S): every hop is checked as a public URL and
/// dialed through the public-only resolver, and the egress policy sees it.
pub struct LiveNet;

impl LiveNet {
    fn agent() -> http::Agent {
        http::agent(http::Opts {
            redirects: 0,
            overall: Some(FETCH_TIMEOUT),
            public_only: true,
            ..Default::default()
        })
    }

    fn get(
        url: &str,
        accept: &str,
        data: &[hx::DataClass],
        have: Option<&CacheEntry>,
        cap: u64,
    ) -> Result<Got, FetchErr> {
        let mut url = url.to_string();
        for hop in 0..=REDIRECTS_MAX {
            if !grokhub_core::public_http_url(&url) {
                return Err(FetchErr::Gone("not a public address".into()));
            }
            hx::guard_quiet(
                &grokhub_agent::perm::config_dir(),
                &hx::EgressReq::new(&url, data),
            )
            .map_err(FetchErr::Later)?;
            let mut call = Self::agent()
                .get(&url)
                .header("user-agent", grokhub_agent::USER_AGENT)
                .header("accept", accept);
            if hop == 0 {
                if let Some(entry) = have {
                    if let Some(etag) = entry.etag.as_deref() {
                        call = call.header("if-none-match", etag);
                    }
                    if let Some(when) = entry.last_modified.as_deref() {
                        call = call.header("if-modified-since", when);
                    }
                }
            }
            let resp = call.call()?;
            let status = resp.status();
            if (300..400).contains(&status) && status != 304 {
                match resp.header("location") {
                    Some(loc) => {
                        url = redirect_target(&url, loc);
                        continue;
                    }
                    None => return Err(FetchErr::Gone(format!("HTTP {status}"))),
                }
            }
            let etag = resp.header("etag").map(str::to_string);
            let modified = resp.header("last-modified").map(str::to_string);
            let mut buf = Vec::new();
            resp.into_reader()
                .take(cap)
                .read_to_end(&mut buf)
                .map_err(|_| FetchErr::Later("timed out or dropped".into()))?;
            return Ok(Got {
                status,
                body: buf,
                etag,
                last_modified: modified,
            });
        }
        Err(FetchErr::Gone("too many redirects".into()))
    }
}

fn redirect_target(from: &str, location: &str) -> String {
    if location.starts_with("https://") || location.starts_with("http://") {
        return location.to_string();
    }
    let (scheme, rest) = from.split_once("://").unwrap_or(("https", from));
    let host = rest.split('/').next().unwrap_or("");
    if location.starts_with('/') {
        format!("{scheme}://{host}{location}")
    } else {
        format!("{scheme}://{host}/{location}")
    }
}

impl Net for LiveNet {
    fn source(&self, req: &SourceReq, have: Option<&CacheEntry>) -> Fetched {
        // A topic search carries the person's topic, so it goes as chat text.
        let data: &[hx::DataClass] = if req.topic.is_some() {
            &[hx::DataClass::Chat]
        } else {
            &[]
        };
        let accept = "application/json, application/atom+xml, application/rss+xml, application/xml;q=0.9, text/html;q=0.8, */*;q=0.5";
        match Self::get(&req.url, accept, data, have, SOURCE_CAP) {
            Ok(got) if got.status == 304 => Fetched::NotModified,
            Ok(got) => Fetched::Body {
                body: String::from_utf8_lossy(&got.body).into_owned(),
                etag: got.etag,
                last_modified: got.last_modified,
            },
            Err(why) => Fetched::Failed(why.into()),
        }
    }

    fn page(&self, url: &str) -> Result<String, String> {
        let got = Self::get(url, "text/html", &[], None, PAGE_CAP)?;
        Ok(String::from_utf8_lossy(&got.body).into_owned())
    }

    fn image(&self, url: &str) -> Result<Vec<u8>, FetchErr> {
        let cap = pc::IMAGE_MAX_BYTES as u64 + 1;
        Ok(Self::get(url, "image/webp,image/png,image/jpeg", &[], None, cap)?.body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use grokhub_agent::{ClientError, StreamEvent, TurnOutput};
    use std::sync::Mutex as StdMutex;

    const NOW: u64 = 1_800_000_000_000;

    /// Replies in order; records each request's text.
    struct Script {
        replies: StdMutex<Vec<String>>,
        seen: StdMutex<Vec<String>>,
    }

    impl Script {
        fn new(replies: &[&str]) -> Self {
            Self {
                replies: StdMutex::new(replies.iter().rev().map(|r| r.to_string()).collect()),
                seen: StdMutex::new(Vec::new()),
            }
        }
    }

    impl ModelClient for Script {
        fn stream(
            &self,
            req: &ResponsesRequest,
            _cancel: &CancelToken,
            _sink: &mut dyn FnMut(StreamEvent),
        ) -> Result<TurnOutput, ClientError> {
            assert!(req.tools.is_empty() && !req.hosted_search);
            let text: String = req.input.iter().map(grokhub_agent::message_text).collect();
            self.seen.lock().unwrap().push(text);
            let reply = self
                .replies
                .lock()
                .unwrap()
                .pop()
                .expect("a scripted reply");
            Ok(TurnOutput {
                text: reply,
                ..Default::default()
            })
        }
    }

    const HN: &str = r#"{"hits":[{"title":"Rust 1.99 ships async closures","url":"https://blog.rust-lang.org/2026/10/20/Rust-1.99.html","points":420,"created_at_i":1799990000,"objectID":"1"}]}"#;

    /// An RGB JPEG drawn by `px`.
    fn jpeg(w: u32, h: u32, quality: u8, px: impl Fn(u32, u32) -> [u8; 3]) -> Vec<u8> {
        let img = image::RgbImage::from_fn(w, h, |x, y| image::Rgb(px(x, y)));
        let mut out = Vec::new();
        img.write_with_encoder(image::codecs::jpeg::JpegEncoder::new_with_quality(
            &mut out, quality,
        ))
        .expect("jpeg");
        out
    }

    /// The page's hero image: a real 256×256 JPEG of noise, well over
    /// `pc::IMAGE_MIN_BYTES` and small enough to be cached as is.
    static FIXTURE: std::sync::LazyLock<Vec<u8>> = std::sync::LazyLock::new(|| {
        let noise = |x: u32, y: u32, c: u32| {
            let mut v = (x * 7919 + y * 104_729 + c * 1_299_709) ^ 0x9E37_79B9;
            v ^= v << 13;
            v ^= v >> 17;
            v ^= v << 5;
            v.to_le_bytes()[0]
        };
        jpeg(256, 256, 90, |x, y| {
            [noise(x, y, 0), noise(x, y, 1), noise(x, y, 2)]
        })
    });

    struct FakeNet;

    impl Net for FakeNet {
        fn source(&self, req: &SourceReq, _have: Option<&CacheEntry>) -> Fetched {
            if req.url.contains("hn.algolia.com") {
                Fetched::Body {
                    body: HN.into(),
                    etag: None,
                    last_modified: None,
                }
            } else {
                Fetched::Failed("offline".into())
            }
        }

        fn page(&self, _url: &str) -> Result<String, String> {
            Ok(r#"<meta property="og:image" content="https://blog.rust-lang.org/hero.jpg"><img src="/logo.png" width="900">"#.into())
        }

        fn image(&self, url: &str) -> Result<Vec<u8>, FetchErr> {
            assert_eq!(url, "https://blog.rust-lang.org/hero.jpg");
            Ok(FIXTURE.clone())
        }
    }

    struct Dirs {
        config: PathBuf,
        _guard: grokhub_agent::perm::ConfigGuard,
    }

    fn dirs(name: &str) -> Dirs {
        let config = std::env::temp_dir().join(format!(
            "grokhub-shell-feed-{name}-{}-{}",
            std::process::id(),
            grokhub_core::now_ms()
        ));
        let _ = std::fs::remove_dir_all(&config);
        std::fs::create_dir_all(&config).expect("cfg");
        let guard = grokhub_agent::perm::ConfigGuard::set(&config);
        let mut profile = grokhub_core::interests::InterestProfile::default();
        profile.add(
            "rust",
            grokhub_core::interests::InterestSource::Explicit,
            NOW,
        );
        std::fs::write(
            config.join(grokhub_core::interests::FILE_NAME),
            serde_json::to_string(&profile).expect("json"),
        )
        .expect("interests");
        Dirs {
            config,
            _guard: guard,
        }
    }

    const POST: &str = r#"{"title":"Async closures are stable in Rust 1.99","snippet":"Rust 1.99 makes [async closures stable](https://blog.rust-lang.org/2026/10/20/Rust-1.99.html), per [a rumor](https://made-up.example/x).","icon":"🦀","why":"You follow Rust."}"#;

    #[test]
    fn a_run_writes_one_post_with_checked_cites_and_a_cached_image() {
        let d = dirs("post");
        with_state(&d.config, |s| {
            s.save_prompt("Only Rust news, please.");
            s.note_taste(TasteKind::Love, "Rust 1.98 notes", NOW - 1);
        })
        .expect("state");
        let client = Script::new(&["They love Rust release notes.", POST]);
        let out = run_once(&d.config, "grok-test", &client, &FakeNet, NOW).expect("run");
        let Outcome::Posted(id) = out else {
            panic!("{out:?}");
        };

        let seen = client.seen.lock().unwrap();
        assert_eq!(seen.len(), 2, "taste rebuild, then the post");
        assert!(
            seen[0].contains("- Loved a post: Rust 1.98 notes"),
            "{}",
            seen[0]
        );
        assert!(seen[1].contains("Only Rust news, please."), "{}", seen[1]);
        assert!(
            seen[1].contains("They love Rust release notes."),
            "{}",
            seen[1]
        );
        assert!(
            seen[1].contains("https://blog.rust-lang.org/2026/10/20/Rust-1.99.html"),
            "{}",
            seen[1]
        );

        let cards = crate::pulse::cards(&d.config).expect("cards");
        let card = cards.iter().find(|c| c.id == id).expect("posted");
        assert_eq!(card.title, "Async closures are stable in Rust 1.99");
        assert_eq!(
            card.body.as_deref(),
            Some("Rust 1.99 makes [async closures stable](https://blog.rust-lang.org/2026/10/20/Rust-1.99.html), per a rumor.")
        );
        assert_eq!(
            card.citations,
            vec!["https://blog.rust-lang.org/2026/10/20/Rust-1.99.html".to_string()]
        );
        assert_eq!(card.pulse.icon.as_deref(), Some("🦀"));
        assert_eq!(card.why.as_deref(), Some("You follow Rust."));
        assert_eq!(
            card.pulse.image_urls,
            vec!["https://blog.rust-lang.org/hero.jpg".to_string()]
        );
        let cached =
            images_dir(&d.config).join(pc::image_cache_name("https://blog.rust-lang.org/hero.jpg"));
        assert_eq!(
            std::fs::metadata(&cached).expect("cached").len(),
            FIXTURE.len() as u64
        );

        let s = state(&d.config);
        assert!(!s.queued, "the saved prompt's post is paid");
        assert_eq!(s.last_run_ms, NOW);
        assert_eq!(s.taste_built_ms, NOW);
        assert!(!s.taste_stale());
        assert_eq!(s.note, "Posted a new story.");
        assert_eq!(
            std::fs::read_to_string(d.config.join(pw::TASTE_FILE)).unwrap(),
            "They love Rust release notes."
        );

        // The same story is never posted twice: next run has nothing new.
        let again = Script::new(&[]);
        assert_eq!(
            run_once(&d.config, "grok-test", &again, &FakeNet, NOW + 3_600_000),
            Ok(Outcome::Nothing(
                grokhub_core::update_feed::HONEST_EMPTY.into()
            ))
        );
        assert!(again.seen.lock().unwrap().is_empty(), "no model call");
        let cache = ps::load_cache(&d.config.join(ps::CACHE_FILE));
        // Both checks move the topic rotation on; only the two model calls
        // of the first run spent today's budget.
        assert_eq!(cache.runs, 2);
        assert_eq!(cache.calls_on(ps::local_day(NOW, None)), 2);
    }

    #[test]
    fn a_new_love_with_nothing_new_to_read_makes_no_taste_call() {
        let d = dirs("taste-gate");
        let client = Script::new(&[POST]);
        run_once(&d.config, "grok-test", &client, &FakeNet, NOW).expect("post");
        assert_eq!(
            client.seen.lock().unwrap().len(),
            1,
            "no taste yet: post only"
        );
        with_state(&d.config, |s| {
            s.note_taste(TasteKind::Love, "Async closures", NOW + 1);
        })
        .expect("love");
        let again = Script::new(&[]);
        assert_eq!(
            run_once(&d.config, "grok-test", &again, &FakeNet, NOW + 3_600_000),
            Ok(Outcome::Nothing(
                grokhub_core::update_feed::HONEST_EMPTY.into()
            ))
        );
        assert!(again.seen.lock().unwrap().is_empty(), "no model call");
        assert!(
            state(&d.config).taste_stale(),
            "the love waits for the next post"
        );
    }

    #[test]
    fn a_used_day_budget_spends_no_fetch_and_the_next_local_day_runs() {
        let d = dirs("budget");
        with_state(&d.config, |s| s.utc_offset_min = Some(-600)).expect("clock");
        let day = ps::local_day(NOW, Some(-600));
        let mut cache = ps::SourceCache::default();
        for _ in 0..ps::SUMMARY_CALLS_PER_DAY {
            assert!(cache.take_call_on(day));
        }
        ps::save_cache(&d.config.join(ps::CACHE_FILE), &cache).expect("cache");
        let net = Recording::default();
        assert_eq!(
            run_once(&d.config, "grok-test", &Script::new(&[]), &net, NOW),
            Ok(Outcome::Nothing(BUDGET_USED.into()))
        );
        assert!(net.0.lock().unwrap().is_empty(), "no fetch on a spent day");
        // NOW is 22:00 the day before at UTC-10; two hours later is a new day there.
        let client = Script::new(&[POST]);
        let next = NOW + 2 * 3_600_000;
        assert_ne!(ps::local_day(next, Some(-600)), day);
        assert!(matches!(
            run_once(&d.config, "grok-test", &client, &FakeNet, next),
            Ok(Outcome::Posted(_))
        ));
    }

    #[test]
    fn quiet_hours_come_from_behavior_settings() {
        let d = dirs("quiet");
        let mut st = WriterState {
            utc_offset_min: Some(0),
            ..WriterState::default()
        };
        let at = |h: u64| NOW - NOW % 86_400_000 + h * 3_600_000;
        // Nothing saved: never quiet, even at 23:00.
        assert!(!quiet_now(&d.config, &st, at(23)));
        std::fs::write(
            d.config.join(crate::settings::FILE),
            r#"{"behavior":{"quietStart":"01:00","quietEnd":"06:00"}}"#,
        )
        .expect("shell.json");
        assert!(quiet_now(&d.config, &st, at(3)));
        assert!(!quiet_now(&d.config, &st, at(23)));
        st.utc_offset_min = None;
        assert!(!quiet_now(&d.config, &st, at(3)), "no clock yet: not quiet");
    }

    #[test]
    fn the_first_run_seeds_interests_from_memory() {
        let d = dirs("seed");
        std::fs::remove_file(d.config.join(grokhub_core::interests::FILE_NAME)).expect("rm");
        std::fs::create_dir_all(d.config.join("memory")).expect("memory");
        std::fs::write(d.config.join("memory").join("USER.md"), "Interests: Rust\n").expect("user");
        let client = Script::new(&[POST]);
        assert!(matches!(
            run_once(&d.config, "grok-test", &client, &FakeNet, NOW),
            Ok(Outcome::Posted(_))
        ));
        let profile =
            grokhub_core::interests::load(&d.config.join(grokhub_core::interests::FILE_NAME));
        assert!(profile.seeded);
        assert_eq!(profile.search_topics(), vec!["Rust".to_string()]);
        // Removed topics never come back: the seed runs once.
        assert_eq!(crate::interests::seed_once(&d.config, NOW + 1), Ok(0));
    }

    /// FakeNet that writes down every source URL it was asked for.
    #[derive(Default)]
    struct Recording(Mutex<Vec<String>>);

    impl Net for Recording {
        fn source(&self, req: &SourceReq, have: Option<&CacheEntry>) -> Fetched {
            self.0.lock().unwrap().push(req.url.clone());
            FakeNet.source(req, have)
        }

        fn page(&self, url: &str) -> Result<String, String> {
            FakeNet.page(url)
        }

        fn image(&self, url: &str) -> Result<Vec<u8>, FetchErr> {
            FakeNet.image(url)
        }
    }

    #[test]
    fn a_run_asks_only_the_sources_left_on_in_settings() {
        let d = dirs("sources");
        std::fs::write(
            d.config.join("app.json"),
            r#"{"pulseSources":{"webSearch":false,"reddit":false,"youtube":false}}"#,
        )
        .expect("app.json");
        with_state(&d.config, |s| {
            s.note_taste(TasteKind::Love, "Rust 1.98 notes", NOW - 1);
        })
        .expect("state");
        let client = Script::new(&["They love Rust release notes.", POST]);
        let net = Recording::default();
        run_once(&d.config, "grok-test", &client, &net, NOW).expect("run");
        let urls = net.0.lock().unwrap().clone();
        assert!(
            urls.iter().any(|u| u.contains("hn.algolia.com")),
            "{urls:?}"
        );
        assert!(
            !urls.iter().any(|u| u.contains("duckduckgo")
                || u.contains("reddit.com")
                || u.contains("youtube")),
            "a source turned off was asked: {urls:?}"
        );
    }

    #[test]
    fn a_post_citing_only_invented_pages_is_dropped() {
        let d = dirs("invented");
        let client = Script::new(&[
            r#"{"title":"Rust news","snippet":"See [here](https://made-up.example/x).","icon":"🦀","why":"You follow Rust."}"#,
        ]);
        assert_eq!(
            run_once(&d.config, "grok-test", &client, &FakeNet, NOW),
            Ok(Outcome::Nothing(pw::NO_SOURCE.into()))
        );
        assert!(crate::pulse::cards(&d.config).expect("cards").is_empty());
        assert_eq!(state(&d.config).note, pw::NO_SOURCE);
    }

    #[test]
    fn no_interests_means_no_fetch_and_no_model_call() {
        let d = dirs("empty");
        std::fs::remove_file(d.config.join(grokhub_core::interests::FILE_NAME)).expect("rm");
        let client = Script::new(&[]);
        assert_eq!(
            run_once(&d.config, "grok-test", &client, &FakeNet, NOW),
            Ok(Outcome::Nothing(NOTHING_TO_READ.into()))
        );
    }

    const IDEAS: &str = r#"[{"title":"Rust 1.99 upgrade plan","description":"I can check which of your crates need changes for Rust 1.99.","emoji":"🦀","category":"productivity","action":"check my crates for Rust 1.99 upgrade work"},{"title":"Cheap flights","description":"Here are flights.","emoji":"✈️","category":"shopping","action":"find cheap flights"}]"#;

    #[test]
    fn the_idea_run_posts_written_ideas_once_per_six_hours() {
        let d = dirs("ideas");
        with_state(&d.config, |s| {
            s.note_taste(
                TasteKind::Chat,
                "help me move my crates to Rust 1.99",
                NOW - 5,
            )
        })
        .expect("state");
        with_ideas_state(&d.config, |s| s.note_more_like("Rust release notes"))
            .expect("ideas state");
        let client = Script::new(&[IDEAS]);
        assert_eq!(run_ideas(&d.config, "grok-test", &client, NOW), Ok(1));
        let seen = client.seen.lock().unwrap().join("\n");
        assert!(
            seen.contains("You suggest ideas for one person's Pulse"),
            "{seen}"
        );
        assert!(
            seen.contains("- help me move my crates to Rust 1.99"),
            "{seen}"
        );
        assert!(
            seen.contains("More like this:\n- Rust release notes"),
            "{seen}"
        );
        assert!(
            with_ideas_state(&d.config, |s| s.more_like.clone())
                .expect("state")
                .is_empty(),
            "More like this is spent"
        );

        let page = crate::pulse::load(&d.config, None, NOW).expect("page");
        let ideas: Vec<(&str, &str, &str)> = page
            .ideas
            .iter()
            .flat_map(|g| {
                g.ideas
                    .iter()
                    .map(move |i| (g.label.as_str(), i.title.as_str(), i.icon.as_str()))
            })
            .collect();
        assert_eq!(
            ideas,
            vec![("", "Rust 1.99 upgrade plan", "🦀")],
            "the one with no \"I can\" line is dropped"
        );

        let client = Script::new(&[]);
        assert_eq!(
            run_ideas(&d.config, "grok-test", &client, NOW + pi::EVERY_MS - 1),
            Ok(0),
            "no call inside six hours"
        );
        let page = crate::pulse::load(&d.config, None, NOW + pc::IDEA_TTL_MS).expect("page");
        assert!(page.ideas.is_empty(), "retired after two weeks: {page:?}");
        assert!(
            crate::pulse::cards(&d.config).expect("cards").is_empty(),
            "and gone from the store"
        );
    }

    #[test]
    fn an_idea_run_with_nothing_personal_makes_no_call_and_keeps_the_gate_open() {
        let d = dirs("ideas-empty");
        std::fs::remove_file(d.config.join(grokhub_core::interests::FILE_NAME)).expect("rm");
        let client = Script::new(&[]);
        assert_eq!(run_ideas(&d.config, "grok-test", &client, NOW), Ok(0));
        assert_eq!(
            with_ideas_state(&d.config, |s| s.last_run_ms).expect("state"),
            0,
            "the first real run is not held back six hours"
        );
    }

    #[test]
    fn the_mock_network_is_loopback_only() {
        assert!(LoopbackNet::new("http://127.0.0.1:4711").is_some());
        assert_eq!(
            LoopbackNet::new("http://localhost:9/").map(|n| n.base),
            Some("http://localhost:9".to_string())
        );
        assert!(LoopbackNet::new("https://127.0.0.1:4711").is_none());
        assert!(LoopbackNet::new("http://example.com").is_none());
        assert!(LoopbackNet::new("http://127.0.0.1.example.com").is_none());
    }

    #[test]
    fn redirects_stay_on_the_same_host() {
        assert_eq!(
            redirect_target("https://a.example.com/x/y", "/z"),
            "https://a.example.com/z"
        );
        assert_eq!(
            redirect_target("https://a.example.com/x", "z"),
            "https://a.example.com/z"
        );
        assert_eq!(
            redirect_target("https://a.example.com/x", "https://b.example.com/"),
            "https://b.example.com/"
        );
    }

    const HERO: &str = "https://example.com/hero.jpg";

    fn decoded_size(bytes: &[u8]) -> (u32, u32) {
        let img = image::load_from_memory(bytes).expect("decodes");
        (img.width(), img.height())
    }

    #[test]
    fn store_image_shrinks_a_large_jpeg() {
        let d = dirs("store-large");
        let big = jpeg(2560, 1440, 90, |x, y| [(x / 10) as u8, (y / 6) as u8, 128]);
        assert!(store_image(&d.config, HERO, &big));
        let stored =
            std::fs::read(images_dir(&d.config).join(pc::image_cache_name(HERO))).expect("stored");
        assert_eq!(decoded_size(&stored), (1280, 720));
        assert_eq!(pc::image_mime(&stored), Some("image/jpeg"));
    }

    #[test]
    fn store_image_keeps_a_small_jpeg_byte_for_byte() {
        let d = dirs("store-small");
        let small = jpeg(480, 320, 90, |x, y| [(x / 2) as u8, (y / 2) as u8, 64]);
        assert!(store_image(&d.config, HERO, &small));
        let stored =
            std::fs::read(images_dir(&d.config).join(pc::image_cache_name(HERO))).expect("stored");
        assert_eq!(stored, small);
    }

    #[test]
    fn store_image_refuses_a_jpeg_with_a_broken_header() {
        let d = dirs("store-broken");
        let dir = images_dir(&d.config);
        std::fs::create_dir_all(&dir).expect("dir");
        let mut broken = vec![0xFF, 0xD8, 0xFF, 0xE0];
        broken.resize(4 + 16 * 1024, 7);
        broken.extend_from_slice(&[0xFF, 0xD9]);
        assert!(
            pc::image_is_whole(&broken),
            "only the decoder can refuse it"
        );
        assert!(!store_image(&d.config, HERO, &broken));
        assert_eq!(std::fs::read_dir(&dir).expect("dir").count(), 0);
    }

    #[test]
    fn store_image_refuses_a_jpeg_cut_off_mid_download() {
        let d = dirs("store-cut");
        let dir = images_dir(&d.config);
        std::fs::create_dir_all(&dir).expect("dir");
        let big = jpeg(2560, 1440, 90, |x, y| [(x / 10) as u8, (y / 6) as u8, 128]);
        let small = jpeg(480, 320, 90, |x, y| [(x / 2) as u8, (y / 2) as u8, 64]);
        assert!(!store_image(&d.config, HERO, &big[..big.len() / 2]));
        assert!(!store_image(&d.config, HERO, &small[..small.len() / 2]));
        assert_eq!(std::fs::read_dir(&dir).expect("dir").count(), 0);
    }

    /// A small JPEG whose header claims `w`×`h`: a decompression bomb.
    fn jpeg_claiming(w: u16, h: u16) -> Vec<u8> {
        let mut bytes = jpeg(64, 64, 90, |_, _| [90, 90, 90]);
        let sof = bytes
            .windows(2)
            .position(|m| m == [0xFF, 0xC0])
            .expect("baseline frame header");
        bytes[sof + 5..sof + 7].copy_from_slice(&h.to_be_bytes());
        bytes[sof + 7..sof + 9].copy_from_slice(&w.to_be_bytes());
        bytes
    }

    #[test]
    fn store_image_refuses_a_jpeg_too_big_to_decode() {
        let d = dirs("store-bomb");
        let dir = images_dir(&d.config);
        std::fs::create_dir_all(&dir).expect("dir");
        // 8192 x 6000 x 3 bytes is 140.6 MiB, over the 128 MiB budget, though
        // each side is within DECODE_MAX_PX.
        let bomb = jpeg_claiming(8192, 6000);
        assert_eq!(decoded_header(&bomb), (8192, 6000));
        assert!(!store_image(&d.config, HERO, &bomb));
        assert_eq!(std::fs::read_dir(&dir).expect("dir").count(), 0);
    }

    fn decoded_header(bytes: &[u8]) -> (u32, u32) {
        image::ImageReader::with_format(std::io::Cursor::new(bytes), image::ImageFormat::Jpeg)
            .into_dimensions()
            .expect("header")
    }

    #[test]
    fn store_image_keeps_the_colour_profile_of_a_resized_jpeg() {
        use image::{ImageDecoder as _, ImageEncoder as _};
        let d = dirs("store-icc");
        let profile: Vec<u8> = (0..=255u8).cycle().take(600).collect();
        let img = image::RgbImage::from_fn(2560, 1440, |x, y| {
            image::Rgb([(x / 10) as u8, (y / 6) as u8, 128])
        });
        let mut src = Vec::new();
        let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut src, 90);
        encoder.set_icc_profile(profile.clone()).expect("icc");
        img.write_with_encoder(encoder).expect("jpeg");

        assert!(store_image(&d.config, HERO, &src));
        let stored =
            std::fs::read(images_dir(&d.config).join(pc::image_cache_name(HERO))).expect("stored");
        assert_eq!(decoded_size(&stored), (1280, 720));
        let mut decoder =
            image::codecs::jpeg::JpegDecoder::new(std::io::Cursor::new(&stored)).expect("jpeg");
        assert_eq!(decoder.icc_profile().expect("read"), Some(profile));
    }

    #[test]
    fn store_image_refuses_a_too_wide_png() {
        let d = dirs("store-wide");
        let img = image::RgbImage::from_pixel(9000, 10, image::Rgb([200, 30, 30]));
        let mut png = Vec::new();
        img.write_with_encoder(image::codecs::png::PngEncoder::new(&mut png))
            .expect("png");
        assert_eq!(pc::image_mime(&png), Some("image/png"));
        assert!(!store_image(
            &d.config,
            "https://example.com/wide.png",
            &png
        ));
        assert!(!images_dir(&d.config)
            .join(pc::image_cache_name("https://example.com/wide.png"))
            .exists());
    }

    #[test]
    fn prune_images_keeps_everything_when_the_store_is_damaged() {
        let d = dirs("prune-damaged");
        let dir = images_dir(&d.config);
        std::fs::create_dir_all(&dir).expect("dir");
        let stray = dir.join("0123456789abcdef.jpg");
        std::fs::write(&stray, &*FIXTURE).expect("img");
        let two_hours_ago = SystemTime::now() - Duration::from_secs(2 * 3600);
        std::fs::File::options()
            .write(true)
            .open(&stray)
            .and_then(|f| f.set_modified(two_hours_ago))
            .expect("mtime");
        let now = grokhub_core::now_ms();

        assert!(!d.config.join("updates.json").exists());
        assert_eq!(prune_images(&d.config, now), 0);
        assert!(stray.is_file(), "a missing store deletes nothing");

        std::fs::write(d.config.join("updates.json"), " \n").expect("store");
        assert_eq!(prune_images(&d.config, now), 0);
        assert!(stray.is_file(), "a blank store deletes nothing");

        std::fs::write(d.config.join("updates.json"), "{").expect("store");
        assert_eq!(prune_images(&d.config, now), 0);
        assert!(stray.is_file(), "a damaged store deletes nothing");

        // The same file goes once the store reads and no card names it.
        std::fs::write(d.config.join("updates.json"), "[]").expect("store");
        assert_eq!(prune_images(&d.config, now), 1);
        assert!(!stray.exists());
    }

    const YT: &str = r#"<feed xmlns:yt="http://www.youtube.com/xml/schemas/2015" xmlns:media="http://search.yahoo.com/mrss/" xmlns="http://www.w3.org/2005/Atom">
 <title>Rust Tours</title>
 <entry><title>Rust 1.99 in ten minutes</title>
  <link rel="alternate" href="https://www.youtube.com/watch?v=abc"/>
  <published>2027-01-15T06:00:00+00:00</published>
  <media:group><media:title>Rust 1.99</media:title>
   <media:thumbnail url="https://i.ytimg.com/vi/abc/hqdefault.jpg" width="480" height="360"/>
   <media:description>A quick tour of Rust 1.99.</media:description></media:group>
 </entry></feed>"#;

    /// One YouTube channel and the page FakeNet serves; records image asks.
    #[derive(Default)]
    struct VideoNet(Mutex<Vec<String>>);

    impl Net for VideoNet {
        fn source(&self, req: &SourceReq, _have: Option<&CacheEntry>) -> Fetched {
            if req.url.contains("youtube.com/feeds") {
                Fetched::Body {
                    body: YT.into(),
                    etag: None,
                    last_modified: None,
                }
            } else {
                Fetched::Failed("offline".into())
            }
        }

        fn page(&self, url: &str) -> Result<String, String> {
            FakeNet.page(url)
        }

        fn image(&self, url: &str) -> Result<Vec<u8>, FetchErr> {
            self.0.lock().unwrap().push(url.to_string());
            Ok(FIXTURE.clone())
        }
    }

    #[test]
    fn a_written_video_post_caches_its_thumbnail_then_the_page_image() {
        let d = dirs("video");
        std::fs::write(
            d.config.join("app.json"),
            r#"{"pulseSources":{"hackerNews":false,"webSearch":false,"reddit":false,"github":false,"rss":false,"channels":["UCabc"]}}"#,
        )
        .expect("app.json");
        let client = Script::new(&[
            r#"{"title":"Rust 1.99 in ten minutes","snippet":"A [quick tour](https://www.youtube.com/watch?v=abc) of Rust 1.99.","icon":"🦀","why":"You follow Rust."}"#,
        ]);
        let net = VideoNet::default();
        let out = run_once(&d.config, "grok-test", &client, &net, NOW).expect("run");
        let Outcome::Posted(id) = out else {
            panic!("{out:?}");
        };
        let cards = crate::pulse::cards(&d.config).expect("cards");
        let card = cards.iter().find(|c| c.id == id).expect("posted");
        assert_eq!(
            card.pulse.image_urls,
            vec![
                "https://i.ytimg.com/vi/abc/hqdefault.jpg".to_string(),
                "https://blog.rust-lang.org/hero.jpg".to_string()
            ]
        );
        assert_eq!(
            *net.0.lock().unwrap(),
            vec![
                "https://i.ytimg.com/vi/abc/hqdefault.jpg".to_string(),
                "https://blog.rust-lang.org/hero.jpg".to_string()
            ]
        );
        let page = crate::pulse::load(&d.config, None, NOW).expect("page");
        assert_eq!(
            page.feed[0].images,
            vec![
                pc::image_cache_name("https://i.ytimg.com/vi/abc/hqdefault.jpg"),
                pc::image_cache_name("https://blog.rust-lang.org/hero.jpg")
            ]
        );
    }

    const CAPSULE_620: &str = "https://cdn.akamai.steamstatic.com/steam/apps/620/header.jpg";

    /// A release radar post for `app_id`, posted at `at`.
    fn capsule_card(app_id: u32, at: u64) -> grokhub_core::UpdateCard {
        let reminder = grokhub_core::release_radar::Reminder {
            name: format!("Game {app_id}"),
            date_ms: at,
        };
        grokhub_core::release_radar::out_today_card(app_id, &reminder, at)
    }

    fn store(config: &Path, cards: &[grokhub_core::UpdateCard]) {
        std::fs::write(
            config.join("updates.json"),
            serde_json::to_string(cards).expect("json"),
        )
        .expect("updates.json");
    }

    /// Answers every image ask with `answer` (the fixture JPEG by default)
    /// and writes the ask down.
    #[derive(Default)]
    struct Images {
        asked: Mutex<Vec<String>>,
        answer: Mutex<Option<Result<Vec<u8>, FetchErr>>>,
    }

    impl Images {
        fn answering(answer: Result<Vec<u8>, FetchErr>) -> Self {
            Self {
                asked: Mutex::new(Vec::new()),
                answer: Mutex::new(Some(answer)),
            }
        }

        fn asked(&self) -> Vec<String> {
            self.asked.lock().unwrap().clone()
        }
    }

    impl Net for Images {
        fn source(&self, _req: &SourceReq, _have: Option<&CacheEntry>) -> Fetched {
            Fetched::Failed("no sources here".into())
        }

        fn page(&self, _url: &str) -> Result<String, String> {
            Err("no pages here".into())
        }

        fn image(&self, url: &str) -> Result<Vec<u8>, FetchErr> {
            self.asked.lock().unwrap().push(url.to_string());
            self.answer
                .lock()
                .unwrap()
                .clone()
                .unwrap_or_else(|| Ok(FIXTURE.clone()))
        }
    }

    fn marker(config: &Path, url: &str) -> Option<String> {
        std::fs::read_to_string(images_dir(config).join(pc::image_miss_name(url))).ok()
    }

    #[test]
    fn fill_images_caches_a_release_radar_capsule() {
        let d = dirs("fill-capsule");
        store(&d.config, &[capsule_card(620, NOW - 3_600_000)]);
        let net = Images::default();
        assert_eq!(fill_images(&d.config, &net, NOW), 1);
        assert_eq!(net.asked(), vec![CAPSULE_620.to_string()]);
        let page = crate::pulse::load(&d.config, None, NOW).expect("page");
        assert_eq!(page.feed[0].images, vec![pc::image_cache_name(CAPSULE_620)]);
        // Cached: the next tick asks nothing.
        assert_eq!(fill_images(&d.config, &net, NOW + 60_000), 0);
        assert_eq!(net.asked().len(), 1);
    }

    #[test]
    fn fill_images_never_asks_again_for_a_url_that_answers_html() {
        let d = dirs("fill-html");
        store(&d.config, &[capsule_card(620, NOW - 3_600_000)]);
        let mut html = b"<!doctype html><title>Not found</title>".to_vec();
        html.resize(pc::IMAGE_MIN_BYTES * 2, b' ');
        let net = Images::answering(Ok(html));
        assert_eq!(fill_images(&d.config, &net, NOW), 1);
        assert_eq!(marker(&d.config, CAPSULE_620).as_deref(), Some("gone\n"));
        assert_eq!(fill_images(&d.config, &net, NOW + 3_600_000), 0);
        assert_eq!(net.asked(), vec![CAPSULE_620.to_string()]);
        // The card itself is never edited, and it shows no image.
        let cards = crate::pulse::cards(&d.config).expect("cards");
        assert_eq!(cards[0].pulse.image_urls, vec![CAPSULE_620.to_string()]);
        let page = crate::pulse::load(&d.config, None, NOW).expect("page");
        assert_eq!(page.feed[0].images, Vec::<String>::new());
    }

    #[test]
    fn fill_images_never_asks_again_after_a_404_or_a_tracking_pixel() {
        let d = dirs("fill-404");
        store(&d.config, &[capsule_card(620, NOW - 3_600_000)]);
        let net = Images::answering(Err(FetchErr::Gone("HTTP 404".into())));
        assert_eq!(fill_images(&d.config, &net, NOW), 1);
        assert_eq!(fill_images(&d.config, &net, NOW + 3_600_000), 0);
        assert_eq!(marker(&d.config, CAPSULE_620).as_deref(), Some("gone\n"));

        let d = dirs("fill-pixel");
        store(&d.config, &[capsule_card(620, NOW - 3_600_000)]);
        // A whole 1×1 JPEG: an image, but far under the size floor.
        let pixel = jpeg(1, 1, 90, |_, _| [0, 0, 0]);
        let net = Images::answering(Ok(pixel));
        assert_eq!(fill_images(&d.config, &net, NOW), 1);
        assert_eq!(marker(&d.config, CAPSULE_620).as_deref(), Some("gone\n"));
        assert!(!images_dir(&d.config)
            .join(pc::image_cache_name(CAPSULE_620))
            .exists());
    }

    #[test]
    fn fill_images_keeps_a_url_on_network_error_and_retries_it_twice_later() {
        let d = dirs("fill-later");
        store(&d.config, &[capsule_card(620, NOW - 3_600_000)]);
        let net = Images::answering(Err(FetchErr::Later("network error".into())));
        let min = 60_000;
        assert_eq!(fill_images(&d.config, &net, NOW), 1);
        assert_eq!(
            marker(&d.config, CAPSULE_620).as_deref(),
            Some("later 1 1800000900000\n")
        );
        assert_eq!(fill_images(&d.config, &net, NOW + min), 0, "waits 15 min");
        assert_eq!(fill_images(&d.config, &net, NOW + 15 * min), 1);
        assert_eq!(fill_images(&d.config, &net, NOW + 44 * min), 0, "then 30");
        assert_eq!(fill_images(&d.config, &net, NOW + 45 * min), 1);
        assert_eq!(marker(&d.config, CAPSULE_620).as_deref(), Some("gone\n"));
        assert_eq!(fill_images(&d.config, &net, NOW + 600 * min), 0);
        assert_eq!(net.asked().len(), 3);
        let cards = crate::pulse::cards(&d.config).expect("cards");
        assert_eq!(cards[0].pulse.image_urls.len(), 1);
    }

    #[test]
    fn a_retry_that_works_caches_the_image_and_clears_its_marker() {
        let d = dirs("fill-recover");
        store(&d.config, &[capsule_card(620, NOW - 3_600_000)]);
        let net = Images::answering(Err(FetchErr::Later("HTTP 503".into())));
        assert_eq!(fill_images(&d.config, &net, NOW), 1);
        *net.answer.lock().unwrap() = None;
        assert_eq!(fill_images(&d.config, &net, NOW + 15 * 60_000), 1);
        assert_eq!(marker(&d.config, CAPSULE_620), None);
        assert!(images_dir(&d.config)
            .join(pc::image_cache_name(CAPSULE_620))
            .is_file());
    }

    #[test]
    fn fill_images_stops_at_six_per_tick_newest_first() {
        let d = dirs("fill-six");
        let cards: Vec<_> = (1..=8)
            .map(|app| capsule_card(app, NOW - 3_600_000 + u64::from(app)))
            .collect();
        store(&d.config, &cards);
        let net = Images::default();
        let capsule =
            |app: u32| format!("https://cdn.akamai.steamstatic.com/steam/apps/{app}/header.jpg");
        assert_eq!(fill_images(&d.config, &net, NOW), 6);
        assert_eq!(net.asked(), (3..=8).rev().map(capsule).collect::<Vec<_>>());
        assert_eq!(fill_images(&d.config, &net, NOW + 60_000), 2);
        assert_eq!(net.asked()[6..], [capsule(2), capsule(1)]);
    }

    #[test]
    fn fill_images_never_asks_a_lan_url_an_old_card_or_a_dismissed_one() {
        let d = dirs("fill-lan");
        let mut printer = capsule_card(1, NOW - 60_000);
        printer.pulse.image_urls = vec![
            "http://192.168.1.20/webcam/?action=snapshot".into(),
            "http://localhost:8080/snap.jpg".into(),
            "model-thumb:benchy.stl:1".into(),
        ];
        let old = capsule_card(2, NOW - FILL_WINDOW_MS - 1);
        let mut dismissed = capsule_card(3, NOW - 60_000);
        dismissed.status = grokhub_core::UpdateStatus::Dismissed;
        store(&d.config, &[printer, old, dismissed]);
        let net = Images::default();
        assert_eq!(fill_images(&d.config, &net, NOW), 0);
        assert_eq!(net.asked(), Vec::<String>::new());
        assert!(!images_dir(&d.config).exists(), "no marker either");
    }

    #[test]
    fn fill_images_does_nothing_offline() {
        let d = dirs("fill-offline");
        store(&d.config, &[capsule_card(620, NOW - 3_600_000)]);
        let net = Images::default();
        {
            let _off = grokhub_core::offline::force_for_thread(true);
            assert_eq!(fill_images(&d.config, &net, NOW), 0);
        }
        assert_eq!(net.asked(), Vec::<String>::new());
        assert_eq!(marker(&d.config, CAPSULE_620), None);
        assert_eq!(
            fill_images(&d.config, &net, NOW),
            1,
            "and runs once it's off"
        );
    }

    #[test]
    fn prune_images_keeps_the_marker_of_a_url_a_card_still_names() {
        let d = dirs("prune-miss");
        store(&d.config, &[capsule_card(620, NOW - 3_600_000)]);
        let dir = images_dir(&d.config);
        std::fs::create_dir_all(&dir).expect("dir");
        let named = dir.join(pc::image_miss_name(CAPSULE_620));
        let stray = dir.join(pc::image_miss_name("https://img.test/gone.jpg"));
        let two_hours_ago = SystemTime::now() - Duration::from_secs(2 * 3600);
        for path in [&named, &stray] {
            std::fs::write(path, "gone\n").expect("marker");
            std::fs::File::options()
                .write(true)
                .open(path)
                .and_then(|f| f.set_modified(two_hours_ago))
                .expect("mtime");
        }
        assert_eq!(prune_images(&d.config, grokhub_core::now_ms()), 1);
        assert!(named.is_file());
        assert!(!stray.exists());
    }

    #[test]
    fn a_4xx_is_gone_and_a_5xx_or_rate_limit_is_later() {
        let status = |code| http::Error::Status {
            code,
            body: String::new(),
            retry_after: None,
            message: format!("status code {code}"),
        };
        assert_eq!(
            FetchErr::from(status(404)),
            FetchErr::Gone("HTTP 404".into())
        );
        assert_eq!(
            FetchErr::from(status(410)),
            FetchErr::Gone("HTTP 410".into())
        );
        assert_eq!(
            FetchErr::from(status(429)),
            FetchErr::Later("HTTP 429".into())
        );
        assert_eq!(
            FetchErr::from(status(408)),
            FetchErr::Later("HTTP 408".into())
        );
        assert_eq!(
            FetchErr::from(status(503)),
            FetchErr::Later("HTTP 503".into())
        );
        let transport = http::Error::Transport {
            kind: http::TransportKind::Connect,
            message: "refused".into(),
        };
        assert_eq!(
            FetchErr::from(transport),
            FetchErr::Later("network error".into())
        );
        assert_eq!(
            LiveNet.image("http://192.168.1.20/webcam/?action=snapshot"),
            Err(FetchErr::Gone("not a public address".into()))
        );
    }
}
