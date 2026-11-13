use super::*;
use crate::voice_turn::{AUTO_PAUSE_MS, RATE};

/// Speech: a 220 Hz tone with a syllable wobble.
fn tone(ms: u32, amp: f32) -> Vec<i16> {
    let n = (RATE * ms / 1000) as usize;
    (0..n)
        .map(|i| {
            let t = i as f32 / RATE as f32;
            let env = 0.08 + 0.92 * (2.0 * std::f32::consts::PI * 2.0 * t).sin().abs();
            (amp * env * (2.0 * std::f32::consts::PI * 220.0 * t).sin()) as i16
        })
        .collect()
}

/// A quiet room.
fn hush(ms: u32) -> Vec<i16> {
    let n = (RATE * ms / 1000) as usize;
    (0..n).map(|i| ((i * 7919 % 61) as i16) - 30).collect()
}

/// The reply's own echo: a steady tone (900 is about -34 dBFS).
fn steady(ms: u32, amp: f32) -> Vec<i16> {
    let n = (RATE * ms / 1000) as usize;
    (0..n)
        .map(|i| {
            let t = i as f32 / RATE as f32;
            (amp * (2.0 * std::f32::consts::PI * 180.0 * t).sin()) as i16
        })
        .collect()
}

/// Feeds `audio` in 100 ms blocks, as the shell sends them.
fn feed(mic: &mut OpenMic, audio: &[i16]) -> Vec<Heard> {
    audio.chunks(1600).flat_map(|b| mic.push(b)).collect()
}

fn ms(turn: &[i16]) -> u32 {
    turn.len() as u32 * 1000 / RATE
}

#[test]
fn a_long_turn_arrives_whole_and_the_listen_ends() {
    let mut mic = OpenMic::new();
    mic.listen();
    let mut audio = hush(1000);
    audio.extend(tone(7000, 9000.0));
    audio.extend(hush(2000));
    let heard = feed(&mut mic, &audio);
    assert_eq!(heard.first(), Some(&Heard::Started), "{heard:?}");
    let Some(Heard::Turn(turn)) = heard.last() else {
        panic!("a turn: {heard:?}");
    };
    assert!(
        ms(turn) >= 7000,
        "a 7 s turn is kept whole: {} ms",
        ms(turn)
    );
    assert_eq!(heard.len(), 2, "{heard:?}");
    assert!(!mic.armed(), "one turn per listen");
}

#[test]
fn blocks_between_listens_are_dropped() {
    let mut mic = OpenMic::new();
    // Speaking while nobody listens (thinking) is never a turn.
    assert_eq!(feed(&mut mic, &tone(3000, 9000.0)), vec![]);
    mic.listen();
    assert_eq!(feed(&mut mic, &hush(1500)), vec![]);
    assert!(!mic.talking());
}

#[test]
fn a_second_turn_needs_a_second_listen() {
    let mut mic = OpenMic::new();
    mic.listen();
    let mut turn = hush(500);
    turn.extend(tone(1500, 9000.0));
    turn.extend(hush(2000));
    assert!(matches!(feed(&mut mic, &turn).last(), Some(Heard::Turn(_))));
    assert_eq!(feed(&mut mic, &turn), vec![], "not armed");
    mic.listen();
    assert!(matches!(feed(&mut mic, &turn).last(), Some(Heard::Turn(_))));
}

#[test]
fn a_tap_mid_turn_keeps_what_was_said() {
    let mut mic = OpenMic::new();
    mic.listen();
    let mut audio = hush(800);
    audio.extend(tone(1500, 9000.0));
    let heard = feed(&mut mic, &audio);
    assert_eq!(heard, vec![Heard::Started]);
    assert!(mic.talking());
    mic.finish();
    let heard = feed(&mut mic, &tone(100, 9000.0));
    let [Heard::Turn(turn)] = heard.as_slice() else {
        panic!("the tap hands over the turn: {heard:?}");
    };
    assert!(ms(turn) >= 1500, "{} ms", ms(turn));
    assert!(!mic.talking());
    assert!(!mic.armed());
}

#[test]
fn a_tap_before_any_words_hears_nothing() {
    let mut mic = OpenMic::new();
    mic.listen();
    assert_eq!(feed(&mut mic, &hush(600)), vec![]);
    mic.finish();
    assert_eq!(feed(&mut mic, &hush(100)), vec![Heard::Nothing]);
    assert!(!mic.armed());
}

#[test]
fn a_quiet_minute_checks_in_then_pauses() {
    let mut mic = OpenMic::new();
    mic.listen();
    let heard = feed(&mut mic, &hush(AUTO_PAUSE_MS + 200));
    assert_eq!(heard, vec![Heard::CheckIn, Heard::Pause]);
    assert!(!mic.armed());
}

#[test]
fn the_reply_echo_is_dropped_and_talking_over_it_starts_a_turn() {
    let mut mic = OpenMic::new();
    mic.set_playing(true);
    mic.listen();
    assert_eq!(
        feed(&mut mic, &steady(2000, 900.0)),
        vec![],
        "the reply's echo is not a turn"
    );
    let heard = feed(&mut mic, &tone(900, 16000.0));
    assert_eq!(heard.first(), Some(&Heard::Started), "{heard:?}");
    assert!(mic.talking());
}

#[test]
fn a_long_reply_is_not_a_quiet_minute() {
    let mut mic = OpenMic::new();
    mic.set_playing(true);
    mic.listen();
    assert_eq!(feed(&mut mic, &steady(70_000, 900.0)), vec![]);
    assert!(mic.armed());
    // Once the reply ends, the quiet minute counts again.
    mic.set_playing(false);
    let heard = feed(&mut mic, &hush(AUTO_PAUSE_MS + 200));
    assert_eq!(heard, vec![Heard::CheckIn, Heard::Pause]);
}

#[test]
fn stop_drops_a_turn_under_way() {
    let mut mic = OpenMic::new();
    mic.listen();
    let mut audio = hush(500);
    audio.extend(tone(1000, 9000.0));
    assert_eq!(feed(&mut mic, &audio), vec![Heard::Started]);
    mic.stop();
    assert!(!mic.talking());
    assert_eq!(feed(&mut mic, &hush(2000)), vec![], "no turn after stop");
}

#[test]
fn the_first_sentence_is_ready_before_the_reply_finishes() {
    let mut reply = ReplyVoice::new();
    let mut spoken = Vec::new();
    for delta in [
        "Sure thing, the build ",
        "finished in two minutes. ",
        "Next I'll ",
        "run the tests.",
    ] {
        spoken.extend(reply.feed(delta));
    }
    assert!(reply.started());
    assert_eq!(
        spoken.first().map(String::as_str),
        Some("Sure thing, the build finished in two minutes."),
        "{spoken:?}"
    );
    spoken.extend(reply.finish("ignored once it streamed"));
    assert_eq!(
        spoken.join(" "),
        "Sure thing, the build finished in two minutes. Next I'll run the tests."
    );
}

#[test]
fn a_reply_that_did_not_stream_is_spoken_whole_in_order() {
    let mut reply = ReplyVoice::new();
    let spoken = reply.finish("First this. Then that. Done.");
    assert_eq!(spoken.join(" "), "First this. Then that. Done.");
    assert_eq!(reply.finish("again"), Vec::<String>::new(), "once only");
}

#[test]
fn markdown_and_code_are_not_read_aloud() {
    let mut reply = ReplyVoice::new();
    let mut spoken = reply.feed("Run **this** one:\n```sh\ncargo test\n```\nThat's it.\n");
    spoken.extend(reply.finish(""));
    let all = spoken.join(" ");
    assert!(!all.contains("**"), "{all}");
    assert!(!all.contains("cargo test"), "{all}");
    assert!(all.contains("That's it."), "{all}");
}

#[test]
fn a_huge_reply_is_cut_at_the_cap() {
    let mut reply = ReplyVoice::new();
    let sentence = "This is one sentence of the reply. ";
    let whole = sentence.repeat(SPOKEN_CAP / sentence.len() + 200);
    let spoken = reply.finish(&whole);
    let total: usize = spoken.iter().map(String::len).sum();
    assert!(total <= SPOKEN_CAP, "{total}");
    assert!(total > SPOKEN_CAP / 2, "{total}");
}

#[test]
fn a_hushed_reply_says_nothing_more() {
    let mut reply = ReplyVoice::new();
    assert_eq!(
        reply.feed("Here is the first sentence of it. And"),
        vec!["Here is the first sentence of it.".to_string()]
    );
    reply.hush();
    assert_eq!(reply.feed("And more after the tap. "), Vec::<String>::new());
    assert_eq!(reply.finish(""), Vec::<String>::new());
}
