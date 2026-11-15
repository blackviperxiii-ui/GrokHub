//! Streaming speech-to-text for one voice-mode turn.
//!
//! The socket opens when a listen starts and gets a copy of every mic block.
//! It sends back live captions and the turn's words. Smart Turn on the
//! server can end a turn as soon as the person has clearly finished; the
//! mic's own detector still ends it at the latest, and then the words are
//! usually already there. Anything that goes wrong is reported once, and the
//! turn falls back to the one-shot upload ([`crate::voice::transcribe`]).

use std::hash::{Hash, Hasher};
use std::io::ErrorKind;
use std::net::{TcpStream, ToSocketAddrs};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, TryRecvError};
use std::sync::Arc;
use std::time::{Duration, Instant};

use grokhub_agent::harness as hx;
use grokhub_core::stt_stream::{parse_stt_event, SttEvent, SttTurn, TurnStep, FINALIZE};
use tungstenite::client::IntoClientRequest;
use tungstenite::stream::MaybeTlsStream;
use tungstenite::Message;

/// Points the stream at a local fake socket, for tests only: `ws://` on
/// loopback, any port and path. Anything else is ignored.
pub const TEST_WS_ENV: &str = "GROKHUB_SHELL_STT_WS_URL";
/// How long a turn's audio may stream, from its first block: the longest
/// turn (60 s) and the wait for its words.
#[cfg(not(test))]
const MAX_STREAM: Duration = Duration::from_secs(75);
#[cfg(test)]
const MAX_STREAM: Duration = Duration::from_millis(2500);
/// How long a socket may wait for someone to start talking. A listen can
/// outlast it (a cough or a hum puts off its pause), so it then reopens one.
#[cfg(not(test))]
const MAX_IDLE: Duration = Duration::from_secs(90);
#[cfg(test)]
const MAX_IDLE: Duration = Duration::from_secs(4);
/// How long the socket waits for the words once the turn is over.
const FINAL_WAIT: Duration = Duration::from_secs(2);
/// Read timeout while streaming: how often new audio goes out.
const POLL: Duration = Duration::from_millis(30);
const CONNECT: Duration = Duration::from_secs(5);
/// Offline only came on while the stream ran.
const OFFLINE: &str = "speech stream stopped: Offline only is on";
/// How long hearing a turn waits for the stream's words before it uploads.
#[cfg(not(test))]
pub const HEAR_WAIT: Duration = Duration::from_millis(2500);
#[cfg(test)]
pub const HEAR_WAIT: Duration = Duration::from_millis(150);

/// What the stream says about the turn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SttNews {
    /// Everything heard so far.
    Caption(String),
    /// The turn is over: its words (empty when only noise was heard).
    Ended(String),
    /// The stream stopped; the turn uses the one-shot upload.
    Failed(String),
    /// Nobody started talking for [`MAX_IDLE`]: the socket closed, and a
    /// listen still waiting may open a new one.
    Idle,
}

/// One listen's stream: mic blocks go in, news comes out. Dropping `audio`
/// ends the turn (the socket asks for the final words).
pub struct SttLink {
    pub audio: Sender<Vec<i16>>,
    pub news: Receiver<SttNews>,
    /// The server said it is ready for audio.
    pub ready: Arc<AtomicBool>,
}

impl SttLink {
    /// Whether this stream can have words for a turn: a stream the server
    /// never got ready for sends no audio, so a turn doesn't wait on it.
    pub fn is_ready(&self) -> bool {
        self.ready.load(Ordering::SeqCst)
    }
}

/// The key the server refused for streaming (401 or 403), as a hash: later
/// listens with that key go straight to the one-shot upload instead of trying
/// again. A new key gets its own try.
pub struct Gate(AtomicU64);

impl Gate {
    pub const fn new() -> Self {
        Self(AtomicU64::new(0))
    }

    fn shut_for(&self, bearer: &str) -> bool {
        self.0.load(Ordering::SeqCst) == key_hash(bearer)
    }

    fn shut(&self, bearer: &str) {
        self.0.store(key_hash(bearer), Ordering::SeqCst);
    }
}

/// Never 0, the gate's "nothing refused".
fn key_hash(bearer: &str) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    bearer.hash(&mut h);
    h.finish().max(1)
}

static GATE: Gate = Gate::new();

/// Where the stream goes and the credential it carries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamTarget {
    pub url: String,
    pub bearer: String,
}

/// The loopback fake in tests, else xAI with the speech-to-text key. `None`
/// without a key. Like the upload, the socket dials direct, not through an
/// env proxy.
pub fn target() -> Option<StreamTarget> {
    if let Some(url) = ws_loopback_override(std::env::var(TEST_WS_ENV).ok().as_deref()) {
        return Some(StreamTarget {
            url,
            bearer: crate::credential::LOOPBACK_TEST_BEARER.into(),
        });
    }
    Some(StreamTarget {
        url: grokhub_core::stt_stream::stt_stream_url(&[
            grokhub_core::brand::product_name(),
            "Grok",
        ]),
        bearer: crate::voice::stt_bearer().ok()?,
    })
}

/// A stream for one listen to `to`. `None` when it can't be used: this key
/// was refused earlier this run, or the egress guard says no.
pub fn open(config_dir: &Path, to: &StreamTarget) -> Option<SttLink> {
    open_at(
        config_dir,
        &to.url,
        &to.bearer,
        &GATE,
        grokhub_core::offline::enabled,
    )
}

/// [`open`] against `url` with `bearer`, refused for the run by `gate`.
/// The egress guard runs here, on the caller's thread, before any socket;
/// `offline` is asked again before each block goes out, so turning Offline
/// only on mid-listen stops the audio.
pub(crate) fn open_at(
    config_dir: &Path,
    url: &str,
    bearer: &str,
    gate: &'static Gate,
    offline: fn() -> bool,
) -> Option<SttLink> {
    let bearer = bearer.trim().to_string();
    if bearer.is_empty() || gate.shut_for(&bearer) {
        return None;
    }
    hx::guard_quiet(config_dir, &hx::EgressReq::new(url, &[hx::DataClass::Chat])).ok()?;
    let (audio_tx, audio_rx) = mpsc::channel();
    let (news_tx, news_rx) = mpsc::channel();
    let ready = Arc::new(AtomicBool::new(false));
    let said_ready = ready.clone();
    let url = url.to_string();
    std::thread::Builder::new()
        .name("voice-stt".into())
        .spawn(move || {
            let to = Ends {
                news: &news_tx,
                ready: &said_ready,
            };
            if let Err(e) = stream(&url, &bearer, gate, offline, &audio_rx, &to) {
                let _ = news_tx.send(SttNews::Failed(e));
            }
        })
        .ok()?;
    Some(SttLink {
        audio: audio_tx,
        news: news_rx,
        ready,
    })
}

/// The test socket: `ws://127.0.0.1`, `ws://localhost` or `ws://[::1]`, any
/// port and path, with no user info. Anything else, `wss` included, is
/// ignored, so the setting can't move real audio.
pub fn ws_loopback_override(raw: Option<&str>) -> Option<String> {
    let raw = raw?.trim();
    let rest = raw.strip_prefix("ws://")?;
    let host_port = rest.split(['/', '\\', '?', '#']).next().unwrap_or("");
    if host_port.contains('@') {
        return None;
    }
    let host = if let Some(v6) = host_port.strip_prefix('[') {
        v6.split(']').next().unwrap_or("")
    } else {
        host_port.split(':').next().unwrap_or("")
    };
    matches!(host, "127.0.0.1" | "localhost" | "::1").then(|| raw.to_string())
}

/// The turn's words from the stream, waiting up to `wait` and skipping
/// captions. `None` when it failed, went away or ran out of time: upload.
pub fn final_words(news: &Receiver<SttNews>, wait: Duration) -> Option<String> {
    let by = Instant::now() + wait;
    loop {
        let left = by.saturating_duration_since(Instant::now());
        match news.recv_timeout(left) {
            Ok(SttNews::Ended(words)) => return Some(words),
            Ok(SttNews::Caption(_)) => continue,
            Ok(SttNews::Failed(_) | SttNews::Idle) => return None,
            Err(RecvTimeoutError::Timeout | RecvTimeoutError::Disconnected) => return None,
        }
    }
}

type Socket = tungstenite::WebSocket<MaybeTlsStream<TcpStream>>;

/// TCP with a timeout to the URL's host and port, TLS (rustls, ring, webpki
/// roots) for `wss`, then the upgrade with the bearer.
fn connect(url: &str, bearer: &str, gate: &Gate) -> Result<Socket, String> {
    let mut req = url
        .into_client_request()
        .map_err(|e| format!("speech stream: {e}"))?;
    let header = |v: &str| {
        tungstenite::http::HeaderValue::from_str(v)
            .map_err(|_| "speech stream: bad sign-in".to_string())
    };
    req.headers_mut()
        .insert("authorization", header(&format!("Bearer {bearer}"))?);
    req.headers_mut()
        .insert("user-agent", header(grokhub_agent::USER_AGENT)?);
    let uri = req.uri();
    let host = uri.host().ok_or("speech stream: no host")?.to_string();
    let tls = uri.scheme_str() == Some("wss");
    let port = uri.port_u16().unwrap_or(if tls { 443 } else { 80 });
    let tcp = (host.as_str(), port)
        .to_socket_addrs()
        .map_err(|e| format!("speech stream: {e}"))?
        .find_map(|a| TcpStream::connect_timeout(&a, CONNECT).ok())
        .ok_or("speech stream: couldn't connect")?;
    let _ = tcp.set_nodelay(true);
    tcp.set_read_timeout(Some(CONNECT))
        .and_then(|()| tcp.set_write_timeout(Some(CONNECT)))
        .map_err(|e| e.to_string())?;
    let connector = if tls {
        Some(tungstenite::Connector::Rustls(
            grokhub_agent::ws_tls_config()?,
        ))
    } else {
        None
    };
    match tungstenite::client_tls_with_config(req, tcp, None, connector) {
        Ok((ws, _)) => Ok(ws),
        Err(tungstenite::HandshakeError::Failure(tungstenite::Error::Http(resp))) => {
            let code = resp.status().as_u16();
            if matches!(code, 401 | 403) {
                gate.shut(bearer);
            }
            Err(format!("speech stream refused ({code})"))
        }
        Err(e) => Err(format!("speech stream: {e}")),
    }
}

fn tcp_of(ws: &Socket) -> Option<&TcpStream> {
    match ws.get_ref() {
        MaybeTlsStream::Plain(s) => Some(s),
        MaybeTlsStream::Rustls(s) => Some(s.get_ref()),
        _ => None,
    }
}

/// Where one stream reports: its news, and the flag that says the server is
/// ready for audio.
struct Ends<'a> {
    news: &'a Sender<SttNews>,
    ready: &'a AtomicBool,
}

fn stream(
    url: &str,
    bearer: &str,
    gate: &Gate,
    offline: fn() -> bool,
    audio: &Receiver<Vec<i16>>,
    to: &Ends,
) -> Result<(), String> {
    let news = to.news;
    let mut ws = connect(url, bearer, gate)?;
    tcp_of(&ws)
        .ok_or("speech stream: unknown socket")?
        .set_read_timeout(Some(POLL))
        .map_err(|e| e.to_string())?;
    let opened = Instant::now();
    // The turn's clock starts with its first block sent, not at connect: a
    // listen may sit quiet for a while before anyone talks.
    let mut first_sent: Option<Instant> = None;
    let mut turn = SttTurn::new();
    let mut pending: Vec<Vec<i16>> = Vec::new();
    let (mut ready, mut over, mut finalized) = (false, false, false);
    let mut final_by = None;
    loop {
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
        if over && !ready {
            // The turn is over before the server was ready: nobody waits for
            // these words any more (the turn uploads), so its audio stays here.
            let _ = ws.close(None);
            let _ = ws.flush();
            return Ok(());
        }
        if ready {
            if !pending.is_empty() && offline() {
                let _ = ws.close(None);
                let _ = ws.flush();
                return Err(OFFLINE.into());
            }
            for block in pending.drain(..) {
                ws.send(Message::binary(pcm_bytes(&block)))
                    .map_err(|e| e.to_string())?;
                first_sent.get_or_insert_with(Instant::now);
            }
            if over && !finalized {
                ws.send(Message::text(FINALIZE))
                    .map_err(|e| e.to_string())?;
                finalized = true;
                final_by = Some(Instant::now() + FINAL_WAIT);
            }
        }
        // After the blocks that came in went out: a start just before the
        // idle cap gets its stream.
        match first_sent {
            Some(t) if t.elapsed() > MAX_STREAM => {
                return Err("speech stream ran too long".into());
            }
            None if opened.elapsed() > MAX_IDLE => {
                let _ = ws.close(None);
                let _ = ws.flush();
                let _ = news.send(SttNews::Idle);
                return Ok(());
            }
            _ => {}
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
                to.ready.store(true, Ordering::SeqCst);
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
                let _ = ws.flush();
                return Ok(());
            }
            TurnStep::Nothing => continue,
        };
        if sent.is_err() {
            // Nobody is listening any more.
            let _ = ws.close(None);
            let _ = ws.flush();
            return Ok(());
        }
    }
}

fn pcm_bytes(block: &[i16]) -> Vec<u8> {
    block.iter().flat_map(|s| s.to_le_bytes()).collect()
}

#[cfg(test)]
#[path = "stt_stream_tests.rs"]
mod tests;
