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

pub fn clean_tab_title(name: &str) -> Option<String> {
    let t: String = name.trim().chars().take(80).collect();
    if t.is_empty() {
        None
    } else {
        Some(t)
    }
}

pub fn apply_manual_rename(tab: &mut ThreadTab, name: &str) -> bool {
    let Some(title) = clean_tab_title(name) else {
        return false;
    };
    tab.title = title;
    tab.title_locked = true;
    true
}

pub fn apply_auto_title(tab: &mut ThreadTab, name: &str) -> bool {
    if tab.title_locked {
        return false;
    }
    let Some(title) = clean_tab_title(name) else {
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
        assert_eq!(tab.title, "porn and comics");
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
