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
/// `config_dir` is the shell config folder (for the `source` receipt); when
/// omitted, a user `bin` install without `$APPIMAGE` stays Source.
pub fn install_kind(
    os: &str,
    exe: &Path,
    appimage: Option<&str>,
    config_dir: Option<&Path>,
) -> InstallKind {
    if os == "windows" || appimage.is_some_and(|p| !p.is_empty()) {
        InstallKind::AppUpdater
    } else if os == "linux"
        && config_dir.is_some_and(|dir| {
            let in_bin = exe.parent().and_then(Path::file_name) == Some("bin".as_ref());
            in_bin
                && !exe.starts_with("/usr")
                && std::fs::read_to_string(dir.join("source"))
                    .ok()
                    .map(|t| t.trim().to_string())
                    .filter(|t| !t.is_empty() && Path::new(t).is_dir())
                    .is_none()
        })
    {
        // Prebuilt AppImage under ~/.local/bin (Arch/CachyOS): same as AppImage.
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
scripts/install.sh --channel <channel>."
            .to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_install_kind_gets_its_own_line() {
        let dir = std::env::temp_dir().join(format!("hub-update-kind-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("dir");
        assert_eq!(
            install_kind(
                "linux",
                Path::new("/tmp/.mount_x/usr/bin/grokhub-hub"),
                Some("/home/u/Loadout.AppImage"),
                None
            ),
            InstallKind::AppUpdater
        );
        assert_eq!(
            install_kind(
                "windows",
                Path::new("C:\\Program Files\\GrokHub\\grokhub-hub.exe"),
                None,
                None
            ),
            InstallKind::AppUpdater
        );
        assert_eq!(
            install_kind("linux", Path::new("/usr/bin/grokhub-hub"), None, None),
            InstallKind::Package
        );
        // No config_dir: keep Source when $APPIMAGE is empty.
        assert_eq!(
            install_kind(
                "linux",
                Path::new("/home/u/.local/bin/grokhub-hub"),
                Some(""),
                None
            ),
            InstallKind::Source
        );
        // Prebuilt user install (no clone receipt): AppUpdater on Arch-style paths.
        assert_eq!(
            install_kind(
                "linux",
                Path::new("/home/u/.local/bin/grokhub-hub"),
                None,
                Some(&dir)
            ),
            InstallKind::AppUpdater
        );
        let clone = dir.join("clone");
        std::fs::create_dir_all(&clone).expect("clone");
        std::fs::write(dir.join("source"), format!("{}\n", clone.display())).expect("receipt");
        assert_eq!(
            install_kind(
                "linux",
                Path::new("/home/u/.local/bin/grokhub-hub"),
                None,
                Some(&dir)
            ),
            InstallKind::Source
        );
        assert_eq!(
            install_kind("macos", Path::new("/Applications/x/grokhub-hub"), None, None),
            InstallKind::Dmg
        );
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(
            advice(InstallKind::AppUpdater),
            "Open Loadout → Settings → Updates to update this install."
        );
        assert_eq!(
            advice(InstallKind::Source),
            "This copy was built from source. In your GrokHub clone run scripts/install.sh --channel <channel>."
        );
        assert_eq!(
            advice(InstallKind::Package),
            "This copy came from a package. Update it with your package manager: sudo apt upgrade for the .deb, or your AUR helper on Arch."
        );
    }
}
