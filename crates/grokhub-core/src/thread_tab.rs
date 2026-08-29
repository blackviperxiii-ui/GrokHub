//! Chat tab pin, delete, and a rename that the goal namer cannot overwrite.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThreadTab {
    pub title: String,
    pub pinned: bool,
    pub title_locked: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeleteOutcome {
    Removed { next: usize },
    ResetLast,
}

pub const AUTO_TITLE_MAX: usize = 16;

pub fn clean_tab_title(name: &str) -> Option<String> {
    let t: String = name.trim().chars().take(80).collect();
    if t.is_empty() {
        None
    } else {
        Some(t)
    }
}

/// One short name for the rail. "chowder and food interest and cho" → "chowder".
pub fn short_auto_title(name: &str) -> Option<String> {
    let t = name.trim();
    if t.is_empty() {
        return None;
    }
    let first = t
        .split(" and ")
        .next()
        .unwrap_or(t)
        .split(',')
        .next()
        .unwrap_or(t)
        .trim();
    if first.is_empty() {
        return None;
    }
    Some(clip_title(first, AUTO_TITLE_MAX))
}

/// What the sidebar paints. Topic lists collapse; a manual name stays until it is too long.
pub fn display_tab_title(name: &str) -> String {
    if name.contains(" and ") || name.contains(',') {
        short_auto_title(name).unwrap_or_else(|| name.trim().to_string())
    } else {
        clip_title(name.trim(), AUTO_TITLE_MAX)
    }
}

fn clip_title(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        return s.to_string();
    }
    let mut out = String::new();
    for (i, ch) in s.chars().enumerate() {
        if i + 1 >= n {
            break;
        }
        out.push(ch);
    }
    format!("{}…", out.trim_end())
}

pub fn apply_manual_rename(tab: &mut ThreadTab, name: &str) -> bool {
    let Some(title) = clean_tab_title(name) else {
        return false;
    };
    tab.title = title;
    tab.title_locked = true;
    true
}

pub fn auto_title_blocked(title_locked: bool, renaming: bool) -> bool {
    title_locked || renaming
}

pub fn apply_auto_title(tab: &mut ThreadTab, name: &str) -> bool {
    apply_auto_title_in(tab, name, false)
}

pub fn apply_auto_title_in(tab: &mut ThreadTab, name: &str, renaming: bool) -> bool {
    if auto_title_blocked(tab.title_locked, renaming) {
        return false;
    }
    let Some(title) = short_auto_title(name) else {
        return false;
    };
    tab.title = title;
    true
}

pub fn toggle_pin(pinned: bool) -> bool {
    !pinned
}

pub fn history_order(pinned: &[bool], accessed: &[u64]) -> Vec<usize> {
    let n = pinned.len();
    let mut idx: Vec<usize> = (0..n).collect();
    idx.sort_by(|&a, &b| {
        let pa = pinned.get(a).copied().unwrap_or(false);
        let pb = pinned.get(b).copied().unwrap_or(false);
        match (pa, pb) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => {
                let aa = accessed.get(a).copied().unwrap_or(0);
                let bb = accessed.get(b).copied().unwrap_or(0);
                bb.cmp(&aa).then(b.cmp(&a))
            }
        }
    });
    idx
}

pub fn delete_thread(count: usize, idx: usize, current: usize) -> DeleteOutcome {
    if count <= 1 || idx >= count {
        return DeleteOutcome::ResetLast;
    }
    let next = if current == idx {
        idx.min(count - 2)
    } else if current > idx {
        current - 1
    } else {
        current
    };
    DeleteOutcome::Removed { next }
}

/// Default History title for a fresh chat or scratch tab.
pub fn default_thread_title(scratch: bool) -> &'static str {
    if scratch {
        "Scratch"
    } else {
        "Chat"
    }
}

/// Empty default-titled leftover from New chat / restart. Keep the current tab.
pub fn leftover_empty_thread(title: &str, scratch: bool, empty: bool) -> bool {
    empty && title.trim().eq_ignore_ascii_case(default_thread_title(scratch))
}

pub fn history_row_visible(
    title: &str,
    scratch: bool,
    empty: bool,
    is_current: bool,
    pinned: bool,
) -> bool {
    is_current || pinned || !leftover_empty_thread(title, scratch, empty)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThreadReuseView<'a> {
    pub title: &'a str,
    pub scratch: bool,
    pub empty: bool,
}

/// Reuse this empty tab, or another leftover empty of the same kind, instead of stacking Chats.
pub fn reuse_empty_thread_idx(
    threads: &[ThreadReuseView<'_>],
    current: usize,
    want_scratch: bool,
) -> Option<usize> {
    if let Some(t) = threads.get(current) {
        if t.scratch == want_scratch && t.empty {
            return Some(current);
        }
    }
    threads.iter().position(|t| {
        t.scratch == want_scratch && leftover_empty_thread(t.title, t.scratch, t.empty)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rename_is_permanent_against_auto_title() {
        let mut tab = ThreadTab {
            title: "Chat".into(),
            pinned: false,
            title_locked: false,
        };
        assert!(apply_manual_rename(&mut tab, "  night watch  "));
        assert_eq!(tab.title, "night watch");
        assert!(tab.title_locked);
        assert!(!apply_manual_rename(&mut tab, "   "));
        assert_eq!(tab.title, "night watch");
        assert!(!apply_auto_title(&mut tab, "porn"));
        assert_eq!(tab.title, "night watch");
        assert!(tab.title_locked);
        let mut open = ThreadTab {
            title: "Chat".into(),
            pinned: false,
            title_locked: false,
        };
        assert!(auto_title_blocked(false, true));
        assert!(!apply_auto_title_in(&mut open, "porn", true));
        assert_eq!(open.title, "Chat");
    }

    #[test]
    fn auto_title_works_until_someone_renames() {
        let mut tab = ThreadTab {
            title: "Chat".into(),
            pinned: false,
            title_locked: false,
        };
        assert!(apply_auto_title(&mut tab, "porn"));
        assert_eq!(tab.title, "porn");
        assert!(!tab.title_locked);
        assert!(apply_auto_title(&mut tab, "porn and comics"));
        assert_eq!(tab.title, "porn");
    }

    #[test]
    fn auto_title_is_one_short_name() {
        assert_eq!(
            short_auto_title("chowder and food interest and cho").as_deref(),
            Some("chowder")
        );
        assert_eq!(
            display_tab_title("chowder and food interest and cho"),
            "chowder"
        );
        assert_eq!(display_tab_title("food interest"), "food interest");
        let long = display_tab_title("supercalifragilistic");
        assert!(long.chars().count() <= AUTO_TITLE_MAX, "{long}");
        assert!(long.ends_with('…'), "{long}");
    }

    #[test]
    fn pin_sorts_to_the_top_and_delete_keeps_a_tab() {
        assert!(toggle_pin(false));
        assert!(!toggle_pin(true));
        assert_eq!(
            history_order(&[false, true, false, true], &[0, 0, 0, 0]),
            vec![3, 1, 2, 0],
            "pins first (newest pin first), then newest unpinned"
        );
        assert_eq!(
            history_order(&[false, false, false], &[1_000, 9_000, 5_000]),
            vec![1, 2, 0],
            "newest accessed chats sit on top"
        );
        assert_eq!(
            history_order(&[true, false, true], &[1, 99, 2]),
            vec![2, 0, 1],
            "pinned chats stay above unpinned, newest pin first"
        );
        assert_eq!(delete_thread(3, 0, 0), DeleteOutcome::Removed { next: 0 });
        assert_eq!(delete_thread(3, 0, 2), DeleteOutcome::Removed { next: 1 });
        assert_eq!(delete_thread(3, 2, 2), DeleteOutcome::Removed { next: 1 });
        assert_eq!(delete_thread(1, 0, 0), DeleteOutcome::ResetLast);
    }

    #[test]
    fn new_chat_reuses_empty_tabs_and_hides_leftovers() {
        assert_eq!(default_thread_title(false), "Chat");
        assert_eq!(default_thread_title(true), "Scratch");
        assert!(leftover_empty_thread("Chat", false, true));
        assert!(leftover_empty_thread("scratch", true, true));
        assert!(!leftover_empty_thread("night watch", false, true));
        assert!(!leftover_empty_thread("Chat", false, false));
        assert!(history_row_visible("Chat", false, true, true, false));
        assert!(!history_row_visible("Chat", false, true, false, false));
        assert!(history_row_visible("Chat", false, true, false, true));
        assert!(history_row_visible("casual greeting", false, false, false, false));
        let tabs = [
            ThreadReuseView {
                title: "casual greeting",
                scratch: false,
                empty: false,
            },
            ThreadReuseView {
                title: "Chat",
                scratch: false,
                empty: true,
            },
            ThreadReuseView {
                title: "Chat",
                scratch: false,
                empty: true,
            },
            ThreadReuseView {
                title: "Scratch",
                scratch: true,
                empty: true,
            },
        ];
        assert_eq!(reuse_empty_thread_idx(&tabs, 0, false), Some(1));
        assert_eq!(reuse_empty_thread_idx(&tabs, 1, false), Some(1));
        assert_eq!(reuse_empty_thread_idx(&tabs, 0, true), Some(3));
        let already = [ThreadReuseView {
            title: "Chat",
            scratch: false,
            empty: true,
        }];
        assert_eq!(reuse_empty_thread_idx(&already, 0, false), Some(0));
        assert_eq!(reuse_empty_thread_idx(&already, 0, true), None);
    }
}
