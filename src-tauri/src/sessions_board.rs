//! Sessions board: place shell conversations into columns by rules and
//! operator pins (not model classification). Separate from the Cards
//! workboard (`workboard.json`); this file is `sessions_board.json`.
//!
//! Clean-room inspiration from OpenClaw's Sessions board idea only — no
//! copied code. Placement: pin wins, else the first matching column rule,
//! else the fallback column. Personal filters are viewer-local. The Board
//! agent reshape path is a deterministic parser (Auto only; no model picker).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

pub const FILE: &str = "sessions_board.json";
/// Activity window for which chats appear on the board (72 hours).
pub const ACTIVE_MS: u64 = 72 * 60 * 60 * 1000;
/// Plain-text preview cap on a tile.
pub const PREVIEW_MAX: usize = 400;
/// At most this many PR links on a tile.
pub const PR_LINK_MAX: usize = 4;

pub fn path(config_dir: &Path) -> PathBuf {
    config_dir.join(FILE)
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// A column placement rule over session facts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Rule {
    Waiting,
    Failed,
    Running,
    PrOpen,
    PrMerged,
    Idle,
}

impl Rule {
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "waiting" | "needs_input" | "needs-input" => Some(Self::Waiting),
            "failed" | "stuck" => Some(Self::Failed),
            "running" | "working" => Some(Self::Running),
            "pr_open" | "pr-open" | "in_review" | "in-review" => Some(Self::PrOpen),
            "pr_merged" | "pr-merged" | "merged" => Some(Self::PrMerged),
            "idle" | "done" => Some(Self::Idle),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Waiting => "waiting",
            Self::Failed => "failed",
            Self::Running => "running",
            Self::PrOpen => "pr_open",
            Self::PrMerged => "pr_merged",
            Self::Idle => "idle",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ColumnSpec {
    pub id: String,
    pub label: String,
    pub rule: Rule,
    /// When no other column matched.
    #[serde(default)]
    pub fallback: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BoardSpec {
    pub name: String,
    pub columns: Vec<ColumnSpec>,
    /// Conversation id → column id. Pins win over rules.
    #[serde(default)]
    pub pins: HashMap<String, String>,
}

impl Default for BoardSpec {
    fn default() -> Self {
        Self {
            name: "Sessions".into(),
            columns: default_columns(),
            pins: HashMap::new(),
        }
    }
}

pub fn default_columns() -> Vec<ColumnSpec> {
    vec![
        ColumnSpec {
            id: "needs-input".into(),
            label: "Needs input".into(),
            rule: Rule::Waiting,
            fallback: false,
        },
        ColumnSpec {
            id: "stuck".into(),
            label: "Stuck".into(),
            rule: Rule::Failed,
            fallback: false,
        },
        ColumnSpec {
            id: "working".into(),
            label: "Working".into(),
            rule: Rule::Running,
            fallback: false,
        },
        ColumnSpec {
            id: "in-review".into(),
            label: "In review".into(),
            rule: Rule::PrOpen,
            fallback: false,
        },
        ColumnSpec {
            id: "merged".into(),
            label: "Merged".into(),
            rule: Rule::PrMerged,
            fallback: false,
        },
        ColumnSpec {
            id: "done".into(),
            label: "Done".into(),
            rule: Rule::Idle,
            fallback: true,
        },
    ]
}

/// Live facts about one conversation used for placement and tiles.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionFact {
    pub id: String,
    pub title: String,
    pub updated_ms: u64,
    pub preview: String,
    pub run: RunStatus,
    pub prs: Vec<PrLink>,
    /// True when this chat involves the viewer (they sent a turn).
    pub involving_me: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Waiting,
    Failed,
    Running,
    Idle,
}

impl RunStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Waiting => "waiting",
            Self::Failed => "failed",
            Self::Running => "running",
            Self::Idle => "idle",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrLink {
    pub url: String,
    /// open, draft, merged, or closed.
    pub state: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PersonalFilter {
    Everyone,
    InvolvingMe,
}

impl PersonalFilter {
    pub fn parse(raw: &str) -> Self {
        match raw.trim().to_ascii_lowercase().as_str() {
            "involving_me" | "involving-me" | "involvingme" | "me" => Self::InvolvingMe,
            _ => Self::Everyone,
        }
    }
}

/// One tile on the Sessions board page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Tile {
    pub id: String,
    pub title: String,
    pub preview: String,
    pub run: &'static str,
    pub prs: Vec<PrLink>,
    pub updated_ms: u64,
    pub pinned: bool,
    pub column_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ColumnView {
    pub id: String,
    pub label: String,
    pub rule: &'static str,
    pub fallback: bool,
    pub tiles: Vec<Tile>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Page {
    pub name: String,
    pub columns: Vec<ColumnView>,
    pub filter: PersonalFilter,
    /// Title substring filter (case-insensitive); empty means none.
    pub query: String,
    pub stamp: u64,
    pub total: u32,
    pub shown: u32,
}

/// Board agent reshape reply: a note plus the refreshed page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReshapeResult {
    pub note: String,
    pub page: Page,
}

pub fn load(config_dir: &Path) -> BoardSpec {
    let raw = std::fs::read_to_string(path(config_dir)).unwrap_or_default();
    if raw.trim().is_empty() {
        return BoardSpec::default();
    }
    serde_json::from_str(&raw).unwrap_or_default()
}

pub fn save(config_dir: &Path, spec: &BoardSpec) -> Result<(), String> {
    std::fs::create_dir_all(config_dir).map_err(|e| e.to_string())?;
    let raw = serde_json::to_string_pretty(spec).map_err(|e| e.to_string())?;
    let tmp = config_dir.join(format!("{FILE}.tmp"));
    std::fs::write(&tmp, raw).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, path(config_dir)).map_err(|e| e.to_string())?;
    Ok(())
}

pub fn stamp(config_dir: &Path) -> u64 {
    std::fs::metadata(path(config_dir))
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Whether a fact matches a column rule.
pub fn matches_rule(fact: &SessionFact, rule: Rule) -> bool {
    match rule {
        Rule::Waiting => fact.run == RunStatus::Waiting,
        Rule::Failed => fact.run == RunStatus::Failed,
        Rule::Running => fact.run == RunStatus::Running,
        Rule::PrOpen => fact
            .prs
            .iter()
            .any(|p| p.state == "open" || p.state == "draft"),
        Rule::PrMerged => fact.prs.iter().any(|p| p.state == "merged"),
        Rule::Idle => fact.run == RunStatus::Idle && fact.prs.is_empty(),
    }
}

/// Pick the column id for one session. Pin wins; else first matching rule;
/// else the fallback column (or the last column when none is marked).
pub fn place(spec: &BoardSpec, fact: &SessionFact) -> String {
    if let Some(col) = spec.pins.get(&fact.id) {
        if spec.columns.iter().any(|c| c.id == *col) {
            return col.clone();
        }
    }
    for col in &spec.columns {
        if matches_rule(fact, col.rule) {
            return col.id.clone();
        }
    }
    spec.columns
        .iter()
        .find(|c| c.fallback)
        .or_else(|| spec.columns.last())
        .map(|c| c.id.clone())
        .unwrap_or_else(|| "done".into())
}

fn passes_filter(fact: &SessionFact, filter: PersonalFilter, query: &str) -> bool {
    if filter == PersonalFilter::InvolvingMe && !fact.involving_me {
        return false;
    }
    let q = query.trim().to_ascii_lowercase();
    if q.is_empty() {
        return true;
    }
    fact.title.to_ascii_lowercase().contains(&q)
        || fact.preview.to_ascii_lowercase().contains(&q)
}

/// Build the page from a board spec and live session facts.
pub fn build_page(
    spec: &BoardSpec,
    facts: &[SessionFact],
    filter: PersonalFilter,
    query: &str,
    stamp: u64,
) -> Page {
    let mut columns: Vec<ColumnView> = spec
        .columns
        .iter()
        .map(|c| ColumnView {
            id: c.id.clone(),
            label: c.label.clone(),
            rule: c.rule.as_str(),
            fallback: c.fallback,
            tiles: Vec::new(),
        })
        .collect();
    let mut shown = 0u32;
    let total = facts.len() as u32;
    for fact in facts {
        if !passes_filter(fact, filter, query) {
            continue;
        }
        let column_id = place(spec, fact);
        let pinned = spec.pins.contains_key(&fact.id);
        let tile = Tile {
            id: fact.id.clone(),
            title: fact.title.clone(),
            preview: clip_preview(&fact.preview),
            run: fact.run.as_str(),
            prs: fact.prs.iter().take(PR_LINK_MAX).cloned().collect(),
            updated_ms: fact.updated_ms,
            pinned,
            column_id: column_id.clone(),
        };
        if let Some(col) = columns.iter_mut().find(|c| c.id == column_id) {
            col.tiles.push(tile);
            shown += 1;
        }
    }
    for col in &mut columns {
        col.tiles.sort_by(|a, b| {
            b.updated_ms
                .cmp(&a.updated_ms)
                .then(a.id.cmp(&b.id))
        });
    }
    Page {
        name: spec.name.clone(),
        columns,
        filter,
        query: query.to_string(),
        stamp,
        total,
        shown,
    }
}

fn clip_preview(text: &str) -> String {
    let one: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if one.chars().count() <= PREVIEW_MAX {
        return one;
    }
    let cut: String = one.chars().take(PREVIEW_MAX - 1).collect();
    format!("{}…", cut.trim_end())
}

/// Pin a conversation to a column, or clear the pin when `column_id` is empty.
pub fn set_pin(spec: &mut BoardSpec, chat_id: &str, column_id: &str) -> Result<(), String> {
    let id = chat_id.trim();
    if id.is_empty() {
        return Err("Give a chat id.".into());
    }
    let col = column_id.trim();
    if col.is_empty() {
        spec.pins.remove(id);
        return Ok(());
    }
    if !spec.columns.iter().any(|c| c.id == col) {
        return Err("That column is not on this board.".into());
    }
    spec.pins.insert(id.to_string(), col.to_string());
    Ok(())
}

/// Board agent reshape: apply a short instruction without a model picker.
/// Supported forms (case-insensitive):
/// - `rename <column-id> to <label>`
/// - `rule <column-id> <rule>`
/// - `fallback <column-id>`
/// - `move <column-id> before <column-id>`
/// - `move <column-id> after <column-id>`
/// - `reset columns` — restore the default column set; pins stay
pub fn reshape(spec: &mut BoardSpec, instruction: &str) -> Result<String, String> {
    let text = instruction.trim();
    if text.is_empty() {
        return Err("Say how to reshape the board.".into());
    }
    let lower = text.to_ascii_lowercase();
    if lower == "reset columns" || lower == "reset" {
        let pins = spec.pins.clone();
        *spec = BoardSpec {
            name: spec.name.clone(),
            columns: default_columns(),
            pins,
        };
        return Ok("Restored the default columns.".into());
    }
    if let Some(rest) = strip_prefix_ci(&lower, "rename ") {
        let (id, label) = split_to(rest, text)?;
        let col = spec
            .columns
            .iter_mut()
            .find(|c| c.id.eq_ignore_ascii_case(id))
            .ok_or_else(|| "That column is not on this board.".to_string())?;
        col.label = label.to_string();
        return Ok(format!("Renamed {id} to {label}."));
    }
    if let Some(rest) = strip_prefix_ci(&lower, "rule ") {
        let mut parts = rest.split_whitespace();
        let id = parts
            .next()
            .ok_or_else(|| "Say which column and rule.".to_string())?;
        let rule_raw = parts
            .next()
            .ok_or_else(|| "Say which rule.".to_string())?;
        let rule = Rule::parse(rule_raw).ok_or_else(|| {
            "Use waiting, failed, running, pr_open, pr_merged, or idle.".to_string()
        })?;
        let col = spec
            .columns
            .iter_mut()
            .find(|c| c.id.eq_ignore_ascii_case(id))
            .ok_or_else(|| "That column is not on this board.".to_string())?;
        col.rule = rule;
        return Ok(format!("Set {id} rule to {}.", rule.as_str()));
    }
    if let Some(rest) = strip_prefix_ci(&lower, "fallback ") {
        let id = rest
            .split_whitespace()
            .next()
            .ok_or_else(|| "Say which column is the fallback.".to_string())?;
        if !spec.columns.iter().any(|c| c.id.eq_ignore_ascii_case(id)) {
            return Err("That column is not on this board.".into());
        }
        for c in &mut spec.columns {
            c.fallback = c.id.eq_ignore_ascii_case(id);
        }
        return Ok(format!("Fallback is now {id}."));
    }
    if let Some(rest) = strip_prefix_ci(&lower, "move ") {
        let (id, where_part) = split_first_word(rest);
        let id = id.ok_or_else(|| "Say which column to move.".to_string())?;
        let where_part = where_part.trim();
        let (before, anchor) = if let Some(a) = strip_prefix_ci(where_part, "before ") {
            (true, a.trim())
        } else if let Some(a) = strip_prefix_ci(where_part, "after ") {
            (false, a.trim())
        } else {
            return Err("Say move <id> before|after <id>.".into());
        };
        if anchor.is_empty() {
            return Err("Say the anchor column.".into());
        }
        move_column(&mut spec.columns, id, anchor, before)?;
        return Ok(format!(
            "Moved {id} {} {anchor}.",
            if before { "before" } else { "after" }
        ));
    }
    Err(
        "Try: rename <id> to <label>, rule <id> <rule>, fallback <id>, move <id> before|after <id>, or reset columns."
            .into(),
    )
}

fn strip_prefix_ci<'a>(lower: &'a str, prefix: &str) -> Option<&'a str> {
    lower.strip_prefix(prefix)
}

fn split_to<'a>(lower_rest: &'a str, original: &'a str) -> Result<(&'a str, &'a str), String> {
    let idx = lower_rest
        .find(" to ")
        .ok_or_else(|| "Say rename <id> to <label>.".to_string())?;
    // Map back into the original casing for the label.
    let orig_rest_start = original.len().saturating_sub(lower_rest.len());
    let orig_rest = &original[orig_rest_start..];
    let id = orig_rest[..idx].trim();
    let label = orig_rest[idx + 4..].trim();
    if id.is_empty() || label.is_empty() {
        return Err("Say rename <id> to <label>.".into());
    }
    Ok((id, label))
}

fn split_first_word(s: &str) -> (Option<&str>, &str) {
    let s = s.trim_start();
    match s.find(char::is_whitespace) {
        Some(i) => (Some(s[..i].trim()), &s[i..]),
        None => (Some(s).filter(|x| !x.is_empty()), ""),
    }
}

fn move_column(
    columns: &mut Vec<ColumnSpec>,
    id: &str,
    anchor: &str,
    before: bool,
) -> Result<(), String> {
    let from = columns
        .iter()
        .position(|c| c.id.eq_ignore_ascii_case(id))
        .ok_or_else(|| "That column is not on this board.".to_string())?;
    let to = columns
        .iter()
        .position(|c| c.id.eq_ignore_ascii_case(anchor))
        .ok_or_else(|| "The anchor column is not on this board.".to_string())?;
    if from == to {
        return Ok(());
    }
    let col = columns.remove(from);
    let to = columns
        .iter()
        .position(|c| c.id.eq_ignore_ascii_case(anchor))
        .ok_or_else(|| "The anchor column is not on this board.".to_string())?;
    let insert_at = if before { to } else { to + 1 };
    columns.insert(insert_at, col);
    Ok(())
}

/// Pull github.com pull-request URLs out of transcript text.
pub fn extract_prs(text: &str) -> Vec<PrLink> {
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for word in text.split_whitespace() {
        let cleaned = word.trim_matches(|c: char| {
            matches!(c, ')' | '(' | '[' | ']' | '"' | '\'' | ',' | '.' | ';' | '!')
        });
        let lower = cleaned.to_ascii_lowercase();
        let Some(idx) = lower.find("github.com/") else {
            continue;
        };
        let path = &lower[idx + "github.com/".len()..];
        let parts: Vec<&str> = path.split('/').collect();
        if parts.len() < 4 || parts[2] != "pull" {
            continue;
        }
        let num = parts[3]
            .trim_end_matches(|c: char| !c.is_ascii_digit())
            .to_string();
        if num.is_empty() || !num.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        let owner = parts[0];
        let repo = parts[1];
        let url = format!("https://github.com/{owner}/{repo}/pull/{num}");
        if !seen.insert(url.clone()) {
            continue;
        }
        let state = pr_state_near(text, &url, cleaned);
        out.push(PrLink { url, state });
        if out.len() >= PR_LINK_MAX {
            break;
        }
    }
    out
}

fn pr_state_near(hay: &str, url: &str, raw: &str) -> String {
    let lower = hay.to_ascii_lowercase();
    let key = url.to_ascii_lowercase();
    let around = if let Some(i) = lower.find(&key) {
        let start = i.saturating_sub(16);
        let end = (i + key.len() + 16).min(lower.len());
        lower[start..end].to_string()
    } else {
        raw.to_ascii_lowercase()
    };
    // Prefer the token closest to the URL; scan outward from the URL span.
    let url_at = around.find(&key).unwrap_or(around.len() / 2);
    let mut best: Option<(usize, &'static str)> = None;
    let mut offset = 0usize;
    for token in around.split(|c: char| !c.is_ascii_alphabetic()) {
        let dist = offset.abs_diff(url_at);
        let state = match token {
            "draft" => Some("draft"),
            "merged" => Some("merged"),
            "closed" => Some("closed"),
            _ => None,
        };
        if let Some(state) = state {
            if best.is_none_or(|(d, _)| dist < d) {
                best = Some((dist, state));
            }
        }
        offset += token.len() + 1;
    }
    best.map(|(_, s)| s.to_string()).unwrap_or_else(|| "open".into())
}

/// Keep chats with activity inside the board window.
pub fn recent_enough(updated_ms: u64, now: u64) -> bool {
    now.saturating_sub(updated_ms) <= ACTIVE_MS
}

/// Build a page from config + supplied facts (tests and the shell command).
pub fn page_from(
    config_dir: &Path,
    facts: Vec<SessionFact>,
    filter: PersonalFilter,
    query: &str,
) -> Page {
    let now = now_ms();
    let facts: Vec<SessionFact> = facts
        .into_iter()
        .filter(|f| recent_enough(f.updated_ms, now) || f.updated_ms == 0)
        .collect();
    let spec = load(config_dir);
    build_page(&spec, &facts, filter, query, stamp(config_dir))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fact(id: &str, run: RunStatus, prs: Vec<PrLink>) -> SessionFact {
        SessionFact {
            id: id.into(),
            title: id.into(),
            updated_ms: 1_000,
            preview: "hello".into(),
            run,
            prs,
            involving_me: true,
        }
    }

    #[test]
    fn default_columns_match_openclaw_order() {
        let cols = default_columns();
        let ids: Vec<_> = cols.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(
            ids,
            [
                "needs-input",
                "stuck",
                "working",
                "in-review",
                "merged",
                "done"
            ]
        );
        assert!(cols.last().unwrap().fallback);
    }

    #[test]
    fn pin_beats_rule() {
        let mut spec = BoardSpec::default();
        spec.pins
            .insert("shell-a".into(), "done".into());
        let f = fact("shell-a", RunStatus::Running, vec![]);
        assert_eq!(place(&spec, &f), "done");
    }

    #[test]
    fn rules_place_in_column_order() {
        let spec = BoardSpec::default();
        assert_eq!(
            place(&spec, &fact("a", RunStatus::Waiting, vec![])),
            "needs-input"
        );
        assert_eq!(
            place(&spec, &fact("b", RunStatus::Failed, vec![])),
            "stuck"
        );
        assert_eq!(
            place(&spec, &fact("c", RunStatus::Running, vec![])),
            "working"
        );
        let open = vec![PrLink {
            url: "https://github.com/o/r/pull/1".into(),
            state: "open".into(),
        }];
        assert_eq!(
            place(&spec, &fact("d", RunStatus::Idle, open)),
            "in-review"
        );
        let merged = vec![PrLink {
            url: "https://github.com/o/r/pull/2".into(),
            state: "merged".into(),
        }];
        assert_eq!(
            place(&spec, &fact("e", RunStatus::Idle, merged)),
            "merged"
        );
        assert_eq!(place(&spec, &fact("f", RunStatus::Idle, vec![])), "done");
    }

    #[test]
    fn personal_filter_and_query_hide_tiles() {
        let spec = BoardSpec::default();
        let facts = vec![
            SessionFact {
                id: "shell-me".into(),
                title: "Mine".into(),
                updated_ms: 1,
                preview: "alpha".into(),
                run: RunStatus::Idle,
                prs: vec![],
                involving_me: true,
            },
            SessionFact {
                id: "shell-other".into(),
                title: "Other".into(),
                updated_ms: 2,
                preview: "beta".into(),
                run: RunStatus::Idle,
                prs: vec![],
                involving_me: false,
            },
        ];
        let page = build_page(&spec, &facts, PersonalFilter::InvolvingMe, "", 0);
        let ids: Vec<_> = page
            .columns
            .iter()
            .flat_map(|c| c.tiles.iter().map(|t| t.id.as_str()))
            .collect();
        assert_eq!(ids, ["shell-me"]);
        let page = build_page(&spec, &facts, PersonalFilter::Everyone, "oth", 0);
        let ids: Vec<_> = page
            .columns
            .iter()
            .flat_map(|c| c.tiles.iter().map(|t| t.id.as_str()))
            .collect();
        assert_eq!(ids, ["shell-other"]);
    }

    #[test]
    fn board_agent_reshape_renames_moves_and_resets() {
        let mut spec = BoardSpec::default();
        assert!(reshape(&mut spec, "rename working to Active")
            .unwrap()
            .contains("Active"));
        assert_eq!(
            spec.columns.iter().find(|c| c.id == "working").unwrap().label,
            "Active"
        );
        reshape(&mut spec, "move done before needs-input").unwrap();
        assert_eq!(spec.columns[0].id, "done");
        reshape(&mut spec, "rule stuck waiting").unwrap();
        assert_eq!(
            spec.columns.iter().find(|c| c.id == "stuck").unwrap().rule,
            Rule::Waiting
        );
        reshape(&mut spec, "fallback working").unwrap();
        assert!(spec.columns.iter().find(|c| c.id == "working").unwrap().fallback);
        assert!(!spec.columns.iter().find(|c| c.id == "done").unwrap().fallback);
        spec.pins.insert("shell-x".into(), "working".into());
        reshape(&mut spec, "reset columns").unwrap();
        assert_eq!(spec.columns[0].id, "needs-input");
        assert_eq!(spec.pins.get("shell-x").map(String::as_str), Some("working"));
    }

    #[test]
    fn extract_prs_reads_github_pull_urls_and_state() {
        let text = "Opened https://github.com/o/r/pull/12 draft and later merged https://github.com/o/r/pull/13.";
        let prs = extract_prs(text);
        assert_eq!(prs.len(), 2);
        assert_eq!(prs[0].url, "https://github.com/o/r/pull/12");
        assert_eq!(prs[0].state, "draft");
        assert_eq!(prs[1].url, "https://github.com/o/r/pull/13");
        assert_eq!(prs[1].state, "merged");
    }

    #[test]
    fn save_and_load_round_trip() {
        let dir = std::env::temp_dir().join(format!(
            "gh-sessions-board-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut spec = BoardSpec::default();
        spec.name = "Ops".into();
        spec.pins.insert("shell-z".into(), "working".into());
        save(&dir, &spec).unwrap();
        let got = load(&dir);
        assert_eq!(got.name, "Ops");
        assert_eq!(got.pins.get("shell-z").map(String::as_str), Some("working"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn cards_workboard_file_name_stays_separate() {
        assert_ne!(FILE, crate::workboard::FILE);
    }
}
