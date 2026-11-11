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
    let chars = s.chars().count();
    if chars <= max {
        return s.to_string();
    }
    let clipped: String = s.chars().take(max).collect();
    format!("{}… [truncated {} chars]", clipped, chars - max)
}

/// Whether `name` is one of the workspace's own files, in any letter case.
pub fn is_core(name: &str) -> bool {
    OPENCLAW_CORE.iter().any(|c| c.eq_ignore_ascii_case(name))
}

/// The memory file a workspace file goes to, from its name alone, so a
/// caller can decide before it reads the file. `None`: it is not imported.
pub fn import_dest(name: &str) -> Option<&'static str> {
    let n = name.rsplit('/').next().unwrap_or(name);
    if !is_core(n) && !n.ends_with(".md") {
        return None;
    }
    if n.eq_ignore_ascii_case("TOOLS.md") {
        return None;
    }
    Some(if n.eq_ignore_ascii_case("SOUL.md") {
        "SOUL.md"
    } else if n.eq_ignore_ascii_case("USER.md") {
        "USER.md"
    } else {
        "MEMORY.md"
    })
}

pub fn import_memory_file(name: &str, content: &str) -> Option<(String, String)> {
    let dest = import_dest(name)?;
    if !is_plain_text(content) {
        return None;
    }
    Some((dest.into(), clip_import(content.trim(), 8_000)))
}

/// Extra OpenClaw markdown must append under a heading, not replace MEMORY.md.
pub fn merge_imported_memory(existing: &str, incoming: &str, heading: &str) -> String {
    let incoming = incoming.trim();
    if incoming.is_empty() {
        return existing.to_string();
    }
    let block = if heading.eq_ignore_ascii_case("MEMORY.md") {
        incoming.to_string()
    } else {
        format!("## {heading}\n\n{incoming}")
    };
    let existing = existing.trim();
    if existing.is_empty() {
        block
    } else {
        format!("{existing}\n\n{block}")
    }
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
        let long = "é".repeat(9000);
        let clipped = clip_import(&long, 8000);
        assert!(clipped.contains("truncated"));
        assert!(!clipped.contains('\u{FFFD}'));
        let merged = merge_imported_memory("keep me", "be useful", "AGENTS.md");
        assert!(
            merged.contains("keep me")
                && merged.contains("## AGENTS.md")
                && merged.contains("be useful"),
            "import must not clobber MEMORY.md with the last readdir file"
        );
        assert_eq!(merge_imported_memory("", "notes", "MEMORY.md"), "notes");
    }

    #[test]
    fn the_name_alone_says_where_a_file_goes() {
        assert_eq!(import_dest("SOUL.md"), Some("SOUL.md"));
        assert_eq!(import_dest("soul.md"), Some("SOUL.md"));
        assert_eq!(import_dest("USER.md"), Some("USER.md"));
        assert_eq!(import_dest("AGENTS.md"), Some("MEMORY.md"));
        assert_eq!(import_dest("notes/habits.md"), Some("MEMORY.md"));
        assert_eq!(import_dest("TOOLS.md"), None);
        assert_eq!(import_dest("export.log"), None);
        assert_eq!(import_dest("state.sqlite"), None);
        assert!(is_core("Soul.md"));
        assert!(!is_core("habits.md"));
        // The content check is unchanged: a name that passes still needs plain text.
        assert_eq!(
            import_memory_file("habits.md", "Reads before bed.\n"),
            Some(("MEMORY.md".to_string(), "Reads before bed.".to_string()))
        );
        assert_eq!(import_memory_file("export.log", "plain"), None);
    }
}
