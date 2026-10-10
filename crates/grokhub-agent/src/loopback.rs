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

/// The request up to the end of its first line. A client may send it in
/// several writes, so one read can stop at `GET /`.
fn request_line(stream: &mut impl Read) -> String {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    while buf.len() < READ_CAP && !buf.windows(2).any(|w| w == b"\r\n") {
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
        }
    }
    buf.truncate(READ_CAP);
    String::from_utf8_lossy(&buf).into_owned()
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
        let head = request_line(&mut stream);
        if !head.starts_with("GET /callback") {
            let _ = stream.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
            continue;
        }
        let _ = stream.write_all(&close_page());
        return grokhub_core::pkce::loopback_code(&head, state, who);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_callback_sent_in_pieces_is_read_whole() {
        let (listener, redirect) = bind().unwrap();
        let host = redirect.trim_start_matches("http://").trim_end_matches("/callback").to_string();
        let client = std::thread::spawn(move || {
            let mut sock = std::net::TcpStream::connect(&host).unwrap();
            sock.set_nodelay(true).unwrap();
            sock.write_all(b"GET /").unwrap();
            std::thread::sleep(Duration::from_millis(150));
            sock.write_all(b"callback?code=abc HTTP/1.1\r\nHost: x\r\n\r\n").unwrap();
            let mut sink = String::new();
            let _ = sock.read_to_string(&mut sink);
            sink
        });
        assert_eq!(wait_code(&listener, "", "Test sign-in", Duration::from_secs(5)), Ok("abc".to_string()));
        assert!(client.join().unwrap().starts_with("HTTP/1.1 200 OK"));
    }
}
