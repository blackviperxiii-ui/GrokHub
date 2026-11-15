use super::*;
use std::cell::RefCell;

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "grokhub-shell-catalog-{name}-{}-{}",
        std::process::id(),
        grokhub_core::now_ms()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

/// A release that ships no catalog: nothing is downloaded from it.
fn bare_release() -> Release {
    Release {
        tag: "v9.9.9-beta.1".into(),
        prerelease: true,
        draft: false,
        created: String::new(),
        assets: Vec::new(),
    }
}

fn no_download(url: &str, _cap: u64) -> Result<Vec<u8>, String> {
    panic!("nothing may be downloaded, asked for {url}")
}

fn stamp_in(dir: &Path) -> String {
    std::fs::read_to_string(cu::dir(dir).join("checked_ms")).unwrap_or_default()
}

#[test]
fn a_look_runs_once_a_day_on_the_saved_channel() {
    let dir = temp_dir("daily");
    crate::shell_update::set_channel(&dir, Channel::Beta).expect("channel");
    let asked = RefCell::new(Vec::new());
    let find = |_: &Path, channel: Channel| {
        asked.borrow_mut().push(channel);
        Ok(bare_release())
    };
    let now = 1_000_000;
    assert_eq!(run(&dir, now, &find, &mut no_download), Ok(None));
    assert_eq!(*asked.borrow(), vec![Channel::Beta]);
    assert_eq!(stamp_in(&dir), now.to_string());

    let never =
        |_: &Path, _: Channel| -> Result<Release, String> { panic!("a second look the same day") };
    assert_eq!(
        run(&dir, now + cu::CHECK_EVERY_MS - 1, &never, &mut no_download),
        Ok(None)
    );
    assert_eq!(
        run(&dir, now + cu::CHECK_EVERY_MS, &find, &mut no_download),
        Ok(None)
    );
    assert_eq!(*asked.borrow(), vec![Channel::Beta, Channel::Beta]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_failed_lookup_still_waits_a_day() {
    let dir = temp_dir("failed");
    let offline = |_: &Path, _: Channel| -> Result<Release, String> {
        Err("couldn't reach GitHub".to_string())
    };
    let now = 5_000;
    assert_eq!(
        run(&dir, now, &offline, &mut no_download),
        Err("couldn't reach GitHub".to_string())
    );
    assert!(!cu::due(&dir, now + 1));
    assert!(!cu::dir(&dir).join(cu::FILE).exists());
    let _ = std::fs::remove_dir_all(&dir);
}

/// The live download only dials GitHub's release hosts over HTTPS.
#[test]
fn downloads_go_only_to_githubs_release_hosts() {
    let dir = temp_dir("hosts");
    for url in [
        "http://github.com/blackviperxiii-ui/GrokHub/releases/download/v1/local_catalog.toml",
        "https://example.com/local_catalog.toml",
        "https://127.0.0.1/local_catalog.toml",
    ] {
        let got = download(&dir, url, cu::MAX_BYTES);
        assert!(got.is_err(), "{url}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}
