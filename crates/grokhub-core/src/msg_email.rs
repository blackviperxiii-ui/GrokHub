//! Messaging channels, part 9: email, without the network. The app reads the
//! person's own mailbox over IMAP and answers over SMTP, both on implicit TLS
//! (993 and 465), with an app password in the keychain.
//!
//! Mail is easy to forge, so a message reaches the model only when two things
//! hold. Its sender is an address the person added in the app, and the
//! person's own mail server vouched for that sender: the topmost
//! `Authentication-Results` header, written by the server id pinned in the
//! setup, shows DMARC passing for the From domain, or DKIM or SPF passing
//! for a domain aligned with it. Servers add their header above any already
//! there, so a header a sender wrote into their own message sits lower down
//! and is never read. Everything else is dropped without an answer, so a
//! forged sender gets no reply and nobody gets backscatter.
//!
//! This part parses and writes messages. The IMAP and SMTP clients are in
//! `grokhub-agent`'s `channels_net`.

/// The IMAP port with TLS from the first byte.
pub const IMAP_PORT: u16 = 993;
/// The SMTP submission port with TLS from the first byte.
pub const SMTP_PORT: u16 = 465;
/// Longest text passed on or sent, in characters.
pub const MAX_TEXT: usize = 12_000;
/// How deep nested `multipart` parts are followed.
const MAX_DEPTH: usize = 6;

/// The mailbox and the servers it is reached through, as saved. The app
/// password is kept apart from this, under its own keychain entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    /// The person's address, lowercase.
    pub address: String,
    pub imap_host: String,
    pub smtp_host: String,
    /// The `authserv-id` the person's own server writes at the start of its
    /// `Authentication-Results` header, like `mx.google.com`.
    pub auth_server: String,
}

/// Servers for mailboxes the app knows, so the person types only their
/// address: (domains, IMAP host, SMTP host, authserv-id).
const KNOWN: &[(&[&str], &str, &str, &str)] = &[(
    &["gmail.com", "googlemail.com"],
    "imap.gmail.com",
    "smtp.gmail.com",
    "mx.google.com",
)];

impl Config {
    /// What the keychain keeps: the address, then the three servers when
    /// they aren't the known ones for its domain.
    pub fn saved(&self) -> String {
        match known(&self.address) {
            Some((imap, smtp, auth))
                if imap == self.imap_host && smtp == self.smtp_host && auth == self.auth_server =>
            {
                self.address.clone()
            }
            _ => format!(
                "{} {} {} {}",
                self.address, self.imap_host, self.smtp_host, self.auth_server
            ),
        }
    }

    /// The From domain this mailbox sends as.
    pub fn domain(&self) -> &str {
        domain_of(&self.address).unwrap_or("")
    }
}

fn known(address: &str) -> Option<(&'static str, &'static str, &'static str)> {
    let domain = domain_of(address)?;
    KNOWN
        .iter()
        .find(|(domains, ..)| domains.contains(&domain))
        .map(|&(_, imap, smtp, auth)| (imap, smtp, auth))
}

/// The setup text: an address alone for a known provider, or the address,
/// the IMAP host, the SMTP host and the server id, separated by spaces.
pub fn parse_config(text: &str) -> Result<Config, String> {
    let parts: Vec<&str> = text.split_whitespace().collect();
    let address = parts
        .first()
        .map(|a| a.to_ascii_lowercase())
        .filter(|a| valid_address(a))
        .ok_or("That isn't an email address.")?;
    let (imap_host, smtp_host, auth_server) = match parts.len() {
        1 => known(&address)
            .map(|(i, s, a)| (i.to_string(), s.to_string(), a.to_string()))
            .ok_or(
                "For this provider, type the address, its IMAP server, its SMTP server and the server name its Authentication-Results header starts with, separated by spaces.",
            )?,
        4 => (
            host(parts[1])?,
            host(parts[2])?,
            host(parts[3])?,
        ),
        _ => {
            return Err(
                "Type the address alone, or the address, the IMAP server, the SMTP server and the Authentication-Results server name.".into(),
            );
        }
    };
    Ok(Config {
        address,
        imap_host,
        smtp_host,
        auth_server,
    })
}

/// A server name: letters, digits, dots and dashes, with a dot, and no
/// scheme, port or path. Loopback and bare addresses are refused here,
/// because mail is only fetched from a real provider.
fn host(text: &str) -> Result<String, String> {
    let h = text.trim().trim_end_matches('.').to_ascii_lowercase();
    let labels_ok = h.split('.').all(|l| {
        !l.is_empty()
            && l.len() <= 63
            && !l.starts_with('-')
            && !l.ends_with('-')
            && l.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
    });
    let numeric = h.split('.').all(|l| l.chars().all(|c| c.is_ascii_digit()));
    if h.len() > 253 || !h.contains('.') || !labels_ok || numeric || h == "localhost" {
        return Err(format!("{text} isn't a mail server name."));
    }
    Ok(h)
}

/// One `local@domain` with a dotted domain and no spaces, brackets or
/// quotes. Stricter than RFC 5322 on purpose: an address that needs quoting
/// isn't one the app will answer.
pub fn valid_address(address: &str) -> bool {
    let Some((local, domain)) = address.rsplit_once('@') else {
        return false;
    };
    let local_ok = !local.is_empty()
        && local.len() <= 64
        && !local.starts_with('.')
        && !local.ends_with('.')
        && !local.contains("..")
        && local
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "!#$%&'*+-/=?^_`{|}~.".contains(c));
    local_ok && host(domain).is_ok()
}

/// The domain of a valid address.
pub fn domain_of(address: &str) -> Option<&str> {
    address.rsplit_once('@').map(|(_, d)| d)
}

/// A parsed message: its headers in order, unfolded, and its raw body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Message {
    /// Every value of one header, top first. Names match without case.
    pub fn all(&self, name: &str) -> impl Iterator<Item = &str> {
        self.headers
            .iter()
            .filter(move |(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    /// The topmost value of one header.
    pub fn first(&self, name: &str) -> Option<&str> {
        self.all(name).next()
    }
}

/// Split a message into headers and body. Folded lines are joined, and
/// bytes that aren't UTF-8 in a header are read as Latin-1.
pub fn parse_message(raw: &[u8]) -> Message {
    let (head, body) = split_head(raw);
    let mut headers: Vec<(String, String)> = Vec::new();
    for line in text_of(head, "").split('\n') {
        let line = line.strip_suffix('\r').unwrap_or(line);
        if line.starts_with([' ', '\t']) {
            if let Some((_, value)) = headers.last_mut() {
                value.push(' ');
                value.push_str(line.trim());
            }
        } else if let Some((name, value)) = line.split_once(':') {
            let name = name.trim();
            if !name.is_empty() && !name.contains(' ') {
                headers.push((name.to_string(), value.trim().to_string()));
            }
        }
    }
    Message {
        headers,
        body: body.to_vec(),
    }
}

/// The header block and the body, split at the first empty line.
fn split_head(raw: &[u8]) -> (&[u8], &[u8]) {
    for i in 0..raw.len() {
        if raw[i..].starts_with(b"\r\n\r\n") {
            return (&raw[..i], &raw[i + 4..]);
        }
        if raw[i..].starts_with(b"\n\n") {
            return (&raw[..i], &raw[i + 2..]);
        }
    }
    (raw, &[])
}

/// The one sender address in `From`, lowercase. `None` when there are two
/// `From` headers, two addresses in one, or no valid address: each is a way
/// to show one sender and be checked as another.
pub fn sender(msg: &Message) -> Option<String> {
    let mut from = msg.all("From");
    let value = from.next()?;
    if from.next().is_some() {
        return None;
    }
    let value = decode_words(value);
    let address = match (value.rfind('<'), value.rfind('>')) {
        (Some(open), Some(close)) if open < close => {
            if value[..open].contains('<') || value.contains(',') && !quoted_comma(&value, open) {
                return None;
            }
            &value[open + 1..close]
        }
        (None, None) if !value.contains(',') => value.as_str(),
        _ => return None,
    };
    let address = address.trim().to_ascii_lowercase();
    valid_address(&address).then_some(address)
}

/// Whether every comma before the angle bracket at `open` sits inside a
/// quoted display name, like `"Doe, Jane" <jane@example.com>`.
fn quoted_comma(value: &str, open: usize) -> bool {
    let name = &value[..open];
    let mut quoted = false;
    for c in name.chars() {
        match c {
            '"' => quoted = !quoted,
            ',' if !quoted => return false,
            _ => {}
        }
    }
    !value[open..].contains(',')
}

/// Whether the person's own server vouched for the sender. Only the topmost
/// `Authentication-Results` header counts, and only when it starts with the
/// pinned server id. It passes on `dmarc=pass` for the From domain, or on
/// `dkim=pass` or `spf=pass` for a domain aligned with it.
pub fn authenticated(msg: &Message, auth_server: &str, from_domain: &str) -> bool {
    let Some(header) = msg.first("Authentication-Results") else {
        return false;
    };
    let header = strip_comments(header);
    let mut clauses = header.split(';');
    let id = clauses.next().unwrap_or("");
    // The id may carry a version number after it: `mx.example.com 1`.
    let id = id.split_whitespace().next().unwrap_or("");
    if !id.trim_end_matches('.').eq_ignore_ascii_case(auth_server) {
        return false;
    }
    clauses.any(|clause| {
        let mut words = clause.split_whitespace();
        let Some((method, result)) = words.next().and_then(|w| w.split_once('=')) else {
            return false;
        };
        if !result.eq_ignore_ascii_case("pass") {
            return false;
        }
        let props: Vec<(String, String)> = words
            .filter_map(|w| w.split_once('='))
            .map(|(k, v)| {
                (
                    k.to_ascii_lowercase(),
                    v.trim_matches('"').to_ascii_lowercase(),
                )
            })
            .collect();
        let prop = |key: &str| {
            props
                .iter()
                .find(|(k, _)| k == key)
                .map(|(_, v)| v.as_str())
        };
        match method.to_ascii_lowercase().as_str() {
            "dmarc" => prop("header.from") == Some(from_domain),
            "dkim" => prop("header.d")
                .or_else(|| prop("header.i").and_then(|i| i.rsplit_once('@').map(|(_, d)| d)))
                .is_some_and(|d| aligned(d, from_domain)),
            "spf" => prop("smtp.mailfrom")
                .map(|m| m.rsplit_once('@').map_or(m, |(_, d)| d))
                .is_some_and(|d| aligned(d, from_domain)),
            _ => false,
        }
    })
}

/// Relaxed alignment, without a public suffix list: the domains match, or
/// the From domain sits under the checked one and the checked one has at
/// least two labels, so `mail.example.com` aligns with `example.com` but
/// nothing aligns with a bare `com`.
fn aligned(checked: &str, from_domain: &str) -> bool {
    let checked = checked.trim_end_matches('.');
    checked == from_domain
        || (checked.contains('.') && from_domain.ends_with(&format!(".{checked}")))
}

/// RFC 5322 comments in parentheses removed, so `(sender IP is 1.2.3.4)`
/// can't hold a `dmarc=pass` that would be read.
fn strip_comments(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut depth = 0u32;
    let mut quoted = false;
    for c in value.chars() {
        match c {
            '"' if depth == 0 => {
                quoted = !quoted;
                out.push(c);
            }
            '(' if !quoted => depth += 1,
            ')' if !quoted && depth > 0 => depth -= 1,
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    out
}

/// What the model reads from a message: its subject and its new text, with
/// quoted earlier mail cut off.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Incoming {
    pub sender: String,
    pub subject: String,
    pub text: String,
    /// `Message-ID`, angle brackets kept, for the reply's threading headers.
    pub message_id: String,
    /// `References`, then this message's id, for the reply.
    pub references: String,
}

/// The message as the model should see it, when it comes from an allowed,
/// authenticated sender. `allowed` gets the lowercase sender address.
pub fn incoming(raw: &[u8], config: &Config, allowed: impl Fn(&str) -> bool) -> Option<Incoming> {
    let msg = parse_message(raw);
    let sender = sender(&msg)?;
    if !allowed(&sender) {
        return None;
    }
    let from_domain = domain_of(&sender)?;
    if !authenticated(&msg, &config.auth_server, from_domain) {
        return None;
    }
    // A reply the app itself sent (seen again in a shared mailbox) is skipped.
    if msg
        .first("Auto-Submitted")
        .is_some_and(|v| !v.eq_ignore_ascii_case("no"))
    {
        return None;
    }
    let text = new_text(&body_text(&msg, 0)?);
    if text.is_empty() {
        return None;
    }
    let message_id = msg
        .first("Message-ID")
        .map(|v| v.split_whitespace().next().unwrap_or("").to_string())
        .filter(|id| id.starts_with('<') && id.ends_with('>'))
        .unwrap_or_default();
    let mut references: Vec<&str> = msg
        .first("References")
        .unwrap_or("")
        .split_whitespace()
        .filter(|r| r.starts_with('<') && r.ends_with('>'))
        .collect();
    if !message_id.is_empty() {
        references.push(&message_id);
    }
    // Keep the first and the last few, as RFC 5322 suggests for long chains.
    if references.len() > 10 {
        let tail = references.split_off(references.len() - 9);
        references.truncate(1);
        references.extend(tail);
    }
    Some(Incoming {
        sender,
        subject: one_line(&decode_words(msg.first("Subject").unwrap_or(""))),
        text: cap(&text),
        references: references.join(" "),
        message_id,
    })
}

/// The readable text of a message or part: `text/plain` first, else
/// `text/html` with its tags removed. Attachments are skipped.
fn body_text(part: &Message, depth: usize) -> Option<String> {
    let content_type = part.first("Content-Type").unwrap_or("text/plain");
    let (mime, params) = media_type(content_type);
    let disposition = part.first("Content-Disposition").unwrap_or("");
    if disposition
        .trim_start()
        .to_ascii_lowercase()
        .starts_with("attachment")
    {
        return None;
    }
    if let Some(sub) = mime.strip_prefix("multipart/") {
        if depth >= MAX_DEPTH {
            return None;
        }
        let boundary = param(&params, "boundary")?;
        let parts: Vec<Message> = split_parts(&part.body, &boundary)
            .into_iter()
            .map(parse_message)
            .collect();
        let pick = |want: &str| {
            parts.iter().find_map(|p| {
                let (m, _) = media_type(p.first("Content-Type").unwrap_or("text/plain"));
                (m == want || m.starts_with("multipart/"))
                    .then(|| body_text(p, depth + 1))
                    .flatten()
            })
        };
        return if sub == "alternative" {
            pick("text/plain").or_else(|| pick("text/html"))
        } else {
            parts.iter().find_map(|p| body_text(p, depth + 1))
        };
    }
    let bytes = transfer_decode(
        &part.body,
        part.first("Content-Transfer-Encoding").unwrap_or("7bit"),
    );
    let charset = param(&params, "charset").unwrap_or_default();
    match mime.as_str() {
        "text/plain" => Some(text_of(&bytes, &charset)),
        "text/html" => Some(strip_html(&text_of(&bytes, &charset))),
        _ => None,
    }
}

/// `type/subtype` lowercase, then the parameters as written.
fn media_type(value: &str) -> (String, String) {
    let (mime, params) = value.split_once(';').unwrap_or((value, ""));
    (mime.trim().to_ascii_lowercase(), params.to_string())
}

/// One parameter of a header value, unquoted.
fn param(params: &str, name: &str) -> Option<String> {
    params.split(';').find_map(|p| {
        let (k, v) = p.split_once('=')?;
        k.trim()
            .eq_ignore_ascii_case(name)
            .then(|| v.trim().trim_matches('"').to_string())
            .filter(|v| !v.is_empty())
    })
}

/// The parts between `--boundary` lines, up to the closing `--boundary--`.
fn split_parts<'a>(body: &'a [u8], boundary: &str) -> Vec<&'a [u8]> {
    let marker = format!("--{boundary}");
    let mut parts = Vec::new();
    let mut start: Option<usize> = None;
    let mut pos = 0;
    while pos <= body.len() {
        let end = body[pos..]
            .iter()
            .position(|&b| b == b'\n')
            .map_or(body.len(), |i| pos + i);
        let line = &body[pos..end];
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        if line.starts_with(marker.as_bytes()) {
            let rest = &line[marker.len()..];
            if let Some(s) = start {
                // The line break before the boundary belongs to it.
                let mut stop = pos;
                if stop > s && body[stop - 1] == b'\n' {
                    stop -= 1;
                }
                if stop > s && body[stop - 1] == b'\r' {
                    stop -= 1;
                }
                parts.push(&body[s..stop]);
            }
            if rest.starts_with(b"--") {
                return parts;
            }
            start = Some((end + 1).min(body.len()));
        }
        pos = end + 1;
    }
    parts
}

/// The body bytes after `base64` or `quoted-printable`.
fn transfer_decode(body: &[u8], encoding: &str) -> Vec<u8> {
    match encoding.trim().to_ascii_lowercase().as_str() {
        "base64" => base64_decode(body),
        "quoted-printable" => qp_decode(body, false),
        _ => body.to_vec(),
    }
}

/// Bytes as text in their charset. UTF-8 and ASCII are read as UTF-8, with
/// bad bytes replaced; Latin-1 and Windows-1252 byte for byte; anything else
/// as UTF-8 too, which is what most mail is.
fn text_of(bytes: &[u8], charset: &str) -> String {
    let charset = charset.to_ascii_lowercase();
    let latin = matches!(
        charset.as_str(),
        "iso-8859-1" | "latin1" | "latin-1" | "windows-1252" | "cp1252" | "iso-8859-15"
    );
    if latin {
        return bytes.iter().map(|&b| b as char).collect();
    }
    match std::str::from_utf8(bytes) {
        Ok(s) => s.to_string(),
        // Raw 8-bit header bytes, with no charset named, are most often Latin-1.
        Err(_) if charset.is_empty() => bytes.iter().map(|&b| b as char).collect(),
        Err(_) => String::from_utf8_lossy(bytes).into_owned(),
    }
}

/// Tags removed, a few entities read, and line breaks where blocks end.
fn strip_html(html: &str) -> String {
    let mut out = String::with_capacity(html.len() / 2);
    let mut rest = html;
    while let Some(open) = rest.find('<') {
        out.push_str(&rest[..open]);
        let tag_end = rest[open..].find('>').map_or(rest.len(), |i| open + i + 1);
        let tag = rest[open..tag_end].to_ascii_lowercase();
        let name: String = tag
            .trim_start_matches(['<', '/'])
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric())
            .collect();
        rest = &rest[tag_end..];
        // Skip what a style or script block holds, not just its tags.
        if (name == "style" || name == "script") && !tag.starts_with("</") {
            let close = format!("</{name}");
            let lower = rest.to_ascii_lowercase();
            rest = lower.find(&close).map_or("", |i| &rest[i..]);
            continue;
        }
        if matches!(
            name.as_str(),
            "br" | "p" | "div" | "tr" | "li" | "h1" | "h2" | "h3"
        ) {
            out.push('\n');
        }
    }
    out.push_str(rest);
    let out = out
        .replace("&nbsp;", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&amp;", "&");
    let lines: Vec<&str> = out.lines().map(str::trim_end).collect();
    lines.join("\n")
}

/// The text the sender just wrote: quoted lines dropped, and everything from
/// an attribution line like `On Tue, Jane wrote:` or a signature `-- ` on.
fn new_text(text: &str) -> String {
    let mut kept: Vec<&str> = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim_end();
        if trimmed == "--" || line == "-- " {
            break;
        }
        if trimmed.starts_with("On ") && trimmed.ends_with("wrote:") {
            break;
        }
        if trimmed.starts_with("-----Original Message-----") {
            break;
        }
        if trimmed.starts_with('>') {
            continue;
        }
        kept.push(trimmed);
    }
    let joined = kept.join("\n");
    joined.trim().to_string()
}

fn cap(text: &str) -> String {
    match text.char_indices().nth(MAX_TEXT) {
        Some((cut, _)) => text[..cut].to_string(),
        None => text.to_string(),
    }
}

fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// RFC 2047 encoded words (`=?utf-8?B?...?=` and `?Q?`) in a header value,
/// read; anything that isn't one is kept as written.
pub fn decode_words(value: &str) -> String {
    let mut out = String::new();
    let mut rest = value;
    let mut last_was_word = false;
    while let Some(start) = rest.find("=?") {
        let Some(word) = encoded_word(&rest[start..]) else {
            out.push_str(&rest[..start + 2]);
            rest = &rest[start + 2..];
            last_was_word = false;
            continue;
        };
        let between = &rest[..start];
        // Space between two encoded words is dropped (RFC 2047 section 6.2).
        if !(last_was_word && between.trim().is_empty()) {
            out.push_str(between);
        }
        out.push_str(&word.0);
        rest = &rest[start + word.1..];
        last_was_word = true;
    }
    out.push_str(rest);
    out
}

/// One encoded word at the start of `text`: its text and its length.
fn encoded_word(text: &str) -> Option<(String, usize)> {
    let inner = text.strip_prefix("=?")?;
    let (charset, inner) = inner.split_once('?')?;
    let (encoding, inner) = inner.split_once('?')?;
    let end = inner.find("?=")?;
    let data = &inner[..end];
    if data.contains(' ') {
        return None;
    }
    let bytes = match encoding {
        "B" | "b" => base64_decode(data.as_bytes()),
        "Q" | "q" => qp_decode(data.as_bytes(), true),
        _ => return None,
    };
    let len = 2 + charset.len() + 1 + encoding.len() + 1 + end + 2;
    // A language tag may follow the charset: `utf-8*en`.
    let charset = charset.split('*').next().unwrap_or(charset);
    Some((text_of(&bytes, charset), len))
}

/// Quoted-printable read. `header` also reads `_` as a space.
fn qp_decode(data: &[u8], header: bool) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len());
    let mut i = 0;
    while i < data.len() {
        match data[i] {
            b'=' if data[i + 1..].starts_with(b"\r\n") => i += 3,
            b'=' if data[i + 1..].starts_with(b"\n") => i += 2,
            b'=' => {
                let hex = data
                    .get(i + 1..i + 3)
                    .and_then(|h| std::str::from_utf8(h).ok());
                match hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
                    Some(b) => {
                        out.push(b);
                        i += 3;
                    }
                    None => {
                        out.push(b'=');
                        i += 1;
                    }
                }
            }
            b'_' if header => {
                out.push(b' ');
                i += 1;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    out
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Standard base64 read, skipping line breaks and anything else outside the
/// alphabet, and stopping at padding.
pub fn base64_decode(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() * 3 / 4);
    let mut acc = 0u32;
    let mut bits = 0;
    for &c in data {
        if c == b'=' {
            break;
        }
        let Some(v) = B64.iter().position(|&b| b == c) else {
            continue;
        };
        acc = (acc << 6) | v as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    out
}

/// Standard base64 with padding, on one line.
pub fn base64_encode(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let n = chunk.len();
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let v = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for i in 0..4 {
            if i <= n {
                out.push(B64[((v >> (18 - 6 * i)) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// A reply ready for SMTP `DATA`: CRLF line ends, UTF-8 text in base64,
/// threaded under the message it answers. `message_id` is the new
/// message's own id, angle brackets included; `date` is an RFC 5322 date.
pub struct Reply<'a> {
    pub from: &'a str,
    pub to: &'a str,
    pub subject: &'a str,
    pub in_reply_to: &'a str,
    pub references: &'a str,
    pub text: &'a str,
    pub message_id: &'a str,
    pub date: &'a str,
}

impl Reply<'_> {
    pub fn render(&self) -> String {
        let subject = clean(self.subject);
        let subject = if subject.to_ascii_lowercase().starts_with("re:") {
            subject
        } else if subject.is_empty() {
            "Re: your message".to_string()
        } else {
            format!("Re: {subject}")
        };
        let mut head = vec![
            format!("From: {}", clean(self.from)),
            format!("To: {}", clean(self.to)),
            format!("Subject: {}", encode_header(&subject)),
            format!("Date: {}", clean(self.date)),
            format!("Message-ID: {}", clean(self.message_id)),
        ];
        if !self.in_reply_to.is_empty() {
            head.push(format!("In-Reply-To: {}", clean(self.in_reply_to)));
        }
        if !self.references.is_empty() {
            head.push(format!("References: {}", clean(self.references)));
        }
        head.extend([
            // Marks the answer as sent by a program, so mail rules and the
            // app itself don't answer it in turn (RFC 3834).
            "Auto-Submitted: auto-replied".to_string(),
            "MIME-Version: 1.0".to_string(),
            "Content-Type: text/plain; charset=utf-8".to_string(),
            "Content-Transfer-Encoding: base64".to_string(),
        ]);
        let body = base64_encode(cap(self.text).as_bytes());
        let lines: Vec<&str> = body
            .as_bytes()
            .chunks(76)
            .map(|c| std::str::from_utf8(c).unwrap_or(""))
            .collect();
        format!("{}\r\n\r\n{}\r\n", head.join("\r\n"), lines.join("\r\n"))
    }
}

/// A header value on one line, with no CR or LF a sender could use to add a
/// header of their own.
fn clean(value: &str) -> String {
    one_line(&value.replace(['\r', '\n'], " "))
}

/// A header value as is when it is plain ASCII, else one UTF-8 encoded word.
fn encode_header(value: &str) -> String {
    if value.is_ascii() {
        value.to_string()
    } else {
        format!("=?utf-8?B?{}?=", base64_encode(value.as_bytes()))
    }
}

/// An RFC 5322 date in UTC for Unix seconds, like `Fri, 27 Nov 2026 06:00:00 +0000`.
pub fn rfc5322_date(unix: u64) -> String {
    const DAYS: [&str; 7] = ["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"];
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let days = unix / 86_400;
    let secs = unix % 86_400;
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{}, {:02} {} {} {:02}:{:02}:{:02} +0000",
        DAYS[(days % 7) as usize],
        day,
        MONTHS[(month - 1) as usize],
        year,
        secs / 3600,
        secs / 60 % 60,
        secs % 60
    )
}

/// A new `Message-ID` from random bytes, under the sending domain.
pub fn message_id(random: &[u8], domain: &str) -> String {
    let local: String = random.iter().map(|b| format!("{b:02x}")).collect();
    format!("<{local}@{domain}>")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A test mailbox at the known provider, built from its table so no
    /// real-looking address sits in the source.
    fn known_address() -> String {
        format!("me@{}", KNOWN[0].0[0])
    }

    fn gmail() -> Config {
        parse_config(&known_address().to_uppercase()).unwrap()
    }

    fn mail(auth: &str, from: &str, rest: &str) -> Vec<u8> {
        format!(
            "Authentication-Results: {auth}\r\nFrom: {from}\r\nSubject: Lunch\r\nMessage-ID: <a1@example.com>\r\n{rest}\r\nHello there.\r\n"
        )
        .into_bytes()
    }

    const PASS: &str = "mx.google.com; dkim=pass header.i=@example.com header.s=s1; spf=pass (google.com: domain of jane@example.com designates 1.2.3.4) smtp.mailfrom=jane@example.com; dmarc=pass (p=REJECT) header.from=example.com";

    #[test]
    fn a_known_address_fills_its_servers_and_others_need_all_four() {
        let config = gmail();
        assert_eq!(config.address, known_address());
        assert_eq!(config.imap_host, "imap.gmail.com");
        assert_eq!(config.saved(), known_address());
        let custom =
            parse_config("me@example.org imap.example.org smtp.example.org mx.example.org")
                .unwrap();
        assert_eq!(
            custom.saved(),
            "me@example.org imap.example.org smtp.example.org mx.example.org"
        );
        assert_eq!(parse_config(&custom.saved()).unwrap(), custom);
        assert!(parse_config("me@example.org")
            .unwrap_err()
            .starts_with("For this provider"));
        assert_eq!(
            parse_config("not an address").unwrap_err(),
            "That isn't an email address."
        );
        for bad in [
            "127.0.0.1",
            "localhost",
            "https://imap.x.org",
            "imap.x.org:993",
            "imap",
        ] {
            let text = format!("me@example.org {bad} smtp.example.org mx.example.org");
            assert!(parse_config(&text).is_err(), "{bad}");
        }
        assert!(!valid_address("a@b"));
        assert!(!valid_address("a b@example.com"));
        assert!(valid_address("jane.doe+tag@mail.example.com"));
    }

    #[test]
    fn only_the_topmost_header_from_the_pinned_server_counts() {
        let config = gmail();
        let allowed = |a: &str| a == "jane@example.com";
        let ok = mail(PASS, "Jane <jane@example.com>", "");
        let got = incoming(&ok, &config, allowed).unwrap();
        assert_eq!(
            (got.sender.as_str(), got.subject.as_str(), got.text.as_str()),
            ("jane@example.com", "Lunch", "Hello there.")
        );
        // A header the sender wrote sits under the server's and is never read.
        let forged = format!(
            "Authentication-Results: mx.google.com; dmarc=fail header.from=example.com\r\n{}",
            String::from_utf8(mail(PASS, "jane@example.com", "")).unwrap()
        );
        assert!(incoming(forged.as_bytes(), &config, allowed).is_none());
        // Another server's id, or none at all.
        let other = mail(
            &PASS.replace("mx.google.com", "mx.evil.test"),
            "jane@example.com",
            "",
        );
        assert!(incoming(&other, &config, allowed).is_none());
        let none = b"From: jane@example.com\r\n\r\nHi\r\n";
        assert!(incoming(none, &config, allowed).is_none());
        // A pass hidden in a comment isn't a pass.
        let comment = mail("mx.google.com; dmarc=fail (dmarc=pass header.from=example.com) header.from=example.com", "jane@example.com", "");
        assert!(incoming(&comment, &config, allowed).is_none());
        // A sender the person didn't add is dropped even when it passes.
        assert!(incoming(&ok, &config, |_| false).is_none());
    }

    #[test]
    fn dkim_and_spf_pass_only_when_aligned_with_the_from_domain() {
        let msg = |auth: &str| parse_message(&mail(auth, "jane@mail.example.com", ""));
        let check = |auth: &str| authenticated(&msg(auth), "mx.google.com", "mail.example.com");
        assert!(check("mx.google.com; dkim=pass header.d=example.com"));
        assert!(check(
            "mx.google.com; spf=pass smtp.mailfrom=bounce@mail.example.com"
        ));
        assert!(!check("mx.google.com; dkim=pass header.d=evil.test"));
        assert!(!check("mx.google.com; dkim=pass header.d=com"));
        assert!(!check("mx.google.com; spf=pass smtp.mailfrom=x@evil.test"));
        assert!(!check("mx.google.com; dmarc=pass header.from=example.com"));
        assert!(check(
            "mx.google.com 1; dmarc=pass header.from=mail.example.com"
        ));
        assert!(!check("mx.google.com; dkim=fail header.d=example.com"));
    }

    #[test]
    fn a_from_with_two_senders_is_never_trusted() {
        let one = |from: &str| sender(&parse_message(format!("From: {from}\r\n\r\n").as_bytes()));
        assert_eq!(
            one("\"Doe, Jane\" <Jane@Example.com>").as_deref(),
            Some("jane@example.com")
        );
        assert_eq!(
            one("=?utf-8?Q?J=C3=A9r=C3=B4me?= <j@example.com>").as_deref(),
            Some("j@example.com")
        );
        assert_eq!(one("jane@example.com, eve@evil.test"), None);
        assert_eq!(one("Jane <jane@example.com>, <eve@evil.test>"), None);
        assert_eq!(one("<eve@evil.test> <jane@example.com>"), None);
        let twice = b"From: jane@example.com\r\nFrom: eve@evil.test\r\n\r\n";
        assert_eq!(sender(&parse_message(twice)), None);
    }

    #[test]
    fn reads_the_plain_part_decodes_it_and_cuts_the_quoted_mail() {
        let body = "This is a multi-part message.\r\n--b1\r\nContent-Type: text/plain; charset=\"utf-8\"\r\nContent-Transfer-Encoding: quoted-printable\r\n\r\nCaf=C3=A9 at noon?\r\nSee you =\r\nthere.\r\n\r\nOn Tue, Nov 24, Me wrote:\r\n> earlier\r\n--b1\r\nContent-Type: text/html\r\n\r\n<p>ignored</p>\r\n--b1--\r\n";
        let raw = format!(
            "Authentication-Results: {PASS}\r\nFrom: jane@example.com\r\nSubject: =?UTF-8?B?Q2Fmw6k=?= plans\r\nMessage-ID: <m2@example.com>\r\nReferences: <m0@example.com> <m1@example.com>\r\nContent-Type: multipart/alternative; boundary=b1\r\n\r\n{body}"
        );
        let got = incoming(raw.as_bytes(), &gmail(), |_| true).unwrap();
        assert_eq!(got.subject, "Café plans");
        assert_eq!(got.text, "Café at noon?\nSee you there.");
        assert_eq!(
            got.references,
            "<m0@example.com> <m1@example.com> <m2@example.com>"
        );
        assert_eq!(got.message_id, "<m2@example.com>");
    }

    #[test]
    fn html_only_mail_loses_its_tags_and_attachments_are_skipped() {
        let html = "Content-Type: text/html; charset=iso-8859-1\r\n\r\n<style>p{color:red}</style><p>Caf\u{e9} &amp; tea</p><br>Bye";
        let raw = format!("Authentication-Results: {PASS}\r\nFrom: jane@example.com\r\n{html}");
        // The body is Latin-1 on the wire: é as one byte.
        let raw = raw.replace('\u{e9}', "\u{0}");
        let mut bytes = raw.into_bytes();
        let at = bytes.iter().position(|&b| b == 0).unwrap();
        bytes[at] = 0xe9;
        let got = incoming(&bytes, &gmail(), |_| true).unwrap();
        assert_eq!(got.text, "Café & tea\n\nBye");
        let only_file = format!(
            "Authentication-Results: {PASS}\r\nFrom: jane@example.com\r\nContent-Type: multipart/mixed; boundary=x\r\n\r\n--x\r\nContent-Type: text/plain\r\nContent-Disposition: attachment; filename=a.txt\r\n\r\nsecret\r\n--x--\r\n"
        );
        assert!(incoming(only_file.as_bytes(), &gmail(), |_| true).is_none());
    }

    #[test]
    fn an_automatic_reply_is_not_answered() {
        let raw = mail(PASS, "jane@example.com", "Auto-Submitted: auto-replied\r\n");
        assert!(incoming(&raw, &gmail(), |_| true).is_none());
    }

    #[test]
    fn a_reply_threads_under_the_message_and_cannot_grow_headers() {
        let reply = Reply {
            from: "me@example.com",
            to: "jane@example.com\r\nBcc: eve@evil.test",
            subject: "Café plans",
            in_reply_to: "<m2@example.com>",
            references: "<m1@example.com> <m2@example.com>",
            text: "Noon works.",
            message_id: &message_id(&[0xab, 0x01], "example.com"),
            date: &rfc5322_date(1_795_759_200),
        }
        .render();
        assert!(reply.contains("To: jane@example.com Bcc: eve@evil.test\r\n"));
        assert!(!reply.contains("\r\nBcc:"));
        assert!(reply.contains("Subject: =?utf-8?B?UmU6IENhZsOpIHBsYW5z?=\r\n"));
        assert!(reply.contains("Message-ID: <ab01@example.com>\r\n"));
        assert!(reply.contains(
            "In-Reply-To: <m2@example.com>\r\nReferences: <m1@example.com> <m2@example.com>\r\n"
        ));
        assert!(reply.contains("Date: Fri, 27 Nov 2026 06:00:00 +0000\r\n"));
        let body = reply.split("\r\n\r\n").nth(1).unwrap();
        assert_eq!(base64_decode(body.as_bytes()), b"Noon works.");
        assert_eq!(base64_encode(b"ab"), "YWI=");
        assert_eq!(base64_encode(b""), "");
    }
}
