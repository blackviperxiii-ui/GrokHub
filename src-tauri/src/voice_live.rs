//! Voice mode: one open mic for the whole session and spoken replies.
//!
//! The page keeps the mic open and sends 16 kHz mono blocks here; the core's
//! [`OpenMic`] finds each turn. While they talk, each block also goes to
//! xAI's streaming speech-to-text ([`crate::stt_stream`]): its Smart Turn
//! can end a clearly finished turn before the mic's silence does, and when
//! the mic ends it the words are usually already there. A turn the stream
//! can't hear goes up once as a recording ([`crate::voice::transcribe`]). The reply's text streams
//! back through [`ReplyVoice`], which cuts it into pieces; each piece is
//! spoken with xAI speech in the chat's voice, or with Kokoro on this PC when
//! there is no API key and the on-device voice is downloaded. The page plays
//! the pieces in order and stops them when someone talks over them.

use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc::{Receiver, TryRecvError};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use grokhub_agent::harness as hx;
use grokhub_agent::route::voice_route::{
    self, SpeechSource, StageKind, StagePlan, VoiceEnv, VoiceRouteConfig, VoiceRouteMode,
};
use grokhub_core::voice_engine as ve;
use grokhub_core::voice_live::{Heard, OpenMic, ReplyVoice};
use grokhub_core::voice_turn::{s16le, wav, RATE};
use serde::Serialize;

use crate::stt_stream::{SttLink, SttNews};

/// Points speak at a local mock TTS server, for tests only (same loopback
/// rule as speech-to-text).
pub const TEST_TTS_ENV: &str = "GROKHUB_SHELL_TTS_URL";
/// Catalog id for the on-device voice auto-downloaded when needed.
pub const TTS_MODEL_ID: &str = "kokoro-82m-multi-lang-v1_0";
/// One block from the page: at most this many bytes of samples (a second).
pub const BLOCK_MAX: usize = RATE as usize * 2;
/// Spoken audio past this is not a piece of speech.
const AUDIO_MAX: u64 = 16 * 1024 * 1024;
const TTS_TIMEOUT: Duration = Duration::from_secs(30);
/// The on-device voice gets this long per piece.
const ON_DEVICE_TIMEOUT: Duration = Duration::from_secs(30);

pub const NOT_ON: &str = "Voice mode isn't on.";
pub const NO_VOICE: &str =
    "Spoken replies need Grok sign-in or an xAI API key in Settings → Account, or Local AI enabled in Settings → Local AI.";
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
    /// Everything heard so far in this turn, while they talk.
    Caption { text: String },
}

/// A turn waiting to be turned into words.
#[derive(Debug)]
pub enum Pending {
    /// Smart Turn ended it: its words, nothing to upload.
    Words(String),
    /// The mic ended it: the audio, and the stream's news when one ran.
    Audio {
        samples: Vec<i16>,
        news: Option<Receiver<SttNews>>,
    },
}

/// Opens a stream for one listen; `None` when it can't.
pub type Opener = Box<dyn FnMut() -> Option<SttLink> + Send>;

/// One voice session.
#[derive(Default)]
pub struct Live {
    mic: OpenMic,
    reply: Option<ReplyVoice>,
    /// The last turn heard, waiting to be turned into words.
    turn: Option<Pending>,
    /// This listen's stream, while it runs.
    stt: Option<SttLink>,
    open: Option<Opener>,
    /// Samples heard while listening, for the length of a Smart Turn turn.
    heard: usize,
    /// The page shows a caption from this listen's stream.
    captioned: bool,
    /// The mic heard someone start talking in this listen. Audio goes to
    /// the stream only from then on, and Smart Turn's words count only
    /// then: without it they are someone in the room the mic took for
    /// background.
    spoke: bool,
    /// The last [`RECENT_MS`] of blocks heard while listening. When the mic
    /// hears a start they go to the stream first, so it gets the turn's
    /// lead-in the mic keeps too, even from before the stream opened.
    recent: std::collections::VecDeque<Vec<i16>>,
    /// A reply is playing: no stream (its echo could end a turn nobody
    /// spoke).
    playing: bool,
    /// The stream closed while nobody talked and the listen still waits:
    /// open a new one.
    reopen: bool,
}

/// How much audio before a start the stream gets: the detector's pre-roll
/// plus its longest onset window (talking over the end of a reply).
const RECENT_MS: u32 =
    grokhub_core::voice_turn::PRE_ROLL_MS + grokhub_core::voice_turn::BARGE_IN_WINDOW_MS;

impl Live {
    /// A session listening for its first turn. `open` gives each listen
    /// not over a reply its stream; without it every turn is uploaded.
    pub fn new(open: Option<Opener>) -> Self {
        let mut live = Self {
            open,
            ..Self::default()
        };
        live.listen(false);
        live
    }

    /// One block of 16 kHz mono s16le from the page.
    pub fn push(&mut self, bytes: &[u8]) -> Result<Vec<LiveEvent>, String> {
        if bytes.len() > BLOCK_MAX || !bytes.len().is_multiple_of(2) {
            return Err(BAD_BLOCK.into());
        }
        let block = s16le(bytes);
        let armed = self.mic.armed();
        let heard = self.mic.push(&block);
        if armed {
            self.heard += block.len();
            self.hold_recent(block);
            let started = heard.iter().any(|h| matches!(h, Heard::Started));
            self.stream_block(started);
        }
        let mut events: Vec<LiveEvent> = heard
            .into_iter()
            .map(|h| match h {
                Heard::Started => LiveEvent::Started,
                Heard::Turn(turn) => {
                    // Dropping the stream's audio asks it for the last words.
                    // A stream the server never got ready for has none: the
                    // turn uploads at once.
                    let news = self
                        .stt
                        .take()
                        .filter(SttLink::is_ready)
                        .map(|link| link.news);
                    let ms = ms_of(turn.len());
                    self.turn = Some(Pending::Audio {
                        samples: turn,
                        news,
                    });
                    LiveEvent::Turn { ms }
                }
                Heard::Nothing => {
                    self.stt = None;
                    LiveEvent::Nothing
                }
                Heard::CheckIn => LiveEvent::CheckIn,
                Heard::Pause => {
                    self.stt = None;
                    LiveEvent::Pause
                }
            })
            .collect();
        // A start the mic took back (a hum that stayed steady is never a
        // turn): nobody is talking after all. The stream heard the hum, so it
        // goes, and the page listens again as after noise, with a new one.
        let over = events.iter().any(|e| {
            matches!(
                e,
                LiveEvent::Turn { .. } | LiveEvent::Nothing | LiveEvent::Pause
            )
        });
        if self.spoke && !over && self.mic.armed() && !self.mic.talking() {
            self.spoke = false;
            self.stt = None;
            self.captioned = false;
            events.push(LiveEvent::Nothing);
        }
        let streaming = self.stt.is_some();
        let (caption, words) = self.stream_news();
        if let Some(words) = words {
            self.mic.stop();
            self.stt = None;
            self.captioned = false;
            self.turn = Some(Pending::Words(words));
            events.push(LiveEvent::Turn {
                ms: ms_of(self.heard),
            });
        } else if streaming && self.stt.is_none() {
            // The stream went away mid-listen: its caption would stand still
            // while they talk on, so it goes.
            if std::mem::take(&mut self.captioned) {
                events.push(LiveEvent::Caption {
                    text: String::new(),
                });
            }
        } else if let Some(text) = caption.filter(|_| self.spoke) {
            // Only once the mic heard a start: before that it is someone
            // else in the room, or the TV.
            self.captioned = true;
            events.push(LiveEvent::Caption { text });
        }
        Ok(events)
    }

    fn hold_recent(&mut self, block: Vec<i16>) {
        let keep = (RATE * RECENT_MS / 1000) as usize;
        self.recent.push_back(block);
        let mut total: usize = self.recent.iter().map(Vec::len).sum();
        while self.recent.len() > 1 && total - self.recent[0].len() >= keep {
            total -= self.recent[0].len();
            self.recent.pop_front();
        }
    }

    /// Audio to the stream once someone started talking: everything held at
    /// the start, then each block. Before that, room sound stays here. A
    /// stream that already ended can't take audio; its news still waits in
    /// the channel, so the link stays until that is read.
    fn stream_block(&mut self, started: bool) {
        let first = started && !self.spoke;
        self.spoke |= started;
        let Some(link) = &self.stt else { return };
        if first {
            for block in &self.recent {
                let _ = link.audio.send(block.clone());
            }
        } else if self.spoke {
            if let Some(block) = self.recent.back() {
                let _ = link.audio.send(block.clone());
            }
        }
    }

    /// What the stream said since the last block: its newest caption, and
    /// its words when Smart Turn ended the turn. A stream that failed or
    /// went away is dropped, so the turn falls back to the upload; so is one
    /// that ended a turn the mic never heard start.
    fn stream_news(&mut self) -> (Option<String>, Option<String>) {
        let mut caption = None;
        let Some(link) = self.stt.as_ref() else {
            return (None, None);
        };
        loop {
            match link.news.try_recv() {
                Ok(SttNews::Caption(text)) => caption = Some(text),
                Ok(SttNews::Ended(words)) if !words.is_empty() && self.spoke => {
                    return (caption, Some(words))
                }
                Ok(SttNews::Ended(words)) if !words.is_empty() => {
                    self.stt = None;
                    return (caption, None);
                }
                Ok(SttNews::Ended(_)) => {}
                Ok(SttNews::Idle) => {
                    self.stt = None;
                    self.reopen = true;
                    return (caption, None);
                }
                Ok(SttNews::Failed(_)) | Err(TryRecvError::Disconnected) => {
                    self.stt = None;
                    return (caption, None);
                }
                Err(TryRecvError::Empty) => return (caption, None),
            }
        }
    }

    /// Listen for the next turn; `playing` while a reply still plays, so only
    /// talking over it starts one. A listen over a reply opens no stream: its
    /// echo could end a turn nobody spoke.
    pub fn listen(&mut self, playing: bool) {
        if self.restart(playing) {
            self.open_now();
        }
    }

    /// [`Live::listen`] without the stream: whether this listen wants one.
    fn restart(&mut self, playing: bool) -> bool {
        self.mic.set_playing(playing);
        self.mic.listen();
        self.heard = 0;
        self.captioned = false;
        self.spoke = false;
        self.recent.clear();
        self.stt = None;
        self.playing = playing;
        self.reopen = false;
        !playing
    }

    /// The reply started, stopped or finished playing: whether a stream is
    /// wanted now. Once it is over and nobody is mid-turn, the next turn gets
    /// one. A turn already under way (talking over the reply) keeps the
    /// upload: a stream opened now would miss its start.
    fn playing_changed(&mut self, playing: bool) -> bool {
        self.mic.set_playing(playing);
        self.playing = playing;
        self.reopen = false;
        if playing {
            self.stt = None;
            false
        } else if self.mic.armed() && !self.mic.talking() && self.stt.is_none() {
            self.heard = 0;
            self.spoke = false;
            self.captioned = false;
            true
        } else {
            false
        }
    }

    /// Whether this listen can take a stream now: armed, no reply playing,
    /// none open, and nobody started talking (a stream that missed the start
    /// would hear clipped words).
    fn wants_stream(&self) -> bool {
        !self.playing
            && self.stt.is_none()
            && self.mic.armed()
            && !self.mic.talking()
            && !self.spoke
    }

    /// Whether an idle stream closed under a listen that still wants one.
    fn take_reopen(&mut self) -> bool {
        std::mem::take(&mut self.reopen) && self.wants_stream()
    }

    fn open_now(&mut self) {
        self.stt = self.open.as_mut().and_then(|open| open());
    }

    /// The opener, lent out so a stream opens without the session locked
    /// (the key comes from the OS keychain).
    fn lend_opener(&mut self) -> Option<Opener> {
        self.open.take()
    }

    /// The opener back, and its stream if the listen on now still wants
    /// one. A stream nothing was sent to yet is as good for a later listen
    /// (one that asked while the opener was lent) as for the one it was
    /// opened for.
    fn take_back(&mut self, open: Opener, link: Option<SttLink>) {
        if self.open.is_none() {
            self.open = Some(open);
        }
        if self.wants_stream() {
            self.stt = link;
        }
    }

    /// End the turn now (a tap while they talk).
    pub fn finish(&mut self) {
        self.mic.finish();
    }

    /// The last turn, once.
    pub fn take_turn(&mut self) -> Option<Pending> {
        self.turn.take()
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

fn ms_of(samples: usize) -> u32 {
    (samples as u64 * 1000 / u64::from(RATE)) as u32
}

/// Runs `f` on the session, or says voice mode is off.
fn with<T>(f: impl FnOnce(&mut Live) -> T) -> Result<T, String> {
    with_in(&SESSION, f)
}

fn with_in<T>(cell: &Mutex<Option<Live>>, f: impl FnOnce(&mut Live) -> T) -> Result<T, String> {
    let mut guard = cell.lock().unwrap_or_else(|e| e.into_inner());
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

/// The loopback mock in tests, else xAI with Grok sign-in or the saved API key.
fn tts_target() -> Option<TtsTarget> {
    let test = std::env::var(TEST_TTS_ENV).ok();
    if let Some(url) = crate::voice::loopback_override(test.as_deref()) {
        return Some(TtsTarget {
            url,
            bearer: crate::credential::LOOPBACK_TEST_BEARER.into(),
        });
    }
    let bearer = crate::voice::stt_bearer().ok()?;
    Some(TtsTarget {
        url: grokhub_core::tts_url(),
        bearer,
    })
}

fn tts_env(config_dir: &Path, models: &Path) -> VoiceEnv {
    grokhub_agent::route::host::sync(config_dir);
    grokhub_agent::route::connectivity::sync_from_core();
    VoiceEnv {
        loopback_override: crate::voice::loopback_override(
            std::env::var(TEST_TTS_ENV).ok().as_deref(),
        )
        .is_some(),
        offline_only: grokhub_core::offline::enabled(),
        net: grokhub_agent::route::connectivity::state(),
        local_enabled: grokhub_agent::route::local::switched_on(),
        local_ai_capable: crate::voice::local_ai_capable(),
        local_ready: ve::tts_installed(config_dir) && ve::voice_model(models).is_some(),
        local_warm: grokhub_agent::route::local::warm_runtime().is_some(),
        has_xai_auth: crate::voice::stt_bearer().is_ok(),
        xai_recently_failed: voice_route::xai_in_cooldown(StageKind::Tts, grokhub_core::now_ms()),
    }
}

/// Speak `text` in `voice` using the shared voice router without an app handle.
pub fn speak(
    config_dir: &Path,
    models_dir: &Path,
    voice: &str,
    text: &str,
    asked: u64,
) -> Result<Spoken, String> {
    speak_with(None, config_dir, models_dir, voice, text, asked)
}

/// Speak `text` in `voice` via the shared voice router, starting automatic
/// local voice download via `app` if local assets are needed and missing.
pub fn speak_with(
    app: Option<&tauri::AppHandle>,
    config_dir: &Path,
    models_dir: &Path,
    voice: &str,
    text: &str,
    asked: u64,
) -> Result<Spoken, String> {
    let text = text.trim();
    if text.is_empty() {
        return Err("Nothing to speak.".into());
    }
    let cfg = voice_route::read(config_dir);
    let env = tts_env(config_dir, models_dir);
    speak_with_env(
        app, &cfg, &env, config_dir, models_dir, voice, text, asked, tts_target,
    )
}

#[allow(clippy::too_many_arguments)]
fn speak_with_env(
    app: Option<&tauri::AppHandle>,
    cfg: &VoiceRouteConfig,
    env: &VoiceEnv,
    config_dir: &Path,
    models_dir: &Path,
    voice: &str,
    text: &str,
    asked: u64,
    resolve_target: fn() -> Option<TtsTarget>,
) -> Result<Spoken, String> {
    match voice_route::plan_stage(cfg, StageKind::Tts, env) {
        StagePlan::Blocked(msg) => Err(msg),
        StagePlan::DownloadLocal => {
            if let Some(app) = app {
                let _ = crate::model_library::download(
                    app.clone(),
                    config_dir.to_path_buf(),
                    vec![TTS_MODEL_ID.to_string()],
                );
            }
            Err("Downloading the on-device voice (Kokoro). Spoken replies will start once it finishes.".into())
        }
        StagePlan::Try(sources) => {
            let mut last_err: Option<String> = None;
            for (idx, source) in sources.iter().enumerate() {
                match source {
                    SpeechSource::GrokVoice3 => {
                        match resolve_target()
                            .ok_or_else(|| NO_VOICE.to_string())
                            .and_then(|to| speak_xai(config_dir, &to, voice, text))
                        {
                            Ok(spoken) => return Ok(spoken),
                            Err(err) => {
                                if !env.loopback_override
                                    && err != *grokhub_core::offline::OFFLINE_MSG
                                {
                                    voice_route::note_xai_failure(
                                        StageKind::Tts,
                                        grokhub_core::now_ms(),
                                    );
                                }
                                let has_local_after =
                                    sources[idx + 1..].contains(&SpeechSource::Local);
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
                                        vec![TTS_MODEL_ID.to_string()],
                                    );
                                }
                                last_err = Some(err);
                            }
                        }
                    }
                    SpeechSource::Local => {
                        match speak_on_device(config_dir, models_dir, voice, text, asked) {
                            Some(Ok(spoken)) => return Ok(spoken),
                            Some(Err(err)) if err == HUSHED => return Err(err),
                            Some(Err(err)) => last_err = Some(err),
                            None => {}
                        }
                    }
                }
            }
            Err(last_err.unwrap_or_else(|| NO_VOICE.to_string()))
        }
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
                "xAI didn't accept the Grok sign-in or API key. Check Settings → Account."
                    .to_string()
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

/// On-device pieces run one at a time.
static SPEAK_ONE: std::sync::Mutex<()> = std::sync::Mutex::new(());
/// Bumped when the person talks over a reply: pieces of it still waiting
/// for [`SPEAK_ONE`] are dropped instead of spoken.
static HUSHES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
/// What a piece of a hushed reply answers; the page has already let it go.
const HUSHED: &str = "That reply was stopped.";

/// Hushes so far; a piece notes this when the page asks for it.
fn hushes() -> u64 {
    HUSHES.load(std::sync::atomic::Ordering::SeqCst)
}

/// Said when the on-device voice, needing `mb`, doesn't fit in free memory now.
fn no_room_to_speak(mb: u64) -> String {
    format!(
        "The on-device voice needs about {} GB of free memory, and this computer doesn't have it right now.",
        mb.div_ceil(1024)
    )
}

/// Kokoro in the on-device speech engine. None when the engine or a voice
/// isn't downloaded.
fn speak_on_device(
    config_dir: &Path,
    models_dir: &Path,
    voice: &str,
    text: &str,
    asked: u64,
) -> Option<Result<Spoken, String>> {
    use base64::Engine as _;
    if !ve::tts_installed(config_dir) {
        return None;
    }
    let model = ve::voice_model(models_dir)?;
    // One piece at a time on this computer: the page asks for every piece of
    // a reply at once, and each run loads the whole voice model.
    let _one = SPEAK_ONE.lock().unwrap_or_else(|e| e.into_inner());
    if hushes() != asked {
        return Some(Err(HUSHED.into()));
    }
    // The voice doesn't unload the chat model: that would reload it on every
    // turn. Without room beside it, the piece isn't spoken.
    let Ok(_speaking) = grokhub_agent::route::local_resident::reserve(
        grokhub_agent::route::local_resident::Kind::Speech,
        grokhub_agent::route::local_resident::Mem::ram(model.run_mb),
        false,
    ) else {
        return Some(Err(no_room_to_speak(model.run_mb)));
    };
    // Each piece writes its own new file.
    let out = match ve::claim_speak_file(&std::env::temp_dir()) {
        Ok(out) => out,
        Err(_) => return Some(Err("The on-device voice had nowhere to write.".into())),
    };
    let Some(args) = ve::speak_args(models_dir, model, voice, text, &out) else {
        let _ = std::fs::remove_file(&out);
        return None;
    };
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
/// The stream's words when it has them within
/// [`crate::stt_stream::HEAR_WAIT`], else one `upload` of the recording.
pub fn hear(
    turn: Option<Pending>,
    upload: impl FnOnce(&[u8]) -> Result<String, String>,
) -> Result<String, String> {
    let (samples, news) = match turn {
        None => return Ok(String::new()),
        Some(Pending::Words(words)) => return Ok(words),
        Some(Pending::Audio { samples, news }) => (samples, news),
    };
    let streamed =
        news.and_then(|n| crate::stt_stream::final_words(&n, crate::stt_stream::HEAR_WAIT));
    if let Some(words) = streamed {
        return Ok(words);
    }
    match upload(&wav(&samples, RATE)) {
        Ok(words) => Ok(words),
        // A cough or a door: listen again.
        Err(e) if e == crate::voice::EMPTY => Ok(String::new()),
        Err(e) => Err(e),
    }
}

fn models_dir(config_dir: &Path) -> std::path::PathBuf {
    crate::local_ai::models_dir().unwrap_or_else(|| config_dir.join("models"))
}

/// Voice mode on: the mic is listening for the first turn, with a stream
/// when the shared voice router puts Grok Voice 3 first.
/// Each listen's stream re-checks auth and routing: a key or mode changed
/// while voice mode is on takes effect on the next listen.
#[cfg(test)]
fn opener(
    dir: std::path::PathBuf,
    target: fn() -> Option<crate::stt_stream::StreamTarget>,
) -> Opener {
    Box::new(move || target().and_then(|to| crate::stt_stream::open(&dir, &to)))
}

fn opener_for(dir: std::path::PathBuf) -> Opener {
    Box::new(move || {
        crate::stt_stream::target_for(&dir).and_then(|to| crate::stt_stream::open(&dir, &to))
    })
}

/// Off the main thread: reads config/keychain, marks the voice session active
/// for reply routing, starts any needed local voice asset download, and opens
/// the first STT stream if Grok Voice 3 is planned first.
#[tauri::command]
pub(crate) async fn voice_live_start(
    app: tauri::AppHandle,
    state: tauri::State<'_, crate::ShellState>,
) -> Result<(), String> {
    crate::capabilities::needs(crate::capabilities::current().voice, "Voice")?;
    let dir = state.config_dir.clone();
    crate::off_main(move || {
        voice_route::set_session_active(true);
        let cfg = voice_route::read(&dir);
        let models = models_dir(&dir);
        let mut missing = Vec::new();
        if matches!(
            voice_route::plan_stage(&cfg, StageKind::Stt, &crate::voice::stt_env(&dir, &models)),
            StagePlan::DownloadLocal
        ) {
            missing.push(crate::voice::STT_MODEL_ID.to_string());
        }
        if matches!(
            voice_route::plan_stage(&cfg, StageKind::Tts, &tts_env(&dir, &models)),
            StagePlan::DownloadLocal
        ) {
            missing.push(TTS_MODEL_ID.to_string());
        }
        if !missing.is_empty() {
            let _ = crate::model_library::download(app, dir.clone(), missing);
        }
        let live = Live::new(Some(opener_for(dir)));
        *SESSION.lock().unwrap_or_else(|e| e.into_inner()) = Some(live);
    })
    .await
}

/// Voice mode off: any turn under way is dropped.
#[tauri::command]
pub(crate) fn voice_live_stop() {
    voice_route::set_session_active(false);
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
    let (events, reopen) = with(|live| (live.push(&bytes), live.take_reopen()))?;
    if reopen {
        // Off the main thread: opening reads the OS keychain.
        tauri::async_runtime::spawn_blocking(|| then_open(&SESSION, |live| live.wants_stream()));
    }
    events
}

/// `change` under the session lock, then the stream it wants opened with
/// the lock let go: the opener reads the OS keychain, and the page's mic
/// blocks (on the main thread) must not wait for that.
fn then_open(
    cell: &Mutex<Option<Live>>,
    change: impl FnOnce(&mut Live) -> bool,
) -> Result<(), String> {
    let lent = with_in(cell, |live| {
        if change(live) {
            live.lend_opener()
        } else {
            None
        }
    })?;
    if let Some(mut open) = lent {
        let link = open();
        // Voice mode turned off meanwhile: the stream just goes.
        let _ = with_in(cell, |live| live.take_back(open, link));
    }
    Ok(())
}

/// Off the main thread: a listen may open a stream.
#[tauri::command]
pub(crate) async fn voice_live_listen(playing: bool) -> Result<(), String> {
    crate::off_main(move || then_open(&SESSION, |live| live.restart(playing))).await?
}

/// Off the main thread: the end of a reply may open a stream.
#[tauri::command]
pub(crate) async fn voice_live_playing(playing: bool) -> Result<(), String> {
    crate::off_main(move || then_open(&SESSION, |live| live.playing_changed(playing))).await?
}

#[tauri::command]
pub(crate) fn voice_live_finish() -> Result<(), String> {
    with(Live::finish)
}

/// The last turn's words (empty: listen again).
#[tauri::command]
pub(crate) async fn voice_live_hear(
    app: tauri::AppHandle,
    state: tauri::State<'_, crate::ShellState>,
) -> Result<String, String> {
    let turn = with(Live::take_turn)?;
    let dir = state.config_dir.clone();
    crate::off_main(move || {
        hear(turn, |wav| {
            crate::voice::transcribe_with(Some(&app), &dir, wav)
        })
    })
    .await?
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
    HUSHES.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    with(Live::reply_hush)
}

/// One piece of a reply as speech, in the chat's voice.
#[tauri::command]
pub(crate) async fn voice_speak(
    app: tauri::AppHandle,
    state: tauri::State<'_, crate::ShellState>,
    text: String,
) -> Result<Spoken, String> {
    crate::capabilities::needs(crate::capabilities::current().voice, "Voice")?;
    let dir = state.config_dir.clone();
    let voice = crate::personality::speech_voice(&dir, &crate::current(&state));
    // Taken as the page asks, so a hush that comes later drops this piece.
    let asked = hushes();
    crate::off_main(move || speak_with(Some(&app), &dir, &models_dir(&dir), voice, &text, asked))
        .await?
}

#[tauri::command]
pub(crate) fn get_voice_route(state: tauri::State<'_, crate::ShellState>) -> VoiceRouteConfig {
    voice_route::read(&state.config_dir)
}

#[tauri::command]
pub(crate) fn set_voice_route(
    state: tauri::State<'_, crate::ShellState>,
    config: VoiceRouteConfig,
) -> Result<VoiceRouteConfig, String> {
    voice_route::write(&state.config_dir, config)
}

#[cfg(test)]
#[path = "voice_live_tests.rs"]
mod tests;
