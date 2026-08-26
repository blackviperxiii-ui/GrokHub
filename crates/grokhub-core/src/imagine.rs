use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub const DEFAULT_IMAGINE_MODEL: &str = "grok-imagine-image-2.0";
pub const DEFAULT_VIDEO_MODEL: &str = "grok-imagine-video-1.5";
/// OAuth / older console keys often allow these when the 2.0 / 1.5 ids 404.
pub const FALLBACK_IMAGINE_MODEL: &str = "grok-imagine-image";
pub const FALLBACK_VIDEO_MODEL: &str = "grok-imagine-video";

/// xAI retired grok-2-image / grok-2-image-1212 (EOL 2026-02-28).
pub fn retired_imagine_model(user: &str) -> bool {
    let u = user.trim().to_ascii_lowercase();
    u == "grok-2-image" || u.starts_with("grok-2-image-")
}

/// Imagine never shares the chat model. Only a live *image* model wins.
pub fn dedicated_imagine_model(user: &str) -> String {
    let u = user.trim();
    if u.contains("image") && !retired_imagine_model(u) {
        u.to_string()
    } else {
        DEFAULT_IMAGINE_MODEL.to_string()
    }
}

/// Video never shares the chat or still-image model.
pub fn dedicated_video_model(user: &str) -> String {
    let u = user.trim();
    if u.contains("video") {
        u.to_string()
    } else {
        DEFAULT_VIDEO_MODEL.to_string()
    }
}

pub fn imagine_request_body(prompt: &str, model: &str) -> Value {
    imagine_image_body(prompt, model, None, None)
}

pub fn imagine_image_body(
    prompt: &str,
    model: &str,
    aspect: Option<&str>,
    resolution: Option<&str>,
) -> Value {
    imagine_image_shaped(prompt, model, aspect, resolution, None)
}

/// `quality` is only valid on grok-imagine-image-2.0 (`low` / `medium`).
pub fn imagine_image_shaped(
    prompt: &str,
    model: &str,
    aspect: Option<&str>,
    resolution: Option<&str>,
    quality: Option<&str>,
) -> Value {
    let model = dedicated_imagine_model(model);
    let mut body = json!({
        "model": model,
        "prompt": prompt,
        "n": 1,
        "response_format": "url",
    });
    if let Some(a) = aspect.map(str::trim).filter(|s| !s.is_empty()) {
        body["aspect_ratio"] = json!(a);
    }
    if let Some(r) = resolution.map(str::trim).filter(|s| !s.is_empty()) {
        body["resolution"] = json!(r);
    }
    let q = quality
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_ascii_lowercase())
        .or_else(|| match resolution.map(str::trim) {
            Some("1k") => Some("low".into()),
            Some("2k") => Some("medium".into()),
            _ => None,
        });
    if model.contains("2.0") {
        if let Some(q) = q {
            if q == "low" || q == "medium" {
                body["quality"] = json!(q);
            }
        }
    }
    body
}

pub fn imagine_image_quality(quality: bool) -> &'static str {
    if quality {
        "medium"
    } else {
        "low"
    }
}

pub fn imagine_image_fallback_model(model: &str) -> Option<&'static str> {
    let m = dedicated_imagine_model(model);
    if m == FALLBACK_IMAGINE_MODEL {
        None
    } else {
        Some(FALLBACK_IMAGINE_MODEL)
    }
}

pub fn imagine_video_fallback_model(model: &str) -> Option<&'static str> {
    let m = dedicated_video_model(model);
    if m == FALLBACK_VIDEO_MODEL {
        None
    } else {
        Some(FALLBACK_VIDEO_MODEL)
    }
}

/// Retry the cheaper Imagine alias when 2.0 / 1.5 is missing, slow, or times out.
pub fn imagine_should_retry_model(err: &str) -> bool {
    let e = err.to_ascii_lowercase();
    if e.contains("bad credentials")
        || e.contains("unauthenticated")
        || e.contains("http 401")
    {
        return false;
    }
    e.contains("timeout")
        || e.contains("timed out")
        || e.contains("timedout")
        || e.contains("model")
        || e.contains("not found")
        || e.contains("does not exist")
        || e.contains("unknown")
        || e.contains("invalid_argument")
        || e.contains("empty imagine")
        || e.contains("empty video")
        || e.contains("http 404")
        || e.contains("http 400")
        || e.contains("http 403")
        || e.contains("http 429")
        || e.contains("http 5")
}

pub fn imagine_image_resolution(quality: bool) -> &'static str {
    if quality {
        "2k"
    } else {
        "1k"
    }
}

pub fn imagine_video_duration_secs(label: &str) -> u32 {
    label
        .trim()
        .trim_end_matches(['s', 'S'])
        .parse::<u32>()
        .unwrap_or(6)
        .clamp(1, 15)
}

pub fn imagine_video_resolution(label: &str) -> &'static str {
    match label.trim() {
        "720p" => "720p",
        "1080p" => "1080p",
        _ => "480p",
    }
}

pub fn video_request_body(
    prompt: &str,
    model: &str,
    duration: u32,
    aspect: &str,
    resolution: &str,
) -> Value {
    json!({
        "model": dedicated_video_model(model),
        "prompt": prompt,
        "duration": duration.clamp(1, 15),
        "aspect_ratio": aspect.trim(),
        "resolution": imagine_video_resolution(resolution),
    })
}

fn nonempty_json_str(v: Option<&Value>) -> Option<String> {
    v.and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
}

pub fn parse_video_request_id(body: &Value) -> Option<String> {
    nonempty_json_str(body.get("request_id")).or_else(|| nonempty_json_str(body.get("id")))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VideoJobStatus {
    Pending,
    Done,
    Failed,
    Expired,
}

pub fn parse_video_job_status(status: &str) -> VideoJobStatus {
    match status.trim().to_ascii_lowercase().as_str() {
        "done" => VideoJobStatus::Done,
        "failed" => VideoJobStatus::Failed,
        "expired" => VideoJobStatus::Expired,
        "pending" | "" => VideoJobStatus::Pending,
        _ => VideoJobStatus::Pending,
    }
}

pub fn parse_video_url(body: &Value) -> Option<String> {
    let video = body.get("video")?;
    nonempty_json_str(video.get("url")).or_else(|| {
        video
            .get("file_output")
            .and_then(|f| nonempty_json_str(f.get("public_url")))
    })
}

pub fn video_moderation_blocked(body: &Value) -> bool {
    body.get("video")
        .and_then(|v| v.get("respect_moderation"))
        .and_then(|b| b.as_bool())
        == Some(false)
}

pub fn imagine_is_video_path(path: &str) -> bool {
    let ext = path
        .rsplit('.')
        .next()
        .unwrap_or("")
        .split(|c: char| c == '?' || c == '#')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    matches!(ext.as_str(), "mp4" | "webm" | "mov")
}

pub fn imagine_slug(prompt: &str) -> String {
    let s: String = prompt
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .take(40)
        .collect();
    let s = s.trim_matches('-').to_string();
    if s.is_empty() {
        "imagine".into()
    } else {
        s
    }
}

fn imagine_item_url(data: &Value) -> Option<String> {
    nonempty_json_str(data.get("url"))
        .or_else(|| {
            nonempty_json_str(data.get("b64_json")).map(|s| {
                if s.starts_with("data:") {
                    s
                } else {
                    format!("data:image/png;base64,{s}")
                }
            })
        })
        .or_else(|| {
            data.get("file_output")
                .and_then(|f| nonempty_json_str(f.get("public_url")))
        })
}

pub fn parse_imagine_url(body: &Value) -> Option<String> {
    if let Some(data) = body
        .get("data")
        .and_then(|d| d.as_array())
        .and_then(|a| a.first())
    {
        if let Some(u) = imagine_item_url(data) {
            return Some(u);
        }
    }
    nonempty_json_str(body.get("url"))
}

/// File extension from magic bytes so a jpeg/webp still is not saved as `.png`.
pub fn media_ext_from_bytes<'a>(buf: &'a [u8], fallback: &'a str) -> &'a str {
    if buf.starts_with(&[0x89, b'P', b'N', b'G']) {
        "png"
    } else if buf.len() >= 3 && buf[0] == 0xFF && buf[1] == 0xD8 && buf[2] == 0xFF {
        "jpg"
    } else if buf.len() >= 12 && &buf[0..4] == b"RIFF" && &buf[8..12] == b"WEBP" {
        "webp"
    } else if buf.len() >= 8 && &buf[4..8] == b"ftyp" {
        "mp4"
    } else {
        fallback
    }
}

/// grok.com/imagine Aspect Ratio menu, measured 2026-08-15.
pub const IMAGINE_ASPECTS: &[(&str, &str)] = &[
    ("2:3", "Tall"),
    ("3:2", "Wide"),
    ("1:1", "Square"),
    ("9:16", "Vertical"),
    ("16:9", "Widescreen"),
];

/// Style Auto menu. Cabin stills only — each label becomes a prompt suffix.
pub const IMAGINE_STYLES: &[&str] = &["Auto", "Cinematic", "Anime", "Comic", "Photo", "Illustration"];

/// grok.com/imagine Video-mode chips, measured 2026-08-15.
pub const IMAGINE_VIDEO_RES: &[&str] = &["480p", "720p"];
pub const IMAGINE_VIDEO_DURS: &[&str] = &["6s", "10s", "15s"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImagineKind {
    Image,
    Video,
    Agent,
}

pub struct ImagineSpec<'a> {
    pub prompt: &'a str,
    pub kind: ImagineKind,
    pub quality: bool,
    pub style: &'a str,
    pub aspect: &'a str,
    pub video_res: &'a str,
    pub video_dur: &'a str,
    pub video_audio: bool,
}

pub fn imagine_aspect_label(i: u8) -> &'static str {
    IMAGINE_ASPECTS[(i as usize) % IMAGINE_ASPECTS.len()].0
}

pub fn imagine_aspect_name(i: u8) -> &'static str {
    IMAGINE_ASPECTS[(i as usize) % IMAGINE_ASPECTS.len()].1
}

pub fn imagine_style_label(i: u8) -> &'static str {
    IMAGINE_STYLES[(i as usize) % IMAGINE_STYLES.len()]
}

pub fn imagine_video_res_label(i: u8) -> &'static str {
    IMAGINE_VIDEO_RES[(i as usize) % IMAGINE_VIDEO_RES.len()]
}

pub fn imagine_video_dur_label(i: u8) -> &'static str {
    IMAGINE_VIDEO_DURS[(i as usize) % IMAGINE_VIDEO_DURS.len()]
}

/// Image/Agent selectors suffix the still prompt. Video duration/res go to the video API.
pub fn compose_imagine_prompt(spec: &ImagineSpec<'_>) -> String {
    let prompt = spec.prompt.trim();
    let mut parts = Vec::new();
    if !prompt.is_empty() {
        parts.push(prompt.to_string());
    }
    let aspect = spec.aspect.trim();
    match spec.kind {
        ImagineKind::Video => {}
        ImagineKind::Image | ImagineKind::Agent => {
            if !aspect.is_empty() && !prompt.contains(aspect) {
                parts.push(format!("{aspect} still"));
            } else if !prompt.to_ascii_lowercase().contains("still") {
                parts.push("still".into());
            }
        }
    }
    match spec.kind {
        ImagineKind::Image => {
            if spec.quality {
                parts.push("high detail, quality v2.0".into());
            } else {
                parts.push("speed draft".into());
            }
        }
        ImagineKind::Video => {
            if spec.video_audio {
                parts.push("with diegetic sound".into());
            } else {
                parts.push("silent, no speech".into());
            }
        }
        ImagineKind::Agent => {
            parts.push("character sprite still, living agent pose".into());
            if spec.quality {
                parts.push("high detail, quality v2.0".into());
            } else {
                parts.push("speed draft".into());
            }
        }
    }
    let style = spec.style.trim();
    if !style.is_empty() && !style.eq_ignore_ascii_case("auto") {
        parts.push(format!("{style} style"));
    }
    parts.join(", ")
}

/// Assistant verb that kicks Imagine. Distinct from `IMAGINE: <url>` receipts.
pub fn extract_imagine_prompt(text: &str) -> Option<String> {
    for line in text.lines() {
        let Some(rest) = line.trim().strip_prefix("IMAGINE_PROMPT:") else {
            continue;
        };
        let p = rest.trim();
        if !p.is_empty() {
            return Some(p.chars().take(400).collect());
        }
    }
    None
}

/// Filesystem path (or URL) from an `IMAGINE: …` receipt. Not a prompt verb.
pub fn imagine_receipt_path(text: &str) -> Option<String> {
    for line in text.lines() {
        let Some(rest) = line.trim().strip_prefix("IMAGINE:") else {
            continue;
        };
        let p = rest.trim();
        if !p.is_empty() {
            return Some(p.to_string());
        }
    }
    None
}

/// Last generated still in a thread. Newer receipts win.
pub fn last_imagine_receipt<'a, I>(messages: I) -> Option<String>
where
    I: IntoIterator<Item = &'a str>,
{
    let mut last = None;
    for text in messages {
        if let Some(path) = imagine_receipt_path(text) {
            last = Some(path);
        }
    }
    last
}

/// Gap above the pane floor when the Imagine toolbox docks to the bottom.
pub const IMAGINE_TOOLBOX_PAD: f32 = 24.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImagineToolboxDock {
    Middle,
    Bottom,
}

pub fn imagine_toolbox_dock(
    _prompt_filled: bool,
    has_result: bool,
    working: bool,
) -> ImagineToolboxDock {
    if working || has_result {
        ImagineToolboxDock::Bottom
    } else {
        ImagineToolboxDock::Middle
    }
}

/// Generating / finished still sits above the docked chat box.
pub fn imagine_stage_visible(working: bool, has_result: bool) -> bool {
    working || has_result
}

fn imagine_aspect_wh(aspect: &str) -> (f32, f32) {
    match aspect.trim() {
        "3:2" => (3.0, 2.0),
        "1:1" => (1.0, 1.0),
        "9:16" => (9.0, 16.0),
        "16:9" => (16.0, 9.0),
        _ => (2.0, 3.0),
    }
}

/// Height of the generating/result card in leftover space above the chat box.
pub fn imagine_stage_h(avail_above: f32, aspect: &str, bar_w: f32) -> f32 {
    let (w, h) = imagine_aspect_wh(aspect);
    let want = bar_w.max(1.0) * (h / w.max(0.01));
    let cap = (avail_above * 0.88).max(0.0);
    if cap < 48.0 {
        cap
    } else {
        want.min(cap).max(48.0)
    }
}

pub fn imagine_toolbox_shows_title(dock: ImagineToolboxDock) -> bool {
    match dock {
        ImagineToolboxDock::Middle => true,
        ImagineToolboxDock::Bottom => false,
    }
}

pub fn imagine_toolbox_top(
    content_top: f32,
    content_h: f32,
    box_h: f32,
    dock: ImagineToolboxDock,
) -> f32 {
    match dock {
        ImagineToolboxDock::Middle => content_top + ((content_h - box_h) * 0.5).max(0.0),
        ImagineToolboxDock::Bottom => {
            (content_top + content_h - box_h - IMAGINE_TOOLBOX_PAD).max(content_top)
        }
    }
}

/// Gap between the Imagine chat box and the photogif wall.
pub const IMAGINE_WALL_GAP: f32 = IMAGINE_TOOLBOX_PAD;

/// Top and height of the photogif wall.
/// Idle: under the chat box. Generating: under the stage, above the docked box.
pub fn imagine_wall_bounds(
    content_top: f32,
    content_h: f32,
    toolbox_top: f32,
    toolbox_h: f32,
    dock: ImagineToolboxDock,
    stage_h: f32,
) -> (f32, f32) {
    let content_bottom = content_top + content_h;
    match dock {
        ImagineToolboxDock::Middle => {
            let top = toolbox_top + toolbox_h + IMAGINE_WALL_GAP;
            (top, (content_bottom - top).max(0.0))
        }
        ImagineToolboxDock::Bottom => {
            let band_bottom = (toolbox_top - IMAGINE_WALL_GAP).max(content_top);
            let top = if stage_h > 0.0 {
                content_top + stage_h + IMAGINE_WALL_GAP
            } else {
                content_top
            };
            (top, (band_bottom - top).max(0.0))
        }
    }
}

pub fn imagine_wall_overlaps_toolbox(
    wall_top: f32,
    wall_h: f32,
    toolbox_top: f32,
    toolbox_h: f32,
) -> bool {
    let wall_bottom = wall_top + wall_h;
    let toolbox_bottom = toolbox_top + toolbox_h;
    wall_top < toolbox_bottom && toolbox_top < wall_bottom
}

/// Result lives in the stage under the chat box, not as a wall takeover.
pub fn imagine_shows_result_above(_has_result: bool, _dock: ImagineToolboxDock) -> bool {
    false
}

/// Letterbox a still inside the wall so the full generated image sits above the chat box.
pub fn imagine_result_fit(
    wall_x: f32,
    wall_y: f32,
    wall_w: f32,
    wall_h: f32,
    img_w: f32,
    img_h: f32,
) -> (f32, f32, f32, f32) {
    let iw = img_w.max(1.0);
    let ih = img_h.max(1.0);
    let ww = wall_w.max(0.0);
    let wh = wall_h.max(0.0);
    if ww <= 0.0 || wh <= 0.0 {
        return (wall_x, wall_y, 0.0, 0.0);
    }
    let ia = iw / ih;
    let da = ww / wh;
    if ia > da {
        let h = ww / ia;
        let y = wall_y + (wh - h) * 0.5;
        (wall_x, y, ww, h)
    } else {
        let w = wh * ia;
        let x = wall_x + (ww - w) * 0.5;
        (wall_x.max(x), wall_y, w, wh)
    }
}

/// Twenty live covers. Oldest leaves first.
pub const WALL_GIF_MAX: usize = 20;
/// A new cover every few hours.
pub const WALL_GIF_EVERY_MS: u64 = 3 * 60 * 60 * 1000;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImagineWall {
    #[serde(default)]
    pub last_ms: u64,
    #[serde(default)]
    pub gifs: Vec<WallGif>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WallGif {
    pub id: String,
    pub title: String,
    pub prompt: String,
    pub created_ms: u64,
    pub path_a: String,
    pub path_b: String,
    pub tall: bool,
}

/// Pin a generated still or video onto the Imagine wall.
pub fn wall_gif_from_generation(
    path: &str,
    prompt: &str,
    created_ms: u64,
    aspect: &str,
) -> WallGif {
    let path = path.trim().to_string();
    let title = imagine_slug(prompt).replace('-', " ");
    let tall = matches!(aspect.trim(), "2:3" | "9:16");
    WallGif {
        id: format!("gen-{created_ms:x}"),
        title,
        prompt: prompt.trim().chars().take(400).collect(),
        created_ms,
        path_a: path.clone(),
        path_b: path,
        tall,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WallSeed {
    pub title: &'static str,
    pub prompt: &'static str,
    pub prompt_b: &'static str,
    pub tall: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WallSlot {
    Stock(usize),
    Gif(usize),
}

/// Cabin-real stills the night can paint. No video, no faces, no photo-edit verbs.
pub const WALL_SEEDS: &[WallSeed] = &[
    WallSeed {
        title: "Ember night",
        prompt: "still of dying embers in a dark timber cabin stove, no people, no text",
        prompt_b: "still of the same cabin stove a breath later, closer on the grate, no people, no text",
        tall: true,
    },
    WallSeed {
        title: "Snow porch",
        prompt: "still of a cabin porch at night, snow on the rail, one lantern, no people, no text",
        prompt_b: "still of the same snow porch, wider, pines beyond the rail, no people, no text",
        tall: false,
    },
    WallSeed {
        title: "Kettle steam",
        prompt: "still of a black kettle on a wood stove, faint steam, dark cabin, no people, no text",
        prompt_b: "still of the same kettle, closer, steam catching lamplight, no people, no text",
        tall: true,
    },
    WallSeed {
        title: "Bound ledger",
        prompt: "still of a bound ledger on a worn desk, cabin lamp, no people, no readable text",
        prompt_b: "still of the same ledger, pages half turned, no people, no readable text",
        tall: false,
    },
    WallSeed {
        title: "Frost pane",
        prompt: "still of frost on a cabin window at dawn, dark room, no people, no text",
        prompt_b: "still of the same frosted pane, closer crystals, no people, no text",
        tall: true,
    },
    WallSeed {
        title: "Tool wall",
        prompt: "still of hand tools hung on dark cabin wood, warm lamp, no people, no text",
        prompt_b: "still of the same tool wall, tighter crop, no people, no text",
        tall: false,
    },
    WallSeed {
        title: "Creek ice",
        prompt: "still of a frozen creek below pines at night, no people, no text",
        prompt_b: "still of the same creek, closer on the ice edge, no people, no text",
        tall: false,
    },
    WallSeed {
        title: "Oil lamp",
        prompt: "still of an oil lamp on a cabin table, dark room, no people, no text",
        prompt_b: "still of the same lamp, glass glowing, no people, no text",
        tall: true,
    },
    WallSeed {
        title: "Split wood",
        prompt: "still of split firewood stacked by a cabin door, night, no people, no text",
        prompt_b: "still of the same woodpile, closer bark and frost, no people, no text",
        tall: false,
    },
    WallSeed {
        title: "Empty mug",
        prompt: "still of an empty enamel mug on a windowsill, cabin night, no people, no text",
        prompt_b: "still of the same mug, frost on the pane behind it, no people, no text",
        tall: true,
    },
    WallSeed {
        title: "Ridge wind",
        prompt: "still of a wind-cut pine ridge above a dark valley, no people, no text",
        prompt_b: "still of the same ridge, clouds moving in, no people, no text",
        tall: false,
    },
    WallSeed {
        title: "Wool blanket",
        prompt: "still of a folded wool blanket on a wooden chair, cabin lamp, no people, no text",
        prompt_b: "still of the same chair, blanket slightly shifted, no people, no text",
        tall: true,
    },
    WallSeed {
        title: "Host glow",
        prompt: "still of a Linux workstation in a dark cabin, monitor glow, no people, no faces, no text",
        prompt_b: "still of the same desk, closer on the dark wood and keys, no people, no text",
        tall: true,
    },
    WallSeed {
        title: "Night path",
        prompt: "still of a snow path to a cabin, one window lit, no people, no text",
        prompt_b: "still of the same path, a few steps closer, no people, no text",
        tall: false,
    },
    WallSeed {
        title: "Iron latch",
        prompt: "still of an iron latch on a heavy cabin door, lamplight, no people, no text",
        prompt_b: "still of the same latch, closer metal grain, no people, no text",
        tall: true,
    },
    WallSeed {
        title: "Quiet shelf",
        prompt: "still of a cabin shelf of blank notebooks, warm lamp, no people, no readable text",
        prompt_b: "still of the same shelf, one book pulled forward, no people, no readable text",
        tall: false,
    },
    WallSeed {
        title: "Ash bucket",
        prompt: "still of an ash bucket beside a wood stove, dark cabin, no people, no text",
        prompt_b: "still of the same bucket, embers reflecting, no people, no text",
        tall: true,
    },
    WallSeed {
        title: "Pine table",
        prompt: "still of a bare pine table, one lamp, empty cabin, no people, no text",
        prompt_b: "still of the same table, wider, chairs in shadow, no people, no text",
        tall: false,
    },
];

pub fn wall_due(last_ms: u64, now_ms: u64, interval_ms: u64) -> bool {
    if last_ms == 0 {
        return true;
    }
    now_ms.saturating_sub(last_ms) >= interval_ms
}

pub fn wall_can_paint(
    has_key: bool,
    wall_on: bool,
    wall_busy: bool,
    running: bool,
    quiet: bool,
    last_ms: u64,
    now_ms: u64,
) -> bool {
    if !has_key || !wall_on || wall_busy || running {
        return false;
    }
    if last_ms == 0 {
        return true;
    }
    if quiet {
        return false;
    }
    wall_due(last_ms, now_ms, WALL_GIF_EVERY_MS)
}

pub fn wall_evict(mut gifs: Vec<WallGif>, max: usize) -> (Vec<WallGif>, Vec<WallGif>) {
    gifs.sort_by(|a, b| a.created_ms.cmp(&b.created_ms).then_with(|| a.id.cmp(&b.id)));
    if gifs.len() <= max {
        return (gifs, Vec::new());
    }
    let drop_n = gifs.len() - max;
    let evicted = gifs.drain(..drop_n).collect();
    (gifs, evicted)
}

pub fn pick_fresh_seed(roll: u64, taken: &[&str]) -> &'static WallSeed {
    let n = WALL_SEEDS.len().max(1);
    for i in 0..n {
        let s = &WALL_SEEDS[((roll as usize) + i) % n];
        if !taken.iter().any(|t| t.eq_ignore_ascii_case(s.title)) {
            return s;
        }
    }
    &WALL_SEEDS[(roll as usize) % n]
}

fn lcg(seed: u64) -> u64 {
    seed.wrapping_mul(6364136223846793005).wrapping_add(1)
}

pub fn curate_wall(stock_n: usize, gif_n: usize, seed: u64) -> Vec<WallSlot> {
    let mut slots: Vec<WallSlot> = (0..stock_n).map(WallSlot::Stock).collect();
    let mut order: Vec<usize> = (0..gif_n).collect();
    let mut s = seed | 1;
    if gif_n > 1 {
        for i in (1..order.len()).rev() {
            s = lcg(s);
            let j = (s as usize) % (i + 1);
            order.swap(i, j);
        }
    }
    for (k, gi) in order.into_iter().enumerate() {
        s = lcg(s);
        let at = (s as usize) % (stock_n + k + 1);
        slots.insert(at.min(slots.len()), WallSlot::Gif(gi));
    }
    slots
}

pub fn wall_curate_seed(gifs: &[WallGif]) -> u64 {
    let mut h = 0xcbf29ce484222325u64;
    for g in gifs {
        for b in g.id.as_bytes() {
            h ^= u64::from(*b);
            h = h.wrapping_mul(0x100000001b3);
        }
        h ^= g.created_ms;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

pub fn imagine_dest(project: Option<&str>) -> String {
    match project.filter(|s| !s.is_empty()) {
        Some(p) => format!("{p}/imagine"),
        None => "GrokHub-Work/imagine".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn body_and_url() {
        assert_eq!(DEFAULT_IMAGINE_MODEL, "grok-imagine-image-2.0");
        assert_eq!(dedicated_imagine_model("grok-3-mini-fast"), DEFAULT_IMAGINE_MODEL);
        assert_eq!(dedicated_imagine_model("grok-2-image"), DEFAULT_IMAGINE_MODEL);
        assert_eq!(dedicated_imagine_model("grok-2-image-1212"), DEFAULT_IMAGINE_MODEL);
        assert!(retired_imagine_model("grok-2-image-1212"));
        assert!(!retired_imagine_model("grok-imagine-image-2.0"));
        let b = imagine_request_body("a cabin at night", "grok-3-mini-fast");
        assert_eq!(b["model"], DEFAULT_IMAGINE_MODEL);
        assert_eq!(b["response_format"], "url");
        let shaped = imagine_image_body("a cabin at night", "grok-2-image", Some("2:3"), Some("2k"));
        assert_eq!(shaped["model"], DEFAULT_IMAGINE_MODEL);
        assert_eq!(shaped["aspect_ratio"], "2:3");
        assert_eq!(shaped["resolution"], "2k");
        assert_eq!(shaped["quality"], "medium");
        let draft = imagine_image_shaped(
            "a cabin at night",
            DEFAULT_IMAGINE_MODEL,
            Some("16:9"),
            Some("1k"),
            Some("low"),
        );
        assert_eq!(draft["quality"], "low");
        assert_eq!(draft["response_format"], "url");
        assert_eq!(dedicated_imagine_model(""), DEFAULT_IMAGINE_MODEL);
        assert_eq!(dedicated_imagine_model("grok-imagine"), DEFAULT_IMAGINE_MODEL);
        assert_eq!(
            dedicated_imagine_model("grok-imagine-image-2.0"),
            "grok-imagine-image-2.0"
        );
        let reply = json!({ "data": [{ "url": "https://img/x.png" }] });
        assert_eq!(parse_imagine_url(&reply).as_deref(), Some("https://img/x.png"));
        let empty_url = json!({ "data": [{ "url": "", "b64_json": "aaaa" }] });
        assert_eq!(
            parse_imagine_url(&empty_url).as_deref(),
            Some("data:image/png;base64,aaaa"),
            "an empty url must not hide b64_json"
        );
        let top = json!({ "url": "https://img/top.png" });
        assert_eq!(parse_imagine_url(&top).as_deref(), Some("https://img/top.png"));
        assert_eq!(
            parse_video_request_id(&json!({ "id": "vid-1" })).as_deref(),
            Some("vid-1")
        );
        assert_eq!(
            parse_video_url(&json!({
                "video": { "url": "", "file_output": { "public_url": "https://vid/x.mp4" } }
            }))
            .as_deref(),
            Some("https://vid/x.mp4")
        );
        assert!(video_moderation_blocked(&json!({
            "video": { "url": "", "respect_moderation": false }
        })));
        assert!(!imagine_should_retry_model("HTTP 401: Bad credentials."));
        assert!(imagine_should_retry_model(
            "HTTP 404: model grok-imagine-image-2.0 not found"
        ));
        assert!(imagine_should_retry_model("timed out waiting for 2.0"));
        assert!(imagine_should_retry_model("empty Imagine reply"));
        assert!(imagine_should_retry_model("empty video request_id"));
        assert_eq!(
            imagine_image_fallback_model(DEFAULT_IMAGINE_MODEL),
            Some(FALLBACK_IMAGINE_MODEL)
        );
        assert_eq!(
            imagine_video_fallback_model(DEFAULT_VIDEO_MODEL),
            Some(FALLBACK_VIDEO_MODEL)
        );
        assert_eq!(media_ext_from_bytes(&[0xFF, 0xD8, 0xFF, 0xE0], "png"), "jpg");
        assert_eq!(media_ext_from_bytes(b"nope", "png"), "png");
        assert_eq!(imagine_dest(None), "GrokHub-Work/imagine");
        assert_eq!(
            extract_imagine_prompt("ok\nIMAGINE_PROMPT: a cabin at night\n").as_deref(),
            Some("a cabin at night")
        );
        assert!(extract_imagine_prompt("IMAGINE: https://img/x.png").is_none());
    }

    #[test]
    fn grok_com_selectors_shape_the_still() {
        assert_eq!(
            IMAGINE_ASPECTS,
            &[
                ("2:3", "Tall"),
                ("3:2", "Wide"),
                ("1:1", "Square"),
                ("9:16", "Vertical"),
                ("16:9", "Widescreen"),
            ]
        );
        assert_eq!(IMAGINE_STYLES, &["Auto", "Cinematic", "Anime", "Comic", "Photo", "Illustration"]);
        assert_eq!(IMAGINE_VIDEO_RES, &["480p", "720p"]);
        assert_eq!(IMAGINE_VIDEO_DURS, &["6s", "10s", "15s"]);
        let quality = compose_imagine_prompt(&ImagineSpec {
            prompt: "a cabin porch at night",
            kind: ImagineKind::Image,
            quality: true,
            style: "Cinematic",
            aspect: "2:3",
            video_res: "480p",
            video_dur: "6s",
            video_audio: true,
        });
        assert!(quality.contains("a cabin porch at night"));
        assert!(quality.contains("2:3"));
        assert!(quality.to_ascii_lowercase().contains("cinematic"));
        assert!(quality.to_ascii_lowercase().contains("quality"));
        assert!(quality.to_ascii_lowercase().contains("still"));
        assert!(!quality.to_ascii_lowercase().contains("mp4"));
        let speed = compose_imagine_prompt(&ImagineSpec {
            prompt: "a kettle",
            kind: ImagineKind::Image,
            quality: false,
            style: "Auto",
            aspect: "16:9",
            video_res: "480p",
            video_dur: "6s",
            video_audio: false,
        });
        assert!(speed.contains("16:9"));
        assert!(speed.to_ascii_lowercase().contains("speed"));
        assert!(!speed.to_ascii_lowercase().contains("cinematic"));
        let video = compose_imagine_prompt(&ImagineSpec {
            prompt: "snow on the rail",
            kind: ImagineKind::Video,
            quality: true,
            style: "Auto",
            aspect: "9:16",
            video_res: "720p",
            video_dur: "10s",
            video_audio: true,
        });
        assert!(video.contains("snow on the rail"));
        assert!(video.to_ascii_lowercase().contains("sound") || video.to_ascii_lowercase().contains("audio"));
        assert!(!video.to_ascii_lowercase().contains("storyboard"));
        assert!(!video.to_ascii_lowercase().contains("still"));
        assert_eq!(imagine_video_duration_secs("10s"), 10);
        assert_eq!(imagine_video_resolution("720p"), "720p");
        let vbody = video_request_body("snow on the rail", "", 10, "9:16", "720p");
        assert_eq!(vbody["model"], DEFAULT_VIDEO_MODEL);
        assert_eq!(vbody["duration"], 10);
        assert_eq!(vbody["aspect_ratio"], "9:16");
        assert_eq!(vbody["resolution"], "720p");
        assert_eq!(
            parse_video_request_id(&json!({ "request_id": "abc-1" })).as_deref(),
            Some("abc-1")
        );
        assert_eq!(parse_video_job_status("done"), VideoJobStatus::Done);
        assert_eq!(parse_video_job_status("pending"), VideoJobStatus::Pending);
        assert_eq!(parse_video_job_status("failed"), VideoJobStatus::Failed);
        assert_eq!(
            parse_video_url(&json!({ "status": "done", "video": { "url": "https://vid/x.mp4" } }))
                .as_deref(),
            Some("https://vid/x.mp4")
        );
        assert!(imagine_is_video_path("/tmp/clip.mp4"));
        assert!(!imagine_is_video_path("/tmp/still.png"));
        let agent = compose_imagine_prompt(&ImagineSpec {
            prompt: "a mascot on the desk",
            kind: ImagineKind::Agent,
            quality: true,
            style: "Comic",
            aspect: "1:1",
            video_res: "480p",
            video_dur: "6s",
            video_audio: false,
        });
        assert!(agent.contains("1:1"));
        assert!(agent.to_ascii_lowercase().contains("comic"));
        assert!(agent.to_ascii_lowercase().contains("sprite") || agent.to_ascii_lowercase().contains("agent"));
        assert!(agent.to_ascii_lowercase().contains("still"));
    }

    fn gif(id: &str, created_ms: u64) -> WallGif {
        WallGif {
            id: id.into(),
            title: id.into(),
            prompt: format!("still of {id}, no people, no text"),
            created_ms,
            path_a: format!("{id}_a.jpg"),
            path_b: format!("{id}_b.jpg"),
            tall: false,
        }
    }

    #[test]
    fn wall_paints_every_few_hours() {
        assert_eq!(WALL_GIF_MAX, 20);
        assert_eq!(WALL_GIF_EVERY_MS, 3 * 60 * 60 * 1000);
        assert!(wall_due(0, 1, WALL_GIF_EVERY_MS));
        assert!(!wall_due(1_000, 1_000 + WALL_GIF_EVERY_MS - 1, WALL_GIF_EVERY_MS));
        assert!(wall_due(1_000, 1_000 + WALL_GIF_EVERY_MS, WALL_GIF_EVERY_MS));
        assert!(!wall_can_paint(false, true, false, false, false, 0, 10_000));
        assert!(!wall_can_paint(true, false, false, false, false, 0, 10_000));
        assert!(!wall_can_paint(true, true, true, false, false, 0, 10_000));
        assert!(!wall_can_paint(true, true, false, true, false, 0, 10_000));
        assert!(wall_can_paint(true, true, false, false, true, 0, 10_000));
        assert!(wall_can_paint(true, true, false, false, false, 0, 10_000));
        assert!(!wall_can_paint(
            true,
            true,
            false,
            false,
            true,
            1_000,
            1_000 + WALL_GIF_EVERY_MS
        ));
        assert!(!wall_can_paint(
            true,
            true,
            false,
            false,
            false,
            1_000,
            1_000 + WALL_GIF_EVERY_MS - 1
        ));
    }

    #[test]
    fn wall_evicts_oldest_first() {
        let gifs: Vec<WallGif> = (0..22).map(|i| gif(&format!("g{i}"), 100 + i)).collect();
        let (kept, evicted) = wall_evict(gifs, WALL_GIF_MAX);
        assert_eq!(kept.len(), 20);
        assert_eq!(evicted.len(), 2);
        assert_eq!(evicted[0].id, "g0");
        assert_eq!(evicted[1].id, "g1");
        assert_eq!(kept[0].id, "g2");
        assert_eq!(kept.last().unwrap().id, "g21");
        let five: Vec<WallGif> = (0..5).map(|i| gif(&format!("k{i}"), i)).collect();
        let (kept, evicted) = wall_evict(five, WALL_GIF_MAX);
        assert_eq!(kept.len(), 5);
        assert!(evicted.is_empty());
    }

    #[test]
    fn wall_curation_is_random_and_stable() {
        assert!(WALL_SEEDS.len() >= 16);
        for s in WALL_SEEDS {
            let blob = format!("{} {} {}", s.title, s.prompt, s.prompt_b).to_ascii_lowercase();
            assert!(blob.contains("still") || blob.contains("cabin") || blob.contains("desk"));
            assert!(blob.contains("no people"));
            assert!(!blob.contains("video"));
            assert!(!blob.contains("photo edit"));
        }
        let taken = ["Ember night"];
        let a = pick_fresh_seed(0, &taken);
        assert_ne!(a.title, "Ember night");
        let slots_a = curate_wall(9, 4, 42);
        let slots_b = curate_wall(9, 4, 42);
        assert_eq!(slots_a, slots_b);
        assert_eq!(slots_a.len(), 13);
        assert_eq!(
            slots_a.iter().filter(|s| matches!(s, WallSlot::Stock(_))).count(),
            9
        );
        assert_eq!(
            slots_a.iter().filter(|s| matches!(s, WallSlot::Gif(_))).count(),
            4
        );
        let slots_c = curate_wall(9, 4, 99);
        assert_ne!(slots_a, slots_c);
        let gifs = vec![gif("alpha", 1), gif("beta", 2)];
        assert_eq!(wall_curate_seed(&gifs), wall_curate_seed(&gifs));
        assert_ne!(wall_curate_seed(&gifs), wall_curate_seed(&[gif("alpha", 1)]));
    }

    #[test]
    fn idle_toolbox_sits_in_the_middle() {
        assert_eq!(
            imagine_toolbox_dock(false, false, false),
            ImagineToolboxDock::Middle
        );
        assert!(imagine_toolbox_shows_title(ImagineToolboxDock::Middle));
        let top = imagine_toolbox_top(100.0, 600.0, 180.0, ImagineToolboxDock::Middle);
        assert_eq!(top, 310.0);
        assert!(top > 200.0, "must not pin under the titlebar: {top}");
    }

    #[test]
    fn typing_keeps_the_toolbox_in_the_middle_send_docks_it() {
        assert_eq!(
            imagine_toolbox_dock(true, false, false),
            ImagineToolboxDock::Middle
        );
        assert_eq!(
            imagine_toolbox_dock(false, false, true),
            ImagineToolboxDock::Bottom
        );
        assert_eq!(
            imagine_toolbox_dock(false, true, false),
            ImagineToolboxDock::Bottom
        );
        let idle = imagine_toolbox_top(100.0, 600.0, 180.0, ImagineToolboxDock::Middle);
        let busy = imagine_toolbox_top(
            100.0,
            600.0,
            180.0,
            imagine_toolbox_dock(true, false, true),
        );
        assert!(busy > idle, "send must drop the chat box to the floor: idle={idle} busy={busy}");
        assert!(!imagine_toolbox_shows_title(ImagineToolboxDock::Bottom));
    }

    #[test]
    fn idle_photogif_wall_starts_under_the_chat_box() {
        let toolbox_top = imagine_toolbox_top(100.0, 600.0, 180.0, ImagineToolboxDock::Middle);
        let (top, h) = imagine_wall_bounds(
            100.0,
            600.0,
            toolbox_top,
            180.0,
            ImagineToolboxDock::Middle,
            0.0,
        );
        assert!(
            top >= toolbox_top + 180.0,
            "wall must sit under the idle chat box, not behind it: wall_top={top} box_bottom={}",
            toolbox_top + 180.0
        );
        assert!(h > 0.0);
        assert!(top + h <= 700.0 + 0.01);
        assert!(!imagine_wall_overlaps_toolbox(
            top,
            h,
            toolbox_top,
            180.0
        ));
    }

    #[test]
    fn photogif_wall_stays_under_the_stage_when_working() {
        let dock = imagine_toolbox_dock(false, false, true);
        assert_eq!(dock, ImagineToolboxDock::Bottom);
        let toolbox_top = imagine_toolbox_top(100.0, 600.0, 180.0, dock);
        let leftover = (toolbox_top - 100.0 - IMAGINE_WALL_GAP).max(0.0);
        let stage_h = imagine_stage_h(leftover, "2:3", 720.0);
        let (top, h) = imagine_wall_bounds(100.0, 600.0, toolbox_top, 180.0, dock, stage_h);
        assert!(
            top >= 100.0 + stage_h,
            "wall must sit under the generating box: wall_top={top} stage_bottom={}",
            100.0 + stage_h
        );
        assert!(
            top + h <= toolbox_top + 0.01,
            "wall must not run behind the docked chat box: wall_bottom={} box_top={toolbox_top}",
            top + h
        );
        assert!(!imagine_wall_overlaps_toolbox(top, h, toolbox_top, 180.0));
    }

    #[test]
    fn imagine_receipt_path_reads_the_saved_still() {
        assert_eq!(
            imagine_receipt_path("IMAGINE: /tmp/cabin.png").as_deref(),
            Some("/tmp/cabin.png")
        );
        assert_eq!(
            imagine_receipt_path("  IMAGINE: /work/night.jpg \n").as_deref(),
            Some("/work/night.jpg")
        );
        assert!(imagine_receipt_path("IMAGINE:").is_none());
        assert!(imagine_receipt_path("IMAGINE_PROMPT: a cabin at night").is_none());
        assert!(imagine_receipt_path("ok").is_none());
        assert_eq!(
            last_imagine_receipt(
                [
                    "hello",
                    "IMAGINE: /tmp/one.png",
                    "IMAGINE_PROMPT: skip me",
                    "IMAGINE: /tmp/two.png",
                ]
                .into_iter()
            )
            .as_deref(),
            Some("/tmp/two.png")
        );
        assert!(last_imagine_receipt(["hello"].into_iter()).is_none());
    }

    #[test]
    fn generating_stage_sits_above_the_docked_chat_box() {
        assert!(imagine_stage_visible(true, false));
        assert!(imagine_stage_visible(false, true));
        assert!(!imagine_stage_visible(false, false));
        let leftover = 400.0;
        let stage_h = imagine_stage_h(leftover, "2:3", 720.0);
        assert!(
            stage_h > leftover * 0.7 && stage_h < leftover,
            "stage must use most of the space above the chat box: {stage_h} of {leftover}"
        );
        let dock = imagine_toolbox_dock(true, true, true);
        assert_eq!(dock, ImagineToolboxDock::Bottom);
        let toolbox_top = imagine_toolbox_top(100.0, 600.0, 180.0, dock);
        let avail = (toolbox_top - 100.0 - IMAGINE_WALL_GAP).max(0.0);
        let stage_h = imagine_stage_h(avail, "2:3", 720.0);
        let (wall_top, _wall_h) =
            imagine_wall_bounds(100.0, 600.0, toolbox_top, 180.0, dock, stage_h);
        assert!(
            100.0 + stage_h <= toolbox_top,
            "generating box must sit above the docked chat box: stage_bottom={} box_top={toolbox_top}",
            100.0 + stage_h
        );
        assert!(
            wall_top >= 100.0 + stage_h,
            "wall starts under the generating box so you can scroll to it: wall_top={wall_top}"
        );
        assert!(wall_top <= toolbox_top);
        let gif = wall_gif_from_generation("/tmp/night.png", "a night cabin", 9, "2:3");
        assert_eq!(gif.path_a, "/tmp/night.png");
        assert!(gif.prompt.contains("night cabin"));
        assert!(gif.tall);
        let vid = wall_gif_from_generation("/tmp/clip.mp4", "waves", 10, "16:9");
        assert!(!vid.tall);
        assert!(imagine_is_video_path(&vid.path_a));
    }
}
