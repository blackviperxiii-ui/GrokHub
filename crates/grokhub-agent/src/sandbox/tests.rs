use std::path::{Path, PathBuf};

use serde_json::json;

use super::*;

fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "gh-sbx-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir.canonicalize().unwrap()
}

#[test]
fn settings_default_on_and_the_file_can_widen_or_turn_it_off() {
    assert_eq!(parse_file("", None), Settings::default());
    assert!(Settings::default().enabled && Settings::default().network_guard);
    let home = Path::new("/home/user");
    // An absolute path on this OS ("/opt/sdk" has no drive on Windows).
    let sdk = std::env::temp_dir().join("sdk");
    let s = parse_file(
        &format!(
            "# mine\nenabled = true\nnetwork_guard = false\nwritable = [\"~/src/other\", '{}', \"relative/no\"]\n",
            sdk.display()
        ),
        Some(home),
    );
    assert!(s.enabled);
    assert!(!s.network_guard);
    assert_eq!(s.writable, [home.join("src/other"), sdk]);
    assert!(!parse_file("enabled = FALSE", None).enabled);
}

#[test]
fn privilege_raising_commands_are_spotted_wherever_they_start() {
    for cmd in [
        "sudo apt update",
        "/usr/bin/sudo -n true",
        "cd /x && sudo make install",
        "echo hi; pkexec ls",
        "FOO=1 doas reboot",
        "env -i sudo ls",
        "x=$(sudo cat /etc/shadow)",
        "su -c id",
        "run0 systemctl restart foo",
    ] {
        assert!(raises_privileges(cmd), "{cmd}");
    }
    for cmd in [
        "cargo build",
        "echo sudo",
        "grep -r sudo .",
        "git commit -m 'no sudo here'",
        "pseudo-thing --help",
    ] {
        assert!(!raises_privileges(cmd), "{cmd}");
    }
}

#[test]
fn only_privileges_or_a_hard_card_run_on_the_whole_machine() {
    let call = |args| runs_outside("run_terminal_command", &args);
    assert!(!call(json!({"command": "cargo test"})));
    assert!(call(json!({"command": "sudo apt upgrade -y"})));
    assert!(call(json!({"command": "shutdown -h now"})), "hard class");
    assert!(
        call(json!({"command": "cp a.txt /etc/thing", "outside_sandbox": true})),
        "the flag is a hard card"
    );
    assert!(!call(json!({"command": "ls", "outside_sandbox": false})));
    assert!(runs_outside(
        "monitor",
        &json!({"command": "sudo dmesg -w"})
    ));
}

#[test]
fn the_flag_is_classed_hard_so_the_gate_always_asks() {
    let hit = crate::harness::classify(
        "run_terminal_command",
        &json!({"command": "touch /opt/x", "outside_sandbox": true}).to_string(),
    );
    assert_eq!(
        hit,
        crate::harness::HardHit::Class(crate::harness::HardClass::IrreversibleOs)
    );
}

#[test]
fn writable_roots_keep_the_workspace_and_never_expose_the_config_folder() {
    let base = scratch("roots");
    let ws = base.join("ws");
    let home = base.join("home");
    let config = home.join(".cache").join("GrokHub");
    std::fs::create_dir_all(&ws).unwrap();
    std::fs::create_dir_all(&config).unwrap();
    std::fs::create_dir_all(home.join(".cargo")).unwrap();
    let extra = [home.clone(), base.join("missing")];
    let roots = writable_roots(&ws, Some(&home), &config, &extra);
    assert_eq!(roots[0], ws, "workspace first");
    assert!(roots.contains(&home.join(".cargo")));
    assert!(!roots.contains(&home.join(".cache")), "holds the config");
    assert!(!roots.contains(&home), "holds the config");
    assert!(!roots.iter().any(|r| r.ends_with("missing")));
    // A workspace that holds the config folder is still writable.
    let roots = writable_roots(&home, Some(&home), &config, &[]);
    assert_eq!(roots[0], home);
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn the_settings_file_turns_the_plan_off_and_a_sudo_call_gets_none() {
    let config = scratch("plan-cfg");
    let ws = scratch("plan-ws");
    let args = json!({"command": "ls"});
    let plan = plan_in(&config, "run_terminal_command", &args, &ws).expect("on by default");
    assert_eq!(plan.writable[0], ws);
    assert!(plan
        .proxy
        .as_deref()
        .is_some_and(|p| p.starts_with("127.0.0.1:")));
    assert!(plan_in(
        &config,
        "run_terminal_command",
        &json!({"command": "sudo ls"}),
        &ws
    )
    .is_none());
    std::fs::write(config.join(CONFIG_NAME), "network_guard = false\n").unwrap();
    let plan = plan_in(&config, "run_terminal_command", &args, &ws).unwrap();
    assert!(plan.proxy.is_none());
    std::fs::write(config.join(CONFIG_NAME), "enabled = false\n").unwrap();
    assert!(plan_in(&config, "run_terminal_command", &args, &ws).is_none());
    let _ = std::fs::remove_dir_all(&config);
    let _ = std::fs::remove_dir_all(&ws);
}

/// The real thing, through `run_terminal_command`: a write in the workspace
/// lands, a write outside it is refused by the kernel, a delete and a rename
/// out are refused too, reads stay open, and the proxy variables are set.
#[cfg(target_os = "linux")]
#[test]
fn a_sandboxed_command_writes_only_in_the_workspace() {
    if landlock::abi().is_none() {
        eprintln!("skipped: Landlock is not available on this kernel");
        return;
    }
    // The config folder lives under the temp folder here, so the temp folder
    // is not writable in this test and a sibling scratch folder is "outside".
    let config = scratch("live-cfg");
    let _pin = crate::perm::ConfigGuard::set(&config);
    let ws = scratch("live-ws");
    let outside = scratch("live-out");
    std::fs::write(outside.join("keep.txt"), "keep").unwrap();
    let run =
        |command: String| crate::tools::shell::run(&ws, &json!({ "command": command }), &|| false);

    let out = run("echo in > inside.txt && cat inside.txt && echo err >&2".into());
    assert!(!out.failed, "{}", out.text);
    assert!(
        out.text.contains("in") && out.text.contains("err"),
        "{}",
        out.text
    );

    let out = run(format!("echo out > {}/new.txt", outside.display()));
    assert!(out.failed, "{}", out.text);
    assert!(out.text.contains("Permission denied"), "{}", out.text);
    assert!(!outside.join("new.txt").exists());

    // `rm` is a hard card (it runs outside once approved), so delete another way.
    let delete = format!(
        "cat {0}/keep.txt; python3 -c 'import os; os.remove(\"{0}/keep.txt\")'",
        outside.display()
    );
    assert!(!runs_outside(
        "run_terminal_command",
        &json!({ "command": delete })
    ));
    let out = run(delete);
    assert!(out.text.contains("keep"), "reads stay open: {}", out.text);
    assert!(outside.join("keep.txt").exists(), "{}", out.text);

    let out = run(format!("mv inside.txt {}/moved.txt", outside.display()));
    assert!(out.failed, "{}", out.text);
    assert!(ws.join("inside.txt").exists());

    let out = run(format!("echo x > {}/policy.toml", config.display()));
    assert!(out.failed, "the config folder is read-only: {}", out.text);

    let out = run("echo \"$HTTPS_PROXY|$NO_PROXY\"; : > /dev/null".into());
    assert!(!out.failed, "{}", out.text);
    assert!(out.text.contains("http://127.0.0.1:"), "{}", out.text);
    assert!(out.text.contains(proxy::NO_PROXY), "{}", out.text);

    // A background task gets the same confinement.
    let hub = crate::tasks::TaskHub::new("sandbox-test");
    let plan = plan_for_call("run_terminal_command", &json!({"command": "x"}), &ws);
    let id = hub
        .spawn_for(
            &ws,
            &format!("echo bg > {}/bg.txt; echo bg > bg.txt", outside.display()),
            None,
            plan,
        )
        .unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !ws.join("bg.txt").exists() && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(
        ws.join("bg.txt").exists(),
        "task {id} wrote in the workspace"
    );
    assert!(!outside.join("bg.txt").exists());

    for dir in [&config, &ws, &outside] {
        let _ = std::fs::remove_dir_all(dir);
    }
}

/// Windows: the same check with the write-restricted token. The folders sit
/// under the home folder, not the temp folder: a grant on the temp folder is
/// kept in its ACL and would be inherited by a scratch folder made there.
#[cfg(windows)]
#[test]
fn a_sandboxed_command_writes_only_in_the_workspace_on_windows() {
    let home = grokhub_core::user_home().expect("home");
    let base = home.join(format!("gh-sbx-win-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let config = base.join("config");
    let ws = base.join("ws");
    let outside = base.join("out");
    for dir in [&config, &ws, &outside] {
        std::fs::create_dir_all(dir).unwrap();
    }
    let ws = ws.canonicalize().unwrap();
    let _pin = crate::perm::ConfigGuard::set(&config);
    // The sandbox falls back to no token when it can't be set up, so check
    // each step here: a failure names the step instead of a stray write.
    let plan = crate::sandbox::plan_for_call(
        "run_terminal_command",
        &json!({ "command": "Get-Location" }),
        &ws,
    )
    .expect("a sandbox plan");
    assert_eq!(plan.writable.first(), Some(&ws), "{:?}", plan.writable);
    assert!(
        !plan.writable.iter().any(|root| outside.starts_with(root)),
        "the outside folder sits under a writable root: {:?}",
        plan.writable
    );
    crate::sandbox::windows::grant(&ws).expect("grant on the workspace");
    if let Err(e) = crate::sandbox::windows::restricted_token() {
        panic!(
            "the restricted token: {e} ({})",
            crate::sandbox::windows::probe_restricting_sids()
        );
    }
    let acl = |path: &std::path::Path| {
        std::process::Command::new("powershell")
            .args(["-NoProfile", "-Command"])
            .arg(format!(
                "(Get-Acl -LiteralPath '{}').AccessToString; whoami /groups /fo list",
                path.display()
            ))
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
            .unwrap_or_default()
    };
    let run =
        |command: String| crate::tools::shell::run(&ws, &json!({ "command": command }), &|| false);

    let out = run("Set-Content -Path inside.txt -Value in; Get-Content inside.txt".into());
    assert!(!out.failed, "{}", out.text);
    assert!(out.text.contains("in"), "{}", out.text);

    let target = outside.join("new.txt");
    let out = run(format!(
        "Set-Content -Path '{}' -Value out -ErrorAction Stop",
        target.display()
    ));
    assert!(
        out.failed && !target.exists(),
        "a write outside the workspace went through: {}\n{}",
        out.text,
        acl(&outside)
    );

    let out = run(format!(
        "Set-Content -Path '{}' -Value x -ErrorAction Stop",
        config.join("policy.toml").display()
    ));
    assert!(out.failed, "the config folder is read-only: {}", out.text);

    let out = run("Write-Output $env:HTTPS_PROXY; Write-Output x | Out-Null".into());
    assert!(!out.failed, "{}", out.text);
    assert!(out.text.contains("http://127.0.0.1:"), "{}", out.text);

    let _ = std::fs::remove_dir_all(&base);
}
