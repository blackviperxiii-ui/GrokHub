//! Always-listening on-device wake word (Order 970).
//!
//! Mic stay opt-in. Matching runs on local transcripts only — no cloud wake
//! listening. Complements push-to-talk and open-mic; does not replace them.

use serde::{Deserialize, Serialize};

/// Refuse enable / feed until the person allows the mic for wake word.
pub const MIC_OFF: &str =
    "Wake word mic is off. Allow the mic for wake word in Settings → Local AI, then try again.";
/// Refuse enable when the feature toggle is off.
pub const LISTEN_OFF: &str = "Wake word listening is off.";
/// Refuse feed while push-to-talk or open-mic owns the mic.
pub const VOICE_BUSY: &str = "Wake word is paused while voice mode or push-to-talk owns the mic.";
/// Empty phrase list after normalize.
pub const NO_PHRASES: &str = "Add at least one wake phrase.";

/// Cap on open-vocabulary wake phrases.
pub const MAX_PHRASES: usize = 32;
/// Cap on profile routes.
pub const MAX_ROUTES: usize = 32;
/// Cap on hands-free stop phrases.
pub const MAX_STOP: usize = 16;
/// Max UTF-8 bytes kept per phrase after trim.
pub const PHRASE_MAX: usize = 64;

/// Built-in wake phrases when the saved list is empty.
pub const DEFAULT_PHRASES: &[&str] = &["hey loadout", "hey grok"];
/// Built-in hands-free stop phrases.
pub const DEFAULT_STOP: &[&str] = &["stop listening", "that's enough", "cancel"];

/// Explicit mic grant for always-listen. Starts off; only a user click turns it on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct WakeConsent {
    #[serde(default)]
    pub mic: bool,
}

/// One open-vocabulary phrase → personality / voice profile id.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WakeRoute {
    pub phrase: String,
    /// Personality id (or empty for the cabin default / current profile).
    #[serde(default)]
    pub profile_id: String,
}

/// Saved wake-word preferences under the cabin config dir.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct WakeConfig {
    #[serde(default)]
    pub consent: WakeConsent,
    /// Always-listen toggle. Needs [`WakeConsent::mic`] before it can arm.
    #[serde(default)]
    pub enabled: bool,
    /// Open-vocabulary wake phrases. Empty means the built-in defaults.
    #[serde(default)]
    pub phrases: Vec<String>,
    /// Phrase → profile routes. Unmatched wakes use [`WakeConfig::default_profile`].
    #[serde(default)]
    pub routes: Vec<WakeRoute>,
    /// Hands-free stop phrases while a wake-started turn is live.
    #[serde(default)]
    pub stop_phrases: Vec<String>,
    /// Profile used when no route matches. Empty = current / default personality.
    #[serde(default)]
    pub default_profile: String,
}

/// What feeding a local transcript did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WakeAction {
    None,
    /// A wake phrase matched; start a turn without a button press.
    StartTurn {
        phrase: String,
        profile_id: String,
    },
    /// A stop phrase matched during a wake-started turn.
    Stop,
}

/// Listener phase. Pure — the shell owns the mic and on-device STT.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WakePhase {
    #[default]
    Off,
    /// Consent + enabled; waiting for a wake phrase on local transcripts.
    Armed,
    /// A wake started a turn; listening for hands-free stop.
    InTurn,
}

/// On-device wake listener. The shell feeds **local** transcript snippets only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WakeListener {
    pub config: WakeConfig,
    phase: WakePhase,
    /// Push-to-talk or open-mic currently owns the mic.
    voice_busy: bool,
}

impl Default for WakeListener {
    fn default() -> Self {
        Self::new(WakeConfig::default())
    }
}

impl WakeListener {
    pub fn new(config: WakeConfig) -> Self {
        Self {
            config,
            phase: WakePhase::Off,
            voice_busy: false,
        }
    }

    pub fn phase(&self) -> WakePhase {
        self.phase
    }

    pub fn voice_busy(&self) -> bool {
        self.voice_busy
    }

    /// Mark whether open-mic / push-to-talk owns the mic. Wake pauses while busy.
    pub fn set_voice_busy(&mut self, busy: bool) {
        self.voice_busy = busy;
        if busy && self.phase == WakePhase::Armed {
            // Stay enabled in config; just stop matching until voice releases.
        }
    }

    /// Arm always-listen. Needs mic consent and the enabled toggle.
    pub fn arm(&mut self) -> Result<(), &'static str> {
        if !self.config.consent.mic {
            return Err(MIC_OFF);
        }
        if !self.config.enabled {
            return Err(LISTEN_OFF);
        }
        if effective_phrases(&self.config).is_empty() {
            return Err(NO_PHRASES);
        }
        self.phase = WakePhase::Armed;
        Ok(())
    }

    /// Disarm without clearing consent or phrases.
    pub fn disarm(&mut self) {
        self.phase = WakePhase::Off;
    }

    /// After a StartTurn was acted on, mark the turn live for stop phrases.
    pub fn mark_in_turn(&mut self) {
        if self.phase == WakePhase::Armed || self.phase == WakePhase::InTurn {
            self.phase = WakePhase::InTurn;
        }
    }

    /// Feed one **on-device** transcript snippet. Cloud STT must never call this.
    pub fn feed_local(&mut self, text: &str) -> Result<WakeAction, &'static str> {
        if !self.config.consent.mic {
            return Err(MIC_OFF);
        }
        if !self.config.enabled {
            return Err(LISTEN_OFF);
        }
        if self.voice_busy {
            return Err(VOICE_BUSY);
        }
        match self.phase {
            WakePhase::Off => Ok(WakeAction::None),
            WakePhase::Armed => {
                if let Some(phrase) = match_phrase(text, &effective_phrases(&self.config)) {
                    let profile_id = route_profile(&self.config, &phrase);
                    self.phase = WakePhase::InTurn;
                    Ok(WakeAction::StartTurn {
                        phrase,
                        profile_id,
                    })
                } else {
                    Ok(WakeAction::None)
                }
            }
            WakePhase::InTurn => {
                if match_phrase(text, &effective_stop(&self.config)).is_some() {
                    self.phase = WakePhase::Armed;
                    Ok(WakeAction::Stop)
                } else {
                    Ok(WakeAction::None)
                }
            }
        }
    }
}

/// Turn mic consent on or off. Call only from a user click path.
pub fn set_mic_consent(cfg: &mut WakeConfig, on: bool) {
    cfg.consent.mic = on;
    if !on {
        cfg.enabled = false;
    }
}

/// Turn always-listen on or off. Needs mic consent to enable.
pub fn set_enabled(cfg: &mut WakeConfig, on: bool) -> Result<(), &'static str> {
    if on && !cfg.consent.mic {
        return Err(MIC_OFF);
    }
    cfg.enabled = on;
    Ok(())
}

/// Replace the open-vocabulary wake phrase list (normalized, capped).
pub fn set_phrases(cfg: &mut WakeConfig, phrases: &[String]) {
    cfg.phrases = normalize_list(phrases, MAX_PHRASES);
}

/// Replace hands-free stop phrases (normalized, capped).
pub fn set_stop_phrases(cfg: &mut WakeConfig, phrases: &[String]) {
    cfg.stop_phrases = normalize_list(phrases, MAX_STOP);
}

/// Replace phrase → profile routes (normalized triggers, capped).
pub fn set_routes(cfg: &mut WakeConfig, routes: &[WakeRoute]) {
    let mut out = Vec::new();
    let mut seen = Vec::new();
    for route in routes {
        let phrase = normalize_phrase(&route.phrase);
        if phrase.is_empty() || phrase.len() > PHRASE_MAX {
            continue;
        }
        if seen.iter().any(|s: &String| s == &phrase) {
            continue;
        }
        seen.push(phrase.clone());
        out.push(WakeRoute {
            phrase,
            profile_id: route.profile_id.trim().to_string(),
        });
        if out.len() >= MAX_ROUTES {
            break;
        }
    }
    cfg.routes = out;
}

/// Set the default profile id for unmatched wakes.
pub fn set_default_profile(cfg: &mut WakeConfig, profile_id: &str) {
    cfg.default_profile = profile_id.trim().to_string();
}

/// Phrases used for matching: saved list, or built-in defaults when empty.
pub fn effective_phrases(cfg: &WakeConfig) -> Vec<String> {
    if cfg.phrases.is_empty() {
        DEFAULT_PHRASES
            .iter()
            .map(|s| (*s).to_string())
            .collect()
    } else {
        cfg.phrases.clone()
    }
}

/// Stop phrases used for matching: saved list, or built-in defaults when empty.
pub fn effective_stop(cfg: &WakeConfig) -> Vec<String> {
    if cfg.stop_phrases.is_empty() {
        DEFAULT_STOP.iter().map(|s| (*s).to_string()).collect()
    } else {
        cfg.stop_phrases.clone()
    }
}

/// Lowercase, strip leading/trailing punctuation per word, collapse spaces.
pub fn normalize_phrase(raw: &str) -> String {
    let mut words = Vec::new();
    for part in raw.split_whitespace() {
        let trimmed: String = part
            .chars()
            .map(|c| c.to_ascii_lowercase())
            .collect::<String>()
            .trim_matches(|c: char| !c.is_alphanumeric())
            .to_string();
        if !trimmed.is_empty() {
            words.push(trimmed);
        }
    }
    words.join(" ")
}

fn normalize_list(phrases: &[String], cap: usize) -> Vec<String> {
    let mut out = Vec::new();
    for raw in phrases {
        let n = normalize_phrase(raw);
        if n.is_empty() || n.len() > PHRASE_MAX {
            continue;
        }
        if out.iter().any(|s: &String| s == &n) {
            continue;
        }
        out.push(n);
        if out.len() >= cap {
            break;
        }
    }
    out
}

/// True when `hay` contains the normalized `needle` as a whole-phrase run
/// (start, end, or bounded by spaces).
pub fn phrase_in_text(hay: &str, needle: &str) -> bool {
    let hay = normalize_phrase(hay);
    let needle = normalize_phrase(needle);
    if needle.is_empty() {
        return false;
    }
    if hay == needle {
        return true;
    }
    if hay.starts_with(&format!("{needle} ")) {
        return true;
    }
    if hay.ends_with(&format!(" {needle}")) {
        return true;
    }
    hay.contains(&format!(" {needle} "))
}

/// First matching phrase in `candidates`, or None.
pub fn match_phrase(text: &str, candidates: &[String]) -> Option<String> {
    candidates
        .iter()
        .find(|p| phrase_in_text(text, p))
        .cloned()
}

/// Profile for a matched wake phrase.
pub fn route_profile(cfg: &WakeConfig, phrase: &str) -> String {
    let key = normalize_phrase(phrase);
    for route in &cfg.routes {
        if normalize_phrase(&route.phrase) == key {
            return route.profile_id.clone();
        }
    }
    cfg.default_profile.clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mic_consent_required_to_enable_and_arm() {
        let mut cfg = WakeConfig::default();
        assert_eq!(set_enabled(&mut cfg, true), Err(MIC_OFF));
        set_mic_consent(&mut cfg, true);
        set_enabled(&mut cfg, true).unwrap();
        assert!(cfg.enabled);

        let mut listen = WakeListener::new(cfg);
        listen.arm().unwrap();
        assert_eq!(listen.phase(), WakePhase::Armed);

        set_mic_consent(&mut listen.config, false);
        assert!(!listen.config.enabled);
        assert_eq!(listen.arm(), Err(MIC_OFF));
    }

    #[test]
    fn open_vocabulary_wake_starts_turn_with_profile_route() {
        let mut cfg = WakeConfig::default();
        set_mic_consent(&mut cfg, true);
        set_enabled(&mut cfg, true).unwrap();
        set_phrases(
            &mut cfg,
            &[
                "hey cabin".into(),
                "ok loadout".into(),
                "Hey, Cabin!!".into(), // duplicate after normalize
            ],
        );
        assert_eq!(cfg.phrases, vec!["hey cabin", "ok loadout"]);
        set_routes(
            &mut cfg,
            &[WakeRoute {
                phrase: "hey cabin".into(),
                profile_id: "nyx".into(),
            }],
        );
        set_default_profile(&mut cfg, "neutral");

        let mut listen = WakeListener::new(cfg);
        listen.arm().unwrap();
        assert_eq!(
            listen.feed_local("noise only").unwrap(),
            WakeAction::None
        );
        match listen.feed_local("…Hey, Cabin!! what is the weather").unwrap() {
            WakeAction::StartTurn {
                phrase,
                profile_id,
            } => {
                assert_eq!(phrase, "hey cabin");
                assert_eq!(profile_id, "nyx");
            }
            other => panic!("expected StartTurn, got {other:?}"),
        }
        assert_eq!(listen.phase(), WakePhase::InTurn);

        // Unmatched phrase uses default profile.
        listen.disarm();
        listen.arm().unwrap();
        match listen.feed_local("ok loadout tell me a joke").unwrap() {
            WakeAction::StartTurn { profile_id, .. } => {
                assert_eq!(profile_id, "neutral");
            }
            other => panic!("expected StartTurn, got {other:?}"),
        }
    }

    #[test]
    fn hands_free_stop_returns_to_armed() {
        let mut cfg = WakeConfig::default();
        set_mic_consent(&mut cfg, true);
        set_enabled(&mut cfg, true).unwrap();
        let mut listen = WakeListener::new(cfg);
        listen.arm().unwrap();
        listen.feed_local("hey loadout").unwrap();
        assert_eq!(listen.phase(), WakePhase::InTurn);
        assert_eq!(
            listen.feed_local("please stop listening now").unwrap(),
            WakeAction::Stop
        );
        assert_eq!(listen.phase(), WakePhase::Armed);
    }

    #[test]
    fn pauses_while_voice_busy_no_regression_path() {
        let mut cfg = WakeConfig::default();
        set_mic_consent(&mut cfg, true);
        set_enabled(&mut cfg, true).unwrap();
        let mut listen = WakeListener::new(cfg);
        listen.arm().unwrap();
        listen.set_voice_busy(true);
        assert_eq!(listen.feed_local("hey loadout").unwrap_err(), VOICE_BUSY);
        listen.set_voice_busy(false);
        match listen.feed_local("hey loadout").unwrap() {
            WakeAction::StartTurn { .. } => {}
            other => panic!("expected StartTurn, got {other:?}"),
        }
    }

    #[test]
    fn default_phrases_when_list_empty() {
        let cfg = WakeConfig::default();
        assert_eq!(
            effective_phrases(&cfg),
            vec!["hey loadout".to_string(), "hey grok".to_string()]
        );
        assert!(phrase_in_text("Hey Grok, status?", "hey grok"));
        assert!(!phrase_in_text("heyloadout", "hey loadout"));
    }

    #[test]
    fn clearing_mic_consent_disables_listen() {
        let mut cfg = WakeConfig::default();
        set_mic_consent(&mut cfg, true);
        set_enabled(&mut cfg, true).unwrap();
        set_mic_consent(&mut cfg, false);
        assert!(!cfg.enabled);
        assert!(!cfg.consent.mic);
    }
}
