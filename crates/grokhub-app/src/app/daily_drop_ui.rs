//! Dreambeans daily drop: settings, ledger persistence, ranked Pulse rows, and feed wiring.

use serde::{Deserialize, Serialize};

use grokhub_core::daily_drop::DailyDropLedger;
use grokhub_core::governor::{daily_drop_refresh, DailyDropRefreshCtx};
use grokhub_core::pulse_sources::{self as ps, Ranked, SourceItem, SourceKind};

use super::pulse_sources_ui::sources_cache_path;
use super::{now_ms, Cabin};
use crate::config;

pub(super) const DAILY_DROP_FILE: &str = "daily_drop.json";

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct DailyDropFile {
    #[serde(default)]
    pub ledger: DailyDropLedger,
    #[serde(default)]
    pub last_run_day: Option<String>,
}

fn state_path() -> std::path::PathBuf {
    config::config_dir().join(DAILY_DROP_FILE)
}

pub(super) fn load_state() -> DailyDropFile {
    std::fs::read_to_string(state_path())
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

pub(super) fn save_state(s: &DailyDropFile) {
    if let Ok(text) = serde_json::to_string_pretty(s) {
        let path = state_path();
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(path, text);
    }
}

impl Cabin {
    pub(super) fn daily_drop_file(&mut self) -> DailyDropFile {
        if self.daily_drop_file.is_none() {
            self.daily_drop_file = Some(load_state());
        }
        self.daily_drop_file.clone().unwrap_or_default()
    }

    fn set_daily_drop_file(&mut self, file: DailyDropFile) {
        save_state(&file);
        self.daily_drop_file = Some(file);
    }

    /// Ranked free Pulse source rows from the on-disk cache (no network).
    pub(super) fn daily_drop_ranked(&mut self, now: u64) -> Vec<Ranked> {
        let cache = ps::load_cache(&sources_cache_path());
        let items: Vec<SourceItem> = cache
            .entries
            .values()
            .flat_map(|e| e.items.iter().cloned())
            .collect();
        ps::rank_with(
            &items,
            self.interests_now(),
            &cache.source_bias,
            &cache.source_posts,
            now,
        )
    }

    /// Governor [`JobKind::DailyDrop`]: overnight local pick from ranked Pulse rows.
    pub(super) fn tick_daily_drop(&mut self, today: &str) {
        let now = now_ms();
        let mut file = self.daily_drop_file();
        let ranked = self.daily_drop_ranked(now);
        let clock = Self::local_clock();
        let out = daily_drop_refresh(DailyDropRefreshCtx {
            settings: &self.cfg.daily_drop,
            ranked: &ranked,
            ledger: &mut file.ledger,
            last_run_day: file.last_run_day.as_deref(),
            today,
            clock: &clock,
            night_hour: self.dream_hour(),
            now_ms: now,
        });
        let mark = out.mark_day.is_some();
        if let Some(day) = out.mark_day {
            file.last_run_day = Some(day);
        }
        let posted = !out.cards.is_empty();
        for card in out.cards {
            self.post_feed_card(card);
        }
        if mark || posted {
            self.set_daily_drop_file(file);
        }
    }

    pub(super) fn ui_daily_drop(&mut self, ui: &mut eframe::egui::Ui) {
        use eframe::egui::RichText;

        ui.add_space(16.0);
        ui.label(
            RichText::new("Daily drop")
                .size(crate::theme::FONT_BODY)
                .strong()
                .color(crate::theme::fg()),
        );
        ui.add_space(2.0);
        crate::cards::help_text(
            ui,
            "Once a night after dream time, GrokHub may add up to ten ranked source posts to the Feed. Off until you turn it on; each source is opt-in. No model calls.",
        );
        ui.add_space(8.0);
        let mut changed = false;
        let mut on = self.cfg.daily_drop.enabled;
        if ui
            .checkbox(
                &mut on,
                RichText::new("Daily drop").size(crate::theme::FONT_BODY),
            )
            .changed()
        {
            self.cfg.daily_drop.enabled = on;
            changed = true;
        }
        if self.cfg.daily_drop.enabled {
            ui.add_space(6.0);
            for kind in SourceKind::ALL {
                let mut source_on = self.cfg.daily_drop.on(kind);
                if ui
                    .checkbox(
                        &mut source_on,
                        RichText::new(kind.label()).size(crate::theme::FONT_BODY),
                    )
                    .changed()
                {
                    self.cfg.daily_drop.set_on(kind, source_on);
                    changed = true;
                }
            }
        }
        if changed {
            self.persist_cfg();
            self.status = "Saved".into();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use grokhub_core::digest_card;
    use grokhub_core::pulse_sources::{CacheEntry, SourceItem, SourceKind};
    use grokhub_core::review::review_due;

    fn cabin() -> (
        std::sync::MutexGuard<'static, ()>,
        crate::config::TestConfigDir,
        Cabin,
    ) {
        let lock = crate::config::hold_test_config();
        let root = crate::config::test_config_root("daily-drop-ui");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let pin = crate::config::TestConfigDir::set(root);
        (lock, pin, Cabin::quiet_for_test())
    }

    #[test]
    fn settings_round_trip_in_app_json() {
        let (_lock, _root, mut cabin) = cabin();
        cabin.cfg.daily_drop.enabled = true;
        cabin.cfg.daily_drop.hacker_news = true;
        crate::config::save(&cabin.cfg).unwrap();
        let cfg = crate::config::load();
        assert!(cfg.daily_drop.enabled);
        assert!(cfg.daily_drop.hacker_news);
        assert!(!cfg.daily_drop.reddit);
    }

    #[test]
    fn ledger_and_last_run_day_persist() {
        let (_lock, _root, _cabin) = cabin();
        let file = DailyDropFile {
            last_run_day: Some("2026-10-12".into()),
            ledger: DailyDropLedger {
                picked_today: 2,
                ..DailyDropLedger::default()
            },
        };
        save_state(&file);
        let back = load_state();
        assert_eq!(back.last_run_day.as_deref(), Some("2026-10-12"));
        assert_eq!(back.ledger.picked_today, 2);
        let raw =
            std::fs::read_to_string(crate::config::config_dir().join(DAILY_DROP_FILE)).unwrap();
        assert!(raw.contains("lastRunDay"));
    }

    #[test]
    fn tick_posts_todays_cards_to_the_feed() {
        let (_lock, _root, mut cabin) = cabin();
        cabin.cfg.dream_hour = 0;
        cabin.cfg.daily_drop.enabled = true;
        cabin.cfg.daily_drop.hacker_news = true;
        // Wall-clock-relative: tick_daily_drop ranks with now_ms(); a frozen
        // published_ms ages past the 72h freshness band and falls under RANK_MIN.
        let now = now_ms();
        let item = SourceItem {
            title: "HN pick".into(),
            url: "https://hn.test/drop-1".into(),
            kind: SourceKind::HackerNews,
            source_name: "Hacker News".into(),
            published_ms: now,
            summary: "summary".into(),
            image: None,
            topic: None,
            engagement: 0,
        };
        let cache_path = sources_cache_path();
        let mut cache = ps::SourceCache::default();
        cache.entries.insert(
            "https://hn.test/feed".into(),
            CacheEntry {
                fetched_ms: now,
                items: vec![item],
                etag: None,
                last_modified: None,
            },
        );
        ps::save_cache(&cache_path, &cache).unwrap();
        cabin.seed_interests("Interests: Rust", "");
        let today = "2026-10-11";
        let clock = Cabin::local_clock();
        assert!(review_due(None, today, &clock, 0));
        cabin.tick_daily_drop(today);
        assert!(
            cabin
                .updates
                .iter()
                .any(|c| c.title == "HN pick" && c.source_id.starts_with("source:hn:")),
            "daily drop posted a source card"
        );
        let posts = cabin.pulse_feed_posts(now);
        assert!(
            posts.iter().any(|c| c.title == "HN pick"),
            "the feed lists today's drop"
        );
        let file = load_state();
        assert_eq!(file.last_run_day.as_deref(), Some(today));
        assert_eq!(file.ledger.picked_today, 1);
    }

    #[test]
    fn feed_keeps_todays_drop_when_pulse_score_is_low() {
        use grokhub_core::pulse::{ledger_line, LedgerReason};

        let (_lock, _root, mut cabin) = cabin();
        cabin.cfg.daily_drop.enabled = true;
        cabin.cfg.daily_drop.rss = true;
        let now = 1_791_705_600_000;
        crate::config::write_memory(
            "MEMORY.md",
            &ledger_line("2026-10-11", "Drop story", LedgerReason::NotThis),
        )
        .unwrap();
        let mut card = digest_card(
            "source:rss:https://feed.test/drop",
            "Drop story",
            "body",
            now,
        );
        card.citations = vec!["https://feed.test/drop".into()];
        card.pulse.source_name = Some("feed".into());
        cabin.updates = vec![card];
        let posts = cabin.pulse_feed_posts(now);
        assert!(
            posts.iter().any(|c| c.title == "Drop story"),
            "today's daily-drop card stays on the feed when Pulse score is under the floor: {posts:?}"
        );
    }
}
