//! Voice mode: one open mic for the whole session and spoken replies.
//!
//! The page keeps the mic open and sends 16 kHz mono blocks here; the core's
//! [`OpenMic`] finds each turn and the turn becomes words with xAI
//! speech-to-text ([`crate::voice::transcribe`]). The reply's text streams
//! back through [`ReplyVoice`], which cuts it into pieces; each piece is
//! spoken with xAI speech in the chat's voice, or with Kokoro on this PC when
//! there is no API key and the on-device voice is downloaded. The page plays
//! the pieces in order and stops them when someone talks over them.

use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use grokhub_agent::harness as hx;
use grokhub_core::voice_engine as ve;
use grokhub_core::voice_live::{Heard, OpenMic, ReplyVoice};
use grokhub_core::voice_turn::{s16le, wav, RATE};
use serde::Serialize;

/// Points speak at a local mock TTS server, for tests only (same loopback
/// rule as speech-to-text).
pub const TEST_TTS_ENV: &str = "GROKHUB_SHELL_TTS_URL";
/// One block from the page: at most this many bytes of samples (a second).
pub const BLOCK_MAX: usize = RATE as usize * 2;
/// Spoken audio past this is not a piece of speech.
const AUDIO_MAX: u64 = 16 * 1024 * 1024;
const TTS_TIMEOUT: Duration = Duration::from_secs(30);
/// The on-device voice gets this long per piece.
const ON_DEVICE_TIMEOUT: Duration = Duration::from_secs(30);

pub const NOT_ON: &str = "Voice mode isn't on.";
pub const NO_VOICE: &str =
    "Spoken replies need an xAI API key (Settings → Account) or the on-device voice.";
pub const BAD_BLOCK: &str = "That audio block isn't 16-bit samples.";

/// What the open mic heard, for the page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LiveEvent {
    /// Someone started talking (over a reply: stop it).
    Started,
    /// A turn ended; `voice_live_hear` turns it into words.
    Turn { ms: u32 },
    /// A tap before any words.
    Nothing,
    /// Quiet for a while; still listening.
    CheckIn,
    /// Quiet for a minute; voice mode pauses.
    Pause,
}

/// One voice session.
#[derive(Default)]
pub struct Live {
    mic: OpenMic,
    reply: Option<ReplyVoice>,
    /// The last turn heard, waiting to be turned into words.
    turn: Option<Vec<i16>>,
}

impl Live {
    /// A session listening for its first turn.
    pub fn start() -> Self {
        let mut live = Self::default();
        live.mic.listen();
        live
    }

    /// One block of 16 kHz mono s16le from the page.
    pub fn push(&mut self, bytes: &[u8]) -> Result<Vec<LiveEvent>, String> {
        if bytes.len() > BLOCK_MAX || bytes.len() % 2 != 0 {
            return Err(BAD_BLOCK.into());
        }
        let heard = self.mic.push(&s16le(bytes));
        Ok(heard
            .into_iter()
            .map(|h| match h {
                Heard::Started => LiveEvent::Started,
                Heard::Turn(turn) => {
                    let ms = (turn.len() as u64 * 1000 / u64::from(RATE)) as u32;
                    self.turn = Some(turn);
                    LiveEvent::Turn { ms }
                }
                Heard::Nothing => LiveEvent::Nothing,
                Heard::CheckIn => LiveEvent::CheckIn,
                Heard::Pause => LiveEvent::Pause,
            })
            .collect())
    }

    /// Listen for the next turn; `playing` while a reply still plays, so only
    /// talking over it starts one.
    pub fn listen(&mut self, playing: bool) {
        self.mic.set_playing(playing);
        self.mic.listen();
    }

    /// The reply stopped or finished playing.
    pub fn set_playing(&mut self, playing: bool) {
        self.mic.set_playing(playing);
    }

    /// End the turn now (a tap while they talk).
    pub fn finish(&mut self) {
        self.mic.finish();
    }

    /// The last turn as a WAV, once.
    pub fn take_turn(&mut self) -> Option<Vec<u8>> {
        self.turn.take().map(|t| wav(&t, RATE))
    }

    /// More of the reply; the pieces ready to speak. A new reply starts on
    /// its first text.
    pub fn reply_feed(&mut self, delta: &str) -> Vec<String> {
        self.reply.get_or_insert_with(ReplyVoice::new).feed(delta)
    }

    /// The reply ended: the rest to speak. `whole` when nothing streamed.
    pub fn reply_finish(&mut self, whole: &str) -> Vec<String> {
        match self.reply.take() {
            Some(mut r) => r.finish(whole),
            None => ReplyVoice::new().finish(&grokhub_core::voice_tts_script(whole)),
        }
    }

    /// Say no more of this reply.
    pub fn reply_hush(&mut self) {
        if let Some(r) = self.reply.as_mut() {
            r.hush();
        }
    }
}

static SESSION: Mutex<Option<Live>> = Mutex::new(None);

/// Runs `f` on the session, or says voice mode is off.
fn with<T>(f: impl FnOnce(&mut Live) -> T) -> Result<T, String> {
    let mut guard = SESSION.lock().unwrap_or_else(|e| e.into_inner());
    guard.as_mut().map(f).ok_or_else(|| NOT_ON.to_string())
}

/// A piece of speech for the page to play.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Spoken {
    /// `audio/mpeg` from xAI, `audio/wav` from the on-device voice.
    pub mime: &'static str,
    /// Base64 of the audio.
    pub audio: String,
}

/// Where speech goes and the bearer it carries.
struct TtsTarget {
    url: String,
    bearer: String,
}

/// The loopback mock in tests, else xAI with the saved key.
fn tts_target() -> Option<TtsTarget> {
    let test = std::env::var(TEST_TTS_ENV).ok();
    if let Some(url) = crate::voice::loopback_override(test.as_deref()) {
        return Some(TtsTarget {
            url,
            bearer: crate::credential::LOOPBACK_TEST_BEARER.into(),
        });
    }
    let bearer = crate::credential::load_key().ok()?;
    Some(TtsTarget {
        url: grokhub_core::tts_url(),
        bearer,
    })
}

/// Speak `text` in `voice`: xAI with a key, else Kokoro on this PC.
pub fn speak(
    config_dir: &Path,
    models_dir: &Path,
    voice: &str,
    text: &str,
) -> Result<Spoken, String> {
    let text = text.trim();
    if text.is_empty() {
        return Err("Nothing to speak.".into());
    }
    if let Some(to) = tts_target() {
        return speak_xai(config_dir, &to, voice, text);
    }
    match speak_on_device(config_dir, models_dir, voice, text) {
        Some(done) => done,
        None => Err(NO_VOICE.into()),
    }
}

fn speak_xai(config_dir: &Path, to: &TtsTarget, voice: &str, text: &str) -> Result<Spoken, String> {
    use base64::Engine as _;
    hx::guard_quiet(
        config_dir,
        &hx::EgressReq::new(&to.url, &[hx::DataClass::Chat]),
    )?;
    let body = serde_json::to_vec(&grokhub_core::tts_request_body(text, voice))
        .map_err(|e| e.to_string())?;
    let agent = grokhub_core::http::agent(grokhub_core::http::Opts {
        overall: Some(TTS_TIMEOUT),
        redirects: 0,
        ..Default::default()
    });
    let resp = agent
        .post(&to.url)
        .header("authorization", &format!("Bearer {}", to.bearer))
        .header("content-type", "application/json")
        .header("user-agent", grokhub_agent::USER_AGENT)
        .send_bytes(&body)
        .map_err(|err| match err {
            grokhub_core::http::Error::Status { code: 401, .. }
            | grokhub_core::http::Error::Status { code: 403, .. } => {
                "xAI didn't accept the API key. Check it in Settings → Account.".to_string()
            }
            grokhub_core::http::Error::Status { code, .. } => {
                format!("Text to speech failed (HTTP {code}).")
            }
            grokhub_core::http::Error::Transport { .. } => {
                "Couldn't reach xAI for text to speech.".to_string()
            }
        })?;
    let mut audio = Vec::new();
    resp.into_reader()
        .take(AUDIO_MAX + 1)
        .read_to_end(&mut audio)
        .map_err(|_| "Couldn't read the spoken reply.".to_string())?;
    if audio.len() as u64 > AUDIO_MAX {
        return Err("The spoken reply was too large.".into());
    }
    if audio.len() < 32 {
        return Err("xAI sent back no speech.".into());
    }
    Ok(Spoken {
        mime: "audio/mpeg",
        audio: base64::engine::general_purpose::STANDARD.encode(audio),
    })
}

/// Kokoro in the on-device speech engine. None when the engine or a voice
/// isn't downloaded.
fn speak_on_device(
    config_dir: &Path,
    models_dir: &Path,
    voice: &str,
    text: &str,
) -> Option<Result<Spoken, String>> {
    use base64::Engine as _;
    if !ve::tts_installed(config_dir) {
        return None;
    }
    let model = ve::voice_model(models_dir)?;
    let out = std::env::temp_dir().join(format!(
        "grokhub-speak-{}-{}.wav",
        std::process::id(),
        grokhub_core::now_ms()
    ));
    let args = ve::speak_args(models_dir, model, voice, text, &out)?;
    let mut cmd = Command::new(ve::tts_path(config_dir));
    cmd.args(&args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // CREATE_NO_WINDOW: no console flashes up per piece.
        cmd.creation_flags(0x0800_0000);
    }
    let done = run_for(cmd, ON_DEVICE_TIMEOUT).and_then(|()| {
        std::fs::read(&out).map_err(|_| "The on-device voice wrote no audio.".to_string())
    });
    let _ = std::fs::remove_file(&out);
    Some(done.map(|bytes| Spoken {
        mime: "audio/wav",
        audio: base64::engine::general_purpose::STANDARD.encode(bytes),
    }))
}

/// Runs `cmd` to the end, killing it past `limit`.
fn run_for(mut cmd: Command, limit: Duration) -> Result<(), String> {
    let mut child = cmd
        .spawn()
        .map_err(|_| "The on-device voice didn't start.".to_string())?;
    let by = Instant::now() + limit;
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(()),
            Ok(Some(_)) => return Err("The on-device voice couldn't speak that.".into()),
            Ok(None) if Instant::now() < by => std::thread::sleep(Duration::from_millis(50)),
            Ok(None) | Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("The on-device voice timed out.".into());
            }
        }
    }
}

/// Words for the last turn; empty when nothing worth sending was said.
fn hear(config_dir: &Path, turn: Option<Vec<u8>>) -> Result<String, String> {
    let Some(wav) = turn else {
        return Ok(String::new());
    };
    match crate::voice::transcribe(config_dir, &wav) {
        Ok(words) => Ok(words),
        // A cough or a door: listen again.
        Err(e) if e == crate::voice::EMPTY => Ok(String::new()),
        Err(e) => Err(e),
    }
}

fn models_dir(config_dir: &Path) -> std::path::PathBuf {
    crate::local_ai::models_dir().unwrap_or_else(|| config_dir.join("models"))
}

/// Voice mode on: the mic is listening for the first turn.
#[tauri::command]
pub(crate) fn voice_live_start() -> Result<(), String> {
    crate::capabilities::needs(crate::capabilities::current().voice, "Voice")?;
    *SESSION.lock().unwrap_or_else(|e| e.into_inner()) = Some(Live::start());
    Ok(())
}

/// Voice mode off: any turn under way is dropped.
#[tauri::command]
pub(crate) fn voice_live_stop() {
    SESSION.lock().unwrap_or_else(|e| e.into_inner()).take();
}

/// One block of the open mic (base64 of 16 kHz mono s16le).
#[tauri::command]
pub(crate) fn voice_live_push(block: String) -> Result<Vec<LiveEvent>, String> {
    use base64::Engine as _;
    if block.len() / 4 * 3 > BLOCK_MAX + 3 {
        return Err(BAD_BLOCK.into());
    }
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(block.trim())
        .map_err(|_| BAD_BLOCK.to_string())?;
    with(|live| live.push(&bytes))?
}

#[tauri::command]
pub(crate) fn voice_live_listen(playing: bool) -> Result<(), String> {
    with(|live| live.listen(playing))
}

#[tauri::command]
pub(crate) fn voice_live_playing(playing: bool) -> Result<(), String> {
    with(|live| live.set_playing(playing))
}

#[tauri::command]
pub(crate) fn voice_live_finish() -> Result<(), String> {
    with(Live::finish)
}

/// The last turn's words (empty: listen again).
#[tauri::command]
pub(crate) async fn voice_live_hear(
    state: tauri::State<'_, crate::ShellState>,
) -> Result<String, String> {
    let turn = with(Live::take_turn)?;
    let dir = state.config_dir.clone();
    crate::off_main(move || hear(&dir, turn)).await
}

#[tauri::command]
pub(crate) fn voice_reply_feed(delta: String) -> Result<Vec<String>, String> {
    with(|live| live.reply_feed(&delta))
}

#[tauri::command]
pub(crate) fn voice_reply_finish(whole: String) -> Result<Vec<String>, String> {
    with(|live| live.reply_finish(&whole))
}

#[tauri::command]
pub(crate) fn voice_reply_hush() -> Result<(), String> {
    with(Live::reply_hush)
}

/// One piece of a reply as speech, in the chat's voice.
#[tauri::command]
pub(crate) async fn voice_speak(
    state: tauri::State<'_, crate::ShellState>,
    text: String,
) -> Result<Spoken, String> {
    crate::capabilities::needs(crate::capabilities::current().voice, "Voice")?;
    let dir = state.config_dir.clone();
    let voice = crate::personality::speech_voice(&dir, &crate::current(&state));
    crate::off_main(move || speak(&dir, &models_dir(&dir), voice, &text)).await
}

#[cfg(test)]
#[path = "voice_live_tests.rs"]
mod tests;
