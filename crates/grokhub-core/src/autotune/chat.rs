//! Settings asked for in chat ("make the text bigger", "dark mode", "undo
//! that", "why is the text big?"), read without a model call.
//!
//! Strict on purpose: only a short ask whose every word is accounted for
//! counts, so "make a bigger image" or "how do I add dark mode to my site"
//! go to the model as usual.

use super::settings::{describe, Key, Value, TEXT_STEPS};
use super::store::{AutoStore, Source};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SettingAsk {
    Set(Key, Value),
    /// One text-size step up (`1`) or down (`-1`).
    Step(Key, i8),
    /// Undo the last change auto setup or a chat ask made.
    Undo,
    /// Why is this setting the way it is?
    Why(Key),
}

/// Words that carry no meaning in an ask.
const FILLER: &[&str] = &[
    "please", "pls", "the", "my", "a", "bit", "little", "lil", "all", "can", "could", "would",
    "you", "u", "just", "now", "for", "me", "some", "of", "again", "thanks", "thank", "so", "too",
    "way", "much", "lot", "loadout", "hey", "ok", "okay", "to", "go", "be",
];

fn words(text: &str) -> Vec<String> {
    text.to_lowercase()
        .replace(['’', '\''], "")
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_string)
        .collect()
}

/// Read a chat message as a settings ask, or `None` for anything else.
pub fn read_setting_ask(text: &str) -> Option<SettingAsk> {
    let all = words(text);
    if all.is_empty() || all.len() > 9 {
        return None;
    }
    if let Some(a) = read_undo(&all) {
        return Some(a);
    }
    if all[0] == "why" {
        return read_why(&all[1..]);
    }
    let w: Vec<&str> = all
        .iter()
        .map(String::as_str)
        .filter(|w| !FILLER.contains(w))
        .collect();
    read_theme(&w)
        .or_else(|| read_size(&w))
        .or_else(|| read_switch(&w))
}

fn read_undo(all: &[String]) -> Option<SettingAsk> {
    let s = all.join(" ");
    const UNDO: &[&str] = &[
        "undo",
        "undo that",
        "undo it",
        "undo the last change",
        "undo last change",
        "put it back",
        "change it back",
        "set it back",
        "revert that",
        "revert it",
    ];
    let s = s.trim_start_matches("please ").trim_end_matches(" please");
    UNDO.contains(&s).then_some(SettingAsk::Undo)
}

/// A setting's name at `w[i..]`: its key and how many words it took.
fn noun_at(w: &[&str], i: usize, keys: &[Key]) -> Option<(Key, usize)> {
    let mut best: Option<(Key, usize)> = None;
    for k in keys {
        for name in k.spec().words {
            let parts: Vec<&str> = name.split(' ').collect();
            let n = parts.len();
            if w.len() >= i + n && w[i..i + n] == parts[..] && best.is_none_or(|(_, m)| n > m) {
                best = Some((*k, n));
            }
        }
    }
    best
}

const SIZE_UP: &[&str] = &["bigger", "larger", "up", "increase", "enlarge"];
const SIZE_DOWN: &[&str] = &["smaller", "down", "decrease", "shrink", "tinier"];
const SIZE_BACK: &[&str] = &["normal", "default", "back", "reset", "regular", "standard"];

/// "make the text bigger", "bigger text", "text smaller", "text size back to normal",
/// "zoom in".
fn read_size(w: &[&str]) -> Option<SettingAsk> {
    match w {
        ["zoom", "in"] => return Some(SettingAsk::Step(Key::TextSize, 1)),
        ["zoom", "out"] => return Some(SettingAsk::Step(Key::TextSize, -1)),
        _ => {}
    }
    let rest = match w.first() {
        Some(&("make" | "set" | "turn" | "put" | "change")) => &w[1..],
        _ => w,
    };
    let dir = |x: &str| {
        if SIZE_UP.contains(&x) {
            Some(1)
        } else if SIZE_DOWN.contains(&x) {
            Some(-1)
        } else if SIZE_BACK.contains(&x) {
            Some(0)
        } else {
            None
        }
    };
    let ask = |d: i8| {
        if d == 0 {
            SettingAsk::Set(Key::TextSize, Value::Pct(TEXT_STEPS[0]))
        } else {
            SettingAsk::Step(Key::TextSize, d)
        }
    };
    // noun then direction: "text bigger", "text size back normal"
    if let Some((Key::TextSize, n)) = noun_at(rest, 0, &[Key::TextSize]) {
        let tail = &rest[n..];
        if let Some(d) = one_direction(tail, dir) {
            return Some(ask(d));
        }
    }
    // direction then noun: "bigger text"
    if let Some((first, tail)) = rest.split_first() {
        if let Some(d) = dir(first) {
            if d != 0 && noun_at(tail, 0, &[Key::TextSize]) == Some((Key::TextSize, tail.len())) {
                return Some(ask(d));
            }
        }
    }
    None
}

/// The tail is one direction word, or "back" plus a resting word ("back normal").
fn one_direction(tail: &[&str], dir: impl Fn(&str) -> Option<i8>) -> Option<i8> {
    match tail {
        [d] => dir(d),
        ["back", x] | [x, "again"] if SIZE_BACK.contains(x) => Some(0),
        _ => None,
    }
}

/// "dark mode", "use dark mode", "switch dark mode", "turn on dark mode",
/// "turn dark mode off", "light theme".
fn read_theme(w: &[&str]) -> Option<SettingAsk> {
    let rest = match w.first() {
        Some(&("use" | "switch" | "turn" | "set" | "make" | "enable" | "change" | "put")) => {
            &w[1..]
        }
        _ => w,
    };
    let (on, rest) = match rest {
        ["on", tail @ ..] => (true, tail),
        ["off", tail @ ..] => (false, tail),
        _ => (true, rest),
    };
    let (on, mode) = match rest {
        [m, "mode" | "theme"] => (on, *m),
        [m, "mode" | "theme", "on"] => (true, *m),
        [m, "mode" | "theme", "off"] => (false, *m),
        [m] if w.first().is_some_and(|f| *f == "use" || *f == "switch") => (on, *m),
        _ => return None,
    };
    let pick = match (mode, on) {
        ("dark", true) | ("light", false) => "dark",
        ("light", true) | ("dark", false) => "light",
        _ => return None,
    };
    Some(SettingAsk::Set(Key::Theme, Value::pick(pick)))
}

/// "turn off notifications", "notifications off", "turn on animations",
/// "reduce motion", "stop the animations", "fewer notifications".
fn read_switch(w: &[&str]) -> Option<SettingAsk> {
    match w {
        ["reduce", "motion"]
        | ["less", "motion"]
        | ["stop", "animations"]
        | ["no", "animations"] => {
            return Some(SettingAsk::Set(Key::ReduceMotion, Value::Bool(true)));
        }
        ["fewer" | "less", "notifications" | "pings"] => {
            return Some(SettingAsk::Set(
                Key::Notifications,
                Value::pick("important"),
            ));
        }
        _ => {}
    }
    let keys = [
        Key::Notifications,
        Key::ReduceMotion,
        Key::Effects,
        Key::Suggestions,
    ];
    let verb = matches!(w.first(), Some(&("turn" | "switch" | "set" | "put")));
    let rest = if verb { &w[1..] } else { w };
    // on/off first: "turn off notifications"
    let (state, key) = match rest {
        [s @ ("on" | "off"), tail @ ..] => {
            let (k, n) = noun_at(tail, 0, &keys)?;
            (n == tail.len()).then_some((*s == "on", k))?
        }
        // name first: "notifications off", "turn animations back on"
        _ => {
            let (k, n) = noun_at(rest, 0, &keys)?;
            match &rest[n..] {
                ["on"] | ["back", "on"] => (true, k),
                ["off"] => (false, k),
                _ => return None,
            }
        }
    };
    Some(match key {
        Key::Notifications => SettingAsk::Set(
            Key::Notifications,
            Value::pick(if state { "all" } else { "quiet" }),
        ),
        // Animations on means reduce motion off.
        Key::ReduceMotion => SettingAsk::Set(Key::ReduceMotion, Value::Bool(!state)),
        k => SettingAsk::Set(k, Value::Bool(state)),
    })
}

/// "why is the text so big", "why are animations off", "why did you change the theme",
/// "why dark mode".
fn read_why(w: &[String]) -> Option<SettingAsk> {
    const GLUE: &[&str] = &[
        "is", "are", "did", "you", "change", "changed", "the", "my", "so", "in", "turn", "turned",
        "make", "made",
    ];
    const STATE: &[&str] = &[
        "big",
        "bigger",
        "small",
        "smaller",
        "large",
        "huge",
        "tiny",
        "off",
        "on",
        "different",
        "gone",
        "slow",
        "missing",
    ];
    let w: Vec<&str> = w
        .iter()
        .map(String::as_str)
        .filter(|x| !GLUE.contains(x) && !STATE.contains(x))
        .collect();
    let keys = [
        Key::TextSize,
        Key::ReduceMotion,
        Key::Theme,
        Key::Effects,
        Key::Notifications,
        Key::Suggestions,
        Key::CollapseThoughts,
    ];
    let (k, n) = noun_at(&w, 0, &keys)?;
    (n == w.len()).then_some(SettingAsk::Why(k))
}

/// The text size one step up or down from `cur`, or `None` at the end.
pub fn step_text(cur: &Value, dir: i8) -> Option<Value> {
    let Value::Pct(p) = cur else {
        return Some(Value::Pct(TEXT_STEPS[0]));
    };
    let i = TEXT_STEPS.iter().rposition(|s| s <= p).unwrap_or(0);
    let next = if dir > 0 {
        TEXT_STEPS.get(i + 1)
    } else if TEXT_STEPS[i] < *p {
        TEXT_STEPS.get(i)
    } else {
        i.checked_sub(1).and_then(|j| TEXT_STEPS.get(j))
    };
    next.map(|n| Value::Pct(*n))
}

/// What the chat says after a setting changed: "Text size is 130% now."
pub fn done_reply(key: Key, to: &Value, clock_12h: bool) -> String {
    format!(
        "{} is {} now.",
        key.label(),
        describe(key, to, clock_12h).to_lowercase_first()
    )
}

/// The answer to "why is the text so big?".
pub fn why_reply(store: &AutoStore, key: Key, now: Option<&Value>, clock_12h: bool) -> String {
    let what = now
        .map(|v| {
            format!(
                "{} is {}",
                key.label(),
                describe(key, v, clock_12h).to_lowercase_first()
            )
        })
        .unwrap_or_else(|| key.label().to_string());
    match store.entries.get(&key) {
        Some(e) => match &e.source {
            Source::UserSet => format!("{what} because you set it that way."),
            Source::Asked { .. } | Source::Inferred { .. } => format!("{what}. {}", e.reason),
        },
        None => format!(
            "{what}, which is how {} starts.",
            crate::brand::product_name()
        ),
    }
}

trait LowerFirst {
    fn to_lowercase_first(&self) -> String;
}

impl LowerFirst for String {
    /// "On" → "on", "Only important" → "only important"; "130%" and
    /// "10 PM to 7 AM" stay as they are.
    fn to_lowercase_first(&self) -> String {
        let mut c = self.chars();
        match c.next() {
            Some(f) if f.is_uppercase() && !self.contains(" AM") && !self.contains(" PM") => {
                f.to_lowercase().collect::<String>() + c.as_str()
            }
            _ => self.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ask(s: &str) -> Option<SettingAsk> {
        read_setting_ask(s)
    }

    #[test]
    fn text_size_asks() {
        for s in [
            "make the text bigger",
            "Make the text bigger please",
            "bigger text",
            "make everything bigger",
            "make things a bit larger",
            "text bigger",
            "can you make the font bigger",
            "zoom in",
        ] {
            assert_eq!(ask(s), Some(SettingAsk::Step(Key::TextSize, 1)), "{s}");
        }
        for s in [
            "make the text smaller",
            "smaller text",
            "zoom out",
            "font size down",
        ] {
            assert_eq!(ask(s), Some(SettingAsk::Step(Key::TextSize, -1)), "{s}");
        }
        for s in [
            "text size back to normal",
            "make the text normal again",
            "text size default",
        ] {
            assert_eq!(
                ask(s),
                Some(SettingAsk::Set(Key::TextSize, Value::Pct(100))),
                "{s}"
            );
        }
    }

    #[test]
    fn theme_and_switch_asks() {
        let dark = Some(SettingAsk::Set(Key::Theme, Value::pick("dark")));
        let light = Some(SettingAsk::Set(Key::Theme, Value::pick("light")));
        for s in [
            "dark mode",
            "use dark mode",
            "switch to dark mode",
            "turn on dark mode",
            "dark mode please",
            "turn light mode off",
        ] {
            assert_eq!(ask(s), dark, "{s}");
        }
        for s in [
            "light mode",
            "light theme",
            "turn dark mode off",
            "switch to light",
        ] {
            assert_eq!(ask(s), light, "{s}");
        }
        let quiet = Some(SettingAsk::Set(Key::Notifications, Value::pick("quiet")));
        for s in [
            "turn off notifications",
            "notifications off",
            "turn the notifications off",
        ] {
            assert_eq!(ask(s), quiet, "{s}");
        }
        assert_eq!(
            ask("turn on notifications"),
            Some(SettingAsk::Set(Key::Notifications, Value::pick("all")))
        );
        assert_eq!(
            ask("fewer notifications"),
            Some(SettingAsk::Set(
                Key::Notifications,
                Value::pick("important")
            ))
        );
        let calm = Some(SettingAsk::Set(Key::ReduceMotion, Value::Bool(true)));
        for s in [
            "turn off animations",
            "reduce motion",
            "stop the animations",
            "animations off",
        ] {
            assert_eq!(ask(s), calm, "{s}");
        }
        assert_eq!(
            ask("turn animations back on"),
            Some(SettingAsk::Set(Key::ReduceMotion, Value::Bool(false)))
        );
        assert_eq!(
            ask("turn off suggestions"),
            Some(SettingAsk::Set(Key::Suggestions, Value::Bool(false)))
        );
    }

    #[test]
    fn undo_and_why_asks() {
        for s in [
            "undo",
            "undo that",
            "Undo that.",
            "put it back",
            "please undo that",
            "change it back",
        ] {
            assert_eq!(ask(s), Some(SettingAsk::Undo), "{s}");
        }
        assert_eq!(
            ask("why is the text so big?"),
            Some(SettingAsk::Why(Key::TextSize))
        );
        assert_eq!(
            ask("why are the animations off"),
            Some(SettingAsk::Why(Key::ReduceMotion))
        );
        assert_eq!(
            ask("why did you change the theme"),
            Some(SettingAsk::Why(Key::Theme))
        );
        assert_eq!(
            ask("why is everything so big"),
            Some(SettingAsk::Why(Key::TextSize))
        );
        assert_eq!(
            ask("why are notifications off"),
            Some(SettingAsk::Why(Key::Notifications))
        );
        assert_eq!(ask("why dark mode"), Some(SettingAsk::Why(Key::Theme)));
    }

    #[test]
    fn steps_and_replies() {
        assert_eq!(step_text(&Value::Pct(100), 1), Some(Value::Pct(115)));
        assert_eq!(step_text(&Value::Pct(150), 1), None);
        assert_eq!(step_text(&Value::Pct(130), -1), Some(Value::Pct(115)));
        assert_eq!(step_text(&Value::Pct(100), -1), None);
        assert_eq!(step_text(&Value::Pct(140), -1), Some(Value::Pct(130)));
        assert_eq!(step_text(&Value::Pct(140), 1), Some(Value::Pct(150)));
        assert_eq!(
            done_reply(Key::TextSize, &Value::Pct(130), true),
            "Text size is 130% now."
        );
        assert_eq!(
            done_reply(Key::Notifications, &Value::pick("important"), true),
            "Notifications is only important now."
        );
        let mut store = AutoStore::default();
        assert_eq!(
            why_reply(&store, Key::Theme, Some(&Value::pick("dark")), true),
            format!(
                "Theme is dark, which is how {} starts.",
                crate::brand::product_name()
            )
        );
        store.record(
            Key::TextSize,
            Some(Value::Pct(100)),
            Value::Pct(150),
            Source::Inferred {
                rule: "os_text_size".into(),
            },
            "Text is bigger because Windows text size is 150%.",
            1,
        );
        assert_eq!(
            why_reply(&store, Key::TextSize, Some(&Value::Pct(150)), true),
            "Text size is 150%. Text is bigger because Windows text size is 150%."
        );
        store.undo(Key::TextSize, 2).expect("undo");
        assert_eq!(
            why_reply(&store, Key::TextSize, Some(&Value::Pct(100)), true),
            "Text size is 100% because you set it that way."
        );
    }

    #[test]
    fn everything_else_goes_to_the_model() {
        for s in [
            "make a bigger image",
            "make the logo bigger",
            "make me a picture of a dark castle",
            "how do I add dark mode to my website",
            "write a dark mode toggle in react",
            "turn off the lights",
            "turn off my pc",
            "undo the last commit",
            "why is the sky blue",
            "why is my pc slow",
            "why is it so big",
            "make it bigger",
            "why is it dark",
            "dark",
            "text",
            "bigger",
            "make the text bigger and also write me a poem about the sea",
            "notifications",
            "my notifications are broken off and on all day",
            "can you turn off notifications for discord",
            "",
        ] {
            assert_eq!(ask(s), None, "{s:?}");
        }
    }
}
