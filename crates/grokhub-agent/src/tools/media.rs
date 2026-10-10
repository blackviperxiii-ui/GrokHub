//! Imagine tools for the native engine. Request bodies come from
//! `grokhub_core`'s Imagine module. A missing bearer does not dial.

use std::time::Duration;

use serde_json::{json, Value};

use super::ToolOutput;

pub trait ImagineApi: Send + Sync {
    fn post_json(&self, path: &str, body: &Value) -> Result<Value, String>;
    fn get_json(&self, path: &str) -> Result<Value, String>;
    /// Bytes of a result URL, capped. The default never dials.
    fn download(&self, url: &str) -> Result<Vec<u8>, String> {
        let _ = url;
        Err("download not available".into())
    }
}

/// Largest result file kept (same cap as the cabin's Imagine downloads).
const MEDIA_CAP: u64 = grokhub_core::MEDIA_FILE_CAP;
/// Video jobs: poll every 5 s for up to 8 minutes, like the cabin's Imagine page.
const VIDEO_POLL: Duration = Duration::from_secs(5);
const VIDEO_DEADLINE: Duration = Duration::from_secs(480);

pub(crate) struct BlockedImagine;

impl ImagineApi for BlockedImagine {
    fn post_json(&self, _path: &str, _body: &Value) -> Result<Value, String> {
        Err(grokhub_core::XAI_NEED_SIGNIN.to_string())
    }

    fn get_json(&self, _path: &str) -> Result<Value, String> {
        Err(grokhub_core::XAI_NEED_SIGNIN.to_string())
    }
}

pub(crate) struct UreqImagine {
    agent: ureq::Agent,
    bearer: String,
}

impl UreqImagine {
    pub(crate) fn new(bearer: String) -> Self {
        let agent = ureq::AgentBuilder::new()
            .timeout_connect(Duration::from_secs(20))
            .timeout(Duration::from_secs(120))
            .resolver(super::web_fetch::public_only_resolve)
            .build();
        Self { agent, bearer }
    }

    fn auth_header(&self) -> String {
        let bearer = &self.bearer;
        format!("Bearer {bearer}")
    }
}

impl ImagineApi for UreqImagine {
    fn post_json(&self, path: &str, body: &Value) -> Result<Value, String> {
        let response = self
            .agent
            .post(&endpoint(path))
            .set("Authorization", &self.auth_header())
            .set("User-Agent", crate::USER_AGENT)
            .send_json(body)
            .map_err(http_err)?;
        response.into_json::<Value>().map_err(|err| err.to_string())
    }

    fn get_json(&self, path: &str) -> Result<Value, String> {
        let response = self
            .agent
            .get(&endpoint(path))
            .set("Authorization", &self.auth_header())
            .set("User-Agent", crate::USER_AGENT)
            .call()
            .map_err(http_err)?;
        response.into_json::<Value>().map_err(|err| err.to_string())
    }

    /// https only, public addresses only (the agent's resolver), and the bearer
    /// only goes to x.ai hosts. ureq drops it on a cross-host redirect.
    fn download(&self, url: &str) -> Result<Vec<u8>, String> {
        let host = https_host(url).ok_or_else(|| "media url must be https".to_string())?;
        let mut req = self.agent.get(url).set("User-Agent", crate::USER_AGENT);
        if host == "x.ai" || host.ends_with(".x.ai") {
            req = req.set("Authorization", &self.auth_header());
        }
        let resp = req.call().map_err(http_err)?;
        let mut buf = Vec::new();
        std::io::Read::read_to_end(
            &mut std::io::Read::take(resp.into_reader(), MEDIA_CAP + 1),
            &mut buf,
        )
        .map_err(|err| err.to_string())?;
        if buf.len() as u64 > MEDIA_CAP {
            return Err("media too large".into());
        }
        Ok(buf)
    }
}

fn endpoint(path: &str) -> String {
    format!(
        "{}/{}",
        grokhub_core::XAI_BASE.trim_end_matches('/'),
        path.trim_start_matches('/')
    )
}

/// EgressGuard (Spike-4c) in front of each Imagine call. The prompt is chat,
/// or personal when it carries a recall-pack line; api.x.ai is a model host,
/// so both go with one line. Polls and result downloads carry no user data.
fn guard(url: &str, data: &[crate::harness::DataClass], stop: &dyn Fn() -> bool) -> Result<(), String> {
    let req = crate::harness::EgressReq::new(url, data);
    crate::harness::guard_or_park(&crate::perm::config_dir(), &req, "imagine", &mut || stop())
}

fn https_host(url: &str) -> Option<String> {
    let rest = url.trim().strip_prefix("https://")?;
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let hostport = &rest[..end];
    if hostport.contains('@') {
        return None;
    }
    let host = if let Some(v6) = hostport.strip_prefix('[') {
        v6.split(']').next().unwrap_or("")
    } else {
        hostport.split(':').next().unwrap_or("")
    };
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    if host.is_empty() {
        None
    } else {
        Some(host)
    }
}

fn http_err(err: ureq::Error) -> String {
    match err {
        ureq::Error::Status(status, resp) => {
            let body = resp.into_string().unwrap_or_default();
            let body: String = body.chars().take(500).collect();
            format!("HTTP {status} {body}")
        }
        ureq::Error::Transport(err) => format!("imagine request failed: {err}"),
    }
}

#[derive(Debug)]
pub(crate) struct MediaCall {
    pub path: &'static str,
    pub body: Value,
    pub poll: bool,
    pub mask_fallback: Option<Value>,
}

pub(crate) fn schemas() -> Vec<Value> {
    vec![
        image_generate_schema(),
        image_edit_schema(),
        video_generate_schema(),
        video_edit_schema(),
        video_extend_schema(),
    ]
}

fn image_generate_schema() -> Value {
    json!({
        "type": "function",
        "name": "image_generate",
        "description": "Generate images. n is 1 to 10. resolution is 1k or 2k. quality is auto, low, or medium on grok-imagine-image-2.0. Spends credits and is not read-only.",
        "parameters": {
            "type": "object",
            "properties": {
                "prompt": {"type": "string"},
                "n": {"type": "integer", "description": "1 to 10. Default 1."},
                "resolution": {"type": "string", "description": "1k or 2k. Default 1k."},
                "aspect_ratio": {"type": "string"},
                "quality": {"type": "string", "description": "auto, low, or medium."},
                "model": {"type": "string"}
            },
            "required": ["prompt"],
            "additionalProperties": false
        }
    })
}

fn image_edit_schema() -> Value {
    json!({
        "type": "function",
        "name": "image_edit",
        "description": "Edit one to three source images. Optional mask. n is 1 to 10. resolution is 1k or 2k. Spends credits and is not read-only.",
        "parameters": {
            "type": "object",
            "properties": {
                "prompt": {"type": "string"},
                "image": {"type": "string", "description": "Source image URL."},
                "images": {"type": "array", "items": {"type": "string"}, "description": "One to three source URLs."},
                "mask": {"type": "string"},
                "n": {"type": "integer"},
                "resolution": {"type": "string"},
                "aspect_ratio": {"type": "string"},
                "model": {"type": "string"}
            },
            "required": ["prompt"],
            "additionalProperties": false
        }
    })
}

fn video_generate_schema() -> Value {
    json!({
        "type": "function",
        "name": "video_generate",
        "description": "Text-to-video, or image-to-video when image is set. duration is 1 to 15 seconds. resolution is 480p, 720p, or 1080p. Spends credits and is not read-only.",
        "parameters": {
            "type": "object",
            "properties": {
                "prompt": {"type": "string"},
                "duration": {"type": "integer"},
                "resolution": {"type": "string"},
                "aspect_ratio": {"type": "string"},
                "image": {"type": "string", "description": "When set, the call is image-to-video."},
                "generate_audio": {"type": "boolean"},
                "model": {"type": "string"}
            },
            "required": ["prompt"],
            "additionalProperties": false
        }
    })
}

fn video_edit_schema() -> Value {
    json!({
        "type": "function",
        "name": "video_edit",
        "description": "Edit an existing video. Spends credits and is not read-only.",
        "parameters": {
            "type": "object",
            "properties": {
                "prompt": {"type": "string"},
                "video": {"type": "string"},
                "model": {"type": "string"}
            },
            "required": ["prompt", "video"],
            "additionalProperties": false
        }
    })
}

fn video_extend_schema() -> Value {
    json!({
        "type": "function",
        "name": "video_extend",
        "description": "Extend an existing video. duration is clamped by the Imagine API (default 6 seconds). Spends credits and is not read-only.",
        "parameters": {
            "type": "object",
            "properties": {
                "prompt": {"type": "string"},
                "video": {"type": "string"},
                "duration": {"type": "integer"},
                "model": {"type": "string"}
            },
            "required": ["prompt", "video"],
            "additionalProperties": false
        }
    })
}

pub(crate) fn run_with_ports(name: &str, args: &Value, stop: &dyn Fn() -> bool) -> ToolOutput {
    let ports = super::ports::current();
    match ports.imagine {
        Some(api) => execute_with(name, args, api.as_ref(), stop, ports.media_dir.as_deref()),
        None => ToolOutput::err(grokhub_core::XAI_NEED_SIGNIN),
    }
}

#[cfg(test)]
pub(crate) fn execute(name: &str, args: &Value, api: &dyn ImagineApi) -> ToolOutput {
    let (dir, _cfg) = tests::scratch_config();
    let out = execute_with(name, args, api, &|| false, None);
    let _ = std::fs::remove_dir_all(dir);
    out
}

/// `stop` is cancel or Halt; a video poll checks it every 200 ms.
/// With `media_dir`, each result is downloaded into that folder and the card gets
/// the local path. A failed download keeps the remote URL.
pub(crate) fn execute_with(
    name: &str,
    args: &Value,
    api: &dyn ImagineApi,
    stop: &dyn Fn() -> bool,
    media_dir: Option<&std::path::Path>,
) -> ToolOutput {
    let call = match build_media_call(name, args) {
        Ok(call) => call,
        Err(err) => return ToolOutput::err(err),
    };
    if stop() {
        return ToolOutput::err("cancelled");
    }
    match run_call(&call, api, stop) {
        Ok(text) => match media_dir {
            Some(dir) => ToolOutput::ok(save_results(&text, api, dir, call.poll)),
            None => ToolOutput::ok(text),
        },
        Err(err) => ToolOutput::err(err),
    }
}

/// Rewrite `IMAGINE: <url>` lines to local files inside `dir`. File names are
/// generated here, never taken from the model or the URL.
fn save_results(text: &str, api: &dyn ImagineApi, dir: &std::path::Path, video: bool) -> String {
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let mut out = Vec::new();
    for line in text.lines() {
        let Some(url) = line.strip_prefix("IMAGINE: ").map(str::trim) else {
            out.push(line.to_string());
            continue;
        };
        let bytes = if let Some(data) = url.strip_prefix("data:") {
            decode_data_url(data)
        } else {
            guard(url, &[], &|| false).and_then(|()| api.download(url))
        };
        let saved = bytes.and_then(|bytes| {
            if bytes.len() < 32 {
                return Err("empty media download".into());
            }
            let ext = media_ext(&bytes, video);
            std::fs::create_dir_all(dir).map_err(|err| err.to_string())?;
            let n = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let path = dir.join(format!(
                "{}-{}-{n}.{ext}",
                if video { "video" } else { "image" },
                grokhub_core::now_ms()
            ));
            std::fs::write(&path, &bytes).map_err(|err| err.to_string())?;
            Ok(path)
        });
        match saved {
            Ok(path) => out.push(format!("IMAGINE: {}", path.display())),
            Err(err) => {
                out.push(format!("IMAGINE: {url}"));
                out.push(format!("(not saved: {err})"));
            }
        }
    }
    out.join("\n")
}

fn decode_data_url(data: &str) -> Result<Vec<u8>, String> {
    use base64::Engine as _;
    let (meta, payload) = data
        .split_once(',')
        .ok_or_else(|| "bad data url".to_string())?;
    if !meta.ends_with(";base64") {
        return Err("bad data url".into());
    }
    if payload.len() as u64 > MEDIA_CAP.saturating_mul(4) / 3 + 4 {
        return Err("media too large".into());
    }
    base64::engine::general_purpose::STANDARD
        .decode(payload.trim())
        .map_err(|err| err.to_string())
}

fn media_ext(bytes: &[u8], video: bool) -> &'static str {
    if bytes.starts_with(b"\x89PNG") {
        "png"
    } else if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        "jpg"
    } else if bytes.len() > 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        "webp"
    } else if video || (bytes.len() > 8 && &bytes[4..8] == b"ftyp") {
        "mp4"
    } else {
        "png"
    }
}

pub(crate) fn build_media_call(name: &str, args: &Value) -> Result<MediaCall, String> {
    match name {
        "image_generate" => image_generate(args),
        "image_edit" => image_edit(args),
        "video_generate" => video_generate(args),
        "video_edit" => video_edit(args),
        "video_extend" => video_extend(args),
        other => Err(format!("unknown media tool {other}")),
    }
}

fn image_generate(args: &Value) -> Result<MediaCall, String> {
    let prompt = prompt_of(args)?;
    let model = model_of(args);
    let n = image_n(args)?;
    let resolution = image_resolution(args)?;
    let aspect = aspect_of(args)?;
    let quality = image_quality(args)?;
    Ok(MediaCall {
        path: "images/generations",
        body: grokhub_core::imagine_generation_body(
            &prompt,
            &model,
            n,
            &resolution,
            &aspect,
            &quality,
            "url",
        ),
        poll: false,
        mask_fallback: None,
    })
}

fn image_edit(args: &Value) -> Result<MediaCall, String> {
    let prompt = prompt_of(args)?;
    let model = model_of(args);
    let sources = image_sources(args)?;
    let n = image_n(args)?;
    let resolution = image_resolution(args)?;
    let aspect = aspect_of(args)?;
    let mask = optional_str(args, "mask");
    let refs: Vec<&str> = sources.iter().map(String::as_str).collect();
    let mask_ref = mask.as_deref();
    let body = grokhub_core::imagine_edit_body(
        &prompt,
        &model,
        &refs,
        mask_ref,
        n,
        &resolution,
        &aspect,
        "url",
    );
    let mask_fallback = if mask_ref.is_some_and(|item| !item.is_empty()) {
        Some(grokhub_core::imagine_edit_mask_fallback(
            &prompt,
            &model,
            &refs,
            mask_ref,
            n,
            &resolution,
            &aspect,
            "url",
        ))
    } else {
        None
    };
    Ok(MediaCall {
        path: "images/edits",
        body,
        poll: false,
        mask_fallback,
    })
}

fn video_generate(args: &Value) -> Result<MediaCall, String> {
    let prompt = prompt_of(args)?;
    let model = model_of(args);
    let duration = generate_duration(args)?;
    let resolution = video_resolution(args)?;
    let aspect = aspect_of(args)?;
    let generate_audio = audio_flag(args)?;
    let image = optional_str(args, "image").unwrap_or_default();
    let op = if image.is_empty() {
        grokhub_core::ImagineVideoOp::TextToVideo
    } else {
        grokhub_core::ImagineVideoOp::ImageToVideo
    };
    Ok(video_call(
        "videos/generations",
        &prompt,
        &model,
        op,
        duration,
        &resolution,
        &aspect,
        generate_audio,
        &image,
        "",
    ))
}

fn video_edit(args: &Value) -> Result<MediaCall, String> {
    let prompt = prompt_of(args)?;
    let model = model_of(args);
    let video = video_of(args)?;
    Ok(video_call(
        "videos/edits",
        &prompt,
        &model,
        grokhub_core::ImagineVideoOp::Edit,
        0,
        "",
        "",
        false,
        "",
        &video,
    ))
}

fn video_extend(args: &Value) -> Result<MediaCall, String> {
    let prompt = prompt_of(args)?;
    let model = model_of(args);
    let video = video_of(args)?;
    let duration = opt_u32(args, "duration")?.unwrap_or_default();
    Ok(video_call(
        "videos/extensions",
        &prompt,
        &model,
        grokhub_core::ImagineVideoOp::Extend,
        duration,
        "",
        "",
        false,
        "",
        &video,
    ))
}

fn video_call(
    path: &'static str,
    prompt: &str,
    model: &str,
    op: grokhub_core::ImagineVideoOp,
    duration: u32,
    resolution: &str,
    aspect: &str,
    generate_audio: bool,
    image_url: &str,
    video_url: &str,
) -> MediaCall {
    let body = grokhub_core::imagine_video_body(&grokhub_core::VideoBodyReq {
        prompt,
        model,
        op,
        duration,
        resolution,
        aspect,
        generate_audio,
        image_url,
        video_url,
    });
    MediaCall {
        path,
        body,
        poll: true,
        mask_fallback: None,
    }
}

fn run_call(
    call: &MediaCall,
    api: &dyn ImagineApi,
    stop: &dyn Fn() -> bool,
) -> Result<String, String> {
    let url = endpoint(call.path);
    guard(&url, crate::harness::model_text_classes(&call.body.to_string()), stop)?;
    match api.post_json(call.path, &call.body) {
        Ok(body) => finish(call, api, body, false, stop),
        Err(err) => {
            if let Some(fallback) = call.mask_fallback.as_ref() {
                if grokhub_core::imagine_mask_rejected(&err) && !stop() {
                    guard(&url, crate::harness::model_text_classes(&fallback.to_string()), stop)?;
                    let body = api.post_json(call.path, fallback)?;
                    return finish(call, api, body, true, stop);
                }
            }
            Err(err)
        }
    }
}

fn finish(
    call: &MediaCall,
    api: &dyn ImagineApi,
    body: Value,
    mask_note: bool,
    stop: &dyn Fn() -> bool,
) -> Result<String, String> {
    let mut text = if call.poll {
        format!("IMAGINE: {}", poll_video(api, &body, stop)?)
    } else {
        image_lines(&body)?
    };
    if mask_note {
        if !text.ends_with('\n') {
            text.push('\n');
        }
        text.push_str("Mask was rejected; sent it as a reference image.");
    }
    Ok(text)
}

fn image_lines(body: &Value) -> Result<String, String> {
    let urls = grokhub_core::parse_imagine_urls(body);
    if urls.is_empty() {
        return Err("empty image url".into());
    }
    Ok(urls
        .into_iter()
        .map(|url| format!("IMAGINE: {url}"))
        .collect::<Vec<_>>()
        .join("\n"))
}

fn poll_video(
    api: &dyn ImagineApi,
    started: &Value,
    stop: &dyn Fn() -> bool,
) -> Result<String, String> {
    if let Some(url) = video_url_now(started)? {
        return Ok(url);
    }
    let id = grokhub_core::parse_video_request_id(started)
        .ok_or_else(|| "empty video request_id".to_string())?;
    let deadline = std::time::Instant::now() + VIDEO_DEADLINE;
    let mut first = true;
    loop {
        if !first {
            let wake = std::time::Instant::now() + VIDEO_POLL;
            while std::time::Instant::now() < wake {
                if stop() {
                    return Err("cancelled".into());
                }
                std::thread::sleep(Duration::from_millis(200));
            }
        }
        first = false;
        if stop() {
            return Err("cancelled".into());
        }
        if std::time::Instant::now() >= deadline {
            return Err("video timed out".into());
        }
        let path = format!("videos/{id}");
        guard(&endpoint(&path), &[], stop)?;
        let body = api.get_json(&path)?;
        if let Some(url) = video_url_now(&body)? {
            return Ok(url);
        }
    }
}

fn video_url_now(body: &Value) -> Result<Option<String>, String> {
    let status = body
        .get("status")
        .and_then(|item| item.as_str())
        .unwrap_or("");
    match grokhub_core::parse_video_job_status(status) {
        grokhub_core::VideoJobStatus::Failed => Err(grokhub_core::video_failure_detail(body)),
        grokhub_core::VideoJobStatus::Expired => Err("video expired".into()),
        grokhub_core::VideoJobStatus::Done => {
            if grokhub_core::video_moderation_blocked(body) {
                return Err("video blocked by moderation".into());
            }
            grokhub_core::parse_video_url(body)
                .map(Some)
                .ok_or_else(|| "empty video url".to_string())
        }
        grokhub_core::VideoJobStatus::Pending => {
            if status.is_empty() {
                if let Some(url) = grokhub_core::parse_video_url(body) {
                    return Ok(Some(url));
                }
            }
            Ok(None)
        }
    }
}

fn prompt_of(args: &Value) -> Result<String, String> {
    let prompt = super::str_field(args, "prompt");
    if prompt.is_empty() {
        Err("prompt is required".into())
    } else {
        Ok(prompt)
    }
}

fn model_of(args: &Value) -> String {
    super::str_field(args, "model")
}

fn optional_str(args: &Value, key: &str) -> Option<String> {
    let text = super::str_field(args, key);
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

fn video_of(args: &Value) -> Result<String, String> {
    optional_str(args, "video").ok_or_else(|| "video is required".into())
}

fn image_sources(args: &Value) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    if let Some(one) = optional_str(args, "image") {
        out.push(one);
    }
    if let Some(arr) = args.get("images") {
        let Some(arr) = arr.as_array() else {
            return Err("images must be an array of strings".into());
        };
        for item in arr {
            let Some(text) = item.as_str() else {
                return Err("images entries must be strings".into());
            };
            let text = text.trim();
            if !text.is_empty() {
                out.push(text.to_string());
            }
        }
    }
    if out.is_empty() {
        return Err("image is required".into());
    }
    if out.len() > 3 {
        return Err("at most 3 images".into());
    }
    Ok(out)
}

fn image_n(args: &Value) -> Result<u32, String> {
    match opt_u32(args, "n")? {
        None => Ok(1),
        Some(n) => in_range("n", n, 1, 10),
    }
}

fn opt_u32(args: &Value, key: &str) -> Result<Option<u32>, String> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(n)) => n
            .as_u64()
            .and_then(|n| u32::try_from(n).ok())
            .map(Some)
            .ok_or_else(|| format!("{key} must be an integer")),
        Some(_) => Err(format!("{key} must be an integer")),
    }
}

fn in_range(key: &str, n: u32, lo: u32, hi: u32) -> Result<u32, String> {
    if (lo..=hi).contains(&n) {
        Ok(n)
    } else {
        Err(format!("{key} must be an integer from {lo} to {hi}"))
    }
}

fn image_resolution(args: &Value) -> Result<String, String> {
    match args.get("resolution") {
        None | Some(Value::Null) => Ok("1k".into()),
        Some(Value::String(text)) => {
            if text.trim().eq_ignore_ascii_case("1k") {
                Ok("1k".into())
            } else if text.trim().eq_ignore_ascii_case("2k") {
                Ok("2k".into())
            } else {
                Err("resolution must be 1k or 2k".into())
            }
        }
        Some(_) => Err("resolution must be 1k or 2k".into()),
    }
}

fn image_quality(args: &Value) -> Result<String, String> {
    match args.get("quality") {
        None | Some(Value::Null) => Ok(String::new()),
        Some(Value::String(text)) => {
            let text = text.trim().to_ascii_lowercase();
            if matches!(text.as_str(), "auto" | "low" | "medium") {
                Ok(text)
            } else {
                Err("quality must be auto, low, or medium".into())
            }
        }
        Some(_) => Err("quality must be auto, low, or medium".into()),
    }
}

fn aspect_of(args: &Value) -> Result<String, String> {
    match args.get("aspect_ratio") {
        None | Some(Value::Null) => Ok(String::new()),
        Some(Value::String(text)) => {
            let text = text.trim();
            if text.is_empty() || text.eq_ignore_ascii_case("auto") {
                return Ok(String::new());
            }
            if grokhub_core::IMAGINE_API_ASPECTS.contains(&text) {
                Ok(text.to_string())
            } else {
                Err("aspect_ratio is not supported".into())
            }
        }
        Some(_) => Err("aspect_ratio is not supported".into()),
    }
}

fn video_resolution(args: &Value) -> Result<String, String> {
    match args.get("resolution") {
        None | Some(Value::Null) => Ok("480p".into()),
        Some(Value::String(text)) => {
            let text = text.trim().to_ascii_lowercase();
            if matches!(text.as_str(), "480p" | "720p" | "1080p") {
                Ok(text)
            } else {
                Err("resolution must be 480p, 720p, or 1080p".into())
            }
        }
        Some(_) => Err("resolution must be 480p, 720p, or 1080p".into()),
    }
}

fn generate_duration(args: &Value) -> Result<u32, String> {
    match opt_u32(args, "duration")? {
        None => Ok(6),
        Some(n) => in_range("duration", n, 1, 15),
    }
}

fn audio_flag(args: &Value) -> Result<bool, String> {
    match args.get("generate_audio") {
        None | Some(Value::Null) => Ok(true),
        Some(Value::Bool(value)) => Ok(*value),
        Some(_) => Err("generate_audio must be a boolean".into()),
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::auto_review::{review, ReviewIn};
    use crate::gate::{self, decide_with, Decision, Gate, PermMode};
    use crate::perm::{parse_rule, Action, Policy};
    use crate::{
        CancelToken, ClientError, ModelClient, ResponsesRequest, StreamEvent, TurnOutput, Usage,
    };
    use std::path::Path;
    use std::sync::Mutex;
    use std::time::Duration;

    fn body_of(name: &str, args: Value) -> MediaCall {
        build_media_call(name, &args).unwrap_or_else(|err| panic!("{name}: {err}"))
    }

    #[test]
    fn media_request_bodies_match_imagine() {
        let gen = body_of(
            "image_generate",
            json!({
                "prompt": "a cabin",
                "n": 4,
                "resolution": "2k",
                "aspect_ratio": "16:9",
                "quality": "medium",
                "model": "grok-imagine-image-2.0"
            }),
        );
        assert_eq!(gen.path, "images/generations");
        assert_eq!(
            gen.body,
            grokhub_core::imagine_generation_body(
                "a cabin",
                "grok-imagine-image-2.0",
                4,
                "2k",
                "16:9",
                "medium",
                "url"
            )
        );
        assert_eq!(gen.body["n"], 4);
        assert_eq!(gen.body["resolution"], "2k");
        assert_eq!(gen.body["quality"], "medium");
        assert_eq!(gen.body["response_format"], "url");

        let edit = body_of(
            "image_edit",
            json!({
                "prompt": "paint the door",
                "image": "https://cdn.example/a.png",
                "mask": "https://cdn.example/m.png",
                "n": 1,
                "resolution": "1k",
                "aspect_ratio": "1:1",
                "model": "grok-imagine-image-2.0"
            }),
        );
        assert_eq!(edit.path, "images/edits");
        assert_eq!(
            edit.body,
            grokhub_core::imagine_edit_body(
                "paint the door",
                "grok-imagine-image-2.0",
                &["https://cdn.example/a.png"],
                Some("https://cdn.example/m.png"),
                1,
                "1k",
                "1:1",
                "url"
            )
        );
        assert_eq!(edit.body["mask"]["url"], "https://cdn.example/m.png");

        let video = body_of(
            "video_generate",
            json!({
                "prompt": "waves",
                "duration": 6,
                "resolution": "1080p",
                "aspect_ratio": "16:9",
                "model": "grok-imagine-video-1.5"
            }),
        );
        assert_eq!(video.path, "videos/generations");
        assert_eq!(
            video.body,
            grokhub_core::imagine_video_body(&grokhub_core::VideoBodyReq {
                prompt: "waves",
                model: "grok-imagine-video-1.5",
                op: grokhub_core::ImagineVideoOp::TextToVideo,
                duration: 6,
                resolution: "1080p",
                aspect: "16:9",
                generate_audio: true,
                image_url: "",
                video_url: "",
            })
        );
        assert_eq!(video.body["resolution"], "1080p");

        let edited = body_of(
            "video_edit",
            json!({
                "prompt": "slow it",
                "video": "https://cdn.example/v.mp4",
                "model": "grok-imagine-video-1.5"
            }),
        );
        assert_eq!(edited.path, "videos/edits");
        assert_eq!(
            edited.body,
            json!({
                "model": "grok-imagine-video-1.5",
                "prompt": "slow it",
                "video": {"url": "https://cdn.example/v.mp4"}
            })
        );
        assert!(edited.body.get("duration").is_none());
        assert!(edited.body.get("resolution").is_none());

        for (input, expect) in [(0u32, 6u32), (1, 2), (15, 10), (6, 6)] {
            let extended = body_of(
                "video_extend",
                json!({
                    "prompt": "more",
                    "video": "https://cdn.example/v.mp4",
                    "duration": input,
                    "model": "grok-imagine-video-1.5"
                }),
            );
            assert_eq!(extended.path, "videos/extensions");
            assert_eq!(
                extended.body,
                grokhub_core::imagine_video_body(&grokhub_core::VideoBodyReq {
                    prompt: "more",
                    model: "grok-imagine-video-1.5",
                    op: grokhub_core::ImagineVideoOp::Extend,
                    duration: input,
                    resolution: "",
                    aspect: "",
                    generate_audio: false,
                    image_url: "",
                    video_url: "https://cdn.example/v.mp4",
                })
            );
            assert_eq!(extended.body["duration"], expect);
        }
    }

    #[test]
    fn media_params_are_validated() {
        let bad = |name: &str, args: Value| {
            let err = build_media_call(name, &args).expect_err(name);
            assert!(!err.is_empty(), "{name}");
            err
        };
        assert!(bad("image_generate", json!({"prompt": "x", "n": 0})).contains("n"));
        assert!(bad("image_generate", json!({"prompt": "x", "n": 11})).contains("n"));
        assert!(bad("image_generate", json!({"prompt": "x", "n": -1})).contains("n"));
        assert!(bad("image_generate", json!({"prompt": "x", "n": 10.5})).contains("n"));
        let ten = body_of("image_generate", json!({"prompt": "x", "n": 10}));
        assert_eq!(ten.body["n"], 10);
        assert!(
            bad("image_generate", json!({"prompt": "x", "resolution": "4k"}))
                .contains("resolution")
        );
        assert!(
            bad("image_generate", json!({"prompt": "x", "quality": "high"})).contains("quality")
        );
        assert!(bad(
            "image_generate",
            json!({"prompt": "x", "aspect_ratio": "nope"})
        )
        .contains("aspect"));
        assert!(
            bad("video_generate", json!({"prompt": "x", "resolution": "4k"}))
                .contains("resolution")
        );
        assert!(bad("video_generate", json!({"prompt": "x", "duration": 0})).contains("duration"));
        assert!(bad("video_generate", json!({"prompt": "x", "duration": 16})).contains("duration"));
        assert!(bad(
            "video_generate",
            json!({"prompt": "x", "generate_audio": "yes"})
        )
        .contains("generate_audio"));
        assert!(bad("image_edit", json!({"prompt": "x"})).contains("image"));
        assert!(bad(
            "image_edit",
            json!({"prompt": "x", "images": ["a", "b", "c", "d"]})
        )
        .contains("3"));
        assert!(bad("video_edit", json!({"prompt": "x"})).contains("video"));
        let extended = body_of(
            "video_extend",
            json!({"prompt": "x", "video": "https://cdn.example/v.mp4"}),
        );
        assert_eq!(extended.body["duration"], 6);
    }

    /// A scratch config folder (own in-memory keyring) pinned for this thread.
    pub(crate) fn scratch_config() -> (std::path::PathBuf, crate::perm::ConfigGuard) {
        static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = crate::harness::test_dir(&format!("media-{n}"));
        let cfg = crate::perm::ConfigGuard::set(&dir);
        (dir, cfg)
    }

    struct FakeImagine {
        posts: Mutex<Vec<(String, Value)>>,
        gets: Mutex<Vec<String>>,
    }

    impl ImagineApi for FakeImagine {
        fn post_json(&self, path: &str, body: &Value) -> Result<Value, String> {
            self.posts
                .lock()
                .unwrap_or_else(|err| err.into_inner())
                .push((path.to_string(), body.clone()));
            if path.starts_with("videos/") {
                Ok(json!({"request_id": "req-1"}))
            } else {
                Ok(json!({"data": [
                    {"url": "https://cdn.example/cabin.png"},
                    {"url": "https://cdn.example/cabin-2.png"}
                ]}))
            }
        }

        fn get_json(&self, path: &str) -> Result<Value, String> {
            self.gets
                .lock()
                .unwrap_or_else(|err| err.into_inner())
                .push(path.to_string());
            Ok(json!({
                "status": "done",
                "video": {"url": "https://cdn.example/cabin.mp4"}
            }))
        }
    }

    #[test]
    fn media_fake_transport_returns_imagine_card_text() {
        let args = json!({
            "prompt": "a cabin",
            "n": 2,
            "resolution": "2k",
            "quality": "low",
            "model": "grok-imagine-image-2.0"
        });
        let call = build_media_call("image_generate", &args).unwrap();
        let fake = FakeImagine {
            posts: Mutex::new(Vec::new()),
            gets: Mutex::new(Vec::new()),
        };
        let out = execute("image_generate", &args, &fake);
        assert!(!out.failed, "{}", out.text);
        assert!(out.text.starts_with("IMAGINE: "), "{}", out.text);
        assert!(
            out.text.contains("https://cdn.example/cabin.png"),
            "{}",
            out.text
        );
        assert!(
            out.text.contains("https://cdn.example/cabin-2.png"),
            "{}",
            out.text
        );
        let posts = fake.posts.lock().unwrap();
        assert_eq!(posts.len(), 1);
        assert_eq!(posts[0].0, "images/generations");
        assert_eq!(posts[0].1, call.body);
        assert!(fake.gets.lock().unwrap().is_empty());

        let vargs = json!({
            "prompt": "waves",
            "resolution": "1080p",
            "model": "grok-imagine-video-1.5"
        });
        let vcall = build_media_call("video_generate", &vargs).unwrap();
        let video = FakeImagine {
            posts: Mutex::new(Vec::new()),
            gets: Mutex::new(Vec::new()),
        };
        let out = execute("video_generate", &vargs, &video);
        assert!(!out.failed, "{}", out.text);
        assert_eq!(out.text, "IMAGINE: https://cdn.example/cabin.mp4");
        assert_eq!(video.posts.lock().unwrap()[0].1, vcall.body);
        assert_eq!(video.gets.lock().unwrap().as_slice(), ["videos/req-1"]);

        let blocked = execute("image_generate", &args, &BlockedImagine);
        assert!(blocked.failed);
        assert!(blocked.text.contains("Sign in"));
    }

    fn policy(rules: Vec<crate::perm::Rule>) -> Policy {
        Policy {
            rules,
            grants: Vec::new(),
        }
    }

    fn chat(attended: bool, mode: PermMode, readonly: bool) -> Gate {
        Gate {
            mode,
            readonly_session: readonly,
            attended,
            desktop: false,
        }
    }

    #[test]
    fn media_rules_deny_ask_allow_and_unattended() {
        let args = r#"{"prompt":"a cabin"}"#;
        let ws = Path::new(".");
        let deny = parse_rule("image_generate", Action::Deny).unwrap();
        let allow = parse_rule("image_generate", Action::Allow).unwrap();
        let ask = parse_rule("image_generate", Action::Ask).unwrap();
        let star = parse_rule("*", Action::Allow).unwrap();
        let bash = parse_rule("Bash(*)", Action::Allow).unwrap();

        let decided = decide_with(
            &chat(false, PermMode::Ask, false),
            "image_generate",
            args,
            false,
            None,
            ws,
            Some(&policy(vec![star, deny])),
        );
        assert!(matches!(decided, Decision::Refuse(_)), "{decided:?}");

        let allowed = decide_with(
            &chat(false, PermMode::Ask, false),
            "image_generate",
            args,
            false,
            None,
            ws,
            Some(&policy(vec![allow])),
        );
        assert_eq!(allowed, Decision::Run);

        // Always Allow runs past an ask rule (only hard classes stop it); Supervised asks.
        let always = decide_with(
            &chat(true, PermMode::Always, false),
            "image_generate",
            args,
            false,
            None,
            ws,
            Some(&policy(vec![ask.clone()])),
        );
        assert_eq!(always, Decision::Run);
        let asked = decide_with(
            &chat(true, PermMode::Ask, false),
            "image_generate",
            args,
            false,
            None,
            ws,
            Some(&policy(vec![ask.clone()])),
        );
        assert_eq!(asked, Decision::Ask);
        let asked_away = decide_with(
            &chat(false, PermMode::Ask, false),
            "video_extend",
            args,
            false,
            None,
            ws,
            Some(&policy(vec![
                parse_rule("video_extend", Action::Ask).unwrap()
            ])),
        );
        assert!(matches!(asked_away, Decision::Refuse(_)), "{asked_away:?}");

        let bare = decide_with(
            &chat(false, PermMode::Ask, false),
            "video_generate",
            args,
            false,
            None,
            ws,
            None,
        );
        assert!(matches!(bare, Decision::Refuse(_)), "{bare:?}");

        let shell = decide_with(
            &chat(false, PermMode::Ask, false),
            "image_edit",
            args,
            false,
            None,
            ws,
            Some(&policy(vec![bash])),
        );
        assert!(matches!(shell, Decision::Refuse(_)), "{shell:?}");

        let plan = decide_with(
            &chat(true, PermMode::Always, true),
            "image_generate",
            args,
            false,
            None,
            ws,
            Some(&policy(vec![
                parse_rule("image_generate", Action::Allow).unwrap()
            ])),
        );
        match plan {
            Decision::Refuse(text) => assert!(text.contains(gate::READ_ONLY_PHASE), "{text}"),
            other => panic!("{other:?}"),
        }

        struct AllowJudge;
        impl ModelClient for AllowJudge {
            fn stream(
                &self,
                _req: &ResponsesRequest,
                _cancel: &CancelToken,
                _sink: &mut dyn FnMut(StreamEvent),
            ) -> Result<TurnOutput, ClientError> {
                Ok(TurnOutput {
                    text: r#"{"verdict":"allow","reason":"draw the cabin"}"#.into(),
                    reasoning: String::new(),
                    calls: Vec::new(),
                    usage: Usage::default(),
                })
            }
        }
        let auto = chat(true, PermMode::Auto, false);
        let empty = policy(Vec::new());
        let base = decide_with(&auto, "image_generate", args, false, None, ws, Some(&empty));
        assert_eq!(base, Decision::Ask);
        let client = AllowJudge;
        let cancel = CancelToken::new();
        let halt = || false;
        let reviewed = review(&ReviewIn {
            client: &client,
            cancel: &cancel,
            halt: &halt,
            gate: &auto,
            name: "image_generate",
            arguments: args,
            workspace: ws,
            policy: Some(&empty),
            latched_always: false,
            desk: None,
            history: &[],
            conversation_id: "judge",
            base,
            timeout: Duration::from_secs(2),
        });
        assert_eq!(reviewed.decision, Decision::Run);

        let explicit = policy(vec![ask]);
        let base = decide_with(
            &auto,
            "image_generate",
            args,
            false,
            None,
            ws,
            Some(&explicit),
        );
        assert_eq!(base, Decision::Ask);
        let reviewed = review(&ReviewIn {
            client: &client,
            cancel: &cancel,
            halt: &halt,
            gate: &auto,
            name: "image_generate",
            arguments: args,
            workspace: ws,
            policy: Some(&explicit),
            latched_always: false,
            desk: None,
            history: &[],
            conversation_id: "judge",
            base: base.clone(),
            timeout: Duration::from_secs(2),
        });
        assert_eq!(reviewed.decision, Decision::Ask);
    }

    struct Bytes {
        pending: bool,
    }

    impl ImagineApi for Bytes {
        fn post_json(&self, path: &str, _body: &Value) -> Result<Value, String> {
            if path.starts_with("videos/") {
                Ok(json!({"request_id": "req-9"}))
            } else {
                Ok(json!({"data": [
                    {"url": "https://imgen.x.ai/a.png"},
                    {"url": "https://imgen.x.ai/b.png"}
                ]}))
            }
        }

        fn get_json(&self, _path: &str) -> Result<Value, String> {
            if self.pending {
                Ok(json!({"status": "pending"}))
            } else {
                Ok(json!({"status": "done", "video": {"url": "https://vidgen.x.ai/c.mp4"}}))
            }
        }

        fn download(&self, url: &str) -> Result<Vec<u8>, String> {
            if url.ends_with("b.png") {
                return Err("HTTP 404".into());
            }
            let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();
            bytes.resize(64, 7);
            Ok(bytes)
        }
    }

    #[test]
    fn media_results_are_saved_inside_the_session_folder() {
        let root = std::env::temp_dir().join(format!(
            "grokhub-media-{}-{}",
            std::process::id(),
            grokhub_core::now_ms()
        ));
        let dir = root.join("sessions").join("native-x").join("media");
        let (cfg_dir, _cfg) = scratch_config();
        let args = json!({"prompt": "a cabin", "n": 2});
        let out = execute_with(
            "image_generate",
            &args,
            &Bytes { pending: false },
            &|| false,
            Some(&dir),
        );
        assert!(!out.failed, "{}", out.text);
        let lines: Vec<&str> = out.text.lines().collect();
        let first = lines[0].strip_prefix("IMAGINE: ").unwrap();
        let saved = std::path::PathBuf::from(first);
        assert!(saved.starts_with(&dir), "{}", out.text);
        assert_eq!(saved.extension().and_then(|e| e.to_str()), Some("png"));
        assert!(saved.is_file());
        // A failed download keeps the remote URL and says so. Nothing is written elsewhere.
        assert!(
            out.text.contains("IMAGINE: https://imgen.x.ai/b.png"),
            "{}",
            out.text
        );
        assert!(out.text.contains("(not saved: HTTP 404)"), "{}", out.text);
        let names: Vec<_> = std::fs::read_dir(&dir).unwrap().flatten().collect();
        assert_eq!(names.len(), 1);
        // One line for the prompt, one per download: hosts only, no prompt, no path.
        let log = crate::harness::read_egress(&cfg_dir);
        let rows: Vec<(&str, &str)> = log.iter().map(|l| (l.dest.as_str(), l.basis.as_str())).collect();
        assert_eq!(rows, vec![("api.x.ai", "model_host"), ("imgen.x.ai", "public"), ("imgen.x.ai", "public")]);
        assert!(!format!("{log:?}").contains("cabin") && !format!("{log:?}").contains(".png"), "{log:?}");
        let _ = std::fs::remove_dir_all(&cfg_dir);
        let _ = std::fs::remove_dir_all(&root);
        assert_eq!(
            https_host("https://imgen.x.ai/a.png").as_deref(),
            Some("imgen.x.ai")
        );
        assert_eq!(https_host("http://imgen.x.ai/a.png"), None);
        assert_eq!(https_host("https://user@evil.example/a.png"), None);
    }

    #[test]
    fn video_poll_stops_on_cancel() {
        let (cfg_dir, _cfg) = scratch_config();
        let stop_at = std::time::Instant::now() + Duration::from_millis(300);
        let start = std::time::Instant::now();
        let out = execute_with(
            "video_generate",
            &json!({"prompt": "waves"}),
            &Bytes { pending: true },
            &|| std::time::Instant::now() >= stop_at,
            None,
        );
        assert!(out.failed);
        assert!(out.text.contains("cancelled"), "{}", out.text);
        assert!(
            start.elapsed() < Duration::from_secs(3),
            "{:?}",
            start.elapsed()
        );
        let _ = std::fs::remove_dir_all(cfg_dir);
    }
}
