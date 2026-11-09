use std::path::{Path, PathBuf};

use super::*;

/// A fake package manager: answers by the probe's program and package.
fn runner(
    answers: &'static [(&'static str, &'static str, &'static str)],
) -> impl Fn(&ProbeSpec) -> Option<String> {
    move |spec: &ProbeSpec| {
        let pkg = spec.argv.last()?;
        answers
            .iter()
            .find(|(prog, p, _)| *prog == spec.program && p == pkg)
            .map(|(_, _, out)| out.to_string())
    }
}

const PACMAN_PENDING: &str =
    "linux 6.10.1-1 -> 6.10.2-1\nnvidia-utils 560.35.03-1 -> 565.57.01-1\n";

#[test]
fn a_pacman_driver_checked_twice_with_the_old_package_cached_can_update() {
    let run = runner(&[("pacman", "nvidia-utils", "nvidia-utils 560.35.03-1\n")]);
    // -Q and -Si share the program; answer -Si by its own field.
    let run = move |spec: &ProbeSpec| {
        if spec.argv.first().map(String::as_str) == Some("-Si") {
            Some("Name            : nvidia-utils\nVersion         : 565.57.01-1\n".into())
        } else {
            run(spec)
        }
    };
    let cached = |p: &Path| {
        p == Path::new("/var/cache/pacman/pkg/nvidia-utils-560.35.03-1-x86_64.pkg.tar.zst")
    };
    let c = check(PackageManager::Pacman, PACMAN_PENDING, &run, &[], &cached);
    assert_eq!(
        c.verified,
        vec![VerifiedDriver {
            pkg: "nvidia-utils".into(),
            from: "560.35.03-1".into(),
            to: "565.57.01-1".into(),
        }]
    );
    // linux isn't a driver package, so it isn't checked at all.
    assert!(c.doubts.is_empty());
    assert_eq!(
        c.rollback,
        Some(Rollback::Cached(vec![PathBuf::from(
            "/var/cache/pacman/pkg/nvidia-utils-560.35.03-1-x86_64.pkg.tar.zst"
        )]))
    );
    assert_eq!(
        update_script(PackageManager::Pacman, &c, false),
        Ok("pacman -Su --noconfirm".into())
    );
    assert_eq!(
        update_script(PackageManager::Pacman, &c, true),
        Err(DriverBlock::GameRunning)
    );
    let record = DriverRecord {
        drivers: c.verified.clone(),
        rollback: c.rollback.clone().unwrap(),
    };
    assert_eq!(
        rollback_script(&record, false),
        Ok("pacman -U --noconfirm /var/cache/pacman/pkg/nvidia-utils-560.35.03-1-x86_64.pkg.tar.zst".into())
    );
}

#[test]
fn a_repo_that_offers_another_version_than_the_list_is_left_alone() {
    let run = |spec: &ProbeSpec| match spec.argv.first().map(String::as_str) {
        Some("-Si") => Some("Version         : 570.00-1\n".to_string()),
        _ => Some("nvidia-utils 560.35.03-1\n".to_string()),
    };
    let c = check(PackageManager::Pacman, PACMAN_PENDING, &run, &[], &|_| true);
    assert!(c.verified.is_empty());
    assert_eq!(c.doubts.len(), 1);
    assert!(matches!(
        c.doubts[0].1,
        DriverDoubt::CandidateDiffers { .. }
    ));
    assert_eq!(
        update_script(PackageManager::Pacman, &c, false),
        Err(DriverBlock::Nothing)
    );
}

#[test]
fn with_no_way_back_nothing_updates() {
    let run = |spec: &ProbeSpec| match spec.argv.first().map(String::as_str) {
        Some("-Si") => Some("Version         : 565.57.01-1\n".to_string()),
        _ => Some("nvidia-utils 560.35.03-1\n".to_string()),
    };
    let c = check(
        PackageManager::Pacman,
        PACMAN_PENDING,
        &run,
        &[SnapshotBackend::Btrfs],
        &|_| false,
    );
    assert_eq!(c.verified.len(), 1);
    // A bare btrfs snapshot doesn't count, and the old package isn't cached.
    assert_eq!(c.rollback, None);
    assert_eq!(
        update_script(PackageManager::Pacman, &c, false),
        Err(DriverBlock::NoWayBack)
    );
}

#[test]
fn apt_snapshots_first_then_updates_to_the_exact_version() {
    const PENDING: &str = "nvidia-driver-550/jammy-updates 550.127.05-0ubuntu1 amd64 [upgradable from: 550.120-0ubuntu1]\n";
    const POLICY: &str = "nvidia-driver-550:\n  Installed: 550.120-0ubuntu1\n  Candidate: 550.127.05-0ubuntu1\n  Version table:\n     550.127.05-0ubuntu1 500\n        500 http://archive.ubuntu.com/ubuntu jammy-updates/restricted amd64 Packages\n *** 550.120-0ubuntu1 100\n        100 /var/lib/dpkg/status\n";
    let run = runner(&[
        (
            "dpkg-query",
            "nvidia-driver-550",
            "nvidia-driver-550\t550.120-0ubuntu1\n",
        ),
        ("apt-cache", "nvidia-driver-550", POLICY),
    ]);
    let snapper = check(
        PackageManager::Apt,
        PENDING,
        &run,
        &[SnapshotBackend::Snapper],
        &|_| false,
    );
    assert_eq!(
        snapper.rollback,
        Some(Rollback::Snapshot(SnapshotBackend::Snapper))
    );
    assert_eq!(
        update_script(PackageManager::Apt, &snapper, false),
        Ok("snapper create --description before-driver-update\napt-get install --only-upgrade -y nvidia-driver-550=550.127.05-0ubuntu1".into())
    );
    // No snapshot tool: the old version is still in apt's table.
    let plain = check(PackageManager::Apt, PENDING, &run, &[], &|_| false);
    assert_eq!(plain.rollback, Some(Rollback::AptVersions));
    let record = DriverRecord {
        drivers: plain.verified.clone(),
        rollback: Rollback::AptVersions,
    };
    assert_eq!(
        rollback_script(&record, false),
        Ok("apt-get install -y --allow-downgrades nvidia-driver-550=550.120-0ubuntu1".into())
    );
    // A snapshot goes back with its own tool.
    let snap = DriverRecord {
        drivers: plain.verified,
        rollback: Rollback::Snapshot(SnapshotBackend::Snapper),
    };
    assert_eq!(rollback_script(&snap, false), Err(DriverBlock::NoWayBack));
}

#[test]
fn the_recheck_needs_every_driver_at_its_new_version() {
    let d = vec![VerifiedDriver {
        pkg: "nvidia-utils".into(),
        from: "560.35.03-1".into(),
        to: "565.57.01-1".into(),
    }];
    let now_new = |_: &ProbeSpec| Some("nvidia-utils 565.57.01-1\n".to_string());
    let still_old = |_: &ProbeSpec| Some("nvidia-utils 560.35.03-1\n".to_string());
    assert!(all_updated(PackageManager::Pacman, &d, &now_new));
    assert!(!all_updated(PackageManager::Pacman, &d, &still_old));
}

#[test]
fn the_record_round_trips_through_its_file() {
    let dir = std::env::temp_dir().join(format!("driver-record-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let r = DriverRecord {
        drivers: vec![VerifiedDriver {
            pkg: "mesa".into(),
            from: "24.1".into(),
            to: "24.2".into(),
        }],
        rollback: Rollback::Cached(vec![PathBuf::from(
            "/var/cache/pacman/pkg/mesa-24.1-x86_64.pkg.tar.zst",
        )]),
    };
    save_record(&dir, &r).unwrap();
    assert_eq!(load_record(&dir), Some(r));
    clear_record(&dir);
    assert_eq!(load_record(&dir), None);
    std::fs::remove_dir_all(&dir).unwrap();
}
