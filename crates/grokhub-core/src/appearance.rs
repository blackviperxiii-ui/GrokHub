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

/// Whether a KDE Plasma `kdeglobals` file sets a dark scheme. The window
/// background (`[Colors:Window] BackgroundNormal=r,g,b`) decides, since a
/// custom scheme can be named anything; the `[General] ColorScheme` name
/// decides when there is no color (Breeze and BreezeLight are light).
/// `None` when the file sets neither.
pub fn kde_prefers_dark(kdeglobals: &str) -> Option<bool> {
    let mut section = "";
    let mut scheme = None;
    for line in kdeglobals.lines().map(str::trim) {
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            section = name;
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        match (section, key.trim()) {
            ("Colors:Window", "BackgroundNormal") => {
                let rgb: Vec<f32> = value
                    .split(',')
                    .filter_map(|c| c.trim().parse().ok())
                    .collect();
                if let [r, g, b] = rgb[..] {
                    return Some(0.2126 * r + 0.7152 * g + 0.0722 * b < 128.0);
                }
            }
            ("General", "ColorScheme") if !value.trim().is_empty() => {
                scheme = Some(looks_dark(value))
            }
            _ => {}
        }
    }
    scheme
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

    #[test]
    fn kde_scheme_reads_from_kdeglobals() {
        let light =
            "[General]\nColorScheme=BreezeLight\n\n[Colors:Window]\nBackgroundNormal=239,240,241\n";
        let dark =
            "[General]\nColorScheme=BreezeDark\n\n[Colors:Window]\nBackgroundNormal=32,35,38\n";
        assert_eq!(kde_prefers_dark(light), Some(false));
        assert_eq!(kde_prefers_dark(dark), Some(true));
        // A custom name goes by the window color, not the name.
        assert_eq!(
            kde_prefers_dark(
                "[General]\nColorScheme=Nordic\n[Colors:Window]\nBackgroundNormal=46,52,64\n"
            ),
            Some(true)
        );
        assert_eq!(
            kde_prefers_dark(
                "[General]\nColorScheme=MyDarkish\n[Colors:Window]\nBackgroundNormal=250,250,250\n"
            ),
            Some(false)
        );
        // No colors: the name decides. Plain Breeze is light.
        assert_eq!(
            kde_prefers_dark("[General]\nColorScheme=Breeze\n"),
            Some(false)
        );
        assert_eq!(
            kde_prefers_dark("[General]\nColorScheme=BreezeDark\n"),
            Some(true)
        );
        // BackgroundNormal in another section is not the window.
        assert_eq!(
            kde_prefers_dark("[Colors:View]\nBackgroundNormal=10,10,10\n"),
            None
        );
        assert_eq!(kde_prefers_dark(""), None);
    }
}
