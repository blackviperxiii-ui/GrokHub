//! Shared voice routing (`Auto` / `Local only` / `Pinned`) for speech-to-text,
//! reply model, and voice/TTS.
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
    Pinned,
}

/// Source choice for speech-to-text or text-to-speech.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpeechSource {
    Local,
    GrokVoice3,
}

/// Stage-level source pins used when [`VoiceRouteMode::Pinned`] is active.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct VoicePin {
    pub stt: Option<SpeechSource>,
    /// Reply source id: `"local"`, `"xai"`, a configured provider id, or `None` for Auto.
    pub reply: Option<String>,
    pub tts: Option<SpeechSource>,
}

/// Persisted voice routing preference in `{config}/voice-route.json`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct VoiceRouteConfig {
    pub mode: VoiceRouteMode,
    pub pinned: VoicePin,
}

/// Reply-model routing constraint injected into [`super::RouteInput`] while
/// a voice session is active.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReplyPref {
    LocalOnly,
    Provider(String),
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

/// Marks whether an open-mic voice session is active in this process so
/// reply routing applies [`VoiceRouteConfig`] only during voice conversations.
pub fn set_session_active(active: bool) {
    SESSION_ACTIVE.store(active, Ordering::SeqCst);
}

pub fn session_active() -> bool {
    SESSION_ACTIVE.load(Ordering::SeqCst)
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

fn normalize_reply_pin(raw: Option<&str>) -> Option<String> {
    let trimmed = raw?.trim();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("auto") {
        return None;
    }
    if trimmed.eq_ignore_ascii_case("grok_voice_3") || trimmed.eq_ignore_ascii_case("grok") {
        return Some(super::PROVIDER_XAI.to_string());
    }
    Some(trimmed.to_ascii_lowercase())
}

fn clean(mut cfg: VoiceRouteConfig) -> VoiceRouteConfig {
    cfg.pinned.reply = normalize_reply_pin(cfg.pinned.reply.as_deref());
    cfg
}

fn parse_lenient(bytes: &[u8]) -> Result<VoiceRouteConfig, String> {
    let v: Value = serde_json::from_slice(bytes)
        .map_err(|e| format!("Couldn't read {FILE}, so it was left as it is: {e}"))?;
    let mode = v
        .get("mode")
        .and_then(|m| serde_json::from_value::<VoiceRouteMode>(m.clone()).ok())
        .unwrap_or_default();
    let pin_val = v.get("pinned");
    let stt = pin_val
        .and_then(|p| p.get("stt"))
        .and_then(|s| serde_json::from_value::<Option<SpeechSource>>(s.clone()).ok())
        .flatten();
    let tts = pin_val
        .and_then(|p| p.get("tts"))
        .and_then(|s| serde_json::from_value::<Option<SpeechSource>>(s.clone()).ok())
        .flatten();
    let reply = pin_val
        .and_then(|p| p.get("reply"))
        .and_then(Value::as_str)
        .and_then(|s| normalize_reply_pin(Some(s)));
    Ok(VoiceRouteConfig {
        mode,
        pinned: VoicePin { stt, reply, tts },
    })
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
    let cleaned = clean(cfg);
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
        VoiceRouteMode::Pinned => match normalize_reply_pin(cfg.pinned.reply.as_deref()) {
            Some(p) if p == super::local::PROVIDER_LOCAL => Some(ReplyPref::LocalOnly),
            Some(p) => Some(ReplyPref::Provider(p)),
            None => None,
        },
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

fn pinned_xai_auth_blocker(kind: StageKind) -> String {
    format!(
        "{} is pinned to Grok Voice 3. Sign in with Grok or add an xAI API key in Settings → Account, or switch Voice routing to Auto.",
        stage_noun(kind)
    )
}

fn pinned_xai_offline_blocker(kind: StageKind, offline_only: bool) -> String {
    if offline_only {
        format!(
            "{} is pinned to Grok Voice 3, while Offline only is on. Switch Voice routing to Auto or Local only, or turn off Offline only.",
            stage_noun(kind)
        )
    } else {
        format!(
            "{} is pinned to Grok Voice 3, and you're offline. Switch Voice routing to Auto or Local only to use on-device speech.",
            stage_noun(kind)
        )
    }
}

/// Plans source order (or auto-download / actionable blocker) for STT or TTS.
pub fn plan_stage(cfg: &VoiceRouteConfig, kind: StageKind, env: &VoiceEnv) -> StagePlan {
    if env.loopback_override {
        return StagePlan::Try(vec![SpeechSource::GrokVoice3]);
    }
    let pinned = match kind {
        StageKind::Stt => cfg.pinned.stt,
        StageKind::Tts => cfg.pinned.tts,
    };
    let offline = env.offline_only || env.net.prefer_local();
    match (cfg.mode, pinned) {
        (VoiceRouteMode::LocalOnly, _) | (VoiceRouteMode::Pinned, Some(SpeechSource::Local)) => {
            if env.local_ready {
                StagePlan::Try(vec![SpeechSource::Local])
            } else if !offline && env.local_ai_capable {
                StagePlan::DownloadLocal
            } else {
                StagePlan::Blocked(local_offline_blocker(kind, env))
            }
        }
        (VoiceRouteMode::Pinned, Some(SpeechSource::GrokVoice3)) => {
            if !env.has_xai_auth {
                StagePlan::Blocked(pinned_xai_auth_blocker(kind))
            } else if offline {
                StagePlan::Blocked(pinned_xai_offline_blocker(kind, env.offline_only))
            } else {
                StagePlan::Try(vec![SpeechSource::GrokVoice3])
            }
        }
        _ => {
            let online_usable = !offline && env.has_xai_auth && !env.xai_recently_failed;
            let local_usable = env.local_enabled && env.local_ready;
            if offline {
                return if local_usable {
                    StagePlan::Try(vec![SpeechSource::Local])
                } else {
                    StagePlan::Blocked(local_offline_blocker(kind, env))
                };
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
    fn local_only_and_pinned_modes_enforce_selected_sources() {
        let local_only = VoiceRouteConfig {
            mode: VoiceRouteMode::LocalOnly,
            pinned: VoicePin::default(),
        };
        let mut e = env();
        e.has_xai_auth = true;
        e.local_ready = true;
        assert_eq!(
            plan_stage(&local_only, StageKind::Stt, &e),
            StagePlan::Try(vec![SpeechSource::Local])
        );
        assert_eq!(reply_pref_for(&local_only), Some(ReplyPref::LocalOnly));

        let pinned = VoiceRouteConfig {
            mode: VoiceRouteMode::Pinned,
            pinned: VoicePin {
                stt: Some(SpeechSource::Local),
                reply: Some("openrouter".into()),
                tts: Some(SpeechSource::GrokVoice3),
            },
        };
        assert_eq!(
            plan_stage(&pinned, StageKind::Stt, &e),
            StagePlan::Try(vec![SpeechSource::Local])
        );
        assert_eq!(
            plan_stage(&pinned, StageKind::Tts, &e),
            StagePlan::Try(vec![SpeechSource::GrokVoice3])
        );
        assert_eq!(
            reply_pref_for(&pinned),
            Some(ReplyPref::Provider("openrouter".into()))
        );
    }

    #[test]
    fn config_round_trips_and_leniently_ignores_unknown_values() {
        let dir = std::env::temp_dir().join(format!("grokhub-voice-route-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(read(&dir), VoiceRouteConfig::default());
        let saved = VoiceRouteConfig {
            mode: VoiceRouteMode::Pinned,
            pinned: VoicePin {
                stt: Some(SpeechSource::Local),
                reply: Some("Grok_Voice_3".into()),
                tts: Some(SpeechSource::GrokVoice3),
            },
        };
        let written = write(&dir, saved).expect("write");
        assert_eq!(written.pinned.reply.as_deref(), Some("xai"));
        assert_eq!(read(&dir), written);

        std::fs::write(
            dir.join(FILE),
            r#"{"mode":"future_mode","pinned":{"stt":"future","reply":"anthropic","tts":"local"}}"#,
        )
        .unwrap();
        let lenient = read(&dir);
        assert_eq!(lenient.mode, VoiceRouteMode::Auto);
        assert_eq!(lenient.pinned.stt, None);
        assert_eq!(lenient.pinned.reply.as_deref(), Some("anthropic"));
        assert_eq!(lenient.pinned.tts, Some(SpeechSource::Local));
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
}
