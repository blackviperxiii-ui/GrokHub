//! The shell's Pulse Feed writer. On the system cadence, and once right after
//! the feed prompt is saved, it fetches the free sources, asks the model for
//! one post whose cites must be pages fetched this run, caches the post's
//! images locally, and adds the post to the same `updates.json` the page
//! reads. The taste summary is rebuilt first when the person loved,
//! discussed or chatted since the last rebuild.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Condvar, Mutex, MutexGuard};
use std::time::Duration;

use grokhub_agent::harness as hx;
use grokhub_agent::{CancelToken, ContentPart, InputItem, ModelClient, ResponsesRequest};
use grokhub_core::pulse::{self as pc};
use grokhub_core::pulse_sources::{self as ps, CacheEntry, Fetched, SourceReq};
use grokhub_core::pulse_writer::{self as pw, TasteKind, WriterState};

/// Cached post images, named by `pc::image_cache_name`.
pub const IMAGES_DIR: &str = "pulse-images";
const TICK: Duration = Duration::from_secs(60);
const FETCH_TIMEOUT: Duration = Duration::from_secs(20);
const REDIRECTS_MAX: usize = 4;
const PAGE_CAP: u64 = 2 * 1024 * 1024;
const SOURCE_CAP: u64 = 4 * 1024 * 1024;

/// Points the writer's network at a local mock (the shell smoke test). Only a
/// plain-http loopback URL is accepted, so it can never move real traffic.
pub const NET_ENV: &str = "GROKHUB_SHELL_PULSE_NET";

pub const NOTHING_TO_READ: &str =
    "Nothing to look into yet. Add an interest in Settings → Pulse → Interests.";
pub const BUDGET_USED: &str = "Today's post budget is used. The feed picks up tomorrow.";

/// One read-change-write of the writer state at a time.
static STATE: Mutex<()> = Mutex::new(());
/// One run at a time.
static RUNNING: AtomicBool = AtomicBool::new(false);
/// Wakes the clock early (a saved prompt owes a post now).
static WAKE: (Mutex<bool>, Condvar) = (Mutex::new(false), Condvar::new());

fn state_lock() -> MutexGuard<'static, ()> {
    STATE.lock().unwrap_or_else(|e| e.into_inner())
}

fn state_path(config_dir: &Path) -> PathBuf {
    config_dir.join(pw::STATE_FILE)
}

pub fn images_dir(config_dir: &Path) -> PathBuf {
    config_dir.join(IMAGES_DIR)
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

/// The page's local clock, so quiet hours land on the person's own night.
pub fn note_clock(config_dir: &Path, hour: u32, minute: u32, now_ms: u64) -> Result<(), String> {
    with_state(config_dir, |s| {
        s.utc_offset_min = Some(pw::utc_offset_min(hour, minute, now_ms));
    })
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

/// The network a run uses. The live one fetches public pages only.
pub trait Net {
    fn source(&self, req: &SourceReq, have: Option<&CacheEntry>) -> Fetched;
    fn page(&self, url: &str) -> Result<String, String>;
    fn image(&self, url: &str) -> Result<Vec<u8>, String>;
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
    let taste = taste_summary(config_dir, model, client, &state, now_ms);
    let outcome = write_post(config_dir, model, client, net, &state, &taste, now_ms);
    let note = match &outcome {
        Ok(Outcome::Posted(_)) => "Posted a new story.".to_string(),
        Ok(Outcome::Nothing(why)) => why.clone(),
        Err(err) => format!("The last post failed: {err}"),
    };
    with_state(config_dir, |s| s.note = note)?;
    outcome
}

/// The current taste note, rebuilt first when new taste events landed.
fn taste_summary(
    config_dir: &Path,
    model: &str,
    client: &dyn ModelClient,
    state: &WriterState,
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

fn write_post(
    config_dir: &Path,
    model: &str,
    client: &dyn ModelClient,
    net: &dyn Net,
    state: &WriterState,
    taste: &str,
    now_ms: u64,
) -> Result<Outcome, String> {
    let profile =
        grokhub_core::interests::load(&config_dir.join(grokhub_core::interests::FILE_NAME));
    let topics = profile.search_topics();
    let cache_path = config_dir.join(ps::CACHE_FILE);
    let mut cache = ps::load_cache(&cache_path);
    let reqs = ps::requests_for(&ps::SourceSettings::default(), &topics, cache.runs, now_ms);
    if reqs.is_empty() {
        return Ok(Outcome::Nothing(NOTHING_TO_READ.into()));
    }
    if !cache.take_summary_call(now_ms) {
        return Ok(Outcome::Nothing(BUDGET_USED.into()));
    }
    let got = ps::gather(&reqs, &mut cache, now_ms, &mut |req, have| {
        net.source(req, have)
    });
    cache.last_run_ms = now_ms;
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
    let research = pw::research_from(&picks);
    let recent = pw::recent_titles(&cards);
    let messages = pw::writer_messages(&pw::WriterInput {
        prompt: state.prompt_or_default(),
        taste,
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
            cache.runs = cache.runs.saturating_add(1);
            ps::save_cache(&cache_path, &cache)?;
            return Ok(Outcome::Nothing(why));
        }
    };
    let mut card = pw::post_card(&written, &research, now_ms);
    card.pulse.image_urls = cache_gallery(config_dir, net, &written.cites);
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
    cache.runs = cache.runs.saturating_add(1);
    ps::save_cache(&cache_path, &cache)?;
    Ok(Outcome::Posted(id))
}

/// The first cited page's gallery, downloaded once into the local cache.
/// Returns the image URLs that are cached; the page shows those files and
/// never loads the image from its site.
fn cache_gallery(config_dir: &Path, net: &dyn Net, cites: &[String]) -> Vec<String> {
    let Some(page_url) = cites.first() else {
        return Vec::new();
    };
    let Ok(html) = net.page(page_url) else {
        return Vec::new();
    };
    let dir = images_dir(config_dir);
    let mut kept = Vec::new();
    for url in pc::gallery_images(&html, page_url) {
        let file = dir.join(pc::image_cache_name(&url));
        if !file.is_file() {
            let Ok(bytes) = net.image(&url) else {
                continue;
            };
            if !pc::image_bytes_ok(&bytes)
                || std::fs::create_dir_all(&dir).is_err()
                || std::fs::write(&file, &bytes).is_err()
            {
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
    let req = ResponsesRequest {
        model: model.to_string(),
        effort: Some(grokhub_core::BACKGROUND_EFFORT.to_string()),
        input: messages
            .iter()
            .map(|(role, text)| InputItem::Message {
                role: role.clone(),
                content: vec![ContentPart::InputText(text.clone())],
            })
            .collect(),
        conversation_id: "pulse-writer".into(),
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
    let state = state(config_dir);
    let quiet = state.utc_offset_min.is_some_and(|offset| {
        grokhub_core::quiet_hours_active(&pw::local_hm(now, offset), pw::QUIET_START, pw::QUIET_END)
    });
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
    RUNNING.store(false, Ordering::SeqCst);
}

/// The live network, or the loopback mock when [`NET_ENV`] names one.
fn net() -> Box<dyn Net> {
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

    fn get(&self, kind: &str, url: &str) -> Result<Vec<u8>, String> {
        let at = format!("{}/pulse/{kind}?url={}", self.base, ps::query_value(url));
        let resp = ureq::get(&at)
            .timeout(FETCH_TIMEOUT)
            .call()
            .map_err(|e| e.to_string())?;
        let mut buf = Vec::new();
        resp.into_reader()
            .take(SOURCE_CAP)
            .read_to_end(&mut buf)
            .map_err(|e| e.to_string())?;
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
            Err(why) => Fetched::Failed(why),
        }
    }

    fn page(&self, url: &str) -> Result<String, String> {
        self.get("page", url)
            .map(|body| String::from_utf8_lossy(&body).into_owned())
    }

    fn image(&self, url: &str) -> Result<Vec<u8>, String> {
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
    fn agent() -> ureq::Agent {
        ureq::AgentBuilder::new()
            .redirects(0)
            .timeout(FETCH_TIMEOUT)
            .resolver(grokhub_agent::public_only_resolve)
            .build()
    }

    fn get(
        url: &str,
        accept: &str,
        data: &[hx::DataClass],
        have: Option<&CacheEntry>,
        cap: u64,
    ) -> Result<Got, String> {
        let mut url = url.to_string();
        for hop in 0..=REDIRECTS_MAX {
            if !grokhub_core::public_http_url(&url) {
                return Err("not a public address".into());
            }
            hx::guard_quiet(
                &grokhub_agent::perm::config_dir(),
                &hx::EgressReq::new(&url, data),
            )?;
            let mut call = Self::agent()
                .get(&url)
                .set("user-agent", grokhub_agent::USER_AGENT)
                .set("accept", accept);
            if hop == 0 {
                if let Some(entry) = have {
                    if let Some(etag) = entry.etag.as_deref() {
                        call = call.set("if-none-match", etag);
                    }
                    if let Some(when) = entry.last_modified.as_deref() {
                        call = call.set("if-modified-since", when);
                    }
                }
            }
            let resp = match call.call() {
                Ok(resp) => resp,
                Err(ureq::Error::Status(code, _)) => return Err(format!("HTTP {code}")),
                Err(ureq::Error::Transport(_)) => return Err("network error".into()),
            };
            let status = resp.status();
            if (300..400).contains(&status) && status != 304 {
                match resp.header("location") {
                    Some(loc) => {
                        url = redirect_target(&url, loc);
                        continue;
                    }
                    None => return Err(format!("HTTP {status}")),
                }
            }
            let etag = resp.header("etag").map(str::to_string);
            let modified = resp.header("last-modified").map(str::to_string);
            let mut buf = Vec::new();
            resp.into_reader()
                .take(cap)
                .read_to_end(&mut buf)
                .map_err(|_| "timed out or dropped".to_string())?;
            return Ok(Got {
                status,
                body: buf,
                etag,
                last_modified: modified,
            });
        }
        Err("too many redirects".into())
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
            Err(why) => Fetched::Failed(why),
        }
    }

    fn page(&self, url: &str) -> Result<String, String> {
        let got = Self::get(url, "text/html", &[], None, PAGE_CAP)?;
        Ok(String::from_utf8_lossy(&got.body).into_owned())
    }

    fn image(&self, url: &str) -> Result<Vec<u8>, String> {
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

        fn image(&self, url: &str) -> Result<Vec<u8>, String> {
            assert_eq!(url, "https://blog.rust-lang.org/hero.jpg");
            let mut jpg = vec![0xFF, 0xD8, 0xFF, 0xE0];
            jpg.resize(pc::IMAGE_MIN_BYTES * 2, 7);
            Ok(jpg)
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
            (pc::IMAGE_MIN_BYTES * 2) as u64
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
}
