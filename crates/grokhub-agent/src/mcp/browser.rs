//! Browser discovery for the default Playwright MCP launch, without changing user config.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

const NO_BROWSER: &str = "Playwright browser not found. Install Chrome, or run \
    `npx -y playwright install chromium`, then reconnect Playwright in Native MCP. \
    For a custom browser, set --executable-path in the Playwright MCP args.";

pub(super) fn launch_args(
    server: &str,
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
    let cwd = absolute_cwd(cwd, &std::env::current_dir().unwrap_or_default());
    let args = if is_playwright(command, args) {
        let config = crate::perm::config_dir();
        let isolated = is_isolated(server);
        // The isolated retry after a failed attach runs the agent's own browser.
        let attach = Some(Attach::load(&config)).filter(|a| a.usable() && !isolated);
        let out = agent_defaults(
            args,
            &lookup,
            &config.join("browser").join("playwright-profile"),
            isolated,
            attach.as_ref(),
        );
        remember_attach(server, attach.filter(|a| out.contains(&a.args()[0])));
        out
    } else {
        args.to_vec()
    };
    configure(command, &args, &lookup, || {
        let home = lookup(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
            .map(PathBuf::from)
            .or_else(grokhub_core::user_home)
            .unwrap_or_default();
        candidates(std::env::consts::OS, &lookup, &home, &cwd)
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
    let is_playwright = is_playwright(command, args);
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
            || env(&key).is_some_and(|value| is_set(&value))
    });
    // `0` keeps browsers in Playwright's package-local cache, which only
    // Playwright can resolve: an installed browser is still preferred, and
    // without one Playwright is left to look in its own cache.
    let package_cache = env("PLAYWRIGHT_BROWSERS_PATH").is_some_and(|value| value == "0");
    let mut out = args.to_vec();
    if is_playwright && !explicit {
        match detect() {
            Some(browser) => {
                out.push("--executable-path".into());
                out.push(browser.to_string_lossy().into_owned());
            }
            None if package_cache => {}
            None => return Err(NO_BROWSER.to_string()),
        }
    }
    Ok(out)
}

pub(super) fn is_playwright(command: &str, args: &[String]) -> bool {
    Path::new(command)
        .file_stem()
        .is_some_and(|name| name == "playwright-mcp")
        || args
            .iter()
            .any(|arg| arg == "@playwright/mcp" || arg.starts_with("@playwright/mcp@"))
}

fn has_flag(args: &[String], env: &impl Fn(&str) -> Option<OsString>, option: &str) -> bool {
    let flag = format!("--{option}");
    let key = format!("PLAYWRIGHT_MCP_{}", option.replace('-', "_").to_uppercase());
    args.iter()
        .any(|arg| arg == &flag || arg.starts_with(&format!("{flag}=")))
        || env(&key).is_some_and(|value| is_set(&value))
}

/// The agent's own browser (LO-51): headless unless the server's env sets
/// `GROKHUB_BROWSER_HEADED`, and a profile the cabin owns, so a shared or
/// locked Playwright profile can't kill the launch. After a launch died, the
/// retry runs `--isolated` (a fresh in-memory profile). A server that attaches
/// to a browser already running, or names its own choice, keeps its flags.
fn agent_defaults(
    args: &[String],
    env: &impl Fn(&str) -> Option<OsString>,
    profile: &Path,
    isolated: bool,
    attach: Option<&Attach>,
) -> Vec<String> {
    let mut out = args.to_vec();
    if ["cdp-endpoint", "endpoint", "extension"]
        .iter()
        .any(|option| has_flag(args, env, option))
    {
        return out;
    }
    // The user's own browser keeps its window and profile.
    if let Some(attach) = attach {
        out.extend(attach.args());
        return out;
    }
    let headed = env("GROKHUB_BROWSER_HEADED").is_some_and(|value| is_set(&value));
    if !headed && !has_flag(args, env, "headless") {
        out.push("--headless".into());
    }
    let own_profile = has_flag(args, env, "user-data-dir") || has_flag(args, env, "isolated");
    if isolated && !has_flag(args, env, "isolated") {
        out.push("--isolated".into());
    } else if !isolated && !own_profile {
        out.push("--user-data-dir".into());
        out.push(profile.to_string_lossy().into_owned());
    }
    out
}

fn isolated_servers() -> &'static std::sync::Mutex<std::collections::BTreeSet<String>> {
    static SET: std::sync::OnceLock<std::sync::Mutex<std::collections::BTreeSet<String>>> =
        std::sync::OnceLock::new();
    SET.get_or_init(Default::default)
}

fn is_isolated(server: &str) -> bool {
    isolated_servers()
        .lock()
        .map(|set| set.contains(server))
        .unwrap_or(false)
}

/// Mark `server` for an isolated relaunch. False when it already had one,
/// so a launch that keeps dying is retried only once.
pub(super) fn isolate(server: &str) -> bool {
    isolated_servers()
        .lock()
        .map(|mut set| set.insert(server.to_string()))
        .unwrap_or(false)
}

/// Every server goes back to its normal launch (the attach changed).
pub(super) fn forget_isolated() {
    if let Ok(mut set) = isolated_servers().lock() {
        set.clear();
    }
}

/// A Playwright error that means the browser process never came up.
pub(super) fn launch_died(text: &str) -> bool {
    let t = text.to_lowercase();
    (t.contains("browser has been closed") && t.contains("target page"))
        || t.contains("browsertype.launch")
        || t.contains("failed to launch")
        || t.contains("missing x server")
        || t.contains("cannot open display")
        // The attached browser isn't listening (closed, or no debugging port).
        || t.contains("connectovercdp")
}

/// The lead the model gets once the isolated retry died too, above the
/// fenced error, so the turn still answers.
pub(super) const CANT_START: &str =
    "The browser can't start on this computer, so nothing was opened. \
    Read the page with web_fetch, or find it with web_search, and answer from that. \
    The browser's own error:";

/// Opt-in attach to a browser the user already runs (with
/// `--remote-debugging-port`), kept in `browser/attach.json` in the config
/// folder. The agent then works in the user's signed-in browser, so it may
/// open only the origins the user allowed: Playwright gets them as
/// `--allowed-origins`, and [`navigate_refusal`] turns away a
/// `browser_navigate` anywhere else before it is sent. With no origin
/// allowed yet, or an endpoint that isn't on this computer, nothing attaches
/// and the agent keeps its own browser.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Attach {
    /// The browser's DevTools endpoint, such as `http://127.0.0.1:9222`.
    pub endpoint: String,
    /// The origins the user allowed, as `https://host[:port]`.
    pub origins: Vec<String>,
}

pub const ATTACH_FILE: &str = "attach.json";

fn attach_path(config_dir: &Path) -> PathBuf {
    config_dir.join("browser").join(ATTACH_FILE)
}

impl Attach {
    /// The saved attach, as written (it may not be usable yet).
    pub fn load(config_dir: &Path) -> Self {
        std::fs::read_to_string(attach_path(config_dir))
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, config_dir: &Path) -> Result<(), String> {
        let path = attach_path(config_dir);
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let text = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(&path, text).map_err(|e| e.to_string())
    }

    /// Ready to attach: a local endpoint and at least one allowed origin.
    pub fn usable(&self) -> bool {
        loopback_endpoint(&self.endpoint) && !self.origins.is_empty()
    }

    /// Adds `site` (a URL or an origin). The origin it stored, or why not.
    pub fn allow(&mut self, site: &str) -> Result<String, String> {
        let origin = origin_of(site).ok_or_else(|| format!("\"{site}\" isn't an http(s) site"))?;
        if !self.origins.contains(&origin) {
            self.origins.push(origin.clone());
        }
        Ok(origin)
    }

    pub fn allows(&self, url: &str) -> bool {
        origin_of(url).is_some_and(|origin| self.origins.contains(&origin))
    }

    /// The Playwright flags for this attach.
    fn args(&self) -> [String; 2] {
        [
            format!("--cdp-endpoint={}", self.endpoint.trim()),
            format!("--allowed-origins={}", self.origins.join(";")),
        ]
    }
}

/// `http(s)://127.0.0.1|localhost|[::1]:port`, or the `ws://` form: the
/// browser on this computer, never one across the network.
pub fn loopback_endpoint(endpoint: &str) -> bool {
    let e = endpoint.trim();
    let Some(rest) = ["http://", "https://", "ws://", "wss://"]
        .iter()
        .find_map(|scheme| e.strip_prefix(scheme))
    else {
        return false;
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    matches!(
        host_port(authority),
        Some(("127.0.0.1" | "localhost" | "[::1]", Some(_)))
    )
}

/// `host[:port]` or `[v6][:port]`, with the port all digits when there is one.
fn host_port(authority: &str) -> Option<(&str, Option<&str>)> {
    let (host, port) = match authority.strip_prefix('[').and_then(|r| r.find(']')) {
        Some(end) => {
            let (host, after) = authority.split_at(end + 2);
            match after.strip_prefix(':') {
                Some(port) => (host, Some(port)),
                None if after.is_empty() => (host, None),
                None => return None,
            }
        }
        None => match authority.split_once(':') {
            Some((host, port)) => (host, Some(port)),
            None => (authority, None),
        },
    };
    let bad_port = port.is_some_and(|p| p.is_empty() || !p.bytes().all(|b| b.is_ascii_digit()));
    (!host.is_empty() && !bad_port).then_some((host, port))
}

/// `https://Example.com:443/a?b` → `https://example.com`. Only http and
/// https; a URL with a user name or password in it has no origin here.
pub fn origin_of(url: &str) -> Option<String> {
    let (scheme, rest) = url.trim().split_once("://")?;
    let scheme = scheme.to_ascii_lowercase();
    let default_port = match scheme.as_str() {
        "http" => "80",
        "https" => "443",
        _ => return None,
    };
    let authority = rest.split(['/', '?', '#']).next()?.to_ascii_lowercase();
    if authority.contains('@') {
        return None;
    }
    let (host, port) = host_port(&authority)?;
    Some(match port.filter(|p| *p != default_port) {
        Some(port) => format!("{scheme}://{host}:{port}"),
        None => format!("{scheme}://{host}"),
    })
}

fn attached_servers() -> &'static std::sync::Mutex<BTreeMap<String, Attach>> {
    static MAP: std::sync::OnceLock<std::sync::Mutex<BTreeMap<String, Attach>>> =
        std::sync::OnceLock::new();
    MAP.get_or_init(Default::default)
}

/// Why `tool` on `server` must not run: a `browser_navigate` in the attached
/// browser to an origin the user hasn't allowed. `None` lets it through.
pub(super) fn navigate_refusal(
    server: &str,
    tool: &str,
    args: &serde_json::Value,
) -> Option<String> {
    if tool != "browser_navigate" {
        return None;
    }
    let map = attached_servers().lock().ok()?;
    let attach = map.get(server)?;
    let url = args.get("url").and_then(|v| v.as_str()).unwrap_or("");
    if attach.allows(url) {
        return None;
    }
    let site = origin_of(url).unwrap_or_else(|| url.to_string());
    Some(format!(
        "The browser here is the user's own, signed in, so it opens only the sites they allowed. \
        {site} isn't one of them. Ask the user to allow it in Settings → General → Your browser, \
        or read the page with web_fetch instead."
    ))
}

fn remember_attach(server: &str, attach: Option<Attach>) {
    if let Ok(mut map) = attached_servers().lock() {
        match attach {
            Some(attach) => map.insert(server.to_string(), attach),
            None => map.remove(server),
        };
    }
}

/// A relative MCP cwd is relative to the cabin's own working folder.
fn absolute_cwd(cwd: &Path, current: &Path) -> PathBuf {
    if cwd.is_absolute() {
        cwd.to_path_buf()
    } else {
        current.join(cwd)
    }
}

/// An env choice counts unless it is empty or turns the option off.
fn is_set(value: &OsString) -> bool {
    let value = value.to_string_lossy();
    let value = value.trim();
    !value.is_empty() && value != "0" && !value.eq_ignore_ascii_case("false")
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
    // `0`: the browsers live in the package's own folder, not a shared cache.
    let package_cache = env("PLAYWRIGHT_BROWSERS_PATH").is_some_and(|value| value == "0");
    let cache = env("PLAYWRIGHT_BROWSERS_PATH")
        .filter(|value| !value.is_empty() && !package_cache)
        .map(PathBuf::from)
        .map(|path| {
            env("INIT_CWD")
                .map(PathBuf::from)
                .unwrap_or_else(|| cwd.to_path_buf())
                .join(path)
        })
        .unwrap_or(cache);
    let mut revisions: Vec<_> = (cache.is_absolute() && !package_cache)
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
                "chrome-mac-arm64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing",
                "chrome-mac-x64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing",
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
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.is_file() && metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        metadata.is_file()
    }
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
    fn the_agent_browser_runs_headless_in_its_own_profile_and_retries_isolated() {
        let profile = Path::new("/cfg/browser/playwright-profile");
        let none = |_: &str| None;
        assert_eq!(
            agent_defaults(&args(&[]), &none, profile, false, None),
            args(&[
                "--headless",
                "--user-data-dir",
                "/cfg/browser/playwright-profile"
            ])
        );
        assert_eq!(
            agent_defaults(&args(&[]), &none, profile, true, None),
            args(&["--headless", "--isolated"])
        );
        let headed = |key: &str| (key == "GROKHUB_BROWSER_HEADED").then(|| OsString::from("1"));
        assert_eq!(
            agent_defaults(
                &args(&["--user-data-dir=/mine"]),
                &headed,
                profile,
                false,
                None
            ),
            args(&["--user-data-dir=/mine"])
        );
        let attach = args(&["--cdp-endpoint=http://localhost:9222"]);
        assert_eq!(agent_defaults(&attach, &none, profile, true, None), attach);
    }

    fn github() -> Attach {
        Attach {
            endpoint: "http://127.0.0.1:9222".into(),
            origins: vec!["https://github.com".into()],
        }
    }

    #[test]
    fn an_allowed_attach_joins_the_users_browser_with_only_those_origins() {
        let profile = Path::new("/cfg/browser/playwright-profile");
        let none = |_: &str| None;
        let mut attach = github();
        attach.allow("https://docs.rs/serde").unwrap();
        assert_eq!(
            agent_defaults(&args(&[]), &none, profile, false, Some(&attach)),
            args(&[
                "--cdp-endpoint=http://127.0.0.1:9222",
                "--allowed-origins=https://github.com;https://docs.rs",
            ])
        );
        // A server that names its own endpoint keeps it.
        let own = args(&["--cdp-endpoint=http://localhost:9333"]);
        assert_eq!(
            agent_defaults(&own, &none, profile, false, Some(&attach)),
            own
        );
    }

    #[test]
    fn attach_needs_a_local_endpoint_and_an_allowed_origin() {
        assert!(github().usable());
        let no_sites = Attach {
            origins: vec![],
            ..github()
        };
        assert!(!no_sites.usable());
        for endpoint in [
            "http://192.168.1.20:9222",
            "http://example.com:9222",
            "http://127.0.0.1",
            "http://localhost:92a2",
            "file:///tmp/devtools",
            "",
        ] {
            let far = Attach {
                endpoint: endpoint.into(),
                ..github()
            };
            assert!(!far.usable(), "{endpoint}");
        }
        for endpoint in [
            "http://localhost:9222",
            "ws://127.0.0.1:9222/devtools/browser/abc",
            "http://[::1]:9222",
        ] {
            assert!(loopback_endpoint(endpoint), "{endpoint}");
        }
    }

    #[test]
    fn origins_are_scheme_host_and_a_port_only_when_it_isnt_the_default() {
        assert_eq!(
            origin_of("https://GitHub.com:443/user/my-app?tab=1#top").as_deref(),
            Some("https://github.com")
        );
        assert_eq!(
            origin_of("http://localhost:3000/").as_deref(),
            Some("http://localhost:3000")
        );
        assert_eq!(
            origin_of("http://[::1]:8080/x").as_deref(),
            Some("http://[::1]:8080")
        );
        assert_eq!(origin_of("https://user:pw@github.com/"), None);
        assert_eq!(origin_of("javascript:alert(1)"), None);
        assert_eq!(origin_of("file:///etc/passwd"), None);
        assert_eq!(origin_of("https://"), None);
        assert_eq!(origin_of("https://github.com:x/"), None);
        let mut attach = github();
        assert_eq!(
            attach.allow("github.com"),
            Err("\"github.com\" isn't an http(s) site".to_string())
        );
        assert_eq!(
            attach.allow("https://github.com/a").unwrap(),
            "https://github.com"
        );
        assert_eq!(attach.origins, vec!["https://github.com".to_string()]);
    }

    #[test]
    fn the_attached_browser_is_turned_away_from_sites_not_allowed() {
        let go = |url: &str| serde_json::json!({ "url": url });
        // Not attached: nothing is refused.
        assert_eq!(
            navigate_refusal(
                "pw-test-free",
                "browser_navigate",
                &go("https://bank.example")
            ),
            None
        );
        remember_attach("pw-test-attached", Some(github()));
        assert_eq!(
            navigate_refusal(
                "pw-test-attached",
                "browser_navigate",
                &go("https://github.com/user")
            ),
            None
        );
        let why = navigate_refusal(
            "pw-test-attached",
            "browser_navigate",
            &go("https://bank.example/login"),
        )
        .expect("refused");
        assert!(
            why.contains("https://bank.example isn't one of them"),
            "{why}"
        );
        assert!(navigate_refusal(
            "pw-test-attached",
            "browser_navigate",
            &go("javascript:alert(1)")
        )
        .is_some());
        // Other tools run; Playwright's --allowed-origins blocks their requests.
        assert_eq!(
            navigate_refusal(
                "pw-test-attached",
                "browser_click",
                &go("https://bank.example")
            ),
            None
        );
        remember_attach("pw-test-attached", None);
        assert_eq!(
            navigate_refusal(
                "pw-test-attached",
                "browser_navigate",
                &go("https://bank.example")
            ),
            None
        );
    }

    #[test]
    fn the_attach_file_round_trips_in_the_browser_folder() {
        let dir = std::env::temp_dir().join(format!("grokhub-attach-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(Attach::load(&dir), Attach::default());
        github().save(&dir).unwrap();
        assert!(dir.join("browser").join(ATTACH_FILE).is_file());
        assert_eq!(Attach::load(&dir), github());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_dead_launch_is_seen_and_the_retry_happens_once() {
        assert!(launch_died(
            "Error: browserType.launchPersistentContext: Target page, context or browser has been closed"
        ));
        assert!(launch_died(
            "browserType.connectOverCDP: connect ECONNREFUSED 127.0.0.1:9222"
        ));
        assert!(!launch_died("Error: net::ERR_NAME_NOT_RESOLVED"));
        let server = format!("pw-retry-{}", std::process::id());
        assert!(!is_isolated(&server));
        assert!(isolate(&server));
        assert!(is_isolated(&server));
        assert!(!isolate(&server));
        assert!(CANT_START.contains("web_fetch"));
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
    }

    #[test]
    fn off_values_and_a_package_local_cache_still_get_the_fallback() {
        for (key, value) in [
            ("PLAYWRIGHT_MCP_EXTENSION", "false"),
            ("PLAYWRIGHT_MCP_EXTENSION", "FALSE"),
            ("PLAYWRIGHT_MCP_EXTENSION", "0"),
            ("PLAYWRIGHT_MCP_BROWSER", " "),
        ] {
            assert_eq!(
                configure(
                    "npx",
                    &args(&[]),
                    &|k| (k == key).then(|| value.into()),
                    || Some(PathBuf::from("/browser/chrome"))
                ),
                Ok(args(&["--executable-path", "/browser/chrome"])),
                "{key}={value:?}"
            );
        }
        let local = |k: &str| (k == "PLAYWRIGHT_BROWSERS_PATH").then(|| "0".into());
        assert_eq!(
            configure("npx", &args(&[]), &local, || Some(PathBuf::from(
                "/opt/google/chrome/chrome"
            ))),
            Ok(args(&["--executable-path", "/opt/google/chrome/chrome"])),
            "an installed browser is still used"
        );
        assert_eq!(
            configure("npx", &args(&[]), &local, || None),
            Ok(args(&[])),
            "without one, Playwright looks in its own package cache"
        );
        let home = std::env::temp_dir().join(format!("gh-browser-local-{}", std::process::id()));
        std::fs::create_dir_all(home.join(".cache/ms-playwright/chromium-1000")).unwrap();
        let paths = candidates("linux", &local, &home, &home);
        std::fs::remove_dir_all(&home).unwrap();
        assert_eq!(paths[0], PathBuf::from("/opt/google/chrome/chrome"));
        assert!(
            paths
                .iter()
                .all(|p| !p.to_string_lossy().contains("ms-playwright") && !p.starts_with("/0")),
            "{paths:?}"
        );
    }

    #[test]
    fn macos_finds_chrome_for_testing_before_old_chromium() {
        let root = std::env::temp_dir().join(format!("gh-browser-mac-{}", std::process::id()));
        let cache = root.join("Library/Caches/ms-playwright/chromium-1200");
        std::fs::create_dir_all(&cache).unwrap();
        let paths = candidates("macos", &|_| None, &root, &root);
        assert_eq!(
            paths[1],
            cache.join(
                "chrome-mac-arm64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing"
            )
        );
        assert_eq!(
            paths[2],
            cache.join(
                "chrome-mac-x64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing"
            )
        );
        assert_eq!(
            paths[3],
            cache.join("chrome-mac/Chromium.app/Contents/MacOS/Chromium")
        );
        std::fs::remove_dir_all(root).unwrap();
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

    #[test]
    fn relative_cwd_and_relative_cache_resolve_against_the_working_folder() {
        let current = std::env::temp_dir().join(format!("gh-browser-cwd-{}", std::process::id()));
        let cache = current.join("work/browsers/chromium-1000");
        std::fs::create_dir_all(&cache).unwrap();
        let cwd = absolute_cwd(Path::new("work"), &current);
        assert_eq!(cwd, current.join("work"));
        assert_eq!(absolute_cwd(&current, Path::new("/elsewhere")), current);
        let lookup =
            |key: &str| (key == "PLAYWRIGHT_BROWSERS_PATH").then(|| OsString::from("browsers"));
        let paths = candidates("linux", &lookup, &current, &cwd);
        assert_eq!(paths[1], cache.join("chrome-linux/chrome"));
        std::fs::remove_dir_all(current).unwrap();
    }
}
