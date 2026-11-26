use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use grokhub_core::model_registry::{
    Capability, Credential, ModelMeta, ModelRecord, ModelState, Prices, Registry, SourceKind,
};

use super::*;
use crate::gate::Decision;
use crate::route::local::{LocalCapacity, LocalGate};
use crate::route::local_repeat::REPEAT_NOTE;
use crate::route::table::RoutingTable;
use crate::route::{RouteInput, Router};

fn scratch_dir(label: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/code-panel-tests")
        .join(format!(
            "{label}-{}-{}",
            std::process::id(),
            grokhub_core::now_ms()
        ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("create test dir");
    dir
}

fn git_in(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .expect("run git");
    assert!(
        out.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn init_git_repo(dir: &Path, branch: &str) {
    git_in(dir, &["init", "-b", branch]);
    git_in(dir, &["config", "user.name", "Test User"]);
    git_in(dir, &["config", "user.email", "test@example.com"]);
    fs::write(dir.join("app.txt"), "initial content\n").expect("write app.txt");
    git_in(dir, &["add", "."]);
    git_in(dir, &["commit", "-m", "initial"]);
}

fn test_registry() -> Registry {
    let mut reg = Registry::default();
    reg.entitlement.credential = Credential::ApiKey;
    reg.models.insert(
        "grok-4.7".into(),
        ModelRecord {
            id: "grok-4.7".into(),
            state: ModelState::Available,
            meta: ModelMeta {
                source: SourceKind::Xai,
                context_length: Some(500_000),
                capabilities: vec![
                    Capability::Text,
                    Capability::Tools,
                    Capability::Reasoning,
                    Capability::Vision,
                ],
                prices: Prices {
                    input_usd_per_m: Some(2.0),
                    output_usd_per_m: Some(10.0),
                    cached_input_usd_per_m: Some(0.5),
                },
                ..ModelMeta::default()
            },
            ..ModelRecord::default()
        },
    );
    reg.models.insert(
        "local:small".into(),
        ModelRecord {
            id: "local:small".into(),
            state: ModelState::Available,
            meta: ModelMeta {
                source: SourceKind::Local,
                context_length: Some(32_768),
                capabilities: vec![Capability::Text, Capability::Tools],
                ..ModelMeta::default()
            },
            ..ModelRecord::default()
        },
    );
    reg
}

#[test]
fn defaults_are_off_and_saved_independently_per_project() {
    let root = scratch_dir("per-project");
    let cfg = root.join("config");
    let proj_a = root.join("proj-a");
    let proj_b = root.join("proj-b");
    fs::create_dir_all(&proj_a).unwrap();
    fs::create_dir_all(&proj_b).unwrap();

    // Both toggles are off by default for every project.
    let def_a = load_settings(&cfg, &proj_a);
    assert!(!def_a.local_only_nonstop);
    assert!(!def_a.backup_before_risky);
    assert_eq!(def_a.max_repeated_failures, 3);
    assert_eq!(def_a.stall_timeout_mins, 15);
    assert_eq!(def_a.backup_retention_warn, 20);

    // Turning both on for project A leaves project B untouched.
    let saved = save_settings(
        &cfg,
        &proj_a,
        PanelSettings {
            local_only_nonstop: true,
            backup_before_risky: true,
            max_repeated_failures: 4,
            stall_timeout_mins: 10,
            backup_retention_warn: 5,
        },
    )
    .unwrap();
    assert!(saved.local_only_nonstop);
    assert!(saved.backup_before_risky);
    assert_eq!(load_settings(&cfg, &proj_a), saved);
    assert_eq!(load_settings(&cfg, &proj_b), PanelSettings::default());
}

#[test]
fn ac1_local_only_routes_strictly_local_and_never_escalates_to_frontier() {
    let reg = test_registry();
    let profiles = BTreeMap::new();
    let table = RoutingTable::default();
    let mut input = RouteInput {
        provider: "xai",
        class: "chat:default",
        current_model: "grok-4.7",
        d: 0.95,
        ctx_tokens: 8_000,
        needs_tools: true,
        needs_image: true,
        local: LocalGate {
            enabled: true,
            tier: "local:small".into(),
            local_verify_failed: true,
            capacity: LocalCapacity {
                ready: true,
                level: grokhub_core::local_governor::Level::Normal,
            },
            ..LocalGate::default()
        },
        code_local_only: true,
        text: "refactor the entire compiler and think very hard",
        ..RouteInput::default()
    };

    // Even with high difficulty, verify failure, tools, and image, router picks local:small.
    let route = Router::choose(&input, &reg, &profiles, &table, 1_000);
    assert!(!route.no_route);
    assert_eq!(route.model, "local:small");
    assert!(route.rule_ids.iter().any(|r| r == "code:local_only"));

    // If no local model is ready, it pauses instead of escalating to grok-4.7.
    input.local.enabled = false;
    input.local.capacity.ready = false;
    let paused = Router::choose(&input, &reg, &profiles, &table, 1_000);
    assert!(paused.no_route);
    assert!(paused.rule_ids.iter().any(|r| r == "code:no_local"));
}

#[test]
fn ac3_checkpoint_commits_on_work_branch_and_never_commits_on_main() {
    let root = scratch_dir("ac3-checkpoint");
    let cfg = root.join("config");
    let repo = root.join("repo");
    fs::create_dir_all(&repo).unwrap();
    init_git_repo(&repo, "main");

    // On `main`, a checkpoint MUST NOT create a commit on `main`.
    let head_on_main = git_in(&repo, &["rev-parse", "HEAD"]);
    fs::write(repo.join("app.txt"), "dirty on main\n").unwrap();
    let main_entry = checkpoint_before_risky_step(
        &cfg,
        &repo,
        RiskyActionKind::Overwrite,
        "Before overwrite: write app.txt",
    )
    .unwrap();
    assert_eq!(git_in(&repo, &["rev-parse", "HEAD"]), head_on_main);
    assert_eq!(main_entry.strategy, BackupStrategy::GitStash);
    // Working tree stayed intact for the upcoming action.
    assert_eq!(fs::read_to_string(repo.join("app.txt")).unwrap(), "dirty on main\n");

    // Switch to a work branch: checkpoint commits directly on the work branch.
    git_in(&repo, &["checkout", "-b", "work/feature-1"]);
    let head_before = git_in(&repo, &["rev-parse", "HEAD"]);
    let work_entry = checkpoint_before_risky_step(
        &cfg,
        &repo,
        RiskyActionKind::Overwrite,
        "Before overwrite: write app.txt",
    )
    .unwrap();
    assert_eq!(work_entry.strategy, BackupStrategy::GitCommit);
    assert_eq!(work_entry.branch.as_deref(), Some("work/feature-1"));
    assert_ne!(git_in(&repo, &["rev-parse", "HEAD"]), head_before);
    assert_eq!(work_entry.ref_value, git_in(&repo, &["rev-parse", "HEAD"]));
}

#[test]
fn ac4_nonstop_guard_stops_on_repeated_failures_stall_timer_and_repetition_cut() {
    let settings = PanelSettings {
        local_only_nonstop: true,
        max_repeated_failures: 3,
        stall_timeout_mins: 15,
        ..PanelSettings::default()
    };

    // 1. Stops after 3 repeated failures of the same step with why + where card.
    let mut guard = NonstopGuard::new(&settings, 10_000);
    assert_eq!(
        guard.note_call(
            "run_terminal_command\ncargo test",
            "run_terminal_command cargo test",
            Did::Nothing,
            true,
            "error[E0308]: mismatched types",
            11_000
        ),
        None
    );
    assert_eq!(
        guard.note_call(
            "run_terminal_command\ncargo test",
            "run_terminal_command cargo test",
            Did::Nothing,
            true,
            "error[E0308]: mismatched types",
            12_000
        ),
        None
    );
    let stop = guard
        .note_call(
            "run_terminal_command\ncargo test",
            "run_terminal_command cargo test",
            Did::Nothing,
            true,
            "error[E0308]: mismatched types",
            13_000,
        )
        .expect("3rd failure must stop");
    assert_eq!(
        stop,
        NonstopStopReason::RepeatedFailure {
            step: "run_terminal_command cargo test".into(),
            count: 3,
            limit: 3,
            last_error: "error[E0308]: mismatched types".into(),
        }
    );
    let card = stop.status_card_body();
    assert!(card.contains("3 repeated failures"));
    assert!(card.contains("Where: run_terminal_command cargo test"));
    assert!(card.contains("mismatched types"));

    // 2. Stops after 15 minutes with no progress.
    let timer_guard = NonstopGuard::new(&settings, 1_000_000);
    assert_eq!(timer_guard.check_clock("task-2: fix parser", 1_000_000 + 14 * 60_000), None);
    let stall_stop = timer_guard
        .check_clock("task-2: fix parser", 1_000_000 + 15 * 60_000)
        .expect("15 min without progress must stop");
    assert_eq!(
        stall_stop,
        NonstopStopReason::NoProgress {
            elapsed_mins: 15,
            limit_mins: 15,
            where_at: "task-2: fix parser".into(),
        }
    );
    assert!(stall_stop
        .status_card_body()
        .contains("15 minutes with no progress"));
    assert!(stall_stop
        .status_card_body()
        .contains("Where: task-2: fix parser"));

    // 3. Stops on a repetition-guard cut.
    let cut_reply = format!("done done done{REPEAT_NOTE}");
    let rep_stop = timer_guard
        .check_repetition_cut(&cut_reply, "task-3: write tests")
        .expect("repetition note must stop");
    assert_eq!(
        rep_stop,
        NonstopStopReason::RepetitionCut {
            where_at: "task-3: write tests".into(),
        }
    );
    assert!(rep_stop.status_card_body().contains("repetition guard"));
    assert!(rep_stop
        .status_card_body()
        .contains("Where: task-3: write tests"));
}

#[test]
fn ac5_dry_run_delete_creates_backup_before_tool_runs_and_classifies_all_risky_kinds() {
    let root = scratch_dir("ac5-dry-run-delete");
    let cfg = root.join("config");
    let folder = root.join("plain-project");
    fs::create_dir_all(&folder).unwrap();
    let victim = folder.join("important.txt");
    fs::write(&victim, "precious data v1\n").unwrap();

    // Classify every AC-5 risky category with literal inputs.
    let empty_edits = HashSet::new();
    assert_eq!(
        classify_risky_action(
            "run_terminal_command",
            r#"{"command":"rm important.txt"}"#,
            &folder,
            &empty_edits
        ),
        Some(RiskyActionKind::Delete)
    );
    assert_eq!(
        classify_risky_action(
            "write",
            r#"{"path":"important.txt","content":"new"}"#,
            &folder,
            &empty_edits
        ),
        Some(RiskyActionKind::Overwrite)
    );
    let mut two_edits = HashSet::new();
    two_edits.insert(folder.join("a.rs").to_string_lossy().replace('\\', "/"));
    two_edits.insert(folder.join("b.rs").to_string_lossy().replace('\\', "/"));
    assert_eq!(
        classify_risky_action(
            "search_replace",
            r#"{"file_path":"c.rs","old_string":"x","new_string":"y"}"#,
            &folder,
            &two_edits
        ),
        Some(RiskyActionKind::MassEdit)
    );
    assert_eq!(
        classify_risky_action(
            "run_terminal_command",
            r#"{"command":"git reset --hard HEAD"}"#,
            &folder,
            &empty_edits
        ),
        Some(RiskyActionKind::GitDiscard)
    );
    assert_eq!(
        classify_risky_action(
            "run_terminal_command",
            r#"{"command":"git clean -fd"}"#,
            &folder,
            &empty_edits
        ),
        Some(RiskyActionKind::GitDiscard)
    );
    assert_eq!(
        classify_risky_action(
            "run_terminal_command",
            r#"{"command":"git checkout -- important.txt"}"#,
            &folder,
            &empty_edits
        ),
        Some(RiskyActionKind::GitDiscard)
    );
    assert_eq!(
        classify_risky_action(
            "run_terminal_command",
            r#"{"command":"npm install lodash"}"#,
            &folder,
            &empty_edits
        ),
        Some(RiskyActionKind::PackageInstall)
    );

    // Dry-run: create backup BEFORE running the simulated delete tool, verify
    // the backup exists with original content while the file is still there,
    // then run the delete and restore via AC-6 one-click restore!
    let mut timeline: Vec<&str> = Vec::new();
    let kind = classify_risky_action(
        "run_terminal_command",
        r#"{"command":"rm important.txt"}"#,
        &folder,
        &empty_edits,
    )
    .unwrap();
    let label = action_label(
        kind,
        "run_terminal_command",
        r#"{"command":"rm important.txt"}"#,
    );
    let backup = create_backup(&cfg, &folder, kind, &label).unwrap();
    timeline.push("backup_created");
    assert_eq!(backup.strategy, BackupStrategy::FolderCopy);
    assert!(backup.ref_value.starts_with(".loadout-backups/"));
    let backup_copy = folder.join(&backup.ref_value).join("important.txt");
    assert!(backup_copy.is_file(), "backup copy must exist before delete");
    assert_eq!(fs::read_to_string(&backup_copy).unwrap(), "precious data v1\n");
    assert!(victim.is_file(), "victim file must still exist when backup finishes");

    // Now the delete tool runs.
    fs::remove_file(&victim).unwrap();
    timeline.push("delete_ran");
    assert_eq!(timeline, vec!["backup_created", "delete_ran"]);
    assert!(!victim.exists());

    // AC-6: Restore recovers the deleted file AND keeps the backup intact.
    let restored = restore_backup(&cfg, &folder, &backup.id).unwrap();
    assert_eq!(restored.id, backup.id);
    assert_eq!(restored.restored_files, 1);
    assert_eq!(fs::read_to_string(&victim).unwrap(), "precious data v1\n");
    assert!(backup_copy.is_file(), "restore must never delete the backup");
}

#[test]
fn ac6_retention_limit_only_warns_and_never_auto_deletes_backups() {
    let root = scratch_dir("ac6-retention");
    let cfg = root.join("config");
    let folder = root.join("proj");
    fs::create_dir_all(&folder).unwrap();
    fs::write(folder.join("note.txt"), "hello\n").unwrap();

    save_settings(
        &cfg,
        &folder,
        PanelSettings {
            backup_before_risky: true,
            backup_retention_warn: 2,
            ..PanelSettings::default()
        },
    )
    .unwrap();

    let b1 = create_backup(&cfg, &folder, RiskyActionKind::Overwrite, "backup 1").unwrap();
    let b2 = create_backup(&cfg, &folder, RiskyActionKind::Overwrite, "backup 2").unwrap();
    assert_eq!(list_backups(&cfg, &folder).retention_warning, None);

    let b3 = create_backup(&cfg, &folder, RiskyActionKind::Overwrite, "backup 3").unwrap();
    let listed = list_backups(&cfg, &folder);
    assert_eq!(listed.backups.len(), 3, "old backups must never be auto-deleted");
    let warn = listed
        .retention_warning
        .expect("exceeding threshold 2 must show warning");
    assert!(warn.contains("3 saved backups"));
    assert!(warn.contains("never auto-deleted"));
    assert!(folder.join(&b1.ref_value).is_dir());
    assert!(folder.join(&b2.ref_value).is_dir());
    assert!(folder.join(&b3.ref_value).is_dir());
}

#[test]
fn ac7_hard_class_and_dangerous_commands_still_ask_with_both_toggles_and_always_on() {
    let _scope = TestSessionScope::enter();
    let root = scratch_dir("ac7-hard-and-dangerous");
    let cfg = root.join("config");
    let ws = root.join("proj");
    fs::create_dir_all(&ws).unwrap();

    save_settings(
        &cfg,
        &ws,
        PanelSettings {
            local_only_nonstop: true,
            backup_before_risky: true,
            ..PanelSettings::default()
        },
    )
    .unwrap();
    bind_session("code-chat-ac7", &cfg, Some(&ws));

    // Hard-class delete (`rm`) STILL asks even if base were Run.
    assert_eq!(
        crate::code_guard::apply(
            "code-chat-ac7",
            "run_terminal_command",
            r#"{"command":"rm notes.txt"}"#,
            &ws,
            None,
            Decision::Run,
        ),
        Decision::Ask
    );
    // Hard-class ship (`git push`) STILL asks.
    assert_eq!(
        crate::code_guard::apply(
            "code-chat-ac7",
            "run_terminal_command",
            r#"{"command":"git push origin feat"}"#,
            &ws,
            None,
            Decision::Run,
        ),
        Decision::Ask
    );
    // Dangerous command (`chmod 777`) STILL asks under Allow always + both toggles on.
    assert_eq!(
        crate::code_guard::apply(
            "code-chat-ac7",
            "run_terminal_command",
            r#"{"command":"chmod 777 script.sh"}"#,
            &ws,
            None,
            Decision::Run,
        ),
        Decision::Ask
    );
    // Dangerous subshell (`sh -c ...`) STILL asks under Allow always + both toggles on.
    assert_eq!(
        crate::code_guard::apply(
            "code-chat-ac7",
            "run_terminal_command",
            r#"{"command":"sh -c 'echo hi'"}"#,
            &ws,
            None,
            Decision::Run,
        ),
        Decision::Ask
    );
    // Non-hard, non-dangerous risky action (e.g. `npm install`) continues as Run under Allow always.
    assert_eq!(
        crate::code_guard::apply(
            "code-chat-ac7",
            "run_terminal_command",
            r#"{"command":"npm install lodash"}"#,
            &ws,
            None,
            Decision::Run,
        ),
        Decision::Run
    );
}

#[test]
fn ac5_and_ac6_git_repo_backup_and_one_click_restore_preserves_backup() {
    let root = scratch_dir("ac5-ac6-git");
    let cfg = root.join("config");
    let repo = root.join("repo");
    fs::create_dir_all(&repo).unwrap();
    init_git_repo(&repo, "main");
    git_in(&repo, &["checkout", "-b", "work/refactor"]);

    // Modify tracked file and create an untracked file before risky action.
    fs::write(repo.join("app.txt"), "work in progress\n").unwrap();
    fs::write(repo.join("extra.txt"), "untracked note\n").unwrap();

    let backup = create_backup(
        &cfg,
        &repo,
        RiskyActionKind::GitDiscard,
        "Before git reset: git reset --hard HEAD",
    )
    .unwrap();
    assert_eq!(backup.strategy, BackupStrategy::GitStash);

    // Simulate destructive action wiping the working tree.
    git_in(&repo, &["reset", "--hard", "HEAD"]);
    let _ = fs::remove_file(repo.join("extra.txt"));
    assert_eq!(
        fs::read_to_string(repo.join("app.txt")).unwrap(),
        "initial content\n"
    );
    assert!(!repo.join("extra.txt").exists());

    // One-click restore brings both files back and leaves the backup listed.
    let res = restore_backup(&cfg, &repo, &backup.id).unwrap();
    assert_eq!(res.id, backup.id);
    assert_eq!(
        fs::read_to_string(repo.join("app.txt")).unwrap(),
        "work in progress\n"
    );
    assert_eq!(
        fs::read_to_string(repo.join("extra.txt")).unwrap(),
        "untracked note\n"
    );
    let listed = list_backups(&cfg, &repo);
    assert_eq!(listed.backups.len(), 1);
    assert!(listed.backups[0].restored_at_ms.is_some());
}

