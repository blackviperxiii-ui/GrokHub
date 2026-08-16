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

pub fn history_order(pinned: &[bool]) -> Vec<usize> {
    let mut pins = Vec::new();
    let mut rest = Vec::new();
    for (i, on) in pinned.iter().enumerate() {
        if *on {
            pins.push(i);
        } else {
            rest.push(i);
        }
    }
    pins.extend(rest);
    pins
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
        assert_eq!(history_order(&[false, true, false, true]), vec![1, 3, 0, 2]);
        assert_eq!(delete_thread(3, 0, 0), DeleteOutcome::Removed { next: 0 });
        assert_eq!(delete_thread(3, 0, 2), DeleteOutcome::Removed { next: 1 });
        assert_eq!(delete_thread(3, 2, 2), DeleteOutcome::Removed { next: 1 });
        assert_eq!(delete_thread(1, 0, 0), DeleteOutcome::ResetLast);
    }
}
