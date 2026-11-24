//! User-directed model/provider benching from chat or Settings.
//!
//! Benched models and providers persist in `{config_dir}/models/user_bench.json`
//! across restarts. Auto routing, outage stand-ins, helper scopes, and the
//! local/Claude Plan gates respect this list until the user unbenches an entry.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::local;

const FILE_NAME: &str = "user_bench.json";
const SCHEMA: u32 = 1;
const MAX_WORDS: usize = 12;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BenchEntry {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub benched_at_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserBench {
    #[serde(default = "default_schema")]
    pub schema: u32,
    #[serde(default)]
    pub entries: Vec<BenchEntry>,
}

fn default_schema() -> u32 {
    SCHEMA
}

impl Default for UserBench {
    fn default() -> Self {
        Self {
            schema: SCHEMA,
            entries: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BenchAction {
    Bench { id: String, label: String },
    Unbench { id: String, label: String },
    ClearAll,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BenchOutcome {
    pub action: BenchAction,
    pub message: String,
    pub changed: bool,
}

pub fn file_path(config_dir: &Path) -> PathBuf {
    config_dir.join("models").join(FILE_NAME)
}

pub fn load(config_dir: &Path) -> UserBench {
    let path = file_path(config_dir);
    let Ok(bytes) = std::fs::read(path) else {
        return UserBench::default();
    };
    let Ok(mut parsed) = serde_json::from_slice::<UserBench>(&bytes) else {
        return UserBench::default();
    };
    if parsed.schema == 0 {
        parsed.schema = SCHEMA;
    }
    parsed
}

pub fn save(config_dir: &Path, bench: &UserBench) -> Result<(), String> {
    let path = file_path(config_dir);
    let Some(parent) = path.parent() else {
        return Err("user_bench path has no parent".into());
    };
    std::fs::create_dir_all(parent).map_err(|e| format!("create models dir: {e}"))?;
    let json = serde_json::to_vec_pretty(bench).map_err(|e| format!("serialize bench: {e}"))?;
    let tmp = parent.join(format!(".{FILE_NAME}.{}.tmp", std::process::id()));
    std::fs::write(&tmp, json).map_err(|e| format!("write bench temp: {e}"))?;
    std::fs::rename(&tmp, &path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("rename bench file: {e}")
    })
}

impl UserBench {
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn blocks_local(&self) -> bool {
        self.entries.iter().any(|e| e.id == "local")
    }

    pub fn blocks_claude_plan(&self) -> bool {
        self.entries.iter().any(|e| e.id == "claude")
    }

    pub fn matches_model(&self, model_id: &str) -> bool {
        if self.entries.is_empty() {
            return false;
        }
        let lower = model_id.trim().to_ascii_lowercase();
        let is_local = local::is_local(&lower);
        self.entries
            .iter()
            .any(|e| entry_matches_model(&e.id, &lower, is_local))
    }

    pub fn matches_provider(&self, provider: &str) -> bool {
        if self.entries.is_empty() {
            return false;
        }
        let p = provider.trim().to_ascii_lowercase();
        self.entries.iter().any(|e| match e.id.as_str() {
            "local" => p == "local" || p == "local_sidecar",
            "grok" => p == "xai" || p == "x.ai" || p == "x-ai" || p == "grok" || p == "grok_build",
            "claude" => p == "anthropic" || p == "claude" || p == "claude_plan",
            "openrouter" => p == "openrouter",
            "openai" => p == "openai",
            "gemini" => p == "gemini" || p == "google",
            other => p == other,
        })
    }

    /// Add `id` to the bench. Returns `true` if newly added.
    pub fn bench(&mut self, id: String, label: String, now_ms: u64) -> bool {
        if self.entries.iter().any(|e| e.id.eq_ignore_ascii_case(&id)) {
            return false;
        }
        self.entries.push(BenchEntry {
            id: id.to_ascii_lowercase(),
            label,
            benched_at_ms: now_ms,
        });
        true
    }

    /// Remove any entry matching `id`. Returns the removed entries.
    pub fn unbench(&mut self, id: &str) -> Vec<BenchEntry> {
        let q = id.trim().to_ascii_lowercase();
        let mut removed = Vec::new();
        self.entries.retain(|e| {
            let matches = e.id == q
                || e.label.eq_ignore_ascii_case(&q)
                || entry_matches_model(&q, &e.id, e.id == "local")
                || entry_matches_model(&e.id, &q, q == "local");
            if matches {
                removed.push(e.clone());
                false
            } else {
                true
            }
        });
        removed
    }

    pub fn clear(&mut self) -> usize {
        let n = self.entries.len();
        self.entries.clear();
        n
    }
}

fn has_segment(haystack: &str, needle: &str) -> bool {
    haystack
        .split(['/', '-', '_', ':', '.', ' '])
        .any(|seg| seg == needle)
}

fn norm_punct(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '.' | '_' => '-',
            other => other.to_ascii_lowercase(),
        })
        .collect()
}

fn entry_matches_model(entry_id: &str, model_lower: &str, is_local: bool) -> bool {
    match entry_id {
        "local" => is_local,
        "grok" => {
            has_segment(model_lower, "grok")
                || has_segment(model_lower, "xai")
                || model_lower.contains("x-ai/")
                || model_lower.starts_with("grok-")
        }
        "claude" => model_lower.starts_with("anthropic/") || has_segment(model_lower, "claude"),
        "openrouter" => model_lower == "openrouter" || model_lower.starts_with("openrouter/"),
        "openai" => {
            model_lower == "openai"
                || model_lower.starts_with("openai/")
                || model_lower.starts_with("gpt-")
                || model_lower.starts_with("o3")
                || model_lower.starts_with("o4")
                || model_lower.starts_with("o1")
                || has_segment(model_lower, "o1")
                || has_segment(model_lower, "o3")
                || has_segment(model_lower, "o4")
        }
        "gpt" => model_lower.starts_with("gpt-") || has_segment(model_lower, "gpt"),
        "opus" | "sonnet" | "haiku" | "gemini" | "deepseek" | "qwen" | "llama" | "mistral"
        | "nemotron" | "minimax" | "kimi" | "glm" | "gemma" => has_segment(model_lower, entry_id),
        specific => {
            if model_lower == specific {
                return true;
            }
            if model_lower.ends_with(&format!("/{specific}")) {
                return true;
            }
            let ns = norm_punct(specific);
            let nm = norm_punct(model_lower);
            nm == ns || nm.ends_with(&format!("/{ns}"))
        }
    }
}

const KNOWN_FAMILY_PREFIXES: &[&str] = &[
    "grok",
    "claude",
    "opus",
    "sonnet",
    "haiku",
    "openrouter",
    "openai",
    "gpt",
    "gemini",
    "deepseek",
    "qwen",
    "llama",
    "mistral",
    "nemotron",
    "minimax",
    "kimi",
    "glm",
    "gemma",
    "local",
    "anthropic",
    "xai",
    "x-ai",
    "o1",
    "o3",
    "o4",
];

fn normalize_alias(clean: &str) -> Option<(&'static str, &'static str)> {
    match clean {
        "local" | "on-device" | "on device" | "device" | "offline" => {
            Some(("local", "Local models"))
        }
        "grok" | "xai" | "x.ai" | "x-ai" => Some(("grok", "Grok (xAI)")),
        "claude" | "anthropic" => Some(("claude", "Claude")),
        "opus" => Some(("opus", "Opus")),
        "sonnet" => Some(("sonnet", "Sonnet")),
        "haiku" => Some(("haiku", "Haiku")),
        "openrouter" | "open router" => Some(("openrouter", "OpenRouter")),
        "openai" | "chatgpt" => Some(("openai", "OpenAI")),
        "gpt" => Some(("gpt", "GPT")),
        "gemini" | "google gemini" => Some(("gemini", "Gemini")),
        "deepseek" => Some(("deepseek", "DeepSeek")),
        "qwen" => Some(("qwen", "Qwen")),
        "llama" => Some(("llama", "Llama")),
        "mistral" => Some(("mistral", "Mistral")),
        "nemotron" => Some(("nemotron", "Nemotron")),
        "minimax" => Some(("minimax", "MiniMax")),
        "kimi" => Some(("kimi", "Kimi")),
        "glm" => Some(("glm", "GLM")),
        "gemma" => Some(("gemma", "Gemma")),
        _ => None,
    }
}

/// Normalize a raw target phrase (e.g. `"the local model"`, `"Opus"`, `"grok 4.7"`)
/// into `(canonical_id, display_label)`. Optionally validates an explicit model ID
/// against `known_ids` from the active registry.
pub fn normalize_target(raw: &str, known_ids: &[&str]) -> Option<(String, String)> {
    let mut s = raw
        .trim()
        .trim_matches(|c: char| matches!(c, '`' | '"' | '\'' | '.' | '!' | ','))
        .trim()
        .to_ascii_lowercase();

    for art in ["the ", "any ", "my ", "a ", "an "] {
        if let Some(rest) = s.strip_prefix(art) {
            s = rest.trim().to_string();
        }
    }
    for suffix in [" models", " model", " provider", " api"] {
        if let Some(rest) = s.strip_suffix(suffix) {
            s = rest.trim().to_string();
        }
    }
    if s.is_empty() {
        return None;
    }

    if let Some((id, label)) = normalize_alias(&s) {
        return Some((id.to_string(), label.to_string()));
    }

    // Explicit match against active registry models (allows any downloaded/added model id).
    if let Some(exact) = known_ids.iter().find(|id| {
        id.eq_ignore_ascii_case(&s)
            || id
                .split('/')
                .next_back()
                .is_some_and(|tail| tail.eq_ignore_ascii_case(&s))
    }) {
        return Some((exact.to_ascii_lowercase(), (*exact).to_string()));
    }

    // Join multi-token versioned names like "grok 4.7", "claude opus 4.6", "local small".
    if s.contains(' ') {
        let parts: Vec<&str> = s.split_whitespace().collect();
        if (2..=4).contains(&parts.len()) && KNOWN_FAMILY_PREFIXES.contains(&parts[0]) {
            if parts[0] == "local" && parts.len() == 2 {
                let candidate = format!("local:{}", parts[1]);
                let is_local_tier = matches!(
                    parts[1],
                    "small" | "balanced" | "fast" | "deep" | "tiny" | "medium" | "large"
                ) || known_ids
                    .iter()
                    .any(|id| id.eq_ignore_ascii_case(&candidate));
                if is_local_tier {
                    return Some((candidate.clone(), candidate));
                }
                return None;
            }
            let joined = parts.join("-");
            return Some((joined.clone(), joined));
        }
        return None;
    }

    // Single-token model ID (e.g. grok-4.7, claude-sonnet-4-6, local:small, openrouter/vendor/model).
    let looks_like_id = s.starts_with("local:")
        || s.contains('/')
        || KNOWN_FAMILY_PREFIXES
            .iter()
            .any(|p| s.starts_with(&format!("{p}-")) || s.starts_with(&format!("{p}:")));
    if looks_like_id && s.len() >= 3 {
        return Some((s.clone(), s));
    }

    None
}

fn strip_politeness_and_punct(input: &str) -> Option<String> {
    let trimmed = input.trim();
    if trimmed.is_empty() || trimmed.contains('\n') || trimmed.contains('?') {
        return None;
    }
    if trimmed.split_whitespace().count() > MAX_WORDS {
        return None;
    }
    let lower = trimmed
        .trim_end_matches(['.', '!'])
        .trim()
        .to_ascii_lowercase();

    for q in [
        "why ", "how ", "what ", "when ", "where ", "which ", "who ", "does ", "did ", "is ",
        "are ",
    ] {
        if lower.starts_with(q) {
            return None;
        }
    }

    let mut cur = lower.as_str();
    loop {
        let before = cur;
        for lead in [
            "please ",
            "hey loadout, ",
            "hey loadout ",
            "loadout, ",
            "loadout ",
            "ok ",
            "okay ",
            "can you ",
            "could you ",
            "would you ",
            "from now on, ",
            "from now on ",
        ] {
            if let Some(rest) = cur.strip_prefix(lead) {
                cur = rest.trim_start();
            }
        }
        if cur == before {
            break;
        }
    }
    Some(cur.to_string())
}

/// Parse plain-word chat instructions to bench or unbench a model/provider.
/// Returns `None` for ordinary chat messages, questions, or non-model subjects.
pub fn parse_chat_command(text: &str) -> Option<BenchAction> {
    parse_chat_command_with_known(text, &[])
}

pub fn parse_chat_command_with_known(text: &str, known_ids: &[&str]) -> Option<BenchAction> {
    let s = strip_politeness_and_punct(text)?;

    if matches!(
        s.as_str(),
        "unbench all"
            | "unbench everything"
            | "use all models again"
            | "use everything again"
            | "clear benched models"
            | "reset benched models"
            | "enable all models"
            | "re-enable all models"
    ) {
        return Some(BenchAction::ClearAll);
    }

    // Unbench / re-enable forms (check first so `"start using ..."` / `"use ... again"` wins cleanly).
    let unbench_target = s
        .strip_prefix("unbench ")
        .or_else(|| s.strip_prefix("un-bench "))
        .or_else(|| {
            s.strip_prefix("you can use ")
                .and_then(|r| r.strip_suffix(" again"))
        })
        .or_else(|| {
            s.strip_prefix("use ")
                .and_then(|r| r.strip_suffix(" again"))
        })
        .or_else(|| {
            s.strip_prefix("start using ")
                .map(|r| r.strip_suffix(" again").unwrap_or(r))
        })
        .or_else(|| {
            s.strip_prefix("resume using ")
                .map(|r| r.strip_suffix(" again").unwrap_or(r))
        })
        .or_else(|| {
            s.strip_prefix("resume ")
                .map(|r| r.strip_suffix(" again").unwrap_or(r))
        })
        .or_else(|| s.strip_prefix("re-enable "))
        .or_else(|| s.strip_prefix("reenable "))
        .or_else(|| s.strip_prefix("enable "))
        .or_else(|| {
            s.strip_prefix("allow ")
                .map(|r| r.strip_suffix(" again").unwrap_or(r))
        })
        .or_else(|| s.strip_prefix("bring back "))
        .or_else(|| s.strip_prefix("unblock "))
        .or_else(|| s.strip_prefix("restore "))
        .or_else(|| s.strip_prefix("turn back on "))
        .or_else(|| {
            s.strip_prefix("turn ")
                .and_then(|r| r.strip_suffix(" back on"))
        });

    if let Some(raw) = unbench_target {
        if let Some((id, label)) = normalize_target(raw, known_ids) {
            return Some(BenchAction::Unbench { id, label });
        }
        return None;
    }

    // Bench / disable forms.
    let bench_target = s
        .strip_prefix("bench ")
        .or_else(|| s.strip_prefix("stop using "))
        .or_else(|| s.strip_prefix("quit using "))
        .or_else(|| s.strip_prefix("don't use "))
        .or_else(|| s.strip_prefix("dont use "))
        .or_else(|| s.strip_prefix("do not use "))
        .or_else(|| s.strip_prefix("never use "))
        .or_else(|| s.strip_prefix("avoid using "))
        .or_else(|| s.strip_prefix("avoid "))
        .or_else(|| s.strip_prefix("no more "))
        .or_else(|| s.strip_prefix("skip "))
        .or_else(|| s.strip_prefix("disable "))
        .or_else(|| s.strip_prefix("mute "))
        .or_else(|| s.strip_prefix("turn off "))
        .or_else(|| s.strip_prefix("pull "))
        .or_else(|| s.strip_prefix("take ").and_then(|r| r.strip_suffix(" off")));

    if let Some(raw) = bench_target {
        if let Some((id, label)) = normalize_target(raw, known_ids) {
            return Some(BenchAction::Bench { id, label });
        }
    }

    None
}

pub fn apply_action(bench: &mut UserBench, action: BenchAction, now_ms: u64) -> BenchOutcome {
    match action.clone() {
        BenchAction::Bench { id, label } => {
            let added = bench.bench(id, label.clone(), now_ms);
            let message = if added {
                format!(
                    "Benched {label}. Auto routing and helpers will skip it until you say to use it again or unbench it in Settings → AI models."
                )
            } else {
                format!("{label} is already benched.")
            };
            BenchOutcome {
                action,
                message,
                changed: added,
            }
        }
        BenchAction::Unbench { id, label } => {
            let removed = bench.unbench(&id);
            let message = if let Some(first) = removed.first() {
                if removed.len() == 1 {
                    format!(
                        "Unbenched {}. It is available to routing again.",
                        first.label
                    )
                } else {
                    let names: Vec<&str> = removed.iter().map(|e| e.label.as_str()).collect();
                    format!(
                        "Unbenched {}. They are available to routing again.",
                        names.join(", ")
                    )
                }
            } else {
                format!("{label} wasn't benched.")
            };
            BenchOutcome {
                action,
                message,
                changed: !removed.is_empty(),
            }
        }
        BenchAction::ClearAll => {
            let n = bench.clear();
            let message = if n > 0 {
                "Unbenched all models. All configured models are available to routing again."
                    .to_string()
            } else {
                "No models were benched.".to_string()
            };
            BenchOutcome {
                action,
                message,
                changed: n > 0,
            }
        }
    }
}

pub fn apply_and_save(
    config_dir: &Path,
    action: BenchAction,
    now_ms: u64,
) -> Result<BenchOutcome, String> {
    let mut bench = load(config_dir);
    let outcome = apply_action(&mut bench, action, now_ms);
    if outcome.changed {
        save(config_dir, &bench)?;
    }
    Ok(outcome)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_plain_word_bench_and_unbench_commands() {
        assert_eq!(
            parse_chat_command("stop using Grok"),
            Some(BenchAction::Bench {
                id: "grok".into(),
                label: "Grok (xAI)".into()
            })
        );
        assert_eq!(
            parse_chat_command("don't use the local model"),
            Some(BenchAction::Bench {
                id: "local".into(),
                label: "Local models".into()
            })
        );
        assert_eq!(
            parse_chat_command("use Opus again"),
            Some(BenchAction::Unbench {
                id: "opus".into(),
                label: "Opus".into()
            })
        );
        assert_eq!(
            parse_chat_command("Please turn off xAI!"),
            Some(BenchAction::Bench {
                id: "grok".into(),
                label: "Grok (xAI)".into()
            })
        );
        assert_eq!(
            parse_chat_command("re-enable local"),
            Some(BenchAction::Unbench {
                id: "local".into(),
                label: "Local models".into()
            })
        );
        assert_eq!(
            parse_chat_command("bench grok-4.7"),
            Some(BenchAction::Bench {
                id: "grok-4.7".into(),
                label: "grok-4.7".into()
            })
        );
        assert_eq!(
            parse_chat_command("unbench all"),
            Some(BenchAction::ClearAll)
        );
    }

    #[test]
    fn ignores_ordinary_chat_questions_and_non_model_phrases() {
        assert_eq!(parse_chat_command("don't use tabs"), None);
        assert_eq!(parse_chat_command("don't use local variables"), None);
        assert_eq!(parse_chat_command("stop using mocks in tests"), None);
        assert_eq!(parse_chat_command("why don't you use Opus?"), None);
        assert_eq!(parse_chat_command("how does Grok work?"), None);
        assert_eq!(parse_chat_command("disable linting on save"), None);
        assert_eq!(parse_chat_command("use spaces instead of tabs again"), None);
    }

    #[test]
    fn matches_models_and_persists_round_trip() {
        let dir = std::env::temp_dir().join(format!("loadout-user-bench-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);

        let a = parse_chat_command("stop using Grok").unwrap();
        let out = apply_and_save(&dir, a, 100).unwrap();
        assert!(out.changed);

        let b = parse_chat_command("don't use the local model").unwrap();
        assert!(apply_and_save(&dir, b, 200).unwrap().changed);

        let loaded = load(&dir);
        assert!(loaded.blocks_local());
        assert!(!loaded.blocks_claude_plan());
        assert!(loaded.matches_model("grok-4.7"));
        assert!(loaded.matches_model("openrouter/x-ai/grok-4"));
        assert!(loaded.matches_model("local:small"));
        assert!(!loaded.matches_model("anthropic/claude-opus-4-6"));
        assert!(!loaded.matches_model("openrouter/nvidia/nemotron-3.5-super-vl:free"));

        let c = parse_chat_command("stop using Claude").unwrap();
        apply_and_save(&dir, c, 300).unwrap();
        let loaded = load(&dir);
        assert!(loaded.blocks_claude_plan());
        assert!(loaded.matches_model("anthropic/claude-opus-4-6"));

        let u = parse_chat_command("use Claude again").unwrap();
        apply_and_save(&dir, u, 400).unwrap();
        let loaded = load(&dir);
        assert!(!loaded.blocks_claude_plan());
        assert!(!loaded.matches_model("anthropic/claude-opus-4-6"));
        assert!(loaded.matches_model("grok-4.7"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    struct StubLocal;

    impl super::super::local::LocalRuntime for StubLocal {
        fn classify(&self, _: &str, labels: &[&str]) -> Result<serde_json::Value, String> {
            Ok(serde_json::json!({ "label": labels[0], "score": 0.9 }))
        }
        fn summarize(&self, _: &str, _: u32) -> Result<serde_json::Value, String> {
            Ok(serde_json::json!({ "summary": "" }))
        }
        fn difficulty(&self, _: &str) -> Result<serde_json::Value, String> {
            Ok(serde_json::json!({ "d": 0.3 }))
        }
        fn chat_stream(
            &self,
            _: &[super::super::local::ChatMessage],
            on_delta: &mut dyn FnMut(&str),
        ) -> Result<String, String> {
            on_delta("local fallback answer");
            Ok("local fallback answer".into())
        }
    }

    struct FailingXai {
        err: crate::client::ClientError,
        calls: std::sync::atomic::AtomicUsize,
    }

    impl crate::client::ModelClient for FailingXai {
        fn stream(
            &self,
            _req: &crate::client::ResponsesRequest,
            _cancel: &crate::CancelToken,
            _sink: &mut dyn FnMut(crate::client::StreamEvent),
        ) -> Result<crate::client::TurnOutput, crate::client::ClientError> {
            self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Err(match &self.err {
                crate::client::ClientError::RateLimited {
                    retry_after_secs,
                    message,
                } => crate::client::ClientError::RateLimited {
                    retry_after_secs: *retry_after_secs,
                    message: message.clone(),
                },
                crate::client::ClientError::PaymentRequired { status, message } => {
                    crate::client::ClientError::PaymentRequired {
                        status: *status,
                        message: message.clone(),
                    }
                }
                other => crate::client::ClientError::Protocol(other.to_string()),
            })
        }
    }

    fn test_registry() -> grokhub_core::model_registry::Registry {
        use grokhub_core::model_registry::{
            Credential, Listing, ModelMeta, Prices, Registry, SourceKind,
        };
        let meta = |id: &str| ModelMeta {
            id: id.into(),
            context_length: Some(128_000),
            prices: Prices {
                prompt: Some(1_000),
                completion: Some(5_000),
                ..Prices::default()
            },
            efforts: Some(vec!["low".into(), "medium".into(), "high".into()]),
            tool_calling: Some(true),
            reasoning: Some(true),
            ..ModelMeta::default()
        };
        let mut reg = Registry::default();
        reg.entitlement.credential = Credential::Plan;
        reg.apply_refresh(
            &[
                Listing::new(SourceKind::XaiApi, vec![meta("grok-4.7")]),
                Listing::new(
                    SourceKind::Local,
                    super::super::local::TIERS
                        .iter()
                        .map(|t| super::super::local::tier_meta(t))
                        .collect(),
                ),
            ],
            &[],
            1,
        );
        reg
    }

    #[test]
    fn decide_helper_scope_and_all_benched_respect_user_bench() {
        let dir = crate::harness::test_dir("user-bench-decide");
        let _cfg = crate::perm::ConfigGuard::set(&dir);
        grokhub_core::model_registry::store::save_registry(&dir, &test_registry()).unwrap();
        let _local = super::super::local::ForceOn::with(Some(std::sync::Arc::new(StubLocal)));

        let call = super::super::live::RouteCall {
            provider: "xai",
            class: "chat:default",
            model: "grok-4.7",
            episode: "ep-ub-1",
            session: "s-ub-1",
            // A coding ask: light chat would go to the warm device by
            // default, and this test is about the bench, not that rule.
            text: "rename the routing helper in route.rs",
            ..super::super::live::RouteCall::default()
        };

        let first = super::super::live::decide(&dir, &call, 1_000_000);
        assert_eq!(first.send_model(call.model), "grok-4.7");

        // Benching Grok routes Auto + HelperScope turns away from Grok to local, noting why.
        let bench_grok = parse_chat_command("stop using Grok").unwrap();
        apply_and_save(&dir, bench_grok, 1_000_100).unwrap();

        let after_bench = super::super::live::decide(&dir, &call, 1_000_200);
        assert!(super::super::local::is_local(
            &after_bench.send_model(call.model)
        ));
        assert!(
            after_bench
                .route
                .rule_ids
                .iter()
                .any(|r| r == "local:cloud_benched"),
            "{:?}",
            after_bench.route.rule_ids
        );
        assert_eq!(
            super::super::local_live::routing_reason(&after_bench.route.rule_ids, None),
            "cloud models are benched"
        );

        {
            let _helper = super::super::live::HelperScope::enter();
            let helper_pick = super::super::live::decide(&dir, &call, 1_000_300);
            assert!(
                super::super::local::is_local(&helper_pick.send_model(call.model)),
                "helper must respect user bench: {}",
                helper_pick.send_model(call.model)
            );
        }

        // Also benching local leaves no eligible route and pauses clearly.
        let bench_local = parse_chat_command("don't use the local model").unwrap();
        apply_and_save(&dir, bench_local, 1_000_400).unwrap();
        let none_left = super::super::live::decide(&dir, &call, 1_000_500);
        assert!(none_left.paused());
        assert!(none_left
            .route
            .rule_ids
            .iter()
            .any(|r| r == "bench:all_benched"));

        // Unbenching Grok restores cloud routing.
        let unbench_grok = parse_chat_command("use Grok again").unwrap();
        apply_and_save(&dir, unbench_grok, 1_000_600).unwrap();
        let restored = super::super::live::decide(&dir, &call, 1_000_700);
        assert_eq!(restored.send_model(call.model), "grok-4.7");

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn server_denial_triggers_auto_failover_and_sets_local_routing_reason() {
        let dir = crate::harness::test_dir("user-bench-failover");
        let _cfg = crate::perm::ConfigGuard::set(&dir);
        grokhub_core::model_registry::store::save_registry(&dir, &test_registry()).unwrap();
        let _local = super::super::local::ForceOn::with(Some(std::sync::Arc::new(StubLocal)));

        let filler = "architecture notes ".repeat(3_800);
        let req = crate::client::ResponsesRequest {
            model: "grok-4.7".into(),
            conversation_id: "failover-chat-1".into(),
            input: vec![crate::client::InputItem::Message {
                role: "user".into(),
                content: vec![crate::client::ContentPart::InputText(format!(
                    "Compare these options carefully:\n{filler}"
                ))],
            }],
            effort: None,
            tools: Vec::new(),
            hosted_search: false,
            call_timeout: None,
            text_format: None,
        };

        let xai = FailingXai {
            err: crate::client::ClientError::RateLimited {
                retry_after_secs: None,
                message: "429 too many requests".into(),
            },
            calls: std::sync::atomic::AtomicUsize::new(0),
        };
        let mut shown = String::new();
        let mut thought = String::new();
        let out = super::super::live::stream_routed(
            &xai,
            &req,
            &crate::CancelToken::new(),
            &mut |ev| match ev {
                crate::client::StreamEvent::TextDelta(t) => shown.push_str(&t),
                crate::client::StreamEvent::ReasoningDelta(t) => thought.push_str(&t),
                crate::client::StreamEvent::RateWait(_) => {}
            },
            "chat:default",
        )
        .expect("server 429 triggers Auto fallback to local");
        assert_eq!(xai.calls.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!(out.text, "local fallback answer");
        assert_eq!(shown, "local fallback answer");
        assert!(
            thought.contains("too busy")
                || thought.contains("limit")
                || thought.contains("carrying on"),
            "expected handoff thought, got: {thought:?}"
        );

        // Cold load path when cloud models are benched emits loading note + routing reason.
        drop(_local);
        let _cold_local = super::super::local::ForceOn::with(None);
        apply_and_save(
            &dir,
            parse_chat_command("stop using Grok").unwrap(),
            1_000_900,
        )
        .unwrap();
        let mut cold_thought = String::new();
        let _ = super::super::live::stream_routed(
            &xai,
            &req,
            &crate::CancelToken::new(),
            &mut |ev| {
                if let crate::client::StreamEvent::ReasoningDelta(t) = ev {
                    cold_thought.push_str(&t);
                }
            },
            "chat:default",
        );
        assert!(
            cold_thought.contains("Reason: cloud models are benched."),
            "expected local loading note with routing reason, got: {cold_thought:?}"
        );

        let _ = std::fs::remove_dir_all(dir);
    }
}
