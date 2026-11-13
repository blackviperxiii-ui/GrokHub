use super::*;
use rustls::StreamOwned;
use std::net::TcpListener;

use crate::sandbox::mask::Masks;

const REAL: &str = "ghp_realrealrealrealrealrealreal0123";

fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("gh-mitm-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn read_head<S: Read>(s: &mut S, buf: &mut Vec<u8>) -> Option<String> {
    let mut b = [0u8; 4096];
    loop {
        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            let head = String::from_utf8_lossy(&buf[..i + 4]).into_owned();
            buf.drain(..i + 4);
            return Some(head);
        }
        let n = s.read(&mut b).ok()?;
        if n == 0 {
            return None;
        }
        buf.extend_from_slice(&b[..n]);
    }
}

fn header(head: &str, name: &str) -> Option<String> {
    head.split("\r\n").find_map(|l| {
        let (n, v) = l.split_once(':')?;
        n.trim()
            .eq_ignore_ascii_case(name)
            .then(|| v.trim().to_string())
    })
}

/// A TLS server for `localhost` that answers each request with the
/// Authorization header it got and the body's raw bytes.
fn upstream() -> (u16, CertificateDer<'static>) {
    let ck = rcgen::generate_simple_self_signed(vec!["localhost".to_string()]).unwrap();
    let der = ck.cert.der().clone();
    let cfg = ServerConfig::builder_with_provider(provider())
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(
            vec![der.clone()],
            PrivatePkcs8KeyDer::from(ck.signing_key.serialize_der()).into(),
        )
        .unwrap();
    let cfg = Arc::new(cfg);
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for sock in listener.incoming().flatten() {
            let cfg = Arc::clone(&cfg);
            std::thread::spawn(move || {
                let conn = ServerConnection::new(cfg).unwrap();
                let mut tls = StreamOwned::new(conn, sock);
                let mut buf = Vec::new();
                while let Some(head) = read_head(&mut tls, &mut buf) {
                    let mut body = Vec::new();
                    if let Some(len) = header(&head, "content-length") {
                        let len: usize = len.parse().unwrap();
                        while buf.len() < len {
                            let mut b = [0u8; 4096];
                            let n = tls.read(&mut b).unwrap();
                            buf.extend_from_slice(&b[..n]);
                        }
                        body = buf.drain(..len).collect();
                    } else if header(&head, "transfer-encoding").is_some() {
                        while !buf.windows(5).any(|w| w == b"0\r\n\r\n") {
                            let mut b = [0u8; 4096];
                            let n = tls.read(&mut b).unwrap();
                            buf.extend_from_slice(&b[..n]);
                        }
                        let end = buf.windows(5).position(|w| w == b"0\r\n\r\n").unwrap() + 5;
                        body = buf.drain(..end).collect();
                    }
                    let reply = format!(
                        "auth={};body={}",
                        header(&head, "authorization").unwrap_or_default(),
                        String::from_utf8_lossy(&body)
                    );
                    let out = format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{reply}",
                        reply.len()
                    );
                    if tls.write_all(out.as_bytes()).is_err() {
                        break;
                    }
                    let _ = tls.flush();
                }
            });
        }
    });
    (port, der)
}

fn exchange(tls: &mut StreamOwned<ClientConnection, TcpStream>, request: &str) -> String {
    tls.write_all(request.as_bytes()).unwrap();
    tls.flush().unwrap();
    let mut buf = Vec::new();
    let head = read_head(tls, &mut buf).unwrap();
    let len: usize = header(&head, "content-length").unwrap().parse().unwrap();
    while buf.len() < len {
        let mut b = [0u8; 4096];
        let n = tls.read(&mut b).unwrap();
        buf.extend_from_slice(&b[..n]);
    }
    String::from_utf8(buf).unwrap()
}

fn client_for(
    ca: CertificateDer<'static>,
    sock: TcpStream,
    name: &str,
) -> StreamOwned<ClientConnection, TcpStream> {
    let mut roots = RootCertStore::empty();
    roots.add(ca).unwrap();
    let cfg = ClientConfig::builder_with_provider(provider())
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_root_certificates(roots)
        .with_no_client_auth();
    let conn = ClientConnection::new(
        Arc::new(cfg),
        ServerName::try_from(name.to_string()).unwrap(),
    )
    .unwrap();
    StreamOwned::new(conn, sock)
}

/// Through the terminator: the real token reaches the host in a bearer and a
/// Basic header, over one kept-alive connection with a chunked body, the
/// reply comes back, and a request naming another Host keeps the stand-in.
#[test]
fn the_real_token_reaches_its_host_and_nothing_else_changes() {
    let (port, upstream_root) = upstream();
    let dir = scratch("e2e");
    let system = dir.join("system.pem");
    std::fs::write(&system, "\n").unwrap();
    let mitm = Mitm::new(&["localhost"], &system, Some(upstream_root)).unwrap();
    let masks = Masks::for_test("GH_TOKEN", REAL, &["localhost"]);
    let fake = masks.command_env()[0].1.clone();
    assert_ne!(fake, REAL);

    let front = TcpListener::bind("127.0.0.1:0").unwrap();
    let front_addr = front.local_addr().unwrap();
    let ca = mitm.ca();
    let mitm = Arc::new(mitm);
    let worker = {
        let mitm = Arc::clone(&mitm);
        std::thread::spawn(move || {
            let (client, _) = front.accept().unwrap();
            let server = TcpStream::connect(("127.0.0.1", port)).unwrap();
            mitm.serve(&masks, "localhost", client, &[], server)
        })
    };
    let mut tls = client_for(ca, TcpStream::connect(front_addr).unwrap(), "localhost");

    let r = exchange(
        &mut tls,
        &format!("GET / HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer {fake}\r\n\r\n"),
    );
    assert_eq!(r, format!("auth=Bearer {REAL};body="));

    let engine = base64::engine::general_purpose::STANDARD;
    use base64::Engine as _;
    let basic = engine.encode(format!("x-access-token:{fake}"));
    let r = exchange(
        &mut tls,
        &format!(
            "POST /p HTTP/1.1\r\nHost: localhost:{port}\r\nAuthorization: Basic {basic}\r\n\
             Transfer-Encoding: chunked\r\n\r\n5\r\nhello\r\n0\r\n\r\n"
        ),
    );
    let want = engine.encode(format!("x-access-token:{REAL}"));
    assert_eq!(r, format!("auth=Basic {want};body=5\r\nhello\r\n0\r\n\r\n"));

    // The body is never touched, and a request naming another Host keeps
    // the stand-in.
    let r = exchange(
        &mut tls,
        &format!(
            "POST / HTTP/1.1\r\nHost: localhost\r\nContent-Length: {}\r\n\r\n{fake}",
            fake.len()
        ),
    );
    assert_eq!(r, format!("auth=;body={fake}"));
    let r = exchange(
        &mut tls,
        &format!("GET / HTTP/1.1\r\nHost: evil.test\r\nAuthorization: Bearer {fake}\r\n\r\n"),
    );
    assert_eq!(r, format!("auth=Bearer {fake};body="));

    tls.conn.send_close_notify();
    let _ = tls.flush();
    let _ = tls.sock.shutdown(Shutdown::Write);
    worker.join().unwrap().unwrap();
    assert!(mitm.command_env().iter().any(|(k, v)| *k == "SSL_CERT_FILE"
        && std::fs::read_to_string(v)
            .unwrap()
            .contains("BEGIN CERTIFICATE")));
    let _ = std::fs::remove_dir_all(&dir);
}

/// The session CA is constrained to the token hosts: a certificate it signs
/// for any other name is refused by a client that trusts it.
#[test]
fn the_session_ca_vouches_only_for_token_hosts() {
    let dir = scratch("nc");
    let system = dir.join("system.pem");
    std::fs::write(&system, "\n").unwrap();
    let mitm = Mitm::new(&["localhost"], &system, None).unwrap();
    for (name, ok) in [("localhost", true), ("evil.test", false)] {
        let cfg = mitm.leaf(name).unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            let (sock, _) = listener.accept().unwrap();
            let mut conn = Connection::from(ServerConnection::new(cfg).unwrap());
            let _ = handshake(&mut conn, &sock, &[]);
        });
        let mut tls = client_for(mitm.ca(), TcpStream::connect(addr).unwrap(), name);
        let done = tls.conn.complete_io(&mut tls.sock);
        assert_eq!(done.is_ok(), ok, "{name}: {done:?}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// A real client (curl, on OpenSSL) trusts the bundle, accepts the
/// name-constrained CA, and its stand-in token arrives as the real one.
#[test]
fn curl_trusts_the_bundle_and_its_token_is_put_back() {
    if std::process::Command::new("curl")
        .arg("--version")
        .output()
        .is_err()
    {
        eprintln!("skipped: no curl");
        return;
    }
    let (port, upstream_root) = upstream();
    let dir = scratch("curl");
    let system = dir.join("system.pem");
    std::fs::write(&system, "\n").unwrap();
    let mitm = Arc::new(Mitm::new(&["localhost"], &system, Some(upstream_root)).unwrap());
    let masks = Masks::for_test("GH_TOKEN", REAL, &["localhost"]);
    let fake = masks.command_env()[0].1.clone();
    let front = TcpListener::bind("127.0.0.1:0").unwrap();
    let front_port = front.local_addr().unwrap().port();
    {
        let mitm = Arc::clone(&mitm);
        std::thread::spawn(move || {
            let (client, _) = front.accept().unwrap();
            let server = TcpStream::connect(("127.0.0.1", port)).unwrap();
            let _ = mitm.serve(&masks, "localhost", client, &[], server);
        });
    }
    let bundle = mitm
        .command_env()
        .into_iter()
        .find(|(k, _)| *k == "CURL_CA_BUNDLE")
        .unwrap()
        .1;
    let out = std::process::Command::new("curl")
        .args([
            "-sS",
            "--max-time",
            "20",
            "--noproxy",
            "*",
            "--cacert",
            &bundle,
        ])
        .arg("-H")
        .arg(format!("Authorization: Bearer {fake}"))
        .arg(format!("https://localhost:{front_port}/"))
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    assert_eq!(
        text,
        format!("auth=Bearer {REAL};body="),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let _ = std::fs::remove_dir_all(&dir);
}
