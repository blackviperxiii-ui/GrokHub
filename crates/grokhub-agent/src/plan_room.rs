//! How much room the SuperGrok pool has left, and whether xAI's plan gate
//! turned the sign-in away, as xAI's own replies to a signed-in call say.
//!
//! xAI publishes no subscription quota endpoint for apps. The only approved
//! signal is the rate-limit headers on `api.x.ai` replies to a Sign in with
//! Grok bearer (OpenAI-style `x-ratelimit-*` or IETF `ratelimit-*`), so this
//! reads those and nothing else. API-key replies are never noted: console
//! credits and the SuperGrok pool are separate buckets.

use std::sync::Mutex;

/// What the last signed-in reply said about the pool.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PlanRoom {
    pub requests_left: Option<u64>,
    pub requests_limit: Option<u64>,
    pub tokens_left: Option<u64>,
    pub tokens_limit: Option<u64>,
    /// Seconds from `seen_ms` until the window resets, when xAI said.
    pub reset_secs: Option<u64>,
    pub seen_ms: u64,
}

#[derive(Default)]
struct State {
    room: Option<PlanRoom>,
    /// When xAI's plan gate last refused the sign-in, with no OK reply since.
    refused_ms: Option<u64>,
}

static STATE: Mutex<State> = Mutex::new(State {
    room: None,
    refused_ms: None,
});

fn state() -> std::sync::MutexGuard<'static, State> {
    STATE.lock().unwrap_or_else(|e| e.into_inner())
}

/// A room line older than this is not shown: the pool may have moved since.
pub const FRESH_MS: u64 = 60 * 60 * 1000;

/// xAI's own wording when a valid sign-in's plan can't take the call: the
/// tier doesn't include app (API) use, or the pool is used up. A 401, or
/// "could not be validated", is a dead token instead and still means sign in again.
pub fn plan_refusal(status: u16, message: &str) -> bool {
    if status != 403 {
        return false;
    }
    let m = message.to_ascii_lowercase();
    m.contains("active grok subscription")
        || m.contains("run out of available resources")
        || m.contains("subscription tier")
}

fn num(v: Option<String>) -> Option<u64> {
    let v = v?;
    // IETF `ratelimit-*` may carry a list ("100, 100;w=60"): the first item.
    let first = v.split([',', ';']).next()?.trim();
    first.parse::<u64>().ok()
}

/// "1s", "6m0s", "1h2m3.5s", "250ms" (OpenAI style) or plain seconds.
/// A plain number past 1e9 is a Unix time and is turned into seconds from now.
fn reset_secs(v: Option<String>, now_ms: u64) -> Option<u64> {
    let v = v?;
    let t = v.split([',', ';']).next()?.trim().to_ascii_lowercase();
    if let Ok(n) = t.parse::<u64>() {
        let now_s = now_ms / 1000;
        return Some(if n > 1_000_000_000 {
            n.saturating_sub(now_s)
        } else {
            n
        });
    }
    let mut total = 0f64;
    let mut digits = String::new();
    let mut chars = t.chars().peekable();
    let mut any = false;
    while let Some(c) = chars.next() {
        if c.is_ascii_digit() || c == '.' {
            digits.push(c);
            continue;
        }
        let n: f64 = digits.parse().ok()?;
        digits.clear();
        let mult = match c {
            'h' => 3600.0,
            'm' if chars.peek() == Some(&'s') => {
                chars.next();
                0.001
            }
            'm' => 60.0,
            's' => 1.0,
            _ => return None,
        };
        total += n * mult;
        any = true;
    }
    if !digits.is_empty() || !any {
        return None;
    }
    Some(total.ceil() as u64)
}

/// Read the room from a reply's headers. `None` when xAI sent none.
pub fn from_headers(get: &dyn Fn(&str) -> Option<String>, now_ms: u64) -> Option<PlanRoom> {
    let pick = |names: &[&str]| names.iter().find_map(|n| get(n));
    let room = PlanRoom {
        requests_left: num(pick(&[
            "x-ratelimit-remaining-requests",
            "ratelimit-remaining",
        ])),
        requests_limit: num(pick(&["x-ratelimit-limit-requests", "ratelimit-limit"])),
        tokens_left: num(pick(&["x-ratelimit-remaining-tokens"])),
        tokens_limit: num(pick(&["x-ratelimit-limit-tokens"])),
        reset_secs: reset_secs(
            pick(&[
                "x-ratelimit-reset-requests",
                "x-ratelimit-reset-tokens",
                "ratelimit-reset",
            ]),
            now_ms,
        ),
        seen_ms: now_ms,
    };
    let said = room.requests_left.is_some() || room.tokens_left.is_some();
    said.then_some(room)
}

/// A signed-in reply came back. Its headers update the room, and any reply
/// below 400 means the plan gate let the sign-in through.
pub fn note_reply(status: u16, get: &dyn Fn(&str) -> Option<String>, now_ms: u64) {
    let room = from_headers(get, now_ms);
    let mut s = state();
    if let Some(room) = room {
        s.room = Some(room);
    }
    if status < 400 {
        s.refused_ms = None;
    }
}

/// xAI's plan gate refused the sign-in ([`plan_refusal`]).
pub fn note_refused(now_ms: u64) {
    state().refused_ms = Some(now_ms);
}

/// The last room xAI reported, if any.
pub fn latest() -> Option<PlanRoom> {
    state().room.clone()
}

/// When the plan gate last refused the sign-in, with no OK reply since.
pub fn refused_since() -> Option<u64> {
    state().refused_ms
}

fn grouped(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

fn left_of(left: u64, limit: Option<u64>, what: &str) -> String {
    match limit {
        Some(l) if l >= left => format!("{} of {} {what} left", grouped(left), grouped(l)),
        _ => format!("{} {what} left", grouped(left)),
    }
}

/// Settings → Account's line, or `None` when xAI hasn't said or the reading
/// is over an hour old.
pub fn room_line(room: &PlanRoom, now_ms: u64) -> Option<String> {
    if now_ms.saturating_sub(room.seen_ms) > FRESH_MS {
        return None;
    }
    let mut line = match (room.requests_left, room.tokens_left) {
        (Some(r), _) => left_of(r, room.requests_limit, "requests"),
        (None, Some(t)) => left_of(t, room.tokens_limit, "tokens"),
        (None, None) => return None,
    };
    if let Some(secs) = room.reset_secs {
        let since = now_ms.saturating_sub(room.seen_ms) / 1000;
        let left = secs.saturating_sub(since);
        if left > 0 {
            let when = if left < 60 {
                "under a minute".to_string()
            } else if left < 3600 {
                format!("{} min", left.div_ceil(60))
            } else {
                format!("{} h", left.div_ceil(3600))
            };
            line.push_str(&format!(", resets in {when}"));
        }
    }
    Some(format!("SuperGrok room: {line}"))
}

#[cfg(test)]
pub(crate) fn reset_for_test() {
    let mut s = state();
    s.room = None;
    s.refused_ms = None;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(pairs: &'static [(&'static str, &'static str)]) -> impl Fn(&str) -> Option<String> {
        move |n: &str| {
            pairs
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(n))
                .map(|(_, v)| v.to_string())
        }
    }

    #[test]
    fn plan_refusal_is_xais_tier_wording_on_403_only() {
        let tier = "The caller does not have permission to execute the specified operation. You have either run out of available resources or do not have an active Grok subscription.";
        assert!(plan_refusal(403, tier));
        assert!(!plan_refusal(401, tier));
        assert!(!plan_refusal(
            403,
            "The OAuth2 access token could not be validated."
        ));
        assert!(!plan_refusal(403, "forbidden"));
    }

    #[test]
    fn openai_style_headers_read() {
        let get = headers(&[
            ("x-ratelimit-remaining-requests", "1840"),
            ("x-ratelimit-limit-requests", "2000"),
            ("x-ratelimit-remaining-tokens", "950000"),
            ("x-ratelimit-reset-requests", "6m0s"),
        ]);
        let room = from_headers(&get, 1_000).unwrap();
        assert_eq!(room.requests_left, Some(1840));
        assert_eq!(room.requests_limit, Some(2000));
        assert_eq!(room.tokens_left, Some(950_000));
        assert_eq!(room.reset_secs, Some(360));
        assert_eq!(
            room_line(&room, 1_000).as_deref(),
            Some("SuperGrok room: 1,840 of 2,000 requests left, resets in 6 min")
        );
    }

    #[test]
    fn ietf_headers_and_unix_reset_read() {
        let get = headers(&[
            ("ratelimit-remaining", "12, 12;w=60"),
            ("ratelimit-limit", "100"),
            ("ratelimit-reset", "2000000030"),
        ]);
        let room = from_headers(&get, 2_000_000_000_000).unwrap();
        assert_eq!(room.requests_left, Some(12));
        assert_eq!(room.reset_secs, Some(30));
        assert_eq!(
            room_line(&room, 2_000_000_000_000).as_deref(),
            Some("SuperGrok room: 12 of 100 requests left, resets in under a minute")
        );
    }

    #[test]
    fn no_headers_no_room_and_stale_room_hidden() {
        assert!(from_headers(&headers(&[]), 5).is_none());
        let room = PlanRoom {
            tokens_left: Some(5),
            seen_ms: 0,
            ..Default::default()
        };
        assert_eq!(
            room_line(&room, 10).as_deref(),
            Some("SuperGrok room: 5 tokens left")
        );
        assert!(room_line(&room, FRESH_MS + 1).is_none());
    }

    #[test]
    fn reset_wording_parses_mixed_units() {
        assert_eq!(reset_secs(Some("1h2m3.5s".into()), 0), Some(3724));
        assert_eq!(reset_secs(Some("250ms".into()), 0), Some(1));
        assert_eq!(reset_secs(Some("soon".into()), 0), None);
    }

    #[test]
    fn ok_reply_clears_refusal_and_keeps_last_room() {
        reset_for_test();
        note_refused(7);
        assert_eq!(refused_since(), Some(7));
        note_reply(403, &headers(&[]), 8);
        assert_eq!(refused_since(), Some(7));
        note_reply(200, &headers(&[("x-ratelimit-remaining-requests", "3")]), 9);
        assert_eq!(refused_since(), None);
        assert_eq!(latest().and_then(|r| r.requests_left), Some(3));
        note_reply(200, &headers(&[]), 10);
        assert_eq!(latest().and_then(|r| r.requests_left), Some(3));
        reset_for_test();
    }
}
