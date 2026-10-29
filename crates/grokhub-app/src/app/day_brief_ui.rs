//! Start-my-day Pulse brief: cabin glue. Aggregates existing resume /
//! watcher / game-news / proactive signals (no new scans), posts one Digest
//! the first time the cabin opens each day (or at the usual active hour),
//! and remembers per-item one-taps for the Pulse row.

use grokhub_core::code_resume as cr;
use grokhub_core::day_brief::{
    self as db, BriefTone, DayBrief, DayBriefInputs, DayBriefSeed, CARD_TITLE, FILE_NAME,
};
use grokhub_core::nudge_watchers::{self as nw};
use grokhub_core::reactions::{self as rx};
use grokhub_core::update_feed::{self as feed, UpdateCard, UpdateKind, UpdateStatus};
use serde::{Deserialize, Serialize};

use super::Cabin;
use crate::config;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BriefFile {
    #[serde(default)]
    last_day: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    latest_md: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    latest_day: Option<String>,
    #[serde(default)]
    latest_at: u64,
    /// One-tap rows for the live card (`label`, `prompt`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    latest_taps: Vec<TapRow>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TapRow {
    label: String,
    prompt: String,
    /// The Pulse card the line came from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    card: Option<String>,
}

fn state_path() -> std::path::PathBuf {
    config::config_dir().join(FILE_NAME)
}

fn load_state() -> BriefFile {
    std::fs::read_to_string(state_path())
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

fn save_state(s: &BriefFile) {
    if let Ok(text) = serde_json::to_string_pretty(s) {
        let _ = std::fs::write(state_path(), text);
    }
}

impl Cabin {
    /// Housekeep: post the start-my-day brief once per local day at the usual
    /// active hour (or on first open when rhythm has not learned yet).
    pub(super) fn tick_day_brief(&mut self) {
        let now = grokhub_core::now_ms();
        let clock = Self::local_clock();
        let today = Self::local_day();
        // The launch resume scan has not landed yet: wait for it rather than
        // spend today's brief without the resume line.
        if self.day_brief_waits_for_resume() {
            return;
        }
        let state = load_state();
        let rhythm = self.rhythm_now().clone();
        let usually_active = rhythm.usually_active(clock.hour);
        if !db::day_brief_due(
            state.last_day.as_deref(),
            &today,
            &clock,
            usually_active,
            rhythm.brief_hour,
            rhythm.enough(),
        ) {
            return;
        }
        self.run_day_brief_now(&today, now, false);
    }

    /// Build, store, and post (or refresh) the brief. `force` skips the day /
    /// hour gate so tests can regenerate.
    pub(super) fn run_day_brief_now(&mut self, day_key: &str, now: u64, force: bool) {
        let mut state = load_state();
        if !force && state.last_day.as_deref() == Some(day_key) {
            return;
        }
        let brief = self.build_day_brief(day_key, now);
        let md = brief.render();
        let taps: Vec<TapRow> = brief
            .one_taps()
            .into_iter()
            .map(|(label, prompt, card)| TapRow {
                label,
                prompt,
                card,
            })
            .collect();
        state.last_day = Some(day_key.to_string());
        state.latest_day = Some(brief.day_key.clone());
        state.latest_md = Some(md.clone());
        state.latest_at = now;
        state.latest_taps = taps;
        save_state(&state);
        self.post_day_brief_card(&brief, &md, now);
    }

    /// "Not today": dismiss the live brief and keep the day marked so it
    /// does not repost until tomorrow.
    pub(super) fn day_brief_not_today(&mut self, card_id: &str) {
        let today = Self::local_day();
        let mut state = load_state();
        state.last_day = Some(today.clone());
        save_state(&state);
        self.pulse_dismiss(card_id, &today);
        self.status = "Not today. See you tomorrow.".into();
    }

    /// One-taps stored for the live start-my-day card, if any.
    pub(super) fn day_brief_taps(&self) -> Vec<(String, String)> {
        load_state()
            .latest_taps
            .into_iter()
            .map(|t| (t.label, t.prompt))
            .collect()
    }

    /// A one-tap on a brief line: the brief and the card the line came from
    /// count as used (opened, a Tapped reaction), then the prompt runs.
    pub(super) fn day_brief_tap(&mut self, brief_id: &str, prompt: String) {
        self.note_day_brief_tap(brief_id, &prompt);
        self.send_chat(prompt);
    }

    fn note_day_brief_tap(&mut self, brief_id: &str, prompt: &str) {
        let source = load_state()
            .latest_taps
            .into_iter()
            .find(|t| t.prompt == *prompt)
            .and_then(|t| t.card);
        let mut changed = feed::mark_update_opened(&mut self.updates, brief_id);
        if let Some(source) = source {
            if let Some(card) = self.updates.iter().find(|c| c.id == source).cloned() {
                self.react(&card, rx::Outcome::Tapped);
                changed |= feed::mark_update_opened(&mut self.updates, &source);
            }
        }
        if changed {
            self.persist_updates();
        }
    }

    fn day_brief_waits_for_resume(&self) -> bool {
        self.cfg.resume_code && !self.cfg.code_folders.is_empty() && self.focus.resume.pending()
    }

    pub(super) fn build_day_brief(&mut self, day_key: &str, now: u64) -> DayBrief {
        let resume = self.day_brief_resume();
        let watchers = self.day_brief_from_cards(DayGather::Watchers, now);
        let game_news = self.day_brief_from_cards(DayGather::GameNews, now);
        let prints = self.day_brief_from_cards(DayGather::Prints, now);
        let backups = self.day_brief_from_cards(DayGather::Backups, now);
        let next_step = self.day_brief_next_step(now);
        let scores: Vec<(String, i32, bool)> = self
            .reactions_now()
            .categories
            .iter()
            .map(|(k, v)| (k.clone(), v.score, v.quiet))
            .collect();
        let tone = BriefTone::from(&self.chat_personality());
        let input = DayBriefInputs {
            day_key: day_key.to_string(),
            generated_at: now,
            resume,
            watchers,
            game_news,
            prints,
            backups,
            next_step,
            category_scores: scores,
            tone,
        };
        db::build_brief(&input)
    }

    fn day_brief_resume(&self) -> Option<DayBriefSeed> {
        if !self.cfg.resume_code {
            return None;
        }
        let repo = cr::pick(self.focus.resume.repos())?;
        let chips = repo.chips(self.focus.resume.editor());
        let prompt = chips
            .first()
            .map(|c| c.value.clone())
            .unwrap_or_else(|| repo.offer_line());
        Some(DayBriefSeed::new(
            format!("{} on {}", repo.name, repo.branch),
            repo.offer_line(),
            prompt,
            db::DayBriefSection::Resume.default_category(),
        ))
    }

    /// Every card of one kind still showing in Pulse: not dismissed, held or
    /// snoozed, and not news from a Steam game the user muted. No cap here:
    /// `build_brief` drops quiet categories first, then caps.
    fn day_brief_from_cards(&self, kind: DayGather, now: u64) -> Vec<DayBriefSeed> {
        let mut out = Vec::new();
        for card in &self.updates {
            if !grokhub_core::pulse::pulse_visible(card, now) || self.steam_muted(card) {
                continue;
            }
            if !kind.matches(card) {
                continue;
            }
            let title = card.title.trim();
            if title.is_empty() {
                continue;
            }
            let body = card.body.clone().unwrap_or_default();
            let prompt = card
                .prompt
                .clone()
                .filter(|p| !p.trim().is_empty())
                .or_else(|| card.details.clone())
                .unwrap_or_else(|| format!("Look into: {title}"));
            let category = rx::card_category_tag(card)
                .map(|(c, _)| c)
                .unwrap_or_else(|| kind.default_category().to_string());
            out.push(DayBriefSeed::new(title, body, prompt, category).from_card(card.id.clone()));
        }
        out
    }

    /// The top proactive step the normal cards would also show: not a topic
    /// muted by "Not this", and not one the user said never to.
    fn day_brief_next_step(&mut self, now: u64) -> Option<DayBriefSeed> {
        let ranked = self.proactive_candidates(now);
        let mind = self.proactive_mind();
        let top = ranked
            .iter()
            .find(|c| self.day_brief_allows(c, &mind, now))?;
        Some(DayBriefSeed::new(
            upper_first(&top.action),
            top.why.clone(),
            top.action.clone(),
            rx::source_category(top.source).to_string(),
        ))
    }

    fn day_brief_allows(
        &self,
        c: &grokhub_core::proactive::Candidate,
        mind: &grokhub_agent::harness::MindCheck,
        now: u64,
    ) -> bool {
        let never = mind
            .prior(&super::proactive_ui::proactive_key(&c.topic))
            .is_some_and(|p| p.never);
        !never && !self.proactive.muted(&c.topic, now)
    }

    fn post_day_brief_card(&mut self, brief: &DayBrief, details: &str, now: u64) {
        let source = brief.source_id();
        self.updates.retain(|c| {
            !(c.source_id.starts_with(db::CARD_SOURCE_PREFIX)
                && c.kind == UpdateKind::Digest
                && c.status != UpdateStatus::Dismissed)
        });
        let mut card = feed::digest_card(&source, CARD_TITLE, &brief.card_body(), now);
        card.details = Some(details.to_string());
        card.prompt = brief.primary_prompt();
        card.pulse.because = Some("Start my day brief".into());
        grokhub_core::hold_if_quiet(&mut card, self.quiet_now());
        self.updates.insert(0, card);
        self.persist_updates();
    }
}

#[derive(Clone, Copy)]
enum DayGather {
    Watchers,
    GameNews,
    Prints,
    Backups,
}

impl DayGather {
    fn matches(self, card: &UpdateCard) -> bool {
        match self {
            Self::Watchers => nw::is_nudge_watcher_card(card),
            Self::GameNews => is_game_news_card(card),
            Self::Prints => {
                card.source_id.starts_with("print:") || card.source_id.starts_with("printer:")
            }
            Self::Backups => {
                card.source_id.starts_with("backup:") || card.source_id.starts_with("backup-nudge:")
            }
        }
    }

    fn default_category(self) -> &'static str {
        match self {
            Self::Watchers => db::DayBriefSection::Watcher.default_category(),
            Self::GameNews => db::DayBriefSection::GameNews.default_category(),
            Self::Prints => db::DayBriefSection::Print.default_category(),
            Self::Backups => db::DayBriefSection::Backup.default_category(),
        }
    }
}

fn is_game_news_card(card: &UpdateCard) -> bool {
    if card.source_id.starts_with("source:steam-wishlist:") {
        return true;
    }
    if let Some((cat, _)) = rx::card_category_tag(card) {
        if cat == "game news" {
            return true;
        }
    }
    card.pulse
        .source_name
        .as_deref()
        .is_some_and(|n| n.ends_with(" on Steam"))
}

fn upper_first(text: &str) -> String {
    let t = text.trim().trim_end_matches(['.', '?']);
    let mut chars = t.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use grokhub_core::code_resume::RepoStatus;
    use grokhub_core::nudge_watchers::{nudge_watcher_card, WatcherId, WatcherResult};
    use grokhub_core::steam_wishlist::{Alert, Price};
    use std::path::PathBuf;

    #[test]
    fn posts_digest_from_fixture_sources_hides_empty_and_once_per_day() {
        let root = crate::config::test_config_root("day-brief");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("config root");
        let _pin = crate::config::TestConfigDir::set(root.clone());
        std::env::set_var("GROKHUB_CONFIG", &root);

        let now = 1_700_000_000_000u64;
        let mut cabin = Cabin::quiet_for_test();
        cabin.cfg.resume_code = true;
        cabin.focus.resume.seed_repos_for_test(vec![RepoStatus {
            path: PathBuf::from("/home/user/code/grokhub"),
            name: "grokhub".into(),
            branch: "feat-x".into(),
            ahead: 2,
            changed: 1,
            last_commit_s: now / 1000,
            ..Default::default()
        }]);

        let watcher = WatcherResult {
            id: WatcherId::SteamPatch,
            title: "Steam patch for Hades".into(),
            body: "A new patch landed.".into(),
            dedupe_key: "hades".into(),
            prompt: Some("Check the Hades patch notes".into()),
            action: None,
            evidence: vec!["patch notes".into()],
        };
        cabin.updates.push(nudge_watcher_card(&watcher, now));

        let alert = Alert {
            app_id: 42,
            name: "Celeste".into(),
            price: Price {
                initial: 1000,
                final_price: 500,
                discount: 50,
                initial_text: "$10.00".into(),
                final_text: "$5.00".into(),
            },
            new_low: true,
        };
        let mut sale = alert.card(now);
        sale.prompt = Some("Talk about the Celeste sale".into());
        cabin.updates.push(sale);

        let day = "2026-10-28";
        cabin.run_day_brief_now(day, now, true);
        let digests: Vec<_> = cabin
            .updates
            .iter()
            .filter(|c| {
                c.kind == UpdateKind::Digest && c.source_id.starts_with(db::CARD_SOURCE_PREFIX)
            })
            .collect();
        assert_eq!(digests.len(), 1, "{:?}", cabin.updates);
        assert_eq!(digests[0].title, CARD_TITLE);
        let details = digests[0].details.as_deref().unwrap_or("");
        assert!(
            details.contains("## Pick up where you left off"),
            "{details}"
        );
        assert!(details.contains("## Watcher alerts"), "{details}");
        assert!(details.contains("## Game news and sales"), "{details}");
        assert!(!details.contains("## Print status"), "{details}");
        assert!(!details.contains("## Backups due"), "{details}");
        assert!(digests[0].prompt.as_ref().is_some_and(|p| !p.is_empty()));
        assert!(!cabin.day_brief_taps().is_empty());

        let before = cabin
            .updates
            .iter()
            .filter(|c| c.source_id.starts_with(db::CARD_SOURCE_PREFIX))
            .count();
        cabin.run_day_brief_now(day, now + 1, false);
        let after = cabin
            .updates
            .iter()
            .filter(|c| c.source_id.starts_with(db::CARD_SOURCE_PREFIX))
            .count();
        assert_eq!(before, after);
        assert_eq!(load_state().last_day.as_deref(), Some(day));

        cabin.cfg.resume_code = false;
        cabin.focus.resume.seed_repos_for_test(Vec::new());
        cabin
            .updates
            .retain(|c| c.source_id.starts_with(db::CARD_SOURCE_PREFIX));
        cabin.run_day_brief_now(day, now + 2, true);
        let details = cabin
            .updates
            .iter()
            .find(|c| c.source_id.starts_with(db::CARD_SOURCE_PREFIX))
            .and_then(|c| c.details.clone())
            .unwrap_or_default();
        assert!(
            !details.contains("## Pick up where you left off"),
            "{details}"
        );
        assert!(!details.contains("## Watcher alerts"), "{details}");
        assert!(!details.contains("## Game news and sales"), "{details}");

        drop(cabin);
        let _ = std::fs::remove_dir_all(&root);
        std::env::remove_var("GROKHUB_CONFIG");
    }

    fn watcher(n: usize, now: u64) -> UpdateCard {
        nudge_watcher_card(
            &WatcherResult {
                id: WatcherId::SteamPatch,
                title: format!("Patch {n}"),
                body: "A new patch landed.".into(),
                dedupe_key: format!("w{n}"),
                prompt: Some(format!("Check patch {n}")),
                action: None,
                evidence: vec!["patch notes".into()],
            },
            now,
        )
    }

    #[test]
    fn gathering_skips_snoozed_held_and_muted_cards_and_leaves_the_cap_to_the_brief() {
        let now = 1_700_000_000_000u64;
        let mut cabin = Cabin::quiet_for_test();
        cabin.updates.clear();
        for n in 0..8 {
            cabin.updates.push(watcher(n, now));
        }
        cabin.updates[0].pulse.snoozed_until = now + 60_000;
        cabin.updates[1].held = true;
        let seeds = cabin.day_brief_from_cards(DayGather::Watchers, now);
        assert_eq!(
            seeds.iter().map(|s| s.title.as_str()).collect::<Vec<_>>(),
            ["Patch 2", "Patch 3", "Patch 4", "Patch 5", "Patch 6", "Patch 7"],
            "six visible watchers, more than the card shows: build_brief caps after it drops quiet ones"
        );
        assert_eq!(
            seeds[0].card_id.as_deref(),
            Some(cabin.updates[2].id.as_str())
        );
        assert_eq!(
            cabin
                .day_brief_from_cards(DayGather::Watchers, now + 60_000)
                .len(),
            7,
            "the snooze ended"
        );

        let mut news = feed::digest_card("source:steam-news:10", "Old Save patch", "Notes", now);
        news.pulse.source_name = Some("Old Save on Steam".into());
        cabin.updates.push(news);
        cabin.sources.steam = super::super::steam_ui::SteamState::for_test(
            vec![grokhub_core::steam::SteamGame {
                app_id: 10,
                name: "Old Save".into(),
                last_played: 1,
                playtime_min: 0,
            }],
            now,
        );
        let games = |c: &Cabin| {
            c.day_brief_from_cards(DayGather::GameNews, now)
                .into_iter()
                .map(|s| s.title)
                .collect::<Vec<_>>()
        };
        assert_eq!(games(&cabin), ["Old Save patch"]);
        cabin.sources.steam.mute_for_test(10);
        assert!(games(&cabin).is_empty(), "news from a muted game stays out");
    }

    #[test]
    fn the_brief_waits_for_the_launch_resume_scan() {
        let mut cabin = Cabin::quiet_for_test();
        cabin.cfg.resume_code = true;
        cabin.cfg.code_folders = vec!["/home/user/code".into()];
        assert!(cabin.day_brief_waits_for_resume(), "scan not started yet");
        cabin.focus.resume.seed_repos_for_test(Vec::new());
        assert!(!cabin.day_brief_waits_for_resume(), "scan landed");
        let mut off = Cabin::quiet_for_test();
        off.cfg.resume_code = true;
        assert!(
            !off.day_brief_waits_for_resume(),
            "no code folders: nothing to wait for"
        );
    }

    #[test]
    fn the_next_step_skips_muted_and_never_topics() {
        use grokhub_agent::harness as hx;
        use grokhub_core::proactive::{Candidate, CandidateSource};
        let root = crate::config::test_config_root("day-brief-next");
        std::fs::create_dir_all(&root).expect("config root");
        let _pin = crate::config::TestConfigDir::set(root.clone());
        let now = 1_700_000_000_000u64;
        let mut cabin = Cabin::quiet_for_test();
        let step = |topic: &str| {
            Candidate::soft(
                CandidateSource::Workboard,
                "Prep it",
                topic,
                0.9,
                0.9,
                "Due soon",
            )
        };
        let mind = cabin.proactive_mind();
        assert!(cabin.day_brief_allows(&step("standup prep"), &mind, now));
        cabin.proactive.not_this("standup prep", now);
        assert!(!cabin.day_brief_allows(&step("standup prep"), &mind, now));
        assert!(cabin.day_brief_allows(&step("inbox triage"), &mind, now));
        hx::note_proactive(
            &root,
            &hx::answer_span(
                &super::super::proactive_ui::proactive_key("inbox triage"),
                hx::DECISION_NEVER,
                cabin.access_mode(),
            ),
        );
        let mind = cabin.proactive_mind();
        assert!(!cabin.day_brief_allows(&step("inbox triage"), &mind, now));
        drop(cabin);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_one_tap_counts_as_using_the_brief_and_its_card() {
        let root = crate::config::test_config_root("day-brief-tap");
        std::fs::create_dir_all(&root).expect("config root");
        let _pin = crate::config::TestConfigDir::set(root.clone());
        let now = 1_700_000_000_000u64;
        let mut cabin = Cabin::quiet_for_test();
        cabin.updates.clear();
        cabin.updates.push(watcher(0, now));
        let watcher_id = cabin.updates[0].id.clone();
        cabin.run_day_brief_now("2026-10-28", now, true);
        let brief_id = cabin
            .updates
            .iter()
            .find(|c| c.source_id.starts_with(db::CARD_SOURCE_PREFIX))
            .map(|c| c.id.clone())
            .expect("brief");
        let (_, prompt) = cabin
            .day_brief_taps()
            .into_iter()
            .find(|(_, p)| p == "Check patch 0")
            .expect("the watcher's one-tap");
        cabin.note_day_brief_tap(&brief_id, &prompt);
        for id in [&brief_id, &watcher_id] {
            let card = cabin.updates.iter().find(|c| &c.id == id).unwrap();
            assert_eq!(card.status, UpdateStatus::Opened, "{}", card.title);
        }
        let mut learned = rx::Reactions::default();
        assert!(
            learned
                .sweep_ignored(&cabin.updates, now + 2 * rx::IGNORED_AFTER_MS)
                .is_empty(),
            "a tapped line is not counted as ignored"
        );
        drop(cabin);
        let _ = std::fs::remove_dir_all(&root);
    }
}
