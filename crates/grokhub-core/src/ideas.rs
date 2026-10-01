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
    /// Why this is worth making: the pattern in their work it answers.
    /// Shown on the card. Empty when an older reply left it out.
    pub reason: String,
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
        "Before you suggest anything, ask yourself: why would this person want it, and would they miss it if it \
         were not there? Most of the time the honest answer is no, and the right output is nothing."
            .into(),
        format!(
            "Suggest at most {IDEAS_PER_RUN} ideas, and only ones that clear every rule below:"
        ),
        "- It answers work they REPEAT: a pattern marked (2x) or more below, an open workboard card, or something \
         their memory says is ongoing. One ask is not a pattern."
            .into(),
        "- A one-time task gets nothing. Installing a driver or a package, fixing one bug, setting something up \
         once, or a task that is already done does not need an automation, a skill, a reminder, or a follow-up \
         idea, even when it took many messages or failed and was retried."
            .into(),
        "- One idea per need. Never write an automation AND a skill AND a reminder about the same thing: pick the \
         single kind that fits. A skill is for a procedure they redo by hand; an automation is for something that \
         should happen on a schedule without them asking; a reminder is for a real deadline or habit; try is one \
         concrete thing to do now that moves open work forward."
            .into(),
        "- Be concrete: name the project, file, tool, or routine. No generic advice or productivity tips.".into(),
        "Good: \"automation | Morning test run | Your GrokHub tests run before you sit down. | You asked to run \
         the GrokHub tests on 4 different days this week. | Every weekday at 8 the cabin runs the GrokHub tests \
         and posts any failure to your feed, so you start on red instead of finding it later. | every weekday at \
         8, run cargo test in ~/GrokHub and summarize failures\"."
            .into(),
        "Bad: \"automation | Driver update check\" after they installed a driver once; a skill and an automation \
         for the same ask; anything about chips; \"Set up: ...\"; \"A next step\"; restating a sentence they typed."
            .into(),
        String::new(),
        "Output only lines in exactly this form, nothing else:".into(),
        "IDEA: kind | title | short description | reason | details | what to send".into(),
        "- kind: automation, reminder, skill, or try".into(),
        "- title: at most 60 characters, names the help".into(),
        "- short description: one line to them (\"you\"), at most 90 characters".into(),
        "- reason: why you are making this, pointing at the repeated work it answers, at most 120 characters"
            .into(),
        "- details: two to four sentences: what it does, why it helps them, and what it needs, at most 500 characters".into(),
        "- what to send: the exact message to send GrokHub to do it. An automation starts with its schedule."
            .into(),
        "Do not copy their sentences. Do not repeat or overlap anything listed under Existing or Turned down. \
         If nothing clears the rules, output nothing."
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
    let patterns: Vec<String> = ask_patterns(ctx.recent_asks)
        .into_iter()
        .map(|p| {
            let tag = if p.one_off {
                format!("{}x, one-time task", p.count)
            } else {
                format!("{}x", p.count)
            };
            format!("({tag}) {}", p.example)
        })
        .collect();
    push_list(&mut lines, "What they asked lately, grouped by topic", &patterns, 15, 160);
    push_list(&mut lines, "Open workboard cards", ctx.open_cards, 8, 80);
    push_list(&mut lines, "Automations they already run", ctx.automations, 12, 80);
    push_list(&mut lines, "Skills they already have", ctx.skills, 12, 60);
    push_list(&mut lines, "Existing ideas", ctx.existing, 12, 60);
    push_list(&mut lines, "Turned down", ctx.rejected, 12, 60);
    lines.join("\n")
}

/// Words that say nothing about the topic of an ask.
const STOP_WORDS: &[&str] = &[
    "about", "after", "again", "also", "before", "could", "every", "from", "have", "help", "into",
    "just", "like", "make", "need", "please", "should", "some", "that", "than", "them", "then",
    "there", "these", "they", "this", "what", "when", "where", "which", "while", "will", "with",
    "would", "your", "yours", "mine", "it's", "want", "wanna", "gonna", "thing", "things", "still",
    "work", "working", "doesn't", "does", "done", "can't", "cant", "dont", "don't", "today",
    "tomorrow", "check", "show", "tell", "give", "using", "used", "being", "been", "were", "here",
    "each", "time", "times", "daily", "weekly", "weekday", "weekdays", "morning", "night", "hour",
    "hours", "minute", "minutes", "cabin",
];

/// Verbs of a job you do once. An ask led by one of these is not a routine.
const ONE_OFF_STEMS: &[&str] = &[
    "install", "reinstall", "uninstall", "setup", "configur", "fix", "repair", "troubleshoot",
    "debug", "download", "flash", "mount", "upgrad", "migrat", "recover", "restor", "reset",
    "unbrick", "partition", "format", "replac",
];

fn stem(word: &str) -> String {
    let w = word.trim_matches(|c: char| !c.is_ascii_alphanumeric());
    for suffix in ["ing", "ed", "es", "s"] {
        if w.len() > suffix.len() + 3 {
            if let Some(base) = w.strip_suffix(suffix) {
                return base.to_string();
            }
        }
    }
    w.to_string()
}

/// The words that name what a line is about: lowercased, stemmed, no filler.
pub fn topic_words(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for raw in text.to_ascii_lowercase().split(|c: char| !(c.is_ascii_alphanumeric() || c == '\'' || c == '-')) {
        let raw = raw.trim_matches(['\'', '-']);
        if raw.len() < 4 || STOP_WORDS.contains(&raw) || raw.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        let w = stem(raw);
        if w.len() >= 3 && !out.contains(&w) {
            out.push(w);
        }
    }
    out
}

/// Two lines are about the same thing when they share two topic words, or one
/// when either line has only one or two to begin with.
pub fn same_topic(a: &str, b: &str) -> bool {
    let wa = topic_words(a);
    let wb = topic_words(b);
    if wa.is_empty() || wb.is_empty() {
        return false;
    }
    let shared = wa.iter().filter(|w| wb.contains(w)).count();
    shared >= 2 || (shared == 1 && wa.len().min(wb.len()) <= 2)
}

/// An ask for a job you do once: install a driver, fix a bug, set something up.
pub fn is_one_off_ask(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    if lower.contains("set up") || lower.contains("set-up") {
        return true;
    }
    lower
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| !w.is_empty())
        .take(6)
        .any(|w| ONE_OFF_STEMS.iter().any(|s| w.starts_with(s)))
}

/// Their recent asks folded by topic, most repeated first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AskPattern {
    /// The newest ask on this topic.
    pub example: String,
    /// How many distinct asks share the topic.
    pub count: usize,
    /// Every ask on the topic is a one-time job.
    pub one_off: bool,
}

pub fn ask_patterns(asks: &[String]) -> Vec<AskPattern> {
    let mut groups: Vec<(String, Vec<&str>)> = Vec::new();
    for ask in asks.iter().map(|a| a.trim()).filter(|a| !a.is_empty()) {
        match groups.iter_mut().find(|(head, _)| same_topic(head, ask)) {
            Some((_, members)) => members.push(ask),
            None => groups.push((ask.to_string(), vec![ask])),
        }
    }
    let mut out: Vec<AskPattern> = groups
        .into_iter()
        .map(|(head, members)| AskPattern {
            one_off: members.iter().all(|m| is_one_off_ask(m)),
            count: members.len(),
            example: head,
        })
        .collect();
    // Stable: equal counts keep newest-first order.
    out.sort_by_key(|p| std::cmp::Reverse(p.count));
    out
}

/// What an idea may stand on: their recent asks, and lasting context (memory
/// and USER.md lines, open workboard cards).
#[derive(Debug, Clone, Copy, Default)]
pub struct IdeaGround<'a> {
    pub asks: &'a [String],
    pub lasting: &'a [String],
}

/// Does this idea answer something real? An automation, reminder, or skill needs
/// work they repeat (two or more asks on the topic that are not one-time jobs) or
/// lasting context. A Try needs at least one ask or lasting line on the topic that
/// is not a one-time job. An idea built on one install or one fix has no reason.
pub fn idea_has_reason(kind: IdeaKind, text: &str, ground: &IdeaGround) -> bool {
    let lasting = ground
        .lasting
        .iter()
        .any(|l| !l.trim().is_empty() && same_topic(text, l) && !is_one_off_ask(l));
    if lasting {
        return true;
    }
    let routine = ground
        .asks
        .iter()
        .filter(|a| same_topic(text, a) && !is_one_off_ask(a))
        .count();
    match kind {
        IdeaKind::Automation | IdeaKind::Reminder | IdeaKind::Skill => routine >= 2,
        IdeaKind::Try => routine >= 1,
    }
}

/// An idea that only answers a one-time job: every recent ask on its topic is
/// an install, a fix, or a setup, and nothing lasting backs it. Ideas with no
/// matching ask at all are left alone (their evidence may have aged out).
pub fn idea_from_one_off(text: &str, ground: &IdeaGround) -> bool {
    let lasting = ground
        .lasting
        .iter()
        .any(|l| same_topic(text, l) && !is_one_off_ask(l));
    if lasting {
        return false;
    }
    let mut matches = ground.asks.iter().filter(|a| same_topic(text, a)).peekable();
    matches.peek().is_some() && matches.all(|a| is_one_off_ask(a))
}

/// The words an idea is about, for the reason check and for one-per-topic.
pub fn idea_topic_text(seed: &IdeaSeed) -> String {
    format!("{} {} {}", seed.title, seed.body, seed.prompt)
}

/// Keep ideas that have a reason and say something new: one per topic in this
/// batch, and none on a topic already on the board or turned down (`taken`).
pub fn keep_reasoned_ideas(seeds: Vec<IdeaSeed>, ground: &IdeaGround, taken: &[String]) -> Vec<IdeaSeed> {
    let mut out: Vec<IdeaSeed> = Vec::new();
    for seed in seeds {
        let topic = idea_topic_text(&seed);
        if !idea_has_reason(seed.kind, &topic, ground) {
            continue;
        }
        if taken.iter().any(|t| same_topic(&seed.title, t)) {
            continue;
        }
        if out.iter().any(|o| same_topic(&idea_topic_text(o), &topic)) {
            continue;
        }
        out.push(seed);
    }
    out
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
        let parts: Vec<&str> = rest.splitn(6, '|').map(str::trim).collect();
        // Six fields; older replies have no reason (five) or no separate details (four).
        let (kind, title, body, reason, details, prompt) = match parts.as_slice() {
            [k, t, b, r, d, p] => (*k, *t, *b, *r, *d, *p),
            [k, t, b, d, p] => (*k, *t, *b, "", *d, *p),
            [k, t, b, p] => (*k, *t, *b, "", *b, *p),
            _ => continue,
        };
        let Some(kind) = IdeaKind::parse(kind) else {
            continue;
        };
        let title = title.trim_matches(['"', '\'', '*']).trim().to_string();
        let body = clip(body.trim_matches(['"', '\'']).trim(), SHORT_CHARS);
        let details = details.trim_matches(['"', '\'']).trim().to_string();
        let prompt = prompt.trim_matches(['"', '\'', '`']).trim().to_string();
        let reason = clip(reason.trim_matches(['"', '\'']).trim(), 160);
        if !reason.is_empty() && !is_plain_text(&reason) {
            continue;
        }
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
            reason,
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
        let asks = vec![
            "run the grokhub tests again".to_string(),
            "install the nvidia driver".to_string(),
            "run the tests before the release build".to_string(),
        ];
        let autos = vec!["Nightly wrap".to_string()];
        let p = idea_prompt(&ctx(&asks, &autos));
        assert!(p.contains("IDEA: kind | title | short description | reason | details | what to send"));
        assert!(p.contains("- (2x) run the grokhub tests again"), "{p}");
        assert!(p.contains("- (1x, one-time task) install the nvidia driver"), "{p}");
        assert!(p.contains("A one-time task gets nothing"));
        assert!(p.contains("One idea per need"));
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

    fn ideas_of(raw: &str) -> Vec<IdeaSeed> {
        parse_ideas(raw, &[], &[])
    }

    /// One driver install (that took a few tries) is not a reason for an
    /// automation, a skill, a reminder, or a follow-up.
    #[test]
    fn a_one_time_install_makes_no_ideas() {
        let asks = vec![
            "the nvidia driver install failed, try installing it again".to_string(),
            "install the nvidia 550 driver for my 4070".to_string(),
        ];
        let ground = IdeaGround { asks: &asks, lasting: &[] };
        let raw = "\
IDEA: automation | Driver update check | Your NVIDIA driver stays current without you asking. | You installed the nvidia driver. | Every Monday the cabin checks for a newer NVIDIA driver and tells you. | every monday at 9, check for a newer nvidia driver
IDEA: skill | NVIDIA driver install | A saved procedure for the nvidia driver install. | You installed the nvidia driver. | Steps to install the NVIDIA driver on Arch with the right kernel modules. | save a skill that installs the nvidia driver
IDEA: reminder | Reboot after driver | Reboot so the nvidia driver loads. | You installed the nvidia driver. | A reminder to reboot after the NVIDIA driver install so the module loads. | at 6pm remind me to reboot for the nvidia driver
IDEA: try | Verify the driver | Check that the nvidia driver loaded. | You installed the nvidia driver. | Runs nvidia-smi to confirm the NVIDIA driver loaded after the install. | run nvidia-smi and tell me if the nvidia driver loaded
";
        let parsed = ideas_of(raw);
        assert_eq!(parsed.len(), 4, "{parsed:?}");
        assert_eq!(parsed[0].reason, "You installed the nvidia driver.");
        assert!(keep_reasoned_ideas(parsed, &ground, &[]).is_empty(), "nothing to make from one install");
        for a in &asks {
            assert!(is_one_off_ask(a), "{a}");
        }
        assert!(!is_one_off_ask("summarize my open GitHub issues"));
        assert!(is_one_off_ask("can you set up the printer"));
    }

    #[test]
    fn repeated_work_earns_one_idea_per_topic() {
        let asks = vec![
            "run the grokhub tests and tell me what broke".to_string(),
            "summarize my open github issues".to_string(),
            "run the grokhub tests again before the release".to_string(),
            "run grokhub tests on the chips branch".to_string(),
        ];
        let ground = IdeaGround { asks: &asks, lasting: &[] };
        let raw = "\
IDEA: automation | Morning test run | Your GrokHub tests run before you sit down. | You asked to run the GrokHub tests three times. | Every weekday at 8 the cabin runs the GrokHub tests and posts failures. | every weekday at 8, run the grokhub tests and summarize failures
IDEA: skill | GrokHub test triage | Run the GrokHub tests and sort failures. | You run the GrokHub tests often. | A skill that runs the GrokHub tests and groups failures by file. | save a skill that runs the grokhub tests and groups failures
IDEA: automation | Issue digest | Your open GitHub issues each morning. | You asked about issues once. | Every morning the cabin lists your open GitHub issues. | every weekday at 9, summarize my open github issues
";
        let kept = keep_reasoned_ideas(ideas_of(raw), &ground, &[]);
        let titles: Vec<&str> = kept.iter().map(|i| i.title.as_str()).collect();
        assert_eq!(titles, vec!["Morning test run"], "one per topic; one ask is not a pattern");
        let taken = vec!["Nightly GrokHub tests".to_string()];
        assert!(
            keep_reasoned_ideas(ideas_of(raw), &ground, &taken).is_empty(),
            "a topic already on the board gets no second card"
        );
        let lasting = vec!["Triages open GitHub issues every Monday for the release".to_string()];
        let with_memory = IdeaGround { asks: &asks, lasting: &lasting };
        let kept = keep_reasoned_ideas(ideas_of(raw), &with_memory, &[]);
        assert!(kept.iter().any(|i| i.title == "Issue digest"), "lasting context is a reason: {kept:?}");
        let patterns = ask_patterns(&asks);
        assert_eq!(patterns[0].count, 3);
        assert!(!patterns[0].one_off);
    }

    #[test]
    fn untouched_one_off_ideas_are_recognized_for_cleanup() {
        let asks = vec!["install the nvidia driver".to_string(), "summarize my open github issues".to_string()];
        let ground = IdeaGround { asks: &asks, lasting: &[] };
        assert!(idea_from_one_off("Driver update check: every monday check the nvidia driver", &ground));
        assert!(!idea_from_one_off("Issue digest: summarize open github issues", &ground));
        assert!(!idea_from_one_off("Stream prep before Friday streams", &ground), "no matching ask: left alone");
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
