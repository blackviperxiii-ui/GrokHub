//! Browser discovery for the default Playwright MCP launch, without changing user config.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

const NO_BROWSER: &str = "Playwright browser not found. Install Chrome, or run \
    `npx -y playwright install chromium`, then reconnect Playwright in Native MCP. \
    For a custom browser, set --executable-path in the Playwright MCP args.";

pub(super) fn launch_args(
    command: &str,
    args: &[String],
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<Vec<String>, String> {
    let lookup = |key: &str| {
        env.get(key)
            .map(OsString::from)
            .or_else(|| std::env::var_os(key))
    };
    configure(command, args, &lookup, || {
        let home = lookup(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
            .map(PathBuf::from)
            .or_else(grokhub_core::user_home)
            .unwrap_or_default();
        candidates(std::env::consts::OS, &lookup, &home, cwd)
            .into_iter()
            .find(|path| executable(path))
    })
}

fn configure(
    command: &str,
    args: &[String],
    env: &impl Fn(&str) -> Option<OsString>,
    detect: impl FnOnce() -> Option<PathBuf>,
) -> Result<Vec<String>, String> {
    let is_playwright = Path::new(command)
        .file_stem()
        .is_some_and(|name| name == "playwright-mcp")
        || args
            .iter()
            .any(|arg| arg == "@playwright/mcp" || arg.starts_with("@playwright/mcp@"));
    let explicit = [
        "browser",
        "executable-path",
        "config",
        "cdp-endpoint",
        "endpoint",
        "extension",
    ]
    .iter()
    .any(|option| {
        let flag = format!("--{option}");
        let key = format!("PLAYWRIGHT_MCP_{}", option.replace('-', "_").to_uppercase());
        args.iter()
            .any(|arg| arg == &flag || arg.starts_with(&format!("{flag}=")))
            || env(&key).is_some_and(|value| !value.is_empty())
    });
    // Package-local browser caches can only be resolved by Playwright itself.
    let explicit = explicit || env("PLAYWRIGHT_BROWSERS_PATH").is_some_and(|value| value == "0");
    let mut out = args.to_vec();
    if is_playwright && !explicit {
        let browser = detect().ok_or_else(|| NO_BROWSER.to_string())?;
        out.push("--executable-path".into());
        out.push(browser.to_string_lossy().into_owned());
    }
    Ok(out)
}

fn candidates(
    os: &str,
    env: &impl Fn(&str) -> Option<OsString>,
    home: &Path,
    cwd: &Path,
) -> Vec<PathBuf> {
    let mut preferred = Vec::new();
    let mut installed = Vec::new();
    let cache = match os {
        "windows" => {
            for key in ["PROGRAMFILES", "PROGRAMFILES(X86)", "LOCALAPPDATA"] {
                if let Some(root) = env(key).map(PathBuf::from) {
                    preferred.push(root.join("Google/Chrome/Application/chrome.exe"));
                    installed.push(root.join("Microsoft/Edge/Application/msedge.exe"));
                    installed.push(root.join("Chromium/Application/chrome.exe"));
                }
            }
            env("LOCALAPPDATA")
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join("AppData/Local"))
                .join("ms-playwright")
        }
        "macos" => {
            preferred.push("/Applications/Google Chrome.app/Contents/MacOS/Google Chrome".into());
            installed.push("/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge".into());
            home.join("Library/Caches/ms-playwright")
        }
        _ => {
            preferred.push("/opt/google/chrome/chrome".into());
            installed.extend(
                [
                    "/usr/bin/google-chrome",
                    "/usr/bin/google-chrome-stable",
                    "/usr/bin/chromium",
                    "/usr/bin/chromium-browser",
                    "/snap/bin/chromium",
                    "/opt/microsoft/msedge/msedge",
                ]
                .map(PathBuf::from),
            );
            env("XDG_CACHE_HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join(".cache"))
                .join("ms-playwright")
        }
    };
    let cache = env("PLAYWRIGHT_BROWSERS_PATH")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .map(|path| {
            env("INIT_CWD")
                .map(PathBuf::from)
                .unwrap_or_else(|| cwd.to_path_buf())
                .join(path)
        })
        .unwrap_or(cache);
    let mut revisions: Vec<_> = cache
        .is_absolute()
        .then(|| std::fs::read_dir(cache))
        .into_iter()
        .filter_map(Result::ok)
        .flatten()
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name();
            let revision = name
                .to_str()?
                .strip_prefix("chromium-")?
                .parse::<u64>()
                .ok()?;
            Some((revision, entry.path()))
        })
        .collect();
    revisions.sort_by_key(|entry| std::cmp::Reverse(entry.0));
    for (_, root) in revisions {
        let paths: &[&str] = match os {
            "windows" => &["chrome-win/chrome.exe", "chrome-win64/chrome.exe"],
            "macos" => &[
                "chrome-mac/Chromium.app/Contents/MacOS/Chromium",
                "chrome-mac-arm64/Chromium.app/Contents/MacOS/Chromium",
                "chrome-mac-x64/Chromium.app/Contents/MacOS/Chromium",
            ],
            _ => &[
                "chrome-linux/chrome",
                "chrome-linux64/chrome",
                "chrome-linux-arm64/chrome",
            ],
        };
        preferred.extend(paths.iter().map(|path| root.join(path)));
    }
    preferred.extend(installed);
    preferred
}

fn executable(path: &Path) -> bool {
    let Ok(metadata) = path.metadata() else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o111 == 0 {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(extra: &[&str]) -> Vec<String> {
        ["-y", "@playwright/mcp@latest"]
            .into_iter()
            .chain(extra.iter().copied())
            .map(String::from)
            .collect()
    }

    #[test]
    fn default_launch_gets_detected_browser_and_missing_browser_is_actionable() {
        assert_eq!(
            configure("npx", &args(&[]), &|_| None, || Some(PathBuf::from(
                "/browser/chrome"
            ))),
            Ok(args(&["--executable-path", "/browser/chrome"]))
        );
        assert_eq!(
            configure("npx", &args(&[]), &|_| None, || None),
            Err(NO_BROWSER.to_string())
        );
        assert!(NO_BROWSER.contains("npx -y playwright install chromium"));
    }

    #[test]
    fn unrelated_servers_and_explicit_browser_choices_are_untouched() {
        for extra in [
            vec!["--browser", "firefox"],
            vec!["--executable-path=/custom/chrome"],
            vec!["--config", "browser.json"],
            vec!["--cdp-endpoint=http://localhost:9222"],
            vec!["--endpoint", "ws://localhost:9222"],
            vec!["--extension"],
        ] {
            let input = args(&extra);
            assert_eq!(
                configure("npx", &input, &|_| None, || panic!("must not detect")),
                Ok(input)
            );
        }
        let input = vec!["other-mcp".to_string()];
        assert_eq!(
            configure("npx", &input, &|_| None, || panic!("must not detect")),
            Ok(input)
        );
        let input = args(&[]);
        assert_eq!(
            configure(
                "npx",
                &input,
                &|key| (key == "PLAYWRIGHT_MCP_BROWSER").then(|| "webkit".into()),
                || panic!("must not detect")
            ),
            Ok(input)
        );
        assert_eq!(
            configure(
                "npx",
                &args(&[]),
                &|key| (key == "PLAYWRIGHT_BROWSERS_PATH").then(|| "0".into()),
                || panic!("package-local cache belongs to Playwright")
            ),
            Ok(args(&[]))
        );
    }

    #[test]
    fn linux_and_windows_fallback_order_skips_missing_browsers() {
        let root = std::env::temp_dir().join(format!("gh-browser-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        for (os, bundled, chrome, installed) in [
            (
                "linux",
                "chrome-linux64/chrome",
                PathBuf::from("/opt/google/chrome/chrome"),
                PathBuf::from("/usr/bin/chromium"),
            ),
            (
                "linux",
                "chrome-linux-arm64/chrome",
                PathBuf::from("/opt/google/chrome/chrome"),
                PathBuf::from("/usr/bin/chromium"),
            ),
            (
                "windows",
                "chrome-win64/chrome.exe",
                root.join("Google/Chrome/Application/chrome.exe"),
                root.join("Microsoft/Edge/Application/msedge.exe"),
            ),
        ] {
            let cache = root.join(os);
            for revision in ["999", "1000"] {
                std::fs::create_dir_all(cache.join(format!("chromium-{revision}"))).unwrap();
            }
            let lookup = |key: &str| match key {
                "PLAYWRIGHT_BROWSERS_PATH" => Some(cache.clone().into_os_string()),
                "PROGRAMFILES" => Some(root.clone().into_os_string()),
                _ => None,
            };
            let paths = candidates(os, &lookup, &root, &root);
            let newest = cache.join("chromium-1000").join(bundled);
            let older = cache.join("chromium-999").join(bundled);
            assert_eq!(paths[0], chrome);
            let select = |available: &[&PathBuf]| {
                paths.iter().find(|path| available.contains(path)).cloned()
            };
            assert_eq!(select(&[&chrome, &newest, &installed]), Some(chrome));
            assert_eq!(select(&[&newest, &installed]), Some(newest.clone()));
            assert_eq!(select(&[&installed]), Some(installed));
            assert_eq!(select(&[]), None);
            for path in [&older, &newest] {
                std::fs::create_dir_all(path.parent().unwrap()).unwrap();
                std::fs::write(path, b"browser fixture").unwrap();
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).unwrap();
                }
            }
            let bundled_paths: Vec<_> = paths
                .iter()
                .filter(|path| path.starts_with(&cache))
                .collect();
            assert_eq!(
                bundled_paths.into_iter().find(|path| executable(path)),
                Some(&newest)
            );
            std::fs::remove_file(&newest).unwrap();
            assert_eq!(
                paths
                    .iter()
                    .filter(|path| path.starts_with(&cache))
                    .find(|path| executable(path)),
                Some(&older)
            );
            std::fs::remove_file(&older).unwrap();
        }
        assert!(!executable(&root));
        assert!(!executable(&root.join("missing")));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn relative_cache_override_uses_init_cwd() {
        let root = std::env::temp_dir().join(format!("gh-browser-relative-{}", std::process::id()));
        let cache = root.join("browsers/chromium-1000");
        std::fs::create_dir_all(&cache).unwrap();
        let lookup = |key: &str| match key {
            "PLAYWRIGHT_BROWSERS_PATH" => Some("browsers".into()),
            "INIT_CWD" => Some(root.clone().into_os_string()),
            _ => None,
        };
        let paths = candidates("linux", &lookup, &root, &root.join("other"));
        assert_eq!(paths[1], cache.join("chrome-linux/chrome"));
        std::fs::remove_dir_all(root).unwrap();
    }
}
