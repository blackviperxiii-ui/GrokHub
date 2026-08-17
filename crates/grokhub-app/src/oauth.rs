use grokhub_core::{
    apply_profile, merge_refreshed, parse_device_start, parse_poll_result, parse_token_json,
    parse_userinfo_profile, token_needs_refresh, trusted_profile_photo_url, trusted_xai_url,
    DeviceCodeStart, PollResult, PollStatus, XaiOAuthTokens, XAI_DEVICE_CODE_GRANT,
    XAI_OAUTH_CLIENT_ID, XAI_OAUTH_DISCOVERY, XAI_OAUTH_SCOPE, XAI_OAUTH_USERINFO,
};
use serde_json::Value;
use std::io::Read;
use std::time::Duration;

const UA: &str = concat!("GrokHub/", env!("CARGO_PKG_VERSION"), " (xAI OAuth; Linux)");
const PHOTO_MAX: u64 = 2 * 1024 * 1024;

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
    Ok((
        next.access_token.clone(),
        merge_refreshed(tokens, next),
        true,
    ))
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
                let t = r
                    .tokens
                    .ok_or_else(|| "OAuth ready without tokens".to_string())?;
                return Ok(enrich_tokens(t));
            }
            PollStatus::SlowDown => wait = wait.saturating_add(2),
            PollStatus::Expired => return Err(r.error.unwrap_or_else(|| "expired".into())),
            PollStatus::Denied => return Err(r.error.unwrap_or_else(|| "denied".into())),
            PollStatus::Pending => {}
        }
    }
    Err("OAuth timed out".into())
}

pub fn fetch_userinfo(access: &str) -> Result<grokhub_core::OAuthProfile, String> {
    let url = trusted_xai_url(XAI_OAUTH_USERINFO)?;
    let resp = ureq::get(&url)
        .set("authorization", &format!("Bearer {access}"))
        .set("accept", "application/json")
        .set("user-agent", UA)
        .timeout(Duration::from_secs(20))
        .call()
        .map_err(|e| e.to_string())?;
    let v: Value = resp.into_json().map_err(|e| e.to_string())?;
    Ok(parse_userinfo_profile(&v))
}

pub fn enrich_tokens(tokens: XaiOAuthTokens) -> XaiOAuthTokens {
    let mut t = tokens;
    let picture_ok = t
        .picture
        .as_ref()
        .and_then(|u| trusted_profile_photo_url(u).ok())
        .is_some();
    let name_ok = t.name.as_ref().is_some_and(|s| !s.trim().is_empty());
    let email_ok = t.email.as_ref().is_some_and(|s| !s.trim().is_empty());
    if picture_ok && name_ok && email_ok {
        return t;
    }
    if let Ok(profile) = fetch_userinfo(&t.access_token) {
        apply_profile(&mut t, &profile);
    }
    t
}

pub fn fetch_profile_photo(url: &str, access: &str) -> Result<Vec<u8>, String> {
    let url = trusted_profile_photo_url(url)?;
    let host = url
        .strip_prefix("https://")
        .unwrap_or("")
        .split('/')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    let mut req = ureq::get(&url)
        .set(
            "accept",
            "image/avif,image/webp,image/png,image/jpeg,image/*;q=0.8",
        )
        .set("user-agent", UA)
        .timeout(Duration::from_secs(20));
    if host == "x.ai"
        || host.ends_with(".x.ai")
        || host == "grok.com"
        || host.ends_with(".grok.com")
    {
        req = req
            .set("authorization", &format!("Bearer {access}"))
            .set("referer", "https://grok.com/");
    }
    let resp = req.call().map_err(|e| e.to_string())?;
    let mut buf = Vec::new();
    resp.into_reader()
        .take(PHOTO_MAX + 1)
        .read_to_end(&mut buf)
        .map_err(|e| e.to_string())?;
    if buf.len() as u64 > PHOTO_MAX {
        return Err("Profile photo too large".into());
    }
    if buf.is_empty() {
        return Err("Empty profile photo".into());
    }
    Ok(buf)
}

pub fn avatar_rgba(bytes: &[u8]) -> Option<image::RgbaImage> {
    let img = image::load_from_memory(bytes).ok()?.to_rgba8();
    Some(center_square(img))
}

fn center_square(img: image::RgbaImage) -> image::RgbaImage {
    let w = img.width();
    let h = img.height();
    if w == 0 || h == 0 || w == h {
        return img;
    }
    let side = w.min(h);
    let x = (w - side) / 2;
    let y = (h - side) / 2;
    image::imageops::crop_imm(&img, x, y, side, side).to_image()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png_bytes(img: image::RgbaImage) -> Vec<u8> {
        let mut buf = Vec::new();
        image::DynamicImage::ImageRgba8(img)
            .write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Png)
            .expect("png");
        buf
    }

    #[test]
    fn avatar_rgba_center_crops_to_square() {
        let img = image::RgbaImage::from_pixel(8, 4, image::Rgba([200, 40, 10, 255]));
        let out = avatar_rgba(&png_bytes(img)).expect("decode");
        assert_eq!(out.width(), 4);
        assert_eq!(out.height(), 4);
        assert_eq!(out.get_pixel(0, 0).0, [200, 40, 10, 255]);
    }

    #[test]
    fn avatar_rgba_rejects_garbage() {
        assert!(avatar_rgba(b"not-an-image").is_none());
    }

    #[test]
    fn oauth_user_agent_is_2_6_3() {
        assert!(
            UA.contains("GrokHub/2.6.3"),
            "oauth UA must track the cabin version, got {UA}"
        );
    }
}

