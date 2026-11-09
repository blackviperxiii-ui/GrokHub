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

use super::local_sidecar as sidecar;

/// Longest one picture may take (a slow CPU-only machine still finishes).
const DRAW_TIMEOUT: Duration = Duration::from_secs(15 * 60);
/// Longest the guard may take to load and answer.
const GUARD_TIMEOUT: Duration = Duration::from_secs(3 * 60);

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

/// What's on disk for on-device pictures.
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
    if let Some(g) = kit.guard {
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
}
