use std::path::PathBuf;

pub fn user_home() -> Option<PathBuf> {
    user_home_in(
        cfg!(windows),
        std::env::var_os("HOME"),
        std::env::var_os("USERPROFILE"),
    )
}

/// Resolve the OS user home directory without guessing.
/// On Windows, `USERPROFILE` takes precedence over a divergent `HOME`
/// (matching [`session_home`]); on Unix, `HOME` takes precedence.
pub fn user_home_in(
    windows: bool,
    home: Option<std::ffi::OsString>,
    userprofile: Option<std::ffi::OsString>,
) -> Option<PathBuf> {
    let keep = |v: Option<std::ffi::OsString>| {
        v.filter(|s| !s.to_string_lossy().trim().is_empty())
            .map(PathBuf::from)
    };
    if windows {
        keep(userprofile).or_else(|| keep(home))
    } else {
        keep(home).or_else(|| keep(userprofile))
    }
}

/// `GROKHUB_CONFIG` when it names a folder (blank counts as unset).
pub fn env_config_dir() -> Option<PathBuf> {
    let raw = std::env::var("GROKHUB_CONFIG").ok()?;
    let raw = raw.trim();
    (!raw.is_empty()).then(|| PathBuf::from(raw))
}

/// The app's own data folder when nothing pins another one:
/// `%APPDATA%\GrokHub` on Windows, else `<home>/.config/GrokHub`, else
/// `.grokhub`. Every crate's `config_dir()` falls back to this, so the cabin,
/// the agent, the shell and the hub all land in the same folder.
pub fn default_config_dir() -> PathBuf {
    default_config_dir_in(cfg!(windows), std::env::var_os("APPDATA"), user_home())
}

fn default_config_dir_in(
    windows: bool,
    appdata: Option<std::ffi::OsString>,
    home: Option<PathBuf>,
) -> PathBuf {
    if windows {
        if let Some(app) = appdata.filter(|a| !a.to_string_lossy().trim().is_empty()) {
            return PathBuf::from(app).join(crate::brand::INTERNAL_ID);
        }
    }
    match home {
        Some(home) => home.join(".config").join(crate::brand::INTERNAL_ID),
        None => PathBuf::from(".grokhub"),
    }
}

/// `GROKHUB_CONFIG`, else [`default_config_dir`].
pub fn config_dir() -> PathBuf {
    env_config_dir().unwrap_or_else(default_config_dir)
}

fn nonempty(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// Home grok uses for `~/.grok`. On Windows that is USERPROFILE (a divergent
/// HOME such as `%LOCALAPPDATA%` is ignored). On Unix it is HOME.
pub fn session_home(
    windows: bool,
    home: Option<&str>,
    userprofile: Option<&str>,
) -> Option<String> {
    if windows {
        nonempty(userprofile).or_else(|| nonempty(home))
    } else {
        nonempty(home).or_else(|| nonempty(userprofile))
    }
}

fn looks_like_windows_path(path: &str) -> bool {
    (path.len() >= 2 && path.as_bytes()[1] == b':') || path.starts_with('\\') || path.contains('\\')
}

/// Unbound cabin tree: `{session_home}/GrokHub-Work`, with Windows separators
/// when the home is a drive path.
pub fn cabin_work_root(windows: bool, home: Option<&str>, userprofile: Option<&str>) -> String {
    match session_home(windows, home, userprofile) {
        Some(h) => {
            let h = h.trim_end_matches(['/', '\\']);
            if windows && looks_like_windows_path(h) {
                format!("{}\\GrokHub-Work", h.replace('/', "\\"))
            } else {
                format!("{h}/GrokHub-Work")
            }
        }
        None => "GrokHub-Work".into(),
    }
}

/// One directory string for `--cwd` and `grok sessions list`. Grok stores the
/// session under the canonical cwd and lists only that directory.
pub fn canonical_session_cwd(path: &str, windows: bool) -> String {
    let mut p = path.trim().to_string();
    if let Some(rest) = p.strip_prefix(r"\\?\UNC\") {
        p = format!(r"\\{rest}");
    } else if let Some(rest) = p.strip_prefix(r"\\?\") {
        p = rest.to_string();
    }
    if windows && looks_like_windows_path(&p) {
        p = p.replace('/', "\\");
        while p.len() > 3 && p.ends_with('\\') {
            p.pop();
        }
        if p.len() == 2 && p.as_bytes().get(1) == Some(&b':') {
            p.push('\\');
        }
        return p;
    }
    while p.len() > 1 && p.ends_with('/') {
        p.pop();
    }
    p
}

/// Bound project, otherwise the work root. Same value the cabin passes as
/// `--cwd` and as the cwd of `grok sessions list`.
pub fn cabin_session_cwd(
    project_dir: &str,
    windows: bool,
    home: Option<&str>,
    userprofile: Option<&str>,
) -> String {
    let session = session_home(windows, home, userprofile);
    let work = cabin_work_root(windows, home, userprofile);
    let picked = crate::project::resolve_acp_cwd(project_dir, session.as_deref(), &work);
    canonical_session_cwd(&picked, windows)
}

/// Resolve a Chat/Hangout workspace and ensure the per-user default Loadout
/// workspace folder (`<home>/GrokHub-Work`) exists on disk when missing.
///
/// A custom folder the user picked that was later deleted or moved is NOT
/// silently created here, so the workspace check surfaces a clear prompt.
pub fn ensure_chat_workspace(
    project_dir: &str,
    windows: bool,
    home: Option<&str>,
    userprofile: Option<&str>,
) -> PathBuf {
    let resolved = cabin_session_cwd(project_dir, windows, home, userprofile);
    let path = PathBuf::from(&resolved);
    let default_root = canonical_session_cwd(&cabin_work_root(windows, home, userprofile), windows);
    let is_default = project_dir.trim().is_empty()
        || if windows {
            resolved.eq_ignore_ascii_case(&default_root)
        } else {
            resolved == default_root
        };
    if is_default && !path.as_os_str().is_empty() && !path.is_dir() {
        let _ = std::fs::create_dir_all(&path);
    }
    path
}

/// History is `grok sessions list` scoped to the chat cwd. An empty draft has
/// no dialogue, so it is not a saved session.
pub fn chat_appears_in_history(list_cwd: &str, session_cwd: Option<&str>, dialogue: bool) -> bool {
    if !dialogue {
        return false;
    }
    let Some(stored) = session_cwd.map(str::trim).filter(|s| !s.is_empty()) else {
        return false;
    };
    let windows = looks_like_windows_path(list_cwd) || looks_like_windows_path(stored);
    let list = canonical_session_cwd(list_cwd, windows);
    let saved = canonical_session_cwd(stored, windows);
    if windows {
        list.eq_ignore_ascii_case(&saved)
    } else {
        list == saved
    }
}

/// How often History re-lists `grok sessions`. Windows CLI lags new rows, so
/// the watch interval is tighter. Linux stays quieter.
pub fn history_list_refresh_due(windows: bool, loaded: bool, elapsed_ms: u64) -> bool {
    if !loaded {
        return true;
    }
    let wait_ms = if windows { 3_000 } else { 20_000 };
    elapsed_ms >= wait_ms
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    static LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn default_config_dir_is_appdata_on_windows_and_dot_config_elsewhere() {
        let home = Some(PathBuf::from("/home/user"));
        assert_eq!(
            default_config_dir_in(
                true,
                Some(r"C:\Users\user\AppData\Roaming".into()),
                home.clone()
            ),
            PathBuf::from(r"C:\Users\user\AppData\Roaming").join("GrokHub")
        );
        assert_eq!(
            default_config_dir_in(true, Some("  ".into()), home.clone()),
            PathBuf::from("/home/user").join(".config").join("GrokHub")
        );
        assert_eq!(
            default_config_dir_in(false, Some("/ignored".into()), home),
            PathBuf::from("/home/user").join(".config").join("GrokHub")
        );
        assert_eq!(
            default_config_dir_in(false, None, None),
            PathBuf::from(".grokhub")
        );
    }

    #[test]
    fn user_home_prefers_home_then_userprofile() {
        let _g = LOCK.lock().unwrap();
        let old_home = std::env::var_os("HOME");
        let old_up = std::env::var_os("USERPROFILE");
        unsafe { std::env::remove_var("HOME") };
        unsafe { std::env::set_var("USERPROFILE", r"C:\Users\user") };
        let got = user_home().expect("USERPROFILE");
        assert!(got.ends_with("user") || got.to_string_lossy().contains("user"));
        unsafe { std::env::remove_var("USERPROFILE") };
        assert!(user_home().is_none() || old_home.is_some());
        match old_home {
            Some(v) => unsafe { std::env::set_var("HOME", v) },
            None => unsafe { std::env::remove_var("HOME") },
        }
        match old_up {
            Some(v) => unsafe { std::env::set_var("USERPROFILE", v) },
            None => unsafe { std::env::remove_var("USERPROFILE") },
        }
    }

    #[test]
    fn windows_dialogue_session_stays_in_history() {
        let home = Some(r"C:\Users\user\AppData\Local");
        let profile = Some(r"C:\Users\user");
        let list = cabin_session_cwd("", true, home, profile);
        assert_eq!(list, r"C:\Users\user\GrokHub-Work");
        assert!(!list.contains("AppData"));
        assert!(!list.contains("Programs"));
        assert!(chat_appears_in_history(
            &list,
            Some(r"C:\Users\user\GrokHub-Work"),
            true
        ));
        assert!(
            !chat_appears_in_history(&list, Some(r"C:\Users\user\GrokHub-Work"), false),
            "an empty draft is not a saved session"
        );
        assert!(
            !chat_appears_in_history(
                r"C:\Users\user\AppData\Local",
                Some(r"C:\Users\user\GrokHub-Work"),
                true
            ),
            "listing HOME or %LOCALAPPDATA% drops the dialogue session"
        );
        assert!(
            !chat_appears_in_history(
                r"C:\Users\user\AppData\Local\Programs\GrokHub",
                Some(&list),
                true
            ),
            "the install directory is not the session cwd"
        );
        assert!(chat_appears_in_history(
            r"C:\Users\user/GrokHub-Work",
            Some(r"\\?\C:\Users\user\GrokHub-Work\"),
            true
        ));
        let bound = cabin_session_cwd(r"D:\src\app", true, home, profile);
        assert_eq!(bound, r"D:\src\app");
        assert!(chat_appears_in_history(&bound, Some(r"D:/src/app"), true));
        assert!(!chat_appears_in_history(&list, Some(&bound), true));
        assert!(history_list_refresh_due(true, false, 0));
        assert!(history_list_refresh_due(true, true, 3_000));
        assert!(!history_list_refresh_due(true, true, 2_999));
        assert!(!history_list_refresh_due(false, true, 3_000));
        assert!(history_list_refresh_due(false, true, 20_000));
    }

    #[test]
    fn unix_session_cwd_stays_under_home() {
        let list = cabin_session_cwd("", false, Some("/home/user"), Some(r"C:\Users\user"));
        assert_eq!(list, "/home/user/GrokHub-Work");
        assert!(chat_appears_in_history(
            &list,
            Some("/home/user/GrokHub-Work/"),
            true
        ));
        assert!(!chat_appears_in_history("/home/user", Some(&list), true));
    }

    #[test]
    fn user_home_in_prefers_userprofile_on_windows_and_home_on_unix() {
        let win_home = user_home_in(
            true,
            Some(r"C:\Users\alice\AppData\Local".into()),
            Some(r"C:\Users\alice".into()),
        );
        assert_eq!(win_home, Some(PathBuf::from(r"C:\Users\alice")));

        let win_blank_profile = user_home_in(
            true,
            Some(r"C:\Users\fallback".into()),
            Some("   ".into()),
        );
        assert_eq!(win_blank_profile, Some(PathBuf::from(r"C:\Users\fallback")));

        let unix_home = user_home_in(
            false,
            Some("/home/alice".into()),
            Some(r"C:\Users\alice".into()),
        );
        assert_eq!(unix_home, Some(PathBuf::from("/home/alice")));
    }

    #[test]
    fn ensure_chat_workspace_creates_default_folder_only() {
        let root = std::env::temp_dir().join(format!("gh-ensure-ws-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let home_str = root.to_string_lossy().into_owned();

        // Unbound chat creates `<home>/GrokHub-Work` if missing.
        let created = ensure_chat_workspace("", false, Some(&home_str), None);
        assert_eq!(created, root.join("GrokHub-Work"));
        assert!(created.is_dir());

        // Missing custom folder is NOT silently created.
        let missing = root.join("deleted-project");
        let missing_str = missing.to_string_lossy().into_owned();
        let kept = ensure_chat_workspace(&missing_str, false, Some(&home_str), None);
        assert_eq!(kept, missing);
        assert!(!kept.exists());

        let _ = std::fs::remove_dir_all(&root);
    }
}
