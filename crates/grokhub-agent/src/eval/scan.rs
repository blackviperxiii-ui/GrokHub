//! `system-scan` (card 34 PR D): the 2026-10-09 beta session where a system
//! scan under Always Allow was mostly permission cards, stopped at "it's
//! resolved", and ended with a shallow report. A fake host (fixture
//! `sudo`, `systemctl`, `ss`, `pacman`, `journalctl`, `ufw`) answers the
//! real shell tool; a scripted model stops early once and builds its card
//! from what the tools printed. Passes when no permission card showed, the
//! drive pushed the early stop on, the checklist ended covered, and the
//! findings card names systemd-resolved on port 53 and offers the 4 fixes.

use std::path::Path;
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use std::sync::Mutex;

use serde_json::json;

use crate::client::{ClientError, ResponsesRequest, StreamEvent, TurnOutput};
use crate::gate::{Gate, PermAnswer, PermMode, PermitWait, Waited};
use crate::{CancelToken, LoopEvent, LoopIn, ModelClient, SteerQueue, StopReason, Usage};

/// The 4 one-tap fixes the card must offer.
pub(super) const FIXES: [&str; 4] =
    ["Fix port 53 conflict", "Reinstall 6 packages", "Investigate Bluetooth errors", "Explain all"];

const FIXTURES: &[(&str, &str)] = &[
    ("sudo", "#!/bin/sh\nexec \"$@\"\n"),
    (
        "systemctl",
        "#!/bin/sh\necho '  UNIT             LOAD   ACTIVE SUB    DESCRIPTION'\necho '* nextdns.service loaded failed failed NextDNS DNS53 to DoH proxy.'\necho 'nextdns[812]: listen tcp 127.0.0.1:53: bind: address already in use'\n",
    ),
    (
        "ss",
        "#!/bin/sh\necho 'State  Recv-Q Send-Q Local Address:Port Peer Address:Port Process'\necho 'LISTEN 0      4096   127.0.0.53%lo:53    0.0.0.0:*     users:((\"systemd-resolve\",pid=421,fd=18))'\n",
    ),
    (
        "pacman",
        "#!/bin/sh\nfor p in amd-ucode bind cups linux-cachyos linux-cachyos-lts nfs-utils; do echo \"warning: $p: /usr/share/$p/x (No such file or directory)\"; done\necho '6 packages with missing files'\n",
    ),
    (
        "journalctl",
        "#!/bin/sh\necho 'bluetoothd[640]: Failed to set mode: Failed (0x03)'\necho 'pipewire[901]: spa.alsa: hw:0: snd_pcm_open failed'\necho '20 boot errors'\n",
    ),
    ("ufw", "#!/bin/sh\necho 'Status: active'\n"),
];

const CHECKLIST: &str = r#"{"todos":[
 {"id":"svc","content":"failed services","status":"in_progress"},
 {"id":"ports","content":"open ports and who owns them","status":"pending"},
 {"id":"pkgs","content":"package integrity","status":"pending"},
 {"id":"boot","content":"boot and journal errors","status":"pending"},
 {"id":"sec","content":"firewall","status":"pending"}]}"#;

/// Answers every permission card with Deny and counts it.
struct CountCards(AtomicU32);

impl PermitWait for CountCards {
    fn wait(&self, _call_id: &str, _cancel: &CancelToken, _halted: &dyn Fn() -> bool) -> Waited {
        self.0.fetch_add(1, Ordering::SeqCst);
        Waited::Answer(PermAnswer::Deny)
    }
}

struct ScanModel {
    bin: String,
    n: AtomicUsize,
    /// Every request's text, for the checks.
    seen: Mutex<String>,
}

impl ScanModel {
    fn sh(&self, id: &str, command: &str) -> crate::client::FunctionCall {
        let command = format!("PATH='{}':\"$PATH\" {command}", self.bin);
        call(id, "run_terminal_command", &json!({ "command": command }).to_string())
    }
}

fn call(id: &str, name: &str, arguments: &str) -> crate::client::FunctionCall {
    crate::client::FunctionCall { call_id: id.into(), name: name.into(), arguments: arguments.into() }
}

fn calls(calls: Vec<crate::client::FunctionCall>) -> TurnOutput {
    TurnOutput { text: String::new(), reasoning: String::new(), calls, usage: Usage::default() }
}

fn done(todos: &[&str]) -> crate::client::FunctionCall {
    let rows: Vec<_> = todos.iter().map(|id| json!({"id": id, "status": "completed"})).collect();
    call(&format!("done-{}", todos.join("-")), "todo_write", &json!({"merge": true, "todos": rows}).to_string())
}

impl ModelClient for ScanModel {
    fn stream(
        &self,
        req: &ResponsesRequest,
        _cancel: &CancelToken,
        sink: &mut dyn FnMut(StreamEvent),
    ) -> Result<TurnOutput, ClientError> {
        let blob = super::request_blob(req);
        let n = self.n.fetch_add(1, Ordering::SeqCst);
        *self.seen.lock().unwrap_or_else(|e| e.into_inner()) = blob.clone();
        Ok(match n {
            0 => calls(vec![call("plan", "todo_write", CHECKLIST), self.sh("svc", "sudo systemctl --failed")]),
            // The early stop from the session.
            1 => {
                sink(StreamEvent::TextDelta("It's resolved.".into()));
                super::text_turn("It's resolved.")
            }
            2 => calls(vec![
                self.sh("ports", "sudo ss -ltnp"),
                self.sh("pkgs", "sudo pacman -Qk"),
                self.sh("boot", "sudo journalctl -b -p err"),
                done(&["svc", "ports", "pkgs", "boot"]),
            ]),
            3 => calls(vec![self.sh("sec", "sudo ufw status"), done(&["sec"])]),
            4 => {
                // The card says only what the tools showed.
                let owner = if blob.contains("users:((\"systemd-resolve\"") { "systemd-resolved owns it" } else { "cause unknown" };
                let pkgs = if blob.contains("6 packages with missing files") { "6 packages have missing files" } else { "packages not checked" };
                let card = json!({
                    "findings": [
                        {"severity": "high", "text": format!("nextdns can't bind port 53: {owner}")},
                        {"severity": "medium", "text": pkgs},
                        {"severity": "low", "text": "20 boot errors (Bluetooth, PipeWire)"}
                    ],
                    "fixes": [
                        {"label": FIXES[0], "goal": "Stop systemd-resolved's stub listener on port 53, restart nextdns and confirm it is running."},
                        {"label": FIXES[1], "goal": "Reinstall the 6 packages with missing files and re-run pacman -Qk."},
                        {"label": FIXES[2], "goal": "Find the cause of the Bluetooth boot errors."},
                        {"label": FIXES[3], "goal": "Explain each finding in plain words."}
                    ]
                });
                calls(vec![call("card", "report_findings", &card.to_string())])
            }
            _ => {
                sink(StreamEvent::TextDelta("Scan done.".into()));
                super::text_turn("Scan done.")
            }
        })
    }
}

fn write_host(bin: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::create_dir_all(bin)?;
    for (name, body) in FIXTURES {
        let path = bin.join(name);
        std::fs::write(&path, body)?;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))?;
    }
    Ok(())
}

pub(super) fn system_scan_native() -> (&'static str, u32) {
    let Some(dir) = super::scratch("scan") else {
        return ("failed: scratch", 0);
    };
    let bin = dir.join("bin");
    if write_host(&bin).is_err() {
        return ("failed: scratch", 0);
    }
    let model = ScanModel { bin: bin.to_string_lossy().into_owned(), n: AtomicUsize::new(0), seen: Mutex::new(String::new()) };
    let cards = CountCards(AtomicU32::new(0));
    let session = format!("eval-scan-{}", std::process::id());
    let policy = crate::perm::Policy::load(&dir);
    let (cancel, steer) = (CancelToken::new(), SteerQueue::new());
    let input = LoopIn {
        client: &model,
        workspace: &dir,
        model: "grok-4.7",
        effort: Some("high"),
        system: "",
        conversation_id: &session,
        max_turns: 20,
        usage_base: Usage::default(),
        cancel: &cancel,
        steer: &steer,
        halt: &super::NoHalt,
        gate: Gate { mode: PermMode::Always, readonly_session: false, attended: true, desktop: false },
        desktop: None,
        permits: &cards,
        perms: Some(&policy),
        context_length: 0,
        tasks: None,
        depth: 0,
        agent_id: None,
        shared_client: None,
        shared_permits: None,
        shared_desktop: None,
    };
    let mut history = Vec::new();
    let mut card = None;
    let out = crate::run_loop(
        &input,
        &mut history,
        "scan my computer for app gaps, security issues and performance improvements",
        None,
        &mut |ev| {
            if let LoopEvent::Findings(body) = ev {
                card = Some(body);
            }
        },
    );
    let turns = u32::try_from(model.n.load(Ordering::SeqCst)).unwrap_or(u32::MAX);
    let seen = model.seen.lock().unwrap_or_else(|e| e.into_inner()).clone();
    let todos = crate::session_tools::todos_for(&session);
    let _ = std::fs::remove_dir_all(&dir);
    let card = card.as_deref().and_then(grokhub_core::findings::Findings::parse);
    if out.stop != StopReason::EndTurn {
        ("failed: turn did not end", turns)
    } else if cards.0.load(Ordering::SeqCst) != 0 {
        ("failed: permission card under Always", turns)
    } else if !seen.contains("Not done yet: \"failed services\", \"open ports and who owns them\", \"package integrity\", \"boot and journal errors\", \"firewall\".") {
        ("failed: early stop not pushed on", turns)
    } else if todos.len() != 5 || todos.iter().any(|t| t.status != "completed") {
        ("failed: checklist not covered", turns)
    } else if !card.as_ref().is_some_and(|c| c.items.first().is_some_and(|f| f.text == "nextdns can't bind port 53: systemd-resolved owns it")) {
        ("failed: port 53 root cause", turns)
    } else if card.as_ref().map(|c| c.fixes.iter().map(|f| f.label.as_str()).collect::<Vec<_>>()) != Some(FIXES.to_vec()) {
        ("failed: findings card fixes", turns)
    } else {
        ("covered", turns)
    }
}
