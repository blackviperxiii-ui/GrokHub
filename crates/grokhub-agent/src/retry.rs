// Portions derived from xai-org/grok-build (Apache-2.0, © SpaceXAI), commit 2bdd1d6a; modified.

use std::time::Duration;

use crate::ClientError;

pub const RATE_LIMIT_RETRY_THRESHOLD: u32 = 2;
pub const DEFAULT_MAX_RETRIES: u32 = 15;
pub const MAX_RETRY_BACKOFF: Duration = Duration::from_secs(30);
pub const TRANSPORT_REBUILD_BACKOFF: Duration = Duration::from_millis(200);

pub fn resolve_max_retries_with_env(env_override: Option<&str>, model_max_retries: Option<u32>) -> u32 {
    env_override
        .and_then(|value| value.parse::<u32>().ok())
        .or(model_max_retries)
        .unwrap_or(DEFAULT_MAX_RETRIES)
}

pub fn retry_backoff_with_jitter(retry_count: u32) -> Duration {
    let shift = retry_count.saturating_sub(1);
    let base_ms = 2000u64
        .checked_shl(shift)
        .unwrap_or(u64::MAX)
        .min(MAX_RETRY_BACKOFF.as_millis() as u64);
    jitter_backoff(Duration::from_millis(base_ms))
}

pub fn jitter_backoff(base: Duration) -> Duration {
    use std::hash::{Hash, Hasher};
    use std::sync::atomic::{AtomicU64, Ordering};

    static JITTER_SEQ: AtomicU64 = AtomicU64::new(0);

    let base_ms = base.as_millis() as u64;
    let jitter_range = base_ms / 5;
    let mut hasher = std::hash::DefaultHasher::new();
    JITTER_SEQ.fetch_add(1, Ordering::Relaxed).hash(&mut hasher);
    std::thread::current().id().hash(&mut hasher);
    let jitter = hasher.finish() % (jitter_range * 2 + 1);
    Duration::from_millis(base_ms - jitter_range + jitter)
}

pub fn retry_after_or_backoff(attempt: u32, retry_after_secs: Option<u64>) -> Duration {
    match retry_after_secs.filter(|secs| *secs > 0) {
        Some(secs) => jitter_backoff(Duration::from_secs(secs).min(MAX_RETRY_BACKOFF)),
        None => retry_backoff_with_jitter(attempt),
    }
}

/// `Some` means sleep then try again. `None` means the error is final.
/// `retry_count` is how many failures have already happened. The next attempt
/// number is `retry_count + 1`, and it retries while that number is below `max_retries`.
pub fn decide_retry(err: &ClientError, retry_count: u32, max_retries: u32) -> Option<Duration> {
    if !err.is_retryable() || max_retries == 0 {
        return None;
    }
    let next_attempt = retry_count.saturating_add(1);
    match err {
        ClientError::RateLimited { retry_after_secs, .. } => {
            if next_attempt >= max_retries.min(RATE_LIMIT_RETRY_THRESHOLD) {
                return None;
            }
            Some(match retry_after_secs.filter(|secs| *secs > 0) {
                Some(secs) => Duration::from_secs(secs),
                None => retry_backoff_with_jitter(next_attempt),
            })
        }
        ClientError::Transport(_) | ClientError::Disconnect | ClientError::Server { .. } => {
            if next_attempt >= max_retries {
                return None;
            }
            if next_attempt == 1 {
                let backoff = match err {
                    ClientError::Transport(_) => jitter_backoff(TRANSPORT_REBUILD_BACKOFF),
                    _ => retry_after_or_backoff(next_attempt, err.retry_after_secs()),
                };
                return Some(backoff);
            }
            Some(retry_after_or_backoff(next_attempt, err.retry_after_secs()))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_max_retries_env_model_and_default() {
        assert_eq!(resolve_max_retries_with_env(Some("9"), Some(3)), 9);
        assert_eq!(resolve_max_retries_with_env(None, Some(7)), 7);
        assert_eq!(resolve_max_retries_with_env(None, None), DEFAULT_MAX_RETRIES);
        assert_eq!(resolve_max_retries_with_env(Some("abc"), Some(4)), 4);
    }

    #[test]
    fn backoff_first_retry_is_around_two_seconds() {
        let backoff = retry_backoff_with_jitter(1);
        assert!(
            backoff >= Duration::from_millis(1600) && backoff <= Duration::from_millis(2400),
            "{backoff:?}"
        );
        let zero = retry_backoff_with_jitter(0);
        assert!(zero >= Duration::from_millis(1600) && zero <= Duration::from_millis(2400));
    }

    #[test]
    fn backoff_doubles_then_caps_at_thirty_seconds() {
        let r2 = retry_backoff_with_jitter(2);
        assert!(r2 >= Duration::from_millis(3200) && r2 <= Duration::from_millis(4800), "{r2:?}");
        let r10 = retry_backoff_with_jitter(10);
        assert!(
            r10 >= Duration::from_millis(24_000) && r10 <= Duration::from_millis(36_000),
            "{r10:?}"
        );
    }

    #[test]
    fn retry_schedule_honors_retry_after_and_the_fifteen_try_cap() {
        let limited = ClientError::RateLimited {
            retry_after_secs: Some(120),
            message: "slow".into(),
        };
        assert_eq!(
            decide_retry(&limited, 0, DEFAULT_MAX_RETRIES),
            Some(Duration::from_secs(120))
        );
        assert!(decide_retry(&limited, 1, DEFAULT_MAX_RETRIES).is_none());

        let server = ClientError::Server {
            status: 522,
            message: "edge".into(),
            retry_after_secs: Some(120),
        };
        let waited = decide_retry(&server, 1, DEFAULT_MAX_RETRIES).unwrap();
        assert!(waited >= Duration::from_secs(24) && waited <= Duration::from_secs(36), "{waited:?}");

        let transport = ClientError::Transport("reset".into());
        let first = decide_retry(&transport, 0, DEFAULT_MAX_RETRIES).unwrap();
        assert!(
            first >= Duration::from_millis(160) && first <= Duration::from_millis(240),
            "{first:?}"
        );
        assert!(decide_retry(&transport, 14, DEFAULT_MAX_RETRIES).is_none());
        assert!(decide_retry(&transport, 13, DEFAULT_MAX_RETRIES).is_some());

        let oauth = ClientError::KeyOffer {
            status: 401,
            message: "no".into(),
        };
        assert!(decide_retry(&oauth, 0, DEFAULT_MAX_RETRIES).is_none());
        assert!(decide_retry(&ClientError::IdleTimeout, 0, DEFAULT_MAX_RETRIES).is_none());
        assert!(decide_retry(&ClientError::Disconnect, 0, DEFAULT_MAX_RETRIES).is_some());
    }
}
