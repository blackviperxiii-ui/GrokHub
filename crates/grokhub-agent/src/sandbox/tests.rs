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
    assert!(Settings::default().auto_allow);
    assert!(!parse_file("auto_allow = false", None).auto_allow);
}

#[test]
fn only_a_plan_that_holds_writes_and_the_network_counts_as_enforced() {
    let held = |net_confined| Plan {
        writable: vec![PathBuf::from("/work")],
        proxy: Some("127.0.0.1:8123".into()),
        isolation: Some(Isolation {
            hide: Vec::new(),
            bridge: None,
            net_confined,
        }),
    };
    assert!(!held(false).enforced(), "the network isn't fenced in");
    let bare = Plan {
        isolation: None,
        ..held(true)
    };
    assert!(!bare.enforced(), "no isolation at all");
    // Writes: Landlock on Linux; Seatbelt on macOS and the process container
    // on Windows, which a plan has isolation only with.
    #[cfg(target_os = "linux")]
    assert_eq!(held(true).enforced(), landlock::abi().is_some());
    #[cfg(any(target_os = "macos", windows))]
    assert!(held(true).enforced());
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
    std::fs::create_dir_all(home.join(".cargo/bin")).unwrap();
    let extra = [home.clone(), base.join("missing")];
    let roots = writable_roots(&ws, Some(&home), &config, &extra);
    assert_eq!(roots[0], ws, "workspace first");
    // Only the download cache is writable, made up front since cargo is
    // installed. The tool's own folder holds `bin` and its config file, which
    // run later with no sandbox, so neither is a root.
    assert!(roots.contains(&home.join(".cargo/registry")));
    assert!(roots.contains(&home.join(".cargo/git")));
    assert!(!roots.contains(&home.join(".cargo")));
    assert!(!roots.iter().any(|r| r.starts_with(home.join(".cargo/bin"))));
    // No folder appears for a tool that isn't installed.
    assert!(!home.join(".m2").exists());
    assert!(!home.join(".gradle").exists());
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

/// Linux with namespaces: a command can't reach a port on the host's loopback
/// directly (no `--noproxy` way out), the proxy still carries it there, a
/// server the command starts on its own loopback works, the config folder
/// reads as empty, new Unix sockets are refused while socket pairs work, and
/// a script's `wait` doesn't wait on the relay.
#[cfg(target_os = "linux")]
#[test]
fn a_command_in_its_own_network_reaches_out_only_through_the_proxy() {
    if !netns::available() {
        eprintln!("skipped: unprivileged namespaces are not allowed here");
        return;
    }
    let config = scratch("ns-cfg");
    let _pin = crate::perm::ConfigGuard::set(&config);
    std::fs::write(config.join("secret.txt"), "hunter2").unwrap();
    // The host-side test server is on loopback, which the proxy reaches only
    // when a rule names it.
    std::fs::write(
        config.join("permission-rules.json"),
        r#"{"allow":["WebFetch(domain:127.0.0.1)"]}"#,
    )
    .unwrap();
    let ws = scratch("ns-ws");
    let run =
        |command: String| crate::tools::shell::run(&ws, &json!({ "command": command }), &|| false);

    // A host-side server on loopback that answers "pong".
    let server = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = server.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for conn in server.incoming().flatten() {
            let mut conn = conn;
            let _ = std::io::Write::write_all(&mut conn, b"pong\n");
        }
    });

    let out = run(format!(
        "exec 3<>/dev/tcp/127.0.0.1/{port} && head -c4 <&3 || echo blocked"
    ));
    assert!(out.text.contains("blocked"), "{}", out.text);
    assert!(!out.text.contains("pong"), "{}", out.text);

    // Through the proxy (a CONNECT on its port, as an HTTP client would).
    let proxy_port = plan_for_call("run_terminal_command", &json!({"command": "x"}), &ws)
        .and_then(|p| p.proxy)
        .and_then(|a| a.rsplit(':').next().map(str::to_string))
        .unwrap();
    let out = run(format!(
        "exec 3<>/dev/tcp/127.0.0.1/{proxy_port} && \
         printf 'CONNECT 127.0.0.1:{port} HTTP/1.1\\r\\nHost: 127.0.0.1:{port}\\r\\n\\r\\n' >&3 && \
         timeout 5 head -c 200 <&3"
    ));
    assert!(out.text.contains("200"), "{}", out.text);
    assert!(out.text.contains("pong"), "{}", out.text);

    // Its own loopback works for a server it starts itself.
    if std::process::Command::new("python3")
        .arg("-V")
        .output()
        .is_ok()
    {
        let out = run("python3 -c 'import socket,threading\n\
             s=socket.socket(); s.bind((\"127.0.0.1\",0)); s.listen(1)\n\
             p=s.getsockname()[1]\n\
             threading.Thread(target=lambda: s.accept()[0].sendall(b\"hi\")).start()\n\
             c=socket.create_connection((\"127.0.0.1\",p)); print(c.recv(2))\n\
             a,b=socket.socketpair(); a.sendall(b\"pair\"); print(b.recv(4))\n\
             try:\n  socket.socket(socket.AF_UNIX)\n  print(\"unix-ok\")\n\
             except PermissionError:\n  print(\"unix-refused\")'"
            .into());
        assert!(out.text.contains("b'hi'"), "{}", out.text);
        assert!(out.text.contains("b'pair'"), "{}", out.text);
        assert!(out.text.contains("unix-refused"), "{}", out.text);
    }

    let out = run(format!(
        "cat {}/secret.txt || echo hidden",
        config.display()
    ));
    assert!(out.text.contains("hidden"), "{}", out.text);
    assert!(!out.text.contains("hunter2"), "{}", out.text);
    assert_eq!(
        std::fs::read_to_string(config.join("secret.txt")).unwrap(),
        "hunter2"
    );

    let started = std::time::Instant::now();
    let out = run("sleep 0.1 & wait; echo waited".into());
    assert!(out.text.contains("waited"), "{}", out.text);
    assert!(
        started.elapsed() < std::time::Duration::from_secs(10),
        "wait returned without the relay"
    );

    let _ = std::fs::remove_dir_all(&config);
    let _ = std::fs::remove_dir_all(&ws);
}

/// The filter's jumps land where they mean to: on socket, one of the two
/// refused families or the io_uring calls, the last instruction refuses.
#[cfg(target_os = "linux")]
#[test]
fn the_seccomp_program_refuses_unix_vsock_and_io_uring_only() {
    let prog = seccomp::program();
    if prog.is_empty() {
        return;
    }
    let last = prog.len() - 1;
    for (pc, ins) in prog.iter().enumerate() {
        if ins.code == (libc::BPF_JMP | libc::BPF_JEQ | libc::BPF_K) as u16 {
            let target = pc + 1 + ins.jt as usize;
            assert!(target <= last, "jump at {pc} lands past the end");
            if [
                libc::SYS_io_uring_setup as u32,
                libc::SYS_io_uring_enter as u32,
                libc::SYS_io_uring_register as u32,
                libc::AF_UNIX as u32,
                40,
            ]
            .contains(&ins.k)
                && pc > 4
            {
                assert_eq!(target, last, "jump at {pc} should refuse");
            }
        }
    }
    assert_eq!(prog[last].k & 0xffff, libc::EPERM as u32);
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

/// macOS, live: a command under the Seatbelt profile writes only in its
/// writable roots, can't read a hidden file, and reaches only the proxy's
/// port on loopback. Runs where macOS tests run (the nightly and manual
/// cross-check on Apple silicon).
#[cfg(target_os = "macos")]
#[test]
fn a_seatbelt_command_writes_reads_and_connects_only_where_allowed() {
    use std::io::Write as _;
    use std::net::TcpListener;

    if !seatbelt::available() {
        eprintln!("skipped: sandbox-exec is off or missing");
        return;
    }
    // Outside the temp folders, which a real plan leaves writable.
    let home = grokhub_core::user_home().expect("home");
    let base = home.join(format!("gh-sbx-mac-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let ws = base.join("ws");
    let outside = base.join("out");
    let secret_dir = base.join("secret");
    for dir in [&ws, &outside, &secret_dir] {
        std::fs::create_dir_all(dir).unwrap();
    }
    let (ws, outside, secret_dir) = (
        ws.canonicalize().unwrap(),
        outside.canonicalize().unwrap(),
        secret_dir.canonicalize().unwrap(),
    );
    let secret_file = base.join("netrc");
    std::fs::write(&secret_file, "machine x password y").unwrap();
    std::fs::write(secret_dir.join("id_ed25519"), "key").unwrap();
    let secret_file = secret_file.canonicalize().unwrap();

    let serve = || {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for mut s in l.incoming().flatten() {
                let _ = s.write_all(b"hello\n");
            }
        });
        port
    };
    let proxy_port = serve();
    let other_port = serve();

    let profile = seatbelt::profile(
        &[ws.clone(), PathBuf::from("/dev")],
        &[secret_dir.clone(), secret_file.clone()],
        Some(proxy_port),
    );
    let sh = |script: String| {
        let argv = ["bash", "--noprofile", "--norc", "-c", script.as_str()];
        let (program, args) = seatbelt::wrap(&profile, &argv);
        let out = std::process::Command::new(program)
            .args(args)
            .current_dir(&ws)
            .output()
            .expect("sandbox-exec runs");
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        (out.status.success(), text)
    };

    let (ok, text) = sh("echo in > inside.txt && cat inside.txt".into());
    assert!(
        ok && text.contains("in"),
        "a write in the workspace: {text}"
    );

    let target = outside.join("new.txt");
    let (ok, text) = sh(format!("echo out > '{}'", target.display()));
    assert!(
        !ok && !target.exists(),
        "a write outside the workspace went through: {text}"
    );

    let (ok, text) = sh(format!("cat '{}'", secret_file.display()));
    assert!(
        !ok && !text.contains("password"),
        "a hidden file was read: {text}"
    );
    let (ok, text) = sh(format!("ls '{}'", secret_dir.display()));
    assert!(
        !ok && !text.contains("id_ed25519"),
        "a hidden folder was listed: {text}"
    );

    let connect = |port: u16| format!("exec 3<>/dev/tcp/127.0.0.1/{port} && head -n1 <&3");
    let (ok, text) = sh(connect(proxy_port));
    assert!(ok && text.contains("hello"), "the proxy's port: {text}");
    let (ok, text) = sh(connect(other_port));
    assert!(
        !ok && !text.contains("hello"),
        "another loopback port was reached: {text}"
    );

    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn a_failed_connection_in_its_own_network_gets_a_note_and_nothing_else_does() {
    let isolated = Plan {
        writable: Vec::new(),
        proxy: Some("127.0.0.1:1".into()),
        isolation: Some(Isolation {
            hide: Vec::new(),
            bridge: Some((PathBuf::from("/tmp/s"), 1)),
            net_confined: true,
        }),
    };
    let ssh = "ssh: connect to host github.com port 22: Network is unreachable";
    assert!(network_note(Some(&isolated), true, ssh).is_some());
    assert!(network_note(Some(&isolated), false, ssh).is_none());
    assert!(network_note(Some(&isolated), true, "error: no such file").is_none());
    // macOS's words for a fenced-in lookup.
    let mac = "curl: (6) Could not resolve: nodename nor servname provided, or not known";
    assert!(network_note(Some(&isolated), true, mac).is_some());
    let open = Plan {
        isolation: None,
        ..isolated.clone()
    };
    assert!(network_note(Some(&open), true, ssh).is_none());
    assert!(network_note(None, true, ssh).is_none());
    // The proxy's refusal gets the list note wherever the proxy is set.
    let refused = "curl: (56) CONNECT tunnel failed, response 403";
    let note = network_note(Some(&open), true, refused).unwrap();
    assert!(note.contains("WebFetch allow rules"), "{note}");
    assert!(network_note(Some(&open), false, refused).is_none());
    let no_proxy = Plan {
        proxy: None,
        ..open.clone()
    };
    assert!(network_note(Some(&no_proxy), true, refused).is_none());
}
