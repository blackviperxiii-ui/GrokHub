//! Streaming speech-to-text for one voice turn.
//!
//! The socket opens when a listen starts, gets a copy of every mic block, and
//! returns live captions and the turn's words. Smart Turn on the server can
//! end a turn as soon as the person has clearly finished; the app's own
//! detector still ends it at the latest, and then the words are already
//! there. Anything that goes wrong is reported once and the turn falls back
//! to the one-shot upload.

use std::io::ErrorKind;
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::time::{Duration, Instant};

use grokhub_core::stt_stream::{parse_stt_event, SttEvent, SttTurn, TurnStep, FINALIZE};
use tungstenite::client::IntoClientRequest;
use tungstenite::Message;

/// How long one listen's socket may stay open.
const MAX_STREAM: Duration = Duration::from_secs(75);
/// How long to wait for the words once the turn is over.
const FINAL_WAIT: Duration = Duration::from_secs(2);
/// Read timeout while streaming: how often new audio gets sent.
const POLL: Duration = Duration::from_millis(30);
const CONNECT: Duration = Duration::from_secs(5);

/// Set when the server refuses this sign-in for streaming: later turns this
/// run go straight to the one-shot upload instead of trying again.
static REFUSED: AtomicBool = AtomicBool::new(false);

/// What the stream says about the turn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SttNews {
    /// Everything heard so far.
    Caption(String),
    /// The turn is over: its words (empty when only noise was heard).
    Ended(String),
    /// The stream stopped; use the one-shot upload.
    Failed(String),
}

/// Start a stream for one listen: send mic blocks to the returned sender and
/// read news from the receiver. `None` when streaming can't be used (no
/// sign-in, refused earlier this run, or traffic goes through a proxy).
pub fn open(key: &str) -> Option<(Sender<Vec<i16>>, Receiver<SttNews>)> {
    if key.trim().is_empty() || REFUSED.load(Ordering::SeqCst) || behind_proxy() {
        return None;
    }
    let (audio_tx, audio_rx) = mpsc::channel();
    let (news_tx, news_rx) = mpsc::channel();
    let key = key.trim().to_string();
    let url =
        grokhub_core::stt_stream::stt_stream_url(&[grokhub_core::brand::product_name(), "Grok"]);
    std::thread::Builder::new()
        .name("voice-stt".into())
        .spawn(move || {
            if let Err(e) = stream(&key, &url, &audio_rx, &news_tx) {
                let _ = news_tx.send(SttNews::Failed(e));
            }
        })
        .ok()?;
    Some((audio_tx, news_rx))
}

/// A proxy the plain socket can't go through; the upload path honors it.
fn behind_proxy() -> bool {
    ["HTTPS_PROXY", "https_proxy", "ALL_PROXY", "all_proxy"]
        .iter()
        .any(|k| std::env::var(k).is_ok_and(|v| !v.trim().is_empty()))
}

type Socket = tungstenite::WebSocket<tungstenite::stream::MaybeTlsStream<TcpStream>>;

fn connect(key: &str, url: &str) -> Result<(Socket, TcpStream), String> {
    let addrs: Vec<_> = ("api.x.ai", 443)
        .to_socket_addrs()
        .map_err(|e| format!("speech stream: {e}"))?
        .collect();
    let tcp = addrs
        .iter()
        .find_map(|a| TcpStream::connect_timeout(a, CONNECT).ok())
        .ok_or("speech stream: couldn't connect")?;
    let _ = tcp.set_nodelay(true);
    tcp.set_read_timeout(Some(CONNECT))
        .and_then(|()| tcp.set_write_timeout(Some(CONNECT)))
        .map_err(|e| e.to_string())?;
    let handle = tcp.try_clone().map_err(|e| e.to_string())?;
    let mut req = url.into_client_request().map_err(|e| e.to_string())?;
    let auth = tungstenite::http::HeaderValue::from_str(&format!("Bearer {key}"))
        .map_err(|_| "speech stream: bad sign-in".to_string())?;
    req.headers_mut().insert("authorization", auth);
    match tungstenite::client_tls(req, tcp) {
        Ok((ws, _)) => Ok((ws, handle)),
        Err(tungstenite::HandshakeError::Failure(tungstenite::Error::Http(resp))) => {
            let code = resp.status().as_u16();
            if matches!(code, 401 | 403) {
                REFUSED.store(true, Ordering::SeqCst);
            }
            Err(format!("speech stream refused ({code})"))
        }
        Err(e) => Err(format!("speech stream: {e}")),
    }
}

fn stream(
    key: &str,
    url: &str,
    audio: &Receiver<Vec<i16>>,
    news: &Sender<SttNews>,
) -> Result<(), String> {
    crate::xai::egress_ok(url, &[grokhub_agent::harness::DataClass::Chat])?;
    let (mut ws, handle) = connect(key, url)?;
    handle
        .set_read_timeout(Some(POLL))
        .map_err(|e| e.to_string())?;
    let opened = Instant::now();
    let mut turn = SttTurn::new();
    let mut pending: Vec<Vec<i16>> = Vec::new();
    let (mut ready, mut over, mut finalized) = (false, false, false);
    let mut final_by = None;
    loop {
        if opened.elapsed() > MAX_STREAM {
            return Err("speech stream ran too long".into());
        }
        loop {
            match audio.try_recv() {
                Ok(block) => pending.push(block),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    over = true;
                    break;
                }
            }
        }
        if ready {
            for block in pending.drain(..) {
                ws.send(Message::binary(pcm_bytes(&block)))
                    .map_err(|e| e.to_string())?;
            }
            if over && !finalized {
                ws.send(Message::text(FINALIZE))
                    .map_err(|e| e.to_string())?;
                finalized = true;
                final_by = Some(Instant::now() + FINAL_WAIT);
            }
        }
        if final_by.is_some_and(|t| Instant::now() > t) {
            return Err("speech stream: no final words".into());
        }
        let msg = match ws.read() {
            Ok(m) => m,
            Err(tungstenite::Error::Io(e))
                if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) =>
            {
                continue;
            }
            Err(e) => return Err(format!("speech stream: {e}")),
        };
        let text = match msg {
            Message::Text(t) => t,
            Message::Close(_) => return Err("speech stream closed".into()),
            _ => continue,
        };
        let ev = parse_stt_event(text.as_str());
        let step = match &ev {
            SttEvent::Ready => {
                ready = true;
                continue;
            }
            SttEvent::Error(m) => return Err(format!("speech stream: {m}")),
            _ => turn.apply(&ev),
        };
        let sent = match step {
            TurnStep::Caption(c) => news.send(SttNews::Caption(c)),
            // Noise alone mid-listen: keep listening for real words.
            TurnStep::Ended(words) if words.is_empty() && !finalized => continue,
            TurnStep::Ended(words) => {
                let _ = news.send(SttNews::Ended(words));
                let _ = ws.close(None);
                return Ok(());
            }
            TurnStep::Nothing => continue,
        };
        if sent.is_err() {
            // Nobody is listening any more.
            let _ = ws.close(None);
            return Ok(());
        }
    }
}

fn pcm_bytes(block: &[i16]) -> Vec<u8> {
    block.iter().flat_map(|s| s.to_le_bytes()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_go_out_as_16_bit_little_endian() {
        assert_eq!(pcm_bytes(&[1, -2, 256]), vec![1, 0, 0xFE, 0xFF, 0, 1]);
    }

    #[test]
    fn no_sign_in_means_no_stream() {
        assert!(open("  ").is_none());
    }
}
