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
}
