//! The Suggested automations tiles on the Automations page, moved out of the
//! cabin: the nightly review's learned automations first, then the built-in
//! seeds, minus any already running. Adding one turns its seed into a row.
//! The review's state lives in `suggestions.json`, the same file the cabin
//! wrote. The nightly review's reply is read here too: what it suggests,
//! kept only when it answers work the user repeats.

use std::path::Path;

use serde::Serialize;

use crate::automation::{route_schedule, AutoHealth, Automation, ScheduleRoute};
use crate::grok_loop::{loop_interval_ms, parse_loop_line};
use crate::ideas::{idea_has_reason, same_topic, IdeaGround, IdeaKind};
use crate::organs::LocalClock;
use crate::review::{
    dedupe_suggestions, drop_echoed_suggestions, merge_suggestion_store, parse_suggest_lines,
    partition_suggestions, prune_live_suggestions, review_due, suggestions_from_sessions,
    DigestLine, LearnedSuggestion, SuggestionKind, SuggestionStore, CABIN_GITHUB_TOOLS,
};
use crate::update_feed::{turned_down_topic, FeedPulse};

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
                gate: Default::default(),
                wait: Default::default(),
                enabled: true,
                last_run: None,
                next_run: None,
                run_count: 0,
                health: AutoHealth::default(),
                lifecycle: Default::default(),
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

/// What they asked lately, newest first: real asks only (no slash commands,
/// tool output or "no, still…" feedback), each once, first line only.
/// `lines` is `(role, text)`, newest first.
pub fn recent_asks_from<'a>(
    lines: impl IntoIterator<Item = (&'a str, &'a str)>,
    n: usize,
) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for (role, text) in lines {
        if out.len() >= n {
            break;
        }
        if role != "user" {
            continue;
        }
        let t = text.trim();
        if t.len() < 8
            || t.starts_with('/')
            || crate::is_workload_user(t)
            || crate::is_feedback_ask(t)
            || !crate::is_plain_text(t)
        {
            continue;
        }
        let line: String = t.lines().next().unwrap_or("").chars().take(200).collect();
        if !out.iter().any(|o| o.eq_ignore_ascii_case(&line)) {
            out.push(line);
        }
    }
    out
}

/// What they said, for dropping a suggestion that only quotes them: the
/// first 40 of their lines, 280 characters each.
pub fn said_lines<'a>(lines: impl IntoIterator<Item = (&'a str, &'a str)>) -> Vec<String> {
    let mut out = Vec::new();
    for (role, text) in lines {
        if out.len() >= 40 {
            break;
        }
        if role != "user" {
            continue;
        }
        let line: String = text.chars().take(280).collect();
        if !line.trim().is_empty() {
            out.push(line);
        }
    }
    out
}

/// Lasting context a suggestion may stand on: memory lines of 12 or more
/// characters (bullets and headings stripped), then the open board cards.
pub fn lasting_lines(user_md: &str, memory_md: &str, open_cards: &[String]) -> Vec<String> {
    let mut out: Vec<String> = user_md
        .lines()
        .chain(memory_md.lines())
        .map(|l| l.trim().trim_start_matches(['-', '*', '#', ' ']).trim())
        .filter(|l| l.len() >= 12)
        .map(str::to_string)
        .collect();
    out.extend(open_cards.iter().cloned());
    out
}

/// Keep a skill or automation only when it answers work they repeat or
/// lasting context, one per topic, and never one they turned down.
pub fn keep_reasoned_suggestions(
    items: &mut Vec<LearnedSuggestion>,
    ground: &IdeaGround,
    pulse: &FeedPulse,
) {
    let mut kept: Vec<String> = Vec::new();
    items.retain(|item| {
        let kind = match item.kind {
            SuggestionKind::Skill => IdeaKind::Skill,
            SuggestionKind::Auto => IdeaKind::Automation,
            SuggestionKind::Connector => return true,
        };
        let topic = format!(
            "{} {} {} {}",
            item.title,
            item.body,
            item.seed.as_deref().unwrap_or(""),
            item.trigger.as_deref().unwrap_or("")
        );
        if !idea_has_reason(kind, &topic, ground)
            || kept.iter().any(|k| same_topic(k, &topic))
            || turned_down_topic(pulse, &item.title)
            || item
                .name
                .as_deref()
                .is_some_and(|n| turned_down_topic(pulse, n))
        {
            return false;
        }
        kept.push(topic);
        true
    });
}

/// The quiet daily pass over recent chat, with no model call: a need asked
/// on two or more occasions becomes a Suggested tile. Once a local day, at
/// or after `night_hour` the first time. Returns whether it ran.
pub fn session_pass(
    store: &mut SuggestionStore,
    lines: &[DigestLine],
    skills: &[String],
    autos: &[String],
    today: &str,
    clock: &LocalClock,
    night_hour: u32,
) -> bool {
    if !review_due(
        store.last_session_suggest_day.as_deref(),
        today,
        clock,
        night_hour,
    ) {
        return false;
    }
    let items = suggestions_from_sessions(lines, skills, autos);
    store.last_session_suggest_day = Some(today.to_string());
    if !items.is_empty() {
        *store = merge_suggestion_store(store, partition_suggestions(items));
    }
    true
}

/// What a review reply is checked against.
#[derive(Debug, Clone, Copy)]
pub struct ReplyGround<'a> {
    pub skills: &'a [String],
    pub automations: &'a [String],
    /// [`said_lines`].
    pub said: &'a [String],
    pub ideas: IdeaGround<'a>,
    /// What they turned down.
    pub pulse: &'a FeedPulse,
}

/// Fold tonight's review reply into the store and stamp the day. A failed
/// call stamps the day too, so the review waits for tomorrow. Returns how
/// many suggestions were kept. Skill patches are applied apart.
pub fn apply_reply(
    store: &mut SuggestionStore,
    reply: Result<&str, &str>,
    today: &str,
    now_ms: u64,
    ground: &ReplyGround,
) -> usize {
    let day = Some(today.to_string());
    let Ok(text) = reply else {
        store.last_review_day = day;
        store.last_review_ms = now_ms;
        return 0;
    };
    // Every allowlisted GitHub tool counts as wired, as in the cabin: the
    // desktop app shows no connector tiles.
    let live: Vec<String> = CABIN_GITHUB_TOOLS.iter().map(|t| t.to_string()).collect();
    let mut items = dedupe_suggestions(
        drop_echoed_suggestions(parse_suggest_lines(text), ground.said),
        ground.skills,
        ground.automations,
        &live,
    );
    keep_reasoned_suggestions(&mut items, &ground.ideas, ground.pulse);
    let kept = items.len();
    if items.is_empty() {
        store.last_review_day = day;
        store.last_review_ms = now_ms;
    } else {
        let mut incoming = partition_suggestions(items);
        incoming.last_review_day = day;
        incoming.last_review_ms = now_ms;
        *store = merge_suggestion_store(store, incoming);
    }
    prune_live_suggestions(store, &live);
    kept
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

    const TIDY_REPLY: &str = "Here is what I found.\n\
        SUGGEST_SKILL: tidy-board | Tidy the board | Sort open cards | when the board is full | sort cards by due date\n\
        SUGGEST_AUTO: Garden log | Water the plants | every day at 7, water the tomato plants\n";

    fn tidy_asks() -> Vec<String> {
        vec![
            "Can you tidy the board cards for me".to_string(),
            "Please tidy the board cards again today".to_string(),
        ]
    }

    #[test]
    fn recent_asks_skip_slash_short_and_repeats() {
        let lines = [
            ("user", "/help"),
            ("user", "ok"),
            ("assistant", "Here is the workboard summary you asked for."),
            ("user", "Summarize the workboard please\nand list due cards"),
            ("user", "summarize the workboard please"),
        ];
        assert_eq!(
            recent_asks_from(lines, 30),
            vec!["Summarize the workboard please".to_string()]
        );
        let many = [
            ("user", "first long ask here"),
            ("user", "second long ask here"),
        ];
        assert_eq!(
            recent_asks_from(many, 1),
            vec!["first long ask here".to_string()]
        );
    }

    #[test]
    fn said_lines_keep_forty_user_lines_cut_to_280() {
        let long = "x".repeat(300);
        let mut lines = vec![
            ("assistant", "a reply"),
            ("user", "  "),
            ("user", long.as_str()),
        ];
        lines.extend(std::iter::repeat_n(("user", "again"), 50));
        let said = said_lines(lines);
        assert_eq!(said.len(), 40);
        assert_eq!(said[0].len(), 280);
        assert_eq!(said[1], "again");
    }

    #[test]
    fn lasting_lines_strip_bullets_and_drop_short_ones() {
        let lasting = lasting_lines(
            "# About\n- Works on the garden plans weekly\n- short\n",
            "* Keeps a reading list in notes\n",
            &["Ship the beta build".to_string()],
        );
        assert_eq!(
            lasting,
            vec![
                "Works on the garden plans weekly".to_string(),
                "Keeps a reading list in notes".to_string(),
                "Ship the beta build".to_string(),
            ]
        );
    }

    #[test]
    fn a_reply_keeps_only_what_their_asks_back() {
        let asks = tidy_asks();
        let pulse = FeedPulse::default();
        let ground = ReplyGround {
            skills: &[],
            automations: &[],
            said: &[],
            ideas: IdeaGround {
                asks: &asks,
                lasting: &[],
            },
            pulse: &pulse,
        };
        let mut store = SuggestionStore {
            reviewed_turns: 8,
            ..SuggestionStore::default()
        };
        let kept = apply_reply(&mut store, Ok(TIDY_REPLY), "2026-10-07", 9, &ground);
        assert_eq!(kept, 1, "the garden job has no ask behind it");
        assert_eq!(store.skills.len(), 1);
        assert_eq!(store.skills[0].name.as_deref(), Some("tidy-board"));
        assert_eq!(
            store.skills[0].instructions.as_deref(),
            Some("sort cards by due date")
        );
        assert!(store.autos.is_empty());
        assert_eq!(store.last_review_day.as_deref(), Some("2026-10-07"));
        assert_eq!(store.last_review_ms, 9);
        assert_eq!(store.reviewed_turns, 8);

        // A skill they already have, or turned down, is not kept.
        let have = vec!["tidy-board".to_string()];
        let mut store = SuggestionStore::default();
        let kept = apply_reply(
            &mut store,
            Ok(TIDY_REPLY),
            "2026-10-07",
            9,
            &ReplyGround {
                skills: &have,
                ..ground
            },
        );
        assert_eq!((kept, store.skills.len()), (0, 0));
        let no = FeedPulse {
            turned_down: vec!["Tidy the board".into()],
            ..FeedPulse::default()
        };
        let mut store = SuggestionStore::default();
        let kept = apply_reply(
            &mut store,
            Ok(TIDY_REPLY),
            "2026-10-07",
            9,
            &ReplyGround {
                pulse: &no,
                ..ground
            },
        );
        assert_eq!((kept, store.skills.len()), (0, 0));
        assert_eq!(store.last_review_day.as_deref(), Some("2026-10-07"));
    }

    #[test]
    fn a_failed_reply_stamps_the_day_and_keeps_the_tiles() {
        let pulse = FeedPulse::default();
        let mut store = SuggestionStore {
            autos: vec![learned("Inbox sweep", "every day at 8, sweep the inbox")],
            ..SuggestionStore::default()
        };
        let ground = ReplyGround {
            skills: &[],
            automations: &[],
            said: &[],
            ideas: IdeaGround::default(),
            pulse: &pulse,
        };
        assert_eq!(
            apply_reply(&mut store, Err("no key"), "2026-10-07", 9, &ground),
            0
        );
        assert_eq!(store.last_review_day.as_deref(), Some("2026-10-07"));
        assert_eq!(store.last_review_ms, 9);
        assert_eq!(store.autos.len(), 1);
    }

    #[test]
    fn merge_keeps_reviewed_turns_and_idea_titles() {
        let existing = SuggestionStore {
            reviewed_turns: 8,
            idea_titles: vec!["Tidy the board".into()],
            ..SuggestionStore::default()
        };
        let merged = merge_suggestion_store(&existing, SuggestionStore::default());
        assert_eq!(merged.reviewed_turns, 8);
        assert_eq!(merged.idea_titles, vec!["Tidy the board".to_string()]);
        let newer = SuggestionStore {
            reviewed_turns: 12,
            idea_titles: vec!["Inbox sweep".into()],
            ..SuggestionStore::default()
        };
        let merged = merge_suggestion_store(&existing, newer);
        assert_eq!(merged.reviewed_turns, 12);
        assert_eq!(merged.idea_titles, vec!["Inbox sweep".to_string()]);
    }

    #[test]
    fn the_session_pass_runs_once_a_day_after_the_hour() {
        let line = |t: &str| DigestLine {
            role: "user".into(),
            text: t.into(),
        };
        let lines = vec![
            line("summarize the workboard for me"),
            line("what is on the workboard now"),
        ];
        let at = |hour| LocalClock {
            now_ms: 1,
            weekday: 3,
            hour,
            minute: 0,
        };
        let mut store = SuggestionStore::default();
        assert!(!session_pass(
            &mut store,
            &lines,
            &[],
            &[],
            "2026-10-07",
            &at(20),
            21
        ));
        assert!(store.autos.is_empty());
        assert!(session_pass(
            &mut store,
            &lines,
            &[],
            &[],
            "2026-10-07",
            &at(21),
            21
        ));
        assert_eq!(store.autos.len(), 1);
        assert_eq!(store.autos[0].title, "Midday board");
        assert_eq!(
            store.last_session_suggest_day.as_deref(),
            Some("2026-10-07")
        );
        assert!(!session_pass(
            &mut store,
            &lines,
            &[],
            &[],
            "2026-10-07",
            &at(23),
            21
        ));
        // A later day runs at any hour.
        assert!(session_pass(
            &mut store,
            &lines,
            &[],
            &[],
            "2026-10-08",
            &at(9),
            21
        ));
        assert_eq!(store.autos.len(), 1, "one tile per title");
    }
}
