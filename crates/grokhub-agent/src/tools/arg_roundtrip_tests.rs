//! Arguments reach the tool byte for byte: shell commands, file writes,
//! edits and hook stdin, with the characters shells and JSON like to eat.

use serde_json::json;

fn scratch(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "gh-rt-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Each payload starts and ends with a visible character, because tool
/// output is trimmed.
fn payloads() -> Vec<(&'static str, String)> {
    let mut big = String::new();
    while big.len() < 9_000 {
        big.push_str("line \\ \"q\" 'q' $x `y` é 🎮;\n");
    }
    big.push_str("end");
    vec![
        (
            "backslashes",
            r"C:\Users\x\new\table \\srv\share \n \t \\ end\".to_string(),
        ),
        (
            "quotes",
            r#""double" 'single' \"escaped\" '\'' end"#.to_string(),
        ),
        (
            "dollars",
            "$HOME ${PATH} $(whoami) `ls` %PATH% !! & | ; < > end".to_string(),
        ),
        (
            "unicode",
            "héllo wörld 日本語 🎮🔥 zero\u{200b}width ñ end".to_string(),
        ),
        ("lines", "first\n\n  indented\ttab\nlast".to_string()),
        // Edits keep a file's own line endings, so this one is all CRLF.
        ("crlf", "first\r\n\r\n  indented\ttab\r\nlast".to_string()),
        ("big", big),
    ]
}

fn unix_newlines(text: &str) -> String {
    text.replace("\r\n", "\n")
}

#[test]
fn write_and_edit_keep_every_byte() {
    let dir = scratch("write");
    for (name, payload) in payloads() {
        let file = format!("{name}.txt");
        let out = super::write::run(&dir, &json!({"path": file, "content": payload}));
        assert!(!out.failed, "{name}: {}", out.text);
        let back = std::fs::read_to_string(dir.join(&file)).unwrap();
        assert_eq!(back, payload, "{name}: write changed the bytes");

        let swapped = format!("[{payload}]");
        let out = super::search_replace::run(
            &dir,
            &json!({"file_path": file, "old_string": payload, "new_string": swapped}),
        );
        assert!(!out.failed, "{name}: {}", out.text);
        let back = std::fs::read_to_string(dir.join(&file)).unwrap();
        assert_eq!(back, swapped, "{name}: the edit changed the bytes");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[cfg(unix)]
#[test]
fn a_shell_command_carries_its_text_exactly() {
    let dir = scratch("sh");
    for (name, payload) in payloads() {
        let payload = payload.replace('\r', "");
        let command = format!("cat <<'GH_EOF'\n{payload}\nGH_EOF");
        let out = super::shell::run(&dir, &json!({"command": command}), &|| false);
        assert!(!out.failed, "{name}: {}", out.text);
        assert_eq!(out.text, format!("exit 0\n{payload}"), "{name}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[cfg(unix)]
#[test]
fn hook_stdin_arrives_byte_for_byte() {
    let dir = scratch("hook");
    for (name, payload) in payloads() {
        let proc = super::shell::run_hook_command(
            &dir,
            "cat",
            payload.as_bytes(),
            &[],
            std::time::Duration::from_secs(10),
            &dir,
        )
        .unwrap();
        assert_eq!(proc.exit_code, Some(0), "{name}: {}", proc.stderr);
        assert_eq!(proc.stdout, payload, "{name}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[cfg(unix)]
#[test]
fn a_command_too_long_for_one_argument_is_turned_back_with_a_way_round() {
    let dir = scratch("long");
    let command = format!("echo {}", "x".repeat(130 * 1024));
    let out = super::shell::run(&dir, &json!({"command": command}), &|| false);
    assert!(out.failed, "{}", out.text);
    assert!(out.text.contains("128 KB"), "{}", out.text);
    assert!(out.text.contains("write tool"), "{}", out.text);
    let _ = std::fs::remove_dir_all(&dir);
}

/// Over 13K characters overflows the 32,767-character command line once
/// encoded, so this runs through the script-file fallback, and the emoji
/// only survive if PowerShell writes UTF-8.
#[cfg(windows)]
#[test]
fn a_long_powershell_command_with_emoji_round_trips() {
    let dir = scratch("ps");
    let mut payload = String::new();
    while payload.chars().count() < 14_000 {
        payload.push_str("héllo 🎮 \"q\" $x `y` \\ end;\n");
    }
    payload.push_str("done");
    let command = format!("@'\n{payload}\n'@");
    let out = super::shell::run(&dir, &json!({"command": command}), &|| false);
    assert!(!out.failed, "{}", out.text);
    assert_eq!(unix_newlines(&out.text), format!("exit 0\n{payload}"));

    let short = "héllo wörld 日本語 🎮🔥 end";
    let out = super::shell::run(
        &dir,
        &json!({"command": format!("Write-Output '{short}'")}),
        &|| false,
    );
    assert!(!out.failed, "{}", out.text);
    assert_eq!(unix_newlines(&out.text), format!("exit 0\n{short}"));
    let _ = std::fs::remove_dir_all(&dir);
}

/// The UTF-8 prelude sits on its own line: a parse error in the command
/// still shows the command's whole line, and loading a module on first use
/// writes no progress noise into the output.
#[cfg(windows)]
#[test]
fn a_powershell_error_shows_the_whole_command_and_no_progress_noise() {
    let dir = scratch("ps-err");
    let out = super::shell::run(
        &dir,
        &json!({"command": "echo first-part && echo second-part"}),
        &|| false,
    );
    assert!(
        out.text.contains("echo first-part && echo second-part"),
        "{}",
        out.text
    );
    assert!(!out.text.contains("GrokHubUtf8"), "{}", out.text);
    let out = super::shell::run(
        &dir,
        &json!({"command": "Get-ChildItem -LiteralPath . | Out-Null; Write-Output listed"}),
        &|| false,
    );
    assert!(!out.failed, "{}", out.text);
    assert!(!out.text.contains("CLIXML"), "{}", out.text);
    assert_eq!(unix_newlines(&out.text), "exit 0\nlisted");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn newline_helper_only_touches_crlf() {
    assert_eq!(unix_newlines("a\r\nb\rc\n"), "a\nb\rc\n");
}
