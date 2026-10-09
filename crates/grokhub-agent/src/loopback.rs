//! The `127.0.0.1` redirect a browser sign-in comes back to (MCP servers,
//! OpenRouter). It answers strays with 404 and reads only `/callback`.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::time::{Duration, Instant};

const READ_CAP: usize = 16 * 1024;

/// A listener on a free loopback port and its `http://127.0.0.1:<port>/callback`.
pub(crate) fn bind() -> Result<(TcpListener, String), String> {
    let listener = TcpListener::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
    listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    Ok((listener, format!("http://127.0.0.1:{port}/callback")))
}

fn close_page() -> Vec<u8> {
    let body = b"<p>Signed in. You can close this tab.</p>";
    let mut out = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )
    .into_bytes();
    out.extend_from_slice(body);
    out
}

/// Wait for `/callback` and hand back its code ([`grokhub_core::pkce::loopback_code`]).
/// Strays (a favicon, a prefetch) get a 404 and the wait goes on.
pub(crate) fn wait_code(listener: &TcpListener, state: &str, who: &str, wait: Duration) -> Result<String, String> {
    let deadline = Instant::now() + wait;
    loop {
        if Instant::now() >= deadline {
            return Err(format!("{who} timed out"));
        }
        let mut stream = match listener.accept() {
            Ok((stream, _)) => stream,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(40));
                continue;
            }
            Err(e) => return Err(e.to_string()),
        };
        let _ = stream.set_nonblocking(false);
        let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
        let mut buf = [0u8; READ_CAP];
        let n = stream.read(&mut buf).unwrap_or(0).min(READ_CAP);
        let head = String::from_utf8_lossy(&buf[..n]);
        if !head.starts_with("GET /callback") {
            let _ = stream.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
            continue;
        }
        let _ = stream.write_all(&close_page());
        return grokhub_core::pkce::loopback_code(&head, state, who);
    }
}
