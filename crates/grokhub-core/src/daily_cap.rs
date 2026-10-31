//! The per-local-day post counter the Pulse ledgers share (ask offers, hobby
//! paths, nudge watchers): a `day` key and a `posted_today` count that start
//! over when the local day changes, and the serde default for each
//! settings struct's `daily_max`.

/// Start a new day's count when `today` (the local `YYYY-MM-DD`) is not the
/// ledger's day. The same day leaves the count alone.
pub fn day_reset(day: &mut String, posted_today: &mut u32, today: &str) {
    if day != today {
        *day = today.to_string();
        *posted_today = 0;
    }
}

/// `#[serde(default = "crate::daily_cap::daily_max::<N>")]`: the cap on new
/// posts per local day when `app.json` has none for that ledger.
pub fn daily_max<const N: u32>() -> u32 {
    N
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_day_starts_the_count_over_and_the_same_day_keeps_it() {
        let mut day = "2026-10-30".to_string();
        let mut posted = 2;
        day_reset(&mut day, &mut posted, "2026-10-30");
        assert_eq!((day.as_str(), posted), ("2026-10-30", 2));
        day_reset(&mut day, &mut posted, "2026-10-31");
        assert_eq!((day.as_str(), posted), ("2026-10-31", 0));
        let mut fresh = String::new();
        let mut none = 0;
        day_reset(&mut fresh, &mut none, "2026-10-31");
        assert_eq!((fresh.as_str(), none), ("2026-10-31", 0));
    }

    #[test]
    fn the_default_cap_is_the_constant_each_ledger_names() {
        assert_eq!(daily_max::<1>(), 1);
        assert_eq!(daily_max::<3>(), 3);
    }
}
