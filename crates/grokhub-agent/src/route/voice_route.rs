//! Shared voice routing (`Auto` / `Local only`) for speech-to-text, reply
//! model, and voice/TTS. There is no per-stage pin: Auto picks each source,
//! and a file saved with the old `pinned` mode reads as Auto.
//!
//! xAI (`grok-voice-3.0`) is one option alongside on-device engines
//! (`sherpa-onnx` Parakeet/Kokoro, `whisper`, `llama-server`) and added reply
//! providers (`providers::usable`). Neither online nor local is forced blindly:
//! `Auto` picks from live connectivity, credentials, warm local readiness and
//! recent session failures, switching automatically if a cloud stage fails
//! mid-session.
//!
//! Preferences live in `{config}/voice-route.json` with a mutex and atomic
//! rename (same pattern as [`super::local_pin`]).

use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::connectivity::NetState;

const FILE: &str = "voice-route.json";
const XAI_COOLDOWN_MS: u64 = 5 * 60 * 1000;

/// Overall voice routing strategy across STT, reply model, and TTS.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VoiceRouteMode {
    #[default]
    Auto,
    LocalOnly,
}

/// Source choice for speech-to-text or text-to-speech.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpeechSource {
    Local,
    GrokVoice3,
}

/// Persisted voice routing preference in `{config}/voice-route.json`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct VoiceRouteConfig {
    pub mode: VoiceRouteMode,
}

/// Reply-model routing constraint injected into [`super::RouteInput`] while
/// a voice session is active.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReplyPref {
    LocalOnly,
}

/// Speech stage identifier for cooldown tracking and user-facing blocker copy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StageKind {
    Stt,
    Tts,
}

/// Runtime availability snapshot for pure STT / TTS route planning.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VoiceEnv {
    pub loopback_override: bool,
    pub offline_only: bool,
    pub net: NetState,
    pub local_enabled: bool,
    pub local_ai_capable: bool,
    pub local_ready: bool,
    pub local_warm: bool,
    pub has_xai_auth: bool,
    pub xai_recently_failed: bool,
    /// The chat's newest step ran on this computer
    /// ([`super::local_live::runs_here`]): its voice runs here too, with xAI
    /// only as a fallback while the on-device voice isn't ready.
    pub chat_here: bool,
    /// The chat must stay on this computer (an 18+ voice,
    /// [`super::local_live::must_stay_here`]): its voice never goes to xAI.
    pub chat_must_stay: bool,
}

impl Default for VoiceEnv {
    fn default() -> Self {
        Self {
            loopback_override: false,
            offline_only: false,
            net: NetState::Online,
            local_enabled: false,
            local_ai_capable: false,
            local_ready: false,
            local_warm: false,
            has_xai_auth: false,
            xai_recently_failed: false,
            chat_here: false,
            chat_must_stay: false,
        }
    }
}

/// Action the shell takes for a speech (STT) or voice (TTS) step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StagePlan {
    /// Try these sources in order; failover on error moves to the next candidate.
    Try(Vec<SpeechSource>),
    /// Local assets are missing and can be downloaded right now (`model_library::download`).
    DownloadLocal,
    /// Neither chosen source nor a safe automatic download can run right now.
    Blocked(String),
}

static LOCK: Mutex<()> = Mutex::new(());
static SESSION_ACTIVE: AtomicBool = AtomicBool::new(false);
static STT_XAI_COOLDOWN_UNTIL: AtomicU64 = AtomicU64::new(0);
static TTS_XAI_COOLDOWN_UNTIL: AtomicU64 = AtomicU64::new(0);

thread_local! {
    static TEST_SESSION_ACTIVE: std::cell::Cell<Option<bool>> = const { std::cell::Cell::new(None) };
}

/// RAII guard pinning [`session_active`] on the current thread so parallel
/// unit tests never race the process-wide voice session flag.
pub struct SessionActiveGuard(Option<bool>);

impl SessionActiveGuard {
    pub fn set(active: bool) -> Self {
        Self(TEST_SESSION_ACTIVE.with(|c| c.replace(Some(active))))
    }
}

impl Drop for SessionActiveGuard {
    fn drop(&mut self) {
        TEST_SESSION_ACTIVE.with(|c| c.set(self.0));
    }
}

/// Marks whether an open-mic voice session is active in this process so
/// reply routing applies [`VoiceRouteConfig`] only during voice conversations.
pub fn set_session_active(active: bool) {
    SESSION_ACTIVE.store(active, Ordering::SeqCst);
}

pub fn session_active() -> bool {
    if let Some(active) = TEST_SESSION_ACTIVE.with(std::cell::Cell::get) {
        return active;
    }
    SESSION_ACTIVE.load(Ordering::SeqCst)
}

pub const VOICE_SPEED_REASON: &str = "live voice turn";

/// True when an active voice session is running in `Auto` mode, so the
/// router automatically favors low-latency reply routes (`goal:speed` and
/// live waiting fast-variant eligibility) without any UI picker.
pub fn auto_fast_turn(dir: &Path) -> bool {
    session_active() && read(dir).mode == VoiceRouteMode::Auto
}

/// Default speed goal injected on user-facing voice turns in `Auto` mode
/// when no explicit session/pinned goal overrides it.
pub fn voice_turn_goal(dir: &Path) -> Option<super::goal::RouteGoal> {
    auto_fast_turn(dir).then(|| super::goal::RouteGoal {
        goal: super::goal::Goal::Speed,
        reason: VOICE_SPEED_REASON.to_string(),
    })
}

fn cooldown_cell(kind: StageKind) -> &'static AtomicU64 {
    match kind {
        StageKind::Stt => &STT_XAI_COOLDOWN_UNTIL,
        StageKind::Tts => &TTS_XAI_COOLDOWN_UNTIL,
    }
}

/// Records an online xAI failure for `kind` so subsequent turns in this
/// session prefer healthy local assets instead of retrying a broken online leg.
pub fn note_xai_failure(kind: StageKind, now_ms: u64) {
    cooldown_cell(kind).store(now_ms.saturating_add(XAI_COOLDOWN_MS), Ordering::SeqCst);
}

/// Clears any recorded cooldowns (when switching settings or re-authenticating).
pub fn clear_failures() {
    STT_XAI_COOLDOWN_UNTIL.store(0, Ordering::SeqCst);
    TTS_XAI_COOLDOWN_UNTIL.store(0, Ordering::SeqCst);
}

/// Whether `kind` recently failed online and is cooling down at `now_ms`.
pub fn xai_in_cooldown(kind: StageKind, now_ms: u64) -> bool {
    let until = cooldown_cell(kind).load(Ordering::SeqCst);
    until > 0 && now_ms < until
}

fn parse_lenient(bytes: &[u8]) -> Result<VoiceRouteConfig, String> {
    let v: Value = serde_json::from_slice(bytes)
        .map_err(|e| format!("Couldn't read {FILE}, so it was left as it is: {e}"))?;
    let mode = v
        .get("mode")
        .and_then(|m| serde_json::from_value::<VoiceRouteMode>(m.clone()).ok())
        .unwrap_or_default();
    Ok(VoiceRouteConfig { mode })
}

fn load(dir: &Path) -> Result<VoiceRouteConfig, String> {
    match std::fs::read(dir.join(FILE)) {
        Ok(bytes) => parse_lenient(&bytes),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(VoiceRouteConfig::default()),
        Err(e) => Err(format!("Couldn't read {FILE}: {e}")),
    }
}

/// Reads the saved voice route config, falling back to [`VoiceRouteConfig::default`].
pub fn read(dir: &Path) -> VoiceRouteConfig {
    load(dir).unwrap_or_default()
}

/// Saves `cfg` atomically to `{config}/voice-route.json`.
pub fn write(dir: &Path, cfg: VoiceRouteConfig) -> Result<VoiceRouteConfig, String> {
    let _held = LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let cleaned = cfg;
    // Preserve unreadable custom fields by validating readability first, or
    // allow overwriting once valid JSON.
    let _ = load(dir)?;
    let body = serde_json::to_vec_pretty(&cleaned).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(dir).map_err(|e| format!("Couldn't save {FILE}: {e}"))?;
    let tmp = dir.join(format!("{FILE}.tmp"));
    std::fs::write(&tmp, body).map_err(|e| format!("Couldn't save {FILE}: {e}"))?;
    std::fs::rename(&tmp, dir.join(FILE)).map_err(|e| format!("Couldn't save {FILE}: {e}"))?;
    clear_failures();
    Ok(cleaned)
}

/// Computes the reply preference enforced when voice mode is active.
pub fn reply_pref_for(cfg: &VoiceRouteConfig) -> Option<ReplyPref> {
    match cfg.mode {
        VoiceRouteMode::Auto => None,
        VoiceRouteMode::LocalOnly => Some(ReplyPref::LocalOnly),
    }
}

/// Returns the reply preference to inject into [`super::live::decide`] when active.
pub fn active_reply_pref(dir: &Path) -> Option<ReplyPref> {
    if !session_active() {
        return None;
    }
    reply_pref_for(&read(dir))
}

fn stage_noun(kind: StageKind) -> &'static str {
    match kind {
        StageKind::Stt => "Speech to text",
        StageKind::Tts => "Spoken replies",
    }
}

fn model_noun(kind: StageKind) -> &'static str {
    match kind {
        StageKind::Stt => "on-device speech model (Parakeet or whisper)",
        StageKind::Tts => "on-device voice (Kokoro)",
    }
}

fn local_offline_blocker(kind: StageKind, env: &VoiceEnv) -> String {
    let noun = stage_noun(kind);
    let model = model_noun(kind);
    if !env.local_enabled {
        return format!("{noun} needs the {model}. Turn on Local AI in Settings → Local AI.");
    }
    if env.offline_only {
        return format!(
            "Offline only is on and the {model} isn't downloaded yet. Turn off Offline only to download it, or install it in Settings → Local AI."
        );
    }
    if env.net.prefer_local() {
        return format!(
            "You're offline and the {model} isn't downloaded yet. Connect to download it once, and it'll work offline after that."
        );
    }
    format!(
        "{noun} can't download the {model} on this device automatically. Sign in with Grok in Settings → Account, or set up {model}."
    )
}

fn neither_available_blocker(kind: StageKind, env: &VoiceEnv) -> String {
    let noun = stage_noun(kind);
    if !env.local_enabled && !env.has_xai_auth {
        return format!(
            "{noun} needs Grok sign-in (Settings → Account) or Local AI enabled (Settings → Local AI)."
        );
    }
    local_offline_blocker(kind, env)
}

fn must_stay_blocker(kind: StageKind) -> String {
    format!(
        "{} in this 18+ chat stay on this computer, and the {} isn't downloaded. Install it in Settings → Local AI.",
        stage_noun(kind),
        model_noun(kind)
    )
}

fn must_stay_off_blocker(kind: StageKind) -> String {
    format!(
        "{} in this 18+ chat stay on this computer, and Local AI is off. Turn it on in Settings → Local AI.",
        stage_noun(kind)
    )
}

/// An 18+ chat's voice stays on this computer, whatever the routing says:
/// the on-device engine, its download, or a
/// blocker naming the real reason.
fn must_stay_plan(kind: StageKind, env: &VoiceEnv, offline: bool) -> StagePlan {
    if env.local_enabled && env.local_ready {
        StagePlan::Try(vec![SpeechSource::Local])
    } else if !env.local_enabled {
        StagePlan::Blocked(must_stay_off_blocker(kind))
    } else if offline {
        StagePlan::Blocked(local_offline_blocker(kind, env))
    } else if env.local_ai_capable {
        StagePlan::DownloadLocal
    } else {
        StagePlan::Blocked(must_stay_blocker(kind))
    }
}

/// Whether the on-device engine for `kind` should download now: the plan
/// waits on it, or xAI stands in for a chat that runs here only until it is
/// downloaded.
pub fn wants_download(cfg: &VoiceRouteConfig, kind: StageKind, env: &VoiceEnv) -> bool {
    let offline = env.offline_only || env.net.prefer_local();
    matches!(plan_stage(cfg, kind, env), StagePlan::DownloadLocal)
        || (env.chat_here
            && !env.loopback_override
            && !offline
            && env.local_enabled
            && env.local_ai_capable
            && !env.local_ready
            && cfg.mode != VoiceRouteMode::LocalOnly)
}

/// Plans source order (or auto-download / actionable blocker) for STT or TTS.
pub fn plan_stage(cfg: &VoiceRouteConfig, kind: StageKind, env: &VoiceEnv) -> StagePlan {
    if env.loopback_override {
        return StagePlan::Try(vec![SpeechSource::GrokVoice3]);
    }
    let offline = env.offline_only || env.net.prefer_local();
    match cfg.mode {
        VoiceRouteMode::LocalOnly => {
            if env.local_ready {
                StagePlan::Try(vec![SpeechSource::Local])
            } else if !offline && env.local_ai_capable {
                StagePlan::DownloadLocal
            } else {
                StagePlan::Blocked(local_offline_blocker(kind, env))
            }
        }
        VoiceRouteMode::Auto => {
            let online_usable = !offline && env.has_xai_auth && !env.xai_recently_failed;
            let local_usable = env.local_enabled && env.local_ready;
            if offline {
                return if local_usable {
                    StagePlan::Try(vec![SpeechSource::Local])
                } else {
                    StagePlan::Blocked(local_offline_blocker(kind, env))
                };
            }
            // A chat that runs here hears and speaks here. No xAI behind it,
            // so a decline (no room) shows its own line instead of leaving.
            if env.chat_must_stay {
                return must_stay_plan(kind, env, offline);
            }
            if env.chat_here && local_usable {
                return StagePlan::Try(vec![SpeechSource::Local]);
            }
            if env.chat_here && online_usable {
                // The fallback while the on-device voice isn't ready, with a line.
                return StagePlan::Try(vec![SpeechSource::GrokVoice3]);
            }
            match (online_usable, local_usable) {
                (true, true) => {
                    if env.net == NetState::Degraded || env.local_warm {
                        StagePlan::Try(vec![SpeechSource::Local, SpeechSource::GrokVoice3])
                    } else {
                        StagePlan::Try(vec![SpeechSource::GrokVoice3, SpeechSource::Local])
                    }
                }
                (false, true) => StagePlan::Try(vec![SpeechSource::Local]),
                (true, false) => StagePlan::Try(vec![SpeechSource::GrokVoice3]),
                (false, false) => {
                    if env.local_enabled && env.local_ai_capable {
                        StagePlan::DownloadLocal
                    } else {
                        StagePlan::Blocked(neither_available_blocker(kind, env))
                    }
                }
            }
        }
    }
}

/// The line over the composer naming which voice engine ran a stage and why.
/// Shown for the on-device engine, an xAI fallback, or an xAI stand-in for a
/// chat that runs here; a plain cloud chat gets none. `fell_back` is true
/// when `used` ran because the source before it failed; `model` names the
/// on-device model that ran (unused for xAI).
pub fn stage_line(
    kind: StageKind,
    cfg: &VoiceRouteConfig,
    env: &VoiceEnv,
    used: SpeechSource,
    fell_back: bool,
    model: &str,
) -> Option<String> {
    let verb = match kind {
        StageKind::Stt => "Heard",
        StageKind::Tts => "Spoken",
    };
    match used {
        SpeechSource::Local => {
            let why = if env.offline_only {
                "Offline only is on"
            } else if env.net.prefer_local() {
                "you're offline"
            } else if cfg.mode == VoiceRouteMode::LocalOnly {
                "voice routing is Local only"
            } else if env.chat_must_stay {
                "this 18+ chat stays on this computer"
            } else if env.chat_here {
                "this chat runs here"
            } else if fell_back || env.xai_recently_failed {
                "xAI didn't answer"
            } else if !env.has_xai_auth {
                "there's no xAI sign-in"
            } else if env.net == NetState::Degraded {
                "the connection is slow"
            } else {
                "a local model is already running"
            };
            Some(format!("{verb} on this computer with {model}: {why}."))
        }
        SpeechSource::GrokVoice3 if fell_back => Some(format!(
            "{verb} by xAI: the {} didn't answer.",
            model_noun(kind)
        )),
        SpeechSource::GrokVoice3 if env.chat_here => {
            let but = if !env.local_enabled {
                "Local AI is off".to_string()
            } else if !env.local_ready {
                format!("the {} isn't downloaded", model_noun(kind))
            } else {
                format!("the {} isn't ready", model_noun(kind))
            };
            Some(format!("{verb} by xAI: this chat runs here, but {but}."))
        }
        SpeechSource::GrokVoice3 => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env() -> VoiceEnv {
        VoiceEnv {
            loopback_override: false,
            offline_only: false,
            net: NetState::Online,
            local_enabled: true,
            local_ai_capable: true,
            local_ready: false,
            local_warm: false,
            has_xai_auth: false,
            xai_recently_failed: false,
            chat_here: false,
            chat_must_stay: false,
        }
    }

    #[test]
    fn auto_uses_grok_voice_3_over_oauth_when_signed_in_and_online() {
        let cfg = VoiceRouteConfig::default();
        let mut e = env();
        e.has_xai_auth = true;
        assert_eq!(
            plan_stage(&cfg, StageKind::Tts, &e),
            StagePlan::Try(vec![SpeechSource::GrokVoice3])
        );
    }

    #[test]
    fn auto_prefers_warm_local_or_degraded_net_first_with_online_failover() {
        let cfg = VoiceRouteConfig::default();
        let mut e = env();
        e.has_xai_auth = true;
        e.local_ready = true;
        e.local_warm = true;
        assert_eq!(
            plan_stage(&cfg, StageKind::Stt, &e),
            StagePlan::Try(vec![SpeechSource::Local, SpeechSource::GrokVoice3])
        );

        e.local_warm = false;
        e.net = NetState::Degraded;
        assert_eq!(
            plan_stage(&cfg, StageKind::Tts, &e),
            StagePlan::Try(vec![SpeechSource::Local, SpeechSource::GrokVoice3])
        );
    }

    #[test]
    fn auto_includes_local_failover_behind_grok_when_local_is_ready() {
        let cfg = VoiceRouteConfig::default();
        let mut e = env();
        e.has_xai_auth = true;
        e.local_ready = true;
        assert_eq!(
            plan_stage(&cfg, StageKind::Tts, &e),
            StagePlan::Try(vec![SpeechSource::GrokVoice3, SpeechSource::Local])
        );
    }

    #[test]
    fn auto_uses_local_without_any_xai_auth_and_auto_downloads_when_missing() {
        let cfg = VoiceRouteConfig::default();
        let mut e = env();
        assert_eq!(
            plan_stage(&cfg, StageKind::Stt, &e),
            StagePlan::DownloadLocal
        );
        e.local_ready = true;
        assert_eq!(
            plan_stage(&cfg, StageKind::Stt, &e),
            StagePlan::Try(vec![SpeechSource::Local])
        );
    }

    #[test]
    fn offline_never_picks_online_and_blocks_clearly_without_local_assets() {
        let cfg = VoiceRouteConfig::default();
        let mut e = env();
        e.has_xai_auth = true;
        e.net = NetState::Offline;
        assert!(matches!(
            plan_stage(&cfg, StageKind::Tts, &e),
            StagePlan::Blocked(msg) if msg.contains("Kokoro") && msg.contains("offline")
        ));

        e.local_ready = true;
        assert_eq!(
            plan_stage(&cfg, StageKind::Tts, &e),
            StagePlan::Try(vec![SpeechSource::Local])
        );
    }

    #[test]
    fn local_only_keeps_every_stage_on_this_computer() {
        let local_only = VoiceRouteConfig {
            mode: VoiceRouteMode::LocalOnly,
        };
        let mut e = env();
        e.has_xai_auth = true;
        e.local_ready = true;
        assert_eq!(
            plan_stage(&local_only, StageKind::Stt, &e),
            StagePlan::Try(vec![SpeechSource::Local])
        );
        assert_eq!(reply_pref_for(&local_only), Some(ReplyPref::LocalOnly));
        assert_eq!(
            plan_stage(&local_only, StageKind::Tts, &e),
            StagePlan::Try(vec![SpeechSource::Local])
        );
        assert_eq!(reply_pref_for(&VoiceRouteConfig::default()), None);
    }

    #[test]
    fn config_round_trips_and_an_old_pinned_file_reads_as_auto() {
        let dir = std::env::temp_dir().join(format!("grokhub-voice-route-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(read(&dir), VoiceRouteConfig::default());
        let saved = VoiceRouteConfig {
            mode: VoiceRouteMode::LocalOnly,
        };
        let written = write(&dir, saved.clone()).expect("write");
        assert_eq!(written, saved);
        assert_eq!(read(&dir), saved);
        assert_eq!(
            std::fs::read_to_string(dir.join(FILE)).unwrap(),
            "{\n  \"mode\": \"local_only\"\n}"
        );

        for old in [
            r#"{"mode":"pinned","pinned":{"stt":"local","reply":"openrouter","tts":"grok_voice_3"}}"#,
            r#"{"mode":"future_mode"}"#,
        ] {
            std::fs::write(dir.join(FILE), old).unwrap();
            assert_eq!(read(&dir).mode, VoiceRouteMode::Auto, "{old}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn cooldown_gates_xai_after_mid_session_failure() {
        clear_failures();
        assert!(!xai_in_cooldown(StageKind::Tts, 1_000));
        note_xai_failure(StageKind::Tts, 1_000);
        assert!(xai_in_cooldown(StageKind::Tts, 2_000));
        assert!(!xai_in_cooldown(StageKind::Stt, 2_000));
        assert!(!xai_in_cooldown(StageKind::Tts, 1_000 + XAI_COOLDOWN_MS));
        clear_failures();
    }

    #[test]
    fn a_chat_that_runs_here_hears_and_speaks_on_device() {
        let cfg = VoiceRouteConfig::default();
        let mut e = env();
        e.has_xai_auth = true;
        e.local_ready = true;
        e.chat_here = true;
        assert_eq!(
            plan_stage(&cfg, StageKind::Stt, &e),
            StagePlan::Try(vec![SpeechSource::Local])
        );
        assert_eq!(
            plan_stage(&cfg, StageKind::Tts, &e),
            StagePlan::Try(vec![SpeechSource::Local])
        );
    }

    #[test]
    fn a_chat_that_runs_here_uses_xai_only_while_local_isnt_ready() {
        let cfg = VoiceRouteConfig::default();
        let mut e = env();
        e.chat_here = true;
        e.has_xai_auth = true;
        assert_eq!(
            plan_stage(&cfg, StageKind::Stt, &e),
            StagePlan::Try(vec![SpeechSource::GrokVoice3])
        );
        e.has_xai_auth = false;
        assert_eq!(
            plan_stage(&cfg, StageKind::Stt, &e),
            StagePlan::DownloadLocal
        );
    }

    #[test]
    fn an_18_plus_chat_never_plans_xai() {
        let mut e = env();
        e.chat_must_stay = true;
        e.has_xai_auth = true;
        e.local_ai_capable = false;
        let blocked = StagePlan::Blocked(
            "Spoken replies in this 18+ chat stay on this computer, and the on-device voice (Kokoro) isn't downloaded. Install it in Settings → Local AI.".into(),
        );
        assert_eq!(
            plan_stage(&VoiceRouteConfig::default(), StageKind::Tts, &e),
            blocked
        );
        // With the voice downloaded it speaks here.
        e.local_ready = true;
        assert_eq!(
            plan_stage(&VoiceRouteConfig::default(), StageKind::Tts, &e),
            StagePlan::Try(vec![SpeechSource::Local])
        );
    }

    #[test]
    fn the_line_names_the_voice_and_why_and_stays_quiet_for_plain_cloud() {
        let cfg = VoiceRouteConfig::default();
        let mut here = env();
        here.has_xai_auth = true;
        here.local_ready = true;
        here.chat_here = true;
        assert_eq!(
            stage_line(
                StageKind::Tts,
                &cfg,
                &here,
                SpeechSource::Local,
                false,
                "Kokoro text to speech"
            )
            .as_deref(),
            Some("Spoken on this computer with Kokoro text to speech: this chat runs here.")
        );
        here.local_ready = false;
        assert_eq!(
            stage_line(StageKind::Stt, &cfg, &here, SpeechSource::GrokVoice3, false, "")
                .as_deref(),
            Some("Heard by xAI: this chat runs here, but the on-device speech model (Parakeet or whisper) isn't downloaded.")
        );
        let mut cloud = env();
        cloud.has_xai_auth = true;
        assert_eq!(
            stage_line(
                StageKind::Stt,
                &cfg,
                &cloud,
                SpeechSource::GrokVoice3,
                false,
                ""
            ),
            None
        );
        assert_eq!(
            stage_line(
                StageKind::Tts,
                &cfg,
                &cloud,
                SpeechSource::GrokVoice3,
                true,
                ""
            )
            .as_deref(),
            Some("Spoken by xAI: the on-device voice (Kokoro) didn't answer.")
        );
        cloud.offline_only = true;
        assert_eq!(
            stage_line(
                StageKind::Stt,
                &cfg,
                &cloud,
                SpeechSource::Local,
                false,
                "Parakeet v3 speech to text"
            )
            .as_deref(),
            Some("Heard on this computer with Parakeet v3 speech to text: Offline only is on.")
        );
    }

    #[test]
    fn an_18_plus_chat_speaks_here_and_says_so() {
        let cfg = VoiceRouteConfig::default();
        let mut e = env();
        e.chat_must_stay = true;
        e.has_xai_auth = true;
        e.local_ready = true;
        assert_eq!(
            plan_stage(&cfg, StageKind::Tts, &e),
            StagePlan::Try(vec![SpeechSource::Local])
        );
        assert_eq!(
            stage_line(StageKind::Tts, &cfg, &e, SpeechSource::Local, false, "Kokoro text to speech")
                .as_deref(),
            Some("Spoken on this computer with Kokoro text to speech: this 18+ chat stays on this computer.")
        );
        // Downloaded but Local AI off: the blocker says that.
        e.local_enabled = false;
        assert_eq!(
            plan_stage(&cfg, StageKind::Tts, &e),
            StagePlan::Blocked(
                "Spoken replies in this 18+ chat stay on this computer, and Local AI is off. Turn it on in Settings → Local AI.".into(),
            )
        );
        // Not downloaded yet: it downloads, never xAI.
        e.local_enabled = true;
        e.local_ready = false;
        assert_eq!(
            plan_stage(&cfg, StageKind::Tts, &e),
            StagePlan::DownloadLocal
        );
    }

    #[test]
    fn the_xai_line_for_a_chat_that_runs_here_names_the_real_reason() {
        let mut e = env();
        e.chat_here = true;
        e.has_xai_auth = true;
        e.local_ready = true;
        let line = |cfg: &VoiceRouteConfig, e: &VoiceEnv| {
            stage_line(StageKind::Tts, cfg, e, SpeechSource::GrokVoice3, false, "")
        };
        assert_eq!(
            line(&VoiceRouteConfig::default(), &e).as_deref(),
            Some(
                "Spoken by xAI: this chat runs here, but the on-device voice (Kokoro) isn't ready."
            )
        );
        e.local_enabled = false;
        assert_eq!(
            line(&VoiceRouteConfig::default(), &e).as_deref(),
            Some("Spoken by xAI: this chat runs here, but Local AI is off.")
        );
        // A warm chat model is not the voice: the reason says what it is.
        let mut warm = env();
        warm.local_ready = true;
        warm.local_warm = true;
        warm.has_xai_auth = true;
        assert_eq!(
            stage_line(
                StageKind::Stt,
                &VoiceRouteConfig::default(),
                &warm,
                SpeechSource::Local,
                false,
                "whisper"
            )
            .as_deref(),
            Some("Heard on this computer with whisper: a local model is already running.")
        );
    }

    #[test]
    fn a_chat_that_runs_here_downloads_its_voice_while_xai_stands_in() {
        let cfg = VoiceRouteConfig::default();
        let mut e = env();
        e.chat_here = true;
        e.has_xai_auth = true;
        assert_eq!(
            plan_stage(&cfg, StageKind::Tts, &e),
            StagePlan::Try(vec![SpeechSource::GrokVoice3])
        );
        assert!(wants_download(&cfg, StageKind::Tts, &e));
        assert!(wants_download(&cfg, StageKind::Stt, &e));
        // Ready, or a cloud chat: nothing to fetch.
        let mut cloud = e;
        cloud.chat_here = false;
        assert!(!wants_download(&cfg, StageKind::Tts, &cloud));
        e.local_ready = true;
        assert!(!wants_download(&cfg, StageKind::Tts, &e));
        // The plan itself waits on the download.
        let mut none = env();
        none.has_xai_auth = false;
        assert!(wants_download(&cfg, StageKind::Stt, &none));
    }

    #[test]
    fn auto_voice_session_injects_speed_goal_and_local_only_does_not() {
        let dir = std::env::temp_dir().join(format!(
            "gh-voice-fast-{}-{}",
            std::process::id(),
            grokhub_core::now_ms()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        {
            let _off = SessionActiveGuard::set(false);
            assert!(!auto_fast_turn(&dir));
            assert_eq!(voice_turn_goal(&dir), None);
        }
        {
            let _on = SessionActiveGuard::set(true);
            assert!(auto_fast_turn(&dir));
            let goal = voice_turn_goal(&dir).expect("speed goal in auto voice session");
            assert_eq!(goal.goal, super::super::goal::Goal::Speed);
            assert_eq!(goal.reason, VOICE_SPEED_REASON);

            write(
                &dir,
                VoiceRouteConfig {
                    mode: VoiceRouteMode::LocalOnly,
                },
            )
            .unwrap();
            assert!(!auto_fast_turn(&dir));
            assert_eq!(voice_turn_goal(&dir), None);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
