//! Empty-chat greeting blurb. Fast mode. Secrets never in the line.
//! Local line is a situation pick — time, name, signed-in, first-run, last project.

use crate::is_plain_text;

pub const GREETING_LLM_MODE: &str = "fast";
pub const GREETING_MAX_CHARS: usize = 92;
pub const GREETING_LLM_DEBOUNCE_MS: u64 = 800;

#[derive(Debug, Clone, Copy)]
pub struct GreetingInput<'a> {
    pub user_md: &'a str,
    pub memory_md: &'a str,
    pub insights: &'a [String],
    pub display_name: &'a str,
    pub hour: u8,
    pub last_night: &'a str,
    pub signed_in: bool,
    pub first_run: bool,
    pub last_project: &'a str,
    /// A paused workboard task. Empty when nothing is waiting.
    pub paused_task: &'a str,
}

/// Why this empty-home line was chosen. Tests lock the ranker, not the adjectives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GreetingKind {
    FirstRunUnsigned,
    FirstRunSigned,
    ReturningUnsigned,
    ReturningProject,
    ReturningQuiet,
}

pub fn is_cabin_first_run(get_started_done: bool, has_history: bool) -> bool {
    !get_started_done && !has_history
}

pub fn should_paint_greeting(empty_chat: bool, scratch: bool) -> bool {
    empty_chat && !scratch
}

pub fn greeting_name(user_md: &str, display_name: &str) -> String {
    for line in user_md.lines() {
        let raw = line.trim().trim_start_matches(['-', '*']).trim();
        let raw = raw.trim_matches('*').trim();
        let lower = raw.to_ascii_lowercase();
        if let Some(rest) = split_name_line(&lower, raw) {
            let n = clean_name(rest);
            if !n.is_empty() {
                return n;
            }
        }
    }
    for line in user_md.lines() {
        let t = line.trim();
        if let Some(rest) = t.strip_prefix('#') {
            let h = rest.trim().trim_start_matches('#').trim();
            if is_generic_heading(h) {
                continue;
            }
            let n = clean_name(h);
            if !n.is_empty() {
                return n;
            }
        }
    }
    clean_name(display_name)
}

fn split_name_line<'a>(lower: &str, raw: &'a str) -> Option<&'a str> {
    let is_name_key = lower.starts_with("name:")
        || lower.starts_with("name :")
        || lower.starts_with("name-")
        || lower.starts_with("name -")
        || lower.starts_with("name–")
        || lower.starts_with("name –");
    if !is_name_key {
        return None;
    }
    raw.split_once(':')
        .or_else(|| raw.split_once('–'))
        .or_else(|| raw.split_once('-'))
        .map(|(_, rest)| rest)
}

fn is_generic_heading(h: &str) -> bool {
    let t = h.trim().to_ascii_lowercase();
    t == "user"
        || t == "profile"
        || t.starts_with("user profile")
        || t == "who you are"
        || t == "long-term memory"
        || t == "memory"
}

fn is_placeholder_name(s: &str) -> bool {
    matches!(
        s.trim().to_ascii_lowercase().as_str(),
        "user" | "profile" | "guest" | "anonymous"
    )
}

fn is_product_name(s: &str) -> bool {
    let t = s.trim().to_ascii_lowercase();
    t.is_empty()
        || t == "grok"
        || t == "grokhub"
        || t == "grok hub"
        || t == "cabin"
        || t == "this computer"
        || t == "localhost"
        || t.contains("grokhub")
        || is_placeholder_name(&t)
        || is_machine_name(&t)
}

/// Hostnames and distro names must not become "Hello, CachyOS."
fn is_machine_name(s: &str) -> bool {
    let t = s.trim().to_ascii_lowercase();
    if t.is_empty() {
        return false;
    }
    if t.contains("cachy")
        || t.contains("x86")
        || t.contains("x64")
        || t == "arch"
        || t == "archlinux"
        || t == "ubuntu"
        || t == "fedora"
        || t == "debian"
        || t == "manjaro"
        || t == "nixos"
        || t == "linux"
        || t == "hostname"
    {
        return true;
    }
    t.contains('-') && t.chars().any(|c| c.is_ascii_digit())
}

fn clean_name(s: &str) -> String {
    let s = s.trim().trim_matches('"').trim_matches('*').trim();
    if s.is_empty() || !is_plain_text(s) || is_product_name(s) {
        return String::new();
    }
    let first: String = s
        .split_whitespace()
        .next()
        .unwrap_or("")
        .chars()
        .take(24)
        .collect();
    if first.chars().count() < 2 || is_product_name(&first) {
        String::new()
    } else {
        first
    }
}

/// Spoken time of day. Night stays "Hey" so it does not sound like goodbye.
fn hello_phrase(hour: u8) -> &'static str {
    match hour {
        5..=11 => "Good morning",
        12..=16 => "Good afternoon",
        17..=21 => "Good evening",
        _ => "Hey",
    }
}

/// Short titled project. Strips "Continue ", rejects machine / product / fail dumps.
pub fn clean_project_title(raw: &str) -> String {
    let t = raw.trim();
    let t = t
        .strip_prefix("Continue ")
        .or_else(|| t.strip_prefix("continue "))
        .unwrap_or(t)
        .trim();
    if t.is_empty() || !is_plain_text(t) || is_product_name(t) || is_machine_name(t) {
        return String::new();
    }
    let low = t.to_ascii_lowercase();
    if low.starts_with("failed:")
        || low.contains("failed:")
        || low == "chat"
        || low == "scratch"
        || low == "user"
        || low == "profile"
    {
        return String::new();
    }
    let words: Vec<&str> = t.split_whitespace().take(4).collect();
    let bit = words.join(" ");
    let bit: String = bit.chars().take(28).collect();
    let bit = bit
        .trim()
        .trim_end_matches(['.', ',', ';', ':'])
        .to_string();
    if bit.chars().count() < 2 || is_product_name(&bit) {
        String::new()
    } else {
        bit
    }
}

pub fn project_title_from_hint(hint: &str) -> String {
    clean_project_title(hint)
}

pub fn classify_greeting(input: &GreetingInput) -> GreetingKind {
    let project = clean_project_title(input.last_project);
    if input.first_run {
        if input.signed_in {
            GreetingKind::FirstRunSigned
        } else {
            GreetingKind::FirstRunUnsigned
        }
    } else if !input.signed_in {
        GreetingKind::ReturningUnsigned
    } else if !project.is_empty() {
        GreetingKind::ReturningProject
    } else {
        GreetingKind::ReturningQuiet
    }
}

pub fn local_greeting(input: &GreetingInput) -> String {
    let name = greeting_name(input.user_md, input.display_name);
    let hello = hello_phrase(input.hour);
    let project = clean_project_title(input.last_project);
    let line = match classify_greeting(input) {
        GreetingKind::FirstRunUnsigned => {
            format!("{hello}. Connect Grok and I'll be right here.")
        }
        GreetingKind::FirstRunSigned => {
            if name.is_empty() {
                format!("{hello}. I'm here whenever you are.")
            } else {
                format!("{hello}, {name}. I'm here whenever you are.")
            }
        }
        GreetingKind::ReturningUnsigned => {
            format!("{hello}. Sign in and we can pick up where we left off.")
        }
        GreetingKind::ReturningProject => {
            if let Some(line) = paused_line(hello, &name, input.paused_task) {
                line
            } else if name.is_empty() {
                format!("{hello}. {project} is still open if you want it.")
            } else {
                format!("{hello}, {name}. {project} is still open if you want it.")
            }
        }
        GreetingKind::ReturningQuiet => {
            if let Some(line) = paused_line(hello, &name, input.paused_task) {
                line
            } else if name.is_empty() {
                format!("{hello}. I'm here.")
            } else {
                format!("{hello}, {name}. I'm here.")
            }
        }
    };
    clip_greeting(&line)
}

fn paused_line(hello: &str, name: &str, task: &str) -> Option<String> {
    let task = task.trim();
    if task.is_empty() || !is_plain_text(task) || is_product_name(task) {
        return None;
    }
    let task: String = task.chars().take(36).collect();
    Some(if name.is_empty() {
        format!("{hello}. {task} is paused. I'll pick it up.")
    } else {
        format!("{hello}, {name}. {task} is paused. I'll pick it up.")
    })
}

pub fn greeting_fingerprint(input: &GreetingInput) -> String {
    let name = greeting_name(input.user_md, input.display_name);
    let project = clean_project_title(input.last_project);
    let user: String = input.user_md.trim().chars().take(80).collect();
    let mem: String = input.memory_md.trim().chars().take(80).collect();
    let ins = input.insights.first().cloned().unwrap_or_default();
    format!(
        "{}|{}|{}|{}|n{}|p{}|s{}|f{}|h{}",
        name,
        user,
        mem,
        ins.chars().take(40).collect::<String>(),
        input.last_night.chars().take(40).collect::<String>(),
        project,
        if input.signed_in { 1 } else { 0 },
        if input.first_run { 1 } else { 0 },
        input.hour / 6
    )
}

pub fn should_refresh_greeting(
    prev_fp: &str,
    next_fp: &str,
    last_at: u64,
    now_ms: u64,
    has_auth: bool,
    busy: bool,
) -> bool {
    has_auth
        && !busy
        && next_fp != prev_fp
        && now_ms.saturating_sub(last_at) >= GREETING_LLM_DEBOUNCE_MS
}

fn clip_greeting(s: &str) -> String {
    let t = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if t.chars().count() <= GREETING_MAX_CHARS {
        return t;
    }
    let mut out = String::new();
    for (i, ch) in t.chars().enumerate() {
        if i + 1 >= GREETING_MAX_CHARS {
            break;
        }
        out.push(ch);
    }
    let out = out.trim_end().trim_end_matches([',', ';', ':']);
    format!("{out}…")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input<'a>(
        user_md: &'a str,
        memory_md: &'a str,
        insights: &'a [String],
        display_name: &'a str,
        hour: u8,
    ) -> GreetingInput<'a> {
        GreetingInput {
            user_md,
            memory_md,
            insights,
            display_name,
            hour,
            last_night: "",
            signed_in: true,
            first_run: false,
            last_project: "",
            paused_task: "",
        }
    }

    #[test]
    fn greeting_llm_is_fast_mode() {
        assert_eq!(GREETING_LLM_MODE, "fast");
        assert_eq!(crate::CABIN_FAST_MODEL, "grok-4.7");
    }

    #[test]
    fn greeting_name_prefers_user_md() {
        assert_eq!(
            greeting_name("Name: Tester\neditor: nvim\n", "Tester"),
            "Tester"
        );
        assert_eq!(greeting_name("# User\n", "Tester Doe"), "Tester");
        assert_eq!(
            greeting_name("# User profile\nWho you are, preferred tools.\n", "Tester"),
            "Tester",
            "the USER.md template heading must not become Hello, User"
        );
        assert_eq!(greeting_name("", "Tester"), "Tester");
        assert_eq!(greeting_name("", ""), "");
        assert_eq!(
            greeting_name("Named: my project\n", "Tester"),
            "Tester",
            "nameplate-style lines must not steal the greeting"
        );
        assert_eq!(
            greeting_name("", "GrokHub"),
            "",
            "device / product names must not become the empty-home greeting"
        );
        assert_eq!(greeting_name("", "This computer"), "");
        assert_eq!(greeting_name("", "cachyos-x8664"), "");
        assert_eq!(greeting_name("Name: CachyOS\n", "Tester"), "Tester");
        assert_eq!(greeting_name("", "CachyOS"), "");
    }

    #[test]
    fn local_greeting_uses_name_not_memory_dump() {
        let insights = ["prefers nvim".into()];
        let mut g_in = input(
            "Name: Tester\neditor: nvim\n",
            "Last night we painted the cabin wall.\n",
            &insights,
            "Tester",
            21,
        );
        g_in.last_project = "Night cabin";
        let g = local_greeting(&g_in);
        assert_eq!(classify_greeting(&g_in), GreetingKind::ReturningProject);
        assert!(g.to_ascii_lowercase().contains("tester"), "{g}");
        assert!(g.to_ascii_lowercase().contains("night cabin"), "{g}");
        assert!(
            !g.to_ascii_lowercase().contains("painted") && !g.to_ascii_lowercase().contains("nvim"),
            "local line must not dump MEMORY.md: {g}"
        );
        assert!(g.chars().count() <= GREETING_MAX_CHARS, "{g}");
        assert!(!g.contains('?'), "local blurb stays quiet: {g}");
    }

    #[test]
    fn local_greeting_without_profile_is_quiet() {
        let g = local_greeting(&input("", "", &[], "", 8));
        assert!(g.to_ascii_lowercase().contains("morning"), "{g}");
        assert!(g.chars().count() <= GREETING_MAX_CHARS, "{g}");
        assert!(!g.contains("GrokHub"), "{g}");
        let device = local_greeting(&input("", "", &[], "GrokHub", 21));
        assert!(
            !device.to_ascii_lowercase().contains("grokhub"),
            "hostname GrokHub must not paint as the hero: {device}"
        );
        assert!(device.to_ascii_lowercase().contains("evening"), "{device}");
        let host = local_greeting(&input("", "", &[], "cachyos-x8664", 21));
        assert!(
            !host.to_ascii_lowercase().contains("cachy"),
            "hostname must not paint as the hero: {host}"
        );
        let distro = local_greeting(&input("Name: CachyOS\n", "", &[], "Tester", 8));
        assert!(
            distro.to_ascii_lowercase().contains("tester"),
            "distro Name: must fall through: {distro}"
        );
        assert!(!distro.to_ascii_lowercase().contains("cachy"), "{distro}");
    }

    #[test]
    fn local_greeting_surfaces_the_last_project() {
        let mut continue_in = input("", "", &[], "", 8);
        continue_in.last_project = "Night cabin";
        let local = local_greeting(&continue_in);
        assert!(
            local.to_ascii_lowercase().contains("night cabin"),
            "empty-chat greeting should surface the last project: {local}"
        );
        assert!(
            !local.to_ascii_lowercase().contains("continue night cabin"),
            "do not dump the raw continue hint: {local}"
        );
    }

    #[test]
    fn should_paint_only_on_new_chats() {
        assert!(should_paint_greeting(true, false));
        assert!(!should_paint_greeting(false, false));
        assert!(!should_paint_greeting(true, true));
        assert!(!should_paint_greeting(false, true));
    }

    #[test]
    fn fingerprint_moves_with_memory_and_hour() {
        let a = greeting_fingerprint(&input("Name: User\n", "wall\n", &[], "User", 21));
        let b = greeting_fingerprint(&input("Name: User\n", "nvim\n", &[], "User", 21));
        let c = greeting_fingerprint(&input("Name: User\n", "wall\n", &[], "User", 8));
        assert_ne!(a, b);
        assert_ne!(a, c);
        let mut d = input("Name: User\n", "wall\n", &[], "User", 21);
        d.last_project = "Night cabin";
        assert_ne!(a, greeting_fingerprint(&d));
        let mut e = input("Name: User\n", "wall\n", &[], "User", 21);
        e.first_run = true;
        assert_ne!(a, greeting_fingerprint(&e));
    }

    #[test]
    fn greeting_kind_ranks_situation() {
        assert!(is_cabin_first_run(false, false));
        assert!(!is_cabin_first_run(true, false));
        assert!(!is_cabin_first_run(false, true));

        let mut first = input("", "", &[], "", 8);
        first.first_run = true;
        first.signed_in = false;
        assert_eq!(classify_greeting(&first), GreetingKind::FirstRunUnsigned);
        let g = local_greeting(&first);
        assert_eq!(g, "Good morning. Connect Grok and I'll be right here.");

        first.signed_in = true;
        first.display_name = "Tester";
        assert_eq!(classify_greeting(&first), GreetingKind::FirstRunSigned);
        let g = local_greeting(&first);
        assert!(g.contains("Tester"), "{g}");
        assert!(g.contains("I'm here"), "{g}");
        assert!(!g.contains('?'), "{g}");

        let mut back = input("Name: Tester\n", "", &[], "Tester", 21);
        back.signed_in = false;
        assert_eq!(classify_greeting(&back), GreetingKind::ReturningUnsigned);
        assert_eq!(
            local_greeting(&back),
            "Good evening. Sign in and we can pick up where we left off."
        );

        back.signed_in = true;
        back.last_project = "Continue Night cabin";
        assert_eq!(clean_project_title(back.last_project), "Night cabin");
        assert_eq!(classify_greeting(&back), GreetingKind::ReturningProject);
        let g = local_greeting(&back);
        assert!(g.starts_with("Good evening, Tester."), "{g}");
        assert!(g.contains("Night cabin is still open"), "{g}");

        back.last_project = "failed: host snapshot";
        assert_eq!(classify_greeting(&back), GreetingKind::ReturningQuiet);
        assert_eq!(local_greeting(&back), "Good evening, Tester. I'm here.");

        assert_eq!(
            project_title_from_hint("Continue Night cabin"),
            "Night cabin"
        );
        assert!(clean_project_title("CachyOS").is_empty());
        assert!(clean_project_title("GrokHub").is_empty());
        assert!(clean_project_title("Chat").is_empty());
    }
}
