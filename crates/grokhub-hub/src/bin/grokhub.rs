//! `grokhub` was the cabin's binary. The cabin is retired, and its headless
//! modes (`--mcp-self`, `--mcp-desktop`, `--mcp-cua`, `--doctor`, `--oauth`,
//! `--update`, `--hub` and `bench local`) live in `grokhub-hub`. This shim keeps old MCP configs and scripts working for at
//! least one release cycle: it hands those flags to `grokhub-hub` with the
//! same arguments, returns its exit code, and prints one line on stderr naming
//! the new binary. Stdout is the target's own, so MCP stdio is untouched.

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

/// The binary that owns the headless tools now.
const TARGET: &str = "grokhub-hub";

/// The one stderr line on every forwarded run.
const RENAMED_NOTE: &str = "grokhub was renamed to grokhub-hub; this grokhub shim forwards for at least one more release, so point your config at grokhub-hub.";

/// Anything else (the old window, `--agent`, `--tray`): nothing to forward.
const RETIRED_NOTE: &str = "grokhub was renamed: the desktop app is grokhub-shell (grokhub-shell --autostart starts it in the tray; --agent and --tray work there too), and --mcp-*, --doctor, --oauth, --update, --hub and bench local are in grokhub-hub.";

/// Test hook: run this file instead of the `grokhub-hub` next to the shim.
const TARGET_ENV: &str = "GROKHUB_SHIM_TARGET";

/// Only the headless modes move over; the rest of the old flags had a window.
/// `bench local` is read by position, as `grokhub-hub` reads it.
fn forwards(args: &[String]) -> bool {
    let bench = args.first().map(String::as_str) == Some("bench")
        && args.get(1).map(String::as_str) == Some("local");
    bench
        || args.iter().any(|a| {
            matches!(
                a.as_str(),
                "--mcp-self"
                    | "--mcp-desktop"
                    | "--mcp-cua"
                    | "--doctor"
                    | "--oauth"
                    | "--update"
                    | "--hub"
            )
        })
}

/// `grokhub-hub` beside the shim (both install into the same folder), else
/// whatever `grokhub-hub` the PATH finds.
fn target_path(exe_dir: Option<&Path>, over: Option<PathBuf>) -> PathBuf {
    if let Some(path) = over {
        return path;
    }
    let name = format!("{TARGET}{}", std::env::consts::EXE_SUFFIX);
    match exe_dir {
        Some(dir) if dir.join(&name).is_file() => dir.join(name),
        _ => PathBuf::from(name),
    }
}

/// The target's exit code as the shim's; killed by a signal reads as 1.
fn exit_byte(code: Option<i32>) -> u8 {
    code.map_or(1, |c| (c & 0xff) as u8)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if !forwards(&args) {
        eprintln!("{RETIRED_NOTE}");
        return ExitCode::from(2);
    }
    eprintln!("{RENAMED_NOTE}");
    let exe = std::env::current_exe().ok();
    let target = target_path(
        exe.as_deref().and_then(Path::parent),
        std::env::var_os(TARGET_ENV).map(PathBuf::from),
    );
    match Command::new(&target).args(&args).status() {
        Ok(status) => ExitCode::from(exit_byte(status.code())),
        Err(err) => {
            eprintln!("grokhub: could not start {}: {err}", target.display());
            ExitCode::from(127)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn only_the_headless_tools_forward() {
        assert!(forwards(&args(&["--mcp-self"])));
        assert!(forwards(&args(&["--mcp-desktop"])));
        assert!(forwards(&args(&["--mcp-cua"])));
        assert!(forwards(&args(&["--doctor"])));
        assert!(forwards(&args(&["--hub", "--mcp-self"])));
        assert!(forwards(&args(&["--oauth"])));
        assert!(forwards(&args(&["--update"])));
        assert!(forwards(&args(&["--hub"])));
        assert!(forwards(&args(&["bench", "local"])));
        assert!(!forwards(&args(&[])));
        assert!(!forwards(&args(&["bench"])));
        assert!(!forwards(&args(&["local", "bench"])));
        assert!(!forwards(&args(&["--agent"])));
        assert!(!forwards(&args(&["--tray"])));
        assert!(!forwards(&args(&["--doctor-ish"])));
        assert!(!forwards(&args(&["--oauth-ish"])));
        // A mistyped tool name would start the LAN hub, so it never forwards.
        assert!(!forwards(&args(&["--mcp-selfx"])));
        assert!(!forwards(&args(&["--mcp-desk"])));
    }

    #[test]
    fn the_notes_name_the_new_binaries() {
        assert_eq!(
            RENAMED_NOTE,
            "grokhub was renamed to grokhub-hub; this grokhub shim forwards for at least one more release, so point your config at grokhub-hub."
        );
        assert_eq!(
            RETIRED_NOTE,
            "grokhub was renamed: the desktop app is grokhub-shell (grokhub-shell --autostart starts it in the tray; --agent and --tray work there too), and --mcp-*, --doctor, --oauth, --update, --hub and bench local are in grokhub-hub."
        );
        assert!(!RENAMED_NOTE.contains('\n') && !RETIRED_NOTE.contains('\n'));
    }

    #[test]
    fn the_target_is_the_hub_beside_the_shim_or_on_path() {
        let name = format!("grokhub-hub{}", std::env::consts::EXE_SUFFIX);
        let dir = std::env::temp_dir().join(format!("grokhub-shim-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(target_path(Some(&dir), None), PathBuf::from(&name));
        std::fs::write(dir.join(&name), "").unwrap();
        assert_eq!(target_path(Some(&dir), None), dir.join(&name));
        assert_eq!(target_path(None, None), PathBuf::from(&name));
        assert_eq!(
            target_path(Some(&dir), Some(PathBuf::from("other"))),
            PathBuf::from("other")
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn exit_codes_pass_through() {
        assert_eq!(exit_byte(Some(0)), 0);
        assert_eq!(exit_byte(Some(3)), 3);
        assert_eq!(exit_byte(Some(256 + 7)), 7);
        assert_eq!(exit_byte(None), 1);
    }
}
