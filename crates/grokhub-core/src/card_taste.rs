//! Card taste: what the user likes and doesn't, learned from Like and
//! I don't like on Pulse cards. Dismiss is not a signal and never lands here.
//!
//! Every card carries a few generic traits (`kind:value`, such as
//! `topic:<x>`, `subject:<x>`, `tone:<x>`, `source:<x>`). One reaction says
//! little about which trait caused it, so the weights are refit from all
//! reactions together: a small regularised logistic fit. A trait seen on
//! liked and disliked cards alike stays near even, and the blame for a
//! dislike lands on the trait only the disliked cards share. The fit starts
//! every weight at zero and pulls it back toward zero, so one or two clicks
//! move a little and repeated clicks move more.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

use crate::update_feed::UpdateCard;

pub const FILE_NAME: &str = "card-taste.json";
/// Reactions kept; the oldest go first.
pub const EVENTS_MAX: usize = 400;
/// Traits read from one card.
pub const TRAITS_MAX: usize = 16;
/// Pull toward zero: higher means each click counts for less.
const L2: f64 = 1.0;
const STEPS: usize = 300;
const RATE: f64 = 0.2;
/// A trait at or below this has left the user's preferences.
pub const AVOID_AT: f64 = -0.2;
/// A trait at or above this is one they like.
pub const LIKE_AT: f64 = 0.2;
/// A card scoring at or below this is held off the page.
pub const HIDE_AT: f64 = -1.5;
/// How far a point of score moves a card on the page, in ms (6 hours).
pub const RANK_MS_PER_POINT: f64 = 6.0 * 3_600_000.0;
/// The most a score moves a card, in points.
const RANK_CAP: f64 = 3.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Reaction {
    Like,
    Dislike,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Event {
    pub card_id: String,
    pub traits: Vec<String>,
    pub reaction: Reaction,
    pub at_ms: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Taste {
    #[serde(default)]
    pub events: Vec<Event>,
}

/// What the fit says, for the writer and for "why".
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Preferences {
    /// Best first.
    pub liked: Vec<(String, f64)>,
    /// Worst first. These have left the user's preferences.
    pub avoided: Vec<(String, f64)>,
}

/// One trait in canonical form: `kind:value`, lowercase, single-spaced.
/// None when either side is empty.
pub fn trait_key(kind: &str, value: &str) -> Option<String> {
    let norm = |s: &str| {
        s.split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .to_lowercase()
    };
    let (kind, value) = (norm(kind), norm(value));
    (!kind.is_empty() && !value.is_empty() && !kind.contains(':'))
        .then(|| format!("{kind}:{}", value.chars().take(48).collect::<String>()))
}

/// Parse `kind:value` (as a writer tags it) into canonical form.
pub fn parse_trait(tag: &str) -> Option<String> {
    let (kind, value) = tag.split_once(':')?;
    trait_key(kind, value)
}

/// The card's traits: its own tags first, then what every card has (its
/// category, source and kind).
pub fn card_traits(card: &UpdateCard) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut push = |t: Option<String>| {
        if let Some(t) = t {
            if !out.contains(&t) && out.len() < TRAITS_MAX {
                out.push(t);
            }
        }
    };
    for tag in &card.pulse.traits {
        push(parse_trait(tag));
    }
    if let Some(cat) = &card.pulse.category {
        push(trait_key("category", cat));
    }
    if let Some((category, tag)) = crate::reactions::card_category_tag(card) {
        push(trait_key("category", &category));
        push(trait_key("source", &tag));
    }
    if let Some(src) = &card.pulse.source_name {
        push(trait_key("source", src));
    }
    if let Some(p) = &card.pulse.proactive {
        push(trait_key("topic", &p.topic));
    }
    push(trait_key("kind", &format!("{:?}", card.kind)));
    out
}

fn sigmoid(x: f64) -> f64 {
    1.0 / (1.0 + (-x).exp())
}

impl Taste {
    /// Note a reaction. Dismiss never calls this.
    pub fn record(&mut self, card_id: &str, traits: Vec<String>, reaction: Reaction, now_ms: u64) {
        if traits.is_empty() {
            return;
        }
        self.events.push(Event {
            card_id: card_id.to_string(),
            traits,
            reaction,
            at_ms: now_ms,
        });
        if self.events.len() > EVENTS_MAX {
            let extra = self.events.len() - EVENTS_MAX;
            self.events.drain(..extra);
        }
    }

    /// Take back the latest reaction of this kind on this card (Like pressed
    /// again). True when one was removed.
    pub fn undo(&mut self, card_id: &str, reaction: Reaction) -> bool {
        match self
            .events
            .iter()
            .rposition(|e| e.card_id == card_id && e.reaction == reaction)
        {
            Some(i) => {
                self.events.remove(i);
                true
            }
            None => false,
        }
    }

    /// Every trait's weight, refit from all reactions. Deterministic.
    pub fn weights(&self) -> BTreeMap<String, f64> {
        let mut w: BTreeMap<String, f64> = BTreeMap::new();
        for e in &self.events {
            for t in &e.traits {
                w.entry(t.clone()).or_insert(0.0);
            }
        }
        if w.is_empty() {
            return w;
        }
        for _ in 0..STEPS {
            let mut grad: BTreeMap<String, f64> = w.keys().map(|k| (k.clone(), 0.0)).collect();
            for e in &self.events {
                let y = match e.reaction {
                    Reaction::Like => 1.0,
                    Reaction::Dislike => -1.0,
                };
                let margin: f64 = e.traits.iter().map(|t| w[t]).sum();
                // d/dw of log(1 + exp(-y·margin)).
                let g = -y * sigmoid(-y * margin);
                for t in &e.traits {
                    if let Some(v) = grad.get_mut(t) {
                        *v += g;
                    }
                }
            }
            for (k, v) in w.iter_mut() {
                *v -= RATE * (grad[k] + L2 * *v);
            }
        }
        w
    }

    /// The fit as liked and avoided traits. Card plumbing (kind, source,
    /// category) stays in the score but only tagged traits are listed.
    pub fn preferences(&self) -> Preferences {
        let mut liked = Vec::new();
        let mut avoided = Vec::new();
        for (t, v) in self.weights() {
            if v >= LIKE_AT {
                liked.push((t, v));
            } else if v <= AVOID_AT {
                avoided.push((t, v));
            }
        }
        liked.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
        avoided.sort_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)));
        Preferences { liked, avoided }
    }

    /// A card's score from fitted `weights`: the sum over its traits.
    pub fn score(weights: &BTreeMap<String, f64>, traits: &[String]) -> f64 {
        traits.iter().filter_map(|t| weights.get(t)).sum()
    }
}

/// Where a card sorts on the page: its time, moved by its score.
pub fn rank_time(created_at: u64, score: f64) -> i64 {
    created_at as i64 + (score.clamp(-RANK_CAP, RANK_CAP) * RANK_MS_PER_POINT) as i64
}

/// Order feed cards by taste: liked kinds rise, disliked kinds sink, and
/// cards at or below [`HIDE_AT`] leave the page.
pub fn rank(cards: Vec<UpdateCard>, taste: &Taste) -> Vec<UpdateCard> {
    rank_with(cards, &taste.weights())
}

/// [`rank`] with weights already fitted. No weights: the order is kept.
pub fn rank_with(cards: Vec<UpdateCard>, weights: &BTreeMap<String, f64>) -> Vec<UpdateCard> {
    if weights.is_empty() {
        return cards;
    }
    let mut scored: Vec<(i64, UpdateCard)> = cards
        .into_iter()
        .filter_map(|c| {
            let s = Taste::score(weights, &card_traits(&c));
            (s > HIDE_AT).then(|| (rank_time(c.created_at, s), c))
        })
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.id.cmp(&b.1.id)));
    scored.into_iter().map(|(_, c)| c).collect()
}

/// One line for the post writer: "More of: …. Less of: …." Empty with no signal.
pub fn writer_line(prefs: &Preferences) -> String {
    let words = |v: &[(String, f64)]| {
        v.iter()
            .filter(|(t, _)| !t.starts_with("kind:"))
            .take(8)
            .map(|(t, _)| t.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    };
    let (more, less) = (words(&prefs.liked), words(&prefs.avoided));
    let mut out = String::new();
    if !more.is_empty() {
        out.push_str(&format!("More of: {more}."));
    }
    if !less.is_empty() {
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(&format!("Less of: {less}."));
    }
    out
}

type Cached = (
    PathBuf,
    Option<std::time::SystemTime>,
    u64,
    Arc<BTreeMap<String, f64>>,
);

static WEIGHTS: Mutex<Option<Cached>> = Mutex::new(None);

/// The fitted weights for the taste on disk, refit only when the file
/// changes. Cheap enough to call every frame.
pub fn cached_weights(config_dir: &Path) -> Arc<BTreeMap<String, f64>> {
    let file = path(config_dir);
    let meta = std::fs::metadata(&file).ok();
    let stamp = (
        meta.as_ref().and_then(|m| m.modified().ok()),
        meta.map_or(0, |m| m.len()),
    );
    let mut guard = WEIGHTS.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((p, at, len, w)) = guard.as_ref() {
        if *p == file && (*at, *len) == stamp {
            return Arc::clone(w);
        }
    }
    let w = Arc::new(load(config_dir).weights());
    *guard = Some((file, stamp.0, stamp.1, Arc::clone(&w)));
    w
}

pub fn path(config_dir: &Path) -> PathBuf {
    config_dir.join(FILE_NAME)
}

pub fn load(config_dir: &Path) -> Taste {
    std::fs::read_to_string(path(config_dir))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save(config_dir: &Path, taste: &Taste) -> Result<(), String> {
    let text = serde_json::to_string(taste).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(config_dir).map_err(|e| e.to_string())?;
    crate::pulse_writer::write_atomic(&path(config_dir), &text)
}

#[cfg(test)]
#[path = "card_taste_tests.rs"]
mod tests;
