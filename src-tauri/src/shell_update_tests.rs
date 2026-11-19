use super::*;
use grokhub_core::release_verify::ReleaseAsset;

fn v(s: &str) -> Version {
    Version::parse(s).expect("semver")
}

#[test]
fn the_running_version_carries_the_beta_number() {
    assert_eq!(running_version("3.5.0", Channel::Beta, 47), "3.5.0-beta.47");
    assert_eq!(running_version("3.5.0", Channel::Stable, 47), "3.5.0");
    assert!(Version::parse(&version()).is_ok());
}

#[test]
fn only_a_newer_build_is_offered_unless_the_channel_changed() {
    assert!(offer(&v("3.5.0-beta.48"), &v("3.5.0-beta.47"), false));
    assert!(!offer(&v("3.5.0-beta.47"), &v("3.5.0-beta.47"), false));
    assert!(!offer(&v("3.5.0-beta.46"), &v("3.5.0-beta.47"), false));
    // A beta of 3.5.0 is older than 3.5.0 itself.
    assert!(!offer(&v("3.5.0-beta.48"), &v("3.5.0"), false));
    assert!(offer(&v("3.5.0-beta.48"), &v("3.5.0"), true));
    assert!(offer(&v("3.4.9"), &v("3.5.0-beta.48"), true));
    assert!(!offer(&v("3.5.0"), &v("3.5.0"), true));
}

fn release(url: &str) -> Release {
    Release {
        tag: "v3.5.0-beta.48".into(),
        prerelease: true,
        draft: false,
        created: "2026-11-01T03:00:00Z".into(),
        assets: vec![ReleaseAsset {
            name: MANIFEST_NAME.into(),
            url: url.into(),
        }],
    }
}

#[test]
fn the_manifest_comes_only_from_this_repos_releases() {
    let good =
        "https://github.com/blackviperxiii-ui/GrokHub/releases/download/v3.5.0-beta.48/latest.json";
    assert_eq!(manifest_url(&release(good)), Ok(good.to_string()));
    assert_eq!(
        manifest_url(&release(
            "https://example.org/blackviperxiii-ui/GrokHub/releases/download/v3.5.0-beta.48/latest.json"
        )),
        Err("the update manifest isn't from Loadout's GitHub releases".to_string())
    );
    let mut none = release(good);
    none.assets.clear();
    assert_eq!(
        manifest_url(&none),
        Err("v3.5.0-beta.48 has no update manifest".to_string())
    );
}

#[test]
fn only_the_appimage_and_the_windows_installer_update_themselves() {
    assert!(!updatable("linux", None));
    assert!(!updatable("linux", Some("")));
    assert!(updatable("linux", Some("/home/v/Apps/loadout.AppImage")));
    assert!(updatable("windows", None));
    assert!(!updatable("macos", None));
    assert!(!updatable("android", None));
}

#[test]
fn a_build_that_cant_update_itself_says_how_to_update_it() {
    let dir = std::env::temp_dir().join(format!("shell-update-note-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let clone = dir.join("my-clone");
    std::fs::create_dir_all(&clone).expect("dir");
    let note = |os: &str, channel: Channel, exe: &str| {
        not_updatable_note(os, &dir, channel, Path::new(exe))
    };
    let outside = "This copy updates outside the app: use your package manager for a .deb or AUR install, or run scripts/install.sh --from-source --channel beta again in the clone you built it from.";
    let not_yet = "This build doesn't update itself yet.";
    // No receipt: nothing says where this copy came from.
    assert_eq!(
        note(
            "linux",
            Channel::Beta,
            "/home/user/.local/bin/grokhub-shell"
        ),
        outside
    );
    assert_eq!(
        note("linux", Channel::Beta, "/usr/bin/grokhub-shell"),
        outside
    );
    std::fs::write(dir.join("source"), format!("{}\n", clone.display())).expect("receipt");
    let shown = clone.display();
    for exe in [
        "/home/user/.local/bin/grokhub-shell",
        "/opt/loadout/bin/grokhub-shell",
    ] {
        assert_eq!(
            note("linux", Channel::Stable, exe),
            format!(
                "Built from the clone in {shown}: to update it, run scripts/install.sh --from-source --channel stable there with the options you installed it with."
            ),
            "{exe}"
        );
    }
    // A user receipt doesn't describe a copy under /usr (a package, or a sudo
    // install whose receipt went to root) or a dev build in a target folder.
    for exe in [
        "/usr/bin/grokhub-shell",
        "/usr/local/bin/grokhub-shell",
        "/home/user/GrokHub/src-tauri/target/debug/grokhub-shell",
    ] {
        assert_eq!(note("linux", Channel::Beta, exe), outside, "{exe}");
    }
    // A receipt whose clone is gone no longer describes this install.
    std::fs::remove_dir_all(&clone).expect("rm clone");
    assert_eq!(
        note(
            "linux",
            Channel::Beta,
            "/home/user/.local/bin/grokhub-shell"
        ),
        outside
    );
    assert_eq!(
        note("android", Channel::Stable, "/data/app/grokhub-shell"),
        not_yet
    );
    assert_eq!(
        note(
            "macos",
            Channel::Stable,
            "/Applications/Loadout.app/Contents/MacOS/grokhub-shell"
        ),
        not_yet
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_receipt_is_shared_and_a_switch_drops_the_baseline() {
    let dir = std::env::temp_dir().join(format!("shell-update-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(channel(&dir), build_channel());
    std::fs::create_dir_all(&dir).expect("dir");
    std::fs::write(dir.join(CHANNEL_BETA_SINCE), "x").expect("baseline");
    set_channel(&dir, Channel::Beta).expect("write");
    assert_eq!(
        std::fs::read_to_string(dir.join(CHANNEL_RECEIPT)).expect("read"),
        "beta\n"
    );
    assert!(!dir.join(CHANNEL_BETA_SINCE).exists());
    assert_eq!(channel(&dir), Channel::Beta);
    set_channel(&dir, Channel::Stable).expect("write");
    assert_eq!(channel(&dir), Channel::Stable);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_updater_key_is_the_release_key_in_base64() {
    use base64::Engine as _;
    let b64 = updater_pubkey().expect("a key ships");
    let text = String::from_utf8(
        base64::engine::general_purpose::STANDARD
            .decode(&b64)
            .expect("base64"),
    )
    .expect("utf8");
    assert_eq!(
        text.lines().next(),
        Some("untrusted comment: minisign public key 31AA83C3CB171678")
    );
}

// A throwaway minisign key pair (secret never kept) proves the plugin's
// check: the signature over the bytes verifies, one flipped byte fails.
const TEST_PUB: &str = "untrusted comment: minisign public key CB53A10AE47051AE\nRWSuUXDkCqFTy8aCWU95WxgzjtEf7hIRvFsHGNTsMnUNJM52KlpBfGYS\n";
const TEST_SUMS: &str = "a82d46432d0f6baa57ebb1b0e8a03307df6724580cc316c763e61ae1bfe8cccf  grokhub-linux-v9.9.9.tar.gz\n8e527193e30f66d04ad8a6f13d97f4d577a00dec5a936ccb968ea06ac332399f  grokhub-windows-v9.9.9.zip\n";
const TEST_SIG: &str = "untrusted comment: signature from minisign secret key\nRUSuUXDkCqFTywkM99kAdB59lHEjxXHCvwXejm2aIWEwZkvY8ryGGeQv1kP/DwbWoUiK6t0Ap4T2qIpC5ZvQ19xwurPwtbqKlgc=\ntrusted comment: grokhub v9.9.9 SHA256SUMS\noH4W8+yEHIBuwIbbjoh4i8fO4uZeWygt/BAkA1j6j+BIo9Yh2zTrjYEWrsRDRkBOp3OcE50kjKj9uZIDUDCnAw==\n";

#[test]
fn a_minisign_signature_verifies_the_way_the_updater_reads_it() {
    let pk = minisign_verify::PublicKey::decode(TEST_PUB.trim()).expect("key");
    let sig = minisign_verify::Signature::decode(TEST_SIG.trim()).expect("sig");
    assert!(pk.verify(TEST_SUMS.as_bytes(), &sig, true).is_ok());
    let mut tampered = TEST_SUMS.as_bytes().to_vec();
    tampered[0] ^= 1;
    assert!(pk.verify(&tampered, &sig, true).is_err());
}
