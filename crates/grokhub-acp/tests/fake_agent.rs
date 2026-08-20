use grokhub_acp::{connect, wait_event, AcpEvent, SessionMode, SpawnOpts};
use std::time::{Duration, Instant};

fn fake_opts() -> SpawnOpts {
    SpawnOpts {
        program: std::path::PathBuf::from(env!("CARGO_BIN_EXE_grokhub-fake-acp")),
        args: vec![],
        cwd: std::env::temp_dir(),
        api_key: None,
        always_approve: true,
        auto: false,
        session_mode: SessionMode::Chat,
        extra_env: vec![],
        handshake_timeout: None,
        resume: None,
    }
}

#[test]
fn handshake_prompt_and_stream() {
    let h = connect(fake_opts()).expect("connect fake acp");
    assert_eq!(h.session_id, "sess-test");
    h.prompt("hi").unwrap();
    let mut thought = String::new();
    let mut text = String::new();
    let mut done = false;
    for _ in 0..40 {
        match wait_event(&h.events, Duration::from_secs(2)) {
            Ok(AcpEvent::Ready { .. }) => {}
            Ok(AcpEvent::Thought(t)) => thought.push_str(&t),
            Ok(AcpEvent::Text(t)) => text.push_str(&t),
            Ok(AcpEvent::Done { .. }) => {
                done = true;
                break;
            }
            Ok(AcpEvent::Err(e)) => panic!("{e}"),
            Ok(_) => {}
            Err(e) => panic!("{e}"),
        }
    }
    assert!(thought.contains("thinking"), "{thought}");
    assert!(text.contains("hello from grok build"), "{text}");
    assert!(done);
}

#[test]
fn computer_tool_emits_frame() {
    let mut opts = fake_opts();
    opts.extra_env = vec![
        ("FAKE_ACP_TOOL".into(), "computer_screenshot".into()),
        ("FAKE_ACP_IMAGE".into(), "QQ==".into()),
    ];
    let h = connect(opts).expect("connect");
    h.prompt("look").unwrap();
    let mut card = None;
    for _ in 0..40 {
        match wait_event(&h.events, Duration::from_secs(2)) {
            Ok(AcpEvent::Tool(t)) => {
                card = Some(t);
            }
            Ok(AcpEvent::Done { .. }) => break,
            Ok(AcpEvent::Err(e)) => panic!("{e}"),
            Ok(_) => {}
            Err(e) => panic!("{e}"),
        }
    }
    let card = card.expect("tool card");
    assert!(card.is_computer_use());
    assert!(card
        .image_data_url
        .as_deref()
        .unwrap_or("")
        .contains("QQ=="));
}

#[test]
fn cancel_does_not_panic() {
    let h = connect(fake_opts()).expect("connect");
    h.prompt("hi").unwrap();
    h.cancel().unwrap();
}

#[test]
fn handshake_times_out_on_a_silent_child() {
    let opts = SpawnOpts {
        program: std::path::PathBuf::from("sleep"),
        args: vec!["30".into()],
        cwd: std::env::temp_dir(),
        api_key: None,
        always_approve: true,
        auto: false,
        session_mode: SessionMode::Chat,
        extra_env: vec![],
        handshake_timeout: Some(Duration::from_secs(2)),
        resume: None,
    };
    let t = Instant::now();
    let err = match connect(opts) {
        Err(e) => e,
        Ok(_) => panic!("silent child must fail handshake"),
    };
    assert!(
        err.to_ascii_lowercase().contains("timed out") || err.contains("closed"),
        "{err}"
    );
    assert!(
        t.elapsed() < Duration::from_secs(8),
        "handshake hung for {:?}",
        t.elapsed()
    );
}

#[test]
fn handshake_does_not_deadlock_when_stderr_floods() {
    let opts = SpawnOpts {
        program: std::path::PathBuf::from("sh"),
        args: vec![
            "-c".into(),
            "dd if=/dev/zero bs=1024 count=256 1>&2 2>/dev/null; sleep 30".into(),
        ],
        cwd: std::env::temp_dir(),
        api_key: None,
        always_approve: true,
        auto: false,
        session_mode: SessionMode::Chat,
        extra_env: vec![],
        handshake_timeout: Some(Duration::from_secs(2)),
        resume: None,
    };
    let t = Instant::now();
    let err = match connect(opts) {
        Err(e) => e,
        Ok(_) => panic!("flooded stderr must not deadlock"),
    };
    assert!(!err.is_empty(), "{err}");
    assert!(
        t.elapsed() < Duration::from_secs(8),
        "stderr flood deadlocked for {:?}",
        t.elapsed()
    );
}
