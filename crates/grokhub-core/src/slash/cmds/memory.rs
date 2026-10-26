//! Memory notes, recall and forget.

use crate::slash::registry::register_slash;
use crate::slash::Slash;

register_slash! {
    FORGET {
        names: ["/forget"],
        effect: Local,
        sample: "/forget wifi",
        parse: |rest| Some(Slash::Forget((!rest.is_empty()).then(|| rest.to_string()))),
    }
}

register_slash! {
    /// `/memory`, `/memory show|dream|prune`, `/memory note <text>`.
    MEMORY {
        names: ["/memory"],
        effect: Local,
        sample: "/memory note prefer nvim",
        parse: |rest| {
            if rest.eq_ignore_ascii_case("show") || rest.is_empty() {
                return Some(Slash::MemoryShow);
            }
            if rest.eq_ignore_ascii_case("dream") {
                return Some(Slash::MemoryDream);
            }
            if rest.eq_ignore_ascii_case("prune") {
                return Some(Slash::MemoryPrune);
            }
            let note = rest
                .strip_prefix("note")
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())?;
            Some(Slash::MemoryNote(note.to_string()))
        },
    }
}

register_slash! {
    /// Bare `/mem` shows memory; `/mem <text>` remembers it.
    MEM {
        names: ["/mem"],
        effect: Local,
        sample: "/mem tabs over spaces",
        parse: |rest| {
            if rest.is_empty() || rest.eq_ignore_ascii_case("show") {
                Some(Slash::MemoryShow)
            } else {
                Some(Slash::Remember(rest.to_string()))
            }
        },
    }
}
