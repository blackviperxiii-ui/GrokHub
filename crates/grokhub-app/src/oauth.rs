use grokhub_core::{
    parse_device_start, parse_poll_result, parse_token_json, token_needs_refresh, trusted_xai_url,
    DeviceCodeStart, PollResult, PollStatus, XaiOAuthTokens, XAI_DEVICE_CODE_GRANT,
    XAI_OAUTH_CLIENT_ID, XAI_OAUTH_DISCOVERY, XAI_OAUTH_SCOPE,
};
use serde_json::Value;
use std::time::Duration;

const UA: &str = "GrokHub/2.0.0 (xAI OAuth; Linux)";

struct Discovery {
    device: String,
    token: String,
}

fn form(pairs: &[(&str, &str)]) -> String {
    pairs
        .iter()
        .map(|(k, v)| format!("{}={}", urlencode(k), urlencode(v)))
        .collect::<Vec<_>>()
        .join("&")
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

fn discovery() -> Result<Discovery, String> {
    let resp = ureq::get(XAI_OAUTH_DISCOVERY)
        .set("accept", "application/json")
        .set("user-agent", UA)
        .timeout(Duration::from_secs(20))
        .call()
        .map_err(|e| e.to_string())?;
    let v: Value = resp.into_json().map_err(|e| e.to_string())?;
    let device = v
        .get("device_authorization_endpoint")
        .and_then(|x| x.as_str())
        .ok_or_else(|| "xAI discovery missing device endpoint".to_string())?;
    let token = v
        .get("token_endpoint")
        .and_then(|x| x.as_str())
        .ok_or_else(|| "xAI discovery missing token endpoint".to_string())?;
    Ok(Discovery {
        device: trusted_xai_url(device)?,
        token: trusted_xai_url(token)?,
    })
}

fn post_form(url: &str, body: &str) -> Result<(bool, Value), String> {
    let resp = ureq::post(url)
        .set("content-type", "application/x-www-form-urlencoded")
        .set("accept", "application/json")
        .set("user-agent", UA)
        .timeout(Duration::from_secs(20))
        .send_string(body);
    match resp {
        Ok(r) => {
            let v: Value = r.into_json().map_err(|e| e.to_string())?;
            Ok((true, v))
        }
        Err(ureq::Error::Status(code, r)) => {
            let v: Value = r.into_json().unwrap_or(Value::Null);
            let _ = code;
            Ok((false, v))
        }
        Err(e) => Err(e.to_string()),
    }
}

pub fn start_device() -> Result<DeviceCodeStart, String> {
    let d = discovery()?;
    let body = form(&[
        ("client_id", XAI_OAUTH_CLIENT_ID),
        ("scope", XAI_OAUTH_SCOPE),
    ]);
    let (ok, v) = post_form(&d.device, &body)?;
    if !ok {
        let msg = v
            .get("error_description")
            .or_else(|| v.get("error"))
            .and_then(|x| x.as_str())
            .unwrap_or("device code failed");
        return Err(msg.into());
    }
    parse_device_start(&v)
}

pub fn poll_device(device_code: &str) -> Result<PollResult, String> {
    let d = discovery()?;
    let body = form(&[
        ("grant_type", XAI_DEVICE_CODE_GRANT),
        ("client_id", XAI_OAUTH_CLIENT_ID),
        ("device_code", device_code),
    ]);
    let (ok, v) = post_form(&d.token, &body)?;
    let now = grokhub_core::now_ms();
    Ok(parse_poll_result(ok, &v, now))
}

pub fn refresh_tokens(refresh_token: &str) -> Result<XaiOAuthTokens, String> {
    let d = discovery()?;
    let body = form(&[
        ("grant_type", "refresh_token"),
        ("client_id", XAI_OAUTH_CLIENT_ID),
        ("refresh_token", refresh_token),
    ]);
    let (ok, v) = post_form(&d.token, &body)?;
    if !ok {
        return Err(v
            .get("error_description")
            .or_else(|| v.get("error"))
            .and_then(|x| x.as_str())
            .unwrap_or("refresh failed")
            .into());
    }
    parse_token_json(&v, grokhub_core::now_ms())
}

pub fn ensure_access(tokens: &XaiOAuthTokens) -> Result<(String, XaiOAuthTokens, bool), String> {
    if tokens.access_token.trim().is_empty() {
        return Err("No OAuth access token — Connect Grok OAuth in Settings".into());
    }
    if !token_needs_refresh(tokens, grokhub_core::now_ms()) {
        return Ok((tokens.access_token.clone(), tokens.clone(), false));
    }
    let rt = tokens
        .refresh_token
        .as_ref()
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| "Grok OAuth session expired — sign in again".to_string())?;
    let next = refresh_tokens(rt)?;
    let mut merged = tokens.clone();
    merged.access_token = next.access_token;
    if next.refresh_token.is_some() {
        merged.refresh_token = next.refresh_token;
    }
    merged.expires_at = next.expires_at;
    if next.id_token.is_some() {
        merged.id_token = next.id_token;
    }
    Ok((merged.access_token.clone(), merged, true))
}

pub fn open_browser(url: &str) -> Result<(), String> {
    trusted_xai_url(url)?;
    let _ = std::process::Command::new("xdg-open").arg(url).spawn();
    Ok(())
}

pub fn poll_until_ready(device_code: &str, interval_s: u64) -> Result<XaiOAuthTokens, String> {
    let mut wait = interval_s.max(1);
    for _ in 0..180 {
        std::thread::sleep(Duration::from_secs(wait));
        let r = poll_device(device_code)?;
        match r.status {
            PollStatus::Ready => {
                return r
                    .tokens
                    .ok_or_else(|| "OAuth ready without tokens".to_string())
            }
            PollStatus::SlowDown => wait = wait.saturating_add(2),
            PollStatus::Expired => return Err(r.error.unwrap_or_else(|| "expired".into())),
            PollStatus::Denied => return Err(r.error.unwrap_or_else(|| "denied".into())),
            PollStatus::Pending => {}
        }
    }
    Err("OAuth timed out".into())
}

