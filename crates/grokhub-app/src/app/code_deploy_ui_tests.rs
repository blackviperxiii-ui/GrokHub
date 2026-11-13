use super::*;
use grokhub_code::activity::{self, Kind as ActKind};
use grokhub_code::runs::{self, Kind};
use grokhub_code::secrets;
use std::collections::HashMap;
use std::sync::Mutex;

/// Secret values in memory, as the keychain would hold them.
#[derive(Default)]
struct MemVault {
    items: Mutex<HashMap<String, String>>,
}

impl Vault for MemVault {
    fn get(&self, account: &str) -> Result<Option<String>, String> {
        Ok(self.items.lock().unwrap().get(account).cloned())
    }
    fn set(&self, account: &str, value: &str) -> Result<(), String> {
        self.items
            .lock()
            .unwrap()
            .insert(account.to_string(), value.to_string());
        Ok(())
    }
    fn delete(&self, account: &str) -> Result<(), String> {
        self.items.lock().unwrap().remove(account);
        Ok(())
    }
}

const KEY: &str = "cabin-repo-key-4";
const VALUE: &str = "s3cr3tDeployValue77";

fn dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!(
        "cabin-code-deploy-{tag}-{}-{}",
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

fn turn_on(config: &Path, command: &str) {
    deploy::save_settings(
        config,
        KEY,
        Settings {
            deploy_enabled: true,
            command: command.into(),
        },
    )
    .unwrap();
}

fn plan(config: &Path, mode: ShipMode) -> Plan {
    deploy::plan_for_key(config, KEY, &ship(mode)).unwrap()
}

#[cfg(windows)]
fn echo_secret() -> &'static str {
    "echo value=%DEPLOY_TOKEN%"
}

#[cfg(not(windows))]
fn echo_secret() -> &'static str {
    "echo value=$DEPLOY_TOKEN; echo again=$DEPLOY_TOKEN >&2"
}

/// Pump the panel until its worker is done (or 20 s pass).
fn drain(panel: &mut DeployPanel) {
    let until = std::time::Instant::now() + std::time::Duration::from_secs(20);
    while !panel.pump() {
        assert!(
            std::time::Instant::now() < until,
            "the deploy worker never finished"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

#[test]
fn deploy_is_off_until_turned_on_and_press_runs_nothing() {
    let config = dir("off");
    let p = plan(&config, ShipMode::AutoBeta);
    assert!(!p.deploy_enabled);
    assert_eq!(p.command, "");
    assert_eq!(p.blocked.as_deref(), Some(deploy::DISABLED));
    let mut panel = DeployPanel::default();
    assert_eq!(panel.press(&p), None);
    assert_eq!(panel.confirming, None);
    assert_eq!(
        panel.note.as_deref(),
        Some("Deploy is off for this project. Turn it on below.")
    );
    assert_eq!(
        mode_line(Some(&p)),
        "Deploy is off for this project. Turn it on below."
    );
}

#[test]
fn an_empty_command_cannot_be_turned_on() {
    let config = dir("empty");
    let err = deploy::save_settings(
        &config,
        KEY,
        Settings {
            deploy_enabled: true,
            command: "   ".into(),
        },
    )
    .unwrap_err();
    assert_eq!(err, deploy::NO_COMMAND);
    let mut panel = DeployPanel {
        loading: true,
        ..DeployPanel::default()
    };
    panel.apply(Msg::Saved(Err(err)));
    assert!(!panel.loading);
    assert_eq!(
        panel.settings_note.as_deref(),
        Some("Type a deploy command and save it, then turn deploy on.")
    );
    assert!(!plan(&config, ShipMode::Ask).deploy_enabled);
}

#[test]
fn an_unremembered_deploy_asks_first_in_both_modes() {
    let config = dir("asks");
    turn_on(&config, "  npm run deploy  ");
    for mode in [ShipMode::Ask, ShipMode::AutoBeta] {
        let p = plan(&config, mode);
        assert_eq!(p.command, "npm run deploy");
        assert!(p.needs_confirm);
        let mut panel = DeployPanel::default();
        assert_eq!(panel.press(&p), None);
        assert_eq!(panel.confirming.as_deref(), Some("npm run deploy"));
        assert!(!panel.busy);
    }
}

#[test]
fn auto_beta_runs_a_remembered_command_and_asks_again_when_it_changes() {
    let config = dir("auto");
    turn_on(&config, "make deploy");
    runs::remember(&config, Kind::Deploy, KEY, "make deploy").unwrap();
    let p = plan(&config, ShipMode::AutoBeta);
    assert!(!p.needs_confirm);
    assert_eq!(p.remembered.as_deref(), Some("make deploy"));
    assert_eq!(p.mode, "autoBeta");
    assert_eq!(start_step(&p), Start::Run("make deploy".into()));

    // Ask mode still confirms a remembered command.
    let ask = plan(&config, ShipMode::Ask);
    assert_eq!(start_step(&ask), Start::Confirm("make deploy".into()));

    turn_on(&config, "make deploy-prod");
    let changed = plan(&config, ShipMode::AutoBeta);
    assert!(changed.needs_confirm);
    assert_eq!(
        start_step(&changed),
        Start::Confirm("make deploy-prod".into())
    );
}

#[test]
fn forget_makes_the_next_deploy_ask() {
    let config = dir("forget");
    turn_on(&config, "make deploy");
    runs::remember(&config, Kind::Deploy, KEY, "make deploy").unwrap();
    deploy::forget_for_key(&config, KEY).unwrap();
    let p = plan(&config, ShipMode::AutoBeta);
    assert_eq!(p.remembered, None);
    assert_eq!(start_step(&p), Start::Confirm("make deploy".into()));
}

#[test]
fn cancel_on_the_confirm_runs_nothing() {
    let config = dir("cancel");
    turn_on(&config, "make deploy");
    let mut panel = DeployPanel::default();
    assert_eq!(panel.press(&plan(&config, ShipMode::Ask)), None);
    panel.cancel();
    assert_eq!(panel.confirming, None);
    assert!(!panel.busy);
    assert!(activity::list(&config, 5).is_empty());
    assert_eq!(runs::remembered(&config, Kind::Deploy, KEY).unwrap(), None);
}

#[test]
fn a_confirmed_run_streams_redacted_output_logs_deploy_and_remembers() {
    let config = dir("run-config");
    let work = dir("run-work");
    let vault = MemVault::default();
    secrets::set(&config, &vault, KEY, "DEPLOY_TOKEN", VALUE).unwrap();
    turn_on(&config, echo_secret());

    let mut panel = DeployPanel::default();
    panel.begin();
    assert!(panel.busy);
    let rx = spawn_run_with(
        config.clone(),
        KEY.into(),
        work.clone(),
        ship(ShipMode::Ask),
        echo_secret().into(),
        true,
        Box::new(vault),
        deploy::Where {
            host: "github".into(),
            branch: "code/deploy".into(),
        },
        None,
    );
    // Every chunk the panel sees is checked, not only the final output.
    let mut seen = String::new();
    let until = std::time::Instant::now() + std::time::Duration::from_secs(20);
    loop {
        match rx.recv_timeout(until.saturating_duration_since(std::time::Instant::now())) {
            Ok(Msg::Chunk(text)) => {
                assert!(!text.contains(VALUE), "a chunk showed the secret: {text}");
                seen.push_str(&text);
                panel.apply(Msg::Chunk(text));
            }
            Ok(msg) => panel.apply(msg),
            Err(_) => break,
        }
    }
    assert!(!panel.busy);
    assert_eq!(panel.note.as_deref(), Some("Deploy finished."));
    assert!(seen.contains("value=[redacted]"), "{seen}");
    assert!(panel.log.contains("value=[redacted]"), "{}", panel.log);
    assert!(!panel.log.contains(VALUE));

    let log = activity::list(&config, 5);
    assert_eq!(log.len(), 1);
    assert_eq!(log[0].kind, ActKind::Deploy);
    assert_eq!(log[0].approved_by, "user");
    assert_eq!(log[0].mode, "ask");
    assert_eq!(log[0].host, "github");
    assert_eq!(log[0].branch, "code/deploy");
    assert_eq!(log[0].exit, Some(0));
    assert_eq!(log[0].summary, "Deploy finished.");
    assert_eq!(log[0].command, echo_secret());
    let raw = std::fs::read_to_string(config.join(activity::FILE)).unwrap();
    assert!(!raw.contains(VALUE));

    assert_eq!(
        runs::remembered(&config, Kind::Deploy, KEY)
            .unwrap()
            .as_deref(),
        Some(echo_secret())
    );
}

#[test]
fn a_run_for_a_command_that_changed_since_the_confirm_runs_nothing() {
    let config = dir("changed-config");
    let work = dir("changed-work");
    turn_on(&config, "echo second");
    let mut panel = DeployPanel::default();
    panel.begin();
    panel.watch(spawn_run_with(
        config.clone(),
        KEY.into(),
        work,
        ship(ShipMode::AutoBeta),
        "echo first".into(),
        true,
        Box::new(MemVault::default()),
        deploy::Where {
            host: "other".into(),
            branch: String::new(),
        },
        None,
    ));
    drain(&mut panel);
    assert!(!panel.busy);
    assert_eq!(panel.note.as_deref(), Some(deploy::CHANGED));
    assert_eq!(panel.log, "");
    assert!(activity::list(&config, 5).is_empty());
    assert_eq!(runs::remembered(&config, Kind::Deploy, KEY).unwrap(), None);
}

#[test]
fn an_unconfirmed_run_that_needs_a_confirm_is_refused() {
    let config = dir("refuse-config");
    let work = dir("refuse-work");
    turn_on(&config, "echo hi");
    let mut panel = DeployPanel::default();
    panel.begin();
    panel.watch(spawn_run_with(
        config.clone(),
        KEY.into(),
        work,
        ship(ShipMode::AutoBeta),
        "echo hi".into(),
        false,
        Box::new(MemVault::default()),
        deploy::Where {
            host: "other".into(),
            branch: String::new(),
        },
        None,
    ));
    drain(&mut panel);
    assert_eq!(panel.note.as_deref(), Some(deploy::NEEDS_CONFIRM));
    assert!(activity::list(&config, 5).is_empty());
}

#[test]
fn a_finished_run_replaces_the_streamed_log_and_marks_a_cut() {
    let mut panel = DeployPanel::default();
    panel.begin();
    panel.apply(Msg::Chunk("partial ".into()));
    assert_eq!(panel.log, "partial ");
    panel.apply(Msg::Done(Ok(RunResult {
        command: "make deploy".into(),
        exit: 2,
        output: "full output".into(),
        cut: true,
        changed_files: false,
    })));
    assert!(!panel.busy);
    assert!(
        panel.changed,
        "the tab reads its activity again after a deploy"
    );
    assert_eq!(panel.note.as_deref(), Some("Deploy exited with 2."));
    assert_eq!(
        panel.log,
        "full output\n… cut here; the output is too long to show in full."
    );
}

#[test]
fn a_worker_that_ends_without_a_result_frees_the_button() {
    let mut panel = DeployPanel::default();
    panel.begin();
    let (tx, rx) = mpsc::channel::<Msg>();
    panel.watch(rx);
    assert!(!panel.pump());
    drop(tx);
    assert!(panel.pump());
    assert!(!panel.busy);
    assert_eq!(
        panel.note.as_deref(),
        Some("The deploy stopped without a result.")
    );
}

#[test]
fn a_new_plan_resets_the_draft_and_the_mode_line_follows_it() {
    let config = dir("draft");
    turn_on(&config, "make deploy");
    let mut panel = DeployPanel {
        loading: true,
        draft_command: "typed but not saved".into(),
        ..DeployPanel::default()
    };
    panel.apply(Msg::Plan(Ok(plan(&config, ShipMode::AutoBeta))));
    assert!(!panel.loading);
    assert!(panel.draft_on);
    assert_eq!(panel.draft_command, "make deploy");
    assert_eq!(
        mode_line(panel.plan.as_ref()),
        "Auto-beta: a remembered deploy runs without asking, and asks again if its command changes. Your project's secrets are set as environment variables for the run and hidden in the output and the activity list."
    );
    panel.apply(Msg::Plan(Ok(plan(&config, ShipMode::Ask))));
    assert_eq!(
        mode_line(panel.plan.as_ref()),
        "Ask: every deploy shows the exact command first. Your project's secrets are set as environment variables for the run and hidden in the output and the activity list."
    );
}

#[test]
fn a_missing_secret_value_is_named_and_points_at_this_page() {
    assert_eq!(
        here("DEPLOY_TOKEN has no value in the keychain. Set it again in Settings → Code."),
        "DEPLOY_TOKEN has no value in the keychain. Set it again below."
    );
    assert_eq!(here(deploy::CHANGED), deploy::CHANGED);
}

#[test]
fn a_listed_secret_with_no_value_stops_the_deploy_before_it_runs() {
    let config = dir("novalue-config");
    let work = dir("novalue-work");
    secrets::set(&config, &MemVault::default(), KEY, "DEPLOY_TOKEN", VALUE).unwrap();
    turn_on(&config, "echo should-not-run");
    let mut panel = DeployPanel::default();
    panel.begin();
    panel.watch(spawn_run_with(
        config.clone(),
        KEY.into(),
        work,
        ship(ShipMode::Ask),
        "echo should-not-run".into(),
        true,
        // A different (empty) keychain: the name is listed, the value is gone.
        Box::new(MemVault::default()),
        deploy::Where {
            host: "github".into(),
            branch: "main".into(),
        },
        None,
    ));
    drain(&mut panel);
    assert_eq!(
        panel.note.as_deref(),
        Some("DEPLOY_TOKEN has no value in the keychain. Set it again below.")
    );
    assert_eq!(panel.log, "");
    assert!(activity::list(&config, 5).is_empty());
    assert_eq!(runs::remembered(&config, Kind::Deploy, KEY).unwrap(), None);
}

// ---- the panel follows the project, and a deploy can be stopped ----

#[test]
fn another_project_starts_the_panel_over() {
    let config = dir("follow");
    turn_on(&config, "./deploy.sh");
    let (a, b) = (Path::new("/home/user/a"), Path::new("/home/user/b"));
    let mut panel = DeployPanel {
        confirming: Some("./deploy.sh".into()),
        note: Some("Deploy finished.".into()),
        settings_note: Some("Saved.".into()),
        log: "uploaded 3 files\n".into(),
        loaded_for: Some(a.to_path_buf()),
        ..DeployPanel::default()
    };
    panel.take_plan(plan(&config, ShipMode::Ask));
    assert_eq!(panel.draft_command, "./deploy.sh");

    // The same project: nothing changes.
    assert!(!panel.follow(a));
    assert_eq!(panel.confirming.as_deref(), Some("./deploy.sh"));

    // Project B: project A's confirm, notes, log, plan and draft are gone.
    assert!(panel.follow(b));
    assert_eq!(panel.confirming, None);
    assert_eq!(panel.note, None);
    assert_eq!(panel.settings_note, None);
    assert_eq!(panel.log, "");
    assert_eq!(panel.plan, None);
    assert_eq!(panel.draft_command, "");
    assert!(!panel.draft_on);
}

#[test]
fn a_running_deploy_keeps_its_panel_until_it_ends() {
    let (a, b) = (Path::new("/home/user/a"), Path::new("/home/user/b"));
    let mut panel = DeployPanel {
        loaded_for: Some(a.to_path_buf()),
        ..DeployPanel::default()
    };
    panel.begin();
    panel.apply(Msg::Chunk("step 1\n".into()));
    assert!(!panel.follow(b), "a deploy still running keeps its output");
    assert_eq!(panel.log, "step 1\n");
    // It ends under project B: its result goes, and the tab still reads the
    // activity list again.
    panel.apply(Msg::Done(Ok(RunResult {
        command: "./deploy.sh".into(),
        exit: 0,
        output: "step 1\nstep 2\n".into(),
        cut: false,
        changed_files: false,
    })));
    assert!(panel.follow(b));
    assert_eq!(panel.note, None);
    assert_eq!(panel.log, "");
    assert!(panel.changed);
}

#[cfg(unix)]
#[test]
fn stop_ends_a_running_deploy_and_says_so() {
    let config = dir("stop-config");
    let work = dir("stop-work");
    turn_on(&config, "echo up; sleep 30");

    let mut panel = DeployPanel::default();
    assert!(!panel.stop(), "no deploy is running yet");
    panel.begin();
    panel.watch(spawn_run_with(
        config.clone(),
        KEY.into(),
        work.clone(),
        ship(ShipMode::Ask),
        "echo up; sleep 30".into(),
        true,
        Box::new(MemVault::default()),
        deploy::Where {
            host: "github".into(),
            branch: "code/deploy".into(),
        },
        None,
    ));
    let until = std::time::Instant::now() + std::time::Duration::from_secs(20);
    while !panel.log.contains("up") && std::time::Instant::now() < until {
        panel.pump();
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert_eq!(panel.log, "up\n");

    // Stop: the 30-second sleep ends within five.
    assert!(panel.stop());
    let until = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while panel.busy && std::time::Instant::now() < until {
        panel.pump();
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    // Whatever happened, the sleep is not left behind.
    runs::stop();
    assert!(!panel.busy, "Stop left the deploy going");
    assert_eq!(panel.note.as_deref(), Some("Deploy stopped."));
    // One run at a time: the next one starts, so the stopped one is gone.
    let next = runs::run(&config, &work, "exit 0", true, runs::Mode::Auto, |_| {});
    assert_eq!(next.map(|done| done.exit), Ok(0));

    // A deploy that fails by itself is not called stopped.
    panel.begin();
    panel.apply(Msg::Done(Ok(RunResult {
        command: "echo up; sleep 30".into(),
        exit: 2,
        output: String::new(),
        cut: false,
        changed_files: false,
    })));
    assert_eq!(panel.note.as_deref(), Some("Deploy exited with 2."));
}
