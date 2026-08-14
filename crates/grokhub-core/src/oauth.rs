//! xAI Grok OAuth — same public device-code client as Grok CLI / the Electron cabin.

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const XAI_OAUTH_CLIENT_ID: &str = "b1a00492-073a-47ea-816f-4c329264a828";
pub const XAI_OAUTH_SCOPE: &str =
    "openid profile email offline_access grok-cli:access api:access";
pub const XAI_OAUTH_ISSUER: &str = "https://auth.x.ai";
pub const XAI_OAUTH_DISCOVERY: &str = "https://auth.x.ai/.well-known/openid-configuration";
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
    pub connected_at: u64,
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

#[derive(Debug, Clone, PartialEq, Eq)]
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

pub fn has_auth(api_key: &str, access_token: &str) -> bool {
    !api_key.trim().is_empty() || !access_token.trim().is_empty()
}

pub fn auth_bearer(api_key: &str, access_token: &str) -> Option<String> {
    let key = api_key.trim();
    if !key.is_empty() {
        return Some(key.to_string());
    }
    let tok = access_token.trim();
    if !tok.is_empty() {
        return Some(tok.to_string());
    }
    None
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
    tokens.connected_at > 0 && now_ms.saturating_sub(tokens.connected_at) >= TOKEN_MAX_AGE_WITHOUT_EXP_MS
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
    if let Some(id) = &id_token {
        if let Some(claims) = decode_jwt_payload(id) {
            email = claims.get("email").and_then(|v| v.as_str()).map(|s| s.to_string());
            name = claims
                .get("name")
                .or_else(|| claims.get("preferred_username"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
        }
    }
    Ok(XaiOAuthTokens {
        access_token,
        refresh_token,
        expires_at,
        id_token,
        email,
        name,
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
                status: PollStatus::Pending,
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
            status: PollStatus::Pending,
            tokens: None,
            error: json
                .get("error_description")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .or_else(|| Some(format!("waiting ({err})"))),
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
        assert_eq!(auth_bearer("xai-k", "tok").as_deref(), Some("xai-k"));
        assert_eq!(auth_bearer("", "tok").as_deref(), Some("tok"));
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
    }

    fn now_far() -> u64 {
        9_999_999_999
    }
}
