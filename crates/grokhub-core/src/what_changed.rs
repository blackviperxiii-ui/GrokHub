//! A short "What changed" block above a dream or self-review report: counts
//! for added, merged and removed, then the top items by name. Plain text that
//! also reads as a Markdown list, so chat and the Ideas card show it the same.

/// Items named per group before "and N more".
pub const TOP_ITEMS: usize = 3;
/// Characters kept from one item name.
pub const ITEM_CHARS: usize = 48;

/// What one report changed. `removed_word` is "retired" for a dream, where
/// nothing is deleted, and "removed" for a diff.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WhatChanged {
    pub added: Vec<String>,
    pub merged: Vec<String>,
    pub removed: Vec<String>,
    pub removed_word: &'static str,
}

impl WhatChanged {
    pub fn is_empty(&self) -> bool {
        self.added.is_empty() && self.merged.is_empty() && self.removed.is_empty()
    }

    /// `What changed: 1 added, 2 merged, 0 retired` and one line per group
    /// that has items. Ends with a blank line so the full report follows.
    pub fn block(&self) -> String {
        let removed_word = if self.removed_word.is_empty() {
            "removed"
        } else {
            self.removed_word
        };
        let mut out = format!(
            "What changed: {} added, {} merged, {} {removed_word}\n",
            self.added.len(),
            self.merged.len(),
            self.removed.len()
        );
        let mut label = removed_word.to_string();
        if let Some(first) = label.get_mut(0..1) {
            first.make_ascii_uppercase();
        }
        for (name, items) in [
            ("Added", &self.added),
            ("Merged", &self.merged),
            (label.as_str(), &self.removed),
        ] {
            if !items.is_empty() {
                out.push_str(&format!("- {name}: {}\n", top_items(items)));
            }
        }
        out.push('\n');
        out
    }
}

/// `"a", "b", "c" and 2 more`, each name cut to `ITEM_CHARS`.
pub fn top_items(items: &[String]) -> String {
    let shown: Vec<String> = items
        .iter()
        .take(TOP_ITEMS)
        .map(|s| format!("\"{}\"", clip(s.trim(), ITEM_CHARS)))
        .collect();
    let mut line = shown.join(", ");
    if items.len() > TOP_ITEMS {
        line.push_str(&format!(" and {} more", items.len() - TOP_ITEMS));
    }
    line
}

fn clip(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let kept: String = s.chars().take(max.saturating_sub(1)).collect();
    format!("{}…", kept.trim_end())
}

/// The first `"…"` quote in `s`, after `from`.
fn quote_after<'a>(s: &'a str, from: &str) -> Option<&'a str> {
    let rest = &s[s.find(from)? + from.len()..];
    let open = rest.find('"')?;
    let tail = &rest[open + 1..];
    Some(&tail[..tail.find('"')?])
}

/// Read back what a dream report (`DreamReport::markdown`) did: each merged
/// note by its quote, each retired note, and each line proposed for USER.md
/// (proposed, not applied, so they are not counted as added).
pub fn from_dream(report: &str) -> WhatChanged {
    let mut out = WhatChanged {
        removed_word: "retired",
        ..Default::default()
    };
    let mut section = "";
    for line in report.lines() {
        if let Some(head) = line.strip_prefix("## ") {
            section = head.trim();
            continue;
        }
        match section {
            "Merged" => {
                if let Some(q) = line
                    .strip_prefix("- Kept ")
                    .and_then(|l| quote_after(l, ", merged "))
                {
                    out.merged.push(q.to_string());
                }
            }
            "Retired" => {
                if let Some(q) = line
                    .strip_prefix("- Retired ")
                    .and_then(|l| quote_after(l, "`"))
                {
                    out.removed.push(q.to_string());
                }
            }
            _ => {}
        }
    }
    out
}

/// Read a `line_diff` back: `+ ` lines added, `- ` lines removed. Blank and
/// kept lines and `…` are skipped, and front-matter fences are not items.
pub fn from_line_diff(diff: &str) -> WhatChanged {
    let mut out = WhatChanged {
        removed_word: "removed",
        ..Default::default()
    };
    for line in diff.lines() {
        let (bucket, text) = if let Some(t) = line.strip_prefix("+ ") {
            (&mut out.added, t)
        } else if let Some(t) = line.strip_prefix("- ") {
            (&mut out.removed, t)
        } else {
            continue;
        };
        let text = text.trim();
        if text.is_empty() || text == "---" {
            continue;
        }
        bucket.push(text.to_string());
    }
    out
}

/// `report` with the block on top, or `report` alone when nothing changed.
pub fn with_block(changes: &WhatChanged, report: &str) -> String {
    if changes.is_empty() {
        report.to_string()
    } else {
        format!("{}{report}", changes.block())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DREAM: &str = "# Memory dream 2026-10-08\n\nGrokHub tidied its memory overnight. Nothing was deleted: merged and retired notes are marked forgotten and stay on disk.\n\n- Looked at: 40 notes (6 changed in the last 14 days, 2 with low confidence)\n- Merged: 2\n- Retired: 1\n\n## Merged\n\n- Kept `n1` \"Uses pnpm, not npm\", merged `n2` \"uses pnpm\" because 80% shared words. Kept `n1` because it is newer.\n- Kept `n3` \"Prefers the dark theme\", merged `n4` \"likes dark mode\" because same tags. Kept `n3` because it has more uses.\n\n## Retired\n\n- Retired `n9` \"Token lives in ~/.old\" because confidence 0.2 after 30 days.\n\n## Proposed USER.md changes (not applied)\n\n```diff\n+- Writes Rust\n```\n";

    #[test]
    fn a_dream_report_names_what_it_merged_and_retired() {
        let c = from_dream(DREAM);
        assert_eq!(c.merged, vec!["uses pnpm".to_string(), "likes dark mode".to_string()]);
        assert_eq!(c.removed, vec!["Token lives in ~/.old".to_string()]);
        assert!(c.added.is_empty(), "proposed USER.md lines are not added: {c:?}");
        assert_eq!(
            c.block(),
            "What changed: 0 added, 2 merged, 1 retired\n- Merged: \"uses pnpm\", \"likes dark mode\"\n- Retired: \"Token lives in ~/.old\"\n\n"
        );
        let shown = with_block(&c, DREAM);
        assert!(shown.starts_with("What changed: 0 added, 2 merged, 1 retired\n"));
        assert!(shown.ends_with(DREAM), "the full report follows the block");
    }

    #[test]
    fn a_quiet_dream_and_an_empty_diff_add_no_block() {
        let quiet = "# Memory dream 2026-10-07\n\n## Merged\n\nNo duplicates found.\n\n## Retired\n\nNothing was stale.\n";
        let c = from_dream(quiet);
        assert!(c.is_empty());
        assert_eq!(with_block(&c, quiet), quiet);
        assert_eq!(with_block(&from_line_diff("  same\n"), "x"), "x");
    }

    #[test]
    fn a_skill_diff_counts_added_and_removed_steps_and_names_the_top_three() {
        let diff = "  ---\n- ---\n+ ---\n  name: deploy\n- 1. run tests\n+ 1. run tests with --locked\n+ 2. build release\n+ 3. tag it\n+ 4. push the tag\n…\n";
        let c = from_line_diff(diff);
        assert_eq!(c.added.len(), 4);
        assert_eq!(c.removed, vec!["1. run tests".to_string()]);
        assert_eq!(
            c.block(),
            "What changed: 4 added, 0 merged, 1 removed\n- Added: \"1. run tests with --locked\", \"2. build release\", \"3. tag it\" and 1 more\n- Removed: \"1. run tests\"\n\n"
        );
    }

    #[test]
    fn long_names_are_cut_with_an_ellipsis() {
        let long = "a".repeat(80);
        assert_eq!(top_items(&[long]), format!("\"{}…\"", "a".repeat(ITEM_CHARS - 1)));
    }
}
