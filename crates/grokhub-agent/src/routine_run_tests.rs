use super::*;
use std::cell::RefCell;
use std::path::PathBuf;

fn temp(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("routine-run-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn routine(v: Value) -> Routine {
    let r = Routine::parse(&v.to_string()).unwrap();
    assert_eq!(r.problems(&step_tool), Vec::<String>::new());
    r
}

/// Says yes, and keeps what it was asked.
#[derive(Default)]
struct Yes(RefCell<Vec<(String, String, String)>>);

impl Approve for Yes {
    fn approve(&self, step: &str, tool: &str, arguments: &str) -> bool {
        self.0
            .borrow_mut()
            .push((step.into(), tool.into(), arguments.into()));
        true
    }
}

fn spec<'a>(r: &'a Routine, dir: &'a Path, stop: &'a dyn Fn() -> bool) -> RunSpec<'a> {
    RunSpec {
        routine: r,
        dir,
        workspace: dir,
        inputs: Map::new(),
        dry_run: false,
        approve: None,
        desktop: None,
        stop,
        brain: None,
        words: None,
    }
}

fn disk_usage(dir: &Path) -> Routine {
    std::fs::write(dir.join("usage.txt"), "system disk used 93% today\n").unwrap();
    routine(json!({
        "version": 1, "name": "disk-usage",
        "inputs": { "file": { "type": "string", "default": "usage.txt" }, "limit": { "type": "integer", "default": 90 } },
        "steps": [
            { "id": "read", "tool": "read_file", "args": { "target_file": "{{inputs.file}}" } },
            { "id": "use", "expect": { "from": "read.stdout", "regex": "used (\\d+)%", "group": 1, "at_least": 90 }, "else": "done" }
        ],
        "result": "The disk is {{use.value}}% full."
    }))
}

#[test]
fn a_read_only_routine_runs_unattended_with_no_model_call() {
    let dir = temp("read");
    let r = disk_usage(&dir);
    let stop = || false;
    let report = run(&spec(&r, &dir, &stop));
    assert_eq!(report.ended, Ended::Finished);
    assert_eq!(report.result.as_deref(), Some("The disk is 93% full."));
    assert_eq!(report.model_calls, 0);
    assert_eq!(report.lines[1], "use: read.stdout is 93, as expected");
    // Under the limit: the expect's else ends the run with nothing to show.
    std::fs::write(dir.join("usage.txt"), "system disk used 41% today\n").unwrap();
    let report = run(&spec(&r, &dir, &stop));
    assert_eq!(report.ended, Ended::Done { at: "use".into() });
    assert_eq!(report.result, None);
    assert_eq!(
        report.text().lines().last(),
        Some("Stopped early at use, as planned. Nothing to report.")
    );
}

#[test]
fn a_change_step_is_refused_unattended_and_asks_when_someone_is_there() {
    let dir = temp("change");
    let r = routine(json!({
        "version": 1, "name": "hello",
        "inputs": { "who": { "type": "string", "default": "my disk's" } },
        "steps": [{ "id": "say", "tool": "run_terminal_command", "args": { "command": "echo {{inputs.who}}" } }],
        "result": "{{say.stdout}}"
    }));
    let stop = || false;
    let report = run(&spec(&r, &dir, &stop));
    assert_eq!(
        report.ended,
        Ended::Stopped {
            at: "say".into(),
            why: gate::unattended_deny("run_terminal_command")
        }
    );
    assert_eq!(report.lines, Vec::<String>::new());
    let yes = Yes::default();
    let attended = RunSpec {
        approve: Some(&yes),
        ..spec(&r, &dir, &stop)
    };
    let report = run(&attended);
    let quoted = if cfg!(windows) {
        "'my disk''s'"
    } else {
        r"'my disk'\''s'"
    };
    assert_eq!(
        yes.0.borrow().as_slice(),
        [(
            "say".to_string(),
            "run_terminal_command".to_string(),
            json!({ "command": format!("echo {quoted}") }).to_string()
        )]
    );
    assert_eq!(report.ended, Ended::Finished, "{}", report.text());
    assert!(report.result.unwrap_or_default().contains("my disk's"));
}

#[test]
fn a_dry_run_fills_and_gates_every_step_and_runs_none() {
    let dir = temp("dry");
    let r = routine(json!({
        "version": 1, "name": "dry",
        "inputs": { "mount": { "type": "string", "default": "/" } },
        "steps": [
            { "id": "df", "tool": "run_terminal_command", "args": { "command": "df -P {{inputs.mount}} > {{inputs.mount}}/x" } },
            { "id": "use", "expect": { "from": "df.stdout", "regex": "(\\d+)%", "group": 1, "at_least": 90 }, "else": "done" },
            { "id": "note", "tool": "read_file", "args": { "target_file": "{{df.stdout}}" } },
            { "id": "pick", "ask": "Clean up?", "branches": { "yes": "done", "no": "done" } }
        ]
    }));
    let stop = || false;
    let yes = Yes::default();
    let report = run(&RunSpec {
        dry_run: true,
        approve: Some(&yes),
        ..spec(&r, &dir, &stop)
    });
    // `'/'` is the same word in Bash and PowerShell.
    let q = "'/'";
    assert_eq!(
        report.lines,
        [
            format!(
                "df: run_terminal_command {} (asks you first)",
                json!({ "command": format!("df -P {q} > {q}/x") })
            ),
            "use: checks df.stdout; on a miss goes to done".to_string(),
            format!(
                "note: read_file {} (runs)",
                json!({ "target_file": "{{df.stdout}}" })
            ),
            "pick: asks \"Clean up?\" (no / yes) (refused: routines can't ask the model yet)"
                .to_string(),
        ]
    );
    assert_eq!(report.ended, Ended::Finished);
    assert_eq!(yes.0.borrow().len(), 0, "a dry run asks nothing");
}

#[test]
fn inputs_are_typed_and_a_missing_one_stops_before_any_step() {
    let dir = temp("inputs");
    let r = routine(json!({
        "version": 1, "name": "inputs",
        "inputs": { "path": { "type": "string" }, "count": { "type": "integer", "default": 3 } },
        "steps": [{ "id": "read", "tool": "read_file", "args": { "target_file": "{{inputs.path}}" } }]
    }));
    let stop = || false;
    let stopped = |inputs: Value| {
        run(&RunSpec {
            inputs: inputs.as_object().unwrap().clone(),
            ..spec(&r, &dir, &stop)
        })
        .ended
    };
    let at = |why: &str| Ended::Stopped {
        at: "inputs".into(),
        why: why.into(),
    };
    assert_eq!(stopped(json!({})), at("it needs a value for inputs.path"));
    assert_eq!(
        stopped(json!({ "path": "a", "count": "three" })),
        at("inputs.count must be a integer, not \"three\"")
    );
    assert_eq!(
        stopped(json!({ "path": "a", "size": 1 })),
        at("this routine has no input named size")
    );
    assert_eq!(
        parse_sets(&r, &["count=7".into(), "path=a b=c".into()]),
        Ok(json!({ "count": 7, "path": "a b=c" })
            .as_object()
            .unwrap()
            .clone())
    );
    assert_eq!(
        parse_sets(&r, &["count=lots".into()]),
        Err("--set count=lots: count must be a integer".into())
    );
}

#[test]
fn an_ask_a_halt_and_a_missing_desktop_each_stop_the_run() {
    let dir = temp("stops");
    let r = routine(json!({
        "version": 1, "name": "stops",
        "steps": [
            { "id": "click", "desktop": { "screen": null, "ops": [{ "Click": { "x": 10, "y": 20 } }] } },
            { "id": "pick", "ask": "Which?", "branches": { "a": "done", "b": "done" } }
        ]
    }));
    let halted = || true;
    assert_eq!(
        run(&spec(&r, &dir, &halted)).ended,
        Ended::Stopped {
            at: "click".into(),
            why: "halted".into()
        }
    );
    let stop = || false;
    assert_eq!(
        run(&spec(&r, &dir, &stop)).ended,
        Ended::Stopped {
            at: "click".into(),
            why: "desktop steps need the app's desktop control".into()
        }
    );
}

/// A desktop that keeps the calls it gets.
#[derive(Default)]
struct Desk(RefCell<Vec<(String, Value)>>);

impl DesktopOps for Desk {
    fn halted(&self) -> bool {
        false
    }
    fn locked(&self) -> bool {
        false
    }
    fn call(&self, name: &str, args: &Value) -> crate::tools::ToolOutput {
        self.0.borrow_mut().push((name.into(), args.clone()));
        crate::tools::ToolOutput::ok(format!("{name} done"))
    }
}

#[test]
fn a_desktop_replay_goes_through_the_gate_one_op_at_a_time() {
    let dir = temp("desk");
    let r = routine(json!({
        "version": 1, "name": "desk",
        "steps": [
            { "id": "open", "desktop": { "screen": null, "ops": [
                { "DoubleClick": { "x": 10, "y": 20 } }, { "Type": { "text": "notes" } }, { "Key": { "name": "Return" } }
            ] } },
            { "id": "spin", "desktop": { "screen": null, "ops": [{ "Cursor": null }] } }
        ]
    }));
    let stop = || false;
    let desk = Desk::default();
    let yes = Yes::default();
    let report = run(&RunSpec {
        desktop: Some(&desk),
        approve: Some(&yes),
        ..spec(&r, &dir, &stop)
    });
    assert_eq!(
        desk.0.borrow().as_slice(),
        [
            (
                "click".to_string(),
                json!({ "x": 10, "y": 20, "double": true })
            ),
            ("type".to_string(), json!({ "text": "notes" })),
            ("key".to_string(), json!({ "keys": "Return" })),
        ]
    );
    assert_eq!(
        yes.0
            .borrow()
            .iter()
            .map(|c| c.1.as_str())
            .collect::<Vec<_>>(),
        ["click", "type", "key"]
    );
    assert_eq!(report.lines, ["open: click done\ntype done\nkey done"]);
    assert_eq!(
        report.ended,
        Ended::Stopped {
            at: "spin".into(),
            why: "a saved Cursor can't replay in a routine yet".into()
        }
    );
}

#[test]
fn a_script_step_reaches_the_sandbox_with_its_args() {
    let dir = temp("script");
    std::fs::write(dir.join("check.py"), "print(ARGS['mount'])\n").unwrap();
    let r = routine(json!({
        "version": 1, "name": "script",
        "inputs": { "mount": { "type": "string", "default": "/srv" } },
        "steps": [{ "id": "check", "script": "check.py", "args": { "mount": "{{inputs.mount}}" } }]
    }));
    let stop = || false;
    let s = spec(&r, &dir, &stop);
    let policy = crate::perm::Policy::load(&dir);
    let runner = Runner {
        spec: &s,
        gate: Gate {
            mode: PermMode::Ask,
            readonly_session: false,
            attended: false,
            desktop: false,
        },
        policy: &policy,
    };
    let values = Values {
        inputs: json!({ "mount": "/srv" }).as_object().unwrap().clone(),
        ..Values::default()
    };
    let StepKind::Script { file, args } = &r.steps[0].kind else {
        unreachable!()
    };
    let call: Value =
        serde_json::from_str(&runner.script_call(file, args, &values).unwrap()).unwrap();
    assert_eq!(
        call["script"],
        "import json as _json\nARGS = _json.loads(\"{\\\"mount\\\":\\\"/srv\\\"}\")\nprint(ARGS['mount'])\n"
    );
    // A tool script runs unattended only where the OS sandbox fully holds it
    // (writes fenced, the network only through the proxy). Elsewhere, as on
    // Windows or a Linux without unprivileged namespaces, it asks, and with
    // nobody here to answer it is refused.
    let refused =
        "Tool `run_tool_script` was not executed: Denied by permission policy: deny rule on edit";
    let want = if tools::ptc::sandbox_holds(&dir) {
        Ok(())
    } else {
        Err(refused.to_string())
    };
    assert_eq!(
        runner.cleared("check", tools::ptc::NAME, &call.to_string()),
        want
    );
}

#[test]
fn a_failed_command_stops_unless_the_next_step_checks_its_exit_code() {
    let dir = temp("exit");
    let checked = routine(json!({
        "version": 1, "name": "exit",
        "steps": [
            { "id": "find", "tool": "run_terminal_command", "args": { "command": "exit 3" } },
            { "id": "missing", "expect": { "from": "find.exit_code", "equals": 0 }, "else": "done" }
        ]
    }));
    let stop = || false;
    let yes = Yes::default();
    let report = run(&RunSpec {
        approve: Some(&yes),
        ..spec(&checked, &dir, &stop)
    });
    assert_eq!(
        report.ended,
        Ended::Done {
            at: "missing".into()
        },
        "{}",
        report.text()
    );
    assert_eq!(
        report.lines.last().map(String::as_str),
        Some("missing: find.exit_code is 3, so on to done")
    );
    let mut unchecked = checked.clone();
    unchecked.steps.truncate(1);
    let report = run(&RunSpec {
        approve: Some(&yes),
        ..spec(&unchecked, &dir, &stop)
    });
    assert_eq!(
        report.ended,
        Ended::Stopped {
            at: "find".into(),
            why: "run_terminal_command failed: exit 3".into()
        }
    );
    assert_eq!(
        exit_line("run_terminal_command", "exit 0\nFilesystem\n/dev/vda"),
        Some((0, "Filesystem\n/dev/vda"))
    );
    assert_eq!(exit_line("read_file", "exit 0\nx"), None);
}

/// A stand-in model: the replies it was given, in order.
struct Model(RefCell<Vec<String>>, RefCell<Vec<&'static str>>);

impl Model {
    fn new(replies: &[&str]) -> Model {
        Model(
            RefCell::new(replies.iter().map(|r| r.to_string()).collect()),
            RefCell::default(),
        )
    }
}

impl Brain for Model {
    fn ask(
        &self,
        class: &'static str,
        format: crate::client::TextFormat,
        _: &str,
        _: &str,
    ) -> Option<String> {
        self.1.borrow_mut().push(format.name);
        assert_eq!(
            class,
            if format.name == "routine_repair" {
                "routine:repair"
            } else {
                "routine:fill"
            }
        );
        let mut r = self.0.borrow_mut();
        (!r.is_empty()).then(|| r.remove(0))
    }
}

#[test]
fn the_model_fills_a_missing_input_from_the_words_that_started_the_run() {
    let dir = temp("fill");
    std::fs::write(dir.join("notes.txt"), "used 97%\n").unwrap();
    let r = routine(json!({
        "version": 1, "name": "fill",
        "inputs": { "file": { "type": "string" } },
        "steps": [{ "id": "read", "tool": "read_file", "args": { "target_file": "{{inputs.file}}" } }],
        "result": "read {{inputs.file}}"
    }));
    let stop = || false;
    let model = Model::new(&[r#"{"values":[{"name":"file","value":"notes.txt"}]}"#]);
    let report = run(&RunSpec {
        brain: Some(&model),
        words: Some("check notes.txt"),
        ..spec(&r, &dir, &stop)
    });
    assert_eq!(report.ended, Ended::Finished, "{}", report.text());
    assert_eq!(report.lines[0], "inputs: the model filled inputs.file");
    assert_eq!(report.result.as_deref(), Some("read notes.txt"));
    assert_eq!(report.model_calls, 1);
    // With no words to fill from, the model isn't asked.
    let model = Model::new(&[]);
    let report = run(&RunSpec {
        brain: Some(&model),
        ..spec(&r, &dir, &stop)
    });
    assert_eq!(
        report.ended,
        Ended::Stopped {
            at: "inputs".into(),
            why: "it needs a value for inputs.file".into()
        }
    );
    assert_eq!(model.1.borrow().len(), 0);
}

#[test]
fn an_ask_step_takes_the_branch_the_model_names() {
    let dir = temp("branch");
    let r = routine(json!({
        "version": 1, "name": "branch",
        "steps": [
            { "id": "pick", "ask": "Photo or document?", "branches": { "photo": "shrink", "document": "done" } },
            { "id": "shrink", "tool": "list_dir", "args": { "target_directory": "." } }
        ],
        "result": "{{pick.value}}"
    }));
    let stop = || false;
    let model = Model::new(&[r#"{"branch":"photo","why":"It's a JPEG."}"#]);
    let report = run(&RunSpec {
        brain: Some(&model),
        ..spec(&r, &dir, &stop)
    });
    assert_eq!(report.ended, Ended::Finished, "{}", report.text());
    assert_eq!(report.lines[0], "pick: the model chose photo: It's a JPEG.");
    assert_eq!(report.result.as_deref(), Some("photo"));
    let model = Model::new(&[r#"{"branch":"document","why":"A PDF."}"#]);
    assert_eq!(
        run(&RunSpec {
            brain: Some(&model),
            ..spec(&r, &dir, &stop)
        })
        .ended,
        Ended::Done { at: "pick".into() }
    );
    let model = Model::new(&[r#"{"branch":"video","why":""}"#]);
    assert_eq!(
        run(&RunSpec {
            brain: Some(&model),
            ..spec(&r, &dir, &stop)
        })
        .ended,
        Ended::Stopped {
            at: "pick".into(),
            why: "the model chose \"video\", which isn't one of document, photo".into()
        }
    );
}

#[test]
fn a_failed_step_gets_one_repair_and_the_run_at_most_two() {
    let dir = temp("repair");
    std::fs::write(dir.join("real.txt"), "used 95%\n").unwrap();
    let r = routine(json!({
        "version": 1, "name": "repair",
        "steps": [
            { "id": "read", "tool": "read_file", "args": { "target_file": "gone.txt" } },
            { "id": "use", "expect": { "from": "read.stdout", "regex": "used (\\d+)%", "group": 1 } },
            { "id": "again", "tool": "read_file", "args": { "target_file": "gone.txt" } },
            { "id": "last", "tool": "read_file", "args": { "target_file": "gone.txt" } }
        ],
        "result": "{{use.value}}"
    }));
    let stop = || false;
    // Retry with new arguments, then skip: two repairs, then the third
    // failure stops the run.
    let model = Model::new(&[
        r#"{"do":"retry","args_json":"{\"target_file\":\"real.txt\"}","why":"The file is real.txt."}"#,
        r#"{"do":"skip","args_json":"{}","why":"Not needed."}"#,
    ]);
    let report = run(&RunSpec {
        brain: Some(&model),
        ..spec(&r, &dir, &stop)
    });
    assert!(
        report.lines[0].starts_with("read: read_file failed: "),
        "{}",
        report.lines[0]
    );
    assert!(
        report.lines[0].ends_with("; trying again: The file is real.txt."),
        "{}",
        report.lines[0]
    );
    assert_eq!(report.lines[2], "use: read.stdout is 95, as expected");
    assert!(
        report.lines[3].ends_with("; skipped: Not needed."),
        "{}",
        report.lines[3]
    );
    let Ended::Stopped { at, .. } = &report.ended else {
        panic!("{:?}", report.ended)
    };
    assert_eq!(at, "last");
    assert_eq!(report.model_calls, 2);
    // The retry that worked is a fix; the skip is not.
    assert_eq!(
        report.fixes,
        vec![Fix {
            step: "read".into(),
            args: json!({ "target_file": "real.txt" })
                .as_object()
                .unwrap()
                .clone()
        }]
    );
    assert_eq!(
        model.1.borrow().as_slice(),
        ["routine_repair", "routine_repair"]
    );
    // A retry that fails again isn't repaired twice.
    let model = Model::new(&[
        r#"{"do":"retry","args_json":"{\"target_file\":\"still-gone.txt\"}","why":"Try again."}"#,
        r#"{"do":"skip","args_json":"{}","why":"unused"}"#,
    ]);
    let report = run(&RunSpec {
        brain: Some(&model),
        ..spec(&r, &dir, &stop)
    });
    let Ended::Stopped { at, why } = &report.ended else {
        panic!("{:?}", report.ended)
    };
    assert_eq!(at, "read");
    assert!(why.ends_with(" (it still failed after one fix)"), "{why}");
    assert_eq!(report.model_calls, 1);
    assert_eq!(report.fixes, Vec::new());
    // Replan hands the job to a full turn.
    let model =
        Model::new(&[r#"{"do":"replan","args_json":"{}","why":"There is no such file here."}"#]);
    assert_eq!(
        run(&RunSpec {
            brain: Some(&model),
            ..spec(&r, &dir, &stop)
        })
        .ended,
        Ended::Replan {
            at: "read".into(),
            why: "There is no such file here.".into()
        }
    );
}

#[test]
fn a_refused_step_is_never_repaired() {
    let dir = temp("refused");
    let r = routine(json!({
        "version": 1, "name": "refused",
        "steps": [{ "id": "say", "tool": "run_terminal_command", "args": { "command": "echo hi" } }]
    }));
    let stop = || false;
    let model =
        Model::new(&[r#"{"do":"retry","args_json":"{\"command\":\"echo hi\"}","why":"x"}"#]);
    let report = run(&RunSpec {
        brain: Some(&model),
        ..spec(&r, &dir, &stop)
    });
    assert_eq!(
        report.ended,
        Ended::Stopped {
            at: "say".into(),
            why: gate::unattended_deny("run_terminal_command")
        }
    );
    assert_eq!(report.model_calls, 0);
}

#[test]
fn a_value_read_as_an_option_stops_the_run_unrepaired_and_the_dry_run_says_so() {
    let dir = temp("option");
    let r = routine(json!({
        "version": 1, "name": "fetch",
        "inputs": { "r": { "type": "string", "default": "main" } },
        "steps": [{ "id": "fetch", "tool": "run_terminal_command", "args": { "command": "git fetch origin {{inputs.r}}" } }]
    }));
    let stop = || false;
    let inputs: Map<String, Value> = json!({ "r": "--upload-pack=touch PWNED" })
        .as_object()
        .unwrap()
        .clone();
    let model = Model::new(&[
        r#"{"do":"retry","args_json":"{\"command\":\"git fetch origin --upload-pack=touch PWNED\"}","why":"x"}"#,
    ]);
    let yes = Yes::default();
    let why = "inputs.r starts with \"-\" and would be read as an option; put -- before it in the step, or join it to its flag (--name=...)";
    let report = run(&RunSpec {
        inputs: inputs.clone(),
        brain: Some(&model),
        approve: Some(&yes),
        ..spec(&r, &dir, &stop)
    });
    assert_eq!(
        report.ended,
        Ended::Stopped {
            at: "fetch".into(),
            why: why.into()
        }
    );
    assert_eq!(report.model_calls, 0);
    assert_eq!(yes.0.borrow().len(), 0);
    let dry = run(&RunSpec {
        inputs,
        dry_run: true,
        approve: Some(&yes),
        ..spec(&r, &dir, &stop)
    });
    // `'--upload-pack=touch PWNED'` is the same word in Bash and PowerShell.
    assert_eq!(
        dry.lines,
        [format!(
            "fetch: run_terminal_command {} (refused: {why})",
            json!({ "command": "git fetch origin '--upload-pack=touch PWNED'" })
        )]
    );
}

/// Allows only the steps it names, as a routine's consent does, and keeps
/// what it was asked.
struct Only(&'static str, RefCell<Vec<(String, String)>>);

impl Approve for Only {
    fn approve(&self, step: &str, _: &str, arguments: &str) -> bool {
        self.1.borrow_mut().push((step.into(), arguments.into()));
        step == self.0
    }
}

#[test]
fn a_retry_with_new_arguments_is_not_covered_by_the_steps_consent() {
    let dir = temp("retry-consent");
    let r = routine(json!({
        "version": 1, "name": "retry-consent",
        "steps": [{ "id": "run", "tool": "run_terminal_command", "args": { "command": "exit 3" } }]
    }));
    let stop = || false;
    let model = Model::new(&[
        r#"{"do":"retry","args_json":"{\"command\":\"echo replaced\"}","why":"Try this."}"#,
    ]);
    let consent = Only("run", RefCell::default());
    let report = run(&RunSpec {
        brain: Some(&model),
        approve: Some(&consent),
        ..spec(&r, &dir, &stop)
    });
    assert_eq!(
        report.ended,
        Ended::Stopped {
            at: "run".into(),
            why: "you didn't allow run_terminal_command".into()
        }
    );
    assert_eq!(
        consent.1.borrow().as_slice(),
        [
            (
                "run".to_string(),
                json!({ "command": "exit 3" }).to_string()
            ),
            (
                "run (retry)".to_string(),
                json!({ "command": "echo replaced" }).to_string()
            )
        ]
    );
    assert_eq!(report.model_calls, 1);
    assert_eq!(report.fixes, Vec::new());
}

#[test]
fn a_failed_check_is_never_skipped_by_a_repair() {
    let dir = temp("skip-check");
    std::fs::write(dir.join("disk.txt"), "used 50%\n").unwrap();
    std::fs::write(dir.join("after.txt"), "after\n").unwrap();
    let r = routine(json!({
        "version": 1, "name": "skip-check",
        "steps": [
            { "id": "read", "tool": "read_file", "args": { "target_file": "disk.txt" } },
            { "id": "full", "expect": { "from": "read.stdout", "regex": "used (\\d+)%", "group": 1, "at_least": 90 } },
            { "id": "after", "tool": "read_file", "args": { "target_file": "after.txt" } }
        ]
    }));
    let stop = || false;
    let model = Model::new(&[r#"{"do":"skip","args_json":"{}","why":"Not needed."}"#]);
    let report = run(&RunSpec {
        brain: Some(&model),
        ..spec(&r, &dir, &stop)
    });
    let Ended::Stopped { at, why } = &report.ended else {
        panic!("{:?}", report.ended)
    };
    assert_eq!(at, "full");
    assert!(
        why.ends_with(". A check can't be skipped: Not needed."),
        "{why}"
    );
    assert_eq!(report.model_calls, 1);
}

#[test]
fn a_fix_keeps_the_templates_the_retry_left_alone() {
    let dir = temp("fix-templates");
    std::fs::create_dir_all(dir.join("in")).unwrap();
    std::fs::write(dir.join("in/notes.txt"), "x\n").unwrap();
    let r = routine(json!({
        "version": 1, "name": "fixed",
        "inputs": { "file": { "type": "string", "default": "notes.txt" } },
        "steps": [{ "id": "read", "tool": "read_file", "args": { "target_file": "{{inputs.file}}", "offset": 1 } }]
    }));
    let stop = || false;
    // The model keeps the offset and points the file into in/.
    let model = Model::new(&[
        r#"{"do":"retry","args_json":"{\"target_file\":\"in/notes.txt\",\"offset\":1}","why":"It moved."}"#,
    ]);
    let report = run(&RunSpec {
        brain: Some(&model),
        ..spec(&r, &dir, &stop)
    });
    assert_eq!(report.ended, Ended::Finished, "{}", report.text());
    assert_eq!(
        report.fixes,
        vec![Fix {
            step: "read".into(),
            args: json!({ "target_file": "in/notes.txt", "offset": 1 })
                .as_object()
                .unwrap()
                .clone()
        }]
    );
    let keep = fix_args(
        &r.steps[0].kind,
        json!({ "target_file": "notes.txt", "offset": 2 })
            .as_object()
            .unwrap(),
        &Values {
            inputs: json!({ "file": "notes.txt" }).as_object().unwrap().clone(),
            ..Values::default()
        },
    );
    assert_eq!(
        keep,
        Some(
            json!({ "target_file": "{{inputs.file}}", "offset": 2 })
                .as_object()
                .unwrap()
                .clone()
        )
    );
}
