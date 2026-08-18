//! xAI Grok OAuth — same public device-code client as Grok CLI / the Electron cabin.

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const XAI_OAUTH_CLIENT_ID: &str = "b1a00492-073a-47ea-816f-4c329264a828";
pub const XAI_OAUTH_SCOPE: &str =
    "openid profile email offline_access grok-cli:access api:access";
pub const XAI_OAUTH_ISSUER: &str = "https://auth.x.ai";
pub const XAI_OAUTH_DISCOVERY: &str = "https://auth.x.ai/.well-known/openid-configuration";
pub const XAI_OAUTH_USERINFO: &str = "https://auth.x.ai/oauth2/userinfo";
pub const XAI_DEVICE_CODE_GRANT: &str = "urn:ietf:params:oauth:grant-type:device_code";
pub const TOKEN_REFRESH_SKEW_MS: u64 = 30 * 60 * 1000;
pub const TOKEN_MAX_AGE_WITHOUT_EXP_MS: u64 = 5 * 60 * 60 * 1000;

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct XaiOAuthTokens {
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
    pub name: Option<String>,
    #[serde(default)]
    pub picture: Option<String>,
    #[serde(default)]
    pub connected_at: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OAuthProfile {
    pub name: Option<String>,
    pub email: Option<String>,
    pub picture: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceCodeStart {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub verification_uri_complete: Option<String>,
    pub expires_in: u64,
    pub interval: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PollStatus {
    Pending,
    SlowDown,
    Expired,
    Denied,
    Ready,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PollResult {
    pub status: PollStatus,
    pub tokens: Option<XaiOAuthTokens>,
    pub error: Option<String>,
}

/// Seconds to wait before the next device-code poll. `None` means stop.
pub fn next_oauth_poll_secs(interval: u64, status: PollStatus) -> Option<u64> {
    let interval = interval.max(1);
    match status {
        PollStatus::Pending => Some(interval),
        PollStatus::SlowDown => Some(interval.saturating_add(5)),
        PollStatus::Ready | PollStatus::Expired | PollStatus::Denied => None,
    }
}

pub fn trusted_xai_url(url: &str) -> Result<String, String> {
    let rest = url
        .strip_prefix("https://")
        .ok_or_else(|| "xAI OAuth requires https".to_string())?;
    let host = rest.split('/').next().unwrap_or("");
    if host == "x.ai" || host.ends_with(".x.ai") {
        Ok(url.to_string())
    } else {
        Err(format!("Untrusted xAI host: {host}"))
    }
}

pub fn trusted_profile_photo_url(url: &str) -> Result<String, String> {
    let url = url.trim();
    let rest = url
        .strip_prefix("https://")
        .ok_or_else(|| "Profile photo requires https".to_string())?;
    let host = rest
        .split('/')
        .next()
        .unwrap_or("")
        .split(':')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    if profile_photo_host_ok(&host) {
        Ok(url.to_string())
    } else {
        Err(format!("Untrusted profile photo host: {host}"))
    }
}

fn profile_photo_host_ok(host: &str) -> bool {
    host == "x.ai"
        || host.ends_with(".x.ai")
        || host == "grok.com"
        || host.ends_with(".grok.com")
        || host == "pbs.twimg.com"
        || host.ends_with(".twimg.com")
        || host == "googleusercontent.com"
        || host.ends_with(".googleusercontent.com")
        || host == "x.com"
        || host.ends_with(".x.com")
        || host == "twitter.com"
        || host.ends_with(".twitter.com")
}

fn claim_text(v: &Value, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|k| {
        v.get(*k)
            .and_then(|x| x.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
    })
}

fn picture_from_claims(v: &Value) -> Option<String> {
    let raw = claim_text(
        v,
        &[
            "picture",
            "profile_image_url",
            "profileImageUrl",
            "avatar",
            "avatar_url",
            "avatarUrl",
            "picture_url",
            "image",
        ],
    )?;
    trusted_profile_photo_url(&raw).ok()
}

pub fn parse_userinfo_profile(json: &Value) -> OAuthProfile {
    let mut name = claim_text(json, &["name", "preferred_username"]);
    if name.is_none() {
        let first = claim_text(json, &["given_name", "first_name", "firstName"]);
        let last = claim_text(json, &["family_name", "last_name", "lastName"]);
        name = match (first, last) {
            (Some(a), Some(b)) => Some(format!("{a} {b}")),
            (Some(a), None) => Some(a),
            (None, Some(b)) => Some(b),
            (None, None) => None,
        };
    }
    OAuthProfile {
        name,
        email: claim_text(json, &["email"]),
        picture: picture_from_claims(json),
    }
}

pub fn apply_profile(tokens: &mut XaiOAuthTokens, profile: &OAuthProfile) {
    if let Some(name) = profile.name.clone() {
        tokens.name = Some(name);
    }
    if let Some(email) = profile.email.clone() {
        tokens.email = Some(email);
    }
    if let Some(picture) = profile.picture.clone() {
        tokens.picture = Some(picture);
    }
}

pub fn merge_refreshed(prev: &XaiOAuthTokens, mut next: XaiOAuthTokens) -> XaiOAuthTokens {
    if next.refresh_token.is_none() {
        next.refresh_token = prev.refresh_token.clone();
    }
    if next.id_token.is_none() {
        next.id_token = prev.id_token.clone();
    }
    if next.email.is_none() {
        next.email = prev.email.clone();
    }
    if next.name.is_none() {
        next.name = prev.name.clone();
    }
    if next.picture.is_none() {
        next.picture = prev.picture.clone();
    }
    next.connected_at = prev.connected_at;
    next
}

pub fn has_auth(api_key: &str, access_token: &str) -> bool {
    !api_key.trim().is_empty() || !access_token.trim().is_empty()
}

pub fn auth_bearer(api_key: &str, access_token: &str, oauth_usable: bool) -> Option<String> {
    chat_bearer(api_key, access_token, oauth_usable)
}

/// When OAuth refresh failed, a live console key must still send chat.
pub fn chat_bearer(api_key: &str, access_token: &str, oauth_usable: bool) -> Option<String> {
    if oauth_usable {
        let tok = access_token.trim();
        if !tok.is_empty() {
            return Some(tok.to_string());
        }
    }
    let key = api_key.trim();
    if !key.is_empty() {
        return Some(key.to_string());
    }
    None
}

/// Duplex Voice (`wss://api.x.ai/v1/realtime`) is console-key only. OAuth does not grant it.
pub fn realtime_bearer(api_key: &str, _access_token: &str) -> Option<String> {
    let key = api_key.trim();
    if key.is_empty() {
        None
    } else {
        Some(key.to_string())
    }
}

/// Expired access with no refresh token is dead — do not prefer it over a console key.
pub fn oauth_access_live(tokens: &XaiOAuthTokens, now_ms: u64) -> bool {
    if tokens.access_token.trim().is_empty() {
        return false;
    }
    let exp = tokens
        .expires_at
        .or_else(|| jwt_exp_ms(&tokens.access_token));
    if let Some(exp) = exp {
        return exp > now_ms;
    }
    if tokens.connected_at > 0 {
        return now_ms.saturating_sub(tokens.connected_at) < TOKEN_MAX_AGE_WITHOUT_EXP_MS;
    }
    false
}

pub fn token_needs_refresh(tokens: &XaiOAuthTokens, now_ms: u64) -> bool {
    if tokens.access_token.trim().is_empty() {
        return false;
    }
    if tokens
        .refresh_token
        .as_ref()
        .map(|s| s.trim().is_empty())
        .unwrap_or(true)
    {
        return false;
    }
    let exp = tokens
        .expires_at
        .or_else(|| jwt_exp_ms(&tokens.access_token));
    if let Some(exp) = exp {
        return exp.saturating_sub(TOKEN_REFRESH_SKEW_MS) < now_ms;
    }
    if tokens.connected_at == 0 {
        return true;
    }
    now_ms.saturating_sub(tokens.connected_at) >= TOKEN_MAX_AGE_WITHOUT_EXP_MS
}

pub fn jwt_exp_ms(token: &str) -> Option<u64> {
    let payload = decode_jwt_payload(token)?;
    payload.get("exp")?.as_u64().map(|s| s.saturating_mul(1000))
}

pub fn decode_jwt_payload(token: &str) -> Option<Value> {
    let part = token.split('.').nth(1)?;
    let bytes = b64url_decode(part)?;
    serde_json::from_slice(&bytes).ok()
}

pub fn parse_device_start(json: &Value) -> Result<DeviceCodeStart, String> {
    let device_code = json
        .get("device_code")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let user_code = json
        .get("user_code")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let verification_uri = json
        .get("verification_uri")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    if device_code.is_empty() || user_code.is_empty() || verification_uri.is_empty() {
        return Err("Invalid device code response from xAI".into());
    }
    trusted_xai_url(&verification_uri)?;
    let complete = json
        .get("verification_uri_complete")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    if let Some(c) = &complete {
        trusted_xai_url(c)?;
    }
    Ok(DeviceCodeStart {
        device_code,
        user_code,
        verification_uri,
        verification_uri_complete: complete,
        expires_in: json.get("expires_in").and_then(|v| v.as_u64()).unwrap_or(1800),
        interval: json.get("interval").and_then(|v| v.as_u64()).unwrap_or(5),
    })
}

pub fn parse_token_json(json: &Value, now_ms: u64) -> Result<XaiOAuthTokens, String> {
    let access_token = json
        .get("access_token")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "Token response missing access_token".to_string())?
        .to_string();
    let refresh_token = json
        .get("refresh_token")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());
    let id_token = json
        .get("id_token")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());
    let mut expires_at = json.get("expires_in").and_then(|v| {
        v.as_u64()
            .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
            .map(|secs| now_ms.saturating_add(secs.saturating_mul(1000)))
    });
    if let Some(jwt) = jwt_exp_ms(&access_token) {
        expires_at = Some(expires_at.map(|e| e.min(jwt)).unwrap_or(jwt));
    }
    let mut email = None;
    let mut name = None;
    let mut picture = None;
    if let Some(id) = &id_token {
        if let Some(claims) = decode_jwt_payload(id) {
            let profile = parse_userinfo_profile(&claims);
            email = profile.email;
            name = profile.name;
            picture = profile.picture;
        }
    }
    Ok(XaiOAuthTokens {
        access_token,
        refresh_token,
        expires_at,
        id_token,
        email,
        name,
        picture,
        connected_at: now_ms,
    })
}

pub fn parse_poll_result(ok: bool, json: &Value, now_ms: u64) -> PollResult {
    if ok && json.get("access_token").and_then(|v| v.as_str()).is_some() {
        return match parse_token_json(json, now_ms) {
            Ok(tokens) => PollResult {
                status: PollStatus::Ready,
                tokens: Some(tokens),
                error: None,
            },
            Err(e) => PollResult {
                status: PollStatus::Denied,
                tokens: None,
                error: Some(e),
            },
        };
    }
    let err = json
        .get("error")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");
    match err {
        "authorization_pending" => PollResult {
            status: PollStatus::Pending,
            tokens: None,
            error: Some(err.into()),
        },
        "slow_down" => PollResult {
            status: PollStatus::SlowDown,
            tokens: None,
            error: None,
        },
        "expired_token" => PollResult {
            status: PollStatus::Expired,
            tokens: None,
            error: json
                .get("error_description")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .or_else(|| Some(err.into())),
        },
        "access_denied" => PollResult {
            status: PollStatus::Denied,
            tokens: None,
            error: Some(err.into()),
        },
        _ => PollResult {
            status: PollStatus::Denied,
            tokens: None,
            error: json
                .get("error_description")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .or_else(|| Some(format!("oauth error ({err})"))),
        },
    }
}

fn b64url_decode(s: &str) -> Option<Vec<u8>> {
    let mut t = s.replace('-', "+").replace('_', "/");
    while t.len() % 4 != 0 {
        t.push('=');
    }
    let table = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = Vec::new();
    let bytes = t.as_bytes();
    let mut i = 0;
    while i + 3 < bytes.len() {
        let mut n = 0u32;
        let mut pad = 0;
        for k in 0..4 {
            let c = bytes[i + k];
            if c == b'=' {
                pad += 1;
                n <<= 6;
                continue;
            }
            let v = table.iter().position(|x| *x == c)? as u32;
            n = (n << 6) | v;
        }
        out.push(((n >> 16) & 0xff) as u8);
        if pad < 2 {
            out.push(((n >> 8) & 0xff) as u8);
        }
        if pad < 1 {
            out.push((n & 0xff) as u8);
        }
        i += 4;
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn trusted_hosts_and_bearer() {
        assert!(trusted_xai_url("https://auth.x.ai/oauth2/token").is_ok());
        assert!(trusted_xai_url("http://auth.x.ai/x").is_err());
        assert!(trusted_xai_url("https://evil.com").is_err());
        assert!(has_auth("", "tok"));
        assert!(has_auth("xai-k", ""));
        assert!(!has_auth("", ""));
        assert_eq!(auth_bearer("xai-k", "tok", true).as_deref(), Some("tok"));
        assert_eq!(auth_bearer("", "tok", true).as_deref(), Some("tok"));
        assert_eq!(auth_bearer("xai-k", "", true).as_deref(), Some("xai-k"));
        assert_eq!(
            auth_bearer("xai-live", "expired-oauth", false).as_deref(),
            Some("xai-live"),
            "dead OAuth must not beat a console key for STT/TTS"
        );
        assert_eq!(
            chat_bearer("xai-k", "expired-tok", false).as_deref(),
            Some("xai-k"),
            "dead OAuth must not beat a console key"
        );
        assert_eq!(chat_bearer("", "expired-tok", false), None);
        assert_eq!(realtime_bearer("xai-k", "tok").as_deref(), Some("xai-k"));
        assert_eq!(realtime_bearer("", "tok"), None);
        assert_eq!(realtime_bearer("", ""), None);
    }

    #[test]
    fn device_and_poll() {
        let start = parse_device_start(&json!({
            "device_code": "dev",
            "user_code": "ABCD-EFGH",
            "verification_uri": "https://auth.x.ai/device",
            "verification_uri_complete": "https://auth.x.ai/device?user_code=ABCD-EFGH",
            "expires_in": 900,
            "interval": 5
        }))
        .unwrap();
        assert_eq!(start.user_code, "ABCD-EFGH");
        let pending = parse_poll_result(false, &json!({"error":"authorization_pending"}), 1);
        assert_eq!(pending.status, PollStatus::Pending);
        let ready = parse_poll_result(
            true,
            &json!({"access_token":"tok","refresh_token":"ref","expires_in":3600}),
            1_000,
        );
        assert_eq!(ready.status, PollStatus::Ready);
        let bad = parse_poll_result(true, &json!({"access_token": ""}), 1);
        assert_eq!(
            bad.status,
            PollStatus::Denied,
            "malformed success must not spin forever"
        );
        let grant = parse_poll_result(false, &json!({"error":"invalid_grant"}), 1);
        assert_eq!(
            grant.status,
            PollStatus::Denied,
            "unknown token errors must not poll forever"
        );
        let t = ready.tokens.unwrap();
        assert_eq!(t.access_token, "tok");
        assert!(token_needs_refresh(
            &XaiOAuthTokens {
                access_token: "tok".into(),
                refresh_token: Some("ref".into()),
                expires_at: Some(10_000),
                connected_at: 1,
                ..Default::default()
            },
            9_000
        ));
        assert!(!token_needs_refresh(
            &XaiOAuthTokens {
                access_token: "tok".into(),
                refresh_token: Some("ref".into()),
                expires_at: Some(now_far()),
                connected_at: 1,
                ..Default::default()
            },
            1_000
        ));
        let dead = XaiOAuthTokens {
            access_token: "dead-oauth".into(),
            refresh_token: None,
            expires_at: Some(10),
            connected_at: 1,
            ..Default::default()
        };
        assert!(
            !oauth_access_live(&dead, 1_000),
            "expired OAuth without refresh must not beat a console key"
        );
        assert!(!token_needs_refresh(&dead, 1_000));
        assert_eq!(
            chat_bearer("xai-live-key", "dead-oauth", oauth_access_live(&dead, 1_000))
                .as_deref(),
            Some("xai-live-key")
        );
        let opaque = XaiOAuthTokens {
            access_token: "dead-opaque".into(),
            refresh_token: Some("ref".into()),
            expires_at: None,
            connected_at: 0,
            ..Default::default()
        };
        assert!(
            token_needs_refresh(&opaque, 9_000_000_000),
            "legacy opaque tokens with no expiry metadata must refresh"
        );
        assert!(
            !oauth_access_live(&opaque, 9_000_000_000),
            "unknown-age OAuth must not beat a console key"
        );
        assert_eq!(
            chat_bearer("xai-live-key", "dead-opaque", oauth_access_live(&opaque, 9_000_000_000))
                .as_deref(),
            Some("xai-live-key")
        );
    }

    #[test]
    fn device_poll_honors_interval_and_slow_down() {
        assert_eq!(next_oauth_poll_secs(5, PollStatus::Pending), Some(5));
        assert_eq!(next_oauth_poll_secs(0, PollStatus::Pending), Some(1));
        assert_eq!(
            next_oauth_poll_secs(5, PollStatus::SlowDown),
            Some(10),
            "RFC 8628 increases the interval by 5 seconds"
        );
        assert_eq!(next_oauth_poll_secs(5, PollStatus::Ready), None);
        assert_eq!(next_oauth_poll_secs(5, PollStatus::Expired), None);
        assert_eq!(next_oauth_poll_secs(5, PollStatus::Denied), None);
        let slow = parse_poll_result(false, &json!({"error": "slow_down"}), 1);
        assert_eq!(slow.status, PollStatus::SlowDown);
        assert_eq!(next_oauth_poll_secs(2, slow.status), Some(7));
    }

    fn now_far() -> u64 {
        9_999_999_999
    }

    fn jwt_from_payload(payload: &str) -> String {
        format!("eyJhbGciOiJub25lIn0.{}.sig", b64url_encode(payload.as_bytes()))
    }

    fn b64url_encode(data: &[u8]) -> String {
        const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
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
        out.replace('+', "-").replace('/', "_")
    }

    #[test]
    fn id_token_picture_claim() {
        let jwt = jwt_from_payload(
            r#"{"email":"viper@x.ai","name":"Viper","picture":"https://assets.grok.com/users/viper.png"}"#,
        );
        let t = parse_token_json(
            &json!({"access_token":"tok","id_token": jwt}),
            1,
        )
        .unwrap();
        assert_eq!(t.email.as_deref(), Some("viper@x.ai"));
        assert_eq!(t.name.as_deref(), Some("Viper"));
        assert_eq!(
            t.picture.as_deref(),
            Some("https://assets.grok.com/users/viper.png")
        );
    }

    #[test]
    fn userinfo_profile_picture() {
        let p = parse_userinfo_profile(&json!({
            "sub": "u1",
            "name": "Viper",
            "email": "viper@x.ai",
            "picture": "https://pbs.twimg.com/profile_images/viper.jpg"
        }));
        assert_eq!(p.name.as_deref(), Some("Viper"));
        assert_eq!(p.email.as_deref(), Some("viper@x.ai"));
        assert_eq!(
            p.picture.as_deref(),
            Some("https://pbs.twimg.com/profile_images/viper.jpg")
        );
        let avatar = parse_userinfo_profile(&json!({
            "avatar_url": "https://x.com/users/viper.png"
        }));
        assert_eq!(
            avatar.picture.as_deref(),
            Some("https://x.com/users/viper.png")
        );
    }

    #[test]
    fn userinfo_rejects_untrusted_picture_host() {
        let p = parse_userinfo_profile(&json!({
            "picture": "https://evil.com/x.png"
        }));
        assert!(p.picture.is_none());
    }

    #[test]
    fn trusted_profile_photo_hosts() {
        assert!(trusted_profile_photo_url("https://assets.grok.com/users/viper.png").is_ok());
        assert!(trusted_profile_photo_url("https://auth.x.ai/avatar/viper.png").is_ok());
        assert!(trusted_profile_photo_url("https://pbs.twimg.com/profile_images/viper.jpg").is_ok());
        assert!(trusted_profile_photo_url(
            "https://lh3.googleusercontent.com/a/viper"
        )
        .is_ok());
        assert!(trusted_profile_photo_url("https://x.com/users/viper.png").is_ok());
        assert!(trusted_profile_photo_url("https://evil.com/x.png").is_err());
        assert!(trusted_profile_photo_url("http://assets.grok.com/users/viper.png").is_err());
    }

    #[test]
    fn refresh_keeps_oauth_picture() {
        let prev = XaiOAuthTokens {
            access_token: "old".into(),
            refresh_token: Some("ref".into()),
            email: Some("viper@x.ai".into()),
            name: Some("Viper".into()),
            picture: Some("https://assets.grok.com/users/viper.png".into()),
            connected_at: 9,
            ..Default::default()
        };
        let next = XaiOAuthTokens {
            access_token: "new".into(),
            refresh_token: Some("ref2".into()),
            connected_at: 10,
            ..Default::default()
        };
        let merged = merge_refreshed(&prev, next);
        assert_eq!(merged.access_token, "new");
        assert_eq!(merged.refresh_token.as_deref(), Some("ref2"));
        assert_eq!(merged.picture.as_deref(), Some("https://assets.grok.com/users/viper.png"));
        assert_eq!(merged.name.as_deref(), Some("Viper"));
        assert_eq!(merged.email.as_deref(), Some("viper@x.ai"));
    }

    #[test]
    fn apply_profile_fills_picture() {
        let mut t = XaiOAuthTokens {
            access_token: "tok".into(),
            ..Default::default()
        };
        apply_profile(
            &mut t,
            &OAuthProfile {
                name: Some("Viper".into()),
                email: Some("viper@x.ai".into()),
                picture: Some("https://assets.grok.com/users/viper.png".into()),
            },
        );
        assert_eq!(t.name.as_deref(), Some("Viper"));
        assert_eq!(t.picture.as_deref(), Some("https://assets.grok.com/users/viper.png"));
    }
}
