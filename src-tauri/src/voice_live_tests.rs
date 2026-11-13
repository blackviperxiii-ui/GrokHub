use super::*;
use std::io::Write;
use std::net::TcpListener;

fn bytes(samples: &[i16]) -> Vec<u8> {
    samples.iter().flat_map(|s| s.to_le_bytes()).collect()
}

fn tone(ms: u32) -> Vec<i16> {
    let n = (RATE * ms / 1000) as usize;
    (0..n)
        .map(|i| {
            let t = i as f32 / RATE as f32;
            let env = 0.08 + 0.92 * (2.0 * std::f32::consts::PI * 2.0 * t).sin().abs();
            (9000.0 * env * (2.0 * std::f32::consts::PI * 220.0 * t).sin()) as i16
        })
        .collect()
}

fn hush(ms: u32) -> Vec<i16> {
    let n = (RATE * ms / 1000) as usize;
    (0..n).map(|i| ((i * 7919 % 61) as i16) - 30).collect()
}

/// Sends `audio` as the page does: 100 ms blocks.
fn send(live: &mut Live, audio: &[i16]) -> Vec<LiveEvent> {
    audio
        .chunks(1600)
        .flat_map(|b| live.push(&bytes(b)).expect("block"))
        .collect()
}

#[test]
fn a_spoken_turn_comes_out_as_a_wav_once() {
    let mut live = Live::start();
    let mut audio = hush(500);
    audio.extend(tone(1500));
    audio.extend(hush(2000));
    let events = send(&mut live, &audio);
    assert_eq!(events.first(), Some(&LiveEvent::Started), "{events:?}");
    let Some(LiveEvent::Turn { ms }) = events.last() else {
        panic!("a turn: {events:?}");
    };
    assert!(*ms >= 1500, "{ms}");
    let wav = live.take_turn().expect("the turn");
    assert_eq!(&wav[0..4], b"RIFF");
    assert_eq!(u32::from_le_bytes(wav[24..28].try_into().unwrap()), 16_000);
    assert_eq!(live.take_turn(), None, "once only");
    // Not listening until the page asks again.
    assert_eq!(send(&mut live, &audio), vec![]);
    live.listen(false);
    assert!(matches!(
        send(&mut live, &audio).last(),
        Some(LiveEvent::Turn { .. })
    ));
}

#[test]
fn a_bad_block_is_refused() {
    let mut live = Live::start();
    assert_eq!(live.push(&[1, 2, 3]), Err(BAD_BLOCK.to_string()));
    assert_eq!(
        live.push(&vec![0; BLOCK_MAX + 2]),
        Err(BAD_BLOCK.to_string())
    );
    assert_eq!(live.push(&[0; 3200]), Ok(vec![]));
}

#[test]
fn events_reach_the_page_as_tagged_objects() {
    assert_eq!(
        serde_json::to_value(LiveEvent::Turn { ms: 1200 }).unwrap(),
        serde_json::json!({"kind": "turn", "ms": 1200})
    );
    assert_eq!(
        serde_json::to_value(LiveEvent::CheckIn).unwrap(),
        serde_json::json!({"kind": "check_in"})
    );
}

#[test]
fn a_reply_streams_into_pieces_and_a_new_one_starts_after() {
    let mut live = Live::start();
    let mut spoken = live.reply_feed("Okay, the game is updated and ready ");
    spoken.extend(live.reply_feed("to launch now. Have fun!"));
    assert_eq!(
        spoken,
        vec!["Okay, the game is updated and ready to launch now.".to_string()]
    );
    assert_eq!(live.reply_finish("ignored"), vec!["Have fun!".to_string()]);
    // The next reply starts fresh.
    assert_eq!(
        live.reply_finish("A reply that never streamed."),
        vec!["A reply that never streamed.".to_string()]
    );
}

#[test]
fn a_whole_reply_drops_thinking_lines() {
    let mut live = Live::start();
    assert_eq!(
        live.reply_finish("THINKING:\nplan the night\n\nI'll look."),
        vec!["I'll look.".to_string()]
    );
}

#[test]
fn a_hushed_reply_speaks_no_more() {
    let mut live = Live::start();
    let _ = live.reply_feed("Here is the first sentence of it. And");
    live.reply_hush();
    assert_eq!(
        live.reply_feed(" more words after the tap."),
        Vec::<String>::new()
    );
    assert_eq!(live.reply_finish(""), Vec::<String>::new());
}

/// A mock TTS server: answers one request with `audio`, hands back the request.
fn mock_tts(audio: &'static [u8]) -> (String, std::thread::JoinHandle<Vec<u8>>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let handle = std::thread::spawn(move || {
        let (mut sock, _) = listener.accept().expect("accept");
        let mut got = Vec::new();
        let mut byte = [0u8; 1];
        while !got.ends_with(b"\r\n\r\n") && sock.read(&mut byte).unwrap_or(0) == 1 {
            got.push(byte[0]);
        }
        let head = String::from_utf8_lossy(&got).to_ascii_lowercase();
        let len = head
            .lines()
            .find_map(|l| l.strip_prefix("content-length:"))
            .map_or(0, |n| n.trim().parse::<usize>().expect("length"));
        let mut body = vec![0u8; len];
        sock.read_exact(&mut body).expect("body");
        got.extend_from_slice(&body);
        let head = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: audio/mpeg\r\nContent-Length: {}\r\n\r\n",
            audio.len()
        );
        sock.write_all(head.as_bytes()).expect("head");
        sock.write_all(audio).expect("audio");
        got
    });
    (format!("http://127.0.0.1:{port}/v1/tts"), handle)
}

fn temp_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "grokhub-shell-voice-live-{name}-{}-{}",
        std::process::id(),
        grokhub_core::now_ms()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

const MP3: &[u8] = b"ID3\x04\x00\x00\x00\x00\x00\x00 forty bytes of pretend mp3 data";

#[test]
fn a_piece_is_spoken_with_xai_in_the_chats_voice() {
    use base64::Engine as _;
    let (url, server) = mock_tts(MP3);
    let config = temp_dir("xai");
    let to = TtsTarget {
        url,
        bearer: "sk-abcdefghijklmnopqrstuv".into(),
    };
    let spoken = speak_xai(&config, &to, "rex", "Hello there.").expect("spoken");
    assert_eq!(spoken.mime, "audio/mpeg");
    assert_eq!(
        base64::engine::general_purpose::STANDARD
            .decode(spoken.audio)
            .unwrap(),
        MP3
    );
    let req = server.join().expect("server");
    let text = String::from_utf8_lossy(&req);
    assert!(text.starts_with("POST /v1/tts HTTP/1.1\r\n"), "{text}");
    assert!(
        text.contains("authorization: Bearer sk-abcdefghijklmnopqrstuv\r\n"),
        "{text}"
    );
    let body_at = text.find("\r\n\r\n").expect("head end") + 4;
    let body: serde_json::Value = serde_json::from_str(&text[body_at..]).expect("json");
    assert_eq!(
        body,
        serde_json::json!({"text": "Hello there.", "voice_id": "rex", "language": "en"})
    );
}

#[test]
fn a_tiny_answer_is_not_speech() {
    let (url, server) = mock_tts(b"nope");
    let config = temp_dir("tiny");
    let to = TtsTarget {
        url,
        bearer: "sk-abcdefghijklmnopqrstuv".into(),
    };
    assert_eq!(
        speak_xai(&config, &to, "ara", "Hello there."),
        Err("xAI sent back no speech.".to_string())
    );
    server.join().expect("server");
}

#[test]
fn offline_refuses_speech_before_anything_leaves() {
    let _off = grokhub_core::offline::force_for_thread(true);
    let config = temp_dir("offline");
    let to = TtsTarget {
        url: grokhub_core::tts_url(),
        bearer: "sk-abcdefghijklmnopqrstuv".into(),
    };
    assert_eq!(
        speak_xai(&config, &to, "ara", "Hello."),
        Err(grokhub_core::offline::OFFLINE_MSG.to_string())
    );
}

#[test]
fn without_the_on_device_engine_there_is_no_local_voice() {
    let config = temp_dir("no-engine");
    assert_eq!(
        speak_on_device(&config, &config.join("models"), "ara", "Hello."),
        None
    );
}

#[test]
fn no_turn_hears_nothing() {
    let config = temp_dir("hear");
    assert_eq!(hear(&config, None), Ok(String::new()));
}
