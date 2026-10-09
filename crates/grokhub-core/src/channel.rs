//! Install channels. `stable` builds `main`; `beta` builds the `beta` branch.
//! `scripts/install.sh --channel beta|stable` writes the choice to the
//! `channel` receipt in the config folder, and the in-app Update reads it so a
//! beta install never pulls `main`.

/// The receipt file name inside the config folder (next to `source`).
pub const CHANNEL_RECEIPT: &str = "channel";

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
    /// `2.10.91-beta` on beta, `2.10.91` on stable.
    pub version: String,
    pub channel: Channel,
    /// Branch (or release tag) the binary was built from; empty when unknown.
    pub branch: String,
    /// Short commit SHA; empty when unknown.
    pub sha: String,
}

/// `GrokHub 2.10.91-beta (beta @ abc1234)` / `GrokHub 2.10.91 (main @ abc1234)`.
/// Without git info it is `GrokHub 2.10.91`.
pub fn version_line(base: &str, channel: Channel, branch: &str, sha: &str) -> String {
    let version = match channel {
        Channel::Beta => format!("{base}-beta"),
        Channel::Stable => base.to_string(),
    };
    match (branch.trim(), sha.trim()) {
        ("", "") => format!("GrokHub {version}"),
        ("", sha) => format!("GrokHub {version} ({sha})"),
        (branch, "") => format!("GrokHub {version} ({branch})"),
        (branch, sha) => format!("GrokHub {version} ({branch} @ {sha})"),
    }
}

/// Reads a `--version` line back. Also takes the old bare `2.10.90` output.
pub fn parse_version_line(line: &str) -> Option<BuildVersion> {
    let line = line.trim();
    let rest = line.strip_prefix("GrokHub ").unwrap_or(line).trim();
    let (version, paren) = match rest.split_once(' ') {
        Some((v, p)) => (v.trim(), p.trim()),
        None => (rest, ""),
    };
    let core = version.strip_suffix("-beta").unwrap_or(version);
    let parts: Vec<&str> = core.split('.').collect();
    if parts.len() != 3
        || parts
            .iter()
            .any(|p| p.is_empty() || !p.bytes().all(|b| b.is_ascii_digit()))
    {
        return None;
    }
    let channel = if version.ends_with("-beta") {
        Channel::Beta
    } else {
        Channel::Stable
    };
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
            version_line("2.10.91", Channel::Beta, "beta", "abc1234"),
            "GrokHub 2.10.91-beta (beta @ abc1234)"
        );
        assert_eq!(
            version_line("2.10.91", Channel::Stable, "main", "abc1234"),
            "GrokHub 2.10.91 (main @ abc1234)"
        );
        assert_eq!(
            version_line("2.10.91", Channel::Stable, "v2.10.91", "bb7890b"),
            "GrokHub 2.10.91 (v2.10.91 @ bb7890b)"
        );
        assert_eq!(
            version_line("2.10.91", Channel::Stable, "", ""),
            "GrokHub 2.10.91"
        );
        assert_eq!(
            version_line("2.10.91", Channel::Beta, "", "abc1234"),
            "GrokHub 2.10.91-beta (abc1234)"
        );
    }

    #[test]
    fn version_lines_parse_back() {
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
            parse_version_line("GrokHub 2.10.91 (main @ 0f1e2d3)"),
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
            parse_version_line("GrokHub 2.10.91-beta (abc1234)").map(|v| (v.channel, v.sha)),
            Some((Channel::Beta, "abc1234".to_string()))
        );
        assert_eq!(parse_version_line("GrokHub two (main @ x)"), None);
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
            format!("channel={c} branch={b} switch={sw} prefix=/tmp/gh-chan receipt={receipt}")
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
        let git = |dir: &std::path::Path, args: &[&str]| {
            let out = std::process::Command::new("git").args(args).current_dir(dir).output().unwrap();
            assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
        };
        git(&base, &["init", "-q", "--bare", "-b", "main", "origin.git"]);
        git(&repo, &["init", "-q", "-b", "main"]);
        git(&repo, &["config", "user.email", "cabin@test"]);
        git(&repo, &["config", "user.name", "Cabin"]);
        git(&repo, &["config", "commit.gpgsign", "false"]);
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-q", "-m", "seed"]);
        git(&repo, &["remote", "add", "origin", &base.join("origin.git").display().to_string()]);
        git(&repo, &["push", "-q", "origin", "main", "main:beta"]);
        (base, repo)
    }

    #[cfg(unix)]
    fn run_switch_to_beta(base: &std::path::Path, repo: &std::path::Path) -> std::process::Output {
        let path = format!("{}:{}", base.join("bin").display(), std::env::var("PATH").unwrap_or_default());
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
        let out = std::process::Command::new("git").args(args).current_dir(dir).output().unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    /// Jeremy's case: a cargo run rewrote Cargo.lock in the source folder. The
    /// switch sets that one file aside in a named stash and goes on to beta.
    #[cfg(unix)]
    #[test]
    fn install_sh_stashes_a_lone_cargo_lock_change_and_switches() {
        let (base, repo) = switch_fixture("lock-only");
        std::fs::write(repo.join("Cargo.lock"), "version = 4\n# rewritten by cargo\n").unwrap();
        let out = run_switch_to_beta(&base, &repo);
        let stdout = String::from_utf8_lossy(&out.stdout).to_string();
        let lines: Vec<&str> = stdout.lines().collect();
        assert_eq!(out.status.code(), Some(7), "stopped at the cargo stub: {stdout}");
        let sha = git_stdout(&repo, &["rev-parse", "--short=7", "HEAD"]);
        assert!(
            lines.contains(&"set aside a tool-made Cargo.lock change (git stash list: grokhub install.sh: Cargo.lock set aside before --channel beta)"),
            "{stdout}"
        );
        assert!(lines.contains(&format!("channel beta: {sha} on beta").as_str()), "{stdout}");
        assert!(lines.contains(&"cargo-stub build --release --locked -p grokhub-app -p grokhub-hub"), "{stdout}");
        assert_eq!(git_stdout(&repo, &["symbolic-ref", "--short", "HEAD"]), "beta");
        assert_eq!(git_stdout(&repo, &["status", "--porcelain", "--untracked-files=no"]), "");
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
        std::fs::write(repo.join("Cargo.lock"), "version = 4\n# rewritten by cargo\n").unwrap();
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
        assert_eq!(git_stdout(&repo, &["symbolic-ref", "--short", "HEAD"]), "main");
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
}

/// Shown next to the Labs Beta channel toggle: `beta · beta @ abc1234`.
pub fn channel_status_line(channel: Channel, branch: &str, sha: &str) -> String {
    let ch = channel.as_str();
    match (branch.trim(), sha.trim()) {
        ("", "") => ch.to_string(),
        ("", sha) => format!("{ch} · {sha}"),
        (branch, "") => format!("{ch} · {branch}"),
        (branch, sha) => format!("{ch} · {branch} @ {sha}"),
    }
}


/// Tips of `origin/beta` and `origin/main` after a fetch: commit SHAs plus
/// their tree hashes (`origin/<branch>^{tree}`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChannelTips {
    pub beta_sha: String,
    pub main_sha: String,
    pub beta_tree: String,
    pub main_tree: String,
}

/// True when beta has caught up to main: the trees are identical, or the tips
/// are the same commit. Main moves by squash and the main → beta sync always
/// adds a merge commit, so after a promote the SHAs differ but the trees match.
/// Empty, short (< 7) or non-hex values never count as caught up.
pub fn beta_caught_up_to_main(tips: &ChannelTips) -> bool {
    same_tip_sha(&tips.beta_sha, &tips.main_sha)
        || same_tree(&tips.beta_tree, &tips.main_tree)
}

/// Same commit: exact match, or one is a prefix of the other (short vs full SHA).
fn same_tip_sha(beta_sha: &str, main_sha: &str) -> bool {
    let (b, m) = (normalize_git_sha(beta_sha), normalize_git_sha(main_sha));
    valid_git_hash(&b) && valid_git_hash(&m) && (b == m || b.starts_with(&m) || m.starts_with(&b))
}

/// Same tree: the full tree hashes from `git rev-parse` are equal.
fn same_tree(beta_tree: &str, main_tree: &str) -> bool {
    let (b, m) = (normalize_git_sha(beta_tree), normalize_git_sha(main_tree));
    valid_git_hash(&b) && b == m
}

/// A real hash (git's default short length is 7), hex only.
fn valid_git_hash(s: &str) -> bool {
    s.len() >= 7 && s.bytes().all(|c| c.is_ascii_hexdigit())
}

fn normalize_git_sha(s: &str) -> String {
    s.trim().to_ascii_lowercase()
}

/// Next to the `channel` receipt: main's tree on the first auto-off check after
/// opting in to Beta. Auto-off waits until main's tree moves past it (a real
/// promote), so a beta that equals main right after a main → beta sync stays on.
/// Turning Beta on clears it; switching to stable deletes it.
pub const CHANNEL_BETA_SINCE: &str = "channel.beta-since";

/// What one auto-off check should do. See [`auto_off_step`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AutoOffStep {
    /// Stay on the current channel; nothing to write.
    Stay,
    /// First check since opting in: store this main tree as the baseline, stay on beta.
    RecordBaseline(String),
    /// Main moved past the baseline and beta caught up to it: switch to stable.
    SwitchToStable,
}

/// Auto-off policy for one check. Only Beta ever moves. With no valid
/// `beta_since` baseline, record main's tree and stay. With one, switch to
/// stable only when beta caught up to main (same tree or tip) and main's tree
/// is no longer the baseline. Pure — no I/O.
pub fn auto_off_step(current: Channel, tips: &ChannelTips, beta_since: Option<&str>) -> AutoOffStep {
    if current != Channel::Beta {
        return AutoOffStep::Stay;
    }
    let main_tree = normalize_git_sha(&tips.main_tree);
    if !valid_git_hash(&main_tree) {
        return AutoOffStep::Stay;
    }
    let baseline = beta_since.map(normalize_git_sha).filter(|b| valid_git_hash(b));
    match baseline {
        None => AutoOffStep::RecordBaseline(main_tree),
        Some(b) if b != main_tree && beta_caught_up_to_main(tips) => AutoOffStep::SwitchToStable,
        Some(_) => AutoOffStep::Stay,
    }
}

/// Windows Labs copy until the installer supports channels.
pub const CHANNEL_WINDOWS_NOTE: &str =
    "Channel switching is not available on Windows yet — the installer does not support channels. When channels land, Beta will also turn off automatically once beta matches main.";

/// Labs / docs copy: auto-off when beta catches up to main (Linux now; Windows with channels).
pub const CHANNEL_AUTO_OFF_NOTE: &str =
    "When beta catches up to main (same code), Labs Beta turns off and stays on stable — re-enable anytime.";

/// Clear Labs failure copy for a channel switch.
pub fn channel_switch_fail_hint(stderr_or_status: &str) -> &'static str {
    let s = stderr_or_status.to_ascii_lowercase();
    if let Some(hint) = specific_fail_hint(&s) {
        hint
    } else if s.contains("channel switching is not available on windows")
        || s.contains("windows") && s.contains("channel")
    {
        CHANNEL_WINDOWS_NOTE
    } else {
        "Channel switch failed — previous install kept."
    }
}

/// Clear Settings → Update failure copy from the host output of an Update.
/// Falls back to "Update failed" when nothing specific matches.
pub fn update_fail_hint(output: &str) -> &'static str {
    specific_fail_hint(&output.to_ascii_lowercase()).unwrap_or("Update failed")
}

/// The causes an Update or channel switch can name (input is lowercased):
/// dirty clone, Rust too old, no clone, clone on the other branch, build failed.
fn specific_fail_hint(s: &str) -> Option<&'static str> {
    Some(if s.contains("untracked working tree files would be") {
        "Your GrokHub source folder has new files the update would overwrite. Move or delete them (git status lists them), then try again."
    } else if s.contains("uncommitted changes")
        || s.contains("would be overwritten by")
        || s.contains("you have unstaged changes")
        || s.contains("commit your changes or stash them")
    {
        "Your GrokHub source folder has unsaved code changes. Save or undo them (git commit or git stash), then try again."
    } else if s.contains("requires rustc")
        || s.contains("rustc") && s.contains("is not supported")
        || s.contains("rust-version")
    {
        "GrokHub needs a newer Rust to build. Run rustup update, then try again."
    } else if s.contains("not a grokhub source")
        || s.contains("can't find its source folder")
        || s.contains("no clone")
        || s.contains("set settings → source")
        || s.contains("grokhub_src")
    {
        "GrokHub can't find its source folder (~/GrokHub or ~/.config/GrokHub/source)."
    } else if s.contains("is on main but the beta channel") || s.contains("source clone is on main") {
        "Your GrokHub source folder is on stable, but Labs Beta is on. Turn Labs Beta off, or switch the folder to beta (git checkout beta)."
    } else if s.contains("is on beta but the stable channel") || s.contains("source clone is on beta") {
        "Your GrokHub source folder is on beta, but GrokHub is set to stable. Turn Labs Beta on, or switch the folder to stable (git checkout main)."
    } else if s.contains("build failed")
        || s.contains("cargo")
        || s.contains("could not compile")
        || s.contains("error: could not compile")
    {
        "Build failed — previous install kept."
    } else {
        return None;
    })
}

/// Shell plan that backups binaries, runs `install.sh --user --channel …`, and
/// restores the previous binaries if the install exits non-zero.
pub fn channel_switch_shell(source: &str, target: Channel, home: &str) -> String {
    let src = source.trim().trim_end_matches('/');
    let home = home.trim().trim_end_matches('/');
    let channel = target.as_str();
    let bak = format!("{home}/.local/share/grokhub/channel-bak");
    let bin = format!("{home}/.local/bin");
    // Single host command so a failed install always restores before we return.
    format!(
        "set -euo pipefail; \
bak='{bak}'; bin='{bin}'; \
mkdir -p \"$bak\" \"$bin\"; \
cp -f \"$bin/grokhub\" \"$bak/grokhub\" 2>/dev/null || true; \
cp -f \"$bin/grokhub-hub\" \"$bak/grokhub-hub\" 2>/dev/null || true; \
if ! '{src}/scripts/install.sh' --user --channel {channel}; then \
  cp -f \"$bak/grokhub\" \"$bin/grokhub\" 2>/dev/null || true; \
  cp -f \"$bak/grokhub-hub\" \"$bin/grokhub-hub\" 2>/dev/null || true; \
  echo 'channel switch failed — previous install kept' >&2; \
  exit 1; \
fi"
    )
}

/// Preflight errors before spawning the switch. `windows` is passed in so
/// Linux unit tests can assert the Windows note.
pub fn channel_switch_preflight(
    windows: bool,
    source: Option<&std::path::Path>,
) -> Result<(), String> {
    if windows {
        return Err(CHANNEL_WINDOWS_NOTE.to_string());
    }
    match source {
        Some(p) if is_source_tree(p) => Ok(()),
        Some(_) | None => Err(
            "GrokHub can't find its source folder (~/GrokHub or ~/.config/GrokHub/source).".into(),
        ),
    }
}

fn is_source_tree(dir: &std::path::Path) -> bool {
    dir.join("Cargo.toml").is_file()
        && dir.join("scripts/install.sh").is_file()
        && dir.join("crates/grokhub-app").is_dir()
}

#[cfg(test)]
mod channel_switch_tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn status_line_names_channel_branch_and_sha() {
        assert_eq!(
            channel_status_line(Channel::Beta, "beta", "abc1234"),
            "beta · beta @ abc1234"
        );
        assert_eq!(
            channel_status_line(Channel::Stable, "main", "0f1e2d3"),
            "stable · main @ 0f1e2d3"
        );
        assert_eq!(channel_status_line(Channel::Stable, "", ""), "stable");
        assert_eq!(
            channel_status_line(Channel::Beta, "", "deadbeef"),
            "beta · deadbeef"
        );
    }

    #[test]
    fn fail_hints_are_literal_and_specific() {
        assert_eq!(
            channel_switch_fail_hint("error: /x has uncommitted changes; commit or stash"),
            "Your GrokHub source folder has unsaved code changes. Save or undo them (git commit or git stash), then try again."
        );
        assert_eq!(
            channel_switch_fail_hint("not a GrokHub source tree — set Settings → source"),
            "GrokHub can't find its source folder (~/GrokHub or ~/.config/GrokHub/source)."
        );
        assert_eq!(
            channel_switch_fail_hint("error: could not compile `grokhub-app`"),
            "Build failed — previous install kept."
        );
        assert_eq!(
            channel_switch_fail_hint("something else blew up"),
            "Channel switch failed — previous install kept."
        );
        assert_eq!(
            channel_switch_fail_hint(CHANNEL_WINDOWS_NOTE),
            CHANNEL_WINDOWS_NOTE
        );
    }

    #[test]
    fn update_and_switch_hints_name_the_real_cause() {
        let dirty = "error: /src has uncommitted changes; commit or stash them before --channel stable";
        let pull_dirty = "error: Your local changes to the following files would be overwritten by merge:\n\tREADME.md";
        let rustc = "error: package `eframe v0.36.2` cannot be built because it requires rustc 1.88 or newer, while the currently active rustc version is 1.80.0";
        let on_main = "error: /src is on main but the beta channel builds beta; run with --channel stable to switch, or git checkout beta";
        let on_beta = "source clone is on beta — checkout main, then Update";
        let untracked = "error: The following untracked working tree files would be overwritten by merge:\n\tdocs/notes.md\nPlease move or remove them before you merge.";
        for (out, hint) in [
            (untracked, "Your GrokHub source folder has new files the update would overwrite. Move or delete them (git status lists them), then try again."),
            (dirty, "Your GrokHub source folder has unsaved code changes. Save or undo them (git commit or git stash), then try again."),
            (pull_dirty, "Your GrokHub source folder has unsaved code changes. Save or undo them (git commit or git stash), then try again."),
            (rustc, "GrokHub needs a newer Rust to build. Run rustup update, then try again."),
            ("GrokHub can't find its source folder (~/GrokHub or ~/.config/GrokHub/source).", "GrokHub can't find its source folder (~/GrokHub or ~/.config/GrokHub/source)."),
            (on_main, "Your GrokHub source folder is on stable, but Labs Beta is on. Turn Labs Beta off, or switch the folder to beta (git checkout beta)."),
            (on_beta, "Your GrokHub source folder is on beta, but GrokHub is set to stable. Turn Labs Beta on, or switch the folder to stable (git checkout main)."),
            ("error: could not compile `grokhub-app`", "Build failed — previous install kept."),
        ] {
            assert_eq!(update_fail_hint(out), hint, "{out}");
            assert_eq!(channel_switch_fail_hint(out), hint, "{out}");
        }
        assert_eq!(update_fail_hint("$ git pull\nexit 1 · 20ms\nfatal: unable to access"), "Update failed");
        assert_eq!(
            channel_switch_fail_hint("fatal: unable to access"),
            "Channel switch failed — previous install kept."
        );
    }

    #[test]
    fn switch_shell_backs_up_runs_install_and_restores_on_fail() {
        let sh = channel_switch_shell("/repo/GrokHub", Channel::Beta, "/home/box");
        assert!(sh.contains("--channel beta"), "{sh}");
        assert!(sh.contains("/repo/GrokHub/scripts/install.sh"), "{sh}");
        assert!(sh.contains("/home/box/.local/share/grokhub/channel-bak"), "{sh}");
        assert!(sh.contains("cp -f \"$bak/grokhub\" \"$bin/grokhub\""), "{sh}");
        assert!(sh.contains("previous install kept"), "{sh}");
        let stable = channel_switch_shell("/repo", Channel::Stable, "/home/u");
        assert!(stable.contains("--channel stable"), "{stable}");
        assert!(!stable.contains("--channel beta"), "{stable}");
    }

    #[test]
    fn preflight_blocks_windows_and_missing_clone() {
        assert_eq!(
            channel_switch_preflight(true, Some(Path::new("/tmp"))),
            Err(CHANNEL_WINDOWS_NOTE.to_string())
        );
        assert_eq!(
            channel_switch_preflight(false, None),
            Err("GrokHub can't find its source folder (~/GrokHub or ~/.config/GrokHub/source).".into())
        );
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        assert!(channel_switch_preflight(false, Some(&root)).is_ok());
    }

    #[test]
    fn windows_note_is_the_labs_copy() {
        assert!(CHANNEL_WINDOWS_NOTE.starts_with(
            "Channel switching is not available on Windows yet — the installer does not support channels."
        ));
        assert!(CHANNEL_WINDOWS_NOTE.contains("turn off automatically once beta matches main"));
    }

    fn tips(beta_sha: &str, main_sha: &str, beta_tree: &str, main_tree: &str) -> ChannelTips {
        ChannelTips {
            beta_sha: beta_sha.into(),
            main_sha: main_sha.into(),
            beta_tree: beta_tree.into(),
            main_tree: main_tree.into(),
        }
    }

    #[test]
    fn beta_caught_up_to_main_same_sha_true_false_and_prefix() {
        let sha = |b: &str, m: &str| beta_caught_up_to_main(&tips(b, m, "", ""));
        assert!(sha("abc1234deadbeef", "abc1234deadbeef"));
        assert!(sha("abc1234", "abc1234deadbeef"));
        assert!(sha("ABC1234DEADBEEF", "abc1234deadbeef"));
        assert!(!sha("abc1234", "def5678"));
        assert!(!sha("", "abc1234"));
        assert!(!sha("abc1234", ""));
        assert!(!sha("  ", "abc"));
        assert!(!sha("ab", "abc1234")); // under 7 chars
        assert!(!sha("abc12xx", "abc1234")); // non-hex
    }

    #[test]
    fn beta_caught_up_to_main_same_tree_different_sha() {
        // Squash promote + merge-commit sync (#507 / #508): SHAs differ, tree 47fd2b27 matches.
        let tree = "47fd2b27aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        assert!(beta_caught_up_to_main(&tips("e79aa50f", "7266ae6a", tree, tree)));
        assert!(beta_caught_up_to_main(&tips(
            "e79aa50f",
            "7266ae6a",
            &tree.to_ascii_uppercase(),
            &format!(" {tree}\n")
        )));
        // Different trees and different SHAs: not caught up.
        assert!(!beta_caught_up_to_main(&tips(
            "e79aa50f",
            "7266ae6a",
            tree,
            "1111111aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
        )));
        // Trees compare in full — a prefix is not the same tree.
        assert!(!beta_caught_up_to_main(&tips("e79aa50f", "7266ae6a", tree, "47fd2b27")));
        // Missing or junk trees never match.
        assert!(!beta_caught_up_to_main(&tips("e79aa50f", "7266ae6a", "", "")));
        assert!(!beta_caught_up_to_main(&tips("e79aa50f", "7266ae6a", "zzzzzzzz", "zzzzzzzz")));
        assert!(!beta_caught_up_to_main(&ChannelTips::default()));
    }

    #[test]
    fn auto_off_waits_for_main_to_move_past_the_opt_in_baseline() {
        let tree = "47fd2b27aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let older = "1111111aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let caught_up = tips("e79aa50f", "7266ae6a", tree, tree);
        // First check after opting in: record main's tree, stay on beta.
        assert_eq!(
            auto_off_step(Channel::Beta, &caught_up, None),
            AutoOffStep::RecordBaseline(tree.into())
        );
        assert_eq!(
            auto_off_step(Channel::Beta, &caught_up, Some("  \n")),
            AutoOffStep::RecordBaseline(tree.into())
        );
        // Same tree as the baseline (a sync, no promote): stays on.
        assert_eq!(
            auto_off_step(Channel::Beta, &caught_up, Some(&format!("{tree}\n"))),
            AutoOffStep::Stay
        );
        // An opt-in from before a promote: main moved past the baseline and
        // beta caught up, so Beta turns off.
        assert_eq!(
            auto_off_step(Channel::Beta, &caught_up, Some(older)),
            AutoOffStep::SwitchToStable
        );
        // Main moved but beta is not caught up: stays on.
        let ahead = tips("e79aa50f", "7266ae6a", older, tree);
        assert_eq!(
            auto_off_step(Channel::Beta, &ahead, Some("2222222aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")),
            AutoOffStep::Stay
        );
        // Stable never moves; a missing main tree records nothing.
        assert_eq!(auto_off_step(Channel::Stable, &caught_up, Some(older)), AutoOffStep::Stay);
        assert_eq!(auto_off_step(Channel::Beta, &ChannelTips::default(), None), AutoOffStep::Stay);
        assert_eq!(
            auto_off_step(Channel::Beta, &tips("abc1234", "abc1234", "", ""), Some(older)),
            AutoOffStep::Stay
        );
        assert_eq!(CHANNEL_BETA_SINCE, "channel.beta-since");
    }

    #[test]
    fn beta_turned_on_while_beta_equals_main_sticks_across_checks() {
        let tree = "47fd2b27aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let synced = tips("e79aa50f", "7266ae6a", tree, tree);
        let AutoOffStep::RecordBaseline(baseline) = auto_off_step(Channel::Beta, &synced, None) else {
            panic!("first check records the baseline");
        };
        assert_eq!(baseline, tree);
        // Every later check and every main -> beta sync with the same tree stays on.
        for _ in 0..3 {
            assert_eq!(auto_off_step(Channel::Beta, &synced, Some(&baseline)), AutoOffStep::Stay);
        }
        let same_sha = tips("7266ae6a", "7266ae6a", tree, tree);
        assert_eq!(auto_off_step(Channel::Beta, &same_sha, Some(&baseline)), AutoOffStep::Stay);
        // The next promote changes main's tree; once beta catches up, Beta turns off.
        let promoted = "3333333aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        assert_eq!(
            auto_off_step(Channel::Beta, &tips("aa11bb22", "cc33dd44", promoted, promoted), Some(&baseline)),
            AutoOffStep::SwitchToStable
        );
    }

    #[test]
    fn auto_off_note_documents_linux_and_future_windows() {
        assert!(CHANNEL_AUTO_OFF_NOTE.contains("beta catches up to main"));
        assert!(CHANNEL_WINDOWS_NOTE.contains("turn off automatically once beta matches main"));
    }

}
