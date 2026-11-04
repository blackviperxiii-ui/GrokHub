//! macOS: a sandboxed command runs under `sandbox-exec` with a Seatbelt
//! profile built for it, the mechanism Chromium, Codex and others use there.
//!
//! The profile starts from what a normal process may do and takes three
//! things away, so compilers, package managers and system services keep
//! working:
//!
//! - Network: no outbound connection except to the egress proxy's port on
//!   `localhost`. So `curl --noproxy`, raw sockets, DNS lookups and services
//!   on the host's `localhost` don't get out. Unix sockets are outbound
//!   connections too, so the Docker socket, an SSH agent and the like stay
//!   closed, as on Linux. Unlike Linux, where the command has a loopback of
//!   its own, a test that starts a server and connects to it can't here.
//! - Writes: only beneath the writable roots (the workspace, temp folders,
//!   `/dev` and the tool caches), as Landlock does on Linux.
//! - Reads: the secret folders and files ([`super::secrets`]) can't be read.
//!
//! What stays open: services reached through Mach (the keychain through the
//! `security` tool among them), and a server a command starts can still be
//! reached from the local network. Tools that sandbox themselves with
//! `sandbox-exec` (SwiftPM's manifest step) can't nest a second sandbox and
//! need their own switch (`--disable-sandbox`) or `outside_sandbox`.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

pub const SANDBOX_EXEC: &str = "/usr/bin/sandbox-exec";

/// Whether commands get a Seatbelt profile on this machine.
/// `GROKHUB_SANDBOX_NO_SEATBELT=1` turns it off.
#[cfg(target_os = "macos")]
pub fn available() -> bool {
    std::env::var_os("GROKHUB_SANDBOX_NO_SEATBELT").is_none() && Path::new(SANDBOX_EXEC).is_file()
}

/// A string literal in the profile language.
fn quote(path: &Path) -> String {
    let raw = path.to_string_lossy();
    let mut out = String::with_capacity(raw.len() + 2);
    out.push('"');
    for c in raw.chars() {
        if matches!(c, '"' | '\\') {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('"');
    out
}

/// The profile for one command. `proxy_port` is None when the user turned
/// the network guard off: then only writes and reads are limited. Seatbelt
/// takes `localhost` here, not an address.
pub fn profile(writable: &[PathBuf], hide: &[PathBuf], proxy_port: Option<u16>) -> String {
    let mut p = String::from("(version 1)\n(allow default)\n");
    if let Some(port) = proxy_port {
        p.push_str("(deny network-outbound)\n");
        let _ = writeln!(
            p,
            "(allow network-outbound (remote ip \"localhost:{port}\"))"
        );
    }
    p.push_str("(deny file-write*)\n");
    for root in writable {
        let _ = writeln!(p, "(allow file-write* (subpath {}))", quote(root));
    }
    // Last, so they win over a writable root that holds them.
    for path in hide {
        let filter = if path.is_dir() { "subpath" } else { "literal" };
        let _ = writeln!(
            p,
            "(deny file-read* file-write* ({filter} {}))",
            quote(path)
        );
    }
    p
}

/// The program and arguments that run `argv` under `profile`.
pub fn wrap(profile: &str, argv: &[&str]) -> (String, Vec<String>) {
    let mut args = vec!["-p".to_string(), profile.to_string()];
    args.extend(argv.iter().map(|a| (*a).to_string()));
    (SANDBOX_EXEC.to_string(), args)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_profile_limits_network_writes_and_secret_reads_in_that_order() {
        let base = std::env::temp_dir().join(format!("loadout-sb-{}", std::process::id()));
        let secret_dir = base.join("ssh");
        std::fs::create_dir_all(&secret_dir).unwrap();
        let secret_file = base.join("netrc");
        std::fs::write(&secret_file, "x").unwrap();
        let ws = PathBuf::from("/Users/user/src/my \"app\"");

        let p = profile(
            &[ws, PathBuf::from("/private/tmp")],
            &[secret_dir.clone(), secret_file.clone()],
            Some(8123),
        );
        let lines: Vec<&str> = p.lines().collect();
        assert_eq!(lines[0], "(version 1)");
        assert_eq!(lines[1], "(allow default)");
        assert_eq!(lines[2], "(deny network-outbound)");
        assert_eq!(
            lines[3],
            "(allow network-outbound (remote ip \"localhost:8123\"))"
        );
        assert_eq!(lines[4], "(deny file-write*)");
        assert_eq!(
            lines[5],
            r#"(allow file-write* (subpath "/Users/user/src/my \"app\""))"#
        );
        assert_eq!(lines[6], "(allow file-write* (subpath \"/private/tmp\"))");
        assert!(
            lines[7].starts_with("(deny file-read* file-write* (subpath "),
            "{}",
            lines[7]
        );
        assert!(
            lines[8].starts_with("(deny file-read* file-write* (literal "),
            "{}",
            lines[8]
        );
        // Every quote opens and closes: the profile parses.
        let unescaped = p.replace("\\\"", "");
        assert_eq!(unescaped.matches('"').count() % 2, 0);
        assert_eq!(p.matches('(').count(), p.matches(')').count());

        let open = profile(&[], &[], None);
        assert!(!open.contains("network"), "{open}");
        assert!(open.contains("(deny file-write*)"));

        let (program, args) = wrap(&p, &["bash", "-c", "echo hi"]);
        assert_eq!(program, SANDBOX_EXEC);
        assert_eq!(args[0], "-p");
        assert_eq!(args[1], p);
        assert_eq!(&args[2..], ["bash", "-c", "echo hi"]);
        let _ = std::fs::remove_dir_all(&base);
    }
}
