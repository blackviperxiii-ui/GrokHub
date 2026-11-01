//! One shape for every mod source: what a search returns, what a mod's page
//! and files say, and the one door a source reads the network through.
//!
//! Sources only read public metadata. The network is the [`Fetch`] trait: the
//! app implements it over the guarded HTTP client, and tests use a fake that
//! serves fixtures, so nothing here dials anything.

use serde::{Deserialize, Serialize};

use super::allowlist::Source;
use super::ids::GameKey;

/// How a browse tab orders mods.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Browse {
    Popular,
    Trending,
    Updated,
}

impl Browse {
    pub const ALL: [Browse; 3] = [Browse::Popular, Browse::Trending, Browse::Updated];

    pub fn label(self) -> &'static str {
        match self {
            Browse::Popular => "Popular",
            Browse::Trending => "Trending",
            Browse::Updated => "Updated",
        }
    }
}

/// One page of a search or a browse tab. `text` set means a text search.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Query {
    pub text: Option<String>,
    pub browse: Browse,
    /// Zero-based page.
    pub page: u32,
}

/// Mods per page, for every source.
pub const PAGE_SIZE: u32 = 20;

impl Query {
    pub fn browse(browse: Browse) -> Self {
        Self {
            text: None,
            browse,
            page: 0,
        }
    }

    pub fn search(text: &str) -> Self {
        Self {
            text: Some(text.trim().to_string()),
            browse: Browse::Popular,
            page: 0,
        }
    }

    /// The search text, when there is any after trimming.
    pub fn text(&self) -> Option<&str> {
        self.text
            .as_deref()
            .map(str::trim)
            .filter(|t| !t.is_empty())
    }

    pub fn offset(&self) -> u32 {
        self.page.saturating_mul(PAGE_SIZE)
    }
}

/// A mod as a result card shows it.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ModSummary {
    pub source: Option<SourceId>,
    pub id: String,
    pub name: String,
    pub author: String,
    pub summary: String,
    pub downloads: u64,
    /// Endorsements, follows, favourites or rating, whichever the source has.
    pub likes: u64,
    /// Unix seconds.
    pub updated: u64,
    pub adult: bool,
    /// The mod's own page on its site.
    pub page_url: String,
    pub icon_url: Option<String>,
    pub categories: Vec<String>,
}

/// A serializable [`Source`] (the allowlist enum stays plain).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SourceId(pub u8);

impl From<Source> for SourceId {
    fn from(s: Source) -> Self {
        SourceId(Source::ALL.iter().position(|x| *x == s).unwrap_or(0) as u8)
    }
}

impl SourceId {
    pub fn source(self) -> Source {
        Source::ALL[usize::from(self.0) % Source::ALL.len()]
    }
}

/// A mod's detail drawer.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ModDetail {
    pub summary: ModSummary,
    /// Plain text (HTML, BBCode and markdown markup stripped), capped.
    pub description: String,
    pub game_versions: Vec<String>,
}

/// A published hash for one file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum FileHash {
    Sha512(String),
    Sha1(String),
    Md5(String),
}

/// How a file reaches the user.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Download {
    /// A direct link on the source's own CDN.
    Direct(String),
    /// The user downloads it in their browser from this page.
    Browser(String),
    /// Steam downloads it when the user subscribes on this Workshop item.
    Steam(u64),
}

/// One downloadable file of a mod.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModFile {
    pub id: String,
    pub name: String,
    pub file_name: String,
    pub version: String,
    pub size: u64,
    pub hash: Option<FileHash>,
    pub download: Download,
    pub game_versions: Vec<String>,
    /// Ids (on the same source) of mods this file needs.
    pub deps: Vec<String>,
    pub primary: bool,
    /// Unix seconds.
    pub uploaded: u64,
}

/// One page of results.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Page {
    pub mods: Vec<ModSummary>,
    pub total: Option<u64>,
}

/// One request to a source's public API.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub source: Source,
    pub url: String,
    pub headers: Vec<(String, String)>,
    /// A body makes it a POST.
    pub body: Option<String>,
    /// The request carries text the user typed (a search).
    pub carries_text: bool,
}

impl Request {
    pub fn get(source: Source, url: String) -> Self {
        Self {
            source,
            url,
            headers: Vec::new(),
            body: None,
            carries_text: false,
        }
    }

    pub fn header(mut self, name: &str, value: &str) -> Self {
        self.headers.push((name.to_string(), value.to_string()));
        self
    }

    pub fn with_text(mut self, carries: bool) -> Self {
        self.carries_text = carries;
        self
    }
}

/// What came back.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Reply {
    pub status: u16,
    pub body: Vec<u8>,
    /// Lower-case names.
    pub headers: Vec<(String, String)>,
}

impl Reply {
    pub fn ok(body: &str) -> Self {
        Self {
            status: 200,
            body: body.as_bytes().to_vec(),
            headers: Vec::new(),
        }
    }

    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

/// Why a source gave nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceError {
    /// Offline only is on.
    Offline,
    /// This source needs a key the user hasn't saved.
    NeedsKey(Source),
    /// Too many requests; try again after this many seconds.
    RateLimited(u64),
    /// The source answered with this HTTP status.
    Http(u16),
    /// Couldn't reach it.
    Net(String),
    /// The answer wasn't what the source documents.
    Parse(String),
    /// This source can't do that here (and the line says what to do instead).
    Unsupported(String),
}

impl SourceError {
    /// One plain line for the page.
    pub fn line(&self) -> String {
        match self {
            SourceError::Offline => "Offline only is on.".into(),
            SourceError::NeedsKey(s) => format!("{} needs a key first.", s.label()),
            SourceError::RateLimited(secs) => {
                format!("Slowing down for the source's limits; try again in {secs} s.")
            }
            SourceError::Http(code) => format!("The source answered {code}."),
            SourceError::Net(why) => format!("Couldn't reach the source: {why}"),
            SourceError::Parse(why) => format!("The source sent something unexpected: {why}"),
            SourceError::Unsupported(why) => why.clone(),
        }
    }
}

/// The only way a source reaches the network.
pub trait Fetch: Send + Sync {
    fn send(&self, req: &Request) -> Result<Reply, SourceError>;
}

/// A source's reply, turned into an error unless it is a 2xx.
pub fn ok_body(reply: Reply) -> Result<Vec<u8>, SourceError> {
    match reply.status {
        200..=299 => Ok(reply.body),
        429 => Err(SourceError::RateLimited(
            reply
                .header("retry-after")
                .and_then(|v| v.trim().parse().ok())
                .unwrap_or(60),
        )),
        code => Err(SourceError::Http(code)),
    }
}

pub fn parse_json<T: serde::de::DeserializeOwned>(body: &[u8]) -> Result<T, SourceError> {
    serde_json::from_slice(body).map_err(|e| SourceError::Parse(e.to_string()))
}

/// Keys the user (or the build) gave. Never logged, never written here.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct Keys {
    pub nexus: Option<String>,
    pub steam: Option<String>,
    pub curseforge: Option<String>,
}

impl std::fmt::Debug for Keys {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Keys")
            .field("nexus", &self.nexus.is_some())
            .field("steam", &self.steam.is_some())
            .field("curseforge", &self.curseforge.is_some())
            .finish()
    }
}

/// One mod source: read-only search, detail and file list.
pub trait ModSource {
    fn source(&self) -> Source;
    fn search(&self, net: &dyn Fetch, game: GameKey, q: &Query) -> Result<Page, SourceError>;
    fn detail(&self, net: &dyn Fetch, game: GameKey, id: &str) -> Result<ModDetail, SourceError>;
    fn files(&self, net: &dyn Fetch, game: GameKey, id: &str) -> Result<Vec<ModFile>, SourceError>;
}

/// Most characters of a description a page or a tool shows.
pub const DESCRIPTION_CAP: usize = 8 * 1024;

/// Plain text from a source's HTML, BBCode or markdown: tags dropped, common
/// entities decoded, blank runs folded, capped at `cap` characters.
pub fn plain_text(raw: &str, cap: usize) -> String {
    let chars: Vec<char> = raw.chars().collect();
    let mut out = String::with_capacity(raw.len().min(cap));
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let end = match c {
            '<' => chars[i..].iter().position(|&x| x == '>'),
            '[' => bbcode_end(&chars[i..]),
            _ => None,
        };
        match end {
            Some(n) => {
                let tag: String = chars[i + 1..i + n].iter().collect();
                if breaks_line(&tag) {
                    out.push('\n');
                }
                i += n + 1;
            }
            None => {
                out.push(c);
                i += 1;
            }
        }
    }
    let decoded = out
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'");
    let mut text = String::new();
    let mut blank = false;
    for line in decoded.lines() {
        let line = line.trim().trim_start_matches('#');
        let line = line.split_whitespace().collect::<Vec<_>>().join(" ");
        if line.is_empty() {
            blank = true;
            continue;
        }
        if !text.is_empty() {
            text.push_str(if blank { "\n\n" } else { "\n" });
        }
        blank = false;
        text.push_str(&line);
    }
    let trimmed = text.trim();
    if trimmed.chars().count() > cap {
        let mut cut: String = trimmed.chars().take(cap).collect();
        cut.push('…');
        cut
    } else {
        trimmed.to_string()
    }
}

/// Tags that end a line (`<br>`, `<p>`, `</div>`, `[*]`, `[list]`); every
/// other tag just goes away.
fn breaks_line(tag: &str) -> bool {
    let name: String = tag
        .trim_start_matches('/')
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '*')
        .collect::<String>()
        .to_ascii_lowercase();
    matches!(
        name.as_str(),
        "br" | "p"
            | "div"
            | "li"
            | "ul"
            | "ol"
            | "tr"
            | "h1"
            | "h2"
            | "h3"
            | "h4"
            | "h5"
            | "h6"
            | "*"
            | "list"
            | "center"
            | "quote"
    )
}

/// The length to the `]` of a BBCode tag (`[b]`, `[/url]`, `[size=4]`), so a
/// markdown link's `[text]` is kept.
fn bbcode_end(chars: &[char]) -> Option<usize> {
    let close = chars.iter().take(40).position(|&x| x == ']')?;
    let inner: String = chars[1..close].iter().collect();
    let name = inner.trim_start_matches('/');
    let name = name.split('=').next().unwrap_or("");
    let known = [
        "b", "i", "u", "s", "url", "img", "size", "color", "font", "center", "quote", "code",
        "list", "*", "spoiler", "line", "youtube", "left", "right", "h1", "h2", "h3",
    ];
    known
        .contains(&name.to_ascii_lowercase().as_str())
        .then_some(close)
}

/// Percent-encode one query value (RFC 3986 unreserved kept).
pub fn enc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// Unix seconds from an RFC 3339 / ISO 8601 UTC time (`2024-05-01T12:00:00Z`,
/// fractional seconds and offsets allowed; the offset is applied).
pub fn unix_from_iso(s: &str) -> u64 {
    let s = s.trim();
    let (date, rest) = s.split_at(s.find('T').unwrap_or(s.len()));
    let Some(days) = super::allowlist::days_from_ymd(date) else {
        return 0;
    };
    let rest = rest.trim_start_matches('T');
    let time_end = rest.find(['Z', '+', '-']).unwrap_or(rest.len());
    let (time, zone) = rest.split_at(time_end);
    let mut parts = time.split(':');
    let h: i64 = parts.next().and_then(|v| v.parse().ok()).unwrap_or(0);
    let m: i64 = parts.next().and_then(|v| v.parse().ok()).unwrap_or(0);
    let sec: i64 = parts
        .next()
        .and_then(|v| v.split('.').next())
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let offset = match zone.chars().next() {
        Some(sign @ ('+' | '-')) => {
            let z = &zone[1..];
            let (zh, zm) = z.split_once(':').unwrap_or((z, "0"));
            let mins = zh.parse::<i64>().unwrap_or(0) * 60 + zm.parse::<i64>().unwrap_or(0);
            if sign == '+' {
                mins * 60
            } else {
                -mins * 60
            }
        }
        _ => 0,
    };
    (days * 86_400 + h * 3600 + m * 60 + sec - offset).max(0) as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_text_drops_markup_and_caps() {
        assert_eq!(
            plain_text("<p>Hello&nbsp;<b>world</b></p>[b]bold[/b] &amp; more", 100),
            "Hello world\nbold & more"
        );
        assert_eq!(plain_text("abcdef", 3), "abc…");
        assert_eq!(plain_text("one\n\n\n\ntwo", 50), "one\n\ntwo");
        assert_eq!(
            plain_text("see [the guide](x) [url=y]here[/url]", 50),
            "see [the guide](x) here"
        );
    }

    #[test]
    fn iso_times_turn_into_unix_seconds() {
        assert_eq!(unix_from_iso("1970-01-02T00:00:00Z"), 86_400);
        assert_eq!(
            unix_from_iso("2026-11-01T10:30:00.123Z"),
            20_758 * 86_400 + 37_800
        );
        assert_eq!(
            unix_from_iso("2026-11-01T10:30:00+01:00"),
            20_758 * 86_400 + 34_200
        );
        assert_eq!(unix_from_iso("garbage"), 0);
    }

    #[test]
    fn query_values_are_percent_encoded() {
        assert_eq!(enc("sky ui & more"), "sky%20ui%20%26%20more");
        assert_eq!(enc("a-b_c.d~"), "a-b_c.d~");
    }

    #[test]
    fn replies_turn_into_errors_by_status() {
        assert_eq!(ok_body(Reply::ok("x")), Ok(b"x".to_vec()));
        let mut r = Reply::ok("");
        r.status = 429;
        r.headers.push(("retry-after".into(), "12".into()));
        assert_eq!(ok_body(r), Err(SourceError::RateLimited(12)));
        let mut r = Reply::ok("");
        r.status = 404;
        assert_eq!(ok_body(r), Err(SourceError::Http(404)));
        assert_eq!(
            SourceError::NeedsKey(Source::CurseForge).line(),
            "CurseForge needs a key first."
        );
    }

    #[test]
    fn keys_never_print_their_values() {
        let k = Keys {
            nexus: Some("secret-value".into()),
            ..Default::default()
        };
        let shown = format!("{k:?}");
        assert!(!shown.contains("secret-value"), "{shown}");
        assert!(shown.contains("nexus: true"));
    }

    #[test]
    fn source_ids_survive_a_round_trip() {
        for s in Source::ALL {
            assert_eq!(SourceId::from(s).source(), s);
        }
    }
}
