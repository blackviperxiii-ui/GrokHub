use super::*;
use std::cell::RefCell;
use std::collections::HashMap;

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

const KEY: &str = "repo-key-9";
const VALUE: &str = "abcd1234XYZ";

fn dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!(
        "code-deploy-{tag}-{}-{}",
        std::process::id(),
        grokhub_core::now_ms()
    ));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn ship(mode: ShipMode) -> CodeShip {
    CodeShip {
        mode,
        ..CodeShip::default()
    }
}

fn place() -> Where {
    Where {
        host: "github".into(),
        branch: "code/deploy".into(),
    }
}

fn on(config: &Path, command: &str) {
    save_settings(
        config,
        KEY,
        Settings {
            deploy_enabled: true,
            command: command.into(),
        },
    )
    .unwrap();
}

// Only the unix-gated secret test calls this; a windows variant would be dead code
// under the workspace clippy run.
#[cfg(unix)]
fn echo_secret() -> String {
    "echo value=$DEPLOY_TOKEN; echo more=$DEPLOY_TOKEN >&2".into()
}

fn go(
    config: &Path,
    work: &Path,
    mode: ShipMode,
    command: &str,
    confirmed: bool,
    vault: &MockVault,
) -> Result<RunResult, String> {
    run_with(
        config,
        KEY,
        work,
        &ship(mode),
        command,
        confirmed,
        vault,
        &place(),
        |_| {},
    )
}

#[test]
fn deploy_is_off_by_default_with_no_command() {
    let config = dir("default");
    assert_eq!(
        settings(&config, KEY),
        Settings {
            deploy_enabled: false,
            command: String::new()
        }
    );
    let plan = plan_for_key(&config, KEY, &ship(ShipMode::Ask)).unwrap();
    assert!(!plan.deploy_enabled);
    assert_eq!(plan.blocked.as_deref(), Some(DISABLED));
    let work = dir("default-work");
    let err = go(
        &config,
        &work,
        ShipMode::Ask,
        "echo hi",
        true,
        &MockVault::default(),
    )
    .unwrap_err();
    assert_eq!(err, DISABLED);
}

#[test]
fn turning_deploy_on_needs_a_command() {
    let config = dir("needs-command");
    let err = save_settings(
        &config,
        KEY,
        Settings {
            deploy_enabled: true,
            command: "   ".into(),
        },
    )
    .unwrap_err();
    assert_eq!(err, NO_COMMAND);
}

#[test]
fn settings_are_per_project() {
    let config = dir("per-project");
    on(&config, "npm run deploy");
    assert_eq!(settings(&config, KEY).command, "npm run deploy");
    assert_eq!(settings(&config, "other-repo"), Settings::default());
}

#[test]
fn ask_mode_asks_every_time_even_when_remembered() {
    let config = dir("ask");
    let work = dir("ask-work");
    let vault = MockVault::default();
    on(&config, "echo deployed");
    assert_eq!(
        go(
            &config,
            &work,
            ShipMode::Ask,
            "echo deployed",
            false,
            &vault
        )
        .unwrap_err(),
        NEEDS_CONFIRM
    );
    let done = go(&config, &work, ShipMode::Ask, "echo deployed", true, &vault).unwrap();
    assert_eq!(done.exit, 0);
    assert!(done.output.contains("deployed"), "{}", done.output);
    // Remembered now, but Ask still asks.
    assert_eq!(
        go(
            &config,
            &work,
            ShipMode::Ask,
            "echo deployed",
            false,
            &vault
        )
        .unwrap_err(),
        NEEDS_CONFIRM
    );
    assert!(
        plan_for_key(&config, KEY, &ship(ShipMode::Ask))
            .unwrap()
            .needs_confirm
    );
}

#[test]
fn auto_beta_runs_a_remembered_deploy_without_asking_and_asks_when_the_command_changes() {
    let config = dir("auto");
    let work = dir("auto-work");
    let vault = MockVault::default();
    on(&config, "echo one");
    // First run asks, in Auto-beta too.
    assert_eq!(
        go(
            &config,
            &work,
            ShipMode::AutoBeta,
            "echo one",
            false,
            &vault
        )
        .unwrap_err(),
        NEEDS_CONFIRM
    );
    go(&config, &work, ShipMode::AutoBeta, "echo one", true, &vault).unwrap();
    let again = go(
        &config,
        &work,
        ShipMode::AutoBeta,
        "echo one",
        false,
        &vault,
    )
    .unwrap();
    assert!(again.output.contains("one"), "{}", again.output);
    assert!(
        !plan_for_key(&config, KEY, &ship(ShipMode::AutoBeta))
            .unwrap()
            .needs_confirm
    );
    // The command text changes: asks again.
    on(&config, "echo two");
    assert!(
        plan_for_key(&config, KEY, &ship(ShipMode::AutoBeta))
            .unwrap()
            .needs_confirm
    );
    assert_eq!(
        go(
            &config,
            &work,
            ShipMode::AutoBeta,
            "echo two",
            false,
            &vault
        )
        .unwrap_err(),
        NEEDS_CONFIRM
    );
}

#[test]
fn forget_clears_the_remembered_deploy() {
    let config = dir("forget");
    let work = dir("forget-work");
    let vault = MockVault::default();
    on(&config, "echo one");
    go(&config, &work, ShipMode::AutoBeta, "echo one", true, &vault).unwrap();
    assert_eq!(
        plan_for_key(&config, KEY, &ship(ShipMode::AutoBeta))
            .unwrap()
            .remembered
            .as_deref(),
        Some("echo one")
    );
    forget_for_key(&config, KEY).unwrap();
    let plan = plan_for_key(&config, KEY, &ship(ShipMode::AutoBeta)).unwrap();
    assert_eq!(plan.remembered, None);
    assert!(plan.needs_confirm);
    assert_eq!(
        go(
            &config,
            &work,
            ShipMode::AutoBeta,
            "echo one",
            false,
            &vault
        )
        .unwrap_err(),
        NEEDS_CONFIRM
    );
}

#[test]
fn a_command_that_changed_since_it_was_shown_does_not_run() {
    let config = dir("stale");
    let work = dir("stale-work");
    on(&config, "echo new");
    let err = go(
        &config,
        &work,
        ShipMode::Ask,
        "echo old",
        true,
        &MockVault::default(),
    )
    .unwrap_err();
    assert_eq!(err, CHANGED);
}

#[cfg(unix)]
#[test]
fn a_secret_value_never_shows_in_the_deploy_output_the_chunks_or_the_activity_log() {
    let config = dir("secret");
    let work = dir("secret-work");
    let vault = MockVault::default();
    code_secrets::set_logged(&config, &vault, KEY, "DEPLOY_TOKEN", VALUE, 100).unwrap();
    let command = echo_secret();
    on(&config, &command);
    let mut chunks = String::new();
    let done = run_with(
        &config,
        KEY,
        &work,
        &ship(ShipMode::Ask),
        &command,
        true,
        &vault,
        &place(),
        |c| chunks.push_str(c),
    )
    .unwrap();
    assert_eq!(done.exit, 0);
    // The child really received the value (it echoed something), and it came back hidden.
    assert!(done.output.contains("value=[redacted]"), "{}", done.output);
    assert!(done.output.contains("more=[redacted]"), "{}", done.output);
    assert!(!done.output.contains(VALUE), "{}", done.output);
    assert!(!chunks.contains(VALUE), "{chunks}");
    let log = std::fs::read_to_string(config.join(code_activity::FILE)).unwrap();
    assert!(!log.contains(VALUE), "{log}");
    // The deploy line is there, and so is the secret add, by name.
    assert!(log.contains("\"kind\":\"deploy\""), "{log}");
    assert!(log.contains("Deploy finished."), "{log}");
    assert!(log.contains("Added secret DEPLOY_TOKEN."), "{log}");
    // Nothing on disk in the config folder holds the value.
    for entry in std::fs::read_dir(&config).unwrap() {
        let text = std::fs::read_to_string(entry.unwrap().path()).unwrap_or_default();
        assert!(!text.contains(VALUE), "{text}");
    }
}

#[cfg(unix)]
#[test]
fn a_secret_in_the_command_text_is_hidden_in_the_activity_log() {
    let config = dir("secret-cmd");
    let work = dir("secret-cmd-work");
    let vault = MockVault::default();
    code_secrets::set_logged(&config, &vault, KEY, "DEPLOY_TOKEN", VALUE, 100).unwrap();
    let command = format!("echo --token={VALUE}");
    on(&config, &command);
    let done = go(&config, &work, ShipMode::Ask, &command, true, &vault).unwrap();
    assert_eq!(done.command, "echo --token=[redacted]");
    assert_eq!(done.output.trim(), "--token=[redacted]");
    let log = std::fs::read_to_string(config.join(code_activity::FILE)).unwrap();
    assert!(!log.contains(VALUE), "{log}");
}

#[cfg(unix)]
#[test]
fn a_failed_deploy_is_logged_with_its_exit_code() {
    let config = dir("fail");
    let work = dir("fail-work");
    on(&config, "exit 3");
    let done = go(
        &config,
        &work,
        ShipMode::Ask,
        "exit 3",
        true,
        &MockVault::default(),
    )
    .unwrap();
    assert_eq!(done.exit, 3);
    let rows = code_activity::list(&config, 10);
    assert_eq!(rows[0].kind, ActKind::Deploy);
    assert_eq!(rows[0].exit, Some(3));
    assert_eq!(rows[0].summary, "Deploy exited with 3.");
    assert_eq!(rows[0].approved_by, "user");
    assert_eq!(rows[0].branch, "code/deploy");
}

#[test]
fn a_listed_secret_with_no_value_stops_the_deploy_before_it_runs() {
    let config = dir("missing");
    let work = dir("missing-work");
    let vault = MockVault::default();
    code_secrets::set(&config, &vault, KEY, "DEPLOY_TOKEN", VALUE).unwrap();
    vault
        .delete(&code_secrets::account(KEY, "DEPLOY_TOKEN"))
        .unwrap();
    on(&config, "echo hi");
    let err = go(&config, &work, ShipMode::Ask, "echo hi", true, &vault).unwrap_err();
    assert_eq!(
        err,
        "DEPLOY_TOKEN has no value in the keychain. Set it again in Settings → Code."
    );
    // Not remembered, not logged: nothing ran.
    assert_eq!(
        code_runs::remembered(&config, Kind::Deploy, KEY).unwrap(),
        None
    );
    assert!(code_activity::list(&config, 10).is_empty());
}

#[test]
fn secret_add_replace_and_delete_are_logged_by_name_only() {
    let config = dir("secret-log");
    let vault = MockVault::default();
    code_secrets::set_logged(&config, &vault, KEY, "DEPLOY_TOKEN", VALUE, 100).unwrap();
    code_secrets::set_logged(&config, &vault, KEY, "DEPLOY_TOKEN", "zzzz9999QQQ", 100).unwrap();
    code_secrets::delete_logged(&config, &vault, KEY, "DEPLOY_TOKEN", true, 100).unwrap();
    let rows = code_activity::list(&config, 10);
    let summaries: Vec<&str> = rows.iter().map(|r| r.summary.as_str()).collect();
    assert_eq!(
        summaries,
        vec![
            "Deleted secret DEPLOY_TOKEN.",
            "Replaced the value of secret DEPLOY_TOKEN.",
            "Added secret DEPLOY_TOKEN.",
        ]
    );
    assert!(rows.iter().all(|r| r.kind == ActKind::Secret));
    let log = std::fs::read_to_string(config.join(code_activity::FILE)).unwrap();
    assert!(
        !log.contains(VALUE) && !log.contains("zzzz9999QQQ"),
        "{log}"
    );
    // A refused delete logs nothing.
    let before = code_activity::list(&config, 10).len();
    assert!(code_secrets::delete_logged(&config, &vault, KEY, "DEPLOY_TOKEN", false, 100).is_err());
    assert_eq!(code_activity::list(&config, 10).len(), before);
}

#[test]
fn the_shield_hides_a_value_split_across_two_reads() {
    let mut shield = code_secrets::Shield::new(vec![VALUE.to_string()]);
    let mut out = String::new();
    out.push_str(&shield.push("token=abcd12"));
    out.push_str(&shield.push("34XYZ ok\nnext line\n"));
    out.push_str(&shield.push("tail abcd"));
    out.push_str(&shield.finish());
    assert_eq!(out, "token=[redacted] ok\nnext line\ntail abcd");
}

#[test]
fn the_shield_with_nothing_to_hide_passes_text_straight_through() {
    let mut shield = code_secrets::Shield::new(Vec::new());
    assert_eq!(shield.push("no newline yet"), "no newline yet");
    assert_eq!(shield.finish(), "");
}

#[test]
fn the_shield_lets_a_very_long_line_out_without_splitting_a_value() {
    let mut shield = code_secrets::Shield::new(vec![VALUE.to_string()]);
    let long = "a".repeat(40_000);
    let mut out = shield.push(&long);
    out.push_str(&shield.push(&format!("{VALUE} end")));
    out.push_str(&shield.finish());
    assert!(!out.contains(VALUE));
    assert_eq!(out, format!("{long}[redacted] end"));
}
