//! Nightly learned suggestions — a built-in organ, not a user automation.
//!
//! Once a local night (21:00) or on catch-up after a missed night, a quiet
//! Balanced pass reads a compact digest and writes `SUGGEST_*` tiles. The
//! user still has to click Add. Static catalog seeds stay as fallback.

use crate::automation::parse_nl_automation;
use crate::chat_view::is_workload_user;
use crate::organs::LocalClock;
use crate::redact::{is_plain_text, redact_secrets};
use serde::{Deserialize, Serialize};

/// Local hour when the first-run review becomes due (21:00).
pub const REVIEW_NIGHT_HOUR: u32 = 21;

/// Hard cap per suggestion kind so the Suggested grids stay short.
pub const SUGGEST_CAP: usize = 6;

/// Digest character budget (threads + memory + receipts).
pub const DIGEST_CHAR_CAP: usize = 6_000;

/// Per chat line so the UI thread never clones an 8MB HOST_RESULT into the digest.
pub const DIGEST_LINE_CAP: usize = 280;

/// GitHub tools we already ship. A suggestion is a tile that runs one of these
/// or sends the user to Settings for a PAT — not a fake app.
pub const CABIN_GITHUB_TOOLS: &[&str] = &[
    "user",
    "list_repos",
    "list_issues",
    "search_code",
    "search_issues",
];

/// Suggestion kind written by the nightly review.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SuggestionKind {
    Auto,
    Skill,
    Connector,
}

/// One learned tile. Fields map onto the existing Suggested cards.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LearnedSuggestion {
    pub kind: SuggestionKind,
    pub title: String,
    pub body: String,
    /// Automations: natural-language schedule (`every day at 21, …`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seed: Option<String>,
    /// Skills: kebab-case folder name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Skills: when this skill should fire.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trigger: Option<String>,
    /// Skills: `SKILL.md` body.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
    /// Connectors: `github`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    /// Connectors: allowlisted GitHub tool.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
}

/// Persisted review state (`~/.config/GrokHub/suggestions.json`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SuggestionStore {
    /// Local calendar day of the last finished review (`YYYY-MM-DD`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_review_day: Option<String>,
    #[serde(default)]
    pub last_review_ms: u64,
    #[serde(default)]
    pub autos: Vec<LearnedSuggestion>,
    #[serde(default)]
    pub skills: Vec<LearnedSuggestion>,
    #[serde(default)]
    pub connectors: Vec<LearnedSuggestion>,
}

/// Whether the built-in nightly review should fire.
///
/// - Already finished today → not due.
/// - Last review was a previous day → due any hour (catch-up after downtime).
/// - Never ran → due only at `night_hour` or later (avoid first-boot API spam).
pub fn review_due(last_day: Option<&str>, today: &str, clock: &LocalClock, night_hour: u32) -> bool {
    if last_day == Some(today) {
        return false;
    }
    if last_day.is_some() {
        return true;
    }
    clock.hour >= night_hour
}

/// Muted Suggested-header copy. No chat dump.
pub fn review_status_line(last_day: Option<&str>, today: &str) -> &'static str {
    if last_day == Some(today) {
        "Reviewed today"
    } else {
        "Review due tonight"
    }
}

/// One thread line for the digest (user or assistant only).
#[derive(Debug, Clone)]
pub struct DigestLine {
    pub role: String,
    pub text: String,
}

/// Borrowed scan: skip host dumps and cap the line so an 8MB complete stays off the digest.
pub fn digest_line_from(role: &str, content: &str) -> Option<DigestLine> {
    if role != "user" && role != "assistant" {
        return None;
    }
    if role == "user" && is_workload_user(content) {
        return None;
    }
    Some(DigestLine {
        role: role.to_string(),
        text: content.chars().take(DIGEST_LINE_CAP).collect(),
    })
}

/// Inputs for the compact nightly digest. All fields are already local.
#[derive(Debug, Clone, Default)]
pub struct ReviewDigest {
    pub insight_pin: String,
    pub user_md: String,
    pub memory_md: String,
    pub skill_names: Vec<String>,
    pub automation_names: Vec<String>,
    pub github_pat: bool,
    pub host_receipts: Vec<String>,
    pub chip_habits: Vec<String>,
    pub thread_lines: Vec<DigestLine>,
    pub trajectory: String,
}

/// Compact, redacted digest for the Balanced review. Caps length.
pub fn build_review_digest(input: &ReviewDigest) -> String {
    let mut out = String::new();
    push_capped(&mut out, "Insight pin", &input.insight_pin);
    push_capped(&mut out, "USER.md", &input.user_md);
    push_capped(&mut out, "MEMORY.md", &input.memory_md);
    if !input.skill_names.is_empty() {
        out.push_str("Existing skills: ");
        out.push_str(&input.skill_names.join(", "));
        out.push('\n');
    }
    if !input.automation_names.is_empty() {
        out.push_str("Existing automations: ");
        out.push_str(&input.automation_names.join(", "));
        out.push('\n');
    }
    out.push_str(if input.github_pat {
        "GitHub PAT: present\n"
    } else {
        "GitHub PAT: missing\n"
    });
    if !input.chip_habits.is_empty() {
        out.push_str("Chip habits: ");
        out.push_str(&input.chip_habits.join(", "));
        out.push('\n');
    }
    if !input.host_receipts.is_empty() {
        out.push_str("Recent host receipts:\n");
        for line in &input.host_receipts {
            let clean = redact_secrets(line);
            if is_plain_text(&clean) && !clean.trim().is_empty() {
                out.push_str("- ");
                out.push_str(clean.trim());
                out.push('\n');
            }
        }
    }
    if !input.trajectory.trim().is_empty() {
        out.push_str("Yesterday's host/hands:\n");
        for line in input.trajectory.lines() {
            let clean = redact_secrets(line);
            if !is_plain_text(&clean) || clean.trim().is_empty() || !cabin_real_text(&clean) {
                continue;
            }
            out.push_str("- ");
            out.push_str(clean.trim());
            out.push('\n');
        }
    }
    if !input.thread_lines.is_empty() {
        out.push_str("Recent chat:\n");
        for line in &input.thread_lines {
            let role = line.role.trim();
            if role != "user" && role != "assistant" {
                continue;
            }
            let clean = redact_secrets(&line.text);
            if !is_plain_text(&clean) || clean.trim().is_empty() {
                continue;
            }
            out.push_str(role);
            out.push_str(": ");
            out.push_str(clean.trim());
            out.push('\n');
        }
    }
    if out.len() > DIGEST_CHAR_CAP {
        out.truncate(DIGEST_CHAR_CAP);
        out.push_str("\n…\n");
    }
    out
}

fn push_capped(out: &mut String, label: &str, raw: &str) {
    let clean = redact_secrets(raw);
    let trimmed = clean.trim();
    if trimmed.is_empty() || !is_plain_text(trimmed) {
        return;
    }
    out.push_str(label);
    out.push_str(":\n");
    out.push_str(trimmed);
    out.push('\n');
}

/// System prompt for the quiet Balanced review. Cabin-real verbs only.
pub fn review_system_prompt() -> &'static str {
    "You are GrokHub's nightly cabin review. Read the digest and propose only \
     things this Linux cabin can actually do. Allowed verbs: Grok Build tools \
     (bash, files, grep, web), workboard cards, Imagine stills (no video), \
     computer-use (Grok looks at and drives the desktop), GitHub MCP if configured. \
     Do not propose Outlook, Gmail, \
     Drive, Slack, or video. Do not dump a chat reply. Output only suggestion \
     lines, one per line, using exactly these forms:\n\
     SUGGEST_AUTO: title | body | every day at 21, <what to do>\n\
     SUGGEST_SKILL: name | title | body | trigger | instructions\n\
     SUGGEST_SKILL_PATCH: name | trigger | improved instructions\n\
     SUGGEST_CONNECTOR: title | body | github <tool>\n\
     Keep titles short. Propose at most 6 of each kind. Skip anything already \
     listed as existing. SUGGEST_SKILL_PATCH only for an existing skill name. \
     If nothing useful, output nothing."
}

/// Patch an existing `SKILL.md` from the nightly review. Not a Suggested tile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillPatch {
    pub name: String,
    pub trigger: String,
    pub instructions: String,
}

/// Parse `SUGGEST_SKILL_PATCH` lines. Name must be kebab-case; body is cabin-real.
pub fn parse_suggest_skill_patches(raw: &str) -> Vec<SkillPatch> {
    let mut out = Vec::new();
    for line in raw.lines() {
        let line = line.trim();
        let Some(rest) = strip_prefix_ci(line, "SUGGEST_SKILL_PATCH:") else {
            continue;
        };
        if let Some(item) = parse_suggest_skill_patch(rest) {
            if out.len() < SUGGEST_CAP {
                out.push(item);
            }
        }
    }
    out
}

fn parse_suggest_skill_patch(rest: &str) -> Option<SkillPatch> {
    let parts: Vec<&str> = rest.splitn(3, '|').collect();
    if parts.len() < 2 {
        return None;
    }
    let name = sanitize_skill_name(parts[0]);
    let (trigger, instructions) = if parts.len() == 2 {
        (String::new(), redact_secrets(parts[1].trim()))
    } else {
        (
            redact_secrets(parts[1].trim()),
            redact_secrets(parts[2].trim()),
        )
    };
    if name.is_empty() || instructions.is_empty() {
        return None;
    }
    if !cabin_real_text(&trigger) || !cabin_real_text(&instructions) {
        return None;
    }
    Some(SkillPatch {
        name,
        trigger,
        instructions,
    })
}

/// Parse `SUGGEST_*` lines. Ignore prose, reject non-cabin verbs, cap per kind.
pub fn parse_suggest_lines(raw: &str) -> Vec<LearnedSuggestion> {
    let mut autos = Vec::new();
    let mut skills = Vec::new();
    let mut connectors = Vec::new();
    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(rest) = strip_prefix_ci(line, "SUGGEST_AUTO:") {
            if let Some(item) = parse_suggest_auto(rest) {
                if autos.len() < SUGGEST_CAP {
                    autos.push(item);
                }
            }
            continue;
        }
        if let Some(rest) = strip_prefix_ci(line, "SUGGEST_SKILL:") {
            if let Some(item) = parse_suggest_skill(rest) {
                if skills.len() < SUGGEST_CAP {
                    skills.push(item);
                }
            }
            continue;
        }
        if let Some(rest) = strip_prefix_ci(line, "SUGGEST_CONNECTOR:") {
            if let Some(item) = parse_suggest_connector(rest) {
                if connectors.len() < SUGGEST_CAP {
                    connectors.push(item);
                }
            }
        }
    }
    let mut out = autos;
    out.extend(skills);
    out.extend(connectors);
    out
}

fn strip_prefix_ci<'a>(line: &'a str, prefix: &str) -> Option<&'a str> {
    if line.len() < prefix.len() {
        return None;
    }
    if line.get(..prefix.len())?.eq_ignore_ascii_case(prefix) {
        return Some(line[prefix.len()..].trim());
    }
    None
}

fn parse_suggest_auto(rest: &str) -> Option<LearnedSuggestion> {
    let parts = split_pipes(rest, 3)?;
    let title = redact_secrets(parts[0].trim());
    let body = redact_secrets(parts[1].trim());
    let seed = redact_secrets(parts[2].trim());
    if title.is_empty() || body.is_empty() || seed.is_empty() {
        return None;
    }
    if !cabin_real_text(&title) || !cabin_real_text(&body) || !cabin_real_text(&seed) {
        return None;
    }
    parse_nl_automation(&seed)?;
    Some(LearnedSuggestion {
        kind: SuggestionKind::Auto,
        title,
        body,
        seed: Some(seed),
        name: None,
        trigger: None,
        instructions: None,
        provider: None,
        tool: None,
    })
}

fn parse_suggest_skill(rest: &str) -> Option<LearnedSuggestion> {
    let parts = split_pipes(rest, 5)?;
    let name = sanitize_skill_name(parts[0]);
    let title = redact_secrets(parts[1].trim());
    let body = redact_secrets(parts[2].trim());
    let trigger = redact_secrets(parts[3].trim());
    let instructions = redact_secrets(parts[4].trim());
    if name.is_empty() || title.is_empty() || body.is_empty() {
        return None;
    }
    if !cabin_real_text(&title)
        || !cabin_real_text(&body)
        || !cabin_real_text(&trigger)
        || !cabin_real_text(&instructions)
    {
        return None;
    }
    Some(LearnedSuggestion {
        kind: SuggestionKind::Skill,
        title,
        body,
        seed: None,
        name: Some(name),
        trigger: Some(trigger),
        instructions: Some(instructions),
        provider: None,
        tool: None,
    })
}

fn parse_suggest_connector(rest: &str) -> Option<LearnedSuggestion> {
    let parts = split_pipes(rest, 3)?;
    let title = redact_secrets(parts[0].trim());
    let body = redact_secrets(parts[1].trim());
    let spec = parts[2].trim();
    let mut words = spec.split_whitespace();
    let provider = words.next()?.to_ascii_lowercase();
    let tool = words.next()?.to_ascii_lowercase();
    if words.next().is_some() {
        return None;
    }
    if provider != "github" || !CABIN_GITHUB_TOOLS.contains(&tool.as_str()) {
        return None;
    }
    if title.is_empty() || body.is_empty() {
        return None;
    }
    if !cabin_real_text(&title) || !cabin_real_text(&body) {
        return None;
    }
    Some(LearnedSuggestion {
        kind: SuggestionKind::Connector,
        title,
        body,
        seed: None,
        name: None,
        trigger: None,
        instructions: None,
        provider: Some(provider),
        tool: Some(tool),
    })
}

fn split_pipes(rest: &str, n: usize) -> Option<Vec<&str>> {
    let parts: Vec<&str> = rest.splitn(n, '|').collect();
    if parts.len() != n {
        return None;
    }
    Some(parts)
}

fn sanitize_skill_name(raw: &str) -> String {
    let cleaned: String = raw
        .trim()
        .to_ascii_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let collapsed = cleaned
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    collapsed.chars().take(48).collect()
}

/// Reject Outlook / Gmail / Drive / video and other off-cabin verbs.
pub fn cabin_real_text(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    const BANNED: &[&str] = &[
        "outlook",
        "gmail",
        "google drive",
        "gdrive",
        "onedrive",
        "slack",
        "teams meeting",
        "zoom",
        "video call",
        "generate a video",
        "make a video",
        "render a video",
        "youtube upload",
    ];
    !BANNED.iter().any(|w| lower.contains(w))
}

/// Drop suggestions that collide with saved skills, active autos, or live connectors.
pub fn dedupe_suggestions(
    items: Vec<LearnedSuggestion>,
    skill_names: &[String],
    automation_names: &[String],
    live_tools: &[String],
) -> Vec<LearnedSuggestion> {
    let skills_l: Vec<String> = skill_names.iter().map(|s| s.to_ascii_lowercase()).collect();
    let autos_l: Vec<String> = automation_names
        .iter()
        .map(|s| s.to_ascii_lowercase())
        .collect();
    let tools_l: Vec<String> = live_tools.iter().map(|s| s.to_ascii_lowercase()).collect();
    let mut seen_auto = Vec::new();
    let mut seen_skill = Vec::new();
    let mut seen_conn = Vec::new();
    let mut out = Vec::new();
    for item in items {
        match item.kind {
            SuggestionKind::Auto => {
                let key = item.title.to_ascii_lowercase();
                if autos_l.iter().any(|n| n == &key) || seen_auto.contains(&key) {
                    continue;
                }
                seen_auto.push(key);
                out.push(item);
            }
            SuggestionKind::Skill => {
                let key = item
                    .name
                    .as_deref()
                    .unwrap_or(&item.title)
                    .to_ascii_lowercase();
                if skills_l.iter().any(|n| n == &key) || seen_skill.contains(&key) {
                    continue;
                }
                seen_skill.push(key);
                out.push(item);
            }
            SuggestionKind::Connector => {
                let tool = item.tool.as_deref().unwrap_or("").to_ascii_lowercase();
                let key = format!("github:{tool}");
                if tools_l.iter().any(|t| t == &tool || t == &key) || seen_conn.contains(&key) {
                    continue;
                }
                seen_conn.push(key);
                out.push(item);
            }
        }
    }
    out
}

fn suggestion_key(item: &LearnedSuggestion) -> String {
    match item.kind {
        SuggestionKind::Auto => item.title.to_ascii_lowercase(),
        SuggestionKind::Skill => item
            .name
            .as_deref()
            .unwrap_or(&item.title)
            .to_ascii_lowercase(),
        SuggestionKind::Connector => {
            format!(
                "github:{}",
                item.tool.as_deref().unwrap_or("").to_ascii_lowercase()
            )
        }
    }
}

fn merge_suggest_bucket(
    existing: &[LearnedSuggestion],
    incoming: Vec<LearnedSuggestion>,
) -> Vec<LearnedSuggestion> {
    let mut out = Vec::new();
    let mut seen = Vec::new();
    for item in incoming.into_iter().chain(existing.iter().cloned()) {
        let key = suggestion_key(&item);
        if seen.contains(&key) {
            continue;
        }
        seen.push(key);
        out.push(item);
        if out.len() >= SUGGEST_CAP {
            break;
        }
    }
    out
}

/// Drop Suggested tiles that collide with already-wired connectors.
pub fn prune_live_suggestions(store: &mut SuggestionStore, live_tools: &[String]) {
    if live_tools.is_empty() {
        return;
    }
    let tools_l: Vec<String> = live_tools.iter().map(|t| t.to_ascii_lowercase()).collect();
    store.connectors.retain(|item| {
        let tool = item.tool.as_deref().unwrap_or("").to_ascii_lowercase();
        let key = format!("github:{tool}");
        !tools_l.iter().any(|t| t == &tool || t == &key)
    });
}

/// Keep prior tiles when tonight's review only names some kinds.
pub fn merge_suggestion_store(
    existing: &SuggestionStore,
    incoming: SuggestionStore,
) -> SuggestionStore {
    SuggestionStore {
        last_review_day: incoming
            .last_review_day
            .or_else(|| existing.last_review_day.clone()),
        last_review_ms: incoming.last_review_ms.max(existing.last_review_ms),
        autos: merge_suggest_bucket(&existing.autos, incoming.autos),
        skills: merge_suggest_bucket(&existing.skills, incoming.skills),
        connectors: merge_suggest_bucket(&existing.connectors, incoming.connectors),
    }
}

/// Split a mixed list into the three store buckets, capped.
pub fn partition_suggestions(items: Vec<LearnedSuggestion>) -> SuggestionStore {
    let mut store = SuggestionStore::default();
    for item in items {
        match item.kind {
            SuggestionKind::Auto if store.autos.len() < SUGGEST_CAP => store.autos.push(item),
            SuggestionKind::Skill if store.skills.len() < SUGGEST_CAP => store.skills.push(item),
            SuggestionKind::Connector if store.connectors.len() < SUGGEST_CAP => {
                store.connectors.push(item)
            }
            SuggestionKind::Auto | SuggestionKind::Skill | SuggestionKind::Connector => {}
        }
    }
    store
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::organs::LocalClock;

    fn clock(hour: u32) -> LocalClock {
        LocalClock {
            weekday: 0,
            hour,
            minute: 0,
            now_ms: 1,
        }
    }

    #[test]
    fn review_due_before_21_never_ran() {
        assert!(!review_due(None, "2026-08-16", &clock(10), REVIEW_NIGHT_HOUR));
    }

    #[test]
    fn review_due_after_21_no_review() {
        assert!(review_due(None, "2026-08-16", &clock(21), REVIEW_NIGHT_HOUR));
        assert!(review_due(None, "2026-08-16", &clock(22), REVIEW_NIGHT_HOUR));
    }

    #[test]
    fn review_due_already_today() {
        assert!(!review_due(
            Some("2026-08-16"),
            "2026-08-16",
            &clock(22),
            REVIEW_NIGHT_HOUR
        ));
        assert!(!review_due(
            Some("2026-08-16"),
            "2026-08-16",
            &clock(10),
            REVIEW_NIGHT_HOUR
        ));
    }

    #[test]
    fn review_status_line_today_or_due() {
        assert_eq!(
            review_status_line(Some("2026-08-16"), "2026-08-16"),
            "Reviewed today"
        );
        assert_eq!(review_status_line(None, "2026-08-16"), "Review due tonight");
        assert_eq!(
            review_status_line(Some("2026-08-15"), "2026-08-16"),
            "Review due tonight"
        );
    }

    #[test]
    fn review_due_catch_up_yesterday() {
        assert!(review_due(
            Some("2026-08-15"),
            "2026-08-16",
            &clock(10),
            REVIEW_NIGHT_HOUR
        ));
    }

    #[test]
    fn parse_suggest_auto_skill_connector() {
        let raw = "\
here is a thought
SUGGEST_AUTO: Night wrap | Close the day | every day at 21, say good night
SUGGEST_SKILL: desk-tidy | Desk tidy | Straighten windows | when the desk is messy | stack the windows
SUGGEST_CONNECTOR: My GitHub | Who am I on GitHub | github user
noise
";
        let items = parse_suggest_lines(raw);
        assert_eq!(items.len(), 3);
        assert_eq!(items[0].kind, SuggestionKind::Auto);
        assert_eq!(items[0].title, "Night wrap");
        assert_eq!(items[1].kind, SuggestionKind::Skill);
        assert_eq!(items[1].name.as_deref(), Some("desk-tidy"));
        assert_eq!(items[2].kind, SuggestionKind::Connector);
        assert_eq!(items[2].tool.as_deref(), Some("user"));
    }

    #[test]
    fn parse_rejects_non_cabin_verbs() {
        let raw = "\
SUGGEST_AUTO: Mail sweep | Clear Outlook | every day at 21, open Outlook
SUGGEST_SKILL: gmail-triage | Gmail | Inbox zero on Gmail | morning | archive Gmail
SUGGEST_CONNECTOR: Drive dump | Pull Google Drive | github user
SUGGEST_AUTO: Night wrap | Close the day | every day at 21, say good night
";
        let items = parse_suggest_lines(raw);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].title, "Night wrap");
    }

    #[test]
    fn parse_rejects_unknown_github_tool() {
        let raw = "SUGGEST_CONNECTOR: Fake | Not a real tool | github delete_repo\n";
        assert!(parse_suggest_lines(raw).is_empty());
    }

    #[test]
    fn parse_redacts_secrets_in_suggest_lines() {
        let raw =
            "SUGGEST_AUTO: Night | token ghp_abcdefghijklmnopqrstuvwx | every day at 21, say hi\n";
        let items = parse_suggest_lines(raw);
        assert_eq!(items.len(), 1);
        assert!(!items[0].body.contains("ghp_abcdefghijklmnopqrstuvwx"));
        assert!(items[0].body.contains("[redacted]"));
    }

    #[test]
    fn digest_redacts_and_caps() {
        let input = ReviewDigest {
            insight_pin: "pin ghp_abcdefghijklmnopqrstuvwx".into(),
            thread_lines: vec![DigestLine {
                role: "user".into(),
                text: "please run HOST_CMD".into(),
            }],
            github_pat: true,
            ..ReviewDigest::default()
        };
        let digest = build_review_digest(&input);
        assert!(!digest.contains("ghp_abcdefghijklmnopqrstuvwx"));
        assert!(digest.contains("[redacted]"));
        assert!(digest.contains("user: please run HOST_CMD"));
        assert!(digest.contains("GitHub PAT: present"));
    }

    #[test]
    fn digest_line_from_skips_host_dumps_and_caps() {
        assert!(digest_line_from("user", "HOST_RESULT (facts only):\nhuge").is_none());
        assert!(digest_line_from("user", "HOST_DIFF:\n- a").is_none());
        assert!(digest_line_from("tool", "ok").is_none());
        let line = digest_line_from("user", &"please run HOST_CMD ".repeat(80)).unwrap();
        assert_eq!(line.role, "user");
        assert!(line.text.chars().count() <= DIGEST_LINE_CAP);
        assert!(digest_line_from("assistant", "ok").is_some());
    }

    #[test]
    fn merge_keeps_prior_tiles_when_tonight_is_partial() {
        let prior = partition_suggestions(parse_suggest_lines(
            "SUGGEST_AUTO: Night wrap | Close | every day at 21, say hi\n\
             SUGGEST_CONNECTOR: Who | me | github user\n",
        ));
        let incoming = partition_suggestions(parse_suggest_lines(
            "SUGGEST_SKILL: desk-tidy | Desk | Body | trig | do it\n",
        ));
        let merged = merge_suggestion_store(&prior, incoming);
        assert_eq!(merged.autos.len(), 1, "prior autos must survive a skill-only night");
        assert_eq!(merged.autos[0].title, "Night wrap");
        assert_eq!(merged.skills.len(), 1);
        assert_eq!(merged.skills[0].name.as_deref(), Some("desk-tidy"));
        assert_eq!(merged.connectors.len(), 1);
        assert_eq!(merged.connectors[0].tool.as_deref(), Some("user"));
    }

    #[test]
    fn prune_drops_already_wired_github_tiles() {
        let mut store = partition_suggestions(parse_suggest_lines(
            "SUGGEST_CONNECTOR: Who | me | github user\n\
             SUGGEST_AUTO: Night wrap | Close | every day at 21, say hi\n",
        ));
        prune_live_suggestions(&mut store, &["user".into()]);
        assert!(
            store.connectors.is_empty(),
            "wired GitHub tools must leave the Suggested grid"
        );
        assert_eq!(store.autos.len(), 1, "other kinds stay");
    }

    #[test]
    fn merge_prefers_tonight_when_titles_collide() {
        let prior = partition_suggestions(parse_suggest_lines(
            "SUGGEST_AUTO: Night wrap | Old body | every day at 21, say hi\n",
        ));
        let incoming = partition_suggestions(parse_suggest_lines(
            "SUGGEST_AUTO: Night wrap | New body | every day at 21, say bye\n",
        ));
        let merged = merge_suggestion_store(&prior, incoming);
        assert_eq!(merged.autos.len(), 1);
        assert_eq!(merged.autos[0].body, "New body");
    }

    #[test]
    fn dedupe_drops_existing_names() {
        let items = parse_suggest_lines(
            "SUGGEST_AUTO: Night wrap | Close | every day at 21, say hi\n\
             SUGGEST_SKILL: desk-tidy | Desk | Body | trig | do it\n\
             SUGGEST_CONNECTOR: Who | me | github user\n",
        );
        let kept = dedupe_suggestions(
            items,
            &["desk-tidy".into()],
            &["Night wrap".into()],
            &["user".into()],
        );
        assert!(kept.is_empty());
    }

    #[test]
    fn parse_skill_patch_and_digest_trajectory() {
        let raw = "\
SUGGEST_SKILL_PATCH: desk-tidy | when messy | stack the windows then act Save
SUGGEST_SKILL_PATCH: gmail-triage | inbox | archive Gmail
SUGGEST_SKILL: desk-tidy | Desk | Body | trig | do it
";
        let patches = parse_suggest_skill_patches(raw);
        assert_eq!(patches.len(), 1);
        assert_eq!(patches[0].name, "desk-tidy");
        assert!(patches[0].instructions.contains("stack the windows"));
        let tiles = parse_suggest_lines(raw);
        assert_eq!(tiles.len(), 1);
        let digest = build_review_digest(&ReviewDigest {
            trajectory: "ok: COMPUTER_CMD: tab close github — closed GitHub\nfail: HOST_CMD: open Outlook — mail".into(),
            ..ReviewDigest::default()
        });
        assert!(digest.contains("tab close github"), "{digest}");
        assert!(!digest.contains("Outlook"), "{digest}");
    }
}
