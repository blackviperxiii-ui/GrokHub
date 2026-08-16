use grokhub_core::{
    chat_include_usage, chat_request_body_vision, chat_stream_flag, chat_timeout_secs,
    client_secrets_body, client_secrets_url, dedicated_imagine_model, dedicated_video_model,
    frame_bytes, imagine_image_body, imagine_slug, merge_thinking, parse_client_secret,
    parse_imagine_url, parse_model_reasoning, parse_video_job_status, parse_video_request_id,
    parse_video_url, video_request_body, VideoJobStatus,
    parse_model_text, parse_sse_text, parse_sse_thought, parse_stt_text, realtime_can_connect,
    fold_sse_acc, sse_live_delta,
    responses_request_body, responses_url, sse_done, stt_multipart, stt_url, tts_request_body, tts_url,
    voice_client_secret_denied, PresenceFrame, XAI_BASE,
};
use std::io::Read;

fn json_error(v: &serde_json::Value) -> Option<String> {
    v.get("error")
        .and_then(|e| e.get("message").and_then(|m| m.as_str()).or(e.as_str()))
        .map(|s| s.to_string())
}

fn grok_json(
    url: &str,
    key: &str,
    body: serde_json::Value,
    timeout_secs: u64,
) -> Result<serde_json::Value, String> {
    let resp = ureq::post(url)
        .set("authorization", &format!("Bearer {key}"))
        .set("content-type", "application/json")
        .timeout(std::time::Duration::from_secs(timeout_secs))
        .send_json(body)
        .map_err(http_err)?;
    let v: serde_json::Value = resp.into_json().map_err(|e| e.to_string())?;
    if let Some(err) = json_error(&v) {
        return Err(err);
    }
    Ok(v)
}

fn consume_sse(
    mut reader: impl Read,
    on_delta: &mut impl FnMut(&str),
    on_thought: &mut impl FnMut(&str),
) -> Result<String, String> {
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
            if apply_sse_line(&line, &mut acc, on_delta, on_thought) {
                return Ok(acc);
            }
        }
    }
    if !raw.trim().is_empty() {
        apply_sse_line(raw.trim_end_matches('\r'), &mut acc, on_delta, on_thought);
    }
    Ok(acc)
}

fn apply_sse_line(
    line: &str,
    acc: &mut String,
    on_delta: &mut impl FnMut(&str),
    on_thought: &mut impl FnMut(&str),
) -> bool {
    if sse_done(line) {
        return true;
    }
    if let Some(t) = parse_sse_thought(line) {
        on_thought(&t);
    }
    if let Some((d, kind)) = parse_sse_text(line) {
        if sse_live_delta(acc.is_empty(), kind) {
            on_delta(&d);
        }
        fold_sse_acc(acc, &d, kind);
    }
    false
}

fn grok_sse(
    url: &str,
    key: &str,
    body: serde_json::Value,
    timeout_secs: u64,
    on_delta: &mut impl FnMut(&str),
    on_thought: &mut impl FnMut(&str),
) -> Result<String, String> {
    let resp = ureq::post(url)
        .set("authorization", &format!("Bearer {key}"))
        .set("content-type", "application/json")
        .set("accept", "text/event-stream")
        .timeout(std::time::Duration::from_secs(timeout_secs))
        .send_json(body)
        .map_err(http_err)?;
    consume_sse(resp.into_reader(), on_delta, on_thought)
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

pub fn grok_chat_stream(
    api_key: &str,
    model: &str,
    messages: &[(String, String)],
    image_data_url: Option<&str>,
    effort: Option<&str>,
    mut on_delta: impl FnMut(&str),
    mut on_thought: impl FnMut(&str),
) -> Result<String, String> {
    let key = api_key.trim();
    if key.is_empty() {
        return Err("Connect Grok in Settings".into());
    }
    let timeout = chat_timeout_secs(effort);
    let mut responses = responses_request_body(model, messages, image_data_url, effort);
    chat_stream_flag(&mut responses, true);
    if let Ok(acc) = grok_sse(
        &responses_url(),
        key,
        responses,
        timeout,
        &mut on_delta,
        &mut on_thought,
    ) {
        if !acc.is_empty() {
            return Ok(acc);
        }
    }
    let mut body = chat_request_body_vision(model, messages, image_data_url, effort);
    chat_stream_flag(&mut body, true);
    chat_include_usage(&mut body);
    match grok_sse(
        &format!("{XAI_BASE}/chat/completions"),
        key,
        body,
        timeout,
        &mut on_delta,
        &mut on_thought,
    ) {
        Ok(acc) if !acc.is_empty() => Ok(acc),
        Ok(_) => grok_chat(api_key, model, messages, image_data_url, effort),
        Err(e) => grok_chat(api_key, model, messages, image_data_url, effort).map_err(|_| e),
    }
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
    let body = imagine_image_body(prompt, &dedicated_imagine_model(model), aspect, resolution);
    let v = grok_json(
        &format!("{XAI_BASE}/images/generations"),
        key,
        body,
        120,
    )?;
    let url = parse_imagine_url(&v).ok_or_else(|| "empty Imagine reply".to_string())?;
    save_media(&url, prompt, "png")
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
    let body = video_request_body(
        prompt,
        &dedicated_video_model(model),
        duration,
        aspect,
        resolution,
    );
    let started = grok_json(
        &format!("{XAI_BASE}/videos/generations"),
        key,
        body,
        60,
    )?;
    let request_id =
        parse_video_request_id(&started).ok_or_else(|| "empty video request_id".to_string())?;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(480);
    loop {
        if std::time::Instant::now() >= deadline {
            return Err("video timed out".into());
        }
        let poll = ureq::get(&format!("{XAI_BASE}/videos/{request_id}"))
            .set("authorization", &format!("Bearer {key}"))
            .timeout(std::time::Duration::from_secs(30))
            .call()
            .map_err(http_err)?;
        let v: serde_json::Value = poll.into_json().map_err(|e| e.to_string())?;
        if let Some(err) = json_error(&v) {
            return Err(err);
        }
        let status = v.get("status").and_then(|s| s.as_str()).unwrap_or("");
        match parse_video_job_status(status) {
            VideoJobStatus::Pending => {
                std::thread::sleep(std::time::Duration::from_secs(5));
            }
            VideoJobStatus::Done => {
                let url = parse_video_url(&v).ok_or_else(|| "empty video url".to_string())?;
                return save_media(&url, prompt, "mp4");
            }
            VideoJobStatus::Failed => return Err("video failed".into()),
            VideoJobStatus::Expired => return Err("video expired".into()),
        }
    }
}

fn save_media(url: &str, prompt: &str, ext: &str) -> Result<String, String> {
    let path = if ext.trim().eq_ignore_ascii_case("png") {
        crate::desktop::imagine_save_path(&imagine_slug(prompt))
    } else {
        crate::desktop::imagine_save_path_ext(&imagine_slug(prompt), ext)
    };
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
            .map_err(http_err)?;
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

pub fn grok_realtime_secret(api_key: &str) -> Result<serde_json::Value, String> {
    if !realtime_can_connect(api_key) {
        return Err(voice_client_secret_denied(false)
            .unwrap_or("Duplex Voice needs a console API key.")
            .into());
    }
    let resp = ureq::post(&client_secrets_url())
        .set("authorization", &format!("Bearer {}", api_key.trim()))
        .set("content-type", "application/json")
        .timeout(std::time::Duration::from_secs(20))
        .send_json(client_secrets_body())
        .map_err(http_err)?;
    let v: serde_json::Value = resp.into_json().map_err(|e| e.to_string())?;
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
    fn status_prefix() {
        assert_eq!(http_status_of("HTTP 429: rate"), Some(429));
        assert!(http_status_of("timeout").is_none());
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

    #[test]
    fn responses_done_fills_empty_acc() {
        let data = b"data: {\"type\":\"response.output_text.done\",\"text\":\"Hello\"}\n\n";
        let mut deltas = String::new();
        let acc = consume_sse(&data[..], &mut |d| deltas.push_str(d), &mut |_| {}).unwrap();
        assert_eq!(acc, "Hello");
        assert_eq!(deltas, "Hello");
    }

    #[test]
    fn responses_done_after_delta_does_not_duplicate() {
        let data = concat!(
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\"Hel\"}\n",
            "data: {\"type\":\"response.output_text.done\",\"text\":\"Hello\"}\n",
        );
        let mut deltas = String::new();
        let acc = consume_sse(data.as_bytes(), &mut |d| deltas.push_str(d), &mut |_| {}).unwrap();
        assert_eq!(acc, "Hello");
        assert_eq!(deltas, "Hel");
    }

    #[test]
    fn responses_tail_without_newline_is_kept() {
        let data = b"data: {\"type\":\"response.output_text.delta\",\"delta\":\"CMD\"}";
        let mut deltas = String::new();
        let acc = consume_sse(&data[..], &mut |d| deltas.push_str(d), &mut |_| {}).unwrap();
        assert_eq!(acc, "CMD");
        assert_eq!(deltas, "CMD");
    }

    #[test]
    fn responses_short_done_keeps_longer_deltas() {
        let data = concat!(
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\"Hello\\nCOMPUTER_CMD: key Alt+F4\"}\n",
            "data: {\"type\":\"response.output_text.done\",\"text\":\"Hello\"}\n",
        );
        let acc = consume_sse(data.as_bytes(), &mut |_| {}, &mut |_| {}).unwrap();
        assert!(
            acc.contains("COMPUTER_CMD: key Alt+F4"),
            "short done wiped the tool line: {acc}"
        );
    }
}
