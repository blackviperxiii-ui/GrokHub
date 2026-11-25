//! `grokhub-hub node`: this device as a companion of the app on another
//! computer (docs/design/companion-nodes/README.md). It pairs with the
//! app's companion host, holds a long poll, and answers the app's MCP
//! requests with what this device can do and its person offers:
//!
//! - Android in Termux (with the Termux:API app): camera, location, text
//!   messages, speech out and in, panels.
//! - Linux, macOS and Windows: screen, camera (ffmpeg with a webcam, or
//!   imagesnap on macOS), speech out, panels.
//! - Anywhere: this device's own MCP servers and skills, and a wake event.
//!
//! Nothing is offered until its person runs `grokhub-hub node offer`. Every
//! request prints a line, so the person sees each use. Programs run with an
//! argument list, never through a shell.

use std::collections::HashMap;
use std::io::Read;
use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use base64::Engine as _;
use grokhub_core::companion_nodes::{self as nodes, NodeFile};
use grokhub_core::http;
use serde_json::{json, Value};

use crate::node_host::{POLL, PREFIX};

/// The MCP version this companion answers with when the app names none.
const MCP_VERSION: &str = "2025-06-18";
/// A photo or screenshot past this many pixels on its long side is scaled
/// down before it goes to the app, so it fits the host's request limit.
const MAX_SIDE: u32 = 1600;
const MAX_REPLY: u64 = 9 * 1024 * 1024;
const MAX_SAY: usize = 2000;
const MAX_SMS: usize = 1000;
const MAX_SKILL: u64 = 64 * 1024;
const CANVAS_FILE: &str = "companion-canvas.html";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Os {
    /// Android, in Termux.
    Termux,
    Linux,
    Macos,
    Windows,
    Other,
}

impl Os {
    fn platform(self) -> &'static str {
        match self {
            Os::Termux => "android",
            Os::Linux => "linux",
            Os::Macos => "macos",
            Os::Windows => "windows",
            Os::Other => "other",
        }
    }
}

/// Programs a capability may use, looked up on `PATH` once.
const BINS: &[&str] = &[
    "termux-camera-photo",
    "termux-location",
    "termux-sms-list",
    "termux-sms-send",
    "termux-tts-speak",
    "termux-speech-to-text",
    "termux-open",
    "ffmpeg",
    "imagesnap",
    "espeak-ng",
    "espeak",
    "spd-say",
    "say",
    "xdg-open",
    "open",
];

/// What this device has to lend.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Kit {
    pub os: Os,
    pub bins: Vec<String>,
    /// A webcam device (`/dev/video0`) on Linux.
    pub webcam: Option<String>,
}

impl Kit {
    pub fn probe() -> Kit {
        let termux = std::env::var_os("TERMUX_VERSION").is_some()
            || std::env::var("PREFIX").is_ok_and(|p| p.contains("com.termux"));
        let os = if termux {
            Os::Termux
        } else {
            match std::env::consts::OS {
                "linux" => Os::Linux,
                "macos" => Os::Macos,
                "windows" => Os::Windows,
                _ => Os::Other,
            }
        };
        let webcam = (os == Os::Linux && Path::new("/dev/video0").exists())
            .then(|| "/dev/video0".to_string());
        Kit {
            os,
            bins: BINS
                .iter()
                .filter(|b| grokhub_desktop::which(b))
                .map(|b| b.to_string())
                .collect(),
            webcam,
        }
    }

    fn has(&self, bin: &str) -> bool {
        self.bins.iter().any(|b| b == bin)
    }

    /// The program that speaks text out loud here.
    fn voice(&self) -> Option<&'static str> {
        match self.os {
            Os::Termux => self.has("termux-tts-speak").then_some("termux-tts-speak"),
            Os::Macos => self.has("say").then_some("say"),
            Os::Windows => Some("powershell"),
            Os::Linux | Os::Other => ["espeak-ng", "espeak", "spd-say"]
                .into_iter()
                .find(|b| self.has(b)),
        }
    }

    /// The program that opens a file in its app here.
    fn opener(&self) -> Option<&'static str> {
        match self.os {
            Os::Termux => self.has("termux-open").then_some("termux-open"),
            Os::Macos => self.has("open").then_some("open"),
            // Opens the page in the default browser, with no shell.
            Os::Windows => Some("rundll32"),
            Os::Linux | Os::Other => self.has("xdg-open").then_some("xdg-open"),
        }
    }

    fn can_listen(&self) -> bool {
        self.os == Os::Termux && self.has("termux-speech-to-text")
    }

    /// This device can lend `offer`.
    pub fn can(&self, offer: &str) -> bool {
        match offer {
            "camera" => match self.os {
                Os::Termux => self.has("termux-camera-photo"),
                Os::Linux => self.has("ffmpeg") && self.webcam.is_some(),
                Os::Macos => self.has("imagesnap"),
                _ => false,
            },
            "screen" => matches!(self.os, Os::Linux | Os::Macos | Os::Windows),
            "location" => self.os == Os::Termux && self.has("termux-location"),
            "sms" => {
                self.os == Os::Termux && self.has("termux-sms-list") && self.has("termux-sms-send")
            }
            "talk" => self.voice().is_some(),
            "canvas" => self.opener().is_some(),
            "wake" | "mcp" | "skills" => true,
            _ => false,
        }
    }
}

/// Runs this device's programs. Tests use a fake.
pub trait Runner: Send + Sync {
    /// Run `program` with `args` (no shell) and return its stdout, or why
    /// it failed.
    fn run(
        &self,
        program: &str,
        args: &[String],
        env: &[(&str, String)],
        timeout: Duration,
    ) -> Result<String, String>;
    /// Start `program` with `args` (no shell) and don't wait for it: an
    /// opener can stay up as long as the window it opened.
    fn launch(&self, program: &str, args: &[String]) -> Result<(), String>;
    /// One screenshot as `(bytes, mime)`.
    fn screen(&self) -> Result<(Vec<u8>, String), String>;
}

pub struct RealRunner;

impl Runner for RealRunner {
    fn run(
        &self,
        program: &str,
        args: &[String],
        env: &[(&str, String)],
        timeout: Duration,
    ) -> Result<String, String> {
        let mut cmd = std::process::Command::new(program);
        cmd.args(args);
        for (key, value) in env {
            cmd.env(key, value);
        }
        grokhub_core::proc_util::output_within(&mut cmd, timeout, 1024 * 1024)
            .ok_or_else(|| format!("{program} didn't finish."))
    }

    fn launch(&self, program: &str, args: &[String]) -> Result<(), String> {
        let mut child = std::process::Command::new(program)
            .args(args)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(|e| format!("Couldn't start {program}: {e}"))?;
        std::thread::spawn(move || child.wait());
        Ok(())
    }

    fn screen(&self) -> Result<(Vec<u8>, String), String> {
        grokhub_desktop::screen_grabber()()
    }
}

fn s(text: &str) -> String {
    text.to_string()
}

/// A word a program could read as one of its options gets a leading space.
fn not_an_option(text: &str) -> String {
    if text.starts_with('-') {
        format!(" {text}")
    } else {
        text.to_string()
    }
}

fn tool(name: &str, description: &str, schema: Value, read_only: bool) -> Value {
    json!({
        "name": name,
        "description": description,
        "inputSchema": schema,
        "annotations": { "readOnlyHint": read_only },
    })
}

fn no_args() -> Value {
    json!({ "type": "object", "properties": {} })
}

fn text_result(text: impl Into<String>) -> Value {
    json!({ "content": [{ "type": "text", "text": text.into() }] })
}

fn failed(text: impl Into<String>) -> Value {
    json!({ "content": [{ "type": "text", "text": text.into() }], "isError": true })
}

/// `image/jpeg` bytes as MCP image content, scaled down when large.
fn image_result(bytes: Vec<u8>, mime: &str) -> Value {
    let (bytes, mime) = shrink(bytes, mime);
    let data = base64::engine::general_purpose::STANDARD.encode(&bytes);
    json!({ "content": [{ "type": "image", "data": data, "mimeType": mime }] })
}

fn shrink(bytes: Vec<u8>, mime: &str) -> (Vec<u8>, String) {
    let Ok(img) = image::load_from_memory(&bytes) else {
        return (bytes, mime.to_string());
    };
    if img.width().max(img.height()) <= MAX_SIDE {
        return (bytes, mime.to_string());
    }
    let small = img.thumbnail(MAX_SIDE, MAX_SIDE).to_rgb8();
    let mut out = std::io::Cursor::new(Vec::new());
    let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 82);
    match small.write_with_encoder(encoder) {
        Ok(()) => (out.into_inner(), "image/jpeg".to_string()),
        Err(_) => (bytes, mime.to_string()),
    }
}

/// A phone number as `termux-sms-send` takes it.
fn phone_number(raw: &str) -> Option<String> {
    let digits: String = raw
        .chars()
        .filter(|c| !matches!(c, ' ' | '-' | '(' | ')' | '.'))
        .collect();
    let body = digits.strip_prefix('+').unwrap_or(&digits);
    (body.len() >= 3 && body.len() <= 16 && body.bytes().all(|b| b.is_ascii_digit()))
        .then_some(digits)
}

fn html_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

/// A panel as a page with no script: every word escaped, refreshed every
/// two seconds so the next `canvas_show` appears by itself.
pub fn canvas_html(spec: Option<&grokhub_core::panel_spec::PanelSpec>, from: &str) -> String {
    use grokhub_core::panel_spec::PanelElement as El;
    let mut body = String::new();
    match spec {
        None => body.push_str("<p class=\"quiet\">Nothing to show.</p>"),
        Some(spec) => {
            if let Some(title) = &spec.title {
                body.push_str(&format!("<h1>{}</h1>", html_escape(title)));
            }
            for el in &spec.elements {
                match el {
                    El::Heading(h) => {
                        let level = h.level.clamp(1, 4) + 1;
                        body.push_str(&format!("<h{level}>{}</h{level}>", html_escape(&h.text)));
                    }
                    El::Text(t) => body.push_str(&format!("<p>{}</p>", html_escape(&t.text))),
                    El::List(l) => {
                        let tag = if l.ordered { "ol" } else { "ul" };
                        body.push_str(&format!("<{tag}>"));
                        for item in &l.items {
                            body.push_str(&format!("<li>{}</li>", html_escape(item)));
                        }
                        body.push_str(&format!("</{tag}>"));
                    }
                    El::Table(t) => {
                        body.push_str("<table><tr>");
                        for h in &t.headers {
                            body.push_str(&format!("<th>{}</th>", html_escape(h)));
                        }
                        body.push_str("</tr>");
                        for row in &t.rows {
                            body.push_str("<tr>");
                            for cell in row {
                                body.push_str(&format!("<td>{}</td>", html_escape(cell)));
                            }
                            body.push_str("</tr>");
                        }
                        body.push_str("</table>");
                    }
                    El::KeyValue(kv) => {
                        body.push_str("<dl>");
                        for pair in &kv.pairs {
                            body.push_str(&format!(
                                "<dt>{}</dt><dd>{}</dd>",
                                html_escape(&pair.key),
                                html_escape(&pair.value)
                            ));
                        }
                        body.push_str("</dl>");
                    }
                    // Buttons and inputs answer in the app, not here.
                    El::Button(b) => {
                        body.push_str(&format!("<p class=\"chip\">{}</p>", html_escape(&b.label)))
                    }
                    El::Input(i) => body.push_str(&format!(
                        "<p class=\"chip\">{}: {}</p>",
                        html_escape(&i.label),
                        html_escape(i.value.as_deref().unwrap_or(""))
                    )),
                    El::Progress(p) => body.push_str(&format!(
                        "<p>{}</p><progress max=\"1\" value=\"{}\"></progress>",
                        html_escape(&p.label),
                        p.value.clamp(0.0, 1.0)
                    )),
                }
            }
        }
    }
    format!(
        "<!doctype html><html><head><meta charset=\"utf-8\">\
<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\
<meta http-equiv=\"refresh\" content=\"2\">\
<meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; style-src 'unsafe-inline'\">\
<title>{}</title><style>body{{font:16px system-ui,sans-serif;margin:16px;max-width:720px}}\
table{{border-collapse:collapse}}td,th{{border:1px solid #8884;padding:4px 8px}}\
.quiet,.from{{color:#888}}.chip{{display:inline-block;border:1px solid #8886;border-radius:12px;padding:2px 10px}}\
</style></head><body>{body}<p class=\"from\">From {}</p></body></html>",
        html_escape(spec.and_then(|s| s.title.as_deref()).unwrap_or("Panel")),
        html_escape(from)
    )
}

/// One companion session: the saved pairing, this device's kit, and the
/// names of the MCP tools it lends.
pub struct Node<'a> {
    pub file: NodeFile,
    pub kit: Kit,
    pub runner: &'a dyn Runner,
    /// Where the canvas page and scratch files go.
    pub dir: PathBuf,
    /// The person's home, where skills are found.
    pub home: Option<PathBuf>,
    /// Lent name (`server-tool`) → this device's `server__tool`.
    lent: Mutex<HashMap<String, String>>,
    canvas_opened: AtomicBool,
}

impl<'a> Node<'a> {
    pub fn new(file: NodeFile, kit: Kit, runner: &'a dyn Runner, dir: PathBuf) -> Node<'a> {
        Node {
            file,
            kit,
            runner,
            dir,
            home: home_dir(),
            lent: Mutex::new(HashMap::new()),
            canvas_opened: AtomicBool::new(false),
        }
    }

    /// Offered by its person and possible on this device.
    fn lends(&self, offer: &str) -> bool {
        self.file.offers.iter().any(|o| o == offer) && self.kit.can(offer)
    }

    /// The offers `/hello` sends.
    pub fn offers(&self) -> Vec<String> {
        nodes::known_offers(&self.file.offers)
            .into_iter()
            .filter(|o| self.kit.can(o))
            .collect()
    }

    /// The answer to one relayed JSON-RPC message; `None` for a
    /// notification.
    pub fn handle(&self, rpc: &Value) -> Option<Value> {
        let id = rpc.get("id").cloned().filter(|v| !v.is_null())?;
        let method = rpc.get("method").and_then(Value::as_str).unwrap_or("");
        let params = rpc.get("params").cloned().unwrap_or(Value::Null);
        let result = match method {
            "initialize" => json!({
                "protocolVersion": params.get("protocolVersion").and_then(Value::as_str).unwrap_or(MCP_VERSION),
                "capabilities": { "tools": {} },
                "serverInfo": { "name": self.file.name, "version": env!("CARGO_PKG_VERSION") },
            }),
            "ping" => json!({}),
            "tools/list" => json!({ "tools": self.tools() }),
            "tools/call" => {
                let name = params.get("name").and_then(Value::as_str).unwrap_or("");
                let args = params
                    .get("arguments")
                    .cloned()
                    .unwrap_or_else(|| json!({}));
                eprintln!("companion: {} asked for {name}", self.file.host_name);
                self.call(name, &args)
            }
            _ => {
                return Some(json!({
                    "jsonrpc": "2.0", "id": id,
                    "error": { "code": -32601, "message": format!("{method} isn't supported.") },
                }));
            }
        };
        Some(json!({ "jsonrpc": "2.0", "id": id, "result": result }))
    }

    pub fn tools(&self) -> Vec<Value> {
        let mut out = vec![];
        if self.lends("camera") {
            out.push(tool(
                "camera_snap",
                &format!("Take a photo with {}'s camera.", self.file.name),
                json!({ "type": "object", "properties": { "facing": { "type": "string", "enum": ["back", "front"] } } }),
                true,
            ));
        }
        if self.lends("screen") {
            out.push(tool(
                "screen_snap",
                &format!("Take a screenshot of {}.", self.file.name),
                no_args(),
                true,
            ));
        }
        if self.lends("location") {
            out.push(tool(
                "location_get",
                &format!("Where {} is now: latitude, longitude and accuracy in meters.", self.file.name),
                json!({ "type": "object", "properties": { "precise": { "type": "boolean", "description": "Use GPS: slower, more accurate." } } }),
                true,
            ));
        }
        if self.lends("sms") {
            out.push(tool(
                "sms_list",
                &format!("The newest text messages {} received.", self.file.name),
                json!({ "type": "object", "properties": { "limit": { "type": "integer", "minimum": 1, "maximum": 50 } } }),
                true,
            ));
            out.push(tool(
                "sms_send",
                &format!("Send a text message from {}.", self.file.name),
                json!({ "type": "object", "properties": {
                    "to": { "type": "string", "description": "Phone number, like +15555550100." },
                    "body": { "type": "string", "maxLength": MAX_SMS }
                }, "required": ["to", "body"] }),
                false,
            ));
        }
        if self.lends("talk") {
            out.push(tool(
                "talk_say",
                &format!("Say something out loud on {}.", self.file.name),
                json!({ "type": "object", "properties": { "text": { "type": "string", "maxLength": MAX_SAY } }, "required": ["text"] }),
                false,
            ));
            if self.kit.can_listen() {
                out.push(tool(
                    "talk_listen",
                    &format!(
                        "Listen on {}'s microphone and return what was said.",
                        self.file.name
                    ),
                    no_args(),
                    true,
                ));
            }
        }
        if self.lends("canvas") {
            out.push(tool(
                "canvas_show",
                &format!("Show a panel on {}'s screen: headings, text, lists, tables, key-value rows, progress.", self.file.name),
                grokhub_core::panel_spec::panel_spec_json_schema(),
                false,
            ));
            out.push(tool(
                "canvas_clear",
                &format!("Clear the panel on {}.", self.file.name),
                no_args(),
                false,
            ));
        }
        if self.lends("skills") {
            out.push(tool(
                "skills_list",
                &format!("The skills on {}.", self.file.name),
                no_args(),
                true,
            ));
            out.push(tool(
                "skill_read",
                &format!("Read one of {}'s skills.", self.file.name),
                json!({ "type": "object", "properties": { "name": { "type": "string" } }, "required": ["name"] }),
                true,
            ));
        }
        if self.lends("mcp") {
            let mut lent = self.lent.lock().unwrap_or_else(|e| e.into_inner());
            lent.clear();
            for t in grokhub_agent::mcp::lent_tools() {
                let name = t.qualified.replacen("__", "-", 1);
                out.push(json!({
                    "name": name,
                    "description": t.description,
                    "inputSchema": t.input_schema,
                    "annotations": { "readOnlyHint": t.read_only },
                }));
                lent.insert(name, t.qualified);
            }
        }
        out
    }

    fn call(&self, name: &str, args: &Value) -> Value {
        let offer = nodes::offer_for_tool(name);
        if !self.lends(offer) {
            return failed(format!("{} doesn't lend `{offer}`.", self.file.name));
        }
        let text = |key: &str| {
            args.get(key)
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim()
                .to_string()
        };
        let run = |program: &str, argv: Vec<String>, secs: u64| {
            self.runner
                .run(program, &argv, &[], Duration::from_secs(secs))
        };
        match name {
            "camera_snap" => self.snap(&text("facing")),
            "screen_snap" => match self.runner.screen() {
                Ok((bytes, mime)) => image_result(bytes, &mime),
                Err(e) => failed(e),
            },
            "location_get" => {
                let provider = if args.get("precise").and_then(Value::as_bool) == Some(true) {
                    "gps"
                } else {
                    "network"
                };
                match run("termux-location", vec![s("-p"), s(provider), s("-r"), s("once")], 60) {
                    Ok(out) => match serde_json::from_str::<Value>(&out) {
                        Ok(v) if v.get("latitude").is_some() => text_result(
                            json!({ "lat": v["latitude"], "lon": v["longitude"], "accuracyM": v["accuracy"] }).to_string(),
                        ),
                        _ => failed("The phone didn't report a location. Is location on?"),
                    },
                    Err(e) => failed(e),
                }
            }
            "sms_list" => {
                let limit = args
                    .get("limit")
                    .and_then(Value::as_u64)
                    .unwrap_or(10)
                    .clamp(1, 50);
                match run(
                    "termux-sms-list",
                    vec![s("-l"), limit.to_string(), s("-t"), s("inbox")],
                    30,
                ) {
                    Ok(out) => {
                        let list: Vec<Value> = serde_json::from_str::<Vec<Value>>(&out)
                            .unwrap_or_default()
                            .into_iter()
                            .map(|m| json!({ "from": m["number"], "at": m["received"], "body": m["body"] }))
                            .collect();
                        text_result(Value::Array(list).to_string())
                    }
                    Err(e) => failed(e),
                }
            }
            "sms_send" => {
                let Some(to) = phone_number(&text("to")) else {
                    return failed("`to` must be a phone number, like +15555550100.");
                };
                let body = text("body");
                if body.is_empty() || body.chars().count() > MAX_SMS {
                    return failed(format!("`body` must be 1 to {MAX_SMS} characters."));
                }
                match run(
                    "termux-sms-send",
                    vec![s("-n"), to.clone(), not_an_option(&body)],
                    30,
                ) {
                    Ok(_) => text_result(format!("Sent to {to}.")),
                    Err(e) => failed(e),
                }
            }
            "talk_say" => self.say(&text("text")),
            "talk_listen" => {
                if !self.kit.can_listen() {
                    return failed(format!("{} can't listen.", self.file.name));
                }
                match run("termux-speech-to-text", vec![], 60) {
                    Ok(heard) if !heard.trim().is_empty() => text_result(heard.trim()),
                    Ok(_) => failed("Nothing was heard."),
                    Err(e) => failed(e),
                }
            }
            "canvas_show" => {
                match grokhub_core::panel_spec::panel_spec_from_args(&args.to_string()) {
                    Ok(spec) => self.show(Some(&spec)),
                    Err(e) => failed(e),
                }
            }
            "canvas_clear" => self.show(None),
            "skills_list" => {
                let list: Vec<Value> = self
                    .skills()
                    .iter()
                    .map(|s| json!({ "name": s.name, "description": s.description }))
                    .collect();
                text_result(Value::Array(list).to_string())
            }
            "skill_read" => {
                let want = text("name");
                let Some(skill) = self.skills().into_iter().find(|s| s.name == want) else {
                    return failed(format!("{} has no skill named `{want}`.", self.file.name));
                };
                let mut body = String::new();
                match std::fs::File::open(&skill.path) {
                    Ok(file) => {
                        let _ = file.take(MAX_SKILL).read_to_string(&mut body);
                        text_result(body)
                    }
                    Err(e) => failed(e.to_string()),
                }
            }
            lent => {
                let qualified = self
                    .lent
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .get(lent)
                    .cloned();
                let Some(out) = qualified.and_then(|q| grokhub_agent::mcp::call_lent(&q, args))
                else {
                    return failed(format!("{} has no tool named `{lent}`.", self.file.name));
                };
                let mut content = vec![json!({ "type": "text", "text": out.text })];
                if let Some((mime, data)) = out
                    .image_data_url
                    .as_deref()
                    .and_then(|u| u.strip_prefix("data:"))
                    .and_then(|u| u.split_once(";base64,"))
                {
                    content.push(json!({ "type": "image", "data": data, "mimeType": mime }));
                }
                json!({ "content": content, "isError": out.failed })
            }
        }
    }

    fn snap(&self, facing: &str) -> Value {
        let dest = self.dir.join("companion-snap.jpg");
        let path = dest.to_string_lossy().into_owned();
        let _ = std::fs::remove_file(&dest);
        let ran = match self.kit.os {
            Os::Termux => {
                let camera = if facing == "front" { "1" } else { "0" };
                self.runner.run(
                    "termux-camera-photo",
                    &[s("-c"), s(camera), path.clone()],
                    &[],
                    Duration::from_secs(30),
                )
            }
            Os::Linux => {
                let device = self.kit.webcam.clone().unwrap_or_default();
                let argv = grokhub_core::capture::ffmpeg_webcam_args(&device, &path);
                self.runner
                    .run("ffmpeg", &argv, &[], Duration::from_secs(30))
            }
            Os::Macos => self.runner.run(
                "imagesnap",
                &[s("-q"), s("-w"), s("1"), path.clone()],
                &[],
                Duration::from_secs(30),
            ),
            _ => Err("This device has no camera to lend.".into()),
        };
        if let Err(e) = ran {
            return failed(e);
        }
        let bytes = std::fs::read(&dest);
        let _ = std::fs::remove_file(&dest);
        match bytes {
            Ok(bytes) if !bytes.is_empty() => image_result(bytes, "image/jpeg"),
            _ => failed("The camera didn't save a photo. Is the camera allowed for Termux:API?"),
        }
    }

    fn say(&self, text: &str) -> Value {
        if text.is_empty() || text.chars().count() > MAX_SAY {
            return failed(format!("`text` must be 1 to {MAX_SAY} characters."));
        }
        let Some(voice) = self.kit.voice() else {
            return failed(format!("{} has no voice.", self.file.name));
        };
        let timeout = Duration::from_secs(120);
        let ran = if voice == "powershell" {
            // The words go in an environment variable, never into the script.
            let script = "Add-Type -AssemblyName System.Speech; (New-Object System.Speech.Synthesis.SpeechSynthesizer).Speak($env:COMPANION_SAY)";
            self.runner.run(
                voice,
                &[
                    s("-NoProfile"),
                    s("-NonInteractive"),
                    s("-Command"),
                    s(script),
                ],
                &[("COMPANION_SAY", text.to_string())],
                timeout,
            )
        } else {
            self.runner.run(voice, &[not_an_option(text)], &[], timeout)
        };
        match ran {
            Ok(_) => text_result(format!("Said it on {}.", self.file.name)),
            Err(e) => failed(e),
        }
    }

    fn show(&self, spec: Option<&grokhub_core::panel_spec::PanelSpec>) -> Value {
        let page = self.dir.join(CANVAS_FILE);
        let html = canvas_html(spec, &self.file.host_name);
        if let Err(e) =
            std::fs::create_dir_all(&self.dir).and_then(|()| std::fs::write(&page, html))
        {
            return failed(e.to_string());
        }
        if spec.is_some() && !self.canvas_opened.swap(true, Ordering::SeqCst) {
            let path = page.to_string_lossy().into_owned();
            let opened = match self.kit.opener() {
                Some("rundll32") => self
                    .runner
                    .launch("rundll32", &[s("url.dll,FileProtocolHandler"), path]),
                Some(opener) => self.runner.launch(opener, &[path]),
                None => Err("This device can't open a panel.".into()),
            };
            if let Err(e) = opened {
                self.canvas_opened.store(false, Ordering::SeqCst);
                return failed(e);
            }
        }
        text_result(if spec.is_some() {
            format!("Shown on {}.", self.file.name)
        } else {
            format!("Cleared the panel on {}.", self.file.name)
        })
    }

    fn skills(&self) -> Vec<grokhub_agent::Skill> {
        let home = self.home.clone().unwrap_or_else(|| self.dir.clone());
        grokhub_agent::discover_skills(&home, Some(&home), &[])
            .into_iter()
            .filter(|s| s.source != grokhub_agent::SkillSource::Builtin)
            .collect()
    }
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .filter(|h| !h.is_empty())
        .map(PathBuf::from)
}

/// `ip:port` for what the person typed: `192.168.1.20`, `192.168.1.20:18767`
/// or with `http://`. A number address on a private network only.
pub fn host_address(raw: &str) -> Result<String, String> {
    let text = raw
        .trim()
        .trim_start_matches("http://")
        .trim_end_matches('/');
    let addr: SocketAddr = match text.parse::<SocketAddr>() {
        Ok(addr) => addr,
        Err(_) => {
            let ip: IpAddr = text
                .trim_start_matches('[')
                .trim_end_matches(']')
                .parse()
                .map_err(|_| {
                    "Type the number address the app shows, like 192.168.1.20.".to_string()
                })?;
            SocketAddr::new(ip, nodes::PORT)
        }
    };
    if !nodes::private_address(addr.ip()) {
        return Err("The app must be on this network or this computer.".into());
    }
    Ok(addr.to_string())
}

/// The id this device pairs as: its name in lowercase, dashes for the rest.
pub fn id_from(name: &str) -> String {
    let mut id = String::new();
    for c in name.chars().flat_map(char::to_lowercase) {
        if c.is_ascii_alphanumeric() {
            id.push(c);
        } else if !id.ends_with('-') && !id.is_empty() {
            id.push('-');
        }
        if id.len() >= 40 {
            break;
        }
    }
    let id = id.trim_end_matches('-').to_string();
    if id.is_empty() {
        "companion".into()
    } else {
        id
    }
}

fn agent() -> http::Agent {
    // No redirects: one could carry the token off the network.
    http::agent(http::Opts {
        connect: Duration::from_secs(5),
        read: Some(POLL + Duration::from_secs(15)),
        redirects: 0,
        ..Default::default()
    })
}

/// One call to the host: status and JSON body. The address was checked
/// private when the pairing was saved and is checked again here.
fn host_call(
    host: &str,
    method: &str,
    path: &str,
    token: &str,
    body: Option<Value>,
) -> Result<(u16, Value), String> {
    host_address(host)?;
    let url = format!("http://{host}{PREFIX}{path}");
    let agent = agent();
    let req = match method {
        "GET" => agent.get(&url),
        _ => agent.post(&url),
    }
    .any_status();
    let req = if token.is_empty() {
        req
    } else {
        req.header("Authorization", &format!("Bearer {token}"))
    };
    let resp = match body {
        Some(body) => req.send_json(body),
        None => req.call(),
    }
    .map_err(|_| format!("Couldn't reach the app at {host}."))?;
    let status = resp.status();
    let value = serde_json::from_reader(resp.into_reader().take(MAX_REPLY)).unwrap_or(Value::Null);
    Ok((status, value))
}

fn refusal(status: u16, body: &Value) -> String {
    body.get("error")
        .and_then(Value::as_str)
        .map_or_else(|| format!("The app answered {status}."), str::to_string)
}

/// Pair with the app at `address` using the code it shows.
pub fn pair(address: &str, code: &str, name: &str, id: &str, os: Os) -> Result<NodeFile, String> {
    let host = host_address(address)?;
    let body = json!({ "code": code.trim(), "id": id, "name": name, "platform": os.platform() });
    let (status, v) = host_call(&host, "POST", "/pair", "", Some(body))?;
    if status != 200 {
        return Err(refusal(status, &v));
    }
    let token = v
        .get("token")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    if token.is_empty() {
        return Err("The app didn't send a token.".into());
    }
    Ok(NodeFile {
        host,
        host_name: v
            .pointer("/host/name")
            .and_then(Value::as_str)
            .unwrap_or("the app")
            .to_string(),
        id: id.to_string(),
        name: name.to_string(),
        token,
        offers: vec![],
    })
}

/// What one poll did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Poll {
    /// Still waiting for the person's yes in the app.
    Pending,
    /// Nothing came; poll again.
    Idle,
    /// Answered a request (or took a notification).
    Served,
}

impl Node<'_> {
    /// Tell the host what this device lends now. `Ok(approved)`.
    pub fn hello(&self) -> Result<bool, String> {
        let (status, v) = host_call(
            &self.file.host,
            "POST",
            "/hello",
            &self.file.token,
            Some(json!({ "offers": self.offers() })),
        )?;
        if status != 200 {
            return Err(refusal(status, &v));
        }
        Ok(v.get("approved").and_then(Value::as_bool).unwrap_or(false))
    }

    /// One long poll and, when a request came, its answer.
    pub fn poll_once(&self) -> Result<Poll, String> {
        let (status, v) = host_call(&self.file.host, "GET", "/next", &self.file.token, None)?;
        if status != 200 {
            return Err(refusal(status, &v));
        }
        if v.get("pending").and_then(Value::as_bool) == Some(true) {
            return Ok(Poll::Pending);
        }
        let Some(rpc) = v.get("rpc") else {
            return Ok(Poll::Idle);
        };
        if let Some(answer) = self.handle(rpc) {
            let (status, v) = host_call(
                &self.file.host,
                "POST",
                "/reply",
                &self.file.token,
                Some(json!({ "rpc": answer })),
            )?;
            if status != 200 {
                return Err(refusal(status, &v));
            }
        }
        Ok(Poll::Served)
    }

    /// Send a wake or a heard line to the app.
    pub fn event(&self, kind: &str, text: &str) -> Result<(), String> {
        let (status, v) = host_call(
            &self.file.host,
            "POST",
            "/event",
            &self.file.token,
            Some(json!({ "kind": kind, "text": text })),
        )?;
        if status != 200 {
            return Err(refusal(status, &v));
        }
        Ok(())
    }
}

const HELP: &str = "grokhub-hub node — lend this device to the app on another computer\n\n  \
grokhub-hub node pair <address> <code> [--name NAME]   pair with the code the app shows under Devices → Companions\n  \
grokhub-hub node offer <list|none>   what to lend: camera,screen,location,sms,talk,wake,canvas,mcp,skills\n  \
grokhub-hub node run                 stay connected and answer the app\n  \
grokhub-hub node wake [--listen]     wake the app; with --listen, send what you say (Termux)\n  \
grokhub-hub node status              what is paired and lent\n  \
grokhub-hub node forget              forget the pairing on this device\n";

fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .cloned()
}

fn default_name() -> String {
    std::env::var("COMPANION_NAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .or_else(|_| std::env::var("COMPUTERNAME"))
        .ok()
        .or_else(|| std::fs::read_to_string("/etc/hostname").ok())
        .map(|n| n.trim().to_string())
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| "Companion".into())
}

/// What `offer` and `status` say: the offers this device can lend now. An
/// offer it can't lend yet stays saved, so installing the program it needs
/// is enough.
fn lending(offers: &[String], kit: &Kit) -> String {
    let now: Vec<&str> = offers
        .iter()
        .map(String::as_str)
        .filter(|o| kit.can(o))
        .collect();
    if now.is_empty() {
        "Lending: nothing.".into()
    } else {
        format!("Lending: {}.", now.join(", "))
    }
}

/// `grokhub-hub node …`; `args` are the words after `node`. Exit code.
pub fn run_cli(args: &[String]) -> i32 {
    let dir = grokhub_core::paths::config_dir();
    let path = dir.join(nodes::NODE_FILE);
    let kit = Kit::probe();
    let say = |text: &str| println!("{text}");
    let fail = |text: &str| {
        eprintln!("{text}");
        1
    };
    let words: Vec<&str> = args.iter().map(String::as_str).collect();
    match words.as_slice() {
        ["pair", address, code, ..] => {
            let name = flag(args, "--name").unwrap_or_else(default_name);
            let id = id_from(&name);
            match pair(address, code, &name, &id, kit.os).and_then(|f| f.save(&path).map(|()| f)) {
                Ok(f) => {
                    say(&format!("Paired with {} as {}.", f.host_name, f.name));
                    say("Say yes on that computer under Devices → Companions, then choose what to lend:");
                    say("  grokhub-hub node offer camera,location   and then   grokhub-hub node run");
                    0
                }
                Err(e) => fail(&e),
            }
        }
        ["offer", list] => {
            let Some(mut f) = NodeFile::load(&path) else {
                return fail("Pair first: grokhub-hub node pair <address> <code>");
            };
            let wanted: Vec<String> = if *list == "none" {
                vec![]
            } else {
                list.split(',').map(|w| w.trim().to_string()).collect()
            };
            let known = nodes::known_offers(&wanted);
            for w in &wanted {
                if !known.contains(w) {
                    eprintln!("`{w}` isn't something a companion can lend.");
                } else if !kit.can(w) {
                    eprintln!(
                        "This device can't lend `{w}` yet (a program it needs is missing). It stays offered."
                    );
                }
            }
            f.offers = known;
            if let Err(e) = f.save(&path) {
                return fail(&e);
            }
            say(&lending(&f.offers, &kit));
            say("A running `grokhub-hub node run` picks this up when it restarts.");
            0
        }
        ["run"] => {
            let Some(f) = NodeFile::load(&path) else {
                return fail("Pair first: grokhub-hub node pair <address> <code>");
            };
            if f.offers.iter().any(|o| o == "mcp") {
                grokhub_agent::mcp::connect_in_background(
                    &home_dir().unwrap_or_else(|| dir.clone()),
                );
            }
            let node = Node::new(f, kit, &RealRunner, dir);
            say(&format!(
                "Lending {} to {}. Ctrl+C stops.",
                node.offers().join(", "),
                node.file.host_name
            ));
            serve(&node)
        }
        ["wake", rest @ ..] => {
            let Some(f) = NodeFile::load(&path) else {
                return fail("Pair first: grokhub-hub node pair <address> <code>");
            };
            let node = Node::new(f, kit, &RealRunner, dir);
            let listen = rest.contains(&"--listen");
            let sent = if listen {
                if !node.kit.can_listen() {
                    return fail("This device can't listen (Termux with Termux:API only).");
                }
                RealRunner
                    .run("termux-speech-to-text", &[], &[], Duration::from_secs(60))
                    .and_then(|heard| node.event("heard", heard.trim()))
            } else {
                node.event("wake", "")
            };
            match sent {
                Ok(()) => 0,
                Err(e) => fail(&e),
            }
        }
        ["status"] => {
            match NodeFile::load(&path) {
                Some(f) => {
                    say(&format!(
                        "Paired with {} at {} as {}.",
                        f.host_name, f.host, f.name
                    ));
                    say(&lending(&f.offers, &kit));
                }
                None => say("Not paired."),
            }
            let can: Vec<&str> = nodes::OFFERS
                .iter()
                .map(|(o, _)| *o)
                .filter(|o| kit.can(o))
                .collect();
            say(&format!("This device can lend: {}.", can.join(", ")));
            0
        }
        ["forget"] => {
            let _ = std::fs::remove_file(&path);
            say("Forgot the pairing. Remove it in the app too, under Devices → Companions.");
            0
        }
        _ => {
            print!("{HELP}");
            if words.is_empty() || words == ["--help"] || words == ["help"] {
                0
            } else {
                2
            }
        }
    }
}

/// Poll and answer until the app removes this companion.
fn serve(node: &Node) -> i32 {
    let mut backoff = 2;
    let mut said_pending = false;
    let mut hello_due = true;
    loop {
        if hello_due {
            match node.hello() {
                Ok(_) => hello_due = false,
                Err(e) => {
                    eprintln!("{e}");
                    std::thread::sleep(Duration::from_secs(backoff));
                    backoff = (backoff * 2).min(30);
                    continue;
                }
            }
        }
        match node.poll_once() {
            Ok(Poll::Pending) => {
                if !said_pending {
                    println!(
                        "Waiting for your yes on {} (Devices → Companions).",
                        node.file.host_name
                    );
                    said_pending = true;
                }
                std::thread::sleep(Duration::from_secs(5));
                hello_due = true;
            }
            Ok(_) => backoff = 2,
            Err(e) if e.starts_with("Pair this companion first") => {
                eprintln!(
                    "{} removed this companion. Pair again to come back.",
                    node.file.host_name
                );
                return 1;
            }
            Err(e) => {
                eprintln!("{e}");
                std::thread::sleep(Duration::from_secs(backoff));
                backoff = (backoff * 2).min(30);
                hello_due = true;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node_host::NodeHost;
    use std::net::Ipv4Addr;

    /// One program run: program, arguments, environment.
    type Ran = (String, Vec<String>, Vec<(String, String)>);

    /// Records each program run and answers from a table.
    #[derive(Default)]
    struct Fake {
        ran: Mutex<Vec<Ran>>,
        answers: HashMap<&'static str, &'static str>,
    }

    impl Runner for Fake {
        fn run(
            &self,
            program: &str,
            args: &[String],
            env: &[(&str, String)],
            _: Duration,
        ) -> Result<String, String> {
            self.ran.lock().expect("ran").push((
                program.to_string(),
                args.to_vec(),
                env.iter()
                    .map(|(k, v)| (k.to_string(), v.clone()))
                    .collect(),
            ));
            if program == "termux-camera-photo" {
                let mut img = image::RgbImage::new(2400, 1200);
                img.put_pixel(0, 0, image::Rgb([255, 0, 0]));
                img.save(args.last().expect("dest"))
                    .map_err(|e| e.to_string())?;
            }
            self.answers
                .get(program)
                .map(|a| a.to_string())
                .ok_or_else(|| format!("{program} isn't here"))
        }
        fn launch(&self, program: &str, args: &[String]) -> Result<(), String> {
            self.run(program, args, &[], Duration::ZERO).map(|_| ())
        }
        fn screen(&self) -> Result<(Vec<u8>, String), String> {
            Ok((vec![1, 2, 3], "image/png".into()))
        }
    }

    fn termux() -> Kit {
        Kit {
            os: Os::Termux,
            bins: [
                "termux-camera-photo",
                "termux-location",
                "termux-sms-list",
                "termux-sms-send",
                "termux-tts-speak",
                "termux-speech-to-text",
                "termux-open",
            ]
            .map(String::from)
            .to_vec(),
            webcam: None,
        }
    }

    fn file(offers: &[&str]) -> NodeFile {
        NodeFile {
            host: "127.0.0.1:1".into(),
            host_name: "Desk".into(),
            id: "pixel".into(),
            name: "Pixel".into(),
            token: "t".into(),
            offers: offers.iter().map(|o| o.to_string()).collect(),
        }
    }

    fn dir() -> PathBuf {
        std::env::temp_dir().join(grokhub_core::uid("node-client"))
    }

    fn call(node: &Node, name: &str, args: Value) -> Value {
        node.handle(&json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":name,"arguments":args}}))
            .expect("answer")["result"]
            .clone()
    }

    #[test]
    fn a_device_lends_only_what_it_has_and_its_person_offered() {
        let desktop = Kit {
            os: Os::Linux,
            bins: vec!["espeak-ng".into(), "xdg-open".into()],
            webcam: None,
        };
        assert!(desktop.can("screen") && desktop.can("talk") && desktop.can("canvas"));
        assert!(!desktop.can("camera"), "no webcam");
        assert!(!desktop.can("sms") && !desktop.can("location"));
        assert!(!termux().can("screen"));
        let fake = Fake::default();
        let node = Node::new(file(&["camera", "screen", "sms"]), termux(), &fake, dir());
        assert_eq!(
            node.offers(),
            vec!["camera", "sms"],
            "Termux has no screen to lend"
        );
        let names: Vec<String> = node
            .tools()
            .iter()
            .map(|t| t["name"].as_str().expect("name").to_string())
            .collect();
        assert_eq!(names, ["camera_snap", "sms_list", "sms_send"]);
        let send = node
            .tools()
            .into_iter()
            .find(|t| t["name"] == "sms_send")
            .expect("send");
        assert_eq!(
            send["annotations"]["readOnlyHint"], false,
            "sending asks first"
        );
        let snap = call(&node, "location_get", json!({}));
        assert_eq!(snap["isError"], true);
        assert_eq!(snap["content"][0]["text"], "Pixel doesn't lend `location`.");
        assert!(fake.ran.lock().expect("ran").is_empty(), "nothing ran");
    }

    #[test]
    fn location_and_messages_come_from_termux_api() {
        let mut fake = Fake::default();
        fake.answers.insert(
            "termux-location",
            r#"{"latitude":45.5,"longitude":-122.6,"accuracy":12.0,"provider":"network"}"#,
        );
        fake.answers.insert(
            "termux-sms-list",
            r#"[{"number":"+15555550100","received":"2026-11-24 18:00","body":"hi","read":true}]"#,
        );
        fake.answers.insert("termux-sms-send", "");
        let node = Node::new(file(&["location", "sms"]), termux(), &fake, dir());
        let at = call(&node, "location_get", json!({ "precise": true }));
        let at: Value =
            serde_json::from_str(at["content"][0]["text"].as_str().expect("text")).expect("json");
        assert_eq!(at, json!({ "lat": 45.5, "lon": -122.6, "accuracyM": 12.0 }));
        let list = call(&node, "sms_list", json!({ "limit": 500 }));
        assert_eq!(
            list["content"][0]["text"],
            r#"[{"at":"2026-11-24 18:00","body":"hi","from":"+15555550100"}]"#
        );
        let bad = call(
            &node,
            "sms_send",
            json!({ "to": "+1; rm -rf ~", "body": "x" }),
        );
        assert_eq!(bad["isError"], true);
        let sent = call(
            &node,
            "sms_send",
            json!({ "to": "+1 (555) 555-0100", "body": "-n 911 hi" }),
        );
        assert_eq!(sent["content"][0]["text"], "Sent to +15555550100.");
        let ran = fake.ran.lock().expect("ran").clone();
        assert_eq!(ran[0].1, ["-p", "gps", "-r", "once"]);
        assert_eq!(ran[1].1, ["-l", "50", "-t", "inbox"], "limit capped at 50");
        assert_eq!(
            ran[2].1,
            ["-n", "+15555550100", " -n 911 hi"],
            "a body can't pass as an option"
        );
    }

    #[test]
    fn a_big_photo_comes_back_scaled_down() {
        let mut fake = Fake::default();
        fake.answers.insert("termux-camera-photo", "");
        let d = dir();
        std::fs::create_dir_all(&d).expect("dir");
        let node = Node::new(file(&["camera"]), termux(), &fake, d.clone());
        let out = call(&node, "camera_snap", json!({ "facing": "front" }));
        assert_eq!(out["content"][0]["type"], "image");
        assert_eq!(out["content"][0]["mimeType"], "image/jpeg");
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(out["content"][0]["data"].as_str().expect("data"))
            .expect("b64");
        let img = image::load_from_memory(&bytes).expect("jpeg");
        assert_eq!((img.width(), img.height()), (1600, 800));
        assert_eq!(fake.ran.lock().expect("ran")[0].1[..2], ["-c", "1"]);
        assert!(
            !d.join("companion-snap.jpg").exists(),
            "the photo isn't left behind"
        );
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn windows_speech_takes_the_words_from_the_environment() {
        let mut fake = Fake::default();
        fake.answers.insert("powershell", "");
        let kit = Kit {
            os: Os::Windows,
            bins: vec![],
            webcam: None,
        };
        let node = Node::new(file(&["talk"]), kit, &fake, dir());
        let said = call(
            &node,
            "talk_say",
            json!({ "text": "'); Remove-Item C:\\ -Recurse; ('" }),
        );
        assert_eq!(said["content"][0]["text"], "Said it on Pixel.");
        let ran = fake.ran.lock().expect("ran").clone();
        assert!(
            !ran[0].1.iter().any(|a| a.contains("Remove-Item")),
            "{:?}",
            ran[0].1
        );
        assert_eq!(
            ran[0].2,
            [(
                "COMPANION_SAY".to_string(),
                "'); Remove-Item C:\\ -Recurse; ('".to_string()
            )]
        );
    }

    #[test]
    fn a_panel_is_escaped_and_opened_once() {
        let mut fake = Fake::default();
        fake.answers.insert("termux-open", "");
        let d = dir();
        let node = Node::new(file(&["canvas"]), termux(), &fake, d.clone());
        let script =
            json!({ "elements": [{ "type": "heading", "text": "<script>alert(1)</script>" }] });
        let refused = call(&node, "canvas_show", script);
        assert_eq!(refused["isError"], true);
        assert_eq!(
            refused["content"][0]["text"],
            "element 0: heading must not contain raw HTML"
        );
        let bad = call(
            &node,
            "canvas_show",
            json!({ "elements": [{ "type": "html", "html": "b" }] }),
        );
        assert_eq!(bad["isError"], true);
        assert!(
            !d.join(CANVAS_FILE).exists(),
            "nothing shown for a refused panel"
        );

        let spec = json!({ "title": "Tom's \"plan\"", "elements": [
            { "type": "list", "items": ["a & b"] }
        ]});
        assert_eq!(
            call(&node, "canvas_show", spec.clone())["content"][0]["text"],
            "Shown on Pixel."
        );
        call(&node, "canvas_show", spec);
        let page = std::fs::read_to_string(d.join(CANVAS_FILE)).expect("page");
        assert!(
            page.contains("<h1>Tom&#39;s &quot;plan&quot;</h1>"),
            "{page}"
        );
        assert!(page.contains("<li>a &amp; b</li>") && page.contains("default-src 'none'"));
        assert_eq!(
            fake.ran.lock().expect("ran").len(),
            1,
            "opened once, then it refreshes"
        );
        call(&node, "canvas_clear", json!({}));
        assert!(std::fs::read_to_string(d.join(CANVAS_FILE))
            .expect("page")
            .contains("Nothing to show."));
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn addresses_ids_and_numbers() {
        assert_eq!(
            host_address("192.168.1.20").as_deref(),
            Ok("192.168.1.20:18767")
        );
        assert_eq!(
            host_address("http://10.0.0.5:9000/").as_deref(),
            Ok("10.0.0.5:9000")
        );
        assert!(host_address("8.8.8.8").is_err());
        assert!(host_address("evil.example").is_err());
        assert_eq!(id_from("Pixel 8 Pro!"), "pixel-8-pro");
        assert_eq!(id_from("  "), "companion");
        assert_eq!(
            phone_number("+1 (555) 555-0100").as_deref(),
            Some("+15555550100")
        );
        assert_eq!(phone_number("12"), None);
        assert_eq!(phone_number("555;ls"), None);
    }

    /// Pair, wait for the yes, then answer a tool call the app makes,
    /// end to end over loopback.
    #[test]
    fn pairs_waits_for_the_yes_and_answers_the_app() {
        let d = dir();
        let key = "local-key-0123456789".to_string();
        let host = NodeHost::start(
            &d.join(nodes::FILE),
            IpAddr::V4(Ipv4Addr::LOCALHOST),
            0,
            key.clone(),
            "Desk",
        )
        .expect("host");
        let code = host.open_pairing().code;
        let mut f = pair(
            &format!("127.0.0.1:{}", host.port()),
            &code,
            "Pixel",
            "pixel",
            Os::Termux,
        )
        .expect("pair");
        assert_eq!(f.host_name, "Desk");
        f.offers = vec!["location".into()];
        let mut fake = Fake::default();
        fake.answers.insert(
            "termux-location",
            r#"{"latitude":1.5,"longitude":2.5,"accuracy":3.0}"#,
        );
        let fake: &'static Fake = Box::leak(Box::new(fake));
        let node = Node::new(f, termux(), fake, d.clone());
        assert_eq!(node.hello(), Ok(false));
        assert_eq!(node.poll_once(), Ok(Poll::Pending));
        assert!(host.approve("pixel", &["location".into()]));
        assert_eq!(node.hello(), Ok(true));

        let port = host.port();
        let app = std::thread::spawn(move || {
            // The app's call, once the companion is polling.
            std::thread::sleep(Duration::from_millis(100));
            let body = json!({"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"location_get","arguments":{}}});
            let resp = agent()
                .post(&format!("http://127.0.0.1:{port}/node/v1/mcp/pixel"))
                .header("Authorization", &format!("Bearer {key}"))
                .send_json(body)
                .expect("call");
            serde_json::from_reader::<_, Value>(resp.into_reader()).expect("json")
        });
        assert_eq!(node.poll_once(), Ok(Poll::Served));
        let answer = app.join().expect("app");
        assert_eq!(answer["id"], 5);
        assert_eq!(
            answer["result"]["content"][0]["text"],
            r#"{"accuracyM":3.0,"lat":1.5,"lon":2.5}"#
        );
        assert!(host.remove("pixel"));
        assert_eq!(node.poll_once(), Err("Pair this companion first.".into()));
        let _ = std::fs::remove_dir_all(d);
    }

    /// A fake MCP server on loopback. `/mcp` lists `add`; a path under
    /// `/node/v1/mcp/` plays another companion's relay and lists `camera_snap`.
    fn fake_mcp() -> u16 {
        let server = tiny_http::Server::http("127.0.0.1:0").expect("bind");
        let port = server.server_addr().to_ip().expect("ip").port();
        std::thread::spawn(move || {
            for mut req in server.incoming_requests() {
                let mut body = String::new();
                let _ = req.as_reader().read_to_string(&mut body);
                let rpc: Value = serde_json::from_str(&body).unwrap_or(Value::Null);
                let Some(id) = rpc.get("id").cloned() else {
                    let _ = req.respond(tiny_http::Response::empty(202));
                    continue;
                };
                let relay = req.url().contains("/node/v1/mcp/");
                let result = match rpc["method"].as_str().unwrap_or("") {
                    "initialize" => json!({
                        "protocolVersion": rpc["params"]["protocolVersion"],
                        "capabilities": { "tools": {} },
                        "serverInfo": { "name": "fake", "version": "1" },
                    }),
                    "tools/list" if relay => json!({ "tools": [
                        { "name": "camera_snap", "inputSchema": { "type": "object" } }
                    ]}),
                    "tools/list" => json!({ "tools": [{
                        "name": "add",
                        "description": "Add a note.",
                        "inputSchema": { "type": "object", "properties": { "text": { "type": "string" } } },
                    }]}),
                    "tools/call" => json!({ "content": [{ "type": "text", "text": format!(
                        "added {}",
                        rpc["params"]["arguments"]["text"].as_str().unwrap_or("")
                    )}]}),
                    _ => json!({}),
                };
                let body = json!({ "jsonrpc": "2.0", "id": id, "result": result }).to_string();
                let header = tiny_http::Header::from_bytes("Content-Type", "application/json")
                    .expect("header");
                let _ = req.respond(tiny_http::Response::from_string(body).with_header(header));
            }
        });
        port
    }

    /// The `mcp` offer lends this device's own MCP servers under
    /// `server-tool` names, and never another companion's relay.
    #[test]
    fn lends_its_own_mcp_servers_but_never_another_companions() {
        let d = dir();
        let config = d.join("config");
        std::fs::create_dir_all(&config).expect("config");
        let port = fake_mcp();
        std::fs::write(
            config.join("mcp.json"),
            json!({ "mcpServers": {
                "notes": { "url": format!("http://127.0.0.1:{port}/mcp") },
                "node-cam": { "url": format!("http://127.0.0.1:{port}/node/v1/mcp/cam") },
            }})
            .to_string(),
        )
        .expect("mcp.json");
        // nextest gives each test its own process; under `cargo test` the
        // old value is put back below so later tests don't inherit it.
        let before = std::env::var_os("GROKHUB_CONFIG");
        // SAFETY: no other thread of this test reads the environment.
        unsafe { std::env::set_var("GROKHUB_CONFIG", &config) };
        grokhub_agent::mcp::connect_in_background(&d);
        let mut connected = vec![];
        for _ in 0..500 {
            connected = grokhub_agent::mcp::tool_list();
            if connected.len() == 2 {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(connected.len(), 2, "both servers connect on this device");

        let fake = Fake::default();
        let node = Node::new(file(&["mcp"]), termux(), &fake, d.clone());
        let tools = node.tools();
        let names: Vec<&str> = tools
            .iter()
            .map(|t| t["name"].as_str().expect("name"))
            .collect();
        assert_eq!(names, ["notes-add"], "the relay's camera isn't lent on");
        assert_eq!(tools[0]["description"], "Add a note.");
        assert_eq!(tools[0]["annotations"]["readOnlyHint"], false);
        let out = call(&node, "notes-add", json!({ "text": "milk" }));
        assert_eq!(out["content"][0]["text"], "added milk");
        assert_eq!(out["isError"], false);
        let relayed = call(&node, "node-cam-camera_snap", json!({}));
        assert_eq!(relayed["isError"], true);
        assert_eq!(
            relayed["content"][0]["text"],
            "Pixel has no tool named `node-cam-camera_snap`."
        );

        grokhub_agent::mcp::shutdown_all();
        // SAFETY: as above.
        unsafe {
            match before {
                Some(v) => std::env::set_var("GROKHUB_CONFIG", v),
                None => std::env::remove_var("GROKHUB_CONFIG"),
            }
        }
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn skills_are_listed_and_read_but_not_the_built_in_ones() {
        let d = dir();
        let skill = d.join("home/.claude/skills/groceries");
        std::fs::create_dir_all(&skill).expect("skill dir");
        let text =
            "---\nname: groceries\ndescription: Plan the weekly shop.\n---\n\nList, then buy.\n";
        std::fs::write(skill.join("SKILL.md"), text).expect("skill");
        let fake = Fake::default();
        let mut node = Node::new(file(&["skills"]), termux(), &fake, d.clone());
        node.home = Some(d.join("home"));
        let list = call(&node, "skills_list", json!({}));
        assert_eq!(
            list["content"][0]["text"],
            r#"[{"description":"Plan the weekly shop.","name":"groceries"}]"#
        );
        let read = call(&node, "skill_read", json!({ "name": "groceries" }));
        assert_eq!(read["content"][0]["text"], text);
        let missing = call(&node, "skill_read", json!({ "name": "../../etc/passwd" }));
        assert_eq!(
            missing["content"][0]["text"],
            "Pixel has no skill named `../../etc/passwd`."
        );
        let _ = std::fs::remove_dir_all(d);
    }
}
