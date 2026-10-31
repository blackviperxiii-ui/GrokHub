//! Weekly "your week" Pulse recap: cabin glue. Gathers existing counters
//! (Steam library playtime, code-resume repos, optional week-counters ledger,
//! task outcomes, reactions), posts one Digest at the usual active hour, and
//! writes a path-free share PNG when the card goes up.

use grokhub_core::outcome;
use grokhub_core::update_feed::{self as feed, UpdateKind};
use grokhub_core::week_recap::{
    self as wr, RecapTone, WeekRecap, WeekRecapInputs, CARD_TITLE, FILE_NAME,
};
use serde::{Deserialize, Serialize};

use super::Cabin;
use crate::config;

/// Optional ledger written by other opt-in features (prints, disk tidy).
const COUNTERS_FILE: &str = "week-counters.json";
const SHARE_PNG: &str = "week-recap-share.png";

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WeekCounters {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    prints_made: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    disk_freed_bytes: Option<u64>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RecapFile {
    #[serde(default)]
    last_week: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    latest_md: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    latest_week: Option<String>,
    #[serde(default)]
    latest_at: u64,
}

/// How often the housekeep slot re-reads the recap file to see whether
/// this week's recap went up. The gate itself turns on the hour.
const CHECK_EVERY_MS: u64 = 60_000;

fn share_path() -> std::path::PathBuf {
    config::config_dir().join(SHARE_PNG)
}

fn load_state() -> RecapFile {
    super::ledger_store::load_ledger(FILE_NAME)
}

fn save_state(s: &RecapFile) {
    super::ledger_store::save_ledger(FILE_NAME, s);
}

fn load_counters() -> WeekCounters {
    super::ledger_store::load_ledger(COUNTERS_FILE)
}

impl Cabin {
    /// Housekeep slot: day roll, situation ping, rhythm minute, then week recap.
    /// Keeps those four calls out of `mod.rs` (god-file line budget).
    pub(super) fn housekeep_rhythm_and_recap(&mut self) {
        self.roll_today();
        self.release_situation_ping();
        self.note_rhythm();
        self.tick_week_recap();
    }

    /// Post the weekly recap once per week at the usual active hour.
    pub(super) fn tick_week_recap(&mut self) {
        let now = grokhub_core::now_ms();
        if !self.ticks.due(FILE_NAME, now, CHECK_EVERY_MS) {
            return;
        }
        let clock = Self::local_clock();
        let today = Self::local_day();
        let week_key = wr::week_key_from_day(&today, clock.weekday);
        let state = load_state();
        let rhythm = self.rhythm_now().clone();
        let usually_active = rhythm.usually_active(clock.hour);
        if !wr::week_recap_due(
            state.last_week.as_deref(),
            &week_key,
            &clock,
            usually_active,
            rhythm.brief_hour,
            rhythm.enough(),
        ) {
            return;
        }
        self.run_week_recap_now(&week_key, now, false);
    }

    /// Build, store, and post (or refresh) the week recap. `force` skips the
    /// week / hour gate so tests can regenerate.
    pub(super) fn run_week_recap_now(&mut self, week_key: &str, now: u64, force: bool) {
        let mut state = load_state();
        if !force && state.last_week.as_deref() == Some(week_key) {
            return;
        }
        let recap = self.build_week_recap(week_key, now);
        let md = recap.render();
        state.last_week = Some(week_key.to_string());
        state.latest_week = Some(recap.week_key.clone());
        state.latest_md = Some(md.clone());
        state.latest_at = now;
        save_state(&state);
        // Encoding the PNG takes tens of milliseconds: not on the frame.
        let share = share_path();
        let for_png = recap.clone();
        self.week_recap_share = std::thread::Builder::new()
            .name("week-recap-share".into())
            .spawn(move || {
                let _ = std::fs::write(share, wr::share_png(&for_png));
            })
            .ok();
        self.post_week_recap_card(&recap, &md, now);
    }

    pub(super) fn build_week_recap(&mut self, week_key: &str, now: u64) -> WeekRecap {
        let games = self.week_recap_games(now);
        let projects = self.week_recap_projects(now);
        let counters = load_counters();
        let dir = config::config_dir();
        let outcomes = outcome::read_outcomes(&dir);
        let top_tasks = wr::top_tasks_from_outcomes(&outcomes, now);
        let scores: Vec<(String, i32, bool)> = self
            .reactions_now()
            .categories
            .iter()
            .map(|(k, v)| (k.clone(), v.score, v.quiet))
            .collect();
        let top_cat = wr::top_reaction_category(&scores);
        let suggestion = wr::suggest_next(
            top_cat.as_deref(),
            top_tasks.first().map(|t| t.label.as_str()),
        );
        let tone = RecapTone::from(&self.chat_personality());
        let input = WeekRecapInputs {
            week_key: week_key.to_string(),
            generated_at: now,
            games,
            projects,
            prints_made: counters.prints_made,
            disk_freed_bytes: counters.disk_freed_bytes,
            top_tasks,
            suggestion,
            tone,
        };
        wr::build_recap(&input)
    }

    fn week_recap_games(&mut self, now: u64) -> Vec<wr::GamePlayed> {
        self.steam_refresh_library(now);
        let Some(games) = self.sources.steam.library_for_recap() else {
            return Vec::new();
        };
        let rows: Vec<(String, u64, u64)> = games
            .iter()
            .map(|g| (g.name.clone(), g.last_played, g.playtime_min))
            .collect();
        wr::games_from_library(&rows, now)
    }

    fn week_recap_projects(&self, now: u64) -> Vec<wr::ProjectTouched> {
        if !self.cfg.resume_code {
            return Vec::new();
        }
        let rows: Vec<(String, u64, u32)> = self
            .focus
            .resume
            .repos()
            .iter()
            .map(|r| (r.name.clone(), r.last_commit_s, r.ahead))
            .collect();
        wr::projects_from_repos(&rows, now)
    }

    fn post_week_recap_card(&mut self, recap: &WeekRecap, details: &str, now: u64) {
        let source = recap.source_id();
        self.updates.retain(|c| {
            !(c.source_id.starts_with(wr::CARD_SOURCE_PREFIX)
                && c.kind == UpdateKind::Digest
                && c.status != feed::UpdateStatus::Dismissed)
        });
        let mut card = feed::digest_card(&source, CARD_TITLE, &recap.card_body(), now);
        card.details = Some(details.to_string());
        grokhub_core::hold_if_quiet(&mut card, self.quiet_now());
        self.updates.insert(0, card);
        self.persist_updates();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use grokhub_core::steam::SteamGame;
    use grokhub_core::week_recap::{GamePlayed, ProjectTouched};

    #[test]
    fn posts_digest_from_fixture_counters_and_hides_off_signals() {
        let root = crate::config::test_config_root("week-recap");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("config root");
        let _pin = crate::config::TestConfigDir::set(root.clone());
        std::env::set_var("GROKHUB_CONFIG", &root);

        let now = 1_700_000_000_000u64;
        let now_s = now / 1000;
        std::fs::write(
            root.join(COUNTERS_FILE),
            r#"{"printsMade":2,"diskFreedBytes":2048}"#,
        )
        .expect("counters");
        std::fs::write(
            root.join(outcome::OUTCOMES_FILE),
            format!(
                "{}\n",
                serde_json::json!({
                    "task": "t1",
                    "signature": "skill:tidy-cache",
                    "result": "success",
                    "reasons": ["verify_ok"],
                    "finished_at": now - 1_000,
                    "at": now - 1_000,
                })
            ),
        )
        .expect("outcomes");

        let mut cabin = Cabin::quiet_for_test();
        cabin.cfg.resume_code = true;
        cabin
            .focus
            .resume
            .seed_repos_for_test(vec![grokhub_core::code_resume::RepoStatus {
                name: "grokhub".into(),
                last_commit_s: now_s - 30,
                ahead: 5,
                ..Default::default()
            }]);
        cabin.sources.steam.seed_library_for_test(
            vec![SteamGame {
                app_id: 1,
                name: "Hades".into(),
                last_played: now_s - 60,
                playtime_min: 90,
            }],
            true,
            now,
        );

        let week = "2026-10-27";
        cabin.run_week_recap_now(week, now, true);
        let digests: Vec<_> = cabin
            .updates
            .iter()
            .filter(|c| {
                c.kind == UpdateKind::Digest && c.source_id.starts_with(wr::CARD_SOURCE_PREFIX)
            })
            .collect();
        assert_eq!(digests.len(), 1, "{:?}", cabin.updates);
        assert_eq!(digests[0].title, CARD_TITLE);
        let details = digests[0].details.as_deref().unwrap_or("");
        assert!(details.contains("## Games played"), "{details}");
        assert!(details.contains("Hades"), "{details}");
        assert!(details.contains("## Projects touched"), "{details}");
        assert!(details.contains("grokhub · 5 commits"), "{details}");
        assert!(details.contains("## Prints made"), "{details}");
        assert!(details.contains("## Disk freed"), "{details}");
        assert!(details.contains("## Top Loadout tasks"), "{details}");
        assert!(!details.contains("/home/"), "{details}");
        cabin
            .week_recap_share
            .take()
            .expect("the share png is written on a worker")
            .join()
            .unwrap();
        assert!(share_path().is_file(), "share png written");
        let png = std::fs::read(share_path()).expect("png");
        assert!(png.starts_with(b"\x89PNG\r\n\x1a\n"));

        // Steam source off → games section gone on regenerate.
        cabin.sources.steam.seed_library_for_test(
            vec![SteamGame {
                app_id: 1,
                name: "Hades".into(),
                last_played: now_s - 60,
                playtime_min: 90,
            }],
            false,
            now,
        );
        cabin.cfg.resume_code = false;
        let _ = std::fs::remove_file(root.join(COUNTERS_FILE));
        cabin.run_week_recap_now(week, now + 1, true);
        let details = cabin
            .updates
            .iter()
            .find(|c| c.source_id.starts_with(wr::CARD_SOURCE_PREFIX))
            .and_then(|c| c.details.clone())
            .unwrap_or_default();
        assert!(!details.contains("## Games played"), "{details}");
        assert!(!details.contains("## Projects touched"), "{details}");
        assert!(!details.contains("## Prints made"), "{details}");
        assert!(!details.contains("## Disk freed"), "{details}");
        assert!(details.contains("## Top Loadout tasks"), "{details}");

        // Pure helpers still match gather shape.
        assert_eq!(cabin.week_recap_games(now), Vec::<GamePlayed>::new());
        assert_eq!(cabin.week_recap_projects(now), Vec::<ProjectTouched>::new());

        drop(cabin);
        let _ = std::fs::remove_dir_all(&root);
        std::env::remove_var("GROKHUB_CONFIG");
    }
}
