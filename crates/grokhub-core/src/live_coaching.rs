//! Round 3 item 37: no live coaching during a match whose publisher bars it.
//!
//! Riot's third-party policy bans apps "drawing conclusions for you during
//! gameplay". While a game from [`crate::game_profile::MATCH_RULES`] is in a
//! match, the router answers a gameplay-advice ask with [`refusal`] instead
//! of sending it. Only advice is held: settings and performance help, and a
//! look back at a match that ended, still go through, and so does anything
//! that isn't about playing.
//!
//! [`gameplay_advice`] is deliberately narrow, so it blocks as little as it
//! can: the ask has to be a request (a question form or "tips", "best",
//! "counter") and name a gameplay subject (a build, a lane, a site, a
//! fight). A word about settings or a finished match lets it through.

use crate::game_profile::LiveMatch;

/// Openings and words that make a message a request for advice.
const ASKS: &[&str] = &[
    "how do i",
    "how should i",
    "how can i",
    "how to",
    "what should i",
    "what do i",
    "what's the best",
    "whats the best",
    "which",
    "should i",
    "where should",
    "where do i",
    "when should",
    "when do i",
    "tell me",
    "give me",
    "help me",
    "best",
    "tip",
    "tips",
    "strat",
    "strats",
    "strategy",
    "counter",
    "advice",
];

/// Gameplay subjects across Riot's games. Whole words.
const PLAY: &[&str] = &[
    // Items and loadouts.
    "build",
    "item",
    "items",
    "buy",
    "rune",
    "runes",
    "augment",
    "augments",
    "eco",
    "force",
    // Map and objectives.
    "lane",
    "laning",
    "gank",
    "jungle",
    "jungler",
    "dragon",
    "drake",
    "baron",
    "herald",
    "ward",
    "objective",
    "rotate",
    "rotation",
    "push",
    "split",
    "site",
    "plant",
    "defuse",
    "retake",
    "angle",
    "peek",
    // Fights and matchups.
    "fight",
    "teamfight",
    "engage",
    "trade",
    "duel",
    "combo",
    "beat",
    "counter",
    "matchup",
    "enemy",
    "enemies",
    "comp",
    "carry",
    "win",
    "round",
    "ult",
    "ultimate",
    "ability",
    "abilities",
    "utility",
    "smoke",
    "smokes",
    "flash",
];

/// Words that make an ask about settings, the PC or a match that ended.
const EXEMPT: &[&str] = &[
    "setting",
    "settings",
    "sensitivity",
    "sens",
    "dpi",
    "fps",
    "graphics",
    "keybind",
    "keybinds",
    "crosshair",
    "resolution",
    "driver",
    "drivers",
    "lag",
    "stutter",
    "latency",
    "audio",
    "mic",
    "review",
    "replay",
    "vod",
    "recap",
    "stats",
    "postgame",
    "post-game",
    "last game",
    "last match",
    "that game",
    "that match",
    "after the game",
    "after the match",
    "patch notes",
];

fn words(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|c: char| !(c.is_alphanumeric() || c == '\'' || c == '-'))
        .filter(|w| !w.is_empty())
        .map(str::to_string)
        .collect()
}

/// `phrase` (one or more words) appears in `words` as whole words.
fn has(words: &[String], phrase: &str) -> bool {
    let want: Vec<&str> = phrase.split(' ').collect();
    words
        .windows(want.len())
        .any(|w| w.iter().zip(&want).all(|(a, b)| a == b))
}

/// Whether `text` asks for gameplay advice.
pub fn gameplay_advice(text: &str) -> bool {
    let w = words(text);
    if w.is_empty() || EXEMPT.iter().any(|p| has(&w, p)) {
        return false;
    }
    let asks = text.trim_end().ends_with('?') || ASKS.iter().any(|p| has(&w, p));
    asks && PLAY.iter().any(|p| has(&w, p))
}

/// The one line sent instead of advice during `m`.
pub fn refusal(m: &LiveMatch) -> String {
    format!(
        "{} doesn't allow live coaching during a {} match, so I'll hold gameplay tips until it ends. Settings help and a look back at the match are fine anytime.",
        m.rule.publisher, m.game
    )
}

/// The refusal for `text` when it asks for advice and a barred match is up.
/// Checks the words first, so most messages never list processes.
pub fn hold(text: &str) -> Option<String> {
    hold_during(text, crate::game_profile::live_match_now)
}

/// [`hold`] with the live match read by `live`.
pub fn hold_during(text: &str, live: impl FnOnce() -> Option<LiveMatch>) -> Option<String> {
    if !gameplay_advice(text) {
        return None;
    }
    live().map(|m| refusal(&m))
}

#[cfg(test)]
#[path = "live_coaching_tests.rs"]
mod tests;
