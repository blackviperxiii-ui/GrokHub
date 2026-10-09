// Portions derived from xai-org/grok-build (Apache-2.0, © SpaceXAI), commit 2bdd1d6a; modified.

use std::fs::File;
use std::io::Read;
use std::path::Path;

use crate::episode::clip_bytes;

const TEMPLATE: &str = include_str!("prompt_template.md");
const AGENTS_CAP: usize = 64 * 1024;

pub fn system_prompt(cabin_rules: &str, workspace: &Path) -> String {
    compose(cabin_rules, workspace, grokhub_core::user_home().as_deref())
}

fn compose(cabin_rules: &str, workspace: &Path, home: Option<&Path>) -> String {
    let mut out = template_body();
    let rules = cabin_rules.trim();
    if !rules.is_empty() {
        out.push_str("\n\n");
        out.push_str(rules);
    }
    if let Some(agents) = layered_agents(workspace, home) {
        out.push_str("\n\n<agents>\n");
        out.push_str(&agents);
        out.push_str("\n</agents>");
    }
    let skills = crate::skills::discover(workspace, home, &crate::plugins::skill_dirs(workspace));
    let reminder = crate::skills::reminder(&skills);
    if !reminder.is_empty() {
        out.push_str("\n\n");
        out.push_str(&reminder);
    }
    let extras = crate::plugins::prompt_extras(workspace);
    if !extras.is_empty() {
        out.push_str("\n\n");
        out.push_str(&extras);
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

fn layered_agents(workspace: &Path, home: Option<&Path>) -> Option<String> {
    let mut parts: Vec<String> = Vec::new();
    if let Some(home) = home {
        push_md(&mut parts, &home.join(".grok").join("AGENTS.md"));
        push_md(&mut parts, &home.join(".grok").join("CLAUDE.md"));
        push_md(&mut parts, &home.join(".claude").join("AGENTS.md"));
        push_md(&mut parts, &home.join(".claude").join("CLAUDE.md"));
    }
    for dir in crate::skills::project_chain(workspace) {
        push_md(&mut parts, &dir.join("AGENTS.md"));
        push_md(&mut parts, &dir.join("CLAUDE.md"));
        push_md(&mut parts, &dir.join(".claude").join("CLAUDE.md"));
        push_md(&mut parts, &dir.join(".claude").join("AGENTS.md"));
    }
    if parts.is_empty() {
        return None;
    }
    Some(clip_bytes(&parts.join("\n\n"), AGENTS_CAP).to_string())
}

fn push_md(out: &mut Vec<String>, path: &Path) {
    if !regular_file(path) {
        return;
    }
    let Some(text) = read_capped(path) else {
        return;
    };
    let text = text.trim();
    if text.is_empty() {
        return;
    }
    out.push(format!("# {}\n{text}", path.display()));
}

fn read_capped(path: &Path) -> Option<String> {
    let file = File::open(path).ok()?;
    let mut limited = file.take((AGENTS_CAP as u64).saturating_add(1));
    let mut buf = Vec::new();
    limited.read_to_end(&mut buf).ok()?;
    if buf.len() > AGENTS_CAP {
        let mut cut = AGENTS_CAP;
        while cut > 0 && buf[cut] & 0xC0 == 0x80 {
            cut -= 1;
        }
        buf.truncate(cut);
    }
    Some(String::from_utf8_lossy(&buf).into_owned())
}

fn regular_file(path: &Path) -> bool {
    std::fs::symlink_metadata(path)
        .map(|meta| meta.file_type().is_file())
        .unwrap_or(false)
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

    #[test]
    fn rules_layer_global_then_root_then_cwd_and_cap() {
        let root = std::env::temp_dir().join(format!(
            "gh-rules-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let _ = std::fs::remove_dir_all(&root);
        let home = root.join("home");
        let repo = root.join("repo");
        let cwd = repo.join("nested");
        std::fs::create_dir_all(home.join(".grok")).unwrap();
        std::fs::create_dir_all(repo.join(".git")).unwrap();
        std::fs::create_dir_all(&cwd).unwrap();
        std::fs::write(home.join(".grok").join("AGENTS.md"), "GLOBAL_MARK\n").unwrap();
        std::fs::write(repo.join("AGENTS.md"), "ROOT_MARK\n").unwrap();
        std::fs::write(cwd.join("AGENTS.md"), "CWD_MARK\n").unwrap();
        std::fs::write(cwd.join("CLAUDE.md"), "X".repeat(80_000)).unwrap();
        let layered = layered_agents(&cwd, Some(&home)).unwrap();
        let global_at = layered.find("GLOBAL_MARK").unwrap();
        let root_at = layered.find("ROOT_MARK").unwrap();
        let cwd_at = layered.find("CWD_MARK").unwrap();
        assert!(global_at < root_at && root_at < cwd_at, "{layered}");
        assert!(layered.len() <= AGENTS_CAP, "{}", layered.len());
        let full = compose("Cabin rule line.", &cwd, Some(&home));
        assert!(full.find("Cabin rule line.").unwrap() < full.find("GLOBAL_MARK").unwrap());
        assert!(full.contains("<agents>"));
        let _ = std::fs::remove_dir_all(&root);
    }
}
