//! HTTPS for Settings → Web access. The web app and its API are served on the
//! LAN over TLS with a self-signed certificate, so the pairing code and the
//! chats don't cross the network as plain text. This module keeps that
//! certificate and runs the TLS front: each connection is decrypted here and
//! passed to the plain server in `remote.rs`, which listens on loopback only.
//!
//! The certificate is saved under the config dir and reused while it names
//! the same hosts, so a browser that accepted it once doesn't warn again.
//! Settings shows its SHA-256 fingerprint to compare with the browser's.

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls::ServerConfig;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::sync::{oneshot, Semaphore};
use tokio_rustls::TlsAcceptor;

/// Folder under the config dir with `cert.pem`, `key.pem` and `hosts`.
pub const DIR: &str = "web-access";
/// TLS connections at once; past this a new one is closed. The plain server
/// behind caps calls and streams on its own.
const MAX_CONNECTIONS: usize = 64;
/// How many of those one address may hold, so one device can't take them all.
const MAX_PER_PEER: usize = 16;
/// A connection is closed after this long with no whole request waiting and
/// nothing sent back. An open event stream gets a ping every 15 s
/// (`remote.rs`), so it stays.
pub(crate) const IDLE: Duration = Duration::from_secs(60);
/// A client that hasn't finished the handshake by then is dropped.
const HANDSHAKE: Duration = Duration::from_secs(10);

/// The certificate and its key, as PEM.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cert {
    pub cert_pem: String,
    pub key_pem: String,
}

impl Cert {
    /// A new self-signed certificate for `hosts` (names or IP addresses).
    pub fn make(hosts: &[String]) -> Result<Cert, String> {
        let made = rcgen::generate_simple_self_signed(hosts.to_vec())
            .map_err(|e| format!("Couldn't make the web access certificate: {e}"))?;
        Ok(Cert {
            cert_pem: made.cert.pem(),
            key_pem: made.signing_key.serialize_pem(),
        })
    }

    /// The saved certificate when it was made for exactly `hosts`, else a
    /// new one, saved (the key readable by this user only).
    pub fn load_or_make(config_dir: &Path, hosts: &[String]) -> Result<Cert, String> {
        let dir = config_dir.join(DIR);
        let wanted = hosts.join("\n");
        let saved = (|| {
            let names = std::fs::read_to_string(dir.join("hosts")).ok()?;
            let cert = Cert {
                cert_pem: std::fs::read_to_string(dir.join("cert.pem")).ok()?,
                key_pem: std::fs::read_to_string(dir.join("key.pem")).ok()?,
            };
            (names == wanted && cert.der().is_ok()).then_some(cert)
        })();
        if let Some(cert) = saved {
            return Ok(cert);
        }
        let cert = Cert::make(hosts)?;
        let save = |e: std::io::Error| format!("Couldn't save the web access certificate: {e}");
        std::fs::create_dir_all(&dir).map_err(save)?;
        write_private(&dir.join("key.pem"), &cert.key_pem).map_err(save)?;
        std::fs::write(dir.join("cert.pem"), &cert.cert_pem).map_err(save)?;
        std::fs::write(dir.join("hosts"), &wanted).map_err(save)?;
        Ok(cert)
    }

    fn der(&self) -> Result<(CertificateDer<'static>, PrivateKeyDer<'static>), String> {
        let cert = CertificateDer::from_pem_slice(self.cert_pem.as_bytes())
            .map_err(|e| format!("Bad web access certificate: {e}"))?;
        let key = PrivateKeyDer::from_pem_slice(self.key_pem.as_bytes())
            .map_err(|e| format!("Bad web access key: {e}"))?;
        Ok((cert, key))
    }

    /// SHA-256 of the certificate, as browsers show it: `AB:CD:…`.
    pub fn fingerprint(&self) -> Result<String, String> {
        let (cert, _) = self.der()?;
        let hex = grokhub_core::model_download::sha256_hex(cert.as_ref()).to_uppercase();
        Ok(hex
            .as_bytes()
            .chunks(2)
            .map(|pair| String::from_utf8_lossy(pair).into_owned())
            .collect::<Vec<_>>()
            .join(":"))
    }

    fn server_config(&self) -> Result<Arc<ServerConfig>, String> {
        let (cert, key) = self.der()?;
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let config = ServerConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .map_err(|e| e.to_string())?
            .with_no_client_auth()
            .with_single_cert(vec![cert], key)
            .map_err(|e| format!("Bad web access certificate: {e}"))?;
        Ok(Arc::new(config))
    }
}

fn write_private(path: &Path, text: &str) -> std::io::Result<()> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    std::io::Write::write_all(&mut options.open(path)?, text.as_bytes())
}

/// The TLS front, serving until dropped.
pub struct Front {
    pub port: u16,
    stop: Option<oneshot::Sender<()>>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Drop for Front {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// The connections each address has open.
type Peers = Arc<Mutex<HashMap<IpAddr, usize>>>;

/// One address's place among its connections; dropping it gives the place back.
struct PeerSlot {
    peers: Peers,
    ip: IpAddr,
}

impl PeerSlot {
    /// None when `ip` already holds `most`.
    fn take(peers: &Peers, ip: IpAddr, most: usize) -> Option<Self> {
        let mut open = peers.lock().unwrap_or_else(|e| e.into_inner());
        let held = open.get(&ip).copied().unwrap_or(0);
        if held >= most {
            return None;
        }
        open.insert(ip, held + 1);
        Some(Self {
            peers: Arc::clone(peers),
            ip,
        })
    }
}

impl Drop for PeerSlot {
    fn drop(&mut self) {
        let mut open = self.peers.lock().unwrap_or_else(|e| e.into_inner());
        match open.get_mut(&self.ip) {
            Some(held) if *held > 1 => *held -= 1,
            _ => {
                open.remove(&self.ip);
            }
        }
    }
}

/// Writes `bytes` to `out`. False when that failed or took longer than `idle`.
async fn pass(out: &mut (impl AsyncWrite + Unpin), bytes: &[u8], idle: Duration) -> bool {
    let sent = tokio::time::timeout(idle, async {
        out.write_all(bytes).await?;
        out.flush().await
    });
    matches!(sent.await, Ok(Ok(())))
}

/// Counts in `seen` how much of `\r\n\r\n`, the blank line that ends a
/// request head, the browser's bytes have spelled so far. True when `bytes`
/// finish it. `seen` is kept between reads: the four can arrive apart.
fn ends_head(seen: &mut usize, bytes: &[u8]) -> bool {
    for &byte in bytes {
        *seen = match (*seen, byte) {
            (0 | 2, b'\r') | (1 | 3, b'\n') => *seen + 1,
            (_, b'\r') => 1,
            _ => 0,
        };
        if *seen == 4 {
            *seen = 0;
            return true;
        }
    }
    false
}

/// Copies both ways between the browser and the inner server until the
/// server closes, either side fails, or `idle` passed since the server last
/// sent anything. A whole request still waiting for the first byte of its
/// reply is not idle: a chat turn holds its call open and silent until the
/// whole reply is ready. A request whose head never ends is: its bytes are
/// passed on but don't hold the connection open, however slowly they come.
async fn pump<F, B>(front: F, back: B, idle: Duration)
where
    F: AsyncRead + AsyncWrite + Unpin,
    B: AsyncRead + AsyncWrite + Unpin,
{
    let (mut front_in, mut front_out) = tokio::io::split(front);
    let (mut back_in, mut back_out) = tokio::io::split(back);
    let mut up = vec![0u8; 16 * 1024];
    let mut down = vec![0u8; 16 * 1024];
    let mut sending = true;
    let mut awaiting = false;
    let mut head = 0;
    let mut quiet_from = tokio::time::Instant::now();
    loop {
        tokio::select! {
            read = front_in.read(&mut up), if sending => match read {
                // The browser is done sending; its reply may still be coming.
                Ok(0) => {
                    sending = false;
                    if back_out.shutdown().await.is_err() {
                        break;
                    }
                }
                Ok(n) => {
                    if !awaiting && ends_head(&mut head, &up[..n]) {
                        awaiting = true;
                    }
                    if !pass(&mut back_out, &up[..n], idle).await {
                        break;
                    }
                }
                Err(_) => break,
            },
            read = back_in.read(&mut down) => match read {
                Ok(n) if n > 0 => {
                    awaiting = false;
                    head = 0;
                    if !pass(&mut front_out, &down[..n], idle).await {
                        break;
                    }
                    quiet_from = tokio::time::Instant::now();
                }
                _ => break,
            },
            () = tokio::time::sleep_until(quiet_from + idle), if !awaiting => break,
        }
    }
    // Tells the browser the connection ended on purpose.
    let _ = tokio::time::timeout(idle, front_out.shutdown()).await;
}

/// Accepts TLS on `bind` and passes each decrypted connection to `inner`.
/// Binds before it returns, so a port in use is an error here.
pub fn serve(bind: SocketAddr, inner: SocketAddr, cert: &Cert) -> Result<Front, String> {
    serve_with(bind, inner, cert, MAX_PER_PEER, IDLE)
}

/// [`serve`] with its limits given: the connections one address may hold,
/// and how long a connection may sit with nothing sent.
fn serve_with(
    bind: SocketAddr,
    inner: SocketAddr,
    cert: &Cert,
    per_peer: usize,
    idle: Duration,
) -> Result<Front, String> {
    let acceptor = TlsAcceptor::from(cert.server_config()?);
    let listener = std::net::TcpListener::bind(bind)
        .map_err(|e| format!("Web access couldn't start on port {}: {e}", bind.port()))?;
    listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_io()
        .enable_time()
        .build()
        .map_err(|e| e.to_string())?;
    let (stop, mut stopped) = oneshot::channel::<()>();
    let thread = std::thread::Builder::new()
        .name("web-access-tls".into())
        .spawn(move || {
            runtime.block_on(async move {
                let Ok(listener) = tokio::net::TcpListener::from_std(listener) else {
                    return;
                };
                let slots = Arc::new(Semaphore::new(MAX_CONNECTIONS));
                let peers = Peers::default();
                loop {
                    let (sock, peer) = tokio::select! {
                        _ = &mut stopped => return,
                        accepted = listener.accept() => match accepted {
                            Ok(accepted) => accepted,
                            Err(_) => {
                                tokio::time::sleep(Duration::from_millis(100)).await;
                                continue;
                            }
                        },
                    };
                    let Some(place) = PeerSlot::take(&peers, peer.ip(), per_peer) else {
                        continue;
                    };
                    let Ok(slot) = Arc::clone(&slots).try_acquire_owned() else {
                        continue;
                    };
                    let acceptor = acceptor.clone();
                    tokio::spawn(async move {
                        let _held = (slot, place);
                        let Ok(Ok(tls)) =
                            tokio::time::timeout(HANDSHAKE, acceptor.accept(sock)).await
                        else {
                            return;
                        };
                        let Ok(back) = tokio::net::TcpStream::connect(inner).await else {
                            return;
                        };
                        pump(tls, back, idle).await;
                    });
                }
            });
        })
        .map_err(|e| e.to_string())?;
    Ok(Front {
        port,
        stop: Some(stop),
        thread: Some(thread),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::{Ipv4Addr, TcpListener, TcpStream};

    fn temp(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("gh-web-tls-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn hosts(ip: &str) -> Vec<String> {
        vec!["localhost".into(), "127.0.0.1".into(), ip.into()]
    }

    #[test]
    fn the_certificate_is_kept_for_the_same_hosts_and_remade_for_new_ones() {
        let dir = temp("keep");
        let first = Cert::load_or_make(&dir, &hosts("192.168.1.20")).unwrap();
        let again = Cert::load_or_make(&dir, &hosts("192.168.1.20")).unwrap();
        assert_eq!(again, first);
        let moved = Cert::load_or_make(&dir, &hosts("10.0.0.7")).unwrap();
        assert_ne!(moved.fingerprint().unwrap(), first.fingerprint().unwrap());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(dir.join(DIR).join("key.pem"))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_fingerprint_is_the_sha256_of_the_certificate_in_colon_pairs() {
        let cert = Cert::make(&hosts("192.168.1.20")).unwrap();
        let (der, _) = cert.der().unwrap();
        let print = cert.fingerprint().unwrap();
        assert_eq!(print.len(), 32 * 3 - 1);
        assert_eq!(
            print.replace(':', ""),
            grokhub_core::model_download::sha256_hex(der.as_ref()).to_uppercase()
        );
    }

    /// A TLS client that trusts only `cert`, as a browser that accepted it.
    fn client(cert: &Cert) -> Arc<rustls::ClientConfig> {
        let (der, _) = cert.der().unwrap();
        let mut roots = rustls::RootCertStore::empty();
        roots.add(der).unwrap();
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        Arc::new(
            rustls::ClientConfig::builder_with_provider(provider)
                .with_safe_default_protocol_versions()
                .unwrap()
                .with_root_certificates(roots)
                .with_no_client_auth(),
        )
    }

    #[test]
    fn the_front_decrypts_to_the_inner_server_and_stops_on_drop() {
        // The inner server: reads one request line, answers with it.
        let inner = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let inner_addr = inner.local_addr().unwrap();
        std::thread::spawn(move || {
            for sock in inner.incoming() {
                let Ok(mut sock) = sock else { return };
                let mut buf = [0u8; 256];
                let n = sock.read(&mut buf).unwrap_or(0);
                let line = String::from_utf8_lossy(&buf[..n])
                    .lines()
                    .next()
                    .unwrap_or("")
                    .to_string();
                let _ = write!(
                    sock,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{line}",
                    line.len()
                );
            }
        });
        let cert = Cert::make(&hosts("192.168.1.20")).unwrap();
        let front = serve(
            SocketAddr::from((Ipv4Addr::LOCALHOST, 0)),
            inner_addr,
            &cert,
        )
        .unwrap();
        let port = front.port;

        let name = rustls::pki_types::ServerName::try_from("127.0.0.1").unwrap();
        let conn = rustls::ClientConnection::new(client(&cert), name).unwrap();
        let sock = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
        sock.set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        let mut tls = rustls::StreamOwned::new(conn, sock);
        write!(tls, "GET /remote/v1/hello HTTP/1.1\r\nHost: x\r\n\r\n").unwrap();
        let mut reply = Vec::new();
        let _ = tls.read_to_end(&mut reply);
        let reply = String::from_utf8_lossy(&reply);
        assert!(
            reply.ends_with("\r\n\r\nGET /remote/v1/hello HTTP/1.1"),
            "{reply}"
        );

        // Plain HTTP on the TLS port gets no page.
        let mut plain = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
        plain
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        write!(plain, "GET /remote/v1/hello HTTP/1.1\r\nHost: x\r\n\r\n").unwrap();
        let mut got = Vec::new();
        let _ = plain.read_to_end(&mut got);
        assert!(!String::from_utf8_lossy(&got).contains("200 OK"));

        drop(front);
        assert!(TcpStream::connect((Ipv4Addr::LOCALHOST, port)).is_err());
    }

    type Tls = rustls::StreamOwned<rustls::ClientConnection, TcpStream>;

    /// A browser's connection to the front, not yet shaken hands.
    fn connect(port: u16, cert: &Cert) -> Tls {
        let name = rustls::pki_types::ServerName::try_from("127.0.0.1").unwrap();
        let conn = rustls::ClientConnection::new(client(cert), name).unwrap();
        let sock = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
        sock.set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        rustls::StreamOwned::new(conn, sock)
    }

    /// Shakes hands and then says nothing, like a phone that left the Wi-Fi.
    fn silent(port: u16, cert: &Cert) -> Tls {
        let mut tls = connect(port, cert);
        while tls.conn.is_handshaking() {
            tls.conn.complete_io(&mut tls.sock).unwrap();
        }
        tls
    }

    /// Sends one request and reads to the end: what came back, and whether
    /// the front closed the connection itself (no reset, no 10 s timeout).
    fn get(mut tls: Tls) -> (String, bool) {
        let _ = write!(tls, "GET /remote/v1/hello HTTP/1.1\r\nHost: x\r\n\r\n");
        let mut reply = Vec::new();
        let closed = tls.read_to_end(&mut reply).is_ok();
        (String::from_utf8_lossy(&reply).into_owned(), closed)
    }

    /// An inner server that hands each connection to `answer` on its own thread.
    fn inner_server(answer: fn(TcpStream)) -> SocketAddr {
        let inner = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let addr = inner.local_addr().unwrap();
        std::thread::spawn(move || {
            for sock in inner.incoming().flatten() {
                std::thread::spawn(move || answer(sock));
            }
        });
        addr
    }

    fn front(inner: SocketAddr, cert: &Cert, per_peer: usize, idle: Duration) -> Front {
        let bind = SocketAddr::from((Ipv4Addr::LOCALHOST, 0));
        serve_with(bind, inner, cert, per_peer, idle).unwrap()
    }

    #[test]
    fn one_address_holds_only_its_share_and_silent_connections_are_closed() {
        let inner = inner_server(|mut sock| {
            let mut buf = [0u8; 256];
            let n = sock.read(&mut buf).unwrap_or(0);
            let line = String::from_utf8_lossy(&buf[..n])
                .lines()
                .next()
                .unwrap_or("")
                .to_string();
            let _ = write!(
                sock,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{line}",
                line.len()
            );
        });
        let cert = Cert::make(&hosts("192.168.1.20")).unwrap();
        let front = front(inner, &cert, 2, Duration::from_secs(1));

        let mut held = [silent(front.port, &cert), silent(front.port, &cert)];
        // The address has its two; a third is closed unanswered.
        assert!(!get(connect(front.port, &cert)).0.contains("200 OK"));

        // Nothing moves on the two for a second, so the front closes them.
        for tls in &mut held {
            assert_eq!(tls.read(&mut [0u8; 16]).ok(), Some(0));
        }
        let (reply, closed) = get(connect(front.port, &cert));
        assert!(
            reply.ends_with("\r\n\r\nGET /remote/v1/hello HTTP/1.1"),
            "{reply}"
        );
        assert!(closed);
    }

    #[test]
    fn a_kept_alive_connection_carries_one_request_after_another() {
        const REPLY: &[u8] = b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok";
        let inner = inner_server(|mut sock| {
            let mut buf = [0u8; 256];
            while sock.read(&mut buf).is_ok_and(|n| n > 0) {
                if sock.write_all(REPLY).is_err() {
                    return;
                }
            }
        });
        let cert = Cert::make(&hosts("192.168.1.20")).unwrap();
        let front = front(inner, &cert, 2, Duration::from_secs(60));
        let mut tls = connect(front.port, &cert);
        for path in ["/", "/assets/app.js"] {
            write!(tls, "GET {path} HTTP/1.1\r\nHost: x\r\n\r\n").unwrap();
            let mut reply = [0u8; REPLY.len()];
            tls.read_exact(&mut reply).unwrap();
            assert_eq!(reply, REPLY, "{path}");
        }
    }

    #[test]
    fn a_stream_that_keeps_sending_outlives_the_idle_limit() {
        // Twelve lines 50 ms apart: 600 ms in all, with never 500 ms of quiet.
        let inner = inner_server(|mut sock| {
            let _ = sock.read(&mut [0u8; 256]);
            for _ in 0..12 {
                std::thread::sleep(Duration::from_millis(50));
                if sock.write_all(b": ping\n").is_err() {
                    return;
                }
            }
        });
        let cert = Cert::make(&hosts("192.168.1.20")).unwrap();
        let front = front(inner, &cert, 2, Duration::from_millis(500));
        assert_eq!(
            get(connect(front.port, &cert)),
            (": ping\n".repeat(12), true)
        );

        // A connection that goes quiet after its reply began is still closed.
        let inner = inner_server(|mut sock| {
            let _ = sock.read(&mut [0u8; 256]);
            let _ = sock.write_all(b": open\n");
            let _ = sock.read(&mut [0u8; 256]);
        });
        let front = self::front(inner, &cert, 2, Duration::from_millis(500));
        assert_eq!(
            get(connect(front.port, &cert)),
            (": open\n".to_string(), true)
        );
    }

    /// A chat turn holds its call open and silent until the reply is whole.
    #[test]
    fn a_call_waiting_for_its_reply_is_not_closed_as_idle() {
        const REPLY: &str = "HTTP/1.1 200 OK\r\nContent-Length: 4\r\n\r\ndone";
        // The reply comes after three idle limits of silence.
        let inner = inner_server(|mut sock| {
            let _ = sock.read(&mut [0u8; 256]);
            std::thread::sleep(Duration::from_millis(900));
            let _ = sock.write_all(REPLY.as_bytes());
        });
        let cert = Cert::make(&hosts("192.168.1.20")).unwrap();
        let front = front(inner, &cert, 2, Duration::from_millis(300));
        assert_eq!(get(connect(front.port, &cert)), (REPLY.to_string(), true));
    }

    /// One byte, or headers with no blank line after them, is not a call: the
    /// connection is closed like a silent one.
    #[test]
    fn a_half_sent_request_is_closed_as_idle() {
        let inner = inner_server(|mut sock| {
            let mut buf = [0u8; 256];
            while sock.read(&mut buf).is_ok_and(|n| n > 0) {}
        });
        let cert = Cert::make(&hosts("192.168.1.20")).unwrap();
        let front = front(inner, &cert, 2, Duration::from_millis(300));
        for sent in [
            "G",
            "POST /remote/v1/call/get_behavior HTTP/1.1\r\nHost: x\r\n",
        ] {
            let mut tls = connect(front.port, &cert);
            tls.write_all(sent.as_bytes()).unwrap();
            tls.flush().unwrap();
            assert_eq!(tls.read(&mut [0u8; 16]).ok(), Some(0), "{sent:?}");
        }
    }

    /// A byte every 100 ms for three seconds never finishes the request, so
    /// the 300 ms limit still closes the connection.
    #[test]
    fn a_dripped_request_head_does_not_extend_the_idle_limit() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .unwrap();
        let closed = runtime.block_on(async {
            let (mut browser, front) = tokio::io::duplex(1024);
            let (back, _inner) = tokio::io::duplex(1024);
            tokio::spawn(async move {
                for byte in b"GET /remote/v1/hello HTTP/1.1\r\n" {
                    if browser.write_all(&[*byte]).await.is_err() {
                        return;
                    }
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
            });
            let pumped = pump(front, back, Duration::from_millis(300));
            tokio::time::timeout(Duration::from_millis(1500), pumped)
                .await
                .is_ok()
        });
        assert!(closed);
    }

    /// The blank line that ends the request can come in a later read than the
    /// rest; the call still waits out three idle limits for its reply.
    #[test]
    fn a_request_head_split_across_reads_still_waits_for_its_reply() {
        const REPLY: &str = "HTTP/1.1 200 OK\r\nContent-Length: 4\r\n\r\ndone";
        let inner = inner_server(|mut sock| {
            let mut head = Vec::new();
            let mut buf = [0u8; 256];
            while !head.ends_with(b"\r\n\r\n") {
                match sock.read(&mut buf) {
                    Ok(n) if n > 0 => head.extend_from_slice(&buf[..n]),
                    _ => return,
                }
            }
            std::thread::sleep(Duration::from_millis(1500));
            let _ = sock.write_all(REPLY.as_bytes());
        });
        let cert = Cert::make(&hosts("192.168.1.20")).unwrap();
        let front = front(inner, &cert, 2, Duration::from_millis(500));
        let mut tls = connect(front.port, &cert);
        tls.write_all(b"GET /remote/v1/hello HTTP/1.1\r\nHost: x\r\n\r")
            .unwrap();
        tls.flush().unwrap();
        std::thread::sleep(Duration::from_millis(50));
        tls.write_all(b"\n").unwrap();
        let mut reply = Vec::new();
        let closed = tls.read_to_end(&mut reply).is_ok();
        assert_eq!(
            (String::from_utf8_lossy(&reply).into_owned(), closed),
            (REPLY.to_string(), true)
        );
    }
}
