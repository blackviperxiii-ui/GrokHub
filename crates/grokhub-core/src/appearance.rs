//! Cabin appearance. Dark, or System (follow the desktop). No Light option.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeChoice {
    Dark,
    System,
}

pub fn appearance_choices() -> &'static [ThemeChoice] {
    &[ThemeChoice::Dark, ThemeChoice::System]
}

pub fn theme_id(choice: ThemeChoice) -> &'static str {
    match choice {
        ThemeChoice::Dark => "dark",
        ThemeChoice::System => "system",
    }
}

pub fn theme_label(choice: ThemeChoice) -> &'static str {
    match choice {
        ThemeChoice::Dark => "Dark",
        ThemeChoice::System => "System",
    }
}

pub fn parse_theme(raw: &str) -> ThemeChoice {
    match raw.trim().to_ascii_lowercase().as_str() {
        "system" => ThemeChoice::System,
        _ => ThemeChoice::Dark,
    }
}

pub fn resolve_dark(choice: ThemeChoice, os_dark: bool) -> bool {
    match choice {
        ThemeChoice::Dark => true,
        ThemeChoice::System => os_dark,
    }
}

pub fn pick_theme(current: ThemeChoice, clicked: ThemeChoice) -> Option<ThemeChoice> {
    if current == clicked {
        None
    } else {
        Some(clicked)
    }
}

pub fn os_prefers_dark(color_scheme: &str, gtk_theme: &str, xfce_theme: &str) -> bool {
    if looks_light(color_scheme) {
        return false;
    }
    if looks_dark(color_scheme) {
        return true;
    }
    if looks_light(gtk_theme) || looks_light(xfce_theme) {
        return false;
    }
    if looks_dark(gtk_theme) || looks_dark(xfce_theme) {
        return true;
    }
    true
}

fn looks_light(s: &str) -> bool {
    let t = s.to_ascii_lowercase();
    t.contains("light")
}

fn looks_dark(s: &str) -> bool {
    let t = s.to_ascii_lowercase();
    t.contains("dark")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cabin_offers_dark_and_system_only() {
        let ids: Vec<&str> = appearance_choices().iter().copied().map(theme_id).collect();
        assert_eq!(ids, vec!["dark", "system"]);
        assert!(!ids.contains(&"light"));
        assert_eq!(theme_label(ThemeChoice::Dark), "Dark");
        assert_eq!(theme_label(ThemeChoice::System), "System");
    }

    #[test]
    fn light_config_becomes_dark() {
        assert_eq!(parse_theme("system"), ThemeChoice::System);
        assert_eq!(parse_theme("dark"), ThemeChoice::Dark);
        assert_eq!(parse_theme("light"), ThemeChoice::Dark);
        assert_eq!(parse_theme(""), ThemeChoice::Dark);
        assert_eq!(parse_theme("LIGHT"), ThemeChoice::Dark);
    }

    #[test]
    fn dark_ignores_the_desktop_system_follows_it() {
        assert!(resolve_dark(ThemeChoice::Dark, false));
        assert!(resolve_dark(ThemeChoice::Dark, true));
        assert!(resolve_dark(ThemeChoice::System, true));
        assert!(!resolve_dark(ThemeChoice::System, false));
        assert_eq!(
            pick_theme(ThemeChoice::Dark, ThemeChoice::System),
            Some(ThemeChoice::System)
        );
        assert!(pick_theme(ThemeChoice::Dark, ThemeChoice::Dark).is_none());
    }

    #[test]
    fn whitesur_light_desktop_is_not_dark() {
        assert!(!os_prefers_dark("default", "", "WhiteSur-Light"));
        assert!(!os_prefers_dark("prefer-light", "Adwaita", ""));
        assert!(os_prefers_dark("prefer-dark", "Adwaita", ""));
        assert!(os_prefers_dark("default", "Adwaita-dark", ""));
        assert!(os_prefers_dark("", "", ""));
    }
}
