use super::*;

// A throwaway key made for these tests; its secret half was never kept.
const PUB: &str = include_str!("../data/test_catalog/test.pub");
const TEXT: &str = include_str!("../data/test_catalog/local_catalog.toml");
const SUMS: &str = include_str!("../data/test_catalog/SHA256SUMS");
const SIG: &str = include_str!("../data/test_catalog/SHA256SUMS.minisig");

fn temp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("catalog-update-{name}-{}", crate::new_token()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn the_built_in_catalog_has_a_version_and_loads_whole() {
    assert!(crate::local_setup::bundled_version() >= 2);
    let (_, rows) = crate::local_setup::parse_catalog(include_str!("../data/local_catalog.toml"))
        .expect("the built-in catalog loads whole");
    assert_eq!(rows.len(), crate::local_setup::local_models().len());
}

#[test]
fn a_signed_catalog_verifies_and_anything_changed_does_not() {
    assert_eq!(check(PUB, TEXT, SUMS, SIG), Ok(99));
    let edited = TEXT.replace("Test Chat", "Test Chat 2");
    assert!(check(PUB, &edited, SUMS, SIG).is_err());
    let other_key = crate::release_verify::RELEASE_PUBKEY.unwrap();
    assert!(check(other_key, TEXT, SUMS, SIG).is_err());
    assert!(check(PUB, TEXT, SUMS, "").is_err());
}

#[test]
fn a_catalog_that_doesnt_load_whole_is_refused() {
    let (_, rows) = crate::local_setup::parse_catalog(TEXT).unwrap();
    assert_eq!(rows[0].id, "test-chat-q4_k");
    // A row with neither check, an unknown field type or no default chat
    // model: the whole file is refused, not trimmed.
    for bad in [
        TEXT.replace("sha256 = ", "sha_typo = "),
        TEXT.replace("run_mb = 1_024", "run_mb = \"lots\""),
        TEXT.replace("[[model]]\n", "[[model]]\noptional = true\n"),
        "version = 9\nmodel = []\n".to_string(),
    ] {
        assert!(crate::local_setup::parse_catalog(&bad).is_none(), "{bad}");
    }
}

#[test]
fn only_a_newer_catalog_is_saved_and_used() {
    let cfg = temp("adopt");
    assert_eq!(saved_with(&cfg, PUB, 2), None, "nothing saved yet");
    assert_eq!(
        adopt(&cfg, PUB, 99, "beta", TEXT, SUMS, SIG),
        Ok(None),
        "not newer"
    );
    assert!(!dir(&cfg).join(FILE).exists());
    assert_eq!(adopt(&cfg, PUB, 2, "beta", TEXT, SUMS, SIG), Ok(Some(99)));
    assert_eq!(saved_with(&cfg, PUB, 2).as_deref(), Some(TEXT));
    // An app whose own catalog caught up ignores the saved one.
    assert_eq!(saved_with(&cfg, PUB, 99), None);
    // Changed on disk after saving: no longer used.
    std::fs::write(dir(&cfg).join(FILE), TEXT.replace("Test Chat", "Swapped")).unwrap();
    assert_eq!(saved_with(&cfg, PUB, 2), None);
    assert!(adopt(
        &cfg,
        PUB,
        2,
        "beta",
        &TEXT.replace("Test", "Bad"),
        SUMS,
        SIG
    )
    .is_err());
    let _ = std::fs::remove_dir_all(&cfg);
}

#[test]
fn the_release_assets_must_come_from_this_repo() {
    let base = "https://github.com/blackviperxiii-ui/GrokHub/releases/download/v9.9.9";
    let asset = |name: &str, url: String| ReleaseAsset {
        name: name.into(),
        url,
    };
    let mut rel = Release {
        tag: "v9.9.9".into(),
        prerelease: false,
        draft: false,
        created: String::new(),
        assets: [FILE, rv::SUMS_NAME, rv::SIG_NAME]
            .iter()
            .map(|n| asset(n, format!("{base}/{n}")))
            .collect(),
    };
    assert!(assets(&rel).is_some());
    rel.assets[0].url = format!("https://example.com/{FILE}");
    assert!(assets(&rel).is_none());
    rel.assets.remove(0);
    assert!(assets(&rel).is_none());
}

#[test]
fn a_look_waits_a_day() {
    let cfg = temp("due");
    assert!(due(&cfg, 1_000));
    stamp(&cfg, 1_000);
    assert!(!due(&cfg, 1_000 + CHECK_EVERY_MS - 1));
    assert!(due(&cfg, 1_000 + CHECK_EVERY_MS));
    // Stamped while the clock ran ahead: due again once it is set back.
    stamp(&cfg, 1_000 + 10 * CHECK_EVERY_MS);
    assert!(due(&cfg, 1_000 + CHECK_EVERY_MS));
    let _ = std::fs::remove_dir_all(&cfg);
}

#[test]
fn forgetting_drops_the_saved_catalog_and_its_stamp() {
    let cfg = temp("forget");
    assert_eq!(adopt(&cfg, PUB, 2, "beta", TEXT, SUMS, SIG), Ok(Some(99)));
    stamp(&cfg, 5_000);
    forget(&cfg);
    assert_eq!(saved_with(&cfg, PUB, 2), None);
    assert!(due(&cfg, 5_001));
    let _ = std::fs::remove_dir_all(&cfg);
}

#[test]
fn a_save_that_cant_write_keeps_the_old_catalog_and_looks_again() {
    let cfg = temp("full");
    assert_eq!(adopt(&cfg, PUB, 2, "beta", TEXT, SUMS, SIG), Ok(Some(99)));
    stamp(&cfg, 5_000);
    // The signature's part file can't be written (a folder is in the way).
    std::fs::create_dir_all(dir(&cfg).join(format!("{}.part", rv::SIG_NAME))).unwrap();
    let got = adopt(&cfg, PUB, 2, "beta", TEXT, SUMS, SIG).unwrap_err();
    assert!(got.starts_with("SHA256SUMS.minisig: "), "{got}");
    // Nothing saved was replaced, so the saved catalog still verifies.
    assert_eq!(saved_with(&cfg, PUB, 2).as_deref(), Some(TEXT));
    assert!(due(&cfg, 5_001), "the next look runs at once");
    let _ = std::fs::remove_dir_all(&cfg);
}

fn release_with_catalog() -> Release {
    let base = "https://github.com/blackviperxiii-ui/GrokHub/releases/download/v9.9.9";
    Release {
        tag: "v9.9.9".into(),
        prerelease: false,
        draft: false,
        created: String::new(),
        assets: [FILE, rv::SUMS_NAME, rv::SIG_NAME]
            .iter()
            .map(|n| ReleaseAsset {
                name: (*n).into(),
                url: format!("{base}/{n}"),
            })
            .collect(),
    }
}

/// A `get` that serves the release's three files from `files` and records
/// each URL and cap it was asked for.
fn serve<'a>(
    files: [&'a str; 3],
    asked: &'a mut Vec<(String, u64)>,
) -> impl FnMut(&str, u64) -> Result<Vec<u8>, String> + 'a {
    move |url: &str, cap: u64| {
        asked.push((url.rsplit('/').next().unwrap_or("").to_string(), cap));
        let body = match url.rsplit('/').next() {
            Some(FILE) => files[0],
            Some(rv::SUMS_NAME) => files[1],
            Some(rv::SIG_NAME) => files[2],
            other => panic!("unexpected download {other:?}"),
        };
        Ok(body.as_bytes().to_vec())
    }
}

#[test]
fn a_newer_signed_catalog_from_the_release_is_saved_and_a_tampered_one_is_not() {
    let cfg = temp("refresh");
    let rel = release_with_catalog();
    let try_with = |files: [&str; 3]| {
        let mut asked = Vec::new();
        let got = refresh(PUB, &rel, &cfg, "beta", &mut serve(files, &mut asked));
        (got, asked)
    };

    let edited = TEXT.replace("Test Chat", "Test Chat 2");
    let (got, _) = try_with([&edited, SUMS, SIG]);
    assert_eq!(got, Err("the download's checksum didn't match".to_string()));
    assert!(
        !dir(&cfg).join(FILE).exists(),
        "a changed catalog is never written"
    );

    let sums = SUMS.replacen('0', "1", 1);
    let (got, _) = try_with([TEXT, &sums, SIG]);
    assert_eq!(got, Err("signature didn't verify".to_string()));
    assert!(
        !dir(&cfg).join(FILE).exists(),
        "changed sums fail the signature"
    );

    let (got, asked) = try_with([TEXT, SUMS, SIG]);
    assert_eq!(got, Ok(Some(99)));
    // The sums and signature come first, each capped small; the catalog last.
    assert_eq!(
        asked,
        vec![
            (rv::SUMS_NAME.to_string(), TEXT_CAP),
            (rv::SIG_NAME.to_string(), TEXT_CAP),
            (FILE.to_string(), MAX_BYTES),
        ]
    );
    assert_eq!(saved_with(&cfg, PUB, 2).as_deref(), Some(TEXT));
    let _ = std::fs::remove_dir_all(&cfg);
}

#[test]
fn an_unsigned_or_foreign_release_is_never_fetched() {
    let cfg = temp("unsigned");
    let mut nothing = |url: &str, _cap: u64| -> Result<Vec<u8>, String> {
        panic!("nothing may be downloaded, asked for {url}")
    };
    let mut rel = release_with_catalog();
    rel.assets.retain(|a| a.name != rv::SIG_NAME);
    assert_eq!(refresh(PUB, &rel, &cfg, "beta", &mut nothing), Ok(None));

    let mut rel = release_with_catalog();
    rel.assets[2].url = format!("https://example.com/{}", rv::SIG_NAME);
    assert_eq!(refresh(PUB, &rel, &cfg, "beta", &mut nothing), Ok(None));
    assert!(!dir(&cfg).exists());
    let _ = std::fs::remove_dir_all(&cfg);
}

#[test]
fn a_catalog_that_isnt_text_is_refused() {
    let cfg = temp("binary");
    let rel = release_with_catalog();
    let mut get = |url: &str, _cap: u64| -> Result<Vec<u8>, String> {
        Ok(if url.ends_with(FILE) {
            vec![0xff, 0xfe]
        } else if url.ends_with(rv::SIG_NAME) {
            SIG.as_bytes().to_vec()
        } else {
            SUMS.as_bytes().to_vec()
        })
    };
    assert_eq!(
        refresh(PUB, &rel, &cfg, "beta", &mut get),
        Err("local_catalog.toml isn't text".to_string())
    );
    assert!(!dir(&cfg).join(FILE).exists());
    let _ = std::fs::remove_dir_all(&cfg);
}

#[test]
fn a_catalog_from_another_channel_or_half_saved_is_tidied_away() {
    let cfg = temp("tidy");
    assert!(!tidy_with(&cfg, "beta", PUB, 2), "nothing saved");
    assert_eq!(adopt(&cfg, PUB, 2, "beta", TEXT, SUMS, SIG), Ok(Some(99)));
    assert_eq!(saved_channel(&cfg).as_deref(), Some("beta"));
    stamp(&cfg, 5_000);
    assert!(!tidy_with(&cfg, "beta", PUB, 2), "same channel, still good");
    assert_eq!(saved_with(&cfg, PUB, 2).as_deref(), Some(TEXT));

    // The channel changed without the switch (an installer, a script).
    assert!(tidy_with(&cfg, "stable", PUB, 2));
    assert_eq!(saved_with(&cfg, PUB, 2), None);
    assert!(due(&cfg, 5_001), "the new channel is looked at at once");

    // Half saved: the new catalog beside the old signature.
    assert_eq!(adopt(&cfg, PUB, 2, "beta", TEXT, SUMS, SIG), Ok(Some(99)));
    std::fs::write(dir(&cfg).join(rv::SIG_NAME), "untrusted comment: torn\n").unwrap();
    assert!(tidy_with(&cfg, "beta", PUB, 2));
    assert!(!dir(&cfg).join(FILE).exists());

    // An app whose own catalog caught up drops the saved one.
    assert_eq!(adopt(&cfg, PUB, 2, "beta", TEXT, SUMS, SIG), Ok(Some(99)));
    assert!(tidy_with(&cfg, "beta", PUB, 99));
    let _ = std::fs::remove_dir_all(&cfg);
}

#[test]
fn every_reader_skips_a_catalog_from_another_channel() {
    let cfg = temp("receipt");
    assert_eq!(adopt(&cfg, PUB, 2, "beta", TEXT, SUMS, SIG), Ok(Some(99)));
    // No receipt yet: the app's start-up tidy decides.
    assert_eq!(saved_with(&cfg, PUB, 2).as_deref(), Some(TEXT));
    std::fs::write(cfg.join(crate::CHANNEL_RECEIPT), "beta\n").unwrap();
    assert_eq!(saved_with(&cfg, PUB, 2).as_deref(), Some(TEXT));
    // An installer wrote the stable receipt: the hub doesn't use it either.
    std::fs::write(cfg.join(crate::CHANNEL_RECEIPT), "stable\n").unwrap();
    assert_eq!(saved_with(&cfg, PUB, 2), None);
    // A catalog saved before channels were recorded isn't used.
    std::fs::write(cfg.join(crate::CHANNEL_RECEIPT), "beta\n").unwrap();
    std::fs::remove_file(dir(&cfg).join("channel")).unwrap();
    assert_eq!(saved_with(&cfg, PUB, 2), None);
    let _ = std::fs::remove_dir_all(&cfg);
}
