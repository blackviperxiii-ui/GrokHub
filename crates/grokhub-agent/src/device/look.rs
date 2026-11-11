//! The auto setup's readers beyond the hardware: what the OS says about how
//! things should look, the clock and language, the maker's model name, and a
//! few things installed (Steam, git folders, a microphone).

use std::path::Path;

use grokhub_core::autotune::device::{self as dev, DeviceProfile, OsLook};
use grokhub_core::autotune::os_signals as os;
use grokhub_core::local_profile::LocalProfile;

#[cfg(any(target_os = "linux", windows, target_os = "macos"))]
use super::run;

/// The whole record: the hardware from `profile`, then everything else.
/// Slow: call it off the UI thread.
pub fn read_device(home: &Path, profile: &LocalProfile) -> DeviceProfile {
    let mut d = DeviceProfile::from_local(profile);
    d.look = read_look();
    d.model = read_model();
    let (locale, clock) = read_locale();
    d.clock_12h = clock.unwrap_or_else(|| dev::locale_uses_12h(&locale));
    d.locale = locale;
    d.steam = grokhub_core::steam::detect_root().is_some();
    d.dev_folders = has_dev_folders(home);
    let audio = grokhub_core::audio_check::read_facts();
    d.mic = (!audio.is_empty()).then(|| {
        !audio.inputs.is_empty()
            || audio
                .devices
                .iter()
                .any(|(n, _)| n.to_ascii_lowercase().contains("mic"))
    });
    d.read_at_ms = grokhub_core::now_ms();
    d
}

/// What the OS says about text size, motion, contrast and dark mode.
pub fn read_look() -> OsLook {
    #[cfg(target_os = "linux")]
    {
        let g = |schema: &str, key: &str| run("gsettings", &["get", schema, key]);
        let gnome = os::gnome_look(
            g("org.gnome.desktop.interface", "text-scaling-factor").as_deref(),
            g("org.gnome.desktop.interface", "enable-animations").as_deref(),
            g("org.gnome.desktop.interface", "color-scheme").as_deref(),
            g("org.gnome.desktop.a11y.interface", "high-contrast").as_deref(),
        );
        let cfg = grokhub_core::paths::user_home().map(|h| h.join(".config"));
        let kde_desktop = std::env::var("XDG_CURRENT_DESKTOP")
            .map(|d| d.to_ascii_uppercase().contains("KDE"))
            .unwrap_or(false);
        let kde = cfg
            .map(|c| {
                os::kde_look(
                    std::fs::read_to_string(c.join("kdeglobals"))
                        .ok()
                        .as_deref(),
                    std::fs::read_to_string(c.join("kcmfonts")).ok().as_deref(),
                )
            })
            .unwrap_or_default();
        // On Plasma the KDE files are the truth; GNOME keys may be stale defaults.
        if kde_desktop {
            kde.or(gnome)
        } else {
            gnome.or(kde)
        }
    }
    #[cfg(windows)]
    {
        let q = |key: &str, value: &str| run("reg", &["query", key, "/v", value]);
        // A missing TextScaleFactor value means 100 %; reg exits non-zero, so
        // pass an empty answer when the key itself reads.
        let scale = q(
            "HKCU\\Software\\Microsoft\\Accessibility",
            "TextScaleFactor",
        )
        .or_else(|| {
            run(
                "reg",
                &["query", "HKCU\\Software\\Microsoft\\Accessibility"],
            )
            .map(|_| String::new())
        });
        os::windows_look(
            scale.as_deref(),
            q("HKCU\\Control Panel\\Desktop\\WindowMetrics", "MinAnimate").as_deref(),
            q("HKCU\\Control Panel\\Accessibility\\HighContrast", "Flags").as_deref(),
            q(
                "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize",
                "AppsUseLightTheme",
            )
            .as_deref(),
        )
    }
    #[cfg(target_os = "macos")]
    {
        let dark = run("defaults", &["read", "-g", "AppleInterfaceStyle"])
            .map(|s| s.trim().eq_ignore_ascii_case("dark"));
        let motion = run(
            "defaults",
            &["read", "com.apple.universalaccess", "reduceMotion"],
        )
        .map(|s| s.trim() == "1");
        OsLook {
            prefers_dark: Some(dark.unwrap_or(false)),
            reduce_motion: motion,
            ..OsLook::default()
        }
    }
    #[cfg(not(any(target_os = "linux", windows, target_os = "macos")))]
    {
        OsLook::default()
    }
}

/// The maker's model name: DMI on Linux, `Win32_ComputerSystem` on Windows.
fn read_model() -> String {
    #[cfg(target_os = "linux")]
    {
        let read = |p: &str| std::fs::read_to_string(p).ok().map(|s| os::clean_model(&s));
        let product = read("/sys/class/dmi/id/product_name").unwrap_or_default();
        let family = read("/sys/class/dmi/id/product_family").unwrap_or_default();
        if product.is_empty() {
            family
        } else if !family.is_empty() && !product.contains(&family) {
            format!("{family} {product}")
        } else {
            product
        }
    }
    #[cfg(windows)]
    {
        run(
            "powershell",
            &[
                "-NoProfile",
                "-Command",
                "(Get-CimInstance Win32_ComputerSystem).Model",
            ],
        )
        .map(|s| os::clean_model(&s))
        .unwrap_or_default()
    }
    #[cfg(target_os = "macos")]
    {
        run("sysctl", &["-n", "hw.model"])
            .map(|s| os::clean_model(&s))
            .unwrap_or_default()
    }
    #[cfg(not(any(target_os = "linux", windows, target_os = "macos")))]
    {
        String::new()
    }
}

/// The locale and, where the OS says it directly, whether times are 12-hour.
fn read_locale() -> (String, Option<bool>) {
    #[cfg(windows)]
    {
        let key = "HKCU\\Control Panel\\International";
        let locale = run("reg", &["query", key, "/v", "LocaleName"])
            .and_then(|o| os::windows_locale(&o))
            .unwrap_or_default();
        let clock =
            run("reg", &["query", key, "/v", "iTime"]).and_then(|o| os::windows_clock_12h(&o));
        (locale, clock)
    }
    #[cfg(not(windows))]
    {
        let var = |k: &str| std::env::var(k).ok();
        let locale = os::env_locale(
            var("LC_ALL").as_deref(),
            var("LC_TIME").as_deref(),
            var("LANG").as_deref(),
        );
        (locale, None)
    }
}

/// A folder of git projects one level under a usual place in the home folder
/// (`~/code/<repo>/.git`, `~/Documents/GitHub/<repo>/.git`).
pub fn has_dev_folders(home: &Path) -> bool {
    const ROOTS: &[&str] = &[
        "code",
        "Code",
        "src",
        "dev",
        "Dev",
        "projects",
        "Projects",
        "repos",
        "git",
        "GitHub",
        "Documents/GitHub",
        "source/repos",
    ];
    ROOTS.iter().any(|r| {
        let dir = home.join(r);
        dir.join(".git").exists()
            || std::fs::read_dir(&dir)
                .map(|entries| {
                    entries
                        .flatten()
                        .take(200)
                        .any(|e| e.path().join(".git").exists())
                })
                .unwrap_or(false)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn git_folders_one_level_down_count() {
        let home = std::env::temp_dir().join(format!("grokhub-devfolders-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(home.join("code/notes")).expect("dir");
        assert!(
            !has_dev_folders(&home),
            "a folder without .git isn't a project"
        );
        std::fs::create_dir_all(home.join("code/loadout/.git")).expect("git");
        assert!(has_dev_folders(&home));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(home.join("Documents/GitHub/app/.git")).expect("gh");
        assert!(has_dev_folders(&home));
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn this_machine_reads_a_device_record_without_panicking() {
        let home = std::env::temp_dir();
        let p = super::super::read_profile(&home);
        let d = read_device(&home, &p);
        assert_eq!(d.ram_mb, p.ram_mb);
        assert!(d.read_at_ms > 0);
        assert_eq!(d.fingerprint().len(), 16);
    }
}
