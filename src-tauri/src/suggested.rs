//! The Automations page's Suggested tiles: the nightly review's learned
//! automations, then the cabin's built-in ones, minus those already running.
//! Add saves the tile's seed as a row and takes a learned tile off the list.

use std::path::Path;
use std::sync::Mutex;

use grokhub_core::review_pass::{
    automation_from_seed, load_suggestions, merge_suggested_autos, read_suggestions,
    save_suggestions, seed_running, SuggestedTile,
};
use grokhub_core::{Automation, LocalClock, SuggestionStore};

/// One read-modify-write of `suggestions.json` at a time.
static STORE: Mutex<()> = Mutex::new(());

/// Change `suggestions.json` under its lock. A file that can't be read is
/// left alone, and an edit that changes nothing writes nothing.
pub fn with_suggestions<T>(
    config_dir: &Path,
    edit: impl FnOnce(&mut SuggestionStore) -> T,
) -> Result<T, String> {
    let _one = STORE.lock().unwrap_or_else(|e| e.into_inner());
    let before = read_suggestions(config_dir)?;
    let mut store = before.clone();
    let out = edit(&mut store);
    if store != before {
        save_suggestions(config_dir, &store)?;
    }
    Ok(out)
}

/// The tiles to show now.
pub fn suggested(config_dir: &Path) -> Vec<SuggestedTile> {
    let running = crate::automations::load(config_dir).unwrap_or_default();
    let learned = {
        let _one = STORE.lock().unwrap_or_else(|e| e.into_inner());
        load_suggestions(config_dir).autos
    };
    merge_suggested_autos(&learned, &running)
}

/// Take a learned tile with this seed off the list. The job is what
/// matters, so a file that can't be written only leaves a tile that hides
/// anyway while its job runs.
fn dismiss(config_dir: &Path, seed: &str) {
    if let Err(e) = with_suggestions(config_dir, |s| {
        grokhub_core::dismiss_accepted_auto(s, seed, "")
    }) {
        eprintln!("suggested: {e}");
    }
}

/// Add a tile: its seed becomes an automation, and a learned tile with that
/// seed leaves the list.
pub fn add_suggested(
    config_dir: &Path,
    seed: &str,
    clock: LocalClock,
) -> Result<Vec<Automation>, String> {
    let a = automation_from_seed(seed)
        .ok_or_else(|| "That suggestion has no schedule to save.".to_string())?;
    let running = crate::automations::load(config_dir)?;
    if seed_running(seed, &running) {
        dismiss(config_dir, seed);
        return Err(format!("{} is already an automation.", a.name.trim()));
    }
    let list = crate::automations::add_parsed(config_dir, a, clock)?;
    dismiss(config_dir, seed);
    Ok(list)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!(
            "shell-suggested-{name}-{}-{}",
            std::process::id(),
            grokhub_core::now_ms()
        ));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// 2026-10-07, a Wednesday, at 20:00.
    fn clock() -> LocalClock {
        LocalClock {
            now_ms: 1_791_403_200_000,
            weekday: 3,
            hour: 20,
            minute: 0,
        }
    }

    #[test]
    fn adding_a_loop_tile_makes_a_heartbeat_row_and_hides_the_tile() {
        let d = dir("loop");
        let titles =
            |d: &Path| -> Vec<String> { suggested(d).into_iter().map(|t| t.title).collect() };
        assert_eq!(
            titles(&d).len(),
            6,
            "Dawn snapshot shares Host heartbeat's prompt"
        );
        assert!(titles(&d).contains(&"Midday board".to_string()));

        let list = add_suggested(&d, "/loop 12h summarize the workboard", clock()).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(
            (
                list[0].name.as_str(),
                list[0].schedule.as_str(),
                list[0].heartbeat_every_min
            ),
            ("summarize the workboard", "heartbeat", 720)
        );
        assert!(list[0].next_run.is_some(), "it is scheduled");
        let saved = crate::automations::load(&d).unwrap();
        assert_eq!(saved, list);
        assert!(!titles(&d).contains(&"Midday board".to_string()));
        assert_eq!(titles(&d).len(), 5);

        assert_eq!(
            add_suggested(&d, "/loop 12h summarize the workboard", clock()),
            Err("summarize the workboard is already an automation.".into())
        );
        assert_eq!(crate::automations::load(&d).unwrap().len(), 1);
        assert_eq!(
            add_suggested(&d, "tidy the board", clock()),
            Err("That suggestion has no schedule to save.".into())
        );
    }

    #[test]
    fn a_learned_tile_shows_first_and_leaves_once_added() {
        let d = dir("learned");
        std::fs::write(
            d.join(grokhub_core::review_pass::SUGGESTIONS_FILE),
            r#"{"last_review_day":"2026-10-06","autos":[{"kind":"auto","title":"Inbox sweep","body":"Every day at 8.","seed":"every day at 8, sweep the inbox"}]}"#,
        )
        .unwrap();
        let tiles = suggested(&d);
        assert_eq!(tiles[0].title, "Inbox sweep");
        assert_eq!(tiles[0].body, "Every day at 8.");

        let list = add_suggested(&d, "every day at 8, sweep the inbox", clock()).unwrap();
        assert_eq!(
            (list[0].schedule.as_str(), list[0].time.as_str()),
            ("daily", "08:00")
        );
        let store = load_suggestions(&d);
        assert!(store.autos.is_empty(), "{store:?}");
        assert_eq!(store.last_review_day.as_deref(), Some("2026-10-06"));
        assert!(!suggested(&d).iter().any(|t| t.title == "Inbox sweep"));
    }

    #[test]
    fn a_suggestions_file_that_cannot_be_read_is_never_replaced() {
        let d = dir("broken");
        let path = d.join(grokhub_core::review_pass::SUGGESTIONS_FILE);
        let broken = r#"{"autos":[{"kind":"widget","title":"Inbox sweep","body":"b"}]}"#;
        std::fs::write(&path, broken).unwrap();
        // The page still lists the built-in tiles, and Add still saves.
        assert_eq!(suggested(&d).len(), 6);
        let list = add_suggested(&d, "/loop 12h summarize the workboard", clock()).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), broken);
        // Adding a built-in tile writes no file when there was none.
        let fresh = dir("none");
        add_suggested(&fresh, "/loop 1d replay the last desktop task", clock()).unwrap();
        assert!(!fresh
            .join(grokhub_core::review_pass::SUGGESTIONS_FILE)
            .exists());
    }

    #[test]
    fn a_change_to_a_broken_suggestions_file_is_refused() {
        let d = dir("refused");
        let path = d.join(grokhub_core::review_pass::SUGGESTIONS_FILE);
        std::fs::write(&path, "{ not json").unwrap();
        let said = with_suggestions(&d, |s| s.last_review_day = Some("2026-10-07".into()));
        assert!(said.unwrap_err().starts_with("suggestions.json: "));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{ not json");
        std::fs::remove_file(&path).unwrap();
        with_suggestions(&d, |s| s.last_review_day = Some("2026-10-07".into())).unwrap();
        assert_eq!(
            load_suggestions(&d).last_review_day.as_deref(),
            Some("2026-10-07")
        );
    }

    #[test]
    fn a_suggestions_file_that_cannot_be_written_still_saves_the_job() {
        let d = dir("unwritable");
        let learned = r#"{"autos":[{"kind":"auto","title":"Inbox sweep","body":"b","seed":"every day at 8, sweep the inbox"}]}"#;
        std::fs::write(d.join(grokhub_core::review_pass::SUGGESTIONS_FILE), learned).unwrap();
        // The temp file the save writes through is a folder, so the save fails.
        std::fs::create_dir_all(
            d.join(format!("suggestions.{}.tmp", std::process::id()))
                .join("block"),
        )
        .unwrap();
        let list = add_suggested(&d, "every day at 8, sweep the inbox", clock()).unwrap();
        assert_eq!(list[0].name, "sweep the inbox");
        assert_eq!(crate::automations::load(&d).unwrap().len(), 1);
        assert_eq!(
            std::fs::read_to_string(d.join(grokhub_core::review_pass::SUGGESTIONS_FILE)).unwrap(),
            learned,
            "the save failed"
        );
        // The tile hides anyway: its job runs.
        assert!(!suggested(&d).iter().any(|t| t.title == "Inbox sweep"));
    }

    #[test]
    fn a_tile_whose_job_runs_under_other_words_is_hidden_and_refused() {
        let d = dir("edited");
        add_suggested(&d, "/loop 12h summarize the workboard", clock()).unwrap();
        let id = crate::automations::load(&d).unwrap()[0].id.clone();
        // Edit, then Save the words the box shows.
        crate::automations::edit(
            &d,
            &id,
            "heartbeat every 720 min summarize the workboard",
            clock(),
        )
        .unwrap();
        assert_eq!(
            crate::automations::load(&d).unwrap()[0].name,
            "720 min summarize the workboard"
        );
        assert!(!suggested(&d).iter().any(|t| t.title == "Midday board"));
        assert_eq!(
            add_suggested(&d, "/loop 12h summarize the workboard", clock()),
            Err("summarize the workboard is already an automation.".into())
        );
        assert_eq!(crate::automations::load(&d).unwrap().len(), 1);
    }
}
