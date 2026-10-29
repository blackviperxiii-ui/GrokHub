//! Spot repeated local asks and offer once to automate them.
//!
//! Clusters the user's own GrokHub requests by intent (`same_topic`) and time of
//! day. After [`OFFER_MIN_COUNT`] similar asks, posts a one-tap Automate offer
//! (or a Skill idea when hours are scattered). Nothing leaves the device: the
//! ledger stays under `{config}/ask-offers.json`.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::chat_view::is_workload_user;
use crate::ideas::{is_one_off_ask, same_topic, topic_words, IdeaKind};
use crate::redact::{is_plain_text, redact_secrets};
use crate::review::{LearnedSuggestion, SuggestionKind};
use crate::rhythm::hour_label;
use crate::skill::{is_feedback_ask, skill_dir_name};
use crate::update_feed::{automate_offer_card, idea_card, UpdateCard};

pub const HOUR_MS: u64 = 60 * 60 * 1000;
pub const DAY_MS: u64 = 24 * HOUR_MS;
/// Asks older than this do not count toward a cluster.
pub const WINDOW_MS: u64 = 14 * DAY_MS;
/// How many similar asks unlock one offer.
pub const OFFER_MIN_COUNT: usize = 3;
/// Hours within this distance of the peak still count toward it.
pub const HOUR_TOLERANCE: u32 = 1;
/// Cap on remembered asks in the ledger.
pub const LOG_CAP: usize = 400;
/// Cap on permanently dismissed cluster keys.
pub const DISMISS_CAP: usize = 128;
/// How often the cabin should call [`tick`] at most.
pub const TICK_EVERY_MS: u64 = 15 * 60 * 1000;
/// Default cap on new offers per local day.
pub const DEFAULT_DAILY_MAX: u32 = 1;

pub const LEDGER_FILE: &str = "ask-offers.json";
pub const SOURCE_PREFIX: &str = "askoffer:";

fn yes() -> bool {
    true
}

fn default_daily_max() -> u32 {
    DEFAULT_DAILY_MAX
}

/// Settings in `app.json`. On by default; Settings → Behavior can turn it off.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AskOfferSettings {
    #[serde(default = "yes")]
    pub enabled: bool,
    #[serde(default = "default_daily_max")]
    pub daily_max: u32,
}

impl Default for AskOfferSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            daily_max: DEFAULT_DAILY_MAX,
        }
    }
}

impl AskOfferSettings {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

/// One local user ask: clipped first line, hour, and weekday. Never leaves disk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AskLogEntry {
    pub at_ms: u64,
    pub hour: u32,
    pub weekday: u8,
    pub text: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AskOfferLedger {
    #[serde(default)]
    pub entries: Vec<AskLogEntry>,
    /// Cluster keys the user dismissed: never suggest again.
    #[serde(default)]
    pub dismissed: BTreeSet<String>,
    #[serde(default)]
    pub last_offered: BTreeMap<String, u64>,
    #[serde(default)]
    pub last_tick_ms: u64,
    #[serde(default)]
    pub day: String,
    #[serde(default)]
    pub posted_today: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OfferKind {
    Automation,
    Skill,
}

/// A cluster that crossed the offer threshold.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AskCluster {
    pub key: String,
    pub example: String,
    pub count: usize,
    pub peak_hour: u32,
    /// Fraction of asks on Mon–Fri (1..=5). Used to pick daily vs weekdays.
    pub weekday_share_pct: u32,
    pub kind: OfferKind,
}

/// Ready-to-post card plus the schedule or skill preview Accept will use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OfferCandidate {
    pub cluster: AskCluster,
    pub title: String,
    pub preview: String,
    pub card: UpdateCard,
}

pub struct TickInput<'a> {
    pub settings: &'a AskOfferSettings,
    pub ledger: &'a mut AskOfferLedger,
    /// Titles / instructions of jobs already on the Automations list.
    pub existing_autos: &'a [String],
    /// Source ids of undismissed feed cards (skip when already live).
    pub live_sources: &'a [String],
    pub today: &'a str,
    pub now_ms: u64,
}

pub struct TickOutput {
    pub cards: Vec<UpdateCard>,
    pub posted: u32,
}

/// First-line ask text worth logging, or `None` when filtered out.
pub fn may_log_ask(text: &str) -> Option<String> {
    let t = text.trim();
    if t.len() < 8 || t.starts_with('/') {
        return None;
    }
    if is_workload_user(t) || is_feedback_ask(t) || !is_plain_text(t) || is_one_off_ask(t) {
        return None;
    }
    let line: String = t.lines().next().unwrap_or("").chars().take(200).collect();
    let line = redact_secrets(line.trim());
    if line.len() < 8 || !is_plain_text(&line) {
        return None;
    }
    Some(line)
}

/// Append a filtered ask. Cap and drop the oldest when over [`LOG_CAP`].
pub fn append_ask(ledger: &mut AskOfferLedger, text: &str, hour: u32, weekday: u8, at_ms: u64) {
    let Some(line) = may_log_ask(text) else {
        return;
    };
    // Same line in the same hour of the same day: keep the newest stamp only.
    // The same ask at that hour on another day is a new occurrence.
    if let Some(prev) = ledger.entries.iter_mut().rev().find(|e| {
        e.hour == hour % 24
            && at_ms.abs_diff(e.at_ms) < HOUR_MS
            && e.text.eq_ignore_ascii_case(&line)
    }) {
        prev.at_ms = at_ms;
        prev.weekday = weekday.min(6);
        return;
    }
    ledger.entries.push(AskLogEntry {
        at_ms,
        hour: hour % 24,
        weekday: weekday.min(6),
        text: line,
    });
    if ledger.entries.len() > LOG_CAP {
        let drop = ledger.entries.len() - LOG_CAP;
        ledger.entries.drain(0..drop);
    }
}

fn hour_distance(a: u32, b: u32) -> u32 {
    let a = a % 24;
    let b = b % 24;
    let d = a.abs_diff(b);
    d.min(24 - d)
}

fn part_of_day(hour: u32) -> &'static str {
    match hour % 24 {
        5..=11 => "mornings",
        12..=16 => "afternoons",
        17..=21 => "evenings",
        _ => "nights",
    }
}

/// Clock token `parse_nl_automation` accepts (`9`, `9pm`, `21:00`).
pub fn clock_token(hour: u32) -> String {
    match hour % 24 {
        0 => "12am".into(),
        12 => "12pm".into(),
        h if h < 12 => format!("{h}"),
        h => format!("{}pm", h - 12),
    }
}

fn topic_key(text: &str) -> String {
    let mut words = topic_words(text);
    words.sort();
    if words.is_empty() {
        text.trim().to_ascii_lowercase()
    } else {
        words.join("-")
    }
}

fn cluster_key(topic: &str, peak_hour: u32, kind: OfferKind) -> String {
    match kind {
        OfferKind::Automation => format!("{topic}@h{peak_hour}"),
        OfferKind::Skill => format!("{topic}@skill"),
    }
}

/// Fold recent asks into intent × time-of-day clusters.
pub fn cluster_asks(entries: &[AskLogEntry], now_ms: u64) -> Vec<AskCluster> {
    let cutoff = now_ms.saturating_sub(WINDOW_MS);
    let fresh: Vec<&AskLogEntry> = entries
        .iter()
        .filter(|e| e.at_ms >= cutoff && may_log_ask(&e.text).is_some())
        .collect();
    if fresh.is_empty() {
        return Vec::new();
    }

    // Intent groups: newest ask is the example head.
    let mut groups: Vec<(String, Vec<&AskLogEntry>)> = Vec::new();
    for e in fresh.iter().copied().rev() {
        match groups
            .iter_mut()
            .find(|(head, _)| same_topic(head, &e.text))
        {
            Some((_, members)) => members.push(e),
            None => groups.push((e.text.clone(), vec![e])),
        }
    }

    let mut out = Vec::new();
    for (example, members) in groups {
        let topic = topic_key(&example);
        if topic.is_empty() {
            continue;
        }
        // Peak hour by counting each ask toward its hour (± tolerance).
        // Ties go to the hour with the most exact asks, so asks all at 19
        // peak at 19, not at the 18 that also has them all within tolerance.
        let mut hour_scores = [0u32; 24];
        let mut exact = [0u32; 24];
        for m in &members {
            exact[(m.hour % 24) as usize] += 1;
            for h in 0u32..24 {
                if hour_distance(m.hour, h) <= HOUR_TOLERANCE {
                    hour_scores[h as usize] += 1;
                }
            }
        }
        let (peak_hour, peak_score) = hour_scores
            .iter()
            .enumerate()
            .max_by_key(|(h, score)| (**score, exact[*h], std::cmp::Reverse(*h)))
            .map(|(h, s)| (h as u32, *s))
            .unwrap_or((0, 0));

        let weekday_n = members
            .iter()
            .filter(|m| (1..=5).contains(&m.weekday))
            .count();
        let weekday_share_pct = if members.is_empty() {
            0
        } else {
            ((weekday_n * 100) / members.len()) as u32
        };

        let tight = peak_score as usize >= OFFER_MIN_COUNT;
        if tight {
            out.push(AskCluster {
                key: cluster_key(&topic, peak_hour, OfferKind::Automation),
                example: example.clone(),
                count: peak_score as usize,
                peak_hour,
                weekday_share_pct,
                kind: OfferKind::Automation,
            });
        } else if members.len() >= OFFER_MIN_COUNT {
            out.push(AskCluster {
                key: cluster_key(&topic, peak_hour, OfferKind::Skill),
                example,
                count: members.len(),
                peak_hour,
                weekday_share_pct,
                kind: OfferKind::Skill,
            });
        }
    }
    out.sort_by_key(|c| std::cmp::Reverse(c.count));
    out
}

/// Human title for the suggestion card.
pub fn suggestion_title(cluster: &AskCluster) -> String {
    let when = part_of_day(cluster.peak_hour);
    let clock = hour_label(cluster.peak_hour);
    match cluster.kind {
        OfferKind::Automation => {
            format!("You ask this most {when} around {clock}, want me to do it automatically?")
        }
        OfferKind::Skill => "You keep asking this — want me to save it as a skill?".into(),
    }
}

/// Editable preview Accept / Apply will use.
pub fn offer_preview(cluster: &AskCluster) -> String {
    let ask = cluster.example.trim();
    match cluster.kind {
        OfferKind::Automation => {
            let sched = if cluster.weekday_share_pct >= 70 {
                "every weekday"
            } else {
                "every day"
            };
            format!("{sched} at {}, {ask}", clock_token(cluster.peak_hour))
        }
        OfferKind::Skill => ask.to_string(),
    }
}

fn skill_suggestion(cluster: &AskCluster) -> LearnedSuggestion {
    let name = skill_dir_name(&topic_key(&cluster.example));
    let name = if name.is_empty() {
        "repeated-ask".into()
    } else {
        name.chars().take(48).collect()
    };
    LearnedSuggestion {
        kind: SuggestionKind::Skill,
        title: suggestion_title(cluster),
        body: format!("You asked this about {} times recently.", cluster.count),
        seed: None,
        name: Some(name),
        trigger: Some(format!("when the user asks about {}", cluster.example)),
        instructions: Some(offer_preview(cluster)),
        provider: None,
        tool: None,
    }
}

/// Build the feed card for a cluster. Automation → `automate_offer` with
/// `prompt` = schedule preview; Skill → idea with a `LearnedSuggestion`.
pub fn build_offer_card(cluster: &AskCluster, now_ms: u64) -> UpdateCard {
    let source = format!("{SOURCE_PREFIX}{}", cluster.key);
    let title = suggestion_title(cluster);
    let preview = offer_preview(cluster);
    match cluster.kind {
        OfferKind::Automation => {
            let mut card = automate_offer_card(&source, &title, now_ms);
            card.prompt = Some(preview.clone());
            card.draft = None;
            card.body = Some(format!(
                "Preview: {preview}. Open it to edit first, or Accept to schedule it."
            ));
            card.why = Some(format!(
                "Asked about this {} times around {}.",
                cluster.count,
                hour_label(cluster.peak_hour)
            ));
            card
        }
        OfferKind::Skill => {
            let skill = skill_suggestion(cluster);
            let mut card = idea_card(&source, &title, skill.body.as_str(), now_ms);
            card.idea_kind = Some(IdeaKind::Skill);
            card.skill = Some(skill);
            card.prompt = Some(preview);
            card.why = Some(format!(
                "Asked about this {} times with no single hour.",
                cluster.count
            ));
            card
        }
    }
}

fn ledger_day_reset(ledger: &mut AskOfferLedger, today: &str) {
    if ledger.day != today {
        ledger.day = today.to_string();
        ledger.posted_today = 0;
    }
}

fn already_automated(existing: &[String], example: &str) -> bool {
    existing
        .iter()
        .any(|a| !a.trim().is_empty() && same_topic(a, example))
}

/// Mark a cluster dismissed forever (Dismiss on the suggestion card).
pub fn dismiss_cluster(ledger: &mut AskOfferLedger, cluster_key: &str) {
    let key = cluster_key.trim();
    if key.is_empty() {
        return;
    }
    ledger.dismissed.insert(key.to_string());
    while ledger.dismissed.len() > DISMISS_CAP {
        if let Some(first) = ledger.dismissed.iter().next().cloned() {
            ledger.dismissed.remove(&first);
        } else {
            break;
        }
    }
}

/// Source id → cluster key (`askoffer:topic@h19` → `topic@h19`).
pub fn cluster_key_from_source(source_id: &str) -> Option<&str> {
    source_id
        .strip_prefix(SOURCE_PREFIX)
        .filter(|s| !s.is_empty())
}

pub fn is_ask_offer_card(card: &UpdateCard) -> bool {
    card.source_id.starts_with(SOURCE_PREFIX)
}

/// Detect clusters ready to offer and build cards. Updates the ledger when a
/// card is selected (posted_today / last_offered).
pub fn tick(input: TickInput<'_>) -> TickOutput {
    let TickInput {
        settings,
        ledger,
        existing_autos,
        live_sources,
        today,
        now_ms,
    } = input;

    ledger.last_tick_ms = now_ms;
    if !settings.enabled {
        return TickOutput {
            cards: Vec::new(),
            posted: 0,
        };
    }
    ledger_day_reset(ledger, today);
    let daily_max = settings.daily_max.max(1);
    if ledger.posted_today >= daily_max {
        return TickOutput {
            cards: Vec::new(),
            posted: 0,
        };
    }

    let clusters = cluster_asks(&ledger.entries, now_ms);
    let mut cards = Vec::new();
    for cluster in clusters {
        if ledger.posted_today >= daily_max {
            break;
        }
        if ledger.dismissed.contains(&cluster.key) {
            continue;
        }
        if already_automated(existing_autos, &cluster.example) {
            continue;
        }
        let source = format!("{SOURCE_PREFIX}{}", cluster.key);
        if live_sources.iter().any(|s| s == &source) {
            continue;
        }
        // One offer per cluster lifetime unless dismissed then — last_offered
        // blocks a re-post after Accept (Accept does not dismiss the key).
        if ledger.last_offered.contains_key(&cluster.key) {
            continue;
        }
        let card = build_offer_card(&cluster, now_ms);
        ledger.last_offered.insert(cluster.key.clone(), now_ms);
        ledger.posted_today += 1;
        cards.push(card);
    }
    let posted = cards.len() as u32;
    TickOutput { cards, posted }
}

/// Pure helper: Accept seed for an Automate offer card (draft → prompt → title).
pub fn automate_accept_seed(card: &UpdateCard) -> String {
    card.idea_action()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::automation::{route_schedule, ScheduleRoute};

    fn entry(text: &str, hour: u32, day_offset: u64) -> AskLogEntry {
        // Base: 2027-01-15 00:00 UTC Friday.
        let base = 1_800_000_000_000u64 - (1_800_000_000_000u64 % 86_400_000);
        AskLogEntry {
            at_ms: base + day_offset * DAY_MS + u64::from(hour) * HOUR_MS,
            hour,
            weekday: ((day_offset + 5) % 7) as u8, // Fri=5
            text: text.into(),
        }
    }

    fn evening_patch_notes() -> Vec<AskLogEntry> {
        vec![
            entry("check the game patch notes", 19, 0),
            entry("check tonight's patch notes please", 19, 1),
            entry("check patch notes for the update", 20, 2),
        ]
    }

    #[test]
    fn clustering_groups_fixture_asks_by_intent_and_hour() {
        let now = entry("check patch notes", 19, 2).at_ms + HOUR_MS;
        let clusters = cluster_asks(&evening_patch_notes(), now);
        assert_eq!(clusters.len(), 1);
        let c = &clusters[0];
        assert_eq!(c.kind, OfferKind::Automation);
        assert!(c.count >= OFFER_MIN_COUNT);
        assert!(c.peak_hour == 19 || c.peak_hour == 20);
        assert!(c.example.to_ascii_lowercase().contains("patch"));
    }

    #[test]
    fn asks_all_at_one_hour_peak_at_that_hour() {
        let asks = vec![
            entry("check the game patch notes", 19, 0),
            entry("check tonight's patch notes please", 19, 1),
            entry("check patch notes for the update", 19, 2),
        ];
        let now = asks[2].at_ms + HOUR_MS;
        let clusters = cluster_asks(&asks, now);
        assert_eq!(clusters.len(), 1);
        assert_eq!(clusters[0].peak_hour, 19);
        assert_eq!(
            offer_preview(&clusters[0]),
            "every day at 7pm, check patch notes for the update"
        );
        // Pulse paints the offer's own question, not a rewritten "I can" line.
        let card = build_offer_card(&clusters[0], now);
        assert_eq!(
            crate::pulse::i_can_title(&card),
            "You ask this most evenings around 7 PM, want me to do it automatically?"
        );
    }

    #[test]
    fn suggestion_waits_for_threshold() {
        let now = entry("summarize my pull requests", 9, 2).at_ms;
        let two = vec![
            entry("summarize my pull requests", 9, 0),
            entry("summarize open PRs for me", 9, 1),
        ];
        assert!(cluster_asks(&two, now).is_empty());
        let three = [two, vec![entry("summarize my PRs this morning", 8, 2)]].concat();
        let clusters = cluster_asks(&three, now);
        assert_eq!(clusters.len(), 1);
        assert_eq!(clusters[0].count, 3);
    }

    #[test]
    fn created_automation_preview_matches_route_schedule() {
        let c = AskCluster {
            key: "patch-note@h19".into(),
            example: "check patch notes".into(),
            count: 3,
            peak_hour: 19,
            weekday_share_pct: 40,
            kind: OfferKind::Automation,
        };
        let preview = offer_preview(&c);
        assert_eq!(preview, "every day at 7pm, check patch notes");
        let Some(ScheduleRoute::Clock(a)) = route_schedule(&preview) else {
            panic!("preview must file a clock job: {preview}");
        };
        assert_eq!(a.schedule, "daily");
        assert_eq!(a.time, "19:00");
        assert_eq!(a.name, "check patch notes");

        let card = build_offer_card(&c, 1);
        assert_eq!(card.prompt.as_deref(), Some(preview.as_str()));
        assert_eq!(automate_accept_seed(&card), preview);
        assert!(suggestion_title(&c).contains("evenings"));
        assert!(suggestion_title(&c).contains("7 PM"));
    }

    #[test]
    fn dismiss_persists_and_blocks_retick() {
        let mut ledger = AskOfferLedger::default();
        for e in evening_patch_notes() {
            append_ask(&mut ledger, &e.text, e.hour, e.weekday, e.at_ms);
        }
        let now = ledger.entries.last().unwrap().at_ms + HOUR_MS;
        let settings = AskOfferSettings::default();
        let out = tick(TickInput {
            settings: &settings,
            ledger: &mut ledger,
            existing_autos: &[],
            live_sources: &[],
            today: "2027-01-17",
            now_ms: now,
        });
        assert_eq!(out.cards.len(), 1);
        assert_eq!(out.posted, 1);
        let source = out.cards[0].source_id.clone();
        let key = cluster_key_from_source(&source).expect("prefix");
        dismiss_cluster(&mut ledger, key);
        assert!(ledger.dismissed.contains(key));

        // Clear last_offered so only dismiss should block.
        ledger.last_offered.clear();
        ledger.posted_today = 0;
        let again = tick(TickInput {
            settings: &settings,
            ledger: &mut ledger,
            existing_autos: &[],
            live_sources: &[],
            today: "2027-01-18",
            now_ms: now + DAY_MS,
        });
        assert!(again.cards.is_empty(), "dismissed cluster must stay gone");
    }

    #[test]
    fn scattered_hours_offer_a_skill_instead() {
        let entries = vec![
            entry("draft a standup summary from my commits", 8, 0),
            entry("draft standup from yesterday's commits", 14, 1),
            entry("draft my standup summary please", 21, 2),
        ];
        let now = entries[2].at_ms + HOUR_MS;
        let clusters = cluster_asks(&entries, now);
        assert_eq!(clusters.len(), 1);
        assert_eq!(clusters[0].kind, OfferKind::Skill);
        let card = build_offer_card(&clusters[0], now);
        assert_eq!(card.idea_kind, Some(IdeaKind::Skill));
        assert!(card.skill.is_some());
        assert!(card.prompt.as_deref().unwrap().contains("standup"));
    }

    #[test]
    fn may_log_ask_filters_slash_and_one_offs() {
        assert!(may_log_ask("check patch notes tonight").is_some());
        assert!(may_log_ask("/help").is_none());
        assert!(may_log_ask("install the nvidia driver").is_none());
        assert!(may_log_ask("ok").is_none());
    }

    #[test]
    fn the_same_ask_at_the_same_hour_on_three_days_counts_three_times() {
        let mut ledger = AskOfferLedger::default();
        let day0 = 1_800_000_000_000u64 - 1_800_000_000_000 % DAY_MS + 19 * HOUR_MS;
        append_ask(&mut ledger, "check patch notes tonight", 19, 1, day0);
        // Asked again ten minutes later: still one occurrence.
        append_ask(
            &mut ledger,
            "check patch notes tonight",
            19,
            1,
            day0 + 10 * 60_000,
        );
        append_ask(
            &mut ledger,
            "check patch notes tonight",
            19,
            2,
            day0 + DAY_MS,
        );
        append_ask(
            &mut ledger,
            "check patch notes tonight",
            19,
            3,
            day0 + 2 * DAY_MS,
        );
        let stamps: Vec<u64> = ledger.entries.iter().map(|e| e.at_ms).collect();
        assert_eq!(
            stamps,
            vec![day0 + 10 * 60_000, day0 + DAY_MS, day0 + 2 * DAY_MS]
        );
        let clusters = cluster_asks(&ledger.entries, day0 + 2 * DAY_MS + HOUR_MS);
        assert_eq!(clusters.len(), 1);
        assert_eq!(clusters[0].count, 3);
        assert_eq!(clusters[0].peak_hour, 19);
        assert_eq!(clusters[0].kind, OfferKind::Automation);
    }

    #[test]
    fn append_ask_caps_and_redacts_locally() {
        let mut ledger = AskOfferLedger::default();
        for i in 0..(LOG_CAP + 5) {
            append_ask(
                &mut ledger,
                &format!("check patch notes round {i} please"),
                19,
                5,
                i as u64 * HOUR_MS,
            );
        }
        assert!(ledger.entries.len() <= LOG_CAP);
    }
}
