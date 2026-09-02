//! Night-shift automations. Inherit chat YOLO / supervised. No cron language product.

use serde::{Deserialize, Serialize};

use crate::attach::bound_scan;
use crate::grok_loop::parse_loop_line;
use crate::organs::{hm_min, LocalClock};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Automation {
    #[serde(default)]
    pub id: String,
    pub name: String,
    pub schedule: String,
    pub time: String,
    #[serde(default)]
    pub times: Vec<String>,
    pub instructions: String,
    #[serde(default)]
    pub heartbeat_every_min: u32,
    #[serde(default)]
    pub check_command: String,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub last_run: Option<u64>,
    #[serde(default)]
    pub next_run: Option<u64>,
    #[serde(default)]
    pub run_count: u32,
}

fn default_enabled() -> bool {
    true
}

pub fn normalize_times(time: &str, times: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for t in times.iter().map(|s| s.as_str()).chain(std::iter::once(time)) {
        let Some(min) = hm_min(t) else {
            continue;
        };
        let key = format!("{:02}:{:02}", min / 60, min % 60);
        if seen.insert(key.clone()) {
            out.push(key);
        }
    }
    if out.is_empty() {
        out.push("09:00".into());
    }
    out.sort();
    out
}

pub fn compute_next_run(a: &Automation, clock: LocalClock) -> u64 {
    if a.schedule == "heartbeat" {
        let mins = a.heartbeat_every_min.clamp(1, 24 * 60);
        let gap = mins as u64 * 60_000;
        return match a.last_run {
            Some(last) if clock.now_ms.saturating_sub(last) < gap => last + gap,
            _ => clock.now_ms,
        };
    }
    let slots = normalize_times(&a.time, &a.times);
    let mut best = u64::MAX;
    for slot in &slots {
        let t = next_for_slot(&a.schedule, slot, clock);
        if t < best {
            best = t;
        }
    }
    if best == u64::MAX {
        clock.now_ms + 24 * 3_600_000
    } else {
        best
    }
}

fn next_for_slot(schedule: &str, time: &str, clock: LocalClock) -> u64 {
    let tgt = hm_min(time).unwrap_or(9 * 60);
    let now = clock.hour * 60 + clock.minute;
    match schedule {
        "once" => {
            if tgt > now {
                clock.now_ms + (tgt - now) as u64 * 60_000
            } else {
                clock.now_ms + 365 * 24 * 3_600_000
            }
        }
        "weekdays" => next_matching_day(clock, tgt, now, |wd| (1..=5).contains(&wd)),
        "weekly" => next_matching_day(clock, tgt, now, |wd| wd == 1),
        "monthly" => {
            if tgt > now {
                clock.now_ms + (tgt - now) as u64 * 60_000
            } else {
                clock.now_ms + 30 * 24 * 3_600_000
            }
        }
        _ => {
            let delta = if tgt > now {
                tgt - now
            } else {
                24 * 60 - now + tgt
            };
            clock.now_ms + delta as u64 * 60_000
        }
    }
}

fn next_matching_day(clock: LocalClock, tgt: u32, now: u32, ok: impl Fn(u8) -> bool) -> u64 {
    if tgt > now && ok(clock.weekday) {
        return clock.now_ms + (tgt - now) as u64 * 60_000;
    }
    for ahead in 1..=8u32 {
        let wd = (clock.weekday as u32 + ahead) % 7;
        if ok(wd as u8) {
            let mins = ahead * 24 * 60 - now + tgt;
            return clock.now_ms + mins as u64 * 60_000;
        }
    }
    clock.now_ms + 24 * 3_600_000
}

pub fn ensure_automation_schedule(mut a: Automation, clock: LocalClock) -> Automation {
    let times = normalize_times(&a.time, &a.times);
    a.time = times.first().cloned().unwrap_or_else(|| "09:00".into());
    a.times = times;
    if !a.enabled {
        a.next_run = None;
        return a;
    }
    if a.schedule == "heartbeat" {
        a.next_run = Some(compute_next_run(&a, clock));
        return a;
    }
    if a.next_run.map(|t| t > clock.now_ms).unwrap_or(false) {
        return a;
    }
    a.next_run = Some(compute_next_run(&a, clock));
    a
}

pub fn due_automations(list: &[Automation], now_ms: u64) -> Vec<Automation> {
    list.iter()
        .filter(|a| {
            if !a.enabled || a.next_run.map(|t| t > now_ms).unwrap_or(true) {
                return false;
            }
            if a.schedule == "once" {
                return a.run_count == 0;
            }
            if a.schedule == "heartbeat" {
                let mins = a.heartbeat_every_min.max(1);
                if let Some(last) = a.last_run {
                    if now_ms.saturating_sub(last) < mins as u64 * 60_000 - 5_000 {
                        return false;
                    }
                }
            }
            true
        })
        .cloned()
        .collect()
}

pub fn automation_blocked_by_policy(quiet: bool, destructive: bool, autonomy: u8) -> bool {
    (quiet && destructive) || (autonomy == 0 && destructive)
}

/// Replay that did not start must not consume the night slot.
pub fn night_counts_run(replay_started: Option<bool>) -> bool {
    !matches!(replay_started, Some(false))
}

/// Missing OAuth must leave the due set, same as quiet/policy — not hammer every tick.
pub fn night_unauth_should_skip(has_key: bool) -> bool {
    !has_key
}

/// A finished night check must not halt a live chat/update job.
pub fn night_check_may_fire(running: bool) -> bool {
    !running
}

/// `replay last` or `every day at 21, replay last` — night runs the saved recipe, not a chat hop.
pub fn replay_automation_target(instructions: &str) -> Option<&str> {
    let t = instructions.trim();
    let lower = t.to_ascii_lowercase();
    let idx = lower.find("replay ")?;
    let rest = t[idx + "replay ".len()..].trim();
    if rest.is_empty() {
        None
    } else {
        Some(rest)
    }
}

pub fn mark_automation_ran(mut a: Automation, now_ms: u64) -> Automation {
    a.last_run = Some(now_ms);
    a.run_count = a.run_count.saturating_add(1);
    if a.schedule == "once" {
        a.enabled = false;
        a.next_run = None;
    }
    a
}

/// Policy/quiet skip: leave the due set without counting a successful run.
pub fn mark_automation_skipped(mut a: Automation, now_ms: u64, clock: LocalClock) -> Automation {
    a.last_run = Some(now_ms);
    a.next_run = Some(compute_next_run(&a, clock).max(now_ms.saturating_add(60_000)));
    a
}

/// Night compose and an explicit "remind me / schedule / automation" ask.
pub fn user_asked_to_schedule(user: &str) -> bool {
    let t = user.to_ascii_lowercase();
    [
        "every day",
        "every weekday",
        "heartbeat every",
        "remind me",
        "schedule",
        "automat",
        "night job",
        "nightly",
    ]
    .iter()
    .any(|p| t.contains(p))
}

/// Chat must not turn advice ("you could heartbeat every 15 min") into a live job.
pub fn chat_may_save_automation(user: &str, assistant: &str) -> bool {
    user_asked_to_schedule(user) && parse_nl_automation(assistant).is_some()
}

pub fn parse_nl_automation(text: &str) -> Option<Automation> {
    let text = bound_scan(text);
    let t = text.trim();
    if t.is_empty() {
        return None;
    }
    if let Some(rest) = t.to_ascii_lowercase().find("heartbeat every ") {
        let after = &t[rest + "heartbeat every ".len()..];
        let mins: u32 = after
            .split_whitespace()
            .next()
            .and_then(|s| s.trim_end_matches("mins").trim_end_matches("min").parse().ok())
            .unwrap_or(15)
            .max(1);
        let name = t
            .replace("heartbeat every ", "")
            .replace("Heartbeat every ", "");
        return Some(Automation {
            id: String::new(),
            name: name.trim().to_string(),
            schedule: "heartbeat".into(),
            time: "00:00".into(),
            times: vec![],
            instructions: t.to_string(),
            heartbeat_every_min: mins,
            check_command: String::new(),
            enabled: true,
            last_run: None,
            next_run: None,
            run_count: 0,
        });
    }
    let lower = t.to_ascii_lowercase();
    let weekday = lower.contains("every weekday at ");
    let daily = lower.contains("every day at ");
    if !weekday && !daily {
        return None;
    }
    let needle = if weekday {
        "every weekday at "
    } else {
        "every day at "
    };
    let idx = lower.find(needle)?;
    let clock_src = &t[idx + needle.len()..];
    let mut toks = clock_src.split_whitespace();
    let first = toks.next().unwrap_or("9:00");
    // "9 pm" splits into two words; "9pm" does not.
    let (hh, mm) = match toks.next().filter(|s| matches!(s.trim_end_matches(','), "am" | "pm")) {
        Some(half) => parse_clock_token(&format!("{first}{half}")),
        None => parse_clock_token(first),
    }
    .unwrap_or((9, 0));
    let clock = format!("{hh:02}:{mm:02}");
    Some(Automation {
        id: String::new(),
        name: schedule_name(&t[..idx], clock_src),
        schedule: if weekday { "weekdays" } else { "daily" }.into(),
        time: clock,
        times: vec![],
        instructions: t.to_string(),
        heartbeat_every_min: 0,
        check_command: String::new(),
        enabled: true,
        last_run: None,
        next_run: None,
        run_count: 0,
    })
}

/// The page label for a clock job: the ask with the schedule words taken out, so a row
/// reads "summarize the workboard", not "9, summarize the workboard".
fn schedule_name(before: &str, after_needle: &str) -> String {
    let tail = after_needle
        .split_whitespace()
        .skip_while(|w| {
            let w = w.trim_end_matches(',').to_ascii_lowercase();
            w == "am" || w == "pm" || parse_clock_token(&w).is_some()
        })
        .collect::<Vec<_>>()
        .join(" ");
    let trim = |s: &str| {
        s.trim()
            .trim_start_matches([',', '-', ':', '.'])
            .trim()
            .to_string()
    };
    let head = trim(before);
    let tail = trim(&tail);
    let joined = match (head.is_empty(), tail.is_empty()) {
        (true, true) => after_needle.trim().to_string(),
        (true, false) => tail,
        (false, true) => head,
        (false, false) => format!("{head} {tail}"),
    };
    joined.trim().to_string()
}

/// Which scheduler owns a scheduling ask.
///
/// A clock time ("every weekday at 9") is a cabin automation in `automations.json`; the
/// Grok Build `/loop` list only understands intervals, and reading "every day at 9" as a
/// `1d` loop drops the hour and leaves "at 9," glued to the prompt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScheduleRoute {
    Clock(Box<Automation>),
    Interval { interval: String, prompt: String },
}

pub fn route_schedule(text: &str) -> Option<ScheduleRoute> {
    if let Some(a) = parse_nl_automation(text) {
        if a.schedule != "heartbeat" {
            return Some(ScheduleRoute::Clock(Box::new(a)));
        }
        return Some(ScheduleRoute::Interval {
            interval: format!("{}m", a.heartbeat_every_min.max(1)),
            prompt: a.instructions,
        });
    }
    parse_loop_line(text).map(|(interval, prompt)| ScheduleRoute::Interval { interval, prompt })
}

/// "weekdays at 09:00" / "every 15m" — how a job repeats, without the next-run clock.
pub fn automation_schedule_label(a: &Automation) -> String {
    if a.schedule == "heartbeat" {
        return format!("every {}", minutes_label(a.heartbeat_every_min.max(1)));
    }
    let slots = normalize_times(&a.time, &a.times).join(", ");
    let word = match a.schedule.as_str() {
        "weekdays" => "weekdays",
        "weekly" => "weekly",
        "monthly" => "monthly",
        "once" => "once",
        _ => "daily",
    };
    format!("{word} at {slots}")
}

/// The line under an automation on the Automations page.
pub fn automation_summary_line(a: &Automation, now_ms: u64) -> String {
    let mut bits = vec![automation_schedule_label(a)];
    if !a.enabled {
        bits.push("paused".into());
    } else if let Some(next) = a.next_run {
        bits.push(format!("next {}", relative_when(next, now_ms)));
    }
    if a.run_count > 0 {
        let plural = if a.run_count == 1 { "" } else { "s" };
        bits.push(format!("{} run{plural}", a.run_count));
    }
    bits.join(" · ")
}

fn minutes_label(mins: u32) -> String {
    if mins.is_multiple_of(60) && mins >= 60 {
        let h = mins / 60;
        return if h == 24 {
            "24h".into()
        } else {
            format!("{h}h")
        };
    }
    format!("{mins}m")
}

fn relative_when(then_ms: u64, now_ms: u64) -> String {
    if then_ms <= now_ms {
        return "now".into();
    }
    let mins = (then_ms - now_ms) / 60_000;
    match mins {
        0 => "in under a minute".into(),
        1 => "in 1 min".into(),
        2..=59 => format!("in {mins} min"),
        60..=1439 => {
            let h = mins / 60;
            let m = mins % 60;
            if m == 0 {
                format!("in {h}h")
            } else {
                format!("in {h}h {m}m")
            }
        }
        _ => {
            let days = mins / 1440;
            if days == 1 {
                "in 1 day".into()
            } else {
                format!("in {days} days")
            }
        }
    }
}

/// `9`, `9:30`, `9pm`, `9:30 pm`, `21:00` — the hour a night job is asked for.
pub fn parse_clock_token(tok: &str) -> Option<(u32, u32)> {
    let t = tok.trim().trim_end_matches(',').to_ascii_lowercase();
    if t.is_empty() {
        return None;
    }
    let (body, half) = if let Some(rest) = t.strip_suffix("pm") {
        (rest.trim(), Some(true))
    } else if let Some(rest) = t.strip_suffix("am") {
        (rest.trim(), Some(false))
    } else {
        (t.as_str(), None)
    };
    let mut parts = body.split(':');
    let mut hh: u32 = parts.next()?.trim().parse().ok()?;
    let mm: u32 = match parts.next() {
        Some(m) => m.trim().parse().ok()?,
        None => 0,
    };
    if hh > 23 || mm > 59 {
        return None;
    }
    match half {
        Some(true) if hh < 12 => hh += 12,
        Some(false) if hh == 12 => hh = 0,
        _ => {}
    }
    Some((hh, mm))
}

pub fn skip_automation(check_output: &str, check_exit: i32) -> bool {
    check_exit != 0 || check_output.trim().is_empty()
}

/// Night `checkCommand` must run off the UI thread. Empty means no gate.
pub fn night_check_command(check_command: &str) -> Option<&str> {
    let t = check_command.trim();
    if t.is_empty() {
        None
    } else {
        Some(t)
    }
}

pub fn night_check_exit_code(output: &str) -> i32 {
    let line = match output.find("\nexit ") {
        Some(idx) => &output[idx + 1..],
        None => output.trim_start(),
    };
    let Some(rest) = line.strip_prefix("exit ") else {
        return 1;
    };
    let num: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    num.parse().unwrap_or(1)
}

/// Host receipts wrap `$ cmd` / `exit N`. Only the command stdout gates skip-on-empty.
pub fn night_check_stdout(receipt: &str) -> &str {
    let rest = match receipt.find("\nexit ") {
        Some(idx) => {
            let after = &receipt[idx + 1..];
            match after.find('\n') {
                Some(nl) => &after[nl + 1..],
                None => "",
            }
        }
        None => receipt,
    };
    match rest.find("[stderr]") {
        Some(idx) => &rest[..idx],
        None => rest,
    }
}

pub fn skip_night_check_receipt(receipt: &str) -> bool {
    skip_automation(night_check_stdout(receipt), night_check_exit_code(receipt))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_nl_automation_does_not_lowercase_an_8mb_complete() {
        let src = include_str!("automation.rs");
        let parse = src
            .split("pub fn parse_nl_automation(")
            .nth(1)
            .and_then(|s| s.split("#[cfg(test)]").next())
            .expect("parse_nl_automation");
        let lower = parse.find("to_ascii_lowercase").expect("lowercase scan");
        assert!(
            parse[..lower].contains("bound_scan")
                || parse[..lower].contains("TEXT_FILE_CAP")
                || parse[..lower].contains("chip_scan"),
            "night parse must not lowercase an 8MB complete: {parse}"
        );
    }

    #[test]
    fn parse_nl_automation_reads_a_schedule_after_a_huge_prefix() {
        let mut huge = "a".repeat(crate::attach::TEXT_FILE_CAP + 8);
        huge.push_str("\nevery weekday at 9, summarize the workboard");
        let a = parse_nl_automation(&huge).expect("tail schedule");
        assert_eq!(a.schedule, "weekdays");
        assert_eq!(a.time, "09:00");
    }

    #[test]
    fn a_clock_time_routes_to_the_cabin_scheduler_not_a_one_day_loop() {
        // `parse_loop_line` reads "every day at 9" as a 1d interval and keeps "at 9," in
        // the prompt, so the hour is lost. Clock asks must reach `automations.json`.
        let Some(ScheduleRoute::Clock(a)) = route_schedule("every day at 9, summarize the board")
        else {
            panic!("clock ask must route to the cabin scheduler");
        };
        assert_eq!(a.schedule, "daily");
        assert_eq!(a.time, "09:00");
        assert_eq!(a.name, "summarize the board");
        assert_eq!(
            route_schedule("/loop 30m check deploy"),
            Some(ScheduleRoute::Interval {
                interval: "30m".into(),
                prompt: "check deploy".into()
            })
        );
        assert_eq!(
            route_schedule("heartbeat every 15 min check the board"),
            Some(ScheduleRoute::Interval {
                interval: "15m".into(),
                prompt: "heartbeat every 15 min check the board".into()
            })
        );
        assert_eq!(
            route_schedule("every 2 hours summarize commits"),
            Some(ScheduleRoute::Interval {
                interval: "2h".into(),
                prompt: "summarize commits".into()
            })
        );
        assert_eq!(route_schedule("what is rust"), None);
    }

    #[test]
    fn nine_pm_is_not_nine_am() {
        assert_eq!(parse_clock_token("9"), Some((9, 0)));
        assert_eq!(parse_clock_token("9:30"), Some((9, 30)));
        assert_eq!(parse_clock_token("21:00"), Some((21, 0)));
        assert_eq!(parse_clock_token("9pm"), Some((21, 0)));
        assert_eq!(parse_clock_token("9:30PM"), Some((21, 30)));
        assert_eq!(parse_clock_token("12am"), Some((0, 0)));
        assert_eq!(parse_clock_token("12pm"), Some((12, 0)));
        assert_eq!(parse_clock_token("25"), None);
        assert_eq!(parse_clock_token("later"), None);
        assert_eq!(
            parse_nl_automation("every day at 9pm, back up the notes")
                .expect("pm job")
                .time,
            "21:00",
            "an evening job must not silently run in the morning"
        );
        let spaced = parse_nl_automation("every weekday at 9 pm, back up the notes").expect("pm");
        assert_eq!(spaced.time, "21:00");
        assert_eq!(spaced.name, "back up the notes");
        let asked = parse_nl_automation("remind me every weekday at 9 to water the plants")
            .expect("remind");
        assert_eq!(asked.name, "remind me to water the plants");
        assert_eq!(asked.time, "09:00");
    }

    #[test]
    fn summary_line_reads_like_a_schedule() {
        let mut a = parse_nl_automation("every weekday at 9, summarize the board").expect("job");
        a.next_run = Some(90 * 60_000);
        assert_eq!(
            automation_summary_line(&a, 0),
            "weekdays at 09:00 · next in 1h 30m"
        );
        a.run_count = 1;
        assert!(automation_summary_line(&a, 0).ends_with("· 1 run"));
        a.run_count = 4;
        assert!(automation_summary_line(&a, 0).ends_with("· 4 runs"));
        a.enabled = false;
        let paused = automation_summary_line(&a, 0);
        assert!(
            paused.contains("paused") && !paused.contains("next"),
            "a paused job must not advertise a next run: {paused}"
        );
        let beat = parse_nl_automation("heartbeat every 90 min check the board").expect("beat");
        assert_eq!(automation_schedule_label(&beat), "every 90m");
        let hourly = parse_nl_automation("heartbeat every 120 min check").expect("beat");
        assert_eq!(automation_schedule_label(&hourly), "every 2h");
        let mut due = a.clone();
        due.enabled = true;
        due.next_run = Some(0);
        assert!(automation_summary_line(&due, 10).contains("next now"));
    }

    #[test]
    fn nl_and_gate() {
        let a = parse_nl_automation("every weekday at 9, summarize the workboard").unwrap();
        assert_eq!(a.schedule, "weekdays");
        assert_eq!(a.time, "09:00");
        let h = parse_nl_automation("heartbeat every 15 min check the board").unwrap();
        assert_eq!(h.schedule, "heartbeat");
        assert_eq!(h.heartbeat_every_min, 15);
        assert!(
            user_asked_to_schedule("remind me every weekday at 9 to summarize the board"),
            "Night compose and an explicit ask still save a job"
        );
        assert!(user_asked_to_schedule("add an automation: heartbeat every 15 min"));
        assert!(
            !user_asked_to_schedule("what is rust"),
            "a normal question must not arm a night job"
        );
        assert!(
            !chat_may_save_automation(
                "how do I check disk",
                "You could heartbeat every 15 min check the board.\nHOST_CMD: df -h\n"
            ),
            "advice that mentions a schedule must not become a live night job"
        );
        assert!(chat_may_save_automation(
            "remind me every weekday at 9",
            "every weekday at 9, summarize the workboard"
        ));
        assert!(skip_automation("", 0));
        assert!(skip_automation("ok", 1));
        assert!(!skip_automation("work to do", 0));
        assert_eq!(night_check_command("  "), None);
        assert_eq!(night_check_command("test -f /tmp/ready"), Some("test -f /tmp/ready"));
        assert_eq!(night_check_exit_code("$ cmd\nexit 0\n"), 0);
        assert_eq!(night_check_exit_code("$ cmd\nexit 1\n"), 1);
        assert_eq!(
            night_check_exit_code("$ cmd\nwould exit 0 if clean\nexit 1 · 4ms\n"),
            1,
            "stdout mentioning exit 0 must not open the night gate"
        );
        assert_eq!(night_check_exit_code("$ cmd\nexit 0 · 3ms\n"), 0);
        assert!(skip_automation("ready", night_check_exit_code("exit 1")));
        assert!(!skip_automation("ready", night_check_exit_code("exit 0")));
        assert_eq!(night_check_stdout("$ cmd\nexit 0 · 3ms\n"), "");
        assert_eq!(night_check_stdout("$ cmd\nexit 0 · 3ms\ndirty\n"), "dirty\n");
        assert!(skip_night_check_receipt("$ git status --porcelain\nexit 0 · 3ms\n"));
        assert!(!skip_night_check_receipt("$ git status --porcelain\nexit 0 · 3ms\n M src.rs\n"));
        let clock = LocalClock {
            now_ms: 1_000,
            weekday: 3,
            hour: 8,
            minute: 0,
        };
        let scheduled = ensure_automation_schedule(a, clock);
        assert_eq!(scheduled.next_run, Some(1_000 + 60 * 60_000));
        let due = due_automations(
            &[Automation {
                id: "n1".into(),
                name: "now".into(),
                schedule: "daily".into(),
                time: "08:00".into(),
                times: vec![],
                instructions: "check".into(),
                heartbeat_every_min: 0,
                check_command: String::new(),
                enabled: true,
                last_run: None,
                next_run: Some(500),
                run_count: 0,
            }],
            1_000,
        );
        assert_eq!(due.len(), 1);
        assert!(automation_blocked_by_policy(true, true, 3));
        assert!(!automation_blocked_by_policy(false, true, 3));
        assert_eq!(replay_automation_target("replay last"), Some("last"));
        assert_eq!(replay_automation_target("every day at 21, replay last"), Some("last"));
        assert_eq!(replay_automation_target("REPLAY desk-1"), Some("desk-1"));
        assert!(night_counts_run(None), "a chat night job still counts");
        assert!(night_counts_run(Some(true)));
        assert!(
            !night_counts_run(Some(false)),
            "a missing recipe must not consume the night slot"
        );
        assert!(
            night_unauth_should_skip(false),
            "no key must skip the night slot so tick_night stops re-firing"
        );
        assert!(!night_unauth_should_skip(true));
        assert!(
            !night_check_may_fire(true),
            "a passing check must wait until the cabin is idle"
        );
        assert!(night_check_may_fire(false));
        assert_eq!(replay_automation_target("summarize the workboard"), None);
        let blocked = Automation {
            id: "n0".into(),
            name: "rm".into(),
            schedule: "heartbeat".into(),
            time: "00:00".into(),
            times: vec![],
            instructions: "rm -rf /tmp/x".into(),
            heartbeat_every_min: 15,
            check_command: String::new(),
            enabled: true,
            last_run: None,
            next_run: Some(500),
            run_count: 0,
        };
        assert!(automation_blocked_by_policy(false, true, 0));
        let skipped = mark_automation_skipped(blocked, 1_000, clock);
        assert_eq!(skipped.run_count, 0);
        assert!(skipped.next_run.unwrap() > 1_000);
        assert!(due_automations(&[skipped], 1_000).is_empty());
        let once = Automation {
            id: "o1".into(),
            name: "once".into(),
            schedule: "once".into(),
            time: "00:00".into(),
            times: vec![],
            instructions: "hello".into(),
            heartbeat_every_min: 0,
            check_command: "test -f /missing".into(),
            enabled: true,
            last_run: None,
            next_run: Some(500),
            run_count: 0,
        };
        let check_skip = mark_automation_skipped(once.clone(), 1_000, clock);
        assert!(check_skip.enabled, "a gated skip must not disable once jobs");
        assert_eq!(check_skip.run_count, 0);
        let ran = mark_automation_ran(once, 1_000);
        assert!(!ran.enabled);
        assert_eq!(ran.run_count, 1);
        let monthly = Automation {
            id: "m1".into(),
            name: "month".into(),
            schedule: "monthly".into(),
            time: "09:00".into(),
            times: vec![],
            instructions: "summarize".into(),
            heartbeat_every_min: 0,
            check_command: String::new(),
            enabled: true,
            last_run: None,
            next_run: None,
            run_count: 0,
        };
        let after = LocalClock {
            now_ms: 1_000,
            weekday: 5,
            hour: 10,
            minute: 0,
        };
        let next = compute_next_run(&monthly, after);
        assert!(
            next >= 1_000 + 29 * 24 * 3_600_000,
            "monthly after today's slot must not become daily: {next}"
        );
    }
}
