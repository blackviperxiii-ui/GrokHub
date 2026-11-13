//! Parsers for what the OS says about text size, motion, contrast, dark
//! mode, the clock and the machine's model. Pure: the readers in
//! `grokhub_agent::device` run `gsettings`, read `kdeglobals` or run
//! `reg query` and pass the text here.

use super::device::OsLook;

/// `gsettings get org.gnome.desktop.interface text-scaling-factor` → `1.25`.
pub fn gnome_text_scale(out: &str) -> Option<u16> {
    let f: f64 = out.trim().trim_matches('\'').parse().ok()?;
    (0.5..=4.0).contains(&f).then(|| (f * 100.0).round() as u16)
}

/// `gsettings get … enable-animations` → `false` means reduce motion.
pub fn gnome_bool(out: &str) -> Option<bool> {
    match out.trim().trim_matches('\'') {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

/// `gsettings get org.gnome.desktop.interface color-scheme` →
/// `'prefer-dark'` / `'prefer-light'` / `'default'`.
pub fn gnome_dark(out: &str) -> Option<bool> {
    match out.trim().trim_matches('\'') {
        "prefer-dark" => Some(true),
        "prefer-light" => Some(false),
        // GNOME's default is light.
        "default" => Some(false),
        _ => None,
    }
}

/// The four GNOME answers together: text scale, animations, colour scheme,
/// and the a11y high-contrast key. Pass `None` for a key that didn't answer.
pub fn gnome_look(
    text_scale: Option<&str>,
    animations: Option<&str>,
    color_scheme: Option<&str>,
    high_contrast: Option<&str>,
) -> OsLook {
    OsLook {
        text_scale_pct: text_scale.and_then(gnome_text_scale),
        reduce_motion: animations.and_then(gnome_bool).map(|on| !on),
        high_contrast: high_contrast.and_then(gnome_bool),
        prefers_dark: color_scheme.and_then(gnome_dark),
    }
}

/// One `key=value` in an ini section (`[KDE]`, `[General]`).
fn ini_value<'a>(text: &'a str, section: &str, key: &str) -> Option<&'a str> {
    let mut inside = false;
    for line in text.lines() {
        let l = line.trim();
        if l.starts_with('[') {
            inside = l.trim_start_matches('[').trim_end_matches(']') == section;
            continue;
        }
        if inside {
            if let Some((k, v)) = l.split_once('=') {
                if k.trim() == key {
                    return Some(v.trim());
                }
            }
        }
    }
    None
}

/// KDE Plasma: `~/.config/kdeglobals` (animation speed, colour scheme) and
/// `~/.config/kcmfonts` (forced font DPI, 96 = 100 %).
pub fn kde_look(kdeglobals: Option<&str>, kcmfonts: Option<&str>) -> OsLook {
    let g = kdeglobals.unwrap_or("");
    let reduce_motion = ini_value(g, "KDE", "AnimationDurationFactor")
        .and_then(|v| v.parse::<f64>().ok())
        .map(|f| f == 0.0);
    let scheme = ini_value(g, "General", "ColorScheme").map(str::to_ascii_lowercase);
    let prefers_dark = scheme.as_deref().map(|s| s.contains("dark"));
    let high_contrast = scheme.as_deref().map(|s| s.contains("contrast"));
    let text_scale_pct = kcmfonts
        .and_then(|f| ini_value(f, "General", "forceFontDPI"))
        .and_then(|v| v.parse::<u32>().ok())
        .filter(|dpi| *dpi > 0)
        .map(|dpi| ((dpi * 100 + 48) / 96) as u16);
    OsLook {
        text_scale_pct,
        reduce_motion,
        high_contrast,
        prefers_dark,
    }
}

/// The value of one `reg query … /v Name` line:
/// `    TextScaleFactor    REG_DWORD    0x96` → `0x96`.
pub fn reg_value(out: &str, name: &str) -> Option<String> {
    out.lines().find_map(|l| {
        let mut parts = l.split_whitespace();
        if !parts.next()?.eq_ignore_ascii_case(name) {
            return None;
        }
        if !parts.next()?.starts_with("REG_") {
            return None;
        }
        let v = parts.collect::<Vec<_>>().join(" ");
        (!v.is_empty()).then_some(v)
    })
}

/// A REG_DWORD (`0x96`) or REG_SZ number (`150`).
pub fn reg_number(out: &str, name: &str) -> Option<u32> {
    let v = reg_value(out, name)?;
    match v.strip_prefix("0x").or_else(|| v.strip_prefix("0X")) {
        Some(hex) => u32::from_str_radix(hex, 16).ok(),
        None => v.parse().ok(),
    }
}

/// Windows, from four `reg query` answers:
/// - `HKCU\Software\Microsoft\Accessibility /v TextScaleFactor` (100-225),
/// - `HKCU\Control Panel\Desktop\WindowMetrics /v MinAnimate` (`0` = animations off),
/// - `HKCU\Control Panel\Accessibility\HighContrast /v Flags` (bit 1 = on),
/// - `HKCU\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize /v AppsUseLightTheme`.
pub fn windows_look(
    text_scale: Option<&str>,
    min_animate: Option<&str>,
    high_contrast: Option<&str>,
    light_theme: Option<&str>,
) -> OsLook {
    OsLook {
        text_scale_pct: text_scale
            .and_then(|o| reg_number(o, "TextScaleFactor"))
            .filter(|p| (100..=225).contains(p))
            .map(|p| p as u16)
            // No value set means the Windows default, 100 %.
            .or(text_scale.map(|_| 100)),
        reduce_motion: min_animate
            .and_then(|o| reg_number(o, "MinAnimate"))
            .map(|v| v == 0),
        high_contrast: high_contrast
            .and_then(|o| reg_number(o, "Flags"))
            .map(|f| f & 1 == 1),
        prefers_dark: light_theme
            .and_then(|o| reg_number(o, "AppsUseLightTheme"))
            .map(|v| v == 0),
    }
}

/// `HKCU\Control Panel\International /v iTime`: `0` = 12-hour, `1` = 24-hour.
pub fn windows_clock_12h(out: &str) -> Option<bool> {
    reg_number(out, "iTime").map(|v| v == 0)
}

/// `HKCU\Control Panel\International /v LocaleName` → `en-US`.
pub fn windows_locale(out: &str) -> Option<String> {
    reg_value(out, "LocaleName")
        .map(|v| v.trim().replace('-', "_"))
        .filter(|v| !v.is_empty())
}

/// The maker's model from DMI or `Win32_ComputerSystem`, without filler
/// ("To Be Filled By O.E.M.", "System Product Name").
pub fn clean_model(raw: &str) -> String {
    let m = raw.trim();
    let filler = [
        "to be filled by o.e.m.",
        "system product name",
        "default string",
        "not applicable",
        "none",
        "",
    ];
    if filler.contains(&m.to_ascii_lowercase().as_str()) {
        String::new()
    } else {
        m.to_string()
    }
}

/// The locale from the environment: `LC_ALL`, then `LC_TIME`, then `LANG`
/// (the first that is set and not `C` / `POSIX`).
pub fn env_locale(lc_all: Option<&str>, lc_time: Option<&str>, lang: Option<&str>) -> String {
    [lc_all, lc_time, lang]
        .into_iter()
        .flatten()
        .map(|v| v.split('.').next().unwrap_or("").trim())
        .find(|v| !v.is_empty() && *v != "C" && *v != "POSIX")
        .unwrap_or("")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gnome_answers() {
        assert_eq!(gnome_text_scale("1.25\n"), Some(125));
        assert_eq!(gnome_text_scale("1.0"), Some(100));
        assert_eq!(gnome_text_scale("nonsense"), None);
        assert_eq!(gnome_text_scale("9.0"), None);
        assert_eq!(
            gnome_look(
                Some("1.5"),
                Some("false"),
                Some("'prefer-dark'"),
                Some("true")
            ),
            OsLook {
                text_scale_pct: Some(150),
                reduce_motion: Some(true),
                high_contrast: Some(true),
                prefers_dark: Some(true),
            }
        );
        assert_eq!(
            gnome_look(None, Some("true"), Some("'default'"), None),
            OsLook {
                text_scale_pct: None,
                reduce_motion: Some(false),
                high_contrast: None,
                prefers_dark: Some(false),
            }
        );
    }

    #[test]
    fn kde_answers() {
        let globals = "[General]\nColorScheme=BreezeDark\n\n[KDE]\nAnimationDurationFactor=0\nLookAndFeelPackage=org.kde.breezedark.desktop\n";
        let fonts = "[General]\nforceFontDPI=144\n";
        assert_eq!(
            kde_look(Some(globals), Some(fonts)),
            OsLook {
                text_scale_pct: Some(150),
                reduce_motion: Some(true),
                high_contrast: Some(false),
                prefers_dark: Some(true),
            }
        );
        let plain = "[General]\nColorScheme=BreezeLight\n[KDE]\nAnimationDurationFactor=0.5\n";
        let none = kde_look(Some(plain), Some("[General]\nforceFontDPI=0\n"));
        assert_eq!(none.text_scale_pct, None);
        assert_eq!(none.reduce_motion, Some(false));
        assert_eq!(none.prefers_dark, Some(false));
        assert_eq!(kde_look(None, None), OsLook::default());
    }

    #[test]
    fn windows_registry_answers() {
        let scale = "\r\nHKEY_CURRENT_USER\\Software\\Microsoft\\Accessibility\r\n    TextScaleFactor    REG_DWORD    0x96\r\n\r\n";
        let anim = "HKEY_CURRENT_USER\\Control Panel\\Desktop\\WindowMetrics\n    MinAnimate    REG_SZ    0\n";
        let hc = "    Flags    REG_SZ    127\n";
        let theme = "    AppsUseLightTheme    REG_DWORD    0x0\n";
        assert_eq!(
            windows_look(Some(scale), Some(anim), Some(hc), Some(theme)),
            OsLook {
                text_scale_pct: Some(150),
                reduce_motion: Some(true),
                high_contrast: Some(true),
                prefers_dark: Some(true),
            }
        );
        // Text size never changed: the value is missing, which is 100 %.
        let unset = "HKEY_CURRENT_USER\\Software\\Microsoft\\Accessibility\n";
        let look = windows_look(
            Some(unset),
            None,
            Some("    Flags    REG_SZ    126\n"),
            None,
        );
        assert_eq!(look.text_scale_pct, Some(100));
        assert_eq!(look.high_contrast, Some(false));
        assert_eq!(look.reduce_motion, None);
        assert_eq!(windows_look(None, None, None, None), OsLook::default());
    }

    #[test]
    fn windows_clock_and_locale() {
        assert_eq!(windows_clock_12h("    iTime    REG_SZ    0\n"), Some(true));
        assert_eq!(windows_clock_12h("    iTime    REG_SZ    1\n"), Some(false));
        assert_eq!(windows_clock_12h(""), None);
        assert_eq!(
            windows_locale("    LocaleName    REG_SZ    en-US\n").as_deref(),
            Some("en_US")
        );
        assert_eq!(windows_locale(""), None);
    }

    #[test]
    fn models_and_env_locale() {
        assert_eq!(clean_model("Surface Pro 7\n"), "Surface Pro 7");
        assert_eq!(clean_model("To Be Filled By O.E.M."), "");
        assert_eq!(clean_model("System Product Name"), "");
        assert_eq!(env_locale(None, None, Some("en_US.UTF-8")), "en_US");
        assert_eq!(
            env_locale(Some("C"), Some("de_DE.UTF-8"), Some("en_US")),
            "de_DE"
        );
        assert_eq!(env_locale(None, None, None), "");
    }
}
