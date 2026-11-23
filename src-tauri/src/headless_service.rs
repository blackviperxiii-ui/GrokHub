//! A login service for `grokhub-shell --automations`, so scheduled
//! automations keep firing after the app is closed: a systemd user unit on
//! Linux, a launchd agent on macOS, and a logon task on Windows. Each runs
//! as the signed-in user, never as root, and restarts the run if it stops.
//!
//! `grokhub-shell --install-automations` sets it up and starts it now;
//! `--remove-automations` stops and removes it. The open app still takes the
//! jobs over while it runs ([`grokhub_core::scheduler_lease`]).

use std::path::{Path, PathBuf};

use crate::AUTOMATIONS_ARG;

/// The systemd unit's file name.
pub const UNIT: &str = "grokhub-automations.service";
/// The launchd agent's label (and file name, with `.plist`).
pub const LABEL: &str = "dev.grokhub.shell.automations";
/// The Windows logon task's name.
pub fn task() -> &'static str {
    grokhub_core::brand::filled("{product} automations")
}

/// The systemd user unit that runs `exe --automations`.
pub fn systemd_unit(exe: &Path) -> String {
    format!(
        "[Unit]\n\
         Description={} scheduled automations (no window)\n\
         After=network-online.target\n\
         \n\
         [Service]\n\
         ExecStart=\"{}\" {AUTOMATIONS_ARG}\n\
         Restart=on-failure\n\
         RestartSec=30\n\
         \n\
         [Install]\n\
         WantedBy=default.target\n",
        grokhub_core::brand::product_name(),
        systemd_quoted(&exe.to_string_lossy())
    )
}

/// Inside systemd's double quotes: `\` and `"` are escaped, and `%` (a
/// specifier) is doubled.
fn systemd_quoted(raw: &str) -> String {
    raw.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('%', "%%")
}

/// The launchd agent that runs `exe --automations` at login and keeps it up.
pub fn launchd_plist(exe: &Path) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
         <plist version=\"1.0\">\n\
         <dict>\n\
         \t<key>Label</key>\n\
         \t<string>{LABEL}</string>\n\
         \t<key>ProgramArguments</key>\n\
         \t<array>\n\
         \t\t<string>{}</string>\n\
         \t\t<string>{AUTOMATIONS_ARG}</string>\n\
         \t</array>\n\
         \t<key>RunAtLoad</key>\n\
         \t<true/>\n\
         \t<key>KeepAlive</key>\n\
         \t<dict>\n\
         \t\t<key>SuccessfulExit</key>\n\
         \t\t<false/>\n\
         \t</dict>\n\
         </dict>\n\
         </plist>\n",
        xml_escaped(&exe.to_string_lossy())
    )
}

fn xml_escaped(raw: &str) -> String {
    raw.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// `schtasks` arguments that create the logon task for `exe`, run as the
/// signed-in user with limited rights, replacing an older one.
pub fn schtasks_create(exe: &Path) -> Vec<String> {
    [
        "/Create",
        "/TN",
        task(),
        "/TR",
        &format!("\"{}\" {AUTOMATIONS_ARG}", exe.to_string_lossy()),
        "/SC",
        "ONLOGON",
        "/RL",
        "LIMITED",
        "/F",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

/// Where the unit or agent file goes under `home` (Windows keeps its task
/// in Task Scheduler, so there is no file).
pub fn service_file(home: &Path) -> Option<PathBuf> {
    service_file_in(home, std::env::var_os("XDG_CONFIG_HOME").as_deref())
}

/// [`service_file`] with `XDG_CONFIG_HOME` passed in.
fn service_file_in(home: &Path, xdg_config: Option<&std::ffi::OsStr>) -> Option<PathBuf> {
    if cfg!(target_os = "macos") {
        Some(
            home.join("Library/LaunchAgents")
                .join(format!("{LABEL}.plist")),
        )
    } else if cfg!(windows) {
        None
    } else {
        let config = xdg_config
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".config"));
        Some(config.join("systemd/user").join(UNIT))
    }
}

/// Whether the login service is set up for this user.
pub fn installed() -> Result<bool, String> {
    let home = grokhub_core::paths::user_home().ok_or("Couldn't find your home folder.")?;
    match service_file(&home) {
        Some(file) => Ok(file.is_file()),
        None => Ok(run("schtasks", &["/Query", "/TN", task()]).is_ok()),
    }
}

/// Sets the service up for this binary and starts it. The text says what
/// was done.
pub fn install() -> Result<String, String> {
    let exe = std::env::current_exe().map_err(|e| format!("Couldn't find this app: {e}"))?;
    let home = grokhub_core::paths::user_home().ok_or("Couldn't find your home folder.")?;
    match service_file(&home) {
        Some(file) => {
            let text = if cfg!(target_os = "macos") {
                launchd_plist(&exe)
            } else {
                systemd_unit(&exe)
            };
            if let Some(dir) = file.parent() {
                std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
            }
            std::fs::write(&file, text).map_err(|e| e.to_string())?;
            let started = if cfg!(target_os = "macos") {
                run("launchctl", &["load", "-w", &file.to_string_lossy()])
            } else {
                run("systemctl", &["--user", "daemon-reload"])
                    .and_then(|()| run("systemctl", &["--user", "enable", "--now", UNIT]))
            };
            // A service that couldn't start isn't left to start at a later login.
            if let Err(why) = started {
                let _ = std::fs::remove_file(&file);
                return Err(why);
            }
            Ok(format!(
                "Automations now run with the app closed ({}).\n",
                file.display()
            ))
        }
        None => {
            let args = schtasks_create(&exe);
            run(
                "schtasks",
                &args.iter().map(String::as_str).collect::<Vec<_>>(),
            )?;
            run("schtasks", &["/Run", "/TN", task()])?;
            Ok(format!(
                "Automations now run with the app closed (task \"{}\").\n",
                task()
            ))
        }
    }
}

/// Stops the service and removes it. Nothing set up is not an error.
pub fn remove() -> Result<String, String> {
    let home = grokhub_core::paths::user_home().ok_or("Couldn't find your home folder.")?;
    match service_file(&home) {
        Some(file) => {
            if cfg!(target_os = "macos") {
                let _ = run("launchctl", &["unload", "-w", &file.to_string_lossy()]);
            } else {
                let _ = run("systemctl", &["--user", "disable", "--now", UNIT]);
            }
            match std::fs::remove_file(&file) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(e.to_string()),
            }
            if !cfg!(target_os = "macos") {
                let _ = run("systemctl", &["--user", "daemon-reload"]);
            }
        }
        None => {
            let _ = run("schtasks", &["/End", "/TN", task()]);
            let _ = run("schtasks", &["/Delete", "/TN", task(), "/F"]);
        }
    }
    Ok("Automations run only while the app is open now.\n".into())
}

fn run(program: &str, args: &[&str]) -> Result<(), String> {
    let mut cmd = std::process::Command::new(program);
    cmd.args(args);
    grokhub_core::proc_util::hide_windows_console(&mut cmd);
    let out = cmd
        .output()
        .map_err(|e| format!("Couldn't run {program}: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        let why = String::from_utf8_lossy(&out.stderr);
        Err(format!("{program} {}: {}", args.join(" "), why.trim()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_systemd_unit_runs_the_app_headless_and_restarts_it() {
        assert_eq!(
            systemd_unit(Path::new("/home/user/Apps/My App 100%/grokhub-shell")),
            format!(
                "[Unit]\n\
                 Description={} scheduled automations (no window)\n\
                 After=network-online.target\n\
                 \n\
                 [Service]\n\
                 ExecStart=\"/home/user/Apps/My App 100%%/grokhub-shell\" --automations\n\
                 Restart=on-failure\n\
                 RestartSec=30\n\
                 \n\
                 [Install]\n\
                 WantedBy=default.target\n",
                grokhub_core::brand::product_name()
            )
        );
        assert_eq!(systemd_quoted(r#"a"b\c"#), r#"a\"b\\c"#);
    }

    #[test]
    fn the_launchd_agent_names_the_app_and_the_flag() {
        let plist = launchd_plist(Path::new(
            "/Applications/A&B.app/Contents/MacOS/grokhub-shell",
        ));
        assert!(plist.contains("\t<string>dev.grokhub.shell.automations</string>\n"));
        assert!(plist.contains(
            "\t\t<string>/Applications/A&amp;B.app/Contents/MacOS/grokhub-shell</string>\n\t\t<string>--automations</string>\n"
        ));
        assert!(plist.contains("\t<key>RunAtLoad</key>\n\t<true/>\n"));
    }

    #[test]
    fn the_windows_task_runs_at_logon_with_limited_rights() {
        assert_eq!(
            schtasks_create(Path::new(r"C:\Program Files\My App\grokhub-shell.exe")),
            [
                "/Create",
                "/TN",
                format!("{} automations", grokhub_core::brand::product_name()).as_str(),
                "/TR",
                r#""C:\Program Files\My App\grokhub-shell.exe" --automations"#,
                "/SC",
                "ONLOGON",
                "/RL",
                "LIMITED",
                "/F",
            ]
        );
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    #[test]
    fn on_linux_the_unit_goes_in_the_user_systemd_folder() {
        let home = Path::new("/home/user");
        assert_eq!(
            service_file_in(home, None),
            Some(PathBuf::from(
                "/home/user/.config/systemd/user/grokhub-automations.service"
            ))
        );
        assert_eq!(
            service_file_in(home, Some(std::ffi::OsStr::new(""))),
            service_file_in(home, None)
        );
        assert_eq!(
            service_file_in(home, Some(std::ffi::OsStr::new("/home/user/cfg"))),
            Some(PathBuf::from(
                "/home/user/cfg/systemd/user/grokhub-automations.service"
            ))
        );
    }
}
