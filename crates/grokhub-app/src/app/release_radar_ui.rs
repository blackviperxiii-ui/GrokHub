//! Release radar (`grokhub_core::release_radar`), opt-in under Settings →
//! Pulse → Sources. Once a day, with the source check and off the UI thread,
//! it asks Steam's public store about the most-played library games and the
//! wishlist: upcoming DLC and sequels the user does not own become Pulse
//! posts. "Remind me" keeps a reminder that posts again on release day.

use std::collections::BTreeSet;

use super::*;
use grokhub_core::release_radar::{self as rr, Radar, Upcoming};
use grokhub_core::QuickChip;

#[derive(Default)]
pub(super) struct RadarState {
    radar: Option<Radar>,
    rx: Option<mpsc::Receiver<Result<Vec<Upcoming>, String>>>,
    /// This session's posts, for their "Remind me" one-taps.
    offers: Vec<Upcoming>,
    pub note: String,
}

impl Cabin {
    fn radar_now(&mut self) -> &mut Radar {
        self.sources
            .radar
            .radar
            .get_or_insert_with(|| rr::load(&crate::config::config_dir()))
    }

    fn save_radar(&mut self) {
        let radar = self.radar_now().clone();
        if let Err(err) = rr::save(&crate::config::config_dir(), &radar) {
            self.sources.radar.note = err;
        }
    }

    /// The games to ask about: most played first, then the wishlist.
    fn radar_seeds(&mut self, now: u64) -> (Vec<(u32, String)>, BTreeSet<u32>) {
        let mut library = self.steam_library(now).1.to_vec();
        library.sort_by_key(|g| std::cmp::Reverse(g.playtime_min));
        let owned: BTreeSet<u32> = library.iter().map(|g| g.app_id).collect();
        let mut seeds: Vec<(u32, String)> =
            library.into_iter().map(|g| (g.app_id, g.name)).collect();
        seeds.extend(
            self.wish_now()
                .apps
                .iter()
                .filter(|(_, seen)| !seen.name.is_empty())
                .map(|(id, seen)| (*id, seen.name.clone())),
        );
        (seeds, owned)
    }

    fn radar_bias() -> i32 {
        grokhub_core::pulse_sources::load_cache(&super::pulse_sources_ui::sources_cache_path())
            .source_bias
            .get(rr::SOURCE_NAME)
            .copied()
            .unwrap_or(0)
    }

    /// With the source check: post reminders that came due, and start the daily look.
    pub(super) fn kick_release_radar(&mut self, now: u64) {
        self.post_due_reminders(now);
        if self.sources.radar.rx.is_some() || !self.radar_now().due(now) {
            return;
        }
        let (seeds, owned) = self.radar_seeds(now);
        if seeds.is_empty() {
            return;
        }
        self.radar_now().last_check_ms = now;
        self.save_radar();
        self.sources.radar.note = "Looking for upcoming DLC and sequels…".into();
        let (tx, rx) = mpsc::channel();
        self.sources.radar.rx = Some(rx);
        std::thread::spawn(move || {
            let _ = tx.send(rr::scan(&seeds, &owned, &mut super::wishlist_ui::get));
        });
    }

    pub(super) fn poll_release_radar(&mut self) {
        let Some(rx) = self.sources.radar.rx.take() else {
            return;
        };
        match rx.try_recv() {
            Ok(found) => self.finish_release_radar(found, now_ms()),
            Err(mpsc::TryRecvError::Empty) => self.sources.radar.rx = Some(rx),
            Err(mpsc::TryRecvError::Disconnected) => {
                self.sources.radar.note = "The release check stopped before it finished.".into();
            }
        }
    }

    fn finish_release_radar(&mut self, found: Result<Vec<Upcoming>, String>, now: u64) {
        let found = match found {
            Ok(found) => found,
            Err(why) => {
                self.sources.radar.note = format!("Release check failed: {why}.");
                return;
            }
        };
        if !self.radar_now().on {
            return;
        }
        let fresh = self
            .radar_now()
            .fresh(found, rr::posts_allowed(Self::radar_bias()));
        self.save_radar();
        let mut added = 0;
        for u in &fresh {
            if grokhub_core::post_update(&mut self.updates, u.radar_card(now)) {
                added += 1;
            }
        }
        if added > 0 {
            self.persist_updates();
        }
        self.sources.radar.note = if added == 0 {
            "Checked your games. Nothing new coming.".into()
        } else {
            format!("Checked your games. {added} coming up.")
        };
        self.sources.radar.offers = fresh;
        self.chip_paint_key.clear();
    }

    fn post_due_reminders(&mut self, now: u64) {
        if self.radar_now().reminders.is_empty() {
            return;
        }
        let due = self.radar_now().take_due(now);
        if due.is_empty() {
            return;
        }
        self.save_radar();
        for (id, r) in &due {
            grokhub_core::post_update(&mut self.updates, rr::out_today_card(*id, r, now));
        }
        self.persist_updates();
    }

    /// "Remind me when it's out" for this session's posts with an exact date.
    pub(super) fn radar_chips(&self) -> Vec<QuickChip> {
        let reminded = |id: u32| {
            self.sources
                .radar
                .radar
                .as_ref()
                .is_some_and(|r| r.reminders.contains_key(&id))
        };
        self.sources
            .radar
            .offers
            .iter()
            .filter(|u| !reminded(u.app_id))
            .filter_map(Upcoming::chip)
            .filter(|c| !self.chip_dismissed.iter().any(|d| d == &c.id))
            .take(1)
            .collect()
    }

    /// The reminder one-tap. True when `chip` was one.
    pub(super) fn take_radar_chip(&mut self, chip: &QuickChip) -> bool {
        let Some(id) = chip
            .id
            .strip_prefix(rr::REMIND_PREFIX)
            .and_then(|id| id.parse::<u32>().ok())
        else {
            return false;
        };
        let Some(u) = self
            .sources
            .radar
            .offers
            .iter()
            .find(|u| u.app_id == id)
            .cloned()
        else {
            return true;
        };
        if self.radar_now().remind(&u) {
            self.save_radar();
            self.status = format!("I'll post again when {} is out ({}).", u.name, u.date_text);
            self.chip_paint_key.clear();
        }
        true
    }

    /// Settings → Pulse → Sources: the radar toggle.
    pub(super) fn ui_release_radar_source(&mut self, ui: &mut egui::Ui) {
        let mut on = self.radar_now().on;
        if ui
            .checkbox(
                &mut on,
                RichText::new("Tell me about upcoming DLC and sequels for my games")
                    .size(crate::theme::FONT_BODY),
            )
            .changed()
        {
            self.radar_now().on = on;
            if on {
                self.radar_now().last_check_ms = 0;
            } else {
                self.sources.radar.note.clear();
            }
            self.save_radar();
        }
        if !on {
            ui.add_space(6.0);
            return;
        }
        ui.indent("pulse-release-radar", |ui| {
            crate::cards::help_text(
                ui,
                "Once a day, from Steam's public store: your most-played games and your wishlist. Only game ids and names are sent to Steam.",
            );
            let reminders = self
                .sources
                .radar
                .radar
                .as_ref()
                .map(|r| r.reminders.len())
                .unwrap_or(0);
            if reminders > 0 {
                crate::cards::help_text(ui, &format!("Release-day reminders: {reminders}."));
            }
            if !self.sources.radar.note.is_empty() {
                crate::cards::help_text(ui, &self.sources.radar.note);
            }
        });
        ui.add_space(6.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use grokhub_core::release_radar::Kind;

    fn up(id: u32, date: &str) -> Upcoming {
        Upcoming {
            app_id: id,
            name: format!("Star Forge {id}"),
            base: "Star Forge".into(),
            kind: Kind::Dlc,
            date_text: date.into(),
            date_ms: rr::release_day_ms(date),
        }
    }

    #[test]
    fn posts_are_deduped_and_a_tap_creates_the_reminder() {
        let root = crate::config::test_config_root("release-radar");
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::create_dir_all(&root);
        let _pin = crate::config::TestConfigDir::set(root);
        let mut cabin = Cabin::quiet_for_test();
        cabin.radar_now().on = true;
        let now = rr::release_day_ms("1 Nov, 2026").unwrap();
        cabin.finish_release_radar(Ok(vec![up(7, "14 Nov, 2026"), up(8, "Q1 2027")]), now);
        let mut titles: Vec<&str> = cabin
            .updates
            .iter()
            .filter(|c| c.title.starts_with("Coming: "))
            .map(|c| c.title.as_str())
            .collect();
        titles.sort_unstable();
        assert_eq!(titles, vec!["Coming: Star Forge 7", "Coming: Star Forge 8"]);
        cabin.finish_release_radar(Ok(vec![up(7, "14 Nov, 2026")]), now + 1);
        assert_eq!(
            cabin
                .updates
                .iter()
                .filter(|c| c.title.starts_with("Coming: "))
                .count(),
            2,
            "a post is never repeated"
        );
        cabin.sources.radar.offers = vec![up(7, "14 Nov, 2026"), up(8, "Q1 2027")];
        let chips = cabin.radar_chips();
        assert_eq!(
            chips.iter().map(|c| c.label.as_str()).collect::<Vec<_>>(),
            vec!["Remind me when Star Forge 7 is out"]
        );
        assert!(cabin.take_radar_chip(&chips[0]));
        assert!(cabin.radar_chips().is_empty());
        let saved = rr::load(&crate::config::config_dir());
        assert_eq!(
            saved.reminders.get(&7).map(|r| r.date_ms),
            rr::release_day_ms("14 Nov, 2026")
        );
        cabin.kick_release_radar(rr::release_day_ms("14 Nov, 2026").unwrap());
        assert!(cabin
            .updates
            .iter()
            .any(|c| c.title == "Out today: Star Forge 7"));
        assert!(rr::load(&crate::config::config_dir()).reminders.is_empty());
    }

    #[test]
    fn a_failed_check_says_why_and_off_posts_nothing() {
        let root = crate::config::test_config_root("release-radar-off");
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::create_dir_all(&root);
        let _pin = crate::config::TestConfigDir::set(root);
        let mut cabin = Cabin::quiet_for_test();
        cabin.radar_now().on = true;
        cabin.finish_release_radar(Err("could not find the host".into()), 5);
        assert_eq!(
            cabin.sources.radar.note,
            "Release check failed: could not find the host."
        );
        cabin.radar_now().on = false;
        cabin.finish_release_radar(Ok(vec![up(7, "14 Nov, 2026")]), 5);
        assert!(cabin
            .updates
            .iter()
            .all(|c| !c.title.starts_with("Coming: ")));
    }
}
