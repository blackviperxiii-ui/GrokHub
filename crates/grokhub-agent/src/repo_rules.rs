//! Discovered repo rules for a workspace: AGENTS.md, CLAUDE.md, `.cursor/rules`,
//! `.github` templates, commit conventions and pre-commit. Helpers get a
//! [`RepoRulesBundle`] with every task (Order 918.95 / 918.5).

use serde::Serialize;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

/// Cap for the concatenated prompt block passed to helpers.
pub const PROMPT_CAP: usize = 48 * 1024;
/// Cap for one file's excerpt in the bundle.
const EXCERPT_CAP: usize = 4 * 1024;
/// Cap for listing rule files (paths only beyond this still count in UI).
const FILES_CAP: usize = 64;

/// Kind of a discovered rule file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RuleKind {
    Agents,
    Claude,
    CursorRules,
    Contributing,
    PrTemplate,
    Codeowners,
    Workflow,
    PreCommit,
    CommitConvention,
    Other,
}

/// One discovered rule file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuleFile {
    /// Path relative to the workspace when possible.
    pub path: String,
    pub kind: RuleKind,
    /// Short excerpt for UI / helpers; never a secret.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub excerpt: Option<String>,
    pub bytes: usize,
}

/// Sync snapshot attached to the bundle (from git status).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct RepoSync {
    pub ahead: u32,
    pub behind: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub upstream: Option<String>,
}

/// Structured rules for a project workspace. Attached to every helper task.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoRulesBundle {
    pub project_id: String,
    pub workspace: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remote_url: Option<String>,
    /// Working / PR-base branch: override, else `beta` when present, else default.
    pub preferred_branch: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_branch: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sync: Option<RepoSync>,
    pub files: Vec<RuleFile>,
    /// Concatenated text safe for the helper system/prompt (capped).
    pub prompt_block: String,
}

/// Inputs the host already knows when building a bundle.
#[derive(Debug, Clone, Default)]
pub struct RepoRulesInput<'a> {
    pub project_id: &'a str,
    pub remote_url: Option<&'a str>,
    pub preferred_branch: Option<&'a str>,
    pub active_branch: Option<&'a str>,
    pub sync: Option<RepoSync>,
    /// Detected default branch (`origin/HEAD`), when known.
    pub default_branch: Option<&'a str>,
    /// True when a local or remote `beta` ref exists.
    pub has_beta: bool,
}

/// Pick the working branch: override → beta (when present) → default → active → `main` last.
pub fn resolve_preferred_branch(input: &RepoRulesInput<'_>) -> String {
    if let Some(b) = input.preferred_branch.map(str::trim).filter(|s| !s.is_empty()) {
        return b.to_string();
    }
    if input.has_beta {
        return "beta".into();
    }
    if let Some(b) = input.default_branch.map(str::trim).filter(|s| !s.is_empty()) {
        return b.to_string();
    }
    if let Some(b) = input.active_branch.map(str::trim).filter(|s| !s.is_empty()) {
        return b.to_string();
    }
    "main".into()
}

/// Discover rule files under `workspace` and build a [`RepoRulesBundle`].
pub fn discover(workspace: &Path, input: RepoRulesInput<'_>) -> RepoRulesBundle {
    let preferred_branch = resolve_preferred_branch(&input);
    let mut files = Vec::new();
    scan_root(workspace, &mut files);
    scan_cursor_rules(workspace, &mut files);
    scan_github(workspace, &mut files);
    scan_commit_tools(workspace, &mut files);
    files.truncate(FILES_CAP);

    let prompt_block = build_prompt_block(workspace, &files, &preferred_branch, &input);

    RepoRulesBundle {
        project_id: input.project_id.to_string(),
        workspace: workspace.display().to_string(),
        remote_url: input.remote_url.map(str::to_string),
        preferred_branch,
        active_branch: input.active_branch.map(str::to_string),
        sync: input.sync,
        files,
        prompt_block,
    }
}

/// Prompt text to prepend on every helper task. Empty when nothing was found.
pub fn task_prefix(bundle: &RepoRulesBundle) -> String {
    let block = bundle.prompt_block.trim();
    if block.is_empty() {
        return String::new();
    }
    format!("<repo_rules>\n{block}\n</repo_rules>\n\n")
}

fn build_prompt_block(
    workspace: &Path,
    files: &[RuleFile],
    preferred_branch: &str,
    input: &RepoRulesInput<'_>,
) -> String {
    let mut parts = Vec::new();
    parts.push(format!("preferred_branch: {preferred_branch}"));
    if let Some(b) = input.active_branch {
        parts.push(format!("active_branch: {b}"));
    }
    if let Some(url) = input.remote_url {
        parts.push(format!("remote: {url}"));
    }
    if !files.is_empty() {
        let names: Vec<&str> = files.iter().map(|f| f.path.as_str()).collect();
        parts.push(format!("rule_files: {}", names.join(", ")));
    }
    let mut used = parts.join("\n").len();
    for f in files {
        if used >= PROMPT_CAP {
            break;
        }
        let Some(excerpt) = f.excerpt.as_deref() else {
            continue;
        };
        // Prefer agents / claude / cursor rules in the prompt body.
        if !matches!(
            f.kind,
            RuleKind::Agents | RuleKind::Claude | RuleKind::CursorRules | RuleKind::Contributing
        ) {
            continue;
        }
        let header = format!("\n\n# {}\n", rel_display(workspace, &f.path));
        let room = PROMPT_CAP.saturating_sub(used).saturating_sub(header.len());
        if room < 32 {
            break;
        }
        let body = clip(excerpt, room);
        used = used.saturating_add(header.len() + body.len());
        parts.push(format!("{header}{body}"));
    }
    clip(&parts.join(""), PROMPT_CAP)
}

fn scan_root(workspace: &Path, out: &mut Vec<RuleFile>) {
    for (name, kind) in [
        ("AGENTS.md", RuleKind::Agents),
        ("CLAUDE.md", RuleKind::Claude),
        (".claude/CLAUDE.md", RuleKind::Claude),
        (".claude/AGENTS.md", RuleKind::Agents),
    ] {
        push_file(workspace, workspace.join(name), kind, out);
    }
}

fn scan_cursor_rules(workspace: &Path, out: &mut Vec<RuleFile>) {
    let dir = workspace.join(".cursor").join("rules");
    let Ok(entries) = fs::read_dir(&dir) else {
        return;
    };
    let mut paths: Vec<PathBuf> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_file())
        .collect();
    paths.sort();
    for path in paths {
        push_file(workspace, path, RuleKind::CursorRules, out);
    }
}

fn scan_github(workspace: &Path, out: &mut Vec<RuleFile>) {
    let gh = workspace.join(".github");
    for name in ["CONTRIBUTING.md", "CONTRIBUTING", "CODEOWNERS"] {
        let kind = if name.starts_with("CODE") {
            RuleKind::Codeowners
        } else {
            RuleKind::Contributing
        };
        push_file(workspace, gh.join(name), kind, out);
    }
    push_file(
        workspace,
        gh.join("PULL_REQUEST_TEMPLATE.md"),
        RuleKind::PrTemplate,
        out,
    );
    let tpl_dir = gh.join("PULL_REQUEST_TEMPLATE");
    if let Ok(entries) = fs::read_dir(&tpl_dir) {
        let mut paths: Vec<PathBuf> = entries
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.is_file())
            .collect();
        paths.sort();
        for path in paths {
            push_file(workspace, path, RuleKind::PrTemplate, out);
        }
    }
    let owners = gh.join("CODEOWNERS");
    push_file(workspace, owners, RuleKind::Codeowners, out);
    let wf = gh.join("workflows");
    if let Ok(entries) = fs::read_dir(&wf) {
        let mut paths: Vec<PathBuf> = entries
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| {
                p.extension()
                    .and_then(|e| e.to_str())
                    .is_some_and(|e| e == "yml" || e == "yaml")
            })
            .collect();
        paths.sort();
        for path in paths.into_iter().take(12) {
            push_file(workspace, path, RuleKind::Workflow, out);
        }
    }
}

fn scan_commit_tools(workspace: &Path, out: &mut Vec<RuleFile>) {
    for (name, kind) in [
        (".pre-commit-config.yaml", RuleKind::PreCommit),
        (".pre-commit-config.yml", RuleKind::PreCommit),
        ("commitlint.config.js", RuleKind::CommitConvention),
        ("commitlint.config.cjs", RuleKind::CommitConvention),
        ("commitlint.config.mjs", RuleKind::CommitConvention),
        (".commitlintrc", RuleKind::CommitConvention),
        (".commitlintrc.json", RuleKind::CommitConvention),
        (".commitlintrc.yml", RuleKind::CommitConvention),
        ("CONVENTIONAL_COMMIT.md", RuleKind::CommitConvention),
    ] {
        push_file(workspace, workspace.join(name), kind, out);
    }
}

fn push_file(workspace: &Path, path: PathBuf, kind: RuleKind, out: &mut Vec<RuleFile>) {
    if out.len() >= FILES_CAP {
        return;
    }
    if !path.is_file() {
        return;
    }
    let Ok(meta) = fs::metadata(&path) else {
        return;
    };
    let bytes = meta.len() as usize;
    let excerpt = read_excerpt(&path);
    let rel = path
        .strip_prefix(workspace)
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|_| path.display().to_string().replace('\\', "/"));
    if out.iter().any(|f| f.path == rel) {
        return;
    }
    out.push(RuleFile {
        path: rel,
        kind,
        excerpt,
        bytes,
    });
}

fn read_excerpt(path: &Path) -> Option<String> {
    let file = fs::File::open(path).ok()?;
    let mut limited = file.take((EXCERPT_CAP as u64).saturating_add(1));
    let mut buf = Vec::new();
    limited.read_to_end(&mut buf).ok()?;
    if buf.len() > EXCERPT_CAP {
        let mut cut = EXCERPT_CAP;
        while cut > 0 && buf[cut] & 0xC0 == 0x80 {
            cut -= 1;
        }
        buf.truncate(cut);
    }
    let text = String::from_utf8_lossy(&buf);
    let trimmed = text.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

fn rel_display(workspace: &Path, rel: &str) -> String {
    workspace.join(rel).display().to_string()
}

fn clip(text: &str, cap: usize) -> String {
    if text.len() <= cap {
        return text.to_string();
    }
    let mut cut = cap;
    while cut > 0 && !text.is_char_boundary(cut) {
        cut -= 1;
    }
    text[..cut].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "gh-repo-rules-{name}-{}-{}",
            std::process::id(),
            grokhub_core::now_ms()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp");
        dir
    }

    #[test]
    fn preferred_branch_prefers_override_then_beta_then_default() {
        let override_wins = RepoRulesInput {
            preferred_branch: Some("staging"),
            has_beta: true,
            default_branch: Some("main"),
            ..Default::default()
        };
        assert_eq!(resolve_preferred_branch(&override_wins), "staging");
        let beta = RepoRulesInput {
            has_beta: true,
            default_branch: Some("main"),
            ..Default::default()
        };
        assert_eq!(resolve_preferred_branch(&beta), "beta");
        let def = RepoRulesInput {
            default_branch: Some("develop"),
            ..Default::default()
        };
        assert_eq!(resolve_preferred_branch(&def), "develop");
    }

    #[test]
    fn discover_finds_agents_cursor_and_github_files() {
        let root = temp("scan");
        fs::write(root.join("AGENTS.md"), "use beta").unwrap();
        fs::create_dir_all(root.join(".cursor/rules")).unwrap();
        fs::write(root.join(".cursor/rules/ship.mdc"), "no main").unwrap();
        fs::create_dir_all(root.join(".github")).unwrap();
        fs::write(root.join(".github/CONTRIBUTING.md"), "PRs to beta").unwrap();
        fs::write(root.join(".pre-commit-config.yaml"), "repos: []").unwrap();

        let bundle = discover(
            &root,
            RepoRulesInput {
                project_id: "p1",
                remote_url: Some("https://github.com/org/app.git"),
                has_beta: true,
                active_branch: Some("feat"),
                ..Default::default()
            },
        );
        assert_eq!(bundle.preferred_branch, "beta");
        assert_eq!(bundle.remote_url.as_deref(), Some("https://github.com/org/app.git"));
        assert!(bundle.files.iter().any(|f| f.path == "AGENTS.md"));
        assert!(bundle
            .files
            .iter()
            .any(|f| f.path == ".cursor/rules/ship.mdc"));
        assert!(bundle
            .files
            .iter()
            .any(|f| f.path == ".github/CONTRIBUTING.md"));
        assert!(bundle
            .files
            .iter()
            .any(|f| f.path == ".pre-commit-config.yaml"));
        assert!(bundle.prompt_block.contains("preferred_branch: beta"));
        assert!(bundle.prompt_block.contains("use beta"));
        let prefix = task_prefix(&bundle);
        assert!(prefix.starts_with("<repo_rules>"));
        assert!(prefix.contains("</repo_rules>"));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn empty_workspace_yields_empty_prefix() {
        let root = temp("empty");
        let bundle = discover(&root, RepoRulesInput::default());
        assert!(bundle.files.is_empty());
        assert!(task_prefix(&bundle).is_empty() || !task_prefix(&bundle).contains("# "));
        // Still names a preferred branch for helpers.
        assert_eq!(bundle.preferred_branch, "main");
        let _ = fs::remove_dir_all(&root);
    }
}
