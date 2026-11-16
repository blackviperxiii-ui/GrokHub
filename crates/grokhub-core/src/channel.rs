//! Install channels. `stable` builds `main`; `beta` builds the `beta` branch.
//! `scripts/install.sh --channel beta|stable` writes the choice to the
//! `channel` receipt in the config folder, and the app's Updates page reads it
//! to pick which releases to offer.

/// The receipt file name inside the config folder (next to `source`).
pub const CHANNEL_RECEIPT: &str = "channel";

/// The old cabin's Beta auto-off baseline, next to the `channel` receipt.
/// A channel switch deletes it so a stale baseline never lingers.
pub const CHANNEL_BETA_SINCE: &str = "channel.beta-since";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Channel {
    #[default]
    Stable,
    Beta,
}

impl Channel {
    pub fn parse(s: &str) -> Option<Channel> {
        match s.trim().to_ascii_lowercase().as_str() {
            "stable" | "main" => Some(Channel::Stable),
            "beta" => Some(Channel::Beta),
            _ => None,
        }
    }

    /// A missing, empty, or unknown receipt reads as stable.
    pub fn from_receipt(text: &str) -> Channel {
        text.lines()
            .map(str::trim)
            .find(|l| !l.is_empty())
            .and_then(Channel::parse)
            .unwrap_or_default()
    }

    pub fn receipt(self) -> String {
        format!("{}\n", self.as_str())
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Channel::Stable => "stable",
            Channel::Beta => "beta",
        }
    }

    /// The branch this channel builds and Update pulls.
    pub fn branch(self) -> &'static str {
        match self {
            Channel::Stable => "main",
            Channel::Beta => "beta",
        }
    }
}

/// What `grokhub --version` reports.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BuildVersion {
    /// `2.10.91-beta.3` on beta, `2.10.91` on stable (legacy bare `-beta` still parses).
    pub version: String,
    pub channel: Channel,
    /// Branch (or release tag) the binary was built from; empty when unknown.
    pub branch: String,
    /// Short commit SHA; empty when unknown.
    pub sha: String,
}

/// `Loadout 2.10.91-beta.3 (beta @ abc1234)` / `Loadout 2.10.91 (main @ abc1234)`,
/// led by the product name from `brand.json`. Without git info it is
/// `Loadout 2.10.91`. `beta_n` is the prerelease number
/// on Beta (`-beta.N`); ignored on Stable.
pub fn version_line(base: &str, channel: Channel, branch: &str, sha: &str, beta_n: u64) -> String {
    let version = match channel {
        Channel::Beta => format!("{base}-beta.{beta_n}"),
        Channel::Stable => base.to_string(),
    };
    let name = crate::brand::product_name();
    match (branch.trim(), sha.trim()) {
        ("", "") => format!("{name} {version}"),
        ("", sha) => format!("{name} {version} ({sha})"),
        (branch, "") => format!("{name} {version} ({branch})"),
        (branch, sha) => format!("{name} {version} ({branch} @ {sha})"),
    }
}

/// A `--version` line without its leading product name. Takes the current
/// name and the old `GrokHub ` one, which installs before the rename print.
pub fn strip_product_prefix(line: &str) -> &str {
    let line = line.trim_start();
    [crate::brand::product_name(), crate::brand::INTERNAL_ID]
        .iter()
        .find_map(|name| line.strip_prefix(name).and_then(|r| r.strip_prefix(' ')))
        .unwrap_or(line)
}

/// Core `X.Y.Z` and channel from a version token (`2.13.0-beta.2`, bare
/// `2.13.0-beta`, or `2.13.0`).
fn version_core_channel(version: &str) -> Option<(&str, Channel)> {
    if let Some((core, n)) = version.rsplit_once("-beta.") {
        if !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()) {
            return Some((core, Channel::Beta));
        }
        return None;
    }
    if let Some(core) = version.strip_suffix("-beta") {
        return Some((core, Channel::Beta));
    }
    Some((version, Channel::Stable))
}

/// Reads a `--version` line back. Also takes the old `GrokHub ` prefix, the
/// old bare `2.10.90` output and legacy `2.10.91-beta` (no `.N`).
pub fn parse_version_line(line: &str) -> Option<BuildVersion> {
    let line = line.trim();
    let rest = strip_product_prefix(line).trim();
    let (version, paren) = match rest.split_once(' ') {
        Some((v, p)) => (v.trim(), p.trim()),
        None => (rest, ""),
    };
    let (core, channel) = version_core_channel(version)?;
    let parts: Vec<&str> = core.split('.').collect();
    if parts.len() != 3
        || parts
            .iter()
            .any(|p| p.is_empty() || !p.bytes().all(|b| b.is_ascii_digit()))
    {
        return None;
    }
    let (branch, sha) = if paren.is_empty() {
        (String::new(), String::new())
    } else {
        let inner = paren.strip_prefix('(')?.strip_suffix(')')?.trim();
        match inner.split_once(" @ ") {
            Some((b, s)) => (b.trim().to_string(), s.trim().to_string()),
            None if inner.bytes().all(|b| b.is_ascii_hexdigit()) => {
                (String::new(), inner.to_string())
            }
            None => (inner.to_string(), String::new()),
        }
    };
    Some(BuildVersion {
        version: version.to_string(),
        channel,
        branch,
        sha,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channel_defaults_to_stable_and_round_trips_through_the_receipt() {
        assert_eq!(Channel::default(), Channel::Stable);
        assert_eq!(Channel::from_receipt(""), Channel::Stable);
        assert_eq!(Channel::from_receipt("nightly\n"), Channel::Stable);
        assert_eq!(Channel::Beta.receipt(), "beta\n");
        assert_eq!(Channel::Stable.receipt(), "stable\n");
        assert_eq!(
            Channel::from_receipt(&Channel::Beta.receipt()),
            Channel::Beta
        );
        assert_eq!(
            Channel::from_receipt(&Channel::Stable.receipt()),
            Channel::Stable
        );
        assert_eq!(Channel::from_receipt("\n  BETA \n"), Channel::Beta);
        assert_eq!(Channel::parse("main"), Some(Channel::Stable));
        assert_eq!(Channel::parse("alpha"), None);
        assert_eq!(Channel::Beta.branch(), "beta");
        assert_eq!(Channel::Stable.branch(), "main");
        assert_eq!(CHANNEL_RECEIPT, "channel");
    }

    #[test]
    fn version_lines_name_the_channel_branch_and_sha() {
        assert_eq!(
            version_line("2.10.91", Channel::Beta, "beta", "abc1234", 3),
            "Loadout 2.10.91-beta.3 (beta @ abc1234)"
        );
        assert_eq!(
            version_line("2.10.91", Channel::Stable, "main", "abc1234", 0),
            "Loadout 2.10.91 (main @ abc1234)"
        );
        assert_eq!(
            version_line("2.10.91", Channel::Stable, "v2.10.91", "bb7890b", 0),
            "Loadout 2.10.91 (v2.10.91 @ bb7890b)"
        );
        assert_eq!(
            version_line("2.10.91", Channel::Stable, "", "", 0),
            "Loadout 2.10.91"
        );
        assert_eq!(
            version_line("2.10.91", Channel::Beta, "", "abc1234", 0),
            "Loadout 2.10.91-beta.0 (abc1234)"
        );
    }

    #[test]
    fn version_lines_parse_back() {
        assert_eq!(
            parse_version_line("Loadout 2.10.91-beta.3 (beta @ abc1234)\n"),
            Some(BuildVersion {
                version: "2.10.91-beta.3".into(),
                channel: Channel::Beta,
                branch: "beta".into(),
                sha: "abc1234".into(),
            })
        );
        assert_eq!(
            parse_version_line("GrokHub 2.10.91-beta (beta @ abc1234)\n"),
            Some(BuildVersion {
                version: "2.10.91-beta".into(),
                channel: Channel::Beta,
                branch: "beta".into(),
                sha: "abc1234".into(),
            })
        );
        assert_eq!(
            parse_version_line("Loadout 2.10.91 (main @ 0f1e2d3)"),
            Some(BuildVersion {
                version: "2.10.91".into(),
                channel: Channel::Stable,
                branch: "main".into(),
                sha: "0f1e2d3".into(),
            })
        );
        assert_eq!(
            parse_version_line("2.10.90"),
            Some(BuildVersion {
                version: "2.10.90".into(),
                channel: Channel::Stable,
                branch: String::new(),
                sha: String::new(),
            })
        );
        assert_eq!(
            parse_version_line("GrokHub 2.13.0-beta.10 (abc1234)").map(|v| (v.channel, v.version)),
            Some((Channel::Beta, "2.13.0-beta.10".to_string()))
        );
        assert_eq!(
            parse_version_line("GrokHub 2.10.91-beta (abc1234)").map(|v| (v.channel, v.sha)),
            Some((Channel::Beta, "abc1234".to_string()))
        );
        assert_eq!(parse_version_line("Loadout two (main @ x)"), None);
        assert_eq!(
            strip_product_prefix("Loadout 3.5.0 (beta @ a)"),
            "3.5.0 (beta @ a)"
        );
        assert_eq!(strip_product_prefix("GrokHub 3.5.0"), "3.5.0");
        assert_eq!(strip_product_prefix("3.5.0"), "3.5.0");
        assert_eq!(parse_version_line("GrokHub 2.10.91 (main @ abc1234"), None);
        assert_eq!(parse_version_line(""), None);
    }

    /// Runs the real `scripts/install.sh --dry-run`: it parses the flags and
    /// reads the receipt, then exits before git, cargo, or any install.
    #[cfg(unix)]
    #[test]
    fn install_sh_parses_channel_flags_and_follows_the_receipt() {
        let script = concat!(env!("CARGO_MANIFEST_DIR"), "/../../scripts/install.sh");
        let cfg = std::env::temp_dir().join(format!("grokhub-chan-sh-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&cfg);
        std::fs::create_dir_all(&cfg).unwrap();
        let run = |args: &[&str]| {
            let out = std::process::Command::new("bash")
                .arg(script)
                .args(args)
                .env("GROKHUB_CONFIG", &cfg)
                .env("PREFIX", "/tmp/gh-chan")
                .output()
                .unwrap();
            (
                out.status.code(),
                String::from_utf8_lossy(&out.stdout).trim().to_string(),
                String::from_utf8_lossy(&out.stderr).trim().to_string(),
            )
        };
        let receipt = cfg.join("channel").display().to_string();
        let plan = |c: &str, b: &str, sw: u8| {
            format!(
                "channel={c} branch={b} switch={sw} prefix=/tmp/gh-chan receipt={receipt} app=shell"
            )
        };
        assert_eq!(run(&["--user", "--dry-run"]).1, plan("stable", "main", 0));
        assert_eq!(
            run(&["--user", "--channel", "beta", "--dry-run"]).1,
            plan("beta", "beta", 1)
        );
        assert_eq!(
            run(&["--channel=stable", "--dry-run"]).1,
            plan("stable", "main", 1)
        );
        std::fs::write(cfg.join("channel"), Channel::Beta.receipt()).unwrap();
        assert_eq!(run(&["--user", "--dry-run"]).1, plan("beta", "beta", 0));
        assert_eq!(
            run(&["--user", "--channel", "stable", "--dry-run"]).1,
            plan("stable", "main", 1)
        );
        assert_eq!(
            run(&["--channel", "nightly", "--dry-run"]),
            (
                Some(2),
                String::new(),
                "error: --channel takes beta or stable, not 'nightly'".into()
            )
        );
        assert_eq!(
            run(&["--channel"]),
            (
                Some(2),
                String::new(),
                "error: --channel takes beta or stable".into()
            )
        );
        assert_eq!(run(&["--bogus"]).0, Some(2));
        let _ = std::fs::remove_dir_all(&cfg);
    }

    /// `--app` picks which desktop app the clone install builds. No `--app`
    /// installs the shell, so a fresh install and an old cabin's own update
    /// (`install.sh --user`) both move to it; the retired cabin is refused.
    #[cfg(unix)]
    #[test]
    fn install_sh_app_flag_defaults_to_the_shell() {
        let script = concat!(env!("CARGO_MANIFEST_DIR"), "/../../scripts/install.sh");
        let cfg = std::env::temp_dir().join(format!("grokhub-app-sh-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&cfg);
        std::fs::create_dir_all(&cfg).unwrap();
        let run = |args: &[&str]| {
            let out = std::process::Command::new("bash")
                .arg(script)
                .args(args)
                .env("GROKHUB_CONFIG", &cfg)
                .env("PREFIX", "/tmp/gh-app")
                .output()
                .unwrap();
            (
                out.status.code(),
                String::from_utf8_lossy(&out.stdout).trim().to_string(),
                String::from_utf8_lossy(&out.stderr).trim().to_string(),
            )
        };
        let receipt = cfg.join("channel").display().to_string();
        let plan = |app: &str| {
            format!(
                "channel=stable branch=main switch=0 prefix=/tmp/gh-app receipt={receipt} app={app}"
            )
        };
        assert_eq!(
            run(&["--user", "--dry-run"]),
            (Some(0), plan("shell"), String::new())
        );
        assert_eq!(run(&["--dry-run"]).1, plan("shell"));
        assert_eq!(
            run(&["--user", "--app", "shell", "--dry-run"]).1,
            plan("shell")
        );
        let retired = (
            Some(2),
            String::new(),
            "error: the cabin was retired; --app takes only shell".to_string(),
        );
        assert_eq!(run(&["--app=both", "--dry-run"]), retired);
        assert_eq!(run(&["--app", "cabin", "--dry-run"]), retired);
        assert_eq!(
            run(&["--app", "tauri", "--dry-run"]),
            (
                Some(2),
                String::new(),
                "error: --app takes shell, not 'tauri'".into()
            )
        );
        assert_eq!(
            run(&["--app"]),
            (Some(2), String::new(), "error: --app takes shell".into())
        );
        let sh = include_str!("../../../scripts/install.sh");
        assert!(sh.contains("\nAPP=shell\n"), "the default app is the shell");
        let _ = std::fs::remove_dir_all(&cfg);
    }

    /// A plain install with a beta receipt on a `main` checkout stops before
    /// cargo, instead of labeling a main build as beta.
    #[cfg(unix)]
    #[test]
    fn install_sh_refuses_a_checkout_on_the_other_channel() {
        let base = std::env::temp_dir().join(format!("grokhub-chan-mix-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let repo = base.join("clone");
        let cfg = base.join("cfg");
        std::fs::create_dir_all(repo.join("scripts")).unwrap();
        std::fs::create_dir_all(&cfg).unwrap();
        let script = concat!(env!("CARGO_MANIFEST_DIR"), "/../../scripts/install.sh");
        std::fs::copy(script, repo.join("scripts/install.sh")).unwrap();
        let git = |args: &[&str]| {
            let ok = std::process::Command::new("git")
                .args(args)
                .current_dir(&repo)
                .output()
                .unwrap()
                .status
                .success();
            assert!(ok, "git {args:?}");
        };
        git(&["init", "-q", "-b", "main"]);
        std::fs::write(cfg.join("channel"), Channel::Beta.receipt()).unwrap();
        let out = std::process::Command::new("bash")
            .arg(repo.join("scripts/install.sh"))
            .arg("--user")
            .env("GROKHUB_CONFIG", &cfg)
            .env("PREFIX", base.join("prefix"))
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(1));
        assert_eq!(
            String::from_utf8_lossy(&out.stderr).trim(),
            format!(
                "error: {} is on main but the beta channel builds beta; run with --channel stable to switch, or git checkout beta",
                repo.display()
            )
        );
        assert!(!base.join("prefix").exists(), "nothing installed");
        let _ = std::fs::remove_dir_all(&base);
    }

    /// A temp `origin` with `main` and `beta`, and a `clone` on `main` carrying
    /// the real `scripts/install.sh` and a committed `Cargo.lock`. A stub
    /// `cargo` on PATH exits 7, so a run that gets past the switch stops there.
    #[cfg(unix)]
    fn switch_fixture(tag: &str) -> (std::path::PathBuf, std::path::PathBuf) {
        use std::os::unix::fs::PermissionsExt;
        let base = std::env::temp_dir().join(format!("grokhub-chan-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let repo = base.join("clone");
        std::fs::create_dir_all(repo.join("scripts")).unwrap();
        std::fs::create_dir_all(repo.join("src")).unwrap();
        std::fs::create_dir_all(base.join("cfg")).unwrap();
        std::fs::create_dir_all(base.join("bin")).unwrap();
        let script = concat!(env!("CARGO_MANIFEST_DIR"), "/../../scripts/install.sh");
        std::fs::copy(script, repo.join("scripts/install.sh")).unwrap();
        std::fs::write(repo.join("Cargo.lock"), "version = 4\n").unwrap();
        std::fs::write(repo.join("src/lib.rs"), "pub fn a() {}\n").unwrap();
        let cargo = base.join("bin/cargo");
        std::fs::write(&cargo, "#!/bin/sh\necho cargo-stub \"$@\"\nexit 7\n").unwrap();
        std::fs::set_permissions(&cargo, std::fs::Permissions::from_mode(0o755)).unwrap();
        // The default install builds the shell, which first checks for npm
        // and webkit2gtk; the cargo stub stops the run before either is used.
        for tool in ["npm", "pkg-config"] {
            let stub = base.join("bin").join(tool);
            std::fs::write(&stub, "#!/bin/sh\nexit 0\n").unwrap();
            std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let git = |dir: &std::path::Path, args: &[&str]| {
            let out = std::process::Command::new("git")
                .args(args)
                .current_dir(dir)
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "git {args:?}: {}",
                String::from_utf8_lossy(&out.stderr)
            );
        };
        git(&base, &["init", "-q", "--bare", "-b", "main", "origin.git"]);
        git(&repo, &["init", "-q", "-b", "main"]);
        git(&repo, &["config", "user.email", "cabin@test"]);
        git(&repo, &["config", "user.name", "Cabin"]);
        git(&repo, &["config", "commit.gpgsign", "false"]);
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-q", "-m", "seed"]);
        git(
            &repo,
            &[
                "remote",
                "add",
                "origin",
                &base.join("origin.git").display().to_string(),
            ],
        );
        git(&repo, &["push", "-q", "origin", "main", "main:beta"]);
        (base, repo)
    }

    #[cfg(unix)]
    fn run_switch_to_beta(base: &std::path::Path, repo: &std::path::Path) -> std::process::Output {
        let path = format!(
            "{}:{}",
            base.join("bin").display(),
            std::env::var("PATH").unwrap_or_default()
        );
        std::process::Command::new("bash")
            .arg(repo.join("scripts/install.sh"))
            .args(["--user", "--channel", "beta"])
            .env("GROKHUB_CONFIG", base.join("cfg"))
            .env("PREFIX", base.join("prefix"))
            .env("PATH", path)
            .output()
            .unwrap()
    }

    #[cfg(unix)]
    fn git_stdout(dir: &std::path::Path, args: &[&str]) -> String {
        let out = std::process::Command::new("git")
            .args(args)
            .current_dir(dir)
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    /// user's case: a cargo run rewrote Cargo.lock in the source folder. The
    /// switch sets that one file aside in a named stash and goes on to beta.
    #[cfg(unix)]
    #[test]
    fn install_sh_stashes_a_lone_cargo_lock_change_and_switches() {
        let (base, repo) = switch_fixture("lock-only");
        std::fs::write(
            repo.join("Cargo.lock"),
            "version = 4\n# rewritten by cargo\n",
        )
        .unwrap();
        let out = run_switch_to_beta(&base, &repo);
        let stdout = String::from_utf8_lossy(&out.stdout).to_string();
        let lines: Vec<&str> = stdout.lines().collect();
        assert_eq!(
            out.status.code(),
            Some(7),
            "stopped at the cargo stub: {stdout}"
        );
        let sha = git_stdout(&repo, &["rev-parse", "--short=7", "HEAD"]);
        assert!(
            lines.contains(&"set aside a tool-made Cargo.lock change (git stash list: grokhub install.sh: Cargo.lock set aside before --channel beta)"),
            "{stdout}"
        );
        assert!(
            lines.contains(&format!("channel beta: {sha} on beta").as_str()),
            "{stdout}"
        );
        assert!(
            lines.contains(&"cargo-stub build --release --locked -p grokhub-hub"),
            "{stdout}"
        );
        assert_eq!(
            git_stdout(&repo, &["symbolic-ref", "--short", "HEAD"]),
            "beta"
        );
        assert_eq!(
            git_stdout(&repo, &["status", "--porcelain", "--untracked-files=no"]),
            ""
        );
        assert_eq!(
            git_stdout(&repo, &["stash", "list", "--format=%gs"]),
            "On main: grokhub install.sh: Cargo.lock set aside before --channel beta"
        );
        // The set-aside change is recoverable as it was.
        assert_eq!(
            git_stdout(&repo, &["show", "stash@{0}:Cargo.lock"]),
            "version = 4\n# rewritten by cargo"
        );
        let _ = std::fs::remove_dir_all(&base);
    }

    /// A real code edit next to Cargo.lock still refuses, lists both files
    /// (into update.log via the host output), and touches neither.
    #[cfg(unix)]
    #[test]
    fn install_sh_refuses_a_real_edit_plus_cargo_lock_and_lists_both() {
        let (base, repo) = switch_fixture("lock-and-rs");
        std::fs::write(
            repo.join("Cargo.lock"),
            "version = 4\n# rewritten by cargo\n",
        )
        .unwrap();
        std::fs::write(repo.join("src/lib.rs"), "pub fn a() { /* wip */ }\n").unwrap();
        let out = run_switch_to_beta(&base, &repo);
        assert_eq!(out.status.code(), Some(1));
        assert_eq!(String::from_utf8_lossy(&out.stdout), "");
        assert_eq!(
            String::from_utf8_lossy(&out.stderr).trim_end(),
            format!(
                "error: {} has uncommitted changes; commit or stash them before --channel beta\nchanged files:\n M Cargo.lock\n M src/lib.rs",
                repo.display()
            )
        );
        assert_eq!(
            git_stdout(&repo, &["symbolic-ref", "--short", "HEAD"]),
            "main"
        );
        assert_eq!(git_stdout(&repo, &["stash", "list"]), "");
        assert_eq!(
            std::fs::read_to_string(repo.join("Cargo.lock")).unwrap(),
            "version = 4\n# rewritten by cargo\n"
        );
        assert_eq!(
            std::fs::read_to_string(repo.join("src/lib.rs")).unwrap(),
            "pub fn a() { /* wip */ }\n"
        );
        let _ = std::fs::remove_dir_all(&base);
    }

    /// Local `beta` with commits that are not on `origin/beta` must refuse
    /// instead of `checkout -B` dropping them.
    #[cfg(unix)]
    #[test]
    fn install_sh_refuses_when_local_channel_branch_is_ahead() {
        let (base, repo) = switch_fixture("local-ahead");
        let git = |args: &[&str]| {
            let out = std::process::Command::new("git")
                .args(args)
                .current_dir(&repo)
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "git {args:?}: {}",
                String::from_utf8_lossy(&out.stderr)
            );
        };
        git(&["fetch", "-q", "origin"]);
        git(&["checkout", "-q", "-B", "beta", "origin/beta"]);
        std::fs::write(repo.join("src/lib.rs"), "pub fn local_only() {}\n").unwrap();
        git(&["add", "."]);
        git(&["commit", "-q", "-m", "local beta only"]);
        let local_beta = git_stdout(&repo, &["rev-parse", "beta"]);
        git(&["checkout", "-q", "main"]);
        let out = run_switch_to_beta(&base, &repo);
        assert_eq!(
            out.status.code(),
            Some(1),
            "stdout={}",
            String::from_utf8_lossy(&out.stdout)
        );
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(
            err.contains("local beta has commits that are not on origin/beta"),
            "{err}"
        );
        assert_eq!(git_stdout(&repo, &["rev-parse", "beta"]), local_beta);
        assert_eq!(
            git_stdout(&repo, &["symbolic-ref", "--short", "HEAD"]),
            "beta"
        );
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn install_sh_never_pgreps_grokhub() {
        let install = include_str!("../../../scripts/install.sh");
        assert!(
            !install.contains("pgrep grokhub"),
            "install.sh must never pgrep grokhub: {install}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn install_sh_skips_sudo_when_packages_present_or_no_tty() {
        let base = std::env::temp_dir().join(format!(
            "grokhub-pkg-skip-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = std::fs::remove_dir_all(&base);
        let bin = base.join("bin");
        let cfg = base.join("cfg");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::create_dir_all(&cfg).unwrap();
        // Shim pacman: -Q succeeds (packages present); -S would write a marker.
        std::fs::write(
            bin.join("pacman"),
            "#!/bin/sh\nif [ \"$1\" = -Q ]; then exit 0; fi\necho PACMAN \"$*\" >>\"$GROKHUB_PKG_LOG\"\nexit 0\n",
        )
        .unwrap();
        std::fs::write(
            bin.join("sudo"),
            "#!/bin/sh\necho SUDO \"$*\" >>\"$GROKHUB_PKG_LOG\"\nexec \"$@\"\n",
        )
        .unwrap();
        // Shim id -u: as root the block installs without asking, but this
        // case is a normal user, also when the tests run as root.
        std::fs::write(
            bin.join("id"),
            "#!/bin/sh\nif [ \"$1\" = -u ]; then echo 1000; else exec /usr/bin/id \"$@\"; fi\n",
        )
        .unwrap();
        {
            use std::os::unix::fs::PermissionsExt;
            for name in ["pacman", "sudo", "id"] {
                let mut perms = std::fs::metadata(bin.join(name)).unwrap().permissions();
                perms.set_mode(0o755);
                std::fs::set_permissions(bin.join(name), perms).unwrap();
            }
        }
        // Extract and run only the package block via a tiny harness: dry-run
        // never reaches try_pkgs, so the test runs that section on its own.
        let harness = base.join("pkg-harness.sh");
        let install_text = include_str!("../../../scripts/install.sh");
        let start = install_text
            .find("PKG_SKIP_FILE=")
            .expect("PKG_SKIP_FILE in install.sh");
        let end = install_text[start..]
            .find("\nif command -v systemctl")
            .map(|i| start + i)
            .expect("end of package block");
        let block = &install_text[start..end];
        std::fs::write(
            &harness,
            format!(
                "#!/usr/bin/env bash\nset -euo pipefail\nGROKHUB_CONFIG=\"{cfg}\"\nexport GROKHUB_CONFIG\n{block}\n",
                cfg = cfg.display()
            ),
        )
        .unwrap();
        let log = base.join("pkg.log");
        let path = format!(
            "{}:{}",
            bin.display(),
            std::env::var("PATH").unwrap_or_default()
        );
        // Packages present → pacman -Q succeeds → no sudo.
        let out = std::process::Command::new("bash")
            .arg(&harness)
            .env("PATH", &path)
            .env("GROKHUB_PKG_LOG", &log)
            .env("HOME", &base)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "stderr={}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(
            !log.exists() || std::fs::read_to_string(&log).unwrap().is_empty(),
            "no sudo when packages present"
        );
        // Missing packages + no TTY → note, no sudo.
        std::fs::write(
            bin.join("pacman"),
            "#!/bin/sh\nif [ \"$1\" = -Q ]; then exit 1; fi\necho PACMAN \"$*\" >>\"$GROKHUB_PKG_LOG\"\nexit 0\n",
        )
        .unwrap();
        let _ = std::fs::remove_file(&log);
        let out = std::process::Command::new("bash")
            .arg(&harness)
            .env("PATH", &path)
            .env("GROKHUB_PKG_LOG", &log)
            .env("HOME", &base)
            .stdin(std::process::Stdio::null())
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&out.stdout);
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(out.status.success(), "stderr={stderr}");
        assert!(
            stdout.contains("pkgs: skipped (no TTY)") || stderr.contains("pkgs: skipped (no TTY)"),
            "stdout={stdout} stderr={stderr}"
        );
        assert!(
            !log.exists() || !std::fs::read_to_string(&log).unwrap().contains("SUDO"),
            "no sudo without TTY"
        );
        let _ = std::fs::remove_dir_all(&base);
    }
}
