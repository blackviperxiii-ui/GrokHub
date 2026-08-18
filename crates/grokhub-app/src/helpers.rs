use grokhub_core::{chip_thread_from_messages, ChipThread};

use crate::threads;

pub fn click_project_opens_board(already_selected: bool) -> bool {
    already_selected
}

pub fn collect_other_chip_threads(threads: &[threads::ChatThread], current_id: &str) -> Vec<ChipThread> {
    threads
        .iter()
        .rev()
        .filter(|t| t.id != current_id && !t.scratch)
        .filter_map(|t| chip_thread_from_messages(&t.title, &t.messages))
        .take(6)
        .collect()
}

pub fn next_maximized(currently: bool) -> bool {
    !currently
}

pub fn cabin_menu_should_dismiss(ignore: bool, outside_click: bool) -> bool {
    !ignore && outside_click
}

pub fn next_starter_skill_name(existing: &[String]) -> String {
    if !existing.iter().any(|n| n == "new-skill") {
        return "new-skill".into();
    }
    let mut i = 2_u32;
    loop {
        let name = format!("new-skill-{i}");
        if !existing.iter().any(|n| n == &name) {
            return name;
        }
        i = i.saturating_add(1);
        if i > 99 {
            return format!("new-skill-{i}");
        }
    }
}

pub fn wants_live_repaint(
    running: bool,
    chip_busy: bool,
    hub_on: bool,
    window_visible: bool,
    imagine: bool,
    wall_busy: bool,
) -> bool {
    let _ = window_visible;
    running || chip_busy || hub_on || imagine || wall_busy
}

pub fn expand_home(p: &str) -> String {
    grokhub_core::expand_project_root(p, std::env::var("HOME").ok().as_deref())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maximize_toggles_restore() {
        assert!(next_maximized(false));
        assert!(!next_maximized(true));
    }

    #[test]
    fn cabin_menu_closes_on_outside_click() {
        assert!(cabin_menu_should_dismiss(false, true));
        assert!(!cabin_menu_should_dismiss(true, true));
        assert!(!cabin_menu_should_dismiss(false, false));
    }

    #[test]
    fn new_skill_gets_a_free_name() {
        assert_eq!(next_starter_skill_name(&[]), "new-skill");
        assert_eq!(
            next_starter_skill_name(&["new-skill".into()]),
            "new-skill-2"
        );
        assert_eq!(
            next_starter_skill_name(&["new-skill".into(), "new-skill-2".into()]),
            "new-skill-3"
        );
    }

    #[test]
    fn selected_project_opens_the_board() {
        assert!(click_project_opens_board(true));
        assert!(!click_project_opens_board(false));
    }

    #[test]
    fn expand_home_understands_dollar_home() {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/home/j".into());
        assert_eq!(expand_home("$HOME/proj"), format!("{home}/proj"));
        assert_eq!(expand_home("~/proj"), format!("{home}/proj"));
    }
}
