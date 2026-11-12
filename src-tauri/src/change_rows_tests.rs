use super::*;
use grokhub_agent::harness::{self as hx, Origin};
use std::path::PathBuf;

const V1: &str = "---\nname: weekly-report\n---\n\n# weekly-report\n\n## Steps\nopen the sheet\n";
const V2: &str =
    "---\nname: weekly-report\n---\n\n# weekly-report\n\n## Steps\nopen the sheet, then mail it\n";

struct Dir(PathBuf);

impl std::ops::Deref for Dir {
    type Target = Path;
    fn deref(&self) -> &Path {
        &self.0
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn dir(label: &str) -> Dir {
    let root = std::env::temp_dir().join(format!(
        "grokhub-change-rows-{label}-{}-{}",
        std::process::id(),
        grokhub_core::now_ms()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("skills")).expect("skills dir");
    Dir(root)
}

fn skill_text(root: &Path) -> Option<String> {
    std::fs::read_to_string(root.join("skills/weekly-report/SKILL.md")).ok()
}

fn record(root: &Path, body: &str) {
    hx::record_skill_change(
        root,
        &root.join("skills"),
        "weekly-report",
        Origin::SelfManage,
        "tightened the steps",
        || {
            let folder = root.join("skills/weekly-report");
            std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
            std::fs::write(folder.join("SKILL.md"), body).map_err(|e| e.to_string())
        },
    )
    .expect("record");
}

#[test]
fn nothing_changed_shows_no_groups() {
    let root = dir("empty");
    let v = view(&root, 1_000);
    assert_eq!(
        v.heading,
        format!(
            "What {} changed on its own",
            grokhub_core::brand::product_name()
        )
    );
    assert_eq!(v.groups, Vec::new());
    assert!(v.empty.starts_with("Nothing to undo. When "), "{}", v.empty);
}

#[test]
fn a_self_made_skill_shows_undo_and_keep() {
    let root = dir("row");
    record(&root, V1);
    let v = view(&root, grokhub_core::now_ms());
    assert_eq!(v.groups.len(), 1);
    assert_eq!(v.groups[0].kind, "skill");
    assert_eq!(v.groups[0].title, "Skills");
    let row = &v.groups[0].rows[0];
    assert_eq!(row.id, "weekly-report");
    assert!(
        row.label.starts_with("weekly-report · added "),
        "{}",
        row.label
    );
    assert_eq!(row.act, changes::Act::Undo);
    assert!(row.keep);
}

#[test]
fn undo_then_restore_round_trips_through_the_ledger() {
    let root = dir("round");
    record(&root, V1);
    assert_eq!(
        undo(&root, "skill", "weekly-report").expect("undo"),
        "Removed weekly-report: it was new. Its text is kept, and /skills restore weekly-report brings it back."
    );
    assert_eq!(skill_text(&root), None);
    let v = view(&root, grokhub_core::now_ms());
    let row = &v.groups[0].rows[0];
    assert_eq!(row.act, changes::Act::Restore);
    assert!(!row.keep);
    assert_eq!(
        restore(&root, "skill", "weekly-report").expect("restore"),
        "Restored weekly-report from its kept copy."
    );
    assert_eq!(skill_text(&root).as_deref(), Some(V1));
}

#[test]
fn undo_puts_back_the_version_before() {
    let root = dir("modify");
    record(&root, V1);
    record(&root, V2);
    assert_eq!(
        undo(&root, "skill", "weekly-report").expect("undo"),
        "Undid the newest change to weekly-report. The version before it is back."
    );
    assert_eq!(skill_text(&root).as_deref(), Some(V1));
}

#[test]
fn keep_takes_the_keep_pill_away_and_leaves_the_file() {
    let root = dir("keep");
    record(&root, V1);
    assert_eq!(
        keep(&root, "skill", "weekly-report").expect("keep"),
        "Kept the skill weekly-report."
    );
    assert_eq!(skill_text(&root).as_deref(), Some(V1));
    let v = view(&root, grokhub_core::now_ms());
    assert!(!v.groups[0].rows[0].keep);
}

#[test]
fn other_kinds_are_refused() {
    let root = dir("refuse");
    assert_eq!(
        undo(&root, "model", "table").unwrap_err(),
        "No change rows for model."
    );
    assert_eq!(
        restore(&root, "automation", "x").unwrap_err(),
        "Only a removed skill can be restored."
    );
}
