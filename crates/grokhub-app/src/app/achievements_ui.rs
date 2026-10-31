//! Achievement nudges (`grokhub_core::steam_achievements`), opt-in under
//! Settings → Pulse → Sources. Uses the SteamID the wishlist alerts sealed
//! (`wishlist_ui`); it only leaves in the stats request to Steam. Once a
//! week, off the UI thread, the most recently played Steam games are read
//! and a card posts for each game a few achievements from 100% or with a
//! rare one next.

use super::*;
use grokhub_core::steam_achievements::{self as sa, Nudge, Player, Tracker};

/// Deck shots (`scripts/deck-shots.ps1`) set this to run the check on canned
/// Steam replies, since the runner has no SteamID or public profile.
const DECK_ENV: &str = "GROKHUB_DECK_ACHIEVEMENTS";

#[derive(Default)]
pub(super) struct AchState {
    tracker: Option<Tracker>,
    rx: Option<mpsc::Receiver<AchRun>>,
    pub note: String,
}

struct AchRun {
    picks: Vec<Nudge>,
    private: bool,
    error: Option<String>,
}

/// The whole check for the given games, given a GET. Stops at the first
/// private reply, since every game on the profile is hidden then. Never puts
/// the SteamID in an error.
fn run_check(
    games: &[(u32, String)],
    steam_id: u64,
    get: &mut dyn FnMut(&str) -> Result<String, String>,
) -> AchRun {
    let mut picks = Vec::new();
    let mut failed = 0;
    for (app_id, name) in games {
        let list = match get(&sa::player_url(steam_id, *app_id)).map(|b| sa::parse_player(&b)) {
            Ok(Some(Player::List(list))) => list,
            Ok(Some(Player::Private)) => {
                return AchRun {
                    picks: Vec::new(),
                    private: true,
                    error: None,
                }
            }
            Ok(Some(Player::NoStats)) => continue,
            Ok(None) | Err(_) => {
                failed += 1;
                continue;
            }
        };
        let rates = if sa::worth_rates(&list) {
            get(&sa::global_url(*app_id))
                .map(|b| sa::parse_global(&b))
                .unwrap_or_default()
        } else {
            Default::default()
        };
        picks.extend(sa::pick(*app_id, name, &list, &rates));
    }
    let error = (failed > 0 && failed == games.len())
        .then(|| "Steam's stats pages didn't answer".to_string());
    AchRun {
        picks,
        private: false,
        error,
    }
}

fn deck_check() -> bool {
    super::loadout_home::deck_switch_is(DECK_ENV, "1")
}

/// Canned replies for the deck shot: Hades two from 100%.
fn deck_reply(url: &str) -> Result<String, String> {
    if url.contains("GetGlobalAchievementPercentages") {
        return Ok(r#"{"achievementpercentages":{"achievements":[{"name":"ACH_48","percent":"21.4"},{"name":"ACH_49","percent":"3.1"}]}}"#.into());
    }
    let mut body = String::from("<playerstats><privacyState>public</privacyState><achievements>");
    for i in 0..50 {
        let name = match i {
            48 => "Is There No Escape?".to_string(),
            49 => "Rare Breed".to_string(),
            _ => format!("Feat {i}"),
        };
        body.push_str(&format!(
            "<achievement closed=\"{}\"><name><![CDATA[{name}]]></name><apiname>ACH_{i}</apiname></achievement>",
            u8::from(i < 48)
        ));
    }
    body.push_str("</achievements></playerstats>");
    Ok(body)
}

/// The games to read: the most recently played installs that have play time.
fn games_to_check() -> Vec<(u32, String)> {
    if deck_check() {
        return vec![(1145360, "Hades".into())];
    }
    let Some(root) = grokhub_core::steam::detect_root() else {
        return Vec::new();
    };
    grokhub_core::steam::read_library(&root)
        .into_iter()
        .filter(|g| g.playtime_min > 0)
        .take(sa::GAMES_PER_CHECK)
        .map(|g| (g.app_id, g.name))
        .collect()
}

impl Cabin {
    fn ach_now(&mut self) -> &mut Tracker {
        self.sources
            .ach
            .tracker
            .get_or_insert_with(|| sa::load(&crate::config::config_dir()))
    }

    fn save_ach(&mut self) {
        let tracker = self.ach_now().clone();
        if let Err(err) = sa::save(&crate::config::config_dir(), &tracker) {
            self.status = err;
        }
    }

    /// Every frame with the source poll: finish a check, or start the weekly
    /// one when it's on, due, and a SteamID is saved.
    pub(super) fn tick_achievements(&mut self, now: u64) {
        if let Some(rx) = self.sources.ach.rx.take() {
            match rx.try_recv() {
                Ok(run) => self.finish_achievements(run, now),
                Err(mpsc::TryRecvError::Empty) => self.sources.ach.rx = Some(rx),
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.ach_failed("The achievement check stopped before it finished", now);
                }
            }
            return;
        }
        if !self.ach_now().due(now) {
            return;
        }
        let deck = deck_check();
        let steam_id = if deck {
            // Not a real account: the deck replies never look at it.
            76_561_197_960_265_729
        } else {
            if !self.wish_has_id() {
                return;
            }
            let Some(id) = super::wishlist_ui::open_steam_id(&crate::config::config_dir()) else {
                return;
            };
            id
        };
        // Counted as a check either way; a failure shortens the wait (`ach_failed`).
        self.ach_now().last_check_ms = now;
        self.save_ach();
        let (tx, rx) = mpsc::channel();
        self.sources.ach.rx = Some(rx);
        self.sources.ach.note = "Checking your achievements…".into();
        std::thread::spawn(move || {
            let games = games_to_check();
            let run = if deck {
                run_check(&games, steam_id, &mut deck_reply)
            } else {
                run_check(&games, steam_id, &mut super::wishlist_ui::get)
            };
            let _ = tx.send(run);
        });
    }

    fn finish_achievements(&mut self, run: AchRun, now: u64) {
        if let Some(why) = run.error {
            self.ach_failed(&format!("Achievement check failed: {why}"), now);
            return;
        }
        // Switched off while it ran: the user's choice wins.
        if !self.ach_now().on {
            return;
        }
        let bias =
            grokhub_core::pulse_sources::load_cache(&super::pulse_sources_ui::sources_cache_path())
                .source_bias
                .get(sa::SOURCE_NAME)
                .copied()
                .unwrap_or(0);
        let picks = self
            .ach_now()
            .check(run.picks, run.private, sa::nudges_allowed(bias), now);
        let mut added = 0;
        for nudge in &picks {
            if grokhub_core::post_update(&mut self.updates, nudge.card(now)) {
                added += 1;
            }
        }
        if added > 0 {
            self.persist_updates();
        }
        self.sources.ach.note = if run.private {
            String::new()
        } else if added == 0 {
            "Checked your games. Nothing close to 100% yet.".into()
        } else {
            format!("Checked your games. {added} close to done.")
        };
        self.save_ach();
    }

    /// A check Steam didn't answer: try again in hours, not a week. Nothing
    /// when the source was switched off while it ran.
    fn ach_failed(&mut self, why: &str, now: u64) {
        if !self.ach_now().on {
            return;
        }
        self.ach_now().failed(now);
        let hours = self.ach_now().wait_ms() / (60 * 60 * 1000);
        let when = if hours == 1 {
            "an hour".to_string()
        } else {
            format!("{hours} hours")
        };
        self.sources.ach.note = format!("{why}. Trying again in {when}.");
        self.save_ach();
    }

    fn set_ach_on(&mut self, on: bool) {
        if on {
            self.ach_now().on = true;
        } else {
            self.ach_now().turn_off();
            self.sources.ach.note.clear();
        }
        self.save_ach();
    }

    /// Settings → Pulse → Sources: the achievement nudge toggle.
    pub(super) fn ui_achievements_source(&mut self, ui: &mut egui::Ui) {
        let mut on = self.ach_now().on;
        if ui
            .checkbox(
                &mut on,
                RichText::new("Nudge me when a game is close to 100% achievements")
                    .size(crate::theme::FONT_BODY),
            )
            .changed()
        {
            self.set_ach_on(on);
        }
        if !on {
            ui.add_space(6.0);
            return;
        }
        ui.indent("pulse-achievements", |ui| {
            crate::cards::help_text(
                ui,
                "Once a week, from your public Steam achievements for the games you play most, with links to Steam's free guides. Uses the SteamID saved for wishlist alerts.",
            );
            if self.wish_has_id() {
                if crate::cards::ghost_pill(ui, "Check now") {
                    self.ach_now().last_check_ms = 0;
                    self.tick_achievements(now_ms());
                }
            } else {
                self.ui_steam_id_entry(ui);
            }
            if self.ach_now().private {
                crate::cards::settings_note(
                    ui,
                    "Your Steam game details look private. In Steam, set Privacy → Game details to Public to get nudges.",
                );
            }
            if !self.sources.ach.note.is_empty() {
                crate::cards::help_text(ui, &self.sources.ach.note.clone());
            }
        });
        ui.add_space(6.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FAKE_ID: u64 = 76_561_197_960_278_073;

    #[test]
    fn a_check_reads_stats_then_rates_only_for_games_worth_it() {
        let mut asked = Vec::new();
        let games = vec![(1145360, "Hades".to_string()), (20, "Fresh".to_string())];
        let run = run_check(&games, FAKE_ID, &mut |url| {
            asked.push(url.to_string());
            if url.contains("/stats/20/") {
                Ok("<playerstats><privacyState>public</privacyState><achievements><achievement closed=\"0\"><name>A</name><apiname>A</apiname></achievement></achievements></playerstats>".into())
            } else {
                deck_reply(url)
            }
        });
        assert_eq!(run.error, None);
        assert!(!run.private);
        assert_eq!(run.picks.len(), 1);
        assert_eq!(run.picks[0].title(), "2 away from 100% in Hades");
        assert_eq!(
            run.picks[0].body(),
            "Left: Is There No Escape? (21% of players), Rare Breed (3% of players). Steam's community guides are free."
        );
        assert_eq!(
            asked.len(),
            3,
            "stats for both, rates only for Hades: {asked:?}"
        );
        assert!(asked[1].contains("gameid=1145360"));
    }

    #[test]
    fn a_private_profile_stops_the_check_with_no_picks() {
        let mut calls = 0;
        let games = vec![(1, "A".to_string()), (2, "B".to_string())];
        let run = run_check(&games, FAKE_ID, &mut |_| {
            calls += 1;
            Ok("<response><error><![CDATA[This profile is private.]]></error></response>".into())
        });
        assert!(run.private);
        assert!(run.picks.is_empty());
        assert_eq!(calls, 1);
    }

    #[test]
    fn errors_never_name_the_steam_id() {
        let games = vec![(1, "A".to_string())];
        let run = run_check(&games, FAKE_ID, &mut |url| Err(format!("refused {url}")));
        let why = run.error.expect("all failed");
        assert_eq!(why, "Steam's stats pages didn't answer");
        assert!(!why.contains(&FAKE_ID.to_string()));
    }

    /// A fresh config folder for this test, so parallel test processes never
    /// share the real one's achievements file.
    fn isolated_config(label: &str) -> crate::config::TestConfigDir {
        let root = crate::config::test_config_root(label);
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::create_dir_all(&root);
        crate::config::TestConfigDir::set(root)
    }

    #[test]
    fn a_finished_check_posts_the_nudge_once_and_says_so() {
        let _g = crate::config::hold_test_config();
        let _pin = isolated_config("ach-nudge");
        let mut cabin = Cabin::quiet_for_test();
        cabin.sources.ach.tracker = Some(Tracker {
            on: true,
            ..Default::default()
        });
        let games = vec![(1145360, "Hades".to_string())];
        let run = run_check(&games, FAKE_ID, &mut deck_reply);
        let before = cabin.updates.len();
        cabin.finish_achievements(run, 5_000);
        assert_eq!(cabin.updates.len(), before + 1);
        assert!(cabin
            .updates
            .iter()
            .any(|c| c.title == "2 away from 100% in Hades"));
        assert_eq!(
            cabin.sources.ach.note,
            "Checked your games. 1 close to done."
        );
        let again = run_check(&games, FAKE_ID, &mut deck_reply);
        cabin.finish_achievements(again, 6_000);
        assert_eq!(cabin.updates.len(), before + 1, "same unlocks post once");
        assert_eq!(
            sa::load(&crate::config::config_dir()).nudged.get(&1145360),
            Some(&48)
        );
    }

    #[test]
    fn a_failed_check_retries_in_an_hour_then_backs_off() {
        let _g = crate::config::hold_test_config();
        let _pin = isolated_config("ach-retry");
        let mut cabin = Cabin::quiet_for_test();
        let hour = 60 * 60 * 1000;
        cabin.sources.ach.tracker = Some(Tracker {
            on: true,
            last_check_ms: 5_000,
            ..Default::default()
        });
        let games = vec![(1, "A".to_string())];
        let failed = || {
            run_check(&games, FAKE_ID, &mut |_: &str| -> Result<String, String> {
                Err("offline".into())
            })
        };
        cabin.finish_achievements(failed(), 5_000);
        assert_eq!(
            cabin.sources.ach.note,
            "Achievement check failed: Steam's stats pages didn't answer. Trying again in an hour."
        );
        assert!(cabin.ach_now().due(5_000 + hour));
        assert_eq!(sa::load(&crate::config::config_dir()).failures, 1);
        cabin.finish_achievements(failed(), 5_000);
        assert_eq!(
            cabin.sources.ach.note,
            "Achievement check failed: Steam's stats pages didn't answer. Trying again in 2 hours."
        );
        assert!(!cabin.ach_now().due(5_000 + hour));
        // A check that took 3 hours to fail waits from when it failed.
        cabin.finish_achievements(failed(), 5_000 + 3 * hour);
        assert!(!cabin.ach_now().due(5_000 + 3 * hour + 1));
        assert!(cabin.ach_now().due(5_000 + 7 * hour));
        let ok = run_check(&games, FAKE_ID, &mut deck_reply);
        cabin.finish_achievements(ok, 9_000);
        assert_eq!(cabin.ach_now().failures, 0);
        assert!(!cabin.ach_now().due(9_000 + 2 * hour));
    }

    #[test]
    fn a_private_check_posts_nothing_and_flags_the_setting() {
        let _g = crate::config::hold_test_config();
        let _pin = isolated_config("ach-private");
        let mut cabin = Cabin::quiet_for_test();
        cabin.sources.ach.tracker = Some(Tracker {
            on: true,
            ..Default::default()
        });
        let before = cabin.updates.len();
        cabin.finish_achievements(
            AchRun {
                picks: Vec::new(),
                private: true,
                error: None,
            },
            5_000,
        );
        assert_eq!(cabin.updates.len(), before);
        assert!(cabin.ach_now().private);
        assert_eq!(cabin.sources.ach.note, "");
    }
}
