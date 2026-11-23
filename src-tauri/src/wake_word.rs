//! Wake word settings and on-device feed (Order 970).
//!
//! Preferences live in `wake_word.json`. The shell only accepts local
//! transcript snippets for matching — never cloud STT for always-listen.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use grokhub_core::{
    effective_phrases, effective_stop, set_default_profile, set_enabled, set_mic_consent,
    set_phrases, set_routes, set_stop_phrases, WakeAction, WakeConfig, WakeListener, WakePhase,
    WakeRoute, MIC_OFF,
};
use serde::{Deserialize, Serialize};

const FILE: &str = "wake_word.json";
const STORE_CAP: u64 = 256 * 1024;

static STORE: Mutex<()> = Mutex::new(());
static LISTENER: Mutex<Option<(PathBuf, WakeListener)>> = Mutex::new(None);

fn store() -> MutexGuard<'static, ()> {
    STORE.lock().unwrap_or_else(|err| err.into_inner())
}

fn path(config_dir: &Path) -> PathBuf {
    config_dir.join(FILE)
}

#[cfg(test)]
fn reset_listener() {
    let mut slot = LISTENER.lock().unwrap_or_else(|err| err.into_inner());
    *slot = None;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RouteView {
    pub phrase: String,
    pub profile_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Page {
    pub mic_consent: bool,
    pub enabled: bool,
    pub phrases: Vec<String>,
    pub stop_phrases: Vec<String>,
    pub routes: Vec<RouteView>,
    pub default_profile: String,
    pub phase: String,
    pub voice_busy: bool,
    /// Built-in defaults shown when the saved lists are empty.
    pub default_phrases: Vec<String>,
    pub default_stop: Vec<String>,
    /// Privacy line for Settings.
    pub privacy: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FeedResult {
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phrase: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile_id: Option<String>,
    pub phase: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum Act {
    SetMicConsent { on: bool },
    SetEnabled { on: bool },
    SetPhrases { phrases: Vec<String> },
    SetStopPhrases { phrases: Vec<String> },
    SetRoutes { routes: Vec<RouteIn> },
    SetDefaultProfile { profile_id: String },
    Arm,
    Disarm,
    SetVoiceBusy { busy: bool },
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RouteIn {
    pub phrase: String,
    #[serde(default)]
    pub profile_id: String,
}

fn phase_label(phase: WakePhase) -> &'static str {
    match phase {
        WakePhase::Off => "off",
        WakePhase::Armed => "armed",
        WakePhase::InTurn => "in_turn",
    }
}

fn page_of(listen: &WakeListener) -> Page {
    let cfg = &listen.config;
    Page {
        mic_consent: cfg.consent.mic,
        enabled: cfg.enabled,
        phrases: cfg.phrases.clone(),
        stop_phrases: cfg.stop_phrases.clone(),
        routes: cfg
            .routes
            .iter()
            .map(|r| RouteView {
                phrase: r.phrase.clone(),
                profile_id: r.profile_id.clone(),
            })
            .collect(),
        default_profile: cfg.default_profile.clone(),
        phase: phase_label(listen.phase()).to_string(),
        voice_busy: listen.voice_busy(),
        default_phrases: effective_phrases(&WakeConfig::default()),
        default_stop: effective_stop(&WakeConfig::default()),
        privacy: "Wake listening stays on this computer. Mic access is opt-in; audio for wake matching is never sent to the cloud."
            .into(),
    }
}

fn feed_view(action: WakeAction, phase: WakePhase) -> FeedResult {
    match action {
        WakeAction::None => FeedResult {
            kind: "none".into(),
            phrase: None,
            profile_id: None,
            phase: phase_label(phase).into(),
        },
        WakeAction::StartTurn {
            phrase,
            profile_id,
        } => FeedResult {
            kind: "start_turn".into(),
            phrase: Some(phrase),
            profile_id: Some(profile_id),
            phase: phase_label(phase).into(),
        },
        WakeAction::Stop => FeedResult {
            kind: "stop".into(),
            phrase: None,
            profile_id: None,
            phase: phase_label(phase).into(),
        },
    }
}

fn load_unlocked(config_dir: &Path) -> Result<WakeConfig, String> {
    let path = path(config_dir);
    if !path.is_file() {
        return Ok(WakeConfig::default());
    }
    let len = std::fs::metadata(&path).map_err(|e| e.to_string())?.len();
    if len > STORE_CAP {
        return Err(format!("{FILE} is too large to be wake word settings."));
    }
    let raw = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    if raw.trim().is_empty() {
        return Ok(WakeConfig::default());
    }
    serde_json::from_str(&raw).map_err(|e| format!("{FILE}: {e}"))
}

fn save(config_dir: &Path, cfg: &WakeConfig) -> Result<(), String> {
    std::fs::create_dir_all(config_dir).map_err(|e| e.to_string())?;
    let raw = serde_json::to_string_pretty(cfg).map_err(|e| e.to_string())?;
    let tmp = config_dir.join(format!("{FILE}.shell-{}.tmp", std::process::id()));
    std::fs::write(&tmp, raw).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, path(config_dir)).map_err(|e| e.to_string())
}

fn with_listener<F, T>(config_dir: &Path, f: F) -> Result<T, String>
where
    F: FnOnce(&mut WakeListener) -> Result<T, String>,
{
    let _g = store();
    let mut slot = LISTENER.lock().unwrap_or_else(|err| err.into_inner());
    let need_new = match slot.as_ref() {
        Some((dir, _)) => dir != config_dir,
        None => true,
    };
    if need_new {
        let cfg = load_unlocked(config_dir)?;
        *slot = Some((config_dir.to_path_buf(), WakeListener::new(cfg)));
    }
    let listen = slot
        .as_mut()
        .map(|(_, l)| l)
        .ok_or_else(|| MIC_OFF.to_string())?;
    f(listen)
}

/// Load wake word settings and listener phase.
pub fn load(config_dir: &Path) -> Result<Page, String> {
    with_listener(config_dir, |listen| {
        // Refresh config from disk so another process save is visible.
        listen.config = load_unlocked(config_dir)?;
        Ok(page_of(listen))
    })
}

pub fn act(config_dir: &Path, act: Act) -> Result<Page, String> {
    with_listener(config_dir, |listen| {
        listen.config = load_unlocked(config_dir)?;
        match act {
            Act::SetMicConsent { on } => set_mic_consent(&mut listen.config, on),
            Act::SetEnabled { on } => set_enabled(&mut listen.config, on).map_err(|e| e.to_string())?,
            Act::SetPhrases { phrases } => set_phrases(&mut listen.config, &phrases),
            Act::SetStopPhrases { phrases } => set_stop_phrases(&mut listen.config, &phrases),
            Act::SetRoutes { routes } => {
                let mapped: Vec<WakeRoute> = routes
                    .into_iter()
                    .map(|r| WakeRoute {
                        phrase: r.phrase,
                        profile_id: r.profile_id,
                    })
                    .collect();
                set_routes(&mut listen.config, &mapped);
            }
            Act::SetDefaultProfile { profile_id } => {
                set_default_profile(&mut listen.config, &profile_id)
            }
            Act::Arm => {
                listen.arm().map_err(|e| e.to_string())?;
            }
            Act::Disarm => listen.disarm(),
            Act::SetVoiceBusy { busy } => listen.set_voice_busy(busy),
        }
        // Turning listen off or clearing consent drops the phase.
        if !listen.config.enabled || !listen.config.consent.mic {
            listen.disarm();
        }
        save(config_dir, &listen.config)?;
        Ok(page_of(listen))
    })
}

/// Feed one **on-device** transcript. Callers must not pass cloud STT text.
pub fn feed_local(config_dir: &Path, text: &str) -> Result<FeedResult, String> {
    with_listener(config_dir, |listen| {
        listen.config = load_unlocked(config_dir)?;
        let action = listen.feed_local(text).map_err(|e| e.to_string())?;
        Ok(feed_view(action, listen.phase()))
    })
}

/// Pause or resume wake matching when open-mic / push-to-talk owns the mic.
/// No-op when the listener was never loaded.
pub fn set_voice_busy_flag(busy: bool) {
    let mut slot = LISTENER.lock().unwrap_or_else(|err| err.into_inner());
    if let Some((_, listen)) = slot.as_mut() {
        listen.set_voice_busy(busy);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_config(tag: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "grokhub-wake-{}-{}-{}",
            tag,
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        // Fresh listener slot per test process is shared; clear by disarming via act.
        root
    }

    #[test]
    fn consent_arm_feed_and_stop_round_trip() {
        reset_listener();
        let root = temp_config("round");
        let page = load(&root).unwrap();
        assert!(!page.mic_consent);
        assert!(!page.enabled);
        assert_eq!(page.phase, "off");

        act(&root, Act::SetMicConsent { on: true }).unwrap();
        act(&root, Act::SetEnabled { on: true }).unwrap();
        act(
            &root,
            Act::SetPhrases {
                phrases: vec!["hey cabin".into()],
            },
        )
        .unwrap();
        act(
            &root,
            Act::SetRoutes {
                routes: vec![RouteIn {
                    phrase: "hey cabin".into(),
                    profile_id: "nyx".into(),
                }],
            },
        )
        .unwrap();
        let page = act(&root, Act::Arm).unwrap();
        assert_eq!(page.phase, "armed");

        let hit = feed_local(&root, "Hey cabin, status").unwrap();
        assert_eq!(hit.kind, "start_turn");
        assert_eq!(hit.phrase.as_deref(), Some("hey cabin"));
        assert_eq!(hit.profile_id.as_deref(), Some("nyx"));
        assert_eq!(hit.phase, "in_turn");

        let stop = feed_local(&root, "that's enough").unwrap();
        assert_eq!(stop.kind, "stop");
        assert_eq!(stop.phase, "armed");

        let busy = act(&root, Act::SetVoiceBusy { busy: true }).unwrap();
        assert!(busy.voice_busy);
        let err = feed_local(&root, "hey cabin").unwrap_err();
        assert!(err.contains("paused") || err.contains("owns the mic"));
        act(&root, Act::SetVoiceBusy { busy: false }).unwrap();
        act(&root, Act::Disarm).unwrap();
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn enable_without_mic_fails() {
        reset_listener();
        let root = temp_config("nomic");
        let err = act(&root, Act::SetEnabled { on: true }).unwrap_err();
        assert!(err.contains("mic") || err.contains("Mic"));
        let _ = std::fs::remove_dir_all(&root);
    }
}
