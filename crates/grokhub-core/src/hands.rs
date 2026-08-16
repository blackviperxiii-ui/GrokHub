//! Find desk tools and say why hands are down. No spawn here.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

pub const HANDS_PACMAN: &str = "scripts/build-hands.sh";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HandsDown {
    Ready,
    Missing,
    Uinput,
    Daemon,
}

pub fn extra_bin_dirs(home: Option<&str>) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(h) = home.map(str::trim).filter(|s| !s.is_empty()) {
        dirs.push(PathBuf::from(h).join(".local/lib/grokhub/bin"));
    }
    dirs.push(PathBuf::from("/usr/lib/grokhub/bin"));
    if let Some(h) = home.map(str::trim).filter(|s| !s.is_empty()) {
        dirs.push(PathBuf::from(h).join(".local/bin"));
    }
    dirs.push(PathBuf::from("/usr/local/bin"));
    dirs.push(PathBuf::from("/usr/bin"));
    dirs
}

/// Walk PATH plus GrokHub sidecars and `~/.local/bin` even when the GUI PATH is only `/usr/bin`.
pub fn resolve_bin_in(name: &str, path: Option<&str>, home: Option<&str>) -> Option<PathBuf> {
    let name = name.trim();
    if name.is_empty() || name.contains('/') || name.contains('\\') {
        return None;
    }
    let mut dirs = extra_bin_dirs(home);
    if let Some(p) = path {
        for d in p.split(':') {
            if !d.is_empty() {
                dirs.push(PathBuf::from(d));
            }
        }
    }
    let mut seen = BTreeSet::new();
    for d in dirs {
        if !seen.insert(d.clone()) {
            continue;
        }
        let cand = d.join(name);
        if file_is_bin(&cand) {
            return Some(cand);
        }
    }
    None
}

fn file_is_bin(p: &Path) -> bool {
    p.is_file()
}

pub fn diagnose_hands(
    has_ydotool: bool,
    has_xdotool: bool,
    uinput_writable: Option<bool>,
    daemon_up: Option<bool>,
) -> HandsDown {
    if !has_ydotool && !has_xdotool {
        return HandsDown::Missing;
    }
    if has_ydotool {
        if uinput_writable == Some(false) {
            return HandsDown::Uinput;
        }
        if daemon_up == Some(false) {
            return HandsDown::Daemon;
        }
    }
    HandsDown::Ready
}

pub const PYATSPI_MISSING: &str =
    "python-atspi missing — install python-atspi so Eyes / act / wait_for can walk the accessibility tree. Without it the windshield falls back to a wmctrl window list.";

pub fn hands_down_receipt(reason: HandsDown) -> &'static str {
    match reason {
        HandsDown::Ready => "hands ready",
        HandsDown::Missing => {
            "ydotool/xdotool missing — run ./scripts/install.sh to build ydotool, grim, xdotool, and wmctrl into ~/.local/lib/grokhub/bin. Include ~/.local/lib/grokhub/bin and ~/.local/bin on PATH."
        }
        HandsDown::Uinput => {
            "uinput blocked — load the uinput module, add your user to the input group, then log out. Hands cannot drive the desk until /dev/uinput is writable."
        }
        HandsDown::Daemon => {
            "ydotoold is down — start the user unit or run ydotoold. COMPUTER_CMD cannot move the pointer until the socket is up."
        }
    }
}

pub fn hands_chip_label(reason: HandsDown, driver: &str) -> String {
    match reason {
        HandsDown::Ready => {
            if driver.is_empty() || driver == "missing" {
                "ready".into()
            } else {
                driver.to_string()
            }
        }
        HandsDown::Missing => "not installed".into(),
        HandsDown::Uinput => "uinput".into(),
        HandsDown::Daemon => "daemon".into(),
    }
}

pub fn hands_chip_live(reason: HandsDown) -> bool {
    reason == HandsDown::Ready
}

pub fn ydotool_socket_path(explicit: Option<&str>, runtime_dir: Option<&str>) -> PathBuf {
    if let Some(p) = explicit.map(str::trim).filter(|s| !s.is_empty()) {
        return PathBuf::from(p);
    }
    if let Some(rt) = runtime_dir.map(str::trim).filter(|s| !s.is_empty()) {
        return PathBuf::from(rt).join("ydotool.sock");
    }
    PathBuf::from("/tmp/.ydotool_socket")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn path_walk_finds_local_bin_when_path_is_usr_bin() {
        let root = std::env::temp_dir().join(format!("grokhub-hands-{}", std::process::id()));
        let local = root.join(".local/bin");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&local).unwrap();
        let bin = local.join("ydotool");
        fs::write(&bin, "#!/bin/sh\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&bin, fs::Permissions::from_mode(0o755)).unwrap();
        }
        let found = resolve_bin_in("ydotool", Some("/usr/bin"), root.to_str());
        assert_eq!(found.as_deref(), Some(bin.as_path()));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn sidecar_prefix_wins_over_local_bin() {
        let root = std::env::temp_dir().join(format!("grokhub-hands-prefix-{}", std::process::id()));
        let sidecar = root.join(".local/lib/grokhub/bin");
        let local = root.join(".local/bin");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&sidecar).unwrap();
        fs::create_dir_all(&local).unwrap();
        let side_bin = sidecar.join("ydotool");
        let local_bin = local.join("ydotool");
        fs::write(&side_bin, "#!/bin/sh\n").unwrap();
        fs::write(&local_bin, "#!/bin/sh\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&side_bin, fs::Permissions::from_mode(0o755)).unwrap();
            fs::set_permissions(&local_bin, fs::Permissions::from_mode(0o755)).unwrap();
        }
        let found = resolve_bin_in("ydotool", Some("/usr/bin"), root.to_str());
        assert_eq!(found.as_deref(), Some(side_bin.as_path()));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn missing_uinput_daemon_receipts_are_distinct() {
        assert_eq!(
            diagnose_hands(false, false, None, None),
            HandsDown::Missing
        );
        assert_eq!(
            diagnose_hands(true, false, Some(false), Some(false)),
            HandsDown::Uinput
        );
        assert_eq!(
            diagnose_hands(true, false, Some(true), Some(false)),
            HandsDown::Daemon
        );
        assert_eq!(
            diagnose_hands(true, false, Some(true), Some(true)),
            HandsDown::Ready
        );
        assert_eq!(
            diagnose_hands(false, true, None, None),
            HandsDown::Ready
        );
        let miss = hands_down_receipt(HandsDown::Missing);
        let uinput = hands_down_receipt(HandsDown::Uinput);
        let daemon = hands_down_receipt(HandsDown::Daemon);
        assert!(miss.contains("lib/grokhub/bin"));
        assert!(uinput.contains("uinput"));
        assert!(daemon.contains("ydotoold"));
        assert_ne!(miss, uinput);
        assert_ne!(uinput, daemon);
        assert_eq!(hands_chip_label(HandsDown::Missing, "missing"), "not installed");
        assert_eq!(hands_chip_label(HandsDown::Uinput, "ydotool"), "uinput");
        assert_eq!(hands_chip_label(HandsDown::Daemon, "ydotool"), "daemon");
        assert_eq!(hands_chip_label(HandsDown::Ready, "ydotool"), "ydotool");
        assert!(!hands_chip_live(HandsDown::Missing));
        assert!(hands_chip_live(HandsDown::Ready));
        assert_eq!(HANDS_PACMAN, "scripts/build-hands.sh");
        let build = include_str!("../../../scripts/build-hands.sh");
        assert!(
            build.contains("v1.0.4")
                && build.contains("xdotool")
                && build.contains("wmctrl")
                && build.contains("lib/grokhub/bin"),
            "build-hands.sh must pin ydotool and build xdotool/wmctrl sidecars"
        );
        let dirs = extra_bin_dirs(Some("/home/cabin"));
        assert!(
            dirs[0].ends_with(".local/lib/grokhub/bin"),
            "sidecar prefix must be first: {dirs:?}"
        );
        assert_eq!(dirs[1], PathBuf::from("/usr/lib/grokhub/bin"));
        let sh = include_str!("../../../scripts/install.sh");
        assert!(
            sh.contains("build-hands.sh")
                && sh.contains("usermod -aG input")
                && sh.contains("sudo install")
                && sh.contains("60-grokhub-uinput.rules")
                && sh.contains("modprobe uinput")
                && sh.contains("python-atspi"),
            "clone install must build sidecars, install python-atspi, the uinput udev rule, and keep the input group"
        );
        assert!(
            !sh.contains("sudo pacman -S --needed ydotool"),
            "clone install must not hard-require pacman ydotool"
        );
        let srcinfo = include_str!("../../../packaging/aur/.SRCINFO");
        assert!(
            srcinfo.contains("optdepends = ydotool")
                && srcinfo.contains("optdepends = grim")
                && srcinfo.contains("makedepends = cmake")
                && srcinfo.contains("makedepends = libx11")
                && srcinfo.contains("depends = python-atspi")
                && !srcinfo.contains("\tdepends = ydotool")
                && !srcinfo.contains("slurp"),
            "AUR metadata must treat pointer tools as sidecars and python-atspi as a depend: {srcinfo}"
        );
        let local_pkg = include_str!("../../../packaging/PKGBUILD");
        assert!(
            local_pkg.contains("build-hands.sh")
                && local_pkg.contains("cmake")
                && local_pkg.contains("libx11")
                && local_pkg.contains("'python-atspi'")
                && !local_pkg.contains("slurp"),
            "clone makepkg must build sidecars and depend on python-atspi"
        );
        assert!(
            PYATSPI_MISSING.contains("python-atspi") && PYATSPI_MISSING.contains("wmctrl"),
            "Install hands must tell the user when pyatspi is missing"
        );
        let bundle = include_str!("../../../scripts/make-release-bundle.sh");
        assert!(
            bundle.contains("build-hands.sh")
                && bundle.contains("ydotoold.service")
                && bundle.contains("modprobe uinput")
                && bundle.contains("python-atspi"),
            "release tarball install must ship the unit, build sidecars, and install python-atspi"
        );
        assert!(
            !bundle.contains("sudo pacman -S --needed ydotool"),
            "release tarball must not hard-require pacman ydotool"
        );
        assert_eq!(
            ydotool_socket_path(None, Some("/run/user/1000")),
            PathBuf::from("/run/user/1000/ydotool.sock")
        );
        assert_eq!(
            ydotool_socket_path(Some("/tmp/custom.sock"), Some("/run/user/1000")),
            PathBuf::from("/tmp/custom.sock")
        );
    }
}
