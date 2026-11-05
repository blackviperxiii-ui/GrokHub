//! What makes the live eval run the app's own shape: the provider a run goes
//! to, a scripted answerer for the Auto gate's asks, the `--ab` switches, and
//! the generated files the long cases need.

use std::path::Path;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use serde_json::json;

use crate::client::{map_http_status, ClientError, InputItem, ResponsesRequest, StreamEvent};
use crate::gate::{PermAnswer, PermitWait, Waited};
use crate::route::local::LocalRuntime;
use crate::route::local_http::HttpLocalRuntime;
use crate::route::providers::{openai_body, parse_openai_reply, ProviderTransport, UreqTransport};
use crate::{CancelToken, ModelClient, TurnOutput};

/// Where the model calls go. Grok is first; the others show how the same
/// tasks fare on a provider you add or on this computer's model.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Provider {
    #[default]
    Grok,
    OpenRouter,
    Local,
}

impl Provider {
    pub fn parse(raw: &str) -> Result<Self, String> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "grok" | "xai" => Ok(Self::Grok),
            "openrouter" => Ok(Self::OpenRouter),
            "local" => Ok(Self::Local),
            other => Err(format!(
                "--provider {other} is not one of grok, openrouter, local"
            )),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Grok => "grok",
            Self::OpenRouter => "openrouter",
            Self::Local => "local",
        }
    }
}

/// The Auto gate's answerer for a run with nobody at the keyboard: it allows
/// every ask, as a person who wants the task done would, and counts them, so
/// the report shows how often Auto mode would have stopped to ask.
#[derive(Default)]
pub struct ScriptedPermits {
    asked: AtomicU32,
}

impl ScriptedPermits {
    pub fn asked(&self) -> u32 {
        self.asked.load(Ordering::SeqCst)
    }
}

impl PermitWait for ScriptedPermits {
    fn wait(&self, _call_id: &str, cancel: &CancelToken, halted: &dyn Fn() -> bool) -> Waited {
        if cancel.is_cancelled() {
            return Waited::Cancelled;
        }
        if halted() {
            return Waited::Halted;
        }
        self.asked.fetch_add(1, Ordering::SeqCst);
        Waited::Answer(PermAnswer::Allow)
    }
}

/// One `--ab` switch: the config file it lives in and its key.
pub struct AbFlag {
    pub name: &'static str,
    pub file: &'static str,
    pub key: &'static str,
}

/// The switches `--ab` can flip. Each is a setting the app reads from its
/// config folder, so the eval sets it in the run's own throwaway folder.
pub const AB_FLAGS: &[AbFlag] = &[
    AbFlag {
        name: "websocket",
        file: crate::ws_mode::CONFIG_NAME,
        key: "enabled",
    },
    AbFlag {
        name: "tool_output_compact",
        file: crate::compact::TOOL_OUTPUT_COMPACT_CONFIG,
        key: "enabled",
    },
    AbFlag {
        name: "deferred_tools",
        file: "deferred_tools.toml",
        key: "enabled",
    },
    AbFlag {
        name: "batched_tools",
        file: "batched_tools.toml",
        key: "enabled",
    },
    AbFlag {
        name: "history_trim",
        file: crate::compact::HISTORY_TRIM_CONFIG,
        key: "enabled",
    },
    AbFlag {
        name: "age_out",
        file: crate::compact::HISTORY_TRIM_CONFIG,
        key: "age_out",
    },
    AbFlag {
        name: "lean_prompt",
        file: crate::diet::CONFIG_NAME,
        key: "lean_prompt",
    },
    AbFlag {
        name: "sparse_line_numbers",
        file: crate::diet::CONFIG_NAME,
        key: "sparse_line_numbers",
    },
];

/// The switch named `name`, or a message listing the ones there are.
pub fn ab_flag(name: &str) -> Result<&'static AbFlag, String> {
    AB_FLAGS
        .iter()
        .find(|f| f.name == name.trim())
        .ok_or_else(|| {
            let names: Vec<&str> = AB_FLAGS.iter().map(|f| f.name).collect();
            format!("--ab {name} is not one of {}", names.join(", "))
        })
}

/// Turn `flag` on or off in `config_dir`, keeping any other key in its file.
pub fn write_ab(config_dir: &Path, flag: &AbFlag, on: bool) -> Result<(), String> {
    std::fs::create_dir_all(config_dir).map_err(|e| e.to_string())?;
    let path = config_dir.join(flag.file);
    let old = std::fs::read_to_string(&path).unwrap_or_default();
    let prefix = format!("{} ", flag.key);
    let prefix_eq = format!("{}=", flag.key);
    let mut lines: Vec<String> = old
        .lines()
        .filter(|l| {
            let t = l.trim_start();
            !(t.starts_with(&prefix) || t.starts_with(&prefix_eq))
        })
        .map(str::to_string)
        .collect();
    lines.push(format!("{} = {on}", flag.key));
    std::fs::write(&path, lines.join("\n") + "\n").map_err(|e| e.to_string())
}

/// Sends every call to one OpenRouter model, the way the router's provider
/// path builds the body, with the key from the environment.
pub struct OpenRouterClient {
    key: String,
    model: String,
}

impl OpenRouterClient {
    pub const URL: &'static str = "https://openrouter.ai/api/v1/chat/completions";

    pub fn new(key: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            model: model.into(),
        }
    }
}

impl ModelClient for OpenRouterClient {
    fn stream(
        &self,
        req: &ResponsesRequest,
        cancel: &CancelToken,
        sink: &mut dyn FnMut(StreamEvent),
    ) -> Result<TurnOutput, ClientError> {
        if cancel.is_cancelled() {
            return Err(ClientError::Cancelled);
        }
        let mut req = req.clone();
        // As in the app: the model is told which model really answers.
        crate::route::identity::stamp(&mut req.input, &self.model, None, req.effort.as_deref());
        let mut body = openai_body(&self.model, req.effort.as_deref(), &req);
        // Ask OpenRouter for the call's cost so the cap counts real money.
        body["usage"] = json!({ "include": true });
        let headers = vec![
            ("Authorization".to_string(), format!("Bearer {}", self.key)),
            ("Content-Type".to_string(), "application/json".to_string()),
        ];
        let reply = UreqTransport
            .post_within(Self::URL, &headers, &body.to_string(), req.call_timeout)
            .map_err(|e| ClientError::Transport(grokhub_core::redact_secrets(&e)))?;
        if cancel.is_cancelled() {
            return Err(ClientError::Cancelled);
        }
        if !(200..300).contains(&reply.status) {
            return Err(map_http_status(
                reply.status,
                &grokhub_core::redact_secrets(&reply.body),
                None,
                crate::AuthKind::ApiKey,
            ));
        }
        let mut turn = parse_openai_reply(&reply.body)?;
        for item in &mut turn.reasoning_items {
            if let InputItem::Reasoning { model, .. } = item {
                *model = self.model.clone();
            }
        }
        if !turn.text.is_empty() {
            sink(StreamEvent::TextDelta(turn.text.clone()));
        }
        Ok(turn)
    }
}

/// Sends every call to this computer's model server, as the router's local
/// path does (tools included).
pub struct LocalClient {
    runtime: Arc<HttpLocalRuntime>,
}

impl LocalClient {
    pub fn new(base_url: &str, token: &str, model: &str) -> Self {
        Self {
            runtime: Arc::new(HttpLocalRuntime::new(base_url, token).with_model(model)),
        }
    }
}

impl ModelClient for LocalClient {
    fn stream(
        &self,
        req: &ResponsesRequest,
        cancel: &CancelToken,
        sink: &mut dyn FnMut(StreamEvent),
    ) -> Result<TurnOutput, ClientError> {
        let turn = self
            .runtime
            .chat_tools(req, cancel)
            .map_err(ClientError::Transport)?;
        if !turn.text.is_empty() {
            sink(StreamEvent::TextDelta(turn.text.clone()));
        }
        Ok(turn)
    }
}

/// Files for the masking case: twelve logs, each big enough to be masked once
/// ten newer outputs follow it, with one fault code in each.
pub fn seed_long_logs(ws: &Path) {
    let dir = ws.join("logs/run");
    let _ = std::fs::create_dir_all(&dir);
    for n in 1..=12u32 {
        let mut body = String::new();
        for line in 0..60u32 {
            body.push_str(&format!(
                "2026-11-0{} 0{}:{:02}:00 frame {line:03} ok, 7{line:02} fps, gpu {}C\n",
                n % 9 + 1,
                line % 10,
                line,
                60 + line % 20
            ));
        }
        body.push_str(&format!(
            "FAULT code LX-{:04} on part {n:02}\n",
            1000 + n * 37
        ));
        let _ = std::fs::write(dir.join(format!("part{n:02}.log")), body);
    }
}

/// The fault code seeded in part `n` of [`seed_long_logs`].
#[cfg(test)]
pub fn long_log_code(n: u32) -> String {
    format!("LX-{:04}", 1000 + n * 37)
}

/// Files for the compaction case: three long, dull reports. Read in turn they
/// fill a small window and force a compaction.
pub fn seed_big_reports(ws: &Path) {
    let dir = ws.join("reports/big");
    let _ = std::fs::create_dir_all(&dir);
    for n in 1..=3u32 {
        let mut body = String::new();
        for line in 0..800u32 {
            body.push_str(&format!(
                "sample {n}.{line:04}: cpu {}%, gpu {}%, vram {} MB, fan {} rpm, all normal\n",
                20 + line % 50,
                30 + line % 60,
                4000 + line % 900,
                900 + line % 700
            ));
        }
        let _ = std::fs::write(dir.join(format!("week{n}.txt")), body);
    }
}

/// A small source tree for the Explore case: the function sits a few folders
/// down, among files that only mention it.
pub fn seed_src_tree(ws: &Path) {
    let files: &[(&str, &str)] = &[
        ("src/main.rs", "mod render;\nmod hud;\nfn main() { hud::draw(); }\n"),
        ("src/hud/mod.rs", "pub fn draw() { /* calls frame_budget later */ }\n"),
        ("src/render/mod.rs", "pub mod pacing;\npub mod shaders;\n"),
        ("src/render/shaders.rs", "// frame_budget is not here\npub fn compile() {}\n"),
        (
            "src/render/pacing/limiter.rs",
            "/// Milliseconds a frame may take at `fps`.\npub fn frame_budget(fps: u32) -> f32 {\n    1000.0 / fps as f32\n}\n",
        ),
        ("src/render/pacing/mod.rs", "pub mod limiter;\n"),
        ("docs/notes.md", "frame_budget: see the pacing code\n"),
    ];
    for (path, body) in files {
        let p = ws.join(path);
        if let Some(parent) = p.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(p, body);
    }
}

/// Eight small screenshots, one solid colour each. Shot 6 is the red one;
/// reading all eight passes the six-image limit, so older ones get masked.
pub fn seed_shots(ws: &Path) {
    let dir = ws.join("shots");
    let _ = std::fs::create_dir_all(&dir);
    let colours: [[u8; 3]; 8] = [
        [30, 30, 200],
        [30, 160, 30],
        [200, 200, 30],
        [120, 120, 120],
        [160, 30, 160],
        [220, 20, 20],
        [20, 160, 160],
        [240, 240, 240],
    ];
    for (i, rgb) in colours.iter().enumerate() {
        let img = image::RgbImage::from_pixel(64, 64, image::Rgb(*rgb));
        let _ = img.save(dir.join(format!("shot{}.png", i + 1)));
    }
}

/// The red shot in [`seed_shots`].
#[cfg(test)]
pub const RED_SHOT: &str = "shot6";
