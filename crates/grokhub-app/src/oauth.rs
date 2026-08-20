use grokhub_core::{
    apply_profile, merge_refreshed, parse_device_start, parse_poll_result, parse_token_json,
    TEXT_FILE_CAP,
    parse_userinfo_profile, token_needs_refresh, trusted_profile_photo_url, trusted_xai_url,
    DeviceCodeStart, PollResult, PollStatus, XaiOAuthTokens, XAI_DEVICE_CODE_GRANT,
    XAI_OAUTH_CLIENT_ID, XAI_OAUTH_DISCOVERY, XAI_OAUTH_SCOPE, XAI_OAUTH_USERINFO,
};
use serde_json::Value;
use std::io::Read;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

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

fn read_json_capped(resp: ureq::Response) -> Result<Value, String> {
    let mut buf = Vec::new();
    resp.into_reader()
        .take(TEXT_FILE_CAP as u64)
        .read_to_end(&mut buf)
        .map_err(|e| e.to_string())?;
    serde_json::from_slice(&buf).map_err(|e| e.to_string())
}

fn discovery() -> Result<Discovery, String> {
    let resp = ureq::get(XAI_OAUTH_DISCOVERY)
        .set("accept", "application/json")
        .set("user-agent", UA)
        .timeout(Duration::from_secs(20))
        .call()
        .map_err(|e| e.to_string())?;
    let v = read_json_capped(resp)?;
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
            let v = read_json_capped(r)?;
            Ok((true, v))
        }
        Err(ureq::Error::Status(code, r)) => {
            let v = read_json_capped(r).unwrap_or(Value::Null);
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

/// Refresh a `grok login` JWT (CLI client id, not cabin OAuth) and write it back.
struct GrokLoginRefresh {
    fail_at: Option<Instant>,
    inflight: bool,
    ready: Option<String>,
}

fn grok_login_refresh_state() -> &'static Mutex<GrokLoginRefresh> {
    static C: OnceLock<Mutex<GrokLoginRefresh>> = OnceLock::new();
    C.get_or_init(|| {
        Mutex::new(GrokLoginRefresh {
            fail_at: None,
            inflight: false,
            ready: None,
        })
    })
}

pub fn refresh_grok_login() -> Option<String> {
    let mut held = grok_login_refresh_state().lock().ok()?;
    if let Some(tok) = held.ready.take() {
        return Some(tok);
    }
    if let Some(at) = held.fail_at {
        if at.elapsed() < Duration::from_secs(30) {
            return None;
        }
    }
    if held.inflight {
        return None;
    }
    held.inflight = true;
    drop(held);
    std::thread::spawn(|| {
        let out = refresh_grok_login_now();
        if let Ok(mut held) = grok_login_refresh_state().lock() {
            held.inflight = false;
            if out.is_some() {
                held.ready = out;
                held.fail_at = None;
            } else {
                held.fail_at = Some(Instant::now());
            }
        }
    });
    None
}

struct CabinOauthRefresh {
    fail_at: Option<Instant>,
    inflight: bool,
    ready: Option<XaiOAuthTokens>,
}

fn cabin_oauth_refresh_state() -> &'static Mutex<CabinOauthRefresh> {
    static C: OnceLock<Mutex<CabinOauthRefresh>> = OnceLock::new();
    C.get_or_init(|| {
        Mutex::new(CabinOauthRefresh {
            fail_at: None,
            inflight: false,
            ready: None,
        })
    })
}

/// Refresh cabin OAuth off the UI thread. Returns a completed refresh if one is ready.
pub fn refresh_cabin_oauth(tokens: &XaiOAuthTokens) -> Option<XaiOAuthTokens> {
    let mut held = cabin_oauth_refresh_state().lock().ok()?;
    if let Some(tok) = held.ready.take() {
        return Some(tok);
    }
    if let Some(at) = held.fail_at {
        if at.elapsed() < Duration::from_secs(30) {
            return None;
        }
    }
    if held.inflight {
        return None;
    }
    held.inflight = true;
    let snap = tokens.clone();
    drop(held);
    std::thread::spawn(move || {
        let out = ensure_access(&snap)
            .ok()
            .and_then(|(_, next, refreshed)| if refreshed { Some(next) } else { None });
        if let Ok(mut held) = cabin_oauth_refresh_state().lock() {
            held.inflight = false;
            if out.is_some() {
                held.ready = out;
                held.fail_at = None;
            } else {
                held.fail_at = Some(Instant::now());
            }
        }
    });
    None
}

fn refresh_grok_login_now() -> Option<String> {
    let path = grokhub_acp::grok_auth_path()?;
    let mut raw = String::new();
    std::fs::File::open(&path)
        .ok()?
        .take(TEXT_FILE_CAP as u64)
        .read_to_string(&mut raw)
        .ok()?;
    if raw.is_empty() {
        return None;
    }
    let mut v: Value = serde_json::from_str(&raw).ok()?;
    let obj = v.as_object_mut()?;
    let mut slot: Option<String> = None;
    let mut refresh = String::new();
    let mut client_id = String::new();
    let mut best_exp = String::new();
    for (k, rec) in obj.iter() {
        let rt = rec
            .get("refresh_token")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .trim();
        if rt.is_empty() {
            continue;
        }
        let exp = rec
            .get("expires_at")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string();
        let take = slot.is_none() || exp > best_exp;
        if take {
            best_exp = exp;
            refresh = rt.to_string();
            client_id = rec
                .get("oidc_client_id")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string();
            if client_id.is_empty() {
                if let Some((_, id)) = k.rsplit_once("::") {
                    client_id = id.to_string();
                }
            }
            slot = Some(k.clone());
        }
    }
    let slot = slot?;
    if client_id.is_empty() || refresh.is_empty() {
        return None;
    }
    let d = discovery().ok()?;
    let body = form(&[
        ("grant_type", "refresh_token"),
        ("client_id", &client_id),
        ("refresh_token", &refresh),
    ]);
    let (ok, tok) = post_form(&d.token, &body).ok()?;
    if !ok {
        return None;
    }
    let access = tok.get("access_token")?.as_str()?.trim().to_string();
    if access.is_empty() {
        return None;
    }
    if let Some(rec) = v.get_mut(&slot).and_then(|x| x.as_object_mut()) {
        rec.insert("key".into(), Value::String(access.clone()));
        if let Some(rt) = tok.get("refresh_token").and_then(|x| x.as_str()) {
            if !rt.trim().is_empty() {
                rec.insert("refresh_token".into(), Value::String(rt.to_string()));
            }
        }
    }
    let out = serde_json::to_string_pretty(&v).ok()?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, out).ok()?;
    std::fs::rename(&tmp, &path).ok()?;
    Some(access)
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
            PollStatus::SlowDown | PollStatus::Pending => {
                if let Some(next) = grokhub_core::next_oauth_poll_secs(wait, r.status) {
                    wait = next;
                }
            }
            PollStatus::Expired => return Err(r.error.unwrap_or_else(|| "expired".into())),
            PollStatus::Denied => return Err(r.error.unwrap_or_else(|| "denied".into())),
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
    let v = read_json_capped(resp)?;
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
    if !crate::desktop::image_pixels_ok_for_bytes(bytes) {
        return None;
    }
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
    fn avatar_rgba_rejects_a_pixel_bomb() {
        let src = include_str!("oauth.rs");
        let av = src
            .split("pub fn avatar_rgba(")
            .nth(1)
            .and_then(|s| s.split("fn center_square(").next())
            .expect("avatar_rgba");
        assert!(
            av.contains("image_pixels_ok") || av.contains("png_ihdr_size"),
            "Settings avatar must not decode a pixel bomb: {av}"
        );
        let mut hdr = vec![0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a];
        hdr.extend_from_slice(&13u32.to_be_bytes());
        hdr.extend_from_slice(b"IHDR");
        hdr.extend_from_slice(&50_000u32.to_be_bytes());
        hdr.extend_from_slice(&50_000u32.to_be_bytes());
        assert!(avatar_rgba(&hdr).is_none());
    }

    #[test]
    fn oauth_json_does_not_slurp_a_huge_body() {
        let src = include_str!("oauth.rs");
        let helper = src
            .split("fn read_json_capped(")
            .nth(1)
            .and_then(|s| s.split("fn discovery(").next())
            .expect("read_json_capped");
        assert!(
            helper.contains(".take(") && helper.contains("TEXT_FILE_CAP"),
            "OAuth JSON must stop at TEXT_FILE_CAP: {helper}"
        );
        for (name, next) in [
            ("fn discovery(", "fn post_form("),
            ("fn post_form(", "pub fn start_device("),
            ("pub fn fetch_userinfo(", "pub fn enrich_tokens("),
        ] {
            let slice = src
                .split(name)
                .nth(1)
                .and_then(|s| s.split(next).next())
                .unwrap_or(name);
            assert!(
                slice.contains("read_json_capped") && !slice.contains("into_json()"),
                "OAuth on the UI thread must not slurp a huge JSON body: {name} {slice}"
            );
        }
    }

    #[test]
    fn oauth_user_agent_tracks_cabin_version() {
        assert!(
            UA.contains(concat!("GrokHub/", env!("CARGO_PKG_VERSION"))),
            "oauth UA must track the cabin version, got {UA}"
        );
    }

    #[test]
    fn grok_login_refresh_does_not_hammer_the_ui_thread() {
        let src = include_str!("oauth.rs");
        let wrap = src
            .split("pub fn refresh_grok_login(")
            .nth(1)
            .and_then(|s| s.split("fn refresh_grok_login_now(").next())
            .expect("refresh_grok_login");
        assert!(
            wrap.contains("elapsed") && wrap.contains("from_secs(30)"),
            "a failed grok login refresh must not retry every chip/Imagine paint: {wrap}"
        );
        assert!(
            wrap.contains("thread::spawn") && wrap.contains("refresh_grok_login_now"),
            "grok login refresh HTTP must leave the UI thread: {wrap}"
        );
        let now = src
            .split("fn refresh_grok_login_now(")
            .nth(1)
            .and_then(|s| s.split("pub fn ensure_access(").next())
            .expect("refresh_grok_login_now");
        assert!(
            now.contains("TEXT_FILE_CAP") && now.contains(".take(") && !now.contains("read_to_string(&path)"),
            "grok login refresh must not slurp a huge auth.json: {now}"
        );
        let cabin = src
            .split("pub fn refresh_cabin_oauth(")
            .nth(1)
            .and_then(|s| s.split("fn refresh_grok_login_now(").next())
            .expect("refresh_cabin_oauth");
        assert!(
            cabin.contains("thread::spawn") && cabin.contains("ensure_access"),
            "cabin OAuth refresh HTTP must leave the UI thread: {cabin}"
        );
        assert!(
            cabin.contains("from_secs(30)"),
            "a failed cabin OAuth refresh must not retry every chip/Imagine paint: {cabin}"
        );
    }
}

