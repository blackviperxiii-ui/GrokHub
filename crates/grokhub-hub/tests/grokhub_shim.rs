//! The `grokhub` shim end to end: it runs the target with the same
//! arguments, returns its exit code, keeps stdout clean and prints one note
//! on stderr.

#[cfg(unix)]
#[test]
fn the_shim_forwards_args_and_exit_code_with_one_note() {
    use std::os::unix::fs::PermissionsExt;
    let dir = std::env::temp_dir().join(format!("grokhub-shim-e2e-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let target = dir.join("fake-hub");
    std::fs::write(&target, "#!/bin/sh\necho \"args: $*\"\nexit 3\n").unwrap();
    std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o755)).unwrap();
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_grokhub"))
        .args(["--mcp-self", "--verbose"])
        .env("GROKHUB_SHIM_TARGET", &target)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(3));
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "args: --mcp-self --verbose\n"
    );
    assert_eq!(
        String::from_utf8_lossy(&out.stderr),
        "grokhub was renamed to grokhub-hub; this grokhub shim forwards for at least one more release, so point your config at grokhub-hub.\n"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_shim_refuses_the_old_window_flags() {
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_grokhub"))
        .arg("--agent")
        .env("GROKHUB_SHIM_TARGET", "not-run")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(out.stdout.is_empty());
    assert_eq!(
        String::from_utf8_lossy(&out.stderr),
        "grokhub was renamed: the desktop app is grokhub-shell, and --mcp-* and --doctor are in grokhub-hub.\n"
    );
}
