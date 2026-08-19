use grokhub_acp::{connect, wait_event, AcpEvent, SessionMode, SpawnOpts};
use std::time::Duration;

fn fake_bin() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_BIN_EXE_grokhub-fake-acp"))
}

#[test]
fn handshake_prompt_and_stream() {
    let opts = SpawnOpts {
        program: fake_bin(),
        args: vec![],
        cwd: std::env::temp_dir(),
        api_key: None,
        always_approve: true,
        auto: false,
        session_mode: SessionMode::Code,
        extra_env: vec![],
    };
    let h = connect(opts).expect("connect fake acp");
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
    let opts = SpawnOpts {
        program: fake_bin(),
        args: vec![],
        cwd: std::env::temp_dir(),
        api_key: None,
        always_approve: true,
        auto: false,
        session_mode: SessionMode::Code,
        extra_env: vec![
            ("FAKE_ACP_TOOL".into(), "computer_screenshot".into()),
            ("FAKE_ACP_IMAGE".into(), "QQ==".into()),
        ],
    };
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
    let opts = SpawnOpts {
        program: fake_bin(),
        args: vec![],
        cwd: std::env::temp_dir(),
        api_key: None,
        always_approve: true,
        auto: false,
        session_mode: SessionMode::Code,
        extra_env: vec![],
    };
    let h = connect(opts).expect("connect");
    h.prompt("hi").unwrap();
    h.cancel().unwrap();
}
