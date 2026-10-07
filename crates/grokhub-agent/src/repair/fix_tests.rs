//! Spike-9 acceptance tests (design P6). No shell, no `include_str!`, paths
//! from `std::path`, so they run the same on Windows CI.

use std::path::{Path, PathBuf};

use super::*;
use crate::harness::{
    credential_action, decide, done_without_criteria, grant_scope, read_spans, test_dir, GateOutcome, HardClass,
    Span, Step, UndoAsk, UserClick, REPLY_TOOL, VERIFY_TOOL,
};

const SESSION: &str = "chat-fix";

fn none(_: &str) -> bool {
    false
}

fn finding(probe: ProbeId, severity: Severity, plain: &str, detail: &str) -> Finding {
    Finding { severity, plain: plain.into(), detail: detail.into(), probe }
}

fn dns_down() -> Finding {
    finding(
        ProbeId::Dns,
        Severity::Warning,
        "Your computer can't look up website names (DNS), so websites won't load even when you're connected.",
        "",
    )
}

fn plan(steps: Vec<DraftStep>) -> FixPlan {
    FixPlan::new(dns_down(), "I'll fix the lookups.", "Lookups are stuck.", "Undo puts the file back.", "Low.", steps).unwrap()
}

fn ctx<'a>(dir: &'a Path, attended: bool, access: AccessMode, has_bin: &'a dyn Fn(&str) -> bool) -> ApplyCtx<'a> {
    ApplyCtx { config_dir: dir, session_id: SESSION, os: Os::Linux, attended, access, has_bin }
}

fn tools(dir: &Path) -> Vec<(String, String)> {
    read_spans(dir, SESSION).unwrap().into_iter().map(|s| (s.tool, s.decision)).collect()
}

fn report(findings: Vec<Finding>, ran: Vec<ProbeId>) -> DiagnoseReport {
    DiagnoseReport { ask: None, findings, ran }
}

#[test]
fn steps_are_classified_by_rules_soft_hard_and_floor() {
    let cases: &[(&str, StepClass)] = &[
        ("nmcli networking off; nmcli networking on", StepClass::Soft),
        ("resolvectl flush-caches; systemctl restart systemd-resolved", StepClass::Soft),
        ("systemctl restart cups", StepClass::Soft),
        ("pkexec dpkg --configure -a", StepClass::Soft),
        ("ipconfig /flushdns", StepClass::Soft),
        ("pkexec pacman -Rns nvidia", StepClass::Hard(HardClass::Delete)),
        ("sudo apt-get purge cups", StepClass::Hard(HardClass::Delete)),
        ("winget uninstall Zoom.Zoom", StepClass::Hard(HardClass::Delete)),
        ("pkexec paccache -rk1", StepClass::Hard(HardClass::Delete)),
        ("reg.exe delete HKLM\\Software\\Foo /f", StepClass::Hard(HardClass::Delete)),
        ("Remove-Item -Path $env:TEMP\\* -Recurse -Force", StepClass::Hard(HardClass::Delete)),
        ("rm -rf ~/.cache/thumbnails", StepClass::Hard(HardClass::Delete)),
        ("pkexec modprobe -r nouveau", StepClass::Hard(HardClass::IrreversibleOs)),
        ("pnputil /delete-driver oem12.inf /uninstall", StepClass::Hard(HardClass::IrreversibleOs)),
        ("pkexec grub-mkconfig -o /boot/grub/grub.cfg", StepClass::Hard(HardClass::IrreversibleOs)),
        ("bcdedit /set {default} safeboot minimal", StepClass::Hard(HardClass::IrreversibleOs)),
        ("pkexec systemctl disable gdm", StepClass::Hard(HardClass::IrreversibleOs)),
        ("Set-Service -Name RpcSs -StartupType Disabled", StepClass::Hard(HardClass::IrreversibleOs)),
        ("Restart-Computer", StepClass::Hard(HardClass::IrreversibleOs)),
        ("echo hunter2 | sudo -S apt-get install cups", StepClass::Hard(HardClass::Credentials)),
        ("mkfs.ext4 /dev/sdb1", StepClass::HardFloor),
        ("dd if=/dev/zero of=/dev/sda bs=1M", StepClass::HardFloor),
        ("rm -rf /", StepClass::HardFloor),
        ("Format-Volume -DriveLetter D", StepClass::HardFloor),
        ("format D: /q", StepClass::HardFloor),
        ("Clear-Disk -Number 1 -RemoveData", StepClass::HardFloor),
        ("wipefs -a /dev/sdb", StepClass::HardFloor),
    ];
    for (cmd, want) in cases {
        assert_eq!(classify_step(cmd, &[]), *want, "{cmd}");
    }
    // A harmless command that edits a boot file is hard by the file.
    let fstab = [PathBuf::from("/etc/fstab")];
    assert_eq!(classify_step("sed -i s/noatime/relatime/ /tmp/x", &fstab), StepClass::Hard(HardClass::IrreversibleOs));
    // The gate is the one place that says so.
    assert_eq!(
        decide(Step::Repair { command: "mkfs.ext4 /dev/sdb1" }),
        GateOutcome::Refuse { reason: "hard floor: mkfs".into() }
    );
}

#[test]
fn a_plan_missing_what_why_undo_or_risk_is_rejected() {
    let step = || vec![DraftStep::new("ipconfig /flushdns", "Clear the list.")];
    let make = |w: &str, y: &str, u: &str, r: &str| FixPlan::new(dns_down(), w, y, u, r, step());
    assert_eq!(make("", "y", "u", "r"), Err(PlanError::Empty("what")));
    assert_eq!(make("w", "  ", "u", "r"), Err(PlanError::Empty("why")));
    assert_eq!(make("w", "y", "", "r"), Err(PlanError::Empty("undo")));
    assert_eq!(make("w", "y", "u", "\n"), Err(PlanError::Empty("risk")));
    assert_eq!(FixPlan::new(dns_down(), "w", "y", "u", "r", vec![]), Err(PlanError::NoSteps));
}

/// Every finding the rules can fix, on both OSes.
fn all_rule_plans() -> Vec<FixPlan> {
    let linux = |b: &str| matches!(b, "nmcli" | "resolvectl" | "apt-get" | "pacman" | "paccache");
    let pacman = |b: &str| b == "pacman";
    let mut plans = plans_for(
        &[
            finding(ProbeId::NetworkLinks, Severity::Warning, "You're not connected to any network. Wi-Fi may be off, or the cable may be unplugged.", "wlp2s0 DOWN"),
            finding(ProbeId::FailedServices, Severity::Warning, "A background service has stopped working: cups.", "cups.service loaded failed failed CUPS"),
            finding(ProbeId::DiskUsage, Severity::Critical, "Your main disk is almost full (98% used).", ""),
            finding(ProbeId::PackageHealth, Severity::Warning, "Some packages are half-installed or broken.", "dpkg: cups is half-configured"),
        ],
        Os::Linux,
        &linux,
    );
    let resolvectl = |b: &str| b == "resolvectl";
    plans.insert(1, plans_for(&[dns_down()], Os::Linux, &resolvectl).remove(0));
    plans.extend(plans_for(
        &[finding(ProbeId::PackageHealth, Severity::Warning, "2 installed packages are missing files.", "gtk3 /usr/lib/x (No such file)\nfirefox /usr/lib/y (No such file)")],
        Os::Linux,
        &pacman,
    ));
    plans.extend(plans_for(
        &[
            finding(ProbeId::NetTest, Severity::Warning, "You're not connected to the internet. Wi-Fi may be off, or the cable may be unplugged.", "False\tFalse"),
            finding(ProbeId::ServicesStoppedAuto, Severity::Warning, "A background service should start on its own but isn't running: Print Spooler.", "Spooler\tPrint Spooler"),
            finding(ProbeId::Volumes, Severity::Critical, "Drive C: is almost full (99% used).", ""),
            finding(ProbeId::WuPending, Severity::Warning, "Windows needs a restart to finish installing updates.", "True\t0"),
        ],
        Os::Windows,
        &none,
    ));
    plans
}

#[test]
fn every_fix_card_has_what_why_undo_and_risk() {
    let plans = all_rule_plans();
    assert_eq!(plans.len(), 10, "{:#?}", plans.iter().map(|p| &p.what).collect::<Vec<_>>());
    for p in &plans {
        for (heading, text) in p.card_sections() {
            assert!(!text.trim().is_empty(), "{heading} empty on {}", p.what);
        }
        assert!(!p.steps.is_empty(), "{}", p.what);
    }
    let classes: Vec<(&str, &str)> = plans.iter().map(|p| (p.what.as_str(), p.steps[0].class.as_str())).collect();
    assert_eq!(
        classes,
        vec![
            ("I'll turn your network connection off and on again.", "soft"),
            ("I'll clear the list of website addresses your computer remembers, so it looks them up fresh.", "soft"),
            ("I'll restart the background service that stopped: cups.", "soft"),
            ("I'll delete files your computer doesn't need to run anything, to free up space.", "hard"),
            ("I'll finish setting up the apps that were left half-installed.", "soft"),
            ("I'll reinstall the apps with missing files: gtk3, firefox.", "soft"),
            ("I'll turn your network connection off and on again.", "soft"),
            ("I'll restart the background service that stopped: Spooler.", "soft"),
            ("I'll delete files your computer doesn't need to run anything, to free up space.", "hard"),
            ("I'll restart your computer to finish installing Windows updates.", "hard"),
        ]
    );
}

#[test]
fn names_from_probe_output_never_carry_shell_text() {
    let evil = finding(ProbeId::FailedServices, Severity::Warning, "A background service has stopped working.", "x;rm -rf ~ failed\n$(reboot) failed\n-rf failed");
    assert_eq!(plans_for(&[evil], Os::Linux, &none), Vec::<FixPlan>::new());
}

#[test]
fn apply_writes_the_restore_point_span_before_the_first_mutating_span() {
    let dir = test_dir("fix-order");
    let conf = dir.join("resolved.conf");
    std::fs::write(&conf, b"[Resolve]\nDNS=1.1.1.1\n").unwrap();
    let p = plan(vec![
        DraftStep::new("resolvectl flush-caches", "Clear the list.").touching(&[&conf]),
        DraftStep::new("systemctl restart systemd-resolved", "Restart the lookup service."),
    ]);
    let mut run = ApplyRun::begin(&ctx(&dir, true, AccessMode::Supervised, &none), p).unwrap();
    let Action::Gb { prompt, command, approved } = run.next_action() else { panic!("first step goes to Grok Build") };
    assert_eq!(command, "resolvectl flush-caches");
    assert!(!approved);
    assert!(prompt.starts_with("GrokHub fix, step 1 of 2. Run exactly this one command"), "{prompt}");
    assert!(run.step_finished("ran it\nSTEP_OK"));
    assert!(matches!(run.next_action(), Action::Gb { .. }));
    let spans = read_spans(&dir, SESSION).unwrap();
    assert_eq!(
        spans.iter().map(|s| (s.tool.as_str(), s.decision.as_str())).collect::<Vec<_>>(),
        vec![(RESTORE_TOOL, "restore"), (REPAIR_TOOL, "allow"), (REPAIR_TOOL, "allow")]
    );
    assert!(spans.iter().all(|s| s.origin == crate::harness::Origin::Repair));
    assert!(spans[0].ts_ms <= spans[1].ts_ms);
    let restore: serde_json::Value = serde_json::from_str(&spans[0].args_redacted).unwrap();
    assert_eq!(restore["restore_ref"], format!("files:{}", run.id));
    assert_eq!(restore["files"], 1);
    assert_eq!(spans[1].path, "B");
    // The backup holds the file as it was before step 1.
    assert_eq!(std::fs::read(backup_dir(&dir, &run.id).join("0.bak")).unwrap(), b"[Resolve]\nDNS=1.1.1.1\n");
}

#[test]
fn a_hard_step_parks_under_always_and_full() {
    let dir = test_dir("fix-hard-full");
    let disk = finding(ProbeId::DiskUsage, Severity::Critical, "Your main disk is almost full (98% used).", "");
    let apt = |b: &str| b == "apt-get";
    let p = plans_for(&[disk], Os::Linux, &apt).remove(0);
    assert_eq!(p.steps[0].class, StepClass::Hard(HardClass::Delete));
    // Full access and the Always pill change nothing: the gate parks it.
    let mut run = ApplyRun::begin(&ctx(&dir, true, AccessMode::Full, &apt), p).unwrap();
    assert_eq!(
        run.next_action(),
        Action::Park {
            class: HardClass::Delete,
            command: "pkexec apt-get clean".into(),
            plain: "Delete old copies of app installers kept after updates.".into()
        }
    );
    assert_eq!(tools(&dir), vec![(RESTORE_TOOL.into(), "restore".into()), (REPAIR_TOOL.into(), "park".into())]);
    // A click on Approve sends it once with GB's own Allow; Deny stops the fix.
    run.answer_hard(true);
    let Action::Gb { approved, command, .. } = run.next_action() else { panic!("approved step goes to Grok Build") };
    assert!(approved);
    assert_eq!(command, "pkexec apt-get clean");
    let sent = read_spans(&dir, SESSION).unwrap().pop().unwrap();
    assert!(sent.hard_approved);
    assert_eq!(sent.approval_class, "delete");

    let dir = test_dir("fix-hard-deny");
    let disk = finding(ProbeId::DiskUsage, Severity::Critical, "Your main disk is almost full (98% used).", "");
    let mut run = ApplyRun::begin(&ctx(&dir, true, AccessMode::Full, &apt), plans_for(&[disk], Os::Linux, &apt).remove(0)).unwrap();
    assert!(matches!(run.next_action(), Action::Park { .. }));
    run.answer_hard(false);
    assert_eq!(run.next_action(), Action::Done);
    assert_eq!(run.result_text(), "You said no to a step, so I stopped there. Nothing after it ran. Undo fix puts back what I changed.");
}

#[test]
fn a_hard_floor_step_is_guidance_and_never_runs() {
    let dir = test_dir("fix-floor");
    let p = plan(vec![
        DraftStep::new("mkfs.ext4 /dev/sdb1", "Erase the second disk."),
        DraftStep::new("ipconfig /flushdns", "Clear the list."),
    ]);
    assert_eq!(p.steps[0].class, StepClass::HardFloor);
    assert_eq!(p.runnable(), 1);
    assert_eq!(p.steps[0].card_line(), format!("Erase the second disk. {FLOOR_GUIDANCE}"));
    let mut run = ApplyRun::begin(&ctx(&dir, true, AccessMode::Full, &none), p).unwrap();
    let mut sent = Vec::new();
    loop {
        match run.next_action() {
            Action::Gb { command, .. } => {
                sent.push(command);
                run.step_finished(STEP_OK);
            }
            Action::Park { command, .. } => panic!("parked {command}"),
            Action::Verify(_) => {
                run.record_verify(&report(vec![], vec![ProbeId::Dns]));
            }
            Action::Done => break,
        }
    }
    assert_eq!(sent, vec!["ipconfig /flushdns".to_string()]);
    assert!(read_spans(&dir, SESSION).unwrap().iter().all(|s| !s.args_redacted.contains("mkfs")));
    // A plan of only floor steps has nothing to apply.
    let only = plan(vec![DraftStep::new("dd if=/dev/zero of=/dev/sda", "Wipe the disk.")]);
    let err = ApplyRun::begin(&ctx(&test_dir("fix-floor-only"), true, AccessMode::Full, &none), only).unwrap_err();
    assert_eq!(err, "This fix has no step I'm allowed to run, so I only explained what to do.");
}

/// Run a one-step plan to its verify.
fn applied(label: &str) -> (PathBuf, ApplyRun) {
    let dir = test_dir(label);
    let mut run = ApplyRun::begin(&ctx(&dir, true, AccessMode::Supervised, &none), plan(vec![DraftStep::new("ipconfig /flushdns", "Clear the list.")])).unwrap();
    assert!(matches!(run.next_action(), Action::Gb { .. }));
    assert!(run.step_finished("STEP_OK"));
    assert_eq!(run.next_action(), Action::Verify(ProbeId::Dns));
    (dir, run)
}

#[test]
fn a_failed_verify_never_claims_fixed() {
    let (dir, mut run) = applied("fix-verify-fail");
    assert!(!run.record_verify(&report(vec![dns_down()], vec![ProbeId::Dns])));
    assert_eq!(run.next_action(), Action::Done);
    let text = run.result_text();
    assert_eq!(
        text,
        "That didn't fix it. I checked again: Your computer can't look up website names (DNS), so websites won't load even when you're connected. Undo fix puts back what I changed."
    );
    let mut spans = read_spans(&dir, SESSION).unwrap();
    let verify = spans.iter().find(|s| s.tool == VERIFY_TOOL).unwrap();
    assert_eq!(verify.result, "fail");
    assert!(!spans.iter().any(|s| s.tool == REPLY_TOOL && s.claim.contains("GOAL_COMPLETE")));
    assert!(done_without_criteria(&spans).is_empty(), "no done claim was made");
    // If anything did claim fixed after that failed check, the detector catches it.
    spans.push(Span::reply(SESSION, "Fixed it.\nGOAL_COMPLETE", &[]));
    assert_eq!(done_without_criteria(&spans).len(), 1);

    let (dir, mut run) = applied("fix-verify-pass");
    let ok = finding(ProbeId::Dns, Severity::Ok, "Looking up website names works.", "");
    assert!(run.record_verify(&report(vec![ok], vec![ProbeId::Dns])));
    assert_eq!(run.result_text(), "Fixed. I checked again: Looking up website names works. If anything seems off, press Undo fix.");
    let spans = read_spans(&dir, SESSION).unwrap();
    assert!(spans.iter().any(|s| s.tool == REPLY_TOOL && s.claim.ends_with("VERIFY_OK\nGOAL_COMPLETE")));
    assert!(done_without_criteria(&spans).is_empty());

    // A re-check that couldn't run (no grant) is not a pass.
    let (_dir, mut run) = applied("fix-verify-no-grant");
    let no_grant = DiagnoseReport { ask: Some(SCOPE_ASK.into()), findings: vec![], ran: vec![] };
    assert!(!run.record_verify(&no_grant));
    assert!(run.result_text().starts_with("That didn't fix it."));
}

#[test]
fn undo_restores_fixture_configs_byte_identical() {
    let dir = test_dir("fix-undo");
    let etc = dir.join("etc");
    std::fs::create_dir_all(&etc).unwrap();
    let conf = etc.join("resolved.conf");
    let crlf = etc.join("hosts.ini");
    let fresh = etc.join("created-by-fix.conf");
    let before_conf: &[u8] = b"[Resolve]\nDNS=9.9.9.9\n#FallbackDNS=\n";
    let before_crlf: &[u8] = b"[hosts]\r\n127.0.0.1 localhost\r\n\xff\xfe raw bytes\r\n";
    std::fs::write(&conf, before_conf).unwrap();
    std::fs::write(&crlf, before_crlf).unwrap();
    let p = plan(vec![DraftStep::new("resolvectl revert wlan0", "Reset the lookups.").touching(&[&conf, &crlf, &fresh])]);
    let mut run = ApplyRun::begin(&ctx(&dir, true, AccessMode::Supervised, &none), p).unwrap();
    // What the fix did, standing in for Grok Build.
    std::fs::write(&conf, b"[Resolve]\nDNS=1.1.1.1\n").unwrap();
    std::fs::write(&crlf, b"changed").unwrap();
    std::fs::write(&fresh, b"new").unwrap();
    let text = run.undo(UndoAsk::from_click());
    assert_eq!(text, "Undone. I put back the 3 files this fix changed, exactly as they were.");
    assert_eq!(std::fs::read(&conf).unwrap(), before_conf);
    assert_eq!(std::fs::read(&crlf).unwrap(), before_crlf);
    assert!(!fresh.exists());
    assert!(run.undone);
    let last = read_spans(&dir, SESSION).unwrap().pop().unwrap();
    assert_eq!((last.tool.as_str(), last.decision.as_str(), last.result.as_str()), (UNDO_TOOL, "undo", "restored 2, removed 1, failed 0"));
}

#[test]
fn a_snapshot_is_the_first_step_and_its_guidance_names_it() {
    let dir = test_dir("fix-snapper");
    let snapper = |b: &str| b == "snapper";
    let mut run = ApplyRun::begin(&ctx(&dir, true, AccessMode::Supervised, &snapper), plan(vec![DraftStep::new("ipconfig /flushdns", "Clear the list.")])).unwrap();
    let label = format!("grokhub-repair-{}", run.id);
    assert_eq!(run.restore_ref(), format!("files:{};snapper:{label}", run.id));
    let Action::Gb { command, prompt, .. } = run.next_action() else { panic!("snapshot first") };
    assert_eq!(command, format!("pkexec snapper create --description {label}"));
    assert!(prompt.contains("Never type into it; the user types the password."), "{prompt}");
    run.step_finished("STEP_OK");
    let text = run.undo(UndoAsk::from_click());
    assert!(text.ends_with(&format!("run sudo snapper list, find {label}, run sudo snapper rollback with its number, then restart.")), "{text}");
    // Windows gets a System Restore checkpoint through UAC.
    assert_eq!(detect_backends(Os::Windows, &none), vec![SnapshotBackend::SystemRestore]);
    let cp = Snapshot { backend: SnapshotBackend::SystemRestore, label: "grokhub-repair-1".into() };
    assert_eq!(
        cp.create_step().unwrap(),
        "Start-Process powershell -Verb RunAs -Wait -ArgumentList '-NoProfile -Command Checkpoint-Computer -Description grokhub-repair-1 -RestorePointType MODIFY_SETTINGS'"
    );
    assert_eq!(detect_backends(Os::Linux, &none), Vec::<SnapshotBackend>::new());
    // A snapshot that fails leaves the file backup and the fix going.
    let dir = test_dir("fix-snapper-fail");
    let mut run = ApplyRun::begin(&ctx(&dir, true, AccessMode::Supervised, &snapper), plan(vec![DraftStep::new("ipconfig /flushdns", "Clear the list.")])).unwrap();
    assert!(matches!(run.next_action(), Action::Gb { .. }));
    assert!(!run.step_finished("STEP_FAILED snapper has no root config"));
    assert!(run.snapshot.is_none());
    let Action::Gb { command, .. } = run.next_action() else { panic!("the fix goes on") };
    assert_eq!(command, "ipconfig /flushdns");
}

#[test]
fn the_agent_never_types_into_an_elevation_prompt() {
    // polkit and UAC password fields are hard credentials, value hidden.
    let polkit = serde_json::json!({ "text": "hunter2", "role": "password text", "label": "Authentication is required to restart NetworkManager" });
    let uac = serde_json::json!({ "text": "hunter2", "label": "User Account Control", "field": "Password" });
    for args in [&polkit, &uac] {
        assert_eq!(
            decide(Step::Desk { tool: "type", args }),
            GateOutcome::Park {
                reason: "hard-class credentials: Credentials / secrets — Always cannot skip".into(),
                hard: Some(HardClass::Credentials),
                needs_jeremy: true,
            }
        );
        assert!(!credential_action(args).contains("hunter2"));
    }
    // No rule plan pipes a password, and every elevated step tells GB to wait.
    for p in all_rule_plans() {
        for s in &p.steps {
            assert!(!matches!(s.class, StepClass::Hard(HardClass::Credentials)), "{}", s.command);
            let prompt = gb_prompt(&s.command, 1, 1, s.elevated);
            assert_eq!(prompt.contains("Never type into it"), s.elevated, "{}", s.command);
        }
    }
    assert_eq!(classify_step("printf 'pw\\n' | sudo --stdin systemctl restart cups", &[]), StepClass::Hard(HardClass::Credentials));
}

#[test]
fn apply_refuses_in_an_unattended_context() {
    let dir = test_dir("fix-unattended");
    let err = ApplyRun::begin(&ctx(&dir, false, AccessMode::Full, &none), plan(vec![DraftStep::new("ipconfig /flushdns", "Clear the list.")])).unwrap_err();
    assert_eq!(err, UNATTENDED);
    assert_eq!(tools(&dir), vec![(APPLY_TOOL.into(), "deny".into())]);
    assert!(!dir.join("rewind").exists(), "no backup, no step");
}

#[test]
fn halt_stops_the_fix_with_a_span() {
    let (dir, mut run) = applied("fix-halt");
    run.halt("halted — fail-closed Deny");
    assert!(run.is_done());
    assert_eq!(run.next_action(), Action::Done);
    assert_eq!(tools(&dir).last().unwrap(), &(APPLY_TOOL.to_string(), "deny".to_string()));
    assert_eq!(run.result_text(), "I stopped the fix because you halted. Nothing more will run. Undo fix puts back what I changed.");
}

#[test]
fn my_wifi_doesnt_work_reaches_a_plain_proposal() {
    let dir = test_dir("fix-wifi");
    grant_scope(&dir, &Scope::SystemState, None, UserClick::from_click()).unwrap();
    let ledger = ConsentLedger::load(&dir);
    let probes = probes_for_intent("my wifi doesn't work", Os::Linux).unwrap();
    let answer = |spec: &ProbeSpec| match spec.program.as_str() {
        "ip" => ProbeRun::Done { code: Some(0), stdout: "lo UNKNOWN 127.0.0.1/8\nwlp2s0 DOWN\n".into(), stderr: String::new() },
        "getent" => ProbeRun::Done { code: Some(2), stdout: String::new(), stderr: String::new() },
        other => panic!("unexpected {other}"),
    };
    let nm = |b: &str| b == "nmcli";
    let dctx = DiagnoseCtx { config_dir: &dir, session_id: SESSION, ledger: &ledger, access: AccessMode::Supervised, os: Os::Linux, has_bin: &nm, runner: &answer };
    let found = diagnose(&dctx, &probes);
    let plans = plans_for(&found.findings, Os::Linux, &nm);
    assert_eq!(plans.len(), 1, "one network fix covers both findings");
    let commands = ["nmcli", "systemctl", "resolvectl", "ipconfig", "pkexec", "sudo", "`"];
    for (heading, text) in plans[0].card_sections() {
        let first = text.split(". ").next().unwrap_or(text);
        assert!(!commands.iter().any(|c| first.contains(c)), "{heading}: {first}");
    }
    assert_eq!(plans[0].finding.plain, "You're not connected to any network. Wi-Fi may be off, or the cable may be unplugged.");
    assert_eq!(plans[0].what, "I'll turn your network connection off and on again.");
    assert_eq!(plans[0].steps[0].command, "nmcli networking off; nmcli networking on");
}
