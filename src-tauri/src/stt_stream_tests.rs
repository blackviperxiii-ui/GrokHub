//! The speech stream against a loopback socket that plays a script and
//! records what the client sent.

use std::io::{Read as _, Write as _};
use std::net::TcpListener;
use std::sync::mpsc::Receiver;
use std::thread::JoinHandle;

use super::*;

const KEY: &str = "sk-abcdefghijklmnopqrstuv";

/// Offline only stays off.
fn online() -> bool {
    false
}
const WAIT: Duration = Duration::from_secs(5);

fn temp_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "grokhub-shell-stt-{name}-{}-{}",
        std::process::id(),
        grokhub_core::now_ms()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

/// One frame the client sent.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Got {
    Bin(Vec<u8>),
    Text(String),
}

type Ws = tungstenite::WebSocket<TcpStream>;

/// What the fake saw: the upgrade's authorization header and every frame.
struct Seen {
    auth: String,
    got: Vec<Got>,
}

/// A loopback socket for one client: it accepts, runs `script`, then
/// records frames until the client closes.
fn fake(
    script: impl FnOnce(&mut Ws, &mut Vec<Got>) + Send + 'static,
) -> (String, JoinHandle<Seen>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let url = format!("ws://{}/v1/stt", listener.local_addr().expect("addr"));
    let done = std::thread::spawn(move || {
        let (tcp, _) = listener.accept().expect("accept");
        tcp.set_read_timeout(Some(WAIT)).expect("timeout");
        // The upgrade request, read without taking it off the socket.
        let mut head = [0u8; 4096];
        let n = tcp.peek(&mut head).expect("peek");
        let auth = String::from_utf8_lossy(&head[..n])
            .lines()
            .find_map(|l| l.strip_prefix("authorization: "))
            .unwrap_or_default()
            .to_string();
        let mut ws = tungstenite::accept(tcp).expect("upgrade");
        let mut got = Vec::new();
        script(&mut ws, &mut got);
        while next(&mut ws, &mut got).is_some() {}
        Seen { auth, got }
    });
    (url, done)
}

/// The client's next data frame, recorded; `None` once it closes.
fn next(ws: &mut Ws, got: &mut Vec<Got>) -> Option<Got> {
    loop {
        let frame = match ws.read().ok()? {
            Message::Binary(b) => Got::Bin(b.to_vec()),
            Message::Text(t) => Got::Text(t.as_str().to_string()),
            Message::Close(_) => return None,
            _ => continue,
        };
        got.push(frame.clone());
        return Some(frame);
    }
}

fn say(ws: &mut Ws, event: &str) {
    ws.send(Message::text(event)).expect("send");
}

fn news(rx: &Receiver<SttNews>) -> SttNews {
    rx.recv_timeout(WAIT).expect("news")
}

#[test]
fn blocks_go_out_as_16_bit_little_endian() {
    assert_eq!(pcm_bytes(&[1, -2, 256]), vec![1, 0, 0xFE, 0xFF, 0, 1]);
}

#[test]
fn no_key_means_no_stream() {
    static G: Gate = Gate::new();
    let dir = temp_dir("no-key");
    assert!(open_at(&dir, "ws://127.0.0.1:9/v1/stt", "  ", &G, online).is_none());
}

#[test]
fn only_a_plain_ws_loopback_override_counts() {
    for ok in [
        "ws://127.0.0.1:4612/v1/stt",
        "ws://localhost:9/stt",
        "ws://[::1]:9/stt",
    ] {
        assert_eq!(ws_loopback_override(Some(ok)).as_deref(), Some(ok), "{ok}");
    }
    for no in [
        "wss://api.x.ai/v1/stt",
        "wss://127.0.0.1:9/stt",
        "ws://10.0.0.5/stt",
        "ws://user@127.0.0.1:9/stt",
        "http://127.0.0.1:9/stt",
        "ws://localhost.example.com/stt",
        "ws://evil.com\\@127.0.0.1/",
        "",
    ] {
        assert_eq!(ws_loopback_override(Some(no)), None, "{no}");
    }
    assert_eq!(ws_loopback_override(None), None);
}

#[test]
fn audio_waits_for_ready_then_goes_out_and_finalize_brings_the_words() {
    static G: Gate = Gate::new();
    let (url, seen) = fake(|ws, got| {
        // The client holds its audio until the server is ready.
        let tcp = ws.get_ref();
        tcp.set_read_timeout(Some(Duration::from_millis(400)))
            .expect("timeout");
        assert!(
            matches!(ws.read(), Err(tungstenite::Error::Io(_))),
            "no audio before ready"
        );
        ws.get_ref().set_read_timeout(Some(WAIT)).expect("timeout");
        say(ws, r#"{"type":"transcript.created"}"#);
        assert!(matches!(next(ws, got), Some(Got::Bin(_))));
        say(
            ws,
            r#"{"type":"transcript.partial","text":"remind me","is_final":false,"speech_final":false}"#,
        );
        assert!(matches!(next(ws, got), Some(Got::Text(_))));
        say(
            ws,
            r#"{"type":"transcript.done","text":"Remind me to call Sam."}"#,
        );
    });
    let dir = temp_dir("finalize");
    let link = open_at(&dir, &url, KEY, &G, online).expect("a stream");
    link.audio.send(vec![1, -2, 256]).expect("audio");
    assert_eq!(news(&link.news), SttNews::Caption("remind me".into()));
    drop(link.audio);
    assert_eq!(
        news(&link.news),
        SttNews::Ended("Remind me to call Sam.".into())
    );
    let seen = seen.join().expect("fake");
    assert_eq!(seen.auth, format!("Bearer {KEY}"));
    assert_eq!(
        seen.got,
        vec![
            Got::Bin(vec![1, 0, 0xFE, 0xFF, 0, 1]),
            Got::Text(r#"{"type":"finalize"}"#.into()),
        ]
    );
}

#[test]
fn smart_turn_ends_the_turn_before_finalize() {
    static G: Gate = Gate::new();
    let (url, seen) = fake(|ws, _| {
        say(ws, r#"{"type":"transcript.created"}"#);
        say(
            ws,
            r#"{"type":"transcript.partial","text":"Buy milk.","is_final":true,"speech_final":true,"end_of_turn_confidence":0.98}"#,
        );
    });
    let dir = temp_dir("smart");
    let link = open_at(&dir, &url, KEY, &G, online).expect("a stream");
    assert_eq!(news(&link.news), SttNews::Ended("Buy milk.".into()));
    // The audio side is still open, yet nothing asked for the words.
    let seen = seen.join().expect("fake");
    assert_eq!(seen.got, Vec::<Got>::new());
    drop(link);
}

#[test]
fn a_server_error_is_reported_once() {
    static G: Gate = Gate::new();
    let (url, seen) = fake(|ws, _| {
        say(ws, r#"{"type":"transcript.created"}"#);
        say(ws, r#"{"type":"error","message":"bad audio"}"#);
    });
    let dir = temp_dir("error");
    let link = open_at(&dir, &url, KEY, &G, online).expect("a stream");
    assert_eq!(
        news(&link.news),
        SttNews::Failed("speech stream: bad audio".into())
    );
    assert!(link.news.recv_timeout(WAIT).is_err(), "nothing after");
    drop(link);
    let _ = seen.join();
}

#[test]
fn a_refused_upgrade_turns_streaming_off_for_the_run() {
    static G: Gate = Gate::new();
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let url = format!("ws://{}/v1/stt", listener.local_addr().expect("addr"));
    let server = std::thread::spawn(move || {
        let (mut tcp, _) = listener.accept().expect("accept");
        let mut buf = [0u8; 2048];
        let _ = tcp.read(&mut buf);
        tcp.write_all(
            b"HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        )
        .expect("answer");
    });
    let dir = temp_dir("refused");
    let link = open_at(&dir, &url, KEY, &G, online).expect("a stream");
    assert_eq!(
        news(&link.news),
        SttNews::Failed("speech stream refused (401)".into())
    );
    server.join().expect("server");
    assert!(
        open_at(&dir, &url, KEY, &G, online).is_none(),
        "refused for the run"
    );
    // A new key gets its own try.
    let other = open_at(&dir, &url, "sk-zyxwvutsrqponmlkjihgf", &G, online);
    assert!(other.is_some(), "a new key tries again");
}

#[test]
fn no_final_words_in_time_means_upload() {
    let short = Duration::from_millis(50);
    let (tx, rx) = mpsc::channel();
    assert_eq!(final_words(&rx, short), None, "silent");
    tx.send(SttNews::Failed("closed".into())).expect("send");
    assert_eq!(final_words(&rx, short), None, "failed");
    tx.send(SttNews::Caption("half".into())).expect("send");
    tx.send(SttNews::Ended("buy milk".into())).expect("send");
    assert_eq!(final_words(&rx, short), Some("buy milk".into()));
    drop(tx);
    assert_eq!(final_words(&rx, short), None, "gone");
}

#[test]
fn offline_only_opens_no_stream() {
    static G: Gate = Gate::new();
    let _off = grokhub_core::offline::force_for_thread(true);
    let dir = temp_dir("offline");
    assert!(open_at(&dir, "wss://api.x.ai/v1/stt", KEY, &G, online).is_none());
}

#[test]
fn noise_alone_mid_listen_keeps_the_stream_listening() {
    static G: Gate = Gate::new();
    let (url, seen) = fake(|ws, got| {
        say(ws, r#"{"type":"transcript.created"}"#);
        say(
            ws,
            r#"{"type":"transcript.partial","text":"","is_final":true,"speech_final":true}"#,
        );
        // Frames are handled in order: this caption arriving proves the
        // empty final before it ended nothing.
        say(
            ws,
            r#"{"type":"transcript.partial","text":"buy","is_final":false,"speech_final":false}"#,
        );
        assert!(matches!(next(ws, got), Some(Got::Bin(_))));
        assert!(matches!(next(ws, got), Some(Got::Text(_))));
        say(ws, r#"{"type":"transcript.done","text":"Buy milk."}"#);
    });
    let dir = temp_dir("noise");
    let link = open_at(&dir, &url, KEY, &G, online).expect("a stream");
    assert_eq!(
        news(&link.news),
        SttNews::Caption("buy".into()),
        "noise ends no turn"
    );
    link.audio.send(vec![7]).expect("audio");
    drop(link.audio);
    assert_eq!(news(&link.news), SttNews::Ended("Buy milk.".into()));
    let seen = seen.join().expect("fake");
    assert_eq!(
        seen.got,
        vec![
            Got::Bin(vec![7, 0]),
            Got::Text(r#"{"type":"finalize"}"#.into()),
        ]
    );
}

static OFFLINE_NOW: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn offline_now() -> bool {
    OFFLINE_NOW.load(Ordering::SeqCst)
}

#[test]
fn offline_only_mid_listen_stops_the_audio() {
    static G: Gate = Gate::new();
    let (url, seen) = fake(|ws, got| {
        say(ws, r#"{"type":"transcript.created"}"#);
        assert!(matches!(next(ws, got), Some(Got::Bin(_))));
        say(
            ws,
            r#"{"type":"transcript.partial","text":"remind me","is_final":false,"speech_final":false}"#,
        );
    });
    let dir = temp_dir("offline-mid");
    let link = open_at(&dir, &url, KEY, &G, offline_now).expect("a stream");
    link.audio.send(vec![1]).expect("audio");
    assert_eq!(news(&link.news), SttNews::Caption("remind me".into()));
    OFFLINE_NOW.store(true, Ordering::SeqCst);
    let _ = link.audio.send(vec![2]);
    assert_eq!(news(&link.news), SttNews::Failed(OFFLINE.into()));
    let seen = seen.join().expect("fake");
    assert_eq!(seen.got, vec![Got::Bin(vec![1, 0])], "nothing after");
}

#[test]
fn a_turn_over_before_the_server_is_ready_sends_nothing_and_ends() {
    static G: Gate = Gate::new();
    // The server never says it is ready.
    let (url, seen) = fake(|_, _| {});
    let dir = temp_dir("not-ready");
    let link = open_at(&dir, &url, KEY, &G, online).expect("a stream");
    link.audio.send(vec![1]).expect("audio");
    assert!(!link.is_ready(), "the server never said so");
    drop(link.audio);
    let from = Instant::now();
    assert!(matches!(
        link.news.recv_timeout(WAIT),
        Err(RecvTimeoutError::Disconnected)
    ));
    assert!(from.elapsed() < FINAL_WAIT, "ends at once");
    assert_eq!(seen.join().expect("fake").got, Vec::<Got>::new());
}

#[test]
fn a_quiet_wait_longer_than_a_turn_still_gets_the_turn_and_its_words() {
    static G: Gate = Gate::new();
    let (url, seen) = fake(|ws, got| {
        say(ws, r#"{"type":"transcript.created"}"#);
        assert!(matches!(next(ws, got), Some(Got::Bin(_))));
        say(
            ws,
            r#"{"type":"transcript.partial","text":"call mom","is_final":false,"speech_final":false}"#,
        );
        assert!(matches!(next(ws, got), Some(Got::Text(_))));
        say(ws, r#"{"type":"transcript.done","text":"Call mom."}"#);
    });
    let dir = temp_dir("quiet-first");
    let link = open_at(&dir, &url, KEY, &G, online).expect("a stream");
    // Nobody talks for longer than a turn may stream.
    let quiet = MAX_STREAM + Duration::from_millis(500);
    assert!(quiet < MAX_IDLE);
    std::thread::sleep(quiet);
    assert!(link.is_ready(), "the server said it is ready");
    link.audio.send(vec![7]).expect("audio");
    assert_eq!(news(&link.news), SttNews::Caption("call mom".into()));
    drop(link.audio);
    assert_eq!(news(&link.news), SttNews::Ended("Call mom.".into()));
    assert_eq!(
        seen.join().expect("fake").got,
        vec![
            Got::Bin(vec![7, 0]),
            Got::Text(r#"{"type":"finalize"}"#.into()),
        ]
    );
}

#[test]
fn a_turn_streaming_past_its_cap_fails_over_to_the_upload() {
    static G: Gate = Gate::new();
    let (url, seen) = fake(|ws, got| {
        say(ws, r#"{"type":"transcript.created"}"#);
        assert!(matches!(next(ws, got), Some(Got::Bin(_))));
    });
    let dir = temp_dir("cap");
    let link = open_at(&dir, &url, KEY, &G, online).expect("a stream");
    while !link.is_ready() {
        std::thread::sleep(POLL);
    }
    let from = Instant::now();
    link.audio.send(vec![1]).expect("audio");
    // Nothing ends the turn.
    assert_eq!(
        link.news.recv_timeout(MAX_STREAM + WAIT),
        Ok(SttNews::Failed("speech stream ran too long".into()))
    );
    let took = from.elapsed();
    assert!(took >= MAX_STREAM, "{took:?}");
    assert!(took < MAX_STREAM + Duration::from_millis(1500), "{took:?}");
    drop(link);
    let _ = seen.join();
}

#[test]
fn a_stream_nobody_talks_into_closes_after_the_idle_cap() {
    static G: Gate = Gate::new();
    let (url, seen) = fake(|ws, _| say(ws, r#"{"type":"transcript.created"}"#));
    let dir = temp_dir("idle");
    let from = Instant::now();
    let link = open_at(&dir, &url, KEY, &G, online).expect("a stream");
    assert_eq!(link.news.recv_timeout(MAX_IDLE + WAIT), Ok(SttNews::Idle));
    let took = from.elapsed();
    assert!(took >= MAX_IDLE, "{took:?}");
    assert!(took < MAX_IDLE + Duration::from_millis(1500), "{took:?}");
    assert_eq!(seen.join().expect("fake").got, Vec::<Got>::new());
}

#[test]
fn local_only_voice_route_skips_xai_stream_target() {
    use grokhub_agent::route::voice_route::{self, VoiceRouteConfig, VoiceRouteMode};
    let dir = temp_dir("local-only-target");
    voice_route::write(
        &dir,
        VoiceRouteConfig {
            mode: VoiceRouteMode::LocalOnly,
        },
    )
    .expect("write route");
    if ws_loopback_override(std::env::var(TEST_WS_ENV).ok().as_deref()).is_none() {
        assert!(target_for(&dir, "").is_none());
    }
}
