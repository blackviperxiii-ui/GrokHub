use super::*;
use std::sync::Mutex;

use grokhub_agent::{
    ClientError, FunctionCall, InputItem, ResponsesRequest, StreamEvent, TurnOutput,
};

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "grokhub-shell-unattended-{name}-{}-{}",
        std::process::id(),
        grokhub_core::now_ms()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

/// Asks to write `out.txt` once, then says what came back.
struct WantsToWrite {
    requests: Mutex<Vec<ResponsesRequest>>,
}

impl ModelClient for WantsToWrite {
    fn stream(
        &self,
        req: &ResponsesRequest,
        _cancel: &CancelToken,
        _sink: &mut dyn FnMut(StreamEvent),
    ) -> Result<TurnOutput, ClientError> {
        let mut requests = self.requests.lock().expect("requests");
        requests.push(req.clone());
        if requests.len() == 1 {
            return Ok(TurnOutput {
                calls: vec![FunctionCall {
                    call_id: "w".into(),
                    name: "write".into(),
                    arguments: r#"{"path":"out.txt","content":"x"}"#.into(),
                }],
                ..TurnOutput::default()
            });
        }
        Ok(TurnOutput {
            text: "ok".into(),
            ..TurnOutput::default()
        })
    }
}

/// One run of `prompt` behind `gate`: the client, and whether `out.txt` landed.
fn run_job(name: &str, prompt: &str, gate: RunGate) -> (Arc<WantsToWrite>, bool) {
    run_job_with(name, prompt, None, gate)
}

/// [`run_job`] with the tool set bound from `tools_from`.
fn run_job_with(
    name: &str,
    prompt: &str,
    tools_from: Option<&str>,
    gate: RunGate,
) -> (Arc<WantsToWrite>, bool) {
    let (config, workspace) = (
        temp_dir(&format!("{name}-cfg")),
        temp_dir(&format!("{name}-ws")),
    );
    let _config = grokhub_agent::perm::ConfigGuard::set(&config);
    let client = Arc::new(WantsToWrite {
        requests: Mutex::new(Vec::new()),
    });
    let done = run(Job {
        client: client.clone(),
        model: "grok-4.7".into(),
        workspace: workspace.clone(),
        session_id: format!("shell-unattended-test-{name}-{}", std::process::id()),
        prompt: prompt.into(),
        tools_from: tools_from.map(str::to_string),
        gate,
        cancel: CancelToken::new(),
        resume: false,
    });
    assert_eq!(done.stop_reason, "end_turn", "{}", done.error);
    (client, workspace.join("out.txt").exists())
}

fn tool_names(req: &ResponsesRequest) -> Vec<String> {
    req.tools
        .iter()
        .filter_map(|t| t["name"].as_str().map(str::to_string))
        .collect()
}

fn write_reply(client: &WantsToWrite) -> Option<String> {
    let requests = client.requests.lock().expect("requests");
    requests[1].input.iter().find_map(|item| match item {
        InputItem::FunctionCallOutput { output, .. } => Some(output.clone()),
        _ => None,
    })
}

#[test]
fn ask_is_a_read_only_unattended_session_and_always_is_not() {
    let ask = RunGate::scheduled(PermMode::Ask, false, None).gate();
    assert_eq!(
        (ask.mode, ask.readonly_session, ask.attended, ask.desktop),
        (PermMode::Ask, true, false, false)
    );
    let always = RunGate::scheduled(PermMode::Always, false, None).gate();
    assert_eq!((always.readonly_session, always.attended), (false, false));
    assert_eq!(RunGate::BACKGROUND.gate(), ask);
    assert_eq!(RunGate::BACKGROUND.wire(), PermissionMode::Ask);
    assert_eq!(
        RunGate::BACKGROUND
            .in_mode(PermissionMode::AlwaysApprove)
            .mode,
        PermMode::Always
    );
}

#[test]
fn desktop_tools_come_only_with_control_on_and_no_game_holding_it() {
    assert!(RunGate::scheduled(PermMode::Auto, true, None).desktop);
    assert!(!RunGate::scheduled(PermMode::Auto, false, None).desktop);
    let held = RunGate::scheduled(PermMode::Auto, true, Some("A game is running"));
    assert!(!held.desktop);
    assert!(!held.gate().desktop);
}

#[test]
fn an_ask_run_fails_closed_on_a_write() {
    let (client, wrote) = run_job("ask", "write out.txt", RunGate::BACKGROUND);
    assert!(!wrote);
    assert_eq!(
        write_reply(&client).as_deref(),
        Some(
            "read-only in this phase: `write` is not available. Use read_file, list_dir, grep, or glob."
        )
    );
}

#[test]
fn an_always_run_writes_like_the_cabins_scheduled_run() {
    let gate = RunGate::scheduled(PermMode::Always, false, None);
    let (client, wrote) = run_job("always", "write out.txt", gate);
    assert!(wrote, "{:?}", write_reply(&client));
    // An undeclared job keeps the full tool set, as chat does.
    let first = &client.requests.lock().expect("requests")[0];
    assert!(tool_names(first).iter().any(|n| n == "write"));
}

#[test]
fn a_tools_line_leans_the_set_like_the_cabins_jobs() {
    let gate = RunGate::scheduled(PermMode::Always, false, None);
    let (full, _) = run_job("full", "tidy the notes", gate);
    let (lean, _) = run_job("lean", "NEED_SHELL\ntidy the notes", gate);
    let full = tool_names(&full.requests.lock().expect("requests")[0]);
    let lean = tool_names(&lean.requests.lock().expect("requests")[0]);
    let allowed = grokhub_agent::ToolSet::background().with_tool("shell");
    assert!(lean.iter().all(|n| allowed.allows(n)), "{lean:?}");
    assert!(lean.iter().any(|n| n == "run_terminal_command"), "{lean:?}");
    // The full set has tools the lean one leaves out.
    let left_out: Vec<&String> = full.iter().filter(|n| !allowed.allows(n)).collect();
    assert!(!left_out.is_empty(), "{full:?}");
    assert!(lean.len() < full.len(), "{} vs {}", lean.len(), full.len());
}

#[test]
fn a_resumed_automation_keeps_the_tools_its_instructions_declare() {
    let gate = RunGate::scheduled(PermMode::Always, false, None);
    // The prompt is the resume block, which declares nothing; the set comes
    // from the automation's own instructions.
    let block = "Resuming workboard card a-1 from its checkpoint.";
    let (lean, _) = run_job_with("resumed", block, Some("NEED_SHELL\ndigest the feed"), gate);
    let (full, _) = run_job("unbound", block, gate);
    let lean = tool_names(&lean.requests.lock().expect("requests")[0]);
    let full = tool_names(&full.requests.lock().expect("requests")[0]);
    let allowed = grokhub_agent::ToolSet::background().with_tool("shell");
    assert!(lean.iter().all(|n| allowed.allows(n)), "{lean:?}");
    assert!(lean.len() < full.len(), "{} vs {}", lean.len(), full.len());
}

/// Counts the desktop calls that reach the screen.
#[derive(Default)]
struct CountingDesktop {
    calls: Arc<Mutex<Vec<String>>>,
}

impl DesktopOps for CountingDesktop {
    fn halted(&self) -> bool {
        false
    }

    fn locked(&self) -> bool {
        false
    }

    fn call(&self, name: &str, _args: &serde_json::Value) -> ToolOutput {
        self.calls.lock().expect("calls").push(name.to_string());
        ToolOutput::ok("clicked")
    }
}

fn save_desktop_control(config: &std::path::Path, on: bool) {
    crate::settings::save(
        config,
        &crate::settings::ShellSettings {
            desktop_control: on,
            ..crate::settings::ShellSettings::default()
        },
    )
    .expect("settings");
}

#[test]
fn each_desktop_step_reads_the_saved_switch_again() {
    let config = temp_dir("saved-switch");
    save_desktop_control(&config, true);
    let calls = Arc::new(Mutex::new(Vec::new()));
    let desk = SavedSwitch {
        inner: Box::new(CountingDesktop {
            calls: Arc::clone(&calls),
        }),
        config_dir: config.clone(),
    };
    let args = serde_json::json!({ "x": 10, "y": 10 });
    assert_eq!(desk.call("click", &args).text, "clicked");
    // Settings turns desktop control off while the job runs.
    save_desktop_control(&config, false);
    let off = desk.call("click", &args);
    assert!(off.failed);
    assert_eq!(off.text, *grokhub_core::desktop_mcp::OFF_MSG);
    assert_eq!(desk.hint("click", &args), args);
    // A look reads the screen too, and nobody is there to ask.
    let look = desk.look(&serde_json::json!({}));
    assert!(look.failed);
    assert_eq!(look.text, *grokhub_core::desktop_mcp::OFF_MSG);
    assert_eq!(*calls.lock().expect("calls"), ["click"]);
    // And back on: the next step goes through again.
    save_desktop_control(&config, true);
    assert_eq!(desk.call("screenshot", &args).text, "clicked");
    assert_eq!(desk.look(&serde_json::json!({})).text, "clicked");
    assert_eq!(
        *calls.lock().expect("calls"),
        ["click", "screenshot", "screenshot"]
    );
}

/// Runs `before` on the first turn (the user's action mid-run), asks for
/// `calls`, then says done.
struct Flips {
    requests: Mutex<Vec<ResponsesRequest>>,
    calls: Vec<(&'static str, &'static str)>,
    before: Box<dyn Fn() + Send + Sync>,
}

impl ModelClient for Flips {
    fn stream(
        &self,
        req: &ResponsesRequest,
        _cancel: &CancelToken,
        _sink: &mut dyn FnMut(StreamEvent),
    ) -> Result<TurnOutput, ClientError> {
        let mut requests = self.requests.lock().expect("requests");
        requests.push(req.clone());
        if requests.len() == 1 {
            (self.before)();
            return Ok(TurnOutput {
                calls: self
                    .calls
                    .iter()
                    .enumerate()
                    .map(|(i, (name, args))| FunctionCall {
                        call_id: format!("c{i}"),
                        name: (*name).into(),
                        arguments: (*args).into(),
                    })
                    .collect(),
                ..TurnOutput::default()
            });
        }
        Ok(TurnOutput {
            text: "ok".into(),
            ..TurnOutput::default()
        })
    }
}

/// A scheduled or Run now job started with desktop control on, and the
/// switch went off mid-run: its later desktop steps are refused.
#[cfg(target_os = "linux")]
#[test]
fn switching_desktop_control_off_stops_a_running_jobs_next_desktop_steps() {
    let (config, workspace) = (temp_dir("flip-cfg"), temp_dir("flip-ws"));
    let _config = grokhub_agent::perm::ConfigGuard::set(&config);
    save_desktop_control(&config, true);
    let flip = config.clone();
    let client = Arc::new(Flips {
        requests: Mutex::new(Vec::new()),
        calls: vec![
            ("click", r#"{"x":10,"y":10}"#),
            ("look_at_screen", "{}"),
            ("screenshot", "{}"),
        ],
        // As save_settings writes it.
        before: Box::new(move || save_desktop_control(&flip, false)),
    });
    let done = run(Job {
        client: client.clone(),
        model: "grok-4.7".into(),
        workspace,
        session_id: format!("shell-unattended-test-flip-{}", std::process::id()),
        prompt: "click at 10,10 then take a screenshot".into(),
        tools_from: None,
        gate: RunGate::scheduled(PermMode::Always, true, None),
        cancel: CancelToken::new(),
        resume: false,
    });
    assert_eq!(done.stop_reason, "end_turn", "{}", done.error);
    let requests = client.requests.lock().expect("requests");
    let outputs: Vec<String> = requests[1]
        .input
        .iter()
        .filter_map(|item| match item {
            InputItem::FunctionCallOutput { output, .. } => Some(output.clone()),
            _ => None,
        })
        .collect();
    let off = grokhub_core::desktop_mcp::OFF_MSG.to_string();
    assert_eq!(outputs, [off.clone(), off.clone(), off]);
}

/// What a run behind `gate` in session `session` got back from a click and
/// a look, with desktop control saved on.
fn desktop_steps(name: &str, session: &str, gate: RunGate) -> Vec<String> {
    let (config, workspace) = (
        temp_dir(&format!("{name}-cfg")),
        temp_dir(&format!("{name}-ws")),
    );
    let _config = grokhub_agent::perm::ConfigGuard::set(&config);
    save_desktop_control(&config, true);
    grokhub_agent::web_origin::note_chat(&config, "shell-web-chat-u1").expect("web chat");
    let client = Arc::new(Flips {
        requests: Mutex::new(Vec::new()),
        calls: vec![("click", r#"{"x":10,"y":10}"#), ("look_at_screen", "{}")],
        before: Box::new(|| {}),
    });
    let done = run(Job {
        client: client.clone(),
        model: "grok-4.7".into(),
        workspace,
        session_id: session.to_string(),
        prompt: "click at 10,10 then look at the screen".into(),
        tools_from: None,
        gate,
        cancel: CancelToken::new(),
        resume: false,
    });
    assert_eq!(done.stop_reason, "end_turn", "{}", done.error);
    let requests = client.requests.lock().expect("requests");
    requests[1]
        .input
        .iter()
        .filter_map(|item| match item {
            InputItem::FunctionCallOutput { output, .. } => Some(output.clone()),
            _ => None,
        })
        .collect()
}

/// A run that started from a paired browser's words has no desktop, with
/// desktop control on: a browser's Run now or a job a browser wrote, and a
/// run in a chat a browser started.
#[test]
fn a_browsers_run_gets_no_desktop_with_control_on() {
    let on = RunGate::scheduled(PermMode::Always, true, None);
    let none = [
        grokhub_core::desktop_mcp::OFF_MSG.to_string(),
        "There is no screen to look at here.".to_string(),
    ];
    let pid = std::process::id();
    let browser = on.with_origin(RunOrigin::Browser);
    assert_eq!(
        desktop_steps("web-run", &format!("shell-auto-web-{pid}"), browser),
        none
    );
    assert_eq!(desktop_steps("web-chat", "shell-web-chat-u1", on), none);
}
