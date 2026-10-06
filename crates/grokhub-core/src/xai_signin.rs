//! Imagine's own xAI sign-in. PKCE and device-code helpers. No HTTP, no keychain.
//!
//! Cabin OAuth (`oauth` module, `grok-cli:access`) is a different client use.
//! This scope is `openid profile email offline_access api:access` only.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::oauth::{
    merge_refreshed, oauth_access_live, token_needs_refresh, trusted_xai_url, XaiOAuthTokens,
    XAI_DEVICE_CODE_GRANT, XAI_OAUTH_CLIENT_ID,
};

/// Imagine does not ask for `grok-cli:access`.
pub const IMAGINE_OAUTH_SCOPE: &str = "openid profile email offline_access api:access";
pub const IMAGINE_OAUTH_REFERRER: &str = "grokhub";

pub const IMAGINE_NEED_SIGNIN: &str =
    "Sign in with Grok in Settings → Account, or add a console API key.";
pub const IMAGINE_OAUTH_DENIED: &str = "xAI didn't let this Grok sign-in use the Imagine API (their account allowlist). Use your console API key instead?";
pub const IMAGINE_KEY_REJECTED: &str = "API key rejected.";
pub const IMAGINE_RATE_LIMIT: &str = "Usage limit hit; try later or use the API key.";
pub const IMAGINE_SIGN_IN_AGAIN: &str = "Sign in again";
pub const IMAGINE_NO_KEYCHAIN: &str =
    "No keychain is available (Secret Service is not running). Add a console API key in Settings.";

/// Keychain / Secret Service failures. The text names the missing keychain and the API key.
pub fn keychain_unavailable_message(err: &str) -> String {
    let detail = err.trim();
    if detail.is_empty() {
        IMAGINE_NO_KEYCHAIN.to_string()
    } else if detail.to_ascii_lowercase().contains("keychain")
        && detail.to_ascii_lowercase().contains("api key")
    {
        detail.to_string()
    } else {
        format!("No keychain is available ({detail}). Add a console API key in Settings.")
    }
}

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

fn urlencode(s: &str) -> String {
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

fn form(pairs: &[(&str, &str)]) -> String {
    pairs
        .iter()
        .map(|(k, v)| format!("{}={}", urlencode(k), urlencode(v)))
        .collect::<Vec<_>>()
        .join("&")
}

pub struct ImagineAuthorize<'a> {
    pub authorization_endpoint: &'a str,
    pub redirect_uri: &'a str,
    pub code_challenge: &'a str,
    pub state: &'a str,
    pub nonce: &'a str,
}

/// Authorize URL. `referrer=grokhub` is appended once. Scope has no `grok-cli:access`.
pub fn imagine_authorize_url(p: &ImagineAuthorize<'_>) -> String {
    let endpoint = p.authorization_endpoint.trim().trim_end_matches('?');
    let query = form(&[
        ("response_type", "code"),
        ("client_id", XAI_OAUTH_CLIENT_ID),
        ("redirect_uri", p.redirect_uri),
        ("scope", IMAGINE_OAUTH_SCOPE),
        ("code_challenge", p.code_challenge),
        ("code_challenge_method", "S256"),
        ("state", p.state),
        ("nonce", p.nonce),
        ("referrer", IMAGINE_OAUTH_REFERRER),
    ]);
    format!("{endpoint}?{query}")
}

pub fn imagine_device_form() -> String {
    form(&[
        ("client_id", XAI_OAUTH_CLIENT_ID),
        ("scope", IMAGINE_OAUTH_SCOPE),
        ("referrer", IMAGINE_OAUTH_REFERRER),
    ])
}

pub fn imagine_device_poll_form(device_code: &str) -> String {
    form(&[
        ("grant_type", XAI_DEVICE_CODE_GRANT),
        ("client_id", XAI_OAUTH_CLIENT_ID),
        ("device_code", device_code),
    ])
}

pub fn imagine_code_form(code: &str, verifier: &str, redirect_uri: &str) -> String {
    form(&[
        ("grant_type", "authorization_code"),
        ("code", code),
        ("code_verifier", verifier),
        ("client_id", XAI_OAUTH_CLIENT_ID),
        ("redirect_uri", redirect_uri),
    ])
}

pub fn imagine_refresh_form(refresh_token: &str) -> String {
    form(&[
        ("grant_type", "refresh_token"),
        ("refresh_token", refresh_token),
        ("client_id", XAI_OAUTH_CLIENT_ID),
    ])
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImagineEndpoints {
    pub authorization: String,
    pub token: String,
    pub device: String,
}

pub fn parse_imagine_discovery(v: &Value) -> Result<ImagineEndpoints, String> {
    let take = |key: &str, missing: &str| -> Result<String, String> {
        let raw = v
            .get(key)
            .and_then(|x| x.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| missing.to_string())?;
        trusted_xai_url(raw)
    };
    Ok(ImagineEndpoints {
        authorization: take(
            "authorization_endpoint",
            "xAI discovery missing authorization endpoint",
        )?,
        token: take("token_endpoint", "xAI discovery missing token endpoint")?,
        device: take(
            "device_authorization_endpoint",
            "xAI discovery missing device endpoint",
        )?,
    })
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
/// and a state mismatch each return their own error.
pub fn parse_loopback_callback(raw: &str, expected_state: &str) -> Result<String, String> {
    let line = raw.lines().next().unwrap_or("").trim();
    let mut parts = line.split_whitespace();
    let method = parts.next().unwrap_or("");
    let target = parts.next().unwrap_or("");
    if !method.eq_ignore_ascii_case("GET") {
        return Err("Imagine sign-in callback was not a GET".into());
    }
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    if path != "/callback" {
        return Err("Imagine sign-in callback was not /callback".into());
    }
    let q = query_map(query);
    let got_state = query_get(&q, "state").unwrap_or("");
    let state_ok = ct_eq(got_state, expected_state);
    if let Some(err) = query_get(&q, "error").filter(|s| !s.is_empty()) {
        if !state_ok {
            return Err("Imagine sign-in state did not match".into());
        }
        let desc = query_get(&q, "error_description")
            .filter(|s| !s.is_empty())
            .unwrap_or(err);
        return Err(format!("Imagine sign-in failed: {desc}"));
    }
    let code = query_get(&q, "code").unwrap_or("").to_string();
    if code.is_empty() {
        return Err("Imagine sign-in callback had no code".into());
    }
    if !state_ok {
        return Err("Imagine sign-in state did not match".into());
    }
    Ok(code)
}

/// Keychain JSON. `Debug` never prints access, refresh, or id tokens.
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct ImagineTokens {
    pub access_token: String,
    #[serde(default)]
    pub refresh_token: Option<String>,
    #[serde(default)]
    pub expires_at: Option<u64>,
    #[serde(default)]
    pub id_token: Option<String>,
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub connected_at: u64,
}

impl std::fmt::Debug for ImagineTokens {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ImagineTokens")
            .field("access_token", &"<redacted>")
            .field(
                "refresh_token",
                &self.refresh_token.as_ref().map(|_| "<redacted>"),
            )
            .field("id_token", &self.id_token.as_ref().map(|_| "<redacted>"))
            .field("expires_at", &self.expires_at)
            .field("email", &self.email)
            .field("connected_at", &self.connected_at)
            .finish()
    }
}

pub fn imagine_tokens_from_xai(t: &XaiOAuthTokens) -> ImagineTokens {
    ImagineTokens {
        access_token: t.access_token.clone(),
        refresh_token: t.refresh_token.clone(),
        expires_at: t.expires_at,
        id_token: t.id_token.clone(),
        email: t.email.clone(),
        connected_at: t.connected_at,
    }
}

fn as_xai(t: &ImagineTokens) -> XaiOAuthTokens {
    XaiOAuthTokens {
        access_token: t.access_token.clone(),
        refresh_token: t.refresh_token.clone(),
        expires_at: t.expires_at,
        id_token: t.id_token.clone(),
        email: t.email.clone(),
        connected_at: t.connected_at,
        ..Default::default()
    }
}

pub fn imagine_needs_refresh(t: &ImagineTokens, now_ms: u64) -> bool {
    token_needs_refresh(&as_xai(t), now_ms)
}

pub fn imagine_access_usable(t: &ImagineTokens, now_ms: u64) -> bool {
    oauth_access_live(&as_xai(t), now_ms)
}

/// Prefer Imagine sign-in when the access token still works, or a refresh token can renew it.
pub fn imagine_oauth_preferred(t: &ImagineTokens, now_ms: u64) -> bool {
    if t.access_token.trim().is_empty() {
        return false;
    }
    if imagine_access_usable(t, now_ms) {
        return true;
    }
    t.refresh_token
        .as_ref()
        .is_some_and(|s| !s.trim().is_empty())
}

pub fn merge_imagine_refresh(prev: &ImagineTokens, next: ImagineTokens) -> ImagineTokens {
    imagine_tokens_from_xai(&merge_refreshed(&as_xai(prev), as_xai(&next)))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImagineCredKind {
    OAuth,
    ConsoleKey,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImagineCred {
    pub secret: String,
    pub kind: ImagineCredKind,
}

pub const XAI_NEED_SIGNIN: &str = "Sign in with Grok or add an API key.";
pub const XAI_OAUTH_ACCOUNT: &str = "xai-oauth";
pub const XAI_OAUTH_LEGACY_ACCOUNT: &str = "imagine-oauth";
pub const METER_OAUTH: &str = "SuperGrok pool";
pub const METER_API_KEY: &str = "API credits";

/// Keychain accounts. Tests use an in-memory store. This module never opens a keychain.
pub trait OAuthAccountStore {
    fn load_account(&self, account: &str) -> Result<Option<ImagineTokens>, String>;
    fn save_account(&self, account: &str, tokens: &ImagineTokens) -> Result<(), String>;
    fn delete_account(&self, account: &str) -> Result<(), String>;
}

fn tokens_present(tokens: &ImagineTokens) -> bool {
    !tokens.access_token.trim().is_empty()
        || tokens
            .refresh_token
            .as_ref()
            .is_some_and(|s| !s.trim().is_empty())
}

/// Load the current account. On the first load, copy a legacy account once.
/// A failed save leaves the legacy account in place.
pub fn load_xai_oauth(store: &dyn OAuthAccountStore) -> Result<Option<ImagineTokens>, String> {
    if let Some(current) = store.load_account(XAI_OAUTH_ACCOUNT)? {
        if tokens_present(&current) {
            return Ok(Some(current));
        }
    }
    if let Some(legacy) = store.load_account(XAI_OAUTH_LEGACY_ACCOUNT)? {
        if tokens_present(&legacy) {
            store.save_account(XAI_OAUTH_ACCOUNT, &legacy)?;
            store.delete_account(XAI_OAUTH_LEGACY_ACCOUNT)?;
            return Ok(Some(legacy));
        }
    }
    Ok(None)
}

/// Sign-out removes both accounts so a later load cannot resurrect the legacy copy.
pub fn delete_xai_oauth(store: &dyn OAuthAccountStore) -> Result<(), String> {
    store.delete_account(XAI_OAUTH_ACCOUNT)?;
    store.delete_account(XAI_OAUTH_LEGACY_ACCOUNT)?;
    Ok(())
}

/// Same order as [`choose_imagine_bearer`], with the chat sign-in sentence.
pub fn choose_xai_bearer(
    oauth_access: Option<&str>,
    oauth_usable: bool,
    console_key: &str,
) -> Result<ImagineCred, &'static str> {
    choose_imagine_bearer(oauth_access, oauth_usable, console_key).map_err(|_| XAI_NEED_SIGNIN)
}

/// OAuth when signed in and usable, else the console key, else the sign-in hint.
/// CLI login is not a parameter.
pub fn choose_imagine_bearer(
    oauth_access: Option<&str>,
    oauth_usable: bool,
    console_key: &str,
) -> Result<ImagineCred, &'static str> {
    if oauth_usable {
        if let Some(tok) = oauth_access.map(str::trim).filter(|s| !s.is_empty()) {
            return Ok(ImagineCred {
                secret: tok.to_string(),
                kind: ImagineCredKind::OAuth,
            });
        }
    }
    let key = console_key.trim();
    if !key.is_empty() {
        return Ok(ImagineCred {
            secret: key.to_string(),
            kind: ImagineCredKind::ConsoleKey,
        });
    }
    Err(IMAGINE_NEED_SIGNIN)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImagineMappedError {
    pub text: String,
    /// OAuth 401/403 and a console key exists. The page shows [Use API key].
    pub offer_use_api_key: bool,
    /// OAuth 401/403 and no console key. The page shows a Settings link.
    pub offer_settings: bool,
}

fn imagine_http_status(err_lower: &str) -> Option<u16> {
    for code in [401u16, 403, 429, 400, 404, 408, 500, 502, 503] {
        let token = format!("http {code}");
        if err_lower.contains(&token) {
            return Some(code);
        }
    }
    None
}

/// OAuth 401/403, key 401/403, 429, moderation, and video failure codes.
/// Timeouts and other network stalls keep `imagine_network_hint`.
pub fn map_imagine_error(
    err: &str,
    kind: ImagineCredKind,
    has_console_key: bool,
) -> ImagineMappedError {
    let lower = err.to_ascii_lowercase();
    let status = imagine_http_status(&lower);
    let plain = |text: String| ImagineMappedError {
        text,
        offer_use_api_key: false,
        offer_settings: false,
    };
    if matches!(status, Some(401 | 403))
        || lower.contains("bad credentials")
        || lower.contains("unauthenticated")
    {
        return match kind {
            ImagineCredKind::OAuth => ImagineMappedError {
                text: IMAGINE_OAUTH_DENIED.into(),
                offer_use_api_key: has_console_key,
                offer_settings: !has_console_key,
            },
            ImagineCredKind::ConsoleKey => plain(IMAGINE_KEY_REJECTED.into()),
        };
    }
    if kind == ImagineCredKind::OAuth && err.trim() == IMAGINE_SIGN_IN_AGAIN {
        // A failed refresh: offer the way out instead of looping on the same error.
        return ImagineMappedError {
            text: IMAGINE_SIGN_IN_AGAIN.into(),
            offer_use_api_key: has_console_key,
            offer_settings: !has_console_key,
        };
    }
    if status == Some(429) || lower.contains("rate limit") || lower.contains("usage limit") {
        return plain(IMAGINE_RATE_LIMIT.into());
    }
    if lower.contains("moderation") {
        return plain(err.to_string());
    }
    if crate::imagine::imagine_is_network_stall(err) {
        return plain(crate::imagine::imagine_network_hint(err));
    }
    plain(err.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn imagine_failed_refresh_offers_a_way_out() {
        let with_key = map_imagine_error(IMAGINE_SIGN_IN_AGAIN, ImagineCredKind::OAuth, true);
        assert_eq!(with_key.text, IMAGINE_SIGN_IN_AGAIN);
        assert!(with_key.offer_use_api_key && !with_key.offer_settings);
        let no_key = map_imagine_error(IMAGINE_SIGN_IN_AGAIN, ImagineCredKind::OAuth, false);
        assert!(!no_key.offer_use_api_key && no_key.offer_settings);
    }

    #[test]
    fn imagine_pkce_rfc7636_appendix_b() {
        // RFC 7636 appendix B. The verifier is the 32-octet sample, not a lookalike.
        assert_eq!(
            pkce_challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
        let v = pkce_verifier();
        assert_eq!(v.len(), 86, "64 bytes, base64url, no padding");
        assert!(v
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'));
        assert!(!v.contains('='));
        assert_ne!(pkce_verifier(), pkce_verifier());
        assert!(ct_eq("same", "same"));
        assert!(!ct_eq("same", "same "));
        assert!(!ct_eq("abc", "abd"));
    }

    #[test]
    fn imagine_authorize_url_referrer_once() {
        let url = imagine_authorize_url(&ImagineAuthorize {
            authorization_endpoint: "https://auth.x.ai/oauth2/authorize",
            redirect_uri: "http://127.0.0.1:1234/callback",
            code_challenge: "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM",
            state: "st",
            nonce: "no",
        });
        assert_eq!(url.matches("referrer=grokhub").count(), 1, "{url}");
        assert!(url.contains("response_type=code"), "{url}");
        assert!(url.contains("code_challenge_method=S256"), "{url}");
        assert!(url.contains(&format!("client_id={XAI_OAUTH_CLIENT_ID}")), "{url}");
        assert!(url.contains("redirect_uri="), "{url}");
        assert!(url.contains("127.0.0.1"), "{url}");
        assert!(url.contains("callback"), "{url}");
        assert!(url.contains("code_challenge=E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"));
        assert!(url.contains(&urlencode(IMAGINE_OAUTH_SCOPE)), "{url}");
        assert!(!url.contains("grok-cli"), "{url}");
        assert!(!IMAGINE_OAUTH_SCOPE.contains("grok-cli"));
        let bad = parse_imagine_discovery(&json!({
            "authorization_endpoint": "https://evil.example/authorize",
            "token_endpoint": "https://auth.x.ai/oauth2/token",
            "device_authorization_endpoint": "https://auth.x.ai/oauth2/device",
        }));
        assert!(bad.is_err(), "{bad:?}");
        let ok = parse_imagine_discovery(&json!({
            "authorization_endpoint": "https://auth.x.ai/oauth2/authorize",
            "token_endpoint": "https://auth.x.ai/oauth2/token",
            "device_authorization_endpoint": "https://auth.x.ai/oauth2/device",
        }))
        .unwrap();
        assert!(ok.authorization.contains("authorize"));
        assert!(ok.token.contains("token"));
        assert!(ok.device.contains("device"));
    }

    #[test]
    fn imagine_callback_parses_and_rejects() {
        let ok = parse_loopback_callback(
            "GET /callback?code=abc&state=xyz HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n",
            "xyz",
        )
        .unwrap();
        assert_eq!(ok, "abc");
        let swapped = parse_loopback_callback(
            "GET /callback?state=xyz&code=abc HTTP/1.1",
            "xyz",
        )
        .unwrap();
        assert_eq!(swapped, "abc");
        let mismatch = parse_loopback_callback(
            "GET /callback?code=abc&state=nope HTTP/1.1",
            "xyz",
        );
        let mismatch = mismatch.expect_err("state mismatch");
        assert!(mismatch.contains("state"), "{mismatch}");
        let err = parse_loopback_callback(
            "GET /callback?error=access_denied&error_description=Not%20now&state=xyz HTTP/1.1",
            "xyz",
        );
        let err = err.unwrap_err();
        assert!(err.contains("Not now"), "{err}");
        let path = parse_loopback_callback("GET /other?code=abc&state=xyz HTTP/1.1", "xyz");
        assert!(path.unwrap_err().contains("/callback"));
        let missing = parse_loopback_callback("GET /callback?state=xyz HTTP/1.1", "xyz");
        assert!(missing.unwrap_err().to_ascii_lowercase().contains("code"));
        let long = parse_loopback_callback(
            "GET /callback?code=abc&state=xxxxxxxx HTTP/1.1",
            "x",
        );
        assert!(long.is_err());
    }

    #[test]
    fn imagine_device_form_has_referrer() {
        let body = imagine_device_form();
        assert_eq!(body.matches("referrer=grokhub").count(), 1, "{body}");
        assert!(body.contains(&format!("client_id={XAI_OAUTH_CLIENT_ID}")), "{body}");
        assert!(body.contains(&urlencode(IMAGINE_OAUTH_SCOPE)), "{body}");
        assert!(!body.contains("grok-cli"), "{body}");
        let code = imagine_code_form("the-code", "the-verifier", "http://127.0.0.1:9/callback");
        assert!(code.contains("grant_type=authorization_code"), "{code}");
        assert!(code.contains("code=the-code"), "{code}");
        assert!(code.contains("code_verifier=the-verifier"), "{code}");
        assert!(code.contains("redirect_uri="), "{code}");
        let refresh = imagine_refresh_form("the-refresh");
        assert!(refresh.contains("grant_type=refresh_token"), "{refresh}");
        assert!(refresh.contains("refresh_token=the-refresh"), "{refresh}");
        assert!(refresh.contains("client_id="), "{refresh}");
        let poll = imagine_device_poll_form("dev-1");
        assert!(poll.contains("device_code=dev-1"), "{poll}");
        assert!(poll.contains("urn%3Aietf%3Aparams%3Aoauth%3Agrant-type%3Adevice_code") || poll.contains("device_code"), "{poll}");
    }

    #[test]
    fn imagine_token_debug_redacts_and_refresh_merges() {
        let t = ImagineTokens {
            access_token: "secret-access".into(),
            refresh_token: Some("secret-refresh".into()),
            id_token: Some("secret-id".into()),
            expires_at: Some(50_000),
            email: Some("ada@example.com".into()),
            connected_at: 1,
        };
        let dbg = format!("{t:?}");
        assert!(!dbg.contains("secret-access"), "{dbg}");
        assert!(!dbg.contains("secret-refresh"), "{dbg}");
        assert!(!dbg.contains("secret-id"), "{dbg}");
        assert!(dbg.contains("redacted"), "{dbg}");
        assert!(dbg.contains("ada@example.com"), "{dbg}");
        let json = serde_json::to_string(&t).unwrap();
        assert!(json.contains("secret-access"), "keychain JSON keeps the token");
        let back: ImagineTokens = serde_json::from_str(&json).unwrap();
        assert_eq!(back, t);
        let now = 40_000u64;
        assert!(
            imagine_needs_refresh(&t, now),
            "inside the 30 minute skew"
        );
        let fresh = ImagineTokens {
            expires_at: Some(now + 60 * 60 * 1000),
            ..t.clone()
        };
        assert!(!imagine_needs_refresh(&fresh, now));
        let rotated = ImagineTokens {
            access_token: "new-access".into(),
            refresh_token: None,
            expires_at: Some(now + 3_600_000),
            id_token: None,
            email: None,
            connected_at: now,
        };
        let merged = merge_imagine_refresh(&t, rotated);
        assert_eq!(merged.access_token, "new-access");
        assert_eq!(merged.refresh_token.as_deref(), Some("secret-refresh"));
        assert_eq!(merged.id_token.as_deref(), Some("secret-id"));
        assert_eq!(merged.email.as_deref(), Some("ada@example.com"));
        assert_eq!(merged.connected_at, t.connected_at);
        let spun = ImagineTokens {
            refresh_token: Some("spun-refresh".into()),
            ..ImagineTokens {
                access_token: "newer".into(),
                expires_at: Some(now + 3_600_000),
                ..Default::default()
            }
        };
        let merged = merge_imagine_refresh(&t, spun);
        assert_eq!(merged.refresh_token.as_deref(), Some("spun-refresh"));
    }

    #[test]
    fn imagine_error_mapping_oauth_key_429_moderation_video() {
        let oauth = map_imagine_error("HTTP 401: no", ImagineCredKind::OAuth, true);
        assert_eq!(oauth.text, IMAGINE_OAUTH_DENIED);
        assert!(oauth.offer_use_api_key);
        assert!(!oauth.offer_settings);
        let oauth_no = map_imagine_error("HTTP 403: no", ImagineCredKind::OAuth, false);
        assert_eq!(oauth_no.text, IMAGINE_OAUTH_DENIED);
        assert!(oauth_no.offer_settings);
        assert!(!oauth_no.offer_use_api_key);
        let key = map_imagine_error("HTTP 401: Bad credentials.", ImagineCredKind::ConsoleKey, true);
        assert_eq!(key.text, IMAGINE_KEY_REJECTED);
        assert!(!key.offer_use_api_key);
        assert!(!key.offer_settings);
        let key_403 = map_imagine_error("HTTP 403: denied", ImagineCredKind::ConsoleKey, false);
        assert_eq!(key_403.text, IMAGINE_KEY_REJECTED);
        let rate = map_imagine_error("HTTP 429: slow down", ImagineCredKind::OAuth, true);
        assert_eq!(rate.text, IMAGINE_RATE_LIMIT);
        let blocked = map_imagine_error(
            "image blocked by moderation",
            ImagineCredKind::OAuth,
            false,
        );
        assert!(blocked.text.contains("moderation"));
        assert!(!blocked.offer_use_api_key);
        let video = map_imagine_error(
            "video failed (content_policy)",
            ImagineCredKind::ConsoleKey,
            true,
        );
        assert!(video.text.contains("content_policy"), "{}", video.text);
        let stall = map_imagine_error("os error 10060", ImagineCredKind::OAuth, false);
        assert!(
            stall.text.to_ascii_lowercase().contains("api.x.ai"),
            "{}",
            stall.text
        );
        let chain = keychain_unavailable_message("org.freedesktop.secrets missing");
        assert!(chain.to_ascii_lowercase().contains("keychain"), "{chain}");
        assert!(chain.to_ascii_lowercase().contains("api key"), "{chain}");
    }

    #[test]
    fn imagine_bearer_order() {
        let oauth = choose_imagine_bearer(Some("oa-token"), true, "console-key").unwrap();
        assert_eq!(oauth.kind, ImagineCredKind::OAuth);
        assert_eq!(oauth.secret, "oa-token");
        let dead = choose_imagine_bearer(Some("oa-token"), false, "console-key").unwrap();
        assert_eq!(dead.kind, ImagineCredKind::ConsoleKey);
        assert_eq!(dead.secret, "console-key");
        let only = choose_imagine_bearer(None, false, " console-key ").unwrap();
        assert_eq!(only.secret, "console-key");
        let none = choose_imagine_bearer(None, true, "  ").unwrap_err();
        assert_eq!(none, IMAGINE_NEED_SIGNIN);
        assert!(!none.contains("grok login"));
        let expired = ImagineTokens {
            access_token: "old".into(),
            refresh_token: Some("ref".into()),
            expires_at: Some(10),
            connected_at: 1,
            ..Default::default()
        };
        assert!(imagine_oauth_preferred(&expired, 10_000));
        assert!(!imagine_access_usable(&expired, 10_000));
        let gone = ImagineTokens {
            access_token: "old".into(),
            refresh_token: None,
            expires_at: Some(10),
            connected_at: 1,
            ..Default::default()
        };
        assert!(!imagine_oauth_preferred(&gone, 10_000));
    }

    #[test]
    fn imagine_auth_source_never_names_cli_login() {
        let src = concat!(include_str!("xai_signin.rs"), include_str!("imagine_auth.rs"));
        for banned in [
            concat!("grok_cli_", "key"),
            concat!("auth", ".json"),
            concat!("refresh_grok_", "login"),
            concat!("cli-chat-", "proxy"),
        ] {
            assert!(!src.contains(banned), "{banned}");
        }
    }

    struct MemAccounts {
        current: std::sync::Mutex<Option<ImagineTokens>>,
        legacy: std::sync::Mutex<Option<ImagineTokens>>,
        fail_save: bool,
    }

    impl OAuthAccountStore for MemAccounts {
        fn load_account(&self, account: &str) -> Result<Option<ImagineTokens>, String> {
            let slot = if account == XAI_OAUTH_ACCOUNT {
                &self.current
            } else if account == XAI_OAUTH_LEGACY_ACCOUNT {
                &self.legacy
            } else {
                return Ok(None);
            };
            Ok(slot.lock().unwrap_or_else(|e| e.into_inner()).clone())
        }

        fn save_account(&self, account: &str, tokens: &ImagineTokens) -> Result<(), String> {
            if self.fail_save {
                return Err("save failed".into());
            }
            if account == XAI_OAUTH_ACCOUNT {
                *self.current.lock().unwrap_or_else(|e| e.into_inner()) = Some(tokens.clone());
                return Ok(());
            }
            Err("unexpected account".into())
        }

        fn delete_account(&self, account: &str) -> Result<(), String> {
            let slot = if account == XAI_OAUTH_ACCOUNT {
                &self.current
            } else {
                &self.legacy
            };
            *slot.lock().unwrap_or_else(|e| e.into_inner()) = None;
            Ok(())
        }
    }

    fn sample_tokens() -> ImagineTokens {
        ImagineTokens {
            access_token: "access".into(),
            refresh_token: Some("refresh".into()),
            connected_at: 1,
            ..ImagineTokens::default()
        }
    }

    #[test]
    fn xai_signin_keychain_account_moves_once() {
        let store = MemAccounts {
            current: std::sync::Mutex::new(None),
            legacy: std::sync::Mutex::new(Some(sample_tokens())),
            fail_save: true,
        };
        assert!(load_xai_oauth(&store).is_err());
        assert!(store.legacy.lock().unwrap().is_some(), "failed save keeps legacy");
        assert!(store.current.lock().unwrap().is_none());

        let store = MemAccounts {
            current: std::sync::Mutex::new(None),
            legacy: std::sync::Mutex::new(Some(sample_tokens())),
            fail_save: false,
        };
        let loaded = load_xai_oauth(&store).unwrap().unwrap();
        assert_eq!(loaded.access_token, "access");
        assert!(store.legacy.lock().unwrap().is_none());
        assert!(store.current.lock().unwrap().is_some());
        store.legacy.lock().unwrap().replace(ImagineTokens {
            access_token: "old".into(),
            ..ImagineTokens::default()
        });
        let again = load_xai_oauth(&store).unwrap().unwrap();
        assert_eq!(again.access_token, "access");
        assert_eq!(
            store.legacy.lock().unwrap().as_ref().unwrap().access_token,
            "old"
        );
        delete_xai_oauth(&store).unwrap();
        assert!(store.current.lock().unwrap().is_none());
        assert!(store.legacy.lock().unwrap().is_none());
        assert_eq!(
            choose_xai_bearer(None, false, "  ").unwrap_err(),
            XAI_NEED_SIGNIN
        );
        assert_eq!(METER_OAUTH, "SuperGrok pool");
        assert_eq!(METER_API_KEY, "API credits");
    }
}
