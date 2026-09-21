use std::path::PathBuf;

pub fn user_home() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    static LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn user_home_prefers_home_then_userprofile() {
        let _g = LOCK.lock().unwrap();
        let old_home = std::env::var_os("HOME");
        let old_up = std::env::var_os("USERPROFILE");
        std::env::remove_var("HOME");
        std::env::set_var("USERPROFILE", r"C:\Users\viper");
        let got = user_home().expect("USERPROFILE");
        assert!(got.ends_with("viper") || got.to_string_lossy().contains("viper"));
        std::env::remove_var("USERPROFILE");
        assert!(user_home().is_none() || old_home.is_some());
        match old_home {
            Some(v) => std::env::set_var("HOME", v),
            None => std::env::remove_var("HOME"),
        }
        match old_up {
            Some(v) => std::env::set_var("USERPROFILE", v),
            None => std::env::remove_var("USERPROFILE"),
        }
    }

    #[test]
    fn windows_dialogue_session_stays_in_history() {
        let home = Some(r"C:\Users\viper\AppData\Local");
        let profile = Some(r"C:\Users\viper");
        let list = cabin_session_cwd("", true, home, profile);
        assert_eq!(list, r"C:\Users\viper\GrokHub-Work");
        assert!(!list.contains("AppData"));
        assert!(!list.contains("Programs"));
        assert!(chat_appears_in_history(
            &list,
            Some(r"C:\Users\viper\GrokHub-Work"),
            true
        ));
        assert!(
            !chat_appears_in_history(&list, Some(r"C:\Users\viper\GrokHub-Work"), false),
            "an empty draft is not a saved session"
        );
        assert!(
            !chat_appears_in_history(
                r"C:\Users\viper\AppData\Local",
                Some(r"C:\Users\viper\GrokHub-Work"),
                true
            ),
            "listing HOME or %LOCALAPPDATA% drops the dialogue session"
        );
        assert!(
            !chat_appears_in_history(
                r"C:\Users\viper\AppData\Local\Programs\GrokHub",
                Some(&list),
                true
            ),
            "the install directory is not the session cwd"
        );
        assert!(chat_appears_in_history(
            r"C:\Users\viper/GrokHub-Work",
            Some(r"\\?\C:\Users\viper\GrokHub-Work\"),
            true
        ));
        let bound = cabin_session_cwd(r"D:\src\app", true, home, profile);
        assert_eq!(bound, r"D:\src\app");
        assert!(chat_appears_in_history(&bound, Some(r"D:/src/app"), true));
        assert!(!chat_appears_in_history(&list, Some(&bound), true));
    }

    #[test]
    fn unix_session_cwd_stays_under_home() {
        let list = cabin_session_cwd("", false, Some("/home/viper"), Some(r"C:\Users\viper"));
        assert_eq!(list, "/home/viper/GrokHub-Work");
        assert!(chat_appears_in_history(
            &list,
            Some("/home/viper/GrokHub-Work/"),
            true
        ));
        assert!(!chat_appears_in_history("/home/viper", Some(&list), true));
    }
}
