use grokhub_core::{
    chat_request_body_vision, chat_timeout_secs, client_secrets_body, client_secrets_url,
    dedicated_imagine_model, dedicated_video_model, frame_bytes, imagine_empty_reply_hint,
    imagine_image_fallback_model, imagine_image_shaped, imagine_is_network_stall,
    imagine_moderation_blocked, imagine_network_hint, imagine_should_retry_model, imagine_slug,
    imagine_video_fallback_model, media_ext_from_bytes,
    merge_thinking, parse_client_secret, parse_imagine_url, parse_model_reasoning,
    parse_model_text, parse_stt_text, parse_video_job_status, parse_video_request_id,
    parse_video_url, realtime_can_connect, responses_request_body, responses_url, stt_multipart,
    stt_url, tts_request_body, tts_url, video_moderation_blocked, video_request_body,
    voice_client_secret_denied, PresenceFrame, VideoJobStatus, MEDIA_FILE_CAP, TEXT_FILE_CAP,
    XAI_BASE,
};
use std::io::Read;

/// Production Imagine calls `XAI_BASE`. Tests can point the generations POST at a local origin.
fn imagine_api_base() -> String {
    #[cfg(test)]
    if let Some(base) = imagine_base_override() {
        return base;
    }
    XAI_BASE.to_string()
}

#[cfg(test)]
static IMAGINE_BASE_OVERRIDE: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

#[cfg(test)]
fn imagine_base_override() -> Option<String> {
    IMAGINE_BASE_OVERRIDE
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
}

/// Test-only. Production builds do not compile this, so they always POST to `XAI_BASE`.
#[cfg(test)]
pub(crate) fn set_imagine_base_override(base: Option<&str>) {
    let mut slot = IMAGINE_BASE_OVERRIDE
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    *slot = base.map(|s| s.trim_end_matches('/').to_string());
}

fn json_error(v: &serde_json::Value) -> Option<String> {
    v.get("error")
        .and_then(|e| e.get("message").and_then(|m| m.as_str()).or(e.as_str()))
        .map(|s| s.to_string())
}

fn xai_agent(timeout_secs: u64) -> ureq::Agent {
    ureq::AgentBuilder::new()
        .try_proxy_from_env(true)
        .timeout_connect(std::time::Duration::from_secs(25))
        .timeout(std::time::Duration::from_secs(timeout_secs.max(1)))
        .build()
}

fn grok_json(
    url: &str,
    key: &str,
    body: serde_json::Value,
    timeout_secs: u64,
) -> Result<serde_json::Value, String> {
    let resp = xai_agent(timeout_secs)
        .post(url)
        .set("authorization", &format!("Bearer {key}"))
        .set("content-type", "application/json")
        .send_json(body)
        .map_err(http_err)?;
    let v = read_json_capped(resp)?;
    if let Some(err) = json_error(&v) {
        return Err(err);
    }
    Ok(v)
}

fn read_json_capped(resp: ureq::Response) -> Result<serde_json::Value, String> {
    let mut buf = Vec::new();
    resp.into_reader()
        .take(MEDIA_FILE_CAP + 1)
        .read_to_end(&mut buf)
        .map_err(|e| e.to_string())?;
    if buf.len() as u64 > MEDIA_FILE_CAP {
        return Err("response too large".into());
    }
    serde_json::from_slice(&buf).map_err(|e| e.to_string())
}

fn merge_reply(v: &serde_json::Value) -> Option<String> {
    parse_model_text(v).map(|content| {
        merge_thinking(&parse_model_reasoning(v).unwrap_or_default(), &content)
    })
}

pub fn grok_chat(
    api_key: &str,
    model: &str,
    messages: &[(String, String)],
    image_data_url: Option<&str>,
    effort: Option<&str>,
) -> Result<String, String> {
    let key = api_key.trim();
    if key.is_empty() {
        return Err("Connect Grok in Settings".into());
    }
    let timeout = chat_timeout_secs(effort);
    let responses = responses_request_body(model, messages, image_data_url, effort);
    if let Ok(v) = grok_json(&responses_url(), key, responses, timeout) {
        if let Some(text) = merge_reply(&v) {
            return Ok(text);
        }
    }
    let body = chat_request_body_vision(model, messages, image_data_url, effort);
    let v = grok_json(
        &format!("{XAI_BASE}/chat/completions"),
        key,
        body,
        timeout,
    )?;
    merge_reply(&v).ok_or_else(|| "empty Grok reply".into())
}

pub fn grok_imagine_opts(
    api_key: &str,
    model: &str,
    prompt: &str,
    aspect: Option<&str>,
    resolution: Option<&str>,
) -> Result<String, String> {
    let key = api_key.trim();
    if key.is_empty() {
        return Err("Connect Grok in Settings".into());
    }
    let primary = dedicated_imagine_model(model);
    let quality = match resolution.map(str::trim) {
        Some("1k") => Some("low"),
        Some("2k") => Some("medium"),
        _ => None,
    };
    let try_model = |m: &str, timeout_secs: u64| -> Result<String, String> {
        let body = imagine_image_shaped(prompt, m, aspect, resolution, quality);
        let v = grok_json(
            &format!("{}/images/generations", imagine_api_base()),
            key,
            body,
            timeout_secs,
        )?;
        if imagine_moderation_blocked(&v) {
            return Err("image blocked by moderation".into());
        }
        let url = parse_imagine_url(&v).ok_or_else(|| imagine_empty_reply_hint(&v))?;
        // The local-send test returns a URL on port 80 and asserts the cabin keeps that
        // string. Production still downloads the image and stores the file path.
        #[cfg(test)]
        if imagine_base_override().is_some() {
            return Ok(url);
        }
        save_media(&url, prompt, "png", key)
    };
    match try_model(&primary, 120) {
        Ok(path) => Ok(path),
        Err(e) => {
            if imagine_is_network_stall(&e) {
                if let Ok(path) = try_model(&primary, 180) {
                    return Ok(path);
                }
            }
            if let Some(fb) = imagine_image_fallback_model(&primary) {
                if imagine_should_retry_model(&e) {
                    return try_model(fb, 180).map_err(|fb_e| imagine_network_hint(&fb_e));
                }
            }
            Err(imagine_network_hint(&e))
        }
    }
}

pub fn grok_imagine_video(
    api_key: &str,
    model: &str,
    prompt: &str,
    duration: u32,
    aspect: &str,
    resolution: &str,
) -> Result<String, String> {
    let key = api_key.trim();
    if key.is_empty() {
        return Err("Connect Grok in Settings".into());
    }
    let primary = dedicated_video_model(model);
    let try_start = |m: &str| -> Result<String, String> {
        let body = video_request_body(prompt, m, duration, aspect, resolution);
        let started = grok_json(
            &format!("{XAI_BASE}/videos/generations"),
            key,
            body,
            120,
        )?;
        parse_video_request_id(&started).ok_or_else(|| "empty video request_id".to_string())
    };
    let request_id = match try_start(&primary) {
        Ok(id) => id,
        Err(e) => {
            let retry = if imagine_is_network_stall(&e) {
                try_start(&primary).ok()
            } else {
                None
            };
            if let Some(id) = retry {
                id
            } else if let Some(fb) = imagine_video_fallback_model(&primary) {
                if imagine_should_retry_model(&e) {
                    try_start(fb).map_err(|fb_e| imagine_network_hint(&fb_e))?
                } else {
                    return Err(imagine_network_hint(&e));
                }
            } else {
                return Err(imagine_network_hint(&e));
            }
        }
    };
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(480);
    loop {
        if std::time::Instant::now() >= deadline {
            return Err(imagine_network_hint("video timed out"));
        }
        let poll = xai_agent(45)
            .get(&format!("{XAI_BASE}/videos/{request_id}"))
            .set("authorization", &format!("Bearer {key}"))
            .call()
            .map_err(|e| imagine_network_hint(&http_err(e)))?;
        let v = read_json_capped(poll)?;
        if let Some(err) = json_error(&v) {
            return Err(imagine_network_hint(&err));
        }
        let status = v.get("status").and_then(|s| s.as_str()).unwrap_or("");
        match parse_video_job_status(status) {
            VideoJobStatus::Pending => {
                std::thread::sleep(std::time::Duration::from_secs(5));
            }
            VideoJobStatus::Done => {
                if video_moderation_blocked(&v) {
                    return Err("video blocked by moderation".into());
                }
                let url = parse_video_url(&v).ok_or_else(|| "empty video url".to_string())?;
                return save_media(&url, prompt, "mp4", key);
            }
            VideoJobStatus::Failed => {
                return Err(json_error(&v).unwrap_or_else(|| "video failed".into()));
            }
            VideoJobStatus::Expired => return Err("video expired".into()),
        }
    }
}

fn save_media(url: &str, prompt: &str, ext: &str, key: &str) -> Result<String, String> {
    let buf = if url.starts_with("data:image") {
        let f = PresenceFrame {
            data_url: url.to_string(),
            at: 0,
        };
        frame_bytes(&f)
            .ok_or_else(|| "bad imagine data url".to_string())?
            .1
    } else {
        let fetch = |auth: bool| -> Result<Vec<u8>, String> {
            let mut req = xai_agent(120).get(url);
            if auth && !key.trim().is_empty() {
                req = req.set("authorization", &format!("Bearer {key}"));
            }
            let resp = req.call().map_err(http_err)?;
            let mut buf = Vec::new();
            resp.into_reader()
                .take(MEDIA_FILE_CAP + 1)
                .read_to_end(&mut buf)
                .map_err(|e| e.to_string())?;
            if buf.len() as u64 > MEDIA_FILE_CAP {
                return Err("media too large".into());
            }
            if buf.len() < 32 {
                return Err("empty media download".into());
            }
            Ok(buf)
        };
        match fetch(true) {
            Ok(buf) => buf,
            Err(_) => fetch(false)?,
        }
    };
    let ext = media_ext_from_bytes(&buf, ext);
    let path = crate::desktop::imagine_save_path_ext(&imagine_slug(prompt), ext);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    std::fs::write(&path, buf).map_err(|e| e.to_string())?;
    Ok(path.display().to_string())
}

pub fn grok_stt(api_key: &str, wav: &[u8]) -> Result<String, String> {
    let key = api_key.trim();
    if key.is_empty() {
        return Err("Connect Grok in Settings".into());
    }
    if wav.len() < 32 {
        return Err("empty recording".into());
    }
    let boundary = "----grokhubstt";
    let body = stt_multipart(wav, "grokhub-voice.wav", boundary);
    let resp = xai_agent(60)
        .post(&stt_url())
        .set("authorization", &format!("Bearer {key}"))
        .set(
            "content-type",
            &format!("multipart/form-data; boundary={boundary}"),
        )
        .send_bytes(&body)
        .map_err(|e| e.to_string())?;
    let v = read_json_capped(resp)?;
    if let Some(err) = v
        .get("error")
        .and_then(|e| e.get("message").and_then(|m| m.as_str()).or(e.as_str()))
    {
        return Err(err.to_string());
    }
    parse_stt_text(&v).ok_or_else(|| "empty transcript".into())
}

pub fn http_err(e: ureq::Error) -> String {
    match e {
        ureq::Error::Status(code, resp) => {
            let mut buf = Vec::new();
            let _ = resp
                .into_reader()
                .take(TEXT_FILE_CAP as u64)
                .read_to_end(&mut buf);
            let body = String::from_utf8_lossy(&buf);
            format!("HTTP {code}: {}", body.chars().take(200).collect::<String>())
        }
        other => {
            let raw = other.to_string();
            if imagine_is_network_stall(&raw) {
                format!(
                    "Network timeout reaching api.x.ai. Check VPN, firewall, or HTTPS_PROXY. ({})",
                    raw.chars().take(160).collect::<String>()
                )
            } else {
                raw
            }
        }
    }
}

pub fn grok_tts(api_key: &str, text: &str) -> Result<Vec<u8>, String> {
    let key = api_key.trim();
    if key.is_empty() {
        return Err("Connect Grok in Settings".into());
    }
    let text = text.trim();
    if text.is_empty() {
        return Err("nothing to speak".into());
    }
    let resp = xai_agent(60)
        .post(&tts_url())
        .set("authorization", &format!("Bearer {key}"))
        .set("content-type", "application/json")
        .send_json(tts_request_body(text))
        .map_err(|e| e.to_string())?;
    let mut buf = Vec::new();
    resp.into_reader()
        .take(MEDIA_FILE_CAP + 1)
        .read_to_end(&mut buf)
        .map_err(|e| e.to_string())?;
    if buf.len() as u64 > MEDIA_FILE_CAP {
        return Err("speech too large".into());
    }
    if buf.len() < 32 {
        return Err("empty speech".into());
    }
    Ok(buf)
}

pub fn grok_realtime_secret(api_key: &str) -> Result<serde_json::Value, String> {
    if !realtime_can_connect(api_key) {
        return Err(voice_client_secret_denied(false)
            .unwrap_or("Duplex Voice needs a console API key.")
            .into());
    }
    let resp = xai_agent(20)
        .post(&client_secrets_url())
        .set("authorization", &format!("Bearer {}", api_key.trim()))
        .set("content-type", "application/json")
        .send_json(client_secrets_body())
        .map_err(http_err)?;
    let v = read_json_capped(resp)?;
    if let Some(err) = v
        .get("error")
        .and_then(|e| e.get("message").and_then(|m| m.as_str()).or(e.as_str()))
    {
        return Err(err.to_string());
    }
    if parse_client_secret(&v).is_none() {
        return Err("empty client secret".into());
    }
    Ok(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn imagine_download_sends_the_bearer() {
        let src = include_str!("xai.rs");
        let save = src
            .split("fn save_media")
            .nth(1)
            .and_then(|s| s.split("pub fn grok_stt").next())
            .expect("save_media");
        assert!(
            save.contains("authorization"),
            "vidgen / image URLs need the same Bearer as the generate call: {save}"
        );
        let take = save.find(".take(").expect("capped download");
        let read = save.find("read_to_end").expect("download read");
        assert!(
            take < read && save.contains("MEDIA_FILE_CAP"),
            "Imagine download must not slurp a huge body: {save}"
        );
        let tts = src
            .split("pub fn grok_tts(")
            .nth(1)
            .and_then(|s| s.split("pub fn grok_realtime_secret").next())
            .expect("grok_tts");
        let tts_take = tts.find(".take(").expect("capped tts");
        let tts_read = tts.find("read_to_end").expect("tts read");
        assert!(
            tts_take < tts_read && tts.contains("MEDIA_FILE_CAP"),
            "TTS must not slurp a huge audio body: {tts}"
        );
        let json = src
            .split("fn grok_json(")
            .nth(1)
            .and_then(|s| s.split("fn merge_reply(").next())
            .expect("grok_json");
        assert!(
            json.contains(".take(") && json.contains("MEDIA_FILE_CAP") && !json.contains("into_json()"),
            "chat JSON must not slurp an unbounded completion: {json}"
        );
        let err = src
            .split("pub fn http_err(")
            .nth(1)
            .and_then(|s| s.split("pub fn grok_tts(").next())
            .expect("http_err");
        assert!(
            err.contains(".take(") && !err.contains("into_string()"),
            "HTTP error must not slurp a huge error page: {err}"
        );
        assert!(
            src.contains("imagine_should_retry_model"),
            "OAuth often lacks grok-imagine-image-2.0 — retry grok-imagine-image"
        );
        assert!(
            src.contains("try_proxy_from_env(true)") && src.contains("timeout_connect"),
            "Imagine must honor HTTPS_PROXY and not hang forever on connect: {src}"
        );
        let imagine = src
            .split("pub fn grok_imagine_opts(")
            .nth(1)
            .and_then(|s| s.split("pub fn grok_imagine_video(").next())
            .expect("grok_imagine_opts");
        assert!(
            imagine.contains("try_model(&primary, 120)") && !imagine.contains("try_model(&primary, 45)"),
            "2k stills routinely exceed a 45s ureq budget and surface as Windows 10060: {imagine}"
        );
        assert!(
            src.contains("imagine_is_network_stall") && src.contains("imagine_network_hint"),
            "os error 10060 must retry and then name VPN/proxy, not dump a raw WinSock string"
        );
        assert!(
            src.contains("imagine_video_fallback_model"),
            "video 1.5 must fall back to grok-imagine-video"
        );
        assert!(
            src.contains("video_moderation_blocked"),
            "an empty video url after moderation must not look like a parse bug"
        );
    }

    #[test]
    fn realtime_secret_needs_console_key() {
        let err = grok_realtime_secret("").expect_err("oauth cannot mint");
        assert!(
            err.to_ascii_lowercase().contains("console")
                || err.to_ascii_lowercase().contains("api key"),
            "{err}"
        );
    }
}
