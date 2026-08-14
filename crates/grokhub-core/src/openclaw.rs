//! OpenClaw workspace → GrokHub. Credentials and sqlite are skipped.

use crate::is_plain_text;

pub const OPENCLAW_CORE: &[&str] = &[
    "AGENTS.md",
    "SOUL.md",
    "USER.md",
    "IDENTITY.md",
    "TOOLS.md",
    "HEARTBEAT.md",
    "MEMORY.md",
    "BOOT.md",
    "BOOTSTRAP.md",
];

pub fn default_openclaw_paths(home: &str) -> Vec<String> {
    let h = home.trim_end_matches('/');
    vec![
        format!("{h}/.openclaw/workspace"),
        format!("{h}/.openclaw/workspace-default"),
        format!("{h}/openclaw/workspace"),
    ]
}

pub fn is_openclaw_workspace(names: &[&str]) -> bool {
    names.iter().any(|n| {
        let u = n.to_ascii_uppercase();
        u == "SOUL.MD" || u == "AGENTS.MD" || u == "IDENTITY.MD"
    })
}

pub fn clip_import(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    format!("{}… [truncated {} chars]", &s[..max], s.len() - max)
}

pub fn import_memory_file(name: &str, content: &str) -> Option<(String, String)> {
    let n = name.rsplit('/').next().unwrap_or(name);
    if !OPENCLAW_CORE.iter().any(|c| c.eq_ignore_ascii_case(n)) && !n.ends_with(".md") {
        return None;
    }
    if n.eq_ignore_ascii_case("TOOLS.md") {
        return None;
    }
    if !is_plain_text(content) {
        return None;
    }
    let dest = if n.eq_ignore_ascii_case("SOUL.md") {
        "SOUL.md"
    } else if n.eq_ignore_ascii_case("USER.md") {
        "USER.md"
    } else {
        "MEMORY.md"
    };
    Some((dest.into(), clip_import(content.trim(), 8_000)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspace_and_skip_secrets() {
        assert!(is_openclaw_workspace(&["SOUL.md", "foo"]));
        assert!(!is_openclaw_workspace(&["README.md"]));
        assert!(default_openclaw_paths("/home/j")[0].ends_with(".openclaw/workspace"));
        assert!(import_memory_file("SOUL.md", "be useful").is_some());
        assert!(import_memory_file("SOUL.md", "token sk-abcdefghijklmnopqrstuv").is_none());
        assert!(import_memory_file("TOOLS.md", "ok").is_none());
    }
}
