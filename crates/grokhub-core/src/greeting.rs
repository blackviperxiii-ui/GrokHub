//! Empty-chat greeting blurb. Fast mode. Secrets never in the line.

use crate::is_plain_text;
use crate::strip_thinking;

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
            if h.eq_ignore_ascii_case("user") || h.eq_ignore_ascii_case("profile") {
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

fn greeting_uses_machine_name(s: &str) -> bool {
    s.split(|c: char| !c.is_alphanumeric() && c != '-')
        .any(|w| !w.is_empty() && (is_machine_name(w) || w.eq_ignore_ascii_case("grokhub")))
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

fn time_word(hour: u8) -> &'static str {
    match hour {
        5..=11 => "Morning",
        12..=16 => "Afternoon",
        17..=21 => "Evening",
        _ => "Night",
    }
}

fn memory_hooks(
    user_md: &str,
    memory_md: &str,
    insights: &[String],
    max: usize,
) -> Vec<String> {
    let mut out = vec![];
    for src in [memory_md, user_md] {
        for line in src.lines() {
            if out.len() >= max {
                return out;
            }
            let t = line.trim().trim_start_matches('#').trim();
            if t.is_empty() {
                continue;
            }
            let low = t.to_ascii_lowercase();
            if low.starts_with("name") || low == "user" || low == "profile" {
                continue;
            }
            if !is_plain_text(t) {
                continue;
            }
            let bit: String = t.trim_end_matches('.').chars().take(48).collect();
            if bit.chars().count() < 4 {
                continue;
            }
            out.push(bit);
        }
    }
    for i in insights {
        if out.len() >= max {
            break;
        }
        if !is_plain_text(i) {
            continue;
        }
        let bit: String = i.trim().trim_end_matches('.').chars().take(48).collect();
        if bit.chars().count() >= 4 {
            out.push(bit);
        }
    }
    out
}

pub fn local_greeting(input: &GreetingInput) -> String {
    let name = greeting_name(input.user_md, input.display_name);
    let hooks = memory_hooks(input.user_md, input.memory_md, input.insights, 1);
    let tod = time_word(input.hour);
    let mut line = if name.is_empty() {
        format!("{tod}.")
    } else {
        format!("{tod}, {name}.")
    };
    if let Some(h) = hooks.first() {
        let next = format!("{line} {h}.");
        if next.chars().count() <= GREETING_MAX_CHARS {
            line = next;
        }
    } else if name.is_empty() {
        line = format!("{tod}. The cabin is ready.");
    }
    if !input.last_night.trim().is_empty() && is_plain_text(input.last_night) {
        let bit: String = input.last_night.trim().chars().take(40).collect();
        if !line.to_ascii_lowercase().contains(&bit.to_ascii_lowercase()) {
            let next = format!("{line} {bit}");
            let next = if next.ends_with('.') {
                next
            } else {
                format!("{next}.")
            };
            if next.chars().count() <= GREETING_MAX_CHARS {
                line = next;
            }
        }
    }
    clip_greeting(&line)
}

pub fn greeting_prompt(input: &GreetingInput) -> String {
    let name = greeting_name(input.user_md, input.display_name);
    let mut lines = vec![
        "Write one quiet greeting line for an empty cabin chat.".into(),
        format!(
            "One sentence, at most {GREETING_MAX_CHARS} characters. No markdown, no emoji, no quotes, no secrets."
        ),
        "Address the user by name if known. Ground in USER.md / MEMORY.md. Invite without shouting.".into(),
        "Reply with only the line.".into(),
        String::new(),
        format!("Hour: {}", input.hour),
        format!(
            "Name: {}",
            if name.is_empty() {
                "(unknown)".into()
            } else {
                name
            }
        ),
    ];
    if !input.last_night.trim().is_empty() && is_plain_text(input.last_night) {
        lines.push(format!(
            "Last night: {}",
            input.last_night.chars().take(80).collect::<String>()
        ));
    }
    if !input.user_md.trim().is_empty() && is_plain_text(input.user_md) {
        lines.push("USER.md:".into());
        lines.push(input.user_md.chars().take(400).collect());
    }
    if !input.memory_md.trim().is_empty() && is_plain_text(input.memory_md) {
        lines.push("MEMORY.md:".into());
        lines.push(input.memory_md.chars().take(400).collect());
    }
    if !input.insights.is_empty() {
        lines.push("Insights:".into());
        for i in input.insights.iter().take(6) {
            if is_plain_text(i) {
                lines.push(format!("- {i}"));
            }
        }
    }
    lines.join("\n")
}

pub fn parse_llm_greeting(raw: &str) -> Option<String> {
    let stripped = strip_thinking(raw);
    let mut t = if stripped.trim().is_empty() {
        return None;
    } else {
        stripped.trim().to_string()
    };
    if let Some(rest) = t.strip_prefix("```") {
        t = rest
            .trim_start_matches("text")
            .trim_start_matches("markdown")
            .trim()
            .trim_end_matches("```")
            .trim()
            .to_string();
    }
    let line = t.lines().map(str::trim).find(|l| {
        !l.is_empty()
            && !l.eq_ignore_ascii_case("thinking:")
            && !l.starts_with("<think>")
    })?;
    let line = line.trim_matches('"').trim_matches('\'').trim();
    if line.is_empty() || !is_plain_text(line) {
        return None;
    }
    let out = clip_greeting(line);
    if out.chars().count() < 8 || is_product_name(&out) || is_product_greeting(&out) || greeting_uses_machine_name(&out) {
        None
    } else {
        Some(out)
    }
}

fn is_product_greeting(s: &str) -> bool {
    let t = s.trim().to_ascii_lowercase();
    t == "grokhub"
        || t == "grokhub."
        || t == "grok"
        || t == "grok."
        || t.starts_with("welcome to grokhub")
        || t == "native grok build cabin"
        || t.contains(", grok.")
        || t.contains(", grok ")
        || t.contains(", grokhub")
        || t.contains("hello, grok")
        || t.contains("hi, grok")
        || t.contains("morning, grok")
        || t.contains("afternoon, grok")
        || t.contains("evening, grok")
        || t.contains("night, grok")
}

pub fn pick_greeting(local: &str, llm: Option<&str>) -> String {
    if let Some(raw) = llm {
        if let Some(g) = parse_llm_greeting(raw) {
            return g;
        }
    }
    local.to_string()
}

pub fn greeting_fingerprint(input: &GreetingInput) -> String {
    let name = greeting_name(input.user_md, input.display_name);
    let user: String = input.user_md.trim().chars().take(80).collect();
    let mem: String = input.memory_md.trim().chars().take(80).collect();
    let ins = input.insights.first().cloned().unwrap_or_default();
    format!(
        "{}|{}|{}|{}|n{}|h{}",
        name,
        user,
        mem,
        ins.chars().take(40).collect::<String>(),
        input.last_night.chars().take(40).collect::<String>(),
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
    has_auth && !busy && next_fp != prev_fp && now_ms.saturating_sub(last_at) >= GREETING_LLM_DEBOUNCE_MS
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
        }
    }

    #[test]
    fn greeting_llm_is_fast_mode() {
        assert_eq!(GREETING_LLM_MODE, "fast");
        assert_eq!(crate::CABIN_FAST_MODEL, "grok-4-1-fast-non-reasoning");
    }

    #[test]
    fn greeting_name_prefers_user_md() {
        assert_eq!(
            greeting_name("Name: Viper\neditor: nvim\n", "Jeremy"),
            "Viper"
        );
        assert_eq!(greeting_name("# User\n", "Jeremy Strickland"), "Jeremy");
        assert_eq!(greeting_name("", ""), "");
        assert_eq!(
            greeting_name("Named: my project\n", "Jeremy"),
            "Jeremy",
            "nameplate-style lines must not steal the greeting"
        );
        assert_eq!(
            greeting_name("", "GrokHub"),
            "",
            "device / product names must not become the empty-home greeting"
        );
        assert_eq!(greeting_name("", "This computer"), "");
        assert_eq!(greeting_name("", "cachyos-x8664"), "");
        assert_eq!(greeting_name("Name: CachyOS\n", "Viper"), "Viper");
        assert_eq!(greeting_name("", "CachyOS"), "");
        assert!(parse_llm_greeting("Hello, CachyOS.").is_none());
        assert!(parse_llm_greeting("Hello, Grok.").is_none());
        assert!(parse_llm_greeting("Evening, Viper. The cabin is ready.").is_some());
    }

    #[test]
    fn local_greeting_uses_profile_and_memory() {
        let insights = ["prefers nvim".into()];
        let g = local_greeting(&input(
            "Name: Viper\neditor: nvim\n",
            "Last night we painted the cabin wall.\n",
            &insights,
            "Jeremy",
            21,
        ));
        assert!(g.to_ascii_lowercase().contains("viper"), "{g}");
        assert!(
            g.to_ascii_lowercase().contains("nvim")
                || g.to_ascii_lowercase().contains("wall")
                || g.to_ascii_lowercase().contains("paint"),
            "should ground in memory, got {g}"
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
        let distro = local_greeting(&input("Name: CachyOS\n", "", &[], "Viper", 8));
        assert!(
            distro.to_ascii_lowercase().contains("viper"),
            "distro Name: must fall through: {distro}"
        );
        assert!(!distro.to_ascii_lowercase().contains("cachy"), "{distro}");
    }

    #[test]
    fn greeting_prompt_grounds_in_user_memory() {
        let prompt = greeting_prompt(&input(
            "Name: Viper\n",
            "Paint the wall tonight.\n",
            &["likes the cabin quiet".into()],
            "Viper",
            22,
        ));
        assert!(prompt.contains("Viper"), "{prompt}");
        assert!(prompt.contains("Paint the wall"), "{prompt}");
        assert!(
            prompt.contains("cabin quiet") || prompt.contains("USER.md"),
            "{prompt}"
        );
        assert!(
            prompt.to_ascii_lowercase().contains("one")
                && (prompt.contains("90")
                    || prompt.contains(&GREETING_MAX_CHARS.to_string())
                    || prompt.contains("short")),
            "Fast prompt should demand a short line: {prompt}"
        );
        assert_eq!(GREETING_LLM_MODE, "fast");
        let insights = ["likes the cabin quiet".into()];
        let mut with_night = input(
            "Name: Viper\n",
            "Paint the wall tonight.\n",
            &insights,
            "Viper",
            22,
        );
        with_night.last_night = "host snapshot failed";
        let night_prompt = greeting_prompt(&with_night);
        assert!(
            night_prompt.contains("Last night: host snapshot failed"),
            "{night_prompt}"
        );
        let mut continue_in = input("", "", &[], "", 8);
        continue_in.last_night = "Continue Night cabin";
        let local = local_greeting(&continue_in);
        assert!(
            local.to_ascii_lowercase().contains("continue night cabin"),
            "empty-chat greeting should surface the last-access hint: {local}"
        );
    }

    #[test]
    fn parse_llm_greeting_keeps_a_short_line() {
        let g = parse_llm_greeting(
            "\"Night, Viper. The wall still wants a second coat.\"\nAnother line.",
        )
        .expect("parse");
        assert!(g.contains("Viper"), "{g}");
        assert!(!g.contains('"'), "{g}");
        assert!(g.chars().count() <= GREETING_MAX_CHARS, "{g}");
    }

    #[test]
    fn parse_llm_greeting_drops_secrets() {
        assert!(parse_llm_greeting("token sk-abcdefghijklmnopqrstuv in the cabin").is_none());
        assert!(parse_llm_greeting("").is_none());
    }

    #[test]
    fn parse_llm_greeting_strips_thinking() {
        let g = parse_llm_greeting("THINKING:\nplan a quiet line\n\nAfternoon, Viper. The cabin is ready.")
            .expect("body after thought");
        assert!(g.to_ascii_lowercase().contains("afternoon"), "{g}");
        assert!(!g.to_ascii_lowercase().contains("thinking"), "{g}");
        assert!(
            parse_llm_greeting("THINKING:").is_none(),
            "a thought-only Fast reply must not paint THINKING:"
        );
        assert_eq!(
            pick_greeting("Afternoon, Viper.", Some("THINKING:")),
            "Afternoon, Viper."
        );
    }

    #[test]
    fn pick_greeting_prefers_fast_when_clean() {
        let local = "Evening, Viper.";
        let llm = "Evening, Viper. nvim is still on the desk.";
        assert_eq!(pick_greeting(local, Some(llm)), llm);
        assert_eq!(pick_greeting(local, None), local);
        assert_eq!(
            pick_greeting(local, Some("token sk-abcdefghijklmnopqrstuv")),
            local
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
        let a = greeting_fingerprint(&input("Name: Viper\n", "wall\n", &[], "Viper", 21));
        let b = greeting_fingerprint(&input("Name: Viper\n", "nvim\n", &[], "Viper", 21));
        let c = greeting_fingerprint(&input("Name: Viper\n", "wall\n", &[], "Viper", 8));
        assert_ne!(a, b);
        assert_ne!(a, c);
    }
}
