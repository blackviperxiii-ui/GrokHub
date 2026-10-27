//! Cabin appearance: Loadout dark themes, classic Dark/Light, or System.

/// Settings → Appearance choices. New installs default to [`ThemeChoice::OpsDark`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeChoice {
    /// Loadout primary dark (`#0B0B0D` base). Default for new installs.
    OpsDark,
    /// Loadout OLED variant (`#000000` base).
    OledBlack,
    /// Loadout softer charcoal variant (`#17181B` base).
    SoftCharcoal,
    /// Classic Grok cabin dark (true black canvas). Kept for existing users.
    Dark,
    Light,
    System,
}

/// Which dark token set to paint when the cabin is not in light mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DarkPalette {
    /// Classic Grok OLED black tokens.
    Classic,
    OpsDark,
    OledBlack,
    SoftCharcoal,
}

pub fn appearance_choices() -> &'static [ThemeChoice] {
    &[
        ThemeChoice::OpsDark,
        ThemeChoice::OledBlack,
        ThemeChoice::SoftCharcoal,
        ThemeChoice::Dark,
        ThemeChoice::Light,
        ThemeChoice::System,
    ]
}

pub fn appearance_hint() -> &'static str {
    "Ops Dark is the Loadout default. OLED Black and Soft Charcoal are variants. Dark, Light, and System keep the classic cabin look. System follows the desktop."
}

pub fn theme_id(choice: ThemeChoice) -> &'static str {
    match choice {
        ThemeChoice::OpsDark => "ops-dark",
        ThemeChoice::OledBlack => "oled-black",
        ThemeChoice::SoftCharcoal => "soft-charcoal",
        ThemeChoice::Dark => "dark",
        ThemeChoice::Light => "light",
        ThemeChoice::System => "system",
    }
}

pub fn theme_label(choice: ThemeChoice) -> &'static str {
    match choice {
        ThemeChoice::OpsDark => "Ops Dark",
        ThemeChoice::OledBlack => "OLED Black",
        ThemeChoice::SoftCharcoal => "Soft Charcoal",
        ThemeChoice::Dark => "Dark",
        ThemeChoice::Light => "Light",
        ThemeChoice::System => "System",
    }
}

/// Parse a stored theme id. Empty or unknown values become Ops Dark (new-install default).
/// Existing `"dark"` / `"light"` / `"system"` strings keep their classic meaning.
pub fn parse_theme(raw: &str) -> ThemeChoice {
    match raw.trim().to_ascii_lowercase().as_str() {
        "ops-dark" | "ops_dark" | "opsdark" => ThemeChoice::OpsDark,
        "oled-black" | "oled_black" | "oledblack" | "oled" => ThemeChoice::OledBlack,
        "soft-charcoal" | "soft_charcoal" | "softcharcoal" | "charcoal" => {
            ThemeChoice::SoftCharcoal
        }
        "light" => ThemeChoice::Light,
        "system" => ThemeChoice::System,
        "dark" => ThemeChoice::Dark,
        _ => ThemeChoice::OpsDark,
    }
}

pub fn resolve_dark(choice: ThemeChoice, os_dark: bool) -> bool {
    match choice {
        ThemeChoice::Light => false,
        ThemeChoice::System => os_dark,
        ThemeChoice::OpsDark
        | ThemeChoice::OledBlack
        | ThemeChoice::SoftCharcoal
        | ThemeChoice::Dark => true,
    }
}

/// Dark token set for a choice, or `None` when light paint should run.
/// System + dark desktop keeps the classic Grok dark set so existing System
/// users do not jump to Loadout chrome.
pub fn resolve_palette(choice: ThemeChoice, os_dark: bool) -> Option<DarkPalette> {
    match choice {
        ThemeChoice::Light => None,
        ThemeChoice::System => {
            if os_dark {
                Some(DarkPalette::Classic)
            } else {
                None
            }
        }
        ThemeChoice::Dark => Some(DarkPalette::Classic),
        ThemeChoice::OpsDark => Some(DarkPalette::OpsDark),
        ThemeChoice::OledBlack => Some(DarkPalette::OledBlack),
        ThemeChoice::SoftCharcoal => Some(DarkPalette::SoftCharcoal),
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
    fn cabin_offers_loadout_and_classic_themes() {
        let ids: Vec<&str> = appearance_choices().iter().copied().map(theme_id).collect();
        assert_eq!(
            ids,
            vec![
                "ops-dark",
                "oled-black",
                "soft-charcoal",
                "dark",
                "light",
                "system"
            ]
        );
        assert_eq!(theme_label(ThemeChoice::OpsDark), "Ops Dark");
        assert_eq!(theme_label(ThemeChoice::OledBlack), "OLED Black");
        assert_eq!(theme_label(ThemeChoice::SoftCharcoal), "Soft Charcoal");
        assert_eq!(theme_label(ThemeChoice::Dark), "Dark");
        assert_eq!(theme_label(ThemeChoice::Light), "Light");
        assert_eq!(theme_label(ThemeChoice::System), "System");
        assert!(appearance_hint().contains("Ops Dark"));
        assert!(appearance_hint().contains("System follows the desktop"));
    }

    #[test]
    fn loadout_and_classic_config_ids_round_trip() {
        assert_eq!(parse_theme("ops-dark"), ThemeChoice::OpsDark);
        assert_eq!(parse_theme("oled-black"), ThemeChoice::OledBlack);
        assert_eq!(parse_theme("soft-charcoal"), ThemeChoice::SoftCharcoal);
        assert_eq!(parse_theme("system"), ThemeChoice::System);
        assert_eq!(parse_theme("dark"), ThemeChoice::Dark);
        assert_eq!(parse_theme("light"), ThemeChoice::Light);
        assert_eq!(parse_theme("LIGHT"), ThemeChoice::Light);
        assert_eq!(parse_theme(""), ThemeChoice::OpsDark);
        assert_eq!(parse_theme("sepia"), ThemeChoice::OpsDark);
    }

    #[test]
    fn dark_themes_ignore_the_desktop_system_follows_it() {
        assert!(resolve_dark(ThemeChoice::OpsDark, false));
        assert!(resolve_dark(ThemeChoice::OledBlack, false));
        assert!(resolve_dark(ThemeChoice::SoftCharcoal, true));
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
        assert_eq!(
            pick_theme(ThemeChoice::OpsDark, ThemeChoice::OledBlack),
            Some(ThemeChoice::OledBlack)
        );
    }

    #[test]
    fn resolve_palette_keeps_system_on_classic_dark() {
        assert_eq!(
            resolve_palette(ThemeChoice::OpsDark, false),
            Some(DarkPalette::OpsDark)
        );
        assert_eq!(
            resolve_palette(ThemeChoice::OledBlack, true),
            Some(DarkPalette::OledBlack)
        );
        assert_eq!(
            resolve_palette(ThemeChoice::SoftCharcoal, false),
            Some(DarkPalette::SoftCharcoal)
        );
        assert_eq!(
            resolve_palette(ThemeChoice::Dark, false),
            Some(DarkPalette::Classic)
        );
        assert_eq!(resolve_palette(ThemeChoice::Light, true), None);
        assert_eq!(
            resolve_palette(ThemeChoice::System, true),
            Some(DarkPalette::Classic)
        );
        assert_eq!(resolve_palette(ThemeChoice::System, false), None);
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
