//! Start-my-day Pulse brief. Aggregates existing proactive signals (code
//! resume, watcher alerts, game news/sales, print status, backups due, one
//! next step) into one Digest. Empty sections stay hidden. At most
//! [`ITEM_CAP`] items, ranked by Pulse reaction scores. Pure: no files, no
//! model, no network. The cabin injects inputs and posts.

use crate::organs::LocalClock;
use crate::personality::{Emoji, Personality, Verbosity};

/// Source ids of start-my-day cards start with this.
pub const CARD_SOURCE_PREFIX: &str = "day-brief:";
/// Pulse card title.
pub const CARD_TITLE: &str = "Start my day";
/// Cabin state file (under the config dir).
pub const FILE_NAME: &str = "day_brief.json";
/// Hard cap on ranked items shown on the card.
pub const ITEM_CAP: usize = 5;

/// Which proactive bucket an item came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DayBriefSection {
    Resume,
    Watcher,
    GameNews,
    Print,
    Backup,
    NextStep,
}

impl DayBriefSection {
    pub fn heading(self) -> &'static str {
        match self {
            Self::Resume => "Pick up where you left off",
            Self::Watcher => "Watcher alerts",
            Self::GameNews => "Game news and sales",
            Self::Print => "Print status",
            Self::Backup => "Backups due",
            Self::NextStep => "Suggested next step",
        }
    }

    pub fn short(self) -> &'static str {
        match self {
            Self::Resume => "Resume",
            Self::Watcher => "Watchers",
            Self::GameNews => "Games",
            Self::Print => "Prints",
            Self::Backup => "Backups",
            Self::NextStep => "Next",
        }
    }

    /// Default reaction category when the seed does not name one.
    pub fn default_category(self) -> &'static str {
        match self {
            Self::Resume => "coding next step",
            Self::Watcher => "system",
            Self::GameNews => "game news",
            Self::Print => "print status",
            Self::Backup => "backups",
            Self::NextStep => "suggestions",
        }
    }
}

/// One actionable line the cabin already knows about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DayBriefSeed {
    pub title: String,
    pub body: String,
    /// One-tap chat / `/bg` prompt.
    pub prompt: String,
    /// Reaction category used for ranking (e.g. `"game news"`).
    pub category: String,
}

impl DayBriefSeed {
    pub fn new(
        title: impl Into<String>,
        body: impl Into<String>,
        prompt: impl Into<String>,
        category: impl Into<String>,
    ) -> Self {
        Self {
            title: title.into(),
            body: body.into(),
            prompt: prompt.into(),
            category: category.into(),
        }
    }
}

/// Personality flavor for greeting / closing lines.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BriefTone {
    pub name: String,
    pub verbosity: Verbosity,
    pub emoji: Emoji,
    pub address: Option<String>,
    pub roast: bool,
}

impl Default for BriefTone {
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

impl From<&Personality> for BriefTone {
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

/// Injected day snapshot. Built by the cabin from caches it already holds.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DayBriefInputs {
    /// Local day key (`YYYY-MM-DD`).
    pub day_key: String,
    pub generated_at: u64,
    pub resume: Option<DayBriefSeed>,
    pub watchers: Vec<DayBriefSeed>,
    pub game_news: Vec<DayBriefSeed>,
    pub prints: Vec<DayBriefSeed>,
    pub backups: Vec<DayBriefSeed>,
    pub next_step: Option<DayBriefSeed>,
    /// `(category, score, quiet)` from Pulse reactions.
    pub category_scores: Vec<(String, i32, bool)>,
    pub tone: BriefTone,
}

/// One ranked item on the brief.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DayBriefItem {
    pub section: DayBriefSection,
    pub title: String,
    pub body: String,
    pub prompt: String,
    pub category: String,
    pub score: i32,
}

/// One generated start-my-day brief. Empty sections never appear.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DayBrief {
    pub day_key: String,
    pub generated_at: u64,
    pub greeting: String,
    pub items: Vec<DayBriefItem>,
    pub closing: String,
}

impl DayBrief {
    pub fn source_id(&self) -> String {
        format!("{CARD_SOURCE_PREFIX}{}", self.day_key)
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Short Pulse body: greeting + up to a few item lines.
    pub fn card_body(&self) -> String {
        let mut lines = vec![self.greeting.clone()];
        for item in self.items.iter().take(ITEM_CAP) {
            lines.push(format!("{} · {}", item.section.short(), item.title));
        }
        if self.items.is_empty() {
            lines.push(self.closing.clone());
        }
        lines.join("\n")
    }

    /// Full markdown for card `details`. Omits empty sections. Groups by
    /// section while keeping score rank inside each section.
    pub fn render(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!("# {CARD_TITLE}\n\n"));
        out.push_str(&format!("{}\n\n", self.greeting));
        let mut ordered = self.items.clone();
        ordered.sort_by(|a, b| {
            a.section
                .cmp(&b.section)
                .then(b.score.cmp(&a.score))
                .then(a.title.cmp(&b.title))
        });
        let mut last: Option<DayBriefSection> = None;
        for item in &ordered {
            if last != Some(item.section) {
                out.push_str(&format!("## {}\n\n", item.section.heading()));
                last = Some(item.section);
            }
            out.push_str(&format!("- **{}**", item.title));
            if !item.body.is_empty() {
                out.push_str(&format!(" — {}", item.body));
            }
            out.push('\n');
            if !item.prompt.is_empty() {
                out.push_str(&format!("  - One-tap: {}\n", item.prompt));
            }
        }
        if self.items.is_empty() {
            out.push_str("_Nothing proactive queued for today._\n\n");
        } else {
            out.push('\n');
        }
        out.push_str(&self.closing);
        out.push('\n');
        out
    }

    /// `(label, prompt)` pairs for per-item one-taps. Label is the short
    /// section + title clip.
    pub fn one_taps(&self) -> Vec<(String, String)> {
        self.items
            .iter()
            .filter(|i| !i.prompt.trim().is_empty())
            .map(|i| {
                let label = format!("{}: {}", i.section.short(), clip(&i.title, 36));
                (label, i.prompt.clone())
            })
            .collect()
    }

    /// Primary Run prompt: the top-ranked item, if any.
    pub fn primary_prompt(&self) -> Option<String> {
        self.items
            .iter()
            .find(|i| !i.prompt.trim().is_empty())
            .map(|i| i.prompt.clone())
    }
}

/// Whether a card's source id belongs to this brief.
pub fn is_day_brief_card(source_id: &str) -> bool {
    source_id.starts_with(CARD_SOURCE_PREFIX)
}

/// Whether the daily brief may post now: new day key, at the usual active
/// hour (or the brief hour when set). When rhythm has not learned yet, the
/// first open of the day posts immediately.
pub fn day_brief_due(
    last_day: Option<&str>,
    today: &str,
    clock: &LocalClock,
    usually_active: bool,
    brief_hour: Option<u32>,
    rhythm_enough: bool,
) -> bool {
    if last_day == Some(today) || today.is_empty() {
        return false;
    }
    delivery_ok(clock, usually_active, brief_hour, rhythm_enough)
}

/// Hour gate only (day key checked by the caller).
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
    true
}

/// Build the brief. Empty / quiet / off signals produce no section. Items are
/// ranked by reaction score (higher first); quiet categories drop; at most
/// [`ITEM_CAP`] survive.
pub fn build_brief(input: &DayBriefInputs) -> DayBrief {
    let mut scored: Vec<DayBriefItem> = Vec::new();
    push_opt(
        &mut scored,
        DayBriefSection::Resume,
        input.resume.as_ref(),
        input,
    );
    for seed in &input.watchers {
        push_seed(&mut scored, DayBriefSection::Watcher, seed, input);
    }
    for seed in &input.game_news {
        push_seed(&mut scored, DayBriefSection::GameNews, seed, input);
    }
    for seed in &input.prints {
        push_seed(&mut scored, DayBriefSection::Print, seed, input);
    }
    for seed in &input.backups {
        push_seed(&mut scored, DayBriefSection::Backup, seed, input);
    }
    push_opt(
        &mut scored,
        DayBriefSection::NextStep,
        input.next_step.as_ref(),
        input,
    );

    scored.retain(|i| !category_quiet(input, &i.category));
    scored.sort_by(|a, b| {
        b.score
            .cmp(&a.score)
            .then(a.section.cmp(&b.section))
            .then(a.title.cmp(&b.title))
    });
    scored.truncate(ITEM_CAP);

    let has_signal = !scored.is_empty();
    DayBrief {
        day_key: input.day_key.clone(),
        generated_at: input.generated_at,
        greeting: greeting_line(&input.tone, &input.day_key, has_signal),
        items: scored,
        closing: closing_line(&input.tone),
    }
}

fn push_opt(
    out: &mut Vec<DayBriefItem>,
    section: DayBriefSection,
    seed: Option<&DayBriefSeed>,
    input: &DayBriefInputs,
) {
    if let Some(seed) = seed {
        push_seed(out, section, seed, input);
    }
}

fn push_seed(
    out: &mut Vec<DayBriefItem>,
    section: DayBriefSection,
    seed: &DayBriefSeed,
    input: &DayBriefInputs,
) {
    let title = seed.title.trim();
    if title.is_empty() {
        return;
    }
    let category = if seed.category.trim().is_empty() {
        section.default_category().to_string()
    } else {
        seed.category.trim().to_string()
    };
    let score = category_score(input, &category);
    out.push(DayBriefItem {
        section,
        title: title.to_string(),
        body: seed.body.trim().to_string(),
        prompt: seed.prompt.trim().to_string(),
        category,
        score,
    });
}

fn category_score(input: &DayBriefInputs, category: &str) -> i32 {
    input
        .category_scores
        .iter()
        .find(|(k, _, _)| k.eq_ignore_ascii_case(category))
        .map(|(_, s, _)| *s)
        .unwrap_or(0)
}

fn category_quiet(input: &DayBriefInputs, category: &str) -> bool {
    input
        .category_scores
        .iter()
        .find(|(k, _, _)| k.eq_ignore_ascii_case(category))
        .map(|(_, _, q)| *q)
        .unwrap_or(false)
}

fn greeting_line(tone: &BriefTone, day_key: &str, has_signal: bool) -> String {
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
            Verbosity::Terse => format!("Start my day{who}{spark}"),
            Verbosity::Chatty => format!(
                "Here's your start-my-day brief for {day_key}{who} — a few things worth a look{spark}"
            ),
            Verbosity::Normal => format!("Start my day ({day_key}){who}{spark}"),
        }
    } else if tone.roast {
        format!("Quiet morning ({day_key}){who} — even the watchers slept in{spark}")
    } else {
        format!("Quiet morning ({day_key}){who} — nothing proactive queued{spark}")
    };
    if tone.verbosity == Verbosity::Chatty && !tone.name.is_empty() {
        format!("{} here. {body}", tone.name)
    } else {
        body
    }
}

fn closing_line(tone: &BriefTone) -> String {
    match (tone.roast, tone.verbosity) {
        (true, Verbosity::Terse) => "Or don't. Not today works too.".into(),
        (true, _) => "Tap one, or hit Not today and go be mysterious.".into(),
        (false, Verbosity::Terse) => "Not today dismisses the brief.".into(),
        (false, Verbosity::Chatty) => {
            "Each line has a one-tap. Not today dismisses the whole brief.".into()
        }
        (false, Verbosity::Normal) => "One-tap a line, or Not today to dismiss.".into(),
    }
}

fn clip(s: &str, max: usize) -> String {
    let t = s.trim();
    if t.chars().count() <= max {
        return t.to_string();
    }
    let mut out: String = t.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone() -> BriefTone {
        BriefTone::default()
    }

    fn seed(title: &str, cat: &str) -> DayBriefSeed {
        DayBriefSeed::new(title, format!("{title} body"), format!("Do {title}"), cat)
    }

    fn fixture_full() -> DayBriefInputs {
        DayBriefInputs {
            day_key: "2026-10-28".into(),
            generated_at: 1,
            resume: Some(seed("grokhub on feat-x", "coding next step")),
            watchers: vec![seed("Steam patch for Hades", "system")],
            game_news: vec![seed("Celeste 50% off", "game news")],
            prints: vec![seed("Benchy finished", "print status")],
            backups: vec![seed("Notes backup due", "backups")],
            next_step: Some(seed("Prep standup", "suggestions")),
            category_scores: vec![
                ("game news".into(), 5, false),
                ("coding next step".into(), 3, false),
                ("system".into(), 2, false),
                ("suggestions".into(), 1, false),
                ("print status".into(), 0, false),
                ("backups".into(), -1, false),
            ],
            tone: tone(),
        }
    }

    #[test]
    fn builds_from_fixture_sources_and_caps_at_five() {
        let brief = build_brief(&fixture_full());
        assert_eq!(brief.items.len(), ITEM_CAP);
        assert_eq!(brief.source_id(), "day-brief:2026-10-28");
        // Ranked by score: game news 5, resume 3, watcher 2, next 1, print 0.
        // Backups (-1) drops past the cap.
        let titles: Vec<&str> = brief.items.iter().map(|i| i.title.as_str()).collect();
        assert!(titles.contains(&"Celeste 50% off"), "{titles:?}");
        assert!(titles.contains(&"grokhub on feat-x"), "{titles:?}");
        assert!(titles.contains(&"Steam patch for Hades"), "{titles:?}");
        assert!(titles.contains(&"Prep standup"), "{titles:?}");
        assert!(titles.contains(&"Benchy finished"), "{titles:?}");
        assert!(
            !titles.iter().any(|t| t.contains("Notes backup")),
            "sixth item dropped by cap: {titles:?}"
        );
        let md = brief.render();
        assert!(md.contains("## Game news and sales"), "{md}");
        assert!(md.contains("## Pick up where you left off"), "{md}");
        assert!(md.contains("One-tap:"), "{md}");
        assert_eq!(brief.one_taps().len(), ITEM_CAP);
        // Highest reaction score wins the primary Run one-tap.
        assert_eq!(
            brief.primary_prompt().as_deref(),
            Some("Do Celeste 50% off")
        );
        assert_eq!(brief.items[0].title, "Celeste 50% off");
    }

    #[test]
    fn empty_sections_are_hidden() {
        let brief = build_brief(&DayBriefInputs {
            day_key: "2026-10-28".into(),
            generated_at: 1,
            resume: Some(seed("my-app", "coding next step")),
            next_step: Some(seed("Tidy Downloads", "suggestions")),
            tone: tone(),
            ..DayBriefInputs::default()
        });
        assert_eq!(brief.items.len(), 2);
        let md = brief.render();
        assert!(md.contains("## Pick up where you left off"), "{md}");
        assert!(md.contains("## Suggested next step"), "{md}");
        assert!(!md.contains("## Watcher alerts"), "{md}");
        assert!(!md.contains("## Game news"), "{md}");
        assert!(!md.contains("## Print status"), "{md}");
        assert!(!md.contains("## Backups due"), "{md}");
        let body = brief.card_body();
        assert!(body.contains("Resume ·"), "{body}");
        assert!(body.contains("Next ·"), "{body}");
        assert!(!body.contains("Watchers"), "{body}");
    }

    #[test]
    fn quiet_categories_drop_and_ranking_follows_scores() {
        let mut input = fixture_full();
        input.prints.clear();
        input.backups.clear();
        input.category_scores = vec![
            ("game news".into(), 8, false),
            ("coding next step".into(), 4, false),
            ("system".into(), 9, true), // quiet → drop
            ("suggestions".into(), 1, false),
        ];
        let brief = build_brief(&input);
        let titles: Vec<&str> = brief.items.iter().map(|i| i.title.as_str()).collect();
        assert!(
            !titles.iter().any(|t| t.contains("Steam patch")),
            "quiet watcher dropped: {titles:?}"
        );
        // Score order on the card: game news (8), resume (4), next (1).
        assert_eq!(brief.items[0].section, DayBriefSection::GameNews);
        assert_eq!(brief.items[0].score, 8);
        assert_eq!(brief.items[1].section, DayBriefSection::Resume);
        assert_eq!(brief.items[1].score, 4);
        assert_eq!(brief.items[2].section, DayBriefSection::NextStep);
        let md = brief.render();
        // Render still groups by section (Resume heading before Games).
        let resume_at = md.find("## Pick up where you left off").expect("resume");
        let games_at = md.find("## Game news and sales").expect("games");
        assert!(resume_at < games_at, "{md}");
    }

    #[test]
    fn shows_once_per_day_at_active_or_brief_hour() {
        let clock = LocalClock {
            now_ms: 1,
            weekday: 3,
            hour: 9,
            minute: 0,
        };
        assert!(day_brief_due(None, "2026-10-28", &clock, true, None, true));
        assert!(!day_brief_due(
            Some("2026-10-28"),
            "2026-10-28",
            &clock,
            true,
            None,
            true
        ));
        // Rhythm learned, not usually active, no brief hour → hold.
        assert!(!day_brief_due(
            None,
            "2026-10-28",
            &LocalClock { hour: 14, ..clock },
            false,
            None,
            true
        ));
        // Brief hour match.
        assert!(day_brief_due(
            None,
            "2026-10-28",
            &LocalClock { hour: 8, ..clock },
            false,
            Some(8),
            true
        ));
        // Rhythm not learned yet → first open of the day, any hour.
        assert!(day_brief_due(
            None,
            "2026-10-28",
            &LocalClock { hour: 3, ..clock },
            false,
            None,
            false
        ));
        assert!(!day_brief_due(None, "", &clock, true, None, false));
    }

    #[test]
    fn personality_shapes_greeting_and_card_helpers() {
        let mut input = DayBriefInputs {
            day_key: "2026-10-28".into(),
            generated_at: 1,
            resume: Some(seed("alpha", "coding next step")),
            tone: BriefTone {
                name: "Butler".into(),
                verbosity: Verbosity::Chatty,
                emoji: Emoji::None,
                address: Some("sir".into()),
                roast: false,
            },
            ..DayBriefInputs::default()
        };
        let brief = build_brief(&input);
        assert!(brief.greeting.contains("Butler"), "{}", brief.greeting);
        assert!(brief.greeting.contains("sir"), "{}", brief.greeting);
        assert!(is_day_brief_card(&brief.source_id()));
        assert!(!is_day_brief_card("systems-brief:2026-10-28"));

        input.resume = None;
        input.tone.roast = true;
        input.tone.verbosity = Verbosity::Normal;
        let quiet = build_brief(&input);
        assert!(quiet.is_empty());
        assert!(
            quiet.greeting.contains("Quiet morning"),
            "{}",
            quiet.greeting
        );
        assert!(
            quiet.render().contains("Nothing proactive"),
            "{}",
            quiet.render()
        );
    }
}
