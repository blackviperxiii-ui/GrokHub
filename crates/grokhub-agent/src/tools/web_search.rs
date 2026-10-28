//! Cabin-executed `web_search` for OpenRouter and the local model.
//!
//! Default backend is DuckDuckGo Lite (free, no key) — same index Pulse uses.
//! Optional SearXNG (self-hosted URL) and Brave Search (bring-your-own key in
//! the OS keyring). xAI keeps hosted `web_search` / `x_search`; this function
//! tool is stripped from the xAI Responses body when hosted search is on.
//! Offline or network failure returns a clear unavailable line instead of
//! failing the turn.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use super::web_fetch::{public_only_resolve, PageFetch};
use super::ToolOutput;

const MAX_RESULTS: usize = 8;
const CACHE_TTL: Duration = Duration::from_secs(90);
const MIN_GAP: Duration = Duration::from_secs(1);
const QUERY_CAP: usize = 400;
const OFFLINE_MSG: &str =
    "Web search is unavailable (no network or the search host did not answer). Continue without live sources, or try again when online.";

/// Keyring account for an optional Brave Search API key (never in app.json).
pub const BRAVE_KEY_ACCOUNT: &str = "cabin-web-search-brave";
/// Keyring service — same vault as provider keys.
pub const KEY_SERVICE: &str = "GrokHub provider keys";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    DuckDuckGo,
    Searxng,
    Brave,
}

impl Backend {
    pub fn parse(raw: &str) -> Self {
        match raw.trim().to_ascii_lowercase().as_str() {
            "searxng" | "searx" => Self::Searxng,
            "brave" => Self::Brave,
            _ => Self::DuckDuckGo,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::DuckDuckGo => "duckduckgo",
            Self::Searxng => "searxng",
            Self::Brave => "brave",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    pub backend: Backend,
    pub searxng_url: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            backend: Backend::DuckDuckGo,
            searxng_url: String::new(),
        }
    }
}

impl Settings {
    pub fn is_default(&self) -> bool {
        self.backend == Backend::DuckDuckGo && self.searxng_url.is_empty()
    }

    /// Read `webSearch` from `{config}/app.json`. Missing or bad → defaults.
    pub fn load(config_dir: &std::path::Path) -> Self {
        let Ok(text) = std::fs::read_to_string(config_dir.join("app.json")) else {
            return Self::default();
        };
        let Ok(v) = serde_json::from_str::<Value>(&text) else {
            return Self::default();
        };
        let ws = v.get("webSearch").cloned().unwrap_or(Value::Null);
        Self {
            backend: Backend::parse(ws.get("backend").and_then(|b| b.as_str()).unwrap_or("")),
            searxng_url: ws
                .get("searxngUrl")
                .and_then(|u| u.as_str())
                .unwrap_or("")
                .trim()
                .to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit {
    pub title: String,
    pub url: String,
    pub snippet: String,
}

struct CacheEntry {
    at: Instant,
    text: String,
}

struct SearchState {
    last: Option<Instant>,
    cache: HashMap<String, CacheEntry>,
}

fn state() -> &'static Mutex<SearchState> {
    static S: OnceLock<Mutex<SearchState>> = OnceLock::new();
    S.get_or_init(|| {
        Mutex::new(SearchState {
            last: None,
            cache: HashMap::new(),
        })
    })
}

pub(crate) fn schema() -> Value {
    json!({
        "type": "function",
        "name": "web_search",
        "description": "Search the public web and return titles, URLs, and snippets. Default is free DuckDuckGo Lite (no key). Open promising links with web_fetch. Not read-only: it sends a network request. When offline, it reports that search is unavailable instead of failing the turn.",
        "parameters": {
            "type": "object",
            "properties": {
                "query": {"type": "string", "description": "Search query."}
            },
            "required": ["query"],
            "additionalProperties": false
        }
    })
}

pub(crate) fn run_with_ports(args: &Value, stop: &dyn Fn() -> bool) -> ToolOutput {
    match super::ports::current().fetch {
        Some(fetch) => run(args, fetch.as_ref(), stop),
        None => ToolOutput::err(grokhub_core::XAI_NEED_SIGNIN),
    }
}

pub(crate) fn run(args: &Value, fetch: &dyn PageFetch, stop: &dyn Fn() -> bool) -> ToolOutput {
    let query = super::str_field(args, "query");
    let query = query.trim();
    if query.is_empty() {
        return ToolOutput::err("query is required");
    }
    if query.chars().count() > QUERY_CAP {
        return ToolOutput::err(format!("query is too long (max {QUERY_CAP} characters)"));
    }
    let settings = Settings::load(&crate::perm::config_dir());
    let cache_key = format!("{}|{query}", settings.backend.as_str());
    if let Some(hit) = cache_get(&cache_key) {
        return ToolOutput::ok(super::fence::fence("web_search", &hit));
    }
    rate_limit();
    match search(query, &settings, fetch, stop) {
        Ok(hits) => {
            let text = format_hits(query, &hits);
            cache_put(&cache_key, &text);
            ToolOutput::ok(super::fence::fence("web_search", &text))
        }
        Err(err) if is_offline(&err) => {
            ToolOutput::ok(super::fence::fence("web_search", OFFLINE_MSG))
        }
        Err(err) => ToolOutput::err(err),
    }
}

fn is_offline(err: &str) -> bool {
    let e = err.to_ascii_lowercase();
    e.contains("dns")
        || e.contains("offline")
        || e.contains("network")
        || e.contains("timed out")
        || e.contains("timeout")
        || e.contains("connection")
        || e.contains("unreachable")
        || e.contains("name or service not known")
        || e.contains("failed to lookup")
        || e.contains("fetch failed")
}

fn rate_limit() {
    let mut g = state().lock().unwrap_or_else(|e| e.into_inner());
    if let Some(last) = g.last {
        let wait = MIN_GAP.saturating_sub(last.elapsed());
        if !wait.is_zero() {
            drop(g);
            std::thread::sleep(wait);
            g = state().lock().unwrap_or_else(|e| e.into_inner());
        }
    }
    g.last = Some(Instant::now());
}

fn cache_get(key: &str) -> Option<String> {
    let mut g = state().lock().unwrap_or_else(|e| e.into_inner());
    g.cache.retain(|_, e| e.at.elapsed() < CACHE_TTL);
    g.cache.get(key).map(|e| e.text.clone())
}

fn cache_put(key: &str, text: &str) {
    let mut g = state().lock().unwrap_or_else(|e| e.into_inner());
    g.cache.insert(
        key.to_string(),
        CacheEntry {
            at: Instant::now(),
            text: text.to_string(),
        },
    );
}

fn search(
    query: &str,
    settings: &Settings,
    fetch: &dyn PageFetch,
    stop: &dyn Fn() -> bool,
) -> Result<Vec<Hit>, String> {
    match settings.backend {
        Backend::DuckDuckGo => search_duckduckgo(query, fetch, stop),
        Backend::Searxng => search_searxng(query, &settings.searxng_url, fetch, stop),
        Backend::Brave => search_brave(query, fetch, stop),
    }
}

fn guard_hop(url: &str, stop: &dyn Fn() -> bool) -> Result<(), String> {
    let data = crate::harness::model_text_classes(url);
    let req = crate::harness::EgressReq::new(url, data);
    crate::harness::guard_or_park(&crate::perm::config_dir(), &req, "web_search", &mut || {
        stop()
    })
}

fn get_text(url: &str, fetch: &dyn PageFetch, stop: &dyn Fn() -> bool) -> Result<String, String> {
    guard_hop(url, stop)?;
    let resp = fetch.get(url)?;
    if !(200..300).contains(&resp.status) {
        return Err(format!("HTTP {}", resp.status));
    }
    String::from_utf8(resp.body).map_err(|_| "search reply was not utf-8".to_string())
}

fn search_duckduckgo(
    query: &str,
    fetch: &dyn PageFetch,
    stop: &dyn Fn() -> bool,
) -> Result<Vec<Hit>, String> {
    let url = grokhub_core::pulse_sources::duckduckgo_lite_url(query);
    let html = get_text(&url, fetch, stop)?;
    let hits = grokhub_core::pulse_sources::parse_duckduckgo_lite(&html, MAX_RESULTS);
    if hits.is_empty() {
        return Ok(Vec::new());
    }
    Ok(hits
        .into_iter()
        .map(|h| Hit {
            title: h.title,
            url: h.url,
            snippet: h.snippet,
        })
        .collect())
}

fn search_searxng(
    query: &str,
    base: &str,
    fetch: &dyn PageFetch,
    stop: &dyn Fn() -> bool,
) -> Result<Vec<Hit>, String> {
    let base = base.trim().trim_end_matches('/');
    if base.is_empty() {
        return Err(
            "SearXNG backend needs a URL in Settings → Models & providers → Web search".into(),
        );
    }
    if !(base.starts_with("http://") || base.starts_with("https://")) {
        return Err("SearXNG URL must be http or https".into());
    }
    let url = format!(
        "{base}/search?q={}&format=json",
        grokhub_core::pulse_sources::query_value(query)
    );
    let body = get_text(&url, fetch, stop)?;
    parse_searxng_json(&body)
}

fn parse_searxng_json(body: &str) -> Result<Vec<Hit>, String> {
    let v: Value = serde_json::from_str(body).map_err(|e| format!("SearXNG JSON: {e}"))?;
    let mut out = Vec::new();
    for r in v["results"].as_array().into_iter().flatten() {
        if out.len() >= MAX_RESULTS {
            break;
        }
        let title = r["title"].as_str().unwrap_or("").trim();
        let url = r["url"].as_str().unwrap_or("").trim();
        let snippet = r["content"]
            .as_str()
            .or_else(|| r["snippet"].as_str())
            .unwrap_or("")
            .trim();
        if title.is_empty() || url.is_empty() {
            continue;
        }
        if !(url.starts_with("http://") || url.starts_with("https://")) {
            continue;
        }
        out.push(Hit {
            title: title.chars().take(200).collect(),
            url: url.to_string(),
            snippet: snippet.chars().take(400).collect(),
        });
    }
    Ok(out)
}

fn search_brave(
    query: &str,
    fetch: &dyn PageFetch,
    stop: &dyn Fn() -> bool,
) -> Result<Vec<Hit>, String> {
    let key = brave_key()?.ok_or_else(|| {
        "Brave Search needs an API key in Settings → Models & providers → Web search".to_string()
    })?;
    let url = format!(
        "https://api.search.brave.com/res/v1/web/search?q={}&count={}",
        grokhub_core::pulse_sources::query_value(query),
        MAX_RESULTS
    );
    guard_hop(&url, stop)?;
    // Brave needs an API key header; PageFetch::get has no headers. Use ureq directly
    // with the same public-only resolver as web_fetch.
    let _ = fetch; // SSRF path still goes through guard_hop + public host below.
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(10))
        .timeout(Duration::from_secs(30))
        .redirects(0)
        .resolver(public_only_resolve)
        .build();
    let resp = agent
        .get(&url)
        .set("User-Agent", crate::USER_AGENT)
        .set("Accept", "application/json")
        .set("X-Subscription-Token", &key)
        .call()
        .map_err(|e| format!("fetch failed: {e}"))?;
    if !(200..300).contains(&resp.status()) {
        return Err(format!("HTTP {}", resp.status()));
    }
    let body = resp
        .into_string()
        .map_err(|e| format!("read failed: {e}"))?;
    parse_brave_json(&body)
}

fn parse_brave_json(body: &str) -> Result<Vec<Hit>, String> {
    let v: Value = serde_json::from_str(body).map_err(|e| format!("Brave JSON: {e}"))?;
    let mut out = Vec::new();
    for r in v["web"]["results"].as_array().into_iter().flatten() {
        if out.len() >= MAX_RESULTS {
            break;
        }
        let title = r["title"].as_str().unwrap_or("").trim();
        let url = r["url"].as_str().unwrap_or("").trim();
        let snippet = r["description"].as_str().unwrap_or("").trim();
        if title.is_empty() || url.is_empty() {
            continue;
        }
        out.push(Hit {
            title: title.chars().take(200).collect(),
            url: url.to_string(),
            snippet: snippet.chars().take(400).collect(),
        });
    }
    Ok(out)
}

fn brave_key() -> Result<Option<String>, String> {
    let entry = keyring::Entry::new(KEY_SERVICE, BRAVE_KEY_ACCOUNT).map_err(|e| e.to_string())?;
    match entry.get_password() {
        Ok(p) if !p.trim().is_empty() => Ok(Some(p)),
        Ok(_) | Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

/// Seal a Brave Search API key (Settings). Empty clears it.
pub fn seal_brave_key(key: &str) -> Result<(), String> {
    let entry = keyring::Entry::new(KEY_SERVICE, BRAVE_KEY_ACCOUNT).map_err(|e| e.to_string())?;
    let key = key.trim();
    if key.is_empty() {
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(e.to_string()),
        }
    } else {
        entry.set_password(key).map_err(|e| e.to_string())
    }
}

pub fn brave_key_present() -> bool {
    brave_key().ok().flatten().is_some()
}

fn format_hits(query: &str, hits: &[Hit]) -> String {
    let hits = filter_relevant(query, &dedupe_hits_by_host(hits));
    if hits.is_empty() {
        return format!("No results for {query:?}.");
    }
    let mut out = format!("Web search results for {query:?}:\n");
    for (i, h) in hits.iter().enumerate() {
        out.push_str(&format!("{}. {}\n   {}\n", i + 1, h.title, h.url));
        if !h.snippet.is_empty() {
            out.push_str(&format!("   {}\n", h.snippet));
        }
    }
    out.push_str("Open pages you rely on with web_fetch.\n");
    out
}

/// Keep the first hit per host so Sources does not list releases.rs twice.
fn dedupe_hits_by_host(hits: &[Hit]) -> Vec<Hit> {
    let mut out = Vec::with_capacity(hits.len());
    let mut seen = std::collections::HashSet::new();
    for h in hits {
        let host = hit_host(&h.url);
        if host.is_empty() || !seen.insert(host) {
            continue;
        }
        out.push(h.clone());
    }
    out
}

fn hit_host(url: &str) -> String {
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .unwrap_or(url);
    let host = rest.split('/').next().unwrap_or("").to_ascii_lowercase();
    host.strip_prefix("www.").unwrap_or(&host).to_string()
}

/// Drop obvious off-topic hits (e.g. the Rust video game for a programming
/// release-notes query) by token overlap and a small wrong-sense host list.
fn filter_relevant(query: &str, hits: &[Hit]) -> Vec<Hit> {
    if hits.is_empty() {
        return Vec::new();
    }
    let tokens = query_tokens(query);
    let prog = looks_programming(query);
    let rust_q = query_has_token(query, "rust");
    let mut scored: Vec<(usize, &Hit)> = Vec::with_capacity(hits.len());
    for h in hits {
        let host = hit_host(&h.url);
        if prog && rust_q && is_rust_game_host(&host) {
            continue;
        }
        let s = if tokens.is_empty() {
            1
        } else {
            token_score(&tokens, &hit_blob(h))
        };
        scored.push((s, h));
    }
    if scored.is_empty() {
        // Everything was a known collision — keep the original list rather than
        // returning silence for a live search.
        return hits.to_vec();
    }
    let best = scored.iter().map(|(s, _)| *s).max().unwrap_or(0);
    let min_keep = if best == 0 { 0 } else { best.div_ceil(2) };
    let out: Vec<Hit> = scored
        .into_iter()
        .filter(|(s, _)| *s >= min_keep)
        .map(|(_, h)| h.clone())
        .collect();
    if out.is_empty() {
        hits.to_vec()
    } else {
        out
    }
}

const QUERY_STOPWORDS: &[&str] = &[
    "a", "an", "the", "and", "or", "for", "to", "of", "in", "on", "at", "is", "are", "was", "be",
    "with", "from", "by", "as", "it", "this", "that", "these", "those", "current", "latest", "new",
    "how", "what", "where", "when", "why", "which", "who", "web", "search", "find", "look",
    "about", "please", "show", "get", "cite", "real", "links", "link", "today", "now",
];

const PROGRAMMING_SIGNALS: &[&str] = &[
    "release",
    "notes",
    "changelog",
    "cargo",
    "crate",
    "crates",
    "compiler",
    "language",
    "lang",
    "docs",
    "documentation",
    "rfc",
    "stable",
    "nightly",
    "edition",
    "npm",
    "pip",
    "pypi",
    "maven",
    "sdk",
    "api",
    "programming",
    "stdlib",
    "toolchain",
];

fn query_tokens(query: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for raw in query
        .to_ascii_lowercase()
        .split(|c: char| !c.is_ascii_alphanumeric())
    {
        if raw.len() < 3 || QUERY_STOPWORDS.contains(&raw) {
            continue;
        }
        if seen.insert(raw.to_string()) {
            out.push(raw.to_string());
        }
    }
    out
}

fn query_has_token(query: &str, want: &str) -> bool {
    query
        .to_ascii_lowercase()
        .split(|c: char| !c.is_ascii_alphanumeric())
        .any(|t| t == want)
}

fn looks_programming(query: &str) -> bool {
    let lower = query.to_ascii_lowercase();
    PROGRAMMING_SIGNALS.iter().any(|s| lower.contains(s))
}

fn is_rust_game_host(host: &str) -> bool {
    host == "facepunch.com" || host.ends_with(".facepunch.com")
}

fn hit_blob(h: &Hit) -> String {
    format!("{} {} {}", h.title, h.url, h.snippet).to_ascii_lowercase()
}

fn token_score(tokens: &[String], blob: &str) -> usize {
    tokens.iter().filter(|t| blob.contains(t.as_str())).count()
}

/// True when the function tool named `web_search` should be stripped because
/// hosted xAI search is already on the wire.
pub fn is_function_web_search(tool: &Value) -> bool {
    tool.get("type").and_then(|t| t.as_str()) == Some("function")
        && tool.get("name").and_then(|n| n.as_str()) == Some("web_search")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::IpAddr;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    struct FakeFetch {
        body: String,
        calls: Arc<AtomicUsize>,
    }

    impl PageFetch for FakeFetch {
        fn resolve(&self, _host: &str, _port: u16) -> Result<Vec<IpAddr>, String> {
            Ok(vec![IpAddr::from([93, 184, 216, 34])])
        }
        fn get(&self, _url: &str) -> Result<super::super::web_fetch::FetchResp, String> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(super::super::web_fetch::FetchResp {
                status: 200,
                location: None,
                content_type: "text/html".into(),
                body: self.body.as_bytes().to_vec(),
            })
        }
    }

    struct FailFetch;

    impl PageFetch for FailFetch {
        fn resolve(&self, _host: &str, _port: u16) -> Result<Vec<IpAddr>, String> {
            Err("dns failed for lite.duckduckgo.com: offline".into())
        }
        fn get(&self, _url: &str) -> Result<super::super::web_fetch::FetchResp, String> {
            Err("fetch failed: connection refused".into())
        }
    }

    #[test]
    fn schema_is_function_web_search() {
        let s = schema();
        assert_eq!(s["type"], "function");
        assert_eq!(s["name"], "web_search");
        assert!(s["parameters"]["required"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v == "query"));
    }

    #[test]
    fn formats_ddg_hits_and_fences() {
        let dir = crate::harness::test_dir("web-search-ddg");
        let _cfg = crate::perm::ConfigGuard::set(&dir);
        crate::tools::fence::reset_run_nonce_for_tests();
        let html = r#"
            <a rel="nofollow" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fexample.org%2Frust&amp;rut=x" class='result-link'>Rust Lang</a>
            <td class='result-snippet'>A systems language.</td>
        "#;
        let fetch = FakeFetch {
            body: html.into(),
            calls: Arc::new(AtomicUsize::new(0)),
        };
        let out = run(&json!({"query": "rust"}), &fetch, &|| false);
        assert!(!out.failed, "{}", out.text);
        assert!(
            out.text.contains("<<<UNTRUSTED source=web_search"),
            "{}",
            out.text
        );
        assert!(
            out.text.contains("https://example.org/rust") || out.text.contains("example.org"),
            "{}",
            out.text
        );
        assert!(
            out.text.contains("Rust Lang") || out.text.contains("results"),
            "{}",
            out.text
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn offline_returns_unavailable_not_error() {
        let dir = crate::harness::test_dir("web-search-offline");
        let _cfg = crate::perm::ConfigGuard::set(&dir);
        crate::tools::fence::reset_run_nonce_for_tests();
        let out = run(&json!({"query": "news today"}), &FailFetch, &|| false);
        assert!(!out.failed, "{}", out.text);
        assert!(
            out.text.contains("unavailable") || out.text.contains("offline"),
            "{}",
            out.text
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn searxng_json_parses_title_url_snippet() {
        let body = r#"{"results":[{"title":"One","url":"https://a.example/","content":"snip"}]}"#;
        let hits = parse_searxng_json(body).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].title, "One");
        assert_eq!(hits[0].url, "https://a.example/");
        assert_eq!(hits[0].snippet, "snip");
    }

    #[test]
    fn backend_parse_defaults_to_duckduckgo() {
        assert_eq!(Backend::parse(""), Backend::DuckDuckGo);
        assert_eq!(Backend::parse("searxng"), Backend::Searxng);
        assert_eq!(Backend::parse("brave"), Backend::Brave);
    }

    #[test]
    fn strips_function_web_search_detection() {
        assert!(is_function_web_search(
            &json!({"type":"function","name":"web_search"})
        ));
        assert!(!is_function_web_search(&json!({"type":"web_search"})));
        assert!(!is_function_web_search(
            &json!({"type":"function","name":"web_fetch"})
        ));
    }

    #[test]
    fn format_hits_numbers_up_and_collapses_duplicate_hosts() {
        let hits = [
            Hit {
                title: "Doc".into(),
                url: "https://doc.rust-lang.org/stable/releases.html".into(),
                snippet: "Official notes.".into(),
            },
            Hit {
                title: "1.99".into(),
                url: "https://releases.rs/docs/1.99.0/".into(),
                snippet: "Changelog.".into(),
            },
            Hit {
                title: "All".into(),
                url: "https://releases.rs/".into(),
                snippet: "Index.".into(),
            },
            Hit {
                title: "Blog".into(),
                url: "https://www.rust.dev/updates".into(),
                snippet: "Updates.".into(),
            },
        ];
        let text = format_hits("rust release", &hits);
        assert!(text.contains("1. Doc\n"), "{text}");
        assert!(text.contains("2. 1.99\n"), "{text}");
        assert!(text.contains("3. Blog\n"), "{text}");
        assert!(!text.contains("4."), "{text}");
        assert!(
            text.matches("releases.rs").count() == 1,
            "duplicate host must collapse: {text}"
        );
        assert!(text.contains("Official notes."), "{text}");
        assert!(!text.contains("Index."), "{text}");
    }

    #[test]
    fn drops_rust_game_hit_for_programming_release_notes() {
        let hits = [
            Hit {
                title: "Releases".into(),
                url: "https://doc.rust-lang.org/releases.html".into(),
                snippet: "Rust release notes for each stable.".into(),
            },
            Hit {
                title: "1.91.0".into(),
                url: "https://releases.rs/docs/1.91.0/".into(),
                snippet: "Changelog for the Rust language.".into(),
            },
            Hit {
                title: "Rust".into(),
                url: "https://rust.facepunch.com/".into(),
                snippet: "The only surviving multiplayer survival game.".into(),
            },
            Hit {
                title: "Blog".into(),
                url: "https://blog.rust-lang.org/releases/".into(),
                snippet: "Official Rust release announcements.".into(),
            },
        ];
        let kept = filter_relevant("current Rust release notes", &hits);
        assert!(
            !kept.iter().any(|h| h.url.contains("facepunch")),
            "game host must drop: {kept:?}"
        );
        assert!(
            kept.iter().any(|h| h.url.contains("rust-lang.org")),
            "programming hits must stay: {kept:?}"
        );
        let text = format_hits("current Rust release notes", &hits);
        assert!(!text.contains("facepunch"), "{text}");
        assert!(text.contains("doc.rust-lang.org"), "{text}");
    }

    #[test]
    fn keeps_rust_game_hit_when_query_asks_for_the_game() {
        let hits = [
            Hit {
                title: "Rust".into(),
                url: "https://rust.facepunch.com/".into(),
                snippet: "The Facepunch multiplayer survival game.".into(),
            },
            Hit {
                title: "Steam".into(),
                url: "https://store.steampowered.com/app/252490/Rust/".into(),
                snippet: "Buy Rust on Steam.".into(),
            },
        ];
        let kept = filter_relevant("Rust game Facepunch", &hits);
        assert!(
            kept.iter().any(|h| h.url.contains("facepunch")),
            "game query must keep Facepunch: {kept:?}"
        );
    }
}
