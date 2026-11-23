//! Pictures made on this computer: one `sd-cli` run per picture with the
//! catalog picture model (Z-Image Turbo), so the model loads on demand and
//! leaves memory the moment the picture is done. Then the picture guard
//! (Qwen3-VL 2B in its own short-lived `llama-server`) looks at it against
//! the hard bans before anything is shown, and is stopped right after.
//!
//! Nothing runs while the governor has local work paused (a game or a heavy
//! app is running), so a picture never costs a game frames. An 18+ prompt
//! is only drawn when the guard is on disk; a guard that can't answer counts
//! as a no, and the picture is deleted.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use grokhub_core::image_engine as ie;
use grokhub_core::local_setup::LocalModel;
use serde_json::{json, Value};

use super::local_resident as resident;
use super::local_sidecar as sidecar;

/// Longest one picture may take (a slow CPU-only machine still finishes).
const DRAW_TIMEOUT: Duration = Duration::from_secs(15 * 60);
/// Longest the guard may take to load and answer.
const GUARD_TIMEOUT: Duration = Duration::from_secs(3 * 60);

/// Nothing here can draw a picture: no Grok Imagine sign-in and no picture
/// model on disk.
pub const NO_SOURCE: &str = "Not made, and nothing was sent: nothing here can draw pictures yet. Download Z-Image Turbo from Settings → Download models → Image, or sign in with Grok or add an API key.";
/// Why a picture wasn't made on this computer.
pub const NOT_SET_UP: &str = "Not made, and nothing was sent: on-device pictures need Z-Image Turbo from Settings → Download models.";
/// An 18+ ask with no guard on disk.
pub const NEEDS_GUARD: &str = "Not made, and nothing was sent: 18+ pictures are only made on this computer, with Z-Image Turbo and the picture guard from Settings → Download models → Image.";
/// The governor has local work paused.
pub const PAUSED: &str = "Not made yet: a game or a heavy app is running, so on-device pictures wait until it's done. Ask again then.";
/// The guard found a hard ban in the finished picture.
pub const GUARD_BLOCKED: &str = "Not shown, and deleted: the picture guard found something every picture is barred from (minors, non-consent, real people in sexual content, or animals).";
/// The guard didn't give a clear answer, so the picture isn't shown.
pub const GUARD_UNSURE: &str = "Not shown, and deleted: the picture guard couldn't check this picture, so it stays hidden. Try again.";
/// There wasn't the memory to run the guard on the finished picture.
pub const GUARD_NO_ROOM: &str = "Not shown, and deleted: this computer didn't have the free memory to check the picture. Close some apps, then ask again.";

/// What's on disk for on-device pictures.
#[derive(Clone)]
pub struct Kit {
    pub cli: PathBuf,
    pub models_dir: PathBuf,
    pub model: &'static LocalModel,
    pub guard: Option<&'static LocalModel>,
}

/// The engine and picture model, when both are on disk.
pub fn kit(config_dir: &Path, models_dir: &Path) -> Option<Kit> {
    if !ie::installed(config_dir) {
        return None;
    }
    Some(Kit {
        cli: ie::cli_path(config_dir),
        models_dir: models_dir.to_path_buf(),
        model: ie::image_model(models_dir)?,
        guard: ie::guard_model(models_dir),
    })
}

/// The kit in the usual places: `{config}/bin/sd-cpp` and `~/GrokHub/models`.
pub fn kit_here() -> Option<Kit> {
    kit(
        &crate::perm::config_dir(),
        &grokhub_core::local_draft::models_root()?,
    )
}

/// The tool an on-device turn draws with.
pub const TOOL: &str = crate::tools::registry::IMAGE_GENERATE;
/// Told to the device when this computer can draw.
pub const CAN_DRAW: &str = "[Pictures] You can draw pictures here: for a picture, call image_generate with a prompt you write. Never say you can't make images.";
/// Added when the newest ask is for a picture.
pub const ASKED: &str = "They want one now.";
/// Told to the device on a picture ask when nothing here can draw yet.
pub const NO_KIT: &str = "[Pictures] Nothing here can draw pictures yet. Never say you can't make images: call image_generate, and if it can't, tell the user to download a picture model (Z-Image Turbo or Chroma1-HD) and the picture guard from Settings → Download models → Image.";

#[cfg(test)]
thread_local! {
    static TEST_KIT: std::cell::Cell<Option<bool>> = const { std::cell::Cell::new(None) };
}

/// Pins [`can_draw_here`] on this thread until dropped.
#[cfg(test)]
pub(crate) struct KitForTests;

#[cfg(test)]
impl KitForTests {
    pub(crate) fn set(on: bool) -> Self {
        TEST_KIT.with(|c| c.set(Some(on)));
        Self
    }
}

#[cfg(test)]
impl Drop for KitForTests {
    fn drop(&mut self) {
        TEST_KIT.with(|c| c.set(None));
    }
}

/// The engine and a picture model are on disk: a few file checks, run once
/// per on-device turn and never in the background.
pub fn can_draw_here() -> bool {
    #[cfg(test)]
    if let Some(on) = TEST_KIT.with(std::cell::Cell::get) {
        return on;
    }
    kit_here().is_some()
}

/// The gate offers the picture tool this turn: in the list, or in the
/// deferred index (`name — blurb` lines) of `tool_search`.
fn offered(tools: &[Value]) -> bool {
    let index_line = format!("\n{TOOL} — ");
    tools.iter().any(|t| {
        t["name"].as_str() == Some(TOOL)
            || (t["name"].as_str() == Some(crate::tools::registry::TOOL_SEARCH)
                && t["description"]
                    .as_str()
                    .is_some_and(|d| d.contains(&index_line)))
            || (t["name"].as_str() == Some(crate::tools::registry::LOAD_TOOL_SCHEMA)
                && t["description"]
                    .as_str()
                    .is_some_and(|d| d.contains(&index_line)))
    })
}

/// Get an on-device turn ready to draw. When this computer `can_draw` or the
/// newest message is a picture `ask`, and the gate offers the picture tool,
/// its full schema is in the list and the first system message says what
/// the device can do. Returns the tools [`super::local_fit::fit_keeping`]
/// must keep. The model still writes the prompt, so the hard-ban check and
/// the picture guard see what it asks for.
pub fn ready_on_device(
    req: &mut crate::client::ResponsesRequest,
    can_draw: bool,
    ask: bool,
) -> &'static [&'static str] {
    if !(can_draw || ask) || !offered(&req.tools) {
        return &[];
    }
    if !req.tools.iter().any(|t| t["name"].as_str() == Some(TOOL)) {
        if let Some(schema) = crate::tools::native_schema(TOOL) {
            req.tools.push(schema);
        }
    }
    let line = match (can_draw, ask) {
        (true, true) => format!("{CAN_DRAW} {ASKED}"),
        (true, false) => CAN_DRAW.to_string(),
        _ => NO_KIT.to_string(),
    };
    add_to_system(&mut req.input, &line);
    &[TOOL]
}

/// The newest message is a picture ask this computer can draw, and nothing
/// has answered it yet: this step offers only the picture tool and must call
/// it ([`ForceGuard`]), so the picture never hangs on the chat model choosing
/// to. The model still writes the prompt. Run after [`ready_on_device`].
pub fn picture_step(req: &crate::client::ResponsesRequest, can_draw: bool, ask: bool) -> bool {
    use crate::client::InputItem;
    can_draw
        && ask
        && matches!(req.input.last(), Some(InputItem::Message { role, .. }) if role == "user")
        && req.tools.iter().any(|t| t["name"].as_str() == Some(TOOL))
}

/// `req` with only the picture tool offered.
pub fn only_picture_tool(req: &crate::client::ResponsesRequest) -> crate::client::ResponsesRequest {
    let mut req = req.clone();
    req.tools.retain(|t| t["name"].as_str() == Some(TOOL));
    req
}

thread_local! {
    static FORCED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// While held, this thread's on-device call must call a tool
/// (`tool_choice: "required"`): a [`picture_step`].
pub struct ForceGuard {
    prev: bool,
}

impl ForceGuard {
    pub fn set() -> Self {
        Self {
            prev: FORCED.with(|f| f.replace(true)),
        }
    }
}

impl Drop for ForceGuard {
    fn drop(&mut self) {
        FORCED.with(|f| f.set(self.prev));
    }
}

/// A [`ForceGuard`] is held on this thread.
pub fn forced() -> bool {
    FORCED.with(std::cell::Cell::get)
}

/// The forced picture step's answer, with the picture call made from the
/// person's own words when the model answered without one: an engine or a
/// small model can ignore `tool_choice: "required"` and say it drew. Its
/// text (the claim) is dropped; the tool then draws or says why not.
pub fn with_picture_call(
    mut out: crate::client::TurnOutput,
    ask: &str,
) -> crate::client::TurnOutput {
    if out.calls.iter().any(|c| c.name == TOOL) || ask.trim().is_empty() {
        return out;
    }
    out.text.clear();
    out.calls = vec![crate::client::FunctionCall {
        call_id: format!("call_local_picture_{}", grokhub_core::now_ms()),
        name: TOOL.to_string(),
        arguments: json!({ "prompt": ask.trim() }).to_string(),
    }];
    out
}

/// Memory kept free above the picture model while it draws.
pub const DRAW_HEADROOM_MB: u64 = 512;

/// Said when a picture can't fit in memory now.
pub fn no_room_line(need: resident::Mem) -> String {
    let gb = |mb: u64| mb.div_ceil(1024);
    let what = if need.vram_mb == 0 {
        format!("about {} GB of free memory", gb(need.ram_mb))
    } else {
        format!(
            "about {} GB of free memory and {} GB of free graphics memory",
            gb(need.ram_mb),
            gb(need.vram_mb)
        )
    };
    format!("Not made, and nothing was sent: drawing needs {what} and this computer doesn't have it right now. Close some apps, then ask again.")
}

/// Graphics memory one picture takes: `sd-cli --offload-to-cpu` keeps the
/// weights in RAM and moves one piece at a time (the text encoder, the
/// picture model, the decoder) onto the GPU, so it needs the largest piece,
/// the compute buffers (the catalog's run size past the download) and
/// headroom.
pub fn draw_vram_mb(m: &LocalModel) -> u64 {
    let parts: Vec<u64> = m.parts.iter().map(|p| p.bytes.div_ceil(1 << 20)).collect();
    let main = m.download_mb.saturating_sub(parts.iter().sum());
    let largest = parts.into_iter().chain([main]).max().unwrap_or(0);
    largest + m.run_mb.saturating_sub(m.download_mb) + DRAW_HEADROOM_MB
}

/// What one picture needs: RAM for the weights, and on a separate graphics
/// card its share there too, so a chat model filling the card unloads first
/// (or the picture is refused) instead of the engine running out of memory.
/// With shared memory the GPU's share is the same RAM.
pub fn draw_need(m: &LocalModel, room: Option<&super::local_scale::Room>) -> resident::Mem {
    let ram = resident::Mem::ram(m.run_mb + DRAW_HEADROOM_MB);
    match room {
        Some(r) if !r.unified && r.free_vram_mb.is_some_and(|v| v > 0) => resident::Mem {
            vram_mb: draw_vram_mb(m),
            ..ram
        },
        _ => ram,
    }
}

/// Append `line` as its own paragraph to the first system message.
fn add_to_system(input: &mut Vec<crate::client::InputItem>, line: &str) {
    use crate::client::{ContentPart, InputItem};
    let system = input.iter_mut().find_map(|item| match item {
        InputItem::Message { role, content } if role == "system" || role == "developer" => {
            content.iter_mut().find_map(|c| match c {
                ContentPart::InputText(t) => Some(t),
                _ => None,
            })
        }
        _ => None,
    });
    match system {
        Some(text) => {
            text.push_str("\n\n");
            text.push_str(line);
        }
        None => input.insert(
            0,
            InputItem::Message {
                role: "system".into(),
                content: vec![ContentPart::InputText(line.into())],
            },
        ),
    }
}

/// Make one picture into `out_dir` and return its path. `explicit` is an 18+
/// prompt (it already passed the hard-ban check). `stop` is the chat's
/// cancel; a stopped run deletes its half-made file.
pub fn make(
    kit: &Kit,
    prompt: &str,
    aspect: &str,
    explicit: bool,
    out_dir: &Path,
    stop: &dyn Fn() -> bool,
) -> Result<PathBuf, String> {
    if super::local_gov::paused() {
        return Err(PAUSED.into());
    }
    if explicit && kit.guard.is_none() {
        return Err(NEEDS_GUARD.into());
    }
    // Held from the memory check until sd-cli exits, so keep-warm doesn't
    // load the chat model back into the room the picture needs.
    let room = super::local_scale::room_at(grokhub_core::now_ms()).map(|(r, _)| r);
    let need = draw_need(kit.model, room.as_ref());
    let drawing =
        resident::reserve(resident::Kind::Picture, need, true).map_err(|_| no_room_line(need))?;
    std::fs::create_dir_all(out_dir)
        .map_err(|e| format!("Couldn't make the picture folder: {e}"))?;
    let out = out_dir.join(format!("image-{}-local.png", grokhub_core::now_ms()));
    let seed = grokhub_core::now_ms() % 1_000_000_007;
    let args = ie::generate_args(
        &kit.models_dir,
        kit.model,
        prompt,
        ie::size_for(aspect),
        seed,
        &out,
    )
    .ok_or_else(|| NOT_SET_UP.to_string())?;
    let mut cmd = Command::new(&kit.cli);
    cmd.args(&args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    grokhub_core::proc_util::hide_windows_console(&mut cmd);
    let child = cmd
        .spawn()
        .map_err(|e| format!("Couldn't start the picture engine: {e}"))?;
    if let Err(e) = wait(child, DRAW_TIMEOUT, stop) {
        let _ = std::fs::remove_file(&out);
        return Err(e);
    }
    if !out.is_file() {
        return Err("The picture engine finished without a picture.".into());
    }
    // The picture hands its place to the guard: sd-cli's memory counts as
    // free again, and keep-warm can't load the chat model in between.
    if let Some(g) = kit.guard {
        let Ok(_checking) = drawing.then(
            resident::Kind::PictureGuard,
            resident::Mem::ram(g.run_mb),
            true,
        ) else {
            let _ = std::fs::remove_file(&out);
            return Err(GUARD_NO_ROOM.into());
        };
        let verdict = judge(&kit.models_dir, g, &out, stop);
        if verdict != Ok(false) {
            let _ = std::fs::remove_file(&out);
            return Err(match verdict {
                Ok(true) => GUARD_BLOCKED.into(),
                _ => GUARD_UNSURE.into(),
            });
        }
    }
    Ok(out)
}

/// Wait for a child, killing it on `stop` or after `limit`. A failed exit
/// names the last line it printed.
fn wait(mut child: Child, limit: Duration, stop: &dyn Fn() -> bool) -> Result<(), String> {
    let end = Instant::now() + limit;
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(()),
            Ok(Some(status)) => {
                let mut err = String::new();
                if let Some(mut e) = child.stderr.take() {
                    let _ = e.read_to_string(&mut err);
                }
                let last = err.lines().rev().find(|l| !l.trim().is_empty());
                return Err(format!(
                    "The picture engine stopped ({status}){}",
                    last.map(|l| format!(": {}", l.trim())).unwrap_or_default()
                ));
            }
            Ok(None) if stop() => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("cancelled".into());
            }
            Ok(None) if Instant::now() >= end => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("The picture took too long and was stopped.".into());
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(200)),
            Err(e) => return Err(e.to_string()),
        }
    }
}

/// Ask the guard about one picture in its own `llama-server`, stopped right
/// after. `Ok(true)` is banned; an error or an unclear answer is not `Ok(false)`.
fn judge(
    models_dir: &Path,
    g: &LocalModel,
    picture: &Path,
    stop: &dyn Fn() -> bool,
) -> Result<bool, String> {
    let config = crate::perm::config_dir();
    let bin = sidecar::find_binary(&config).ok_or("The local engine isn't installed.")?;
    let (model, mmproj) = ie::guard_files(models_dir, g).ok_or("The guard's files are missing.")?;
    let port = sidecar::free_loopback_port()?;
    let token = format!("{:x}{:x}", grokhub_core::now_ms(), std::process::id());
    let mut cmd = Command::new(&bin);
    cmd.args(guard_args(&model, &mmproj, port))
        .env("LLAMA_API_KEY", &token)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    grokhub_core::proc_util::hide_windows_console(&mut cmd);
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("Couldn't start the picture guard: {e}"))?;
    let out = ask_guard(port, &token, picture, stop);
    let _ = child.kill();
    let _ = child.wait();
    out
}

/// The guard's server: loopback only, a small context, one slot.
pub fn guard_args(model: &Path, mmproj: &Path, port: u16) -> Vec<String> {
    [
        "-m",
        &model.display().to_string(),
        "--mmproj",
        &mmproj.display().to_string(),
        "--host",
        "127.0.0.1",
        "--port",
        &port.to_string(),
        "-c",
        "4096",
        "-np",
        "1",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

fn ask_guard(
    port: u16,
    token: &str,
    picture: &Path,
    stop: &dyn Fn() -> bool,
) -> Result<bool, String> {
    let end = Instant::now() + GUARD_TIMEOUT;
    while !sidecar::health_ok("127.0.0.1", port, token, Duration::from_millis(500)) {
        if stop() {
            return Err("cancelled".into());
        }
        if Instant::now() >= end {
            return Err("The picture guard didn't start.".into());
        }
        std::thread::sleep(Duration::from_millis(300));
    }
    let png = std::fs::read(picture).map_err(|e| e.to_string())?;
    let body = guard_body(&png);
    let agent = grokhub_core::http::agent(grokhub_core::http::Opts {
        connect: Duration::from_secs(5),
        read: Some(GUARD_TIMEOUT),
        ..Default::default()
    });
    let reply: Value = agent
        .post(&format!("http://127.0.0.1:{port}/v1/chat/completions"))
        .header("Authorization", &format!("Bearer {token}"))
        .header("Content-Type", "application/json")
        .send_json(body)
        .map_err(|e| format!("The picture guard didn't answer: {e}"))?
        .into_json()
        .map_err(|e| format!("The picture guard's answer was unreadable: {e}"))?;
    let text = reply["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or_default();
    ie::guard_verdict(text).ok_or_else(|| "unclear".to_string())
}

/// One user turn: the picture and the question; a few tokens back.
pub fn guard_body(png: &[u8]) -> Value {
    use base64::Engine as _;
    let data = base64::engine::general_purpose::STANDARD.encode(png);
    json!({
        "messages": [{
            "role": "user",
            "content": [
                { "type": "image_url", "image_url": { "url": format!("data:image/png;base64,{data}") } },
                { "type": "text", "text": ie::GUARD_QUESTION },
            ],
        }],
        "max_tokens": 8,
        "temperature": 0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_is_drawn_without_the_engine_and_the_model_on_disk() {
        let d = std::env::temp_dir().join(format!("grokhub-img-{}", std::process::id()));
        assert!(kit(&d, &d).is_none());
    }

    #[test]
    fn the_guard_listens_on_loopback_and_gets_the_picture_and_the_question() {
        let a = guard_args(Path::new("/m/g.gguf"), Path::new("/m/mmproj.gguf"), 4242);
        let after = |k: &str| a[a.iter().position(|x| x == k).unwrap() + 1].as_str();
        assert_eq!(after("--host"), "127.0.0.1");
        assert_eq!(after("--mmproj"), "/m/mmproj.gguf");
        assert_eq!(after("--port"), "4242");
        let b = guard_body(b"\x89PNG");
        let parts = &b["messages"][0]["content"];
        assert!(parts[0]["image_url"]["url"]
            .as_str()
            .unwrap()
            .starts_with("data:image/png;base64,"));
        assert_eq!(parts[1]["text"], ie::GUARD_QUESTION);
        assert_eq!(b["temperature"], 0);
    }

    #[test]
    fn a_stopped_or_failing_run_reports_why() {
        #[cfg(unix)]
        {
            let ok = Command::new("true").spawn().unwrap();
            assert_eq!(wait(ok, Duration::from_secs(5), &|| false), Ok(()));
            let mut sh = Command::new("sh");
            sh.args(["-c", "echo 'out of memory' >&2; exit 3"])
                .stderr(Stdio::piped());
            let err = wait(sh.spawn().unwrap(), Duration::from_secs(5), &|| false).unwrap_err();
            assert!(err.contains("out of memory"), "{err}");
            let slow = Command::new("sleep").arg("30").spawn().unwrap();
            assert_eq!(
                wait(slow, Duration::from_secs(5), &|| true),
                Err("cancelled".into())
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_picture_declines_before_the_engine_starts_when_memory_is_short() {
        use super::super::local_scale::{clear_room, set_room, Room};
        use std::os::unix::fs::PermissionsExt;
        let root = std::env::temp_dir().join(format!(
            "grokhub-image-room-{}-{}",
            std::process::id(),
            grokhub_core::now_ms()
        ));
        std::fs::create_dir_all(&root).unwrap();
        // A stand-in engine: notes that it ran, then writes a PNG header to
        // the path after -o.
        let cli = root.join("sd-cli");
        let ran = root.join("ran");
        std::fs::write(
            &cli,
            format!(
                "#!/bin/sh\ntouch '{}'\nwhile [ $# -gt 0 ]; do if [ \"$1\" = -o ]; then printf '\\211PNG' > \"$2\"; fi; shift; done\n",
                ran.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&cli, std::fs::Permissions::from_mode(0o755)).unwrap();
        let model = grokhub_core::local_setup::local_models()
            .iter()
            .find(|m| m.kind == grokhub_core::local_setup::ModelKind::Image)
            .unwrap();
        let kit = Kit {
            cli,
            models_dir: root.clone(),
            model,
            guard: None,
        };
        let room = |free_ram_mb| Room {
            free_vram_mb: Some(24_576),
            free_ram_mb,
            cpu_busy_pct: None,
            on_battery: false,
            unified: false,
        };
        let out = root.join("media");
        // No chat model is loaded, and 80% of 4 GB is short of the picture
        // model plus 512 MB: declined, and sd-cli never ran.
        set_room(room(4_096), grokhub_core::now_ms());
        let short = make(&kit, "a fox", "1:1", false, &out, &|| false);
        assert_eq!(
            short,
            Err(no_room_line(super::super::local_resident::Mem {
                ram_mb: model.run_mb + DRAW_HEADROOM_MB,
                vram_mb: draw_vram_mb(model),
            }))
        );
        assert!(!ran.exists());
        // Enough room: it draws, and the job leaves the ledger when done.
        set_room(room(65_536), grokhub_core::now_ms());
        let made = make(&kit, "a fox", "1:1", false, &out, &|| false);
        clear_room();
        let made = made.unwrap();
        assert_eq!(std::fs::read(&made).unwrap(), b"\x89PNG");
        assert!(ran.exists());
        assert!(!super::super::local_resident::held());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_picture_step_that_only_claims_it_drew_calls_the_tool_with_the_ask() {
        use crate::client::{FunctionCall, TurnOutput};
        let claimed = TurnOutput {
            text: "Here is your picture of a fox!".into(),
            ..Default::default()
        };
        let out = with_picture_call(claimed, " a fox in snow ");
        assert_eq!(out.text, "");
        assert_eq!(
            out.calls
                .iter()
                .map(|c| (c.name.as_str(), c.arguments.as_str()))
                .collect::<Vec<_>>(),
            [(TOOL, r#"{"prompt":"a fox in snow"}"#)]
        );
        assert!(out.calls[0].call_id.starts_with("call_local_picture_"));
        // A real call is kept as the model wrote it.
        let called = TurnOutput {
            calls: vec![FunctionCall {
                call_id: "c1".into(),
                name: TOOL.into(),
                arguments: r#"{"prompt":"a red fox, snow, dusk"}"#.into(),
            }],
            ..Default::default()
        };
        assert_eq!(with_picture_call(called.clone(), "a fox"), called);
    }

    #[test]
    fn a_picture_on_a_graphics_card_counts_its_share_there_and_can_unload_the_chat_model() {
        use super::super::local_resident::{admit, Admit, Entry, Kind, Mem};
        use super::super::local_scale::Room;
        let model = grokhub_core::local_setup::model_by_id("z-image-turbo-q4_k").unwrap();
        // Largest piece 3,685 MB (the picture model), 781 MB of buffers, 512 spare.
        assert_eq!(draw_vram_mb(model), 4_978);
        let card = Room {
            free_vram_mb: Some(2_048),
            free_ram_mb: 48_000,
            ..Room::default()
        };
        let need = draw_need(model, Some(&card));
        assert_eq!(
            need,
            Mem {
                ram_mb: 7_680,
                vram_mb: 4_978
            }
        );
        // Shared memory, or no graphics card: RAM only, as before.
        let shared = Room {
            unified: true,
            ..card
        };
        assert_eq!(draw_need(model, Some(&shared)), Mem::ram(7_680));
        assert_eq!(draw_need(model, None), Mem::ram(7_680));
        // A 24B chat model fills the card: it unloads first.
        let free = Mem {
            ram_mb: 38_400,
            vram_mb: 2_048,
        };
        let chat = Entry {
            kind: Kind::Chat,
            mem: Mem {
                ram_mb: 1_000,
                vram_mb: 20_000,
            },
            last_used_secs: 1,
            pinned: false,
        };
        assert_eq!(
            admit(need, free, &[chat], false),
            Admit::Evict(vec![Kind::Chat])
        );
        // One answering now can't go: the picture is refused, not started.
        let busy = Entry {
            pinned: true,
            ..chat
        };
        assert_eq!(
            admit(need, free, &[busy], false),
            Admit::Decline { need_mb: 12_658 }
        );
    }

    #[test]
    fn a_picture_without_room_says_how_much_memory_it_needs() {
        use super::super::local_resident::Mem;
        // 7 GB picture model plus 512 MB to spare.
        assert_eq!(
            no_room_line(Mem::ram(7_168 + DRAW_HEADROOM_MB)),
            "Not made, and nothing was sent: drawing needs about 8 GB of free memory and this computer doesn't have it right now. Close some apps, then ask again."
        );
        assert_eq!(
            no_room_line(Mem {
                ram_mb: 7_680,
                vram_mb: 4_980
            }),
            "Not made, and nothing was sent: drawing needs about 8 GB of free memory and 5 GB of free graphics memory and this computer doesn't have it right now. Close some apps, then ask again."
        );
    }

    #[test]
    fn only_an_unanswered_picture_ask_this_computer_can_draw_forces_the_step() {
        use crate::client::{ContentPart, InputItem, ResponsesRequest};
        let msg = |role: &str, t: &str| InputItem::Message {
            role: role.into(),
            content: vec![ContentPart::InputText(t.into())],
        };
        let req = ResponsesRequest {
            model: "local:small".into(),
            effort: None,
            input: vec![msg("system", "s"), msg("user", "draw me a fox")],
            conversation_id: "c".into(),
            tools: vec![
                json!({"name": TOOL, "description": "Generate images."}),
                json!({"name": "web_search", "description": "Search."}),
            ],
            hosted_search: false,
            call_timeout: None,
            text_format: None,
        };
        assert!(picture_step(&req, true, true));
        assert!(!picture_step(&req, false, true), "nothing here can draw");
        assert!(!picture_step(&req, true, false), "not a picture ask");
        let narrow = only_picture_tool(&req);
        assert_eq!(
            narrow.tools,
            vec![json!({"name": TOOL, "description": "Generate images."})]
        );
        assert_eq!(narrow.input, req.input);
        // The picture came back: the next step answers in words.
        let mut answered = req.clone();
        answered.input.push(InputItem::FunctionCall {
            call_id: "p1".into(),
            name: TOOL.into(),
            arguments: "{}".into(),
        });
        answered.input.push(InputItem::FunctionCallOutput {
            call_id: "p1".into(),
            output: "IMAGINE: /tmp/a.png".into(),
        });
        assert!(!picture_step(&answered, true, true));
        // The gate didn't offer the tool.
        let mut bare = req.clone();
        bare.tools.retain(|t| t["name"] != TOOL);
        assert!(!picture_step(&bare, true, true));
        assert!(!forced());
        {
            let _f = ForceGuard::set();
            assert!(forced());
        }
        assert!(!forced());
    }
}
