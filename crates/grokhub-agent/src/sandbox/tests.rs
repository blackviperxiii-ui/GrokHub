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
fn the_container_backend_is_opt_in_and_names_its_image() {
    let default = Settings::default();
    assert!(!default.container);
    assert_eq!(default.image, "docker.io/library/debian:stable-slim");
    let s = parse_file(
        "backend = \"container\"\nimage = \"docker.io/library/rust:1-slim\"\n",
        None,
    );
    assert!(s.container);
    assert_eq!(s.image, "docker.io/library/rust:1-slim");
    assert!(!parse_file("backend = \"host\"", None).container);
    // A value that would read as a flag to the runtime is refused.
    assert_eq!(
        parse_file("image = \"--privileged\"", None).image,
        "docker.io/library/debian:stable-slim"
    );
}

/// A container plan counts as enforced and leaves the client unconfined, so
/// the runtime can write its own state; the host fields stay for the PTC
/// harness.
#[test]
fn a_container_plan_is_enforced_and_skips_host_confinement() {
    let plan = Plan {
        writable: vec![PathBuf::from("/home/user/my-app")],
        proxy: None,
        isolation: None,
        container: Some(container::Container {
            runtime: container::Runtime {
                kind: container::Kind::Podman,
                program: PathBuf::from("/usr/bin/podman"),
            },
            image: container::DEFAULT_IMAGE.into(),
            mounts: vec![PathBuf::from("/home/user/my-app")],
            hide: Vec::new(),
            egress: None,
        }),
    };
    assert!(plan.enforced());
    // `apply` is Unix only: Windows never plans a container (see
    // `container::detect`) and spawns through its process container.
    #[cfg(unix)]
    {
        let mut cmd = std::process::Command::new("true");
        apply(&mut cmd, &plan).unwrap();
        assert_eq!(cmd.get_envs().count(), 0, "no proxy variables or tokens");
    }
    assert_eq!(
        network_note(Some(&plan), true, "curl: (6) Could not resolve host: example.com"),
        Some("[sandbox] This command ran in a container with no network, so nothing it starts can reach a host. If it needs the network, rerun with outside_sandbox (asks the user).")
    );
    // With the relay, a refused host names the proxy's list, and a lookup
    // that fails names what the relay doesn't carry.
    let mut relayed = plan.clone();
    relayed.container.as_mut().unwrap().egress = Some(container::Egress {
        socket: PathBuf::from("/tmp/egress-7-0/s"),
        port: 4000,
        env: Vec::new(),
        files: Vec::new(),
    });
    assert!(relayed.enforced());
    assert_eq!(
        network_note(Some(&relayed), true, "curl: (56) CONNECT tunnel failed, response 403"),
        Some("[sandbox] The proxy only lets sandboxed commands reach package registries, code hosts, model hosts and hosts your WebFetch allow rules name. For another host, rerun with outside_sandbox (asks the user).")
    );
    assert_eq!(
        network_note(Some(&relayed), true, "ssh: Could not resolve hostname github.com"),
        Some("[sandbox] This command ran in a container whose only way out is HTTP(S) through the proxy, so SSH, DNS lookups, raw sockets and services on this computer don't reach. If it needs them, rerun with outside_sandbox (asks the user).")
    );
}

/// Live: with `backend = "container"` and a rootless runtime (or the app
/// running as root) that already has the default image, a command writes
/// the workspace, can't write the image or see the config folder, reaches
/// out only through the egress proxy, and runs as the user's own uid.
#[cfg(unix)]
#[test]
fn a_command_in_a_container_sees_only_the_workspace_and_reaches_out_only_through_the_proxy() {
    let Some(runtime) = container::detect() else {
        eprintln!("skipped: no rootless Podman or Docker here");
        return;
    };
    let has_image = std::process::Command::new(&runtime.program)
        .args(["image", "inspect", container::DEFAULT_IMAGE])
        .output()
        .is_ok_and(|o| o.status.success());
    if !has_image {
        eprintln!("skipped: {} is not pulled", container::DEFAULT_IMAGE);
        return;
    }
    let config = scratch("ctr-cfg");
    let _pin = crate::perm::ConfigGuard::set(&config);
    std::fs::write(config.join(CONFIG_NAME), "backend = \"container\"\n").unwrap();
    std::fs::write(config.join("secret.txt"), "hunter2").unwrap();
    let ws = scratch("ctr-ws").canonicalize().unwrap();
    let run_with = |args: serde_json::Value| crate::tools::shell::run(&ws, &args, &|| false);
    let run = |command: String| run_with(json!({ "command": command }));

    let out = run("echo made > out.txt && id -u && pwd".into());
    // SAFETY: getuid can't fail.
    let uid = unsafe { libc::getuid() };
    assert_eq!(
        out.text.trim(),
        format!("exit 0\n{uid}\n{}", ws.display()),
        "{}",
        out.text
    );
    assert_eq!(
        std::fs::read_to_string(ws.join("out.txt")).unwrap(),
        "made\n"
    );

    let out = run(format!(
        "test -e {} && echo seen || echo hidden; touch /usr/x 2>/dev/null && echo wrote || echo read-only",
        config.join("secret.txt").display()
    ));
    assert_eq!(out.text.trim(), "exit 0\nhidden\nread-only", "{}", out.text);

    let out = run("(exec 3<>/dev/tcp/1.1.1.1/80) 2>/dev/null && echo out || echo blocked".into());
    assert_eq!(out.text.trim(), "exit 0\nblocked", "{}", out.text);

    // The egress proxy still carries it, through the relay inside (a
    // CONNECT on the port the variables name, as an HTTP client would), to
    // a host-side server a rule names; that server isn't reachable directly.
    std::fs::write(
        config.join("permission-rules.json"),
        r#"{"allow":["WebFetch(domain:127.0.0.1)"]}"#,
    )
    .unwrap();
    let server = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = server.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for mut conn in server.incoming().flatten() {
            let _ = std::io::Write::write_all(&mut conn, b"pong\n");
        }
    });
    let out = run(format!(
        "(exec 3<>/dev/tcp/127.0.0.1/{port}) 2>/dev/null && echo direct || echo blocked; \
         p=${{HTTPS_PROXY##*:}}; exec 3<>/dev/tcp/127.0.0.1/$p && \
         printf 'CONNECT 127.0.0.1:{port} HTTP/1.1\\r\\nHost: 127.0.0.1:{port}\\r\\n\\r\\n' >&3 && \
         timeout 5 head -c 200 <&3"
    ));
    assert!(out.text.contains("blocked"), "{}", out.text);
    assert!(!out.text.contains("direct"), "{}", out.text);
    assert!(out.text.contains(" 200 "), "{}", out.text);
    assert!(out.text.contains("pong"), "{}", out.text);

    // A timeout kills the runtime's client; the watchdog then ends the
    // container, so none is left running. The config pin is per thread, so
    // the command runs here and a helper thread watches.
    let program = runtime.program.clone();
    let running = move || {
        let out = std::process::Command::new(&program)
            .args([
                "ps",
                "--quiet",
                "--filter",
                &format!("label={}", container::LABEL),
            ])
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).lines().count()
    };
    let watch = {
        let running = running.clone();
        std::thread::spawn(move || {
            (0..40).any(|_| {
                std::thread::sleep(std::time::Duration::from_millis(100));
                running() == 1
            })
        })
    };
    let out = run_with(json!({ "command": "sleep 60", "timeout": 4000 }));
    assert!(out.text.contains("timed out"), "{}", out.text);
    assert!(
        watch.join().unwrap(),
        "the slow command's container never showed up"
    );
    let gone = (0..50).any(|_| {
        std::thread::sleep(std::time::Duration::from_millis(100));
        running() == 0
    });
    assert!(gone, "a container outlived its command");
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
        container: None,
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

/// Only a command that is one privilege raise leaves the sandbox: its
/// password dialog stands for all of it. A raise inside a longer command
/// used to take every other part out with it, dialog or not (a sudo that
/// never runs asks nothing), so those now stay in. So does a `sudo` that
/// is not the system's: one run from a path (`./sudo`), found through a
/// `PATH=` the command sets, or a program renamed with `exec -a sudo`, and
/// so does anything set or wrapped around the raise.
#[test]
fn a_privilege_raise_counts_only_when_it_is_the_whole_command() {
    for cmd in [
        "sudo apt update",
        "/usr/bin/sudo -n true",
        "doas reboot",
        "pkexec ls",
        "\"sudo\" ls",
        "su -c id",
        "run0 systemctl restart foo",
        "sudo ls > /dev/null 2>&1",
    ] {
        assert!(raises_privileges(cmd), "{cmd}");
    }
    for cmd in [
        "./sudo",
        "./sudo apt update",
        "bin/sudo true",
        "/tmp/sudo true",
        "/usr/bin/../../tmp/sudo true",
        "PATH=/work:/usr/bin:/bin sudo true",
        "FOO=1 doas reboot",
        "LD_PRELOAD=/work/x.so run0 true",
        "env -i sudo ls",
        "env PATH=/work sudo ls",
        "nohup sudo make install",
        "exec -a sudo bash -c 'echo out > /tmp/x'",
        "command sudo ls",
        "cd /x && sudo make install",
        "echo hi; pkexec ls",
        "x=$(sudo cat /etc/shadow)",
        "false && sudo true; echo out > /tmp/x",
        "sudo true > /home/user/.bashrc",
        "sudo ls $(touch /tmp/x)",
        "sudo ls | tee x",
        "sudo ls &",
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

#[test]
fn a_container_plan_leaves_out_a_writable_folder_that_holds_the_config_folder() {
    let base = scratch("plan-ctr");
    let ws = base.join("ws");
    let extra = base.join("extra");
    let config = extra.join("cfg");
    std::fs::create_dir_all(&ws).unwrap();
    std::fs::create_dir_all(&config).unwrap();
    let settings = format!(
        "backend = \"container\"\nwritable = [{:?}]\n",
        extra.display().to_string()
    );
    std::fs::write(config.join(CONFIG_NAME), settings).unwrap();
    let docker = || {
        Some(container::Runtime {
            kind: container::Kind::Docker,
            program: PathBuf::from("/usr/bin/docker"),
        })
    };
    let args = json!({"command": "ls"});
    let plan = plan_with(&config, "run_terminal_command", &args, &ws, docker).unwrap();
    let c = plan.container.expect("the container backend is on");
    // `extra` holds the config folder, so only the workspace mounts.
    assert_eq!(c.mounts, vec![ws.canonicalize().unwrap()]);
    // A config folder inside the workspace is covered in the container.
    let inner = ws.join(".cfg");
    std::fs::create_dir_all(&inner).unwrap();
    std::fs::write(inner.join(CONFIG_NAME), "backend = \"container\"\n").unwrap();
    let plan = plan_with(&inner, "run_terminal_command", &args, &ws, docker).unwrap();
    let c = plan.container.expect("the container backend is on");
    assert_eq!(c.mounts, vec![ws.canonicalize().unwrap()]);
    assert_eq!(c.hide, vec![inner.canonicalize().unwrap()]);
    let _ = std::fs::remove_dir_all(&base);
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
        container: None,
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

/// A sudo that never runs (`false && sudo true; …`) takes nothing out of
/// the sandbox: the rest of the command is held like any other, so its
/// write outside the workspace is refused. Always mode still runs it.
#[test]
fn a_sudo_that_never_runs_keeps_the_rest_of_the_command_in_the_sandbox() {
    let config = scratch("sudo-dodge-cfg");
    let _pin = crate::perm::ConfigGuard::set(&config);
    let ws = scratch("sudo-dodge-ws");
    let outside = scratch("sudo-dodge-out");
    let dodge = format!(
        "false && sudo true; echo out > {}/dodge.txt",
        outside.display()
    );
    let args = json!({ "command": dodge });
    assert!(!runs_outside("run_terminal_command", &args));
    assert!(plan_for_call("run_terminal_command", &args, &ws).is_some());
    let always = crate::gate::Gate {
        mode: crate::gate::PermMode::Always,
        readonly_session: false,
        attended: true,
        desktop: false,
    };
    let policy = crate::perm::Policy::empty();
    assert_eq!(
        crate::gate::decide_with(
            &always,
            "run_terminal_command",
            &args.to_string(),
            false,
            None,
            &ws,
            Some(&policy)
        ),
        crate::gate::Decision::Run
    );
    #[cfg(target_os = "linux")]
    if landlock::abi().is_some() {
        let out = crate::tools::shell::run(&ws, &args, &|| false);
        assert!(out.failed, "{}", out.text);
        assert!(out.text.contains("Permission denied"), "{}", out.text);
        assert!(!outside.join("dodge.txt").exists(), "{}", out.text);
    }
    for dir in [&config, &ws, &outside] {
        let _ = std::fs::remove_dir_all(dir);
    }
}

/// A `sudo` the model left in the workspace, one a `PATH=` in the command
/// finds, or a program renamed `sudo` with `exec -a` raises nothing and
/// shows no password dialog, so it stays in the sandbox: its write outside
/// the workspace is refused, in Always mode too.
#[test]
fn a_planted_or_renamed_sudo_stays_in_the_sandbox() {
    let config = scratch("planted-sudo-cfg");
    let _pin = crate::perm::ConfigGuard::set(&config);
    let ws = scratch("planted-sudo-ws");
    let outside = scratch("planted-sudo-out");
    let planted = outside.join("planted.txt");
    std::fs::write(
        ws.join("sudo"),
        format!("#!/bin/sh\necho out > {}\n", planted.display()),
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(ws.join("sudo"), std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let always = crate::gate::Gate {
        mode: crate::gate::PermMode::Always,
        readonly_session: false,
        attended: true,
        desktop: false,
    };
    let policy = crate::perm::Policy::empty();
    let renamed = outside.join("argv0.txt");
    for (cmd, target) in [
        ("./sudo".to_string(), &planted),
        (
            format!("PATH={}:/usr/bin:/bin sudo true", ws.display()),
            &planted,
        ),
        (
            format!("exec -a sudo sh -c 'echo out > {}'", renamed.display()),
            &renamed,
        ),
    ] {
        let args = json!({ "command": cmd });
        assert!(!raises_privileges(&cmd), "{cmd}");
        assert!(!runs_outside("run_terminal_command", &args), "{cmd}");
        assert!(
            plan_for_call("run_terminal_command", &args, &ws).is_some(),
            "{cmd}"
        );
        assert_eq!(
            crate::gate::decide_with(
                &always,
                "run_terminal_command",
                &args.to_string(),
                false,
                None,
                &ws,
                Some(&policy)
            ),
            crate::gate::Decision::Run,
            "{cmd}"
        );
        #[cfg(target_os = "linux")]
        if landlock::abi().is_some() {
            let out = crate::tools::shell::run(&ws, &args, &|| false);
            assert!(out.failed, "{cmd}: {}", out.text);
            assert!(
                out.text.contains("Permission denied"),
                "{cmd}: {}",
                out.text
            );
            assert!(!target.exists(), "{cmd}: {}", out.text);
        }
        // Only Linux runs the command, so only it checks the target.
        #[cfg(not(target_os = "linux"))]
        let _ = target;
    }
    for dir in [&config, &ws, &outside] {
        let _ = std::fs::remove_dir_all(dir);
    }
}

/// A raise the sandbox refused gets a note to run it on its own.
#[test]
fn a_refused_privilege_raise_gets_a_note_to_run_it_alone() {
    let plan = Plan {
        writable: vec![PathBuf::from("/work")],
        proxy: None,
        isolation: None,
        container: None,
    };
    let refused =
        "sudo: The \"no new privileges\" flag is set, which prevents sudo from running as root.";
    assert_eq!(
        network_note(Some(&plan), true, refused),
        Some("[sandbox] sudo (or pkexec, doas, su, run0) can't raise privileges inside the sandbox. It runs outside only as a command of its own that starts with it, with nothing set, wrapped or chained around it (`sudo apt update`). Run it that way (the password dialog asks), or rerun with outside_sandbox (asks the user).")
    );
    assert_eq!(network_note(Some(&plan), false, refused), None);
    assert_eq!(network_note(None, true, refused), None);
}

/// With the sandbox off, the floor alone keeps a key path or the consent
/// ledger out of reach, however the command spells it, in Always and Ask.
#[test]
fn with_the_sandbox_off_quoted_and_globbed_key_paths_are_refused() {
    let config = scratch("rail-off-cfg");
    std::fs::write(config.join(CONFIG_NAME), "enabled = false\n").unwrap();
    let _pin = crate::perm::ConfigGuard::set(&config);
    let ws = scratch("rail-off-ws");
    let always = crate::gate::Gate {
        mode: crate::gate::PermMode::Always,
        readonly_session: false,
        attended: true,
        desktop: false,
    };
    let ask = crate::gate::Gate {
        mode: crate::gate::PermMode::Ask,
        ..always
    };
    let policy = crate::perm::Policy::empty();
    let ledger = "hard floor: consent ledger (only your click changes it)";
    for (c, why) in [
        ("cat ~/.ssh/id_rsa", "forbidden path: ssh keys"),
        (r#"cat ~/."ssh"/id_rsa"#, "forbidden path: ssh keys"),
        ("cat ~/.ss?/id_rsa", "forbidden path: ssh keys"),
        ("cat /etc//shadow", "forbidden path: /etc/shadow"),
        (
            concat!("cat ~/.grok//auth", ".json"),
            "forbidden path: grok login",
        ),
        ("echo '{}' >> ~/.config/GrokHub/consent.jsonl", ledger),
        ("echo '{}' >> ~/.config/GrokHub/consent.json?", ledger),
        (r#"echo '{}' >> ~/.config/GrokHub/"consent".jsonl"#, ledger),
        ("cat ~/.[[:alpha:]]sh/id_rsa", "forbidden path: ssh keys"),
        ("cat /etc/[[:lower:]]hadow", "forbidden path: /etc/shadow"),
        ("cat ~/.ss\\\nh/id_rsa", "forbidden path: ssh keys"),
        ("cat ~/.$\"ssh\"/id_rsa", "forbidden path: ssh keys"),
        ("cat ~/.s{s..s}h/id_rsa", "forbidden path: ssh keys"),
        ("cd ~ && cat .*/id_rsa", "forbidden path: ssh keys"),
        (
            "echo '{}' >> ~/.config/GrokHub/consent.json[[:lower:]]",
            ledger,
        ),
        ("echo '{}' >> ~/.config/GrokHub/consent.json$'l'", ledger),
        ("echo '{}' >> ~/.config/GrokHub/consent.json{l..l}", ledger),
        ("cd ~/.config/GrokHub && echo '{}' | tee -a *.jsonl", ledger),
    ] {
        let a = json!({"command": c}).to_string();
        for gate in [&always, &ask] {
            assert_eq!(
                crate::gate::decide_with(
                    gate,
                    "run_terminal_command",
                    &a,
                    false,
                    None,
                    &ws,
                    Some(&policy)
                ),
                crate::gate::Decision::Refuse(why.into()),
                "{c} in {:?}",
                gate.mode
            );
        }
    }
    for dir in [&config, &ws] {
        let _ = std::fs::remove_dir_all(dir);
    }
}
