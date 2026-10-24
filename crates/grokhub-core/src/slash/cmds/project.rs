//! `/project`: show, bind, clear, and the project tree edits.

use crate::slash::registry::register_slash;
use crate::slash::Slash;

fn looks_like_bind_path(s: &str) -> bool {
    s.starts_with('/') || s.starts_with('~') || s.starts_with('.')
}

fn strip_bind_word(rest: &str) -> Option<&str> {
    let rest = rest.trim();
    if !rest
        .get(..4)
        .is_some_and(|w| w.eq_ignore_ascii_case("bind"))
    {
        return None;
    }
    let after = &rest[4..];
    if after.is_empty() {
        return Some("");
    }
    if after.starts_with(char::is_whitespace) {
        return Some(after.trim());
    }
    None
}

/// `/project new|folder|rename|move <name>`: `None` when the name is blank.
fn named(rest: &str, word: &str, make: fn(String) -> Slash) -> Option<Option<Slash>> {
    let name = rest.strip_prefix(word)?.trim();
    Some((!name.is_empty()).then(|| make(name.to_string())))
}

register_slash! {
    PROJECT {
        names: ["/project"],
        effect: Local,
        sample: "/project bind ~/code",
        parse: |rest| {
            if rest.eq_ignore_ascii_case("clear") || rest.eq_ignore_ascii_case("unbind") {
                Some(Slash::ProjectClear)
            } else if rest.is_empty() || rest.eq_ignore_ascii_case("show") {
                Some(Slash::ProjectShow)
            } else if let Some(made) = named(rest, "new ", Slash::ProjectNew) {
                made
            } else if let Some(made) = named(rest, "folder ", Slash::ProjectFolder) {
                made
            } else if let Some(made) = named(rest, "rename ", Slash::ProjectRename) {
                made
            } else if rest.eq_ignore_ascii_case("delete") {
                Some(Slash::ProjectDelete)
            } else if let Some(made) = named(rest, "move ", Slash::ProjectMove) {
                made
            } else if let Some(path) = strip_bind_word(rest) {
                if path.is_empty() {
                    None
                } else {
                    Some(Slash::ProjectBind(Some(path.to_string())))
                }
            } else if looks_like_bind_path(rest) {
                Some(Slash::ProjectBind(Some(rest.to_string())))
            } else {
                None
            }
        },
    }
}
