use serde_json::{json, Value};

use crate::chat::XAI_BASE;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VoiceState {
    Idle,
    Listening,
    Speaking,
    Hands,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TranscribeRoute {
    Xai,
    Local,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeyGrokAction {
    Start,
    BargeIn,
    Halt,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeyGrokRoute {
    Realtime,
    PushToTalk,
    None,
}

pub const DEFAULT_VOICE_MODEL: &str = "grok-voice-think-fast-2.0";

pub fn dedicated_voice_model(user: &str) -> String {
    let u = user.trim();
    if u.contains("voice") || u.contains("realtime") {
        u.to_string()
    } else {
        DEFAULT_VOICE_MODEL.to_string()
    }
}

pub fn voice_session_url(model: &str) -> String {
    format!(
        "wss://api.x.ai/v1/realtime?model={}",
        dedicated_voice_model(model)
    )
}

pub fn is_voice_error(s: &str) -> bool {
    s.starts_with("VOICE_RECEIPT:")
}

pub const RECORDERS: &[&str] = &["arecord", "ffmpeg", "sox", "rec"];
pub const TRANSCRIBERS: &[&str] = &["whisper", "whisper-cli", "whisper.cpp", "faster-whisper"];

pub fn hey_grok_on_press(voice: VoiceState, hands_on: bool) -> HeyGrokAction {
    if hands_on {
        return HeyGrokAction::Halt;
    }
    match voice {
        VoiceState::Idle => HeyGrokAction::Start,
        VoiceState::Listening | VoiceState::Speaking => HeyGrokAction::BargeIn,
        VoiceState::Hands => HeyGrokAction::Halt,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VoiceEvent {
    Start,
    Open,
    Close,
    Error(String),
    Transcript { text: String, final_: bool },
    AudioOut { pcm_b64: String },
    AudioEnd,
    BargeIn,
    Hands,
    Halt,
    Fallback,
}

pub fn reduce_voice_state(state: VoiceState, event: &VoiceEvent) -> VoiceState {
    match event {
        VoiceEvent::Start | VoiceEvent::Open => {
            if state == VoiceState::Hands {
                VoiceState::Hands
            } else {
                VoiceState::Listening
            }
        }
        VoiceEvent::Transcript { .. } => VoiceState::Listening,
        VoiceEvent::AudioOut { .. } => {
            if state == VoiceState::Hands {
                VoiceState::Hands
            } else {
                VoiceState::Speaking
            }
        }
        VoiceEvent::AudioEnd => {
            if state == VoiceState::Hands {
                VoiceState::Hands
            } else {
                VoiceState::Listening
            }
        }
        VoiceEvent::BargeIn => VoiceState::Listening,
        VoiceEvent::Hands => VoiceState::Hands,
        VoiceEvent::Halt | VoiceEvent::Close => VoiceState::Idle,
        VoiceEvent::Error(_) | VoiceEvent::Fallback => VoiceState::Idle,
    }
}

pub fn should_mute_speaker(quiet_hours: bool) -> bool {
    quiet_hours
}

pub fn voice_can_connect(bearer: &str) -> bool {
    !bearer.trim().is_empty()
}

pub fn speech_can_connect(bearer: &str) -> bool {
    voice_can_connect(bearer)
}

pub fn realtime_can_connect(api_key: &str) -> bool {
    !api_key.trim().is_empty()
}

pub fn hey_grok_route(has_api_key: bool, has_speech_auth: bool, has_local: bool) -> HeyGrokRoute {
    if has_api_key {
        HeyGrokRoute::Realtime
    } else if has_speech_auth || has_local {
        HeyGrokRoute::PushToTalk
    } else {
        HeyGrokRoute::None
    }
}

pub fn encode_session_update() -> String {
    serde_json::json!({
        "type": "session.update",
        "session": {
            "voice": "eve",
            "instructions": "You are Grok in the GrokHub cabin. Be brief. Use host and computer tools when the user asks to change this Linux box.",
            "turn_detection": { "type": "server_vad" },
            "audio": {
                "input": { "format": { "type": "audio/pcm", "rate": 24000 } },
                "output": { "format": { "type": "audio/pcm", "rate": 24000 } }
            }
        }
    })
    .to_string()
}

pub fn parse_realtime_event(v: &Value) -> Option<VoiceEvent> {
    let t = v.get("type").and_then(|x| x.as_str()).unwrap_or("");
    match t {
        "session.created" | "session.updated" => Some(VoiceEvent::Open),
        "input_audio_buffer.speech_started" => Some(VoiceEvent::BargeIn),
        "input_audio_buffer.speech_stopped" => Some(VoiceEvent::AudioEnd),
        "response.audio.delta" | "response.output_audio.delta" => Some(VoiceEvent::AudioOut {
            pcm_b64: v
                .get("delta")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string(),
        }),
        "response.audio.done" | "response.done" => Some(VoiceEvent::AudioEnd),
        "response.audio_transcript.delta"
        | "conversation.item.input_audio_transcription.delta" => Some(VoiceEvent::Transcript {
            text: parse_voice_event_text(v).unwrap_or_default(),
            final_: false,
        }),
        "response.audio_transcript.done"
        | "conversation.item.input_audio_transcription.completed" => Some(VoiceEvent::Transcript {
            text: parse_voice_event_text(v).unwrap_or_default(),
            final_: true,
        }),
        "error" => Some(VoiceEvent::Error(
            v.get("error")
                .and_then(|e| e.get("message"))
                .and_then(|m| m.as_str())
                .unwrap_or("voice error")
                .to_string(),
        )),
        _ => None,
    }
}

pub fn encode_input_audio_append(b64_pcm: &str) -> String {
    serde_json::json!({
        "type": "input_audio_buffer.append",
        "audio": b64_pcm,
    })
    .to_string()
}

pub fn redact_cabin_from_memory(text: &str) -> String {
    let mut s = text.to_string();
    for n in ["face", "faces", "webcam"] {
        let low = s.to_ascii_lowercase();
        if let Some(i) = low.find(n) {
            s.replace_range(i..i + n.len(), "[cabin-redacted]");
        }
    }
    s
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CabinEyesState {
    Off,
    Armed,
    Seeing,
    EmptyChair,
}

pub fn should_attach_cabin_frame(state: CabinEyesState, user_opt_in: bool) -> bool {
    user_opt_in && matches!(state, CabinEyesState::Seeing)
}

pub fn should_capture_before_chat(user_opt_in: bool) -> bool {
    user_opt_in
}

pub fn cabin_eyes_for_turn(user_opt_in: bool, has_frame: bool) -> CabinEyesState {
    if !user_opt_in {
        CabinEyesState::Off
    } else if has_frame {
        CabinEyesState::Seeing
    } else {
        CabinEyesState::Armed
    }
}

pub fn client_secrets_url() -> String {
    format!("{XAI_BASE}/realtime/client_secrets")
}

pub fn client_secrets_body() -> Value {
    json!({
        "expires_after": { "seconds": 300 }
    })
}

pub fn parse_client_secret(body: &Value) -> Option<String> {
    let from_str = |v: &Value| {
        v.as_str()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
    };
    body.get("value")
        .and_then(from_str)
        .or_else(|| {
            body.get("client_secret").and_then(|v| {
                from_str(v).or_else(|| v.get("value").and_then(from_str))
            })
        })
}

pub fn client_secret_ws_protocol(secret: &str) -> String {
    format!("xai-client-secret.{}", secret.trim())
}

pub fn voice_transcript_sends_chat(live_duplex: bool) -> bool {
    !live_duplex
}

pub fn hey_grok_starts_ptt(sock_live: bool, running: bool) -> bool {
    !sock_live && !running
}

pub fn voice_client_secret_denied(has_api_key: bool) -> Option<&'static str> {
    if has_api_key {
        None
    } else {
        Some("Duplex Voice needs a console API key. OAuth covers STT and TTS.")
    }
}

/// Strip a WAV container so realtime appends get raw PCM.
pub fn pcm_from_capture(bytes: &[u8]) -> &[u8] {
    if bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WAVE" {
        let mut i = 12usize;
        while i + 8 <= bytes.len() {
            let id = &bytes[i..i + 4];
            let sz = u32::from_le_bytes([
                bytes[i + 4],
                bytes[i + 5],
                bytes[i + 6],
                bytes[i + 7],
            ]) as usize;
            i += 8;
            if id == b"data" {
                let end = (i + sz).min(bytes.len());
                return bytes.get(i..end).unwrap_or(&[]);
            }
            i = i.saturating_add(sz);
        }
        return bytes.get(44..).unwrap_or(&[]);
    }
    bytes
}

pub fn stt_url() -> String {
    format!("{XAI_BASE}/stt")
}

pub fn tts_url() -> String {
    format!("{XAI_BASE}/tts")
}

pub fn tts_request_body(text: &str) -> Value {
    json!({
        "text": text,
        "voice_id": "eve",
        "language": "en",
    })
}

pub fn parse_stt_text(body: &Value) -> Option<String> {
    body.get("text")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
}

pub fn transcribe_route(has_key: bool, has_local: bool) -> TranscribeRoute {
    if has_key {
        TranscribeRoute::Xai
    } else if has_local {
        TranscribeRoute::Local
    } else {
        TranscribeRoute::None
    }
}

pub fn parse_voice_event_text(body: &Value) -> Option<String> {
    if let Some(t) = body.get("transcript").and_then(|v| v.as_str()) {
        let t = t.trim();
        if !t.is_empty() {
            return Some(t.to_string());
        }
    }
    body.get("delta")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
}

/// `file` must be last in the multipart body.
pub fn stt_multipart(wav: &[u8], filename: &str, boundary: &str) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
    out.extend_from_slice(b"Content-Disposition: form-data; name=\"language\"\r\n\r\n");
    out.extend_from_slice(b"en\r\n");
    out.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
    out.extend_from_slice(b"Content-Disposition: form-data; name=\"format\"\r\n\r\n");
    out.extend_from_slice(b"true\r\n");
    out.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
    out.extend_from_slice(
        format!(
            "Content-Disposition: form-data; name=\"file\"; filename=\"{filename}\"\r\nContent-Type: audio/wav\r\n\r\n"
        )
        .as_bytes(),
    );
    out.extend_from_slice(wav);
    out.extend_from_slice(b"\r\n");
    out.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hey_and_url() {
        assert_eq!(dedicated_voice_model("grok-3-mini-fast"), DEFAULT_VOICE_MODEL);
        assert!(voice_session_url("").contains("grok-voice-think-fast-2.0"));
        assert!(is_voice_error("VOICE_RECEIPT: arecord missing"));
        assert!(!is_voice_error("hey grok flash the pi"));
        assert_eq!(hey_grok_on_press(VoiceState::Idle, false), HeyGrokAction::Start);
        assert_eq!(hey_grok_on_press(VoiceState::Listening, false), HeyGrokAction::BargeIn);
        assert_eq!(hey_grok_on_press(VoiceState::Idle, true), HeyGrokAction::Halt);
        assert_eq!(hey_grok_on_press(VoiceState::Hands, false), HeyGrokAction::Halt);
        let ev = parse_realtime_event(&serde_json::json!({
            "type": "conversation.item.input_audio_transcription.completed",
            "transcript": "hey grok"
        }))
        .unwrap();
        assert_eq!(
            ev,
            VoiceEvent::Transcript {
                text: "hey grok".into(),
                final_: true
            }
        );
        assert_eq!(
            reduce_voice_state(VoiceState::Idle, &VoiceEvent::Start),
            VoiceState::Listening
        );
        assert!(voice_can_connect("xai-1"));
        assert!(!voice_can_connect(""));
        assert!(encode_session_update().contains("session.update"));
        assert!(should_mute_speaker(true));
        assert!(redact_cabin_from_memory("a face here").contains("[cabin-redacted]"));
        assert!(should_attach_cabin_frame(CabinEyesState::Seeing, true));
        assert!(!should_attach_cabin_frame(CabinEyesState::Off, true));
        assert!(should_capture_before_chat(true));
        assert!(!should_capture_before_chat(false));
        assert_eq!(cabin_eyes_for_turn(true, true), CabinEyesState::Seeing);
        assert_eq!(cabin_eyes_for_turn(true, false), CabinEyesState::Armed);
        assert_eq!(cabin_eyes_for_turn(false, true), CabinEyesState::Off);
        assert!(should_attach_cabin_frame(cabin_eyes_for_turn(true, true), true));
        assert!(!should_attach_cabin_frame(cabin_eyes_for_turn(true, false), true));
        assert_eq!(stt_url(), "https://api.x.ai/v1/stt");
        assert_eq!(tts_url(), "https://api.x.ai/v1/tts");
        assert_eq!(
            parse_stt_text(&serde_json::json!({ "text": " flash the pi " })).as_deref(),
            Some("flash the pi")
        );
        assert_eq!(transcribe_route(true, false), TranscribeRoute::Xai);
        assert_eq!(transcribe_route(false, true), TranscribeRoute::Local);
        assert_eq!(transcribe_route(false, false), TranscribeRoute::None);
        let ev = serde_json::json!({
            "type": "conversation.item.input_audio_transcription.completed",
            "transcript": "hey grok"
        });
        assert_eq!(parse_voice_event_text(&ev).as_deref(), Some("hey grok"));
        let tts = tts_request_body("hello");
        assert_eq!(tts["voice_id"], "eve");
        assert_eq!(tts["text"], "hello");
        let form = stt_multipart(b"RIFF", "grokhub-voice.wav", "bound");
        let s = String::from_utf8_lossy(&form);
        assert!(s.find("name=\"language\"").unwrap() < s.find("name=\"file\"").unwrap());
        assert!(s.contains("filename=\"grokhub-voice.wav\""));
    }

    #[test]
    fn oauth_speech_and_api_key_realtime() {
        assert_eq!(DEFAULT_VOICE_MODEL, "grok-voice-think-fast-2.0");
        assert!(voice_session_url("").contains("grok-voice-think-fast-2.0"));
        assert!(voice_session_url("grok-voice-latest").contains("grok-voice-latest"));
        assert_eq!(
            dedicated_voice_model(""),
            "grok-voice-think-fast-2.0"
        );
        assert_eq!(hey_grok_route(true, true, false), HeyGrokRoute::Realtime);
        assert_eq!(hey_grok_route(false, true, false), HeyGrokRoute::PushToTalk);
        assert_eq!(hey_grok_route(false, false, true), HeyGrokRoute::PushToTalk);
        assert_eq!(hey_grok_route(false, false, false), HeyGrokRoute::None);
        assert!(realtime_can_connect("xai-k"));
        assert!(!realtime_can_connect(""));
        assert!(speech_can_connect("tok"));
        assert!(speech_can_connect("xai-k"));
        assert!(!speech_can_connect(""));
        let sess = encode_session_update();
        assert!(sess.contains("\"voice\":\"eve\"") || sess.contains("\"voice\": \"eve\""));
        assert!(sess.contains("server_vad"));
        assert!(sess.contains("audio/pcm"));
        assert!(sess.contains("24000"));
        assert!(!sess.contains("whisper-1"));
        assert!(!sess.contains("modalities"));
        assert_eq!(
            client_secrets_url(),
            "https://api.x.ai/v1/realtime/client_secrets"
        );
        let body = client_secrets_body();
        assert_eq!(body["expires_after"]["seconds"], 300);
        assert!(body.get("session").is_none(), "xAI client_secrets rejects session");
        assert_eq!(
            parse_client_secret(&serde_json::json!({
                "value": "xai-realtime-client-secret-abc",
                "expires_at": 1
            }))
            .as_deref(),
            Some("xai-realtime-client-secret-abc")
        );
        assert_eq!(
            parse_client_secret(&serde_json::json!({
                "client_secret": "ek_from_field"
            }))
            .as_deref(),
            Some("ek_from_field")
        );
        assert!(parse_client_secret(&serde_json::json!({ "error": "nope" })).is_none());
        assert_eq!(
            client_secret_ws_protocol("ek_abc"),
            "xai-client-secret.ek_abc"
        );
        assert!(!voice_transcript_sends_chat(true));
        assert!(voice_transcript_sends_chat(false));
        assert!(!hey_grok_starts_ptt(true, false));
        assert!(hey_grok_starts_ptt(false, false));
        assert!(!hey_grok_starts_ptt(false, true));
        assert_eq!(
            voice_client_secret_denied(false),
            Some("Duplex Voice needs a console API key. OAuth covers STT and TTS.")
        );
        assert!(voice_client_secret_denied(true).is_none());
        let ev = parse_realtime_event(&serde_json::json!({
            "type": "response.output_audio.delta",
            "delta": "AAAA"
        }))
        .unwrap();
        match ev {
            VoiceEvent::AudioOut { pcm_b64 } => assert_eq!(pcm_b64, "AAAA"),
            other => panic!("{other:?}"),
        }
        let mut wav = b"RIFF".to_vec();
        wav.extend_from_slice(&[36, 0, 0, 0]);
        wav.extend_from_slice(b"WAVE");
        wav.extend_from_slice(b"fmt ");
        wav.extend_from_slice(&[16, 0, 0, 0]);
        wav.extend_from_slice(&[1, 0, 1, 0]);
        wav.extend_from_slice(&24000u32.to_le_bytes());
        wav.extend_from_slice(&48000u32.to_le_bytes());
        wav.extend_from_slice(&[2, 0, 16, 0]);
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&4u32.to_le_bytes());
        wav.extend_from_slice(b"PCM!");
        assert_eq!(pcm_from_capture(&wav), b"PCM!");
        assert_eq!(pcm_from_capture(b"raw-pcm"), b"raw-pcm");
        assert_eq!(transcribe_route(true, false), TranscribeRoute::Xai);
        assert_eq!(transcribe_route(false, true), TranscribeRoute::Local);
    }

    #[test]
    fn nested_secret_hands_and_bad_wav() {
        assert_eq!(
            parse_client_secret(&serde_json::json!({
                "client_secret": { "value": "ek_nested" }
            }))
            .as_deref(),
            Some("ek_nested")
        );
        assert!(parse_client_secret(&serde_json::json!({
            "client_secret": { "value": "   " }
        }))
        .is_none());
        assert_eq!(
            reduce_voice_state(VoiceState::Hands, &VoiceEvent::Start),
            VoiceState::Hands
        );
        assert_eq!(
            reduce_voice_state(
                VoiceState::Hands,
                &VoiceEvent::AudioOut {
                    pcm_b64: "AA".into()
                }
            ),
            VoiceState::Hands
        );
        assert_eq!(
            reduce_voice_state(VoiceState::Hands, &VoiceEvent::Halt),
            VoiceState::Idle
        );
        let ev = parse_realtime_event(&serde_json::json!({
            "type": "error",
            "error": { "message": "quota" }
        }))
        .unwrap();
        assert_eq!(ev, VoiceEvent::Error("quota".into()));
        let mut wav = b"RIFF".to_vec();
        wav.extend_from_slice(&[12, 0, 0, 0]);
        wav.extend_from_slice(b"WAVE");
        wav.extend_from_slice(&[0u8; 40]);
        assert_eq!(pcm_from_capture(&wav), &wav[44..]);
        assert_eq!(pcm_from_capture(b"RIFF????"), b"RIFF????");
    }
}
