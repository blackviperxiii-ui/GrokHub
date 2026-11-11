use super::*;
use crate::activity::{self as code_activity, Entry, Kind};
use std::cell::RefCell;
use std::collections::HashMap;

/// An in-memory stand-in for the OS keychain.
#[derive(Default)]
struct MockVault {
    items: RefCell<HashMap<String, String>>,
}

impl Vault for MockVault {
    fn get(&self, account: &str) -> Result<Option<String>, String> {
        Ok(self.items.borrow().get(account).cloned())
    }
    fn set(&self, account: &str, value: &str) -> Result<(), String> {
        self.items
            .borrow_mut()
            .insert(account.to_string(), value.to_string());
        Ok(())
    }
    fn delete(&self, account: &str) -> Result<(), String> {
        self.items.borrow_mut().remove(account);
        Ok(())
    }
}

const KEY: &str = "repo-key-1";
const VALUE: &str = "abcd1234XYZ";

fn dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("code-secrets-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn activity(summary: &str, command: &str) -> Entry {
    Entry {
        ts_ms: 1,
        kind: Kind::Push,
        repo_key: KEY.into(),
        host: "github".into(),
        branch: "code/deploy".into(),
        command: command.into(),
        mode: "ask".into(),
        approved_by: "user".into(),
        exit: Some(0),
        summary: summary.into(),
    }
}

#[test]
fn a_stored_value_goes_to_the_vault_and_never_into_a_list_or_file() {
    let d = dir("list");
    let vault = MockVault::default();
    let listed = set(&d, &vault, KEY, "DEPLOY_TOKEN", VALUE).unwrap();
    assert_eq!(listed.names, vec!["DEPLOY_TOKEN".to_string()]);
    assert_eq!(
        vault.get("code-secret:repo-key-1:DEPLOY_TOKEN").unwrap(),
        Some(VALUE.to_string())
    );
    let json = serde_json::to_string(&list(&d, KEY)).unwrap();
    assert_eq!(json, r#"{"names":["DEPLOY_TOKEN"]}"#);
    let on_disk = std::fs::read_to_string(d.join(FILE)).unwrap();
    assert!(on_disk.contains("DEPLOY_TOKEN"), "{on_disk}");
    assert!(!on_disk.contains(VALUE), "{on_disk}");
    // Another project sees nothing.
    assert!(list(&d, "other-key").names.is_empty());
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn a_stored_value_never_reaches_the_activity_log() {
    let d = dir("activity");
    let vault = MockVault::default();
    set(&d, &vault, KEY, "DEPLOY_TOKEN", VALUE).unwrap();
    let loaded = load_values(&d, &vault, KEY).unwrap();
    let e = activity(
        &format!("deploy said token={VALUE} ok"),
        &format!("deploy --token {VALUE}"),
    );
    code_activity::append_held(&d, &e, code_activity::KEEP_DEFAULT, &loaded.held()).unwrap();
    let raw = std::fs::read_to_string(d.join(code_activity::FILE)).unwrap();
    assert!(!raw.contains(VALUE), "{raw}");
    let got = code_activity::list(&d, 1);
    assert_eq!(got[0].summary, "deploy said token=[redacted] ok");
    assert_eq!(got[0].command, "deploy --token [redacted]");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn redaction_replaces_the_value_with_the_fixed_marker() {
    assert_eq!(MARKER, "[redacted]");
    assert_eq!(
        redact("token=abcd1234XYZ ok", &[VALUE.to_string()]),
        "token=[redacted] ok"
    );
    // A value that contains another is hidden whole, not left half-shown.
    assert_eq!(
        redact(
            "a=abcd1234XYZ-long b=abcd1234XYZ",
            &[VALUE.to_string(), "abcd1234XYZ-long".to_string()]
        ),
        "a=[redacted] b=[redacted]"
    );
    let loaded = Loaded {
        pairs: vec![("DEPLOY_TOKEN".into(), VALUE.into())],
    };
    assert_eq!(loaded.redact("x abcd1234XYZ y"), "x [redacted] y");
    // Debug never prints the value either.
    let dbg = format!("{loaded:?}");
    assert!(!dbg.contains(VALUE), "{dbg}");
    assert!(dbg.contains("DEPLOY_TOKEN"), "{dbg}");
}

#[test]
fn env_injection_sets_the_exact_name_and_value_for_the_child() {
    let d = dir("env");
    let vault = MockVault::default();
    set(&d, &vault, KEY, "DEPLOY_TOKEN", VALUE).unwrap();
    set(&d, &vault, KEY, "API_KEY", "key-9876").unwrap();
    let loaded = load_values(&d, &vault, KEY).unwrap();
    assert_eq!(
        loaded.names(),
        vec!["API_KEY".to_string(), "DEPLOY_TOKEN".to_string()]
    );
    let mut cmd = Command::new("env");
    loaded.inject(&mut cmd);
    let envs: Vec<(String, String)> = cmd
        .get_envs()
        .map(|(k, v)| {
            (
                k.to_string_lossy().into_owned(),
                v.map(|v| v.to_string_lossy().into_owned())
                    .unwrap_or_default(),
            )
        })
        .collect();
    assert_eq!(
        envs,
        vec![
            ("API_KEY".to_string(), "key-9876".to_string()),
            ("DEPLOY_TOKEN".to_string(), VALUE.to_string()),
        ]
    );
    let _ = std::fs::remove_dir_all(&d);
}

#[cfg(unix)]
#[test]
fn a_spawned_child_reads_the_injected_value() {
    let d = dir("spawn");
    let vault = MockVault::default();
    set(&d, &vault, KEY, "DEPLOY_TOKEN", VALUE).unwrap();
    let loaded = load_values(&d, &vault, KEY).unwrap();
    let mut cmd = Command::new("sh");
    cmd.args(["-c", "printf '%s' \"$DEPLOY_TOKEN\""]);
    loaded.inject(&mut cmd);
    let out = cmd.output().unwrap();
    assert_eq!(String::from_utf8_lossy(&out.stdout), VALUE);
    assert_eq!(
        loaded.redact(&String::from_utf8_lossy(&out.stdout)),
        "[redacted]"
    );
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn delete_without_a_confirmation_is_refused_and_keeps_the_secret() {
    let d = dir("delete");
    let vault = MockVault::default();
    set(&d, &vault, KEY, "DEPLOY_TOKEN", VALUE).unwrap();
    assert_eq!(
        delete(&d, &vault, KEY, "DEPLOY_TOKEN", false),
        Err("Deleting a secret needs a confirmation first.".to_string())
    );
    assert_eq!(list(&d, KEY).names, vec!["DEPLOY_TOKEN".to_string()]);
    assert_eq!(
        vault.get("code-secret:repo-key-1:DEPLOY_TOKEN").unwrap(),
        Some(VALUE.to_string())
    );
    let after = delete(&d, &vault, KEY, "DEPLOY_TOKEN", true).unwrap();
    assert!(after.names.is_empty());
    assert_eq!(
        vault.get("code-secret:repo-key-1:DEPLOY_TOKEN").unwrap(),
        None
    );
    assert_eq!(
        delete(&d, &vault, KEY, "DEPLOY_TOKEN", true),
        Err("There's no secret called DEPLOY_TOKEN in this project.".to_string())
    );
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn replace_keeps_one_name_and_swaps_the_value() {
    let d = dir("replace");
    let vault = MockVault::default();
    set(&d, &vault, KEY, "DEPLOY_TOKEN", VALUE).unwrap();
    let listed = set(&d, &vault, KEY, "DEPLOY_TOKEN", "new-value-42\n").unwrap();
    assert_eq!(listed.names, vec!["DEPLOY_TOKEN".to_string()]);
    assert_eq!(
        vault.get("code-secret:repo-key-1:DEPLOY_TOKEN").unwrap(),
        Some("new-value-42".to_string())
    );
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn bad_names_and_short_values_are_refused_before_the_vault() {
    let d = dir("refuse");
    let vault = MockVault::default();
    for name in [
        "",
        "1TOKEN",
        "MY-TOKEN",
        "A B",
        "PATH",
        "ld_preload",
        "DYLD_X",
    ] {
        assert!(set(&d, &vault, KEY, name, VALUE).is_err(), "{name}");
    }
    assert_eq!(
        set(&d, &vault, KEY, "TOKEN", "abc"),
        Err("A secret needs at least 4 characters so it can be hidden in output.".to_string())
    );
    assert_eq!(
        set(&d, &vault, KEY, "TOKEN", "   "),
        Err("Paste the secret's value first.".to_string())
    );
    assert!(vault.items.borrow().is_empty());
    assert!(list(&d, KEY).names.is_empty());
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn a_listed_name_with_no_value_is_an_error_that_names_it() {
    let d = dir("missing");
    let vault = MockVault::default();
    set(&d, &vault, KEY, "DEPLOY_TOKEN", VALUE).unwrap();
    vault.items.borrow_mut().clear();
    assert_eq!(
        load_values(&d, &vault, KEY).unwrap_err(),
        "DEPLOY_TOKEN has no value in the keychain. Set it again in Settings → Code."
    );
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn the_keychain_account_carries_the_project_and_name() {
    assert_eq!(account("k1", "DEPLOY_TOKEN"), "code-secret:k1:DEPLOY_TOKEN");
}

#[test]
fn held_values_skip_a_name_with_no_value_and_keep_the_rest() {
    let d = dir("held");
    let vault = MockVault::default();
    set(&d, &vault, KEY, "DEPLOY_TOKEN", VALUE).unwrap();
    set(&d, &vault, KEY, "OTHER_TOKEN", "zzzz9999").unwrap();
    vault.delete(&account(KEY, "OTHER_TOKEN")).unwrap();
    assert_eq!(held_values(&d, &vault, KEY), vec![VALUE.to_string()]);
    assert!(held_values(&d, &vault, "another-repo").is_empty());
}
