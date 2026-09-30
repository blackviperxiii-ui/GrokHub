//! Ideas the model writes for this person: automations, reminders, skills, and
//! things to try, grounded in what they actually work on.
//!
//! The cabin used to fill the Ideas board from templates ("A chip for the next
//! step", "Set up: exit 1 · 9ms"). Those never said anything about the person's
//! work. Ideas now come from one model call over their memory, recent asks, open
//! cards, and what they already run, and every line is checked before it posts.

use serde::{Deserialize, Serialize};

use crate::redact::is_plain_text;
use crate::situation::echoes_source;

/// At most this many ideas per refresh.
pub const IDEAS_PER_RUN: usize = 4;
/// Ask again at most this often.
pub const IDEAS_REFRESH_MS: u64 = 6 * 60 * 60 * 1000;
/// With this many live generated ideas on the board, do not ask for more.
pub const IDEAS_LIVE_ENOUGH: usize = 4;
/// The short line under a card's title.
pub const SHORT_CHARS: usize = 110;
/// Source id on cards the model wrote.
pub const IDEA_SOURCE_GENERATED: &str = "gen";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum IdeaKind {
    /// Runs on a clock or an interval.
    Automation,
    /// A one-time or repeating nudge.
    Reminder,
    /// A saved procedure for work that repeats.
    Skill,
    /// Something to ask the cabin right now.
    Try,
}

impl IdeaKind {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "automation" | "automate" | "schedule" => Some(Self::Automation),
            "reminder" | "remind" => Some(Self::Reminder),
            "skill" => Some(Self::Skill),
            "try" | "chat" | "ask" | "action" => Some(Self::Try),
            _ => None,
        }
    }

    /// The type shown first on a card: Automation, Skill, or Suggestion.
    /// A reminder is a scheduled nudge, so it reads as an automation.
    pub fn type_label(self) -> &'static str {
        match self {
            Self::Automation | Self::Reminder => "Automation",
            Self::Skill => "Skill",
            Self::Try => "Suggestion",
        }
    }

    /// Short line shown as the card's "why this" label.
    pub fn why_label(self) -> &'static str {
        match self {
            Self::Automation => "An automation you can turn on",
            Self::Reminder => "A reminder for your work",
            Self::Skill => "A skill for work that repeats",
            Self::Try => "Something to try now",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdeaSeed {
    pub kind: IdeaKind,
    pub title: String,
    /// One short line under the title.
    pub body: String,
    /// The full explanation shown when the card opens.
    pub details: String,
    /// The exact message to send the cabin to do it.
    pub prompt: String,
}

/// What the idea call may look at. Every field is optional.
#[derive(Debug, Clone, Default)]
pub struct IdeaContext<'a> {
    pub user_md: &'a str,
    pub memory_md: &'a str,
    /// Their recent asks, newest first.
    pub recent_asks: &'a [String],
    pub open_cards: &'a [String],
    pub automations: &'a [String],
    pub skills: &'a [String],
    /// Idea titles on the board now, and ones they turned down.
    pub existing: &'a [String],
    pub rejected: &'a [String],
    pub hour: u8,
    pub weekday: &'a str,
}

fn clip(s: &str, n: usize) -> String {
    let t = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if t.chars().count() <= n {
        t
    } else {
        let mut out: String = t.chars().take(n.saturating_sub(1)).collect();
        out.push('…');
        out
    }
}

fn push_list(lines: &mut Vec<String>, head: &str, items: &[String], max: usize, each: usize) {
    let picked: Vec<String> = items
        .iter()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty() && is_plain_text(s))
        .take(max)
        .map(|s| format!("- {}", clip(s, each)))
        .collect();
    if picked.is_empty() {
        return;
    }
    lines.push(format!("{head}:"));
    lines.extend(picked);
}

/// One prompt. The reply is only `IDEA:` lines.
pub fn idea_prompt(ctx: &IdeaContext) -> String {
    let mut lines: Vec<String> = vec![
        "You suggest ideas for one person who uses GrokHub, a desktop assistant on their own computer.".into(),
        "GrokHub can: run shell commands and edit files on this computer, drive the desktop, search the web, \
         schedule automations on a clock (\"every weekday at 9, ...\") or an interval (\"every 2 hours, ...\"), \
         keep skills (saved step-by-step procedures), manage a workboard of tasks, and make images."
            .into(),
        String::new(),
        format!(
            "Suggest up to {IDEAS_PER_RUN} ideas that would genuinely save them time or help their work this week."
        ),
        "Base every idea on what they actually do, from the context below. Be concrete: name the project, file, \
         tool, or routine it is about. Prefer an automation that removes an ask they keep repeating, a reminder \
         tied to real work, a skill for a procedure they redo, or one thing to try now that moves their work forward."
            .into(),
        "Good: \"automation | Morning test run | Your GrokHub tests run before you sit down. | Every weekday at 8 \
         the cabin runs the GrokHub tests and posts any failure to your feed, so you start on red instead of \
         finding it later. Needs the repo at ~/GrokHub. | every weekday at 8, run cargo test in ~/GrokHub and \
         summarize failures\"."
            .into(),
        "Bad: generic advice, anything about chips, restating a sentence they typed, \"Set up: ...\", \"A next step\", \
         productivity tips that ignore their work."
            .into(),
        String::new(),
        "Output only lines in exactly this form, nothing else:".into(),
        "IDEA: kind | title | short description | details | what to send".into(),
        "- kind: automation, reminder, skill, or try".into(),
        "- title: at most 60 characters, names the help".into(),
        "- short description: one line to them (\"you\"), at most 90 characters".into(),
        "- details: two to four sentences: what it does, why it helps them, and what it needs, at most 500 characters".into(),
        "- what to send: the exact message to send GrokHub to do it. An automation starts with its schedule."
            .into(),
        "Do not copy their sentences. Do not repeat anything listed under Existing or Turned down. \
         If nothing would really help, output nothing."
            .into(),
        String::new(),
        format!("Now: hour {} on {}", ctx.hour, if ctx.weekday.is_empty() { "a weekday" } else { ctx.weekday }),
    ];
    if !ctx.user_md.trim().is_empty() && is_plain_text(ctx.user_md) {
        lines.push("About them (USER.md):".into());
        lines.push(clip(ctx.user_md, 500));
    }
    if !ctx.memory_md.trim().is_empty() && is_plain_text(ctx.memory_md) {
        lines.push("What the cabin remembers (MEMORY.md):".into());
        lines.push(clip(ctx.memory_md, 500));
    }
    push_list(&mut lines, "Their recent asks, newest first", ctx.recent_asks, 15, 140);
    push_list(&mut lines, "Open workboard cards", ctx.open_cards, 8, 80);
    push_list(&mut lines, "Automations they already run", ctx.automations, 12, 80);
    push_list(&mut lines, "Skills they already have", ctx.skills, 12, 60);
    push_list(&mut lines, "Existing ideas", ctx.existing, 12, 60);
    push_list(&mut lines, "Turned down", ctx.rejected, 12, 60);
    lines.join("\n")
}

/// Titles the old template generators wrote. An untouched card with one of these goes.
pub fn is_template_idea_title(title: &str) -> bool {
    let t = title.trim().to_ascii_lowercase();
    const EXACT: &[&str] = &[
        "morning reminder",
        "nightly wrap-up",
        "remind me later",
        "ping me when i sit down",
        "a next step from the brief",
        "a reminder that runs on its own",
        "an end-of-day wrap",
        "a friday routine",
        "a morning routine",
        "a night routine",
        "a way of working to keep",
        "idea",
    ];
    const STARTS: &[&str] = &["set up:", "setup:", "a chip for", "a chip to"];
    EXACT.contains(&t.as_str()) || STARTS.iter().any(|p| t.starts_with(p))
}

fn generic_title(t: &str) -> bool {
    let l = t.to_ascii_lowercase();
    is_template_idea_title(t)
        || l.contains("chip")
        || l.starts_with("a next step")
        || l.starts_with("next step")
        || l.starts_with("idea")
        || l == "automation"
        || l == "reminder"
}

fn schedule_like(prompt: &str) -> bool {
    let l = prompt.to_ascii_lowercase();
    l.starts_with("every ")
        || l.starts_with("each ")
        || l.starts_with("/loop ")
        || l.contains(" every ")
        || l.starts_with("daily")
        || l.starts_with("weekdays")
        || l.starts_with("on weekdays")
        || l.starts_with("tomorrow")
        || l.starts_with("at ")
        || l.starts_with("remind me")
        || l.contains(" at ")
}

/// Parse the reply. Anything generic, echoed, duplicated, or unsafe is dropped.
/// `sources` are their own sentences (asks, memory lines) that an idea must not copy.
/// `taken` are titles already on the board or turned down.
pub fn parse_ideas(raw: &str, sources: &[String], taken: &[String]) -> Vec<IdeaSeed> {
    let mut out: Vec<IdeaSeed> = Vec::new();
    for line in raw.lines() {
        if out.len() >= IDEAS_PER_RUN {
            break;
        }
        let line = line.trim().trim_start_matches(['-', '*', ' ']);
        let rest = match line.get(..5) {
            Some(head) if head.eq_ignore_ascii_case("idea:") => &line[5..],
            _ => continue,
        };
        let parts: Vec<&str> = rest.splitn(5, '|').map(str::trim).collect();
        // Five fields; an older four-field reply has no separate details.
        let (kind, title, body, details, prompt) = match parts.as_slice() {
            [k, t, b, d, p] => (*k, *t, *b, *d, *p),
            [k, t, b, p] => (*k, *t, *b, *b, *p),
            _ => continue,
        };
        let Some(kind) = IdeaKind::parse(kind) else {
            continue;
        };
        let title = title.trim_matches(['"', '\'', '*']).trim().to_string();
        let body = clip(body.trim_matches(['"', '\'']).trim(), SHORT_CHARS);
        let details = details.trim_matches(['"', '\'']).trim().to_string();
        let prompt = prompt.trim_matches(['"', '\'', '`']).trim().to_string();
        let t_len = title.chars().count();
        let b_len = body.chars().count();
        let d_len = details.chars().count();
        let p_len = prompt.chars().count();
        if !(6..=70).contains(&t_len)
            || !(12..=SHORT_CHARS).contains(&b_len)
            || !(12..=700).contains(&d_len)
            || !(8..=400).contains(&p_len)
        {
            continue;
        }
        if !is_plain_text(&title)
            || !is_plain_text(&body)
            || !is_plain_text(&details)
            || !is_plain_text(&prompt)
        {
            continue;
        }
        if generic_title(&title) {
            continue;
        }
        if matches!(kind, IdeaKind::Automation) && !schedule_like(&prompt) {
            continue;
        }
        let echoed = sources.iter().any(|s| {
            let s = s.trim();
            !s.is_empty()
                && (echoes_source(&title, s)
                    || echoes_source(&body, s)
                    || prompt.trim().eq_ignore_ascii_case(s))
        });
        if echoed {
            continue;
        }
        let dup = taken
            .iter()
            .chain(out.iter().map(|i| &i.title))
            .any(|t| t.trim().eq_ignore_ascii_case(&title));
        if dup {
            continue;
        }
        out.push(IdeaSeed {
            kind,
            title,
            body,
            details,
            prompt,
        });
    }
    out
}

/// Ask for ideas when the board is thin and the last ask is old enough.
pub fn ideas_refresh_due(last_ms: u64, now_ms: u64, live_generated: usize) -> bool {
    live_generated < IDEAS_LIVE_ENOUGH
        && (last_ms == 0 || now_ms.saturating_sub(last_ms) >= IDEAS_REFRESH_MS)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx<'a>(asks: &'a [String], autos: &'a [String]) -> IdeaContext<'a> {
        IdeaContext {
            user_md: "Name: Viper. Builds GrokHub, a Rust desktop app. Streams on Fridays.",
            memory_md: "Uses Arch Linux. Releases go out through AUR.",
            recent_asks: asks,
            automations: autos,
            hour: 9,
            weekday: "Tuesday",
            ..Default::default()
        }
    }

    #[test]
    fn prompt_carries_their_work_and_the_output_contract() {
        let asks = vec!["run the grokhub tests again".to_string(), "bump the AUR pkgver".to_string()];
        let autos = vec!["Nightly wrap".to_string()];
        let p = idea_prompt(&ctx(&asks, &autos));
        assert!(p.contains("IDEA: kind | title | short description | details | what to send"));
        assert!(p.contains("- run the grokhub tests again"));
        assert!(p.contains("Automations they already run:\n- Nightly wrap"));
        assert!(p.contains("Builds GrokHub"));
        assert!(p.contains("hour 9 on Tuesday"));
        let secret = vec!["token sk-abcdefghijklmnopqrstuv".to_string()];
        assert!(!idea_prompt(&ctx(&secret, &[])).contains("sk-abcdef"), "secrets stay out");
    }

    #[test]
    fn parser_keeps_concrete_ideas_and_drops_the_rest() {
        let raw = "\
Here are some ideas:
IDEA: automation | Morning test run | Every weekday at 8 the GrokHub tests run and failures land on your feed, so you start the day knowing. | every weekday at 8, run cargo test in ~/GrokHub and summarize failures
IDEA: skill | AUR release checklist | You bump pkgver and rebuild by hand each release; a skill does the same steps in order every time. | save a skill that bumps pkgver in packaging/aur/PKGBUILD, updates .SRCINFO, and runs makepkg
IDEA: try | A chip for the next step | A chip takes the next step for you. | take the next step
IDEA: automation | Friday stream prep | Your stream setup is ready before you go live on Fridays. | set up my stream
IDEA: reminder | run the grokhub tests again | You asked this a lot. | run the grokhub tests again
IDEA: try | Set up: exit 1 · 9ms | Turn this into a reminder or an automation. | run debug
IDEA: bogus | Something | This kind is not allowed here at all. | do it
IDEA: try | Morning test run | Duplicate title should be dropped by the parser. | anything else
";
        let sources = vec!["run the grokhub tests again".to_string()];
        let ideas = parse_ideas(raw, &sources, &[]);
        let titles: Vec<&str> = ideas.iter().map(|i| i.title.as_str()).collect();
        assert_eq!(titles, vec!["Morning test run", "AUR release checklist"], "{ideas:?}");
        assert_eq!(ideas[0].kind, IdeaKind::Automation);
        assert!(ideas[0].prompt.starts_with("every weekday at 8"));
        assert_eq!(ideas[1].kind, IdeaKind::Skill);
        let taken = vec!["morning test run".to_string()];
        assert_eq!(parse_ideas(raw, &sources, &taken).len(), 1, "a title already on the board is skipped");
        assert!(parse_ideas("I'll think about it.", &[], &[]).is_empty());
    }

    #[test]
    fn template_titles_are_recognized() {
        for t in [
            "Set up: run debug",
            "Set up: exit 1 · 9ms",
            "A chip for the next slice",
            "A chip for when something fails",
            "Ping me when I sit down",
            "Remind me later",
            "A next step from the brief",
        ] {
            assert!(is_template_idea_title(t), "{t}");
        }
        assert!(!is_template_idea_title("Morning test run"));
    }

    #[test]
    fn refresh_waits_for_a_thin_board_and_the_interval() {
        assert!(ideas_refresh_due(0, 10, 0));
        assert!(!ideas_refresh_due(0, 10, IDEAS_LIVE_ENOUGH));
        assert!(!ideas_refresh_due(1_000, 1_000 + IDEAS_REFRESH_MS - 1, 0));
        assert!(ideas_refresh_due(1_000, 1_000 + IDEAS_REFRESH_MS, 0));
    }

    #[test]
    fn five_field_ideas_keep_a_short_line_and_full_details() {
        let raw = "IDEA: try | Triage open issues | Sort this week's GitHub issues by what blocks a release. | The cabin reads your open GrokHub issues, groups them by release impact, and files the top three as Todo cards so the next session starts on what matters. | list my open GrokHub issues and file the three that block the release as workboard cards";
        let ideas = parse_ideas(raw, &[], &[]);
        assert_eq!(ideas.len(), 1, "{ideas:?}");
        assert_eq!(ideas[0].kind.type_label(), "Suggestion");
        assert!(ideas[0].body.chars().count() <= SHORT_CHARS);
        assert!(ideas[0].details.starts_with("The cabin reads your open GrokHub issues"));
        assert_eq!(IdeaKind::Reminder.type_label(), "Automation");
        assert_eq!(IdeaKind::Skill.type_label(), "Skill");
        let json = serde_json::to_string(&IdeaKind::Try).unwrap();
        assert_eq!(json, "\"try\"");
    }
}
