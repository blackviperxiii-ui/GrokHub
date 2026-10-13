//! Dreambeans daily drop: pick up to ten Pulse source cards per UTC day.
//!
//! Each free [`SourceKind`] is opt-in on [`DailyDropSettings`]; the feature is off
//! by default. Selection is pure logic over ranked source items the caller already
//! has — no HTTP, no Google APIs, no scheduler, and no UI.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::pulse_sources::{
    canonical_url, item_card, title_similarity, Ranked, SourceKind, SAME_STORY_PCT,
};
use crate::update_feed::UpdateCard;

const DAY_MS: u64 = 24 * 60 * 60 * 1000;

/// Cards the daily drop may add in one UTC day.
pub const DAILY_CAP: usize = 10;
/// At most this many stories from one `source_name` in a single day's drop.
pub const PER_SOURCE_MAX: usize = 3;
/// Canonical URLs remembered so the same link is not picked again.
pub const RECENT_URL_MAX: usize = 400;
/// Titles kept for same-story dedupe alongside URLs.
pub const RECENT_TITLE_MAX: usize = 80;

/// Settings → Dreambeans → Daily drop. Off until the user turns it on; each source
/// stays off until opted in.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DailyDropSettings {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub hacker_news: bool,
    #[serde(default)]
    pub reddit: bool,
    #[serde(default)]
    pub web_search: bool,
    #[serde(default)]
    pub github: bool,
    #[serde(default)]
    pub youtube: bool,
    #[serde(default)]
    pub rss: bool,
}

impl DailyDropSettings {
    pub fn on(&self, kind: SourceKind) -> bool {
        match kind {
            SourceKind::HackerNews => self.hacker_news,
            SourceKind::Reddit => self.reddit,
            SourceKind::WebSearch => self.web_search,
            SourceKind::GithubReleases => self.github,
            SourceKind::YouTube => self.youtube,
            SourceKind::Rss => self.rss,
        }
    }

    /// True when the feature is on and at least one free Pulse source is opted in.
    pub fn active(&self) -> bool {
        self.enabled && SourceKind::ALL.iter().any(|k| self.on(*k))
    }
}

/// What has already been picked, for the daily cap and dedupe.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct DailyDropLedger {
    /// UTC day index (`now_ms / DAY_MS`).
    #[serde(default)]
    pub day: u64,
    #[serde(default)]
    pub picked_today: u32,
    #[serde(default)]
    pub recent_urls: Vec<String>,
    #[serde(default)]
    pub recent_titles: Vec<String>,
}

impl DailyDropLedger {
    /// How many more cards may be picked today after `roll_day`.
    pub fn slots_left(&self, now_ms: u64) -> usize {
        let day = day_index(now_ms);
        if self.day != day {
            return DAILY_CAP;
        }
        DAILY_CAP.saturating_sub(self.picked_today as usize)
    }

    pub fn roll_day(&mut self, now_ms: u64) {
        let day = day_index(now_ms);
        if self.day != day {
            self.day = day;
            self.picked_today = 0;
        }
    }
}

/// UTC day bucket for the daily cap.
pub fn day_index(now_ms: u64) -> u64 {
    now_ms / DAY_MS
}

fn seen_urls(ledger: &DailyDropLedger) -> std::collections::BTreeSet<String> {
    ledger.recent_urls.iter().cloned().collect()
}

fn record_pick(ledger: &mut DailyDropLedger, ranked: &[Ranked], now_ms: u64) {
    ledger.roll_day(now_ms);
    for r in ranked {
        let key = canonical_url(&r.item.url);
        if !ledger.recent_urls.iter().any(|u| u == &key) {
            ledger.recent_urls.push(key);
        }
        if !ledger.recent_titles.iter().any(|t| t == &r.item.title) {
            ledger.recent_titles.push(r.item.title.clone());
        }
    }
    if ledger.recent_urls.len() > RECENT_URL_MAX {
        let drop = ledger.recent_urls.len() - RECENT_URL_MAX;
        ledger.recent_urls.drain(..drop);
    }
    if ledger.recent_titles.len() > RECENT_TITLE_MAX {
        let drop = ledger.recent_titles.len() - RECENT_TITLE_MAX;
        ledger.recent_titles.drain(..drop);
    }
    ledger.picked_today += u32::try_from(ranked.len()).unwrap_or(u32::MAX);
}

/// Ranked rows whose source kind is opted in on `settings`.
pub fn eligible_ranked(settings: &DailyDropSettings, ranked: &[Ranked]) -> Vec<Ranked> {
    if !settings.active() {
        return Vec::new();
    }
    ranked
        .iter()
        .filter(|r| settings.on(r.item.kind))
        .cloned()
        .collect()
}

/// Pick the next daily-drop cards from `ranked` (highest score first). Returns an
/// empty vec when disabled, nothing is opted in, the daily cap is full, or nothing
/// qualifies. Updates `ledger` only when at least one card is returned.
pub fn pick(
    settings: &DailyDropSettings,
    ranked: &[Ranked],
    ledger: &mut DailyDropLedger,
    now_ms: u64,
) -> Vec<Ranked> {
    if !settings.active() {
        return Vec::new();
    }
    ledger.roll_day(now_ms);
    let limit = ledger.slots_left(now_ms);
    if limit == 0 {
        return Vec::new();
    }
    let mut seen = seen_urls(ledger);
    let mut titles: Vec<String> = ledger.recent_titles.clone();
    let mut per_source: BTreeMap<String, usize> = BTreeMap::new();
    let mut out = Vec::new();
    for r in ranked {
        if out.len() >= limit {
            break;
        }
        if !settings.on(r.item.kind) {
            continue;
        }
        let key = canonical_url(&r.item.url);
        if seen.contains(&key) {
            continue;
        }
        if titles
            .iter()
            .any(|t| title_similarity(t, &r.item.title) >= SAME_STORY_PCT)
        {
            continue;
        }
        let name = r.item.source_name.clone();
        let n = per_source.get(&name).copied().unwrap_or(0);
        if n >= PER_SOURCE_MAX {
            continue;
        }
        seen.insert(key.clone());
        titles.push(r.item.title.clone());
        *per_source.entry(name).or_insert(0) += 1;
        out.push(r.clone());
    }
    if !out.is_empty() {
        record_pick(ledger, &out, now_ms);
    }
    out
}

/// Feed cards for a pick. Same rules as [`pick`].
pub fn pick_cards(
    settings: &DailyDropSettings,
    ranked: &[Ranked],
    ledger: &mut DailyDropLedger,
    now_ms: u64,
) -> Vec<UpdateCard> {
    pick(settings, ranked, ledger, now_ms)
        .iter()
        .map(|r| item_card(r, now_ms))
        .collect()
}

/// Whether a posted Pulse source card could have come from the daily drop.
pub fn is_daily_drop_card(card: &UpdateCard, settings: &DailyDropSettings) -> bool {
    if !settings.active() || !card.source_id.starts_with("source:") {
        return false;
    }
    let rest = card.source_id.strip_prefix("source:").unwrap_or("");
    let key = rest.split(':').next().unwrap_or("");
    SourceKind::ALL.iter().any(|k| k.key() == key && settings.on(*k))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pulse_sources::{SourceItem, SourceKind};

    const T0: u64 = 1_791_705_600_000; // 2026-10-11T08:00:00Z

    fn item(kind: SourceKind, title: &str, url: &str, source_name: &str) -> SourceItem {
        SourceItem {
            title: title.into(),
            url: url.into(),
            kind,
            source_name: source_name.into(),
            published_ms: T0,
            summary: String::new(),
            image: None,
            topic: None,
            engagement: 0,
        }
    }

    fn ranked(kind: SourceKind, title: &str, url: &str, source_name: &str, score: u32) -> Ranked {
        Ranked {
            item: item(kind, title, url, source_name),
            score,
            because: None,
        }
    }

    #[test]
    fn defaults_are_off_and_inactive() {
        let s = DailyDropSettings::default();
        assert!(!s.enabled);
        assert!(!s.hacker_news);
        assert!(!s.active());
    }

    #[test]
    fn needs_enabled_and_one_source() {
        let off = DailyDropSettings {
            enabled: true,
            ..DailyDropSettings::default()
        };
        assert!(!off.active());
        let on = DailyDropSettings {
            enabled: true,
            hacker_news: true,
            ..DailyDropSettings::default()
        };
        assert!(on.active());
    }

    #[test]
    fn pick_empty_when_disabled() {
        let mut ledger = DailyDropLedger::default();
        let rows = [ranked(
            SourceKind::HackerNews,
            "A",
            "https://a.test/1",
            "Hacker News",
            90,
        )];
        let got = pick(&DailyDropSettings::default(), &rows, &mut ledger, T0);
        assert!(got.is_empty());
        assert_eq!(ledger.picked_today, 0);
    }

    #[test]
    fn respects_opt_in_per_source() {
        let s = DailyDropSettings {
            enabled: true,
            hacker_news: true,
            ..DailyDropSettings::default()
        };
        let rows = [
            ranked(
                SourceKind::Reddit,
                "Reddit only",
                "https://reddit.test/r/1",
                "r/rust",
                100,
            ),
            ranked(
                SourceKind::HackerNews,
                "HN story",
                "https://hn.test/2",
                "Hacker News",
                80,
            ),
        ];
        let mut ledger = DailyDropLedger::default();
        let got = pick(&s, &rows, &mut ledger, T0);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].item.title, "HN story");
    }

    #[test]
    fn daily_cap_is_ten() {
        let s = DailyDropSettings {
            enabled: true,
            rss: true,
            ..DailyDropSettings::default()
        };
        let words = [
            "Alpha", "Bravo", "Charlie", "Delta", "Echo", "Foxtrot", "Golf", "Hotel", "India",
            "Juliet", "Kilo", "Lima", "Mike", "November", "Oscar",
        ];
        let rows: Vec<Ranked> = (0..15)
            .map(|i| {
                let title = format!("{} report on zoning", words[i]);
                let url = format!("https://feed.test/{i}");
                Ranked {
                    item: item(SourceKind::Rss, &title, &url, &format!("feed-{i}")),
                    score: 100 - i as u32,
                    because: None,
                }
            })
            .collect();
        let mut ledger = DailyDropLedger::default();
        let got = pick(&s, &rows, &mut ledger, T0);
        assert_eq!(got.len(), 10);
        assert_eq!(ledger.picked_today, 10);
        let more = pick(&s, &rows, &mut ledger, T0);
        assert!(more.is_empty());
    }

    #[test]
    fn dedupes_recent_urls() {
        let s = DailyDropSettings {
            enabled: true,
            web_search: true,
            ..DailyDropSettings::default()
        };
        let mut ledger = DailyDropLedger::default();
        ledger.recent_urls.push(canonical_url("https://dup.test/x"));
        let rows = [
            ranked(
                SourceKind::WebSearch,
                "Dup",
                "https://dup.test/x?utm_source=mail",
                "example.com",
                50,
            ),
            ranked(
                SourceKind::WebSearch,
                "Fresh",
                "https://fresh.test/y",
                "example.com",
                40,
            ),
        ];
        let got = pick(&s, &rows, &mut ledger, T0);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].item.title, "Fresh");
    }

    #[test]
    fn dedupes_same_story_titles() {
        let s = DailyDropSettings {
            enabled: true,
            github: true,
            ..DailyDropSettings::default()
        };
        let mut ledger = DailyDropLedger::default();
        ledger
            .recent_titles
            .push("Rust 1.99 released with big changes".into());
        let rows = [
            ranked(
                SourceKind::GithubReleases,
                "Rust 1.99 released with big changes today",
                "https://github.com/rust-lang/rust/releases/1.99",
                "rust-lang/rust on GitHub",
                90,
            ),
            ranked(
                SourceKind::GithubReleases,
                "New CLI tool",
                "https://github.com/o/r/releases/1.0",
                "o/r on GitHub",
                80,
            ),
        ];
        let got = pick(&s, &rows, &mut ledger, T0);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].item.title, "New CLI tool");
    }

    #[test]
    fn per_source_max_three_per_day() {
        let s = DailyDropSettings {
            enabled: true,
            youtube: true,
            ..DailyDropSettings::default()
        };
        let words = ["Piano", "Violin", "Cello", "Flute", "Harp"];
        let rows: Vec<Ranked> = (0..5)
            .map(|i| {
                let title = format!("{} lesson on tuning", words[i]);
                let url = format!("https://youtu.be/v{i}");
                Ranked {
                    item: item(SourceKind::YouTube, &title, &url, "My Channel"),
                    score: 100 - i as u32,
                    because: None,
                }
            })
            .collect();
        let mut ledger = DailyDropLedger::default();
        let got = pick(&s, &rows, &mut ledger, T0);
        assert_eq!(got.len(), 3);
    }

    #[test]
    fn cap_resets_on_new_utc_day() {
        let s = DailyDropSettings {
            enabled: true,
            rss: true,
            ..DailyDropSettings::default()
        };
        let rows = [ranked(
            SourceKind::Rss,
            "One",
            "https://feed.test/1",
            "feed",
            50,
        )];
        let mut ledger = DailyDropLedger::default();
        pick(&s, &rows, &mut ledger, T0);
        assert_eq!(ledger.picked_today, 1);
        let next_day = T0 + DAY_MS;
        let rows2 = [ranked(
            SourceKind::Rss,
            "Two",
            "https://feed.test/2",
            "feed",
            50,
        )];
        let got = pick(&s, &rows2, &mut ledger, next_day);
        assert_eq!(got.len(), 1);
        assert_eq!(ledger.picked_today, 1);
        assert_eq!(ledger.day, day_index(next_day));
    }

    #[test]
    fn pick_cards_build_digest_posts() {
        let s = DailyDropSettings {
            enabled: true,
            hacker_news: true,
            ..DailyDropSettings::default()
        };
        let rows = [ranked(
            SourceKind::HackerNews,
            "Show HN",
            "https://hn.test/show",
            "Hacker News",
            70,
        )];
        let mut ledger = DailyDropLedger::default();
        let cards = pick_cards(&s, &rows, &mut ledger, T0);
        assert_eq!(cards.len(), 1);
        assert_eq!(cards[0].title, "Show HN");
        assert!(cards[0].source_id.starts_with("source:hn:"));
    }

    #[test]
    fn eligible_ranked_filters_kinds() {
        let s = DailyDropSettings {
            enabled: true,
            reddit: true,
            ..DailyDropSettings::default()
        };
        let rows = [
            ranked(SourceKind::Reddit, "R", "https://r.test/1", "r/a", 1),
            ranked(SourceKind::Rss, "F", "https://f.test/1", "f", 2),
        ];
        let el = eligible_ranked(&s, &rows);
        assert_eq!(el.len(), 1);
        assert_eq!(el[0].item.kind, SourceKind::Reddit);
    }
}
