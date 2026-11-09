//! Hey Grok listen, PTT, and speak.

use super::*;

/// Shown when the open mic has heard nothing for a while.
const CHECK_IN_NOTE: &str = "Still listening. Talk when you're ready, or tap the mic to stop.";

/// Wait for the next turn from the open mic and turn it into words. Silence
/// keeps listening; an empty transcript (a cough, a door) listens again.
/// With a speech stream, live captions show while they talk, Smart Turn can
/// end the turn as soon as they've clearly finished, and the words are ready
/// when the mic's own detector ends it; the upload is the fallback.
pub(super) fn listen_turn(
    api_key: &str,
    heard: mpsc::Receiver<crate::voice_mic::Heard>,
    mut stt: Option<(
        mpsc::Receiver<crate::stt_stream::SttNews>,
        crate::voice_mic::MicFinisher,
    )>,
    notes: &mpsc::Sender<JobOut>,
) -> String {
    use crate::stt_stream::SttNews;
    use crate::voice_mic::Heard;
    let tick = std::time::Duration::from_millis(30);
    let mut noise_only = false;
    loop {
        if let Some((news, finisher)) = stt.as_ref() {
            match news.try_recv() {
                Ok(SttNews::Caption(c)) => {
                    let _ = notes.send(JobOut::VoiceNote(caption_note(&c)));
                }
                Ok(SttNews::Ended(words)) if !words.is_empty() => {
                    // Clearly finished: don't wait out the silence.
                    finisher.finish();
                    let _ = notes.send(JobOut::VoiceNote("Got it…".into()));
                    crate::spoken::note_turn(crate::spoken::TurnClock {
                        ended: std::time::Instant::now(),
                        words_ms: 0,
                    });
                    return words;
                }
                // Only noise, and the mic's own end is on its way.
                Ok(SttNews::Ended(_)) => noise_only = true,
                Ok(SttNews::Failed(_)) | Err(mpsc::TryRecvError::Disconnected) => stt = None,
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        let next = if stt.is_some() {
            match heard.recv_timeout(tick) {
                Ok(h) => h,
                Err(mpsc::RecvTimeoutError::Timeout) => continue,
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return "VOICE_RECEIPT: voice stopped".into();
                }
            }
        } else {
            match heard.recv() {
                Ok(h) => h,
                Err(_) => return "VOICE_RECEIPT: voice stopped".into(),
            }
        };
        match next {
            Heard::Started => {
                let _ = notes.send(JobOut::VoiceHeard);
            }
            Heard::CheckIn => {
                let _ = notes.send(JobOut::VoiceNote(CHECK_IN_NOTE.into()));
            }
            Heard::Turn(samples) => {
                let _ = notes.send(JobOut::VoiceNote("Got it…".into()));
                let ended = std::time::Instant::now();
                let streamed = if noise_only {
                    Some(String::new())
                } else {
                    stt.and_then(|(news, _)| final_words(&news))
                };
                let text = streamed.unwrap_or_else(|| heard_text(api_key, &samples));
                crate::spoken::note_turn(crate::spoken::TurnClock {
                    ended,
                    words_ms: ended.elapsed().as_millis() as u64,
                });
                return text;
            }
            Heard::Nothing => return String::new(),
            Heard::Pause => return VOICE_PAUSED.into(),
            Heard::Lost(e) => return format!("VOICE_RECEIPT: {e}"),
        }
    }
}

/// The stream's words once the mic has ended the turn, or `None` to upload.
fn final_words(news: &mpsc::Receiver<crate::stt_stream::SttNews>) -> Option<String> {
    use crate::stt_stream::SttNews;
    let by = std::time::Instant::now() + std::time::Duration::from_millis(2500);
    loop {
        let left = by.checked_duration_since(std::time::Instant::now())?;
        match news.recv_timeout(left) {
            Ok(SttNews::Ended(words)) => return Some(words),
            Ok(SttNews::Caption(_)) => {}
            Ok(SttNews::Failed(_)) | Err(_) => return None,
        }
    }
}

/// A live caption: the last few words, so the line never wraps.
fn caption_note(heard: &str) -> String {
    let chars: Vec<char> = heard.chars().collect();
    let tail: String = chars[chars.len().saturating_sub(80)..].iter().collect();
    let lead = if chars.len() > 80 { "…" } else { "" };
    format!("\u{201c}{lead}{}\u{201d}", tail.trim())
}

/// Words for one heard turn: xAI first, local whisper as the fallback.
/// Empty means nothing worth sending was said.
fn heard_text(api_key: &str, samples: &[i16]) -> String {
    let wav = grokhub_core::voice_turn::wav(samples, grokhub_core::voice_turn::RATE);
    if wav.len() as u64 > IMAGE_FILE_CAP {
        return "VOICE_RECEIPT: recording too large".into();
    }
    let has_local = first_bin(TRANSCRIBERS).is_some();
    let local = || -> Result<String, String> {
        let path = private_temp("voice.wav")?;
        std::fs::write(&path, &wav).map_err(|e| format!("voice file: {e}"))?;
        let text = transcribe_local(&path);
        let _ = std::fs::remove_file(&path);
        text
    };
    match transcribe_route(!api_key.trim().is_empty(), has_local) {
        TranscribeRoute::Xai => match grok_stt(api_key, &wav) {
            Ok(t) => t,
            Err(e) if e == "empty transcript" => String::new(),
            Err(e) if has_local => local()
                .map(|t| t.trim().to_string())
                .unwrap_or_else(|l| format!("VOICE_RECEIPT: {e}; {l}")),
            Err(e) => format!("VOICE_RECEIPT: {e}"),
        },
        TranscribeRoute::Local => match local() {
            Ok(t) => t.trim().to_string(),
            Err(e) => format!("VOICE_RECEIPT: {e}"),
        },
        TranscribeRoute::None => {
            "VOICE_RECEIPT: Connect Grok OAuth for STT, or install whisper".into()
        }
    }
}

impl Cabin {
    pub(super) fn listen_voice(&mut self) {
        if self.voice_is_on() {
            // Mid-sentence, a tap means "I'm done": send what was said.
            if let Some(mic) = self.voice_mic.as_ref().filter(|m| m.talking()) {
                mic.finish();
                self.status = "Got it…".into();
                return;
            }
            // While it talks, a tap means "stop, my turn".
            if self.voice_state == VoiceState::Speaking {
                self.barge_in();
                return;
            }
            self.leave_voice();
            return;
        }
        let action = hey_grok_on_press(self.voice_state, self.running);
        if action == HeyGrokAction::Halt {
            self.halt_work("Hands on — halted");
            return;
        }
        let speech = self.bearer();
        let has_local = first_bin(TRANSCRIBERS).is_some();
        let route = hey_grok_route(
            realtime_can_connect(self.console_key()),
            !speech.is_empty(),
            has_local,
        );
        match route {
            HeyGrokRoute::PushToTalk | HeyGrokRoute::Realtime => {}
            HeyGrokRoute::None => {
                self.status = "Connect Grok OAuth for STT/TTS.".into();
                return;
            }
        }
        self.start_ptt_listen();
    }

    pub(super) fn start_ptt_listen(&mut self) {
        if self.running {
            return;
        }
        let speech = self.bearer();
        // One open mic for the whole session: no first word lost to a device opening.
        let mic = self
            .voice_mic
            .get_or_insert_with(crate::voice_mic::VoiceMic::open);
        let (tap, stt) = match crate::stt_stream::open(&speech) {
            Some((tap, news)) => (Some(tap), Some((news, mic.finisher()))),
            None => (None, None),
        };
        // Over a reply still playing, the mic listens for someone talking over it.
        let over_reply = self.voice_hold_rx.is_some();
        mic.set_playing(over_reply);
        let heard = mic.listen(tap);
        self.running = true;
        self.abandon_turn_card();
        self.chat_job_thread = None;
        if over_reply {
            self.show_speaking();
        } else {
            self.show_listening();
        }
        let (tx, rx) = mpsc::channel();
        self.rx = Some(rx);
        std::thread::spawn(move || {
            let text = listen_turn(&speech, heard, stt, &tx);
            let _ = tx.send(JobOut::Voice(text));
        });
    }

    pub(super) fn maybe_continue_ptt(&mut self) {
        if !ptt_after_speak(self.voice_is_on()) {
            return;
        }
        if self.voice_hold_rx.is_some() || self.running {
            return;
        }
        self.start_ptt_listen();
    }

    pub(super) fn poll_voice_hold(&mut self) {
        self.poll_spoken();
        let Some(rx) = self.voice_hold_rx.take() else {
            return;
        };
        match rx.try_recv() {
            Ok(()) | Err(mpsc::TryRecvError::Disconnected) => {
                if let Some(mic) = &self.voice_mic {
                    mic.set_playing(false);
                }
                if self.running && self.voice_state == VoiceState::Speaking {
                    // Already listening over the reply: just say so.
                    self.show_listening();
                }
                self.maybe_continue_ptt();
            }
            Err(mpsc::TryRecvError::Empty) => {
                self.voice_hold_rx = Some(rx);
                if self.voice_is_on() && !self.running {
                    // Listen while it talks, so they can talk over it.
                    self.start_ptt_listen();
                }
            }
        }
    }

    fn show_listening(&mut self) {
        self.voice_orb = "listening".into();
        self.voice_state = VoiceState::Listening;
        self.status = voice_mode_label(VoiceState::Listening).into();
    }

    /// Stop the spoken reply now, and let the mic know it stopped.
    pub(super) fn stop_speaking(&mut self) {
        self.voice_spoken = None;
        self.voice_hold_rx = None;
        if let Some(mic) = &self.voice_mic {
            mic.set_playing(false);
        }
    }

    /// The mic heard someone start talking. Over a reply, they're talking
    /// over it: stop the reply so they can be heard.
    pub(super) fn heard_talking(&mut self) {
        if self.voice_state == VoiceState::Speaking {
            self.stop_speaking();
        }
        self.voice_orb = "listening".into();
        self.voice_state = VoiceState::Listening;
        self.status = "Listening… go on".into();
    }

    /// They want to talk while the reply is being spoken: stop it and listen.
    fn barge_in(&mut self) {
        if self.voice_hold_rx.is_some() {
            // The reply is written and playing: stop it and listen now.
            self.stop_speaking();
            if self.running {
                self.show_listening();
            } else {
                self.start_ptt_listen();
            }
            return;
        }
        // Still being written: say no more of it, and listen once it's done.
        if let Some(spoken) = self.voice_spoken.as_mut() {
            spoken.hush();
        }
        self.voice_state = VoiceState::Thinking;
        self.voice_orb = "idle".into();
        self.status = "Okay, I'll stop talking.".into();
    }

    pub(super) fn voice_is_on(&self) -> bool {
        self.voice_state != VoiceState::Idle
    }

    pub(super) fn leave_voice(&mut self) {
        self.voice_hold_rx = None;
        // Stops a reply mid-sentence.
        self.voice_spoken = None;
        self.voice_ready_at = None;
        // Closes the mic.
        self.voice_mic = None;
        let ptt = self.running && self.voice_state != VoiceState::Idle;
        self.voice_state = VoiceState::Idle;
        self.voice_orb = "idle".into();
        if ptt {
            self.halt_work("Voice off");
        } else {
            self.status = "Voice off".into();
        }
    }

    /// A new spoken reply in the chosen voice. Times count from the voice
    /// turn it answers, when there is one.
    fn new_spoken(&mut self) -> crate::spoken::SpokenReply {
        let turn = self.voice_is_on().then(crate::spoken::take_turn).flatten();
        let key = self.bearer();
        // Without a Grok sign-in, Kokoro speaks on this PC when it's downloaded.
        if key.is_empty() && crate::desktop::on_device_voice_ready() {
            return crate::spoken::SpokenReply::on_device(
                self.speech_voice(),
                self.lips.clone(),
                turn,
            );
        }
        crate::spoken::SpokenReply::xai(key, self.speech_voice(), self.lips.clone(), turn)
    }

    fn show_speaking(&mut self) {
        if self.voice_is_on() {
            self.voice_state = VoiceState::Speaking;
            self.voice_orb = "speaking".into();
            self.status = voice_mode_label(VoiceState::Speaking).into();
        }
    }

    /// Speak a reply's text while it streams, so the first sentence plays
    /// before the reply is done.
    pub(super) fn speak_stream(&mut self, added: &str) {
        if !self.speak_next || !self.stream_here() {
            return;
        }
        if !self.voice_spoken.as_ref().is_some_and(|s| s.is_open()) {
            self.voice_spoken = Some(self.new_spoken());
        }
        if self.voice_spoken.as_mut().is_some_and(|s| s.feed(added)) {
            self.show_speaking();
        }
    }

    /// The reply is complete: speak what has not been spoken yet, then
    /// listen again once the last piece has played.
    pub(super) fn speak_reply(&mut self, text: &str) {
        let text = voice_tts_script(text);
        let streamed = self.voice_spoken.take().filter(|s| s.is_open());
        if text.is_empty() && !streamed.as_ref().is_some_and(|s| s.started()) {
            self.maybe_continue_ptt();
            return;
        }
        self.show_speaking();
        let mut spoken = match streamed {
            Some(s) => s,
            None => self.new_spoken(),
        };
        // Capped inside: never more than TEXT_FILE_CAP goes to speech.
        spoken.finish(&text);
        if self.voice_is_on() {
            self.voice_hold_rx = spoken.take_done();
        }
        self.voice_spoken = Some(spoken);
    }

    /// A spoken reply's errors go to the status line; a finished one is
    /// let go and its timings kept.
    fn poll_spoken(&mut self) {
        let Some(spoken) = &self.voice_spoken else {
            return;
        };
        if let Some(e) = spoken.error() {
            let e: String = e.chars().take(120).collect();
            self.status = format!("Couldn't speak the reply: {e}");
        }
        if spoken.finished() {
            if let Some(t) = spoken.timings() {
                crate::spoken::log_timings(t);
            }
            self.voice_spoken = None;
        }
    }

    pub(super) fn paint_voice_mode_row(&mut self, ui: &mut egui::Ui) {
        if !self.voice_is_on() {
            return;
        }
        let ready_ms = match self.voice_state {
            VoiceState::Ready => {
                let at = self.voice_ready_at.get_or_insert(Instant::now());
                at.elapsed().as_millis() as u64
            }
            VoiceState::Listening
            | VoiceState::Thinking
            | VoiceState::Speaking
            | VoiceState::Hands => {
                self.voice_ready_at = None;
                0
            }
            VoiceState::Idle => 0,
        };
        if !voice_strip_visible(self.voice_state, ready_ms) {
            if self.voice_state == VoiceState::Ready {
                self.leave_voice();
            }
            return;
        }
        if crate::cards::voice_mode_row(ui, voice_mode_label(self.voice_state)) {
            self.leave_voice();
        }
    }

    pub(super) fn paint_voice_mic(&mut self, ui: &mut egui::Ui, size: f32) -> egui::Rect {
        let on = self.voice_is_on();
        let mood = match self.voice_state {
            VoiceState::Speaking => crate::icons::MicMood::Speaking,
            VoiceState::Thinking => crate::icons::MicMood::Thinking,
            VoiceState::Listening | VoiceState::Hands => crate::icons::MicMood::Live,
            VoiceState::Idle | VoiceState::Ready => {
                if on {
                    crate::icons::MicMood::Live
                } else {
                    crate::icons::MicMood::Idle
                }
            }
        };
        let tip = match self.voice_state {
            VoiceState::Listening if self.voice_mic.as_ref().is_some_and(|m| m.talking()) => {
                "Done talking"
            }
            VoiceState::Speaking => "Stop and listen",
            _ if on => "Leave voice",
            _ => "Hey Grok",
        };
        let resp = crate::icons::paint_composer_mic(ui, size, mood)
            .0
            .on_hover_text(tip);
        if resp.clicked() {
            self.listen_voice();
        }
        resp.rect
    }
}

#[cfg(test)]
mod stream_tests {
    use super::*;
    use crate::stt_stream::SttNews;
    use crate::voice_mic::{Heard, MicFinisher};

    fn notes() -> (mpsc::Sender<JobOut>, mpsc::Receiver<JobOut>) {
        mpsc::channel()
    }

    fn captions(rx: &mpsc::Receiver<JobOut>) -> Vec<String> {
        rx.try_iter()
            .filter_map(|j| match j {
                JobOut::VoiceNote(n) => Some(n),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_clearly_finished_sentence_is_sent_without_waiting_for_silence() {
        let (_mic, heard) = mpsc::channel::<Heard>();
        let (news_tx, news) = mpsc::channel();
        news_tx
            .send(SttNews::Caption("remind me to call".into()))
            .unwrap();
        news_tx
            .send(SttNews::Ended("Remind me to call Sam.".into()))
            .unwrap();
        let finisher = MicFinisher::detached();
        let (tx, rx) = notes();
        let text = listen_turn(
            "sk-abcdefghijklmnopqrstuv",
            heard,
            Some((news, finisher.clone())),
            &tx,
        );
        assert_eq!(text, "Remind me to call Sam.");
        assert!(finisher.asked(), "the mic stops listening for this turn");
        let shown = captions(&rx);
        assert!(
            shown.iter().any(|n| n.contains("remind me to call")),
            "the caption shows while they talk: {shown:?}"
        );
    }

    #[test]
    fn when_the_mic_ends_the_turn_the_streamed_words_are_used() {
        let (mic, heard) = mpsc::channel();
        let (news_tx, news) = mpsc::channel();
        mic.send(Heard::Turn(vec![0; 160])).unwrap();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(60));
            let _ = news_tx.send(SttNews::Ended("buy milk".into()));
        });
        let (tx, _rx) = notes();
        let text = listen_turn(
            "sk-abcdefghijklmnopqrstuv",
            heard,
            Some((news, MicFinisher::detached())),
            &tx,
        );
        assert_eq!(text, "buy milk");
    }

    #[test]
    fn noise_alone_listens_again() {
        let (mic, heard) = mpsc::channel();
        let (news_tx, news) = mpsc::channel();
        // Mid-listen the stream never reports an empty end, only after the mic's.
        mic.send(Heard::Turn(vec![0; 160])).unwrap();
        news_tx.send(SttNews::Ended(String::new())).unwrap();
        let (tx, _rx) = notes();
        let text = listen_turn(
            "sk-abcdefghijklmnopqrstuv",
            heard,
            Some((news, MicFinisher::detached())),
            &tx,
        );
        assert_eq!(text, "");
    }

    #[test]
    fn a_failed_stream_falls_back_to_the_upload() {
        let (news_tx, news) = mpsc::channel();
        news_tx.send(SttNews::Caption("half".into())).unwrap();
        news_tx.send(SttNews::Failed("closed".into())).unwrap();
        assert_eq!(final_words(&news), None);
        let (_keep, quiet) = mpsc::channel::<SttNews>();
        drop(_keep);
        assert_eq!(final_words(&quiet), None);
    }

    #[test]
    fn a_long_caption_shows_only_its_end() {
        assert_eq!(caption_note("hi there"), "\u{201c}hi there\u{201d}");
        let long = "word ".repeat(40);
        let shown = caption_note(&long);
        assert!(shown.starts_with("\u{201c}…"), "{shown}");
        assert!(shown.chars().count() <= 84, "{shown}");
    }

    fn speaking_cabin() -> Cabin {
        let mut app = Cabin::quiet_for_test();
        app.voice_state = VoiceState::Speaking;
        app.voice_orb = "speaking".into();
        app
    }

    #[test]
    fn a_tap_while_a_reply_plays_stops_it_and_listens() {
        let mut app = speaking_cabin();
        let (_still_playing, done) = mpsc::channel::<()>();
        app.voice_hold_rx = Some(done);
        // Already listening over the reply.
        app.running = true;
        app.listen_voice();
        assert!(app.voice_hold_rx.is_none(), "the reply stops");
        assert!(app.voice_spoken.is_none());
        assert_eq!(app.voice_state, VoiceState::Listening);
        assert!(app.running, "the same listen carries on");
    }

    #[test]
    fn talking_over_a_reply_stops_it() {
        let mut app = speaking_cabin();
        let (_still_playing, done) = mpsc::channel::<()>();
        app.voice_hold_rx = Some(done);
        app.running = true;
        app.heard_talking();
        assert!(app.voice_hold_rx.is_none());
        assert_eq!(app.voice_state, VoiceState::Listening);
        assert_eq!(app.status, "Listening… go on");
    }

    #[test]
    fn a_tap_while_the_reply_is_still_being_written_says_no_more_of_it() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let played = std::sync::Arc::new(AtomicUsize::new(0));
        let seen = played.clone();
        let synth: crate::spoken::Synth = Box::new(|t| Ok(t.as_bytes().to_vec()));
        let play: crate::spoken::Play = Box::new(move |_, _| {
            seen.fetch_add(1, Ordering::SeqCst);
        });
        let mut app = speaking_cabin();
        app.voice_spoken = Some(crate::spoken::SpokenReply::start(synth, play, None));
        app.listen_voice();
        assert_eq!(app.voice_state, VoiceState::Thinking);
        let spoken = app
            .voice_spoken
            .as_mut()
            .expect("kept until the reply ends");
        assert!(!spoken.feed("More words. And more after that.\n"));
        spoken.finish("");
        let done = spoken.take_done().expect("done");
        done.recv_timeout(std::time::Duration::from_secs(5))
            .expect("a hushed reply still ends, so listening resumes");
        assert_eq!(played.load(Ordering::SeqCst), 0, "nothing more is spoken");
    }
}
