//! Shared OAuth helpers for the browser sign-ins (Imagine's xAI sign-in, MCP
//! servers): PKCE (RFC 7636), `state`, form encoding, and the loopback
//! callback. No HTTP, no keychain.

fn b64url(data: &[u8]) -> String {
    const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::new();
    let mut i = 0;
    while i < data.len() {
        let b0 = data[i];
        let b1 = if i + 1 < data.len() { data[i + 1] } else { 0 };
        let b2 = if i + 2 < data.len() { data[i + 2] } else { 0 };
        let n = ((b0 as u32) << 16) | ((b1 as u32) << 8) | (b2 as u32);
        out.push(T[((n >> 18) & 63) as usize] as char);
        out.push(T[((n >> 12) & 63) as usize] as char);
        if i + 1 < data.len() {
            out.push(T[((n >> 6) & 63) as usize] as char);
        }
        if i + 2 < data.len() {
            out.push(T[(n & 63) as usize] as char);
        }
        i += 3;
    }
    out
}

/// 64 random bytes, base64url, no padding. RFC 7636 verifier length stays inside 43–128.
pub fn pkce_verifier() -> String {
    let mut buf = [0u8; 64];
    crate::fill_random(&mut buf);
    b64url(&buf)
}

/// 32 random bytes, base64url, no padding. Used for `state` and `nonce`.
pub fn oauth_nonce() -> String {
    let mut buf = [0u8; 32];
    crate::fill_random(&mut buf);
    b64url(&buf)
}

/// S256 code challenge. `pkce_challenge` hashes the verifier string, not the raw bytes.
pub fn pkce_challenge(verifier: &str) -> String {
    use sha2::{Digest, Sha256};
    let dig = Sha256::digest(verifier.as_bytes());
    b64url(&dig)
}

/// Constant-time compare. Different lengths still walk the longer side.
pub fn ct_eq(a: &str, b: &str) -> bool {
    let ab = a.as_bytes();
    let bb = b.as_bytes();
    let mut diff = ab.len() ^ bb.len();
    let n = ab.len().max(bb.len());
    for i in 0..n {
        let x = if i < ab.len() { ab[i] } else { 0 };
        let y = if i < bb.len() { bb[i] } else { 0 };
        diff |= usize::from(x ^ y);
    }
    diff == 0
}

/// Percent-encode everything but RFC 3986 unreserved characters.
pub fn urlencode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// `application/x-www-form-urlencoded` body (or query string) from pairs, in order.
pub fn form(pairs: &[(&str, &str)]) -> String {
    pairs
        .iter()
        .map(|(k, v)| format!("{}={}", urlencode(k), urlencode(v)))
        .collect::<Vec<_>>()
        .join("&")
}

fn pct_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b'%' if i + 2 < bytes.len() => {
                let h = std::str::from_utf8(&bytes[i + 1..i + 3]).ok();
                if let Some(hex) = h.and_then(|h| u8::from_str_radix(h, 16).ok()) {
                    out.push(hex);
                    i += 3;
                } else {
                    out.push(b'%');
                    i += 1;
                }
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn query_map(query: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for pair in query.split('&') {
        if pair.is_empty() {
            continue;
        }
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        out.push((pct_decode(k), pct_decode(v)));
    }
    out
}

fn query_get<'a>(q: &'a [(String, String)], key: &str) -> Option<&'a str> {
    q.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
}

/// First line of the loopback request: `GET /callback?code=..&state=.. HTTP/1.1`.
/// State is compared in constant time. `error=`, a wrong path, a missing code,
/// and a state mismatch each return their own error, led by `who` ("Imagine sign-in").
pub fn loopback_code(raw: &str, expected_state: &str, who: &str) -> Result<String, String> {
    let line = raw.lines().next().unwrap_or("").trim();
    let mut parts = line.split_whitespace();
    let method = parts.next().unwrap_or("");
    let target = parts.next().unwrap_or("");
    if !method.eq_ignore_ascii_case("GET") {
        return Err(format!("{who} callback was not a GET"));
    }
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    if path != "/callback" {
        return Err(format!("{who} callback was not /callback"));
    }
    let q = query_map(query);
    let got_state = query_get(&q, "state").unwrap_or("");
    let state_ok = ct_eq(got_state, expected_state);
    if let Some(err) = query_get(&q, "error").filter(|s| !s.is_empty()) {
        if !state_ok {
            return Err(format!("{who} state did not match"));
        }
        let desc = query_get(&q, "error_description")
            .filter(|s| !s.is_empty())
            .unwrap_or(err);
        return Err(format!("{who} failed: {desc}"));
    }
    let code = query_get(&q, "code").unwrap_or("").to_string();
    if code.is_empty() {
        return Err(format!("{who} callback had no code"));
    }
    if !state_ok {
        return Err(format!("{who} state did not match"));
    }
    Ok(code)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn form_encodes_unreserved_only_and_keeps_order() {
        assert_eq!(urlencode("a b/c:d~e"), "a%20b%2Fc%3Ad~e");
        assert_eq!(
            form(&[("redirect_uri", "http://127.0.0.1:9/callback"), ("scope", "read write")]),
            "redirect_uri=http%3A%2F%2F127.0.0.1%3A9%2Fcallback&scope=read%20write"
        );
    }

    #[test]
    fn the_loopback_callback_names_who_and_checks_state() {
        let raw = "GET /callback?code=c%2D1&state=s1 HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n";
        assert_eq!(loopback_code(raw, "s1", "Linear sign-in").as_deref(), Ok("c-1"));
        assert_eq!(
            loopback_code("GET /callback?code=c&state=s2 HTTP/1.1", "s1", "Linear sign-in"),
            Err("Linear sign-in state did not match".to_string())
        );
        assert_eq!(
            loopback_code("GET /callback?error=access_denied&state=s1 HTTP/1.1", "s1", "Linear sign-in"),
            Err("Linear sign-in failed: access_denied".to_string())
        );
        assert_eq!(
            loopback_code("POST /callback?code=c&state=s1 HTTP/1.1", "s1", "Linear sign-in"),
            Err("Linear sign-in callback was not a GET".to_string())
        );
    }
}
