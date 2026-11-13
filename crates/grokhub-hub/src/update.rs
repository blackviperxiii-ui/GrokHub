//! `grokhub-hub --update`: says how this install gets a newer version. It
//! never downloads, installs or runs anything; the apps' own updaters and the
//! package manager do that.

use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallKind {
    /// The Windows installer or an AppImage: the app's own updater.
    AppUpdater,
    /// Under `/usr`: a .deb or the AUR package.
    Package,
    /// macOS: a new .dmg from the release page.
    Dmg,
    /// Anything else, such as `scripts/install.sh --user` from a clone.
    Source,
}

/// Where this binary runs from. `appimage` is the `APPIMAGE` variable.
pub fn install_kind(os: &str, exe: &Path, appimage: Option<&str>) -> InstallKind {
    if os == "windows" || appimage.is_some_and(|p| !p.is_empty()) {
        InstallKind::AppUpdater
    } else if os == "macos" {
        InstallKind::Dmg
    } else if exe.starts_with("/usr") {
        InstallKind::Package
    } else {
        InstallKind::Source
    }
}

pub fn advice(kind: InstallKind) -> String {
    let app = grokhub_core::brand::product_name();
    match kind {
        InstallKind::AppUpdater => {
            format!("Open {app} → Settings → Updates to update this install.")
        }
        InstallKind::Package => "This copy came from a package. Update it with your package \
manager: sudo apt upgrade for the .deb, or your AUR helper on Arch."
            .to_string(),
        InstallKind::Dmg => "Download the newest .dmg from \
https://github.com/blackviperxiii-ui/GrokHub/releases/latest and replace the app."
            .to_string(),
        InstallKind::Source => "This copy was built from source. In your GrokHub clone run \
git pull, then scripts/install.sh --user."
            .to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_install_kind_gets_its_own_line() {
        assert_eq!(
            install_kind(
                "linux",
                Path::new("/tmp/.mount_x/usr/bin/grokhub-hub"),
                Some("/home/u/Loadout.AppImage")
            ),
            InstallKind::AppUpdater
        );
        assert_eq!(
            install_kind(
                "windows",
                Path::new("C:\\Program Files\\GrokHub\\grokhub-hub.exe"),
                None
            ),
            InstallKind::AppUpdater
        );
        assert_eq!(
            install_kind("linux", Path::new("/usr/bin/grokhub-hub"), None),
            InstallKind::Package
        );
        assert_eq!(
            install_kind(
                "linux",
                Path::new("/home/u/.local/bin/grokhub-hub"),
                Some("")
            ),
            InstallKind::Source
        );
        assert_eq!(
            install_kind("macos", Path::new("/Applications/x/grokhub-hub"), None),
            InstallKind::Dmg
        );
        assert_eq!(
            advice(InstallKind::AppUpdater),
            "Open Loadout → Settings → Updates to update this install."
        );
        assert_eq!(
            advice(InstallKind::Source),
            "This copy was built from source. In your GrokHub clone run git pull, then scripts/install.sh --user."
        );
        assert_eq!(
            advice(InstallKind::Package),
            "This copy came from a package. Update it with your package manager: sudo apt upgrade for the .deb, or your AUR helper on Arch."
        );
    }
}
