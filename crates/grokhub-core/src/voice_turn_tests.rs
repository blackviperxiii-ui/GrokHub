use super::*;

/// Fixture audio: room noise around -66 dBFS, speech as a 220 Hz tone with a
/// 4 Hz syllable wobble around -18 dBFS, soft speech around -40 dBFS.
struct Tape {
    seed: u32,
    out: Vec<i16>,
}

impl Tape {
    fn new() -> Self {
        Self {
            seed: 7,
            out: Vec::new(),
        }
    }

    fn noise(&mut self) -> f64 {
        self.seed = self.seed.wrapping_mul(1_103_515_245).wrapping_add(12_345);
        f64::from((self.seed >> 16) & 0x7fff) / 32768.0 - 0.5
    }

    fn room(mut self, ms: u32) -> Self {
        for _ in 0..samples(ms) {
            let n = self.noise() * 30.0;
            self.out.push(n as i16);
        }
        self
    }

    fn talk_at(mut self, ms: u32, amp: f64) -> Self {
        let start = self.out.len();
        for i in 0..samples(ms) {
            let t = (start + i) as f64 / f64::from(RATE);
            // Syllables: four a second, dipping almost to nothing between them.
            let wobble = 0.08 + 0.92 * (2.0 * std::f64::consts::PI * 2.0 * t).sin().abs();
            // Each syllable a little louder or softer, as voices are.
            let syllable = (t * 4.0) as u32;
            let stress = 0.6 + 0.4 * f64::from(syllable.wrapping_mul(2_654_435_761) >> 28) / 15.0;
            let s = amp * stress * wobble * (2.0 * std::f64::consts::PI * 220.0 * t).sin();
            let n = self.noise() * 30.0;
            self.out.push((s + n) as i16);
        }
        self
    }

    fn talk(self, ms: u32) -> Self {
        self.talk_at(ms, 4000.0)
    }

    fn soft(self, ms: u32) -> Self {
        self.talk_at(ms, 330.0)
    }

    fn noise_at(mut self, ms: u32, amp: f64) -> Self {
        for _ in 0..samples(ms) {
            let n = self.noise() * amp;
            self.out.push(n as i16);
        }
        self
    }
}

/// Runs a tape through the detector in 100 ms blocks, as the mic hands them over.
fn run(tape: &[i16]) -> (Vec<TurnEvent>, TurnDetector) {
    let mut d = TurnDetector::new();
    let mut events = Vec::new();
    for block in tape.chunks(samples(100)) {
        events.extend(d.push(block));
    }
    (events, d)
}

fn turns(events: &[TurnEvent]) -> Vec<&Vec<i16>> {
    events
        .iter()
        .filter_map(|e| match e {
            TurnEvent::TurnEnded(t) => Some(t),
            _ => None,
        })
        .collect()
}

fn ms_of(n: usize) -> u32 {
    (n as u64 * 1000 / u64::from(RATE)) as u32
}

#[test]
fn a_ten_second_sentence_is_kept_whole_not_cut_at_four_seconds() {
    let tape = Tape::new().room(1000).talk(10_000).room(2000).out;
    let (events, _) = run(&tape);
    let t = turns(&events);
    assert_eq!(t.len(), 1);
    // 500 ms pre-roll + 10 s of speech + 250 ms hangover, within one frame.
    let ms = ms_of(t[0].len());
    assert!((10_700..=10_800).contains(&ms), "turn was {ms} ms");
    assert_eq!(events[0], TurnEvent::SpeechStarted);
}

#[test]
fn a_pause_mid_sentence_does_not_split_the_turn() {
    // "I want to... [0.8 s] ...book a table": one turn.
    let tape = Tape::new()
        .room(600)
        .talk(2000)
        .room(800)
        .talk(1500)
        .room(2000)
        .out;
    let (events, _) = run(&tape);
    let t = turns(&events);
    assert_eq!(t.len(), 1);
    let ms = ms_of(t[0].len());
    assert!((5_000..=5_100).contains(&ms), "turn was {ms} ms");
}

#[test]
fn a_short_question_waits_longer_before_ending() {
    // "Uh, what's..." 1 s of speech then a 1.2 s think: not over yet.
    let tape = Tape::new()
        .room(600)
        .talk(1000)
        .room(1200)
        .talk(800)
        .room(2000)
        .out;
    let (events, _) = run(&tape);
    assert_eq!(turns(&events).len(), 1);
}

#[test]
fn a_finished_long_sentence_ends_after_900_ms_of_quiet() {
    let tape = Tape::new().room(600).talk(3000).room(880).out;
    let (events, d) = run(&tape);
    assert!(turns(&events).is_empty(), "ended before 900 ms");
    assert!(d.talking());
    let tape = Tape::new().room(600).talk(3000).room(940).out;
    let (events, _) = run(&tape);
    assert_eq!(turns(&events).len(), 1);
}

#[test]
fn a_soft_start_is_in_the_pre_roll() {
    // A quiet first word, then normal speech: the turn starts before the quiet word.
    let tape = Tape::new().room(1000).soft(300).talk(2000).room(2000).out;
    let (events, _) = run(&tape);
    let t = turns(&events);
    assert_eq!(t.len(), 1);
    // Pre-roll before the soft word, the soft word, the speech and the hangover.
    let ms = ms_of(t[0].len());
    assert!((2_740..=3_100).contains(&ms), "turn was {ms} ms");
    let soft_start = samples(1000);
    let head = &tape[soft_start..soft_start + samples(300)];
    assert!(
        t[0].windows(head.len()).any(|w| w == head),
        "the soft first word was dropped"
    );
}

#[test]
fn the_trailing_word_and_hangover_are_kept() {
    let tape = Tape::new().room(600).talk(2500).soft(200).room(2000).out;
    let (events, _) = run(&tape);
    let t = turns(&events);
    assert_eq!(t.len(), 1);
    let tail_start = samples(600 + 2500);
    let tail = &tape[tail_start..tail_start + samples(200)];
    assert!(t[0].windows(tail.len()).any(|w| w == tail));
}

#[test]
fn room_noise_and_a_click_never_start_a_turn() {
    let tape = Tape::new()
        .room(3000)
        .noise_at(100, 20_000.0)
        .room(3000)
        .out;
    let (events, d) = run(&tape);
    assert!(turns(&events).is_empty());
    assert!(!events.contains(&TurnEvent::SpeechStarted));
    assert!(!d.talking());
}

#[test]
fn a_fan_coming_on_becomes_the_floor() {
    // Steady -40 dBFS hiss for 8 s: the floor climbs to it and no turn starts,
    // then speech over the fan still counts.
    let tape = Tape::new().room(500).noise_at(8000, 1200.0).out;
    let (events, _) = run(&tape);
    assert!(turns(&events).is_empty(), "the fan was a turn");
    let mut tape = tape;
    let speech = Tape::new().talk(2000).out;
    let fan = Tape::new().noise_at(2000 + 2000, 1200.0).out;
    for (i, f) in fan.iter().enumerate() {
        let s = speech.get(i).copied().unwrap_or(0);
        tape.push(s.saturating_add(*f));
    }
    let (events, _) = run(&tape);
    assert_eq!(turns(&events).len(), 1);
}

#[test]
fn a_minute_long_turn_ends_at_the_cap() {
    let tape = Tape::new().room(500).talk(61_000).out;
    let (events, _) = run(&tape);
    let t = turns(&events);
    assert_eq!(t.len(), 1);
    let ms = ms_of(t[0].len());
    assert!((60_000..=60_600).contains(&ms), "turn was {ms} ms");
}

#[test]
fn quiet_checks_in_at_nine_seconds_and_pauses_at_a_minute() {
    let tape = Tape::new().room(8_900).out;
    let (events, _) = run(&tape);
    assert!(events.is_empty());
    let tape = Tape::new().room(9_100).out;
    let (events, _) = run(&tape);
    assert_eq!(events, vec![TurnEvent::CheckIn]);
    let tape = Tape::new().room(60_100).out;
    let (events, _) = run(&tape);
    assert_eq!(events, vec![TurnEvent::CheckIn, TurnEvent::AutoPause]);
}

#[test]
fn a_turn_resets_the_quiet_clock() {
    let tape = Tape::new().room(6_000).talk(1_000).room(6_000).out;
    let (events, _) = run(&tape);
    assert!(!events.contains(&TurnEvent::CheckIn));
}

#[test]
fn tapping_to_finish_hands_back_what_was_said_or_nothing() {
    let mut d = TurnDetector::new();
    d.push(&Tape::new().room(500).talk(1_500).out);
    let t = d.finish_now().expect("words");
    assert!(ms_of(t.len()) >= 1_900);
    assert!(!d.talking());
    let mut d = TurnDetector::new();
    d.push(&Tape::new().room(2_000).out);
    assert_eq!(d.finish_now(), None);
}

#[test]
fn over_a_reply_only_sustained_speech_barges_in() {
    let mut d = TurnDetector::new();
    d.set_playing(true);
    // The reply's echo: steady sound around -36 dBFS for 2 s raises the floor.
    let mut events = d.push(&Tape::new().room(300).noise_at(2_000, 1_800.0).out);
    // A 300 ms cough-level burst over it is not a barge-in.
    events.extend(
        d.push(
            &Tape::new()
                .talk_at(300, 16_000.0)
                .noise_at(1_000, 1_800.0)
                .out,
        ),
    );
    assert!(!events.contains(&TurnEvent::SpeechStarted), "{events:?}");
    // Half a second of real speech over it is.
    let events = d.push(&Tape::new().talk_at(600, 16_000.0).out);
    assert_eq!(events, vec![TurnEvent::SpeechStarted]);
}

#[test]
fn audio_helpers_downmix_resample_and_wrap_a_wav() {
    // 48 kHz stereo, left 1000 right 3000: 16 kHz mono 2000, a third as long.
    let stereo: Vec<i16> = (0..4800).flat_map(|_| [1000i16, 3000]).collect();
    let mono = to_voice_rate(&stereo, 2, 48_000);
    assert_eq!(mono.len(), 1600);
    assert!(mono.iter().all(|&s| s == 2000));
    assert_eq!(to_voice_rate(&[1, 2, 3], 1, 16_000), vec![1, 2, 3]);
    let w = wav(&[1, -1], RATE);
    assert_eq!(&w[..4], b"RIFF");
    assert_eq!(w.len(), 48);
    assert_eq!(u32::from_le_bytes([w[24], w[25], w[26], w[27]]), 16_000);
    assert_eq!(s16le(&w[44..]), vec![1, -1]);
    assert_eq!(dbfs(&[0; 320]), -120.0);
    assert!((dbfs(&[16384; 320]) + 6.02).abs() < 0.01);
}

fn chunks(parts: &[&str]) -> Vec<String> {
    let mut c = SpeechChunker::new();
    let mut out = Vec::new();
    for p in parts {
        out.extend(c.push(p));
    }
    out.extend(c.finish());
    out
}

#[test]
fn the_first_piece_goes_at_the_first_clause_with_six_words() {
    let mut c = SpeechChunker::new();
    assert!(
        c.push("Sure, ").is_empty(),
        "two words is too short to speak alone"
    );
    let first = c.push("the forecast for tomorrow looks sunny, with a high of 72.");
    assert_eq!(
        first,
        vec!["Sure, the forecast for tomorrow looks sunny,".to_string()]
    );
    assert!(c.started());
    let rest = c.push(" Bring a hat. It");
    assert_eq!(
        rest,
        vec!["with a high of 72.".to_string(), "Bring a hat.".to_string()]
    );
    assert_eq!(c.finish().as_deref(), Some("It"));
}

#[test]
fn later_pieces_are_whole_sentences_streamed_in_any_split() {
    let text =
        "It's 3.5 miles away, about ten minutes by car. Dr. Lee opens at nine! Want directions?";
    let whole = chunks(&[text]);
    assert_eq!(
        whole,
        vec![
            "It's 3.5 miles away, about ten minutes by car.",
            "Dr. Lee opens at nine!",
            "Want directions?",
        ]
    );
    let split: Vec<String> = text.chars().map(|c| c.to_string()).collect();
    let refs: Vec<&str> = split.iter().map(String::as_str).collect();
    assert_eq!(chunks(&refs), whole);
}

#[test]
fn markdown_is_read_as_speech_and_code_is_skipped() {
    let out = chunks(&[
        "## Your plan\n",
        "- **Monday**: rest\n",
        "1. Check [the guide](https://example.com/g) first\n",
        "```sh\nrm -rf build\n```\n",
        "See https://example.com for more.",
    ]);
    assert_eq!(
        out,
        vec![
            "Your plan.",
            "Monday: rest.",
            "Check the guide first.",
            "See a link for more.",
        ]
    );
}

#[test]
fn a_long_run_on_is_split_at_a_comma_before_the_cap() {
    let long = format!(
        "{}, and then {}",
        "word ".repeat(30).trim(),
        "more ".repeat(30)
    );
    let out = chunks(&[&long]);
    assert!(out.len() >= 2);
    assert!(out.iter().all(|p| p.len() <= MAX_CHUNK_CHARS));
    assert!(out[0].ends_with(','));
}

#[test]
fn timings_print_one_line_and_percentiles() {
    let t = VoiceTimings {
        turn_ms: Some(900),
        transcript_ms: Some(1400),
        first_token_ms: None,
        first_audio_ms: Some(2700),
    };
    assert_eq!(
        t.line(),
        "voice turn: end 900 ms, words 1400, first token -, first audio 2700"
    );
    let turns: Vec<VoiceTimings> = (1..=20)
        .map(|i| VoiceTimings {
            first_audio_ms: Some(i * 100),
            ..Default::default()
        })
        .collect();
    let p = timing_percentiles(&turns);
    assert_eq!(p[0], (None, None));
    assert_eq!(p[3], (Some(1100), Some(1900)));
}

#[test]
fn every_mic_recorder_streams_raw_16k_mono_without_a_time_limit() {
    for bin in MIC_RECORDERS {
        let argv = mic_argv(bin).expect("every listed recorder has argv");
        let joined = argv.join(" ");
        assert!(
            joined.contains("16000"),
            "{bin} must ask for 16 kHz: {joined}"
        );
        assert!(
            !argv.contains(&"-d") && !argv.contains(&"-t ") && !joined.contains("trim"),
            "{bin} must stream until stopped: {joined}"
        );
    }
    assert!(mic_argv("ffmpeg").unwrap().contains(&"s16le"));
    assert!(mic_argv("arecord").unwrap().contains(&"raw"));
    assert!(mic_argv("whisper").is_none());
}

#[test]
fn thoughts_and_commands_never_reach_the_speaker_in_any_split() {
    let text = "THINKING:\nCheck the weather tool first, then answer briefly.\n\n\
                I'll look that up for you.\n\
                HOST_CMD: cat > /tmp/w.txt <<EOF\nsecret plan, do not say\nEOF\n\
                <think>quiet thought, hidden too</think>\n\
                It's sunny, with a high of 72.";
    let whole = chunks(&[text]);
    assert_eq!(
        whole,
        vec![
            "I'll look that up for you.",
            "It's sunny, with a high of 72."
        ]
    );
    let split: Vec<String> = text.chars().map(|c| c.to_string()).collect();
    let refs: Vec<&str> = split.iter().map(String::as_str).collect();
    assert_eq!(
        chunks(&refs),
        whole,
        "a partial line is held until it is known to be prose"
    );
}

#[test]
fn a_reply_opening_in_capitals_is_still_spoken() {
    assert_eq!(
        chunks(&["OK, sure. NASA says yes."]),
        vec!["OK, sure.", "NASA says yes."]
    );
}
