// Portions derived from xai-org/grok-build (Apache-2.0, © SpaceXAI), commit 2bdd1d6a; modified.

//! `web_fetch`: one public http(s) GET, HTML rewritten as markdown.
//! Redirects are checked again. Loopback, private, and link-local addresses
//! are refused after DNS. The xAI bearer is never sent with the page request.

use std::collections::HashSet;
use std::io::Read;
use std::net::{IpAddr, Ipv4Addr, ToSocketAddrs};
use std::time::Duration;

use serde_json::{json, Value};

use super::html_md;
use super::ToolOutput;

const MAX_REDIRECTS: u32 = 10;
const MAX_URL_LEN: usize = 2000;
const RAW_CAP: usize = 5 * 1024 * 1024;
const MD_CAP: usize = 100_000;

#[derive(Debug, Clone)]
pub struct FetchResp {
    pub status: u16,
    pub location: Option<String>,
    pub content_type: String,
    pub body: Vec<u8>,
}

pub trait PageFetch: Send + Sync {
    fn resolve(&self, host: &str, port: u16) -> Result<Vec<IpAddr>, String>;
    fn get(&self, url: &str) -> Result<FetchResp, String>;
}

pub(crate) struct BlockedFetch;

impl PageFetch for BlockedFetch {
    fn resolve(&self, _host: &str, _port: u16) -> Result<Vec<IpAddr>, String> {
        Err(grokhub_core::XAI_NEED_SIGNIN.to_string())
    }

    fn get(&self, _url: &str) -> Result<FetchResp, String> {
        Err(grokhub_core::XAI_NEED_SIGNIN.to_string())
    }
}

pub(crate) struct UreqFetch {
    agent: ureq::Agent,
}

impl Default for UreqFetch {
    fn default() -> Self {
        Self::new()
    }
}

impl UreqFetch {
    pub(crate) fn new() -> Self {
        let agent = ureq::AgentBuilder::new()
            .timeout_connect(Duration::from_secs(10))
            .timeout(Duration::from_secs(30))
            .redirects(0)
            .resolver(public_only_resolve)
            .build();
        Self { agent }
    }
}

impl PageFetch for UreqFetch {
    fn resolve(&self, host: &str, port: u16) -> Result<Vec<IpAddr>, String> {
        let mut ips = Vec::new();
        for sock in (host, port)
            .to_socket_addrs()
            .map_err(|err| format!("dns failed for {host}: {err}"))?
        {
            let ip = sock.ip();
            if !ips.contains(&ip) {
                ips.push(ip);
            }
        }
        if ips.is_empty() {
            return Err(format!("dns failed for {host}: no addresses"));
        }
        Ok(ips)
    }

    fn get(&self, url: &str) -> Result<FetchResp, String> {
        match self
            .agent
            .get(url)
            .set("User-Agent", crate::USER_AGENT)
            .set(
                "Accept",
                "text/html, text/plain, application/json;q=0.9, */*;q=0.1",
            )
            .call()
        {
            Ok(resp) => owned_response(resp.status(), resp),
            Err(ureq::Error::Status(status, resp)) => {
                if is_redirect(status) {
                    return Ok(FetchResp {
                        status,
                        location: resp.header("location").map(str::to_string),
                        content_type: resp.header("content-type").unwrap_or("").to_string(),
                        body: Vec::new(),
                    });
                }
                Err(format!("HTTP {status}"))
            }
            Err(ureq::Error::Transport(err)) => Err(format!("fetch failed: {err}")),
        }
    }
}

fn owned_response(status: u16, resp: ureq::Response) -> Result<FetchResp, String> {
    let location = resp.header("location").map(str::to_string);
    let content_type = resp.header("content-type").unwrap_or("").to_string();
    if is_redirect(status) {
        return Ok(FetchResp {
            status,
            location,
            content_type,
            body: Vec::new(),
        });
    }
    if !(200..300).contains(&status) {
        return Err(format!("HTTP {status}"));
    }
    let mut reader = resp.into_reader();
    let body = read_limited(&mut reader)?;
    Ok(FetchResp {
        status,
        location,
        content_type,
        body,
    })
}

fn read_limited(reader: &mut impl Read) -> Result<Vec<u8>, String> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 8192];
    let cap = RAW_CAP.saturating_add(1);
    loop {
        let n = reader
            .read(&mut chunk)
            .map_err(|err| format!("read failed: {err}"))?;
        if n == 0 {
            break;
        }
        let room = cap.saturating_sub(buf.len());
        if room == 0 {
            break;
        }
        let take = n.min(room);
        buf.extend_from_slice(&chunk[..take]);
        if buf.len() >= cap {
            break;
        }
    }
    Ok(buf)
}

pub(crate) fn schema() -> Value {
    json!({
        "type": "function",
        "name": "web_fetch",
        "description": "Fetch a public http or https URL and return the page as markdown. Refuses non-http schemes, userinfo, and localhost, private, or link-local addresses. Not read-only: it sends a network request.",
        "parameters": {
            "type": "object",
            "properties": {
                "url": {"type": "string", "description": "Absolute http or https URL."}
            },
            "required": ["url"],
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

/// `stop` is cancel or Halt: it denies a hard Send card that is waiting.
pub(crate) fn run(args: &Value, fetch: &dyn PageFetch, stop: &dyn Fn() -> bool) -> ToolOutput {
    let url = super::str_field(args, "url");
    if url.is_empty() {
        return ToolOutput::err("url is required");
    }
    match fetch_markdown(&url, fetch, stop) {
        Ok(text) => ToolOutput::ok(text),
        Err(err) => ToolOutput::err(err),
    }
}

/// EgressGuard (Spike-4c) before each hop: the URL is one the model picked, so
/// it is chat, or personal when it carries a recall-pack line. A redirect to a
/// new host asks again. A hard Send that is not approved sends nothing.
fn guard_hop(url: &str, stop: &dyn Fn() -> bool) -> Result<(), String> {
    let data = crate::harness::model_text_classes(url);
    let req = crate::harness::EgressReq::new(url, data);
    crate::harness::guard_or_park(&crate::perm::config_dir(), &req, "web_fetch", &mut || stop())
}

fn fetch_markdown(raw: &str, fetch: &dyn PageFetch, stop: &dyn Fn() -> bool) -> Result<String, String> {
    let mut current = parse_public_url(raw)?;
    let mut seen = HashSet::new();
    let mut redirects = 0u32;
    loop {
        let url = current.display();
        if !seen.insert(url.clone()) {
            return Err("redirect loop".into());
        }
        check_ssrf(&current, fetch)?;
        guard_hop(&url, stop)?;
        let resp = fetch.get(&url)?;
        if is_redirect(resp.status) {
            redirects = redirects.saturating_add(1);
            if redirects > MAX_REDIRECTS {
                return Err("too many redirects".into());
            }
            let loc = resp
                .location
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .ok_or_else(|| "redirect missing location".to_string())?;
            current = join_redirect(&current, loc)?;
            continue;
        }
        if !(200..300).contains(&resp.status) {
            return Err(format!("HTTP {}", resp.status));
        }
        return render_body(&resp.content_type, &resp.body);
    }
}

fn is_redirect(status: u16) -> bool {
    matches!(status, 301 | 302 | 303 | 307 | 308)
}

fn render_body(content_type: &str, body: &[u8]) -> Result<String, String> {
    let (body, raw_cut) = cap_raw(body);
    match classify(content_type, body)? {
        BodyKind::Html => {
            let text = String::from_utf8_lossy(body);
            Ok(cap_text(html_md::html_to_markdown(&text), raw_cut))
        }
        BodyKind::Text => {
            let text = String::from_utf8_lossy(body).into_owned();
            Ok(cap_text(text, raw_cut))
        }
    }
}

fn cap_raw(body: &[u8]) -> (&[u8], bool) {
    if body.len() > RAW_CAP {
        (&body[..RAW_CAP], true)
    } else {
        (body, false)
    }
}

fn cap_text(text: String, raw_cut: bool) -> String {
    let over = text.chars().count() > MD_CAP;
    let mut text: String = text.chars().take(MD_CAP).collect();
    if over || raw_cut {
        if !text.ends_with('\n') {
            text.push('\n');
        }
        text.push_str("\n[truncated]\n");
    }
    text
}

enum BodyKind {
    Html,
    Text,
}

fn classify(content_type: &str, body: &[u8]) -> Result<BodyKind, String> {
    let ct = content_type
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    match ct.as_str() {
        "text/html" | "application/xhtml+xml" => Ok(BodyKind::Html),
        "text/plain"
        | "text/markdown"
        | "text/x-markdown"
        | "application/json"
        | "text/json"
        | "application/xml"
        | "text/xml"
        | "application/javascript"
        | "text/javascript"
        | "text/css" => Ok(BodyKind::Text),
        "" => sniff_empty(body),
        other if other.starts_with("text/") => Ok(BodyKind::Text),
        other => Err(format!("unsupported content type: {other}")),
    }
}

fn sniff_empty(body: &[u8]) -> Result<BodyKind, String> {
    let head = String::from_utf8_lossy(&body[..body.len().min(256)]).to_ascii_lowercase();
    let head = head.trim_start();
    if head.starts_with("<!doctype html")
        || head.starts_with("<html")
        || head.starts_with("<head")
        || head.starts_with("<body")
    {
        return Ok(BodyKind::Html);
    }
    if std::str::from_utf8(body).is_ok() {
        Ok(BodyKind::Text)
    } else {
        Err("unsupported content type".into())
    }
}

#[derive(Debug, Clone)]
struct UrlParts {
    host: String,
    port: Option<u16>,
    path: String,
}

impl UrlParts {
    fn display(&self) -> String {
        let host = if self.host.contains(':') {
            format!("[{}]", self.host)
        } else {
            self.host.clone()
        };
        match self.port {
            Some(port) if port != 443 => format!("https://{host}:{port}{}", self.path),
            _ => format!("https://{host}{}", self.path),
        }
    }

    fn dns_port(&self) -> u16 {
        self.port.unwrap_or(443)
    }
}

fn parse_public_url(raw: &str) -> Result<UrlParts, String> {
    let raw = raw.trim();
    if raw.len() > MAX_URL_LEN {
        return Err("url is too long".into());
    }
    let Some((scheme, rest)) = raw.split_once("://") else {
        return Err("only http and https urls are allowed".into());
    };
    let scheme = scheme.to_ascii_lowercase();
    if scheme != "http" && scheme != "https" {
        return Err("only http and https urls are allowed".into());
    }
    if rest.contains('@') {
        return Err("urls with userinfo are not allowed".into());
    }
    let (host, port, path_src) = split_host(rest)?;
    let host = normalize_host(&host);
    if host.is_empty() {
        return Err("url has no host".into());
    }
    Ok(UrlParts {
        host,
        port,
        path: normalize_path(path_src),
    })
}

fn split_host(rest: &str) -> Result<(String, Option<u16>, &str), String> {
    if rest.is_empty() {
        return Err("url has no host".into());
    }
    if let Some(body) = rest.strip_prefix('[') {
        let end = body.find(']').ok_or_else(|| "bad ipv6 host".to_string())?;
        let host = body[..end].to_string();
        let after = &body[end + 1..];
        if let Some(after_port) = after.strip_prefix(':') {
            let pend = after_port.find(['/', '?', '#']).unwrap_or(after_port.len());
            let port = parse_port(&after_port[..pend])?;
            return Ok((host, Some(port), &after_port[pend..]));
        }
        return Ok((host, None, after));
    }
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let hostport = &rest[..end];
    if hostport.matches(':').count() > 1 {
        return Err("bracket ipv6 hosts".into());
    }
    let (host, port) = split_host_port(hostport)?;
    Ok((host, port, &rest[end..]))
}

fn split_host_port(hostport: &str) -> Result<(String, Option<u16>), String> {
    if let Some((host, port)) = hostport.rsplit_once(':') {
        if !host.is_empty() && !port.is_empty() && port.bytes().all(|b| b.is_ascii_digit()) {
            return Ok((host.to_string(), Some(parse_port(port)?)));
        }
    }
    Ok((hostport.to_string(), None))
}

fn parse_port(text: &str) -> Result<u16, String> {
    if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
        return Err("bad port".into());
    }
    text.parse::<u16>().map_err(|_| "bad port".into())
}

fn normalize_host(host: &str) -> String {
    host.trim()
        .trim_matches(|c| c == '[' || c == ']')
        .trim_end_matches('.')
        .to_ascii_lowercase()
}

fn normalize_path(rest: &str) -> String {
    let rest = rest.split('#').next().unwrap_or("");
    if rest.is_empty() {
        return "/".into();
    }
    if rest.starts_with('/') || rest.starts_with('?') {
        if rest.starts_with('?') {
            return format!("/{rest}");
        }
        return rest.to_string();
    }
    format!("/{rest}")
}

fn join_redirect(base: &UrlParts, location: &str) -> Result<UrlParts, String> {
    let loc = location.trim();
    if loc.is_empty() {
        return Err("redirect missing location".into());
    }
    if loc.contains("://") {
        return parse_public_url(loc);
    }
    if let Some(rest) = loc.strip_prefix("//") {
        return parse_public_url(&format!("https://{rest}"));
    }
    if loc.starts_with('/') {
        return Ok(UrlParts {
            host: base.host.clone(),
            port: base.port,
            path: normalize_path(loc),
        });
    }
    let (rel, query) = loc
        .split_once('?')
        .map(|(p, q)| (p, Some(q)))
        .unwrap_or((loc, None));
    let base_path = base.path.split('?').next().unwrap_or("/");
    let dir = match base_path.rsplit_once('/') {
        Some((dir, _)) if !dir.is_empty() => dir,
        _ => "",
    };
    let joined = if dir.is_empty() {
        format!("/{rel}")
    } else {
        format!("{dir}/{rel}")
    };
    let collapsed = collapse_dots(&joined);
    let path = match query {
        Some(q) => format!("{collapsed}?{q}"),
        None => collapsed,
    };
    Ok(UrlParts {
        host: base.host.clone(),
        port: base.port,
        path,
    })
}

fn collapse_dots(path: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for part in path.split('/') {
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." {
            parts.pop();
            continue;
        }
        parts.push(part);
    }
    if parts.is_empty() {
        "/".into()
    } else {
        format!("/{}", parts.join("/"))
    }
}

fn check_ssrf(url: &UrlParts, fetch: &dyn PageFetch) -> Result<(), String> {
    if host_name_blocked(&url.host) {
        return Err(format!("refused non-public address {}", url.host));
    }
    if let Ok(ip) = url.host.parse::<IpAddr>() {
        if ip_blocked(ip) {
            return Err(format!("refused non-public address {}", url.host));
        }
        return Ok(());
    }
    let ips = fetch.resolve(&url.host, url.dns_port())?;
    if ips.is_empty() {
        return Err(format!("dns failed for {}: no addresses", url.host));
    }
    if ips.iter().any(|ip| ip_blocked(*ip)) {
        return Err(format!("refused non-public address {}", url.host));
    }
    Ok(())
}

/// The resolver ureq dials with. It drops every non-public address, so a DNS answer
/// that changes between `check_ssrf` and the connect (rebinding) still can't reach
/// localhost, link-local or private ranges.
pub(crate) fn public_only_resolve(netloc: &str) -> std::io::Result<Vec<std::net::SocketAddr>> {
    let host = netloc
        .rsplit_once(':')
        .map(|(host, _)| host)
        .unwrap_or(netloc)
        .trim_matches(|c| c == '[' || c == ']');
    if host_name_blocked(host) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            format!("refused non-public address {host}"),
        ));
    }
    let all: Vec<std::net::SocketAddr> = netloc.to_socket_addrs()?.collect();
    if all.iter().any(|addr| ip_blocked(addr.ip())) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            format!("refused non-public address {host}"),
        ));
    }
    if all.is_empty() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("dns failed for {host}: no addresses"),
        ));
    }
    Ok(all)
}

fn host_name_blocked(host: &str) -> bool {
    let host = host.trim().trim_end_matches('.').to_ascii_lowercase();
    host == "localhost" || host == "localhost.localdomain" || host.ends_with(".localhost")
}

pub(crate) fn ip_blocked(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => ipv4_blocked(v4),
        IpAddr::V6(v6) => {
            if let Some(v4) = v6.to_ipv4_mapped() {
                return ipv4_blocked(v4);
            }
            let seg = v6.segments();
            // IPv4-compatible (::a.b.c.d), NAT64 (64:ff9b::/96) and 6to4 (2002::/16)
            // carry an IPv4 address that must pass the same check.
            if seg[..6] == [0, 0, 0, 0, 0, 0] && !v6.is_loopback() && !v6.is_unspecified() {
                return ipv4_blocked(Ipv4Addr::new(
                    (seg[6] >> 8) as u8,
                    seg[6] as u8,
                    (seg[7] >> 8) as u8,
                    seg[7] as u8,
                ));
            }
            if seg[0] == 0x64 && seg[1] == 0xff9b && seg[2..6] == [0, 0, 0, 0] {
                return ipv4_blocked(Ipv4Addr::new(
                    (seg[6] >> 8) as u8,
                    seg[6] as u8,
                    (seg[7] >> 8) as u8,
                    seg[7] as u8,
                ));
            }
            if seg[0] == 0x2002 {
                return ipv4_blocked(Ipv4Addr::new(
                    (seg[1] >> 8) as u8,
                    seg[1] as u8,
                    (seg[2] >> 8) as u8,
                    seg[2] as u8,
                ));
            }
            v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_multicast()
                || v6.is_unique_local()
                || v6.is_unicast_link_local()
                // fec0::/10 site-local (deprecated, still routed on some LANs)
                || (seg[0] & 0xffc0) == 0xfec0
        }
    }
}

fn ipv4_blocked(ip: Ipv4Addr) -> bool {
    let [a, b, c, _] = ip.octets();
    ip.is_loopback()
        || ip.is_private()
        || ip.is_link_local()
        || ip.is_unspecified()
        || ip.is_broadcast()
        || ip.is_multicast()
        // 0.0.0.0/8 "this network"
        || a == 0
        // 100.64.0.0/10 carrier-grade NAT (Tailscale and similar overlays)
        || (a == 100 && (64..=127).contains(&b))
        // 192.0.0.0/24 IETF protocol assignments
        || (a == 192 && b == 0 && c == 0)
        // 198.18.0.0/15 benchmarking
        || (a == 198 && (b == 18 || b == 19))
        // 240.0.0.0/4 reserved
        || a >= 240
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auto_review::{review, ReviewIn};
    use crate::gate::{self, decide_with, Decision, Gate, PermMode};
    use crate::perm::{parse_rule, Action, Policy};
    use crate::{
        CancelToken, ClientError, ModelClient, ResponsesRequest, StreamEvent, TurnOutput, Usage,
    };
    use std::collections::HashMap;
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
    use std::path::Path;
    use std::sync::Mutex;
    use std::time::Duration;

    struct Scripted {
        dns: HashMap<String, Vec<IpAddr>>,
        pages: HashMap<String, FetchResp>,
        gets: Mutex<Vec<String>>,
        lookups: Mutex<Vec<String>>,
    }

    impl Scripted {
        fn new() -> Self {
            Self {
                dns: HashMap::new(),
                pages: HashMap::new(),
                gets: Mutex::new(Vec::new()),
                lookups: Mutex::new(Vec::new()),
            }
        }

        fn page(mut self, url: &str, resp: FetchResp) -> Self {
            self.pages.insert(url.to_string(), resp);
            self
        }

        fn host(mut self, host: &str, ips: Vec<IpAddr>) -> Self {
            self.dns.insert(host.to_string(), ips);
            self
        }
    }

    impl PageFetch for Scripted {
        fn resolve(&self, host: &str, port: u16) -> Result<Vec<IpAddr>, String> {
            self.lookups
                .lock()
                .unwrap_or_else(|err| err.into_inner())
                .push(format!("{host}:{port}"));
            self.dns
                .get(host)
                .cloned()
                .ok_or_else(|| format!("no dns for {host}"))
        }

        fn get(&self, url: &str) -> Result<FetchResp, String> {
            self.gets
                .lock()
                .unwrap_or_else(|err| err.into_inner())
                .push(url.to_string());
            self.pages
                .get(url)
                .cloned()
                .ok_or_else(|| format!("no page {url}"))
        }
    }

    fn html_page(body: &str) -> FetchResp {
        FetchResp {
            status: 200,
            location: None,
            content_type: "text/html; charset=utf-8".into(),
            body: body.as_bytes().to_vec(),
        }
    }

    fn text_page(body: &[u8]) -> FetchResp {
        FetchResp {
            status: 200,
            location: None,
            content_type: "text/plain".into(),
            body: body.to_vec(),
        }
    }

    fn redirect(to: &str) -> FetchResp {
        FetchResp {
            status: 302,
            location: Some(to.to_string()),
            content_type: String::new(),
            body: Vec::new(),
        }
    }

    fn args(url: &str) -> Value {
        json!({"url": url})
    }

    /// The tool with its egress lines in a scratch config folder.
    fn run(args: &Value, fetch: &dyn PageFetch) -> ToolOutput {
        static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = crate::harness::test_dir(&format!("web-fetch-{n}"));
        let _cfg = crate::perm::ConfigGuard::set(&dir);
        let out = super::run(args, fetch, &|| false);
        let _ = std::fs::remove_dir_all(dir);
        out
    }

    #[test]
    fn web_fetch_html_becomes_markdown_and_truncates() {
        let html = r#"<!doctype html><html><head><style>.x{color:red}</style><script>alert(1)</script></head>
<body><h1>Cabin</h1><p>See <a href="https://example.com/docs">docs</a>.</p>
<ul><li>pine</li><li>oak</li></ul><pre><code>fn cabin() {}</code></pre></body></html>"#;
        let fetch = Scripted::new()
            .host("example.com", vec![IpAddr::from([203, 0, 113, 5])])
            .page("https://example.com/a", html_page(html));
        let out = run(&args("http://example.com/a"), &fetch);
        assert!(!out.failed, "{}", out.text);
        assert!(out.text.contains("# Cabin"), "{}", out.text);
        assert!(
            out.text.contains("[docs](https://example.com/docs)"),
            "{}",
            out.text
        );
        assert!(out.text.contains("- pine"), "{}", out.text);
        assert!(out.text.contains("- oak"), "{}", out.text);
        assert!(out.text.contains("fn cabin() {}"), "{}", out.text);
        assert!(!out.text.contains("alert"), "{}", out.text);
        assert!(!out.text.contains("color:red"), "{}", out.text);
        assert_eq!(
            fetch.gets.lock().unwrap().as_slice(),
            ["https://example.com/a"]
        );

        let mut big = vec![b'a'; 200_000];
        big.extend_from_slice(b"MARKER_PAST_CAP");
        let plain = Scripted::new()
            .host("example.com", vec![IpAddr::from([1, 1, 1, 1])])
            .page("https://example.com/plain", text_page(&big));
        let capped = run(&args("https://example.com/plain"), &plain);
        assert!(!capped.failed, "{}", capped.text);
        assert!(
            capped.text.contains("[truncated]"),
            "{}",
            capped.text.chars().rev().take(40).collect::<String>()
        );
        assert!(
            !capped.text.contains("MARKER_PAST_CAP"),
            "markdown cap must drop the tail"
        );
        assert!(
            capped.text.chars().count() < 100_200,
            "{}",
            capped.text.chars().count()
        );

        let mut raw = vec![b'b'; RAW_CAP + 64];
        raw.extend_from_slice(b"MARKER_PAST_RAW");
        let huge = Scripted::new()
            .host("example.com", vec![IpAddr::from([1, 1, 1, 1])])
            .page("https://example.com/raw", text_page(&raw));
        let raw_out = run(&args("https://example.com/raw"), &huge);
        assert!(!raw_out.failed, "{}", raw_out.text);
        assert!(raw_out.text.contains("[truncated]"));
        assert!(!raw_out.text.contains("MARKER_PAST_RAW"));
    }

    #[test]
    fn web_fetch_refuses_ssrf_and_bad_schemes() {
        let fetch = Scripted::new().host("localhost", vec![IpAddr::from([1, 1, 1, 1])]);
        for url in [
            "file:///etc/passwd",
            "ftp://example.com/a",
            "javascript:alert(1)",
            "http://user:pass@example.com/a",
            "http://127.0.0.1/",
            "http://127.0.0.1:9/secret",
            "http://10.1.2.3/",
            "http://192.168.1.9/a",
            "http://172.16.0.1/",
            "http://169.254.1.1/",
            "http://[::1]/",
            "http://[fe80::1]/",
            "http://[fc00::1]/",
            "http://[::ffff:127.0.0.1]/",
            "http://localhost/x",
            "http://localhost.localdomain/x",
            "http://0.0.0.0/",
            "http://224.0.0.1/",
        ] {
            let out = run(&args(url), &fetch);
            assert!(out.failed, "{url} -> {}", out.text);
            assert!(
                fetch.gets.lock().unwrap().is_empty(),
                "{url} was fetched: {:?}",
                fetch.gets.lock().unwrap()
            );
        }
        assert!(
            fetch.lookups.lock().unwrap().is_empty(),
            "localhost must be refused before dns: {:?}",
            fetch.lookups.lock().unwrap()
        );

        let mixed = Scripted::new().host(
            "evil.example",
            vec![IpAddr::from([1, 1, 1, 1]), IpAddr::from([10, 1, 1, 1])],
        );
        let out = run(&args("https://evil.example/a"), &mixed);
        assert!(out.failed, "{}", out.text);
        assert!(out.text.contains("non-public"), "{}", out.text);
        assert!(mixed.gets.lock().unwrap().is_empty());
        assert!(!mixed.lookups.lock().unwrap().is_empty());

        let public_ip = Scripted::new().page("https://1.1.1.1/ok", text_page(b"hi"));
        let out = run(&args("http://1.1.1.1/ok"), &public_ip);
        assert!(!out.failed, "{}", out.text);
        assert_eq!(
            public_ip.gets.lock().unwrap().as_slice(),
            ["https://1.1.1.1/ok"]
        );
        assert!(public_ip.lookups.lock().unwrap().is_empty());

        let named = Scripted::new()
            .host("example.com", vec![IpAddr::from([203, 0, 113, 5])])
            .page("https://example.com/ok", text_page(b"public"));
        let out = run(&args("https://example.com/ok"), &named);
        assert!(!out.failed, "{}", out.text);
        assert!(out.text.contains("public"), "{}", out.text);

        let hop = Scripted::new()
            .host("example.com", vec![IpAddr::from([203, 0, 113, 5])])
            .page(
                "https://example.com/go",
                redirect("http://127.0.0.1/secret"),
            );
        let out = run(&args("https://example.com/go"), &hop);
        assert!(out.failed, "{}", out.text);
        assert_eq!(
            hop.gets.lock().unwrap().as_slice(),
            ["https://example.com/go"]
        );

        let mut chain = Scripted::new().host("example.com", vec![IpAddr::from([203, 0, 113, 5])]);
        for n in 0..12 {
            let here = format!("https://example.com/r{n}");
            let next = format!("https://example.com/r{}", n + 1);
            chain.pages.insert(here, redirect(&next));
        }
        let out = run(&args("https://example.com/r0"), &chain);
        assert!(out.failed, "{}", out.text);
        assert!(out.text.contains("redirect"), "{}", out.text);
        let gets = chain.gets.lock().unwrap().clone();
        assert!(!gets.iter().any(|url| url.ends_with("/r11")), "{gets:?}");
        assert!(gets.iter().any(|url| url.ends_with("/r10")), "{gets:?}");
    }

    fn policy(rules: Vec<crate::perm::Rule>) -> Policy {
        Policy {
            rules,
            grants: Vec::new(),
        }
    }

    fn chat(attended: bool, mode: PermMode, readonly: bool) -> Gate {
        Gate {
            mode,
            readonly_session: readonly,
            attended,
            desktop: false,
        }
    }

    fn judge(gate: &Gate, arguments: &str, rules: &Policy, base: Decision) -> Decision {
        struct AllowJudge;
        impl ModelClient for AllowJudge {
            fn stream(
                &self,
                _req: &ResponsesRequest,
                _cancel: &CancelToken,
                _sink: &mut dyn FnMut(StreamEvent),
            ) -> Result<TurnOutput, ClientError> {
                Ok(TurnOutput {
                    text: r#"{"verdict":"allow","reason":"routine fetch"}"#.into(),
                    reasoning: String::new(),
                    calls: Vec::new(),
                    usage: Usage::default(),
                })
            }
        }
        let client = AllowJudge;
        let cancel = CancelToken::new();
        let halt = || false;
        let workspace = Path::new(".");
        review(&ReviewIn {
            client: &client,
            cancel: &cancel,
            halt: &halt,
            gate,
            name: "web_fetch",
            arguments,
            workspace,
            policy: Some(rules),
            latched_always: false,
            desk: None,
            history: &[],
            conversation_id: "judge",
            base,
            timeout: Duration::from_secs(2),
        })
        .decision
    }

    #[test]
    fn web_fetch_rules_deny_ask_allow_and_unattended() {
        let url = r#"{"url":"https://example.com/a"}"#;
        let ws = Path::new(".");
        let deny = parse_rule("WebFetch(domain:example.com)", Action::Deny).unwrap();
        let allow = parse_rule("WebFetch(domain:example.com)", Action::Allow).unwrap();
        let ask = parse_rule("WebFetch(domain:example.com)", Action::Ask).unwrap();

        let both = policy(vec![allow.clone(), deny.clone()]);
        let decided = decide_with(
            &chat(true, PermMode::Ask, false),
            "web_fetch",
            url,
            false,
            None,
            ws,
            Some(&both),
        );
        assert!(
            matches!(decided, Decision::Refuse(ref text) if text.contains("web_fetch")),
            "{decided:?}"
        );

        let asked = decide_with(
            &chat(true, PermMode::Always, false),
            "web_fetch",
            url,
            false,
            None,
            ws,
            Some(&policy(vec![ask.clone()])),
        );
        assert_eq!(asked, Decision::Ask);
        let asked_away = decide_with(
            &chat(false, PermMode::Ask, false),
            "web_fetch",
            url,
            false,
            None,
            ws,
            Some(&policy(vec![ask.clone()])),
        );
        assert!(matches!(asked_away, Decision::Refuse(_)), "{asked_away:?}");

        let allowed = decide_with(
            &chat(false, PermMode::Ask, false),
            "web_fetch",
            url,
            false,
            None,
            ws,
            Some(&policy(vec![allow.clone()])),
        );
        assert_eq!(allowed, Decision::Run);

        let open = decide_with(
            &chat(true, PermMode::Ask, false),
            "web_fetch",
            url,
            false,
            None,
            ws,
            Some(&policy(Vec::new())),
        );
        assert_eq!(open, Decision::Ask);
        let away = decide_with(
            &chat(false, PermMode::Ask, false),
            "web_fetch",
            url,
            false,
            None,
            ws,
            Some(&policy(Vec::new())),
        );
        assert!(matches!(away, Decision::Refuse(_)), "{away:?}");

        let plan = decide_with(
            &chat(true, PermMode::Always, true),
            "web_fetch",
            url,
            false,
            None,
            ws,
            Some(&policy(vec![allow.clone()])),
        );
        match plan {
            Decision::Refuse(text) => assert!(text.contains(gate::READ_ONLY_PHASE), "{text}"),
            other => panic!("plan mode must stay read-only: {other:?}"),
        }

        let auto = chat(true, PermMode::Auto, false);
        let base = decide_with(
            &auto,
            "web_fetch",
            url,
            false,
            None,
            ws,
            Some(&policy(Vec::new())),
        );
        assert_eq!(base, Decision::Ask);
        assert_eq!(judge(&auto, url, &policy(Vec::new()), base), Decision::Run);

        let explicit = policy(vec![ask]);
        let base = decide_with(&auto, "web_fetch", url, false, None, ws, Some(&explicit));
        assert_eq!(base, Decision::Ask);
        assert_eq!(judge(&auto, url, &explicit, base), Decision::Ask);

        let away_auto = chat(false, PermMode::Auto, false);
        let base = decide_with(
            &away_auto,
            "web_fetch",
            url,
            false,
            None,
            ws,
            Some(&explicit),
        );
        assert!(matches!(base, Decision::Refuse(_)), "{base:?}");
        assert!(matches!(
            judge(&away_auto, url, &explicit, base.clone()),
            Decision::Refuse(_)
        ));

        let names = |gate: Gate| -> Vec<String> {
            crate::schemas_for(&gate)
                .iter()
                .filter_map(|tool| {
                    tool.get("name")
                        .and_then(|n| n.as_str())
                        .map(str::to_string)
                })
                .collect()
        };
        let plan_names = names(Gate::phase_readonly());
        assert!(!plan_names.iter().any(|n| n == "web_fetch"));
        assert!(!plan_names.iter().any(|n| n == "image_generate"));
        let live = names(chat(true, PermMode::Ask, false));
        for name in [
            "web_fetch",
            "image_generate",
            "image_edit",
            "video_generate",
            "video_edit",
            "video_extend",
        ] {
            assert!(
                live.iter().any(|n| n == name),
                "{name} missing from {live:?}"
            );
        }
    }

    #[test]
    fn blocked_ranges_cover_mapped_and_ula() {
        assert!(ip_blocked(IpAddr::from([127, 0, 0, 2])));
        assert!(ip_blocked(IpAddr::from([10, 0, 0, 1])));
        assert!(ip_blocked(IpAddr::from([192, 168, 0, 1])));
        assert!(ip_blocked(IpAddr::from([172, 16, 5, 1])));
        assert!(ip_blocked(IpAddr::from([169, 254, 169, 254])));
        assert!(!ip_blocked(IpAddr::from([203, 0, 113, 5])));
        assert!(!ip_blocked(IpAddr::from([1, 1, 1, 1])));
        assert!(ip_blocked(IpAddr::V6(Ipv6Addr::LOCALHOST)));
        assert!(ip_blocked(IpAddr::V6(Ipv6Addr::new(
            0xfe80, 0, 0, 0, 0, 0, 0, 1
        ))));
        assert!(ip_blocked(IpAddr::V6(Ipv6Addr::new(
            0xfc00, 0, 0, 0, 0, 0, 0, 1
        ))));
        assert!(ip_blocked(IpAddr::V6(
            Ipv4Addr::new(127, 0, 0, 1).to_ipv6_mapped()
        )));
        assert!(!ip_blocked(IpAddr::V6(
            Ipv4Addr::new(1, 1, 1, 1).to_ipv6_mapped()
        )));
    }

    #[test]
    fn blocked_ranges_cover_cgnat_reserved_and_embedded_v4() {
        assert!(ip_blocked(IpAddr::from([100, 64, 0, 1])));
        assert!(ip_blocked(IpAddr::from([100, 101, 7, 9])));
        assert!(!ip_blocked(IpAddr::from([100, 128, 0, 1])));
        assert!(ip_blocked(IpAddr::from([0, 1, 2, 3])));
        assert!(ip_blocked(IpAddr::from([192, 0, 0, 8])));
        assert!(ip_blocked(IpAddr::from([198, 18, 0, 1])));
        assert!(ip_blocked(IpAddr::from([240, 0, 0, 1])));
        // ::127.0.0.1, 64:ff9b::192.168.1.1, 2002:c0a8:0101:: and fec0::1
        assert!(ip_blocked(IpAddr::V6(Ipv6Addr::new(
            0, 0, 0, 0, 0, 0, 0x7f00, 1
        ))));
        assert!(ip_blocked(IpAddr::V6(Ipv6Addr::new(
            0x64, 0xff9b, 0, 0, 0, 0, 0xc0a8, 0x0101
        ))));
        assert!(ip_blocked(IpAddr::V6(Ipv6Addr::new(
            0x2002, 0xc0a8, 0x0101, 0, 0, 0, 0, 1
        ))));
        assert!(ip_blocked(IpAddr::V6(Ipv6Addr::new(
            0xfec0, 0, 0, 0, 0, 0, 0, 1
        ))));
        assert!(!ip_blocked(IpAddr::V6(Ipv6Addr::new(
            0x2002, 0x0101, 0x0101, 0, 0, 0, 0, 1
        ))));
    }

    /// The resolver ureq dials with refuses non-public answers, so DNS rebinding
    /// between the check and the connect can't reach the LAN. Literals only: no DNS.
    #[test]
    fn dial_resolver_refuses_non_public_addresses() {
        assert!(public_only_resolve("localhost:443").is_err());
        assert!(public_only_resolve("127.0.0.1:443").is_err());
        assert!(public_only_resolve("169.254.169.254:80").is_err());
        assert!(public_only_resolve("10.1.2.3:443").is_err());
        assert!(public_only_resolve("[::1]:443").is_err());
        assert!(public_only_resolve("[fd00::1]:443").is_err());
        assert_eq!(
            public_only_resolve("1.1.1.1:443").unwrap(),
            vec!["1.1.1.1:443".parse().unwrap()]
        );
    }

    const MEMORY: &str = "the harbor ferry leaves at nine from pier four";

    fn gets(fetch: &Scripted) -> usize {
        fetch.gets.lock().unwrap_or_else(|err| err.into_inner()).len()
    }

    /// Spike-4c: a URL that carries a recall-pack line is personal. To a host
    /// with no grant it parks a hard Send: Deny fetches nothing, Approve once
    /// fetches exactly once and logs one `approved_once` line.
    #[test]
    fn web_fetch_memory_in_the_url_parks_and_fetches_nothing_until_approved() {
        let dir = crate::harness::test_dir("web-fetch-park");
        let _cfg = crate::perm::ConfigGuard::set(&dir);
        let _recall = crate::harness::RecallScope::enter(vec![MEMORY.to_string()]);
        let url = "https://example.org/search?q=the%20harbor%20ferry%20leaves%20at%20nine%20from%20pier%20four";
        let fetch = Scripted::new()
            .host("example.org", vec![IpAddr::from([203, 0, 113, 9])])
            .page(url, text_page(b"ferry times"));
        let deny = crate::harness::answer_next_park(dir.clone(), false);
        let out = super::run(&args(url), &fetch, &|| false);
        let card = deny.join().unwrap().expect("hard card");
        assert_eq!(card.action, "web_fetch → example.org (chats, memory)");
        assert_eq!(card.class, "send");
        assert!(out.failed && out.text.contains("nothing was sent"), "{}", out.text);
        assert_eq!(gets(&fetch), 0, "a denied card fetches nothing");
        assert!(crate::harness::read_egress(&dir).is_empty());

        let approve = crate::harness::answer_next_park(dir.clone(), true);
        let out = super::run(&args(url), &fetch, &|| false);
        assert!(approve.join().unwrap().is_some());
        assert!(!out.failed, "{}", out.text);
        assert_eq!(gets(&fetch), 1, "approve once fetches once");
        let log = crate::harness::read_egress(&dir);
        assert_eq!(log.len(), 1, "{log:?}");
        assert_eq!((log[0].dest.as_str(), log[0].basis.as_str()), ("example.org", "approved_once"));

        // A halt (the turn's stop) denies a waiting card: still nothing fetched.
        let out = super::run(&args(url), &fetch, &|| true);
        assert!(out.failed, "{}", out.text);
        assert_eq!(gets(&fetch), 1);
        assert!(crate::harness::pending_parks(&dir).is_empty(), "the park file is gone");
        let _ = std::fs::remove_dir_all(dir);
    }

    /// A model-picked URL with no memory is chat: it goes, with one line that
    /// names the host only (no path, query, or user info). A redirect to a new
    /// host is its own line.
    #[test]
    fn web_fetch_chat_url_goes_with_a_host_only_line() {
        let dir = crate::harness::test_dir("web-fetch-chat");
        let _cfg = crate::perm::ConfigGuard::set(&dir);
        let fetch = Scripted::new()
            .host("example.com", vec![IpAddr::from([203, 0, 113, 5])])
            .host("docs.example.net", vec![IpAddr::from([203, 0, 113, 6])])
            .page("https://example.com/go?token=sk-abcdefghijklmnopqrstuv", redirect("https://docs.example.net/page"))
            .page("https://docs.example.net/page", text_page(b"landed"));
        let out = super::run(&args("https://example.com/go?token=sk-abcdefghijklmnopqrstuv"), &fetch, &|| false);
        assert!(!out.failed, "{}", out.text);
        assert_eq!(gets(&fetch), 2);
        let log = crate::harness::read_egress(&dir);
        let rows: Vec<(&str, &str)> = log.iter().map(|l| (l.dest.as_str(), l.basis.as_str())).collect();
        assert_eq!(rows, vec![("example.com", "chat"), ("docs.example.net", "chat")]);
        let raw = std::fs::read_to_string(crate::harness::egress_path(&dir)).unwrap();
        let opened = format!("{log:?}");
        for text in [&raw, &opened] {
            assert!(!text.contains("sk-abc") && !text.contains("/go") && !text.contains("landed"), "{text}");
        }
        let _ = std::fs::remove_dir_all(dir);
    }
}
