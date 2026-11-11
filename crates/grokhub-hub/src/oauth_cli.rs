//! `grokhub-hub --oauth`: Sign in with Grok from a terminal. It is the same
//! device-code flow the desktop shell runs, and it saves the tokens to the
//! same keychain entry (`shell-xai-oauth`), so the shell is signed in
//! afterwards. Nothing here prints a token.

use grokhub_agent::xai_keychain::{CallError, Poll, SignIn, Store};
use std::io::Write;
use std::time::{Duration, Instant};

/// The most polls, however long the code says it lasts.
const MAX_POLLS: u32 = 180;

/// How many polls in a row may fail for a reason other than a lost connection
/// before giving up.
const MAX_REFUSALS: u32 = 3;

/// Runs the flow against `store`. `wait` sleeps between polls. Returns the
/// exit code: 0 once signed in, 1 on any failure. A poll that gets no answer
/// (no network, a dropped connection, a server error) is asked again while
/// the code is good; why is printed once. The code is good for as long as
/// xAI said it lasts, counted from the waits and the time the polls took.
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
    let mut told: Option<String> = None;
    let mut refusals = 0;
    let lasts = Duration::from_secs(started.expires_in);
    let mut spent = Duration::ZERO;
    for _ in 0..MAX_POLLS {
        if spent >= lasts {
            break;
        }
        let pause = Duration::from_secs(interval);
        wait(pause);
        let asked = Instant::now();
        let got = flow.ask(store);
        spent += pause + asked.elapsed();
        match got {
            Ok(Poll::Pending { interval: next }) => {
                interval = next.max(1);
                refusals = 0;
            }
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
            // `ask` reports a call with no answer as an error; the variant is
            // what the app's own `poll` hands its page.
            Ok(Poll::NoAnswer { error }) => {
                say_once(err, &mut told, error);
                refusals = 0;
            }
            Err(e) => {
                let lost = matches!(e, CallError::NoAnswer(_));
                say_once(err, &mut told, String::from(e));
                refusals = if lost { 0 } else { refusals + 1 };
                if refusals == MAX_REFUSALS {
                    return 1;
                }
            }
        }
    }
    let _ = writeln!(err, "The code ran out. Start again.");
    1
}

/// Prints why a poll failed, once for each run of the same reason.
fn say_once(err: &mut dyn Write, told: &mut Option<String>, why: String) {
    if told.as_deref() != Some(why.as_str()) {
        let _ = writeln!(err, "{why}");
        *told = Some(why);
    }
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

    /// A loopback auth server answering each path once, in order. Code 0
    /// hangs up without an answer, as a dropped connection does.
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
                if code == 0 {
                    let _ = stream.shutdown(std::net::Shutdown::Both);
                } else {
                    let _ = write!(
                        stream,
                        "HTTP/1.1 {code} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{text}",
                        text.len()
                    );
                }
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

    /// The env var that points sign-in at the mock is process-wide: one run
    /// at a time holds this.
    static ENV: Mutex<()> = Mutex::new(());

    /// Runs the flow against a mock that gives `answers`. Returns the exit
    /// code, what was printed, the error lines, the waits and the mock's
    /// address.
    fn run_against(
        name: &str,
        answers: Vec<(&'static str, u16, &'static str)>,
    ) -> (i32, String, String, Vec<u64>, String) {
        let _env = ENV.lock().unwrap_or_else(|e| e.into_inner());
        let dir = std::env::temp_dir().join(format!("hub-oauth-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let _cfg = grokhub_agent::perm::ConfigGuard::set(&dir);
        let base = mock(answers);
        // SAFETY: every test that touches this variable holds `ENV`, and the
        // mock thread does not read the environment.
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
        let _ = std::fs::remove_dir_all(&dir);
        let waits = waits.lock().unwrap().clone();
        (
            code,
            String::from_utf8(out).unwrap(),
            String::from_utf8(err).unwrap(),
            waits,
            base,
        )
    }

    #[test]
    fn a_dropped_poll_is_asked_again_while_the_code_is_good() {
        let (code, out, err, waits, base) = run_against(
            "dropped",
            vec![
                ("/device", 200, START),
                ("/token", 0, ""),
                ("/token", 0, ""),
                ("/token", 400, PENDING),
                ("/token", 200, TOKENS),
                ("/userinfo", 200, USERINFO),
            ],
        );
        assert_eq!(code, 0, "{err}");
        assert_eq!(
            out,
            "Grok OAuth user code: ABCD-EFGH\nhttps://accounts.x.ai/device\nconnected Sam Rivera\n"
        );
        assert_eq!(waits, vec![5, 5, 5, 5]);
        // Two polls were dropped; why is said once.
        assert_eq!(err.lines().count(), 1, "{err}");
        assert!(err.starts_with(&format!("{base}/token: ")), "{err}");
    }

    #[test]
    fn an_answer_that_is_not_json_stops_after_three_polls() {
        let (code, out, err, waits, _base) = run_against(
            "nonsense",
            vec![
                ("/device", 200, START),
                ("/token", 200, "<html>"),
                ("/token", 200, "<html>"),
                ("/token", 200, "<html>"),
                ("/token", 200, TOKENS),
            ],
        );
        assert_eq!(code, 1);
        assert_eq!(
            out,
            "Grok OAuth user code: ABCD-EFGH\nhttps://accounts.x.ai/device\n"
        );
        assert_eq!(waits, vec![5, 5, 5]);
        assert_eq!(err, "expected value at line 1 column 1\n");
    }

    #[test]
    fn a_passing_server_error_is_asked_again_while_the_code_is_good() {
        let (code, out, err, waits, base) = run_against(
            "busy",
            vec![
                ("/device", 200, START),
                ("/token", 503, "<html>Service Unavailable</html>"),
                ("/token", 502, "<html>Bad Gateway</html>"),
                ("/token", 400, PENDING),
                ("/token", 200, TOKENS),
                ("/userinfo", 200, USERINFO),
            ],
        );
        assert_eq!(code, 0, "{err}");
        assert_eq!(
            out,
            "Grok OAuth user code: ABCD-EFGH\nhttps://accounts.x.ai/device\nconnected Sam Rivera\n"
        );
        assert_eq!(waits, vec![5, 5, 5, 5]);
        assert_eq!(
            err,
            format!("{base}/token: status code 503\n{base}/token: status code 502\n")
        );
    }

    #[test]
    fn with_no_answers_it_stops_when_the_code_has_run_out() {
        const SHORT: &str = r#"{"device_code":"dev-1","user_code":"ABCD-EFGH","verification_uri":"https://accounts.x.ai/device","expires_in":20,"interval":5}"#;
        let (code, out, err, waits, base) = run_against(
            "offline",
            vec![
                ("/device", 200, SHORT),
                ("/token", 0, ""),
                ("/token", 0, ""),
                ("/token", 0, ""),
                ("/token", 0, ""),
            ],
        );
        assert_eq!(code, 1);
        assert_eq!(
            out,
            "Grok OAuth user code: ABCD-EFGH\nhttps://accounts.x.ai/device\n"
        );
        // The code lasts 20 seconds: four polls five seconds apart, no more.
        assert_eq!(waits, vec![5, 5, 5, 5]);
        let lines: Vec<&str> = err.lines().collect();
        assert_eq!(lines.len(), 2, "{err}");
        assert!(lines[0].starts_with(&format!("{base}/token: ")), "{err}");
        assert_eq!(lines[1], "The code ran out. Start again.");
    }

    /// The env var and the config dir are process-wide, so the run holds
    /// `ENV`.
    #[test]
    fn a_device_code_run_saves_a_token_the_shell_reads() {
        let _env = ENV.lock().unwrap_or_else(|e| e.into_inner());
        let dir = std::env::temp_dir().join(format!("hub-oauth-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let _cfg = grokhub_agent::perm::ConfigGuard::set(&dir);
        let base = mock(vec![
            ("/device", 200, START),
            ("/token", 400, PENDING),
            ("/token", 200, TOKENS),
            ("/userinfo", 200, USERINFO),
        ]);
        // SAFETY: every test that touches this variable holds `ENV`, and the
        // mock thread does not read the environment.
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
