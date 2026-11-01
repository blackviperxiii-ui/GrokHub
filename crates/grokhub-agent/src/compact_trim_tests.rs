//! History trim (LO-47): repeated read-only outputs dropped above 40% of the
//! window, and the optional `age_out` spool.

use super::*;

const NOTES_ARGS: &str = r#"{"path":"notes.txt"}"#;
const NOTE_C2: &str =
    "[Output dropped: read_file ran again later with the same arguments (call c2). Use that result.]";

fn msg(role: &str, text: &str) -> InputItem {
    InputItem::Message {
        role: role.into(),
        content: vec![ContentPart::InputText(text.into())],
    }
}

fn call(id: &str, name: &str, arguments: &str) -> InputItem {
    InputItem::FunctionCall {
        call_id: id.into(),
        name: name.into(),
        arguments: arguments.into(),
    }
}

fn out(id: &str, text: &str) -> InputItem {
    InputItem::FunctionCallOutput {
        call_id: id.into(),
        output: text.into(),
    }
}

fn output_of<'a>(items: &'a [InputItem], id: &str) -> &'a str {
    items
        .iter()
        .find_map(|item| match item {
            InputItem::FunctionCallOutput { call_id, output } if call_id == id => {
                Some(output.as_str())
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("no output for {id}"))
}

fn messages(items: &[InputItem]) -> Vec<InputItem> {
    items
        .iter()
        .filter(|item| matches!(item, InputItem::Message { .. }))
        .cloned()
        .collect()
}

/// Two calls of `name` with the given arguments, each answered.
fn two_calls(name: &str, first_args: &str, second_args: &str) -> Vec<InputItem> {
    vec![
        msg("system", "RULES"),
        msg("user", "read my notes"),
        call("c1", name, first_args),
        out("c1", "1|save folder is D:/saves\n2|old line"),
        msg("assistant", "Read them."),
        msg("user", "read them again please"),
        call("c2", name, second_args),
        out("c2", "1|save folder is D:/saves\n2|new line"),
    ]
}

/// Every answered call in `items` counted as succeeded.
fn all_succeeded(items: &[InputItem]) -> std::collections::HashSet<String> {
    items
        .iter()
        .filter_map(|item| match item {
            InputItem::FunctionCallOutput { call_id, .. } => Some(call_id.clone()),
            _ => None,
        })
        .collect()
}

fn trim(items: &mut [InputItem], ctx: u64) -> usize {
    let ok = all_succeeded(items);
    trim_history(items, ctx, &ok)
}

/// A window the history fills to 50%, above the trim threshold.
fn half_full(items: &[InputItem]) -> u64 {
    estimate_input_tokens(items) * 2
}

#[test]
fn a_repeated_read_above_40_percent_drops_only_the_older_output() {
    let mut history = two_calls("read_file", NOTES_ARGS, NOTES_ARGS);
    let before = history.clone();
    let ctx = half_full(&history);
    assert_eq!(trim(&mut history, ctx), 1);
    assert_eq!(output_of(&history, "c1"), NOTE_C2);
    assert_eq!(
        output_of(&history, "c2"),
        "1|save folder is D:/saves\n2|new line"
    );
    assert_eq!(messages(&history), messages(&before));
    assert_eq!(history.len(), before.len());
    // Calls keep their pairs: every item but the older output is unchanged.
    for (i, (a, b)) in before.iter().zip(&history).enumerate() {
        if i != 3 {
            assert_eq!(a, b, "item {i} changed");
        }
    }
    // A second pass finds nothing new.
    assert_eq!(trim(&mut history, ctx), 0);
    assert_eq!(output_of(&history, "c1"), NOTE_C2);
}

#[test]
fn different_paths_other_tools_and_a_roomy_window_trim_nothing() {
    let mut paths = two_calls("read_file", NOTES_ARGS, r#"{"path":"todo.txt"}"#);
    let ctx = half_full(&paths);
    let before = paths.clone();
    assert_eq!(trim(&mut paths, ctx), 0);
    assert_eq!(paths, before);

    let shell_args = r#"{"command":"cat notes.txt"}"#;
    let mut shell = two_calls("run_terminal_command", shell_args, shell_args);
    let ctx = half_full(&shell);
    let before = shell.clone();
    assert_eq!(trim(&mut shell, ctx), 0);
    assert_eq!(shell, before);

    let mut roomy = two_calls("read_file", NOTES_ARGS, NOTES_ARGS);
    let before = roomy.clone();
    let under = estimate_input_tokens(&roomy) * 3;
    assert_eq!(trim(&mut roomy, under), 0);
    assert_eq!(trim(&mut roomy, 0), 0);
    assert_eq!(roomy, before);
    assert_eq!(HISTORY_TRIM_PERCENT, 40);
}

#[test]
fn every_older_repeat_points_at_the_newest_and_key_order_does_not_matter() {
    let mut history = two_calls(
        "grep",
        r#"{"pattern":"TODO","path":"src"}"#,
        r#"{"path":"src","pattern":"TODO"}"#,
    );
    history.push(call(
        "c3",
        "grep",
        r#"{ "pattern": "TODO", "path": "src" }"#,
    ));
    history.push(out("c3", "src/main.rs:3: TODO"));
    let ctx = half_full(&history);
    assert_eq!(trim(&mut history, ctx), 2);
    let note =
        "[Output dropped: grep ran again later with the same arguments (call c3). Use that result.]";
    assert_eq!(output_of(&history, "c1"), note);
    assert_eq!(output_of(&history, "c2"), note);
    assert_eq!(output_of(&history, "c3"), "src/main.rs:3: TODO");
}

#[test]
fn a_later_call_with_no_output_yet_keeps_the_older_result() {
    let mut history = two_calls("list_dir", r#"{"path":""}"#, r#"{"path":""}"#);
    history.retain(
        |item| !matches!(item, InputItem::FunctionCallOutput { call_id, .. } if call_id == "c2"),
    );
    let ctx = half_full(&history);
    let before = history.clone();
    assert_eq!(trim(&mut history, ctx), 0);
    assert_eq!(history, before);
}

#[test]
fn history_trim_settings_default_on_with_age_out_off_and_parse_the_file() {
    assert_eq!(
        HistoryTrimSettings::default(),
        HistoryTrimSettings {
            enabled: true,
            age_out: false,
            age_out_min_bytes: 12_000,
            age_out_user_turns: 3,
        }
    );
    assert_eq!(
        parse_history_trim_file(
            "# trim\nenabled = false\nage_out = true # trades quality\n\
             age_out_min_bytes = 500\nage_out_user_turns = 2\nbogus = 1\n"
        ),
        HistoryTrimSettings {
            enabled: false,
            age_out: true,
            age_out_min_bytes: 500,
            age_out_user_turns: 2,
        }
    );
    assert_eq!(
        parse_history_trim_file("age_out = maybe\nage_out_min_bytes = lots\n"),
        HistoryTrimSettings::default()
    );
    let missing = std::env::temp_dir().join(format!("gh-trim-none-{}", std::process::id()));
    assert_eq!(
        history_trim_settings(&missing),
        HistoryTrimSettings::default()
    );
}

/// A big shell output followed by `users_after` user turns.
fn old_big_output(users_after: usize) -> Vec<InputItem> {
    let mut items = vec![
        msg("system", "RULES"),
        msg("user", "build it"),
        call("c1", "run_terminal_command", r#"{"command":"make"}"#),
        out("c1", &format!("error: link failed\n{}", "x".repeat(12_981))),
    ];
    for i in 0..users_after {
        items.push(msg("assistant", "ok"));
        items.push(msg("user", &format!("next {i}")));
    }
    items
}

#[test]
fn age_out_is_off_by_default_and_spools_old_big_outputs_when_on() {
    let ws = std::env::temp_dir().join(format!("gh-trim-age-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&ws);
    let big = format!("error: link failed\n{}", "x".repeat(12_981));
    assert_eq!(big.len(), 13_000);

    let mut history = old_big_output(3);
    let ctx = half_full(&history);
    let before = history.clone();
    let off = HistoryTrimSettings::default();
    assert_eq!(age_out_history(&mut history, ctx, &off, &ws, "sess"), 0);
    assert_eq!(history, before);

    let on = HistoryTrimSettings {
        age_out: true,
        ..HistoryTrimSettings::default()
    };
    // Two user turns after it: still too fresh.
    let mut fresh = old_big_output(2);
    let fresh_ctx = half_full(&fresh);
    assert_eq!(age_out_history(&mut fresh, fresh_ctx, &on, &ws, "sess"), 0);

    assert_eq!(age_out_history(&mut history, ctx, &on, &ws, "sess"), 1);
    let body = output_of(&history, "c1");
    assert!(
        body.starts_with(
            "<tool_output_compact tool=\"run_terminal_command\" handle=\"to-c1\" bytes=\"13000\">"
        ),
        "{body}"
    );
    assert!(body.contains("- error: link failed"), "{body}");
    assert!(
        body.contains("`.grokhub/tool-output-compact/sess/to-c1.txt`"),
        "{body}"
    );
    assert_eq!(
        read_spooled_tool_output(&ws, "sess", "to-c1").as_deref(),
        Some(big.as_str())
    );
    assert_eq!(messages(&history), messages(&before));
    // Already spooled: a second pass leaves it.
    assert_eq!(age_out_history(&mut history, ctx, &on, &ws, "sess"), 0);
    let _ = std::fs::remove_dir_all(&ws);
}

#[test]
fn trim_pass_follows_the_config_file_and_only_trusts_noted_results() {
    let dir = std::env::temp_dir().join(format!("gh-trim-pass-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let conv = format!("trim-pass-{}", std::process::id());
    let mut history = two_calls("read_file", NOTES_ARGS, NOTES_ARGS);
    let ctx = half_full(&history);
    let before = history.clone();
    // Nothing noted for this conversation yet (a resumed chat): nothing drops.
    assert_eq!(trim_pass(&mut history, ctx, &dir, &dir, &conv), 0);
    assert_eq!(history, before);
    note_tool_result(&conv, "c1", "read_file", false);
    note_tool_result(&conv, "c2", "read_file", false);
    note_tool_result(&conv, "s1", "run_terminal_command", false);
    let noted: std::collections::HashSet<String> = ["c1".into(), "c2".into()].into();
    assert_eq!(trim_results(&conv), noted);
    std::fs::write(dir.join(HISTORY_TRIM_CONFIG), "enabled = false\n").unwrap();
    assert_eq!(trim_pass(&mut history, ctx, &dir, &dir, &conv), 0);
    assert_eq!(history, before);
    std::fs::remove_file(dir.join(HISTORY_TRIM_CONFIG)).unwrap();
    assert_eq!(trim_pass(&mut history, ctx, &dir, &dir, &conv), 1);
    assert_eq!(output_of(&history, "c1"), NOTE_C2);
    let _ = std::fs::remove_dir_all(&dir);
}

/// One real read_file call through the tool dispatch, recorded as the run loop does.
fn real_read(ws: &Path, conv: &str, id: &str) -> (InputItem, InputItem, bool) {
    let args = r#"{"target_file":"notes.txt"}"#;
    let stop = || false;
    let ctx = crate::tools::ToolCtx {
        workspace: ws,
        desktop: None,
        stop: &stop,
        tasks: None,
        owner: None,
        session: None,
    };
    let result = crate::tools::dispatch(&ctx, "read_file", args);
    note_tool_result(conv, id, "read_file", result.failed);
    (
        call(id, "read_file", args),
        out(id, &result.text),
        result.failed,
    )
}

#[test]
fn a_later_read_that_errored_never_drops_the_good_result() {
    let ws = std::env::temp_dir().join(format!("gh-trim-err-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&ws);
    std::fs::create_dir_all(&ws).unwrap();
    let conv = format!("trim-err-{}", std::process::id());
    std::fs::write(ws.join("notes.txt"), "save folder is D:/saves\n").unwrap();
    let mut history = vec![msg("system", "RULES"), msg("user", "read my notes")];
    let (c1, o1, failed1) = real_read(&ws, &conv, "c1");
    assert!(!failed1);
    assert_eq!(o1, out("c1", "1|save folder is D:/saves\n"));
    history.extend([c1, o1]);
    std::fs::remove_file(ws.join("notes.txt")).unwrap();
    history.push(msg("user", "read them again"));
    let (c2, o2, failed2) = real_read(&ws, &conv, "c2");
    assert!(failed2);
    let InputItem::FunctionCallOutput { output: err, .. } = &o2 else {
        panic!("not an output: {o2:?}");
    };
    assert!(err.starts_with("read "), "{err}");
    history.extend([c2, o2.clone()]);
    let ctx = half_full(&history);
    let before = history.clone();
    assert_eq!(trim_pass(&mut history, ctx, &ws, &ws, &conv), 0);
    assert_eq!(output_of(&history, "c1"), "1|save folder is D:/saves\n");
    assert_eq!(history, before);
    // Once the file is back and read again, both older outputs point at it.
    std::fs::write(ws.join("notes.txt"), "save folder is D:/saves\n").unwrap();
    let (c3, o3, failed3) = real_read(&ws, &conv, "c3");
    assert!(!failed3);
    history.extend([c3, o3]);
    let ctx = half_full(&history);
    assert_eq!(trim_pass(&mut history, ctx, &ws, &ws, &conv), 2);
    assert_eq!(
        output_of(&history, "c1"),
        "[Output dropped: read_file ran again later with the same arguments (call c3). Use that result.]"
    );
    assert_eq!(output_of(&history, "c3"), "1|save folder is D:/saves\n");
    let _ = std::fs::remove_dir_all(&ws);
}

#[test]
fn a_refused_repeat_is_not_a_result_to_point_at() {
    let mut history = two_calls("read_file", NOTES_ARGS, NOTES_ARGS);
    history.push(call("c3", "read_file", NOTES_ARGS));
    history.push(out("c3", "not run: the same call already ran twice."));
    let ctx = half_full(&history);
    // The repeat guard's refusal is a failed ToolOutput, so c3 is never noted.
    let ok: std::collections::HashSet<String> = ["c1".into(), "c2".into()].into();
    assert_eq!(trim_history(&mut history, ctx, &ok), 1);
    assert_eq!(output_of(&history, "c1"), NOTE_C2);
    assert_eq!(
        output_of(&history, "c2"),
        "1|save folder is D:/saves\n2|new line"
    );
    assert_eq!(
        output_of(&history, "c3"),
        "not run: the same call already ran twice."
    );
}
