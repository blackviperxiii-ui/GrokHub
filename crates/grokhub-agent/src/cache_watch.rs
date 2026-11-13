//! Prompt-cache share. xAI reports how much of each prompt it served from its
//! cache; a cached token costs a fraction of a fresh one. This sums that share
//! for Home's AI usage tile, and spots a turn whose share fell under
//! the floor right after one above it (a broken prefix), so the turn's usage
//! line can say so.

use crate::route::log::{RouteRecord, CACHE_FLOOR_PCT};
use crate::route::PROVIDER_XAI;

/// Under this share, a chat pays for prompt it should have had cached.
pub const FLOOR_PCT: u8 = CACHE_FLOOR_PCT as u8;
/// A turn with less fresh-plus-cached input than this says nothing about the cache.
pub const MIN_TURN_INPUT: u64 = 2_000;
/// The window the usage tile sums over.
pub const WINDOW_MS: u64 = 24 * 60 * 60 * 1_000;

/// Cached share of the prompt tokens xAI calls sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Share {
    /// Whole percent, 0 to 100.
    pub pct: u8,
    /// xAI calls that reported input.
    pub calls: usize,
}

/// The cached share of xAI prompt tokens in `records` stamped at or after
/// `since_ms`. `None` when no such call reported any input. Other providers
/// are left out: the local model never reports a cache.
pub fn share_since(records: &[(u64, RouteRecord)], since_ms: u64) -> Option<Share> {
    let (mut cached, mut input, mut calls) = (0u64, 0u64, 0usize);
    for (ts, rec) in records {
        if *ts < since_ms || rec.used.provider != PROVIDER_XAI {
            continue;
        }
        let Some(out) = &rec.outcome else {
            continue;
        };
        if out.tokens.input == 0 {
            continue;
        }
        cached = cached.saturating_add(out.tokens.cached.min(out.tokens.input));
        input = input.saturating_add(out.tokens.input);
        calls += 1;
    }
    (input > 0).then(|| Share {
        pct: grokhub_core::wire::cache_pct(cached, input),
        calls,
    })
}

/// The share this turn fell from, when it dropped under [`FLOOR_PCT`] right
/// after a turn at or above it. A small turn, or one with no earlier turn,
/// never counts.
pub fn dropped_from(prev_pct: Option<u8>, now_pct: u8, turn_input: u64) -> Option<u8> {
    let prev = prev_pct?;
    (turn_input >= MIN_TURN_INPUT && prev >= FLOOR_PCT && now_pct < FLOOR_PCT).then_some(prev)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::route::log::{Chosen, RouteOutcome, RouteTokens};

    fn rec(provider: &str, input: u64, cached: u64) -> RouteRecord {
        RouteRecord {
            used: Chosen {
                provider: provider.into(),
                model: "grok-4.3".into(),
                effort: None,
            },
            outcome: Some(RouteOutcome {
                ok: true,
                tokens: RouteTokens {
                    input,
                    cached,
                    ..RouteTokens::default()
                },
                ..RouteOutcome::default()
            }),
            ..RouteRecord::default()
        }
    }

    #[test]
    fn the_share_sums_xai_calls_in_the_window_only() {
        let records = vec![
            (10, rec(PROVIDER_XAI, 10_000, 10_000)),
            (100, rec(PROVIDER_XAI, 8_000, 6_000)),
            (110, rec(PROVIDER_XAI, 2_000, 2_000)),
            (120, rec("local", 50_000, 0)),
            (130, rec(PROVIDER_XAI, 0, 0)),
        ];
        assert_eq!(
            share_since(&records, 100),
            Some(Share { pct: 80, calls: 2 })
        );
        assert_eq!(share_since(&records, 200), None);
        assert_eq!(share_since(&[(5, rec("local", 9, 0))], 0), None);
    }

    #[test]
    fn the_usage_tile_window_reads_the_last_day() {
        let now = WINDOW_MS + 1_000;
        let records = vec![
            (0, rec(PROVIDER_XAI, 10_000, 0)),
            (now - 10, rec(PROVIDER_XAI, 4_000, 3_000)),
        ];
        assert_eq!(
            share_since(&records, now - WINDOW_MS),
            Some(Share { pct: 75, calls: 1 })
        );
        assert_eq!(share_since(&[], now - WINDOW_MS), None);
    }

    #[test]
    fn a_drop_needs_a_cached_turn_before_it_and_a_real_turn_now() {
        assert_eq!(dropped_from(Some(92), 30, 9_000), Some(92));
        assert_eq!(dropped_from(None, 30, 9_000), None, "first turn");
        assert_eq!(dropped_from(Some(60), 30, 9_000), None, "never cached");
        assert_eq!(dropped_from(Some(92), 85, 9_000), None, "still cached");
        assert_eq!(dropped_from(Some(92), 0, 500), None, "tiny turn");
    }
}
