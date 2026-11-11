use super::*;
use grokhub_agent::perm::ConfigGuard;
use grokhub_agent::{ContentPart, InputItem, Usage};
use std::path::PathBuf;

/// The cabin's own test shapes: none of these may reach the bundle.
const XAI_KEY: &str = concat!("xai-", "abcdefghijklmnopqrstuvwxyz012345");
const BEARER_VALUE: &str = "zyxwvutsrqponmlkjihgfedcba543210";
const BEARER: &str = concat!("Bearer ", "zyxwvutsrqponmlkjihgfedcba543210");
const PAIRING: &str = "pairing_token=PR-4Q7Z-91LM-XK20";
const AWS_KEY: &str = "AKIAIOSFODNN7EXAMPLE";
const GH_TOKEN: &str = concat!("ghp_", "abcdefghijklmnopqrstuvwxyz0123456789");
const PEM: &str =
    "-----BEGIN PRIVATE KEY-----\nMIIEvQIBADANBgkqhkiG9w0BAQEF\n-----END PRIVATE KEY-----";
const EMAIL: &str = "sam.example@fixture.test";
const HOME: &str = "/home/fixture-person";

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "grokhub-shell-diag-{name}-{}-{}",
        std::process::id(),
        grokhub_core::now_ms()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

fn say(role: &str, text: &str) -> InputItem {
    InputItem::Message {
        role: role.into(),
        content: vec![ContentPart::InputText(text.into())],
    }
}

/// A saved shell chat whose text is full of fake secrets.
fn seed_chat(config: &Path, id: &str) {
    let _guard = ConfigGuard::set(config);
    let user = format!(
        "my key is {XAI_KEY} and {PAIRING}, mail {EMAIL}, file {HOME}/notes/todo.txt\n{AWS_KEY} {GH_TOKEN}\n{PEM}"
    );
    let reply = format!("Saw {BEARER} at {HOME}/notes. Done.");
    grokhub_agent::record_turn(
        id,
        HOME,
        "grok-test",
        &[say("user", &user), say("assistant", &reply)],
        &Usage::default(),
        "",
        &user,
    )
    .expect("seed session");
}

fn every_text(export: &DiagnosticsExport) -> String {
    format!(
        "{}\n{}",
        diagnostics_markdown(export),
        serde_json::to_string(export).expect("json")
    )
}

const LEAKS: &[&str] = &[
    XAI_KEY,
    BEARER_VALUE,
    "PR-4Q7Z-91LM-XK20",
    AWS_KEY,
    GH_TOKEN,
    "MIIEvQIBADANBgkqhkiG9w0BAQEF",
    EMAIL,
    "fixture-person",
];

#[test]
fn a_bundle_of_a_chat_full_of_secrets_holds_none_of_them() {
    let config = temp_dir("leaks");
    seed_chat(&config, "shell-leaky");
    let export = build(&config, "1.0.0", true, Some("shell-leaky"), Some(HOME)).expect("bundle");
    assert_eq!(export.turns.len(), 1);
    let all = every_text(&export);
    for secret in LEAKS {
        assert!(!all.contains(secret), "{secret} leaked into the bundle");
    }
    // Redacted, not dropped: the turn is still there with its home path as ~-style /home/user.
    assert!(all.contains("/home/user/notes"), "{all}");
    assert!(export.turns[0].user_text.contains("my key is"));
}

#[test]
fn no_chat_text_goes_in_unless_a_chat_is_picked() {
    let config = temp_dir("nochat");
    seed_chat(&config, "shell-private");
    let export = build(&config, "1.0.0", false, None, Some(HOME)).expect("bundle");
    assert!(export.turns.is_empty());
    assert!(export.chat_id.is_empty() && export.chat_title.is_empty());
    let all = every_text(&export);
    assert!(!all.contains("my key is"), "{all}");
    assert!(all.contains("fail — xAI auth missing") || all.contains("xAI auth"));
}

#[test]
fn the_switches_are_the_cabins_app_json_keys_and_other_keys_stay() {
    let config = temp_dir("switches");
    std::fs::write(config.join("app.json"), r#"{"deviceName":"cabin"}"#).unwrap();
    assert!(!page(&config).mode);
    let saved = set(&config, true, true).expect("set");
    assert!(saved.mode && saved.include_logs);
    let raw: Value =
        serde_json::from_str(&std::fs::read_to_string(config.join("app.json")).unwrap()).unwrap();
    assert_eq!(raw["diagnosticsMode"], json!(true));
    assert_eq!(raw["diagnosticsIncludeLogs"], json!(true));
    assert_eq!(raw["deviceName"], json!("cabin"));
    assert_eq!(saved.preview, diagnostics_preview_lines(true));
    std::fs::write(config.join("app.json"), "not json").unwrap();
    assert!(set(&config, false, false).is_err());
    assert_eq!(
        std::fs::read_to_string(config.join("app.json")).unwrap(),
        "not json"
    );
}

#[test]
fn logs_go_in_only_when_ticked_and_are_redacted() {
    let config = temp_dir("logs");
    std::fs::write(
        config.join("cabin.log"),
        format!("started {XAI_KEY} in {HOME}/work\n"),
    )
    .unwrap();
    let off = build(&config, "1.0.0", true, None, Some(HOME)).expect("bundle");
    assert!(off.recent_logs.is_empty());
    set(&config, true, true).unwrap();
    let on = build(&config, "1.0.0", true, None, Some(HOME)).expect("bundle");
    assert!(on.recent_logs.contains("started"));
    assert!(!on.recent_logs.contains(XAI_KEY) && !on.recent_logs.contains("fixture-person"));
}

#[test]
fn preview_needs_the_mode_and_copy_summary_is_the_cabins_line() {
    let config = temp_dir("preview");
    seed_chat(&config, "shell-one");
    assert_eq!(
        preview_for(&config, "1.0.0", true, Some("shell-one"), Some(HOME)).unwrap_err(),
        MODE_OFF
    );
    set(&config, true, false).unwrap();
    let shown =
        preview_for(&config, "1.0.0", true, Some("shell-one"), Some(HOME)).expect("preview");
    assert_eq!(shown.turns, 1);
    assert_eq!(
        shown.summary,
        diagnostics_redaction_summary(&shown.markdown)
    );
    assert!(shown.summary.starts_with("Redacted: "), "{}", shown.summary);
    assert!(
        !shown.summary.starts_with("Redacted: 0 keys"),
        "{}",
        shown.summary
    );
    for secret in LEAKS {
        assert!(!shown.markdown.contains(secret), "{secret}");
    }
}

#[test]
fn save_writes_the_previewed_bundle_once_into_the_diagnostics_folder() {
    let config = temp_dir("save");
    let files = temp_dir("save-files");
    std::fs::write(
        config.join("folders.json"),
        serde_json::to_string(&json!({ "files_root": files.to_string_lossy() })).unwrap(),
    )
    .unwrap();
    seed_chat(&config, "shell-save");
    assert_eq!(save(&config).unwrap_err(), NOTHING_PENDING);
    set(&config, true, false).unwrap();
    preview_for(&config, "1.0.0", true, Some("shell-save"), Some(HOME)).expect("preview");
    let saved = save(&config).expect("save");
    let path = PathBuf::from(&saved.path);
    assert!(
        path.starts_with(files.join("Diagnostics")),
        "{}",
        saved.path
    );
    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(&bytes[..2], b"PK");
    assert!(
        saved.status.starts_with("Saved loadout-diagnostics-"),
        "{}",
        saved.status
    );
    assert_eq!(save(&config).unwrap_err(), NOTHING_PENDING);
}

#[test]
fn turning_the_mode_off_drops_a_bundle_waiting_for_save() {
    let config = temp_dir("off");
    seed_chat(&config, "shell-off");
    set(&config, true, false).unwrap();
    preview_for(&config, "1.0.0", true, Some("shell-off"), Some(HOME)).expect("preview");
    set(&config, false, false).unwrap();
    assert_eq!(save(&config).unwrap_err(), NOTHING_PENDING);
}

#[test]
fn asks_group_with_their_replies_and_tool_steps() {
    use crate::history::{Line, Role};
    let line = |role, text: &str| Line {
        role,
        text: text.into(),
    };
    let groups = group_turns(&[
        line(Role::User, "first"),
        line(Role::Tools, "read_file"),
        line(Role::Assistant, "one"),
        line(Role::Assistant, "two"),
        line(Role::User, "second"),
        line(Role::Assistant, "three"),
    ]);
    assert_eq!(groups.len(), 2);
    assert_eq!(
        (groups[0].turn, groups[0].raw_first, groups[0].raw_last),
        (1, 1, 1)
    );
    assert_eq!(groups[0].assistant, "one\n\ntwo");
    assert_eq!(groups[0].tools, vec!["read_file".to_string()]);
    assert_eq!((groups[1].turn, groups[1].user.as_str()), (2, "second"));
}

/// The repo's standard fake key.
const SK_KEY: &str = "sk-abcdefghijklmnopqrstuv";

#[test]
fn a_key_the_log_tail_starts_inside_is_still_hidden() {
    let config = temp_dir("logcut");
    // The last 4,000 characters start four characters into the key.
    let rest = format!(" accepted\n{}", "y".repeat(LOG_TAIL - 21 - 10));
    let log = format!("started\n{SK_KEY}{rest}");
    assert_eq!(
        tail_chars(&log, LOG_TAIL)
            .chars()
            .take(31)
            .collect::<String>(),
        "bcdefghijklmnopqrstuv accepted\n"
    );
    std::fs::write(config.join("cabin.log"), log).unwrap();
    set(&config, true, true).unwrap();
    let export = build(&config, "1.0.0", true, None, Some(HOME)).expect("bundle");
    assert!(
        export.recent_logs.contains(" accepted\n"),
        "{}",
        export.recent_logs
    );
    assert!(
        !export.recent_logs.contains("bcdefghijklmnopqrstuv"),
        "most of the key is in the bundle: {}",
        export.recent_logs.chars().take(60).collect::<String>()
    );
    assert!(export.recent_logs.chars().count() <= LOG_TAIL);
}

#[test]
fn a_key_a_step_result_or_error_is_cut_inside_is_still_hidden() {
    use grokhub_agent::harness::{append_span, AccessMode, Span};
    let config = temp_dir("stepcut");
    seed_chat(&config, "shell-cut");
    // The 160-character result cap and the 200-character error cap both fall
    // ten characters into the key.
    let result = format!("{}{SK_KEY}", "r".repeat(150));
    let claim = format!("error: {}{SK_KEY}", "c".repeat(183));
    let span = Span::soft_allow(
        "shell-cut",
        "run_command",
        "{}",
        &result,
        &claim,
        AccessMode::Supervised,
        "grok_build",
    )
    .in_turn("shell-cut", 1);
    append_span(&config, &span).expect("span");
    let export = build(&config, "1.0.0", true, Some("shell-cut"), Some(HOME)).expect("bundle");
    let turn = &export.turns[0];
    assert_eq!(turn.tool_calls.len(), 1);
    let summary = &turn.tool_calls[0].result_summary;
    assert!(summary.starts_with("rrrr"), "{summary}");
    assert!(!summary.contains("sk-abc"), "{summary}");
    assert_eq!(turn.errors.len(), 1);
    assert!(
        turn.errors[0].starts_with("error: cccc"),
        "{}",
        turn.errors[0]
    );
    assert!(!turn.errors[0].contains("sk-abc"), "{}", turn.errors[0]);
}

#[test]
fn a_turn_names_the_model_its_chat_was_saved_with_and_no_provider() {
    let config = temp_dir("model");
    seed_chat(&config, "shell-model");
    let export = build(&config, "1.0.0", true, Some("shell-model"), Some(HOME)).expect("bundle");
    assert_eq!(
        (
            export.turns[0].model.as_str(),
            export.turns[0].provider.as_str()
        ),
        ("grok-test", "")
    );
}
