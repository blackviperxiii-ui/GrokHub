//! Local Home preferences. `card_prefs.json` holds feature keys and numbers only.
//! No card text, no network, no model call.
//!
//! A missing or unreadable file is rebuilt from `card_signals.jsonl` (and `.1`).
//! That log has kind, group, and event, so the rebuild has no topic keywords.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::card_signals::{signal_group, CardEvent, CardSignal};
use crate::update_feed::{
    card_needs_you, surfaces_on_home, visible_updates, FeedPulse, UpdateCard, UpdateKind,
    FEED_PAINT_MAX,
};

/// Topic keywords kept per store.
pub const TOPIC_CAP: usize = 200;
/// Group keys kept per store.
pub const GROUP_CAP: usize = 200;
/// Half-life for `pos` and `neg`, in days.
const HALF_LIFE_DAYS: f64 = 14.0;
const DAY_MS: f64 = 86_400_000.0;
const HOUR_MS: f64 = 3_600_000.0;
/// A kind or group with fewer observations than this can take the novelty bonus.
const NOVELTY_OBS: u32 = 3;
/// Added to at most one card per ranking pass.
pub const NOVELTY_BONUS: f64 = 0.2;
/// Cards under this score fold into "More (n)", unless they are protected.
pub const FOLD_BELOW: f64 = 0.15;
/// A hint term has to be at least this large. Novelty is the exception.
pub const HINT_MIN: f64 = 0.25;

pub const HINT_OPEN_GROUP: &str = "You usually open these";
pub const HINT_NEW: &str = "New for you";
pub const HINT_NEEDS: &str = "Needs a look";
pub const FOLD_NOTE: &str = "Showing fewer of these; you've been dismissing them";

const STOP: &[&str] = &[
    "about", "after", "again", "also", "and", "are", "been", "before", "being", "but", "cabin",
    "cant", "can't", "check", "could", "daily", "does", "doesn't", "don't", "done", "dont", "each",
    "every", "for", "from", "give", "gonna", "had", "has", "have", "help", "here", "hour", "hours",
    "into", "it's", "its", "just", "like", "make", "mine", "minute", "minutes", "morning", "need",
    "night", "not", "over", "please", "should", "show", "some", "still", "tell", "than", "that",
    "the", "them", "then", "there", "these", "they", "thing", "things", "this", "time", "times",
    "today", "tomorrow", "under", "used", "using", "wanna", "want", "was", "were", "weekly",
    "weekday", "weekdays", "what", "when", "where", "which", "while", "will", "with", "work",
    "working", "would", "you", "your", "yours",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LearnedBucket {
    Group,
    Topic,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LearnedEntry {
    pub bucket: LearnedBucket,
    pub key: String,
    /// `1` liked, `-1` disliked.
    pub sign: i8,
}

/// One feature value. `pos` and `neg` decay lazily. `n` is the observation count.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Weight {
    #[serde(default)]
    pub pos: f64,
    #[serde(default)]
    pub neg: f64,
    #[serde(default)]
    pub n: u32,
    #[serde(default)]
    pub at: u64,
}

/// Kind names, group keys, and single title keywords. Values are numbers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct CardPrefs {
    #[serde(default)]
    pub kinds: BTreeMap<String, Weight>,
    #[serde(default)]
    pub groups: BTreeMap<String, Weight>,
    #[serde(default)]
    pub topics: BTreeMap<String, Weight>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct HomeRank {
    pub deck: Vec<UpdateCard>,
    pub folded: Vec<UpdateCard>,
    /// The one card that received [`NOVELTY_BONUS`] this pass.
    pub novelty_id: Option<String>,
}

#[derive(Clone, Copy)]
enum Delta {
    Pos(f64),
    Neg(f64),
    Count,
    Unhide,
}

fn kind_key(kind: UpdateKind) -> &'static str {
    match kind {
        UpdateKind::AutomationDone => "automation_done",
        UpdateKind::ScheduleCreated => "schedule_created",
        UpdateKind::Suggestion => "suggestion",
        UpdateKind::AutomateOffer => "automate_offer",
        UpdateKind::Idea => "idea",
        UpdateKind::Digest => "digest",
    }
}

fn is_stop(word: &str) -> bool {
    STOP.contains(&word)
}

fn clock_len(chars: &[char]) -> Option<usize> {
    let digits = |slice: &[char], max: usize| {
        slice.iter().take(max).take_while(|c| c.is_ascii_digit()).count()
    };
    let mut hours = digits(chars, 2);
    if hours == 0 {
        return None;
    }
    if hours == 2 && chars.get(2) != Some(&':') {
        hours = 1;
    }
    if chars.get(hours) != Some(&':') {
        return None;
    }
    let mut i = hours + 1;
    if digits(&chars[i..], 2) != 2 {
        return None;
    }
    i += 2;
    if chars.get(i) == Some(&':') && digits(&chars[i + 1..], 2) == 2 {
        i += 3;
    }
    Some(i)
}

/// At most three lowercase words from a card title. Stopwords, ASCII digits,
/// clock times, and words shorter than three characters are dropped.
/// Single words only. Callers pass the title, never the body.
pub fn topic_keywords(title: &str) -> Vec<String> {
    let chars: Vec<char> = title.to_ascii_lowercase().chars().collect();
    let mut stripped = String::new();
    let mut i = 0;
    while i < chars.len() {
        if let Some(n) = clock_len(&chars[i..]) {
            stripped.push(' ');
            i += n;
            continue;
        }
        if chars[i].is_ascii_digit() {
            i += 1;
            continue;
        }
        stripped.push(chars[i]);
        i += 1;
    }
    let mut out = Vec::new();
    for raw in stripped.split(|c: char| !c.is_ascii_alphabetic()) {
        if raw.len() < 3 || is_stop(raw) || out.iter().any(|kept: &String| kept == raw) {
            continue;
        }
        out.push(raw.to_string());
        if out.len() == 3 {
            break;
        }
    }
    out
}

fn decay_factor(at: u64, now: u64) -> f64 {
    let age_ms = now.saturating_sub(at);
    let age_days = age_ms as f64 / DAY_MS;
    0.5_f64.powf(age_days / HALF_LIFE_DAYS)
}

/// `pos` and `neg` as of `now`. `n` does not decay.
pub fn decayed_weight(weight: &Weight, now: u64) -> (f64, f64) {
    let factor = decay_factor(weight.at, now);
    (weight.pos * factor, weight.neg * factor)
}

fn affinity(pos: f64, neg: f64) -> f64 {
    (pos + 1.0) / (pos + neg + 2.0) - 0.5
}

fn bump(map: &mut BTreeMap<String, Weight>, key: &str, delta: Delta, now: u64) {
    if matches!(delta, Delta::Unhide) && !map.contains_key(key) {
        return;
    }
    let weight = map.entry(key.to_string()).or_default();
    let (pos, neg) = decayed_weight(weight, now);
    match delta {
        Delta::Pos(value) => {
            weight.pos = pos + value;
            weight.neg = neg;
            weight.n = weight.n.saturating_add(1);
        }
        Delta::Neg(value) => {
            weight.pos = pos;
            weight.neg = neg + value;
            weight.n = weight.n.saturating_add(1);
        }
        Delta::Count => {
            weight.pos = pos;
            weight.neg = neg;
            weight.n = weight.n.saturating_add(1);
        }
        Delta::Unhide => {
            weight.pos = pos;
            weight.neg = (neg - 3.0).max(0.0);
        }
    }
    weight.at = now;
}

fn enforce_cap(map: &mut BTreeMap<String, Weight>, now: u64, cap: usize) {
    if map.len() <= cap {
        return;
    }
    let mut ranked: Vec<(f64, String)> = map
        .iter()
        .map(|(key, weight)| {
            let (pos, neg) = decayed_weight(weight, now);
            (pos + neg, key.clone())
        })
        .collect();
    ranked.sort_by(|a, b| a.0.total_cmp(&b.0).then(b.1.cmp(&a.1)));
    while map.len() > cap {
        let Some((_, key)) = ranked.first().cloned() else {
            break;
        };
        map.remove(&key);
        ranked.remove(0);
    }
}

fn event_deltas(event: CardEvent, after_open: Option<bool>) -> (Option<Delta>, Option<Delta>, Option<Delta>) {
    match event {
        CardEvent::Opened => (Some(Delta::Pos(1.0)), Some(Delta::Pos(1.0)), Some(Delta::Pos(1.0))),
        CardEvent::Dismissed => {
            let delta = if after_open == Some(true) {
                Delta::Count
            } else {
                Delta::Neg(1.0)
            };
            (Some(delta), Some(delta), Some(delta))
        }
        CardEvent::More => (Some(Delta::Pos(3.0)), Some(Delta::Pos(3.0)), Some(Delta::Pos(3.0))),
        CardEvent::Less => (None, Some(Delta::Neg(3.0)), Some(Delta::Neg(3.0))),
        CardEvent::Hidden => (None, Some(Delta::Neg(3.0)), None),
        CardEvent::Unhidden => (None, Some(Delta::Unhide), None),
        CardEvent::FollowUp => (Some(Delta::Count), Some(Delta::Count), Some(Delta::Count)),
    }
}

/// Learn from one card event. Topic keywords come from the title at this moment.
pub fn apply_card_event(
    prefs: &mut CardPrefs,
    card: &UpdateCard,
    event: CardEvent,
    after_open: Option<bool>,
    now: u64,
) {
    let (kind_delta, group_delta, topic_delta) = event_deltas(event, after_open);
    if let Some(delta) = kind_delta {
        bump(&mut prefs.kinds, kind_key(card.kind), delta, now);
    }
    if let Some(delta) = group_delta {
        bump(&mut prefs.groups, &signal_group(card), delta, now);
        enforce_cap(&mut prefs.groups, now, GROUP_CAP);
    }
    if let Some(delta) = topic_delta {
        for topic in topic_keywords(&card.title) {
            bump(&mut prefs.topics, &topic, delta, now);
        }
        enforce_cap(&mut prefs.topics, now, TOPIC_CAP);
    }
}

fn apply_signal(prefs: &mut CardPrefs, signal: &CardSignal) {
    let (kind_delta, group_delta, _) = event_deltas(signal.event, signal.after_open);
    let now = signal.ts;
    if let Some(delta) = kind_delta {
        let kind = signal.kind.trim();
        if !kind.is_empty() {
            bump(&mut prefs.kinds, kind, delta, now);
        }
    }
    if let Some(delta) = group_delta {
        let group = signal.group.trim();
        if !group.is_empty() {
            bump(&mut prefs.groups, group, delta, now);
            enforce_cap(&mut prefs.groups, now, GROUP_CAP);
        }
    }
}

fn read_signal_file(prefs: &mut CardPrefs, path: &Path) {
    let Ok(text) = fs::read_to_string(path) else {
        return;
    };
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Ok(signal) = serde_json::from_str::<CardSignal>(line) {
            apply_signal(prefs, &signal);
        }
    }
}

fn rebuild_from_signals(dir: &Path) -> CardPrefs {
    let mut prefs = CardPrefs::default();
    read_signal_file(&mut prefs, &dir.join("card_signals.jsonl.1"));
    read_signal_file(&mut prefs, &dir.join("card_signals.jsonl"));
    prefs
}

/// Load `<dir>/card_prefs.json`. A missing or unreadable file is rebuilt from
/// the signal log. An empty object is a real reset and is not rebuilt.
pub fn load_card_prefs(dir: &Path) -> CardPrefs {
    let path = dir.join("card_prefs.json");
    match fs::read_to_string(&path) {
        Ok(text) => serde_json::from_str(&text).unwrap_or_else(|_| rebuild_from_signals(dir)),
        Err(_) => rebuild_from_signals(dir),
    }
}

pub fn card_prefs_json(prefs: &CardPrefs) -> Result<String, String> {
    serde_json::to_string_pretty(prefs).map_err(|err| err.to_string())
}

fn feature_affinity(map: &BTreeMap<String, Weight>, key: &str, now: u64) -> (f64, u32) {
    match map.get(key) {
        Some(weight) => {
            let (pos, neg) = decayed_weight(weight, now);
            (affinity(pos, neg), weight.n)
        }
        None => (0.0, 0),
    }
}

fn base_score(card: &UpdateCard) -> f64 {
    if card_needs_you(card) {
        1.0
    } else {
        match card.kind {
            UpdateKind::AutomationDone => 0.5,
            UpdateKind::ScheduleCreated | UpdateKind::Suggestion | UpdateKind::AutomateOffer => 0.3,
            UpdateKind::Idea | UpdateKind::Digest => 0.0,
        }
    }
}

struct TopicScore {
    mean: f64,
    best_positive: Option<String>,
}

fn topic_score(card: &UpdateCard, prefs: &CardPrefs, now: u64) -> TopicScore {
    let mut sum = 0.0;
    let mut count = 0.0;
    let mut best: Option<(f64, String)> = None;
    for topic in topic_keywords(&card.title) {
        let Some(weight) = prefs.topics.get(&topic) else {
            continue;
        };
        let (pos, neg) = decayed_weight(weight, now);
        let score = affinity(pos, neg);
        sum += score;
        count += 1.0;
        let replace = match &best {
            None => true,
            Some((prev, _)) => score > *prev,
        };
        if score > 0.0 && replace {
            best = Some((score, topic));
        }
    }
    TopicScore {
        mean: if count == 0.0 { 0.0 } else { sum / count },
        best_positive: best.map(|(_, topic)| topic),
    }
}

/// Score parts. `novelty` adds [`NOVELTY_BONUS`] when the caller already chose this card.
pub fn card_score(card: &UpdateCard, prefs: &CardPrefs, now: u64, novelty: bool) -> f64 {
    let (kind_a, _) = feature_affinity(&prefs.kinds, kind_key(card.kind), now);
    let (group_a, _) = feature_affinity(&prefs.groups, &signal_group(card), now);
    let topics = topic_score(card, prefs, now);
    let age_h = now.saturating_sub(card.created_at) as f64 / HOUR_MS;
    let recency = 0.3 * 0.5_f64.powf(age_h / 12.0);
    let novelty_term = if novelty { NOVELTY_BONUS } else { 0.0 };
    base_score(card) + kind_a + 1.5 * group_a + topics.mean + recency + novelty_term
}

struct HintTerm {
    mag: f64,
    show: Option<String>,
    /// Lower wins a magnitude tie.
    order: u8,
}

fn consider(terms: &mut Vec<HintTerm>, contrib: f64, show: Option<String>, order: u8, always: bool) {
    let mag = contrib.abs();
    if mag == 0.0 && !always {
        return;
    }
    if !always && mag < HINT_MIN {
        return;
    }
    terms.push(HintTerm { mag, show, order });
}

/// Hint for the largest |contribution| among the explain terms. Not stored.
/// Novelty is listed even at 0.2 so the one new card can say "New for you"
/// when nothing stronger qualifies. A larger negative term blocks it.
pub fn explain_hint(card: &UpdateCard, prefs: &CardPrefs, now: u64, novelty: bool) -> Option<String> {
    let (group_a, _) = feature_affinity(&prefs.groups, &signal_group(card), now);
    let group_c = 1.5 * group_a;
    let topics = topic_score(card, prefs, now);
    let topic_c = topics.mean;
    let mut terms = Vec::new();
    if card_needs_you(card) {
        consider(&mut terms, base_score(card), Some(HINT_NEEDS.to_string()), 0, false);
    }
    let group_label = if group_c > 0.0 {
        Some(HINT_OPEN_GROUP.to_string())
    } else {
        None
    };
    consider(&mut terms, group_c, group_label, 1, false);
    let topic_label = if topic_c > 0.0 {
        topics.best_positive.as_deref().map(hint_open_topic)
    } else {
        None
    };
    consider(&mut terms, topic_c, topic_label, 2, false);
    if novelty {
        consider(&mut terms, NOVELTY_BONUS, Some(HINT_NEW.to_string()), 3, true);
    }
    terms.sort_by(|a, b| b.mag.total_cmp(&a.mag).then(a.order.cmp(&b.order)));
    terms.into_iter().next().and_then(|term| term.show)
}

pub fn hint_open_topic(topic: &str) -> String {
    format!("You often open {topic} cards")
}

fn novel(prefs: &CardPrefs, card: &UpdateCard) -> bool {
    let kind_n = prefs
        .kinds
        .get(kind_key(card.kind))
        .map(|weight| weight.n)
        .unwrap_or(0);
    let group_n = prefs
        .groups
        .get(&signal_group(card))
        .map(|weight| weight.n)
        .unwrap_or(0);
    kind_n < NOVELTY_OBS || group_n < NOVELTY_OBS
}

fn protected(card: &UpdateCard) -> bool {
    card_needs_you(card) || card.feed_pin || card.modified
}

/// Higher score first, then newer `created_at`, then smaller id.
fn rank_cmp(a: &(UpdateCard, f64), b: &(UpdateCard, f64)) -> std::cmp::Ordering {
    b.1.total_cmp(&a.1)
        .then(b.0.created_at.cmp(&a.0.created_at))
        .then(a.0.id.cmp(&b.0.id))
}

/// Home event cards after hide, Less-mute, and the failure floor.
/// Failures, pinned cards, and cards the user worked on are never folded.
/// At most [`FEED_PAINT_MAX`] cards stay in the deck.
pub fn rank_home_events(
    cards: &[UpdateCard],
    pulse: &FeedPulse,
    prefs: &CardPrefs,
    now: u64,
) -> HomeRank {
    let mut prelim: Vec<(UpdateCard, f64)> = visible_updates(cards)
        .into_iter()
        .filter(|card| surfaces_on_home(card, pulse, now))
        .map(|card| {
            let score = card_score(&card, prefs, now, false);
            (card, score)
        })
        .collect();
    prelim.sort_by(rank_cmp);
    let novelty_id = prelim
        .iter()
        .find(|(card, _)| novel(prefs, card))
        .map(|(card, _)| card.id.clone());
    if let Some(id) = &novelty_id {
        if let Some(item) = prelim.iter_mut().find(|(card, _)| &card.id == id) {
            item.1 += NOVELTY_BONUS;
        }
    }
    prelim.sort_by(rank_cmp);

    let mut pool = Vec::new();
    let mut folded_scored = Vec::new();
    for item in prelim {
        if protected(&item.0) || item.1 >= FOLD_BELOW {
            pool.push(item);
        } else {
            folded_scored.push(item);
        }
    }
    pool.sort_by(rank_cmp);
    let mut picked = Vec::new();
    for item in pool.iter().filter(|item| protected(&item.0)) {
        picked.push(item.clone());
    }
    if picked.len() > FEED_PAINT_MAX {
        picked.sort_by(rank_cmp);
        picked.truncate(FEED_PAINT_MAX);
    }
    for item in pool.iter().filter(|item| !protected(&item.0)) {
        if picked.len() >= FEED_PAINT_MAX {
            break;
        }
        picked.push(item.clone());
    }
    picked.sort_by(rank_cmp);
    folded_scored.sort_by(rank_cmp);
    HomeRank {
        deck: picked.into_iter().map(|(card, _)| card).collect(),
        folded: folded_scored.into_iter().map(|(card, _)| card).collect(),
        novelty_id,
    }
}

fn ranked_learned(bucket: LearnedBucket, map: &BTreeMap<String, Weight>, now: u64) -> Vec<(f64, LearnedEntry)> {
    let mut rows = Vec::new();
    for (key, weight) in map {
        let (pos, neg) = decayed_weight(weight, now);
        let score = pos - neg;
        if score == 0.0 {
            continue;
        }
        rows.push((
            score,
            LearnedEntry {
                bucket,
                key: key.clone(),
                sign: if score > 0.0 { 1 } else { -1 },
            },
        ));
    }
    rows
}

/// Up to five liked and five disliked groups and topics. Kinds stay out of the list.
pub fn top_learned(prefs: &CardPrefs, now: u64) -> (Vec<LearnedEntry>, Vec<LearnedEntry>) {
    let mut rows = ranked_learned(LearnedBucket::Group, &prefs.groups, now);
    rows.extend(ranked_learned(LearnedBucket::Topic, &prefs.topics, now));
    let mut liked: Vec<(f64, LearnedEntry)> = rows.iter().filter(|(score, _)| *score > 0.0).cloned().collect();
    let mut disliked: Vec<(f64, LearnedEntry)> = rows.iter().filter(|(score, _)| *score < 0.0).cloned().collect();
    liked.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.key.cmp(&b.1.key)));
    disliked.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.key.cmp(&b.1.key)));
    liked.truncate(5);
    disliked.truncate(5);
    (
        liked.into_iter().map(|(_, row)| row).collect(),
        disliked.into_iter().map(|(_, row)| row).collect(),
    )
}

pub fn forget_learned(prefs: &mut CardPrefs, bucket: LearnedBucket, key: &str) {
    match bucket {
        LearnedBucket::Group => {
            prefs.groups.remove(key);
        }
        LearnedBucket::Topic => {
            prefs.topics.remove(key);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::card_signals::{append_signal, signal_for};
    use crate::update_feed::automation_done_card;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir(label: &str) -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!(
            "grokhub-prefs-{label}-{}-{nanos}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn weight_of<'a>(map: &'a BTreeMap<String, Weight>, key: &str) -> &'a Weight {
        map.get(key).unwrap_or_else(|| panic!("missing {key}"))
    }

    #[test]
    fn scores_are_deterministic_for_a_fixed_event_history() {
        let card = automation_done_card("loop-7", "Host snapshot report", "body stays out", 5_000);
        let other = automation_done_card("loop-8", "Ledger digest report", "other body", 4_000);
        let play = || {
            let mut prefs = CardPrefs::default();
            apply_card_event(&mut prefs, &card, CardEvent::Opened, None, 5_000);
            apply_card_event(&mut prefs, &card, CardEvent::Dismissed, Some(true), 5_100);
            apply_card_event(&mut prefs, &other, CardEvent::Less, None, 5_200);
            apply_card_event(&mut prefs, &card, CardEvent::More, None, 5_300);
            apply_card_event(&mut prefs, &card, CardEvent::FollowUp, None, 6_000);
            prefs
        };
        let first = play();
        let second = play();
        assert_eq!(card_prefs_json(&first).unwrap(), card_prefs_json(&second).unwrap());
        let now = 6_000;
        assert_eq!(
            card_score(&card, &first, now, false).to_bits(),
            card_score(&card, &second, now, false).to_bits()
        );
        assert_eq!(
            card_score(&other, &first, now, true).to_bits(),
            card_score(&other, &second, now, true).to_bits()
        );
    }

    #[test]
    fn weights_halve_every_fourteen_days() {
        let card = automation_done_card("loop-7", "Host snapshot report", "body", 0);
        let mut prefs = CardPrefs::default();
        apply_card_event(&mut prefs, &card, CardEvent::Opened, None, 0);
        apply_card_event(&mut prefs, &card, CardEvent::Dismissed, Some(false), 0);
        let group = signal_group(&card);
        let before = weight_of(&prefs.groups, &group);
        assert!((before.pos - 1.0).abs() < 1e-9);
        assert!((before.neg - 1.0).abs() < 1e-9);
        let later = 14 * 24 * 60 * 60 * 1000;
        apply_card_event(&mut prefs, &card, CardEvent::FollowUp, None, later);
        let after = weight_of(&prefs.groups, &group);
        assert!((after.pos - 0.5).abs() < 1e-9, "pos {}", after.pos);
        assert!((after.neg - 0.5).abs() < 1e-9, "neg {}", after.neg);
        assert_eq!(after.n, 3);
        let (pos, neg) = decayed_weight(after, later + 14 * 24 * 60 * 60 * 1000);
        assert!((pos - 0.25).abs() < 1e-9, "{pos}");
        assert!((neg - 0.25).abs() < 1e-9, "{neg}");
    }

    #[test]
    fn dismiss_after_open_stays_neutral() {
        let card = automation_done_card("loop-7", "Host snapshot report", "body", 10);
        let mut opened = CardPrefs::default();
        apply_card_event(&mut opened, &card, CardEvent::Opened, None, 10);
        let mut neutral = opened.clone();
        apply_card_event(&mut neutral, &card, CardEvent::Dismissed, Some(true), 10);
        let group = signal_group(&card);
        let kept = weight_of(&neutral.groups, &group);
        let base = weight_of(&opened.groups, &group);
        assert!((kept.pos - base.pos).abs() < 1e-12);
        assert!((kept.neg - base.neg).abs() < 1e-12);
        assert_eq!(kept.neg, 0.0);
        assert_eq!(kept.n, base.n + 1);

        let mut disliked = CardPrefs::default();
        apply_card_event(&mut disliked, &card, CardEvent::Dismissed, Some(false), 10);
        let down = weight_of(&disliked.groups, &group);
        assert_eq!(down.pos, 0.0);
        assert!((down.neg - 1.0).abs() < 1e-12);
        assert_eq!(down.n, 1);
    }

    #[test]
    fn topic_keywords_are_three_title_words_without_digits_or_times() {
        let words = topic_keywords("Run a 12:30 host snapshot 42 please ZephyrFourth");
        assert_eq!(words, vec!["run".to_string(), "host".to_string(), "snapshot".to_string()]);
        assert!(!words.iter().any(|word| word.chars().any(|c| c.is_ascii_digit())));
        assert!(!words.iter().any(|word| word.contains(':')));
        assert!(words.len() <= 3);
        assert!(!words.iter().any(|word| word == "please" || word == "zephyrfourth"));
    }

    #[test]
    fn prefs_cap_drops_the_weakest_entries() {
        let mut prefs = CardPrefs::default();
        let now = 1_000u64;
        for i in 0..TOPIC_CAP {
            prefs.topics.insert(
                format!("topic{i:03}"),
                Weight {
                    pos: i as f64,
                    neg: 0.0,
                    n: 1,
                    at: now,
                },
            );
        }
        let fresh = automation_done_card("src", "Zebrakeyword", "body", now);
        apply_card_event(&mut prefs, &fresh, CardEvent::Opened, None, now);
        assert_eq!(prefs.topics.len(), TOPIC_CAP);
        assert!(prefs.topics.contains_key("zebrakeyword"), "a stronger new keyword stays");
        assert!(!prefs.topics.contains_key("topic000"), "the weakest topic is dropped");

        for i in 0..GROUP_CAP {
            prefs.groups.insert(
                format!("run:keep{i:03}"),
                Weight {
                    pos: 10.0,
                    neg: 0.0,
                    n: 1,
                    at: now,
                },
            );
        }
        let weak = automation_done_card("weak-new", "Quiet ledger report", "body", now);
        apply_card_event(&mut prefs, &weak, CardEvent::FollowUp, None, now);
        assert_eq!(prefs.groups.len(), GROUP_CAP);
        assert!(
            !prefs.groups.contains_key("run:weak-new"),
            "a zero-weight new group loses to the stored ones"
        );
    }

    #[test]
    fn prefs_rebuild_groups_from_the_signal_log() {
        let dir = temp_dir("rebuild");
        let card = automation_done_card("alpha", "Host snapshot report", "ZephyrBodyNo", 1);
        append_signal(&dir, &signal_for(&card, CardEvent::Opened, 1_000, None));
        // Rotate the active log aside the way a full file would, then add more.
        // Same timestamp so the half-life factor stays 1 and the counts stay exact.
        fs::rename(dir.join("card_signals.jsonl"), dir.join("card_signals.jsonl.1")).unwrap();
        append_signal(&dir, &signal_for(&card, CardEvent::Dismissed, 1_000, Some(false)));
        append_signal(&dir, &signal_for(&card, CardEvent::Hidden, 1_000, None));
        append_signal(&dir, &signal_for(&card, CardEvent::Unhidden, 1_000, None));
        fs::write(dir.join("card_prefs.json"), b"not-json").unwrap();

        let prefs = load_card_prefs(&dir);
        assert!(prefs.topics.is_empty(), "the signal log has no topics");
        let kind = weight_of(&prefs.kinds, "automation_done");
        assert!((kind.pos - 1.0).abs() < 1e-9, "kind pos {} neg {} n {}", kind.pos, kind.neg, kind.n);
        assert!((kind.neg - 1.0).abs() < 1e-9, "dismiss without open is negative");
        let group = weight_of(&prefs.groups, "run:alpha");
        assert!((group.pos - 1.0).abs() < 1e-9);
        assert!(
            (group.neg - 1.0).abs() < 1e-9,
            "dismiss leaves 1, hidden adds 3, unhidden removes that 3, neg={}",
            group.neg
        );
        assert!(!card_prefs_json(&prefs).unwrap().contains("ZephyrBodyNo"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn prefs_file_holds_only_keywords_and_numbers() {
        let card_title = "ZephyrTitleAlpha host snapshot 09:15";
        let mut card = automation_done_card("src-plain", card_title, "placeholder", 7);
        card.body = Some("ZephyrBodyOmega".into());
        card.why = Some("ZephyrWhyOmega".into());
        card.prompt = Some("ZephyrPromptOmega".into());
        card.details = Some("ZephyrDetailsOmega".into());
        let mut prefs = CardPrefs::default();
        apply_card_event(&mut prefs, &card, CardEvent::More, None, 9);
        let text = card_prefs_json(&prefs).unwrap();
        for secret in [
            "ZephyrBodyOmega",
            "ZephyrWhyOmega",
            "ZephyrPromptOmega",
            "ZephyrDetailsOmega",
            "placeholder",
            "09:15",
        ] {
            assert!(!text.contains(secret), "{secret} leaked into {text}");
        }
        assert!(text.contains("zephyrtitlealpha"), "{text}");
        assert!(text.contains("host"), "{text}");
        assert!(text.contains("snapshot"), "{text}");
        assert!(!text.contains("zephyrtitlealpha host"), "{text}");
        let value: serde_json::Value = serde_json::from_str(&text).unwrap();
        fn leaves_are_numbers(value: &serde_json::Value) -> bool {
            match value {
                serde_json::Value::Number(_) => true,
                serde_json::Value::Object(map) => map.values().all(leaves_are_numbers),
                serde_json::Value::Array(items) => items.iter().all(leaves_are_numbers),
                _ => false,
            }
        }
        assert!(leaves_are_numbers(&value), "prefs hold numbers, not text values: {text}");
        let obj = value.as_object().unwrap();
        for key in obj.keys() {
            assert!(matches!(key.as_str(), "kinds" | "groups" | "topics"), "{key}");
        }
        for bucket in ["kinds", "groups", "topics"] {
            let Some(map) = obj.get(bucket).and_then(|v| v.as_object()) else {
                continue;
            };
            for (feature, weight) in map {
                assert!(!feature.contains(' '), "feature keys are single tokens: {feature}");
                let fields = weight.as_object().unwrap();
                for field in fields.keys() {
                    assert!(matches!(field.as_str(), "pos" | "neg" | "n" | "at"), "{field}");
                }
            }
        }
    }
}
