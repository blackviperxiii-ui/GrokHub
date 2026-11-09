//! Driver updates, checked twice before anything changes.
//!
//! On Linux a driver is a package from the distro's signed repos, so that is
//! the official source. First check: the update list (the `PendingUpdates`
//! probe, read from the package lists already on disk) names a driver
//! package with a newer version. Second check, right before the plan: the
//! package database's installed version and the repo's candidate version,
//! each read again with its own probe, must match what the first check said.
//! Any mismatch and nothing runs.
//!
//! There must be a way back before the update runs: a system snapshot, or the
//! old package (pacman's cache, or apt still offering the old version). With
//! neither, the card explains instead of updating. Never while a game runs.
//! On Windows, drivers come from the GPU maker's own app or Windows Update,
//! which can't be checked twice from here yet, so the card guides instead.

use std::path::{Path, PathBuf};

use super::fix::{elevate, DraftStep, FixPlan};
use super::interpret::Finding;
use super::probes::{Os, PackageManager, ProbeId, ProbeSpec, ProgramName};
use super::restore::SnapshotBackend;

/// pacman keeps old packages here.
pub const PACMAN_CACHE: &str = "/var/cache/pacman/pkg";

/// One driver package the update list says is behind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DriverUpdate {
    pub pkg: String,
    /// dnf's list doesn't say.
    pub from: Option<String>,
    pub to: String,
}

/// A driver update both checks agree on.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct VerifiedDriver {
    pub pkg: String,
    pub from: String,
    pub to: String,
}

/// Why the second check didn't agree with the first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DriverDoubt {
    NotADriver,
    NotInstalled,
    /// The installed version isn't the one the update list named.
    InstalledDiffers {
        listed: String,
        installed: String,
    },
    /// The repo offers another version than the update list named.
    CandidateDiffers {
        listed: String,
        repo: Option<String>,
    },
    /// Already at that version.
    NotNewer,
}

/// The way back if the new driver misbehaves.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Rollback {
    /// The fix takes this snapshot first.
    Snapshot(SnapshotBackend),
    /// The old packages in pacman's cache.
    Cached(Vec<PathBuf>),
    /// apt still offers each old version.
    AptVersions,
}

/// Why a driver update won't run, in plain words for the card.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DriverBlock {
    GameRunning,
    NoWayBack,
    Windows,
    Unsupported,
    Nothing,
}

impl DriverBlock {
    pub fn plain(self) -> &'static str {
        match self {
            Self::GameRunning => "A game is running, so I won't touch drivers now. Close it and I can do it then.",
            Self::NoWayBack => "I couldn't find a way back if the new driver misbehaves (no system snapshot, and the old version isn't kept), so I won't update it. Setting up Timeshift or Snapper gives me one.",
            Self::Windows => "On Windows, update drivers through your GPU maker's own app (NVIDIA app or AMD Software) or Windows Update. I can't check those versions twice from here yet, so I won't install one myself.",
            Self::Unsupported => "I can't check driver versions twice with this package manager yet, so I won't update one myself.",
            Self::Nothing => "Your drivers are up to date.",
        }
    }
}

/// GPU drivers, their Vulkan and GL parts, and firmware.
pub fn is_driver_pkg(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    n.contains("nvidia")
        || n.contains("mesa")
        || n.starts_with("linux-firmware")
        || n.starts_with("firmware-")
        || n.starts_with("vulkan-radeon")
        || n.starts_with("vulkan-intel")
        || n.starts_with("lib32-vulkan-radeon")
        || n.starts_with("lib32-vulkan-intel")
        || n.starts_with("xf86-video-")
}

/// Driver packages in a `PendingUpdates` probe's output.
pub fn driver_updates(pm: PackageManager, stdout: &str) -> Vec<DriverUpdate> {
    let mut out = Vec::new();
    for line in stdout.lines().map(str::trim).filter(|l| !l.is_empty()) {
        let w: Vec<&str> = line.split_whitespace().collect();
        let u = match pm {
            // `nvidia-utils 560.35.03-1 -> 565.57.01-1`
            PackageManager::Pacman if w.len() == 4 && w[2] == "->" => Some(DriverUpdate {
                pkg: w[0].into(),
                from: Some(w[1].into()),
                to: w[3].into(),
            }),
            // `nvidia-driver-550/jammy-updates 550.127.05-0ubuntu1 amd64 [upgradable from: 550.120-0ubuntu1]`
            PackageManager::Apt if line.contains("[upgradable from:") && w.len() >= 6 => {
                Some(DriverUpdate {
                    pkg: w[0].split('/').next().unwrap_or(w[0]).into(),
                    from: Some(w[w.len() - 1].trim_end_matches(']').into()),
                    to: w[1].into(),
                })
            }
            // `akmod-nvidia.x86_64   3:565.57.01-1.fc40   rpmfusion-nonfree-updates`
            PackageManager::Dnf if w.len() == 3 && w[0].contains('.') => Some(DriverUpdate {
                pkg: w[0].rsplit_once('.').map_or(w[0], |(n, _)| n).into(),
                from: None,
                to: w[1].into(),
            }),
            _ => None,
        };
        if let Some(u) = u.filter(|u| is_driver_pkg(&u.pkg)) {
            out.push(u);
        }
    }
    out
}

/// The probe that reads `pkg`'s installed version.
pub fn installed_spec(pm: PackageManager, pkg: &ProgramName) -> Option<ProbeSpec> {
    let id = ProbeId::DriverInstalled;
    let p = pkg.as_str();
    match pm {
        PackageManager::Pacman => Some(ProbeSpec::new(id, "pacman", &["-Q", p])),
        PackageManager::Apt => Some(ProbeSpec::new(id, "dpkg-query", &["-W", p])),
        PackageManager::Dnf => Some(ProbeSpec::new(
            id,
            "rpm",
            &["-q", "--qf", "%{EPOCH}:%{VERSION}-%{RELEASE}", p],
        )),
        PackageManager::Zypper => None,
    }
}

/// The probe that reads the version `pkg`'s repo offers, from the package
/// lists already on disk.
pub fn candidate_spec(pm: PackageManager, pkg: &ProgramName) -> Option<ProbeSpec> {
    let id = ProbeId::DriverCandidate;
    let p = pkg.as_str();
    match pm {
        PackageManager::Pacman => Some(ProbeSpec::new(id, "pacman", &["-Si", p])),
        PackageManager::Apt => Some(ProbeSpec::new(id, "apt-cache", &["policy", p])),
        PackageManager::Dnf => Some(ProbeSpec::new(
            id,
            "dnf",
            &[
                "-q",
                "-C",
                "repoquery",
                "--latest-limit",
                "1",
                "--qf",
                "%{epoch}:%{version}-%{release}",
                p,
            ],
        )),
        PackageManager::Zypper => None,
    }
}

/// The installed version from [`installed_spec`]'s output.
pub fn parse_installed(pm: PackageManager, stdout: &str) -> Option<String> {
    let line = stdout.lines().map(str::trim).find(|l| !l.is_empty())?;
    let v = match pm {
        // `nvidia-utils 560.35.03-1`, `nvidia-driver-550\t550.120-0ubuntu1`
        PackageManager::Pacman | PackageManager::Apt => line.split_whitespace().nth(1)?,
        PackageManager::Dnf if line.contains("not installed") => return None,
        PackageManager::Dnf => line,
        PackageManager::Zypper => return None,
    };
    Some(v.to_string())
}

/// The repo's version from [`candidate_spec`]'s output.
pub fn parse_candidate(pm: PackageManager, stdout: &str) -> Option<String> {
    let field = |key: &str| {
        stdout.lines().find_map(|l| {
            let (k, v) = l.split_once(':')?;
            (k.trim() == key).then(|| v.trim().to_string())
        })
    };
    let v = match pm {
        PackageManager::Pacman => field("Version")?,
        PackageManager::Apt => field("Candidate").filter(|v| v != "(none)")?,
        PackageManager::Dnf => stdout
            .lines()
            .map(str::trim)
            .find(|l| !l.is_empty())?
            .to_string(),
        PackageManager::Zypper => return None,
    };
    Some(v)
}

/// Every version `apt-cache policy` lists in its version table.
pub fn apt_versions(stdout: &str) -> Vec<String> {
    let mut table = false;
    let mut out = Vec::new();
    for line in stdout.lines() {
        if line.trim() == "Version table:" {
            table = true;
            continue;
        }
        // Versions sit 5 spaces in (`*** ` marks the installed one); their
        // sources sit deeper and start with a priority number.
        let t = line.trim_start_matches(' ').trim_start_matches("*** ");
        let indent = line.len() - line.trim_start().len();
        if table && indent <= 5 {
            if let Some(v) = t.split_whitespace().next() {
                out.push(v.to_string());
            }
        }
    }
    out
}

/// A version without its epoch (`3:565.57.01-1` and `565.57.01-1` are one;
/// rpm prints `(none):` for no epoch).
fn bare(v: &str) -> &str {
    v.split_once(':').map_or(v, |(e, rest)| {
        if e == "(none)" || e.bytes().all(|b| b.is_ascii_digit()) {
            rest
        } else {
            v
        }
    })
}

/// The second check: the installed and repo versions, read again, must match
/// what the update list said.
pub fn verify_driver(
    u: &DriverUpdate,
    installed: Option<&str>,
    candidate: Option<&str>,
) -> Result<VerifiedDriver, DriverDoubt> {
    if !is_driver_pkg(&u.pkg) {
        return Err(DriverDoubt::NotADriver);
    }
    let installed = installed.ok_or(DriverDoubt::NotInstalled)?;
    if let Some(listed) = &u.from {
        if bare(listed) != bare(installed) {
            return Err(DriverDoubt::InstalledDiffers {
                listed: listed.clone(),
                installed: installed.into(),
            });
        }
    }
    if candidate.map(bare) != Some(bare(&u.to)) {
        return Err(DriverDoubt::CandidateDiffers {
            listed: u.to.clone(),
            repo: candidate.map(str::to_string),
        });
    }
    if bare(installed) == bare(&u.to) {
        return Err(DriverDoubt::NotNewer);
    }
    Ok(VerifiedDriver {
        pkg: u.pkg.clone(),
        from: installed.into(),
        to: u.to.clone(),
    })
}

/// The way back, best first: a snapshot, then the old packages.
/// `cached` says whether a file is in pacman's cache; `apt_old` whether apt
/// still offers a package's old version.
pub fn rollback_for(
    pm: PackageManager,
    drivers: &[VerifiedDriver],
    snapshots: &[SnapshotBackend],
    cached: &dyn Fn(&Path) -> bool,
    apt_old: &dyn Fn(&VerifiedDriver) -> bool,
) -> Option<Rollback> {
    if let Some(s) = snapshots.first() {
        return Some(Rollback::Snapshot(*s));
    }
    if drivers.is_empty() {
        return None;
    }
    match pm {
        PackageManager::Pacman => {
            let mut files = Vec::new();
            for d in drivers {
                let file = ["x86_64", "any"]
                    .iter()
                    // A Linux path on every build, so "/" and not the host's separator.
                    .map(|arch| {
                        PathBuf::from(format!(
                            "{PACMAN_CACHE}/{}-{}-{arch}.pkg.tar.zst",
                            d.pkg, d.from
                        ))
                    })
                    .find(|f| cached(f))?;
                files.push(file);
            }
            Some(Rollback::Cached(files))
        }
        PackageManager::Apt => drivers.iter().all(apt_old).then_some(Rollback::AptVersions),
        _ => None,
    }
}

/// The update command (run as root). pacman upgrades everything from the
/// lists already on disk (the versions just checked), because Arch doesn't
/// support updating one package alone; apt and dnf update just the driver
/// packages, apt at the exact version both checks agreed on.
pub fn update_cmd(pm: PackageManager, drivers: &[VerifiedDriver]) -> Option<String> {
    update_cmd_plain(pm, drivers).map(|(cmd, _)| cmd)
}

fn update_cmd_plain(pm: PackageManager, drivers: &[VerifiedDriver]) -> Option<(String, String)> {
    let names: Vec<String> = drivers
        .iter()
        .map(|d| ProgramName::parse(&d.pkg).map(|p| p.as_str().to_string()))
        .collect::<Option<_>>()?;
    let plain_versions = |d: &VerifiedDriver| {
        d.to.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '+' | '~' | ':' | '_'))
    };
    if !drivers.iter().all(plain_versions) {
        return None;
    }
    Some(match pm {
        PackageManager::Pacman => (
            "pacman -Su --noconfirm".to_string(),
            "Update your system from Arch's signed repos, the new driver included. Arch updates everything together so the driver matches the rest.".to_string(),
        ),
        PackageManager::Apt => (
            format!(
                "apt-get install --only-upgrade -y {}",
                drivers.iter().map(|d| format!("{}={}", d.pkg, d.to)).collect::<Vec<_>>().join(" ")
            ),
            format!("Update {} from your distro's signed repos.", names.join(", ")),
        ),
        PackageManager::Dnf => (
            format!("dnf -y -C upgrade {}", names.join(" ")),
            format!("Update {} from your distro's signed repos.", names.join(", ")),
        ),
        PackageManager::Zypper => return None,
    })
}

/// The update step for the driver card.
fn update_step(pm: PackageManager, drivers: &[VerifiedDriver]) -> Option<DraftStep> {
    let (cmd, plain) = update_cmd_plain(pm, drivers)?;
    Some(DraftStep::new(elevate(Os::Linux, &cmd), plain).elevated())
}

/// The driver update, once both checks agree and there is a way back.
pub fn driver_plan(
    finding: Finding,
    os: Os,
    pm: Option<PackageManager>,
    drivers: &[VerifiedDriver],
    rollback: Option<&Rollback>,
    game_running: bool,
) -> Result<FixPlan, DriverBlock> {
    if os == Os::Windows {
        return Err(DriverBlock::Windows);
    }
    if game_running {
        return Err(DriverBlock::GameRunning);
    }
    if drivers.is_empty() {
        return Err(DriverBlock::Nothing);
    }
    let pm = pm.ok_or(DriverBlock::Unsupported)?;
    let rollback = rollback.ok_or(DriverBlock::NoWayBack)?;
    let step = update_step(pm, drivers).ok_or(DriverBlock::Unsupported)?;
    let versions = drivers
        .iter()
        .map(|d| format!("{} {} to {}", d.pkg, d.from, d.to))
        .collect::<Vec<_>>()
        .join(", ");
    let undo = match rollback {
        Rollback::Snapshot(_) => "I take a system snapshot first. If the new driver misbehaves, roll back to it.",
        Rollback::Cached(_) | Rollback::AptVersions => {
            "The old driver stays available, so Undo can put it back. Restart afterwards either way."
        }
    };
    FixPlan::new(
        finding,
        format!("Update your driver ({versions}), then ask you to restart so it loads."),
        "I checked twice that this is the version your distro's signed repos offer and that it is newer than yours.",
        undo,
        "Medium: a new driver can misbehave with some games. The way back is ready before anything changes.",
        vec![step],
    )
    .map_err(|_| DriverBlock::Unsupported)
}

/// The old driver back, when the way back is the old packages. A snapshot is
/// rolled back with its own tool, so it has no plan here.
pub fn driver_rollback_plan(
    finding: Finding,
    drivers: &[VerifiedDriver],
    rollback: &Rollback,
    game_running: bool,
) -> Result<FixPlan, DriverBlock> {
    if game_running {
        return Err(DriverBlock::GameRunning);
    }
    let cmd = rollback_cmd(rollback, drivers).ok_or(DriverBlock::NoWayBack)?;
    let step = DraftStep::new(elevate(Os::Linux, &cmd), "Put the old driver back.").elevated();
    FixPlan::new(
        finding,
        "Put your old driver back, then ask you to restart so it loads.",
        "You asked to undo the driver update.",
        "Running the driver update again brings the new one back.",
        "Low: this is the driver you had before.",
        vec![step],
    )
    .map_err(|_| DriverBlock::Unsupported)
}

/// The command (run as root) that puts the old drivers back, when the way
/// back is the old packages. A snapshot is rolled back with its own tool.
pub fn rollback_cmd(rollback: &Rollback, drivers: &[VerifiedDriver]) -> Option<String> {
    match rollback {
        Rollback::Snapshot(_) => None,
        Rollback::Cached(files) => {
            let paths: Vec<&str> = files.iter().filter_map(|f| f.to_str()).collect();
            Some(format!("pacman -U --noconfirm {}", paths.join(" ")))
        }
        Rollback::AptVersions => Some(format!(
            "apt-get install -y --allow-downgrades {}",
            drivers
                .iter()
                .map(|d| format!("{}={}", d.pkg, d.from))
                .collect::<Vec<_>>()
                .join(" ")
        )),
    }
}

/// The re-check: the installed version now is the new one.
pub fn driver_updated(d: &VerifiedDriver, installed_now: Option<&str>) -> bool {
    installed_now.is_some_and(|v| bare(v) == bare(&d.to))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repair::{classify_step, read_only_violation, Severity, StepClass};

    fn finding() -> Finding {
        Finding {
            severity: Severity::Warning,
            plain: "A newer graphics driver is ready.".into(),
            detail: String::new(),
            probe: ProbeId::PendingUpdates,
        }
    }

    const PACMAN_QU: &str = "firefox 131.0-1 -> 132.0-1\nnvidia-utils 560.35.03-1 -> 565.57.01-1\nlib32-nvidia-utils 560.35.03-1 -> 565.57.01-1\n";

    #[test]
    fn the_update_list_names_only_driver_packages() {
        assert_eq!(
            driver_updates(PackageManager::Pacman, PACMAN_QU),
            vec![
                DriverUpdate {
                    pkg: "nvidia-utils".into(),
                    from: Some("560.35.03-1".into()),
                    to: "565.57.01-1".into()
                },
                DriverUpdate {
                    pkg: "lib32-nvidia-utils".into(),
                    from: Some("560.35.03-1".into()),
                    to: "565.57.01-1".into()
                },
            ]
        );
        let apt = "Listing...\nnvidia-driver-550/jammy-updates 550.127.05-0ubuntu1 amd64 [upgradable from: 550.120-0ubuntu1]\ncurl/jammy-updates 7.81.0-1ubuntu1.18 amd64 [upgradable from: 7.81.0-1ubuntu1.17]\n";
        assert_eq!(
            driver_updates(PackageManager::Apt, apt),
            vec![DriverUpdate {
                pkg: "nvidia-driver-550".into(),
                from: Some("550.120-0ubuntu1".into()),
                to: "550.127.05-0ubuntu1".into()
            }]
        );
        let dnf = "akmod-nvidia.x86_64   3:565.57.01-1.fc40   rpmfusion-nonfree-updates\nkernel.x86_64 6.11.5-300.fc40 updates\n";
        assert_eq!(
            driver_updates(PackageManager::Dnf, dnf),
            vec![DriverUpdate {
                pkg: "akmod-nvidia".into(),
                from: None,
                to: "3:565.57.01-1.fc40".into()
            }]
        );
    }

    #[test]
    fn the_second_check_must_agree_with_the_first() {
        let u = &driver_updates(PackageManager::Pacman, PACMAN_QU)[0];
        let installed = parse_installed(PackageManager::Pacman, "nvidia-utils 560.35.03-1\n");
        let si = "Repository      : extra\nName            : nvidia-utils\nVersion         : 565.57.01-1\nDescription     : NVIDIA drivers utilities\n";
        let candidate = parse_candidate(PackageManager::Pacman, si);
        assert_eq!(installed.as_deref(), Some("560.35.03-1"));
        assert_eq!(candidate.as_deref(), Some("565.57.01-1"));
        assert_eq!(
            verify_driver(u, installed.as_deref(), candidate.as_deref()),
            Ok(VerifiedDriver {
                pkg: "nvidia-utils".into(),
                from: "560.35.03-1".into(),
                to: "565.57.01-1".into()
            })
        );
        // The repo moved on since the list was read: nothing runs.
        assert_eq!(
            verify_driver(u, installed.as_deref(), Some("570.10-1")),
            Err(DriverDoubt::CandidateDiffers {
                listed: "565.57.01-1".into(),
                repo: Some("570.10-1".into())
            })
        );
        assert_eq!(
            verify_driver(u, Some("565.57.01-1"), candidate.as_deref()),
            Err(DriverDoubt::InstalledDiffers {
                listed: "560.35.03-1".into(),
                installed: "565.57.01-1".into()
            })
        );
        assert_eq!(
            verify_driver(u, None, candidate.as_deref()),
            Err(DriverDoubt::NotInstalled)
        );
        let firefox = DriverUpdate {
            pkg: "firefox".into(),
            from: None,
            to: "132.0-1".into(),
        };
        assert_eq!(
            verify_driver(&firefox, Some("131.0-1"), Some("132.0-1")),
            Err(DriverDoubt::NotADriver)
        );
        // dnf: rpm's epoch matches the list's, with or without it.
        let dnf = &driver_updates(
            PackageManager::Dnf,
            "akmod-nvidia.x86_64 3:565.57.01-1.fc40 rpmfusion\n",
        )[0];
        assert_eq!(
            verify_driver(dnf, Some("3:560.35.03-1.fc40"), Some("3:565.57.01-1.fc40"))
                .map(|v| v.from),
            Ok("3:560.35.03-1.fc40".to_string())
        );
        assert_eq!(bare("(none):1.2-3"), "1.2-3");
        // apt's candidate and version table.
        let policy = "nvidia-driver-550:\n  Installed: 550.120-0ubuntu1\n  Candidate: 550.127.05-0ubuntu1\n  Version table:\n     550.127.05-0ubuntu1 500\n        500 http://archive.ubuntu.com/ubuntu jammy-updates/restricted amd64 Packages\n *** 550.120-0ubuntu1 100\n        100 /var/lib/dpkg/status\n";
        assert_eq!(
            parse_candidate(PackageManager::Apt, policy).as_deref(),
            Some("550.127.05-0ubuntu1")
        );
        assert_eq!(
            apt_versions(policy),
            vec![
                "550.127.05-0ubuntu1".to_string(),
                "550.120-0ubuntu1".to_string()
            ]
        );
        assert_eq!(
            parse_installed(PackageManager::Apt, "nvidia-driver-550\t550.120-0ubuntu1\n")
                .as_deref(),
            Some("550.120-0ubuntu1")
        );
        // Every check is read only.
        let pkg = ProgramName::parse("nvidia-utils").unwrap();
        for pm in [
            PackageManager::Pacman,
            PackageManager::Apt,
            PackageManager::Dnf,
        ] {
            for spec in [
                installed_spec(pm, &pkg).unwrap(),
                candidate_spec(pm, &pkg).unwrap(),
            ] {
                assert_eq!(read_only_violation(&spec), None, "{}", spec.command_line());
            }
        }
    }

    #[test]
    fn a_driver_updates_only_with_a_way_back_and_never_during_a_game() {
        let v = vec![VerifiedDriver {
            pkg: "nvidia-utils".into(),
            from: "560.35.03-1".into(),
            to: "565.57.01-1".into(),
        }];
        let none_cached = |_: &Path| false;
        let no_apt = |_: &VerifiedDriver| false;
        assert_eq!(
            rollback_for(PackageManager::Pacman, &v, &[], &none_cached, &no_apt),
            None
        );
        assert_eq!(
            driver_plan(
                finding(),
                Os::Linux,
                Some(PackageManager::Pacman),
                &v,
                None,
                false
            )
            .unwrap_err(),
            DriverBlock::NoWayBack
        );
        let cached = |p: &Path| {
            p == Path::new("/var/cache/pacman/pkg/nvidia-utils-560.35.03-1-x86_64.pkg.tar.zst")
        };
        let back = rollback_for(PackageManager::Pacman, &v, &[], &cached, &no_apt).unwrap();
        assert_eq!(
            back,
            Rollback::Cached(vec![PathBuf::from(
                "/var/cache/pacman/pkg/nvidia-utils-560.35.03-1-x86_64.pkg.tar.zst"
            )])
        );
        assert_eq!(
            rollback_for(
                PackageManager::Pacman,
                &v,
                &[SnapshotBackend::Snapper],
                &none_cached,
                &no_apt
            ),
            Some(Rollback::Snapshot(SnapshotBackend::Snapper))
        );
        assert_eq!(
            driver_plan(
                finding(),
                Os::Linux,
                Some(PackageManager::Pacman),
                &v,
                Some(&back),
                true
            )
            .unwrap_err(),
            DriverBlock::GameRunning
        );
        let plan = driver_plan(
            finding(),
            Os::Linux,
            Some(PackageManager::Pacman),
            &v,
            Some(&back),
            false,
        )
        .unwrap();
        assert_eq!(plan.steps[0].command, "pkexec pacman -Su --noconfirm");
        assert_eq!(plan.what, "Update your driver (nvidia-utils 560.35.03-1 to 565.57.01-1), then ask you to restart so it loads.");
        assert!(plan.steps[0].elevated);
        assert_ne!(plan.steps[0].class, StepClass::HardFloor);
        let undo = driver_rollback_plan(finding(), &v, &back, false).unwrap();
        assert_eq!(
            undo.steps[0].command,
            "pkexec pacman -U --noconfirm /var/cache/pacman/pkg/nvidia-utils-560.35.03-1-x86_64.pkg.tar.zst"
        );
        let apt = vec![VerifiedDriver {
            pkg: "nvidia-driver-550".into(),
            from: "550.120-0ubuntu1".into(),
            to: "550.127.05-0ubuntu1".into(),
        }];
        let apt_back =
            rollback_for(PackageManager::Apt, &apt, &[], &none_cached, &|_| true).unwrap();
        let plan = driver_plan(
            finding(),
            Os::Linux,
            Some(PackageManager::Apt),
            &apt,
            Some(&apt_back),
            false,
        )
        .unwrap();
        assert_eq!(
            plan.steps[0].command,
            "pkexec apt-get install --only-upgrade -y nvidia-driver-550=550.127.05-0ubuntu1"
        );
        assert_eq!(
            driver_rollback_plan(finding(), &apt, &apt_back, false)
                .unwrap()
                .steps[0]
                .command,
            "pkexec apt-get install -y --allow-downgrades nvidia-driver-550=550.120-0ubuntu1"
        );
        assert_eq!(
            driver_plan(finding(), Os::Windows, None, &v, Some(&back), false).unwrap_err(),
            DriverBlock::Windows
        );
        assert!(driver_updated(&v[0], Some("565.57.01-1")));
        assert!(!driver_updated(&v[0], Some("560.35.03-1")));
        assert_eq!(
            classify_step(&plan.steps[0].command, &[]),
            plan.steps[0].class
        );
    }
}
