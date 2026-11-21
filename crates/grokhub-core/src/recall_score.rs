//! The recall score shared by `MEMORY.md` recall and AMR recall.
//!
//! SQLite FTS5 `bm25()` is negative: a closer lexical match is more negative.
//! `rank = -bm25`, and the rank halves every `half_life` seconds of age.

/// `score = (−bm25) × exp(−ln(2) × age / half_life)`.
///
/// `bm25` is the raw SQLite FTS5 value (negative, closer to zero is worse).
/// `age_secs` and `half_life_secs` are seconds. A non-positive half-life
/// disables decay and returns the rank alone. At one half-life the score is
/// half the rank, so a newer equal match sorts first.
pub fn combine_score(bm25: f64, age_secs: f64, half_life_secs: f64) -> f64 {
    let rank = -bm25;
    if !rank.is_finite() || rank <= 0.0 {
        return 0.0;
    }
    if !half_life_secs.is_finite() || half_life_secs <= 0.0 {
        return rank;
    }
    let age = if age_secs.is_finite() {
        age_secs.max(0.0)
    } else {
        0.0
    };
    rank * (-std::f64::consts::LN_2 * age / half_life_secs).exp()
}
