//! Duplex Grok Voice over `wss://api.x.ai/v1/realtime`. Console API key only.

use crate::desktop::{LivePcm, PcmSink};
use grokhub_core::{
    dedicated_voice_model, encode_input_audio_append, encode_session_update, parse_realtime_event,
    pcm_from_capture, realtime_can_connect, voice_session_url, VoiceEvent,
};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::net::TcpStream;
use tungstenite::http::{header::AUTHORIZATION, HeaderValue};
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{client::IntoClientRequest, connect, Message};

fn set_ws_nonblocking(stream: &mut MaybeTlsStream<TcpStream>) {
    match stream {
        MaybeTlsStream::Plain(s) => {
            let _ = s.set_nonblocking(true);
        }
        MaybeTlsStream::Rustls(s) => {
            let _ = s.get_mut().set_nonblocking(true);
        }
        _ => {}
    }
}

pub struct VoiceSock {
    pub rx: Receiver<VoiceEvent>,
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl VoiceSock {
    pub fn halt(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        // Do not join — the read loop may still be in a network wait.
        let _ = self.handle.take();
    }
}

pub fn start(bearer: &str, model: &str) -> Result<VoiceSock, String> {
    if !realtime_can_connect(bearer) {
        return Err(
            "Duplex Voice needs a console API key. OAuth covers STT and TTS."
                .into(),
        );
    }
    let url = voice_session_url(&dedicated_voice_model(model));
    let mut req = url.into_client_request().map_err(|e| e.to_string())?;
    let auth = format!("Bearer {}", bearer.trim());
    req.headers_mut().insert(
        AUTHORIZATION,
        HeaderValue::from_str(&auth).map_err(|e| e.to_string())?,
    );
    let (tx, rx) = mpsc::channel();
    let stop = Arc::new(AtomicBool::new(false));
    let stop_t = stop.clone();
    let handle = std::thread::spawn(move || {
        let (socket, _) = match connect(req) {
            Ok(s) => s,
            Err(e) => {
                let _ = tx.send(VoiceEvent::Error(e.to_string()));
                let _ = tx.send(VoiceEvent::Fallback);
                return;
            }
        };
        let sock = Arc::new(Mutex::new(socket));
        {
            let mut g = match sock.lock() {
                Ok(g) => g,
                Err(_) => return,
            };
            if g.send(Message::Text(encode_session_update())).is_err() {
                let _ = tx.send(VoiceEvent::Fallback);
                return;
            }
        }
        let _ = tx.send(VoiceEvent::Open);
        let mic_sock = sock.clone();
        let mic_stop = stop_t.clone();
        std::thread::spawn(move || {
            if let Some(mut mic) = LivePcm::start() {
                loop {
                    if mic_stop.load(Ordering::SeqCst) {
                        break;
                    }
                    let Some(pcm) = mic.read_frame() else {
                        break;
                    };
                    let pcm = pcm_from_capture(&pcm);
                    if pcm.is_empty() {
                        continue;
                    }
                    let b64 =
                        base64::Engine::encode(&base64::engine::general_purpose::STANDARD, pcm);
                    if let Ok(mut g) = mic_sock.lock() {
                        if g.send(Message::Text(encode_input_audio_append(&b64)))
                            .is_err()
                        {
                            break;
                        }
                    }
                }
                return;
            }
            loop {
                if mic_stop.load(Ordering::SeqCst) {
                    break;
                }
                let chunks = crate::desktop::record_pcm_chunks();
                if chunks.is_empty() {
                    std::thread::sleep(std::time::Duration::from_millis(200));
                    continue;
                }
                for chunk in chunks {
                    if mic_stop.load(Ordering::SeqCst) {
                        return;
                    }
                    let pcm = pcm_from_capture(&chunk);
                    if pcm.is_empty() {
                        continue;
                    }
                    let b64 =
                        base64::Engine::encode(&base64::engine::general_purpose::STANDARD, pcm);
                    if let Ok(mut g) = mic_sock.lock() {
                        if g.send(Message::Text(encode_input_audio_append(&b64)))
                            .is_err()
                        {
                            return;
                        }
                    }
                }
            }
        });
        let mut sink = PcmSink::new();
        loop {
            if stop_t.load(Ordering::SeqCst) {
                break;
            }
            let msg = {
                let mut g = match sock.lock() {
                    Ok(g) => g,
                    Err(_) => break,
                };
                set_ws_nonblocking(g.get_mut());
                g.read()
            };
            match msg {
                Ok(Message::Close(_)) => {
                    let _ = tx.send(VoiceEvent::Close);
                    break;
                }
                Err(tungstenite::Error::Io(e))
                    if e.kind() == std::io::ErrorKind::WouldBlock
                        || e.kind() == std::io::ErrorKind::TimedOut =>
                {
                    std::thread::sleep(std::time::Duration::from_millis(40));
                }
                Err(_) => {
                    let _ = tx.send(VoiceEvent::Close);
                    break;
                }
                Ok(Message::Text(t)) => {
                    if let Ok(v) = serde_json::from_str(&t) {
                        if let Some(ev) = parse_realtime_event(&v) {
                            if let VoiceEvent::AudioOut { pcm_b64 } = &ev {
                                if let Ok(pcm) = base64::Engine::decode(
                                    &base64::engine::general_purpose::STANDARD,
                                    pcm_b64.as_bytes(),
                                ) {
                                    sink.push(&pcm);
                                }
                            }
                            if tx.send(ev).is_err() {
                                break;
                            }
                        }
                    }
                }
                Ok(_) => {}
            }
        }
    });
    Ok(VoiceSock {
        rx,
        stop,
        handle: Some(handle),
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn refuses_empty_bearer() {
        assert!(super::start("", "grok-voice-think-fast-2.0").is_err());
        let err = match super::start("", "grok-voice-think-fast-2.0") {
            Err(e) => e,
            Ok(_) => panic!("empty bearer must not open duplex Voice"),
        };
        assert!(
            err.to_ascii_lowercase().contains("api key")
                || err.to_ascii_lowercase().contains("console"),
            "{err}"
        );
    }
}
