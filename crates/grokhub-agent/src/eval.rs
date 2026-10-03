//! Phase 16 eval harness. Dry-run is the default and uses scripted model clients
//! plus the fake CLI agent. `--live` is refused before any client is built.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use grokhub_acp::{connect, wait_event, AcpEvent, SessionMode, SpawnOpts};
use serde_json::{json, Value};

use crate::client::{
    ClientError, ContentPart, FunctionCall, InputItem, ResponsesRequest, StreamEvent, TurnOutput,
};
use crate::events::Engine;
use crate::gate::{ClosedPermits, Gate, PermMode};
use crate::perm::ConfigGuard;
use crate::tools::ImagineApi;
use crate::{
    AuthKind, CancelToken, EngineParts, HaltCheck, ModelClient, NativeEngine, SteerQueue, Usage,
};

pub const SUITE_ITEMS: [&str; 7] = [
    "xvfb-desktop",
    "repo-bugfix",
    "ask-refusal",
    "background-halt",
    "mcp-tool",
    "compaction",
    "imagine",
];

const SKIP_XVFB: &str = "skipped: no Xvfb";
const NO_FAKE_ACP: &str = "not verified: fake_acp binary not found";
const NO_FAKE_MCP: &str = "not verified: fake_mcp binary not found";
const IMAGINE_PROMPT: &str = "a red square";
const LIVE_KEY: &str = "GROKHUB_EVAL_API_KEY";
const ERR_NO_KEY: &str = "refusing --live: GROKHUB_EVAL_API_KEY is not set";
const ERR_NO_BUDGET: &str =
    "refusing --live: --budget-usd is required and must satisfy 0 < amount <= 0.20";
const ERR_BAD_BUDGET: &str = "refusing --live: --budget-usd must satisfy 0 < amount <= 0.20";
const LIVE_UNIMPLEMENTED: &str = "live mode is not implemented in this build";

static IMAGINE_SENDS: AtomicU64 = AtomicU64::new(0);
static SCRATCH_SEQ: AtomicU64 = AtomicU64::new(0);
static SUITE: Mutex<()> = Mutex::new(());

#[derive(Debug)]
pub struct Opts {
    pub live: bool,
    /// Whole cents. `20` is the $0.20 cap. Empty unless `--budget-usd` parsed.
    pub budget_cents: Option<u32>,
    pub out: Option<PathBuf>,
    pub fake_acp: Option<PathBuf>,
}

pub struct ItemResult {
    pub item: &'static str,
    pub engine: &'static str,
    pub success: bool,
    pub turns: u32,
    /// Measured for the example's stderr line. The report writes `dry-run` instead.
    pub wall_ms: u128,
    pub status: &'static str,
    pub verified: bool,
}

/// Sends recorded by the dry-run Imagine path. Stays zero because that path
/// only builds the request body.
pub fn imagine_send_count() -> u64 {
    IMAGINE_SENDS.load(Ordering::SeqCst)
}

/// Parse harness flags. `--live` is rejected here, before a client exists.
pub fn parse_args(args: &[String]) -> Result<Opts, String> {
    let key = std::env::var(LIVE_KEY).ok();
    parse_args_with_key(args, key.as_deref())
}

/// `parse_args` with the key value passed in, so tests never touch the
/// process environment.
fn parse_args_with_key(args: &[String], key: Option<&str>) -> Result<Opts, String> {
    let mut live = false;
    let mut dry = false;
    let mut budget: Option<Result<u32, String>> = None;
    let mut out = None;
    let mut fake_acp = None;
    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        if arg == "--live" {
            live = true;
        } else if arg == "--dry-run" {
            dry = true;
        } else if arg == "--help" || arg == "-h" {
            return Err(help_text());
        } else if arg == "--budget-usd" || arg == "--out" || arg == "--fake-acp" {
            let Some(value) = args.get(i + 1) else {
                return Err(missing_value(arg, live));
            };
            if value.starts_with("--") {
                return Err(missing_value(arg, live));
            }
            i += 1;
            match arg.as_str() {
                "--budget-usd" => budget = Some(parse_budget_token(value)),
                "--out" => out = Some(PathBuf::from(value)),
                "--fake-acp" => fake_acp = Some(PathBuf::from(value)),
                _ => {}
            }
        } else if let Some(value) = arg.strip_prefix("--budget-usd=") {
            budget = Some(parse_budget_token(value));
        } else if let Some(value) = arg.strip_prefix("--out=") {
            out = Some(PathBuf::from(value));
        } else if let Some(value) = arg.strip_prefix("--fake-acp=") {
            fake_acp = Some(PathBuf::from(value));
        } else {
            return Err(format!("unknown argument: {arg}"));
        }
        i += 1;
    }
    if live && dry {
        return Err("refusing --live: do not pass --dry-run with --live".into());
    }
    if live {
        if !key.is_some_and(|value| !value.trim().is_empty()) {
            return Err(ERR_NO_KEY.into());
        }
        match budget {
            None => return Err(ERR_NO_BUDGET.into()),
            Some(Err(err)) => return Err(err),
            Some(Ok(_)) => {}
        }
    } else if let Some(Err(err)) = budget {
        return Err(err);
    }
    Ok(Opts {
        live,
        budget_cents: budget.and_then(|item| item.ok()),
        out,
        fake_acp,
    })
}

/// Even a sanctioned key and a budget do not start a live client in this build.
pub fn reject_live(opts: &Opts) -> Result<(), String> {
    if opts.live {
        Err(LIVE_UNIMPLEMENTED.into())
    } else {
        Ok(())
    }
}

pub fn run_suite(opts: &Opts) -> Vec<ItemResult> {
    let _lock = SUITE.lock().unwrap_or_else(|err| err.into_inner());
    if opts.live {
        return Vec::new();
    }
    let cfg = scratch("cfg");
    let _guard = cfg.as_ref().map(ConfigGuard::set);
    let fake_acp = locate_fake_acp(opts.fake_acp.as_deref());
    let mut items = Vec::with_capacity(SUITE_ITEMS.len() * 2);
    items.push(measure("xvfb-desktop", "native", desktop_item));
    items.push(measure("xvfb-desktop", "cli", desktop_item));
    items.push(measure("repo-bugfix", "native", repo_bugfix_native));
    items.push(measure("repo-bugfix", "cli", || {
        repo_bugfix_cli(fake_acp.as_deref())
    }));
    items.push(measure("ask-refusal", "native", ask_native));
    items.push(measure("ask-refusal", "cli", || {
        ask_cli(fake_acp.as_deref())
    }));
    items.push(measure("background-halt", "native", background_halt_native));
    items.push(measure("background-halt", "cli", || {
        background_halt_cli(fake_acp.as_deref())
    }));
    items.push(measure("mcp-tool", "native", || mcp_native(cfg.as_deref())));
    items.push(measure("mcp-tool", "cli", || mcp_cli(fake_acp.as_deref())));
    items.push(measure("compaction", "native", compact_native));
    items.push(measure("compaction", "cli", || {
        compact_cli(fake_acp.as_deref())
    }));
    items.push(measure("imagine", "native", imagine_native));
    items.push(measure("imagine", "cli", || {
        imagine_cli(fake_acp.as_deref())
    }));
    cleanup_sessions();
    if let Some(dir) = cfg {
        let _ = std::fs::remove_dir_all(dir);
    }
    items
}

pub fn render_report(items: &[ItemResult]) -> String {
    let mut out = String::new();
    out.push_str("# Native parity v1\n\n");
    out.push_str("## Method\n\n");
    out.push_str(
        "Phase 16 runs one fixed suite against the native engine and the CLI engine. \
Dry-run is the default (`--dry-run`, and the mode when no mode flag is given). \
The native engine uses scripted fake model clients. The CLI engine is the \
`grokhub-fake-acp` binary driven with no API key and without a cabin home. \
Dry-run uses no network and no credentials.\n\n",
    );
    out.push_str(
        "Wall time is measured for each item and printed on the example's stderr. \
This file records wall time as `dry-run`, and pool % and cost as `0`, so two \
dry-runs are byte-identical.\n\n",
    );
    out.push_str(
        "`--live` is refused before any client is built unless `GROKHUB_EVAL_API_KEY` \
is set to a non-empty value and `--budget-usd` is set with `0 < amount <= 0.20`. ",
    );
    out.push_str(&credential_boundary());
    out.push_str(
        " Live mode is not implemented in this build and was not run for this report.\n\n",
    );
    out.push_str(
        "Suite items: an Xvfb desktop probe, a temp repo bugfix with a test, an Ask-mode \
refusal, background plus Halt, an MCP tool call through a fake stdio server, compaction \
of a long transcript, and an Imagine call that only builds the request.\n\n",
    );
    out.push_str("This report does not switch the default engine.\n\n");
    out.push_str("## Results\n\n");
    out.push_str("| Item | Engine | Success | Turns | Wall time | Pool % | Cost | Status |\n");
    out.push_str("|---|---|---|---|---|---|---|---|\n");
    for item in items {
        let success = if item.success { "yes" } else { "no" };
        out.push_str(&format!(
            "| {} | {} | {success} | {} | dry-run | 0 | 0 | {} |\n",
            item.item, item.engine, item.turns, item.status
        ));
    }
    out.push_str("\n## GAPS\n\n");
    let gaps = gap_lines(items);
    if gaps.is_empty() {
        out.push_str("- none\n");
    } else {
        for line in gaps {
            out.push_str(&line);
            out.push('\n');
        }
    }
    out.push_str("\nNo default was switched.\n");
    out
}

/// Sentence that names the credential files this harness does not open.
pub fn credential_boundary() -> String {
    format!(
        "The key is read only from GROKHUB_EVAL_API_KEY. This harness does not read {}, {}, {}, or a keychain, and it does not read the cabin's stored credentials.",
        concat!("~/.grok/", "auth", ".json"),
        concat!("config", ".toml"),
        concat!("secrets", ".json"),
    )
}

fn help_text() -> String {
    "eval [--dry-run] [--live --budget-usd AMOUNT] [--out PATH] [--fake-acp PATH]\n\
dry-run is the default and uses no network\n\
--live requires GROKHUB_EVAL_API_KEY and 0 < --budget-usd <= 0.20"
        .into()
}

fn missing_value(flag: &str, live: bool) -> String {
    if flag == "--budget-usd" && live {
        ERR_NO_BUDGET.into()
    } else {
        format!("missing value for {flag}")
    }
}

fn parse_budget_token(raw: &str) -> Result<u32, String> {
    match cents_of(raw) {
        Ok(cents) if cents > 0 && cents <= 20 => Ok(cents),
        Ok(cents) if cents > 20 => Err(format!(
            "refusing --live: --budget-usd {raw} is above the 0.20 cap"
        )),
        _ => Err(ERR_BAD_BUDGET.into()),
    }
}

fn cents_of(raw: &str) -> Result<u32, ()> {
    let raw = raw.trim();
    if raw.is_empty() || raw.starts_with('+') {
        return Err(());
    }
    if raw.starts_with('-') {
        return Err(());
    }
    if !raw
        .bytes()
        .all(|byte| byte.is_ascii_digit() || byte == b'.')
    {
        return Err(());
    }
    let mut parts = raw.split('.');
    let whole = parts.next().unwrap_or("");
    let frac = parts.next().unwrap_or("");
    if parts.next().is_some() || frac.len() > 2 {
        return Err(());
    }
    if whole.is_empty() && frac.is_empty() {
        return Err(());
    }
    if !whole.bytes().all(|byte| byte.is_ascii_digit())
        || !frac.bytes().all(|byte| byte.is_ascii_digit())
    {
        return Err(());
    }
    let dollars: u32 = if whole.is_empty() {
        0
    } else {
        whole.parse().map_err(|_| ())?
    };
    let mut cents: u32 = if frac.is_empty() {
        0
    } else {
        frac.parse().map_err(|_| ())?
    };
    if frac.len() == 1 {
        cents = cents.saturating_mul(10);
    }
    dollars
        .checked_mul(100)
        .and_then(|base| base.checked_add(cents))
        .ok_or(())
}

fn measure(
    item: &'static str,
    engine: &'static str,
    work: impl FnOnce() -> (&'static str, u32),
) -> ItemResult {
    let start = Instant::now();
    let (status, turns) = work();
    ItemResult {
        item,
        engine,
        success: success_of(status),
        turns,
        wall_ms: start.elapsed().as_millis(),
        status,
        verified: verified_of(status),
    }
}

fn success_of(status: &str) -> bool {
    matches!(
        status,
        "fixed"
            | "refused"
            | "halted"
            | "echoed"
            | "compacted"
            | "request built"
            | "scripted reply"
            | "scripted tool"
            | "probe ok"
    )
}

fn verified_of(status: &str) -> bool {
    !status.starts_with("skipped:") && !status.starts_with("not verified:")
}

/// Gaps that hold for every dry-run, whatever the rows say.
pub const STANDING_GAPS: [&str; 3] = [
    "- live: `--live` is a stub. It refuses to start without GROKHUB_EVAL_API_KEY and a budget of $0.20 or less, and even with both it only prints \"live mode is not implemented in this build\" and exits non-zero. No live eval has been run, so nothing in this report measures a real model's success, turns, wall time, pool % or cost.",
    "- cli engine: every CLI row ran against grokhub-fake-acp, which replays scripted events. Those rows show that the ACP client handles each scenario, not what the real Grok CLI does, so CLI-versus-native parity is not verified for any item.",
    "- pool % and cost: written as 0 for every dry-run row. They were not measured.",
];

const DESKTOP_PROBE_GAP: &str = "- xvfb-desktop: the probe only checks that an X display answers (`xdpyinfo`), the same check for both engines. Neither engine's desktop tools were driven.";

fn gap_lines(items: &[ItemResult]) -> Vec<String> {
    let mut lines: Vec<String> = STANDING_GAPS.iter().map(|line| line.to_string()).collect();
    if items
        .iter()
        .any(|item| item.item == "xvfb-desktop" && item.status == "probe ok")
    {
        lines.push(DESKTOP_PROBE_GAP.to_string());
    }
    for name in SUITE_ITEMS {
        let Some(native) = items
            .iter()
            .find(|item| item.item == name && item.engine == "native")
        else {
            lines.push(format!("- {name}: missing native result"));
            continue;
        };
        let Some(cli) = items
            .iter()
            .find(|item| item.item == name && item.engine == "cli")
        else {
            lines.push(format!("- {name}: missing cli result"));
            continue;
        };
        if native.status == SKIP_XVFB && cli.status == SKIP_XVFB {
            lines.push(format!("- {name}: {SKIP_XVFB}"));
            continue;
        }
        let mut parts = Vec::new();
        if !native.verified {
            parts.push(format!("native {}", native.status));
        } else if !native.success {
            parts.push(format!("native failed ({})", native.status));
        }
        if !cli.verified {
            parts.push(format!("cli {}", cli.status));
        } else if !cli.success {
            parts.push(format!("cli failed ({})", cli.status));
        }
        if !parts.is_empty() {
            lines.push(format!("- {name}: {}", parts.join("; ")));
        }
    }
    lines
}

fn desktop_item() -> (&'static str, u32) {
    (desktop_status(), 0)
}

fn desktop_status() -> &'static str {
    if !command_on_path("Xvfb") || !display_set() {
        return SKIP_XVFB;
    }
    if probe_current_display() {
        "probe ok"
    } else {
        SKIP_XVFB
    }
}

fn display_set() -> bool {
    std::env::var("DISPLAY")
        .ok()
        .is_some_and(|value| !value.trim().is_empty())
}

fn command_on_path(name: &str) -> bool {
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    let file = format!("{name}{}", std::env::consts::EXE_SUFFIX);
    std::env::split_paths(&path).any(|dir| dir.join(&file).is_file() || dir.join(name).is_file())
}

fn probe_current_display() -> bool {
    let mut cmd = Command::new("xdpyinfo");
    cmd.stdout(std::process::Stdio::null());
    cmd.stderr(std::process::Stdio::null());
    let Ok(mut child) = cmd.spawn() else {
        return false;
    };
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return status.success(),
            Ok(None) if start.elapsed() > Duration::from_secs(2) => {
                let _ = child.kill();
                let _ = child.wait();
                return false;
            }
            Ok(None) => thread::sleep(Duration::from_millis(20)),
            Err(_) => return false,
        }
    }
}

fn repo_bugfix_native() -> (&'static str, u32) {
    let Some(dir) = scratch("bugfix") else {
        return ("failed: scratch", 0);
    };
    if write_bugfix_fixture(&dir).is_err() {
        return ("failed: scratch", 0);
    }
    let client = Arc::new(Scripted::bugfix());
    let shared: Arc<dyn ModelClient + Send + Sync> = client.clone();
    let done = run_native(
        &dir,
        "eval-bugfix",
        shared,
        PermMode::Always,
        "fix the failing test",
    );
    let turns = client.turns();
    let fixed = fixture_passes(&dir);
    let _ = std::fs::remove_dir_all(&dir);
    if done.stop == "end_turn" && fixed {
        ("fixed", turns)
    } else {
        ("failed: test", turns)
    }
}

fn repo_bugfix_cli(program: Option<&Path>) -> (&'static str, u32) {
    let Some(program) = program else {
        return (NO_FAKE_ACP, 0);
    };
    let Some(dir) = scratch("bugfix-cli") else {
        return ("failed: scratch", 0);
    };
    let done = cli_turn(
        program,
        &dir,
        vec![("FAKE_ACP_TEXT".into(), "bugfix-dry-run".into())],
        SessionMode::Chat,
        true,
        false,
        "fix the failing test",
        CliAct::None,
    );
    let _ = std::fs::remove_dir_all(&dir);
    match done {
        Ok(done) if done.stop == "end_turn" && done.text.contains("bugfix-dry-run") => {
            ("scripted reply", 1)
        }
        Ok(_) => ("failed: cli", 1),
        Err(()) => ("failed: cli", 0),
    }
}

fn ask_native() -> (&'static str, u32) {
    let Some(dir) = scratch("ask") else {
        return ("failed: scratch", 0);
    };
    let client = Arc::new(Scripted::ask());
    let shared: Arc<dyn ModelClient + Send + Sync> = client.clone();
    let done = run_native(&dir, "eval-ask", shared, PermMode::Ask, "write the secret");
    let blob = client.blob();
    let refused = done.perms == 0
        && done.stop == "end_turn"
        && !dir.join("secret.txt").exists()
        && blob.contains(&crate::gate::unattended_deny("write"));
    let turns = client.turns();
    let _ = std::fs::remove_dir_all(&dir);
    if refused {
        ("refused", turns)
    } else {
        ("failed: deny", turns)
    }
}

fn ask_cli(program: Option<&Path>) -> (&'static str, u32) {
    let Some(program) = program else {
        return (NO_FAKE_ACP, 0);
    };
    let Some(dir) = scratch("ask-cli") else {
        return ("failed: scratch", 0);
    };
    let done = cli_turn(
        program,
        &dir,
        vec![("FAKE_ACP_PERMISSION".into(), "1".into())],
        SessionMode::Ask,
        false,
        false,
        "write the secret",
        CliAct::Reject,
    );
    let leaked = dir.join("secret.txt").exists();
    let _ = std::fs::remove_dir_all(&dir);
    match done {
        Ok(done) if !leaked && done.text == "perm:selected:reject-once" && done.perms == 1 => {
            ("refused", 1)
        }
        Ok(_) => ("failed: cli", 1),
        Err(()) => ("failed: cli", 0),
    }
}

fn background_halt_native() -> (&'static str, u32) {
    let Some(dir) = scratch("halt") else {
        return ("failed: scratch", 0);
    };
    let blocked = Arc::new(AtomicBool::new(false));
    let client = Arc::new(Scripted::halt(Arc::clone(&blocked)));
    let shared: Arc<dyn ModelClient + Send + Sync> = client.clone();
    let cancel = CancelToken::new();
    let cancel_main = cancel.clone();
    let dir_run = dir.clone();
    let (tx, rx) = std::sync::mpsc::channel();
    let worker = thread::spawn(move || {
        let _guard = ConfigGuard::set(dir_run.join("cfg"));
        let _ = std::fs::create_dir_all(dir_run.join("cfg"));
        let done = run_native_cancel(
            &dir_run,
            "eval-bg-halt",
            shared,
            PermMode::Always,
            "start a background sleep and wait",
            cancel,
        );
        let _ = tx.send(done);
    });
    let start = Instant::now();
    while !blocked.load(Ordering::SeqCst) && start.elapsed() < Duration::from_secs(5) {
        thread::sleep(Duration::from_millis(10));
    }
    let blob_now = client.blob();
    let alive_before = command_is_live(&dir, "eval-bg-halt", &blob_now);
    let pids = workspace_pids(&dir);
    cancel_main.cancel();
    crate::tasks::halt_session("eval-bg-halt");
    // The run forgets the session when it returns, so the process check happens
    // before that join. Halt already signalled the process group.
    let stopped = command_is_stopped("eval-bg-halt", &blob_now, &pids);
    let done = rx.recv_timeout(Duration::from_secs(5)).ok();
    let _ = worker.join();
    #[cfg(unix)]
    {
        for pid in &pids {
            force_kill(*pid);
        }
    }
    crate::tasks::forget_session("eval-bg-halt");
    let turns = client.turns();
    let blob = client.blob();
    let stop_ok = done
        .as_ref()
        .is_some_and(|item| item.stop == "cancelled" || item.stop == "halted");
    let _ = std::fs::remove_dir_all(&dir);
    if blocked.load(Ordering::SeqCst)
        && alive_before
        && stopped
        && stop_ok
        && blob.contains("task id:")
    {
        ("halted", turns)
    } else {
        ("failed: halt", turns)
    }
}

fn background_halt_cli(program: Option<&Path>) -> (&'static str, u32) {
    let Some(program) = program else {
        return (NO_FAKE_ACP, 0);
    };
    let Some(dir) = scratch("halt-cli") else {
        return ("failed: scratch", 0);
    };
    let done = cli_turn(
        program,
        &dir,
        vec![("FAKE_ACP_BLOCK_UNTIL_CANCEL".into(), "1".into())],
        SessionMode::Chat,
        true,
        false,
        "run in the background",
        CliAct::CancelAfterThought,
    );
    let _ = std::fs::remove_dir_all(&dir);
    match done {
        Ok(done) if done.stop == "cancelled" => ("halted", 1),
        Ok(_) => ("failed: cli", 1),
        Err(()) => ("failed: cli", 0),
    }
}

fn mcp_native(cfg: Option<&Path>) -> (&'static str, u32) {
    let Some(cfg) = cfg else {
        return ("failed: scratch", 0);
    };
    let Some(server) = sibling_bin("fake_mcp") else {
        return (NO_FAKE_MCP, 0);
    };
    let _guard = ConfigGuard::set(cfg);
    if write_mcp_config(cfg, &server).is_err() {
        return ("failed: mcp", 0);
    }
    crate::mcp::shutdown_all();
    crate::mcp::invalidate();
    let Some(dir) = scratch("mcp") else {
        clear_mcp(cfg);
        return ("failed: scratch", 0);
    };
    let client = Arc::new(Scripted::mcp());
    let shared: Arc<dyn ModelClient + Send + Sync> = client.clone();
    let done = run_native(
        &dir,
        "eval-mcp",
        shared,
        PermMode::Always,
        "call the echo tool",
    );
    let blob = client.blob();
    let turns = client.turns();
    clear_mcp(cfg);
    let _ = std::fs::remove_dir_all(&dir);
    if done.stop == "end_turn" && blob.contains("pid=") && blob.contains("args=") {
        ("echoed", turns)
    } else {
        ("failed: mcp", turns)
    }
}

fn mcp_cli(program: Option<&Path>) -> (&'static str, u32) {
    scripted_tool(program, "evalmcp__echo", "mcp-dry-run")
}

fn compact_native() -> (&'static str, u32) {
    let Some(dir) = scratch("compact") else {
        return ("failed: scratch", 0);
    };
    let history = long_history();
    if !crate::compact::needs_auto_compact(&history, crate::models::GROK_47_CONTEXT_LENGTH) {
        let _ = std::fs::remove_dir_all(&dir);
        return ("failed: compact", 0);
    }
    let client = Arc::new(Scripted::compact());
    let shared: Arc<dyn ModelClient + Send + Sync> = client.clone();
    let mut engine = native_engine(&dir, "eval-compact", shared, PermMode::Always, 4);
    engine.resume(history, Usage::default());
    let mut text = String::new();
    let mut started = false;
    let mut finished = false;
    let _ = engine.prompt("continue", None, &mut |ev| match ev {
        AcpEvent::Text(chunk) => text.push_str(&chunk),
        AcpEvent::Compact {
            started: true,
            error: None,
            ..
        } => started = true,
        AcpEvent::Compact {
            started: false,
            error: None,
            ..
        } => finished = true,
        _ => {}
    });
    let turns = client.turns();
    crate::tasks::forget_session("eval-compact");
    let _ = std::fs::remove_dir_all(&dir);
    if started && finished && text.contains("continued") {
        ("compacted", turns)
    } else {
        ("failed: compact", turns)
    }
}

fn compact_cli(program: Option<&Path>) -> (&'static str, u32) {
    let Some(program) = program else {
        return (NO_FAKE_ACP, 0);
    };
    let Some(dir) = scratch("compact-cli") else {
        return ("failed: scratch", 0);
    };
    let done = cli_turn(
        program,
        &dir,
        vec![("FAKE_ACP_COMPACT".into(), "1".into())],
        SessionMode::Chat,
        true,
        false,
        "continue",
        CliAct::None,
    );
    let _ = std::fs::remove_dir_all(&dir);
    match done {
        Ok(done) if done.compact_started && done.compact_done && done.stop == "end_turn" => {
            ("compacted", 1)
        }
        Ok(_) => ("failed: cli", 1),
        Err(()) => ("failed: cli", 0),
    }
}

fn imagine_native() -> (&'static str, u32) {
    let api = NopImagine;
    let before = imagine_send_count();
    let built = build_imagine_request(&api);
    let sends = imagine_send_count().saturating_sub(before);
    match built {
        Ok(body)
            if sends == 0
                && body.get("prompt").and_then(|value| value.as_str()) == Some(IMAGINE_PROMPT) =>
        {
            ("request built", 1)
        }
        _ => ("failed: imagine", 1),
    }
}

fn imagine_cli(program: Option<&Path>) -> (&'static str, u32) {
    scripted_tool(program, "image_generate", "imagine-dry-run")
}

fn scripted_tool(program: Option<&Path>, tool: &str, text: &str) -> (&'static str, u32) {
    let Some(program) = program else {
        return (NO_FAKE_ACP, 0);
    };
    let Some(dir) = scratch("tool") else {
        return ("failed: scratch", 0);
    };
    let done = cli_turn(
        program,
        &dir,
        vec![
            ("FAKE_ACP_TOOL".into(), tool.to_string()),
            ("FAKE_ACP_TEXT".into(), text.to_string()),
        ],
        SessionMode::Chat,
        true,
        false,
        "use the tool",
        CliAct::None,
    );
    let _ = std::fs::remove_dir_all(&dir);
    match done {
        Ok(done)
            if done.stop == "end_turn"
                && done.tools.iter().any(|title| title == tool)
                && done.text.contains(text) =>
        {
            ("scripted tool", 1)
        }
        Ok(_) => ("failed: cli", 1),
        Err(()) => ("failed: cli", 0),
    }
}

/// Build the Imagine generation body. `api` is not called.
pub fn build_imagine_request(api: &dyn ImagineApi) -> Result<Value, String> {
    let _ = api;
    let expect =
        grokhub_core::imagine_generation_body(IMAGINE_PROMPT, "", 1, "1k", "1:1", "", "url");
    let body = crate::tools::dry_imagine_body(&imagine_args())?;
    if body != expect {
        return Err("imagine body did not match the media builder".into());
    }
    Ok(body)
}

fn imagine_args() -> Value {
    json!({
        "prompt": IMAGINE_PROMPT,
        "n": 1,
        "resolution": "1k",
        "aspect_ratio": "1:1",
    })
}

struct NopImagine;

impl ImagineApi for NopImagine {
    fn post_json(&self, _path: &str, _body: &Value) -> Result<Value, String> {
        IMAGINE_SENDS.fetch_add(1, Ordering::SeqCst);
        Err("dry-run must not send".into())
    }

    fn get_json(&self, _path: &str) -> Result<Value, String> {
        IMAGINE_SENDS.fetch_add(1, Ordering::SeqCst);
        Err("dry-run must not send".into())
    }

    fn download(&self, _url: &str) -> Result<Vec<u8>, String> {
        IMAGINE_SENDS.fetch_add(1, Ordering::SeqCst);
        Err("dry-run must not send".into())
    }
}

struct NativeDone {
    stop: String,
    perms: u32,
}

fn run_native(
    dir: &Path,
    id: &str,
    client: Arc<dyn ModelClient + Send + Sync>,
    mode: PermMode,
    prompt: &str,
) -> NativeDone {
    run_native_cancel(dir, id, client, mode, prompt, CancelToken::new())
}

fn run_native_cancel(
    dir: &Path,
    id: &str,
    client: Arc<dyn ModelClient + Send + Sync>,
    mode: PermMode,
    prompt: &str,
    cancel: CancelToken,
) -> NativeDone {
    let done = crate::run_unattended(crate::UnattendedRun {
        client,
        workspace: dir.to_path_buf(),
        model: "grok-4.7".into(),
        effort: Some("high".into()),
        system: String::new(),
        session_id: id.to_string(),
        auth_kind: AuthKind::ApiKey,
        prompt: prompt.to_string(),
        image: None,
        mode,
        readonly_session: false,
        desktop: false,
        cancel,
        halt: Box::new(NoHalt),
        desktop_ops: None,
        imagine_bearer: String::new(),
        resume: false,
    });
    NativeDone {
        stop: done.stop_reason,
        perms: done.permission_cards,
    }
}

fn native_engine(
    dir: &Path,
    id: &str,
    client: Arc<dyn ModelClient + Send + Sync>,
    mode: PermMode,
    max_turns: u32,
) -> NativeEngine {
    NativeEngine::new(EngineParts {
        client,
        workspace: dir.to_path_buf(),
        model: "grok-4.7".into(),
        effort: Some("high".into()),
        system: String::new(),
        conversation_id: id.to_string(),
        auth_kind: AuthKind::ApiKey,
        max_turns,
        cancel: CancelToken::new(),
        steer: SteerQueue::new(),
        halt: Box::new(NoHalt),
        gate: Gate {
            mode,
            readonly_session: false,
            attended: false,
            desktop: false,
        },
        desktop: None,
        permits: Arc::new(ClosedPermits),
    })
}

struct NoHalt;

impl HaltCheck for NoHalt {
    fn halted(&self) -> bool {
        false
    }
}

enum Kind {
    Bugfix,
    Ask,
    Halt { blocked: Arc<AtomicBool> },
    Mcp,
    Compact,
}

struct Scripted {
    kind: Kind,
    n: AtomicUsize,
    blob: Mutex<String>,
}

impl Scripted {
    fn bugfix() -> Self {
        Self::new(Kind::Bugfix)
    }
    fn ask() -> Self {
        Self::new(Kind::Ask)
    }
    fn halt(blocked: Arc<AtomicBool>) -> Self {
        Self::new(Kind::Halt { blocked })
    }
    fn mcp() -> Self {
        Self::new(Kind::Mcp)
    }
    fn compact() -> Self {
        Self::new(Kind::Compact)
    }
    fn new(kind: Kind) -> Self {
        Self {
            kind,
            n: AtomicUsize::new(0),
            blob: Mutex::new(String::new()),
        }
    }
    fn turns(&self) -> u32 {
        u32::try_from(self.n.load(Ordering::SeqCst)).unwrap_or(u32::MAX)
    }
    fn blob(&self) -> String {
        self.blob
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .clone()
    }
    fn save_blob(&self, req: &ResponsesRequest) {
        *self.blob.lock().unwrap_or_else(|err| err.into_inner()) = request_blob(req);
    }
}

impl ModelClient for Scripted {
    fn stream(
        &self,
        req: &ResponsesRequest,
        cancel: &CancelToken,
        sink: &mut dyn FnMut(StreamEvent),
    ) -> Result<TurnOutput, ClientError> {
        let n = self.n.fetch_add(1, Ordering::SeqCst);
        if matches!(self.kind, Kind::Compact) && is_summary(req) {
            return Ok(text_turn("<summary>kept the plan</summary>"));
        }
        if let Kind::Halt { blocked } = &self.kind {
            if n > 0 {
                self.save_blob(req);
                blocked.store(true, Ordering::SeqCst);
                let start = Instant::now();
                loop {
                    if cancel.is_cancelled() {
                        return Err(ClientError::Cancelled);
                    }
                    if start.elapsed() > Duration::from_secs(8) {
                        return Err(ClientError::Protocol("halt did not arrive".into()));
                    }
                    thread::sleep(Duration::from_millis(10));
                }
            }
        }
        if n == 0 {
            return Ok(match &self.kind {
                Kind::Bugfix => tool_turn(
                    "edit",
                    "search_replace",
                    &json!({
                        "file_path": "lib.sh",
                        "old_string": "echo $(( $1 - $2 ))",
                        "new_string": "echo $(( $1 + $2 ))",
                    })
                    .to_string(),
                ),
                Kind::Ask => tool_turn(
                    "w",
                    "write",
                    &json!({"path": "secret.txt", "content": "nope"}).to_string(),
                ),
                Kind::Halt { .. } => tool_turn(
                    "bg",
                    "run_terminal_command",
                    &json!({"command": bg_command(), "is_background": true}).to_string(),
                ),
                Kind::Mcp => tool_turn("m", "evalmcp__echo", "{}"),
                Kind::Compact => {
                    sink(StreamEvent::TextDelta("continued".into()));
                    text_turn("continued")
                }
            });
        }
        self.save_blob(req);
        let text = match self.kind {
            Kind::Bugfix => "fixed",
            Kind::Ask => "refused",
            Kind::Mcp => "echoed",
            Kind::Compact => "continued",
            Kind::Halt { .. } => "halted",
        };
        sink(StreamEvent::TextDelta(text.into()));
        Ok(text_turn(text))
    }
}

fn tool_turn(id: &str, name: &str, arguments: &str) -> TurnOutput {
    TurnOutput {
        text: String::new(),
        reasoning: String::new(),
        calls: vec![FunctionCall {
            call_id: id.to_string(),
            name: name.to_string(),
            arguments: arguments.to_string(),
        }],
        usage: Usage::default(),
    }
}

fn text_turn(text: &str) -> TurnOutput {
    TurnOutput {
        text: text.to_string(),
        reasoning: String::new(),
        calls: Vec::new(),
        usage: Usage::default(),
    }
}

fn is_summary(req: &ResponsesRequest) -> bool {
    req.input
        .iter()
        .any(|item| crate::compact::message_text(item).contains("faithful, concise summary"))
}

fn request_blob(req: &ResponsesRequest) -> String {
    let mut out = String::new();
    for item in &req.input {
        match item {
            InputItem::Message { content, .. } => {
                for part in content {
                    if let ContentPart::InputText(text) = part {
                        out.push_str(text);
                        out.push('\n');
                    }
                }
            }
            InputItem::FunctionCallOutput { output, .. } => {
                out.push_str(output);
                out.push('\n');
            }
            InputItem::FunctionCall {
                name, arguments, ..
            } => {
                out.push_str(name);
                out.push(' ');
                out.push_str(arguments);
                out.push('\n');
            }
        }
    }
    out
}

fn long_history() -> Vec<InputItem> {
    vec![InputItem::Message {
        role: "user".into(),
        content: vec![ContentPart::InputText("x".repeat(900_000))],
    }]
}

fn write_bugfix_fixture(dir: &Path) -> Result<(), String> {
    std::fs::write(dir.join("lib.sh"), "add() {\n  echo $(( $1 - $2 ))\n}\n")
        .map_err(|err| err.to_string())?;
    std::fs::write(
        dir.join("test.sh"),
        "#!/bin/bash\nset -eu\n. ./lib.sh\nresult=$(add 2 3)\ntest \"$result\" = 5\nprintf '%s\\n' ok\n",
    )
    .map_err(|err| err.to_string())
}

fn fixture_passes(dir: &Path) -> bool {
    let Ok(output) = Command::new("bash")
        .arg("test.sh")
        .current_dir(dir)
        .output()
    else {
        return false;
    };
    output.status.success() && String::from_utf8_lossy(&output.stdout).contains("ok")
}

fn write_mcp_config(cfg: &Path, server: &Path) -> Result<(), String> {
    std::fs::create_dir_all(cfg).map_err(|err| err.to_string())?;
    let body = json!({
        "mcpServers": {
            "evalmcp": {
                "command": server.display().to_string(),
                "args": []
            }
        }
    });
    std::fs::write(cfg.join("mcp.json"), body.to_string()).map_err(|err| err.to_string())
}

fn clear_mcp(cfg: &Path) {
    let _ = std::fs::remove_file(cfg.join("mcp.json"));
    crate::mcp::shutdown_all();
    crate::mcp::invalidate();
}

fn bg_command() -> &'static str {
    // A single plain command. `$`, redirects, and `;` do not split cleanly, and
    // an unsplittable shell call is refused while unattended even in Always mode.
    #[cfg(unix)]
    {
        "sleep 30"
    }
    #[cfg(not(unix))]
    {
        "ping -n 31 127.0.0.1"
    }
}

fn task_id_from(blob: &str) -> Option<String> {
    let line = blob.lines().find(|line| line.contains("task id:"))?;
    let rest = line.split("task id:").nth(1)?.trim();
    let id = rest.split_whitespace().next()?.trim();
    if id.is_empty() {
        None
    } else {
        Some(id.to_string())
    }
}

fn task_text(session: &str, blob: &str) -> Option<String> {
    let id = task_id_from(blob)?;
    crate::tasks::hub_for(session).read_output(&id, 0).ok()
}

fn command_is_live(dir: &Path, session: &str, blob: &str) -> bool {
    let running = task_text(session, blob).is_some_and(|text| text.contains("status: running"));
    if !running {
        return false;
    }
    #[cfg(unix)]
    {
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(2) {
            let pids = workspace_pids(dir);
            if !pids.is_empty() && pids.iter().copied().any(process_alive) {
                return true;
            }
            thread::sleep(Duration::from_millis(20));
        }
        false
    }
    #[cfg(not(unix))]
    {
        let _ = dir;
        true
    }
}

fn command_is_stopped(session: &str, blob: &str, pids: &[u32]) -> bool {
    #[cfg(unix)]
    {
        let _ = (session, blob);
        if pids.is_empty() {
            return false;
        }
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(2) {
            if pids.iter().copied().all(|pid| !process_alive(pid)) {
                return true;
            }
            thread::sleep(Duration::from_millis(20));
        }
        pids.iter().copied().all(|pid| !process_alive(pid))
    }
    #[cfg(not(unix))]
    {
        let _ = pids;
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(2) {
            if task_text(session, blob).is_some_and(|text| text.contains("status: killed")) {
                return true;
            }
            thread::sleep(Duration::from_millis(20));
        }
        false
    }
}

fn workspace_pids(dir: &Path) -> Vec<u32> {
    #[cfg(unix)]
    {
        let Ok(canon) = dir.canonicalize() else {
            return Vec::new();
        };
        let Ok(entries) = std::fs::read_dir("/proc") else {
            return Vec::new();
        };
        let self_pid = std::process::id();
        let mut pids = Vec::new();
        for entry in entries.flatten() {
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            let Ok(pid) = name.parse::<u32>() else {
                continue;
            };
            if pid == 0 || pid == self_pid {
                continue;
            }
            let Ok(cwd) = std::fs::read_link(entry.path().join("cwd")) else {
                continue;
            };
            if cwd == canon {
                pids.push(pid);
            }
        }
        pids
    }
    #[cfg(not(unix))]
    {
        let _ = dir;
        Vec::new()
    }
}

#[cfg(unix)]
fn process_alive(pid: u32) -> bool {
    let pid = pid as i32;
    if pid <= 0 {
        return false;
    }
    // Signal 0 checks that the process exists. It does not deliver a signal.
    let rc = unsafe { libc::kill(pid, 0) };
    if rc == 0 {
        return true;
    }
    std::io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH)
}

#[cfg(unix)]
fn force_kill(pid: u32) {
    if pid > 0 && process_alive(pid) {
        unsafe { libc::kill(pid as i32, libc::SIGKILL) };
    }
}

fn cleanup_sessions() {
    for id in [
        "eval-bugfix",
        "eval-ask",
        "eval-bg-halt",
        "eval-mcp",
        "eval-compact",
    ] {
        crate::tasks::forget_session(id);
    }
    crate::mcp::shutdown_all();
}

struct CliDone {
    text: String,
    stop: String,
    tools: Vec<String>,
    perms: u32,
    compact_started: bool,
    compact_done: bool,
}

enum CliAct {
    None,
    Reject,
    CancelAfterThought,
}

fn cli_turn(
    program: &Path,
    cwd: &Path,
    extra: Vec<(String, String)>,
    mode: SessionMode,
    always_approve: bool,
    auto: bool,
    prompt: &str,
    act: CliAct,
) -> Result<CliDone, ()> {
    let _ = std::fs::create_dir_all(cwd);
    let handle = connect(SpawnOpts {
        program: program.to_path_buf(),
        args: Vec::new(),
        cwd: cwd.to_path_buf(),
        api_key: None,
        xai_api_key: None,
        always_approve,
        auto,
        session_mode: mode,
        reasoning_effort: None,
        extra_env: extra,
        handshake_timeout: Some(Duration::from_secs(5)),
        resume: None,
        skip_cabin_home: true,
        worktree: false,
    })
    .map_err(|_| ())?;
    handle.prompt(prompt).map_err(|_| ())?;
    let mut done = CliDone {
        text: String::new(),
        stop: String::new(),
        tools: Vec::new(),
        perms: 0,
        compact_started: false,
        compact_done: false,
    };
    let deadline = Instant::now() + Duration::from_secs(8);
    let mut acted = false;
    while Instant::now() < deadline {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            break;
        }
        match wait_event(&handle.events, left.min(Duration::from_secs(2))) {
            Ok(AcpEvent::Ready { .. }) | Ok(AcpEvent::Usage(_)) => {}
            Ok(AcpEvent::Thought(_)) => {
                if matches!(act, CliAct::CancelAfterThought) && !acted {
                    acted = true;
                    let _ = handle.cancel();
                }
            }
            Ok(AcpEvent::Text(text)) => done.text.push_str(&text),
            Ok(AcpEvent::Tool(card)) => done.tools.push(card.title),
            Ok(AcpEvent::Permission(ask)) => {
                done.perms = done.perms.saturating_add(1);
                if matches!(act, CliAct::Reject) {
                    let _ = handle.reject_permission(&ask);
                }
            }
            Ok(AcpEvent::Compact {
                started: true,
                error: None,
                ..
            }) => done.compact_started = true,
            Ok(AcpEvent::Compact {
                started: false,
                error: None,
                ..
            }) => done.compact_done = true,
            Ok(AcpEvent::Done { stop_reason }) => {
                done.stop = stop_reason;
                return Ok(done);
            }
            Ok(AcpEvent::Err(_)) => return Err(()),
            Ok(_) => {}
            Err(_) => {
                if !done.stop.is_empty() {
                    return Ok(done);
                }
            }
        }
    }
    if done.stop.is_empty() {
        Err(())
    } else {
        Ok(done)
    }
}

fn locate_fake_acp(explicit: Option<&Path>) -> Option<PathBuf> {
    if let Some(path) = explicit {
        return path.is_file().then(|| path.to_path_buf());
    }
    if let Some(path) = env_file("GROKHUB_EVAL_FAKE_ACP") {
        return Some(path);
    }
    sibling_bin("grokhub-fake-acp")
}

fn env_file(key: &str) -> Option<PathBuf> {
    let raw = std::env::var(key).ok()?;
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    let path = PathBuf::from(raw);
    path.is_file().then_some(path)
}

fn sibling_bin(name: &str) -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let file = format!("{name}{}", std::env::consts::EXE_SUFFIX);
    exe.ancestors().take(8).find_map(|dir| {
        let candidate = dir.join(&file);
        candidate.is_file().then_some(candidate)
    })
}

fn scratch(label: &str) -> Option<PathBuf> {
    let n = SCRATCH_SEQ.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("gh-eval-{label}-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::Ordering;

    struct Rec {
        posts: AtomicUsize,
        gets: AtomicUsize,
        downloads: AtomicUsize,
    }

    impl ImagineApi for Rec {
        fn post_json(&self, _path: &str, _body: &Value) -> Result<Value, String> {
            self.posts.fetch_add(1, Ordering::SeqCst);
            Ok(Value::Null)
        }
        fn get_json(&self, _path: &str) -> Result<Value, String> {
            self.gets.fetch_add(1, Ordering::SeqCst);
            Ok(Value::Null)
        }
        fn download(&self, _url: &str) -> Result<Vec<u8>, String> {
            self.downloads.fetch_add(1, Ordering::SeqCst);
            Err("not sent".into())
        }
    }

    #[test]
    fn dry_run_is_the_default() {
        let opts = parse_args(&[]).expect("default");
        assert!(!opts.live);
        assert_eq!(opts.budget_cents, None);
        assert!(opts.out.is_none());
        let flagged = parse_args(&["--dry-run".into()]).expect("dry-run");
        assert!(!flagged.live);
    }

    #[test]
    fn live_refuses_without_api_key() {
        let err = parse_args_with_key(&["--live".into()], None).expect_err("no key");
        assert_eq!(err, "refusing --live: GROKHUB_EVAL_API_KEY is not set");
    }

    #[test]
    fn live_refuses_when_api_key_is_empty() {
        let err = parse_args_with_key(
            &["--live".into(), "--budget-usd".into(), "0.20".into()],
            Some("   "),
        )
        .expect_err("empty");
        assert_eq!(err, "refusing --live: GROKHUB_EVAL_API_KEY is not set");
    }

    #[test]
    fn live_refuses_with_key_but_no_budget() {
        let err = parse_args_with_key(&["--live".into()], Some("sanctioned-test-key"))
            .expect_err("no budget");
        assert_eq!(
            err,
            "refusing --live: --budget-usd is required and must satisfy 0 < amount <= 0.20"
        );
    }

    #[test]
    fn live_refuses_budget_above_cap() {
        let err = parse_args_with_key(
            &["--live".into(), "--budget-usd".into(), "0.25".into()],
            Some("sanctioned-test-key"),
        )
        .expect_err("cap");
        assert_eq!(
            err,
            "refusing --live: --budget-usd 0.25 is above the 0.20 cap"
        );
    }

    #[test]
    fn live_refuses_budget_zero() {
        let err = parse_args_with_key(
            &["--live".into(), "--budget-usd".into(), "0".into()],
            Some("sanctioned-test-key"),
        )
        .expect_err("zero");
        assert_eq!(
            err,
            "refusing --live: --budget-usd must satisfy 0 < amount <= 0.20"
        );
    }

    #[test]
    fn live_refuses_budget_negative() {
        let err = parse_args_with_key(
            &["--live".into(), "--budget-usd".into(), "-1".into()],
            Some("sanctioned-test-key"),
        )
        .expect_err("neg");
        assert_eq!(
            err,
            "refusing --live: --budget-usd must satisfy 0 < amount <= 0.20"
        );
    }

    #[test]
    fn live_with_key_and_budget_is_not_implemented() {
        let opts = parse_args_with_key(
            &["--live".into(), "--budget-usd".into(), "0.20".into()],
            Some("sanctioned-test-key"),
        )
        .expect("parsed");
        assert!(opts.live);
        assert_eq!(opts.budget_cents, Some(20));
        let err = reject_live(&opts).expect_err("unimplemented");
        assert_eq!(err, "live mode is not implemented in this build");
        assert!(run_suite(&opts).is_empty());
    }

    #[test]
    fn imagine_dry_run_sends_nothing() {
        let api = Rec {
            posts: AtomicUsize::new(0),
            gets: AtomicUsize::new(0),
            downloads: AtomicUsize::new(0),
        };
        let body = build_imagine_request(&api).expect("body");
        let expect =
            grokhub_core::imagine_generation_body("a red square", "", 1, "1k", "1:1", "", "url");
        assert_eq!(body, expect);
        assert_eq!(body["prompt"], json!("a red square"));
        assert_eq!(body["n"], json!(1));
        assert_eq!(api.posts.load(Ordering::SeqCst), 0);
        assert_eq!(api.gets.load(Ordering::SeqCst), 0);
        assert_eq!(api.downloads.load(Ordering::SeqCst), 0);
        assert_eq!(imagine_send_count(), 0);
    }

    #[test]
    fn dry_run_report_lists_every_item_and_is_byte_identical() {
        let opts = parse_args(&[]).expect("dry-run");
        assert!(!opts.live);
        let first = run_suite(&opts);
        let second = run_suite(&opts);
        let report = render_report(&first);
        let again = render_report(&second);
        assert_eq!(report.as_bytes(), again.as_bytes());
        assert_eq!(first.len(), SUITE_ITEMS.len() * 2);
        for name in SUITE_ITEMS {
            let needle_native = format!("| {name} | native |");
            let needle_cli = format!("| {name} | cli |");
            assert_eq!(report.matches(&needle_native).count(), 1, "{report}");
            assert_eq!(report.matches(&needle_cli).count(), 1, "{report}");
        }
        assert!(report.contains("## GAPS\n"));
        let gaps = report.split("## GAPS\n\n").nth(1).expect("gaps body");
        for line in STANDING_GAPS {
            assert_eq!(gaps.matches(line).count(), 1, "{line}");
        }
        assert!(gaps.starts_with("- live: `--live` is a stub."), "{gaps}");
        assert!(!gaps.contains("- none"));
        assert!(report.contains("No default was switched.\n"));
        assert!(report.contains("GROKHUB_EVAL_API_KEY"));
        assert!(report.contains("0.20"));
        assert!(report.contains(&credential_boundary()));
        assert!(report.contains("| dry-run |"));
        assert!(!report.contains("gh-eval"));
        assert!(!report.contains("/tmp"));
        assert!(report.contains("| imagine | native | yes | 1 | dry-run | 0 | 0 | request built |"));
        assert_eq!(imagine_send_count(), 0);
        assert!(report.contains("| repo-bugfix | native | yes |"));
        assert!(report.contains("| ask-refusal | native | yes |"));
        assert!(report.contains("| background-halt | native | yes |"));
        assert!(report.contains("| compaction | native | yes |"));
        assert!(report.contains("| mcp-tool | native | yes |"));
        if report.contains(SKIP_XVFB) {
            assert!(report.contains("- xvfb-desktop: skipped: no Xvfb\n"));
        }
        let ci = std::env::var("CI").ok().as_deref() == Some("true");
        if ci {
            assert!(
                !report.contains(NO_FAKE_ACP),
                "CI requires the grokhub-fake-acp binary\n{report}"
            );
        }
        if !report.contains(NO_FAKE_ACP) {
            assert!(report.contains("| repo-bugfix | cli | yes |"));
            assert!(report.contains("| ask-refusal | cli | yes |"));
            assert!(report.contains("| background-halt | cli | yes |"));
            assert!(report.contains("| mcp-tool | cli | yes |"));
            assert!(report.contains("| compaction | cli | yes |"));
            assert!(report.contains("| imagine | cli | yes |"));
        }
    }
}
