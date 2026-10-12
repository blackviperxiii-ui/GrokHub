//! Free Pulse sources: Hacker News (Algolia), Reddit (public JSON), DuckDuckGo Lite
//! web search, GitHub release feeds, YouTube channel feeds and any RSS or Atom feed.
//! No keys, no paid search. This module builds the requests, parses the replies and
//! keeps the ETag cache; the app does the HTTP. Every item it yields has a title and
//! a real link.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::update_feed::{digest_card, UpdateCard};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    HackerNews,
    Reddit,
    WebSearch,
    GithubReleases,
    YouTube,
    Rss,
}

impl SourceKind {
    pub const ALL: [SourceKind; 6] = [
        SourceKind::HackerNews,
        SourceKind::Reddit,
        SourceKind::WebSearch,
        SourceKind::GithubReleases,
        SourceKind::YouTube,
        SourceKind::Rss,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::HackerNews => "Hacker News",
            Self::Reddit => "Reddit",
            Self::WebSearch => "Web search",
            Self::GithubReleases => "GitHub releases",
            Self::YouTube => "YouTube",
            Self::Rss => "RSS and changelogs",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Self::HackerNews => "hn",
            Self::Reddit => "reddit",
            Self::WebSearch => "web",
            Self::GithubReleases => "github",
            Self::YouTube => "youtube",
            Self::Rss => "rss",
        }
    }
}

fn yes() -> bool {
    true
}

fn every_default() -> u32 {
    EVERY_DEFAULT_MINS
}

/// Settings → Pulse → Sources. Each source can be off; the lists are what to follow.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceSettings {
    #[serde(default = "yes")]
    pub hacker_news: bool,
    #[serde(default = "yes")]
    pub reddit: bool,
    /// DuckDuckGo Lite topic search (no API key).
    #[serde(default = "yes")]
    pub web_search: bool,
    #[serde(default = "yes")]
    pub github: bool,
    #[serde(default = "yes")]
    pub youtube: bool,
    #[serde(default = "yes")]
    pub rss: bool,
    /// RSS or Atom feed URLs.
    #[serde(default)]
    pub feeds: Vec<String>,
    /// `owner/name` GitHub repos whose releases to follow.
    #[serde(default)]
    pub repos: Vec<String>,
    /// Subreddit names without `r/`.
    #[serde(default)]
    pub subreddits: Vec<String>,
    /// YouTube channel ids (`UC…`).
    #[serde(default)]
    pub channels: Vec<String>,
    /// Minutes between background checks, one of `EVERY_CHOICES`.
    #[serde(default = "every_default")]
    pub every_mins: u32,
}

impl Default for SourceSettings {
    fn default() -> Self {
        Self {
            hacker_news: true,
            reddit: true,
            web_search: true,
            github: true,
            youtube: true,
            rss: true,
            feeds: Vec::new(),
            repos: Vec::new(),
            subreddits: Vec::new(),
            channels: Vec::new(),
            every_mins: EVERY_DEFAULT_MINS,
        }
    }
}

impl SourceSettings {
    /// How often the background check runs, held to 1–4 hours.
    pub fn every_ms(&self) -> u64 {
        let lo = EVERY_CHOICES[0];
        let hi = EVERY_CHOICES[EVERY_CHOICES.len() - 1];
        u64::from(self.every_mins.clamp(lo, hi)) * 60 * 1000
    }

    pub fn on(&self, kind: SourceKind) -> bool {
        match kind {
            SourceKind::HackerNews => self.hacker_news,
            SourceKind::Reddit => self.reddit,
            SourceKind::WebSearch => self.web_search,
            SourceKind::GithubReleases => self.github,
            SourceKind::YouTube => self.youtube,
            SourceKind::Rss => self.rss,
        }
    }

    pub fn set_on(&mut self, kind: SourceKind, on: bool) {
        match kind {
            SourceKind::HackerNews => self.hacker_news = on,
            SourceKind::Reddit => self.reddit = on,
            SourceKind::WebSearch => self.web_search = on,
            SourceKind::GithubReleases => self.github = on,
            SourceKind::YouTube => self.youtube = on,
            SourceKind::Rss => self.rss = on,
        }
    }

    /// The list a kind follows. Hacker News and Web search have none: they search by topic.
    pub fn list_mut(&mut self, kind: SourceKind) -> Option<&mut Vec<String>> {
        match kind {
            SourceKind::HackerNews | SourceKind::WebSearch => None,
            SourceKind::Reddit => Some(&mut self.subreddits),
            SourceKind::GithubReleases => Some(&mut self.repos),
            SourceKind::YouTube => Some(&mut self.channels),
            SourceKind::Rss => Some(&mut self.feeds),
        }
    }

    /// Add one entry in its canonical form. Err says what was wrong.
    pub fn add(&mut self, kind: SourceKind, raw: &str) -> Result<String, String> {
        let entry = canonical_entry(kind, raw)?;
        let list = self.list_mut(kind).ok_or_else(|| {
            format!(
                "{} follows your interests; there is nothing to add.",
                kind.label()
            )
        })?;
        if let Some(have) = list.iter().find(|have| have.eq_ignore_ascii_case(&entry)) {
            return Err(format!("{have} is already on the list."));
        }
        if list.len() >= LIST_MAX {
            return Err(format!("At most {LIST_MAX} per source."));
        }
        list.push(entry.clone());
        Ok(entry)
    }
}

/// Settings → Pulse → Check every, in minutes.
pub const EVERY_CHOICES: [u32; 5] = [60, 90, 120, 180, 240];
pub const EVERY_DEFAULT_MINS: u32 = 90;
/// When the app opens, a check runs if the last one is at least this old.
pub const OPEN_DEBOUNCE_MS: u64 = 10 * 60 * 1000;
/// Fast-model calls a day for post summaries. One call covers a whole run, so the
/// default 90-minute clock (16 runs a day) never runs out.
pub const SUMMARY_CALLS_PER_DAY: u32 = 16;
/// What a model summary may hold: two or three short sentences.
pub const MODEL_SUMMARY_CHARS: usize = 300;
const DAY_MS: u64 = 24 * 60 * 60 * 1000;

/// Is a background check due? Never run: yes. Otherwise once the interval passed,
/// or right after the app opened when the last check is more than a few minutes old.
pub fn check_due(last_run_ms: u64, now_ms: u64, every_ms: u64, just_opened: bool) -> bool {
    if last_run_ms == 0 {
        return true;
    }
    let since = now_ms.saturating_sub(last_run_ms);
    since >= every_ms || (just_opened && since >= OPEN_DEBOUNCE_MS)
}

/// Most entries one source list holds.
pub const LIST_MAX: usize = 30;
/// Interest topics searched on Hacker News, Reddit and Web search per run.
pub const TOPIC_QUERIES: usize = 3;
/// Most results kept from one DuckDuckGo Lite page.
pub const WEB_SEARCH_MAX: usize = 10;
/// How far back a topic search looks.
pub const SEARCH_WINDOW_MS: u64 = 3 * 24 * 60 * 60 * 1000;
/// A source fetched this recently is read from the cache instead.
pub const REFETCH_MS: u64 = 15 * 60 * 1000;
/// Items older than this never post.
pub const ITEM_MAX_AGE_MS: u64 = 7 * 24 * 60 * 60 * 1000;
/// Feed posts one run adds.
pub const POSTS_PER_RUN: usize = 5;
/// Most posts one `source_name` may contribute in a single run (stops one place flooding).
pub const PER_SOURCE_MAX: usize = 2;
/// Positive Save / More like this hits before Pulse follows a place on its own.
pub const LEARN_PLACE_HITS: u32 = 2;
/// Learned place rows kept in the source cache.
pub const LEARNED_PLACES_MAX: usize = 40;
/// Prior posts from a source below this get a novelty score bump.
pub const SOURCE_NOVELTY_BELOW: u32 = 3;
/// Percent added to an under-seen source's score when the cache has post history.
pub const SOURCE_NOVELTY_PCT: u32 = 20;
/// Canonical URLs remembered as already posted.
pub const SEEN_MAX: usize = 600;
pub const SUMMARY_CHARS: usize = 280;
pub const CACHE_FILE: &str = "pulse-sources.json";

/// Extension point, off: X and general web through xAI's paid live search. The app
/// owner chose free sources only (2026-10-09), so nothing builds or sends these
/// requests. Turning it on needs their OK on the spend, an adapter that builds the
/// request beside `requests`, and a Settings switch in place of the disabled row.
pub const PAID_SEARCH_ENABLED: bool = false;
/// The disabled row's label in Settings → Pulse → Sources.
pub const PAID_SEARCH_LABEL: &str = "X and web search (xAI, paid)";
pub const PAID_SEARCH_NOTE: &str = "Off. Pulse uses free sources only for now.";

/// The form an entry is stored in, from what a person might paste.
pub fn canonical_entry(kind: SourceKind, raw: &str) -> Result<String, String> {
    let raw = raw.trim();
    match kind {
        SourceKind::HackerNews => Err("Hacker News follows your interests.".into()),
        SourceKind::WebSearch => Err("Web search follows your interests.".into()),
        SourceKind::Reddit => {
            let name = raw
                .trim_start_matches("https://")
                .trim_start_matches("www.")
                .trim_start_matches("reddit.com")
                .trim_start_matches('/')
                .trim_start_matches("r/")
                .trim_end_matches('/');
            let ok = (2..=21).contains(&name.len())
                && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
            if ok {
                Ok(name.to_string())
            } else {
                Err("A subreddit is a name like rust or r/rust.".into())
            }
        }
        SourceKind::GithubReleases => {
            let path = raw
                .trim_start_matches("https://")
                .trim_start_matches("http://")
                .trim_start_matches("github.com/")
                .trim_end_matches('/')
                .trim_end_matches(".git");
            let parts: Vec<&str> = path.split('/').take(3).collect();
            let part_ok = |p: &str| {
                !p.is_empty()
                    && p.len() <= 100
                    && p.chars()
                        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
                    && p != "."
                    && p != ".."
            };
            if parts.len() >= 2 && part_ok(parts[0]) && part_ok(parts[1]) {
                Ok(format!("{}/{}", parts[0], parts[1]))
            } else {
                Err("A repo is owner/name, like rust-lang/rust.".into())
            }
        }
        SourceKind::YouTube => {
            let id = raw.rsplit(['/', '=']).next().unwrap_or("").trim();
            let ok = id.len() == 24
                && id.starts_with("UC")
                && id
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'));
            if ok {
                Ok(id.to_string())
            } else {
                Err(
                    "A channel id starts with UC and is 24 characters (from the channel's URL)."
                        .into(),
                )
            }
        }
        SourceKind::Rss => {
            let ok = (raw.starts_with("https://") || raw.starts_with("http://"))
                && raw.len() <= 500
                && !raw.chars().any(char::is_whitespace)
                && crate::pulse::source_host(raw).is_some();
            if ok {
                Ok(raw.to_string())
            } else {
                Err("A feed is a full http(s) URL.".into())
            }
        }
    }
}

/// One fetch the app makes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceReq {
    pub kind: SourceKind,
    pub url: String,
    /// What a post from it says it came from when the item has nothing better.
    pub name: String,
    /// The interest a topic search was for.
    pub topic: Option<String>,
}

/// Percent-encode for a query value.
pub fn query_value(text: &str) -> String {
    let mut out = String::new();
    for b in text.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// Which interest topics this check searches. Keeps the strongest first, then
/// rotates the remaining slots so weaker interests still get turns.
pub fn topic_queries(topics: &[String], run_index: u64) -> Vec<String> {
    if topics.is_empty() {
        return Vec::new();
    }
    if topics.len() <= TOPIC_QUERIES {
        return topics.to_vec();
    }
    let mut out = vec![topics[0].clone()];
    let rest = &topics[1..];
    let start = (run_index as usize) % rest.len();
    for i in 0..rest.len() {
        if out.len() >= TOPIC_QUERIES {
            break;
        }
        let topic = &rest[(start + i) % rest.len()];
        if !out.iter().any(|have| have.eq_ignore_ascii_case(topic)) {
            out.push(topic.clone());
        }
    }
    out
}

/// Every fetch for one run. `topics` strongest first, muted ones already left out.
/// `run_index` rotates which weaker topics are searched (`topic_queries`).
pub fn requests(s: &SourceSettings, topics: &[String], now_ms: u64) -> Vec<SourceReq> {
    requests_for(s, topics, 0, now_ms)
}

/// Like [`requests`], with an explicit run index for topic rotation.
pub fn requests_for(
    s: &SourceSettings,
    topics: &[String],
    run_index: u64,
    now_ms: u64,
) -> Vec<SourceReq> {
    let mut out = Vec::new();
    let searched = topic_queries(topics, run_index);
    if s.hacker_news {
        let since = now_ms.saturating_sub(SEARCH_WINDOW_MS) / 1000;
        for topic in &searched {
            out.push(SourceReq {
                kind: SourceKind::HackerNews,
                url: format!(
                    "https://hn.algolia.com/api/v1/search?query={}&tags=story&numericFilters=created_at_i%3E{since}&hitsPerPage=10",
                    query_value(topic)
                ),
                name: "Hacker News".into(),
                topic: Some(topic.clone()),
            });
        }
    }
    if s.reddit {
        for sub in &s.subreddits {
            out.push(SourceReq {
                kind: SourceKind::Reddit,
                url: format!("https://www.reddit.com/r/{sub}/hot.json?limit=15&raw_json=1"),
                name: format!("r/{sub}"),
                topic: None,
            });
        }
        for topic in &searched {
            out.push(SourceReq {
                kind: SourceKind::Reddit,
                url: format!(
                    "https://www.reddit.com/search.json?q={}&sort=top&t=week&limit=10&raw_json=1",
                    query_value(topic)
                ),
                name: "Reddit".into(),
                topic: Some(topic.clone()),
            });
        }
    }
    if s.web_search {
        for topic in &searched {
            out.push(SourceReq {
                kind: SourceKind::WebSearch,
                url: format!("https://lite.duckduckgo.com/lite/?q={}", query_value(topic)),
                name: "Web search".into(),
                topic: Some(topic.clone()),
            });
        }
    }
    if s.github {
        for repo in &s.repos {
            out.push(SourceReq {
                kind: SourceKind::GithubReleases,
                url: format!("https://github.com/{repo}/releases.atom"),
                name: format!("{repo} on GitHub"),
                topic: None,
            });
        }
    }
    if s.youtube {
        for id in &s.channels {
            out.push(SourceReq {
                kind: SourceKind::YouTube,
                url: format!("https://www.youtube.com/feeds/videos.xml?channel_id={id}"),
                name: "YouTube".into(),
                topic: None,
            });
        }
    }
    if s.rss {
        for url in &s.feeds {
            out.push(SourceReq {
                kind: SourceKind::Rss,
                url: url.clone(),
                name: crate::pulse::source_host(url).unwrap_or_else(|| "RSS".into()),
                topic: None,
            });
        }
    }
    out
}

/// One thing a source returned.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceItem {
    pub title: String,
    pub url: String,
    pub kind: SourceKind,
    /// "Hacker News", "r/rust", "rust-lang/rust on GitHub", the channel or site name.
    pub source_name: String,
    /// Unix ms. 0 when the source gave none.
    pub published_ms: u64,
    /// Plain text, at most SUMMARY_CHARS.
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub image: Option<String>,
    /// The interest a topic search found it for.
    #[serde(default)]
    pub topic: Option<String>,
    /// Hacker News points or Reddit score. 0 for feeds.
    #[serde(default)]
    pub engagement: u32,
}

pub fn parse(req: &SourceReq, body: &str) -> Vec<SourceItem> {
    let mut items = match req.kind {
        SourceKind::HackerNews => parse_hn(req, body),
        SourceKind::Reddit => parse_reddit(req, body),
        SourceKind::WebSearch => parse_web_search(req, body),
        SourceKind::GithubReleases | SourceKind::YouTube | SourceKind::Rss => parse_feed(req, body),
    };
    items.retain(|i| !i.title.is_empty() && http_url(&i.url));
    items
}

fn http_url(url: &str) -> bool {
    (url.starts_with("https://") || url.starts_with("http://"))
        && !url.chars().any(char::is_whitespace)
        && crate::pulse::source_host(url).is_some()
}

fn one_line(text: &str, max: usize) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= max {
        return flat;
    }
    let mut cut: String = flat.chars().take(max).collect();
    if let Some(at) = cut.rfind(' ') {
        cut.truncate(at);
    }
    format!("{}…", cut.trim_end_matches([',', ';', ':', '.']))
}

fn parse_hn(req: &SourceReq, body: &str) -> Vec<SourceItem> {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(body) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for hit in v["hits"].as_array().into_iter().flatten() {
        let title = one_line(hit["title"].as_str().unwrap_or(""), 200);
        let id = hit["objectID"].as_str().unwrap_or("");
        let discussion = format!("https://news.ycombinator.com/item?id={id}");
        let url = hit["url"]
            .as_str()
            .filter(|u| http_url(u))
            .map(str::to_string)
            .unwrap_or(discussion);
        let points = hit["points"].as_u64().unwrap_or(0);
        let comments = hit["num_comments"].as_u64().unwrap_or(0);
        out.push(SourceItem {
            title,
            url,
            kind: SourceKind::HackerNews,
            source_name: "Hacker News".into(),
            published_ms: hit["created_at_i"].as_u64().unwrap_or(0) * 1000,
            summary: format!("{points} points, {comments} comments on Hacker News."),
            image: None,
            topic: req.topic.clone(),
            engagement: u32::try_from(points).unwrap_or(u32::MAX),
        });
    }
    out
}

/// Percent-decode a query value once (`%XX` only; `+` stays `+`).
fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hi = hex_nibble(bytes[i + 1]);
            let lo = hex_nibble(bytes[i + 2]);
            if let (Some(h), Some(l)) = (hi, lo) {
                out.push((h << 4) | l);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_nibble(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// HTML attribute value for `name=` (double, single, or bare).
fn html_attr(tag: &str, name: &str) -> Option<String> {
    let needle = format!("{name}=");
    let rest = tag.split(&needle).nth(1)?;
    let rest = rest.trim_start();
    let (raw, _) = match rest.as_bytes().first() {
        Some(b'"') => {
            let end = rest[1..].find('"')?;
            (&rest[1..1 + end], ())
        }
        Some(b'\'') => {
            let end = rest[1..].find('\'')?;
            (&rest[1..1 + end], ())
        }
        _ => {
            let end = rest
                .find(|c: char| c.is_whitespace() || c == '>')
                .unwrap_or(rest.len());
            (&rest[..end], ())
        }
    };
    Some(decode_entities(raw))
}

/// Turn a DuckDuckGo result href into the destination URL, or None.
fn unwrap_ddg_url(href: &str) -> Option<String> {
    let href = href.trim();
    let abs = if let Some(rest) = href.strip_prefix("//") {
        format!("https://{rest}")
    } else if href.starts_with("https://") || href.starts_with("http://") {
        href.to_string()
    } else {
        return None;
    };
    let host = crate::pulse::source_host(&abs)?;
    if host == "duckduckgo.com" || host == "www.duckduckgo.com" || host == "duck.com" {
        // Bounce links: /l/?uddg=<encoded destination>
        let path_q = abs
            .strip_prefix("https://")
            .or_else(|| abs.strip_prefix("http://"))?;
        let after_host = path_q.split_once('/').map(|(_, p)| p).unwrap_or("");
        if !(after_host.starts_with("l/") || after_host.starts_with("l?")) {
            return None;
        }
        let q = after_host.split('?').nth(1).unwrap_or("");
        for pair in q.split('&') {
            let Some((k, v)) = pair.split_once('=') else {
                continue;
            };
            if k == "uddg" {
                let dest = percent_decode(v);
                if http_url(&dest) {
                    let dest_host = crate::pulse::source_host(&dest)?;
                    if dest_host != "duckduckgo.com"
                        && dest_host != "www.duckduckgo.com"
                        && dest_host != "duck.com"
                    {
                        return Some(dest);
                    }
                }
                return None;
            }
        }
        return None;
    }
    http_url(&abs).then_some(abs)
}

fn ddg_nav_href(href: &str) -> bool {
    let h = href.trim();
    h.is_empty()
        || h.starts_with('#')
        // Site-relative nav (`/lite/`), not protocol-relative result links (`//duckduckgo.com/…`).
        || (h.starts_with('/') && !h.starts_with("//"))
        || h.contains("duckduckgo.com/y.js")
        || h.contains("duckduckgo.com/settings")
}

/// DuckDuckGo Lite HTML: result anchors, uddg unwrap, source_name = result host.
fn parse_web_search(req: &SourceReq, body: &str) -> Vec<SourceItem> {
    let mut out = Vec::new();
    let mut rest = body;
    while out.len() < WEB_SEARCH_MAX {
        let Some(at) = rest.find("<a") else {
            break;
        };
        rest = &rest[at..];
        let Some(tag_end) = rest.find('>') else {
            break;
        };
        let tag = &rest[..=tag_end];
        rest = &rest[tag_end + 1..];
        let Some(close) = rest.find("</a>") else {
            break;
        };
        let title_raw = &rest[..close];
        rest = &rest[close + 4..];
        let Some(href) = html_attr(tag, "href") else {
            continue;
        };
        if ddg_nav_href(&href) {
            continue;
        }
        let Some(url) = unwrap_ddg_url(&href) else {
            continue;
        };
        let title = one_line(&decode_entities(&strip_tags(title_raw)), 200);
        if title.is_empty() {
            continue;
        }
        let Some(source_name) = crate::pulse::source_host(&url) else {
            continue;
        };
        if out.iter().any(|i: &SourceItem| i.url == url) {
            continue;
        }
        out.push(SourceItem {
            title,
            url,
            kind: SourceKind::WebSearch,
            source_name,
            published_ms: 0,
            summary: String::new(),
            image: None,
            topic: req.topic.clone(),
            engagement: 0,
        });
    }
    out
}

fn parse_reddit(req: &SourceReq, body: &str) -> Vec<SourceItem> {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(body) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for child in v["data"]["children"].as_array().into_iter().flatten() {
        let d = &child["data"];
        if d["over_18"].as_bool().unwrap_or(false) || d["stickied"].as_bool().unwrap_or(false) {
            continue;
        }
        let permalink = d["permalink"].as_str().unwrap_or("");
        let thread = format!("https://www.reddit.com{permalink}");
        let is_self = d["is_self"].as_bool().unwrap_or(false);
        let url = d["url_overridden_by_dest"]
            .as_str()
            .or_else(|| d["url"].as_str())
            .filter(|u| !is_self && http_url(u))
            .map(str::to_string)
            .unwrap_or(thread);
        let image = d["preview"]["images"][0]["source"]["url"]
            .as_str()
            .or_else(|| d["thumbnail"].as_str())
            .filter(|u| u.starts_with("https://"))
            .map(str::to_string);
        let sub = d["subreddit"].as_str().unwrap_or("");
        out.push(SourceItem {
            title: one_line(d["title"].as_str().unwrap_or(""), 200),
            url,
            kind: SourceKind::Reddit,
            source_name: if sub.is_empty() {
                req.name.clone()
            } else {
                format!("r/{sub}")
            },
            published_ms: (d["created_utc"].as_f64().unwrap_or(0.0).max(0.0) as u64) * 1000,
            summary: one_line(d["selftext"].as_str().unwrap_or(""), SUMMARY_CHARS),
            image,
            topic: req.topic.clone(),
            engagement: u32::try_from(d["score"].as_i64().unwrap_or(0).max(0)).unwrap_or(u32::MAX),
        });
    }
    out
}

#[derive(Default)]
struct Entry {
    title: String,
    link: String,
    alt_link: String,
    date: String,
    summary: String,
    image: String,
}

/// RSS 2.0 or Atom, including GitHub release feeds and YouTube channel feeds.
fn parse_feed(req: &SourceReq, body: &str) -> Vec<SourceItem> {
    use quick_xml::events::Event;
    let mut reader = quick_xml::Reader::from_str(body);
    reader.config_mut().trim_text(false);
    let mut out = Vec::new();
    let mut feed_title = String::new();
    let mut stack: Vec<String> = Vec::new();
    let mut entry: Option<Entry> = None;
    let mut text = String::new();
    while let Ok(event) = reader.read_event() {
        match event {
            Event::Start(e) => {
                let name = local(e.local_name().as_ref());
                if name == "item" || name == "entry" {
                    entry = Some(Entry::default());
                }
                if let Some(cur) = entry.as_mut() {
                    take_attrs(cur, &name, &e);
                }
                stack.push(name);
                text.clear();
            }
            Event::Empty(e) => {
                let name = local(e.local_name().as_ref());
                if let Some(cur) = entry.as_mut() {
                    take_attrs(cur, &name, &e);
                }
            }
            Event::Text(t) => {
                if let Ok(s) = t.decode() {
                    text.push_str(&s);
                }
            }
            Event::CData(c) => {
                if let Ok(s) = c.decode() {
                    text.push_str(&s);
                }
            }
            Event::GeneralRef(r) => {
                if let Ok(name) = r.decode() {
                    text.push_str(&entity(&name));
                }
            }
            Event::End(_) => {
                let name = stack.pop().unwrap_or_default();
                let value = text.trim().to_string();
                text.clear();
                match entry.as_mut() {
                    Some(cur) => match name.as_str() {
                        "item" | "entry" => {
                            if let Some(done) = entry.take() {
                                out.push(finish(req, &feed_title, done));
                            }
                        }
                        "title" if cur.title.is_empty() => cur.title = value,
                        "link" if cur.link.is_empty() && !value.is_empty() => cur.link = value,
                        "guid" if cur.alt_link.is_empty() && http_url(&value) => {
                            cur.alt_link = value
                        }
                        "pubDate" | "published" | "date" if !value.is_empty() => cur.date = value,
                        "updated" if cur.date.is_empty() => cur.date = value,
                        "description" | "summary" | "content" | "encoded"
                            if cur.summary.is_empty() && !value.is_empty() =>
                        {
                            cur.summary = value
                        }
                        _ => {}
                    },
                    None => {
                        if name == "title" && feed_title.is_empty() && stack.len() <= 2 {
                            feed_title = value;
                        }
                    }
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    out
}

fn local(name: &[u8]) -> String {
    String::from_utf8_lossy(name).to_string()
}

fn attr(e: &quick_xml::events::BytesStart<'_>, key: &str) -> Option<String> {
    e.attributes()
        .flatten()
        .find(|a| a.key.local_name().as_ref() == key.as_bytes())
        .map(|a| decode_entities(&String::from_utf8_lossy(&a.value)))
}

fn take_attrs(cur: &mut Entry, name: &str, e: &quick_xml::events::BytesStart<'_>) {
    match name {
        // Atom: <link rel="alternate" href="…"/>; rel defaults to alternate.
        "link" => {
            let rel = attr(e, "rel").unwrap_or_else(|| "alternate".into());
            if let Some(href) = attr(e, "href") {
                if rel == "alternate" && cur.link.is_empty() {
                    cur.link = href;
                }
            }
        }
        "thumbnail" if cur.image.is_empty() => {
            if let Some(url) = attr(e, "url") {
                cur.image = url;
            }
        }
        "content" | "enclosure" if cur.image.is_empty() => {
            let typ = attr(e, "type").unwrap_or_default();
            let medium = attr(e, "medium").unwrap_or_default();
            if typ.starts_with("image/") || medium == "image" {
                if let Some(url) = attr(e, "url") {
                    cur.image = url;
                }
            }
        }
        _ => {}
    }
}

fn finish(req: &SourceReq, feed_title: &str, e: Entry) -> SourceItem {
    let link = if http_url(&e.link) {
        e.link
    } else {
        e.alt_link
    };
    let source_name = match req.kind {
        SourceKind::GithubReleases => req.name.clone(),
        _ => {
            let t = one_line(&decode_entities(feed_title), 40);
            if t.is_empty() {
                req.name.clone()
            } else {
                t
            }
        }
    };
    // Many feeds escape their HTML twice (`&amp;nbsp;`), so decode a second time.
    let plain = |raw: &str| decode_entities(&decode_entities(&strip_tags(raw)));
    SourceItem {
        title: one_line(&plain(&e.title), 200),
        url: link,
        kind: req.kind,
        source_name,
        published_ms: parse_date_ms(&e.date).unwrap_or(0),
        summary: one_line(&plain(&e.summary), SUMMARY_CHARS),
        image: Some(e.image).filter(|u| u.starts_with("https://")),
        topic: req.topic.clone(),
        engagement: 0,
    }
}

fn entity(name: &str) -> String {
    match name {
        "amp" => "&".into(),
        "lt" => "<".into(),
        "gt" => ">".into(),
        "quot" => "\"".into(),
        "apos" => "'".into(),
        "nbsp" => " ".into(),
        _ => {
            let code = name
                .strip_prefix("#x")
                .or_else(|| name.strip_prefix("#X"))
                .and_then(|h| u32::from_str_radix(h, 16).ok())
                .or_else(|| name.strip_prefix('#').and_then(|d| d.parse().ok()));
            match code.and_then(char::from_u32) {
                Some(c) => c.to_string(),
                None => format!("&{name};"),
            }
        }
    }
}

/// `&amp;`, `&#39;`, `&#x2019;` and friends, in text that was escaped once more
/// inside a feed's HTML description.
pub fn decode_entities(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        let tail = &rest[at + 1..];
        match tail.find(';').filter(|&end| end <= 10) {
            Some(end) => {
                out.push_str(&entity(&tail[..end]));
                rest = &tail[end + 1..];
            }
            None => {
                out.push('&');
                rest = tail;
            }
        }
    }
    out.push_str(rest);
    out
}

/// Drop HTML tags from a description; block tags become spaces.
pub fn strip_tags(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut in_tag = false;
    for c in html.chars() {
        match c {
            '<' => {
                in_tag = true;
                out.push(' ');
            }
            '>' if in_tag => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out
}

/// Days since 1970-01-01 for a civil date.
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn to_ms(y: i64, mo: i64, d: i64, h: i64, mi: i64, s: i64, offset_min: i64) -> Option<u64> {
    if !(1..=12).contains(&mo) || !(1..=31).contains(&d) || h > 23 || mi > 59 || s > 60 {
        return None;
    }
    let secs = days_from_civil(y, mo, d) * 86_400 + h * 3600 + mi * 60 + s - offset_min * 60;
    u64::try_from(secs).ok().map(|s| s * 1000)
}

fn offset_minutes(tz: &str) -> Option<i64> {
    let tz = tz.trim();
    match tz {
        "" | "Z" | "z" | "GMT" | "UT" | "UTC" => return Some(0),
        "EST" => return Some(-300),
        "EDT" => return Some(-240),
        "CST" => return Some(-360),
        "CDT" => return Some(-300),
        "MST" => return Some(-420),
        "MDT" => return Some(-360),
        "PST" => return Some(-480),
        "PDT" => return Some(-420),
        _ => {}
    }
    let sign = match tz.chars().next()? {
        '+' => 1,
        '-' => -1,
        _ => return None,
    };
    let digits: String = tz[1..].chars().filter(char::is_ascii_digit).collect();
    if digits.len() != 4 {
        return None;
    }
    let h: i64 = digits[..2].parse().ok()?;
    let m: i64 = digits[2..].parse().ok()?;
    Some(sign * (h * 60 + m))
}

/// RFC 3339 (`2026-10-11T07:00:00Z`) or RFC 2822 (`Sat, 11 Oct 2026 07:00:00 GMT`).
pub fn parse_date_ms(raw: &str) -> Option<u64> {
    let raw = raw.trim();
    if raw.len() >= 19 && raw.as_bytes().get(4) == Some(&b'-') {
        let n = |a: usize, b: usize| raw.get(a..b)?.parse::<i64>().ok();
        let (y, mo, d, h, mi, s) = (
            n(0, 4)?,
            n(5, 7)?,
            n(8, 10)?,
            n(11, 13)?,
            n(14, 16)?,
            n(17, 19)?,
        );
        let mut tz = &raw[19..];
        if let Some(frac) = tz.strip_prefix('.') {
            tz = frac.trim_start_matches(|c: char| c.is_ascii_digit());
        }
        return to_ms(y, mo, d, h, mi, s, offset_minutes(tz)?);
    }
    let rest = raw.split_once(',').map(|(_, r)| r).unwrap_or(raw);
    let parts: Vec<&str> = rest.split_whitespace().collect();
    if parts.len() < 4 {
        return None;
    }
    const MONTHS: [&str; 12] = [
        "jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec",
    ];
    let d: i64 = parts[0].parse().ok()?;
    let mon = parts[1].get(..3)?.to_ascii_lowercase();
    let mo = MONTHS.iter().position(|m| *m == mon)? as i64 + 1;
    let mut y: i64 = parts[2].parse().ok()?;
    if y < 100 {
        y += 2000;
    }
    let hms: Vec<i64> = parts[3]
        .split(':')
        .map(|p| p.parse().unwrap_or(0))
        .collect();
    let tz = parts.get(4).copied().unwrap_or("Z");
    to_ms(
        y,
        mo,
        d,
        *hms.first()?,
        *hms.get(1)?,
        hms.get(2).copied().unwrap_or(0),
        offset_minutes(tz)?,
    )
}

/// Lowercase host without `www.`, no fragment, no `utm_*` or `ref` params, no trailing slash.
pub fn canonical_url(url: &str) -> String {
    let url = url.trim();
    let scheme_less = url.split_once("://").map(|(_, rest)| rest).unwrap_or(url);
    let no_frag = scheme_less.split('#').next().unwrap_or("");
    let (path, query) = match no_frag.split_once('?') {
        Some((p, q)) => (p, q),
        None => (no_frag, ""),
    };
    let (host, rest) = match path.split_once('/') {
        Some((h, r)) => (h, r),
        None => (path, ""),
    };
    let host = host.to_ascii_lowercase();
    let host = host.strip_prefix("www.").unwrap_or(&host);
    let kept: Vec<&str> = query
        .split('&')
        .filter(|p| {
            !p.is_empty()
                && !p.starts_with("utm_")
                && !p.starts_with("ref=")
                && !p.starts_with("ref_src=")
        })
        .collect();
    let mut out = format!("{host}/{}", rest.trim_end_matches('/'));
    out = out.trim_end_matches('/').to_string();
    if !kept.is_empty() {
        out.push('?');
        out.push_str(&kept.join("&"));
    }
    out
}

/// More like this: a search for a post's own words on the next check, scored
/// for the interest the post was for.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Related {
    pub query: String,
    #[serde(default)]
    pub topic: Option<String>,
}

/// Searches queued by More like this, kept until the next check runs them.
pub const RELATED_MAX: usize = 5;
const RELATED_WORDS: usize = 6;

/// A post title's main words, in order, for a related search.
pub fn related_query(title: &str) -> String {
    let mut seen = std::collections::BTreeSet::new();
    title
        .split(|c: char| !c.is_alphanumeric() && c != '.' && c != '-')
        .map(|w| w.trim_matches(|c: char| c == '.' || c == '-'))
        .filter(|w| w.len() > 2 && !TITLE_STOP.contains(&w.to_lowercase().as_str()))
        .filter(|w| seen.insert(w.to_lowercase()))
        .take(RELATED_WORDS)
        .collect::<Vec<_>>()
        .join(" ")
}

/// Hacker News, Reddit and Web search for queued More like this asks. Sources that
/// are off are not searched.
pub fn related_requests(s: &SourceSettings, related: &[Related], now_ms: u64) -> Vec<SourceReq> {
    let mut out = Vec::new();
    for r in related.iter().filter(|r| !r.query.trim().is_empty()) {
        if s.hacker_news {
            let since = now_ms.saturating_sub(SEARCH_WINDOW_MS) / 1000;
            out.push(SourceReq {
                kind: SourceKind::HackerNews,
                url: format!(
                    "https://hn.algolia.com/api/v1/search?query={}&tags=story&numericFilters=created_at_i%3E{since}&hitsPerPage=10",
                    query_value(&r.query)
                ),
                name: "Hacker News".into(),
                topic: r.topic.clone(),
            });
        }
        if s.reddit {
            out.push(SourceReq {
                kind: SourceKind::Reddit,
                url: format!(
                    "https://www.reddit.com/search.json?q={}&sort=top&t=week&limit=10&raw_json=1",
                    query_value(&r.query)
                ),
                name: "Reddit".into(),
                topic: r.topic.clone(),
            });
        }
        if s.web_search {
            out.push(SourceReq {
                kind: SourceKind::WebSearch,
                url: format!(
                    "https://lite.duckduckgo.com/lite/?q={}",
                    query_value(&r.query)
                ),
                name: "Web search".into(),
                topic: r.topic.clone(),
            });
        }
    }
    out
}

/// What the app's fetch returned.
pub enum Fetched {
    Body {
        body: String,
        etag: Option<String>,
        last_modified: Option<String>,
    },
    NotModified,
    Failed(String),
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CacheEntry {
    #[serde(default)]
    pub etag: Option<String>,
    #[serde(default)]
    pub last_modified: Option<String>,
    pub fetched_ms: u64,
    #[serde(default)]
    pub items: Vec<SourceItem>,
}

/// `{config}/pulse-sources.json`: per-URL cache, what already posted, and the last run.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceCache {
    #[serde(default)]
    pub entries: BTreeMap<String, CacheEntry>,
    /// Canonical URLs already posted, newest last.
    #[serde(default)]
    pub seen: Vec<String>,
    #[serde(default)]
    pub last_run_ms: u64,
    /// The UTC day (`ms / DAY_MS`) `summary_calls` counts.
    #[serde(default)]
    pub summary_day: u64,
    #[serde(default)]
    pub summary_calls: u32,
    /// Not interested (−1) and More like this (+1) per source name, −4 to 4.
    #[serde(default)]
    pub source_bias: BTreeMap<String, i32>,
    /// More like this searches for the next check.
    #[serde(default)]
    pub related: Vec<Related>,
    /// How many Feed posts each `source_name` has contributed (for novelty).
    #[serde(default)]
    pub source_posts: BTreeMap<String, u32>,
    /// Places Save / More like this pointed at, with hit counts toward auto-follow.
    #[serde(default)]
    pub places: Vec<PlaceHit>,
    /// Completed source checks; rotates weaker interest topic queries.
    #[serde(default)]
    pub runs: u64,
}

/// A followable place learned from posts the person liked.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaceHit {
    pub kind: SourceKind,
    pub entry: String,
    pub hits: u32,
}

impl SourceCache {
    /// Summary calls made today.
    pub fn calls_today(&self, now_ms: u64) -> u32 {
        if self.summary_day == now_ms / DAY_MS {
            self.summary_calls
        } else {
            0
        }
    }

    /// Not interested lowers a source a step, More like this raises it.
    pub fn nudge_source(&mut self, source_name: &str, step: i32) {
        let name = source_name.trim();
        if name.is_empty() {
            return;
        }
        let bias = self.source_bias.entry(name.to_string()).or_insert(0);
        *bias = (*bias + step).clamp(-SOURCE_BIAS_MAX, SOURCE_BIAS_MAX);
        if *bias == 0 {
            self.source_bias.remove(name);
        }
    }

    /// Queue a related search for the next check. The oldest drops past `RELATED_MAX`.
    pub fn queue_related(&mut self, query: &str, topic: Option<&str>) {
        let query = query.trim();
        if query.is_empty()
            || self
                .related
                .iter()
                .any(|r| r.query.eq_ignore_ascii_case(query))
        {
            return;
        }
        self.related.push(Related {
            query: query.to_string(),
            topic: topic.map(str::to_string),
        });
        if self.related.len() > RELATED_MAX {
            let drop = self.related.len() - RELATED_MAX;
            self.related.drain(..drop);
        }
    }

    /// Count a Save / More like this toward following a place. True the first time
    /// hits reach [`LEARN_PLACE_HITS`] so the app can call [`SourceSettings::add`].
    pub fn note_place(&mut self, kind: SourceKind, entry: &str) -> bool {
        let entry = entry.trim();
        if entry.is_empty() || matches!(kind, SourceKind::HackerNews | SourceKind::WebSearch) {
            return false;
        }
        if let Some(row) = self
            .places
            .iter_mut()
            .find(|p| p.kind == kind && p.entry.eq_ignore_ascii_case(entry))
        {
            if row.hits >= LEARN_PLACE_HITS {
                return false;
            }
            row.hits = row.hits.saturating_add(1);
            return row.hits == LEARN_PLACE_HITS;
        }
        self.places.push(PlaceHit {
            kind,
            entry: entry.to_string(),
            hits: 1,
        });
        if self.places.len() > LEARNED_PLACES_MAX {
            let drop = self.places.len() - LEARNED_PLACES_MAX;
            self.places.drain(..drop);
        }
        LEARN_PLACE_HITS == 1
    }

    /// Remember which sources contributed the posts that just went up.
    pub fn mark_source_posts(&mut self, posted: &[Ranked]) {
        for r in posted {
            let name = r.item.source_name.trim();
            if name.is_empty() {
                continue;
            }
            *self.source_posts.entry(name.to_string()).or_insert(0) += 1;
        }
    }

    /// Spend one summary call. False when today's budget is used up.
    pub fn take_summary_call(&mut self, now_ms: u64) -> bool {
        let used = self.calls_today(now_ms);
        if used >= SUMMARY_CALLS_PER_DAY {
            return false;
        }
        self.summary_day = now_ms / DAY_MS;
        self.summary_calls = used + 1;
        true
    }
}

pub fn load_cache(path: &std::path::Path) -> SourceCache {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

pub fn save_cache(path: &std::path::Path, cache: &SourceCache) -> Result<(), String> {
    let text = serde_json::to_string(cache).map_err(|e| format!("Pulse sources: {e}"))?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("Pulse sources: {e}"))?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, text).map_err(|e| format!("Pulse sources: {e}"))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("Pulse sources: {e}"))
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Gathered {
    pub items: Vec<SourceItem>,
    /// "r/rust: timed out", one per source that failed. Its cached items still count.
    pub errors: Vec<String>,
    /// Requests that went to the network (not served from the cache window).
    pub fetched: usize,
}

/// Run every request through `fetch`, using the cache for anything fetched in the
/// last REFETCH_MS and for 304s. Entries for URLs no longer requested are dropped.
pub fn gather(
    reqs: &[SourceReq],
    cache: &mut SourceCache,
    now_ms: u64,
    fetch: &mut dyn FnMut(&SourceReq, Option<&CacheEntry>) -> Fetched,
) -> Gathered {
    let mut out = Gathered::default();
    for req in reqs {
        let have = cache.entries.get(&req.url).cloned();
        if let Some(entry) = have
            .as_ref()
            .filter(|e| now_ms.saturating_sub(e.fetched_ms) < REFETCH_MS)
        {
            out.items.extend(entry.items.iter().cloned());
            continue;
        }
        out.fetched += 1;
        match fetch(req, have.as_ref()) {
            Fetched::Body {
                body,
                etag,
                last_modified,
            } => {
                let items = parse(req, &body);
                out.items.extend(items.iter().cloned());
                cache.entries.insert(
                    req.url.clone(),
                    CacheEntry {
                        etag,
                        last_modified,
                        fetched_ms: now_ms,
                        items,
                    },
                );
            }
            Fetched::NotModified => {
                let mut entry = have.unwrap_or_default();
                entry.fetched_ms = now_ms;
                out.items.extend(entry.items.iter().cloned());
                cache.entries.insert(req.url.clone(), entry);
            }
            Fetched::Failed(why) => {
                out.errors.push(format!("{}: {why}", req.name));
                if let Some(entry) = have {
                    out.items.extend(entry.items.iter().cloned());
                }
            }
        }
    }
    let wanted: std::collections::BTreeSet<&str> = reqs.iter().map(|r| r.url.as_str()).collect();
    cache.entries.retain(|url, _| wanted.contains(url.as_str()));
    out
}

/// An item with its local score and the interest it is for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ranked {
    pub item: SourceItem,
    /// relevance × freshness × source quality, about 0–130.
    pub score: u32,
    /// The interest it matched, for the "Why:" line.
    pub because: Option<String>,
}

/// Below this an item never posts.
pub const RANK_MIN: u32 = 12;
/// What a followed source's item that names no interest is worth: you chose the source.
pub const FOLLOWED_RELEVANCE: u32 = 35;
/// Titles this alike (shared-word ratio, in percent) are the same story.
pub const SAME_STORY_PCT: u32 = 60;

/// How well an item fits: the strongest interest it names (pinned counts as 100), else
/// half the weight of the interest its search was for, else the followed-source floor.
fn relevance(
    item: &SourceItem,
    profile: &crate::interests::InterestProfile,
) -> (u32, Option<String>) {
    let text = format!("{} {}", item.title, item.summary);
    let weight = |t: &crate::interests::Interest| if t.pinned { 100 } else { u32::from(t.weight) };
    if let Some(t) = profile
        .ranked()
        .into_iter()
        .find(|t| crate::interests::names_topic(&text, &t.topic))
    {
        return (weight(t), Some(t.topic.clone()));
    }
    match item.topic.as_deref() {
        Some(topic) => match profile.find(topic).filter(|t| !t.muted) {
            Some(t) => (weight(t) / 2, Some(t.topic.clone())),
            None => (0, None),
        },
        None => (FOLLOWED_RELEVANCE, None),
    }
}

/// Percent by age: newer is worth more. Unknown dates sit in the middle.
fn freshness(item: &SourceItem, now_ms: u64) -> u32 {
    const HOUR: u64 = 3_600_000;
    if item.published_ms == 0 {
        return 50;
    }
    match now_ms.saturating_sub(item.published_ms) {
        age if age <= 6 * HOUR => 100,
        age if age <= 24 * HOUR => 80,
        age if age <= 72 * HOUR => 60,
        _ => 40,
    }
}

/// Percent from the source's own signal: points on Hacker News, score on Reddit.
fn quality(item: &SourceItem) -> u32 {
    let e = item.engagement;
    match item.kind {
        SourceKind::HackerNews => match e {
            200.. => 130,
            50..=199 => 115,
            10..=49 => 100,
            _ => 80,
        },
        SourceKind::Reddit => match e {
            500.. => 125,
            100..=499 => 110,
            10..=99 => 100,
            _ => 85,
        },
        SourceKind::GithubReleases
        | SourceKind::YouTube
        | SourceKind::Rss
        | SourceKind::WebSearch => 100,
    }
}

/// Score every item. Muted interests, items past ITEM_MAX_AGE_MS and anything under
/// RANK_MIN drop out. Highest first.
pub fn rank(
    items: &[SourceItem],
    profile: &crate::interests::InterestProfile,
    now_ms: u64,
) -> Vec<Ranked> {
    rank_with(items, profile, &BTreeMap::new(), &BTreeMap::new(), now_ms)
}

/// Most steps Not interested or More like this moves one source.
pub const SOURCE_BIAS_MAX: i32 = 4;
/// What one step is worth, in percent of an item's score.
const SOURCE_BIAS_STEP_PCT: i32 = 15;

fn novelty_pct(source_name: &str, source_posts: &BTreeMap<String, u32>) -> u32 {
    if source_posts.is_empty() {
        return 0;
    }
    let posts = source_posts.get(source_name).copied().unwrap_or(0);
    if posts < SOURCE_NOVELTY_BELOW {
        SOURCE_NOVELTY_PCT
    } else {
        0
    }
}

/// `rank`, with each source's score moved 15% a step by Not interested / More like this,
/// and a novelty bump for under-seen sources once any posts have been counted.
pub fn rank_with(
    items: &[SourceItem],
    profile: &crate::interests::InterestProfile,
    bias: &BTreeMap<String, i32>,
    source_posts: &BTreeMap<String, u32>,
    now_ms: u64,
) -> Vec<Ranked> {
    let muted: Vec<&str> = profile
        .topics
        .iter()
        .filter(|t| t.muted)
        .map(|t| t.topic.as_str())
        .collect();
    let mut out: Vec<Ranked> = items
        .iter()
        .filter(|i| i.published_ms == 0 || now_ms.saturating_sub(i.published_ms) <= ITEM_MAX_AGE_MS)
        .filter(|i| {
            let text = format!("{} {}", i.title, i.summary);
            !muted
                .iter()
                .any(|m| crate::interests::names_topic(&text, m))
        })
        .filter_map(|i| {
            let (rel, because) = relevance(i, profile);
            let score = rel * freshness(i, now_ms) * quality(i) / 10_000;
            let step = bias
                .get(&i.source_name)
                .copied()
                .unwrap_or(0)
                .clamp(-SOURCE_BIAS_MAX, SOURCE_BIAS_MAX);
            let pct = u32::try_from(100 + step * SOURCE_BIAS_STEP_PCT).unwrap_or(100);
            let score = score * pct / 100;
            let score = score * (100 + novelty_pct(&i.source_name, source_posts)) / 100;
            (score >= RANK_MIN).then(|| Ranked {
                item: i.clone(),
                score,
                because,
            })
        })
        .collect();
    out.sort_by(|a, b| {
        b.score
            .cmp(&a.score)
            .then(b.item.published_ms.cmp(&a.item.published_ms))
            .then(a.item.url.cmp(&b.item.url))
    });
    out
}

const TITLE_STOP: &[&str] = &[
    "the", "and", "for", "with", "from", "that", "this", "into", "your", "are", "has", "have",
    "its", "now", "new", "how", "why", "what",
];

fn title_words(title: &str) -> std::collections::BTreeSet<String> {
    title
        .split(|c: char| !c.is_alphanumeric())
        .map(str::to_lowercase)
        .filter(|w| w.len() > 2 && !TITLE_STOP.contains(&w.as_str()))
        .collect()
}

/// Shared words over all words, in percent. Two headlines about one story score high.
pub fn title_similarity(a: &str, b: &str) -> u32 {
    let (a, b) = (title_words(a), title_words(b));
    let union = a.union(&b).count();
    if union == 0 {
        return 0;
    }
    let shared = a.intersection(&b).count();
    u32::try_from(shared * 100 / union).unwrap_or(0)
}

/// What posts from a run: best score first, not already posted (by canonical URL),
/// not the same story as a post already up or picked, at most [`POSTS_PER_RUN`],
/// and at most [`PER_SOURCE_MAX`] from one `source_name`.
pub fn pick_new(
    ranked: &[Ranked],
    cache: &SourceCache,
    posted_urls: &[String],
    posted_titles: &[String],
) -> Vec<Ranked> {
    let mut seen: std::collections::BTreeSet<String> = cache.seen.iter().cloned().collect();
    seen.extend(posted_urls.iter().map(|u| canonical_url(u)));
    let mut titles: Vec<String> = posted_titles.to_vec();
    let mut per_source: BTreeMap<String, usize> = BTreeMap::new();
    let mut out = Vec::new();
    for r in ranked {
        if out.len() >= POSTS_PER_RUN {
            break;
        }
        if seen.contains(&canonical_url(&r.item.url)) {
            continue;
        }
        if titles
            .iter()
            .any(|t| title_similarity(t, &r.item.title) >= SAME_STORY_PCT)
        {
            continue;
        }
        let name = r.item.source_name.clone();
        let n = per_source.get(&name).copied().unwrap_or(0);
        if n >= PER_SOURCE_MAX {
            continue;
        }
        seen.insert(canonical_url(&r.item.url));
        titles.push(r.item.title.clone());
        *per_source.entry(name).or_insert(0) += 1;
        out.push(r.clone());
    }
    out
}

/// A followable place implied by a source item (subreddit, GitHub repo, YouTube channel).
pub fn place_from_item(item: &SourceItem) -> Option<(SourceKind, String)> {
    if let Some(sub) = item.source_name.strip_prefix("r/") {
        if let Ok(entry) = canonical_entry(SourceKind::Reddit, sub) {
            return Some((SourceKind::Reddit, entry));
        }
    }
    if let Some(repo) = item.source_name.strip_suffix(" on GitHub") {
        if let Ok(entry) = canonical_entry(SourceKind::GithubReleases, repo) {
            return Some((SourceKind::GithubReleases, entry));
        }
    }
    place_from_url(&item.url)
}

/// A followable place from a Feed card's source line or citation URLs.
pub fn place_from_card(card: &UpdateCard) -> Option<(SourceKind, String)> {
    if let Some(name) = card.pulse.source_name.as_deref() {
        if let Some(sub) = name.strip_prefix("r/") {
            if let Ok(entry) = canonical_entry(SourceKind::Reddit, sub) {
                return Some((SourceKind::Reddit, entry));
            }
        }
        if let Some(repo) = name.strip_suffix(" on GitHub") {
            if let Ok(entry) = canonical_entry(SourceKind::GithubReleases, repo) {
                return Some((SourceKind::GithubReleases, entry));
            }
        }
    }
    for url in &card.citations {
        if let Some(place) = place_from_url(url) {
            return Some(place);
        }
    }
    None
}

/// GitHub repo, Reddit sub, or YouTube channel id from a public URL.
pub fn place_from_url(url: &str) -> Option<(SourceKind, String)> {
    let url = url.trim();
    let host = crate::pulse::source_host(url)?.to_lowercase();
    if host == "github.com" || host.ends_with(".github.com") {
        return canonical_entry(SourceKind::GithubReleases, url)
            .ok()
            .map(|entry| (SourceKind::GithubReleases, entry));
    }
    if host == "reddit.com" || host.ends_with(".reddit.com") {
        return canonical_entry(SourceKind::Reddit, url)
            .ok()
            .map(|entry| (SourceKind::Reddit, entry));
    }
    if host == "youtube.com" || host.ends_with(".youtube.com") || host == "youtu.be" {
        return canonical_entry(SourceKind::YouTube, url)
            .ok()
            .map(|entry| (SourceKind::YouTube, entry));
    }
    None
}

/// Remember what posted so a dismissed post never comes back.
pub fn mark_seen(cache: &mut SourceCache, posted: &[Ranked]) {
    for r in posted {
        let key = canonical_url(&r.item.url);
        if !cache.seen.contains(&key) {
            cache.seen.push(key);
        }
    }
    if cache.seen.len() > SEEN_MAX {
        let drop = cache.seen.len() - SEEN_MAX;
        cache.seen.drain(..drop);
    }
}

const SUMMARY_RULES: &str = "You write the summaries on a news feed. For each numbered item, write two or three short sentences, under 220 characters in all, saying what it is and why it matters. Use only the text given: never add facts, names, numbers or links that are not there. If an item says too little, say what it is in one sentence. Reply with JSON only, like [{\"n\":1,\"summary\":\"...\"}].";

/// One fast-model ask that summarizes a run's picks. None when nothing cleared the
/// threshold: then no model is called.
pub fn summary_messages(picks: &[Ranked]) -> Option<Vec<(String, String)>> {
    if picks.is_empty() {
        return None;
    }
    let mut ask = String::new();
    for (i, r) in picks.iter().enumerate() {
        let text = one_line(&r.item.summary, SUMMARY_CHARS);
        ask.push_str(&format!(
            "{}. {} ({})\n{}\n\n",
            i + 1,
            r.item.title,
            r.item.source_name,
            text
        ));
    }
    Some(vec![
        ("system".into(), SUMMARY_RULES.into()),
        ("user".into(), ask.trim_end().to_string()),
    ])
}

#[derive(Deserialize)]
struct SummaryRow {
    n: usize,
    summary: String,
}

/// Put the model's summaries on the picks they name. Returns how many changed.
/// A reply that is not the JSON asked for changes nothing.
pub fn apply_summaries(picks: &mut [Ranked], reply: &str) -> usize {
    let (Some(start), Some(end)) = (reply.find('['), reply.rfind(']')) else {
        return 0;
    };
    if end <= start {
        return 0;
    }
    let Ok(rows) = serde_json::from_str::<Vec<SummaryRow>>(&reply[start..=end]) else {
        return 0;
    };
    let mut changed = 0;
    for row in rows {
        let text = one_line(&row.summary, MODEL_SUMMARY_CHARS);
        if text.is_empty() || row.n == 0 {
            continue;
        }
        if let Some(r) = picks.get_mut(row.n - 1) {
            r.item.summary = text;
            changed += 1;
        }
    }
    changed
}

/// A Feed post for one item: its own title, the source's name, a working link,
/// the source's image when it has one, and the interest it matched. The summary is the
/// source's own text, or the fast model's when the run was summarized.
pub fn item_card(ranked: &Ranked, now_ms: u64) -> UpdateCard {
    let item = &ranked.item;
    let created = if item.published_ms > 0 && item.published_ms <= now_ms {
        item.published_ms
    } else {
        now_ms
    };
    let source_id = format!("source:{}:{}", item.kind.key(), canonical_url(&item.url));
    let mut card = digest_card(&source_id, &item.title, &item.summary, created);
    card.citations = vec![item.url.clone()];
    card.pulse.source_name = Some(item.source_name.clone());
    card.pulse.image_urls = item.image.iter().cloned().collect();
    card.pulse.because = ranked.because.clone();
    card
}

/// A Feed post made from a source item (it gets Save / Not interested / More like this).
pub fn is_source_post(card: &UpdateCard) -> bool {
    card.source_id.starts_with("source:")
}

/// Saved Finds: posts kept with Save, newest first, in `{config}/saved-finds.json`.
pub const SAVED_FILE: &str = "saved-finds.json";
pub const SAVED_MAX: usize = 200;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedFind {
    pub title: String,
    pub url: String,
    #[serde(default)]
    pub source_name: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub because: Option<String>,
    pub saved_ms: u64,
}

/// A source post as a Saved Find. None for a card with no link.
pub fn saved_from_card(card: &UpdateCard, now_ms: u64) -> Option<SavedFind> {
    let url = card.citations.first()?.clone();
    Some(SavedFind {
        title: card.title.clone(),
        url,
        source_name: card.pulse.source_name.clone().unwrap_or_default(),
        summary: card.body.clone().unwrap_or_default(),
        because: card.pulse.because.clone(),
        saved_ms: now_ms,
    })
}

/// Add a find to the front. False when the same link is already saved.
pub fn save_find(list: &mut Vec<SavedFind>, find: SavedFind) -> bool {
    let key = canonical_url(&find.url);
    if list.iter().any(|f| canonical_url(&f.url) == key) {
        return false;
    }
    list.insert(0, find);
    list.truncate(SAVED_MAX);
    true
}

pub fn load_saved(path: &std::path::Path) -> Vec<SavedFind> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

pub fn save_saved(path: &std::path::Path, list: &[SavedFind]) -> Result<(), String> {
    let text = serde_json::to_string_pretty(list).map_err(|e| format!("Saved finds: {e}"))?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("Saved finds: {e}"))?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, text).map_err(|e| format!("Saved finds: {e}"))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("Saved finds: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: u64 = 1_791_705_600_000; // 2026-10-11T08:00:00Z

    fn req(kind: SourceKind, name: &str, topic: Option<&str>) -> SourceReq {
        SourceReq {
            kind,
            url: format!("https://example.test/{}", kind.key()),
            name: name.into(),
            topic: topic.map(str::to_string),
        }
    }

    #[test]
    fn dates_parse_from_rss_and_atom() {
        assert_eq!(parse_date_ms("Sun, 11 Oct 2026 08:00:00 GMT"), Some(NOW));
        assert_eq!(parse_date_ms("Sun, 11 Oct 2026 03:00:00 -0500"), Some(NOW));
        assert_eq!(parse_date_ms("11 Oct 2026 01:00:00 PDT"), Some(NOW));
        assert_eq!(parse_date_ms("2026-10-11T08:00:00Z"), Some(NOW));
        assert_eq!(parse_date_ms("2026-10-11T10:00:00.123+02:00"), Some(NOW));
        assert_eq!(parse_date_ms("yesterday"), None);
        assert_eq!(parse_date_ms("2026-13-11T08:00:00Z"), None);
    }

    #[test]
    fn rss_items_keep_title_link_date_summary_and_image() {
        let xml = r#"<?xml version="1.0"?><rss version="2.0" xmlns:media="http://search.yahoo.com/mrss/"><channel>
<title>Compressor Weekly</title><link>https://cw.test/</link>
<item><title>Quiet &amp; oil-free: the 2026 lineup</title><link>https://cw.test/2026-lineup?utm_source=rss</link>
<pubDate>Sun, 11 Oct 2026 06:00:00 GMT</pubDate>
<description><![CDATA[<p>Six <b>new</b> units under 60&amp;nbsp;dB.</p>]]></description>
<media:thumbnail url="https://cw.test/img/lineup.jpg"/></item>
<item><title>No link here</title></item>
</channel></rss>"#;
        let items = parse(&req(SourceKind::Rss, "cw.test", None), xml);
        assert_eq!(
            items,
            vec![SourceItem {
                title: "Quiet & oil-free: the 2026 lineup".into(),
                url: "https://cw.test/2026-lineup?utm_source=rss".into(),
                kind: SourceKind::Rss,
                source_name: "Compressor Weekly".into(),
                published_ms: NOW - 2 * 3_600_000,
                summary: "Six new units under 60 dB.".into(),
                image: Some("https://cw.test/img/lineup.jpg".into()),
                topic: None,
                engagement: 0,
            }]
        );
    }

    #[test]
    fn github_and_youtube_atom_feeds_parse() {
        let gh = r#"<?xml version="1.0" encoding="UTF-8"?>
<feed xmlns="http://www.w3.org/2005/Atom" xmlns:media="http://search.yahoo.com/mrss/" xml:lang="en-US">
  <title>Release notes from rust</title>
  <entry><id>tag:github.com,2008:Repository/724712/1.99.0</id><updated>2026-10-09T15:00:00Z</updated>
    <link rel="alternate" type="text/html" href="https://github.com/rust-lang/rust/releases/tag/1.99.0"/>
    <title>Rust 1.99.0</title>
    <content type="html">&lt;h2&gt;Language&lt;/h2&gt;&lt;p&gt;Async closures are stable.&lt;/p&gt;</content>
    <author><name>rust-lang</name></author>
    <media:thumbnail height="30" width="30" url="https://avatars.githubusercontent.com/u/5430905?s=60&amp;v=4"/>
  </entry>
</feed>"#;
        let mut r = req(SourceKind::GithubReleases, "rust-lang/rust on GitHub", None);
        r.url = "https://github.com/rust-lang/rust/releases.atom".into();
        let items = parse(&r, gh);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].title, "Rust 1.99.0");
        assert_eq!(
            items[0].url,
            "https://github.com/rust-lang/rust/releases/tag/1.99.0"
        );
        assert_eq!(items[0].source_name, "rust-lang/rust on GitHub");
        assert_eq!(items[0].summary, "Language Async closures are stable.");
        assert_eq!(
            items[0].published_ms,
            parse_date_ms("2026-10-09T15:00:00Z").unwrap()
        );
        assert_eq!(
            items[0].image.as_deref(),
            Some("https://avatars.githubusercontent.com/u/5430905?s=60&v=4")
        );

        let yt = r#"<feed xmlns:yt="http://www.youtube.com/xml/schemas/2015" xmlns:media="http://search.yahoo.com/mrss/" xmlns="http://www.w3.org/2005/Atom">
 <title>Shop Tools</title>
 <entry><title>Rebuilding a 60 gallon compressor</title>
  <link rel="alternate" href="https://www.youtube.com/watch?v=abc123"/>
  <published>2026-10-10T12:00:00+00:00</published>
  <media:group><media:title>Rebuilding</media:title>
   <media:thumbnail url="https://i1.ytimg.com/vi/abc123/hqdefault.jpg" width="480" height="360"/>
   <media:description>Full teardown and rebuild.</media:description></media:group>
 </entry></feed>"#;
        let items = parse(&req(SourceKind::YouTube, "YouTube", None), yt);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].source_name, "Shop Tools");
        assert_eq!(items[0].url, "https://www.youtube.com/watch?v=abc123");
        assert_eq!(items[0].summary, "Full teardown and rebuild.");
        assert_eq!(
            items[0].image.as_deref(),
            Some("https://i1.ytimg.com/vi/abc123/hqdefault.jpg")
        );
    }

    #[test]
    fn hacker_news_and_reddit_json_parse() {
        let hn = r#"{"hits":[
 {"title":"Rust 1.99 released","url":"https://blog.rust-lang.org/2026/10/09/Rust-1.99.html","objectID":"41","points":812,"num_comments":301,"created_at_i":1791705000},
 {"title":"Ask HN: Rust at work?","url":null,"objectID":"42","points":5,"num_comments":2,"created_at_i":1791704000}]}"#;
        let items = parse(
            &req(SourceKind::HackerNews, "Hacker News", Some("Rust")),
            hn,
        );
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].summary, "812 points, 301 comments on Hacker News.");
        assert_eq!(items[0].topic.as_deref(), Some("Rust"));
        assert_eq!(items[0].published_ms, 1_791_705_000_000);
        assert_eq!(items[1].url, "https://news.ycombinator.com/item?id=42");

        let rd = r#"{"data":{"children":[
 {"data":{"title":"Pinned rules","stickied":true,"permalink":"/r/rust/comments/1/","subreddit":"rust","created_utc":1791700000.0}},
 {"data":{"title":"NSFW","over_18":true,"permalink":"/r/rust/comments/2/","subreddit":"rust","created_utc":1791700000.0}},
 {"data":{"title":"My compressor build","is_self":true,"url":"https://www.reddit.com/r/rust/comments/3/x/","permalink":"/r/rust/comments/3/x/","selftext":"Took  a weekend.","subreddit":"rust","created_utc":1791700000.0,"thumbnail":"self"}},
 {"data":{"title":"egui 0.40 is out","is_self":false,"url_overridden_by_dest":"https://github.com/emilk/egui/releases","permalink":"/r/rust/comments/4/e/","subreddit":"rust","created_utc":1791701000.0,
   "preview":{"images":[{"source":{"url":"https://preview.redd.it/egui.png?width=640&s=abc"}}]}}}]}}"#;
        let items = parse(&req(SourceKind::Reddit, "r/rust", None), rd);
        let got: Vec<(&str, &str, &str, Option<&str>)> = items
            .iter()
            .map(|i| {
                (
                    i.title.as_str(),
                    i.url.as_str(),
                    i.source_name.as_str(),
                    i.image.as_deref(),
                )
            })
            .collect();
        assert_eq!(
            got,
            vec![
                (
                    "My compressor build",
                    "https://www.reddit.com/r/rust/comments/3/x/",
                    "r/rust",
                    None
                ),
                (
                    "egui 0.40 is out",
                    "https://github.com/emilk/egui/releases",
                    "r/rust",
                    Some("https://preview.redd.it/egui.png?width=640&s=abc")
                ),
            ]
        );
        assert_eq!(items[0].summary, "Took a weekend.");
        assert!(parse(
            &req(SourceKind::Reddit, "r/rust", None),
            "<html>blocked</html>"
        )
        .is_empty());
    }

    #[test]
    fn requests_follow_settings_and_the_top_topics() {
        let mut s = SourceSettings::default();
        assert_eq!(
            s.add(SourceKind::Reddit, "https://www.reddit.com/r/rust/"),
            Ok("rust".into())
        );
        assert_eq!(
            s.add(
                SourceKind::GithubReleases,
                "https://github.com/emilk/egui.git"
            ),
            Ok("emilk/egui".into())
        );
        assert_eq!(
            s.add(
                SourceKind::YouTube,
                "https://www.youtube.com/channel/UCxxxxxxxxxxxxxxxxxxxxxx"
            ),
            Ok("UCxxxxxxxxxxxxxxxxxxxxxx".into())
        );
        assert_eq!(
            s.add(SourceKind::Rss, "https://this-week-in-rust.org/rss.xml"),
            Ok("https://this-week-in-rust.org/rss.xml".into())
        );
        assert_eq!(
            s.add(SourceKind::Reddit, "r/Rust"),
            Err("rust is already on the list.".into())
        );
        assert!(s.add(SourceKind::GithubReleases, "just-a-name").is_err());
        assert!(s.add(SourceKind::Rss, "ftp://x.test/feed").is_err());
        assert!(s.add(SourceKind::HackerNews, "anything").is_err());
        assert!(s.add(SourceKind::WebSearch, "anything").is_err());
        let topics: Vec<String> = ["air compressors", "Rust", "xAI", "F1"]
            .iter()
            .map(|t| t.to_string())
            .collect();
        let urls: Vec<String> = requests(&s, &topics, NOW)
            .into_iter()
            .map(|r| r.url)
            .collect();
        let since = (NOW - SEARCH_WINDOW_MS) / 1000;
        assert_eq!(
            urls,
            vec![
                format!("https://hn.algolia.com/api/v1/search?query=air%20compressors&tags=story&numericFilters=created_at_i%3E{since}&hitsPerPage=10"),
                format!("https://hn.algolia.com/api/v1/search?query=Rust&tags=story&numericFilters=created_at_i%3E{since}&hitsPerPage=10"),
                format!("https://hn.algolia.com/api/v1/search?query=xAI&tags=story&numericFilters=created_at_i%3E{since}&hitsPerPage=10"),
                "https://www.reddit.com/r/rust/hot.json?limit=15&raw_json=1".to_string(),
                "https://www.reddit.com/search.json?q=air%20compressors&sort=top&t=week&limit=10&raw_json=1".to_string(),
                "https://www.reddit.com/search.json?q=Rust&sort=top&t=week&limit=10&raw_json=1".to_string(),
                "https://www.reddit.com/search.json?q=xAI&sort=top&t=week&limit=10&raw_json=1".to_string(),
                "https://lite.duckduckgo.com/lite/?q=air%20compressors".to_string(),
                "https://lite.duckduckgo.com/lite/?q=Rust".to_string(),
                "https://lite.duckduckgo.com/lite/?q=xAI".to_string(),
                "https://github.com/emilk/egui/releases.atom".to_string(),
                "https://www.youtube.com/feeds/videos.xml?channel_id=UCxxxxxxxxxxxxxxxxxxxxxx".to_string(),
                "https://this-week-in-rust.org/rss.xml".to_string(),
            ]
        );
        s.set_on(SourceKind::HackerNews, false);
        s.set_on(SourceKind::Reddit, false);
        s.set_on(SourceKind::WebSearch, false);
        s.set_on(SourceKind::Rss, false);
        let kinds: Vec<SourceKind> = requests(&s, &topics, NOW)
            .into_iter()
            .map(|r| r.kind)
            .collect();
        assert_eq!(kinds, vec![SourceKind::GithubReleases, SourceKind::YouTube]);
    }

    #[test]
    fn web_search_requests_follow_topics_like_hn() {
        let s = SourceSettings::default();
        let topics: Vec<String> = ["Rust", "xAI", "F1", "extra"]
            .iter()
            .map(|t| (*t).to_string())
            .collect();
        let web: Vec<String> = requests(&s, &topics, NOW)
            .into_iter()
            .filter(|r| r.kind == SourceKind::WebSearch)
            .map(|r| r.url)
            .collect();
        assert_eq!(
            web,
            vec![
                "https://lite.duckduckgo.com/lite/?q=Rust".to_string(),
                "https://lite.duckduckgo.com/lite/?q=xAI".to_string(),
                "https://lite.duckduckgo.com/lite/?q=F1".to_string(),
            ]
        );
        let mut off = s;
        off.set_on(SourceKind::WebSearch, false);
        assert!(!requests(&off, &topics, NOW)
            .iter()
            .any(|r| r.url.contains("lite.duckduckgo.com")));
        let empty: SourceSettings = serde_json::from_str("{}").unwrap();
        assert!(empty.web_search);
    }

    #[test]
    fn web_search_html_parse_unwraps_uddg_and_names_the_host() {
        let body = r#"<html><body>
<a rel="nofollow" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fblog.rust-lang.org%2F1.99">Rust 1.99 released</a>
<a rel="nofollow" href="https://example.com/compressors">Quiet compressors</a>
<a href="/lite/">Lite</a>
<a rel="nofollow" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fduckduckgo.com%2Fabout">About DDG</a>
</body></html>"#;
        let items = parse(
            &req(SourceKind::WebSearch, "Web search", Some("Rust")),
            body,
        );
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].title, "Rust 1.99 released");
        assert_eq!(items[0].url, "https://blog.rust-lang.org/1.99");
        assert_eq!(items[0].source_name, "blog.rust-lang.org");
        assert_eq!(items[0].kind, SourceKind::WebSearch);
        assert_eq!(items[0].topic.as_deref(), Some("Rust"));
        assert_eq!(items[0].published_ms, 0);
        assert_eq!(items[0].engagement, 0);
        assert_eq!(items[1].url, "https://example.com/compressors");
        assert_eq!(items[1].source_name, "example.com");
        assert!(parse(
            &req(SourceKind::WebSearch, "Web search", Some("Rust")),
            "<html>captcha</html>"
        )
        .is_empty());
    }

    #[test]
    fn gather_uses_etags_the_refetch_window_and_stale_items_on_failure() {
        let hn = r#"{"hits":[{"title":"Rust 1.99","url":"https://blog.rust-lang.org/1.99","objectID":"1","points":1,"num_comments":0,"created_at_i":1791705000}]}"#;
        let a = SourceReq {
            kind: SourceKind::HackerNews,
            url: "https://hn.test/a".into(),
            name: "Hacker News".into(),
            topic: Some("Rust".into()),
        };
        let b = SourceReq {
            kind: SourceKind::Rss,
            url: "https://feed.test/b".into(),
            name: "feed.test".into(),
            topic: None,
        };
        let mut cache = SourceCache::default();
        cache
            .entries
            .insert("https://gone.test/".into(), CacheEntry::default());
        let mut calls: Vec<(String, Option<String>)> = Vec::new();
        let got = gather(&[a.clone(), b.clone()], &mut cache, NOW, &mut |r, have| {
            calls.push((r.url.clone(), have.and_then(|h| h.etag.clone())));
            if r.url == a.url {
                Fetched::Body {
                    body: hn.into(),
                    etag: Some("\"v1\"".into()),
                    last_modified: None,
                }
            } else {
                Fetched::Failed("timed out".into())
            }
        });
        assert_eq!(got.items.len(), 1);
        assert_eq!(got.errors, vec!["feed.test: timed out".to_string()]);
        assert_eq!(got.fetched, 2);
        assert!(
            !cache.entries.contains_key("https://gone.test/"),
            "unrequested URLs leave the cache"
        );
        // Inside the window nothing is fetched.
        calls.clear();
        let again = gather(
            std::slice::from_ref(&a),
            &mut cache,
            NOW + 60_000,
            &mut |r, _| {
                calls.push((r.url.clone(), None));
                Fetched::Failed("should not fetch".into())
            },
        );
        assert!(calls.is_empty());
        assert_eq!(again.items.len(), 1);
        // After it, the ETag goes along and a 304 keeps the items.
        let later = gather(
            std::slice::from_ref(&a),
            &mut cache,
            NOW + REFETCH_MS,
            &mut |r, have| {
                calls.push((r.url.clone(), have.and_then(|h| h.etag.clone())));
                Fetched::NotModified
            },
        );
        assert_eq!(calls, vec![(a.url.clone(), Some("\"v1\"".to_string()))]);
        assert_eq!(later.items.len(), 1);
        assert_eq!(cache.entries[&a.url].fetched_ms, NOW + REFETCH_MS);
        // A failure after that still offers the cached items.
        let failed = gather(
            std::slice::from_ref(&a),
            &mut cache,
            NOW + 2 * REFETCH_MS,
            &mut |_, _| Fetched::Failed("HTTP 503".into()),
        );
        assert_eq!(failed.items.len(), 1);
        assert_eq!(failed.errors, vec!["Hacker News: HTTP 503".to_string()]);
    }

    fn item(title: &str, url: &str, published_ms: u64) -> SourceItem {
        SourceItem {
            title: title.into(),
            url: url.into(),
            kind: SourceKind::Rss,
            source_name: "feed.test".into(),
            published_ms,
            summary: String::new(),
            image: None,
            topic: None,
            engagement: 0,
        }
    }

    fn ranked(r: SourceItem) -> Ranked {
        Ranked {
            item: r,
            score: 50,
            because: None,
        }
    }

    fn profile(topics: &[(&str, u8, bool, bool)]) -> crate::interests::InterestProfile {
        let mut p = crate::interests::InterestProfile::default();
        for (topic, weight, pinned, muted) in topics {
            p.topics.push(crate::interests::Interest {
                topic: topic.to_string(),
                weight: *weight,
                source: crate::interests::InterestSource::Explicit,
                pinned: *pinned,
                muted: *muted,
                last_seen_ms: 0,
            });
        }
        p
    }

    #[test]
    fn rank_scores_relevance_freshness_and_source_signal() {
        const H: u64 = 3_600_000;
        let p = profile(&[
            ("Rust", 60, false, false),
            ("air compressors", 40, false, false),
            ("F1", 20, true, false),
            ("crypto", 90, false, true),
        ]);
        let mut hn = item(
            "Rust 1.99 released",
            "https://blog.rust-lang.org/1.99",
            NOW - H,
        );
        hn.kind = SourceKind::HackerNews;
        hn.engagement = 812;
        hn.topic = Some("Rust".into());
        let mut quiet_hn = item("Show HN: my editor", "https://e.test", NOW - H);
        quiet_hn.kind = SourceKind::HackerNews;
        quiet_hn.engagement = 3;
        quiet_hn.topic = Some("Rust".into());
        let compressor = item(
            "Rebuilding air compressors at home",
            "https://c.test",
            NOW - 20 * H,
        );
        let f1 = item("F1 calendar for 2027", "https://f1.test", NOW - 80 * H);
        let followed = item("This week in a newsletter", "https://n.test", 0);
        let crypto = item("Rust crypto wallet hacked", "https://x.test", NOW);
        let old = item(
            "Rust 1.0 retrospective",
            "https://o.test",
            NOW - ITEM_MAX_AGE_MS - 1,
        );
        let got: Vec<(String, u32, Option<String>)> = rank(
            &[hn, quiet_hn, compressor, f1, followed, crypto, old],
            &p,
            NOW,
        )
        .into_iter()
        .map(|r| (r.item.title, r.score, r.because))
        .collect();
        assert_eq!(
            got,
            vec![
                // 60 × 100% × 130% (812 points).
                (
                    "Rust 1.99 released".to_string(),
                    78,
                    Some("Rust".to_string())
                ),
                // Pinned counts as 100, at 72+ hours: 100 × 40%.
                (
                    "F1 calendar for 2027".to_string(),
                    40,
                    Some("F1".to_string())
                ),
                // 40 × 80% (20 hours old).
                (
                    "Rebuilding air compressors at home".to_string(),
                    32,
                    Some("air compressors".to_string())
                ),
                // Its search was for Rust but it never says so: 60/2 × 100% × 80% (3 points).
                (
                    "Show HN: my editor".to_string(),
                    24,
                    Some("Rust".to_string())
                ),
                // A followed source, no date: 35 × 50%.
                ("This week in a newsletter".to_string(), 17, None),
            ]
        );
    }

    #[test]
    fn same_story_titles_match_and_different_ones_do_not() {
        assert!(
            title_similarity(
                "Rust 1.99 released with async closures",
                "Rust 1.99 is released: async closures land"
            ) >= SAME_STORY_PCT
        );
        assert!(title_similarity("Rust 1.99 released", "Go 1.30 released") < SAME_STORY_PCT);
        assert_eq!(title_similarity("", "anything"), 0);
    }

    #[test]
    fn pick_new_skips_seen_links_and_stories_already_up() {
        let mut cache = SourceCache::default();
        mark_seen(
            &mut cache,
            &[ranked(item("Seen", "https://a.test/seen", NOW))],
        );
        let list: Vec<Ranked> = [
            item("Seen again", "https://www.a.test/seen/?utm_source=x", NOW),
            item(
                "Rust 1.99 released with async closures",
                "https://a.test/n",
                NOW - 1,
            ),
            item(
                "Rust 1.99 is released: async closures land",
                "https://b.test/n",
                NOW - 1,
            ),
            item("Same link twice", "https://A.test/n#top", NOW - 2),
            item("On a card", "https://a.test/card", NOW - 3),
            item(
                "Egui 0.40 ships a new layout engine",
                "https://c.test/e",
                NOW - 4,
            ),
            item("Older", "https://a.test/o", NOW - 10),
        ]
        .into_iter()
        .map(ranked)
        .collect();
        let posted = vec!["https://a.test/card".to_string()];
        let up = vec!["egui 0.40 ships new layout engine".to_string()];
        let got: Vec<String> = pick_new(&list, &cache, &posted, &up)
            .into_iter()
            .map(|r| r.item.title)
            .collect();
        assert_eq!(got, vec!["Rust 1.99 released with async closures", "Older"]);
        assert_eq!(
            canonical_url("https://WWW.Example.com/a/?utm_medium=x&id=3#frag"),
            "example.com/a?id=3"
        );
    }

    #[test]
    fn an_item_card_names_its_item_source_and_link() {
        let mut it = item(
            "Rust 1.99.0",
            "https://github.com/rust-lang/rust/releases/tag/1.99.0",
            NOW - 1000,
        );
        it.source_name = "rust-lang/rust on GitHub".into();
        it.summary = "Async closures are stable.".into();
        it.image = Some("https://img.test/r.png".into());
        let card = item_card(
            &Ranked {
                item: it,
                score: 40,
                because: Some("Rust".into()),
            },
            NOW,
        );
        assert_eq!(card.kind, crate::update_feed::UpdateKind::Digest);
        assert_eq!(card.title, "Rust 1.99.0");
        assert_eq!(card.body.as_deref(), Some("Async closures are stable."));
        assert_eq!(
            card.citations,
            vec!["https://github.com/rust-lang/rust/releases/tag/1.99.0".to_string()]
        );
        assert_eq!(
            card.pulse.source_name.as_deref(),
            Some("rust-lang/rust on GitHub")
        );
        assert_eq!(
            card.pulse.image_urls,
            vec!["https://img.test/r.png".to_string()]
        );
        assert_eq!(card.created_at, NOW - 1000);
        assert_eq!(
            card.source_id,
            "source:rss:github.com/rust-lang/rust/releases/tag/1.99.0"
        );
        assert_eq!(card.pulse.because.as_deref(), Some("Rust"));
        assert!(crate::update_feed::names_its_item(&card));
        // A future date from a bad feed clock posts as now.
        assert_eq!(
            item_card(&ranked(item("Later", "https://a.test/l", NOW + 5)), NOW).created_at,
            NOW
        );
    }

    #[test]
    fn the_clock_runs_every_interval_and_once_on_open() {
        let s = SourceSettings::default();
        assert_eq!(s.every_ms(), 90 * 60_000);
        let mut far = s.clone();
        far.every_mins = 999;
        assert_eq!(far.every_ms(), 240 * 60_000);
        far.every_mins = 5;
        assert_eq!(far.every_ms(), 60 * 60_000);
        let every = s.every_ms();
        assert!(check_due(0, NOW, every, false));
        assert!(!check_due(NOW - 89 * 60_000, NOW, every, false));
        assert!(check_due(NOW - 90 * 60_000, NOW, every, false));
        // Opening the app runs a check unless one ran in the last ten minutes.
        assert!(check_due(NOW - 11 * 60_000, NOW, every, true));
        assert!(!check_due(NOW - 9 * 60_000, NOW, every, true));
        let old: SourceSettings = serde_json::from_str("{}").unwrap();
        assert_eq!(old.every_mins, 90);
    }

    #[test]
    fn the_daily_summary_budget_resets_each_day() {
        let mut cache = SourceCache::default();
        for _ in 0..SUMMARY_CALLS_PER_DAY {
            assert!(cache.take_summary_call(NOW));
        }
        assert_eq!(cache.calls_today(NOW), 16);
        assert!(!cache.take_summary_call(NOW));
        assert_eq!(cache.calls_today(NOW), 16);
        let tomorrow = NOW + DAY_MS;
        assert_eq!(cache.calls_today(tomorrow), 0);
        assert!(cache.take_summary_call(tomorrow));
        assert_eq!(cache.calls_today(tomorrow), 1);
    }

    #[test]
    fn only_the_picks_go_to_the_model_and_nothing_new_asks_nothing() {
        assert_eq!(summary_messages(&[]), None);
        let mut a = ranked(item("Rust 1.99 released", "https://a.test/1", NOW));
        a.item.summary = "812 points on Hacker News.".into();
        let mut b = ranked(item("Quiet compressors", "https://b.test/2", NOW));
        b.item.source_name = "Shop Notes".into();
        b.item.summary = "Six   new units\nunder 60 dB.".into();
        let msgs = summary_messages(&[a.clone(), b.clone()]).expect("asks");
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].0, "system");
        assert!(
            msgs[0].1.contains("Use only the text given"),
            "{}",
            msgs[0].1
        );
        assert_eq!(
            msgs[1].1,
            "1. Rust 1.99 released (feed.test)\n812 points on Hacker News.\n\n2. Quiet compressors (Shop Notes)\nSix new units under 60 dB."
        );
        let mut picks = vec![a, b];
        let reply = "```json\n[{\"n\":2,\"summary\":\"  Six quiet units.  Tested at home. \"},{\"n\":7,\"summary\":\"no such item\"},{\"n\":1,\"summary\":\"\"}]\n```";
        assert_eq!(apply_summaries(&mut picks, reply), 1);
        assert_eq!(picks[1].item.summary, "Six quiet units. Tested at home.");
        assert_eq!(picks[0].item.summary, "812 points on Hacker News.");
        assert_eq!(apply_summaries(&mut picks, "Sorry, I can't."), 0);
    }

    #[test]
    fn more_like_this_queues_a_search_for_the_posts_own_words() {
        assert_eq!(
            related_query("Rust 1.99 ships async closures for the web"),
            "Rust 1.99 ships async closures web"
        );
        assert_eq!(related_query("The new: the NEW thing"), "thing");
        let mut cache = SourceCache::default();
        cache.queue_related("Rust async closures", Some("Rust"));
        cache.queue_related("rust ASYNC closures", Some("Rust"));
        assert_eq!(cache.related.len(), 1);
        for n in 0..6 {
            cache.queue_related(&format!("query {n}"), None);
        }
        assert_eq!(cache.related.len(), RELATED_MAX);
        assert_eq!(cache.related[0].query, "query 1");
        let one = [Related {
            query: "Rust async closures".into(),
            topic: Some("Rust".into()),
        }];
        let reqs = related_requests(&SourceSettings::default(), &one, NOW);
        let got: Vec<(SourceKind, Option<String>, bool)> = reqs
            .iter()
            .map(|r| {
                (
                    r.kind,
                    r.topic.clone(),
                    r.url.contains("Rust%20async%20closures"),
                )
            })
            .collect();
        assert_eq!(
            got,
            vec![
                (SourceKind::HackerNews, Some("Rust".into()), true),
                (SourceKind::Reddit, Some("Rust".into()), true),
                (SourceKind::WebSearch, Some("Rust".into()), true),
            ]
        );
        assert!(reqs
            .iter()
            .any(|r| { r.kind == SourceKind::WebSearch && r.url.contains("lite.duckduckgo.com") }));
        let no_search = SourceSettings {
            hacker_news: false,
            reddit: false,
            web_search: false,
            ..Default::default()
        };
        assert!(related_requests(&no_search, &one, NOW).is_empty());
    }

    #[test]
    fn source_bias_moves_a_sources_score_in_steps() {
        let mut cache = SourceCache::default();
        cache.nudge_source("feed.test", -1);
        cache.nudge_source("feed.test", -1);
        assert_eq!(cache.source_bias.get("feed.test"), Some(&-2));
        for _ in 0..9 {
            cache.nudge_source("feed.test", 1);
        }
        assert_eq!(cache.source_bias.get("feed.test"), Some(&SOURCE_BIAS_MAX));
        for _ in 0..4 {
            cache.nudge_source("feed.test", -1);
        }
        assert_eq!(cache.source_bias.get("feed.test"), None);
        cache.nudge_source("  ", 1);
        assert!(cache.source_bias.is_empty());

        // A followed source's fresh item: 35 × 100 × 100 / 10 000 = 35.
        let followed = item("Shop notes", "https://feed.test/1", NOW - 60_000);
        let p = profile(&[]);
        let score = |bias: i32| {
            let map: BTreeMap<String, i32> =
                [("feed.test".to_string(), bias)].into_iter().collect();
            rank_with(
                std::slice::from_ref(&followed),
                &p,
                &map,
                &BTreeMap::new(),
                NOW,
            )
            .first()
            .map(|r| r.score)
        };
        assert_eq!(score(0), Some(35));
        assert_eq!(score(1), Some(40));
        assert_eq!(score(-2), Some(24));
        assert_eq!(score(-4), Some(14));
    }

    #[test]
    fn topic_queries_keep_the_strongest_and_rotate_the_rest() {
        let topics: Vec<String> = ["Rust", "xAI", "F1", "compressors"]
            .iter()
            .map(|t| (*t).into())
            .collect();
        assert_eq!(
            topic_queries(&topics, 0),
            vec!["Rust".to_string(), "xAI".into(), "F1".into()]
        );
        assert_eq!(
            topic_queries(&topics, 1),
            vec!["Rust".to_string(), "F1".into(), "compressors".into()]
        );
        assert_eq!(
            topic_queries(&topics, 2),
            vec!["Rust".to_string(), "compressors".into(), "xAI".into()]
        );
        assert_eq!(
            topic_queries(&topics[..2], 9),
            vec!["Rust".to_string(), "xAI".into()]
        );
    }

    #[test]
    fn pick_new_caps_posts_per_source_name() {
        let cache = SourceCache::default();
        let mut list = Vec::new();
        for (i, title) in [
            "Alpha ships",
            "Bravo lands",
            "Charlie opens",
            "Delta closes",
        ]
        .iter()
        .enumerate()
        {
            let mut it = item(title, &format!("https://a.test/{i}"), NOW - i as u64);
            it.source_name = "Hacker News".into();
            list.push(ranked(it));
        }
        for (i, title) in ["Kernel notes", "Editor release", "Database tip"]
            .iter()
            .enumerate()
        {
            let mut it = item(title, &format!("https://b.test/{i}"), NOW - 10 - i as u64);
            it.source_name = "r/rust".into();
            list.push(ranked(it));
        }
        let mut feed = item("Quiet compressors", "https://c.test/a", NOW - 20);
        feed.source_name = "blog.test".into();
        list.push(ranked(feed));
        let mut feed_b = item("Shop lighting", "https://c.test/b", NOW - 21);
        feed_b.source_name = "blog.test".into();
        list.push(ranked(feed_b));
        let got: Vec<(String, String)> = pick_new(&list, &cache, &[], &[])
            .into_iter()
            .map(|r| (r.item.source_name, r.item.title))
            .collect();
        assert_eq!(
            got,
            vec![
                ("Hacker News".into(), "Alpha ships".into()),
                ("Hacker News".into(), "Bravo lands".into()),
                ("r/rust".into(), "Kernel notes".into()),
                ("r/rust".into(), "Editor release".into()),
                ("blog.test".into(), "Quiet compressors".into()),
            ]
        );
    }

    #[test]
    fn place_learning_follows_reddit_and_github_after_two_hits() {
        assert_eq!(
            place_from_item(&SourceItem {
                title: "Async in the kernel".into(),
                url: "https://www.reddit.com/r/rust/comments/abc/x".into(),
                kind: SourceKind::Reddit,
                source_name: "r/rust".into(),
                published_ms: NOW,
                summary: String::new(),
                image: None,
                topic: None,
                engagement: 10,
            }),
            Some((SourceKind::Reddit, "rust".into()))
        );
        assert_eq!(
            place_from_url("https://github.com/emilk/egui/releases/tag/0.40"),
            Some((SourceKind::GithubReleases, "emilk/egui".into()))
        );
        assert_eq!(
            place_from_url("https://blog.rust-lang.org/1.99"),
            None,
            "plain blogs are not auto-followed without a real feed id"
        );
        let mut cache = SourceCache::default();
        assert!(!cache.note_place(SourceKind::Reddit, "rust"));
        assert!(cache.note_place(SourceKind::Reddit, "Rust"));
        assert!(!cache.note_place(SourceKind::Reddit, "rust"));
        assert_eq!(
            cache.places,
            vec![PlaceHit {
                kind: SourceKind::Reddit,
                entry: "rust".into(),
                hits: LEARN_PLACE_HITS,
            }]
        );
    }

    #[test]
    fn under_seen_sources_get_a_novelty_bump_once_history_exists() {
        let followed = item("Shop notes", "https://feed.test/1", NOW - 60_000);
        let p = profile(&[]);
        let bias = BTreeMap::<String, i32>::new();
        let empty_posts = BTreeMap::<String, u32>::new();
        let history: BTreeMap<String, u32> = [("other".into(), 5)].into_iter().collect();
        let plain = rank_with(
            std::slice::from_ref(&followed),
            &p,
            &bias,
            &empty_posts,
            NOW,
        )[0]
        .score;
        let novel = rank_with(std::slice::from_ref(&followed), &p, &bias, &history, NOW)[0].score;
        assert_eq!(plain, 35);
        assert_eq!(novel, 42);
        let known: BTreeMap<String, u32> = [("feed.test".into(), SOURCE_NOVELTY_BELOW)]
            .into_iter()
            .collect();
        assert_eq!(
            rank_with(std::slice::from_ref(&followed), &p, &bias, &known, NOW)[0].score,
            35
        );
    }

    #[test]
    fn saved_finds_keep_one_entry_per_link_newest_first() {
        let mut card = item_card(
            &ranked(item(
                "Rust 1.99 released",
                "https://a.test/1?utm_source=x",
                NOW,
            )),
            NOW,
        );
        card.pulse.because = Some("Rust".into());
        let first = saved_from_card(&card, NOW).expect("has a link");
        assert_eq!(first.url, "https://a.test/1?utm_source=x");
        assert_eq!(first.because.as_deref(), Some("Rust"));
        let mut list = Vec::new();
        assert!(save_find(&mut list, first));
        let mut again = saved_from_card(&card, NOW + 1).expect("has a link");
        again.url = "https://www.a.test/1".into();
        assert!(!save_find(&mut list, again));
        let other = saved_from_card(
            &item_card(&ranked(item("Other", "https://b.test/2", NOW)), NOW),
            NOW + 2,
        )
        .expect("link");
        assert!(save_find(&mut list, other));
        let titles: Vec<&str> = list.iter().map(|f| f.title.as_str()).collect();
        assert_eq!(titles, vec!["Other", "Rust 1.99 released"]);
        card.citations.clear();
        assert_eq!(saved_from_card(&card, NOW), None);
        assert!(is_source_post(&item_card(
            &ranked(item("x", "https://c.test/3", NOW)),
            NOW
        )));
    }
}
