use super::*;
use serde_json::json;

const DISK_REPORT: &str = r#"{
  "version": 1,
  "name": "disk-report",
  "about": "Tell me when the system disk passes 90%.",
  "triggers": [{ "timer": "daily 09:00" }, "chat"],
  "inputs": {
    "mount": { "type": "string", "default": "/" }
  },
  "steps": [
    { "id": "df", "tool": "run_terminal_command", "args": { "command": "df -P {{inputs.mount}}" } },
    { "id": "use", "expect": { "from": "df.stdout", "regex": "(\\d+)%", "group": 1, "at_least": 90 },
      "else": "done" }
  ],
  "result": "{{inputs.mount}} is {{use.value}}% full."
}"#;

fn tools(name: &str) -> bool {
    matches!(name, "run_terminal_command" | "read_file" | "notify")
}

fn routine(steps: Value) -> Routine {
    Routine::parse(
        &json!({ "version": 1, "name": "t", "inputs": { "n": { "type": "integer" } }, "steps": steps })
            .to_string(),
    )
    .unwrap()
}

#[test]
fn the_design_notes_disk_report_reads_cleanly_and_writes_back_the_same() {
    let r = Routine::parse(DISK_REPORT).unwrap();
    assert_eq!(r.problems(&tools), Vec::<String>::new());
    assert_eq!(
        r.triggers,
        [Trigger::Timer(Timer::Daily { at_min: 540 }), Trigger::Chat]
    );
    assert_eq!(
        r.steps[1].kind,
        StepKind::Expect {
            expect: Expect {
                from: "df.stdout".into(),
                regex: Some(r"(\d+)%".into()),
                group: Some(1),
                equals: None,
                at_least: Some(90.0),
                at_most: None,
            },
            otherwise: Some("done".into()),
        }
    );
    // Written back, it reads as the same routine (90 comes back as 90.0).
    let again = serde_json::to_string(&r).unwrap();
    assert_eq!(Routine::parse(&again).unwrap(), r);
    assert!(
        again.contains(r#""triggers":[{"timer":"daily 09:00"},"chat"]"#),
        "{again}"
    );
}

#[test]
fn a_step_takes_exactly_one_kind_and_only_its_own_keys() {
    let err = |step: Value| {
        Routine::parse(&json!({ "version": 1, "name": "t", "steps": [step] }).to_string())
            .unwrap_err()
    };
    assert!(
        err(json!({ "id": "a", "tool": "notify", "script": "x.py" }))
            .contains("step \"a\": give it one of tool, script, desktop, expect or ask (it has tool and script)"),
    );
    assert!(err(json!({ "id": "a" })).contains("(it has none)"));
    assert!(err(json!({ "id": "a", "tool": "notify", "else": "done" }))
        .contains("step \"a\": \"else\" doesn't go on a tool step"));
    assert!(err(json!({ "id": "a", "ask": "Which?" })).contains("an ask step needs \"branches\""));
    assert!(
        err(json!({ "id": "a", "tool": "notify", "argz": {} })).contains("unknown field `argz`")
    );
}

#[test]
fn every_problem_is_listed_in_step_order() {
    let mut r = routine(json!([
        { "id": "a", "tool": "rm_everything" },
        { "id": "a", "script": "../up.py" },
        { "id": "Bad", "expect": { "from": "a.value", "regex": "(", "group": 2 } },
        { "id": "pick", "ask": " ", "branches": { "only": "a" } },
        { "id": "lost", "tool": "notify", "args": { "text": "{{inputs.missing}} {{nope.stdout}}" } }
    ]));
    r.version = 2;
    r.name = "Disk Report".into();
    r.inputs.insert(
        "Size".into(),
        Input {
            kind: InputKind::Integer,
            default: Some(json!("big")),
            about: String::new(),
        },
    );
    r.result = Some("{{lost.stdout}}".into());
    assert_eq!(
        r.problems(&tools),
        [
            "version 2 isn't one this app reads (use 1)",
            "name \"Disk Report\": use lowercase letters, digits and dashes",
            "input \"Size\": use lowercase letters, digits and _",
            "input \"Size\": its default \"big\" isn't a integer",
            "step \"a\" is named twice",
            "step \"Bad\": name it with lowercase letters, digits and _ (not done or inputs)",
            "step \"a\": there is no tool named \"rm_everything\"",
            "step \"a\": script \"../up.py\" must be a file name in the routine's folder",
            "step \"Bad\": the regex \"(\" doesn't compile",
            "step \"Bad\" reads a value that step \"a\" doesn't give",
            "step \"pick\" jumps to \"a\", which isn't a later step or done",
            "step \"pick\": the ask has no question",
            "step \"pick\": an ask needs at least two branches",
            "step \"lost\" can never run",
            "step \"lost\" uses inputs.missing, which isn't one of the inputs",
            "step \"lost\" uses nope, which isn't a step",
            "result never shows: the last step can't finish the run",
        ]
    );
}

#[test]
fn a_step_skipped_by_a_jump_cant_be_read_after_it() {
    let mut r = routine(json!([
        { "id": "probe", "tool": "run_terminal_command", "args": { "command": "free -m" } },
        { "id": "low", "expect": { "from": "probe.stdout", "regex": "Mem: +(\\d+)", "group": 1, "at_most": 512 },
          "else": "tell" },
        { "id": "top", "tool": "run_terminal_command", "args": { "command": "ps aux --sort=-rss" } },
        { "id": "tell", "tool": "notify", "args": { "text": "{{probe.stdout}} {{top.stdout}}" } }
    ]));
    assert_eq!(
        r.problems(&tools),
        ["step \"tell\" uses top, which may not have run by then"]
    );
    // Reading a step that comes later is the same mistake; `low` ran on
    // every path, so the result may read it.
    r.result = Some("{{top.stdout}} {{low.value}}".into());
    let StepKind::Expect { expect, .. } = &mut r.steps[1].kind else {
        unreachable!()
    };
    expect.from = "top.stdout".into();
    assert_eq!(
        r.problems(&tools),
        [
            "step \"low\" uses top, which may not have run by then",
            "step \"tell\" uses top, which may not have run by then",
            "result uses top, which may not have run by then",
        ]
    );
}

#[test]
fn an_ask_jumps_forward_and_each_branch_can_run() {
    let r = routine(json!([
        { "id": "pick", "ask": "Photo or document?", "branches": { "photo": "shrink", "document": "file" } },
        { "id": "shrink", "tool": "notify", "args": { "text": "photo" } },
        { "id": "file", "tool": "notify", "args": { "text": "{{pick.value}}" } }
    ]));
    assert_eq!(r.problems(&tools), Vec::<String>::new());
}

#[test]
fn a_value_in_a_command_is_quoted_as_one_word() {
    let values = Values {
        inputs: json!({ "mount": "/mnt/my disk's; rm -rf ~", "n": 3 })
            .as_object()
            .unwrap()
            .clone(),
        ..Values::default()
    };
    assert_eq!(
        fill("df -P {{inputs.mount}}", &values, Quote::Posix).unwrap(),
        r"df -P '/mnt/my disk'\''s; rm -rf ~'"
    );
    assert_eq!(
        fill("Get-PSDrive {{ inputs.mount }}", &values, Quote::PowerShell).unwrap(),
        "Get-PSDrive '/mnt/my disk''s; rm -rf ~'"
    );
    let args = json!({
        "command": "du -sh {{inputs.mount}}",
        "timeout": "{{inputs.n}}",
        "title": "Disk {{inputs.n}}",
        "tags": ["{{inputs.mount}}"]
    });
    let filled = fill_args(
        "run_terminal_command",
        args.as_object().unwrap(),
        &values,
        Quote::Posix,
    )
    .unwrap();
    assert_eq!(
        Value::Object(filled),
        json!({
            "command": r"du -sh '/mnt/my disk'\''s; rm -rf ~'",
            "timeout": 3,
            "title": "Disk 3",
            "tags": ["/mnt/my disk's; rm -rf ~"]
        })
    );
}

#[test]
fn a_command_cant_put_its_own_quotes_around_a_value() {
    let r = routine(json!([
        { "id": "a", "tool": "run_terminal_command", "args": { "command": "echo '{{inputs.n}}'" } },
        { "id": "b", "tool": "run_terminal_command", "args": { "command": "echo \"x\" {{inputs.n}}" } },
        { "id": "c", "tool": "notify", "args": { "text": "'{{inputs.n}}'" } }
    ]));
    assert_eq!(
        r.problems(&tools),
        ["step \"a\": a value in a command is quoted for you; take the quotes off around it"]
    );
}

#[test]
fn outputs_fill_from_text_exit_codes_and_json_paths() {
    let mut values = Values::default();
    values.steps.insert(
        "probe".into(),
        StepOut {
            stdout: r#" {"disks": [{"name": "nvme0", "free": 512, "ok": true}], "note": null} "#
                .into(),
            stderr: "warn".into(),
            exit_code: Some(0),
            value: None,
        },
    );
    values.steps.insert(
        "use".into(),
        StepOut {
            value: Some("93".into()),
            ..StepOut::default()
        },
    );
    let f = |t: &str| fill(t, &values, Quote::Plain);
    assert_eq!(
        f("{{probe.json.disks.0.name}} has {{probe.json.disks.0.free}} MB, ok={{probe.json.disks.0.ok}}[{{probe.json.note}}]"),
        Ok("nvme0 has 512 MB, ok=true[]".into())
    );
    assert_eq!(
        f("{{probe.exit_code}}/{{probe.stderr}}/{{use.value}}"),
        Ok("0/warn/93".into())
    );
    assert_eq!(
        f("{{probe.json.disks.1.free}}"),
        Err("probe's JSON has no disks.1.free".into())
    );
    assert_eq!(f("{{use.exit_code}}"), Err("use has no exit code".into()));
    assert_eq!(f("{{gone.stdout}}"), Err("gone hasn't run".into()));
    assert_eq!(
        f("{{probe.json"),
        Err("\"{{probe.json\" opens {{ without closing it".into())
    );
    assert_eq!(
        f("{{probe.size}}"),
        Err("\"probe.size\" isn't something a routine can read".into())
    );
    assert_eq!(f("{{use.json}}"), Err("use didn't print JSON".into()));
}

#[test]
fn timers_read_the_three_shapes_and_write_them_back() {
    let t = |s: &str| Timer::try_from(s.to_string());
    assert_eq!(t("weekdays 07:30"), Ok(Timer::Weekdays { at_min: 450 }));
    assert_eq!(t("every 15m"), Ok(Timer::Every { minutes: 15 }));
    assert_eq!(t("every 2h"), Ok(Timer::Every { minutes: 120 }));
    assert_eq!(String::from(Timer::Every { minutes: 120 }), "every 2h");
    assert_eq!(
        String::from(Timer::Weekdays { at_min: 450 }),
        "weekdays 07:30"
    );
    assert_eq!(
        t("daily 9am"),
        Err("timer \"daily 9am\": use \"daily 09:00\", \"weekdays 09:00\" or \"every 15m\"".into())
    );
    assert_eq!(
        t("every 25h"),
        Err("timer \"every 25h\": every 1m to every 24h".into())
    );
    assert!(t("every 5é").is_err());
}

#[test]
fn loading_checks_the_folder_name_and_each_script() {
    let dir = std::env::temp_dir().join(format!("routine-load-{}", std::process::id()));
    let folder = dir.join("disk-report");
    std::fs::create_dir_all(&folder).unwrap();
    let text = DISK_REPORT.replace(
        r#"{ "id": "df", "tool""#,
        r#"{ "id": "pre", "script": "check.py" }, { "id": "df", "tool""#,
    );
    std::fs::write(folder.join(FILE), &text).unwrap();
    assert_eq!(
        Routine::load(&folder, &tools).unwrap_err(),
        ["step \"pre\": script \"check.py\" isn't in the folder"]
    );
    std::fs::write(folder.join("check.py"), "print('ok')").unwrap();
    assert_eq!(Routine::load(&folder, &tools).unwrap().steps.len(), 3);
    let moved = dir.join("disk");
    std::fs::rename(&folder, &moved).unwrap();
    assert_eq!(
        Routine::load(&moved, &tools).unwrap_err(),
        ["name \"disk-report\" doesn't match its folder \"disk\""]
    );
    assert_eq!(
        routines_dir(Path::new("/home/user/.config/GrokHub")),
        Path::new("/home/user/.config/GrokHub/routines")
    );
    let _ = std::fs::remove_dir_all(&dir);
}
