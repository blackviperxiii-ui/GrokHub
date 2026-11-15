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
    let mut live = Live::new(None);
    let mut audio = hush(500);
    audio.extend(tone(1500));
    audio.extend(hush(2000));
    let events = send(&mut live, &audio);
    assert_eq!(events.first(), Some(&LiveEvent::Started), "{events:?}");
    let Some(LiveEvent::Turn { ms }) = events.last() else {
        panic!("a turn: {events:?}");
    };
    assert!(*ms >= 1500, "{ms}");
    let Some(Pending::Audio {
        samples,
        news: None,
    }) = live.take_turn()
    else {
        panic!("the turn's audio, with no stream");
    };
    let wav = wav(&samples, RATE);
    assert_eq!(&wav[0..4], b"RIFF");
    assert_eq!(u32::from_le_bytes(wav[24..28].try_into().unwrap()), 16_000);
    assert!(live.take_turn().is_none(), "once only");
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
    let mut live = Live::new(None);
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
    assert_eq!(
        serde_json::to_value(LiveEvent::Caption { text: "hi".into() }).unwrap(),
        serde_json::json!({"kind": "caption", "text": "hi"})
    );
}

#[test]
fn a_reply_streams_into_pieces_and_a_new_one_starts_after() {
    let mut live = Live::new(None);
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
    let mut live = Live::new(None);
    assert_eq!(
        live.reply_finish("THINKING:\nplan the night\n\nI'll look."),
        vec!["I'll look.".to_string()]
    );
}

#[test]
fn a_hushed_reply_speaks_no_more() {
    let mut live = Live::new(None);
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
    assert_eq!(hear(None, |_| panic!("no upload")), Ok(String::new()));
}

/// A session whose every listen gets a fresh test stream: the test holds
/// each stream's audio side and news sender, in order.
struct Streams {
    audio: Vec<Receiver<Vec<i16>>>,
    news: Vec<std::sync::mpsc::Sender<SttNews>>,
}

fn streaming() -> (Live, std::sync::Arc<std::sync::Mutex<Streams>>) {
    let streams = std::sync::Arc::new(std::sync::Mutex::new(Streams {
        audio: Vec::new(),
        news: Vec::new(),
    }));
    let held = streams.clone();
    let live = Live::new(Some(Box::new(move || {
        let (audio_tx, audio_rx) = std::sync::mpsc::channel();
        let (news_tx, news_rx) = std::sync::mpsc::channel();
        let mut s = held.lock().unwrap();
        s.audio.push(audio_rx);
        s.news.push(news_tx);
        Some(SttLink {
            audio: audio_tx,
            news: news_rx,
        })
    })));
    (live, streams)
}

#[test]
fn every_block_heard_while_listening_goes_to_the_stream() {
    let (mut live, streams) = streaming();
    let block = tone(100);
    live.push(&bytes(&block)).expect("block");
    let got = streams.lock().unwrap().audio[0]
        .try_recv()
        .expect("a block");
    assert_eq!(got, block);
}

#[test]
fn smart_turn_ends_the_turn_early_with_its_words() {
    let (mut live, streams) = streaming();
    let _ = send(&mut live, &tone(1500));
    streams.lock().unwrap().news[0]
        .send(SttNews::Ended("Remind me to call Sam.".into()))
        .expect("news");
    assert_eq!(
        send(&mut live, &tone(100)),
        vec![LiveEvent::Turn { ms: 1600 }]
    );
    let turn = live.take_turn();
    assert!(
        matches!(&turn, Some(Pending::Words(w)) if w == "Remind me to call Sam."),
        "{turn:?}"
    );
    // The mic stopped: more talk is no turn until the page listens again.
    assert_eq!(send(&mut live, &tone(1500)), vec![]);
    assert_eq!(
        hear(turn, |_| panic!("no upload")),
        Ok("Remind me to call Sam.".to_string())
    );
}

#[test]
fn when_the_mic_ends_the_turn_the_streamed_words_are_used() {
    let (mut live, streams) = streaming();
    let mut audio = tone(1500);
    audio.extend(hush(2000));
    assert!(matches!(
        send(&mut live, &audio).last(),
        Some(LiveEvent::Turn { .. })
    ));
    // The stream's audio side is closed: it asks for the last words.
    let s = streams.lock().unwrap();
    while s.audio[0].try_recv().is_ok() {}
    assert_eq!(s.audio[0].try_recv(), Err(TryRecvError::Disconnected));
    let news = s.news[0].clone();
    drop(s);
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(30));
        let _ = news.send(SttNews::Caption("buy".into()));
        let _ = news.send(SttNews::Ended("buy milk".into()));
    });
    assert_eq!(
        hear(live.take_turn(), |_| panic!("no upload")),
        Ok("buy milk".to_string())
    );
}

#[test]
fn a_failed_stream_falls_back_to_one_upload() {
    let (mut live, streams) = streaming();
    let _ = send(&mut live, &tone(300));
    streams.lock().unwrap().news[0]
        .send(SttNews::Failed("speech stream closed".into()))
        .expect("news");
    let mut audio = tone(1200);
    audio.extend(hush(2000));
    assert!(matches!(
        send(&mut live, &audio).last(),
        Some(LiveEvent::Turn { .. })
    ));
    let mut uploads = 0;
    let heard = hear(live.take_turn(), |wav| {
        uploads += 1;
        assert_eq!(&wav[0..4], b"RIFF");
        Ok("open the build log".into())
    });
    assert_eq!(heard, Ok("open the build log".to_string()));
    assert_eq!(uploads, 1);
}

#[test]
fn a_silent_stream_falls_back_after_the_wait() {
    let (mut live, streams) = streaming();
    let mut audio = tone(1200);
    audio.extend(hush(2000));
    let _ = send(&mut live, &audio);
    // The news side stays open and says nothing.
    let _keep = streams.lock().unwrap().news[0].clone();
    let started = Instant::now();
    let mut uploads = 0;
    let heard = hear(live.take_turn(), |_| {
        uploads += 1;
        Ok("turn on the lights".into())
    });
    assert_eq!(heard, Ok("turn on the lights".to_string()));
    assert_eq!(uploads, 1);
    assert!(started.elapsed() >= crate::stt_stream::HEAR_WAIT);
}

#[test]
fn noise_alone_listens_again() {
    let (mut live, streams) = streaming();
    let mut audio = tone(1200);
    audio.extend(hush(2000));
    let _ = send(&mut live, &audio);
    streams.lock().unwrap().news[0]
        .send(SttNews::Ended(String::new()))
        .expect("news");
    assert_eq!(
        hear(live.take_turn(), |_| panic!("no upload")),
        Ok(String::new())
    );
}

#[test]
fn empty_words_mid_listen_do_not_end_the_turn() {
    let (mut live, streams) = streaming();
    streams.lock().unwrap().news[0]
        .send(SttNews::Ended(String::new()))
        .expect("news");
    let events = send(&mut live, &tone(300));
    assert!(
        !events.iter().any(|e| matches!(e, LiveEvent::Turn { .. })),
        "{events:?}"
    );
    assert!(live.take_turn().is_none());
    // The stream still runs: the next block reaches it.
    let s = streams.lock().unwrap();
    while s.audio[0].try_recv().is_ok() {}
    drop(s);
    live.push(&bytes(&tone(100))).expect("block");
    assert!(streams.lock().unwrap().audio[0].try_recv().is_ok());
}

#[test]
fn listening_over_a_reply_opens_no_stream() {
    let (mut live, streams) = streaming();
    assert_eq!(streams.lock().unwrap().audio.len(), 1);
    live.listen(true);
    assert_eq!(streams.lock().unwrap().audio.len(), 1);
    // The stream from before went with the old listen.
    assert_eq!(
        streams.lock().unwrap().audio[0].try_recv(),
        Err(TryRecvError::Disconnected)
    );
    live.listen(false);
    assert_eq!(streams.lock().unwrap().audio.len(), 2);
}

#[test]
fn captions_reach_the_page_while_they_talk() {
    let (mut live, streams) = streaming();
    {
        let s = streams.lock().unwrap();
        s.news[0]
            .send(SttNews::Caption("remind me".into()))
            .expect("news");
        s.news[0]
            .send(SttNews::Caption("remind me to call".into()))
            .expect("news");
    }
    // The newest caption only, once.
    assert_eq!(
        live.push(&bytes(&hush(100))).expect("block"),
        vec![LiveEvent::Caption {
            text: "remind me to call".into()
        }]
    );
    assert_eq!(live.push(&bytes(&hush(100))).expect("block"), vec![]);
}

#[test]
fn a_caption_before_smart_turn_ends_it_gives_way_to_the_turn() {
    let (mut live, streams) = streaming();
    let _ = send(&mut live, &tone(1500));
    {
        let s = streams.lock().unwrap();
        s.news[0]
            .send(SttNews::Caption("buy".into()))
            .expect("news");
        s.news[0]
            .send(SttNews::Ended("Buy milk.".into()))
            .expect("news");
    }
    assert_eq!(
        live.push(&bytes(&tone(100))).expect("block"),
        vec![LiveEvent::Turn { ms: 1600 }]
    );
}
