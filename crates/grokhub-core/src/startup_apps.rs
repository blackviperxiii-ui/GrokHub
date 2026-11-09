//! The apps that start with the PC, for Home's PC Loadout tile. Read only:
//! the Linux autostart folder, and on Windows the `Run` keys and Startup
//! folders as `reg query` and the folder listing show them, minus the ones
//! turned off in Task Manager (`StartupApproved`). Nothing here changes them.

/// Most files read from an autostart folder.
pub const MAX_FILES: usize = 200;

/// The app's name from a Linux autostart `.desktop` file, or `None` when it
/// doesn't start: `Hidden=true`, `X-GNOME-Autostart-enabled=false`, or not
/// an application. `stem` names it when the file has no `Name=`.
pub fn desktop_autostart(stem: &str, text: &str) -> Option<String> {
    let mut in_entry = false;
    let mut name = None;
    let mut is_app = true;
    for line in text.lines().map(str::trim) {
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
            continue;
        }
        if !in_entry {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let (key, value) = (key.trim(), value.trim());
        match key {
            "Hidden" if value.eq_ignore_ascii_case("true") => return None,
            "X-GNOME-Autostart-enabled" if value.eq_ignore_ascii_case("false") => return None,
            "Type" => is_app = value == "Application",
            "Name" if name.is_none() && !value.is_empty() => name = Some(value.to_string()),
            _ => {}
        }
    }
    is_app.then(|| name.unwrap_or_else(|| stem.to_string()))
}

/// Value names and types from one key of `reg query` output, the key lines
/// and `(Default)` left out.
pub fn reg_values(text: &str) -> Vec<(String, String, String)> {
    text.lines()
        .filter(|l| l.starts_with("    "))
        .filter_map(|l| {
            let l = l.trim();
            let (name, rest) = l.split_once("    ")?;
            let rest = rest.trim_start();
            let (kind, data) = rest.split_once("    ").unwrap_or((rest, ""));
            let name = name.trim();
            (kind.starts_with("REG_") && name != "(Default)")
                .then(|| (name.to_string(), kind.to_string(), data.trim().to_string()))
        })
        .collect()
}

/// The entries Task Manager turned off, from a `StartupApproved` key: its
/// REG_BINARY data's first byte is odd (0x03, 0x07) when off.
pub fn turned_off(approved: &str) -> Vec<String> {
    reg_values(approved)
        .into_iter()
        .filter(|(_, kind, data)| {
            kind == "REG_BINARY"
                && data
                    .get(..2)
                    .and_then(|b| u8::from_str_radix(b, 16).ok())
                    .is_some_and(|b| b & 1 == 1)
        })
        .map(|(name, _, _)| name)
        .collect()
}

/// The apps a `Run` key starts, minus the ones its `StartupApproved` key
/// turned off.
pub fn run_key_apps(run: &str, approved: &str) -> Vec<String> {
    let off = turned_off(approved);
    reg_values(run)
        .into_iter()
        .map(|(name, _, _)| name)
        .filter(|n| !off.iter().any(|o| o.eq_ignore_ascii_case(n)))
        .collect()
}

/// The shortcuts a Startup folder holds (by file name), minus `desktop.ini`
/// and the ones `StartupApproved\StartupFolder` turned off.
pub fn startup_folder_apps(files: &[String], approved: &str) -> Vec<String> {
    let off = turned_off(approved);
    files
        .iter()
        .filter(|f| !f.eq_ignore_ascii_case("desktop.ini"))
        .filter(|f| !off.iter().any(|o| o.eq_ignore_ascii_case(f)))
        .map(|f| {
            f.rsplit_once('.')
                .map_or(f.as_str(), |(stem, _)| stem)
                .to_string()
        })
        .collect()
}

/// The tile's value: "None", one or two names, or "5 apps".
pub fn summary(names: &[String]) -> String {
    match names {
        [] => "None".into(),
        [one] => one.clone(),
        [a, b] => format!("{a}, {b}"),
        many => format!("{} apps", many.len()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_linux_autostart_entry_counts_unless_it_is_turned_off() {
        let on = "[Desktop Entry]\nType=Application\nName=Discord\nExec=discord\n";
        assert_eq!(desktop_autostart("discord", on).as_deref(), Some("Discord"));
        let hidden = "[Desktop Entry]\nName=Steam\nHidden=true\n";
        assert_eq!(desktop_autostart("steam", hidden), None);
        let gnome_off = "[Desktop Entry]\nName=Slack\nX-GNOME-Autostart-enabled=false\n";
        assert_eq!(desktop_autostart("slack", gnome_off), None);
        // A Name in another section doesn't count; the file name stands in.
        let bare = "[Desktop Action x]\nName=Other\n[Desktop Entry]\nExec=foo\n";
        assert_eq!(desktop_autostart("foo", bare).as_deref(), Some("foo"));
        assert_eq!(desktop_autostart("l", "[Desktop Entry]\nType=Link\n"), None);
    }

    const RUN: &str = "\r\nHKEY_CURRENT_USER\\Software\\Microsoft\\Windows\\CurrentVersion\\Run\r\n    Discord    REG_SZ    \"C:\\Users\\user\\AppData\\Local\\Discord\\Update.exe\" --processStart Discord.exe\r\n    Steam    REG_SZ    \"C:\\Program Files (x86)\\Steam\\steam.exe\" -silent\r\n    OneDrive    REG_SZ    C:\\x\\OneDrive.exe /background\r\n\r\n";
    const APPROVED: &str = "\r\nHKEY_CURRENT_USER\\Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\StartupApproved\\Run\r\n    Steam    REG_BINARY    030000004A1B2C3D4E5F6071\r\n    Discord    REG_BINARY    020000000000000000000000\r\n\r\n";

    #[test]
    fn a_windows_run_key_counts_what_task_manager_left_on() {
        assert_eq!(reg_values(RUN).len(), 3);
        assert_eq!(turned_off(APPROVED), vec!["Steam".to_string()]);
        assert_eq!(
            run_key_apps(RUN, APPROVED),
            vec!["Discord".to_string(), "OneDrive".to_string()]
        );
        // No StartupApproved key: everything in Run starts.
        assert_eq!(run_key_apps(RUN, "").len(), 3);
    }

    #[test]
    fn a_startup_folder_counts_its_shortcuts() {
        let files = vec![
            "desktop.ini".to_string(),
            "Spotify.lnk".to_string(),
            "Old tool.lnk".to_string(),
        ];
        let approved = "    Old tool.lnk    REG_BINARY    03000000\r\n";
        assert_eq!(
            startup_folder_apps(&files, approved),
            vec!["Spotify".to_string()]
        );
        let names = |n: usize| (0..n).map(|i| format!("App {i}")).collect::<Vec<_>>();
        assert_eq!(summary(&names(0)), "None");
        assert_eq!(summary(&names(2)), "App 0, App 1");
        assert_eq!(summary(&names(4)), "4 apps");
    }
}
