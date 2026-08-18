//! Focused Grok catalog. Not the 1300-line website dump.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModelRow {
    pub id: &'static str,
    pub label: &'static str,
    pub kind: &'static str,
}

pub const MODEL_CATALOG: &[ModelRow] = &[
    ModelRow { id: "grok-3-mini-fast", label: "Grok 3 Mini Fast", kind: "chat" },
    ModelRow { id: "grok-3", label: "Grok 3", kind: "chat" },
    ModelRow { id: "grok-4.3", label: "Grok 4.3", kind: "chat" },
    ModelRow { id: "grok-4.6", label: "Grok 4.6", kind: "chat" },
    ModelRow { id: "grok-4-latest", label: "Grok 4", kind: "chat" },
    ModelRow { id: "grok-4-1-fast-non-reasoning", label: "Grok 4.1 Fast", kind: "chat" },
    ModelRow { id: "grok-imagine-image-2.0", label: "Grok Imagine Image 2.0", kind: "imagine" },
    ModelRow { id: "grok-imagine-video-1.5", label: "Grok Imagine Video 1.5", kind: "imagine-video" },
    ModelRow { id: "grok-voice-think-fast-2.0", label: "Grok Voice Think Fast 2.0", kind: "voice" },
    ModelRow { id: "grok-voice-latest", label: "Grok Voice (latest alias)", kind: "voice" },
];

pub fn sanitize_chat_model(id: &str) -> &'static str {
    let t = id.trim();
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
        assert_eq!(sanitize_chat_model("grok-3"), "grok-3");
        assert_eq!(sanitize_chat_model("grok-4.6"), "grok-4.6");
        assert_eq!(sanitize_chat_model("grok-4.3"), "grok-4.3");
        assert_eq!(sanitize_chat_model("nope"), crate::DEFAULT_MODEL);
        assert!(catalog_line().contains("grok-4.6"));
        assert!(catalog_line().contains("grok-4.3"));
        assert!(catalog_line().contains("Grok 4.3"));
        assert!(catalog_line().contains("Grok 4.6"));
        assert!(!catalog_line().contains("Grok 4.6 xhigh"));
        assert!(catalog_line().contains("grok-voice-think-fast-2.0"));
        assert!(catalog_line().contains("grok-voice-latest"));
        assert!(catalog_line().contains("grok-imagine-image-2.0"));
        assert!(catalog_line().contains("grok-imagine-video-1.5"));
        assert!(!catalog_line().contains("grok-2-image"));
    }
}
