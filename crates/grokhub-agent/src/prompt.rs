// Portions derived from xai-org/grok-build (Apache-2.0, © SpaceXAI), commit 2bdd1d6a; modified.

use std::path::Path;

const TEMPLATE: &str = include_str!("prompt_template.md");
const AGENTS_CAP: usize = 64 * 1024;

pub fn system_prompt(cabin_rules: &str, workspace: &Path) -> String {
    let mut out = template_body();
    let rules = cabin_rules.trim();
    if !rules.is_empty() {
        out.push_str("\n\n");
        out.push_str(rules);
    }
    if let Some(agents) = read_agents(workspace) {
        out.push_str("\n\n<agents>\n");
        out.push_str(&agents);
        out.push_str("\n</agents>");
    }
    out
}

fn template_body() -> String {
    let text = TEMPLATE.trim_start();
    let text = text
        .strip_prefix("<!--")
        .and_then(|rest| rest.split_once("-->"))
        .map(|(_, body)| body.trim_start())
        .unwrap_or(text);
    text.to_string()
}

fn read_agents(workspace: &Path) -> Option<String> {
    let path = crate::tools::confine(workspace, "AGENTS.md").ok()?;
    let bytes = std::fs::read(&path).ok()?;
    if bytes.len() > AGENTS_CAP {
        let mut cut = AGENTS_CAP;
        while cut > 0 && bytes[cut] & 0xC0 == 0x80 {
            cut -= 1;
        }
        return Some(String::from_utf8_lossy(&bytes[..cut]).into_owned());
    }
    String::from_utf8(bytes).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_strips_notice_and_appends_rules_and_agents() {
        let dir = std::env::temp_dir().join(format!("gh-prompt-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("AGENTS.md"), "Prefer small patches.\n").unwrap();
        let text = system_prompt("Cabin rule line.", &dir);
        assert!(!text.contains("Portions derived"));
        assert!(text.contains("read_file"));
        assert!(text.contains("Cabin rule line."));
        assert!(text.contains("Prefer small patches."));
        assert!(text.contains("read-only"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
