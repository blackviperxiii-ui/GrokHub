//! A Code tab session's lead files, kept in the session's own folder
//! (`<config>/sessions/<id>/`, removed with the session): the mark of a
//! session the agent window started, its plan as last shown or edited, and
//! the mark of its first approved plan.

use std::path::{Path, PathBuf};

use crate::agent::SessionMode;

const LEAD: &str = "lead";
const PLAN: &str = "plan.md";
const APPROVED: &str = "plan.approved";

/// The id as a file name, or `None` for one that could leave the folder.
fn safe(conversation_id: &str) -> Option<&str> {
    let id = conversation_id.trim();
    let bad = id.is_empty()
        || id.len() > 200
        || id.contains('/')
        || id.contains('\\')
        || id.contains("..")
        || id.contains('\0');
    (!bad).then_some(id)
}

fn folder(config_dir: &Path, conversation_id: &str) -> Option<PathBuf> {
    Some(config_dir.join("sessions").join(safe(conversation_id)?))
}

fn write(config_dir: &Path, conversation_id: &str, name: &str, text: &str) -> std::io::Result<()> {
    let dir = folder(config_dir, conversation_id)
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, "bad session id"))?;
    std::fs::create_dir_all(&dir)?;
    std::fs::write(dir.join(name), text)
}

fn has(config_dir: &Path, conversation_id: &str, name: &str) -> bool {
    folder(config_dir, conversation_id).is_some_and(|dir| dir.join(name).is_file())
}

/// True once the session has saved turns.
fn saved(config_dir: &Path, conversation_id: &str) -> bool {
    safe(conversation_id).is_some_and(|id| {
        config_dir
            .join("sessions")
            .join(format!("{id}.jsonl"))
            .is_file()
    })
}

/// Marks the session as one the agent window started.
pub fn mark_lead(config_dir: &Path, conversation_id: &str) -> std::io::Result<()> {
    write(config_dir, conversation_id, LEAD, "")
}

/// True for a session the agent window started. A saved chat that the agent
/// window only shows is not one.
pub fn is_lead(config_dir: &Path, conversation_id: &str) -> bool {
    has(config_dir, conversation_id, LEAD)
}

/// The agent window shows the session. One with no saved turns yet started
/// there, so it gets the mark; a saved chat keeps what it has.
pub fn shown(config_dir: &Path, conversation_id: &str) {
    if !saved(config_dir, conversation_id) {
        let _ = mark_lead(config_dir, conversation_id);
    }
}

/// The session was closed. One that never saved a turn leaves no folder.
pub fn closed(config_dir: &Path, conversation_id: &str) {
    if saved(config_dir, conversation_id) {
        return;
    }
    if let Some(dir) = folder(config_dir, conversation_id) {
        let _ = std::fs::remove_file(dir.join(LEAD));
        // Fails, as it should, while the folder holds anything else.
        let _ = std::fs::remove_dir(&dir);
    }
}

pub fn save_plan(config_dir: &Path, conversation_id: &str, text: &str) -> std::io::Result<()> {
    write(config_dir, conversation_id, PLAN, text)
}

/// The session's plan as last shown, or as the person edited it.
pub fn read_plan(config_dir: &Path, conversation_id: &str) -> Option<String> {
    std::fs::read_to_string(folder(config_dir, conversation_id)?.join(PLAN)).ok()
}

/// Remembers that the session's first plan was approved.
pub fn approve(config_dir: &Path, conversation_id: &str) -> std::io::Result<()> {
    write(config_dir, conversation_id, APPROVED, "")
}

pub fn approved(config_dir: &Path, conversation_id: &str) -> bool {
    has(config_dir, conversation_id, APPROVED)
}

/// The mode of a lead session's message. Until its first plan is approved
/// every message plans, whatever the page asked for (an explicit `/chat` or
/// `/queue` too); a side answer stays a side answer.
pub fn first_mode(asked: Option<SessionMode>, lead: bool, approved: bool) -> Option<SessionMode> {
    if lead && !approved && asked != Some(SessionMode::Btw) {
        Some(SessionMode::Plan)
    } else {
        asked
    }
}

/// Which of a turn's plan events are the lead's own. A helper's plan reaches
/// the same stream, so the draft counts only while the session itself is in
/// plan mode, and the edited text only as the first event after its approval.
#[derive(Default)]
pub struct PlanWatch {
    drafted: bool,
}

impl PlanWatch {
    /// Reads one event of the turn: `plan` is its text when it is a plan
    /// event, and `planning` is whether the session is in plan mode now.
    /// Returns the text to save.
    pub fn keep<'a>(&mut self, plan: Option<&'a str>, planning: bool) -> Option<&'a str> {
        if planning {
            self.drafted |= plan.is_some();
            return plan;
        }
        // Plan mode just went off: the edited text, if any, comes first.
        let edited = plan.filter(|_| self.drafted);
        self.drafted = false;
        edited
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "grokhub-shell-lead-{name}-{}-{}",
            std::process::id(),
            grokhub_core::now_ms()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    #[test]
    fn a_saved_plan_reads_back() {
        let cfg = temp_dir("plan");
        assert_eq!(read_plan(&cfg, "s1"), None);
        save_plan(&cfg, "s1", "1. Read\n\nTasks\n1. Write the note").expect("save");
        assert_eq!(
            read_plan(&cfg, "s1").as_deref(),
            Some("1. Read\n\nTasks\n1. Write the note")
        );
        assert_eq!(
            std::fs::read_to_string(cfg.join("sessions").join("s1").join("plan.md")).expect("file"),
            "1. Read\n\nTasks\n1. Write the note"
        );
        save_plan(&cfg, "s1", "1. Edited").expect("save again");
        assert_eq!(read_plan(&cfg, "s1").as_deref(), Some("1. Edited"));
        let _ = std::fs::remove_dir_all(&cfg);
    }

    #[test]
    fn an_id_that_could_leave_the_folder_has_no_files() {
        let cfg = temp_dir("ids");
        for id in ["a/b", "a\\b", "..", "", "nul\0"] {
            assert_eq!(folder(&cfg, id), None, "{id:?}");
            assert!(save_plan(&cfg, id, "x").is_err(), "{id:?}");
            assert_eq!(read_plan(&cfg, id), None, "{id:?}");
            assert!(mark_lead(&cfg, id).is_err(), "{id:?}");
            assert!(!is_lead(&cfg, id), "{id:?}");
        }
        assert_eq!(folder(&cfg, "s1"), Some(cfg.join("sessions").join("s1")));
        let _ = std::fs::remove_dir_all(&cfg);
    }

    #[test]
    fn an_approval_is_remembered() {
        let cfg = temp_dir("approved");
        assert!(!approved(&cfg, "s1"));
        approve(&cfg, "s1").expect("approve");
        assert!(approved(&cfg, "s1"));
        assert!(!approved(&cfg, "s2"));
        let _ = std::fs::remove_dir_all(&cfg);
    }

    #[test]
    fn only_a_session_with_no_saved_turns_becomes_a_lead_when_shown() {
        let cfg = temp_dir("origin");
        std::fs::create_dir_all(cfg.join("sessions")).expect("sessions");
        std::fs::write(cfg.join("sessions").join("old-chat.jsonl"), "{}\n").expect("saved chat");
        shown(&cfg, "old-chat");
        assert!(!is_lead(&cfg, "old-chat"));
        assert!(!cfg.join("sessions").join("old-chat").exists());

        assert!(!is_lead(&cfg, "fresh"));
        shown(&cfg, "fresh");
        assert!(is_lead(&cfg, "fresh"));
        // Its turns are saved later; shown again, it is still a lead.
        std::fs::write(cfg.join("sessions").join("fresh.jsonl"), "{}\n").expect("saved turns");
        shown(&cfg, "fresh");
        assert!(is_lead(&cfg, "fresh"));
        let _ = std::fs::remove_dir_all(&cfg);
    }

    #[test]
    fn a_closed_session_with_no_turns_leaves_no_folder() {
        let cfg = temp_dir("closed");
        shown(&cfg, "empty");
        assert!(cfg.join("sessions").join("empty").join("lead").is_file());
        closed(&cfg, "empty");
        assert!(!cfg.join("sessions").join("empty").exists());

        shown(&cfg, "used");
        std::fs::write(cfg.join("sessions").join("used.jsonl"), "{}\n").expect("saved turns");
        closed(&cfg, "used");
        assert!(is_lead(&cfg, "used"));
        let _ = std::fs::remove_dir_all(&cfg);
    }

    #[test]
    fn a_lead_plans_until_its_first_approval() {
        use SessionMode::{Btw, Chat, Plan};
        assert_eq!(first_mode(None, true, false), Some(Plan));
        // An explicit /chat or /queue choice plans too.
        assert_eq!(first_mode(Some(Chat), true, false), Some(Plan));
        assert_eq!(first_mode(Some(Btw), true, false), Some(Btw));
        assert_eq!(first_mode(None, true, true), None);
        assert_eq!(first_mode(Some(Chat), true, true), Some(Chat));
        assert_eq!(first_mode(Some(Chat), false, false), Some(Chat));
        assert_eq!(first_mode(None, false, false), None);
    }

    #[test]
    fn only_the_leads_own_plan_events_are_kept() {
        let mut watch = PlanWatch::default();
        // The draft, shown while the session plans.
        assert_eq!(watch.keep(None, true), None);
        assert_eq!(watch.keep(Some("1. Draft"), true), Some("1. Draft"));
        assert_eq!(watch.keep(None, true), None);
        // Approved with edits: the edited text is the next event.
        assert_eq!(watch.keep(Some("1. Edited"), false), Some("1. Edited"));
        // A helper's plan later in the turn is not the lead's.
        assert_eq!(watch.keep(Some("helper plan"), false), None);

        // Approved as shown: the next event is something else, and a
        // helper's plan after it is dropped.
        let mut watch = PlanWatch::default();
        assert_eq!(watch.keep(Some("1. Draft"), true), Some("1. Draft"));
        assert_eq!(watch.keep(None, false), None);
        assert_eq!(watch.keep(Some("helper plan"), false), None);

        // A turn that never planned keeps nothing.
        let mut watch = PlanWatch::default();
        assert_eq!(watch.keep(Some("helper plan"), false), None);
    }
}
