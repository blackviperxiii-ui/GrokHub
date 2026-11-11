//! `grokhub-hub --oauth`: Sign in with Grok from a terminal. It is the same
//! device-code flow the desktop shell runs, and it saves the tokens to the
//! same keychain entry (`shell-xai-oauth`), so the shell is signed in
//! afterwards. Nothing here prints a token.

use grokhub_agent::xai_keychain::{Poll, SignIn, Store};
use std::io::Write;
use std::time::Duration;

/// How many polls to try before giving up (the code lasts ten minutes).
const MAX_POLLS: u32 = 180;

/// Runs the flow against `store`. `wait` sleeps between polls. Returns the
/// exit code: 0 once signed in, 1 on any failure.
pub fn run(
    store: &dyn Store,
    wait: &dyn Fn(Duration),
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> i32 {
    let flow = SignIn::default();
    let started = match flow.start() {
        Ok(s) => s,
        Err(e) => {
            let _ = writeln!(err, "{e}");
            return 1;
        }
    };
    let _ = writeln!(out, "Grok OAuth user code: {}", started.user_code);
    let _ = writeln!(out, "{}", started.verification_uri);
    if let Some(u) = &started.verification_uri_complete {
        let _ = writeln!(out, "{u}");
    }
    let mut interval = started.interval.max(1);
    for _ in 0..MAX_POLLS {
        wait(Duration::from_secs(interval));
        match flow.poll(store) {
            Ok(Poll::Pending { interval: next }) => interval = next.max(1),
            Ok(Poll::Ready { signed_in }) => {
                let who = signed_in
                    .name
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .unwrap_or("grok");
                let _ = writeln!(out, "connected {who}");
                return 0;
            }
            Ok(Poll::Failed { error }) => {
                let _ = writeln!(err, "{error}");
                return 1;
            }
            Err(e) => {
                let _ = writeln!(err, "{e}");
                return 1;
            }
        }
    }
    let _ = writeln!(err, "The code ran out. Start again.");
    1
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Read};
    use std::net::TcpListener;
    use std::sync::Mutex;

    #[derive(Default)]
    struct Mem(Mutex<Option<String>>);
    impl Store for Mem {
        fn get(&self) -> Result<Option<String>, String> {
            Ok(self.0.lock().unwrap().clone())
        }
        fn set(&self, value: &str) -> Result<(), String> {
            *self.0.lock().unwrap() = Some(value.to_string());
            Ok(())
        }
        fn delete(&self) -> Result<(), String> {
            *self.0.lock().unwrap() = None;
            Ok(())
        }
    }

    /// A loopback auth server answering each path once, in order.
    fn mock(mut answers: Vec<(&'static str, u16, &'static str)>) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { break };
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                let path = line.split_whitespace().nth(1).unwrap_or("").to_string();
                let mut len = 0usize;
                loop {
                    let mut h = String::new();
                    reader.read_line(&mut h).unwrap();
                    if h.trim().is_empty() {
                        break;
                    }
                    if let Some(v) = h.to_ascii_lowercase().strip_prefix("content-length:") {
                        len = v.trim().parse().unwrap_or(0);
                    }
                }
                let mut body = vec![0; len];
                reader.read_exact(&mut body).unwrap();
                let (code, text) = match answers.iter().position(|(p, _, _)| *p == path) {
                    Some(i) => {
                        let (_, c, t) = answers.remove(i);
                        (c, t)
                    }
                    None => (404, "{}"),
                };
                let _ = write!(
                    stream,
                    "HTTP/1.1 {code} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{text}",
                    text.len()
                );
                if answers.is_empty() {
                    break;
                }
            }
        });
        base
    }

    const START: &str = r#"{"device_code":"dev-1","user_code":"ABCD-EFGH","verification_uri":"https://accounts.x.ai/device","expires_in":600,"interval":5}"#;
    const PENDING: &str = r#"{"error":"authorization_pending"}"#;
    const TOKENS: &str =
        r#"{"access_token":"at-1","refresh_token":"rt-1","expires_in":3600,"token_type":"Bearer"}"#;
    const USERINFO: &str = r#"{"name":"Sam Rivera","email":"sam@example.com"}"#;

    /// One test owns the env var and the config dir, since both are
    /// process-wide.
    #[test]
    fn a_device_code_run_saves_a_token_the_shell_reads() {
        let dir = std::env::temp_dir().join(format!("hub-oauth-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let _cfg = grokhub_agent::perm::ConfigGuard::set(&dir);
        let base = mock(vec![
            ("/device", 200, START),
            ("/token", 400, PENDING),
            ("/token", 200, TOKENS),
            ("/userinfo", 200, USERINFO),
        ]);
        // SAFETY: this is the only test in this crate that touches the process
        // environment, and nothing else is running a thread that reads it.
        unsafe { std::env::set_var(grokhub_agent::xai_keychain::TEST_URL_ENV, &base) };
        let store = Mem::default();
        let waits = Mutex::new(Vec::new());
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = run(
            &store,
            &|d| waits.lock().unwrap().push(d.as_secs()),
            &mut out,
            &mut err,
        );
        // SAFETY: as above.
        unsafe { std::env::remove_var(grokhub_agent::xai_keychain::TEST_URL_ENV) };
        let out = String::from_utf8(out).unwrap();
        assert_eq!(code, 0, "{}", String::from_utf8_lossy(&err));
        assert_eq!(
            out,
            "Grok OAuth user code: ABCD-EFGH\nhttps://accounts.x.ai/device\nconnected Sam Rivera\n"
        );
        assert_eq!(*waits.lock().unwrap(), vec![5, 5]);
        // The shell reads the same entry through `bearer`.
        assert_eq!(
            grokhub_agent::xai_keychain::bearer(&store),
            Ok(Some("at-1".to_string()))
        );
        assert!(!out.contains("at-1"), "a token must never be printed");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
