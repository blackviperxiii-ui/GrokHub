//! Stardew Valley (`grokhub_core::stardew_saves`).
//!
//! While the game watcher is on and the game runs from Steam, this notes how
//! each farm's `SaveGameInfo` stood when the game started. When it closes it
//! reads them again and posts what the farm saved during the session to
//! Pulse, and keeps the newest farm for Settings. The only reads are those
//! small files, at start and close; nothing opens a handle to the game and
//! no save is written.

use super::*;
use grokhub_core::stardew_saves::{self as stardew, FarmSession, StardewFarm};

#[derive(Default)]
pub(super) struct StardewState {
    /// The farms as they stood at launch, while the game runs.
    before: Option<Vec<StardewFarm>>,
    /// The newest farm, read when the game last closed.
    latest: Option<StardewFarm>,
}

impl Cabin {
    /// Each frame: note the farms at launch, post the session at close.
    pub(super) fn tick_stardew(&mut self) {
        let running = self.cfg.game_watch && self.game_running() == Some(stardew::APP_ID);
        if running == self.focus.stardew.before.is_some() {
            return;
        }
        let farms = stardew::find_farms(self.focus.watch.steam_root().as_deref());
        if running {
            self.focus.stardew.before = Some(farms);
            return;
        }
        let before = self.focus.stardew.before.take().unwrap_or_default();
        self.end_stardew_session(&before, farms, now_ms());
    }

    fn end_stardew_session(&mut self, before: &[StardewFarm], after: Vec<StardewFarm>, now: u64) {
        let session = FarmSession::between(before, &after);
        self.focus.stardew.latest = after.into_iter().next();
        let Some(session) = session else {
            return;
        };
        let card = stardew::session_card(&session, now);
        if grokhub_core::post_update(&mut self.updates, card) {
            self.persist_updates();
        }
    }

    /// The newest farm, for the game watcher's row in Settings.
    pub(super) fn stardew_line(&self) -> Option<String> {
        let farm = self.focus.stardew.latest.as_ref()?;
        Some(format!("{}: {}", stardew::NAME, farm.farm_line()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(day: u32, earned: u64) -> String {
        format!(
            "<?xml version=\"1.0\" encoding=\"utf-8\"?><Farmer><name>Testfarmer</name><farmName>Pickle</farmName><money>500</money><totalMoneyEarned>{earned}</totalMoneyEarned><millisecondsPlayed>{day}000</millisecondsPlayed><dayOfMonthForSaveGame>{day}</dayOfMonthForSaveGame><seasonForSaveGame>0</seasonForSaveGame><yearForSaveGame>1</yearForSaveGame></Farmer>"
        )
    }

    #[test]
    fn the_farm_saved_while_the_game_ran_is_posted_when_it_closes() {
        let root = crate::config::test_config_root("stardew-session");
        let _ = std::fs::remove_dir_all(&root);
        let config = root.join("config");
        std::fs::create_dir_all(&config).unwrap();
        let _pin = crate::config::TestConfigDir::set(config);
        let steam = root.join("steam");
        let farm = steam.join(
            "steamapps/compatdata/413150/pfx/drive_c/users/steamuser/AppData/Roaming/StardewValley/Saves/Testfarmer_1",
        );
        std::fs::create_dir_all(&farm).unwrap();
        std::fs::write(farm.join("SaveGameInfo"), info(3, 1000)).unwrap();

        let mut cabin = Cabin::quiet_for_test();
        cabin.cfg.game_watch = true;
        cabin.focus.watch.steam_root = Some(steam);
        cabin.note_running(None, 1);
        cabin.note_running(Some(413150), 2);
        cabin.tick_stardew();
        assert!(cabin.focus.stardew.before.is_some());

        std::fs::write(farm.join("SaveGameInfo"), info(5, 1600)).unwrap();
        let before = cabin.updates.len();
        cabin.note_running(None, 3);
        cabin.tick_stardew();
        assert!(cabin.focus.stardew.before.is_none());
        assert_eq!(cabin.updates.len(), before + 1);
        let card = cabin.updates.last().unwrap();
        assert_eq!(card.title, "Stardew Valley session: 2 days");
        assert_eq!(
            card.body.as_deref(),
            Some("Pickle Farm, Spring 3 to Spring 5, Year 1. Earned 600g.")
        );
        assert!(cabin
            .stardew_line()
            .unwrap()
            .starts_with("Stardew Valley: Testfarmer of Pickle Farm: Spring 5, Year 1."));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_session_with_no_save_posts_nothing() {
        let mut cabin = Cabin::quiet_for_test();
        let before = cabin.updates.len();
        cabin.end_stardew_session(&[], Vec::new(), 1);
        assert_eq!(cabin.updates.len(), before);
        assert_eq!(cabin.stardew_line(), None);
    }
}
