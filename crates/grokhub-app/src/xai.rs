use grokhub_core::{
    chat_request_body_vision, chat_stream_flag, chat_timeout_secs, dedicated_imagine_model, frame_bytes,
    imagine_request_body, imagine_slug, parse_chat_content, parse_imagine_url, parse_sse_delta,
    parse_stt_text, sse_done, stt_multipart, stt_url, tts_request_body, tts_url, PresenceFrame,
    XAI_BASE,
};
use std::io::Read;

pub fn grok_chat(
    api_key: &str,
    model: &str,
    messages: &[(String, String)],
    image_data_url: Option<&str>,
) -> Result<String, String> {
    let key = api_key.trim();
    if key.is_empty() {
        return Err("Connect Grok in Settings".into());
    }
    let body = chat_request_body_vision(model, messages, image_data_url);
    let resp = ureq::post(&format!("{XAI_BASE}/chat/completions"))
        .set("authorization", &format!("Bearer {key}"))
        .set("content-type", "application/json")
        .timeout(std::time::Duration::from_secs(chat_timeout_secs(model)))
        .send_json(body)
        .map_err(http_err)?;
    let v: serde_json::Value = resp.into_json().map_err(|e| e.to_string())?;
    if let Some(err) = v
        .get("error")
        .and_then(|e| e.get("message").and_then(|m| m.as_str()).or(e.as_str()))
    {
        return Err(err.to_string());
    }
    parse_chat_content(&v).ok_or_else(|| "empty Grok reply".into())
}

pub fn grok_chat_stream(
    api_key: &str,
    model: &str,
    messages: &[(String, String)],
    image_data_url: Option<&str>,
    mut on_delta: impl FnMut(&str),
) -> Result<String, String> {
    let key = api_key.trim();
    if key.is_empty() {
        return Err("Connect Grok in Settings".into());
    }
    let mut body = chat_request_body_vision(model, messages, image_data_url);
    chat_stream_flag(&mut body, true);
    let resp = match ureq::post(&format!("{XAI_BASE}/chat/completions"))
        .set("authorization", &format!("Bearer {key}"))
        .set("content-type", "application/json")
        .set("accept", "text/event-stream")
        .timeout(std::time::Duration::from_secs(chat_timeout_secs(model)))
        .send_json(body)
    {
        Ok(r) => r,
        Err(e) => return grok_chat(api_key, model, messages, image_data_url).map_err(|_| http_err(e)),
    };
    let mut reader = resp.into_reader();
    let mut raw = String::new();
    let mut acc = String::new();
    let mut buf = [0u8; 2048];
    loop {
        let n = reader.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        raw.push_str(&String::from_utf8_lossy(&buf[..n]));
        while let Some(idx) = raw.find('\n') {
            let line = raw[..idx].trim_end_matches('\r').to_string();
            raw = raw[idx + 1..].to_string();
            if sse_done(&line) {
                return if acc.is_empty() {
                    grok_chat(api_key, model, messages, image_data_url)
                } else {
                    Ok(acc)
                };
            }
            if let Some(d) = parse_sse_delta(&line) {
                on_delta(&d);
                acc.push_str(&d);
            }
        }
    }
    if acc.is_empty() {
        grok_chat(api_key, model, messages, image_data_url)
    } else {
        Ok(acc)
    }
}

pub fn grok_imagine(api_key: &str, model: &str, prompt: &str) -> Result<String, String> {
    let key = api_key.trim();
    if key.is_empty() {
        return Err("Connect Grok in Settings".into());
    }
    let body = imagine_request_body(prompt, &dedicated_imagine_model(model));
    let resp = ureq::post(&format!("{XAI_BASE}/images/generations"))
        .set("authorization", &format!("Bearer {key}"))
        .set("content-type", "application/json")
        .timeout(std::time::Duration::from_secs(120))
        .send_json(body)
        .map_err(|e| e.to_string())?;
    let v: serde_json::Value = resp.into_json().map_err(|e| e.to_string())?;
    if let Some(err) = v
        .get("error")
        .and_then(|e| e.get("message").and_then(|m| m.as_str()).or(e.as_str()))
    {
        return Err(err.to_string());
    }
    let url = parse_imagine_url(&v).ok_or_else(|| "empty Imagine reply".to_string())?;
    save_imagine(&url, prompt)
}

fn save_imagine(url: &str, prompt: &str) -> Result<String, String> {
    let path = crate::desktop::imagine_save_path(&imagine_slug(prompt));
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    if url.starts_with("data:image") {
        let f = PresenceFrame {
            data_url: url.to_string(),
            at: 0,
        };
        let (_, buf) = frame_bytes(&f).ok_or_else(|| "bad imagine data url".to_string())?;
        std::fs::write(&path, buf).map_err(|e| e.to_string())?;
    } else {
        let resp = ureq::get(url)
            .timeout(std::time::Duration::from_secs(60))
            .call()
            .map_err(|e| e.to_string())?;
        let mut reader = resp.into_reader();
        let mut file = std::fs::File::create(&path).map_err(|e| e.to_string())?;
        std::io::copy(&mut reader, &mut file).map_err(|e| e.to_string())?;
    }
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
    let resp = ureq::post(&stt_url())
        .set("authorization", &format!("Bearer {key}"))
        .set(
            "content-type",
            &format!("multipart/form-data; boundary={boundary}"),
        )
        .timeout(std::time::Duration::from_secs(60))
        .send_bytes(&body)
        .map_err(|e| e.to_string())?;
    let v: serde_json::Value = resp.into_json().map_err(|e| e.to_string())?;
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
            let body = resp.into_string().unwrap_or_default();
            format!("HTTP {code}: {}", body.chars().take(200).collect::<String>())
        }
        other => other.to_string(),
    }
}

pub fn http_status_of(err: &str) -> Option<u16> {
    let rest = err.strip_prefix("HTTP ")?;
    rest.split(|c: char| !c.is_ascii_digit())
        .next()
        .and_then(|s| s.parse().ok())
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
    let resp = ureq::post(&tts_url())
        .set("authorization", &format!("Bearer {key}"))
        .set("content-type", "application/json")
        .timeout(std::time::Duration::from_secs(60))
        .send_json(tts_request_body(text))
        .map_err(|e| e.to_string())?;
    let mut buf = Vec::new();
    resp.into_reader()
        .read_to_end(&mut buf)
        .map_err(|e| e.to_string())?;
    if buf.len() < 32 {
        return Err("empty speech".into());
    }
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_prefix() {
        assert_eq!(http_status_of("HTTP 429: rate"), Some(429));
        assert!(http_status_of("timeout").is_none());
    }
}
