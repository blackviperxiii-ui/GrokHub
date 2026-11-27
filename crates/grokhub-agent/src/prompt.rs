// Portions derived from xai-org/grok-build (Apache-2.0, © SpaceXAI), commit 2bdd1d6a; modified.

use std::collections::HashSet;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Instant, SystemTime};

use crate::client::{ContentPart, InputItem};
use crate::episode::clip_bytes;

const TEMPLATE: &str = include_str!("prompt_template.md");

/// The prompt's `<dangerous_actions>` block, word for word, tags included.
/// Compaction pins it after the summary so the safety rules stay fresh.
pub(crate) fn safety_rules() -> &'static str {
    const OPEN: &str = "<dangerous_actions>";
    const CLOSE: &str = "</dangerous_actions>";
    let start = TEMPLATE.find(OPEN).unwrap_or(0);
    let end = TEMPLATE[start..]
        .find(CLOSE)
        .map_or(start, |at| start + at + CLOSE.len());
    &TEMPLATE[start..end]
}
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
/// the lean nightly body from [`grokhub_core::review_system_prompt`]. Peer is
/// an agent answering a paired computer: no AGENTS.md or CLAUDE.md layer, no
/// environment facts, skills or plugin extras, since those are the person's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptProfile {
    Chat,
    Background,
    Unattended,
    Review,
    Peer,
}

/// Extras a job opts into beyond the profile default.
///
/// `tools` is the tool/no-tool prompt split: when false, the fixed template
/// drops tool-calling instructions (Hangout chat-only / plain chat). Schemas
/// are omitted separately by the loop when it sends an empty tool list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PromptNeed {
    pub skills: bool,
    pub plugins: bool,
    pub tools: bool,
}

impl Default for PromptNeed {
    fn default() -> Self {
        Self::NONE
    }
}

impl PromptNeed {
    pub const NONE: Self = Self {
        skills: false,
        plugins: false,
        tools: true,
    };
    /// Plain chat: no skills reminder, no plugin extras, no tool instructions.
    pub const NO_TOOLS: Self = Self {
        skills: false,
        plugins: false,
        tools: false,
    };
    pub const SKILLS: Self = Self {
        skills: true,
        plugins: false,
        tools: true,
    };
    pub const FULL: Self = Self {
        skills: true,
        plugins: true,
        tools: true,
    };
}

/// Parse `NEED_SKILLS` / `NEED_PLUGINS` tokens from a job brief or task body.
pub fn prompt_need_from_job(job: &str) -> PromptNeed {
    PromptNeed {
        skills: job.contains("NEED_SKILLS"),
        plugins: job.contains("NEED_PLUGINS"),
        tools: true,
    }
}

/// Parse a per-job [`crate::tools::ToolSet`] from a brief or task body.
///
/// - No tool tokens → [`crate::tools::ToolSet::Full`] (same as chat).
/// - `NEED_SHELL` / `TOOLS:shell` adds the shell family to a lean background set.
/// - `NEED_MCP` / `TOOLS:mcp` keeps MCP discovery tools.
/// - `TOOLS:a,b,c` starts from the background set and adds those names.
///
/// SP2 wires callers; SP1 only defines the mapping next to [`prompt_need_from_job`].
pub fn tool_set_from_job(job: &str) -> crate::tools::ToolSet {
    use crate::tools::ToolSet;

    let has_tools_line = job.lines().any(|line| {
        let t = line.trim();
        t.starts_with("TOOLS:") || t.starts_with("TOOLS=")
    });
    let need_shell = job.contains("NEED_SHELL");
    let need_mcp = job.contains("NEED_MCP");
    if !has_tools_line && !need_shell && !need_mcp {
        return ToolSet::FULL;
    }

    let mut set = ToolSet::background();
    if need_shell {
        set = set.with_tool("shell");
    }
    if need_mcp {
        set = set.with_mcp();
    }
    for line in job.lines() {
        let t = line.trim();
        let rest = t
            .strip_prefix("TOOLS:")
            .or_else(|| t.strip_prefix("TOOLS="));
        let Some(rest) = rest else {
            continue;
        };
        for part in rest.split(|c: char| c == ',' || c.is_whitespace()) {
            let part = part.trim();
            if part.is_empty() {
                continue;
            }
            if part.eq_ignore_ascii_case("mcp") {
                set = set.with_mcp();
            } else {
                set = set.with_tool(part);
            }
        }
    }
    set
}

impl PromptProfile {
    fn want_skills(self, need: PromptNeed) -> bool {
        match self {
            PromptProfile::Chat => true,
            PromptProfile::Peer => false,
            _ => need.skills,
        }
    }

    fn want_plugins(self, need: PromptNeed) -> bool {
        match self {
            PromptProfile::Chat => true,
            PromptProfile::Peer => false,
            _ => need.plugins,
        }
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

/// The pieces of a chat's prompt layers, already read by the caller.
/// Personality ([`ChatTail::voice`]) is the cached static prefix. Notes,
/// learned brief, identity and intentions are the per-turn dynamic suffix.
/// The app and the live eval both build from this, so the eval runs the
/// prompt the app sends.
#[derive(Debug, Clone, Default)]
pub struct ChatTail {
    /// The personality's prompt block. Cached in the static prefix.
    pub voice: String,
    /// Tuned notes.
    pub notes: String,
    /// The learned brief for chat, before it is wrapped.
    pub learned: String,
    /// SOUL.md and USER.md blocks.
    pub identity: String,
    /// This session's intentions block.
    pub intentions: String,
    /// The personality allows explicit talk (picks the safety closer).
    pub explicit: bool,
}

/// Join a chat's volatile suffix in the app's order. Personality stays out
/// (it is the static prefix). When notes, identity or intentions are present,
/// a short closer comes last so the cabin safety rules still outrank them.
pub fn chat_volatile_tail(tail: &ChatTail) -> String {
    let mut parts: Vec<String> = Vec::new();
    let mut closer = false;
    for (block, needs_closer) in [
        (tail.notes.clone(), true),
        (
            grokhub_core::cabin_rules::cabin_learned_tail(&tail.learned),
            false,
        ),
        (tail.identity.clone(), true),
        (tail.intentions.clone(), true),
    ] {
        if !block.is_empty() {
            parts.push(block);
            closer |= needs_closer;
        }
    }
    if closer {
        parts.push(grokhub_core::talk::safety_closer(tail.explicit).to_string());
    }
    parts.join("\n\n")
}

/// Cache-friendly split of one system prompt: a byte-stable prefix, then a
/// per-turn suffix (timestamp, volatile state, active files, reminders).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DynamicTail {
    /// Current time for this turn, already formatted. Empty skips the line.
    pub timestamp: String,
    /// Tuned notes, identity, intentions, and other volatile state.
    pub volatile: String,
    /// Open / recently touched files for this turn.
    pub active_files: String,
    /// Short-lived reminders that must not sit in the cached prefix.
    pub reminders: String,
}

impl DynamicTail {
    /// Join the per-turn pieces in a fixed order, skipping empties.
    pub fn render(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        let ts = self.timestamp.trim();
        if !ts.is_empty() {
            parts.push(format!("Current time: {ts}"));
        }
        let files = self.active_files.trim();
        if !files.is_empty() {
            parts.push(files.to_string());
        }
        let reminders = self.reminders.trim();
        if !reminders.is_empty() {
            parts.push(reminders.to_string());
        }
        let volatile = self.volatile.trim();
        if !volatile.is_empty() {
            parts.push(volatile.to_string());
        }
        parts.join("\n\n")
    }
}

/// Marker that starts the per-turn suffix. Prefix bytes stop immediately
/// before this tag so a new timestamp cannot break the xAI prompt cache.
pub const DYNAMIC_BOUNDARY: &str = "<turn-context>";

/// One assembled system prompt, split at [`DYNAMIC_BOUNDARY`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssembledPrompt {
    /// Core instructions, personality, project `AGENTS.md` / `CLAUDE.md`,
    /// facts, skills and plugin extras. Byte-identical across turns when
    /// only the suffix changes.
    pub prefix: String,
    /// Timestamp, volatile state, active files, ephemeral reminders.
    pub suffix: String,
}

impl AssembledPrompt {
    /// Prefix, then the dynamic suffix wrapped in [`DYNAMIC_BOUNDARY`].
    pub fn render(&self) -> String {
        if self.suffix.is_empty() {
            return self.prefix.clone();
        }
        format!(
            "{}\n\n{DYNAMIC_BOUNDARY}\n{}\n</turn-context>",
            self.prefix, self.suffix
        )
    }
}

/// Cache-friendly chat system prompt: stable rules + personality + disk
/// blocks first, volatile notes last. Put the personality in `stable_rules`
/// (or pass it through [`assemble`]) so it stays in the cached prefix.
pub fn system_prompt_parts(stable_rules: &str, volatile_tail: &str, workspace: &Path) -> String {
    system_prompt_with(
        PromptProfile::Chat,
        stable_rules,
        volatile_tail,
        workspace,
        PromptNeed::FULL,
    )
}

/// Chat prompt from a [`ChatTail`]: personality in the static prefix, the
/// rest of the tail after [`DYNAMIC_BOUNDARY`].
pub fn system_prompt_from_chat(stable_rules: &str, tail: &ChatTail, workspace: &Path) -> String {
    assemble(
        stable_rules,
        &tail.voice,
        &DynamicTail {
            volatile: chat_volatile_tail(tail),
            ..DynamicTail::default()
        },
        workspace,
        grokhub_core::user_home().as_deref(),
        PromptProfile::Chat,
        PromptNeed::FULL,
    )
    .render()
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

/// Build the cache-friendly prefix and per-turn suffix as separate strings.
/// Two calls that differ only in [`DynamicTail`] keep an identical `prefix`.
pub fn assemble(
    stable_rules: &str,
    personality: &str,
    dynamic: &DynamicTail,
    workspace: &Path,
    home: Option<&Path>,
    profile: PromptProfile,
    need: PromptNeed,
) -> AssembledPrompt {
    if profile == PromptProfile::Review {
        return AssembledPrompt {
            prefix: grokhub_core::review_system_prompt().to_string(),
            suffix: String::new(),
        };
    }
    let started = Instant::now();
    let mut prefix = fixed_prompt(need.tools);
    let rules = stable_rules.trim();
    if !rules.is_empty() {
        prefix.push_str("\n\n");
        prefix.push_str(rules);
    }
    let voice = personality.trim();
    if !voice.is_empty() {
        prefix.push_str("\n\n");
        prefix.push_str(voice);
    }
    if profile == PromptProfile::Chat {
        if need.tools {
            prefix.push_str("\n\n");
            prefix.push_str(grokhub_core::cabin_rules::LOOKUP_RULES);
        }
        // How every voice talks: cached, under the personality.
        prefix.push_str("\n\n");
        prefix.push_str(grokhub_core::conversation::CORE);
    }
    let peer = profile == PromptProfile::Peer;
    if let Some(agents) = (!peer).then(|| layered_agents(workspace, home)).flatten() {
        prefix.push_str("\n\n<agents>\n");
        prefix.push_str(&agents);
        prefix.push_str("\n</agents>");
    }
    let facts = if peer {
        String::new()
    } else {
        facts_block(workspace)
    };
    if !facts.is_empty() {
        prefix.push_str("\n\n");
        prefix.push_str(&facts);
    }
    if profile.want_skills(need) {
        let skills =
            crate::skills::discover(workspace, home, &crate::plugins::skill_dirs(workspace));
        let reminder = crate::skills::reminder(&skills);
        if !reminder.is_empty() {
            prefix.push_str("\n\n");
            prefix.push_str(&reminder);
        }
    }
    if profile.want_plugins(need) {
        let extras = crate::plugins::prompt_extras(workspace);
        if !extras.is_empty() {
            prefix.push_str("\n\n");
            prefix.push_str(&extras);
        }
    }
    let suffix = dynamic.render();
    let prefix = prefix.trim_end().to_string();
    let assembled = AssembledPrompt { prefix, suffix };
    let out_len = assembled.render().len();
    let ms = started.elapsed().as_secs_f64() * 1000.0;
    if cfg!(debug_assertions) {
        // Local compose cost only; provider cache hits are Usage.cached_tokens.
        eprintln!("grokhub prompt_compose_ms={ms:.3} bytes={out_len}");
    }
    assembled
}

/// Checked environment facts for `workspace` (round 3 item 39). Tests only
/// see them under a pinned config, so a test never writes the real one.
fn facts_block(workspace: &Path) -> String {
    #[cfg(test)]
    let Some(config) = crate::perm::pinned_config() else {
        return String::new();
    };
    #[cfg(not(test))]
    let config = crate::perm::config_dir();
    let facts =
        grokhub_core::env_facts::observe_workspace(&config, workspace, grokhub_core::now_ms());
    grokhub_core::env_facts::prompt_block(&facts)
}

/// Real OS home and workspace paths so the model never guesses a username or
/// user folder path. Stripped of Windows verbatim `\\?\` prefixes.
pub fn paths_block(workspace: &Path, home: Option<&Path>) -> String {
    let format_path = |p: &Path| {
        let raw = p.to_string_lossy();
        let windows = cfg!(windows)
            || raw.starts_with(r"\\?\")
            || (raw.len() >= 2 && raw.as_bytes()[1] == b':');
        grokhub_core::canonical_session_cwd(&raw, windows)
    };
    let mut lines = Vec::new();
    if let Some(h) = home.filter(|p| !p.as_os_str().is_empty()) {
        let cleaned = format_path(h);
        if !cleaned.is_empty() {
            lines.push(format!("- Home: `{cleaned}`"));
        }
    }
    if workspace.as_os_str().is_empty() {
        lines.push(
            "- Workspace: (none selected — ask the user to pick a folder before running file or shell tools)".to_string(),
        );
    } else {
        let cleaned = format_path(workspace);
        if !cleaned.is_empty() {
            lines.push(format!("- Workspace: `{cleaned}`"));
        }
    }
    if lines.is_empty() {
        return String::new();
    }
    format!(
        "<paths>\nReal OS paths on this computer (use these exact paths; never guess a username or home directory):\n{}\n</paths>",
        lines.join("\n")
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
    assemble(
        stable_rules,
        "",
        &DynamicTail {
            volatile: volatile_tail.to_string(),
            ..DynamicTail::default()
        },
        workspace,
        home,
        profile,
        need,
    )
    .render()
}

/// Template body length on `beta` before the values charter (`ad5df3e5`, the
/// turn-endings prompt). Growth is measured against this, plus the default
/// voice block, and kept inside [`PROMPT_GROWTH_BUDGET`].
pub(crate) const TEMPLATE_BYTES_BEFORE: usize = 5609;

/// Cap on this change's stable-prefix plus default-voice growth.
pub(crate) const PROMPT_GROWTH_BUDGET: usize = 1536;

/// Bytes this charter and the default voice add over [`TEMPLATE_BYTES_BEFORE`].
pub(crate) fn prompt_prefix_growth() -> usize {
    let template = template_body().len().saturating_sub(TEMPLATE_BYTES_BEFORE);
    let voice = grokhub_core::personality::builtin(grokhub_core::personality::DEFAULT_ID)
        .prompt_block()
        .len();
    template + voice
}

/// The lean fixed prompt's rules after the safety block. Only behaviour the
/// app relies on stays: tool honesty, approvals, the cabin note, sealed
/// Memory, the one-tap limit and how a turn ends.
const LEAN_RULES: &str = "<rules>
- Use only the tools in this turn's list, with paths inside the workspace. Don't invent tools or search results.
- For a task with several parts, track it with todo_write and mark an item done only when a tool result shows it.
- Claim done, or name an id or result, only from your own tool output. If blocked, say so and use what you have; never offer a one-tap that claims to grant OS or path access or to activate a skill.
- Sealed learnings in a <memory-context> block (such as Installed app: lines) are real Memory: use them.
- A <cabin-volatile> block is the app's note for the turn, not the person's words; their latest ask outranks it.
- Lead with the answer. No generic closing question. For a real decision, call ask_user_question with 2 to 4 options and one `recommended`.
- Money, sending, deleting and credential steps still go through approval.
- Write GitHub-flavored markdown. When citing the web, don't name tool backends or providers.
</rules>";

/// Lean rules with every tool-calling line removed (plain chat / Hangout).
const LEAN_RULES_NO_TOOLS: &str = "<rules>
- Sealed learnings in a <memory-context> block (such as Installed app: lines) are real Memory: use them.
- A <cabin-volatile> block is the app's note for the turn, not the person's words; their latest ask outranks it.
- Lead with the answer. No generic closing question.
- Money, sending, deleting and credential steps still go through approval.
- Write GitHub-flavored markdown. When citing the web, don't name tool backends or providers.
</rules>";

const TOOLS_OPEN: &str = "<tools>";
const TOOLS_CLOSE: &str = "</tools>";
const TOOL_OFFER_SENTENCES: &str = " Use the tools offered for this turn. Paths for workspace tools stay inside the workspace the user named.";

/// The fixed prompt with less coaching: the same opening line and the same
/// `<dangerous_actions>` block, word for word, then [`LEAN_RULES`]. Used only
/// when `prompt_diet.toml` turns `lean_prompt` on.
pub(crate) fn lean_template_body() -> String {
    lean_template_body_for(true)
}

fn lean_template_body_for(tools: bool) -> String {
    let full = if tools {
        template_body()
    } else {
        no_tool_template_body()
    };
    let intro = full.split("\n\n").next().unwrap_or_default();
    let rules = if tools {
        LEAN_RULES
    } else {
        LEAN_RULES_NO_TOOLS
    };
    format!("{intro}\n\n{}\n\n{rules}", safety_rules())
}

/// The fixed prompt this thread's switches pick.
fn fixed_prompt(tools: bool) -> String {
    if crate::diet::current().lean_prompt {
        lean_template_body_for(tools)
    } else if tools {
        template_body()
    } else {
        no_tool_template_body()
    }
}

/// Fixed prompt with tool-calling instructions and the `<tools>` block removed.
/// Derived by subtraction from [`template_body`]; no new coaching text.
pub(crate) fn no_tool_template_body() -> String {
    strip_tool_instructions(&template_body())
}

fn strip_tool_instructions(full: &str) -> String {
    let mut t = full.replace(TOOL_OFFER_SENTENCES, "");
    if let Some(start) = t.find(TOOLS_OPEN) {
        if let Some(rel) = t[start..].find(TOOLS_CLOSE) {
            let mut end = start + rel + TOOLS_CLOSE.len();
            while end < t.len() && t.as_bytes()[end] == b'\n' {
                end += 1;
            }
            let mut from = start;
            while from > 0 && t.as_bytes()[from - 1] == b'\n' {
                from -= 1;
            }
            t.replace_range(from..end, "\n");
        }
    }
    while t.contains("\n\n\n") {
        t = t.replace("\n\n\n", "\n\n");
    }
    t
}

/// Rewrite the first system message for a no-tool turn: drop the `<tools>`
/// block, the tool-offer sentences, and chat lookup rules. Cabin rules and
/// `AGENTS.md` stay. Idempotent, same idea as the lean coding swap.
pub fn apply_no_tool_prompt(input: &mut [InputItem]) {
    let full = template_body();
    let none = no_tool_template_body();
    let full_trim = full.trim();
    let none_trim = none.trim();
    let lookup = grokhub_core::cabin_rules::LOOKUP_RULES;
    for item in input {
        let InputItem::Message { role, content } = item else {
            continue;
        };
        if role != "system" {
            continue;
        }
        for part in content.iter_mut() {
            let ContentPart::InputText(text) = part else {
                continue;
            };
            if text.starts_with(none_trim) && !text.contains(TOOLS_OPEN) {
                return;
            }
            if let Some(rest) = text.strip_prefix(full_trim) {
                let rest = rest.replacen(&format!("\n\n{lookup}"), "", 1);
                *text = format!("{none_trim}{rest}");
                return;
            }
        }
        return;
    }
}

pub(crate) fn template_body() -> String {
    let text = TEMPLATE.trim_start();
    let text = text
        .strip_prefix("<!--")
        .and_then(|rest| rest.split_once("-->"))
        .map(|(_, body)| body.trim_start())
        .unwrap_or(text);
    text.replace("{product}", grokhub_core::brand::product_name())
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

    #[test]
    fn chat_tail_keeps_the_apps_order_and_closes_with_safety() {
        assert_eq!(chat_volatile_tail(&ChatTail::default()), "");
        // A learned brief alone is the user's own facts: no closer.
        let learned = chat_volatile_tail(&ChatTail {
            learned: "likes FPS games".into(),
            ..Default::default()
        });
        assert!(learned.contains("likes FPS games"));
        assert!(!learned.contains(grokhub_core::talk::safety_closer(false)));
        let full = chat_volatile_tail(&ChatTail {
            voice: "VOICE".into(),
            notes: "NOTES".into(),
            learned: "LEARNED".into(),
            identity: "IDENTITY".into(),
            intentions: "INTENTIONS".into(),
            explicit: false,
        });
        let at = |needle: &str| full.find(needle).unwrap();
        assert!(
            !full.contains("VOICE"),
            "personality stays in the static prefix, not the suffix"
        );
        assert!(at("NOTES") < at("LEARNED"));
        assert!(at("LEARNED") < at("IDENTITY"));
        assert!(at("IDENTITY") < at("INTENTIONS"));
        assert!(full.ends_with(grokhub_core::talk::safety_closer(false)));
    }

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
    fn checked_facts_follow_agents_and_stay_out_of_unpinned_tests() {
        let dir = fixture_workspace("facts");
        std::fs::write(dir.join("Cargo.toml"), "[package]\n").unwrap();
        // No pinned config: a test never reads or writes the real one.
        assert!(!system_prompt("", &dir).contains("<facts>"));
        let cfg = dir.join("cfg");
        let _pin = crate::perm::ConfigGuard::set(&cfg);
        let text = system_prompt("", &dir);
        let at = |needle: &str| text.find(needle).unwrap_or_else(|| panic!("{needle}"));
        assert!(at("</agents>") < at("<facts>"));
        assert!(text.contains("- Tests: `cargo test` (from Cargo.toml)"));
        // Same facts, same bytes: the cached prefix holds.
        assert_eq!(text, system_prompt("", &dir));
        assert!(cfg.join(grokhub_core::env_facts::FILE).is_file());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn prompt_says_how_a_turn_ends() {
        let text = system_prompt("Cabin rule line.", &std::env::temp_dir());
        let start = text.find("<turn_endings>").expect("turn endings section");
        let end = text.find("</turn_endings>").expect("closed");
        let rules = &text[start..end];
        assert!(
            rules.contains("- Lead with the answer or the result."),
            "{rules}"
        );
        assert!(rules
            .contains("no generic closing question such as \"What's next?\", \"Anything else?\""));
        assert!(rules.contains("When the next step is obvious, reversible and inside what the user asked, do it in this turn"));
        assert!(rules.contains("call ask_user_question with 2 to 4 concrete options and mark exactly one `recommended`"));
        assert!(rules.contains("A suggestion or a chosen option never replaces it."));
        // The chat paints this block as the recap card, so the heading must match.
        assert!(
            rules.contains(&format!(
                "End multi-step work with {}: Done:, Partly done: or Not done: per task",
                grokhub_core::recap::RECAP_HEAD
            )),
            "{rules}"
        );
        assert!(
            text.contains("Don't be sycophantic"),
            "the charter keeps the honesty line"
        );
        assert!(text.contains("intelligent adult"));
        assert!(text.contains("real goal"));
        assert!(
            text.starts_with("You are Grok inside Loadout. Help with the user's real goal."),
            "the prompt names the brand, not a placeholder"
        );
        assert!(
            template_body().len() >= TEMPLATE_BYTES_BEFORE,
            "template shrank below the charter baseline"
        );
        let growth = prompt_prefix_growth();
        assert!(
            growth <= PROMPT_GROWTH_BUDGET,
            "prompt grew by {growth} bytes (template plus default voice)"
        );
        // No line anywhere asks the model to close on an offer or a question.
        for line in text.lines() {
            let low = line.to_ascii_lowercase();
            assert!(
                !low.contains("end with a question") && !low.contains("offer the next step"),
                "{line}"
            );
        }
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
        // An agent answering a paired computer gets none of these layers.
        let peer = compose(
            "Cabin rule line.",
            "",
            &cwd,
            Some(&home),
            PromptProfile::Peer,
            PromptNeed::FULL,
        );
        assert!(peer.contains("Cabin rule line."), "{peer}");
        for mark in ["GLOBAL_MARK", "ROOT_MARK", "CWD_MARK", "<agents>", ".grok"] {
            assert!(!peer.contains(mark), "{mark}: {peer}");
        }
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
        assert!(with_a.contains("VOLATILE_A"));
        assert!(with_b.contains("VOLATILE_B"));
        assert!(with_a.ends_with("</turn-context>"));
        assert!(with_b.ends_with("</turn-context>"));
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
    fn the_lean_prompt_keeps_the_safety_block_word_for_word_and_is_much_smaller() {
        let dir = fixture_workspace("lean");
        let cfg = dir.join("cfg");
        std::fs::create_dir_all(&cfg).expect("cfg");
        let _pin = crate::perm::ConfigGuard::set(&cfg);
        let full = system_prompt_with(PromptProfile::Chat, "", "", &dir, PromptNeed::NONE);
        std::fs::write(cfg.join(crate::diet::CONFIG_NAME), "lean_prompt = true\n").expect("on");
        let lean = system_prompt_with(PromptProfile::Chat, "", "", &dir, PromptNeed::NONE);
        assert!(
            lean.contains(safety_rules()),
            "safety block must stay verbatim"
        );
        assert!(lean.starts_with(template_body().split("\n\n").next().unwrap()));
        for kept in [
            "<cabin-volatile>",
            "go through approval",
            "Don't invent tools or search results",
            "ask_user_question",
            "<memory-context>",
            "grant OS or path access",
        ] {
            assert!(lean.contains(kept), "lean prompt lost: {kept}");
        }
        assert!(!lean.contains("{product}"));
        assert!(
            lean.len() + 2048 <= full.len(),
            "lean {} vs full {}",
            lean.len(),
            full.len()
        );
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
        let rules = *grokhub_core::cabin_rules::CABIN_DESKTOP_RULES;
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
        assert!(chat.contains(grokhub_core::cabin_rules::LOOKUP_RULES));
        assert!(!background.contains(grokhub_core::cabin_rules::LOOKUP_RULES));
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
                plugins: true,
                tools: true
            }
        );
        assert_eq!(
            prompt_need_from_job("NEED_SKILLS\nNEED_PLUGINS"),
            PromptNeed::FULL
        );
    }

    #[test]
    fn tool_set_from_job_parses_shell_and_tools_line() {
        use crate::tools::ToolSet;

        assert!(tool_set_from_job("plain task").is_full());
        let shell = tool_set_from_job("NEED_SHELL\nfinish the report");
        assert!(!shell.is_full());
        assert!(shell.allows("run_terminal_command"));
        assert!(shell.allows("read_file"));
        let explicit = tool_set_from_job("TOOLS:read_file,write\nNEED_MCP");
        assert!(explicit.allows("read_file"));
        assert!(explicit.allows("write"));
        assert!(explicit.allows("search_tool"));
        assert!(explicit.allows("use_tool"));
        // SP2 keeps shell on the background base; change only with an explicit allowlist decision.
        assert!(
            ToolSet::background().allows("run_terminal_command"),
            "background ToolSet must keep run_terminal_command unless an allowlist PR drops it"
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

    #[test]
    fn system_prompt_carries_real_os_home_and_workspace_paths() {
        let win_home = Path::new(r"C:\Users\alice");
        let win_ws = Path::new(r"\\?\C:\Users\alice\GrokHub-Work");
        let block = paths_block(win_ws, Some(win_home));
        assert!(block.contains("<paths>"));
        assert!(block.contains(r"- Home: `C:\Users\alice`"), "{block}");
        assert!(
            block.contains(r"- Workspace: `C:\Users\alice\GrokHub-Work`"),
            "{block}"
        );
        assert!(!block.contains(r"\\?\"), "{block}");

        // Empty workspace explicitly tells the model to ask the user to pick a folder.
        let empty_ws = paths_block(Path::new(""), Some(win_home));
        assert!(empty_ws.contains("pick a folder"), "{empty_ws}");

        // Peer and offline eval compose stay free of local OS paths unless injected.
        let peer = compose(
            "",
            "",
            win_ws,
            Some(win_home),
            PromptProfile::Peer,
            PromptNeed::NONE,
        );
        assert!(!peer.contains("<paths>"));
    }

    #[test]
    fn personality_sits_in_the_static_prefix_before_the_dynamic_suffix() {
        let dir = fixture_workspace("voice-prefix");
        let tail = ChatTail {
            voice: "PERSONALITY_MARK".into(),
            notes: "NOTES_MARK".into(),
            ..Default::default()
        };
        let assembled = assemble(
            "Cabin rule.",
            &tail.voice,
            &DynamicTail {
                timestamp: "2026-01-02T03:04:05Z".into(),
                volatile: chat_volatile_tail(&tail),
                ..DynamicTail::default()
            },
            &dir,
            None,
            PromptProfile::Chat,
            PromptNeed::FULL,
        );
        assert!(
            assembled.prefix.contains("PERSONALITY_MARK"),
            "{}",
            assembled.prefix
        );
        assert!(
            !assembled.prefix.contains("NOTES_MARK"),
            "{}",
            assembled.prefix
        );
        assert!(
            !assembled.prefix.contains("2026-01-02T03:04:05Z"),
            "{}",
            assembled.prefix
        );
        assert!(assembled.suffix.contains("NOTES_MARK"));
        assert!(assembled
            .suffix
            .contains("Current time: 2026-01-02T03:04:05Z"));
        let at = |n: &str| assembled.render().find(n).unwrap();
        assert!(at("PERSONALITY_MARK") < at(DYNAMIC_BOUNDARY));
        assert!(at(DYNAMIC_BOUNDARY) < at("NOTES_MARK"));
        assert!(
            assembled.prefix.find("Cabin rule.").unwrap()
                < assembled.prefix.find("PERSONALITY_MARK").unwrap()
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn no_tool_prompt_omits_tool_instructions_and_saves_tokens() {
        let dir = fixture_workspace("no-tools");
        let with = system_prompt_with(PromptProfile::Chat, "Cabin.", "", &dir, PromptNeed::FULL);
        let without = system_prompt_with(
            PromptProfile::Chat,
            "Cabin.",
            "",
            &dir,
            PromptNeed::NO_TOOLS,
        );
        assert!(with.contains("<tools>"), "{with}");
        assert!(with.contains("read_file"), "{with}");
        assert!(with.contains(grokhub_core::cabin_rules::LOOKUP_RULES));
        assert!(with.contains("Use the tools offered for this turn"));
        assert!(!without.contains("<tools>"), "{without}");
        assert!(!without.contains("read_file"), "{without}");
        assert!(!without.contains(grokhub_core::cabin_rules::LOOKUP_RULES));
        assert!(!without.contains("Use the tools offered for this turn"));
        assert!(without.contains("<dangerous_actions>"));
        assert!(without.contains("Cabin."));
        assert!(without.contains("Prefer small patches."));
        let saved = with.len().saturating_sub(without.len());
        assert!(
            saved >= 400,
            "no-tool prompt must drop tool text: with={} without={} saved={saved}",
            with.len(),
            without.len()
        );
        let with_tok = with.len().div_ceil(4);
        let without_tok = without.len().div_ceil(4);
        assert!(
            without_tok < with_tok,
            "with_tok={with_tok} without_tok={without_tok}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn apply_no_tool_prompt_strips_the_template_and_lookup_rules() {
        let full = template_body();
        let lookup = grokhub_core::cabin_rules::LOOKUP_RULES;
        let mut input = vec![InputItem::Message {
            role: "system".into(),
            content: vec![ContentPart::InputText(format!(
                "{}\n\n{lookup}\n\nCabin rule.\n\n<agents>\nrules\n</agents>",
                full.trim()
            ))],
        }];
        apply_no_tool_prompt(&mut input);
        let text = match &input[0] {
            InputItem::Message { content, .. } => match &content[0] {
                ContentPart::InputText(t) => t.clone(),
                _ => panic!("text"),
            },
            _ => panic!("system"),
        };
        assert!(text.starts_with(no_tool_template_body().trim()), "{text}");
        assert!(!text.contains("<tools>"), "{text}");
        assert!(!text.contains(lookup), "{text}");
        assert!(text.contains("Cabin rule."));
        assert!(text.contains("<agents>\nrules\n</agents>"));
        apply_no_tool_prompt(&mut input);
        let again = match &input[0] {
            InputItem::Message { content, .. } => match &content[0] {
                ContentPart::InputText(t) => t.clone(),
                _ => panic!("text"),
            },
            _ => panic!("system"),
        };
        assert_eq!(again, text);
    }
}
