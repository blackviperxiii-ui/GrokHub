//! The Suggested automations tiles on the Automations page, moved out of the
//! cabin: the nightly review's learned automations first, then the built-in
//! seeds, minus any already running. Adding one turns its seed into a row.
//! The review's state lives in `suggestions.json`, the same file the cabin
//! wrote.

use std::path::Path;

use serde::Serialize;

use crate::automation::{route_schedule, AutoHealth, Automation, ScheduleRoute};
use crate::grok_loop::{loop_interval_ms, parse_loop_line};
use crate::review::{LearnedSuggestion, SuggestionKind, SuggestionStore};

/// The review's state, in the config folder.
pub const SUGGESTIONS_FILE: &str = "suggestions.json";

/// A bigger file is not a suggestions list.
const SUGGESTIONS_CAP: u64 = 1024 * 1024;

/// The longest heartbeat a row takes, as the automation scheduler caps it.
const HEARTBEAT_MAX_MIN: u64 = 24 * 60;

/// One built-in tile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SuggestedAuto {
    pub title: &'static str,
    pub body: &'static str,
    pub seed: &'static str,
}

/// The cabin's built-in tiles, as it showed them.
pub const SUGGESTED_AUTOS: [SuggestedAuto; 7] = [
    SuggestedAuto {
        title: "Morning brief",
        body: "Every 1d — workboard and last host receipt.",
        seed: "/loop 1d summarize the workboard and last host receipt",
    },
    SuggestedAuto {
        title: "Host heartbeat",
        body: "Every 60m — read-only snapshot, then a short note.",
        seed: "/loop 60m run a read-only host snapshot and summarize",
    },
    SuggestedAuto {
        title: "Task extractor",
        body: "Every 1d — pull today's open tasks onto the board.",
        seed: "/loop 1d extract open tasks onto the workboard",
    },
    SuggestedAuto {
        title: "Dawn snapshot",
        body: "Every 1d — read-only host snapshot.",
        seed: "/loop 1d run a read-only host snapshot and summarize",
    },
    SuggestedAuto {
        title: "Midday board",
        body: "Every 12h — summarize the workboard.",
        seed: "/loop 12h summarize the workboard",
    },
    SuggestedAuto {
        title: "Nightly triage",
        body: "Every 1d — extract leftover tasks onto the board.",
        seed: "/loop 1d extract leftover tasks onto the workboard",
    },
    SuggestedAuto {
        title: "Replay last desktop run",
        body: "Every 1d — repeat the last desktop task.",
        seed: "/loop 1d replay the last desktop task",
    },
];

/// One tile the page shows. `seed` is what Add saves.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SuggestedTile {
    pub title: String,
    pub body: String,
    pub seed: String,
}

/// Words a heartbeat row's text may put between "heartbeat every N" and
/// its prompt.
const UNITS: [&str; 10] = [
    "m", "min", "mins", "minute", "minutes", "h", "hr", "hrs", "hour", "hours",
];

/// "heartbeat every 720 min summarize the workboard" → "summarize the
/// workboard": a heartbeat row's prompt however its words were saved.
fn heartbeat_prompt(text: &str) -> Option<String> {
    let lower = text.trim().to_ascii_lowercase();
    let rest = lower.strip_prefix("heartbeat every ")?;
    let mut words = rest.split_whitespace().peekable();
    let count = words.next()?;
    if !count.starts_with(|c: char| c.is_ascii_digit()) {
        return None;
    }
    if count.chars().all(|c| c.is_ascii_digit()) && words.peek().is_some_and(|w| UNITS.contains(w))
    {
        words.next();
    }
    let prompt = words.collect::<Vec<_>>().join(" ");
    (!prompt.is_empty()).then_some(prompt)
}

/// What a running job is known by: its name, its instructions and, for a
/// heartbeat, its prompt without the schedule words.
fn job_keys(a: &Automation) -> Vec<String> {
    let mut keys = vec![
        a.name.trim().to_ascii_lowercase(),
        a.instructions.trim().to_ascii_lowercase(),
    ];
    keys.extend(heartbeat_prompt(&a.instructions));
    keys.extend(heartbeat_prompt(&format!(
        "heartbeat every {}",
        a.name.trim()
    )));
    keys.retain(|k| !k.is_empty());
    keys
}

/// What a tile is known by: its title, its seed, a loop's prompt and the
/// job its seed would save.
fn tile_keys(title: &str, seed: &str) -> Vec<String> {
    let mut keys = vec![title.trim().to_ascii_lowercase()];
    let seed = seed.trim();
    if !seed.is_empty() {
        keys.push(seed.to_ascii_lowercase());
        if let Some((_, prompt)) = parse_loop_line(seed) {
            keys.push(prompt.to_ascii_lowercase());
        }
        keys.extend(automation_from_seed(seed).iter().flat_map(job_keys));
    }
    keys.retain(|k| !k.is_empty());
    keys
}

fn running_keys(running: &[Automation]) -> Vec<String> {
    running.iter().flat_map(job_keys).collect()
}

/// Whether the job a tile's seed would save is already running.
pub fn seed_running(seed: &str, running: &[Automation]) -> bool {
    let known = running_keys(running);
    tile_keys("", seed).iter().any(|k| known.contains(k))
}

/// Learned automations first, then the built-in tiles, leaving out any
/// already running or already shown, by title, seed or prompt.
pub fn merge_suggested_autos(
    learned: &[LearnedSuggestion],
    running: &[Automation],
) -> Vec<SuggestedTile> {
    let mut seen = running_keys(running);
    let mut out = Vec::new();
    let learned = learned
        .iter()
        .filter(|s| s.kind == SuggestionKind::Auto)
        .map(|s| {
            (
                s.title.as_str(),
                s.body.as_str(),
                s.seed.as_deref().unwrap_or(""),
            )
        });
    let built_in = SUGGESTED_AUTOS.iter().map(|s| (s.title, s.body, s.seed));
    for (title, body, seed) in learned.chain(built_in) {
        let keys = tile_keys(title, seed);
        if keys.iter().any(|k| seen.contains(k)) {
            continue;
        }
        seen.extend(keys);
        out.push(SuggestedTile {
            title: title.into(),
            body: body.into(),
            seed: seed.into(),
        });
    }
    out
}

/// The row a tile's seed makes: a clock job for "every day at 21, …", a
/// heartbeat for a `/loop` interval. `None` when the seed names no schedule.
pub fn automation_from_seed(seed: &str) -> Option<Automation> {
    match route_schedule(seed)? {
        ScheduleRoute::Clock(a) => Some(*a),
        ScheduleRoute::Interval { interval, prompt } => {
            let prompt = prompt.trim().to_string();
            if prompt.is_empty() {
                return None;
            }
            let mins = (loop_interval_ms(&interval)? / 60_000).clamp(1, HEARTBEAT_MAX_MIN);
            Some(Automation {
                id: String::new(),
                name: prompt.clone(),
                schedule: "heartbeat".into(),
                time: "00:00".into(),
                times: vec![],
                instructions: prompt,
                heartbeat_every_min: mins as u32,
                check_command: String::new(),
                enabled: true,
                last_run: None,
                next_run: None,
                run_count: 0,
                health: AutoHealth::default(),
            })
        }
    }
}

/// `suggestions.json`; missing, too big or broken reads as empty.
pub fn load_suggestions(config_dir: &Path) -> SuggestionStore {
    read_suggestions(config_dir).unwrap_or_default()
}

/// `suggestions.json` for a change: missing is empty, but a file too big,
/// unreadable or broken is an error, so a save never replaces it.
pub fn read_suggestions(config_dir: &Path) -> Result<SuggestionStore, String> {
    let path = config_dir.join(SUGGESTIONS_FILE);
    let len = match std::fs::metadata(&path) {
        Ok(m) => m.len(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(SuggestionStore::default()),
        Err(e) => return Err(format!("{SUGGESTIONS_FILE}: {e}")),
    };
    if len > SUGGESTIONS_CAP {
        return Err(format!(
            "{SUGGESTIONS_FILE} is too large to be a suggestions list."
        ));
    }
    let raw = std::fs::read_to_string(&path).map_err(|e| format!("{SUGGESTIONS_FILE}: {e}"))?;
    let raw = raw.trim_start_matches('\u{feff}');
    if raw.trim().is_empty() {
        return Ok(SuggestionStore::default());
    }
    serde_json::from_str(raw).map_err(|e| format!("{SUGGESTIONS_FILE}: {e}"))
}

/// Write `suggestions.json` through a temp file, as the cabin did.
pub fn save_suggestions(config_dir: &Path, store: &SuggestionStore) -> Result<(), String> {
    let text = serde_json::to_string_pretty(store).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(config_dir).map_err(|e| e.to_string())?;
    crate::pulse_writer::write_atomic(&config_dir.join(SUGGESTIONS_FILE), &text)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn learned(title: &str, seed: &str) -> LearnedSuggestion {
        LearnedSuggestion {
            kind: SuggestionKind::Auto,
            title: title.into(),
            body: "b".into(),
            seed: Some(seed.into()),
            name: None,
            trigger: None,
            instructions: None,
            provider: None,
            tool: None,
        }
    }

    #[test]
    fn every_built_in_seed_makes_a_row() {
        assert_eq!(SUGGESTED_AUTOS.len(), 7);
        for tile in SUGGESTED_AUTOS {
            assert!(automation_from_seed(tile.seed).is_some(), "{}", tile.seed);
        }
        let midday = automation_from_seed("/loop 12h summarize the workboard").unwrap();
        assert_eq!(
            (
                midday.schedule.as_str(),
                midday.heartbeat_every_min,
                midday.name.as_str(),
                midday.instructions.as_str()
            ),
            (
                "heartbeat",
                720,
                "summarize the workboard",
                "summarize the workboard"
            )
        );
        assert!(midday.enabled);
        let replay = automation_from_seed("/loop 1d replay the last desktop task").unwrap();
        assert_eq!(replay.heartbeat_every_min, 1440);
        let hourly = automation_from_seed("/loop 60m run a read-only host snapshot and summarize");
        assert_eq!(hourly.unwrap().heartbeat_every_min, 60);
        let nightly = automation_from_seed("every day at 21, tidy the board").unwrap();
        assert_eq!(
            (nightly.schedule.as_str(), nightly.time.as_str()),
            ("daily", "21:00")
        );
        assert_eq!(automation_from_seed("tidy the board"), None);
    }

    #[test]
    fn learned_tiles_come_first_and_running_ones_drop() {
        let tiles = merge_suggested_autos(
            &[learned("Inbox sweep", "/loop 1d sweep the inbox")],
            &[automation_from_seed("/loop 12h summarize the workboard").unwrap()],
        );
        let titles: Vec<&str> = tiles.iter().map(|t| t.title.as_str()).collect();
        assert_eq!(
            titles,
            [
                "Inbox sweep",
                "Morning brief",
                "Host heartbeat",
                "Task extractor",
                "Nightly triage",
                "Replay last desktop run"
            ],
            "Midday board runs already, and Dawn snapshot has Host heartbeat's prompt"
        );
        assert_eq!(tiles[0].seed, "/loop 1d sweep the inbox");
        // A learned skill is not a tile, and a learned tile named like a
        // built-in one replaces it.
        let mut skill = learned("Release notes", "");
        skill.kind = SuggestionKind::Skill;
        let tiles = merge_suggested_autos(
            &[
                skill,
                learned("Midday board", "/loop 6h summarize the workboard"),
            ],
            &[],
        );
        assert_eq!(tiles[0].title, "Midday board");
        assert_eq!(tiles[0].seed, "/loop 6h summarize the workboard");
        assert_eq!(
            tiles.iter().filter(|t| t.title == "Midday board").count(),
            1
        );
        assert!(!tiles.iter().any(|t| t.title == "Release notes"));
    }

    #[test]
    fn a_cabin_suggestions_file_still_loads_and_saves() {
        let dir = std::env::temp_dir().join(format!(
            "grokhub-review-pass-{}-{}",
            std::process::id(),
            crate::now_ms()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(load_suggestions(&dir), SuggestionStore::default());
        std::fs::write(
            dir.join(SUGGESTIONS_FILE),
            r#"{"last_review_day":"2026-10-07","last_review_ms":5,"autos":[{"kind":"auto","title":"Morning brief","body":"b","seed":"/loop 1d x"}]}"#,
        )
        .unwrap();
        let store = load_suggestions(&dir);
        assert_eq!(store.last_review_day.as_deref(), Some("2026-10-07"));
        assert_eq!(store.autos[0].title, "Morning brief");
        save_suggestions(&dir, &store).unwrap();
        let text = std::fs::read_to_string(dir.join(SUGGESTIONS_FILE)).unwrap();
        assert!(
            text.contains("\"last_review_day\": \"2026-10-07\""),
            "{text}"
        );
        assert_eq!(load_suggestions(&dir), store);
        std::fs::write(dir.join(SUGGESTIONS_FILE), "{ not json").unwrap();
        assert_eq!(load_suggestions(&dir), SuggestionStore::default());
        assert!(read_suggestions(&dir)
            .unwrap_err()
            .starts_with("suggestions.json: "));
        std::fs::write(
            dir.join(SUGGESTIONS_FILE),
            r#"{"autos":[{"kind":"widget","title":"t","body":"b"}]}"#,
        )
        .unwrap();
        assert!(
            read_suggestions(&dir).is_err(),
            "an unknown kind is not empty"
        );
        std::fs::write(
            dir.join(SUGGESTIONS_FILE),
            "\u{feff}{\"last_review_day\":\"2026-10-08\"}",
        )
        .unwrap();
        assert_eq!(
            read_suggestions(&dir).unwrap().last_review_day.as_deref(),
            Some("2026-10-08"),
            "a byte-order mark from a hand edit still reads"
        );
        std::fs::write(
            dir.join(SUGGESTIONS_FILE),
            " ".repeat(SUGGESTIONS_CAP as usize + 1),
        )
        .unwrap();
        assert_eq!(
            read_suggestions(&dir),
            Err("suggestions.json is too large to be a suggestions list.".into())
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_job_is_known_however_its_schedule_words_were_saved() {
        // A Midday board row after Edit, saved in the heartbeat wording.
        let edited =
            crate::parse_nl_automation("heartbeat every 720 min summarize the workboard").unwrap();
        assert_eq!(edited.name, "720 min summarize the workboard");
        assert!(seed_running(
            "/loop 12h summarize the workboard",
            std::slice::from_ref(&edited)
        ));
        let tiles = merge_suggested_autos(&[], &[edited]);
        assert!(!tiles.iter().any(|t| t.title == "Midday board"));
        assert_eq!(tiles.len(), 5);

        // A learned clock tile whose job runs already as a heartbeat.
        let midday = automation_from_seed("/loop 12h summarize the workboard").unwrap();
        assert!(seed_running(
            "every weekday at 9, summarize the workboard",
            std::slice::from_ref(&midday)
        ));
        let tiles = merge_suggested_autos(
            &[learned(
                "Weekday board",
                "every weekday at 9, summarize the workboard",
            )],
            &[midday],
        );
        assert!(!tiles.iter().any(|t| t.title == "Weekday board"));

        // A different prompt is a different job.
        let inbox = automation_from_seed("every day at 8, sweep the inbox").unwrap();
        assert!(!seed_running("/loop 12h summarize the workboard", &[inbox]));
        assert_eq!(
            heartbeat_prompt("heartbeat every 30m check the build").as_deref(),
            Some("check the build")
        );
        assert_eq!(heartbeat_prompt("heartbeat every day"), None);
    }
}
