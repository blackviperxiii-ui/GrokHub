use std::cell::RefCell;
use std::path::Path;

use super::*;
use crate::harness::{grant_scope, read_spans, test_dir, UserClick};

const SESSION: &str = "chat-diag";

fn granted(dir: &Path) -> ConsentLedger {
    grant_scope(dir, &Scope::SystemState, None, UserClick::from_click()).expect("grant");
    ConsentLedger::load(dir)
}

fn done(stdout: &str) -> ProbeRun {
    ProbeRun::Done { code: Some(0), stdout: stdout.into(), stderr: String::new() }
}

/// Canned Linux answers keyed by program.
fn linux_answer(spec: &ProbeSpec) -> ProbeRun {
    match spec.program.as_str() {
        "df" => done("Filesystem 1024-blocks Used Available Capacity Mounted on\n/dev/sda2 1000 980 20 98% /\n"),
        "free" => done("Mem: 16000000000 4000000000 8000000000 100000000 4000000000 11000000000\n"),
        "systemctl" => done(""),
        "journalctl" => done("Oct 07 10:01:00 box sync[9]: auth failed for jeremy@example.com key sk-abcdefghijklmnopqrstuv\n"),
        "ip" => done("lo UNKNOWN 127.0.0.1/8\nwlp2s0 DOWN\n"),
        "getent" => ProbeRun::Done { code: Some(2), stdout: String::new(), stderr: String::new() },
        "dpkg" => done(""),
        "apt" => done("Listing...\n"),
        other => panic!("unexpected program {other}"),
    }
}

struct Run {
    dir: std::path::PathBuf,
    report: DiagnoseReport,
    calls: Vec<String>,
}

fn run(label: &str, grant: bool, os: Os, has_bin: &dyn Fn(&str) -> bool, probes: &[ProbeId]) -> Run {
    run_with(label, grant, os, has_bin, probes, &linux_answer)
}

fn run_with(
    label: &str,
    grant: bool,
    os: Os,
    has_bin: &dyn Fn(&str) -> bool,
    probes: &[ProbeId],
    answer: &dyn Fn(&ProbeSpec) -> ProbeRun,
) -> Run {
    let dir = test_dir(label);
    let ledger = if grant { granted(&dir) } else { ConsentLedger::load(&dir) };
    let calls = RefCell::new(Vec::new());
    let runner = |spec: &ProbeSpec| {
        assert_eq!(read_only_violation(spec), None, "{}", spec.command_line());
        calls.borrow_mut().push(spec.command_line());
        answer(spec)
    };
    let ctx = DiagnoseCtx {
        config_dir: &dir,
        session_id: SESSION,
        ledger: &ledger,
        access: AccessMode::Supervised,
        os,
        has_bin,
        runner: &runner,
    };
    let report = diagnose(&ctx, probes);
    Run { dir, report, calls: calls.into_inner() }
}

fn apt(bin: &str) -> bool {
    bin == "apt-get"
}

#[test]
fn no_system_state_grant_runs_zero_probes_and_posts_the_ask() {
    let r = run("diag-no-grant", false, Os::Linux, &apt, &[]);
    assert!(r.calls.is_empty(), "{:?}", r.calls);
    assert_eq!(r.report.ask.as_deref(), Some(SCOPE_ASK));
    assert!(r.report.findings.is_empty());
    assert_eq!(report_text(&r.report), SCOPE_ASK);
    let spans = read_spans(&r.dir, SESSION).unwrap();
    assert_eq!(spans.len(), 1);
    assert_eq!(spans[0].decision, "deny");
    assert_eq!(spans[0].origin, Origin::Repair);
    assert_eq!(spans[0].result, "scope system_state is off — only your click in Settings turns it on");
}

#[test]
fn diagnose_runs_only_the_fixed_read_only_commands() {
    let r = run("diag-all", true, Os::Linux, &apt, &[]);
    assert_eq!(
        r.calls,
        vec![
            "df -P -k",
            "free -b",
            "systemctl --failed --no-legend --no-pager --plain",
            "journalctl -p err -b --no-pager -q -n 20 -o short",
            "ip -brief address",
            "getent hosts example.com",
            "dpkg --audit",
            "apt list --upgradable",
        ]
    );
    assert_eq!(r.report.ran.len(), 8);
}

#[test]
fn every_probe_span_has_origin_repair_and_the_grant() {
    let r = run("diag-spans", true, Os::Linux, &apt, &[]);
    let spans = read_spans(&r.dir, SESSION).unwrap();
    assert_eq!(spans.len(), 8);
    let grant = ConsentLedger::load(&r.dir).scope_grant(&Scope::SystemState).unwrap().id.clone();
    for span in &spans {
        assert_eq!(span.origin, Origin::Repair, "{span:?}");
        assert_eq!(span.tool, DIAGNOSE_TOOL);
        assert_eq!(span.decision, "allow");
        assert_eq!(span.consent_ref, grant);
    }
    assert_eq!(spans[0].args_redacted, r#"{"command":"df -P -k","probe":"disk_usage"}"#);
}

#[test]
fn journal_secrets_and_emails_never_reach_the_span_or_the_model() {
    let r = run("diag-redact", true, Os::Linux, &apt, &[ProbeId::BootErrors]);
    let raw = std::fs::read_to_string(harness::span_path(&r.dir, SESSION)).unwrap();
    let model = model_text(&r.report);
    for text in [&raw, &model] {
        assert!(!text.contains("sk-abcdefghijklmnopqrstuv"), "{text}");
        assert!(!text.contains("jeremy@example.com"), "{text}");
    }
    assert!(model.contains("[boot_errors] Oct 07 10:01:00 box sync[9]: auth failed for [email] key [redacted]"), "{model}");
}

#[test]
fn a_probe_that_needs_admin_is_skipped_with_the_plain_note() {
    let zypper = |b: &str| b == "zypper";
    let r = run_with("diag-admin", true, Os::Linux, &zypper, &[ProbeId::PackageHealth], &|_| panic!("must not run"));
    assert!(r.calls.is_empty());
    assert_eq!(r.report.findings.len(), 1);
    assert_eq!(r.report.findings[0].plain, "Checking installed packages needs admin, skipped.");
    let spans = read_spans(&r.dir, SESSION).unwrap();
    assert_eq!(spans[0].decision, "skip");
    assert_eq!(spans[0].result, NEEDS_ADMIN);
    assert_eq!(spans[0].origin, Origin::Repair);
}

#[test]
fn a_probe_that_times_out_becomes_a_plain_note() {
    let r = run_with("diag-timeout", true, Os::Linux, &apt, &[ProbeId::Memory], &|_| ProbeRun::TimedOut);
    assert_eq!(r.report.findings[0].plain, "Checking memory took too long, so I stopped it.");
    assert_eq!(read_spans(&r.dir, SESSION).unwrap()[0].result, "timeout");
}

#[test]
fn my_wifi_doesnt_work_picks_network_probes_and_answers_in_plain_words() {
    assert_eq!(probes_for_intent("my wifi doesn't work", Os::Linux), Some(vec![ProbeId::NetworkLinks, ProbeId::Dns]));
    assert_eq!(probes_for_intent("My WiFi doesn’t work!", Os::Windows), Some(vec![ProbeId::NetTest]));
    let probes = probes_for_intent("my wifi doesn't work", Os::Linux).unwrap();
    let r = run("diag-wifi", true, Os::Linux, &apt, &probes);
    assert_eq!(r.calls, vec!["ip -brief address", "getent hosts example.com"]);
    let text = report_text(&r.report);
    let first = text.lines().next().unwrap();
    assert_eq!(first, "I checked your computer and found 2 things worth fixing.");
    for raw in ["ip ", "getent", "-brief", "example.com"] {
        assert!(!first.contains(raw), "{first}");
    }
    assert_eq!(
        text,
        "I checked your computer and found 2 things worth fixing.\n\
         - You're not connected to any network. Wi-Fi may be off, or the cable may be unplugged.\n\
         - Your computer can't look up website names (DNS), so websites won't load even when you're connected.\n\n\
         I only looked. Nothing on your computer was changed."
    );
}

#[test]
fn intent_phrases_pick_probes_and_other_text_goes_to_the_model() {
    assert_eq!(probes_for_intent("something's wrong with my computer", Os::Linux), Some(LINUX_PROBES.to_vec()));
    assert_eq!(probes_for_intent("Something is wrong with my PC", Os::Windows), Some(WINDOWS_PROBES.to_vec()));
    assert_eq!(probes_for_intent("my disk is full", Os::Linux), Some(vec![ProbeId::DiskUsage]));
    assert_eq!(
        probes_for_intent("my laptop is so slow", Os::Linux),
        Some(vec![ProbeId::Memory, ProbeId::DiskUsage])
    );
    assert_eq!(probes_for_intent("my printer isn't working", Os::Linux), Some(vec![ProbeId::FailedServices, ProbeId::BootErrors]));
    assert_eq!(probes_for_intent("write a poem about my computer", Os::Linux), None);
    assert_eq!(probes_for_intent("the build is broken on my machine", Os::Linux), None);
    assert_eq!(probes_for_intent("this test is slow, can you speed it up?", Os::Linux), None);
    assert_eq!(probes_for_intent("what's wrong with this function?", Os::Linux), None);
    assert_eq!(probes_for_intent("/diagnose", Os::Linux), None);
}

#[test]
fn findings_come_worst_first_and_the_first_sentence_has_no_command() {
    let r = run("diag-order", true, Os::Linux, &apt, &[]);
    let sev: Vec<Severity> = r.report.findings.iter().map(|f| f.severity).collect();
    let mut sorted = sev.clone();
    sorted.sort_by_key(|s| std::cmp::Reverse(*s));
    assert_eq!(sev, sorted);
    assert_eq!(r.report.findings[0].plain, "Your main disk is almost full (98% used), so updates can't install and apps may fail to save files.");
    let first = report_text(&r.report).lines().next().unwrap().to_string();
    assert_eq!(first, "I checked your computer and found 3 things worth fixing.");
    for p in ["df", "free -b", "systemctl", "journalctl", "getent", "dpkg", "apt list"] {
        assert!(!first.contains(p), "{first}");
    }
}

#[test]
fn the_tool_schema_offers_ids_not_commands() {
    let s = schema();
    assert_eq!(s["name"], "diagnose");
    let props = s["parameters"]["properties"].as_object().unwrap();
    assert_eq!(props.keys().collect::<Vec<_>>(), vec!["probes"]);
    assert_eq!(s["parameters"]["additionalProperties"], false);
    assert_eq!(s["parameters"]["properties"]["probes"]["items"]["enum"].as_array().unwrap().len(), 14);
}
