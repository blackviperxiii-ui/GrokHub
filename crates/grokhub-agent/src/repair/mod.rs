//! Spike-8b diagnose (pillar P6 step 1): "something's wrong with my computer"
//! runs a fixed list of read-only probes and answers in plain words.
//! Spike-9 fixes what it found: `fix` drafts a plain-language plan per
//! finding with every step classified by rules, `restore` takes a restore
//! point, and `apply` hands soft steps to Grok Build one at a time, parks hard
//! ones, never runs hard-floor ones, re-checks, and undoes.
//!
//! Every probe goes through `harness::decide` twice: `Step::Scope` for
//! `system_state` (nothing runs without the user's grant), then the probe's
//! fixed command line as a tool step. Probes are ids, never commands, run with
//! no shell, killed at 10 s, capped at 64 KB, and redacted before their output
//! reaches a span or the model. Spans carry `origin: repair`. Nothing elevates:
//! a probe that needs admin is skipped with a note.

mod apply;
mod fix;
mod interpret;
mod probes;
mod restore;
mod run;

use std::path::Path;

use serde_json::{json, Value};

use crate::harness::{self, AccessMode, ConsentLedger, GateOutcome, Origin, Scope, Span, Step};

pub use interpret::{
    interpret, Finding, ProbeOutput, Severity, DISK_CRIT_PCT, DISK_WARN_PCT, LOG_WARN_LINES, MEM_WARN_PCT,
};
pub use probes::{
    on_path, probe_spec, probe_spec_with, read_only_violation, Os, PackageManager, ProbeId, ProbeSpec,
    LINUX_PROBES, LOG_LINE_CAP, OUTPUT_CAP, PROBE_TIMEOUT, WINDOWS_PROBES,
};
pub use run::{redact_output, run_spec, ProbeRun};
pub use apply::{
    gb_prompt, step_ok, Action, ApplyCtx, ApplyRun, APPLY_TOOL, REPAIR_TOOL, RESTORE_TOOL, STEP_FAILED, STEP_OK,
    UNATTENDED, UNDO_TOOL,
};
pub(crate) use fix::repair_hit;
pub use fix::{classify_step, elevate, gate_text, plans_for, DraftStep, FixPlan, FixStep, PlanError, StepClass, FLOOR_GUIDANCE};
pub use restore::{
    backup_dir, backup_files, detect_backends, restore_files, BackupEntry, FileBackup, RestorePoint, Snapshot,
    SnapshotBackend, UndoReport,
};

/// The tool name in the native registry and in every probe span.
pub const DIAGNOSE_TOOL: &str = "diagnose";
/// What the user reads when `system_state` is off. No probe has run.
pub const SCOPE_ASK: &str = "To check your computer I need to read its system state: disk space, services, logs and updates, read only. Allow it on the card below (or turn on \"System state\" in Settings → Permissions), then ask me again.";
/// The why line on the Spike-8a ask card diagnose posts when the scope is off.
pub const SCOPE_ASK_WHY: &str = "To check what's wrong I'd read your disk space, services, logs and updates. Read only; nothing is changed.";
/// The note on a probe that would need root or admin.
pub const NEEDS_ADMIN: &str = "needs admin, skipped";
/// Span result text kept per probe.
const SPAN_RESULT_CAP: usize = 4096;

/// What diagnose needs from its caller. Tests swap `runner` and `has_bin`.
pub struct DiagnoseCtx<'a> {
    pub config_dir: &'a Path,
    pub session_id: &'a str,
    pub ledger: &'a ConsentLedger,
    pub access: AccessMode,
    pub os: Os,
    pub has_bin: &'a dyn Fn(&str) -> bool,
    pub runner: &'a dyn Fn(&ProbeSpec) -> ProbeRun,
}

/// The outcome of one diagnose run.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DiagnoseReport {
    /// Set when `system_state` is off: the ask to show, and no probe ran.
    pub ask: Option<String>,
    /// Worst first.
    pub findings: Vec<Finding>,
    /// Probes that actually ran.
    pub ran: Vec<ProbeId>,
}

/// Run `probes` (all of this OS's when empty) under the gate.
pub fn diagnose(ctx: &DiagnoseCtx<'_>, probes: &[ProbeId]) -> DiagnoseReport {
    let mut report = DiagnoseReport::default();
    let scope_args = json!({ "scope": "system_state" }).to_string();
    let scope = decide_scope(ctx.ledger);
    if let GateOutcome::Refuse { reason } | GateOutcome::Park { reason, .. } = &scope {
        let span = Span::deny(ctx.session_id, DIAGNOSE_TOOL, &scope_args, reason, "scope");
        write_span(ctx, span);
        report.ask = Some(SCOPE_ASK.into());
        return report;
    }
    let grant = ctx
        .ledger
        .scope_grant(&Scope::SystemState)
        .map(|g| g.id.clone())
        .unwrap_or_default();
    let mut wanted: Vec<ProbeId> = if probes.is_empty() { ProbeId::all(ctx.os).to_vec() } else { probes.to_vec() };
    wanted.retain(|p| ProbeId::all(ctx.os).contains(p));
    wanted.dedup();
    for id in wanted {
        let label = id.label();
        let Some(spec) = probe_spec_with(id, ctx.os, ctx.has_bin) else {
            report.findings.push(note(id, format!("I couldn't check {label}: no supported tool for it is installed.")));
            continue;
        };
        let line = spec.command_line();
        let args = json!({ "probe": id.key(), "command": line }).to_string();
        if let Some(why) = read_only_violation(&spec) {
            write_span(ctx, Span::deny(ctx.session_id, DIAGNOSE_TOOL, &args, &why, "floor"));
            continue;
        }
        let shell = json!({ "command": line }).to_string();
        let step = harness::decide(Step::Tool { name: "run_terminal_command", arguments: &shell });
        if !step.is_allow() || !crate::gate::is_readonly(DIAGNOSE_TOOL) {
            let why = match step {
                GateOutcome::Refuse { reason } | GateOutcome::Park { reason, .. } => reason,
                GateOutcome::Allow => "diagnose is not a read tool".into(),
            };
            write_span(ctx, Span::deny(ctx.session_id, DIAGNOSE_TOOL, &args, &why, "gate"));
            continue;
        }
        if spec.needs_admin {
            let mut span = Span::soft_allow(ctx.session_id, DIAGNOSE_TOOL, &args, NEEDS_ADMIN, label, ctx.access, "repair");
            span.decision = "skip".into();
            write_span(ctx, span.with_consent(&grant));
            report.findings.push(note(id, format!("Checking {label} {NEEDS_ADMIN}.")));
            continue;
        }
        let (result, found) = match (ctx.runner)(&spec) {
            ProbeRun::Done { code, stdout, stderr } => {
                let out = ProbeOutput {
                    program: spec.program.clone(),
                    code,
                    stdout: redact_output(&stdout, id.is_log()),
                    stderr: redact_output(&stderr, id.is_log()),
                };
                let mut found = interpret(id, &out);
                if found.is_empty() {
                    found.push(note(id, format!("I couldn't check {label} on this computer.")));
                }
                let code = code.map(|c| c.to_string()).unwrap_or_else(|| "signal".into());
                (cap(&format!("exit {code}\n{}{}", out.stdout, out.stderr)), found)
            }
            ProbeRun::TimedOut => ("timeout".into(), vec![note(id, format!("Checking {label} took too long, so I stopped it."))]),
            ProbeRun::Missing(why) => (
                cap(&format!("missing: {}", redact_output(&why, false))),
                vec![note(id, format!("I couldn't check {label} on this computer."))],
            ),
        };
        let span = Span::soft_allow(ctx.session_id, DIAGNOSE_TOOL, &args, &result, label, ctx.access, "repair");
        write_span(ctx, span.with_consent(&grant));
        report.ran.push(id);
        report.findings.extend(found);
    }
    report.findings.sort_by_key(|f| std::cmp::Reverse(f.severity));
    report
}

/// The scope check every diagnose run starts with.
fn decide_scope(ledger: &ConsentLedger) -> GateOutcome {
    harness::decide(Step::Scope { scope: &Scope::SystemState, ledger })
}

fn note(probe: ProbeId, plain: String) -> Finding {
    Finding {
        severity: Severity::Info,
        plain,
        detail: String::new(),
        probe,
    }
}

fn cap(text: &str) -> String {
    if text.len() <= SPAN_RESULT_CAP {
        return text.to_string();
    }
    let mut end = SPAN_RESULT_CAP;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &text[..end])
}

fn write_span(ctx: &DiagnoseCtx<'_>, span: Span) {
    let _ = harness::append_span(ctx.config_dir, &span.from_origin(Origin::Repair));
}

/// The chat answer: plain sentences first, never a command.
pub fn report_text(report: &DiagnoseReport) -> String {
    if let Some(ask) = &report.ask {
        return ask.clone();
    }
    let problems: Vec<&Finding> = report.findings.iter().filter(|f| f.severity >= Severity::Warning).collect();
    let notes: Vec<&Finding> = report.findings.iter().filter(|f| f.severity == Severity::Info).collect();
    let fine: Vec<&Finding> = report.findings.iter().filter(|f| f.severity == Severity::Ok).collect();
    let mut out = match problems.len() {
        0 if report.findings.is_empty() => "I couldn't run any checks on this computer.".to_string(),
        0 => "I checked your computer and didn't find anything wrong.".to_string(),
        1 => "I checked your computer and found one thing worth fixing.".to_string(),
        n => format!("I checked your computer and found {n} things worth fixing."),
    };
    for f in &problems {
        out.push_str("\n- ");
        out.push_str(&f.plain);
    }
    if !notes.is_empty() {
        out.push_str("\n\nAlso:");
        for f in &notes {
            out.push_str("\n- ");
            out.push_str(&f.plain);
        }
    }
    if !fine.is_empty() {
        out.push_str("\n\nLooks fine:");
        for f in &fine {
            out.push_str("\n- ");
            out.push_str(&f.plain);
        }
    }
    out.push_str("\n\nI only looked. Nothing on your computer was changed.");
    out
}

/// A message about code ("the DNS issue in resolver.rs", "the printer
/// module") goes to the model: a file name, a path, backticks, `::`, or a
/// code word.
fn names_code(lower: &str) -> bool {
    const CODE_WORDS: &[&str] = &["module", "function", "crate", "class", "method", "repo", "compile", "unit test", "code"];
    const EXTS: &[&str] = &[
        "rs", "py", "js", "ts", "tsx", "jsx", "go", "c", "h", "cpp", "hpp", "cs", "java", "kt", "rb", "php", "swift", "sh",
        "ps1", "toml", "json", "yaml", "yml", "md", "sql", "html", "css",
    ];
    if lower.contains('`') || lower.contains("::") {
        return true;
    }
    let words: Vec<&str> = lower.split(|c: char| !(c.is_alphanumeric() || c == '_')).filter(|w| !w.is_empty()).collect();
    if CODE_WORDS.iter().any(|w| if w.contains(' ') { lower.contains(w) } else { words.contains(w) }) {
        return true;
    }
    lower.split_whitespace().any(|tok| {
        let tok = tok.trim_matches(|c: char| !(c.is_alphanumeric() || c == '/' || c == '\\' || c == '.' || c == '_'));
        let path = (tok.contains('/') || tok.contains('\\')) && tok.len() > 1;
        let file = tok.rsplit_once('.').is_some_and(|(stem, ext)| !stem.is_empty() && EXTS.contains(&ext));
        path || file
    })
}

/// Phrases that mean "check my computer". Some(probes) picks what to check;
/// None leaves the message to the model. Narrow on purpose: "the build is
/// broken on my machine" is a coding question, not a diagnose.
pub fn probes_for_intent(text: &str, os: Os) -> Option<Vec<ProbeId>> {
    const TROUBLE: &[&str] = &[
        "wrong", "broken", "doesn't work", "does not work", "isn't working", "is not working", "not working",
        "won't", "can't connect", "cannot connect", "keeps dropping", "keeps disconnecting", "problem", "issue",
        "acting up", "crash", "slow", "freez",
    ];
    let lower = text.trim().to_lowercase().replace('\u{2019}', "'");
    if lower.is_empty() || lower.len() > 160 || lower.starts_with('/') {
        return None;
    }
    if names_code(&lower) {
        return None;
    }
    let has = |words: &[&str]| words.iter().any(|w| lower.contains(w));
    let trouble = has(TROUBLE);
    let computer = has(&["my computer", "my pc", "my laptop", "this computer", "this pc", "this laptop", "the computer"]);
    let network = trouble && has(&["wifi", "wi-fi", "internet", "ethernet", "dns"]);
    let disk = has(&["disk space", "disk is full", "disk full", "out of space", "storage is full", "drive is full"]);
    let slow = computer && has(&["slow", "freez"]);
    let updates = has(&["updates won't", "update won't", "can't update", "can't install updates", "updates fail", "updates keep failing"]);
    let services = trouble && has(&["printer", "bluetooth", "sound", "audio"]);
    let mut picked = Vec::new();
    let mut add = |linux: &[ProbeId], windows: &[ProbeId]| {
        picked.extend_from_slice(if os == Os::Windows { windows } else { linux });
    };
    if network {
        add(&[ProbeId::NetworkLinks, ProbeId::Dns], &[ProbeId::NetTest]);
    }
    if disk {
        add(&[ProbeId::DiskUsage], &[ProbeId::Volumes]);
    }
    if slow {
        add(&[ProbeId::Memory, ProbeId::DiskUsage], &[ProbeId::Volumes]);
    }
    if updates {
        add(&[ProbeId::PackageHealth, ProbeId::PendingUpdates, ProbeId::DiskUsage], &[ProbeId::WingetUpgrades, ProbeId::WuPending, ProbeId::Volumes]);
    }
    if services {
        add(&[ProbeId::FailedServices, ProbeId::BootErrors], &[ProbeId::ServicesStoppedAuto, ProbeId::EventErrors24h]);
    }
    if picked.is_empty() {
        if !(computer && trouble) {
            return None;
        }
        picked = ProbeId::all(os).to_vec();
    }
    let mut seen = Vec::new();
    picked.retain(|p| {
        let first = !seen.contains(p);
        seen.push(*p);
        first
    });
    Some(picked)
}

/// The native registry's `diagnose` schema (read class).
pub(crate) fn schema() -> Value {
    let keys: Vec<&str> = LINUX_PROBES.iter().chain(WINDOWS_PROBES).map(|p| p.key()).collect();
    json!({
        "type": "function",
        "name": DIAGNOSE_TOOL,
        "description": "Check this computer with fixed read-only probes (disk, memory, services, logs, network, updates) and get plain-language findings. Pick probe ids; you cannot pass commands. Needs the user's System state permission.",
        "parameters": {
            "type": "object",
            "properties": {
                "probes": {
                    "type": "array",
                    "items": { "type": "string", "enum": keys },
                    "description": "Probe ids for this OS. Empty runs them all."
                }
            },
            "additionalProperties": false
        }
    })
}

/// Run diagnose for a native tool call: the cabin's config dir, its consent
/// ledger, and the chat the cabin is on.
pub(crate) fn tool_run(args: &Value) -> crate::tools::ToolOutput {
    let os = Os::current();
    let probes: Vec<ProbeId> = args
        .get("probes")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).filter_map(|k| ProbeId::parse(k, os)).collect())
        .unwrap_or_default();
    let config_dir = crate::perm::config_dir();
    let ledger = ConsentLedger::load(&config_dir);
    let turn = harness::read_turn_context(&config_dir);
    let session = if turn.chat_id.is_empty() { "native".to_string() } else { turn.chat_id };
    let ctx = DiagnoseCtx {
        config_dir: &config_dir,
        session_id: &session,
        ledger: &ledger,
        access: AccessMode::Readonly,
        os,
        has_bin: &on_path,
        runner: &run_spec,
    };
    let report = diagnose(&ctx, &probes);
    crate::tools::ToolOutput::ok(model_text(&report))
}

/// What the model reads: the plain answer, then the redacted detail lines.
pub fn model_text(report: &DiagnoseReport) -> String {
    let mut out = report_text(report);
    let details: Vec<&Finding> = report.findings.iter().filter(|f| !f.detail.is_empty()).collect();
    if !details.is_empty() {
        out.push_str("\n\nDetails (redacted):");
        for f in details {
            out.push_str(&format!("\n[{}] {}", f.probe.key(), f.detail));
        }
    }
    out
}

#[cfg(test)]
mod fix_tests;
#[cfg(test)]
mod tests;
