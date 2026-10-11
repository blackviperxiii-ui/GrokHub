//! Brain mode policy table: intent × care → model goal and reason, depth,
//! ask vs act, stop rule and talk style. Data, not code
//! (`assets/brain_policy.toml`), and a user copy may replace or add rows.
//! Safety and approval rules are never rows: a file naming such a key is
//! refused whole. Phase 1 only logs the matched row.

use serde::{Deserialize, Serialize};

const BUILTIN: &str = include_str!("../assets/brain_policy.toml");

/// Matches every intent or every care.
pub const ANY: &str = "any";
pub const INTENTS: &[&str] = &["chat", "question", "task", "repair", "creative", "research", "background"];
pub const CARES: &[&str] = &["casual", "careful", "critical"];
pub const GOALS: &[&str] = &["speed", "quality", "cost", "balanced"];
/// Longest text a row field may hold.
pub const FIELD_MAX: usize = 120;

/// A key holding any of these is a safety key, wherever it sits in the file.
const SAFETY_KEYS: &[&str] = &[
    "approv", "hard", "perm", "safe", "consent", "mindcheck", "gate", "sandbox", "network", "honest", "sudo", "allow",
    "deny", "bypass", "credential", "risk",
];
/// An `ask` that would drop the hard asks.
const ASK_REFUSED: &[&str] = &["bypass", "skip approval", "no approval", "without approval", "don't ask", "never ask", "without asking"];

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PolicyRow {
    pub intent: String,
    pub care: String,
    pub goal: String,
    pub reason: String,
    pub depth: String,
    pub ask: String,
    pub stop: String,
    pub talk: String,
}

impl PolicyRow {
    /// "repair × critical"
    pub fn key(&self) -> String {
        format!("{} × {}", self.intent, self.care)
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PolicyFile {
    #[serde(default)]
    row: Vec<PolicyRow>,
}

fn safety_key(table: &toml::Table) -> Option<String> {
    for (k, v) in table {
        let lower = k.to_ascii_lowercase();
        if SAFETY_KEYS.iter().any(|s| lower.contains(s)) {
            return Some(k.clone());
        }
        let nested = match v {
            toml::Value::Table(t) => safety_key(t),
            toml::Value::Array(items) => items.iter().find_map(|i| i.as_table().and_then(safety_key)),
            _ => None,
        };
        if nested.is_some() {
            return nested;
        }
    }
    None
}

fn check_row(r: &PolicyRow) -> Result<(), String> {
    let key = r.key();
    if r.intent != ANY && !INTENTS.contains(&r.intent.as_str()) {
        return Err(format!("{key}: unknown intent `{}`", r.intent));
    }
    if r.care != ANY && !CARES.contains(&r.care.as_str()) {
        return Err(format!("{key}: unknown care `{}`", r.care));
    }
    if !GOALS.contains(&r.goal.as_str()) {
        return Err(format!("{key}: goal must be one of {}", GOALS.join(", ")));
    }
    for (name, text) in [("reason", &r.reason), ("depth", &r.depth), ("ask", &r.ask), ("stop", &r.stop), ("talk", &r.talk)] {
        if text.trim().is_empty() || text.chars().count() > FIELD_MAX {
            return Err(format!("{key}: {name} must be 1 to {FIELD_MAX} characters"));
        }
    }
    let ask = r.ask.to_ascii_lowercase();
    if ASK_REFUSED.iter().any(|w| ask.contains(w)) {
        return Err(format!("{key}: ask can't drop approvals; hard approvals always ask"));
    }
    Ok(())
}

/// Parse and check one policy file. Any safety key, unknown key, bad word or
/// repeated intent × care refuses the whole file.
pub fn parse_policy(text: &str) -> Result<Vec<PolicyRow>, String> {
    let table: toml::Table = toml::from_str(text).map_err(|e| format!("not valid TOML: {}", e.message()))?;
    if let Some(k) = safety_key(&table) {
        return Err(format!("`{k}` is a safety key; safety and approval rules aren't in the policy table"));
    }
    let file: PolicyFile = toml::from_str(text).map_err(|e| e.message().to_string())?;
    let mut seen: Vec<String> = Vec::new();
    for r in &file.row {
        check_row(r)?;
        let key = r.key();
        if seen.contains(&key) {
            return Err(format!("{key} is listed twice"));
        }
        seen.push(key);
    }
    Ok(file.row)
}

/// The shipped rows.
pub fn builtin_policy() -> Vec<PolicyRow> {
    parse_policy(BUILTIN).unwrap_or_default()
}

/// The shipped rows with the user's: a user row replaces the shipped row with
/// the same intent × care, and a new pair is added.
pub fn merge_policy(defaults: Vec<PolicyRow>, user: Vec<PolicyRow>) -> Vec<PolicyRow> {
    let mut rows = defaults;
    for u in user {
        match rows.iter_mut().find(|r| r.intent == u.intent && r.care == u.care) {
            Some(r) => *r = u,
            None => rows.push(u),
        }
    }
    rows
}

/// The row for an intent and care: exact, then intent × any, then any × care,
/// then any × any.
pub fn pick_row<'a>(rows: &'a [PolicyRow], intent: &str, care: &str) -> Option<&'a PolicyRow> {
    let find = |i: &str, c: &str| rows.iter().find(|r| r.intent == i && r.care == c);
    find(intent, care).or_else(|| find(intent, ANY)).or_else(|| find(ANY, care)).or_else(|| find(ANY, ANY))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shipped_policy_loads_with_the_design_rows() {
        let rows = parse_policy(BUILTIN).unwrap();
        let keys: Vec<String> = rows.iter().map(PolicyRow::key).collect();
        assert_eq!(
            keys,
            vec![
                "chat × casual",
                "question × careful",
                "task × careful",
                "repair × critical",
                "research × careful",
                "background × any",
                "question × any",
                "task × any",
                "creative × any",
                "any × critical",
                "any × any",
            ]
        );
        let repair = pick_row(&rows, "repair", "critical").unwrap();
        assert_eq!((repair.goal.as_str(), repair.reason.as_str()), ("quality", "mistakes can break the system"));
        assert_eq!(pick_row(&rows, "background", "critical").unwrap().key(), "background × any");
        assert_eq!(pick_row(&rows, "chat", "critical").unwrap().key(), "any × critical");
        assert_eq!(pick_row(&rows, "chat", "careful").unwrap().key(), "any × any");
        assert_eq!(builtin_policy(), rows);
    }

    #[test]
    fn safety_keys_and_bad_rows_refuse_the_whole_file() {
        let row = |extra: &str, ask: &str| {
            format!(
                "[[row]]\nintent = \"chat\"\ncare = \"casual\"\ngoal = \"speed\"\nreason = \"r\"\ndepth = \"d\"\nask = \"{ask}\"\nstop = \"s\"\ntalk = \"t\"\n{extra}"
            )
        };
        assert_eq!(parse_policy(&row("", "act")).unwrap().len(), 1);
        assert_eq!(
            parse_policy(&row("approvals = \"off\"\n", "act")).unwrap_err(),
            "`approvals` is a safety key; safety and approval rules aren't in the policy table"
        );
        assert_eq!(
            parse_policy(&format!("hard_classes = []\n{}", row("", "act"))).unwrap_err(),
            "`hard_classes` is a safety key; safety and approval rules aren't in the policy table"
        );
        assert_eq!(
            parse_policy(&format!("[network]\nopen = true\n{}", row("", "act"))).unwrap_err(),
            "`network` is a safety key; safety and approval rules aren't in the policy table"
        );
        assert_eq!(parse_policy(&row("", "act without asking")).unwrap_err(), "chat × casual: ask can't drop approvals; hard approvals always ask");
        assert!(parse_policy(&row("colour = \"red\"\n", "act")).unwrap_err().contains("unknown field `colour`"));
        assert_eq!(parse_policy(&row("", "act").replace("speed", "fastest")).unwrap_err(), "chat × casual: goal must be one of speed, quality, cost, balanced");
        assert_eq!(parse_policy(&row("", "act").replace("\"chat\"", "\"gossip\"")).unwrap_err(), "gossip × casual: unknown intent `gossip`");
        assert_eq!(parse_policy(&row("", "act").replace("\"r\"", "\"\"")).unwrap_err(), "chat × casual: reason must be 1 to 120 characters");
        assert_eq!(parse_policy(&format!("{}{}", row("", "act"), row("", "act"))).unwrap_err(), "chat × casual is listed twice");
        assert!(parse_policy("[[row]\n").unwrap_err().starts_with("not valid TOML"));
    }

    #[test]
    fn a_user_row_wins_and_a_new_pair_is_added() {
        let user = parse_policy(
            "[[row]]\nintent = \"chat\"\ncare = \"casual\"\ngoal = \"cost\"\nreason = \"keep it cheap\"\ndepth = \"none\"\nask = \"act\"\nstop = \"answered\"\ntalk = \"one line\"\n\
             [[row]]\nintent = \"creative\"\ncare = \"careful\"\ngoal = \"quality\"\nreason = \"for work\"\ndepth = \"two drafts\"\nask = \"act\"\nstop = \"draft delivered\"\ntalk = \"drafts\"\n",
        )
        .unwrap();
        let rows = merge_policy(builtin_policy(), user);
        assert_eq!(rows.len(), 12);
        let chat = pick_row(&rows, "chat", "casual").unwrap();
        assert_eq!((chat.goal.as_str(), chat.reason.as_str()), ("cost", "keep it cheap"));
        assert_eq!(pick_row(&rows, "creative", "careful").unwrap().reason, "for work");
        assert_eq!(pick_row(&rows, "creative", "casual").unwrap().key(), "creative × any");
    }
}
