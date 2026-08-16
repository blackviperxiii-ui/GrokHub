//! Cabin appearance: Dark, Light, or System (follow the desktop).

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeChoice {
    Dark,
    Light,
    System,
}

pub fn appearance_choices() -> &'static [ThemeChoice] {
    &[ThemeChoice::Dark, ThemeChoice::Light, ThemeChoice::System]
}

pub fn appearance_hint() -> &'static str {
    "Dark, Light, or System. System follows the desktop."
}

pub fn theme_id(choice: ThemeChoice) -> &'static str {
    match choice {
        ThemeChoice::Dark => "dark",
        ThemeChoice::Light => "light",
        ThemeChoice::System => "system",
    }
}

pub fn theme_label(choice: ThemeChoice) -> &'static str {
    match choice {
        ThemeChoice::Dark => "Dark",
        ThemeChoice::Light => "Light",
        ThemeChoice::System => "System",
    }
}

pub fn parse_theme(raw: &str) -> ThemeChoice {
    match raw.trim().to_ascii_lowercase().as_str() {
        "light" => ThemeChoice::Light,
        "system" => ThemeChoice::System,
        _ => ThemeChoice::Dark,
    }
}

pub fn resolve_dark(choice: ThemeChoice, os_dark: bool) -> bool {
    match choice {
        ThemeChoice::Dark => true,
        ThemeChoice::Light => false,
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
    fn cabin_offers_dark_light_and_system() {
        let ids: Vec<&str> = appearance_choices().iter().copied().map(theme_id).collect();
        assert_eq!(ids, vec!["dark", "light", "system"]);
        assert_eq!(theme_label(ThemeChoice::Dark), "Dark");
        assert_eq!(theme_label(ThemeChoice::Light), "Light");
        assert_eq!(theme_label(ThemeChoice::System), "System");
        assert_eq!(
            appearance_hint(),
            "Dark, Light, or System. System follows the desktop."
        );
    }

    #[test]
    fn light_config_is_light() {
        assert_eq!(parse_theme("system"), ThemeChoice::System);
        assert_eq!(parse_theme("dark"), ThemeChoice::Dark);
        assert_eq!(parse_theme("light"), ThemeChoice::Light);
        assert_eq!(parse_theme("LIGHT"), ThemeChoice::Light);
        assert_eq!(parse_theme(""), ThemeChoice::Dark);
    }

    #[test]
    fn dark_and_light_ignore_the_desktop_system_follows_it() {
        assert!(resolve_dark(ThemeChoice::Dark, false));
        assert!(resolve_dark(ThemeChoice::Dark, true));
        assert!(!resolve_dark(ThemeChoice::Light, false));
        assert!(!resolve_dark(ThemeChoice::Light, true));
        assert!(resolve_dark(ThemeChoice::System, true));
        assert!(!resolve_dark(ThemeChoice::System, false));
        assert_eq!(
            pick_theme(ThemeChoice::Dark, ThemeChoice::Light),
            Some(ThemeChoice::Light)
        );
        assert!(pick_theme(ThemeChoice::Light, ThemeChoice::Light).is_none());
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
