//! HTTPS for Settings → Web access. The web app and its API are served on the
//! LAN over TLS with a self-signed certificate, so the pairing code and the
//! chats don't cross the network as plain text. This module keeps that
//! certificate and runs the TLS front: each connection is decrypted here and
//! passed to the plain server in `remote.rs`, which listens on loopback only.
//!
//! The certificate is saved under the config dir and reused while it names
//! the same hosts, so a browser that accepted it once doesn't warn again.
//! Settings shows its SHA-256 fingerprint to compare with the browser's.

use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls::ServerConfig;
use tokio::sync::{oneshot, Semaphore};
use tokio_rustls::TlsAcceptor;

/// Folder under the config dir with `cert.pem`, `key.pem` and `hosts`.
pub const DIR: &str = "web-access";
/// TLS connections at once; past this a new one is closed. The plain server
/// behind caps calls and streams on its own.
const MAX_CONNECTIONS: usize = 64;
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
            key_pem: made.key_pair.serialize_pem(),
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

/// Accepts TLS on `bind` and passes each decrypted connection to `inner`.
/// Binds before it returns, so a port in use is an error here.
pub fn serve(bind: SocketAddr, inner: SocketAddr, cert: &Cert) -> Result<Front, String> {
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
                loop {
                    let sock = tokio::select! {
                        _ = &mut stopped => return,
                        accepted = listener.accept() => match accepted {
                            Ok((sock, _)) => sock,
                            Err(_) => {
                                tokio::time::sleep(Duration::from_millis(100)).await;
                                continue;
                            }
                        },
                    };
                    let Ok(slot) = Arc::clone(&slots).try_acquire_owned() else {
                        continue;
                    };
                    let acceptor = acceptor.clone();
                    tokio::spawn(async move {
                        let _slot = slot;
                        let Ok(Ok(mut tls)) =
                            tokio::time::timeout(HANDSHAKE, acceptor.accept(sock)).await
                        else {
                            return;
                        };
                        let Ok(mut back) = tokio::net::TcpStream::connect(inner).await else {
                            return;
                        };
                        let _ = tokio::io::copy_bidirectional(&mut tls, &mut back).await;
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
}
