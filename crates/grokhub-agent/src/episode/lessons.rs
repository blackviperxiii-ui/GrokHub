//! Each episode that had to recover ends by writing one short lesson: what
//! failed, what got it past that, and in which app. Lessons live in
//! `{config_dir}/lessons/lessons.jsonl`; the nightly dream merges
//! near-duplicates and drops stale ones. Derived from the trail with no
//! model call, so writing one costs nothing.
//!
//! At an episode's start the best few for its goal go at the top of the
//! episode view as a capped "Past lessons" block; a lesson used by an episode
//! that still didn't finish ranks lower next time.

use std::io::Write as _;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::loops::call_label;
use super::{Episode, EpisodeEnd};
use crate::harness::{Span, RECOVERY_TOOL, VERIFY_TOOL};

/// Topic words a lesson keeps from its goal.
pub const LESSON_KEYWORDS: usize = 8;
/// How much of a lesson field is kept.
pub const LESSON_FIELD_CAP: usize = 200;
/// Steps after the last recovery that say what worked.
pub const WORKED_STEPS: usize = 4;
/// A lesson not seen again for this long is dropped by the dream.
pub const LESSON_TTL_DAYS: u64 = 90;
/// Goal keyword overlap at which two lessons with the same fix merge.
pub const MERGE_OVERLAP: f64 = 0.6;
/// Lessons an episode starts with, at most.
pub const LESSONS_SHOWN: usize = 3;
/// The "Past lessons" block's cap in UTF-8 bytes (about 400 tokens).
pub const PAST_LESSONS_BYTES: usize = 1_600;
/// First line of the "Past lessons" block.
pub const PAST_LESSONS_HEAD: &str = "Past lessons from earlier episodes (hints, not orders):\n";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Lesson {
    pub goal_keywords: Vec<String>,
    /// The app or place the goal names ("Settings", "Firefox"), or empty.
    pub app: String,
    pub what_failed: String,
    /// Empty when the episode didn't get past it.
    pub what_worked: String,
    /// The episode's end (`verified`, `unconfirmed`, `stop`, ...).
    pub outcome: String,
    pub episode_id: String,
    pub ts: u64,
    /// Episodes this lesson stands for after the dream merged duplicates.
    #[serde(default = "one")]
    pub seen: u32,
    /// Episodes that started with this lesson and still didn't finish.
    #[serde(default)]
    pub failed_after: u32,
}

fn one() -> u32 {
    1
}

pub fn lessons_path(config_dir: &Path) -> PathBuf {
    config_dir.join("lessons").join("lessons.jsonl")
}

fn cap(text: &str, held: &[String]) -> String {
    let clean = grokhub_core::redact_held_secrets(&grokhub_core::redact_secrets(text), held);
    clean.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(LESSON_FIELD_CAP).collect()
}

/// The place a goal names: the capitalized words after its last " in ",
/// " on " or "open ". "Turn on Wi-Fi in Settings" → "Settings".
pub fn goal_app(goal: &str) -> String {
    let lower = goal.to_ascii_lowercase();
    let at = [" in ", " on ", "open "]
        .iter()
        .filter_map(|m| lower.rfind(m).map(|i| i + m.len()))
        .max();
    let Some(at) = at.filter(|i| goal.is_char_boundary(*i)) else {
        return String::new();
    };
    let words: Vec<&str> = goal[at..]
        .split_whitespace()
        .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric()))
        .skip_while(|w| matches!(w.to_ascii_lowercase().as_str(), "the" | "a" | "an" | "my"))
        .take_while(|w| w.chars().next().is_some_and(char::is_uppercase))
        .take(3)
        .collect();
    words.join(" ")
}

fn is_recovery(s: &Span) -> bool {
    s.tool == RECOVERY_TOOL
}

fn is_reject(s: &Span) -> bool {
    s.tool == VERIFY_TOOL && s.result == "fail"
}

fn is_step(s: &Span) -> bool {
    s.claim.starts_with("episode step")
}

/// The lesson an episode's trail teaches, or `None` for a trivial run (no
/// re-plan, no recovery, no checker reject).
pub fn derive(ep: &Episode, end: EpisodeEnd, now_ms: u64, held: &[String]) -> Option<Lesson> {
    let trouble: Vec<usize> = ep.trail.iter().enumerate().filter(|(_, s)| is_recovery(s) || is_reject(s)).map(|(i, _)| i).collect();
    let last = *trouble.last()?;
    let mut failed: Vec<String> = Vec::new();
    for &i in &trouble {
        let s = &ep.trail[i];
        let line = if is_reject(s) { format!("check said: {}", s.claim) } else { s.claim.clone() };
        if !line.trim().is_empty() && !failed.contains(&line) {
            failed.push(line);
        }
    }
    let worked = if end == EpisodeEnd::Verified {
        let mut steps: Vec<String> = Vec::new();
        for s in ep.trail[last..].iter().filter(|s| is_step(s) && s.decision == "allow" && !s.result.starts_with("failed")) {
            let label = call_label(&s.tool, &s.args_redacted);
            if !steps.contains(&label) {
                steps.push(label);
            }
            if steps.len() >= WORKED_STEPS {
                break;
            }
        }
        if steps.is_empty() {
            "the next claim passed the check".to_string()
        } else {
            format!("then {}", steps.join(", "))
        }
    } else {
        String::new()
    };
    let mut goal_keywords = grokhub_core::topic_words(&ep.goal);
    goal_keywords.truncate(LESSON_KEYWORDS);
    Some(Lesson {
        goal_keywords,
        app: cap(&goal_app(&ep.goal), held),
        what_failed: cap(&failed.join("; "), held),
        what_worked: cap(&worked, held),
        outcome: end.as_str().into(),
        episode_id: ep.id.clone(),
        ts: now_ms,
        seen: 1,
        failed_after: 0,
    })
}

/// "Settings: replan: click 'Save' failed 2× → then key 'ctrl+s'".
pub fn lesson_line(l: &Lesson) -> String {
    let mut line = String::new();
    if !l.app.is_empty() {
        line.push_str(&format!("{}: ", l.app));
    }
    line.push_str(&l.what_failed);
    if !l.what_worked.is_empty() {
        line.push_str(&format!(" → {}", l.what_worked));
    }
    line
}

pub fn append(config_dir: &Path, lesson: &Lesson) -> std::io::Result<()> {
    let path = lessons_path(config_dir);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut f = std::fs::OpenOptions::new().create(true).append(true).open(path)?;
    writeln!(f, "{}", serde_json::to_string(lesson).map_err(std::io::Error::other)?)
}

/// Every lesson that parses; a bad line is skipped, never fatal.
pub fn load(config_dir: &Path) -> Vec<Lesson> {
    std::fs::read_to_string(lessons_path(config_dir))
        .unwrap_or_default()
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect()
}

fn overlap(a: &[String], b: &[String]) -> f64 {
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    let shared = a.iter().filter(|w| b.contains(w)).count() as f64;
    let union = (a.len() + b.len()) as f64 - shared;
    shared / union
}

fn norm(text: &str) -> String {
    text.to_lowercase().split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Same app, and either the same failure or the same fix on mostly the same goal.
pub fn near_duplicate(a: &Lesson, b: &Lesson) -> bool {
    a.app.eq_ignore_ascii_case(&b.app)
        && (norm(&a.what_failed) == norm(&b.what_failed)
            || (norm(&a.what_worked) == norm(&b.what_worked) && overlap(&a.goal_keywords, &b.goal_keywords) >= MERGE_OVERLAP))
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LessonDream {
    pub merged: usize,
    pub dropped: usize,
    pub kept: usize,
}

/// Merge near-duplicates into the newest of them (keywords joined, counts
/// summed; a fix that worked beats one that didn't) and drop lessons older
/// than [`LESSON_TTL_DAYS`]. Pure.
pub fn dream(lessons: Vec<Lesson>, now_ms: u64) -> (Vec<Lesson>, LessonDream) {
    let ttl = LESSON_TTL_DAYS * 24 * 60 * 60 * 1000;
    let mut report = LessonDream::default();
    let mut sorted = lessons;
    sorted.sort_by_key(|l| std::cmp::Reverse(l.ts));
    let mut kept: Vec<Lesson> = Vec::new();
    for l in sorted {
        if now_ms.saturating_sub(l.ts) > ttl {
            report.dropped += 1;
            continue;
        }
        match kept.iter_mut().find(|k| near_duplicate(k, &l)) {
            Some(k) => {
                report.merged += 1;
                k.seen = k.seen.saturating_add(l.seen);
                k.failed_after = k.failed_after.saturating_add(l.failed_after);
                for w in l.goal_keywords {
                    if k.goal_keywords.len() < LESSON_KEYWORDS && !k.goal_keywords.contains(&w) {
                        k.goal_keywords.push(w);
                    }
                }
                if k.what_worked.is_empty() && !l.what_worked.is_empty() {
                    k.what_worked = l.what_worked;
                    k.outcome = l.outcome;
                }
            }
            None => kept.push(l),
        }
    }
    kept.sort_by_key(|l| l.ts);
    report.kept = kept.len();
    (kept, report)
}

/// The dream's pass over the store: rewrite it merged and pruned. Nothing
/// on disk is touched when there is nothing to change.
pub fn dream_store(config_dir: &Path, now_ms: u64) -> std::io::Result<LessonDream> {
    let (kept, report) = dream(load(config_dir), now_ms);
    if report.merged == 0 && report.dropped == 0 {
        return Ok(report);
    }
    rewrite(config_dir, &kept)?;
    Ok(report)
}

/// Replace the store with `lessons` (a temp file, then a rename).
fn rewrite(config_dir: &Path, lessons: &[Lesson]) -> std::io::Result<()> {
    let path = lessons_path(config_dir);
    let tmp = path.with_extension("jsonl.tmp");
    let mut body = String::new();
    for l in lessons {
        body.push_str(&serde_json::to_string(l).map_err(std::io::Error::other)?);
        body.push('\n');
    }
    std::fs::write(&tmp, body)?;
    std::fs::rename(&tmp, &path)
}

/// How well a lesson fits a goal: the same app counts 2, each shared goal
/// word 1. A lesson with no fix counts half, and each episode it preceded
/// that still failed divides it further. 0 when neither app nor word matches.
fn fit(l: &Lesson, app: &str, words: &[String]) -> f64 {
    let same_app = !app.is_empty() && l.app.eq_ignore_ascii_case(app);
    let shared = l.goal_keywords.iter().filter(|w| words.contains(w)).count();
    if !same_app && shared == 0 {
        return 0.0;
    }
    let base = if same_app { 2.0 } else { 0.0 } + shared as f64;
    let fixed = if l.what_worked.is_empty() { 0.5 } else { 1.0 };
    base * fixed / (1.0 + f64::from(l.failed_after))
}

/// The [`LESSONS_SHOWN`] lessons that fit `goal` best, best first (the
/// newest wins a tie). Keyword and app match only; no model call.
pub fn recall(lessons: &[Lesson], goal: &str) -> Vec<Lesson> {
    let app = goal_app(goal);
    let words = grokhub_core::topic_words(goal);
    let mut scored: Vec<(f64, &Lesson)> =
        lessons.iter().map(|l| (fit(l, &app, &words), l)).filter(|(score, _)| *score > 0.0).collect();
    scored.sort_by(|a, b| b.0.total_cmp(&a.0).then(b.1.ts.cmp(&a.1.ts)));
    scored.into_iter().take(LESSONS_SHOWN).map(|(_, l)| l.clone()).collect()
}

/// The "Past lessons" block for `goal` and the lessons in it. Whole lines
/// only, never over [`PAST_LESSONS_BYTES`]; empty when nothing fits.
pub fn past_lessons(lessons: &[Lesson], goal: &str, held: &[String]) -> (String, Vec<Lesson>) {
    let mut block = String::from(PAST_LESSONS_HEAD);
    let mut used = Vec::new();
    for l in recall(lessons, goal) {
        let line = format!("- {}\n", grokhub_core::redact_held_secrets(&lesson_line(&l), held));
        if block.len() + line.len() > PAST_LESSONS_BYTES {
            continue;
        }
        block.push_str(&line);
        used.push(l);
    }
    if used.is_empty() {
        block.clear();
    }
    (block, used)
}

/// An episode that started with these lessons (by their `episode_id`) ended.
/// When it still didn't finish (`unconfirmed` or `idle`), each one counts a
/// failure so [`recall`] ranks it lower. Returns how many were marked.
pub fn note_used(config_dir: &Path, used: &[String], end: EpisodeEnd) -> std::io::Result<usize> {
    if used.is_empty() || !matches!(end, EpisodeEnd::Unconfirmed | EpisodeEnd::Idle) {
        return Ok(0);
    }
    let mut all = load(config_dir);
    let mut marked = 0;
    for l in all.iter_mut().filter(|l| used.contains(&l.episode_id)) {
        l.failed_after = l.failed_after.saturating_add(1);
        marked += 1;
    }
    if marked > 0 {
        rewrite(config_dir, &all)?;
    }
    Ok(marked)
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;
    use crate::harness::Span;

    fn lesson(app: &str, failed: &str, worked: &str, kw: &[&str], ts: u64) -> Lesson {
        Lesson {
            goal_keywords: kw.iter().map(|s| s.to_string()).collect(),
            app: app.into(),
            what_failed: failed.into(),
            what_worked: worked.into(),
            outcome: if worked.is_empty() { "unconfirmed" } else { "verified" }.into(),
            episode_id: format!("ep-{ts}"),
            ts,
            seen: 1,
            failed_after: 0,
        }
    }

    fn step(tool: &str, args: &str, result: &str) -> Span {
        let mut s = Span::deny("c", tool, args, result, "soft");
        s.decision = "allow".into();
        s.claim = "episode step 1".into();
        s
    }

    fn recovery(claim: &str) -> Span {
        let mut s = Span::deny("c", RECOVERY_TOOL, "{}", "replan", "soft");
        s.decision = "replan".into();
        s.claim = claim.into();
        s
    }

    #[test]
    fn a_trail_with_a_replan_teaches_what_failed_and_what_worked() {
        let mut ep = Episode::begin("ep-1", "c", "Save notes.txt in Gedit", 0, &[]);
        ep.trail = vec![
            step("click", r#"{"label":"Save"}"#, "click ok"),
            recovery("replan: click 'Save' changed nothing 3×"),
            step("key", r#"{"keys":"ctrl+s"}"#, "key ok"),
            step("click", r#"{"x":10,"y":20}"#, "failed: gone"),
            step("key", r#"{"keys":"Return"}"#, "key ok"),
        ];
        let l = derive(&ep, EpisodeEnd::Verified, 42, &[]).expect("a lesson");
        assert_eq!(l.app, "Gedit");
        assert_eq!(l.what_failed, "replan: click 'Save' changed nothing 3×");
        assert_eq!(l.what_worked, "then key 'ctrl+s', key 'Return'");
        assert_eq!((l.outcome.as_str(), l.episode_id.as_str(), l.ts, l.seen), ("verified", "ep-1", 42, 1));
        assert_eq!(l.goal_keywords, vec!["save".to_string(), "note".into(), "gedit".into()]);
        assert_eq!(lesson_line(&l), "Gedit: replan: click 'Save' changed nothing 3× → then key 'ctrl+s', key 'Return'");
    }

    #[test]
    fn a_trivial_run_teaches_nothing() {
        let mut ep = Episode::begin("ep-2", "c", "Turn on Wi-Fi in Settings", 0, &[]);
        ep.trail = vec![step("click", r#"{"x":1,"y":2}"#, "click ok"), step("click", r#"{"x":3,"y":4}"#, "click ok")];
        assert_eq!(derive(&ep, EpisodeEnd::Verified, 1, &[]), None);
    }

    #[test]
    fn a_reject_it_never_got_past_is_kept_with_no_fix_and_secrets_stay_out() {
        let mut ep = Episode::begin("ep-3", "c", "Log in on Example Portal", 0, &[]);
        let mut check = Span::deny("c", VERIFY_TOOL, "{}", "fail", "soft");
        check.claim = "still on the hunter2222 login page".into();
        ep.trail = vec![step("type", r#"{"chars":9}"#, "type ok"), check];
        let l = derive(&ep, EpisodeEnd::Unconfirmed, 5, &["hunter2222".to_string()]).unwrap();
        assert_eq!(l.what_failed, "check said: still on the [redacted] login page");
        assert_eq!((l.what_worked.as_str(), l.outcome.as_str(), l.app.as_str()), ("", "unconfirmed", "Example Portal"));
    }

    #[test]
    fn goal_app_names_the_place_after_in_or_on() {
        assert_eq!(goal_app("Turn on Wi-Fi in Settings"), "Settings");
        assert_eq!(goal_app("open Firefox and save the page"), "Firefox");
        assert_eq!(goal_app("Save notes.txt in the editor"), "");
        assert_eq!(goal_app("sort the photos"), "");
    }

    #[test]
    fn the_dream_merges_near_duplicates_and_drops_stale_lessons() {
        let day = 24 * 60 * 60 * 1000;
        let now = 200 * day;
        let lessons = vec![
            // Same failure (case and spacing aside): merged into the newest.
            lesson("gedit", "Replan: click 'Save'  changed nothing 3×", "", &["save", "notes"], now - 2 * day),
            lesson("Gedit", "replan: click 'Save' changed nothing 3×", "then key 'ctrl+s'", &["save", "report"], now - day),
            // Same fix, mostly the same goal, a different failure line: merged too.
            lesson("Gedit", "replan: 3 steps failed with 'no window'", "then key 'ctrl+s'", &["save", "report"], now - 3 * day),
            // Another app: kept apart.
            lesson("Firefox", "replan: click 'Save' changed nothing 3×", "then key 'ctrl+s'", &["save", "page"], now - day),
            // Past the TTL.
            lesson("Settings", "check said: Wi-Fi off", "then click 'Wi-Fi'", &["wi-fi"], now - 91 * day),
        ];
        let (kept, report) = dream(lessons, now);
        assert_eq!(report, LessonDream { merged: 2, dropped: 1, kept: 2 });
        let gedit = kept.iter().find(|l| l.app.eq_ignore_ascii_case("gedit")).unwrap();
        assert_eq!(gedit.seen, 3);
        assert_eq!(gedit.what_worked, "then key 'ctrl+s'");
        assert_eq!(gedit.what_failed, "replan: click 'Save' changed nothing 3×", "the newest one stays");
        assert_eq!(gedit.goal_keywords, vec!["save".to_string(), "report".into(), "notes".into()]);
        assert_eq!(kept.iter().filter(|l| l.app == "Firefox").count(), 1);
    }

    #[test]
    fn recall_ranks_the_same_app_and_goal_first_and_down_ranks_lessons_that_preceded_failures() {
        let mut tried = lesson("Gedit", "replan: b", "then key 'ctrl+s'", &["save", "note"], 9);
        tried.failed_after = 2;
        let lessons = vec![
            lesson("Firefox", "replan: c", "then key 'ctrl+s'", &["save", "page"], 4),
            lesson("Firefox", "replan: d", "then click 'Save'", &["save", "page"], 3),
            tried,
            lesson("Settings", "check said: Wi-Fi off", "then click 'Wi-Fi'", &["wi-fi"], 10),
            lesson("Gedit", "replan: a", "then key 'ctrl+s'", &["save", "note"], 2),
        ];
        let picked: Vec<String> = recall(&lessons, "Save notes.txt in Gedit").into_iter().map(|l| l.what_failed).collect();
        // Gedit + two words = 4; the same over 1 + 2 failures ≈ 1.3; Firefox shares
        // one word = 1, the newer of the two wins the tie; Settings shares nothing.
        assert_eq!(picked, vec!["replan: a".to_string(), "replan: b".into(), "replan: c".into()]);
        // A lesson with no fix counts half.
        let no_fix = vec![lesson("Gedit", "replan: e", "", &["save"], 5), lesson("Gedit", "replan: f", "then g", &["save"], 1)];
        let picked: Vec<String> = recall(&no_fix, "Save it in Gedit").into_iter().map(|l| l.what_failed).collect();
        assert_eq!(picked, vec!["replan: f".to_string(), "replan: e".into()]);
        assert!(recall(&lessons, "Sort the photos").is_empty());
    }

    #[test]
    fn the_past_lessons_block_keeps_whole_lines_within_its_budget() {
        let long = "é".repeat(LESSON_FIELD_CAP);
        let lessons = vec![
            lesson("Gedit", &format!("replan: {long}"), &format!("then {long}"), &["save"], 3),
            lesson("Gedit", &format!("replan 2: {long}"), &format!("then {long}"), &["save"], 2),
            lesson("Gedit", "replan: short", "then key 'hunter2222'", &["save"], 1),
        ];
        let (block, used) = past_lessons(&lessons, "Save it in Gedit", &["hunter2222".to_string()]);
        assert!(block.len() <= PAST_LESSONS_BYTES, "{} bytes", block.len());
        assert_eq!(used.iter().map(|l| l.ts).collect::<Vec<_>>(), vec![3, 1], "the second long line would not fit");
        assert!(block.starts_with(PAST_LESSONS_HEAD));
        assert!(block.ends_with("- Gedit: replan: short → then key '[redacted]'\n"), "{block}");
        assert_eq!(block.lines().count(), 3);
        assert_eq!(past_lessons(&lessons, "Sort the photos", &[]), (String::new(), Vec::new()));
    }

    #[test]
    fn a_lesson_counts_a_failure_only_when_the_episode_that_used_it_did_not_finish() {
        let dir = crate::harness::test_dir("lessons-used");
        append(&dir, &lesson("Gedit", "replan: a", "then b", &["save"], 1)).unwrap();
        append(&dir, &lesson("Firefox", "replan: c", "then d", &["save"], 2)).unwrap();
        let used = vec!["ep-1".to_string()];
        assert_eq!(note_used(&dir, &used, EpisodeEnd::Verified).unwrap(), 0);
        assert_eq!(note_used(&dir, &used, EpisodeEnd::Stop).unwrap(), 0, "a stop is the user's call, not the lesson's");
        assert_eq!(note_used(&dir, &used, EpisodeEnd::Unconfirmed).unwrap(), 1);
        assert_eq!(note_used(&dir, &used, EpisodeEnd::Idle).unwrap(), 1);
        let after: Vec<(String, u32)> = load(&dir).into_iter().map(|l| (l.episode_id, l.failed_after)).collect();
        assert_eq!(after, vec![("ep-1".to_string(), 2), ("ep-2".into(), 0)]);
        assert_eq!(note_used(&dir, &[], EpisodeEnd::Unconfirmed).unwrap(), 0);
    }

    #[test]
    fn the_store_appends_loads_and_rewrites_only_when_the_dream_changed_something() {
        let dir = crate::harness::test_dir("lessons-store");
        let now = 10 * 24 * 60 * 60 * 1000;
        append(&dir, &lesson("Gedit", "replan: a", "then b", &["save"], now - 2)).unwrap();
        append(&dir, &lesson("Gedit", "replan: a", "then b", &["save"], now - 1)).unwrap();
        std::fs::OpenOptions::new().append(true).open(lessons_path(&dir)).unwrap().write_all(b"not json\n").unwrap();
        assert_eq!(load(&dir).len(), 2, "a bad line is skipped");
        assert_eq!(dream_store(&dir, now).unwrap(), LessonDream { merged: 1, dropped: 0, kept: 1 });
        let after = load(&dir);
        assert_eq!((after.len(), after[0].seen, after[0].ts), (1, 2, now - 1));
        let stamp = std::fs::metadata(lessons_path(&dir)).unwrap().modified().unwrap();
        assert_eq!(dream_store(&dir, now).unwrap(), LessonDream { merged: 0, dropped: 0, kept: 1 });
        assert_eq!(std::fs::metadata(lessons_path(&dir)).unwrap().modified().unwrap(), stamp, "nothing to change, nothing written");
    }
}
