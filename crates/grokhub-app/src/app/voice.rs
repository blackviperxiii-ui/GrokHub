//! Hey Grok listen, PTT, and speak.

use super::*;


pub(super) fn listen_turn(api_key: &str) -> String {
    let wav = match record_once() {
        Ok(p) => p,
        Err(e) => return format!("VOICE_RECEIPT: {e}"),
    };
    let has_local = first_bin(TRANSCRIBERS).is_some();
    match transcribe_route(!api_key.trim().is_empty(), has_local) {
        TranscribeRoute::Xai => {
            let len = std::fs::metadata(&wav).map(|m| m.len()).unwrap_or(u64::MAX);
            if len > IMAGE_FILE_CAP {
                "VOICE_RECEIPT: recording too large".into()
            } else {
                match std::fs::read(&wav) {
                    Ok(bytes) => match grok_stt(api_key, &bytes) {
                        Ok(t) => t,
                        Err(e) => transcribe_local(&wav)
                            .unwrap_or_else(|local| format!("VOICE_RECEIPT: {e}; {local}")),
                    },
                    Err(e) => format!("VOICE_RECEIPT: {e}"),
                }
            }
        }
        TranscribeRoute::Local => match transcribe_local(&wav) {
            Ok(t) if !t.trim().is_empty() => t.trim().to_string(),
            Ok(_) => "VOICE_RECEIPT: empty transcript".into(),
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
        if !hey_grok_starts_ptt(self.voice_sock.is_some(), self.running) {
            return;
        }
        let speech = self.bearer();
        self.voice_orb = "listening".into();
        self.voice_state = VoiceState::Listening;
        self.running = true;
        self.abandon_turn_card();
        self.chat_job_thread = None;
        self.status = "Listening… STT".into();
        let (tx, rx) = mpsc::channel();
        self.rx = Some(rx);
        std::thread::spawn(move || {
            let _ = tx.send(JobOut::Voice(listen_turn(&speech)));
        });
    }

    pub(super) fn maybe_continue_ptt(&mut self) {
        if !ptt_after_speak(self.voice_is_on()) {
            return;
        }
        if self.voice_hold_rx.is_some()
            || !hey_grok_starts_ptt(self.voice_sock.is_some(), self.running)
        {
            return;
        }
        self.start_ptt_listen();
    }

    pub(super) fn poll_voice_hold(&mut self) {
        let Some(rx) = self.voice_hold_rx.take() else {
            return;
        };
        match rx.try_recv() {
            Ok(()) | Err(mpsc::TryRecvError::Disconnected) => self.maybe_continue_ptt(),
            Err(mpsc::TryRecvError::Empty) => self.voice_hold_rx = Some(rx),
        }
    }

    pub(super) fn voice_is_on(&self) -> bool {
        voice_mode_active(self.voice_state, self.voice_sock.is_some())
    }

    pub(super) fn leave_voice(&mut self) {
        self.voice_hold_rx = None;
        self.voice_ready_at = None;
        if let Some(mut s) = self.voice_sock.take() {
            s.halt();
        }
        let ptt = self.running && self.voice_state != VoiceState::Idle;
        self.voice_state = VoiceState::Idle;
        self.voice_orb = "idle".into();
        if ptt {
            self.halt_work("Voice off");
        } else {
            self.status = "Voice off".into();
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
            VoiceState::Listening | VoiceState::Speaking | VoiceState::Hands => {
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

    pub(super) fn paint_voice_mic(&mut self, ui: &mut egui::Ui, size: f32) {
        let on = self.voice_is_on();
        let mood = match self.voice_state {
            VoiceState::Speaking => crate::icons::MicMood::Speaking,
            VoiceState::Listening | VoiceState::Hands => crate::icons::MicMood::Live,
            VoiceState::Idle | VoiceState::Ready => {
                if on {
                    crate::icons::MicMood::Live
                } else {
                    crate::icons::MicMood::Idle
                }
            }
        };
        let tip = if on { "Leave voice" } else { "Hey Grok" };
        if crate::icons::paint_composer_mic(ui, size, mood)
            .0
            .on_hover_text(tip)
            .clicked()
        {
            self.listen_voice();
        }
    }

    pub(super) fn poll_voice(&mut self) {
        let Some(sock) = &self.voice_sock else {
            return;
        };
        let mut evs = Vec::new();
        while let Ok(ev) = sock.rx.try_recv() {
            evs.push(ev);
        }
        for ev in evs {
            self.voice_state = reduce_voice_state(self.voice_state, &ev);
            self.voice_orb = match self.voice_state {
                VoiceState::Listening => "listening",
                VoiceState::Speaking => "speaking",
                VoiceState::Hands => "hands",
                VoiceState::Idle | VoiceState::Ready => "idle",
            }
            .into();
            match ev {
                VoiceEvent::Transcript { .. } => {
                    if let Some((role, text, kind)) = voice_stream_token(&ev) {
                        if voice_transcript_sends_chat(self.voice_sock.is_some()) {
                            if voice_log_role(&ev).is_some() && role == "user" {
                                self.send_chat(text.to_string());
                            }
                        } else {
                            let push = {
                                let last =
                                    self.live_mut().last_mut().map(|m| (m.0.as_str(), &mut m.1));
                                fold_stream_fields(last, role, text, kind)
                            };
                            if let Some((role, content)) = push {
                                self.live_mut().push((role, content));
                            }
                            if matches!(kind, StreamTokenKind::Replace)
                                && voice_log_role(&ev).is_some()
                            {
                                self.persist();
                            } else {
                                self.persist_idle_key = self.persist_idle_now();
                            }
                        }
                    }
                }
                VoiceEvent::Fallback | VoiceEvent::Error(_) => {
                    if let Some(mut s) = self.voice_sock.take() {
                        s.halt();
                    }
                    self.status = "Voice socket failed — push-to-talk".into();
                }
                VoiceEvent::Close => {
                    self.voice_sock = None;
                    self.voice_state = VoiceState::Idle;
                }
                _ => {}
            }
        }
    }

    pub(super) fn sync_hub_voice(&self) {
        if let Ok(mut st) = self.hub.lock() {
            st.console_api_key = self.console_key().to_string();
            if st.mint_realtime.is_none() {
                st.mint_realtime = Some(MintRealtimeFn(Arc::new(|key| {
                    crate::xai::grok_realtime_secret(key)
                })));
            }
        }
    }
}
