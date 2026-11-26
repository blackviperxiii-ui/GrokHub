// Portions derived from xai-org/grok-build (Apache-2.0, © SpaceXAI), commit 2bdd1d6a; modified.

//! Glob, prefix, path, and domain matching.
//! `*` does not cross `/` on paths. `**` does. `..` is collapsed before the match.

use std::path::Path;

use globset::GlobBuilder;

use crate::perm::rules::{PatMode, Rule, Tool};

pub fn normalize_domain(raw: &str) -> String {
    let s = raw.trim().trim_end_matches('/').trim_end_matches('.');
    let s = s.strip_prefix("www.").unwrap_or(s);
    s.to_lowercase()
}

pub fn domain_matches(pattern: &str, url: &str) -> bool {
    host_of(url).is_some_and(|host| domain_hit(pattern, &host))
}

/// Whether `host` is the domain `pattern` names or inside it. An IPv4
/// address counts in any spelling (`domain:127.0.0.1` names `2130706433`).
pub(crate) fn domain_hit(pattern: &str, host: &str) -> bool {
    let canon = |text: &str| {
        let text = normalize_domain(text);
        crate::tools::dial_host(&text).unwrap_or(text)
    };
    let (domain, pattern) = (canon(host), canon(pattern));
    if pattern.is_empty() {
        return false;
    }
    domain == pattern || domain.ends_with(&format!(".{pattern}"))
}

/// Where a fetch goes, for a URL rule to read: the scheme, host, port and
/// path.
pub(crate) struct Target {
    scheme: Option<String>,
    pub(crate) host: String,
    /// `None` when the URL's port is no number.
    port: Option<u16>,
    path: String,
}

impl Target {
    /// The fetch web_fetch makes ([`crate::tools::parse_public_url`]): always
    /// https, on its port (443 unless the URL names another).
    pub(crate) fn dialed(url: &str) -> Option<Self> {
        let parts = crate::tools::parse_public_url(url).ok()?;
        Some(Self {
            scheme: Some("https".into()),
            port: Some(parts.dns_port()),
            host: parts.host,
            path: parts.path,
        })
    }

    /// The URL as a browser reads it ([`UrlParts`]), for deny and ask rules
    /// to widen with: the scheme as written, the port as its number (443
    /// unless written).
    pub(crate) fn read(url: &str) -> Option<Self> {
        let parts = url_parts(url);
        let host = host_of(url)?;
        let port = match authority_port(&parts.authority) {
            None => Some(443),
            Some(port) => port_number(port),
        };
        let path = if parts.path.is_empty() || parts.path.starts_with('?') {
            format!("/{}", parts.path)
        } else {
            parts.path
        };
        Some(Self {
            scheme: parts.scheme,
            host: host.trim_end_matches('.').to_lowercase(),
            port,
            path: resolve_dots(&path),
        })
    }
}

/// The port an authority names after its host, as written.
fn authority_port(authority: &str) -> Option<&str> {
    let authority = authority.trim();
    let after = match authority.strip_prefix('[') {
        Some(inner) => inner.split_once(']').map_or("", |(_, after)| after),
        None => authority.split_once(':').map_or("", |(_, port)| port),
    };
    let port = after.strip_prefix(':').unwrap_or(after).trim();
    (!port.is_empty()).then_some(port)
}

/// A port written in digits as its number: `08443` is 8443.
fn port_number(text: &str) -> Option<u16> {
    let text = text.trim();
    if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    match text.trim_start_matches('0') {
        "" => Some(0),
        digits => digits.parse().ok(),
    }
}

/// A URL split the way a browser reads it (the WHATWG URL parser) and so the
/// way a fetch dials it: tabs and newlines dropped, the scheme and its
/// slashes (or backslashes) skipped, the authority ending at the first `/`,
/// `\`, `?` or `#`, userinfo up to the last `@` dropped. `https://a.test?.b.test`
/// dials `a.test`.
struct UrlParts {
    /// Lowercase, `None` when there is none.
    scheme: Option<String>,
    /// `host[:port]` as written, without userinfo.
    authority: String,
    /// From the first `/`, `\` or `?` (`\` read as `/`), without the fragment.
    path: String,
}

fn url_parts(url: &str) -> UrlParts {
    let url: String = url
        .trim()
        .chars()
        .filter(|c| !matches!(c, '\t' | '\n' | '\r'))
        .collect();
    let (scheme, rest) = split_scheme(&url);
    let end = rest.find(['/', '\\', '?', '#']).unwrap_or(rest.len());
    let authority = &rest[..end];
    let authority = authority.rsplit('@').next().unwrap_or(authority);
    let tail = &rest[end..];
    let tail = tail.split('#').next().unwrap_or("");
    UrlParts {
        scheme,
        authority: authority.to_string(),
        path: tail.replace('\\', "/"),
    }
}

/// The scheme and what follows its slashes. A `scheme://` always counts; a
/// web scheme counts with any slashes (`https:evil.test`), as browsers read
/// it. `github.com:443/x` has no scheme.
fn split_scheme(url: &str) -> (Option<String>, &str) {
    let Some(colon) = url.find(':') else {
        return (None, url);
    };
    let scheme = &url[..colon];
    let valid = scheme.starts_with(|c: char| c.is_ascii_alphabetic())
        && scheme
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'));
    let rest = &url[colon + 1..];
    let slashed = rest.starts_with(['/', '\\']) && rest[1..].starts_with(['/', '\\']);
    let web = matches!(
        scheme.to_ascii_lowercase().as_str(),
        "http" | "https" | "ws" | "wss"
    );
    if valid && (slashed || web) {
        let rest = rest.trim_start_matches(['/', '\\']);
        (Some(scheme.to_ascii_lowercase()), rest)
    } else {
        (None, url)
    }
}

/// The host a fetch of `url` dials (see [`UrlParts`]): no port, IPv6 without
/// brackets, `%` escapes decoded. `None` without a host.
fn host_of(url: &str) -> Option<String> {
    let host = host_part(&url_parts(url).authority);
    (!host.is_empty()).then_some(host)
}

/// The host of an authority (`[::1]:80` gives `::1`), `%` escapes decoded,
/// blanks and stray brackets around it trimmed (`evil.test]` gives
/// `evil.test`), and an IPv4 address as its four decimal parts, the way
/// web_fetch reads it ([`crate::tools::dial_host`]).
fn host_part(authority: &str) -> String {
    let authority = authority.trim();
    let host = match authority.strip_prefix('[') {
        Some(inner) => inner.split(']').next().unwrap_or(""),
        None => authority.split(':').next().unwrap_or(""),
    };
    let host = percent_decode(host);
    let host = host.trim().trim_matches(['[', ']']).trim();
    crate::tools::dial_host(host).unwrap_or_else(|_| host.to_string())
}

/// The scheme a fetch dials: web_fetch sends an `http` URL as `https`.
fn dialed_scheme(scheme: &str) -> &str {
    if scheme == "http" {
        "https"
    } else {
        scheme
    }
}

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = bytes
            .get(i + 1..i + 3)
            .and_then(|h| std::str::from_utf8(h).ok())
            .and_then(|h| u8::from_str_radix(h, 16).ok());
        match (bytes[i], hex) {
            (b'%', Some(b)) => {
                out.push(b);
                i += 3;
            }
            (b, _) => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// A URL glob (`*.github.com`, `https://*.github.com/*`, `docs.rs/serde/*`)
/// against where a fetch goes: the pattern's scheme against the scheme
/// (`http` is sent as `https`), its host part against the host, its port
/// against the port as numbers, the rest against the path. So a `*` in the
/// host part never reaches past the host. A pattern with no port names
/// port 443, or any port when `wide` (a deny or ask rule, which only
/// widens).
pub(crate) fn url_glob_hit(pattern: &str, got: &Target, wide: bool) -> bool {
    let want = url_parts(pattern.trim());
    let scheme_hit = want.scheme.as_deref().is_none_or(|scheme| {
        got.scheme
            .as_deref()
            .is_some_and(|have| glob_match(dialed_scheme(scheme), dialed_scheme(have), false))
    });
    if !scheme_hit {
        return false;
    }
    let want_host = want.authority.trim().to_lowercase();
    let (name, port) = match want_host.strip_prefix('[') {
        Some(inner) => inner
            .split_once(']')
            .map_or((inner, ""), |(name, after)| (name, after)),
        None => want_host
            .split_once(':')
            .map_or((&*want_host, ""), |(n, p)| (n, p)),
    };
    let port = port.strip_prefix(':').unwrap_or(port).trim();
    let name = name.trim().trim_end_matches('.');
    let name = crate::tools::dial_host(name).unwrap_or_else(|_| name.to_string());
    if !glob_match(&name, &got.host, false) {
        return false;
    }
    let port_hit = match (port, got.port) {
        ("", Some(have)) => wide || have == 443,
        ("", None) => wide,
        (want, Some(have)) => match port_number(want) {
            Some(want) => want == have,
            None => glob_match(want, &have.to_string(), false),
        },
        (_, None) => wide,
    };
    if !port_hit {
        return false;
    }
    want.path.is_empty() || glob_match(&want.path, &resolve_dots(&got.path), false)
}

/// `path` with its `.` and `..` segments (`%2e` too) resolved, as a fetch
/// sends it: `/serde/../tokio/x` asks for `/tokio/x`. The query stays.
fn resolve_dots(path: &str) -> String {
    let (path, query) = path.split_at(path.find('?').unwrap_or(path.len()));
    let segments: Vec<&str> = path.split('/').skip(1).collect();
    let mut out: Vec<&str> = Vec::new();
    for (i, seg) in segments.iter().enumerate() {
        let last = i + 1 == segments.len();
        match seg.to_ascii_lowercase().replace("%2e", ".").as_str() {
            "." if last => out.push(""),
            "." => {}
            ".." => {
                out.pop();
                if last {
                    out.push("");
                }
            }
            _ => out.push(seg),
        }
    }
    format!("/{}{query}", out.join("/"))
}

pub fn glob_match(pattern: &str, text: &str, path_mode: bool) -> bool {
    let Ok(glob) = GlobBuilder::new(pattern)
        .literal_separator(path_mode)
        .backslash_escape(true)
        .build()
    else {
        return false;
    };
    glob.compile_matcher().is_match(text)
}

/// Word-boundary prefix: `git` matches `git` and `git status`, not `gitleaks`.
pub fn word_prefix(command: &str, pattern: &str) -> bool {
    if pattern.is_empty() {
        return false;
    }
    command == pattern
        || (command.starts_with(pattern) && command.as_bytes().get(pattern.len()) == Some(&b' '))
}

pub fn bash_matches(command: &str, pattern: &str, wide_prefix: bool) -> bool {
    let command = command.trim_start();
    let pattern = pattern.trim();
    if pattern.is_empty() {
        return false;
    }
    if pattern == "*" {
        return true;
    }
    if word_prefix(command, pattern) || glob_match(pattern, command, false) {
        return true;
    }
    wide_prefix && command.starts_with(pattern)
}

pub fn rule_matches_text(rule: &Rule, text: &str, wide_prefix: bool) -> bool {
    match rule.pattern.as_deref() {
        None => true,
        Some("*") => true,
        Some(pattern) => bash_matches(text, pattern, wide_prefix),
    }
}

/// `None` when `raw` leaves the workspace after `.` and `..` collapse, or starts with `~`.
pub fn path_forms(workspace: &Path, raw: &str) -> Option<Vec<String>> {
    let raw = strip_verbatim(&raw.trim().replace('\\', "/"));
    if raw.is_empty() || raw == "." {
        let abs = workspace_string(workspace);
        return Some(vec![abs, ".".into(), "./".into()]);
    }
    if raw.starts_with('~') {
        return None;
    }
    let ws = workspace_string(workspace);
    if is_absolute(&raw) {
        let abs = lexical_absolute(&raw)?;
        let ws_abs = lexical_absolute(&ws)?;
        return forms_if_inside(&ws_abs, &abs);
    }
    let (abs, rel) = collapse_relative(&ws, &raw)?;
    if rel.is_empty() {
        Some(vec![abs, ".".into(), "./".into()])
    } else {
        Some(vec![abs, format!("./{rel}"), rel])
    }
}

fn has_drive_prefix(path: &str) -> bool {
    let bytes = path.as_bytes();
    bytes.len() >= 2 && bytes[1] == b':' && bytes[0].is_ascii_alphabetic()
}

fn strip_verbatim(path: &str) -> String {
    if let Some(rest) = path.strip_prefix("//?/UNC/") {
        format!("//{rest}")
    } else if let Some(rest) = path.strip_prefix("//?/") {
        rest.to_string()
    } else {
        path.to_string()
    }
}

fn forms_if_inside(ws_abs: &str, abs: &str) -> Option<Vec<String>> {
    let trimmed = ws_abs.trim_end_matches('/');
    let ws = if trimmed.is_empty() { "/" } else { trimmed };
    let windows_drive = has_drive_prefix(ws) && has_drive_prefix(abs);
    let inside = if ws == "/" {
        abs.starts_with('/')
    } else if windows_drive {
        let ws_low = ws.to_ascii_lowercase();
        let abs_low = abs.to_ascii_lowercase();
        abs_low == ws_low || abs_low.starts_with(&format!("{ws_low}/"))
    } else {
        abs == ws || abs.starts_with(&format!("{ws}/"))
    };
    if !inside {
        return None;
    }
    let rel = if ws == "/" {
        abs.trim_start_matches('/').to_string()
    } else if windows_drive {
        abs.get(ws.len()..)
            .unwrap_or("")
            .trim_start_matches('/')
            .to_string()
    } else {
        abs.strip_prefix(ws)
            .unwrap_or("")
            .trim_start_matches('/')
            .to_string()
    };
    if rel.is_empty() {
        Some(vec![abs.to_string(), ".".into(), "./".into()])
    } else {
        Some(vec![abs.to_string(), rel.clone(), format!("./{rel}")])
    }
}

/// Collapse `raw` against `ws`. `..` that climbs out of `ws` is an escape.
fn collapse_relative(ws: &str, raw: &str) -> Option<(String, String)> {
    let (drive, ws_rest, ws_abs) = split_root(ws);
    let mut base = Vec::new();
    for comp in ws_rest.split('/') {
        if comp.is_empty() || comp == "." {
            continue;
        }
        if comp == ".." {
            base.pop()?;
            continue;
        }
        base.push(comp.to_string());
    }
    let base_len = base.len();
    let mut stack = base;
    for comp in raw.split('/') {
        if comp.is_empty() || comp == "." {
            continue;
        }
        if comp == ".." {
            if stack.len() <= base_len {
                return None;
            }
            stack.pop();
            continue;
        }
        stack.push(comp.to_string());
    }
    let rel = stack[base_len..].join("/");
    let body = stack.join("/");
    let abs = if let Some(drive) = drive {
        if body.is_empty() {
            drive
        } else {
            format!("{drive}/{body}")
        }
    } else if ws_abs {
        if body.is_empty() {
            "/".to_string()
        } else {
            format!("/{body}")
        }
    } else {
        body
    };
    Some((abs, rel))
}

pub fn path_escapes(workspace: &Path, raw: &str) -> bool {
    let raw = raw.trim();
    if raw.is_empty() || raw == "." {
        return false;
    }
    path_forms(workspace, raw).is_none()
}

pub fn path_rule_matches(rule: &Rule, workspace: &Path, raw: &str) -> bool {
    if path_escapes(workspace, raw) {
        return false;
    }
    let Some(pattern) = rule.pattern.as_deref() else {
        return true;
    };
    if pattern.is_empty() || pattern == "*" {
        return true;
    }
    let Some(forms) = path_forms(workspace, raw) else {
        return false;
    };
    if rule.mode == PatMode::Domain {
        return false;
    }
    forms.iter().any(|form| glob_match(pattern, form, true))
}

pub fn tool_reaches(rule_tool: Tool, kind: Tool) -> bool {
    match rule_tool {
        Tool::Any => true,
        Tool::Read => matches!(kind, Tool::Read | Tool::Grep),
        other => other == kind,
    }
}

fn workspace_string(workspace: &Path) -> String {
    strip_verbatim(&workspace.to_string_lossy().replace('\\', "/"))
}

fn is_absolute(path: &str) -> bool {
    path.starts_with('/')
        || (path.len() >= 3
            && path.as_bytes().get(1) == Some(&b':')
            && path
                .as_bytes()
                .first()
                .is_some_and(|b| b.is_ascii_alphabetic())
            && matches!(path.as_bytes().get(2), Some(b'/' | b'\\')))
}

fn lexical_absolute(path: &str) -> Option<String> {
    let path = path.replace('\\', "/");
    let (drive, rest, absolute) = split_root(&path);
    if !absolute && drive.is_none() {
        return None;
    }
    let mut stack = Vec::new();
    for comp in rest.split('/') {
        if comp.is_empty() || comp == "." {
            continue;
        }
        if comp == ".." {
            if stack.pop().is_none() && drive.is_none() {
                continue;
            }
            continue;
        }
        stack.push(comp);
    }
    let body = stack.join("/");
    let prefix = if let Some(drive) = drive {
        format!("{drive}/")
    } else {
        "/".to_string()
    };
    if body.is_empty() {
        Some(prefix.trim_end_matches('/').to_string())
    } else {
        Some(format!("{prefix}{body}"))
    }
}

fn split_root(path: &str) -> (Option<String>, &str, bool) {
    let bytes = path.as_bytes();
    if bytes.len() >= 2 && bytes[1] == b':' && bytes[0].is_ascii_alphabetic() {
        let drive = path[..2].to_string();
        let rest = path[2..].trim_start_matches('/');
        return (Some(drive), rest, true);
    }
    if let Some(rest) = path.strip_prefix('/') {
        return (None, rest, true);
    }
    (None, path, false)
}
