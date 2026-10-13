// Portions derived from xai-org/grok-build (Apache-2.0, © SpaceXAI), commit 2bdd1d6a; modified.

use std::collections::HashSet;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Instant, SystemTime};

use crate::episode::clip_bytes;

const TEMPLATE: &str = include_str!("prompt_template.md");
pub(crate) const AGENTS_CAP: usize = 64 * 1024;

/// Per-run tracker for just-in-time nested `AGENTS.md` / `CLAUDE.md` notes.
///
/// Folder rules are appended to tool output only. The system prompt is never
/// mutated mid-run (keeps the prompt-cache prefix stable).
#[derive(Debug, Default)]
pub(crate) struct NestedAgents {
    seen: HashSet<PathBuf>,
    used: usize,
}

impl NestedAgents {
    /// Seed with context files already layered into the system prompt so they
    /// are not repeated on the first tool touch of the workspace root.
    pub(crate) fn new(workspace: &Path) -> Self {
        let mut seen = HashSet::new();
        for dir in crate::skills::project_chain(workspace) {
            for path in context_files_in(&dir) {
                seen.insert(canonical_or_owned(&path));
            }
        }
        Self { seen, used: 0 }
    }

    /// Walk from `touched`'s folder up to `workspace`. Unseen context files
    /// become fenced notes, once each, until [`AGENTS_CAP`] is spent.
    ///
    /// Returns an empty string when `touched` is outside the workspace, the
    /// path is sensitive, nothing new is left, or the budget is spent.
    pub(crate) fn notes_for(&mut self, workspace: &Path, touched: &Path) -> String {
        if self.used >= AGENTS_CAP {
            return String::new();
        }
        let Ok(ws) = workspace.canonicalize() else {
            return String::new();
        };
        let Some(start) = walk_start(&ws, touched) else {
            return String::new();
        };
        let home = grokhub_core::user_home().unwrap_or_default();
        let home_s = home.to_string_lossy();
        let mut notes = Vec::new();
        let mut cur = start;
        loop {
            if !cur.starts_with(&ws) {
                break;
            }
            if sensitive_path(&cur, &home_s) {
                // Still walk up; a parent may be safe. Skip only this folder's files.
            } else {
                for path in context_files_in(&cur) {
                    if sensitive_path(&path, &home_s) {
                        continue;
                    }
                    let key = canonical_or_owned(&path);
                    if !self.seen.insert(key) {
                        continue;
                    }
                    let Some(text) = read_capped(&path) else {
                        continue;
                    };
                    let text = text.trim();
                    if text.is_empty() {
                        continue;
                    }
                    let rel = path
                        .strip_prefix(&ws)
                        .unwrap_or(&path)
                        .display()
                        .to_string()
                        .replace('\\', "/");
                    let header = format!("Folder rules from {rel}:\n");
                    let room = AGENTS_CAP.saturating_sub(self.used);
                    if room <= header.len() + 8 {
                        // 8 = fence wrappers roughly; stop spending.
                        self.used = AGENTS_CAP;
                        break;
                    }
                    let body_room = room.saturating_sub(header.len()).saturating_sub(8);
                    let body = clip_bytes(text, body_room);
                    let note = format!("```\n{header}{body}\n```");
                    self.used = self.used.saturating_add(note.len());
                    notes.push(note);
                    if self.used >= AGENTS_CAP {
                        break;
                    }
                }
            }
            if self.used >= AGENTS_CAP || cur == ws {
                break;
            }
            if !cur.pop() {
                break;
            }
        }
        notes.join("\n")
    }
}

fn canonical_or_owned(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

fn walk_start(workspace: &Path, touched: &Path) -> Option<PathBuf> {
    let start = if touched.is_dir() {
        touched.to_path_buf()
    } else {
        touched
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(workspace)
            .to_path_buf()
    };
    let start = start.canonicalize().unwrap_or(start);
    if !start.starts_with(workspace) {
        return None;
    }
    Some(start)
}

fn sensitive_path(path: &Path, home: &str) -> bool {
    let s = path.to_string_lossy();
    grokhub_core::rewind_sensitive(&s, home)
}

fn context_files_in(dir: &Path) -> Vec<PathBuf> {
    #[cfg(windows)]
    {
        let Ok(rd) = std::fs::read_dir(dir) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for ent in rd.flatten() {
            let name = ent.file_name();
            let Some(name) = name.to_str() else {
                continue;
            };
            if !name.eq_ignore_ascii_case("AGENTS.md") && !name.eq_ignore_ascii_case("CLAUDE.md") {
                continue;
            }
            let path = ent.path();
            if regular_file(&path) {
                out.push(path);
            }
        }
        out.sort();
        out
    }
    #[cfg(not(windows))]
    {
        let mut out = Vec::new();
        for name in ["AGENTS.md", "CLAUDE.md"] {
            let path = dir.join(name);
            if regular_file(&path) {
                out.push(path);
            }
        }
        out
    }
}

/// Which surface is building the system prompt.
///
/// Chat keeps the full skills reminder and plugin extras. Background and
/// Unattended drop those unless the job declares a [`PromptNeed`]. Review is
/// the lean nightly body from [`grokhub_core::review_system_prompt`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptProfile {
    Chat,
    Background,
    Unattended,
    Review,
}

/// Extras a job opts into beyond the profile default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PromptNeed {
    pub skills: bool,
    pub plugins: bool,
}

impl PromptNeed {
    pub const NONE: Self = Self {
        skills: false,
        plugins: false,
    };
    pub const SKILLS: Self = Self {
        skills: true,
        plugins: false,
    };
    pub const FULL: Self = Self {
        skills: true,
        plugins: true,
    };
}

/// Parse `NEED_SKILLS` / `NEED_PLUGINS` tokens from a job brief or task body.
pub fn prompt_need_from_job(job: &str) -> PromptNeed {
    PromptNeed {
        skills: job.contains("NEED_SKILLS"),
        plugins: job.contains("NEED_PLUGINS"),
    }
}

impl PromptProfile {
    fn want_skills(self, need: PromptNeed) -> bool {
        matches!(self, PromptProfile::Chat) || need.skills
    }

    fn want_plugins(self, need: PromptNeed) -> bool {
        matches!(self, PromptProfile::Chat) || need.plugins
    }
}

struct AgentsCache {
    key: Vec<(PathBuf, Option<SystemTime>)>,
    text: Option<String>,
}

static AGENTS_CACHE: Mutex<Option<AgentsCache>> = Mutex::new(None);

/// Legacy entry: the whole `cabin_rules` blob is treated as the stable prefix
/// (no volatile tail). Prefer [`system_prompt_parts`] for cache-friendly order.
pub fn system_prompt(cabin_rules: &str, workspace: &Path) -> String {
    system_prompt_with(
        PromptProfile::Chat,
        cabin_rules,
        "",
        workspace,
        PromptNeed::FULL,
    )
}

/// Cache-friendly chat system prompt: stable rules + disk blocks first, volatile
/// personality / tuned notes / learned brief last.
pub fn system_prompt_parts(stable_rules: &str, volatile_tail: &str, workspace: &Path) -> String {
    system_prompt_with(
        PromptProfile::Chat,
        stable_rules,
        volatile_tail,
        workspace,
        PromptNeed::FULL,
    )
}

/// Build a system prompt for `profile`. Background and Unattended omit the
/// skills reminder and plugin extras unless `need` asks for them. Safety text
/// from the template (and caller-supplied cabin rules) always stays.
pub fn system_prompt_with(
    profile: PromptProfile,
    stable_rules: &str,
    volatile_tail: &str,
    workspace: &Path,
    need: PromptNeed,
) -> String {
    if profile == PromptProfile::Review {
        return grokhub_core::review_system_prompt().to_string();
    }
    compose(
        stable_rules,
        volatile_tail,
        workspace,
        grokhub_core::user_home().as_deref(),
        profile,
        need,
    )
}

fn compose(
    stable_rules: &str,
    volatile_tail: &str,
    workspace: &Path,
    home: Option<&Path>,
    profile: PromptProfile,
    need: PromptNeed,
) -> String {
    let started = Instant::now();
    let mut out = template_body();
    let rules = stable_rules.trim();
    if !rules.is_empty() {
        out.push_str("\n\n");
        out.push_str(rules);
    }
    if let Some(agents) = layered_agents(workspace, home) {
        out.push_str("\n\n<agents>\n");
        out.push_str(&agents);
        out.push_str("\n</agents>");
    }
    if profile.want_skills(need) {
        let skills =
            crate::skills::discover(workspace, home, &crate::plugins::skill_dirs(workspace));
        let reminder = crate::skills::reminder(&skills);
        if !reminder.is_empty() {
            out.push_str("\n\n");
            out.push_str(&reminder);
        }
    }
    if profile.want_plugins(need) {
        let extras = crate::plugins::prompt_extras(workspace);
        if !extras.is_empty() {
            out.push_str("\n\n");
            out.push_str(&extras);
        }
    }
    let tail = volatile_tail.trim();
    if !tail.is_empty() {
        out.push_str("\n\n");
        out.push_str(tail);
    }
    let ms = started.elapsed().as_secs_f64() * 1000.0;
    if cfg!(debug_assertions) {
        // Local compose cost only; provider cache hits are Usage.cached_tokens.
        eprintln!("grokhub prompt_compose_ms={ms:.3} bytes={}", out.len());
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

fn agents_paths(workspace: &Path, home: Option<&Path>) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Some(home) = home {
        paths.push(home.join(".grok").join("AGENTS.md"));
        paths.push(home.join(".grok").join("CLAUDE.md"));
        paths.push(home.join(".claude").join("AGENTS.md"));
        paths.push(home.join(".claude").join("CLAUDE.md"));
    }
    for dir in crate::skills::project_chain(workspace) {
        paths.push(dir.join("AGENTS.md"));
        paths.push(dir.join("CLAUDE.md"));
        paths.push(dir.join(".claude").join("CLAUDE.md"));
        paths.push(dir.join(".claude").join("AGENTS.md"));
    }
    paths
}

fn path_mtime(path: &Path) -> Option<SystemTime> {
    std::fs::symlink_metadata(path)
        .ok()
        .and_then(|m| m.modified().ok())
}

fn agents_cache_key(workspace: &Path, home: Option<&Path>) -> Vec<(PathBuf, Option<SystemTime>)> {
    agents_paths(workspace, home)
        .into_iter()
        .map(|p| {
            let mtime = path_mtime(&p);
            (p, mtime)
        })
        .collect()
}

fn layered_agents(workspace: &Path, home: Option<&Path>) -> Option<String> {
    let key = agents_cache_key(workspace, home);
    if let Ok(guard) = AGENTS_CACHE.lock() {
        if let Some(cache) = guard.as_ref() {
            if cache.key == key {
                return cache.text.clone();
            }
        }
    }
    let text = layered_agents_uncached(workspace, home);
    if let Ok(mut guard) = AGENTS_CACHE.lock() {
        *guard = Some(AgentsCache {
            key,
            text: text.clone(),
        });
    }
    text
}

fn layered_agents_uncached(workspace: &Path, home: Option<&Path>) -> Option<String> {
    let mut parts: Vec<String> = Vec::new();
    for path in agents_paths(workspace, home) {
        push_md(&mut parts, &path);
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

    fn fixture_workspace(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "gh-prompt-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("AGENTS.md"), "Prefer small patches.\n").unwrap();
        // Enough skills to fill most of the ~4 KiB reminder so Background is
        // ≥30% smaller than Chat on this fixture.
        let desc = "d".repeat(160);
        for i in 0..28 {
            let name = format!("bulk-skill-{i:02}");
            let skill_dir = dir.join(".grok/skills").join(&name);
            std::fs::create_dir_all(&skill_dir).unwrap();
            std::fs::write(
                skill_dir.join("SKILL.md"),
                format!("---\nname: {name}\ndescription: {desc}\n---\nBody {i}.\n"),
            )
            .unwrap();
        }
        dir
    }

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
        // Chat is not permanently read-only; Plan/Ask are. Never invent a
        // blanket "read-only phase" that blocks host work when tools are listed.
        assert!(text.contains("Never invent a permanent \"read-only phase\""));
        assert!(text.contains("sealed Installed-app learnings"));
        assert!(!text.contains("You are Grok, a read-only coding assistant"));
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
        let full = compose(
            "Cabin rule line.",
            "",
            &cwd,
            Some(&home),
            PromptProfile::Chat,
            PromptNeed::FULL,
        );
        assert!(full.find("Cabin rule line.").unwrap() < full.find("GLOBAL_MARK").unwrap());
        assert!(full.contains("<agents>"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn compose_stable_prefix_byte_identical_when_only_tail_changes() {
        let dir = std::env::temp_dir().join(format!(
            "gh-prompt-stable-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("AGENTS.md"), "Stable agents line.\n").unwrap();
        let base = compose(
            "STABLE_RULES",
            "",
            &dir,
            None,
            PromptProfile::Chat,
            PromptNeed::FULL,
        );
        let again = compose(
            "STABLE_RULES",
            "",
            &dir,
            None,
            PromptProfile::Chat,
            PromptNeed::FULL,
        );
        assert_eq!(base, again, "same inputs must be byte-identical");
        let with_a = compose(
            "STABLE_RULES",
            "VOLATILE_A",
            &dir,
            None,
            PromptProfile::Chat,
            PromptNeed::FULL,
        );
        let with_b = compose(
            "STABLE_RULES",
            "VOLATILE_B",
            &dir,
            None,
            PromptProfile::Chat,
            PromptNeed::FULL,
        );
        assert!(
            with_a.starts_with(&base),
            "volatile A must only extend the stable prefix"
        );
        assert!(
            with_b.starts_with(&base),
            "volatile B must only extend the stable prefix"
        );
        assert!(with_a.ends_with("VOLATILE_A"));
        assert!(with_b.ends_with("VOLATILE_B"));
        assert_ne!(with_a, with_b);
        assert!(
            base.find("STABLE_RULES").unwrap() < base.find("Stable agents line").unwrap(),
            "safety/static rules stay before agents: {base}"
        );
        assert!(
            with_a.find("<agents>").unwrap() < with_a.find("VOLATILE_A").unwrap(),
            "volatile tail stays after disk blocks: {with_a}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn agents_cache_invalidates_when_agents_md_changes() {
        let dir = std::env::temp_dir().join(format!(
            "gh-agents-cache-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("AGENTS.md");
        std::fs::write(&path, "FIRST\n").unwrap();
        let first = layered_agents(&dir, None).unwrap();
        assert!(first.contains("FIRST"));
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::write(&path, "SECOND\n").unwrap();
        let second = layered_agents(&dir, None).unwrap();
        assert!(second.contains("SECOND"), "{second}");
        assert!(!second.contains("FIRST"), "{second}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn every_profile_keeps_safety_rules() {
        let dir = fixture_workspace("safety");
        let honesty = "Do not invent a source, a count, or a fact.";
        let rules = honesty;
        for profile in [
            PromptProfile::Chat,
            PromptProfile::Background,
            PromptProfile::Unattended,
        ] {
            let text = system_prompt_with(profile, rules, "", &dir, PromptNeed::NONE);
            assert!(
                text.contains("<dangerous_actions>"),
                "{profile:?} must keep template safety: {}",
                text.len()
            );
            assert!(
                text.contains("UNTRUSTED"),
                "{profile:?} must keep fence honesty"
            );
            assert!(
                text.contains(honesty),
                "{profile:?} must keep cabin honesty rules"
            );
        }
        let review = system_prompt_with(PromptProfile::Review, rules, "", &dir, PromptNeed::NONE);
        assert_eq!(review, grokhub_core::review_system_prompt());
        assert!(
            review.contains("Do not dump a chat reply"),
            "Review profile keeps its lean safety contract: {review}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn background_prompt_at_least_30_percent_smaller_than_chat() {
        let dir = fixture_workspace("size");
        let rules = grokhub_core::cabin_rules::CABIN_DESKTOP_RULES;
        let chat = system_prompt_with(PromptProfile::Chat, rules, "", &dir, PromptNeed::FULL);
        let background =
            system_prompt_with(PromptProfile::Background, rules, "", &dir, PromptNeed::NONE);
        assert!(
            chat.contains("<skills>"),
            "fixture must load skills into Chat"
        );
        assert!(
            !background.contains("<skills>"),
            "Background must drop skills by default: {}",
            background.len()
        );
        let chat_bytes = chat.len();
        let bg_bytes = background.len();
        let saved = chat_bytes.saturating_sub(bg_bytes);
        let pct = (saved as f64) * 100.0 / (chat_bytes as f64);
        assert!(
            pct >= 30.0,
            "Background must be ≥30% smaller than Chat: chat={chat_bytes} bg={bg_bytes} saved={pct:.1}%"
        );
        // Rough token estimate for the PR body (chars/4).
        let chat_tok = chat_bytes.div_ceil(4);
        let bg_tok = bg_bytes.div_ceil(4);
        assert!(chat_tok > bg_tok, "chat_tok={chat_tok} bg_tok={bg_tok}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn job_declaring_skills_still_gets_them() {
        let dir = fixture_workspace("need-skills");
        let rules = "Cabin rule line.";
        let plain =
            system_prompt_with(PromptProfile::Background, rules, "", &dir, PromptNeed::NONE);
        assert!(!plain.contains("<skills>"), "{plain}");
        let need = prompt_need_from_job("Run the weekly report.\nNEED_SKILLS\n");
        assert_eq!(need, PromptNeed::SKILLS);
        let with = system_prompt_with(PromptProfile::Background, rules, "", &dir, need);
        assert!(with.contains("<skills>"), "{with}");
        assert!(with.contains("bulk-skill-00"), "{with}");
        assert!(with.contains("<dangerous_actions>"));
        let unattended = system_prompt_with(
            PromptProfile::Unattended,
            rules,
            "",
            &dir,
            prompt_need_from_job("NEED_SKILLS and finish"),
        );
        assert!(unattended.contains("<skills>"), "{unattended}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn prompt_need_from_job_parses_tokens() {
        assert_eq!(prompt_need_from_job("plain task"), PromptNeed::NONE);
        assert_eq!(prompt_need_from_job("NEED_SKILLS"), PromptNeed::SKILLS);
        assert_eq!(
            prompt_need_from_job("NEED_PLUGINS only"),
            PromptNeed {
                skills: false,
                plugins: true
            }
        );
        assert_eq!(
            prompt_need_from_job("NEED_SKILLS\nNEED_PLUGINS"),
            PromptNeed::FULL
        );
    }

    fn nested_scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "gh-nested-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn nested_agents_injects_once_then_skips_repeat() {
        let ws = nested_scratch("once");
        let sub = ws.join("sub");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(sub.join("AGENTS.md"), "use tabs\n").unwrap();
        std::fs::write(sub.join("a.rs"), "fn main() {}\n").unwrap();
        let system_before = system_prompt("Cabin rule.", &ws);
        let mut nested = NestedAgents::new(&ws);
        let first = nested.notes_for(&ws, &sub.join("a.rs"));
        assert!(
            first.contains("Folder rules from sub/AGENTS.md:"),
            "{first}"
        );
        assert!(first.contains("use tabs"), "{first}");
        assert!(first.contains("```"), "{first}");
        let second = nested.notes_for(&ws, &sub.join("a.rs"));
        assert!(second.is_empty(), "second touch must not repeat: {second}");
        let again_sibling = nested.notes_for(&ws, &sub.join("b.rs"));
        assert!(
            again_sibling.is_empty(),
            "same folder must stay spent: {again_sibling}"
        );
        let system_after = system_prompt("Cabin rule.", &ws);
        assert_eq!(
            system_before, system_after,
            "system prompt bytes must stay unchanged across nested injection"
        );
        let _ = std::fs::remove_dir_all(&ws);
    }

    #[test]
    fn nested_agents_ignores_paths_outside_workspace() {
        let ws = nested_scratch("outside");
        let outside = nested_scratch("outside-peer");
        std::fs::create_dir_all(outside.join("sub")).unwrap();
        std::fs::write(outside.join("sub").join("AGENTS.md"), "secret rules\n").unwrap();
        std::fs::write(outside.join("sub").join("a.rs"), "x\n").unwrap();
        let mut nested = NestedAgents::new(&ws);
        let note = nested.notes_for(&ws, &outside.join("sub").join("a.rs"));
        assert!(note.is_empty(), "outside workspace must not inject: {note}");
        let _ = std::fs::remove_dir_all(&ws);
        let _ = std::fs::remove_dir_all(&outside);
    }

    #[cfg(windows)]
    #[test]
    fn nested_agents_matches_context_filenames_case_insensitively_on_windows() {
        let ws = nested_scratch("win-case");
        let sub = ws.join("sub");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(sub.join("agents.md"), "windows case fold\n").unwrap();
        std::fs::write(sub.join("a.rs"), "fn main() {}\n").unwrap();
        let mut nested = NestedAgents::new(&ws);
        let note = nested.notes_for(&ws, &sub.join("a.rs"));
        assert!(
            note.to_ascii_lowercase().contains("folder rules from sub/"),
            "{note}"
        );
        assert!(note.contains("windows case fold"), "{note}");
        let _ = std::fs::remove_dir_all(&ws);
    }
}
