//! Weekly "your week" Pulse recap. A fun Digest card built only from opt-in
//! derived tags and counters the cabin already holds (library playtime, code
//! resume repos, prints / disk ledgers when present, task outcomes). No new
//! scanning. Empty signals hide their sections. Share text never carries
//! personal paths.

use std::collections::BTreeMap;

use crate::organs::LocalClock;
use crate::outcome::{OutcomeResult, TaskOutcome};
use crate::personality::{Emoji, Personality, Verbosity};
use crate::self_review::WEEK_MS;

/// Source ids of week-recap cards start with this.
pub const CARD_SOURCE_PREFIX: &str = "week-recap:";
/// Pulse card title.
pub const CARD_TITLE: &str = "Your week";
/// Cabin state file (under the config dir).
pub const FILE_NAME: &str = "week_recap.json";
/// Games listed on the card.
pub const GAMES_CAP: usize = 5;
/// Projects listed on the card.
pub const PROJECTS_CAP: usize = 5;
/// Top tasks listed on the card.
pub const TOP_TASKS_CAP: usize = 5;

/// One game the user opened this week, with library playtime minutes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GamePlayed {
    pub name: String,
    /// Lifetime minutes from the library counter (not a new scan).
    pub minutes: u64,
}

/// One code project touched this week. Name only — never a path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectTouched {
    pub name: String,
    pub commits: u32,
}

/// A GrokHub task that finished successfully this week.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskDone {
    pub label: String,
    pub count: u32,
}

/// Personality flavor for greeting / closing lines.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecapTone {
    pub name: String,
    pub verbosity: Verbosity,
    pub emoji: Emoji,
    pub address: Option<String>,
    pub roast: bool,
}

impl Default for RecapTone {
    fn default() -> Self {
        Self {
            name: "Nyx".into(),
            verbosity: Verbosity::Normal,
            emoji: Emoji::None,
            address: None,
            roast: false,
        }
    }
}

impl From<&Personality> for RecapTone {
    fn from(p: &Personality) -> Self {
        Self {
            name: p.name.clone(),
            verbosity: p.verbosity,
            emoji: p.emoji,
            address: p.address.clone(),
            roast: p.roast,
        }
    }
}

/// Injected week snapshot. Built by the cabin from counters it already holds.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WeekRecapInputs {
    /// Stable week id (`YYYY-Www` or a Monday `YYYY-MM-DD`).
    pub week_key: String,
    pub generated_at: u64,
    pub games: Vec<GamePlayed>,
    pub projects: Vec<ProjectTouched>,
    /// `None` or zero hides the prints section.
    pub prints_made: Option<u32>,
    /// `None` or zero hides the disk section.
    pub disk_freed_bytes: Option<u64>,
    pub top_tasks: Vec<TaskDone>,
    pub suggestion: Option<String>,
    pub tone: RecapTone,
}

/// One generated weekly recap. Sections with no signal stay empty and are omitted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WeekRecap {
    pub week_key: String,
    pub generated_at: u64,
    pub greeting: String,
    pub games: Vec<String>,
    pub projects: Vec<String>,
    pub prints: Option<String>,
    pub disk: Option<String>,
    pub tasks: Vec<String>,
    pub suggestion: Option<String>,
    pub closing: String,
}

impl WeekRecap {
    pub fn source_id(&self) -> String {
        format!("{CARD_SOURCE_PREFIX}{}", self.week_key)
    }

    /// Short Pulse body: greeting + up to a few highlights + suggestion.
    pub fn card_body(&self) -> String {
        let mut lines = vec![self.greeting.clone()];
        if let Some(line) = self.games.first() {
            lines.push(format!("Games · {line}"));
        }
        if let Some(line) = self.projects.first() {
            lines.push(format!("Projects · {line}"));
        }
        if let Some(ref p) = self.prints {
            lines.push(p.clone());
        }
        if let Some(ref d) = self.disk {
            lines.push(d.clone());
        }
        if let Some(line) = self.tasks.first() {
            lines.push(format!("Tasks · {line}"));
        }
        if let Some(ref s) = self.suggestion {
            lines.push(format!("Next · {s}"));
        }
        if lines.len() == 1 {
            lines.push(self.closing.clone());
        }
        lines.join("\n")
    }

    /// Full markdown for card `details` and chat. Omits empty sections.
    pub fn render(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!("# {CARD_TITLE}\n\n"));
        out.push_str(&format!("{}\n\n", self.greeting));
        section_lines(&mut out, "Games played", &self.games);
        section_lines(&mut out, "Projects touched", &self.projects);
        if let Some(ref p) = self.prints {
            out.push_str("## Prints made\n\n");
            out.push_str(&format!("- {p}\n\n"));
        }
        if let Some(ref d) = self.disk {
            out.push_str("## Disk freed\n\n");
            out.push_str(&format!("- {d}\n\n"));
        }
        section_lines(
            &mut out,
            crate::brand::filled("Top {product} tasks"),
            &self.tasks,
        );
        out.push_str("## Next week\n\n");
        match &self.suggestion {
            Some(s) => out.push_str(&format!("- {s}\n\n")),
            None => out.push_str("- Nothing queued — pick something fun.\n\n"),
        }
        out.push_str(&self.closing);
        out.push('\n');
        out
    }

    /// Path-free share text (no home folders, no absolute paths).
    pub fn share_text(&self) -> String {
        let mut lines = vec![CARD_TITLE.to_string(), self.greeting.clone()];
        for g in &self.games {
            lines.push(format!("Games · {g}"));
        }
        for p in &self.projects {
            lines.push(format!("Projects · {p}"));
        }
        if let Some(ref p) = self.prints {
            lines.push(p.clone());
        }
        if let Some(ref d) = self.disk {
            lines.push(d.clone());
        }
        for t in &self.tasks {
            lines.push(format!("Tasks · {t}"));
        }
        if let Some(ref s) = self.suggestion {
            lines.push(format!("Next · {s}"));
        }
        lines.push(self.closing.clone());
        let text = lines.join("\n");
        assert_share_safe(&text);
        text
    }

    /// Lines for a shareable image layout (same content as [`share_text`]).
    pub fn share_lines(&self) -> Vec<String> {
        self.share_text().lines().map(str::to_string).collect()
    }
}

fn section_lines(out: &mut String, title: &str, lines: &[String]) {
    if lines.is_empty() {
        return;
    }
    out.push_str(&format!("## {title}\n\n"));
    for line in lines {
        out.push_str(&format!("- {line}\n"));
    }
    out.push('\n');
}

/// Refuse personal paths in share text (absolute Unix / Windows / home).
pub fn assert_share_safe(text: &str) {
    debug_assert!(
        !text.contains("/home/")
            && !text.contains("\\Users\\")
            && !text.contains("/Users/")
            && !text.contains("C:\\")
            && !text.contains("c:\\"),
        "share text must not carry personal paths: {text}"
    );
}

/// Strip a repo path down to a bare folder name.
pub fn project_name(raw: &str) -> String {
    let trimmed = raw.trim().trim_end_matches(['/', '\\']);
    let name = trimmed.rsplit(['/', '\\']).next().unwrap_or(trimmed).trim();
    if name.is_empty() {
        "project".into()
    } else {
        name.to_string()
    }
}

/// Build the recap. Empty / off signals produce no section.
pub fn build_recap(input: &WeekRecapInputs) -> WeekRecap {
    let games: Vec<String> = input
        .games
        .iter()
        .filter(|g| !g.name.trim().is_empty() && g.minutes > 0)
        .take(GAMES_CAP)
        .map(|g| format!("{} · {}", g.name.trim(), hours_label(g.minutes)))
        .collect();
    let projects: Vec<String> = input
        .projects
        .iter()
        .filter(|p| !p.name.trim().is_empty())
        .take(PROJECTS_CAP)
        .map(|p| {
            let name = project_name(&p.name);
            let n = p.commits.max(1);
            format!("{name} · {n} commit{}", if n == 1 { "" } else { "s" })
        })
        .collect();
    let prints = input
        .prints_made
        .and_then(|n| (n > 0).then(|| format!("{n} print{}", if n == 1 { "" } else { "s" })));
    let disk = input
        .disk_freed_bytes
        .and_then(|b| (b > 0).then(|| format!("{} freed", bytes_label(b))));
    let tasks: Vec<String> = input
        .top_tasks
        .iter()
        .filter(|t| !t.label.trim().is_empty() && t.count > 0)
        .take(TOP_TASKS_CAP)
        .map(|t| format!("{} · {}×", t.label.trim(), t.count))
        .collect();
    let suggestion = input
        .suggestion
        .as_ref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let greeting = greeting_line(
        &input.tone,
        &input.week_key,
        has_any(&games, &projects, &prints, &disk, &tasks),
    );
    let closing = closing_line(&input.tone);
    WeekRecap {
        week_key: input.week_key.clone(),
        generated_at: input.generated_at,
        greeting,
        games,
        projects,
        prints,
        disk,
        tasks,
        suggestion,
        closing,
    }
}

fn has_any(
    games: &[String],
    projects: &[String],
    prints: &Option<String>,
    disk: &Option<String>,
    tasks: &[String],
) -> bool {
    !games.is_empty()
        || !projects.is_empty()
        || prints.is_some()
        || disk.is_some()
        || !tasks.is_empty()
}

fn greeting_line(tone: &RecapTone, week_key: &str, has_signal: bool) -> String {
    let who = tone
        .address
        .as_deref()
        .map(|a| format!(", {a}"))
        .unwrap_or_default();
    let spark = match tone.emoji {
        Emoji::Heavy => " ✨",
        Emoji::Light => " ·",
        Emoji::None => "",
    };
    let body = if has_signal {
        match tone.verbosity {
            Verbosity::Terse => format!("Week {week_key}{who}{spark}"),
            Verbosity::Chatty => format!(
                "Here's your week ({week_key}){who} — a quick look at what showed up{spark}"
            ),
            Verbosity::Normal => format!("Your week ({week_key}){who}{spark}"),
        }
    } else if tone.roast {
        format!("Quiet week ({week_key}){who} — even the counters took a nap{spark}")
    } else {
        format!("Quiet week ({week_key}){who} — nothing opt-in lit up{spark}")
    };
    if tone.verbosity == Verbosity::Chatty && !tone.name.is_empty() {
        format!("{} here. {body}", tone.name)
    } else {
        body
    }
}

fn closing_line(tone: &RecapTone) -> String {
    match (tone.roast, tone.verbosity) {
        (true, Verbosity::Terse) => "Don't trip next week.".into(),
        (true, _) => "Try not to nap through next week.".into(),
        (false, Verbosity::Terse) => "Onward.".into(),
        (false, Verbosity::Chatty) => {
            "Share this card if you want — no personal paths on it.".into()
        }
        (false, Verbosity::Normal) => "See you next week.".into(),
    }
}

fn hours_label(minutes: u64) -> String {
    if minutes < 60 {
        format!("{minutes}m")
    } else {
        let h = minutes / 60;
        let m = minutes % 60;
        if m == 0 {
            format!("{h}h")
        } else {
            format!("{h}h {m}m")
        }
    }
}

fn bytes_label(bytes: u64) -> String {
    const KIB: u64 = 1024;
    const MIB: u64 = KIB * 1024;
    const GIB: u64 = MIB * 1024;
    if bytes >= GIB {
        format!("{:.1} GB", bytes as f64 / GIB as f64)
    } else if bytes >= MIB {
        format!("{:.0} MB", bytes as f64 / MIB as f64)
    } else if bytes >= KIB {
        format!("{:.0} KB", bytes as f64 / KIB as f64)
    } else {
        format!("{bytes} B")
    }
}

/// Whether the weekly recap may post now: new week key, at the usual active
/// hour (or the brief hour when set). When rhythm has not learned yet, posts
/// from 17:00 local so a cold install still gets one card.
pub fn week_recap_due(
    last_week: Option<&str>,
    week_key: &str,
    clock: &LocalClock,
    usually_active: bool,
    brief_hour: Option<u32>,
    rhythm_enough: bool,
) -> bool {
    if last_week == Some(week_key) || week_key.is_empty() {
        return false;
    }
    delivery_ok(clock, usually_active, brief_hour, rhythm_enough)
}

/// Hour gate only (week key checked by the caller).
pub fn delivery_ok(
    clock: &LocalClock,
    usually_active: bool,
    brief_hour: Option<u32>,
    rhythm_enough: bool,
) -> bool {
    if let Some(h) = brief_hour {
        return clock.hour == h;
    }
    if rhythm_enough {
        return usually_active;
    }
    clock.hour >= 17
}

/// ISO-like week key from a local calendar day `YYYY-MM-DD` and weekday
/// (0 = Sunday). Anchors on the Monday of that week.
pub fn week_key_from_day(day: &str, weekday: u8) -> String {
    let Some((y, m, d)) = parse_ymd(day) else {
        return day.to_string();
    };
    // Days since Monday: Sun=6, Mon=0, … Sat=5.
    let since_monday = match weekday {
        0 => 6u32,
        w => u32::from(w) - 1,
    };
    let (y, m, d) = shift_days(y, m, d, -(since_monday as i32));
    format!("{y:04}-{m:02}-{d:02}")
}

fn parse_ymd(day: &str) -> Option<(i32, u32, u32)> {
    let mut parts = day.split('-');
    let y = parts.next()?.parse().ok()?;
    let m = parts.next()?.parse().ok()?;
    let d = parts.next()?.parse().ok()?;
    Some((y, m, d))
}

fn shift_days(y: i32, m: u32, d: u32, delta: i32) -> (i32, u32, u32) {
    let day = civil_to_days(y, m, d) + i64::from(delta);
    days_to_civil(day)
}

fn civil_to_days(y: i32, m: u32, d: u32) -> i64 {
    // Howard Hinnant civil_from_days inverse.
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = (y - era * 400) as u64;
    let m = m as u64;
    let d = d as u64;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    (era as i64) * 146_097 + doe as i64 - 719_468
}

fn days_to_civil(mut z: i64) -> (i32, u32, u32) {
    z += 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe as i32 + (era as i32) * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m as u32, d as u32)
}

/// Games with `last_played` inside the week window, ranked by playtime.
pub fn games_from_library(games: &[(String, u64, u64)], now_ms: u64) -> Vec<GamePlayed> {
    // (name, last_played_unix_s, playtime_min)
    let now_s = now_ms / 1000;
    let week_s = WEEK_MS / 1000;
    let mut out: Vec<GamePlayed> = games
        .iter()
        .filter(|(_, last, mins)| *last > 0 && *mins > 0 && now_s.saturating_sub(*last) <= week_s)
        .map(|(name, _, mins)| GamePlayed {
            name: name.trim().to_string(),
            minutes: *mins,
        })
        .filter(|g| !g.name.is_empty())
        .collect();
    out.sort_by(|a, b| b.minutes.cmp(&a.minutes).then(a.name.cmp(&b.name)));
    out.truncate(GAMES_CAP);
    out
}

/// Projects with a HEAD commit inside the week. `commits` is the existing
/// ahead counter (unpushed), at least 1 when touched.
pub fn projects_from_repos(repos: &[(String, u64, u32)], now_ms: u64) -> Vec<ProjectTouched> {
    // (name_or_path, last_commit_s, ahead)
    let now_s = now_ms / 1000;
    let week_s = WEEK_MS / 1000;
    let mut out: Vec<ProjectTouched> = repos
        .iter()
        .filter(|(_, last, _)| *last > 0 && now_s.saturating_sub(*last) <= week_s)
        .map(|(name, _, ahead)| ProjectTouched {
            name: project_name(name),
            commits: (*ahead).max(1),
        })
        .filter(|p| !p.name.is_empty())
        .collect();
    out.sort_by(|a, b| b.commits.cmp(&a.commits).then(a.name.cmp(&b.name)));
    out.truncate(PROJECTS_CAP);
    out
}

/// Successful GrokHub tasks in the week, ranked by count.
pub fn top_tasks_from_outcomes(outcomes: &[TaskOutcome], now_ms: u64) -> Vec<TaskDone> {
    let mut by: BTreeMap<String, u32> = BTreeMap::new();
    for o in outcomes {
        if o.effective(now_ms) != OutcomeResult::Success {
            continue;
        }
        if o.finished_at > now_ms || now_ms.saturating_sub(o.finished_at) > WEEK_MS {
            continue;
        }
        let label = o
            .skill()
            .or_else(|| o.topic())
            .unwrap_or(o.signature.as_str())
            .trim();
        if label.is_empty() {
            continue;
        }
        *by.entry(label.to_string()).or_default() += 1;
    }
    let mut out: Vec<TaskDone> = by
        .into_iter()
        .map(|(label, count)| TaskDone { label, count })
        .collect();
    out.sort_by(|a, b| b.count.cmp(&a.count).then(a.label.cmp(&b.label)));
    out.truncate(TOP_TASKS_CAP);
    out
}

/// One next-week suggestion from the strongest reaction category, else the
/// top successful task.
pub fn suggest_next(top_category: Option<&str>, top_task: Option<&str>) -> Option<String> {
    if let Some(cat) = top_category.map(str::trim).filter(|s| !s.is_empty()) {
        return Some(format!("Lean into {cat} again"));
    }
    if let Some(task) = top_task.map(str::trim).filter(|s| !s.is_empty()) {
        return Some(format!("Keep the streak on {task}"));
    }
    None
}

/// Highest-scoring non-quiet reaction category.
pub fn top_reaction_category(scores: &[(String, i32, bool)]) -> Option<String> {
    scores
        .iter()
        .filter(|(_, score, quiet)| !*quiet && *score > 0)
        .max_by_key(|(_, score, _)| *score)
        .map(|(name, _, _)| name.clone())
}

/// Tiny shareable PNG (RGB8) of the recap lines. No personal paths; fixed palette.
pub fn share_png(recap: &WeekRecap) -> Vec<u8> {
    let lines = recap.share_lines();
    render_text_png(&lines, 640, 360)
}

fn render_text_png(lines: &[String], width: u32, height: u32) -> Vec<u8> {
    let mut rgba = vec![0u8; (width * height * 4) as usize];
    // Background #0f1419
    for px in rgba.as_chunks_mut::<4>().0 {
        px[0] = 0x0f;
        px[1] = 0x14;
        px[2] = 0x19;
        px[3] = 0xff;
    }
    let fg = [0xe7u8, 0xe9, 0xea, 0xff];
    let mut y = 24i32;
    for line in lines.iter().take(14) {
        draw_text(&mut rgba, width, height, 24, y, line, fg);
        y += 22;
        if y as u32 + 16 > height {
            break;
        }
    }
    encode_png_rgba(&rgba, width, height)
}

fn draw_text(
    rgba: &mut [u8],
    width: u32,
    height: u32,
    mut x: i32,
    y: i32,
    text: &str,
    color: [u8; 4],
) {
    for ch in text.chars().take(72) {
        let glyph = glyph5x7(ch);
        for (row, bits) in glyph.iter().enumerate() {
            for col in 0..5 {
                if bits & (1 << (4 - col)) != 0 {
                    put_px(rgba, width, height, x + col, y + row as i32 * 2, color);
                    put_px(rgba, width, height, x + col, y + row as i32 * 2 + 1, color);
                }
            }
        }
        x += 12;
        if x as u32 + 12 > width.saturating_sub(16) {
            break;
        }
    }
}

fn put_px(rgba: &mut [u8], width: u32, height: u32, x: i32, y: i32, color: [u8; 4]) {
    if x < 0 || y < 0 || x as u32 >= width || y as u32 >= height {
        return;
    }
    let i = ((y as u32 * width + x as u32) * 4) as usize;
    rgba[i..i + 4].copy_from_slice(&color);
}

fn glyph5x7(ch: char) -> [u8; 7] {
    // Minimal 5x7 font for ASCII printable; unknown → block.
    match ch {
        ' ' => [0; 7],
        '!' => [0x04, 0x04, 0x04, 0x04, 0x00, 0x04, 0x00],
        '#' => [0x0a, 0x1f, 0x0a, 0x1f, 0x0a, 0x00, 0x00],
        '\'' => [0x0c, 0x04, 0x08, 0x00, 0x00, 0x00, 0x00],
        ',' => [0x00, 0x00, 0x00, 0x00, 0x0c, 0x04, 0x08],
        '-' => [0x00, 0x00, 0x1f, 0x00, 0x00, 0x00, 0x00],
        '.' => [0x00, 0x00, 0x00, 0x00, 0x00, 0x04, 0x00],
        '/' => [0x01, 0x02, 0x04, 0x08, 0x10, 0x00, 0x00],
        '0' => [0x0e, 0x11, 0x13, 0x15, 0x19, 0x11, 0x0e],
        '1' => [0x04, 0x0c, 0x04, 0x04, 0x04, 0x04, 0x0e],
        '2' => [0x0e, 0x11, 0x01, 0x06, 0x08, 0x10, 0x1f],
        '3' => [0x1f, 0x02, 0x04, 0x02, 0x01, 0x11, 0x0e],
        '4' => [0x02, 0x06, 0x0a, 0x12, 0x1f, 0x02, 0x02],
        '5' => [0x1f, 0x10, 0x1e, 0x01, 0x01, 0x11, 0x0e],
        '6' => [0x06, 0x08, 0x10, 0x1e, 0x11, 0x11, 0x0e],
        '7' => [0x1f, 0x01, 0x02, 0x04, 0x08, 0x08, 0x08],
        '8' => [0x0e, 0x11, 0x11, 0x0e, 0x11, 0x11, 0x0e],
        '9' => [0x0e, 0x11, 0x11, 0x0f, 0x01, 0x02, 0x0c],
        ':' => [0x00, 0x04, 0x00, 0x00, 0x04, 0x00, 0x00],
        '·' | '•' => [0x00, 0x00, 0x04, 0x00, 0x00, 0x00, 0x00],
        '×' => [0x00, 0x11, 0x0a, 0x04, 0x0a, 0x11, 0x00],
        '✨' => [0x04, 0x15, 0x0e, 0x1f, 0x0e, 0x15, 0x04],
        c if c.is_ascii_uppercase() => LETTERS[(c as u8 - b'A') as usize],
        c if c.is_ascii_lowercase() => LETTERS[(c as u8 - b'a') as usize],
        _ => [0x1f, 0x11, 0x11, 0x11, 0x11, 0x11, 0x1f],
    }
}

const LETTERS: [[u8; 7]; 26] = [
    [0x0e, 0x11, 0x11, 0x1f, 0x11, 0x11, 0x11], // A
    [0x1e, 0x11, 0x11, 0x1e, 0x11, 0x11, 0x1e],
    [0x0e, 0x11, 0x10, 0x10, 0x10, 0x11, 0x0e],
    [0x1e, 0x11, 0x11, 0x11, 0x11, 0x11, 0x1e],
    [0x1f, 0x10, 0x10, 0x1e, 0x10, 0x10, 0x1f],
    [0x1f, 0x10, 0x10, 0x1e, 0x10, 0x10, 0x10],
    [0x0e, 0x11, 0x10, 0x17, 0x11, 0x11, 0x0f],
    [0x11, 0x11, 0x11, 0x1f, 0x11, 0x11, 0x11],
    [0x0e, 0x04, 0x04, 0x04, 0x04, 0x04, 0x0e],
    [0x01, 0x01, 0x01, 0x01, 0x11, 0x11, 0x0e],
    [0x11, 0x12, 0x14, 0x18, 0x14, 0x12, 0x11],
    [0x10, 0x10, 0x10, 0x10, 0x10, 0x10, 0x1f],
    [0x11, 0x1b, 0x15, 0x15, 0x11, 0x11, 0x11],
    [0x11, 0x19, 0x15, 0x13, 0x11, 0x11, 0x11],
    [0x0e, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0e],
    [0x1e, 0x11, 0x11, 0x1e, 0x10, 0x10, 0x10],
    [0x0e, 0x11, 0x11, 0x11, 0x15, 0x12, 0x0d],
    [0x1e, 0x11, 0x11, 0x1e, 0x14, 0x12, 0x11],
    [0x0f, 0x10, 0x10, 0x0e, 0x01, 0x01, 0x1e],
    [0x1f, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04],
    [0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0e],
    [0x11, 0x11, 0x11, 0x11, 0x11, 0x0a, 0x04],
    [0x11, 0x11, 0x11, 0x15, 0x15, 0x1b, 0x11],
    [0x11, 0x11, 0x0a, 0x04, 0x0a, 0x11, 0x11],
    [0x11, 0x11, 0x0a, 0x04, 0x04, 0x04, 0x04],
    [0x1f, 0x01, 0x02, 0x04, 0x08, 0x10, 0x1f], // Z
];

fn encode_png_rgba(rgba: &[u8], width: u32, height: u32) -> Vec<u8> {
    // Unfiltered RGBA PNG.
    let mut raw = Vec::with_capacity(((width * 4 + 1) * height) as usize);
    for row in 0..height {
        raw.push(0); // filter None
        let start = (row * width * 4) as usize;
        let end = start + (width * 4) as usize;
        raw.extend_from_slice(&rgba[start..end]);
    }
    let mut out = Vec::new();
    out.extend_from_slice(b"\x89PNG\r\n\x1a\n");
    write_chunk(&mut out, b"IHDR", &{
        let mut d = Vec::new();
        d.extend_from_slice(&width.to_be_bytes());
        d.extend_from_slice(&height.to_be_bytes());
        d.extend_from_slice(&[8, 6, 0, 0, 0]); // 8-bit RGBA
        d
    });
    let compressed = deflate_store(&raw);
    write_chunk(&mut out, b"IDAT", &compressed);
    write_chunk(&mut out, b"IEND", &[]);
    out
}

fn write_chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let mut c = crc32();
    c.update(kind);
    c.update(data);
    out.extend_from_slice(&c.finish().to_be_bytes());
}

/// Stored (uncompressed) zlib stream — fine for a small share card.
fn deflate_store(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    out.push(0x78);
    out.push(0x01);
    let mut i = 0;
    while i < data.len() {
        let end = (i + 65535).min(data.len());
        let chunk = &data[i..end];
        let last = end == data.len();
        out.push(if last { 0x01 } else { 0x00 });
        let n = chunk.len() as u16;
        out.extend_from_slice(&n.to_le_bytes());
        out.extend_from_slice(&(!n).to_le_bytes());
        out.extend_from_slice(chunk);
        i = end;
    }
    let adler = adler32(data);
    out.extend_from_slice(&adler.to_be_bytes());
    out
}

fn adler32(data: &[u8]) -> u32 {
    let mut a: u32 = 1;
    let mut b: u32 = 0;
    for &byte in data {
        a = (a + u32::from(byte)) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}

struct Crc32 {
    value: u32,
}

fn crc32() -> Crc32 {
    Crc32 { value: 0xffff_ffff }
}

impl Crc32 {
    fn update(&mut self, data: &[u8]) {
        for &byte in data {
            self.value ^= u32::from(byte);
            for _ in 0..8 {
                let mask = if self.value & 1 != 0 { 0xedb8_8320 } else { 0 };
                self.value = (self.value >> 1) ^ mask;
            }
        }
    }

    fn finish(self) -> u32 {
        !self.value
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::outcome::{OutcomeResult, TaskOutcome};

    fn tone() -> RecapTone {
        RecapTone::default()
    }

    fn fixture_full() -> WeekRecapInputs {
        WeekRecapInputs {
            week_key: "2026-10-27".into(),
            generated_at: 1_700_000_000_000,
            games: vec![
                GamePlayed {
                    name: "Hades".into(),
                    minutes: 150,
                },
                GamePlayed {
                    name: "Celeste".into(),
                    minutes: 45,
                },
            ],
            projects: vec![
                ProjectTouched {
                    name: "/home/user/code/grokhub".into(),
                    commits: 12,
                },
                ProjectTouched {
                    name: "loadout".into(),
                    commits: 3,
                },
            ],
            prints_made: Some(4),
            disk_freed_bytes: Some(1_500_000_000),
            top_tasks: vec![
                TaskDone {
                    label: "weekly-report".into(),
                    count: 3,
                },
                TaskDone {
                    label: "tidy-cache".into(),
                    count: 2,
                },
            ],
            suggestion: Some("Lean into gaming again".into()),
            tone: tone(),
        }
    }

    #[test]
    fn recap_from_fixture_counters() {
        let recap = build_recap(&fixture_full());
        assert_eq!(recap.week_key, "2026-10-27");
        assert_eq!(recap.games.len(), 2);
        assert!(
            recap.games[0].starts_with("Hades · 2h 30m"),
            "{}",
            recap.games[0]
        );
        assert_eq!(recap.projects[0], "grokhub · 12 commits");
        assert!(
            !recap.projects[0].contains("/home/"),
            "{}",
            recap.projects[0]
        );
        assert_eq!(recap.prints.as_deref(), Some("4 prints"));
        assert!(
            recap.disk.as_deref().unwrap().contains("GB freed"),
            "{:?}",
            recap.disk
        );
        assert_eq!(recap.tasks[0], "weekly-report · 3×");
        assert_eq!(recap.suggestion.as_deref(), Some("Lean into gaming again"));
        let body = recap.card_body();
        assert!(body.contains("Games · Hades"), "{body}");
        assert!(body.contains("Next · Lean into gaming"), "{body}");
        let md = recap.render();
        assert!(md.contains("## Games played"), "{md}");
        assert!(md.contains("## Prints made"), "{md}");
        assert!(!md.contains("/home/user"), "{md}");
        let share = recap.share_text();
        assert!(!share.contains("/home/"), "{share}");
        assert!(!share.contains("\\Users\\"), "{share}");
        let png = share_png(&recap);
        assert!(png.starts_with(b"\x89PNG\r\n\x1a\n"), "png header");
        assert!(png.len() > 100, "non-empty png {}", png.len());
    }

    #[test]
    fn sections_hide_when_a_signal_is_off() {
        let mut input = fixture_full();
        input.games.clear();
        input.prints_made = None;
        input.disk_freed_bytes = Some(0);
        input.projects.clear();
        let recap = build_recap(&input);
        assert!(recap.games.is_empty());
        assert!(recap.projects.is_empty());
        assert!(recap.prints.is_none());
        assert!(recap.disk.is_none());
        assert!(!recap.tasks.is_empty());
        let md = recap.render();
        assert!(!md.contains("## Games played"), "{md}");
        assert!(!md.contains("## Projects touched"), "{md}");
        assert!(!md.contains("## Prints made"), "{md}");
        assert!(!md.contains("## Disk freed"), "{md}");
        assert!(md.contains("## Top Loadout tasks"), "{md}");
    }

    #[test]
    fn empty_week_is_honest() {
        let recap = build_recap(&WeekRecapInputs {
            week_key: "2026-10-20".into(),
            generated_at: 1,
            tone: tone(),
            ..WeekRecapInputs::default()
        });
        assert!(recap.games.is_empty());
        assert!(recap.suggestion.is_none());
        assert!(recap.greeting.contains("Quiet week"), "{}", recap.greeting);
        let md = recap.render();
        assert!(!md.contains("## Games played"), "{md}");
        assert!(md.contains("Nothing queued"), "{md}");
    }

    #[test]
    fn personality_shapes_greeting() {
        let mut input = fixture_full();
        input.tone = RecapTone {
            name: "Butler".into(),
            verbosity: Verbosity::Chatty,
            emoji: Emoji::None,
            address: Some("sir".into()),
            roast: false,
        };
        let recap = build_recap(&input);
        assert!(recap.greeting.contains("Butler"), "{}", recap.greeting);
        assert!(recap.greeting.contains("sir"), "{}", recap.greeting);
        input.tone.verbosity = Verbosity::Terse;
        input.tone.name = "Nyx".into();
        let terse = build_recap(&input);
        assert!(terse.greeting.starts_with("Week "), "{}", terse.greeting);
        assert_eq!(terse.closing, "Onward.");
    }

    #[test]
    fn due_once_per_week_at_active_or_brief_hour() {
        let clock = LocalClock {
            now_ms: 1,
            weekday: 3,
            hour: 19,
            minute: 0,
        };
        assert!(week_recap_due(None, "2026-10-27", &clock, true, None, true));
        assert!(!week_recap_due(
            Some("2026-10-27"),
            "2026-10-27",
            &clock,
            true,
            None,
            true
        ));
        assert!(!week_recap_due(
            None,
            "2026-10-27",
            &LocalClock { hour: 10, ..clock },
            false,
            None,
            true
        ));
        assert!(week_recap_due(
            None,
            "2026-10-27",
            &LocalClock { hour: 9, ..clock },
            false,
            Some(9),
            true
        ));
        assert!(week_recap_due(
            None,
            "2026-10-27",
            &LocalClock { hour: 18, ..clock },
            false,
            None,
            false
        ));
        assert!(!week_recap_due(
            None,
            "2026-10-27",
            &LocalClock { hour: 12, ..clock },
            false,
            None,
            false
        ));
    }

    #[test]
    fn week_key_anchors_on_monday() {
        // 2026-10-28 is Wednesday (weekday 3).
        assert_eq!(week_key_from_day("2026-10-28", 3), "2026-10-26");
        // Sunday → previous Monday.
        assert_eq!(week_key_from_day("2026-10-25", 0), "2026-10-19");
    }

    #[test]
    fn helpers_rank_library_repos_and_outcomes() {
        let now = 1_700_000_000_000u64;
        let now_s = now / 1000;
        let games = games_from_library(
            &[
                ("Hades".into(), now_s - 60, 120),
                ("Old".into(), now_s - WEEK_MS / 1000 - 10, 999),
                ("Celeste".into(), now_s - 10, 30),
            ],
            now,
        );
        assert_eq!(games.len(), 2);
        assert_eq!(games[0].name, "Hades");
        let projects = projects_from_repos(
            &[
                ("/home/user/code/alpha".into(), now_s - 10, 4),
                ("beta".into(), now_s - WEEK_MS / 1000 - 5, 9),
            ],
            now,
        );
        assert_eq!(
            projects,
            vec![ProjectTouched {
                name: "alpha".into(),
                commits: 4,
            }]
        );
        let outcomes = vec![
            TaskOutcome {
                task: "t1".into(),
                signature: "skill:weekly-report".into(),
                result: OutcomeResult::Success,
                finished_at: now - 1_000,
                at: now - 1_000,
                ..TaskOutcome::default()
            },
            TaskOutcome {
                task: "t2".into(),
                signature: "skill:weekly-report".into(),
                result: OutcomeResult::Success,
                finished_at: now - 2_000,
                at: now - 2_000,
                ..TaskOutcome::default()
            },
            TaskOutcome {
                task: "t3".into(),
                signature: "skill:other".into(),
                result: OutcomeResult::Failure,
                finished_at: now - 3_000,
                at: now - 3_000,
                ..TaskOutcome::default()
            },
        ];
        let tasks = top_tasks_from_outcomes(&outcomes, now);
        assert_eq!(
            tasks,
            vec![TaskDone {
                label: "weekly-report".into(),
                count: 2,
            }]
        );
        assert_eq!(
            suggest_next(Some("gaming"), Some("weekly-report")).as_deref(),
            Some("Lean into gaming again")
        );
        assert_eq!(
            top_reaction_category(&[
                ("news".into(), 2, false),
                ("gaming".into(), 5, false),
                ("spam".into(), 9, true),
            ])
            .as_deref(),
            Some("gaming")
        );
    }

    #[test]
    fn project_name_strips_paths() {
        assert_eq!(project_name("/home/user/code/grokhub"), "grokhub");
        assert_eq!(project_name(r"C:\Users\user\dev\app"), "app");
        assert_eq!(project_name("plain"), "plain");
    }
}
