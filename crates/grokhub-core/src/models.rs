//! Focused Grok catalog. Not the 1300-line website dump.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModelRow {
    pub id: &'static str,
    pub label: &'static str,
    pub kind: &'static str,
}

pub const MODEL_CATALOG: &[ModelRow] = &[
    ModelRow {
        id: "grok-3-mini-fast",
        label: "Grok 3 Mini Fast",
        kind: "chat",
    },
    ModelRow {
        id: "grok-4.3",
        label: "Grok 4.3",
        kind: "chat",
    },
    ModelRow {
        id: "grok-4.6",
        label: "Grok 4.6",
        kind: "chat",
    },
    ModelRow {
        id: "grok-4.7",
        label: "Grok 4.7",
        kind: "chat",
    },
    ModelRow {
        id: "grok-imagine-image-2.0",
        label: "Grok Imagine Image 2.0",
        kind: "imagine",
    },
    ModelRow {
        id: "grok-imagine-video-1.5",
        label: "Grok Imagine Video 1.5",
        kind: "imagine-video",
    },
    ModelRow {
        id: "grok-voice-3.0",
        label: "Grok Voice 3.0",
        kind: "voice",
    },
    ModelRow {
        id: "grok-voice-think-fast-2.0",
        label: "Grok Voice Think Fast 2.0",
        kind: "voice",
    },
    ModelRow {
        id: "grok-voice-latest",
        label: "Grok Voice (latest alias)",
        kind: "voice",
    },
];

/// Chat ids xAI retired on May 15, 2026. The slugs still answer, but xAI
/// serves every one of them with grok-4.3, so a saved pin on one maps there.
const RETIRED_CHAT: &[&str] = &[
    "grok-3",
    "grok-4",
    "grok-4-latest",
    "grok-4-0709",
    "grok-4-fast-reasoning",
    "grok-4-fast-non-reasoning",
    "grok-4-1-fast-reasoning",
    "grok-4-1-fast-non-reasoning",
];

/// What xAI serves a retired chat id with now.
const RETIRED_CHAT_TARGET: &str = "grok-4.3";

pub fn sanitize_chat_model(id: &str) -> &'static str {
    let t = id.trim();
    if RETIRED_CHAT.iter().any(|r| r.eq_ignore_ascii_case(t)) {
        return RETIRED_CHAT_TARGET;
    }
    MODEL_CATALOG
        .iter()
        .find(|m| m.kind == "chat" && m.id == t)
        .map(|m| m.id)
        .unwrap_or(crate::DEFAULT_MODEL)
}

pub fn catalog_line() -> String {
    MODEL_CATALOG
        .iter()
        .map(|m| format!("{} — {} ({})", m.id, m.label, m.kind))
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog() {
        for retired in [
            "grok-3",
            "grok-4-latest",
            " grok-4-1-fast-non-reasoning ",
            "GROK-4-0709",
        ] {
            assert_eq!(sanitize_chat_model(retired), "grok-4.3", "{retired}");
            let id = retired.trim().to_ascii_lowercase();
            assert!(MODEL_CATALOG.iter().all(|m| m.id != id), "{retired}");
        }
        assert!(MODEL_CATALOG.iter().any(|m| m.id == RETIRED_CHAT_TARGET));
        assert_eq!(sanitize_chat_model("grok-4.6"), "grok-4.6");
        assert_eq!(sanitize_chat_model("grok-4.7"), "grok-4.7");
        assert_eq!(sanitize_chat_model("grok-4.3"), "grok-4.3");
        assert_eq!(sanitize_chat_model("nope"), crate::DEFAULT_MODEL);
        assert!(catalog_line().contains("grok-4.6"));
        assert!(catalog_line().contains("grok-4.7"));
        assert!(catalog_line().contains("grok-4.3"));
        assert!(catalog_line().contains("Grok 4.3"));
        assert!(catalog_line().contains("Grok 4.6"));
        assert!(catalog_line().contains("Grok 4.7"));
        assert!(!catalog_line().contains("Grok 4.6 xhigh"));
        assert!(!catalog_line().contains("Grok 4.7 xhigh"));
        assert!(catalog_line().contains("grok-voice-3.0"));
        assert!(catalog_line().contains("grok-voice-think-fast-2.0"));
        assert!(catalog_line().contains("grok-voice-latest"));
        assert!(catalog_line().contains("grok-imagine-image-2.0"));
        assert!(catalog_line().contains("grok-imagine-video-1.5"));
        assert!(!catalog_line().contains("grok-2-image"));
    }
}
