//! Hey Grok listen, PTT, and speak.

use super::*;

/// Shown when the open mic has heard nothing for a while.
const CHECK_IN_NOTE: &str = "Still listening. Talk when you're ready, or tap the mic to stop.";

/// Wait for the next turn from the open mic and turn it into words. Silence
/// keeps listening; an empty transcript (a cough, a door) listens again.
pub(super) fn listen_turn(
    api_key: &str,
    heard: mpsc::Receiver<crate::voice_mic::Heard>,
    notes: &mpsc::Sender<JobOut>,
) -> String {
    use crate::voice_mic::Heard;
    loop {
        match heard.recv() {
            Ok(Heard::Started) => {
                let _ = notes.send(JobOut::VoiceNote("Listening… go on".into()));
            }
            Ok(Heard::CheckIn) => {
                let _ = notes.send(JobOut::VoiceNote(CHECK_IN_NOTE.into()));
            }
            Ok(Heard::Turn(samples)) => {
                let _ = notes.send(JobOut::VoiceNote("Got it…".into()));
                let ended = std::time::Instant::now();
                let text = heard_text(api_key, &samples);
                crate::spoken::note_turn(crate::spoken::TurnClock {
                    ended,
                    words_ms: ended.elapsed().as_millis() as u64,
                });
                return text;
            }
            Ok(Heard::Nothing) => return String::new(),
            Ok(Heard::Pause) => return VOICE_PAUSED.into(),
            Ok(Heard::Lost(e)) => return format!("VOICE_RECEIPT: {e}"),
            Err(_) => return "VOICE_RECEIPT: voice stopped".into(),
        }
    }
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
        let heard = self
            .voice_mic
            .get_or_insert_with(crate::voice_mic::VoiceMic::open)
            .listen();
        self.voice_orb = "listening".into();
        self.voice_state = VoiceState::Listening;
        self.running = true;
        self.abandon_turn_card();
        self.chat_job_thread = None;
        self.status = voice_mode_label(VoiceState::Listening).into();
        let (tx, rx) = mpsc::channel();
        self.rx = Some(rx);
        std::thread::spawn(move || {
            let text = listen_turn(&speech, heard, &tx);
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
            Ok(()) | Err(mpsc::TryRecvError::Disconnected) => self.maybe_continue_ptt(),
            Err(mpsc::TryRecvError::Empty) => self.voice_hold_rx = Some(rx),
        }
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
        crate::spoken::SpokenReply::xai(self.bearer(), self.speech_voice(), self.lips.clone(), turn)
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
            VoiceState::Listening | VoiceState::Thinking | VoiceState::Hands => {
                crate::icons::MicMood::Live
            }
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
