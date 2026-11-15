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
    /// Each stream's "the server is ready" flag; ready from the start.
    ready: Vec<std::sync::Arc<std::sync::atomic::AtomicBool>>,
}

fn streaming() -> (Live, std::sync::Arc<std::sync::Mutex<Streams>>) {
    let streams = std::sync::Arc::new(std::sync::Mutex::new(Streams {
        audio: Vec::new(),
        news: Vec::new(),
        ready: Vec::new(),
    }));
    let held = streams.clone();
    let live = Live::new(Some(Box::new(move || {
        let (audio_tx, audio_rx) = std::sync::mpsc::channel();
        let (news_tx, news_rx) = std::sync::mpsc::channel();
        let mut s = held.lock().unwrap();
        let ready = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
        s.audio.push(audio_rx);
        s.news.push(news_tx);
        s.ready.push(ready.clone());
        Some(SttLink {
            audio: audio_tx,
            news: news_rx,
            ready,
        })
    })));
    (live, streams)
}

/// The reply started or stopped, as the page's command does it: the stream
/// opens with the session let go.
fn playing(live: &mut Live, now: bool) {
    let cell = Mutex::new(Some(std::mem::replace(live, Live::new(None))));
    then_open(&cell, |live| live.playing_changed(now)).expect("voice mode on");
    *live = cell.into_inner().unwrap().expect("still on");
}

/// Every block the stream got so far.
fn streamed(streams: &std::sync::Mutex<Streams>, i: usize) -> Vec<Vec<i16>> {
    let s = streams.lock().unwrap();
    std::iter::from_fn(|| s.audio[i].try_recv().ok()).collect()
}

#[test]
fn room_sound_stays_here_until_someone_starts_then_the_lead_in_goes_first() {
    let (mut live, streams) = streaming();
    let quiet = hush(1500);
    let _ = send(&mut live, &quiet);
    assert_eq!(
        streamed(&streams, 0),
        Vec::<Vec<i16>>::new(),
        "nothing before a start"
    );
    // Every block the mic got, in order.
    let mut sent: Vec<Vec<i16>> = quiet.chunks(1600).map(<[i16]>::to_vec).collect();
    let talk = tone(1500);
    let mut first = Vec::new();
    for block in talk.chunks(1600) {
        sent.push(block.to_vec());
        let heard = live.push(&bytes(block)).expect("block");
        if heard.contains(&LiveEvent::Started) {
            first = streamed(&streams, 0);
            // The last 1.1 s, this block included: 11 blocks of 100 ms, the
            // room sound before the start first.
            assert_eq!(first, sent[sent.len() - 11..].to_vec());
            assert_ne!(first[0], first[1], "each block is its own");
            break;
        }
    }
    assert!(!first.is_empty(), "the mic heard a start");
    // From then on, each block as it comes.
    let more = tone(100);
    live.push(&bytes(&more)).expect("block");
    assert_eq!(streamed(&streams, 0), vec![more]);
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
    // The mic stopped: a whole spoken turn after it is no turn until the
    // page listens again.
    let mut more = hush(500);
    more.extend(tone(1500));
    more.extend(hush(2000));
    assert_eq!(send(&mut live, &more), vec![]);
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
    // Captions on the way are skipped for the words.
    s.news[0]
        .send(SttNews::Caption("buy".into()))
        .expect("news");
    s.news[0]
        .send(SttNews::Ended("buy milk".into()))
        .expect("news");
    drop(s);
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
fn a_turn_over_before_its_stream_got_ready_uploads_at_once() {
    let (mut live, streams) = streaming();
    // Still connecting: the server hasn't said it is ready.
    streams.lock().unwrap().ready[0].store(false, std::sync::atomic::Ordering::SeqCst);
    let mut audio = tone(1200);
    audio.extend(hush(2000));
    let _ = send(&mut live, &audio);
    let _keep = streams.lock().unwrap().news[0].clone();
    let started = Instant::now();
    let heard = hear(live.take_turn(), |_| Ok("turn on the lights".into()));
    assert_eq!(heard, Ok("turn on the lights".to_string()));
    assert!(
        started.elapsed() < crate::stt_stream::HEAR_WAIT,
        "no wait for a stream that has no words"
    );
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
    assert!(send(&mut live, &tone(1500)).contains(&LiveEvent::Started));
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

#[test]
fn a_stream_that_already_ended_still_hands_over_its_words() {
    let (mut live, streams) = streaming();
    let _ = send(&mut live, &tone(1500));
    {
        // Smart Turn ended it and the stream's thread went away.
        let mut s = streams.lock().unwrap();
        s.news[0]
            .send(SttNews::Ended("Buy milk.".into()))
            .expect("news");
        s.news.clear();
        s.audio.clear();
    }
    assert_eq!(
        send(&mut live, &tone(100)),
        vec![LiveEvent::Turn { ms: 1600 }]
    );
    assert!(
        matches!(live.take_turn(), Some(Pending::Words(w)) if w == "Buy milk."),
        "the words came through"
    );
}

#[test]
fn the_turn_after_a_spoken_reply_gets_a_stream() {
    let (mut live, streams) = streaming();
    live.listen(true);
    assert_eq!(streams.lock().unwrap().audio.len(), 1);
    playing(&mut live, false);
    assert_eq!(streams.lock().unwrap().audio.len(), 2);
    // Already open: a second stop opens no other.
    playing(&mut live, false);
    assert_eq!(streams.lock().unwrap().audio.len(), 2);
    assert!(send(&mut live, &tone(1000)).contains(&LiveEvent::Started));
    assert!(!streamed(&streams, 1).is_empty(), "their talk goes to it");
    // A reply playing again drops it.
    playing(&mut live, true);
    assert_eq!(
        streams.lock().unwrap().audio[1].try_recv(),
        Err(TryRecvError::Disconnected)
    );
}

#[test]
fn talking_over_a_reply_keeps_the_upload_for_that_turn() {
    let (mut live, streams) = streaming();
    live.listen(true);
    // A quiet reply, then they talk over it, so it stops mid-turn.
    let _ = send(&mut live, &hush(700));
    assert_eq!(send(&mut live, &tone(1500)), vec![LiveEvent::Started]);
    assert!(live.mic.talking(), "mid-turn");
    playing(&mut live, false);
    assert_eq!(streams.lock().unwrap().audio.len(), 1, "no stream mid-turn");
}

#[test]
fn a_caption_goes_when_its_stream_drops_mid_listen() {
    let (mut live, streams) = streaming();
    assert!(send(&mut live, &tone(1500)).contains(&LiveEvent::Started));
    streams.lock().unwrap().news[0]
        .send(SttNews::Caption("remind me".into()))
        .expect("news");
    assert_eq!(
        live.push(&bytes(&hush(100))).expect("block"),
        vec![LiveEvent::Caption {
            text: "remind me".into()
        }]
    );
    streams.lock().unwrap().news[0]
        .send(SttNews::Failed("speech stream: closed".into()))
        .expect("news");
    assert_eq!(
        live.push(&bytes(&hush(100))).expect("block"),
        vec![LiveEvent::Caption {
            text: String::new()
        }]
    );
    assert_eq!(live.push(&bytes(&hush(100))).expect("block"), vec![]);
}

#[test]
fn a_stream_that_drops_before_any_caption_says_nothing() {
    let (mut live, streams) = streaming();
    streams.lock().unwrap().news.clear();
    assert_eq!(live.push(&bytes(&hush(100))).expect("block"), vec![]);
}

#[test]
fn smart_turn_words_without_a_start_the_mic_heard_are_not_a_turn() {
    let (mut live, streams) = streaming();
    // Only room noise reached the mic, yet the stream heard words.
    let _ = send(&mut live, &hush(500));
    streams.lock().unwrap().news[0]
        .send(SttNews::Ended("on the TV".into()))
        .expect("news");
    assert_eq!(send(&mut live, &hush(100)), vec![]);
    assert!(live.take_turn().is_none(), "nothing to hear");
    // The stream is dropped, so their real turn uploads.
    let s = streams.lock().unwrap();
    while s.audio[0].try_recv().is_ok() {}
    assert_eq!(s.audio[0].try_recv(), Err(TryRecvError::Disconnected));
}

#[test]
fn a_start_just_before_a_reply_ends_still_reaches_the_stream() {
    let (mut live, streams) = streaming();
    live.listen(true);
    // The reply plays quietly, and they start under the barge-in bar.
    let _ = send(&mut live, &hush(700));
    let lead = tone(300);
    assert_eq!(send(&mut live, &lead), vec![]);
    playing(&mut live, false);
    assert_eq!(
        streams.lock().unwrap().audio.len(),
        2,
        "a stream for the turn"
    );
    let mut rest = tone(1200);
    rest.extend(hush(2000));
    assert!(matches!(
        send(&mut live, &rest).last(),
        Some(LiveEvent::Turn { .. })
    ));
    // The stream got the start they made while the reply still played.
    let got: Vec<i16> = streamed(&streams, 1).concat();
    assert!(
        got.windows(lead.len()).any(|w| w == lead.as_slice()),
        "the lead-in reached the stream"
    );
}

static TARGETS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// A key for the first listen only, as if it was cleared after.
fn key_once() -> Option<crate::stt_stream::StreamTarget> {
    (TARGETS.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0).then(|| {
        crate::stt_stream::StreamTarget {
            url: "ws://127.0.0.1:9/v1/stt".into(),
            bearer: "sk-abcdefghijklmnopqrstuv".into(),
        }
    })
}

#[test]
fn each_listen_reads_the_key_as_it_is_then() {
    let mut live = Live::new(Some(opener(temp_dir("key-once"), key_once)));
    assert!(live.stt.is_some(), "the first listen streams");
    live.listen(false);
    assert!(live.stt.is_none(), "a cleared key opens nothing");
    assert_eq!(TARGETS.load(std::sync::atomic::Ordering::SeqCst), 2);
}

#[test]
fn a_listen_that_asks_while_a_stream_opens_gets_that_stream() {
    let (mut live, streams) = streaming();
    assert!(live.restart(false), "a listen wants a stream");
    let mut open = live.lend_opener().expect("the opener");
    // An empty turn: the page listens again while the stream is opening,
    // and finds the opener lent out.
    assert!(live.restart(false));
    assert!(live.lend_opener().is_none());
    let link = open();
    live.take_back(open, link);
    assert!(
        live.stt.is_some(),
        "nothing was sent to it: this listen has it"
    );
    assert!(live.open.is_some(), "the opener came back");
    assert_eq!(streams.lock().unwrap().audio.len(), 2);
}

#[test]
fn a_stream_that_opens_while_a_reply_plays_is_not_used() {
    let (mut live, _streams) = streaming();
    assert!(live.restart(false));
    let mut open = live.lend_opener().expect("the opener");
    assert!(!live.playing_changed(true));
    let link = open();
    live.take_back(open, link);
    assert!(live.stt.is_none(), "its echo could end a turn nobody spoke");
}

#[test]
fn a_stream_that_opens_after_a_start_is_not_used() {
    let (mut live, _streams) = streaming();
    assert!(live.restart(false));
    let mut open = live.lend_opener().expect("the opener");
    // They started talking while the key was being read.
    assert!(send(&mut live, &tone(1000)).contains(&LiveEvent::Started));
    let link = open();
    live.take_back(open, link);
    assert!(live.stt.is_none(), "it would have missed the start");
    assert!(live.open.is_some());
}

#[test]
fn a_start_already_over_still_keeps_a_late_stream_out() {
    let (mut live, _streams) = streaming();
    assert!(live.restart(false));
    let mut open = live.lend_opener().expect("the opener");
    // The mic heard a start that is no longer under way.
    live.spoke = true;
    assert!(!live.mic.talking());
    let link = open();
    live.take_back(open, link);
    assert!(live.stt.is_none());
}

#[test]
fn a_stream_that_waited_too_long_for_a_start_is_opened_again() {
    let (mut live, streams) = streaming();
    streams.lock().unwrap().news[0]
        .send(SttNews::Idle)
        .expect("news");
    assert_eq!(send(&mut live, &hush(100)), vec![]);
    assert!(live.stt.is_none());
    assert!(live.take_reopen(), "the listen still waits");
    assert!(!live.take_reopen(), "once");
    let cell = Mutex::new(Some(live));
    then_open(&cell, |live| live.wants_stream()).expect("on");
    let live = cell.into_inner().unwrap().expect("still on");
    assert!(live.stt.is_some());
    assert_eq!(streams.lock().unwrap().audio.len(), 2);
}

#[test]
fn a_stream_that_failed_is_not_opened_again_until_the_next_listen() {
    let (mut live, streams) = streaming();
    streams.lock().unwrap().news[0]
        .send(SttNews::Failed("speech stream closed".into()))
        .expect("news");
    assert_eq!(send(&mut live, &hush(100)), vec![]);
    assert!(live.stt.is_none());
    assert!(!live.take_reopen());
}

#[test]
fn no_caption_before_the_mic_hears_a_start() {
    let (mut live, streams) = streaming();
    // Someone else in the room, or the TV.
    streams.lock().unwrap().news[0]
        .send(SttNews::Caption("and in other news".into()))
        .expect("news");
    assert_eq!(live.push(&bytes(&hush(100))).expect("block"), vec![]);
}
