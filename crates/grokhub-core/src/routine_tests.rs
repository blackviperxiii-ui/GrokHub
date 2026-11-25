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
fn a_value_that_reads_as_an_option_never_reaches_a_command() {
    let values = Values {
        inputs: json!({
            "r": "--upload-pack=touch /tmp/pwned",
            "n": "-5",
            "line": "-5\n",
            "i": "-inf",
            "list": "- fix typo",
            "q": "a\" --upload-pack=calc \"b c",
            "dir": "C:\\Program Files\\"
        })
        .as_object()
        .unwrap()
        .clone(),
        ..Values::default()
    };
    let option = |r: &str| {
        Err(FillError::Unsafe(format!(
            "{r} starts with \"-\" and would be read as an option; put -- before it in the step, or join it to its flag (--name=...)"
        )))
    };
    for quote in [Quote::Posix, Quote::PowerShell] {
        assert_eq!(
            fill("git fetch origin {{inputs.r}}", &values, quote),
            option("inputs.r")
        );
    }
    assert_eq!(
        fill("git log ''{{inputs.r}}", &values, Quote::Posix),
        option("inputs.r")
    );
    assert_eq!(
        fill("sort {{inputs.i}}", &values, Quote::Posix),
        option("inputs.i")
    );
    // Numbers, a value joined to its flag or after `--`, and a file name can
    // start with `-`.
    for (template, filled) in [
        ("head -n {{inputs.n}} log.txt", "head -n '-5' log.txt"),
        ("head -n {{inputs.line}}", "head -n '-5\n'"),
        (
            "git commit --message={{inputs.list}}",
            "git commit --message='- fix typo'",
        ),
        (
            "git rm -- {{inputs.r}}",
            "git rm -- '--upload-pack=touch /tmp/pwned'",
        ),
        (
            "sort notes.txt > {{inputs.list}}",
            "sort notes.txt > '- fix typo'",
        ),
    ] {
        assert_eq!(
            fill(template, &values, Quote::Posix).unwrap(),
            filled,
            "{template}"
        );
    }
    // Windows PowerShell hands a program `"` as is, so the value would split.
    assert_eq!(
        fill("git log {{inputs.q}}", &values, Quote::PowerShell),
        Err(FillError::Unsafe(
            "inputs.q has a double quote, which Windows can't pass to a program safely".into()
        ))
    );
    assert_eq!(
        fill("git log {{inputs.q}}", &values, Quote::Posix).unwrap(),
        r#"git log 'a" --upload-pack=calc "b c'"#
    );
    assert_eq!(
        fill("dir {{inputs.dir}}", &values, Quote::PowerShell),
        Err(FillError::Unsafe(
            r#"inputs.dir has a space and ends with "\", which Windows reads as a quote"#.into()
        ))
    );
    // A message is no command: it keeps the value as written.
    assert_eq!(
        fill("Fetched {{inputs.r}}", &values, Quote::Plain).unwrap(),
        "Fetched --upload-pack=touch /tmp/pwned"
    );
}

#[test]
fn a_default_that_reads_as_an_option_is_refused_when_saved() {
    let r = Routine::parse(
        &json!({
            "version": 1, "name": "t",
            "inputs": { "profile": { "type": "string", "default": "--release" } },
            "steps": [
                { "id": "a", "tool": "run_terminal_command", "args": { "command": "cargo build {{inputs.profile}}" } },
                { "id": "b", "tool": "notify", "args": { "text": "{{inputs.profile}}" } }
            ]
        })
        .to_string(),
    )
    .unwrap();
    assert_eq!(
        r.problems(&tools),
        ["step \"a\": inputs.profile defaults to \"--release\", which would be read as an option; write it in the step instead"]
    );
}

#[test]
fn a_command_cant_put_a_value_where_the_shell_runs_it_as_code() {
    // Bash rules, so read as Linux and macOS read it on every OS (Windows
    // reads a routine's command as PowerShell).
    let problem = |command: &str| -> Vec<String> {
        command::problem(command, Quote::Posix)
            .map(|why| format!("step \"a\": {why}"))
            .into_iter()
            .collect()
    };
    let refused = |why: &str, commands: &[&str]| {
        for command in commands {
            assert_eq!(
                problem(command),
                [format!("step \"a\": {why}")],
                "{command}"
            );
        }
    };
    refused(
        "a value in a command is quoted for you; take the quotes off around it",
        &[
            // `\"` doesn't close the string in bash, so the value sits inside it.
            r#"echo "x\" {{inputs.n}} ""#,
            r#"echo "logs\{{inputs.n}}""#,
            // The apostrophe is in a comment, so the quotes around the value count.
            "# don't run on main\necho --author '{{inputs.n}}'",
            r"echo $'It\'s done: {{inputs.n}}'",
            "echo `{{inputs.n}}`",
        ],
    );
    refused(
        "a value can't come right after \"$\"; put a space before it",
        &["echo ${{inputs.n}}"],
    );
    refused(
        "a value can't come right after \"\\\"; put a space before it",
        &[r"echo \{{inputs.n}}"],
    );
    refused(
        "a value can't go in a heredoc; pass it as an argument",
        &[
            "cat <<EOF\n{{inputs.n}}\nEOF",
            "cat <<-'END'\n\tline {{inputs.n}}\n\tEND",
        ],
    );
    refused("a value can't go in a comment", &["ls # {{inputs.n}}"]);
    refused(
        "a value can't go where the shell does arithmetic ([[ ]], (( )), let); compare it with [ ] instead",
        &[
            "[[ {{inputs.n}} -gt 5 ]] && echo many",
            "let n={{inputs.n}}+1",
            "declare -i n={{inputs.n}}",
            "echo $(( {{inputs.n}} + 1 ))",
            "(( {{inputs.n}} > 1 ))",
            "arr[{{inputs.n}}]=1",
        ],
    );
    refused(
        "a value can't go inside ${...}",
        &["echo ${HOME:-{{inputs.n}}}"],
    );
    refused(
        "a value can't be the command itself; name the program in the step",
        &[
            "echo $({{inputs.n}})",
            "sudo {{inputs.n}}",
            "sudo -u root {{inputs.n}}",
            "timeout 5s {{inputs.n}}",
            "xargs -I {} {{inputs.n}}",
            "nice -n 5 {{inputs.n}}",
            "if true; then {{inputs.n}} {{inputs.n}}; fi",
            "{ {{inputs.n}} x; }",
            "! {{inputs.n}} x",
            "ls & {{inputs.n}}",
            "find . -name x -exec {{inputs.n}} ;",
            "env -S {{inputs.n}}",
            "./{{inputs.n}}",
        ],
    );
    for name in ["eval", "watch", "source"] {
        refused(
            &format!("a value can't follow {name}, which runs it as code"),
            &[&format!("{name} {{{{inputs.n}}}}")],
        );
    }
    for (name, commands) in [
        (
            "sh",
            &[
                "sh -c {{inputs.n}}",
                "env -u HOME sh -c {{inputs.n}}",
                r"find . -maxdepth 0 -exec sh -c {{inputs.n}} \;",
                "docker exec web sh -c {{inputs.n}}",
                "kubectl exec app -- sh -c {{inputs.n}}",
                "busybox sh -c {{inputs.n}}",
            ][..],
        ),
        (
            "bash",
            &[
                "bash -lc {{inputs.n}}",
                "bash -cex {{inputs.n}}",
                "bash -c -- {{inputs.n}}",
                "bash -c -x {{inputs.n}}",
                "bash -c -o pipefail {{inputs.n}}",
                "timeout -s KILL 10 bash -c {{inputs.n}}",
                "sudo -u deploy bash -c {{inputs.n}}",
                "exec -a job bash -c {{inputs.n}}",
                // Quoted text glued to the value is the same -c word.
                "bash -c 'echo checkout '{{inputs.n}}",
                "bash -c 'cd /repo && git checkout '{{inputs.n}}",
            ],
        ),
        (
            "python3",
            &[
                "/usr/bin/python3 -c {{inputs.n}}",
                "python3 -Bc {{inputs.n}}",
            ],
        ),
        ("py", &["py -c {{inputs.n}}"]),
        (
            "perl",
            &[
                "perl -ne {{inputs.n}} notes.txt",
                "perl -Mstrict -e {{inputs.n}}",
            ],
        ),
        ("ruby", &["ruby -r json -e {{inputs.n}}"]),
        (
            "node",
            &[
                "node -p {{inputs.n}}",
                "node -pe {{inputs.n}}",
                "node --eval={{inputs.n}}",
                "node --eval {{inputs.n}}",
            ],
        ),
        ("bun", &["bun -e {{inputs.n}}"]),
        ("php", &["php -r {{inputs.n}}"]),
        (
            "pwsh",
            &[
                "pwsh -NoProfile -Command {{inputs.n}}",
                "pwsh -NoProfile -Command Write-Output {{inputs.n}}",
                "pwsh -ec {{inputs.n}}",
            ],
        ),
        ("powershell", &["powershell Get-Item {{inputs.n}}"]),
        (
            "cmd",
            &[
                "cmd /c {{inputs.n}}",
                "cmd /c echo {{inputs.n}}",
                "cmd /r {{inputs.n}}",
            ],
        ),
        ("su", &["su - deploy -c {{inputs.n}}"]),
        ("flock", &["flock ./lk -c {{inputs.n}}"]),
    ] {
        refused(
            &format!("a value can't be the script {name} runs; pass it as an argument"),
            commands,
        );
    }
    refused(
        "a value can't be the command ssh runs on the other computer",
        &[
            "ssh build-box {{inputs.n}}",
            "ssh -p 2222 build-box uptime {{inputs.n}}",
        ],
    );
    refused(
        "a value can't set an ssh option",
        &[
            "ssh -o {{inputs.n}} build-box uptime",
            "ssh -oProxyCommand={{inputs.n}} build-box",
        ],
    );
    refused(
        "a value can't be the program sed runs; pass it as a file name",
        &[
            "sed -n {{inputs.n}} notes.txt",
            "sed -e {{inputs.n}} notes.txt",
        ],
    );
    refused(
        "a value can't be the program awk runs; pass it as a file name",
        &["awk -v n=5 {{inputs.n}} notes.txt"],
    );
    refused(
        "a value can't be piped into sh, which runs what it reads",
        &["echo {{inputs.n}} | sh"],
    );
    refused(
        "a value can't be piped into xargs, which runs what it reads",
        &["echo {{inputs.n}} | xargs git fetch origin"],
    );
    refused(
        "a value can't be the input of bash, which runs what it reads",
        &["bash <<< {{inputs.n}}"],
    );
    refused(
        "a value can't name a git setting; name it in the step",
        &[
            "git -c {{inputs.n}} fetch",
            "git config --global {{inputs.n}} x",
        ],
    );
    refused(
        "a value can't name an option; write the option in the step",
        &["git log --{{inputs.n}}"],
    );
    // A value as an ordinary argument is fine.
    for ok in [
        "git fetch origin {{inputs.n}}",
        "grep -e {{inputs.n}} notes.txt",
        "sed -n 1p {{inputs.n}}",
        "timeout 5 du -sh {{inputs.n}}",
        "ssh -i {{inputs.n}} build-box uptime",
        "ssh -p 2222 {{inputs.n}} uptime",
        "ssh user@{{inputs.n}} uptime",
        "ssh -o BatchMode=yes {{inputs.n}} uptime",
        "find {{inputs.n}} -name '*.log'",
        "cat <<< {{inputs.n}}",
        r"echo \' {{inputs.n}}",
        r#"bash -c 'echo "$1"' _ {{inputs.n}}"#,
        "bash -e {{inputs.n}}",
        "pwsh -File {{inputs.n}}",
        "command -v {{inputs.n}}",
        r"printf '<<%s>>\n' {{inputs.n}}",
        "cat <<EOF\nhello\nEOF\ngrep -c hello {{inputs.n}}",
        "echo {{inputs.n}} | python3 parse.py",
        "[ {{inputs.n}} -gt 5 ] && echo many",
        "FOO={{inputs.n}} make build",
        "export FOO={{inputs.n}}",
        "git config user.name {{inputs.n}}",
        "git commit --message={{inputs.n}}",
        "echo {{inputs.n}} > out.txt # done",
        "echo hi # it's done\necho {{inputs.n}}",
        "ps -C bash -o pid {{inputs.n}}",
    ] {
        assert_eq!(problem(ok), Vec::<String>::new(), "{ok}");
    }
}

#[test]
fn a_value_in_a_nested_part_or_a_variable_name_is_refused() {
    // Bash rules, so read as Linux and macOS read it on every OS (Windows
    // reads a routine's command as PowerShell).
    let problem = |command: &str| -> Vec<String> {
        command::problem(command, Quote::Posix)
            .map(|why| format!("step \"a\": {why}"))
            .into_iter()
            .collect()
    };
    let refused = |why: &str, commands: &[&str]| {
        for command in commands {
            assert_eq!(
                problem(command),
                [format!("step \"a\": {why}")],
                "{command}"
            );
        }
    };
    // Each of these runs the value in bash once filled. Where a part nested
    // in quotes or a substitution has quotes, escapes or substitutions of its
    // own, or a comment, heredoc or case inside $(...), where it ends turns
    // on rules the check doesn't follow, so no value goes in that command.
    refused(
        "a value can't go in a command with a part nested in quotes or substitutions that this check can't follow; make that part plain (\"$(date +%F)\", not \"$(date +\"%F\")\"), or move it into a script step",
        &[
            r#"echo "$(date +"%F") {{inputs.n}}""#,
            r#"echo "Branch: `git -C "{{inputs.n}}" branch --show-current`""#,
            r#"echo "Result: ${RESULT:-"{{inputs.n}}"}""#,
            "echo ${NAME:-'}'} ' {{inputs.n}} '",
            "echo \"$(echo hi # ) \" '\n' {{inputs.n}} ')\"",
            r#"echo "$(case a in a) echo " {{inputs.n}} ";; esac)""#,
            r#"echo "${A:-$(echo } "{{inputs.n}}" )}""#,
            r#"echo "$(echo ${A:-)} "{{inputs.n}}" )""#,
            "echo \"$( (echo a)#)\" \"\necho \"{{inputs.n}}\"\n)\"",
            // A value before the part counts too: what follows could pipe it.
            r#"echo {{inputs.n}} "$(case a in a) echo;; esac)" | bash"#,
            // Without `))` straight after the inner match, bash reads two
            // subshells, and `#` starts a comment that hides the `"`.
            "((echo a # ) ) \"\n\" {{inputs.n}} \" ))",
            // Bash's end word is `${E}` as written, so the body runs on.
            "cat <<${E}\n$\necho {{inputs.n}}\n${E}",
        ],
    );
    for (name, command) in [
        ("bash", "bash <(echo {{inputs.n}})"),
        ("source", "source <(echo {{inputs.n}})"),
        ("python3", "python3 <(echo {{inputs.n}})"),
        ("bash", "< <(echo {{inputs.n}}) bash"),
        ("exec", "exec 3< <(echo {{inputs.n}}); bash /dev/fd/3"),
        // The value outside, the program inside.
        ("bash", "echo {{inputs.n}} > >(bash); sleep 0.5"),
        ("sh", "echo {{inputs.n}} | tee >(sh) >/dev/null"),
        ("awk", "awk -f <(echo {{inputs.n}}) /dev/null"),
        ("$BASH", r#""$BASH" <(echo {{inputs.n}})"#),
    ] {
        refused(
            &format!(
                "a value can't go in a <(...) in a line where {name} runs a script; pass it as an argument"
            ),
            &[command],
        );
    }
    refused(
        "a value can't be the input of bash, which runs what it reads",
        &[
            "bash /dev/stdin <<< {{inputs.n}}",
            "bash /dev/fd/3 3<<< {{inputs.n}}",
            "bash /dev//stdin <<< {{inputs.n}}",
            "bash /[d]ev/stdin <<< {{inputs.n}}",
        ],
    );
    refused(
        "a value can't be the input of python3, which runs what it reads",
        &["python3 /dev/fd/3 3<<< {{inputs.n}}"],
    );
    refused(
        "a value can't be piped into bash, which runs what it reads",
        &[
            "echo {{inputs.n}} | bash /dev/stdin",
            "echo {{inputs.n}} | bash /dev//stdin",
            "echo {{inputs.n}} | bash /../dev/stdin",
            "echo {{inputs.n}} | bash ../../../../dev/stdin",
            // The pipe goes on past a newline or a comment.
            "echo {{inputs.n}} |\nbash",
            "echo {{inputs.n}} | # run it\nbash",
        ],
    );
    refused(
        "a value can't be piped into xargs, which runs what it reads",
        &[
            "echo {{inputs.n}} |\nxargs -I% sh -c %",
            "{ echo {{inputs.n}}; } | xargs -I% sh -c %",
            "( echo {{inputs.n}} ) | xargs -I% sh -c %",
            "if true; then echo {{inputs.n}}; fi | xargs sh -c",
            "for i in 1; do echo {{inputs.n}}; done | xargs sh -c",
            "case a in a) echo {{inputs.n}};; esac | xargs sh -c",
        ],
    );
    // With a quoted end word, bash joins no lines: the body ends at the
    // first EOF, and the quotes after it hold the value.
    refused(
        "a value in a command is quoted for you; take the quotes off around it",
        &[
            "cat <<'EOF'\nfoo \\\nEOF\necho \"\nEOF\necho {{inputs.n}}\"",
            "cat <<\\EOF\nC:\\temp\\\nEOF\necho \"\nEOF\necho {{inputs.n}}\"",
        ],
    );
    // A line ending in \ joins the next, so that EOF doesn't end the body.
    refused(
        "a value can't go in a heredoc; pass it as an argument",
        &["cat <<EOF\nfoo \\\nEOF\necho {{inputs.n}}\nEOF"],
    );
    refused(
        "a value can't go where the shell does arithmetic ([[ ]], (( )), let); compare it with [ ] instead",
        &[
            r#"[[ "]]" == x || 1 -lt {{inputs.n}} ]]"#,
            "[[ -v {{inputs.n}} ]]",
        ],
    );
    refused(
        "a value can't be a variable's name, where bash reads an array index as code; name the variable in the step",
        &[
            "printf -v {{inputs.n}} %s hi",
            "printf -v{{inputs.n}} %s hi",
            "sleep 0 & wait -p{{inputs.n}} -n",
            // Bash reads the flag after quotes, and in a cluster.
            "printf '-v'{{inputs.n}} %s hi",
            r"printf \-v{{inputs.n}} %s hi",
            "sleep 0 & wait -np{{inputs.n}}",
            "read -r {{inputs.n}} <<< hi",
            "[ -v {{inputs.n}} ]",
            "test -v {{inputs.n}}",
            "declare {{inputs.n}}",
            "mapfile -t {{inputs.n}} < list.txt",
        ],
    );
    refused(
        "a value can't go where the shell does arithmetic ([[ ]], (( )), let); compare it with [ ] instead",
        &["declare -n ref={{inputs.n}}", "local -n ref={{inputs.n}}"],
    );
    // Their safe neighbours still save.
    for ok in [
        "[[ -d {{inputs.n}} ]] && du -sh {{inputs.n}}",
        "[[ {{inputs.n}} == prod ]] && echo deploy",
        "cd {{inputs.n}} && git ls-files | xargs wc -l",
        "echo ${#HOME} {{inputs.n}}",
        "diff <(cat {{inputs.n}}) <(cat {{inputs.n}})",
        r#"echo "Today $(date +%F)" {{inputs.n}}"#,
        "declare FOO={{inputs.n}}",
        r#"git log --format="%h %s" -n 5 -- {{inputs.n}}"#,
        "echo `date` {{inputs.n}}",
        "echo $(( 1 << 2 )) {{inputs.n}}",
        "echo $(( (1 + 2) * 3 )) {{inputs.n}}",
        "(( 1 + 1 )) && echo {{inputs.n}}",
    ] {
        assert_eq!(problem(ok), Vec::<String>::new(), "{ok}");
    }
    // A value that pipes into an interpreter through a file is still refused.
    assert_eq!(
        problem("echo {{inputs.n}} > s.sh; cat s.sh | sh"),
        ["step \"a\": a value can't be piped into sh, which runs what it reads".to_string()]
    );
}

#[test]
fn a_dash_dash_counts_only_for_the_program_that_reads_the_value() {
    let places = |c: &str| {
        places(c, Quote::Posix)
            .into_iter()
            .map(|p| p.option)
            .collect::<Vec<_>>()
    };
    // The value can still be read as an option: checked.
    assert_eq!(
        places(r#"bash -c 'git fetch origin "$1"' -- {{inputs.n}}"#),
        [true]
    );
    assert_eq!(
        places("kubectl exec app -- git fetch origin {{inputs.n}}"),
        [true]
    );
    assert_eq!(places("cargo run -- {{inputs.n}}"), [true]);
    // After an inline script, `--` is the script's: it hands the value on.
    assert_eq!(
        places(r#"unshare -- sh -c 'git fetch origin "$1"' -- {{inputs.n}}"#),
        [true]
    );
    assert_eq!(
        places(r#"perl -e 'exec "git","fetch","origin",$ARGV[0]' -- {{inputs.n}}"#),
        [true]
    );
    assert_eq!(
        places("python3 -c 'import os,sys; os.execvp(\"git\", sys.argv)' -- {{inputs.n}}"),
        [true]
    );
    // A script fed on input or named as a file gets the value as `$1` too.
    assert_eq!(
        places("bash -s -- {{inputs.n}} <<'EOF'\ngit fetch -q \"$1\" origin\nEOF"),
        [true]
    );
    assert_eq!(
        places(r#"bash -s -- {{inputs.n}} <<< 'git fetch -q "$1" origin'"#),
        [true]
    );
    assert_eq!(places("python3 deploy.py -- {{inputs.n}}"), [true]);
    // find's `--` comes before its paths, not its -exec command.
    assert_eq!(
        places(r"find -- . -maxdepth 0 -exec git fetch origin {{inputs.n}} \;"),
        [true]
    );
    assert_eq!(places("git fetch {main,{{inputs.n}}}"), [true]);
    // The program that gets it stops reading options at `--`.
    assert_eq!(places("git fetch origin -- {{inputs.n}}"), [false]);
    assert_eq!(places("cargo run -- -- {{inputs.n}}"), [false]);
    assert_eq!(places("git commit --message={{inputs.n}}"), [false]);
}

#[test]
fn a_powershell_command_is_read_with_powershell_quotes_and_escapes() {
    let problem = |c: &str| command::problem(c, Quote::PowerShell);
    let quoted = "a value in a command is quoted for you; take the quotes off around it";
    for c in [
        // `\` is no escape in PowerShell: the path's quotes hold the value.
        r#"Get-ChildItem "C:\Users\user\{{inputs.x}}""#,
        "Write-Output \"Result: `\"{{inputs.x}}`\"\"",
        "Write-Output \u{201c}Result: {{inputs.x}}\u{201d}",
        "Write-Output \u{2018}{{inputs.x}}\u{2019}",
        "Write-Output @'\n{{inputs.x}}\n'@",
    ] {
        assert_eq!(problem(c).as_deref(), Some(quoted), "{c}");
    }
    for (c, why) in [
        (
            "Write-Output `{{inputs.x}}",
            "a value can't come right after \"`\"; put a space before it",
        ),
        (
            "git log --% {{inputs.x}}",
            "a value can't come after --%, where PowerShell stops quoting",
        ),
        (
            "{{inputs.x}} | iex",
            "a value can't be piped into iex, which runs what it reads",
        ),
        (
            "& {{inputs.x}}",
            "a value can't be the command itself; name the program in the step",
        ),
        (
            "Start-Process {{inputs.x}}",
            "a value can't be the command itself; name the program in the step",
        ),
        (
            "Write-Output 1 # {{inputs.x}}",
            "a value can't go in a comment",
        ),
    ] {
        assert_eq!(problem(c).as_deref(), Some(why), "{c}");
    }
    for ok in [
        r#"Copy-Item "C:\src\" {{inputs.x}}"#,
        "git fetch origin {{inputs.x}}",
        "Start-Process notepad -ArgumentList {{inputs.x}}",
        "pwsh -File {{inputs.x}}",
        "Get-Process powershell {{inputs.x}}",
        "Write-Output 'it''s' {{inputs.x}}",
    ] {
        assert_eq!(problem(ok), None, "{ok}");
    }
    // Curly quotes inside $(...): no value goes in that command.
    assert_eq!(
        problem("Write-Output \"$(Write-Output \u{2018}) \" {{inputs.x}} \u{2019})\"").as_deref(),
        Some("a value can't go in a command with a part nested in quotes or substitutions that this check can't follow; make that part plain (\"$(date +%F)\", not \"$(date +\"%F\")\"), or move it into a script step")
    );
}

#[test]
fn on_windows_a_program_gets_only_values_its_command_line_keeps_whole() {
    let values = Values {
        inputs: json!({
            "env": "prod&calc",
            "msg": "Build \"main\" passed",
            "dir": "C:\\Program Files\\MyApp\\"
        })
        .as_object()
        .unwrap()
        .clone(),
        ..Values::default()
    };
    let f = |t: &str| fill(t, &values, Quote::PowerShell);
    let metachar = Err(FillError::Unsafe(
        "inputs.env has one of & | < > ^, which cmd.exe can run as a command on Windows".into(),
    ));
    // A .cmd or .bat program (mvn is mvn.cmd) runs through cmd.exe, also
    // one named like a cmdlet, and Start-Process builds a command line.
    assert_eq!(f(".\\deploy.cmd {{inputs.env}}"), metachar);
    assert_eq!(f("mvn -P {{inputs.env}} package"), metachar);
    assert_eq!(f(".\\Deploy-App.cmd {{inputs.env}}"), metachar);
    assert_eq!(f("C:\\tools\\Invoke-Build.bat {{inputs.env}}"), metachar);
    assert_eq!(
        f("Start-Process .\\deploy.cmd -ArgumentList {{inputs.env}}"),
        metachar
    );
    // A program named with .exe is no batch file.
    assert_eq!(
        f("curl.exe -L {{inputs.env}}"),
        Ok("curl.exe -L 'prod&calc'".into())
    );
    assert_eq!(
        f("git commit -m {{inputs.msg}}"),
        Err(FillError::Unsafe(
            "inputs.msg has a double quote, which Windows can't pass to a program safely".into()
        ))
    );
    // A cmdlet gets the string as it is.
    assert_eq!(
        f("Add-Content -Path log.txt -Value {{inputs.msg}}"),
        Ok("Add-Content -Path log.txt -Value 'Build \"main\" passed'".into())
    );
    assert_eq!(
        f("Copy-Item build {{inputs.dir}} -Recurse"),
        Ok("Copy-Item build 'C:\\Program Files\\MyApp\\' -Recurse".into())
    );
    assert_eq!(
        f("Write-Output {{inputs.env}}"),
        Ok("Write-Output 'prod&calc'".into())
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
        Err(FillError::Missing(
            "probe's JSON has no disks.1.free".into()
        ))
    );
    assert_eq!(
        f("{{use.exit_code}}"),
        Err(FillError::Missing("use has no exit code".into()))
    );
    assert_eq!(
        f("{{gone.stdout}}"),
        Err(FillError::Missing("gone hasn't run".into()))
    );
    assert_eq!(
        f("{{probe.json"),
        Err(FillError::Missing(
            "\"{{probe.json\" opens {{ without closing it".into()
        ))
    );
    assert_eq!(
        f("{{probe.size}}"),
        Err(FillError::Missing(
            "\"probe.size\" isn't something a routine can read".into()
        ))
    );
    assert_eq!(
        f("{{use.json}}"),
        Err(FillError::Missing("use didn't print JSON".into()))
    );
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

#[test]
fn an_expect_passes_misses_or_cant_read_its_output() {
    let mut values = Values::default();
    values.steps.insert(
        "df".into(),
        StepOut {
            stdout: "Filesystem 1024-blocks Used Available Capacity Mounted on\n/dev/vda 263174212 236856791 26317421 91% /\n".into(),
            exit_code: Some(0),
            ..StepOut::default()
        },
    );
    let expect = |v: Value| serde_json::from_value::<Expect>(v).unwrap().check(&values);
    let full = json!({ "from": "df.stdout", "regex": "(\\d+)%", "group": 1, "at_least": 90 });
    assert_eq!(expect(full), Ok(Checked::Pass("91".into())));
    let roomy = json!({ "from": "df.stdout", "regex": "(\\d+)%", "group": 1, "at_least": 95 });
    assert_eq!(expect(roomy), Ok(Checked::Miss("91".into())));
    assert_eq!(
        expect(json!({ "from": "df.exit_code", "equals": 0 })),
        Ok(Checked::Pass("0".into()))
    );
    assert_eq!(
        expect(
            json!({ "from": "df.stdout", "regex": "Mounted on (\\w+)", "group": 1, "equals": "on" })
        ),
        Err("df.stdout has nothing matching \"Mounted on (\\w+)\"".into())
    );
    assert_eq!(
        expect(json!({ "from": "df.stdout", "regex": "/dev/(\\w+)", "group": 1, "at_most": 3 })),
        Err("df.stdout gave \"vda\", which isn't a number".into())
    );
    assert_eq!(
        expect(json!({ "from": "df.stderr" })),
        Ok(Checked::Miss(String::new()))
    );
    assert_eq!(
        [
            parse_ref("inputs.mount").unwrap().to_string(),
            parse_ref("df.exit_code").unwrap().to_string(),
            parse_ref("p.json").unwrap().to_string(),
            parse_ref("p.json.a.0").unwrap().to_string(),
        ],
        ["inputs.mount", "df.exit_code", "p.json", "p.json.a.0"]
    );
}

#[test]
fn a_script_step_runs_a_python_file() {
    let r = routine(json!([{ "id": "a", "script": "check.sh" }]));
    assert_eq!(
        r.problems(&tools),
        ["step \"a\": script \"check.sh\" must be a Python file (.py)"]
    );
}
