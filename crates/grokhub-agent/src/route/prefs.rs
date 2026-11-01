//! Per-model preferences: Settings → AI models → "Models Auto may use".
//!
//! For every model Auto can see (xAI, Anthropic, OpenRouter, the on-device
//! tiers) you say where Auto may route it: everything, chats, coding and
//! plans, background work, or never. [`ModelUse::Default`] keeps today's
//! rules: xAI and local models are Auto's pool, and a model on a provider you
//! added goes only when you picked it ("Use for your chats"), a pin or a
//! routing-table row names it. A pin and the pick always win over a
//! preference. An effort preference pins the reasoning effort Auto sends that
//! model, inside the effort list its listing names.
//!
//! "Auto: all models" ([`ModelPrefs::all`]) makes every model on a provider
//! you added part of Auto's pool, ranked with xAI and the device per step,
//! unless its own preference says otherwise. Utility models (safety
//! classifiers, embeddings, rerankers) never join on their own.
//!
//! The cabin keeps the map in `app.json` (`modelPrefs`, the switch as
//! `autoAllModels`) and hands both to the router with [`set_model_prefs`];
//! [`super::live::decide`] reads them per call.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

/// Where Auto may send a model on its own.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelUse {
    /// Today's rules: Auto's pool for xAI and local, only when picked for a provider.
    #[default]
    Default,
    /// Every class Auto routes.
    Any,
    /// Your chats (`chat:*`, `desktop:*`).
    Chat,
    /// Coding, plans, design, debugging, repair: everything that is not a chat
    /// or background work.
    Work,
    /// `background:*` only.
    Background,
    /// Never on Auto's own. A pin or the pick still reaches it.
    Never,
}

impl ModelUse {
    pub const ALL: [ModelUse; 6] = [
        ModelUse::Default,
        ModelUse::Any,
        ModelUse::Chat,
        ModelUse::Work,
        ModelUse::Background,
        ModelUse::Never,
    ];

    /// The `app.json` value.
    pub fn as_str(self) -> &'static str {
        match self {
            ModelUse::Default => "default",
            ModelUse::Any => "any",
            ModelUse::Chat => "chat",
            ModelUse::Work => "work",
            ModelUse::Background => "background",
            ModelUse::Never => "never",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s.trim() {
            "any" => ModelUse::Any,
            "chat" => ModelUse::Chat,
            "work" => ModelUse::Work,
            "background" => ModelUse::Background,
            "never" => ModelUse::Never,
            _ => ModelUse::Default,
        }
    }

    /// The Settings label. A provider model's default is "only when picked".
    pub fn label(self, new_provider: bool) -> &'static str {
        match self {
            ModelUse::Default if new_provider => "Only when you pick it",
            ModelUse::Default => "Default (Auto decides)",
            ModelUse::Any => "Everything",
            ModelUse::Chat => "Chats",
            ModelUse::Work => "Coding and plans",
            ModelUse::Background => "Background work",
            ModelUse::Never => "Never",
        }
    }
}

/// What a route class is to a preference.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClassGroup {
    Chat,
    Work,
    Background,
}

/// `chat:*` and `desktop:*` are chats, `background:*` is background work,
/// everything else (code, plan, design, debug, prepare, repair) is work.
pub fn class_group(class: &str) -> ClassGroup {
    let class = super::policy::class_row(class)
        .map(|r| r.class)
        .unwrap_or(class.trim());
    if class.starts_with("background:") {
        ClassGroup::Background
    } else if class.starts_with("chat:") || class.starts_with("desktop:") {
        ClassGroup::Chat
    } else {
        ClassGroup::Work
    }
}

/// One model's preference.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ModelPref {
    #[serde(rename = "use")]
    pub use_: ModelUse,
    /// The reasoning effort Auto sends this model ("" lets Auto pick).
    #[serde(skip_serializing_if = "String::is_empty")]
    pub effort: String,
}

impl ModelPref {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

/// Every model preference, by registry id (`grok-4.7`, `openrouter/openai/gpt-oss-120b`, `local:small`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ModelPrefs {
    pub models: BTreeMap<String, ModelPref>,
    /// "Auto: all models": a provider model with no preference of its own
    /// joins Auto's pool. Saved apart from the map (`autoAllModels`).
    #[serde(skip)]
    pub all: bool,
}

/// A model that answers something other than a chat (a safety classifier,
/// an embedding or rerank model): "Auto: all models" leaves it out unless
/// its own preference lets Auto use it. Matched on whole words of the id
/// (`llama-guard-4`, `text-embedding-3`), so `vanguard-70b` stays a chat
/// model; the router also leaves out a model whose listing says it answers
/// in something other than text.
pub fn utility_model(id: &str) -> bool {
    const WORDS: [&str; 10] = [
        "safety",
        "guard",
        "moderation",
        "embed",
        "embedding",
        "embeddings",
        "rerank",
        "reranker",
        "shield",
        "shieldgemma",
    ];
    id.to_ascii_lowercase()
        .split(|c: char| !c.is_ascii_alphanumeric())
        .any(|w| WORDS.contains(&w))
}

impl ModelPrefs {
    /// No per-model preference (the "all models" switch is saved apart).
    pub fn is_empty(&self) -> bool {
        self.models.is_empty()
    }

    pub fn use_of(&self, id: &str) -> ModelUse {
        self.models
            .get(id.trim())
            .map_or(ModelUse::Default, |p| p.use_)
    }

    pub fn effort(&self, id: &str) -> Option<&str> {
        self.models
            .get(id.trim())
            .map(|p| p.effort.trim())
            .filter(|e| !e.is_empty())
    }

    /// Set where Auto may use `id`. A preference that is all default is dropped.
    pub fn set_use(&mut self, id: &str, use_: ModelUse) {
        let id = id.trim();
        let mut pref = self.models.get(id).cloned().unwrap_or_default();
        pref.use_ = use_;
        if pref.is_default() {
            self.models.remove(id);
        } else {
            self.models.insert(id.to_string(), pref);
        }
    }

    /// Set the effort Auto sends `id` ("" lets Auto pick).
    pub fn set_effort(&mut self, id: &str, effort: &str) {
        let id = id.trim();
        let mut pref = self.models.get(id).cloned().unwrap_or_default();
        pref.effort = effort.trim().to_string();
        if pref.is_default() {
            self.models.remove(id);
        } else {
            self.models.insert(id.to_string(), pref);
        }
    }

    /// Forget `id` altogether.
    pub fn remove(&mut self, id: &str) {
        self.models.remove(id.trim());
    }

    /// Auto may route a `class` step to `id` on its own. `new_provider` is a
    /// model on a provider you added, whose default is "only when picked"
    /// unless "Auto: all models" is on.
    pub fn auto_allows(&self, id: &str, class: &str, new_provider: bool) -> bool {
        match self.use_of(id) {
            ModelUse::Default => !new_provider || (self.all && !utility_model(id)),
            ModelUse::Never => false,
            ModelUse::Any => true,
            ModelUse::Chat => class_group(class) == ClassGroup::Chat,
            ModelUse::Work => class_group(class) == ClassGroup::Work,
            ModelUse::Background => class_group(class) == ClassGroup::Background,
        }
    }

    /// "Never": Auto leaves `id` out of every pool. Pins and the pick still reach it.
    pub fn excluded(&self, id: &str) -> bool {
        self.use_of(id) == ModelUse::Never
    }

    /// Ids with a preference, for Settings rows.
    pub fn ids(&self) -> impl Iterator<Item = &str> {
        self.models.keys().map(String::as_str)
    }
}

static PREFS: Mutex<Option<Arc<ModelPrefs>>> = Mutex::new(None);

/// The cabin sets this on start and whenever Settings changes it.
pub fn set_model_prefs(prefs: ModelPrefs) {
    *PREFS.lock().unwrap_or_else(|e| e.into_inner()) = Some(Arc::new(prefs));
}

/// The preferences as last set; empty until the cabin sets them.
pub fn model_prefs() -> Arc<ModelPrefs> {
    PREFS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_keep_todays_rules_and_a_preference_changes_them() {
        let mut p = ModelPrefs::default();
        assert!(p.is_empty());
        // xAI and local: Auto's pool. A provider model: only when picked.
        assert!(p.auto_allows("grok-4.7", "chat:default", false));
        assert!(p.auto_allows("local:small", "background:summarize", false));
        assert!(!p.auto_allows("openrouter/openai/gpt-oss-120b", "chat:default", true));
        // Let Auto use the provider model everywhere.
        p.set_use("openrouter/openai/gpt-oss-120b", ModelUse::Any);
        assert!(p.auto_allows("openrouter/openai/gpt-oss-120b", "chat:default", true));
        assert!(p.auto_allows("openrouter/openai/gpt-oss-120b", "plan", true));
        // Chats only: a plan step is work, a summary is background.
        p.set_use("openrouter/openai/gpt-oss-120b", ModelUse::Chat);
        assert!(p.auto_allows("openrouter/openai/gpt-oss-120b", "chat:quick", true));
        assert!(p.auto_allows("openrouter/openai/gpt-oss-120b", "desktop:soft", true));
        assert!(!p.auto_allows("openrouter/openai/gpt-oss-120b", "plan", true));
        assert!(!p.auto_allows(
            "openrouter/openai/gpt-oss-120b",
            "background:summarize",
            true
        ));
        // Work and background on an xAI model.
        p.set_use("grok-4.7", ModelUse::Work);
        assert!(p.auto_allows("grok-4.7", "code:multi-file", false));
        assert!(p.auto_allows("grok-4.7", "debug", false));
        assert!(!p.auto_allows("grok-4.7", "chat:default", false));
        p.set_use("grok-4.7", ModelUse::Background);
        assert!(p.auto_allows("grok-4.7", "background:classify", false));
        assert!(!p.auto_allows("grok-4.7", "code:multi-file", false));
        // Never takes it out of every pool.
        p.set_use("grok-4.7", ModelUse::Never);
        assert!(p.excluded("grok-4.7"));
        assert!(!p.auto_allows("grok-4.7", "background:classify", false));
        // Back to Default drops the entry; an effort alone keeps it.
        p.set_use("grok-4.7", ModelUse::Default);
        assert!(!p.excluded("grok-4.7"));
        p.set_effort("grok-4.7", "high");
        assert_eq!(p.effort("grok-4.7"), Some("high"));
        assert_eq!(p.use_of("grok-4.7"), ModelUse::Default);
        p.set_effort("grok-4.7", "");
        assert_eq!(p.effort("grok-4.7"), None);
        assert_eq!(
            p.ids().collect::<Vec<_>>(),
            vec!["openrouter/openai/gpt-oss-120b"]
        );
    }

    #[test]
    fn uses_round_trip_through_app_json_and_labels_name_the_provider_default() {
        let mut p = ModelPrefs::default();
        p.set_use("openrouter/openrouter/free", ModelUse::Chat);
        p.set_effort("grok-4.7", "xhigh");
        let json = serde_json::to_string(&p).unwrap();
        assert_eq!(
            json,
            r#"{"grok-4.7":{"use":"default","effort":"xhigh"},"openrouter/openrouter/free":{"use":"chat"}}"#
        );
        let back: ModelPrefs = serde_json::from_str(&json).unwrap();
        assert_eq!(back, p);
        for u in ModelUse::ALL {
            assert_eq!(ModelUse::parse(u.as_str()), u);
        }
        assert_eq!(ModelUse::parse("bogus"), ModelUse::Default);
        assert_eq!(ModelUse::Default.label(true), "Only when you pick it");
        assert_eq!(ModelUse::Default.label(false), "Default (Auto decides)");
        assert_eq!(ModelUse::Work.label(false), "Coding and plans");
        assert_eq!(class_group("chat:default"), ClassGroup::Chat);
        assert_eq!(class_group("desktop:soft"), ClassGroup::Chat);
        assert_eq!(class_group("background:redact"), ClassGroup::Background);
        assert_eq!(class_group("prepare:hard"), ClassGroup::Work);
        assert_eq!(class_group("code:edit-small"), ClassGroup::Work);
    }

    #[test]
    fn all_models_lets_auto_use_every_provider_model_but_utility_ones_and_never() {
        let mut p = ModelPrefs {
            all: true,
            ..ModelPrefs::default()
        };
        let or = "openrouter/nvidia/nemotron-3-ultra-550b-a55b:free";
        assert!(p.auto_allows(or, "chat:default", true));
        assert!(p.auto_allows(or, "plan", true));
        assert!(p.auto_allows("anthropic/claude-x", "code:multi-file", true));
        // A safety classifier or an embedding model is no chat model.
        assert!(!p.auto_allows(
            "openrouter/nvidia/nemotron-3.5-content-safety:free",
            "chat:default",
            true
        ));
        assert!(!p.auto_allows(
            "openrouter/openai/text-embedding-3-large",
            "background:summarize",
            true
        ));
        assert!(utility_model("openrouter/meta-llama/llama-guard-4-12b"));
        assert!(utility_model("openrouter/google/shieldgemma-9b"));
        assert!(!utility_model("grok-4.7"));
        // Whole words only: a chat model that merely contains one is kept.
        assert!(!utility_model("openrouter/vendor/vanguard-70b"));
        assert!(!utility_model("openrouter/vendor/guardian-chat"));
        assert!(!utility_model("openrouter/vendor/embedded-coder"));
        // Its own preference still wins: Never, or chats only.
        p.set_use(or, ModelUse::Never);
        assert!(!p.auto_allows(or, "chat:default", true));
        p.set_use(or, ModelUse::Chat);
        assert!(!p.auto_allows(or, "plan", true));
        // The switch is not part of the saved map.
        assert_eq!(
            serde_json::to_string(&p).unwrap(),
            format!(r#"{{"{or}":{{"use":"chat"}}}}"#)
        );
        let back: ModelPrefs = serde_json::from_str(r#"{"x/y":{"use":"any"}}"#).unwrap();
        assert!(!back.all);
        assert_eq!(back.use_of("x/y"), ModelUse::Any);
    }
}
