//! Where each OS keeps browser profiles and app shortcuts (D3: Windows parity,
//! `%APPDATA%` / `%LOCALAPPDATA%` and the Start menu).

use std::path::{Path, PathBuf};

use super::fs::{EntryKind, IndexFs};

/// The OS whose folder layout to use. Tests pass any of them on any host.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostOs {
    Linux,
    Windows,
    MacOs,
}

impl HostOs {
    pub const fn current() -> Self {
        if cfg!(windows) {
            Self::Windows
        } else if cfg!(target_os = "macos") {
            Self::MacOs
        } else {
            Self::Linux
        }
    }
}

/// The folders the indexers look in. [`PlatformDirs::from_env`] fills them
/// from the environment; tests build one by hand.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlatformDirs {
    pub os: HostOs,
    /// `$HOME`, or `%USERPROFILE%` on Windows.
    pub home: Option<PathBuf>,
    /// `%APPDATA%` (Roaming).
    pub appdata: Option<PathBuf>,
    /// `%LOCALAPPDATA%`.
    pub local_appdata: Option<PathBuf>,
    /// `%ProgramData%` (the all-users Start menu).
    pub program_data: Option<PathBuf>,
    /// `$XDG_CONFIG_HOME`, else `~/.config`.
    pub config_home: Option<PathBuf>,
    /// `$XDG_DATA_HOME`, else `~/.local/share`.
    pub data_home: Option<PathBuf>,
    /// Linux system data dirs that hold `applications/` (`/usr/share`, …).
    pub system_data: Vec<PathBuf>,
}

fn env_dir(key: &str) -> Option<PathBuf> {
    std::env::var_os(key).filter(|v| !v.is_empty()).map(PathBuf::from)
}

impl PlatformDirs {
    pub fn from_env() -> Self {
        let os = HostOs::current();
        let home = env_dir(if os == HostOs::Windows { "USERPROFILE" } else { "HOME" });
        let config_home = env_dir("XDG_CONFIG_HOME").or_else(|| home.as_ref().map(|h| h.join(".config")));
        let data_home = env_dir("XDG_DATA_HOME").or_else(|| home.as_ref().map(|h| h.join(".local").join("share")));
        let system_data = if os == HostOs::Linux {
            ["/usr/local/share", "/usr/share", "/var/lib/flatpak/exports/share"].iter().map(PathBuf::from).collect()
        } else {
            Vec::new()
        };
        Self {
            os,
            home,
            appdata: env_dir("APPDATA"),
            local_appdata: env_dir("LOCALAPPDATA"),
            program_data: env_dir("ProgramData"),
            config_home,
            data_home,
            system_data,
        }
    }
}

/// How a browser stores history.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrowserKind {
    /// `places.sqlite`, table `moz_places`.
    Firefox,
    /// `History`, table `urls`.
    Chromium,
}

impl BrowserKind {
    /// The one table this kind may read.
    pub fn table(self) -> &'static str {
        match self {
            Self::Firefox => "moz_places",
            Self::Chromium => "urls",
        }
    }

    fn db_name(self) -> &'static str {
        match self {
            Self::Firefox => "places.sqlite",
            Self::Chromium => "History",
        }
    }
}

/// The profiles folder of one browser (the scope id from Settings), or `None`
/// for an unknown browser or a missing env dir.
pub fn browser_root(browser: &str, dirs: &PlatformDirs) -> Option<(BrowserKind, PathBuf)> {
    let mac_support = || dirs.home.as_ref().map(|h| h.join("Library").join("Application Support"));
    let chromium = |linux: &[&str], windows: &[&str], mac: &[&str]| -> Option<PathBuf> {
        let (base, parts) = match dirs.os {
            HostOs::Linux => (dirs.config_home.clone()?, linux),
            HostOs::Windows => (dirs.local_appdata.clone()?, windows),
            HostOs::MacOs => (mac_support()?, mac),
        };
        Some(parts.iter().fold(base, |p, part| p.join(part)))
    };
    let root = match browser {
        "firefox" => {
            let root = match dirs.os {
                HostOs::Linux => dirs.home.as_ref()?.join(".mozilla").join("firefox"),
                HostOs::Windows => dirs.appdata.as_ref()?.join("Mozilla").join("Firefox").join("Profiles"),
                HostOs::MacOs => mac_support()?.join("Firefox").join("Profiles"),
            };
            return Some((BrowserKind::Firefox, root));
        }
        "chrome" => chromium(&["google-chrome"], &["Google", "Chrome", "User Data"], &["Google", "Chrome"]),
        "chromium" => chromium(&["chromium"], &["Chromium", "User Data"], &["Chromium"]),
        "edge" => chromium(&["microsoft-edge"], &["Microsoft", "Edge", "User Data"], &["Microsoft Edge"]),
        "brave" => chromium(
            &["BraveSoftware", "Brave-Browser"],
            &["BraveSoftware", "Brave-Browser", "User Data"],
            &["BraveSoftware", "Brave-Browser"],
        ),
        _ => None,
    }?;
    Some((BrowserKind::Chromium, root))
}

/// Every profile's history file under `root`: Firefox `<profile>/places.sqlite`,
/// Chromium `Default/History` and `Profile N/History`.
pub fn history_dbs(fs: &dyn IndexFs, kind: BrowserKind, root: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs.list(root) else {
        return Vec::new();
    };
    entries
        .into_iter()
        .filter(|e| e.kind == EntryKind::Dir)
        .filter(|e| kind == BrowserKind::Firefox || e.name == "Default" || e.name.starts_with("Profile "))
        .map(|e| e.path.join(kind.db_name()))
        .filter(|p| fs.stat(p).is_some_and(|e| e.kind == EntryKind::File))
        .collect()
}

/// Folders that hold app shortcuts: `.desktop` files on Linux (user first),
/// Start-menu `Programs` on Windows (per user, then all users), `/Applications` on macOS.
pub fn app_dirs(dirs: &PlatformDirs) -> Vec<PathBuf> {
    match dirs.os {
        HostOs::Linux => {
            let mut out: Vec<PathBuf> = Vec::new();
            if let Some(data) = &dirs.data_home {
                out.push(data.join("applications"));
                out.push(data.join("flatpak").join("exports").join("share").join("applications"));
            }
            out.extend(dirs.system_data.iter().map(|d| d.join("applications")));
            out
        }
        HostOs::Windows => {
            let start = |base: &PathBuf| base.join("Microsoft").join("Windows").join("Start Menu").join("Programs");
            dirs.appdata.iter().chain(dirs.program_data.iter()).map(start).collect()
        }
        HostOs::MacOs => {
            let mut out = vec![PathBuf::from("/Applications")];
            if let Some(home) = &dirs.home {
                out.push(home.join("Applications"));
            }
            out
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn windows_dirs() -> PlatformDirs {
        let user = PathBuf::from("C:").join("Users").join("you");
        PlatformDirs {
            os: HostOs::Windows,
            home: Some(user.clone()),
            appdata: Some(user.join("AppData").join("Roaming")),
            local_appdata: Some(user.join("AppData").join("Local")),
            program_data: Some(PathBuf::from("C:").join("ProgramData")),
            config_home: None,
            data_home: None,
            system_data: Vec::new(),
        }
    }

    fn linux_dirs() -> PlatformDirs {
        let home = PathBuf::from("/home/you");
        PlatformDirs {
            os: HostOs::Linux,
            home: Some(home.clone()),
            appdata: None,
            local_appdata: None,
            program_data: None,
            config_home: Some(home.join(".config")),
            data_home: Some(home.join(".local").join("share")),
            system_data: vec![PathBuf::from("/usr/share")],
        }
    }

    /// D3: Firefox lives under `%APPDATA%`, the Chromium family under `%LOCALAPPDATA%`.
    #[test]
    fn windows_browser_roots_use_appdata_and_localappdata() {
        let d = windows_dirs();
        let roaming = d.appdata.clone().unwrap();
        let local = d.local_appdata.clone().unwrap();
        assert_eq!(
            browser_root("firefox", &d),
            Some((BrowserKind::Firefox, roaming.join("Mozilla").join("Firefox").join("Profiles")))
        );
        assert_eq!(
            browser_root("chrome", &d),
            Some((BrowserKind::Chromium, local.join("Google").join("Chrome").join("User Data")))
        );
        assert_eq!(
            browser_root("edge", &d),
            Some((BrowserKind::Chromium, local.join("Microsoft").join("Edge").join("User Data")))
        );
        assert_eq!(
            browser_root("brave", &d),
            Some((BrowserKind::Chromium, local.join("BraveSoftware").join("Brave-Browser").join("User Data")))
        );
        assert_eq!(browser_root("chromium", &d), Some((BrowserKind::Chromium, local.join("Chromium").join("User Data"))));
        assert_eq!(browser_root("netscape", &d), None);
        let no_env = PlatformDirs { appdata: None, local_appdata: None, ..windows_dirs() };
        assert_eq!(browser_root("firefox", &no_env), None, "no %APPDATA%, no guess at HOME");
        assert_eq!(browser_root("chrome", &no_env), None);
    }

    #[test]
    fn linux_browser_roots_use_home_and_xdg_config() {
        let d = linux_dirs();
        let home = PathBuf::from("/home/you");
        assert_eq!(browser_root("firefox", &d), Some((BrowserKind::Firefox, home.join(".mozilla").join("firefox"))));
        assert_eq!(
            browser_root("chrome", &d),
            Some((BrowserKind::Chromium, home.join(".config").join("google-chrome")))
        );
        assert_eq!(
            browser_root("brave", &d),
            Some((BrowserKind::Chromium, home.join(".config").join("BraveSoftware").join("Brave-Browser")))
        );
        assert_eq!(BrowserKind::Firefox.table(), "moz_places");
        assert_eq!(BrowserKind::Chromium.table(), "urls");
    }

    /// D3: Start-menu shortcuts per user (`%APPDATA%`) and for all users (`%ProgramData%`).
    #[test]
    fn app_dirs_per_os() {
        let w = windows_dirs();
        let start = |base: PathBuf| base.join("Microsoft").join("Windows").join("Start Menu").join("Programs");
        assert_eq!(
            app_dirs(&w),
            vec![start(w.appdata.clone().unwrap()), start(w.program_data.clone().unwrap())]
        );
        let l = linux_dirs();
        let share = PathBuf::from("/home/you").join(".local").join("share");
        assert_eq!(
            app_dirs(&l),
            vec![
                share.join("applications"),
                share.join("flatpak").join("exports").join("share").join("applications"),
                PathBuf::from("/usr/share").join("applications"),
            ]
        );
    }
}
