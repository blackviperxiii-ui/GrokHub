//! Push to talk and voice-mode speech-to-text: uses the shared voice router
//! ([`grokhub_agent::route::voice_route`]) to pick between xAI (Grok Voice 3
//! over OAuth sign-in or API key) and on-device engines (`sherpa-onnx`
//! Parakeet or `whisper` on PATH).
//!
//! Online calls go through the egress guard (chat data), so Offline only
//! refuses them with the core's note before anything leaves.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use grokhub_agent::harness as hx;
use grokhub_agent::route::local_resident::{reserve, Kind, Mem};
use grokhub_agent::route::voice_route::{
    self, SpeechSource, StageKind, StagePlan, VoiceEnv, VoiceRouteMode,
};
use grokhub_agent::route::{connectivity, host, local};
use grokhub_core::runtime_download as rd;
use grokhub_core::voice::{parse_stt_text, pcm_from_capture, stt_multipart, stt_url};
use grokhub_core::voice_engine as ve;

/// Points transcribe at a local mock STT server, for tests only. Only a
/// plain-http loopback URL counts; anything else is ignored.
pub const TEST_URL_ENV: &str = "GROKHUB_SHELL_STT_URL";
/// Catalog id for the on-device speech-to-text model auto-downloaded when needed.
pub const STT_MODEL_ID: &str = "parakeet-tdt-0.6b-v3-int8";
/// A recording past this is refused before anything is sent.
pub const WAV_MAX: usize = 10 * 1024 * 1024;
/// Push to talk takes at most this long a recording.
pub const SECONDS_MAX: u64 = 60;
/// The answer is a short JSON object; more than this is not a transcript.
const ANSWER_MAX: u64 = 1024 * 1024;
const TIMEOUT: Duration = Duration::from_secs(60);
const BOUNDARY: &str = "----grokhubstt";

pub const NO_KEY: &str =
    "Voice needs Grok sign-in or an xAI API key in Settings → Account, or Local AI enabled in Settings → Local AI.";
pub const TOO_LONG: &str = "That recording is over 60 seconds. Hold the mic for a minute or less.";
pub const TOO_BIG: &str = "That recording is too big to send.";
pub const NOT_WAV: &str = "That recording isn't a WAV file.";
pub const EMPTY: &str = "Didn't catch any words. Hold the mic and speak, then let go.";

/// Where the recording goes and the bearer it carries.
pub struct Target {
    pub url: String,
    pub bearer: String,
}

/// The loopback test URL when `raw` is one: `http://127.0.0.1`,
/// `http://localhost` or `http://[::1]`, any port and path. Anything else,
/// HTTPS included, is ignored, so the setting can't move real audio.
pub fn loopback_override(raw: Option<&str>) -> Option<String> {
    let raw = raw?.trim();
    let rest = raw.strip_prefix("http://")?;
    let host_port = rest.split('/').next().unwrap_or("");
    if host_port.contains('@') {
        return None;
    }
    let host = if let Some(v6) = host_port.strip_prefix('[') {
        v6.split(']').next().unwrap_or("")
    } else {
        host_port.split(':').next().unwrap_or("")
    };
    matches!(host, "127.0.0.1" | "localhost" | "::1").then(|| raw.to_string())
}

/// The real endpoint with the saved credential, or the loopback mock in tests.
fn target() -> Result<Target, String> {
    if let Some(url) = loopback_override(std::env::var(TEST_URL_ENV).ok().as_deref()) {
        return Ok(Target {
            url,
            bearer: crate::credential::LOOPBACK_TEST_BEARER.into(),
        });
    }
    Ok(Target {
        url: stt_url(),
        bearer: stt_bearer()?,
    })
}

/// Picks the speech credential: a live Grok sign-in wins over a saved xAI API
/// key; an ended sign-in falls back to the API key when present.
pub(crate) fn pick_bearer(
    oauth: Result<Option<String>, String>,
    key: impl FnOnce() -> Result<String, String>,
) -> Result<String, String> {
    match oauth {
        Ok(Some(token)) => Ok(token),
        Ok(None) => key().map_err(|_| NO_KEY.to_string()),
        Err(ended) => key().map_err(|_| ended),
    }
}

/// The credential speech-to-text carries, upload and stream alike: Grok
/// sign-in first, else the saved xAI API key.
pub fn stt_bearer() -> Result<String, String> {
    pick_bearer(
        crate::oauth::bearer(&crate::oauth::Keychain),
        crate::credential::load_key,
    )
}

pub(crate) fn models_dir(config_dir: &Path) -> PathBuf {
    crate::local_ai::models_dir().unwrap_or_else(|| config_dir.join("models"))
}

pub(crate) fn local_ai_capable() -> bool {
    rd::platform_key().is_some_and(|k| ve::asset_for(k).is_some())
}

/// Finds `whisper-cli` or `whisper` on `PATH` for platforms without a built-in
/// `sherpa-onnx` binary (e.g. macOS).
pub fn whisper_path() -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        for bin in ["whisper-cli", "whisper"] {
            let name = format!("{bin}{}", std::env::consts::EXE_SUFFIX);
            let candidate = dir.join(name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

/// Whether on-device speech-to-text (`sherpa-onnx` + catalog model or
/// `whisper` on PATH) is ready right now.
pub fn local_stt_ready(config_dir: &Path, models: &Path) -> bool {
    (ve::installed(config_dir) && ve::speech_model(models).is_some()) || whisper_path().is_some()
}

pub(crate) fn stt_env(config_dir: &Path, models: &Path) -> VoiceEnv {
    host::sync(config_dir);
    connectivity::sync_from_core();
    VoiceEnv {
        loopback_override: loopback_override(std::env::var(TEST_URL_ENV).ok().as_deref()).is_some(),
        offline_only: grokhub_core::offline::enabled(),
        net: connectivity::state(),
        local_enabled: local::switched_on(),
        local_ai_capable: local_ai_capable(),
        local_ready: local_stt_ready(config_dir, models),
        local_warm: local::warm_runtime().is_some(),
        has_xai_auth: stt_bearer().is_ok(),
        xai_recently_failed: voice_route::xai_in_cooldown(StageKind::Stt, grokhub_core::now_ms()),
    }
}

/// Seconds of sound in a PCM WAV, from its header. `None` when it isn't one.
pub fn wav_seconds(wav: &[u8]) -> Option<f64> {
    if wav.len() < 44 || &wav[0..4] != b"RIFF" || &wav[8..12] != b"WAVE" {
        return None;
    }
    let byte_rate = u32::from_le_bytes(wav.get(28..32)?.try_into().ok()?);
    if byte_rate == 0 {
        return None;
    }
    Some(pcm_from_capture(wav).len() as f64 / f64::from(byte_rate))
}

/// The size and length rules, before any request.
pub fn check(wav: &[u8]) -> Result<(), String> {
    if wav.len() > WAV_MAX {
        return Err(TOO_BIG.into());
    }
    let seconds = wav_seconds(wav).ok_or_else(|| NOT_WAV.to_string())?;
    if seconds > SECONDS_MAX as f64 + 0.5 {
        return Err(TOO_LONG.into());
    }
    if seconds <= 0.0 {
        return Err(EMPTY.into());
    }
    Ok(())
}

/// Turns one push-to-talk recording into text without an app handle.
pub fn transcribe(config_dir: &Path, wav: &[u8]) -> Result<String, String> {
    transcribe_with(None, config_dir, wav)
}

/// Turns one recording into text via the shared voice router, starting automatic
/// local model download via `app` if local assets are needed and missing.
pub fn transcribe_with(
    app: Option<&tauri::AppHandle>,
    config_dir: &Path,
    wav: &[u8],
) -> Result<String, String> {
    check(wav)?;
    let cfg = voice_route::read(config_dir);
    let models = models_dir(config_dir);
    let env = stt_env(config_dir, &models);
    match voice_route::plan_stage(&cfg, StageKind::Stt, &env) {
        StagePlan::Blocked(msg) => Err(msg),
        StagePlan::DownloadLocal => {
            if let Some(app) = app {
                let _ = crate::model_library::download(
                    app.clone(),
                    config_dir.to_path_buf(),
                    vec![STT_MODEL_ID.to_string()],
                );
            }
            Err("Downloading the on-device speech model (Parakeet). Try again in a moment.".into())
        }
        StagePlan::Try(sources) => {
            let mut last_err: Option<String> = None;
            for (idx, source) in sources.iter().enumerate() {
                match source {
                    SpeechSource::GrokVoice3 => match target()
                        .and_then(|to| transcribe_at(config_dir, wav, &to))
                    {
                        Ok(text) => return Ok(text),
                        Err(err) if err == EMPTY => return Err(err),
                        Err(err) => {
                            if !env.loopback_override && err != *grokhub_core::offline::OFFLINE_MSG
                            {
                                voice_route::note_xai_failure(
                                    StageKind::Stt,
                                    grokhub_core::now_ms(),
                                );
                            }
                            let has_local_after = sources[idx + 1..].contains(&SpeechSource::Local);
                            if !has_local_after
                                && cfg.mode != VoiceRouteMode::Pinned
                                && !env.local_ready
                                && !env.offline_only
                                && !env.net.prefer_local()
                                && env.local_enabled
                                && env.local_ai_capable
                                && !env.loopback_override
                                && let Some(app) = app
                            {
                                let _ = crate::model_library::download(
                                    app.clone(),
                                    config_dir.to_path_buf(),
                                    vec![STT_MODEL_ID.to_string()],
                                );
                            }
                            last_err = Some(err);
                        }
                    },
                    SpeechSource::Local => match transcribe_on_device(config_dir, &models, wav) {
                        Some(Ok(text)) => return Ok(text),
                        Some(Err(err)) if err == EMPTY => return Err(err),
                        Some(Err(err)) => last_err = Some(err),
                        None => {}
                    },
                }
            }
            Err(last_err.unwrap_or_else(|| NO_KEY.to_string()))
        }
    }
}

fn no_room_to_listen(mb: u64) -> String {
    format!(
        "The on-device speech engine needs about {} GB of free memory, and this computer doesn't have it right now.",
        mb.div_ceil(1024)
    )
}

fn run_capture(mut cmd: Command, limit: Duration) -> Result<String, String> {
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    let mut child = cmd
        .spawn()
        .map_err(|_| "The on-device speech engine didn't start.".to_string())?;
    let out_pipe = child.stdout.take();
    let err_pipe = child.stderr.take();
    let out_reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(p) = out_pipe {
            let _ = p.take(ANSWER_MAX).read_to_end(&mut buf);
        }
        buf
    });
    let err_reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(p) = err_pipe {
            let _ = p.take(ANSWER_MAX).read_to_end(&mut buf);
        }
        buf
    });
    let by = Instant::now() + limit;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < by => std::thread::sleep(Duration::from_millis(50)),
            Ok(None) | Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("The on-device speech engine timed out.".into());
            }
        }
    };
    let out_bytes = out_reader.join().unwrap_or_default();
    let err_bytes = err_reader.join().unwrap_or_default();
    if !status.success() {
        return Err("The on-device speech engine couldn't transcribe that.".into());
    }
    Ok(format!(
        "{}\n{}",
        String::from_utf8_lossy(&out_bytes),
        String::from_utf8_lossy(&err_bytes)
    ))
}

fn transcribe_on_device(
    config_dir: &Path,
    models: &Path,
    wav: &[u8],
) -> Option<Result<String, String>> {
    if ve::installed(config_dir) {
        if let Some(model) = ve::speech_model(models) {
            let Ok(_listening) = reserve(Kind::Speech, Mem::ram(model.run_mb), false) else {
                return Some(Err(no_room_to_listen(model.run_mb)));
            };
            let wav_path = match ve::claim_speak_file(&std::env::temp_dir()) {
                Ok(p) => p,
                Err(_) => {
                    return Some(Err(
                        "The on-device speech engine couldn't write a recording.".into(),
                    ))
                }
            };
            if std::fs::write(&wav_path, wav).is_err() {
                let _ = std::fs::remove_file(&wav_path);
                return Some(Err(
                    "The on-device speech engine couldn't write a recording.".into(),
                ));
            }
            let Some(args) = ve::transcribe_args(models, model, &wav_path) else {
                let _ = std::fs::remove_file(&wav_path);
                return None;
            };
            let mut cmd = Command::new(ve::offline_path(config_dir));
            cmd.args(&args);
            let done = run_capture(cmd, TIMEOUT);
            let _ = std::fs::remove_file(&wav_path);
            return Some(
                done.and_then(|out| ve::transcript_from(&out).ok_or_else(|| EMPTY.to_string())),
            );
        }
    }
    let whisper = whisper_path()?;
    let wav_path = match ve::claim_speak_file(&std::env::temp_dir()) {
        Ok(p) => p,
        Err(_) => {
            return Some(Err(
                "The on-device speech engine couldn't write a recording.".into(),
            ))
        }
    };
    if std::fs::write(&wav_path, wav).is_err() {
        let _ = std::fs::remove_file(&wav_path);
        return Some(Err(
            "The on-device speech engine couldn't write a recording.".into(),
        ));
    }
    let mut cmd = Command::new(whisper);
    cmd.arg("-f").arg(&wav_path).arg("-nt");
    let done = run_capture(cmd, TIMEOUT);
    let _ = std::fs::remove_file(&wav_path);
    Some(done.and_then(|out| {
        let text = out.trim().to_string();
        if text.is_empty() {
            Err(EMPTY.to_string())
        } else {
            Ok(text)
        }
    }))
}

/// [`transcribe`] against `to`: the egress guard, then one POST.
pub fn transcribe_at(config_dir: &Path, wav: &[u8], to: &Target) -> Result<String, String> {
    check(wav)?;
    hx::guard_quiet(
        config_dir,
        &hx::EgressReq::new(&to.url, &[hx::DataClass::Chat]),
    )?;
    let body = stt_multipart(wav, "voice.wav", BOUNDARY);
    let agent = grokhub_core::http::agent(grokhub_core::http::Opts {
        overall: Some(TIMEOUT),
        redirects: 0,
        ..Default::default()
    });
    let resp = agent
        .post(&to.url)
        .header("authorization", &format!("Bearer {}", to.bearer))
        .header(
            "content-type",
            &format!("multipart/form-data; boundary={BOUNDARY}"),
        )
        .header("user-agent", grokhub_agent::USER_AGENT)
        .send_bytes(&body)
        .map_err(|err| match err {
            grokhub_core::http::Error::Status { code: 401, .. }
            | grokhub_core::http::Error::Status { code: 403, .. } => {
                "xAI didn't accept the Grok sign-in or API key. Check Settings → Account."
                    .to_string()
            }
            grokhub_core::http::Error::Status { code, .. } => {
                format!("Speech to text failed (HTTP {code}).")
            }
            grokhub_core::http::Error::Transport { .. } => {
                "Couldn't reach xAI for speech to text.".to_string()
            }
        })?;
    let mut raw = Vec::new();
    resp.into_reader()
        .take(ANSWER_MAX)
        .read_to_end(&mut raw)
        .map_err(|_| "Couldn't read the transcript.".to_string())?;
    let value: serde_json::Value =
        serde_json::from_slice(&raw).map_err(|_| "Couldn't read the transcript.".to_string())?;
    parse_stt_text(&value).ok_or_else(|| EMPTY.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::net::TcpListener;
    use std::path::PathBuf;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "grokhub-shell-voice-{name}-{}-{}",
            std::process::id(),
            grokhub_core::now_ms()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    /// A 16 kHz mono 16-bit WAV of `samples` silent samples.
    fn wav(samples: usize) -> Vec<u8> {
        let data = (samples * 2) as u32;
        let mut out = Vec::new();
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&(36 + data).to_le_bytes());
        out.extend_from_slice(b"WAVEfmt ");
        out.extend_from_slice(&16u32.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&16_000u32.to_le_bytes());
        out.extend_from_slice(&32_000u32.to_le_bytes());
        out.extend_from_slice(&2u16.to_le_bytes());
        out.extend_from_slice(&16u16.to_le_bytes());
        out.extend_from_slice(b"data");
        out.extend_from_slice(&data.to_le_bytes());
        out.resize(44 + samples * 2, 0);
        out
    }

    /// A mock STT server on loopback: answers one request with `answer` and
    /// hands back the raw request.
    fn mock_stt(answer: &'static str) -> (String, std::thread::JoinHandle<Vec<u8>>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let handle = std::thread::spawn(move || {
            let (mut sock, _) = listener.accept().expect("accept");
            let mut got = Vec::new();
            let mut byte = [0u8; 1];
            while !got.ends_with(b"\r\n\r\n") && sock.read(&mut byte).unwrap_or(0) == 1 {
                got.push(byte[0]);
            }
            let head = String::from_utf8_lossy(&got).to_ascii_lowercase();
            let len = head
                .lines()
                .find_map(|l| l.strip_prefix("content-length:"))
                .map_or(0, |n| n.trim().parse::<usize>().expect("length"));
            let mut body = vec![0u8; len];
            sock.read_exact(&mut body).expect("body");
            got.extend_from_slice(&body);
            let reply = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{answer}",
                answer.len()
            );
            sock.write_all(reply.as_bytes()).expect("reply");
            got
        });
        (format!("http://127.0.0.1:{port}/v1/stt"), handle)
    }

    fn at(url: &str) -> Target {
        Target {
            url: url.to_string(),
            bearer: "sk-abcdefghijklmnopqrstuv".into(),
        }
    }

    #[test]
    fn a_wav_comes_back_as_its_words() {
        let (url, server) = mock_stt(r#"{"text":"  open the build log "}"#);
        let config = temp_dir("words");
        let recording = wav(16_000);
        assert_eq!(
            transcribe_at(&config, &recording, &at(&url)),
            Ok("open the build log".to_string())
        );
        let req = server.join().expect("server");
        let text = String::from_utf8_lossy(&req);
        assert!(text.starts_with("POST /v1/stt HTTP/1.1\r\n"), "{text}");
        assert!(
            text.contains("authorization: Bearer sk-abcdefghijklmnopqrstuv\r\n"),
            "{text}"
        );
        assert!(
            text.contains("content-type: multipart/form-data; boundary=----grokhubstt\r\n"),
            "{text}"
        );
        assert!(
            text.contains("Content-Type: audio/wav\r\n\r\nRIFF"),
            "{text}"
        );
        let body_at = req
            .windows(4)
            .position(|w| w == b"\r\n\r\n")
            .expect("head end")
            + 4;
        assert_eq!(
            &req[body_at..],
            stt_multipart(&recording, "voice.wav", BOUNDARY)
        );
    }

    #[test]
    fn an_empty_transcript_is_an_error() {
        let (url, server) = mock_stt(r#"{"text":"   "}"#);
        let config = temp_dir("empty");
        assert_eq!(
            transcribe_at(&config, &wav(1_600), &at(&url)),
            Err(EMPTY.to_string())
        );
        server.join().expect("server");
    }

    #[test]
    fn an_over_cap_recording_is_refused_before_any_request() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        listener.set_nonblocking(true).expect("nonblocking");
        let url = format!(
            "http://127.0.0.1:{}/v1/stt",
            listener.local_addr().expect("addr").port()
        );
        let config = temp_dir("cap");
        let mut big = wav(16_000);
        big.resize(WAV_MAX + 1, 0);
        assert_eq!(
            transcribe_at(&config, &big, &at(&url)),
            Err(TOO_BIG.to_string())
        );
        // 61 seconds at 16 kHz.
        assert_eq!(
            transcribe_at(&config, &wav(16_000 * 61), &at(&url)),
            Err(TOO_LONG.to_string())
        );
        assert_eq!(
            transcribe_at(
                &config,
                b"not a wav at all, just some words here ok",
                &at(&url)
            ),
            Err(NOT_WAV.to_string())
        );
        assert_eq!(
            transcribe_at(&config, &wav(0), &at(&url)),
            Err(EMPTY.to_string())
        );
        assert!(listener.accept().is_err(), "nothing may connect");
        // Exactly a minute goes.
        assert_eq!(check(&wav(16_000 * 60)), Ok(()));
    }

    #[test]
    fn offline_only_refuses_with_the_core_note() {
        let _off = grokhub_core::offline::force_for_thread(true);
        let config = temp_dir("offline");
        assert_eq!(
            transcribe_at(&config, &wav(1_600), &at(&stt_url())),
            Err(grokhub_core::offline::OFFLINE_MSG.to_string())
        );
    }

    #[test]
    fn only_a_plain_http_loopback_override_counts() {
        for ok in [
            "http://127.0.0.1:4611/v1/stt",
            "http://localhost:9/stt",
            "http://[::1]:9/stt",
        ] {
            assert_eq!(loopback_override(Some(ok)), Some(ok.to_string()), "{ok}");
        }
        for ignored in [
            "https://api.x.ai/v1/stt",
            "https://127.0.0.1:9/stt",
            "http://10.0.0.5/stt",
            "http://api.x.ai/v1/stt",
            "http://localhost.example.com/stt",
            "http://127.0.0.1.example.com/stt",
            "http://user@127.0.0.1:9/stt",
            "",
        ] {
            assert_eq!(loopback_override(Some(ignored)), None, "{ignored}");
        }
        assert_eq!(loopback_override(None), None);
    }

    #[test]
    fn the_length_comes_from_the_header() {
        assert_eq!(wav_seconds(&wav(8_000)), Some(0.5));
        assert_eq!(wav_seconds(b"RIFF"), None);
    }

    #[test]
    fn pick_bearer_prefers_grok_sign_in_and_falls_back_to_api_key() {
        assert_eq!(
            pick_bearer(Ok(Some("oauth-tok".into())), || Ok("sk-key".into())),
            Ok("oauth-tok".into())
        );
        assert_eq!(
            pick_bearer(Ok(None), || Ok("sk-key".into())),
            Ok("sk-key".into())
        );
        assert_eq!(
            pick_bearer(Err("Sign-in ended".into()), || Ok("sk-key".into())),
            Ok("sk-key".into())
        );
        assert_eq!(
            pick_bearer(Ok(None), || Err("none".into())),
            Err(NO_KEY.to_string())
        );
        assert_eq!(
            pick_bearer(Err("Sign-in ended".into()), || Err("none".into())),
            Err("Sign-in ended".into())
        );
    }

    #[test]
    fn offline_local_only_transcribe_blocks_without_touching_network() {
        let _off = grokhub_core::offline::force_for_thread(true);
        let config = temp_dir("local-only-off");
        voice_route::write(
            &config,
            voice_route::VoiceRouteConfig {
                mode: VoiceRouteMode::LocalOnly,
                ..Default::default()
            },
        )
        .expect("write route");
        let err = transcribe(&config, &wav(1_600)).expect_err("blocked offline without assets");
        assert!(
            err.contains("Offline") || err.contains("speech model"),
            "{err}"
        );
    }
}
