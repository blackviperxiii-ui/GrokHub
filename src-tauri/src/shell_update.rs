//! The shell updates itself (LU-06). Each release carries a `latest.json`
//! that points at the Linux AppImage and the Windows NSIS installer, each
//! minisign-signed with the release key in `packaging/minisign.pub`. This
//! finds the channel's newest release on GitHub, hands its `latest.json` to
//! `tauri-plugin-updater`, and installs only when the user clicks. Every
//! Windows build and a Linux AppImage update this way; Linux package and
//! clone installs and macOS builds don't.
//!
//! The channel is the `channel` receipt in the config folder, which
//! `scripts/install.sh --channel` writes too, so the two are one setting.

use std::io::Read;
use std::path::Path;
use std::time::Duration;

use grokhub_core::release_verify::{
    download_url_ok, parse_releases, pick_release, releases_api, Release, RELEASE_PUBKEY,
};
use grokhub_core::{Channel, CHANNEL_BETA_SINCE, CHANNEL_RECEIPT};
use semver::Version;
use serde::Serialize;

/// The update manifest each release carries next to its assets.
pub const MANIFEST_NAME: &str = "latest.json";
/// How often the shell looks for an update while it runs.
pub const CHECK_EVERY: Duration = Duration::from_secs(2 * 60 * 60);
/// Emitted with an [`UpdateView`] when a background check finds an update.
pub const UPDATE_EVENT: &str = "update://available";
/// The release list is a few hundred KB at most.
const LIST_CAP: u64 = 4 * 1024 * 1024;

/// `3.5.0-beta.47` on Beta, `3.5.0` on Stable.
pub fn running_version(base: &str, channel: Channel, beta_n: u64) -> String {
    match channel {
        Channel::Beta => format!("{base}-beta.{beta_n}"),
        Channel::Stable => base.to_string(),
    }
}

/// The channel this binary was built for (build.rs).
pub fn build_channel() -> Channel {
    Channel::parse(env!("GROKHUB_SHELL_CHANNEL")).unwrap_or_default()
}

/// This build's version, as Settings → About shows it.
pub fn version() -> String {
    running_version(
        env!("GROKHUB_SHELL_BASE"),
        build_channel(),
        env!("GROKHUB_SHELL_BETA_N").parse().unwrap_or(0),
    )
}

/// Offer `remote` when it is newer, or when the user switched channels and it
/// is any other version (Beta → Stable may step back to the stable build).
pub fn offer(remote: &Version, running: &Version, switching: bool) -> bool {
    if switching {
        remote != running
    } else {
        remote > running
    }
}

/// The channel the user picked: the receipt, else the one this build is on.
pub fn channel(config_dir: &Path) -> Channel {
    std::fs::read_to_string(config_dir.join(CHANNEL_RECEIPT))
        .ok()
        .filter(|t| !t.trim().is_empty())
        .map(|t| Channel::from_receipt(&t))
        .unwrap_or_else(build_channel)
}

/// Write the shared channel receipt. A switch drops the old Beta auto-off
/// baseline.
pub fn set_channel(config_dir: &Path, channel: Channel) -> Result<(), String> {
    // A model catalog saved from the old channel's releases mustn't outlive it.
    if self::channel(config_dir) != channel {
        grokhub_core::catalog_update::forget(config_dir);
    }
    std::fs::create_dir_all(config_dir).map_err(|e| format!("channel receipt: {e}"))?;
    std::fs::write(config_dir.join(CHANNEL_RECEIPT), channel.receipt())
        .map_err(|e| format!("channel receipt: {e}"))?;
    let _ = std::fs::remove_file(config_dir.join(CHANNEL_BETA_SINCE));
    Ok(())
}

/// Whether this install can replace itself: the AppImage (`$APPIMAGE` set)
/// and the Windows installer can; a `.deb`, macOS and phones can't.
pub fn updatable(os: &str, appimage: Option<&str>) -> bool {
    match os {
        "linux" => appimage.is_some_and(|p| !p.trim().is_empty()),
        "windows" => true,
        _ => false,
    }
}

/// Why this install doesn't update itself, for the Updates panel. The note
/// names the clone only when `scripts/install.sh`'s `source` receipt points
/// at one and this binary sits in a user's `bin` folder outside `/usr`: a copy
/// under `/usr` may be a package (root's receipts aren't the user's), and one
/// run from a build folder isn't installed at all. The note never guesses the
/// install's options, since sudo and `--prefix` aren't recorded.
pub fn not_updatable_note(os: &str, config_dir: &Path, channel: Channel, exe: &Path) -> String {
    let ch = channel.as_str();
    if os != "linux" {
        return "This build doesn't update itself yet.".to_string();
    }
    let in_bin = exe.parent().and_then(Path::file_name) == Some("bin".as_ref());
    let clone = std::fs::read_to_string(config_dir.join("source"))
        .ok()
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty() && Path::new(t).is_dir())
        .filter(|_| in_bin && !exe.starts_with("/usr"));
    match clone {
        Some(clone) => format!(
            "Built from the clone in {clone}: to update it, run scripts/install.sh --from-source --channel {ch} there with the options you installed it with."
        ),
        None => format!(
            "This copy updates outside the app: use your package manager for a .deb or AUR install, or run scripts/install.sh --from-source --channel {ch} again in the clone you built it from."
        ),
    }
}

/// Why a link from outside this repo's release downloads was refused.
fn not_ours(what: &str) -> String {
    format!(
        "the update {what} isn't from {}'s GitHub releases",
        grokhub_core::brand::product_name()
    )
}

/// The release's `latest.json` link, only from this repo's release downloads.
pub fn manifest_url(release: &Release) -> Result<String, String> {
    let asset = release
        .asset(MANIFEST_NAME)
        .ok_or_else(|| format!("{} has no update manifest", release.tag))?;
    if !download_url_ok(&asset.url, MANIFEST_NAME) {
        return Err(not_ours("manifest"));
    }
    Ok(asset.url.clone())
}

/// The updater's public key: the minisign key file, base64 as the plugin wants.
pub fn updater_pubkey() -> Option<String> {
    use base64::Engine as _;
    RELEASE_PUBKEY.map(|pk| base64::engine::general_purpose::STANDARD.encode(pk.trim_end()))
}

/// The newest release on `channel`, read from GitHub behind the egress guard.
pub fn find_release(config_dir: &Path, channel: Channel) -> Result<Release, String> {
    let url = releases_api(channel);
    grokhub_agent::harness::guard_quiet(
        config_dir,
        &grokhub_agent::harness::EgressReq::new(url, &[]),
    )?;
    let agent = grokhub_core::http::agent(grokhub_core::http::Opts {
        connect: Duration::from_secs(20),
        overall: Some(Duration::from_secs(60)),
        public_only: true,
        ..Default::default()
    });
    let resp = agent
        .get(url)
        .header("user-agent", grokhub_agent::USER_AGENT)
        .header("accept", "application/vnd.github+json")
        .call()
        .map_err(|e| match e.status() {
            Some(code) => format!("GitHub answered HTTP {code}"),
            None => "couldn't reach GitHub".to_string(),
        })?;
    let mut body = Vec::new();
    resp.into_reader()
        .take(LIST_CAP + 1)
        .read_to_end(&mut body)
        .map_err(|e| e.to_string())?;
    if body.len() as u64 > LIST_CAP {
        return Err("the release list is too large".into());
    }
    let releases = parse_releases(&String::from_utf8_lossy(&body));
    pick_release(&releases, channel)
        .cloned()
        .ok_or_else(|| format!("no {} release yet", channel.as_str()))
}

/// What Settings → About → Updates shows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateView {
    pub version: String,
    pub channel: &'static str,
    pub updatable: bool,
    /// Why it can't update itself, when it can't.
    pub note: Option<String>,
    /// The version a check found, ready to install.
    pub available: Option<String>,
    /// Set when the last update was rolled back because it didn't stay up.
    pub rolled_back: Option<String>,
}

impl UpdateView {
    pub fn now(config_dir: &Path) -> Self {
        let os = std::env::consts::OS;
        let appimage = std::env::var("APPIMAGE").ok();
        let updatable = updatable(os, appimage.as_deref());
        let channel = channel(config_dir);
        Self {
            version: version(),
            channel: channel.as_str(),
            updatable,
            note: (!updatable).then(|| {
                let exe = std::env::current_exe().unwrap_or_default();
                not_updatable_note(os, config_dir, channel, &exe)
            }),
            available: None,
            #[cfg(desktop)]
            rolled_back: crate::update_rollback::rolled_back_note(config_dir),
            #[cfg(not(desktop))]
            rolled_back: None,
        }
    }
}

/// The updater plugin's side: check the channel's `latest.json` and install
/// what it offers. Desktop only; phones never build the plugin.
#[cfg(desktop)]
pub mod plugin {
    use super::*;
    use tauri::AppHandle;
    use tauri_plugin_updater::{Update, UpdaterExt};

    /// The update the channel's newest release offers this build, if any.
    pub async fn check(app: &AppHandle, config_dir: &Path) -> Result<Option<Update>, String> {
        grokhub_core::offline::refuse()?;
        let ch = channel(config_dir);
        let dir = config_dir.to_path_buf();
        let release = tauri::async_runtime::spawn_blocking(move || find_release(&dir, ch))
            .await
            .map_err(|e| e.to_string())??;
        let url = manifest_url(&release)?;
        let running =
            Version::parse(&version()).map_err(|e| format!("this build's version: {e}"))?;
        let switching = ch != build_channel();
        let pubkey = updater_pubkey().ok_or("this build has no release key")?;
        let handle = app.clone();
        let updater = app
            .updater_builder()
            // Windows exits inside the install; run the app's quit steps first.
            .on_before_exit(move || crate::before_update_exit(&handle))
            .endpoints(vec![url
                .parse()
                .map_err(|_| "bad manifest link".to_string())?])
            .map_err(|e| e.to_string())?
            .pubkey(pubkey)
            .timeout(Duration::from_secs(300))
            .version_comparator(move |_bundle, remote| offer(&remote.version, &running, switching))
            .build()
            .map_err(|e| e.to_string())?;
        let update = updater.check().await.map_err(|e| e.to_string())?;
        if let Some(u) = &update {
            let link = u.download_url.as_str();
            let name = link.rsplit('/').next().unwrap_or_default();
            if !download_url_ok(link, name) {
                return Err(not_ours("download"));
            }
            grokhub_agent::harness::guard_quiet(
                config_dir,
                &grokhub_agent::harness::EgressReq::new(link, &[]),
            )?;
        }
        Ok(update)
    }

    /// Download, verify (the plugin refuses a bad signature) and install the
    /// offered update, then restart into it. On Windows the plugin starts the
    /// installer and exits the process itself, after the app's quit steps
    /// ([`crate::before_update_exit`]); a launch it can't make comes back as
    /// an error.
    pub async fn install(app: &AppHandle, config_dir: &Path) -> Result<(), String> {
        let update = check(app, config_dir)
            .await?
            .ok_or("no update to install")?;
        // An AppImage keeps a copy of itself, so a new version that can't
        // stay up is rolled back (crate::update_rollback). Without the copy
        // the update still installs.
        let appimage = std::env::var_os("APPIMAGE").map(std::path::PathBuf::from);
        if let Some(target) = appimage.as_deref().filter(|p| p.is_file()) {
            let _ =
                crate::update_rollback::prepare(config_dir, target, &version(), &update.version);
        }
        if let Err(e) = update.download_and_install(|_, _| {}, || {}).await {
            crate::update_rollback::abandon(config_dir);
            return Err(format!("update not installed: {e}"));
        }
        app.restart()
    }
}

#[cfg(test)]
#[path = "shell_update_tests.rs"]
mod tests;
