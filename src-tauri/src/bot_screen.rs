//! Settings → Bot Desktop: the desktop tools work on a private screen of
//! their own (`grokhub_desktop::bot_desktop`), so the user's cursor, keys
//! and windows stay theirs. Linux only, with Xvnc or Xvfb installed. The
//! switch is kept in `shell.json` and the screen starts again at launch.

use std::path::Path;

use serde::Serialize;

/// What Settings shows for the Bot Desktop.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BotDesktopView {
    /// This computer can run one (Linux).
    pub supported: bool,
    /// The switch.
    pub on: bool,
    /// The screen is up: `:N`.
    pub display: Option<String>,
    /// Why it isn't running though the switch is on, or what to install.
    pub problem: Option<String>,
}

/// The folder for the screen's cookie and preview socket.
pub fn dir(config_dir: &Path) -> std::path::PathBuf {
    config_dir.join("bot-desktop")
}

#[cfg(target_os = "linux")]
mod imp {
    use super::*;
    use grokhub_desktop::bot_desktop::{self, Server};

    pub fn view(on: bool, problem: Option<String>) -> BotDesktopView {
        let display = bot_desktop::shared().map(|d| d.display_name());
        let problem = problem.or_else(|| {
            Server::installed().is_none().then(|| {
                "Needs Xvnc (tigervnc) or Xvfb: sudo apt-get install tigervnc-standalone-server"
                    .to_string()
            })
        });
        BotDesktopView {
            supported: true,
            on,
            display,
            problem,
        }
    }

    /// Starts or stops the screen. An error says why it couldn't start.
    pub fn apply(config_dir: &Path, on: bool) -> Result<(), String> {
        if on {
            bot_desktop::start_shared(&dir(config_dir)).map(|_| ())
        } else {
            bot_desktop::stop_shared();
            Ok(())
        }
    }

    pub fn stop() {
        bot_desktop::stop_shared();
    }
}

#[cfg(not(target_os = "linux"))]
mod imp {
    use super::*;

    pub fn view(_on: bool, _problem: Option<String>) -> BotDesktopView {
        BotDesktopView {
            supported: false,
            on: false,
            display: None,
            problem: None,
        }
    }

    pub fn apply(_config_dir: &Path, on: bool) -> Result<(), String> {
        if on {
            Err("Bot Desktop runs on Linux only.".into())
        } else {
            Ok(())
        }
    }

    pub fn stop() {}
}

pub use imp::{apply, stop, view};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_screen_files_live_in_their_own_config_folder() {
        assert_eq!(
            dir(Path::new("/home/user/.config/GrokHub")),
            Path::new("/home/user/.config/GrokHub/bot-desktop")
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn the_switch_starts_and_stops_the_screen() {
        if grokhub_desktop::bot_desktop::Server::installed().is_none() {
            eprintln!("skipped: no Xvnc or Xvfb");
            return;
        }
        let config =
            std::env::temp_dir().join(format!("grokhub-bot-screen-{}", std::process::id()));
        assert_eq!(view(false, None).display, None);
        apply(&config, true).expect("starts");
        let on = view(true, None);
        assert!(on.supported && on.on && on.problem.is_none());
        let shown = on.display.expect("a display");
        assert!(shown.starts_with(':'), "{shown}");
        assert!(dir(&config).is_dir());
        apply(&config, false).unwrap();
        assert_eq!(
            view(false, None),
            BotDesktopView {
                supported: true,
                on: false,
                display: None,
                problem: None,
            }
        );
        let _ = std::fs::remove_dir_all(&config);
    }
}
