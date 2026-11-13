//! TLS termination in the egress proxy, for credential masking only.
//!
//! When a sandboxed command opens a tunnel to a host one of its masked tokens
//! is for ([`super::mask`]), the proxy answers the TLS handshake itself with a
//! certificate from a session CA, opens its own verified TLS to the real
//! host, and copies requests through, putting the real token back in their
//! headers. Every other tunnel is passed through untouched, and nothing is
//! terminated when no masked token has a host.
//!
//! The CA's key never leaves memory. Its certificate is name-constrained to
//! the hosts tokens go to, so even a copy of it can't vouch for anything
//! else, and the command trusts it through a bundle file (the system roots
//! plus this CA) named by `SSL_CERT_FILE` and the like.
//!
//! Requests are read as HTTP/1.1 (the handshake offers nothing else):
//! `Content-Length` and chunked bodies are copied as they are, and an upgrade
//! hands the rest of the connection over unchanged. A token goes back only
//! when the request's `Host` matches the tunnel's host.

use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::net::{Shutdown, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use rcgen::{
    BasicConstraints, CertificateParams, DnType, ExtendedKeyUsagePurpose, GeneralSubtree, IsCa,
    Issuer, KeyPair, KeyUsagePurpose, NameConstraints,
};
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivatePkcs8KeyDer, ServerName};
use rustls::{
    ClientConfig, ClientConnection, Connection, RootCertStore, ServerConfig, ServerConnection,
};

use super::mask::Masks;

const HEAD_CAP: usize = 64 * 1024;
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(30);

/// Where the system's root bundle usually is.
const SYSTEM_BUNDLES: &[&str] = &[
    "/etc/ssl/certs/ca-certificates.crt",
    "/etc/pki/tls/certs/ca-bundle.crt",
    "/etc/pki/ca-trust/extracted/pem/tls-ca-bundle.pem",
    "/etc/ssl/ca-bundle.pem",
    "/etc/ssl/cert.pem",
];

/// The variables pointing clients at the bundle.
const BUNDLE_VARS: &[&str] = &[
    "SSL_CERT_FILE",
    "CURL_CA_BUNDLE",
    "REQUESTS_CA_BUNDLE",
    "GIT_SSL_CAINFO",
    "NODE_EXTRA_CA_CERTS",
    "PIP_CERT",
    "CARGO_HTTP_CAINFO",
    "AWS_CA_BUNDLE",
    "DENO_CERT",
];

pub struct Mitm {
    issuer: Issuer<'static, KeyPair>,
    ca_der: CertificateDer<'static>,
    leaves: Mutex<HashMap<String, Arc<ServerConfig>>>,
    upstream: Arc<ClientConfig>,
    bundle: PathBuf,
}

fn provider() -> Arc<rustls::crypto::CryptoProvider> {
    Arc::new(rustls::crypto::ring::default_provider())
}

/// This session's terminator, made on first use: `None` when no masked token
/// has a host, or when there's no system bundle to build on (the tokens then
/// stay masked and are never put back).
pub fn session() -> Option<&'static Mitm> {
    static MITM: OnceLock<Option<Mitm>> = OnceLock::new();
    MITM.get_or_init(|| {
        let masks = super::mask::session();
        let hosts = masks.inject_hosts();
        if hosts.is_empty() {
            return None;
        }
        let system = system_bundle()?;
        Mitm::new(&hosts, &system, None).ok()
    })
    .as_ref()
}

fn system_bundle() -> Option<PathBuf> {
    std::env::var_os("SSL_CERT_FILE")
        .map(PathBuf::from)
        .into_iter()
        .chain(SYSTEM_BUNDLES.iter().map(PathBuf::from))
        .find(|p| p.is_file())
}

fn year_now() -> i32 {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    1970 + (secs / 31_556_952) as i32
}

impl Mitm {
    /// A CA constrained to `hosts`, a bundle of `system` plus the CA in a
    /// fresh 0700 folder, and an upstream client that trusts the bundled web
    /// roots and the system's (or `extra_root`, for tests).
    pub fn new(
        hosts: &[&str],
        system: &Path,
        extra_root: Option<CertificateDer<'static>>,
    ) -> Result<Self, String> {
        let err = |e: rcgen::Error| e.to_string();
        let key = KeyPair::generate().map_err(err)?;
        let mut params = CertificateParams::new(Vec::<String>::new()).map_err(err)?;
        params.is_ca = IsCa::Ca(BasicConstraints::Constrained(0));
        params
            .distinguished_name
            .push(DnType::CommonName, "Loadout sandbox session CA");
        params.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
        params.name_constraints = Some(NameConstraints {
            permitted_subtrees: hosts
                .iter()
                .map(|h| GeneralSubtree::DnsName((*h).to_string()))
                .collect(),
            excluded_subtrees: Vec::new(),
        });
        let year = year_now();
        params.not_before = rcgen::date_time_ymd(year - 1, 1, 1);
        params.not_after = rcgen::date_time_ymd(year + 2, 1, 1);
        let cert = params.self_signed(&key).map_err(err)?;
        let ca_pem = cert.pem();
        let ca_der = cert.der().clone();
        let issuer = Issuer::new(params, key);

        let system_pem = std::fs::read(system).map_err(|e| e.to_string())?;
        let mut roots = RootCertStore::empty();
        roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        let (_added, _skipped) = roots.add_parsable_certificates(
            CertificateDer::pem_slice_iter(&system_pem).filter_map(Result::ok),
        );
        if let Some(extra) = extra_root {
            roots.add(extra).map_err(|e| e.to_string())?;
        }
        let mut upstream = ClientConfig::builder_with_provider(provider())
            .with_safe_default_protocol_versions()
            .map_err(|e| e.to_string())?
            .with_root_certificates(roots)
            .with_no_client_auth();
        upstream.alpn_protocols = vec![b"http/1.1".to_vec()];

        let bundle = write_bundle(&system_pem, &ca_pem)?;
        Ok(Self {
            issuer,
            ca_der,
            leaves: Mutex::new(HashMap::new()),
            upstream: Arc::new(upstream),
            bundle,
        })
    }

    /// The variables that make a sandboxed command trust this CA.
    pub fn command_env(&self) -> Vec<(&'static str, String)> {
        let path = self.bundle.to_string_lossy().into_owned();
        BUNDLE_VARS.iter().map(|k| (*k, path.clone())).collect()
    }

    #[cfg(test)]
    pub fn ca(&self) -> CertificateDer<'static> {
        self.ca_der.clone()
    }

    fn leaf(&self, host: &str) -> Result<Arc<ServerConfig>, String> {
        let mut leaves = self.leaves.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(cfg) = leaves.get(host) {
            return Ok(Arc::clone(cfg));
        }
        let err = |e: rcgen::Error| e.to_string();
        let key = KeyPair::generate().map_err(err)?;
        let mut params = CertificateParams::new(vec![host.to_string()]).map_err(err)?;
        params.distinguished_name.push(DnType::CommonName, host);
        params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
        params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
        let year = year_now();
        params.not_before = rcgen::date_time_ymd(year - 1, 1, 1);
        params.not_after = rcgen::date_time_ymd(year + 1, 1, 1);
        let cert = params.signed_by(&key, &self.issuer).map_err(err)?;
        let mut cfg = ServerConfig::builder_with_provider(provider())
            .with_safe_default_protocol_versions()
            .map_err(|e| e.to_string())?
            .with_no_client_auth()
            .with_single_cert(
                vec![cert.der().clone(), self.ca_der.clone()],
                PrivatePkcs8KeyDer::from(key.serialize_der()).into(),
            )
            .map_err(|e| e.to_string())?;
        cfg.alpn_protocols = vec![b"http/1.1".to_vec()];
        let cfg = Arc::new(cfg);
        leaves.insert(host.to_string(), Arc::clone(&cfg));
        Ok(cfg)
    }

    /// Carry one tunnel for `host`: `client` has just been told the tunnel
    /// is up (`early` is whatever it already sent after the CONNECT head),
    /// `server` is connected to the host or through the app's own proxy.
    pub fn serve(
        &self,
        masks: &Masks,
        host: &str,
        client: TcpStream,
        early: &[u8],
        server: TcpStream,
    ) -> Result<(), String> {
        let name = ServerName::try_from(host.to_string()).map_err(|e| e.to_string())?;
        let mut up = Connection::from(
            ClientConnection::new(Arc::clone(&self.upstream), name).map_err(|e| e.to_string())?,
        );
        handshake(&mut up, &server, &[]).map_err(|e| format!("{host}: {e}"))?;
        let mut down =
            Connection::from(ServerConnection::new(self.leaf(host)?).map_err(|e| e.to_string())?);
        handshake(&mut down, &client, early).map_err(|e| format!("client: {e}"))?;

        let down = Half::new(down, client)?;
        let mut up = Half::new(up, server)?;
        let mut down_w = down.twin()?;
        let mut up_r = up.twin()?;
        let replies = std::thread::spawn(move || {
            let mut buf = vec![0u8; 16 * 1024];
            loop {
                match up_r.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        if down_w.write_all(&buf[..n]).is_err() {
                            break;
                        }
                    }
                }
            }
            down_w.close();
        });
        let mut reader = Buffered::new(down);
        let _ = requests(masks, host, &mut reader, &mut up);
        up.close();
        let _ = replies.join();
        Ok(())
    }
}

fn write_bundle(system_pem: &[u8], ca_pem: &str) -> Result<PathBuf, String> {
    let rand = super::mask::session_tag();
    let dir = std::env::temp_dir().join(format!("loadout-ca-{}-{rand}", std::process::id()));
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))
            .map_err(|e| e.to_string())?;
    }
    let path = dir.join("bundle.pem");
    let mut text = system_pem.to_vec();
    if !text.ends_with(b"\n") {
        text.push(b'\n');
    }
    text.extend_from_slice(ca_pem.as_bytes());
    std::fs::write(&path, text).map_err(|e| e.to_string())?;
    Ok(path)
}

/// Run a handshake on a blocking socket, with `early` bytes from the peer
/// fed in first.
fn handshake(conn: &mut Connection, sock: &TcpStream, early: &[u8]) -> io::Result<()> {
    let mut sock = sock;
    let _ = sock.set_read_timeout(Some(HANDSHAKE_TIMEOUT));
    let mut early = early;
    while !early.is_empty() {
        conn.read_tls(&mut early)?;
        conn.process_new_packets().map_err(io::Error::other)?;
    }
    while conn.is_handshaking() {
        conn.complete_io(&mut sock)?;
    }
    while conn.wants_write() {
        conn.write_tls(&mut sock)?;
    }
    let _ = sock.set_read_timeout(None);
    Ok(())
}

/// One direction's handle on a TLS connection shared by two threads: reads
/// and writes each take the lock only while touching the connection, never
/// while waiting on the socket.
struct Half {
    conn: Arc<Mutex<Connection>>,
    sock: TcpStream,
    pending: Vec<u8>,
}

impl Half {
    fn new(conn: Connection, sock: TcpStream) -> Result<Self, String> {
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
            sock,
            pending: Vec::new(),
        })
    }

    fn twin(&self) -> Result<Self, String> {
        Ok(Self {
            conn: Arc::clone(&self.conn),
            sock: self.sock.try_clone().map_err(|e| e.to_string())?,
            pending: Vec::new(),
        })
    }

    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        let mut raw = vec![0u8; 16 * 1024];
        loop {
            {
                let mut conn = lock(&self.conn);
                match conn.reader().read(out) {
                    Ok(n) => return Ok(n),
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => {}
                    Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(0),
                    Err(e) => return Err(e),
                }
                if !self.pending.is_empty() {
                    let mut slice = &self.pending[..];
                    let used = conn.read_tls(&mut slice)?;
                    self.pending.drain(..used);
                    conn.process_new_packets().map_err(io::Error::other)?;
                    while conn.wants_write() {
                        conn.write_tls(&mut self.sock)?;
                    }
                    continue;
                }
            }
            let n = self.sock.read(&mut raw)?;
            if n == 0 {
                let mut conn = lock(&self.conn);
                let _ = conn.read_tls(&mut &[][..]);
                let _ = conn.process_new_packets();
                return match conn.reader().read(out) {
                    Ok(n) => Ok(n),
                    Err(_) => Ok(0),
                };
            }
            self.pending.extend_from_slice(&raw[..n]);
        }
    }

    fn write_all(&mut self, data: &[u8]) -> io::Result<()> {
        let mut conn = lock(&self.conn);
        conn.writer().write_all(data)?;
        while conn.wants_write() {
            conn.write_tls(&mut self.sock)?;
        }
        Ok(())
    }

    fn close(&mut self) {
        let mut conn = lock(&self.conn);
        conn.send_close_notify();
        while conn.wants_write() {
            if conn.write_tls(&mut self.sock).is_err() {
                break;
            }
        }
        let _ = self.sock.shutdown(Shutdown::Write);
    }
}

fn lock(conn: &Mutex<Connection>) -> std::sync::MutexGuard<'_, Connection> {
    conn.lock().unwrap_or_else(|e| e.into_inner())
}

/// Plaintext from the client with lookahead for heads and chunk lines.
struct Buffered {
    half: Half,
    buf: Vec<u8>,
}

impl Buffered {
    fn new(half: Half) -> Self {
        Self {
            half,
            buf: Vec::new(),
        }
    }

    /// More bytes into the buffer; false at the end.
    fn fill(&mut self) -> io::Result<bool> {
        let mut chunk = [0u8; 16 * 1024];
        let n = self.half.read(&mut chunk)?;
        self.buf.extend_from_slice(&chunk[..n]);
        Ok(n > 0)
    }

    /// Up to and including `delim`, or `None` at the end or past `cap`.
    fn until(&mut self, delim: &[u8], cap: usize) -> io::Result<Option<Vec<u8>>> {
        loop {
            if let Some(i) = self.buf.windows(delim.len()).position(|w| w == delim) {
                return Ok(Some(self.buf.drain(..i + delim.len()).collect()));
            }
            if self.buf.len() > cap || !self.fill()? {
                return Ok(None);
            }
        }
    }

    /// Copy exactly `n` bytes to `to`.
    fn copy(&mut self, mut n: u64, to: &mut Half) -> io::Result<()> {
        while n > 0 {
            if self.buf.is_empty() && !self.fill()? {
                return Err(io::ErrorKind::UnexpectedEof.into());
            }
            let take = (n as usize).min(self.buf.len());
            to.write_all(&self.buf[..take])?;
            self.buf.drain(..take);
            n -= take as u64;
        }
        Ok(())
    }

    /// Copy everything left, to the end.
    fn copy_rest(&mut self, to: &mut Half) -> io::Result<()> {
        loop {
            if !self.buf.is_empty() {
                to.write_all(&self.buf)?;
                self.buf.clear();
            }
            if !self.fill()? {
                return Ok(());
            }
        }
    }
}

/// The request loop: each head with tokens put back, then its body as sent.
fn requests(masks: &Masks, host: &str, from: &mut Buffered, to: &mut Half) -> io::Result<()> {
    while let Some(head) = from.until(b"\r\n\r\n", HEAD_CAP)? {
        let head = String::from_utf8_lossy(&head).into_owned();
        let mut lines = head.trim_end_matches("\r\n").split("\r\n");
        let first = lines.next().unwrap_or("");
        let headers: Vec<&str> = lines.collect();
        let value = |name: &str| {
            headers.iter().find_map(|l| {
                let (n, v) = l.split_once(':')?;
                n.trim()
                    .eq_ignore_ascii_case(name)
                    .then(|| v.trim().to_string())
            })
        };
        // A token goes back only to the host the tunnel was opened for.
        let aligned = value("host").is_none_or(|h| {
            let h = h.to_ascii_lowercase();
            let bare = h.rsplit_once(':').map_or(h.as_str(), |(n, p)| {
                if p.chars().all(|c| c.is_ascii_digit()) {
                    n
                } else {
                    h.as_str()
                }
            });
            bare == host
        });
        let mut out = String::with_capacity(head.len());
        out.push_str(first);
        out.push_str("\r\n");
        for line in &headers {
            if aligned {
                out.push_str(&masks.header_line(host, line));
            } else {
                out.push_str(line);
            }
            out.push_str("\r\n");
        }
        out.push_str("\r\n");
        to.write_all(out.as_bytes())?;

        let upgrade = value("upgrade").is_some()
            || first.starts_with("CONNECT ")
            || first.starts_with("PRI ");
        if upgrade {
            return from.copy_rest(to);
        }
        let chunked =
            value("transfer-encoding").is_some_and(|v| v.to_ascii_lowercase().contains("chunked"));
        if chunked {
            loop {
                let Some(line) = from.until(b"\r\n", 4096)? else {
                    return Ok(());
                };
                to.write_all(&line)?;
                let text = String::from_utf8_lossy(&line);
                let size = text.trim().split(';').next().unwrap_or("").trim();
                let size = u64::from_str_radix(size, 16)
                    .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "bad chunk size"))?;
                if size == 0 {
                    // Trailers, then the blank line.
                    while let Some(line) = from.until(b"\r\n", 8192)? {
                        to.write_all(&line)?;
                        if line == b"\r\n" {
                            break;
                        }
                    }
                    break;
                }
                from.copy(size + 2, to)?;
            }
        } else if let Some(len) = value("content-length") {
            let len: u64 = len
                .parse()
                .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "bad length"))?;
            from.copy(len, to)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
