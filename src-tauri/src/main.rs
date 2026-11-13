// No console window behind the shell in Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // The frame-rate setup can install this binary as the probe (when no
    // bundled probe is found), with CAP_PERFMON on the file. A bare
    // `--fps-probe` only counts flips; every other path drops whatever
    // capabilities the exec gave first, so the shell never runs privileged.
    let mut argv = std::env::args().skip(1);
    if argv.next().as_deref() == Some(grokhub_fps_probe::PROBE_ARG) && argv.next().is_none() {
        std::process::exit(grokhub_fps_probe::run_probe());
    }
    grokhub_fps_probe::drop_privileges();
    // Print and leave before the single-instance plugin, which would hand
    // these to an already running shell.
    if let Some(text) = cli_reply(std::env::args()) {
        print!("{text}");
        return;
    }
    grokhub_shell::run()
}

/// `--version` / `-V` and `--help` / `-h`, answered without starting the app.
fn cli_reply(args: impl IntoIterator<Item = String>) -> Option<String> {
    let version = env!("CARGO_PKG_VERSION");
    for arg in args.into_iter().skip(1) {
        match arg.as_str() {
            "--version" | "-V" => return Some(format!("grokhub-shell {version}\n")),
            "--help" | "-h" => {
                return Some(format!(
                    "grokhub-shell {version}\n\n  \
grokhub-shell              open the app\n  \
grokhub-shell --autostart  start in the tray (also --agent, --tray)\n  \
grokhub-shell --version\n\n\
Headless tools (--mcp-self, bench local, --askpass) are in grokhub-hub.\n"
                ))
            }
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::cli_reply;

    fn args(s: &[&str]) -> Vec<String> {
        s.iter().map(|x| (*x).to_string()).collect()
    }

    #[test]
    fn version_and_help_answer_before_the_app_starts() {
        let version = format!("grokhub-shell {}\n", env!("CARGO_PKG_VERSION"));
        assert_eq!(
            cli_reply(args(&["grokhub-shell", "--version"])),
            Some(version.clone())
        );
        assert_eq!(cli_reply(args(&["grokhub-shell", "-V"])), Some(version));
        let help = cli_reply(args(&["grokhub-shell", "--help"])).expect("help");
        assert!(
            help.contains(
                "  grokhub-shell --autostart  start in the tray (also --agent, --tray)\n"
            ),
            "{help}"
        );
        assert_eq!(cli_reply(args(&["grokhub-shell", "-h"])), Some(help));
        assert_eq!(cli_reply(args(&["grokhub-shell"])), None);
        assert_eq!(cli_reply(args(&["grokhub-shell", "--autostart"])), None);
        let main = include_str!("main.rs");
        let body = main.split("fn main() {").nth(1).expect("main");
        assert!(
            body.find("cli_reply(").expect("cli_reply")
                < body.find("grokhub_shell::run()").expect("run"),
            "--version must answer before the app and its single-instance hand-off start"
        );
    }
}
