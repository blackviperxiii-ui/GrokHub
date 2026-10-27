//! Brain 1 PR 2: the mode policy row for a Brief. The table is data in
//! `grokhub_core::brain_policy`; the user's copy lives at
//! `{config}/brain/brain_policy.toml`. A user file that fails its checks is
//! ignored whole and the shipped rows apply.

use std::path::{Path, PathBuf};

use grokhub_core::brain_policy::{builtin_policy, merge_policy, parse_policy, pick_row, PolicyRow};
use serde::{Deserialize, Serialize};

use super::Brief;

/// The matched row, as logged next to a Brief.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolicyPick {
    /// "repair × critical", or "any × any" for the catch-all.
    pub row: String,
    pub goal: String,
    pub reason: String,
    /// The row's talk column (Brain 5 picks the turn's talk style from it).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub talk: String,
}

impl PolicyPick {
    /// "repair × critical, goal quality (mistakes can break the system)"
    pub fn label(&self) -> String {
        format!("{}, goal {} ({})", self.row, self.goal, self.reason)
    }

    /// How the turn talks ([`grokhub_core::talk`]).
    pub fn style(&self) -> grokhub_core::talk::TalkStyle {
        grokhub_core::talk::TalkStyle::of(&self.talk)
    }
}

/// `{config}/brain/brain_policy.toml`
pub fn policy_path(config_dir: &Path) -> PathBuf {
    config_dir.join("brain").join("brain_policy.toml")
}

/// The shipped rows with the user's file on top, and why the user's file was
/// ignored when it was. This is the file, not the care-first overlay.
pub fn load_policy(config_dir: &Path) -> (Vec<PolicyRow>, Option<String>) {
    let Ok(text) = std::fs::read_to_string(policy_path(config_dir)) else {
        return (builtin_policy(), None);
    };
    match parse_policy(&text) {
        Ok(user) => (merge_policy(builtin_policy(), user), None),
        Err(e) => (builtin_policy(), Some(e)),
    }
}

/// `care_first` in the user's policy file. A missing or refused file is off.
pub fn care_first_enabled(config_dir: &Path) -> bool {
    let Ok(text) = std::fs::read_to_string(policy_path(config_dir)) else {
        return false;
    };
    parse_policy(&text)
        .ok()
        .is_some_and(|_| grokhub_core::brain_policy::care_first_flag(&text))
}

/// Rows for a turn. Care-first replaces chat casual after the user's file merges.
pub fn effective_rows(config_dir: &Path) -> (Vec<PolicyRow>, Option<String>) {
    let (rows, note) = load_policy(config_dir);
    if care_first_enabled(config_dir) {
        (grokhub_core::brain_policy::apply_care_first(rows), note)
    } else {
        (rows, note)
    }
}

/// The row a Brief matches.
pub fn pick_policy(rows: &[PolicyRow], brief: &Brief) -> Option<PolicyPick> {
    pick_row(rows, brief.intent.as_str(), brief.care.as_str()).map(|r| PolicyPick {
        row: r.key(),
        goal: r.goal.clone(),
        reason: r.reason.clone(),
        talk: r.talk.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_user_file_wins_and_a_bad_one_is_ignored_with_a_note() {
        let dir = crate::harness::test_dir("brain-policy");
        let brief = super::super::rules(&super::super::BriefInput {
            text: "hi",
            ..Default::default()
        })
        .unwrap();
        let shipped = pick_policy(&load_policy(&dir).0, &brief).unwrap();
        assert_eq!(
            shipped.label(),
            "chat × casual, goal speed (small talk, fast reply matters most)"
        );
        assert_eq!(
            (shipped.talk.as_str(), shipped.style()),
            (
                "brief, personality on",
                grokhub_core::talk::TalkStyle::Short
            )
        );
        assert_eq!(load_policy(&dir).1, None);
        std::fs::create_dir_all(policy_path(&dir).parent().unwrap()).unwrap();
        std::fs::write(
            policy_path(&dir),
            "[[row]]\nintent = \"chat\"\ncare = \"casual\"\ngoal = \"cost\"\nreason = \"keep it cheap\"\ndepth = \"none\"\nask = \"act\"\nstop = \"answered\"\ntalk = \"one line\"\n",
        )
        .unwrap();
        let (rows, note) = load_policy(&dir);
        assert_eq!(note, None);
        assert_eq!(
            pick_policy(&rows, &brief).unwrap().label(),
            "chat × casual, goal cost (keep it cheap)"
        );
        std::fs::write(policy_path(&dir), "require_approval = false\n").unwrap();
        let (rows, note) = load_policy(&dir);
        assert_eq!(pick_policy(&rows, &brief), Some(shipped));
        assert_eq!(note.as_deref(), Some("`require_approval` is a safety key; safety and approval rules aren't in the policy table"));
        assert!(!care_first_enabled(&dir));
        assert_eq!(
            super::super::why_lines(&dir),
            vec!["Policy file ignored (`require_approval` is a safety key; safety and approval rules aren't in the policy table); using the shipped rows.".to_string()]
        );
        std::fs::write(
            policy_path(&dir),
            "care_first = true\n[[row]]\nintent = \"chat\"\ncare = \"casual\"\ngoal = \"cost\"\nreason = \"keep it cheap\"\ndepth = \"none\"\nask = \"act\"\nstop = \"answered\"\ntalk = \"one line\"\n",
        )
        .unwrap();
        assert!(care_first_enabled(&dir));
        let (rows, note) = effective_rows(&dir);
        assert_eq!(note, None);
        let picked = pick_policy(&rows, &brief).unwrap();
        assert_eq!(
            (picked.goal.as_str(), picked.talk.as_str(), picked.style()),
            ("care", "natural", grokhub_core::talk::TalkStyle::Natural)
        );
        assert_eq!(
            load_policy(&dir)
                .0
                .iter()
                .find(|r| r.key() == "chat × casual")
                .unwrap()
                .goal,
            "cost"
        );
        let _ = std::fs::remove_dir_all(dir);
    }
}
