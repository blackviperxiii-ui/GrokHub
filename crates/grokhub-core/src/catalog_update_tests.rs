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
    assert_eq!(adopt(&cfg, PUB, 99, TEXT, SUMS, SIG), Ok(None), "not newer");
    assert!(!dir(&cfg).join(FILE).exists());
    assert_eq!(adopt(&cfg, PUB, 2, TEXT, SUMS, SIG), Ok(Some(99)));
    assert_eq!(saved_with(&cfg, PUB, 2).as_deref(), Some(TEXT));
    // An app whose own catalog caught up ignores the saved one.
    assert_eq!(saved_with(&cfg, PUB, 99), None);
    // Changed on disk after saving: no longer used.
    std::fs::write(dir(&cfg).join(FILE), TEXT.replace("Test Chat", "Swapped")).unwrap();
    assert_eq!(saved_with(&cfg, PUB, 2), None);
    assert!(adopt(&cfg, PUB, 2, &TEXT.replace("Test", "Bad"), SUMS, SIG).is_err());
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
    let _ = std::fs::remove_dir_all(&cfg);
}
