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
    ProbeRun::Done {
        code: Some(0),
        stdout: stdout.into(),
        stderr: String::new(),
    }
}

/// Canned Linux answers keyed by program.
fn linux_answer(spec: &ProbeSpec) -> ProbeRun {
    match spec.program.as_str() {
        "df" => done("Filesystem 1024-blocks Used Available Capacity Mounted on\n/dev/sda2 1000 980 20 98% /\n"),
        "free" => done("Mem: 16000000000 4000000000 8000000000 100000000 4000000000 11000000000\n"),
        "systemctl" => done(""),
        "journalctl" => done("Oct 07 10:01:00 box sync[9]: auth failed for user@example.com key sk-abcdefghijklmnopqrstuv\n"),
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

fn run(
    label: &str,
    grant: bool,
    os: Os,
    has_bin: &dyn Fn(&str) -> bool,
    probes: &[ProbeId],
) -> Run {
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
    run_about(label, os, has_bin, probes, answer, grant, None)
}

fn run_about(
    label: &str,
    os: Os,
    has_bin: &dyn Fn(&str) -> bool,
    probes: &[ProbeId],
    answer: &dyn Fn(&ProbeSpec) -> ProbeRun,
    grant: bool,
    subject: Option<&ProgramName>,
) -> Run {
    let dir = test_dir(label);
    let ledger = if grant {
        granted(&dir)
    } else {
        ConsentLedger::load(&dir)
    };
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
        subject,
    };
    let report = diagnose(&ctx, probes);
    Run {
        dir,
        report,
        calls: calls.into_inner(),
    }
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
    assert_eq!(
        spans[0].result,
        "scope system_state is off — only your click in Settings turns it on"
    );
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
    let grant = ConsentLedger::load(&r.dir)
        .scope_grant(&Scope::SystemState)
        .unwrap()
        .id
        .clone();
    for span in &spans {
        assert_eq!(span.origin, Origin::Repair, "{span:?}");
        assert_eq!(span.tool, DIAGNOSE_TOOL);
        assert_eq!(span.decision, "allow");
        assert_eq!(span.consent_ref, grant);
    }
    assert_eq!(
        spans[0].args_redacted,
        r#"{"command":"df -P -k","probe":"disk_usage"}"#
    );
}

#[test]
fn journal_secrets_and_emails_never_reach_the_span_or_the_model() {
    let r = run("diag-redact", true, Os::Linux, &apt, &[ProbeId::BootErrors]);
    let raw = std::fs::read_to_string(harness::span_path(&r.dir, SESSION)).unwrap();
    let model = model_text(&r.report);
    for text in [&raw, &model] {
        assert!(!text.contains("sk-abcdefghijklmnopqrstuv"), "{text}");
        assert!(!text.contains("user@example.com"), "{text}");
    }
    assert!(
        model.contains(
            "[boot_errors] Oct 07 10:01:00 box sync[9]: auth failed for [email] key [redacted]"
        ),
        "{model}"
    );
}

#[test]
fn a_probe_that_needs_admin_is_skipped_with_the_plain_note() {
    let zypper = |b: &str| b == "zypper";
    let r = run_with(
        "diag-admin",
        true,
        Os::Linux,
        &zypper,
        &[ProbeId::PackageHealth],
        &|_| panic!("must not run"),
    );
    assert!(r.calls.is_empty());
    assert_eq!(r.report.findings.len(), 1);
    assert_eq!(
        r.report.findings[0].plain,
        "Checking installed packages needs admin, skipped."
    );
    let spans = read_spans(&r.dir, SESSION).unwrap();
    assert_eq!(spans[0].decision, "skip");
    assert_eq!(spans[0].result, NEEDS_ADMIN);
    assert_eq!(spans[0].origin, Origin::Repair);
}

#[test]
fn a_probe_that_times_out_becomes_a_plain_note() {
    let r = run_with(
        "diag-timeout",
        true,
        Os::Linux,
        &apt,
        &[ProbeId::Memory],
        &|_| ProbeRun::TimedOut,
    );
    assert_eq!(
        r.report.findings[0].plain,
        "Checking memory took too long, so I stopped it."
    );
    assert_eq!(read_spans(&r.dir, SESSION).unwrap()[0].result, "timeout");
}

#[test]
fn my_wifi_doesnt_work_picks_network_probes_and_answers_in_plain_words() {
    assert_eq!(
        probes_for_intent("my wifi doesn't work", Os::Linux),
        Some(vec![ProbeId::NetworkLinks, ProbeId::Dns])
    );
    assert_eq!(
        probes_for_intent("My WiFi doesn’t work!", Os::Windows),
        Some(vec![ProbeId::NetTest])
    );
    let probes = probes_for_intent("my wifi doesn't work", Os::Linux).unwrap();
    let r = run("diag-wifi", true, Os::Linux, &apt, &probes);
    assert_eq!(
        r.calls,
        vec!["ip -brief address", "getent hosts example.com"]
    );
    let text = report_text(&r.report);
    let first = text.lines().next().unwrap();
    assert_eq!(
        first,
        "I checked your computer and found 2 things worth fixing."
    );
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
    assert_eq!(
        probes_for_intent("something's wrong with my computer", Os::Linux),
        Some(LINUX_PROBES.to_vec())
    );
    assert_eq!(
        probes_for_intent("Something is wrong with my PC", Os::Windows),
        Some(WINDOWS_PROBES.to_vec())
    );
    assert_eq!(
        probes_for_intent("my disk is full", Os::Linux),
        Some(vec![ProbeId::DiskUsage])
    );
    assert_eq!(
        probes_for_intent("my laptop is so slow", Os::Linux),
        Some(vec![ProbeId::Memory, ProbeId::DiskUsage])
    );
    assert_eq!(
        probes_for_intent("my printer isn't working", Os::Linux),
        Some(vec![ProbeId::FailedServices, ProbeId::BootErrors])
    );
    assert_eq!(
        probes_for_intent("write a poem about my computer", Os::Linux),
        None
    );
    assert_eq!(
        probes_for_intent("the build is broken on my machine", Os::Linux),
        None
    );
    assert_eq!(
        probes_for_intent("this test is slow, can you speed it up?", Os::Linux),
        None
    );
    assert_eq!(
        probes_for_intent("what's wrong with this function?", Os::Linux),
        None
    );
    assert_eq!(probes_for_intent("/diagnose", Os::Linux), None);
    // Coding messages that mention a trouble word and a device word.
    assert_eq!(
        probes_for_intent("fix the DNS issue in resolver.rs", Os::Linux),
        None
    );
    assert_eq!(
        probes_for_intent("the printer module has a problem", Os::Linux),
        None
    );
    assert_eq!(
        probes_for_intent("wifi scan in src/net/scan is broken", Os::Linux),
        None
    );
    assert_eq!(
        probes_for_intent("`audio::play` won't start", Os::Linux),
        None
    );
    // Plain words still get the probes.
    assert_eq!(
        probes_for_intent("my internet is not working.", Os::Windows),
        Some(vec![ProbeId::NetTest])
    );
    assert_eq!(
        probes_for_intent("the sound on my laptop is broken", Os::Linux),
        Some(vec![ProbeId::FailedServices, ProbeId::BootErrors])
    );
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
    assert_eq!(
        first,
        "I checked your computer and found 3 things worth fixing."
    );
    for p in [
        "df",
        "free -b",
        "systemctl",
        "journalctl",
        "getent",
        "dpkg",
        "apt list",
    ] {
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
    assert_eq!(
        s["parameters"]["properties"]["probes"]["items"]["enum"]
            .as_array()
            .unwrap()
            .len(),
        14
    );
}

const CRASH_ASK: &str =
    "Help me investigate why bluetoothd keeps crashing (read crash logs metadata only).";

fn crash_answer(spec: &ProbeSpec) -> ProbeRun {
    match spec.program.as_str() {
        "coredumpctl" => done(
            "Sun 2026-11-08 12:40:02 CST  1201 0 0 SIGSEGV present  /usr/lib/bluetooth/bluetoothd 1.1M\n\
             Sun 2026-11-08 13:02:11 CST  1388 0 0 SIGABRT present  /usr/lib/bluetooth/bluetoothd 1.2M\n",
        ),
        "journalctl" => done("Nov 08 13:02:10 beast bluetoothd[1388]: adapter reset failed\n"),
        "systemctl" => done(""),
        other => panic!("unexpected program {other}"),
    }
}

#[test]
fn a_crash_card_chat_picks_that_programs_crash_history() {
    let crash = Some(vec![
        ProbeId::CrashDumps,
        ProbeId::CrashLog,
        ProbeId::FailedServices,
    ]);
    assert_eq!(probes_for_subject(CRASH_ASK, Os::Linux), crash);
    assert_eq!(probes_for_subject("check into it for me", Os::Linux), crash);
    assert_eq!(
        probes_for_subject("why does it keep doing that?", Os::Linux),
        crash
    );
    assert_eq!(
        probes_for_subject("check into it for me", Os::Windows),
        Some(vec![ProbeId::AppCrashes, ProbeId::ServicesStoppedAuto])
    );
    assert_eq!(probes_for_subject("thanks, that's all", Os::Linux), None);
    assert_eq!(probes_for_subject("/diagnose", Os::Linux), None);
    assert_eq!(
        probes_for_subject("why does adapter.rs crash", Os::Linux),
        None
    );
    // Without a card or a name, the generic reading stands: services only.
    assert_eq!(
        probes_for_intent("my bluetooth keeps crashing", Os::Linux),
        Some(vec![ProbeId::FailedServices, ProbeId::BootErrors])
    );
}

/// The bluetoothd transcript: with no card, a crash ask that names its
/// program reads that program's crash history, then the services.
#[test]
fn a_crash_ask_that_names_its_program_reads_its_crash_history() {
    let named =
        |text: &str, os: Os| probes_for_named(text, os).map(|(n, p)| (n.as_str().to_string(), p));
    let service = vec![
        ProbeId::CrashDumps,
        ProbeId::CrashLog,
        ProbeId::FailedServices,
        ProbeId::BootErrors,
    ];
    assert_eq!(
        named("why does bluetoothd keep crashing?", Os::Linux),
        Some(("bluetoothd".into(), service.clone()))
    );
    assert_eq!(
        named(CRASH_ASK, Os::Linux),
        Some(("bluetoothd".into(), service))
    );
    assert_eq!(
        named("Firefox crashed again", Os::Linux),
        Some((
            "Firefox".into(),
            vec![ProbeId::CrashDumps, ProbeId::CrashLog]
        ))
    );
    assert_eq!(
        named("steam keeps dying", Os::Linux),
        Some(("steam".into(), vec![ProbeId::CrashDumps, ProbeId::CrashLog]))
    );
    assert_eq!(
        named("what's wrong with Discord.exe?", Os::Windows),
        Some(("Discord.exe".into(), vec![ProbeId::AppCrashes]))
    );
    assert_eq!(
        named("why is the Spooler service crashing", Os::Windows),
        Some((
            "Spooler".into(),
            vec![
                ProbeId::AppCrashes,
                ProbeId::ServicesStoppedAuto,
                ProbeId::EventErrors24h
            ]
        ))
    );
    // Not a program: the usual paths keep these.
    for text in [
        "my bluetooth keeps crashing",
        "it keeps crashing",
        "my game crashed",
        "I was in a car crash",
        "what's wrong with you",
        "why does adapter.rs crash",
        "my dog died",
        "/diagnose crash",
    ] {
        assert_eq!(named(text, Os::Linux), None, "{text}");
    }
}

/// A hostile ask never becomes a shell string: it names no program, or a
/// plain word that reaches the probe as one argv entry.
#[test]
fn a_crash_ask_with_shell_characters_names_no_program_or_a_plain_word() {
    for text in [
        "why does foo; rm -rf ~ keep crashing",
        "why does foo;rm keep crashing",
        "why does $(reboot) keep crashing",
        "why does a|b keep crashing",
        "why does -rf keep crashing",
        "why does ../etc/passwd keep crashing",
    ] {
        assert_eq!(program_in_ask(text), None, "{text}");
    }
    let name = program_in_ask("what's wrong with foo.exe; rm -rf ~").expect("a plain word");
    assert_eq!(name.as_str(), "foo.exe");
    let spec = subject_probe_spec(ProbeId::CrashDumps, Os::Linux, &name).unwrap();
    assert_eq!(spec.program, "coredumpctl");
    assert_eq!(spec.argv, ["list", "--no-pager", "--no-legend", "foo.exe"]);
    let name = program_in_ask("rm -rf ~; foo keeps crashing").expect("a plain word");
    assert_eq!(name.as_str(), "foo");
    let spec = subject_probe_spec(ProbeId::CrashLog, Os::Linux, &name).unwrap();
    assert_eq!(spec.argv[0], "_COMM=foo");
}

/// The named ask, run end to end: the report names the program, how many
/// times it crashed and when, and the likely cause.
#[test]
fn a_named_crash_ask_reports_the_count_dates_and_likely_cause() {
    let (name, probes) = probes_for_named("why does bluetoothd keep crashing", Os::Linux).unwrap();
    let r = run_about(
        "diag-crash-named",
        Os::Linux,
        &apt,
        &probes,
        &crash_answer,
        true,
        Some(&name),
    );
    assert_eq!(
        r.calls,
        vec![
            "coredumpctl list --no-pager --no-legend bluetoothd",
            "journalctl _COMM=bluetoothd -p warning -n 20 --no-pager -q -o short",
            "systemctl --failed --no-legend --no-pager --plain",
            "journalctl -p err -b --no-pager -q -n 20 -o short",
        ]
    );
    let text = report_text(&r.report);
    assert!(
        text.starts_with(
            "I checked your computer and found one thing worth fixing.\n- bluetoothd has crashed: 2 crash dumps on record, from Sun 2026-11-08 12:40:02 CST to Sun 2026-11-08 13:02:11 CST. The likely cause of the newest is the program stopping itself after an error it couldn't recover from (SIGABRT)."
        ),
        "{text}"
    );
    assert!(text.ends_with("I only looked. Nothing on your computer was changed."));
}

#[test]
fn a_crash_card_reads_its_programs_crash_dumps_and_log_by_name() {
    let name = ProgramName::parse("bluetoothd").unwrap();
    let probes = probes_for_subject(CRASH_ASK, Os::Linux).unwrap();
    let r = run_about(
        "diag-crash-card",
        Os::Linux,
        &apt,
        &probes,
        &crash_answer,
        true,
        Some(&name),
    );
    assert_eq!(
        r.calls,
        vec![
            "coredumpctl list --no-pager --no-legend bluetoothd",
            "journalctl _COMM=bluetoothd -p warning -n 20 --no-pager -q -o short",
            "systemctl --failed --no-legend --no-pager --plain",
        ]
    );
    let plains: Vec<(&str, Severity)> = r
        .report
        .findings
        .iter()
        .map(|f| (f.plain.as_str(), f.severity))
        .collect();
    assert_eq!(
        plains,
        vec![
            (
                "bluetoothd has crashed: 2 crash dumps on record, from Sun 2026-11-08 12:40:02 CST to Sun 2026-11-08 13:02:11 CST. The likely cause of the newest is the program stopping itself after an error it couldn't recover from (SIGABRT).",
                Severity::Warning
            ),
            (
                "bluetoothd logged 1 warning or error recently. The newest is from Nov 08 13:02:10.",
                Severity::Info
            ),
            ("All background services are running.", Severity::Ok),
        ]
    );
    // The same probes with no name from a card: the crash ones never run.
    let bare = run_about(
        "diag-crash-bare",
        Os::Linux,
        &apt,
        &probes,
        &crash_answer,
        true,
        None,
    );
    assert_eq!(
        bare.calls,
        vec!["systemctl --failed --no-legend --no-pager --plain"]
    );
}

#[test]
fn a_program_name_from_a_card_is_one_plain_word() {
    for ok in [
        "bluetoothd",
        "getty@tty1",
        "Discord.exe",
        "pipewire-pulse",
        "nm_applet",
    ] {
        assert_eq!(
            ProgramName::parse(ok).map(|n| n.as_str().to_string()),
            Some(ok.into())
        );
    }
    for bad in [
        "", "-rf", "--all", ".hidden", "../etc", "a b", "x;rm", "a'b", "a\"b", "$(id)", "a|b",
        "über",
    ] {
        assert_eq!(ProgramName::parse(bad), None, "{bad}");
    }
    assert_eq!(ProgramName::parse(&"a".repeat(65)), None);
    // The journal keeps 15 bytes of a process name.
    let long = ProgramName::parse("systemd-resolved").unwrap();
    assert_eq!(
        subject_probe_spec(ProbeId::CrashLog, Os::Linux, &long)
            .unwrap()
            .argv[0],
        "_COMM=systemd-resolve"
    );
    // Windows looks the exe up in the Application log, read only.
    let discord = ProgramName::parse("Discord").unwrap();
    let spec = subject_probe_spec(ProbeId::AppCrashes, Os::Windows, &discord).unwrap();
    assert_eq!(read_only_violation(&spec), None);
    assert!(
        spec.argv[3].contains("EventData[Data[@Name=\"AppName\"]=\"Discord.exe\"]"),
        "{}",
        spec.argv[3]
    );
    // Not a Linux probe on Windows, and not a probe the model can name.
    assert_eq!(
        subject_probe_spec(ProbeId::CrashDumps, Os::Windows, &discord),
        None
    );
    assert_eq!(ProbeId::parse("crash_dumps", Os::Linux), None);
    assert_eq!(probe_spec_with(ProbeId::CrashLog, Os::Linux, &apt), None);
    for os in [Os::Linux, Os::Windows] {
        for id in ProbeId::for_subject(os) {
            let spec = subject_probe_spec(*id, os, &long).unwrap();
            assert_eq!(read_only_violation(&spec), None, "{}", spec.command_line());
        }
    }
}

#[test]
fn crash_history_reads_as_plain_findings() {
    let out = |code: i32, stdout: &str, stderr: &str| ProbeOutput {
        program: "x".into(),
        code: Some(code),
        stdout: stdout.into(),
        stderr: stderr.into(),
    };
    let plain = |id: ProbeId, o: ProbeOutput, name: &str| -> Vec<String> {
        interpret_subject(id, &o, name)
            .into_iter()
            .map(|f| f.plain)
            .collect()
    };
    assert_eq!(
        plain(ProbeId::CrashDumps, out(1, "", "No coredumps found.\n"), "bluetoothd"),
        vec!["I found no crash dumps of bluetoothd that I can read. A system program's dumps can need admin rights to see."]
    );
    assert_eq!(
        plain(
            ProbeId::CrashLog,
            out(0, "", "Hint: You are currently not seeing messages from other users and the system.\n"),
            "bluetoothd"
        ),
        vec!["I could only read your own logs, and bluetoothd logs to the system's, which needs admin rights."]
    );
    assert_eq!(
        plain(
            ProbeId::CrashLog,
            out(0, "-- No entries --\n", ""),
            "bluetoothd"
        ),
        vec!["bluetoothd logged no warnings or errors recently."]
    );
    assert_eq!(
        plain(
            ProbeId::AppCrashes,
            out(
                0,
                "2026-11-08T13:02:11\tKERNELBASE.dll\t0xe0434352\n2026-11-07T09:00:00\tKERNELBASE.dll\t0xe0434352\n",
                ""
            ),
            "Discord.exe"
        ),
        vec!["Discord.exe has crashed: 2 crashes in the Application log, from 2026-11-07 09:00:00 to 2026-11-08 13:02:11. The newest was in KERNELBASE.dll (code 0xe0434352)."]
    );
    assert_eq!(
        plain(ProbeId::AppCrashes, out(0, "", ""), "Discord.exe"),
        vec!["Windows has no crash of Discord.exe on record."]
    );
    let many = "Sun 2026-11-08 13:02:11 CST  1388 0 0 SIGABRT present  /usr/lib/bluetooth/bluetoothd 1.2M\n".repeat(20);
    assert_eq!(
        plain(ProbeId::CrashDumps, out(0, &many, ""), "bluetoothd"),
        vec!["bluetoothd has crashed: 20 or more crash dumps on record, from Sun 2026-11-08 13:02:11 CST. The likely cause of the newest is the program stopping itself after an error it couldn't recover from (SIGABRT)."]
    );
}
